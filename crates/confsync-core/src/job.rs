//! Arka plan işçisi.
//!
//! GUI ana iş parçacığı hiçbir zaman dosya sistemi ya da ağ işlemi yapmaz;
//! komutlar kanal üzerinden işçiye gönderilir, ilerleme olayları geri gelir.

use crate::backup::{self, BackupPlan, BackupReport, ChangeReport, Progress};
use crate::discover::{self, Candidate};
use crate::gitrepo::{self, CommitInfo, PullOutcome};
use crate::restore::{self, RestorePlan, RestoreReport};
use crate::settings::Settings;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

pub enum Command {
    /// Yalnızca tarar; ne alınacağını/sorulacağını döndürür, dosya değiştirmez.
    PlanBackup(Settings),
    /// Kullanıcı kararları verilmiş planı uygular.
    ApplyBackup {
        settings: Settings,
        plan: Box<BackupPlan>,
    },
    PlanRestore { settings: Settings, target_home: PathBuf },
    ApplyRestore { plan: Box<RestorePlan>, make_rollback: bool },
    Push(Settings),
    Pull(Settings),
    LoadHistory(Settings),
    /// `~/.config` içeriğini ölçüp sınıflandırır (bkz. [`crate::discover`]).
    Discover { home: PathBuf },
    /// Verilen yolların diskteki boyutunu ölçer.
    /// Arka plan işidir: arayüzü meşgul duruma sokmaz, [`Event::Idle`] üretmez.
    MeasureSizes { paths: Vec<PathBuf> },
    /// Kaynakları son yedekle karşılaştırır. Arka plan işidir.
    DetectChanges(Settings),
    Shutdown,
}

impl Command {
    /// Arayüzün "meşgul" göstergesini tetikleyen komutlar.
    fn is_foreground(&self) -> bool {
        !matches!(
            self,
            Command::MeasureSizes { .. } | Command::DetectChanges(_)
        )
    }
}

pub enum Event {
    Stage(String),
    FileProgress { path: PathBuf, index: usize, total: usize },
    BackupPlanReady(Box<BackupPlan>),
    BackupDone(Box<BackupReport>),
    RestorePlanReady(Box<RestorePlan>),
    RestoreDone(Box<RestoreReport>),
    Pushed,
    Pulled(PullOutcome),
    History(Vec<CommitInfo>),
    Discovered(Vec<Candidate>),
    /// Ölçülen yol → bayt eşleşmeleri.
    Sizes(Vec<(PathBuf, u64)>),
    ChangesReady(Box<ChangeReport>),
    Failed(String),
    Idle,
}

pub struct Worker {
    tx: Sender<Command>,
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    /// `wake` her olay üretildiğinde çağrılır; egui'de `Context::request_repaint`.
    pub fn spawn(wake: impl Fn() + Send + 'static) -> Self {
        let (cmd_tx, cmd_rx) = channel::<Command>();
        let (ev_tx, ev_rx) = channel::<Event>();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();

        let handle = std::thread::Builder::new()
            .name("confsync-worker".into())
            .spawn(move || {
                for command in cmd_rx {
                    if matches!(command, Command::Shutdown) {
                        break;
                    }
                    worker_cancel.store(false, Ordering::SeqCst);
                    let emitter = Emitter { tx: ev_tx.clone(), wake: &wake };
                    let foreground = command.is_foreground();
                    handle_command(command, &emitter, &worker_cancel);
                    // Arka plan işleri "meşgul" durumunu sonlandırmamalı;
                    // aksi halde sıradaki gerçek işin göstergesi silinirdi.
                    if foreground {
                        emitter.send(Event::Idle);
                    }
                }
            })
            .expect("could not start the worker thread");

        Self { tx: cmd_tx, rx: ev_rx, cancel, handle: Some(handle) }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }

    /// Biriken olayları toplar; GUI her karede çağırır.
    pub fn poll(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct Emitter<'a> {
    tx: Sender<Event>,
    wake: &'a (dyn Fn() + Send),
}

impl Emitter<'_> {
    fn send(&self, event: Event) {
        let _ = self.tx.send(event);
        (self.wake)();
    }
}

struct EmitterProgress<'a> {
    emitter: &'a Emitter<'a>,
    cancel: &'a AtomicBool,
}

