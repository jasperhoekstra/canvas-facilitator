mod audio;
#[doc(hidden)]
pub mod canvas;
mod clock;
mod db;
mod diag;
mod export;
mod ledger;
mod live;
#[doc(hidden)]
pub mod prompt;
mod secrets;

use db::{now_ms, CostSummary, Db, SessionRow, Settings};
use live::{models, Live};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

type R<T> = Result<T, String>;

#[derive(Default)]
struct AppState {
    db: OnceLock<Arc<Mutex<Db>>>,
    storage_error: Mutex<Option<String>>,
    live: Mutex<Option<Arc<Live>>>,
    mic: Mutex<Option<audio::AudioHandle>>,
}

impl AppState {
    fn db(&self) -> R<Arc<Mutex<Db>>> {
        self.db.get().cloned().ok_or_else(|| {
            self.storage_error.lock().unwrap().clone().unwrap_or_else(|| "Opslag is niet beschikbaar".into())
        })
    }
    fn live(&self) -> Option<Arc<Live>> {
        let mut l = self.live.lock().unwrap();
        if l.as_ref().map(|x| x.is_closed()).unwrap_or(false) {
            *l = None;
        }
        l.clone()
    }
    fn live_for(&self, sid: &str) -> R<Arc<Live>> {
        self.live().filter(|l| l.sid == sid).ok_or_else(|| "Sessie is niet actief".into())
    }
}

fn open_storage(app: &tauri::AppHandle, st: &AppState) -> R<()> {
    if st.db.get().is_some() {
        return Ok(());
    }
    let res = (|| {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let key = secrets::db_key()?;
        Db::open(&dir.join("canvas.db"), &key)
    })();
    match res {
        Ok(db) => {
            let _ = st.db.set(Arc::new(Mutex::new(db)));
            *st.storage_error.lock().unwrap() = None;
            recover_and_purge(st);
            Ok(())
        }
        Err(e) => {
            diag::log(format!("storage open failed: {e}"));
            *st.storage_error.lock().unwrap() = Some(e.clone());
            Err(e)
        }
    }
}

/// Crash recovery and retention at startup. Sessions that were live become PAUSED only when the
/// persisted clock data allow it; otherwise they are closed and never get a fresh timer.
fn recover_and_purge(st: &AppState) {
    let Ok(dbh) = st.db() else { return };
    let mut d = dbh.lock().unwrap();
    let now = now_ms();
    for s in d.sessions_in_states(&["CONNECTING", "ACTIVE", "PAUSED", "FINALIZING"]).unwrap_or_default() {
        let ok = match (s.started_at, s.status.as_str()) {
            (Some(start), "CONNECTING" | "ACTIVE" | "PAUSED") => clock::Clock::recover(start, s.last_heartbeat.unwrap_or(0), now).is_some(),
            _ => false,
        };
        let _ = if ok { d.set_status(&s.id, "PAUSED", Some("crashherstel")) } else { d.set_status(&s.id, "INTERRUPTED", Some("crash")) };
    }
    if let Some(days) = d.settings().retention_days {
        for id in d.expired_sessions(days).unwrap_or_default() {
            let _ = d.delete_session(&id, true);
        }
    }
}

// ---------- status & setup ----------

#[tauri::command]
fn app_status(app: tauri::AppHandle, st: State<AppState>) -> Value {
    let _ = open_storage(&app, &st);
    let key = secrets::get(secrets::API_KEY);
    let settings = st.db().ok().map(|d| d.lock().unwrap().settings());
    let recoverable: Vec<SessionRow> = st
        .db()
        .ok()
        .map(|d| d.lock().unwrap().sessions_in_states(&["PAUSED"]).unwrap_or_default())
        .unwrap_or_default()
        .into_iter()
        .filter(|s| st.live().map(|l| l.sid != s.id).unwrap_or(true))
        .collect();
    json!({
        "storageError": st.storage_error.lock().unwrap().clone(),
        "keyPresent": matches!(key, Ok(Some(_))),
        "keyMask": key.as_ref().ok().and_then(|k| k.as_ref().map(|k| secrets::mask(k))),
        "settings": settings,
        "recoverable": recoverable,
        "live": st.live().map(|l| l.snapshot()),
        "version": env!("CARGO_PKG_VERSION"),
    })
}

