// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The building code a model follows (DM-CODE-001): a model-wide default, kept with the units in
//! Model Properties, whose values tools use (today the stair headroom of ARC-STAIR-007). A tool
//! may use another code for one operation.
//!
//! Headroom over stairs, measured vertically from the line of the tread nosings (or a landing's
//! floor), by code (sources in `design_documents/RESEARCH_LOG.md`, 2026-10-14):
//! - US International Residential Code R311.7.2: 6 ft 8 in (2032 mm); spiral stairs (R311.7.10.1)
//!   6 ft 6 in (1982 mm).
//! - US International Building Code 1011.3: 80 in (2032 mm), continuous over the full width;
//!   spiral stairs 78 in (1981 mm).
//! - England, Approved Document K §1.10: 2.0 m (loft conversions 1.9 m at the centre).
//! - National Building Code of Canada 9.8.2.2: 1950 mm in dwelling units, 2050 mm elsewhere.
//! - Australia, NCC 2022 Housing Provisions: 2.0 m above the nosing line.
//! - Germany, DIN 18065: 2.0 m clear headroom.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::store::Store;

/// The parameter of the project holding its code settings.
pub const CODE_KEY: &str = "rxd:building_code";

/// A building code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    /// US International Building Code.
    #[default]
    Ibc,
    /// US International Residential Code.
    Irc,
    /// England: Approved Document K.
    UkAdk,
    /// National Building Code of Canada.
    NbcCanada,
    /// Australia: National Construction Code.
    NccAustralia,
    /// Germany: DIN 18065.
    Din18065,
    /// The model's own values.
    Custom,
}

/// Who uses the building, where a code tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Occupancy {
    /// Within a dwelling unit.
    #[default]
    Dwelling,
    /// Elsewhere (public, commercial, common areas).
    Other,
}

/// A code's stair headroom and where it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct Headroom {
    /// Metres.
    pub stair: f64,
    /// Over spiral stairs, metres.
    pub spiral: f64,
    /// The clause it comes from.
    pub clause: &'static str,
}

