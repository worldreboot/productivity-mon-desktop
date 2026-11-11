//! The little plate over a mon's head: its name (the one it was given, with its species under it, or its
//! species alone), its level, and how far it is into that level. Drawn here into pixels, once each time what
//! it says changes; the renderer shows it as a picture. Windows' own Segoe UI is the type.

use fontdue::Font;

pub struct Fonts {
    bold: Font,
    plain: Font,
}

/// What a plate says.
#[derive(Clone, PartialEq, Debug)]
pub struct Says {
    pub name: String,
    /// Under the name, smaller: its species, where the name is one it was given.
    pub under: Option<String>,
    pub level: Option<i64>,
    /// How far into its level it is, from none to all. None for a mon that is not the user's.
    pub xp: Option<f32>,
    /// The bar is of a wild mon warming to the user, not of experience: it is drawn in a warmer colour.
    pub warm: bool,
}

/// A picture: RGBA, top row first, colour not multiplied by alpha.
pub struct Picture {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Fonts {
    /// Windows' Segoe UI. None where it cannot be read: the mons then go without plates.
    pub fn load() -> Option<Fonts> {
        #[cfg(target_os="windows")]
        let paths = { let folder=std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(||"C:\\Windows".into())).join("Fonts"); vec![(folder.join("segoeui.ttf"),folder.join("segoeuib.ttf"))] };
        #[cfg(target_os="macos")]
        let paths = vec![
            ("/System/Library/Fonts/Supplemental/Arial.ttf".into(),"/System/Library/Fonts/Supplemental/Arial Bold.ttf".into()),
            ("/System/Library/Fonts/Helvetica.ttc".into(),"/System/Library/Fonts/Helvetica.ttc".into()),
        ];
        let read = |path:std::path::PathBuf| Font::from_bytes(std::fs::read(path).ok()?,fontdue::FontSettings::default()).ok();
        paths.into_iter().find_map(|(plain,bold)|Some(Fonts {plain:read(plain)?,bold:read(bold)?}))
    }
}

impl Fonts {
    /// Whether the type has a letter: a sign it lacks would be drawn as an empty box.
    pub fn has(&self, letter: char) -> bool {
        self.bold.lookup_glyph_index(letter) != 0
    }
}

impl Picture {
    fn new(width: u32, height: u32) -> Picture {
        Picture { pixels: vec![0; (width * height * 4) as usize], width, height }
    }

    /// Lays a colour over a pixel, `cover` of the way (0 to 1) on top of the colour's own alpha.
    fn over(&mut self, x: i32, y: i32, colour: [u8; 4], cover: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || cover <= 0.0 {
            return;
        }
        let at = ((y as u32 * self.width + x as u32) * 4) as usize;
        let top = colour[3] as f32 / 255.0 * cover.min(1.0);
        let under = self.pixels[at + 3] as f32 / 255.0;
        let alpha = top + under * (1.0 - top);
        if alpha <= 0.0 {
            return;
        }
        for c in 0..3 {
            self.pixels[at + c] = ((colour[c] as f32 * top + self.pixels[at + c] as f32 * under * (1.0 - top)) / alpha).round() as u8;
        }
        self.pixels[at + 3] = (alpha * 255.0).round() as u8;
    }

    /// A box with round corners, soft at its edge.
    fn round_box(&mut self, x: f32, y: f32, width: f32, height: f32, radius: f32, colour: [u8; 4]) {
        let radius = radius.min(width / 2.0).min(height / 2.0);
        for py in y.floor() as i32..(y + height).ceil() as i32 {
            for px in x.floor() as i32..(x + width).ceil() as i32 {
                // How far the pixel's middle is inside the box, by its nearest edge or corner.
                let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
                let dx = (x + radius - cx).max(cx - (x + width - radius)).max(0.0);
                let dy = (y + radius - cy).max(cy - (y + height - radius)).max(0.0);
                let inside = radius - (dx * dx + dy * dy).sqrt();
                let edge = (cx - x).min(x + width - cx).min(cy - y).min(y + height - cy);
                self.over(px, py, colour, (inside.min(edge) + 0.5).clamp(0.0, 1.0));
            }
        }
    }

    /// Writes text with its baseline at `y`, starting at `x`.
    fn write(&mut self, font: &Font, text: &str, size: f32, x: f32, y: f32, colour: [u8; 4]) {
        let mut pen = x;
        for letter in text.chars() {
            let (metrics, cover) = font.rasterize(letter, size);
            let (left, top) = (pen.round() as i32 + metrics.xmin, y.round() as i32 - metrics.ymin - metrics.height as i32);
            for row in 0..metrics.height {
                for column in 0..metrics.width {
                    self.over(left + column as i32, top + row as i32, colour, cover[row * metrics.width + column] as f32 / 255.0);
                }
            }
            pen += metrics.advance_width;
        }
    }
}

fn width_of(font: &Font, text: &str, size: f32) -> f32 {
    text.chars().map(|letter| font.metrics(letter, size).advance_width).sum()
}

