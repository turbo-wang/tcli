use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use image::ImageFormat;
use image::Luma;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use oauth2::devicecode::{
    DeviceAuthorizationResponse, ExtraDeviceAuthorizationFields,
};
use qrcode::QrCode;
use serde::{Deserialize, Serialize};

use crate::config::ResolvedAuth;
use crate::storage::{OAuthStored, oauth_path, save_oauth};
use crate::Result;

const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// Extra JSON fields on `POST .../device_authorization` beyond RFC 8628.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DeviceAuthorizationExtraFields {
    /// Preferred payload for the login QR image (see `login_qr_png_bytes`).
    ///
    /// Agentic MPP / `qr_code` 表：常为 **业务 URI**（如 `redotpay:…`），由 App/系统处理；CLI 将其**整段编入二维码**并保存为 PNG。
    /// 兼容：PNG 的 Base64 / `data:image/...;base64,...`，或 `http(s)://` 字符串。Jackson 可能序列化为 `qrCode`。
    #[serde(default, alias = "qrCode")]
    pub qr_code: Option<String>,
}

impl ExtraDeviceAuthorizationFields for DeviceAuthorizationExtraFields {}

/// Parsed device authorization response, including optional `qr_code`.
pub type DeviceAuthorizationDetails = DeviceAuthorizationResponse<DeviceAuthorizationExtraFields>;

/// oauth2's `RequestTokenError::Request` only displays as "Request failed"; the useful part is in
/// [`std::error::Error::source`]. Walk the chain so users see e.g. connection errors and URLs.
fn format_err_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(next) = cur {
        s.push_str(": ");
        s.push_str(&next.to_string());
        cur = next.source();
    }
    s
}

fn enrich_oauth_failure(message: String, auth_base: &url::Url) -> String {
    let lower = message.to_lowercase();
    if lower.contains("connection refused")
        || lower.contains("failed to connect")
        || lower.contains("error trying to connect")
        || lower.contains("tcp connect error")
        || lower.contains("operation timed out")
        || lower.contains("timed out")
        || lower.contains("network is unreachable")
        || lower.contains("host unreachable")
        || lower.contains("dns")
        || lower.contains("error resolving")
        || lower.contains("could not resolve")
        || lower.contains("connection reset")
    {
        format!(
            "{message}\n\
             Hint: ensure the auth server is reachable at {auth_base}. \
             If you use HTTP_PROXY, set NO_PROXY=127.0.0.1,localhost,::1 for local OAuth."
        )
    } else {
        message
    }
}

/// How the device-flow token poll is run after the QR step.
#[derive(Debug, Clone, Copy)]
pub struct LoginOptions {
    /// When `true` (default for CLI), print QR path, `MEDIA:`, and `VERIFICATION_CODE:` lines on stdout and spawn a detached
    /// `tcli wallet login --poll-state …` process to poll the token endpoint. When `false` (e.g.
    /// integration tests), poll in the current task so the process does not exit before tokens are saved.
    pub detach_poll: bool,
}

impl Default for LoginOptions {
    fn default() -> Self {
        Self {
            detach_poll: true,
        }
    }
}

const POLL_STATE_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
struct AuthResolvedSnapshot {
    base: String,
    client_id: String,
    device_authorization_url: String,
    token_url: String,
}

impl AuthResolvedSnapshot {
    fn from_resolved(r: &ResolvedAuth) -> Self {
        Self {
            base: r.base.to_string(),
            client_id: r.client_id.clone(),
            device_authorization_url: r.device_authorization_url.to_string(),
            token_url: r.token_url.to_string(),
        }
    }

