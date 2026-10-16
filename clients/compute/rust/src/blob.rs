// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! Content-addressed blobs: hashing, codecs, chunking for `PutBlobs`, and reassembly with
//! hash checks.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::negotiate::{IDENTITY_V1, ZSTD_V1};
use crate::pb::{BlobChunk, BlobRef};

/// Largest `data` in one chunk message.
pub const CHUNK_BYTES: usize = 4 << 20;

/// SHA-256 of `bytes`, lower-case hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Encodes `raw` with a codec identifier from the registry.
pub fn encode(codec: &str, raw: &[u8]) -> Result<Vec<u8>, String> {
    match codec {
        IDENTITY_V1 => Ok(raw.to_vec()),
        ZSTD_V1 => zstd::bulk::compress(raw, 9).map_err(|e| e.to_string()),
        other => Err(format!("unknown codec {other}")),
    }
}

/// Decodes bytes written by [`encode`]; `size` is the expected decoded size.
pub fn decode(codec: &str, encoded: &[u8], size: u64) -> Result<Vec<u8>, String> {
    let out = match codec {
        IDENTITY_V1 => encoded.to_vec(),
        ZSTD_V1 => zstd::bulk::decompress(encoded, size as usize).map_err(|e| e.to_string())?,
        other => return Err(format!("unknown codec {other}")),
    };
    if out.len() as u64 != size {
        return Err(format!("decoded {} bytes, expected {size}", out.len()));
    }
    Ok(out)
}

/// A blob ready to send: its reference and its raw content.
#[derive(Debug, Clone)]
pub struct Blob {
    pub reference: BlobRef,
    pub raw: Vec<u8>,
}

impl Blob {
    pub fn new(raw: Vec<u8>, media_type: &str, schema: &str) -> Blob {
        Blob {
            reference: BlobRef {
                sha256: sha256_hex(&raw),
                media_type: media_type.into(),
                schema: schema.into(),
                size: raw.len() as u64,
            },
            raw,
        }
    }
}

/// The chunk messages that upload `blob` with `codec`.
pub fn chunks(blob: &Blob, codec: &str) -> Result<Vec<BlobChunk>, String> {
    let encoded = encode(codec, &blob.raw)?;
    let mut out = Vec::new();
    let mut offset = 0usize;
    loop {
        let end = (offset + CHUNK_BYTES).min(encoded.len());
        out.push(BlobChunk {
            blob: (offset == 0).then(|| blob.reference.clone()),
            sha256: blob.reference.sha256.clone(),
            codec: if offset == 0 {
                codec.into()
            } else {
                String::new()
            },
            offset: offset as u64,
            data: encoded[offset..end].to_vec(),
            last: end == encoded.len(),
        });
        if end == encoded.len() {
            break;
        }
        offset = end;
    }
    Ok(out)
}

/// Reassembles uploaded blobs, decoding and checking each when its last chunk arrives.
#[derive(Debug, Default)]
pub struct Assembler {
    open: HashMap<String, (BlobRef, String, Vec<u8>)>,
}

impl Assembler {
    /// Adds a chunk; returns the finished blob when this was its last chunk.
    pub fn push(&mut self, chunk: BlobChunk) -> Result<Option<Blob>, String> {
        if let Some(r) = chunk.blob {
            if r.sha256 != chunk.sha256 {
                return Err("chunk hash does not match its blob".into());
            }
            self.open
                .insert(chunk.sha256.clone(), (r, chunk.codec, Vec::new()));
        }
        let entry = self
            .open
            .get_mut(&chunk.sha256)
            .ok_or_else(|| format!("chunk for an unknown blob {}", chunk.sha256))?;
        if entry.2.len() as u64 != chunk.offset {
            return Err(format!(
                "blob {} chunk at {} after {} bytes",
                chunk.sha256,
                chunk.offset,
                entry.2.len()
            ));
        }
        entry.2.extend_from_slice(&chunk.data);
        if !chunk.last {
            return Ok(None);
        }
        let (reference, codec, encoded) = self.open.remove(&chunk.sha256).unwrap();
        let raw = decode(&codec, &encoded, reference.size)?;
        if sha256_hex(&raw) != reference.sha256 {
            return Err(format!("blob {} failed its hash check", reference.sha256));
        }
        Ok(Some(Blob { reference, raw }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_reassemble_and_are_checked() {
        let raw: Vec<u8> = (0..(9 << 20)).map(|i| (i % 251) as u8).collect();
        let blob = Blob::new(raw.clone(), "application/octet-stream", "test");
        for codec in [IDENTITY_V1, ZSTD_V1] {
            let cs = chunks(&blob, codec).unwrap();
            let mut a = Assembler::default();
            let mut done = None;
            for c in cs {
                done = a.push(c).unwrap().or(done);
            }
            assert_eq!(done.unwrap().raw, raw);
        }
        // A corrupted chunk fails the hash check.
        let mut cs = chunks(&blob, IDENTITY_V1).unwrap();
        cs[0].data[0] ^= 1;
        let mut a = Assembler::default();
        let r: Result<Vec<_>, _> = cs.into_iter().map(|c| a.push(c)).collect();
        assert!(r.is_err());
    }
}
