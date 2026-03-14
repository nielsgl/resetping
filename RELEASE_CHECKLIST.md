# Release Checklist: v0.1.2

## Scope
- Release target: `v0.1.2`
- Distribution mode: `FREE_MODE=true`
- Objective: verify release pipeline, Homebrew tap auto-update, and updater/install runtime behavior end-to-end.

## Preflight
- [ ] `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` are all `0.1.1`.
- [ ] `CHANGELOG.md` has `0.1.1` notes.
- [ ] Required Actions secrets/variables are present:
  - [ ] `TAURI_SIGNING_PRIVATE_KEY_B64`
  - [ ] `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
  - [ ] `TAURI_UPDATER_PUBLIC_KEY`
  - [ ] `HOMEBREW_TAP_PAT`

## CI / Release Workflow
- [ ] Trigger `.github/workflows/release.yml` with `free_mode=true`.
- [ ] `build_macos` succeeded.
- [ ] `build_windows` succeeded.
- [ ] `build_linux` (`ubuntu-24.04`) succeeded.
- [ ] `build_linux` (`ubuntu-24.04-arm`) succeeded.
- [ ] `publish` succeeded.
- [ ] `update_homebrew_tap` succeeded.

Evidence:
- Actions run URL:
- Release URL:

## Artifacts and Metadata
- [ ] GitHub release `v0.1.1` exists.
- [ ] macOS DMG exists.
- [ ] macOS updater archive (`.app.tar.gz`) exists.
- [ ] Windows artifacts exist (`.exe` and/or `.msi`).
- [ ] Linux artifact exists (`.AppImage`).
- [ ] Signature files (`.sig`) exist for updater-relevant artifacts.

Evidence:
- Asset list:

## Homebrew Tap Automation Proof
- [ ] `Casks/resetping.rb` in `nielsgl/homebrew-tap` updated to `version "0.1.1"` by CI.
- [ ] `sha256` matches published macOS DMG.
- [ ] No manual commit was required for cask update.

Evidence:
- Tap commit URL:
- Cask file URL:

## macOS Install + Runtime Smoke
- [ ] Install from Homebrew:
  - [ ] `brew tap nielsgl/tap`
  - [ ] `brew install --cask --no-quarantine nielsgl/tap/resetping`
- [ ] App launches and appears in menu bar (not Dock).
- [ ] Tray menu opens and actions are clickable.
- [ ] `Send Test Notification` works.
- [ ] `Force Refresh` works against production endpoint.
- [ ] Diagnostics `Copy Logs` works.
- [ ] Settings persist after app restart.

Evidence:
- Notes/screenshots:

## Updater E2E (`0.1.0 -> 0.1.1`)
- [ ] Start with installed app version `0.1.0`.
- [ ] Trigger update check in app.
- [ ] App reports `0.1.1` available.
- [ ] Install action succeeds.
- [ ] App relaunches.
- [ ] Installed app version is `0.1.1`.

Verification command:
```bash
defaults read /Applications/ResetPing.app/Contents/Info CFBundleShortVersionString
```

Evidence:
- Updater UI notes:
- Post-install version output:

## Remaining Manual Gates
- [ ] Launch-at-login matrix complete (enable -> reboot -> starts; disable -> reboot -> does not start).
- [ ] Gatekeeper flow validated on a clean profile with README instructions.
- [ ] Windows/Linux parity smoke complete (tray, refresh, test notification, degraded indicator).

## Final Decision
- [ ] Ship approved
- [ ] Ship blocked

Blockers (if any):
-
