// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Daylight factor (ANA-LIGHT-002, ANA-ENV-006), the built-in provider. The editor distils each
//! space's sensor grid, its room surfaces and reflectances, and the model's obstructions and
//! glazing as triangles into a [`DaylightInput`]; the built-in runner (`analysis.daylight`, out
//! of process) calls [`run`]; the editor records the study.
//!
//! - **Sky component**: the CIE standard overcast sky (luminance ∝ (1 + 2 sin α)) split into
//!   patches; from each sensor a ray to each patch counts the patch's share of the unobstructed
//!   horizontal illuminance, times the visible transmittance of every pane it passes through,
//!   and nothing if it meets an opaque element.
//! - **Internally reflected component** (the split-flux room formula, per space):
//!   τ·W / (A·(1 − R)) · (C·R_fw + 5·R_cw) per cent, with W the glazing area, A the room's
//!   total surface area, R its area-weighted mean reflectance, R_fw the floor's and lower
//!   walls', R_cw the ceiling's and upper walls', C = 39 (an unobstructed sky).
//! - **Daylight factor** = sky component + internally reflected component, per sensor; each
//!   space's average, minimum, uniformity (minimum ÷ average) and the share of sensors at or
//!   above the threshold.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::energy_model::{self, SurfaceClass};
use crate::error::{Error, Result};
use crate::id::{ElementId, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

pub const KEY: &str = "rxd:daylight";
type V = [f64; 3];

/// The study's settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DaylightOptions {
    /// Sensor height above the floor, m.
    pub height: f64,
    /// Sensor spacing, m.
    pub spacing: f64,
    /// Sensors are kept this far from the space's boundary, m.
    pub border: f64,
    /// Daylight factor threshold, %.
    pub threshold: f64,
    pub ceiling_reflectance: f64,
    pub wall_reflectance: f64,
    pub floor_reflectance: f64,
    /// Visible transmittance of glazing whose type gives none.
    pub glazing_transmittance: f64,
    /// The levels to analyse; every level when empty.
    pub level_ids: Vec<ElementId>,
}

impl Default for DaylightOptions {
    fn default() -> Self {
        DaylightOptions { height: 0.85, spacing: 0.5, border: 0.5, threshold: 2.0, ceiling_reflectance: 0.7, wall_reflectance: 0.5, floor_reflectance: 0.2, glazing_transmittance: 0.7, level_ids: Vec::new() }
    }
}

impl DaylightOptions {
    pub fn check(&self) -> Result<()> {
        let r = |v: f64| (0.0..0.95).contains(&v);
        if !(0.0..=3.0).contains(&self.height) || !(0.1..=5.0).contains(&self.spacing) || !(0.0..=3.0).contains(&self.border) {
            return Err(bad("daylight.bad_option", "the grid needs a height of 0–3 m, a spacing of 0.1–5 m and a border of 0–3 m"));
        }
        if !r(self.ceiling_reflectance) || !r(self.wall_reflectance) || !r(self.floor_reflectance) || !(0.01..=1.0).contains(&self.glazing_transmittance) {
            return Err(bad("daylight.bad_option", "reflectances must be 0–0.95 and the glazing transmittance 0.01–1"));
        }
        if !(0.0..=100.0).contains(&self.threshold) {
            return Err(bad("daylight.bad_option", "the threshold must be 0–100%"));
        }
        Ok(())
    }
}

/// One space's sensors and room values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DaylightSpace {
    pub key: String,
    pub points: Vec<V>,
    /// m² of glazing.
    pub glazing: f64,
    /// Visible transmittance, area-weighted over the space's windows.
    pub transmittance: f64,
    /// m² of all room surfaces.
    pub surface_area: f64,
    pub mean_reflectance: f64,
    pub lower_reflectance: f64,
    pub upper_reflectance: f64,
}

/// An obstruction or a pane: its triangles, its bounds, and the transmittance (None: opaque).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Obstruction {
    pub triangles: Vec<[V; 3]>,
    pub lo: V,
    pub hi: V,
    pub transmittance: Option<f64>,
}