    fn to_resolved(&self) -> Result<ResolvedAuth> {
        Ok(ResolvedAuth {
            base: self
                .base
                .parse()
                .map_err(|e| crate::Error::msg(format!("poll state base URL: {e}")))?,
            client_id: self.client_id.clone(),
            device_authorization_url: self
                .device_authorization_url
                .parse()
                .map_err(|e| crate::Error::msg(format!("poll state device_authorization_url: {e}")))?,
            token_url: self
                .token_url
                .parse()
                .map_err(|e| crate::Error::msg(format!("poll state token_url: {e}")))?,
            payment_token_url: None,
            payment_token_disabled: true,
            app_name: "tcli".to_string(),
            device_name: "tcli-device".to_string(),
            oauth_scope: None,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DevicePollState {
    version: u32,
    home: String,
    /// Directory containing `login_qr.png`; `result.json` is written here when polling finishes.
    artifact_dir: String,
    resolved: AuthResolvedSnapshot,
    device_authorization: DeviceAuthorizationDetails,
}

fn poll_state_path(home: &Path) -> PathBuf {
    home.join("wallet").join(".device_login_poll.json")
}

/// Writes the login QR under `~/.openclaw/workspace/tcli-login/<session>/` (see [`crate::storage::openclaw_login_qr_png_path`]).
fn write_login_qr_png_file(png: &[u8], verbose: bool) -> Result<PathBuf> {
    let path = crate::storage::openclaw_login_qr_png_path();
    if verbose {
        eprintln!("[verbose] login QR output path: {}", path.display());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(crate::Error::Io)?;
    }
    std::fs::write(&path, png).map_err(crate::Error::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
    }
    path.canonicalize().map_err(crate::Error::Io)
}

/// Atomically write `result.json` for OpenClaw (single read after login command returns).
fn write_login_result_json(artifact_dir: &std::path::Path, value: &serde_json::Value) -> Result<()> {
    std::fs::create_dir_all(artifact_dir).map_err(crate::Error::Io)?;
    let path = artifact_dir.join("result.json");
    let tmp = path.with_extension("json.part");
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| crate::Error::msg(format!("serialize login result: {e}")))?;
    std::fs::write(&tmp, &bytes).map_err(crate::Error::Io)?;
    std::fs::rename(&tmp, &path).map_err(crate::Error::Io)?;
    Ok(())
}

fn write_poll_state(
    home: &Path,
    resolved: &ResolvedAuth,
    details: &DeviceAuthorizationDetails,
    artifact_dir: &Path,
) -> Result<PathBuf> {
    let wallet_dir = home.join("wallet");
    std::fs::create_dir_all(&wallet_dir).map_err(crate::Error::Io)?;
    let path = poll_state_path(home);
    let tmp = path.with_extension("json.tmp");
    let artifact_dir = artifact_dir
        .canonicalize()
        .unwrap_or_else(|_| artifact_dir.to_path_buf());
    let state = DevicePollState {
        version: POLL_STATE_VERSION,
        home: home.to_string_lossy().to_string(),
        artifact_dir: artifact_dir.to_string_lossy().to_string(),
        resolved: AuthResolvedSnapshot::from_resolved(resolved),
        device_authorization: details.clone(),
    };
    let json = serde_json::to_vec_pretty(&state).map_err(|e| {
        crate::Error::msg(format!("serialize device login state: {e}"))
    })?;
    std::fs::write(&tmp, &json).map_err(crate::Error::Io)?;
    std::fs::rename(&tmp, &path).map_err(crate::Error::Io)?;
    Ok(path)
}

/// Resume polling from a state file (internal `tcli wallet login --poll-state`).
pub async fn login_poll_from_state_file(state_path: &Path, verbose: bool) -> Result<()> {
    let raw = std::fs::read_to_string(state_path)
        .map_err(|e| crate::Error::msg(format!("read poll state {}: {e}", state_path.display())))?;
    let state: DevicePollState = serde_json::from_str(&raw).map_err(|e| {
        crate::Error::msg(format!("parse poll state {}: {e}", state_path.display()))
    })?;
    if state.version != POLL_STATE_VERSION {
        return Err(crate::Error::msg(format!(
            "unsupported poll state version {} (expected {})",
            state.version, POLL_STATE_VERSION
        )));
    }
    let home = PathBuf::from(state.home);
    let artifact_dir = PathBuf::from(&state.artifact_dir);
    let resolved = state.resolved.to_resolved()?;

    let token_outcome =
        exchange_device_token(&home, &resolved, &state.device_authorization, verbose).await;

    let remove_state = || {
        let _ = std::fs::remove_file(state_path);
    };

    match token_outcome {
        Ok(stored) => {
            let oauth_p = oauth_path(&home);
            let v = serde_json::json!({
                "status": "ok",
                "oauth_path": oauth_p.to_string_lossy(),
                "expires_at": stored.expires_at,
            });
            if let Err(e) = write_login_result_json(&artifact_dir, &v) {
                eprintln!("warning: could not write result.json: {e}");
            }
            remove_state();
            eprintln!("Wallet connected! Token: {}", oauth_p.display());
            print_login_success_banner_stderr(stored.expires_at);
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            let v = serde_json::json!({
                "status": "error",
                "message": msg,
            });
            let _ = write_login_result_json(&artifact_dir, &v);
            remove_state();
            Err(e)
        }
    }
}

/// POST JSON for device authorization (`appName`, `deviceSn`, `timestamp`, …).
#[derive(Serialize)]
struct DeviceAuthorizationJsonBody<'a> {
    client_id: &'a str,
    #[serde(rename = "appName")]
    app_name: &'a str,
    #[serde(rename = "publicKey")]
    public_key: &'a str,
    #[serde(rename = "deviceName")]
    device_name: &'a str,
    #[serde(rename = "deviceSn")]
    device_sn: &'a str,
    timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<&'a str>,
}

async fn request_device_authorization(
    resolved: &ResolvedAuth,
    device_sn: &str,
    verbose: bool,
) -> Result<DeviceAuthorizationDetails> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(1)
        .max(1);
    let body = DeviceAuthorizationJsonBody {
        client_id: &resolved.client_id,
        app_name: &resolved.app_name,
        public_key: "",
        device_name: &resolved.device_name,
        device_sn,
        timestamp: ts,
        scope: resolved.oauth_scope.as_deref(),
    };
    let client = crate::oauth_http::shared_oauth_reqwest_client();
    let url = resolved.device_authorization_url.as_str();
    if verbose {
        eprintln!("[verbose] POST JSON device_authorization → {url}");
    }
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e: reqwest::Error| {
            crate::Error::msg(enrich_oauth_failure(
                format!("device_authorization request failed: {}", format_err_chain(&e)),
                &resolved.base,
            ))
        })?;

    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e: reqwest::Error| {
        crate::Error::msg(format!("device_authorization response body: {e}"))
    })?;

    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes);
        return Err(crate::Error::msg(enrich_oauth_failure(
            format!("device_authorization failed: HTTP {status} {text}"),
            &resolved.base,
        )));
    }

    let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
    serde_json::from_slice::<DeviceAuthorizationDetails>(&bytes).map_err(|e| {
        crate::Error::msg(format!(
            "device_authorization: parse OAuth response JSON: {e}; body prefix: {preview}"
        ))
    })
}

