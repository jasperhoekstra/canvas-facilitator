//! Pricing snapshot, usage parsing and USD cost calculation (PRD §10.2).
//! Pure functions; persistence and dedupe live in db.rs.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const BUNDLED_PRICING: &str = include_str!("../pricing.json");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pricing {
    pub version: String,
    pub currency: String,
    pub source: String,
    pub verified_at: String,
    pub models: BTreeMap<String, ModelPrice>,
}

/// USD per 1M tokens, or per audio minute for duration-priced models.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrice {
    pub audio_in: Option<Decimal>,
    pub audio_cached: Option<Decimal>,
    pub audio_out: Option<Decimal>,
    pub text_in: Option<Decimal>,
    pub text_cached: Option<Decimal>,
    pub text_out: Option<Decimal>,
    pub per_minute: Option<Decimal>,
}

impl Pricing {
    pub fn bundled() -> Self {
        serde_json::from_str(BUNDLED_PRICING).expect("bundled pricing.json is valid")
    }
    pub fn parse(json: &str) -> Result<Self, String> {
        let p: Pricing = serde_json::from_str(json).map_err(|e| format!("Ongeldige prijsconfiguratie: {e}"))?;
        if p.models.is_empty() {
            return Err("Prijsconfiguratie bevat geen modellen".into());
        }
        Ok(p)
    }
}

/// Token usage of one response. Input totals include cached tokens (cached is a subset).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub text_in: i64,
    pub text_cached: i64,
    pub audio_in: i64,
    pub audio_cached: i64,
    pub text_out: i64,
    pub audio_out: i64,
    /// Informational subset of text output; never priced separately.
    pub reasoning: i64,
    /// Billed seconds for duration-priced transcription.
    pub billed_seconds: Option<Decimal>,
    /// Categories we could not attribute; shown as unresolved in the UI.
    pub unresolved: Vec<String>,
}

fn n(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(Value::as_i64).unwrap_or(0)
}

/// Parse `response.done.response.usage` of the Realtime API.
pub fn parse_response_usage(u: &Value) -> Usage {
    let mut out = Usage::default();
    let it = &u["input_token_details"];
    let ot = &u["output_token_details"];
    if it.is_object() {
        out.text_in = n(it, "text_tokens");
        out.audio_in = n(it, "audio_tokens");
        let cd = &it["cached_tokens_details"];
        let cached = n(it, "cached_tokens");
        if cd.is_object() {
            out.text_cached = n(cd, "text_tokens");
            out.audio_cached = n(cd, "audio_tokens");
            for (k, v) in cd.as_object().unwrap() {
                if !matches!(k.as_str(), "text_tokens" | "audio_tokens") && v.as_i64().unwrap_or(0) > 0 {
                    out.unresolved.push(format!("cached.{k}={v}"));
                }
            }
        } else if cached > 0 {
            // Unattributed cache hits: priced conservatively as uncached input.
            out.unresolved.push(format!("cached_tokens zonder uitsplitsing={cached}"));
        }
        for (k, v) in it.as_object().unwrap() {
            if !matches!(k.as_str(), "text_tokens" | "audio_tokens" | "cached_tokens" | "cached_tokens_details")
                && v.as_i64().unwrap_or(0) > 0
            {
                out.unresolved.push(format!("input.{k}={v}"));
            }
        }
    } else if n(u, "input_tokens") > 0 {
        out.text_in = n(u, "input_tokens");
        out.unresolved.push("input zonder modality-uitsplitsing (als tekst geprijsd)".into());
    }
    if ot.is_object() {
        out.text_out = n(ot, "text_tokens");
        out.audio_out = n(ot, "audio_tokens");
        out.reasoning = n(ot, "reasoning_tokens");
        for (k, v) in ot.as_object().unwrap() {
            if !matches!(k.as_str(), "text_tokens" | "audio_tokens" | "reasoning_tokens") && v.as_i64().unwrap_or(0) > 0 {
                out.unresolved.push(format!("output.{k}={v}"));
            }
        }
    } else if n(u, "output_tokens") > 0 {
        out.text_out = n(u, "output_tokens");
        out.unresolved.push("output zonder modality-uitsplitsing (als tekst geprijsd)".into());
    }
    out
}

/// Parse a transcription `usage` object; only duration usage is supported.
pub fn parse_transcription_usage(u: &Value) -> Option<Usage> {
    if u["type"] == "duration" {
        let secs = u["seconds"].as_f64()?;
        return Some(Usage { billed_seconds: Decimal::from_f64_retain(secs), ..Default::default() });
    }
    None
}

fn need(p: Option<Decimal>, model: &str, what: &str) -> Result<Decimal, String> {
    p.ok_or_else(|| format!("Geen tarief '{what}' bekend voor {model}; stel een prijs in"))
}

