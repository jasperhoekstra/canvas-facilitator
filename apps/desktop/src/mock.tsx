// Dev-only harness for AC-UI screenshot tests: renders the real App against mocked IPC.
// Not part of the production bundle (only index.html is built).
import { mockIPC } from "@tauri-apps/api/mocks";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles.css";

const q = new URLSearchParams(location.search);
const view = q.get("view") ?? "live";
const long = q.has("long");
const now = Date.now();
const L = (s: string) => (long ? `${s} — ${"met uitgebreide toelichting over randvoorwaarden, uitzonderingen en afhankelijkheden ".repeat(3)}` : s);

const steps = [
  { key: "KIES", label: "Kies", question: "Welk probleem lossen we eerst op, voor wie en wie is business owner?", domains: ["Desirability", "Viability"],
    fields: ["doelgroep", "job_to_be_done", "pain_gain", "probleem", "ai_fit", "business_owner"] },
  { key: "MEET", label: "Meet", question: "Wat moet beter worden, wanneer is het een succes, ook over een jaar?", domains: ["Viability", "Desirability"],
    fields: ["kpi", "baseline", "target", "termijn", "meetwijze", "eigenaar"] },
  { key: "BEGRENS", label: "Begrens", question: "Wie mag wat, met welke data en systemen en welke controles?", domains: ["Sustainability"],
    fields: ["databron", "modeltaak", "acties", "risico", "guardrail", "menselijke_controle"] },
  { key: "REALISEER", label: "Realiseer", question: "Hoe bouwen, testen, verbeteren en brengen we de oplossing naar productie?", domains: ["Feasibility", "Viability"],
    fields: ["concept", "interactieflow", "build_buy_partner", "experiment", "testcriterium", "kostendrijvers"] },
  { key: "VERANKER", label: "Veranker", question: "Hoe landt dit in het werkproces en wie bezit de verandering?", domains: ["Desirability", "Feasibility", "Sustainability", "Viability"],
    fields: ["procesintegratie", "eigenaar", "adoptie", "monitoring", "evaluatiemoment", "eerste_actie"] },
].map((s) => ({ ...s, fields: s.fields.map((f) => ({ key: f, label: f.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase()), domain: s.domains[0] })) }));

