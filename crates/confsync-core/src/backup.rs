//! Yedekleme akışı: tara → depoya kopyala → manifest yaz → commit (→ push).

use crate::gitrepo;
use crate::manifest::{Entry, EntryKind, Manifest};
use crate::paths;
use crate::scan::{self, ScanItem, ScanResult, SkipReason, SkippedItem};
use crate::settings::Settings;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use crate::lock;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct BackupReport {
    pub scanned: usize,
    pub stored: usize,
    pub skipped: usize,
    pub bytes: u64,
    pub commit_id: Option<String>,
    pub pushed: bool,
    pub scan: ScanResult,
}

impl BackupReport {
    pub fn had_changes(&self) -> bool {
        self.commit_id.is_some()
    }
}

/// İlerleme bildirimi. `false` dönerse işlem iptal edilir.
pub trait Progress {
    fn stage(&mut self, name: &str);
    fn file(&mut self, path: &Path, index: usize, total: usize) -> bool;
}

/// Hiçbir şey yapmayan ilerleme bildirimi (test ve CLI için).
pub struct NoProgress;

impl Progress for NoProgress {
    fn stage(&mut self, _name: &str) {}
    fn file(&mut self, _path: &Path, _index: usize, _total: usize) -> bool {
        true
    }
}

/// Sezginin bir dosyayı neden elediği.
#[derive(Debug, Clone)]
pub struct Question {
    pub reason: SkipReason,
    pub detail: Option<String>,
}

/// Plandaki tek bir dosya ve o dosyanın yedeğe girip girmeyeceği.
#[derive(Debug, Clone)]
pub struct PlanEntry {
    pub item: ScanItem,
    /// `Some` ise sezgi bu dosyayı eledi ve kullanıcıya sorulur.
    pub question: Option<Question>,
    pub include: bool,
}

/// Yedeklemenin ilk aşamasının çıktısı: ne alınacak, ne sorulacak, ne elendi.
/// Kullanıcı kararlarını verdikten sonra [`apply`] ile uygulanır.
#[derive(Debug, Default)]
pub struct BackupPlan {
    pub entries: Vec<PlanEntry>,
    /// Kalıp ya da teknik nedenle kesin elenenler; karar dışıdır.
    pub skipped: Vec<SkippedItem>,
}

impl BackupPlan {
    /// Kullanıcıya sorulacak bir şey var mı.
    pub fn needs_review(&self) -> bool {
        self.entries.iter().any(|e| e.question.is_some())
    }

    pub fn question_count(&self) -> usize {
        self.entries.iter().filter(|e| e.question.is_some()).count()
    }

    pub fn included_count(&self) -> usize {
        self.entries.iter().filter(|e| e.include).count()
    }

    pub fn included_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.include)
            .map(|e| e.item.size)
            .sum()
    }

    /// Sorulan dosyaların tümünü dahil eder ya da tümünü dışarıda bırakır.
    pub fn set_all_questions(&mut self, include: bool) {
        for entry in self.entries.iter_mut().filter(|e| e.question.is_some()) {
            entry.include = include;
        }
    }

    fn split(self) -> (Vec<ScanItem>, Vec<SkippedItem>) {
        let BackupPlan {
            entries,
            mut skipped,
        } = self;
        let mut items = Vec::with_capacity(entries.len());
        for entry in entries {
            if entry.include {
                items.push(entry.item);
            } else {
                let (reason, detail) = match entry.question {
                    Some(q) => (q.reason, q.detail),
                    // Sezgi elemedi ama kullanıcı yine de dışarıda bıraktı.
                    None => (SkipReason::UserChoice, None),
                };
                skipped.push(SkippedItem {
                    path: entry.item.path,
                    reason,
                    detail,
                });
            }
        }
        (items, skipped)
    }
}

/// Yedeklemenin tarama aşaması. Hiçbir dosyayı değiştirmez.
pub fn plan(settings: &Settings, progress: &mut impl Progress) -> Result<BackupPlan> {
    progress.stage("Scanning files");
    // Tarama aşamasında toplam dosya sayısı önceden bilinmez; arayüz
    // `total == 0` görünce belirsiz (animasyonlu) çubuğa geçer. Geri dönen
    // `false` taramayı da iptal edilebilir kılar.
    let mut seen = 0usize;
    let scanned = scan::scan(settings, |path| {
        seen += 1;
        progress.file(path, seen, 0)
    })?;

    let mut entries: Vec<PlanEntry> = scanned
        .items
        .into_iter()
        .map(|item| PlanEntry {
            item,
            question: None,
            include: true,
        })
        .collect();

    // Sezginin elediği dosyalar aksi söylenene kadar dışarıda kalır.
    entries.extend(scanned.questionable.into_iter().map(|q| PlanEntry {
        item: q.item,
        question: Some(Question {
            reason: q.reason,
            detail: q.detail,
        }),
        include: false,
    }));
    entries.sort_by(|a, b| a.item.path.cmp(&b.item.path));

    Ok(BackupPlan {
        entries,
        skipped: scanned.skipped,
    })
}

