//! Website capture entry point. Uses the shipping account widgets and palette unchanged.
use super::*;

pub fn frame(ctx: &egui::Context, page: &str, view: &View, icons: &HashMap<u64, egui::TextureHandle>, size: egui::Vec2) -> egui::FullOutput {
    let mut tab = match page {
        "mons" | "mons-shiny" => Tab::Mons,
        "shop" | "shop-active" => Tab::Shop,
        page if page.starts_with("pokedex") => Tab::Dex,
        "social" => Tab::Ranks,
        "privacy" => Tab::Programs,
        page if page.starts_with("updates") => Tab::App,
        _ => Tab::Today,
    };
    let (mut naming, mut keeping, mut lure, mut adding, mut trading) = (None, String::new(), 0, String::new(), None);
    let mut ranking = Some(String::new());
    let mut typed = Typed::default();
    match page {
        "pokedex-caught" => typed.pokedex.filter = pokedex::Filter::Caught,
        "pokedex-seen" => typed.pokedex.filter = pokedex::Filter::Seen,
        "pokedex-unseen" => typed.pokedex.filter = pokedex::Filter::Unseen,
        "pokedex-search" => typed.pokedex.query = "#0004".into(),
        "pokedex-empty" => typed.pokedex.query = "no-such-mon".into(),
        _ => {},
    }
    let mut activity_icons = crate::activity_icons::Icons::offline();
    let mut acts = Vec::new();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        ..Default::default()
    };
    ctx.run_ui(raw, |ui| {
        egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::same(18)).show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            if page == "welcome" {
                sign_in(ui, icons, &mut None, &mut String::new(), &mut acts);
            } else {
                account(ui, view, icons, &mut activity_icons, &mut tab, &mut naming, &mut keeping, &mut lure, &mut adding, &mut trading, &mut ranking, &mut typed, &mut acts);
            }
        });
    })
}
