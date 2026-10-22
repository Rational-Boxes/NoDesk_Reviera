// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Mechanical Settings (MEP-SET-001): Manage › MEP Settings › Mechanical Settings.
//!
//! - The settings live in the RXD document `rxd:mep_settings` (`MepSettings::mechanical`, with
//!   the friction equation, slopes and hydronic switch kept where earlier features put them).
//! - `mechanical_settings.get(path)` and `.set(path, value)` work on one tree: `hidden_line`,
//!   `duct` (annotation, air properties, separators and suffixes, `angles`, `conversion`,
//!   `sizes.{rectangular,oval,round}`, `calculation.friction_equation`) and `pipe` (annotation,
//!   `angles`, `conversion`, `segments`, `fluids`, `slopes`, `calculation`). A path is dotted
//!   (`duct.air_density`); the empty path is the whole tree. Values are checked before anything
//!   is written.
//! - **Size tables**: each duct shape's sizes, each flagged *Used in Size Lists* (the size
//!   drop-downs) and *Used in Sizing* (what sizing chooses from when a type has no table of its
//!   own). Taking a size off a list leaves ducts of that size as they are.
//! - **Pipe segments**: a material with a schedule or class, its roughness and a catalogue of
//!   nominal, inner and outer diameters, each flagged like duct sizes; exported as one
//!   `IfcPipeSegmentType` per size with `Pset_PipeSegmentTypeCommon`.
//! - **Fluids**: water is built in; others are a table of temperature, density and dynamic
//!   viscosity, interpolated by systems whose type names them.
//! - **Slopes**: the allowed pipe slopes always include zero, which cannot be removed; a slope
//!   steeper than 45° (rise over run above 1) is kept with a warning.
//! - **Angles**: fittings at any angle, at an increment, or at listed angles; runs and
//!   placeholder conversion keep to them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::mep::Shape;
use crate::mep_sizing::Equation;
use crate::store::Store;
use crate::txn::{Transaction, Warning};

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The angles fittings may turn.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum Angles {
    #[default]
    Any,
    /// Multiples of `step` degrees.
    Increment { step: f64 },
    /// These angles (degrees).
    List { angles: Vec<f64> },
}

impl Angles {
    /// Whether a fitting may turn `deg` degrees.
    pub fn allows(&self, deg: f64) -> bool {
        const TOL: f64 = 0.05;
        match self {
            Angles::Any => true,
            Angles::Increment { step } => {
                let k = (deg / step).round();
                k >= 1.0 && (deg - k * step).abs() <= TOL
            }
            Angles::List { angles } => angles.iter().any(|a| (a - deg).abs() <= TOL),
        }
    }

    fn check(&self) -> Result<()> {
        match self {
            Angles::Any => Ok(()),
            Angles::Increment { step } if *step > 0.0 && *step <= 90.0 => Ok(()),
            Angles::Increment { .. } => Err(bad("mechanical_settings.angles", "the increment is above 0° and at most 90°")),
            Angles::List { angles } if !angles.is_empty() && angles.iter().all(|a| *a > 0.0 && *a < 180.0) => Ok(()),
            Angles::List { .. } => Err(bad("mechanical_settings.angles", "list one or more angles, each above 0° and below 180°")),
        }
    }

    pub fn text(&self) -> String {
        match self {
            Angles::Any => "any angle".into(),
            Angles::Increment { step } => format!("multiples of {step}°"),
            Angles::List { angles } => angles.iter().map(|a| format!("{a}°")).collect::<Vec<_>>().join(", "),
        }
    }
}

/// What placeholder conversion and layout generation use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Conversion {
    /// The main's segment type (GlobalId); the placeholder's own when empty.
    pub main_type: Option<String>,
    /// Branches' segment type (GlobalId); the placeholder's own when empty.
    pub branch_type: Option<String>,
    /// Middle elevation of mains and branches above the level (metres).
    pub main_offset: f64,
    pub branch_offset: f64,
    /// Ducts: branches end in flex duct up to `flex_max_length`.
    pub flex: bool,
    pub flex_max_length: f64,
}

