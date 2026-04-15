use serde::Deserialize;
use std::path::PathBuf;

use crate::Result;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub auth: AuthSection,
    #[serde(default)]
    pub payment_token: PaymentTokenSection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthSection {
    #[serde(default = "default_auth_base")]
    pub base: String,
    #[serde(default = "default_client_id")]
    pub client_id: String,
    #[serde(default = "default_device_path")]
    pub device_authorization_path: String,
    #[serde(default = "default_token_path")]
    pub token_path: String,
    /// `appName` in POST /api/v1/oauth/device_authorization (see PAY_AUTHORIZATION_AND_OAUTH_DEVICE_API.md).
    #[serde(default = "default_app_name")]
    pub app_name: String,
    /// `deviceName` in the same request.
    #[serde(default = "default_device_name")]
    pub device_name: String,
    /// Optional `scope` on device authorization (omit from JSON when unset).
    #[serde(default)]
    pub oauth_scope: Option<String>,
}

fn default_auth_base() -> String {
    "https://app.rp-2023app.com".to_string()
}

fn default_client_id() -> String {
    "tcli".to_string()
}

fn default_device_path() -> String {
    "/api/v1/oauth/device_authorization".to_string()
}

fn default_token_path() -> String {
    "/api/v1/oauth/token".to_string()
}

fn default_app_name() -> String {
    "tcli".to_string()
}

fn default_device_name() -> String {
    "tcli-device".to_string()
}

impl Default for AuthSection {
    fn default() -> Self {
        Self {
            base: default_auth_base(),
            client_id: default_client_id(),
            device_authorization_path: default_device_path(),
            token_path: default_token_path(),
            app_name: default_app_name(),
            device_name: default_device_name(),
            oauth_scope: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaymentTokenSection {
    /// Full URL override for POST issue-token (optional).
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub disable: bool,
}

impl Default for PaymentTokenSection {
    fn default() -> Self {
        Self {
            url: None,
            disable: false,
        }
    }
}

pub fn load(path: &PathBuf) -> Result<ConfigFile> {
    if !path.exists() {
        return Ok(ConfigFile::default());
    }
    let raw = std::fs::read_to_string(path)?;
    let cfg: ConfigFile = toml::from_str(&raw)?;
    Ok(cfg)
}
