//! The app's own window: the collection and who is on the desk, the lists of programs, the shop, and getting an
//! account. It floats over the desktop like the mons do, is drawn with egui on the app's own wgpu device, and
//! costs nothing while it is shut: the window and everything egui keeps are made when it is opened and dropped
//! when it is closed. It only shows what the server last said and says what was pressed; `main` does the rest.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

use crate::look;
use crate::render::Gpu;

#[path = "panel/pokedex.rs"]
mod pokedex;
#[path = "panel/items.rs"]
mod items;
const DEX_VISIBLE: &str = "pokedex-visible-icons";

// Offline screenshots of these same widgets, with example data and no tracker or network.
#[cfg(feature = "website-capture")]
#[path = "panel/capture.rs"]
pub mod capture;

/// Something pressed in the panel, for `main` to do.
pub enum Act {
    /// A change to ask the server for: the path, and the body.
    Post(&'static str, Value),
    /// A ball at the wild mon: `ball`, `great` or `ultra`.
    Throw(&'static str),
    /// An account chosen in the browser.
    Link(Option<String>, String),
    EndSession(bool),
    /// The server's page, in the browser.
    Page,
    /// macOS: ask the user for browser-window Accessibility access.
    EnableWebsiteTracking,
    /// A program (its file name) or a site (`site:<name>`) to be never reported from now on, or reported again.
    Private(String, bool),
    /// The friends, their day and the trades, asked for anew.
    Friends,
    /// A change among friends or trades to ask the server for: the path, and the body.
    Social(&'static str, Value),
    /// A friend's collection, by their account id, to pick from.
    FriendMons(String),
    /// A table of the hiscores to ask for, by its path.
    Ranks(String),
    /// One of the app's own switches (see `View::switches`), turned on or off.
    Switch(&'static str, bool),
    /// What there was to say has been said (as a toast): it need not be kept.
    Noted,
}

/// Where a table of the hiscores is asked for: where your own mons stand (none), everybody overall (an empty
/// name), or one species' table.
fn ranks_path(of: &Option<String>) -> String {
    match of.as_deref() {
        None => "/hiscores/mine".to_string(),
        Some("") => "/hiscores".to_string(),
        Some(species) => format!("/hiscores?species={species}"),
    }
}

/// A trade being put together: who with (their account id and name), and the mon picked on each side so far.
pub struct Trading {
    with: String,
    name: String,
    give: Option<String>,
    want: Option<String>,
}

/// What is being typed here and there in the panel and has not been sent yet.
#[derive(Default)]
pub struct Typed {
    /// What a focus session is to be on.
    label: String,
    /// Whether the Today page shows the week and the days before as well as the day.
    more: bool,
    focus_duration: usize,
    account_action: Option<bool>,
    last_classification: Option<(String,String,String)>,
    pokedex: pokedex::Browser,
}

/// A mon just caught, being welcomed: what the server said of it, and the name typed for it so far.
pub struct Welcome {
    mon: Value,
    typed: String,
}

/// How often the friends' day is asked for again while it is being looked at.
const FRIENDS_EVERY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Hash)]
enum Tab {
    Today,
    Mons,
    Dex,
    Programs,
    Sites,
    Shop,
    Friends,
    Ranks,
    /// The app's own switches.
    App,
}

/// The four things the panel is about: each tab is a page of one of them.
#[derive(Clone, Copy, PartialEq)]
enum Group {
    Today,
    Mons,
    Social,
    Settings,
}

impl Tab {
    fn group(self) -> Group {
        match self {
            Tab::Today => Group::Today,
            Tab::Mons | Tab::Dex | Tab::Shop => Group::Mons,
            Tab::Friends | Tab::Ranks => Group::Social,
            Tab::Programs | Tab::Sites | Tab::App => Group::Settings,
        }
    }
}

/// What the panel shows: the answer of `GET /state` (null before there is one), whether there is an account at
/// all, and the last thing there was to say.
pub struct View<'a> {
    pub state: &'a Value,
    pub status: &'a Value,
    pub account: bool,
    pub note: &'a str,
    /// The sites met and those the user has a word on: the name, what it counts as, and whether that is their word.
    pub sites: &'a [(String, Option<&'static str>, bool)],
    /// The programs and sites that are never reported.
    pub private: &'a [String],
    /// The answer of `GET /friends` (null before there is one).
    pub social: &'a Value,
    /// The collections of the friends asked about, by their account ids.
    pub theirs: &'a HashMap<String, Value>,
    /// The tables of the hiscores asked for so far, by their paths.
    pub ranks: &'a HashMap<String, Value>,
    /// Why the last report that earned anything earned what it did (null before there is one).
    pub earning: &'a Value,
    /// The app's own switches, which the icon's menu has too: what each is called by the app, what it says,
    /// and whether it is on.
    pub switches: &'a [(&'static str, &'static str, bool)],
}

pub struct Panel {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    ctx: egui::Context,
    input: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    tab: Tab,
    /// The mon being given a name, by its id, and what has been typed for it so far.
    naming: Option<(String, String)>,
    /// Reserved shop selection state.
    lure: usize,
    /// What has been typed of a program to keep from being reported.
    keeping: String,
    /// What has been typed of a friend's code.
    adding: String,
    /// The trade being put together, if one is.
    trading: Option<Trading>,
    /// When the friends were last asked for.
    asked: Option<Instant>,
    /// The mon just caught, until it has been named or waved through.
    welcome: Option<Welcome>,
    typed: Typed,
    /// What is being said over the foot of the panel for a few seconds, and since when; and the last thing
    /// there was to say, so that it is said once.
    toast: Option<(String, Instant)>,
    said: String,
    /// Which table of the hiscores is looked at (see `ranks_path`), and which was last asked for and when.
    ranking: Option<String>,
    ranked: Option<(String, Instant)>,
    /// The starter picked for a new account: none is the server's choice.
    starter: Option<&'static str>,
    /// Each species' picture by its Pokédex number, once looked for: none where there is no picture.
    icons: HashMap<u64, Option<egui::TextureHandle>>,
    icon_folder: PathBuf,
    activity_icons: crate::activity_icons::Icons,
    /// When egui wants to be drawn again without anything having happened (an animation, a tooltip's delay).
    pub again: Option<Instant>,
}

/// The mons a new account can start with: each region's three, in the order of the regions, and then the two
/// that have been given in their stead. Its id, its name and its Pok\u{e9}dex number (for its picture).
const STARTERS: [(&str, &str, u64); 29] = [
    ("bulbasaur", "Bulbasaur", 1), ("charmander", "Charmander", 4), ("squirtle", "Squirtle", 7),
    ("chikorita", "Chikorita", 152), ("cyndaquil", "Cyndaquil", 155), ("totodile", "Totodile", 158),
    ("treecko", "Treecko", 252), ("torchic", "Torchic", 255), ("mudkip", "Mudkip", 258),
    ("turtwig", "Turtwig", 387), ("chimchar", "Chimchar", 390), ("piplup", "Piplup", 393),
    ("snivy", "Snivy", 495), ("tepig", "Tepig", 498), ("oshawott", "Oshawott", 501),
    ("chespin", "Chespin", 650), ("fennekin", "Fennekin", 653), ("froakie", "Froakie", 656),
    ("rowlet", "Rowlet", 722), ("litten", "Litten", 725), ("popplio", "Popplio", 728),
    ("grookey", "Grookey", 810), ("scorbunny", "Scorbunny", 813), ("sobble", "Sobble", 816),
    ("sprigatito", "Sprigatito", 906), ("fuecoco", "Fuecoco", 909), ("quaxly", "Quaxly", 912),
    ("pikachu", "Pikachu", 25), ("eevee", "Eevee", 133),
];
const CLASSES: [(&str, &str); 3] = [("productive", "Productive"), ("neutral", "Neutral"), ("distracting", "Distracting")];

