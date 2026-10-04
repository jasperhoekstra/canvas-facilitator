import { useEffect, useRef, useState } from "react";
import { api, errText, FIELD_STATUSES, STATUS_LABEL, type Canvas, type CanvasView, type FieldStatus, type StepDef } from "../api";

function FieldEditor({ sessionId, step, f, canvas, onChange }: {
  sessionId: string; step: string; f: StepDef["fields"][number]; canvas: Canvas; onChange: (v: CanvasView) => void;
}) {
  const item = canvas.items.find((i) => i.step === step && i.field === f.key);
  const [value, setValue] = useState(item?.value ?? "");
  const [status, setStatus] = useState<FieldStatus>(item?.status ?? "PARTIAL");
  const [err, setErr] = useState("");
  // Follow live updates unless the user is editing this field.
  const dirty = value !== (item?.value ?? "") || status !== (item?.status ?? "PARTIAL");
  const editing = useRef(false);
  useEffect(() => {
    if (!editing.current) {
      setValue(item?.value ?? "");
      setStatus(item?.status ?? "PARTIAL");
    }
  }, [item?.revision]);
  const id = `f-${step}-${f.key}`;
  return (
    <div className="field-edit">
      <label htmlFor={id}>
        {f.label} <span className="hint">({f.domain}{item ? `, rev ${item.revision}` : ""}{item?.manual ? ", door jou gecorrigeerd" : ""})</span>
      </label>
      <textarea
        id={id}
        value={value}
        maxLength={500}
        onFocus={() => (editing.current = true)}
        onBlur={() => (editing.current = dirty)}
        onChange={(e) => setValue(e.target.value)}
      />
      {item?.evidence && <div className="hint">Onderbouwing: {item.evidence}</div>}
      <div className="row" style={{ marginTop: 6 }}>
        <select aria-label={`Status ${f.label}`} value={status} onChange={(e) => setStatus(e.target.value as FieldStatus)} style={{ maxWidth: 200 }}>
          {FIELD_STATUSES.map((s) => (
            <option key={s} value={s}>{STATUS_LABEL[s]}</option>
          ))}
        </select>
        <button
          className="btn small primary"
          disabled={!dirty}
          onClick={async () => {
            try {
              onChange(await api.editItem(sessionId, step, f.key, value, status));
              editing.current = false;
              setErr("");
            } catch (e) {
              setErr(errText(e));
            }
          }}
        >
          Corrigeer
        </button>
        {err && <span className="error" role="alert">{err}</span>}
      </div>
    </div>
  );
}

export function StepPanel({ steps, step, view, sessionId, onChange, onClose }: {
  steps: StepDef[]; step: string; view: CanvasView; sessionId: string; onChange: (v: CanvasView) => void; onClose: () => void;
}) {
  const canvas = view.canvas;
  const s = steps.find((x) => x.key === step)!;
  const idx = steps.indexOf(s);
  const close = useRef<HTMLButtonElement>(null);
  const [synth, setSynth] = useState(canvas.completed.find((c) => c.step === step)?.synthesis ?? "");
  const [err, setErr] = useState("");
  const [act, setAct] = useState({ kind: "action", content: "", owner: "", due: "" });
  useEffect(() => {
    close.current?.focus();
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);
  const missing = view.steps[idx].missing;
  const done = canvas.completed.find((c) => c.step === step);
  const notes = canvas.notes.filter((n) => n.step === step);

  return (
    <>
      <div className="backdrop" onClick={onClose} />
      <aside className="side" role="dialog" aria-modal="true" aria-labelledby="side-title">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h2 id="side-title">{idx + 1}. {s.key}</h2>
          <button ref={close} className="btn ghost" onClick={onClose} aria-label="Sluit detailpaneel">✕</button>
        </div>
        <p className="hint">{s.question} · {s.domains.join(", ")}</p>

        {s.fields.map((f) => (
          <FieldEditor key={f.key} sessionId={sessionId} step={step} f={f} canvas={canvas} onChange={onChange} />
        ))}

        <h2 style={{ marginTop: 16 }}>Aannames en challenges</h2>
        {notes.length === 0 && <p className="hint">Nog geen.</p>}
        <ul>
          {notes.map((n) => (
            <li key={n.id} className="row" style={{ justifyContent: "space-between" }}>
              <span><strong>{n.kind === "assumption" ? "Aanname" : "Challenge"}:</strong> {n.text}</span>
              <button className="btn small ghost" aria-label="Verwijder" onClick={async () => onChange(await api.deleteNote(sessionId, n.id))}>✕</button>
            </li>
          ))}
        </ul>

        <h2 style={{ marginTop: 16 }}>Synthese</h2>
        {done ? (
          <p className="okText">✓ Bevestigd als voldoende uitgewerkt{done.synthesis ? `: ${done.synthesis}` : ""}</p>
        ) : (
          <>
            {missing.length > 0 && <p className="hint">Ontbreekt nog: {missing.join(", ")}</p>}
            <textarea aria-label="Synthese van deze stap" value={synth} maxLength={500} onChange={(e) => setSynth(e.target.value)} placeholder="Korte samenvatting die je bevestigt" />
            <div className="row" style={{ marginTop: 6 }}>
              <button
                className="btn primary"
                disabled={missing.length > 0}
                onClick={async () => {
                  try {
                    onChange(await api.confirmStep(sessionId, step, synth));
                    setErr("");
                  } catch (e) {
                    setErr(errText(e));
                  }
                }}
              >
                Bevestig: voldoende uitgewerkt
              </button>
              {err && <span className="error" role="alert">{err}</span>}
            </div>
          </>
        )}

        <h2 style={{ marginTop: 16 }}>Besluiten en acties</h2>
        <ul>
          {canvas.decisions.map((d) => (
            <li key={d.id} className="row" style={{ justifyContent: "space-between" }}>
              <span>
                <strong>{d.kind === "action" ? "Actie" : "Besluit"}:</strong> {d.content} <span className="hint">(eigenaar: {d.owner}, termijn: {d.due})</span>
              </span>
              <button className="btn small ghost" aria-label="Verwijder" onClick={async () => onChange(await api.deleteNote(sessionId, d.id))}>✕</button>
            </li>
          ))}
        </ul>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            try {
              onChange(await api.addAction(sessionId, act.kind, act.content, act.owner, act.due));
              setAct({ ...act, content: "", owner: "", due: "" });
            } catch (x) {
              setErr(errText(x));
            }
          }}
        >
          <div className="row">
            <select aria-label="Soort" value={act.kind} onChange={(e) => setAct({ ...act, kind: e.target.value })} style={{ maxWidth: 140 }}>
              <option value="action">Actie</option>
              <option value="decision">Besluit</option>
            </select>
            <input type="text" aria-label="Inhoud" placeholder="Inhoud" value={act.content} onChange={(e) => setAct({ ...act, content: e.target.value })} style={{ flex: 2 }} />
          </div>
          <div className="row" style={{ marginTop: 6 }}>
            <input type="text" aria-label="Eigenaar" placeholder="Eigenaar (leeg = onbekend)" value={act.owner} onChange={(e) => setAct({ ...act, owner: e.target.value })} style={{ flex: 1 }} />
            <input type="text" aria-label="Termijn" placeholder="Termijn (leeg = onbekend)" value={act.due} onChange={(e) => setAct({ ...act, due: e.target.value })} style={{ flex: 1 }} />
            <button className="btn" disabled={!act.content.trim()}>Toevoegen</button>
          </div>
        </form>
      </aside>
    </>
  );
}
