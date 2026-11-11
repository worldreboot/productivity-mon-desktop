//! Small public profile pictures from the configured game server. Network and decoding stay off the UI thread.
use std::{collections::HashMap, io::{Cursor, Read}, sync::mpsc, time::{Duration, Instant}};

struct Entry {
    texture: Option<egui::TextureHandle>,
    pending: bool,
    tried: Instant,
}

pub(super) struct Avatars {
    entries: HashMap<String, Entry>,
    sender: Option<mpsc::SyncSender<String>>,
    results: mpsc::Receiver<(String, Option<image::RgbaImage>)>,
}

impl Avatars {
    pub(super) fn new(server: &str) -> Self {
        let (sender, requests) = mpsc::sync_channel::<String>(32);
        let (completed, results) = mpsc::channel();
        let server = server.trim_end_matches('/').to_string();
        std::thread::Builder::new().name("friend-avatars".into()).spawn(move || {
            let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(4)).redirects(0).build();
            while let Ok(path) = requests.recv() {
                let pixels = load(&agent, &format!("{server}{path}"));
                if completed.send((path, pixels)).is_err() { break; }
            }
        }).expect("profile picture worker");
        Self { entries: HashMap::new(), sender: Some(sender), results }
    }

    #[cfg(any(test, feature = "website-capture"))]
    pub(super) fn offline() -> Self {
        let (_, results) = mpsc::channel();
        Self { entries: HashMap::new(), sender: None, results }
    }

    pub(super) fn update(&mut self, ctx: &egui::Context, social: &serde_json::Value) {
        // Drop old versions and removed friends rather than accumulating textures for the life of the widget.
        self.entries.retain(|path,_| social["board"].as_array().into_iter().flatten().any(|row|row["avatar"].as_str()==Some(path.as_str())));
        while let Ok((path, pixels)) = self.results.try_recv() {
            if let Some(entry) = self.entries.get_mut(&path) {
                entry.pending = false;
                entry.texture = pixels.map(|pixels| ctx.load_texture(
                    format!("friend-{path}"),
                    egui::ColorImage::from_rgba_unmultiplied([pixels.width() as usize, pixels.height() as usize], pixels.as_raw()),
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        if self.entries.values().any(|entry| entry.pending) { ctx.request_repaint_after(Duration::from_millis(100)); }
    }

    pub(super) fn show(&mut self, ui: &egui::Ui, path: &str, rect: egui::Rect) {
        if custom_path(path) {
            if let Some(texture) = self.entries.get(path).and_then(|entry| entry.texture.as_ref()) {
                egui::Image::new(texture).corner_radius(12).paint_at(ui, rect);
                return;
            }
            let retry = self.entries.get(path).is_none_or(|entry| !entry.pending && entry.tried.elapsed() > Duration::from_secs(300));
            if retry && self.entries.len() < 128 && self.sender.as_ref().is_some_and(|sender| sender.try_send(path.to_string()).is_ok()) {
                self.entries.insert(path.to_string(), Entry { texture: None, pending: true, tried: Instant::now() });
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }
        default_avatar(ui.painter(), rect);
    }
}

// Accept only the server's versioned avatar shape, never arbitrary third-party URLs or paths.
fn custom_path(path: &str) -> bool {
    let Some((key, version)) = path.strip_prefix("/avatars/").and_then(|path| path.split_once("?v=")) else { return false; };
    uuid::Uuid::parse_str(key).is_ok() && uuid::Uuid::parse_str(version).is_ok()
}

fn load(agent: &ureq::Agent, url: &str) -> Option<image::RgbaImage> {
    let mut bytes = Vec::new();
    agent.get(url).call().ok()?.into_reader().take(1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 1024 * 1024 { return None; }
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> Option<image::RgbaImage> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(256);
    limits.max_image_height = Some(256);
    limits.max_alloc = Some(4 * 1024 * 1024);
    reader.limits(limits);
    let pixels = reader.decode().ok()?.resize_to_fill(48, 48, image::imageops::FilterType::Lanczos3).into_rgba8();
    Some(pixels)
}

/// The same cream/red trainer motif as the account's default SVG, drawn as vectors at widget size.
fn default_avatar(p: &egui::Painter, rect: egui::Rect) {
    use crate::look;
    let at = |x: f32, y: f32| rect.min + egui::vec2(x, y) * (rect.width() / 128.0);
    let scale = rect.width() / 128.0;
    p.circle_filled(rect.center(), rect.width() / 2.0, look::INSET);
    let body = [(23.,113.),(31.,96.),(48.,84.),(80.,84.),(98.,99.),(105.,113.),(101.,117.),(88.,124.),(64.,128.),(40.,124.),(27.,117.)];
    p.add(egui::Shape::convex_polygon(body.into_iter().map(|(x,y)|at(x,y)).collect(),look::ACCENT,egui::Stroke::NONE));
    p.circle_filled(at(64.,55.),26.0*scale,look::CARD);
    let cap: Vec<_> = (0..=12).map(|step| { let a=std::f32::consts::PI*(step as f32/12.0); at(64.0+28.0*a.cos(),49.0-28.0*a.sin()) }).collect();
    p.add(egui::Shape::convex_polygon(cap,look::ACCENT,egui::Stroke::NONE));
    p.line_segment([at(35.,49.),at(97.,49.)],egui::Stroke::new(5.0*scale,look::INK));
    p.line_segment([at(54.,37.),at(74.,37.)],egui::Stroke::new(6.0*scale,look::CARD));
    for x in [56.,72.] {p.circle_filled(at(x,60.),2.5*scale,look::INK);}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_versioned_profile_paths_and_bounded_pngs() {
        let key=uuid::Uuid::new_v4();let version=uuid::Uuid::new_v4();
        assert!(custom_path(&format!("/avatars/{key}?v={version}")));
        for path in ["https://example.com/a.png","//example.com/a.png","/avatars/../../secret?v=1","/avatars/default.svg","/avatars/a?v=b"] {assert!(!custom_path(path));}
        let mut png=Cursor::new(Vec::new());
        image::RgbaImage::new(256,256).write_to(&mut png,image::ImageFormat::Png).unwrap();
        assert_eq!(decode(png.get_ref()).unwrap().dimensions(),(48,48));
        let mut oversized=Cursor::new(Vec::new());
        image::RgbaImage::new(257,256).write_to(&mut oversized,image::ImageFormat::Png).unwrap();
        assert!(decode(oversized.get_ref()).is_none());
        assert!(decode(b"<svg/>").is_none());
    }
}
