// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Conductors and cables, circuit paths and cable sizing (MEP-SET-003, MEP-ELEC-004,
//! MEP-ELEC-005).
//!
//! - **Conductor data** (in the electrical settings): conductor materials with their
//!   resistivity at 20 °C and temperature coefficient (copper and aluminium by IEC 60028 and the
//!   usual aluminium values), insulations with their maximum operating temperature, conductor
//!   sizes (IEC mm² and AWG/kcmil, areas from their definitions), **cable types** (a material,
//!   an insulation, a size system or a fixed size) and **ampacities**, which Reviera does not
//!   ship: code tables may be copyrighted, so they are entered or imported by the user. Without
//!   them, current capacity is not checked and sizing says so.
//! - **Circuit paths**: *Farthest Device* (the orthogonal route, X then Y then Z, from the
//!   panel's connector to the most distant device), *All Devices* (a daisy chain by nearest
//!   neighbour from the panel) or *Custom* (the user's nodes, then the farthest device), with a
//!   path offset that lifts the horizontal runs above (or below) the higher (lower) end. A
//!   custom path is flagged when its devices have moved since it was set.
//! - **Calculation**: apparent load and power factor from the members' connectors, the design
//!   current (S / V single-phase, S / (√3 V) three-phase), the path length and the voltage drop
//!   (2 or √3 × I × L × (R cos φ + X sin φ)), R at the insulation's maximum temperature per
//!   conductor over the parallel runs.
//! - **Sizing**: the smallest conductor of the cable type's size system that keeps the voltage
//!   drop within the limit (3 % by default) and, where ampacities are given, carries the rating
//!   (or the design current) after the correction factor. A size the user gives is kept, with a
//!   warning when it fails.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mep::{self, Domain};
use crate::mep_electrical::{self as el, CircuitInfo, CircuitRecipe, ElectricalSettings};
use crate::store::Store;
use crate::txn::Transaction;

type V3 = [f64; 3];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

