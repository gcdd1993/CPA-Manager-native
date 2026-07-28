<div align="center">

<img src="src-tauri/icons/icon.png" alt="CPA Manager Native" width="96" />

# CPA Manager Native

**A native desktop manager for CLIProxyAPI, CPA-Manager-Plus, and Octopus**

Install, update, run, monitor, and roll back local AI gateway components from one Tauri desktop app.

[![Tauri](https://img.shields.io/badge/Tauri-24C8DB?logo=tauri&logoColor=white)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![React](https://img.shields.io/badge/React-20232A?logo=react&logoColor=61DAFB)](https://react.dev/)
[![TypeScript](https://img.shields.io/badge/TypeScript-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![Release](https://img.shields.io/github/v/release/gcdd1993/CPA-Manager-native?include_prereleases&sort=semver&label=release&color=4c9a40)](https://github.com/gcdd1993/CPA-Manager-native/releases)

<sub>Keywords: CLIProxyAPI manager · CPA-Manager-Plus · Octopus · local binary manager · Tauri desktop app</sub>

**English** · [简体中文](README.zh-CN.md)

</div>

---

CPA Manager Native is a desktop control plane for running [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI), [CPA-Manager-Plus](https://github.com/seakee/CPA-Manager-Plus), and [Octopus](https://github.com/Hureru/octopus) as local managed binaries.

It does not replace CPA-Manager-Plus. It provides a stable native shell around the upstream components: release discovery, checksum verification, installation, startup, health checks, logs, data-directory management, and rollback are handled from one UI.

## Where it fits

- Users who want a one-click local launcher for CLIProxyAPI, CPA-Manager-Plus, or Octopus.
- Users who prefer a tray-first desktop app instead of manually managing terminal processes.
- Users who need safer upgrades with checksums, health checks, and rollback.
- Users who want the upstream CPA-Manager-Plus web UI opened only after the managed services are ready.

## Where it doesn't

- It is not a fork or replacement of CLIProxyAPI or CPA-Manager-Plus.
- It is not a hosted service or remote management platform.
- It does not run background GitHub polling for updates; update checks are manual to avoid rate-limit noise.

---

## Core features

#### Managed installation

- Discover latest GitHub Releases for CPA Core, CPA-Manager-Plus, and Octopus.
- Match platform-specific release assets for Windows, macOS, and Linux.
- Verify downloaded assets against upstream `checksums.txt` or GitHub's SHA-256 asset digest before activation.
- Keep recent installed versions so a failed startup can roll back to a previous version.

#### Local process control

- Start and stop each installed component from the desktop UI.
- Choose which installed components start automatically when CPA Manager Native launches.
- Configure each component's local service port; port conflicts are rejected and running components restart automatically after a port change.
- Stop managed components automatically when the desktop app exits.
- Check port availability before starting a managed service; when occupied, stop the listening process automatically and start the managed service after the port is released.

#### Health and recovery

- Run service health checks against the expected local ports.
- Open the CPA-Manager-Plus management page after health checks pass.
- Surface runtime logs, process IDs, component status, progress, and errors.
- Roll back after startup health-check failure when a previous version is available.

#### Configuration and secrets

- Store app settings in the user's `~/.cpamanager-native` directory.
- Use a configurable data directory for managed component binaries, manifests, logs, and config.
- Generate and persist a CLIProxyAPI remote-management secret when the upstream config has an empty key.
- Manually upload or restore the manager and managed-component configuration from the dedicated WebDAV Sync view. The allowlist excludes component binaries, databases, versions, logs, and download caches, and restore creates a local backup first.
- Display the recoverable management key in the installed versions view.
- Preserve existing hashed upstream secrets without exposing or rotating them.

#### Desktop experience

- Close to tray by default; quit explicitly from the tray menu.
- Optional launch-at-startup setting.
- Light and dark themes.
- Local-only runtime management with no telemetry in this project.

<div align="center">

| Main dashboard | Installed versions |
| --- | --- |
| ![Main dashboard](assets/screenshots/main-dashboard.png) | ![Installed versions](assets/screenshots/installed-versions.png) |

| Health and logs | Tray menu |
| --- | --- |
| ![Health and logs](assets/screenshots/health-and-logs.png) | ![Tray menu](assets/screenshots/tray-menu.png) |

</div>

---

## Installation

Download the installer for your platform from [Releases](../../releases).

| Platform | Release bundle |
| --- | --- |
| Windows x64 | NSIS installer |
| macOS Apple Silicon | `.app`, `.dmg` |
| macOS Intel | `.app`, `.dmg` |
| Linux x64 | `.deb`, `.rpm` |

Release assets are unsigned unless signing secrets are configured in the release pipeline. On macOS, unsigned builds may require approval in system security settings before first launch.

## Quick start

1. Install and open CPA Manager Native.
2. Choose or confirm the data directory used for managed component binaries and config.
3. Install the components you need from the dashboard.
4. Start them from the app.
5. Wait for health checks to pass, then open the desired management page.

CPA-Manager-Plus is configured to connect to CLIProxyAPI at `http://127.0.0.1:8317`.

## Managed components

| Component | Short name | Role | Default port |
| --- | --- | --- | --- |
| CLIProxyAPI | CPA Core | Local AI API gateway and protocol adapter | `8317` |
| CPA-Manager-Plus | CPAMP | Web management, monitoring, and visualization UI | `18317` |
| Octopus | Octopus | Personal LLM API aggregation and load balancing | `8080` |

## Platform support

| Runtime platform | Managed asset matching | App bundle |
| --- | --- | --- |
| Windows x64 | `windows_amd64.zip` | NSIS installer |
| macOS Apple Silicon | `darwin_aarch64` / `darwin_arm64` archives | `.app`, `.dmg` |
| macOS Intel | `darwin_amd64` archives | `.app`, `.dmg` |
| Linux x64 | `linux_amd64.tar.gz` | `.deb`, `.rpm` |

## Known compatibility notes

- CPA-Manager-Plus `v1.11.0` Windows amd64 is skipped because it exits during SQLite startup with `SQL logic error: out of memory (1)`. See [seakee/CPA-Manager-Plus#345](https://github.com/seakee/CPA-Manager-Plus/issues/345).
- Linux packages use Ubuntu 22.04 and WebKitGTK 4.1 dependencies, following Tauri v2 packaging requirements.

---

## Tech stack

- **Desktop shell**: Tauri 2
- **Backend**: Rust, Tokio, reqwest
- **Frontend**: React, TypeScript, Vite
- **Packaging**: Tauri bundler and GitHub Actions
- **Release integration**: GitHub Releases, SHA-256 checksum verification

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

The Windows build supports signed in-app, in-place updates. The top-bar update check covers both CPA Manager Native and its managed components. When a new app version is available, the signed NSIS updater runs in `/UPDATE` mode and preserves application data, shortcuts, and launch-at-startup settings without requiring a manual uninstall.

Local packaging examples:

```bash
# Windows
npm run tauri -- build --bundles nsis

# macOS
npm run tauri -- build --bundles app,dmg

# Linux
npm run tauri -- build --bundles deb,rpm
```

Linux package builds require the WebKitGTK 4.1 stack and packaging tools:

```bash
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf xdg-utils libfuse2 rpm
```

## Release process

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

Updater artifacts must be signed with a stable Tauri updater private key. Configure these GitHub Actions secrets in the release repository:

- `TAURI_SIGNING_PRIVATE_KEY`: the private key matching the public key in `tauri.conf.json`.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the private-key password; leave it empty for an unencrypted key.

Never commit the private key. Losing it prevents existing installations from validating future updates.

## Project layout

```text
src/        React frontend
src-tauri/  Tauri/Rust backend, installer, process manager, release resolver
assets/     README screenshots and documentation assets
PRD/        Product notes and design references
.github/    CI, release workflow, and Dependabot configuration
```

## Privacy and data

- CPA Manager Native stores its own settings under `~/.cpamanager-native`.
- Managed binaries, manifests, logs, and upstream component config live under the selected data directory.
- The app downloads release metadata and assets from GitHub Releases only when you install or check updates.
- This project does not include telemetry or a hosted backend.

## Acknowledgements

CPA Manager Native manages the upstream projects [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI), [CPA-Manager-Plus](https://github.com/seakee/CPA-Manager-Plus), and [Octopus](https://github.com/Hureru/octopus). Those projects are distributed under their own licenses and release processes.

## Friends

- [Linux DO](https://linux.do/) - A community for technology and open-source enthusiasts.
