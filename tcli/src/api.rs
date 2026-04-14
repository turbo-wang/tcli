use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use reqwest::{Client, Method, StatusCode, Url};
use serde_json::{json, Value};

use crate::config::ResolvedAuth;
use crate::storage::load_oauth;
use crate::x402;
use crate::Result;

const DEFAULT_PAYMENT_TOKEN_HEADER: &str = "X-Payment-Token";
const X402_ACCEPT_HEADER: &str = "X-x402-Accept";

#[derive(Debug, Clone)]
pub struct RequestArgs {
    pub url: String,
    pub method: Option<String>,
    pub json_body: Option<String>,
    pub data_pairs: Vec<String>,
    pub headers: Vec<String>,
    pub timeout_secs: Option<u64>,
    pub dry_run: bool,
    pub max_spend: Option<String>,
    pub verbose: bool,
    pub payment_token_header: Option<String>,
}

fn parse_method(args: &RequestArgs) -> Method {
    if let Some(m) = &args.method {
        return m.parse().unwrap_or(Method::GET);
    }
    if args.json_body.is_some() || !args.data_pairs.is_empty() {
        return Method::POST;
    }
    Method::GET
}

fn build_header_map(headers: &[String]) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for h in headers {
        let Some((k, v)) = h.split_once(':') else {
            return Err(crate::Error::msg(format!(
                "invalid header (expected Name: value): {h}"
            )));
        };
        let name = HeaderName::from_bytes(k.trim().as_bytes())
            .map_err(|e| crate::Error::msg(format!("bad header name: {e}")))?;
        let value = HeaderValue::from_str(v.trim())
            .map_err(|e| crate::Error::msg(format!("bad header value: {e}")))?;
        map.insert(name, value);
    }
    Ok(map)
}

fn merge_body(args: &RequestArgs) -> Option<Vec<u8>> {
    if let Some(j) = &args.json_body {
        return Some(j.as_bytes().to_vec());
    }
    if args.data_pairs.is_empty() {
        return None;
    }
    let joined = args.data_pairs.join("&");
    Some(joined.into_bytes())
}

fn content_type_for(args: &RequestArgs) -> Option<&'static str> {
    if args.json_body.is_some() {
        Some("application/json")
    } else if !args.data_pairs.is_empty() {
        Some("application/x-www-form-urlencoded")
    } else {
        None
    }
}

fn verbose_header_value(k: &reqwest::header::HeaderName, v: &HeaderValue) -> String {
    let name = k.as_str();
    if name.eq_ignore_ascii_case("authorization")
        || name.eq_ignore_ascii_case("x-payment-token")
    {
        return "<redacted>".to_string();
    }
    v.to_str().unwrap_or("<binary>").to_string()
}

/// Max request body logged in verbose mode (avoid huge stdin).
const MAX_VERBOSE_LOG_BYTES: usize = 1_000_000;

fn log_verbose_outgoing(
    label: &str,
    method: &Method,
    url: &Url,
    headers: &HeaderMap,
    body: &Option<Vec<u8>>,
    args: &RequestArgs,
) {
    if !args.verbose {
        return;
    }
    eprintln!("[verbose] {label} --> {method} {url}");
    for (k, v) in headers.iter() {
        eprintln!("[verbose] {label} --> {}: {}", k, verbose_header_value(k, v));
    }
    if let Some(ct) = content_type_for(args) {
        eprintln!("[verbose] {label} --> Content-Type: {ct}");
    }
    if let Some(b) = body {
        let displayed = truncate_for_verbose_log(b);
        eprintln!("[verbose] {label} --> body ({} bytes):\n{displayed}", b.len());
    }
}

fn truncate_for_verbose_log(body: &[u8]) -> String {
    let s = String::from_utf8_lossy(body);
    if s.len() > MAX_VERBOSE_LOG_BYTES {
        format!(
            "{}…\n[verbose] … [truncated: {} bytes total in request]",
            &s[..MAX_VERBOSE_LOG_BYTES],
            s.len()
        )
    } else {
        s.into_owned()
    }
}

/// Redact common OAuth / payment token fields in JSON for stderr logs (verbose).
fn redact_sensitive_json(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                if matches!(
                    k.as_str(),
                    "access_token" | "refresh_token" | "payment_token" | "id_token" | "device_code"
                ) {
                    *val = Value::String("<redacted>".to_string());
                } else {
                    redact_sensitive_json(val);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_sensitive_json(item);
            }
        }
        _ => {}
    }
}

