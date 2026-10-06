//! Canvas definition (PRD §5) and native validation of model tool calls (PRD §11).
//! The model only proposes; everything here decides what is accepted.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub struct FieldDef {
    pub key: &'static str,
    pub label: &'static str,
    pub domain: &'static str,
}

pub struct StepDef {
    pub key: &'static str,
    pub label: &'static str,
    pub question: &'static str,
    pub domains: &'static [&'static str],
    pub fields: &'static [FieldDef],
}

const fn f(key: &'static str, label: &'static str, domain: &'static str) -> FieldDef {
    FieldDef { key, label, domain }
}

pub const STEPS: [StepDef; 5] = [
    StepDef {
        key: "KIES",
        label: "Kies",
        question: "Welk probleem lossen we eerst op, voor wie en wie is business owner?",
        domains: &["Desirability", "Viability"],
        fields: &[
            f("doelgroep", "Doelgroep", "Desirability"),
            f("job_to_be_done", "Job-to-be-done", "Desirability"),
            f("pain_gain", "Pain / gain", "Desirability"),
            f("probleem", "Gekozen probleem", "Desirability"),
            f("ai_fit", "AI-fit", "Viability"),
            f("business_owner", "Business owner", "Viability"),
        ],
    },
    StepDef {
        key: "MEET",
        label: "Meet",
        question: "Wat moet beter worden, wanneer is het een succes, ook over een jaar?",
        domains: &["Viability", "Desirability"],
        fields: &[
            f("kpi", "KPI", "Viability"),
            f("baseline", "Baseline / nulmeting", "Viability"),
            f("target", "Target", "Viability"),
            f("termijn", "Termijn", "Viability"),
            f("meetwijze", "Meetwijze", "Desirability"),
            f("eigenaar", "Eigenaar meting", "Viability"),
        ],
    },
    StepDef {
        key: "BEGRENS",
        label: "Begrens",
        question: "Wie mag wat, met welke data en systemen en welke controles?",
        domains: &["Sustainability"],
        fields: &[
            f("databron", "Databron / beschikbaarheid", "Sustainability"),
            f("modeltaak", "Modeltaak", "Sustainability"),
            f("acties", "Toegestane / verboden acties", "Sustainability"),
            f("risico", "Belangrijkste risico", "Sustainability"),
            f("guardrail", "Guardrail", "Sustainability"),
            f("menselijke_controle", "Menselijke controle", "Sustainability"),
        ],
    },
    StepDef {
        key: "REALISEER",
        label: "Realiseer",
        question: "Hoe bouwen, testen, verbeteren en brengen we de oplossing naar productie?",
        domains: &["Feasibility", "Viability"],
        fields: &[
            f("concept", "Oplossingsconcept", "Feasibility"),
            f("interactieflow", "Interactieflow", "Feasibility"),
            f("build_buy_partner", "Build / buy / partner", "Feasibility"),
            f("experiment", "Kleinste experiment", "Feasibility"),
            f("testcriterium", "Testcriterium", "Feasibility"),
            f("kostendrijvers", "Kostendrijvers", "Viability"),
        ],
    },
    StepDef {
        key: "VERANKER",
        label: "Veranker",
        question: "Hoe landt dit in het werkproces en wie bezit de verandering?",
        domains: &["Desirability", "Feasibility", "Sustainability", "Viability"],
        fields: &[
            f("procesintegratie", "Procesintegratie", "Feasibility"),
            f("eigenaar", "Eigenaar verandering", "Viability"),
            f("adoptie", "Adoptie", "Desirability"),
            f("monitoring", "Monitoring waarde/kwaliteit/kosten", "Sustainability"),
            f("evaluatiemoment", "Evaluatiemoment", "Viability"),
            f("eerste_actie", "Eerste actie", "Feasibility"),
        ],
    },
];

pub const STATUSES: [&str; 7] = ["UNKNOWN", "PARTIAL", "ASSUMPTION", "VALIDATED", "CONTRADICTED", "DECIDED", "PARKED"];
pub const MAX_TEXT: usize = 500;
/// Signal tool (no canvas mutation): the question on screen is answered well enough to move on.
pub const QUESTION_ANSWERED: &str = "question_answered";

