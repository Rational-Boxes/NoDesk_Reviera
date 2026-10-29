// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reviera Extension Data: versioned JSON documents carried in `Reviera_*` property sets (spec Ch 2a).
//!
//! This crate is carrier-independent: it encodes and decodes payloads (§3.3) and defines the
//! property layout (§3.2) and the identity data (§6.4). Reading and writing the property sets in an
//! IFC file is done by `reviera-store`.

pub mod encoding;
pub mod identity;

pub use encoding::{DecodeLimits, Encoded, Encoding, RxdError, canonical_json, decode, encode};
pub use identity::{ElementData, IdentityDoc, IdentitySection};

/// Property set on `IfcProject` describing the RXD set (§3.2).
pub const MANIFEST_PSET: &str = "Reviera_Manifest";
/// Property set on an element holding its element data.
pub const ELEMENT_PSET: &str = "Reviera_Element";
/// Property set on a type object holding its type data.
pub const TYPE_PSET: &str = "Reviera_Type";
/// Prefix of the property set holding one project-level document: `Reviera_Doc:<path>`.
pub const DOC_PSET_PREFIX: &str = "Reviera_Doc:";

/// Whether a property set carries RXD (the manifest, element, type and document sets), as
/// against Reviera's published property sets of user-visible values (`Reviera_GlazingUnit`, …;
/// spec ARC-GLZ-007), which are read as parameters like any other set.
pub fn is_carrier(name: &str) -> bool {
    name == MANIFEST_PSET || name == ELEMENT_PSET || name == TYPE_PSET || name.starts_with(DOC_PSET_PREFIX)
}

pub const FORMAT: &str = "reviera-rxd";
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

/// Property names of `Reviera_Manifest` (§3.2).
pub mod manifest {
    pub const FORMAT: &str = "Format";
    pub const FORMAT_VERSION: &str = "FormatVersion";
    pub const DATA_ID: &str = "DataId";
    pub const DATA_REVISION: &str = "DataRevision";
    pub const WRITER: &str = "Writer";
    pub const CARRIER: &str = "Carrier";
    pub const INDEX: &str = "Index";
}
