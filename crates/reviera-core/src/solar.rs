// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Solar analysis (ANA-SOLAR-001), the calculation: cumulative insolation on grid points of
//! analysed faces over a period, and PV potential per face. It is pure: the editor distils the
//! faces, the shading context and the weather into a [`SolarInput`], and the built-in runner
//! (`analysis.solar`, out of process) calls [`run`].
//!
//! - **Sun**: its position each hour from the day of the year, the hour (local standard time,
//!   at the half hour), the site's latitude, longitude and time zone, by the usual declination
//!   and equation-of-time approximations; azimuths are turned from true north to the model's
//!   axes by the project's true north.
//! - **Direct**: for each hour with the sun up and direct normal irradiance (DNI) in the
//!   weather, DNI × cos(incidence) at each grid point the sun reaches (a ray to the sun misses
//!   the shading context).
//! - **Diffuse**: an isotropic sky of the hour's diffuse horizontal irradiance (DHI), split into
//!   patches (rings of equal altitude steps, each ring cut into patches of near-equal solid
//!   angle); each patch's share is its weight × cos(incidence) where it is seen from the point.
//! - **Faces** are planar polygons with an outward normal; grid points sit at the centres of a
//!   square grid of the given spacing on each face (at least the face's centroid), lifted 1 mm
//!   off it so the face itself does not shade them.
//! - **PV**: usable area = area × coverage; annual energy = mean insolation × usable area ×
//!   efficiency × derate (scaled to a year when the period is shorter); cost = peak kW (usable
//!   area × efficiency × 1 kW/m²) × cost per kWp; payback = cost ÷ (annual energy × price).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A face to analyse: its outline (planar, counter-clockwise seen from outside) and an id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Face {
    pub key: String,
    pub outline: Vec<[f64; 3]>,
}

/// One hour of the weather: month, day, hour (1–24, ending at the hour), DNI and DHI (W/m²).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Hour {
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub dni: f64,
    pub dhi: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PvSettings {
    pub coverage: f64,
    pub efficiency: f64,
    pub derate: f64,
    /// Currency per kW peak.
    pub cost_per_kwp: f64,
    /// Currency per kWh.
    pub energy_price: f64,
}

impl Default for PvSettings {
    fn default() -> Self {
        // Spec defaults (ANA-SOLAR-001); cost and price are placeholders (EXPERT_REVIEW E-36).
        PvSettings { coverage: 0.8, efficiency: 0.18, derate: 0.86, cost_per_kwp: 1200.0, energy_price: 0.25 }
    }
}