// ---- Conductor data ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConductorMaterial {
    pub id: String,
    pub name: String,
    /// Ω·mm²/m at 20 °C.
    pub resistivity: f64,
    /// Temperature coefficient of resistance (per °C).
    pub alpha: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Insulation {
    pub id: String,
    pub name: String,
    /// Maximum conductor operating temperature (°C): resistance is taken there.
    pub max_temperature: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SizeSystem {
    /// mm².
    #[default]
    Iec,
    /// AWG and kcmil.
    Awg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConductorSize {
    pub id: String,
    pub name: String,
    pub system: SizeSystem,
    /// Cross-section (mm²).
    pub area: f64,
}

/// What a conductor carries (amperes), as the user's table gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Ampacity {
    pub material: String,
    pub insulation: String,
    pub size: String,
    pub amperes: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CableType {
    pub id: String,
    pub name: String,
    pub material: String,
    pub insulation: String,
    /// The sizes sizing chooses from.
    #[serde(default)]
    pub system: SizeSystem,
    /// A fixed phase conductor size; sized when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    /// Phase conductors (default the circuit's poles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_count: Option<u8>,
    #[serde(default)]
    pub neutral: bool,
    #[serde(default)]
    pub ground: bool,
}

/// The conductor and cable tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Conductors {
    pub materials: Vec<ConductorMaterial>,
    pub insulations: Vec<Insulation>,
    pub sizes: Vec<ConductorSize>,
    pub ampacities: Vec<Ampacity>,
    pub cable_types: Vec<CableType>,
}

/// The AWG gauges by name and number (1/0 is 0, 4/0 is −3).
const AWG: [(&str, i32); 17] = [
    ("14 AWG", 14), ("12 AWG", 12), ("10 AWG", 10), ("8 AWG", 8), ("6 AWG", 6), ("4 AWG", 4), ("3 AWG", 3), ("2 AWG", 2), ("1 AWG", 1),
    ("1/0 AWG", 0), ("2/0 AWG", -1), ("3/0 AWG", -2), ("4/0 AWG", -3), ("18 AWG", 18), ("16 AWG", 16), ("20 AWG", 20), ("22 AWG", 22),
];

/// The area of an AWG gauge (mm²): d = 0.127 mm × 92^((36 − n) / 39).
pub fn awg_area(n: i32) -> f64 {
    let d = 0.127 * 92f64.powf((36 - n) as f64 / 39.0);
    std::f64::consts::PI / 4.0 * d * d
}

/// One kcmil in mm².
pub const KCMIL: f64 = 0.506_707_479;

impl Default for Conductors {
    fn default() -> Self {
        let iec = [1.0, 1.5, 2.5, 4.0, 6.0, 10.0, 16.0, 25.0, 35.0, 50.0, 70.0, 95.0, 120.0, 150.0, 185.0, 240.0, 300.0];
        let mut sizes: Vec<ConductorSize> = iec
            .iter()
            .map(|a| ConductorSize { id: format!("{a}mm2").replace('.', "_"), name: format!("{a} mm²"), system: SizeSystem::Iec, area: *a })
            .collect();
        let mut awg: Vec<(String, f64)> = AWG.iter().map(|(n, g)| ((*n).to_owned(), awg_area(*g))).collect();
        awg.extend([250.0, 300.0, 350.0, 400.0, 500.0].iter().map(|k| (format!("{k} kcmil"), k * KCMIL)));
        awg.sort_by(|a, b| a.1.total_cmp(&b.1));
        sizes.extend(awg.into_iter().map(|(name, area)| ConductorSize {
            id: name.to_lowercase().replace([' ', '/'], "_"),
            name,
            system: SizeSystem::Awg,
            area,
        }));
        let cable = |id: &str, name: &str, material: &str, insulation: &str, system| CableType {
            id: id.into(),
            name: name.into(),
            material: material.into(),
            insulation: insulation.into(),
            system,
            size: None,
            phase_count: None,
            neutral: true,
            ground: true,
        };
        Self {
            materials: vec![
                ConductorMaterial { id: "copper".into(), name: "Copper".into(), resistivity: 0.017_241, alpha: 0.003_93 },
                ConductorMaterial { id: "aluminium".into(), name: "Aluminium".into(), resistivity: 0.028_264, alpha: 0.004_03 },
            ],
            insulations: vec![
                Insulation { id: "pvc".into(), name: "PVC".into(), max_temperature: 70.0 },
                Insulation { id: "xlpe".into(), name: "XLPE".into(), max_temperature: 90.0 },
                Insulation { id: "thhn".into(), name: "THHN".into(), max_temperature: 90.0 },
            ],
            sizes,
            ampacities: Vec::new(),
            cable_types: vec![
                cable("cu_pvc", "Cu/PVC", "copper", "pvc", SizeSystem::Iec),
                cable("cu_xlpe", "Cu/XLPE", "copper", "xlpe", SizeSystem::Iec),
                cable("al_xlpe", "Al/XLPE", "aluminium", "xlpe", SizeSystem::Iec),
                cable("cu_thhn", "Cu THHN (AWG)", "copper", "thhn", SizeSystem::Awg),
            ],
        }
    }
}

/// Project wiring settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Wiring {
    /// The cable type circuits use unless they name one.
    pub default_cable_type: String,
    /// The voltage drop limit (%).
    pub max_voltage_drop_percent: f64,
    /// Applied to ampacities (ambient temperature and grouping together).
    pub correction_factor: f64,
    /// Default path offset (metres): horizontal runs above the higher end.
    pub path_offset: f64,
    /// Conductor reactance (Ω/m), the same for every size.
    pub reactance: f64,
}

impl Default for Wiring {
    fn default() -> Self {
        Self { default_cable_type: "cu_pvc".into(), max_voltage_drop_percent: 3.0, correction_factor: 1.0, path_offset: 0.0, reactance: 0.000_08 }
    }
}

// ---- Paths ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PathMode {
    #[default]
    FarthestDevice,
    AllDevices,
    Custom,
}

/// A circuit's path settings (on its recipe).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathDef {
    #[serde(default)]
    pub mode: PathMode,
    /// Custom: the nodes after the panel (project coordinates, metres).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<V3>,
    /// Metres; the project's when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    /// Custom: where the devices were when the nodes were set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices_at: Vec<V3>,
}

