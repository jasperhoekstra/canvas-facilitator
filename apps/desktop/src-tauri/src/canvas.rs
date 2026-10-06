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
/// The model's tools: fill a tile while the presenter talks, ask the next question (with its
/// inspiration and the tiles it is about), refresh the inspiration, record a decision.
pub const FILL: &str = "vul_vakje";
pub const ASK: &str = "nieuwe_vraag";
pub const INSPIRE: &str = "nieuwe_inspiratie";
const MAX_BULLETS: usize = 4;

/// A canvas field, addressed by step and key.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldRef {
    pub step: String,
    pub field: String,
}

/// What the facilitator shows next to its question: "inspireert" (ideas while a chapter is
/// still open) or "stelt_voor" (concrete proposals), plus the fields the question is about.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Guide {
    pub kind: String,
    pub bullets: Vec<String>,
    pub fields: Vec<FieldRef>,
}

/// Field keys are unique across steps, so a key alone identifies the field.
pub fn field_ref(field: &str) -> Option<FieldRef> {
    STEPS.iter().find(|s| s.fields.iter().any(|f| f.key == field)).map(|s| FieldRef { step: s.key.into(), field: field.into() })
}

fn strings(a: &Value, k: &str) -> Vec<String> {
    a.get(k)
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(Value::as_str).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
        .unwrap_or_default()
}

/// `nieuwe_inspiratie`: kind and bullets.
pub fn parse_inspiration(a: &Value) -> Result<(String, Vec<String>), String> {
    let kind = str_arg(a, "kind")?;
    if !matches!(kind, "inspireert" | "stelt_voor") {
        return Err(format!("Onbekend soort '{kind}'"));
    }
    let bullets: Vec<String> = strings(a, "bullets").into_iter().take(MAX_BULLETS).map(|b| b.chars().take(200).collect()).collect();
    if bullets.is_empty() {
        return Err("Geen bullets".into());
    }
    Ok((kind.into(), bullets))
}

