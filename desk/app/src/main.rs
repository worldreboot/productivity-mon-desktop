//! Mons that live on the desktop: each in a small see-through window of its own that stays on top and never
//! takes the keyboard, drawn by wgpu from a baked HOME model (desk/bake).
//!
//!   desk-mons [--server URL] [--guest yes] [--models DIR] [--mon-size SHARE] [--act live|still|restless] [--seconds N]
//!   desk-mons --mons DIR,DIR,... [--act ...] [--seconds N]
//!   desk-mons --snap FILE --model DIR [--clip idle|walk|run|roar] [--yaw DEGREES] [--time SECONDS] [--size N]
//!
//! The first is the app: it shows the mons on the desk of the account it is let in to on the server
//! (desk/PROTOCOL.md; http://localhost:2590 unless `--server` or DESK_SERVER says), and reports the time spent in
//! productive programs. The first time it opens the server's page to be let in, or with `--guest yes` makes a new
//! account itself. The second shows the named models with no server. Drag a mon to carry it, click it to pet it,
//! click a wild one to throw a ball. The icon by the clock has the rest: the page, hiding the mons, starting
//! with Windows, quitting. The third draws one picture with no window, for tests and comparisons.

#![cfg_attr(all(target_os="windows",not(debug_assertions)), windows_subsystem = "windows")]

mod actor;
mod variants;
mod activity_icons;
mod app_icon;
mod anim;
mod bake;
mod board;
mod desk;
mod label;
mod look;
mod mon;
mod net;
mod panel;
mod render;
mod sound;
mod toss;
mod track;
mod tray;
mod wild;
mod updates;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId, WindowLevel};

use actor::{Actor, State};
use anim::Pose;
use desk::Surface;
use mon::Mon;
use render::{Gpu, Item, Model, Target, FEET};
use track::Tracker;
use wild::{Done, Outcome, Phase, Wild};

/// Frames a second while any mon walks, falls or is carried, and while all of them only stand and breathe.
const MOVING_FPS: f64 = 30.0;
/// A stable public redirect lets community invites change without rebuilding the app.
const COMMUNITY_URL: &str = "https://productivitymon.com/discord";
const STANDING_FPS: f64 = 15.0;
/// How long nobody has to touch the keyboard or mouse for the mons to stop being drawn, in seconds.
const ASLEEP_AFTER: f32 = 180.0;
/// How often it is asked whether anyone is there and whether something has the whole screen, in seconds.
const LOOK_EVERY: f32 = 2.0;
/// The least time between two readings of the desktop's windows, in seconds: a window being dragged says so constantly.
const SURFACES_EVERY: f32 = 0.05;
/// How often the time gone by is put to the program in front, and how often what has gathered is reported, in seconds.
const TRACK_EVERY: f32 = 2.0;
const REPORT_EVERY: f64 = 60.0;
/// How far the mouse must go with its button down to carry a mon rather than pet it, in pixels.
const CARRY_FROM: i32 = 5;

struct Options {
    server: String,
    guest: bool,
    models: PathBuf,
    /// How big the mons are drawn (`MON_SIZE`).
    mon_size: f32,
    /// Where the species' small pictures are (`<Pokédex number>.png`), if they are anywhere.
    icons: PathBuf,
    mons: Vec<PathBuf>,
    act: String,
    seconds: Option<f64>,
    /// For tests: something picked from the icon's menu three seconds in, as if by hand (hide, quit).
    pick: String,
    /// For tests: a change asked of the server four seconds in, as the panel would: `<path> <json body>`.
    ask: String,
    /// Seconds between reports, where the usual minute is too long to wait for.
    report_every: f64,
    /// For a picture: what its plate says, as `name|species under it|level|share of the level`, parts left empty
    /// left out.
    plate: String,
    /// For a picture: words floating over the mon, `--time` seconds into their float.
    drop: String,
    snap: Option<PathBuf>,
    model: PathBuf,
    clip: String,
    yaw: f32,
    time: f32,
    size: u32,
}

fn options() -> Options {
    // Where the server is: said when the app is started, or in `server.txt` beside the program (an installed
    // copy has one), or this PC itself.
    let written = std::env::current_exe().ok().and_then(|exe| [exe.with_file_name("server.txt"),exe.parent()?.join("../Resources/server.txt")].into_iter().find_map(|path|std::fs::read_to_string(path).ok())).map(|said| said.trim().trim_end_matches('/').to_string()).filter(|said| said.starts_with("http"));
    let server = std::env::var("DESK_SERVER").ok().or(written).unwrap_or_else(|| "http://localhost:2590".into());
    let mut o = Options { server, guest: false, models: "models".into(), mon_size: MON_SIZE, icons: std::env::var_os("HOME_ICONS_DIR").map(PathBuf::from).unwrap_or_default(), mons: Vec::new(), act: "live".into(), seconds: None, pick: String::new(), ask: String::new(), report_every: REPORT_EVERY, plate: String::new(), drop: String::new(), snap: None, model: PathBuf::new(), clip: "idle".into(), yaw: 0.0, time: 0.0, size: 256 };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i + 1 < args.len() {
        let v = &args[i + 1];
        match args[i].as_str() {
            "--server" => o.server = v.trim_end_matches('/').to_string(),
            "--guest" => o.guest = v == "yes",
            "--models" => o.models = v.into(),
            "--icons" => o.icons = v.into(),
            "--mon-size" => o.mon_size = v.parse::<f32>().expect("--mon-size SHARE").clamp(0.2, 2.0),
            "--mons" => o.mons = v.split(',').map(PathBuf::from).collect(),
            "--act" => o.act = v.clone(),
            "--seconds" => o.seconds = Some(v.parse().expect("--seconds N")),
            "--pick" => o.pick = v.clone(),
            "--ask" => o.ask = v.clone(),
            "--report-every" => o.report_every = v.parse().expect("--report-every SECONDS"),
            "--plate" => o.plate = v.clone(),
            "--drop" => o.drop = v.clone(),
            "--snap" => o.snap = Some(v.into()),
            "--model" => o.model = v.into(),
            "--clip" => o.clip = v.clone(),
            "--yaw" => o.yaw = v.parse::<f32>().expect("--yaw DEGREES").to_radians(),
            "--time" => o.time = v.parse().expect("--time SECONDS"),
            "--size" => o.size = v.parse().expect("--size N"),
            other => panic!("unknown option {other}"),
        }
        i += 2;
    }
    // Started by Windows, the app is not in its own folder: the models are looked for beside the program too.
    if !o.models.exists() {
        let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|folder| folder.to_path_buf()));
        if let Some(found) = beside.iter().flat_map(|folder| [folder.join("models"), folder.join("../Resources/models"), folder.join("../../models")]).find(|folder| folder.exists()) {
            o.models = found;
        }
    }
    // The pictures are the user's own copy of the HOME sprites, kept beside the repository.
    if o.icons.as_os_str().is_empty() {
        o.icons = o.models.join("../../../../PokemonHOMEIcons/sprites/pokemon/other/home");
        // Installed, they are small copies in a folder beside the models.
        if !o.icons.exists() && o.models.join("../icons").exists() {
            o.icons = o.models.join("../icons");
        }
    }
    o
}

/// Draws one picture of a mon into a PNG with no window: what tests and comparisons look at, since a picture
/// taken of the screen has been through the desktop's own colour handling.
fn snap(options: &Options, path: &std::path::Path) {
    let gpu = Gpu::headless();
    let mon = Rc::new(Mon::read(&options.model).unwrap_or_else(|error| panic!("{error}")));
    let mut pose = Pose::new(&mon);
    let mut model = gpu.model(mon.clone());
    pose.rest(&mon);
    // `--clip ball:taking` and the like: a wild mon part way through a throw, `--time` seconds into that part.
    let wild = options.clip.strip_prefix("ball:").map(|part| {
        let mut wild = Wild::new(String::new());
        wild.phase = match part {
            "flying" => Phase::Flying(options.time),
            "taking" => Phase::Taking(options.time),
            "dropping" => Phase::Dropping(options.time),
            "shaking" => Phase::Shaking { done: 0, time: options.time },
            "bursting" => Phase::Bursting(options.time),
            "kept" => Phase::Kept(options.time),
            _ => Phase::About,
        };
        wild
    });
    for (name, time) in [("idle", options.time), ("eye", options.time % 4.0), (options.clip.as_str(), options.time)] {
        if let Some(clip) = mon.clip(name) {
            pose.apply(&mon, &mon.header.clips[clip], time, 1.0);
        }
    }
    pose.finish(&mon);
    let camera = model.camera();
    let look = wild.map(|wild| wild.look(camera.0, mon.header.bounds.max[1], options.time));
    let heart = glam::Vec3::new(0.0, mon.header.bounds.max[1] * 0.5, 0.0);
    let (scale, flash) = look.as_ref().map_or((1.0, 0.0), |look| (look.scale, look.flash));
    let place = glam::Mat4::from_rotation_y(options.yaw) * glam::Mat4::from_translation(heart) * glam::Mat4::from_scale(glam::Vec3::splat(scale)) * glam::Mat4::from_translation(-heart);
    let ball = Rc::new(Mon::ball());
    let (mut ball_model, still) = (gpu.model(ball.clone()), Pose::new(&ball));
    let mut scene = vec![Item { model: &mut model, pose: &pose, place, flash, opacity: 1.0 }];
    if let Some((at, rock, radius)) = look.and_then(|look| look.ball) {
        let place = glam::Mat4::from_translation(at) * glam::Mat4::from_rotation_z(rock) * glam::Mat4::from_scale(glam::Vec3::splat(radius));
        scene.push(Item { model: &mut ball_model, pose: &still, place, flash: 0.0, opacity: 1.0 });
    }
    // As a window is: a strip over the square, and in it the plate, if one was asked for.
    let strip = STRIP.round() as u32;
    let units = 2.0 * camera.0 / options.size as f32;
    let parts: Vec<&str> = options.plate.split('|').collect();
    let part = |index: usize| parts.get(index).copied().filter(|part| !part.is_empty());
    let says = part(0).map(|name| label::Says { name: name.to_string(), under: part(1).map(str::to_string), level: part(2).and_then(|v| v.parse().ok()), xp: part(3).and_then(|v| v.parse().ok()), warm: part(4).is_some() });
    let plate = says.zip(label::Fonts::load()).map(|(says, fonts)| {
        let picture = label::draw(&fonts, &says, options.size, 1.0);
        let (width, height) = (picture.width as f32, picture.height as f32);
        let mon = Rc::new(Mon::plate(picture.pixels, picture.width, picture.height));
        (gpu.model(mon.clone()), Pose::new(&mon), width, height)
    });
    let mut plate = plate;
    let top = anim::height(&mon, &pose);
    if let Some((model, pose, width, height)) = &mut plate {
        let front = camera.0 * 0.95;
        let place = glam::Mat4::from_translation(glam::Vec3::new(0.0, top + camera.0 * 0.06 + front * render::PITCH.tan(), front)) * glam::Mat4::from_scale(glam::Vec3::new(*width * units, *height * units, 1.0));
        scene.push(Item { model, pose, place, flash: 0.0, opacity: 1.0 });
    }
    let mut floating = (!options.drop.is_empty()).then(label::Fonts::load).flatten().map(|fonts| {
        let picture = label::floating(&fonts, &options.drop, [255, 214, 92], 1.0);
        let (width, height) = (picture.width as f32, picture.height as f32);
        let mon = Rc::new(Mon::plate(picture.pixels, picture.width, picture.height));
        (gpu.model(mon.clone()), Pose::new(&mon), width, height)
    });
    if let Some((model, pose, width, height)) = &mut floating {
        let along = (options.time / DROP_LASTS).clamp(0.0, 0.999);
        let front = camera.0 * 0.97;
        let rise = strip as f32 * units * DROP_RISES * (1.0 - (1.0 - along).powi(2));
        let place = glam::Mat4::from_translation(glam::Vec3::new(0.0, top + camera.0 * 0.04 + rise + front * render::PITCH.tan(), front)) * glam::Mat4::from_scale(glam::Vec3::new(*width * units, *height * units, 1.0));
        scene.push(Item { model, pose, place, flash: 0.0, opacity: ((1.0 - along) * 2.5).min(1.0) });
    }
    let pixels = gpu.snap((camera.0, camera.1, strip as f32 * units, 1.0), &mut scene, options.size, options.size + strip);
    let file = std::io::BufWriter::new(std::fs::File::create(path).expect("the picture's file"));
    let mut encoder = png::Encoder::new(file, options.size, options.size + strip);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().and_then(|mut writer| writer.write_image_data(&pixels)).expect("the picture written");
}

/// What wakes the app from outside its own windows: something picked from the icon's menu.
enum Poke {
    Menu(tray_icon::menu::MenuId),
    /// The icon itself was clicked.
    Icon,
    /// The server thread has something to tell.
    Told,
    /// A species' model has been baked, by its Pokédex number, or could not be and why.
    Baked(u64, Result<(), String>),
}

