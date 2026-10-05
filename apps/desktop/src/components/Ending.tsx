import { memo } from "react";
import type { CanvasView, StepDef } from "../api";
import { ACCENT } from "./Board";

/** Render a story sentence with the "[nog open]" gaps visibly muted. */
function Sentence({ text }: { text: string }) {
  const parts = text.split("[nog open]");
  return (
    <>
      {parts.map((p, i) => (
        <span key={i}>
          {p}
          {i < parts.length - 1 && <span className="gap">nog open</span>}
        </span>
      ))}
    </>
  );
}

/** Closing slide: the whole story in five sentences, plus the first action. */
export const Ending = memo(function Ending({ title, steps, view }: { title: string; steps: StepDef[]; view: CanvasView }) {
  const decisions = view.canvas.decisions;
  const action = decisions.find((d) => d.kind === "action") ?? decisions[0];
  const open = view.steps.reduce((n, s) => n + s.missing.length, 0);
  return (
    <section className="ending" aria-label="Slotverhaal">
      <div className="chapter-kicker">Het verhaal van</div>
      <h1 className="ending-title">{title}</h1>
      <ol className="ending-lines">
        {steps.map((s, i) => (
          <li key={s.key} className={ACCENT[i]} style={{ animationDelay: `${i * 140}ms` }}>
            <span className="num-circle" aria-hidden>{i + 1}</span>
            <div>
              <div className="ending-step">{s.key}</div>
              <p><Sentence text={view.story[i] ?? ""} /></p>
            </div>
          </li>
        ))}
      </ol>
      <div className="ending-foot">
        <div className="ending-action">
          <div className="voice-kicker">Eerste actie</div>
          {action ? (
            <p>
              {action.content}
              {action.owner !== "onbekend" && <span className="hint"> · {action.owner}</span>}
              {action.due !== "onbekend" && <span className="hint"> · {action.due}</span>}
            </p>
          ) : (
            <p className="gap">nog te bepalen</p>
          )}
        </div>
        {open > 0 && <div className="hint">{open} velden staan nog open: die nemen we mee als vervolgvragen.</div>}
      </div>
      <div className="footer-line">METEN · LEREN · BIJSTUREN</div>
    </section>
  );
});
