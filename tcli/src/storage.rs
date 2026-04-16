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

/// Stable device id for OAuth Device `deviceSn` (persisted under `home/device_sn`).
pub fn ensure_device_sn(home: &Path) -> Result<String> {
    let p = home.join("device_sn");
    if p.exists() {
        let s = fs::read_to_string(&p)?;
        let t = s.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    let x = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ ((std::process::id() as u64) << 32);
    let sn = format!("tcli-{x:016x}");
    fs::write(&p, sn.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    Ok(sn)
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

/// OpenClaw default workspace root (`~/.openclaw/workspace`), where agent tools can read files.
pub fn openclaw_workspace_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw")
        .join("workspace")
}

/// Unique directory name for one `wallet login` attempt (`time_nanos`-`pid`).
pub fn new_login_qr_session_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos}-{}", std::process::id())
}

/// `~/.openclaw/workspace/tcli-login/<session>/login_qr.png` for a given [`new_login_qr_session_id`].
pub fn openclaw_login_qr_png_path_for_session(session: &str) -> PathBuf {
    openclaw_workspace_dir()
        .join("tcli-login")
        .join(session)
        .join("login_qr.png")
}

/// Fallback when OpenClaw workspace is not writable: `<TCLI_HOME>/workspace/tcli-login/<session>/login_qr.png`.
pub fn tcli_workspace_login_qr_png_path(home: &Path, session: &str) -> PathBuf {
    home.join("workspace")
        .join("tcli-login")
        .join(session)
        .join("login_qr.png")
}

/// PNG path for the current `wallet login`: a fresh directory under the OpenClaw workspace each time.
///
/// Layout: `~/.openclaw/workspace/tcli-login/<session>/login_qr.png` (`session` = time + pid).
/// OAuth/token data stays under [`tcli_home`]; only this image is placed for OpenClaw to display.
pub fn openclaw_login_qr_png_path() -> PathBuf {
    openclaw_login_qr_png_path_for_session(&new_login_qr_session_id())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn openclaw_login_qr_lives_under_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());
        let p = openclaw_login_qr_png_path();
        let w = openclaw_workspace_dir();
        std::env::remove_var("HOME");
        assert!(p.starts_with(w.join("tcli-login")));
        assert!(p.ends_with("login_qr.png"));
    }

    #[test]
    #[serial]
    fn openclaw_login_qr_unique_session_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());
        let a = openclaw_login_qr_png_path();
        let b = openclaw_login_qr_png_path();
        std::env::remove_var("HOME");
        assert_ne!(a.parent(), b.parent());
    }
}
