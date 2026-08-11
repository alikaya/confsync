//! `~/.config` keşfi.
//!
//! `~/.config` klasörünün tamamını yedeklemek pratikte işe yaramaz: içinde
//! tarayıcı profilleri, Electron uygulamalarının durum dosyaları ve yüzlerce
//! megabaytlık önbellek bulunur. Bu modül klasörü bir seviye derinlikte gezer,
//! her adayın boyutunu ölçer ve üç gruba ayırır:
//!
//! * [`Verdict::Recommended`] — bilinen, küçük, taşınmaya değer yapılandırma.
//! * [`Verdict::Optional`] — zararsız görünüyor; kararı kullanıcı verir.
//! * [`Verdict::Heavy`] — uygulama durumu ya da önbellek; yedeğe girmemeli.
//!
//! Karar iki kaynaktan gelir: bilinen isim listeleri ve ölçülen boyut/dosya
//! sayısı. İsim listesi eksik kalırsa boyut eşiği yakalar.

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Bu boyutun üzerindeki bir `.config` girdisi yapılandırma değil, veri sayılır.
const HEAVY_BYTES: u64 = 20 * 1024 * 1024;
/// Bu kadar dosya içeren bir girdi de yapılandırma değil, veri sayılır.
const HEAVY_FILES: usize = 800;
/// Tek bir adayı ölçerken gezilecek azami dosya sayısı.
/// Tarayıcı profilleri yüz binlerce dosya içerebiliyor; ölçüm oraya takılmasın.
const MEASURE_LIMIT: usize = 20_000;

/// Neredeyse her kurulumda taşınmaya değer olan `.config` girdileri.
/// Sır barındırdığı bilinenler (`gh`, `rclone`, `sops` …) bilerek dışarıda:
/// onlar [`Verdict::Optional`] olarak görünür, kararı kullanıcı verir.
pub const CURATED: &[&str] = &[
    // kabuk & terminal
    "alacritty",
    "kitty",
    "wezterm",
    "foot",
    "ghostty",
    "fish",
    "nushell",
    "starship.toml",
    "zellij",
    "tmux",
    // editörler
    "nvim",
    "vim",
    "helix",
    "kakoune",
    "micro",
    // pencere yöneticileri & masaüstü kabuğu
    "hypr",
    "sway",
    "i3",
    "i3status",
    "i3blocks",
    "waybar",
    "polybar",
    "eww",
    "ags",
    "bspwm",
    "sxhkd",
    "awesome",
    "xmonad",
    "qtile",
    "picom",
    "kanshi",
    "wlogout",
    "swaylock",
    "swayidle",
    "hyprpaper",
    // bildirim & başlatıcı
    "dunst",
    "mako",
    "rofi",
    "wofi",
    // araçlar
    "btop",
    "htop",
    "bat",
    "lazygit",
    "ranger",
    "yazi",
    "zathura",
    "mpv",
    "neofetch",
    "fastfetch",
    "direnv",
    "tig",
    "keyd",
    "git",
    "systemd",
    // masaüstü entegrasyonu
    "gtk-3.0",
    "gtk-4.0",
    "fontconfig",
    "mimeapps.list",
    "user-dirs.dirs",
    "electron-flags.conf",
];

/// Yapılandırma değil, uygulama durumu tutan bilinen klasörler.
/// Karşılaştırma büyük/küçük harf duyarsızdır.
pub const HEAVY_APPS: &[&str] = &[
    // tarayıcılar
    "bravesoftware",
    "google-chrome",
    "google-chrome-beta",
    "chromium",
    "microsoft-edge",
    "vivaldi",
    "opera",
    "yandex-browser",
    "thorium",
    // electron & sohbet
    "code",
    "code - oss",
    "vscodium",
    "cursor",
    "windsurf",
    "zed",
    "discord",
    "discordcanary",
    "vesktop",
    "slack",
    "signal",
    "telegram-desktop",
    "element",
    "ferdium",
    "whatsapp-for-linux",
    "teams",
    "teams-for-linux",
    "zoom",
    "skypeforlinux",
    "notion-app",
    "obsidian",
    "logseq",
    "figma-linux",
    "postman",
    "insomnia",
    // oyun & medya
    "spotify",
    "heroic",
    "lutris",
    "unity3d",
    "epicgameslauncher",
    // ağır masaüstü verisi
    "chromium-flags.conf.d",
    "pulse",
    "dconf",
    "session",
    "goa-1.0",
    "libreoffice",
    "gimp",
    "darktable",
    "blender",
    "jetbrains",
    "kdeconnect",
    "menus",
    "enchant",
    "ibus",
];