/// Text cut short with an ellipsis so that it is no wider than `most`.
fn fitted(font: &Font, text: &str, size: f32, most: f32) -> String {
    if width_of(font, text, size) <= most {
        return text.to_string();
    }
    let mut kept: Vec<char> = text.chars().collect();
    while !kept.is_empty() && width_of(font, &kept.iter().chain(&['\u{2026}']).collect::<String>(), size) > most {
        kept.pop();
    }
    kept.into_iter().chain(['\u{2026}']).collect()
}

/// Draws a plate no wider than `most` pixels, everything `scale` times its size at 96 dots an inch.
pub fn draw(fonts: &Fonts, says: &Says, most: u32, scale: f32) -> Picture {
    let s = scale;
    let (pad_x, pad_y, gap) = (9.0 * s, 5.0 * s, 7.0 * s);
    let (name_size, level_size, under_size) = (14.0 * s, 12.0 * s, 10.5 * s);
    let level = says.level.map(|level| format!("Lv {level}"));
    let level_width = level.as_ref().map_or(0.0, |text| gap + width_of(&fonts.plain, text, level_size));
    let name = fitted(&fonts.bold, &says.name, name_size, most as f32 - 2.0 * pad_x - level_width);
    let under = says.under.as_ref().map(|text| fitted(&fonts.plain, text, under_size, most as f32 - 2.0 * pad_x));
    let line = width_of(&fonts.bold, &name, name_size) + level_width;
    let inner = line.max(under.as_ref().map_or(0.0, |text| width_of(&fonts.plain, text, under_size))).max(if says.xp.is_some() { 84.0 * s } else { 0.0 });
    let width = (inner + 2.0 * pad_x).ceil().min(most as f32);
    let (line_height, under_height, bar) = (17.0 * s, 13.0 * s, 4.0 * s);
    let height = (pad_y + line_height + if under.is_some() { under_height } else { 0.0 } + if says.xp.is_some() { bar + 4.0 * s } else { 0.0 } + pad_y).ceil();

    let mut picture = Picture::new(width as u32, height as u32);
    picture.round_box(0.0, 0.0, width, height, 9.0 * s, [18, 18, 22, 178]);
    // The name at the left of its line and the level at the right, both on one baseline.
    let baseline = pad_y + 13.0 * s;
    picture.write(&fonts.bold, &name, name_size, pad_x, baseline, [255, 255, 255, 255]);
    if let Some(level) = &level {
        picture.write(&fonts.plain, level, level_size, width - pad_x - width_of(&fonts.plain, level, level_size), baseline, [198, 204, 216, 255]);
    }
    let mut y = pad_y + line_height;
    if let Some(under) = &under {
        picture.write(&fonts.plain, under, under_size, pad_x, y + 9.0 * s, [168, 174, 186, 255]);
        y += under_height;
    }
    if let Some(xp) = says.xp {
        let (track, top) = (width - 2.0 * pad_x, y + 3.0 * s);
        picture.round_box(pad_x, top, track, bar, bar / 2.0, [255, 255, 255, 52]);
        let filled = track * xp.clamp(0.0, 1.0);
        if filled >= bar {
            picture.round_box(pad_x, top, filled, bar, bar / 2.0, if says.warm { [255, 168, 72, 255] } else { [96, 196, 255, 255] });
        }
    }
    picture
}

/// Several pictures side by side with a gap between, each in the middle of the row's height. The row, and where
/// each picture's middle is across it, in pixels from the row's own middle.
pub fn row(pictures: &[Picture], gap: u32) -> (Picture, Vec<f32>) {
    let width = pictures.iter().map(|p| p.width).sum::<u32>() + gap * pictures.len().saturating_sub(1) as u32;
    let height = pictures.iter().map(|p| p.height).max().unwrap_or(1);
    let mut all = Picture::new(width.max(1), height);
    let (mut x, mut middles) = (0u32, Vec::new());
    for picture in pictures {
        let top = (height - picture.height) / 2;
        for line in 0..picture.height {
            let (from, to) = ((line * picture.width * 4) as usize, (((top + line) * all.width + x) * 4) as usize);
            all.pixels[to..to + (picture.width * 4) as usize].copy_from_slice(&picture.pixels[from..from + (picture.width * 4) as usize]);
        }
        middles.push(x as f32 + picture.width as f32 / 2.0 - width as f32 / 2.0);
        x += picture.width + gap;
    }
    (all, middles)
}

/// Draws a few words to float over a mon as something is earned ("+124 XP"): bold, in `colour`, with a dark edge
/// all round so that they read over any desktop, and nothing behind them.
pub fn floating(fonts: &Fonts, words: &str, colour: [u8; 3], scale: f32) -> Picture {
    let size = 15.0 * scale;
    let edge = (1.5 * scale).ceil();
    let width = (width_of(&fonts.bold, words, size) + 2.0 * edge + 2.0).ceil();
    let height = (size * 1.35 + 2.0 * edge).ceil();
    let mut picture = Picture::new(width as u32, height as u32);
    let baseline = edge + size;
    // The edge: the words in near-black, a little every way.
    for step in 0..12 {
        let turn = step as f32 / 12.0 * std::f32::consts::TAU;
        picture.write(&fonts.bold, words, size, edge + turn.cos() * edge, baseline + turn.sin() * edge, [12, 12, 16, 150]);
    }
    picture.write(&fonts.bold, words, size, edge, baseline, [colour[0], colour[1], colour[2], 255]);
    picture
}

