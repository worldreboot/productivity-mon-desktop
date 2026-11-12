//! The app's only interface of its own: an icon by the clock with a short menu. Everything else (the
//! collection, the lists, the shop) is the server's page, so the app carries no UI toolkit.

use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub struct Tray {
    icon: TrayIcon,
    /// Points and balls, as the server last said: a line to read, not to click.
    status: MenuItem,
    pub panel: MenuItem,
    pub page: MenuItem,
    pub community: MenuItem,
    pub hide: CheckMenuItem,
    pub track: CheckMenuItem,
    pub taskbar: CheckMenuItem,
    pub board: CheckMenuItem,
    pub sounds: CheckMenuItem,
    pub startup: CheckMenuItem,
    pub quit: MenuItem,
}

impl Tray {
    /// Puts the icon by the clock. `startup` is whether the app is now set to start with Windows.
    pub fn new(startup: bool, taskbar: bool, board: bool, sounds: bool) -> Result<Tray, String> {
        let status = MenuItem::new("Not signed in yet", false, None);
        let panel = MenuItem::new("Open ProductivityMon", true, None);
        let page = MenuItem::new("Open the web page", true, None);
        let community = MenuItem::new("Join our Discord", true, None);
        let hide = CheckMenuItem::new("Hide the mons", true, false, None);
        let track = CheckMenuItem::new("Pause tracking", true, false, None);
        let taskbar = CheckMenuItem::new("Names and levels on the taskbar", crate::desk::supports_taskbar(), taskbar, None);
        let board = CheckMenuItem::new("Friends' day in a small window", true, board, None);
        let sounds = CheckMenuItem::new("Sounds", true, sounds, None);
        let startup = CheckMenuItem::new(crate::desk::STARTUP_LABEL, true, startup, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append_items(&[&status, &PredefinedMenuItem::separator(), &panel, &page, &community, &hide, &taskbar, &board, &sounds, &track, &startup, &PredefinedMenuItem::separator(), &quit]).map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            // A left click opens the panel; the menu is the right click's.
            .with_menu_on_left_click(false)
            .with_tooltip("ProductivityMon")
            .with_icon(Icon::from_rgba(crate::app_icon::tray_pixels(), 32, 32).map_err(|e| e.to_string())?)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Tray { icon, status, panel, page, community, hide, track, taskbar, board, sounds, startup, quit })
    }

    /// Says what the account has, in the menu and when the mouse rests on the icon.
    pub fn say(&self, text: &str) {
        self.status.set_text(text);
        let _ = self.icon.set_tooltip(Some(format!("ProductivityMon: {text}")));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_icon_is_a_round_ball_red_over_white() {
        let pixels = crate::app_icon::tray_pixels();
        let at = |x: usize, y: usize| &pixels[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(pixels.len(), 32 * 32 * 4);
        // See-through in the corners, solid inside.
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(16, 8)[3], 255);
        // Red above the band, white below it, dark on it.
        assert!(at(16, 8)[0] > 190 && at(16, 8)[1] < 80);
        assert!(at(16, 24).iter().all(|c| *c > 200));
        assert!(at(6, 16)[0] < 60);
    }
}
