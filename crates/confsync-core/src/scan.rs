//! Kaynak yolları gezip yedeklenecek dosya listesini üretir.
//!
//! Hariç tutma kalıpları gitignore sözdizimindedir; `ignore` crate'i ile
//! değerlendirilir. Böylece kullanıcı `**/cache/**`, `!önemli.conf` gibi
//! zaten bildiği bir dil kullanır.

use crate::secrets::{self, SecretReason};
use crate::settings::Settings;
use anyhow::{Context, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Excluded,
    TooLarge,
    Secret,
    Unreadable,
    UnsupportedType,
    NestedRepo,
    UserChoice,
}

impl SkipReason {
    pub fn label(&self) -> &'static str {
        match self {
            SkipReason::Excluded => "excluded",
            SkipReason::TooLarge => "over the size limit",
            SkipReason::Secret => "may hold a secret",
            SkipReason::Unreadable => "unreadable",
            SkipReason::UnsupportedType => "unsupported file type",
            SkipReason::NestedRepo => "nested git repository",
            SkipReason::UserChoice => "marked as always skip",
        }
    }

    /// Kararı sezgiye dayanan, dolayısıyla kullanıcıya sorulması anlamlı olan
    /// nedenler. Diğerleri ya kullanıcının açık tercihi (kalıp) ya da teknik
    /// zorunluluktur (okunamayan dosya, iç içe depo).
    pub fn is_question(&self) -> bool {
        matches!(self, SkipReason::Secret | SkipReason::TooLarge)
    }
}

/// Bir girdinin git deposu işaretçisi olup olmadığı.
///
/// `.git` hem klasör (normal depo) hem de dosya (submodule işaretçisi)
/// olabilir. İkisi de yedek deposuna kopyalanırsa o dizin libgit2 tarafından
/// iç içe depo sayılır ve commit tümüyle başarısız olur:
/// `invalid path: '...'; class=Index`. Bu yüzden kullanıcı kalıplarından
/// bağımsız olarak her zaman atlanır.
fn is_git_marker(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == ".git")
}

#[derive(Debug, Clone)]
pub struct ScanItem {
    pub path: PathBuf,
    pub size: u64,
    pub mode: u32,
    pub is_symlink: bool,
    pub link_target: Option<PathBuf>,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, Clone)]
pub struct SkippedItem {
    pub path: PathBuf,
    pub reason: SkipReason,
    pub detail: Option<String>,
}

/// Sezgiyle elenen ama kullanıcının "yine de al" diyebileceği dosya.
/// Karar verilebilmesi için dosyanın kendisi de saklanır.
#[derive(Debug, Clone)]
pub struct QuestionableItem {
    pub item: ScanItem,
    pub reason: SkipReason,
    pub detail: Option<String>,
}

#[derive(Debug, Default)]
pub struct ScanResult {
    /// Doğrudan yedeğe girecek dosyalar.
    pub items: Vec<ScanItem>,
    /// Kullanıcıya sorulacak dosyalar (sır şüphesi, boyut sınırı).
    pub questionable: Vec<QuestionableItem>,
    /// Kalıp ya da teknik nedenle kesin elenenler; yalnızca bilgi amaçlı.
    pub skipped: Vec<SkippedItem>,
    pub total_bytes: u64,
}

impl ScanResult {
    pub fn questionable_secrets(&self) -> impl Iterator<Item = &QuestionableItem> {
        self.questionable
            .iter()
            .filter(|q| q.reason == SkipReason::Secret)
    }
}

/// Hariç tutma kalıplarından bir eşleştirici kurar.
/// Yorum satırları (`#`) ve boş satırlar yok sayılır.
pub fn build_matcher(excludes: &[String]) -> Result<Gitignore> {
    // Kök olarak `/` verilir; kalıplar mutlak yollara karşı değerlendirilir.
    let mut builder = GitignoreBuilder::new("/");
    for pattern in excludes {
        let pattern = pattern.trim();
        if pattern.is_empty() || pattern.starts_with('#') {
            continue;
        }
        builder
            .add_line(None, pattern)
            .with_context(|| format!("invalid exclude pattern: {pattern}"))?;
    }
    Ok(builder.build()?)
}

/// Tek bir yolun hariç tutulup tutulmadığını söyler (arayüzde ön izleme için).
pub fn is_excluded(matcher: &Gitignore, path: &Path, is_dir: bool) -> bool {
    matcher.matched_path_or_any_parents(path, is_dir).is_ignore()
}