/// How big the mons are drawn, as a share of the size they were first given (a Pikachu about 170 pixels square):
/// they live at the edge of the screen and should not take up much of it. `--mon-size` says otherwise.
const MON_SIZE: f32 = 0.55;
/// The least width of a mon's window, for its plate, in pixels at 96 dots an inch.
const PLATE_WIDE: f32 = 150.0;
/// How long a mon's plate stays up once it has something to say or the mouse has left it, and how long it takes
/// to fade away, in seconds.
const PLATE_UP: f32 = 5.0;
const PLATE_FADES: f32 = 0.6;
/// How long words float over a mon when it earns something, and how far up they go in that time, as a share of the
/// strip over it, in seconds; and how long after one lot the next waits, when two are earned at once.
const DROP_LASTS: f32 = 1.9;
const DROP_RISES: f32 = 0.9;
const DROP_APART: f32 = 0.45;
/// How often the mons say how they find what you are doing, in seconds.
const MOOD_EVERY: f32 = 45.0;
/// How tall the strip over a mon's own square is, for its plate, in pixels at 96 dots an inch.
const STRIP: f32 = 46.0;

/// The plate over a mon's head: what it says, and the picture of that, with its size in pixels.
struct Plate {
    says: label::Says,
    model: Model,
    pose: Pose,
    width: f32,
    height: f32,
}

/// The ball that lies by a wild mon to be flung at it, in a small window of its own.
struct Flung {
    toss: toss::Toss,
    target: Target,
    hwnd: isize,
    model: Model,
    pose: Pose,
    /// Its window's side, in pixels.
    size: u32,
    placed: (i32, i32),
    /// Whether it has been put down yet: it is once the mon it is for has landed.
    down: bool,
}

impl Flung {
    fn place(&mut self) {
        let at = ((self.toss.x - self.size as f32 / 2.0).round() as i32, (self.toss.y - self.size as f32 / 2.0).round() as i32);
        if at != self.placed {
            self.placed = at;
            desk::move_to(self.hwnd, at.0, at.1);
        }
    }

    fn draw(&mut self, gpu: &Gpu) {
        // A ball of one unit, seen whole with a little room round it.
        let place = glam::Mat4::from_rotation_z(-self.toss.spin);
        gpu.draw(&self.target, (1.3, 0.0, 0.0, 1.0), &mut [Item { model: &mut self.model, pose: &self.pose, place, flash: 0.0, opacity: 1.0 }]);
    }
}

/// Words floating up over a mon: experience earned, a level gained.
struct Drop {
    model: Model,
    pose: Pose,
    width: f32,
    height: f32,
    /// When it starts, by the mons' clock: after the one before it, if there is one.
    from: f32,
    /// On the taskbar's strip: how far across from the strip's middle it floats, in pixels.
    across: f32,
}

/// How much room there is over the taskbar for what floats up from the strip, and the gap between two mons'
/// plates on it, in pixels at 96 dots an inch.
const STRIP_ROOM: f32 = 46.0;
const STRIP_GAP: f32 = 6.0;

/// The strip on the taskbar by the clock: every desk mon's plate side by side, always up, and what they earn
/// floating up out of it. A window over the taskbar that the mouse goes straight through.
struct Strip {
    target: Target,
    hwnd: isize,
    /// What the plates say and whose they are, the picture of them, and where each one's middle is across it.
    says: Vec<(String, label::Says)>,
    model: Model,
    pose: Pose,
    middles: Vec<f32>,
    /// The picture's size and the window's, in pixels, and the taskbar's height.
    picture: (f32, f32),
    size: (u32, u32),
    taskbar: f32,
    drops: Vec<Drop>,
    /// Whether it has changed since it was last drawn.
    stale: bool,
    placed: (i32, i32),
}

impl Strip {
    /// Where on the screen it goes: against the clock's corner, its foot on the taskbar's foot. Moved there and
    /// put over the taskbar, which takes the top back whenever it is used.
    fn place(&mut self) {
        let Some((bar, corner)) = desk::clock_corner() else { return };
        let at = (corner[0] - self.size.0 as i32 - 8, bar[3] - self.size.1 as i32);
        self.placed = at;
        desk::put_on_top(self.hwnd, at.0, at.1);
    }

    fn draw(&mut self, gpu: &Gpu, clock: f32) {
        // A camera that shows the window pixel for pixel: across from its middle, up from its foot.
        let (wide, tall) = (self.size.0 as f32, self.size.1 as f32);
        let camera = (wide / 2.0, wide / 2.0, tall - wide, 1.0);
        let foot = (self.taskbar - self.picture.1) / 2.0;
        let place = glam::Mat4::from_translation(glam::Vec3::new(0.0, foot.max(0.0), 0.0)) * glam::Mat4::from_scale(glam::Vec3::new(self.picture.0, self.picture.1, 1.0));
        let mut scene = vec![Item { model: &mut self.model, pose: &self.pose, place, flash: 0.0, opacity: 1.0 }];
        for drop in &mut self.drops {
            let along = (clock - drop.from) / DROP_LASTS;
            if !(0.0..1.0).contains(&along) {
                continue;
            }
            let rise = (tall - self.taskbar * 0.55 - drop.height) * (1.0 - (1.0 - along).powi(2));
            let place = glam::Mat4::from_translation(glam::Vec3::new(drop.across, self.taskbar * 0.55 + rise, 1.0)) * glam::Mat4::from_scale(glam::Vec3::new(drop.width, drop.height, 1.0));
            scene.push(Item { model: &mut drop.model, pose: &drop.pose, place, flash: 0.0, opacity: ((1.0 - along) * 2.5).min(1.0) });
        }
        gpu.draw(&self.target, camera, &mut scene);
        self.stale = false;
    }
}

/// A mouse button held down on a mon: where the mouse was, and where on the mon it has hold.
struct Press {
    from: (i32, i32),
    grip: (f32, f32),
}

/// A mon in its window.
struct Shown {
    /// Which of the account's mons it is, and its Pokédex number; neither for one shown with no server.
    id: String,
    dex: u64,
    actor: Actor,
    target: Target,
    model: Model,
    pose: Pose,
    /// Its square's side, the strip over it for its plate, both in pixels; and the screen's scale.
    size: u32,
    /// Its window's width, which is its square's or a plate's, whichever is more.
    width: u32,
    strip: u32,
    scale: f32,
    plate: Option<Plate>,
    /// When its plate is to be gone, by the mons' clock, and whether the mouse is over it (which keeps it up).
    plate_until: f32,
    over: bool,
    drops: Vec<Drop>,
    /// How high the top of it is as it stands idle, in its own units: where its plate is held over.
    top: f32,
    /// Its clips, where it has them, and the one it is playing once through, with how far into it it is.
    idle: Option<usize>,
    walk: Option<usize>,
    eye: Option<usize>,
    once: Option<(usize, f32)>,
    /// How far it has turned from what its owner is doing, 0 to 1: it fades as it does.
    dim: f32,
    /// When in the blink's four-odd seconds this one blinks.
    blink: f32,
    press: Option<Press>,
    /// For a wild one: the encounter it is, and the ball that is thrown at it.
    wild: Option<(Wild, Model, Pose)>,
    /// And the ball that lies by it to be flung at it by hand.
    flung: Option<Flung>,
    /// Its window, as Windows knows it, and where it was last put.
    hwnd: isize,
    placed: (i32, i32),
}

impl Shown {
    /// The middle of the mon on the screen and how near a flung ball must come to strike it, in pixels.
    fn mark(&self) -> (f32, f32, f32) {
        let tall = self.top * self.size as f32 / (2.0 * self.model.camera().0);
        (self.actor.x, self.actor.y - tall / 2.0, (tall / 2.0).max(self.size as f32 * 0.22) + 10.0 * self.scale)
    }

    /// Puts the ball down beside the mon, on the side toward the middle of what it stands on, and shows it.
    fn put_ball_down(&mut self, surfaces: &[Surface]) {
        let Some(flung) = &mut self.flung else { return };
        let on = surfaces.iter().find(|s| (s.y as f32 - self.actor.y).abs() < 2.0 && self.actor.x >= s.x0 as f32 && self.actor.x <= s.x1 as f32);
        let middle = on.map_or(self.actor.x, |s| (s.x0 + s.x1) as f32 / 2.0);
        let side = if self.actor.x > middle { -1.0 } else { 1.0 };
        let away = self.width as f32 * 0.6 + 90.0 * self.scale;
        let mut x = self.actor.x + side * away;
        if let Some(on) = on {
            x = x.clamp(on.x0 as f32 + flung.size as f32, on.x1 as f32 - flung.size as f32);
        }
        flung.toss.rest_at(x, self.actor.y - flung.size as f32 / 2.0);
        flung.down = true;
        flung.place();
        flung.target.window.set_visible(true);
    }

    fn place(&mut self) {
        let at = ((self.actor.x - self.width as f32 / 2.0).round() as i32, (self.actor.y - self.strip as f32 - self.size as f32 * FEET).round() as i32);
        if at != self.placed {
            self.placed = at;
            desk::move_to(self.hwnd, at.0, at.1);
        }
    }

    fn draw(&mut self, gpu: &Gpu, clock: f32) {
        let mon = self.model.mon.clone();
        let clips = &mon.header.clips;
        self.pose.rest(&mon);
        // The idle under everything, a blink over it, then its walk as far as it is into one, then what it does once.
        if let Some(idle) = self.idle {
            self.pose.apply(&mon, &clips[idle], clock, 1.0);
        }
        if let Some(eye) = self.eye {
            self.pose.apply(&mon, &clips[eye], (clock + self.blink) % 4.3, 1.0);
        }
        if let Some(walk) = self.walk {
            self.pose.apply(&mon, &clips[walk], self.actor.stride, self.actor.gait);
        }
        if let Some((clip, time)) = self.once {
            let clip = &clips[clip];
            self.pose.apply(&mon, clip, time, (time / 0.15).min((clip.duration - time) / 0.15).clamp(0.0, 1.0));
        }
        self.pose.finish(&mon);
        let camera = self.model.camera();
        let tall = mon.header.bounds.max[1];
        let look = self.wild.as_ref().map(|(wild, ..)| wild.look(camera.0, tall, clock));
        let (wild, busy) = (self.wild.is_some(), self.wild.as_ref().is_some_and(|(wild, ..)| wild.busy()));
        let (scale, flash) = look.as_ref().map_or((1.0, 0.0), |look| (look.scale, look.flash));
        // Into the ball it goes toward its own middle.
        let heart = glam::Vec3::new(0.0, tall * 0.5, 0.0);
        let place = glam::Mat4::from_rotation_y(self.actor.yaw) * glam::Mat4::from_translation(heart) * glam::Mat4::from_scale(glam::Vec3::splat(scale)) * glam::Mat4::from_translation(-heart);
        let mut scene = vec![Item { model: &mut self.model, pose: &self.pose, place, flash, opacity: 1.0 - 0.62 * self.dim }];
        if let (Some((_, ball, still)), Some((at, rock, radius))) = (&mut self.wild, look.and_then(|look| look.ball)) {
            let place = glam::Mat4::from_translation(at) * glam::Mat4::from_rotation_z(rock) * glam::Mat4::from_scale(glam::Vec3::splat(radius));
            scene.push(Item { model: ball, pose: still, place, flash: 0.0, opacity: 1.0 });
        }
        // Its plate, held over its head (over the ball, for a wild one), and put away while a ball is out at it.
        let units = 2.0 * camera.0 / self.size as f32;
        // Up while it has news, while the mouse is on it, and always for a wild one, whose bar is worth watching;
        // then it fades and the mon is left alone on the desktop.
        let there = if wild || self.over { 1.0 } else { ((self.plate_until - clock) / PLATE_FADES).clamp(0.0, 1.0) };
        if let (Some(plate), false, true) = (&mut self.plate, busy, there > 0.0) {
            let lift = if wild { (tall * 1.06 + camera.0 * 0.18).max(self.top + camera.0 * 0.05) } else { self.top + camera.0 * 0.06 };
            // Held toward the camera, clear of ears and horns; the camera looks down, which would lower it on the
            // screen, so it is raised by as much.
            let front = camera.0 * 0.95;
            let place = glam::Mat4::from_translation(glam::Vec3::new(0.0, lift + front * render::PITCH.tan(), front)) * glam::Mat4::from_scale(glam::Vec3::new(plate.width * units, plate.height * units, 1.0));
            scene.push(Item { model: &mut plate.model, pose: &plate.pose, place, flash: 0.0, opacity: there });
        }
        // What it has just earned, floating up from over its head and fading as it goes.
        for drop in &mut self.drops {
            let along = (clock - drop.from) / DROP_LASTS;
            if !(0.0..1.0).contains(&along) {
                continue;
            }
            let front = camera.0 * 0.97;
            let rise = self.strip as f32 * units * DROP_RISES * (1.0 - (1.0 - along).powi(2));
            let place = glam::Mat4::from_translation(glam::Vec3::new(0.0, self.top + camera.0 * 0.04 + rise + front * render::PITCH.tan(), front)) * glam::Mat4::from_scale(glam::Vec3::new(drop.width * units, drop.height * units, 1.0));
            scene.push(Item { model: &mut drop.model, pose: &drop.pose, place, flash: 0.0, opacity: ((1.0 - along) * 2.5).min(1.0) });
        }
        gpu.draw(&self.target, (camera.0, camera.1, self.strip as f32 * units, self.width as f32 / self.size as f32), &mut scene);
    }
}

