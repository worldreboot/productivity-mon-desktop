//! Local Windows app icons and a small, explicit directory of public website favicons.
//! All disk/OS/HTTP work stays off the UI thread. Unknown and private sites use a globe;
//! window titles and local executable paths are never sent to an icon service.
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

const MAX_BYTES: usize = 256 * 1024;
const SIZE: u32 = 32;
type Pixels = image::RgbaImage;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Kind {
    App,
    Site,
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Key {
    kind: Kind,
    name: String,
}
struct Entry {
    texture: Option<egui::TextureHandle>,
    display_name: Option<String>,
    tried: Instant,
}

pub struct Icons {
    entries: HashMap<Key, Entry>,
    pending: HashSet<Key>,
    apps: Option<mpsc::SyncSender<Key>>,
    sites: Option<mpsc::SyncSender<Key>>,
    results: mpsc::Receiver<(Key, Option<Pixels>, Option<String>)>,
    enabled: bool,
}

impl Icons {
    pub fn new(folder: PathBuf) -> Self {
        let (results_tx, results) = mpsc::channel();
        let start = |kind: Kind| {
            let (tx, rx) = mpsc::sync_channel::<Key>(32);
            let folder = folder.clone();
            let results = results_tx.clone();
            std::thread::Builder::new()
                .name(format!("activity-icons-{kind:?}"))
                .spawn(move || {
                    let agent = ureq::AgentBuilder::new()
                        .timeout(Duration::from_secs(5))
                        .redirects(0)
                        .user_agent("ProductivityMon/0.1 (favicon)")
                        .build();
                    while let Ok(key) = rx.recv() {
                        let pixels = load(&folder, &key, &agent);
                        let name = if key.kind == Kind::App {
                            load_app_name(&folder, &key)
                        } else {
                            None
                        };
                        if results.send((key, pixels, name)).is_err() {
                            break;
                        }
                    }
                })
                .expect("icon worker");
            tx
        };
        Self {
            apps: Some(start(Kind::App)),
            sites: Some(start(Kind::Site)),
            results,
            entries: HashMap::new(),
            pending: HashSet::new(),
            enabled: true,
        }
    }

    /// Screenshot fixtures stay offline and show the same neutral fallback shapes.
    #[cfg(feature = "website-capture")]
    pub fn offline() -> Self {
        let (_, results) = mpsc::channel();
        Self {
            apps: None,
            sites: None,
            results,
            entries: HashMap::new(),
            pending: HashSet::new(),
            enabled: false,
        }
    }

    pub fn update(&mut self, ctx: &egui::Context, enabled: bool) {
        self.enabled = enabled;
        while let Ok((key, pixels, display_name)) = self.results.try_recv() {
            self.pending.remove(&key);
            let texture = pixels.map(|pixels| {
                ctx.load_texture(
                    format!("activity-{:?}-{}", key.kind, key.name),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [SIZE as usize, SIZE as usize],
                        pixels.as_raw(),
                    ),
                    egui::TextureOptions::LINEAR,
                )
            });
            self.entries.insert(
                key,
                Entry {
                    texture,
                    display_name,
                    tried: Instant::now(),
                },
            );
        }
        if !self.pending.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// A display label only. Reports, classification rules and privacy actions keep the original key.
    pub fn app_name(&self, exe: &str) -> String {
        let key = Key {
            kind: Kind::App,
            name: exe.trim().to_lowercase(),
        };
        common_app_name(&key.name)
            .map(str::to_string)
            .or_else(|| {
                self.entries
                    .get(&key)
                    .and_then(|entry| entry.display_name.clone())
            })
            .unwrap_or_else(|| fallback_app_name(exe))
    }

    pub fn show(&mut self, ui: &mut egui::Ui, kind: Kind, name: &str, private: bool) {
        let key = Key {
            kind,
            name: name.trim().to_lowercase(),
        };
        if !private {
            if let Some(texture) = self
                .entries
                .get(&key)
                .and_then(|entry| entry.texture.as_ref())
            {
                ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(20.0, 20.0)));
                return;
            }
            let retry = if kind == Kind::App {
                Duration::from_secs(60)
            } else {
                Duration::from_secs(86400)
            };
            let known_site =
                kind != Kind::Site || own_site(&key.name) || favicon_url(&key.name).is_some();
            if self.enabled
                && known_site
                && key.name.len() <= 260
                && self.pending.len() < 64
                && !self.pending.contains(&key)
                && self
                    .entries
                    .get(&key)
                    .is_none_or(|entry| entry.tried.elapsed() > retry)
            {
                let sender = if kind == Kind::App {
                    &self.apps
                } else {
                    &self.sites
                };
                if sender
                    .as_ref()
                    .is_some_and(|sender| sender.try_send(key.clone()).is_ok())
                {
                    self.pending.insert(key);
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        fallback(ui, kind);
    }
}

fn common_app_name(exe: &str) -> Option<&'static str> {
    Some(match exe {
        "safari" => "Safari",
        "terminal" => "Terminal",
        "iterm2" => "iTerm2",
        "finder" => "Finder",
        "xcode" => "Xcode",
        "textedit" => "TextEdit",
        "code.exe" => "Visual Studio Code",
        "code - insiders.exe" => "Visual Studio Code Insiders",
        "cursor.exe" => "Cursor",
        "windsurf.exe" => "Windsurf",
        "chrome.exe" => "Google Chrome",
        "msedge.exe" => "Microsoft Edge",
        "firefox.exe" => "Mozilla Firefox",
        "brave.exe" => "Brave",
        "opera.exe" => "Opera",
        "discord.exe" | "discordcanary.exe" | "discordptb.exe" => "Discord",
        "spotify.exe" => "Spotify",
        "explorer.exe" => "File Explorer",
        "shellexperiencehost.exe" => "Windows Shell",
        "startmenuexperiencehost.exe" => "Windows Start",
        "searchhost.exe" | "searchapp.exe" => "Windows Search",
        "applicationframehost.exe" => "Windows App Host",
        "systemsettings.exe" => "Windows Settings",
        "winword.exe" | "word.exe" => "Microsoft Word",
        "excel.exe" => "Microsoft Excel",
        "powerpnt.exe" => "Microsoft PowerPoint",
        "outlook.exe" | "olk.exe" => "Microsoft Outlook",
        "onenote.exe" => "Microsoft OneNote",
        "teams.exe" | "ms-teams.exe" => "Microsoft Teams",
        "notepad.exe" => "Notepad",
        "notepad++.exe" => "Notepad++",
        "windowsterminal.exe" => "Windows Terminal",
        "powershell.exe" | "pwsh.exe" => "PowerShell",
        "cmd.exe" => "Command Prompt",
        "chatgpt.exe" => "ChatGPT",
        "codex.exe" => "Codex",
        "claude.exe" => "Claude",
        "slack.exe" => "Slack",
        "notion.exe" => "Notion",
        "figma.exe" => "Figma",
        "obsidian.exe" => "Obsidian",
        "steam.exe" => "Steam",
        "blender.exe" => "Blender",
        "desk-mons.exe" => "ProductivityMon",
        _ => return None,
    })
}

fn fallback_app_name(exe: &str) -> String {
    let trimmed = exe.trim();
    let stem = if trimmed.to_ascii_lowercase().ends_with(".exe") {
        &trimmed[..trimmed.len() - 4]
    } else {
        trimmed
    };
    let label = stem
        .replace(['_', '-'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    if label.is_empty() {
        "Unknown app".into()
    } else {
        label
    }
}

fn clean_app_name(name: &str) -> Option<String> {
    let name = name.trim_matches('\0').trim();
    (!name.is_empty()
        && name.chars().count() <= 100
        && !name.chars().any(char::is_control)
        && !name.to_ascii_lowercase().ends_with(".exe")
        && !name.contains(['/', '\\']))
    .then(|| name.to_string())
}

fn load_app_name(folder: &Path, key: &Key) -> Option<String> {
    if let Some(name) = common_app_name(&key.name) {
        return Some(name.into());
    }
    let file = cache_path(folder, key).with_extension("name.json");
    if file.metadata().ok().is_some_and(|meta| {
        meta.len() <= 1024
            && meta
                .modified()
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age < Duration::from_secs(7 * 86400))
    }) {
        if let Some(name) = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<String>(&bytes).ok())
            .and_then(|name| clean_app_name(&name))
        {
            return Some(name);
        }
    }
    let name = native::app_name(&key.name)?;
    if std::fs::create_dir_all(folder).is_ok() {
        if let Ok(bytes) = serde_json::to_vec(&name) {
            let _ = std::fs::write(file, bytes);
        }
    }
    Some(name)
}

fn fallback(ui: &mut egui::Ui, kind: Kind) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
    let p = ui.painter();
    let stroke = egui::Stroke::new(1.2, crate::look::FAINT);
    if kind == Kind::Site {
        let center = rect.center();
        p.circle_stroke(center, 7.5, stroke);
        p.add(egui::epaint::EllipseShape::stroke(
            center,
            egui::vec2(3.5, 7.5),
            stroke,
        ));
        p.line_segment(
            [center - egui::vec2(7.5, 0.0), center + egui::vec2(7.5, 0.0)],
            stroke,
        );
    } else {
        let r = rect.shrink(2.0);
        p.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
        p.line_segment(
            [
                r.left_top() + egui::vec2(0.0, 4.0),
                r.right_top() + egui::vec2(0.0, 4.0),
            ],
            stroke,
        );
    }
}

