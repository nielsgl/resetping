# Changelog

All notable user-facing changes to this project are documented in this file.

Format rules:
- Keep entries under `Unreleased` until a release is cut.
- Use these sections only: `Added`, `Changed`, `Fixed`, `Security`.
- Write entries in user-facing language.
- Do not remove historical entries.

## [Unreleased]

## [0.1.4] - 2026-03-15

### Fixed
- In-app updater install now relies on the bundled updater public key from app config, preventing install failures caused by malformed build-time key overrides.
- Release workflow no longer injects `TAURI_UPDATER_PUBLIC_KEY` into build job environments, removing a key-format mismatch path.

## [0.1.3] - 2026-03-14

### Changed
- Packaged release builds now include configured Sentry DSN from GitHub Actions secret so telemetry works in installed app builds.

## [0.1.2] - 2026-03-14

### Added
- Settings screen now displays the running app version for quick verification during install/update testing.

### Changed
- Packaged release builds now support build-time Sentry DSN injection via release workflow secrets.

### Fixed
- Updater release pipeline now publishes `latest*.json` metadata artifacts required by in-app update checks.

## [0.1.1] - 2026-03-14

### Added
- Backend automated tests for smoke-critical poll and transition behavior, including notification policy routing and transition history cap checks.
- Release validation checklist for `v0.1.1` (`RELEASE_CHECKLIST.md`) to track manual gate evidence.

### Changed
- Installation guidance now explicitly documents free-mode Gatekeeper/quarantine handling and source-build workflow in the README.
- Release workflow verification gates now validate updater signatures without requiring `latest.json` artifacts.

### Fixed
- Release workflow now commits Homebrew cask updates on first creation (untracked-file case).
- Tauri updater public key handling in release path is aligned with expected base64 format.

### Added
- Tray-first desktop app that polls Codex reset status and sends native notifications on status changes.
- Settings window with polling controls, notification policy, endpoint override, diagnostics log copy, and launch-at-login toggle.
- In-app updater flow on macOS: manual/scheduled checks, update availability state, and install action.
- Daily GitHub release download snapshot automation for install-trend reporting.

### Changed
- Telemetry model split into independent toggles for crash/error telemetry and usage analytics.
- Added anonymous persistent installation identifier for coarse active-usage metrics.
- Added backend transport diagnostics and fallback path for resilient polling under specific macOS socket failures.

### Fixed
- Non-2xx upstream API responses now count as poll failures and correctly affect degraded health state.
- Diagnostics/transitions rendering now escapes content safely (no raw HTML interpolation).
- Settings update behavior is transactional around autostart/persistence failure paths.
- In-progress settings form edits are preserved during background state refresh events.

### Security
- Hardened diagnostics rendering against endpoint-controlled content injection.
