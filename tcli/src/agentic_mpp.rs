//! Redot `POST /api/v1/agentic/mpp/pay` + MPP `Authorization: Payment` retry after HTTP 402.
//!
//! Spec: workspace `agentic-mpp-requirements-and-api.md`.

use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use reqwest::header::{HeaderValue, AUTHORIZATION};
use reqwest::{Client, Url};
use serde_json::{json, Value};

use crate::config::ResolvedAuth;
use crate::storage::load_oauth;
use crate::Result;

/// CLI: `tcli agentic-mpp pay` — POST pay API, print JSON, fail if no credential in `data`.
pub async fn run_pay_cli(
    home: &std::path::Path,
    resolved: &ResolvedAuth,
    amount: f64,
    challenge_id: Option<String>,
    method: String,
    pay_variety_code: String,
    recipient: Option<String>,
    token_contract: Option<String>,
    verbose: bool,
) -> Result<()> {
    let session = load_oauth(home)?.ok_or_else(|| {
        crate::Error::msg("not logged in — run `tcli wallet login` first")
    })?;
    let body = build_direct_pay_body(
        amount,
        &method,
        challenge_id.as_deref(),
        &pay_variety_code,
        recipient.as_deref(),
        token_contract.as_deref(),
    )?;
    let client = Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let v = post_agentic_mpp_pay(
        &client,
        &resolved.agentic_mpp_pay_url,
        &session.access_token,
        &body,
        verbose,
    )
    .await?;
    let pretty = serde_json::to_string_pretty(&v)
        .map_err(|e| crate::Error::msg(format!("serialize response: {e}")))?;
    println!("{}", pretty);
    extract_payment_authorization_from_typed_result(&v)?;
    Ok(())
}

/// `POST /api/v1/agentic/mpp/pay` with JSON body; returns full `TypedResult` JSON on HTTP success.
pub async fn post_agentic_mpp_pay(
    client: &Client,
    pay_url: &Url,
    access_token: &str,
    body: &Value,
    verbose: bool,
) -> Result<Value> {
    let bearer = HeaderValue::from_str(&format!("Bearer {}", access_token))
        .map_err(|e| crate::Error::msg(format!("Bearer header: {e}")))?;
    if verbose {
        eprintln!("[verbose] agentic-mpp-pay --> POST {pay_url}");
        eprintln!(
            "[verbose] agentic-mpp-pay --> Authorization: Bearer <redacted>, body: {}",
            serde_json::to_string(body).unwrap_or_else(|_| "<encode err>".into())
        );
    }
    let r = client
        .post(pay_url.clone())
        .header(AUTHORIZATION, bearer)
        .json(body)
        .send()
        .await?;

    let st = r.status();
    let hdrs = r.headers().clone();
    let bytes = r.bytes().await?;
    if verbose {
        crate::api::log_verbose_incoming("agentic-mpp-pay", st, &hdrs, &bytes);
    }

    if !st.is_success() {
        return Err(crate::Error::msg(format!(
            "agentic mpp pay HTTP {}: {}",
            st,
            String::from_utf8_lossy(&bytes).chars().take(500).collect::<String>()
        )));
    }

    serde_json::from_slice(&bytes)
        .map_err(|e| crate::Error::msg(format!("agentic mpp pay: invalid JSON: {e}")))
}

/// Build JSON for [`post_agentic_mpp_pay`] from CLI (no HTTP 402 challenge).
pub fn build_direct_pay_body(
    amount: f64,
    method: &str,
    challenge_id: Option<&str>,
    pay_variety_code: &str,
    recipient: Option<&str>,
    token_contract: Option<&str>,
) -> Result<Value> {
    require_mpp_method_tempo(method)?;
    let mut o = json!({
        "mppMethod": method,
        "method": method,
        "amount": amount,
        "payVarietyCode": pay_variety_code,
    });
    if let Some(cid) = challenge_id.filter(|s| !s.is_empty()) {
        if let Some(m) = o.as_object_mut() {
            m.insert("challengeId".to_string(), json!(cid));
        }
    }
    if method.eq_ignore_ascii_case("tempo") && (recipient.is_some() || token_contract.is_some()) {
        let mut t = json!({});
        if let Some(r) = recipient {
            t["recipient"] = json!(r);
        }
        if let Some(tc) = token_contract {
            t["tokenContract"] = json!(tc);
        }
        if let Some(m) = o.as_object_mut() {
            m.insert("tempo".to_string(), t);
        }
    }
    Ok(o)
}

fn require_mpp_method_tempo(method: &str) -> Result<()> {
    if method.eq_ignore_ascii_case("tempo") {
        return Ok(());
    }
    Err(crate::Error::msg(format!(
        "MPP method `{method}` is not supported yet (only `tempo` for now; other methods such as `stripe` are planned)"
    )))
}

