# ResetPing Telemetry Policy (v1)

## Purpose
- Improve reliability (crashes/errors) and understand high-level product usage.
- Track install proxy trends from GitHub release download counts.

## Data we send
- Error telemetry (when enabled):
  - event type, component, error kind, error message
  - app version, platform, build channel
  - anonymous `installation_id`
- Usage analytics (when enabled):
  - `app_open`, `heartbeat`, `transition_detected`, `update_check`
  - app version, platform, build channel
  - anonymous `installation_id`

## Data we do not send
- No account identity (email/name/username)
- No geolocation profiling
- No session replay
- No custom PII fields
- `send_default_pii` is disabled

## User controls
- `error_telemetry_enabled` controls crash/error events.
- `usage_telemetry_enabled` controls product usage events.
- Controls are independent and available in Settings.

## Install proxy tracking
- Daily GitHub Actions job snapshots release asset `download_count`.
- Output is stored in `ops/metrics/install_snapshots.json`.
- Release downloads are an install proxy, not exact installs.

## Retention and handling
- Snapshot history keeps recent long-term trend data (currently last 730 entries).
- Sentry retention/processing follows project-level Sentry configuration.
