//! What the app asks Windows about the desktop: where a mon can stand, where the mouse is, whether anyone is at
//! the keyboard, and whether something full-screen is running. Nothing here is asked every frame.

use super::{surfaces_of, Rect, Surface};
use std::path::Path;
use winit::{
    platform::windows::WindowAttributesExtWindows,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{Window, WindowAttributes},
};

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetTickCount};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::Shell::{
    SHQueryUserNotificationState, ShellExecuteW, QUNS_BUSY, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetCursorPos, GetForegroundWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOCOPYBITS, SWP_NOREDRAW, SWP_NOSENDCHANGING, SWP_NOSIZE, SWP_NOZORDER, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};

/// Set when a window anywhere has moved, shown, hidden or come to the front: the surfaces are then read again.
static CHANGED: AtomicBool = AtomicBool::new(true);

/// Whether the desktop's windows have changed since this was last asked.
pub fn changed() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

/// When another window last came to the front, as Windows' tick count in milliseconds, and whether one has since
/// that was last asked.
static FRONT_AT: AtomicU32 = AtomicU32::new(0);
static FRONT_CHANGED: AtomicBool = AtomicBool::new(true);

/// The date here, as `YYYY-MM-DD`, by the clock in the corner: the day a report's time is counted to.
pub fn local_day() -> String {
    unsafe {
        let mut time = std::mem::zeroed();
        GetLocalTime(&mut time);
        format!("{:04}-{:02}-{:02}", time.wYear, time.wMonth, time.wDay)
    }
}

/// The hour it is here, 0 to 23, by the clock in the corner.
pub fn local_hour() -> u32 {
    unsafe {
        let mut time = std::mem::zeroed();
        GetLocalTime(&mut time);
        time.wHour as u32
    }
}

/// Windows' own clock, in milliseconds: what `front_changed` and the tracker's times are told by. It wraps
/// every 49 days, so only differences of it mean anything (`wrapping_sub`).
pub fn ticks() -> u32 {
    unsafe { GetTickCount() }
}

/// Has the window in front looked at again as if it had just come there: for when what can be asked about it
/// has changed (the server turns out to say what pages are).
pub fn look_again() {
    FRONT_AT.store(ticks(), Ordering::Relaxed);
    FRONT_CHANGED.store(true, Ordering::Relaxed);
}

/// When another window came to the front, if one has since this was last asked.
pub fn front_changed() -> Option<u32> {
    FRONT_CHANGED
        .swap(false, Ordering::Relaxed)
        .then(|| FRONT_AT.load(Ordering::Relaxed))
}

/// The program whose window is in front, by its path, if there is one and Windows will say.
pub fn front_program() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut process = 0;
        GetWindowThreadProcessId(hwnd, &mut process);
        // The app's own panel in front is time in no program: it is not work, nor anything to sort.
        if process == GetCurrentProcessId() {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process);
        if handle.is_null() {
            return None;
        }
        let mut path = [0u16; 1024];
        let mut length = path.len() as u32;
        let found = QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut length) != 0;
        CloseHandle(handle);
        found.then(|| String::from_utf16_lossy(&path[..length as usize]))
    }
}

/// The title of the window in front, if it has one. Only ever asked of a browser's, whose title is its page.
pub fn front_title() -> Option<String> {
    let mut title = [0u16; 512];
    let length = unsafe {
        GetWindowTextW(
            GetForegroundWindow(),
            title.as_mut_ptr(),
            title.len() as i32,
        )
    };
    (length > 0).then(|| String::from_utf16_lossy(&title[..length as usize]))
}

unsafe extern "system" fn on_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    // 3: another window is in front. 0x800C: a window's title has changed, which for the one in front is a
    // browser gone to another page. The tracker wants to know when.
    if event == 3 || (event == 0x800C && object == 0 && child == 0 && hwnd == GetForegroundWindow())
    {
        FRONT_AT.store(GetTickCount(), Ordering::Relaxed);
        FRONT_CHANGED.store(true, Ordering::Relaxed);
    }
    // Only whole windows: the mouse pointer and every control in every window report here too. And not a new
    // title, which moves nothing a mon stands on.
    if object != 0 || child != 0 || event == 0x800C {
        return;
    }
    // And not the mons' own, which move every frame a mon walks and are nothing to stand on.
    let mut process = 0;
    GetWindowThreadProcessId(hwnd, &mut process);
    if process != GetCurrentProcessId() {
        CHANGED.store(true, Ordering::Relaxed);
    }
}

