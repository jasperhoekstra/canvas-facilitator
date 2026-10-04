import { useCallback, useEffect, useState } from "react";
import { api, errText, on, type CanvasView, type Device, type Snapshot, type StepDef, type Turn } from "../api";
import { Board } from "./Board";
import { StepPanel } from "./StepPanel";
import { Transcript } from "./Transcript";

const VOICE_LABEL: Record<string, string> = {
  luistert: "🎙 Luistert",
  denkt: "… Denkt",
  spreekt: "🔊 Spreekt",
  gepauzeerd: "⏸ Gepauzeerd",
  gedempt: "🔇 Gedempt",
};

/** Subscribes to the ~10 Hz level event itself so the board and transcript don't re-render. */
function Meter({ off }: { off: boolean }) {
  const [level, setLevel] = useState(0);
  useEffect(() => {
    const un = on<number>("level", setLevel);
    return () => void un.then((f) => f());
  }, []);
  const v = off ? 0 : level;
  return (
    <div className="meter" role="meter" aria-label="Microfoonniveau" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(v * 100)}>
      <div style={{ width: `${Math.round(v * 100)}%` }} />
    </div>
  );
}

export function LiveView({ steps, snap, view, setView, turns, setTurns, onError }: {
  steps: StepDef[]; snap: Snapshot; view: CanvasView; setView: (v: CanvasView) => void; turns: Turn[];
  setTurns: (f: (t: Turn[]) => Turn[]) => void; onError: (m: string) => void;
}) {
  const onCorrected = useCallback((t: Turn) => setTurns((ts) => ts.map((x) => (x.id === t.id ? t : x))), [setTurns]);
  const [panel, setPanel] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [confirmStop, setConfirmStop] = useState(false);
  const [inputs, setInputs] = useState<Device[]>([]);
  const id = snap.sessionId;
  const ended = ["COMPLETED", "INTERRUPTED", "FAILED"].includes(snap.status);
  const busy = snap.status === "CONNECTING" || snap.status === "FINALIZING";

  useEffect(() => {
    api.audioDevices().then((d) => setInputs(d.inputs)).catch(() => {});
  }, []);

  const run = (p: Promise<unknown>) => p.catch((e) => onError(errText(e)));

  return (
    <div className="live">
      <div className="tagline">Vijf stappen maken van een AI-idee blijvende bedrijfswaarde.</div>
      <Board steps={steps} view={view} onOpen={setPanel} />
      <Transcript turns={turns} onCorrected={onCorrected} />
      <div className="controls" role="toolbar" aria-label="Gespreksbediening">
        <Meter off={snap.muted || snap.paused} />
        <span className="voice-state" aria-live="polite">{VOICE_LABEL[snap.voiceState] ?? snap.voiceState}</span>
        <span className="hint" title="Model, beurten en spreekduur jij / facilitator">
          {snap.model} · {snap.userTurns} beurten · {Math.round(snap.audioInSecs)}s / {Math.round(snap.audioOutSecs)}s
        </span>
        <button className="btn" disabled={ended || busy} aria-pressed={snap.muted} onClick={() => run(api.setMute(id, !snap.muted))}>
          {snap.muted ? "🔇 Microfoon aan" : "🎙 Dempen"}
        </button>
        {snap.paused ? (
          <button className="btn warn" disabled={ended || busy} onClick={() => run(api.resumeSession(id))}>▶ Hervat</button>
        ) : (
          <button className="btn" disabled={ended || busy} onClick={() => run(api.pauseSession(id))}>⏸ Pauze</button>
        )}
        <button className="btn danger" disabled={ended} onClick={() => setConfirmStop(true)}>■ Stop</button>
        <select
          aria-label="Microfoon"
          style={{ maxWidth: 200 }}
          disabled={ended}
          onChange={(e) => run(api.switchInput(id, e.target.value || null))}
          defaultValue=""
        >
          <option value="">Microfoon wisselen…</option>
          {inputs.map((d) => (
            <option key={d.id} value={d.id}>{d.name}</option>
          ))}
        </select>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (!text.trim()) return;
            run(api.sendText(id, text).then(() => setText("")));
          }}
        >
          <input type="text" aria-label="Tekst invoeren" placeholder="Of typ je antwoord…" value={text} maxLength={2000} disabled={ended || snap.paused} onChange={(e) => setText(e.target.value)} />
          <button className="btn" disabled={ended || snap.paused || !text.trim()}>Verstuur</button>
        </form>
      </div>
      {panel && <StepPanel steps={steps} step={panel} view={view} sessionId={id} onChange={setView} onClose={() => setPanel(null)} />}
      {confirmStop && (
        <div className="dialog" role="alertdialog" aria-modal="true" aria-labelledby="stop-t">
          <div>
            <h2 id="stop-t">Sessie stoppen?</h2>
            <p>De verbinding wordt gesloten. Je kunt het canvas daarna lokaal bekijken, corrigeren en exporteren. Deze sessie kan niet opnieuw worden gestart.</p>
            <div className="row">
              <button className="btn danger" autoFocus onClick={() => { setConfirmStop(false); run(api.stopSession(id)); }}>Stop sessie</button>
              <button className="btn ghost" onClick={() => setConfirmStop(false)}>Doorgaan</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
