# Architectuur

Tauri 2: React/TypeScript in de OS-webview, Rust-kern. Geen eigen backend, geen Node/Python-runtime voor eindgebruikers.

```text
React (src/)              vijf kaarten, transcript, bediening, historie, kosten, instellingen
   ↕ IPC-allowlist        build.rs + capabilities/default.json: alleen app-commando's + event listen
Rust (src-tauri/src/)
  live.rs     sessiecontroller: deadline-watchdog, 2 WebSockets, barge-in, tools, budget, reconnect
  clock.rs    900 s klok: max(monotoon, wandklok); slaap/klokwijziging; crashherstel
  audio.rs    cpal capture/playback, resampling naar 24 kHz, energie-VAD met echo-onderdrukking
  canvas.rs   vijf stappen, veldstatussen, native validatie van alle tool-calls
  ledger.rs   prijssnapshot, usage-parsing, USD-berekening (rust_decimal)
  db.rs       SQLCipher (versleuteld), migraties met herstelkopie, append-only usage-ledger
  secrets.rs  Keychain / Credential Manager (API-key en databasekey apart), redactie
  export.rs   Markdown / JSON / CSV
  prompt.rs   facilitatie-instructies + tijdsignalen
```

## Belangrijke keuzes

- **Eén poort voor AI-verkeer.** Elk uitgaand bericht gaat via `Live::send`, dat weigert zodra de klok verstreken is. Een watchdog (50 ms) sluit audio (`Gate`), verbindingen en status onafhankelijk van renderer en model. Playback stopt binnen één watchdog-tick plus één audiobuffer.
- **Klok.** Verstreken tijd = maximum van monotone klok en wandklok. Slaap (macOS pauzeert de monotone klok) telt dus mee; een teruggezette klok verlengt nooit. Klok achteruit tijdens sessie → sessie sluit. Bij herstart wordt een lopende sessie alleen `PAUSED` als heartbeat en wandklok consistent zijn en er tijd over is; hervatten is expliciet.
- **Client-VAD stuurt beide verbindingen.** Dezelfde lokale beurt wordt op de spraak- en transcriptieverbinding gecommit (`turn_detection: null` op beide), zodat beurt-ID's en item-ID's één-op-één te koppelen zijn. Audio wordt alleen tijdens gedetecteerde spraak (+300 ms pre-roll) verstuurd, wat transcriptiekosten beperkt.
- **Barge-in.** Speech-start tijdens playback: lokale buffer leeg, `response.cancel`, `conversation.item.truncate` op de daadwerkelijk afgespeelde milliseconden. Het transcript toont het afgespeelde deel als gesproken, de rest doorgestreept als afgebroken (proportionele benadering).
- **Kosten.** Bij `response.created` wordt een conservatieve schatting geboekt (`est:<id>`), bij `response.done` vervangen door gemeten usage (`resp:<id>`, unieke sleutel). Geannuleerde of afgebroken responses houden zo zichtbaar een schatting; dubbele events tellen nooit dubbel. Transcriptie: schatting per gecommitte beurt op basis van verzonden seconden, vervangen door `usage.type = duration` als die meekomt.
- **Tools.** Model stelt voor, `canvas::validate` beslist: veld-allowlist, lengte, `expected_revision` (handmatige correctie maakt modelwrites stale), VALIDATED alleen met evidence, `complete_step` alleen met bevestiging én minimale inhoud. Herhaalde call-ID's zijn idempotent (`tool_calls`-tabel).
- **Context.** Stabiele instructies (caching), tijdsignalen als systeem-items bij fasewissel, na 60 items worden de oudste 20 verwijderd en vervangen door de bevestigde canvascontext.

## Bewuste vereenvoudigingen (met upgradepad)

- Lineaire resampler en energie-VAD (`ponytail:`-commentaar in `audio.rs`). Vervangen als AC-VOICE (WER, zelf-triggering) dat vereist.
- Echo-onderdrukking is drempelverhoging tijdens playback, geen echte AEC. Headset wordt aanbevolen in de UI.
- Slaapdetectie via sprong in de wandklok (>3 s) in plaats van OS-suspend-events. De deadline blijft in beide gevallen correct.
