//! Yedekleme kilidi.
//!
//! [`crate::backup::apply`] profil dizinini silip yeniden kurar. Arayüz ile
//! arka plan ajanı aynı anda çalışırsa bu yıkıcı olur (ayrıca git indeks
//! kilidi de çakışır). Bu yüzden yazan her taraf önce buradan kilit alır.
//!
//! Kilit `flock(2)` ile alınır: süreç nasıl sonlanırsa sonlansın çekirdek
//! kilidi bırakır, yani çökmüş bir süreç kalıcı kilit bırakmaz.

use anyhow::{anyhow, Context, Result};
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

/// Tutulduğu sürece kilit geçerlidir; düşünce serbest bırakılır.
pub struct Guard {
    _file: File,
    path: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        // flock, dosya tanıtıcısı kapanınca çekirdek tarafından bırakılır;
        // dosyayı silmiyoruz ki yarış durumu oluşmasın.
        log::debug!("yedekleme kilidi bırakıldı: {}", self.path.display());
    }
}

pub fn lock_path(repo_path: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};

    // Kilit adı deponun **tam yolundan** türetilir. Yalnızca son dizin adı
    // kullanılsaydı farklı yerlerdeki iki "repo" dizini aynı kilidi paylaşır,
    // biri diğerinin yedeklemesini engellerdi.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    repo_path.hash(&mut hasher);
    let name = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".into());

    // Depo dizininin kendisi silinip yeniden kurulabildiği için kilit
    // dosyası veri dizininin kökünde tutulur.
    crate::settings::data_dir().join(format!("{name}-{:016x}.lock", hasher.finish()))
}

/// Kilidi almaya çalışır; başka bir süreç tutuyorsa hemen hata döner.
pub fn acquire(repo_path: &Path) -> Result<Guard> {
    let path = lock_path(repo_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(&path)
        .with_context(|| format!("kilit dosyası açılamadı: {}", path.display()))?;

    // LOCK_EX | LOCK_NB: beklemeden dene.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::WouldBlock {
            return Err(anyhow!(
                "başka bir confsync işlemi şu anda yedekleme yapıyor; \
                 bu çalıştırma atlandı"
            ));
        }
        return Err(anyhow!("kilit alınamadı: {err}"));
    }

    Ok(Guard { _file: file, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ikinci_kilit_beklemeden_reddedilir() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");

        let first = acquire(&repo).expect("ilk kilit alınmalı");
        // flock kilitleri açık dosya tanımına bağlıdır: ayrı `open` ile
        // alınan ikinci tanıtıcı aynı süreçte bile reddedilir.
        assert!(
            acquire(&repo).is_err(),
            "kilit tutulurken ikinci alım reddedilmeli"
        );
        drop(first);

        // Bırakıldıktan sonra yeniden alınabilmeli.
        let _second = acquire(&repo).expect("bırakılan kilit yeniden alınmalı");
    }

    /// Aynı ada sahip iki ayrı depo birbirinin kilidini tutmamalı.
    #[test]
    fn farkli_yollardaki_ayni_adli_depolar_ayri_kilit_kullanir() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let repo_a = a.path().join("repo");
        let repo_b = b.path().join("repo");

        assert_ne!(lock_path(&repo_a), lock_path(&repo_b));

        let _first = acquire(&repo_a).expect("ilk depo kilitlenmeli");
        let _second = acquire(&repo_b).expect("ikinci depo ayrı kilit almalı");
    }
}
