//! confsync ajanı: trayde durur, kaynakları düzenli aralıklarla denetler,
//! değişiklik bulunca bildirim gönderir.
//!
//! Penceresi yoktur. Arayüz gerektiğinde ayrı süreç olarak başlatılır; böylece
//! ajan hafif kalır (GPU bağlamı, pencere yönetimi yok) ve arayüz çökse bile
//! denetim sürer.
//!
//! Anlık (inotify) izleme bilinçli olarak tercih edilmedi: tam tarama bu ağaç
//! için saniyenin altında sürüyor, dolayısıyla düzenli yoklama aynı sonucu
//! watch yönetimi ve editörlerin `rename` davranışıyla uğraşmadan veriyor.

mod tray;

use anyhow::{Context, Result};
use confsync_core::backup::{self, ChangeReport, NoProgress};
use confsync_core::settings::Settings;
use ksni::blocking::TrayMethods;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use tray::{State, Tray};

/// Tray menüsünden ya da bildirim düğmesinden gelen istekler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    CheckNow,
    BackupNow,
    OpenGui,
    TogglePause,
    Quit,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,zbus=warn,tracing=warn")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "confsync-agent — watches sources and notifies on change\n\n\
             Usage:\n  \
             confsync-agent            run in the tray, check on a schedule\n  \
             confsync-agent --once     run one check and exit (no tray)\n  \
             confsync-agent --backup   run one backup and exit (no tray)\n\n\
             Interval and automatic backup are managed in the app's Settings."
        );
        return Ok(());
    }

    // Tek seferlik kipler: systemd timer ya da elle çalıştırma için.
    if args.iter().any(|a| a == "--once") {
        let settings = Settings::load().context("could not read settings")?;
        let report = backup::detect_changes(&settings, &mut NoProgress)?;
        println!("{} (awaiting decision: {})", report.summary(), report.questions);
        return Ok(());
    }
    if args.iter().any(|a| a == "--backup") {
        let settings = Settings::load().context("could not read settings")?;
        let report = backup::run(&settings, &mut NoProgress)?;
        println!(
            "{} files · commit {}",
            report.stored,
            report.commit_id.as_deref().unwrap_or("(no changes)")
        );
        return Ok(());
    }

    run_agent()
}

fn run_agent() -> Result<()> {
    let (tx, rx) = channel::<Cmd>();
    let handle = Tray::new(tx.clone())
        .spawn()
        .context("could not create tray: does your desktop support StatusNotifier?")?;
    log::info!("agent started, tray icon registered");

    let mut paused = false;
    // İlk denetim hemen yapılır: kullanıcı ajanı açar açmaz durumu görsün.
    let mut next_check = Instant::now();
    // Son bildirilen değişiklik kümesinin parmak izi. Yedeklenmemiş bir
    // dosya her turda yeniden "değişmiş" görünür; kullanıcıyı beş dakikada
    // bir aynı şey için uyarmamak için küme değişmedikçe susulur.
    let mut last_notified: Option<u64> = None;

    loop {
        let timeout = next_check.saturating_duration_since(Instant::now());
        let cmd = match rx.recv_timeout(timeout) {
            Ok(cmd) => Some(cmd),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };

        let settings = match Settings::load() {
            Ok(settings) => settings,
            Err(err) => {
                log::error!("could not read settings: {err:#}");
                set_state(&handle, State::Error { message: "could not read settings".into() });
                next_check = Instant::now() + Duration::from_secs(300);
                continue;
            }
        };
        let interval = Duration::from_secs(settings.agent_interval_min.max(1) * 60);

        match cmd {
            Some(Cmd::Quit) => break,
            Some(Cmd::OpenGui) => {
                if let Err(err) = open_gui() {
                    log::error!("could not open the app: {err:#}");
                }
                continue;
            }
            Some(Cmd::TogglePause) => {
                paused = !paused;
                handle.update(|tray| {
                    tray.paused = paused;
                    tray.state = if paused { State::Paused } else { State::UpToDate };
                });
                next_check = Instant::now() + interval;
                continue;
            }
            Some(Cmd::BackupNow) => {
                run_backup(&handle, &settings);
                // Yedek sonrası küme sıfırlanır: bundan sonraki gerçek
                // değişiklik yeniden bildirilmeli.
                last_notified = None;
                next_check = Instant::now() + interval;
                continue;
            }
            // Kullanıcı elle denetlediyse sonucu görmeyi hak eder; susma
            // kuralı bu turda uygulanmaz.
            Some(Cmd::CheckNow) => last_notified = None,
            None => {
                if paused {
                    next_check = Instant::now() + interval;
                    continue;
                }
            }
        }

        check_cycle(&handle, &settings, &tx, &mut last_notified);
        next_check = Instant::now() + interval;
    }

    log::info!("agent shutting down");
    handle.shutdown().wait();
    Ok(())
}