/// USD cost of a usage record. Errors when a needed tariff is missing (calc is then blocked).
pub fn cost(pricing: &Pricing, model: &str, u: &Usage) -> Result<Decimal, String> {
    let p = pricing.models.get(model).ok_or_else(|| format!("Geen prijs bekend voor model {model}"))?;
    let m = Decimal::from(1_000_000);
    let mut total = Decimal::ZERO;
    let tok = |count: i64, rate: Option<Decimal>, what: &str| -> Result<Decimal, String> {
        if count <= 0 {
            return Ok(Decimal::ZERO);
        }
        Ok(Decimal::from(count) * need(rate, model, what)? / m)
    };
    total += tok((u.text_in - u.text_cached).max(0), p.text_in, "textIn")?;
    total += tok(u.text_cached, p.text_cached, "textCached")?;
    total += tok((u.audio_in - u.audio_cached).max(0), p.audio_in, "audioIn")?;
    total += tok(u.audio_cached, p.audio_cached, "audioCached")?;
    total += tok(u.text_out, p.text_out, "textOut")?;
    total += tok(u.audio_out, p.audio_out, "audioOut")?;
    if let Some(s) = u.billed_seconds {
        total += s / Decimal::from(60) * need(p.per_minute, model, "perMinute")?;
    }
    Ok(total)
}

/// Part of a budget the app will actually spend: 10% margin for late usage and overshoot (§10.1).
pub fn usable_budget(budget: Decimal) -> Decimal {
    budget * Decimal::new(9, 1)
}

/// Whether a new response (with its reservation) still fits the budget.
pub fn within_budget(spent: Decimal, reserve: Decimal, budget: Decimal) -> bool {
    spent + reserve <= usable_budget(budget)
}

/// Conservative pre-response reservation (PRD §10.1): bounded output plus generous input and
/// a minute of transcription.
pub fn reservation(pricing: &Pricing, model: &str, transcribe_model: &str, max_output_tokens: i64) -> Decimal {
    let u = Usage { text_in: 12_000, audio_in: 3_000, text_out: max_output_tokens, ..Default::default() };
    let t = Usage { billed_seconds: Some(Decimal::from(60)), ..Default::default() };
    cost(pricing, model, &u).unwrap_or(Decimal::ONE) + cost(pricing, transcribe_model, &t).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }
    fn close(a: Decimal, b: &str) {
        assert!((a - d(b)).abs() <= d("0.0001"), "{a} != {b}");
    }

    #[test]
    fn prd_example_quality_and_mini() {
        let p = Pricing::bundled();
        // PRD §12 "beheerste context": cached tokens are a subset of the input totals.
        let u = Usage {
            audio_in: 3_600 + 50_000,
            audio_cached: 50_000,
            audio_out: 7_200,
            text_in: 10_000 + 50_000,
            text_cached: 50_000,
            text_out: 1_500,
            ..Default::default()
        };
        let t = Usage { billed_seconds: Some(d("900")), ..Default::default() };
        let tr = cost(&p, "gpt-live-transcribe", &t).unwrap();
        close(tr, "0.255");
        close(cost(&p, "gpt-realtime-2.1", &u).unwrap() + tr, "0.947");
        close(cost(&p, "gpt-realtime-2.1-mini", &u).unwrap() + tr, "0.4626");
        // "veel herverwerking"
        let heavy = Usage { audio_in: 25_000 + 50_000, ..u };
        close(cost(&p, "gpt-realtime-2.1", &heavy).unwrap() + tr, "1.6318");
    }

    #[test]
    fn parses_realtime_usage_with_cache() {
        let v = json!({
            "total_tokens": 1000, "input_tokens": 800, "output_tokens": 200,
            "input_token_details": {"text_tokens": 500, "audio_tokens": 300, "cached_tokens": 400,
                "cached_tokens_details": {"text_tokens": 350, "audio_tokens": 50}},
            "output_token_details": {"text_tokens": 20, "audio_tokens": 180}
        });
        let u = parse_response_usage(&v);
        assert_eq!((u.text_in, u.text_cached, u.audio_in, u.audio_cached, u.text_out, u.audio_out), (500, 350, 300, 50, 20, 180));
        assert!(u.unresolved.is_empty());
        // 150*4 + 350*0.4 + 250*32 + 50*0.4 + 20*24 + 180*64 = 600+140+8000+20+480+11520 = 20760 / 1e6
        close(cost(&Pricing::bundled(), "gpt-realtime-2.1", &u).unwrap(), "0.02076");
    }

    #[test]
    fn unknown_categories_stay_visible() {
        let v = json!({"input_token_details": {"text_tokens": 1, "image_tokens": 5, "cached_tokens": 3}});
        let u = parse_response_usage(&v);
        assert_eq!(u.unresolved.len(), 2);
    }

    #[test]
    fn missing_price_blocks_calculation() {
        let mut p = Pricing::bundled();
        p.models.remove("gpt-realtime-2.1");
        assert!(cost(&p, "gpt-realtime-2.1", &Usage { text_in: 1, ..Default::default() }).is_err());
    }

    #[test]
    fn transcription_usage() {
        let u = parse_transcription_usage(&json!({"type": "duration", "seconds": 30})).unwrap();
        close(cost(&Pricing::bundled(), "gpt-live-transcribe", &u).unwrap(), "0.0085");
        assert!(parse_transcription_usage(&json!({"type": "tokens"})).is_none());
    }
}
