//! Shared desktop geometry and the OS integration boundary. All coordinates use desktop pixels.
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
// Keep the surface algorithm and its fixtures independent of the Windows ABI.
type RECT = Rect;

#[cfg(target_os = "windows")]
#[path = "desk/windows.rs"]
mod native;
#[cfg(target_os = "macos")]
#[path = "desk/macos.rs"]
mod native;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
compile_error!("The desktop client currently supports Windows and macOS.");
pub use native::*;

#[cfg(any(target_os = "macos", test))]
#[path = "desk/identity.rs"]
mod identity;
#[cfg(any(target_os = "macos", test))]
#[path = "desk/login.rs"]
mod login;

pub fn data_home() -> PathBuf {
    if let Some(path) = std::env::var_os("DESK_MONS_HOME") {
        return path.into();
    }
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("desk-mons")
    }
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join("Library/Application Support/ProductivityMon")
    }
}

pub fn prepare_home(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// A level stretch a mon can stand on: the top of a window, or the floor of a screen above its taskbar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub y: i32,
    pub x0: i32,
    pub x1: i32,
    /// The window whose top it is, or 0 for a screen's floor.
    pub owner: isize,
    /// That window's left edge, which a mon standing on it keeps its distance from as the window is moved.
    pub left: i32,
}

/// The surfaces of windows given from the front one back, and of screens: every screen's floor, and the top of
/// each window where no window in front of it covers the strip just above.
pub fn surfaces_of(windows: &[(isize, RECT)], screens: &[RECT]) -> Vec<Surface> {
    /// The narrowest top worth standing on, and how far above a top a window in front must reach to hide it.
    const NARROWEST: i32 = 120;
    const STRIP: i32 = 4;
    let mut found: Vec<Surface> = screens
        .iter()
        .map(|s| Surface {
            y: s.bottom,
            x0: s.left,
            x1: s.right,
            owner: 0,
            left: s.left,
        })
        .collect();
    for (index, (owner, rect)) in windows.iter().enumerate() {
        // On a screen, with room over it for a mon: not a window whose top is at the top of its screen.
        let Some(screen) = screens.iter().find(|s| {
            rect.top >= s.top + 80
                && rect.top < s.bottom
                && rect.right > s.left
                && rect.left < s.right
        }) else {
            continue;
        };
        let mut pieces = vec![(rect.left.max(screen.left), rect.right.min(screen.right))];
        for (_, front) in &windows[..index] {
            if front.top > rect.top - STRIP || front.bottom < rect.top {
                continue;
            }
            pieces = pieces
                .into_iter()
                .flat_map(|(a, b)| [(a, b.min(front.left)), (a.max(front.right), b)])
                .filter(|(a, b)| b > a)
                .collect();
        }
        found.extend(
            pieces
                .into_iter()
                .filter(|(a, b)| b - a >= NARROWEST)
                .map(|(x0, x1)| Surface {
                    y: rect.top,
                    x0,
                    x1,
                    owner: *owner,
                    left: rect.left,
                }),
        );
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: RECT = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };
    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn a_screen_s_floor_is_always_a_surface() {
        assert_eq!(
            surfaces_of(&[], &[SCREEN]),
            vec![Surface {
                y: 1040,
                x0: 0,
                x1: 1920,
                owner: 0,
                left: 0
            }]
        );
    }

    #[test]
    fn a_window_s_top_is_a_surface_and_a_maximised_one_s_is_not() {
        let found = surfaces_of(
            &[(7, rect(100, 300, 900, 800)), (8, rect(0, 0, 1920, 1040))],
            &[SCREEN],
        );
        assert_eq!(
            found[1..],
            [Surface {
                y: 300,
                x0: 100,
                x1: 900,
                owner: 7,
                left: 100
            }]
        );
    }

    #[test]
    fn a_window_in_front_cuts_the_top_of_the_one_behind_it() {
        // The front window covers the middle of the back one's top edge.
        let found = surfaces_of(
            &[(1, rect(400, 200, 600, 700)), (2, rect(100, 300, 900, 800))],
            &[SCREEN],
        );
        assert_eq!(
            found[1],
            Surface {
                y: 200,
                x0: 400,
                x1: 600,
                owner: 1,
                left: 400
            }
        );
        assert_eq!(
            found[2..],
            [
                Surface {
                    y: 300,
                    x0: 100,
                    x1: 400,
                    owner: 2,
                    left: 100
                },
                Surface {
                    y: 300,
                    x0: 600,
                    x1: 900,
                    owner: 2,
                    left: 100
                }
            ]
        );
    }

    #[test]
    fn a_window_in_front_that_starts_below_a_top_leaves_it_whole() {
        let found = surfaces_of(
            &[(1, rect(400, 500, 600, 700)), (2, rect(100, 300, 900, 800))],
            &[SCREEN],
        );
        assert!(found.contains(&Surface {
            y: 300,
            x0: 100,
            x1: 900,
            owner: 2,
            left: 100
        }));
    }
}