/// Presence never contains browser titles and redacts every paused/excluded app.
fn presence(paused: bool, idle: bool, excluded: bool, program: Option<&str>, class: Option<track::Class>) -> serde_json::Value {
    let redacted = paused || excluded;
    let class = class.map(|class| match class { track::Class::Productive => "productive", track::Class::Neutral => "neutral", track::Class::Distracting => "distracting" });
    serde_json::json!({"paused":paused,"idle":idle,"excluded":excluded,"program":if redacted {None} else {program},"class":if redacted {None} else {class}})
}

struct App {
    options: Options,
    gpu: Option<Gpu>,
    shown: Vec<Shown>,
    surfaces: Vec<Surface>,
    began: Instant,
    last: Instant,
    next: Instant,
    /// Seconds the mons have been awake for: what their idles and blinks are timed by.
    clock: f32,
    /// Seconds since the desktop's windows were read, and since it was asked whether anyone is there.
    since_surfaces: f32,
    since_look: f32,
    /// Nobody at the keyboard, or something has the whole screen.
    asleep: bool,
    hidden: bool,
    drawn: u32,
    spent: Duration,
    tracker: Tracker,
    /// Windows' clock when time was last put to the program in front, and seconds since the last report.
    tracked_at: u32,
    since_report: f64,
    heartbeat_at: Option<Instant>,
    server_seen: Option<Instant>,
    server_away: bool,
    excluded: bool,
    /// Where its files are kept: the lists, and the reports not yet sent.
    home: PathBuf,
    /// The thread that talks to the server, where there is one.
    asks: Option<std::sync::mpsc::Sender<net::Ask>>,
    told: Option<std::sync::mpsc::Receiver<net::Told>>,
    /// How many mons have been put on the desktop, which spreads them along it.
    added: u32,
    started: bool,
    /// The type the plates over the mons are set in, if Windows has it.
    fonts: Option<label::Fonts>,
    /// The icon by the clock, what wakes the app when its menu is used, and whether the mons are hidden from it.
    tray: Option<tray::Tray>,
    proxy: EventLoopProxy<Poke>,
    paused: bool,
    /// Tracking switched off from the icon's menu: nothing is counted or reported, and no title is read.
    untracked: bool,
    /// Whether the mons' names, levels and what they earn are shown on the taskbar by the clock, and the strip
    /// that shows them while it is up.
    on_taskbar: bool,
    /// Whether the friends' day floats in a small window of its own, the window while it does, where it was
    /// last left, and since when that has not been written down.
    on_board: bool,
    board: Option<board::Board>,
    board_at: Option<(i32, i32)>,
    board_moved: Option<Instant>,
    strip: Option<Strip>,
    /// Programs (by file name) and sites (`site:<name>`) that are never reported: in front, they are no program.
    private: std::collections::BTreeSet<String>,
    /// What the server last said of the friends, their day and the trades.
    social: serde_json::Value,
    /// The collections of the friends asked about, by their account ids.
    theirs: std::collections::HashMap<String, serde_json::Value>,
    /// The tables of the hiscores asked for so far, by their paths.
    ranks: std::collections::HashMap<String, serde_json::Value>,
    /// Why the last report that earned anything earned what it did: the server's `earning`, with what one mon
    /// gained (`xp`) and the program most of the time was in (`in`); and that program, as the last report named it.
    earning: serde_json::Value,
    reported_in: Option<String>,
    /// A mon the server says was just caught, not yet welcomed: its ball on the desktop has not closed for good yet.
    newly: Option<serde_json::Value>,
    welcomed: bool,
    /// Seconds nobody was at the keyboard or mouse since the last report: a break is made of them.
    idle: f64,
    /// How long what is in front has been distracting, in seconds, and how far the mons have turned from it
    /// (0 to 1): they fade and stop where they are while it goes on, and come back when it ends.
    distracted_for: f32,
    drift: f32,
    /// Whether the app makes its sounds.
    sounds: bool,
    /// The last ended focus session that has been said (none before the server has first been heard), and when
    /// the one now going ends, for the server to be asked what came of it.
    session_said: Option<String>,
    session_due: Option<Instant>,
    session_check: Option<Instant>,
    /// Whether a mon has said that a break is due, since it last became so.
    break_said: bool,
    /// When two mons standing together next notice each other, by the app's clock.
    notice_at: f32,
    /// When the mons next say how they find what you are doing, by their clock.
    mood_at: f32,
    points: i64,
    /// Great and ultra balls the account has.
    better: (i64, i64),
    /// The app's own window, while it is open; what the server last said the account has; whether there is an
    /// account; and the last thing there was to say.
    panel: Option<panel::Panel>,
    state: serde_json::Value,
    account: bool,
    account_changing: bool,
    updater: updates::Updater,
    ending: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    /// Some(true) restarts after updating; Some(false) updates on quit without reopening.
    exit_mode: Option<bool>,
    note: String,
    /// Balls the account has to throw, as the server last said, and the ball's own model.
    balls: i64,
    ball: Rc<Mon>,
    /// Whether the lists of programs have been read, from the server or from the last time it gave them.
    listed: bool,
    /// What bakes a species' model when it is first needed, where the app can; the species being baked now, and
    /// those that cannot be, by their Pokédex numbers.
    baker: Option<Arc<bake::Baker>>,
    baking: std::collections::HashSet<u64>,
    unbaked: std::collections::HashSet<u64>,
}

impl App {
    fn begin_exit(&mut self, restart: bool) {
        if self.exit_mode.is_some() || self.account_changing { return; }
        if restart && !self.updater.ready() { return; }
        self.track();
        self.keep_settings();
        let (reply,saved)=std::sync::mpsc::channel();
        let report=self.take_report();
        if let Some(asks)=&self.asks {
            // This follows earlier reports in the same FIFO queue; acknowledgment requires durable local storage.
            if let Err(error)=asks.send(net::Ask::Flush(report,reply)) {
                if let net::Ask::Flush(Some((owner,report)),_)=error.0 {
                    if let Err(error)=net::keep_report(&self.home,&owner,&report) {self.note=format!("Could not save activity: {error}");return;}
                }
                self.note="The connection thread stopped. Activity was saved; please try closing again.".into();return;
            }
        } else {
            let result=report.map_or(Ok(()),|(owner,report)|net::keep_report(&self.home,&owner,&report)).map_err(|e|e.to_string());
            let _=reply.send(result);
        }
        self.ending=Some(saved);
        self.exit_mode=Some(restart);
        self.note="Saving activity…".into();self.next=Instant::now();
        if let Some(panel)=&self.panel {panel.window.request_redraw();}
    }

    fn poll_exit(&mut self,event_loop:&ActiveEventLoop) {
        let (changed,exit,restart_pending)=self.updater.poll();
        if restart_pending {self.begin_exit(true);}
        if changed {
            if let Some(panel)=&self.panel {panel.window.request_redraw();}
            if let Some(tray)=&self.tray {
                let status=&self.updater.status;
                tray.update.set_text(match status.phase {"ready"=>"Update ready — restart", "checking"=>"Checking for updates…", "downloading"=>"Downloading update…", "installing"=>"Installing update…",_=>"Check for updates"});
                tray.update.set_enabled(!matches!(status.phase,"unavailable"|"checking"|"downloading"|"installing"));
            }
            if self.updater.status.phase=="error" && self.exit_mode.is_some() && self.ending.is_none() {
                self.exit_mode=None;self.note=self.updater.status.message.clone();self.tracked_at=desk::ticks();
            }
        }
        if exit {if let Some(asks)=&self.asks {let _=asks.send(net::Ask::Quit);}event_loop.exit();return;}
        let result=self.ending.as_ref().and_then(|saved|match saved.try_recv(){
            Ok(result)=>Some(result),Err(std::sync::mpsc::TryRecvError::Empty)=>None,
            Err(_)=>Some(Err("Could not save activity before closing. Please try again.".into()))});
        if let Some(result)=result {
            self.ending=None;
            if let Some(panel)=&self.panel {panel.window.request_redraw();}
            match result {
                Err(error)=>{self.exit_mode=None;self.note=error;self.tracked_at=desk::ticks();},
                Ok(())=>{
                    if self.updater.ready() {self.updater.install(self.exit_mode==Some(true));}
                    else {if let Some(asks)=&self.asks {let _=asks.send(net::Ask::Quit);}event_loop.exit();}
                }
            }
        }
    }

    fn take_report(&mut self) -> Option<(String,String)> {
        if !self.tracker.any() && self.idle < 1.0 {return None;}
        let owner=self.state["account"]["id"].as_str().map(str::to_string)
            .or_else(||std::fs::read_to_string(self.home.join("account-id")).ok().map(|id|id.trim().to_string()))?;
        uuid::Uuid::parse_str(&owner).ok()?;
        let mut report=self.tracker.report();
        report["idle"]=((self.idle*10.0).round()/10.0).into();
        if report["seconds"]["productive"].as_f64().unwrap_or(0.0)>0.0 {self.reported_in=report["in"].as_str().map(str::to_string);}
        self.idle=0.0;
        report["hour"]=desk::local_hour().into();report["day"]=desk::local_day().into();
        Some((owner,report.to_string()))
    }

    fn forget_account(&mut self) {
        self.account=false;self.state=serde_json::Value::Null;self.tracker=Tracker::default();self.listed=false;
        self.shown.clear();self.board=None;self.strip=None;self.newly=None;self.session_said=None;self.session_due=None;self.session_check=None;
        self.social=serde_json::Value::Null;self.theirs.clear();self.ranks.clear();self.earning=serde_json::Value::Null;self.reported_in=None;
        self.points=0;self.balls=0;self.better=(0,0);self.idle=0.0;self.since_report=0.0;self.excluded=false;
        self.heartbeat_at=None;self.server_seen=None;self.server_away=false;self.distracted_for=0.0;self.drift=0.0;self.break_said=false;
        self.tracked_at=desk::ticks();
        if let Some(panel)=&mut self.panel {panel.reset_account();}
    }

    fn send_presence(&mut self) {
        if !self.account || self.heartbeat_at.is_some_and(|at|at.elapsed() < Duration::from_secs(15)) { return; }
        if let Some(asks) = &self.asks {
            let status = presence(self.untracked, !self.untracked && desk::idle_seconds() >= track::AWAY_AFTER, self.excluded,
                self.tracker.program_name(), self.tracker.now().and_then(|(_,class)|class));
            let _ = asks.send(net::Ask::Status(status));
            self.heartbeat_at = Some(Instant::now());
        }
    }

    /// Puts the time since it last did to the program that was in front for it, and reports once a minute.
    fn track(&mut self) {
        let now = desk::ticks();
        let passed = now.wrapping_sub(self.tracked_at);
        if (passed as f32) < TRACK_EVERY * 1000.0 {
            return;
        }
        // Before the lists are known every program would count as one in no list.
        if !self.listed || self.untracked || !self.account {
            self.tracked_at = now;
            return;
        }
        let untouched = desk::idle_seconds();
        match desk::front_changed() {
            // Another program came to the front part way through: the time before it is the last one's.
            Some(at) => {
                let before = at.wrapping_sub(self.tracked_at).min(passed);
                self.tracker.pass(before as f64 / 1000.0, untouched + (passed - before) as f32 / 1000.0);
                // A program the user keeps to themself is, in front, no program at all: not timed, not named.
                let front = desk::front_program();
                let program = front.clone().filter(|path| !self.private.contains(&track::program(path)));
                self.excluded = front.is_some() && program.is_none();
                // A title is read only where it is a page: of a browser, and only where the server can say what a page is.
                let title = (self.state["sorter"].as_bool().unwrap_or(false) && program.as_deref().is_some_and(|path| track::page(&track::program(path), "-").is_some())).then(desk::front_title).flatten();
                // Nor is a site they keep to themself: its page's title goes nowhere.
                let kept = program.as_deref().zip(title.as_deref()).and_then(|(path, title)| track::page(&track::program(path), title)).and_then(|page| track::site_of(&page)).is_some_and(|site| self.private.contains(&format!("site:{site}")));
                self.excluded |= kept;
                let (program, title) = if kept { (None, None) } else { (program, title) };
                if let (Some(page), Some(asks)) = (self.tracker.front_page(program.as_deref(), title.as_deref()), &self.asks) {
                    let _ = asks.send(net::Ask::Page(page));
                }
                self.tracker.pass((passed - before) as f64 / 1000.0, untouched);
                if let Some(panel) = &self.panel {
                    panel.window.request_redraw();
                }
            }
            None => self.tracker.pass(passed as f64 / 1000.0, untouched),
        }
        self.tracked_at = now;
        // The part of it they were away for: a break, if they have earned one.
        self.idle += ((untouched - track::AWAY_AFTER).max(0.0) as f64).min(passed as f64 / 1000.0);
        self.since_report += passed as f64 / 1000.0;
        if self.since_report >= self.options.report_every {
            self.since_report = 0.0;
            if let Some((owner,report))=self.take_report() {
                if let Some(asks) = &self.asks {
                    let _ = asks.send(net::Ask::Report(owner,report));
                }
            }
        }
    }

