import { memo, useEffect, useRef, useState } from "react";
import { STATUS_LABEL, type Canvas, type CanvasView, type StepDef, type StepView } from "../api";

const ACCENT = ["turq", "turq", "yellow", "yellow", "yellow"];

function Card({ s, idx, canvas, sv, onOpen }: { s: StepDef; idx: number; canvas: Canvas; sv: StepView; onOpen: () => void }) {
  const items = canvas.items.filter((i) => i.step === s.key && i.value.trim()).sort((a, b) => b.updatedAt - a.updatedAt);
  const shown = items.slice(0, 5);
  const status = sv.status;
  const open = sv.missing.length + canvas.notes.filter((n) => n.step === s.key && n.kind === "challenge").length;
  const isActive = status === "actief";
  // Briefly highlight only the item that changed, not the whole card.
  const seen = useRef<Map<string, number>>(new Map());
  const [flash, setFlash] = useState<Set<string>>(new Set());
  useEffect(() => {
    const changed = new Set<string>();
    for (const i of items) {
      const prev = seen.current.get(i.id);
      if (prev !== undefined && prev !== i.revision) changed.add(i.id);
      if (prev === undefined && seen.current.size > 0) changed.add(i.id);
      seen.current.set(i.id, i.revision);
    }
    if (seen.current.size === 0) for (const i of items) seen.current.set(i.id, i.revision);
    if (changed.size) {
      setFlash(changed);
      const t = setTimeout(() => setFlash(new Set()), 1300);
      return () => clearTimeout(t);
    }
  }, [items.map((i) => `${i.id}:${i.revision}`).join("|")]);

  const label = `Stap ${idx + 1} ${s.key}, ${status}${open ? `, ${open} open punten` : ""}. Open details.`;
  return (
    <button className={`card ${ACCENT[idx]} ${isActive ? "active" : ""}`} onClick={onOpen} aria-label={label}>
      <div className="card-head">
        <span className="num-circle" aria-hidden>{idx + 1}</span>
        <div>
          <div className="card-title">{s.key}</div>
          <div className="domains">{s.domains.join(" · ")}</div>
        </div>
      </div>
      <div className="question">{s.question}</div>
      <ul className="points">
        {shown.map((i) => {
          const f = s.fields.find((x) => x.key === i.field);
          return (
            <li key={i.id} className={flash.has(i.id) ? "flash" : ""}>
              <span className="lbl">
                {f?.label}
                <span className={`st st-${i.status}`}>{STATUS_LABEL[i.status]}</span>
                {i.manual && <span className="st" title="Handmatig gecorrigeerd">✎ jij</span>}
              </span>
              <span className="val">{i.value}</span>
            </li>
          );
        })}
        {items.length > 5 && <li className="hint">+{items.length - 5} meer in detail</li>}
      </ul>
      <div className="card-foot">
        {isActive && <span className="badge act">● Actief</span>}
        {status === "voldoende uitgewerkt" && <span className="badge done">✓ Voldoende uitgewerkt</span>}
        {status === "open punten" && <span className="badge">Open punten</span>}
        {status === "niet gestart" && <span className="badge">Niet gestart</span>}
        {open > 0 && <span className="badge open">? {open} open</span>}
      </div>
    </button>
  );
}

export const Board = memo(function Board({ steps, view, onOpen }: { steps: StepDef[]; view: CanvasView; onOpen: (step: string) => void }) {
  return (
    <div className="board-wrap">
      <div className="board" role="list" aria-label="Canvas in vijf stappen">
        {steps.map((s, i) => (
          <div role="listitem" key={s.key} style={{ display: "flex", minHeight: 0 }}>
            <Card s={s} idx={i} canvas={view.canvas} sv={view.steps[i]} onOpen={() => onOpen(s.key)} />
          </div>
        ))}
      </div>
      <div className="footer-line">METEN · LEREN · BIJSTUREN</div>
    </div>
  );
});
