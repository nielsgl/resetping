# Repository Guidelines

## Project Structure & Module Organization
- `src/`: Frontend code (TypeScript, CSS, static assets) served by Vite.
- `src-tauri/`: Rust host app (`src/lib.rs`, `src/main.rs`, `Cargo.toml`, capabilities, icons, and Tauri config).
- `index.html`: Frontend entry point.
- `PLAN.md`: Product/implementation planning notes for ResetPing.
- `dist/` (generated): Frontend production build output.

For ResetPing v1, contributors must preserve subsystem boundaries: Rust owns `status_client`, `poll_engine`, `state_store`, `notify`, `updates`, `diagnostics`, and `telemetry`; frontend owns settings and tray-facing UI state only.

## Build, Test, and Development Commands
- `npm run dev`: Start Vite dev server for frontend-only iteration.
- `npm run tauri dev`: Run the desktop app locally with hot-reloading frontend + Rust host.
- `npm run typecheck`: Type-check frontend (`tsc --noEmit`).
- `npm run build`: Build production frontend assets.
- `npm run tauri build`: Produce desktop bundles/installers via Tauri.
- `cd src-tauri && cargo test`: Run Rust tests.
- `cd src-tauri && cargo fmt --check`: Rust formatting check.
- `cd src-tauri && cargo clippy -- -D warnings`: Rust lint check.

Before merge, PRs must pass `typecheck`, `build`, `cargo test`, `cargo fmt --check`, and `cargo clippy -- -D warnings`.

## Dependency Management Rules (Mandatory)
- Never add, remove, or bump dependencies by manually editing manifest files.
- Manual manifest edits are prohibited for dependency changes in:
  - `package.json`
  - `src-tauri/Cargo.toml`
  - any Python dependency manifest in this repo (for example `pyproject.toml`, `requirements*.txt`)
- Use ecosystem-native package manager commands only:
  - Node: `npm add`, `npm add -D`, `npm remove`, `npm update`
  - Rust: `cargo add`, `cargo rm`, `cargo update`
  - Python (uv projects): `uv add`, `uv remove`, `uv lock`
- After dependency changes:
  - Commit the lockfile updates produced by the package manager.
  - Run the relevant validation commands and include results in the PR description.
- If a dependency change cannot be expressed with package-manager commands, stop and document the blocker instead of editing manifests directly.

## Coding Style & Naming Conventions
- TypeScript: 2-space indentation, semicolons, `camelCase` for variables/functions, `PascalCase` for types/classes.
- Rust: `rustfmt` defaults (4-space indentation), `snake_case` for functions/modules, `PascalCase` for structs/enums.
- File naming: frontend modules use `kebab-case` or existing convention; Rust files follow standard module naming.
- Keep functions small and side effects explicit across the Tauri boundary (`invoke` calls and `#[tauri::command]` handlers).
- Commit scopes should be consistent with domain areas: `frontend`, `tauri`, `poll`, `notify`, `config`, `release`, `docs`, `ci`.

## Testing Guidelines
- Frontend logic changes must include unit tests using Vitest (add/update test setup in the same PR when introducing new frontend logic).
- Rust tests should live near implementation (`mod tests`) and cover status normalization, transition policy behavior, failure/degraded recovery, and config bounds/defaults.
- Validate desktop integration manually with `npm run tauri dev` for UI/native command paths.
- For state-machine changes, include at least one sequence scenario (example: `no -> yes -> yes -> no`) in tests.

## Commit & Pull Request Guidelines
- Use Commitizen-style Conventional Commits with scopes and gitmoji.
- Keep commits small, focused, and atomic; avoid mixing frontend and Rust refactors unless required.
- Commit format: `<gitmoji> <type>(<scope>): <summary>`.
- Example messages:
  - `✨ feat(tray): add polling status indicator`
  - `🐛 fix(tauri): handle status endpoint timeout`
  - `♻️ refactor(frontend): simplify settings form state`
- PRs should include:
  - What changed and why.
  - Linked issue/plan item.
  - Validation steps run (build/test/lint commands).
  - Platforms validated (`macOS` required for v1; mention Windows/Linux checks when relevant).
  - Screenshots/GIFs for visible UI changes.

## Git Merge Strategy
- Merge pull requests into `main` using a merge commit (`--no-ff`).
- In GitHub, use **Create a merge commit**.
- Do not use squash merge or rebase merge for `main`.
- Preserve branch commit history on `main`.

## Release & Security Standards
- v1 release readiness must include macOS signing/notarization validation and publish Windows/Linux beta artifacts.
- Do not commit secrets, certificates, tokens, or signing material; use CI-managed secrets only.
- Any updater/release change must document expected artifacts and verification steps in the PR description.

## Telemetry & Privacy Rules
- ResetPing is privacy-first: telemetry must remain minimal, anonymous, and user-opt-out.
- No feature analytics or behavioral tracking is allowed.
- Any new telemetry/error event must document fields, purpose, and sampling/trigger behavior in the PR.

## Plan And Changelog Maintenance (Mandatory)
- This repository uses two different logs with different purposes:
  - `PLAN.md`: engineering execution status and gate tracking.
  - `CHANGELOG.md`: end-user release notes.
- Any PR that changes delivery status must update `PLAN.md` in the same PR.
- Any PR with user-visible behavior changes must update `CHANGELOG.md` under `Unreleased`.
- Do not merge status-changing work without both updates.

### Exact `PLAN.md` Update Requirements
- Update all three sections when applicable:
  - `Current Status Snapshot`
  - `Delivery Ledger`
  - `Final Release Gate Checklist`
- Add one line to `Plan Status Changelog` for every status transition.
- Use only these status tokens: `DONE`, `PARTIAL`, `PENDING`, `DEFERRED`, `BLOCKED`.
- Keep `Appendix A` intact and verbatim. Never delete it. Never rewrite historical content.
- If an item is `PENDING` or `BLOCKED`, add an explicit next step in `Next Actions (Priority Order)`.

### Exact `CHANGELOG.md` Update Requirements
- Use the `Unreleased` section only for in-flight changes.
- Place entries under one of: `Added`, `Changed`, `Fixed`, `Security`.
- Entries must be user-facing, concise, and behavior-oriented (not implementation internals).
- Move `Unreleased` entries into a versioned section only when cutting a release tag.

### Pull Request Checklist (Required)
- `PLAN.md` updated (if delivery/status/gates changed).
- `CHANGELOG.md` updated (if user-visible behavior changed).
- Validation commands listed in PR description with pass/fail outcome.
