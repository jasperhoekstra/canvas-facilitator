//! Encrypted SQLite (SQLCipher) store: the single source of truth (PRD §9).

use crate::canvas::{Canvas, CanvasItem, Decision, Mutation, Note, StepCompletion};
use crate::ledger::{self, Pricing, Usage};
use rusqlite::{params, Connection, OptionalExtension};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub const SCHEMA_VERSION: i64 = 1;

const MIGRATIONS: &[&str] = &[
    // v1
    r#"
    CREATE TABLE settings(k TEXT PRIMARY KEY, v TEXT NOT NULL);
    CREATE TABLE sessions(
        id TEXT PRIMARY KEY, title TEXT NOT NULL, schema_version INTEGER NOT NULL,
        created_at INTEGER NOT NULL, started_at INTEGER, ended_at INTEGER, deadline_at INTEGER,
        last_heartbeat INTEGER, status TEXT NOT NULL, stop_reason TEXT, style TEXT NOT NULL,
        profile TEXT NOT NULL, model TEXT NOT NULL, transcribe_model TEXT NOT NULL,
        parent_id TEXT, price_version TEXT NOT NULL, pricing_json TEXT NOT NULL,
        budget_usd TEXT NOT NULL, metrics_json TEXT NOT NULL DEFAULT '{}');
    CREATE TABLE canvas_items(
        id TEXT PRIMARY KEY, session_id TEXT NOT NULL, step TEXT NOT NULL, field TEXT NOT NULL,
        domain TEXT NOT NULL, value TEXT NOT NULL, status TEXT NOT NULL, evidence TEXT NOT NULL,
        source_turn_ids TEXT NOT NULL, revision INTEGER NOT NULL, manual INTEGER NOT NULL,
        updated_at INTEGER NOT NULL, UNIQUE(session_id, step, field));
    CREATE TABLE item_history(
        id INTEGER PRIMARY KEY, session_id TEXT NOT NULL, step TEXT NOT NULL, field TEXT NOT NULL,
        value TEXT NOT NULL, status TEXT NOT NULL, revision INTEGER NOT NULL, manual INTEGER NOT NULL,
        at INTEGER NOT NULL);
    CREATE TABLE notes(
        id TEXT PRIMARY KEY, session_id TEXT NOT NULL, step TEXT NOT NULL, kind TEXT NOT NULL,
        text TEXT NOT NULL, source_turn_ids TEXT NOT NULL, created_at INTEGER NOT NULL);
    CREATE TABLE decisions(
        id TEXT PRIMARY KEY, session_id TEXT NOT NULL, kind TEXT NOT NULL, content TEXT NOT NULL,
        rationale TEXT NOT NULL, owner TEXT NOT NULL, due TEXT NOT NULL,
        confirmation_turn_id TEXT, created_at INTEGER NOT NULL);
    CREATE TABLE step_completions(
        session_id TEXT NOT NULL, step TEXT NOT NULL, synthesis TEXT NOT NULL,
        confirmation_turn_id TEXT, created_at INTEGER NOT NULL, PRIMARY KEY(session_id, step));
    CREATE TABLE turns(
        id TEXT PRIMARY KEY, session_id TEXT NOT NULL, seq INTEGER NOT NULL, provider_item_id TEXT,
        speaker TEXT NOT NULL, started_at INTEGER NOT NULL, ended_at INTEGER, text TEXT NOT NULL,
        final INTEGER NOT NULL, interrupted INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0,
        spoken_text TEXT, original_text TEXT, corrected_at INTEGER);
    CREATE INDEX turns_session ON turns(session_id, seq);
    CREATE TABLE usage_events(
        id INTEGER PRIMARY KEY, dedupe_key TEXT NOT NULL UNIQUE, session_id TEXT, scope TEXT NOT NULL,
        model TEXT NOT NULL, response_id TEXT, kind TEXT NOT NULL, usage_json TEXT NOT NULL,
        cost_usd TEXT, cost_error TEXT, price_version TEXT NOT NULL,
        superseded INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
    CREATE INDEX usage_session ON usage_events(session_id);
    CREATE INDEX usage_created ON usage_events(created_at);
    CREATE TABLE tool_calls(
        call_id TEXT PRIMARY KEY, session_id TEXT NOT NULL, name TEXT NOT NULL, ok INTEGER NOT NULL,
        result TEXT NOT NULL, created_at INTEGER NOT NULL);
    "#,
];

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

type R<T> = Result<T, String>;
fn e<E: std::fmt::Display>(x: E) -> String {
    format!("Opslagfout: {x}")
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// "quality" | "mini"
    pub profile: String,
    /// "neutraal" | "coachend" | "kritisch"
    pub style: String,
    pub voice: String,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub session_budget_usd: Decimal,
    pub monthly_budget_usd: Decimal,
    /// None = keep until deleted; Some(30|90)
    pub retention_days: Option<i64>,
    pub pricing_override: Option<String>,
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            profile: "quality".into(),
            style: "neutraal".into(),
            voice: "marin".into(),
            input_device: None,
            output_device: None,
            session_budget_usd: Decimal::new(200, 2),
            monthly_budget_usd: Decimal::new(2500, 2),
            retention_days: None,
            pricing_override: None,
            onboarded: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: String,
    pub title: String,
    pub schema_version: i64,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub deadline_at: Option<i64>,
    pub last_heartbeat: Option<i64>,
    pub status: String,
    pub stop_reason: Option<String>,
    pub style: String,
    pub profile: String,
    pub model: String,
    pub transcribe_model: String,
    pub parent_id: Option<String>,
    pub price_version: String,
    pub pricing_json: String,
    pub budget_usd: Decimal,
    pub metrics_json: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: String,
    pub session_id: String,
    pub seq: i64,
    pub provider_item_id: Option<String>,
    /// "user" | "assistant"
    pub speaker: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub text: String,
    #[serde(rename = "final")]
    pub is_final: bool,
    pub interrupted: bool,
    pub failed: bool,
    /// For interrupted assistant turns: the part actually played.
    pub spoken_text: Option<String>,
    pub original_text: Option<String>,
    pub corrected_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    pub id: i64,
    pub session_id: Option<String>,
    pub scope: String,
    pub model: String,
    pub response_id: Option<String>,
    /// "measured" | "estimated" | "missing"
    pub kind: String,
    pub usage: Usage,
    pub cost_usd: Option<Decimal>,
    pub cost_error: Option<String>,
    pub price_version: String,
    pub superseded: bool,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CostSummary {
    pub total_usd: Decimal,
    /// Rows without a computable cost or without measured usage.
    pub incomplete: bool,
    pub measured_usd: Decimal,
    pub estimated_usd: Decimal,
    pub usage: Usage,
    pub responses: i64,
    pub unresolved: Vec<String>,
}

pub struct Db {
    conn: Connection,
    path: Option<PathBuf>,
}

impl Db {
    /// Open (or create) the encrypted database. `key_hex` is 64 hex chars from the OS store.
    pub fn open(path: &Path, key_hex: &str) -> R<Db> {
        assert!(key_hex.len() == 64 && key_hex.chars().all(|c| c.is_ascii_hexdigit()));
        let conn = Connection::open(path).map_err(e)?;
        conn.execute_batch(&format!("PRAGMA key = \"x'{key_hex}'\";")).map_err(e)?;
        // Fails here when the key is wrong.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .map_err(|_| "Database kan niet worden ontsleuteld (sleutel ontbreekt of klopt niet)".to_string())?;
        let mut db = Db { conn, path: Some(path.to_path_buf()) };
        db.init()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn memory() -> Db {
        let mut db = Db { conn: Connection::open_in_memory().unwrap(), path: None };
        db.init().unwrap();
        db
    }

    fn init(&mut self) -> R<()> {
        self.conn
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA secure_delete=ON; PRAGMA foreign_keys=ON;")
            .map_err(e)?;
        let v: i64 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(e)?;
        if v >= SCHEMA_VERSION {
            return Ok(());
        }
        // Encrypted recovery copy before upgrading an existing database (PRD AC-DATA).
        let backup = match (&self.path, v > 0) {
            (Some(p), true) => {
                self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").map_err(e)?;
                let b = p.with_extension(format!("bak-v{v}"));
                std::fs::copy(p, &b).map_err(e)?;
                Some(b)
            }
            _ => None,
        };
        let tx = self.conn.transaction().map_err(e)?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(v as usize) {
            tx.execute_batch(sql).map_err(|x| format!("Migratie naar v{} mislukt, teruggedraaid: {x}", i + 1))?;
        }
        tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};")).map_err(e)?;
        tx.commit().map_err(e)?;
        // The copy holds session data; it must not outlive a successful migration.
        if let Some(b) = backup {
            let _ = std::fs::remove_file(b);
        }
        Ok(())
    }

    // ---------- settings ----------

    pub fn settings(&self) -> Settings {
        self.conn
            .query_row("SELECT v FROM settings WHERE k='settings'", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> R<()> {
        let v = serde_json::to_string(s).map_err(e)?;
        self.conn.execute("INSERT INTO settings(k,v) VALUES('settings',?1) ON CONFLICT(k) DO UPDATE SET v=?1", [v]).map_err(e)?;
        Ok(())
    }

    /// Whole days since an ISO date, for the pricing staleness warning.
    pub fn days_since(&self, date: &str) -> Option<i64> {
        self.conn.query_row("SELECT CAST(julianday('now') - julianday(?1) AS INTEGER)", [date], |r| r.get(0)).ok().flatten()
    }

    pub fn pricing(&self) -> Pricing {
        self.settings().pricing_override.and_then(|j| Pricing::parse(&j).ok()).unwrap_or_else(Pricing::bundled)
    }

    // ---------- sessions ----------

    pub fn insert_session(&self, s: &SessionRow) -> R<()> {
        self.conn
            .execute(
                "INSERT INTO sessions(id,title,schema_version,created_at,status,style,profile,model,transcribe_model,
                 parent_id,price_version,pricing_json,budget_usd,metrics_json)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                params![
                    s.id, s.title, s.schema_version, s.created_at, s.status, s.style, s.profile, s.model,
                    s.transcribe_model, s.parent_id, s.price_version, s.pricing_json, s.budget_usd.to_string(), s.metrics_json
                ],
            )
            .map_err(e)?;
        Ok(())
    }

    fn session_from_row(r: &rusqlite::Row) -> rusqlite::Result<SessionRow> {
        Ok(SessionRow {
            id: r.get("id")?,
            title: r.get("title")?,
            schema_version: r.get("schema_version")?,
            created_at: r.get("created_at")?,
            started_at: r.get("started_at")?,
            ended_at: r.get("ended_at")?,
            deadline_at: r.get("deadline_at")?,
            last_heartbeat: r.get("last_heartbeat")?,
            status: r.get("status")?,
            stop_reason: r.get("stop_reason")?,
            style: r.get("style")?,
            profile: r.get("profile")?,
            model: r.get("model")?,
            transcribe_model: r.get("transcribe_model")?,
            parent_id: r.get("parent_id")?,
            price_version: r.get("price_version")?,
            pricing_json: r.get("pricing_json")?,
            budget_usd: Decimal::from_str(&r.get::<_, String>("budget_usd")?).unwrap_or_default(),
            metrics_json: r.get("metrics_json")?,
        })
    }

    pub fn session(&self, id: &str) -> R<SessionRow> {
        self.conn.query_row("SELECT * FROM sessions WHERE id=?1", [id], Self::session_from_row).map_err(|_| "Sessie niet gevonden".to_string())
    }

    pub fn sessions(&self, query: &str, status: &str, from: Option<i64>, to: Option<i64>) -> R<Vec<SessionRow>> {
        let mut st = self
            .conn
            .prepare(
                "SELECT * FROM sessions WHERE title LIKE ?1 AND (?2='' OR status=?2)
                 AND (?3 IS NULL OR created_at>=?3) AND (?4 IS NULL OR created_at<?4) ORDER BY created_at DESC",
            )
            .map_err(e)?;
        let rows = st.query_map(params![format!("%{query}%"), status, from, to], Self::session_from_row).map_err(e)?;
        rows.collect::<Result<_, _>>().map_err(e)
    }

    pub fn sessions_in_states(&self, states: &[&str]) -> R<Vec<SessionRow>> {
        Ok(self.sessions("", "", None, None)?.into_iter().filter(|s| states.contains(&s.status.as_str())).collect())
    }

    pub fn set_status(&self, id: &str, status: &str, reason: Option<&str>) -> R<()> {
        let ended = matches!(status, "COMPLETED" | "INTERRUPTED" | "FAILED").then(now_ms);
        self.conn
            .execute(
                "UPDATE sessions SET status=?2, stop_reason=COALESCE(?3, stop_reason), ended_at=COALESCE(?4, ended_at) WHERE id=?1",
                params![id, status, reason, ended],
            )
            .map_err(e)?;
        Ok(())
    }

    pub fn set_started(&self, id: &str, started_at: i64, deadline_at: i64) -> R<()> {
        self.conn
            .execute(
                "UPDATE sessions SET started_at=?2, deadline_at=?3, last_heartbeat=?2 WHERE id=?1 AND started_at IS NULL",
                params![id, started_at, deadline_at],
            )
            .map_err(e)?;
        Ok(())
    }

    pub fn heartbeat(&self, id: &str, at: i64) -> R<()> {
        self.conn.execute("UPDATE sessions SET last_heartbeat=?2 WHERE id=?1", params![id, at]).map_err(e)?;
        Ok(())
    }

    pub fn set_metrics(&self, id: &str, json: &str) -> R<()> {
        self.conn.execute("UPDATE sessions SET metrics_json=?2 WHERE id=?1", params![id, json]).map_err(e)?;
        Ok(())
    }

    pub fn rename_session(&self, id: &str, title: &str) -> R<()> {
        self.conn.execute("UPDATE sessions SET title=?2 WHERE id=?1", params![id, title]).map_err(e)?;
        Ok(())
    }

    /// Delete session content; optionally keep the anonymous cost ledger.
    pub fn delete_session(&mut self, id: &str, keep_costs: bool) -> R<()> {
        let tx = self.conn.transaction().map_err(e)?;
        for t in ["canvas_items", "item_history", "notes", "decisions", "step_completions", "turns", "tool_calls"] {
            tx.execute(&format!("DELETE FROM {t} WHERE session_id=?1"), [id]).map_err(e)?;
        }
        if keep_costs {
            tx.execute("UPDATE usage_events SET response_id=NULL WHERE session_id=?1", [id]).map_err(e)?;
        } else {
            tx.execute("DELETE FROM usage_events WHERE session_id=?1", [id]).map_err(e)?;
        }
        tx.execute("DELETE FROM sessions WHERE id=?1", [id]).map_err(e)?;
        tx.commit().map_err(e)?;
        // Purge freed pages and the WAL so deleted content does not linger in local files.
        self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;").map_err(e)?;
        Ok(())
    }

    /// Ids of finished sessions older than the retention window.
    pub fn expired_sessions(&self, days: i64) -> R<Vec<String>> {
        let cutoff = now_ms() - days * 86_400_000;
        let mut st = self
            .conn
            .prepare("SELECT id FROM sessions WHERE created_at<?1 AND status IN ('COMPLETED','INTERRUPTED','FAILED','DRAFT')")
            .map_err(e)?;
        let rows = st.query_map([cutoff], |r| r.get(0)).map_err(e)?;
        rows.collect::<Result<_, _>>().map_err(e)
    }

    // ---------- canvas ----------

    pub fn canvas(&self, sid: &str) -> R<Canvas> {
        let ids = |s: String| serde_json::from_str::<Vec<String>>(&s).unwrap_or_default();
        let mut c = Canvas::default();
        let mut st = self.conn.prepare("SELECT * FROM canvas_items WHERE session_id=?1").map_err(e)?;
        c.items = st
            .query_map([sid], |r| {
                Ok(CanvasItem {
                    id: r.get("id")?,
                    step: r.get("step")?,
                    field: r.get("field")?,
                    domain: r.get("domain")?,
                    value: r.get("value")?,
                    status: r.get("status")?,
                    evidence: r.get("evidence")?,
                    source_turn_ids: ids(r.get("source_turn_ids")?),
                    revision: r.get("revision")?,
                    manual: r.get("manual")?,
                    updated_at: r.get("updated_at")?,
                })
            })
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?;
        let mut st = self.conn.prepare("SELECT * FROM notes WHERE session_id=?1 ORDER BY created_at").map_err(e)?;
        c.notes = st
            .query_map([sid], |r| {
                Ok(Note {
                    id: r.get("id")?,
                    step: r.get("step")?,
                    kind: r.get("kind")?,
                    text: r.get("text")?,
                    source_turn_ids: ids(r.get("source_turn_ids")?),
                    created_at: r.get("created_at")?,
                })
            })
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?;
        let mut st = self.conn.prepare("SELECT * FROM decisions WHERE session_id=?1 ORDER BY created_at").map_err(e)?;
        c.decisions = st
            .query_map([sid], |r| {
                Ok(Decision {
                    id: r.get("id")?,
                    kind: r.get("kind")?,
                    content: r.get("content")?,
                    rationale: r.get("rationale")?,
                    owner: r.get("owner")?,
                    due: r.get("due")?,
                    confirmation_turn_id: r.get("confirmation_turn_id")?,
                    created_at: r.get("created_at")?,
                })
            })
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?;
        let mut st = self.conn.prepare("SELECT * FROM step_completions WHERE session_id=?1 ORDER BY created_at").map_err(e)?;
        c.completed = st
            .query_map([sid], |r| {
                Ok(StepCompletion {
                    step: r.get("step")?,
                    synthesis: r.get("synthesis")?,
                    confirmation_turn_id: r.get("confirmation_turn_id")?,
                    created_at: r.get("created_at")?,
                })
            })
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?;
        Ok(c)
    }

    /// Persist a validated mutation and (optionally) the tool call result in one transaction.
    pub fn apply(&mut self, sid: &str, m: &Mutation, call: Option<(&str, &str, bool, &str)>) -> R<()> {
        let tx = self.conn.transaction().map_err(e)?;
        match m {
            Mutation::Item(i) => {
                tx.execute(
                    "INSERT INTO canvas_items(id,session_id,step,field,domain,value,status,evidence,source_turn_ids,revision,manual,updated_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                     ON CONFLICT(session_id,step,field) DO UPDATE SET value=?6,status=?7,evidence=?8,source_turn_ids=?9,revision=?10,manual=?11,updated_at=?12",
                    params![i.id, sid, i.step, i.field, i.domain, i.value, i.status, i.evidence,
                        serde_json::to_string(&i.source_turn_ids).unwrap(), i.revision, i.manual, i.updated_at],
                )
                .map_err(e)?;
                tx.execute(
                    "INSERT INTO item_history(session_id,step,field,value,status,revision,manual,at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![sid, i.step, i.field, i.value, i.status, i.revision, i.manual, i.updated_at],
                )
                .map_err(e)?;
            }
            Mutation::Note(n) => {
                tx.execute(
                    "INSERT INTO notes(id,session_id,step,kind,text,source_turn_ids,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![n.id, sid, n.step, n.kind, n.text, serde_json::to_string(&n.source_turn_ids).unwrap(), n.created_at],
                )
                .map_err(e)?;
            }
            Mutation::Decision(d) => {
                tx.execute(
                    "INSERT INTO decisions(id,session_id,kind,content,rationale,owner,due,confirmation_turn_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![d.id, sid, d.kind, d.content, d.rationale, d.owner, d.due, d.confirmation_turn_id, d.created_at],
                )
                .map_err(e)?;
            }
            Mutation::Complete(c) => {
                tx.execute(
                    "INSERT OR IGNORE INTO step_completions(session_id,step,synthesis,confirmation_turn_id,created_at) VALUES(?1,?2,?3,?4,?5)",
                    params![sid, c.step, c.synthesis, c.confirmation_turn_id, c.created_at],
                )
                .map_err(e)?;
            }
            Mutation::Read => {}
        }
        if let Some((call_id, name, ok, result)) = call {
            tx.execute(
                "INSERT INTO tool_calls(call_id,session_id,name,ok,result,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![call_id, sid, name, ok, result, now_ms()],
            )
            .map_err(e)?;
        }
        tx.commit().map_err(e)
    }

    pub fn delete_note(&self, sid: &str, id: &str) -> R<()> {
        self.conn.execute("DELETE FROM notes WHERE session_id=?1 AND id=?2", [sid, id]).map_err(e)?;
        self.conn.execute("DELETE FROM decisions WHERE session_id=?1 AND id=?2", [sid, id]).map_err(e)?;
        Ok(())
    }

    pub fn record_tool_rejection(&self, sid: &str, call_id: &str, name: &str, result: &str) -> R<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO tool_calls(call_id,session_id,name,ok,result,created_at) VALUES(?1,?2,?3,0,?4,?5)",
                params![call_id, sid, name, result, now_ms()],
            )
            .map_err(e)?;
        Ok(())
    }

    /// Previously processed result for an idempotent replay of the same call id.
    pub fn tool_result(&self, call_id: &str) -> Option<(bool, String)> {
        self.conn.query_row("SELECT ok, result FROM tool_calls WHERE call_id=?1", [call_id], |r| Ok((r.get(0)?, r.get(1)?))).optional().ok().flatten()
    }

    pub fn tool_counts(&self, sid: &str) -> (i64, i64) {
        self.conn
            .query_row("SELECT count(*), COALESCE(sum(ok=0),0) FROM tool_calls WHERE session_id=?1", [sid], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or((0, 0))
    }

    // ---------- transcript ----------

    pub fn upsert_turn(&self, t: &Turn) -> R<()> {
        self.conn
            .execute(
                "INSERT INTO turns(id,session_id,seq,provider_item_id,speaker,started_at,ended_at,text,final,interrupted,failed,spoken_text)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(id) DO UPDATE SET provider_item_id=COALESCE(?4,provider_item_id), ended_at=?7, text=?8,
                 final=?9, interrupted=?10, failed=?11, spoken_text=?12",
                params![t.id, t.session_id, t.seq, t.provider_item_id, t.speaker, t.started_at, t.ended_at, t.text,
                    t.is_final, t.interrupted, t.failed, t.spoken_text],
            )
            .map_err(e)?;
        Ok(())
    }

    pub fn turns(&self, sid: &str) -> R<Vec<Turn>> {
        let mut st = self.conn.prepare("SELECT * FROM turns WHERE session_id=?1 ORDER BY seq").map_err(e)?;
        let rows = st
            .query_map([sid], |r| {
                Ok(Turn {
                    id: r.get("id")?,
                    session_id: r.get("session_id")?,
                    seq: r.get("seq")?,
                    provider_item_id: r.get("provider_item_id")?,
                    speaker: r.get("speaker")?,
                    started_at: r.get("started_at")?,
                    ended_at: r.get("ended_at")?,
                    text: r.get("text")?,
                    is_final: r.get("final")?,
                    interrupted: r.get("interrupted")?,
                    failed: r.get("failed")?,
                    spoken_text: r.get("spoken_text")?,
                    original_text: r.get("original_text")?,
                    corrected_at: r.get("corrected_at")?,
                })
            })
            .map_err(e)?;
        rows.collect::<Result<_, _>>().map_err(e)
    }

    pub fn correct_turn(&self, id: &str, text: &str) -> R<()> {
        let n = self
            .conn
            .execute(
                "UPDATE turns SET original_text=COALESCE(original_text,text), text=?2, corrected_at=?3 WHERE id=?1 AND final=1",
                params![id, text, now_ms()],
            )
            .map_err(e)?;
        if n == 0 {
            return Err("Alleen definitieve beurten kunnen worden gecorrigeerd".into());
        }
        Ok(())
    }

    // ---------- usage ledger ----------

    /// Append a usage event. Duplicate dedupe keys are ignored (returns false). A measured row
    /// supersedes the estimate for the same response so it is never counted twice.
    pub fn record_usage(
        &self,
        dedupe_key: &str,
        sid: Option<&str>,
        scope: &str,
        model: &str,
        response_id: Option<&str>,
        kind: &str,
        usage: &Usage,
        pricing: &Pricing,
        supersedes: Option<&str>,
    ) -> R<bool> {
        let (cost, err) = match ledger::cost(pricing, model, usage) {
            Ok(c) => (Some(c.to_string()), None),
            Err(x) => (None, Some(x)),
        };
        let n = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO usage_events(dedupe_key,session_id,scope,model,response_id,kind,usage_json,cost_usd,cost_error,price_version,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![dedupe_key, sid, scope, model, response_id, kind, serde_json::to_string(usage).unwrap(), cost, err, pricing.version, now_ms()],
            )
            .map_err(e)?;
        if n > 0 {
            if let Some(k) = supersedes {
                self.conn.execute("UPDATE usage_events SET superseded=1 WHERE dedupe_key=?1", [k]).map_err(e)?;
            }
        }
        Ok(n > 0)
    }

    pub fn usage_rows(&self, filter_sql: &str, p: &[&dyn rusqlite::ToSql]) -> R<Vec<UsageRow>> {
        let mut st = self.conn.prepare(&format!("SELECT * FROM usage_events WHERE {filter_sql} ORDER BY id")).map_err(e)?;
        let rows = st
            .query_map(p, |r| {
                Ok(UsageRow {
                    id: r.get("id")?,
                    session_id: r.get("session_id")?,
                    scope: r.get("scope")?,
                    model: r.get("model")?,
                    response_id: r.get("response_id")?,
                    kind: r.get("kind")?,
                    usage: serde_json::from_str(&r.get::<_, String>("usage_json")?).unwrap_or_default(),
                    cost_usd: r.get::<_, Option<String>>("cost_usd")?.and_then(|s| Decimal::from_str(&s).ok()),
                    cost_error: r.get("cost_error")?,
                    price_version: r.get("price_version")?,
                    superseded: r.get("superseded")?,
                    created_at: r.get("created_at")?,
                })
            })
            .map_err(e)?;
        rows.collect::<Result<_, _>>().map_err(e)
    }

    pub fn summarize(rows: &[UsageRow]) -> CostSummary {
        let mut s = CostSummary::default();
        for r in rows.iter().filter(|r| !r.superseded) {
            let c = r.cost_usd.unwrap_or_default();
            s.total_usd += c;
            match r.kind.as_str() {
                "measured" => s.measured_usd += c,
                _ => {
                    s.estimated_usd += c;
                    s.incomplete = true;
                }
            }
            if r.cost_error.is_some() {
                s.incomplete = true;
                s.unresolved.push(r.cost_error.clone().unwrap());
            }
            let u = &r.usage;
            s.usage.text_in += u.text_in;
            s.usage.text_cached += u.text_cached;
            s.usage.audio_in += u.audio_in;
            s.usage.audio_cached += u.audio_cached;
            s.usage.text_out += u.text_out;
            s.usage.audio_out += u.audio_out;
            s.usage.reasoning += u.reasoning;
            if let Some(b) = u.billed_seconds {
                *s.usage.billed_seconds.get_or_insert(Decimal::ZERO) += b;
            }
            s.unresolved.extend(u.unresolved.iter().cloned());
            if r.scope == "session" && r.response_id.is_some() {
                s.responses += 1;
            }
        }
        s
    }

    pub fn session_cost(&self, sid: &str) -> CostSummary {
        Self::summarize(&self.usage_rows("session_id=?1", &[&sid]).unwrap_or_default())
    }

    pub fn month_cost(&self) -> CostSummary {
        Self::summarize(
            &self
                .usage_rows("created_at >= CAST(strftime('%s', 'now', 'localtime', 'start of month', 'utc') AS INTEGER) * 1000", &[])
                .unwrap_or_default(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ToolCtx;
    use serde_json::json;

    fn session(db: &Db, id: &str) {
        let p = Pricing::bundled();
        db.insert_session(&SessionRow {
            id: id.into(),
            title: "Test".into(),
            schema_version: SCHEMA_VERSION,
            created_at: now_ms(),
            started_at: None,
            ended_at: None,
            deadline_at: None,
            last_heartbeat: None,
            status: "DRAFT".into(),
            stop_reason: None,
            style: "neutraal".into(),
            profile: "quality".into(),
            model: "gpt-realtime-2.1".into(),
            transcribe_model: "gpt-live-transcribe".into(),
            parent_id: None,
            price_version: p.version.clone(),
            pricing_json: serde_json::to_string(&p).unwrap(),
            budget_usd: Decimal::new(200, 2),
            metrics_json: "{}".into(),
        })
        .unwrap();
    }

    #[test]
    fn ledger_dedupe_estimate_supersede_and_reconnect() {
        let db = Db::memory();
        session(&db, "s1");
        let p = Pricing::bundled();
        let u = Usage { text_in: 1_000_000, ..Default::default() }; // $4
        assert!(db.record_usage("est:r1", Some("s1"), "session", "gpt-realtime-2.1", Some("r1"), "estimated", &u, &p, None).unwrap());
        let s = db.session_cost("s1");
        assert!(s.incomplete);
        assert_eq!(s.total_usd, Decimal::from(4));
        // measured replaces the estimate
        let m = Usage { text_in: 500_000, ..Default::default() }; // $2
        assert!(db.record_usage("resp:r1", Some("s1"), "session", "gpt-realtime-2.1", Some("r1"), "measured", &m, &p, Some("est:r1")).unwrap());
        // duplicate delivery (e.g. after reconnect) is ignored
        assert!(!db.record_usage("resp:r1", Some("s1"), "session", "gpt-realtime-2.1", Some("r1"), "measured", &m, &p, Some("est:r1")).unwrap());
        let s = db.session_cost("s1");
        assert_eq!(s.total_usd, Decimal::from(2));
        assert!(!s.incomplete);
        // setup usage counts in totals but not in the session
        db.record_usage("setup:x", None, "setup", "gpt-live-transcribe", None, "measured", &Usage { billed_seconds: Some(Decimal::from(60)), ..Default::default() }, &p, None).unwrap();
        let all = Db::summarize(&db.usage_rows("1=1", &[]).unwrap());
        assert_eq!(all.total_usd, Decimal::from_str("2.017").unwrap());
    }

    #[test]
    fn price_snapshot_survives_pricing_update() {
        let db = Db::memory();
        session(&db, "s1");
        let p = Pricing::bundled();
        db.record_usage("a", Some("s1"), "session", "gpt-realtime-2.1", Some("r"), "measured", &Usage { text_out: 1_000_000, ..Default::default() }, &p, None).unwrap();
        let mut s = db.settings();
        let mut newer = p.clone();
        newer.version = "2027-01-01".into();
        newer.models.get_mut("gpt-realtime-2.1").unwrap().text_out = Some(Decimal::from(99));
        s.pricing_override = Some(serde_json::to_string(&newer).unwrap());
        db.save_settings(&s).unwrap();
        assert_eq!(db.pricing().version, "2027-01-01");
        let rows = db.usage_rows("1=1", &[]).unwrap();
        assert_eq!((rows[0].cost_usd, rows[0].price_version.as_str()), (Some(Decimal::from(24)), "2026-10-04"));
        assert_eq!(db.session("s1").unwrap().price_version, "2026-10-04");
    }

    #[test]
    fn missing_price_marks_incomplete_not_zero() {
        let db = Db::memory();
        session(&db, "s1");
        db.record_usage("a", Some("s1"), "session", "unknown-model", Some("r"), "measured", &Usage { text_in: 10, ..Default::default() }, &Pricing::bundled(), None).unwrap();
        let s = db.session_cost("s1");
        assert!(s.incomplete);
        assert_eq!(s.unresolved.len(), 1);
    }

    #[test]
    fn mutations_are_idempotent_and_deletable() {
        let mut db = Db::memory();
        session(&db, "s1");
        let c = db.canvas("s1").unwrap();
        let ctx = ToolCtx { last_user_turn: Some("t1".into()), now: 1 };
        let m = crate::canvas::validate(crate::canvas::FILL, &json!({"field":"kpi","value":"Doorlooptijd"}), &c, &ctx).unwrap();
        db.apply("s1", &m, Some(("call1", crate::canvas::FILL, true, "ok"))).unwrap();
        assert!(db.apply("s1", &m, Some(("call1", crate::canvas::FILL, true, "ok"))).is_err(), "same call id cannot apply twice");
        assert_eq!(db.tool_result("call1"), Some((true, "ok".into())));
        assert_eq!(db.canvas("s1").unwrap().items.len(), 1);
        db.upsert_turn(&Turn { id: "t1".into(), session_id: "s1".into(), seq: 1, provider_item_id: None, speaker: "user".into(), started_at: 1, ended_at: Some(2), text: "hallo".into(), is_final: true, interrupted: false, failed: false, spoken_text: None, original_text: None, corrected_at: None }).unwrap();
        db.correct_turn("t1", "Hallo!").unwrap();
        assert_eq!(db.turns("s1").unwrap()[0].original_text.as_deref(), Some("hallo"));
        db.record_usage("u", Some("s1"), "session", "gpt-realtime-2.1", Some("r"), "measured", &Usage::default(), &Pricing::bundled(), None).unwrap();
        db.delete_session("s1", true).unwrap();
        assert!(db.session("s1").is_err());
        assert!(db.canvas("s1").unwrap().items.is_empty() && db.turns("s1").unwrap().is_empty());
        assert_eq!(db.usage_rows("1=1", &[]).unwrap().len(), 1, "anonymous cost ledger kept");
    }

    #[test]
    fn encrypted_file_unreadable_without_key() {
        let dir = std::env::temp_dir().join(format!("cf-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let key = "ab".repeat(32);
        {
            let db = Db::open(&path, &key).unwrap();
            session(&db, "geheim-sessie-titel");
            db.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        }
        for entry in std::fs::read_dir(&dir).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("geheim-sessie-titel"));
            assert!(!String::from_utf8_lossy(&bytes).contains("SQLite format"));
        }
        assert!(Db::open(&path, &"cd".repeat(32)).is_err());
        assert!(Db::open(&path, &key).is_ok());
        std::fs::remove_dir_all(dir).ok();
    }
}
