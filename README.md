# CPA Manager Native

CPA Manager Native is a Tauri desktop manager for running [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) and [CPA-Manager-Plus](https://github.com/seakee/CPA-Manager-Plus) as local managed binaries.

It handles release discovery, download verification, installation, startup, health checks, logs, and rollback from one desktop UI. The app does not replace CPA-Manager-Plus; it provides a stable native shell for installing and supervising the two upstream components.

## Features

- Install or update CPA Core and CPA-Manager-Plus from GitHub Releases.
- Verify release assets with upstream `checksums.txt` before activation.
- Start installed components automatically when the desktop app starts.
- Stop managed components automatically when the desktop app exits.
- Close-to-tray behavior: clicking the window close button hides the app; quit from the tray menu.
- Generate and persist a CLIProxyAPI remote-management secret when the upstream config has an empty key.
- Display the generated management key in the installed versions view.
- Open the CPA-Manager-Plus management page after health checks pass.
- Manual update checks only, avoiding GitHub API rate-limit noise from background polling.
- Roll back to the previous installed component version if a newly installed component fails startup health checks.
- Light and dark themes.

## Screenshots

The slots below are reserved for UI screenshots. Replace the placeholder SVG files in `assets/screenshots/` with current app screenshots, or update the image paths if you prefer PNG files.

| Main dashboard                                                                  | Installed versions |
|---------------------------------------------------------------------------------| --- |
| ![Main dashboard screenshot placeholder](assets/screenshots/main-dashboard.png) | ![Installed versions screenshot placeholder](assets/screenshots/installed-versions.png) |

| Health and logs | Tray menu |
| --- | --- |
| ![Health and logs screenshot placeholder](assets/screenshots/health-and-logs.png) | ![Tray menu screenshot placeholder](assets/screenshots/tray-menu.png) |

## Platform Support

CPA Manager Native targets Windows, macOS, and Linux.

| Platform | Release bundles |
| --- | --- |
| Windows x64 | NSIS installer |
| macOS Apple Silicon | `.app`, `.dmg` |
| macOS Intel | `.app`, `.dmg` |
| Linux x64 | `.deb`, `.rpm`, `.AppImage` |

Release assets are unsigned unless signing secrets are configured. On macOS, unsigned builds may require the user to approve the app in system security settings.

## Managed Components

| Component | Role | Default port |
| --- | --- | --- |
| CLIProxyAPI | Local API gateway and protocol adapter | `8317` |
| CPA-Manager-Plus | Web management, monitoring, and visualization UI | `18317` |

CPA-Manager-Plus is configured to connect to CLIProxyAPI at `http://127.0.0.1:8317`.

## Known Compatibility Notes

- CPA-Manager-Plus `v1.11.0` Windows amd64 is skipped because it exits during SQLite startup with `SQL logic error: out of memory (1)`. See [seakee/CPA-Manager-Plus#345](https://github.com/seakee/CPA-Manager-Plus/issues/345).
- The Linux AppImage is built on Ubuntu 22.04 with WebKitGTK 4.1 dependencies, following Tauri v2's Linux packaging requirements.

## Development

Requirements:

- Node.js 22
- Rust stable
- Platform-specific Tauri build dependencies

Install dependencies:

```bash
npm ci
```

Run the frontend dev server:

```bash
npm run dev
```

Run the Tauri app:

```bash
npm run tauri -- dev
```

Build the frontend:

```bash
npm run build
```

Run Rust checks:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

## Packaging

GitHub Actions builds release bundles when a `v*` tag is pushed or the release workflow is started manually.

Local packaging examples:

```bash
# Windows
npm run tauri -- build --bundles nsis

# macOS
npm run tauri -- build --bundles app,dmg

# Linux
npm run tauri -- build --bundles deb,rpm,appimage
```

Linux package builds require the WebKitGTK 4.1 stack and packaging tools:

```bash
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf xdg-utils libfuse2 rpm
```

## Release Process

1. Update `CHANGELOG.md`.
2. Ensure app versions match in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json`.
3. Run the verification commands.
4. Commit the release metadata.
5. Create and push an annotated tag:

```bash
git tag -a vX.Y.Z -m "CPA Manager Native vX.Y.Z"
git push origin master
git push origin vX.Y.Z
```

## Project Layout

```text
src/        React frontend
src-tauri/  Tauri/Rust backend, installer, process manager, release resolver
assets/     README screenshots and documentation assets
PRD/        Product notes and design references
.github/    CI, release workflow, and Dependabot configuration
```