/// OAuth2 device authorization: write QR under OpenClaw workspace; on stdout print path, `MEDIA:`, and `VERIFICATION_CODE:` lines, then poll (detached or in-process).
/// With `verbose`, prints OAuth endpoints and response metadata on stderr (no access_token body).
pub async fn login(
    home: &std::path::Path,
    resolved: &ResolvedAuth,
    verbose: bool,
    options: LoginOptions,
) -> Result<()> {
    if verbose {
        eprintln!("[verbose] OAuth configuration:");
        eprintln!("  auth_base: {}", resolved.base);
        eprintln!("  device_authorization_url: {}", resolved.device_authorization_url);
        eprintln!("  token_url: {}", resolved.token_url);
        eprintln!("  client_id: {}", resolved.client_id);
        eprintln!("  app_name / device_name: {} / {}", resolved.app_name, resolved.device_name);
    }

    let device_sn = crate::storage::ensure_device_sn(home)?;
    let details = request_device_authorization(resolved, &device_sn, verbose).await?;

    if verbose {
        eprintln!("[verbose] device authorization HTTP response (parsed):");
        eprintln!("  verification_uri: {}", details.verification_uri().as_str());
        eprintln!("  user_code: {}", details.user_code().secret());
        eprintln!("  expires_in: {:?}", details.expires_in());
        eprintln!("  interval: {:?}", details.interval());
        let dc = details.device_code().secret();
        eprintln!("  device_code: <redacted, {} bytes>", dc.len());
        match details.extra_fields().qr_code.as_ref().filter(|s| !s.is_empty()) {
            Some(q) => eprintln!("  qr_code: <present, {} chars>", q.len()),
            None => eprintln!("  qr_code: <absent>"),
        }
    }

    let verification_code = details.user_code().secret().to_string();

    let png = login_qr_png_bytes(&details)?;

    if options.detach_poll {
        let qr_path = write_login_qr_png_file(&png, verbose)?;
        let artifact_dir = qr_path
            .parent()
            .ok_or_else(|| crate::Error::msg("login QR path has no parent directory"))?;
        let state_path = write_poll_state(home, resolved, &details, artifact_dir)?;
        let qr_abs = qr_path.display();
        println!("{qr_abs}");
        println!("MEDIA:{qr_abs}");
        println!("VERIFICATION_CODE:{verification_code}");
        let result_json = artifact_dir.join("result.json");
        eprint_login_qr_user_instructions(&details, &result_json);

        spawn_poll_child(&state_path, artifact_dir)?;
    } else {
        if verbose {
            eprintln!("[verbose] Polling token_url in-process (detach_poll=false)…");
        }
        let stored = exchange_device_token(home, resolved, &details, verbose).await?;
        print_login_success_banner(home, stored.expires_at);
    }

    Ok(())
}

