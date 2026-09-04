use super::*;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor, rustls};

type TestError = Box<dyn std::error::Error + Send + Sync>;
type ServerTask = tokio::task::JoinHandle<Result<(String, TcpListener), String>>;

fn runtime() -> Result<tokio::runtime::Runtime, std::io::Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

async fn fixture(
    response: Vec<u8>,
    delay: Duration,
) -> Result<(Client, Url, ServerTask), TestError> {
    fixture_delayed(response, delay, Duration::ZERO).await
}

async fn fixture_delayed(
    response: Vec<u8>,
    header_delay: Duration,
    body_delay: Duration,
) -> Result<(Client, Url, ServerTask), TestError> {
    let certs = CertificateDer::pem_slice_iter(include_bytes!("test-certs/chain.pem"))
        .collect::<Result<Vec<_>, _>>()?;
    let key = PrivateKeyDer::from_pem_slice(include_bytes!("test-certs/end.key"))?;
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certs, key)?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    // These overrides exist only inside cfg(test); production source validation
    // rejects the loopback address/port and the fixture CA is never installed.
    let client = client_builder()
        .tls_built_in_root_certs(false)
        .add_root_certificate(reqwest::Certificate::from_pem(include_bytes!(
            "test-certs/root.pem"
        ))?)
        .resolve("foobar.com", address)
        .build()?;
    let url = Url::parse(&format!("https://foobar.com:{}/bundle", address.port()))?;
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let mut stream = acceptor.accept(stream).await.map_err(|e| e.to_string())?;
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        while !request.windows(4).any(|part| part == b"\r\n\r\n") {
            let count = stream.read(&mut chunk).await.map_err(|e| e.to_string())?;
            if count == 0 || request.len() + count > 16 * 1024 {
                return Err("invalid fixture request".to_owned());
            }
            request.extend_from_slice(&chunk[..count]);
        }
        tokio::time::sleep(header_delay).await;
        let headers_end = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .ok_or("invalid fixture response")?
            + 4;
        // Clients may close immediately after rejecting status/length headers.
        let _ = stream.write_all(&response[..headers_end]).await;
        let _ = stream.flush().await;
        tokio::time::sleep(body_delay).await;
        let _ = stream.write_all(&response[headers_end..]).await;
        let _ = stream.shutdown().await;
        let request = String::from_utf8(request).map_err(|e| e.to_string())?;
        Ok((request, listener))
    });
    Ok((client, url, task))
}

#[test]
fn only_exact_canonical_https_authority_is_authorized() {
    for url in [
        "https://catalog.example",
        "https://catalog.example/a/b?channel=stable",
        "https://catalog.example:443/bundle",
    ] {
        assert!(SyncSource::new(url, "catalog.example").is_ok(), "{url}");
    }
    for (url, host) in [
        ("http://catalog.example/a", "catalog.example"),
        ("HTTPS://catalog.example/a", "catalog.example"),
        ("https://other.example/a", "catalog.example"),
        ("https://catalog.example.evil/a", "catalog.example"),
        ("https://catalog.example:444/a", "catalog.example"),
        ("https://catalog.example:0443/a", "catalog.example"),
        ("https://user:secret@catalog.example/a", "catalog.example"),
        ("https://@catalog.example/a", "catalog.example"),
        ("https://catalog.example/a#fragment", "catalog.example"),
        ("https://catalog.example/a#", "catalog.example"),
        ("https://CATALOG.example/a", "catalog.example"),
        ("https://catalog.example./a", "catalog.example."),
        ("https://catalog%2eexample/a", "catalog.example"),
        ("https://127.0.0.1/a", "127.0.0.1"),
        ("https://127.1/a", "127.1"),
        ("https://2130706433/a", "2130706433"),
        ("https://0x7f000001/a", "0x7f000001"),
        ("https://[::1]/a", "[::1]"),
        ("https://caf\u{e9}.example/a", "caf\u{e9}.example"),
        ("https://catalog..example/a", "catalog..example"),
        ("https://-catalog.example/a", "-catalog.example"),
        ("https://catalog-.example/a", "catalog-.example"),
        ("https://catalog.example/a\n", "catalog.example"),
        ("https://catalog.example/a b", "catalog.example"),
        ("https://catalog.example\\evil/a", "catalog.example"),
        ("https://catalog.example/a", "CATALOG.example"),
        ("https://catalog.example/a", ""),
    ] {
        assert!(
            matches!(SyncSource::new(url, host), Err(SyncError::Denied)),
            "{url}"
        );
    }
    assert!(matches!(
        SyncSource::new(
            &format!("https://catalog.example/{}", "a".repeat(MAX_URL_BYTES)),
            "catalog.example"
        ),
        Err(SyncError::Denied)
    ));
}

