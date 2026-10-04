# Bijdragen

- `main` is beschermd. Werk op een featurebranch (`feat/...`, `fix/...`, `chore/...`) en open een pull request.
- CI (build, tests, secretscan) moet groen zijn vóór samenvoegen.
- Commitberichten volgen Conventional Commits (`feat:`, `fix:`, `chore:`, `docs:`, `test:`).
- Nooit secrets, lokale sessies, databases of echte gebruikersdata committen. `.env.example` bevat alleen lege waarden.
- Productbeslissingen staan in `docs/PRD-v1.0.md`. Wijzig het PRD expliciet als een eis niet haalbaar blijkt.
