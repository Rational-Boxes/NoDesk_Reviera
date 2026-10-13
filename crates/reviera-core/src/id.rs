// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Element identity: an internal integer key plus the IFC GlobalId that persists in files (DM-ID-001).

use std::borrow::Cow;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Internal element key, unique within one model and never reused.
///
/// 1–999 are reserved for model singletons, so ordinary elements start at [`ElementId::FIRST`].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct ElementId(pub u64);

impl ElementId {
    pub const FIRST: ElementId = ElementId(1000);
}

impl fmt::Display for ElementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// IFC GlobalId: a 128-bit UUID in IFC's 22-character base-64 form (alphabet `0-9A-Za-z_$`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GlobalId(String);

const ALPHABET: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

impl GlobalId {
    /// A new random (UUID v4) GlobalId.
    pub fn new() -> Self {
        Self::from_uuid(uuid::Uuid::new_v4())
    }

    /// A new time-ordered (UUID v7) GlobalId, for the "time-ordered GUIDs" setting.
    pub fn new_time_ordered() -> Self {
        Self::from_uuid(uuid::Uuid::now_v7())
    }

    /// Encodes a UUID: the first character carries the top 2 bits, the other 21 carry 6 bits each.
    pub fn from_uuid(id: uuid::Uuid) -> Self {
        let n = id.as_u128();
        let s = (0..22)
            .map(|i| {
                let (shift, mask) = if i == 0 {
                    (126, 0x3)
                } else {
                    (126 - 6 * i, 0x3f)
                };
                ALPHABET[((n >> shift) & mask) as usize] as char
            })
            .collect();
        Self(s)
    }

    /// Parses and validates a 22-character GlobalId.
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        let valid = b.len() == 22 && b[0] <= b'3' && b.iter().all(|c| ALPHABET.contains(c));
        valid.then(|| Self(s.to_owned()))
    }

    pub fn to_uuid(&self) -> uuid::Uuid {
        let n = self.0.bytes().fold(0u128, |n, c| {
            (n << 6)
                | ALPHABET
                    .iter()
                    .position(|&a| a == c)
                    .expect("validated on construction") as u128
        });
        uuid::Uuid::from_u128(n)
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

impl fmt::Display for GlobalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl JsonSchema for GlobalId {
    fn schema_name() -> Cow<'static, str> {
        "GlobalId".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[0-3][0-9A-Za-z_$]{21}$",
            "description": "IFC GlobalId (22 characters)"
        })
    }
}

impl TryFrom<String> for GlobalId {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        Self::parse(&s).ok_or_else(|| format!("`{s}` is not a 22-character IFC GlobalId"))
    }
}

impl From<GlobalId> for String {
    fn from(g: GlobalId) -> String {
        g.0
    }
}

/// An element reference in command arguments: an internal ID (integer) or a GlobalId (string).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ElementRef {
    Id(ElementId),
    Guid(GlobalId),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_id_is_22_chars_and_round_trips() {
        let u = uuid::Uuid::new_v4();
        let g = GlobalId::from_uuid(u);
        assert_eq!(g.as_str().len(), 22);
        assert_eq!(g.to_uuid(), u);
        assert_eq!(GlobalId::parse(g.as_str()), Some(g));
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

    #[test]
    fn parse_rejects_bad_input() {
        for bad in [
            "",
            "abc",
            "4000000000000000000000",
            "000000000000000000000-",
        ] {
            assert_eq!(GlobalId::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn element_ref_accepts_integer_or_guid() {
        let g = GlobalId::new();
        assert_eq!(
            serde_json::from_value::<ElementRef>(5001.into()).unwrap(),
            ElementRef::Id(ElementId(5001))
        );
        assert_eq!(
            serde_json::from_value::<ElementRef>(g.as_str().into()).unwrap(),
            ElementRef::Guid(g)
        );
        assert!(serde_json::from_value::<ElementRef>("nope".into()).is_err());
    }
}