    /// Puts a mon on the desktop from a baked model: dropped in from above the floor, or put where `at` says.
    fn add(&mut self, event_loop: &ActiveEventLoop, folder: &std::path::Path, id: String, dex: u64, at: Option<(f32, f32)>, wild: Option<Wild>) {
        let mon = match Mon::read(folder) {
            Ok(mon) => Rc::new(mon),
            Err(error) => {
                eprintln!("[desk] {error}");
                return;
            }
        };
        let scale = event_loop.primary_monitor().map_or(1.0, |m| m.scale_factor()) as f32;
        let floor = self.surfaces.iter().find(|s| s.owner == 0).copied().unwrap_or(Surface { y: 800, x0: 0, x1: 1200, owner: 0, left: 0 });
        // Bigger mons get bigger windows, though not in proportion: a Snorlax to scale would fill the screen.
        let tall = mon.header.bounds.max[1].max(0.1);
        let size = (((110.0 + 90.0 * tall.sqrt()) * self.options.mon_size).clamp(48.0, 420.0) * scale).round() as u32;
        // The window is no narrower than a plate needs, however small the mon under it.
        let width = size.max((PLATE_WIDE * scale).round() as u32);
        let strip = (STRIP * scale).round() as u32;
        let attributes = Window::default_attributes()
            .with_title("desk mon")
            .with_inner_size(PhysicalSize::new(width, size + strip))
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(false)
            
            // No GDI surface behind the swapchain: without this the window has an opaque backing of its own.
            ;
        let window = Arc::new(event_loop.create_window(desk::overlay_attributes(attributes)).expect("a window"));
        let hwnd = desk::native_handle(&window);
        desk::never_active(hwnd);
        let target = match &self.gpu {
            Some(gpu) => gpu.target(window),
            None => {
                let (gpu, target) = Gpu::open(window);
                self.gpu = Some(gpu);
                target
            }
        };
        let model = self.gpu.as_ref().unwrap().model(mon.clone());
        let walk = mon.clip("walk");
        let pace = walk.map_or(0.0, |clip| mon.header.clips[clip].speed) * model.pixels_a_unit(size);
        // Spread along the floor: a quarter, a half, three quarters of the way, then between those.
        self.added += 1;
        let along = [0.25, 0.5, 0.75, 0.375, 0.625, 0.125, 0.875][(self.added as usize - 1) % 7];
        let (x, y) = at.unwrap_or((floor.x0 as f32 + (floor.x1 - floor.x0) as f32 * along, floor.y as f32 - 320.0 * scale));
        let mut actor = Actor::new(x, y, pace, size as f32 * 0.22, 0x9E3779B97F4A7C15u64.wrapping_mul(self.added as u64) ^ self.began.elapsed().as_nanos() as u64);
        actor.restless = match self.options.act.as_str() {
            "still" => Some(false),
            "restless" => Some(true),
            _ => None,
        };
        let wild = wild.map(|wild| {
            let ball = self.gpu.as_ref().unwrap().model(self.ball.clone());
            (wild, ball, Pose::new(&self.ball))
        });
        // A wild one has a ball lying by it, in a window of its own that is not shown until the mon has landed.
        let flung = wild.as_ref().map(|_| {
            let side = (58.0 * scale).round() as u32;
            let attributes = Window::default_attributes()
                .with_title("desk mon ball")
                .with_inner_size(PhysicalSize::new(side, side))
                .with_decorations(false)
                .with_resizable(false)
                .with_transparent(true)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_active(false)
                .with_visible(false)
                
                ;
            let window = Arc::new(event_loop.create_window(desk::overlay_attributes(attributes)).expect("a window"));
            let hwnd = desk::native_handle(&window);
            desk::never_active(hwnd);
            let gpu = self.gpu.as_ref().unwrap();
            Flung { toss: toss::Toss::new(x, y), target: gpu.target(window), hwnd, model: gpu.model(self.ball.clone()), pose: Pose::new(&self.ball), size: side, placed: (i32::MIN, i32::MIN), down: false }
        });
        // Stood idle, to see how tall it stands.
        let mut standing = Pose::new(&mon);
        if let Some(idle) = mon.clip("idle") {
            standing.apply(&mon, &mon.header.clips[idle], 0.0, 1.0);
        }
        standing.finish(&mon);
        let top = anim::height(&mon, &standing);
        let mut shown = Shown {
            id,
            dex,
            top,
            flung,
            wild,
            actor,
            target,
            model,
            pose: Pose::new(&mon),
            size,
            width,
            strip,
            scale,
            plate: None,
            plate_until: 0.0,
            over: false,
            drops: Vec::new(),
            idle: mon.clip("idle"),
            walk,
            eye: mon.clip("eye"),
            once: None,
            dim: 0.0,
            blink: self.added as f32 * 1.7,
            press: None,
            hwnd,
            placed: (i32::MIN, i32::MIN),
        };
        shown.place();
        self.shown.push(shown);
    }

    /// Makes the desktop's mons those the account has on its desk: `mons` is `<Mon>`s, of which those `onDesk` count.
    fn settle(&mut self, event_loop: &ActiveEventLoop, mons: &serde_json::Value) {
        let wanted: Vec<(String, u64, String)> = mons
            .as_array()
            .into_iter()
            .flatten()
            .filter(|mon| mon["onDesk"].as_bool().unwrap_or(false))
            .map(|mon| (mon["id"].as_str().unwrap_or_default().to_string(), variants::of(mon).unwrap_or(0), mon["name"].as_str().unwrap_or("?").to_string()))
            .collect();
        // Those taken off the desk go; one that has become another species goes too, and comes back as that where it stood.
        let mut places = std::collections::HashMap::new();
        self.shown.retain(|shown| {
            let kept = shown.wild.is_some() || wanted.iter().any(|(id, dex, _)| *id == shown.id && *dex == shown.dex);
            if !kept {
                places.insert(shown.id.clone(), (shown.actor.x, shown.actor.y));
            }
            kept
        });
        let _ = std::fs::write(self.home.join("desk.json"), serde_json::json!(wanted.iter().map(|(id, dex, name)| serde_json::json!({ "id": id, "dex": variants::number(*dex), "shiny": variants::shiny(*dex), "name": name, "onDesk": true })).collect::<Vec<_>>()).to_string());
        for (id, dex, name) in wanted {
            if self.shown.iter().any(|shown| shown.id == id) {
                continue;
            }
            let folder = self.options.models.join(variants::folder(dex));
            if !folder.join("model.mon").exists() {
                // Baked now if it can be, and put on the desktop when that is done.
                self.want(dex, &name);
                continue;
            }
            let at = places.get(&id).copied();
            self.add(event_loop, &folder, id.clone(), dex, at, None);
            // Changed where it stood: it has something to say about it.
            if at.is_some() {
                self.cheer(&id);
            }
        }
    }

    /// Makes the desktop's wild mon the one the account's encounter is, if it has one: `encounter` is an
    /// `<Encounter>` or null.
    fn meet(&mut self, event_loop: &ActiveEventLoop, encounter: &serde_json::Value) {
        let id = encounter["id"].as_str().unwrap_or_default().to_string();
        // One that is no longer the encounter goes, unless a ball is out at it: the throw's own answer says then.
        for shown in &mut self.shown {
            if let Some((wild, ..)) = &mut shown.wild {
                if wild.encounter != id {
                    wild.leave();
                }
            }
        }
        if id.is_empty() || self.shown.iter().any(|shown| shown.wild.as_ref().is_some_and(|(wild, ..)| wild.encounter == id)) {
            return;
        }
        let (dex, name) = (variants::of(encounter).unwrap_or(0), encounter["name"].as_str().unwrap_or("?"));
        let folder = self.options.models.join(variants::folder(dex));
        if !folder.join("model.mon").exists() {
            let _ = &folder;
            self.want(dex, name);
            return;
        }
        println!("a wild {name} (level {}) is about: click it to throw a ball, {} left", encounter["level"], self.balls);
        self.add(event_loop, &folder, format!("wild:{id}"), dex, None, Some(Wild::new(id)));
        self.sound(sound::Sound::Came);
    }

    /// Floats words up over the mon with this id, if it is on the desktop: after whatever is floating there already.
    fn drop_over(&mut self, id: &str, words: &str, colour: [u8; 3]) {
        let (Some(gpu), Some(fonts)) = (&self.gpu, &self.fonts) else { return };
        // With the strip on the taskbar up, what a mon earns floats up from its plate there, not over the mon.
        if let Some(strip) = &mut self.strip {
            if let Some(index) = strip.says.iter().position(|(whose, _)| whose == id) {
                let scale = self.shown.first().map_or(1.0, |shown| shown.scale);
                let picture = label::floating(fonts, words, colour, scale);
                let (width, height) = (picture.width as f32, picture.height as f32);
                let mon = Rc::new(Mon::plate(picture.pixels, picture.width, picture.height));
                let from = strip.drops.last().map_or(self.clock, |last| (last.from + DROP_APART).max(self.clock));
                strip.drops.push(Drop { pose: Pose::new(&mon), model: gpu.model(mon), width, height, from, across: strip.middles[index] });
                return;
            }
        }
        let Some(shown) = self.shown.iter_mut().find(|shown| shown.id == id) else { return };
        let picture = label::floating(fonts, words, colour, shown.scale);
        let (width, height) = (picture.width as f32, picture.height as f32);
        let mon = Rc::new(Mon::plate(picture.pixels, picture.width, picture.height));
        let from = shown.drops.last().map_or(self.clock, |last| (last.from + DROP_APART).max(self.clock));
        shown.drops.push(Drop { pose: Pose::new(&mon), model: gpu.model(mon), width, height, from, across: 0.0 });
    }

    /// Keeps the strip on the taskbar as it should be: there while it is wanted and there are mons to show, saying
    /// what their plates say, drawn when it has changed or something floats from it. Whether it wants the
    /// quicker frames.
    fn tend_strip(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let says: Vec<(String, label::Says)> = self.shown.iter().filter(|shown| shown.wild.is_none()).filter_map(|shown| Some((shown.id.clone(), shown.plate.as_ref()?.says.clone()))).collect();
        if !self.on_taskbar || !self.account || self.hidden || says.is_empty() {
            self.strip = None;
            return false;
        }
        let (Some(gpu), Some(fonts)) = (&self.gpu, &self.fonts) else { return false };
        if self.strip.as_ref().is_none_or(|strip| strip.says != says) {
            let Some((bar, _)) = desk::clock_corner() else { return false };
            let scale = self.shown.first().map_or(1.0, |shown| shown.scale);
            let taskbar = (bar[3] - bar[1]) as f32;
            // Plates a little smaller than over a mon, to sit inside the taskbar.
            let plates: Vec<label::Picture> = says.iter().map(|(_, says)| label::draw(fonts, says, (150.0 * scale) as u32, scale * 0.86)).collect();
            let (picture, middles) = label::row(&plates, (STRIP_GAP * scale) as u32);
            let size = (picture.width, (taskbar + STRIP_ROOM * scale) as u32);
            let dims = (picture.width as f32, picture.height as f32);
            let mon = Rc::new(Mon::plate(picture.pixels, picture.width, picture.height));
            // The same window while it is the same size: another, when a mon has come or gone.
            let old = self.strip.take().filter(|strip| strip.size == size);
            let (target, hwnd, drops) = match old {
                Some(strip) => (strip.target, strip.hwnd, strip.drops),
                None => {
                    let attributes = Window::default_attributes()
                        .with_title("desk mons strip")
                        .with_inner_size(PhysicalSize::new(size.0, size.1))
                        .with_decorations(false)
                        .with_resizable(false)
                        .with_transparent(true)
                        .with_window_level(WindowLevel::AlwaysOnTop)
                        .with_active(false)
                        
                        ;
                    let window = Arc::new(event_loop.create_window(desk::overlay_attributes(attributes)).expect("a window"));
                    // The mouse goes through it to the taskbar and whatever is over it.
                    let _ = window.set_cursor_hittest(false);
                    let hwnd = desk::native_handle(&window);
                    desk::never_active(hwnd);
                    (gpu.target(window), hwnd, Vec::new())
                }
            };
            let mut strip = Strip { target, hwnd, says, pose: Pose::new(&mon), model: gpu.model(mon), middles, picture: dims, size, taskbar, drops, stale: true, placed: (i32::MIN, i32::MIN) };
            strip.place();
            self.strip = Some(strip);
        }
        let Some(strip) = &mut self.strip else { return false };
        let clock = self.clock;
        let floating = !strip.drops.is_empty();
        strip.drops.retain(|drop| clock - drop.from < DROP_LASTS);
        // Once more after the last of them has gone, to clear it away.
        if strip.stale || floating {
            strip.draw(gpu, clock);
        }
        !strip.drops.is_empty()
    }

    /// A species is wanted on the desktop and has no baked model: it is baked, on a thread of its own, if there is
    /// a model of it to bake. What comes of it is told to the app's loop (`Poke::Baked`).
    fn want(&mut self, dex: u64, name: &str) {
        if self.baking.contains(&dex) || self.unbaked.contains(&dex) {
            return;
        }
        match &self.baker {
            Some(baker) if baker.can(dex) => {
                println!("[desk] baking the model for {name}");
                self.baking.insert(dex);
                let (baker, proxy) = (baker.clone(), self.proxy.clone());
                std::thread::spawn(move || {
                    let how = baker.bake(dex);
                    let _ = proxy.send_event(Poke::Baked(dex, how));
                });
            }
            // Said once: there is nothing to make it of. It can still be thrown at in the panel.
            _ => {
                self.unbaked.insert(dex);
                eprintln!("[desk] there is no model for {name} (number {dex}) to show it with");
            }
        }
    }