impl Panel {
    pub fn reset_account(&mut self) {
        self.tab=Tab::Today;self.naming=None;self.keeping.clear();self.adding.clear();self.trading=None;self.welcome=None;
        self.asked=None;self.ranked=None;self.ranking=None;self.typed=Typed::default();self.toast=None;self.said.clear();
    }
    /// Opens the panel in the corner by the clock.
    pub fn open(event_loop: &ActiveEventLoop, gpu: &Gpu, icon_folder: PathBuf, activity_icon_folder: PathBuf) -> Panel {
        let attributes = Window::default_attributes()
            .with_title("ProductivityMon")
            .with_window_icon(Some(crate::app_icon::window()))
            .with_inner_size(LogicalSize::new(440.0, 720.0))
            .with_min_inner_size(LogicalSize::new(320.0, 420.0))
            .with_window_level(WindowLevel::AlwaysOnTop);
        let window = Arc::new(event_loop.create_window(attributes).expect("a window"));
        #[cfg(target_os="windows")]
        if let Some(monitor) = window.current_monitor().or(event_loop.primary_monitor()) {
            let (screen, at, size) = (monitor.size(), monitor.position(), window.outer_size());
            let gap = (64.0 * monitor.scale_factor()) as i32;
            crate::desk::position(&window,at.x + screen.width as i32 - size.width as i32 - gap / 4,at.y + screen.height as i32 - size.height as i32 - gap);
        }
        #[cfg(target_os="macos")]
        if let (Some(screen),Some(bounds))=(crate::desk::screens().first(),crate::desk::window_bounds(&window)) {
            let gap=(32.0*window.scale_factor()) as i32;
            crate::desk::position(&window,screen.right-(bounds.right-bounds.left)-gap,screen.bottom-(bounds.bottom-bounds.top)-gap);
        }
        let (surface, config) = gpu.plain(window.clone());
        let ctx = egui::Context::default();
        look::dress(&ctx);
        let input = egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, &*window, Some(window.scale_factor() as f32), None, None);
        let renderer = egui_wgpu::Renderer::new(&gpu.device, config.format, egui_wgpu::RendererOptions::default());
        Panel { window, surface, config, ctx, input, renderer, tab: Tab::Today, lure: 0, naming: None, keeping: String::new(), adding: String::new(), trading: None, asked: None, welcome: None, typed: Typed::default(), toast: None, said: String::new(), ranking: None, ranked: None, starter: None, icons: HashMap::new(), icon_folder, activity_icons: crate::activity_icons::Icons::new(activity_icon_folder), again: None }
    }

    /// A mon has just been caught: it is shown at the top, to be named and given a place, until that is done.
    pub fn welcome(&mut self, mon: Value) {
        self.welcome = Some(Welcome { mon, typed: String::new() });
        self.tab = Tab::Mons;
        self.ctx.request_repaint();
    }

    /// Turns to a tab by name: for tests, which cannot click.
    pub fn show(&mut self, tab: &str) {
        self.tab = match tab {
            "mons" => Tab::Mons,
            "dex" => Tab::Dex,
            "programs" => Tab::Programs,
            "sites" => Tab::Sites,
            "shop" => Tab::Shop,
            "friends" => Tab::Friends,
            "app" | "settings" => Tab::App,
            "ranks" => {
                // Which table, for tests: `DESK_RANKS` empty is overall, else a species.
                self.ranking = std::env::var("DESK_RANKS").ok();
                Tab::Ranks
            }
            _ => Tab::Today,
        };
    }

    /// Hears one of its window's events. Whether it should be drawn again for it.
    pub fn event(&mut self, gpu: &Gpu, event: &WindowEvent) -> bool {
        if let WindowEvent::Resized(size) = event {
            if size.width > 0 && size.height > 0 {
                (self.config.width, self.config.height) = (size.width, size.height);
                self.surface.configure(&gpu.device, &self.config);
            }
            return true;
        }
        self.input.on_window_event(&self.window, event).repaint
    }

    /// A species' picture, small, if the folder of them is here and has it.
    fn icon(&mut self, dex: u64) -> Option<egui::TextureHandle> {
        if let Some(found) = self.icons.get(&dex) {
            return found.clone();
        }
        const SIDE: usize = 48;
        let made = crate::render::picture(&crate::variants::picture(&self.icon_folder, dex)).ok().and_then(|(pixels, width, height)| {
            let (width, height) = (width as usize, height as usize);
            if width < SIDE || height < SIDE {
                return None;
            }
            // Down to a small square, each pixel the mean of the block over it, weighted by how solid each is.
            let mut small = vec![0u8; SIDE * SIDE * 4];
            for y in 0..SIDE {
                for x in 0..SIDE {
                    let (mut sum, mut count) = ([0u32; 4], 0u32);
                    for sy in y * height / SIDE..((y + 1) * height / SIDE).max(y * height / SIDE + 1) {
                        for sx in x * width / SIDE..((x + 1) * width / SIDE).max(x * width / SIDE + 1) {
                            let p = &pixels[(sy * width + sx) * 4..(sy * width + sx) * 4 + 4];
                            let alpha = p[3] as u32;
                            sum[0] += p[0] as u32 * alpha;
                            sum[1] += p[1] as u32 * alpha;
                            sum[2] += p[2] as u32 * alpha;
                            sum[3] += alpha;
                            count += 1;
                        }
                    }
                    let at = (y * SIDE + x) * 4;
                    if sum[3] > 0 {
                        small[at] = (sum[0] / sum[3]) as u8;
                        small[at + 1] = (sum[1] / sum[3]) as u8;
                        small[at + 2] = (sum[2] / sum[3]) as u8;
                        small[at + 3] = (sum[3] / count) as u8;
                    }
                }
            }
            Some(self.ctx.load_texture(format!("icon-{dex}"), egui::ColorImage::from_rgba_unmultiplied([SIDE, SIDE], &small), egui::TextureOptions::LINEAR))
        });
        self.icons.insert(dex, made.clone());
        made
    }

    /// Draws the panel and says what was pressed in it.
    pub fn draw(&mut self, gpu: &Gpu, view: &View) -> Vec<Act> {
        let mut acts = Vec::new();
        // The pictures the mons on show need, found before egui has the panel borrowed.
        let mut dexes: Vec<u64> = view.state["mons"].as_array().into_iter().flatten().filter_map(crate::variants::of).collect();
        dexes.extend(crate::variants::of(&view.state["encounter"]));
        dexes.extend(self.welcome.as_ref().and_then(|welcome| crate::variants::of(&welcome.mon)));
        if !view.account {
            dexes.extend(STARTERS.iter().map(|(_, _, dex)| *dex));
        }
        if self.tab == Tab::Dex {
            // The list requests only its visible rows, rather than decoding 1,025 PNGs on opening.
            dexes.extend(self.ctx.data(|data| data.get_temp::<Vec<u64>>(egui::Id::unique(DEX_VISIBLE)))
                .unwrap_or_else(|| pokedex::entries(view.state).iter().take(24).map(|entry| entry.number).collect()));
        }
        if self.tab == Tab::Friends {
            let social = view.social;
            dexes.extend(social["board"].as_array().into_iter().flatten().filter_map(|row| crate::variants::of(&row["mon"])));
            dexes.extend(social["trades"].as_array().into_iter().flatten().flat_map(|trade| [crate::variants::of(&trade["give"]), crate::variants::of(&trade["want"])]).flatten());
            if let Some(theirs) = self.trading.as_ref().and_then(|trading| view.theirs.get(&trading.with)) {
                dexes.extend(theirs["mons"].as_array().into_iter().flatten().filter_map(crate::variants::of));
            }
            // Looked at, the friends' day is kept fresh.
            if view.account && self.asked.is_none_or(|at| at.elapsed() >= FRIENDS_EVERY) {
                self.asked = Some(Instant::now());
                acts.push(Act::Friends);
            }
        }
        if self.tab == Tab::Ranks {
            let path = ranks_path(&self.ranking);
            if let Some(table) = view.ranks.get(&path) {
                dexes.extend(table["dex"].as_u64());
                dexes.extend(table["mons"].as_array().into_iter().flatten().filter_map(crate::variants::of));
            }
            // Another table, or the same one a minute on: asked for.
            if view.account && self.ranked.as_ref().is_none_or(|(of, at)| *of != path || at.elapsed() >= FRIENDS_EVERY) {
                self.ranked = Some((path.clone(), Instant::now()));
                acts.push(Act::Ranks(path));
            }
        }
        let icons: HashMap<u64, egui::TextureHandle> = dexes.into_iter().filter_map(|dex| Some((dex, self.icon(dex)?))).collect();

        // Something new to say is said over the foot of the panel for a few seconds, longer the more of it
        // there is, and then need not be kept. (With no account yet it stays where it is: it may be how to get one.)
        if view.note != self.said {
            self.said = view.note.to_string();
            if !view.note.is_empty() && view.account {
                self.toast = Some((view.note.to_string(), Instant::now()));
                acts.push(Act::Noted);
            }
        }
        let lasts = |words: &str| 3.5 + words.len() as f32 / 28.0;
        if self.toast.as_ref().is_some_and(|(words, since)| since.elapsed().as_secs_f32() >= lasts(words)) {
            self.toast = None;
        }
        let toast = self.toast.as_ref().map(|(words, since)| (words.clone(), (lasts(words) - since.elapsed().as_secs_f32()).clamp(0.0, 0.5) * 2.0));

        let raw = self.input.take_egui_input(&self.window);
        let (mut tab, mut starter) = (self.tab, self.starter);
        let mut naming = self.naming.take();
        let mut keeping = std::mem::take(&mut self.keeping);
        let mut lure = self.lure;
        let mut adding = std::mem::take(&mut self.adding);
        let mut trading = self.trading.take();
        let mut ranking = self.ranking.take();
        let mut welcome = self.welcome.take();
        let mut typed = std::mem::take(&mut self.typed);
        let ranking_was = ranking.clone();
        self.activity_icons.update(&self.ctx, view.account && view.status["paused"] != true);
        let activity_icons = &mut self.activity_icons;
        let output = self.ctx.run_ui(raw, |ui| {
            egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::same(18)).show(ui, |ui| {
                if !view.account {
                    if view.status["accountChanging"]==true {
                        ui.heading("Signing out of this desktop…");ui.spinner();ui.label("Saving pending activity for your current account.");
                    } else {sign_in(ui, &icons, &mut starter, &mut typed.label, &mut acts);}
                } else if view.state.is_null() {
                    ui.label("Asking the server who you are...");
                } else {
                    if let Some(now) = welcome.take() {
                        welcome = greet(ui, view.state, &icons, now, &mut acts);
                    }
                    account(ui, view, &icons, activity_icons, &mut tab, &mut naming, &mut keeping, &mut lure, &mut adding, &mut trading, &mut ranking, &mut typed, &mut acts);
                }
                if !view.note.is_empty() && !view.account {
                    ui.separator();
                    ui.label(egui::RichText::new(view.note).color(look::FAINT).small());
                }
            });
            if let Some((words, there)) = &toast {
                let good = ["kept", "Kept", "back from", "Bought", "break well", "new account", "Saved", "Named"].iter().any(|glad| words.contains(glad));
                let bad = ["would not", "cannot", "not kept", "not enough", "fell through", "nobody"].iter().any(|sorry| words.contains(sorry));
                egui::Area::new(egui::Id::unique("toast")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -14.0)).interactable(false).show(ui.ctx(), |ui| {
                    ui.set_opacity(*there);
                    ui.set_max_width(330.0);
                    egui::Frame::new()
                        .fill(look::CARD)
                        .stroke(egui::Stroke::new(1.0, if good { look::GOOD } else if bad { look::BAD } else { look::ACCENT }))
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            // Said as a sentence: it began in the app's log, where it need not be.
                            let mut said = words.clone();
                            if let Some(first) = said.get(..1).map(str::to_uppercase) {
                                said.replace_range(..1, &first);
                            }
                            ui.label(egui::RichText::new(said).color(look::INK));
                        });
                });
            }
        });
        if (tab == Tab::Ranks && self.tab != Tab::Ranks) || ranking != ranking_was {
            // Turned to, or another table picked: asked for at the next drawing, which is asked for now.
            self.ranked = None;
            self.ctx.request_repaint();
        }
        self.ranking = ranking;
        self.welcome = welcome;
        self.typed = typed;
        if tab == Tab::Friends && self.tab != Tab::Friends {
            // Turned to: asked for at the next drawing, which is asked for now.
            self.asked = None;
            self.ctx.request_repaint();
        }
        (self.tab, self.starter, self.naming, self.keeping, self.lure, self.adding, self.trading) = (tab, starter, naming, keeping, lure, adding, trading);
        self.input.handle_platform_output(&self.window, output.platform_output);
        let delay = output.viewport_output.get(&egui::ViewportId::ROOT).map_or(Duration::MAX, |viewport| viewport.repaint_delay);
        let delay = if self.toast.is_some() { delay.min(Duration::from_millis(100)) } else { delay };
        self.again = Instant::now().checked_add(if view.state["session"].is_object() { delay.min(Duration::from_secs(20)) } else if matches!(self.tab, Tab::Friends | Tab::Ranks) { delay.min(FRIENDS_EVERY) } else { delay });

        let jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        // The textures egui wants made, changed and let go: taken out of its answer, done, and then emptied, which
        // is how egui is told they were not forgotten.
        let mut textures = output.textures_delta;
        for (id, deltas) in &textures.set {
            for delta in deltas {
                self.renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.config.width, self.config.height], pixels_per_point: output.pixels_per_point };
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => {
                for id in &textures.free {
                    self.renderer.free_texture(id);
                }
                textures.clear();
                return acts;
            }
        };
        let target = texture.texture.create_view(&Default::default());
        // Under whatever the panel's contents do not reach, the same colour as behind them.
        let fill = self.ctx.global_style().visuals.panel_fill;
        let fill = wgpu::Color { r: fill.r() as f64 / 255.0, g: fill.g() as f64 / 255.0, b: fill.b() as f64 / 255.0, a: 1.0 };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        self.renderer.update_buffers(&gpu.device, &gpu.queue, &mut encoder, &jobs, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("panel"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(fill), store: wgpu::StoreOp::Store },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &jobs, &screen);
        }
        gpu.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        gpu.queue.present(texture);
        for id in &textures.free {
            self.renderer.free_texture(id);
        }
        textures.clear();
        acts
    }
}

