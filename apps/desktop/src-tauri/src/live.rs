//! Live session controller: owns the deadline, both OpenAI connections, audio, tool validation,
//! usage ledger and budget. Every outbound AI message passes through `send`, the single gate
//! that refuses traffic after the deadline (PRD §6) regardless of renderer or model.

use crate::audio::{self, AudioEvent};
use crate::canvas::{self, Mutation, ToolCtx};
use crate::clock::{self, Clock, Tick};
use crate::db::{now_ms, CostSummary, Db, Turn};
use crate::ledger::{self, Pricing, Usage};
use crate::{diag, prompt, secrets};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

/// Versioned model configuration (PRD §7). The only place model ids live.
pub mod models {
    pub const QUALITY: &str = "gpt-realtime-2.1";
    pub const MINI: &str = "gpt-realtime-2.1-mini";
    pub const TRANSCRIBE: &str = "gpt-live-transcribe";
    pub const PROFILES: [&str; 2] = ["quality", "mini"];
    pub fn for_profile(p: &str) -> &'static str {
        if p == "mini" {
            MINI
        } else {
            QUALITY
        }
    }
}

/// Fixed OpenAI host; credentials are never sent anywhere else.
pub const HOST: &str = "api.openai.com";
pub const MAX_OUTPUT_TOKENS: i64 = 800;
const MAX_TOOL_FOLLOWUPS: u32 = 4;
const MAX_CONTEXT_ITEMS: usize = 60;
const TRIM_ITEMS: usize = 20;
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Conn {
    Rt,
    Tr,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct Metrics {
    pub elapsed_ms: i64,
    pub paused_ms: i64,
    pub user_speech_ms: i64,
    pub streamed_audio_secs: f64,
    pub user_turns: i64,
    pub assistant_turns: i64,
    pub text_turns: i64,
    pub responses: i64,
    pub cancelled_responses: i64,
    pub tool_calls: i64,
    pub tool_rejections: i64,
    pub reconnects: i64,
    pub errors: i64,
    pub transcript_failures: i64,
    pub lat_first_delta_ms: Vec<i64>,
    pub lat_final_transcript_ms: Vec<i64>,
    /// User turn end → first text of the facilitator's reply.
    pub lat_first_reply_ms: Vec<i64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub session_id: String,
    pub status: String,
    pub cost_usd: Decimal,
    pub cost_incomplete: bool,
    pub budget_usd: Decimal,
    pub voice_state: String,
    pub connection: String,
    pub muted: bool,
    pub paused: bool,
    pub model: String,
    pub user_turns: i64,
    pub audio_in_secs: f64,
    /// Inspiration/proposals next to the question, and the fields the question is about.
    pub guide: Option<canvas::Guide>,
    /// Field the presenter clicked to fill in again, until it is rewritten.
    pub refill: Option<canvas::FieldRef>,
}

#[derive(Default)]
struct Conns {
    rt: Option<mpsc::UnboundedSender<Message>>,
    tr: Option<mpsc::UnboundedSender<Message>>,
}

#[derive(Default)]
struct St {
    canvas: canvas::Canvas,
    seq: i64,
    turns: HashMap<String, Turn>,
    pending_tr: VecDeque<String>,
    tr_items: HashMap<String, String>,
    asst_items: HashMap<String, String>,
    speaking_turn: Option<String>,
    turn_timing: HashMap<String, (Instant, Option<Instant>, bool)>,
    last_user_turn: Option<String>,
    in_flight: Option<String>,
    /// Reply requested while another response was in flight (Ask wins over a silent update).
    queued: Option<Reply>,
    /// Inspiration requested while busy; runs after the queued reply (dropped when a new question comes).
    queued_inspire: bool,
    guide: Option<canvas::Guide>,
    refill: Option<canvas::FieldRef>,
    /// Kinds of requested responses not yet acknowledged by `response.created`, in order.
    requested: VecDeque<Reply>,
    reply_kind: HashMap<String, Reply>,
    awaiting_reply_since: Option<Instant>,
    tool_followups: u32,
    items: VecDeque<String>,
    budget_warned: u8,
    status: String,
    connection: String,
    unbilled_secs: f64,
    /// Session cost, refreshed after each ledger write (not per snapshot).
    cost: CostSummary,
    pause_started: Option<i64>,
    last_ckpt: HashMap<String, i64>,
    last_emit: HashMap<String, i64>,
    /// Last snapshot sent to the UI; unchanged snapshots are not re-sent.
    last_snapshot: String,
    m: Metrics,
}

pub struct Live {
    pub sid: String,
    app: AppHandle,
    db: Arc<Mutex<Db>>,
    model: String,
    pricing: Pricing,
    budget: Decimal,
    month_budget: Decimal,
    style: String,
    title: String,
    input_dev: Mutex<Option<String>>,
    clock: Mutex<Clock>,
    closing: AtomicBool,
    paused: AtomicBool,
    muted: AtomicBool,
    /// Microphone gate read by the audio thread: false while muted, paused or closed.
    capture: Arc<AtomicBool>,
    conns: Mutex<Conns>,
    generation: AtomicU64,
    audio: Mutex<Option<audio::AudioHandle>>,
    st: Mutex<St>,
}

/// What a model response may do: silently update the canvas, show inspiration next to the
/// question (`show_guide`), or show the next question.
/// Questions advance when the model judges the current one answered (`question_answered`)
/// or when the presenter presses the button, N or PageDown. Inspiration follows every new
/// question and is refreshed with I.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Reply {
    Silent,
    Inspire,
    Ask,
}

#[derive(Debug)]
enum ConnectError {
    Fatal(String),
    Retry(String),
    Expired,
}

fn emit<S: Serialize + Clone>(app: &AppHandle, ev: &str, payload: S) {
    let _ = app.emit(ev, payload);
}

pub fn notice(app: &AppHandle, level: &str, text: &str) {
    diag::log(format!("notice[{level}] {text}"));
    emit(app, "notice", json!({"level": level, "text": text}));
}

fn pct(v: &mut [i64], p: f64) -> Option<i64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[((v.len() as f64 - 1.0) * p).round() as usize])
}