pub fn step_def(step: &str) -> Option<&'static StepDef> {
    STEPS.iter().find(|s| s.key == step)
}
pub fn field_def(step: &str, field: &str) -> Option<&'static FieldDef> {
    step_def(step)?.fields.iter().find(|f| f.key == field)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CanvasItem {
    pub id: String,
    pub step: String,
    pub field: String,
    pub domain: String,
    pub value: String,
    pub status: String,
    pub evidence: String,
    pub source_turn_ids: Vec<String>,
    pub revision: i64,
    /// Last write was a manual user correction.
    pub manual: bool,
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub step: String,
    /// "assumption" | "challenge"
    pub kind: String,
    pub text: String,
    pub source_turn_ids: Vec<String>,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub id: String,
    /// "decision" | "action"
    pub kind: String,
    pub content: String,
    pub rationale: String,
    pub owner: String,
    pub due: String,
    pub confirmation_turn_id: Option<String>,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepCompletion {
    pub step: String,
    pub synthesis: String,
    pub confirmation_turn_id: Option<String>,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Canvas {
    pub items: Vec<CanvasItem>,
    pub notes: Vec<Note>,
    pub decisions: Vec<Decision>,
    pub completed: Vec<StepCompletion>,
}

pub enum Mutation {
    Item(CanvasItem),
    Note(Note),
    Decision(Decision),
    Complete(StepCompletion),
    Read,
}

pub struct ToolCtx {
    /// Most recent final user turn; source/confirmation for accepted claims.
    pub last_user_turn: Option<String>,
    pub now: i64,
}

impl Canvas {
    pub fn item(&self, step: &str, field: &str) -> Option<&CanvasItem> {
        self.items.iter().find(|i| i.step == step && i.field == field)
    }

    pub fn is_complete(&self, step: &str) -> bool {
        self.completed.iter().any(|c| c.step == step)
    }

    /// Minimal content from PRD §5 present: every field filled and not UNKNOWN/PARKED.
    pub fn missing_fields(&self, step: &str) -> Vec<&'static str> {
        step_def(step)
            .map(|s| s.fields)
            .unwrap_or(&[])
            .iter()
            .filter(|f| match self.item(step, f.key) {
                Some(i) => i.value.trim().is_empty() || i.status == "UNKNOWN" || i.status == "PARKED",
                None => true,
            })
            .map(|f| f.label)
            .collect()
    }

    /// "niet gestart" | "actief" | "voldoende uitgewerkt" | "open punten"
    pub fn step_status(&self, step: &str, active: Option<&str>) -> &'static str {
        if self.is_complete(step) {
            "voldoende uitgewerkt"
        } else if active == Some(step) {
            "actief"
        } else if self.items.iter().any(|i| i.step == step) || self.notes.iter().any(|n| n.step == step) {
            "open punten"
        } else {
            "niet gestart"
        }
    }

    /// Step of the most recent item change, used as the "Actief" step.
    pub fn active_step(&self) -> Option<&str> {
        self.items.iter().max_by_key(|i| i.updated_at).map(|i| i.step.as_str())
    }

    /// Compact, traceable context for the model (PRD §11 "canvascontext").
    pub fn summary(&self) -> String {
        let mut s = String::new();
        for step in &STEPS {
            let done = if self.is_complete(step.key) { " [voldoende uitgewerkt]" } else { "" };
            s.push_str(&format!("{}{}:\n", step.key, done));
            for f in step.fields {
                match self.item(step.key, f.key) {
                    Some(i) => s.push_str(&format!(
                        "  - {} = \"{}\" ({}, rev {}{})\n",
                        f.key,
                        i.value,
                        i.status,
                        i.revision,
                        if i.manual { ", handmatig gecorrigeerd door gebruiker" } else { "" }
                    )),
                    None => s.push_str(&format!("  - {} = (leeg, rev 0)\n", f.key)),
                }
            }
            for n in self.notes.iter().filter(|n| n.step == step.key) {
                s.push_str(&format!("  * {}: {}\n", if n.kind == "assumption" { "aanname" } else { "challenge" }, n.text));
            }
        }
        for d in &self.decisions {
            s.push_str(&format!("{}: {} (eigenaar: {}, termijn: {})\n", d.kind, d.content, d.owner, d.due));
        }
        s
    }

    pub fn apply(&mut self, m: &Mutation) {
        match m {
            Mutation::Item(it) => match self.items.iter_mut().find(|i| i.step == it.step && i.field == it.field) {
                Some(slot) => *slot = it.clone(),
                None => self.items.push(it.clone()),
            },
            Mutation::Note(n) => self.notes.push(n.clone()),
            Mutation::Decision(d) => self.decisions.push(d.clone()),
            Mutation::Complete(c) => {
                if !self.is_complete(&c.step) {
                    self.completed.push(c.clone())
                }
            }
            Mutation::Read => {}
        }
    }

    /// A manual user correction: always accepted, bumps revision so pending model writes go stale.
    pub fn manual_edit(&self, step: &str, field: &str, value: &str, status: &str, now: i64) -> Result<CanvasItem, String> {
        let fd = field_def(step, field).ok_or("Onbekend veld")?;
        let value = clip(value)?;
        check_value(&value, status)?;
        let cur = self.item(step, field);
        Ok(CanvasItem {
            id: cur.map(|c| c.id.clone()).unwrap_or_else(new_id),
            step: step.into(),
            field: field.into(),
            domain: fd.domain.into(),
            value,
            status: status.into(),
            evidence: cur.map(|c| c.evidence.clone()).unwrap_or_default(),
            source_turn_ids: cur.map(|c| c.source_turn_ids.clone()).unwrap_or_default(),
            revision: cur.map(|c| c.revision).unwrap_or(0) + 1,
            manual: true,
            updated_at: now,
        })
    }

    /// Mark a step "voldoende uitgewerkt": only with the PRD §5 minimal content and a
    /// registered confirmation (a user turn, or the user's own click).
    pub fn complete(&self, step: &str, synthesis: &str, confirmation: String, now: i64) -> Result<Mutation, String> {
        let s = step_def(step).ok_or_else(|| format!("Onbekende stap '{step}'"))?;
        let missing = self.missing_fields(s.key);
        if !missing.is_empty() {
            return Err(format!("{} is niet voldoende uitgewerkt; ontbreekt: {}. Markeer als open punten.", s.key, missing.join(", ")));
        }
        Ok(Mutation::Complete(StepCompletion {
            step: s.key.into(),
            synthesis: clip(synthesis)?,
            confirmation_turn_id: Some(confirmation),
            created_at: now,
        }))
    }

    /// Value of a field for the closing story, or a visible gap.
    fn say(&self, step: &str, field: &str) -> String {
        match self.item(step, field) {
            Some(i) if !i.value.trim().is_empty() && i.status != "PARKED" => i.value.trim().trim_end_matches('.').to_string(),
            _ => "[nog open]".into(),
        }
    }

    /// The whole story in one sentence per chapter, built from the canvas without an AI call
    /// (works offline, after the session, and costs nothing). Gaps stay visible as "[nog open]".
    pub fn story(&self) -> Vec<String> {
        let s = |step, field| self.say(step, field);
        vec![
            format!(
                "Voor {} lossen we eerst dit op: {}. De klus: {}. Business owner: {}.",
                s("KIES", "doelgroep"), s("KIES", "probleem"), s("KIES", "job_to_be_done"), s("KIES", "business_owner")
            ),
            format!(
                "Succes meten we met {}: van {} naar {}, binnen {}.",
                s("MEET", "kpi"), s("MEET", "baseline"), s("MEET", "target"), s("MEET", "termijn")
            ),
            format!(
                "De AI doet dit: {}, met {}. Grootste risico: {}; daarom {} en {}.",
                s("BEGRENS", "modeltaak"), s("BEGRENS", "databron"), s("BEGRENS", "risico"), s("BEGRENS", "guardrail"), s("BEGRENS", "menselijke_controle")
            ),
            format!(
                "We bouwen {} ({}) en beginnen klein: {}. Geslaagd als: {}.",
                s("REALISEER", "concept"), s("REALISEER", "build_buy_partner"), s("REALISEER", "experiment"), s("REALISEER", "testcriterium")
            ),
            format!(
                "{} borgt het in {}; we evalueren {}.",
                s("VERANKER", "eigenaar"), s("VERANKER", "procesintegratie"), s("VERANKER", "evaluatiemoment")
            ),
        ]
    }

    /// Where the story is: the active chapter (or the first unfinished one) and its open fields.
    pub fn focus_hint(&self) -> String {
        let step = self
            .active_step()
            .filter(|s| !self.is_complete(s))
            .or_else(|| STEPS.iter().map(|s| s.key).find(|s| !self.is_complete(s)));
        match step {
            Some(s) => {
                let missing = self.missing_fields(s);
                if missing.is_empty() {
                    format!("Hoofdstuk {s} is inhoudelijk compleet: vraag om bevestiging van de synthese of ga naar het volgende hoofdstuk.")
                } else {
                    format!("Huidig hoofdstuk: {s}. Nog open: {}.", missing.join(", "))
                }
            }
            None => "Alle hoofdstukken zijn uitgewerkt: vraag naar het besluit en de eerste actie.".into(),
        }
    }

    pub fn view(&self) -> View {
        let active = self.active_step().map(String::from);
        View {
            steps: STEPS
                .iter()
                .map(|s| StepView { step: s.key, status: self.step_status(s.key, active.as_deref()), missing: self.missing_fields(s.key) })
                .collect(),
            active_step: active,
            story: self.story(),
            canvas: self.clone(),
        }
    }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn clip(s: &str) -> Result<String, String> {
    let s = s.trim();
    if s.chars().count() > MAX_TEXT {
        return Err(format!("Tekst langer dan {MAX_TEXT} tekens"));
    }
    Ok(s.to_string())
}

fn check_value(value: &str, status: &str) -> Result<(), String> {
    if !STATUSES.contains(&status) {
        return Err(format!("Onbekende status '{status}'"));
    }
    if value.is_empty() && status != "UNKNOWN" && status != "PARKED" {
        return Err("Lege waarde mag alleen met status UNKNOWN of PARKED".into());
    }
    Ok(())
}

fn str_arg<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a.get(k).and_then(Value::as_str).ok_or_else(|| format!("Argument '{k}' ontbreekt"))
}

