// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! COBie export data (spec Ch 5 DOC-OUT-015): the model's facility-management metadata as COBie
//! tables, and their validation.
//!
//! Built so far: the COBie 2.4 layout:
//! - Contact, Facility, Floor, Space, Zone, Type, Component and Attribute come from the model.
//! - System, Assembly, Connection, Spare, Resource, Job, Impact, Document, Coordinate and Issue
//!   are written with headers only.
//! - PickLists lists the units and categories used.
//!
//! The current NBIMS-US table set follows once its column order is confirmed from the published
//! standard.
//!
//! Every record carries `ExtSystem` = "NoDesk Reviera", `ExtObject` = its IFC class and
//! `ExtIdentifier` = its IFC GlobalId, so a COBie data set and an IFC file describe the same
//! objects. Values are in SI units (metres, square metres, cubic metres), stated on the Facility
//! sheet. Required values the model does not have are "n/a"; the export never invents them.

use std::collections::{BTreeMap, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, GlobalId};
use crate::store::Store;

pub const SETTINGS_CLASS: &str = "rxd:project/cobie";
pub const SETTINGS_KEY: &str = "rxd:cobie";
pub const EXT_SYSTEM: &str = "NoDesk Reviera";
const NA: &str = "n/a";

/// A contact (COBie Contact / Company).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub email: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub given_name: String,
    #[serde(default)]
    pub family_name: String,
    /// The contact's role (for example "Architect").
    #[serde(default)]
    pub category: String,
}

/// The project's COBie settings (RXD `project/cobie`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The first contact creates the data set.
    #[serde(default)]
    pub contacts: Vec<Contact>,
    /// Categories whose elements are components (maintainable assets).
    #[serde(default = "default_categories")]
    pub component_categories: Vec<String>,
    /// The room parameter that names each room's zone, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_parameter: Option<String>,
    /// The room parameter that gives each space's category, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_type_parameter: Option<String>,
    /// Suffix repeated names (" (2)", " (3)") instead of reporting them.
    #[serde(default)]
    pub make_names_unique: bool,
}

fn default_categories() -> Vec<String> {
    [
        "cat.doors",
        "cat.windows",
        "cat.furniture",
        "cat.casework",
        "cat.specialty_equipment",
        "cat.plumbing_fixtures",
        "cat.mechanical_equipment",
        "cat.electrical_equipment",
        "cat.lighting_fixtures",
        "cat.air_terminals",
        "cat.fire_protection",
        "cat.communication_devices",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            contacts: Vec::new(),
            component_categories: default_categories(),
            zone_parameter: None,
            space_type_parameter: None,
            make_names_unique: false,
        }
    }
}

/// The project's COBie settings (defaults when none are stored).
pub fn settings(store: &Store) -> Settings {
    store
        .iter()
        .find(|e| e.class == SETTINGS_CLASS)
        .and_then(|e| serde_json::from_value(e.param(SETTINGS_KEY)?.clone()).ok())
        .unwrap_or_default()
}

/// A COBie table: its sheet name, columns and rows of text.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Table {
    pub name: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// A validation problem.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Issue {
    pub table: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    /// The record's Name.
    pub record: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_id: Option<ElementId>,
    pub rule: String,
    pub message: String,
}

