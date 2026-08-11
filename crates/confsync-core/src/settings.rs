//! Uygulama ayarları: kaynak klasörler, hariç tutma kalıpları, repo bilgisi.
//! `~/.config/confsync/settings.toml` içinde saklanır.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Yedeklenecek tek bir kaynak (dosya ya da klasör).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub path: PathBuf,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Kullanıcının arayüzde gördüğü kısa etiket.
    #[serde(default)]
    pub label: String,
}

impl Source {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let label = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        Self { path, enabled: true, label }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Yerel git deposunun bulunduğu dizin.
    pub repo_path: PathBuf,
    /// Uzak depo (opsiyonel). Boşsa push/pull devre dışı.
    pub remote_url: String,
    pub branch: String,
    /// Makine profili. Aynı repoyu birden fazla makine paylaşabilsin diye.
    pub profile: String,
    pub sources: Vec<Source>,
    /// gitignore sözdizimiyle hariç tutma kalıpları.
    pub excludes: Vec<String>,
    /// Sembolik bağlantıların hedefi kopyalansın mı, yoksa bağlantı olarak mı saklansın.
    pub follow_symlinks: bool,
    /// Bu boyutun üzerindeki dosyalar atlanır (MiB).
    pub max_file_size_mb: u64,
    /// Sır (secret) şüphesi taşıyan dosyalar otomatik atlansın mı.
    pub skip_secrets: bool,
    /// Commit sonrası otomatik push.
    pub auto_push: bool,
    pub author_name: String,
    pub author_email: String,
    /// Sır/boyut sezgisine rağmen kullanıcının "yine de al" dediği yollar.
    pub always_include: Vec<PathBuf>,
    /// Kullanıcının "hep atla" dediği yollar.
    pub always_skip: Vec<PathBuf>,
    /// Sorulacak bir şey olmasa da yedeklemeden önce özet penceresi açılsın mı.
    pub always_ask: bool,
    /// Ajanın denetim aralığı (dakika).
    pub agent_interval_min: u64,
    /// Ajan, karar gerektirmeyen değişiklikleri kendiliğinden yedeklesin mi.
    pub agent_auto_backup: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let home = home_dir();
        Self {
            repo_path: data_dir().join("repo"),
            remote_url: String::new(),
            branch: "main".into(),
            profile: default_profile(),
            sources: default_sources(&home),
            excludes: default_excludes(),
            follow_symlinks: false,
            max_file_size_mb: 5,
            skip_secrets: true,
            auto_push: false,
            author_name: "confsync".into(),
            author_email: "confsync@localhost".into(),
            always_include: Vec::new(),
            always_skip: Vec::new(),
            always_ask: false,
            agent_interval_min: 5,
            agent_auto_backup: false,
        }
    }
}

impl Settings {
    pub fn config_file() -> PathBuf {
        config_dir().join("settings.toml")
    }

    /// Diskten yükler; dosya yoksa varsayılanları döndürür.
    pub fn load() -> Result<Self> {
        let path = Self::config_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("ayar dosyası okunamadı: {}", path.display()))?;
        let settings: Settings = toml::from_str(&raw)
            .with_context(|| format!("ayar dosyası ayrıştırılamadı: {}", path.display()))?;
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(&path, raw)
            .with_context(|| format!("ayar dosyası yazılamadı: {}", path.display()))?;
        Ok(())
    }

    pub fn enabled_sources(&self) -> impl Iterator<Item = &Source> {
        self.sources.iter().filter(|s| s.enabled)
    }

    pub fn max_file_size_bytes(&self) -> u64 {
        self.max_file_size_mb.saturating_mul(1024 * 1024)
    }

    /// Kullanıcı bu yol için daha önce "yine de al" dedi mi?
    pub fn is_always_included(&self, path: &Path) -> bool {
        self.always_include.iter().any(|p| p == path)
    }

    /// Kullanıcı bu yol için daha önce "hep atla" dedi mi?
    pub fn is_always_skipped(&self, path: &Path) -> bool {
        self.always_skip.iter().any(|p| p == path)
    }

    /// Bir yolun kalıcı kararını yazar; karşıt listeden düşürür.
    pub fn remember_decision(&mut self, path: &Path, include: bool) {
        self.always_include.retain(|p| p != path);
        self.always_skip.retain(|p| p != path);
        if include {
            self.always_include.push(path.to_path_buf());
        } else {
            self.always_skip.push(path.to_path_buf());
        }
    }

    /// Aynı yol iki kez eklenmesin.
    pub fn add_source(&mut self, path: impl Into<PathBuf>) -> bool {
        let path = path.into();
        if self.sources.iter().any(|s| s.path == path) {
            return false;
        }
        self.sources.push(Source::new(path));
        true
    }
}

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"))
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| home_dir().join(".config"))
        .join("confsync")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| home_dir().join(".local/share"))
        .join("confsync")
}