/// Has Windows say when windows change, on this thread, while it reads its messages.
pub fn watch() {
    const OUT_OF_CONTEXT: u32 = 0;
    unsafe {
        // Front window, moves and sizes begun and ended, minimised and restored.
        SetWinEventHook(
            0x0003,
            0x0017,
            std::ptr::null_mut(),
            Some(on_event),
            0,
            0,
            OUT_OF_CONTEXT,
        );
        // Destroyed, shown, hidden, reordered, moved or sized, and renamed.
        SetWinEventHook(
            0x8001,
            0x800C,
            std::ptr::null_mut(),
            Some(on_event),
            0,
            0,
            OUT_OF_CONTEXT,
        );
    }
}

/// Keeps a window from ever taking the keyboard: a click on a mon must not pull focus from the editor under it.
pub fn never_active(hwnd: isize) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, style | WS_EX_NOACTIVATE as isize);
    }
}

/// The taskbar of the main screen and, in it, the part by the clock where programs keep their icons: each as
/// left, top, right, bottom. None while there is no taskbar to be found.
pub fn clock_corner() -> Option<([i32; 4], [i32; 4])> {
    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    unsafe {
        let taskbar = FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
        if taskbar.is_null() {
            return None;
        }
        let corner = FindWindowExW(
            taskbar,
            std::ptr::null_mut(),
            wide("TrayNotifyWnd").as_ptr(),
            std::ptr::null(),
        );
        let (mut bar, mut tray): (RECT, RECT) = (std::mem::zeroed(), std::mem::zeroed());
        if GetWindowRect(taskbar, &mut bar) == 0
            || corner.is_null()
            || GetWindowRect(corner, &mut tray) == 0
        {
            return None;
        }
        Some((
            [bar.left, bar.top, bar.right, bar.bottom],
            [tray.left, tray.top, tray.right, tray.bottom],
        ))
    }
}

/// Puts a window somewhere and over every other, the taskbar too, which takes the top back whenever it is used.
pub fn put_on_top(hwnd: isize, x: i32, y: i32) {
    unsafe {
        SetWindowPos(
            hwnd as HWND,
            HWND_TOPMOST,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOREDRAW | SWP_NOCOPYBITS | SWP_NOSENDCHANGING,
        )
    };
}

/// Moves a window and nothing else: no repaint asked for, no messages about it sent round.
pub fn move_to(hwnd: isize, x: i32, y: i32) {
    unsafe {
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            x,
            y,
            0,
            0,
            SWP_NOSIZE
                | SWP_NOZORDER
                | SWP_NOACTIVATE
                | SWP_NOREDRAW
                | SWP_NOCOPYBITS
                | SWP_NOSENDCHANGING,
        )
    };
}

/// Where Windows keeps what this user's programs start with it, and the app's name there.
pub const STARTUP: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
pub const STARTUP_NAME: &str = "DeskMons";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// A line of text kept in the user's registry, if it is there.
pub fn kept(key: &str, name: &str) -> Option<String> {
    let mut text = [0u16; 2048];
    let mut bytes = (text.len() * 2) as u32;
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(key).as_ptr(),
            wide(name).as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            text.as_mut_ptr() as *mut _,
            &mut bytes,
        )
    };
    (found == 0).then(|| String::from_utf16_lossy(&text[..(bytes as usize / 2).saturating_sub(1)]))
}

/// Keeps a line of text in the user's registry, or takes it out. Whether it worked.
pub fn keep(key: &str, name: &str, text: Option<&str>) -> bool {
    unsafe {
        match text {
            Some(text) => {
                let text = wide(text);
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    wide(key).as_ptr(),
                    wide(name).as_ptr(),
                    REG_SZ,
                    text.as_ptr() as *const _,
                    (text.len() * 2) as u32,
                ) == 0
            }
            None => {
                RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(key).as_ptr(), wide(name).as_ptr()) == 0
            }
        }
    }
}

/// Opens a web address in the user's browser.
pub fn open(url: &str) {
    // Only ever an address: this is not a way to run whatever a server names.
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide(url).as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
}

pub fn cursor() -> (i32, i32) {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point) };
    (point.x, point.y)
}

/// How long since the keyboard or mouse was last touched, in seconds.
pub fn idle_seconds() -> f32 {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe {
        if GetLastInputInfo(&mut info) == 0 {
            return 0.0;
        }
        GetTickCount().wrapping_sub(info.dwTime) as f32 / 1000.0
    }
}

/// Whether a game, a film or a presentation has the whole screen: no time for mons.
pub fn busy() -> bool {
    let mut state = 0;
    unsafe {
        SHQueryUserNotificationState(&mut state) == 0
            && (state == QUNS_BUSY
                || state == QUNS_RUNNING_D3D_FULL_SCREEN
                || state == QUNS_PRESENTATION_MODE)
    }
}

