//! Shared test helpers: integration tests talk to the Python mock (`mock_backend/auth_service/main.py`),
//! not random wiremock ports. Base URL is configurable.
//!
//! - **`TCLI_TEST_MOCK_BASE`**: root URL of the mock, default `http://127.0.0.1:8000` (no trailing slash).
//! - Start mock: `python3 mock_backend/auth_service/main.py`  
//!   Optional: `MOCK_AUTH_HOST`, `MOCK_AUTH_PORT` (see `main.py`).

/// Configurable mock server root (same host/port as Python `auth_service`).
pub fn test_mock_base_url() -> String {
    std::env::var("TCLI_TEST_MOCK_BASE")
        .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Fail fast with a clear message if the Python mock is not running.
pub async fn require_python_mock() {
    let base = test_mock_base_url();
    let url = format!("{base}/verify");
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .expect("client");
    match client.get(url).send().await {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => panic!(
            "Python mock at {base} returned HTTP {} (expected 200 on GET /verify)",
            r.status()
        ),
        Err(e) => panic!(
            "Cannot reach Python mock at {base} (set TCLI_TEST_MOCK_BASE if using another host/port).\n\
             Start the server from repo root:\n\
               python3 mock_backend/auth_service/main.py\n\
             Underlying error: {e}"
        ),
    }
}
