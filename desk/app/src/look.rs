//! How the app's own windows look: one palette, one set of sizes, cards with round corners, and a colour for
//! each type of mon. The panel and the friends' small window both dress themselves here, so that they are of a
//! piece.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle};

/// The ground everything is on, a card on it, and a card's own inset (a field, a bar's track).
pub const GROUND: Color32 = Color32::from_rgb(250, 246, 237);
pub const CARD: Color32 = Color32::from_rgb(255, 252, 245);
pub const INSET: Color32 = Color32::from_rgb(234, 226, 210);
pub const LINE: Color32 = Color32::from_rgb(220, 211, 196);
/// Words: plain, and those that matter less.
pub const INK: Color32 = Color32::from_rgb(47, 41, 39);
pub const FAINT: Color32 = Color32::from_rgb(117, 107, 97);
/// Pokédex red for navigation, yellow for points, and green for earned progress.
pub const ACCENT: Color32 = Color32::from_rgb(196, 61, 56);
pub const GOLD: Color32 = Color32::from_rgb(246, 214, 123);
pub const XP: Color32 = Color32::from_rgb(121, 167, 75);
pub const SELECTED: Color32 = Color32::from_rgb(221, 235, 198);
/// Good, not good, and a warning between them.
pub const GOOD: Color32 = Color32::from_rgb(74, 111, 47);
pub const BAD: Color32 = Color32::from_rgb(177, 46, 44);
pub const WARM: Color32 = Color32::from_rgb(145, 101, 41);

/// Dresses an egui context: the palette, round corners, and sizes of words that go from a heading down to a
/// note in four steps.
pub fn dress(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::light());
    ctx.all_styles_mut(|style| {
        for (text, size) in [(TextStyle::Heading, 20.0), (TextStyle::Body, 14.0), (TextStyle::Button, 13.5), (TextStyle::Small, 12.0), (TextStyle::Monospace, 12.5)] {
            let family = if text == TextStyle::Monospace { FontFamily::Monospace } else { FontFamily::Proportional };
            style.text_styles.insert(text, FontId::new(size, family));
        }
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        let visuals = &mut style.visuals;
        visuals.panel_fill = GROUND;
        visuals.window_fill = CARD;
        visuals.extreme_bg_color = INSET;
        visuals.faint_bg_color = CARD;
        visuals.override_text_color = None;
        visuals.weak_text_color = Some(FAINT);
        visuals.hyperlink_color = ACCENT;
        visuals.selection.bg_fill = ACCENT;
        visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.window_stroke = Stroke::new(1.0, LINE);
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = CornerRadius::same(8);
        let round = CornerRadius::same(7);
        for (widget, fill) in [
            (&mut visuals.widgets.noninteractive, CARD),
            (&mut visuals.widgets.inactive, CARD),
            (&mut visuals.widgets.hovered, Color32::from_rgb(244, 235, 221)),
            (&mut visuals.widgets.active, Color32::from_rgb(229, 216, 196)),
            (&mut visuals.widgets.open, Color32::from_rgb(244, 235, 221)),
        ] {
            widget.corner_radius = round;
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = Stroke::new(1.0, LINE);
            widget.fg_stroke = Stroke::new(1.0, INK);
        }
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    });
}

/// A card: a round-cornered block as wide as there is room, for one thing and what belongs to it.
pub fn card<R>(ui: &mut egui::Ui, inside: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            inside(ui)
        })
        .inner
}

/// A card for something to be pleased about or to mind: the same, with a line of colour down its near side.
pub fn marked<R>(ui: &mut egui::Ui, colour: Color32, inside: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let made = egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0, LINE)).corner_radius(CornerRadius::same(10)).inner_margin(Margin { left: 14, right: 10, top: 10, bottom: 10 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        inside(ui)
    });
    let edge = made.response.rect;
    ui.painter().rect_filled(egui::Rect::from_min_max(edge.left_top(), egui::pos2(edge.left() + 4.0, edge.bottom())), CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 }, colour);
    made.inner
}

/// The colour a type of mon is known by.
pub fn type_colour(name: &str) -> Color32 {
    let (r, g, b) = match name {
        "normal" => (168, 167, 122),
        "fire" => (238, 129, 48),
        "water" => (99, 144, 240),
        "electric" => (247, 208, 44),
        "grass" => (122, 199, 76),
        "ice" => (150, 217, 214),
        "fighting" => (194, 46, 40),
        "poison" => (163, 62, 161),
        "ground" => (226, 191, 101),
        "flying" => (169, 143, 243),
        "psychic" => (249, 85, 135),
        "bug" => (166, 185, 26),
        "rock" => (182, 161, 54),
        "ghost" => (115, 87, 151),
        "dragon" => (111, 53, 252),
        "dark" => (112, 87, 70),
        "steel" => (183, 183, 206),
        "fairy" => (214, 133, 173),
        _ => (120, 128, 144),
    };
    Color32::from_rgb(r, g, b)
}

/// A small round-ended label in a colour: a type, or a word on what something is.
pub fn chip(ui: &mut egui::Ui, words: &str, colour: Color32) {
    // Dark words on a light colour and light on a dark one, by how bright it is to the eye.
    let bright = 0.299 * colour.r() as f32 + 0.587 * colour.g() as f32 + 0.114 * colour.b() as f32;
    let ink = if bright > 150.0 { Color32::from_rgb(24, 26, 34) } else { Color32::WHITE };
    egui::Frame::new().fill(colour).stroke(Stroke::new(0.75, LINE)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::symmetric(7, 2)).show(ui, |ui| {
        ui.label(egui::RichText::new(words).size(10.5).color(ink).strong());
    });
}

pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(15.0).strong());
}

/// Reserve space beside row actions while keeping short labels aligned to the left.
pub fn row_label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, width: f32, height: f32) -> egui::Response {
    ui.allocate_ui_with_layout(egui::vec2(width, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add(egui::Label::new(text).truncate())
    }).inner
}

pub fn nav(ui: &mut egui::Ui, name: &str, on: bool) -> egui::Response {
    let response = ui.scope(|ui| {
        ui.spacing_mut().button_padding.x = 3.0;
        ui.add_sized([ui.available_width(), 34.0], egui::Button::new(egui::RichText::new(name).size(13.0).color(if on { ACCENT } else { FAINT }))
            .fill(GROUND).stroke(Stroke::NONE).corner_radius(CornerRadius::same(5)).truncate())
    }).inner.on_hover_text(name);
    if on {
        ui.painter().line_segment([response.rect.left_bottom() + egui::vec2(12.0, 0.0), response.rect.right_bottom() - egui::vec2(12.0, 0.0)], Stroke::new(2.0, ACCENT));
    }
    if response.has_focus() || (response.hovered() && !on) {
        ui.painter().rect_stroke(response.rect, CornerRadius::same(5), Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
    }
    response
}

pub fn sub_tab(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).size(12.0).color(if on { ACCENT } else { FAINT }))
        .fill(if on { CARD } else { GROUND }).stroke(Stroke::NONE).corner_radius(CornerRadius::same(6)))
}

/// A bar of how far along something is, in a colour, as wide as there is room.
pub fn bar(ui: &mut egui::Ui, share: f32, tall: f32, colour: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), tall), egui::Sense::hover());
    let round = CornerRadius::same((tall / 2.0) as u8);
    ui.painter().rect_filled(rect, round, INSET);
    let share = share.clamp(0.0, 1.0);
    if share > 0.0 {
        let mut done = rect;
        done.set_width((rect.width() * share).max(tall.min(rect.width())));
        ui.painter().rect_filled(done, round, colour);
    }
    response
}
