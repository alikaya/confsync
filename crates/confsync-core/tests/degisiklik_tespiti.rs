//! `backup::detect_changes` ajanın tek karar mekanizması: yanlış "değişiklik
//! yok" derse yedek sessizce eskir, yanlış "değişti" derse bildirim yağar.

use confsync_core::backup::{self, ChangeKind, NoProgress};
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
