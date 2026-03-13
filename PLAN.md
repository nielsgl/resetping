# ResetPing v1 Plan + Delivery Ledger

## How to Use This File
- This is the single source of truth for what was planned, what shipped, and what is still open.
- Top sections are operational (`status`, `next actions`, `evidence`).
- The original plan is preserved verbatim in **Appendix A** so no information is lost.

## Plan Update Protocol (Mandatory)
- Every status-changing implementation PR must update this file in the same PR.
- The update is not optional. If status changed and this file was not updated, the PR is incomplete.
- `Appendix A` is immutable historical context. Do not delete it. Do not rewrite it. Only append below it if needed.
- Allowed status values are exactly: `DONE`, `PARTIAL`, `PENDING`, `DEFERRED`, `BLOCKED`.
- `Current Status Snapshot`, `Delivery Ledger`, and `Final Release Gate Checklist` must stay logically consistent with each other.
- Every status change must include an entry in `Plan Status Changelog` (below).
- For each `PENDING` or `BLOCKED` item, include one explicit next action in `Next Actions (Priority Order)`.

## Plan Status Changelog
- Entry format (single line, required fields in this exact order):
  - `YYYY-MM-DD | actor=<human-or-agent> | sections=<comma-separated-section-names> | change=<status transition(s)> | reason=<why this changed> | evidence=<path(s)>`
- Rules:
  - Use absolute dates.
  - Do not delete prior entries.
  - Newest entry goes at the top.
  - `evidence` must reference concrete repository paths when code/docs/workflow changed.

- 2026-03-13 | actor=codex | sections=Current Status Snapshot,Delivery Ledger,Final Release Gate Checklist | change=initialized living ledger from original static plan | reason=make execution status explicit without losing plan detail | evidence=/Users/niels.van.Galen.last/code/codex-reset-notifier/PLAN.md

## Current Status Snapshot
- Product target: `ResetPing` tray app for Codex reset status polling + notifications.
- Current implementation state: **core product complete**, **updater implementation complete**, **release/ops validation partially complete**.
- Delivery posture:
  - `DONE`: core polling/state machine/tray/settings/notifications/diagnostics/telemetry/install-metrics snapshot pipeline.
  - `PENDING`: release hardening and final manual QA gates.
  - `DEFERRED`: Homebrew cask follow-up (explicitly outside v1 ship gate).

## Delivery Ledger

### Workstream 1: Core App Runtime
- Status: `DONE`
- Includes:
  - Tauri tray-first runtime, state machine, persistence, transitions, degraded health behavior.
  - Polling scheduler with low-power cadence + jitter.
  - Notification policies (`flip`, `no_to_yes`) + initial-state notification.
  - Endpoint override and diagnostics log export.
- Evidence:
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/src-tauri/src/lib.rs`
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/src/main.ts`

### Workstream 2: Security/Correctness Hardening
- Status: `DONE`
- Includes:
  - Non-2xx upstream responses treated as poll failures (no false healthy state).
  - Safe diagnostics rendering (no raw `innerHTML` log interpolation).
  - Transactional settings behavior around autostart/persistence.
  - Form dirty-state protection against background state refresh overwrite.
- Evidence:
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/src-tauri/src/lib.rs`
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/src/main.ts`

### Workstream 3: Updater Implementation
- Status: `DONE` (implementation)
- Includes:
  - Real updater check flow (replaced v1 stub command).
  - Pending update state + explicit install action (`download_and_install`).
  - Scheduled update checks driven by settings.
  - UI install button shown only when update is ready.
  - Updater artifacts/signatures configured for release workflow.
- Evidence:
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/src-tauri/src/lib.rs`
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/src/main.ts`
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/src/utils/updater.ts`
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/src-tauri/tauri.conf.json`
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/.github/workflows/release.yml`

### Workstream 4: Telemetry + Install Analytics
- Status: `DONE`
- Includes:
  - Split telemetry toggles (`error_telemetry_enabled`, `usage_telemetry_enabled`).
  - Anonymous persistent `installation_id`.
  - Usage events (`app_open`, heartbeat, transition, update_check).
  - Frontend + backend Sentry integration.
  - Poll failure telemetry throttling to prevent event floods.
  - Daily GitHub release download snapshot pipeline.
- Evidence:
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/src-tauri/src/lib.rs`
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/src/main.ts`
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/docs/TELEMETRY_POLICY.md`
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/.github/workflows/daily-install-snapshot.yml`
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/scripts/snapshot-release-downloads.mjs`

### Workstream 5: Release + Distribution Operations
- Status: `PARTIAL`
- Done:
  - Cross-platform build/release workflows exist.
  - macOS signing/notarization steps and updater artifact checks exist in workflow.