/// What a solar run needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SolarInput {
    pub latitude: f64,
    pub longitude: f64,
    pub time_zone: f64,
    /// Degrees from project north to true north, counter-clockwise.
    pub true_north: f64,
    /// The period: first and last (month, day), inclusive.
    pub from: [u8; 2],
    pub to: [u8; 2],
    pub hours: Vec<Hour>,
    pub faces: Vec<Face>,
    /// The shading context's triangles.
    pub shading: Vec<[[f64; 3]; 3]>,
    /// Metres between grid points.
    pub grid: f64,
    pub pv: Option<PvSettings>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FaceResult {
    pub key: String,
    pub area: f64,
    /// The face's outward unit normal (for drawing its grid).
    #[serde(default)]
    pub normal: [f64; 3],
    /// kWh/m² at each grid point, and the points.
    pub points: Vec<[f64; 3]>,
    pub values: Vec<f64>,
    /// kWh/m² averaged over the points, and kWh over the face.
    pub mean: f64,
    pub total: f64,
    pub pv_kwh_year: Option<f64>,
    pub pv_cost: Option<f64>,
    pub pv_payback_years: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SolarResults {
    pub faces: Vec<FaceResult>,
    /// Hours in the period with the sun up.
    pub sun_hours: usize,
    /// The period's share of a year (for PV).
    pub year_fraction: f64,
}

type V = [f64; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: V) -> V {
    let l = dot(a, a).sqrt().max(1e-15);
    [a[0] / l, a[1] / l, a[2] / l]
}

const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn day_of_year(month: u8, day: u8) -> u32 {
    DAYS[..(month.clamp(1, 12) as usize - 1)].iter().sum::<u32>() + day as u32
}

/// The sun's direction (unit, z up, in the model's axes) at a day of the year and a local
/// standard hour, or none below the horizon.
pub fn sun(input: &SolarInput, doy: u32, hour: f64) -> Option<V> {
    let g = 2.0 * std::f64::consts::PI / 365.0 * (doy as f64 - 1.0 + (hour - 12.0) / 24.0);
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos() + 0.000907 * (2.0 * g).sin() - 0.002697 * (3.0 * g).cos() + 0.00148 * (3.0 * g).sin();
    let eot = 229.18 * (0.000075 + 0.001868 * g.cos() - 0.032077 * g.sin() - 0.014615 * (2.0 * g).cos() - 0.040849 * (2.0 * g).sin());
    // Solar time in minutes, then the hour angle (radians, negative in the morning).
    let solar_min = hour * 60.0 + eot + 4.0 * input.longitude - 60.0 * input.time_zone;
    let ha = (solar_min / 4.0 - 180.0).to_radians();
    let lat = input.latitude.to_radians();
    let cos_zen = lat.sin() * decl.sin() + lat.cos() * decl.cos() * ha.cos();
    if cos_zen <= 0.0 {
        return None;
    }
    let zen = cos_zen.clamp(-1.0, 1.0).acos();
    // Azimuth clockwise from true north.
    let mut az = ((lat.sin() * cos_zen - decl.sin()) / (lat.cos() * zen.sin()).max(1e-12)).clamp(-1.0, 1.0).acos();
    az = if ha > 0.0 { std::f64::consts::PI + az } else { std::f64::consts::PI - az };
    // East and north components in true axes, then into model axes.
    let (e, n) = (zen.sin() * az.sin(), zen.sin() * az.cos());
    let t = input.true_north.to_radians();
    // True north in model axes is (−sin t, cos t); true east is (cos t, sin t).
    let x = e * t.cos() - n * t.sin();
    let y = e * t.sin() + n * t.cos();
    Some(norm([x, y, cos_zen]))
}

/// Sky patches: (direction, weight) with weights summing to 1 for an isotropic sky on a
/// horizontal surface (each ∝ solid angle × cos(zenith)).
pub fn sky_patches() -> Vec<(V, f64)> {
    let rings = 8;
    let mut out = Vec::new();
    let step = std::f64::consts::FRAC_PI_2 / rings as f64;
    for i in 0..rings {
        let (a0, a1) = (i as f64 * step, (i + 1) as f64 * step);
        let alt = (a0 + a1) / 2.0;
        // Patches in the ring: about as wide as high.
        let n = ((2.0 * std::f64::consts::PI * alt.cos() / step).round() as usize).max(1);
        let solid = 2.0 * std::f64::consts::PI * (a1.sin() - a0.sin()) / n as f64;
        for k in 0..n {
            let az = (k as f64 + 0.5) / n as f64 * 2.0 * std::f64::consts::PI;
            let d = [alt.cos() * az.sin(), alt.cos() * az.cos(), alt.sin()];
            out.push((d, solid * alt.sin()));
        }
    }
    let total: f64 = out.iter().map(|p| p.1).sum();
    for p in &mut out {
        p.1 /= total;
    }
    out
}

/// Whether a ray from `o` along `d` hits a triangle (Möller–Trumbore).
fn hits(o: V, d: V, t: &[V; 3]) -> bool {
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return false;
    }
    let inv = 1.0 / det;
    let s = sub(o, t[0]);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    dot(e2, q) * inv > 1e-6
}