fn format_body_for_verbose_response(body: &[u8]) -> String {
    let s = String::from_utf8_lossy(body);
    let t = s.trim();
    if (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']')) {
        if let Ok(mut v) = serde_json::from_str::<Value>(t) {
            redact_sensitive_json(&mut v);
            let pretty = serde_json::to_string_pretty(&v).unwrap_or_else(|_| s.to_string());
            if pretty.len() > MAX_VERBOSE_LOG_BYTES {
                return format!(
                    "{}…\n[verbose] … [truncated JSON log, {} bytes total]",
                    &pretty[..MAX_VERBOSE_LOG_BYTES],
                    body.len()
                );
            }
            return pretty;
        }
    }
    truncate_for_verbose_log(body)
}

/// Full response dump to stderr (verbose). Response body still written to stdout unchanged.
fn log_verbose_incoming(
    label: &str,
    status: StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
) {
    eprintln!("[verbose] {label} <-- HTTP {status}");
    for (k, v) in headers.iter() {
        let vs = v.to_str().unwrap_or("<binary>");
        eprintln!("[verbose] {label} <-- {}: {}", k, vs);
    }
    let displayed = format_body_for_verbose_response(body);
    eprintln!(
        "[verbose] {label} <-- body ({} bytes):\n{displayed}",
        body.len()
    );
}

async fn send_request(
    client: &Client,
    method: &Method,
    url: &Url,
    headers: &HeaderMap,
    body: &Option<Vec<u8>>,
    args: &RequestArgs,
) -> Result<reqwest::Response> {
    log_verbose_outgoing("request", method, url, headers, body, args);
    let mut req = client.request(method.clone(), url.clone());
    for (k, v) in headers.iter() {
        req = req.header(k, v);
    }
    if let Some(ct) = content_type_for(args) {
        req = req.header(reqwest::header::CONTENT_TYPE, ct);
    }
    if let Some(b) = body {
        req = req.body(b.clone());
    }
    Ok(req.send().await?)
}

fn write_response(
    status: StatusCode,
    _headers: &reqwest::header::HeaderMap,
    body: &[u8],
    verbose: bool,
) -> Result<()> {
    if verbose {
        eprintln!(
            "[verbose] → stdout: same body as last [verbose] block, HTTP {status}, {} bytes",
            body.len()
        );
    }
    use std::io::Write;
    std::io::stdout().write_all(body)?;
    if verbose {
        eprintln!();
    }
    Ok(())
}

