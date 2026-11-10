## What changed

<!-- Describe the user-visible behavior change. -->

## Why

<!-- The problem this solves. Link related issues: Closes #123 -->

## How it was verified

<!--
Commands you ran, for example:
- cargo fmt && cargo clippy && cargo test
- manual check: `npm update -g <TAB>` now lists globally installed packages
-->

## Checklist

- [ ] `cargo fmt && cargo clippy && cargo test` pass
- [ ] New user-facing strings are in English (localized text goes into `i18n/*.json`)
- [ ] Snapshot changes were reviewed (`cargo insta review`)
- [ ] Commit message follows `:emoji: type: message`