    /// The mon with this id roars, if it is on the desktop and has a roar.
    fn cheer(&mut self, id: &str) {
        if let Some(shown) = self.shown.iter_mut().find(|shown| shown.id == id) {
            let mon = &shown.model.mon;
            shown.once = mon.clip("roar").or(mon.clip("attack")).map(|clip| (clip, 0.0));
        }
    }

    /// Makes each mon's plate say what the account's state now says of it: nothing made anew where nothing changed.
    fn replate(&mut self) {
        let (Some(gpu), Some(fonts)) = (&self.gpu, &self.fonts) else { return };
        for shown in &mut self.shown {
            let says = match &shown.wild {
                Some((wild, ..)) => {
                    let encounter = &self.state["encounter"];
                    (encounter["id"].as_str() == Some(wild.encounter.as_str())).then(|| label::Says {
                        name: format!("Wild {}{}", if encounter["shiny"]==true {"shiny "} else {""}, encounter["name"].as_str().unwrap_or("?")),
                        under: None,
                        level: encounter["level"].as_i64(),
                        // Its bar is how far it has warmed to the work going on by it.
                        xp: Some(((encounter["warmth"].as_f64().unwrap_or(0.0) * 50.0).round() / 50.0) as f32),
                        warm: true,
                    })
                }
                None => self.state["mons"].as_array().into_iter().flatten().find(|mon| mon["id"].as_str() == Some(shown.id.as_str())).map(|mon| {
                    let (name, nick) = (mon["name"].as_str().unwrap_or("?"), mon["nick"].as_str());
                    let (xp, next) = (mon["xp"].as_f64().unwrap_or(0.0), mon["xpToNext"].as_f64().unwrap_or(0.0));
                    // To a hundredth: a plate is not drawn again for a sliver of a bar nobody could see.
                    let along = if next > 0.0 { ((xp / next) * 100.0).round() / 100.0 } else { 1.0 };
                    label::Says { name: format!("{}{}",if mon["shiny"]==true {"Shiny "} else {""},nick.unwrap_or(name)), under: nick.map(|_| name.to_string()), level: mon["level"].as_i64(), xp: Some(along as f32), warm: false }
                }),
            };
            if shown.plate.as_ref().map(|plate| &plate.says) == says.as_ref() {
                continue;
            }
            // Worth putting up again: a plate that was not there, another name, another level. A bar a little
            // longer is not: it is drawn anew and stays as it was, up or gone.
            let news = match (&shown.plate, &says) {
                (Some(plate), Some(says)) => (&plate.says.name, &plate.says.under, plate.says.level) != (&says.name, &says.under, says.level),
                _ => true,
            };
            if news && !self.on_taskbar {
                shown.plate_until = self.clock + PLATE_UP + PLATE_FADES;
            }
            shown.plate = says.map(|says| {
                let picture = label::draw(fonts, &says, shown.width, shown.scale);
                let (width, height) = (picture.width as f32, picture.height as f32);
                let mon = Rc::new(Mon::plate(picture.pixels, picture.width, picture.height));
                Plate { says, pose: Pose::new(&mon), model: gpu.model(mon), width, height }
            });
        }
    }

    /// Hears what the server thread has to tell.
    fn hear(&mut self, event_loop: &ActiveEventLoop) {
        let told: Vec<net::Told> = self.told.as_ref().map(|told| told.try_iter().collect()).unwrap_or_default();
        let heard = !told.is_empty();
        for told in told {
            // Replies from requests already in flight belong to the previous session.
            if self.account_changing && !matches!(&told,net::Told::SessionEnded(_)) {continue;}
            match told {
                net::Told::SessionEnded(result) => {
                    self.account_changing=false;
                    match result {
                        Ok(switch) => {self.forget_account();self.note=if switch {"Choose your account in the browser."} else {"Signed out of this desktop. Tracking is off until you sign in."}.into();},
                        Err(message) => {self.account=self.state["account"]["id"].is_string();self.note=message;self.heartbeat_at=None;let mons=self.state["mons"].clone();self.settle(event_loop,&mons);}
                    }
                }
                net::Told::Connection(online) => {
                    self.server_away = !online;
                    if online { self.server_seen = Some(Instant::now()); }
                }
                net::Told::Note(text) => {
                    println!("[desk] {text}");
                    self.note = text;
                }
                net::Told::Page(title, class, whole_site, likely) => {
                    println!("page {title:?}: {}{}", class.as_deref().unwrap_or("nobody could say"), if whole_site { ", as its whole site is" } else { "" });
                    self.tracker.learn(&title, class.as_deref().and_then(track::Class::named), whole_site, likely);
                }
                net::Told::Social(social) => {
                    // A trade that is no longer open may have been taken: who has which mon is asked for anew.
                    if social["trades"].as_array().map_or(0, Vec::len) < self.social["trades"].as_array().map_or(0, Vec::len) {
                        if let Some(asks) = &self.asks {
                            let _ = asks.send(net::Ask::Refresh);
                        }
                    }
                    self.social = social;
                }
                net::Told::Ranks(path, table) => {
                    self.ranks.insert(path, table);
                }
                net::Told::FriendMons(id, mons) => {
                    self.theirs.insert(id, mons);
                }
                net::Told::NoAccount => {
                    self.forget_account();
                    self.open_panel(event_loop);
                }
                net::Told::State(state) => {
                    self.account = true;
                    // The server can say what pages are, which was not known till now: the browser that may
                    // already be in front is looked at as if it had just come there.
                    if state["sorter"].as_bool().unwrap_or(false) && !self.state["sorter"].as_bool().unwrap_or(false) {
                        desk::look_again();
                    }
                    self.state = state.clone();
                    self.tracker.set_rules(&state["rules"]);
                    self.tracker.set_sure(&state["sure"]);
                    self.listed = true;
                    // Kept for the next start, should the server be away then.
                    let _ = std::fs::write(self.home.join("rules.json"), state["rules"].to_string());
                    println!("state: {} points, {} balls, {} mons", state["points"], state["balls"], state["mons"].as_array().map_or(0, |m| m.len()));
                    self.balls = state["balls"].as_i64().unwrap_or(0);
                    self.better = (state["great"].as_i64().unwrap_or(0), state["ultra"].as_i64().unwrap_or(0));
                    self.points = state["points"].as_i64().unwrap_or(0);
                    self.say_status();
                    self.settle(event_loop, &state["mons"]);
                    self.meet(event_loop, &state["encounter"]);
                    // A session that ended while nobody was told is said now; one that ended before this start is not.
                    if self.session_said.is_none() {
                        self.session_said = Some(state["lastSession"]["id"].as_str().unwrap_or_default().to_string());
                    } else {
                        self.session_over(&state["lastSession"]);
                    }
                    self.pace(&state);
                    // For tests, which cannot catch: the newest mon is welcomed as if it had just been caught.
                    if std::env::var_os("DESK_WELCOME").is_some_and(|_| !self.welcomed) {
                        self.welcomed = true;
                        self.newly = state["mons"].as_array().and_then(|mons| mons.last()).cloned();
                        self.welcome(event_loop);
                    }
                }
                net::Told::Threw(encounter, answer) => {
                    let outcome = match &answer {
                        Ok(answer) => {
                            self.balls = answer["balls"].as_i64().unwrap_or(self.balls);
                            let (caught, fled) = (answer["caught"].as_bool().unwrap_or(false), answer["fled"].as_bool().unwrap_or(false));
                            println!("threw: {} shakes, {}, {} balls left", answer["shakes"], if caught { "caught" } else if fled { "it fled" } else { "it broke out" }, self.balls);
                            Outcome { shakes: answer["shakes"].as_u64().unwrap_or(0) as u32, caught, stays: !caught && !fled }
                        }
                        // Nothing was thrown: the ball comes open at once. With the encounter over, the mon goes too.
                        Err(code) => {
                            println!("[desk] the ball was not thrown: {code}");
                            Outcome { shakes: 0, caught: false, stays: code != "no-encounter" }
                        }
                    };
                    let on_show = match self.shown.iter_mut().filter_map(|shown| shown.wild.as_mut()).find(|(wild, ..)| wild.encounter == encounter) {
                        Some((wild, ..)) => {
                            wild.answer(outcome);
                            true
                        }
                        None => false,
                    };
                    // A catch is welcomed: when its ball has stopped rocking on the desktop, so as not to give the
                    // end away, or now if there is no ball to watch.
                    if let Ok(answer) = &answer {
                        if answer["mon"].is_object() {
                            self.newly = Some(answer["mon"].clone());
                            if !on_show {
                                self.welcome(event_loop);
                            }
                        }
                    }
                    self.say_status();
                    if self.state.is_object() {
                        self.state["balls"] = self.balls.into();
                    }
                    // Whatever came of it, the account is not as it was.
                    if let Some(asks) = &self.asks {
                        let _ = asks.send(net::Ask::Refresh);
                    }
                }
                net::Told::Reported(answer) => {
                    if answer["gains"].as_array().into_iter().flatten().any(|g|g["xp"].as_f64().unwrap_or(0.0)>0.0) { self.state["tracking"]["hasEarnedXP"] = serde_json::json!(true); }
                    println!("reported: +{} points ({}), {} balls, credited {}", answer["pointsEarned"], answer["points"], answer["balls"], answer["credited"]);
                    self.balls = answer["balls"].as_i64().unwrap_or(self.balls);
                    self.points = answer["points"].as_i64().unwrap_or(self.points);
                    // The panel's copy of the account, brought up to what this answer says of it.
                    if self.state.is_object() {
                        for key in ["points", "balls", "encounter", "streak", "today", "session", "rested", "breakDue"] {
                            self.state[key] = answer[key].clone();
                        }
                        if answer["sessionEnded"].is_object() {
                            self.state["lastSession"] = answer["sessionEnded"].clone();
                        }
                        for mon in answer["mons"].as_array().into_iter().flatten() {
                            if let Some(kept) = self.state["mons"].as_array_mut().into_iter().flatten().find(|kept| kept["id"] == mon["id"]) {
                                *kept = mon.clone();
                            }
                        }
                    }
                    if answer["earning"]["seconds"].as_f64().unwrap_or(0.0) > 0.0 {
                        self.earning = answer["earning"].clone();
                        self.earning["xp"] = answer["gains"][0]["xp"].clone();
                        self.earning["in"] = self.reported_in.clone().into();
                    }
                    self.say_status();
                    self.settle(event_loop, &answer["mons"]);
                    self.meet(event_loop, &answer["encounter"]);
                    if self.session_said.is_some() {
                        self.session_over(&answer["sessionEnded"]);
                    }
                    self.pace(&answer);
                    if answer["breakEarned"].as_bool().unwrap_or(false) {
                        println!("a break was earned");
                        for id in self.own() {
                            self.drop_over(&id, "rested!", [150, 200, 255]);
                        }
                        self.note = "A break well taken: your mons earn a quarter more for a while.".to_string();
                        self.sound(sound::Sound::Rested);
                    }
                    if answer["levels"].as_array().is_some_and(|levels| !levels.is_empty()) {
                        self.sound(sound::Sound::Level);
                    }
                    if answer["goalJustMet"].as_bool().unwrap_or(false) {
                        println!("the day's goal is met");
                        let own: Vec<String> = self.shown.iter().filter(|shown| shown.wild.is_none()).map(|shown| shown.id.clone()).collect();
                        for id in own {
                            self.drop_over(&id, "Goal!", [150, 232, 150]);
                            self.cheer(&id);
                        }
                        if let Some(asks) = &self.asks {
                            let _ = asks.send(net::Ask::Refresh);
                        }
                    }
                    // What each mon earned, over its head: a whole number of it, or it is not worth saying.
                    for gain in answer["gains"].as_array().into_iter().flatten() {
                        let xp = gain["xp"].as_f64().unwrap_or(0.0).round() as i64;
                        if xp >= 1 {
                            self.drop_over(gain["mon"].as_str().unwrap_or_default(), &format!("+{xp} XP"), [255, 214, 92]);
                        }
                    }
                    for level in answer["levels"].as_array().into_iter().flatten() {
                        println!("level {} -> {}", level["from"], level["to"]);
                        self.cheer(level["mon"].as_str().unwrap_or_default());
                        self.drop_over(level["mon"].as_str().unwrap_or_default(), &format!("Level {}!", level["to"]), [140, 230, 255]);
                    }
                    for change in answer["evolutions"].as_array().into_iter().flatten() {
                        println!("evolved: {} -> {}", change["from"], change["to"]);
                    }
                }
            }
        }
        if heard {
            self.replate();
        }
        if let (true, Some(panel)) = (heard, &self.panel) {
            panel.window.request_redraw();
        }
        if let (true, Some(board)) = (heard, &self.board) {
            board.window.request_redraw();
        }
        self.next = self.next.min(Instant::now() + Duration::from_secs(1));
    }

    fn of(&mut self, id: WindowId) -> Option<&mut Shown> {
        self.shown.iter_mut().find(|s| s.target.window.id() == id)
    }
}

impl App {
    /// Installs or removes the platform's login-start entry.
    fn startup_change(&self, enabled: bool) -> bool {
        let Ok(executable)=std::env::current_exe() else {return false;};
        let Ok(models)=std::fs::canonicalize(&self.options.models) else {return false;};
        desk::set_startup(enabled,&executable,&models,&self.options.server)
    }

