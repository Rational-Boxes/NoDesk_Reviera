// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Elevators (ARC-VT-001 to ARC-VT-004): vertical-transport rule packs, an elevator sized from
//! its capacity and made of ordinary elements, its clear zones, and the checks.
//!
//! An elevator is an assembly. Its **car** is a custom object (`IfcTransportElement` ELEVATOR) on
//! the lowest level it serves, and it carries the assembly's data ([`DATA_KEY`]). Around it are a
//! **shaft opening** (ARC-OPEN-004) from the pit floor to the overhead, which cuts every floor
//! between, the **hoistway walls**, and a sliding automatic **landing entrance** at every level
//! served. The members carry [`MEMBER_KEY`] naming the car.
//!
//! An elevator's frame: `point` is the middle of the hoistway's inside front face (the entrance
//! line); local x runs across the hoistway to the right as you face it from the landing, local y
//! into the hoistway; `rotation` turns that frame counter-clockwise.

use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::clearances::{Clearance, ClearanceKind};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

type V2 = [f64; 2];

/// The packs a project has chosen (absent: chosen from its building code).
pub const RULES_KEY: &str = "rxd:vt_rules";
/// The project's own packs.
pub const PACKS_KEY: &str = "rxd:vt_rule_packs";
/// An elevator's data, on its car.
pub const DATA_KEY: &str = "rxd:elevator";
/// On each member of an elevator's assembly: its car's GlobalId.
pub const MEMBER_KEY: &str = "rxd:elevator_of";
/// The landing entrance door type.
pub const ENTRANCE_TYPE: &str = "Elevator Landing Entrance";
/// The height of a landing's clear floor space when its rule gives none.
pub const LANDING_HEIGHT: f64 = 2.03;

/// The published pack schema (MIT).
pub const SCHEMA: &str = include_str!("../../../schemas/vt/reviera.vt-rules-1.schema.json");
const SHIPPED: &[&str] = &[
    include_str!("../../../schemas/vt/packs/us_ibc_ada.json"),
    include_str!("../../../schemas/vt/packs/us_a17.json"),
    include_str!("../../../schemas/vt/packs/us_a18.json"),
    include_str!("../../../schemas/vt/packs/practice_us.json"),
    include_str!("../../../schemas/vt/packs/en81.json"),
    include_str!("../../../schemas/vt/packs/en81_70.json"),
    include_str!("../../../schemas/vt/packs/england_adm.json"),
    include_str!("../../../schemas/vt/packs/practice_en.json"),
];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

// ---- Rule packs (ARC-VT-001) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Minimum,
    Accessible,
    Recommended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Verified,
    Secondary,
    Advisory,
}

/// How the car is driven.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Drive {
    /// Traction with no machine room (the usual low- and mid-rise passenger elevator).
    #[default]
    MrlTraction,
    /// Traction with a machine room.
    Traction,
    /// Hydraulic with a jack in a drilled hole.
    Hydraulic,
    /// Hydraulic with telescopic jacks beside the car.
    Holeless,
}

impl Drive {
    pub fn label(self) -> &'static str {
        match self {
            Drive::MrlTraction => "machine-room-less traction",
            Drive::Traction => "traction",
            Drive::Hydraulic => "hydraulic",
            Drive::Holeless => "holeless hydraulic",
        }
    }
}

/// How the doors open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DoorKind {
    /// Side opening (single-speed or two-speed): the opening is to one side of the car.
    #[default]
    Side,
    /// Centre opening: the opening is centred.
    Centre,
}

/// What an elevator or lift is for (ARC-VT-002, ARC-VT-005, ARC-VT-006).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ElevatorKind {
    #[default]
    Passenger,
    /// A hospital or service car, taking a bed or stretcher.
    Hospital,
    Freight,
    /// A motor-vehicle car (freight class B).
    Vehicle,
    /// Limited-use/limited-application (A17.1 Part 5.2).
    Lula,
    /// A private-residence elevator (A17.1 Part 5.3).
    Residence,
    /// A vertical platform lift (A18.1).
    PlatformLift,
    Dumbwaiter,
}

impl ElevatorKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Passenger => "Elevator",
            Self::Hospital => "Hospital Elevator",
            Self::Freight => "Freight Elevator",
            Self::Vehicle => "Vehicle Elevator",
            Self::Lula => "LULA Elevator",
            Self::Residence => "Home Elevator",
            Self::PlatformLift => "Platform Lift",
            Self::Dumbwaiter => "Dumbwaiter",
        }
    }

    /// It carries people (so accessibility, landings and the stretcher can apply).
    pub fn carries_people(self) -> bool {
        !matches!(self, Self::Freight | Self::Vehicle | Self::Dumbwaiter)
    }

    /// The stretcher rule counts it.
    pub fn stretcher_car(self) -> bool {
        matches!(self, Self::Passenger | Self::Hospital)
    }

    /// IFC's predefined type, and the object type for a user-defined one.
    pub fn ifc(self) -> (&'static str, Option<&'static str>) {
        match self {
            Self::PlatformLift => ("USERDEFINED", Some("PLATFORMLIFT")),
            Self::Dumbwaiter => ("USERDEFINED", Some("DUMBWAITER")),
            _ => ("ELEVATOR", None),
        }
    }
}

/// A freight car's loading class (A17.1 2.16.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoadingClass {
    A,
    B,
    C1,
    C2,
    C3,
}

/// What stands in each landing entrance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Entrance {
    /// A sliding automatic door.
    #[default]
    Sliding,
    /// A swinging door.
    Swing,
    /// An opening, for doors or gates not modelled yet (vertical bi-parting, slide-up).
    Opening,
}