/// No account yet: make one here, with a starter picked from every region's and a name for it, or bring one
/// from the browser.
fn sign_in(ui: &mut egui::Ui, _icons: &HashMap<u64, egui::TextureHandle>, _starter: &mut Option<&'static str>, _name: &mut String, acts: &mut Vec<Act>) {
    ui.add_space(22.0);
    ui.heading("Welcome to ProductivityMon");
    ui.add_space(8.0);
    ui.label("A productivity companion that grows as you work.");
    ui.add_space(24.0);
    let button = egui::Button::new(egui::RichText::new("Sign in").color(egui::Color32::WHITE)).fill(look::ACCENT);
    if ui.add_sized([ui.available_width(), 40.0], button).clicked() { acts.push(Act::Link(None, String::new())); }
    ui.label(egui::RichText::new("Google, Discord, or email").small().color(look::FAINT));
    ui.add_space(20.0);
    ui.label(egui::RichText::new("Tracking starts after sign-in. Pause it anytime.").small().color(look::FAINT));
    ui.collapsing("How it works", |ui| {
        ui.label("Choose a companion, then work in your usual apps. Productive activity earns XP. Returning accounts keep their team.");
        ui.label("Your first activity sync usually arrives within a minute.");
    });
    ui.collapsing("What tracking reads", |ui| {
        ui.label("App names, activity, and supported browser tab titles. Never what you type. Your never-report list stays on this PC.");
    });
}

#[allow(clippy::too_many_arguments)]
fn account(ui: &mut egui::Ui, view: &View, icons: &HashMap<u64, egui::TextureHandle>, activity_icons: &mut crate::activity_icons::Icons, tab: &mut Tab, naming: &mut Option<(String, String)>, keeping: &mut String, lure: &mut usize, adding: &mut String, trading: &mut Option<Trading>, ranking: &mut Option<String>, typed: &mut Typed, acts: &mut Vec<Act>) {
    let state = view.state;
    // Account actions stay close without taking a permanent row of controls.
    ui.horizontal_wrapped(|ui| {
        let name = state["account"]["name"].as_str().unwrap_or("Trainer");
        ui.menu_button(egui::RichText::new(format!("{name}  ···")).strong(), |ui| {
            ui.set_max_width(260.0);
            if state["account"]["anonymous"] == true {
                ui.label("Local guest account");
                ui.label(egui::RichText::new("Link this account before signing out to keep its progress.").small().color(look::FAINT));
                if ui.button("Link account").clicked() { acts.push(Act::Link(None, String::new())); ui.close(); }
            } else {
                if ui.button("Switch account").clicked() { typed.account_action = Some(true); ui.close(); }
                if ui.button("Sign out").clicked() { typed.account_action = Some(false); ui.close(); }
            }
            ui.separator();
            if ui.button("Open web account").clicked() { acts.push(Act::Page); ui.close(); }
        }).response.on_hover_text("Account options");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let balls = state["balls"].as_i64().unwrap_or(0) + state["great"].as_i64().unwrap_or(0) + state["ultra"].as_i64().unwrap_or(0);
            ui.label(egui::RichText::new(format!("{} pts · {balls} balls", thousands(state["points"].as_f64().unwrap_or(0.0)))).small().color(look::FAINT));
        });
    });
    ui.add_space(6.0);
    if let Some(switch)=typed.account_action {
        look::card(ui,|ui| {
            ui.strong(if switch {"Switch to another account?"} else {"Sign out of this desktop?"});
            ui.label("Tracking stops. Your team stays saved, and pending activity syncs when you return to this account.");
            ui.horizontal_wrapped(|ui| {
                if ui.button(if switch {"Continue to sign-in"} else {"Sign out now"}).clicked() {typed.account_action=None;acts.push(Act::EndSession(switch));}
                if ui.button("Cancel").clicked() {typed.account_action=None;}
            });
        });
        return;
    }
    tracking_status(ui, view, activity_icons, typed, acts);
    wild(ui, state, icons, acts);
    ui.add_space(8.0);
    // Four tabs, each as wide as the others; somebody waiting on you is said on Social itself.
    let waiting = view.social["incoming"].as_array().map_or(0, Vec::len) + view.social["trades"].as_array().into_iter().flatten().filter(|trade| !trade["mine"].as_bool().unwrap_or(true)).count();
    let group = tab.group();
    ui.columns(4, |columns| {
        for (column, (of, name, first)) in columns.iter_mut().zip([(Group::Today, "Today".to_string(), Tab::Today), (Group::Mons, "Mons".to_string(), Tab::Mons), (Group::Social, if waiting > 0 { format!("Social ({waiting})") } else { "Social".to_string() }, Tab::Friends), (Group::Settings, "Settings".to_string(), Tab::Programs)]) {
            let on = group == of;
            if look::nav(column, &name, on).clicked() && !on {
                *tab = first;
            }
        }
    });
    let (rule, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().line_segment([rule.left_center(), rule.right_center()], egui::Stroke::new(1.0, look::LINE));
    // The pages of the tab that is open, where it has more than one.
    let pages: &[(Tab, &str)] = match tab.group() {
        Group::Today => &[],
        Group::Mons => &[(Tab::Mons, "Yours"), (Tab::Dex, "Pok\u{e9}dex"), (Tab::Shop, "Shop")],
        Group::Social => &[(Tab::Friends, "Friends"), (Tab::Ranks, "Ranks")],
        Group::Settings => &[(Tab::Programs, "Activity"), (Tab::Sites, "Websites"), (Tab::App, "Preferences")],
    };
    if !pages.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for (page, name) in pages {
                if *page == Tab::Sites && !state["sorter"].as_bool().unwrap_or(false) {
                    continue;
                }
                if look::sub_tab(ui, name, *tab == *page).clicked() { *tab = *page; }
            }
        });
    }
    ui.add_space(10.0);
    if *tab == Tab::Dex {
        look::card(ui, |ui| dex(ui, state, icons, &mut typed.pokedex));
        return;
    }
    egui::ScrollArea::vertical().id_salt(("account-page",*tab)).auto_shrink([false, false]).show(ui, |ui| match tab {
        Tab::Today => today(ui, state, view.earning, icons, typed, acts),
        Tab::Mons => mons(ui, state, icons, naming, acts),
        Tab::Dex => {},
        Tab::Programs => look::card(ui, |ui| programs(ui, state, view.private, activity_icons, keeping, acts)),
        Tab::Sites => look::card(ui, |ui| sites(ui, view.sites, view.private, activity_icons, acts)),
        Tab::Shop => look::card(ui, |ui| shop(ui, state, lure, acts)),
        Tab::Ranks => look::card(ui, |ui| ranks(ui, state, view.ranks, icons, ranking)),
        Tab::Friends => look::card(ui, |ui| friends(ui, state, view.social, view.theirs, icons, adding, trading, acts)),
        Tab::App => app_page(ui, view.switches, acts),
    });
}

fn tracking_status(ui: &mut egui::Ui, view: &View, activity_icons: &mut crate::activity_icons::Icons, typed: &mut Typed, acts: &mut Vec<Act>) {
    let status = view.status;
    let paused = status["paused"] == true;
    let (title, help, colour) = if paused {
        ("Tracking paused", "New activity is not recorded.", look::FAINT)
    } else if status["online"] != true {
        ("Offline · saved locally", "Pending activity syncs when the connection returns.", look::WARM)
    } else if status["excluded"] == true {
        ("Private activity", "This app or site is excluded and earns no XP.", look::FAINT)
    } else if status["idle"] == true {
        ("Idle", "Tracking resumes when you return.", look::FAINT)
    } else if status["program"].is_null() {
        ("Ready to track", "Open an app and start working.", look::GOOD)
    } else if status["class"].is_null() {
        ("Checking this app", "XP starts once this app is classified as productive.", look::WARM)
    } else if status["class"] == "productive" && !view.state["mons"].as_array().into_iter().flatten().any(|mon| mon["onDesk"] == true && mon["away"] != true) {
        ("Tracking · no companion", "Show a companion on the desktop to earn XP.", look::WARM)
    } else if status["class"] == "productive" {
        ("Productive activity detected", "Your next sync saves the XP you earn.", look::GOOD)
    } else {
        ("No XP from this app", "Neutral and distracting activity do not earn XP.", look::WARM)
    };
    if paused || status["excluded"] == true { typed.last_classification = None; }
    if !paused && status["excluded"] != true {
        let current = if status["program"].is_string() { status } else { &status["lastClassification"] };
        if let Some(program) = current["program"].as_str() {
            let (kind, key) = current["site"].as_str().map_or(("app", program), |site| ("site", site));
            typed.last_classification = Some((kind.into(), key.into(), current["class"].as_str().unwrap_or("").into()));
        }
    }
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 18.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.0, colour);
        let width = (ui.available_width() - 80.0).max(80.0);
        look::row_label(ui, egui::RichText::new(title).color(colour), width, 22.0).on_hover_text(help);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(if paused { "Resume" } else { "Pause" }).clicked() { acts.push(Act::Switch("track", !paused)); }
        });
    });
    ui.horizontal(|ui| {
        let details = typed.last_classification.clone();
        let context = if paused || status["excluded"] == true || status["online"] != true || status["idle"] == true {
            help.to_string()
        } else if let Some((kind, key, class)) = &details {
            let name = if kind == "app" { activity_icons.app_name(key) } else { key.clone() };
            format!("{}{name} · {}", if status["program"].is_null() { "Last: " } else { "" }, if class.is_empty() { "checking" } else { class })
        } else { help.to_string() };
        if !paused && status["excluded"] != true && status["online"] == true && status["idle"] != true {
            if let Some((kind, key, _)) = &details {
                activity_icons.show(ui, if kind == "site" { crate::activity_icons::Kind::Site } else { crate::activity_icons::Kind::App }, key, false);
            }
        }
        let width = (ui.available_width() - 70.0).max(80.0);
        let hint = details.as_ref().filter(|(kind, _, _)| kind == "app").map_or_else(|| context.clone(), |(_, key, _)| format!("{context}\n{key}"));
        look::row_label(ui, egui::RichText::new(&context).small().color(look::FAINT), width, 20.0).on_hover_text(hint);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(egui::RichText::new("Details").small(), |ui| {
                ui.set_max_width(280.0);
                ui.strong(title);
                ui.label(help);
                if let Some(age) = status["syncAge"].as_u64() {
                    ui.label(egui::RichText::new(format!("Server checked {age}s ago. Activity syncs about once a minute.")).small().color(look::FAINT));
                }
                if status["class"] == "productive" && !paused {
                    ui.label(egui::RichText::new(format!("Next sync in about {}s.", status["nextReport"].as_u64().unwrap_or(60))).small());
                }
                if let Some((kind, key, _)) = details {
                    ui.separator();
                    ui.label(egui::RichText::new(if kind == "app" { activity_icons.app_name(&key) } else { key.clone() }).strong()).on_hover_text(&key);
                    if kind == "app" { ui.label(egui::RichText::new(&key).small().color(look::FAINT)); }
                    ui.label(egui::RichText::new("Classifications are set by the server and cannot be edited.").small().color(look::FAINT));
                }
                ui.collapsing("How XP works", |ui| {
                    ui.label("Productive activity earns XP. Neutral, distracting, idle, paused, and excluded activity do not. App and site classifications come from the server.");
                });
            });
        });
    });
    if status["accessibilityRequired"] == true {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Website tracking needs Accessibility access.").small().color(look::FAINT));
            if ui.small_button("Enable website tracking").clicked() {acts.push(Act::EnableWebsiteTracking);}
        });
    }
    ui.ctx().request_repaint_after(Duration::from_secs(1));
}

