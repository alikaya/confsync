//! Görsel dil: palet, tipografi, boşluk ve tekrar eden yüzey parçaları.
//!
//! egui'nin varsayılan teması yerine tek aksan renkli koyu bir palet kurulur.
//! Renkler yalnızca burada tanımlanır; görünümler (`views.rs`) doğrudan
//! `Color32::from_rgb(...)` yazmaz, buradaki sabitleri kullanır.

use egui::{Color32, CornerRadius, FontId, Margin, Stroke};

// --- palet ---------------------------------------------------------------

/// Uygulamanın en arka zemini (orta panel).
pub const BG: Color32 = Color32::from_rgb(0x0E, 0x10, 0x14);
/// Üst bar, kenar çubuğu ve durum çubuğu.
pub const PANEL: Color32 = Color32::from_rgb(0x14, 0x17, 0x1D);
/// İçerik kartları.
pub const CARD: Color32 = Color32::from_rgb(0x1A, 0x1E, 0x26);
/// Metin kutusu gibi "gömük" yüzeyler.
pub const SUNKEN: Color32 = Color32::from_rgb(0x10, 0x13, 0x18);
pub const BORDER: Color32 = Color32::from_rgb(0x27, 0x2D, 0x38);

pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE9, 0xEF);
pub const MUTED: Color32 = Color32::from_rgb(0x93, 0x9B, 0xA8);
pub const FAINT: Color32 = Color32::from_rgb(0x6B, 0x74, 0x83);

pub const ACCENT: Color32 = Color32::from_rgb(0x6E, 0x7B, 0xFF);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x8B, 0x95, 0xFF);
/// Seçili satırların arkasındaki soluk aksan.
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x22, 0x26, 0x3D);

pub const SUCCESS: Color32 = Color32::from_rgb(0x46, 0xD0, 0x8B);
pub const WARN: Color32 = Color32::from_rgb(0xE5, 0xA4, 0x4B);
pub const DANGER: Color32 = Color32::from_rgb(0xE8, 0x65, 0x6F);

pub const RADIUS: u8 = 8;

/// Tema, stil ve tipografiyi bağlama uygular. Açılışta bir kez çağrılır.
pub fn apply(ctx: &egui::Context) {
    use egui::{FontFamily::Proportional, TextStyle};

    let mut style = (*ctx.style()).clone();

    style.text_styles = [
        (TextStyle::Heading, FontId::new(19.0, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Small, FontId::new(12.0, Proportional)),
        (
            TextStyle::Monospace,
            FontId::new(13.0, egui::FontFamily::Monospace),
        ),
    ]
    .into();

    let spacing = &mut style.spacing;
    spacing.item_spacing = egui::vec2(10.0, 8.0);
    spacing.button_padding = egui::vec2(12.0, 6.0);
    spacing.menu_margin = Margin::same(6);
    spacing.indent = 20.0;
    spacing.interact_size.y = 26.0;
    spacing.scroll.bar_width = 8.0;
    spacing.scroll.floating = true;

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = None;
    v.panel_fill = PANEL;
    v.window_fill = CARD;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(RADIUS + 2);
    v.extreme_bg_color = SUNKEN;
    v.faint_bg_color = Color32::from_rgb(0x1D, 0x22, 0x2B);
    v.code_bg_color = SUNKEN;
    v.hyperlink_color = ACCENT_HOVER;
    v.warn_fg_color = WARN;
    v.error_fg_color = DANGER;
    v.selection = egui::style::Selection {
        bg_fill: ACCENT_SOFT,
        stroke: Stroke::new(1.0, ACCENT),
    };
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    v.popup_shadow = v.window_shadow;

    // Etkileşim durumları: sakin bir zemin, aksan yalnızca vurgu için.
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = CARD;
    w.noninteractive.weak_bg_fill = CARD;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    w.noninteractive.corner_radius = CornerRadius::same(RADIUS);

    w.inactive.bg_fill = Color32::from_rgb(0x23, 0x28, 0x32);
    w.inactive.weak_bg_fill = Color32::from_rgb(0x1E, 0x23, 0x2C);
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.corner_radius = CornerRadius::same(RADIUS);
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = Color32::from_rgb(0x2C, 0x33, 0x40);
    w.hovered.weak_bg_fill = Color32::from_rgb(0x28, 0x2E, 0x3A);
    w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x3A, 0x42, 0x52));
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.corner_radius = CornerRadius::same(RADIUS);
    w.hovered.expansion = 1.0;

    w.active.bg_fill = ACCENT;
    w.active.weak_bg_fill = Color32::from_rgb(0x33, 0x3B, 0x4B);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.0, TEXT);
    w.active.corner_radius = CornerRadius::same(RADIUS);
    w.active.expansion = 0.0;

    w.open.bg_fill = Color32::from_rgb(0x23, 0x28, 0x32);
    w.open.weak_bg_fill = Color32::from_rgb(0x23, 0x28, 0x32);
    w.open.bg_stroke = Stroke::new(1.0, BORDER);
    w.open.fg_stroke = Stroke::new(1.0, TEXT);
    w.open.corner_radius = CornerRadius::same(RADIUS);

    ctx.set_style(style);
}

// --- yüzeyler ------------------------------------------------------------

/// İçerik kartı: kenarlıklı, yuvarlatılmış, iç boşluklu yüzey.
pub fn card() -> egui::Frame {
    egui::Frame::NONE
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(16, 14))
}