impl Code {
    pub const ALL: &'static [Code] = &[
        Code::Ibc,
        Code::Irc,
        Code::UkAdk,
        Code::NbcCanada,
        Code::NccAustralia,
        Code::Din18065,
        Code::Custom,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Code::Ibc => "ibc",
            Code::Irc => "irc",
            Code::UkAdk => "uk_adk",
            Code::NbcCanada => "nbc_canada",
            Code::NccAustralia => "ncc_australia",
            Code::Din18065 => "din_18065",
            Code::Custom => "custom",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Code::Ibc => "International Building Code (US)",
            Code::Irc => "International Residential Code (US)",
            Code::UkAdk => "Approved Document K (England)",
            Code::NbcCanada => "National Building Code of Canada",
            Code::NccAustralia => "National Construction Code (Australia)",
            Code::Din18065 => "DIN 18065 (Germany)",
            Code::Custom => "Custom",
        }
    }

    /// The structural defaults and design standards that go with this code's region (STR-COL-001,
    /// research/08 §e).
    pub fn structural(self) -> StructuralDefaults {
        let us = StructuralDefaults {
            region: Region::UnitedStates,
            catalogs: &[
                "aisc_w",
                "aisc_hss_rect",
                "aisc_hss_round",
                "aisc_pipe_std",
                "aisc_c",
                "aisc_l",
                "aisc_wt",
                "na_lumber",
            ],
            steel: "Steel ASTM A992",
            concrete: "Concrete f'c 4000 psi",
            timber: "Wood Douglas Fir-Larch No.2",
            beam: ("aisc_w", "W12X26"),
            brace: ("aisc_hss_rect", "HSS6X6X3/8"),
            loads: "ASCE/SEI 7",
            steel_standard: "AISC 360",
            concrete_standard: "ACI 318",
            timber_standard: "AWC NDS",
        };
        let eu = StructuralDefaults {
            region: Region::Europe,
            catalogs: &["eu_heb", "eu_hea", "eu_ipe", "uk_uc", "uk_ub", "eu_timber"],
            steel: "Steel S355 (EN 10025-2)",
            concrete: "Concrete C30/37",
            timber: "Timber C24 (EN 338)",
            beam: ("eu_ipe", "IPE 300"),
            brace: ("eu_hea", "HEA 160"),
            loads: "EN 1990, EN 1991 (with the National Annex)",
            steel_standard: "EN 1993",
            concrete_standard: "EN 1992",
            timber_standard: "EN 1995",
        };
        match self {
            Code::Ibc | Code::Irc => us,
            Code::NbcCanada => StructuralDefaults {
                region: Region::Canada,
                steel: "Steel CSA G40.21 350W",
                timber: "Wood Spruce-Pine-Fir No.1/No.2",
                concrete: "Concrete 30 MPa",
                loads: "NBC Part 4",
                steel_standard: "CSA S16",
                concrete_standard: "CSA A23.3",
                timber_standard: "CSA O86",
                ..us
            },
            Code::UkAdk => StructuralDefaults {
                region: Region::UnitedKingdom,
                catalogs: &["uk_uc", "uk_ub", "eu_heb", "eu_hea", "eu_ipe", "eu_timber"],
                beam: ("uk_ub", "UB 305x165x40"),
                brace: ("uk_uc", "UC 152x152x23"),
                loads: "EN 1990, EN 1991 (with the UK National Annex)",
                ..eu
            },
            Code::NccAustralia => StructuralDefaults {
                region: Region::Australia,
                steel: "Steel AS/NZS 3679.1 300 (300PLUS)",
                concrete: "Concrete 32 MPa",
                loads: "AS/NZS 1170",
                steel_standard: "AS 4100",
                concrete_standard: "AS 3600",
                timber_standard: "AS 1720.1",
                ..eu
            },
            Code::Din18065 | Code::Custom => eu,
        }
    }

    pub fn parse(s: &str) -> Option<Code> {
        let s = s.trim();
        Code::ALL
            .iter()
            .copied()
            .find(|c| c.key().eq_ignore_ascii_case(s) || c.label().eq_ignore_ascii_case(s))
    }

    /// The headroom the code asks over stairs for `occupancy` (`None` for Custom, which has
    /// none of its own).
    pub fn headroom(self, occupancy: Occupancy) -> Option<Headroom> {
        Some(match self {
            Code::Ibc => Headroom {
                stair: 2.032,
                spiral: 1.981,
                clause: "IBC 1011.3: 80 in (2032 mm); spiral stairs 78 in (1011.10)",
            },
            Code::Irc => Headroom {
                stair: 2.032,
                spiral: 1.982,
                clause: "IRC R311.7.2: 6 ft 8 in (2032 mm); spiral stairs 6 ft 6 in (R311.7.10.1)",
            },
            Code::UkAdk => Headroom {
                stair: 2.0,
                spiral: 2.0,
                clause: "Approved Document K 1.10: 2.0 m (loft conversions 1.9 m at the centre)",
            },
            Code::NbcCanada => match occupancy {
                Occupancy::Dwelling => Headroom {
                    stair: 1.95,
                    spiral: 1.95,
                    clause: "NBC 9.8.2.2: 1950 mm within dwelling units",
                },
                Occupancy::Other => Headroom {
                    stair: 2.05,
                    spiral: 2.05,
                    clause: "NBC 9.8.2.2: 2050 mm outside dwelling units",
                },
            },
            Code::NccAustralia => Headroom {
                stair: 2.0,
                spiral: 2.0,
                clause: "NCC 2022 Housing Provisions: 2.0 m above the nosing line",
            },
            Code::Din18065 => Headroom {
                stair: 2.0,
                spiral: 2.0,
                clause: "DIN 18065: 2.0 m clear headroom",
            },
            Code::Custom => return None,
        })
    }
}

/// A region, for its structural defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    UnitedStates,
    Canada,
    Europe,
    UnitedKingdom,
    Australia,
}

/// Structural defaults for a region: the section catalogues offered first, the default steel and
/// concrete, and the design standards that apply (applied by analysis engines, named here).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct StructuralDefaults {
    pub region: Region,
    /// Section catalogues (`structural.catalogs`), most usual first.
    pub catalogs: &'static [&'static str],
    pub steel: &'static str,
    pub concrete: &'static str,
    /// The usual timber grade or strength class (for timber sections without their own).
    pub timber: &'static str,
    /// The usual rolled beam (catalogue, designation): the default beam type.
    pub beam: (&'static str, &'static str),
    /// The usual brace section (catalogue, designation): the default brace type.
    pub brace: (&'static str, &'static str),
    pub loads: &'static str,
    pub steel_standard: &'static str,
    pub concrete_standard: &'static str,
    pub timber_standard: &'static str,
}