impl Default for Conversion {
    fn default() -> Self {
        Conversion { main_type: None, branch_type: None, main_offset: 2.7, branch_offset: 2.7, flex: false, flex_max_length: 1.5 }
    }
}

/// One size of a table (metres), and where it is offered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SizeRow {
    pub size: f64,
    #[serde(default = "yes")]
    pub in_lists: bool,
    #[serde(default = "yes")]
    pub in_sizing: bool,
}

fn yes() -> bool {
    true
}

fn rows(mm: &[f64]) -> Vec<SizeRow> {
    mm.iter().map(|v| SizeRow { size: v / 1000.0, in_lists: true, in_sizing: true }).collect()
}

const ROUND_MM: [f64; 19] = [100.0, 125.0, 150.0, 160.0, 200.0, 250.0, 315.0, 355.0, 400.0, 450.0, 500.0, 560.0, 630.0, 710.0, 800.0, 900.0, 1000.0, 1120.0, 1250.0];
const SIDE_MM: [f64; 19] = [100.0, 150.0, 200.0, 250.0, 300.0, 350.0, 400.0, 450.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0, 1200.0, 1400.0, 1600.0, 1800.0, 2000.0];
const DN_MM: [f64; 15] = [10.0, 15.0, 20.0, 25.0, 32.0, 40.0, 50.0, 65.0, 80.0, 100.0, 125.0, 150.0, 200.0, 250.0, 300.0];

/// Duct size tables by shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DuctSizes {
    /// Rectangle sides.
    pub rectangular: Vec<SizeRow>,
    /// Oval sides.
    pub oval: Vec<SizeRow>,
    /// Diameters.
    pub round: Vec<SizeRow>,
}

impl Default for DuctSizes {
    fn default() -> Self {
        DuctSizes { rectangular: rows(&SIDE_MM), oval: rows(&SIDE_MM), round: rows(&ROUND_MM) }
    }
}

