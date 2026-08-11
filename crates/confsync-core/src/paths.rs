//! Mutlak dosya sistemi yolları ile depo içi göreli yollar arasındaki eşleme.
//!
//! Depo düzeni:
//! ```text
//! <repo>/
//!   profiles/<profil>/
//!     manifest.json
//!     files/
//!       home/.bashrc            <- $HOME/.bashrc
//!       home/.config/nvim/...   <- $HOME/.config/nvim/...
//!       root/etc/hosts          <- /etc/hosts
//! ```
//!
//! `home/` öneki sayesinde depo başka bir kullanıcı adına da geri yüklenebilir.

use anyhow::{anyhow, Result};
use std::path::{Component, Path, PathBuf};

pub const HOME_PREFIX: &str = "home";
pub const ROOT_PREFIX: &str = "root";

/// Mutlak yolu depo içi göreli yola çevirir.
pub fn to_repo_rel(abs: &Path, home: &Path) -> Result<PathBuf> {
    if !abs.is_absolute() {
        return Err(anyhow!("mutlak yol bekleniyordu: {}", abs.display()));
    }
    if let Ok(rest) = abs.strip_prefix(home) {
        return Ok(Path::new(HOME_PREFIX).join(rest));
    }
    let rest = abs.strip_prefix("/")?;
    Ok(Path::new(ROOT_PREFIX).join(rest))
}

/// Depo içi göreli yolu, hedef makinedeki mutlak yola geri çevirir.
pub fn from_repo_rel(rel: &Path, home: &Path) -> Result<PathBuf> {
    let mut comps = rel.components();
    let first = comps
        .next()
        .ok_or_else(|| anyhow!("boş depo yolu"))?;
    let rest: PathBuf = comps.as_path().to_path_buf();
    match first {
        Component::Normal(p) if p == HOME_PREFIX => Ok(home.join(rest)),
        Component::Normal(p) if p == ROOT_PREFIX => Ok(Path::new("/").join(rest)),
        other => Err(anyhow!(
            "bilinmeyen depo yolu öneki: {:?}",
            other.as_os_str()
        )),
    }
}

/// `$HOME` altındaki yolları `~/...` biçiminde kısaltır (arayüzde göstermek için).
pub fn display_short(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// `~` ile başlayan kullanıcı girdisini genişletir.
pub fn expand_tilde(input: &str, home: &Path) -> PathBuf {
    let trimmed = input.trim();
    if trimmed == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(trimmed)
}

/// Bir yolun `/`, `$HOME` gibi tehlikeli derecede geniş olup olmadığını kontrol eder.
/// Arayüz bu durumda kullanıcıyı uyarır (engellemez).
pub fn is_too_broad(path: &Path, home: &Path) -> bool {
    matches!(path.to_str(), Some("/") | Some("/usr") | Some("/var")) || path == home
}

pub fn profile_dir(repo: &Path, profile: &str) -> PathBuf {
    repo.join("profiles").join(profile)
}

pub fn profile_files_dir(repo: &Path, profile: &str) -> PathBuf {
    profile_dir(repo, profile).join("files")
}

pub fn manifest_path(repo: &Path, profile: &str) -> PathBuf {
    profile_dir(repo, profile).join("manifest.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_roundtrip() {
        let home = Path::new("/home/ali");
        let abs = Path::new("/home/ali/.config/nvim/init.lua");
        let rel = to_repo_rel(abs, home).unwrap();
        assert_eq!(rel, Path::new("home/.config/nvim/init.lua"));
        assert_eq!(from_repo_rel(&rel, home).unwrap(), abs);
    }

    #[test]
    fn root_roundtrip() {
        let home = Path::new("/home/ali");
        let abs = Path::new("/etc/hosts");
        let rel = to_repo_rel(abs, home).unwrap();
        assert_eq!(rel, Path::new("root/etc/hosts"));
        assert_eq!(from_repo_rel(&rel, home).unwrap(), abs);
    }

    #[test]
    fn remaps_to_other_user() {
        let rel = Path::new("home/.bashrc");
        let out = from_repo_rel(rel, Path::new("/home/veli")).unwrap();
        assert_eq!(out, Path::new("/home/veli/.bashrc"));
    }
}