let t = now - 400_000;
const item = (step: string, field: string, value: string, status: string, manual = false) => ({
  id: `${step}-${field}`, step, field, domain: "Desirability", value: L(value), status, evidence: status === "VALIDATED" ? "Klanttevredenheidsonderzoek Q2" : "",
  sourceTurnIds: [], revision: manual ? 2 : 1, manual, updatedAt: (t += 1000),
});
const canvas = {
  items: [
    item("KIES", "doelgroep", "Binnendienst offertes (12 fte)", "VALIDATED"),
    item("KIES", "job_to_be_done", "Offerte binnen 1 dag versturen", "DECIDED"),
    item("KIES", "pain_gain", "Veel handmatig overtypen uit e-mails", "PARTIAL"),
    item("KIES", "probleem", "Doorlooptijd offerte-aanvraag", "DECIDED"),
    item("KIES", "ai_fit", "Extractie uit ongestructureerde mails", "ASSUMPTION"),
    item("KIES", "business_owner", "Hoofd Sales Support", "DECIDED", true),
    item("MEET", "kpi", "Doorlooptijd aanvraag→offerte", "DECIDED"),
    item("MEET", "baseline", "Geen nulmeting beschikbaar", "PARKED"),
    item("MEET", "target", "< 24 uur voor 80% van aanvragen", "ASSUMPTION"),
    item("BEGRENS", "databron", "Gedeelde mailbox + ERP-artikelen", "PARTIAL"),
    item("BEGRENS", "acties", "AI stelt concept op; versturen alleen door mens", "CONTRADICTED"),
  ],
  notes: [
    { id: "n1", step: "MEET", kind: "assumption", text: "80% haalbaar zonder extra fte", createdAt: now },
    { id: "n2", step: "BEGRENS", kind: "challenge", text: "Wie controleert prijzen bij maatwerk?", createdAt: now },
  ],
  decisions: [{ id: "d1", kind: "action", content: "Nulmeting doorlooptijd over 4 weken", rationale: "", owner: "onbekend", due: "onbekend", createdAt: now }],
  completed: [{ step: "KIES", synthesis: "Binnendienst, doorlooptijd offertes", confirmationTurnId: "u1", createdAt: now }],
};
// Mirrors what Rust's Canvas::view() derives for this fixture.
const canvasView = {
  canvas: q.has("intro") ? { items: [], notes: [], decisions: [], completed: [] } : canvas,
  activeStep: "BEGRENS",
  story: [
    "Voor Binnendienst offertes (12 fte) lossen we eerst dit op: Doorlooptijd offerte-aanvraag. De klus: Offerte binnen 1 dag versturen. Business owner: Hoofd Sales Support.",
    "Succes meten we met Doorlooptijd aanvraag→offerte: van [nog open] naar < 24 uur voor 80% van aanvragen, binnen [nog open].",
    "De AI doet dit: [nog open], met Gedeelde mailbox + ERP-artikelen. Grootste risico: [nog open]; daarom [nog open] en [nog open].",
    "We bouwen [nog open] ([nog open]) en beginnen klein: [nog open]. Geslaagd als: [nog open].",
    "[nog open] borgt het in [nog open]; we evalueren [nog open].",
  ],
  steps: [
    { step: "KIES", status: "voldoende uitgewerkt", missing: [] },
    { step: "MEET", status: "open punten", missing: ["Baseline", "Termijn", "Meetwijze", "Eigenaar"] },
    { step: "BEGRENS", status: "actief", missing: ["Modeltaak", "Risico", "Guardrail", "Menselijke controle"] },
    { step: "REALISEER", status: "niet gestart", missing: ["Concept", "Interactieflow", "Build buy partner", "Experiment", "Testcriterium", "Kostendrijvers"] },
    { step: "VERANKER", status: "niet gestart", missing: ["Procesintegratie", "Eigenaar", "Adoptie", "Monitoring", "Evaluatiemoment", "Eerste actie"] },
  ],
};
const turn = (id: string, speaker: string, text: string, extra = {}) => ({
  id, sessionId: "s1", seq: Number(id.slice(1)), providerItemId: null, speaker, startedAt: now, endedAt: now, text, final: true,
  interrupted: false, failed: false, spokenText: null, originalText: null, correctedAt: null, ...extra,
});
const turns = q.has("intro") ? [] : [
  turn("a1", "assistant", "Waar gaat je AI-idee over en wat wil je na deze presentatie besloten hebben?"),
  turn("u2", "user", "Offertes duren te lang, ik wil dat AI de aanvragen uit de mail haalt."),
  turn("a3", "assistant", "Hoe lang duurt een offerte nu gemiddeld, en waar baseren we dat op? En wat zou deze verwachting ontkrachten?", { interrupted: true, spokenText: "Hoe lang duurt een offerte nu gemiddeld," }),
  turn("u4", "user", "Dat weten we eigenlijk niet precies.", { failed: false, correctedAt: now, originalText: "dat weten we eigenlijk niet presies" }),
  turn("a5", "assistant", "Dan parkeren we de nulmeting als actie. Mag de AI offertes zelf versturen, of alleen een concept klaarzetten?"),
  turn("u6", "user", "Alleen een concept, maar bij standaardorders zou het", { final: false }),
];
const usage = { textIn: 21000, textCached: 14000, audioIn: 9000, audioCached: 4000, textOut: 900, audioOut: 5200, reasoning: 0, billedSeconds: "182.4", unresolved: [] };
const cost = { totalUsd: "0.4123", incomplete: true, measuredUsd: "0.3800", estimatedUsd: "0.0323", usage, responses: 9, unresolved: [] };
const settings = { profile: "quality", style: "neutraal", voice: "marin", inputDevice: null, outputDevice: null, sessionBudgetUsd: "2.00", monthlyBudgetUsd: "25.00", retentionDays: null, pricingOverride: null, onboarded: view !== "setup" };
const session = { id: "s1", title: "AI-assistent voor offerte-aanvragen", createdAt: now - 400_000, startedAt: now - 400_000, endedAt: view === "live" ? null : now - 10_000,
  deadlineAt: now + 500_000, status: view === "live" ? "ACTIVE" : "COMPLETED", stopReason: view === "live" ? null : "deadline", style: "neutraal", profile: "quality",
  model: "gpt-realtime-2.1", transcribeModel: "gpt-live-transcribe", parentId: null, priceVersion: "2026-10-04", budgetUsd: "2.00",
  metricsJson: JSON.stringify({ elapsedMs: 900000, userTurns: 14, assistantTurns: 15, responses: 22, p50FirstDeltaMs: 640, p95FirstDeltaMs: 1210 }) };
