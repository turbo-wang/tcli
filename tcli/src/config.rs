use std::path::PathBuf;

use crate::config_file::ConfigFile;
use crate::Result;

/// Resolved OAuth / payment-token settings (env + file + defaults).
#[derive(Debug, Clone)]
pub struct ResolvedAuth {
    pub base: url::Url,
    pub client_id: String,
    pub device_authorization_url: url::Url,
    pub token_url: url::Url,
    pub payment_token_url: Option<url::Url>,
    pub payment_token_disabled: bool,
    pub app_name: String,
    pub device_name: String,
    pub oauth_scope: Option<String>,
}

fn join_base(base: &url::Url, path: &str) -> Result<url::Url> {
    let path = path.trim_start_matches('/');
    base
        .join(path)
        .map_err(|e| crate::Error::msg(format!("invalid URL join: {e}")))
}

pub fn resolve(cfg: &ConfigFile) -> Result<ResolvedAuth> {
    let base_str = std::env::var("TCLI_AUTH_BASE")
        .unwrap_or_else(|_| cfg.auth.base.clone());
    let base: url::Url = base_str
        .parse()
        .map_err(|e| crate::Error::msg(format!("TCLI_AUTH_BASE / [auth].base invalid: {e}")))?;

    let device_url = join_base(&base, &cfg.auth.device_authorization_path)?;
    let token_url = join_base(&base, &cfg.auth.token_path)?;

    let payment_token_disabled = cfg.payment_token.disable;
    let payment_token_url = if payment_token_disabled {
        None
    } else if let Some(ref u) = cfg.payment_token.url {
        Some(u.parse().map_err(|e: url::ParseError| crate::Error::from(e))?)
    } else {
        Some(join_base(&base, "issue-token")?)
    };

    Ok(ResolvedAuth {
        base,
        client_id: cfg.auth.client_id.clone(),
        device_authorization_url: device_url,
        token_url,
        payment_token_url,
        payment_token_disabled,
        app_name: cfg.auth.app_name.clone(),
        device_name: cfg.auth.device_name.clone(),
        oauth_scope: cfg.auth.oauth_scope.clone(),
    })
}

pub fn config_path(home: &PathBuf) -> PathBuf {
    home.join("config.toml")
}
