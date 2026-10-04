import { useEffect, useState } from "react";
import { api, errText, type CostSummary, type Settings } from "../api";
import { usd } from "../format";

export function CostDetail({ c }: { c: CostSummary }) {
  const u = c.usage;
  return (
    <>
      <table>
        <thead>
          <tr><th>Categorie</th><th className="num">Niet-gecached</th><th className="num">Gecached</th><th className="num">Output</th></tr>
        </thead>
        <tbody>
          <tr><td>Tekst (tokens)</td><td className="num">{u.textIn - u.textCached}</td><td className="num">{u.textCached}</td><td className="num">{u.textOut}</td></tr>
          <tr><td>Audio (tokens)</td><td className="num">{u.audioIn - u.audioCached}</td><td className="num">{u.audioCached}</td><td className="num">{u.audioOut}</td></tr>
          <tr><td>Transcriptie</td><td className="num" colSpan={3}>{u.billedSeconds ? Number(u.billedSeconds).toFixed(1) : 0} s</td></tr>
          {u.reasoning > 0 && <tr><td>Reasoning (inbegrepen in output)</td><td className="num" colSpan={3}>{u.reasoning}</td></tr>}
        </tbody>
      </table>
      <p className="hint">
        Gemeten: {usd(c.measuredUsd, 4)} · geschat/ontbrekend: {usd(c.estimatedUsd, 4)} · responses: {c.responses}
        {c.incomplete && " · onvolledig: bevat schattingen, geannuleerde of ontbrekende usage"}
      </p>
      {c.unresolved.length > 0 && <p className="hint">Onopgelost: {Array.from(new Set(c.unresolved)).join("; ")}</p>}
      <p className="hint">Berekende factuurindicatie op basis van de prijssnapshot; geen OpenAI-factuur.</p>
    </>
  );
}

export function Costs({ onError, onInfo }: { onError: (m: string) => void; onInfo: (m: string) => void }) {
  const [d, setD] = useState<{ total: CostSummary; month: CostSummary; setup: CostSummary; deletedSessionRows: number; settings: Settings; priceVersion: string } | null>(null);
  useEffect(() => {
    api.costOverview().then(setD).catch((e) => onError(errText(e)));
  }, []);
  if (!d) return <p className="hint" style={{ padding: 24 }}>Laden…</p>;
  const pctMonth = (Number(d.month.totalUsd) / Number(d.settings.monthlyBudgetUsd)) * 100;
  const exp = (k: "costs" | "metrics") => api.exportCosts(k).then((p) => p && onInfo(`Opgeslagen als ${p}`)).catch((e) => onError(errText(e)));
  return (
    <div className="page">
      <div className="grid2">
        <div className="panel">
          <h2>Deze maand</h2>
          <p style={{ fontSize: 28, fontWeight: 800 }}>~{usd(d.month.totalUsd)}</p>
          <p className="hint">{pctMonth.toFixed(0)}% van maandbudget {usd(d.settings.monthlyBudgetUsd)}{pctMonth >= 80 ? " — let op" : ""}</p>
        </div>
        <div className="panel">
          <h2>Totaal deze installatie</h2>
          <p style={{ fontSize: 28, fontWeight: 800 }}>~{usd(d.total.totalUsd)}</p>
          <p className="hint">Waarvan setup-tests: {usd(d.setup.totalUsd, 4)}. {d.deletedSessionRows > 0 && `${d.deletedSessionRows} regels van verwijderde sessies tellen anoniem mee.`}</p>
        </div>
      </div>
      <div className="panel">
        <h2>Totaal verbruik</h2>
        <CostDetail c={d.total} />
        <p className="hint">
          Dit is geen volledig OpenAI-accountoverzicht: gebruik op andere apparaten of buiten deze app valt erbuiten. Huidige prijsversie: {d.priceVersion}.
        </p>
        <div className="row">
          <button className="btn" onClick={() => exp("costs")}>Exporteer kosten (CSV)</button>
          <button className="btn" onClick={() => exp("metrics")}>Exporteer metrics (CSV)</button>
        </div>
      </div>
    </div>
  );
}
