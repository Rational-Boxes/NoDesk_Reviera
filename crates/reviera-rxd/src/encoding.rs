// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Payload encoding (Ch 2a §3.3): canonical ASCII-only JSON, `zstd+base64` above 8 KiB, chunks of
//! at most 32,000 characters, and a SHA-256 over the decoded JSON bytes.

use std::io::Read;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RxdError {
    #[error("unknown RXD encoding `{0}`")]
    UnknownEncoding(String),
    #[error("RXD payload is corrupt: {0}")]
    Corrupt(String),
    #[error("RXD payload hash mismatch")]
    HashMismatch,
    #[error("RXD payload exceeds the decode limit")]
    TooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Json,
    ZstdBase64,
}

impl Encoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Encoding::Json => "json",
            Encoding::ZstdBase64 => "zstd+base64",
        }
    }

    pub fn parse(s: &str) -> Result<Self, RxdError> {
        match s {
            "json" => Ok(Encoding::Json),
            "zstd+base64" => Ok(Encoding::ZstdBase64),
            other => Err(RxdError::UnknownEncoding(other.into())),
        }
    }
}

/// An encoded payload, ready to store as `Encoding`, `Sha256` and `Data` / `Data_nnnn` properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub encoding: Encoding,
    /// Lower-case hex SHA-256 of the canonical JSON bytes.
    pub sha256: String,
    pub chunks: Vec<String>,
}

/// Payloads up to this size are stored as readable JSON.
pub const JSON_LIMIT: usize = 8 * 1024;
/// Maximum characters per chunk.
pub const CHUNK_SIZE: usize = 32_000;

#[derive(Debug, Clone, Copy)]
pub struct DecodeLimits {
    pub max_decoded_bytes: usize,
    pub max_chunks: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_decoded_bytes: 256 * 1024 * 1024,
            max_chunks: 100_000,
        }
    }
}

/// Canonical JSON: sorted keys, no insignificant whitespace, every non-ASCII character escaped.
pub fn canonical_json(value: &Value) -> String {
    // serde_json's default map is a BTreeMap, so keys serialise sorted.
    let compact = serde_json::to_string(value).expect("JSON values always serialise");
    let mut out = String::with_capacity(compact.len());
    for c in compact.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            // Non-ASCII can only occur inside strings, where \u escapes are valid.
            let mut units = [0u16; 2];
            for u in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{u:04x}"));
            }
        }
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn encode(value: &Value) -> Encoded {
    let json = canonical_json(value);
    let sha256 = sha256_hex(json.as_bytes());
    let (encoding, text) = if json.len() <= JSON_LIMIT {
        (Encoding::Json, json)
    } else {
        let compressed =
            zstd::encode_all(json.as_bytes(), 19).expect("in-memory compression cannot fail");
        (Encoding::ZstdBase64, BASE64.encode(compressed))
    };
    // Both forms are ASCII, so byte offsets are character offsets.
    let chunks = text
        .as_bytes()
        .chunks(CHUNK_SIZE)
        .map(|c| String::from_utf8(c.to_vec()).unwrap())
        .collect();
    Encoded {
        encoding,
        sha256,
        chunks,
    }
}

pub fn decode(
    encoding: Encoding,
    chunks: &[String],
    sha256: Option<&str>,
    limits: DecodeLimits,
) -> Result<Value, RxdError> {
    if chunks.len() > limits.max_chunks {
        return Err(RxdError::TooLarge);
    }
    let text = chunks.concat();
    let json = match encoding {
        Encoding::Json => text.into_bytes(),
        Encoding::ZstdBase64 => {
            let compressed = BASE64
                .decode(text.as_bytes())
                .map_err(|e| RxdError::Corrupt(e.to_string()))?;
            let decoder = zstd::stream::read::Decoder::new(compressed.as_slice())
                .map_err(|e| RxdError::Corrupt(e.to_string()))?;
            let mut out = Vec::new();
            decoder
                .take(limits.max_decoded_bytes as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|e| RxdError::Corrupt(e.to_string()))?;
            out
        }
    };
    if json.len() > limits.max_decoded_bytes {
        return Err(RxdError::TooLarge);
    }
    if let Some(expected) = sha256
        && !sha256_hex(&json).eq_ignore_ascii_case(expected)
    {
        return Err(RxdError::HashMismatch);
    }
    serde_json::from_slice(&json).map_err(|e| RxdError::Corrupt(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_is_sorted_compact_and_ascii() {
        let v = json!({"b": 1, "a": {"z": "\u{e9}\u{1f600}", "y": [1, 2]}});
        let expected = "{\"a\":{\"y\":[1,2],\"z\":\"\\u00e9\\ud83d\\ude00\"},\"b\":1}";
        assert_eq!(canonical_json(&v), expected);
        assert_eq!(
            serde_json::from_str::<Value>(&canonical_json(&v)).unwrap(),
            v
        );
    }

    #[test]
    fn small_payloads_are_plain_json() {
        let v = json!({"identity": {"iid": 48213}});
        let e = encode(&v);
        assert_eq!(e.encoding, Encoding::Json);
        assert_eq!(e.chunks, vec![r#"{"identity":{"iid":48213}}"#.to_string()]);
        assert_eq!(
            decode(
                e.encoding,
                &e.chunks,
                Some(&e.sha256),
                DecodeLimits::default()
            )
            .unwrap(),
            v
        );
    }

    #[test]
    fn ten_megabyte_document_is_chunked_and_reassembles() {
        // Pseudo-random text so compression cannot shrink it below one chunk.
        let mut x: u64 = 1;
        let items: Vec<String> = (0..200_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                format!("{x:016x}{x:x}")
            })
            .collect();
        let v = json!({ "items": items });
        assert!(canonical_json(&v).len() > 6_000_000);
        let e = encode(&v);
        assert_eq!(e.encoding, Encoding::ZstdBase64);
        assert!(
            e.chunks.len() > 1
                && e.chunks
                    .iter()
                    .all(|c| c.len() <= CHUNK_SIZE && c.is_ascii())
        );
        assert_eq!(
            decode(
                e.encoding,
                &e.chunks,
                Some(&e.sha256),
                DecodeLimits::default()
            )
            .unwrap(),
            v
        );
    }

    #[test]
    fn encoding_is_deterministic() {
        let v = json!({"z": [1.5, "x"], "a": null});
        assert_eq!(encode(&v), encode(&v.clone()));
    }

    #[test]
    fn decode_detects_corruption_and_limits() {
        let e = encode(&json!({"a": 1}));
        let bad = vec![e.chunks[0].replace('1', "2")];
        assert_eq!(
            decode(e.encoding, &bad, Some(&e.sha256), DecodeLimits::default()),
            Err(RxdError::HashMismatch)
        );
        let limits = DecodeLimits {
            max_decoded_bytes: 3,
            ..DecodeLimits::default()
        };
        assert_eq!(
            decode(e.encoding, &e.chunks, None, limits),
            Err(RxdError::TooLarge)
        );
        assert!(Encoding::parse("gzip").is_err());
    }
}