/// The app's own switches, which the icon by the clock has in its menu too, and the way to the web page.
fn app_page(ui: &mut egui::Ui, switches: &[(&'static str, &'static str, bool)], acts: &mut Vec<Act>) {
    look::card(ui, |ui| {
        ui.strong("Preferences");
        for (key, says, on) in switches {
            if *key == "track" || (*key == "taskbar" && !crate::desk::supports_taskbar()) { continue; } // The pause control is visible above every Settings page.
            let mut now = *on;
            if ui.checkbox(&mut now, *says).changed() {
                acts.push(Act::Switch(key, now));
            }
        }
    });
    ui.add_space(6.0);
    look::card(ui, |ui| {
        ui.strong("In your browser");
        ui.label(egui::RichText::new("Your signed-in account keeps your mons across PCs.").color(look::FAINT).small());
        if ui.button("Open the web page").clicked() {
            acts.push(Act::Page);
        }
    });
}

/// The wild mon that is about, if one is: it can be thrown at here as well as on the desktop.
fn wild(ui: &mut egui::Ui, state: &Value, icons: &HashMap<u64, egui::TextureHandle>, acts: &mut Vec<Act>) {
    let encounter = &state["encounter"];
    if encounter.is_null() {
        return;
    }
    look::marked(ui, look::GOLD, |ui| ui.horizontal(|ui| {
        if let Some(icon) = crate::variants::of(encounter).and_then(|dex| icons.get(&dex)) {
            ui.image((icon.id(), egui::vec2(40.0, 40.0)));
        }
        ui.vertical(|ui| {
            ui.label(format!("A wild {}{} is about (level {})", if encounter["shiny"] == true {"shiny "} else {""}, encounter["name"].as_str().unwrap_or("?"), encounter["level"]));
            ui.horizontal(|ui| {
                for (kind, name) in [("ball", "Ball"), ("great", "Great"), ("ultra", "Ultra")] {
                    let have = state[if kind == "ball" { "balls" } else { kind }].as_i64().unwrap_or(0);
                    if (kind == "ball" || have > 0) && ui.add_enabled(have > 0, egui::Button::new(format!("{name} ({have})"))).on_hover_text("Throw one of these at it.").clicked() {
                        acts.push(Act::Throw(kind));
                    }
                }
                ui.label(egui::RichText::new(format!("{:.0}% a ball", encounter["odds"].as_f64().unwrap_or(0.0) * 100.0)).weak());
            });
            // It warms to work done while it is about, and a ball is surer the warmer it is; distraction sends it off.
            let warmth = encounter["warmth"].as_f64().unwrap_or(0.0) as f32;
            ui.add(egui::ProgressBar::new(warmth).desired_width(170.0).desired_height(5.0).fill(look::WARM))
                .on_hover_text("How far it has warmed to you. Keep working and a ball is surer; a minute or two of distraction and it leaves.");
        });
    }));
}

/// The desk as it would be with one mon come onto it, in the place of another if one is named.
fn desk_with(state: &Value, comes: &str, goes: Option<&str>) -> Value {
    let mut wanted: Vec<&str> = state["mons"].as_array().into_iter().flatten().filter(|mon| mon["onDesk"].as_bool().unwrap_or(false)).filter_map(|mon| mon["id"].as_str()).filter(|id| Some(*id) != goes && *id != comes).collect();
    wanted.push(comes);
    json!({ "mons": wanted })
}

/// The mon just caught: who it is, a name for it if one is wanted, and whether it comes onto the desk. What is
/// left of the welcome after this frame (none once it is done with).
fn greet(ui: &mut egui::Ui, state: &Value, icons: &HashMap<u64, egui::TextureHandle>, mut now: Welcome, acts: &mut Vec<Act>) -> Option<Welcome> {
    let id = now.mon["id"].as_str().unwrap_or_default().to_string();
    let species = now.mon["name"].as_str().unwrap_or("?").to_string();
    // As the account has it now, once the server has said: it may have been named or moved since.
    let all: Vec<&Value> = state["mons"].as_array().into_iter().flatten().collect();
    let kept = all.iter().find(|mon| mon["id"] == id.as_str()).copied().unwrap_or(&now.mon);
    let mut over = false;
    look::marked(ui, look::GOLD, |ui| {
        ui.horizontal(|ui| {
            pictured(ui, icons, &now.mon, 56.0);
            ui.vertical(|ui| {
                ui.strong(format!("You caught {} {species}!", if species.starts_with(['A', 'E', 'I', 'O', 'U']) { "an" } else { "a" }));
                ui.label(egui::RichText::new(format!("Level {}{}", kept["level"], kept["nick"].as_str().map_or(String::new(), |nick| format!(", called {nick}")))).weak());
            });
        });
        // A name of its own, if it is to have one.
        ui.horizontal(|ui| {
            ui.label("Name it");
            let field = ui.add(egui::TextEdit::singleline(&mut now.typed).desired_width(120.0).char_limit(16).hint_text(species.as_str()));
            let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if (ui.small_button("Save").clicked() || entered) && !now.typed.trim().is_empty() {
                acts.push(Act::Post("/mons/name", json!({ "mon": id, "name": now.typed.trim() })));
                now.typed.clear();
            }
        });
        // Where it lives: on the desk if there is room, in another's place if there is not, or off it.
        let on_desk: Vec<&Value> = all.iter().filter(|mon| mon["onDesk"].as_bool().unwrap_or(false)).copied().collect();
        let cap = state["deskCap"].as_u64().unwrap_or(3) as usize;
        if kept["onDesk"].as_bool().unwrap_or(false) {
            ui.label(egui::RichText::new("It has come onto your desk.").weak().small());
        } else if on_desk.len() < cap {
            if ui.small_button("Put it on the desk").clicked() {
                acts.push(Act::Post("/mons/desk", desk_with(state, &id, None)));
            }
        } else {
            ui.label(egui::RichText::new("Your desk is full, so it waits with the rest of your mons. It can take the place of:").weak().small());
            ui.horizontal_wrapped(|ui| {
                for other in on_desk {
                    if ui.small_button(format!("{} Lv {}", other["nick"].as_str().or(other["name"].as_str()).unwrap_or("?"), other["level"])).clicked() {
                        acts.push(Act::Post("/mons/desk", desk_with(state, &id, other["id"].as_str())));
                    }
                }
            });
        }
        if ui.button("Done").clicked() {
            // A name typed and not saved is saved: it was meant.
            if !now.typed.trim().is_empty() {
                acts.push(Act::Post("/mons/name", json!({ "mon": id, "name": now.typed.trim() })));
            }
            over = true;
        }
    });
    ui.add_space(4.0);
    (!over).then_some(now)
}

fn mons(ui: &mut egui::Ui, state: &Value, icons: &HashMap<u64, egui::TextureHandle>, naming: &mut Option<(String, String)>, acts: &mut Vec<Act>) {
    let all: Vec<&Value> = state["mons"].as_array().into_iter().flatten().collect();
    let cap = state["deskCap"].as_u64().unwrap_or(3) as usize;
    let on_desk: Vec<&str> = all.iter().filter(|mon| mon["onDesk"].as_bool().unwrap_or(false)).filter_map(|mon| mon["id"].as_str()).collect();
    ui.label(egui::RichText::new(format!("{} companion{} · {} / {} on desktop", all.len(), if all.len() == 1 { "" } else { "s" }, on_desk.len(), cap)).color(look::FAINT).small());
    for mon in &all {
        let id = mon["id"].as_str().unwrap_or_default();
        let mut out = on_desk.contains(&id);
        let (name, nick) = (mon["name"].as_str().unwrap_or("?"), mon["nick"].as_str());
        let trip = state["expeditions"].as_array().into_iter().flatten().find(|trip| trip["mon"] == id).filter(|_| mon["away"].as_bool().unwrap_or(false));
        look::card(ui, |ui| {
            // Who it is: its picture, its name and its species', its types, and its level.
            ui.horizontal(|ui| {
                match crate::variants::of(mon).and_then(|dex| icons.get(&dex)) {
                    Some(icon) => {
                        ui.add(egui::Image::new((icon.id(), egui::vec2(44.0, 44.0))).tint(if trip.is_some() { egui::Color32::from_gray(110) } else { egui::Color32::WHITE }));
                    }
                    None => {
                        ui.allocate_space(egui::vec2(44.0, 44.0));
                    }
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(nick.unwrap_or(name)).size(15.0).strong());
                        ui.label(egui::RichText::new(format!("Level {}", mon["level"])).small().color(look::FAINT));
                        if mon["shiny"] == true { look::chip(ui,"Shiny",look::GOLD); }
                        if nick.is_some() {
                            ui.label(egui::RichText::new(name).color(look::FAINT).small());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for kind in mon["types"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                            look::chip(ui, kind, look::type_colour(kind));
                        }
                    });
                });
            });
            // Away on an expedition, it has nothing to do here but come back.
            if let Some(trip) = trip {
                let (done, need, trip_id) = (trip["done"].as_f64().unwrap_or(0.0), trip["need"].as_f64().unwrap_or(1.0).max(1.0), trip["id"].as_str().unwrap_or_default());
                look::bar(ui, (done / need) as f32, 7.0, look::WARM);
                ui.horizontal(|ui| {
                    if done >= need {
                        ui.label(egui::RichText::new("It is back with what it found.").color(look::GOLD));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Welcome it back").clicked() {
                                acts.push(Act::Post("/expedition/claim", json!({ "id": trip_id })));
                            }
                        });
                    } else {
                        ui.label(egui::RichText::new(format!("On an expedition: {} of your work to go", spell(need - done))).color(look::FAINT).small());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Call it back").on_hover_text("It comes back now, with nothing.").clicked() {
                                acts.push(Act::Post("/expedition/recall", json!({ "id": trip_id })));
                            }
                        });
                    }
                });
                return;
            }
            // How far it is to its next level.
            let (xp, next) = (mon["xp"].as_f64().unwrap_or(0.0), mon["xpToNext"].as_f64().unwrap_or(0.0));
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 100.0).max(40.0);
                ui.add(egui::ProgressBar::new(if next > 0.0 { (xp / next) as f32 } else { 1.0 }).desired_width(width).desired_height(8.0).fill(look::XP));
                ui.label(egui::RichText::new(if next > 0.0 { format!("{} / {} XP", thousands(xp), thousands(next)) } else { "Max level".to_string() }).small().color(look::FAINT));
            });
            // Being named: a line to type in, kept by Save or the Enter key, dropped by Cancel.
            if naming.as_ref().is_some_and(|(of, _)| of == id) {
                ui.horizontal(|ui| {
                    let mut typed = naming.take().map(|(_, typed)| typed).unwrap_or_default();
                    let field = ui.add(egui::TextEdit::singleline(&mut typed).desired_width(140.0).char_limit(16).hint_text(name));
                    let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if ui.button("Save").clicked() || entered {
                        acts.push(Act::Post("/mons/name", json!({ "mon": id, "name": typed.trim() })));
                    } else if !ui.button("Cancel").clicked() {
                        *naming = Some((id.to_string(), typed));
                    }
                });
                return;
            }
            // What can be done with it, each thing by its name.
            ui.horizontal_wrapped(|ui| {
                // With the desk full, one that is off it can take the place of one that is on it.
                if !out && on_desk.len() >= cap {
                    let named = |other: &str| all.iter().find(|mon| mon["id"] == other).map_or("?".to_string(), |mon| format!("{} Lv {}", mon["nick"].as_str().or(mon["name"].as_str()).unwrap_or("?"), mon["level"]));
                    if let [only] = on_desk[..] {
                        if ui.button("Swap in").on_hover_text(format!("Comes onto the desk in {}'s place.", named(only))).clicked() {
                            acts.push(Act::Post("/mons/desk", desk_with(state, id, Some(only))));
                        }
                    } else {
                        ui.menu_button("Swap in", |ui| {
                            ui.label(egui::RichText::new("In the place of:").color(look::FAINT).small());
                            for other in &on_desk {
                                if ui.button(named(other)).clicked() {
                                    acts.push(Act::Post("/mons/desk", desk_with(state, id, Some(other))));
                                }
                            }
                        });
                    }
                } else if ui.add_enabled(out || on_desk.len() < cap, egui::Checkbox::new(&mut out, "Show on desktop")).on_hover_text("Whether it is out on your desktop, earning as you work.").changed() {
                    let wanted: Vec<&str> = if out { on_desk.iter().copied().chain([id]).collect() } else { on_desk.iter().copied().filter(|other| *other != id).collect() };
                    acts.push(Act::Post("/mons/desk", json!({ "mons": wanted })));
                }
                ui.menu_button("Manage", |ui| {
                if ui.button("Rename").on_hover_text("Give it a name of your own. Saved empty, it goes back to its species' name.").clicked() {
                    *naming = Some((id.to_string(), nick.unwrap_or("").to_string())); ui.close();
                }
                if state["expeditions"].is_array() {
                    ui.menu_button("Expedition", |ui| {
                        ui.label(egui::RichText::new("Off on an expedition while you work.\nThe higher its level, the more it brings back.").color(look::FAINT).small());
                        for (kind, name) in [("short", "Short: back after 30 min of your work"), ("long", "Long: after 2 hours of it"), ("far", "Far: after 6 hours, likeliest to bring a wild mon home")] {
                            if ui.button(name).clicked() {
                                acts.push(Act::Post("/expedition/start", json!({ "mon": id, "kind": kind })));
                            }
                        }
                    });
                }
                let (points, berry) = (state["points"].as_i64().unwrap_or(0), state["prices"]["berry"].as_i64());
                if let Some(berry) = berry {
                    if ui.add_enabled(points >= berry, egui::Button::new(format!("Berry ({berry})"))).on_hover_text("A treat: five busy minutes' worth of experience.").clicked() {
                        acts.push(Act::Post("/shop/buy", json!({ "item": "berry", "mon": id })));
                    }
                }
                });
            });
        });
    }
}

