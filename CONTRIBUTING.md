# Contributing to Sugg

Thanks for taking the time to contribute! This document covers the local workflow,
the conventions we use, and the places where help is most welcome.

## Prerequisites

- **Rust** 1.85 or newer (`rustup toolchain install stable`)
- No Node.js toolchain is required to build or test the project

## Build and test

```bash
cargo build                          # debug build
cargo run -p sugg-deploy --release   # build and install into ~/.sugg
cargo fmt && cargo clippy && cargo test
```

`cargo test` runs the workspace test suite, including integration tests that compile
the completion fixtures under `tests/fixtures/completions/`.

## Project layout

| Crate | Path | Purpose |
|---|---|---|
| `sugg-core` | `crates/sugg-core/` | Shared types, QuickJS runtime, cache structures |
| `sugg-ast` | `crates/sugg-ast/` | AST extraction of static commands and dynamic functions |
| `sugg-engine` | `crates/sugg-engine/` | Bundling (Rolldown), bytecode compilation, install/upgrade |
| `sugg-cli` | `crates/sugg-cli/` | `sugg` binary: completion state machine and shell output |
| `sugg-deploy` | `crates/sugg-deploy/` | Builds and installs the binaries into `~/.sugg` |
| `sugg-wasm` | `crates/sugg-wasm/` | WebAssembly bindings (used by the website) |

## Tests

Completion results are verified with `insta` JSON snapshots
(`insta::assert_json_snapshot!`). Do not hand-assert individual fields — snapshots
capture the full output (`value`, `display`, `description`, `style`, ...).

```bash
cargo test                    # fails on snapshot mismatch, writes .snap.new files
cargo insta review            # accept/reject new snapshots interactively
```

Some integration tests shell out to external tools:

- `git` — the git completion tests create their own throwaway repository with fixed
  branches and remotes, so they do not depend on the surrounding checkout.
- `bun` — the bun completions call `bun getcompletes` / `bun pm ls -g`. When `bun` is
  not installed those tests are skipped with a message instead of failing with a
  misleading snapshot mismatch. CI installs bun so they always run there.

To add a scenario:

1. Add or extend a fixture in `tests/fixtures/completions/`.
2. Add a test in `tests/integration.rs`.
3. Run `cargo test` to generate the snapshot baseline, then review it.

## Commits

Commits use a gitmoji plus a Conventional Commits type:

```
:emoji: type: message
```

| emoji | type | when |
|---|---|---|
| `:sparkles:` | feat | new feature |
| `:bug:` | fix | bug fix |
| `:construction_worker:` | ci | CI/CD |
| `:memo:` | docs | documentation |
| `:recycle:` | refactor | refactor |
| `:green_heart:` | fix/ci | CI fix |
| `:zap:` | perf | performance |
| `:art:` | style | code style |
| `:bookmark:` | chore | release/version |

Release notes are generated from these types, so pick the one that matches the change.

## Code style

- Run `cargo fmt` and `cargo clippy` before pushing; CI enforces both.
- New comments and documentation should be written in **English**.
- User-facing strings (CLI output, error messages) must be in English. Localized
  completion descriptions belong in `i18n/*.json`, not in code.
- JavaScript APIs must be imported explicitly from `"sugg"`
  (`import { execFile } from "sugg"`), never taken from globals.

## Pull requests

- Keep pull requests focused; unrelated refactors belong in a separate PR.
- Describe the user-visible behavior change and how you verified it.
- CI must be green before review.

## Reporting bugs and requesting features

Open an issue with the templates in `.github/ISSUE_TEMPLATE/`. For security issues,
follow [SECURITY.md](SECURITY.md) instead of opening a public issue.
