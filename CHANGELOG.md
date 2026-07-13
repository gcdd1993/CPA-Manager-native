# Changelog

## v0.0.1 - 2026-07-13

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