/// Redirect poll child stderr to a session file so the parent process's stderr pipe (e.g. from an
/// agent/tool runner) is not held open until the poll finishes — otherwise the host may wait
/// indefinitely for EOF on stderr.
fn spawn_poll_child(state_path: &Path, artifact_dir: &Path) -> Result<()> {
    let log_path = artifact_dir.join("poll.log");
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| {
            crate::Error::msg(format!(
                "open poll log {}: {e}",
                log_path.display()
            ))
        })?;
    let exe = std::env::current_exe()
        .map_err(|e| crate::Error::msg(format!("current_exe: {e}")))?;
    let mut cmd = Command::new(exe);
    cmd.arg("wallet")
        .arg("login")
        .arg("--poll-state")
        .arg(state_path);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::from(log_file));
    cmd.spawn()
        .map_err(|e| crate::Error::msg(format!("spawn background login poll: {e}")))?;
    Ok(())
}

/// POST `/oauth/token` body — `OAuthDeviceTokenRequest` JSON (`grant_type`, `device_code`, `client_id`).
#[derive(Serialize)]
struct DeviceTokenRequestBody<'a> {
    grant_type: &'a str,
    device_code: &'a str,
    client_id: &'a str,
}

#[derive(Debug, Deserialize)]
struct TokenEndpointSuccess {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenErrorBody {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Some stacks return `TypedResult` (`code` / `msg`) instead of RFC 6749 `error` on the token endpoint.
#[derive(Debug, Deserialize)]
struct TypedResultLike {
    #[serde(default)]
    code: Option<i64>,
    #[serde(default)]
    msg: Option<String>,
}

enum NonSuccessPoll {
    Pending,
    SlowDown,
    Fatal(String),
}

/// RFC 8628 / OAuth `error` values, plus TypedResult `msg` that still means "keep polling".
#[derive(Debug, Clone, Copy)]
enum PollHint {
    Pending,
    SlowDown,
}

/// True when the body still means authorization in progress (HTTP 200 or 4xx), not a final failure.
fn poll_hint_from_token_body(bytes: &[u8]) -> Option<PollHint> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        match err {
            "authorization_pending" => return Some(PollHint::Pending),
            "slow_down" => return Some(PollHint::SlowDown),
            _ => {}
        }
    }
    let msg = v
        .get("msg")
        .or_else(|| v.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let lower = msg.to_ascii_lowercase();
    if lower.contains("authorization_pending") {
        return Some(PollHint::Pending);
    }
    if lower.contains("slow_down") {
        return Some(PollHint::SlowDown);
    }
    None
}

fn parse_token_success_body(bytes: &[u8]) -> Result<TokenEndpointSuccess> {
    if let Ok(t) = serde_json::from_slice::<TokenEndpointSuccess>(bytes) {
        return Ok(t);
    }
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| {
        crate::Error::msg(format!("token success JSON: {e}; prefix: {}", utf8_prefix(bytes, 200)))
    })?;
    // TypedResult: { "code": 0, "data": { "access_token": ... } }
    if let Some(data) = v.get("data") {
        if data.is_object() && data.get("access_token").is_some() {
            return serde_json::from_value(data.clone()).map_err(|e| {
                crate::Error::msg(format!("token success data: {e}; prefix: {}", utf8_prefix(bytes, 200)))
            });
        }
    }
    Err(crate::Error::msg(format!(
        "token response: expected access_token; prefix: {}",
        utf8_prefix(bytes, 240)
    )))
}

