//! AppKit/ApplicationServices live behind a small Objective-C bridge. No browser contents, screenshots or keys are read.
use super::{Rect, Surface};
use std::{
    ffi::{c_char, c_void, CStr, CString},
    path::Path,
    sync::{
        atomic::{AtomicU32, Ordering},
        OnceLock,
    },
    time::Instant,
};
use winit::{
    platform::macos::WindowAttributesExtMacOS,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{Window, WindowAttributes},
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NativeWindow {
    id: isize,
    bounds: Rect,
}
extern "C" {
    fn pm_front_bundle(out: *mut c_char, capacity: usize) -> bool;
    fn pm_browser_title(out: *mut c_char, capacity: usize) -> bool;
    fn pm_local_time(out: *mut i32);
    fn pm_idle_seconds() -> f64;
    fn pm_cursor(x: *mut f64, y: *mut f64);
    fn pm_screens(out: *mut Rect, capacity: usize) -> usize;
    fn pm_windows(out: *mut NativeWindow, capacity: usize) -> usize;
    fn pm_busy() -> bool;
    fn pm_window_for_view(view: *mut c_void) -> isize;
    fn pm_window_bounds(window: isize, out: *mut Rect) -> bool;
    fn pm_prepare_overlay(window: isize);
    fn pm_move_window(window: isize, x: i32, y: i32, on_top: bool);
    fn pm_open_url(url: *const c_char);
    fn pm_copy_text(text: *const c_char) -> bool;
    fn pm_accessibility_trusted() -> bool;
    fn pm_request_accessibility();
    fn pm_app_name(bundle: *const c_char, out: *mut c_char, capacity: usize) -> bool;
    fn pm_app_icon(bundle: *const c_char, pixels: *mut u8, side: u32) -> bool;
    fn pm_play_wav(bytes: *const u8, length: usize, index: usize);
}

fn text(read: impl FnOnce(*mut c_char, usize) -> bool) -> Option<String> {
    let mut bytes = [0i8; 1024];
    read(bytes.as_mut_ptr(), bytes.len())
        .then(|| {
            unsafe { CStr::from_ptr(bytes.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        })
        .filter(|text| !text.is_empty())
}
pub fn ticks() -> u32 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u32
}
pub fn local_day() -> String {
    let mut parts = [0; 4];
    unsafe { pm_local_time(parts.as_mut_ptr()) };
    format!("{:04}-{:02}-{:02}", parts[0], parts[1], parts[2])
}
pub fn local_hour() -> u32 {
    let mut parts = [0; 4];
    unsafe { pm_local_time(parts.as_mut_ptr()) };
    parts[3].clamp(0, 23) as u32
}
pub fn front_program() -> Option<String> {
    text(|p, n| unsafe { pm_front_bundle(p, n) }).map(|bundle| super::identity::app_key(&bundle))
}
pub fn front_title() -> Option<String> {
    text(|p, n| unsafe { pm_browser_title(p, n) })
}
pub fn watch() {} // Two-second foreground/title polling is driven by the shared tracker.
                  // The shared tracker polls every two seconds. It applies privacy exclusions before asking for a browser title.
pub fn look_again() {}
pub fn front_changed() -> Option<u32> {
    Some(ticks())
}
pub fn changed() -> bool {
    static LAST: AtomicU32 = AtomicU32::new(u32::MAX - 1000);
    let now = ticks();
    if now.wrapping_sub(LAST.load(Ordering::Relaxed)) >= 250 {
        LAST.store(now, Ordering::Relaxed);
        true
    } else {
        false
    }
}
pub fn idle_seconds() -> f32 {
    let seconds = unsafe { pm_idle_seconds() };
    if seconds.is_finite() {
        seconds.max(0.0) as f32
    } else {
        f32::MAX
    }
}
pub fn cursor() -> (i32, i32) {
    let (mut x, mut y) = (0.0, 0.0);
    unsafe { pm_cursor(&mut x, &mut y) };
    (x.round() as i32, y.round() as i32)
}
pub fn screens() -> Vec<Rect> {
    let mut screens = vec![Rect::default(); 32];
    let count = unsafe { pm_screens(screens.as_mut_ptr(), screens.len()) };
    screens.truncate(count.min(screens.len()));
    screens
}
pub fn surfaces() -> Vec<Surface> {
    let mut windows = vec![NativeWindow::default(); 512];
    let count = unsafe { pm_windows(windows.as_mut_ptr(), windows.len()) };
    windows.truncate(count.min(windows.len()));
    let windows: Vec<_> = windows
        .into_iter()
        .map(|window| (window.id, window.bounds))
        .collect();
    super::surfaces_of(&windows, &screens())
}
pub fn busy() -> bool {
    unsafe { pm_busy() }
}
pub fn overlay_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes
        .with_has_shadow(false)
        .with_accepts_first_mouse(true)
}
pub fn native_handle(window: &Window) -> isize {
    match window.window_handle().map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::AppKit(handle)) => unsafe {
            pm_window_for_view(handle.ns_view.as_ptr())
        },
        _ => 0,
    }
}
pub fn window_bounds(window: &Window) -> Option<Rect> {
    let mut bounds = Rect::default();
    unsafe { pm_window_bounds(native_handle(window), &mut bounds) }.then_some(bounds)
}
pub fn position(window: &Window, x: i32, y: i32) {
    move_to(native_handle(window), x, y);
}
pub fn never_active(window: isize) {
    unsafe { pm_prepare_overlay(window) }
}
pub fn move_to(window: isize, x: i32, y: i32) {
    unsafe { pm_move_window(window, x, y, false) }
}
pub fn put_on_top(window: isize, x: i32, y: i32) {
    unsafe { pm_move_window(window, x, y, true) }
}
pub fn supports_taskbar() -> bool {
    false
}
pub fn clock_corner() -> Option<([i32; 4], [i32; 4])> {
    None
}
pub fn open(url: &str) {
    if url.starts_with("https://") || url.starts_with("http://") {
        if let Ok(url) = CString::new(url) {
            unsafe { pm_open_url(url.as_ptr()) }
        }
    }
}
pub fn copy(words: &str) -> bool {
    CString::new(words).is_ok_and(|text| unsafe { pm_copy_text(text.as_ptr()) })
}
pub fn accessibility_needed() -> bool {
    unsafe { !pm_accessibility_trusted() }
}
pub fn request_accessibility() {
    unsafe { pm_request_accessibility() }
}
pub fn app_name(key: &str) -> Option<String> {
    let bundle = CString::new(super::identity::bundle_id(key)?).ok()?;
    text(|out, n| unsafe { pm_app_name(bundle.as_ptr(), out, n) })
}
pub fn app_icon(key: &str) -> Option<image::RgbaImage> {
    let bundle = CString::new(super::identity::bundle_id(key)?).ok()?;
    let mut pixels = vec![0u8; 32 * 32 * 4];
    if !unsafe { pm_app_icon(bundle.as_ptr(), pixels.as_mut_ptr(), 32) } {
        return None;
    }
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha > 0 && alpha < 255 {
            for c in &mut pixel[..3] {
                *c = ((*c as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(32, 32, pixels)
}
pub fn play_wav(bytes: &[u8], index: usize) {
    unsafe { pm_play_wav(bytes.as_ptr(), bytes.len(), index) }
}
pub const STARTUP_LABEL: &str = "Start at login";
fn startup_path() -> Option<std::path::PathBuf> {
    Some(
        std::path::PathBuf::from(std::env::var_os("HOME")?)
            .join("Library/LaunchAgents/com.productivitymon.desktop.plist"),
    )
}
pub fn startup_enabled() -> bool {
    startup_path().is_some_and(|path| path.is_file())
}
pub fn set_startup(enabled: bool, executable: &Path, models: &Path, server: &str) -> bool {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let Some(path) = startup_path() else {
        return false;
    };
    if !enabled {
        return match std::fs::remove_file(path) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
    }
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let temp = path.with_extension("plist.tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(super::login::launch_agent(executable, models, server).as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(temp, path)?;
        Ok(())
    };
    write().is_ok()
}
