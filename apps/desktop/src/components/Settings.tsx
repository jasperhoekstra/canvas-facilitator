import { useEffect, useState } from "react";
import { api, errText, on, type Device, type Settings as S } from "../api";

export function KeyForm({ mask, onChange, profile, onTested }: { mask: string | null; onChange: () => void; profile: string; onTested?: (ok: boolean) => void }) {
  const [key, setKey] = useState("");
  const [editing, setEditing] = useState(!mask);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [tests, setTests] = useState<{ model: string; ok: boolean; error: string | null }[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmDel, setConfirmDel] = useState(false);
  useEffect(() => setEditing(!mask), [mask]);
  return (
    <div>
      <p className="hint">
        Gebruik een eigen OpenAI-projectkey (platform.openai.com → API keys). Verbruik wordt via jouw OpenAI-project afgerekend. De key wordt opgeslagen in de
        beveiligde sleutelbos van je besturingssysteem en is hierna niet meer in te zien.
      </p>
      {mask && !editing && (
        <div className="row">
          <span className="pill turq" aria-label="Opgeslagen key">🔑 {mask}</span>
          <button className="btn small" onClick={() => setEditing(true)}>Vervangen</button>
          <button className="btn small danger" onClick={() => setConfirmDel(true)}>Verwijderen</button>
        </div>
      )}
      {editing && (
        <form
          className="row"
          onSubmit={async (e) => {
            e.preventDefault();
            setBusy(true);
            try {
              const m = await api.saveKey(key);
              setKey(""); // clear form state immediately
              setMsg({ ok: true, text: `Opgeslagen (${m})` });
              onChange();
            } catch (x) {
              setKey("");
              setMsg({ ok: false, text: errText(x) });
            } finally {
              setBusy(false);
            }
          }}
        >
          <label htmlFor="apikey" className="sr-only">OpenAI API-key</label>
          <input id="apikey" type="password" autoComplete="off" spellCheck={false} placeholder="sk-…" value={key} onChange={(e) => setKey(e.target.value)} style={{ flex: 1 }} />
          <button className="btn primary" disabled={busy || !key}>Opslaan</button>
          {mask && <button type="button" className="btn ghost" onClick={() => { setKey(""); setEditing(false); }}>Annuleer</button>}
        </form>
      )}
      {mask && (
        <div className="row" style={{ marginTop: 8 }}>
          <button
            className="btn"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              setTests(null);
              try {
                const r = await api.testConnection(profile);
                setTests(r);
                onTested?.(r.every((t) => t.ok));
              } catch (x) {
                setMsg({ ok: false, text: errText(x) });
              } finally {
                setBusy(false);
              }
            }}
          >
            {busy ? "Testen…" : "Verbinding testen"}
          </button>
          <span className="hint">Controleert authenticatie en modeltoegang zonder betaalde generatie.</span>
        </div>
      )}
      {tests && (
        <ul aria-live="polite">
          {tests.map((t) => (
            <li key={t.model} className={t.ok ? "okText" : "error"}>
              {t.ok ? "✓" : "✕"} {t.model}{t.error ? `: ${t.error}` : ": toegang OK"}
            </li>
          ))}
        </ul>
      )}
      {msg && <p className={msg.ok ? "okText" : "error"} role="status">{msg.text}</p>}
      {confirmDel && (
        <div className="dialog" role="alertdialog" aria-modal="true" aria-labelledby="dk">
          <div>
            <h2 id="dk">Key verwijderen?</h2>
            <p>Een actieve sessie wordt eerst gesloten. Daarna zijn geen betaalde verzoeken meer mogelijk tot je een nieuwe key instelt.</p>
            <div className="row">
              <button className="btn danger" onClick={async () => { await api.deleteKey().catch(() => {}); setConfirmDel(false); setTests(null); onChange(); }}>Verwijder key</button>
              <button className="btn ghost" autoFocus onClick={() => setConfirmDel(false)}>Annuleer</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export function AudioSetup({ settings, onSave }: { settings: S; onSave: (s: S) => void }) {
  const [dev, setDev] = useState<{ inputs: Device[]; outputs: Device[] }>({ inputs: [], outputs: [] });
  const [testing, setTesting] = useState(false);
  const [level, setLevel] = useState(0);
  const [err, setErr] = useState("");
  useEffect(() => {
    api.audioDevices().then(setDev).catch((e) => setErr(errText(e)));
    const un = on<number>("level", setLevel);
    return () => {
      un.then((f) => f());
      api.micTestStop();
    };
  }, []);
  const toggle = async () => {
    setErr("");
    if (testing) {
      await api.micTestStop();
      setTesting(false);
      setLevel(0);
    } else {
      try {
        await api.micTestStart(settings.inputDevice, settings.outputDevice);
        setTesting(true);
      } catch (e) {
        setErr(`${errText(e)}. Controleer of de app microfoontoegang heeft in de systeeminstellingen.`);
      }
    }
  };
  return (
    <div>
      <div className="grid2">
        <div>
          <label htmlFor="in-dev">Microfoon</label>
          <select id="in-dev" value={settings.inputDevice ?? ""} onChange={(e) => onSave({ ...settings, inputDevice: e.target.value || null })}>
            <option value="">Standaardapparaat</option>
            {dev.inputs.map((d) => <option key={d.id} value={d.id}>{d.name}{d.default ? " (standaard)" : ""}</option>)}
          </select>
        </div>
        <div>
          <label htmlFor="out-dev">Uitvoerapparaat</label>
          <select id="out-dev" value={settings.outputDevice ?? ""} onChange={(e) => onSave({ ...settings, outputDevice: e.target.value || null })}>
            <option value="">Standaardapparaat</option>
            {dev.outputs.map((d) => <option key={d.id} value={d.id}>{d.name}{d.default ? " (standaard)" : ""}</option>)}
          </select>
        </div>
      </div>
      <div className="row" style={{ marginTop: 12 }}>
        <button className="btn" onClick={toggle}>{testing ? "Stop audiotest" : "Start audiotest"}</button>
        <div className="meter" role="meter" aria-label="Microfoonniveau" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(level * 100)} style={{ width: 200 }}>
          <div style={{ width: `${Math.round(level * 100)}%` }} />
        </div>
        <button className="btn" disabled={!testing} onClick={() => api.speakerTest().catch((e) => setErr(errText(e)))}>Speel testtoon</button>
      </div>
      {err && <p className="error" role="alert">{err}</p>}
      <p className="hint">Tip: gebruik een headset. Met ingebouwde luidsprekers onderdrukt de app echo, maar kan harde achtergrondgeluiden als onderbreking zien.</p>
    </div>
  );
}

export function BudgetForm({ settings, onSave }: { settings: S; onSave: (s: S) => void }) {
  return (
    <div>
      <div className="grid2">
        <div>
          <label htmlFor="b-s">Budget per sessie (USD)</label>
          <input id="b-s" type="number" min="0.1" step="0.1" value={settings.sessionBudgetUsd} onChange={(e) => onSave({ ...settings, sessionBudgetUsd: e.target.value })} />
        </div>
        <div>
          <label htmlFor="b-m">Budget per maand (USD)</label>
          <input id="b-m" type="number" min="1" step="1" value={settings.monthlyBudgetUsd} onChange={(e) => onSave({ ...settings, monthlyBudgetUsd: e.target.value })} />
        </div>
      </div>
      <p className="hint">
        Waarschuwingen op 80% en 95%. Dit is een lokale best-effort kostenstop met 10% marge, geen gegarandeerd factuurplafond: verbruik kan vertraagd binnenkomen,
        een lopend antwoord kan iets overschrijden en gebruik buiten deze app wordt niet bewaakt.
      </p>
    </div>
  );
}

export function SettingsPage({ keyMask, refresh, onError, onInfo, version }: {
  keyMask: string | null; refresh: () => void; onError: (m: string) => void; onInfo: (m: string) => void; version: string;
}) {
  const [s, setS] = useState<S | null>(null);
  const [pricing, setPricing] = useState<Awaited<ReturnType<typeof api.getPricing>> | null>(null);
  const [override, setOverride] = useState("");
  useEffect(() => {
    api.getSettings().then(setS).catch((e) => onError(errText(e)));
    api.getPricing().then((p) => { setPricing(p); setOverride(JSON.stringify(p.pricing, null, 2)); }).catch(() => {});
  }, []);
  if (!s) return <p className="hint" style={{ padding: 24 }}>Laden…</p>;
  const save = (n: S) => {
    setS(n);
    if (!(Number(n.sessionBudgetUsd) > 0 && Number(n.monthlyBudgetUsd) > 0)) return; // wait for a valid value
    api.saveSettings(n).catch((e) => onError(errText(e)));
  };
  return (
    <div className="page">
      <div className="panel">
        <h2>OpenAI API-key</h2>
        <KeyForm mask={keyMask} onChange={refresh} profile={s.profile} />
      </div>
      <div className="panel">
        <h2>Standaard gesprek</h2>
        <div className="grid2">
          <div>
            <label htmlFor="d-p">Modelprofiel</label>
            <select id="d-p" value={s.profile} onChange={(e) => save({ ...s, profile: e.target.value as S["profile"] })}>
              <option value="quality">Kwaliteit (gpt-realtime-2.1)</option>
              <option value="mini">Kosten (gpt-realtime-2.1-mini)</option>
            </select>
          </div>
          <div>
            <label htmlFor="d-s">Gespreksstijl</label>
            <select id="d-s" value={s.style} onChange={(e) => save({ ...s, style: e.target.value as S["style"] })}>
              <option value="neutraal">Neutraal</option>
              <option value="coachend">Coachend</option>
              <option value="kritisch">Kritisch</option>
            </select>
          </div>
        </div>
      </div>
      <div className="panel">
        <h2>Audio</h2>
        <AudioSetup settings={s} onSave={save} />
      </div>
      <div className="panel">
        <h2>Budget</h2>
        <BudgetForm settings={s} onSave={save} />
      </div>
      <div className="panel">
        <h2>Opslag en privacy</h2>
        <p>Sessies worden versleuteld op dit apparaat opgeslagen. Audio en gesprekscontext gaan voor verwerking naar OpenAI; er worden geen audio-opnames bewaard.</p>
        <label htmlFor="ret">Bewaartermijn</label>
        <select id="ret" value={s.retentionDays ?? ""} onChange={(e) => save({ ...s, retentionDays: e.target.value ? Number(e.target.value) : null })} style={{ maxWidth: 320 }}>
          <option value="">Bewaren tot ik verwijder (standaard)</option>
          <option value="30">30 dagen</option>
          <option value="90">90 dagen</option>
        </select>
        <p className="hint">Verwijderen omvat sessiegegevens en lokale herstelkopieën, maar geen door jou gemaakte exports en geen forensische overschrijving van SSD-sectoren.</p>
      </div>
      {pricing && (
        <div className="panel">
          <h2>Prijzen</h2>
          <p>
            Versie {pricing.pricing.version} · gecontroleerd {pricing.pricing.verifiedAt} ({pricing.ageDays ?? "?"} dagen oud) · bron{" "}
            <span className="hint">{pricing.pricing.source}</span>
          </p>
          {pricing.stale && <p className="error" role="alert">De prijzen zijn ouder dan 30 dagen. Controleer de actuele OpenAI-tarieven en werk ze hieronder bij.</p>}
          <label htmlFor="pr">Prijsconfiguratie (JSON, USD per 1M tokens; transcriptie per minuut)</label>
          <textarea id="pr" value={override} onChange={(e) => setOverride(e.target.value)} style={{ minHeight: 220, fontFamily: "monospace", fontSize: 12 }} />
          <div className="row" style={{ marginTop: 6 }}>
            <button className="btn" onClick={() => api.saveSettings({ ...s, pricingOverride: override }).then(() => { onInfo("Prijzen bijgewerkt; geldt voor nieuwe sessies."); setS({ ...s, pricingOverride: override }); api.getPricing().then(setPricing); }).catch((e) => onError(errText(e)))}>
              Opslaan
            </button>
            {pricing.overridden && (
              <button className="btn ghost" onClick={() => { const n = { ...s, pricingOverride: null }; save(n); setOverride(JSON.stringify(pricing.bundled, null, 2)); api.getPricing().then(setPricing); }}>
                Terug naar meegeleverde prijzen
              </button>
            )}
          </div>
          <p className="hint">Historische sessies houden hun eigen prijssnapshot. Ontbreekt een tarief, dan kan geen sessie met dat model starten.</p>
        </div>
      )}
      <div className="panel">
        <h2>Diagnostiek</h2>
        <p className="hint">Lokaal logbestand zonder keys of audio. Delen gebeurt alleen via export.</p>
        <button className="btn" onClick={() => api.exportDiagnostics().then((p) => p && onInfo(`Opgeslagen als ${p}`)).catch((e) => onError(errText(e)))}>Exporteer diagnostiek</button>
        <p className="hint">Versie {version}. Updates: controleer handmatig de releasepagina; de app werkt nooit bij tijdens een sessie.</p>
      </div>
    </div>
  );
}
