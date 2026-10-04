import { useEffect, useState } from "react";
import { api, errText, type SessionListItem } from "../api";
import { date, duration, SESSION_STATUS_LABEL, usd } from "../format";

export function History({ onOpen, onError }: { onOpen: (id: string) => void; onError: (m: string) => void }) {
  const [q, setQ] = useState("");
  const [status, setStatus] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [rows, setRows] = useState<SessionListItem[] | null>(null);

  useEffect(() => {
    const f = from ? new Date(from).getTime() : null;
    const t = to ? new Date(to).getTime() + 86_400_000 : null;
    api.listSessions(q, status, f, t).then(setRows).catch((e) => onError(errText(e)));
  }, [q, status, from, to]);

  return (
    <div className="page">
      <div className="panel">
        <h1>Sessies</h1>
        <div className="row">
          <input type="search" aria-label="Zoek op titel" placeholder="Zoek op titel" value={q} onChange={(e) => setQ(e.target.value)} style={{ flex: 2 }} />
          <select aria-label="Filter op status" value={status} onChange={(e) => setStatus(e.target.value)} style={{ flex: 1 }}>
            <option value="">Alle statussen</option>
            {Object.entries(SESSION_STATUS_LABEL).map(([k, v]) => (
              <option key={k} value={k}>{v}</option>
            ))}
          </select>
          <input type="date" aria-label="Vanaf datum" value={from} onChange={(e) => setFrom(e.target.value)} style={{ flex: 1 }} />
          <input type="date" aria-label="Tot en met datum" value={to} onChange={(e) => setTo(e.target.value)} style={{ flex: 1 }} />
        </div>
      </div>
      <div className="panel">
        {rows === null ? (
          <p className="hint">Laden…</p>
        ) : rows.length === 0 ? (
          <p className="hint">Geen sessies gevonden.</p>
        ) : (
          <table>
            <thead>
              <tr><th>Titel</th><th>Datum</th><th>Duur</th><th>Model</th><th>Status</th><th className="num">Geschatte kosten</th></tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.id}>
                  <td><button className="btn ghost small" style={{ textAlign: "left" }} onClick={() => onOpen(r.id)}>{r.title}</button></td>
                  <td>{date(r.startedAt ?? r.createdAt)}</td>
                  <td>{duration(r)}</td>
                  <td>{r.model}</td>
                  <td>{SESSION_STATUS_LABEL[r.status] ?? r.status}</td>
                  <td className="num">~{usd(r.cost.totalUsd, 3)}{r.cost.incomplete ? " *" : ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <p className="hint">* bevat geschat of ontbrekend verbruik.</p>
      </div>
    </div>
  );
}
