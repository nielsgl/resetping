# Contributing

Thanks for contributing to ResetPing.

## Local setup

Requirements:
- Node.js 20+
- Rust stable toolchain
- Tauri prerequisites for your OS: <https://v2.tauri.app/start/prerequisites/>

Install dependencies:

```bash
npm ci
```

## Pre-commit hooks

This repository uses `pre-commit` for lightweight local quality checks.

Install `pre-commit` using any preferred tool:

```bash
# Option A
brew install pre-commit

# Option B
pipx install pre-commit

# Option C
uv tool install pre-commit
```

Install hooks:

```bash
npm run hooks:install
```

Run all hooks manually:

```bash
npm run hooks:run
```

Direct alternatives:

```bash
pre-commit run --all-files
pipx run pre-commit run --all-files
uvx pre-commit run --all-files
```

## Required checks before merge

These checks are required and enforced in CI:

```bash
npm run typecheck
npm run build
npm run test
cd src-tauri && cargo test
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -- -D warnings
```

Local hooks improve feedback speed but CI is the final gate.