/// If server returns HTTP 200 with TypedResult-shaped business error (code != 0).
fn typed_result_business_error(bytes: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let code = v.get("code")?.as_i64()?;
    if code == 0 || code == 200 {
        return None;
    }
    let msg = v
        .get("msg")
        .or_else(|| v.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("error");
    Some(format!("server code {code}: {msg}"))
}

fn utf8_prefix(bytes: &[u8], max: usize) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(max)]).into_owned()
}

fn eprintln_token_poll_request(
    round: u32,
    attempt_label: &str,
    token_url: &str,
    content_type: &str,
    grant_type: &str,
    device_code_len: usize,
    client_id: &str,
) {
    let label = if attempt_label.is_empty() {
        String::new()
    } else {
        format!(" {attempt_label}")
    };
    eprintln!("[tcli] oauth token poll #{round}{label} POST {token_url}");
    if let Ok(u) = token_url.parse::<url::Url>() {
        if let Some(q) = u.query() {
            if !q.is_empty() {
                eprintln!("[tcli]   URL query: {q}");
            }
        }
    }
    eprintln!("[tcli]   Content-Type: {content_type}");
    eprintln!(
        "[tcli]   body: grant_type={grant_type} client_id={client_id} device_code=<{device_code_len} bytes omitted>"
    );
}

fn eprintln_token_poll_response(status: reqwest::StatusCode) {
    eprintln!("[tcli]   <- HTTP {}", status);
}

/// Maps body when HTTP status is not OK (or non-OAuth 200). Returns None if we should fall through to raw error.
fn interpret_token_error_body(bytes: &[u8]) -> Option<NonSuccessPoll> {
    if let Some(hint) = poll_hint_from_token_body(bytes) {
        return Some(match hint {
            PollHint::Pending => NonSuccessPoll::Pending,
            PollHint::SlowDown => NonSuccessPoll::SlowDown,
        });
    }
    if let Ok(e) = serde_json::from_slice::<OAuthTokenErrorBody>(bytes) {
        return Some(match e.error.as_str() {
            "authorization_pending" => NonSuccessPoll::Pending,
            "slow_down" => NonSuccessPoll::SlowDown,
            other => NonSuccessPoll::Fatal(format!(
                "{other}{}",
                e.error_description
                    .map(|d| format!(": {d}"))
                    .unwrap_or_default()
            )),
        });
    }
    if let Ok(t) = serde_json::from_slice::<TypedResultLike>(bytes) {
        if let Some(msg) = t.msg.filter(|m| !m.is_empty()) {
            let code_str = t
                .code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Some(NonSuccessPoll::Fatal(format!("server code {code_str}: {msg}")));
        }
    }
    None
}

fn expires_at_from_token_json(expires_in: Option<u64>) -> Option<i64> {
    let secs = expires_in?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|x| x.as_secs() as i64)
        .unwrap_or(0);
    Some(now + secs as i64)
}

