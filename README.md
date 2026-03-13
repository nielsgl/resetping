# ResetPing

ResetPing is a tray-first desktop notifier for Codex reset status. It polls `https://hascodexratelimitreset.today/api/status` and sends native notifications when the state changes.

## Current v1 implementation

- Tauri + Rust backend + TypeScript settings UI
- Tray app with actions:
  - Open Settings
  - Force Refresh
  - Send Test Notification
  - Quit
- Polling engine:
  - Default interval: 60s (configurable 30s-900s)
  - Low-power interval: 300s on macOS low power mode (`pmset` detection)
  - HTTP timeout: configurable (default 8000ms)
- State machine and persistence:
  - Transition detection (`state != "no"` => `yes`)
  - Notification policies: `flip` or `no_to_yes`
  - Initial-state notification toggle
  - Consecutive failure tracking and degraded health threshold (5 failures)
  - Local persistent settings/snapshot/transitions in app config directory
- Diagnostics:
  - In-memory ring buffer logs exposed in settings UI
  - Copy logs action
- Launch at login support via Tauri autostart plugin

## Roadmap notes

- Update check command is currently a v1 stub and logs manual checks.
- Sentry telemetry is env-driven (`SENTRY_DSN`) and currently limited to startup marker + polling errors.
- Full signed updater integration is planned next hardening step.
- On macOS, banner notifications may be suppressed while ResetPing is the active/focused app window. Tray-triggered/background notifications remain the most reliable visual path.

## Development

Requirements:

- Node.js 20+
- Rust stable toolchain
- Tauri system prerequisites for your OS: <https://v2.tauri.app/start/prerequisites/>

Install and run:

```bash
npm install
npm run tauri dev
```

Quality checks:

```bash
npm run test
npm run typecheck
npm run build
cd src-tauri && cargo test && cargo fmt --check && cargo clippy -- -D warnings
```

## Manual testing runbook

### Notification policy test with local mock server

1. Start the mock server:

```bash
npm run mock:status
```

2. In ResetPing Settings, set endpoint to:
`http://127.0.0.1:8787/api/status`

3. Flip state via terminal:

```bash
# to yes
curl -X POST http://127.0.0.1:8787/admin/set \
  -H 'content-type: application/json' \
  -d '{"state":"yes"}'

# to no
curl -X POST http://127.0.0.1:8787/admin/set \
  -H 'content-type: application/json' \
  -d '{"state":"no"}'
```

4. Validate behavior:
- `notification_policy=flip`: expect notification on both `no -> yes` and `yes -> no`.
- `notification_policy=no_to_yes`: expect notification only on `no -> yes`.

### Diagnostics copy test

1. Trigger a few events (refresh, endpoint toggle, etc.).
2. Click `Copy Logs`.
3. Paste into a text editor.
4. Expect timestamped lines like:
`2026-... [INFO] Polling status ...`

### Launch-at-login test (still pending)

1. Enable `Launch at login` and save.
2. Log out/in or reboot.
3. Confirm ResetPing starts automatically and appears in the menu bar.
4. Disable and repeat to confirm it does not auto-start.

### Telemetry test (`SENTRY_DSN`)

1. Set DSN either via `.env` file in repo root or shell env:

`.env` example:

```bash
SENTRY_DSN=https://<key>@<org>.ingest.sentry.io/<project>
```

or launch-time env:

```bash
SENTRY_DSN='https://<key>@<org>.ingest.sentry.io/<project>' npm run tauri dev
```

2. Force polling error by setting invalid endpoint, then refresh.
3. In Sentry, expect:
- startup info event (`ResetPing telemetry initialized`)
- error event(s) for polling failures.

## Settings model

The app persists these settings:

- `poll_interval_sec` (default `60`, bounds `30..900`)
- `low_power_poll_interval_sec` (default `300`)
- `http_timeout_ms` (default `8000`)
- `notification_policy` (`flip` or `no_to_yes`)
- `notify_initial_state` (default `true`)
- `launch_at_login` (default `false`)
- `update_checks_enabled` (default `false`)
- `update_check_interval_hours` (default `24`)
- `status_endpoint_url` (default production endpoint)
- `telemetry_enabled` (default `true`; telemetry backend wiring pending)

## Packaging and release

CI and release workflow files are under `.github/workflows`:

- `ci.yml`: lint/type/build/test on macOS, Linux, and Windows
- `release.yml`: release pipeline with platform artifacts and macOS signing/notarization hooks

For macOS signed+notarized releases, configure these repository secrets:

- `APPLE_CERTIFICATE_P12`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_API_KEY_ID`
- `APPLE_API_ISSUER_ID`
- `APPLE_API_PRIVATE_KEY_B64`
- `TAURI_SIGNING_PRIVATE_KEY_B64`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

And these variables:

- `CODESIGN_IDENTITY`
- `NOTARY_PROFILE_NAME`

## License

MIT