    /// The server's page in the browser, signed in as this account: the token goes in the address's fragment, which
    /// is never sent to the server.
    fn open_page(&self) {
        let token = if self.account && !self.account_changing {std::fs::read_to_string(self.home.join("token")).unwrap_or_default()} else {String::new()};
        let token = token.trim();
        desk::open(&if token.is_empty() { self.options.server.clone() } else { format!("{}/#token={token}", self.options.server) });
        // What is changed there is wanted here soon, not in five minutes.
        if let Some(asks) = &self.asks {
            let _ = asks.send(net::Ask::Refresh);
        }
    }

    /// Shows the mon just caught in the panel, to be named and given a place.
    fn welcome(&mut self, event_loop: &ActiveEventLoop) {
        let Some(mon) = self.newly.take() else { return };
        self.open_panel(event_loop);
        if let Some(panel) = &mut self.panel {
            panel.welcome(mon);
            panel.window.request_redraw();
        }
    }

    fn sound(&self, sound: sound::Sound) {
        if self.sounds {
            sound::play(sound);
        }
    }

    /// The mons on the desktop that are the account's own, by their ids.
    fn own(&self) -> Vec<String> {
        self.shown.iter().filter(|shown| shown.wild.is_none() && !shown.id.is_empty()).map(|shown| shown.id.clone()).collect()
    }

    /// A focus session has ended: the mons say what came of it.
    fn session_over(&mut self, ended: &serde_json::Value) {
        let Some(id) = ended["id"].as_str() else { return };
        if self.session_said.as_deref() == Some(id) {
            return;
        }
        self.session_said = Some(id.to_string());
        let kept = ended["kept"].as_bool().unwrap_or(false);
        println!("the focus session is over: {}", if kept { "kept" } else { "not kept" });
        for id in self.own() {
            if kept {
                self.drop_over(&id, &format!("Kept! +{}", ended["points"]), [150, 232, 150]);
                self.cheer(&id);
            } else if !ended["stopped"].as_bool().unwrap_or(false) {
                self.drop_over(&id, "...", [200, 200, 208]);
            }
        }
        self.note = match (kept, ended["stopped"].as_bool().unwrap_or(false)) {
            (true, _) => format!("Session kept: {} points, and {:.0} xp for each mon on the desk.", ended["points"], ended["xp"].as_f64().unwrap_or(0.0)),
            (false, true) => "Session stopped.".to_string(),
            (false, false) => format!("Session not kept: {:.0}% of it was productive.", ended["share"].as_f64().unwrap_or(0.0) * 100.0),
        };
        if !ended["stopped"].as_bool().unwrap_or(false) {
            self.sound(if kept { sound::Sound::Kept } else { sound::Sound::Lost });
        }
    }

    /// What the server says of sessions and breaks, from the state or from a report's answer.
    fn pace(&mut self, said: &serde_json::Value) {
        // When the session now going ends, the server is asked what came of it.
        self.session_due = said["session"]["endsAt"].as_f64().zip(self.state["now"].as_f64().or(said["now"].as_f64())).map(|(ends, now)| Instant::now() + Duration::from_millis((ends - now).max(0.0) as u64 + 1500));
        let due = said["breakDue"].as_bool().unwrap_or(false);
        if due && !self.break_said {
            if let Some(id) = self.own().first().cloned() {
                self.drop_over(&id, "take five?", [150, 200, 255]);
            }
        }
        self.break_said = due;
    }

    /// Writes down what is to be as it is at the next start.
    fn keep_settings(&self) {
        let kept = serde_json::json!({ "taskbar": self.on_taskbar, "board": self.on_board, "boardAt": self.board_at.map(|(x, y)| [x, y]), "sounds": self.sounds });
        let _ = std::fs::write(self.home.join("settings.json"), kept.to_string());
    }

    /// Keeps the friends' small window as it should be: up while it is wanted and there is an account.
    fn tend_board(&mut self, event_loop: &ActiveEventLoop) {
        if !self.on_board || !self.account {
            self.board = None;
        } else if self.board.is_none() && self.options.snap.is_none() {
            let gpu = self.gpu.get_or_insert_with(Gpu::headless);
            let board = board::Board::open(event_loop, gpu, self.board_at, &self.options.server);
            board.window.request_redraw();
            self.board = Some(board);
        }
    }

    /// The friends' small window was shut by its own cross: off, and the icon's menu says so.
    fn shut_board(&mut self) {
        self.on_board = false;
        self.board = None;
        if let Some(tray) = &self.tray {
            tray.board.set_checked(false);
        }
        self.keep_settings();
    }

    /// Opens the app's own window, or brings it forward if it is open.
    fn open_panel(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(panel) = &self.panel {
            panel.window.set_minimized(false);
            panel.window.focus_window();
            return;
        }
        // With no mon on the desktop yet there is no GPU open yet either.
        let gpu = self.gpu.get_or_insert_with(Gpu::headless);
        let mut panel = panel::Panel::open(event_loop, gpu, self.options.icons.clone(), self.home.join("activity-icons"));
        panel.show(&self.options.pick);
        panel.window.request_redraw();
        self.panel = Some(panel);
        // What it shows should be what is so now.
        if let Some(asks) = &self.asks {
            let _ = asks.send(net::Ask::Refresh);
        }
    }

    /// Does what was pressed in the panel.
    fn act(&mut self, acts: Vec<panel::Act>) {
        for act in acts {
            if self.account_changing || self.exit_mode.is_some() {return;}
            let ask = match act {
                panel::Act::EndSession(switch) => {
                    self.track();let report=self.take_report();
                    if let Some(asks)=&self.asks {
                        if asks.send(net::Ask::EndSession(switch,report)).is_ok() {
                            self.account_changing=true;self.account=false;self.shown.clear();self.board=None;self.strip=None;self.newly=None;
                            self.note="Saving pending activity and signing out…".into();
                        } else {self.note="Could not contact the app’s connection thread. Your account is still signed in.".into();}
                    }
                    return;
                }
                panel::Act::Post(path, body) => Some(net::Ask::Post(path.to_string(), body)),
                panel::Act::Link(starter, name) => Some(net::Ask::Link(starter, name)),
                panel::Act::Friends => Some(net::Ask::Social(String::new(), None)),
                panel::Act::Social(path, body) => Some(net::Ask::Social(path.to_string(), Some(body))),
                panel::Act::FriendMons(id) => Some(net::Ask::FriendMons(id)),
                // Said: the next thing to say is new even if it is the same words.
                panel::Act::Noted => {
                    self.note.clear();
                    None
                }
                // A switch thrown in the panel is the same switch in the icon's menu, thrown the same way.
                panel::Act::Switch(key, on) => {
                    if let Some(tray) = &self.tray {
                        let item = match key {
                            "taskbar" => &tray.taskbar,
                            "board" => &tray.board,
                            "sounds" => &tray.sounds,
                            "hide" => &tray.hide,
                            "track" => &tray.track,
                            _ => &tray.startup,
                        };
                        item.set_checked(on);
                        let _ = self.proxy.send_event(Poke::Menu(item.id().clone()));
                    }
                    None
                }
                panel::Act::Ranks(path) => Some(net::Ask::Ranks(path)),
                // Kept on this machine: what is never reported is nobody else's to know.
                panel::Act::Private(name, kept) => {
                    let name = name.trim().to_lowercase();
                    if kept && !name.is_empty() {
                        self.private.insert(name);
                    } else {
                        self.private.remove(&name);
                    }
                    let _ = std::fs::write(self.home.join("private.json"), serde_json::json!(self.private).to_string());
                    // Should it be in front now, it is taken for what it now is.
                    desk::look_again();
                    None
                }
                panel::Act::EnableWebsiteTracking => { desk::request_accessibility(); desk::look_again(); None },
                panel::Act::Page => {
                    self.open_page();
                    None
                }
                panel::Act::Community => {
                    desk::open(COMMUNITY_URL);
                    None
                }
                panel::Act::CheckUpdates => {self.updater.check();None},
                panel::Act::RestartUpdate => {self.begin_exit(true);return;},
                // At the wild mon on the desktop if it is there, so that the ball is seen; else just asked of the server.
                panel::Act::Throw(ball) => {
                    let id = self.state["encounter"]["id"].as_str().unwrap_or_default().to_string();
                    match self.shown.iter_mut().filter_map(|shown| shown.wild.as_mut()).find(|(wild, ..)| wild.encounter == id) {
                        Some((wild, ..)) if !wild.about() => None,
                        Some((wild, ..)) => {
                            wild.throw();
                            Some(net::Ask::Throw(id, ball.to_string()))
                        }
                        None => (!id.is_empty()).then_some(net::Ask::Throw(id, ball.to_string())),
                    }
                }
            };
            if let (Some(ask), Some(asks)) = (ask, &self.asks) {
                let _ = asks.send(ask);
            }
        }
    }

    /// The ball a click or a fling throws: a plain one while there are any, so that the better ones are spent on
    /// purpose (from the panel), then whatever is left.
    fn ball(&self) -> Option<&'static str> {
        if self.balls > 0 {
            Some("ball")
        } else if self.better.0 > 0 {
            Some("great")
        } else if self.better.1 > 0 {
            Some("ultra")
        } else {
            None
        }
    }

    fn say_status(&self) {
        if let Some(tray) = &self.tray {
            tray.say(&format!("{} points, {} balls", self.points, self.balls));
        }
    }
}

