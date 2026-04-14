use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use base64::Engine;
use image::ImageFormat;
use image::Luma;
use oauth2::basic::BasicTokenType;
use oauth2::devicecode::StandardDeviceAuthorizationResponse;
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, DeviceAuthorizationUrl, EmptyExtraTokenFields, Scope,
    StandardTokenResponse, TokenResponse, TokenUrl,
};
use qrcode::QrCode;
use serde::{Deserialize, Serialize};

use crate::config::ResolvedAuth;
use crate::oauth_http;
use crate::storage::{OAuthStored, oauth_path, save_oauth};
use crate::Result;

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
             Hint: ensure the auth server is reachable at {auth_base} (repo mock: \
             `python3 mock_backend/auth_service/main.py`). \
             If you use HTTP_PROXY, set NO_PROXY=127.0.0.1,localhost,::1 for local OAuth."
        )
    } else {
        message
    }
}

/// How the device-flow token poll is run after the QR step.
#[derive(Debug, Clone, Copy)]
pub struct LoginOptions {
    /// When `true` (default for CLI), print one JSON line with the QR image and spawn a detached
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

const POLL_STATE_VERSION: u32 = 1;

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
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DevicePollState {
    version: u32,
    home: String,
    resolved: AuthResolvedSnapshot,
    device_authorization: StandardDeviceAuthorizationResponse,
}

fn poll_state_path(home: &Path) -> PathBuf {
    home.join("wallet").join(".device_login_poll.json")
}

fn write_poll_state(
    home: &Path,
    resolved: &ResolvedAuth,
    details: &StandardDeviceAuthorizationResponse,
) -> Result<PathBuf> {
    let wallet_dir = home.join("wallet");
    std::fs::create_dir_all(&wallet_dir).map_err(crate::Error::Io)?;
    let path = poll_state_path(home);
    let tmp = path.with_extension("json.tmp");
    let state = DevicePollState {
        version: POLL_STATE_VERSION,
        home: home.to_string_lossy().to_string(),
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
    let resolved = state.resolved.to_resolved()?;
    let stored =
        exchange_device_token(&home, &resolved, &state.device_authorization, verbose).await?;
    let _ = std::fs::remove_file(state_path);
    eprintln!("Wallet connected! Token: {}", oauth_path(&home).display());
    print_login_success_banner_stderr(stored.expires_at);
    Ok(())
}

fn build_client(resolved: &ResolvedAuth) -> Result<BasicClient> {
    let auth_url = AuthUrl::new(resolved.base.to_string())
        .map_err(|e| crate::Error::msg(format!("invalid auth URL: {e}")))?;
    let token_url = TokenUrl::new(resolved.token_url.to_string())
        .map_err(|e| crate::Error::msg(format!("invalid token URL: {e}")))?;
    let device_auth_url = DeviceAuthorizationUrl::new(resolved.device_authorization_url.to_string())
        .map_err(|e| crate::Error::msg(format!("invalid device authorization URL: {e}")))?;

    Ok(
        BasicClient::new(
            ClientId::new(resolved.client_id.clone()),
            None,
            auth_url,
            Some(token_url),
        )
        .set_device_authorization_url(device_auth_url),
    )
}

/// OAuth2 device authorization: emit QR as base64 JSON, then poll the token endpoint (inline or subprocess).
/// With `verbose`, prints OAuth endpoints and response metadata on stderr (no access_token body).
pub async fn login(
    home: &std::path::Path,
    resolved: &ResolvedAuth,
    verbose: bool,
    options: LoginOptions,
) -> Result<()> {
    let client = build_client(resolved)?;

    if verbose {
        eprintln!("[verbose] OAuth configuration:");
        eprintln!("  auth_base: {}", resolved.base);
        eprintln!("  device_authorization_url: {}", resolved.device_authorization_url);
        eprintln!("  token_url: {}", resolved.token_url);
        eprintln!("  client_id: {}", resolved.client_id);
        eprintln!(
            "[verbose] note: oauth2-rs uses its own HTTP client; raw request/response bytes are not available here."
        );
    }

    let details: StandardDeviceAuthorizationResponse = client
        .exchange_device_code()
        .map_err(|e| crate::Error::msg(format!("OAuth client misconfigured: {e}")))?
        .add_scope(Scope::new("openid".to_string()))
        .request_async(oauth_http::async_http_client)
        .await
        .map_err(|e| {
            crate::Error::msg(enrich_oauth_failure(
                format!("device authorization failed: {}", format_err_chain(&e)),
                &resolved.base,
            ))
        })?;

    if verbose {
        eprintln!("[verbose] device authorization HTTP response (parsed):");
        eprintln!("  verification_uri: {}", details.verification_uri().as_str());
        eprintln!("  user_code: {}", details.user_code().secret());
        eprintln!("  expires_in: {:?}", details.expires_in());
        eprintln!("  interval: {:?}", details.interval());
        let dc = details.device_code().secret();
        eprintln!("  device_code: <redacted, {} bytes>", dc.len());
    }

    let auth_url_line = details
        .verification_uri_complete()
        .map(|u| u.secret().to_string())
        .unwrap_or_else(|| details.verification_uri().to_string());
    let verification_code = details.user_code().secret().to_string();

    let png = encode_login_qr_png(&auth_url_line)?;
    let png_b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    let data_url = format!("data:image/png;base64,{png_b64}");

    if options.detach_poll {
        let state_path = write_poll_state(home, resolved, &details)?;
        let line = serde_json::json!({
            "qr_png_base64": png_b64,
            "qr_image_data_url": data_url,
            "verification_code": verification_code,
            "auth_url": auth_url_line,
        });
        println!("{}", line);

        spawn_poll_child(&state_path)?;
        eprintln!("Polling token endpoint in the background until login completes or expires.");
    } else {
        if verbose {
            eprintln!("[verbose] Polling token_url in-process (detach_poll=false)…");
        }
        let stored = exchange_device_token(home, resolved, &details, verbose).await?;
        print_login_success_banner(home, stored.expires_at);
    }

    Ok(())
}

fn spawn_poll_child(state_path: &Path) -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| crate::Error::msg(format!("current_exe: {e}")))?;
    let mut cmd = Command::new(exe);
    cmd.arg("wallet")
        .arg("login")
        .arg("--poll-state")
        .arg(state_path);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::inherit());
    cmd.spawn()
        .map_err(|e| crate::Error::msg(format!("spawn background login poll: {e}")))?;
    Ok(())
}