/// Ayarlardaki tüm etkin kaynakları gezer.
/// `progress` her dosya için çağrılır; `false` dönerse tarama iptal edilir.
pub fn scan(settings: &Settings, mut progress: impl FnMut(&Path) -> bool) -> Result<ScanResult> {
    let matcher = build_matcher(&settings.excludes)?;
    let max_size = settings.max_file_size_bytes();
    let mut result = ScanResult::default();

    for source in settings.enabled_sources() {
        if !source.path.exists() {
            result.skipped.push(SkippedItem {
                path: source.path.clone(),
                reason: SkipReason::Unreadable,
                detail: Some("path not found".into()),
            });
            continue;
        }

        let walker = WalkDir::new(&source.path)
            .follow_links(settings.follow_symlinks)
            .sort_by_file_name()
            .into_iter();

        // Hariç tutulan dizinlere hiç girme: büyük ağaçlarda ciddi hız kazancı.
        let walker = walker.filter_entry(|entry| {
            let is_dir = entry.file_type().is_dir();
            // `.git` klasörüne hiç girilmez; içeriği zaten yedeklenmemeli.
            if is_dir && is_git_marker(entry.path()) {
                return false;
            }
            !is_excluded(&matcher, entry.path(), is_dir)
        });

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    result.skipped.push(SkippedItem {
                        path: err.path().unwrap_or(&source.path).to_path_buf(),
                        reason: SkipReason::Unreadable,
                        detail: Some(err.to_string()),
                    });
                    continue;
                }
            };

            if !progress(entry.path()) {
                return Ok(result);
            }

            let file_type = entry.file_type();
            if file_type.is_dir() {
                continue;
            }

            // Submodule'lerde `.git` bir dosyadır ("gitdir: ..."); klasör
            // budaması bunu yakalamaz, burada ayrıca elenir.
            if is_git_marker(entry.path()) {
                result.skipped.push(SkippedItem {
                    path: entry.path().to_path_buf(),
                    reason: SkipReason::NestedRepo,
                    detail: Some("git repository marker".into()),
                });
                continue;
            }

            match classify(entry.path(), file_type.is_symlink(), max_size, settings) {
                Outcome::Keep(item) => {
                    result.total_bytes += item.size;
                    result.items.push(item);
                }
                Outcome::Ask(item, reason, detail) => {
                    result.questionable.push(QuestionableItem {
                        item,
                        reason,
                        detail,
                    });
                }
                Outcome::Reject(skipped) => result.skipped.push(skipped),
            }
        }
    }

    Ok(result)
}

/// Tek bir dosyanın taramadaki akıbeti.
enum Outcome {
    /// Doğrudan yedeğe girer.
    Keep(ScanItem),
    /// Sezgiyle elendi; kararı kullanıcı verir.
    Ask(ScanItem, SkipReason, Option<String>),
    /// Kesin elenir; yedeğe alınması mümkün ya da anlamlı değil.
    Reject(SkippedItem),
}

fn classify(path: &Path, is_symlink: bool, max_size: u64, settings: &Settings) -> Outcome {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    let skip = |reason: SkipReason, detail: Option<String>| SkippedItem {
        path: path.to_path_buf(),
        reason,
        detail,
    };

    if settings.is_always_skipped(path) {
        return Outcome::Reject(skip(SkipReason::UserChoice, None));
    }

    // symlink_metadata: bağlantının kendisini görmek için.
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) => return Outcome::Reject(skip(SkipReason::Unreadable, Some(e.to_string()))),
    };

    if is_symlink && !settings.follow_symlinks {
        let target = match std::fs::read_link(path) {
            Ok(target) => target,
            Err(e) => return Outcome::Reject(skip(SkipReason::Unreadable, Some(e.to_string()))),
        };
        return Outcome::Keep(ScanItem {
            path: path.to_path_buf(),
            size: 0,
            mode: meta.permissions().mode(),
            is_symlink: true,
            link_target: Some(target),
            uid: meta.uid(),
            gid: meta.gid(),
        });
    }

    if !meta.is_file() {
        // FIFO, soket, aygıt dosyası vb. yedeklenmez.
        return Outcome::Reject(skip(SkipReason::UnsupportedType, None));
    }

    let item = ScanItem {
        path: path.to_path_buf(),
        size: meta.len(),
        mode: meta.permissions().mode(),
        is_symlink: false,
        link_target: None,
        uid: meta.uid(),
        gid: meta.gid(),
    };

    // Kullanıcı bu dosya için daha önce "yine de al" dediyse sezgiler atlanır.
    if settings.is_always_included(path) {
        return Outcome::Keep(item);
    }

    if meta.len() > max_size {
        return Outcome::Ask(
            item,
            SkipReason::TooLarge,
            Some(format!("limit {} MiB", settings.max_file_size_mb)),
        );
    }

    if settings.skip_secrets {
        if let Some(reason) = detect_secret(path) {
            return Outcome::Ask(
                item,
                SkipReason::Secret,
                Some(reason.description().into()),
            );
        }
    }

    Outcome::Keep(item)
}