pub async fn run_request(
    home: &std::path::Path,
    resolved: &ResolvedAuth,
    args: &RequestArgs,
) -> Result<()> {
    let method = parse_method(args);
    let url: Url = args
        .url
        .parse()
        .map_err(|e: url::ParseError| crate::Error::from(e))?;
    let body = merge_body(args);
    let mut headers = build_header_map(&args.headers)?;

    if args.dry_run {
        eprintln!("DRY RUN {method} {url}");
        if args.verbose {
            eprintln!("headers: {headers:?}");
            if let Some(b) = &body {
                eprintln!("body: {}", String::from_utf8_lossy(b));
            }
        }
        return Ok(());
    }

    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(
            args.timeout_secs.unwrap_or(30),
        ))
        .build()?;

    let resp = send_request(&client, &method, &url, &headers, &body, args).await?;
    let mut status = resp.status();
    let mut hdrs = resp.headers().clone();
    let mut body_bytes = resp.bytes().await?.to_vec();
    if args.verbose {
        log_verbose_incoming("response#1", status, &hdrs, &body_bytes);
    }

    if status != StatusCode::PAYMENT_REQUIRED {
        return write_response(status, &hdrs, &body_bytes, args.verbose);
    }

    // --- 402 handling ---
    let mut retried_with_payment_token = false;

    // 1) Payment token (demo)
    if !resolved.payment_token_disabled {
        if let Some(ref pt_url) = resolved.payment_token_url {
            match issue_payment_token(
                pt_url.as_str(),
                args.url.as_str(),
                status.as_u16(),
                &body_bytes,
                args.verbose,
            )
            .await
            {
                Ok(token) => {
                    let hname = args
                        .payment_token_header
                        .as_deref()
                        .unwrap_or(DEFAULT_PAYMENT_TOKEN_HEADER);
                    let hn = HeaderName::from_bytes(hname.as_bytes())
                        .map_err(|e| crate::Error::msg(format!("payment token header: {e}")))?;
                    let hv = HeaderValue::from_str(&token)
                        .map_err(|e| crate::Error::msg(format!("payment token value: {e}")))?;
                    headers.insert(hn, hv);
                    retried_with_payment_token = true;

                    let r2 = send_request(&client, &method, &url, &headers, &body, args).await?;
                    status = r2.status();
                    hdrs = r2.headers().clone();
                    body_bytes = r2.bytes().await?.to_vec();
                    if args.verbose {
                        log_verbose_incoming("response#2 (after payment-token retry)", status, &hdrs, &body_bytes);
                    }
                }
                Err(e) => {
                    if args.verbose {
                        eprintln!("issue-token skipped or failed: {e}");
                    }
                }
            }
        }
    }

    // Still 402? Continue checks on latest response.
    if status == StatusCode::PAYMENT_REQUIRED {
        // 2) MPP
        if let Some(www) = hdrs.get(reqwest::header::WWW_AUTHENTICATE) {
            if let Ok(s) = www.to_str() {
                if x402::is_payment_www_authenticate(s) {
                    if retried_with_payment_token {
                        return Err(crate::Error::msg(
                            "Payment required (MPP) persists after payment-token retry. \
                            On-chain signing is not implemented in tcli — use `tempo request` \
                            or see https://mpp.dev for wallet flows.",
                        ));
                    }
                    return Err(crate::Error::msg(
                        "Payment required (MPP / WWW-Authenticate: Payment). \
                        tcli does not perform on-chain signing — use `tempo request` \
                        or see https://mpp.dev.",
                    ));
                }
            }
        }

        // 3) Legacy x402 JSON body
        if x402::parse_x402_body(&String::from_utf8_lossy(&body_bytes)).is_some() {
            let session = load_oauth(home)?;
            if session.is_none() {
                return Err(crate::Error::msg(
                    "402 x402 body requires a logged-in session (`tcli wallet login`).",
                ));
            }
            check_max_spend(args)?;
            let sess = session.unwrap();
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", sess.access_token))
                    .map_err(|e| crate::Error::msg(format!("{e}")))?,
            );
            let x402_h = HeaderName::from_bytes(X402_ACCEPT_HEADER.as_bytes())
                .map_err(|e| crate::Error::msg(format!("{e}")))?;
            headers.insert(x402_h, HeaderValue::from_static("1"));

            let r3 = send_request(&client, &method, &url, &headers, &body, args).await?;
            status = r3.status();
            hdrs = r3.headers().clone();
            body_bytes = r3.bytes().await?.to_vec();
            if args.verbose {
                log_verbose_incoming("response#3 (after x402 retry)", status, &hdrs, &body_bytes);
            }
        }

        // 4) Problem JSON
        let body_str = String::from_utf8_lossy(&body_bytes);
        if x402::looks_like_payment_problem_json(body_str.as_ref()) {
            return Err(crate::Error::msg(
                "Payment challenge (problem JSON). Use official `tempo request` for Tempo wallet flows.",
            ));
        }
    }

    write_response(status, &hdrs, &body_bytes, args.verbose)
}

fn check_max_spend(args: &RequestArgs) -> Result<()> {
    let env_spend = std::env::var("TCLI_MAX_SPEND").ok();
    let cli_spend = args.max_spend.clone();
    if env_spend.is_none() && cli_spend.is_none() {
        return Err(crate::Error::msg(
            "x402 retry requires a budget: set --max-spend or TCLI_MAX_SPEND.",
        ));
    }
    Ok(())
}

async fn issue_payment_token(
    issue_url: &str,
    original_url: &str,
    response_status: u16,
    response_body: &[u8],
    verbose: bool,
) -> Result<String> {
    let client = Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let body = json!({
        "original_url": original_url,
        "response_status": response_status,
        "response_body": String::from_utf8_lossy(response_body).to_string(),
    });
    if verbose {
        eprintln!("[verbose] issue-token --> POST {issue_url}");
        eprintln!(
            "[verbose] issue-token --> body: {}",
            serde_json::to_string(&body).unwrap_or_else(|_| "<encode error>".into())
        );
    }
    let r = client.post(issue_url).json(&body).send().await?;
    let st = r.status();
    let hdrs = r.headers().clone();
    let bytes = r.bytes().await?;
    if verbose {
        log_verbose_incoming("issue-token", st, &hdrs, &bytes);
    }
    if !st.is_success() {
        return Err(crate::Error::msg(format!(
            "issue-token HTTP {}",
            st
        )));
    }
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
        crate::Error::msg(format!("issue-token: invalid JSON: {e}"))
    })?;
    let token = v
        .get("payment_token")
        .and_then(|x| x.as_str())
        .ok_or_else(|| crate::Error::msg("issue-token: missing payment_token"))?;
    Ok(token.to_string())
}