fn dist(a: V3, b: V3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The orthogonal route from `a` to `b` (X, then Y, then Z), its horizontal runs at the higher
/// end plus a positive offset, or the lower end plus a negative one.
fn route(a: V3, b: V3, offset: f64) -> Vec<V3> {
    let h = if offset >= 0.0 { a[2].max(b[2]) + offset } else { a[2].min(b[2]) + offset };
    let mut pts = vec![a, [a[0], a[1], h], [b[0], a[1], h], [b[0], b[1], h], b];
    pts.dedup_by(|p, q| dist(*p, *q) < 1e-9);
    pts
}

fn length(pts: &[V3]) -> f64 {
    pts.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// The panel's connector a circuit leaves from: its first electrical connector that is not its
/// own feed.
fn home(store: &Store, panel: ElementId) -> Option<V3> {
    let feed = el::supply_of(store, panel).and_then(|f| f.ports.first().copied());
    let e = store.get(panel)?;
    let ports: Vec<mep::Port> = mep::ports(store, e).into_iter().filter(|p| p.connector.domain == Domain::Electrical).collect();
    ports.iter().find(|p| Some((p.element, p.index)) != feed).or(ports.first()).map(|p| p.position)
}

/// A circuit's devices' connector positions.
fn devices(store: &Store, c: &CircuitInfo) -> Vec<V3> {
    c.ports.iter().filter_map(|(e, k)| mep::port(store, *e, *k).map(|p| p.position)).collect()
}

/// A circuit's path: its points, and whether a custom one may be out of date.
pub fn path(store: &Store, c: &CircuitInfo, def: &PathDef, s: &ElectricalSettings) -> (Vec<V3>, bool) {
    let Some(start) = c.panel.and_then(|p| home(store, p)) else { return (Vec::new(), false) };
    let devs = devices(store, c);
    if devs.is_empty() {
        return (vec![start], false);
    }
    let offset = def.offset.unwrap_or(s.wiring.path_offset);
    let farthest = |from: V3| devs.iter().copied().max_by(|a, b| length(&route(from, *a, offset)).total_cmp(&length(&route(from, *b, offset)))).expect("devices");
    match def.mode {
        PathMode::FarthestDevice => (route(start, farthest(start), offset), false),
        PathMode::AllDevices => {
            let mut left = devs.clone();
            let mut at = start;
            let mut pts = vec![start];
            while !left.is_empty() {
                let k = (0..left.len()).min_by(|i, j| length(&route(at, left[*i], offset)).total_cmp(&length(&route(at, left[*j], offset)))).expect("some");
                let next = left.remove(k);
                pts.extend(route(at, next, offset).into_iter().skip(1));
                at = next;
            }
            (pts, false)
        }
        PathMode::Custom => {
            let mut pts = vec![start];
            pts.extend(def.nodes.iter().copied());
            let last = *pts.last().expect("start");
            pts.push(farthest(last));
            pts.dedup_by(|p, q| dist(*p, *q) < 1e-9);
            let moved = def.devices_at.len() != devs.len() || def.devices_at.iter().zip(&devs).any(|(a, b)| dist(*a, *b) > 1e-6);
            (pts, moved)
        }
    }
}

// ---- Calculation and sizing ----

/// A circuit's calculated properties (MEP-ELEC-005).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CircuitCalc {
    pub circuit: ElementId,
    pub name: String,
    pub load_name: String,
    /// Phase labels of its slots (A, B, C).
    pub phases: String,
    pub elements: usize,
    /// VA.
    pub apparent_load: f64,
    pub power_factor: f64,
    /// The design current (A).
    pub current: f64,
    pub rating: Option<f64>,
    pub path_mode: PathMode,
    pub path_offset: f64,
    pub path: Vec<V3>,
    /// Metres.
    pub length: f64,
    pub path_out_of_date: bool,
    pub cable_type: String,
    /// The phase conductor size (its name), when one fits or was given.
    pub cable_size: Option<String>,
    /// Chosen by sizing (not given by the user).
    pub cable_size_auto: bool,
    pub runs: u32,
    /// Volts and percent of the circuit's voltage.
    pub voltage_drop: f64,
    pub voltage_drop_percent: f64,
    pub max_voltage_drop_percent: f64,
    /// The conductor's ampacity × runs × correction, when the table gives one.
    pub ampacity: Option<f64>,
    pub warnings: Vec<String>,
}

impl Default for CircuitCalc {
    fn default() -> Self {
        Self {
            circuit: ElementId(0),
            name: String::new(),
            load_name: String::new(),
            phases: String::new(),
            elements: 0,
            apparent_load: 0.0,
            power_factor: 1.0,
            current: 0.0,
            rating: None,
            path_mode: PathMode::default(),
            path_offset: 0.0,
            path: Vec::new(),
            length: 0.0,
            path_out_of_date: false,
            cable_type: String::new(),
            cable_size: None,
            cable_size_auto: false,
            runs: 1,
            voltage_drop: 0.0,
            voltage_drop_percent: 0.0,
            max_voltage_drop_percent: 0.0,
            ampacity: None,
            warnings: Vec::new(),
        }
    }
}

struct Conductor<'a> {
    material: &'a ConductorMaterial,
    insulation: &'a Insulation,
}