#[cfg(test)]
mod tests {
    use super::*;

    fn says() -> Says {
        Says { name: "Sparky".into(), under: Some("Pikachu".into()), level: Some(12), xp: Some(0.5), warm: false }
    }

    /// The pixels of a plate that are of the bar's bright colour.
    fn bar_pixels(picture: &Picture) -> usize {
        picture.pixels.chunks_exact(4).filter(|p| p[0] < 130 && p[1] > 170 && p[2] > 230 && p[3] > 200).count()
    }

    #[test]
    fn a_plate_has_its_name_in_white_on_a_dark_ground_with_clear_corners() {
        let Some(fonts) = Fonts::load() else { return };
        let picture = draw(&fonts, &says(), 300, 1.0);
        assert_eq!(picture.pixels.len(), (picture.width * picture.height * 4) as usize);
        assert_eq!(picture.pixels[3], 0, "the corner is clear");
        let middle = ((picture.height / 2 * picture.width + 3) * 4) as usize;
        assert!(picture.pixels[middle + 3] > 150 && picture.pixels[middle] < 40, "the ground is dark and nearly solid");
        assert!(picture.pixels.chunks_exact(4).any(|p| p[0] > 240 && p[1] > 240 && p[2] > 240 && p[3] > 240), "there is white type on it");
    }

    #[test]
    fn the_bar_is_as_long_as_the_mon_is_far_into_its_level() {
        let Some(fonts) = Fonts::load() else { return };
        let at = |xp: f32| bar_pixels(&draw(&fonts, &Says { xp: Some(xp), ..says() }, 300, 1.0));
        assert_eq!(at(0.0), 0);
        assert!(at(0.5) > 0 && at(1.0) > at(0.5) * 3 / 2, "{} {}", at(0.5), at(1.0));
        // A mon that is not the user's has no bar, and its plate is shorter for it.
        let wild = draw(&fonts, &Says { xp: None, under: None, ..says() }, 300, 1.0);
        assert!(bar_pixels(&wild) == 0 && wild.height < draw(&fonts, &says(), 300, 1.0).height);
    }

    #[test]
    fn floating_words_are_bright_with_a_dark_edge_and_nothing_behind() {
        let Some(fonts) = Fonts::load() else { return };
        let picture = floating(&fonts, "+124 XP", [255, 214, 92], 1.0);
        assert_eq!(picture.pixels[3], 0, "clear where there are no words");
        assert!(picture.pixels.chunks_exact(4).any(|p| p[0] > 240 && p[1] > 190 && p[2] < 130 && p[3] > 240), "the words, in their colour");
        assert!(picture.pixels.chunks_exact(4).any(|p| p[0] < 40 && p[3] > 100), "their edge");
        assert!(floating(&fonts, "+124 XP", [255, 255, 255], 2.0).width > picture.width * 3 / 2);
    }

    #[test]
    fn plates_in_a_row_keep_their_pixels_and_say_where_each_is() {
        let Some(fonts) = Fonts::load() else { return };
        let (one, other) = (draw(&fonts, &says(), 300, 1.0), draw(&fonts, &Says { name: "Bulbasaur".into(), under: None, ..says() }, 300, 1.0));
        let (widths, heights) = ((one.width, other.width), (one.height, other.height));
        let first = one.pixels.clone();
        let (all, middles) = row(&[one, other], 6);
        assert_eq!((all.width, all.height), (widths.0 + 6 + widths.1, heights.0.max(heights.1)));
        // The first plate's top row is the row's, and the gap between them is clear.
        assert_eq!(all.pixels[..(widths.0 * 4) as usize], first[..(widths.0 * 4) as usize]);
        assert_eq!(all.pixels[((all.height / 2 * all.width + widths.0 + 3) * 4 + 3) as usize], 0);
        assert!(middles[0] < 0.0 && middles[1] > 0.0 && (middles[0] + all.width as f32 / 2.0 - widths.0 as f32 / 2.0).abs() < 0.01);
    }

    #[test]
    fn a_long_name_is_cut_to_fit_and_everything_grows_with_the_screen_s_scale() {
        let Some(fonts) = Fonts::load() else { return };
        let long = draw(&fonts, &Says { name: "Bartholomew the Unreasonably Long-Named".into(), ..says() }, 160, 1.0);
        assert!(long.width <= 160);
        let (small, big) = (draw(&fonts, &says(), 600, 1.0), draw(&fonts, &says(), 600, 2.0));
        assert!(big.width > small.width * 3 / 2 && big.height > small.height * 3 / 2);
    }
}