impl ApplicationHandler<Poke> for App {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, poke: Poke) {
        let id = match poke {
            Poke::Menu(id) => id,
            Poke::Icon => {
                self.open_panel(event_loop);
                return;
            }
            // Heard at the next tick, which is now.
            Poke::Told => {
                self.next = Instant::now();
                return;
            }
            // The mon that was waiting for its model can be put on the desktop now.
            Poke::Baked(dex, how) => {
                self.baking.remove(&dex);
                match how {
                    Ok(()) => {
                        println!("[desk] baked the model for number {dex}");
                        if self.state.is_object() {
                            let state = self.state.clone();
                            self.settle(event_loop, &state["mons"]);
                            self.meet(event_loop, &state["encounter"]);
                            self.replate();
                        }
                    }
                    Err(why) => {
                        eprintln!("[desk] the model for number {dex} could not be baked: {why}");
                        self.unbaked.insert(dex);
                    }
                }
                self.next = Instant::now();
                return;
            }
        };
        if self.tray.as_ref().is_some_and(|tray| id == *tray.panel.id()) {
            self.open_panel(event_loop);
            return;
        }
        let Some(tray) = &self.tray else { return };
        if id == *tray.quit.id() {
            self.begin_exit(false);
        } else if id == *tray.update.id() {
            if self.updater.ready() {self.begin_exit(true);} else {self.updater.check();}
        } else if id == *tray.page.id() {
            self.open_page();
        } else if id == *tray.community.id() {
            desk::open(COMMUNITY_URL);
        } else if id == *tray.hide.id() {
            self.paused = tray.hide.is_checked();
        } else if id == *tray.taskbar.id() {
            self.on_taskbar = desk::supports_taskbar() && tray.taskbar.is_checked();
            self.keep_settings();
        } else if id == *tray.sounds.id() {
            self.sounds = tray.sounds.is_checked();
            self.keep_settings();
        } else if id == *tray.board.id() {
            self.on_board = tray.board.is_checked();
            self.keep_settings();
            self.tend_board(event_loop);
        } else if id == *tray.track.id() {
            self.untracked = tray.track.is_checked();
            self.heartbeat_at = None;
            println!("[desk] tracking {}", if self.untracked { "paused: nothing is counted or sent" } else { "on again" });
        } else if id == *tray.startup.id() {
            if !self.startup_change(tray.startup.is_checked()) {
                eprintln!("[desk] could not change start-at-login");
            }
            tray.startup.set_checked(desk::startup_enabled());
        }
        self.next = Instant::now();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.started {
            return;
        }
        desk::watch();
        if let Err(error)=desk::prepare_home(&self.home) { eprintln!("[desk] profile directory: {error}"); }
        // The lists, until the server gives them: a program's name to "productive", "neutral" or "distracting".
        if let Some(rules) = std::fs::read_to_string(self.home.join("rules.json")).ok().and_then(|text| serde_json::from_str(&text).ok()) {
            self.tracker.set_rules(&rules);
            self.listed = true;
        }
        self.listed |= !self.options.mons.is_empty();
        if let Some(private) = std::fs::read_to_string(self.home.join("private.json")).ok().and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok()) {
            self.private = private.into_iter().collect();
        }
        self.tracker.front(desk::front_program().filter(|path| !self.private.contains(&track::program(path))).as_deref());
        desk::front_changed();
        self.tracked_at = desk::ticks();
        self.surfaces = desk::surfaces();
        self.began = Instant::now();
        self.last = self.began;
        self.next = self.began;
        if self.options.mons.is_empty() {
            let (asks, heard) = std::sync::mpsc::channel();
            let (tell, told) = std::sync::mpsc::channel();
            let proxy = self.proxy.clone();
            let wake = Box::new(move || {
                let _ = proxy.send_event(Poke::Told);
            });
            let link = net::Link { server: self.options.server.clone(), home: self.home.clone(), guest: self.options.guest, wake };
            self.account = self.home.join("token").exists();
            std::thread::spawn(move || net::run(link, heard, tell));
            if let Some((path, body)) = self.options.ask.split_once(' ') {
                let (asks, path, body) = (asks.clone(), path.to_string(), serde_json::from_str(body).expect("--ask '<path> <json>'"));
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(4));
                    let _ = asks.send(net::Ask::Post(path, body));
                });
            }
            (self.asks, self.told) = (Some(asks), Some(told));
            // The mons that were on the desk last time, until the server says who is now: it may be away.
            if let Some(mons) = std::fs::read_to_string(self.home.join("desk.json")).ok().and_then(|text| serde_json::from_str(&text).ok()) {
                self.settle(event_loop, &mons);
            }
        } else {
            for folder in self.options.mons.clone() {
                self.add(event_loop, &folder, String::new(), 0, None, None);
            }
            assert!(!self.shown.is_empty(), "none of the folders named with --mons holds a baked model");
        }
        // The menu's picks come on another thread's say-so: they are handed to this one, which wakes for them.
        let proxy = self.proxy.clone();
        tray_icon::menu::MenuEvent::set_event_handler(Some(move |event: tray_icon::menu::MenuEvent| {
            let _ = proxy.send_event(Poke::Menu(event.id));
        }));
        let proxy = self.proxy.clone();
        tray_icon::TrayIconEvent::set_event_handler(Some(move |event: tray_icon::TrayIconEvent| {
            if let tray_icon::TrayIconEvent::Click { button: tray_icon::MouseButton::Left, button_state: tray_icon::MouseButtonState::Up, .. } = event {
                let _ = proxy.send_event(Poke::Icon);
            }
        }));
        let kept: serde_json::Value = std::fs::read_to_string(self.home.join("settings.json")).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        self.on_taskbar = desk::supports_taskbar() && (kept["taskbar"] == true || self.options.pick == "taskbar");
        self.on_board = kept["board"] == true || self.options.pick == "board";
        self.sounds = kept["sounds"] != false && self.options.seconds.is_none();
        self.board_at = kept["boardAt"][0].as_i64().zip(kept["boardAt"][1].as_i64()).map(|(x, y)| (x as i32, y as i32));
        self.tend_board(event_loop);
        match tray::Tray::new(desk::startup_enabled(), self.on_taskbar, self.on_board, self.sounds) {
            Ok(tray) => {
                let picked = match self.options.pick.as_str() {
                    "hide" => {
                        tray.hide.set_checked(true);
                        Some(tray.hide.id().clone())
                    }
                    "quit" => Some(tray.quit.id().clone()),
                    "panel" | "today" | "mons" | "dex" | "programs" | "sites" | "shop" | "friends" | "ranks" | "app" => Some(tray.panel.id().clone()),
                    _ => None,
                };
                if let Some(id) = picked {
                    let proxy = self.proxy.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(3));
                        let _ = proxy.send_event(Poke::Menu(id));
                    });
                }
                self.tray = Some(tray);
            }
            Err(error) => eprintln!("[desk] no icon by the clock: {error}"),
        }
        // Refresh an existing login entry after migration from the legacy installer or an app move.
        if desk::startup_enabled() {self.startup_change(true);}
        self.started = true;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.board.as_ref().is_some_and(|board| board.window.id() == id) {
            let (Some(board), Some(gpu)) = (&mut self.board, &self.gpu) else { return };
            match event {
                WindowEvent::CloseRequested => self.shut_board(),
                WindowEvent::Moved(at) => {
                    // Written down once it has lain still a moment, not at every step of being dragged.
                    self.board_at = Some((at.x, at.y));
                    self.board_moved = Some(Instant::now());
                }
                WindowEvent::RedrawRequested => {
                    let said = board.draw(gpu, &self.social);
                    if said.ask {
                        if let Some(asks) = &self.asks {
                            let _ = asks.send(net::Ask::Social(String::new(), None));
                        }
                    }
                    if said.shut {
                        self.shut_board();
                    }
                    if said.social {
                        self.open_panel(event_loop);
                        if let Some(panel) = &mut self.panel {
                            panel.show("friends");
                            panel.window.request_redraw();
                        }
                        if let Some(asks) = &self.asks {
                            let _ = asks.send(net::Ask::Social(String::new(), None));
                        }
                    }
                }
                event => {
                    if board.event(gpu, &event) {
                        board.window.request_redraw();
                    }
                }
            }
            return;
        }
        if self.panel.as_ref().is_some_and(|panel| panel.window.id() == id) {
            let (Some(panel), Some(gpu)) = (&mut self.panel, &self.gpu) else { return };
            match event {
                // Shut, it is gone, with everything it kept: the app goes on without it.
                WindowEvent::CloseRequested => self.panel = None,
                WindowEvent::RedrawRequested => {
                    let private: Vec<String> = self.private.iter().cloned().collect();
                    // The app's own switches, as the icon's menu has them.
                    let switches = [
                        ("taskbar", "Names and levels on the taskbar", self.on_taskbar),
                        ("board", "Friends' day in a small window", self.on_board),
                        ("sounds", "Sounds", self.sounds),
                        ("hide", "Hide the mons", self.paused),
                        ("track", "Pause tracking", self.untracked),
                        ("startup", desk::STARTUP_LABEL, self.tray.as_ref().is_some_and(|tray| tray.startup.is_checked())),
                    ];
                    let named = |class: track::Class| ["Productive", "Neutral", "Distracting"][class as usize];
                    let sites: Vec<(String, Option<&'static str>, bool)> = self.tracker.sites().into_iter().map(|(site, class, own)| (site, class.map(named), own)).collect();
                    let live_status = presence(self.untracked, !self.untracked && desk::idle_seconds() >= track::AWAY_AFTER, self.excluded, self.tracker.program_name(), self.tracker.now().and_then(|(_,class)|class));
                    let mut panel_status = live_status;
                    panel_status["online"] = serde_json::json!(self.server_seen.is_some_and(|at|at.elapsed() <= Duration::from_secs(45)) && !self.server_away);
                    panel_status["accessibilityRequired"]=serde_json::json!(desk::accessibility_needed());
                    panel_status["accountChanging"]=serde_json::json!(self.account_changing);
                    panel_status["update"]=serde_json::to_value(&self.updater.status).unwrap_or_default();
                    panel_status["savingForExit"]=serde_json::json!(self.exit_mode.is_some());
                    panel_status["lastClassification"]=if !self.untracked && !self.excluded {self.tracker.previous_classification().map(|(program,site,class)|serde_json::json!({"program":program,"site":site,"class":class.map(|class|["productive","neutral","distracting"][class as usize])})).unwrap_or_default()} else {serde_json::Value::Null};
                    panel_status["site"]=if !self.untracked && !self.excluded {self.tracker.now().and_then(|(what,_)|track::site_of(&what)).map(serde_json::Value::String).unwrap_or_default()} else {serde_json::Value::Null};
                    panel_status["syncAge"] = self.server_seen.map(|at|serde_json::json!(at.elapsed().as_secs())).unwrap_or(serde_json::Value::Null);
                    panel_status["nextReport"] = serde_json::json!((self.options.report_every-self.since_report).max(0.0).ceil());
                    let acts = panel.draw(gpu, &panel::View { state: &self.state, status: &panel_status, account: self.account, note: &self.note, sites: &sites, private: &private, social: &self.social, theirs: &self.theirs, ranks: &self.ranks, earning: &self.earning, switches: &switches });
                    if !acts.is_empty() {
                        panel.window.request_redraw();
                    }
                    self.act(acts);
                }
                event => {
                    if panel.event(gpu, &event) {
                        panel.window.request_redraw();
                    }
                }
            }
            return;
        }
        // Backing dimensions change when an overlay moves between Retina and non-Retina displays.
        if let WindowEvent::Resized(size)=&event {
            if let Some(gpu)=&self.gpu {
                for shown in &mut self.shown {
                    if shown.target.window.id()==id {shown.target.resize(gpu,size.width,size.height);return;}
                    if let Some(flung)=&mut shown.flung {if flung.target.window.id()==id {flung.target.resize(gpu,size.width,size.height);return;}}
                }
                if let Some(strip)=&mut self.strip {if strip.target.window.id()==id {strip.target.resize(gpu,size.width,size.height);return;}}
            }
        }
        // The ball by a wild mon: taken up, carried, and let go to fly.
        if let Some(shown) = self.shown.iter_mut().find(|shown| shown.flung.as_ref().is_some_and(|flung| flung.target.window.id() == id)) {
            let (cursor, time, balls) = (desk::cursor(), self.began.elapsed().as_secs_f32(), self.balls + self.better.0 + self.better.1);
            let cursor = (cursor.0 as f32, cursor.1 as f32);
            let about = shown.wild.as_ref().is_some_and(|(wild, ..)| wild.about());
            let Some(flung) = &mut shown.flung else { return };
            match event {
                WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                    if balls < 1 {
                        println!("[desk] no balls: buy some in the shop");
                    } else if about {
                        flung.toss.pick_up(cursor);
                    }
                }
                WindowEvent::CursorMoved { .. } => {
                    flung.toss.carry(cursor, time);
                    // At once, as a carried mon is.
                    flung.place();
                }
                WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => flung.toss.let_go(time),
                _ => {}
            }
            self.next = Instant::now();
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let from = desk::cursor();
                if let Some(shown) = self.of(id) {
                    shown.press = Some(Press { from, grip: (from.0 as f32 - shown.actor.x, from.1 as f32 - shown.actor.y) });
                }
            }
            // The mouse on a mon puts its plate up, and keeps it up until a while after it has gone.
            WindowEvent::CursorEntered { .. } | WindowEvent::CursorLeft { .. } => {
                let (over, until) = (matches!(event, WindowEvent::CursorEntered { .. }), self.clock + PLATE_UP + PLATE_FADES);
                if let Some(shown) = self.of(id) {
                    (shown.over, shown.plate_until) = (over, until);
                }
                self.next = Instant::now();
            }
            WindowEvent::CursorMoved { .. } => {
                let at = desk::cursor();
                let Some(shown) = self.of(id) else { return };
                let Some(press) = &shown.press else { return };
                if shown.actor.state != State::Carried && (at.0 - press.from.0).abs().max((at.1 - press.from.1).abs()) >= CARRY_FROM {
                    shown.actor.pick_up();
                }
                shown.actor.carry(at.0 as f32 - press.grip.0, at.1 as f32 - press.grip.1);
                // At once, not at the next frame: a mon that lags the hand that holds it feels heavy.
                shown.place();
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                let (ball, asks) = (self.ball(), self.asks.clone());
                let Some(shown) = self.of(id) else { return };
                if shown.press.take().is_none() {
                    return;
                }
                if shown.actor.state == State::Carried {
                    shown.actor.let_go();
                } else if let Some((wild, ..)) = &mut shown.wild {
                    // A ball at it, if there is one to throw and none is out already.
                    if let (true, Some(ball)) = (wild.about(), ball) {
                        wild.throw();
                        if let Some(asks) = asks {
                            let _ = asks.send(net::Ask::Throw(wild.encounter.clone(), ball.to_string()));
                        }
                    } else if wild.about() {
                        println!("[desk] no balls: buy some on the page");
                    }
                } else if shown.once.is_none() {
                    // Petted: it says so.
                    let mon = &shown.model.mon;
                    shown.once = mon.clip("roar").or(mon.clip("attack")).map(|clip| (clip, 0.0));
                }
                self.next = Instant::now();
            }
            // No answer to Windows asking for a repaint: the picture is the swapchain's, which Windows keeps showing,
            // and a frame drawn for every move of a walking mon's window doubled what walking cost.
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.tick(event_loop);
        if self.board_moved.is_some_and(|at| at.elapsed() >= Duration::from_secs(1)) {
            self.board_moved = None;
            self.keep_settings();
        }
        if let Some(board) = &mut self.board {
            match board.again {
                Some(again) if again <= Instant::now() => {
                    board.again = None;
                    board.window.request_redraw();
                }
                Some(again) if again < self.next && !event_loop.exiting() => {
                    self.next = again;
                    event_loop.set_control_flow(ControlFlow::WaitUntil(again));
                }
                _ => {}
            }
        }
        // egui asks to be drawn again by itself for an animation or a tooltip's delay, and for nothing else.
        if let Some(panel) = &mut self.panel {
            match panel.again {
                Some(again) if again <= Instant::now() => {
                    panel.again = None;
                    panel.window.request_redraw();
                }
                Some(again) if again < self.next && !event_loop.exiting() => event_loop.set_control_flow(ControlFlow::WaitUntil(again)),
                _ => {}
            }
        }
    }
}