impl Conductor<'_> {
    /// Ω per metre of one conductor of `area` mm² at the insulation's maximum temperature.
    fn r(&self, area: f64) -> f64 {
        self.material.resistivity * (1.0 + self.material.alpha * (self.insulation.max_temperature - 20.0)) / area
    }
}

/// The voltage drop (V) of `current` over `length` on conductors of `area` in `runs`.
#[allow(clippy::too_many_arguments)]
fn drop_v(three_phase: bool, current: f64, length: f64, r: f64, x: f64, pf: f64, runs: u32) -> f64 {
    let k = if three_phase { 3f64.sqrt() } else { 2.0 };
    let sin = (1.0 - pf * pf).max(0.0).sqrt();
    k * current * length * (r * pf + x * sin) / runs.max(1) as f64
}

/// A circuit's calculated properties, sizing its cable where the user has not.
pub fn calculate(store: &Store, c: &CircuitInfo, r: &CircuitRecipe) -> CircuitCalc {
    let s = el::settings(store);
    let mut out = CircuitCalc {
        circuit: c.id,
        name: c.name.clone(),
        load_name: c.load_name.clone(),
        elements: c.elements.len(),
        apparent_load: c.load,
        rating: c.rating,
        runs: r.runs.unwrap_or(1).max(1),
        max_voltage_drop_percent: r.max_voltage_drop_percent.unwrap_or(s.wiring.max_voltage_drop_percent),
        ..CircuitCalc::default()
    };
    // Power factor: real over apparent power of the members.
    let (mut p, mut va) = (0.0, 0.0);
    for (e, k) in &c.ports {
        if let Some(x) = mep::port(store, *e, *k).and_then(|p| p.connector.electrical) {
            let v: f64 = x.load.iter().sum();
            va += v;
            p += v * x.power_factor;
        }
    }
    out.power_factor = if va > 0.0 { p / va } else { 1.0 };
    if c.feeder {
        out.power_factor = 1.0;
    }
    let three = c.poles == 3;
    out.current = if c.voltage <= 0.0 { 0.0 } else if three { c.load / (3f64.sqrt() * c.voltage) } else { c.load / c.voltage };
    // Phase labels.
    if let Some(pid) = c.panel {
        let n = s
            .distribution_systems
            .iter()
            .find(|d| store.get(pid).is_some_and(|e| el::panel(e).distribution_system == d.id))
            .map_or(1, crate::mep_panel_schedule::phases);
        out.phases = c.slots.iter().map(|sl| ["A", "B", "C"][crate::mep_panel_schedule::phase_of_slot(*sl, n)]).collect::<Vec<_>>().join(",");
    }
    let def = r.path.clone().unwrap_or_default();
    out.path_mode = def.mode;
    out.path_offset = def.offset.unwrap_or(s.wiring.path_offset);
    let (pts, stale) = path(store, c, &def, &s);
    out.length = length(&pts);
    out.path = pts;
    out.path_out_of_date = stale;
    if stale {
        out.warnings.push("The path may be out of date: its devices have moved (Reset Path)".into());
    }
    if c.panel.is_none() {
        out.warnings.push("No panel: the circuit has no path, so no length or voltage drop".into());
    }
    // The cable.
    let ct_id = r.cable_type.clone().unwrap_or_else(|| s.wiring.default_cable_type.clone());
    let Some(ct) = s.conductors.cable_types.iter().find(|t| t.id == ct_id).or(s.conductors.cable_types.first()) else {
        out.warnings.push("There are no cable types".into());
        return out;
    };
    out.cable_type = ct.name.clone();
    let (Some(material), Some(insulation)) = (
        s.conductors.materials.iter().find(|m| m.id == ct.material),
        s.conductors.insulations.iter().find(|i| i.id == ct.insulation),
    ) else {
        out.warnings.push(format!("The cable type {} names a material or insulation that is not defined", ct.name));
        return out;
    };
    let cond = Conductor { material, insulation };
    let table: Vec<&Ampacity> = s.conductors.ampacities.iter().filter(|a| a.material == ct.material && a.insulation == ct.insulation).collect();
    let need = c.rating.unwrap_or(out.current);
    let volts = c.voltage.max(1e-9);
    let check = |size: &ConductorSize| -> (f64, Option<f64>, bool) {
        let dv = drop_v(three, out.current, out.length, cond.r(size.area), s.wiring.reactance, out.power_factor, out.runs);
        let pct = 100.0 * dv / volts;
        let amp = table.iter().find(|a| a.size == size.id).map(|a| a.amperes * out.runs as f64 * s.wiring.correction_factor);
        let carries = table.is_empty() || amp.is_some_and(|a| a + 1e-9 >= need);
        (dv, amp, pct <= out.max_voltage_drop_percent + 1e-9 && carries)
    };
    let user = r.cable_size.as_ref().or(ct.size.as_ref());
    let chosen = match user {
        Some(id) => match s.conductors.sizes.iter().find(|z| &z.id == id) {
            Some(z) => {
                let (_, amp, ok) = check(z);
                if !ok {
                    let pct = 100.0 * check(z).0 / volts;
                    if pct > out.max_voltage_drop_percent + 1e-9 {
                        out.warnings.push(format!("{} gives a {pct:.2} % voltage drop, over the {} % limit", z.name, out.max_voltage_drop_percent));
                    }
                    if !table.is_empty() && amp.is_none_or(|a| a + 1e-9 < need) {
                        out.warnings.push(format!("{} does not carry {need:.1} A after correction", z.name));
                    }
                }
                Some(z)
            }
            None => {
                out.warnings.push(format!("The cable size {id:?} is not in the conductor sizes"));
                None
            }
        },
        None => {
            out.cable_size_auto = true;
            let mut sizes: Vec<&ConductorSize> = s.conductors.sizes.iter().filter(|z| z.system == ct.system).collect();
            sizes.sort_by(|a, b| a.area.total_cmp(&b.area));
            match sizes.iter().find(|z| check(z).2) {
                Some(z) => Some(*z),
                None => {
                    if let Some(z) = sizes.last() {
                        out.warnings.push(format!("No conductor size meets the {} % voltage drop limit and the current; the largest, {}, is shown", out.max_voltage_drop_percent, z.name));
                    }
                    sizes.last().copied()
                }
            }
        }
    };
    if table.is_empty() {
        out.warnings.push(format!("No ampacity table for {}: current capacity is not checked", ct.name));
    }
    if let Some(z) = chosen {
        let (dv, amp, _) = check(z);
        out.cable_size = Some(z.name.clone());
        out.voltage_drop = dv;
        out.voltage_drop_percent = 100.0 * dv / volts;
        out.ampacity = amp;
    }
    out
}