/// TypedResult must be `code==200` and `data` must contain payment credential (per product rule).
pub fn extract_payment_authorization_from_typed_result(v: &Value) -> Result<String> {
    let code = v
        .get("code")
        .and_then(|c| c.as_u64())
        .or_else(|| v.get("code").and_then(|c| c.as_i64()).map(|i| i as u64));
    let msg = v
        .get("msg")
        .or_else(|| v.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("");

    if code != Some(200) {
        return Err(crate::Error::msg(format!(
            "agentic mpp pay failed: code={:?} msg={}",
            code, msg
        )));
    }

    let data = v
        .get("data")
        .filter(|d| !d.is_null())
        .ok_or_else(|| crate::Error::msg("agentic mpp pay: missing data (payment failure)"))?;

    if let Some(s) = data
        .get("credentialAuthorization")
        .and_then(|x| x.as_str())
    {
        let t = s.trim();
        if t.is_empty() {
            return Err(crate::Error::msg(
                "agentic mpp pay: empty credentialAuthorization (payment failure)",
            ));
        }
        return normalize_payment_authorization_value(t);
    }

    if let Some(cred) = data.get("credential") {
        if cred.is_object() {
            return credential_value_to_authorization_header(cred);
        }
    }

    Err(crate::Error::msg(
        "agentic mpp pay: missing credential / credentialAuthorization (payment failure)",
    ))
}

/// Obtain `Authorization` header value (`Payment <base64url(json)>`) after successful agentic pay.
///
/// If the API omits both `credentialAuthorization` and `credential`, returns an error (caller treats as payment failure).
pub async fn obtain_payment_authorization_header(
    client: &Client,
    pay_url: &Url,
    access_token: &str,
    www_authenticate_payment: &str,
    verbose: bool,
) -> Result<String> {
    let params = parse_payment_scheme_params(www_authenticate_payment).ok_or_else(|| {
        crate::Error::msg("agentic mpp: could not parse WWW-Authenticate Payment challenge")
    })?;
    let _ = params
        .get("id")
        .ok_or_else(|| crate::Error::msg("agentic mpp: challenge missing id"))?;
    let request_b64 = params
        .get("request")
        .ok_or_else(|| crate::Error::msg("agentic mpp: challenge missing request"))?;

    let decoded = decode_challenge_request(request_b64)?;
    if verbose {
        let pretty = serde_json::to_string_pretty(&decoded)
            .unwrap_or_else(|_| "<encode err>".into());
        eprintln!(
            "[verbose] agentic-mpp-pay --> challenge `request` decoded (base64url -> JSON):\n{}",
            pretty
        );
    }
    let body = build_agentic_pay_json(&params, &decoded)?;

    let v = post_agentic_mpp_pay(client, pay_url, access_token, &body, verbose).await?;
    extract_payment_authorization_from_typed_result(&v)
}

fn normalize_payment_authorization_value(t: &str) -> Result<String> {
    let u = t.trim();
    if u.to_ascii_lowercase().starts_with("payment ") {
        return Ok(u.to_string());
    }
    Ok(format!("Payment {u}"))
}

fn credential_value_to_authorization_header(cred: &Value) -> Result<String> {
    let s = serde_json::to_string(cred)
        .map_err(|e| crate::Error::msg(format!("credential JSON: {e}")))?;
    let b64 = URL_SAFE_NO_PAD.encode(s.as_bytes());
    Ok(format!("Payment {b64}"))
}

/// One `WWW-Authenticate` header value may contain several MPP challenges separated by `, Payment`
/// (comma before the next `Payment` scheme). This splits without parsing.
pub fn split_www_authenticate_payment_challenges(s: &str) -> Vec<&str> {
    let s = s.trim();
    if s.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < s.len() {
        if i > start && s.as_bytes()[i] == b',' {
            let after_comma = &s[i + 1..];
            let trimmed = after_comma.trim_start();
            if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("payment") {
                let chunk = s[start..i].trim();
                if !chunk.is_empty() {
                    out.push(chunk);
                }
                start = i + 1 + (after_comma.len() - trimmed.len());
                i = start;
                continue;
            }
        }
        i += 1;
    }
    let last = s[start..].trim();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

/// From a single `WWW-Authenticate` header value, pick the `Payment …` segment with `method="tempo"`
/// (if `method` is omitted, it is treated as `tempo`). Returns the substring to pass to
/// [`obtain_payment_authorization_header`].
pub fn select_tempo_payment_challenge<'a>(www_authenticate_value: &'a str) -> Option<&'a str> {
    for seg in split_www_authenticate_payment_challenges(www_authenticate_value) {
        let Some(params) = parse_payment_scheme_params(seg) else {
            continue;
        };
        let method = params
            .get("method")
            .map(|s| s.as_str())
            .unwrap_or("tempo");
        if method.eq_ignore_ascii_case("tempo") {
            return Some(seg);
        }
    }
    None
}

