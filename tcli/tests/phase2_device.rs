//! Device flow against the Python mock (`mock_backend/auth_service/main.py`).
//! Base URL: `TCLI_TEST_MOCK_BASE` (default `http://127.0.0.1:8000`).

mod common;

use std::fs;
use std::path::PathBuf;

use common::{require_python_mock, test_mock_base_url};
use serial_test::serial;
use tcli::auth;
use tcli::config;
use tcli::LoginOptions;
use tcli::config_file::ConfigFile;
use tcli::storage::load_oauth;

#[tokio::test]
#[serial]
async fn wallet_login_oauth_device_flow() {
    require_python_mock().await;

    let tmp = tempfile::tempdir().unwrap();
    let home: PathBuf = tmp.path().join("tcli-home");
    fs::create_dir_all(&home).unwrap();

    let base = test_mock_base_url();
    std::env::set_var("TCLI_AUTH_BASE", &base);

    let cfg = ConfigFile::default();
    let resolved = config::resolve(&cfg).unwrap();

    auth::login(
        &home,
        &resolved,
        false,
        LoginOptions {
            detach_poll: false,
        },
    )
    .await
    .unwrap();

    let stored = load_oauth(&home).unwrap().expect("token saved");
    assert_eq!(stored.access_token, "demo-access-token");

    std::env::remove_var("TCLI_AUTH_BASE");
}