/// The tracker observes title suffixes, not browser URLs. Never turn an arbitrary title
/// into a network address. These aliases resolve only to fixed, public HTTPS endpoints.
fn favicon_url(name: &str) -> Option<&'static str> {
    Some(match name {
        "youtube" | "youtube.com" => "https://www.youtube.com/favicon.ico",
        "github" | "github.com" => "https://github.com/favicon.ico",
        "google" | "google search" | "google.com" => "https://www.google.com/favicon.ico",
        "gmail" | "gmail.com" => "https://ssl.gstatic.com/ui/v1/icons/mail/rfr/gmail.ico",
        "google docs" => "https://ssl.gstatic.com/docs/documents/images/kix-favicon7.ico",
        "google sheets" => "https://ssl.gstatic.com/docs/spreadsheets/favicon3.ico",
        "google slides" => "https://ssl.gstatic.com/docs/presentations/images/favicon5.ico",
        "google drive" => "https://ssl.gstatic.com/docs/doclist/images/drive_2022q3_32dp.png",
        "discord" | "discord.com" => "https://discord.com/assets/favicon.ico",
        "notion" | "notion.so" => "https://www.notion.so/images/favicon.ico",
        "stack overflow" | "stackoverflow.com" => {
            "https://cdn.sstatic.net/Sites/stackoverflow/Img/favicon.ico"
        }
        "reddit" | "reddit.com" => {
            "https://www.redditstatic.com/desktop2x/img/favicon/favicon-32x32.png"
        }
        "wikipedia" | "wikipedia.org" => "https://en.wikipedia.org/static/favicon/wikipedia.ico",
        "twitch" | "twitch.tv" => {
            "https://static.twitchcdn.net/assets/favicon-32-e29e246c157142c94346.png"
        }
        "x" | "twitter" | "x.com" | "twitter.com" => "https://abs.twimg.com/favicons/twitter.3.ico",
        "figma" | "figma.com" => "https://static.figma.com/app/icon/1/favicon.ico",
        "linear" | "linear.app" => "https://linear.app/favicon.ico",
        "chatgpt" | "chatgpt.com" => "https://cdn.oaistatic.com/assets/favicon-32x32-p4ktpm1x.webp",
        "slack" | "slack.com" => "https://a.slack-edge.com/80588/marketing/img/meta/favicon-32.png",
        _ => return None,
    })
}