/// The COBie 2.4 sheets and their columns, in order.
pub const SHEETS_2_4: &[(&str, &[&str])] = &[
    (
        "Contact",
        &[
            "Email",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "Company",
            "Phone",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Department",
            "OrganizationCode",
            "GivenName",
            "FamilyName",
            "Street",
            "PostalBox",
            "Town",
            "StateRegion",
            "PostalCode",
            "Country",
        ],
    ),
    (
        "Facility",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "ProjectName",
            "SiteName",
            "LinearUnits",
            "AreaUnits",
            "VolumeUnits",
            "CurrencyUnit",
            "AreaMeasurement",
            "ExternalSystem",
            "ExternalProjectObject",
            "ExternalProjectIdentifier",
            "ExternalSiteObject",
            "ExternalSiteIdentifier",
            "ExternalFacilityObject",
            "ExternalFacilityIdentifier",
            "Description",
            "ProjectDescription",
            "SiteDescription",
            "Phase",
        ],
    ),
    (
        "Floor",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Description",
            "Elevation",
            "Height",
        ],
    ),
    (
        "Space",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "FloorName",
            "Description",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "RoomTag",
            "UsableHeight",
            "GrossArea",
            "NetArea",
        ],
    ),
    (
        "Zone",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "SpaceNames",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Description",
        ],
    ),
    (
        "Type",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "Description",
            "AssetType",
            "Manufacturer",
            "ModelNumber",
            "WarrantyGuarantorParts",
            "WarrantyDurationParts",
            "WarrantyGuarantorLabor",
            "WarrantyDurationLabor",
            "WarrantyDurationUnit",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "ReplacementCost",
            "ExpectedLife",
            "DurationUnit",
            "WarrantyDescription",
            "NominalLength",
            "NominalWidth",
            "NominalHeight",
            "ModelReference",
            "Shape",
            "Size",
            "Color",
            "Finish",
            "Grade",
            "Material",
            "Constituents",
            "Features",
            "AccessibilityPerformance",
            "CodePerformance",
            "SustainabilityPerformance",
        ],
    ),
    (
        "Component",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "TypeName",
            "Space",
            "Description",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "SerialNumber",
            "InstallationDate",
            "WarrantyStartDate",
            "TagNumber",
            "BarCode",
            "AssetIdentifier",
        ],
    ),
    (
        "System",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "ComponentNames",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Description",
        ],
    ),
    (
        "Assembly",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "SheetName",
            "ParentName",
            "ChildNames",
            "AssemblyType",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Description",
        ],
    ),
    (
        "Connection",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "ConnectionType",
            "SheetName",
            "RowName1",
            "RowName2",
            "RealizingElement",
            "PortName1",
            "PortName2",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Description",
        ],
    ),
    (
        "Spare",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "TypeName",
            "Suppliers",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Description",
            "SetNumber",
            "PartNumber",
        ],
    ),
    (
        "Resource",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Description",
        ],
    ),
    (
        "Job",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "Status",
            "TypeName",
            "Description",
            "Duration",
            "DurationUnit",
            "Start",
            "TaskStartUnit",
            "Frequency",
            "FrequencyUnit",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "TaskNumber",
            "Priors",
            "ResourceNames",
        ],
    ),
    (
        "Impact",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "ImpactType",
            "ImpactStage",
            "SheetName",
            "RowName",
            "Value",
            "ImpactUnit",
            "LeadInTime",
            "Duration",
            "LeadOutTime",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Description",
        ],
    ),
    (
        "Document",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "ApprovalBy",
            "Stage",
            "SheetName",
            "RowName",
            "Directory",
            "File",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Description",
            "Reference",
        ],
    ),
    (
        "Attribute",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "SheetName",
            "RowName",
            "Value",
            "Unit",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "Description",
            "AllowedValues",
        ],
    ),
    (
        "Coordinate",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Category",
            "SheetName",
            "RowName",
            "CoordinateXAxis",
            "CoordinateYAxis",
            "CoordinateZAxis",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
            "ClockwiseRotation",
            "ElevationalRotation",
            "YawRotation",
        ],
    ),
    (
        "Issue",
        &[
            "Name",
            "CreatedBy",
            "CreatedOn",
            "Type",
            "Risk",
            "Chance",
            "Impact",
            "SheetName1",
            "RowName1",
            "SheetName2",
            "RowName2",
            "Description",
            "Owner",
            "Mitigation",
            "ExtSystem",
            "ExtObject",
            "ExtIdentifier",
        ],
    ),
    ("PickLists", &["Category", "Value"]),
];

/// A record being built: its sheet values by column, and where it came from.
struct Record {
    values: BTreeMap<&'static str, String>,
    element: Option<ElementId>,
}

