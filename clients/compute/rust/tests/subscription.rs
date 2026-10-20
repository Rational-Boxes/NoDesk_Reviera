// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! The subscription client against a mock service (DM-RPC-011): Basic credentials only to the
//! token endpoint, bearer tokens elsewhere, renewal, the error reasons, and certificate checks
//! (system store, an extra CA, a pin).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use base64::Engine;
use reviera_compute_proto::subscription::{self as sub, Client, Credentials, Endpoint};
use serde_json::json;

#[derive(Default)]
struct State {
    /// (method, path, authorization) of every request.
    seen: Vec<(String, String, String)>,
    issued: Vec<String>,
    revoked: Vec<String>,
    lifetime_s: i64,
    /// Overrides the subscription answer: (status, body).
    answer: Option<(u16, serde_json::Value)>,
}

struct Mock {
    port: u16,
    state: Arc<Mutex<State>>,
}

fn request(stream: &mut impl Read) -> Option<(String, String, String)> {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next()?.to_owned(), parts.next()?.to_owned());
    let (mut auth, mut length) = (String::new(), 0usize);
    loop {
        let mut h = String::new();
        r.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':')?;
        match k.to_ascii_lowercase().as_str() {
            "authorization" => auth = v.trim().to_owned(),
            "content-length" => length = v.trim().parse().unwrap_or(0),
            _ => {}
        }
    }
    let mut body = vec![0; length];
    r.read_exact(&mut body).ok()?;
    Some((method, path, auth))
}

fn answer(state: &Mutex<State>, (method, path, auth): (String, String, String)) -> (u16, serde_json::Value) {
    let mut s = state.lock().unwrap();
    s.seen.push((method.clone(), path.clone(), auth.clone()));
    match (method.as_str(), path.as_str()) {
        ("POST", "/api/v1/auth/token") => {
            let basic = auth.strip_prefix("Basic ").and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok());
            if basic.as_deref() != Some(b"ak_live_1:s3cret".as_slice()) {
                return (401, json!({"reason": "unauthorised", "message": "bad credentials"}));
            }
            let token = format!("tok-{}", s.issued.len() + 1);
            s.issued.push(token.clone());
            let expires_at = sub::format_time(sub::now() + s.lifetime_s);
            (200, json!({"access_token": token, "expires_at": expires_at, "subscription_id": "sub_42"}))
        }
        ("GET", "/api/v1/subscription") => {
            let token = auth.strip_prefix("Bearer ").unwrap_or("");
            if !s.issued.iter().any(|t| t == token) || s.revoked.iter().any(|t| t == token) {
                return (401, json!({"reason": "unauthorised"}));
            }
            s.answer.clone().unwrap_or((
                200,
                json!({
                    "name": "Acme Engineering", "plan": "Team 200", "state": "active",
                    "period_start": "2026-10-01T00:00:00Z", "period_end": "2026-11-01T00:00:00Z",
                    "allowance": 200.0, "usage": 37.5, "unit": "compute_hours",
                    "job_kinds": ["analysis.clash", "analysis.energy"], "regions": ["eu-west"],
                    "limits": {"max_job_bytes": 2_000_000_000u64, "concurrent_jobs": 4},
                    "terms_url": "https://analysis.example.com/terms"
                }),
            ))
        }
        _ => (404, json!({"reason": "not_found"})),
    }
}

fn respond(stream: &mut impl Write, (status, body): (u16, serde_json::Value)) {
    let body = body.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

/// A mock service; with a TLS configuration it serves HTTPS.
fn mock(tls: Option<Arc<rustls::ServerConfig>>) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = Arc::new(Mutex::new(State { lifetime_s: 3600, ..State::default() }));
    let st = state.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let st = st.clone();
            let tls = tls.clone();
            std::thread::spawn(move || match tls {
                None => {
                    if let Some(r) = request(&mut stream) {
                        let a = answer(&st, r);
                        respond(&mut stream, a);
                    }
                }
                Some(cfg) => {
                    let Ok(conn) = rustls::ServerConnection::new(cfg) else { return };
                    let mut s = rustls::StreamOwned::new(conn, stream);
                    if let Some(r) = request(&mut s) {
                        let a = answer(&st, r);
                        respond(&mut s, a);
                        s.conn.send_close_notify();
                        let _ = s.flush();
                    }
                }
            });
        }
    });
    Mock { port, state }
}

fn creds() -> Credentials {
    Credentials::new("ak_live_1", "s3cret")
}

fn dev(m: &Mock) -> Endpoint {
    Endpoint { url: format!("http://127.0.0.1:{}/api", m.port), development: true, ..Endpoint::default() }
}

#[test]
fn the_secret_goes_only_to_the_token_endpoint() {
    let m = mock(None);
    let c = Client::new(&dev(&m)).unwrap();
    let s = c.subscription(&creds()).unwrap();
    assert_eq!((s.plan.as_str(), s.state.as_str(), s.usage, s.limits.concurrent_jobs), ("Team 200", "active", 37.5, 4));
    c.subscription(&creds()).unwrap();
    let st = m.state.lock().unwrap();
    assert_eq!(st.issued.len(), 1, "a valid token is reused");
    let secret_b64 = base64::engine::general_purpose::STANDARD.encode("ak_live_1:s3cret");
    for (method, path, auth) in &st.seen {
        if path.ends_with("/auth/token") {
            assert_eq!((method.as_str(), auth.as_str()), ("POST", format!("Basic {secret_b64}").as_str()));
        } else {
            assert!(auth.starts_with("Bearer tok-") && !auth.contains("s3cret") && !auth.contains(&secret_b64), "{auth}");
        }
    }
}

