// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Electrical circuits and panels (MEP-ELEC-002, MEP-ELEC-003, MEP-ELEC-012, MEP-SET-002).
//!
//! - **Electrical settings** (`rxd:electrical` on the project): voltage definitions (a value and
//!   the range that accepts it) and distribution systems (phases, wye or delta, wires,
//!   line-to-line and line-to-ground voltage); the project's circuit naming scheme. Built-in
//!   ones are used until changed.
//! - A **panel** is electrical equipment with `rxd:panel`: its distribution system, its number of
//!   slots (numbered down two columns, odd on the left: a pole takes every second slot), its
//!   naming scheme (*By Project* or a scheme).
//! - A **circuit** (`IfcDistributionCircuit`, `recipe.circuit`) has a kind (power, data,
//!   telephone, security, fire alarm, nurse call, controls, communication), a voltage and poles
//!   (power), its member ports (a device's electrical connector of the kind; a connector is on one
//!   circuit at a time), and its panel and start slot. Its load is its members' apparent loads.
//!   Its name follows the panel's naming scheme: *Numerical* ("1", "13,15,17"), *Panel Name +
//!   Number* ("DB1-7") or *Prefixed* ("C7"); without a panel it has no number.
//! - Compatibility: a power device joins a circuit of its voltage and poles; a circuit fits a
//!   panel whose distribution system gives that voltage (line-to-ground for one pole, line-to-line
//!   for two or three, three only on three-phase systems) and has the free slots.
//!
//! Feeders and transformers (MEP-ELEC-006) are here too; panel schedules are
//! [`crate::mep_panel_schedule`], load classifications and demand factors [`crate::mep_loads`].
//!
//! Circuit paths and cable sizing are [`crate::mep_cables`].
//!
//! Not built yet: custom naming schemes, wires, switch systems, the Edit Circuit mode.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{self, Domain, ElectricalSystem, StoredPort};
use crate::store::Store;
use crate::txn::{Transaction, Warning};
use crate::walls::RECIPE_KEY;

pub const SETTINGS_KEY: &str = "rxd:electrical";
pub const PANEL_KEY: &str = "rxd:panel";
pub const RECIPE: &str = "recipe.circuit";
pub const CLASS: &str = "IfcDistributionCircuit";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

