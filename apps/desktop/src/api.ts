// Typed wrappers around the native IPC allowlist. The renderer never sees the API key,
// audio, the filesystem or the network.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type FieldStatus = "UNKNOWN" | "PARTIAL" | "ASSUMPTION" | "VALIDATED" | "CONTRADICTED" | "DECIDED" | "PARKED";
export const FIELD_STATUSES: FieldStatus[] = ["UNKNOWN", "PARTIAL", "ASSUMPTION", "VALIDATED", "CONTRADICTED", "DECIDED", "PARKED"];
export const STATUS_LABEL: Record<FieldStatus, string> = {
  UNKNOWN: "Onbekend",
  PARTIAL: "Deels",
  ASSUMPTION: "Aanname",
  VALIDATED: "Onderbouwd",
  CONTRADICTED: "Tegenstrijdig",
  DECIDED: "Besloten",
  PARKED: "Geparkeerd",
};

export interface FieldDef { key: string; label: string; domain: string }
export interface StepDef { key: string; label: string; question: string; domains: string[]; fields: FieldDef[] }

export interface CanvasItem {
  id: string; step: string; field: string; domain: string; value: string; status: FieldStatus;
  evidence: string; sourceTurnIds: string[]; revision: number; manual: boolean; updatedAt: number;
}
export interface Note { id: string; step: string; kind: "assumption" | "challenge"; text: string; createdAt: number }
export interface Decision { id: string; kind: "decision" | "action"; content: string; rationale: string; owner: string; due: string; createdAt: number }
export interface StepCompletion { step: string; synthesis: string; confirmationTurnId: string | null; createdAt: number }
export interface Canvas { items: CanvasItem[]; notes: Note[]; decisions: Decision[]; completed: StepCompletion[] }
/** Canvas plus step state derived natively (the rules live only in Rust). */
export interface StepView { step: string; status: "niet gestart" | "actief" | "voldoende uitgewerkt" | "open punten"; missing: string[] }
export interface CanvasView { canvas: Canvas; steps: StepView[]; activeStep: string | null }

export interface Turn {
  id: string; sessionId: string; seq: number; providerItemId: string | null; speaker: "user" | "assistant";
  startedAt: number; endedAt: number | null; text: string; final: boolean; interrupted: boolean; failed: boolean;
  spokenText: string | null; originalText: string | null; correctedAt: number | null;
}

export interface Usage {
  textIn: number; textCached: number; audioIn: number; audioCached: number; textOut: number; audioOut: number;
  reasoning: number; billedSeconds: string | null; unresolved: string[];
}
export interface CostSummary {
  totalUsd: string; incomplete: boolean; measuredUsd: string; estimatedUsd: string; usage: Usage; responses: number; unresolved: string[];
}
export interface UsageRow {
  id: number; sessionId: string | null; scope: string; model: string; responseId: string | null; kind: string;
  usage: Usage; costUsd: string | null; costError: string | null; priceVersion: string; superseded: boolean; createdAt: number;
}

export type SessionStatus = "DRAFT" | "CONNECTING" | "ACTIVE" | "PAUSED" | "FINALIZING" | "COMPLETED" | "INTERRUPTED" | "FAILED";
export interface Session {
  id: string; title: string; createdAt: number; startedAt: number | null; endedAt: number | null; deadlineAt: number | null;
  status: SessionStatus; stopReason: string | null; style: string; profile: string; model: string; transcribeModel: string;
  parentId: string | null; priceVersion: string; budgetUsd: string; metricsJson: string;
}
export interface SessionListItem extends Session { cost: CostSummary }
export interface SessionDetail {
  session: Session; view: CanvasView; turns: Turn[]; cost: CostSummary; usage: UsageRow[]; toolCalls: number; toolRejections: number;
}

export interface Settings {
  profile: "quality" | "mini"; style: "neutraal" | "coachend" | "kritisch"; voice: string;
  inputDevice: string | null; outputDevice: string | null; sessionBudgetUsd: string; monthlyBudgetUsd: string;
  retentionDays: number | null; pricingOverride: string | null; onboarded: boolean;
}

