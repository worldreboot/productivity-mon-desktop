//! Bundled game item sprites; the custom desk upgrade uses a monitor symbol.
#[derive(Clone, Copy)]
pub enum Item {
    Ball,
    GreatBall,
    UltraBall,
    Lure,
    SuperLure,
    MaxLure,
    Slot,
}

pub fn show(ui: &mut egui::Ui, item: Item) {
    let (name, bytes): (&str, &[u8]) = match item {
        Item::Ball => (
            "poke-ball",
            include_bytes!("../../assets/items/poke-ball.png"),
        ),
        Item::GreatBall => (
            "great-ball",
            include_bytes!("../../assets/items/great-ball.png"),
        ),
        Item::UltraBall => (
            "ultra-ball",
            include_bytes!("../../assets/items/ultra-ball.png"),
        ),
        Item::Lure => ("lure", include_bytes!("../../assets/items/lure.png")),
        Item::SuperLure => (
            "super-lure",
            include_bytes!("../../assets/items/super-lure.png"),
        ),
        Item::MaxLure => (
            "max-lure",
            include_bytes!("../../assets/items/max-lure.png"),
        ),
        Item::Slot => {
            desktop(ui);
            return;
        }
    };
    let id = egui::Id::unique(("shop-item", name));
    let cached = ui
        .ctx()
        .data(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let pixels = image::load_from_memory(bytes)
            .expect("bundled item sprite")
            .into_rgba8();
        let texture = ui.ctx().load_texture(
            name,
            egui::ColorImage::from_rgba_unmultiplied(
                [pixels.width() as usize, pixels.height() as usize],
                pixels.as_raw(),
            ),
            egui::TextureOptions::NEAREST,
        );
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(40.0, 40.0)));
}

fn desktop(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
    let p = ui.painter();
    let screen = egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 7.0), egui::vec2(28.0, 20.0));
    let stroke = egui::Stroke::new(1.5, crate::look::FAINT);
    p.rect_filled(screen, 3.0, crate::look::INSET);
    p.rect_stroke(screen, 3.0, stroke, egui::StrokeKind::Inside);
    p.line_segment(
        [
            rect.min + egui::vec2(18.0, 27.0),
            rect.min + egui::vec2(18.0, 32.0),
        ],
        stroke,
    );
    p.line_segment(
        [
            rect.min + egui::vec2(11.0, 33.0),
            rect.min + egui::vec2(25.0, 33.0),
        ],
        stroke,
    );
    let plus = rect.min + egui::vec2(31.0, 28.0);
    p.circle_filled(plus, 7.0, crate::look::CARD);
    let green = egui::Stroke::new(1.7, crate::look::GOOD);
    p.line_segment(
        [plus - egui::vec2(4.0, 0.0), plus + egui::vec2(4.0, 0.0)],
        green,
    );
    p.line_segment(
        [plus - egui::vec2(0.0, 4.0), plus + egui::vec2(0.0, 4.0)],
        green,
    );
}