/// Ev dizininde kök seviyede taşınmaya değer klasik dosyalar.
pub const HOME_DOTFILES: &[&str] = &[
    ".bashrc",
    ".bash_profile",
    ".bash_aliases",
    ".zshrc",
    ".zshenv",
    ".zprofile",
    ".profile",
    ".inputrc",
    ".gitconfig",
    ".gitignore_global",
    ".vimrc",
    ".tmux.conf",
    ".editorconfig",
    ".Xresources",
    ".xinitrc",
    ".xprofile",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    Recommended,
    Optional,
    Heavy,
}

impl Verdict {
    pub fn label(&self) -> &'static str {
        match self {
            Verdict::Recommended => "önerilir",
            Verdict::Optional => "isteğe bağlı",
            Verdict::Heavy => "ağır",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub path: PathBuf,
    /// `.config` altındaki girdi adı (`nvim`, `starship.toml` …).
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub files: usize,
    /// Ölçüm [`MEASURE_LIMIT`]'e takıldıysa boyut alt sınırdır.
    pub truncated: bool,
    pub verdict: Verdict,
    pub reason: String,
}

/// Varsayılan kaynak listesi için ucuz (ölçüm yapmayan) öneri üretir.
/// Yalnızca varlık kontrolü yapar; ilk açılışta arayüzü bekletmemek için.
pub fn recommended_paths(home: &Path) -> Vec<PathBuf> {
    let config = home.join(".config");
    let mut out: Vec<PathBuf> = HOME_DOTFILES
        .iter()
        .map(|name| home.join(name))
        .filter(|p| p.exists())
        .collect();

    out.extend(
        CURATED
            .iter()
            .map(|name| config.join(name))
            .filter(|p| p.exists()),
    );

    let apps = home.join(".local/share/applications");
    if apps.exists() {
        out.push(apps);
    }
    out
}

/// `~/.config` girdilerini ölçüp sınıflandırır.
///
/// `progress` her girdi için ölçümden **önce** çağrılır; `false` dönerse
/// tarama o noktada durur ve o ana kadarki sonuçlar döner.
pub fn config_candidates(home: &Path, mut progress: impl FnMut(&Path) -> bool) -> Vec<Candidate> {
    let config_dir = home.join(".config");
    let Ok(entries) = std::fs::read_dir(&config_dir) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // confsync'in kendi ayarları yedeğe girmesin.
        if name == "confsync" {
            continue;
        }
        if !progress(&path) {
            break;
        }

        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let (size, files, truncated) = measure(&path, is_dir);
        let (verdict, reason) = classify(&name, size, files, truncated);

        out.push(Candidate {
            path,
            name,
            is_dir,
            size,
            files,
            truncated,
            verdict,
            reason,
        });
    }

    // Önce öneriler, sonra isteğe bağlılar; her grup büyükten küçüğe.
    out.sort_by(|a, b| {
        a.verdict
            .cmp(&b.verdict)
            .then(b.size.cmp(&a.size))
            .then(a.name.cmp(&b.name))
    });
    out
}

/// Bir yolun diskte kapladığı yeri ölçer (dosya ya da klasör).
/// Hariç tutma kalıpları uygulanmaz; ham disk boyutudur.
pub fn measure_path(path: &Path) -> (u64, usize, bool) {
    measure(path, path.is_dir())
}