async fn exchange_device_token(
    home: &Path,
    resolved: &ResolvedAuth,
    details: &DeviceAuthorizationDetails,
    verbose: bool,
) -> Result<OAuthStored> {
    let http = crate::oauth_http::shared_oauth_reqwest_client();
    let token_url = resolved.token_url.as_str();
    let device_code = details.device_code().secret();
    let client_id = resolved.client_id.as_str();

    let expires_in = details.expires_in();
    let deadline = Instant::now() + expires_in;

    let mut interval = details.interval();
    if interval < Duration::from_secs(1) {
        interval = Duration::from_secs(1);
    }
    let interval_secs = interval.as_secs().max(1);
    let expires_secs = expires_in.as_secs();

    let total_min = std::cmp::max(1u64, expires_secs.saturating_add(59) / 60);
    eprintln!();
    eprintln!(
        "Still waiting for you to approve in the app… checking about every {} second(s), for up to {} minute(s) in total.",
        interval_secs, total_min
    );
    eprintln!();

    if verbose {
        eprintln!("[verbose] Polling token_url until authorization completes…");
    }

    let device_code_len = device_code.len();
    let mut poll_round: u32 = 0;

    loop {
        if Instant::now() >= deadline {
            return Err(crate::Error::msg(enrich_oauth_failure(
                "token polling: device session expired (device_authorization expires_in elapsed)"
                    .to_string(),
                &resolved.base,
            )));
        }

        poll_round += 1;

        // JSON body: `grant_type`, `device_code`, `client_id` (OAuth device token request).
        let body = DeviceTokenRequestBody {
            grant_type: DEVICE_GRANT_TYPE,
            device_code,
            client_id,
        };

        eprintln_token_poll_request(
            poll_round,
            "",
            token_url,
            "application/json; charset=utf-8",
            DEVICE_GRANT_TYPE,
            device_code_len,
            client_id,
        );

        let resp = http
            .post(token_url)
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send()
            .await
            .map_err(|e: reqwest::Error| {
                crate::Error::msg(enrich_oauth_failure(
                    format!("token request (json): {}", format_err_chain(&e)),
                    &resolved.base,
                ))
            })?;

        let status = resp.status();
        eprintln_token_poll_response(status);
        let bytes = resp
            .bytes()
            .await
            .map_err(|e: reqwest::Error| crate::Error::msg(format!("token response body: {e}")))?
            .to_vec();

        if status == reqwest::StatusCode::BAD_REQUEST {
            eprintln!(
                "[tcli]   <- HTTP 400 body: {}",
                utf8_prefix(&bytes, 8192)
            );
        }

        if status.is_success() {
            if let Some(hint) = poll_hint_from_token_body(&bytes) {
                match hint {
                    PollHint::Pending => {
                        if verbose {
                            eprintln!("[verbose] token: authorization_pending (continue polling)");
                        }
                        tokio::time::sleep(interval).await;
                        continue;
                    }
                    PollHint::SlowDown => {
                        if verbose {
                            eprintln!("[verbose] token: slow_down (backing off)");
                        }
                        interval += Duration::from_secs(5);
                        tokio::time::sleep(interval).await;
                        continue;
                    }
                }
            }
            if let Some(biz_err) = typed_result_business_error(&bytes) {
                return Err(crate::Error::msg(enrich_oauth_failure(biz_err, &resolved.base)));
            }
            let parsed = parse_token_success_body(&bytes).map_err(|e| {
                crate::Error::msg(enrich_oauth_failure(
                    format!("{e}"),
                    &resolved.base,
                ))
            })?;
            if verbose {
                eprintln!("[verbose] token HTTP 200 (secrets omitted):");
                eprintln!(
                    "  token_type: {:?}; expires_in: {:?}; access_token: <{} chars>",
                    parsed.token_type,
                    parsed.expires_in,
                    parsed.access_token.len()
                );
            }
            let stored = OAuthStored {
                access_token: parsed.access_token,
                token_type: parsed.token_type,
                expires_at: expires_at_from_token_json(parsed.expires_in),
            };
            save_oauth(home, &stored)?;
            return Ok(stored);
        }

        if let Some(action) = interpret_token_error_body(&bytes) {
            match action {
                NonSuccessPoll::Pending => {
                    tokio::time::sleep(interval).await;
                    continue;
                }
                NonSuccessPoll::SlowDown => {
                    interval += Duration::from_secs(5);
                    tokio::time::sleep(interval).await;
                    continue;
                }
                NonSuccessPoll::Fatal(msg) => {
                    return Err(crate::Error::msg(enrich_oauth_failure(msg, &resolved.base)));
                }
            }
        }

        return Err(crate::Error::msg(enrich_oauth_failure(
            format!(
                "token endpoint HTTP {}: {}",
                status,
                utf8_prefix(&bytes, 500)
            ),
            &resolved.base,
        )));
    }
}