#[test]
fn a_token_near_expiry_is_renewed() {
    let m = mock(None);
    m.state.lock().unwrap().lifetime_s = 30;
    let c = Client::new(&dev(&m)).unwrap();
    c.subscription(&creds()).unwrap();
    c.subscription(&creds()).unwrap();
    assert_eq!(m.state.lock().unwrap().issued.len(), 2);
    // A token the service revoked is renewed once.
    m.state.lock().unwrap().lifetime_s = 3600;
    c.subscription(&creds()).unwrap();
    let last = m.state.lock().unwrap().issued.last().cloned().unwrap();
    m.state.lock().unwrap().revoked.push(last);
    c.subscription(&creds()).unwrap();
    assert_eq!(m.state.lock().unwrap().issued.len(), 4);
}

#[test]
fn refusals_carry_their_reasons() {
    let m = mock(None);
    let c = Client::new(&dev(&m)).unwrap();
    let e = c.subscription(&Credentials::new("ak_live_1", "wrong")).unwrap_err();
    assert_eq!((e.reason.as_str(), e.message.as_str()), ("unauthorised", sub::REFUSED));
    m.state.lock().unwrap().answer = Some((403, json!({"reason": "subscription_inactive", "state": "suspended", "since": "2026-10-18"})));
    let e = c.subscription(&creds()).unwrap_err();
    assert_eq!((e.reason.as_str(), e.message.as_str()), ("subscription_inactive", "The subscription is suspended (since 2026-10-18)"));
    m.state.lock().unwrap().answer =
        Some((429, json!({"reason": "quota_exceeded", "allowance": 200, "unit": "compute_hours", "resets_at": "2026-11-01"})));
    let e = c.subscription(&creds()).unwrap_err();
    assert_eq!(e.reason, "quota_exceeded");
    assert!(e.message.contains("200 compute_hours") && e.message.contains("2026-11-01"), "{}", e.message);
    let e = Client::new(&Endpoint { url: format!("http://127.0.0.1:{}/api", m.port), ..Endpoint::default() }).unwrap_err();
    assert_eq!(e.reason, "insecure_url");
}

/// A CA and a `localhost` certificate it signed.
fn certificates() -> (String, Vec<u8>, Arc<rustls::ServerConfig>) {
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let mut ca = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca.distinguished_name.push(rcgen::DnType::CommonName, "Reviera Test CA");
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = rcgen::Issuer::new(ca, ca_key);
    let leaf_key = rcgen::KeyPair::generate().unwrap();
    let leaf = rcgen::CertificateParams::new(vec!["localhost".to_owned()]).unwrap().signed_by(&leaf_key, &issuer).unwrap();
    let der = leaf.der().to_vec();
    let key = rustls::pki_types::PrivateKeyDer::try_from(leaf_key.serialize_der()).unwrap();
    let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![leaf.der().clone(), ca_cert.der().clone()], key)
        .unwrap();
    (ca_cert.pem(), der, Arc::new(cfg))
}

#[test]
fn certificates_are_verified_or_pinned() {
    let (ca_pem, leaf_der, cfg) = certificates();
    let m = mock(Some(cfg));
    let url = format!("https://localhost:{}/api", m.port);
    // Not in the system's store: refused, with the reason.
    let e = Client::new(&Endpoint { url: url.clone(), ..Endpoint::default() }).unwrap().subscription(&creds()).unwrap_err();
    assert_eq!(e.reason, "certificate", "{e:?}");
    assert!(m.state.lock().unwrap().seen.is_empty(), "nothing was sent");
    // Its CA added.
    let c = Client::new(&Endpoint { url: url.clone(), ca_pem: Some(ca_pem), ..Endpoint::default() }).unwrap();
    assert_eq!(c.subscription(&creds()).unwrap().plan, "Team 200");
    // Pinned.
    let pin = sub::fingerprint(&leaf_der);
    let c = Client::new(&Endpoint { url: url.clone(), pin_sha256: Some(pin), ..Endpoint::default() }).unwrap();
    assert_eq!(c.subscription(&creds()).unwrap().plan, "Team 200");
    let wrong = sub::fingerprint(b"another certificate");
    let e = Client::new(&Endpoint { url, pin_sha256: Some(wrong), ..Endpoint::default() }).unwrap().subscription(&creds()).unwrap_err();
    assert_eq!(e.reason, "certificate");
    assert!(e.message.contains("does not match the subscription's pin"), "{}", e.message);
}

#[test]
fn https_to_a_plain_server_fails_safely() {
    let m = mock(None);
    let e = Client::new(&Endpoint { url: format!("https://localhost:{}/api", m.port), ..Endpoint::default() })
        .unwrap()
        .authenticate(&creds())
        .unwrap_err();
    assert!(matches!(e.reason.as_str(), "tls" | "unreachable"), "{e:?}");
}
