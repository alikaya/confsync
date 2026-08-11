mod theme;
mod views;

use confsync_core::backup::{BackupPlan, BackupReport, ChangeReport};
use confsync_core::discover::Candidate;
use confsync_core::gitrepo::{CommitInfo, PullOutcome};
use confsync_core::job::{Command, Event, Worker};
use confsync_core::restore::RestorePlan;
use confsync_core::scan::SkipReason;
use confsync_core::settings::{self, Settings};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Tab {
    Overview,
    Sources,
    Excludes,
    Restore,
    History,
    Settings,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::Overview,
        Tab::Sources,
        Tab::Excludes,
        Tab::Restore,
        Tab::History,
        Tab::Settings,
    ];

    fn label(&self) -> &'static str {
        match self {
            Tab::Overview => "Genel Bakış",
            Tab::Sources => "Kaynaklar",
            Tab::Excludes => "Hariç Tutulanlar",
            Tab::Restore => "Geri Yükle",
            Tab::History => "Geçmiş",
            Tab::Settings => "Ayarlar",
        }
    }

    /// Kenar çubuğu ikonu. egui'nin gömülü ikon fontunda bulunan
    /// karakterlerden seçilir; ek font yüklenmez.
    fn icon(&self) -> &'static str {
        match self {
            Tab::Overview => "🏠",
            Tab::Sources => "📁",
            Tab::Excludes => "🚫",
            Tab::Restore => "📥",
            Tab::History => "🕘",
            Tab::Settings => "⚙",
        }
    }
}

pub struct App {
    pub settings: Settings,
    pub worker: Worker,
    pub tab: Tab,

    pub busy: bool,
    pub stage: String,
    pub progress: Option<(usize, usize)>,
    pub current_file: String,

    pub log: Vec<(LogLevel, String)>,
    pub last_backup: Option<BackupReport>,
    pub plan: Option<RestorePlan>,
    pub history: Vec<CommitInfo>,

    /// Hariç tutma kalıpları çok satırlı bir metin kutusunda düzenlenir.
    pub excludes_text: String,
    pub new_source_input: String,
    pub restore_target_home: PathBuf,
    pub make_rollback: bool,
    pub settings_dirty: bool,
    pub confirm_restore: bool,

    /// `~/.config` keşfinin sonucu; boşken panel "tara" düğmesi gösterir.
    pub discovery: Vec<Candidate>,
    /// Ağır olarak işaretlenmiş adaylar da listelensin mi.
    pub show_heavy: bool,

    /// Ölçülmüş yol boyutları (kaynak satırları ve toplamlar için).
    pub sizes: HashMap<PathBuf, u64>,
    /// Ölçüm işi sırada/çalışıyor mu; aynı isteği tekrar göndermemek için.
    pub measuring: bool,

    /// Onay bekleyen yedekleme planı; doluyken onay penceresi açıktır.
    pub review: Option<BackupPlan>,
    /// Onay penceresindeki kararlar ayarlara yazılsın mı.
    pub remember_decisions: bool,
    /// Onay penceresinde "dahil edilecekler" listesi açık mı.
    pub show_included: bool,
    /// Bir komut bitmeden ardına yeni komut zincirlendi; `Idle` meşgul
    /// durumunu sonlandırmamalı.
    chained: bool,

    /// Son yedeğe göre değişen dosyalar; Genel Bakış'taki tablo bunu basar.
    pub changes: Option<ChangeReport>,
    /// Karşılaştırma arka planda sürüyor mu.
    pub detecting: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        // İşçi bir olay ürettiğinde arayüzü yeniden çizdir.
        let worker = Worker::spawn(move || ctx.request_repaint());

        let (settings, mut log) = match Settings::load() {
            Ok(s) => (s, Vec::new()),
            Err(err) => (
                Settings::default(),
                vec![(
                    LogLevel::Warn,
                    format!("Ayarlar okunamadı, varsayılanlar kullanılıyor: {err:#}"),
                )],
            ),
        };
        log.push((
            LogLevel::Info,
            format!("Profil: {} · Depo: {}", settings.profile, settings.repo_path.display()),
        ));