/// A circuit's calculation by id.
fn calc_of(store: &Store, r: &ElementRef) -> Result<CircuitCalc> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such circuit"))?;
    let rec = store.get(id).and_then(el::recipe).ok_or_else(|| bad("circuit.not_a_circuit", format!("element {id} is not a circuit")))?;
    let info = el::circuits(store).into_iter().find(|c| c.id == id).expect("a circuit");
    Ok(calculate(store, &info, &rec))
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CircuitRefParams {
    pub circuit_id: ElementRef,
}

/// `circuit.properties`.
pub(crate) fn properties(store: &Store, p: CircuitRefParams) -> Result<CircuitCalc> {
    calc_of(store, &p.circuit_id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SizeParams {
    pub circuit_id: ElementRef,
    /// The voltage drop limit for this circuit (%); the project's when omitted.
    pub max_vd_percent: Option<f64>,
}

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &CircuitRecipe) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(crate::walls::RECIPE_KEY.into(), v);
    })
}

fn recipe_of(store: &Store, r: &ElementRef) -> Result<(ElementId, CircuitRecipe)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such circuit"))?;
    let rec = store.get(id).and_then(el::recipe).ok_or_else(|| bad("circuit.not_a_circuit", format!("element {id} is not a circuit")))?;
    Ok((id, rec))
}

