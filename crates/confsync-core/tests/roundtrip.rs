//! Uçtan uca test: sahte bir `$HOME` kurar, yedek alır, dosyaları bozar,
//! geri yükler ve içerik + izinlerin döndüğünü doğrular.
//!
//! Tek bir test var; `HOME` ortam değişkenini değiştirdiği için paralel
//! çalışacak ikinci bir test eklenmemeli.

use confsync_core::manifest::{EntryKind, Manifest};
use confsync_core::restore::Action;
use confsync_core::settings::{Settings, Source};
use confsync_core::{backup, paths, restore};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn write(path: &Path, contents: &str, mode: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn backup_then_restore_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&home).unwrap();

    // dirs::home_dir() Linux'ta $HOME'u okur.
    std::env::set_var("HOME", &home);

    // --- sahte yapılandırma ağacı ---
    write(&home.join(".bashrc"), "export EDITOR=nvim\n", 0o644);
    write(&home.join(".config/app/settings.toml"), "theme = \"dark\"\n", 0o600);
    write(&home.join(".config/app/run.sh"), "#!/bin/sh\necho hi\n", 0o755);
    // sır: içerik taramasıyla yakalanmalı
    write(
        &home.join(".config/app/token.conf"),
        "api_key = \"AKIA1234567890ABCDEF\"\n",
        0o644,
    );
    // hariç tutma kalıbıyla elenmeli
    write(&home.join(".config/app/cache/blob.bin"), "junk", 0o644);
    // sembolik bağlantı
    std::os::unix::fs::symlink("settings.toml", home.join(".config/app/current.toml")).unwrap();

    let settings = Settings {
        repo_path: repo.clone(),
        remote_url: String::new(),
        branch: "main".into(),
        profile: "testmakine".into(),
        sources: vec![Source::new(home.join(".config")), Source::new(home.join(".bashrc"))],
        excludes: vec!["**/cache/**".into()],
        follow_symlinks: false,
        max_file_size_mb: 5,
        skip_secrets: true,
        auto_push: false,
        author_name: "test".into(),
        author_email: "test@localhost".into(),
        // Kalan alanlar için varsayılanlar: yeni ayar eklendiğinde bu test
        // derlemeyi bozmasın.
        ..Settings::default()
    };

    // --- yedekle ---
    let report = backup::run(&settings, &mut backup::NoProgress).unwrap();
    assert!(report.commit_id.is_some(), "ilk yedek commit üretmeli");

    let manifest = Manifest::load(&paths::manifest_path(&repo, "testmakine")).unwrap();
    let stored: Vec<&str> = manifest.entries.iter().map(|e| e.repo_path.as_str()).collect();

    assert!(stored.contains(&"home/.bashrc"));
    assert!(stored.contains(&"home/.config/app/settings.toml"));
    assert!(stored.contains(&"home/.config/app/run.sh"));
    assert!(
        !stored.contains(&"home/.config/app/cache/blob.bin"),
        "hariç tutulan dosya yedeklenmemeli"
    );
    assert!(
        !stored.contains(&"home/.config/app/token.conf"),
        "sır içeren dosya yedeklenmemeli"
    );

    // izinler ve bağlantı üstverisi korunmuş mu
    let run_sh = manifest
        .entries
        .iter()
        .find(|e| e.repo_path == "home/.config/app/run.sh")
        .unwrap();
    assert_eq!(run_sh.mode & 0o777, 0o755);

    let link = manifest
        .entries
        .iter()
        .find(|e| e.repo_path == "home/.config/app/current.toml")
        .unwrap();
    assert_eq!(link.kind, EntryKind::Symlink);
    assert_eq!(link.link_target.as_deref(), Some("settings.toml"));

    // --- yerel dosyaları boz ---
    fs::write(home.join(".bashrc"), "BOZUK\n").unwrap();
    fs::remove_file(home.join(".config/app/run.sh")).unwrap();
    fs::remove_file(home.join(".config/app/current.toml")).unwrap();

    // --- plan çıkar ---
    let mut plan = restore::plan(&settings, &home).unwrap();
    assert_eq!(plan.count(Action::Overwrite), 1, ".bashrc üzerine yazılmalı");
    assert_eq!(plan.count(Action::Create), 2, "silinen dosya ve bağlantı yeniden kurulmalı");
    assert_eq!(
        plan.count(Action::Unchanged),
        1,
        "settings.toml değişmemiş olmalı"
    );

    plan.select_all(true);
    let restore_report = restore::apply(&plan, true, |_, _, _| true).unwrap();
    assert!(restore_report.failed.is_empty(), "{:?}", restore_report.failed);
    assert_eq!(restore_report.written, 3);

    // --- doğrula ---
    assert_eq!(
        fs::read_to_string(home.join(".bashrc")).unwrap(),
        "export EDITOR=nvim\n"
    );
    let run_meta = fs::metadata(home.join(".config/app/run.sh")).unwrap();
    assert_eq!(run_meta.permissions().mode() & 0o777, 0o755);
    assert_eq!(
        fs::read_link(home.join(".config/app/current.toml")).unwrap(),
        Path::new("settings.toml")
    );

    // geri alma kopyası bozulmuş .bashrc'yi saklamış olmalı
    let rollback = restore_report.rollback_dir.unwrap();
    assert_eq!(
        fs::read_to_string(rollback.join("home/.bashrc")).unwrap(),
        "BOZUK\n"
    );

    // --- ikinci yedek: değişiklik yoksa commit üretilmemeli ---
    let second = backup::run(&settings, &mut backup::NoProgress).unwrap();
    assert!(
        second.commit_id.is_none(),
        "değişiklik yokken boş commit atılmamalı"
    );
}