/// Bir denetim turu: karşılaştır, gerekirse yedekle ya da bildir.
fn check_cycle(
    handle: &ksni::blocking::Handle<Tray>,
    settings: &Settings,
    tx: &Sender<Cmd>,
    last_notified: &mut Option<u64>,
) {
    // Günlük tur her şeyden önce gelir: süresi dolduysa sessizce yedekle ve
    // (uzak tanımlıysa) push et. Bildirim üretmez.
    if backup::daily_due(settings, chrono::Utc::now().timestamp()) {
        run_daily(handle, settings);
        *last_notified = None;
        stamp(handle);
        return;
    }

    set_state(handle, State::Working);

    let report = match backup::detect_changes(settings, &mut NoProgress) {
        Ok(report) => report,
        Err(err) => {
            log::error!("check failed: {err:#}");
            set_state(
                handle,
                State::Error {
                    message: first_line(&format!("{err:#}")),
                },
            );
            stamp(handle);
            return;
        }
    };

    log::info!(
        "check: {} (quiet: {}) · awaiting decision: {}",
        report.summary(),
        report.quiet_count(),
        report.questions
    );

    if report.is_empty() {
        set_state(handle, State::UpToDate);
        *last_notified = None;
        stamp(handle);
        return;
    }

    // Yalnızca sessiz kaynaklarda değişiklik varsa kullanıcı rahatsız
    // edilmez; günlük tur bunları alacak.
    if report.notifiable_count() == 0 {
        set_state(
            handle,
            State::QuietPending {
                summary: report.summary(),
            },
        );
        *last_notified = None;
        stamp(handle);
        return;
    }

    // Tray ikonu her zaman güncel durumu gösterir; susturulan yalnızca
    // açılır bildirimdir.
    let fingerprint = report.notifiable_fingerprint();
    let already_told = *last_notified == Some(fingerprint);
    if already_told {
        log::info!("same change set; notification not repeated");
    } else {
        *last_notified = Some(fingerprint);
    }

    // Karar gerektiren dosya varsa ajan asla kendiliğinden yedeklemez:
    // sır şüpheli bir dosya onay alınmadan depoya (ve uzak depoya) gitmemeli.
    if report.questions > 0 {
        set_state(
            handle,
            State::NeedsReview {
                count: report.questions,
            },
        );
        if !already_told {
            notify_review(&report, tx);
        }
        stamp(handle);
        return;
    }

    if settings.agent_auto_backup {
        run_backup(handle, settings);
        *last_notified = None;
    } else {
        set_state(
            handle,
            State::Changes {
                summary: report.notifiable_summary(),
            },
        );
        if !already_told {
            notify_changes(&report, tx);
        }
    }
    stamp(handle);
}

/// Günlük sessiz tur: yedekler ve uzak tanımlıysa push eder, bildirim yok.
fn run_daily(handle: &ksni::blocking::Handle<Tray>, settings: &Settings) {
    set_state(handle, State::Working);

    // Push bu özelliğin sözünün parçası; `auto_push` ayarından bağımsız
    // olarak, uzak tanımlıysa gönderilir.
    let mut daily = settings.clone();
    daily.auto_push = !settings.remote_url.trim().is_empty();

    match backup::run(&daily, &mut NoProgress) {
        Ok(report) if report.had_changes() => log::info!(
            "daily backup: {} files{}",
            report.stored,
            if report.pushed { ", pushed" } else { "" }
        ),
        Ok(_) => log::info!("daily backup: nothing changed"),
        Err(err) => {
            log::error!("daily backup failed: {err:#}");
            set_state(
                handle,
                State::Error {
                    message: first_line(&format!("{err:#}")),
                },
            );
            return;
        }
    }
    set_state(handle, State::UpToDate);
}