fn measure(path: &Path, is_dir: bool) -> (u64, usize, bool) {
    if !is_dir {
        let size = std::fs::symlink_metadata(path).map(|m| m.len()).unwrap_or(0);
        return (size, 1, false);
    }

    let mut size = 0u64;
    let mut files = 0usize;
    for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
        if files >= MEASURE_LIMIT {
            return (size, files, true);
        }
        if entry.file_type().is_file() {
            if let Ok(meta) = entry.metadata() {
                size += meta.len();
                files += 1;
            }
        }
    }
    (size, files, false)
}

fn classify(name: &str, size: u64, files: usize, truncated: bool) -> (Verdict, String) {
    let lower = name.to_ascii_lowercase();

    if HEAVY_APPS.iter().any(|app| *app == lower) {
        return (
            Verdict::Heavy,
            "uygulama durumu/önbelleği tutar".to_string(),
        );
    }

    let curated = CURATED.iter().any(|c| *c == name);
    let big = truncated || size > HEAVY_BYTES || files > HEAVY_FILES;

    match (curated, big) {
        (true, false) => (Verdict::Recommended, "bilinen yapılandırma".to_string()),
        // Bilinen bir yapılandırma beklenmedik şekilde şişmişse yine de
        // kullanıcıya soralım; körlemesine eklemek yedeği büyütür.
        (true, true) => (
            Verdict::Optional,
            "bilinen yapılandırma, ama beklenenden büyük".to_string(),
        ),
        (false, true) => (Verdict::Heavy, size_reason(size, files, truncated)),
        (false, false) => (Verdict::Optional, "küçük, içeriği bilinmiyor".to_string()),
    }
}

fn size_reason(size: u64, files: usize, truncated: bool) -> String {
    if truncated {
        format!("{MEASURE_LIMIT}+ dosya")
    } else if size > HEAVY_BYTES {
        format!("{} MiB", size / (1024 * 1024))
    } else {
        format!("{files} dosya")
    }
}

/// `~/.config`'in tamamı tek kaynak olarak eklenmiş mi?
/// Eski ayar dosyalarından gelen bu durumu arayüz uyarı olarak gösterir.
pub fn is_whole_config_dir(path: &Path, home: &Path) -> bool {
    path == home.join(".config")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bilinen_agir_uygulama_ayiklanir() {
        let (verdict, _) = classify("BraveSoftware", 1024, 3, false);
        assert_eq!(verdict, Verdict::Heavy);
        // Büyük/küçük harf farkı sonucu değiştirmemeli.
        let (verdict, _) = classify("Code", 1024, 3, false);
        assert_eq!(verdict, Verdict::Heavy);
    }

    #[test]
    fn bilinen_yapilandirma_onerilir() {
        let (verdict, _) = classify("nvim", 500 * 1024, 40, false);
        assert_eq!(verdict, Verdict::Recommended);
    }

    #[test]
    fn sisen_bilinen_yapilandirma_karari_kullaniciya_birakir() {
        let (verdict, _) = classify("nvim", HEAVY_BYTES + 1, 40, false);
        assert_eq!(verdict, Verdict::Optional);
    }

    #[test]
    fn listede_olmayan_buyuk_klasor_agir_sayilir() {
        let (verdict, reason) = classify("bilinmeyen-uygulama", 64 * 1024 * 1024, 10, false);
        assert_eq!(verdict, Verdict::Heavy);
        assert!(reason.contains("MiB"));
    }

    #[test]
    fn listede_olmayan_kucuk_klasor_istege_bagli() {
        let (verdict, _) = classify("bilinmeyen-uygulama", 4 * 1024, 6, false);
        assert_eq!(verdict, Verdict::Optional);
    }

    #[test]
    fn tum_config_klasoru_taninir() {
        let home = Path::new("/home/test");
        assert!(is_whole_config_dir(&home.join(".config"), home));
        assert!(!is_whole_config_dir(&home.join(".config/nvim"), home));
    }
}