/// `circuit.size_cable`: sizes the circuit's cable again (a size given by hand is dropped) and
/// returns the result.
pub(crate) fn size_cable(txn: &mut Transaction<'_>, p: SizeParams) -> Result<CircuitCalc> {
    let store = txn.store().clone();
    let (id, mut r) = recipe_of(&store, &p.circuit_id)?;
    if let Some(m) = p.max_vd_percent {
        if !(m > 0.0 && m <= 50.0) {
            return Err(bad("circuit.max_voltage_drop", "the voltage drop limit runs above 0 to 50 %"));
        }
        r.max_voltage_drop_percent = Some(m);
    }
    r.cable_size = None;
    write(txn, id, &r)?;
    calc_of(txn.store(), &ElementRef::Id(id))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PathParams {
    pub circuit_id: ElementRef,
    pub mode: PathMode,
    /// Custom: the nodes after the panel ([x, y, z], metres); the farthest device follows.
    #[serde(default)]
    pub nodes: Vec<V3>,
    /// The path offset (metres, −10 to 10); the project's when omitted.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: Option<f64>,
}

/// `circuit.set_path`.
pub(crate) fn set_path(txn: &mut Transaction<'_>, p: PathParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = recipe_of(&store, &p.circuit_id)?;
    if r.panel.is_none() {
        return Err(bad("circuit.no_panel", "Select a panel first: a circuit without a panel has no path"));
    }
    if p.offset.is_some_and(|o| !(-10.0..=10.0).contains(&o)) {
        return Err(bad("circuit.path_offset", "the path offset runs from −10 to 10 m"));
    }
    if p.mode != PathMode::Custom && !p.nodes.is_empty() {
        return Err(bad("circuit.path_nodes", "only a custom path has nodes"));
    }
    let info = el::circuits(&store).into_iter().find(|c| c.id == id).expect("a circuit");
    let devices_at = if p.mode == PathMode::Custom { devices(&store, &info) } else { Vec::new() };
    r.path = Some(PathDef { mode: p.mode, nodes: p.nodes, offset: p.offset, devices_at });
    write(txn, id, &r)
}

/// `circuit.reset_path`: a custom path's nodes are dropped (Farthest Device again).
pub(crate) fn reset_path(txn: &mut Transaction<'_>, p: CircuitRefParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = recipe_of(&store, &p.circuit_id)?;
    let offset = r.path.as_ref().and_then(|d| d.offset);
    r.path = Some(PathDef { mode: PathMode::FarthestDevice, nodes: Vec::new(), offset, devices_at: Vec::new() });
    write(txn, id, &r)
}

/// A circuit's path as `circuit.path` gives it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PathOut {
    pub mode: PathMode,
    pub nodes: Vec<V3>,
    pub offset: f64,
    pub points: Vec<V3>,
    pub length: f64,
    pub out_of_date: bool,
}

