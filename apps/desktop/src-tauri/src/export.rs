//! Markdown / JSON / CSV exports (PRD §9, §10). Exports are deliberately unencrypted.

use crate::canvas::{Canvas, STEPS};
use crate::db::{CostSummary, SessionRow, Turn, UsageRow};
use serde_json::{json, Value};

pub const EXPORT_SCHEMA: &str = "canvas-facilitator/session@1";

/// ISO-8601 UTC from epoch ms (no date crate needed).
pub fn iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn dur(ms: i64) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}

pub fn json(s: &SessionRow, c: &Canvas, turns: Option<&[Turn]>, cost: &CostSummary) -> Value {
    json!({
        "schema": EXPORT_SCHEMA,
        "session": {
            "id": s.id, "title": s.title, "status": s.status, "stopReason": s.stop_reason,
            "createdAt": iso(s.created_at), "startedAt": s.started_at.map(iso), "endedAt": s.ended_at.map(iso),
            "deadlineAt": s.deadline_at.map(iso), "style": s.style, "model": s.model,
            "transcribeModel": s.transcribe_model, "parentSessionId": s.parent_id,
            "priceVersion": s.price_version, "pricing": serde_json::from_str::<Value>(&s.pricing_json).unwrap_or(Value::Null),
            "budgetUsd": s.budget_usd.to_string(),
        },
        "canvas": c,
        "openFields": STEPS.iter().map(|st| json!({"step": st.key, "missing": c.missing_fields(st.key)})).collect::<Vec<_>>(),
        "transcript": turns,
        "cost": cost,
        "metrics": serde_json::from_str::<Value>(&s.metrics_json).unwrap_or(Value::Null),
    })
}