/// The model's code settings (Model Properties).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CodeSettings {
    pub code: Code,
    pub occupancy: Occupancy,
    /// The model's own stair headroom (metres), in place of the code's; required for Custom.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headroom: Option<f64>,
}

impl CodeSettings {
    /// The headroom over stairs under `code` (the model's when `None`), and its source: the
    /// model's own value when set, else the code's.
    pub fn headroom(&self, code: Option<Code>) -> Headroom {
        let code = code.unwrap_or(self.code);
        let own = self.headroom.filter(|_| code == self.code);
        match (own, code.headroom(self.occupancy)) {
            (Some(h), _) => Headroom {
                stair: h,
                spiral: h,
                clause: "the model's own headroom (Model Properties)",
            },
            (None, Some(h)) => h,
            // Custom with no value of its own: the commonest figure.
            (None, None) => Headroom {
                stair: 2.0,
                spiral: 2.0,
                clause: "2.0 m (no headroom set for Custom)",
            },
        }
    }
}

/// The model's code settings.
pub fn settings(store: &Store) -> CodeSettings {
    store
        .iter()
        .find(|e| e.class == "IfcProject")
        .and_then(|e| e.param(CODE_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

// ---- Commands ----

/// One code, for choosing.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CodeInfo {
    pub key: &'static str,
    pub label: &'static str,
    /// Its stair headroom for the model's occupancy (none for Custom).
    pub headroom: Option<Headroom>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodeReport {
    pub settings: CodeSettings,
    pub label: &'static str,
    /// The headroom in force and its source.
    pub headroom: Headroom,
    pub codes: Vec<CodeInfo>,
}

pub(crate) fn report(store: &Store) -> CodeReport {
    let s = settings(store);
    CodeReport {
        label: s.code.label(),
        headroom: s.headroom(None),
        codes: Code::ALL
            .iter()
            .map(|c| CodeInfo {
                key: c.key(),
                label: c.label(),
                headroom: c.headroom(s.occupancy),
            })
            .collect(),
        settings: s,
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    /// A code's key or name.
    pub code: Option<String>,
    pub occupancy: Option<Occupancy>,
    /// The model's own stair headroom (accepts units); `null` or empty returns to the code's.
    #[serde(default, deserialize_with = "some_value")]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub headroom: Option<Value>,
}

fn some_value<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// Sets the model's building code, occupancy and headroom (`code.set`).
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let mut s = settings(&store);
    if let Some(c) = &p.code {
        s.code = Code::parse(c).ok_or_else(|| {
            let keys: Vec<&str> = Code::ALL.iter().map(|c| c.key()).collect();
            Error::new(
                "code.unknown",
                format!("no code `{c}` (one of {})", keys.join(", ")),
            )
        })?;
    }
    if let Some(o) = p.occupancy {
        s.occupancy = o;
    }
    if let Some(v) = &p.headroom {
        s.headroom = match v {
            Value::Null => None,
            Value::String(t) if t.trim().is_empty() => None,
            Value::Number(n) => n.as_f64(),
            Value::String(t) => Some(crate::units::parse(
                t,
                "length",
                &crate::units::settings_of(&store),
            )?),
            _ => return Err(Error::new("code.bad_headroom", "the headroom is a length")),
        };
        if let Some(h) = s.headroom
            && !(1.5..=4.0).contains(&h)
        {
            return Err(Error::new(
                "code.bad_headroom",
                "the headroom must be between 1.5 m and 4 m",
            ));
        }
    }
    let project = store
        .iter()
        .find(|e| e.class == "IfcProject")
        .map(crate::element::Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let value = serde_json::to_value(&s).expect("serialises");
    txn.modify(project, |e| {
        e.params.insert(CODE_KEY.into(), value);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_give_their_headroom_and_the_model_may_override_it() {
        let s = CodeSettings::default();
        assert_eq!(s.headroom(None).stair, 2.032);
        assert_eq!(s.headroom(Some(Code::UkAdk)).stair, 2.0);
        let canada = CodeSettings {
            code: Code::NbcCanada,
            occupancy: Occupancy::Other,
            headroom: None,
        };
        assert_eq!(canada.headroom(None).stair, 2.05);
        let own = CodeSettings {
            headroom: Some(2.1),
            ..CodeSettings::default()
        };
        assert_eq!(own.headroom(None).stair, 2.1);
        // Another code for one operation ignores the model's own value.
        assert_eq!(own.headroom(Some(Code::Din18065)).stair, 2.0);
        assert_eq!(Code::parse("IRC"), Some(Code::Irc));
    }
}