/// A face's normal (Newell's method), area and grid points.
fn face_grid(f: &Face, spacing: f64) -> (V, f64, Vec<V>) {
    let p = &f.outline;
    let mut n = [0.0; 3];
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let area = dot(n, n).sqrt() / 2.0;
    let n = norm(n);
    // A frame on the face: u along the first edge, v = n × u.
    let u = norm(sub(p[1 % p.len()], p[0]));
    let v = cross(n, u);
    let loc: Vec<[f64; 2]> = p.iter().map(|q| [dot(sub(*q, p[0]), u), dot(sub(*q, p[0]), v)]).collect();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for q in &loc {
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let inside = |q: [f64; 2]| {
        let mut c = false;
        for i in 0..loc.len() {
            let (a, b) = (loc[i], loc[(i + 1) % loc.len()]);
            if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0] {
                c = !c;
            }
        }
        c
    };
    let s = spacing.max(0.05);
    let mut pts = Vec::new();
    let mut x = lo[0] + s / 2.0;
    while x < hi[0] {
        let mut y = lo[1] + s / 2.0;
        while y < hi[1] {
            if inside([x, y]) {
                pts.push([x, y]);
            }
            y += s;
        }
        x += s;
    }
    if pts.is_empty() {
        let c = loc.iter().fold([0.0, 0.0], |a, q| [a[0] + q[0] / loc.len() as f64, a[1] + q[1] / loc.len() as f64]);
        pts.push(c);
    }
    let world = pts.iter().map(|q| {
        let w = [p[0][0] + u[0] * q[0] + v[0] * q[1], p[0][1] + u[1] * q[0] + v[1] * q[1], p[0][2] + u[2] * q[0] + v[2] * q[1]];
        [w[0] + n[0] * 1e-3, w[1] + n[1] * 1e-3, w[2] + n[2] * 1e-3]
    });
    (n, area, world.collect())
}

fn in_period(input: &SolarInput, h: &Hour) -> bool {
    let d = day_of_year(h.month, h.day);
    let (a, b) = (day_of_year(input.from[0], input.from[1]), day_of_year(input.to[0], input.to[1]));
    if a <= b { (a..=b).contains(&d) } else { d >= a || d <= b }
}

/// Runs a solar analysis.
pub fn run(input: &SolarInput) -> std::result::Result<SolarResults, String> {
    if !(-90.0..=90.0).contains(&input.latitude) {
        return Err("the latitude is out of range".into());
    }
    if !(0.1..=10.0).contains(&input.grid) {
        return Err("the grid spacing runs from 0.1 to 10 m".into());
    }
    let sky = sky_patches();
    let hours: Vec<&Hour> = input.hours.iter().filter(|h| in_period(input, h)).collect();
    let suns: Vec<Option<V>> = hours.iter().map(|h| sun(input, day_of_year(h.month, h.day), h.hour as f64 - 0.5)).collect();
    let dhi_total: f64 = hours.iter().map(|h| h.dhi.max(0.0)).sum::<f64>() / 1000.0;
    let blocked = |o: V, d: V| input.shading.iter().any(|t| hits(o, d, t));
    let mut faces = Vec::new();
    for f in &input.faces {
        if f.outline.len() < 3 {
            continue;
        }
        let (n, area, pts) = face_grid(f, input.grid);
        let mut values = Vec::with_capacity(pts.len());
        for p in &pts {
            // Diffuse: the visible share of the sky, by patch.
            let sky_share: f64 = sky.iter().filter(|(d, _)| dot(*d, n) > 0.0 && !blocked(*p, *d))// A weight is solid angle × cos(zenith) ÷ π; on the face it counts with cos(incidence).
                .map(|(d, w)| w * dot(*d, n) / d[2].max(1e-9))
                .sum();
            let mut kwh = dhi_total * sky_share;
            for (h, s) in hours.iter().zip(&suns) {
                let Some(s) = s else { continue };
                let c = dot(*s, n);
                if c > 0.0 && h.dni > 0.0 && !blocked(*p, *s) {
                    kwh += h.dni * c / 1000.0;
                }
            }
            values.push(kwh);
        }
        let mean = values.iter().sum::<f64>() / values.len().max(1) as f64;
        faces.push(FaceResult { key: f.key.clone(), area, normal: n, points: pts, values, mean, total: mean * area, pv_kwh_year: None, pv_cost: None, pv_payback_years: None });
    }
    let year_fraction = (hours.len() as f64 / 8760.0).clamp(1e-9, 1.0);
    if let Some(pv) = &input.pv {
        for f in &mut faces {
            let usable = f.area * pv.coverage;
            let kwh = f.mean / year_fraction * usable * pv.efficiency * pv.derate;
            let cost = usable * pv.efficiency * pv.cost_per_kwp;
            f.pv_kwh_year = Some(kwh);
            f.pv_cost = Some(cost);
            f.pv_payback_years = (kwh * pv.energy_price > 0.0).then(|| cost / (kwh * pv.energy_price));
        }
    }
    Ok(SolarResults { faces, sun_hours: suns.iter().filter(|s| s.is_some()).count(), year_fraction })
}