fn run_backup(handle: &ksni::blocking::Handle<Tray>, settings: &Settings) {
    set_state(handle, State::Working);
    match backup::run(settings, &mut NoProgress) {
        Ok(report) if report.had_changes() => {
            log::info!("backed up: {} files", report.stored);
            notify_simple(
                "Backed up",
                &format!(
                    "{} files stored{}",
                    report.stored,
                    if report.pushed {
                        ", pushed to the remote"
                    } else {
                        ""
                    }
                ),
            );
            set_state(handle, State::UpToDate);
        }
        Ok(_) => set_state(handle, State::UpToDate),
        Err(err) => {
            log::error!("backup failed: {err:#}");
            let message = first_line(&format!("{err:#}"));
            notify_simple("Backup failed", &message);
            set_state(handle, State::Error { message });
        }
    }
    stamp(handle);
}

// --- bildirimler ---------------------------------------------------------

fn notify_changes(report: &ChangeReport, tx: &Sender<Cmd>) {
    let body = format!("{}\nClick to back up.", report.summary());
    spawn_action_notification(
        "Configuration changed",
        &body,
        &[("backup", "Back up"), ("open", "Open")],
        tx.clone(),
    );
}

fn notify_review(report: &ChangeReport, tx: &Sender<Cmd>) {
    let body = format!(
        "{} files need your decision (secret suspicion or size limit).\n\
         They will not be backed up until you approve.",
        report.questions
    );
    spawn_action_notification(
        "confsync: your decision is needed",
        &body,
        &[("open", "Review")],
        tx.clone(),
    );
}

fn notify_simple(summary: &str, body: &str) {
    if let Err(err) = notify_rust::Notification::new()
        .appname("confsync")
        .summary(summary)
        .body(body)
        .icon("drive-harddisk")
        .show()
    {
        log::warn!("could not send notification: {err}");
    }
}

/// Düğmeli bildirim gönderir. Düğmeye basılmasını beklemek engelleyici
/// olduğundan ayrı bir iş parçacığında beklenir; kullanıcı hiç basmazsa
/// bildirim kapandığında iş parçacığı da sonlanır.
fn spawn_action_notification(
    summary: &str,
    body: &str,
    actions: &[(&str, &str)],
    tx: Sender<Cmd>,
) {
    let mut notification = notify_rust::Notification::new();
    notification
        .appname("confsync")
        .summary(summary)
        .body(body)
        .icon("drive-harddisk");
    for (id, label) in actions {
        notification.action(id, label);
    }

    let handle = match notification.show() {
        Ok(handle) => handle,
        Err(err) => {
            log::warn!("could not send notification: {err}");
            return;
        }
    };

    std::thread::spawn(move || {
        handle.wait_for_action(|action| match action {
            "backup" => {
                let _ = tx.send(Cmd::BackupNow);
            }
            "open" | "default" => {
                let _ = tx.send(Cmd::OpenGui);
            }
            _ => {}
        });
    });
}

// --- yardımcılar ---------------------------------------------------------

/// Arayüzü ayrı süreç olarak başlatır. İkili dosya ajanla aynı dizinde
/// aranır; kurulu değilse `PATH` denenir.
fn open_gui() -> Result<()> {
    let candidate = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("confsync")))
        .filter(|p| p.exists());

    let program = candidate.unwrap_or_else(|| "confsync".into());
    std::process::Command::new(&program)
        .spawn()
        .with_context(|| format!("could not start the app: {}", program.display()))?;
    Ok(())
}

fn set_state(handle: &ksni::blocking::Handle<Tray>, state: State) {
    handle.update(|tray| tray.state = state.clone());
}

fn stamp(handle: &ksni::blocking::Handle<Tray>) {
    let now = chrono::Local::now().format("%H:%M").to_string();
    handle.update(|tray| tray.last_check = now.clone());
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or(text).to_string()
}