/// `nieuwe_vraag`: the question plus its guide (inspiration and the tiles it is about).
pub fn parse_question(a: &Value) -> Result<(String, Guide), String> {
    let q: String = str_arg(a, "question")?.trim().chars().take(MAX_TEXT).collect();
    if q.is_empty() {
        return Err("Lege vraag".into());
    }
    let (kind, bullets) = parse_inspiration(a)?;
    let fields = strings(a, "fields").iter().filter_map(|f| field_ref(f)).take(MAX_BULLETS).collect();
    Ok((q, Guide { kind, bullets, fields }))
}

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
    /// Field keys asked about but left open (skipped, or still empty after follow-ups): the
    /// story moves past them. Session state only, not stored.
    #[serde(default)]
    pub skipped: Vec<String>,
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

    pub fn is_filled(&self, step: &str, field: &str) -> bool {
        self.item(step, field).is_some_and(|i| !i.value.trim().is_empty() && i.status != "UNKNOWN")
    }

    /// Empty tiles still to ask about (not skipped).
    fn open_fields(&self, step: &str) -> Vec<&'static FieldDef> {
        step_def(step)
            .map(|s| s.fields)
            .unwrap_or(&[])
            .iter()
            .filter(|f| !self.is_filled(step, f.key) && !self.skipped.iter().any(|k| k == f.key))
            .collect()
    }

    /// Confirmed, or nothing left to ask: the story may move on.
    pub fn is_handled(&self, step: &str) -> bool {
        self.is_complete(step) || self.open_fields(step).is_empty()
    }

    /// The step the conversation is on: strictly the first one in order that is not handled.
    /// Answers about later steps are recorded but never move the focus ahead.
    pub fn current_step(&self) -> Option<&'static str> {
        STEPS.iter().map(|s| s.key).find(|s| !self.is_handled(s))
    }

    /// The "Actief" step shown in the UI: the current step once the story has started.
    pub fn active_step(&self) -> Option<&str> {
        if self.items.is_empty() {
            return None;
        }
        self.current_step()
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

    /// Where the story is: the current chapter (strict order), what is filled and what is open.
    pub fn focus_hint(&self) -> String {
        let Some(s) = self.current_step() else {
            return "Alle hoofdstukken zijn doorlopen: vraag kort naar het besluit en de eerste actie.".into();
        };
        let idx = STEPS.iter().position(|d| d.key == s).unwrap_or(0);
        let filled: Vec<String> = STEPS[idx]
            .fields
            .iter()
            .filter_map(|f| self.item(s, f.key).filter(|_| self.is_filled(s, f.key)).map(|i| format!("{} = \"{}\"", f.label, i.value)))
            .collect();
        let open: Vec<String> = self.open_fields(s).iter().map(|f| format!("{} ({})", f.label, f.key)).collect();
        format!(
            "Huidig hoofdstuk: {s} ({} van {}). Al ingevuld: {}. Nog leeg: {}. Vraag ALLEEN naar dit hoofdstuk.",
            idx + 1,
            STEPS.len(),
            if filled.is_empty() { "niets".into() } else { filled.join("; ") },
            open.join(", ")
        )
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

/// Validate a model tool call against the current canvas. `Err` text is returned to the model.
pub fn validate(name: &str, a: &Value, c: &Canvas, ctx: &ToolCtx) -> Result<Mutation, String> {
    match name {
        FILL => {
            let field = str_arg(a, "field")?;
            let r = field_ref(field).ok_or_else(|| format!("Onbekend vakje '{field}'"))?;
            let fd = field_def(&r.step, field).ok_or("Onbekend vakje")?;
            let value = clip(str_arg(a, "value")?)?;
            if value.is_empty() {
                return Err("Lege waarde".into());
            }
            let cur = c.item(&r.step, field);
            // A manual correction by the presenter is leading.
            if cur.is_some_and(|i| i.manual) {
                return Err(format!("'{}' is handmatig aangepast door de gebruiker; niet overschrijven.", fd.label));
            }
            Ok(Mutation::Item(CanvasItem {
                id: cur.map(|i| i.id.clone()).unwrap_or_else(new_id),
                step: r.step,
                field: field.into(),
                domain: fd.domain.into(),
                value,
                status: "PARTIAL".into(),
                evidence: String::new(),
                source_turn_ids: ctx.last_user_turn.iter().cloned().collect(),
                revision: cur.map_or(0, |i| i.revision) + 1,
                manual: false,
                updated_at: ctx.now,
            }))
        }
        "mark_decision" => {
            let turn = ctx.last_user_turn.clone().ok_or("Een besluit of actie vereist een gebruikersbeurt")?;
            decision(&opt_str(a, "kind"), str_arg(a, "content")?, "", &opt_str(a, "owner"), &opt_str(a, "due"), turn, ctx.now)
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
    let fields: Vec<&str> = STEPS.iter().flat_map(|s| s.fields.iter().map(|f| f.key)).collect();
    let bullets = json!({"type": "array", "items": {"type": "string", "maxLength": 200}, "minItems": 2, "maxItems": MAX_BULLETS});
    let kind = json!({"type": "string", "enum": ["inspireert", "stelt_voor"]});
    json!([
        {"type": "function", "name": FILL,
         "description": "Vul één vakje met een korte samenvatting van wat gezegd is (herschrijft de hele waarde).",
         "parameters": {"type": "object", "additionalProperties": false, "required": ["field", "value"],
            "properties": {"field": {"type": "string", "enum": fields}, "value": {"type": "string", "maxLength": MAX_TEXT}}}},
        {"type": "function", "name": ASK,
         "description": "Toon de volgende vraag, met inspiratie (inspireert) of voorstellen (stelt_voor) en de vakjes waar de vraag over gaat.",
         "parameters": {"type": "object", "additionalProperties": false, "required": ["question", "kind", "bullets", "fields"],
            "properties": {
                "question": {"type": "string", "maxLength": 200},
                "kind": kind,
                "bullets": bullets,
                "fields": {"type": "array", "items": {"type": "string", "enum": fields}, "minItems": 1, "maxItems": 3}
            }}},
        {"type": "function", "name": INSPIRE,
         "description": "Vervang de inspiratie naast de huidige vraag door nieuwe bullets.",
         "parameters": {"type": "object", "additionalProperties": false, "required": ["kind", "bullets"],
            "properties": {"kind": kind, "bullets": bullets}}},
        {"type": "function", "name": "mark_decision",
         "description": "Leg aan het eind een door de gebruiker genoemd besluit of eerste actie vast.",
         "parameters": {"type": "object", "additionalProperties": false, "required": ["content", "kind"],
            "properties": {
                "kind": {"type": "string", "enum": ["decision", "action"]},
                "content": {"type": "string", "maxLength": MAX_TEXT},
                "owner": {"type": "string", "description": "Leeg als onbekend"},
                "due": {"type": "string", "description": "Leeg als onbekend"}
            }}}
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(user: bool) -> ToolCtx {
        ToolCtx { last_user_turn: user.then(|| "t1".into()), now: 1 }
    }

    fn fill(c: &mut Canvas, field: &str) -> Result<(), String> {
        let m = validate(FILL, &json!({"field": field, "value": "x"}), c, &ctx(true))?;
        c.apply(&m);
        Ok(())
    }

    #[test]
    fn fill_finds_the_step_and_rejects_unknown() {
        let mut c = Canvas::default();
        fill(&mut c, "kpi").unwrap();
        assert!(c.is_filled("MEET", "kpi"));
        assert_eq!(c.item("MEET", "kpi").unwrap().revision, 1);
        assert!(fill(&mut c, "bestaatniet").is_err());
        assert!(validate(FILL, &json!({"field": "kpi", "value": " "}), &c, &ctx(true)).is_err());
        assert!(validate("update_canvas_item", &json!({}), &c, &ctx(true)).is_err());
    }

    #[test]
    fn manual_edit_is_leading() {
        let mut c = Canvas::default();
        let m = c.manual_edit("KIES", "doelgroep", "Planners", "DECIDED", 2).unwrap();
        c.apply(&Mutation::Item(m));
        assert!(fill(&mut c, "doelgroep").is_err());
        assert_eq!(c.item("KIES", "doelgroep").unwrap().value, "Planners");
    }

    #[test]
    fn steps_are_strictly_ordered_and_skipped_fields_move_on() {
        let mut c = Canvas::default();
        assert_eq!(c.current_step(), Some("KIES"));
        assert_eq!(c.active_step(), None, "nothing said yet");
        // An answer about a later chapter is recorded but does not move the focus.
        fill(&mut c, "kpi").unwrap();
        assert_eq!(c.active_step(), Some("KIES"));
        assert!(c.focus_hint().contains("Huidig hoofdstuk: KIES"));
        for f in STEPS[0].fields.iter().skip(1) {
            fill(&mut c, f.key).unwrap();
        }
        assert_eq!(c.current_step(), Some("KIES"), "one tile still open");
        c.skipped.push(STEPS[0].fields[0].key.into());
        assert_eq!(c.current_step(), Some("MEET"));
        let h = c.focus_hint();
        assert!(h.contains("Huidig hoofdstuk: MEET") && h.contains("KPI = \"x\""), "{h}");
    }

    #[test]
    fn question_is_parsed_and_bounded() {
        let (q, g) = parse_question(&json!({"question": " Hoe meet je dat? ", "kind": "stelt_voor",
            "bullets": ["a", " ", "b", "c", "d", "e"], "fields": ["kpi", "bestaatniet"]})).unwrap();
        assert_eq!(q, "Hoe meet je dat?");
        assert_eq!(g.bullets, vec!["a", "b", "c", "d"]);
        assert_eq!(g.fields, vec![FieldRef { step: "MEET".into(), field: "kpi".into() }]);
        assert!(parse_question(&json!({"question": "", "kind": "inspireert", "bullets": ["a"], "fields": []})).is_err());
        assert!(parse_inspiration(&json!({"kind": "x", "bullets": ["a"]})).is_err());
        assert!(parse_inspiration(&json!({"kind": "inspireert", "bullets": []})).is_err());
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
        let a = json!({"field": "kpi", "value": "x".repeat(MAX_TEXT + 1)});
        assert!(validate(FILL, &a, &c, &ctx(true)).is_err());
    }
}
