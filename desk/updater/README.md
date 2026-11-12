# Desktop automatic updates

The new installer uses Velopack 1.2.161 on Windows x64 and Apple Silicon macOS 12+. The app checks at launch,
then every six hours. Downloads run on a background thread. A small “App update ready / Restart” notice appears
above the normal panel; Preferences and the tray menu also offer updates. Quitting applies a ready update
without reopening. If the app was interrupted, an already-downloaded update finishes at the next launch only
after rechecking the signed feed and package hash. Network errors never prevent using the installed version.

## Migration

Alpha.8 and the old Mac ZIP cannot update themselves. Quit the old app, install the new Windows setup or
replace the Mac app in Applications once, then launch it. Windows now installs under Velopack's per-user
ProductivityMon directory; the legacy Programs/DeskMons files may remain, but use the new Start menu entry.
The account, private exclusions, settings and outboxes remain in their existing profile directories. Existing
start-at-login entries are refreshed to the new path when the new app starts. Development copies do not
contact the update feed or install updates.

## Trust and data safety

- Production trusts only `public-key.hex`, compiled into the executable. The private Ed25519 key is kept
  outside this checkout and in the repository's `PRODUCTIVITYMON_UPDATE_SIGNING_KEY` Actions secret.
- The signed payload binds app ID, platform channel, expiry and all package SHA-256 hashes/sizes. Unsigned
  feeds, wrong keys/channels, unsafe paths, expired metadata and altered packages are rejected. No unsigned
  fallback exists. Velopack's default unconditional cached-package startup application is disabled.
- Metadata is valid for 180 days. Publish a new release or re-sign and upload the current feed before it
  expires. Never change the pinned key without a deliberate trust-key rotation migration.
- Newer versions only; `win-alpha` and `osx-alpha` remain separate. Release files are versioned and immutable.
  Publishing uploads both platforms' binaries before replacing the signed feeds.
- Before quit/restart, the UI stops tracking and sends a FIFO save barrier to the account connection thread.
  It acknowledges only after all queued and partial activity has been saved locally and synced to disk.
  A save failure blocks closing and preserves the unsaved queue. Updates never touch the profile directory.
- A cached package is hashed again immediately before the helper starts. A fresh signed-feed check also
  rejects withdrawn updates. Download failures leave the current installation running.
- No account token, activity title or profile information is sent to GitHub. It serves public release files.

## Building and publishing

Run **Desktop updates** (`.github/workflows/desktop-updates.yml`) on `codex/desktop-auto-updates` with a new
increasing `0.1.0-alpha.N` version and `publish=true`. The workflow tests both native platforms, builds the
desktop and an isolated updater probe, performs a real v1 install → unsigned-update rejection → signed v2
download/apply/relaunch, then packages the real app and verified assets. The workflow seeds the last verified
full package to retain the previous release in the feed, and signs only after recomputing every package hash.
Delta delivery is disabled: the reconstructed ZIP container can differ from the published full archive,
and the desktop client deliberately requires the exact signed archive size and SHA-256. The release signer
also rejects delta entries so this cannot silently regress. Updates currently download the full package.

Files are hosted in the public GitHub release tag `desktop-alpha`. The app fetches
`releases.win-alpha.signed.json` or `releases.osx-alpha.signed.json`. The versioned downloads JSON contains
the website's download URLs, file sizes and checksums. Publishing new builds does not change the website's
static download links automatically; update the website manifests and download links using that metadata.

The asset source is the checksum-pinned complete alpha.mac.2 ZIP. New species/model changes require an
intentional update to this source; packaging does not quietly download an unverified asset bundle.

Local checks:

```powershell
cargo test --locked --all-targets --all-features --manifest-path desk/app/Cargo.toml
python -m unittest discover -s desk/updater -p test_*.py
```

`native_smoke.py` is for disposable CI runners only: it installs the test app under the real package ID.
Never run it against a user's normal profile or installation.

## OS publisher signing

Ed25519 update-feed signing is mandatory and independent of OS publisher signatures. Windows Authenticode
and Apple Developer ID/notarization credentials have not been provisioned. Builds remain unsigned and the
Mac alpha remains experimental. `package.py` supports `PRODUCTIVITYMON_WINDOWS_SIGN_PARAMS`,
`PRODUCTIVITYMON_APPLE_SIGN_IDENTITY` and `PRODUCTIVITYMON_APPLE_NOTARY_PROFILE` on a build host with the
corresponding certificates/profile installed; keep these secrets out of source and logs. Do not mark the
website's `signed` or `notarized` fields true until these steps actually run and verify successfully.
