// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Element data and the identity records (Ch 2 §2.5, Ch 2a §6.4).
//!
//! Unknown sections and fields are kept as they are, so data written by newer versions or by
//! add-ins survives a round trip (DM-RXD-007).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::RxdError;

/// The `identity` section of element data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IdentitySection {
    /// Persisted internal element ID.
    pub iid: u64,
    /// GUID of the element this one was copied from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_guid: Option<String>,
    /// `<episode>:<counter>` of the last change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified_txn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified_by: Option<String>,
    /// Milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified_at: Option<u64>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// The payload of a `Reviera_Element` or `Reviera_Type` property set: named sections.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ElementData {
    sections: Map<String, Value>,
}

impl ElementData {
    pub const IDENTITY: &'static str = "identity";

    pub fn from_value(value: Value) -> Result<Self, RxdError> {
        match value {
            Value::Object(sections) => Ok(Self { sections }),
            _ => Err(RxdError::Corrupt(
                "element data is not a JSON object".into(),
            )),
        }
    }

    pub fn to_value(&self) -> Value {
        Value::Object(self.sections.clone())
    }

    /// The `Kind` property: the section names, comma-separated, in sorted order.
    pub fn kind(&self) -> String {
        self.sections.keys().cloned().collect::<Vec<_>>().join(",")
    }

    pub fn section(&self, name: &str) -> Option<&Value> {
        self.sections.get(name)
    }

    pub fn identity(&self) -> Result<Option<IdentitySection>, RxdError> {
        self.sections
            .get(Self::IDENTITY)
            .map(|v| {
                serde_json::from_value(v.clone())
                    .map_err(|e| RxdError::Corrupt(format!("identity: {e}")))
            })
            .transpose()
    }

    pub fn set_identity(&mut self, identity: &IdentitySection) {
        let v = serde_json::to_value(identity).expect("identity serialises");
        self.sections.insert(Self::IDENTITY.into(), v);
    }
}

/// The `project/identity` document: the next free internal ID and the GUIDs of non-rooted IFC
/// resources (Ch 2 §2.5).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IdentityDoc {
    pub next_iid: u64,
    /// Resource key (for example `material:<Name>`) → GUID.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub resources: Map<String, Value>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl IdentityDoc {
    pub const PATH: &'static str = "project/identity";
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_sections_and_fields_survive() {
        let v = json!({
            "identity": {"iid": 5001, "future_field": true},
            "org.example.acoustics": {"rating": 52}
        });
        let mut data = ElementData::from_value(v.clone()).unwrap();
        assert_eq!(data.kind(), "identity,org.example.acoustics");
        let mut id = data.identity().unwrap().unwrap();
        assert_eq!(id.iid, 5001);
        id.last_modified_by = Some("user".into());
        data.set_identity(&id);
        let out = data.to_value();
        assert_eq!(out["identity"]["future_field"], true);
        assert_eq!(out["org.example.acoustics"], v["org.example.acoustics"]);
        assert_eq!(out["identity"]["last_modified_by"], "user");
    }

    #[test]
    fn rejects_non_objects() {
        assert!(ElementData::from_value(json!([1])).is_err());
    }
}
