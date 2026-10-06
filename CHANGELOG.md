# Changelog

## 1.1.0 — 2026-10-07 (niet gesigneerd)

- Eenvoudiger flow. Weg uit het verhaalscherm: vakjes opnieuw invullen, statussen (geparkeerd, aanname, …), aannames/kritische vragen, stapbevestiging en `question_answered`. Het model heeft nog vier tools: `vul_vakje`, `nieuwe_vraag`, `nieuwe_inspiratie`, `mark_decision`.
- Een nieuwe vraag komt in één keer met nieuwe inspiratie en de gloeiende vakjes (één geforceerde tool-aanroep in plaats van tekst plus een aparte inspiratieronde).
- Niet meer zelf doorschakelen: is de vraag beantwoord, dan verschijnt "✓ Beantwoord — druk N" (of "KIES is rond — druk N om door te gaan naar MEET"); het hoofdstuk blijft in beeld tot `N`. Bij een half antwoord volgt automatisch een verdiepings- of challengevraag (max. 2). "Sla over" of "weet ik niet" zeggen, of `N`, gaat door; `D` vraagt zelf door.

## 1.0.1 — 2026-10-06 (niet gesigneerd)

- Bijna realtime vullen: tijdens lang praten wordt na ±4 s bij een korte adempauze (uiterlijk na 8 s) tussentijds verwerkt; einde van een beurt na 500 ms stilte (was 700 ms). Canvasupdates worden per tool-aanroep toegepast zodra die compleet is, niet pas aan het eind van de AI-response.
- Doorschakelen voelt natuurlijker: de vraag is beantwoord zodra alle vakjes waar ze over gaat sinds de vraag zijn ingevuld (of het model `question_answered` aanroept), en de volgende vraag komt pas als de presentator is uitgepraat.

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
