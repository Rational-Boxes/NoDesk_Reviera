// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Element identity: an internal integer key plus the IFC GlobalId that persists in files.

use serde::{Deserialize, Serialize};

/// Internal element key, stable for the lifetime of an open model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ElementId(pub u64);

/// IFC GlobalId: the 22-character compressed GUID stored in the file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GlobalId(String);

impl GlobalId {
    const ALPHABET: &'static [u8; 64] =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

    /// Generates a new random GlobalId.
    pub fn new() -> Self {
        Self::from_uuid(uuid::Uuid::new_v4())
    }

    /// Encodes a UUID in the IFC base-64 form (2 bits, then 21 groups of 6 bits).
    pub fn from_uuid(id: uuid::Uuid) -> Self {
        let n = id.as_u128();
        let s = (0..22)
            .map(|i| {
                Self::ALPHABET
                    [((n >> (126 - 6 * i).max(0)) & if i == 0 { 0x3 } else { 0x3f }) as usize]
                    as char
            })
            .collect();
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for GlobalId {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_id_is_22_chars() {
        assert_eq!(GlobalId::new().as_str().len(), 22);
    }

    #[test]
    fn nil_and_max_uuids_encode_to_known_forms() {
        assert_eq!(
            GlobalId::from_uuid(uuid::Uuid::nil()).as_str(),
            "0000000000000000000000"
        );
        assert_eq!(
            GlobalId::from_uuid(uuid::Uuid::max()).as_str(),
            "3$$$$$$$$$$$$$$$$$$$$$"
        );
    }
}
