//! Manifest: git'in koruyamadığı dosya üstverisini saklar.
//!
//! Git yalnızca "çalıştırılabilir mi" bitini tutar. İzinler (0600 gibi),
//! sahiplik ve sembolik bağlantı hedefleri burada kayıt altına alınır;
//! geri yüklemede bu bilgiye göre yeniden uygulanır.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Symlink,
    /// İçi boş olduğu için ayrıca kaydedilen dizin (git boş dizin tutamaz).
    Dir,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Depo içindeki göreli yol, ör. `home/.bashrc`.
    pub repo_path: String,
    /// Yedek alındığı andaki mutlak yol (bilgi amaçlı).
    pub origin_path: String,
    pub kind: EntryKind,
    /// Unix izin bitleri (yalnızca alt 12 bit anlamlı).
    pub mode: u32,
    pub size: u64,
    /// Dosya içeriğinin SHA-256 özeti (symlink/dir için `None`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Sembolik bağlantı hedefi.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
    pub uid: u32,
    pub gid: u32,
}

/// Manifest içeriği **belirlenimci** olmalıdır: aynı dosya kümesi aynı JSON'u
/// üretmeli. Bu yüzden zaman damgası tutulmaz — değişiklik olmadığında boş
/// commit atılmasına yol açardı. "Ne zaman yedeklendi" bilgisi git commit
/// tarihinden okunur.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub profile: String,
    /// Yedeği alan makinenin ev dizini; farklı kullanıcıya geri yüklemede işe yarar.
    pub source_home: String,
    pub tool_version: String,
    pub entries: Vec<Entry>,
}

impl Manifest {
    pub fn new(profile: &str, source_home: &str) -> Self {
        Self {
            version: MANIFEST_VERSION,
            profile: profile.to_string(),
            source_home: source_home.to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            entries: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("manifest okunamadı: {}", path.display()))?;
        let manifest: Manifest = serde_json::from_str(&raw)
            .with_context(|| format!("manifest ayrıştırılamadı: {}", path.display()))?;
        Ok(manifest)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Okunabilir ve diff'i temiz olsun diye pretty JSON.
        let raw = serde_json::to_string_pretty(self)?;
        std::fs::write(path, raw)
            .with_context(|| format!("manifest yazılamadı: {}", path.display()))?;
        Ok(())
    }

    pub fn by_repo_path(&self) -> BTreeMap<&str, &Entry> {
        self.entries
            .iter()
            .map(|e| (e.repo_path.as_str(), e))
            .collect()
    }

    pub fn total_size(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }
}
