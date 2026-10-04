# Repository (AC-00)

| | |
| --- | --- |
| URL | https://github.com/jasperhoekstra/canvas-facilitator (private) |
| Initiële commit | `69299aa5ca009afb191b5853cd9d2678d41a496b` — `chore: initialize canvas-facilitator repository` |
| Geverifieerd | 4 oktober 2026, via `gh api repos/jasperhoekstra/canvas-facilitator/commits/main` |
| Eerste applicatiecode | Branch `feat/app-v1`, via pull request naar `main` |

## Werkwijze

- `main` is beschermd: alleen via pull request, CI groen (build, tests, secretscan).
- CI: `.github/workflows/ci.yml` (gitleaks, frontend build + vitest, Rust-tests op Windows en macOS).
- Release: tag `vX.Y.Z` → `.github/workflows/release.yml` bouwt gesigneerde installers als draft release met `SHA256SUMS.txt`.
