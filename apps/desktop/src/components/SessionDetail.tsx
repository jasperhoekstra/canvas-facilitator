import { useEffect, useState } from "react";
import { api, errText, type Session, type SessionDetail as Detail, type StepDef } from "../api";
import { date, duration, SESSION_STATUS_LABEL, STOP_REASON_LABEL, usd } from "../format";
import { Board } from "./Board";
import { CostDetail } from "./Costs";
import { StepPanel } from "./StepPanel";
import { Transcript } from "./Transcript";

export function SessionDetail({ id, steps, onBack, onFollowUp, onError, onInfo }: {
  id: string; steps: StepDef[]; onBack: () => void; onFollowUp: (s: Session) => void; onError: (m: string) => void; onInfo: (m: string) => void;
}) {
  const [d, setD] = useState<Detail | null>(null);
  const [panel, setPanel] = useState<string | null>(null);
  const [withTranscript, setWithTranscript] = useState(true);
  const [del, setDel] = useState(false);
  const [keepCosts, setKeepCosts] = useState(true);
  const [title, setTitle] = useState("");

  const load = () =>
    api.getSession(id).then((x) => {
      setD(x);
      setTitle(x.session.title);
    }).catch((e) => onError(errText(e)));
  useEffect(() => {
    load();
  }, [id]);
  if (!d) return <p className="hint" style={{ padding: 24 }}>Laden…</p>;
  const s = d.session;
  const m = JSON.parse(s.metricsJson || "{}");

  const exp = async (format: "md" | "json") => {
    try {
      const p = await api.exportSession(id, format, withTranscript);
      if (p) onInfo(`Opgeslagen als ${p}. Let op: exportbestanden zijn niet versleuteld.`);
    } catch (e) {
      onError(errText(e));
    }
  };

  return (
    <div className="page" style={{ maxWidth: 1400 }}>
      <div className="row" style={{ justifyContent: "space-between", marginBottom: 12 }}>
        <button className="btn ghost" onClick={onBack}>← Sessies</button>
        <div className="row">
          <button className="btn" onClick={() => onFollowUp(s)}>Verder uitwerken (nieuwe sessie)</button>
          <button className="btn danger" onClick={() => setDel(true)}>Verwijderen</button>
        </div>
      </div>
      <div className="panel">
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            api.renameSession(id, title).then(load).catch((x) => onError(errText(x)));
          }}
        >
          <input type="text" aria-label="Titel" value={title} onChange={(e) => setTitle(e.target.value)} style={{ flex: 1, fontSize: 20, fontWeight: 700 }} />
          {title !== s.title && <button className="btn small">Hernoem</button>}
        </form>
        <p className="hint">
          {date(s.startedAt ?? s.createdAt)} · duur {duration(s)} · {SESSION_STATUS_LABEL[s.status]}
          {s.stopReason ? ` (${STOP_REASON_LABEL[s.stopReason] ?? s.stopReason})` : ""} · {s.model} · stijl {s.style} · prijsversie {s.priceVersion}
        </p>
        <p className="hint">Klik op een kaart om te corrigeren. Na afsluiten werkt dit offline; een nieuwe AI-vraag vereist een nieuwe sessie.</p>
      </div>
      <div style={{ height: "52vh", display: "flex", flexDirection: "column", marginBottom: 16 }}>
        <Board steps={steps} view={d.view} onOpen={setPanel} />
      </div>
      <div className="grid2">
        <div className="panel">
          <h2>Exporteren</h2>
          <label className="row" style={{ fontWeight: 400 }}>
            <input type="checkbox" checked={withTranscript} onChange={(e) => setWithTranscript(e.target.checked)} /> Transcript meenemen
          </label>
          <div className="row">
            <button className="btn" onClick={() => exp("md")}>Markdown</button>
            <button className="btn" onClick={() => exp("json")}>JSON</button>
          </div>
          <p className="hint">Exports zijn bewust gekozen en onversleuteld.</p>
        </div>
        <div className="panel">
          <h2>Gebruik</h2>
          <table>
            <tbody>
              <tr><td>Actieve/pauzetijd</td><td className="num">{Math.round((m.elapsedMs ?? 0) / 1000)} s / {Math.round((m.pausedMs ?? 0) / 1000)} s</td></tr>
              <tr><td>Spreektijd jij / facilitator</td><td className="num">{Math.round((m.userSpeechMs ?? 0) / 1000)} s / {Math.round((m.assistantAudioMs ?? 0) / 1000)} s</td></tr>
              <tr><td>Verzonden audio (per stream)</td><td className="num">{(m.streamedAudioSecs ?? 0).toFixed(1)} s</td></tr>
              <tr><td>Beurten jij / facilitator / getypt</td><td className="num">{m.userTurns ?? 0} / {m.assistantTurns ?? 0} / {m.textTurns ?? 0}</td></tr>
              <tr><td>Responses (geannuleerd)</td><td className="num">{m.responses ?? 0} ({m.cancelledResponses ?? 0})</td></tr>
              <tr><td>Tools (geweigerd)</td><td className="num">{d.toolCalls} ({d.toolRejections})</td></tr>
              <tr><td>Challenges / besluiten</td><td className="num">{d.view.canvas.notes.filter((n) => n.kind === "challenge").length} / {d.view.canvas.decisions.length}</td></tr>
              <tr><td>Open canvasvelden</td><td className="num">{d.view.steps.reduce((a, x) => a + x.missing.length, 0)}</td></tr>
              <tr><td>Onderbrekingen / reconnects / fouten</td><td className="num">{m.bargeIns ?? 0} / {m.reconnects ?? 0} / {m.errors ?? 0}</td></tr>
              <tr><td>Latency p50/p95 eerste transcript</td><td className="num">{m.p50FirstDeltaMs ?? "–"} / {m.p95FirstDeltaMs ?? "–"} ms</td></tr>
              <tr><td>Latency p50/p95 definitief transcript</td><td className="num">{m.p50FinalTranscriptMs ?? "–"} / {m.p95FinalTranscriptMs ?? "–"} ms</td></tr>
              <tr><td>Latency p50/p95 eerste audio</td><td className="num">{m.p50FirstAudioMs ?? "–"} / {m.p95FirstAudioMs ?? "–"} ms</td></tr>
            </tbody>
          </table>
        </div>
      </div>
      <div className="panel">
        <h2>Kosten ~{usd(d.cost.totalUsd, 4)}</h2>
        <CostDetail c={d.cost} />
      </div>
      <Transcript turns={d.turns} maxHeight="50vh" onCorrected={(t) => setD({ ...d, turns: d.turns.map((x) => (x.id === t.id ? t : x)) })} />
      {panel && <StepPanel steps={steps} step={panel} view={d.view} sessionId={id} onChange={(v) => setD({ ...d, view: v })} onClose={() => setPanel(null)} />}
      {del && (
        <div className="dialog" role="alertdialog" aria-modal="true" aria-labelledby="del-t">
          <div>
            <h2 id="del-t">Sessie verwijderen?</h2>
            <p>Canvas, transcript en lokale herstelgegevens van deze sessie worden verwijderd. Eerder gemaakte exports blijven bestaan.</p>
            <label className="row" style={{ fontWeight: 400 }}>
              <input type="checkbox" checked={keepCosts} onChange={(e) => setKeepCosts(e.target.checked)} />
              Anonieme kostenboekhouding behouden (telt mee in totalen)
            </label>
            <div className="row">
              <button className="btn danger" onClick={() => api.deleteSession(id, keepCosts).then(onBack).catch((e) => onError(errText(e)))}>Verwijder</button>
              <button className="btn ghost" autoFocus onClick={() => setDel(false)}>Annuleer</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