fn detect_secret(path: &Path) -> Option<SecretReason> {
    if secrets::suspicious_name(path) {
        return Some(SecretReason::KnownName);
    }
    // İçerik taraması için yalnızca ilk 8 KiB okunur.
    let contents = read_head(path, 8 * 1024).ok()?;
    secrets::inspect(path, &contents)
}

fn read_head(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; limit];
    let read = file.read(&mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_handles_negation() {
        let matcher = build_matcher(&[
            "**/.ssh/id_*".to_string(),
            "!**/.ssh/id_*.pub".to_string(),
        ])
        .unwrap();
        assert!(is_excluded(
            &matcher,
            Path::new("/home/a/.ssh/id_rsa"),
            false
        ));
        assert!(!is_excluded(
            &matcher,
            Path::new("/home/a/.ssh/id_rsa.pub"),
            false
        ));
    }

    #[test]
    fn matcher_skips_comments() {
        let matcher = build_matcher(&["# yorum".to_string(), "".to_string()]).unwrap();
        assert!(!is_excluded(&matcher, Path::new("/home/a/.bashrc"), false));
    }

    /// Sır sezgisi dosyayı elemez, kullanıcıya sorulmak üzere ayırır;
    /// hatırlanan kararlar sezgiyi geçersiz kılar.
    #[test]
    fn sir_supheli_dosya_sorulur_hatirlanan_karar_uygulanir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root).unwrap();
        let secret = root.join("token.pem");
        std::fs::write(
            &secret,
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEow==\n-----END RSA PRIVATE KEY-----\n",
        )
        .unwrap();
        std::fs::write(root.join("normal.conf"), "anahtar = değer").unwrap();

        let base = Settings {
            sources: vec![crate::settings::Source::new(root)],
            excludes: Vec::new(),
            skip_secrets: true,
            ..Settings::default()
        };

        // Varsayılan: elenmez, sorulacaklar arasına girer.
        let result = scan(&base, |_| true).unwrap();
        assert!(result.questionable.iter().any(|q| q.item.path == secret));
        assert!(!result.items.iter().any(|i| i.path == secret));
        assert!(result.items.iter().any(|i| i.path.ends_with("normal.conf")));

        // "Yine de al" hatırlandıysa doğrudan yedeğe girer.
        let mut included = base.clone();
        included.remember_decision(&secret, true);
        let result = scan(&included, |_| true).unwrap();
        assert!(result.items.iter().any(|i| i.path == secret));
        assert!(result.questionable.is_empty());

        // "Hep atla" hatırlandıysa hiç sorulmaz.
        let mut skipped = base.clone();
        skipped.remember_decision(&secret, false);
        let result = scan(&skipped, |_| true).unwrap();
        assert!(result.questionable.is_empty());
        assert!(result
            .skipped
            .iter()
            .any(|s| s.path == secret && s.reason == SkipReason::UserChoice));
    }

    /// Submodule işaretçisi `.git` bir dosyadır; depoya kopyalanırsa commit
    /// `invalid path: '...'; class=Index` ile tümüyle başarısız olur.
    #[test]
    fn git_isaretcileri_kullanici_kaliplarindan_bagimsiz_atlanir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Submodule düzeni: dosya olarak `.git`.
        let sub = root.join("plugins/tpm/lib/tmux-test");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(".git"), "gitdir: ../../.git/modules/lib/tmux-test").unwrap();
        std::fs::write(sub.join("README.md"), "içerik").unwrap();

        // Normal depo düzeni: klasör olarak `.git`.
        let repo_dir = root.join("plugins/tpm/.git");
        std::fs::create_dir_all(repo_dir.join("objects")).unwrap();
        std::fs::write(repo_dir.join("HEAD"), "ref: refs/heads/main").unwrap();
        std::fs::write(repo_dir.join("objects/deadbeef"), "nesne").unwrap();

        // Hariç tutma listesi bilerek boş: eleme kalıplara bağlı olmamalı.
        let settings = Settings {
            sources: vec![crate::settings::Source::new(root)],
            excludes: Vec::new(),
            skip_secrets: false,
            ..Settings::default()
        };

        let result = scan(&settings, |_| true).unwrap();
        let stored: Vec<_> = result.items.iter().map(|i| i.path.clone()).collect();

        assert!(stored.contains(&sub.join("README.md")));
        assert!(!stored.iter().any(|p| p.file_name().unwrap() == ".git"));
        // `.git` klasörünün içeriğine hiç girilmemeli.
        assert!(!stored.iter().any(|p| p.starts_with(&repo_dir)));
        assert!(result
            .skipped
            .iter()
            .any(|s| s.reason == SkipReason::NestedRepo && s.path == sub.join(".git")));
    }
}
