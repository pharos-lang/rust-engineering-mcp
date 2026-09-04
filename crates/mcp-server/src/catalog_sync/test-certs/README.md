These public TLS test credentials were copied from the already cached official
`tokio-rustls 0.26.4` crate, `tests/certs/{root.pem,chain.pem,end.key}`.
Source: https://github.com/rustls/tokio-rustls/tree/v/0.26.4/tests/certs

They authenticate only the loopback test server using the `foobar.com` SAN and an
explicit test-only resolver override. They are never compiled into production,
are not secret, and must never be publisher trust roots or deployment identities.
The tests use certificate verification; they do not disable TLS verification.
