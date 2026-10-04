import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, errText, type Turn } from "../api";

function TurnLine({ t, onCorrected }: { t: Turn; onCorrected: (t: Turn) => void }) {
  const [edit, setEdit] = useState<string | null>(null);
  const [err, setErr] = useState("");
  const who = t.speaker === "user" ? "Jij" : "Facilitator";
  if (edit !== null) {
    return (
      <form
        className="turn row"
        onSubmit={async (e) => {
          e.preventDefault();
          try {
            await api.correctTurn(t.id, edit);
            onCorrected({ ...t, originalText: t.originalText ?? t.text, text: edit, correctedAt: Date.now() });
            setEdit(null);
          } catch (x) {
            setErr(errText(x));
          }
        }}
      >
        <input type="text" aria-label="Corrigeer transcript" value={edit} onChange={(e) => setEdit(e.target.value)} style={{ flex: 1 }} autoFocus />
        <button className="btn small primary">Opslaan</button>
        <button type="button" className="btn small ghost" onClick={() => setEdit(null)}>Annuleer</button>
        {err && <span className="error">{err}</span>}
      </form>
    );
  }
  const interrupted = t.interrupted && t.spokenText !== null;
  return (
    <div className={`turn ${t.speaker} ${t.final ? "" : "provisional"}`}>
      <span className="who">{who}:</span>
      {interrupted ? (
        <>
          <span className="txt">{t.spokenText}</span>{" "}
          <span className="cut" aria-label="Niet uitgesproken">{t.text.slice(t.spokenText!.length)}</span>
          <span className="flag">[afgebroken]</span>
        </>
      ) : (
        <span className="txt">{t.text || (t.final ? "" : "…")}</span>
      )}
      {!t.final && <span className="sr-only"> (voorlopig)</span>}
      {t.failed && <span className="flag">[transcript tijdelijk onvolledig]</span>}
      {t.correctedAt && <span className="flag" title={`Oorspronkelijk: ${t.originalText ?? ""}`}>[gecorrigeerd]</span>}
      {t.final && t.speaker === "user" && (
        <button className="btn small ghost" style={{ minHeight: 28, marginLeft: 6 }} aria-label="Corrigeer deze beurt" onClick={() => setEdit(t.text)}>
          ✎
        </button>
      )}
    </div>
  );
}

export const Transcript = memo(function Transcript({ turns, onCorrected, startOpen = true, maxHeight }: {
  turns: Turn[]; onCorrected: (t: Turn) => void; startOpen?: boolean; maxHeight?: string;
}) {
  const [open, setOpen] = useState(startOpen);
  const body = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  const shown = turns.filter((t) => t.text || !t.final);
  useLayoutEffect(() => {
    if (follow.current && body.current) body.current.scrollTop = body.current.scrollHeight;
  }, [shown.length, shown[shown.length - 1]?.text]);
  useEffect(() => {
    follow.current = true;
  }, [open]);
  return (
    <section className="transcript" aria-label="Live transcript">
      <div className="transcript-head">
        <button className="btn small ghost" aria-expanded={open} onClick={() => setOpen(!open)}>
          {open ? "▾" : "▸"} Transcript
        </button>
        <span className="hint">Jij / Facilitator · grijs cursief = voorlopig</span>
      </div>
      {open && (
        <div
          className="transcript-body"
          ref={body}
          style={maxHeight ? { maxHeight } : undefined}
          tabIndex={0}
          onScroll={(e) => {
            const el = e.currentTarget;
            // Stop autoscroll as soon as the user reads back; resume at the bottom.
            follow.current = el.scrollTop + el.clientHeight >= el.scrollHeight - 24;
          }}
        >
          {shown.length === 0 && <p className="hint">Nog geen gesprek.</p>}
          {shown.map((t) => (
            <TurnLine key={t.id} t={t} onCorrected={onCorrected} />
          ))}
        </div>
      )}
    </section>
  );
});
