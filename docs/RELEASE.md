# Release en distributie

## Vereiste GitHub-secrets

| Secret | Doel |
| --- | --- |
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | Base64-PFX van het Authenticode-certificaat. Zonder dit faalt de Windows-job bewust. |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | Developer ID Application-certificaat (base64 .p12). |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Notarisatie (app-specifiek wachtwoord). |

## Stappen

1. Werk `version` bij in `apps/desktop/package.json`, `src-tauri/Cargo.toml` en `src-tauri/tauri.conf.json`; vul `CHANGELOG.md`.
2. Controleer prijzen en modelnamen (`src-tauri/pricing.json`, `live.rs::models`) tegen de actuele OpenAI-documentatie en zet `verifiedAt`.
3. Merge naar `main`, tag `vX.Y.Z`, push de tag. De workflow bouwt Windows x64 (NSIS, met WebView2-bootstrapper), macOS arm64 en x64 (DMG, getekend en genotariseerd) en voegt `SHA256SUMS.txt` toe aan een draft release.
4. Doorloop de open gates in `docs/ACCEPTANCE.md` op de gebouwde artefacten; publiceer daarna de release.

Updates installeert de gebruiker handmatig; sessies en credentials blijven behouden (database en OS-store staan buiten de installatiemap; migraties maken eerst een herstelkopie).

## Lokaal bouwen

```bash
cd apps/desktop
npm ci
npx tauri build            # Windows: zet eerst PERL=C:\Strawberry\perl\bin\perl.exe
```