// ---- Settings ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoltageDefinition {
    pub id: String,
    pub name: String,
    /// Volts.
    pub value: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Single,
    Three,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Configuration {
    Wye,
    Delta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DistributionSystem {
    pub id: String,
    pub name: String,
    pub phase: Phase,
    pub configuration: Configuration,
    /// 2 to 4.
    pub wires: u8,
    /// Line to line (volts); none for a single-phase two-wire system.
    pub v_ll: Option<f64>,
    /// Line to ground (volts); none for a delta system without a neutral.
    pub v_lg: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NamingScheme {
    /// The slots: "7", "13,15,17".
    #[default]
    Numerical,
    /// "C" and the slots: "C7".
    Prefixed,
    /// The panel's name and the slots: "DB1-7".
    PanelNameNumber,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ElectricalSettings {
    pub voltages: Vec<VoltageDefinition>,
    pub distribution_systems: Vec<DistributionSystem>,
    pub naming: NamingScheme,
    /// A multi-pole circuit's name uses its first slot only.
    pub first_slot_only: bool,
    /// Load classifications (MEP-ELEC-009).
    pub load_classifications: Vec<crate::mep_loads::LoadClassification>,
    /// Demand factors (MEP-ELEC-010).
    pub demand_factors: Vec<crate::mep_loads::DemandFactor>,
    /// Conduit standards and their sizes (MEP-ELEC-017).
    pub conduit_standards: Vec<ConduitStandard>,
    /// Conductor materials, insulations, sizes, ampacities and cable types (MEP-SET-003).
    pub conductors: crate::mep_cables::Conductors,
    /// Wiring: the default cable type, voltage drop limit, correction factor, path offset.
    pub wiring: crate::mep_cables::Wiring,
}

/// A conduit standard: the sizes conduit of it comes in (MEP-ELEC-017).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConduitStandard {
    pub id: String,
    pub name: String,
    pub sizes: Vec<ConduitSize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConduitSize {
    /// A trade size such as `3/4"`, when the standard names its sizes so.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Outside diameter (metres).
    pub diameter: f64,
    /// The bend radius the standard gives (metres); otherwise the diameter × the type's
    /// multiplier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend_radius: Option<f64>,
}

impl ConduitStandard {
    /// The size of a diameter, within 0.1 mm.
    pub fn size(&self, diameter: f64) -> Option<&ConduitSize> {
        self.sizes.iter().find(|s| (s.diameter - diameter).abs() < 1e-4)
    }
}

/// The built-in conduit standards: IEC 61386 metric sizes, and EMT trade sizes (outside
/// diameters, and the bend radii of NEC Chapter 9 Table 2 for other bends).
fn default_conduit_standards() -> Vec<ConduitStandard> {
    let mm = |d: f64| d / 1000.0;
    vec![
        ConduitStandard {
            id: "iec_61386".into(),
            name: "IEC 61386 (metric)".into(),
            sizes: [16.0, 20.0, 25.0, 32.0, 40.0, 50.0, 63.0]
                .iter()
                .map(|d| ConduitSize { name: format!("{d:.0} mm"), diameter: mm(*d), bend_radius: None })
                .collect(),
        },
        ConduitStandard {
            id: "emt".into(),
            name: "EMT".into(),
            sizes: [("1/2\"", 17.9, 4.0), ("3/4\"", 23.4, 4.5), ("1\"", 29.5, 5.75), ("1-1/4\"", 38.4, 7.25), ("1-1/2\"", 44.2, 8.25), ("2\"", 55.8, 9.5)]
                .iter()
                .map(|(n, d, r)| ConduitSize { name: (*n).into(), diameter: mm(*d), bend_radius: Some(r * 0.0254) })
                .collect(),
        },
    ]
}

impl Default for ElectricalSettings {
    /// Common low-voltage systems, IEC and North American.
    fn default() -> Self {
        let v = |id: &str, value: f64| VoltageDefinition {
            id: id.into(),
            name: format!("{value:.0} V"),
            value,
            min: value * 0.9,
            max: value * 1.1,
        };
        let ds = |id: &str, name: &str, phase, configuration, wires, v_ll: Option<f64>, v_lg: Option<f64>| DistributionSystem {
            id: id.into(),
            name: name.into(),
            phase,
            configuration,
            wires,
            v_ll,
            v_lg,
        };
        Self {
            voltages: vec![v("v120", 120.0), v("v208", 208.0), v("v230", 230.0), v("v277", 277.0), v("v400", 400.0), v("v480", 480.0)],
            distribution_systems: vec![
                ds("230_400_wye", "230/400 V Wye", Phase::Three, Configuration::Wye, 4, Some(400.0), Some(230.0)),
                ds("230_single", "230 V Single Phase", Phase::Single, Configuration::Wye, 2, None, Some(230.0)),
                ds("120_208_wye", "120/208 V Wye", Phase::Three, Configuration::Wye, 4, Some(208.0), Some(120.0)),
                ds("277_480_wye", "277/480 V Wye", Phase::Three, Configuration::Wye, 4, Some(480.0), Some(277.0)),
            ],
            naming: NamingScheme::PanelNameNumber,
            first_slot_only: false,
            load_classifications: crate::mep_loads::default_classifications(),
            demand_factors: crate::mep_loads::default_demand_factors(),
            conduit_standards: default_conduit_standards(),
            conductors: crate::mep_cables::Conductors::default(),
            wiring: crate::mep_cables::Wiring::default(),
        }
    }
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub fn settings(store: &Store) -> ElectricalSettings {
    project(store)
        .and_then(|p| p.param(SETTINGS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub(crate) fn save_settings(txn: &mut Transaction<'_>, s: &ElectricalSettings) -> Result<()> {
    let pid = project(txn.store()).map(|p| p.id()).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    txn.set_param(pid, SETTINGS_KEY, serde_json::to_value(s).expect("serialises"))
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// `electrical_settings.get`.
pub(crate) fn get_settings(store: &Store, _: Empty) -> Result<ElectricalSettings> {
    Ok(settings(store))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSettingsParams {
    pub naming: Option<NamingScheme>,
    pub first_slot_only: Option<bool>,
}

/// `electrical_settings.set`: the project's naming scheme (panels *By Project* follow it).
pub(crate) fn set_settings(txn: &mut Transaction<'_>, p: SetSettingsParams) -> Result<()> {
    let mut s = settings(txn.store());
    if let Some(n) = p.naming {
        s.naming = n;
    }
    if let Some(f) = p.first_slot_only {
        s.first_slot_only = f;
    }
    save_settings(txn, &s)?;
    rename_all(txn)
}

fn slug(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DistributionParams {
    pub name: String,
    pub phase: Phase,
    pub configuration: Configuration,
    pub wires: u8,
    pub v_ll: Option<f64>,
    pub v_lg: Option<f64>,
}

/// `distribution_system.create`; returns its id.
pub(crate) fn create_distribution(txn: &mut Transaction<'_>, p: DistributionParams) -> Result<String> {
    let mut s = settings(txn.store());
    if !(2..=4).contains(&p.wires) {
        return Err(bad("distribution_system.wires", "a distribution system has 2 to 4 wires"));
    }
    if p.v_ll.is_none() && p.v_lg.is_none() {
        return Err(bad("distribution_system.voltage", "give a line-to-line or line-to-ground voltage"));
    }
    if p.phase == Phase::Three && p.v_ll.is_none() {
        return Err(bad("distribution_system.voltage", "a three-phase system needs its line-to-line voltage"));
    }
    if s.distribution_systems.iter().any(|d| d.name.eq_ignore_ascii_case(p.name.trim())) {
        return Err(bad("distribution_system.duplicate", format!("there is already a distribution system named {:?}", p.name)));
    }
    let id = slug(p.name.trim());
    s.distribution_systems.push(DistributionSystem {
        id: id.clone(),
        name: p.name.trim().into(),
        phase: p.phase,
        configuration: p.configuration,
        wires: p.wires,
        v_ll: p.v_ll,
        v_lg: p.v_lg,
    });
    save_settings(txn, &s)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VoltageParams {
    pub name: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
}

/// `voltage.create`; returns its id.
pub(crate) fn create_voltage(txn: &mut Transaction<'_>, p: VoltageParams) -> Result<String> {
    if !(p.min <= p.value && p.value <= p.max && p.min >= 0.0) {
        return Err(bad("voltage.range", "the voltage must lie between its minimum and maximum"));
    }
    let mut s = settings(txn.store());
    let id = slug(p.name.trim());
    if s.voltages.iter().any(|v| v.id == id) {
        return Err(bad("voltage.duplicate", format!("there is already a voltage named {:?}", p.name)));
    }
    s.voltages.push(VoltageDefinition { id: id.clone(), name: p.name.trim().into(), value: p.value, min: p.min, max: p.max });
    save_settings(txn, &s)?;
    Ok(id)
}

// ---- Panels ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PanelNaming {
    #[default]
    ByProject,
    Numerical,
    Prefixed,
    PanelNameNumber,
}

/// A panel's electrical data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Panel {
    /// The system it is supplied by (a transformer's primary).
    pub distribution_system: String,
    pub slots: u32,
    pub naming: PanelNaming,
    /// It has feed-through lugs: one feeder may leave from them, taking no slot.
    pub feed_through_lugs: bool,
    /// A transformer's secondary distribution system: what it feeds downstream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            distribution_system: "230_400_wye".into(),
            slots: 42,
            naming: PanelNaming::ByProject,
            feed_through_lugs: false,
            secondary: None,
        }
    }
}

/// Whether an element can be a panel: electrical equipment (a distribution board or a
/// transformer) with an electrical connector.
pub fn is_panel(store: &Store, e: &Element) -> bool {
    matches!(e.class.as_str(), "IfcElectricDistributionBoard" | "IfcTransformer")
        && mep::ports(store, e).iter().any(|p| p.connector.domain == Domain::Electrical)
}

/// The distribution system a panel gives its circuits: a transformer's secondary (none until it
/// is set), a board's own.
pub fn downstream(store: &Store, e: &Element) -> Option<DistributionSystem> {
    let pn = panel(e);
    let id = if e.class == "IfcTransformer" { pn.secondary? } else { pn.distribution_system };
    distribution(store, &id)
}

pub fn panel(e: &Element) -> Panel {
    e.param(PANEL_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// A panel's name: its element name, else "Panel" and its id.
pub fn panel_name(e: &Element) -> String {
    e.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Panel {}", e.id().0))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PanelSetParams {
    pub panel_id: ElementRef,
    /// The panel's name (shown in circuit numbers).
    pub name: Option<String>,
    pub distribution_system: Option<String>,
    pub slots: Option<u32>,
    pub naming: Option<PanelNaming>,
    pub feed_through_lugs: Option<bool>,
    /// A transformer's secondary distribution system.
    pub secondary: Option<String>,
}

/// Sets a panel's name, distribution system, slots or naming (`panel.set`). Refused when the
/// circuits on it would no longer fit.
pub(crate) fn set_panel(txn: &mut Transaction<'_>, p: PanelSetParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.panel_id).ok_or_else(|| bad("element.not_found", "no such panel"))?;
    let e = store.get(id).expect("resolved");
    if !is_panel(&store, e) {
        return Err(bad("panel.not_a_panel", format!("element {id} is not electrical equipment with an electrical connector")));
    }
    let mut pn = panel(e);
    if let Some(d) = &p.distribution_system {
        if !settings(&store).distribution_systems.iter().any(|x| &x.id == d) {
            return Err(bad("distribution_system.not_found", format!("no distribution system {d:?}")));
        }
        pn.distribution_system = d.clone();
    }
    if let Some(n) = p.slots {
        if !(1..=168).contains(&n) {
            return Err(bad("panel.slots", "a panel has 1 to 168 slots"));
        }
        pn.slots = n;
    }
    if let Some(n) = p.naming {
        pn.naming = n;
    }
    if let Some(l) = p.feed_through_lugs {
        pn.feed_through_lugs = l;
    }
    if let Some(sec) = &p.secondary {
        if e.class != "IfcTransformer" {
            return Err(bad("panel.not_a_transformer", "only a transformer has a secondary distribution system"));
        }
        if !settings(&store).distribution_systems.iter().any(|x| &x.id == sec) {
            return Err(bad("distribution_system.not_found", format!("no distribution system {sec:?}")));
        }
        pn.secondary = Some(sec.clone());
    }
    // The circuits on it must still fit.
    let guid = e.guid().to_string();
    for c in circuits(&store).iter().filter(|c| c.panel == Some(id)) {
        if let Some(start) = c.start_slot
            && slots_of(start, c.poles).last().is_some_and(|s| *s > pn.slots)
        {
            return Err(bad("panel.slots", format!("circuit {} uses slots beyond {}", c.name, pn.slots)));
        }
        let down_id = if e.class == "IfcTransformer" { pn.secondary.clone() } else { Some(pn.distribution_system.clone()) };
        if let Some(ds) = down_id.and_then(|d| distribution(&store, &d))
            && !c.feeder
            && let Err(e) = fits_system(&ds, c.voltage, c.poles, c.kind)
        {
            return Err(bad("panel.incompatible", format!("circuit {} would not fit: {}", c.name, e)));
        }
    }
    let _ = guid;
    let v = serde_json::to_value(&pn).expect("serialises");
    txn.set_param(id, PANEL_KEY, v)?;
    if let Some(n) = p.name {
        let n = n.trim().to_owned();
        if n.is_empty() {
            return Err(bad("panel.name", "a panel needs a name"));
        }
        txn.modify(id, |e| e.name = Some(n))?;
    }
    rename_all(txn)
}

fn distribution(store: &Store, id: &str) -> Option<DistributionSystem> {
    settings(store).distribution_systems.into_iter().find(|d| d.id == id)
}

/// The slots a circuit of `poles` from `start` uses: every second slot.
pub fn slots_of(start: u32, poles: u8) -> Vec<u32> {
    (0..poles.max(1) as u32).map(|k| start + 2 * k).collect()
}

// ---- Circuits ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CircuitKind {
    Power,
    Data,
    Telephone,
    Security,
    FireAlarm,
    NurseCall,
    Controls,
    Communication,
}

impl CircuitKind {
    fn accepts(self, s: ElectricalSystem) -> bool {
        match self {
            CircuitKind::Power => matches!(s, ElectricalSystem::PowerBalanced | ElectricalSystem::PowerUnbalanced),
            CircuitKind::Data => s == ElectricalSystem::Data,
            CircuitKind::Telephone => s == ElectricalSystem::Telephone,
            CircuitKind::Security => s == ElectricalSystem::Security,
            CircuitKind::FireAlarm => s == ElectricalSystem::FireAlarm,
            CircuitKind::NurseCall => s == ElectricalSystem::NurseCall,
            CircuitKind::Controls => s == ElectricalSystem::Controls,
            CircuitKind::Communication => s == ElectricalSystem::Communication,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CircuitKind::Power => "Power",
            CircuitKind::Data => "Data",
            CircuitKind::Telephone => "Telephone",
            CircuitKind::Security => "Security",
            CircuitKind::FireAlarm => "Fire Alarm",
            CircuitKind::NurseCall => "Nurse Call",
            CircuitKind::Controls => "Controls",
            CircuitKind::Communication => "Communication",
        }
    }

    /// `IfcDistributionSystemEnum` (`lighting` for a power circuit of lighting loads).
    pub fn ifc(self, lighting: bool) -> &'static str {
        match self {
            CircuitKind::Power if lighting => "LIGHTING",
            CircuitKind::Power => "ELECTRICAL",
            CircuitKind::Data => "DATA",
            CircuitKind::Telephone => "TELEPHONE",
            CircuitKind::Security => "SECURITY",
            CircuitKind::FireAlarm | CircuitKind::NurseCall => "SIGNAL",
            CircuitKind::Controls => "CONTROL",
            CircuitKind::Communication => "COMMUNICATION",
        }
    }
}

/// The stored recipe of a circuit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CircuitRecipe {
    pub kind: String,
    pub circuit_kind: CircuitKind,
    /// Power: volts and poles.
    #[serde(default)]
    pub voltage: f64,
    #[serde(default = "one")]
    pub poles: u8,
    pub members: Vec<StoredPort>,
    /// The panel (GlobalId) and the first slot it takes there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_slot: Option<u32>,
    /// A feeder: its one member is a panel's feed connector (MEP-ELEC-006).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub feeder: bool,
    /// A feeder on the parent's feed-through lugs: no slot.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lugs: bool,
    /// The load name, when edited by hand (MEP-ELEC-005); otherwise it is made from the loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_name: Option<String>,
    /// The protective device's rating (amperes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    /// Its cable type (the project's default when none) and a phase conductor size given by
    /// hand (sized when none), in `runs` parallel sets (MEP-ELEC-005).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cable_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cable_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<u32>,
    /// Its own voltage drop limit (%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_voltage_drop_percent: Option<f64>,
    /// Its path (MEP-ELEC-004).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<crate::mep_cables::PathDef>,
    /// Locked in its slots: renumbering leaves it there (MEP-ELEC-015, MEP-ELEC-021).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
}

fn one() -> u8 {
    1
}

pub fn recipe(e: &Element) -> Option<CircuitRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A circuit as it is now.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CircuitInfo {
    pub id: ElementId,
    /// Its number (empty without a panel).
    pub name: String,
    pub kind: CircuitKind,
    pub voltage: f64,
    pub poles: u8,
    pub ports: Vec<(ElementId, usize)>,
    pub elements: Vec<ElementId>,
    pub panel: Option<ElementId>,
    pub start_slot: Option<u32>,
    pub slots: Vec<u32>,
    /// The members' apparent load (VA); a feeder's is the panel it feeds' connected load.
    pub load: f64,
    pub feeder: bool,
    pub lugs: bool,
    /// The load name: edited by hand, or made from the load classification and first device
    /// (a feeder: the panel it feeds).
    pub load_name: String,
    /// Whether the load name was edited by hand.
    pub load_name_edited: bool,
    /// Amperes, when set.
    pub rating: Option<f64>,
    /// Locked in its slots.
    pub locked: bool,
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

fn info(store: &Store, e: &Element, r: &CircuitRecipe) -> CircuitInfo {
    let ports: Vec<(ElementId, usize)> = r
        .members
        .iter()
        .filter_map(|m| {
            let id = find(store, &m.element)?;
            mep::port(store, id, m.port).map(|_| (id, m.port))
        })
        .collect();
    let mut elements = Vec::new();
    let mut load = 0.0;
    for (id, k) in &ports {
        if !elements.contains(id) {
            elements.push(*id);
        }
        if let Some(p) = mep::port(store, *id, *k)
            && let Some(el) = &p.connector.electrical
        {
            load += el.load.iter().sum::<f64>();
        }
    }
    let panel = r.panel.as_deref().and_then(|g| find(store, g));
    let auto = r.load_name.as_ref().filter(|n| !n.trim().is_empty()).is_none().then(|| auto_load_name(store, r, &ports));
    CircuitInfo {
        id: e.id(),
        name: e.name.clone().unwrap_or_default(),
        kind: r.circuit_kind,
        voltage: r.voltage,
        poles: r.poles,
        ports,
        elements,
        panel,
        start_slot: if panel.is_some() { r.start_slot } else { None },
        slots: match (panel, r.start_slot) {
            (Some(_), Some(s)) if !r.lugs => slots_of(s, r.poles),
            _ => Vec::new(),
        },
        load,
        feeder: r.feeder,
        lugs: r.lugs,
        load_name: auto.unwrap_or_else(|| r.load_name.clone().unwrap_or_default()),
        load_name_edited: r.load_name.as_ref().is_some_and(|n| !n.trim().is_empty()),
        rating: r.rating,
        locked: r.locked,
    }
}

/// A load name made from the loads: the classification and the first device's name ("Lighting
/// – Downlight"), or for a feeder the panel it feeds.
fn auto_load_name(store: &Store, r: &CircuitRecipe, ports: &[(ElementId, usize)]) -> String {
    let Some(&(first, _)) = ports.first() else { return String::new() };
    let name = |id: ElementId| {
        store
            .get(id)
            .and_then(|e| e.name.clone().filter(|n| !n.is_empty()).or_else(|| crate::mesh_objects::type_of(store, e).and_then(|(t, _)| store.get(t)?.name.clone())))
            .unwrap_or_default()
    };
    if r.feeder {
        return store.get(first).map(panel_name).unwrap_or_default();
    }
    if r.circuit_kind != CircuitKind::Power {
        return format!("{} – {}", r.circuit_kind.label(), name(first));
    }
    let s = settings(store);
    let classes: Vec<String> = ports
        .iter()
        .filter_map(|(id, k)| mep::port(store, *id, *k)?.connector.electrical.map(|el| crate::mep_loads::classify(&s, &el.load_classification).name.clone()))
        .collect();
    let class = match classes.first() {
        Some(c) if classes.iter().all(|x| x == c) => c.clone(),
        Some(_) => "Mixed".into(),
        None => "Power".into(),
    };
    let n = name(first);
    if n.is_empty() { class } else { format!("{class} – {n}") }
}

/// Every circuit; a feeder's load is the connected load of the panel it feeds, through any
/// depth of sub-panels.
pub fn circuits(store: &Store) -> Vec<CircuitInfo> {
    let mut all: Vec<CircuitInfo> = store
        .iter()
        .filter(|e| e.class == CLASS)
        .filter_map(|e| Some(info(store, e, &recipe(e)?)))
        .collect();
    // Panel loads from the leaves up: a feeder's load is its child's total once that is known.
    let base: Vec<f64> = all.iter().map(|c| if c.feeder { 0.0 } else { c.load }).collect();
    let mut load = base.clone();
    for _ in 0..32 {
        let mut changed = false;
        for k in 0..all.len() {
            if !all[k].feeder {
                continue;
            }
            let Some(child) = all[k].elements.first().copied() else { continue };
            let total: f64 = (0..all.len()).filter(|j| all[*j].panel == Some(child)).map(|j| load[j]).sum();
            if (total - load[k]).abs() > 1e-9 {
                load[k] = total;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for (c, l) in all.iter_mut().zip(load) {
        c.load = l;
    }
    all
}

/// The feeder that supplies a panel, if any.
pub fn supply_of(store: &Store, panel_id: ElementId) -> Option<CircuitInfo> {
    circuits(store).into_iter().find(|c| c.feeder && c.elements.first() == Some(&panel_id))
}

/// The chain of panels from `id` up through its supplies (it first).
fn chain_up(store: &Store, id: ElementId) -> Vec<ElementId> {
    let mut out = vec![id];
    let mut at = id;
    while let Some(parent) = supply_of(store, at).and_then(|f| f.panel) {
        if out.contains(&parent) {
            break;
        }
        out.push(parent);
        at = parent;
    }
    out
}

/// The circuit each port is on.
pub fn port_circuits(store: &Store) -> BTreeMap<(ElementId, usize), ElementId> {
    let mut m = BTreeMap::new();
    for c in circuits(store) {
        for p in c.ports {
            m.insert(p, c.id);
        }
    }
    m
}

/// Why a circuit cannot go on a distribution system, if it cannot.
fn fits_system(ds: &DistributionSystem, voltage: f64, poles: u8, kind: CircuitKind) -> std::result::Result<(), String> {
    if kind != CircuitKind::Power {
        return Ok(());
    }
    if poles == 3 && ds.phase != Phase::Three {
        return Err(format!("a three-pole circuit needs a three-phase panel; {} is single-phase", ds.name));
    }
    let wanted = if poles == 1 { ds.v_lg } else { ds.v_ll.or(ds.v_lg) };
    match wanted {
        Some(v) if (v - voltage).abs() <= v * 0.1 + 1e-9 => Ok(()),
        Some(v) => Err(format!("{} gives {v:.0} V {}, not {voltage:.0} V", ds.name, if poles == 1 { "line to ground" } else { "line to line" })),
        None => Err(format!("{} has no {} voltage", ds.name, if poles == 1 { "line-to-ground" } else { "line-to-line" })),
    }
}

/// The first free start slot for `poles` on a panel, the circuit `except` aside.
fn free_slot(store: &Store, panel_id: ElementId, slots: u32, poles: u8, except: Option<ElementId>) -> Option<u32> {
    let used: Vec<u32> = circuits(store)
        .iter()
        .filter(|c| c.panel == Some(panel_id) && Some(c.id) != except)
        .flat_map(|c| c.slots.clone())
        .collect();
    (1..=slots).find(|s| slots_of(*s, poles).iter().all(|k| *k <= slots && !used.contains(k)))
}

/// A circuit's name on its panel by the panel's scheme.
pub(crate) fn circuit_name(store: &Store, panel_id: ElementId, start: u32, poles: u8) -> String {
    let s = settings(store);
    let pe = store.get(panel_id).expect("a panel");
    let scheme = match panel(pe).naming {
        PanelNaming::ByProject => s.naming,
        PanelNaming::Numerical => NamingScheme::Numerical,
        PanelNaming::Prefixed => NamingScheme::Prefixed,
        PanelNaming::PanelNameNumber => NamingScheme::PanelNameNumber,
    };
    let slots: Vec<String> = if s.first_slot_only { vec![start.to_string()] } else { slots_of(start, poles).iter().map(u32::to_string).collect() };
    let number = slots.join(",");
    match scheme {
        NamingScheme::Numerical => number,
        NamingScheme::Prefixed => format!("C{number}"),
        NamingScheme::PanelNameNumber => format!("{}-{number}", panel_name(pe)),
    }
}

/// Gives every circuit its name by its panel and slots (none without a panel).
pub(crate) fn rename_all(txn: &mut Transaction<'_>) -> Result<()> {
    let store = txn.store().clone();
    for c in circuits(&store) {
        let name = match (c.panel, c.start_slot) {
            (Some(p), _) if c.lugs => format!("{} feed-through", store.get(p).map(panel_name).unwrap_or_default()),
            (Some(p), Some(s)) => circuit_name(&store, p, s, c.poles),
            _ => String::new(),
        };
        if c.name != name {
            txn.modify(c.id, |e| e.name = Some(name))?;
        }
    }
    Ok(())
}

fn element_label(store: &Store, id: ElementId) -> String {
    let Some(e) = store.get(id) else { return id.0.to_string() };
    let name = e
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| crate::mesh_objects::type_of(store, e).and_then(|(t, _)| store.get(t)?.name.clone()))
        .unwrap_or_else(|| e.class.clone());
    format!("{name} {}", id.0)
}

/// A member port of a device for a circuit: its electrical connector of the kind (and, for power,
/// of `voltage` and `poles` when given); or why there is none.
fn member_port(store: &Store, id: ElementId, kind: CircuitKind, vp: Option<(f64, u8)>) -> std::result::Result<mep::Port, String> {
    let Some(e) = store.get(id) else { return Err("not found".into()) };
    let ports: Vec<mep::Port> = mep::ports(store, e)
        .into_iter()
        .filter(|p| p.connector.domain == Domain::Electrical && p.connector.flow_direction != mep::FlowDirection::Out)
        .collect();
    if ports.is_empty() {
        return Err("it has no electrical connector that takes a supply".into());
    }
    let of_kind: Vec<&mep::Port> = ports
        .iter()
        .filter(|p| p.connector.electrical.as_ref().is_some_and(|x| kind.accepts(x.system)))
        .collect();
    let Some(first) = of_kind.first() else {
        return Err(format!("it has no {} connector", kind.label().to_lowercase()));
    };
    let Some((v, poles)) = vp.filter(|_| kind == CircuitKind::Power) else {
        return Ok((*first).clone());
    };
    of_kind
        .iter()
        .find(|p| {
            let x = p.connector.electrical.as_ref().expect("electrical");
            (x.voltage - v).abs() <= v * 0.1 + 1e-9 && x.poles == poles
        })
        .map(|p| (*p).clone())
        .ok_or_else(|| {
            let x = first.connector.electrical.as_ref().expect("electrical");
            format!("it is {:.0} V {} pole, the circuit {v:.0} V {poles} pole", x.voltage, x.poles)
        })
}

/// A device skipped by a circuit command, and why.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Skipped {
    pub element: ElementId,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct CircuitReport {
    pub circuit: Option<ElementId>,
    pub added: Vec<ElementId>,
    pub skipped: Vec<Skipped>,
    /// Devices moved from other circuits: (device, old circuit).
    pub moved: Vec<(ElementId, ElementId)>,
}

/// Adds devices' ports to a circuit recipe; ports on other circuits are moved off them.
fn take_members(
    txn: &mut Transaction<'_>,
    circuit: Option<ElementId>,
    r: &mut CircuitRecipe,
    ids: &[ElementId],
    report: &mut CircuitReport,
) -> Result<()> {
    let store = txn.store().clone();
    let on = port_circuits(&store);
    let vp = (r.circuit_kind == CircuitKind::Power).then_some((r.voltage, r.poles));
    for id in ids {
        match member_port(&store, *id, r.circuit_kind, vp) {
            Ok(p) => {
                let sp = mep::stored(&store, p.element, p.index);
                if r.members.contains(&sp) {
                    continue;
                }
                if let Some(old) = on.get(&(p.element, p.index)).copied().filter(|o| Some(*o) != circuit) {
                    let oe = store.get(old).expect("listed");
                    let mut or = recipe(oe).expect("a circuit");
                    or.members.retain(|m| *m != sp);
                    let v = serde_json::to_value(&or).expect("serialises");
                    txn.modify(old, |e| {
                        e.params.insert(RECIPE_KEY.into(), v);
                    })?;
                    report.moved.push((*id, old));
                }
                r.members.push(sp);
                report.added.push(*id);
            }
            Err(reason) => report.skipped.push(Skipped { element: *id, reason }),
        }
    }
    Ok(())
}

fn ids(store: &Store, refs: &[ElementRef]) -> Result<Vec<ElementId>> {
    let mut out = Vec::new();
    for r in refs {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

pub(crate) fn write(txn: &mut Transaction<'_>, id: ElementId, r: &CircuitRecipe) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub kind: CircuitKind,
    pub element_ids: Vec<ElementRef>,
    /// Power: the circuit's voltage and poles; the first device's when omitted.
    pub voltage: Option<f64>,
    pub poles: Option<u8>,
}

/// Makes a circuit of devices (`circuit.create`): those of another voltage or poles, or without
/// a connector of the kind, are skipped and listed; ones on another circuit are moved, with a
/// notice.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<CircuitReport> {
    let store = txn.store().clone();
    let members = ids(&store, &p.element_ids)?;
    if members.is_empty() {
        return Err(bad("circuit.empty", "select the devices or fixtures to circuit"));
    }
    let (mut voltage, mut poles) = (p.voltage.unwrap_or(0.0), p.poles.unwrap_or(0));
    if p.kind == CircuitKind::Power && (p.voltage.is_none() || p.poles.is_none()) {
        let first = members
            .iter()
            .find_map(|id| member_port(&store, *id, p.kind, None).ok())
            .ok_or_else(|| bad("circuit.incompatible", "none of the elements has a power connector"))?;
        let x = first.connector.electrical.as_ref().expect("electrical");
        if p.voltage.is_none() {
            voltage = x.voltage;
        }
        if p.poles.is_none() {
            poles = x.poles;
        }
    }
    if p.kind == CircuitKind::Power && !(1..=3).contains(&poles) {
        return Err(bad("circuit.poles", "a power circuit has 1 to 3 poles"));
    }
    let mut r = CircuitRecipe {
        kind: RECIPE.into(),
        circuit_kind: p.kind,
        voltage,
        poles: poles.max(1),
        members: Vec::new(),
        panel: None,
        start_slot: None,
        feeder: false,
        lugs: false,
        load_name: None,
        rating: None,
        cable_type: None,
        cable_size: None,
        runs: None,
        max_voltage_drop_percent: None,
        path: None,
        locked: false,
    };
    let mut report = CircuitReport::default();
    take_members(txn, None, &mut r, &members, &mut report)?;
    if r.members.is_empty() {
        let why: Vec<String> = report.skipped.iter().map(|s| format!("{}: {}", element_label(&store, s.element), s.reason)).collect();
        return Err(bad("circuit.incompatible", format!("no device could be circuited: {}", why.join("; "))));
    }
    let lighting = r.members.iter().all(|m| {
        find(&store, &m.element)
            .and_then(|e| mep::port(&store, e, m.port))
            .and_then(|p| p.connector.electrical)
            .is_some_and(|x| x.load_classification.eq_ignore_ascii_case("lighting"))
    });
    let id = txn.create(
        NewElement::new(CLASS)
            .name("")
            .param("@PredefinedType", r.circuit_kind.ifc(lighting))
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    for s in &report.skipped {
        txn.warn(Warning {
            id: "circuit.skipped".into(),
            message: format!("{} was not circuited: {}", element_label(&store, s.element), s.reason),
            element_ids: vec![s.element],
        });
    }
    report.circuit = Some(id);
    Ok(report)
}

pub(crate) fn circuit(store: &Store, r: &ElementRef) -> Result<(ElementId, CircuitRecipe)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such circuit"))?;
    let rec = store.get(id).and_then(recipe).ok_or_else(|| bad("circuit.not_a_circuit", format!("element {id} is not a circuit")))?;
    Ok((id, rec))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MembersParams {
    pub circuit_id: ElementRef,
    pub element_ids: Vec<ElementRef>,
}

/// `circuit.add`: an incompatible device is refused with "Incompatible: <reason>"; one on
/// another circuit is moved.
pub(crate) fn add(txn: &mut Transaction<'_>, p: MembersParams) -> Result<CircuitReport> {
    let store = txn.store().clone();
    let (id, mut r) = circuit(&store, &p.circuit_id)?;
    let new = ids(&store, &p.element_ids)?;
    let mut report = CircuitReport { circuit: Some(id), ..Default::default() };
    take_members(txn, Some(id), &mut r, &new, &mut report)?;
    if let Some(s) = report.skipped.first() {
        return Err(bad("circuit.incompatible", format!("Incompatible: {} ({})", s.reason, element_label(&store, s.element))));
    }
    write(txn, id, &r)?;
    Ok(report)
}

/// `circuit.remove`: removing the last member deletes the circuit.
pub(crate) fn remove(txn: &mut Transaction<'_>, p: MembersParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = circuit(&store, &p.circuit_id)?;
    let guids: Vec<String> = ids(&store, &p.element_ids)?.iter().filter_map(|i| store.get(*i).map(|e| e.guid().to_string())).collect();
    let before = r.members.len();
    r.members.retain(|m| !guids.contains(&m.element));
    if r.members.len() == before {
        return Err(bad("circuit.not_member", "none of the elements is on the circuit"));
    }
    if r.members.is_empty() {
        return txn.delete(id);
    }
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PanelParams {
    pub circuit_id: ElementRef,
    pub panel_id: ElementRef,
    /// The first slot to take; the first free slots that fit when omitted.
    pub slot: Option<u32>,
}

/// `circuit.select_panel`: the first free slots that fit; refused for an incompatible or full
/// panel. The circuit is numbered by the panel's scheme.
pub(crate) fn select_panel(txn: &mut Transaction<'_>, p: PanelParams) -> Result<String> {
    let store = txn.store().clone();
    let (id, mut r) = circuit(&store, &p.circuit_id)?;
    let pid = store.resolve(&p.panel_id).ok_or_else(|| bad("element.not_found", "no such panel"))?;
    let pe = store.get(pid).expect("resolved");
    if !is_panel(&store, pe) {
        return Err(bad("panel.not_a_panel", format!("{} is not a panel", element_label(&store, pid))));
    }
    let pn = panel(pe);
    if r.feeder {
        return Err(bad("circuit.feeder", "a feeder's panel is set with panel.set_supply"));
    }
    let ds = downstream(&store, pe).ok_or_else(|| {
        bad("distribution_system.not_found", format!("{} has no distribution system to feed circuits from", panel_name(pe)))
    })?;
    fits_system(&ds, r.voltage, r.poles, r.circuit_kind).map_err(|e| bad("panel.incompatible", e))?;
    let start = match p.slot {
        Some(s) => {
            let used: Vec<u32> = circuits(&store).iter().filter(|c| c.panel == Some(pid) && c.id != id).flat_map(|c| c.slots.clone()).collect();
            let want = slots_of(s, r.poles);
            if s == 0 || want.iter().any(|k| *k > pn.slots) {
                return Err(bad("panel.slots", format!("slot {s} is not on {} (1 to {})", panel_name(pe), pn.slots)));
            }
            if let Some(k) = want.iter().find(|k| used.contains(k)) {
                return Err(bad("panel.slot_taken", format!("slot {k} of {} is taken", panel_name(pe))));
            }
            s
        }
        None => free_slot(&store, pid, pn.slots, r.poles, Some(id))
            .ok_or_else(|| bad("panel.full", format!("{} has no {} free slot(s) for the circuit", panel_name(pe), r.poles)))?,
    };
    r.panel = Some(pe.guid().to_string());
    r.start_slot = Some(start);
    write(txn, id, &r)?;
    rename_all(txn)?;
    Ok(txn.store().get(id).and_then(|e| e.name.clone()).unwrap_or_default())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CircuitParams {
    pub circuit_id: ElementRef,
}

/// `circuit.disconnect_panel`: frees the slots; the circuit keeps its members and loses its
/// number.
pub(crate) fn disconnect_panel(txn: &mut Transaction<'_>, p: CircuitParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = circuit(&store, &p.circuit_id)?;
    if r.panel.take().is_none() {
        return Err(bad("circuit.no_panel", "the circuit has no panel"));
    }
    r.start_slot = None;
    write(txn, id, &r)?;
    rename_all(txn)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompatibleParams {
    pub circuit_id: ElementRef,
    /// Only panels whose name contains this (case aside).
    #[serde(default)]
    pub search: Option<String>,
}

/// A panel a circuit can go on.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PanelChoice {
    pub id: ElementId,
    pub name: String,
    pub distribution_system: String,
    pub free_slots: u32,
}

/// `circuit.compatible_panels`: panels whose distribution system fits and with free slots.
pub(crate) fn compatible_panels(store: &Store, p: CompatibleParams) -> Result<Vec<PanelChoice>> {
    let (id, r) = circuit(store, &p.circuit_id)?;
    let want = p.search.map(|s| s.to_lowercase());
    let mut out = Vec::new();
    for e in store.iter().filter(|e| is_panel(store, e)) {
        let pn = panel(e);
        let Some(ds) = downstream(store, e) else { continue };
        if fits_system(&ds, r.voltage, r.poles, r.circuit_kind).is_err() {
            continue;
        }
        if free_slot(store, e.id(), pn.slots, r.poles, Some(id)).is_none() {
            continue;
        }
        let name = panel_name(e);
        if want.as_ref().is_some_and(|w| !name.to_lowercase().contains(w)) {
            continue;
        }
        let used: u32 = circuits(store).iter().filter(|c| c.panel == Some(e.id()) && c.id != id).map(|c| c.slots.len() as u32).sum();
        out.push(PanelChoice { id: e.id(), name, distribution_system: ds.name, free_slots: pn.slots.saturating_sub(used) });
    }
    Ok(out)
}

// ---- Feeders (MEP-ELEC-006) ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    #[default]
    Breaker,
    Lugs,
}

/// Why `parent` cannot feed `child`, if it cannot: same systems (a transformer's secondary
/// downstream), no loop, lugs free.
fn feeder_check(store: &Store, child: ElementId, parent: ElementId, connection: Connection, except: Option<ElementId>) -> Result<(DistributionSystem, u32)> {
    let (ce, pe) = (store.get(child).expect("found"), store.get(parent).expect("found"));
    for (e, id) in [(ce, child), (pe, parent)] {
        if !is_panel(store, e) {
            return Err(bad("panel.not_a_panel", format!("{} is not a panel or transformer", element_label(store, id))));
        }
    }
    if child == parent {
        return Err(bad("panel.loop", format!("{} cannot feed itself", panel_name(ce))));
    }
    // A loop: the child is already upstream of the parent.
    let up = chain_up(store, parent);
    if up.contains(&child) {
        let names: Vec<String> = up.iter().take_while(|p| **p != child).chain(std::iter::once(&child)).filter_map(|p| store.get(*p).map(panel_name)).collect();
        return Err(bad("panel.loop", format!("{} cannot feed {}: it is fed from it ({})", panel_name(pe), panel_name(ce), names.join(" ← "))));
    }
    let Some(down) = downstream(store, pe) else {
        return Err(bad("panel.no_secondary", "Transformer has no secondary distribution system"));
    };
    let cpn = panel(ce);
    let input = distribution(store, &cpn.distribution_system)
        .ok_or_else(|| bad("distribution_system.not_found", format!("{} has no distribution system", panel_name(ce))))?;
    let same = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(x), Some(y)) => (x - y).abs() <= x * 0.01,
        (None, None) => true,
        _ => false,
    };
    if down.phase != input.phase || down.configuration != input.configuration || !same(down.v_ll, input.v_ll) {
        return Err(bad(
            "panel.incompatible",
            format!("{} ({}) cannot be fed from {} ({}); a transformer can bridge them", panel_name(ce), input.name, panel_name(pe), down.name),
        ));
    }
    let ppn = panel(pe);
    let poles = if input.phase == Phase::Three { 3 } else { 1 };
    let start = match connection {
        Connection::Lugs => {
            if !ppn.feed_through_lugs {
                return Err(bad("panel.no_lugs", format!("{} has no feed-through lugs (Panel Settings turns them on)", panel_name(pe))));
            }
            if circuits(store).iter().any(|c| c.lugs && c.panel == Some(parent) && Some(c.id) != except) {
                return Err(bad("panel.lugs_taken", format!("{}'s feed-through lugs already feed a panel", panel_name(pe))));
            }
            0
        }
        Connection::Breaker => free_slot(store, parent, ppn.slots, poles, except)
            .ok_or_else(|| bad("panel.full", format!("{} has no {poles} free slot(s) for the feeder", panel_name(pe))))?,
    };
    Ok((input, start))
}

fn feeder_voltage(ds: &DistributionSystem) -> (f64, u8) {
    if ds.phase == Phase::Three { (ds.v_ll.unwrap_or(0.0), 3) } else { (ds.v_lg.or(ds.v_ll).unwrap_or(0.0), 1) }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeederParams {
    pub child_panel_id: ElementRef,
    pub parent_panel_id: ElementRef,
    #[serde(default)]
    pub connection: Connection,
}

/// Feeds a panel or transformer from another (`panel.create_feeder`): a feeder circuit from the
/// parent to the child's feed connector, on a breaker (taking slots) or the parent's
/// feed-through lugs (no slot). Refused across distribution systems (a transformer bridges
/// them), for a loop (named) and for a child that already has a supply.
pub(crate) fn create_feeder(txn: &mut Transaction<'_>, p: FeederParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let child = store.resolve(&p.child_panel_id).ok_or_else(|| bad("element.not_found", "no such panel"))?;
    let parent = store.resolve(&p.parent_panel_id).ok_or_else(|| bad("element.not_found", "no such panel"))?;
    if let Some(f) = supply_of(&store, child) {
        let from = f.panel.and_then(|pp| store.get(pp)).map(panel_name).unwrap_or_default();
        return Err(bad("panel.has_supply", format!("{} is already fed from {from}; panel.set_supply moves it", store.get(child).map(panel_name).unwrap_or_default())));
    }
    let (input, start) = feeder_check(&store, child, parent, p.connection, None)?;
    let port = member_port(&store, child, CircuitKind::Power, None)
        .map_err(|e| bad("panel.no_feed", format!("{} cannot be fed: {e}", element_label(&store, child))))?;
    let (voltage, poles) = feeder_voltage(&input);
    let r = CircuitRecipe {
        kind: RECIPE.into(),
        circuit_kind: CircuitKind::Power,
        voltage,
        poles,
        members: vec![mep::stored(&store, port.element, port.index)],
        panel: Some(store.get(parent).expect("found").guid().to_string()),
        start_slot: (p.connection == Connection::Breaker).then_some(start),
        feeder: true,
        lugs: p.connection == Connection::Lugs,
        load_name: None,
        rating: None,
        cable_type: None,
        cable_size: None,
        runs: None,
        max_voltage_drop_percent: None,
        path: None,
        locked: false,
    };
    let id = txn.create(
        NewElement::new(CLASS)
            .name("")
            .param("@PredefinedType", "ELECTRICAL")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    rename_all(txn)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupplyParams {
    pub panel_id: ElementRef,
    pub source_id: ElementRef,
    #[serde(default)]
    pub connection: Connection,
}

/// Chooses a panel's supply (`panel.set_supply`): its feeder moves to the source (with the
/// same checks), or one is made.
pub(crate) fn set_supply(txn: &mut Transaction<'_>, p: SupplyParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let child = store.resolve(&p.panel_id).ok_or_else(|| bad("element.not_found", "no such panel"))?;
    let parent = store.resolve(&p.source_id).ok_or_else(|| bad("element.not_found", "no such panel"))?;
    let Some(f) = supply_of(&store, child) else {
        return create_feeder(txn, FeederParams { child_panel_id: p.panel_id, parent_panel_id: p.source_id, connection: p.connection });
    };
    let (input, start) = feeder_check(&store, child, parent, p.connection, Some(f.id))?;
    let mut r = recipe(store.get(f.id).expect("listed")).expect("a circuit");
    let (voltage, poles) = feeder_voltage(&input);
    r.voltage = voltage;
    r.poles = poles;
    r.panel = Some(store.get(parent).expect("found").guid().to_string());
    r.lugs = p.connection == Connection::Lugs;
    r.start_slot = (!r.lugs).then_some(start);
    write(txn, f.id, &r)?;
    rename_all(txn)?;
    Ok(f.id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HierarchyParams {
    /// The panel to start from; every panel without a supply when omitted.
    pub root_id: Option<ElementRef>,
}

/// A panel in the distribution tree.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PanelNode {
    pub id: ElementId,
    pub name: String,
    /// The connected load of its circuits, sub-panels included (VA).
    pub load: f64,
    pub feeder: Option<ElementId>,
    pub children: Vec<PanelNode>,
}

/// `panel.hierarchy`.
pub(crate) fn hierarchy(store: &Store, p: HierarchyParams) -> Result<Vec<PanelNode>> {
    let all = circuits(store);
    fn node(store: &Store, all: &[CircuitInfo], id: ElementId, depth: usize) -> PanelNode {
        let feeders: Vec<&CircuitInfo> = all.iter().filter(|c| c.feeder && c.panel == Some(id)).collect();
        PanelNode {
            id,
            name: store.get(id).map(panel_name).unwrap_or_default(),
            load: all.iter().filter(|c| c.panel == Some(id)).map(|c| c.load).sum(),
            feeder: all.iter().find(|c| c.feeder && c.elements.first() == Some(&id)).map(|c| c.id),
            children: if depth > 32 {
                Vec::new()
            } else {
                feeders.iter().filter_map(|f| f.elements.first()).map(|c| node(store, all, *c, depth + 1)).collect()
            },
        }
    }
    let roots: Vec<ElementId> = match p.root_id {
        Some(r) => vec![store.resolve(&r).ok_or_else(|| bad("element.not_found", "no such panel"))?],
        None => store
            .iter()
            .filter(|e| is_panel(store, e))
            .map(|e| e.id())
            .filter(|id| !all.iter().any(|c| c.feeder && c.elements.first() == Some(id)))
            .collect(),
    };
    Ok(roots.into_iter().map(|r| node(store, &all, r, 0)).collect())
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// Only circuits on this panel, or with this device.
    pub element_id: Option<ElementRef>,
}

/// `circuit.list`.
pub(crate) fn list(store: &Store, p: ListParams) -> Result<Vec<CircuitInfo>> {
    let only = p.element_id.map(|r| store.resolve(&r).ok_or_else(|| bad("element.not_found", "no such element"))).transpose()?;
    Ok(circuits(store)
        .into_iter()
        .filter(|c| only.is_none_or(|e| c.panel == Some(e) || c.elements.contains(&e)))
        .collect())
}

/// A panel's summary.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PanelInfo {
    pub id: ElementId,
    pub name: String,
    #[serde(flatten)]
    pub panel: Panel,
    pub circuits: Vec<ElementId>,
    pub used_slots: u32,
    /// The circuits' load (VA).
    pub connected_load: f64,
}

/// `panel.list`.
pub(crate) fn list_panels(store: &Store, _: Empty) -> Result<Vec<PanelInfo>> {
    let all = circuits(store);
    Ok(store
        .iter()
        .filter(|e| is_panel(store, e))
        .map(|e| {
            let on: Vec<&CircuitInfo> = all.iter().filter(|c| c.panel == Some(e.id())).collect();
            PanelInfo {
                id: e.id(),
                name: panel_name(e),
                panel: panel(e),
                circuits: on.iter().map(|c| c.id).collect(),
                used_slots: on.iter().map(|c| c.slots.len() as u32).sum(),
                connected_load: on.iter().map(|c| c.load).sum(),
            }
        })
        .collect())
}

/// Circuit check issues (MEP-SYS-005 *Check Circuits*): a circuit with no panel, or on a panel
/// whose distribution system does not fit it.
pub fn check(store: &Store) -> Vec<crate::mep_systems::Issue> {
    let mut out = Vec::new();
    for c in circuits(store).into_iter().filter(|c| !c.feeder) {
        let label = if c.name.is_empty() { format!("circuit {}", c.id.0) } else { format!("circuit {}", c.name) };
        let Some(pid) = c.panel else {
            out.push(crate::mep_systems::Issue {
                kind: crate::mep_systems::IssueKind::NotWellConnected,
                element_ids: c.elements.clone(),
                connector_refs: c.ports.clone(),
                message: format!("{label} has no panel"),
            });
            continue;
        };
        let pe = store.get(pid).expect("found");
        let reason = match downstream(store, pe) {
            Some(ds) => fits_system(&ds, c.voltage, c.poles, c.kind).err(),
            None => Some(format!("{} has no distribution system to feed it", panel_name(pe))),
        };
        if let Some(r) = reason {
            out.push(crate::mep_systems::Issue {
                kind: crate::mep_systems::IssueKind::FlowMismatch,
                element_ids: vec![pid],
                connector_refs: c.ports.clone(),
                message: format!("{label}: {r}"),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_and_systems() {
        assert_eq!(slots_of(13, 3), vec![13, 15, 17]);
        let s = ElectricalSettings::default();
        let wye = &s.distribution_systems[0];
        assert!(fits_system(wye, 230.0, 1, CircuitKind::Power).is_ok());
        assert!(fits_system(wye, 400.0, 3, CircuitKind::Power).is_ok());
        assert!(fits_system(wye, 400.0, 1, CircuitKind::Power).is_err());
        let us = &s.distribution_systems[2];
        assert!(fits_system(us, 230.0, 1, CircuitKind::Power).unwrap_err().contains("120 V"));
        let single = &s.distribution_systems[1];
        assert!(fits_system(single, 400.0, 3, CircuitKind::Power).unwrap_err().contains("three-phase"));
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CircuitSetParams {
    pub circuit_id: ElementRef,
    /// The load name; empty goes back to the name made from the loads.
    pub load_name: Option<String>,
    /// The protective device's rating (amperes); 0 clears it.
    pub rating: Option<f64>,
    /// A cable type's id; empty: the project's default.
    pub cable_type: Option<String>,
    /// A conductor size's id for the phase conductors; empty: sized automatically.
    pub cable_size: Option<String>,
    /// Parallel sets (1–20).
    pub runs: Option<u32>,
}

/// `circuit.set`: a circuit's load name and rating (MEP-ELEC-005).
pub(crate) fn set_circuit(txn: &mut Transaction<'_>, p: CircuitSetParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.circuit_id).ok_or_else(|| bad("element.not_found", "no such circuit"))?;
    let mut r = store.get(id).and_then(recipe).ok_or_else(|| bad("circuit.not_a_circuit", format!("element {id} is not a circuit")))?;
    if let Some(n) = p.load_name {
        let n = n.trim().to_owned();
        if n.chars().count() > 128 {
            return Err(bad("circuit.load_name", "a load name has at most 128 characters"));
        }
        r.load_name = (!n.is_empty()).then_some(n);
    }
    if let Some(a) = p.rating {
        if !(0.0..=10_000.0).contains(&a) || !a.is_finite() {
            return Err(bad("circuit.rating", "a rating runs from 0 to 10 000 A"));
        }
        r.rating = (a > 0.0).then_some(a);
    }
    let s = settings(&store);
    if let Some(t) = p.cable_type.map(|t| t.trim().to_owned()) {
        if !t.is_empty() && !s.conductors.cable_types.iter().any(|c| c.id == t) {
            return Err(bad("cable_type.not_found", format!("no cable type {t:?}")));
        }
        r.cable_type = (!t.is_empty()).then_some(t);
    }
    if let Some(z) = p.cable_size.map(|z| z.trim().to_owned()) {
        if !z.is_empty() && !s.conductors.sizes.iter().any(|c| c.id == z) {
            return Err(bad("conductor_size.not_found", format!("no conductor size {z:?}")));
        }
        r.cable_size = (!z.is_empty()).then_some(z);
    }
    if let Some(n) = p.runs {
        if !(1..=20).contains(&n) {
            return Err(bad("circuit.runs", "a circuit has 1 to 20 parallel sets"));
        }
        r.runs = (n > 1).then_some(n);
    }
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConduitStandardParams {
    pub name: String,
    /// The sizes: outside diameter (metres), and optionally a trade-size name and the bend
    /// radius (metres).
    pub sizes: Vec<ConduitSize>,
}

/// `conduit_standard.create`; returns its id.
pub(crate) fn create_conduit_standard(txn: &mut Transaction<'_>, p: ConduitStandardParams) -> Result<String> {
    let mut s = settings(txn.store());
    let name = p.name.trim().to_owned();
    if name.is_empty() || s.conduit_standards.iter().any(|c| c.name.eq_ignore_ascii_case(&name)) {
        return Err(bad("conduit_standard.name", format!("the name {name:?} is empty or taken")));
    }
    if p.sizes.is_empty() || p.sizes.iter().any(|z| !(z.diameter > 0.0 && z.diameter < 1.0) || z.bend_radius.is_some_and(|r| r <= 0.0)) {
        return Err(bad("conduit_standard.sizes", "give the sizes: diameters from 0 to 1 m, bend radii above zero"));
    }
    let id = slug(&name);
    if s.conduit_standards.iter().any(|c| c.id == id) {
        return Err(bad("conduit_standard.name", format!("the name {name:?} is taken")));
    }
    let mut sizes = p.sizes;
    sizes.sort_by(|a, b| a.diameter.total_cmp(&b.diameter));
    s.conduit_standards.push(ConduitStandard { id: id.clone(), name, sizes });
    save_settings(txn, &s)?;
    Ok(id)
}