/// The part of each screen that is not taskbar.
pub fn screens() -> Vec<Rect> {
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, found: LPARAM) -> BOOL {
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) != 0 {
            (*(found as *mut Vec<RECT>)).push(info.rcWork);
        }
        1
    }
    let mut found: Vec<RECT> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(each),
            &mut found as *mut _ as LPARAM,
        )
    };
    found.into_iter().map(rect).collect()
}

/// The windows on the desktop that a mon could stand on, from the front one back: where each is, and its handle.
fn windows() -> Vec<(isize, Rect)> {
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
        let found = &mut *(found as *mut Vec<(isize, RECT)>);
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return 1;
        }
        let mut process = 0;
        GetWindowThreadProcessId(hwnd, &mut process);
        // Not the mons' own windows, nor tool windows, nor what Windows keeps but does not show (other desktops).
        if process == GetCurrentProcessId()
            || GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0
        {
            return 1;
        }
        let mut cloaked = 0u32;
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut _ as *mut _,
            4,
        );
        if cloaked != 0 {
            return 1;
        }
        // The desktop itself and the taskbar are windows too.
        let mut class = [0u16; 64];
        let length = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..length]);
        if matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return 1;
        }
        let mut rect: RECT = std::mem::zeroed();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut rect as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        ) == 0
        {
            found.push((hwnd as isize, rect));
        }
        1
    }
    let mut found: Vec<(isize, RECT)> = Vec::new();
    unsafe { EnumWindows(Some(each), &mut found as *mut _ as LPARAM) };
    found
        .into_iter()
        .map(|(id, bounds)| (id, rect(bounds)))
        .collect()
}

/// Everything a mon can stand on now.
pub fn surfaces() -> Vec<Surface> {
    surfaces_of(&windows(), &screens())
}

/// Puts words on the clipboard, to be pasted somewhere else. Whether it could.
pub fn copy(words: &str) -> bool {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    // CF_UNICODETEXT: sixteen-bit letters with a zero after them.
    const UNICODE_TEXT: u32 = 13;
    let wide: Vec<u16> = words.encode_utf16().chain([0]).collect();
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let held = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        let put = !held.is_null() && {
            let at = GlobalLock(held) as *mut u16;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), at, wide.len());
            GlobalUnlock(held);
            // The clipboard keeps what it is given: it is not this program's to free any more.
            !SetClipboardData(UNICODE_TEXT, held as _).is_null()
        };
        CloseClipboard();
        put
    }
}

fn rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}
pub const STARTUP_LABEL: &str = "Start with Windows";
pub fn supports_taskbar() -> bool {
    true
}
pub fn startup_enabled() -> bool {
    kept(STARTUP, STARTUP_NAME).is_some()
}
pub fn set_startup(enabled: bool, executable: &Path, models: &Path, server: &str) -> bool {
    let plain = |path: &Path| {
        path.display()
            .to_string()
            .trim_start_matches(r"\\?\")
            .to_string()
    };
    let line = format!(
        "\"{}\" --models \"{}\" --server \"{}\"",
        plain(executable),
        plain(models),
        server
    );
    keep(STARTUP, STARTUP_NAME, enabled.then_some(line.as_str()))
}
pub fn overlay_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes
        .with_skip_taskbar(true)
        .with_no_redirection_bitmap(true)
}
pub fn native_handle(window: &Window) -> isize {
    match window.window_handle().map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get(),
        _ => 0,
    }
}
pub fn accessibility_needed() -> bool {
    false
}
pub fn request_accessibility() {}
pub fn window_bounds(window: &Window) -> Option<Rect> {
    let at = window.outer_position().ok()?;
    let size = window.outer_size();
    Some(Rect {
        left: at.x,
        top: at.y,
        right: at.x + size.width as i32,
        bottom: at.y + size.height as i32,
    })
}
pub fn position(window: &Window, x: i32, y: i32) {
    window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_line_kept_in_the_registry_is_read_back_and_taken_out() {
        // A key of the test's own, never the one Windows starts programs from.
        let key = "Software\\DeskMonsTest";
        assert_eq!(kept(key, "Line"), None);
        assert!(keep(
            key,
            "Line",
            Some(r#""C:\Apps\desk-mons.exe" --models "C:\Apps\models""#)
        ));
        assert_eq!(
            kept(key, "Line").as_deref(),
            Some(r#""C:\Apps\desk-mons.exe" --models "C:\Apps\models""#)
        );
        assert!(keep(key, "Line", None));
        assert_eq!(kept(key, "Line"), None);
        unsafe {
            windows_sys::Win32::System::Registry::RegDeleteKeyW(
                HKEY_CURRENT_USER,
                wide(key).as_ptr(),
            )
        };
    }
}
