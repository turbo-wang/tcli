//! Try to open the login URL in a compact Chromium window (`--app` + `--window-size`), similar to Google OAuth popups.
//! Falls back to the system default browser when Chromium/Chrome/Edge is not available or launch fails.

use std::process::Command;

/// Approximate OAuth-popup dimensions (px).
const POPUP_W: u32 = 520;
const POPUP_H: u32 = 680;

pub fn open_login_page(url: &str) {
    if try_chromium_compact_window(url) {
        return;
    }
    let _ = webbrowser::open(url);
}

fn try_chromium_compact_window(url: &str) -> bool {
    if let Ok(custom) = std::env::var("TCLI_BROWSER") {
        if !custom.is_empty() && spawn_chromium_with_args(&custom, url) {
            return true;
        }
    }

    #[cfg(target_os = "macos")]
    if macos_open_chromium_app(url) {
        return true;
    }

    #[cfg(target_os = "windows")]
    if windows_chromium_app(url) {
        return true;
    }

    #[cfg(target_os = "linux")]
    if linux_chromium_app(url) {
        return true;
    }

    false
}

fn spawn_chromium_with_args(chrome_exe: &str, url: &str) -> bool {
    let status = Command::new(chrome_exe)
        .args([
            format!("--app={url}"),
            format!("--window-size={POPUP_W},{POPUP_H}"),
        ])
        .status();
    matches!(status, Ok(s) if s.success())
}

#[cfg(target_os = "macos")]
fn macos_open_chromium_app(url: &str) -> bool {
    let size = format!("--window-size={POPUP_W},{POPUP_H}");
    let app_arg = format!("--app={url}");
    for app in [
        "Google Chrome",
        "Microsoft Edge",
        "Brave Browser",
        "Chromium",
        "Google Chrome Canary",
    ] {
        let status = Command::new("open")
            .args(["-a", app, "--args", &app_arg, &size])
            .status();
        if matches!(status, Ok(s) if s.success()) {
            return true;
        }
    }
    false
}

#[cfg(target_os = "windows")]
fn windows_chromium_app(url: &str) -> bool {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
    let pf_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string());
    let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| String::new());

    let candidates = [
        format!(r"{pf}\Google\Chrome\Application\chrome.exe"),
        format!(r"{pf_x86}\Google\Chrome\Application\chrome.exe"),
        format!(r"{pf}\Microsoft\Edge\Application\msedge.exe"),
        format!(r"{local}\Google\Chrome\Application\chrome.exe"),
    ];

    for path in candidates {
        if std::path::Path::new(&path).is_file() && spawn_chromium_with_args(&path, url) {
            return true;
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn linux_chromium_app(url: &str) -> bool {
    for bin in [
        "google-chrome-stable",
        "google-chrome",
        "chromium",
        "chromium-browser",
        "microsoft-edge-stable",
        "microsoft-edge",
    ] {
        if spawn_chromium_with_args(bin, url) {
            return true;
        }
    }
    false
}