/// Tarama + kopyalama; sezgiyle elenen dosyalar sorulmadan dışarıda bırakılır.
/// Arayüz iki aşamayı ayrı çağırır; bu sarmalayıcı test ve betikler içindir.
pub fn run(settings: &Settings, progress: &mut impl Progress) -> Result<BackupReport> {
    let plan = plan(settings, progress)?;
    apply(settings, plan, progress)
}

/// Bir dosyanın son yedeğe göre durumu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeKind {
    Added,
    Modified,
    Removed,
}

impl ChangeKind {
    pub fn label(&self) -> &'static str {
        match self {
            ChangeKind::Added => "new",
            ChangeKind::Modified => "modified",
            ChangeKind::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub kind: ChangeKind,
    /// Silinenlerde son yedekteki boyut, diğerlerinde diskteki güncel boyut.
    pub size: u64,
}

/// Son yedeğe göre kaynaklardaki fark.
#[derive(Debug, Default, Clone)]
pub struct ChangeReport {
    /// Türe, sonra yola göre sıralı: arayüzdeki tablo doğrudan bunu basar.
    pub files: Vec<ChangedFile>,
    /// Kararı kullanıcıya sorulacak dosya sayısı.
    pub questions: usize,
}

impl ChangeReport {
    pub fn count(&self, kind: ChangeKind) -> usize {
        self.files.iter().filter(|f| f.kind == kind).count()
    }

    pub fn total(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Toplam etkilenen bayt (silinenler dahil).
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Değişiklik kümesinin kimliği.
    ///
    /// Ajan bunu, aynı bekleyen değişiklik için tekrar tekrar bildirim
    /// göndermemek üzere kullanır: yedeklenmemiş bir dosya her denetimde
    /// yeniden "değişmiş" görünür, ama kullanıcıya bir kez söylemek yeter.
    /// `files` türe ve yola göre sıralı olduğundan özet çalıştırmalar arasında
    /// da kararlıdır.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for file in &self.files {
            file.kind.hash(&mut hasher);
            file.path.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Bildirimde ve durum çubuğunda gösterilen tek satırlık özet.
    pub fn summary(&self) -> String {
        let parts: Vec<String> = [
            ChangeKind::Added,
            ChangeKind::Modified,
            ChangeKind::Removed,
        ]
        .iter()
        .filter_map(|kind| {
            let count = self.count(*kind);
            (count > 0).then(|| format!("{count} {}", kind.label()))
        })
        .collect();

        if parts.is_empty() {
            "no changes".into()
        } else {
            parts.join(" · ")
        }
    }
}

/// Kaynakları son yedekle karşılaştırır. **Hiçbir dosyayı değiştirmez.**
///
/// Karşılaştırma manifest'teki `sha256` üzerinden yapılır; boyutu aynı kalan
/// içerik değişiklikleri de yakalanır. 24 MiB'lık bir ağaç için özet çıkarmak
/// milisaniyeler sürdüğünden düzenli yoklama ucuzdur.
pub fn detect_changes(settings: &Settings, progress: &mut impl Progress) -> Result<ChangeReport> {
    use std::collections::HashMap;

    let plan = plan(settings, progress)?;
    let manifest_path = paths::manifest_path(&settings.repo_path, &settings.profile);
    let previous: HashMap<String, Entry> = match Manifest::load(&manifest_path) {
        Ok(manifest) => manifest
            .entries
            .into_iter()
            .map(|e| (e.origin_path.clone(), e))
            .collect(),
        // Henüz yedek yoksa her şey "yeni" sayılır.
        Err(_) => HashMap::new(),
    };

    let mut report = ChangeReport {
        questions: plan.question_count(),
        ..Default::default()
    };
    let mut seen = std::collections::HashSet::new();

    progress.stage("Comparing changes");
    for entry in plan.entries.iter().filter(|e| e.include) {
        let key = entry.item.path.to_string_lossy().to_string();
        seen.insert(key.clone());

        let kind = match previous.get(&key) {
            None => Some(ChangeKind::Added),
            Some(old) => {
                let changed = if entry.item.is_symlink {
                    old.link_target.as_deref()
                        != entry.item.link_target.as_ref().map(|t| t.to_str().unwrap_or(""))
                } else if old.size != entry.item.size {
                    true
                } else {
                    // Boyut aynı: içeriği özetleyip karşılaştır.
                    match (&old.sha256, sha256_file(&entry.item.path).ok()) {
                        (Some(old_hash), Some(new_hash)) => old_hash != &new_hash,
                        _ => true,
                    }
                };
                changed.then_some(ChangeKind::Modified)
            }
        };

        if let Some(kind) = kind {
            report.files.push(ChangedFile {
                path: entry.item.path.clone(),
                kind,
                size: entry.item.size,
            });
        }
    }

    for (key, entry) in &previous {
        if !seen.contains(key) {
            report.files.push(ChangedFile {
                path: PathBuf::from(&entry.origin_path),
                kind: ChangeKind::Removed,
                size: entry.size,
            });
        }
    }

    // Tablo doğrudan basılabilsin diye burada sıralanır: önce tür, sonra yol.
    report
        .files
        .sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.path.cmp(&b.path)));