        let excludes_text = settings.excludes.join("\n");
        worker.send(Command::LoadHistory(settings.clone()));
        // Açılışta bir kez karşılaştır: kullanıcı Genel Bakış'ı açar açmaz
        // neyin değiştiğini görsün.
        worker.send(Command::DetectChanges(settings.clone()));

        Self {
            restore_target_home: settings::home_dir(),
            settings,
            worker,
            tab: Tab::Overview,
            busy: false,
            stage: String::new(),
            progress: None,
            current_file: String::new(),
            log,
            last_backup: None,
            plan: None,
            history: Vec::new(),
            excludes_text,
            new_source_input: String::new(),
            make_rollback: true,
            settings_dirty: false,
            confirm_restore: false,
            discovery: Vec::new(),
            show_heavy: false,
            sizes: HashMap::new(),
            measuring: false,
            review: None,
            remember_decisions: true,
            show_included: false,
            chained: false,
            changes: None,
            detecting: false,
        }
    }

    /// Henüz ölçülmemiş yolları arka planda ölçtürür.
    /// Görünümler her karede çağırabilir; tekrarlı istek göndermez.
    pub fn ensure_sizes(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        if self.measuring {
            return;
        }
        let missing: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| !self.sizes.contains_key(p))
            .collect();
        if missing.is_empty() {
            return;
        }
        self.measuring = true;
        self.worker.send(Command::MeasureSizes { paths: missing });
    }

    /// Bir yolun ölçümünü geçersiz kılar (yedek sonrası depo boyutu gibi).
    pub fn invalidate_size(&mut self, path: &Path) {
        self.sizes.remove(path);
    }

    /// Etkin kaynakların ölçülmüş toplamı ve hepsinin ölçülüp ölçülmediği.
    pub fn enabled_sources_total(&self) -> (u64, bool) {
        let mut total = 0;
        let mut complete = true;
        for source in self.settings.enabled_sources() {
            match self.sizes.get(&source.path) {
                Some(bytes) => total += bytes,
                None => complete = false,
            }
        }
        (total, complete)
    }

    /// Değişen dosya tablosunu tazeler. Arka plan işidir; arayüzü kilitlemez.
    pub fn start_detect_changes(&mut self) {
        if self.detecting {
            return;
        }
        self.detecting = true;
        self.worker.send(Command::DetectChanges(self.settings.clone()));
    }

    pub fn start_discovery(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.discovery.clear();
        self.worker.send(Command::Discover {
            home: settings::home_dir(),
        });
    }

    pub fn info(&mut self, msg: impl Into<String>) {
        self.log.push((LogLevel::Info, msg.into()));
    }

    pub fn warn(&mut self, msg: impl Into<String>) {
        self.log.push((LogLevel::Warn, msg.into()));
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.log.push((LogLevel::Error, msg.into()));
    }

    /// Metin kutusundaki kalıpları ayarlara aktarır.
    pub fn sync_excludes_from_text(&mut self) {
        self.settings.excludes = self
            .excludes_text
            .lines()
            .map(|l| l.to_string())
            .collect();
        self.settings_dirty = true;
    }

    pub fn save_settings(&mut self) {
        match self.settings.save() {
            Ok(()) => {
                self.settings_dirty = false;
                self.info("Ayarlar kaydedildi.");
            }
            Err(err) => self.error(format!("Ayarlar kaydedilemedi: {err:#}")),
        }
    }

    /// Yedeklemenin ilk aşaması: yalnızca tarama. Sonuç geldiğinde ya onay
    /// penceresi açılır ya da doğrudan uygulanır.
    pub fn start_backup(&mut self) {
        if self.busy || self.review.is_some() {
            return;
        }
        self.sync_excludes_from_text();
        self.busy = true;
        self.stage = "Başlatılıyor".into();
        self.progress = None;
        self.worker.send(Command::PlanBackup(self.settings.clone()));
    }

    /// Onaylanan planı uygular; istenmişse kararları ayarlara yazar.
    pub fn apply_review(&mut self) {
        let Some(plan) = self.review.take() else {
            return;
        };

        if self.remember_decisions {
            let decided: Vec<(PathBuf, bool)> = plan
                .entries
                .iter()
                .filter(|e| e.question.is_some())
                .map(|e| (e.item.path.clone(), e.include))
                .collect();
            for (path, include) in &decided {
                self.settings.remember_decision(path, *include);
            }
            if !decided.is_empty() {
                self.save_settings();
                self.info(format!(
                    "{} dosya için karar hatırlanacak; bir daha sorulmayacak.",
                    decided.len()
                ));
            }
        }

        self.busy = true;
        self.stage = "Başlatılıyor".into();
        self.progress = None;
        self.worker.send(Command::ApplyBackup {
            settings: self.settings.clone(),
            plan: Box::new(plan),
        });
    }

    pub fn cancel_review(&mut self) {
        self.review = None;
        self.info("Yedekleme iptal edildi; hiçbir dosya değişmedi.");
    }

    pub fn start_plan_restore(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.plan = None;
        self.confirm_restore = false;
        self.worker.send(Command::PlanRestore {
            settings: self.settings.clone(),
            target_home: self.restore_target_home.clone(),
        });
    }

    fn drain_events(&mut self) {
        for event in self.worker.poll() {
            match event {
                Event::Stage(name) => {
                    self.stage = name;
                    self.progress = None;
                }
                Event::FileProgress { path, index, total } => {
                    self.progress = Some((index, total));
                    self.current_file = path.display().to_string();
                }
                Event::BackupPlanReady(plan) => {
                    // Sorulacak bir şey yoksa kullanıcıyı durdurmanın anlamı
                    // yok; doğrudan uygulanır (ayarlarda aksi istenmedikçe).
                    if plan.needs_review() || self.settings.always_ask {
                        self.info(format!(
                            "Tarama bitti: {} dosya yedeğe girecek, {} dosya için kararınız gerekiyor.",
                            plan.included_count(),
                            plan.question_count()
                        ));
                        self.show_included = false;
                        self.review = Some(*plan);
                    } else {
                        self.chained = true;
                        self.worker.send(Command::ApplyBackup {
                            settings: self.settings.clone(),
                            plan,
                        });
                    }
                }
                Event::BackupDone(report) => {
                    let secrets = report
                        .scan
                        .skipped
                        .iter()
                        .filter(|s| s.reason == SkipReason::Secret)
                        .count();
                    if let Some(id) = &report.commit_id {
                        let short = &id[..7.min(id.len())];
                        self.info(format!(
                            "Yedek tamamlandı: {} dosya, commit {}",
                            report.stored, short
                        ));
                    } else {
                        self.info("Değişiklik yok; yeni commit oluşturulmadı.");
                    }
                    if secrets > 0 {
                        self.warn(format!(
                            "{secrets} dosya sır içerdiği için dışarıda bırakıldı."
                        ));
                    }
                    if report.pushed {
                        self.info("Uzak depoya gönderildi.");
                    }
                    self.last_backup = Some(*report);
                    // Depo büyüdü; Genel Bakış'taki boyut yeniden ölçülsün.
                    let repo = self.settings.repo_path.clone();
                    self.invalidate_size(&repo);
                    self.worker.send(Command::LoadHistory(self.settings.clone()));
                    self.start_detect_changes();
                }
                Event::RestorePlanReady(plan) => {
                    self.info(format!(
                        "Plan hazır: {} madde incelendi.",
                        plan.items.len()
                    ));
                    self.plan = Some(*plan);
                    self.tab = Tab::Restore;
                }
                Event::RestoreDone(report) => {
                    self.info(format!("{} dosya geri yüklendi.", report.written));
                    if let Some(dir) = &report.rollback_dir {
                        self.info(format!("Geri alma kopyası: {}", dir.display()));
                    }
                    for (path, err) in &report.failed {
                        self.error(format!("{}: {}", path.display(), err));
                    }
                    self.plan = None;
                    self.confirm_restore = false;
                }
                Event::Pushed => self.info("Push tamamlandı."),
                Event::Pulled(outcome) => match outcome {
                    PullOutcome::UpToDate => self.info("Zaten güncel."),
                    PullOutcome::FastForwarded => {
                        self.info("Uzak depodaki değişiklikler alındı.")
                    }
                },
                Event::History(commits) => self.history = commits,
                Event::Sizes(sizes) => {
                    self.sizes.extend(sizes);
                    self.measuring = false;
                }
                Event::ChangesReady(report) => {
                    self.changes = Some(*report);
                    self.detecting = false;
                }
                Event::Discovered(candidates) => {
                    use confsync_core::discover::Verdict;
                    let recommended = candidates
                        .iter()
                        .filter(|c| c.verdict == Verdict::Recommended)
                        .count();
                    let heavy = candidates
                        .iter()
                        .filter(|c| c.verdict == Verdict::Heavy)
                        .count();
                    self.info(format!(
                        "~/.config incelendi: {} girdi · {recommended} önerilen · \
                         {heavy} ağır (yedeğe alınması önerilmez).",
                        candidates.len()
                    ));
                    self.discovery = candidates;
                }
                Event::Failed(err) => {
                    self.error(err);
                    self.busy = false;
                    // Arka plan işleri de bu olayla düşer; bayrakları bırak.
                    self.detecting = false;
                    self.measuring = false;
                }
                Event::Idle => {
                    // Plan → uygula zincirinde ara `Idle` göstergeyi silmemeli.
                    if self.chained {
                        self.chained = false;
                    } else {
                        self.busy = false;
                        self.stage.clear();
                        self.progress = None;
                        self.current_file.clear();
                    }
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();

        egui::TopBottomPanel::top("baslik")
            .frame(theme::bar(egui::Margin {
                left: 18,
                right: 18,
                top: 12,
                bottom: 0,
            }))
            .show(ctx, |ui| {
                views::header(self, ui);
                ui.add_space(12.0);
                theme::hairline(ui);
            });

        egui::TopBottomPanel::bottom("durum")
            .frame(theme::bar(egui::Margin {
                left: 18,
                right: 18,
                top: 0,
                bottom: 8,
            }))
            .show(ctx, |ui| {
                theme::hairline(ui);
                ui.add_space(8.0);
                views::status_bar(self, ui);
            });

        egui::SidePanel::left("sekmeler")
            .exact_width(208.0)
            .resizable(false)
            .frame(theme::bar(egui::Margin {
                left: 10,
                right: 10,
                top: 14,
                bottom: 10,
            }))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for tab in Tab::ALL {
                    if theme::nav_item(ui, tab.icon(), tab.label(), self.tab == tab).clicked() {
                        self.tab = tab;
                    }
                }

                // Kaydedilmemiş ayar varsa kenar çubuğunun altında hatırlat.
                if self.settings_dirty {
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        theme::notice(ui, theme::WARN, |ui| {
                            ui.label(
                                egui::RichText::new("Kaydedilmemiş değişiklik var")
                                    .size(12.0)
                                    .color(theme::WARN),
                            );
                            if ui.add(theme::ghost_button("Kaydet")).clicked() {
                                self.save_settings();
                            }
                        });
                    });
                }
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(24, 20)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.tab {
                        Tab::Overview => views::overview(self, ui),
                        Tab::Sources => views::sources(self, ui),
                        Tab::Excludes => views::excludes(self, ui),
                        Tab::Restore => views::restore(self, ui),
                        Tab::History => views::history(self, ui),
                        Tab::Settings => views::settings(self, ui),
                    });
            });

        // Onay penceresi en üstte; açıkken arkadaki içerik değişmez.
        views::review_window(self, ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.settings_dirty {
            let _ = self.settings.save();
        }
    }
}
