import { useCallback, useEffect, useRef, useState } from "react";
import { api, errText, on, type AppStatus, type CanvasView, type Notice, type Session, type Snapshot, type StepDef, type Turn } from "./api";
import { clock, STOP_REASON_LABEL, usd } from "./format";
import { Costs } from "./components/Costs";
import { History } from "./components/History";
import { LiveView } from "./components/LiveView";
import { NewSession } from "./components/NewSession";
import { SessionDetail } from "./components/SessionDetail";
import { SettingsPage } from "./components/Settings";
import { Setup } from "./components/Setup";

type View = "session" | "history" | "costs" | "settings";
const EMPTY: CanvasView = { canvas: { items: [], notes: [], decisions: [], completed: [] }, steps: [], activeStep: null };
const LIVE_STATES = ["CONNECTING", "ACTIVE", "PAUSED", "FINALIZING"];

export function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [steps, setSteps] = useState<StepDef[]>([]);
  const [view, setView] = useState<View>("session");
  const [detail, setDetail] = useState<string | null>(null);
  const [parent, setParent] = useState<Session | null>(null);
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [title, setTitle] = useState("");
  const [cview, setCView] = useState<CanvasView>(EMPTY);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [notices, setNotices] = useState<(Notice & { id: number })[]>([]);
  const sid = useRef<string | null>(null);
  const nid = useRef(0);

  const notify = useCallback((n: Notice) => {
    const id = ++nid.current;
    setNotices((ns) => [...ns.slice(-3), { ...n, id }]);
    if (n.level === "info") setTimeout(() => setNotices((ns) => ns.filter((x) => x.id !== id)), 6000);
  }, []);
  const onError = useCallback((text: string) => notify({ level: "error", text }), [notify]);
  const onInfo = useCallback((text: string) => notify({ level: "info", text }), [notify]);

  const refresh = useCallback(() => api.appStatus().then(setStatus).catch((e) => onError(errText(e))), []);

  const attach = useCallback(async (id: string) => {
    sid.current = id;
    const d = await api.getSession(id);
    setTitle(d.session.title);
    setCView(d.view);
    setTurns(d.turns);
    setSnap(await api.liveSnapshot());
    setDetail(null);
    setView("session");
  }, []);

  useEffect(() => {
    refresh().then(() => {});
    api.canvasDefinition().then(setSteps);
    const subs = [
      on<Snapshot>("live", (s) => s.sessionId === sid.current && setSnap(s)),
      on<{ sessionId: string; view: CanvasView }>("canvas", (c) => c.sessionId === sid.current && setCView(c.view)),
      on<Turn>("turn", (t) => {
        if (t.sessionId !== sid.current) return;
        setTurns((ts) => {
          const i = ts.findIndex((x) => x.id === t.id);
          if (i < 0) return [...ts, t].sort((a, b) => a.seq - b.seq);
          const n = ts.slice();
          n[i] = t;
          return n;
        });
      }),
      on<string>("turn-removed", (id) => setTurns((ts) => ts.filter((t) => t.id !== id || t.final || t.text))),
      on<Notice>("notice", notify),
      on<{ sessionId: string; status: string; reason: string }>("ended", (e) => {
        if (e.sessionId !== sid.current) return;
        notify({ level: "info", text: `Sessie afgesloten: ${STOP_REASON_LABEL[e.reason] ?? e.reason}. Het canvas staat lokaal klaar.` });
        sid.current = null;
        setSnap(null);
        setDetail(e.sessionId);
        setView("history");
        refresh();
      }),
      on<{ text: string }>("warning", (w) => notify({ level: "warn", text: w.text })),
    ];
    return () => subs.forEach((p) => p.then((f) => f()));
  }, []);

  // Re-attach after a renderer reload while a native session is still running.
  useEffect(() => {
    if (status?.live && !sid.current && LIVE_STATES.includes(status.live.status)) attach(status.live.sessionId);
  }, [status?.live?.sessionId]);

  if (!status) return <p className="hint" style={{ padding: 24 }}>Laden…</p>;

  if (status.storageError) {
    return (
      <main className="wizard">
        <div className="panel" role="alert">
          <h1>Lokale opslag niet beschikbaar</h1>
          <p>{status.storageError}</p>
          <p className="hint">
            De app bewaart sessies alleen versleuteld, met een sleutel in de beveiligde sleutelbos van je systeem. Ontgrendel de sleutelbos (macOS Sleutelhangertoegang /
            Windows Referentiebeheer) en probeer opnieuw. Er is geen onversleutelde terugvaloptie.
          </p>
          <button className="btn primary" onClick={() => api.retryStorage().then(refresh).catch((e) => onError(errText(e)))}>Opnieuw proberen</button>
        </div>
      </main>
    );
  }

  if (!status.keyPresent || !status.settings?.onboarded) {
    return <Setup status={status} refresh={refresh} onDone={refresh} />;
  }

  const live = snap && LIVE_STATES.includes(snap.status) ? snap : null;
  const lowTime = live && live.remainingMs <= 180_000;
  const nav = (v: View) => {
    setView(v);
    setDetail(null);
    if (v !== "session") setParent(null);
    if (v === "session") refresh();
  };

  return (
    <div className="app">
      <header className="topbar">
        <span className="brand">Canvas Facilitator</span>
        {live && (
          <>
            <span className="sep">|</span>
            <span className="title" title={title}>{title}</span>
            <span className={`pill ${live.connection === "verbonden" ? "turq" : "yellow"}`} aria-label={`Verbinding: ${live.connection}`}>
              {live.connection === "verbonden" ? "●" : "◌"} {live.connection}
            </span>
            <span className={`pill ${lowTime ? "yellow" : ""}`} role="timer" aria-label={`Resterende tijd ${clock(live.remainingMs)}`}>
              Resterend <span className="timer">{clock(live.remainingMs)}</span>
              {live.phase === "SYNTH" && " · synthese"}
            </span>
            <span className="pill" aria-label="Geschatte kosten">
              Kosten ~{usd(live.costUsd)}{live.costIncomplete ? "*" : ""} / {usd(live.budgetUsd)}
            </span>
          </>
        )}
        <span className="spacer" />
        <nav className="mainnav" aria-label="Hoofdnavigatie">
          <button className="btn small" aria-current={view === "session" && !detail ? "page" : undefined} onClick={() => nav("session")}>
            {live ? "Gesprek" : "Nieuw"}
          </button>
          <button className="btn small" aria-current={view === "history" || detail ? "page" : undefined} onClick={() => nav("history")}>Sessies</button>
          <button className="btn small" aria-current={view === "costs" ? "page" : undefined} onClick={() => nav("costs")}>Kosten</button>
          <button className="btn small" aria-current={view === "settings" ? "page" : undefined} onClick={() => nav("settings")}>Instellingen</button>
        </nav>
      </header>

      {view === "session" && live && !detail ? (
        <LiveView steps={steps} snap={live} view={cview} setView={setCView} turns={turns} setTurns={setTurns} onError={onError} />
      ) : (
        <main>
          {detail ? (
            <SessionDetail
              id={detail}
              steps={steps}
              onBack={() => setDetail(null)}
              onFollowUp={(s) => { setParent(s); setDetail(null); setView("session"); }}
              onError={onError}
              onInfo={onInfo}
            />
          ) : view === "session" ? (
            <NewSession key={parent?.id ?? "new"} settings={status.settings!} recoverable={status.recoverable} parent={parent} onStarted={(id) => { setParent(null); attach(id); }} onError={onError} />
          ) : view === "history" ? (
            <History onOpen={setDetail} onError={onError} />
          ) : view === "costs" ? (
            <Costs onError={onError} onInfo={onInfo} />
          ) : (
            <SettingsPage keyMask={status.keyMask} refresh={refresh} onError={onError} onInfo={onInfo} version={status.version} />
          )}
        </main>
      )}

      <div className="notices" aria-live="assertive">
        {notices.map((n) => (
          <div key={n.id} className={`notice ${n.level}`} role={n.level === "error" ? "alert" : "status"}>
            <span className="grow">{n.text}</span>
            <button className="btn small ghost" aria-label="Sluit melding" onClick={() => setNotices((ns) => ns.filter((x) => x.id !== n.id))}>✕</button>
          </div>
        ))}
      </div>
    </div>
  );
}
