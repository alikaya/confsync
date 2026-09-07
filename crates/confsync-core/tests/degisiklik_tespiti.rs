//! `backup::detect_changes` ajanın tek karar mekanizması: yanlış "değişiklik
//! yok" derse yedek sessizce eskir, yanlış "değişti" derse bildirim yağar.

use confsync_core::backup::{self, ChangeKind, ChangeReport, NoProgress};
use confsync_core::settings::{Settings, Source};
use std::fs;

#[test]
fn ekleme_degistirme_silme_yakalanir() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("kaynak");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.conf"), "bir").unwrap();
    fs::write(src.join("b.conf"), "iki").unwrap();

    let settings = Settings {
        repo_path: tmp.path().join("repo"),
        remote_url: String::new(),
        profile: "test".into(),
        sources: vec![Source::new(&src)],
        excludes: Vec::new(),
        skip_secrets: false,
        ..Settings::default()
    };

    // Henüz yedek yok: her şey yeni.
    let report = backup::detect_changes(&settings, &mut NoProgress).unwrap();
    assert_eq!(report.count(ChangeKind::Added), 2);

    backup::run(&settings, &mut NoProgress).unwrap();

    // Yedekten hemen sonra fark olmamalı.
    let report = backup::detect_changes(&settings, &mut NoProgress).unwrap();
    assert!(report.is_empty(), "yedek sonrası fark görünmemeli: {report:?}");

    // Aynı boyutta içerik değişikliği de yakalanmalı (sha256 karşılaştırması).
    fs::write(src.join("a.conf"), "BİR").unwrap();
    fs::write(src.join("c.conf"), "üç").unwrap();
    fs::remove_file(src.join("b.conf")).unwrap();

    let report = backup::detect_changes(&settings, &mut NoProgress).unwrap();
    assert_eq!(report.total(), 3);

    let find = |kind: ChangeKind| {
        report
            .files
            .iter()
            .filter(|f| f.kind == kind)
            .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(find(ChangeKind::Modified), vec!["a.conf"]);
    assert_eq!(find(ChangeKind::Added), vec!["c.conf"]);
    assert_eq!(find(ChangeKind::Removed), vec!["b.conf"]);

    // Silinen dosyanın boyutu manifest'ten gelmeli (dosya artık yok).
    let removed = report.files.iter().find(|f| f.kind == ChangeKind::Removed).unwrap();
    assert_eq!(removed.size, 3, "\"iki\" 3 bayt");
}

/// Ajan aynı bekleyen değişiklik için tekrar bildirim göndermemeli; bunun
/// dayanağı `fingerprint`'in küme değişmedikçe sabit kalması.
#[test]
fn ayni_degisiklik_kumesi_ayni_parmak_izini_verir() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("kaynak");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.conf"), "bir").unwrap();

    let settings = Settings {
        repo_path: tmp.path().join("repo"),
        remote_url: String::new(),
        profile: "test".into(),
        sources: vec![Source::new(&src)],
        excludes: Vec::new(),
        skip_secrets: false,
        ..Settings::default()
    };

    let first = backup::detect_changes(&settings, &mut NoProgress).unwrap();
    let second = backup::detect_changes(&settings, &mut NoProgress).unwrap();
    assert_eq!(
        first.fingerprint(),
        second.fingerprint(),
        "hiçbir şey değişmediyse parmak izi de değişmemeli"
    );

    // Yeni bir dosya kümeyi değiştirmeli.
    fs::write(src.join("b.conf"), "iki").unwrap();
    let third = backup::detect_changes(&settings, &mut NoProgress).unwrap();
    assert_ne!(first.fingerprint(), third.fingerprint());

    // Boş rapor da kendi içinde tutarlı olmalı.
    backup::run(&settings, &mut NoProgress).unwrap();
    let empty = backup::detect_changes(&settings, &mut NoProgress).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.fingerprint(), ChangeReport::default().fingerprint());
}