/// `circuit.path`.
pub(crate) fn path_command(store: &Store, p: CircuitRefParams) -> Result<PathOut> {
    let (id, r) = recipe_of(store, &p.circuit_id)?;
    let info = el::circuits(store).into_iter().find(|c| c.id == id).expect("a circuit");
    let s = el::settings(store);
    let def = r.path.unwrap_or_default();
    let (pts, stale) = path(store, &info, &def, &s);
    Ok(PathOut { mode: def.mode, nodes: def.nodes.clone(), offset: def.offset.unwrap_or(s.wiring.path_offset), length: length(&pts), points: pts, out_of_date: stale })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConductorSizeParams {
    pub name: String,
    /// Cross-section (mm²); or give an AWG `gauge`.
    pub area_mm2: Option<f64>,
    /// AWG gauge: 14, 2, "1/0" … "4/0".
    pub gauge: Option<String>,
}

/// `conductor_size.create`; returns its id.
pub(crate) fn create_conductor_size(txn: &mut Transaction<'_>, p: ConductorSizeParams) -> Result<String> {
    let mut s = el::settings(txn.store());
    let name = p.name.trim().to_owned();
    if name.is_empty() || s.conductors.sizes.iter().any(|z| z.name.eq_ignore_ascii_case(&name)) {
        return Err(bad("conductor_size.name", format!("the name {name:?} is empty or taken")));
    }
    let (area, system) = match (p.area_mm2, p.gauge.as_deref()) {
        (Some(a), None) if a > 0.0 && a < 5000.0 => (a, SizeSystem::Iec),
        (None, Some(g)) => {
            let g = g.trim().trim_end_matches("AWG").trim();
            let n = match g {
                "1/0" => 0,
                "2/0" => -1,
                "3/0" => -2,
                "4/0" => -3,
                x => x.parse::<i32>().map_err(|_| bad("conductor_size.gauge", format!("{x:?} is not an AWG gauge")))?,
            };
            (awg_area(n), SizeSystem::Awg)
        }
        _ => return Err(bad("conductor_size.area", "give an area in mm² or an AWG gauge")),
    };
    let id = name.to_lowercase().replace([' ', '/', '.'], "_");
    s.conductors.sizes.push(ConductorSize { id: id.clone(), name, system, area });
    el::save_settings(txn, &s)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CableTypeParams {
    pub name: String,
    pub material_id: String,
    pub insulation_id: String,
    /// The sizes sizing chooses from (iec or awg).
    #[serde(default)]
    pub size_system: SizeSystem,
    /// A fixed phase conductor arrangement: count and size; sized when omitted.
    pub phase_conductors: Option<PhaseConductors>,
    #[serde(default)]
    pub neutral: bool,
    #[serde(default)]
    pub ground: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PhaseConductors {
    pub count: u8,
    pub size_id: String,
}

/// `cable_type.create`; returns its id.
pub(crate) fn create_cable_type(txn: &mut Transaction<'_>, p: CableTypeParams) -> Result<String> {
    let mut s = el::settings(txn.store());
    let name = p.name.trim().to_owned();
    if name.is_empty() || s.conductors.cable_types.iter().any(|t| t.name.eq_ignore_ascii_case(&name)) {
        return Err(bad("cable_type.name", format!("the name {name:?} is empty or taken")));
    }
    if !s.conductors.materials.iter().any(|m| m.id == p.material_id) {
        return Err(bad("cable_type.material", format!("no conductor material {:?}", p.material_id)));
    }
    if !s.conductors.insulations.iter().any(|i| i.id == p.insulation_id) {
        return Err(bad("cable_type.insulation", format!("no insulation {:?}", p.insulation_id)));
    }
    let (size, count, system) = match &p.phase_conductors {
        Some(pc) => {
            let z = s.conductors.sizes.iter().find(|z| z.id == pc.size_id).ok_or_else(|| bad("cable_type.size", format!("no conductor size {:?}", pc.size_id)))?;
            if !(1..=12).contains(&pc.count) {
                return Err(bad("cable_type.count", "a cable has 1 to 12 phase conductors"));
            }
            (Some(z.id.clone()), Some(pc.count), z.system)
        }
        None => (None, None, p.size_system),
    };
    let id = name.to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    s.conductors.cable_types.push(CableType {
        id: id.clone(),
        name,
        material: p.material_id,
        insulation: p.insulation_id,
        system,
        size,
        phase_count: count,
        neutral: p.neutral,
        ground: p.ground,
    });
    el::save_settings(txn, &s)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AmpacityParams {
    /// Rows of the user's ampacity table; each replaces the row of the same material,
    /// insulation and size. Amperes 0 removes it.
    pub rows: Vec<Ampacity>,
}

/// `conductor_ampacity.set`: rows of the ampacity table; returns how many rows it holds.
pub(crate) fn set_ampacities(txn: &mut Transaction<'_>, p: AmpacityParams) -> Result<usize> {
    let mut s = el::settings(txn.store());
    for row in p.rows {
        if !s.conductors.sizes.iter().any(|z| z.id == row.size) {
            return Err(bad("conductor_ampacity.size", format!("no conductor size {:?}", row.size)));
        }
        if !s.conductors.materials.iter().any(|m| m.id == row.material) || !s.conductors.insulations.iter().any(|i| i.id == row.insulation) {
            return Err(bad("conductor_ampacity.material", format!("no material {:?} or insulation {:?}", row.material, row.insulation)));
        }
        if !(row.amperes >= 0.0 && row.amperes < 100_000.0) {
            return Err(bad("conductor_ampacity.amperes", "amperes run from 0"));
        }
        s.conductors.ampacities.retain(|a| !(a.material == row.material && a.insulation == row.insulation && a.size == row.size));
        if row.amperes > 0.0 {
            s.conductors.ampacities.push(row);
        }
    }
    let n = s.conductors.ampacities.len();
    el::save_settings(txn, &s)?;
    Ok(n)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WiringParams {
    pub default_cable_type: Option<String>,
    pub max_voltage_drop_percent: Option<f64>,
    pub correction_factor: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub path_offset: Option<f64>,
    /// Ω/m.
    pub reactance: Option<f64>,
}

/// `wiring.set`: the project's wiring settings.
pub(crate) fn set_wiring(txn: &mut Transaction<'_>, p: WiringParams) -> Result<()> {
    let mut s = el::settings(txn.store());
    if let Some(t) = p.default_cable_type {
        if !s.conductors.cable_types.iter().any(|c| c.id == t) {
            return Err(bad("cable_type.not_found", format!("no cable type {t:?}")));
        }
        s.wiring.default_cable_type = t;
    }
    if let Some(m) = p.max_voltage_drop_percent {
        if !(m > 0.0 && m <= 50.0) {
            return Err(bad("wiring.max_voltage_drop", "the voltage drop limit runs above 0 to 50 %"));
        }
        s.wiring.max_voltage_drop_percent = m;
    }
    if let Some(c) = p.correction_factor {
        if !(c > 0.0 && c <= 2.0) {
            return Err(bad("wiring.correction_factor", "the correction factor runs above 0 to 2"));
        }
        s.wiring.correction_factor = c;
    }
    if let Some(o) = p.path_offset {
        if !(-10.0..=10.0).contains(&o) {
            return Err(bad("circuit.path_offset", "the path offset runs from −10 to 10 m"));
        }
        s.wiring.path_offset = o;
    }
    if let Some(x) = p.reactance {
        if !(0.0..0.01).contains(&x) {
            return Err(bad("wiring.reactance", "a reactance runs from 0 to 0.01 Ω/m"));
        }
        s.wiring.reactance = x;
    }
    el::save_settings(txn, &s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awg_areas() {
        // 10 AWG is 5.26 mm², 4/0 is 107.2 mm².
        assert!((awg_area(10) - 5.261).abs() < 0.01);
        assert!((awg_area(-3) - 107.2).abs() < 0.1);
    }

    #[test]
    fn routes_are_orthogonal_with_the_offset_above() {
        let r = route([0.0, 0.0, 1.0], [3.0, 4.0, 2.5], 0.0);
        assert!((length(&r) - (3.0 + 4.0 + 1.5)).abs() < 1e-9);
        // An offset of 1 m adds it twice.
        assert!((length(&route([0.0, 0.0, 1.0], [3.0, 4.0, 2.5], 1.0)) - (3.0 + 4.0 + 1.5 + 2.0)).abs() < 1e-9);
    }

    #[test]
    fn a_hand_calculated_drop() {
        // 2 kW at 230 V, power factor 1, 40 m of 2.5 mm² copper at 70 °C: 2 × 8.696 × 40 × R.
        let (cu, pvc) = (Conductors::default().materials[0].clone(), Conductors::default().insulations[0].clone());
        let c = Conductor { material: &cu, insulation: &pvc };
        let r = c.r(2.5);
        assert!((r - 0.017_241 * (1.0 + 0.003_93 * 50.0) / 2.5).abs() < 1e-12);
        let dv = drop_v(false, 2000.0 / 230.0, 40.0, r, 0.000_08, 1.0, 1);
        assert!((dv - 2.0 * (2000.0 / 230.0) * 40.0 * r).abs() < 1e-9);
    }
}