impl Live {
    /// Start (or explicitly resume after a crash) a session. The deadline is fixed and persisted
    /// before any audio or AI traffic is allowed.
    pub async fn start(app: AppHandle, dbh: Arc<Mutex<Db>>, sid: &str) -> Result<Arc<Live>, String> {
        let (row, settings, canvas, turns, month, cost) = {
            let d = dbh.lock().unwrap();
            (d.session(sid)?, d.settings(), d.canvas(sid)?, d.turns(sid)?, d.month_cost(), d.session_cost(sid))
        };
        if secrets::get(secrets::API_KEY)?.is_none() {
            return Err("Geen OpenAI API-key ingesteld".into());
        }
        let now = now_ms();
        let resuming = row.started_at.is_some();
        let clock = if resuming {
            let resumable = row.status == "PAUSED" || (row.status == "INTERRUPTED" && row.stop_reason.as_deref() == Some("netwerk"));
            if !resumable {
                return Err("Deze sessie kan niet worden hervat".into());
            }
            Clock::recover(row.started_at.unwrap(), row.last_heartbeat.unwrap_or(0), now)
                .ok_or("De sessietijd is verstreken of de klok is onbetrouwbaar; de sessie blijft afgesloten")?
        } else {
            if row.status != "DRAFT" {
                return Err("Sessie is al gestart".into());
            }
            Clock::start(now)
        };
        let pricing: Pricing = serde_json::from_str(&row.pricing_json).map_err(|e| e.to_string())?;
        let reserve = ledger::reservation(&pricing, &row.model, models::TRANSCRIBE, MAX_OUTPUT_TOKENS);
        if !ledger::within_budget(month.total_usd, reserve, settings.monthly_budget_usd) {
            return Err("Maandbudget (inclusief 10% marge) is bereikt; verhoog het budget in Instellingen".into());
        }
        {
            let d = dbh.lock().unwrap();
            if !resuming {
                d.set_started(sid, now, clock.deadline_wall_ms())?;
            }
            d.set_status(sid, "CONNECTING", None)?;
        }
        let prior_metrics: Metrics = serde_json::from_str(&row.metrics_json).unwrap_or_default();
        let live = Arc::new(Live {
            sid: sid.into(),
            app: app.clone(),
            db: dbh,
            model: row.model.clone(),
            pricing,
            budget: row.budget_usd,
            month_budget: settings.monthly_budget_usd,
            style: row.style.clone(),
            title: row.title.clone(),
            input_dev: Mutex::new(settings.input_device.clone()),
            clock: Mutex::new(clock),
            closing: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            capture: Arc::new(AtomicBool::new(true)),
            conns: Mutex::new(Conns::default()),
            generation: AtomicU64::new(0),
            audio: Mutex::new(None),
            st: Mutex::new(St {
                seq: turns.iter().map(|t| t.seq).max().unwrap_or(0),
                last_user_turn: turns.iter().rev().find(|t| t.speaker == "user").map(|t| t.id.clone()),
                canvas,
                status: "CONNECTING".into(),
                connection: "verbinden".into(),
                m: prior_metrics,
                cost,
                ..Default::default()
            }),
        });
        live.clone().spawn_watchdog();
        live.emit_snapshot();
        if let Err(e) = live.open_streams(resuming).await {
            return Err(live.fail_connect(e));
        }
        live.set_status("ACTIVE");
        diag::log(format!("session {sid} started (resume={resuming}) model={}", live.model));
        Ok(live)
    }

    // ---------- gate ----------

    fn remaining(&self) -> i64 {
        self.clock.lock().unwrap().remaining_ms(now_ms())
    }

    /// True while AI traffic is permitted. Checked on every outbound message.
    pub fn allowed(&self) -> bool {
        !self.closing.load(SeqCst) && !self.clock.lock().unwrap().expired(now_ms())
    }

    pub fn is_closed(&self) -> bool {
        self.closing.load(SeqCst)
    }

    fn send(&self, c: Conn, v: Value) -> bool {
        self.send_msg(c, Message::Text(v.to_string().into()))
    }

    fn send_msg(&self, c: Conn, m: Message) -> bool {
        if !self.allowed() {
            return false;
        }
        let conns = self.conns.lock().unwrap();
        let tx = match c {
            Conn::Rt => conns.rt.as_ref(),
            Conn::Tr => conns.tr.as_ref(),
        };
        tx.map(|t| t.send(m).is_ok()).unwrap_or(false)
    }

    /// Append to the ledger and refresh the cached session cost.
    fn usage(&self, key: &str, model: &str, response_id: Option<&str>, kind: &str, u: &Usage, supersedes: Option<&str>) {
        let cost = {
            let d = self.db.lock().unwrap();
            let _ = d.record_usage(key, Some(&self.sid), "session", model, response_id, kind, u, &self.pricing, supersedes);
            d.session_cost(&self.sid)
        };
        self.st.lock().unwrap().cost = cost;
        self.emit_snapshot();
    }

    fn system_item(&self, text: &str) {
        self.send(Conn::Rt, json!({"type": "conversation.item.create", "item": {
            "type": "message", "role": "system", "content": [{"type": "input_text", "text": text}]}}));
    }

    // ---------- connections ----------