- Pending to close v1 gate:
  - Confirm CI secrets/vars are configured in GitHub environment.
  - Run real signed/notarized release and verify Gatekeeper-friendly install path on device.
  - Run full updater E2E from published release.
- Evidence:
  - `/Users/niels.van.Galen.last/code/codex-reset-notifier/.github/workflows/release.yml`

## Deferred / Out of Scope (Explicit)
- Homebrew cask packaging follow-up.
- App-store distribution path.

## Final Release Gate Checklist
- `PENDING` Release pipeline executed successfully with real signing/notarization credentials.
- `PENDING` macOS signed install smoke test (fresh machine profile).
- `PENDING` Launch-at-login manual verification matrix (enable/disable + reboot).
- `PENDING` Updater E2E verification (`check -> install -> restart`) on published artifact.
- `PENDING` Cross-platform beta tray/notification parity checks (Windows/Linux).
- `DONE` Automated checks (`npm test/typecheck/build`, `cargo fmt/clippy/test`) on current branch.

## Next Actions (Priority Order)
1. Run one full release dry-run in GitHub Actions with real secrets and verify notarization + stapling output.
2. Validate updater E2E from published `latest.json` + signed artifacts on macOS.
3. Execute manual QA checklist and capture pass/fail evidence in `RELEASE_CHECKLIST.md`.
4. After gates pass, cut/tag v1 release and publish release notes.

---

## Appendix A: Original Plan (Verbatim, Preserved)

# Plan: ResetPing (Codex Reset Notifier) v1

## Summary
Build a Tauri-based desktop tray app (`ResetPing`) that polls `https://hascodexratelimitreset.today/api/status`, detects state transitions, and sends native notifications when status changes (with user-selectable policy). v1 is macOS GA with signed/notarized GitHub release installers and optional background update checks; Windows/Linux artifacts are published as beta. The app is privacy-first but includes minimal anonymous crash/error telemetry (Sentry), opt-out.

## Key Decisions and Tradeoffs
- Stack: Tauri + Rust backend + lightweight web UI.
  - Tradeoff: More setup complexity than pure Electron, but much smaller binaries, lower idle usage, and cleaner cross-platform path.
- Distribution: GitHub Releases first, with Tauri updater support.
  - Tradeoff: Fast OSS distribution and no store friction; app stores can be added later.
- macOS trust bar: CI-based code signing + notarization from day one.
  - Tradeoff: Higher CI secret/setup burden, but dramatically better install UX and fewer Gatekeeper issues.
- Polling source: single authoritative endpoint with advanced URL override.
  - Tradeoff: Minimal logic and lower maintenance vs no built-in multi-source redundancy.
- Notification policy: default “state flip” with user setting to “only no→yes”; always notify initial state.
  - Tradeoff: Better immediate visibility but intentional first-run noise.
- Reliability UX: silent retry; tray degraded warning after 5 consecutive failures.
  - Tradeoff: Avoids notification spam while still surfacing outages.
- Telemetry: minimal anonymous crash/error events via Sentry, opt-out.
  - Tradeoff: Better stability feedback with limited privacy impact.

## Implementation Changes

### 1) App Architecture
- Create a Tauri app with:
  - Rust core loop for polling, state machine, persistence, network/backoff.
  - Tray/menu UI for at-a-glance status, health, and quick actions.
  - Small settings window for configuration and diagnostics.
- Subsystems:
  - `status_client`: typed HTTP client for `/api/status` with timeout and response validation.
  - `poll_engine`: scheduler (normal + low power cadence), jitter, retry tracking.
  - `state_store`: durable local config and runtime snapshot/history.
  - `notify`: OS-native notification adapter + test notification action.
  - `updates`: optional Tauri updater check pipeline.
  - `diagnostics`: ring-buffer logs + copy/export support.
  - `telemetry`: Sentry init, crash/error capture, opt-out gate.

### 2) Public Interfaces / Types (Decision-Complete)
- Upstream status payload contract consumed:
  - `state: string`, `configured: boolean`, `updatedAt: number`, optional fields ignored.
- Normalized internal status type:
  - `effective_state: "yes" | "no"` where upstream `state !== "no"` maps to `"yes"` (mirrors site behavior).
  - `source_timestamp_ms: number | null` from `updatedAt`.
  - `fetched_at_ms: number`.
  - `configured: boolean`.
- Persistent settings schema:
  - `poll_interval_sec` (default `60`, bounds `30..900`).
  - `low_power_poll_interval_sec` (default `300`).
  - `http_timeout_ms` (default `8000`).
  - `notification_policy` (`"flip"` default, `"no_to_yes"` optional).
  - `notify_initial_state` (`true`).
  - `launch_at_login` (`false` default, user opt-in).
  - `update_checks_enabled` (`false` default).
  - `update_check_interval_hours` (`24`).
  - `status_endpoint_url` (default prod endpoint).
  - `telemetry_enabled` (`true`, user can disable).
