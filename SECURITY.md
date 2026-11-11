# Security Policy

## Supported versions

Security fixes land on the latest release line. Older releases are not patched.

| Version | Supported |
|---|---|
| 0.3.x (latest) | ✅ |
| < 0.3 | ❌ |

## Reporting a vulnerability

Please **do not open a public issue** for security problems.

Use GitHub's private reporting flow instead:

1. Go to the [Security tab](https://github.com/getsugg/sugg/security/advisories/new).
2. Submit a private advisory with reproduction steps and affected versions.

We aim to acknowledge reports within 72 hours and to publish an advisory once a fix
is available. Credit will be given in the advisory unless you prefer otherwise.

## Trust model

Sugg runs completion scripts as JavaScript (compiled to QuickJS bytecode) inside the
`sugg` process. This matters when judging what counts as a vulnerability:

- **Installing a completion script runs third-party code from the registry**
  (`getsugg/sugg-completions`). Scripts are code, not data — treat installing a
  script like installing a package. `sugg install --no-build` downloads without
  activating, so you can review the file before running `sugg reload`.
- Completion scripts can execute commands (`exec`, `execFile`), read files
  (`readFile`, `readJson`, `scanPath`) and make network requests (`fetch`) with the
  privileges of your user account.
- Reports about a malicious *third-party script* should go to that script's
  repository. Reports about the runtime failing to isolate, escape, or mis-execute
  scripts belong here.

## Out of scope

- Vulnerabilities in dependencies that do not affect Sugg (please report those
  upstream);
- The lack of checksums/signatures on install scripts while the project is
  pre-1.0 (tracked as a known hardening task);
- Issues that require an already-compromised local machine.