/// Kenarlıksız, yalnızca zeminden ayrılan hafif yüzey (satır grupları için).
pub fn inset() -> egui::Frame {
    egui::Frame::NONE
        .fill(SUNKEN)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
}

/// Üst bar / kenar çubuğu / durum çubuğu zemini.
/// Panelleri ayıran çizgiyi içerik kendisi çizer (bkz. [`hairline`]).
pub fn bar(margin: Margin) -> egui::Frame {
    egui::Frame::NONE.fill(PANEL).inner_margin(margin)
}

/// Panelleri birbirinden ayıran 1 piksellik çizgi.
pub fn hairline(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_at_least(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0, BORDER);
}

// --- parçalar ------------------------------------------------------------

/// Marka işareti. egui'nin gömülü fontlarında `◆` (U+25C6) yok — karakter
/// yerine boş kutu basılıyordu; bu yüzden şekil doğrudan boyanır.
pub fn brand_mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_at_least(egui::vec2(14.0, 14.0), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let c = rect.center();
        let r = 5.5;
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(c.x, c.y - r),
                egui::pos2(c.x + r, c.y),
                egui::pos2(c.x, c.y + r),
                egui::pos2(c.x - r, c.y),
            ],
            ACCENT,
            Stroke::NONE,
        ));
    }
}

/// Durum göstergesi. `●` de fontta yok; daire boyanır.
pub fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_at_least(egui::vec2(9.0, 9.0), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().circle_filled(rect.center(), 3.5, color);
    }
}

/// Satır sonundaki "kaldır" düğmesi: çarpı işareti çizgilerle çizilir.
pub fn close_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_at_least(egui::vec2(24.0, 22.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        if hovered {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), DANGER.gamma_multiply(0.18));
        }
        let c = rect.center();
        let r = 4.0;
        let stroke = Stroke::new(1.4, if hovered { DANGER } else { FAINT });
        ui.painter()
            .line_segment([c + egui::vec2(-r, -r), c + egui::vec2(r, r)], stroke);
        ui.painter()
            .line_segment([c + egui::vec2(r, -r), c + egui::vec2(-r, r)], stroke);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Kenar çubuğundaki gezinme satırı: ikon + etiket, seçiliyken aksan çubuğu.
pub fn nav_item(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool) -> egui::Response {
    let height = 34.0;
    let (rect, response) = ui.allocate_at_least(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, CornerRadius::same(8), ACCENT_SOFT);
            let bar = egui::Rect::from_min_size(
                rect.left_top() + egui::vec2(0.0, 7.0),
                egui::vec2(3.0, height - 14.0),
            );
            painter.rect_filled(bar, CornerRadius::same(2), ACCENT);
        } else if response.hovered() {
            painter.rect_filled(rect, CornerRadius::same(8), CARD);
        }

        let color = if selected { TEXT } else { MUTED };
        painter.text(
            rect.left_center() + egui::vec2(14.0, 0.0),
            egui::Align2::LEFT_CENTER,
            icon,
            FontId::proportional(13.0),
            if selected { ACCENT_HOVER } else { FAINT },
        );
        painter.text(
            rect.left_center() + egui::vec2(38.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(14.0),
            color,
        );
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Sayı + açıklama gösteren küçük kart.
/// Bulunduğu alanın tamamını kaplar; sıra hâlinde `Ui::columns` ile kullanılır.
pub fn stat_tile(ui: &mut egui::Ui, value: &str, label: &str, tint: Color32) {
    card()
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(value)
                        .font(FontId::proportional(22.0))
                        .color(tint),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(label)
                        .font(FontId::proportional(12.0))
                        .color(MUTED),
                );
            });
        });
}

/// Renkli, yuvarlak köşeli küçük etiket.
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        FontId::proportional(11.0),
        color,
    );
    let padding = egui::vec2(7.0, 3.0);
    let size = galley.size() + padding * 2.0;
    let (rect, _) = ui.allocate_at_least(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), color.gamma_multiply(0.18));
        ui.painter().galley(rect.min + padding, galley, color);
    }
}

/// Bölüm başlığı: büyük başlık + isteğe bağlı açıklama satırı.
pub fn title(ui: &mut egui::Ui, text: &str, subtitle: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(FontId::proportional(19.0))
            .color(TEXT),
    );
    if !subtitle.is_empty() {
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(subtitle)
                .font(FontId::proportional(12.5))
                .color(MUTED),
        );
    }
    ui.add_space(12.0);
}

/// Kart içindeki alt başlık.
pub fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(FontId::proportional(13.0))
            .color(FAINT),
    );
}

/// Ana eylem düğmesi: dolu aksan zemin.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_string()).color(Color32::WHITE))
        .fill(ACCENT)
        .corner_radius(CornerRadius::same(RADIUS))
}

/// İkincil eylem: yalnızca kenarlık.
pub fn ghost_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_string()).color(TEXT))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(RADIUS))
}

/// Yıkıcı eylem: kırmızı kenarlık, doldurulmaz.
pub fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_string()).color(DANGER))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0, DANGER.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(RADIUS))
}

/// Uyarı/bilgi şeridi: sol kenarında renk çubuğu olan kutu.
pub fn notice(ui: &mut egui::Ui, color: Color32, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(color.gamma_multiply(0.10))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.vertical(body);
        });
}