fn own_site(name: &str) -> bool {
    matches!(name, "productivitymon" | "productivitymon.com")
}

fn cache_path(folder: &Path, key: &Key) -> PathBuf {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hash);
    folder.join(format!("{:016x}.png", hash.finish()))
}

fn decode(bytes: &[u8]) -> Option<Pixels> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(8 * 1024 * 1024);
    reader.limits(limits);
    Some(
        reader
            .decode()
            .ok()?
            .resize_exact(SIZE, SIZE, image::imageops::FilterType::Lanczos3)
            .into_rgba8(),
    )
}

fn load(folder: &Path, key: &Key, agent: &ureq::Agent) -> Option<Pixels> {
    let file = cache_path(folder, key);
    if file.metadata().ok().is_some_and(|meta| {
        meta.len() <= MAX_BYTES as u64
            && meta
                .modified()
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age < Duration::from_secs(7 * 86400))
    }) {
        if let Some(pixels) = std::fs::read(&file).ok().and_then(|bytes| decode(&bytes)) {
            return Some(pixels);
        }
    }
    let pixels = match key.kind {
        Kind::App => native::app_icon(&key.name),
        Kind::Site if own_site(&key.name) => {
            decode(include_bytes!("../assets/productivitymon-32.png"))
        }
        Kind::Site => {
            let response = agent.get(favicon_url(&key.name)?).call().ok()?;
            let mut bytes = Vec::new();
            response
                .into_reader()
                .take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .ok()?;
            decode(&bytes)
        }
    }?;
    if std::fs::create_dir_all(folder).is_ok() {
        // One worker per kind writes each key; filenames contain no user input.
        let _ = pixels.save_with_format(&file, image::ImageFormat::Png);
    }
    Some(pixels)
}

