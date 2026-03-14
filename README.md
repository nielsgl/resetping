# ResetPing

ResetPing is a tray-first desktop notifier for Codex reset status. It polls `https://hascodexratelimitreset.today/api/status` and sends native notifications when the state changes.

## Install

### Homebrew (recommended)

```bash
brew tap nielsgl/tap
brew install --cask nielsgl/tap/resetping
```

### GitHub Releases

Download the latest macOS `.dmg` from:

- `https://github.com/nielsgl/resetping/releases/latest`

### macOS trust warning (free mode / non-notarized)

When `FREE_MODE=true` releases are not notarized. On first launch, macOS may block the app.

If blocked:
1. Open `System Settings` -> `Privacy & Security`.
2. In the security section, allow opening `ResetPing`.
3. Or right-click the app in Finder and choose `Open` once.

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

- In-app updater is enabled for macOS v1 only.
- Manual and background checks detect stable updates and defer installation until explicit user action.
- Sentry telemetry is split by purpose:
  - `error_telemetry_enabled` (default `true`)
  - `usage_telemetry_enabled` (default `false`)
- Anonymous `installation_id` is generated once and persisted locally.
- Usage events include `app_open`, `heartbeat` (24h), `transition_detected`, and `update_check`.
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

HTTP backend A/B in dev:

```bash
# Default (native-tls)
npm run tauri dev

# Rustls variant
npm run tauri dev -- --no-default-features --features http-rustls
```

Quality checks:

```bash
npm run test
npm run typecheck
npm run build
cd src-tauri && cargo test && cargo fmt --check && cargo clippy -- -D warnings
```

Pre-commit hooks (recommended):

```bash
# install pre-commit (choose one):
#   brew install pre-commit
#   pipx install pre-commit
#   uv tool install pre-commit
#     or one-shot usage: uvx pre-commit run --all-files
npm run hooks:install
```

Hook coverage is stack-aware:
- YAML/JSON/TOML parsing checks (`check-yaml`, `check-json`, `check-toml`)
- GitHub Actions workflow lint (`actionlint`)
- Rust format check on Rust file commits (`cargo fmt --check`)
- Pre-push quality gate: `npm run typecheck`, `npm run test`, `cargo test`, `cargo clippy -- -D warnings`

Run all hooks manually:

```bash
npm run hooks:run
```

Direct command alternatives:

```bash
pre-commit run --all-files
pipx run pre-commit run --all-files
uvx pre-commit run --all-files
```

Notes:
- Contributors are not expected to use `uv`; any supported install path is fine.
- CI remains the enforcement gate even if local hooks are skipped.

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
- error event(s) for polling failures when error telemetry is enabled.
- usage events (`app_open`, `heartbeat`) when usage analytics is enabled.

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
- `error_telemetry_enabled` (default `true`)
- `usage_telemetry_enabled` (default `false`)

## Install metrics snapshots

Capture GitHub release download metrics locally:

```bash
GITHUB_REPOSITORY=<owner>/<repo> GITHUB_TOKEN=<token> npm run metrics:installs
```

Automated snapshots run daily via `.github/workflows/daily-install-snapshot.yml` and write to:

- `ops/metrics/install_snapshots.json`

See telemetry/data constraints in:

- `docs/TELEMETRY_POLICY.md`

## Packaging and release

CI and release workflow files are under `.github/workflows`:

- `ci.yml`: lint/type/build/test on macOS, Linux, and Windows
- `release.yml`: release pipeline with platform artifacts, optional Apple signing/notarization, and Homebrew tap auto-update

The release workflow has a `workflow_dispatch` input:

- `free_mode` (default `true`)
  - `true`: skip Apple Developer signing/notarization (free distribution mode)
  - `false`: run full Apple signing/notarization path

Required secrets for free mode:

- `TAURI_SIGNING_PRIVATE_KEY_B64`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- `HOMEBREW_TAP_PAT` (fine-grained PAT with write access to `nielsgl/homebrew-tap`)

Required variables for free mode:

- `TAURI_UPDATER_PUBLIC_KEY`

Additional secrets/variables needed only for paid Apple signing/notarization mode (`free_mode=false`):

- `APPLE_CERTIFICATE_P12`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_API_KEY_ID`
- `APPLE_API_ISSUER_ID`
- `APPLE_API_PRIVATE_KEY_B64`
- `CODESIGN_IDENTITY`
- `NOTARY_PROFILE_NAME`

## License

MIT

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for local setup, hook installation, and required checks.
