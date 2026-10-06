import { memo } from "react";
import { NOTE_LABEL, STATUS_LABEL, type CanvasView, type StepDef, type Turn } from "../api";
import { ACCENT } from "./Board";

/** Story line: the chapters, the one in focus highlighted. */
function Rail({ steps, view, focus, onPick }: { steps: StepDef[]; view: CanvasView; focus: number; onPick: (i: number) => void }) {
  return (
    <nav className="rail" aria-label="Verhaallijn">
      {steps.map((s, i) => {
        const st = view.steps[i]?.status ?? "niet gestart";
        return (
          <button
            key={s.key}
            className={`rail-stop ${ACCENT[i]} ${i === focus ? "focus" : ""} ${st === "voldoende uitgewerkt" ? "done" : ""} ${st === "niet gestart" ? "todo" : ""}`}
            onClick={() => onPick(i)}
            aria-current={i === focus ? "step" : undefined}
            aria-label={`Hoofdstuk ${i + 1}: ${s.key}, ${st}`}
          >
            <span className="rail-dot" aria-hidden>{st === "voldoende uitgewerkt" ? "✓" : i + 1}</span>
            <span className="rail-label">{s.key}</span>
          </button>
        );
      })}
    </nav>
  );
}

/** One chapter: the step's fields fill in as the story is told. */
function Chapter({ s, idx, total, view }: { s: StepDef; idx: number; total: number; view: CanvasView }) {
  const c = view.canvas;
  const notes = c.notes.filter((n) => n.step === s.key);
  return (
    <section className={`chapter ${ACCENT[idx]}`} aria-live="polite">
      <div className="chapter-kicker">Hoofdstuk {idx + 1} van {total} · {s.domains.join(" · ")}</div>
      <h1 className="chapter-title">{s.key}</h1>
      <p className="chapter-q">{s.question}</p>
      <div className="tiles">
        {s.fields.map((f) => {
          const it = c.items.find((i) => i.step === s.key && i.field === f.key && i.value.trim());
          // The fill-in animation plays when the tile switches from empty to filled.
          return (
            <div key={f.key} className={`tile ${it ? "filled" : "empty"}`}>
              <div className="tile-label">
                {f.label}
                {it && <span className={`st st-${it.status}`}>{STATUS_LABEL[it.status]}</span>}
              </div>
              <div className="tile-value">{it ? it.value : "nog open"}</div>
            </div>
          );
        })}
      </div>
      {notes.length > 0 && (
        <div className="chips">
          {notes.map((n) => (
            <span key={n.id} className={`chip ${n.kind}`}>{NOTE_LABEL[n.kind]}: {n.text}</span>
          ))}
        </div>
      )}
    </section>
  );
}

/** Presentation mode: chapter in focus, the facilitator's question as a quote, live subtitles.
 *  `focus` is driven by LiveView (auto-follows the active step unless the presenter navigates). */
export const StoryView = memo(function StoryView({ title, steps, view, turns, thinking, focus, following, onPick, onFollow, onNext }: {
  title: string; steps: StepDef[]; view: CanvasView; turns: Turn[]; thinking: boolean; focus: number; following: boolean;
  onPick: (i: number) => void; onFollow: () => void; onNext: () => void;
}) {
  if (!steps.length) return null;
  const questions = turns.filter((t) => t.speaker === "assistant" && t.text.trim());
  const question = questions.at(-1);
  const earlier = questions.slice(-3, -1).reverse();
  const said = turns.findLast((t) => t.speaker === "user" && t.text.trim());

  // Opening slide until the story has started (no words from the presenter, empty canvas).
  if (!said && view.canvas.items.length === 0) {
    return (
      <div className="intro">
        <div className="chapter-kicker">Vijf stappen maken van een AI-idee blijvende bedrijfswaarde</div>
        <h1 className="intro-title">{title}</h1>
        <Rail steps={steps} view={view} focus={-1} onPick={onPick} />
        <blockquote key={question?.id ?? "intro"} className="quote intro-quote">
          {question ? question.text : "Vertel over je AI-idee: welk probleem wil je oplossen, en voor wie?"}
        </blockquote>
        <p className="hint">Begin gewoon met vertellen; het canvas vult zich terwijl je praat.</p>
      </div>
    );
  }

  return (
    <div className="story">
      <Rail steps={steps} view={view} focus={focus} onPick={onPick} />
      <div className="story-main">
        <Chapter key={steps[focus].key} s={steps[focus]} idx={focus} total={steps.length} view={view} />
        <aside className="voice">
          <div className="voice-kicker">De facilitator vraagt</div>
          <blockquote key={question?.id ?? "intro"} className="quote">
            {question ? question.text : "Vertel over je AI-idee: welk probleem wil je oplossen, en voor wie?"}
          </blockquote>
          {thinking && <div className="thinking" aria-label="Facilitator schrijft mee"><span /><span /><span /></div>}
          <button className="next-hint" onClick={onNext}>Druk <kbd>N</kbd> voor de volgende vraag</button>
          {said && (
            <div className={`subtitle ${said.final ? "" : "provisional"}`}>
              <span className="who">Jij</span> {said.text}
            </div>
          )}
          {earlier.length > 0 && (
            <div className="earlier" aria-label="Eerder gevraagd">
              <div className="tile-label">Eerder gevraagd</div>
              {earlier.map((q) => <p key={q.id}>{q.text}</p>)}
            </div>
          )}
          {!following && <button className="btn small ghost follow" onClick={onFollow}>↻ Volg live (L)</button>}
        </aside>
      </div>
    </div>
  );
});
