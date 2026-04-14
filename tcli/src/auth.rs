use std::sync::Arc;

use base64::Engine;
use image::ImageFormat;
use image::Luma;
use oauth2::basic::BasicTokenType;
use oauth2::devicecode::StandardDeviceAuthorizationResponse;
use oauth2::reqwest::async_http_client;
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, DeviceAuthorizationUrl, EmptyExtraTokenFields, Scope,
    StandardTokenResponse, TokenResponse, TokenUrl,
};
use qrcode::QrCode;

use crate::config::ResolvedAuth;
use crate::storage::{OAuthStored, oauth_path, save_oauth};
use crate::Result;

/// Run OAuth2 device authorization + polling; persist tokens to disk.
/// Serves a local page with a QR code (from device-authorization response) and opens it in the browser,
/// then polls the token endpoint until authorized.
/// With `verbose`, prints OAuth endpoints and response metadata on stderr (no access_token body).
pub async fn login(home: &std::path::Path, resolved: &ResolvedAuth, verbose: bool) -> Result<()> {
    let auth_url = AuthUrl::new(resolved.base.to_string())
        .map_err(|e| crate::Error::msg(format!("invalid auth URL: {e}")))?;
    let token_url = TokenUrl::new(resolved.token_url.to_string())
        .map_err(|e| crate::Error::msg(format!("invalid token URL: {e}")))?;
    let device_auth_url = DeviceAuthorizationUrl::new(resolved.device_authorization_url.to_string())
        .map_err(|e| crate::Error::msg(format!("invalid device authorization URL: {e}")))?;

    let client = BasicClient::new(
        ClientId::new(resolved.client_id.clone()),
        None,
        auth_url,
        Some(token_url),
    )
    .set_device_authorization_url(device_auth_url);

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
        .request_async(async_http_client)
        .await
        .map_err(|e| crate::Error::msg(format!("device authorization failed: {e}")))?;

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
    let html = login_qr_page_html(&verification_code, &auth_url_line, &png_b64);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| crate::Error::msg(format!("bind local login page server: {e}")))?;
    let addr = listener
        .local_addr()
        .map_err(|e| crate::Error::msg(format!("local_addr: {e}")))?;
    let local_page = format!("http://127.0.0.1:{}/", addr.port());

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let html = Arc::new(html);
    let html_get = html.clone();
    let app = axum::Router::new().route(
        "/",
        axum::routing::get(move || {
            let h = html_get.clone();
            async move { axum::response::Html((*h).clone()) }
        }),
    );

    let server_task = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    println!("Auth URL: {auth_url_line}");
    println!("Verification code: {verification_code}");
    println!();
    println!("Local login page (QR): {local_page}");
    let _ = webbrowser::open(&local_page);
    println!("Waiting for authentication (polling token endpoint)...");
    println!();

    let _spinner_guard = if !verbose {
        let spinner = indicatif::ProgressBar::new_spinner();
        spinner.set_message("");
        spinner.enable_steady_tick(std::time::Duration::from_millis(120));
        Some(spinner)
    } else {
        eprintln!("[verbose] Polling token_url until authorization completes…");
        None
    };

    let poll = client
        .exchange_device_access_token(&details)
        .request_async(
            async_http_client,
            |d| tokio::time::sleep(d),
            Some(details.expires_in()),
        )
        .await;

    let _ = shutdown_tx.send(());
    let _ = server_task.await;

    if let Some(s) = &_spinner_guard {
        s.finish_and_clear();
    }

    let token_res: StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType> =
        poll.map_err(|e| crate::Error::msg(format!("token polling failed: {e}")))?;

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

    print_login_success_banner(home, stored.expires_at);
    Ok(())
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

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn login_qr_page_html(verification_code: &str, auth_url: &str, png_base64: &str) -> String {
    let code_e = html_escape(verification_code);
    let url_e = html_escape(auth_url);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>tcli wallet login</title>
  <style>
    :root {{ --bg: #0d1117; --text: #e6edf3; --muted: #8b949e; --accent: #58a6ff; }}
    body {{
      font-family: ui-sans-serif, system-ui, sans-serif;
      background: var(--bg);
      color: var(--text);
      margin: 0;
      min-height: 100vh;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      padding: 1.5rem;
    }}
    .card {{
      max-width: 420px;
      text-align: center;
    }}
    h1 {{ font-size: 1.1rem; font-weight: 600; margin-bottom: 1rem; }}
    .code {{ font-size: 1.5rem; letter-spacing: 0.1em; color: var(--accent); margin: 0.5rem 0 1.25rem; }}
    a {{ color: var(--accent); }}
    p.muted {{ color: var(--muted); font-size: 0.9rem; margin-top: 1rem; }}
    img {{ display: block; margin: 0 auto; max-width: 100%; height: auto; }}
  </style>
</head>
<body>
  <div class="card">
    <h1>tcli — device login</h1>
    <p class="muted">Verification code</p>
    <div class="code">{code_e}</div>
    <p><a href="{url_e}" target="_blank" rel="noopener">Open verification link</a></p>
    <p class="muted">Scan QR (same as the link above)</p>
    <img src="data:image/png;base64,{png_base64}" width="320" height="320" alt="Login QR"/>
    <p class="muted">Keep this terminal running — tcli is polling the token endpoint in the background.</p>
  </div>
</body>
</html>"#
    )
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
