// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Encryption at rest with SQLCipher (Ch 2b §10.1, DM-LIVE-016).
//!
//! Each store gets a random 256-bit key held in the OS credential store under its own entry. The
//! entry name, which is not secret, is kept in `<key>.keyid` beside the store, so a store moved to
//! the recovery bin still finds its key. Without a credential store the user can choose a
//! passphrase (SQLCipher derives the key with PBKDF2) or turn encryption off, unless policy
//! requires it.

use std::fmt;
use std::fs;
use std::path::Path;

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Credential-store service name for live-store keys.
const SERVICE: &str = "org.nodeskreviera.live";

/// How new stores are encrypted.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum Encryption {
    /// A random key per store in the OS credential store (the default).
    #[default]
    OsKeyring,
    /// A key derived from a recovery passphrase.
    Passphrase(String),
    /// Not encrypted.
    Off,
}

impl fmt::Debug for Encryption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Encryption::OsKeyring => f.write_str("OsKeyring"),
            // Never print the passphrase.
            Encryption::Passphrase(_) => f.write_str("Passphrase(<redacted>)"),
            Encryption::Off => f.write_str("Off"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
    OsStore,
    Passphrase,
    None,
}

/// `live.encryption.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct EncryptionStatus {
    pub enabled: bool,
    pub key_source: KeySource,
    /// Whether policy requires encryption.
    pub required: bool,
    /// Why the store is not encrypted although encryption was asked for.
    pub note: Option<String>,
}

/// The key for one store, ready to apply to a connection.
#[derive(Clone)]
pub(crate) struct StoreKey {
    pragma: Option<String>,
    pub source: KeySource,
    pub note: Option<String>,
}

impl fmt::Debug for StoreKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreKey")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("encryption is required by policy, but no key is available: {0}")]
    Required(String),
    #[error("the recovery data is encrypted and its key is not available: {0}")]
    Unavailable(String),
}

impl StoreKey {
    pub fn none() -> Self {
        Self {
            pragma: None,
            source: KeySource::None,
            note: None,
        }
    }

    /// Opens a connection and unlocks it. Fails with "file is not a database" on a wrong key.
    pub fn open(&self, path: &Path) -> rusqlite::Result<Connection> {
        let conn = Connection::open(path)?;
        if let Some(p) = &self.pragma {
            // The key must come first; every connection must agree on the encrypted page size.
            conn.execute_batch(p)?;
            conn.execute_batch(&format!(
                "PRAGMA cipher_page_size = {};",
                crate::schema::PAGE_SIZE
            ))?;
        }
        Ok(conn)
    }

    pub fn encrypted(&self) -> bool {
        self.pragma.is_some()
    }
}

fn raw_key(hex: &str) -> StoreKey {
    StoreKey {
        pragma: Some(format!("PRAGMA key = \"x'{hex}'\";")),
        source: KeySource::OsStore,
        note: None,
    }
}

fn passphrase_key(p: &str) -> StoreKey {
    StoreKey {
        pragma: Some(format!("PRAGMA key = '{}';", p.replace('\'', "''"))),
        source: KeySource::Passphrase,
        note: None,
    }
}

/// Makes the key for a new store whose keyring entry name is `key_id`.
pub(crate) fn new_key(
    key_id: &str,
    encryption: &Encryption,
    required: bool,
) -> Result<StoreKey, KeyError> {
    match encryption {
        Encryption::Off if required => Err(KeyError::Required("encryption is turned off".into())),
        Encryption::Off => Ok(StoreKey::none()),
        Encryption::Passphrase(p) => Ok(passphrase_key(p)),
        Encryption::OsKeyring => match generate_in_keyring(key_id) {
            Ok(hex) => Ok(raw_key(&hex)),
            Err(e) if required => Err(KeyError::Required(e)),
            Err(e) => Ok(StoreKey {
                note: Some(format!("recovery data not encrypted: {e}")),
                ..StoreKey::none()
            }),
        },
    }
}

/// Finds the key of an existing store: its keyring entry if there is one, otherwise the configured
/// passphrase, otherwise none (an unencrypted store).
pub(crate) fn existing_key(key_id: Option<&str>, encryption: &Encryption) -> StoreKey {
    if let Some(id) = key_id
        && let Ok(hex) = keyring::Entry::new(SERVICE, id).and_then(|e| e.get_password())
    {
        return raw_key(&hex);
    }
    match encryption {
        Encryption::Passphrase(p) => passphrase_key(p),
        _ => StoreKey::none(),
    }
}

/// Removes a store's key from the credential store (when the store itself is deleted for good).
pub(crate) fn forget(key_id: &str) {
    if let Ok(e) = keyring::Entry::new(SERVICE, key_id) {
        let _ = e.delete_credential();
    }
}

fn generate_in_keyring(key_id: &str) -> Result<String, String> {
    keyring::Entry::store_status()
        .as_ref()
        .map_err(|e| format!("no credential store: {e}"))?;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let hex = hex::encode(bytes);
    keyring::Entry::new(SERVICE, key_id)
        .and_then(|e| e.set_password(&hex))
        .map_err(|e| e.to_string())?;
    Ok(hex)
}

pub(crate) fn read_key_id(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}
