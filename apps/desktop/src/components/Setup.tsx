import { useEffect, useState } from "react";
import { api, errText, type AppStatus, type Settings } from "../api";
import { AudioSetup, BudgetForm, KeyForm } from "./Settings";

const TITLES = ["Welkom", "OpenAI-key", "Audio", "Budget", "Klaar"];

export function Setup({ status, refresh, onDone }: { status: AppStatus; refresh: () => void; onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [s, setS] = useState<Settings | null>(status.settings);
  const [tested, setTested] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => {
    if (!s) api.getSettings().then(setS).catch((e) => setErr(errText(e)));
  }, []);
  if (!s) return <p className="hint" style={{ padding: 24 }}>{err || "Laden…"}</p>;
  const save = (n: Settings) => {
    setS(n);
    if (Number(n.sessionBudgetUsd) > 0 && Number(n.monthlyBudgetUsd) > 0) api.saveSettings(n).catch((e) => setErr(errText(e)));
  };
  const canNext = [true, status.keyPresent && tested, true, Number(s.sessionBudgetUsd) > 0 && Number(s.monthlyBudgetUsd) > 0, true][step];

  return (
    <main className="wizard">
      <div className="steps-indicator" aria-hidden>
        {TITLES.map((t, i) => <span key={t} className={i <= step ? "on" : ""} />)}
      </div>
      <div className="panel">
        <p className="hint">Stap {step + 1} van {TITLES.length}</p>
        <h1>{TITLES[step]}</h1>
        {step === 0 && (
          <>
            <p>
              Canvas Facilitator helpt je in één Nederlands gesprek van maximaal 15 minuten een AI-idee uit te werken tot een besluitbaar canvas:
              KIES → MEET → BEGRENS → REALISEER → VERANKER.
            </p>
            <p>Je hebt nodig: een eigen OpenAI API-key, een microfoon (headset aanbevolen) en een budget dat je zelf kiest.</p>
            <p className="hint">Je sessies worden versleuteld op dit apparaat bewaard. Audio en gesprekscontext gaan voor verwerking naar OpenAI.</p>
          </>
        )}
        {step === 1 && (
          <>
            <KeyForm mask={status.keyMask} onChange={() => { setTested(false); refresh(); }} profile={s.profile} onTested={setTested} />
            {status.keyPresent && !tested && <p className="hint">Test de verbinding om verder te gaan.</p>}
            <label htmlFor="w-p">Modelprofiel</label>
            <select id="w-p" value={s.profile} onChange={(e) => { setTested(false); save({ ...s, profile: e.target.value as Settings["profile"] }); }}>
              <option value="quality">Kwaliteit (gpt-realtime-2.1)</option>
              <option value="mini">Kosten (gpt-realtime-2.1-mini)</option>
            </select>
          </>
        )}
        {step === 2 && <AudioSetup settings={s} onSave={save} />}
        {step === 3 && <BudgetForm settings={s} onSave={save} />}
        {step === 4 && <p>Alles staat klaar. Start je eerste sessie en vertel kort over je AI-idee.</p>}
        {err && <p className="error" role="alert">{err}</p>}
        <div className="row" style={{ justifyContent: "space-between", marginTop: 16 }}>
          <button className="btn ghost" disabled={step === 0} onClick={() => setStep(step - 1)}>Vorige</button>
          {step < TITLES.length - 1 ? (
            <button className="btn primary" disabled={!canNext} onClick={() => setStep(step + 1)}>Volgende</button>
          ) : (
            <button className="btn primary" onClick={() => api.saveSettings({ ...s, onboarded: true }).then(onDone).catch((e) => setErr(errText(e)))}>
              Klaar
            </button>
          )}
        </div>
      </div>
    </main>
  );
}
