// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Average estimated illumination (ANA-LIGHT-001): the lumen (zonal cavity) method for each MEP
//! space, a built-in live calculation.
//!
//! - **E = N × Φ × CU × LLF ÷ A** summed over the lighting fixtures whose room calculation
//!   point lies in the space (A its floor area).
//! - **Φ**: the rated lumens of the fixture's type: from its photometric file, else the type's
//!   own lumens (`rxd:lighting`).
//! - **RCR = 2.5 × h_rc × perimeter ÷ area** (5 × h_rc × (L + W) ÷ (L × W) for a rectangle),
//!   h_rc from the workplane (the space's *Lighting Calculation Workplane* above its base) to the
//!   fixture plane (the counted fixtures' average mounting height); zero when the fixtures are
//!   at or below the workplane.
//! - **CU** is interpolated in the type's CU table (rows by RCR, columns by ceiling, wall and
//!   floor reflectance) at the space's RCR and reflectances: linearly in RCR, then bilinearly in
//!   ceiling and wall reflectance among the columns for the floor reflectance nearest the
//!   space's (no floor correction), the nearest column when the grid is incomplete, clamped at
//!   the table's edges. Reviera ships no CU tables; a fixture whose type has none is excluded
//!   with the reason "no CU data", one with no lumens with "no lumen data".
//! - **LLF**: the space's override, else the type's, else 0.8.
//! - Worked out from the model whenever it is read (`lighting.calc_average`, `space.info`,
//!   schedules), so moving a fixture changes the two spaces concerned and no other.
//! - The target is `Pset_SpaceLightingRequirements.Illuminance` on the space, an ordinary IFC
//!   property; *Meets target* is whether the average reaches it.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mesh_objects::{self, Category};
use crate::store::Store;
use crate::txn::Transaction;

/// On a lighting fixture type: its lumens, light loss factor and CU table.
pub const LIGHTING_KEY: &str = "rxd:lighting";
/// On a space: the target illuminance (lx).
pub const TARGET_KEY: &str = "Pset_SpaceLightingRequirements.Illuminance";
pub const DEFAULT_LLF: f64 = 0.8;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A coefficient of utilisation table: `values[row][column]` at `rcr[row]` and the
/// reflectances of `columns[column]` (ceiling, wall, floor; 0–1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CuTable {
    pub rcr: Vec<f64>,
    pub columns: Vec<[f64; 3]>,
    pub values: Vec<Vec<f64>>,
}

/// A lighting fixture type's data for the lumen method.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LightingData {
    /// Rated lumens per fixture, used when the type has no photometric file that states them.
    pub lumens: Option<f64>,
    pub light_loss_factor: Option<f64>,
    pub cu_table: Option<CuTable>,
}