/// Same column alignment as `tempo wallet login` success output; `—` = not available in tcli (OAuth demo).
fn print_login_success_banner(home: &std::path::Path, expires_at: Option<i64>) {
    let expires_str = format_expires_human(expires_at);
    let token_path = oauth_path(home).display().to_string();

    println!("Wallet connected!");
    println!();
    println!("    Wallet: —");
    println!("   Balance: —");
    println!();
    println!("       Key: —");
    println!("     Chain: —");
    println!("   Expires: {expires_str}");
    println!("     Limit: —");
    println!();
    println!("tcli: OAuth demo only (not Tempo passkey / USDC). Token: {token_path}");
}

fn print_login_success_banner_stderr(expires_at: Option<i64>) {
    let expires_str = format_expires_human(expires_at);
    eprintln!();
    eprintln!("    Wallet: —");
    eprintln!("   Balance: —");
    eprintln!();
    eprintln!("       Key: —");
    eprintln!("     Chain: —");
    eprintln!("   Expires: {expires_str}");
    eprintln!("     Limit: —");
    eprintln!();
}

/// Roughly matches Tempo style `29d 23h` from stored expiry.
fn format_expires_human(expires_at: Option<i64>) -> String {
    let Some(ts) = expires_at else {
        return "—".to_string();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|x| x.as_secs() as i64)
        .unwrap_or(0);
    let rem = (ts - now).max(0);
    let days = rem / 86400;
    let hours = (rem % 86400) / 3600;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        let mins = (rem % 3600) / 60;
        format!("{hours}h {mins}m")
    } else {
        let mins = rem / 60;
        if mins < 1 {
            "<1m".to_string()
        } else {
            format!("{mins}m")
        }
    }
}

/// Friendly stderr after stdout lines: path, `MEDIA:…`, `VERIFICATION_CODE:…` (uses server `interval` / `expires_in`).
fn eprint_login_qr_user_instructions(
    details: &DeviceAuthorizationDetails,
    result_json: &Path,
) {
    let mut interval = details.interval();
    if interval < Duration::from_secs(1) {
        interval = Duration::from_secs(1);
    }
    let interval_secs = interval.as_secs().max(1);
    let expires_secs = details.expires_in().as_secs();
    let total_min = std::cmp::max(1u64, expires_secs.saturating_add(59) / 60);

    eprintln!();
    eprintln!("Scan the QR code with your phone to sign in to tcli.");
    eprintln!("Match the verification code to what you see in the app (also printed on stdout as VERIFICATION_CODE:…).");
    eprintln!();
    eprintln!(
        "After you approve in the app, login completes automatically. We check about every {} second(s), for up to {} minute(s) in total.",
        interval_secs, total_min
    );
    eprintln!(
        "Stdout has the QR path, MEDIA: line, and VERIFICATION_CODE: line. When finished, read this file once: {}",
        result_json.display()
    );
    eprintln!();
}

/// Builds the PNG bytes written to `login_qr.png`.
///
/// When `qr_code` is present: **http(s) URL** → encode that URL into a QR matrix; **PNG base64** (incl. `data:…;base64,`) → use decoded bytes; **any other string** (e.g. `redotpay:…` business URI for Android/App) → encode that string into the QR matrix.
/// When absent: embed `verification_uri_complete` or `verification_uri` (unchanged).
fn login_qr_png_bytes(details: &DeviceAuthorizationDetails) -> Result<Vec<u8>> {
    let fallback_url = details
        .verification_uri_complete()
        .map(|u| u.secret().to_string())
        .unwrap_or_else(|| details.verification_uri().to_string());

    if let Some(raw) = details
        .extra_fields()
        .qr_code
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        // Web URLs: encode as QR (same as custom schemes, but checked first for clarity).
        if raw.starts_with("http://") || raw.starts_with("https://") {
            return encode_login_qr_png(raw);
        }
        // Optional: server-rendered PNG (legacy/alternate).
        if let Some(png) = try_decode_qr_code_png_bytes(raw) {
            return Ok(png);
        }
        // Business / deep-link URIs (`redotpay:…`, `myapp://…`, etc.): encode UTF-8 bytes into QR.
        return encode_login_qr_png(raw);
    }

    encode_login_qr_png(&fallback_url)
}