#[tauri::command]
fn retry_storage(app: tauri::AppHandle, st: State<AppState>) -> R<()> {
    open_storage(&app, &st)
}

#[tauri::command]
fn save_key(key: String) -> R<String> {
    let key = key.trim().to_string();
    if !key.starts_with("sk-") || key.len() < 20 || key.chars().any(|c| c.is_whitespace()) {
        return Err("Dit lijkt geen geldige OpenAI API-key (begint met sk-)".into());
    }
    secrets::set_verified(secrets::API_KEY, &key)?;
    Ok(secrets::mask(&key))
}

#[tauri::command]
fn delete_key(st: State<AppState>) -> R<()> {
    if let Some(l) = st.live() {
        l.finish("INTERRUPTED", "key verwijderd");
    }
    secrets::delete(secrets::API_KEY)
}

#[tauri::command]
async fn test_connection(profile: String) -> R<Value> {
    let key = secrets::get(secrets::API_KEY)?.ok_or("Geen key ingesteld")?;
    let res = live::test_models(&key, &[models::for_profile(&profile), models::TRANSCRIBE]).await;
    Ok(json!(res
        .into_iter()
        .map(|(m, r)| json!({"model": m, "ok": r.is_ok(), "error": r.err()}))
        .collect::<Vec<_>>()))
}

#[tauri::command]
fn audio_devices() -> Vec<audio::DeviceInfo> {
    audio::input_devices()
}

#[tauri::command]
fn mic_test_start(app: tauri::AppHandle, st: State<AppState>, input: Option<String>) -> R<()> {
    if st.live().is_some() {
        return Err("Niet beschikbaar tijdens een sessie".into());
    }
    let sink: audio::Sink = Arc::new(move |e| match e {
        audio::AudioEvent::Level(l) => {
            let _ = app.emit("level", l);
        }
        audio::AudioEvent::DeviceError(m) | audio::AudioEvent::DeviceFallback(m) => live::notice(&app, "warn", &m),
        _ => {}
    });
    let h = audio::start(input, Arc::new(std::sync::atomic::AtomicBool::new(true)), sink)?;
    *st.mic.lock().unwrap() = Some(h);
    Ok(())
}

#[tauri::command]
fn mic_test_stop(st: State<AppState>) {
    st.mic.lock().unwrap().take();
}


#[tauri::command]
fn get_settings(st: State<AppState>) -> R<Settings> {
    Ok(st.db()?.lock().unwrap().settings())
}

#[tauri::command]
fn save_settings(st: State<AppState>, settings: Settings) -> R<()> {
    if !models::PROFILES.contains(&settings.profile.as_str()) || !prompt::STYLES.contains(&settings.style.as_str()) {
        return Err("Ongeldige instelling".into());
    }
    if settings.session_budget_usd <= Decimal::ZERO || settings.monthly_budget_usd <= Decimal::ZERO {
        return Err("Budget moet groter dan 0 zijn".into());
    }
    if !matches!(settings.retention_days, None | Some(30) | Some(90)) {
        return Err("Bewaartermijn moet onbeperkt, 30 of 90 dagen zijn".into());
    }
    if let Some(p) = &settings.pricing_override {
        ledger::Pricing::parse(p)?;
    }
    st.db()?.lock().unwrap().save_settings(&settings)
}