fn opt_str(a: &Value, k: &str) -> String {
    a.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string()
}

fn step_arg(a: &Value) -> Result<&'static StepDef, String> {
    let s = str_arg(a, "step")?;
    step_def(s).ok_or_else(|| format!("Onbekende stap '{s}'"))
}

/// Validate a model tool call against the current canvas. `Err` text is returned to the model.
pub fn validate(name: &str, a: &Value, c: &Canvas, ctx: &ToolCtx) -> Result<Mutation, String> {
    let sources = || ctx.last_user_turn.iter().cloned().collect::<Vec<_>>();
    match name {
        "get_canvas_state" => Ok(Mutation::Read),
        "update_canvas_item" => {
            let step = step_arg(a)?;
            let field = str_arg(a, "field")?;
            let fd = field_def(step.key, field).ok_or_else(|| format!("Veld '{field}' hoort niet bij {}", step.key))?;
            let value = clip(str_arg(a, "value")?)?;
            let status = str_arg(a, "status")?;
            let evidence = clip(&opt_str(a, "evidence"))?;
            let expected = a.get("expected_revision").and_then(Value::as_i64).ok_or("Argument 'expected_revision' ontbreekt")?;
            let cur = c.item(step.key, field);
            let rev = cur.map(|i| i.revision).unwrap_or(0);
            if expected != rev {
                let hint = cur.map(|i| format!(" Huidige waarde: \"{}\" ({}).", i.value, i.status)).unwrap_or_default();
                return Err(format!("Verouderde revisie: verwacht {expected}, huidig {rev}.{hint} Gebruik de huidige waarde; draai een gebruikerscorrectie niet terug."));
            }
            if matches!(status, "VALIDATED" | "DECIDED") && ctx.last_user_turn.is_none() {
                return Err(format!("Status {status} vereist een gebruikersbeurt als bron"));
            }
            if status == "VALIDATED" && evidence.chars().count() < 5 {
                return Err("VALIDATED vereist concrete onderbouwing (evidence) van de gebruiker; gebruik anders ASSUMPTION".into());
            }
            check_value(&value, status)?;
            Ok(Mutation::Item(CanvasItem {
                id: cur.map(|i| i.id.clone()).unwrap_or_else(new_id),
                step: step.key.into(),
                field: field.into(),
                domain: fd.domain.into(),
                value,
                status: status.into(),
                evidence,
                source_turn_ids: sources(),
                revision: rev + 1,
                manual: false,
                updated_at: ctx.now,
            }))
        }
        "add_assumption" | "add_challenge" => {
            let step = step_arg(a)?;
            let text = clip(str_arg(a, "text")?)?;
            if text.is_empty() {
                return Err("Lege tekst".into());
            }
            Ok(Mutation::Note(Note {
                id: new_id(),
                step: step.key.into(),
                kind: if name == "add_assumption" { "assumption" } else { "challenge" }.into(),
                text,
                source_turn_ids: sources(),
                created_at: ctx.now,
            }))
        }
        "mark_decision" => {
            let turn = ctx.last_user_turn.clone().ok_or("Een besluit of actie vereist bevestiging door de gebruiker")?;
            decision(&opt_str(a, "kind"), str_arg(a, "content")?, &opt_str(a, "rationale"), &opt_str(a, "owner"), &opt_str(a, "due"), turn, ctx.now)
        }
        "complete_step" => {
            if a.get("user_confirmed").and_then(Value::as_bool) != Some(true) {
                return Err("complete_step vereist dat de gebruiker de synthese expliciet heeft bevestigd (user_confirmed=true)".into());
            }
            let turn = ctx.last_user_turn.clone().ok_or("Geen bevestigende gebruikersbeurt geregistreerd")?;
            c.complete(str_arg(a, "step")?, &opt_str(a, "synthesis"), turn, ctx.now)
        }
        other => Err(format!("Onbekende tool '{other}'")),
    }
}