/// Decode `qr_code` when the server sends a PNG as base64 (with or without a `data:` URL prefix).
fn try_decode_qr_code_png_bytes(raw: &str) -> Option<Vec<u8>> {
    let s = raw.trim();
    let b64_payload = if let Some(i) = s.find("base64,") {
        s[i + "base64,".len()..].trim()
    } else {
        s
    };
    let b64_payload: String = b64_payload
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let bytes = STANDARD
        .decode(b64_payload.as_bytes())
        .or_else(|_| URL_SAFE_NO_PAD.decode(b64_payload.as_bytes()))
        .ok()?;
    if bytes.len() >= 8 && bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        Some(bytes)
    } else {
        None
    }
}

fn encode_login_qr_png(auth_url: &str) -> Result<Vec<u8>> {
    let qr = QrCode::new(auth_url.as_bytes())
        .map_err(|e| crate::Error::msg(format!("QR encode failed: {e}")))?;
    let luma = qr
        .render::<Luma<u8>>()
        .min_dimensions(120, 120)
        .max_dimensions(360, 360)
        .build();
    let mut png = Vec::new();
    image::DynamicImage::ImageLuma8(luma)
        .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|e| crate::Error::msg(format!("encode QR PNG: {e}")))?;
    Ok(png)
}

#[cfg(test)]
mod token_poll_tests {
    use super::*;

    #[test]
    fn poll_hint_oauth_error_pending() {
        let b = br#"{"error":"authorization_pending"}"#;
        assert!(matches!(
            poll_hint_from_token_body(b),
            Some(PollHint::Pending)
        ));
    }

    #[test]
    fn poll_hint_typed_msg_contains_pending() {
        let b = br#"{"code":500,"msg":"authorization_pending","data":null}"#;
        assert!(matches!(
            poll_hint_from_token_body(b),
            Some(PollHint::Pending)
        ));
    }

    #[test]
    fn interpret_typed_fatal_uses_plain_code_not_debug_option() {
        let b = br#"{"code":500,"msg":"error"}"#;
        match interpret_token_error_body(b) {
            Some(NonSuccessPoll::Fatal(s)) => {
                assert!(s.contains("500"));
                assert!(!s.contains("Some("), "message was: {s}");
            }
            _other => panic!("expected Fatal, got unexpected variant"),
        }
    }

    #[test]
    fn try_decode_qr_code_accepts_raw_base64_png() {
        let png = encode_login_qr_png("https://example.com/z").unwrap();
        let b64 = STANDARD.encode(&png);
        assert_eq!(super::try_decode_qr_code_png_bytes(&b64).unwrap(), png);
    }

    #[test]
    fn try_decode_qr_code_accepts_data_url() {
        let png = encode_login_qr_png("x").unwrap();
        let b64 = STANDARD.encode(&png);
        let data = format!("data:image/png;base64,{b64}");
        assert_eq!(super::try_decode_qr_code_png_bytes(&data).unwrap(), png);
    }

    #[test]
    fn try_decode_qr_code_strips_whitespace_in_base64() {
        let png = encode_login_qr_png("https://example.com/a").unwrap();
        let b64 = STANDARD.encode(&png);
        let mut spaced = String::new();
        for (i, c) in b64.chars().enumerate() {
            if i > 0 && i % 40 == 0 {
                spaced.push('\n');
            }
            spaced.push(c);
        }
        assert_eq!(super::try_decode_qr_code_png_bytes(&spaced).unwrap(), png);
    }

    #[test]
    fn device_authorization_extra_fields_accepts_qr_code_camel_case() {
        let j = r#"{"qrCode":"https://example.com/deeplink"}"#;
        let e: super::DeviceAuthorizationExtraFields = serde_json::from_str(j).unwrap();
        assert_eq!(
            e.qr_code.as_deref(),
            Some("https://example.com/deeplink")
        );
    }

    #[test]
    fn encode_login_qr_accepts_redotpay_business_uri() {
        let png = encode_login_qr_png("redotpay:xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx").unwrap();
        assert!(png.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        assert!(png.len() > 64);
    }
}