impl Obstruction {
    pub fn new(triangles: Vec<[V; 3]>, transmittance: Option<f64>) -> Obstruction {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for t in &triangles {
            for p in t {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        Obstruction { triangles, lo, hi, transmittance }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DaylightInput {
    pub spaces: Vec<DaylightSpace>,
    pub obstructions: Vec<Obstruction>,
    pub threshold: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpaceDaylight {
    pub key: String,
    pub points: Vec<V>,
    /// Daylight factor at each sensor, %.
    pub values: Vec<f64>,
    pub average: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub uniformity: f64,
    /// Share of the sensors at or above the threshold, 0–1.
    pub above: f64,
    /// The internally reflected component, %, included in every value.
    pub reflected: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DaylightResults {
    pub spaces: Vec<SpaceDaylight>,
    pub threshold: f64,
}

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The distance along the ray to a triangle, if it hits (Möller–Trumbore).
fn hit(o: V, d: V, t: &[V; 3]) -> Option<f64> {
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, t[0]);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(e2, q) * inv;
    (t > 1e-6).then_some(t)
}

/// Whether a ray meets a box (slab test).
fn meets_box(o: V, d: V, lo: V, hi: V) -> bool {
    let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
    for k in 0..3 {
        if d[k].abs() < 1e-12 {
            if o[k] < lo[k] - 1e-9 || o[k] > hi[k] + 1e-9 {
                return false;
            }
            continue;
        }
        let (a, b) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 + 1e-9 {
            return false;
        }
    }
    true
}

/// The overcast sky's patches: (direction, share of the unobstructed horizontal illuminance).
pub fn overcast_patches() -> Vec<(V, f64)> {
    let rings = 18;
    let step = std::f64::consts::FRAC_PI_2 / rings as f64;
    let mut out = Vec::new();
    for i in 0..rings {
        let (a0, a1) = (i as f64 * step, (i + 1) as f64 * step);
        let alt = (a0 + a1) / 2.0;
        let n = ((2.0 * std::f64::consts::PI * alt.cos() / step).round() as usize).max(1);
        let solid = 2.0 * std::f64::consts::PI * (a1.sin() - a0.sin()) / n as f64;
        for k in 0..n {
            let az = (k as f64 + 0.5) / n as f64 * 2.0 * std::f64::consts::PI;
            out.push(([alt.cos() * az.sin(), alt.cos() * az.cos(), alt.sin()], solid * alt.sin() * (1.0 + 2.0 * alt.sin())));
        }
    }
    let total: f64 = out.iter().map(|p| p.1).sum();
    for p in &mut out {
        p.1 /= total;
    }
    out
}

/// The transmittance along a ray to the sky: 0 when blocked.
fn to_sky(o: V, d: V, obstructions: &[Obstruction]) -> f64 {
    let mut tau = 1.0;
    for ob in obstructions {
        if !meets_box(o, d, ob.lo, ob.hi) {
            continue;
        }
        // A pane counts once even when the ray crosses both of its faces.
        let mut crossed = false;
        for t in &ob.triangles {
            if hit(o, d, t).is_some() {
                match ob.transmittance {
                    None => return 0.0,
                    Some(_) => crossed = true,
                }
            }
        }
        if crossed {
            tau *= ob.transmittance.unwrap_or(0.0);
        }
    }
    tau
}

/// Runs the study.
pub fn run(input: &DaylightInput) -> std::result::Result<DaylightResults, String> {
    let sky = overcast_patches();
    let mut out = DaylightResults { spaces: Vec::new(), threshold: input.threshold };
    for s in &input.spaces {
        let reflected = if s.surface_area > 0.0 && s.mean_reflectance < 1.0 { 100.0 * s.transmittance * s.glazing / (s.surface_area * (1.0 - s.mean_reflectance)) * (39.0 * s.lower_reflectance + 5.0 * s.upper_reflectance) / 100.0 } else { 0.0 };
        let values: Vec<f64> = s.points.iter().map(|p| 100.0 * sky.iter().map(|(d, w)| w * to_sky(*p, *d, &input.obstructions)).sum::<f64>() + reflected).collect();
        let n = values.len().max(1) as f64;
        let average = values.iter().sum::<f64>() / n;
        let minimum = values.iter().copied().fold(f64::INFINITY, f64::min);
        let minimum = if minimum.is_finite() { minimum } else { 0.0 };
        out.spaces.push(SpaceDaylight {
            key: s.key.clone(),
            points: s.points.clone(),
            average,
            minimum,
            maximum: values.iter().copied().fold(0.0, f64::max),
            uniformity: if average > 0.0 { minimum / average } else { 0.0 },
            above: values.iter().filter(|v| **v >= input.threshold).count() as f64 / n,
            reflected,
            values,
        });
    }
    Ok(out)
}

// ---- In the editor ----

const OBSTRUCTING: &[&str] = &["IfcWall", "IfcWallStandardCase", "IfcSlab", "IfcRoof", "IfcCovering", "IfcColumn", "IfcBeam", "IfcMember", "IfcDoor", "IfcWindow", "IfcCurtainWall", "IfcPlate", "IfcStair", "IfcStairFlight", "IfcRamp", "IfcRampFlight", "IfcBuildingElementProxy", "IfcChimney", "IfcShadingDevice", "IfcGeographicElement"];

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.get("value")?.as_f64())
}

/// A window's visible transmittance from its type (`Pset_DoorWindowGlazingType.VisibleLightTransmittance`).
pub(crate) fn transmittance_of(store: &Store, e: &crate::element::Element) -> Option<f64> {
    let t = e.param("@type").and_then(Value::as_str).and_then(GlobalId::parse).and_then(|g| store.find_by_guid(&g)).and_then(|id| store.get(id)).unwrap_or(e);
    t.param("Pset_DoorWindowGlazingType.VisibleLightTransmittance").and_then(num).filter(|v| (0.0..=1.0).contains(v))
}

fn inside(poly: &[[f64; 2]], q: [f64; 2]) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

/// The squared distance from a point to a polygon's boundary.
fn edge_distance2(poly: &[[f64; 2]], q: [f64; 2]) -> f64 {
    let mut best = f64::INFINITY;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let ab = [b[0] - a[0], b[1] - a[1]];
        let l2 = ab[0] * ab[0] + ab[1] * ab[1];
        let t = if l2 > 0.0 { (((q[0] - a[0]) * ab[0] + (q[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let p = [a[0] + ab[0] * t - q[0], a[1] + ab[1] * t - q[1]];
        best = best.min(p[0] * p[0] + p[1] * p[1]);
    }
    best
}

/// A space's sensor grid: points inside the floor outline, `border` from its edges, at
/// `height` above it (the centroid when none fit).
pub fn sensor_grid(outline: &[V], height: f64, spacing: f64, border: f64) -> Vec<V> {
    if outline.len() < 3 {
        return Vec::new();
    }
    let z = outline.iter().map(|p| p[2]).sum::<f64>() / outline.len() as f64 + height;
    let poly: Vec<[f64; 2]> = outline.iter().map(|p| [p[0], p[1]]).collect();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for q in &poly {
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let s = spacing.max(0.05);
    // Centre the grid in the box.
    let start = |k: usize| lo[k] + ((hi[k] - lo[k]) % s) / 2.0;
    let mut pts = Vec::new();
    let mut x = start(0);
    while x <= hi[0] + 1e-9 {
        let mut y = start(1);
        while y <= hi[1] + 1e-9 {
            if inside(&poly, [x, y]) && edge_distance2(&poly, [x, y]) >= border * border - 1e-9 {
                pts.push([x, y, z]);
            }
            y += s;
        }
        x += s;
    }
    if pts.is_empty() {
        let c = poly.iter().fold([0.0, 0.0], |a, q| [a[0] + q[0] / poly.len() as f64, a[1] + q[1] / poly.len() as f64]);
        pts.push([c[0], c[1], z]);
    }
    pts
}

/// The distillation: the spaces (opaque keys, on the chosen levels) and the obstructions, with
/// the key → GlobalId map.
pub fn distill(store: &Store, o: &DaylightOptions) -> (DaylightInput, BTreeMap<String, String>) {
    let model = energy_model::build(store);
    let mut map = BTreeMap::new();
    let mut spaces = Vec::new();
    for a in &model.spaces {
        if !o.level_ids.is_empty() && !a.level.is_some_and(|l| o.level_ids.contains(&l)) {
            continue;
        }
        let Some(floor) = a.surfaces.iter().find(|f| matches!(f.class, SurfaceClass::SlabOnGrade | SurfaceClass::InteriorFloor)) else { continue };
        let key = format!("s{}", spaces.len() + 1);
        if let Some(e) = store.get(a.space) {
            map.insert(key.clone(), e.guid().to_string());
        }
        let (mut glazing, mut tau_area) = (0.0, 0.0);
        for w in a.surfaces.iter().filter(|f| f.class == SurfaceClass::Window) {
            let tau = w.element.and_then(|id| store.get(id)).and_then(|e| transmittance_of(store, e)).unwrap_or(o.glazing_transmittance);
            glazing += w.area;
            tau_area += tau * w.area;
        }
        // Room surfaces: floor and ceiling at the floor area; walls at the perimeter × height
        // (their windows and doors included, glass reflecting 0.1).
        let walls: f64 = a.surfaces.iter().filter(|f| matches!(f.class, SurfaceClass::ExteriorWall | SurfaceClass::InteriorWall | SurfaceClass::Air)).map(|f| f.area).sum::<f64>()
            + a.surfaces.iter().filter(|f| f.class == SurfaceClass::Door).map(|f| f.area).sum::<f64>();
        let wall_r = if walls + glazing > 0.0 { (walls * o.wall_reflectance + glazing * 0.1) / (walls + glazing) } else { o.wall_reflectance };
        let (f, c, w) = (a.area, a.area, walls + glazing);
        let total = f + c + w;
        let mean = if total > 0.0 { (f * o.floor_reflectance + c * o.ceiling_reflectance + w * wall_r) / total } else { 0.5 };
        let lower = if f + w / 2.0 > 0.0 { (f * o.floor_reflectance + w / 2.0 * wall_r) / (f + w / 2.0) } else { o.floor_reflectance };
        let upper = if c + w / 2.0 > 0.0 { (c * o.ceiling_reflectance + w / 2.0 * wall_r) / (c + w / 2.0) } else { o.ceiling_reflectance };
        spaces.push(DaylightSpace {
            key,
            points: sensor_grid(&floor.outline, o.height, o.spacing, o.border),
            glazing,
            transmittance: if glazing > 0.0 { tau_area / glazing } else { o.glazing_transmittance },
            surface_area: total,
            mean_reflectance: mean,
            lower_reflectance: lower,
            upper_reflectance: upper,
        });
    }
    let mut obstructions = Vec::new();
    for b in crate::bodies::all_but_reinforcement(store) {
        let Some(e) = store.get(b.element) else { continue };
        if !OBSTRUCTING.contains(&e.class.as_str()) {
            continue;
        }
        let tau = (e.class == "IfcWindow").then(|| transmittance_of(store, e).unwrap_or(o.glazing_transmittance));
        let mut tris = Vec::new();
        for p in &b.parts {
            for t in p.mesh.1.as_chunks::<3>().0 {
                tris.push([p.mesh.0[t[0] as usize], p.mesh.0[t[1] as usize], p.mesh.0[t[2] as usize]]);
            }
        }
        if !tris.is_empty() {
            obstructions.push(Obstruction::new(tris, tau));
        }
    }
    (DaylightInput { spaces, obstructions, threshold: o.threshold }, map)
}

/// A study as kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DaylightStudy {
    pub id: String,
    pub at: i64,
    pub options: DaylightOptions,
    pub results: DaylightResults,
    pub key_map: BTreeMap<String, String>,
    pub inputs_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DaylightData {
    pub studies: Vec<DaylightStudy>,
}

pub fn data(store: &Store) -> DaylightData {
    store.iter().find(|e| e.class == "IfcProject").and_then(|p| p.param(KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordParams {
    pub job_id: String,
    pub options: DaylightOptions,
    pub results: DaylightResults,
    pub key_map: BTreeMap<String, String>,
    pub inputs_hash: String,
    #[serde(default)]
    pub at: i64,
}

/// `daylight.record`: keeps a study on the project (the last five, as they carry grids).
pub(crate) fn record(txn: &mut Transaction<'_>, p: RecordParams) -> Result<DaylightStudy> {
    let st = DaylightStudy { id: p.job_id, at: p.at, options: p.options, results: p.results, key_map: p.key_map, inputs_hash: p.inputs_hash };
    let store = txn.store().clone();
    let mut d = data(&store);
    d.studies.retain(|x| x.id != st.id);
    d.studies.push(st.clone());
    if d.studies.len() > 5 {
        let n = d.studies.len() - 5;
        d.studies.drain(..n);
    }
    let pid = store.iter().find(|e| e.class == "IfcProject").map(crate::element::Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(&d).expect("serialises");
    txn.modify(pid, |e| {
        e.params.insert(KEY.into(), v);
    })?;
    Ok(st)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReportSpace {
    pub space: Option<ElementId>,
    pub number: String,
    pub name: String,
    pub sensors: usize,
    pub average: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub uniformity: f64,
    pub above: f64,
    pub reflected: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    pub id: String,
    pub options: DaylightOptions,
    pub threshold: f64,
    pub spaces: Vec<ReportSpace>,
    /// The sensor-weighted average over every space, %.
    pub average: f64,
    /// Share of all sensors at or above the threshold.
    pub above: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    pub study_id: Option<String>,
}

pub fn study(store: &Store, id: Option<&str>) -> Result<DaylightStudy> {
    let d = data(store);
    match id {
        Some(id) => d.studies.into_iter().find(|x| x.id == id),
        None => d.studies.into_iter().last(),
    }
    .ok_or_else(|| bad("daylight.no_study", "no daylight study yet"))
}

/// `daylight.results`.
pub(crate) fn report(store: &Store, p: ReportParams) -> Result<Report> {
    let st = study(store, p.study_id.as_deref())?;
    let find = |k: &str| st.key_map.get(k).and_then(|g| GlobalId::parse(g)).and_then(|g| store.find_by_guid(&g));
    let n: usize = st.results.spaces.iter().map(|s| s.values.len()).sum();
    let sum: f64 = st.results.spaces.iter().flat_map(|s| s.values.iter()).sum();
    let above = st.results.spaces.iter().flat_map(|s| s.values.iter()).filter(|v| **v >= st.results.threshold).count();
    Ok(Report {
        id: st.id.clone(),
        options: st.options.clone(),
        threshold: st.results.threshold,
        average: if n > 0 { sum / n as f64 } else { 0.0 },
        above: if n > 0 { above as f64 / n as f64 } else { 0.0 },
        spaces: st
            .results
            .spaces
            .iter()
            .map(|s| {
                let id = find(&s.key);
                let (number, name) = id.and_then(|i| store.get(i)).map(crate::rooms::number_and_name).unwrap_or_default();
                ReportSpace { space: id, number, name, sensors: s.values.len(), average: s.average, minimum: s.minimum, maximum: s.maximum, uniformity: s.uniformity, above: s.above, reflected: s.reflected }
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(a: V, b: V, c: V, d: V) -> Vec<[V; 3]> {
        vec![[a, b, c], [a, c, d]]
    }

    fn space(points: Vec<V>) -> DaylightSpace {
        DaylightSpace { key: "s1".into(), points, glazing: 0.0, transmittance: 0.7, surface_area: 0.0, mean_reflectance: 0.5, lower_reflectance: 0.3, upper_reflectance: 0.6 }
    }

    /// An unobstructed point sees the whole sky: 100%.
    #[test]
    fn open_sky_is_a_hundred() {
        let r = run(&DaylightInput { spaces: vec![space(vec![[0.0, 0.0, 0.0]])], obstructions: Vec::new(), threshold: 2.0 }).unwrap();
        assert!((r.spaces[0].average - 100.0).abs() < 1e-9);
    }

    /// Under a horizontal pane of transmittance 0.6, 60%; under a roof, 0; a roof with a pane over
    /// the point's zenith alone gives a small share.
    #[test]
    fn panes_and_roofs() {
        let big = 1000.0;
        let pane = Obstruction::new(quad([-big, -big, 1.0], [big, -big, 1.0], [big, big, 1.0], [-big, big, 1.0]), Some(0.6));
        let r = run(&DaylightInput { spaces: vec![space(vec![[0.0, 0.0, 0.0]])], obstructions: vec![pane], threshold: 2.0 }).unwrap();
        assert!((r.spaces[0].average - 60.0).abs() < 0.5, "{}", r.spaces[0].average);
        let roof = Obstruction::new(quad([-big, -big, 1.0], [big, -big, 1.0], [big, big, 1.0], [-big, big, 1.0]), None);
        let r = run(&DaylightInput { spaces: vec![space(vec![[0.0, 0.0, 0.0]])], obstructions: vec![roof], threshold: 2.0 }).unwrap();
        assert_eq!(r.spaces[0].average, 0.0);
        assert_eq!(r.spaces[0].above, 0.0);
    }

    /// The reflected component by hand: τ 0.7, W 4 m², A 100 m², R 0.5, R_fw 0.3, R_cw 0.6.
    #[test]
    fn reflected_by_hand() {
        let mut s = space(vec![[0.0, 0.0, 0.0]]);
        s.glazing = 4.0;
        s.surface_area = 100.0;
        let big = 1000.0;
        let roof = Obstruction::new(quad([-big, -big, 1.0], [big, -big, 1.0], [big, big, 1.0], [-big, big, 1.0]), None);
        let r = run(&DaylightInput { spaces: vec![s], obstructions: vec![roof], threshold: 2.0 }).unwrap();
        let expect = 0.7 * 4.0 / (100.0 * 0.5) * (39.0 * 0.3 + 5.0 * 0.6);
        assert!((r.spaces[0].reflected - expect).abs() < 1e-9 && (r.spaces[0].average - expect).abs() < 1e-9);
    }

    #[test]
    fn grid_keeps_its_border() {
        let sq = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 4.0, 0.0], [0.0, 4.0, 0.0]];
        let g = sensor_grid(&sq, 0.85, 0.5, 0.5);
        assert_eq!(g.len(), 49, "{g:?}");
        assert!(g.iter().all(|p| p[0] >= 0.5 - 1e-9 && p[0] <= 3.5 + 1e-9 && (p[2] - 0.85).abs() < 1e-9));
    }
}
