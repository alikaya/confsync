//! confsync-core — GUI'den bağımsız tüm iş mantığı.
//!
//! Bu crate hiçbir arayüz kütüphanesine bağlı değildir; aynı mantık
//! ileride bir CLI ya da systemd servisi tarafından da kullanılabilir.

pub mod backup;
pub mod discover;
pub mod gitrepo;
pub mod job;
pub mod lock;
pub mod manifest;
pub mod paths;
pub mod restore;
pub mod scan;
pub mod secrets;
pub mod settings;

pub use settings::Settings;
