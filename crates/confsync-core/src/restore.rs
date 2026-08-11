//! Geri yükleme.
//!
//! İki aşamalıdır: önce **plan** çıkarılır (hiçbir şey yazılmaz), kullanıcı
//! onaylayınca **uygulanır**. Üzerine yazılacak her dosyanın kopyası
//! `~/.local/share/confsync/rollback/<zaman>/` altına alınır.

use crate::backup::sha256_file;
use crate::manifest::{Entry, EntryKind, Manifest};
use crate::paths;
use crate::settings::{self, Settings};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Hedefte dosya yok, oluşturulacak.
    Create,
    /// Hedefteki dosya farklı, üzerine yazılacak.
    Overwrite,
    /// İçerik zaten aynı, dokunulmayacak.
    Unchanged,
    /// Hedefte beklenmedik bir tür var (ör. dosya yerine dizin).
    Conflict,
    /// Depodaki içerik bulunamadı.
    Missing,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::Create => "oluşturulacak",
            Action::Overwrite => "üzerine yazılacak",
            Action::Unchanged => "değişmedi",
            Action::Conflict => "çakışma",
            Action::Missing => "depoda bulunamadı",
        }
    }

    pub fn writes(&self) -> bool {
        matches!(self, Action::Create | Action::Overwrite)
    }
}

#[derive(Debug, Clone)]
pub struct PlanItem {
    pub entry: Entry,
    pub target: PathBuf,
    pub source: PathBuf,
    pub action: Action,
    /// Kullanıcı tek tek seçebilsin diye.
    pub selected: bool,
    pub note: Option<String>,
}

#[derive(Debug, Default)]
pub struct RestorePlan {
    pub items: Vec<PlanItem>,
    pub profile: String,
    pub source_home: String,
}

impl RestorePlan {
    pub fn count(&self, action: Action) -> usize {
        self.items.iter().filter(|i| i.action == action).count()
    }

    pub fn selected_writes(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.selected && i.action.writes())
            .count()
    }

    pub fn select_all(&mut self, value: bool) {
        for item in &mut self.items {
            if item.action.writes() {
                item.selected = value;
            }
        }
    }
}

/// Depodaki profil için geri yükleme planı çıkarır. Diske yazmaz.
pub fn plan(settings: &Settings, target_home: &Path) -> Result<RestorePlan> {
    let manifest_path = paths::manifest_path(&settings.repo_path, &settings.profile);
    let manifest = Manifest::load(&manifest_path).with_context(|| {
        format!(
            "'{}' profili için manifest bulunamadı. Önce yedek alın ya da uzak depodan çekin.",
            settings.profile
        )
    })?;

    let files_dir = paths::profile_files_dir(&settings.repo_path, &settings.profile);
    let mut plan = RestorePlan {
        profile: manifest.profile.clone(),
        source_home: manifest.source_home.clone(),
        ..Default::default()
    };

    for entry in manifest.entries {
        let rel = PathBuf::from(&entry.repo_path);
        let target = paths::from_repo_rel(&rel, target_home)?;
        let source = files_dir.join(&rel);
        let (action, note) = decide(&entry, &source, &target)?;
        plan.items.push(PlanItem {
            entry,
            target,
            source,
            selected: action.writes(),
            action,
            note,
        });
    }

    plan.items.sort_by(|a, b| a.target.cmp(&b.target));
    Ok(plan)
}

