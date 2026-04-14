use oauth2::basic::BasicTokenType;
use oauth2::devicecode::StandardDeviceAuthorizationResponse;
use oauth2::reqwest::async_http_client;
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, DeviceAuthorizationUrl, EmptyExtraTokenFields, Scope,
    StandardTokenResponse, TokenResponse, TokenUrl,
};

use crate::config::ResolvedAuth;
use crate::storage::{OAuthStored, oauth_path, save_oauth};
use crate::Result;

/// Run OAuth2 device authorization + polling; persist tokens to disk.
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

    println!("Auth URL: {auth_url_line}");
    println!("Verification code: {verification_code}");
    println!();
    println!("Waiting for authentication...");
    println!();

    if let Some(uri) = details.verification_uri_complete() {
        let _ = webbrowser::open(uri.secret().as_str());
    }

    let _spinner_guard = if !verbose {
        let spinner = indicatif::ProgressBar::new_spinner();
        spinner.set_message("");
        spinner.enable_steady_tick(std::time::Duration::from_millis(120));
        Some(spinner)
    } else {
        eprintln!("[verbose] Polling token_url until authorization completes…");
        None
    };

    let token_res: StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType> = client
        .exchange_device_access_token(&details)
        .request_async(
            async_http_client,
            |d| tokio::time::sleep(d),
            Some(details.expires_in()),
        )
        .await
        .map_err(|e| crate::Error::msg(format!("token polling failed: {e}")))?;

    if let Some(s) = &_spinner_guard {
        s.finish_and_clear();
    }

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