#[cfg(target_os="windows")]
mod native {
    use super::*;
    use std::{
        ffi::c_void,
        mem::{size_of, zeroed},
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Graphics::Gdi::*,
        Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        System::{Diagnostics::ToolHelp::*, Registry::*, Threading::*},
        UI::{Shell::*, WindowsAndMessaging::*},
    };

    fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        s.as_ref().encode_wide().chain(Some(0)).collect()
    }

    fn executable(name: &str) -> Option<PathBuf> {
        if !name.ends_with(".exe") || name.contains(['/', '\\', ':']) {
            return None;
        }
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot != INVALID_HANDLE_VALUE {
                let mut entry: PROCESSENTRY32W = zeroed();
                entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
                let mut found = Process32FirstW(snapshot, &mut entry);
                while found != 0 {
                    let exe = String::from_utf16_lossy(
                        &entry.szExeFile[..entry
                            .szExeFile
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(entry.szExeFile.len())],
                    );
                    if exe.eq_ignore_ascii_case(name) {
                        let process =
                            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                        if !process.is_null() {
                            let mut path = vec![0u16; 32768];
                            let mut len = path.len() as u32;
                            let ok =
                                QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len);
                            CloseHandle(process);
                            if ok != 0 {
                                CloseHandle(snapshot);
                                return Some(PathBuf::from(String::from_utf16_lossy(
                                    &path[..len as usize],
                                )));
                            }
                        }
                    }
                    found = Process32NextW(snapshot, &mut entry);
                }
                CloseHandle(snapshot);
            }
            let subkey = wide(format!(
                "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{name}"
            ));
            for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
                let mut path = vec![0u16; 32768];
                let mut bytes = (path.len() * 2) as u32;
                if RegGetValueW(
                    root,
                    subkey.as_ptr(),
                    null(),
                    RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                    null_mut(),
                    path.as_mut_ptr().cast(),
                    &mut bytes,
                ) == 0
                {
                    let end = path.iter().position(|&c| c == 0).unwrap_or(path.len());
                    let path =
                        PathBuf::from(String::from_utf16_lossy(&path[..end]).trim_matches('"'));
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    pub(super) fn app_icon(name: &str) -> Option<Pixels> {
        file_icon(&executable(name)?)
    }

    pub(super) fn app_name(name: &str) -> Option<String> {
        file_name(&executable(name)?)
    }

    /// Use the installed executable's localized version resource, never its window title.
    fn file_name(path: &Path) -> Option<String> {
        unsafe {
            let path = wide(path);
            let size = GetFileVersionInfoSizeW(path.as_ptr(), null_mut());
            if size == 0 || size > 1024 * 1024 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            if GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) == 0 {
                return None;
            }
            let mut value: *mut c_void = null_mut();
            let mut len = 0;
            let mut languages = Vec::new();
            if VerQueryValueW(
                data.as_ptr().cast(),
                wide("\\VarFileInfo\\Translation").as_ptr(),
                &mut value,
                &mut len,
            ) != 0
                && !value.is_null()
                && len <= 1024
            {
                let words = std::slice::from_raw_parts(value.cast::<u16>(), len as usize / 2);
                languages.extend(words.chunks_exact(2).map(|words| (words[0], words[1])));
            }
            languages.extend([(0x0409, 0x04b0), (0x0409, 0x04e4)]);
            for field in ["FileDescription", "ProductName"] {
                for (language, codepage) in &languages {
                    let query = wide(format!(
                        "\\StringFileInfo\\{language:04x}{codepage:04x}\\{field}"
                    ));
                    value = null_mut();
                    len = 0;
                    if VerQueryValueW(data.as_ptr().cast(), query.as_ptr(), &mut value, &mut len)
                        != 0
                        && !value.is_null()
                        && len > 0
                        && len <= 256
                    {
                        let text = String::from_utf16_lossy(std::slice::from_raw_parts(
                            value.cast::<u16>(),
                            len as usize,
                        ));
                        if let Some(name) = clean_app_name(&text) {
                            return Some(name);
                        }
                    }
                }
            }
            None
        }
    }

    pub(super) fn file_icon(path: &Path) -> Option<Pixels> {
        unsafe {
            let mut info: SHFILEINFOW = zeroed();
            if SHGetFileInfoW(
                wide(path).as_ptr(),
                0,
                &mut info,
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            ) == 0
                || info.hIcon.is_null()
            {
                return None;
            }
            let black = draw(info.hIcon, 0);
            let white = draw(info.hIcon, 255);
            DestroyIcon(info.hIcon);
            let (black, white) = (black?, white?);
            let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
            for (b, w) in black.chunks_exact(4).zip(white.chunks_exact(4)) {
                let alpha = 255 - (0..3).map(|i| w[i].saturating_sub(b[i])).max().unwrap_or(0);
                for i in [2, 1, 0] {
                    rgba.push(if alpha == 0 {
                        0
                    } else {
                        ((b[i] as u32 * 255 + alpha as u32 / 2) / alpha as u32).min(255) as u8
                    });
                }
                rgba.push(alpha);
            }
            image::RgbaImage::from_raw(SIZE, SIZE, rgba)
        }
    }

    unsafe fn draw(icon: HICON, background: u8) -> Option<Vec<u8>> {
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return None;
        }
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = SIZE as i32;
        info.bmiHeader.biHeight = -(SIZE as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            return None;
        }
        let old = SelectObject(dc, bitmap);
        std::ptr::write_bytes(bits, background, (SIZE * SIZE * 4) as usize);
        let ok = DrawIconEx(
            dc,
            0,
            0,
            icon,
            SIZE as i32,
            SIZE as i32,
            0,
            null_mut(),
            DI_NORMAL,
        );
        GdiFlush();
        let result = if ok != 0 {
            Some(std::slice::from_raw_parts(bits.cast::<u8>(), (SIZE * SIZE * 4) as usize).to_vec())
        } else {
            None
        };
        SelectObject(dc, old);
        DeleteObject(bitmap);
        DeleteDC(dc);
        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn extracts_real_windows_executable_icon() {
            let pixels = file_icon(&std::env::current_exe().unwrap()).unwrap();
            assert_eq!(pixels.dimensions(), (SIZE, SIZE));
            assert!(pixels.pixels().any(|p| p.0[3] > 0));
            let name = std::env::current_exe()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            assert!(app_icon(&name).is_some());
            assert_eq!(
                file_name(&std::env::current_exe().unwrap()).as_deref(),
                Some("ProductivityMon desktop companion")
            );
            assert!(executable("../../bad.exe").is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn friendly_names_preserve_identity_and_handle_missing_metadata() {
        assert_eq!(common_app_name("code.exe"), Some("Visual Studio Code"));
        assert_eq!(common_app_name("chrome.exe"), Some("Google Chrome"));
        assert_eq!(common_app_name("winword.exe"), Some("Microsoft Word"));
        assert_eq!(common_app_name("explorer.exe"), Some("File Explorer"));
        assert_eq!(fallback_app_name(" my_custom-app.EXE "), "My Custom App");
        assert_eq!(fallback_app_name("éditor.exe"), "Éditor");
        assert_eq!(fallback_app_name(".exe"), "Unknown app");
        assert_eq!(clean_app_name("  My App\0"), Some("My App".into()));
        for invalid in [
            "",
            "app.exe",
            "line\nbreak",
            "C:\\private\\app",
            "file/path",
        ] {
            assert!(clean_app_name(invalid).is_none());
        }
    }
    #[test]
    fn only_known_public_sites_can_make_network_requests() {
        assert_eq!(
            favicon_url("github"),
            Some("https://github.com/favicon.ico")
        );
        assert_eq!(favicon_url("youtube.com"), favicon_url("youtube"));
        for name in [
            "localhost",
            "127.0.0.1",
            "169.254.169.254",
            "http://github.com",
            "github.com/private",
            "github.evil.com",
            "private project",
            "",
        ] {
            assert!(favicon_url(name).is_none());
        }
        assert_eq!(
            cache_path(
                Path::new("cache"),
                &Key {
                    kind: Kind::App,
                    name: "../../private.exe".into()
                }
            )
            .parent(),
            Some(Path::new("cache"))
        );
    }
    #[test]
    fn decoder_limits_and_transparency() {
        assert!(decode(&vec![0; MAX_BYTES + 1]).is_none());
        assert!(decode(b"not an icon").is_none());
        assert!(decode(include_bytes!("../assets/productivitymon.ico")).is_some());
        let mut bytes = Cursor::new(Vec::new());
        let source = image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 30, 40, 128]));
        source
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let pixels = decode(bytes.get_ref()).unwrap();
        assert_eq!(pixels.dimensions(), (SIZE, SIZE));
        assert_eq!(pixels.get_pixel(16, 16).0, [200, 30, 40, 128]);
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::new(1025, 1)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        assert!(decode(bytes.get_ref()).is_none());
    }

    #[test]
    #[ignore = "manual public HTTPS smoke test; ordinary tests stay offline"]
    fn public_favicons_decode_and_cache() {
        let folder =
            std::env::temp_dir().join(format!("productivitymon-icons-{}", uuid::Uuid::new_v4()));
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(8))
            .redirects(0)
            .user_agent("ProductivityMon/0.1 (favicon)")
            .build();
        for name in [
            "youtube",
            "github",
            "google",
            "gmail",
            "google docs",
            "google sheets",
            "google slides",
            "google drive",
            "discord",
            "notion",
            "stack overflow",
            "reddit",
            "wikipedia",
            "twitch",
            "x",
            "figma",
            "linear",
            "chatgpt",
            "slack",
        ] {
            let key = Key {
                kind: Kind::Site,
                name: name.into(),
            };
            let pixels =
                load(&folder, &key, &agent).unwrap_or_else(|| panic!("no favicon for {name}"));
            assert_eq!(pixels.dimensions(), (SIZE, SIZE));
            assert_eq!(
                std::fs::read(cache_path(&folder, &key))
                    .ok()
                    .and_then(|bytes| decode(&bytes)),
                Some(pixels)
            );
            std::fs::remove_file(cache_path(&folder, &key)).unwrap();
        }
        std::fs::remove_dir(folder).unwrap();
    }
}

#[cfg(target_os="macos")]
mod native {
    use super::*;
    pub(super) fn app_name(key: &str) -> Option<String> {crate::desk::app_name(key)}
    pub(super) fn app_icon(key: &str) -> Option<Pixels> {crate::desk::app_icon(key)}
}
