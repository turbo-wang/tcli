use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthStored {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    /// Unix timestamp when the access token expires (if known).
    #[serde(default)]
    pub expires_at: Option<i64>,
}

pub fn tcli_home() -> PathBuf {
    std::env::var("TCLI_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".tcli")
        })
}

pub fn oauth_path(home: &Path) -> PathBuf {
    home.join("wallet").join("oauth.json")
}

pub fn load_oauth(home: &Path) -> Result<Option<OAuthStored>> {
    let p = oauth_path(home);
    if !p.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&p)?;
    let v: OAuthStored = serde_json::from_str(&raw)?;
    Ok(Some(v))
}

pub fn save_oauth(home: &Path, data: &OAuthStored) -> Result<()> {
    let p = oauth_path(home);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(data)?;
    let mut f = fs::File::create(&p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(json.as_bytes())?;
    f.flush()?;
    Ok(())
}

pub fn remove_oauth(home: &Path) -> Result<()> {
    let p = oauth_path(home);
    if p.exists() {
        fs::remove_file(&p)?;
    }
    Ok(())
}