    async fn connect(self: &Arc<Self>, c: Conn, generation: u64) -> Result<(), ConnectError> {
        let url = match c {
            Conn::Rt => format!("wss://{HOST}/v1/realtime?model={}", self.model),
            Conn::Tr => format!("wss://{HOST}/v1/realtime?intent=transcription"),
        };
        let mut req = url.into_client_request().map_err(|e| ConnectError::Fatal(e.to_string()))?;
        {
            let key = secrets::get(secrets::API_KEY).map_err(ConnectError::Fatal)?.ok_or(ConnectError::Fatal("Geen API-key".into()))?;
            let mut hv = HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| ConnectError::Fatal("Ongeldige API-key".into()))?;
            hv.set_sensitive(true);
            req.headers_mut().insert("Authorization", hv);
        }
        let budget = Duration::from_millis(self.remaining().clamp(0, 15_000) as u64);
        let (ws, _) = match tokio::time::timeout(budget, tokio_tungstenite::connect_async(req)).await {
            Err(_) => return Err(ConnectError::Retry("Time-out bij verbinden".into())),
            Ok(Err(tokio_tungstenite::tungstenite::Error::Http(resp))) => {
                let s = resp.status().as_u16();
                let msg = match s {
                    401 => "API-key ongeldig of ingetrokken (401)".to_string(),
                    403 | 404 => format!("Geen toegang tot model {} ({s})", if c == Conn::Rt { &self.model } else { models::TRANSCRIBE }),
                    429 => "Quotum of rate limit bereikt bij OpenAI (429)".to_string(),
                    _ => format!("OpenAI weigerde de verbinding ({s})"),
                };
                return Err(if s >= 500 { ConnectError::Retry(msg) } else { ConnectError::Fatal(msg) });
            }
            Ok(Err(e)) => return Err(ConnectError::Retry(format!("Netwerkfout: {e}"))),
            Ok(Ok(v)) => v,
        };
        let (mut w, mut r) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        tauri::async_runtime::spawn(async move {
            while let Some(m) = rx.recv().await {
                if w.send(m).await.is_err() {
                    break;
                }
            }
            let _ = w.close().await;
        });
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(msg) = r.next().await {
                match msg {
                    Ok(Message::Text(t)) => {
                        if me.generation.load(SeqCst) != generation || me.closing.load(SeqCst) {
                            break;
                        }
                        if let Ok(v) = serde_json::from_str::<Value>(&t) {
                            match c {
                                Conn::Rt => me.on_rt(v),
                                Conn::Tr => me.on_tr(v),
                            }
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
            me.on_disconnect(generation);
        });
        let mut conns = self.conns.lock().unwrap();
        match c {
            Conn::Rt => conns.rt = Some(tx),
            Conn::Tr => conns.tr = Some(tx),
        }
        Ok(())
    }

    fn configure(&self, resumed: bool) {
        self.send(Conn::Rt, json!({"type": "session.update", "session": {
            "type": "realtime",
            "instructions": prompt::instructions(&self.style, &self.title),
            // The facilitator never speaks: questions appear as text on the (presentation) screen.
            "output_modalities": ["text"],
            "audio": {
                "input": {"format": {"type": "audio/pcm", "rate": audio::RATE}, "turn_detection": null}
            },
            "tools": canvas::tool_schemas(),
            "tool_choice": "auto",
            "max_output_tokens": MAX_OUTPUT_TOKENS
        }}));
        self.send(Conn::Tr, json!({"type": "session.update", "session": {
            "type": "transcription",
            "audio": {"input": {
                "format": {"type": "audio/pcm", "rate": audio::RATE},
                "transcription": {"model": models::TRANSCRIBE, "languages": ["nl"],
                    "prompt": "Nederlandstalig zakelijk gesprek over een AI-idee, KPI's en bedrijfsprocessen."},
                "turn_detection": null
            }}
        }}));
        let (summary, has_content) = {
            let st = self.st.lock().unwrap();
            (st.canvas.summary(), st.canvas != canvas::Canvas::default())
        };
        if resumed || has_content {
            let prefix = if resumed { "Het gesprek is hervat na een onderbreking. " } else { "Vervolgsessie: eerder bevestigde context. " };
            self.system_item(&format!("{prefix}Huidige canvascontext (leidend):\n{summary}"));
        }
    }

    /// Open both connections and audio; used at start, after suspend and on reconnect.
    async fn open_streams(self: &Arc<Self>, resumed: bool) -> Result<(), ConnectError> {
        let generation = self.generation.fetch_add(1, SeqCst) + 1;
        self.set_connection("verbinden");
        let mut last = None;
        for attempt in 0..3u32 {
            if !self.allowed() {
                return Err(ConnectError::Expired);
            }
            let res = match self.connect(Conn::Rt, generation).await {
                Ok(()) => self.connect(Conn::Tr, generation).await,
                Err(e) => Err(e),
            };
            match res {
                Ok(()) => {
                    last = None;
                    break;
                }
                Err(ConnectError::Retry(m)) => {
                    self.drop_conns();
                    diag::log(format!("connect attempt {} failed: {m}", attempt + 1));
                    last = Some(ConnectError::Retry(m));
                    let wait = 1000u64 << attempt;
                    if self.remaining() <= wait as i64 + clock::MIN_RESPONSE_MS {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                }
                Err(e) => {
                    self.drop_conns();
                    return Err(e);
                }
            }
        }
        if let Some(e) = last {
            return Err(e);
        }
        self.configure(resumed);
        self.start_audio();
        self.set_connection("verbonden");
        if !resumed {
            // Facilitator opens with its first question.
            self.request(Reply::Ask);
        }
        Ok(())
    }

    /// Close the session for a connection failure; returns the text for the user.
    fn fail_connect(&self, e: ConnectError) -> String {
        match e {
            ConnectError::Fatal(m) => {
                self.finish("FAILED", "verbinding");
                m
            }
            // Transient network failures stay resumable within the original deadline.
            ConnectError::Retry(m) => {
                self.finish("INTERRUPTED", "netwerk");
                m
            }
            ConnectError::Expired => {
                self.finish("COMPLETED", "deadline");
                "De sessietijd is verstreken".into()
            }
        }
    }

    fn drop_conns(&self) {
        *self.conns.lock().unwrap() = Conns::default();
    }

    fn close_streams(&self) {
        self.generation.fetch_add(1, SeqCst);
        self.drop_conns();
        self.audio.lock().unwrap().take();
        let mut st = self.st.lock().unwrap();
        st.in_flight = None;
        st.pending_tr.clear();
        st.speaking_turn = None;
    }

    fn start_audio(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        let sink: audio::Sink = Arc::new(move |e| {
            if let Some(l) = me.upgrade() {
                l.on_audio(e)
            }
        });
        self.capture.store(!self.muted.load(SeqCst) && !self.paused.load(SeqCst), SeqCst);
        let dev = self.input_dev.lock().unwrap().clone();
        match audio::start(dev, self.capture.clone(), sink) {
            Ok(h) => *self.audio.lock().unwrap() = Some(h),
            // Text input stays possible without a microphone.
            Err(e) => notice(&self.app, "warn", &format!("{e}. Je kunt typen of een ander apparaat kiezen.")),
        }
    }

    pub fn switch_input(self: &Arc<Self>, device: Option<String>) {
        *self.input_dev.lock().unwrap() = device;
        self.audio.lock().unwrap().take();
        self.start_audio();
    }

    fn on_disconnect(self: &Arc<Self>, generation: u64) {
        if self.closing.load(SeqCst) || self.generation.load(SeqCst) != generation || !self.allowed() {
            return;
        }
        diag::log("connection lost; reconnecting");
        self.close_streams();
        self.st.lock().unwrap().m.reconnects += 1;
        self.set_connection("herverbinden");
        notice(&self.app, "warn", "Verbinding verbroken; opnieuw verbinden (max. 3 pogingen)…");
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            match me.open_streams(true).await {
                Ok(()) => notice(&me.app, "info", "Verbinding hersteld; de canvascontext is opnieuw gedeeld."),
                Err(e) => {
                    let msg = me.fail_connect(e);
                    notice(&me.app, "error", &format!("Verbinding niet hersteld: {msg}"));
                }
            }
        });
    }

    // ---------- watchdog ----------

