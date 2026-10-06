//! AC-FAC evaluation: runs the 20 fixed Dutch scenarios against the Realtime API in text mode,
//! with the production instructions, tool schemas and native tool validator.
//!
//!   OPENAI_API_KEY=sk-... cargo run --example fac_eval -- ../../../docs/acceptance/fac-scenarios.json [S01,S05]
//!
//! Costs real money (text tokens only; roughly a few cents per scenario). FAC_MODEL overrides the model.

use canvas_facilitator_lib::{canvas, prompt};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string().into())).await.expect("send");
}

async fn until_done(ws: &mut Ws) -> Value {
    while let Some(Ok(m)) = ws.next().await {
        if let Message::Text(t) = m {
            let v: Value = serde_json::from_str(&t).unwrap_or_default();
            match v["type"].as_str() {
                Some("response.done") => return v["response"].clone(),
                Some("error") => eprintln!("  ! {}", v["error"]["message"]),
                _ => {}
            }
        }
    }
    panic!("connection closed");
}

fn text_of(resp: &Value) -> String {
    resp["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| o["type"] == "message")
        .flat_map(|o| o["content"].as_array().cloned().unwrap_or_default())
        .filter_map(|c| c["text"].as_str().or(c["transcript"].as_str()).map(String::from))
        .collect::<Vec<_>>()
        .join(" ")
}

struct Run {
    texts: Vec<(usize, String)>,
    canvas: canvas::Canvas,
    rejected: Vec<String>,
    tokens: i64,
}

/// One assistant reply incl. tool round-trips. Returns texts produced.
async fn respond(ws: &mut Ws, run: &mut Run, line_idx: usize) {
    for _ in 0..5 {
        send(ws, json!({"type": "response.create"})).await;
        let r = until_done(ws).await;
        run.tokens += r["usage"]["total_tokens"].as_i64().unwrap_or(0);
        let t = text_of(&r);
        if !t.trim().is_empty() {
            run.texts.push((line_idx, t));
        }
        let calls: Vec<Value> = r["output"].as_array().into_iter().flatten().filter(|o| o["type"] == "function_call").cloned().collect();
        if calls.is_empty() {
            return;
        }
        for c in calls {
            let name = c["name"].as_str().unwrap_or("");
            let args: Value = serde_json::from_str(c["arguments"].as_str().unwrap_or("{}")).unwrap_or_default();
            let ctx = canvas::ToolCtx { last_user_turn: Some(format!("u{line_idx}")), now: line_idx as i64 };
            // Questions arrive as a tool call; inspiration is display only.
            let out = if name == canvas::ASK || name == canvas::INSPIRE {
                if let Some(q) = args["question"].as_str() {
                    run.texts.push((line_idx, q.to_string()));
                }
                json!({"ok": true})
            } else { match canvas::validate(name, &args, &run.canvas, &ctx) {
                Ok(m) => {
                    let res = canvas::tool_result(&m, &run.canvas);
                    run.canvas.apply(&m);
                    res
                }
                Err(e) => {
                    run.rejected.push(format!("{name}: {e}"));
                    json!({"ok": false, "error": e})
                }
            } };
            send(ws, json!({"type": "conversation.item.create", "item": {"type": "function_call_output", "call_id": c["call_id"], "output": out.to_string()}})).await;
        }
    }
}

fn value_of<'a>(c: &'a canvas::Canvas, key: &str) -> Option<&'a canvas::CanvasItem> {
    let (s, f) = key.split_once('.')?;
    c.item(s, f)
}

