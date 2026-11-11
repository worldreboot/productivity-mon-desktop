# ProductivityMon Desktop

Desktop companions that earn XP from productive activity. Windows alpha, with Apple Silicon support in development.

The desktop app is in `desk/app/`. This repository contains the desktop source, bundled UI art, and the Pokédex data it embeds. Prepared runtime models and companion pictures are supplied when packaging.

## Apple Silicon

See [the macOS build notes](desk/macos/README.md). The `Apple Silicon desktop` Actions workflow runs native macOS tests and builds an arm64 executable. Development artifacts are separate from signed and notarized public releases.

## Windows

Run `cargo test --locked --manifest-path desk/app/Cargo.toml`, then `cargo build --locked --release --manifest-path desk/app/Cargo.toml`.

Start with `--models /path/to/models --icons /path/to/icons --server https://api.productivitymon.com`.
