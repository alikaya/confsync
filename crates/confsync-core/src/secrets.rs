//! Sır (secret) sezgisi.
//!
//! Amaç kesin tespit değil; kullanıcıyı yanlışlıkla özel anahtar/token
//! push etmekten korumak. Yanlış pozitif, sızıntıya yeğdir.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretReason {
    /// Dosya adı bilinen bir anahtar/kimlik dosyasıyla eşleşiyor.
    KnownName,
    /// İçerik PEM benzeri bir özel anahtar başlığı taşıyor.
    PrivateKeyHeader,
    /// İçerikte `token=`, `api_key=` gibi atamalar var.
    CredentialAssignment,
}

impl SecretReason {
    pub fn description(&self) -> &'static str {
        match self {
            SecretReason::KnownName => "file name matches a known credential or key file",
            SecretReason::PrivateKeyHeader => "content contains a private key header",
            SecretReason::CredentialAssignment => "content contains a password or token assignment",
        }
    }
}

const SECRET_FILE_NAMES: &[&str] = &[
    "credentials",
    ".netrc",
    ".pgpass",
    ".my.cnf",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
    "secrets.yaml",
    "secrets.yml",
    "secrets.json",
    ".env",
    ".env.local",
    ".npmrc",
    ".pypirc",
    "shadow",
];

const SECRET_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "jks", "keystore", "kdbx", "gpg"];

const KEY_HEADERS: &[&str] = &[
    "-----BEGIN RSA PRIVATE KEY-----",
    "-----BEGIN OPENSSH PRIVATE KEY-----",
    "-----BEGIN PRIVATE KEY-----",
    "-----BEGIN EC PRIVATE KEY-----",
    "-----BEGIN DSA PRIVATE KEY-----",
    "-----BEGIN PGP PRIVATE KEY BLOCK-----",
];

const CREDENTIAL_KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "access_token",
    "auth_token",
    "private_key",
    "client_secret",
];

/// Dosya adına bakarak hızlı ön eleme yapar (içerik okumadan).
pub fn suspicious_name(path: &Path) -> bool {
    let name = match path.file_name().and_then(|s| s.to_str()) {
        Some(n) => n.to_ascii_lowercase(),
        None => return false,
    };
    if SECRET_FILE_NAMES.iter().any(|n| *n == name) {
        return true;
    }
    // `.pub` uzantılı açık anahtarlar zararsız.
    if name.ends_with(".pub") {
        return false;
    }
    path.extension()
        .and_then(|s| s.to_str())
        .map(|e| SECRET_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// İçeriğin ilk parçasını inceleyerek sır olup olmadığını tahmin eder.
///
/// Not: Dosya izninin 0600 olması bilinçli olarak *kanıt sayılmaz*.
/// Pek çok meşru yapılandırma dosyası da 0600'dür; bunu ölçüt yapmak
/// kullanıcının yedeğinden sessizce dosya düşürür.
pub fn inspect(path: &Path, contents: &[u8]) -> Option<SecretReason> {
    if suspicious_name(path) {
        return Some(SecretReason::KnownName);
    }

    // Yalnızca metin dosyalarında içerik taraması yapılır.
    let head_len = contents.len().min(8 * 1024);
    let head = &contents[..head_len];
    if let Ok(text) = std::str::from_utf8(head) {
        if KEY_HEADERS.iter().any(|h| text.contains(h)) {
            return Some(SecretReason::PrivateKeyHeader);
        }
        if has_credential_assignment(text) {
            return Some(SecretReason::CredentialAssignment);
        }
    }

    None
}

/// `password = "..."` gibi *değer atanmış* satırları arar.
/// Boş değerler ve yorum satırları yok sayılır ki şablon dosyaları elenmesin.
fn has_credential_assignment(text: &str) -> bool {
    for line in text.lines().take(400) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        let Some(sep) = lower.find(['=', ':']) else {
            continue;
        };
        let (key, value) = lower.split_at(sep);
        let key = key.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
        if !CREDENTIAL_KEYS.iter().any(|k| key.ends_with(k)) {
            continue;
        }
        let value = value[1..].trim().trim_matches(['"', '\'']);
        // Şablon yer tutucuları ("<your-key>", "changeme", "") sır sayılmaz.
        if value.len() >= 8
            && !value.starts_with('<')
            && !value.starts_with('$')
            && value != "changeme"
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detects_known_names() {
        assert!(suspicious_name(Path::new("/home/a/.ssh/id_ed25519")));
        assert!(suspicious_name(Path::new("/home/a/cert.pem")));
        assert!(!suspicious_name(Path::new("/home/a/.ssh/id_ed25519.pub")));
        assert!(!suspicious_name(Path::new("/home/a/.bashrc")));
    }

    #[test]
    fn detects_pem_header() {
        let body = b"-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n";
        assert_eq!(
            inspect(Path::new("/tmp/x"), body),
            Some(SecretReason::PrivateKeyHeader)
        );
    }

    #[test]
    fn ignores_placeholder_values() {
        let body = b"password = \"<your-password>\"\n";
        assert_eq!(inspect(Path::new("/tmp/x.conf"), body), None);
    }

    #[test]
    fn catches_real_looking_token() {
        let body = b"api_key = \"a1b2c3d4e5f6g7h8\"\n";
        assert_eq!(
            inspect(Path::new("/tmp/x.conf"), body),
            Some(SecretReason::CredentialAssignment)
        );
    }
}