fn columns(sheet: &str) -> &'static [&'static str] {
    SHEETS_2_4
        .iter()
        .find(|(n, _)| *n == sheet)
        .map(|(_, c)| *c)
        .expect("a COBie 2.4 sheet")
}

fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    if s == "-0" { "0".into() } else { s }
}

fn text_param(e: &Element, key: &str) -> Option<String> {
    e.param(key).and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o.get("value").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    })
}

struct Builder<'a> {
    store: &'a Store,
    parents: &'a HashMap<ElementId, ElementId>,
    settings: Settings,
    created_by: String,
    created_on: String,
    sheets: BTreeMap<&'static str, Vec<Record>>,
}

impl Builder<'_> {
    fn record(
        &mut self,
        sheet: &'static str,
        element: Option<ElementId>,
        pairs: &[(&'static str, String)],
    ) {
        let mut values: BTreeMap<&'static str, String> = BTreeMap::new();
        values.insert("CreatedBy", self.created_by.clone());
        values.insert("CreatedOn", self.created_on.clone());
        for (k, v) in pairs {
            values.insert(k, v.clone());
        }
        if let Some(e) = element.and_then(|e| self.store.get(e)) {
            values
                .entry("ExtSystem")
                .or_insert_with(|| EXT_SYSTEM.into());
            values.entry("ExtObject").or_insert_with(|| e.class.clone());
            values
                .entry("ExtIdentifier")
                .or_insert_with(|| e.guid().as_str().to_owned());
        }
        self.sheets
            .entry(sheet)
            .or_default()
            .push(Record { values, element });
    }

    /// Parameters of an element as Attribute records of a sheet row.
    fn attributes(&mut self, sheet: &'static str, row: &str, e: ElementId) {
        let Ok(defs) = crate::parameters::list(self.store, None, Some(e)) else {
            return;
        };
        let mut seen = std::collections::BTreeSet::new();
        for d in defs {
            if !seen.insert(d.name.clone()) {
                continue;
            }
            let Ok(v) = crate::parameters::get(self.store, e, &d.reference) else {
                continue;
            };
            let (value, unit) = match &v.value {
                Value::Null => continue,
                Value::Number(n) => {
                    let unit = match crate::units::data_type(&d.data_type).map(|t| t.key) {
                        Some("length") => "meters",
                        Some("area") => "square meters",
                        Some("volume") => "cubic meters",
                        Some("angle") => "radians",
                        _ => "",
                    };
                    (num(n.as_f64().unwrap_or_default()), unit.to_owned())
                }
                Value::String(s) if s.is_empty() => continue,
                Value::String(s) => (s.clone(), String::new()),
                other => (other.to_string(), String::new()),
            };
            let category = if v.source == "type" {
                "Requirement"
            } else {
                "As Built"
            };
            self.record(
                "Attribute",
                Some(e),
                &[
                    ("Name", d.name.clone()),
                    ("Category", category.into()),
                    ("SheetName", sheet.into()),
                    ("RowName", row.into()),
                    ("Value", value),
                    ("Unit", unit),
                    ("Description", d.name),
                    ("AllowedValues", NA.into()),
                ],
            );
        }
    }

    /// The level an element is on, up the model tree.
    fn level_of(&self, e: ElementId) -> Option<ElementId> {
        let mut id = e;
        for _ in 0..8 {
            id = *self.parents.get(&id)?;
            if self.store.get(id)?.class == crate::levels::CLASS {
                return Some(id);
            }
        }
        None
    }

    /// The room a component is in: a door or window's room on either side of its wall, else
    /// the space it is contained in.
    fn space_of(&self, e: &Element, rooms: &HashMap<ElementId, String>) -> Option<String> {
        if crate::inserts::kind_of(e).is_some()
            && let Ok(p) = crate::inserts::resolve(self.store, e)
            && let Some(level) = self.level_of(p.host)
        {
            let (d, n) = (p.wall.direction(), p.wall.normal());
            let base = [
                p.wall.start[0] + d[0] * p.recipe.along,
                p.wall.start[1] + d[1] * p.recipe.along,
            ];
            let off = p.wall.width / 2.0 + 0.3;
            for s in [1.0, -1.0] {
                let q = [base[0] + n[0] * off * s, base[1] + n[1] * off * s];
                if let Some(r) = crate::rooms::room_at(self.store, level, q) {
                    return rooms.get(&r).cloned();
                }
            }
        }
        // An element contained in a space.
        let mut id = e.id();
        for _ in 0..8 {
            id = *self.parents.get(&id)?;
            let p = self.store.get(id)?;
            if p.class == crate::rooms::CLASS {
                return Some(p.name.clone().unwrap_or_default());
            }
        }
        None
    }
}

/// The COBie 2.4 tables of a model, with the model tree's parents, created by the first contact
/// at `created_on` (an ISO date and time). Validation problems are returned alongside.
pub fn tables_2_4(
    store: &Store,
    parents: &HashMap<ElementId, ElementId>,
    created_on: &str,
) -> (Vec<Table>, Vec<Issue>) {
    let settings = settings(store);
    let created_by = settings
        .contacts
        .first()
        .map_or(NA.to_owned(), |c| c.email.clone());
    let mut b = Builder {
        store,
        parents,
        settings: settings.clone(),
        created_by,
        created_on: created_on.to_owned(),
        sheets: BTreeMap::new(),
    };
    // Contact.
    for c in &settings.contacts {
        b.record(
            "Contact",
            None,
            &[
                ("Email", c.email.clone()),
                (
                    "Category",
                    if c.category.is_empty() {
                        NA.into()
                    } else {
                        c.category.clone()
                    },
                ),
                (
                    "Company",
                    if c.company.is_empty() {
                        NA.into()
                    } else {
                        c.company.clone()
                    },
                ),
                (
                    "Phone",
                    if c.phone.is_empty() {
                        NA.into()
                    } else {
                        c.phone.clone()
                    },
                ),
                ("GivenName", c.given_name.clone()),
                ("FamilyName", c.family_name.clone()),
                ("ExtSystem", EXT_SYSTEM.into()),
            ],
        );
    }
    // Facility: the building, in its project and site.
    let first = |class: &str| {
        store
            .iter()
            .filter(|e| e.class == class)
            .min_by_key(|e| e.id())
    };
    let (project, site, building) = (first("IfcProject"), first("IfcSite"), first("IfcBuilding"));
    let name = |e: Option<&Element>| e.and_then(|e| e.name.clone()).unwrap_or_else(|| NA.into());
    let guid = |e: Option<&Element>| e.map_or(NA.into(), |e| e.guid().as_str().to_owned());
    let facility_name = name(building);
    b.record(
        "Facility",
        building.map(Element::id),
        &[
            ("Name", facility_name.clone()),
            ("Category", NA.into()),
            ("ProjectName", name(project)),
            ("SiteName", name(site)),
            ("LinearUnits", "meters".into()),
            ("AreaUnits", "square meters".into()),
            ("VolumeUnits", "cubic meters".into()),
            ("CurrencyUnit", NA.into()),
            ("AreaMeasurement", "Net area at wall finish faces".into()),
            ("ExternalSystem", EXT_SYSTEM.into()),
            ("ExternalProjectObject", "IfcProject".into()),
            ("ExternalProjectIdentifier", guid(project)),
            ("ExternalSiteObject", "IfcSite".into()),
            ("ExternalSiteIdentifier", guid(site)),
            ("ExternalFacilityObject", "IfcBuilding".into()),
            ("ExternalFacilityIdentifier", guid(building)),
            ("Phase", NA.into()),
        ],
    );
    if let Some(bld) = building {
        b.attributes("Facility", &facility_name, bld.id());
    }
    // Floor: the levels, with the height to the next.
    let levels = crate::levels::list(store);
    for (i, l) in levels.iter().enumerate() {
        let height = levels
            .get(i + 1)
            .map_or(NA.into(), |n| num(n.elevation - l.elevation));
        b.record(
            "Floor",
            Some(l.id),
            &[
                ("Name", l.name.clone()),
                ("Category", "Floor".into()),
                ("Description", l.name.clone()),
                ("Elevation", num(l.elevation)),
                ("Height", height),
            ],
        );
        b.attributes("Floor", &l.name, l.id);
    }
    // Space: placed, enclosed rooms.
    let level_name = |guid: &str| {
        GlobalId::parse(guid)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|l| store.get(l)?.name.clone())
            .unwrap_or_else(|| NA.into())
    };
    let mut room_names: HashMap<ElementId, String> = HashMap::new();
    for r in crate::rooms::all(store) {
        if !r.geometry.enclosed() {
            continue;
        }
        let e = store.get(r.id).expect("listed");
        let category = settings
            .space_type_parameter
            .as_ref()
            .and_then(|p| crate::parameters::get(store, r.id, p).ok()?.display)
            .unwrap_or_else(|| NA.into());
        room_names.insert(r.id, r.number.clone());
        b.record(
            "Space",
            Some(r.id),
            &[
                ("Name", r.number.clone()),
                ("Category", category),
                ("FloorName", level_name(&r.level)),
                (
                    "Description",
                    if r.name.is_empty() {
                        NA.into()
                    } else {
                        r.name.clone()
                    },
                ),
                ("RoomTag", r.number.clone()),
                ("UsableHeight", num(r.geometry.unbounded_height())),
                ("GrossArea", NA.into()),
                ("NetArea", num(r.geometry.area())),
            ],
        );
        b.attributes("Space", &r.number, e.id());
        // Zone: one record per zone and space.
        if let Some(zone) = settings
            .zone_parameter
            .as_ref()
            .and_then(|p| crate::parameters::get(store, r.id, p).ok()?.display)
            .filter(|z| !z.is_empty())
        {
            b.record(
                "Zone",
                None,
                &[
                    ("Name", zone),
                    ("Category", "Zone".into()),
                    ("SpaceNames", r.number.clone()),
                    ("ExtSystem", EXT_SYSTEM.into()),
                    ("ExtObject", "IfcZone".into()),
                    ("ExtIdentifier", NA.into()),
                    ("Description", NA.into()),
                ],
            );
        }
    }
    // Component and Type: elements of the component categories and their types.
    let mut components: Vec<&Element> = store
        .iter()
        .filter(|e| !crate::categories::is_type(e) && crate::inspect::in_tree(e))
        .filter(|e| {
            crate::categories::category_of(e)
                .is_some_and(|c| settings.component_categories.iter().any(|k| k == c.key))
        })
        .collect();
    components.sort_by_key(|e| e.id());
    let mut types: BTreeMap<ElementId, Vec<ElementId>> = BTreeMap::new();
    let mut counters: HashMap<String, usize> = HashMap::new();
    for e in &components {
        let ty = e
            .param("@type")
            .and_then(Value::as_str)
            .and_then(GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g));
        let type_name = ty
            .and_then(|t| store.get(t)?.name.clone())
            .unwrap_or_else(|| NA.into());
        if let Some(t) = ty {
            types.entry(t).or_default().push(e.id());
        }
        let name = text_param(e, "@Tag")
            .filter(|m| !m.is_empty())
            .or_else(|| e.name.clone().filter(|n| !n.is_empty()))
            .unwrap_or_else(|| {
                let n = counters.entry(type_name.clone()).or_insert(0);
                *n += 1;
                format!("{type_name} {n}")
            });
        let space = b.space_of(e, &room_names).unwrap_or_else(|| NA.into());
        b.record(
            "Component",
            Some(e.id()),
            &[
                ("Name", name.clone()),
                ("TypeName", type_name),
                ("Space", space),
                ("Description", e.name.clone().unwrap_or_else(|| NA.into())),
                ("SerialNumber", NA.into()),
                ("InstallationDate", NA.into()),
                ("WarrantyStartDate", NA.into()),
                (
                    "TagNumber",
                    text_param(e, "@Tag").unwrap_or_else(|| NA.into()),
                ),
                ("BarCode", NA.into()),
                ("AssetIdentifier", NA.into()),
            ],
        );
        b.attributes("Component", &name, e.id());
    }
    for t in types.keys() {
        let Some(te) = store.get(*t) else { continue };
        let name = te.name.clone().unwrap_or_else(|| NA.into());
        let param = |key: &str| {
            crate::parameters::get(store, *t, key)
                .ok()
                .and_then(|v| v.display)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| NA.into())
        };
        // Doors and windows know their nominal size.
        let (w, h) = crate::inserts::insert_type(te).map_or((NA.to_owned(), NA.to_owned()), |it| {
            (num(it.width), num(it.height))
        });
        let category = crate::categories::category_of(te).map_or(NA.into(), |c| c.name.to_owned());
        let mut pairs: Vec<(&'static str, String)> = vec![
            ("Name", name.clone()),
            ("Category", category),
            ("Description", param("type.description")),
            ("AssetType", "Fixed".into()),
            ("Manufacturer", param("type.manufacturer")),
            ("ModelNumber", param("type.model")),
            ("ReplacementCost", param("type.cost")),
            ("NominalWidth", w),
            ("NominalHeight", h),
        ];
        for c in columns("Type") {
            if !pairs.iter().any(|(k, _)| k == c)
                && ![
                    "CreatedBy",
                    "CreatedOn",
                    "ExtSystem",
                    "ExtObject",
                    "ExtIdentifier",
                ]
                .contains(c)
            {
                pairs.push((c, NA.into()));
            }
        }
        b.record("Type", Some(*t), &pairs);
        b.attributes("Type", &name, *t);
    }
    // PickLists: the units and categories used.
    let mut picks: Vec<(String, String)> = vec![
        ("LinearUnits".into(), "meters".into()),
        ("AreaUnits".into(), "square meters".into()),
        ("VolumeUnits".into(), "cubic meters".into()),
    ];
    for r in b.sheets.get("Type").into_iter().flatten() {
        let c = r.values.get("Category").cloned().unwrap_or_default();
        if c != NA
            && !picks
                .iter()
                .any(|(k, v)| k == "Category-Product" && *v == c)
        {
            picks.push(("Category-Product".into(), c));
        }
    }
    // Tables, with unique names and validation.
    let mut issues = Vec::new();
    let mut out = Vec::new();
    for (sheet, cols) in SHEETS_2_4 {
        let mut records = b.sheets.remove(sheet).unwrap_or_default();
        if *sheet == "PickLists" {
            out.push(Table {
                name: (*sheet).into(),
                columns: cols.iter().map(|c| (*c).to_owned()).collect(),
                rows: picks
                    .iter()
                    .map(|(k, v)| vec![k.clone(), v.clone()])
                    .collect(),
            });
            continue;
        }
        // Names are unique within a sheet (Attribute and Zone rows repeat by design).
        if !["Attribute", "Zone"].contains(sheet) {
            let mut seen: HashMap<String, usize> = HashMap::new();
            for r in &mut records {
                let key = if *sheet == "Contact" { "Email" } else { "Name" };
                let name = r.values.get(key).cloned().unwrap_or_default();
                let n = seen.entry(name.clone()).or_insert(0);
                *n += 1;
                if *n > 1 {
                    if b.settings.make_names_unique {
                        let renamed = format!("{name} ({n})");
                        issues.push(Issue {
                            table: (*sheet).into(),
                            column: Some(key.into()),
                            record: name.clone(),
                            element_id: r.element,
                            rule: "renamed".into(),
                            message: format!("renamed to \"{renamed}\" to make it unique"),
                        });
                        r.values.insert(key, renamed);
                    } else {
                        issues.push(Issue {
                            table: (*sheet).into(),
                            column: Some(key.into()),
                            record: name,
                            element_id: r.element,
                            rule: "unique_name".into(),
                            message: "another record has this name".into(),
                        });
                    }
                }
            }
        }
        let rows: Vec<Vec<String>> = records
            .iter()
            .map(|r| {
                cols.iter()
                    .map(|c| r.values.get(c).cloned().unwrap_or_else(|| NA.into()))
                    .collect()
            })
            .collect();
        out.push(Table {
            name: (*sheet).into(),
            columns: cols.iter().map(|c| (*c).to_owned()).collect(),
            rows,
        });
    }
    // References: components' spaces and types, spaces' floors; a creating contact.
    let names_of = |sheet: &str| -> std::collections::HashSet<String> {
        out.iter()
            .find(|t| t.name == sheet)
            .map(|t| t.rows.iter().map(|r| r[0].clone()).collect())
            .unwrap_or_default()
    };
    let (spaces, types_n, floors) = (names_of("Space"), names_of("Type"), names_of("Floor"));
    if settings.contacts.is_empty() {
        issues.push(Issue {
            table: "Contact".into(),
            column: Some("Email".into()),
            record: String::new(),
            element_id: None,
            rule: "required".into(),
            message: "no contact: add the company that creates the data set (COBie settings)"
                .into(),
        });
    }
    if let Some(t) = out.iter().find(|t| t.name == "Component") {
        let col = |n: &str| t.columns.iter().position(|c| c == n).expect("column");
        for (r, rec) in t.rows.iter().zip(&components) {
            let (space, ty) = (&r[col("Space")], &r[col("TypeName")]);
            if space == NA || !spaces.contains(space) {
                issues.push(Issue {
                    table: "Component".into(),
                    column: Some("Space".into()),
                    record: r[0].clone(),
                    element_id: Some(rec.id()),
                    rule: "reference".into(),
                    message: "the component is in no space of the Space sheet".into(),
                });
            }
            if ty == NA || !types_n.contains(ty) {
                issues.push(Issue {
                    table: "Component".into(),
                    column: Some("TypeName".into()),
                    record: r[0].clone(),
                    element_id: Some(rec.id()),
                    rule: "reference".into(),
                    message: "the component's type is not on the Type sheet".into(),
                });
            }
        }
    }
    if let Some(t) = out.iter().find(|t| t.name == "Space") {
        for r in &t.rows {
            if !floors.contains(&r[4]) {
                issues.push(Issue {
                    table: "Space".into(),
                    column: Some("FloorName".into()),
                    record: r[0].clone(),
                    element_id: None,
                    rule: "reference".into(),
                    message: "the space's floor is not on the Floor sheet".into(),
                });
            }
        }
    }
    (out, issues)
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSettingsParams {
    pub settings: Settings,
}