/// A decision or action, from the model (`mark_decision`) or entered by the user.
pub fn decision(kind: &str, content: &str, rationale: &str, owner: &str, due: &str, turn: String, now: i64) -> Result<Mutation, String> {
    let content = clip(content)?;
    if content.is_empty() {
        return Err("Lege inhoud".into());
    }
    let or_unknown = |v: &str| -> Result<String, String> {
        let v = clip(v)?;
        Ok(if v.is_empty() { "onbekend".into() } else { v })
    };
    Ok(Mutation::Decision(Decision {
        id: new_id(),
        kind: if kind == "action" { "action" } else { "decision" }.into(),
        content,
        rationale: clip(rationale)?,
        owner: or_unknown(owner)?,
        due: or_unknown(due)?,
        confirmation_turn_id: Some(turn),
        created_at: now,
    }))
}

/// Result returned to the model for an accepted mutation.
pub fn tool_result(m: &Mutation, c: &Canvas) -> Value {
    match m {
        Mutation::Read => json!({"ok": true, "canvas": c.summary()}),
        Mutation::Item(i) => json!({"ok": true, "revision": i.revision}),
        _ => json!({"ok": true}),
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    pub step: &'static str,
    pub status: &'static str,
    pub missing: Vec<&'static str>,
}

/// Canvas plus the derived step state, so the UI never re-implements the rules.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub canvas: Canvas,
    pub steps: Vec<StepView>,
    pub active_step: Option<String>,
    /// One sentence per chapter for the closing slide.
    pub story: Vec<String>,
}