impl App {
    fn tick(&mut self, event_loop: &ActiveEventLoop) {
        if !self.started || event_loop.exiting() {
            return;
        }
        self.poll_exit(event_loop);
        if event_loop.exiting() {return;}
        if self.exit_mode.is_some() {self.next=Instant::now()+Duration::from_millis(100);event_loop.set_control_flow(ControlFlow::WaitUntil(self.next));return;}
        let now = Instant::now();
        if let Some(seconds) = self.options.seconds {
            if self.began.elapsed().as_secs_f64() >= seconds {
                println!("drew {} frames in {:.1} s, {:.3} ms of this thread a frame", self.drawn, self.began.elapsed().as_secs_f64(), self.spent.as_secs_f64() * 1000.0 / self.drawn.max(1) as f64);
                if let Some(gpu) = &self.gpu {
                    let parts = gpu.spent.get().map(|d| d.as_secs_f64() * 1000.0 / self.drawn.max(1) as f64);
                    println!("of which {:.3} ms getting the picture, {:.3} ms drawing, {:.3} ms showing", parts[0], parts[1], parts[2]);
                }
                if let Some(asks) = &self.asks {
                    let _ = asks.send(net::Ask::Quit);
                }
                event_loop.exit();
                return;
            }
        }
        if now < self.next {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next));
            return;
        }
        // Whatever the mons are doing, asleep and hidden too.
        self.track();
        self.send_presence();
        self.hear(event_loop);
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;

        // Is anyone there, and is the screen theirs to look at?
        self.since_look += dt;
        if self.since_look >= LOOK_EVERY || self.asleep || self.hidden || self.paused {
            self.since_look = 0.0;
            let hidden = self.paused || desk::busy();
            if let Some(strip) = &mut self.strip {
                strip.place();
            }
            if hidden != self.hidden {
                self.hidden = hidden;
                for shown in &self.shown {
                    shown.target.window.set_visible(!hidden);
                }
                if let Some(board) = &self.board {
                    board.window.set_visible(!hidden);
                }
            }
            // Measured with nobody at the keyboard, the mons stay up.
            self.asleep = self.options.act == "live" && desk::idle_seconds() > ASLEEP_AFTER;
            if self.hidden || self.asleep {
                // Nothing is drawn: the windows keep their last picture, and this is all the app does.
                self.next = now + Duration::from_secs_f32(LOOK_EVERY);
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.next));
                return;
            }
        }

        self.since_surfaces += dt;
        if self.since_surfaces >= SURFACES_EVERY && desk::changed() {
            self.since_surfaces = 0.0;
            self.surfaces = desk::surfaces();
            for shown in &mut self.shown {
                shown.actor.resettle(&self.surfaces);
            }
        }

        self.clock += dt;
        if self.gpu.is_none() {
            // No mons yet: nothing to draw, only the tracker and the server to hear from.
            self.next = now + Duration::from_secs(1);
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next));
            return;
        }
        let mut moving = false;
        // Now and then the mons say how they find what you are doing: pleased with work, unimpressed otherwise.
        if self.clock >= self.mood_at {
            self.mood_at = self.clock + MOOD_EVERY;
            let own: Vec<String> = self.shown.iter().filter(|shown| shown.wild.is_none() && !shown.id.is_empty()).map(|shown| shown.id.clone()).collect();
            let note = if self.fonts.as_ref().is_some_and(|fonts| fonts.has('\u{266a}')) { "\u{266a}" } else { "!" };
            match self.tracker.now().and_then(|(_, class)| class) {
                // One of them, a different one each time, hums along.
                Some(track::Class::Productive) if !self.untracked && !own.is_empty() => {
                    let which = &own[(self.clock / MOOD_EVERY) as usize % own.len()];
                    self.drop_over(which, note, [150, 232, 150]);
                }
                // All of them stop and look at you.
                Some(track::Class::Distracting) if !self.untracked => {
                    for id in &own {
                        self.drop_over(id, "...", [200, 200, 208]);
                    }
                    for shown in self.shown.iter_mut().filter(|shown| shown.wild.is_none()) {
                        if matches!(shown.actor.state, State::Idle(_) | State::Walk(_)) {
                            shown.actor.state = State::Idle(6.0);
                        }
                    }
                }
                _ => {}
            }
        }
        // What is in front has been a distraction for a while: the mons turn from it, fading and stopping where
        // they are, sooner in a focus session; they are back a moment after it ends.
        let distracted = !self.untracked && matches!(self.tracker.now().and_then(|(_, class)| class), Some(track::Class::Distracting));
        let in_session = self.state["session"].is_object();
        self.distracted_for = if distracted { self.distracted_for + dt } else { 0.0 };
        let toward = if self.distracted_for > if in_session { 5.0 } else { 15.0 } { 1.0 } else { 0.0 };
        if toward != self.drift {
            let step = if toward > self.drift { dt / 10.0 } else { dt / 1.2 };
            self.drift += (toward - self.drift).clamp(-step, step);
            moving = true;
        }
        for shown in self.shown.iter_mut().filter(|shown| shown.wild.is_none()) {
            shown.dim = self.drift;
            // Turned away, or settled in beside you for a session, it stays where it is.
            if (self.drift > 0.5 || in_session) && matches!(shown.actor.state, State::Walk(_)) {
                shown.actor.state = State::Idle(5.0);
            }
        }
        // A session's time is up: what is left of it is reported first, since that report's answer says what
        // came of it; the state is asked for a little after, in case there was nothing to report.
        if self.session_due.is_some_and(|at| at <= now) {
            self.session_due = None;
            self.since_report = self.options.report_every;
            self.session_check = Some(now + Duration::from_secs(8));
        }
        if self.session_check.is_some_and(|at| at <= now) {
            self.session_check = None;
            if let Some(asks) = &self.asks {
                let _ = asks.send(net::Ask::Refresh);
            }
        }
        // Two of them standing together notice each other now and then: one calls, the other answers.
        if self.clock >= self.notice_at {
            self.notice_at = self.clock + 25.0;
            let standing: Vec<(String, f32, f32, f32)> = self.shown.iter().filter(|shown| shown.wild.is_none() && !shown.id.is_empty() && matches!(shown.actor.state, State::Idle(_))).map(|shown| (shown.id.clone(), shown.actor.x, shown.actor.y, shown.size as f32)).collect();
            let pair = standing.iter().enumerate().find_map(|(index, a)| standing[index + 1..].iter().find(|b| (a.1 - b.1).abs() < (a.3 + b.3) * 0.8 && (a.2 - b.2).abs() < 4.0).map(|b| (a.0.clone(), b.0.clone())));
            if let (Some((caller, answerer)), false) = (pair, self.drift > 0.5) {
                let note = if self.fonts.as_ref().is_some_and(|fonts| fonts.has('\u{266a}')) { "\u{266a}" } else { "!" };
                self.cheer(&caller);
                self.drop_over(&answerer, note, [255, 214, 92]);
            }
        }
        let ball = self.ball().unwrap_or("ball");
        let gpu = self.gpu.as_ref().unwrap();
        let mut caught = false;
        // The wild mons whose ball is to be put down beside them again.
        let mut back: Vec<String> = Vec::new();
        let floor = self.surfaces.iter().filter(|s| s.owner == 0).map(|s| s.y).max().unwrap_or(0) as f32;
        for shown in &mut self.shown {
            if let Some((wild, ..)) = &mut shown.wild {
                match wild.step(dt) {
                    Some(Done::Caught) => caught = true,
                    // Out again, and cross about it.
                    Some(done) => {
                        shown.once = shown.model.mon.clip("roar").or(shown.model.mon.clip("attack")).map(|clip| (clip, 0.0));
                        // Still about: the ball is back beside it to be flung again.
                        if done == Done::BrokeOut {
                            back.push(shown.id.clone());
                        }
                    }
                    None => {}
                }
                shown.actor.leaving = wild.phase == Phase::Leaving && shown.once.is_none();
                // With a ball out at it, it stands where it is.
                if wild.busy() && matches!(shown.actor.state, State::Idle(_) | State::Walk(_)) {
                    shown.actor.state = State::Idle(1.0);
                }
                moving = true;
            }
            shown.actor.step(dt, &self.surfaces);
            // The ball that lies by a wild mon: put down once the mon has landed, flown when flung, gone with the mon.
            if shown.wild.as_ref().is_some_and(|(wild, ..)| wild.phase == Phase::Leaving) {
                shown.flung = None;
            }
            if shown.flung.as_ref().is_some_and(|flung| !flung.down) && !matches!(shown.actor.state, State::Fall(_) | State::Carried) {
                shown.put_ball_down(&self.surfaces);
            }
            if back.contains(&shown.id) {
                shown.put_ball_down(&self.surfaces);
            }
            let mark = shown.mark();
            let ground = shown.actor.y;
            if let Some(flung) = &mut shown.flung {
                if flung.down {
                    if flung.toss.step(dt, ground - flung.size as f32 / 2.0, mark) {
                        // It struck: the mon goes into it, in the mon's own window, and the server is asked what came of it.
                        flung.target.window.set_visible(false);
                        if let Some((wild, ..)) = &mut shown.wild {
                            if wild.about() {
                                wild.struck();
                                if let Some(asks) = &self.asks {
                                    let _ = asks.send(net::Ask::Throw(wild.encounter.clone(), ball.to_string()));
                                }
                            }
                        }
                    }
                    if flung.toss.shown() {
                        flung.place();
                        flung.draw(gpu);
                    }
                    moving |= flung.toss.moving();
                }
            }
            if let Some((clip, time)) = shown.once {
                let time = time + dt;
                shown.once = (time < shown.model.mon.header.clips[clip].duration).then_some((clip, time));
            }
            moving |= shown.actor.moving() || shown.once.is_some();
            // A fade wants the quicker frames while it lasts.
            // Drops that have floated away are let go; while any floats, the quicker frames.
            let clock = self.clock;
            shown.drops.retain(|drop| clock - drop.from < DROP_LASTS);
            moving |= !shown.drops.is_empty();
            let left = shown.plate_until - self.clock;
            moving |= shown.plate.is_some() && !shown.over && left > 0.0 && left < PLATE_FADES;
            shown.place();
            shown.draw(gpu, self.clock);
            self.drawn += 1;
        }
        // The caught one's window goes; the server says, a moment on, whether it has come to the desk. One that left
        // has fallen off the bottom of the screen.
        self.shown.retain(|shown| match &shown.wild {
            Some((wild, ..)) => !(matches!(wild.phase, Phase::Kept(_)) && caught && wild.step_done()) && shown.actor.y < floor + shown.size as f32 * 2.0,
            None => true,
        });
        if caught {
            if let Some(asks) = &self.asks {
                let _ = asks.send(net::Ask::Refresh);
            }
            self.sound(sound::Sound::Caught);
            self.welcome(event_loop);
        }
        moving |= self.tend_strip(event_loop);
        self.spent += now.elapsed();
        let step = Duration::from_secs_f64(1.0 / if moving { MOVING_FPS } else { STANDING_FPS });
        self.next = (self.next + step).max(now);
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next));
    }
}

fn main() {
    // Our signed-feed layer must approve cached packages again before applying them.
    velopack::VelopackApp::build().set_auto_apply_on_startup(false).run();
    let options = options();
    if let Some(path) = options.snap.clone() {
        return snap(&options, &path);
    }
    let mut builder = EventLoop::<Poke>::with_user_event();
    #[cfg(target_os="macos")]
    { use winit::platform::macos::{EventLoopBuilderExtMacOS,ActivationPolicy}; builder.with_activation_policy(ActivationPolicy::Accessory); }
    let event_loop = builder.build().expect("an event loop");
    let now = Instant::now();
    let baker = bake::Baker::find(&options.models).map(Arc::new);
    let update_proxy=event_loop.create_proxy();
    let updater=updates::Updater::start(move ||{let _=update_proxy.send_event(Poke::Told);});
    let mut app = App {
        options,
        gpu: None,
        shown: Vec::new(),
        surfaces: Vec::new(),
        began: now,
        last: now,
        next: now,
        clock: 0.0,
        since_surfaces: 0.0,
        since_look: 0.0,
        asleep: false,
        hidden: false,
        drawn: 0,
        spent: Duration::ZERO,
        tracker: Tracker::default(),
        tracked_at: 0,
        since_report: 0.0,
        heartbeat_at: None,
        server_seen: None,
        server_away: false,
        excluded: false,
        asks: None,
        told: None,
        added: 0,
        started: false,
        fonts: label::Fonts::load(),
        tray: None,
        proxy: event_loop.create_proxy(),
        paused: false,
        untracked: false,
        on_taskbar: false,
        on_board: false,
        board: None,
        board_at: None,
        board_moved: None,
        strip: None,
        private: Default::default(),
        social: serde_json::Value::Null,
        theirs: Default::default(),
        ranks: Default::default(),
        // For tests, which cannot work: `DESK_EARNING` is taken for what the last report earned.
        earning: std::env::var("DESK_EARNING").ok().and_then(|said| serde_json::from_str(&said).ok()).unwrap_or_default(),
        reported_in: None,
        newly: None,
        welcomed: false,
        idle: 0.0,
        distracted_for: 0.0,
        drift: 0.0,
        sounds: true,
        session_said: None,
        session_due: None,
        session_check: None,
        break_said: false,
        notice_at: 20.0,
        mood_at: MOOD_EVERY,
        points: 0,
        better: (0, 0),
        panel: None,
        state: serde_json::Value::Null,
        account: false,
        account_changing: false,
        updater,
        ending: None,
        exit_mode: None,
        note: String::new(),
        balls: 0,
        ball: Rc::new(Mon::ball()),
        listed: false,
        baker,
        baking: Default::default(),
        unbaked: Default::default(),
        home: desk::data_home(),
    };
    event_loop.run_app(&mut app).expect("the event loop");
}

#[cfg(test)]
mod presence_tests {
    use super::*;
    #[test]
    fn paused_and_excluded_presence_never_discloses_the_program() {
        for (paused,excluded) in [(true,false),(false,true)] {
            let status=presence(paused,false,excluded,Some("private.exe"),Some(track::Class::Productive));
            assert!(status["program"].is_null() && status["class"].is_null());
        }
        assert_eq!(presence(false,false,false,Some("code.exe"),Some(track::Class::Productive))["class"],"productive");
    }
}
