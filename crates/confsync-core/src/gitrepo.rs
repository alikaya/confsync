//! git2 (libgit2) sarmalayıcısı.
//!
//! Kimlik doğrulama sırası: ssh-agent → varsayılan anahtar dosyaları →
//! git credential helper. Böylece kullanıcı parolasını uygulamaya girmez.

use anyhow::{anyhow, Context, Result};
use git2::{
    Cred, CredentialType, FetchOptions, IndexAddOption, PushOptions, RemoteCallbacks, Repository,
    Signature,
};
use std::path::Path;

pub const REMOTE_NAME: &str = "origin";

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub author: String,
    pub timestamp: i64,
}

impl CommitInfo {
    pub fn local_time(&self) -> String {
        use chrono::TimeZone;
        chrono::Local
            .timestamp_opt(self.timestamp, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".into())
    }
}

/// Depoyu açar; yoksa oluşturur ve ilk yapılandırmayı yapar.
pub fn open_or_init(path: &Path, branch: &str) -> Result<Repository> {
    if path.join(".git").exists() {
        return Repository::open(path).with_context(|| format!("could not open repository: {}", path.display()));
    }
    std::fs::create_dir_all(path)?;
    let mut opts = git2::RepositoryInitOptions::new();
    opts.initial_head(branch);
    let repo = Repository::init_opts(path, &opts)
        .with_context(|| format!("could not create repository: {}", path.display()))?;
    Ok(repo)
}

/// Uzak adresi ayarlar (varsa günceller, yoksa ekler).
pub fn set_remote(repo: &Repository, url: &str) -> Result<()> {
    if url.trim().is_empty() {
        return Ok(());
    }
    match repo.find_remote(REMOTE_NAME) {
        Ok(existing) => {
            if existing.url() != Some(url) {
                repo.remote_set_url(REMOTE_NAME, url)?;
            }
        }
        Err(_) => {
            repo.remote(REMOTE_NAME, url)?;
        }
    }
    Ok(())
}

/// Çalışma ağacındaki tüm değişiklikleri (silmeler dahil) commit'ler.
/// Değişiklik yoksa `None` döner.
pub fn commit_all(
    repo: &Repository,
    message: &str,
    author_name: &str,
    author_email: &str,
) -> Result<Option<String>> {
    let mut index = repo.index()?;
    // update_all: silinen/değişen izlenen dosyalar. add_all: yeni dosyalar.
    //
    // FORCE şart: yedeklenen ağaçlardan gelen `.gitignore` dosyaları
    // (örn. `~/.config/tmux/plugins/.../.gitignore` içindeki `lib/`)
    // aksi halde yedeğe alınmış dosyaların commit'e girmesini sessizce
    // engeller; manifest dosyayı sayar ama depoda bulunmaz.
    index.update_all(["*"].iter(), None)?;
    index.add_all(["*"].iter(), IndexAddOption::FORCE, None)?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());

    // Ağaç değişmediyse boş commit üretme.
    if let Some(ref parent) = parent {
        if parent.tree_id() == tree_id {
            return Ok(None);
        }
    }

    let signature = Signature::now(author_name, author_email)?;
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    let oid = repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parents,
    )?;
    Ok(Some(oid.to_string()))
}

pub fn log(repo: &Repository, limit: usize) -> Result<Vec<CommitInfo>> {
    let mut revwalk = match repo.revwalk() {
        Ok(r) => r,
        Err(_) => return Ok(Vec::new()),
    };
    if revwalk.push_head().is_err() {
        // Henüz commit yok.
        return Ok(Vec::new());
    }
    revwalk.set_sorting(git2::Sort::TIME)?;

    let mut out = Vec::new();
    for oid in revwalk.take(limit) {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        out.push(CommitInfo {
            id: oid.to_string(),
            short_id: oid.to_string()[..7.min(oid.to_string().len())].to_string(),
            summary: commit.summary().unwrap_or("(no subject)").to_string(),
            author: commit.author().name().unwrap_or("-").to_string(),
            timestamp: commit.time().seconds(),
        });
    }
    Ok(out)
}

