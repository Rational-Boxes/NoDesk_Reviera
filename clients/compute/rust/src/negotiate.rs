// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! Technique negotiation (DM-RPC-001): each side offers the identifiers it accepts, in order of
//! preference, per category; the choice is the client's most preferred identifier that the
//! server also accepts. A category with no common identifier fails with `negotiation_failed`
//! rather than falling back to something weaker.

use crate::pb::{TechniqueChoice, TechniqueOffer};

pub const COMPRESSION: &str = "compression";
pub const HASH: &str = "hash";
pub const CONTENT_ENCRYPTION: &str = "content-encryption";

pub const IDENTITY_V1: &str = "identity-v1";
pub const ZSTD_V1: &str = "zstd-v1";
pub const SHA256_V1: &str = "sha256-v1";
pub const NONE_LOCAL_V1: &str = "none-local-v1";

/// What this crate supports, most preferred first.
pub fn supported() -> Vec<TechniqueOffer> {
    vec![
        offer(COMPRESSION, &[ZSTD_V1, IDENTITY_V1]),
        offer(HASH, &[SHA256_V1]),
        offer(CONTENT_ENCRYPTION, &[NONE_LOCAL_V1]),
    ]
}

pub fn offer(category: &str, preferred: &[&str]) -> TechniqueOffer {
    TechniqueOffer {
        category: category.into(),
        preferred: preferred.iter().map(|s| s.to_string()).collect(),
    }
}

/// Chooses, for every category the client offers, its most preferred identifier the server
/// accepts. Errors name the first category with no common identifier.
pub fn choose(
    client: &[TechniqueOffer],
    server: &[TechniqueOffer],
) -> Result<Vec<TechniqueChoice>, String> {
    let mut out = Vec::new();
    for c in client {
        let accepted = server
            .iter()
            .find(|s| s.category == c.category)
            .map(|s| s.preferred.as_slice())
            .unwrap_or(&[]);
        let id = c
            .preferred
            .iter()
            .find(|id| accepted.contains(id))
            .ok_or_else(|| {
                format!(
                    "no common {} technique (client offers {:?}, server accepts {:?})",
                    c.category, c.preferred, accepted
                )
            })?;
        out.push(TechniqueChoice {
            category: c.category.clone(),
            id: id.clone(),
        });
    }
    Ok(out)
}

/// The identifier chosen for `category`, if any.
pub fn chosen<'a>(choices: &'a [TechniqueChoice], category: &str) -> Option<&'a str> {
    choices
        .iter()
        .find(|c| c.category == category)
        .map(|c| c.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clients_preference_wins_among_common_identifiers() {
        let client = [offer(COMPRESSION, &["future-v9", ZSTD_V1, IDENTITY_V1])];
        let server = [offer(COMPRESSION, &[IDENTITY_V1, ZSTD_V1])];
        let c = choose(&client, &server).unwrap();
        assert_eq!(chosen(&c, COMPRESSION), Some(ZSTD_V1));
    }

    #[test]
    fn no_common_identifier_fails_without_falling_back() {
        let client = [offer(COMPRESSION, &["future-v9"])];
        let err = choose(&client, &supported()).unwrap_err();
        assert!(err.contains("compression"), "{err}");
    }
}
