use serde::Deserialize;
use serde_json::Value;

/// Detect `WWW-Authenticate: Payment …` (MPP-style).
pub fn is_payment_www_authenticate(val: &str) -> bool {
    let s = val.trim();
    s.to_ascii_lowercase().starts_with("payment")
}

#[derive(Debug, Deserialize)]
pub struct X402Envelope {
    pub x402: Value,
}

pub fn parse_x402_body(body: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(body).ok()?;
    let obj = v.as_object()?;
    obj.get("x402").cloned()
}

/// Rough "problem JSON" detector (e.g. challengeId + payment-required).
pub fn looks_like_payment_problem_json(body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let Some(obj) = v.as_object() else {
        return false;
    };
    let has_challenge = obj.contains_key("challengeId") || obj.contains_key("challenge_id");
    let status = obj
        .get("status")
        .and_then(|x| x.as_str())
        .map(|s| s.eq_ignore_ascii_case("payment-required"))
        .unwrap_or(false)
        || obj
            .get("type")
            .and_then(|x| x.as_str())
            .map(|s| s.contains("payment"))
            .unwrap_or(false);
    has_challenge && status
}