#[tauri::command]
fn get_pricing(st: State<AppState>) -> R<Value> {
    let d = st.db()?;
    let d = d.lock().unwrap();
    let p = d.pricing();
    let age = d.days_since(&p.verified_at);
    Ok(json!({"pricing": p, "ageDays": age, "stale": age.map(|a| a > 30).unwrap_or(true),
        "overridden": d.settings().pricing_override.is_some(), "bundled": ledger::Pricing::bundled()}))
}

/// Indicative range for the new-session screen, from the PRD §12 usage profiles.
#[tauri::command]
fn cost_estimate(st: State<AppState>, profile: String) -> R<Value> {
    let p = st.db()?.lock().unwrap().pricing();
    let m = models::for_profile(&profile);
    let t = ledger::Usage { billed_seconds: Some(Decimal::from(900)), ..Default::default() };
    // Text-only facilitator: no audio output; questions + tool calls as text output.
    let base = ledger::Usage { audio_in: 53_600, audio_cached: 50_000, text_in: 60_000, text_cached: 50_000, text_out: 4_000, ..Default::default() };
    let heavy = ledger::Usage { audio_in: 75_000, ..base.clone() };
    let tr = ledger::cost(&p, models::TRANSCRIBE, &t)?;
    Ok(json!({"model": m, "low": ledger::cost(&p, m, &base)? + tr, "high": ledger::cost(&p, m, &heavy)? + tr, "priceVersion": p.version}))
}

// ---------- sessions ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewSession {
    title: String,
    style: String,
    profile: String,
    budget_usd: Decimal,
    parent_id: Option<String>,
}

#[tauri::command]
fn create_session(st: State<AppState>, req: NewSession) -> R<SessionRow> {
    let title = valid_title(&req.title)?;
    if !prompt::STYLES.contains(&req.style.as_str()) || !models::PROFILES.contains(&req.profile.as_str()) || req.budget_usd <= Decimal::ZERO {
        return Err("Ongeldige sessie-instellingen".into());
    }
    let dbh = st.db()?;
    let mut d = dbh.lock().unwrap();
    let pricing = d.pricing();
    let model = models::for_profile(&req.profile);
    if !pricing.models.contains_key(model) || !pricing.models.contains_key(models::TRANSCRIBE) {
        return Err(format!("Geen prijs bekend voor {model}; stel een tarief in bij Instellingen"));
    }
    let s = SessionRow {
        id: canvas::new_id(),
        title,
        schema_version: db::SCHEMA_VERSION,
        created_at: now_ms(),
        started_at: None,
        ended_at: None,
        deadline_at: None,
        last_heartbeat: None,
        status: "DRAFT".into(),
        stop_reason: None,
        style: req.style,
        profile: req.profile.clone(),
        model: model.into(),
        transcribe_model: models::TRANSCRIBE.into(),
        parent_id: req.parent_id.clone(),
        price_version: pricing.version.clone(),
        pricing_json: serde_json::to_string(&pricing).unwrap(),
        budget_usd: req.budget_usd,
        metrics_json: "{}".into(),
    };
    d.insert_session(&s)?;
    // Follow-up session: carry the confirmed canvas context with fresh ids.
    if let Some(pid) = &req.parent_id {
        use canvas::Mutation as M;
        let c = d.canvas(pid)?;
        let now = now_ms();
        let muts = c.items.into_iter().map(|i| M::Item(canvas::CanvasItem { id: canvas::new_id(), updated_at: now, ..i }))
            .chain(c.notes.into_iter().map(|n| M::Note(canvas::Note { id: canvas::new_id(), ..n })))
            .chain(c.decisions.into_iter().map(|x| M::Decision(canvas::Decision { id: canvas::new_id(), ..x })))
            .chain(c.completed.into_iter().map(M::Complete));
        for m in muts {
            d.apply(&s.id, &m, None)?;
        }
    }
    Ok(s)
}