/// "Sessiz" kaynaklar: değişiklik raporunda görünürler ama bildirime konu
/// olmazlar. Ajanın susma kararı bu ayrıma dayanıyor.
#[test]
fn sessiz_kaynaktaki_degisiklik_bildirime_konu_olmaz() {
    let tmp = tempfile::tempdir().unwrap();
    let loud = tmp.path().join("loud");
    let quiet = tmp.path().join("quiet");
    fs::create_dir_all(&loud).unwrap();
    fs::create_dir_all(&quiet).unwrap();
    fs::write(loud.join("a.conf"), "bir").unwrap();
    fs::write(quiet.join("b.conf"), "iki").unwrap();

    let mut quiet_source = Source::new(&quiet);
    quiet_source.quiet = true;

    let settings = Settings {
        repo_path: tmp.path().join("repo"),
        remote_url: String::new(),
        profile: "test".into(),
        sources: vec![Source::new(&loud), quiet_source],
        excludes: Vec::new(),
        skip_secrets: false,
        ..Settings::default()
    };

    backup::run(&settings, &mut NoProgress).unwrap();

    // Yalnızca sessiz kaynak değişsin.
    fs::write(quiet.join("b.conf"), "İKİ").unwrap();
    let report = backup::detect_changes(&settings, &mut NoProgress).unwrap();
    assert_eq!(report.total(), 1, "değişiklik raporda görünmeli");
    assert_eq!(report.quiet_count(), 1);
    assert_eq!(
        report.notifiable_count(),
        0,
        "sessiz kaynak tek başına bildirim üretmemeli"
    );

    // Gürültülü kaynak da değişince bildirim yalnızca onu anlatmalı.
    fs::write(loud.join("a.conf"), "BİR").unwrap();
    let report = backup::detect_changes(&settings, &mut NoProgress).unwrap();
    assert_eq!(report.total(), 2);
    assert_eq!(report.notifiable_count(), 1);
    assert_eq!(report.notifiable_summary(), "1 modified");
    assert!(report.notifiable().all(|f| f.path.ends_with("a.conf")));

    // Sessiz taraftaki gürültü, bildirim kimliğini değiştirmemeli:
    // aksi halde ajan her turda yeniden bildirim gönderirdi.
    let before = report.notifiable_fingerprint();
    fs::write(quiet.join("c.conf"), "üç").unwrap();
    let after = backup::detect_changes(&settings, &mut NoProgress)
        .unwrap()
        .notifiable_fingerprint();
    assert_eq!(before, after, "sessiz değişiklik parmak izini kaydırmamalı");
}

/// Sessiz kaynaktaki sır şüphesi de bildirim üretmemeli.
#[test]
fn sessiz_kaynaktaki_sir_suphesi_sayilmaz() {
    let tmp = tempfile::tempdir().unwrap();
    let quiet = tmp.path().join("quiet");
    fs::create_dir_all(&quiet).unwrap();
    fs::write(
        quiet.join("token.pem"),
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEow==\n-----END RSA PRIVATE KEY-----\n",
    )
    .unwrap();

    let mut source = Source::new(&quiet);
    source.quiet = true;
    let settings = Settings {
        repo_path: tmp.path().join("repo"),
        remote_url: String::new(),
        profile: "test".into(),
        sources: vec![source],
        excludes: Vec::new(),
        skip_secrets: true,
        ..Settings::default()
    };

    let report = backup::detect_changes(&settings, &mut NoProgress).unwrap();
    assert_eq!(report.questions, 0, "sessiz kaynaktan karar sorulmamalı");
}

/// Günlük tur son commit'in yaşına bakar: taze yedekten sonra tetiklenmemeli,
/// 24 saat geçince tetiklenmeli, ayar kapalıyken hiç tetiklenmemeli.
#[test]
fn gunluk_tur_son_yedegin_yasina_gore_tetiklenir() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("kaynak");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.conf"), "bir").unwrap();

    let mut settings = Settings {
        repo_path: tmp.path().join("repo"),
        remote_url: String::new(),
        profile: "test".into(),
        sources: vec![Source::new(&src)],
        excludes: Vec::new(),
        skip_secrets: false,
        agent_daily_backup: true,
        ..Settings::default()
    };

    let now = chrono::Utc::now().timestamp();

    // Henüz depo yok: ilk tur hemen alınmalı.
    assert!(backup::daily_due(&settings, now), "ilk turda tetiklenmeli");

    backup::run(&settings, &mut NoProgress).unwrap();

    assert!(
        !backup::daily_due(&settings, now),
        "taze yedekten sonra tetiklenmemeli"
    );
    assert!(
        !backup::daily_due(&settings, now + 23 * 3600),
        "24 saat dolmadan tetiklenmemeli"
    );
    assert!(
        backup::daily_due(&settings, now + 24 * 3600 + 60),
        "24 saat geçince tetiklenmeli"
    );

    settings.agent_daily_backup = false;
    assert!(
        !backup::daily_due(&settings, now + 10 * 24 * 3600),
        "ayar kapalıyken hiç tetiklenmemeli"
    );
}
