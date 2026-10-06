import { memo } from "react";
import { type CanvasView, type FieldRef, type Guide, type StepDef, type Turn } from "../api";
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

/** One chapter: the step's tiles fill in as the story is told; the ones the question is about glow. */
function Chapter({ s, idx, total, view, working }: { s: StepDef; idx: number; total: number; view: CanvasView; working: FieldRef[] }) {
  const c = view.canvas;
  return (
    <section className={`chapter ${ACCENT[idx]}`} aria-live="polite">
      <div className="chapter-kicker">Hoofdstuk {idx + 1} van {total} · {s.domains.join(" · ")}</div>
      <h1 className="chapter-title">{s.key}</h1>
      <p className="chapter-q">{s.question}</p>
      <div className="tiles">
        {s.fields.map((f) => {
          const it = c.items.find((i) => i.step === s.key && i.field === f.key && i.value.trim());
          const glow = working.some((w) => w.step === s.key && w.field === f.key);
          // The fill-in animation plays when the tile switches from empty to filled.
          return (
            <div key={f.key} className={`tile ${it ? "filled" : "empty"} ${glow ? "working" : ""}`}>
              <div className="tile-label">{f.label}</div>
              <div className="tile-value">{it ? it.value : "nog open"}</div>
            </div>
          );
        })}
      </div>
    </section>
  );
}

/** Presentation mode: chapter in focus, the facilitator's question as a quote, live subtitles.
 *  `focus` is driven by LiveView (auto-follows the active step unless the presenter navigates). */
export const StoryView = memo(function StoryView({ title, steps, view, turns, thinking, focus, following, onPick, onFollow, onNext, guide, ready, onInspire, onDeepen }: {
  title: string; steps: StepDef[]; view: CanvasView; turns: Turn[]; thinking: boolean; focus: number; following: boolean;
  onPick: (i: number) => void; onFollow: () => void; onNext: () => void;
  guide: Guide | null; ready: string | null; onInspire: () => void; onDeepen: () => void;
}) {
  if (!steps.length) return null;
  const questions = turns.filter((t) => t.speaker === "assistant" && t.text.trim());
  const question = questions.at(-1);
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
        <Chapter
          key={steps[focus].key} s={steps[focus]} idx={focus} total={steps.length} view={view}
          working={guide?.fields ?? []}
        />
        <aside className="voice">
          <div className="voice-kicker">De facilitator vraagt</div>
          <blockquote key={question?.id ?? "intro"} className="quote">
            {question ? question.text : "Vertel over je AI-idee: welk probleem wil je oplossen, en voor wie?"}
          </blockquote>
          {thinking && <div className="thinking" aria-label="Facilitator schrijft mee"><span /><span /><span /></div>}
          {guide && guide.bullets.length > 0 && (
            <div className="guide" key={guide.bullets.join("|")}>
              <div className="voice-kicker">De facilitator {guide.kind === "stelt_voor" ? "stelt voor" : "inspireert"}</div>
              <ul>{guide.bullets.map((b, i) => <li key={i}>{b}</li>)}</ul>
            </div>
          )}
          {ready && (
            <button className="ready" onClick={onNext} aria-live="polite">
              ✓ {ready === "vraag" ? "Beantwoord" : ready === "einde" ? "Alle stappen zijn rond" : `${steps[steps.findIndex((s) => s.key === ready) - 1]?.key ?? "Dit hoofdstuk"} is rond`}
              {" — "}druk <kbd>N</kbd>{ready === "vraag" ? " voor de volgende vraag" : ready === "einde" ? " voor de afronding" : ` om door te gaan naar ${ready}`}
            </button>
          )}
          <div className="hints">
            <button className="next-hint" onClick={onNext}><kbd>N</kbd> volgende vraag</button>
            <button className="next-hint" onClick={onDeepen}><kbd>D</kbd> doorvragen</button>
            <button className="next-hint" onClick={onInspire}><kbd>I</kbd> nieuw voorstel</button>
          </div>
          {said && (
            <div className={`subtitle ${said.final ? "" : "provisional"}`}>
              <span className="who">Jij</span> {said.text}
            </div>
          )}
          {!following && <button className="btn small ghost follow" onClick={onFollow}>↻ Volg live (L)</button>}
        </aside>
      </div>
    </div>
  );
});
