# Acceptatiestatus v1.0

Stand: 4 oktober 2026. Zie ook "Afwijkingen van PRD v1.0" in `docs/ARCHITECTURE.md` (tekst in plaats van spraak, geen 15-minutenlimiet); AC-TIME en AC-VOICE (spraakuitvoer) gelden daardoor in aangepaste vorm. **Dit is nog geen production-ready 1.0** (PRD §13): de gates hieronder met status *open* vereisen echte hardware, een OpenAI-key met budget, of signing-certificaten.

| Gate | Status | Bewijs / wat nog moet |
| --- | --- | --- |
| Repository (AC-00) | ✅ | `docs/REPOSITORY.md` |
| Setup/installatie | ◐ | Setup-wizard (key, verbindingstest, audio, budget) gebouwd en gerenderd. Open: schone installatie met getekende installer op Windows 11 x64, macOS 13+ Intel en Apple Silicon. |
| UI (AC-UI) | ◐ | `npm run screenshots`: alle schermen op 1440×900, 1280×720 en 200% zoom, met lange inhoud. Open: ontwerp-review naast de originele slide; toetsenbordronde met schermlezer. |
| Faciliteren (AC-FAC) | ◐ | 20 scenario's in `docs/acceptance/fac-scenarios.json`; runner `cargo run --example fac_eval` (tekstmodus, productie-instructies en -validator). Native garanties zijn unit-getest (geen VALIDATED zonder evidence, stale write na correctie geweigerd, geen kunstmatige voltooiing). Open: run met echte key, rapport vastleggen. |
| Deadline (AC-TIME) | ◐ | Unit: klok (slaap, klokwijziging, herstel, 100-run sweep). Architectuur: één send-poort + watchdog. Open: 100 geautomatiseerde end-to-end runs met echte verbinding, slaap/hervat op beide OS'en, playback-stop ≤250 ms meten. |
| Spraak/transcript (AC-VOICE) | ☐ | Latency-metrics (p50/p95 eerste delta, definitief transcript, eerste audio) worden per sessie vastgelegd. Open: 50 beurten per OS, WER-set (30 opnames), echo-test met headset en ingebouwde audio. |
| Security (AC-SEC) | ◐ | Unit: DB en WAL onleesbaar zonder sleutel, verkeerde sleutel faalt, key-redactie. IPC-allowlist + CSP + geen fs/shell/http-plugins voor de renderer. Open: zoek een herkenbare testkey in bestanden/logs/bundle na echte sessie; vergrendelde store en tweede OS-gebruiker. |
| Bewaren/herstel (AC-DATA) | ◐ | Unit: transactionele mutaties, idempotente tool-calls, verwijderen met/zonder kostenledger. Migratie maakt versleutelde herstelkopie, rolt terug bij falen. Open: geforceerde crash tijdens transcriptie/canvaswijziging/migratie op echte build. |
| Kosten (AC-COST) | ◐ | Unit: PRD-rekenvoorbeeld (0,947 / 0,4626 / 1,6318), cache-subset, dubbele events, schatting→meting, ontbrekende prijs blokkeert, prijsupdate herschrijft historie niet, setup in totaal. Open: tien echte sessies per profiel, vergelijken met OpenAI-dashboard. |
| Performance | ☐ | Open: koude start, geheugen, CPU, payload meten op referentiehardware (release-build). |
| Release | ☐ | Workflow klaar (`release.yml`: signing, notarisatie, checksums). Open: certificaten als secrets, eerste getekende release, handmatige updatecheck (zie hieronder). |

## Bekende open punten

1. **Handmatige updatecheck in de app** ontbreekt nog; nu verwijst Instellingen naar de releasepagina. Voor een private repo werkt de anonieme GitHub-API niet; kies een publiek release-endpoint en voeg dan `tauri-plugin-updater` (handmatig, nooit tijdens sessie) toe.
2. **Model- en eventnamen** zijn gebaseerd op de OpenAI-documentatie van 4 oktober 2026 (`gpt-realtime-2.1`, `gpt-live-transcribe`, `response.output_audio.*`, `conversation.item.input_audio_transcription.*`). De transport-spike (PRD §14 stap 2) met echte key moet dit bevestigen; oudere eventnamen (`response.audio.*`) worden ook geaccepteerd.
3. **Transcriptie-usage**: als `gpt-live-transcribe` geen `usage` in het completed-event stuurt, blijft de transcriptieregel een schatting (gemarkeerd als onvolledig), conform §10.2.

## Handmatige procedures

- **AC-SEC sleutelzoektocht**: stel een herkenbare testkey in (`sk-test-CANARY-…`), voer een sessie, export en diagnostiekexport uit, en zoek daarna:
  `rg -a "CANARY" "%APPDATA%\nl.canvasfacilitator.app" "%LOCALAPPDATA%\nl.canvasfacilitator.app" <exportmap> <installatiemap>` (macOS: `~/Library/Application Support/nl.canvasfacilitator.app`, `~/Library/Logs/nl.canvasfacilitator.app`). Verwacht: geen treffers.
- **End-to-end (PRD §13)**: installeer → key → Nederlands gesprek → transcript en vijf kaarten wijzigen → corrigeer een veld → onderbreek spraak → pauze → hervat → automatische stop op 15:00 → heropen in Sessies → kosten → export → verwijder key en sessie. Op Windows en beide Mac-architecturen.
