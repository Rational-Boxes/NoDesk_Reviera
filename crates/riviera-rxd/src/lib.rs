// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Riviera Extension Data: versioned JSON documents carried in `Riviera_*` property sets (spec Ch 2a).
//!
//! This crate is carrier-independent: it encodes and decodes payloads (§3.3) and defines the
//! property layout (§3.2) and the identity data (§6.4). Reading and writing the property sets in an
//! IFC file is done by `riviera-store`.

pub mod encoding;
pub mod identity;

pub use encoding::{DecodeLimits, Encoded, Encoding, RxdError, canonical_json, decode, encode};
pub use identity::{ElementData, IdentityDoc, IdentitySection};

/// Property set on `IfcProject` describing the RXD set (§3.2).
pub const MANIFEST_PSET: &str = "Riviera_Manifest";
/// Property set on an element holding its element data.
pub const ELEMENT_PSET: &str = "Riviera_Element";
/// Property set on a type object holding its type data.
pub const TYPE_PSET: &str = "Riviera_Type";
/// Prefix of the property set holding one project-level document: `Riviera_Doc:<path>`.
pub const DOC_PSET_PREFIX: &str = "Riviera_Doc:";

/// Whether a property set carries RXD (the manifest, element, type and document sets), as
/// against Riviera's published property sets of user-visible values (`Riviera_GlazingUnit`, …;
/// spec ARC-GLZ-007), which are read as parameters like any other set.
pub fn is_carrier(name: &str) -> bool {
    name == MANIFEST_PSET || name == ELEMENT_PSET || name == TYPE_PSET || name.starts_with(DOC_PSET_PREFIX)
}

/// The prefix of Riviera's property sets.
pub const PREFIX: &str = "Riviera_";
/// The prefix in files saved before the project's name was spelt correctly (2026-11-04).
pub const LEGACY_PREFIX: &str = "Reviera_";

/// A property set's name as Riviera now writes it: a `Reviera_*` set from an older file becomes
/// `Riviera_*`; any other name is unchanged.
pub fn current_name(name: &str) -> std::borrow::Cow<'_, str> {
    match name.strip_prefix(LEGACY_PREFIX) {
        Some(rest) => format!("{PREFIX}{rest}").into(),
        None => name.into(),
    }
}

pub const FORMAT: &str = "riviera-rxd";
/// The format name in files saved before the name was spelt correctly.
pub const LEGACY_FORMAT: &str = "reviera-rxd";
pub const FORMAT_VERSION: &str = "1.0";
pub const SCHEMA_VERSION: &str = "1.0";

/// Property names of a document or element-data property set (§3.2).
pub mod prop {
    pub const KIND: &str = "Kind";
    pub const SCHEMA_VERSION: &str = "SchemaVersion";
    pub const ENCODING: &str = "Encoding";
    pub const SHA256: &str = "Sha256";
    pub const REV: &str = "Rev";
    pub const GEOM_HASH: &str = "GeomHash";
    pub const DATA: &str = "Data";

    /// Name of payload chunk `n` (1-based): `Data_0001`, …
    pub fn chunk(n: usize) -> String {
        format!("Data_{n:04}")
    }
}

/// Property names of `Riviera_Manifest` (§3.2).
pub mod manifest {
    pub const FORMAT: &str = "Format";
    pub const FORMAT_VERSION: &str = "FormatVersion";
    pub const DATA_ID: &str = "DataId";
    pub const DATA_REVISION: &str = "DataRevision";
    pub const WRITER: &str = "Writer";
    pub const CARRIER: &str = "Carrier";
    pub const INDEX: &str = "Index";
}

#[cfg(test)]
mod legacy_tests {
    #[test]
    fn old_spelling_is_read_as_the_current_one() {
        assert_eq!(super::current_name("Reviera_Element"), "Riviera_Element");
        assert_eq!(super::current_name("Reviera_Doc:identity"), "Riviera_Doc:identity");
        assert_eq!(super::current_name("Riviera_Type"), "Riviera_Type");
        assert_eq!(super::current_name("Pset_WallCommon"), "Pset_WallCommon");
    }
}