#[tauri::command]
async fn start_session(app: tauri::AppHandle, st: State<'_, AppState>, id: String) -> R<live::Snapshot> {
    if st.live().is_some() {
        return Err("Er is al een actieve sessie".into());
    }
    st.mic.lock().unwrap().take();
    let dbh = st.db()?;
    let l = Live::start(app, dbh, &id).await?;
    *st.live.lock().unwrap() = Some(l.clone());
    Ok(l.snapshot())
}

#[tauri::command]
async fn resume_session(app: tauri::AppHandle, st: State<'_, AppState>, id: String) -> R<live::Snapshot> {
    if let Some(l) = st.live() {
        if l.sid != id {
            return Err("Er is al een andere actieve sessie".into());
        }
        l.resume().await?;
        return Ok(l.snapshot());
    }
    start_session(app, st, id).await
}

#[tauri::command]
fn pause_session(st: State<AppState>, id: String) -> R<()> {
    st.live_for(&id)?.pause();
    Ok(())
}

#[tauri::command]
fn stop_session(st: State<AppState>, id: String) -> R<()> {
    st.live_for(&id)?.finish("COMPLETED", "gebruiker");
    Ok(())
}

#[tauri::command]
fn set_mute(st: State<AppState>, id: String, muted: bool) -> R<()> {
    st.live_for(&id)?.set_mute(muted);
    Ok(())
}

#[tauri::command]
fn next_question(st: State<AppState>, id: String) -> R<()> {
    st.live_for(&id)?.next_question();
    Ok(())
}

#[tauri::command]
fn inspire(st: State<AppState>, id: String) -> R<()> {
    st.live_for(&id)?.inspire();
    Ok(())
}

#[tauri::command]
fn refill_field(st: State<AppState>, id: String, step: String, field: String) -> R<()> {
    st.live_for(&id)?.refill_field(&step, &field)
}

#[tauri::command]
fn send_text(st: State<AppState>, id: String, text: String) -> R<()> {
    st.live_for(&id)?.send_text(&text)
}

#[tauri::command]
fn switch_input(st: State<AppState>, id: String, device: Option<String>) -> R<()> {
    st.live_for(&id)?.switch_input(device);
    Ok(())
}