async fn exchange_device_token(
    home: &Path,
    resolved: &ResolvedAuth,
    details: &StandardDeviceAuthorizationResponse,
    verbose: bool,
) -> Result<OAuthStored> {
    let client = build_client(resolved)?;

    if verbose {
        eprintln!("[verbose] Polling token_url until authorization completes…");
    }

    let poll = client
        .exchange_device_access_token(details)
        .request_async(
            oauth_http::async_http_client,
            |d| tokio::time::sleep(d),
            Some(details.expires_in()),
        )
        .await;

    let token_res: StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType> =
        poll.map_err(|e| {
            crate::Error::msg(enrich_oauth_failure(
                format!("token polling failed: {}", format_err_chain(&e)),
                &resolved.base,
            ))
        })?;

    if verbose {
        eprintln!("[verbose] token HTTP response (parsed, secrets omitted):");
        eprintln!("  token_type: {}", token_res.token_type().as_ref());
        eprintln!("  expires_in: {:?}", token_res.expires_in());
        eprintln!(
            "  access_token: <{} chars, not printed>",
            token_res.access_token().secret().len()
        );
    }

    let access = token_res.access_token().secret().to_string();
    let stored = OAuthStored {
        access_token: access,
        token_type: Some(token_res.token_type().as_ref().to_string()),
        expires_at: token_expires_at(&token_res),
    };
    save_oauth(home, &stored)?;
    Ok(stored)
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

fn token_expires_at(
    tr: &StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType>,
) -> Option<i64> {
    tr.expires_in().map(|d: std::time::Duration| {
        let secs = d.as_secs() as i64;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|x| x.as_secs() as i64)
            .unwrap_or(0);
        now + secs
    })
}