const snap = { sessionId: "s1", status: "ACTIVE", costUsd: "0.4123", costIncomplete: true, budgetUsd: "2.00",
  voiceState: q.has("late") ? "denkt" : "luistert", connection: "verbonden", muted: false, paused: false, model: "gpt-realtime-2.1", userTurns: 7, audioInSecs: 95,
  guide: { kind: "stelt_voor" as const, bullets: ["Doorlooptijd offerte als KPI: van aanvraag tot verzending", "Nulmeting: steekproef van 50 offertes uit Q3", "Target: 30% sneller binnen zes maanden"], fields: [{ step: "BEGRENS", field: "risico" }, { step: "BEGRENS", field: "guardrail" }] },
  refill: null };

mockIPC((cmd) => {
  switch (cmd) {
    case "app_status":
      return { storageError: null, keyPresent: view !== "setup", keyMask: view !== "setup" ? "sk-…a1B2" : null, keyError: null, settings, recoverable: [], live: view === "live" ? snap : null, version: "0.1.0" };
    case "canvas_definition": return steps;
    case "get_session":
      return { session, view: canvasView, turns, cost, usage: [], toolCalls: 31, toolRejections: 2 };
    case "live_snapshot": return snap;
    case "list_sessions": return [{ ...session, cost }, { ...session, id: "s2", title: "Chatbot klantenservice", status: "INTERRUPTED", cost: { ...cost, incomplete: false } }];
    case "get_settings": return settings;
    case "get_pricing": return { pricing: { version: "2026-10-04", verifiedAt: "2026-10-04", source: "https://developers.openai.com/api/docs/pricing", models: {} }, ageDays: 0, stale: false, overridden: false, bundled: {} };
    case "cost_overview": return { total: cost, month: cost, setup: { ...cost, totalUsd: "0" }, deletedSessionRows: 0, settings, priceVersion: "2026-10-04" };
    case "cost_estimate": return { model: "gpt-realtime-2.1", low: "0.947", high: "1.632", priceVersion: "2026-10-04" };
    case "audio_devices": return [{ id: "wasapi:1", name: "Headset-microfoon", default: true }];
    case "plugin:event|listen": return 1;
    default: return null;
  }
}, { shouldMockEvents: true });

if (view !== "live" && view !== "setup") {
  // Navigate after first render.
  setTimeout(() => {
    const label = { history: "Sessies", costs: "Kosten", settings: "Instellingen", new: "Nieuw", detail: "Sessies" }[view];
    [...document.querySelectorAll("nav button")].find((b) => b.textContent === label)?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    if (view === "detail") setTimeout(() => (document.querySelector("tbody button") as HTMLButtonElement | null)?.click(), 200);
  }, 300);
}
const clickText = (t: string) => [...document.querySelectorAll("button")].find((b) => b.textContent === t)?.click();
if (q.has("overview") || q.has("panel")) setTimeout(() => clickText("Overzicht"), 300);
if (q.has("panel")) setTimeout(() => (document.querySelectorAll(".card")[2] as HTMLButtonElement | null)?.click(), 500);
if (q.has("transcript")) setTimeout(() => clickText("Transcript"), 300);
if (q.has("ending")) setTimeout(() => clickText("Slot"), 300);
if (q.has("present")) setTimeout(() => document.body.classList.add("presenting"), 800); // window API is not mocked

createRoot(document.getElementById("root")!).render(<App />);