/// The programs the account has used, by what the server says each counts as. To read, not to change: what
/// counts as work is the server's to say, the same for everybody.

fn programs(ui: &mut egui::Ui, state: &Value, private: &[String], activity_icons: &mut crate::activity_icons::Icons, keeping: &mut String, acts: &mut Vec<Act>) {
    ui.label(egui::RichText::new("Productivity classifications are managed by the server.").small().color(look::FAINT));
    ui.add_space(6.0);
    let waiting: Vec<&str> = state["seen"].as_array().into_iter().flatten().filter_map(|seen| seen["exe"].as_str()).collect();
    if !waiting.is_empty() {
        ui.strong("Checking new apps");
        for exe in waiting {
            ui.horizontal(|ui| {
                activity_icons.show(ui, crate::activity_icons::Kind::App, exe, private.iter().any(|name| name == exe));
                ui.add(egui::Label::new(egui::RichText::new(activity_icons.app_name(exe)).small().color(look::FAINT)).truncate()).on_hover_text(exe);
            });
        }
        ui.add_space(8.0);
    }
    let rules: Vec<(&String, &str)> = state["rules"].as_object().into_iter().flatten().filter(|(exe, _)| !exe.starts_with("site:")).filter_map(|(exe, class)| Some((exe, class.as_str()?))).collect();
    if rules.is_empty() {
        ui.strong("Your apps");
        ui.label(egui::RichText::new("Apps appear here as you use them.").small().color(look::FAINT));
    }
    for (class, name) in CLASSES {
        let of_class: Vec<&String> = rules.iter().filter(|(_, c)| *c == class).map(|(exe, _)| *exe).collect();
        if of_class.is_empty() { continue; }
        ui.strong(name);
        for exe in of_class {
            ui.horizontal(|ui| {
                activity_icons.show(ui, crate::activity_icons::Kind::App, exe, private.contains(exe));
                let width = (ui.available_width() - 110.0).max(60.0);
                look::row_label(ui, activity_icons.app_name(exe), width, 28.0).on_hover_text(exe);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Never report").clicked() { acts.push(Act::Private(exe.to_string(), true)); }

                });
            });
        }
        ui.add_space(8.0);
    }
    ui.separator();
    egui::CollapsingHeader::new(format!("Privacy exclusions ({})", private.len())).default_open(!private.is_empty()).show(ui, |ui| {
        ui.label(egui::RichText::new("Excluded apps and sites are never timed or reported.").small().color(look::FAINT));
        for name in private {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 115.0).max(60.0);
                let label = name.strip_prefix("site:").map(str::to_string).unwrap_or_else(|| activity_icons.app_name(name));
                look::row_label(ui, label, width, 24.0).on_hover_text(name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Report again").clicked() { acts.push(Act::Private(name.clone(), false)); }
                });
            });
        }
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 60.0).max(80.0);
            let field = ui.add(egui::TextEdit::singleline(keeping).desired_width(width).hint_text("program.exe"));
            let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if (ui.button("Add").clicked() || entered) && !keeping.trim().is_empty() { acts.push(Act::Private(std::mem::take(keeping), true)); }
        });
    });
}

/// The sites met in the browser, by the name their pages' titles end with, and what each counts as.
fn sites(ui: &mut egui::Ui, sites: &[(String, Option<&'static str>, bool)], private: &[String], activity_icons: &mut crate::activity_icons::Icons, acts: &mut Vec<Act>) {
    ui.label(egui::RichText::new("A page counts as what it is, not as its browser. What that is, the server decides.").weak().small());
    ui.add_space(4.0);
    if sites.is_empty() {
        ui.label("No sites yet. They turn up here as you browse.");
    }
    for (site, class, whole) in sites {
        ui.horizontal(|ui| {
            activity_icons.show(ui, crate::activity_icons::Kind::Site, site, private.contains(&format!("site:{site}")));
            let width = (ui.available_width() - 110.0).max(60.0);
            ui.allocate_ui_with_layout(egui::vec2(width, 38.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add(egui::Label::new(site.as_str()).truncate()).on_hover_text(site);
                let (said, hint) = match (class, whole) {
                    (Some(class), true) => ((*class).to_string(), "Classification set by the server."),
                    (Some(class), false) => ((*class).to_string(), "Every judged page has the same classification."),
                    (None, _) => ("Page by page".to_string(), "Each page is judged by its title."),
                };
                ui.label(egui::RichText::new(said).weak().small()).on_hover_text(hint);
            });
            if !private.contains(&format!("site:{site}")) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Never report").on_hover_text("From now on this site's pages are not timed and their titles go nowhere.").clicked() {
                        acts.push(Act::Private(format!("site:{site}"), true));
                    }
                });
            }
        });
    }
}