pub fn lighting_data(t: &Element) -> LightingData {
    t.param(LIGHTING_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// A space's target illuminance (lx), from a plain number or a typed IFC value.
pub fn target(e: &Element) -> Option<f64> {
    let v = e.param(TARGET_KEY)?;
    v.as_f64().or_else(|| v.get("value")?.as_f64())
}

impl CuTable {
    fn check(&self) -> Result<()> {
        if self.rcr.len() < 2 || self.columns.is_empty() {
            return Err(bad("lighting.cu_table", "a CU table needs at least two RCR rows and one column"));
        }
        if self.rcr.windows(2).any(|w| w[1].is_nan() || w[1] <= w[0]) || self.rcr.iter().any(|r| !(r.is_finite() && *r >= 0.0)) {
            return Err(bad("lighting.cu_table", "the RCR rows must rise from 0 or more"));
        }
        if self.values.len() != self.rcr.len() || self.values.iter().any(|r| r.len() != self.columns.len()) {
            return Err(bad("lighting.cu_table", format!("the table needs {} rows of {} values", self.rcr.len(), self.columns.len())));
        }
        if self.columns.iter().flatten().any(|r| !(0.0..=1.0).contains(r)) {
            return Err(bad("lighting.cu_table", "column reflectances run from 0 to 1"));
        }
        if self.values.iter().flatten().any(|v| !(v.is_finite() && *v >= 0.0 && *v <= 2.0)) {
            return Err(bad("lighting.cu_table", "coefficients of utilisation run from 0 to 2"));
        }
        Ok(())
    }

    /// The column's value at `rcr`, linear between rows, clamped at the ends.
    fn at_rcr(&self, col: usize, rcr: f64) -> f64 {
        let n = self.rcr.len();
        if rcr <= self.rcr[0] {
            return self.values[0][col];
        }
        if rcr >= self.rcr[n - 1] {
            return self.values[n - 1][col];
        }
        let k = self.rcr.windows(2).position(|w| rcr <= w[1]).expect("inside");
        let t = (rcr - self.rcr[k]) / (self.rcr[k + 1] - self.rcr[k]);
        self.values[k][col] * (1.0 - t) + self.values[k + 1][col] * t
    }

    /// The CU at `rcr` for ceiling, wall and floor reflectances.
    pub fn cu(&self, rcr: f64, refl: [f64; 3]) -> f64 {
        let floor = self.columns.iter().map(|c| c[2]).min_by(|a, b| (a - refl[2]).abs().total_cmp(&(b - refl[2]).abs())).expect("columns");
        let cols: Vec<usize> = (0..self.columns.len()).filter(|i| (self.columns[*i][2] - floor).abs() < 1e-9).collect();
        let find = |c: f64, w: f64| cols.iter().copied().find(|i| (self.columns[*i][0] - c).abs() < 1e-9 && (self.columns[*i][1] - w).abs() < 1e-9);
        let axis = |k: usize, v: f64| -> (f64, f64, f64) {
            let mut xs: Vec<f64> = cols.iter().map(|i| self.columns[*i][k]).collect();
            xs.sort_by(f64::total_cmp);
            xs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            let v = v.clamp(xs[0], xs[xs.len() - 1]);
            let hi = xs.iter().copied().find(|x| *x >= v - 1e-12).unwrap_or(xs[xs.len() - 1]);
            let lo = xs.iter().copied().rev().find(|x| *x <= v + 1e-12).unwrap_or(xs[0]);
            let t = if hi - lo > 1e-12 { (v - lo) / (hi - lo) } else { 0.0 };
            (lo, hi, t)
        };
        let (c0, c1, tc) = axis(0, refl[0]);
        let (w0, w1, tw) = axis(1, refl[1]);
        if let (Some(a), Some(b), Some(c), Some(d)) = (find(c0, w0), find(c0, w1), find(c1, w0), find(c1, w1)) {
            let v = |i| self.at_rcr(i, rcr);
            let lo = v(a) * (1.0 - tw) + v(b) * tw;
            let hi = v(c) * (1.0 - tw) + v(d) * tw;
            return lo * (1.0 - tc) + hi * tc;
        }
        let near = cols
            .iter()
            .copied()
            .min_by(|i, j| {
                let d = |k: usize| (self.columns[k][0] - refl[0]).powi(2) + (self.columns[k][1] - refl[1]).powi(2);
                d(*i).total_cmp(&d(*j))
            })
            .expect("columns");
        self.at_rcr(near, rcr)
    }
}

/// A fixture left out of a space's average, and why.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Excluded {
    pub fixture: ElementId,
    pub reason: String,
}

/// A space's average estimated illumination.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Illumination {
    pub space: ElementId,
    pub number: String,
    pub name: String,
    /// lx; none when the space was skipped.
    pub illuminance: Option<f64>,
    pub rcr: f64,
    /// Fixtures counted.
    pub fixtures: usize,
    pub excluded: Vec<Excluded>,
    pub target: Option<f64>,
    pub meets_target: Option<bool>,
    /// Why the space was skipped (not placed, zero area).
    pub skipped: Option<String>,
}

/// The space's average now.
pub fn illumination(store: &Store, space: ElementId) -> Option<Illumination> {
    let e = store.get(space)?;
    let r = crate::mep_spaces::space_recipe(e)?;
    let pr = crate::mep_spaces::props(e);
    let (number, name) = crate::rooms::number_and_name(e);
    let tgt = target(e);
    let mut out = Illumination { space, number, name, illuminance: None, rcr: 0.0, fixtures: 0, excluded: Vec::new(), target: tgt, meets_target: None, skipped: None };
    let g = crate::mep_spaces::geometry(store, e);
    let Some((g, region)) = g.as_ref().and_then(|g| Some((g, g.region.as_ref()?))) else {
        out.skipped = Some(if pr.unplaced { "not placed".into() } else { "not enclosed".into() });
        return Some(out);
    };
    let area = region.area();
    if area <= 1e-9 || g.z_top <= g.z_base {
        out.skipped = Some("zero area or no upper limit".into());
        return Some(out);
    }
    let level = GlobalId::parse(&r.level).and_then(|g| store.find_by_guid(&g));
    // (lumens, LLF, mounting height, CU table) of each fixture in the space.
    let mut counted: Vec<(f64, f64, f64, CuTable)> = Vec::new();
    for c in store.iter() {
        let Some(cr) = mesh_objects::mesh_object_recipe(c) else { continue };
        let Some((tid, t)) = mesh_objects::type_of(store, c) else { continue };
        if t.category != Category::LightingFixture || GlobalId::parse(&cr.level).and_then(|g| store.find_by_guid(&g)) != level {
            continue;
        }
        let Some(p) = crate::mep_lighting::calc_point(store, c) else { continue };
        if !region.contains([p[0], p[1]]) {
            continue;
        }
        let data = store.get(tid).map(lighting_data).unwrap_or_default();
        let lumens = t.photometry.as_ref().and_then(|ph| ph.lumens).or(data.lumens);
        let Some(lumens) = lumens.filter(|l| *l > 0.0) else {
            out.excluded.push(Excluded { fixture: c.id(), reason: "no lumen data".into() });
            continue;
        };
        let Some(cu) = data.cu_table else {
            out.excluded.push(Excluded { fixture: c.id(), reason: "no CU data".into() });
            continue;
        };
        let z = mesh_objects::regenerate(store, c).map(|m| m.placement.apply([0.0; 3])[2]).unwrap_or(p[2]);
        let llf = pr.light_loss_factor.or(data.light_loss_factor).unwrap_or(DEFAULT_LLF);
        counted.push((lumens, llf, z, cu));
    }
    out.fixtures = counted.len();
    if !counted.is_empty() {
        let plane = counted.iter().map(|c| c.2).sum::<f64>() / counted.len() as f64;
        let h_rc = (plane - (g.z_base + pr.workplane_height)).max(0.0);
        out.rcr = 2.5 * h_rc * region.perimeter() / area;
    }
    let refl = [pr.ceiling_reflectance, pr.wall_reflectance, pr.floor_reflectance];
    let flux: f64 = counted.iter().map(|(l, llf, _, cu)| l * cu.cu(out.rcr, refl) * llf).sum();
    let lx = flux / area;
    out.illuminance = Some(lx);
    out.meets_target = tgt.map(|t| lx + 1e-9 >= t);
    Some(out)
}

