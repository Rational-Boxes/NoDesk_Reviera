// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! The client side of remote analysis subscriptions (BIM Reviera spec Ch 2 §2.17.11,
//! DM-RPC-011), over the HTTPS binding described in `reviera/compute/v1/subscription.proto`.
//!
//! - Only `https://` URLs, except `http://localhost` when the caller asks for development use.
//! - TLS 1.2 or newer (rustls), verified against the system's certificate store plus an optional
//!   extra CA, or, for a self-hosted service, against a pinned SHA-256 of its certificate.
//! - The key and secret are sent only to the token endpoint, as HTTP Basic credentials; every
//!   other call carries the bearer token, which [`Client`] renews before it expires.
//! - Redirects are not followed, so credentials never go anywhere but the service's own URL.
//!
//! Needs the crate's `subscription` feature.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Url;
use reqwest::blocking::Response;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The token endpoint, relative to the service URL.
pub const TOKEN_PATH: &str = "v1/auth/token";
/// The subscription endpoint, relative to the service URL.
pub const SUBSCRIPTION_PATH: &str = "v1/subscription";
/// A token this close to expiry (seconds) is renewed before it is used.
pub const RENEW_BEFORE_S: i64 = 60;
/// The message for `unauthorised`.
pub const REFUSED: &str = "The service refused the key; check it or ask your administrator";

/// Where a service is and how its certificate is checked.
#[derive(Debug, Clone, Default)]
pub struct Endpoint {
    /// The service URL, `https://…`.
    pub url: String,
    /// Extra trusted CA certificates (PEM), for a self-hosted service.
    pub ca_pem: Option<String>,
    /// The SHA-256 of the service's certificate (hex, colons allowed). When set, it replaces
    /// certificate-chain verification: only that certificate is accepted.
    pub pin_sha256: Option<String>,
    /// Allows `http://localhost` (and 127.0.0.1, ::1), for developing a service.
    pub development: bool,
    /// The time allowed for each request; 20 s when zero.
    pub timeout: Duration,
}

/// An API key and its secret. `Debug` never shows the secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub key: String,
    pub secret: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("key", &self.key).field("secret", &"<redacted>").finish()
    }
}

impl Credentials {
    pub fn new(key: impl Into<String>, secret: impl Into<String>) -> Self {
        Self { key: key.into(), secret: secret.into() }
    }

    /// `key:secret`, split at the first colon.
    pub fn parse(s: &str) -> Option<Self> {
        let (k, v) = s.trim().split_once(':')?;
        (!k.is_empty() && !v.is_empty()).then(|| Self::new(k, v))
    }
}

/// A failed call: a contract reason code, a message for people and the service's details.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    /// `insecure_url`, `invalid_url`, `certificate`, `tls`, `unreachable`, `unauthorised`,
    /// `subscription_inactive`, `quota_exceeded`, `unsupported_kind` or `protocol`.
    pub reason: String,
    pub message: String,
    pub detail: serde_json::Value,
}

impl Error {
    fn new(reason: &str, message: impl Into<String>) -> Self {
        Self { reason: reason.into(), message: message.into(), detail: serde_json::Value::Null }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// A bearer token. `Debug` never shows it.
#[derive(Clone)]
pub struct Token {
    pub access_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub subscription_id: String,
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("expires_at", &self.expires_at)
            .field("subscription_id", &self.subscription_id)
            .finish_non_exhaustive()
    }
}

/// `GetSubscription`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Subscription {
    pub name: String,
    pub plan: String,
    /// `active`, `trial`, `suspended` or `expired`.
    pub state: String,
    pub period_start: String,
    pub period_end: String,
    /// 0 means unlimited.
    pub allowance: f64,
    pub usage: f64,
    /// `compute_hours`, `credits` or `jobs`.
    pub unit: String,
    pub job_kinds: Vec<String>,
    pub regions: Vec<String>,
    pub limits: Limits,
    pub terms_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    /// 0 means no stated limit.
    pub max_job_bytes: u64,
    /// 0 means no stated limit.
    pub concurrent_jobs: u32,
}

#[derive(Deserialize)]
struct TokenBody {
    access_token: String,
    #[serde(default)]
    expires_at: Option<String>,
    /// Accepted too: seconds from now, as OAuth token endpoints answer.
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    subscription_id: String,
}