/// A number with its thousands apart, as hiscores have them: 1,234,567.
fn thousands(number: f64) -> String {
    let digits = format!("{:.0}", number.max(0.0));
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The hiscores: where each of your mons stands among every mon of its species, one species' table, or
/// everybody's accounts overall.
fn ranks(ui: &mut egui::Ui, state: &Value, tables: &HashMap<String, Value>, icons: &HashMap<u64, egui::TextureHandle>, ranking: &mut Option<String>) {
    // Which table: yours, overall, or a species among those you have seen.
    ui.horizontal(|ui| {
        if look::sub_tab(ui, "Your mons", ranking.is_none()).clicked() {
            *ranking = None;
        }
        if look::sub_tab(ui, "Overall", ranking.as_deref() == Some("")).clicked() {
            *ranking = Some(String::new());
        }
        let entries: Vec<&Value> = state["dex"]["entries"].as_array().into_iter().flatten().collect();
        let picked: Option<String> = ranking.clone().filter(|species| !species.is_empty());
        let picked = picked.as_deref();
        let shown = picked.map_or("A species", |species| entries.iter().find(|entry| entry["species"] == species).and_then(|entry| entry["name"].as_str()).unwrap_or(species)).to_string();
        egui::ComboBox::from_id_salt("ranked").selected_text(shown).show_ui(ui, |ui| {
            for entry in entries {
                let (species, name) = (entry["species"].as_str().unwrap_or_default(), entry["name"].as_str().unwrap_or("?"));
                if ui.selectable_label(picked == Some(species), name).clicked() {
                    *ranking = Some(species.to_string());
                }
            }
        });
    });
    ui.add_space(4.0);
    let Some(table) = tables.get(&ranks_path(ranking)) else {
        ui.label("Asking the server...");
        return;
    };
    let highlight = look::GOOD;
    let Some(species) = ranking.clone() else {
        // Your own: each mon's place among its kind, and yours among everybody. A mon's row opens its table.
        if let Some(overall) = table["overall"].as_object() {
            ui.label(format!("Overall you are {} of {}: {} levels, {} xp in all", thousands(overall["rank"].as_f64().unwrap_or(0.0)), thousands(overall["of"].as_f64().unwrap_or(0.0)), overall["level"], thousands(overall["xp"].as_f64().unwrap_or(0.0))));
            ui.add_space(4.0);
        }
        egui::Grid::new("mine").num_columns(4).spacing([14.0, 4.0]).striped(true).show(ui, |ui| {
            for mon in table["mons"].as_array().into_iter().flatten() {
                pictured(ui, icons, mon, 28.0);
                if ui.link(called(mon)).on_hover_text("Open this species' table.").clicked() {
                    *ranking = mon["species"].as_str().map(str::to_string);
                }
                ui.label(format!("{} xp", thousands(mon["xp"].as_f64().unwrap_or(0.0))));
                ui.label(egui::RichText::new(format!("{} of {}", thousands(mon["rank"].as_f64().unwrap_or(0.0)), thousands(mon["of"].as_f64().unwrap_or(0.0)))).strong());
                ui.end_row();
            }
        });
        if table["mons"].as_array().is_none_or(Vec::is_empty) {
            ui.label("You have no mons to rank yet.");
        }
        return;
    };
    let overall = species.is_empty();
    ui.horizontal(|ui| {
        if !overall {
            pictured(ui, icons, table, 36.0);
        }
        ui.vertical(|ui| {
            look::heading(ui, if overall { "Overall" } else { table["name"].as_str().unwrap_or("?") });
            ui.label(egui::RichText::new(if overall { format!("{} trainers", thousands(table["total"].as_f64().unwrap_or(0.0))) } else { format!("{} of them, everywhere", thousands(table["total"].as_f64().unwrap_or(0.0))) }).weak().small());
        });
    });
    let rows: Vec<&Value> = table["rows"].as_array().into_iter().flatten().collect();
    // Yours that the table's top does not reach are put under it, as a hiscore page shows your own line.
    let below: Vec<&Value> = table["yours"].as_array().into_iter().flatten().filter(|mine| !rows.iter().any(|row| row["rank"] == mine["rank"])).collect();
    egui::Grid::new("table").num_columns(4).spacing([14.0, 4.0]).striped(true).show(ui, |ui| {
        for head in ["Rank", "Trainer", "Level", "XP"] {
            ui.label(egui::RichText::new(head).weak().small());
        }
        ui.end_row();
        let line = |ui: &mut egui::Ui, row: &Value, you: bool| {
            let backdrop = you.then(|| ui.painter().add(egui::Shape::Noop));
            let paint = |text: String| if you { egui::RichText::new(text).color(highlight).strong() } else { egui::RichText::new(text) };
            let first = ui.label(paint(thousands(row["rank"].as_f64().unwrap_or(0.0))));
            let owner = if you { state["account"]["name"].as_str().unwrap_or("you") } else { row["owner"].as_str().unwrap_or("?") };
            ui.label(paint(match (overall, row["nick"].as_str()) {
                (false, Some(nick)) => format!("{owner}'s {nick}"),
                (true, _) => owner.to_string(),
                _ => owner.to_string(),
            })).on_hover_text(if overall { format!("{} mons", row["mons"]) } else { owner.to_string() });
            ui.label(paint(row["level"].to_string()));
            let last = ui.label(paint(thousands(row["xp"].as_f64().unwrap_or(0.0))));
            if let Some(backdrop) = backdrop {
                ui.painter().set(backdrop, egui::Shape::rect_filled(first.rect.union(last.rect).expand2(egui::vec2(3.0, 2.0)), egui::CornerRadius::same(5), look::SELECTED));
            }
            ui.end_row();
        };
        for row in &rows {
            line(ui, row, row["you"].as_bool().unwrap_or(false));
        }
        if !below.is_empty() {
            for _ in 0..4 {
                ui.label(egui::RichText::new("...").weak());
            }
            ui.end_row();
            for mine in below {
                line(ui, mine, true);
            }
        }
    });
}

/// A mon as a few words: its given name or its species', and its level.
fn called(mon: &Value) -> String {
    format!("{} (level {})", mon["nick"].as_str().or(mon["name"].as_str()).unwrap_or("?"), mon["level"])
}

/// A species' picture at this size, or the room it would take.
fn pictured(ui: &mut egui::Ui, icons: &HashMap<u64, egui::TextureHandle>, mon: &Value, side: f32) {
    match crate::variants::of(mon).and_then(|dex| icons.get(&dex)) {
        Some(icon) => {
            ui.image((icon.id(), egui::vec2(side, side)));
        }
        None => {
            ui.allocate_space(egui::vec2(side, side));
        }
    }
}

/// Friends: the day's experience of each beside your own, who has asked and been asked, and the trades.
#[allow(clippy::too_many_arguments)]
fn friends(ui: &mut egui::Ui, state: &Value, social: &Value, theirs: &HashMap<String, Value>, icons: &HashMap<u64, egui::TextureHandle>, adding: &mut String, trading: &mut Option<Trading>, acts: &mut Vec<Act>) {
    if social.is_null() {
        ui.label("Asking the server about your friends...");
        return;
    }
    // A trade being put together takes the tab until it is offered or dropped.
    if let Some(now) = trading.take() {
        *trading = trade(ui, state, theirs, icons, now, acts);
        return;
    }
    let list = |key: &str| -> Vec<&Value> { social[key].as_array().into_iter().flatten().collect() };

    // The board: everybody's experience today, most first.
    ui.strong("Today");
    let board = list("board");
    for (place, row) in board.iter().enumerate() {
        let (id, you) = (row["id"].as_str().unwrap_or_default(), row["you"].as_bool().unwrap_or(false));
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}", place + 1)).weak());
            pictured(ui, icons, &row["mon"], 36.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let name = row["name"].as_str().unwrap_or("?");
                    if you {
                        ui.label(egui::RichText::new(name).strong().color(look::GOOD));
                        ui.label(egui::RichText::new("you").weak().small());
                    } else {
                        ui.strong(name);
                    }
                });
                let with = if row["mon"].is_null() { "no mon yet".to_string() } else { format!("with {}", called(&row["mon"])) };
                ui.label(egui::RichText::new(format!("{} productive, {with}", spell(row["productive"].as_f64().unwrap_or(0.0)))).weak().small());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !you {
                    ui.menu_button("...", |ui| {
                        if ui.button("Offer a trade").clicked() {
                            *trading = Some(Trading { with: id.to_string(), name: row["name"].as_str().unwrap_or("?").to_string(), give: None, want: None });
                            acts.push(Act::FriendMons(id.to_string()));
                        }
                        if ui.button("Stop being friends").clicked() {
                            acts.push(Act::Social("/friends/remove", json!({ "id": id })));
                        }
                    });
                }
                ui.label(format!("{:.0} xp", row["xp"].as_f64().unwrap_or(0.0)));
            });
        });
    }
    if board.len() < 2 {
        ui.label(egui::RichText::new("Nobody here but you yet. Give a friend your code, or type theirs below.").weak().small());
    }

    // Trades offered either way.
    let trades = list("trades");
    if !trades.is_empty() {
        ui.add_space(6.0);
        ui.separator();
        ui.strong("Trades");
    }
    for offer in trades {
        let (id, mine, other) = (offer["id"].as_str().unwrap_or_default(), offer["mine"].as_bool().unwrap_or(false), offer["with"]["name"].as_str().unwrap_or("?"));
        // Said from where you stand: what leaves you first, what comes to you second.
        let (yours, their) = if mine { (&offer["give"], &offer["want"]) } else { (&offer["want"], &offer["give"]) };
        ui.horizontal(|ui| {
            pictured(ui, icons, yours, 32.0);
            ui.label("for");
            pictured(ui, icons, their, 32.0);
            ui.vertical(|ui| {
                ui.label(if mine { format!("You asked {other}") } else { format!("{other} asks you") });
                ui.label(egui::RichText::new(format!("your {} for their {}", called(yours), called(their))).weak().small());
            });
        });
        ui.horizontal(|ui| {
            if mine {
                if ui.small_button("Take it back").clicked() {
                    acts.push(Act::Social("/trades/cancel", json!({ "id": id })));
                }
            } else {
                if ui.small_button("Trade").on_hover_text("The two mons change hands at once. Each keeps its level and its name.").clicked() {
                    acts.push(Act::Social("/trades/accept", json!({ "id": id })));
                }
                if ui.small_button("No thanks").clicked() {
                    acts.push(Act::Social("/trades/cancel", json!({ "id": id })));
                }
            }
        });
        ui.add_space(4.0);
    }

    // Who has asked, and who has been asked.
    let (incoming, outgoing) = (list("incoming"), list("outgoing"));
    if !incoming.is_empty() || !outgoing.is_empty() {
        ui.add_space(6.0);
        ui.separator();
    }
    for asker in incoming {
        let id = asker["id"].as_str().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(format!("{} wants to be friends", asker["name"].as_str().unwrap_or("?")));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("No").clicked() {
                    acts.push(Act::Social("/friends/remove", json!({ "id": id })));
                }
                if ui.small_button("Yes").clicked() {
                    acts.push(Act::Social("/friends/accept", json!({ "id": id })));
                }
            });
        });
    }
    for asked in outgoing {
        let id = asked["id"].as_str().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Asked {}: no answer yet", asked["name"].as_str().unwrap_or("?"))).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Take it back").clicked() {
                    acts.push(Act::Social("/friends/remove", json!({ "id": id })));
                }
            });
        });
    }

    // Your code to give away, and somewhere to type somebody's.
    ui.add_space(6.0);
    ui.separator();
    ui.horizontal(|ui| {
        let code = social["code"].as_str().unwrap_or("?");
        ui.label("Your friend code");
        ui.label(egui::RichText::new(code).monospace().strong());
        if ui.small_button("Copy").clicked() {
            crate::desk::copy(code);
        }
    });
    ui.horizontal(|ui| {
        let field = ui.add(egui::TextEdit::singleline(adding).desired_width(110.0).char_limit(12).hint_text("a friend's code"));
        let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if (ui.small_button("Add").clicked() || entered) && !adding.trim().is_empty() {
            acts.push(Act::Social("/friends/add", json!({ "code": std::mem::take(adding).trim() })));
        }
    });
    ui.label(egui::RichText::new("Friends see your name, how long you were productive each day, the experience your mons earned, and your mons. Never which programs or sites.").weak().small());
}

/// Putting a trade together: one of yours, one of theirs. What is left of it after this frame (none once it is
/// offered or dropped).
fn trade(ui: &mut egui::Ui, state: &Value, theirs: &HashMap<String, Value>, icons: &HashMap<u64, egui::TextureHandle>, mut now: Trading, acts: &mut Vec<Act>) -> Option<Trading> {
    ui.strong(format!("A trade with {}", now.name));
    ui.label(egui::RichText::new("Pick one of yours and one of theirs. Nothing changes hands until they say yes.").weak().small());
    let pick = |ui: &mut egui::Ui, mons: &Value, picked: &mut Option<String>| {
        for mon in mons.as_array().into_iter().flatten() {
            let id = mon["id"].as_str().unwrap_or_default();
            ui.horizontal(|ui| {
                pictured(ui, icons, mon, 28.0);
                if ui.selectable_label(picked.as_deref() == Some(id), called(mon)).clicked() {
                    *picked = Some(id.to_string());
                }
            });
        }
    };
    ui.add_space(4.0);
    ui.strong("You give");
    pick(ui, &state["mons"], &mut now.give);
    ui.add_space(4.0);
    ui.strong(format!("{} gives", now.name));
    match theirs.get(&now.with) {
        Some(answer) if answer["mons"].as_array().is_some_and(|mons| !mons.is_empty()) => pick(ui, &answer["mons"], &mut now.want),
        Some(_) => {
            ui.label(egui::RichText::new("They have no mons to trade.").weak());
        }
        None => {
            ui.label(egui::RichText::new("Asking what they have...").weak());
        }
    }
    ui.add_space(8.0);
    let mut over = false;
    ui.horizontal(|ui| {
        if ui.add_enabled(now.give.is_some() && now.want.is_some(), egui::Button::new("Offer it")).clicked() {
            acts.push(Act::Social("/trades/offer", json!({ "to": now.with, "give": now.give, "want": now.want })));
            over = true;
        }
        if ui.button("Never mind").clicked() {
            over = true;
        }
    });
    (!over).then_some(now)
}