/// Where an accessible-car option wants the door.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DoorPosition {
    Centred,
    Side,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AboveLast {
    pub per_kg: f64,
    pub add_m2: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CarArea {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Rated load (kg) and maximum area (m²).
    pub rows: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub above_last: Option<AboveLast>,
    pub source: String,
    pub confidence: Confidence,
}

impl CarArea {
    /// The maximum area for a load; none below the first row.
    pub fn max_area(&self, load: f64) -> Option<f64> {
        let first = self.rows.first()?;
        if load < first[0] - 1e-9 {
            return None;
        }
        for w in self.rows.windows(2) {
            let ([l0, a0], [l1, a1]) = (w[0], w[1]);
            if load <= l1 + 1e-9 {
                return Some(a0 + (a1 - a0) * (load - l0) / (l1 - l0));
            }
        }
        let last = self.rows.last()?;
        let more = self.above_last?;
        Some(last[1] + (load - last[0]) / more.per_kg * more.add_m2)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CarOption {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub door: DoorPosition,
    /// Omitted: the door is not checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door_width: Option<f64>,
    pub width: f64,
    pub depth: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccessibleCar {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The kinds it applies to; empty for passenger and hospital cars.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<ElevatorKind>,
    pub options: Vec<CarOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turning_circle: Option<f64>,
    pub source: String,
    pub confidence: Confidence,
}

/// Room for measuring error in a modelled size (0.5 mm).
const TOL: f64 = 0.0005;

impl AccessibleCar {
    pub fn applies(&self, kind: ElevatorKind) -> bool {
        if self.kinds.is_empty() { kind.stretcher_car() } else { self.kinds.contains(&kind) }
    }

    /// The option a car meets, if any (a label, or "turning circle").
    pub fn met_by(&self, car: V2, door: DoorKind, door_width: f64) -> Option<String> {
        for o in &self.options {
            let placed = match o.door {
                DoorPosition::Any => true,
                DoorPosition::Centred => door == DoorKind::Centre,
                DoorPosition::Side => true,
            };
            if placed && door_width + TOL >= o.door_width.unwrap_or(0.0) && car[0] + TOL >= o.width && car[1] + TOL >= o.depth {
                return Some(o.label.clone().unwrap_or_else(|| format!("{:.0} × {:.0} mm", o.width * 1000.0, o.depth * 1000.0)));
            }
        }
        let narrowest = self.options.iter().map(|o| o.door_width.unwrap_or(0.0)).fold(f64::MAX, f64::min);
        self.turning_circle
            .filter(|d| car[0].min(car[1]) + TOL >= *d && door_width + TOL >= narrowest)
            .map(|d| format!("{:.0} mm turning circle", d * 1000.0))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Stretcher {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub length: f64,
    pub width: f64,
    pub corner_radius: f64,
    pub min_storeys: u32,
    pub source: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Landing {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub width: f64,
    pub depth: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    pub source: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Band {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub min: f64,
    pub max: f64,
    pub source: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MinWidth {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub min: f64,
    pub source: String,
    pub confidence: Confidence,
}

/// A limit on a kind of elevator or lift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Limit {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub kinds: Vec<ElevatorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_area: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_load: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rise: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<f64>,
    pub source: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClassRow {
    pub class: LoadingClass,
    pub min_load_per_m2: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The least rated load per area for each freight loading class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FreightClasses {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub rows: Vec<ClassRow>,
    pub source: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatalogueCar {
    pub load: f64,
    #[serde(default)]
    pub kind: ElevatorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<LoadingClass>,
    pub door: DoorKind,
    pub width: f64,
    pub depth: f64,
    pub door_width: f64,
    /// Hoistway inside width and depth, in place of the allowances.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoistway: Option<V2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overhead: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub car_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sill: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrance: Option<Entrance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persons: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#use: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HoistwayAllowance {
    pub side_door_width: f64,
    pub centre_door_width: f64,
    pub depth: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rear_entrance_depth: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_below_depth: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PitOverhead {
    pub drives: Vec<Drive>,
    pub max_speed: f64,
    pub pit: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overhead: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overhead_above_car: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sizing {
    pub cars: Vec<CatalogueCar>,
    pub hoistway: HoistwayAllowance,
    pub pit_overhead: Vec<PitOverhead>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub car_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persons_kg: Option<f64>,
    pub source: String,
    pub confidence: Confidence,
}

/// A vertical-transport rule pack (`schemas/vt/reviera.vt-rules-1.schema.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Pack {
    pub format: String,
    pub id: String,
    pub title: String,
    pub jurisdiction: String,
    pub edition: String,
    pub layer: Layer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub car_area: Option<CarArea>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accessible_cars: Vec<AccessibleCar>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stretcher: Option<Stretcher>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing: Option<Landing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_buttons: Option<Band>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door_width: Option<MinWidth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizing: Option<Sizing>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limits: Vec<Limit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freight_classes: Option<FreightClasses>,
}

fn validator() -> &'static jsonschema::Validator {
    static V: OnceLock<jsonschema::Validator> = OnceLock::new();
    V.get_or_init(|| {
        let schema: Value = serde_json::from_str(SCHEMA).expect("the published schema is JSON");
        jsonschema::validator_for(&schema).expect("the published schema compiles")
    })
}

/// Reads and checks a pack.
pub fn parse(v: &Value) -> Result<Pack> {
    let errors: Vec<String> = validator().iter_errors(v).take(5).map(|e| format!("{} at {}", e, e.instance_path())).collect();
    if !errors.is_empty() {
        return Err(bad("vt.pack_schema", format!("the pack does not follow reviera.vt-rules-1: {}", errors.join("; "))));
    }
    let pack: Pack = serde_json::from_value(v.clone()).map_err(|e| bad("vt.pack_schema", e.to_string()))?;
    if let Some(a) = &pack.car_area
        && a.rows.windows(2).any(|w| w[1][0] <= w[0][0] || w[1][1] < w[0][1])
    {
        return Err(bad("vt.pack_range", format!("{}: rows must rise in load and not fall in area", a.id)));
    }
    if let Some(b) = &pack.call_buttons
        && b.min > b.max
    {
        return Err(bad("vt.pack_range", format!("{}: its minimum is above its maximum", b.id)));
    }
    if let Some(s) = &pack.sizing {
        if s.pit_overhead.iter().any(|r| r.overhead.is_none() == r.overhead_above_car.is_none()) {
            return Err(bad("vt.pack_range", "each pit_overhead row gives either overhead or overhead_above_car"));
        }
        if s.cars.windows(2).any(|w| w[1].load < w[0].load) {
            return Err(bad("vt.pack_range", "sizing cars must be listed by rising load"));
        }
    }
    Ok(pack)
}

/// The packs that ship with Reviera.
pub fn shipped() -> &'static [Pack] {
    static P: OnceLock<Vec<Pack>> = OnceLock::new();
    P.get_or_init(|| SHIPPED.iter().map(|t| parse(&serde_json::from_str(t).expect("a shipped pack is JSON")).expect("a shipped pack is valid")).collect())
}

fn project(store: &Store) -> Option<&crate::element::Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

fn project_id(txn: &Transaction<'_>) -> Result<ElementId> {
    project(txn.store()).map(crate::element::Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))
}

/// The project's own packs.
pub fn project_packs(store: &Store) -> Vec<Pack> {
    project(store).and_then(|e| e.param(PACKS_KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// Every pack the project can use: the shipped ones, each replaced by a project pack of its id,
/// then the project's others.
pub fn available(store: &Store) -> Vec<Pack> {
    let own = project_packs(store);
    let mut out: Vec<Pack> = shipped().iter().map(|s| own.iter().find(|p| p.id == s.id).unwrap_or(s).clone()).collect();
    out.extend(own.into_iter().filter(|p| !shipped().iter().any(|s| s.id == p.id)));
    out
}

/// The packs the project names, if it names any.
pub fn chosen(store: &Store) -> Option<Vec<String>> {
    project(store).and_then(|e| e.param(RULES_KEY)).and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// The packs chosen for a project that names none, from its building code: the US packs under
/// the IBC or IRC, England's under Approved Document K, and the EN packs otherwise.
pub fn automatic(store: &Store) -> Vec<String> {
    use crate::codes::Code;
    let ids: &[&str] = match crate::codes::settings(store).code {
        Code::Ibc | Code::Irc => &["us_ibc_ada", "us_a17", "us_a18", "practice_us"],
        Code::UkAdk => &["en81", "en81_70", "england_adm", "practice_en"],
        _ => &["en81", "en81_70", "practice_en"],
    };
    ids.iter().map(|s| (*s).to_owned()).collect()
}

/// The packs in use.
pub fn active(store: &Store) -> Vec<Pack> {
    let ids = chosen(store).unwrap_or_else(|| automatic(store));
    let all = available(store);
    ids.iter().filter_map(|id| all.iter().find(|p| &p.id == id).cloned()).collect()
}

/// A pack for listing.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PackInfo {
    #[serde(flatten)]
    pub pack: Pack,
    pub in_use: bool,
    /// It ships with Reviera (and is not replaced by the project's own).
    pub shipped: bool,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// `vt.rule_packs`.
pub(crate) fn list_rule_packs(store: &Store, _: Option<NoParams>) -> Result<Vec<PackInfo>> {
    let in_use: Vec<String> = active(store).into_iter().map(|p| p.id).collect();
    let own = project_packs(store);
    Ok(available(store)
        .into_iter()
        .map(|p| PackInfo { in_use: in_use.contains(&p.id), shipped: !own.iter().any(|o| o.id == p.id), pack: p })
        .collect())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPacksParams {
    /// The packs to use, by id; none to choose them from the building code.
    pub packs: Option<Vec<String>>,
}

/// Chooses the packs in use (`vt.set_rule_packs`).
pub(crate) fn set_rule_packs(txn: &mut Transaction<'_>, p: SetPacksParams) -> Result<()> {
    let all = available(txn.store());
    if let Some(ids) = &p.packs {
        for id in ids {
            if !all.iter().any(|a| &a.id == id) {
                let known: Vec<&str> = all.iter().map(|a| a.id.as_str()).collect();
                return Err(bad("vt.no_pack", format!("no rule pack {id:?}; there are: {}", known.join(", "))));
            }
        }
    }
    let project = project_id(txn)?;
    txn.modify(project, |e| match &p.packs {
        Some(ids) => {
            e.params.insert(RULES_KEY.into(), serde_json::to_value(ids).expect("serialises"));
        }
        None => {
            e.params.remove(RULES_KEY);
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddPackParams {
    /// A pack following `reviera.vt-rules-1`.
    pub pack: Value,
}

/// Adds a pack to the project, or replaces its pack (or a shipped one) of that id
/// (`vt.add_rule_pack`).
pub(crate) fn add_rule_pack(txn: &mut Transaction<'_>, p: AddPackParams) -> Result<String> {
    let pack = parse(&p.pack)?;
    let mut own = project_packs(txn.store());
    own.retain(|o| o.id != pack.id);
    let id = pack.id.clone();
    own.push(pack);
    let project = project_id(txn)?;
    let value = serde_json::to_value(&own).expect("serialises");
    txn.modify(project, |e| {
        e.params.insert(PACKS_KEY.into(), value);
    })?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemovePackParams {
    pub id: String,
}

/// Removes one of the project's own packs (`vt.remove_rule_pack`).
pub(crate) fn remove_rule_pack(txn: &mut Transaction<'_>, p: RemovePackParams) -> Result<()> {
    let mut own = project_packs(txn.store());
    let before = own.len();
    own.retain(|o| o.id != p.id);
    if own.len() == before {
        return Err(bad("vt.no_pack", format!("the project has no pack {:?} of its own", p.id)));
    }
    let project = project_id(txn)?;
    let value = serde_json::to_value(&own).expect("serialises");
    txn.modify(project, |e| {
        if own.is_empty() {
            e.params.remove(PACKS_KEY);
        } else {
            e.params.insert(PACKS_KEY.into(), value);
        }
    })
}

// ---- Sizing (ARC-VT-002) ----

/// What to size an elevator for. Everything left out comes from the sizing pack in use.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SizeParams {
    /// What it is for; default a passenger elevator.
    #[serde(default)]
    pub kind: ElevatorKind,
    /// A freight car's loading class; default the catalogue car's (A, or B for vehicles).
    pub class: Option<LoadingClass>,
    /// Rated load (kg). Omitted: the smallest car of its kind in the sizing pack that meets the
    /// accessibility packs and, where one is needed, takes the stretcher.
    pub load: Option<f64>,
    /// Rated speed (m/s); default 1.0, 0.5 for hydraulic drives, and the usual speed of a small
    /// lift (0.15 for LULAs and home elevators, 0.1 for platform lifts, 0.25 for dumbwaiters).
    pub speed: Option<f64>,
    #[serde(default)]
    pub drive: Drive,
    /// Door opening; omitted, the catalogue car's.
    pub door: Option<DoorKind>,
    /// Inside car width and depth (both or neither; with them, `load` is needed).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub car_width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub car_depth: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub car_height: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub door_width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub door_height: Option<f64>,
    /// How many levels it serves, for the stretcher rule; default 2.
    pub storeys: Option<u32>,
    /// Occupied space below the pit: counterweight safeties make the hoistway deeper.
    #[serde(default)]
    pub space_below: bool,
}

/// An elevator's sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sized {
    #[serde(default)]
    pub kind: ElevatorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<LoadingClass>,
    /// Rated load (kg).
    pub load: f64,
    pub persons: u32,
    pub speed: f64,
    pub drive: Drive,
    pub door: DoorKind,
    /// Inside car width, depth and clear height.
    pub car: [f64; 3],
    /// Door clear width and height.
    pub door_size: [f64; 2],
    #[serde(default)]
    pub entrance: Entrance,
    /// The entrances' bottom above each landing floor.
    #[serde(default)]
    pub sill: f64,
    /// Hoistway inside width and depth.
    pub hoistway: [f64; 2],
    pub pit: f64,
    /// Top landing floor to the underside of the structure above.
    pub overhead: f64,
    /// The pack the defaults came from.
    pub sizing_pack: String,
    /// What the sizing had to assume or could not meet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// The first sizing pack in use.
fn sizing_pack(packs: &[Pack]) -> Result<(&Pack, &Sizing)> {
    packs
        .iter()
        .find_map(|p| p.sizing.as_ref().map(|s| (p, s)))
        .ok_or_else(|| bad("vt.no_sizing", "no rule pack in use gives sizing defaults (add practice_us or practice_en)"))
}

/// Whether a car of `kind` meets every accessibility pack in use that applies to it.
fn accessible(packs: &[Pack], kind: ElevatorKind, car: V2, door: DoorKind, door_width: f64) -> bool {
    packs.iter().flat_map(|p| &p.accessible_cars).filter(|a| a.applies(kind)).all(|a| a.met_by(car, door, door_width).is_some())
}

/// The stretcher a car of `kind` serving `storeys` must take, from the packs in use.
fn stretcher_needed(packs: &[Pack], kind: ElevatorKind, storeys: u32) -> Option<(&Pack, &Stretcher)> {
    if !kind.stretcher_car() {
        return None;
    }
    packs.iter().find_map(|p| p.stretcher.as_ref().filter(|s| storeys >= s.min_storeys).map(|s| (p, s)))
}

/// Whether a `length × width` rectangle with rounded corners fits flat in a `w × d` car, straight
/// or turned. The door is not tried.
pub fn stretcher_fits(car: V2, length: f64, width: f64, radius: f64) -> bool {
    let r = radius.clamp(0.0, width.min(length) / 2.0);
    let (a, b) = (length - 2.0 * r, width - 2.0 * r);
    (0..=180).any(|k| {
        let t = std::f64::consts::FRAC_PI_2 * f64::from(k) / 180.0;
        let (c, s) = (t.cos(), t.sin());
        let x = a * c + b * s + 2.0 * r;
        let y = a * s + b * c + 2.0 * r;
        (x <= car[0] + TOL && y <= car[1] + TOL) || (x <= car[1] + TOL && y <= car[0] + TOL)
    })
}

/// Sizes an elevator from the packs in use.
pub fn size(packs: &[Pack], p: &SizeParams) -> Result<Sized> {
    let (pack, sz) = sizing_pack(packs)?;
    let kind = p.kind;
    let hydraulic = matches!(p.drive, Drive::Hydraulic | Drive::Holeless);
    // Small lifts' usual speeds (research/16 C1–C4): 30 fpm for LULAs and home elevators, 20 fpm
    // for platform lifts, 50 fpm for dumbwaiters.
    let usual = match kind {
        ElevatorKind::Lula | ElevatorKind::Residence => 0.15,
        ElevatorKind::PlatformLift => 0.1,
        ElevatorKind::Dumbwaiter => 0.25,
        _ if hydraulic => 0.5,
        _ => 1.0,
    };
    let speed = p.speed.unwrap_or(usual);
    if speed <= 0.0 {
        return Err(bad("vt.params", "the speed must be positive"));
    }
    let storeys = p.storeys.unwrap_or(2);
    let mut notes = Vec::new();
    let stretcher = stretcher_needed(packs, kind, storeys);
    let of_kind: Vec<&CatalogueCar> = sz.cars.iter().filter(|c| c.kind == kind).collect();
    // The catalogue car it is, or the nearest to the sizes given.
    let base: Option<&CatalogueCar> = match (p.car_width, p.car_depth) {
        (Some(_), Some(_)) => {
            let load = p.load.ok_or_else(|| bad("vt.params", "give the rated load with the car's width and depth"))?;
            of_kind.iter().rev().find(|c| c.load <= load + 1e-9).or(of_kind.first()).copied()
        }
        (None, None) => {
            if of_kind.is_empty() {
                return Err(bad("vt.no_sizing", format!("{} lists no {} car; give the car's width and depth", pack.id, kind.label().to_lowercase())));
            }
            let fits = |c: &CatalogueCar| {
                let door_w = p.door_width.unwrap_or(c.door_width);
                accessible(packs, kind, [c.width, c.depth], p.door.unwrap_or(c.door), door_w)
                    && stretcher.is_none_or(|(_, s)| stretcher_fits([c.width, c.depth], s.length, s.width, s.corner_radius))
            };
            let wanted: Vec<&CatalogueCar> = of_kind
                .iter()
                .copied()
                .filter(|c| p.load.is_none_or(|l| c.load + 1e-9 >= l))
                .filter(|c| p.door.is_none_or(|d| c.door == d))
                .filter(|c| p.class.is_none_or(|k| c.class.is_none_or(|ck| ck == k || k != LoadingClass::B)))
                .collect();
            Some(match wanted.iter().find(|c| fits(c)) {
                Some(c) => *c,
                None => {
                    let c = wanted.last().copied().or_else(|| of_kind.last().copied()).expect("not empty");
                    if wanted.is_empty() {
                        notes.push(format!("{} lists no {} car of {:.0} kg; its largest is used", pack.id, kind.label().to_lowercase(), p.load.unwrap_or(0.0)));
                    } else {
                        notes.push(format!("no {} car of {} meets every accessibility pack in use{}; the largest that matches is used", kind.label().to_lowercase(), pack.id, if stretcher.is_some() { " and takes the stretcher" } else { "" }));
                    }
                    c
                }
            })
        }
        _ => return Err(bad("vt.params", "give both the car's width and depth, or neither")),
    };
    let given = p.car_width.zip(p.car_depth);
    let (car_w, car_d) = given.or(base.map(|c| (c.width, c.depth))).ok_or_else(|| bad("vt.no_sizing", "no car to size from"))?;
    let load = p.load.or(base.map(|c| c.load)).unwrap_or_default();
    let door = p.door.or(base.map(|c| c.door)).unwrap_or_default();
    let door_w = p.door_width.or(base.map(|c| c.door_width)).unwrap_or(0.9).min(car_w);
    let car_h = p.car_height.or(base.and_then(|c| c.car_height)).or(sz.car_height).unwrap_or(2.2);
    let door_h = p.door_height.or(base.and_then(|c| c.door_height)).or(sz.door_height).unwrap_or(2.1);
    // The catalogue car's own hoistway, pit and overhead hold only for its own car.
    let own = base.filter(|_| given.is_none());
    let hoistway = match own.and_then(|c| c.hoistway) {
        Some(h) => [h[0], h[1] + if p.space_below { sz.hoistway.space_below_depth.unwrap_or(0.2) } else { 0.0 }],
        None => {
            let (cw, cd) = (base.map_or(car_w, |c| c.width), base.map_or(car_d, |c| c.depth));
            let by_car = base.and_then(|c| c.hoistway).map(|h| [h[0] - cw, h[1] - cd]);
            let allowance = by_car.unwrap_or([
                match door {
                    DoorKind::Side => sz.hoistway.side_door_width,
                    DoorKind::Centre => sz.hoistway.centre_door_width,
                },
                sz.hoistway.depth,
            ]);
            let below = if p.space_below { sz.hoistway.space_below_depth.unwrap_or(0.2) } else { 0.0 };
            [car_w + allowance[0], car_d + allowance[1] + below]
        }
    };
    let (pit, overhead) = match base.and_then(|c| c.pit.zip(c.overhead)) {
        Some((pit, o)) => (pit, o + (car_h - base.and_then(|c| c.car_height).unwrap_or(car_h)).max(0.0)),
        None => {
            let row = sz
                .pit_overhead
                .iter()
                .filter(|r| r.drives.contains(&p.drive) && speed <= r.max_speed + 1e-9)
                .min_by(|a, b| a.max_speed.total_cmp(&b.max_speed))
                .ok_or_else(|| bad("vt.no_sizing", format!("{} gives no pit and overhead for a {} elevator at {speed} m/s", pack.id, p.drive.label())))?;
            let overhead = match (row.overhead, row.overhead_above_car) {
                (Some(o), _) => o + (car_h - sz.car_height.unwrap_or(car_h)).max(0.0),
                (None, Some(a)) => car_h + a,
                _ => unreachable!("checked when the pack was read"),
            };
            (row.pit, overhead)
        }
    };
    let class = match kind {
        ElevatorKind::Freight | ElevatorKind::Vehicle => p.class.or(base.and_then(|c| c.class)).or(Some(if kind == ElevatorKind::Vehicle { LoadingClass::B } else { LoadingClass::A })),
        _ => None,
    };
    let persons = if kind.carries_people() { (load / sz.persons_kg.unwrap_or(75.0)).floor() as u32 } else { 0 };
    Ok(Sized {
        kind,
        class,
        load,
        persons,
        speed,
        drive: p.drive,
        door,
        car: [car_w, car_d, car_h],
        door_size: [door_w, door_h],
        entrance: base.and_then(|c| c.entrance).unwrap_or_default(),
        sill: base.and_then(|c| c.sill).unwrap_or(0.0),
        hoistway,
        pit,
        overhead,
        sizing_pack: pack.id.clone(),
        notes,
    })
}

/// `elevator.size`: the sizes an elevator would get, without making it.
pub(crate) fn size_command(store: &Store, p: SizeParams) -> Result<Sized> {
    size(&active(store), &p)
}

// ---- The assembly (ARC-VT-002) ----

/// An elevator's data, kept on its car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Elevator {
    #[serde(flatten)]
    pub sized: Sized,
    /// The middle of the hoistway's inside front face (metres).
    pub point: V2,
    /// Counter-clockwise (radians).
    pub rotation: f64,
    /// The levels served, lowest first (GlobalIds).
    pub levels: Vec<String>,
    pub wall_thickness: f64,
    /// Members (GlobalIds): the shaft opening, hoistway walls and landing entrances.
    pub shaft: String,
    #[serde(default)]
    pub walls: Vec<String>,
    #[serde(default)]
    pub entrances: Vec<String>,
    /// Occupied space below the pit.
    #[serde(default)]
    pub space_below: bool,
    /// Across the hoistway, the middle of the landing entrances as built: the car door's, moved
    /// where the front wall leaves too little room for the leaves to slide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrance_x: Option<f64>,
}

impl Elevator {
    /// A point of the elevator's frame in plan.
    pub fn at(&self, x: f64, y: f64) -> V2 {
        let (s, c) = self.rotation.sin_cos();
        [self.point[0] + x * c - y * s, self.point[1] + x * s + y * c]
    }

    /// Across the hoistway, the middle of the door opening.
    pub fn door_x(&self) -> f64 {
        if let Some(x) = self.entrance_x {
            return x;
        }
        match self.sized.door {
            DoorKind::Centre => 0.0,
            DoorKind::Side => -(self.sized.car[0] - self.sized.door_size[0]) / 2.0,
        }
    }

    /// Where the car's front is, into the hoistway.
    pub fn car_y(&self) -> f64 {
        car_y(&self.sized)
    }

    /// The hoistway's inside outline in plan.
    pub fn hoistway_outline(&self) -> Vec<V2> {
        let [w, d] = self.sized.hoistway;
        vec![self.at(-w / 2.0, 0.0), self.at(w / 2.0, 0.0), self.at(w / 2.0, d), self.at(-w / 2.0, d)]
    }
}

fn car_y(s: &Sized) -> f64 {
    ((s.hoistway[1] - s.car[1]) * 0.55).max(0.0)
}

/// An elevator's data, if the element is a car.
pub fn elevator(e: &crate::element::Element) -> Option<Elevator> {
    serde_json::from_value(e.param(DATA_KEY)?.clone()).ok()
}

/// Every elevator: its car and data.
pub fn elevators(store: &Store) -> Vec<(ElementId, Elevator)> {
    store.iter().filter_map(|e| Some((e.id(), elevator(e)?))).collect()
}

fn by_guid(store: &Store, guid: &str) -> Option<ElementId> {
    store.resolve(&ElementRef::Guid(GlobalId::parse(guid)?))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The middle of the hoistway's inside front face, the entrance line (metres).
    pub point: V2,
    /// Turns the elevator counter-clockwise; at 0 the landing is toward −y.
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
    /// The lowest level served.
    #[serde(alias = "level")]
    pub base_level: ElementRef,
    /// The highest level served; every level between is served too.
    pub top_level: ElementRef,
    #[serde(flatten)]
    pub size: SizeParams,
    /// Make the hoistway walls (default true).
    pub walls: Option<bool>,
    /// Hoistway wall thickness (default 0.2 m).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub wall_thickness: Option<f64>,
}

/// What `elevator.create` made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Created {
    /// The car, which stands for the elevator.
    pub elevator: ElementId,
    pub shaft: ElementId,
    pub walls: Vec<ElementId>,
    pub entrances: Vec<ElementId>,
    #[serde(flatten)]
    pub sized: Sized,
}

/// Triangles of a box.
fn cuboid(x: V2, y: V2, z: V2) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let p: Vec<[f64; 3]> = (0..8).map(|k| [if k & 1 == 0 { x[0] } else { x[1] }, if k & 2 == 0 { y[0] } else { y[1] }, if k & 4 == 0 { z[0] } else { z[1] }]).collect();
    let t = vec![[0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4], [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5]];
    (p, t)
}

/// The car type for these sizes, made the first time.
fn car_type(txn: &mut Transaction<'_>, s: &Sized) -> Result<ElementId> {
    let name = format!("{} car {:.0} × {:.0} × {:.0}", s.kind.label(), s.car[0] * 1000.0, s.car[1] * 1000.0, s.car[2] * 1000.0);
    if let Some(e) = txn.store().iter().find(|e| e.name.as_deref() == Some(name.as_str()) && crate::mesh_objects::mesh_object_type(e).is_some()) {
        return Ok(e.id());
    }
    let (p, t) = cuboid([-s.car[0] / 2.0, s.car[0] / 2.0], [0.0, s.car[1]], [0.0, s.car[2]]);
    let (predefined, object_type) = s.kind.ifc();
    let id = crate::mesh_objects::create_generated_type(txn, &name, crate::mesh_objects::Category::Transport, &p, &t, [0.72, 0.74, 0.78, 1.0], Vec::new(), Some(predefined))?;
    if let Some(o) = object_type {
        txn.modify(id, |e| {
            e.params.insert("@ElementType".into(), o.into());
        })?;
    }
    Ok(id)
}

/// The hoistway wall type of this thickness, made the first time.
fn wall_type(txn: &mut Transaction<'_>, t: f64) -> Result<ElementId> {
    let name = format!("Elevator Hoistway {:.0} mm", t * 1000.0);
    if let Some(e) = txn.store().iter().find(|e| e.class == "IfcWallType" && e.name.as_deref() == Some(name.as_str())) {
        return Ok(e.id());
    }
    txn.create(
        crate::element::NewElement::new("IfcWallType")
            .name(name)
            .param("@PredefinedType", "STANDARD")
            .param(crate::walls::RECIPE_KEY, json!({ "kind": crate::walls::WALL_TYPE_RECIPE, "width": t })),
    )
}

/// The landing entrance door type, made the first time: sliding, automatic.
fn entrance_type(txn: &mut Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn.store().iter().find(|e| e.class == "IfcDoorType" && e.name.as_deref() == Some(ENTRANCE_TYPE)) {
        return Ok(e.id());
    }
    let recipe = crate::inserts::InsertType {
        operation: crate::inserts::doors::Operation::Sliding,
        automatic: true,
        ..crate::inserts::InsertType::default_for(crate::inserts::Kind::Door)
    };
    txn.create(
        crate::element::NewElement::new("IfcDoorType")
            .name(ENTRANCE_TYPE)
            .param("@PredefinedType", "DOOR")
            .param(crate::walls::RECIPE_KEY, serde_json::to_value(recipe).expect("serialises")),
    )
}

/// The levels from `base` to `top`, lowest first.
fn served(store: &Store, base: &ElementRef, top: &ElementRef) -> Result<Vec<crate::levels::LevelInfo>> {
    let level = |r: &ElementRef| {
        store.resolve(r).filter(|id| store.get(*id).is_some_and(|e| e.class == crate::levels::CLASS)).ok_or_else(|| bad("vt.no_level", "no such level"))
    };
    let (b, t) = (level(base)?, level(top)?);
    let all = crate::levels::list(store);
    let z = |id: ElementId| all.iter().find(|l| l.id == id).map(|l| l.elevation).unwrap_or_default();
    let (z0, z1) = (z(b), z(t));
    if z1 <= z0 + 1e-6 {
        return Err(bad("vt.params", "the top level must be above the base level"));
    }
    Ok(all.into_iter().filter(|l| l.elevation >= z0 - 1e-6 && l.elevation <= z1 + 1e-6).collect())
}

fn measure(kind: &str, v: impl Into<Value>) -> Value {
    json!({ "type": kind, "value": v.into() })
}

/// Makes an elevator: car, shaft, hoistway walls and landing entrances (`elevator.create`).
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Created> {
    let store = txn.store().clone();
    let levels = served(&store, &p.base_level, &p.top_level)?;
    let packs = active(&store);
    let sized = size(&packs, &SizeParams { storeys: Some(levels.len() as u32), ..p.size.clone() })?;
    let t = p.wall_thickness.unwrap_or(0.2);
    if t <= 0.0 {
        return Err(bad("vt.params", "the wall thickness must be positive"));
    }
    let (base, top) = (&levels[0], levels.last().expect("two levels at least"));
    let mut el = Elevator {
        sized: sized.clone(),
        point: p.point,
        rotation: p.rotation,
        levels: levels.iter().map(|l| l.guid.clone()).collect(),
        wall_thickness: t,
        shaft: String::new(),
        walls: Vec::new(),
        entrances: Vec::new(),
        space_below: p.size.space_below,
        entrance_x: None,
    };

    // The car, on the lowest level.
    let ty = car_type(txn, &sized)?;
    let car = crate::mesh_objects::place(
        txn,
        crate::mesh_objects::PlaceParams {
            type_id: ElementRef::Id(ty),
            point: el.at(0.0, el.car_y()),
            level: ElementRef::Id(base.id),
            offset: 0.0,
            rotation: p.rotation,
            mirror: false,
            scale: 1.0,
        },
    )?;
    let car_guid = txn.store().get(car).expect("made").guid().to_string();
    let member = |txn: &mut Transaction<'_>, id: ElementId| -> Result<String> {
        let g = car_guid.clone();
        txn.modify(id, |e| {
            e.params.insert(MEMBER_KEY.into(), g.into());
        })?;
        Ok(txn.store().get(id).expect("made").guid().to_string())
    };

    // The shaft: pit floor to the overhead above the top landing. With walls it takes their
    // outer faces, so that the floors stop at the continuous hoistway walls.
    let make_walls = p.walls.unwrap_or(true);
    let outline = if make_walls {
        let [w, d] = sized.hoistway;
        vec![el.at(-w / 2.0 - t, -t), el.at(w / 2.0 + t, -t), el.at(w / 2.0 + t, d + t), el.at(-w / 2.0 - t, d + t)]
    } else {
        el.hoistway_outline()
    };
    let shaft = crate::slab_openings::create_shaft(
        txn,
        crate::slab_openings::CreateShaftParams {
            loops: vec![outline],
            curves: Vec::new(),
            base_level: ElementRef::Id(base.id),
            base_offset: -sized.pit,
            top_constraint: crate::walls::TopConstraint::Level(crate::walls::ElementRefValue::Id(top.id)),
            top_offset: sized.overhead,
        },
    )?;
    txn.modify(shaft, |e| e.name = Some("Elevator Hoistway".into()))?;
    el.shaft = member(txn, shaft)?;

    // The walls round it, and an entrance at each level in the front wall.
    let mut walls = Vec::new();
    let mut entrances = Vec::new();
    if make_walls {
        let wt = wall_type(txn, t)?;
        let [w, d] = sized.hoistway;
        let h = t / 2.0;
        let corners = [el.at(-w / 2.0 - h, -h), el.at(w / 2.0 + h, -h), el.at(w / 2.0 + h, d + h), el.at(-w / 2.0 - h, d + h)];
        for k in 0..4 {
            let args = json!({
                "curve": { "line": { "start": corners[k], "end": corners[(k + 1) % 4] } },
                "base_level": base.id.0,
                "base_offset": -sized.pit,
                "top_constraint": { "level": top.id.0 },
                "top_offset": sized.overhead,
                "type_id": wt.0,
            });
            let id = crate::walls::create(txn, serde_json::from_value(args).map_err(|e| bad("vt.params", e.to_string()))?)?;
            el.walls.push(member(txn, id)?);
            walls.push(id);
        }
        let front = walls[0];
        let length = w + t;
        let dw = sized.door_size[0];
        // A sliding door's leaves slide over the wall face either side of the opening, half the
        // width each, inside the hoistway's inside face (the side walls take the ends); other
        // entrances need only their own width.
        let half_of = |e: Entrance| match e {
            Entrance::Sliding => dw + crate::inserts::SLIDE_LAP + 0.005,
            _ => dw / 2.0 + 0.06,
        };
        // Side-opening (telescopic) doors stack inside the hoistway, which Reviera's bi-parting
        // sliding door cannot show: where its leaves would not fit the front wall, the entrance
        // is an opening.
        if el.sized.entrance == Entrance::Sliding && w < 2.0 * half_of(Entrance::Sliding) - 1e-9 {
            el.sized.entrance = Entrance::Opening;
            el.sized.notes.push(format!("the {:.0} mm doors' leaves would not fit the front wall as a bi-parting door, so each entrance is an opening until telescopic doors are modelled", dw * 1000.0));
        }
        let half = half_of(el.sized.entrance);
        let along = (w / 2.0 + h + el.door_x()).clamp(h + half, length - h - half);
        el.entrance_x = Some(along - w / 2.0 - h);
        let et = match el.sized.entrance {
            Entrance::Sliding => Some(entrance_type(txn)?),
            _ => None,
        };
        for l in &levels {
            let id = match el.sized.entrance {
                Entrance::Opening => {
                    let args = json!({
                        "wall_id": front.0,
                        "u0": along - dw / 2.0,
                        "u1": along + dw / 2.0,
                        "base_level": l.id.0,
                        "base_offset": sized.sill,
                        "top_constraint": { "unconnected": sized.door_size[1] },
                    });
                    crate::wall_openings::create(txn, serde_json::from_value(args).map_err(|e| bad("vt.params", e.to_string()))?)?
                }
                _ => {
                    let mut args = json!({
                        "host_wall_id": front.0,
                        "point_along": along,
                        // A sill is measured from the wall's base level, not its offset bottom in
                        // the pit.
                        "sill_height": l.elevation - base.elevation + sized.sill,
                        "width": dw,
                        "height": sized.door_size[1],
                        "fit": false,
                    });
                    if let Some(et) = et {
                        args["type_id"] = json!(et.0);
                    }
                    let door = crate::inserts::place(txn, crate::inserts::Kind::Door, serde_json::from_value(args).map_err(|e| bad("vt.params", e.to_string()))?)?;
                    // A swinging door opens out onto the landing, not into the hoistway.
                    if let Some(sw) = crate::clearances::all(txn.store()).into_iter().find(|v| v.element == door && v.kind == ClearanceKind::DoorSwing)
                        && let Some((lo, hi)) = sw.body().bounds()
                    {
                        let mid = [(lo[0] + hi[0]) / 2.0 - el.point[0], (lo[1] + hi[1]) / 2.0 - el.point[1]];
                        let (sin, cos) = el.rotation.sin_cos();
                        if -mid[0] * sin + mid[1] * cos > 0.0 {
                            crate::inserts::flip(txn, serde_json::from_value(json!({ "element_ids": [door.0] })).map_err(|e| bad("vt.params", e.to_string()))?, false)?;
                        }
                    }
                    door
                }
            };
            txn.modify(id, |e| e.name = Some(format!("{} entrance, {}", sized.kind.label(), l.name)))?;
            el.entrances.push(member(txn, id)?);
            entrances.push(id);
        }
    }

    let data = serde_json::to_value(&el).expect("serialises");
    let props = properties(&el);
    txn.modify(car, |e| {
        e.name = Some(el.sized.kind.label().into());
        e.params.insert(DATA_KEY.into(), data);
        e.params.extend(props);
    })?;
    Ok(Created { elevator: car, shaft, walls, entrances, sized: el.sized.clone() })
}

/// An elevator's property sets: IFC's capacity and clear car size, and `Reviera_Elevator`.
pub fn properties(el: &Elevator) -> Vec<(String, Value)> {
    let s = &el.sized;
    let label = |v: Value| measure("IfcLabel", v);
    vec![
        ("Pset_TransportElementCommon.CapacityPeople".into(), measure("IfcCountMeasure", s.persons)),
        ("Pset_TransportElementCommon.CapacityWeight".into(), measure("IfcMassMeasure", s.load)),
        ("Pset_TransportElementElevator.ClearWidth".into(), measure("IfcPositiveLengthMeasure", s.car[0])),
        ("Pset_TransportElementElevator.ClearDepth".into(), measure("IfcPositiveLengthMeasure", s.car[1])),
        ("Pset_TransportElementElevator.ClearHeight".into(), measure("IfcPositiveLengthMeasure", s.car[2])),
        ("Pset_TransportElementElevator.FireFightingLift".into(), measure("IfcBoolean", false)),
        ("Reviera_Elevator.RatedSpeed".into(), measure("IfcLinearVelocityMeasure", s.speed)),
        ("Reviera_Elevator.DriveType".into(), label(serde_json::to_value(s.drive).expect("serialises"))),
        ("Reviera_Elevator.DoorOperation".into(), label(serde_json::to_value(s.door).expect("serialises"))),
        ("Reviera_Elevator.DoorClearWidth".into(), measure("IfcPositiveLengthMeasure", s.door_size[0])),
        ("Reviera_Elevator.DoorClearHeight".into(), measure("IfcPositiveLengthMeasure", s.door_size[1])),
        ("Reviera_Elevator.HoistwayWidth".into(), measure("IfcPositiveLengthMeasure", s.hoistway[0])),
        ("Reviera_Elevator.HoistwayDepth".into(), measure("IfcPositiveLengthMeasure", s.hoistway[1])),
        ("Reviera_Elevator.PitDepth".into(), measure("IfcPositiveLengthMeasure", s.pit)),
        ("Reviera_Elevator.Overhead".into(), measure("IfcPositiveLengthMeasure", s.overhead)),
        ("Reviera_Elevator.StopsServed".into(), measure("IfcCountMeasure", el.levels.len())),
        ("Reviera_Elevator.SizingPack".into(), label(s.sizing_pack.clone().into())),
        ("Reviera_Elevator.Kind".into(), label(serde_json::to_value(s.kind).expect("serialises"))),
    ]
    .into_iter()
    .chain(s.class.map(|c| ("Reviera_Elevator.LoadingClass".into(), label(serde_json::to_value(c).expect("serialises")))))
    .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElevatorParams {
    /// The elevator's car.
    pub elevator: ElementRef,
}

fn car_of(store: &Store, r: &ElementRef) -> Result<(ElementId, Elevator)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
    let e = store.get(id).expect("resolved");
    // A member names its car.
    let car = match e.param(MEMBER_KEY).and_then(Value::as_str) {
        Some(g) => by_guid(store, g).ok_or_else(|| bad("vt.not_elevator", "the elevator this belonged to is gone"))?,
        None => id,
    };
    let data = store.get(car).and_then(elevator).ok_or_else(|| bad("vt.not_elevator", format!("element {id} is not part of an elevator")))?;
    Ok((car, data))
}

/// Deletes an elevator: its entrances, walls, shaft and car (`elevator.delete`).
pub(crate) fn delete(txn: &mut Transaction<'_>, p: ElevatorParams) -> Result<usize> {
    let store = txn.store().clone();
    let (car, el) = car_of(&store, &p.elevator)?;
    let mut n = 0;
    for g in el.entrances.iter().chain(&el.walls).chain(std::iter::once(&el.shaft)) {
        if let Some(id) = by_guid(txn.store(), g) {
            txn.delete(id)?;
            n += 1;
        }
    }
    txn.delete(car)?;
    Ok(n + 1)
}

/// An elevator for listing.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ElevatorInfo {
    /// The car, which stands for the elevator.
    pub elevator: ElementId,
    pub name: String,
    /// The levels served, lowest first (names).
    pub levels: Vec<String>,
    #[serde(flatten)]
    pub data: Elevator,
}

/// `elevator.list`.
pub(crate) fn list(store: &Store, _: Option<NoParams>) -> Result<Vec<ElevatorInfo>> {
    let names = crate::levels::list(store);
    Ok(elevators(store)
        .into_iter()
        .map(|(car, data)| ElevatorInfo {
            elevator: car,
            name: store.get(car).and_then(|e| e.name.clone()).unwrap_or_default(),
            levels: data.levels.iter().map(|g| names.iter().find(|l| &l.guid == g).map_or_else(|| g.clone(), |l| l.name.clone())).collect(),
            data,
        })
        .collect())
}

// ---- Clear zones (ARC-VT-003) ----

/// A box in an elevator's frame, extruded from `z0` to `z1`.
fn frame_box(el: &Elevator, x: V2, y: V2, z0: f64, z1: f64) -> crate::bodies::Mesh {
    let ring = [el.at(x[0], y[0]), el.at(x[1], y[0]), el.at(x[1], y[1]), el.at(x[0], y[1])];
    crate::bodies::extrude(&ring, &[], z0, z1)
}

/// Every elevator's clear zones: its hoistway, and the landing in front of each entrance under
/// each pack in use that gives one.
pub fn zones(store: &Store) -> Vec<Clearance> {
    let packs = active(store);
    let levels = crate::levels::list(store);
    let mut out = Vec::new();
    for (car, el) in elevators(store) {
        let z = |g: &str| levels.iter().find(|l| l.guid == g).map(|l| l.elevation);
        let (Some(z0), Some(z1)) = (el.levels.first().and_then(|g| z(g)), el.levels.last().and_then(|g| z(g))) else {
            continue;
        };
        let [w, d] = el.sized.hoistway;
        out.push(Clearance {
            element: car,
            kind: ClearanceKind::Hoistway,
            rule: "hoistway".into(),
            mesh: frame_box(&el, [-w / 2.0, w / 2.0], [0.0, d], z0 - el.sized.pit, z1 + el.sized.overhead),
            door_swing_forbidden: false,
        });
        for pack in &packs {
            let Some(l) = pack.landing.as_ref().filter(|_| el.sized.kind.carries_people()) else { continue };
            let x = el.door_x();
            let front = if el.walls.is_empty() { 0.0 } else { -el.wall_thickness };
            for g in &el.levels {
                let Some(zl) = z(g) else { continue };
                out.push(Clearance {
                    element: car,
                    kind: ClearanceKind::ElevatorLanding,
                    rule: format!("{} {}", pack.id, l.id),
                    mesh: frame_box(&el, [x - l.width / 2.0, x + l.width / 2.0], [front - l.depth, front], zl, zl + l.height.unwrap_or(LANDING_HEIGHT)),
                    door_swing_forbidden: false,
                });
            }
        }
    }
    out
}

/// Whether an element may stand in an elevator's hoistway: the elevator's own members and cars.
pub fn may_share(store: &Store, other: ElementId) -> bool {
    store.get(other).is_some_and(|e| e.param(MEMBER_KEY).is_some() || e.param(DATA_KEY).is_some() || crate::slab_openings::shaft_recipe(e).is_some())
}

// ---- Checks (ARC-VT-004) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// The car is bigger than its rated load allows.
    CarArea,
    /// The car or its door is smaller than an accessibility pack asks.
    AccessibleCar,
    /// No elevator serving every floor takes the stretcher.
    Stretcher,
    /// The rated load is below the pack's table.
    LoadBelowTable,
    /// A freight car's rated load is below its loading class's least load for its area.
    FreightClass,
    /// The car's area, load, height or the rise is beyond its kind's limit.
    Limit,
}

/// One finding of `elevator.check`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Finding {
    pub severity: Severity,
    pub kind: FindingKind,
    /// The elevator's car.
    pub elevator: ElementId,
    pub pack: String,
    pub edition: String,
    pub layer: Layer,
    pub rule: String,
    pub description: String,
    pub confidence: Confidence,
    pub source: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<f64>,
}

#[allow(clippy::too_many_arguments)]
fn finding(pack: &Pack, kind: FindingKind, elevator: ElementId, rule: &str, description: &Option<String>, confidence: Confidence, source: &str, message: String) -> Finding {
    let note = pack.layer == Layer::Recommended || confidence == Confidence::Advisory;
    Finding {
        severity: if note { Severity::Note } else { Severity::Error },
        kind,
        elevator,
        pack: pack.id.clone(),
        edition: pack.edition.clone(),
        layer: pack.layer,
        rule: rule.to_owned(),
        description: description.clone().unwrap_or_default(),
        confidence,
        source: source.to_owned(),
        message,
        measured: None,
        required: None,
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// The elevators to check (cars or members); omitted, every elevator.
    #[serde(default)]
    pub elevators: Vec<ElementRef>,
}

/// Checks elevators against the packs in use (`elevator.check`). Intrusions into hoistways and
/// landings are reported by `clearance.check`.
pub(crate) fn check(store: &Store, p: Option<CheckParams>) -> Result<Vec<Finding>> {
    let p = p.unwrap_or_default();
    let packs = active(store);
    let all = elevators(store);
    let chosen: Vec<(ElementId, Elevator)> = if p.elevators.is_empty() { all.clone() } else { p.elevators.iter().map(|r| car_of(store, r)).collect::<Result<_>>()? };
    let mut out = Vec::new();
    for (car, el) in &chosen {
        let s = &el.sized;
        let area = s.car[0] * s.car[1];
        let levels = crate::levels::list(store);
        let z = |g: &String| levels.iter().find(|l| &l.guid == g).map(|l| l.elevation);
        let rise = match (el.levels.first().and_then(z), el.levels.last().and_then(z)) {
            (Some(a), Some(b)) => b - a,
            _ => 0.0,
        };
        for pack in &packs {
            if let Some(a) = pack.car_area.as_ref().filter(|_| s.kind.stretcher_car()) {
                match a.max_area(s.load) {
                    Some(max) if area > max + 1e-4 => {
                        let mut f = finding(pack, FindingKind::CarArea, *car, &a.id, &a.description, a.confidence, &a.source, format!("the car's {area:.2} m² is more than the {max:.2} m² allowed for {:.0} kg", s.load));
                        f.measured = Some(area);
                        f.required = Some(max);
                        out.push(f);
                    }
                    None => out.push(finding(pack, FindingKind::LoadBelowTable, *car, &a.id, &a.description, Confidence::Advisory, &a.source, format!("{:.0} kg is below the table's first row; the area was not checked", s.load))),
                    _ => {}
                }
            }
            for a in pack.accessible_cars.iter().filter(|a| a.applies(s.kind)) {
                if a.met_by([s.car[0], s.car[1]], s.door, s.door_size[0]).is_some() {
                    continue;
                }
                let options: Vec<String> = a
                    .options
                    .iter()
                    .map(|o| match o.door_width {
                        Some(d) => format!("{:.0} × {:.0} mm with a {:.0} mm door", o.width * 1000.0, o.depth * 1000.0, d * 1000.0),
                        None => format!("{:.0} × {:.0} mm", o.width * 1000.0, o.depth * 1000.0),
                    })
                    .collect();
                out.push(finding(
                    pack,
                    FindingKind::AccessibleCar,
                    *car,
                    &a.id,
                    &a.description,
                    a.confidence,
                    &a.source,
                    format!("the car ({:.0} × {:.0} mm, {:.0} mm door) is smaller than any of: {}", s.car[0] * 1000.0, s.car[1] * 1000.0, s.door_size[0] * 1000.0, options.join("; ")),
                ));
            }
            if let (Some(fc), Some(class)) = (&pack.freight_classes, s.class)
                && let Some(row) = fc.rows.iter().find(|r| r.class == class)
            {
                let least = area * row.min_load_per_m2;
                if s.load + 1e-6 < least {
                    let mut f = finding(pack, FindingKind::FreightClass, *car, &fc.id, &fc.description, fc.confidence, &fc.source, format!("a class {} car of {area:.2} m² needs a rated load of at least {least:.0} kg; it has {:.0} kg", serde_json::to_value(class).expect("serialises").as_str().unwrap_or_default().to_uppercase(), s.load));
                    f.measured = Some(s.load);
                    f.required = Some(least);
                    out.push(f);
                }
            }
            for l in pack.limits.iter().filter(|l| l.kinds.contains(&s.kind)) {
                let checks = [("car area", area, l.max_area, "m²"), ("rated load", s.load, l.max_load, "kg"), ("rise", rise, l.max_rise, "m"), ("car height", s.car[2], l.max_height, "m")];
                for (what, value, max, unit) in checks {
                    let Some(max) = max.filter(|m| value > m + 1e-4) else { continue };
                    let mut f = finding(pack, FindingKind::Limit, *car, &l.id, &l.description, l.confidence, &l.source, format!("its {what} of {value:.2} {unit} is more than the {max:.2} {unit} a {} may have", s.kind.label().to_lowercase()));
                    f.measured = Some(value);
                    f.required = Some(max);
                    out.push(f);
                }
            }
        }
    }
    // The stretcher: one elevator serving every floor the most-storeys elevator serves must take
    // it, once per pack.
    if let Some((tall, el)) = all.iter().filter(|(_, e)| e.sized.kind.stretcher_car()).max_by_key(|(_, e)| e.levels.len())
        && (p.elevators.is_empty() || chosen.iter().any(|(c, _)| c == tall))
    {
        for pack in &packs {
            let Some(st) = pack.stretcher.as_ref().filter(|s| el.levels.len() as u32 >= s.min_storeys) else { continue };
            let serving: Vec<&(ElementId, Elevator)> = all.iter().filter(|(_, e)| e.sized.kind.stretcher_car() && el.levels.iter().all(|g| e.levels.contains(g))).collect();
            if !serving.iter().any(|(_, e)| stretcher_fits([e.sized.car[0], e.sized.car[1]], st.length, st.width, st.corner_radius)) {
                out.push(finding(
                    pack,
                    FindingKind::Stretcher,
                    *tall,
                    &st.id,
                    &st.description,
                    st.confidence,
                    &st.source,
                    format!("no elevator serving all {} storeys takes a {:.0} × {:.0} mm stretcher", el.levels.len(), st.length * 1000.0, st.width * 1000.0),
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_packs_are_valid_and_cite_registered_sources() {
        let register = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../design_documents/SOURCE_REGISTER.md")).expect("the register is there");
        for p in shipped() {
            let sources = [p.car_area.as_ref().map(|a| &a.source), p.sizing.as_ref().map(|a| &a.source), p.stretcher.as_ref().map(|a| &a.source), p.landing.as_ref().map(|a| &a.source), p.call_buttons.as_ref().map(|a| &a.source), p.sizing.as_ref().map(|a| &a.source)];
            let more = p.accessible_cars.iter().map(|a| &a.source).chain(p.limits.iter().map(|l| &l.source)).chain(p.freight_classes.as_ref().map(|f| &f.source));
            for s in sources.into_iter().flatten().chain(more).flat_map(|s| s.split(' ')) {
                assert!(register.contains(&format!("| {s} |")), "{}: {s} is not in the register", p.id);
            }
        }
    }

    #[test]
    fn the_published_definitions_cover_what_is_written() {
        let defs: Value = serde_json::from_str(include_str!("../../../schemas/psets/reviera-psets-1.json")).unwrap();
        let set = defs["property_sets"].as_array().unwrap().iter().find(|s| s["name"] == "Reviera_Elevator").expect("Reviera_Elevator");
        let us: Vec<Pack> = shipped().iter().filter(|p| p.id == "practice_us").cloned().collect();
        let el = Elevator { sized: size(&us, &SizeParams::default()).unwrap(), point: [0.0; 2], rotation: 0.0, levels: vec![], wall_thickness: 0.2, shaft: String::new(), walls: vec![], entrances: vec![], space_below: false, entrance_x: None };
        for (key, value) in properties(&el) {
            let Some(name) = key.strip_prefix("Reviera_Elevator.") else { continue };
            let def = set["properties"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("{name} is not published"));
            assert_eq!(def["measure"], value["type"], "{name}");
        }
    }

    #[test]
    fn a_bad_pack_is_refused() {
        let mut v: Value = serde_json::from_str(SHIPPED[1]).unwrap();
        v["car_area"]["rows"] = json!([[1000, 2.0], [900, 2.2]]);
        assert_eq!(parse(&v).unwrap_err().code, "vt.pack_range");
        v["layer"] = json!("optional");
        assert_eq!(parse(&v).unwrap_err().code, "vt.pack_schema");
    }

    #[test]
    fn car_area_interpolates_and_extends() {
        let en = shipped().iter().find(|p| p.id == "en81").unwrap().car_area.clone().unwrap();
        assert!((en.max_area(630.0).unwrap() - 1.66).abs() < 1e-9);
        assert!((en.max_area(1300.0).unwrap() - (2.95 + (3.10 - 2.95) * 25.0 / 75.0)).abs() < 1e-9);
        assert!((en.max_area(3000.0).unwrap() - 5.80).abs() < 1e-9);
        assert!(en.max_area(50.0).is_none());
    }

    #[test]
    fn the_stretcher_fits_a_3500_lb_car_but_not_a_2500_lb_one() {
        // 24 × 84 in, 5 in corners (IBC 3002.4).
        assert!(stretcher_fits([2.032, 1.651], 2.134, 0.610, 0.127));
        assert!(!stretcher_fits([2.032, 1.295], 2.134, 0.610, 0.127));
        // KONE's wide 2500 lb "IBC" car takes it (research/16 A6).
        assert!(stretcher_fits([2.127, 1.332], 2.134, 0.610, 0.127));
    }

    #[test]
    fn sizing_picks_the_smallest_compliant_car() {
        let us: Vec<Pack> = shipped().iter().filter(|p| ["us_ibc_ada", "us_a17", "practice_us"].contains(&p.id.as_str())).cloned().collect();
        // Two storeys: the ADA side-door car needs 1725 × 1295 mm with a 36 in door; the 2000 lb
        // car (1727 × 1295, 914 mm side door) is the first that meets it.
        let s = size(&us, &SizeParams::default()).unwrap();
        assert_eq!(s.load, 907.0);
        assert!((s.hoistway[0] - (1.727 + 0.559)).abs() < 1e-9 && (s.hoistway[1] - (1.295 + 0.457)).abs() < 1e-9);
        assert_eq!((s.pit, s.overhead), (1.524, 4.115));
        // Four storeys: the stretcher needs the 3500 lb car.
        let s = size(&us, &SizeParams { storeys: Some(4), ..SizeParams::default() }).unwrap();
        assert_eq!(s.load, 1588.0);
        assert_eq!(s.persons, 21);
        let en: Vec<Pack> = shipped().iter().filter(|p| ["en81", "en81_70", "practice_en"].contains(&p.id.as_str())).cloned().collect();
        let s = size(&en, &SizeParams { speed: Some(1.6), ..SizeParams::default() }).unwrap();
        assert_eq!((s.load, s.car), (630.0, [1.1, 1.4, 2.2]));
        assert!((s.overhead - 3.8).abs() < 1e-9 && s.pit == 1.4);
    }
}