/// Unix seconds now.
pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Checks a service URL: `https://`, or `http://localhost` in development; no user name or
/// password in it. The result ends in `/`, so the contract's paths join under it.
pub fn check_url(url: &str, development: bool) -> Result<Url, Error> {
    let mut u = Url::parse(url.trim()).map_err(|e| Error::new("invalid_url", format!("The service URL is not valid: {e}")))?;
    let local = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]" | "::1"));
    match u.scheme() {
        "https" => {}
        "http" if development && local => {}
        "http" if local => {
            return Err(Error::new(
                "insecure_url",
                "Plain http:// is refused; for a service on this computer, turn on Development in Advanced",
            ));
        }
        "http" => return Err(Error::new("insecure_url", "The service URL must start with https:// (plain http:// is refused)")),
        s => return Err(Error::new("invalid_url", format!("The service URL must start with https://, not {s}://"))),
    }
    if u.host_str().is_none_or(str::is_empty) {
        return Err(Error::new("invalid_url", "The service URL has no host"));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(Error::new("invalid_url", "Put the key and secret in their own fields, not in the URL"));
    }
    u.set_query(None);
    u.set_fragment(None);
    if !u.path().ends_with('/') {
        let p = format!("{}/", u.path());
        u.set_path(&p);
    }
    Ok(u)
}

/// A pin as 32 bytes: hex with optional colons or spaces.
pub fn parse_pin(pin: &str) -> Result<[u8; 32], Error> {
    let h: String = pin.chars().filter(|c| !matches!(c, ':' | ' ')).collect();
    let bad = || Error::new("invalid_input", "A certificate pin is the SHA-256 of the certificate: 64 hex digits");
    let v = hex::decode(h.trim().strip_prefix("sha256/").unwrap_or(h.trim())).map_err(|_| bad())?;
    v.try_into().map_err(|_| bad())
}

/// The SHA-256 of a certificate (DER), as lowercase hex: what a pin names.
pub fn fingerprint(der: &[u8]) -> String {
    hex::encode(Sha256::digest(der))
}

/// Accepts exactly the pinned certificate (its signatures still checked).
#[derive(Debug)]
struct Pinned {
    sha256: [u8; 32],
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if Sha256::digest(end_entity.as_ref()).as_slice() == self.sha256 {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(format!(
                "the certificate (SHA-256 {}) does not match the subscription's pin",
                fingerprint(end_entity.as_ref())
            )))
        }
    }

    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.algorithms)
    }

    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// The TLS error inside a request's error, if any.