/// Stores the project's COBie settings (undoable).
pub(crate) fn set_settings(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetSettingsParams,
) -> Result<()> {
    if p.settings.contacts.iter().any(|c| !c.email.contains('@')) {
        return Err(Error::new(
            "cobie.bad_contact",
            "every contact needs an e-mail address",
        ));
    }
    let value = serde_json::to_value(&p.settings).expect("serialises");
    let existing = txn
        .store()
        .iter()
        .find(|e| e.class == SETTINGS_CLASS)
        .map(Element::id);
    match existing {
        Some(id) => txn.modify(id, |e| {
            e.params.insert(SETTINGS_KEY.into(), value);
        }),
        None => txn
            .create(
                NewElement::new(SETTINGS_CLASS)
                    .name("COBie")
                    .param(SETTINGS_KEY, value),
            )
            .map(|_| ()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_plain() {
        assert_eq!(num(2.1), "2.1");
        assert_eq!(num(3.0), "3");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(12.34567), "12.3457");
    }

    #[test]
    fn every_sheet_has_a_name_or_key_column_first() {
        for (sheet, cols) in SHEETS_2_4 {
            let first = if *sheet == "Contact" {
                "Email"
            } else if *sheet == "PickLists" {
                "Category"
            } else {
                "Name"
            };
            assert_eq!(cols[0], first, "{sheet}");
        }
    }
}