/// PV energy per year from a mean insolation (kWh/m² a year) and a face area, as the spec's
/// formula: insolation × area × coverage × efficiency × derate.
pub fn pv_energy(insolation: f64, area: f64, pv: &PvSettings) -> f64 {
    insolation * area * pv.coverage * pv.efficiency * pv.derate
}

// ---- In the editor: distillation, recording, results ----

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::id::ElementId;
use crate::store::Store;
use crate::txn::Transaction;

pub const KEY: &str = "rxd:solar";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The hourly records of an EPW file's text: month, day, hour, DNI (field 14) and DHI (15).
pub fn epw_hours(text: &str) -> Result<Vec<Hour>> {
    let mut out = Vec::new();
    for (k, line) in text.lines().enumerate().skip(8) {
        if line.trim().is_empty() {
            continue;
        }
        let c: Vec<&str> = line.split(',').collect();
        let n = k + 1;
        if c.len() < 16 {
            return Err(bad("weather.bad_epw", format!("line {n}: an hourly record has {} fields", c.len())));
        }
        let num = |i: usize| c[i].trim().parse::<f64>().map_err(|_| bad("weather.bad_epw", format!("line {n}: field {} is not a number", i + 1)));
        let (dni, dhi) = (num(14)?, num(15)?);
        // 9999 marks a missing value.
        let ok = |v: f64| if (0.0..2000.0).contains(&v) { v } else { 0.0 };
        out.push(Hour { month: num(1)? as u8, day: num(2)? as u8, hour: num(3)? as u8, dni: ok(dni), dhi: ok(dhi) });
    }
    if out.len() < 8760 {
        return Err(bad("weather.short_epw", format!("the weather file has {} hourly records, not a year", out.len())));
    }
    Ok(out)
}

/// Which faces a study analyses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Surfaces {
    /// Every roof's top faces.
    Roofs,
    /// Every mass face facing up or sideways.
    Masses,
    /// Roofs and masses of these elements only.
    Elements(Vec<crate::id::ElementRef>),
}

/// The faces to analyse: (element, face index, face), each keyed `<GlobalId>/<index>`.
pub fn analysed_faces(store: &Store, which: &Surfaces) -> Result<Vec<(ElementId, Face)>> {
    let pick: Option<Vec<ElementId>> = match which {
        Surfaces::Elements(refs) => Some(refs.iter().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))).collect::<Result<_>>()?),
        _ => None,
    };
    let wanted = |id: ElementId| pick.as_ref().is_none_or(|p| p.contains(&id));
    let mut out = Vec::new();
    for e in store.iter() {
        if !wanted(e.id()) {
            continue;
        }
        let roofs = matches!(which, Surfaces::Roofs | Surfaces::Elements(_));
        let masses = matches!(which, Surfaces::Masses | Surfaces::Elements(_));
        if roofs && crate::roofs::roof_recipe(e).is_some() {
            let Ok(g) = crate::roofs::regenerate(store, e) else { continue };
            for (k, f) in g.faces.iter().enumerate() {
                // The top surface: the underside lifted by the thickness along the normal.
                let n = f.normal;
                let outline: Vec<[f64; 3]> = f.bottom.iter().map(|p| [p[0] + n[0] * g.thickness, p[1] + n[1] * g.thickness, p[2] + n[2] * g.thickness]).collect();
                if outline.len() >= 3 {
                    out.push((e.id(), Face { key: format!("{}/{k}", e.guid().as_str()), outline }));
                }
            }
        }
        if masses && crate::masses::recipe(e).is_some() {
            for (k, f) in crate::masses::faces(store, e).into_iter().enumerate() {
                // Undersides see no sky.
                if f.normal[2] > -0.1 && f.points.len() >= 3 {
                    out.push((e.id(), Face { key: format!("{}/{k}", e.guid().as_str()), outline: f.points }));
                }
            }
        }
    }
    Ok(out)
}

