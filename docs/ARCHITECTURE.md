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
- **Tools.** Vier tools: `vul_vakje` (veld + tekst; stap volgt uit de veldsleutel, handmatige correcties zijn leidend), `nieuwe_vraag` (vraag + inspiratie + 1–3 vakjes), `nieuwe_inspiratie` en `mark_decision`. Model stelt voor, `canvas::validate` beslist; resultaten zijn idempotent op `call_id`. Geen statussen, aannames of stapbevestiging in de live-flow (het Overzicht kan nog handmatig corrigeren).
- **Context.** Stabiele instructies (caching), tijdsignalen als systeem-items bij fasewissel, na 60 items worden de oudste 20 verwijderd en vervangen door de bevestigde canvascontext.

## Bewuste vereenvoudigingen (met upgradepad)

- Lineaire resampler en energie-VAD (`ponytail:`-commentaar in `audio.rs`). Vervangen als AC-VOICE (WER, zelf-triggering) dat vereist.
- Echo-onderdrukking is drempelverhoging tijdens playback, geen echte AEC. Headset wordt aanbevolen in de UI.
- Slaapdetectie via sprong in de wandklok (>3 s) in plaats van OS-suspend-events. De deadline blijft in beide gevallen correct.

## Afwijkingen van PRD v1.0 (besluit opdrachtgever, 4 oktober 2026)

De app wordt ingezet tijdens presentaties. Daarom:

- **Geen spraakuitvoer.** De Realtime-sessie draait met `output_modalities: ["text"]`; de facilitator stelt zijn vragen als tekst op het scherm. De microfoon blijft de presentator volgen (spraak-in en live transcriptie). Barge-in is niet meer nodig; pauze annuleert nog wel een lopend antwoord. Kosten dalen sterk (geen audio-output).
- **Geen 15-minutenlimiet en geen zichtbare klok.** Tijdsignalen aan het model zijn vervallen; het model volgt het tempo van de presentator. Als vangnet blijft een harde maximale sessieduur van 4 uur (`clock::LIMIT_MS`) en het kostenbudget. Alle deadline-mechaniek (één poort, watchdog, slaap/klokwijziging, crashherstel) werkt ongewijzigd met die grens.
- **Presentatiemodus.** Standaardweergave "Verhaal": verhaallijn met vijf hoofdstukken, het actieve hoofdstuk groot met velden die zich vullen, de vraag van de facilitator als citaat en live ondertiteling. "Overzicht" toont de vijf kaarten met besluiten. Toetsen: ←/→ hoofdstuk, L live volgen, O overzicht, T transcript, F volledig scherm.
- **Distributie voor demo's**: een losse, niet-gesigneerde `.exe` (`npx tauri build --no-bundle`); Windows SmartScreen kan eenmalig waarschuwen. Vereist de WebView2-runtime (standaard aanwezig op Windows 11).
- **Bijna realtime.** De VAD splitst lang praten in stukken (`SpeechChunk`: na 4 s bij 160 ms pauze, uiterlijk na 8 s); elk stuk wordt gecommit en krijgt een stille canvas-update. Tool-aanroepen worden al bij `response.output_item.done` toegepast; `response.done` krijgt het gecachete resultaat (idempotent op `call_id`).
- **Eén ding per response.** `Reply::Silent` vult vakjes tijdens het praten; `Ask`/`Deepen` roepen geforceerd (`tool_choice`) `nieuwe_vraag` aan, `Inspire` (`I`) `nieuwe_inspiratie`. De vraag komt dus altijd samen met nieuwe inspiratie en de gloeiende vakjes; er is geen losse tekst en er zijn geen vervolg-responses.
- **Beoordelen, niet zelf doorschakelen.** Als er geen spraak en geen response meer loopt (`Live::decide`): alle vakjes van de vraag gevuld (en er kwam iets bij) → `ready` in de snapshot ("vraag", het volgende hoofdstuk of "einde"); de presentator gaat door met `N`. Half → automatisch een verdiepings- of challengevraag (`Deepen`, max. 2; daarna `ready`). Niets → blijven luisteren. "Sla over"/"weet ik niet" (`prompt::is_skip`) of `N` gaat direct door; nog lege vakjes van die vraag komen in `Canvas::skipped`. `D` vraagt handmatig door.
- **Strikte volgorde.** `Canvas::current_step` is het eerste hoofdstuk met lege, niet-overgeslagen vakjes. Antwoorden over latere hoofdstukken worden wel ingevuld maar verplaatsen de focus niet; een afgerond hoofdstuk blijft in beeld tot `N`.
- **Microfoon-xruns** (buffer under/overrun) en ontbrekende realtime-prioriteit worden alleen gelogd; de opname loopt door.
- **Volledig scherm** (`F`/knop, `Esc` sluit): geen bovenbalk, grotere typografie, zwevende bediening die na 2,5 s zonder muisbeweging verdwijnt.
- **Start- en slotdia.** Zolang de presentator nog niets zei en het canvas leeg is, toont de verhaalweergave een openingsdia. Het slotverhaal (`S`, knop *Slot*, en standaard in de sessiedetails) zet het canvas om in vijf zinnen via `Canvas::story()`: lokaal, zonder AI-aanroep, met zichtbare "nog open"-gaten; ook in de Markdown-export.
- **Minder UI-verkeer.** Voorlopige tekst gaat maximaal ~8× per seconde per beurt naar de UI; snapshots alleen bij wijziging.
- **Mac-testbuild.** `.github/workflows/build-desktop.yml` bouwt een universele, ad-hoc gesigneerde `.dmg` (Intel + Apple Silicon) en de Windows-`.exe` als artifacts.
