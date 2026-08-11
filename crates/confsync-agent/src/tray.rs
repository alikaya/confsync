//! Tray ikonu (StatusNotifierItem) ve menüsü.
//!
//! İkon dosyadan yüklenmez, kod içinde çizilir: kurulum sırasında ikon
//! temasına dosya kopyalamak gerekmez, ikon her masaüstünde aynı görünür.

use crate::Cmd;
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Icon, ToolTip};
use std::sync::mpsc::Sender;

/// Ajanın kullanıcıya gösterdiği durum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Kaynaklar son yedekle aynı.
    UpToDate,
    /// Değişiklik var, yedeklenmeyi bekliyor.
    Changes { summary: String },
    /// Değişiklik var ama önce kullanıcı kararı gerekiyor.
    NeedsReview { count: usize },
    Working,
    Paused,
    Error { message: String },
}

impl State {
    fn line(&self) -> String {
        match self {
            State::UpToDate => "Her şey yedeklendi".into(),
            State::Changes { summary } => format!("Bekleyen değişiklik: {summary}"),
            State::NeedsReview { count } => {
                format!("{count} dosya kararınızı bekliyor")
            }
            State::Working => "Yedekleniyor…".into(),
            State::Paused => "Duraklatıldı".into(),
            State::Error { message } => format!("Hata: {message}"),
        }
    }

    /// İkonun ana rengi (R, G, B) — arayüzdeki paletle aynı.
    fn color(&self) -> (u8, u8, u8) {
        match self {
            State::UpToDate => (0x46, 0xD0, 0x8B),   // yeşil
            State::Changes { .. } => (0x6E, 0x7B, 0xFF), // aksan
            State::NeedsReview { .. } => (0xE5, 0xA4, 0x4B), // turuncu
            State::Working => (0x8B, 0x95, 0xFF),
            State::Paused => (0x6B, 0x74, 0x83), // soluk
            State::Error { .. } => (0xE8, 0x65, 0x6F), // kırmızı
        }
    }
}

pub struct Tray {
    pub state: State,
    pub last_check: String,
    pub paused: bool,
    pub tx: Sender<Cmd>,
}

impl Tray {
    pub fn new(tx: Sender<Cmd>) -> Self {
        Self {
            state: State::UpToDate,
            last_check: "henüz denetlenmedi".into(),
            paused: false,
            tx,
        }
    }

    fn send(&self, cmd: Cmd) {
        if self.tx.send(cmd).is_err() {
            log::warn!("ana döngü kapanmış, komut iletilemedi");
        }
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "confsync".into()
    }

    fn title(&self) -> String {
        "confsync".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let (r, g, b) = self.state.color();
        vec![diamond(22, r, g, b), diamond(44, r, g, b)]
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "confsync".into(),
            description: format!("{}\nSon denetim: {}", self.state.line(), self.last_check),
            ..Default::default()
        }
    }

    /// Sol tık: arayüzü aç.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Cmd::OpenGui);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let busy = self.state == State::Working;
        vec![
            StandardItem {
                label: self.state.line(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("Son denetim: {}", self.last_check),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Şimdi denetle".into(),
                enabled: !busy,
                activate: Box::new(|tray: &mut Self| tray.send(Cmd::CheckNow)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Şimdi yedekle".into(),
                enabled: !busy,
                activate: Box::new(|tray: &mut Self| tray.send(Cmd::BackupNow)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "confsync'i aç".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Cmd::OpenGui)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: if self.paused {
                    "Denetimi sürdür".into()
                } else {
                    "Denetimi duraklat".to_string()
                },
                activate: Box::new(|tray: &mut Self| tray.send(Cmd::TogglePause)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Çık".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Cmd::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Durum rengiyle boyanmış eşkenar dörtgen (arayüzdeki marka işaretiyle aynı).
/// Veri ARGB32, ağ bayt sırası: her piksel [A, R, G, B].
fn diamond(size: i32, r: u8, g: u8, b: u8) -> Icon {
    let n = size as f32;
    let center = (n - 1.0) / 2.0;
    // Kenardan biraz boşluk bırak: panelde sıkışık görünmesin.
    let radius = center * 0.86;
    let mut data = Vec::with_capacity((size * size * 4) as usize);

    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - center).abs();
            let dy = (y as f32 - center).abs();
            // |dx| + |dy| <= r  →  eşkenar dörtgen.
            let distance = dx + dy;
            // Kenarda 1 piksellik yumuşatma; aksi halde köşeler tırtıklı çıkar.
            let alpha = if distance <= radius - 1.0 {
                255.0
            } else if distance >= radius + 1.0 {
                0.0
            } else {
                (radius + 1.0 - distance) / 2.0 * 255.0
            };
            let alpha = alpha.clamp(0.0, 255.0) as u8;
            data.extend_from_slice(&[alpha, r, g, b]);
        }
    }

    Icon {
        width: size,
        height: size,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ikon_boyutu_argb32_ile_tutarli() {
        let icon = diamond(22, 1, 2, 3);
        assert_eq!(icon.data.len(), 22 * 22 * 4);
        // Merkez piksel tamamen opak ve istenen renkte olmalı.
        let center = ((11 * 22 + 11) * 4) as usize;
        assert_eq!(&icon.data[center..center + 4], &[255, 1, 2, 3]);
        // Köşe piksel tamamen saydam olmalı (ARGB sırasında alfa ilk bayt).
        assert_eq!(icon.data[0], 0);
    }
}