impl DuctSizes {
    pub fn of(&self, shape: Shape) -> &Vec<SizeRow> {
        match shape {
            Shape::Round => &self.round,
            Shape::Oval => &self.oval,
            _ => &self.rectangular,
        }
    }
    fn of_mut(&mut self, shape: Shape) -> &mut Vec<SizeRow> {
        match shape {
            Shape::Round => &mut self.round,
            Shape::Oval => &mut self.oval,
            _ => &mut self.rectangular,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct HiddenLine {
    /// Whether MEP elements behind others draw hidden lines.
    pub draw: bool,
    pub line_style: String,
    /// Gaps on paper (mm): inside a crossing, outside it, and for single lines.
    pub inside_gap: f64,
    pub outside_gap: f64,
    pub single_line_gap: f64,
}

impl Default for HiddenLine {
    fn default() -> Self {
        HiddenLine { draw: true, line_style: "Hidden".into(), inside_gap: 1.5, outside_gap: 1.5, single_line_gap: 1.5 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DuctSettings {
    /// Single-line fittings drawn at the annotation size (else at their real size).
    pub fitting_annotation_scale: bool,
    /// Paper mm.
    pub fitting_annotation_size: f64,
    /// kg/m³ and Pa·s.
    pub air_density: f64,
    pub air_viscosity: f64,
    pub size_separator: String,
    pub size_suffix: String,
    pub connector_separator: String,
    pub flat_on_top: String,
    pub flat_on_bottom: String,
    /// Paper mm.
    pub rise_drop_size: f64,
    pub angles: Angles,
    pub conversion: Conversion,
    pub sizes: DuctSizes,
}

impl Default for DuctSettings {
    fn default() -> Self {
        DuctSettings {
            fitting_annotation_scale: true,
            fitting_annotation_size: 3.0,
            air_density: crate::mep_sizing::AIR.0,
            air_viscosity: crate::mep_sizing::AIR.1,
            size_separator: "×".into(),
            size_suffix: String::new(),
            connector_separator: "-".into(),
            flat_on_top: "FOT".into(),
            flat_on_bottom: "FOB".into(),
            rise_drop_size: 3.0,
            angles: Angles::Any,
            conversion: Conversion::default(),
            sizes: DuctSizes::default(),
        }
    }
}

/// A pipe catalogue row (metres).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PipeSize {
    pub nominal: f64,
    pub inner: f64,
    pub outer: f64,
    #[serde(default = "yes")]
    pub in_lists: bool,
    #[serde(default = "yes")]
    pub in_sizing: bool,
}

/// A pipe segment: a material and schedule or class with its roughness and sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PipeSegment {
    pub id: String,
    pub material: String,
    pub schedule: String,
    /// Metres.
    pub roughness: f64,
    pub sizes: Vec<PipeSize>,
}

impl PipeSegment {
    pub fn name(&self) -> String {
        if self.schedule.is_empty() { self.material.clone() } else { format!("{} - {}", self.material, self.schedule) }
    }
}

/// A fluid's property row: °C, kg/m³, Pa·s.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FluidRow {
    pub temperature: f64,
    pub density: f64,
    pub viscosity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Fluid {
    pub id: String,
    pub name: String,
    pub table: Vec<FluidRow>,
}

impl Fluid {
    /// Density and viscosity at `t`, between the table's rows.
    pub fn at(&self, t: f64) -> Result<(f64, f64)> {
        let (lo, hi) = (self.table[0].temperature, self.table[self.table.len() - 1].temperature);
        if !(lo..=hi).contains(&t) {
            return Err(bad("mep_size.fluid_temperature", "Temperature outside fluid table range"));
        }
        if self.table.len() == 1 {
            return Ok((self.table[0].density, self.table[0].viscosity));
        }
        let k = self.table.windows(2).position(|w| t <= w[1].temperature).unwrap_or(self.table.len() - 2);
        let (a, b) = (self.table[k], self.table[k + 1]);
        let u = (t - a.temperature) / (b.temperature - a.temperature);
        Ok((a.density + (b.density - a.density) * u, a.viscosity + (b.viscosity - a.viscosity) * u))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PipeSettings {
    /// Paper mm.
    pub annotation_size: f64,
    pub size_suffix: String,
    pub size_prefix: String,
    pub connector_separator: String,
    pub angles: Angles,
    pub conversion: Conversion,
    /// Made or changed segments; the built-in Generic segment otherwise.
    pub segments: Vec<PipeSegment>,
    /// Made fluids (water is built in).
    pub fluids: Vec<Fluid>,
    /// The pressure-drop equation of pipes; the duct one when not set.
    pub pressure_drop: Option<Equation>,
}

impl Default for PipeSettings {
    fn default() -> Self {
        PipeSettings {
            annotation_size: 3.0,
            size_suffix: String::new(),
            size_prefix: "Ø".into(),
            connector_separator: "-".into(),
            angles: Angles::Any,
            conversion: Conversion::default(),
            segments: Vec::new(),
            fluids: Vec::new(),
            pressure_drop: None,
        }
    }
}

/// The settings stored in `MepSettings::mechanical`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MechanicalSettings {
    pub hidden_line: HiddenLine,
    pub duct: DuctSettings,
    pub pipe: PipeSettings,
}

/// The built-in pipe segment: the DN series with no wall (inner and outer as nominal),
/// commercial steel's roughness.
fn generic_segment() -> PipeSegment {
    PipeSegment {
        id: "generic".into(),
        material: "Generic".into(),
        schedule: String::new(),
        roughness: 4.5e-5,
        sizes: DN_MM.iter().map(|d| PipeSize { nominal: d / 1000.0, inner: d / 1000.0, outer: d / 1000.0, in_lists: true, in_sizing: true }).collect(),
    }
}

pub fn get_all(store: &Store) -> MechanicalSettings {
    crate::mep_systems::settings(store).mechanical
}

/// The pipe segments: those made, or the built-in Generic one.
pub fn segments(store: &Store) -> Vec<PipeSegment> {
    let s = get_all(store).pipe.segments;
    if s.is_empty() { vec![generic_segment()] } else { s }
}

/// The fluids: water (built in), then those made.
pub fn fluid_names(store: &Store) -> Vec<String> {
    std::iter::once("Water".to_owned()).chain(get_all(store).pipe.fluids.into_iter().map(|f| f.name)).collect()
}

/// Air's density and viscosity.
pub fn air(store: &Store) -> (f64, f64) {
    let d = get_all(store).duct;
    (d.air_density, d.air_viscosity)
}

/// A fluid's density and viscosity at `t` °C: water, or a made fluid by name.
pub fn fluid(store: &Store, name: &str, t: f64) -> Result<(f64, f64)> {
    if name.eq_ignore_ascii_case("water") {
        return crate::mep_sizing::water(t);
    }
    get_all(store)
        .pipe
        .fluids
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case(name) || f.id == name)
        .ok_or_else(|| bad("mep_size.fluid", format!("the fluid {name:?} is not in Mechanical Settings › Pipe › Fluids")))?
        .at(t)
}

/// The friction equation for air (ducts) or water (pipes).
pub fn equation(store: &Store, air: bool) -> Equation {
    let s = crate::mep_systems::settings(store);
    if air { s.friction_equation } else { s.mechanical.pipe.pressure_drop.unwrap_or(s.friction_equation) }
}

/// The duct sizes offered in drop-downs (metres).
pub fn list_sizes(store: &Store, shape: Shape) -> Vec<f64> {
    get_all(store).duct.sizes.of(shape).iter().filter(|r| r.in_lists).map(|r| r.size).collect()
}

/// The duct sizes sizing chooses from (metres).
pub fn sizing_sizes(store: &Store, shape: Shape) -> Vec<f64> {
    get_all(store).duct.sizes.of(shape).iter().filter(|r| r.in_sizing).map(|r| r.size).collect()
}

/// The pipe diameters offered in drop-downs (nominal, metres), across segments.
pub fn pipe_list_sizes(store: &Store) -> Vec<f64> {
    let mut v: Vec<f64> = segments(store).iter().flat_map(|s| s.sizes.iter().filter(|z| z.in_lists).map(|z| z.nominal)).collect();
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    v
}

/// The angles fittings of a domain may turn.
pub fn angles(store: &Store, domain: crate::mep::Domain) -> Angles {
    let s = get_all(store);
    match domain {
        crate::mep::Domain::Duct => s.duct.angles,
        crate::mep::Domain::Pipe => s.pipe.angles,
        _ => Angles::Any,
    }
}

// ---- The tree ----

/// The whole tree as `get` shows it.
fn tree(store: &Store) -> Value {
    let s = crate::mep_systems::settings(store);
    let mut v = serde_json::to_value(&s.mechanical).expect("serialises");
    v["duct"]["calculation"] = serde_json::json!({ "friction_equation": s.friction_equation });
    let mut slopes = vec![0.0];
    slopes.extend(crate::mep_slopes::allowed(store));
    v["pipe"]["slopes"] = serde_json::json!(slopes);
    v["pipe"]["segments"] = serde_json::to_value(segments(store)).expect("serialises");
    v["pipe"]["calculation"] = serde_json::json!({ "hydronic_analysis": s.hydronic_analysis, "pressure_drop": equation(store, false) });
    if let Some(o) = v["pipe"].as_object_mut() {
        o.remove("pressure_drop");
    }
    v
}

fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(v);
    }
    path.split('.').try_fold(v, |v, k| v.get(k))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetParams {
    /// A dotted path (`duct.air_density`, `pipe.slopes`); the whole tree when empty.
    #[serde(default)]
    pub path: String,
}

/// `mechanical_settings.get`.
pub(crate) fn get(store: &Store, p: GetParams) -> Result<Value> {
    let t = tree(store);
    at(&t, &p.path).cloned().ok_or_else(|| bad("mechanical_settings.path", format!("no setting {:?}", p.path)))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    /// A dotted path, as `mechanical_settings.get` shows the tree.
    pub path: String,
    pub value: Value,
}

/// `mechanical_settings.set`: one setting (or a branch of the tree), checked before it is kept.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let mut t = tree(&store);
    {
        let parts: Vec<&str> = p.path.split('.').filter(|k| !k.is_empty()).collect();
        let Some((last, head)) = parts.split_last() else {
            return Err(bad("mechanical_settings.path", "name the setting to change"));
        };
        let mut v = &mut t;
        for k in head {
            v = v.get_mut(*k).ok_or_else(|| bad("mechanical_settings.path", format!("no setting {:?}", p.path)))?;
        }
        let slot = v.get_mut(*last).ok_or_else(|| bad("mechanical_settings.path", format!("no setting {:?}", p.path)))?;
        *slot = p.value;
    }
    // Split the tree back into the stored settings.
    let mut s = crate::mep_systems::settings(&store);
    let eq: Equation = serde_json::from_value(t["duct"]["calculation"]["friction_equation"].clone()).map_err(|e| bad("mechanical_settings.value", format!("duct.calculation.friction_equation: {e}")))?;
    let pressure_drop: Equation = serde_json::from_value(t["pipe"]["calculation"]["pressure_drop"].clone()).map_err(|e| bad("mechanical_settings.value", format!("pipe.calculation.pressure_drop: {e}")))?;
    let hydronic = t["pipe"]["calculation"]["hydronic_analysis"].as_bool().ok_or_else(|| bad("mechanical_settings.value", "pipe.calculation.hydronic_analysis is true or false"))?;
    let slopes: Vec<f64> = serde_json::from_value(t["pipe"]["slopes"].clone()).map_err(|e| bad("mechanical_settings.value", format!("pipe.slopes: {e}")))?;
    let segs: Vec<PipeSegment> = serde_json::from_value(t["pipe"]["segments"].clone()).map_err(|e| bad("mechanical_settings.value", format!("pipe.segments: {e}")))?;
    for k in ["calculation", "slopes", "segments"] {
        if let Some(o) = t["pipe"].as_object_mut() {
            o.remove(k);
        }
    }
    if let Some(o) = t["duct"].as_object_mut() {
        o.remove("calculation");
    }
    let mut m: MechanicalSettings = serde_json::from_value(t).map_err(|e| bad("mechanical_settings.value", format!("{}: {e}", p.path)))?;
    m.pipe.pressure_drop = (pressure_drop != eq).then_some(pressure_drop);
    m.pipe.segments = if segs == vec![generic_segment()] { Vec::new() } else { segs };
    let warnings = check(&m)?;
    s.slopes = keep_slopes(&slopes)?;
    s.friction_equation = eq;
    s.hydronic_analysis = hydronic;
    s.mechanical = m;
    for w in warnings.into_iter().chain(steep(&slopes)) {
        txn.warn(Warning { id: "mechanical_settings.warning".into(), message: w, element_ids: Vec::new() });
    }
    crate::mep_systems::save_settings(txn, &s)
}

/// The slopes kept (zero is implied); refused without zero.
fn keep_slopes(slopes: &[f64]) -> Result<Vec<f64>> {
    if !slopes.iter().any(|s| s.abs() < 1e-12) {
        return Err(bad("mechanical_settings.zero_slope", "The zero slope cannot be removed"));
    }
    if slopes.iter().any(|s| !(s.is_finite() && *s >= 0.0)) {
        return Err(bad("slope.value", "each slope is rise over run, zero or more"));
    }
    let mut v: Vec<f64> = slopes.iter().copied().filter(|s| *s > 1e-12).collect();
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    Ok(v)
}

fn steep(slopes: &[f64]) -> Vec<String> {
    slopes.iter().filter(|s| **s > 1.0).map(|s| format!("A slope of {:.1}° is steeper than 45°", s.atan().to_degrees())).collect()
}

/// Checks the settings; returns warnings.
fn check(m: &MechanicalSettings) -> Result<Vec<String>> {
    let d = &m.duct;
    if !(d.air_density > 0.0 && d.air_density.is_finite() && d.air_viscosity > 0.0 && d.air_viscosity.is_finite()) {
        return Err(bad("mechanical_settings.air", "air density and viscosity are above zero"));
    }
    for g in [m.hidden_line.inside_gap, m.hidden_line.outside_gap, m.hidden_line.single_line_gap, d.fitting_annotation_size, d.rise_drop_size, m.pipe.annotation_size] {
        if !(g.is_finite() && g >= 0.0) {
            return Err(bad("mechanical_settings.paper", "paper sizes and gaps are zero or more (mm)"));
        }
    }
    d.angles.check()?;
    m.pipe.angles.check()?;
    for (name, t) in [("rectangular", &d.sizes.rectangular), ("oval", &d.sizes.oval), ("round", &d.sizes.round)] {
        check_rows(name, &t.iter().map(|r| r.size).collect::<Vec<_>>())?;
    }
    for c in [&d.conversion, &m.pipe.conversion] {
        if !(c.flex_max_length > 0.0 && c.main_offset.is_finite() && c.branch_offset.is_finite()) {
            return Err(bad("mechanical_settings.conversion", "the conversion offsets are numbers and the flex length is above zero"));
        }
    }
    for s in &m.pipe.segments {
        check_segment(s)?;
    }
    for f in &m.pipe.fluids {
        check_fluid(f)?;
    }
    let mut ids: Vec<&str> = m.pipe.segments.iter().map(|s| s.id.as_str()).chain(m.pipe.fluids.iter().map(|f| f.id.as_str())).collect();
    ids.sort_unstable();
    if ids.windows(2).any(|w| w[0] == w[1]) {
        return Err(bad("mechanical_settings.duplicate", "two segments or fluids share an id"));
    }
    Ok(Vec::new())
}

fn check_rows(name: &str, sizes: &[f64]) -> Result<()> {
    if sizes.iter().any(|s| !(s.is_finite() && *s > 0.0)) {
        return Err(bad("duct_size_table.size", format!("{name} sizes are above zero")));
    }
    let mut v = sizes.to_vec();
    v.sort_by(f64::total_cmp);
    if v.windows(2).any(|w| (w[1] - w[0]).abs() < 1e-9) {
        return Err(bad("duct_size_table.duplicate", format!("a {name} size is listed twice")));
    }
    Ok(())
}

fn check_segment(s: &PipeSegment) -> Result<()> {
    if s.material.trim().is_empty() {
        return Err(bad("pipe_segment.material", "name the segment's material"));
    }
    if !(s.roughness.is_finite() && s.roughness >= 0.0) {
        return Err(bad("pipe_segment.roughness", "the roughness is zero or more"));
    }
    if s.sizes.is_empty() {
        return Err(bad("pipe_segment.sizes", "a segment has one size or more"));
    }
    for z in &s.sizes {
        if !(z.nominal > 0.0 && z.inner > 0.0 && z.outer >= z.inner) {
            return Err(bad("pipe_segment.size", format!("DN {:.0}: diameters are above zero and the outer is at least the inner", z.nominal * 1000.0)));
        }
    }
    check_rows("pipe", &s.sizes.iter().map(|z| z.nominal).collect::<Vec<_>>())
}

fn check_fluid(f: &Fluid) -> Result<()> {
    if f.name.trim().is_empty() || f.name.eq_ignore_ascii_case("water") {
        return Err(bad("fluid.name", "name the fluid (water is built in)"));
    }
    if f.table.is_empty() {
        return Err(bad("fluid.table", "a fluid has one row or more"));
    }
    if f.table.iter().any(|r| !(r.temperature.is_finite() && r.density > 0.0 && r.viscosity > 0.0)) {
        return Err(bad("fluid.table", "densities and viscosities are above zero"));
    }
    if f.table.windows(2).any(|w| w[1].temperature <= w[0].temperature) {
        return Err(bad("fluid.table", "temperatures rise row by row"));
    }
    Ok(())
}

fn slug(name: &str, taken: &[String]) -> String {
    let base: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let base = base.trim_matches('_').to_owned();
    let mut id = base.clone();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{base}_{n}");
        n += 1;
    }
    id
}

fn save(txn: &mut Transaction<'_>, m: MechanicalSettings) -> Result<()> {
    check(&m)?;
    let mut s = crate::mep_systems::settings(txn.store());
    s.mechanical = m;
    crate::mep_systems::save_settings(txn, &s)
}

// ---- Commands for tables ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DuctSizeTableParams {
    pub shape: Shape,
    /// The table's rows (metres), replacing it.
    pub sizes: Vec<SizeRow>,
}

