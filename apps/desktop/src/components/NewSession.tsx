import { useEffect, useState } from "react";
import { api, errText, type Session, type Settings } from "../api";
import { date, usd } from "../format";

export function NewSession({ settings, recoverable, parent, onStarted, onError }: {
  settings: Settings; recoverable: Session[]; parent: Session | null;
  onStarted: (sessionId: string) => void; onError: (m: string) => void;
}) {
  const [title, setTitle] = useState(parent ? `Vervolg: ${parent.title}` : "");
  const [style, setStyle] = useState(settings.style);
  const [profile, setProfile] = useState(settings.profile);
  const [budget, setBudget] = useState(settings.sessionBudgetUsd);
  const [est, setEst] = useState<{ low: string; high: string; model: string } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.costEstimate(profile).then(setEst).catch(() => setEst(null));
  }, [profile]);

  const start = async () => {
    setBusy(true);
    try {
      const s = await api.createSession({ title, style, profile, budgetUsd: budget, parentId: parent?.id ?? null });
      await api.startSession(s.id);
      onStarted(s.id);
    } catch (e) {
      onError(errText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page">
      {recoverable.length > 0 && (
        <div className="panel" role="region" aria-label="Herstelbare sessies">
          <h2>Onderbroken sessie hervatten</h2>
          <p className="hint">De tijd liep door tijdens de onderbreking. Hervatten kan alleen binnen de oorspronkelijke 15 minuten.</p>
          {recoverable.map((s) => (
            <div key={s.id} className="row" style={{ justifyContent: "space-between" }}>
              <span>{s.title} · gestart {date(s.startedAt)}</span>
              <button
                className="btn warn"
                onClick={() => api.resumeSession(s.id).then(() => onStarted(s.id)).catch((e) => onError(errText(e)))}
              >
                Hervat
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="panel">
        <h1>{parent ? "Vervolgsessie" : "Nieuwe sessie"}</h1>
        {parent && <p className="hint">Bevestigde canvasinhoud van "{parent.title}" wordt meegenomen. Dit is een nieuwe sessie met een eigen tijd en budget.</p>}
        <label htmlFor="ns-title">Titel / idee</label>
        <input id="ns-title" type="text" maxLength={120} value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Bijv. AI-assistent voor offerte-aanvragen" autoFocus />
        <div className="grid2">
          <div>
            <label htmlFor="ns-style">Gespreksstijl</label>
            <select id="ns-style" value={style} onChange={(e) => setStyle(e.target.value as Settings["style"])}>
              <option value="neutraal">Neutraal (standaard)</option>
              <option value="coachend">Coachend</option>
              <option value="kritisch">Kritisch</option>
            </select>
          </div>
          <div>
            <label htmlFor="ns-model">Model</label>
            <select id="ns-model" value={profile} onChange={(e) => setProfile(e.target.value as Settings["profile"])}>
              <option value="quality">Kwaliteit (gpt-realtime-2.1)</option>
              <option value="mini">Kosten (gpt-realtime-2.1-mini)</option>
            </select>
          </div>
        </div>
        <label htmlFor="ns-budget">Budget voor deze sessie (USD)</label>
        <input id="ns-budget" type="number" min="0.10" step="0.10" value={budget} onChange={(e) => setBudget(e.target.value)} style={{ maxWidth: 160 }} />
        <p className="hint">
          Lokale best-effort kostenstop met 10% marge, geen gegarandeerd factuurplafond: verbruik kan vertraagd binnenkomen en een lopend antwoord kan iets overschrijden.
        </p>
        <div className="panel" style={{ background: "rgba(12,19,38,0.6)" }}>
          <p>
            <strong>Duur: maximaal 15 minuten</strong>, inclusief pauzes. Vanaf 13:30 rondt de facilitator af; op 15:00 sluit de app de verbinding.
          </p>
          {est && (
            <p>
              Indicatie: ~{usd(est.low)} – {usd(est.high)} per kwartier met {est.model} (aannames uit het PRD; geen maximum).
            </p>
          )}
          <p>Je sessie wordt lokaal opgeslagen. Je audio en gesprekscontext worden voor verwerking naar OpenAI verstuurd.</p>
        </div>
        <button className="btn primary" disabled={busy || !title.trim() || !(Number(budget) > 0)} onClick={start}>
          {busy ? "Verbinden…" : "Start gesprek"}
        </button>
      </div>
    </div>
  );
}