/// The shading context: every model element's body as triangles, except excluded categories.
pub fn shading(store: &Store, excluded: &[String]) -> Vec<[[f64; 3]; 3]> {
    let mut out = Vec::new();
    for b in crate::bodies::all_but_reinforcement(store) {
        if store.get(b.element).and_then(crate::categories::category_of).is_some_and(|c| excluded.iter().any(|k| k == c.key)) {
            continue;
        }
        for p in &b.parts {
            for t in p.mesh.1.as_chunks::<3>().0 {
                out.push([p.mesh.0[t[0] as usize], p.mesh.0[t[1] as usize], p.mesh.0[t[2] as usize]]);
            }
        }
    }
    out
}

/// A study as kept: what it ran on and its results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Study {
    /// The job it came from.
    pub id: String,
    pub at: i64,
    /// `insolation` or `pv`.
    pub study_type: String,
    pub from: [u8; 2],
    pub to: [u8; 2],
    pub grid: f64,
    pub pv: Option<PvSettings>,
    pub results: SolarResults,
    /// A hash of the faces, shading, location and weather it ran on, for staleness.
    pub inputs_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SolarData {
    pub studies: Vec<Study>,
}

pub fn data(store: &Store) -> SolarData {
    store.iter().find(|e| e.class == "IfcProject").and_then(|p| p.param(KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// A short hash of anything serialisable (FNV-1a over its JSON).
pub fn hash_of<T: Serialize>(v: &T) -> String {
    let bytes = serde_json::to_vec(v).unwrap_or_default();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The hash of a study's inputs, for staleness (the weather by its revision).
pub fn inputs_hash(input: &SolarInput, weather_revision: u64) -> String {
    hash_of(&(input.latitude, input.longitude, input.time_zone, input.true_north, &input.faces, &input.shading, input.grid, input.from, input.to, weather_revision))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordParams {
    pub job_id: String,
    pub study_type: String,
    pub from: [u8; 2],
    pub to: [u8; 2],
    pub grid: f64,
    pub pv: Option<PvSettings>,
    pub results: SolarResults,
    pub inputs_hash: String,
    #[serde(default)]
    pub at: i64,
}

/// `solar.record`: keeps a finished study (the last ten).
pub(crate) fn record(txn: &mut Transaction<'_>, p: RecordParams) -> Result<Study> {
    let st = Study { id: p.job_id, at: p.at, study_type: p.study_type, from: p.from, to: p.to, grid: p.grid, pv: p.pv, results: p.results, inputs_hash: p.inputs_hash };
    let mut d = data(txn.store());
    d.studies.retain(|x| x.id != st.id);
    d.studies.push(st.clone());
    if d.studies.len() > 10 {
        let n = d.studies.len() - 10;
        d.studies.drain(..n);
    }
    let pid = txn.store().iter().find(|e| e.class == "IfcProject").map(crate::element::Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(&d).expect("serialises");
    txn.modify(pid, |e| {
        e.params.insert(KEY.into(), v);
    })?;
    Ok(st)
}

/// A face's results, by its element and index.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FaceRow {
    pub key: String,
    pub element: Option<ElementId>,
    pub area: f64,
    pub mean: f64,
    pub total: f64,
    pub pv_kwh_year: Option<f64>,
    pub pv_cost: Option<f64>,
    pub pv_payback_years: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StudyReport {
    pub id: String,
    pub study_type: String,
    pub from: [u8; 2],
    pub to: [u8; 2],
    pub sun_hours: usize,
    pub faces: Vec<FaceRow>,
    /// kWh over every face, and PV kWh a year.
    pub total: f64,
    pub pv_total: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResultsParams {
    /// A study's id; the latest when omitted.
    pub study_id: Option<String>,
}

/// `solar.results`: a study's per-face results (the stale check is the app's, which can read
/// the weather file).
pub(crate) fn results(store: &Store, p: ResultsParams) -> Result<StudyReport> {
    let d = data(store);
    let st = match &p.study_id {
        Some(id) => d.studies.iter().find(|s| &s.id == id),
        None => d.studies.last(),
    }
    .ok_or_else(|| bad("solar.no_study", "no solar study yet"))?;
    let faces: Vec<FaceRow> = st
        .results
        .faces
        .iter()
        .map(|f| FaceRow {
            element: f.key.split('/').next().and_then(crate::id::GlobalId::parse).and_then(|g| store.find_by_guid(&g)),
            key: f.key.clone(),
            area: f.area,
            mean: f.mean,
            total: f.total,
            pv_kwh_year: f.pv_kwh_year,
            pv_cost: f.pv_cost,
            pv_payback_years: f.pv_payback_years,
        })
        .collect();
    Ok(StudyReport {
        id: st.id.clone(),
        study_type: st.study_type.clone(),
        from: st.from,
        to: st.to,
        sun_hours: st.results.sun_hours,
        total: faces.iter().map(|f| f.total).sum(),
        pv_total: st.pv.as_ref().map(|_| faces.iter().filter_map(|f| f.pv_kwh_year).sum()),
        faces,
    })
}

/// The grid values of a study's faces, by element (for drawing).
pub fn grid_values(store: &Store, st: &Study) -> BTreeMap<String, (Vec<[f64; 3]>, Vec<f64>)> {
    let _ = store;
    st.results.faces.iter().map(|f| (f.key.clone(), (f.points.clone(), f.values.clone()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(lat: f64) -> SolarInput {
        let mut hours = Vec::new();
        for (m, d) in DAYS.iter().enumerate() {
            for day in 1..=*d {
                for h in 1..=24 {
                    hours.push(Hour { month: m as u8 + 1, day: day as u8, hour: h, dni: if (7..=18).contains(&h) { 600.0 } else { 0.0 }, dhi: if (7..=18).contains(&h) { 100.0 } else { 0.0 } });
                }
            }
        }
        SolarInput { latitude: lat, longitude: 0.0, time_zone: 0.0, true_north: 0.0, from: [1, 1], to: [12, 31], hours, faces: Vec::new(), shading: Vec::new(), grid: 1.0, pv: None }
    }

    #[test]
    fn the_sun_at_noon_on_the_equinox() {
        let i = input(52.0);
        let s = sun(&i, 80, 12.0).unwrap();
        // Due south, at about 90° − 52° altitude.
        assert!(s[0].abs() < 0.05 && s[1] < 0.0, "{s:?}");
        let alt = s[2].asin().to_degrees();
        assert!((alt - 38.0).abs() < 1.5, "{alt}");
        assert!(sun(&i, 80, 0.5).is_none(), "night");
    }

    #[test]
    fn sky_weights_make_a_whole_sky() {
        let s = sky_patches();
        let horiz: f64 = s.iter().map(|p| p.1).sum();
        assert!((horiz - 1.0).abs() < 1e-12);
        assert!(s.len() > 50);
    }

    #[test]
    fn the_pv_example() {
        assert!((pv_energy(1200.0, 100.0, &PvSettings::default()) - 14_860.8).abs() < 0.5);
    }

    #[test]
    fn south_beats_north_and_shade_cuts() {
        let mut i = input(52.0);
        // Two 30° roof faces, 4 m by 4 m: one tilted to the south, one to the north.
        let tilt = |south: bool, x0: f64| {
            let (h, run) = (4.0 * 30f64.to_radians().sin(), 4.0 * 30f64.to_radians().cos());
            if south {
                vec![[x0, 0.0, h], [x0, -run, 0.0], [x0 + 4.0, -run, 0.0], [x0 + 4.0, 0.0, h]]
            } else {
                vec![[x0, 0.0, h], [x0 + 4.0, 0.0, h], [x0 + 4.0, run, 0.0], [x0, run, 0.0]]
            }
        };
        i.faces = vec![Face { key: "south".into(), outline: tilt(true, 0.0) }, Face { key: "north".into(), outline: tilt(false, 10.0) }];
        let r = run(&i).unwrap();
        assert!(r.faces[0].mean > r.faces[1].mean * 1.3, "{} against {}", r.faces[0].mean, r.faces[1].mean);
        // A tall slab just south of the south face shades it.
        let (x0, x1, y, z) = (-5.0, 9.0, -5.0, 30.0);
        i.shading = vec![[[x0, y, 0.0], [x1, y, 0.0], [x1, y, z]], [[x0, y, 0.0], [x1, y, z], [x0, y, z]]];
        let shaded = run(&i).unwrap();
        assert!(shaded.faces[0].mean < r.faces[0].mean * 0.5, "{} against {}", shaded.faces[0].mean, r.faces[0].mean);
    }
}