/// Geri yükleme öncesi alınan güvenlik kopyalarının kökü.
pub fn rollback_dir() -> PathBuf {
    data_dir().join("rollback")
}

pub fn default_profile() -> String {
    let host = gethostname::gethostname().to_string_lossy().to_string();
    if host.is_empty() {
        "default".into()
    } else {
        host
    }
}

/// Varsayılan kaynaklar.
///
/// `~/.config` bilerek **bütün olarak** eklenmez: içinde tarayıcı profilleri ve
/// Electron uygulamalarının yüzlerce megabaytlık durum klasörleri bulunur.
/// Bunun yerine bilinen yapılandırma girdileri tek tek eklenir; gerisi için
/// arayüzdeki keşif paneli kullanılır (bkz. [`crate::discover`]).
fn default_sources(home: &Path) -> Vec<Source> {
    crate::discover::recommended_paths(home)
        .into_iter()
        .map(Source::new)
        .collect()
}

/// Neredeyse her kurulumda dışlanması gereken kalıplar.
/// gitignore sözdizimi: `!` ile başlayanlar istisnadır.
pub fn default_excludes() -> Vec<String> {
    [
        "# --- önbellek ve geçici dosyalar ---",
        "**/[Cc]ache/**",
        "**/cache2/**",
        "**/.cache/**",
        "**/Crash Reports/**",
        "**/*.log",
        "**/*.tmp",
        "**/*.swp",
        "**/*.lock",
        "**/lock",
        "**/*.socket",
        "**/*.sock",
        "# --- oturum ve durum verisi ---",
        "**/Session Storage/**",
        "**/Local Storage/**",
        "**/IndexedDB/**",
        "**/Service Worker/**",
        "**/GPUCache/**",
        "**/Code Cache/**",
        "# --- gizli anahtarlar (varsayılan olarak dışarıda) ---",
        "**/.ssh/id_*",
        "!**/.ssh/id_*.pub",
        "**/*.pem",
        "**/*.key",
        "**/*.p12",
        "**/*.kdbx",
        "**/.gnupg/private-keys-v1.d/**",
        "**/.netrc",
        "**/.aws/credentials",
        "**/.docker/config.json",
        "**/.npmrc",
        "**/.pypirc",
        "# --- büyük/anlamsız içerik ---",
        "**/node_modules/**",
        "**/__pycache__/**",
        // `.git`'in kendisi de elenir: submodule'lerde bu bir dosyadır ve
        // yedek deposunu iç içe depo durumuna sokar. Tarayıcı bunu kalıptan
        // bağımsız olarak da atlar; kalıp yalnızca görünürlük için burada.
        "**/.git",
        "**/.git/**",
        "**/Trash/**",
        "# --- uygulama durumu (yapılandırma değil) ---",
        // Kaynak olarak yanlışlıkla `~/.config` eklenirse ağır olanlar
        // yine de taramaya girmesin diye ikinci bir güvenlik ağı.
        "**/.config/BraveSoftware/**",
        "**/.config/google-chrome*/**",
        "**/.config/chromium/**",
        "**/.config/microsoft-edge/**",
        "**/.config/vivaldi/**",
        "**/.config/opera/**",
        "**/.config/Code/**",
        "**/.config/Code - OSS/**",
        "**/.config/VSCodium/**",
        "**/.config/Cursor/**",
        "**/.config/discord/**",
        "**/.config/vesktop/**",
        "**/.config/Slack/**",
        "**/.config/Signal/**",
        "**/.config/teams*/**",
        "**/.config/telegram-desktop/**",
        "**/.config/Element/**",
        "**/.config/spotify/**",
        "**/.config/obsidian/**",
        "**/.config/logseq/**",
        "**/.config/Postman/**",
        "**/.config/heroic/**",
        "**/.config/lutris/**",
        "**/.config/JetBrains/**",
        "**/.config/libreoffice/**",
        "**/.config/pulse/**",
        "**/.config/dconf/**",
        "**/.config/session/**",
        "**/.config/kdeconnect/**",
        "**/Partitions/**",
        "**/blob_storage/**",
        "**/DawnCache/**",
        "**/ShaderCache/**",
        "**/component_crx_cache/**",
        "**/Extension State/**",
        "**/*.ldb",
        "**/*.sqlite*",
        "**/*.mdb",
        "**/*.pack",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