export interface Snapshot {
  sessionId: string; status: SessionStatus; remainingMs: number; elapsedMs: number; costUsd: string; costIncomplete: boolean;
  budgetUsd: string; voiceState: string; connection: string; muted: boolean; paused: boolean; model: string; phase: string;
  userTurns: number; audioInSecs: number; audioOutSecs: number;
}

export interface AppStatus {
  storageError: string | null; keyPresent: boolean; keyMask: string | null;
  settings: Settings | null; recoverable: Session[]; live: Snapshot | null; version: string;
}

export interface Device { id: string; name: string; default: boolean }
export interface Notice { level: "info" | "warn" | "error"; text: string }

export const api = {
  appStatus: () => invoke<AppStatus>("app_status"),
  retryStorage: () => invoke<void>("retry_storage"),
  saveKey: (key: string) => invoke<string>("save_key", { key }),
  deleteKey: () => invoke<void>("delete_key"),
  testConnection: (profile: string) => invoke<{ model: string; ok: boolean; error: string | null }[]>("test_connection", { profile }),
  audioDevices: () => invoke<{ inputs: Device[]; outputs: Device[] }>("audio_devices"),
  micTestStart: (input: string | null, output: string | null) => invoke<void>("mic_test_start", { input, output }),
  micTestStop: () => invoke<void>("mic_test_stop"),
  speakerTest: () => invoke<void>("speaker_test"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  getPricing: () => invoke<{ pricing: any; ageDays: number | null; stale: boolean; overridden: boolean; bundled: any }>("get_pricing"),
  costEstimate: (profile: string) => invoke<{ model: string; low: string; high: string; priceVersion: string }>("cost_estimate", { profile }),
  createSession: (req: { title: string; style: string; profile: string; budgetUsd: string; parentId: string | null }) =>
    invoke<Session>("create_session", { req }),
  startSession: (id: string) => invoke<Snapshot>("start_session", { id }),
  resumeSession: (id: string) => invoke<Snapshot>("resume_session", { id }),
  pauseSession: (id: string) => invoke<void>("pause_session", { id }),
  stopSession: (id: string) => invoke<void>("stop_session", { id }),
  setMute: (id: string, muted: boolean) => invoke<void>("set_mute", { id, muted }),
  sendText: (id: string, text: string) => invoke<void>("send_text", { id, text }),
  switchInput: (id: string, device: string | null) => invoke<void>("switch_input", { id, device }),
  liveSnapshot: () => invoke<Snapshot | null>("live_snapshot"),
  listSessions: (query: string, status: string, from: number | null, to: number | null) =>
    invoke<SessionListItem[]>("list_sessions", { query, status, from, to }),
  getSession: (id: string) => invoke<SessionDetail>("get_session", { id }),
  canvasDefinition: () => invoke<StepDef[]>("canvas_definition"),
  editItem: (sessionId: string, step: string, field: string, value: string, status: FieldStatus) =>
    invoke<CanvasView>("edit_item", { sessionId, step, field, value, status }),
  confirmStep: (sessionId: string, step: string, synthesis: string) => invoke<CanvasView>("confirm_step", { sessionId, step, synthesis }),
  addAction: (sessionId: string, kind: string, content: string, owner: string, due: string) =>
    invoke<CanvasView>("add_action", { sessionId, kind, content, owner, due }),
  deleteNote: (sessionId: string, id: string) => invoke<CanvasView>("delete_note", { sessionId, id }),
  correctTurn: (id: string, text: string) => invoke<void>("correct_turn", { id, text }),
  renameSession: (id: string, title: string) => invoke<void>("rename_session", { id, title }),
  deleteSession: (id: string, keepCosts: boolean) => invoke<void>("delete_session", { id, keepCosts }),
  costOverview: () => invoke<{ total: CostSummary; month: CostSummary; setup: CostSummary; deletedSessionRows: number; settings: Settings; priceVersion: string }>("cost_overview"),
  exportSession: (id: string, format: "md" | "json", includeTranscript: boolean) =>
    invoke<string | null>("export_session", { id, format, includeTranscript }),
  exportCosts: (kind: "costs" | "metrics") => invoke<string | null>("export_costs", { kind }),
  exportDiagnostics: () => invoke<string | null>("export_diagnostics"),
};

export function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => cb(e.payload));
}

export const errText = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