- Persisted runtime snapshot:
  - `last_known_state`, `last_success_at`, `consecutive_failures`, `last_error_summary`.
- Transition history:
  - store last 100 transitions: `from`, `to`, `detected_at`, `source_updated_at`.

### 3) Polling + State Machine Behavior
- On startup:
  - immediate fetch attempt.
  - if success: persist state and send initial notification (per chosen behavior: always notify initial state).
- Continuous loop:
  - normal cadence at configured interval; low-power cadence auto-switches to 5m.
  - apply small jitter to avoid synchronized bursts.
- Transition detection:
  - compare normalized `effective_state` against persisted `last_known_state`.
  - notification routing:
    - `"flip"`: notify on any state change.
    - `"no_to_yes"`: notify only when transition target is `"yes"`.
- Failure handling:
  - increment `consecutive_failures`; no per-failure notifications.
  - show degraded tray status when failures reach threshold (5).
  - clear degraded status on first success.
- Notification content:
  - include previous and current state when applicable.
  - include source label and timestamp context.
- Tray/menu display:
  - current state, last checked time, and health indicator.
  - actions: open settings, force refresh, send test notification, quit.

### 4) UI/UX Scope (v1)
- Tray-first app (no persistent main window).
- Settings window tabs/sections:
  - General: launch at login, polling interval, low-power behavior.
  - Notifications: policy selector, initial-state toggle, test notification.
  - Updates: enable background checks, last check result.
  - Advanced: endpoint override, timeout.
  - Diagnostics: recent logs view + copy button; telemetry toggle.
- Keep controls intentionally minimal; no full poll-log UI, no heavy dashboard.

### 5) Packaging, Signing, and Release
- GitHub Actions release workflow:
  - Build macOS (GA), Windows/Linux (beta artifacts).
  - macOS signing with Developer ID cert from encrypted secrets.
  - Apple notarization via `notarytool`, then staple.
  - Publish installer artifacts to GitHub Releases.
- Updater:
  - Tauri updater metadata/signatures published alongside releases.
  - app setting controls background check (default off).
- Homebrew strategy:
  - not in v1 ship gate, but release asset naming and metadata structured for immediate follow-up cask work.
- OSS posture:
  - public MIT repo, issue/PR templates, basic SECURITY/contact docs.

### 6) Observability and Operations
- Diagnostics local ring buffer (recent events/errors/status changes).
- Sentry events limited to:
  - unhandled crashes.
  - explicit error events (network parsing failures, updater failures, startup failures).
- No feature analytics, no behavior tracking beyond minimal error telemetry.
- Network etiquette:
  - custom `User-Agent` and project contact URL in requests.

## Test Plan and Acceptance Scenarios

### Automated
- Unit tests:
  - status payload parsing + normalization (`state !== "no"` => `yes`).
  - transition detection and notification policy branching.
  - failure threshold/degraded-state transitions.
  - config validation (bounds/defaults).
  - history retention cap at 100 entries.
- Integration tests:
  - mocked endpoint sequence (`no -> yes -> yes -> no`) and expected notifications.
  - startup initial notification behavior.
  - timeout + retry counter behavior.
  - endpoint override and invalid URL handling.
- Platform CI:
  - Rust tests + frontend checks.
  - Tauri build verification on macOS/Windows/Linux.

### Manual QA
- macOS:
  - first-run permissions and notification delivery.
  - launch-at-login behavior.
  - tray rendering and degraded health indicator.
  - signed/notarized install path with no Gatekeeper workaround.
- Cross-platform beta:
  - tray + notification parity checks on Windows/Linux.
- Updater:
  - manual trigger and optional scheduled check behavior.
- Low power:
  - confirm cadence switch to 5m and return to normal on power mode exit.

### Release Gates
- Must pass:
  - all tests, build jobs, notarization, staple validation.
  - real-device notification smoke test on macOS.
  - endpoint outage simulation (degraded indicator appears at 5 failures, clears on recovery).

## Assumptions and Defaults
- Product name locked for v1: `ResetPing`.
- Bundle identifier will use personal namespace initially.
- Source of truth is `https://hascodexratelimitreset.today/api/status`; override is advanced-user only.
- Default polling `60s`; configurable `30s..15m`; low power uses `5m`.
- HTTP timeout `8s`.
- Background update checks disabled by default; if enabled, run every `24h`.
- Initial-state notification is enabled.
- v1 implementation target: macOS GA, Windows/Linux beta artifacts.
- PLAN.md write step will happen after leaving Plan Mode; this plan content is the canonical draft to copy verbatim into `PLAN.md`.
