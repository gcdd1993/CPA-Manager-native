# Changelog

All notable changes to CPA Manager Native are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/gcdd1993/CPA-Manager-native/compare/v0.0.2...HEAD
[0.0.2]: https://github.com/gcdd1993/CPA-Manager-native/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/gcdd1993/CPA-Manager-native/releases/tag/v0.0.1
