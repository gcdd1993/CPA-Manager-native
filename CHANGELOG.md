# Changelog

All notable changes to CPA Manager Native are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.5] - 2026-08-09

### Added

- Managed-service startup now identifies and stops processes occupying the required local ports, waits for the ports to be released, and then starts the managed services automatically.

### Changed

- Application and component update downloads no longer use a request timeout, allowing them to complete on slow networks.
- Component startup failures now preserve the selected version and report the original error instead of automatically retrying with an older version.
- CPA Core now opens its management console at `/management.html`.

### Fixed

- Launch-at-startup synchronization no longer tries to delete a missing Windows startup entry.

## [0.0.4] - 2026-07-22

### Added

- Added a persistent LAN-access setting for CLIProxyAPI that updates both the bind host and remote-management access, with automatic restart when the service is running.
- Added single-instance application handling so repeated launches activate the existing window instead of starting duplicate manager and component processes.
- Added signed in-app updates for Windows using the latest GitHub Release updater manifest.

### Changed

- Windows NSIS application updates now run in in-place `/UPDATE` mode, preserving application data, shortcuts, and launch-at-startup settings without requiring a manual uninstall.
- The update check now downloads the application update before stopping managed components and restores their previous running state if installation preparation fails.

## [0.0.3] - 2026-07-14

### Added

- A dedicated WebDAV Sync tab for configuring a server, testing connectivity, and manually uploading or restoring configuration.
- Configuration-only backups covering CPA Manager Native settings, CLIProxyAPI `config.yaml`, and CPA-Manager-Plus `config.json`.
- Timestamped local backups before downloaded configuration is applied.

### Security

- WebDAV archives use an explicit three-file allowlist and reject executable, component-version, undeclared, duplicate, or path-traversal entries.
- WebDAV passwords are excluded from uploaded archives, while machine-local data-directory and connection settings are preserved during restore.
- Configuration downloads are size-limited, validated before local files are changed, and blocked while managed components are running or busy.

## [0.0.2] - 2026-07-14

### Added

- A system-aware theme preference alongside explicit light and dark modes.
- A launch-at-startup preference backed by the native operating-system integration.
- An application data-directory picker with migration support that preserves the source directory.
- A fixed manager configuration directory, separate from the movable component data directory.
- Simplified Chinese documentation with English remaining the default README.
- Product screenshots covering the dashboard, installed versions, health and logs, and tray menu.
- Release packaging for macOS app/DMG and Linux deb/rpm bundles.

### Changed

- Reworked the light theme with a muted warm palette and accessible semantic color tokens.
- Expanded the English README with setup, development, packaging, release, privacy, and troubleshooting guidance.
- Improved settings feedback, pending states, and accessibility announcements.

## [0.0.1] - 2026-07-13

### Added

- Initial CPA Manager Native desktop release for Windows.
- One-click install, update, start, stop, and health monitoring for CLIProxyAPI and CPA-Manager-Plus.
- Automatic startup and shutdown for installed CPA Core and Manager components.
- Tray-first close behavior, with explicit quit from the tray menu.
- Automatic CLIProxyAPI remote-management secret generation and display.
- Manual update checks only, avoiding GitHub API rate-limit noise from background polling.
- GitHub Actions release pipeline for Windows NSIS bundles.

### Fixed

- Skips CPA-Manager-Plus v1.11.0 on Windows because of upstream startup failure seakee/CPA-Manager-Plus#345.
- Falls back to the previous installed component version when a newly installed component cannot pass startup health checks.

[Unreleased]: https://github.com/gcdd1993/CPA-Manager-native/compare/v0.0.5...HEAD
[0.0.5]: https://github.com/gcdd1993/CPA-Manager-native/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/gcdd1993/CPA-Manager-native/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/gcdd1993/CPA-Manager-native/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/gcdd1993/CPA-Manager-native/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/gcdd1993/CPA-Manager-native/releases/tag/v0.0.1