/// Tool schemas sent to the Realtime session. Kept to the PRD §11 surface.
pub fn tool_schemas() -> Value {
    let steps: Vec<&str> = STEPS.iter().map(|s| s.key).collect();
    let fields: Vec<&str> = STEPS.iter().flat_map(|s| s.fields.iter().map(|f| f.key)).collect();
    json!([
        {"type": "function", "name": "update_canvas_item",
         "description": "Schrijf een compacte samenvatting in één canvasveld. Gebruik de huidige revisie als expected_revision.",
         "parameters": {"type": "object", "additionalProperties": false,
            "required": ["step", "field", "value", "status", "expected_revision"],
            "properties": {
                "step": {"type": "string", "enum": steps},
                "field": {"type": "string", "enum": fields},
                "value": {"type": "string", "maxLength": MAX_TEXT},
                "status": {"type": "string", "enum": STATUSES},
                "evidence": {"type": "string", "description": "Bron/onderbouwing zoals de gebruiker die noemde; leeg als die ontbreekt."},
                "expected_revision": {"type": "integer"}
            }}},
        {"type": "function", "name": "add_assumption",
         "description": "Leg een aanname vast die nog gevalideerd moet worden.",
         "parameters": {"type": "object", "additionalProperties": false, "required": ["step", "text"],
            "properties": {"step": {"type": "string", "enum": steps}, "text": {"type": "string", "maxLength": MAX_TEXT}}}},
        {"type": "function", "name": "add_challenge",
         "description": "Leg een kritische vraag of tegenstrijdigheid vast.",
         "parameters": {"type": "object", "additionalProperties": false, "required": ["step", "text"],
            "properties": {"step": {"type": "string", "enum": steps}, "text": {"type": "string", "maxLength": MAX_TEXT}}}},
        {"type": "function", "name": "mark_decision",
         "description": "Leg een door de gebruiker bevestigd besluit of een vervolgactie vast.",
         "parameters": {"type": "object", "additionalProperties": false, "required": ["content", "kind"],
            "properties": {
                "kind": {"type": "string", "enum": ["decision", "action"]},
                "content": {"type": "string", "maxLength": MAX_TEXT},
                "rationale": {"type": "string"},
                "owner": {"type": "string", "description": "Leeg als onbekend"},
                "due": {"type": "string", "description": "Leeg als onbekend"}
            }}},
        {"type": "function", "name": "get_canvas_state",
         "description": "Haal de actuele canvasstaat met revisies op.",
         "parameters": {"type": "object", "additionalProperties": false, "properties": {}}},
        {"type": "function", "name": "complete_step",
         "description": "Markeer een stap als voldoende uitgewerkt, alleen nadat de gebruiker de synthese expliciet bevestigde.",
         "parameters": {"type": "object", "additionalProperties": false, "required": ["step", "synthesis", "user_confirmed"],
            "properties": {"step": {"type": "string", "enum": steps}, "synthesis": {"type": "string"}, "user_confirmed": {"type": "boolean"}}}},
        {"type": "function", "name": QUESTION_ANSWERED,
         "description": "De vraag op het scherm is voldoende beantwoord: toon de volgende vraag.",
         "parameters": {"type": "object", "additionalProperties": false, "properties": {}}}
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(user: bool) -> ToolCtx {
        ToolCtx { last_user_turn: user.then(|| "t1".into()), now: 1 }
    }

    fn upd(c: &mut Canvas, field: &str, status: &str, rev: i64, ev: &str) -> Result<(), String> {
        let a = json!({"step": "KIES", "field": field, "value": "x", "status": status, "expected_revision": rev, "evidence": ev});
        let m = validate("update_canvas_item", &a, c, &ctx(true))?;
        c.apply(&m);
        Ok(())
    }

    #[test]
    fn rejects_unknown_field_and_step() {
        let c = Canvas::default();
        let a = json!({"step": "KIES", "field": "kpi", "value": "x", "status": "PARTIAL", "expected_revision": 0});
        assert!(validate("update_canvas_item", &a, &c, &ctx(true)).is_err());
        let a = json!({"step": "X", "field": "kpi", "value": "x", "status": "PARTIAL", "expected_revision": 0});
        assert!(validate("update_canvas_item", &a, &c, &ctx(true)).is_err());
        assert!(validate("delete_everything", &json!({}), &c, &ctx(true)).is_err());
    }

    #[test]
    fn validated_needs_evidence() {
        let mut c = Canvas::default();
        assert!(upd(&mut c, "doelgroep", "VALIDATED", 0, "").is_err());
        assert!(upd(&mut c, "doelgroep", "VALIDATED", 0, "klantonderzoek Q2 2026").is_ok());
    }

    #[test]
    fn stale_write_after_manual_edit_is_rejected() {
        let mut c = Canvas::default();
        upd(&mut c, "doelgroep", "PARTIAL", 0, "").unwrap();
        let m = c.manual_edit("KIES", "doelgroep", "Planners", "DECIDED", 2).unwrap();
        c.apply(&Mutation::Item(m));
        let e = upd(&mut c, "doelgroep", "PARTIAL", 1, "").unwrap_err();
        assert!(e.contains("Verouderde revisie"));
        assert_eq!(c.item("KIES", "doelgroep").unwrap().value, "Planners");
    }

    #[test]
    fn complete_step_requires_content_and_confirmation() {
        let mut c = Canvas::default();
        let a = json!({"step": "KIES", "synthesis": "s", "user_confirmed": true});
        assert!(validate("complete_step", &a, &c, &ctx(true)).is_err());
        for f in STEPS[0].fields {
            upd(&mut c, f.key, "ASSUMPTION", 0, "").unwrap();
        }
        assert!(validate("complete_step", &a, &c, &ctx(false)).is_err());
        assert!(validate("complete_step", &json!({"step": "KIES", "synthesis": "s", "user_confirmed": false}), &c, &ctx(true)).is_err());
        let m = validate("complete_step", &a, &c, &ctx(true)).unwrap();
        c.apply(&m);
        assert_eq!(c.step_status("KIES", None), "voldoende uitgewerkt");
        // parked field blocks completion of another step
        c.items.iter_mut().for_each(|i| i.status = "PARKED".into());
        assert!(!c.missing_fields("KIES").is_empty());
    }

    #[test]
    fn decision_without_user_turn_rejected_and_unknown_owner_explicit() {
        let c = Canvas::default();
        let a = json!({"kind": "action", "content": "Nulmeting plannen"});
        assert!(validate("mark_decision", &a, &c, &ctx(false)).is_err());
        match validate("mark_decision", &a, &c, &ctx(true)).unwrap() {
            Mutation::Decision(d) => assert_eq!((d.owner.as_str(), d.due.as_str()), ("onbekend", "onbekend")),
            _ => panic!(),
        }
    }

    #[test]
    fn manual_completion_shares_rules() {
        let c = Canvas::default();
        assert!(c.complete("BESTAATNIET", "s", "handmatig".into(), 1).is_err());
        assert!(c.complete("KIES", "s", "handmatig".into(), 1).is_err(), "minimal content still required");
        assert!(c.manual_edit("KIES", "doelgroep", "", "DECIDED", 1).is_err(), "empty value only with UNKNOWN/PARKED");
        let v = c.view();
        assert_eq!(v.steps.len(), 5);
        assert_eq!(v.steps[0].status, "niet gestart");
    }

    #[test]
    fn story_marks_gaps_and_uses_values() {
        let mut c = Canvas::default();
        let m = c.manual_edit("KIES", "doelgroep", "Binnendienst.", "DECIDED", 1).unwrap();
        c.apply(&Mutation::Item(m));
        let s = c.story();
        assert_eq!(s.len(), 5);
        assert!(s[0].starts_with("Voor Binnendienst lossen"));
        assert!(s[0].contains("[nog open]"));
    }

    #[test]
    fn length_limit() {
        let c = Canvas::default();
        let a = json!({"step": "KIES", "text": "x".repeat(MAX_TEXT + 1)});
        assert!(validate("add_assumption", &a, &c, &ctx(true)).is_err());
    }
}
