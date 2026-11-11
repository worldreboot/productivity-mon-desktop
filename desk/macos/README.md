# Apple Silicon development port

This is the first macOS platform implementation, not a published Mac release. The shared Rust UI, tracker,
models, animations, accounts and server protocol are reused. Windows remains supported.

The OS boundary is `app/src/desk.rs`, with implementations under `app/src/desk/`. macOS uses a small
AppKit/ApplicationServices Objective-C bridge, compiled by the Cargo build script on a Mac. It provides:

- Foreground bundle IDs, mapped to existing shared app classifications where possible.
- Browser titles through Accessibility, read only after the shared tracker checks exclusions.
- Aggregate idle time, cursor position, screen work areas and public window bounds; no screen capture or key logging.
- Nonactivating transparent companion windows, native icons/names, clipboard, sound and browser opening.
- Start-at-login through a user LaunchAgent with separate XML-escaped arguments, without a shell.
- Private profile storage under `~/Library/Application Support/ProductivityMon`.

Browser tracking is optional. The panel explains when Accessibility access is missing. Its **Enable website
tracking** button asks macOS to present the permission prompt; permission is never requested automatically.
App detection and ordinary app tracking do not depend on browser-window access.

Rendering selects Metal on macOS and DirectX on Windows. Metal's straight-alpha surface is handled by a
final unpremultiplication pass; scene and UI blending remain premultiplied. Windows-only taskbar strips
are disabled on Mac; the menu-bar icon and the floating friends board remain available.

## Build and check on an Apple Silicon Mac

Install Xcode Command Line Tools and Rust, then run from the repository root:

```sh
rustup target add aarch64-apple-darwin
MACOSX_DEPLOYMENT_TARGET=12.0 cargo test --locked --manifest-path desk/app/Cargo.toml --target aarch64-apple-darwin
MACOSX_DEPLOYMENT_TARGET=12.0 cargo build --locked --release --manifest-path desk/app/Cargo.toml --target aarch64-apple-darwin
desk/app/target/aarch64-apple-darwin/release/desk-mons --models /path/to/prepared/models --icons /path/to/prepared/icons --server https://api.productivitymon.com
```

The `Apple Silicon desktop` GitHub Actions workflow runs the same target tests/build and uploads a development
executable. It checks the Mach-O architecture and framework links, then attempts a timed signed-out launch
with a temporary profile and a closed loopback server if the runner exposes Metal. The smoke report explicitly
records when graphics are unavailable; that does not establish interactive UI behavior. It does not publish,
request permissions, use signing credentials or include the local model assets.

To create a local unsigned app bundle with the already-prepared assets:

```sh
python3 desk/macos/package.py --executable desk/app/target/aarch64-apple-darwin/release/desk-mons --models /path/to/prepared/models --icons /path/to/prepared/icons --output /path/to/new-output-folder --archive
```

The packager checks the arm64 Mach-O header, includes the existing app mark as an ICNS icon, preserves
executable permissions in its ZIP, and refuses to replace an existing app or archive. Keep the bundle path
stable while testing Accessibility permissions. Packaged clients require their normal and shiny model assets;
the source-only baker needs the corresponding Node/Python tools and source caches.

## Native validation still required

Windows-host checks cannot compile/link the Objective-C bridge against AppKit or establish that it behaves
correctly on a Mac. Before a Mac alpha, run the workflow and test: permission denied/granted/revoked, Safari
and Chromium titles, private exclusions, first XP, account switching, nonactivating pet clicks, moving/perching
windows, Retina and mixed-scale displays, fullscreen apps/Spaces, sleep/wake, login startup, icons, sound,
transparent text edges and persistence. Signing, notarization and download publication are
separate release work; no Mac download should be advertised yet.

API references: [AppKit](https://developer.apple.com/documentation/appkit),
[Accessibility](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions),
[wgpu Metal surface implementation](https://docs.rs/wgpu-hal/latest/src/wgpu_hal/metal/surface.rs.html),
[macOS runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
