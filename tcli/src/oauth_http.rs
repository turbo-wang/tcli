//! OAuth2 HTTP transport: same behavior as `oauth2::reqwest::async_http_client`, but the client uses
//! [`reqwest::ClientBuilder::no_proxy`] so `HTTP_PROXY` / corporate proxies do not break localhost auth servers.

use std::sync::OnceLock;

use oauth2::http::{HeaderMap as OauthHeaderMap, HeaderName, HeaderValue, StatusCode as OauthStatusCode};
use oauth2::reqwest::Error as OAuthHttpError;
use oauth2::{HttpRequest, HttpResponse};

static OAUTH_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn oauth_reqwest_client() -> &'static reqwest::Client {
    OAUTH_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .expect("oauth reqwest client build")
    })
}

pub async fn async_http_client(
    request: HttpRequest,
) -> Result<HttpResponse, OAuthHttpError<reqwest::Error>> {
    let client = oauth_reqwest_client();
    // oauth2 uses `http` 0.2 types; reqwest 0.12 uses `http` 1.x — convert at the boundary.
    let method = reqwest::Method::from_bytes(request.method.as_str().as_bytes())
        .map_err(|_| OAuthHttpError::Other("invalid HTTP method".to_string()))?;
    let mut request_builder = client
        .request(method, request.url.as_str())
        .body(request.body);
    for (name, value) in &request.headers {
        request_builder = request_builder.header(name.as_str(), value.as_bytes());
    }
    let request = request_builder.build().map_err(OAuthHttpError::Reqwest)?;
    let response = client.execute(request).await.map_err(OAuthHttpError::Reqwest)?;
    let status_code = OauthStatusCode::from_u16(response.status().as_u16())
        .unwrap_or(OauthStatusCode::INTERNAL_SERVER_ERROR);
    let mut headers = OauthHeaderMap::new();
    for (name, value) in response.headers().iter() {
        let hn = HeaderName::from_bytes(name.as_str().as_bytes())
            .map_err(|e| OAuthHttpError::Other(format!("header name: {e}")))?;
        let hv =
            HeaderValue::from_bytes(value.as_bytes()).map_err(|e| OAuthHttpError::Other(format!("header value: {e}")))?;
        headers.insert(hn, hv);
    }
    let chunks = response.bytes().await.map_err(OAuthHttpError::Reqwest)?;
    Ok(HttpResponse {
        status_code,
        headers,
        body: chunks.to_vec(),
    })
}