fn esc(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

pub fn markdown(s: &SessionRow, c: &Canvas, turns: Option<&[Turn]>, cost: &CostSummary) -> String {
    let mut o = String::new();
    o.push_str(&format!("# {}\n\n", s.title));
    let duration = match (s.started_at, s.ended_at) {
        (Some(a), Some(b)) => dur(b - a),
        _ => "-".into(),
    };
    o.push_str(&format!(
        "- Datum: {}\n- Duur: {duration}\n- Status: {}{}\n- Model: {} (transcriptie: {})\n- Gespreksstijl: {}\n- Prijsversie: {}\n- Geschatte kosten: ~${} {}\n\n",
        iso(s.started_at.unwrap_or(s.created_at)),
        s.status,
        s.stop_reason.as_ref().map(|r| format!(" ({r})")).unwrap_or_default(),
        s.model,
        s.transcribe_model,
        s.style,
        s.price_version,
        cost.total_usd.round_dp(4),
        if cost.incomplete { "(onvolledig: bevat schattingen of ontbrekend verbruik)" } else { "(berekende indicatie)" },
    ));
    o.push_str("> \"Bevestigd\" betekent door de gebruiker bevestigd, niet extern bewezen. Bron en onderbouwing staan apart vermeld.\n\n");
    for (i, st) in STEPS.iter().enumerate() {
        let status = c.step_status(st.key, None);
        o.push_str(&format!("## {}. {} — {}\n\n_{}_ · Domeinen: {}\n\n", i + 1, st.key, status, st.question, st.domains.join(", ")));
        o.push_str("| Veld | Inhoud | Status | Onderbouwing |\n| --- | --- | --- | --- |\n");
        for f in st.fields {
            match c.item(st.key, f.key) {
                Some(it) => o.push_str(&format!(
                    "| {} | {}{} | {} | {} |\n",
                    f.label,
                    esc(&it.value),
                    if it.manual { " ✎" } else { "" },
                    it.status,
                    esc(if it.evidence.is_empty() { "—" } else { &it.evidence })
                )),
                None => o.push_str(&format!("| {} | _open_ | UNKNOWN | — |\n", f.label)),
            }
        }
        if let Some(done) = c.completed.iter().find(|x| x.step == st.key) {
            o.push_str(&format!("\n**Synthese (bevestigd):** {}\n", done.synthesis));
        }
        for n in c.notes.iter().filter(|n| n.step == st.key) {
            o.push_str(&format!("\n- {}: {}", if n.kind == "assumption" { "Aanname" } else { "Challenge" }, n.text));
        }
        let missing = c.missing_fields(st.key);
        if !missing.is_empty() {
            o.push_str(&format!("\n\nOpen punten: {}", missing.join(", ")));
        }
        o.push_str("\n\n");
    }
    o.push_str("## Besluiten en acties\n\n");
    if c.decisions.is_empty() {
        o.push_str("- Eerste actie: nog te bepalen\n");
    }
    for d in &c.decisions {
        o.push_str(&format!(
            "- **{}**: {} (eigenaar: {}, termijn: {}){}\n",
            if d.kind == "action" { "Actie" } else { "Besluit" },
            d.content,
            d.owner,
            d.due,
            if d.rationale.is_empty() { String::new() } else { format!(" — {}", d.rationale) }
        ));
    }
    o.push_str(&format!(
        "\n## Kosten en verbruik\n\n| Categorie | Aantal |\n| --- | ---: |\n| Tekst-input (waarvan cached) | {} ({}) |\n| Audio-input (waarvan cached) | {} ({}) |\n| Tekst-output | {} |\n| Audio-output | {} |\n| Transcriptie (seconden) | {} |\n| Responses | {} |\n\n",
        cost.usage.text_in, cost.usage.text_cached, cost.usage.audio_in, cost.usage.audio_cached, cost.usage.text_out,
        cost.usage.audio_out, cost.usage.billed_seconds.map(|d| d.round_dp(1).to_string()).unwrap_or("0".into()), cost.responses
    ));
    if !cost.unresolved.is_empty() {
        o.push_str(&format!("Onopgeloste verbruikscategorieën: {}\n\n", cost.unresolved.join("; ")));
    }
    if let Some(ts) = turns {
        o.push_str("## Transcript\n\n");
        for t in ts.iter().filter(|t| !t.text.is_empty()) {
            let who = if t.speaker == "user" { "Jij" } else { "Facilitator" };
            let text = match (&t.spoken_text, t.interrupted) {
                (Some(sp), true) => format!("{sp} _[afgebroken]_"),
                _ => t.text.clone(),
            };
            let flag = if t.failed { " _[transcript onvolledig]_" } else if t.corrected_at.is_some() { " _[gecorrigeerd]_" } else { "" };
            o.push_str(&format!("**{who}:** {text}{flag}\n\n"));
        }
    }
    o.push_str("---\nGeëxporteerd uit Canvas Facilitator. Dit bestand is niet versleuteld.\n");
    o
}

fn csv(fields: &[String]) -> String {
    fields
        .iter()
        .map(|f| if f.contains([',', '"', '\n']) { format!("\"{}\"", f.replace('"', "\"\"")) } else { f.clone() })
        .collect::<Vec<_>>()
        .join(",")
}

pub fn costs_csv(rows: &[UsageRow]) -> String {
    let mut o = String::from("datum,sessie,scope,model,soort,response_id,tekst_in,tekst_cached,audio_in,audio_cached,tekst_out,audio_out,reasoning,transcriptie_sec,kosten_usd,kostenfout,prijsversie,vervangen\n");
    for r in rows {
        let u = &r.usage;
        o.push_str(&csv(&[
            iso(r.created_at),
            r.session_id.clone().unwrap_or_default(),
            r.scope.clone(),
            r.model.clone(),
            r.kind.clone(),
            r.response_id.clone().unwrap_or_default(),
            u.text_in.to_string(),
            u.text_cached.to_string(),
            u.audio_in.to_string(),
            u.audio_cached.to_string(),
            u.text_out.to_string(),
            u.audio_out.to_string(),
            u.reasoning.to_string(),
            u.billed_seconds.map(|d| d.to_string()).unwrap_or_default(),
            r.cost_usd.map(|d| d.to_string()).unwrap_or_default(),
            r.cost_error.clone().unwrap_or_default(),
            r.price_version.clone(),
            r.superseded.to_string(),
        ]));
        o.push('\n');
    }
    o
}

pub fn metrics_csv(sessions: &[(SessionRow, CostSummary)]) -> String {
    let keys = [
        "elapsedMs", "pausedMs", "userSpeechMs", "streamedAudioSecs", "userTurns", "assistantTurns",
        "textTurns", "responses", "cancelledResponses", "toolCalls", "toolRejections", "reconnects", "errors",
        "transcriptFailures", "p50FirstDeltaMs", "p95FirstDeltaMs", "p50FinalTranscriptMs", "p95FinalTranscriptMs",
        "p50FirstReplyMs", "p95FirstReplyMs", "promptVersion",
    ];
    let mut o = format!("sessie,titel,status,stopreden,model,prijsversie,kosten_usd,kosten_onvolledig,{}\n", keys.join(","));
    for (s, c) in sessions {
        let m: Value = serde_json::from_str(&s.metrics_json).unwrap_or(Value::Null);
        let mut f = vec![
            s.id.clone(),
            s.title.clone(),
            s.status.clone(),
            s.stop_reason.clone().unwrap_or_default(),
            s.model.clone(),
            s.price_version.clone(),
            c.total_usd.to_string(),
            c.incomplete.to_string(),
        ];
        f.extend(keys.iter().map(|k| match &m[*k] {
            Value::Null => String::new(),
            Value::String(x) => x.clone(),
            v => v.to_string(),
        }));
        o.push_str(&csv(&f));
        o.push('\n');
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_791_115_200_000), "2026-10-04T12:00:00Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn csv_quotes() {
        assert_eq!(csv(&["a,b".into(), "x\"y".into(), "z".into()]), "\"a,b\",\"x\"\"y\",z");
    }
}