fn tls_error<'a>(e: &'a (dyn std::error::Error + 'static)) -> Option<&'a rustls::Error> {
    let mut at = Some(e);
    while let Some(s) = at {
        if let Some(t) = s.downcast_ref::<rustls::Error>() {
            return Some(t);
        }
        // An io::Error's source() skips the error it wraps, so step into that instead.
        at = match s.downcast_ref::<std::io::Error>().and_then(|io| io.get_ref()) {
            Some(inner) => Some(inner as &(dyn std::error::Error + 'static)),
            None => s.source(),
        };
    }
    None
}

fn transport(e: reqwest::Error) -> Error {
    if let Some(t) = tls_error(&e) {
        return match t {
            rustls::Error::InvalidCertificate(_) | rustls::Error::NoCertificatesPresented | rustls::Error::General(_) => {
                Error::new("certificate", format!("The service's certificate was refused: {t}"))
            }
            _ => Error::new("tls", format!("A secure connection to the service failed: {t}")),
        };
    }
    if e.is_timeout() {
        return Error::new("unreachable", "The service did not answer in time");
    }
    let mut text = e.to_string();
    let mut at = std::error::Error::source(&e);
    while let Some(s) = at {
        text = format!("{text}: {s}");
        at = s.source();
    }
    Error::new("unreachable", format!("The service could not be reached: {text}"))
}

/// A connection to one service, holding its current token.
pub struct Client {
    http: reqwest::blocking::Client,
    base: Url,
    token: Mutex<Option<Token>>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("base", &self.base.as_str()).finish_non_exhaustive()
    }
}

impl Client {
    pub fn new(ep: &Endpoint) -> Result<Self, Error> {
        let base = check_url(&ep.url, ep.development)?;
        let provider: Arc<CryptoProvider> = Arc::new(rustls::crypto::ring::default_provider());
        let verifier: Arc<dyn ServerCertVerifier> = match ep.pin_sha256.as_deref().filter(|p| !p.trim().is_empty()) {
            Some(pin) => Arc::new(Pinned { sha256: parse_pin(pin)?, algorithms: provider.signature_verification_algorithms }),
            None => {
                let mut roots = Vec::new();
                if let Some(pem) = ep.ca_pem.as_deref().filter(|p| !p.trim().is_empty()) {
                    for c in CertificateDer::pem_slice_iter(pem.as_bytes()) {
                        roots.push(c.map_err(|e| Error::new("invalid_input", format!("The CA certificate is not valid PEM: {e}")))?);
                    }
                    if roots.is_empty() {
                        return Err(Error::new("invalid_input", "The CA file holds no certificate"));
                    }
                }
                Arc::new(
                    rustls_platform_verifier::Verifier::new_with_extra_roots(roots, provider.clone())
                        .map_err(|e| Error::new("certificate", format!("The certificates could not be loaded: {e}")))?,
                )
            }
        };
        // rustls offers only TLS 1.2 and 1.3.
        let mut tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| Error::new("tls", e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        let timeout = if ep.timeout.is_zero() { Duration::from_secs(20) } else { ep.timeout };
        let http = reqwest::blocking::Client::builder()
            .tls_backend_preconfigured(tls)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .user_agent(concat!("BIM-Reviera/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Error::new("tls", format!("The connection could not be set up: {e}")))?;
        Ok(Self { http, base, token: Mutex::new(None) })
    }

    /// The service URL, normalised.
    pub fn url(&self) -> &str {
        self.base.as_str()
    }

    fn at(&self, path: &str) -> Url {
        self.base.join(path).expect("a relative path joins")
    }

    /// `Authenticate`: a new token for these credentials, kept for the next calls.
    pub fn authenticate(&self, c: &Credentials) -> Result<Token, Error> {
        let r = self
            .http
            .post(self.at(TOKEN_PATH))
            .basic_auth(&c.key, Some(&c.secret))
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .send()
            .map_err(transport)?;
        let body: TokenBody = read(r)?;
        let expires_at = body
            .expires_at
            .as_deref()
            .and_then(parse_time)
            .or(body.expires_in.map(|s| now() + s))
            .ok_or_else(|| Error::new("protocol", "The service's token has no valid expiry"))?;
        if body.access_token.is_empty() {
            return Err(Error::new("protocol", "The service returned an empty token"));
        }
        let t = Token { access_token: body.access_token, expires_at, subscription_id: body.subscription_id };
        *self.token.lock().expect("token") = Some(t.clone());
        Ok(t)
    }

    /// The current token, renewed first when it is missing or near expiry.
    pub fn bearer(&self, c: &Credentials) -> Result<String, Error> {
        if let Some(t) = self.token.lock().expect("token").as_ref().filter(|t| t.expires_at - now() > RENEW_BEFORE_S) {
            return Ok(t.access_token.clone());
        }
        Ok(self.authenticate(c)?.access_token)
    }

    /// When the current token expires (Unix seconds), if there is one.
    pub fn token_expires_at(&self) -> Option<i64> {
        self.token.lock().expect("token").as_ref().map(|t| t.expires_at)
    }

    /// Drops the current token; the next call authenticates again.
    pub fn forget_token(&self) {
        *self.token.lock().expect("token") = None;
    }

    /// `GetSubscription`. A token the service no longer accepts is renewed once.
    pub fn subscription(&self, c: &Credentials) -> Result<Subscription, Error> {
        let had = self.token_expires_at().is_some();
        let get = |token: &str| self.http.get(self.at(SUBSCRIPTION_PATH)).bearer_auth(token).send().map_err(transport);
        let r = get(&self.bearer(c)?)?;
        if had && r.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.forget_token();
            return read(get(&self.bearer(c)?)?);
        }
        read(r)
    }
}

/// A response's JSON body, or its error.
fn read<T: serde::de::DeserializeOwned>(r: Response) -> Result<T, Error> {
    let status = r.status();
    let bytes = r.bytes().map_err(transport)?;
    if status.is_success() {
        return serde_json::from_slice(&bytes).map_err(|e| Error::new("protocol", format!("The service's answer was not understood: {e}")));
    }
    let detail: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    let reason = detail["reason"].as_str().map(str::to_owned).unwrap_or_else(|| {
        match status.as_u16() {
            401 => "unauthorised",
            403 => "subscription_inactive",
            429 => "quota_exceeded",
            _ => "protocol",
        }
        .to_owned()
    });
    let said = detail["message"].as_str().unwrap_or("").to_owned();
    let s = |k: &str| detail[k].as_str().map(str::to_owned).or_else(|| detail[k].as_f64().map(|v| v.to_string()));
    let message = match reason.as_str() {
        "unauthorised" => REFUSED.to_owned(),
        "subscription_inactive" => {
            let state = s("state").unwrap_or_else(|| "inactive".into());
            match s("since") {
                Some(d) => format!("The subscription is {state} (since {d})"),
                None => format!("The subscription is {state}"),
            }
        }
        "quota_exceeded" => match (s("allowance"), s("resets_at")) {
            (Some(a), Some(r)) => format!("The subscription's allowance of {a} {} is used; it resets on {r}", s("unit").unwrap_or_default()),
            _ => "The subscription's allowance is used".to_owned(),
        },
        "unsupported_kind" => format!("The subscription does not offer this job kind{}", if said.is_empty() { String::new() } else { format!(": {said}") }),
        _ if !said.is_empty() => said.clone(),
        _ => format!("The service answered HTTP {status}"),
    };
    Err(Error { reason, message, detail })
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// An RFC 3339 time as Unix seconds (fractions dropped).
pub fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d, h, mi, sec) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(f) = rest.strip_prefix('.') {
        rest = f.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let (oh, om) = (rest.get(1..3)?.parse::<i64>().ok()?, rest.get(4..6)?.parse::<i64>().ok()?);
            if rest.len() != 6 || rest.as_bytes()[3] != b':' {
                return None;
            }
            sign * (oh * 3600 + om * 60)
        }
    };
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset)
}

/// Unix seconds as RFC 3339 UTC (`2026-10-20T12:00:00Z`).
pub fn format_time(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_round_trip() {
        for t in [0, 951_782_400, 1_792_497_600, 4_102_444_800] {
            assert_eq!(parse_time(&format_time(t)), Some(t));
        }
        assert_eq!(parse_time("2026-10-20T12:00:00.250+02:00"), parse_time("2026-10-20T10:00:00Z"));
        assert_eq!(parse_time("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_time("yesterday"), None);
    }

    #[test]
    fn only_secure_urls() {
        assert_eq!(check_url("https://analysis.example.com/api", false).unwrap().as_str(), "https://analysis.example.com/api/");
        assert_eq!(check_url("http://analysis.example.com", true).unwrap_err().reason, "insecure_url");
        assert_eq!(check_url("http://localhost:8080", false).unwrap_err().reason, "insecure_url");
        assert!(check_url("http://localhost:8080", true).is_ok());
        assert_eq!(check_url("https://k:s@example.com", false).unwrap_err().reason, "invalid_url");
        assert_eq!(check_url("ftp://example.com", false).unwrap_err().reason, "invalid_url");
    }

    #[test]
    fn credentials_and_pins() {
        let c = Credentials::parse("ak_123:s:e:c").unwrap();
        assert_eq!((c.key.as_str(), c.secret.as_str()), ("ak_123", "s:e:c"));
        assert!(!format!("{c:?}").contains("s:e:c"));
        assert!(Credentials::parse("nocolon").is_none());
        let fp = fingerprint(b"x");
        assert_eq!(parse_pin(&fp).unwrap().to_vec(), hex::decode(&fp).unwrap());
        let colons: Vec<String> = fp.as_bytes().chunks(2).map(|c| String::from_utf8_lossy(c).to_uppercase()).collect();
        assert!(parse_pin(&colons.join(":")).is_ok());
        assert!(parse_pin("abc").is_err());
    }
}
