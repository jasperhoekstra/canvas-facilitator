import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, errText, on, type CanvasView, type Snapshot, type StepDef, type Turn } from "../api";
import { Board } from "./Board";
import { Ending } from "./Ending";
import { StepPanel } from "./StepPanel";
import { StoryView } from "./StoryView";
import { Transcript } from "./Transcript";

const VOICE_LABEL: Record<string, string> = {
  luistert: "● Luistert",
  denkt: "… Schrijft mee",
  gepauzeerd: "⏸ Gepauzeerd",
  gedempt: "🔇 Gedempt",
};

/** Subscribes to the ~10 Hz level event itself so the rest of the view doesn't re-render. */
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

export function LiveView({ title, steps, snap, view, setView, turns, setTurns, onError }: {
  title: string; steps: StepDef[]; snap: Snapshot; view: CanvasView; setView: (v: CanvasView) => void; turns: Turn[];
  setTurns: (f: (t: Turn[]) => Turn[]) => void; onError: (m: string) => void;
}) {
  const onCorrected = useCallback((t: Turn) => setTurns((ts) => ts.map((x) => (x.id === t.id ? t : x))), [setTurns]);
  const [mode, setMode] = useState<"story" | "overview" | "ending">("story");
  const [panel, setPanel] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [confirmStop, setConfirmStop] = useState(false);
  const [showTranscript, setShowTranscript] = useState(false);
  // Full-screen presentation: no top bar, controls only appear while the mouse moves.
  const [presenting, setPresenting] = useState(false);
  const [idle, setIdle] = useState(false);
  // Story focus: follows the active step until the presenter navigates (←/→ or the rail).
  const [pinned, setPinned] = useState<number | null>(null);
  const activeIdx = Math.max(0, steps.findIndex((s) => s.key === view.activeStep));
  const focus = pinned ?? activeIdx;
  const focusRef = useRef(focus);
  focusRef.current = focus;
  const last = Math.max(0, steps.length - 1);
  const id = snap.sessionId;
  const ended = ["COMPLETED", "INTERRUPTED", "FAILED"].includes(snap.status);
  const busy = snap.status === "CONNECTING" || snap.status === "FINALIZING";
  const run = (p: Promise<unknown>) => p.catch((e) => onError(errText(e)));
  const setFullscreen = (on: boolean) =>
    getCurrentWindow().setFullscreen(on).then(() => setPresenting(on)).catch((e) => onError(errText(e)));
  const presentingRef = useRef(presenting);
  presentingRef.current = presenting;
  const next = useCallback(() => {
    api.nextQuestion(id).catch((e) => onError(errText(e)));
  }, [id, onError]);

  useEffect(() => {
    document.body.classList.toggle("presenting", presenting);
    if (!presenting) return setIdle(false);
    let t = setTimeout(() => setIdle(true), 2500);
    const wake = () => {
      setIdle(false);
      clearTimeout(t);
      t = setTimeout(() => setIdle(true), 2500);
    };
    window.addEventListener("mousemove", wake);
    return () => {
      clearTimeout(t);
      window.removeEventListener("mousemove", wake);
      document.body.classList.remove("presenting");
    };
  }, [presenting]);

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).closest("input, textarea, select")) return;
      const key = e.key.toLowerCase();
      if (key === "o") setMode((m) => (m === "overview" ? "story" : "overview"));
      if (key === "s") setMode((m) => (m === "ending" ? "story" : "ending"));
      if (key === "f") setFullscreen(!presentingRef.current);
      if (key === "escape" && presentingRef.current) setFullscreen(false);
      // "N", or PageDown from a presentation clicker: next question.
      if (key === "n" || key === "pagedown") {
        e.preventDefault();
        next();
      }
      if (key === "t") setShowTranscript((s) => !s);
      if (key === "l") setPinned(null);
      if (key === "arrowright") setPinned(Math.min(last, focusRef.current + 1));
      if (key === "arrowleft") setPinned(Math.max(0, focusRef.current - 1));
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [last]);

  const pick = useCallback((i: number) => setPinned(i === activeIdx ? null : i), [activeIdx]);
  const follow = useCallback(() => setPinned(null), []);
  const actions = view.canvas.decisions;

  return (
    <div className={`live ${mode}`}>
      {mode === "ending" ? (
        <Ending title={title} steps={steps} view={view} />
      ) : mode === "story" ? (
        <StoryView
          title={title} steps={steps} view={view} turns={turns} thinking={snap.voiceState === "denkt"}
          focus={focus} following={pinned === null || pinned === activeIdx} onPick={pick} onFollow={follow} onNext={next}
        />
      ) : (
        <div className="overview">
          <div className="tagline">Vijf stappen maken van een AI-idee blijvende bedrijfswaarde.</div>
          <Board steps={steps} view={view} onOpen={setPanel} />
          {actions.length > 0 && (
            <div className="actions-strip" aria-label="Besluiten en acties">
              {actions.map((d) => (
                <span key={d.id} className="chip action">
                  <strong>{d.kind === "action" ? "Actie" : "Besluit"}:</strong> {d.content}
                  {d.owner !== "onbekend" && ` · ${d.owner}`}
                </span>
              ))}
            </div>
          )}
        </div>
      )}
      {showTranscript && <Transcript turns={turns} onCorrected={onCorrected} />}
      <div className={`controls ${presenting && idle ? "hidden" : ""}`} role="toolbar" aria-label="Bediening">
        <Meter off={snap.muted || snap.paused} />
        <span className="voice-state" aria-live="polite">{VOICE_LABEL[snap.voiceState] ?? snap.voiceState}</span>
        <div className="seg" role="group" aria-label="Weergave">
          <button className="btn small" aria-pressed={mode === "story"} onClick={() => setMode("story")}>Verhaal</button>
          <button className="btn small" aria-pressed={mode === "overview"} onClick={() => setMode("overview")}>Overzicht</button>
          <button className="btn small" aria-pressed={mode === "ending"} onClick={() => setMode("ending")} title="Slotverhaal (S)">Slot</button>
        </div>
        <button className="btn small ghost" aria-pressed={showTranscript} onClick={() => setShowTranscript(!showTranscript)}>Transcript</button>
        <button className="btn small primary" disabled={ended || busy || snap.paused} onClick={next} title="Volgende vraag (N of PageDown)">Volgende vraag ⏭</button>
        <button className="btn small ghost" onClick={() => setFullscreen(!presenting)} title="Volledig scherm (F, Esc om te sluiten)" aria-label="Volledig scherm">{presenting ? "Venster" : "⛶"}</button>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (!text.trim()) return;
            run(api.sendText(id, text).then(() => setText("")));
          }}
        >
          <input type="text" aria-label="Tekst invoeren" placeholder="Typ een antwoord…" value={text} maxLength={2000} disabled={ended || snap.paused} onChange={(e) => setText(e.target.value)} />
        </form>
        <button className="btn small" disabled={ended || busy} aria-pressed={snap.muted} onClick={() => run(api.setMute(id, !snap.muted))}>
          {snap.muted ? "Microfoon aan" : "Dempen"}
        </button>
        {snap.paused ? (
          <button className="btn small warn" disabled={ended || busy} onClick={() => run(api.resumeSession(id))}>▶ Hervat</button>
        ) : (
          <button className="btn small" disabled={ended || busy} onClick={() => run(api.pauseSession(id))}>⏸ Pauze</button>
        )}
        <button className="btn small danger" disabled={ended} onClick={() => setConfirmStop(true)}>■ Afronden</button>
      </div>
      {panel && <StepPanel steps={steps} step={panel} view={view} sessionId={id} onChange={setView} onClose={() => setPanel(null)} />}
      {confirmStop && (
        <div className="dialog" role="alertdialog" aria-modal="true" aria-labelledby="stop-t">
          <div>
            <h2 id="stop-t">Sessie afronden?</h2>
            <p>De verbinding wordt gesloten. Het canvas blijft bewaard; je kunt het daarna bekijken, corrigeren en exporteren.</p>
            <div className="row">
              <button className="btn danger" autoFocus onClick={() => { setConfirmStop(false); run(api.stopSession(id)); }}>Afronden</button>
              <button className="btn ghost" onClick={() => setConfirmStop(false)}>Doorgaan</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