#[tokio::main]
async fn main() {
    let key = std::env::var("OPENAI_API_KEY").expect("set OPENAI_API_KEY");
    let model = std::env::var("FAC_MODEL").unwrap_or_else(|_| "gpt-realtime-2.1".into());
    let path = std::env::args().nth(1).unwrap_or_else(|| "../../../docs/acceptance/fac-scenarios.json".into());
    let only: Option<Vec<String>> = std::env::args().nth(2).map(|s| s.split(',').map(String::from).collect());
    let scenarios: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&path).expect("scenarios file")).expect("json");

    let (mut q_turns, mut q_ok, mut passed, mut total) = (0, 0, 0, 0);
    let mut report = Vec::new();
    for sc in scenarios.iter().filter(|s| only.as_ref().map(|o| o.iter().any(|x| s["id"] == x.as_str())).unwrap_or(true)) {
        total += 1;
        let id = sc["id"].as_str().unwrap();
        println!("== {id} {}", sc["title"].as_str().unwrap());
        let mut req = format!("wss://api.openai.com/v1/realtime?model={model}").into_client_request().unwrap();
        req.headers_mut().insert("Authorization", HeaderValue::from_str(&format!("Bearer {key}")).unwrap());
        let (mut ws, _) = tokio_tungstenite::connect_async(req).await.expect("connect");
        send(&mut ws, json!({"type": "session.update", "session": {
            "type": "realtime", "output_modalities": ["text"],
            "instructions": prompt::instructions(sc["style"].as_str().unwrap_or("neutraal"), sc["title"].as_str().unwrap()),
            "tools": canvas::tool_schemas(), "tool_choice": "auto", "max_output_tokens": 800}})).await;
        let mut run = Run { texts: vec![], canvas: Default::default(), rejected: vec![], tokens: 0 };
        respond(&mut ws, &mut run, 0).await;
        let lines: Vec<&str> = sc["lines"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        for (i, l) in lines.iter().enumerate() {
            println!("  jij: {l}");
            send(&mut ws, json!({"type": "conversation.item.create", "item": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": l}]}})).await;
            respond(&mut ws, &mut run, i + 1).await;
            if let Some((_, t)) = run.texts.last() {
                println!("  fac: {t}");
            }
        }
        let _ = ws.close(None).await;

        // --- checks ---
        let mut fails = Vec::new();
        let last_line = lines.len();
        for (li, t) in &run.texts {
            let q = t.matches('?').count();
            if q > 0 && *li < last_line {
                q_turns += 1;
                if q == 1 {
                    q_ok += 1;
                }
            }
        }
        let all_text = run.texts.iter().map(|x| x.1.to_lowercase()).collect::<Vec<_>>().join(" ");
        if run.canvas.items.iter().any(|i| i.status == "VALIDATED" && i.evidence.trim().is_empty()) {
            fails.push("VALIDATED zonder evidence".to_string());
        }
        let final_text = run.texts.iter().filter(|x| x.0 == last_line).map(|x| x.1.to_lowercase()).collect::<String>();
        let has_action = run.canvas.decisions.iter().any(|d| d.kind == "action") || final_text.contains("actie") || final_text.contains("nog te bepalen");
        if !has_action {
            fails.push("geen (expliciet open) eerste actie".into());
        }
        let e = &sc["expect"];
        if let Some(words) = e["mentionAny"].as_array() {
            if !words.iter().filter_map(Value::as_str).any(|w| all_text.contains(&w.to_lowercase())) {
                fails.push(format!("noemt geen van {words:?}"));
            }
        }
        if let Some(words) = e["mentionNone"].as_array() {
            for w in words.iter().filter_map(Value::as_str) {
                if all_text.contains(&w.to_lowercase()) {
                    fails.push(format!("noemt verboden '{w}'"));
                }
            }
        }
        if let Some(m) = e["anyStatus"].as_object() {
            for (k, allowed) in m {
                let st = value_of(&run.canvas, k).map(|i| Value::from(i.status.clone())).unwrap_or(Value::Null);
                if !allowed.as_array().unwrap().contains(&st) {
                    fails.push(format!("{k} status {st}, verwacht {allowed}"));
                }
            }
        }
        if let Some(m) = e["anyValue"].as_object() {
            for (k, subs) in m {
                let v = value_of(&run.canvas, k).map(|i| i.value.to_lowercase()).unwrap_or_default();
                if !subs.as_array().unwrap().iter().filter_map(Value::as_str).any(|s| v.contains(&s.to_lowercase())) {
                    fails.push(format!("{k} = '{v}', verwacht een van {subs}"));
                }
            }
        }
        if let Some(steps) = e["notCompleted"].as_array() {
            for s in steps.iter().filter_map(Value::as_str) {
                if run.canvas.is_complete(s) {
                    fails.push(format!("{s} kunstmatig voltooid"));
                }
            }
        }
        if let Some(m) = e["maxWordsAfter"].as_object() {
            let after = m["line"].as_u64().unwrap() as usize;
            let max = m["words"].as_u64().unwrap() as usize;
            for (li, t) in run.texts.iter().filter(|x| x.0 > after && x.0 < last_line) {
                if t.split_whitespace().count() > max {
                    fails.push(format!("beurt {li} heeft {} woorden (> {max})", t.split_whitespace().count()));
                }
            }
        }
        let ok = fails.is_empty();
        if ok {
            passed += 1;
        }
        println!("  -> {} {}", if ok { "PASS" } else { "FAIL" }, fails.join("; "));
        report.push(json!({"id": id, "pass": ok, "fails": fails, "rejectedTools": run.rejected, "tokens": run.tokens,
            "canvas": run.canvas, "texts": run.texts}));
    }
    let pct = if q_turns == 0 { 100.0 } else { q_ok as f64 * 100.0 / q_turns as f64 };
    println!("\nScenarios: {passed}/{total} PASS · één primaire vraag: {q_ok}/{q_turns} ({pct:.0}%, eis ≥90%) · model {model} · prompt {}", prompt::PROMPT_VERSION);
    let out = format!("fac-report-{}.json", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
    std::fs::write(&out, serde_json::to_string_pretty(&json!({"model": model, "prompt": prompt::PROMPT_VERSION, "passed": passed, "total": total,
        "singleQuestionPct": pct, "scenarios": report})).unwrap()).unwrap();
    println!("Rapport: {out}");
    if passed < total || pct < 90.0 {
        std::process::exit(1);
    }
}