#[test]
fn tls_success_sends_only_expected_request_metadata() -> Result<(), TestError> {
    runtime()?.block_on(async {
        let (client, url, server) = fixture(
            b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nbundle!".to_vec(),
            Duration::ZERO,
        )
        .await?;
        assert_eq!(
            fetch_response(&client, url, 7, Duration::from_secs(2)).await?,
            b"bundle!"
        );
        let (request, listener) = server.await??;
        let lower = request.to_ascii_lowercase();
        assert!(request.starts_with("GET /bundle HTTP/1.1\r\n"));
        assert!(request.contains(&format!("user-agent: {USER_AGENT}\r\n")));
        assert!(lower.contains("accept-encoding: identity\r\n"));
        for unexpected in [
            "authorization:",
            "proxy-authorization:",
            "cookie:",
            "referer:",
        ] {
            assert!(!lower.contains(unexpected));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        Ok(())
    })
}

#[test]
fn redirects_never_connect_to_the_target() -> Result<(), TestError> {
    runtime()?.block_on(async {
        let target = TcpListener::bind("127.0.0.1:0").await?;
        let response = format!("HTTP/1.1 302 Found\r\nLocation: https://127.0.0.1:{}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", target.local_addr()?.port());
        let (client, url, server) = fixture(response.into_bytes(), Duration::ZERO).await?;
        assert_eq!(fetch_response(&client, url, 16, Duration::from_secs(2)).await, Err(SyncError::RejectedResponse));
        let (_, listener) = server.await??;
        assert!(tokio::time::timeout(Duration::from_millis(50), target.accept()).await.is_err());
        assert!(tokio::time::timeout(Duration::from_millis(50), listener.accept()).await.is_err());
        Ok(())
    })
}

#[test]
fn streamed_and_declared_lengths_obey_the_same_budget() -> Result<(), TestError> {
    runtime()?.block_on(async {
        for response in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\n".as_slice(),
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n0123456789abcdefg".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n01234567\r\n9\r\n89abcdefg\r\n0\r\n\r\n".as_slice(),
        ] {
            let (client, url, server) = fixture(response.to_vec(), Duration::ZERO).await?;
            assert_eq!(fetch_response(&client, url, 16, Duration::from_secs(2)).await, Err(SyncError::Budget));
            server.await??;
        }
        Ok(())
    })
}

#[test]
fn encoded_partial_and_error_responses_are_rejected_without_retry() -> Result<(), TestError> {
    runtime()?.block_on(async {
        for headers in [
            "200 OK\r\nContent-Encoding: gzip",
            "206 Partial Content",
            "503 Service Unavailable\r\nRetry-After: 0",
        ] {
            let response =
                format!("HTTP/1.1 {headers}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let (client, url, server) = fixture(response.into_bytes(), Duration::ZERO).await?;
            assert_eq!(
                fetch_response(&client, url, 16, Duration::from_secs(2)).await,
                Err(SyncError::RejectedResponse)
            );
            let (_, listener) = server.await??;
            assert!(
                tokio::time::timeout(Duration::from_millis(50), listener.accept())
                    .await
                    .is_err()
            );
        }
        Ok(())
    })
}

#[test]
fn untrusted_tls_and_elapsed_deadline_fail_without_bytes() -> Result<(), TestError> {
    runtime()?.block_on(async {
        let (_, url, server) = fixture(
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
            Duration::ZERO,
        )
        .await?;
        let address =
            std::net::SocketAddr::from(([127, 0, 0, 1], url.port().ok_or("test port missing")?));
        let untrusted = client_builder().resolve("foobar.com", address).build()?;
        assert_eq!(
            fetch_response(&untrusted, url, 16, Duration::from_secs(2)).await,
            Err(SyncError::Unavailable)
        );
        assert!(server.await?.is_err());
        for (headers, body) in [
            (Duration::from_millis(200), Duration::ZERO),
            (Duration::ZERO, Duration::from_millis(200)),
        ] {
            let (client, url, server) = fixture_delayed(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
                headers,
                body,
            )
            .await?;
            assert_eq!(
                fetch_response(&client, url, 16, Duration::from_millis(25)).await,
                Err(SyncError::Budget)
            );
            server.abort();
            let _ = server.await;
        }
        Ok(())
    })
}