fn callbacks() -> RemoteCallbacks<'static> {
    let mut cb = RemoteCallbacks::new();
    // Aynı bağlantıda birden çok kez çağrılabildiği için sırayı takip ediyoruz.
    let mut attempt = 0usize;
    cb.credentials(move |url, username, allowed| {
        let user = username.unwrap_or("git");
        attempt += 1;

        if allowed.contains(CredentialType::SSH_KEY) {
            match attempt {
                1 => return Cred::ssh_key_from_agent(user),
                2 => {
                    let home = crate::settings::home_dir();
                    for name in ["id_ed25519", "id_rsa", "id_ecdsa"] {
                        let key = home.join(".ssh").join(name);
                        if key.exists() {
                            return Cred::ssh_key(user, None, &key, None);
                        }
                    }
                }
                _ => {}
            }
        }

        if allowed.contains(CredentialType::USER_PASS_PLAINTEXT) {
            if let Ok(config) = git2::Config::open_default() {
                return Cred::credential_helper(&config, url, username);
            }
        }

        if allowed.contains(CredentialType::DEFAULT) {
            return Cred::default();
        }

        Err(git2::Error::from_str(
            "authentication failed: no ssh-agent, ssh key or credential helper found",
        ))
    });
    cb
}

pub fn push(repo: &Repository, branch: &str) -> Result<()> {
    let mut remote = repo
        .find_remote(REMOTE_NAME)
        .context("no remote configured")?;
    let mut opts = PushOptions::new();
    opts.remote_callbacks(callbacks());
    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    remote
        .push(&[refspec.as_str()], Some(&mut opts))
        .context("push failed")?;
    Ok(())
}

/// Uzaktan çeker ve yalnızca fast-forward ise birleştirir.
/// Ayrışma varsa hata döndürür; çakışma çözümü kullanıcıya bırakılır.
pub fn pull_fast_forward(repo: &Repository, branch: &str) -> Result<PullOutcome> {
    let mut remote = repo
        .find_remote(REMOTE_NAME)
        .context("no remote configured")?;

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks());
    remote
        .fetch(&[branch], Some(&mut fetch_opts), None)
        .context("fetch failed")?;

    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;
    let (analysis, _) = repo.merge_analysis(&[&fetch_commit])?;

    if analysis.is_up_to_date() {
        return Ok(PullOutcome::UpToDate);
    }
    if analysis.is_fast_forward() || analysis.is_unborn() {
        let refname = format!("refs/heads/{branch}");
        match repo.find_reference(&refname) {
            Ok(mut reference) => {
                reference.set_target(fetch_commit.id(), "confsync: fast-forward")?;
            }
            Err(_) => {
                repo.reference(&refname, fetch_commit.id(), true, "confsync: first fetch")?;
            }
        }
        repo.set_head(&refname)?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
        return Ok(PullOutcome::FastForwarded);
    }

    Err(anyhow!(
        "local and remote branches have diverged; merge manually"
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullOutcome {
    UpToDate,
    FastForwarded,
}

/// Depoda commit'lenmemiş değişiklik var mı?
pub fn is_dirty(repo: &Repository) -> Result<bool> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true).include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    Ok(!statuses.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Yedeklenen ağaçtan gelen bir `.gitignore`, yedeğe alınmış dosyayı
    /// commit dışında bırakmamalı: manifest onu sayarken depoda bulunmazsa
    /// geri yükleme sessizce eksik kalır.
    #[test]
    fn yedeklenen_gitignore_commit_icerigini_daraltmaz() {
        let dir = tempfile::tempdir().unwrap();
        let repo = open_or_init(dir.path(), "main").unwrap();

        let files = dir.path().join("files/lib");
        std::fs::create_dir_all(&files).unwrap();
        // tpm/lib/tmux-test/.gitignore ile aynı içerik: `lib/`.
        std::fs::write(dir.path().join("files/.gitignore"), "lib/\n").unwrap();
        std::fs::write(files.join("app.conf"), "ayar").unwrap();

        let id = commit_all(&repo, "test", "confsync", "confsync@localhost")
            .unwrap()
            .expect("commit oluşmalı");

        let commit = repo.find_commit(git2::Oid::from_str(&id).unwrap()).unwrap();
        let tree = commit.tree().unwrap();
        assert!(
            tree.get_path(Path::new("files/lib/app.conf")).is_ok(),
            "`.gitignore` kuralı yedeklenen dosyayı commit dışı bıraktı"
        );
    }
}

/// Son commit'in zamanı (unix saniye); depo yoksa `None`.
/// [`open_or_init`]'ten farkı: depoyu **oluşturmaz**, salt okurdur.
pub fn last_commit_time(repo_path: &Path) -> Option<i64> {
    let repo = Repository::open(repo_path).ok()?;
    log(&repo, 1).ok()?.first().map(|c| c.timestamp)
}