/// Seconds as hours and minutes: "1 h 12 min", "45 min".
fn spell(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor() as i64;
    if minutes >= 60 { format!("{} h {:02} min", minutes / 60, minutes % 60) } else { format!("{minutes} min") }
}

/// The day at a glance: the focus session, the day against its goal, and what the last work earned and why.
/// The week and the days before are a press away.
fn today(ui: &mut egui::Ui, state: &Value, earning: &Value, icons: &HashMap<u64,egui::TextureHandle>, typed: &mut Typed, acts: &mut Vec<Act>) {
    let day = &state["today"];
    let (done, goal) = (day["productive"].as_f64().unwrap_or(0.0), day["goal"].as_f64().unwrap_or(10800.0).max(1.0));
    let met = day["goalMet"].as_bool().unwrap_or(false);
    look::card(ui, |ui| {
        look::heading(ui, "Today");
        ui.label(egui::RichText::new(spell(done)).size(30.0).color(if met { look::GOOD } else { look::INK }));
        ui.label(egui::RichText::new(format!("productive · {} daily goal", spell(goal))).small().color(look::FAINT));
        look::bar(ui, (done / goal) as f32, 6.0, look::XP);
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("{} XP earned", thousands(day["xp"].as_f64().unwrap_or(0.0)))).small());
            ui.label(egui::RichText::new(format!("{} points today", thousands(day["points"].as_f64().unwrap_or(0.0)))).small().color(look::FAINT));
        });
        lure_banner(ui,state);
        if !state["lure"].is_object() && state["encounter"].is_null() {
            ui.label(egui::RichText::new("Use a lure from the Shop to meet wild mons.").small().color(look::FAINT));
        }
        let streak = state["dayStreak"].as_i64().unwrap_or(0);
        if met || streak > 0 {
            ui.label(egui::RichText::new(if met { "Daily goal reached".to_string() } else { format!("{streak}-day streak") }).small().color(if met { look::GOOD } else { look::FAINT }));
        }
        if let Some(mon) = state["mons"].as_array().into_iter().flatten().find(|mon| mon["onDesk"] == true && mon["away"] != true) {
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                pictured(ui, icons, mon, 40.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(mon["nick"].as_str().filter(|name| !name.is_empty()).or(mon["name"].as_str()).unwrap_or("Your companion"));
                    ui.label(egui::RichText::new(format!("Level {}", mon["level"])).small().color(look::FAINT));
                        if mon["shiny"] == true { look::chip(ui,"Shiny",look::GOLD); }
                });
            });
            let xp = mon["xp"].as_f64().unwrap_or(0.0);
            let next = mon["xpToNext"].as_f64().unwrap_or(0.0);
            look::bar(ui, if next > 0.0 { (xp / next) as f32 } else { 1.0 }, 5.0, look::XP);
            ui.label(egui::RichText::new(if next > 0.0 { format!("{} / {} XP to next level", thousands(xp), thousands(next)) } else { "Max level".to_string() }).small().color(look::FAINT));
        }
        if state["tracking"]["hasEarnedXP"] == false {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Work in a productive app to earn your first XP.").small().color(look::FAINT));
        }
    });
    ui.add_space(12.0);
    look::card(ui, |ui| session(ui, state, typed, acts));


    let rested = state["rested"].as_f64().unwrap_or(0.0);
    if rested > 0.0 {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("Rested · bonus XP for {}", spell(rested.max(60.0)))).small().color(look::GOOD));
    } else if state["breakDue"] == true {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Time for a break. Step away for three minutes to recharge.").small().color(look::WARM));
    }
    ui.add_space(10.0);
    if ui.link(egui::RichText::new(if typed.more { "Hide details" } else { "Activity details" }).small()).clicked() { typed.more = !typed.more; }
    if !typed.more { return; }
    if earning["factor"].is_number() { look::card(ui, |ui| earned(ui, earning)); ui.add_space(12.0); }
    if let Some(season) = state["season"]["name"].as_str() {
        ui.label(egui::RichText::new(format!("{season} season · more {}-type visitors", state["season"]["type"].as_str().unwrap_or("?"))).small().color(look::FAINT));
    }
    ui.add_space(6.0);
    look::card(ui, |ui| {
        ui.strong("The day in full");
        egui::Grid::new("today").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
            ui.label("Current focus");
            ui.label(spell(state["streak"].as_f64().unwrap_or(0.0)));
            ui.end_row();
            for (name, key) in [("Productive", "productive"), ("Neutral", "neutral"), ("Distracting", "distracting"), ("Longest focus", "longestStreak")] {
                ui.label(name);
                ui.label(spell(day[key].as_f64().unwrap_or(0.0)));
                ui.end_row();
            }
            for (name, key) in [("Caught", "caught"), ("Sessions kept", "sessions")] {
                ui.label(name);
                ui.label(day[key].as_i64().unwrap_or(0).to_string());
                ui.end_row();
            }
        });
    });
    if state["week"]["this"].is_object() {
        ui.add_space(6.0);
        look::card(ui, |ui| week(ui, state));
    }
    // The days before: a square a day, greener the nearer its goal, its numbers under the mouse.
    let history: Vec<&Value> = state["history"].as_array().into_iter().flatten().collect();
    if history.len() > 1 {
        ui.add_space(6.0);
        look::card(ui, |ui| {
            ui.strong("The days before");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                for past in history {
                    let share = (past["productive"].as_f64().unwrap_or(0.0) / goal).clamp(0.0, 1.0) as f32;
                    let colour = if past["goalMet"].as_bool().unwrap_or(false) { look::GOOD } else { egui::Color32::from_rgb((234.0 - 113.0 * share) as u8, (226.0 - 59.0 * share) as u8, (210.0 - 135.0 * share) as u8) };
                    let (rect, sense) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(4), colour);
                    sense.on_hover_text(format!("{}\n{} productive, {} distracting\n{} caught", past["day"].as_str().unwrap_or("?"), spell(past["productive"].as_f64().unwrap_or(0.0)), spell(past["distracting"].as_f64().unwrap_or(0.0)), past["caught"].as_i64().unwrap_or(0)));
                }
            });
        });
    }
}

/// Why the last stretch of work earned the experience it did: what it could have earned, and each thing that
/// came off it or went onto it.
fn earned(ui: &mut egui::Ui, earning: &Value) {
    let (Some(factor), Some(xp)) = (earning["factor"].as_f64(), earning["xp"].as_f64()) else { return };
    let full = if factor > 0.0 { xp / factor } else { 0.0 };
    let seconds = earning["seconds"].as_f64().unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.strong(format!("+{xp:.1} xp a mon"));
        ui.label(egui::RichText::new(if factor > 0.0 { format!("for the last {seconds:.0} s of work, of {full:.1} at the most") } else { format!("for the last {seconds:.0} s of work") }).weak().small());
    });
    let (sure, active, by_active, rested) = (earning["sure"].as_f64().unwrap_or(1.0), earning["active"].as_f64().unwrap_or(1.0), earning["byActive"].as_f64().unwrap_or(1.0), earning["rested"].as_f64().unwrap_or(1.0));
    let (good, less) = (look::GOOD, look::WARM);
    let line = |ui: &mut egui::Ui, times: f64, why: String| {
        ui.horizontal(|ui| {
            let colour = if times > 1.0 { good } else if times < 0.995 { less } else { ui.visuals().weak_text_color() };
            ui.label(egui::RichText::new(format!("x{times:.2}")).monospace().small().color(colour));
            ui.label(egui::RichText::new(why).weak().small());
        });
    };
    let what = earning["in"].as_str().unwrap_or("what you were in");
    line(ui, sure, if sure < 0.995 { format!("the server is {:.0}% sure that {what} is work", sure * 100.0) } else { format!("the server is sure that {what} is work") });
    line(ui, by_active, if active < 0.995 { format!("the keyboard or mouse was touched in {:.0}% of it", active * 100.0) } else { "you were at the keyboard or mouse all through".to_string() });
    if rested > 1.0 {
        line(ui, rested, "rested after a break".to_string());
    }
}

/// A focus session: said beforehand how long, kept or not at the end of it.
fn session(ui: &mut egui::Ui, state: &Value, typed: &mut Typed, acts: &mut Vec<Act>) {
    let going = &state["session"];
    if going.is_object() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |since| since.as_millis() as f64);
        let (from, to) = (going["startedAt"].as_f64().unwrap_or(now), going["endsAt"].as_f64().unwrap_or(now));
        let left = ((to - now) / 1000.0).max(0.0);
        ui.horizontal(|ui| {
            ui.strong("Focus session");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Stop").on_hover_text("End this session without a bonus.").clicked() { acts.push(Act::Post("/session/stop", json!({}))); }
            });
        });
        ui.label(egui::RichText::new(if left < 60.0 { "Under a minute left".to_string() } else { format!("{} left", spell(left)) }).size(22.0));
        if let Some(label) = going["label"].as_str().filter(|label| !label.is_empty()) { ui.label(egui::RichText::new(label).small().color(look::FAINT)); }
        look::bar(ui, (((now - from) / (to - from).max(1.0)) as f32).clamp(0.0, 1.0), 6.0, look::XP);
        ui.label(egui::RichText::new(format!("{} productive so far", spell(going["productive"].as_f64().unwrap_or(0.0)))).small().color(look::FAINT));
        if going["distracting"].as_f64().unwrap_or(0.0) > 30.0 { ui.label(egui::RichText::new("Too much distraction can lose the session bonus.").small().color(look::WARM)); }
        return;
    }
    look::heading(ui, "Focus timer");
    ui.add_space(2.0);
    let durations = [25, 45, 60, 90];
    typed.focus_duration = typed.focus_duration.min(durations.len() - 1);
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("focus-duration").width(76.0).selected_text(format!("{} min", durations[typed.focus_duration])).show_ui(ui, |ui| {
            for (index, minutes) in durations.iter().enumerate() { ui.selectable_value(&mut typed.focus_duration, index, format!("{minutes} min")); }
        });
        let start = egui::Button::new(egui::RichText::new("Start focus").color(egui::Color32::WHITE)).fill(look::ACCENT);
        if ui.add_sized([120.0, 32.0], start).on_hover_text("An optional timer. Productive activity earns XP with or without a focus session.").clicked() {
            acts.push(Act::Post("/session/start", json!({"minutes":durations[typed.focus_duration],"label":std::mem::take(&mut typed.label).trim()})));
        }
    });
    ui.collapsing("Add a task", |ui| {
        ui.add(egui::TextEdit::singleline(&mut typed.label).desired_width(f32::INFINITY).char_limit(40).hint_text("What are you working on?"));
    });
    let last = &state["lastSession"];
    if last.is_object() {
        ui.label(egui::RichText::new(if last["kept"] == true { format!("Last session complete · +{} points", last["points"]) } else if last["stopped"] == true { "Last session stopped".to_string() } else { "Last session finished without a bonus".to_string() }).small().color(look::FAINT));
    }
}

