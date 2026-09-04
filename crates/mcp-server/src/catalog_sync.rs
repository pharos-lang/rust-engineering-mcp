//! Explicit CLI-only HTTPS acquisition. Every returned byte remains untrusted.
use reqwest::{Client, ClientBuilder, Url};
use std::time::Duration;

const MAX_BYTES: usize = 80 * 1024 * 1024;
const MAX_URL_BYTES: usize = 2048;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const OVERALL_TIMEOUT: Duration = Duration::from_secs(60);
const USER_AGENT: &str = concat!("rust-engineering-mcp/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncError {
    Denied,
    Unavailable,
    RejectedResponse,
    Budget,
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never disclose URLs, queries, certificate details or network errors.
        write!(f, "catalog sync: {self:?}")
    }
}
impl std::error::Error for SyncError {}

/// A URL authorized by the trusted host, never by a runtime tool argument.
pub struct SyncSource {
    url: Url,
}

impl SyncSource {
    pub fn new(url: &str, allowed_host: &str) -> Result<Self, SyncError> {
        if url.len() > MAX_URL_BYTES
            || !url.is_ascii()
            || url
                .bytes()
                .any(|b| b.is_ascii_control() || b == b' ' || b == b'\\')
            || !canonical_hostname(allowed_host)
        {
            return Err(SyncError::Denied);
        }
        // Compare the original authority too: URL normalization must not make
        // uppercase, percent-encoded hosts, userinfo or numeric aliases acceptable.
        let rest = url.strip_prefix("https://").ok_or(SyncError::Denied)?;
        let authority = rest
            .split(['/', '?', '#'])
            .next()
            .ok_or(SyncError::Denied)?;
        if authority != allowed_host && authority.strip_suffix(":443") != Some(allowed_host) {
            return Err(SyncError::Denied);
        }
        let url = Url::parse(url).map_err(|_| SyncError::Denied)?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.port_or_known_default() != Some(443)
            || url.host_str() != Some(allowed_host)
            || allowed_host.parse::<std::net::IpAddr>().is_ok()
        {
            return Err(SyncError::Denied);
        }
        Ok(Self { url })
    }

    pub async fn fetch(&self) -> Result<Vec<u8>, SyncError> {
        let client = client_builder()
            .build()
            .map_err(|_| SyncError::Unavailable)?;
        fetch_response(&client, self.url.clone(), MAX_BYTES, OVERALL_TIMEOUT).await
    }
}

fn canonical_hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        })
}

fn client_builder() -> ClientBuilder {
    Client::builder()
        .use_rustls_tls()
        // Disable native roots even if another crate enables that reqwest feature.
        .tls_built_in_root_certs(false)
        .tls_built_in_webpki_certs(true)
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .retry(reqwest::retry::never())
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(OVERALL_TIMEOUT)
        .pool_max_idle_per_host(0)
}

fn request_error(error: reqwest::Error) -> SyncError {
    if error.is_timeout() {
        SyncError::Budget
    } else {
        SyncError::Unavailable
    }
}

// Private parameters allow tests to exercise the exact transport/reader against
// loopback TLS and a small byte budget; SyncSource cannot expose these overrides.
async fn fetch_response(
    client: &Client,
    url: Url,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Vec<u8>, SyncError> {
    tokio::time::timeout(timeout, async {
        let mut response = client
            .get(url)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(request_error)?;
        if response.status() != reqwest::StatusCode::OK
            || response
                .headers()
                .get_all(reqwest::header::CONTENT_ENCODING)
                .iter()
                .any(|v| v.as_bytes() != b"identity")
        {
            return Err(SyncError::RejectedResponse);
        }
        if response
            .content_length()
            .is_some_and(|size| size > max_bytes as u64)
        {
            return Err(SyncError::Budget);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if chunk.len() > max_bytes.saturating_sub(bytes.len()) {
                return Err(SyncError::Budget);
            }
            bytes
                .try_reserve(chunk.len())
                .map_err(|_| SyncError::Budget)?;
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| SyncError::Budget)?
}

#[cfg(test)]
mod tests;