fn parse_payment_scheme_params(www: &str) -> Option<std::collections::HashMap<String, String>> {
    let s = www.trim();
    let rest = strip_prefix_ci(s, "payment")?.trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(parse_key_quoted_values(rest))
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let sl = s.len();
    let pl = prefix.len();
    if sl < pl {
        return None;
    }
    if s[..pl].eq_ignore_ascii_case(prefix) {
        Some(&s[pl..])
    } else {
        None
    }
}

fn parse_key_quoted_values(mut rest: &str) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        if rest.is_empty() {
            break;
        }
        let Some(eq) = rest.find('=') else {
            break;
        };
        let key = rest[..eq].trim();
        rest = rest[eq + 1..].trim_start();
        if !rest.starts_with('"') {
            break;
        }
        rest = &rest[1..];
        let Some(endq) = rest.find('"') else {
            break;
        };
        let val = &rest[..endq];
        m.insert(key.to_string(), val.to_string());
        rest = &rest[endq + 1..];
    }
    m
}

fn decode_challenge_request(request_b64: &str) -> Result<Value> {
    let padded = request_b64.trim();
    let bytes = URL_SAFE_NO_PAD
        .decode(padded.as_bytes())
        .or_else(|_| URL_SAFE.decode(padded.as_bytes()))
        .map_err(|e| crate::Error::msg(format!("challenge request base64url decode: {e}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| crate::Error::msg(format!("challenge request JSON: {e}")))
}

fn build_agentic_pay_json(
    challenge: &std::collections::HashMap<String, String>,
    request: &Value,
) -> Result<Value> {
    let method = challenge
        .get("method")
        .map(|s| s.as_str())
        .unwrap_or("tempo");
    require_mpp_method_tempo(method)?;

    let challenge_id = challenge
        .get("id")
        .ok_or_else(|| crate::Error::msg("challenge id"))?;

    let amount = extract_amount(request)?;

    let mut body = json!({
        "mppMethod": method,
        "method": method,
        "challengeId": challenge_id,
        "amount": amount,
        "payVarietyCode": infer_pay_variety(request),
    });

    if method.eq_ignore_ascii_case("tempo") {
        if let Some(obj) = body.as_object_mut() {
            if let Some(t) = build_tempo_object(request) {
                obj.insert("tempo".to_string(), t);
            }
        }
    }

    Ok(body)
}

fn extract_amount(request: &Value) -> Result<f64> {
    let v = request.get("amount").ok_or_else(|| {
        crate::Error::msg("challenge request JSON missing amount (cannot call agentic mpp pay)")
    })?;
    if let Some(n) = v.as_f64() {
        return Ok(n);
    }
    if let Some(i) = v.as_i64() {
        return Ok(i as f64);
    }
    if let Some(u) = v.as_u64() {
        return Ok(u as f64);
    }
    if let Some(s) = v.as_str() {
        return s
            .parse::<f64>()
            .map_err(|_| crate::Error::msg("challenge request amount: not a number"));
    }
    Err(crate::Error::msg(
        "challenge request amount: unsupported JSON type",
    ))
}

fn infer_pay_variety(request: &Value) -> String {
    request
        .get("currency")
        .and_then(|c| c.as_str())
        .map(|s| {
            let t = s.trim();
            if t.eq_ignore_ascii_case("usd") || t.eq_ignore_ascii_case("usdt") {
                return "usdt".to_string();
            }
            if t.len() <= 12 && !t.starts_with("0x") {
                return t.to_ascii_lowercase();
            }
            "usdt".to_string()
        })
        .unwrap_or_else(|| "usdt".to_string())
}

/// Maps decoded MPP `request` JSON (inside `Payment … request="…"`) into `AgenticMppPayRequest.tempo`.
///
/// Typical Tempo payloads include top-level `recipient`, `currency` (often `0x…` token contract),
/// and `methodDetails.chainId` (EIP-155 style chain id for routing).
fn build_tempo_object(request: &Value) -> Option<Value> {
    let mut t = json!({});
    let obj = t.as_object_mut()?;

    if let Some(r) = request.get("recipient").and_then(|x| x.as_str()) {
        obj.insert("recipient".to_string(), json!(r));
    }
    if let Some(c) = request.get("currency").and_then(|x| x.as_str()) {
        if c.starts_with("0x") {
            obj.insert("tokenContract".to_string(), json!(c));
        }
    }
    if let Some(md) = request.get("methodDetails").and_then(|x| x.as_object()) {
        if let Some(cid) = md.get("chainId") {
            if !cid.is_null() {
                obj.insert("chainId".to_string(), cid.clone());
            }
        }
        if let Some(d) = md.get("decimals") {
            if !d.is_null() {
                obj.insert("decimals".to_string(), d.clone());
            }
        }
    }
    if obj.is_empty() {
        None
    } else {
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_payment_www_authenticate() {
        let s = r#"Payment id="qB3wErTyU7iOpAsD9fGhJk", realm="mpp.dev", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMC41IiwiY3VycmVuY3kiOiJ1c2QifQ""#;
        let m = parse_payment_scheme_params(s).unwrap();
        assert_eq!(m.get("id").map(|s| s.as_str()), Some("qB3wErTyU7iOpAsD9fGhJk"));
        assert_eq!(m.get("method").map(|s| s.as_str()), Some("tempo"));
        assert!(m.get("request").is_some());
    }

    #[test]
    fn challenge_method_stripe_is_rejected() {
        let mut challenge = std::collections::HashMap::new();
        challenge.insert("id".to_string(), "cid".to_string());
        challenge.insert("method".to_string(), "stripe".to_string());
        let request = json!({"amount": 1.0, "currency": "usd"});
        let err = build_agentic_pay_json(&challenge, &request).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("tempo") && msg.contains("stripe"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn direct_pay_body_rejects_non_tempo() {
        let err = build_direct_pay_body(1.0, "stripe", None, "usdt", None, None).unwrap_err();
        assert!(err.to_string().contains("tempo"));
    }

    #[test]
    fn splits_two_payment_challenges_in_one_header() {
        let s = r#"Payment id="a", realm="alchemy.com", method="tempo", intent="charge", request="e30", Payment id="b", realm="alchemy.com", method="stripe", intent="charge", request="e31"#;
        let parts = split_www_authenticate_payment_challenges(s);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("method=\"tempo\""));
        assert!(parts[1].contains("method=\"stripe\""));
    }

    #[test]
    fn select_tempo_picks_tempo_when_listed_before_stripe() {
        let s = r#"Payment id="a", realm="alchemy.com", method="tempo", intent="charge", request="e30", Payment id="b", realm="alchemy.com", method="stripe", intent="charge", request="e31"#;
        let seg = select_tempo_payment_challenge(s).unwrap();
        assert!(seg.contains("method=\"tempo\""));
        assert!(!seg.contains("method=\"stripe\""));
    }

    #[test]
    fn select_tempo_picks_tempo_when_listed_after_stripe() {
        let s = r#"Payment id="b", realm="alchemy.com", method="stripe", intent="charge", request="e31", Payment id="a", realm="alchemy.com", method="tempo", intent="charge", request="e30"#;
        let seg = select_tempo_payment_challenge(s).unwrap();
        assert!(seg.contains("method=\"tempo\""));
        assert!(seg.contains("id=\"a\""));
    }

    #[test]
    fn agentic_pay_maps_tempo_request_including_method_details_chain_id() {
        let mut challenge = std::collections::HashMap::new();
        challenge.insert("id".to_string(), "H3Lv6LuTW6UoIAXtren1-rbdryhXoDOpV2_UMPHtOp8".to_string());
        challenge.insert("method".to_string(), "tempo".to_string());
        let request = json!({
            "amount": "1000",
            "currency": "0x20C0000000000000000000000000009537d11c60E8b50",
            "recipient": "0x7f51327A5A0927815DCcA531aa97Ec7252354091",
            "methodDetails": { "chainId": 4217 }
        });
        let body = build_agentic_pay_json(&challenge, &request).unwrap();
        assert_eq!(body.get("challengeId").and_then(|x| x.as_str()), Some("H3Lv6LuTW6UoIAXtren1-rbdryhXoDOpV2_UMPHtOp8"));
        assert_eq!(body.get("amount").and_then(|x| x.as_f64()), Some(1000.0));
        let tempo = body.get("tempo").unwrap();
        assert_eq!(
            tempo.get("recipient").and_then(|x| x.as_str()),
            Some("0x7f51327A5A0927815DCcA531aa97Ec7252354091")
        );
        assert_eq!(
            tempo.get("tokenContract").and_then(|x| x.as_str()),
            Some("0x20C0000000000000000000000000009537d11c60E8b50")
        );
        assert_eq!(tempo.get("chainId").and_then(|x| x.as_u64()), Some(4217));
    }
}