/// This week beside the last, and the bests there have been.
fn week(ui: &mut egui::Ui, state: &Value) {
    let (this, last, bests) = (&state["week"]["this"], &state["week"]["last"], &state["week"]["bests"]);
    if !this.is_object() {
        return;
    }
    ui.strong("This week");
    egui::Grid::new("week").num_columns(3).spacing([24.0, 4.0]).show(ui, |ui| {
        ui.label("");
        ui.label(egui::RichText::new("this week").weak().small());
        ui.label(egui::RichText::new("last week").weak().small());
        ui.end_row();
        let seconds = |week: &Value, key: &str| spell(week[key].as_f64().unwrap_or(0.0));
        let whole = |week: &Value, key: &str| (week[key].as_f64().unwrap_or(0.0).round() as i64).to_string();
        for (name, key, timed) in [("Productive", "productive", true), ("Distracting", "distracting", true), ("Experience", "xp", false), ("Points", "points", false), ("Caught", "caught", false), ("Goals met", "goals", false), ("Sessions kept", "sessions", false)] {
            ui.label(name);
            // The better of the two weeks in green, where more is better.
            let (now, then) = (this[key].as_f64().unwrap_or(0.0), last[key].as_f64().unwrap_or(0.0));
            let said = if timed { seconds(this, key) } else { whole(this, key) };
            let ahead = if key == "distracting" { now < then } else { now > then };
            ui.label(if ahead && then > 0.0 { egui::RichText::new(said).color(look::GOOD) } else { egui::RichText::new(said) });
            ui.label(egui::RichText::new(if timed { seconds(last, key) } else { whole(last, key) }).weak());
            ui.end_row();
        }
    });
    let mut best: Vec<String> = Vec::new();
    if let Some(day) = bests["day"]["day"].as_str() {
        best.push(format!("best day {} ({day})", spell(bests["day"]["productive"].as_f64().unwrap_or(0.0))));
    }
    if bests["streak"].as_f64().unwrap_or(0.0) >= 60.0 {
        best.push(format!("longest focus {}", spell(bests["streak"].as_f64().unwrap_or(0.0))));
    }
    if let Some(from) = bests["week"]["from"].as_str() {
        best.push(format!("best week {} (from {from})", spell(bests["week"]["productive"].as_f64().unwrap_or(0.0))));
    }
    if !best.is_empty() {
        ui.label(egui::RichText::new(format!("Your bests: {}.", best.join(", "))).weak().small());
    }
}

/// Every species, with discoveries in colour and unseen entries as silhouettes.
fn dex(ui: &mut egui::Ui, state: &Value, icons: &HashMap<u64, egui::TextureHandle>, browser: &mut pokedex::Browser) {
    let dex = &state["dex"];
    if dex.is_null() {
        ui.label("The server has not said what you have seen yet.");
        return;
    }
    let entries = pokedex::entries(state);
    let (seen, caught, total) = (dex["seen"].as_u64().unwrap_or_else(|| entries.iter().filter(|entry| entry.seen).count() as u64), dex["caught"].as_u64().unwrap_or_else(|| entries.iter().filter(|entry| entry.caught).count() as u64), entries.len().max(1));
    ui.add(egui::ProgressBar::new(caught as f32 / total as f32).desired_height(14.0));
    ui.label(format!("{caught} caught, {seen} seen, of {total}"));
    ui.add_space(6.0);
    let mut changed = std::mem::take(&mut browser.reset_scroll);
    ui.horizontal(|ui| {
        // TextEdit's requested width excludes its frame padding; leave room for that and Clear.
        let width = (ui.available_width() - 64.0).max(50.0);
        changed |= ui.add(egui::TextEdit::singleline(&mut browser.query).desired_width(width).char_limit(80).hint_text("Search name or #number")).changed();
        if !browser.query.is_empty() && ui.small_button("Clear").clicked() { browser.query.clear(); changed = true; }
    });
    ui.horizontal_wrapped(|ui| {
        for (filter, name, hint) in [
            (pokedex::Filter::All, "All", "Every mon"),
            (pokedex::Filter::Caught, "Caught", "Mons you have caught"),
            (pokedex::Filter::Seen, "Seen", "Encountered, but not caught yet"),
            (pokedex::Filter::Unseen, "Unseen", "Mons you have not encountered yet"),
        ] {
            changed |= ui.selectable_value(&mut browser.filter, filter, name).on_hover_text(hint).changed();
        }
    });
    let entries: Vec<_> = entries.iter().filter(|entry| browser.matches(entry)).collect();
    ui.label(egui::RichText::new(format!("{} shown", entries.len())).small().color(look::FAINT));
    ui.add_space(4.0);
    if entries.is_empty() {
        ui.label("No mons match your search and filter.");
        if ui.small_button("Reset search and filters").clicked() { *browser = pokedex::Browser::default(); browser.reset_scroll = true; }
        return;
    }
    let gap = 8.0;
    let columns = ((ui.available_width() + gap) / (100.0 + gap)).floor().max(1.0) as usize;
    let tile_width = (ui.available_width() - gap * (columns - 1) as f32) / columns as f32;
    let mut scroll = egui::ScrollArea::vertical().id_salt("pokedex-grid").auto_shrink([false, false]);
    if changed { scroll = scroll.vertical_scroll_offset(0.0); }
    scroll.show_rows(ui, 116.0, entries.len().div_ceil(columns), |ui, range| {
        let visible = (range.start * columns)..(range.end * columns).min(entries.len());
        let requested: Vec<u64> = entries[visible.clone()].iter().map(|entry| entry.number).collect();
        let changed = ui.ctx().data_mut(|data| {
            let id = egui::Id::unique(DEX_VISIBLE);
            if data.get_temp::<Vec<u64>>(id).as_ref() == Some(&requested) { false }
            else { data.insert_temp(id, requested); true }
        });
        if changed { ui.ctx().request_repaint(); }
        for row in entries[visible].chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for entry in row {
                    let status = if entry.caught { "Caught" } else if entry.seen { "Seen" } else { "Unseen" };
                    ui.allocate_ui_with_layout(egui::vec2(tile_width, 116.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        egui::Frame::new().fill(look::GROUND)
                            .stroke(egui::Stroke::new(1.0, if entry.caught { look::GOOD.gamma_multiply(0.5) } else { look::LINE }))
                            .corner_radius(8).inner_margin(8).show(ui, |ui| {
                                ui.set_width((tile_width - 18.0).max(1.0));
                                ui.set_min_height(98.0);
                                ui.label(egui::RichText::new(format!("#{:04}", entry.number)).small().color(look::FAINT));
                                if let Some(icon) = icons.get(&entry.number) {
                                    ui.add(egui::Image::new((icon.id(), egui::vec2(48.0, 48.0))).tint(if entry.seen { egui::Color32::WHITE } else { egui::Color32::BLACK }));
                                } else { ui.allocate_space(egui::vec2(48.0, 48.0)); }
                                ui.add(egui::Label::new(egui::RichText::new(&entry.name).size(12.5)).truncate());
                                ui.label(egui::RichText::new(status).size(11.0).color(if entry.caught { look::GOOD } else { look::FAINT }));
                            });
                    }).response.on_hover_text(format!("#{:04} · {}\n{status}", entry.number, entry.name));
                }
            });
        }
    });
}

fn lure_banner(ui: &mut egui::Ui, state: &Value) {
    let active=&state["lure"];
    if !active.is_object() { return; }
    let remaining=active["remainingSeconds"].as_f64().unwrap_or(0.0);
    ui.label(egui::RichText::new(format!("{} · {} productive min left",active["name"].as_str().unwrap_or("Lure"),(remaining/60.0).ceil() as u64)).small().color(look::GOOD))
        .on_hover_text("Updates when activity syncs. Idle, paused, neutral, and distracting time do not use the lure.");
    let total=active["totalSeconds"].as_f64().unwrap_or(6000.0).max(1.0);
    ui.add(egui::ProgressBar::new((remaining/total).clamp(0.0,1.0) as f32).desired_height(6.0));
}

fn shop(ui: &mut egui::Ui, state: &Value, _lure: &mut usize, acts: &mut Vec<Act>) {
    let (points, prices) = (state["points"].as_i64().unwrap_or(0), &state["prices"]);
    ui.label(format!("You have {points} points."));
    lure_banner(ui,state);
    ui.add_space(6.0);
    // One line a thing to buy: what it is, how many you have, and what it costs.
    let mut line = |ui: &mut egui::Ui, item: items::Item, name: &str, about: &str, have: String, price: Option<i64>, body: Value| {
        ui.horizontal(|ui| {
            items::show(ui, item);
            let width = (ui.available_width() - 104.0).max(60.0);
            ui.allocate_ui_with_layout(egui::vec2(width, 56.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(width);
                ui.strong(name);
                if !have.is_empty() { ui.label(egui::RichText::new(have).weak().small()); }
                ui.label(egui::RichText::new(about).weak().small());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match price {
                Some(price) => {
                    if ui.add_enabled(points >= price && !(body["item"].as_str().is_some_and(|name| name.ends_with("lure")) && state["lure"].is_object()), egui::Button::new(format!("{price} points")).min_size(egui::vec2(96.0, 28.0))).clicked() {
                        acts.push(Act::Post("/shop/buy", body));
                    }
                }
                None => {
                    ui.label(egui::RichText::new("no more to buy").weak().small());
                }
            });
        });
        ui.add_space(4.0);
    };
    let have = |key: &str| format!("you have {}", state[key].as_i64().unwrap_or(0));
    line(ui, items::Item::Ball, "Poké Ball", "Holds what it holds.", have("balls"), prices["ball"].as_i64().or(state["ballPrice"].as_i64()), json!({ "item": "ball", "count": 1 }));
    line(ui, items::Item::GreatBall, "Great Ball", "Half as sure again as a ball.", have("great"), prices["great"].as_i64(), json!({ "item": "great", "count": 1 }));
    line(ui, items::Item::UltraBall, "Ultra Ball", "Twice as sure as a ball.", have("ultra"), prices["ultra"].as_i64(), json!({ "item": "ultra", "count": 1 }));
    line(ui, items::Item::Slot, "Desk slot", "Room on the desk for one more mon.", format!("the desk holds {}", state["deskCap"]), prices["slot"].as_i64(), json!({ "item": "slot" }));
    ui.separator();
    ui.strong("Lures");
    ui.label(egui::RichText::new("Use a lure to attract wild mons. Time advances only while productive.").small().color(look::FAINT));
    ui.label(egui::RichText::new("All tiers boost rarity and shiny odds. One active at a time.").small().color(look::FAINT));
    ui.add_space(4.0);
    for (item, kind, name, minutes) in [(items::Item::Lure,"lure","Lure",100),(items::Item::SuperLure,"super-lure","Super Lure",200),(items::Item::MaxLure,"max-lure","Max Lure",250)] {
        line(ui,item,name,&format!("{minutes} productive minutes"),String::new(),prices[kind].as_i64(),json!({"item":kind}));
    }
}
