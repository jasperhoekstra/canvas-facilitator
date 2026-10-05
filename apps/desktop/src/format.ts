export function usd(v: string | number | null | undefined, dp = 2): string {
  const n = typeof v === "number" ? v : Number(v ?? 0);
  return "$" + n.toLocaleString("nl-NL", { minimumFractionDigits: dp, maximumFractionDigits: dp });
}

export function clock(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function date(ms: number | null | undefined): string {
  if (!ms) return "–";
  return new Date(ms).toLocaleString("nl-NL", { dateStyle: "medium", timeStyle: "short" });
}

export function duration(s: { startedAt: number | null; endedAt: number | null }): string {
  if (!s.startedAt) return "–";
  return clock((s.endedAt ?? Date.now()) - s.startedAt);
}

export const SESSION_STATUS_LABEL: Record<string, string> = {
  DRAFT: "Concept",
  CONNECTING: "Verbinden",
  ACTIVE: "Actief",
  PAUSED: "Gepauzeerd",
  FINALIZING: "Afronden",
  COMPLETED: "Afgerond",
  INTERRUPTED: "Onderbroken",
  FAILED: "Mislukt",
};

export const STOP_REASON_LABEL: Record<string, string> = {
  deadline: "maximale sessieduur (4 uur) bereikt",
  gebruiker: "gestopt door jou",
  budget: "budget bereikt",
  netwerk: "netwerk",
  verbinding: "verbinding mislukt",
  klokwijziging: "klokwijziging",
  crash: "app afgesloten tijdens sessie",
  crashherstel: "hervatbaar na herstart",
  opslag: "opslagfout",
  provider: "OpenAI-fout",
  "key verwijderd": "key verwijderd",
  "app gesloten": "app gesloten",
};