impl Progress for EmitterProgress<'_> {
    fn stage(&mut self, name: &str) {
        self.emitter.send(Event::Stage(name.to_string()));
    }

    fn file(&mut self, path: &Path, index: usize, total: usize) -> bool {
        // Her dosyada olay göndermek arayüzü boğar; seyreltiyoruz.
        if index % 25 == 0 || index == total {
            self.emitter.send(Event::FileProgress {
                path: path.to_path_buf(),
                index,
                total,
            });
        }
        !self.cancel.load(Ordering::SeqCst)
    }
}

fn handle_command(command: Command, emitter: &Emitter, cancel: &AtomicBool) {
    let result: anyhow::Result<()> = (|| {
        match command {
            Command::PlanBackup(settings) => {
                let mut progress = EmitterProgress { emitter, cancel };
                let plan = backup::plan(&settings, &mut progress)?;
                emitter.send(Event::BackupPlanReady(Box::new(plan)));
            }
            Command::ApplyBackup { settings, plan } => {
                let mut progress = EmitterProgress { emitter, cancel };
                let report = backup::apply(&settings, *plan, &mut progress)?;
                emitter.send(Event::BackupDone(Box::new(report)));
            }
            Command::PlanRestore { settings, target_home } => {
                emitter.send(Event::Stage("Building restore plan".into()));
                let plan = restore::plan(&settings, &target_home)?;
                emitter.send(Event::RestorePlanReady(Box::new(plan)));
            }
            Command::ApplyRestore { plan, make_rollback } => {
                emitter.send(Event::Stage("Restoring files".into()));
                let report = restore::apply(&plan, make_rollback, |path, index, total| {
                    if index % 10 == 0 || index == total {
                        emitter.send(Event::FileProgress {
                            path: path.to_path_buf(),
                            index,
                            total,
                        });
                    }
                    !cancel.load(Ordering::SeqCst)
                })?;
                emitter.send(Event::RestoreDone(Box::new(report)));
            }
            Command::Push(settings) => {
                emitter.send(Event::Stage("Pushing to the remote".into()));
                let repo = gitrepo::open_or_init(&settings.repo_path, &settings.branch)?;
                gitrepo::set_remote(&repo, &settings.remote_url)?;
                gitrepo::push(&repo, &settings.branch)?;
                emitter.send(Event::Pushed);
            }
            Command::Pull(settings) => {
                emitter.send(Event::Stage("Fetching from the remote".into()));
                let repo = gitrepo::open_or_init(&settings.repo_path, &settings.branch)?;
                gitrepo::set_remote(&repo, &settings.remote_url)?;
                let outcome = gitrepo::pull_fast_forward(&repo, &settings.branch)?;
                emitter.send(Event::Pulled(outcome));
            }
            Command::LoadHistory(settings) => {
                let repo = gitrepo::open_or_init(&settings.repo_path, &settings.branch)?;
                let commits = gitrepo::log(&repo, 100)?;
                emitter.send(Event::History(commits));
            }
            Command::Discover { home } => {
                emitter.send(Event::Stage("Inspecting ~/.config".into()));
                let mut index = 0usize;
                let candidates = discover::config_candidates(&home, |path| {
                    index += 1;
                    emitter.send(Event::FileProgress {
                        path: path.to_path_buf(),
                        index,
                        // Toplam önceden bilinmiyor; ilerleme çubuğu yerine
                        // yalnızca hangi girdinin ölçüldüğü gösterilir.
                        total: 0,
                    });
                    !cancel.load(Ordering::SeqCst)
                });
                emitter.send(Event::Discovered(candidates));
            }
            Command::MeasureSizes { paths } => {
                let mut sizes = Vec::with_capacity(paths.len());
                for path in paths {
                    if cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    // Okunamayan/olmayan yol da sonuca girer; aksi halde
                    // arayüz ölçümü sonsuza dek yeniden isterdi.
                    let (size, _, _) = discover::measure_path(&path);
                    sizes.push((path, size));
                }
                emitter.send(Event::Sizes(sizes));
            }
            Command::DetectChanges(settings) => {
                // Arka plan işi: ilerleme olayı üretmez ki durum çubuğundaki
                // gerçek işin göstergesini bozmasın.
                let report = backup::detect_changes(&settings, &mut backup::NoProgress)?;
                emitter.send(Event::ChangesReady(Box::new(report)));
            }
            Command::Shutdown => {}
        }
        Ok(())
    })();

    if let Err(err) = result {
        emitter.send(Event::Failed(format!("{err:#}")));
    }
}