fn decide(entry: &Entry, source: &Path, target: &Path) -> Result<(Action, Option<String>)> {
    match entry.kind {
        EntryKind::Symlink => {
            let want = entry.link_target.clone().unwrap_or_default();
            match std::fs::symlink_metadata(target) {
                Err(_) => Ok((Action::Create, None)),
                Ok(meta) if meta.file_type().is_symlink() => {
                    let current = std::fs::read_link(target)?;
                    if current.to_string_lossy() == want {
                        Ok((Action::Unchanged, None))
                    } else {
                        Ok((
                            Action::Overwrite,
                            Some(format!("mevcut hedef: {}", current.display())),
                        ))
                    }
                }
                Ok(_) => Ok((
                    Action::Conflict,
                    Some("hedefte bağlantı yerine gerçek bir dosya/dizin var".into()),
                )),
            }
        }
        EntryKind::Dir => {
            if target.is_dir() {
                Ok((Action::Unchanged, None))
            } else if target.exists() {
                Ok((Action::Conflict, Some("hedef bir dizin değil".into())))
            } else {
                Ok((Action::Create, None))
            }
        }
        EntryKind::File => {
            if !source.exists() {
                return Ok((
                    Action::Missing,
                    Some("depodaki içerik silinmiş olabilir".into()),
                ));
            }
            match std::fs::symlink_metadata(target) {
                Err(_) => Ok((Action::Create, None)),
                Ok(meta) if meta.is_dir() => Ok((
                    Action::Conflict,
                    Some("hedefte aynı isimde bir dizin var".into()),
                )),
                Ok(_) => {
                    let current = sha256_file(target).ok();
                    if current.is_some() && current == entry.sha256 {
                        Ok((Action::Unchanged, None))
                    } else {
                        Ok((Action::Overwrite, None))
                    }
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct RestoreReport {
    pub written: usize,
    pub skipped: usize,
    pub failed: Vec<(PathBuf, String)>,
    pub rollback_dir: Option<PathBuf>,
}

/// Planı uygular. Yalnızca `selected` ve yazma gerektiren maddeler işlenir.
pub fn apply(
    plan: &RestorePlan,
    make_rollback: bool,
    mut progress: impl FnMut(&Path, usize, usize) -> bool,
) -> Result<RestoreReport> {
    let mut report = RestoreReport::default();

    let rollback_root = if make_rollback {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let dir = settings::rollback_dir().join(stamp);
        std::fs::create_dir_all(&dir)?;
        report.rollback_dir = Some(dir.clone());
        Some(dir)
    } else {
        None
    };

    let todo: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|i| i.selected && i.action.writes())
        .collect();
    let total = todo.len();

    for (index, item) in todo.into_iter().enumerate() {
        if !progress(&item.target, index + 1, total) {
            break;
        }
        if let Err(err) = restore_one(item, rollback_root.as_deref()) {
            report
                .failed
                .push((item.target.clone(), format!("{err:#}")));
        } else {
            report.written += 1;
        }
    }

    report.skipped = plan.items.len() - report.written - report.failed.len();
    Ok(report)
}

fn restore_one(item: &PlanItem, rollback_root: Option<&Path>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(parent) = item.target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("dizin oluşturulamadı: {}", parent.display()))?;
    }

    // Mevcut dosyanın yedeğini al.
    if let Some(root) = rollback_root {
        if std::fs::symlink_metadata(&item.target).is_ok() {
            let dest = root.join(&item.entry.repo_path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // Bağlantıysa hedefi değil kendisini not et.
            let meta = std::fs::symlink_metadata(&item.target)?;
            if meta.file_type().is_symlink() {
                let target = std::fs::read_link(&item.target)?;
                std::fs::write(dest.with_extension("symlink"), target.to_string_lossy().as_bytes())?;
            } else {
                std::fs::copy(&item.target, &dest)?;
            }
        }
    }

    match item.entry.kind {
        EntryKind::Symlink => {
            let target = item.entry.link_target.clone().unwrap_or_default();
            if std::fs::symlink_metadata(&item.target).is_ok() {
                std::fs::remove_file(&item.target)?;
            }
            std::os::unix::fs::symlink(&target, &item.target)
                .with_context(|| format!("bağlantı kurulamadı: {}", item.target.display()))?;
        }
        EntryKind::Dir => {
            std::fs::create_dir_all(&item.target)?;
            std::fs::set_permissions(
                &item.target,
                std::fs::Permissions::from_mode(item.entry.mode),
            )?;
        }
        EntryKind::File => {
            // Önce geçici dosyaya yaz, sonra yerine taşı: yarım kalmış yazma olmasın.
            let tmp = item.target.with_extension("confsync-tmp");
            std::fs::copy(&item.source, &tmp).with_context(|| {
                format!(
                    "kopyalanamadı: {} -> {}",
                    item.source.display(),
                    tmp.display()
                )
            })?;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(item.entry.mode))?;
            std::fs::rename(&tmp, &item.target)
                .with_context(|| format!("yerine taşınamadı: {}", item.target.display()))?;
        }
    }

    Ok(())
}

/// Depoda hangi profillerin bulunduğunu listeler.
pub fn available_profiles(repo_path: &Path) -> Vec<String> {
    let dir = repo_path.join("profiles");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    out.sort();
    out
}