/// Every placed or unplaced space's result, by id (for schedules).
pub fn by_id(store: &Store) -> BTreeMap<ElementId, Illumination> {
    crate::mep_spaces::all(store).into_iter().filter_map(|(id, _, _)| Some((id, illumination(store, id)?))).collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalcParams {
    /// Spaces; every space when empty.
    #[serde(default)]
    pub space_ids: Vec<ElementRef>,
}

/// `lighting.calc_average`.
pub(crate) fn calc_average(store: &Store, p: CalcParams) -> Result<Vec<Illumination>> {
    if p.space_ids.is_empty() {
        return Ok(by_id(store).into_values().collect());
    }
    p.space_ids
        .iter()
        .map(|r| {
            let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such space"))?;
            illumination(store, id).ok_or_else(|| bad("space.not_a_space", format!("element {id} is not a space")))
        })
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLightingParams {
    /// A lighting fixture type.
    pub type_id: ElementRef,
    /// Rated lumens per fixture (used when no photometric file states them); null clears.
    #[serde(default, deserialize_with = "some")]
    pub lumens: Option<Option<f64>>,
    /// 0–1; null clears (0.8 is used).
    #[serde(default, deserialize_with = "some")]
    pub light_loss_factor: Option<Option<f64>>,
    /// The CU table; null clears.
    #[serde(default, deserialize_with = "some_table")]
    pub cu_table: Option<Option<CuTable>>,
}

fn some<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Option<f64>>, D::Error> {
    Option::<f64>::deserialize(d).map(Some)
}

fn some_table<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Option<CuTable>>, D::Error> {
    Option::<CuTable>::deserialize(d).map(Some)
}

/// `lighting_fixture_type.set_lighting`.
pub(crate) fn set_lighting(txn: &mut Transaction<'_>, p: SetLightingParams) -> Result<LightingData> {
    let store = txn.store().clone();
    let id = store.resolve(&p.type_id).ok_or_else(|| bad("element.not_found", "no such type"))?;
    let e = store.get(id).expect("resolved");
    let t = mesh_objects::mesh_object_type(e).ok_or_else(|| bad("mesh_object.not_a_type", format!("element {id} is not a component type")))?;
    if t.category != Category::LightingFixture {
        return Err(bad("lighting_fixture.category", "lumen method data belongs to lighting fixture types"));
    }
    let mut d = lighting_data(e);
    if let Some(l) = p.lumens {
        if l.is_some_and(|l| !(l.is_finite() && l > 0.0)) {
            return Err(bad("lighting.lumens", "the lumens must be above zero"));
        }
        d.lumens = l;
    }
    if let Some(f) = p.light_loss_factor {
        if f.is_some_and(|f| !(f > 0.0 && f <= 1.0)) {
            return Err(bad("lighting.llf", "the light loss factor runs from above 0 to 1"));
        }
        d.light_loss_factor = f;
    }
    if let Some(c) = p.cu_table {
        if let Some(c) = &c {
            c.check()?;
        }
        d.cu_table = c;
    }
    let v = serde_json::to_value(&d).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(LIGHTING_KEY.into(), v);
    })?;
    Ok(d)
}

/// The typed value written for a target illuminance.
pub(crate) fn target_value(lx: f64) -> Value {
    json!({ "type": "IfcIlluminanceMeasure", "value": lx })
}
