//! Shared [`reqwest::Client`] for OAuth-related HTTP calls (device + token).
//! Uses [`reqwest::ClientBuilder::no_proxy`] so `HTTP_PROXY` does not break local auth servers.

use std::sync::OnceLock;

static OAUTH_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Shared client (no redirect, no proxy through corporate HTTP_PROXY for localhost).
pub fn shared_oauth_reqwest_client() -> &'static reqwest::Client {
    OAUTH_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .expect("oauth reqwest client build")
    })
}
