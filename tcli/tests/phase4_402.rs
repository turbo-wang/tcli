//! 402 / MPP / x402 / payment-token against the Python mock (`/test/*` routes).
//! Base URL: `TCLI_TEST_MOCK_BASE` (default `http://127.0.0.1:8000`).

mod common;

use std::fs;
use std::path::PathBuf;

use common::{require_python_mock, test_mock_base_url};
use tcli::api::{run_request, RequestArgs};
use tcli::config::{self, config_path};
use tcli::config_file;
use tcli::config_file::ConfigFile;

fn req_args(url: &str) -> RequestArgs {
    RequestArgs {
        url: url.to_string(),
        method: None,
        json_body: None,
        data_pairs: vec![],
        headers: vec![],
        timeout_secs: Some(5),
        dry_run: false,
        max_spend: Some("1".to_string()),
        verbose: false,
        payment_token_header: None,
    }
}

#[tokio::test]
async fn mpp_www_authenticate_errors() {
    require_python_mock().await;
    let base = test_mock_base_url();
    let url = format!("{base}/test/mpp");

    let tmp = tempfile::tempdir().unwrap();
    let home: PathBuf = tmp.path().join("tcli-home");
    fs::create_dir_all(&home).unwrap();

    let mut cfg = ConfigFile::default();
    cfg.payment_token.disable = true;
    let resolved = config::resolve(&cfg).unwrap();

    let err = run_request(&home, &resolved, &req_args(&url))
        .await
        .err()
        .expect("MPP should error");
    let s = err.to_string();
    assert!(s.contains("tempo request") || s.contains("MPP"), "{s}");
}

#[tokio::test]
async fn x402_retry_with_session() {
    require_python_mock().await;
    let base = test_mock_base_url();
    let url = format!("{base}/test/x402");

    let tmp = tempfile::tempdir().unwrap();
    let home: PathBuf = tmp.path().join("tcli-home");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(home.join("wallet")).unwrap();

    let oauth_path = home.join("wallet").join("oauth.json");
    fs::write(
        &oauth_path,
        r#"{"access_token":"tok","token_type":"Bearer"}"#,
    )
    .unwrap();

    let cfg_path = config_path(&home);
    fs::write(
        &cfg_path,
        r#"
[auth]
base = "http://127.0.0.1:1"
[payment_token]
disable = true
"#,
    )
    .unwrap();
    let file_cfg = config_file::load(&cfg_path).unwrap();
    let resolved = config::resolve(&file_cfg).unwrap();

    run_request(&home, &resolved, &req_args(&url))
        .await
        .expect("x402 retry succeeds");
}

#[tokio::test]
async fn payment_token_issue_retry() {
    require_python_mock().await;
    let base = test_mock_base_url();
    let url = format!("{base}/test/paid");

    let tmp = tempfile::tempdir().unwrap();
    let home: PathBuf = tmp.path().join("tcli-home");
    fs::create_dir_all(&home).unwrap();

    let cfg_path = config_path(&home);
    fs::write(
        &cfg_path,
        format!(
            r#"
[auth]
base = "{base}"
[payment_token]
disable = false
"#
        ),
    )
    .unwrap();

    let file_cfg = config_file::load(&cfg_path).unwrap();
    let resolved = config::resolve(&file_cfg).unwrap();

    run_request(
        &home,
        &resolved,
        &RequestArgs {
            url: url.clone(),
            method: None,
            json_body: None,
            data_pairs: vec![],
            headers: vec![],
            timeout_secs: Some(5),
            dry_run: false,
            max_spend: None,
            verbose: false,
            payment_token_header: None,
        },
    )
    .await
    .expect("payment token retry");
}