    fn spawn_watchdog(self: Arc<Self>) {
        tauri::async_runtime::spawn(async move {
            let mut n: u64 = 0;
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if self.closing.load(SeqCst) {
                    break;
                }
                let tick = self.clock.lock().unwrap().tick(now_ms());
                match tick {
                    Tick::Expired => {
                        self.finish("COMPLETED", "deadline");
                        break;
                    }
                    Tick::ClockChanged => {
                        notice(&self.app, "error", "Systeemklok gewijzigd; de sessie is veiligheidshalve afgesloten.");
                        self.finish("INTERRUPTED", "klokwijziging");
                        break;
                    }
                    Tick::Suspended => {
                        diag::log("suspend detected");
                        self.close_streams();
                        self.paused.store(true, SeqCst);
                        self.capture.store(false, SeqCst);
                        self.st.lock().unwrap().pause_started.get_or_insert(now_ms());
                        self.set_connection("gesloten");
                        self.set_status("PAUSED");
                        notice(&self.app, "warn", "Slaapstand gedetecteerd: verbindingen gesloten. De tijd liep door; hervat als er nog tijd is.");
                    }
                    Tick::Running => {}
                }
                n += 1;
                if n % 5 == 0 {
                    self.emit_snapshot();
                }
                if n % 20 == 0 {
                    self.every_second();
                }
            }
        });
    }

    fn every_second(self: &Arc<Self>) {
        let now = now_ms();
        let _ = self.db.lock().unwrap().heartbeat(&self.sid, now);
        // Budget: warnings at 80/95% of the usable (post-margin) budget.
        let spent = self.st.lock().unwrap().cost.total_usd;
        let usable = ledger::usable_budget(self.budget);
        let ratio = if usable.is_zero() { Decimal::ONE } else { spent / usable };
        let mut st = self.st.lock().unwrap();
        for (lvl, pct) in [(1u8, 80), (2u8, 95)] {
            if st.budget_warned < lvl && ratio >= Decimal::new(pct, 2) {
                st.budget_warned = lvl;
                notice(&self.app, "warn", &format!("{pct}% van het sessiebudget gebruikt"));
            }
        }
        drop(st);
        if spent >= usable {
            notice(&self.app, "warn", "Sessiebudget bereikt; de sessie wordt afgesloten.");
            self.finish("COMPLETED", "budget");
        }
    }

    // ---------- state & UI ----------

    fn set_status(&self, s: &str) {
        let _ = self.db.lock().unwrap().set_status(&self.sid, s, None);
        self.st.lock().unwrap().status = s.into();
        self.emit_snapshot();
    }

    fn set_connection(&self, s: &str) {
        self.st.lock().unwrap().connection = s.into();
        self.emit_snapshot();
    }

    pub fn snapshot(&self) -> Snapshot {
        let st = self.st.lock().unwrap();
        let voice = if self.paused.load(SeqCst) {
            "gepauzeerd"
        } else if st.in_flight.is_some() {
            "denkt"
        } else if self.muted.load(SeqCst) {
            "gedempt"
        } else {
            "luistert"
        };
        Snapshot {
            session_id: self.sid.clone(),
            status: st.status.clone(),
            cost_usd: st.cost.total_usd,
            cost_incomplete: st.cost.incomplete,
            budget_usd: self.budget,
            voice_state: voice.into(),
            connection: st.connection.clone(),
            muted: self.muted.load(SeqCst),
            paused: self.paused.load(SeqCst),
            model: self.model.clone(),
            user_turns: st.m.user_turns,
            audio_in_secs: st.m.user_speech_ms as f64 / 1000.0,
            guide: st.guide.clone(),
            refill: st.refill.clone(),
        }
    }

    pub fn emit_snapshot(&self) {
        let s = self.snapshot();
        let key = serde_json::to_string(&s).unwrap_or_default();
        {
            let mut st = self.st.lock().unwrap();
            if st.last_snapshot == key {
                return;
            }
            st.last_snapshot = key;
        }
        emit(&self.app, "live", s);
    }

    fn emit_canvas(&self) {
        let view = self.st.lock().unwrap().canvas.view();
        emit(&self.app, "canvas", json!({"sessionId": self.sid, "view": view}));
    }

    fn save_turn(&self, t: &Turn, force: bool) {
        let now = now_ms();
        let write = {
            let mut st = self.st.lock().unwrap();
            let last = st.last_ckpt.get(&t.id).copied().unwrap_or(0);
            let w = force || t.is_final || now - last >= 1000; // provisional checkpoint ≤ 1/s
            if w {
                st.last_ckpt.insert(t.id.clone(), now);
            }
            st.turns.insert(t.id.clone(), t.clone());
            w
        };
        if write {
            let res = self.db.lock().unwrap().upsert_turn(t);
            if let Err(e) = res {
                self.storage_failure(&e);
            }
        }
        // Provisional text: at most ~8 UI updates per second per turn (final always goes out).
        let show = force || t.is_final || {
            let mut st = self.st.lock().unwrap();
            let last = st.last_emit.get(&t.id).copied().unwrap_or(0);
            let due = now - last >= 120;
            if due {
                st.last_emit.insert(t.id.clone(), now);
            }
            due
        };
        if show {
            emit(&self.app, "turn", t);
        }
    }

    fn storage_failure(&self, e: &str) {
        notice(&self.app, "error", &format!("Opslaan mislukt ({e}). Het gesprek stopt; de laatst bevestigde staat blijft bewaard."));
        self.finish("INTERRUPTED", "opslag");
    }

    fn new_turn(&self, speaker: &str, text: &str, is_final: bool) -> Turn {
        let mut st = self.st.lock().unwrap();
        st.seq += 1;
        Turn {
            id: canvas::new_id(),
            session_id: self.sid.clone(),
            seq: st.seq,
            provider_item_id: None,
            speaker: speaker.into(),
            started_at: now_ms(),
            ended_at: is_final.then(now_ms),
            text: text.into(),
            is_final,
            interrupted: false,
            failed: false,
            spoken_text: None,
            original_text: None,
            corrected_at: None,
        }
    }

    // ---------- responses & budget ----------

    fn budget_ok(self: &Arc<Self>) -> bool {
        let reserve = ledger::reservation(&self.pricing, &self.model, models::TRANSCRIBE, MAX_OUTPUT_TOKENS);
        let spent = self.st.lock().unwrap().cost.total_usd;
        let month = self.db.lock().unwrap().month_cost().total_usd;
        if !ledger::within_budget(spent, reserve, self.budget) || !ledger::within_budget(month, reserve, self.month_budget) {
            notice(&self.app, "warn", "Onvoldoende budgetruimte voor een nieuw antwoord; de sessie wordt afgesloten.");
            self.finish("COMPLETED", "budget");
            return false;
        }
        true
    }

    /// Ask the model to respond if nothing else is generating and time/budget allow.
    fn maybe_respond(self: &Arc<Self>, kind: Reply) -> bool {
        if self.paused.load(SeqCst) || self.remaining() < clock::MIN_RESPONSE_MS {
            return false;
        }
        if self.st.lock().unwrap().in_flight.is_some() {
            return false;
        }
        if !self.budget_ok() {
            return false;
        }
        // Response-level instructions replace the session ones, so repeat them (same prefix: cacheable).
        let rule = {
            let st = self.st.lock().unwrap();
            let mut asked: Vec<&Turn> = st.turns.values().filter(|t| t.speaker == "assistant" && t.is_final && !t.text.is_empty()).collect();
            asked.sort_by_key(|t| t.seq);
            let asked: Vec<String> = asked.iter().rev().take(6).map(|t| t.text.clone()).collect();
            let focus = match &st.refill {
                Some(r) => st.canvas.refill_hint(&r.step, &r.field),
                None => st.canvas.focus_hint(),
            };
            match kind {
                Reply::Inspire => {
                    let prev = st.guide.as_ref().map(|g| g.bullets.clone()).unwrap_or_default();
                    prompt::inspire_rule(&focus, asked.first().map(String::as_str), &prev)
                }
                _ => prompt::reply_rule(kind == Reply::Ask, &focus, &asked),
            }
        };
        let instructions = format!("{}\n\n{}", prompt::instructions(&self.style, &self.title), rule);
        let ok = self.send(Conn::Rt, json!({"type": "response.create", "response": {"instructions": instructions}}));
        if ok {
            let mut st = self.st.lock().unwrap();
            // Placeholder until response.created arrives: prevents concurrent generations.
            st.in_flight = Some(String::new());
            st.requested.push_back(kind);
        }
        ok
    }

    /// Respond now, or as soon as the response in flight settles.
    fn request(self: &Arc<Self>, kind: Reply) {
        let busy = {
            let mut st = self.st.lock().unwrap();
            if st.in_flight.is_some() {
                if kind == Reply::Inspire {
                    st.queued_inspire = true;
                } else {
                    st.queued = st.queued.max(Some(kind));
                }
            }
            st.in_flight.is_some()
        };
        if !busy {
            self.maybe_respond(kind);
        }
    }

    /// Move on to the next question (button, N/PageDown, or the model's `question_answered`).
    pub fn next_question(self: &Arc<Self>) {
        self.st.lock().unwrap().tool_followups = 0;
        self.request(Reply::Ask);
    }

    /// New inspiration/proposals next to the current question (I, and after every new question).
    pub fn inspire(self: &Arc<Self>) {
        self.st.lock().unwrap().tool_followups = 0;
        self.request(Reply::Inspire);
    }

    /// The presenter clicked a tile to fill it in again: ask about that field next.
    pub fn refill_field(self: &Arc<Self>, step: &str, field: &str) -> Result<(), String> {
        let fd = canvas::field_def(step, field).ok_or("Onbekend veld")?;
        if !self.allowed() {
            return Err("Sessie is niet actief".into());
        }
        let r = canvas::FieldRef { step: step.into(), field: field.into() };
        {
            let mut st = self.st.lock().unwrap();
            st.refill = Some(r.clone());
            // Glow on the clicked tile right away; the new guide follows with the question.
            if let Some(g) = st.guide.as_mut() {
                g.fields = vec![r];
            }
        }
        self.emit_snapshot();
        self.system_item(&format!(
            "De presentator wil het veld '{}' ({step}) opnieuw invullen. Vraag daar nu naar en overschrijf de huidige waarde met het nieuwe antwoord.",
            fd.label
        ));
        self.next_question();
        Ok(())
    }

    /// Pause cancels a reply that is still being written.
    fn cancel_response(&self) {
        if self.st.lock().unwrap().in_flight.is_some() {
            self.send(Conn::Rt, json!({"type": "response.cancel"}));
        }
    }

    // ---------- audio events (audio thread) ----------

    fn on_audio(self: &Arc<Self>, e: AudioEvent) {
        match e {
            AudioEvent::Level(l) => emit(&self.app, "level", l),
            AudioEvent::SpeechStart => {
                if !self.allowed() {
                    return;
                }
                let t = self.new_turn("user", "", false);
                let mut st = self.st.lock().unwrap();
                st.speaking_turn = Some(t.id.clone());
                st.turn_timing.insert(t.id.clone(), (Instant::now(), None, false));
                st.turns.insert(t.id.clone(), t.clone());
                st.tool_followups = 0;
                drop(st);
                emit(&self.app, "turn", t);
            }
            AudioEvent::Frame(pcm) => {
                if self.paused.load(SeqCst) || self.muted.load(SeqCst) {
                    return;
                }
                let secs = pcm.len() as f64 / 2.0 / audio::RATE as f64;
                // Serialize once; base64 needs no JSON escaping.
                let m = Message::Text(format!(r#"{{"type":"input_audio_buffer.append","audio":"{}"}}"#, B64.encode(&pcm)).into());
                let a = self.send_msg(Conn::Rt, m.clone());
                let t = self.send_msg(Conn::Tr, m);
                let mut st = self.st.lock().unwrap();
                if t {
                    st.unbilled_secs += secs;
                }
                if a || t {
                    st.m.streamed_audio_secs += secs;
                }
            }
            AudioEvent::SpeechEnd { duration_ms } => {
                let Some(tid) = self.st.lock().unwrap().speaking_turn.take() else { return };
                if !self.send(Conn::Rt, json!({"type": "input_audio_buffer.commit"})) {
                    return;
                }
                self.send(Conn::Tr, json!({"type": "input_audio_buffer.commit"}));
                {
                    let mut st = self.st.lock().unwrap();
                    st.pending_tr.push_back(tid.clone());
                    st.last_user_turn = Some(tid.clone());
                    st.m.user_turns += 1;
                    st.m.user_speech_ms += duration_ms as i64;
                    if let Some(tt) = st.turn_timing.get_mut(&tid) {
                        tt.1 = Some(Instant::now());
                    }
                    st.awaiting_reply_since = Some(Instant::now());
                }
                let t = self.st.lock().unwrap().turns.get(&tid).cloned();
                if let Some(mut t) = t {
                    t.ended_at = Some(now_ms());
                    self.save_turn(&t, true);
                }
                // Keep the canvas current; the model advances via question_answered once answered.
                self.request(Reply::Silent);
            }
            AudioEvent::SpeechDiscard => {
                let tid = self.st.lock().unwrap().speaking_turn.take();
                self.send(Conn::Rt, json!({"type": "input_audio_buffer.clear"}));
                self.send(Conn::Tr, json!({"type": "input_audio_buffer.clear"}));
                if let Some(tid) = tid {
                    emit(&self.app, "turn-removed", tid);
                }
            }
            AudioEvent::DeviceFallback(m) => notice(&self.app, "warn", &m),
            AudioEvent::DeviceError(m) => {
                self.st.lock().unwrap().m.errors += 1;
                notice(&self.app, "error", &format!("{m}. Kies een ander apparaat of typ je antwoord."));
            }
        }
    }

    // ---------- realtime (speech-to-speech) events ----------

    fn on_rt(self: &Arc<Self>, v: Value) {
        let ty = v["type"].as_str().unwrap_or("");
        match ty {
            "conversation.item.added" | "conversation.item.created" => {
                if let Some(id) = v["item"]["id"].as_str() {
                    self.st.lock().unwrap().items.push_back(id.to_string());
                    self.trim_context();
                }
            }
            "response.created" => {
                let id = v["response"]["id"].as_str().unwrap_or("").to_string();
                {
                    let mut st = self.st.lock().unwrap();
                    st.in_flight = Some(id.clone());
                    let kind = st.requested.pop_front().unwrap_or(Reply::Silent);
                    st.reply_kind.insert(id.clone(), kind);
                }
                // Conservative estimate until measured usage replaces it (cancel/close stays visible).
                let est = Usage { text_in: 12_000, audio_in: 3_000, text_out: MAX_OUTPUT_TOKENS, ..Default::default() };
                self.usage(&format!("est:{id}"), &self.model, Some(&id), "estimated", &est, None);
            }
            // Text of silent canvas updates is never shown: the current question stays on screen.
            "response.output_text.delta" | "response.output_text.done" if !self.shows_text(&v) => {}
            "response.output_text.delta" => {
                let item = v["item_id"].as_str().unwrap_or("").to_string();
                let tid = self.asst_turn(&item);
                let t = {
                    let mut st = self.st.lock().unwrap();
                    if let Some(since) = st.awaiting_reply_since.take() {
                        st.m.lat_first_reply_ms.push(since.elapsed().as_millis() as i64);
                    }
                    let t = st.turns.get_mut(&tid).unwrap();
                    t.text.push_str(v["delta"].as_str().unwrap_or(""));
                    t.clone()
                };
                self.save_turn(&t, false);
            }
            "response.output_text.done" => {
                let item = v["item_id"].as_str().unwrap_or("").to_string();
                let t = {
                    let mut st = self.st.lock().unwrap();
                    let Some(tid) = st.asst_items.get(&item).cloned() else { return };
                    let Some(t) = st.turns.get_mut(&tid) else { return };
                    if let Some(tr) = v["text"].as_str() {
                        t.text = tr.to_string();
                    }
                    t.is_final = true;
                    t.ended_at = Some(now_ms());
                    st.m.assistant_turns += 1;
                    st.turns.get(&tid).unwrap().clone()
                };
                self.save_turn(&t, true);
            }
            "response.done" => self.on_response_done(&v["response"]),
            "error" => {
                let msg = v["error"]["message"].as_str().unwrap_or("onbekende fout");
                let code = v["error"]["code"].as_str().unwrap_or("");
                diag::log(format!("realtime error {code}: {msg}"));
                if code.contains("cancel") || code.contains("buffer_too_small") {
                    return;
                }
                self.st.lock().unwrap().m.errors += 1;
                if code == "insufficient_quota" || code == "invalid_api_key" {
                    notice(&self.app, "error", &format!("OpenAI: {msg}"));
                    self.finish("FAILED", "provider");
                } else {
                    notice(&self.app, "warn", &format!("OpenAI meldde een fout: {msg}"));
                    // A failed response.create leaves no in-flight response.
                    let mut st = self.st.lock().unwrap();
                    if st.in_flight.as_deref() == Some("") {
                        st.in_flight = None;
                        st.requested.pop_back();
                    }
                }
            }
            _ => {}
        }
    }

    fn shows_text(&self, v: &Value) -> bool {
        let id = v["response_id"].as_str().unwrap_or("");
        self.st.lock().unwrap().reply_kind.get(id) == Some(&Reply::Ask)
    }

    /// Turn id for an assistant text item, created on first sight.
    fn asst_turn(&self, item: &str) -> String {
        if let Some(t) = self.st.lock().unwrap().asst_items.get(item) {
            return t.clone();
        }
        let t = self.new_turn("assistant", "", false);
        let id = t.id.clone();
        let mut st = self.st.lock().unwrap();
        st.asst_items.insert(item.into(), id.clone());
        st.turns.insert(id.clone(), t);
        id
    }

    fn on_response_done(self: &Arc<Self>, r: &Value) {
        let id = r["id"].as_str().unwrap_or("").to_string();
        let status = r["status"].as_str().unwrap_or("");
        {
            let mut st = self.st.lock().unwrap();
            st.in_flight = None;
            st.m.responses += 1;
            if status == "cancelled" {
                st.m.cancelled_responses += 1;
            }
        }
        if r["usage"].is_object() {
            let u = ledger::parse_response_usage(&r["usage"]);
            self.usage(&format!("resp:{id}"), &self.model, Some(&id), "measured", &u, Some(&format!("est:{id}")));
        } else {
            self.usage(&format!("missing:{id}"), &self.model, Some(&id), "missing", &Usage::default(), None);
        }
        let kind = self.st.lock().unwrap().reply_kind.remove(&id).unwrap_or(Reply::Silent);
        let output = r["output"].as_array().map(Vec::as_slice).unwrap_or_default();
        let calls: Vec<&Value> = output.iter().filter(|o| o["type"] == "function_call").collect();
        // A shown question is followed by fresh inspiration instead of another question.
        let asked = kind == Reply::Ask && status == "completed" && output.iter().any(|o| o["type"] == "message");
        let mut follow = None;
        if !calls.is_empty() && status == "completed" {
            follow = self.run_tools(&calls, kind);
        }
        if asked {
            self.st.lock().unwrap().tool_followups = 0;
            follow = Some(Reply::Inspire);
        }
        let (queued, inspire) = {
            let mut st = self.st.lock().unwrap();
            let q = st.queued.take();
            // A new question brings its own inspiration.
            if q == Some(Reply::Ask) || asked {
                st.queued_inspire = false;
            }
            let i = std::mem::take(&mut st.queued_inspire);
            (q, i)
        };
        if let Some(q) = queued {
            self.request(q);
        }
        if inspire {
            self.request(Reply::Inspire);
        }
        if let Some(f) = follow {
            self.request(f);
        }
    }

    /// Execute tool calls; returns the follow-up that lets the model continue in the same mode
    /// (an Ask that only updated the canvas still owes its question).
    fn run_tools(self: &Arc<Self>, calls: &[&Value], kind: Reply) -> Option<Reply> {
        let mut answered = false;
        let mut guided = false;
        for c in calls {
            let name = c["name"].as_str().unwrap_or("");
            let args = c["arguments"].as_str().unwrap_or("{}");
            let out = if name == canvas::QUESTION_ANSWERED {
                // Only a silent update can move on; an Ask is already writing the next question.
                answered |= kind == Reply::Silent;
                json!({"ok": true}).to_string()
            } else if name == canvas::SHOW_GUIDE {
                match canvas::parse_guide(&serde_json::from_str(args).unwrap_or(Value::Null)) {
                    Ok(g) => {
                        guided = true;
                        self.st.lock().unwrap().guide = Some(g);
                        self.emit_snapshot();
                        json!({"ok": true}).to_string()
                    }
                    Err(e) => json!({"ok": false, "error": e}).to_string(),
                }
            } else {
                self.run_tool(name, c["call_id"].as_str().unwrap_or(""), args)
            };
            self.send(Conn::Rt, json!({"type": "conversation.item.create", "item": {
                "type": "function_call_output", "call_id": c["call_id"], "output": out}}));
        }
        if answered {
            self.next_question();
            return None;
        }
        if kind == Reply::Inspire && guided {
            return None;
        }
        let mut st = self.st.lock().unwrap();
        st.tool_followups += 1;
        (st.tool_followups <= MAX_TOOL_FOLLOWUPS).then_some(kind)
    }

    /// Validate and apply one tool call natively. Returns the JSON string for the model.
    fn run_tool(self: &Arc<Self>, name: &str, call_id: &str, args: &str) -> String {
        if call_id.is_empty() {
            return json!({"ok": false, "error": "call_id ontbreekt"}).to_string();
        }
        if let Some((_, prev)) = self.db.lock().unwrap().tool_result(call_id) {
            return prev; // idempotent replay
        }
        if !self.allowed() {
            return json!({"ok": false, "error": "Sessietijd verstreken"}).to_string();
        }
        self.st.lock().unwrap().m.tool_calls += 1;
        let a: Value = serde_json::from_str(args).unwrap_or(Value::Null);
        let validated = {
            let st = self.st.lock().unwrap();
            let ctx = ToolCtx { last_user_turn: st.last_user_turn.clone(), now: now_ms() };
            canvas::validate(name, &a, &st.canvas, &ctx).map(|m| {
                let r = canvas::tool_result(&m, &st.canvas).to_string();
                (m, r)
            })
        };
        match validated {
            Ok((m, result)) => {
                let res = self.db.lock().unwrap().apply(&self.sid, &m, Some((call_id, name, true, &result)));
                if let Err(e) = res {
                    self.storage_failure(&e);
                    return json!({"ok": false, "error": "opslag mislukt"}).to_string();
                }
                {
                    let mut st = self.st.lock().unwrap();
                    st.canvas.apply(&m);
                    // The clicked tile has been rewritten: back to the normal order.
                    if let Mutation::Item(it) = &m {
                        if st.refill.as_ref().is_some_and(|r| r.step == it.step && r.field == it.field) {
                            st.refill = None;
                        }
                    }
                }
                if !matches!(m, Mutation::Read) {
                    self.emit_canvas();
                    self.emit_snapshot();
                }
                result
            }
            Err(err) => {
                self.st.lock().unwrap().m.tool_rejections += 1;
                diag::log(format!("tool {name} rejected: {err}"));
                let result = json!({"ok": false, "error": err}).to_string();
                let _ = self.db.lock().unwrap().record_tool_rejection(&self.sid, call_id, name, &result);
                result
            }
        }
    }

    /// Bounded context window: drop the oldest items and restate confirmed canvas context.
    fn trim_context(&self) {
        let drop_ids: Vec<String> = {
            let mut st = self.st.lock().unwrap();
            if st.items.len() <= MAX_CONTEXT_ITEMS {
                return;
            }
            st.items.drain(..TRIM_ITEMS).collect()
        };
        for id in drop_ids {
            self.send(Conn::Rt, json!({"type": "conversation.item.delete", "item_id": id}));
        }
        let summary = self.st.lock().unwrap().canvas.summary();
        self.system_item(&format!("Oudere gespreksdelen zijn ingekort. Bevestigde canvascontext (leidend):\n{summary}"));
    }

    // ---------- transcription events ----------

    fn tr_turn(&self, v: &Value) -> Option<Turn> {
        let item = v["item_id"].as_str()?;
        let st = self.st.lock().unwrap();
        let tid = st.tr_items.get(item)?;
        st.turns.get(tid).cloned()
    }

    fn on_tr(self: &Arc<Self>, v: Value) {
        match v["type"].as_str().unwrap_or("") {
            "input_audio_buffer.committed" => {
                let item = v["item_id"].as_str().unwrap_or("").to_string();
                let (tid, secs) = {
                    let mut st = self.st.lock().unwrap();
                    let Some(tid) = st.pending_tr.pop_front() else { return };
                    st.tr_items.insert(item.clone(), tid.clone());
                    let secs = std::mem::take(&mut st.unbilled_secs);
                    if let Some(t) = st.turns.get_mut(&tid) {
                        t.provider_item_id = Some(item.clone());
                    }
                    (tid, secs)
                };
                let est = Usage { billed_seconds: Decimal::from_f64_retain(secs), ..Default::default() };
                self.usage(&format!("trest:{item}"), models::TRANSCRIBE, None, "estimated", &est, None);
                let t = self.st.lock().unwrap().turns.get(&tid).cloned();
                if let Some(t) = t {
                    self.save_turn(&t, true);
                }
            }
            "conversation.item.input_audio_transcription.delta" => {
                let Some(mut t) = self.tr_turn(&v) else { return };
                t.text.push_str(v["delta"].as_str().unwrap_or(""));
                {
                    let mut st = self.st.lock().unwrap();
                    if let Some(tt) = st.turn_timing.get_mut(&t.id) {
                        if !tt.2 {
                            tt.2 = true;
                            let ms = tt.0.elapsed().as_millis() as i64;
                            st.m.lat_first_delta_ms.push(ms);
                        }
                    }
                }
                self.save_turn(&t, false);
            }
            "conversation.item.input_audio_transcription.completed" => {
                let Some(mut t) = self.tr_turn(&v) else { return };
                t.text = v["transcript"].as_str().unwrap_or(&t.text).to_string();
                t.is_final = true;
                {
                    let mut st = self.st.lock().unwrap();
                    if let Some((_, Some(end), _)) = st.turn_timing.get(&t.id).copied() {
                        st.m.lat_final_transcript_ms.push(end.elapsed().as_millis() as i64);
                    }
                }
                self.save_turn(&t, true);
                let item = v["item_id"].as_str().unwrap_or("");
                if let Some(u) = ledger::parse_transcription_usage(&v["usage"]) {
                    self.usage(&format!("tr:{item}"), models::TRANSCRIBE, None, "measured", &u, Some(&format!("trest:{item}")));
                }
            }
            "conversation.item.input_audio_transcription.failed" => {
                let Some(mut t) = self.tr_turn(&v) else { return };
                t.failed = true;
                t.is_final = true;
                self.st.lock().unwrap().m.transcript_failures += 1;
                self.save_turn(&t, true);
                notice(&self.app, "warn", "Transcript tijdelijk onvolledig. Je kunt corrigeren, typen of stoppen.");
            }
            "error" => {
                let msg = v["error"]["message"].as_str().unwrap_or("");
                let code = v["error"]["code"].as_str().unwrap_or("");
                diag::log(format!("transcription error {code}: {msg}"));
                if !code.contains("buffer_too_small") {
                    self.st.lock().unwrap().m.errors += 1;
                    notice(&self.app, "warn", "Transcript tijdelijk onvolledig.");
                }
            }
            _ => {}
        }
    }

    // ---------- user controls ----------

    pub fn set_mute(&self, m: bool) {
        self.muted.store(m, SeqCst);
        self.capture.store(!m && !self.paused.load(SeqCst), SeqCst);
        self.emit_snapshot();
    }

    pub fn pause(&self) {
        if self.paused.swap(true, SeqCst) {
            return;
        }
        self.capture.store(false, SeqCst);
        self.cancel_response();
        self.st.lock().unwrap().pause_started = Some(now_ms());
        self.set_status("PAUSED");
    }

    pub async fn resume(self: &Arc<Self>) -> Result<(), String> {
        // Deadline is checked before anything resumes.
        if !self.allowed() {
            self.finish("COMPLETED", "deadline");
            return Err("De sessietijd is verstreken".into());
        }
        let reopen = self.conns.lock().unwrap().rt.is_none();
        self.paused.store(false, SeqCst);
        {
            let mut st = self.st.lock().unwrap();
            if let Some(s) = st.pause_started.take() {
                st.m.paused_ms += now_ms() - s;
            }
        }
        if reopen {
            if let Err(e) = self.open_streams(true).await {
                return Err(self.fail_connect(e));
            }
        }
        self.capture.store(!self.muted.load(SeqCst), SeqCst);
        if let Some(a) = self.audio.lock().unwrap().as_ref() {
            a.reset_vad.store(true, SeqCst);
        }
        self.set_status("ACTIVE");
        Ok(())
    }

    pub fn send_text(self: &Arc<Self>, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > 2000 {
            return Err("Tekst is leeg of te lang".into());
        }
        if !self.allowed() {
            return Err("Sessie is niet actief".into());
        }
        if self.paused.load(SeqCst) {
            return Err("Hervat de sessie eerst".into());
        }
        let t = self.new_turn("user", text, true);
        if !self.send(Conn::Rt, json!({"type": "conversation.item.create", "item": {
            "type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]}}))
        {
            return Err("Geen verbinding".into());
        }
        {
            let mut st = self.st.lock().unwrap();
            st.last_user_turn = Some(t.id.clone());
            st.m.user_turns += 1;
            st.m.text_turns += 1;
            st.tool_followups = 0;
            st.awaiting_reply_since = Some(Instant::now());
        }
        self.save_turn(&t, true);
        self.request(Reply::Silent);
        Ok(())
    }

    /// Manual canvas edit or step confirmation during a live session: inform the model, which
    /// must treat it as leading. Does not trigger a response.
    pub fn after_manual_change(&self, canvas: canvas::Canvas, what: &str) {
        self.st.lock().unwrap().canvas = canvas;
        self.emit_canvas();
        self.system_item(&format!("De gebruiker heeft het canvas handmatig aangepast: {what}. Dit is leidend; draai het niet terug."));
    }

    pub fn last_user_turn(&self) -> Option<String> {
        self.st.lock().unwrap().last_user_turn.clone()
    }

    /// Close everything. Idempotent. Never blocked by a missing model summary.
    pub fn finish(&self, status: &str, reason: &str) {
        if self.closing.swap(true, SeqCst) {
            return;
        }
        self.capture.store(false, SeqCst);
        self.close_streams();
        {
            let d = self.db.lock().unwrap();
            let _ = d.set_status(&self.sid, "FINALIZING", None);
        }
        self.st.lock().unwrap().status = "FINALIZING".into();
        emit(&self.app, "live", self.snapshot());
        let (tail, metrics) = {
            let mut st = self.st.lock().unwrap();
            if let Some(s) = st.pause_started.take() {
                st.m.paused_ms += now_ms() - s;
            }
            st.m.elapsed_ms = self.clock.lock().unwrap().elapsed_ms(now_ms()).min(clock::LIMIT_MS);
            // Transcription audio sent after the last commit (e.g. cut off by the deadline).
            (std::mem::take(&mut st.unbilled_secs), st.m.clone())
        };
        if tail > 0.0 {
            // Usage writes are still allowed here: they are local ledger rows, not AI traffic.
            let est = Usage { billed_seconds: Decimal::from_f64_retain(tail), ..Default::default() };
            self.usage(&format!("trtail:{}:{}", self.sid, now_ms()), models::TRANSCRIBE, None, "estimated", &est, None);
        }
        let mut v = serde_json::to_value(&metrics).unwrap();
        for (k, mut arr) in [
            ("FirstDelta", metrics.lat_first_delta_ms.clone()),
            ("FinalTranscript", metrics.lat_final_transcript_ms.clone()),
            ("FirstReply", metrics.lat_first_reply_ms.clone()),
        ] {
            v[format!("p50{k}Ms")] = json!(pct(&mut arr, 0.5));
            v[format!("p95{k}Ms")] = json!(pct(&mut arr, 0.95));
        }
        v["promptVersion"] = json!(prompt::PROMPT_VERSION);
        let d = self.db.lock().unwrap();
        let _ = d.set_metrics(&self.sid, &v.to_string());
        let _ = d.set_status(&self.sid, status, Some(reason));
        drop(d);
        self.st.lock().unwrap().status = status.into();
        diag::log(format!("session {} ended: {status} ({reason})", self.sid));
        emit(&self.app, "live", self.snapshot());
        emit(&self.app, "ended", json!({"sessionId": self.sid, "status": status, "reason": reason}));
    }
}

/// Model access check for setup (no paid generation): GET /v1/models/{id}.
pub async fn test_models(key: &str, ids: &[&str]) -> Vec<(String, Result<(), String>)> {
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(15)).build() {
        Ok(c) => c,
        Err(e) => return ids.iter().map(|i| (i.to_string(), Err(e.to_string()))).collect(),
    };
    let mut out = Vec::new();
    for id in ids {
        let r = client.get(format!("https://{HOST}/v1/models/{id}")).bearer_auth(key).send().await;
        let res = match r {
            Err(e) if e.is_timeout() || e.is_connect() => Err("Netwerkfout: geen verbinding met OpenAI".to_string()),
            Err(e) => Err(format!("Netwerkfout: {e}")),
            Ok(resp) => match resp.status().as_u16() {
                200 => Ok(()),
                401 => Err("Key ongeldig (401)".into()),
                403 | 404 => Err("Geen toegang tot dit model".into()),
                429 => Err("Quotum of rate limit bereikt (429)".into()),
                s => Err(format!("Onverwachte status {s}")),
            },
        };
        out.push((id.to_string(), res));
    }
    out
}