#[tauri::command]
fn live_snapshot(st: State<AppState>) -> Option<live::Snapshot> {
    st.live().map(|l| l.snapshot())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionListItem {
    #[serde(flatten)]
    session: SessionRow,
    cost: CostSummary,
}

#[tauri::command]
fn list_sessions(st: State<AppState>, query: String, status: String, from: Option<i64>, to: Option<i64>) -> R<Vec<SessionListItem>> {
    let dbh = st.db()?;
    let d = dbh.lock().unwrap();
    Ok(d.sessions(&query, &status, from, to)?
        .into_iter()
        .map(|s| SessionListItem { cost: d.session_cost(&s.id), session: s })
        .collect())
}

#[tauri::command]
fn get_session(st: State<AppState>, id: String) -> R<Value> {
    let dbh = st.db()?;
    let d = dbh.lock().unwrap();
    let (tool_calls, tool_rejections) = d.tool_counts(&id);
    let usage = d.usage_rows("session_id=?1", &[&id])?;
    Ok(json!({
        "session": d.session(&id)?, "view": d.canvas(&id)?.view(), "turns": d.turns(&id)?, "cost": Db::summarize(&usage),
        "usage": usage, "toolCalls": tool_calls, "toolRejections": tool_rejections,
    }))
}

#[tauri::command]
fn canvas_definition() -> Value {
    json!(canvas::STEPS
        .iter()
        .map(|s| json!({"key": s.key, "label": s.label, "question": s.question, "domains": s.domains,
            "fields": s.fields.iter().map(|f| json!({"key": f.key, "label": f.label, "domain": f.domain})).collect::<Vec<_>>()}))
        .collect::<Vec<_>>())
}

/// Apply a user change to a session canvas, tell a running facilitator about it, return the view.
fn mutate(st: &AppState, sid: &str, what: &str, f: impl FnOnce(&mut Db, &canvas::Canvas) -> R<()>) -> R<canvas::View> {
    let dbh = st.db()?;
    let c = {
        let mut d = dbh.lock().unwrap();
        let c = d.canvas(sid)?;
        f(&mut d, &c)?;
        d.canvas(sid)?
    };
    let view = c.view();
    if let Some(l) = st.live().filter(|l| l.sid == sid) {
        l.after_manual_change(c, what);
    }
    Ok(view)
}

#[tauri::command]
fn edit_item(st: State<AppState>, session_id: String, step: String, field: String, value: String, status: String) -> R<canvas::View> {
    mutate(&st, &session_id, &format!("{step}.{field} = \"{value}\" ({status})"), |d, c| {
        d.apply(&session_id, &canvas::Mutation::Item(c.manual_edit(&step, &field, &value, &status, now_ms())?), None)
    })
}

/// User confirms a step synthesis by clicking: this is the registered confirmation.
#[tauri::command]
fn confirm_step(st: State<AppState>, session_id: String, step: String, synthesis: String) -> R<canvas::View> {
    let turn = st.live().filter(|l| l.sid == session_id).and_then(|l| l.last_user_turn()).unwrap_or_else(|| "handmatig".into());
    mutate(&st, &session_id, &format!("stap {step} bevestigd als voldoende uitgewerkt"), |d, c| {
        d.apply(&session_id, &c.complete(&step, &synthesis, turn, now_ms())?, None)
    })
}

#[tauri::command]
fn add_action(st: State<AppState>, session_id: String, kind: String, content: String, owner: String, due: String) -> R<canvas::View> {
    mutate(&st, &session_id, &format!("{kind} toegevoegd: {content}"), |d, _| {
        d.apply(&session_id, &canvas::decision(&kind, &content, "handmatig toegevoegd", &owner, &due, "handmatig".into(), now_ms())?, None)
    })
}

#[tauri::command]
fn delete_note(st: State<AppState>, session_id: String, id: String) -> R<canvas::View> {
    mutate(&st, &session_id, "een aanname/challenge/besluit verwijderd", |d, _| d.delete_note(&session_id, &id))
}

fn valid_title(t: &str) -> R<String> {
    let t = t.trim();
    if t.is_empty() || t.chars().count() > 120 {
        return Err("Geef een titel van maximaal 120 tekens".into());
    }
    Ok(t.into())
}

#[tauri::command]
fn correct_turn(st: State<AppState>, id: String, text: String) -> R<()> {
    if text.trim().is_empty() || text.chars().count() > 4000 {
        return Err("Tekst is leeg of te lang".into());
    }
    st.db()?.lock().unwrap().correct_turn(&id, text.trim())
}

#[tauri::command]
fn rename_session(st: State<AppState>, id: String, title: String) -> R<()> {
    st.db()?.lock().unwrap().rename_session(&id, &valid_title(&title)?)
}

#[tauri::command]
fn delete_session(st: State<AppState>, id: String, keep_costs: bool) -> R<()> {
    if st.live().map(|l| l.sid == id).unwrap_or(false) {
        return Err("Stop de sessie eerst".into());
    }
    st.db()?.lock().unwrap().delete_session(&id, keep_costs)
}

#[tauri::command]
fn cost_overview(st: State<AppState>) -> R<Value> {
    let dbh = st.db()?;
    let d = dbh.lock().unwrap();
    let all = d.usage_rows("1=1", &[])?;
    let setup: Vec<_> = all.iter().filter(|r| r.scope == "setup").cloned().collect();
    let orphan = all.iter().filter(|r| r.scope == "session" && r.session_id.as_ref().map(|s| d.session(s).is_err()).unwrap_or(false)).count();
    let pricing = d.pricing();
    Ok(json!({
        "total": Db::summarize(&all), "month": d.month_cost(), "setup": Db::summarize(&setup),
        "deletedSessionRows": orphan, "settings": d.settings(), "priceVersion": pricing.version,
    }))
}

// ---------- exports (native save dialog; renderer never touches the filesystem) ----------

async fn save_dialog(app: &tauri::AppHandle, name: String, ext: &'static str, content: String) -> R<Option<String>> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = app.dialog().file().set_file_name(&name).add_filter(ext.to_uppercase(), &[ext]).blocking_save_file();
        match path {
            None => Ok(None),
            Some(p) => {
                let p = p.into_path().map_err(|e| e.to_string())?;
                std::fs::write(&p, content).map_err(|e| format!("Opslaan mislukt: {e}"))?;
                Ok(Some(p.display().to_string()))
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

fn safe_name(t: &str) -> String {
    let s: String = t.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).take(60).collect();
    if s.is_empty() {
        "sessie".into()
    } else {
        s
    }
}

#[tauri::command]
async fn export_session(app: tauri::AppHandle, st: State<'_, AppState>, id: String, format: String, include_transcript: bool) -> R<Option<String>> {
    let (content, name, ext) = {
        let dbh = st.db()?;
        let d = dbh.lock().unwrap();
        let s = d.session(&id)?;
        let c = d.canvas(&id)?;
        let turns = d.turns(&id)?;
        let cost = d.session_cost(&id);
        let t = include_transcript.then_some(turns.as_slice());
        let name = safe_name(&s.title);
        match format.as_str() {
            "json" => (serde_json::to_string_pretty(&export::json(&s, &c, t, &cost)).unwrap(), name, "json"),
            _ => (export::markdown(&s, &c, t, &cost), name, "md"),
        }
    };
    save_dialog(&app, format!("{name}.{ext}"), ext, content).await
}

#[tauri::command]
async fn export_costs(app: tauri::AppHandle, st: State<'_, AppState>, kind: String) -> R<Option<String>> {
    let content = {
        let dbh = st.db()?;
        let d = dbh.lock().unwrap();
        if kind == "metrics" {
            let rows: Vec<_> = d.sessions("", "", None, None)?.into_iter().map(|s| { let c = d.session_cost(&s.id); (s, c) }).collect();
            export::metrics_csv(&rows)
        } else {
            export::costs_csv(&d.usage_rows("1=1", &[])?)
        }
    };
    let name = if kind == "metrics" { "canvas-facilitator-metrics.csv" } else { "canvas-facilitator-kosten.csv" };
    save_dialog(&app, name.into(), "csv", content).await
}

#[tauri::command]
async fn export_diagnostics(app: tauri::AppHandle) -> R<Option<String>> {
    let content = diag::path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    // Re-redact on the way out, in case anything slipped into the file.
    save_dialog(&app, "canvas-facilitator-diagnostiek.log".into(), "log", secrets::redact(&content)).await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            if let Ok(dir) = app.path().app_log_dir() {
                diag::init(dir);
            }
            diag::log(format!("app start v{}", env!("CARGO_PKG_VERSION")));
            let st = app.state::<AppState>();
            let _ = open_storage(app.handle(), &st);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_status, retry_storage, save_key, delete_key, test_connection, audio_devices, mic_test_start, mic_test_stop,
            get_settings, save_settings, get_pricing, cost_estimate, create_session, start_session,
            resume_session, pause_session, stop_session, set_mute, send_text, next_question, inspire, refill_field, switch_input, live_snapshot, list_sessions,
            get_session, canvas_definition, edit_item, confirm_step, add_action, delete_note, correct_turn, rename_session,
            delete_session, cost_overview, export_session, export_costs, export_diagnostics
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, ev| {
            if let tauri::RunEvent::ExitRequested { .. } = ev {
                if let Some(l) = app.state::<AppState>().live() {
                    l.finish("INTERRUPTED", "app gesloten");
                }
            }
        });
}