    Ok(report)
}

/// Planı uygular: dosyaları depoya kopyalar, manifest yazar, commit'ler.
///
/// Depo dizini işlem başında sıfırlandığı için aynı anda yalnızca tek bir
/// yedekleme çalışabilir; ajan ile arayüz çakışmasın diye kilit alınır.
pub fn apply(
    settings: &Settings,
    plan: BackupPlan,
    progress: &mut impl Progress,
) -> Result<BackupReport> {
    let _guard = lock::acquire(&settings.repo_path)?;
    let home = crate::settings::home_dir();
    let mut report = BackupReport::default();

    let (items, skipped) = plan.split();
    report.scanned = items.len();
    report.skipped = skipped.len();
    let scanned = ScanResult {
        items,
        questionable: Vec::new(),
        skipped,
        total_bytes: 0,
    };

    progress.stage("Preparing repository");
    let repo = gitrepo::open_or_init(&settings.repo_path, &settings.branch)?;
    gitrepo::set_remote(&repo, &settings.remote_url)?;
    ensure_repo_scaffold(&settings.repo_path)?;

    let files_dir = paths::profile_files_dir(&settings.repo_path, &settings.profile);
    // Kaynakta silinen dosyalar depoda kalmasın diye profil dizini sıfırlanır.
    if files_dir.exists() {
        std::fs::remove_dir_all(&files_dir)
            .with_context(|| format!("eski profil temizlenemedi: {}", files_dir.display()))?;
    }
    std::fs::create_dir_all(&files_dir)?;

    progress.stage("Copying files");
    let mut manifest = Manifest::new(&settings.profile, &home.to_string_lossy());
    let total = scanned.items.len();

    for (index, item) in scanned.items.iter().enumerate() {
        if !progress.file(&item.path, index + 1, total) {
            anyhow::bail!("cancelled by the user");
        }

        let rel = paths::to_repo_rel(&item.path, &home)?;
        let dest = files_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let entry = if item.is_symlink {
            let target = item
                .link_target
                .clone()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            // Bağlantının kendisi değil, hedefi manifest'e yazılır;
            // depoda yer tutucu bir dosya bırakılmaz.
            Entry {
                repo_path: rel.to_string_lossy().to_string(),
                origin_path: item.path.to_string_lossy().to_string(),
                kind: EntryKind::Symlink,
                mode: item.mode & 0o7777,
                size: 0,
                sha256: None,
                link_target: Some(target),
                uid: item.uid,
                gid: item.gid,
            }
        } else {
            std::fs::copy(&item.path, &dest).with_context(|| {
                format!("could not copy: {} -> {}", item.path.display(), dest.display())
            })?;
            let digest = sha256_file(&dest)?;
            report.bytes += item.size;
            Entry {
                repo_path: rel.to_string_lossy().to_string(),
                origin_path: item.path.to_string_lossy().to_string(),
                kind: EntryKind::File,
                mode: item.mode & 0o7777,
                size: item.size,
                sha256: Some(digest),
                link_target: None,
                uid: item.uid,
                gid: item.gid,
            }
        };

        manifest.entries.push(entry);
        report.stored += 1;
    }

    progress.stage("Writing manifest");
    // Dizin okuma sırası platforma göre değişebilir; manifest'in her koşuda
    // birebir aynı çıkması için sıralıyoruz (gereksiz commit olmasın).
    manifest.entries.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));
    manifest.save(&paths::manifest_path(&settings.repo_path, &settings.profile))?;

    progress.stage("Creating commit");
    let message = commit_message(&settings.profile, &manifest);
    report.commit_id = gitrepo::commit_all(
        &repo,
        &message,
        &settings.author_name,
        &settings.author_email,
    )?;

    if settings.auto_push && !settings.remote_url.trim().is_empty() && report.commit_id.is_some() {
        progress.stage("Pushing to the remote");
        gitrepo::push(&repo, &settings.branch)?;
        report.pushed = true;
    }

    report.scan = scanned;
    Ok(report)
}

fn commit_message(profile: &str, manifest: &Manifest) -> String {
    format!(
        "{profile}: {} files backed up ({})",
        manifest.entries.len(),
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    )
}

/// Depo ilk kez oluşturulurken README ve .gitattributes koyar.
fn ensure_repo_scaffold(repo_path: &Path) -> Result<()> {
    let readme = repo_path.join("README.md");
    if !readme.exists() {
        std::fs::write(
            &readme,
            "# confsync repository\n\n\
             This repository is managed by confsync. Use the application \
             instead of editing it by hand.\n\n\
             Layout:\n\
             - `profiles/<machine>/manifest.json` — file permissions and link targets\n\
             - `profiles/<machine>/files/home/...` — files under `$HOME`\n\
             - `profiles/<machine>/files/root/...` — other files under `/`\n",
        )?;
    }
    let attrs = repo_path.join(".gitattributes");
    if !attrs.exists() {
        // Yapılandırma dosyalarında satır sonu dönüşümü yapılmamalı.
        std::fs::write(&attrs, "* -text\n")?;
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
