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
}

fn default_auth_base() -> String {
    "http://127.0.0.1:8000".to_string()
}

fn default_client_id() -> String {
    "tcli".to_string()
}

fn default_device_path() -> String {
    "/oauth/device".to_string()
}

fn default_token_path() -> String {
    "/oauth/token".to_string()
}

impl Default for AuthSection {
    fn default() -> Self {
        Self {
            base: default_auth_base(),
            client_id: default_client_id(),
            device_authorization_path: default_device_path(),
            token_path: default_token_path(),
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
