# Changelog

## 1.0.0 — 2026-10-06 (niet gesigneerd)

- De facilitator werkt de vijf stappen strikt in volgorde af. Het huidige hoofdstuk is het eerste dat nog niet af is (bevestigd, of alle velden ingevuld of geparkeerd); antwoorden over latere hoofdstukken worden vastgelegd maar verplaatsen de focus niet. `complete_step` voor een later hoofdstuk wordt geweigerd.
- Sneller door naar de volgende vraag: een bruikbare (ook voorlopige) waarde is genoeg. Eén antwoord vult en verbetert meerdere velden tegelijk; vragen dekken bij voorkeur twee open velden.
- Naast "De facilitator vraagt" staat een blok "De facilitator inspireert" of "stelt voor" met 3–4 bullets (tool `show_guide`); het verschijnt automatisch na elke nieuwe vraag en `I` haalt een nieuw voorstel op. "Eerder gevraagd" is verwijderd.
- Geen samenvatting en bevestigingsvraag meer per hoofdstuk: is een hoofdstuk ingevuld (of geparkeerd), dan gaat de facilitator direct door. Ook aan het eind geen samenvatting; het slotscherm toont het verhaal. Inspiratie mag ook een concrete oplossing zijn (agent, flow, app, automatisering).
- Rechterblok compacter: kleinere vraag (26px, presentatie 30px) en bullets (16px, presentatie 19px).
- Vakjes waar de huidige vraag over gaat krijgen een gloed. Klik op een vakje om het opnieuw te laten invullen: de volgende vraag gaat over dat vakje en het nieuwe antwoord overschrijft de waarde.

## 0.9.0 — 2026-10-06 (preview, niet gesigneerd)

- Facilitator schakelt zelf door naar de volgende vraag zodra de huidige voldoende beantwoord is (`question_answered`); "volgende" zeggen schakelt niet meer door, `N`/`PageDown`/knop wel.
- Kortere tekst op het scherm: vragen maximaal 15 woorden zonder samenvatting vooraf, canvasvelden maximaal 25 woorden.

## 0.1.0 — 2026-10-05 (preview, niet gesigneerd)

Eerste implementatie van alle v1.0-functies uit het PRD; acceptatiegates nog open (zie `docs/ACCEPTANCE.md`).