/// `duct_size_table.set`: one shape's size table.
pub(crate) fn set_duct_sizes(txn: &mut Transaction<'_>, p: DuctSizeTableParams) -> Result<()> {
    let mut m = get_all(txn.store());
    let mut rows = p.sizes;
    rows.sort_by(|a, b| a.size.total_cmp(&b.size));
    *m.duct.sizes.of_mut(p.shape) = rows;
    save(txn, m)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DuctSizesParams {
    pub shape: Shape,
}

/// `duct_size_table.get`.
pub(crate) fn get_duct_sizes(store: &Store, p: DuctSizesParams) -> Result<Vec<SizeRow>> {
    Ok(get_all(store).duct.sizes.of(p.shape).clone())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PipeSegmentParams {
    /// The material's name.
    pub material_id: String,
    /// Its schedule or class ("Schedule 40", "PN 16").
    #[serde(default)]
    pub schedule: String,
    /// Metres.
    pub roughness: f64,
    pub sizes: Vec<PipeSize>,
}

/// `pipe_segment.create`: a segment; returns its id. The first one made replaces the built-in
/// Generic segment.
pub(crate) fn create_segment(txn: &mut Transaction<'_>, p: PipeSegmentParams) -> Result<String> {
    let mut m = get_all(txn.store());
    let taken: Vec<String> = m.pipe.segments.iter().map(|s| s.id.clone()).chain(m.pipe.fluids.iter().map(|f| f.id.clone())).collect();
    let mut sizes = p.sizes;
    sizes.sort_by(|a, b| a.nominal.total_cmp(&b.nominal));
    let s = PipeSegment { id: String::new(), material: p.material_id, schedule: p.schedule, roughness: p.roughness, sizes };
    let id = slug(&s.name(), &taken);
    if m.pipe.segments.iter().any(|x| x.name() == s.name()) {
        return Err(bad("pipe_segment.exists", format!("{} exists", s.name())));
    }
    m.pipe.segments.push(PipeSegment { id: id.clone(), ..s });
    save(txn, m)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FluidParams {
    pub name: String,
    pub table: Vec<FluidRow>,
}

/// `fluid.create`: a fluid for systems to name; returns its id.
pub(crate) fn create_fluid(txn: &mut Transaction<'_>, p: FluidParams) -> Result<String> {
    let mut m = get_all(txn.store());
    if m.pipe.fluids.iter().any(|f| f.name.eq_ignore_ascii_case(&p.name)) {
        return Err(bad("fluid.exists", format!("{} exists", p.name)));
    }
    let taken: Vec<String> = m.pipe.segments.iter().map(|s| s.id.clone()).chain(m.pipe.fluids.iter().map(|f| f.id.clone())).collect();
    let id = slug(&p.name, &taken);
    let mut table = p.table;
    table.sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
    m.pipe.fluids.push(Fluid { id: id.clone(), name: p.name, table });
    save(txn, m)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_and_fluids() {
        assert!(Angles::Any.allows(37.0));
        let inc = Angles::Increment { step: 15.0 };
        assert!(inc.allows(45.0) && inc.allows(90.0) && !inc.allows(40.0));
        let list = Angles::List { angles: vec![45.0, 90.0] };
        assert!(list.allows(90.0) && !list.allows(60.0));
        assert!(Angles::Increment { step: 0.0 }.check().is_err());
        let glycol = Fluid {
            id: "g".into(),
            name: "Glycol 30%".into(),
            table: vec![FluidRow { temperature: 0.0, density: 1050.0, viscosity: 0.004 }, FluidRow { temperature: 40.0, density: 1030.0, viscosity: 0.002 }],
        };
        let (rho, mu) = glycol.at(20.0).unwrap();
        assert!((rho - 1040.0).abs() < 1e-9 && (mu - 0.003).abs() < 1e-12);
        assert!(glycol.at(50.0).is_err());
        assert!(keep_slopes(&[0.01, 0.02]).is_err());
        assert_eq!(keep_slopes(&[0.0, 0.02, 0.01]).unwrap(), vec![0.01, 0.02]);
    }
}
