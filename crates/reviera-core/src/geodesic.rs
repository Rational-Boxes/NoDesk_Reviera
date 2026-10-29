// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Geodesic domes (spec Ch 3 §3.6a.6, ARC-GLZ-006): a glazed dome of triangular panels on
//! struts, from an icosahedron whose faces are each divided `frequency` times along every edge
//! (class I) and pushed out onto the sphere.
//!
//! The icosahedron stands on a vertex at its top, so the hubs lie in level rings; the dome is
//! cut at the ring nearest the chosen `truncation` (the share of the sphere's diameter it
//! keeps: 1/2 a hemisphere, 5/8 taller), every panel wholly above it kept. Where that ring is
//! level (even frequencies at 1/2, for instance) the dome stands on a level base; elsewhere its
//! base steps between rings, which the report says. The base ring sits on the level at
//! `base_offset`.
//!
//! The dome is an `IfcRoof` (DOME_ROOF) whose body is its panels and struts; its glazing and
//! frame are library entries (`glazing.list_types`, `frame.list_types`) whose values it carries
//! (`Reviera_GlazingUnit`, `Reviera_FrameSystem`). `roof.geodesic` reports what fabrication
//! needs: the hubs, the panels, and the struts by length class with their counts.
//!
//! Not yet: the struts and panels as their own `IfcMember` and `IfcPlate` in IFC (the dome's
//! body is one shape there), hub connectors, vents and doors in it, and riser walls under a
//! stepped base.

use std::collections::{BTreeMap, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const RECIPE: &str = "recipe.geodesic_dome";
pub const CLASS: &str = "IfcRoof";
/// The dome's glazing and frame when none is chosen (provisional, EXPERT_REVIEW E-53).
pub const GLAZING: &str = "Toughened 4 mm";
pub const FRAME: &str = "Aluminium Glazing Bar 50×100";

/// The stored recipe of a geodesic dome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DomeRecipe {
    pub kind: String,
    /// The level (GlobalId).
    pub level: String,
    pub center: [f64; 2],
    #[serde(default)]
    pub base_offset: f64,
    #[serde(default)]
    pub rotation: f64,
    /// The sphere's radius, metres.
    pub radius: f64,
    /// Divisions of each icosahedron edge (2V, 3V, …).
    pub frequency: u32,
    /// The share of the sphere's diameter kept (0.5 a hemisphere).
    pub truncation: f64,
    /// Library entries; the defaults when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glazing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_system: Option<String>,
}

pub fn recipe(e: &Element) -> Option<DomeRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    scale(a, 1.0 / len(a).max(1e-12))
}

/// The unit sphere's subdivided icosahedron: hubs, and triangles outward-facing.
fn sphere(frequency: u32) -> (Vec<V3>, Vec<[usize; 3]>) {
    let (h, r) = (1.0 / 5f64.sqrt(), 2.0 / 5f64.sqrt());
    let ring = |z: f64, a0: f64| -> Vec<V3> {
        (0..5)
            .map(|k| {
                let a = a0 + std::f64::consts::TAU * k as f64 / 5.0;
                [r * a.cos(), r * a.sin(), z]
            })
            .collect()
    };
    let (top, bottom) = ([0.0, 0.0, 1.0], [0.0, 0.0, -1.0]);
    let (up, down) = (ring(h, 0.0), ring(-h, std::f64::consts::PI / 5.0));
    let mut faces: Vec<[V3; 3]> = Vec::new();
    for k in 0..5 {
        let n = (k + 1) % 5;
        faces.push([top, up[k], up[n]]);
        faces.push([up[k], down[k], up[n]]);
        faces.push([up[n], down[k], down[n]]);
        faces.push([bottom, down[n], down[k]]);
    }
    let n = frequency.max(1) as usize;
    let mut hubs: Vec<V3> = Vec::new();
    let mut index: HashMap<[i64; 3], usize> = HashMap::new();
    let mut hub = |p: V3| -> usize {
        let p = unit(p);
        let key = [(p[0] * 1e9).round() as i64, (p[1] * 1e9).round() as i64, (p[2] * 1e9).round() as i64];
        *index.entry(key).or_insert_with(|| {
            hubs.push(p);
            hubs.len() - 1
        })
    };
    let mut tris = Vec::new();
    for [a, b, c] in faces {
        let at = |i: usize, j: usize| add(a, add(scale(sub(b, a), i as f64 / n as f64), scale(sub(c, a), j as f64 / n as f64)));
        for i in 0..n {
            for j in 0..n - i {
                tris.push([hub(at(i, j)), hub(at(i + 1, j)), hub(at(i, j + 1))]);
                if i + j + 1 < n {
                    tris.push([hub(at(i + 1, j)), hub(at(i + 1, j + 1)), hub(at(i, j + 1))]);
                }
            }
        }
    }
    // Outward: counter-clockwise seen from outside.
    for t in &mut tris {
        let (p, q, s) = (hubs[t[0]], hubs[t[1]], hubs[t[2]]);
        if dot(cross(sub(q, p), sub(s, p)), add(p, add(q, s))) < 0.0 {
            t.swap(1, 2);
        }
    }
    (hubs, tris)
}

/// A length class of struts.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct StrutClass {
    /// A, B, C … from the shortest.
    pub label: String,
    /// Hub centre to hub centre, metres.
    pub length: f64,
    pub count: usize,
}

/// A dome as it is now, in the project's coordinates.
#[derive(Debug, Clone)]
pub struct DomeGeometry {
    pub hubs: Vec<V3>,
    pub panels: Vec<[usize; 3]>,
    pub struts: Vec<[usize; 2]>,
    /// The sphere's centre.
    pub centre: V3,
    /// The base ring's height and plan radius; whether every base hub lies on it.
    pub base_z: f64,
    pub base_radius: f64,
    pub level_base: bool,
    pub height: f64,
}

/// The dome's hubs, panels and struts (`level_z` the level's elevation).
pub fn geometry(r: &DomeRecipe, level_z: f64) -> DomeGeometry {
    let (unit_hubs, tris) = sphere(r.frequency);
    // The ring nearest the cut kept, every panel wholly above it.
    let want = 1.0 - 2.0 * r.truncation.clamp(0.05, 1.0);
    let mut rings: Vec<f64> = unit_hubs.iter().map(|p| p[2]).collect();
    rings.sort_by(f64::total_cmp);
    rings.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    let cut = rings.iter().copied().min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs())).unwrap_or(0.0);
    let kept: Vec<[usize; 3]> = tris.into_iter().filter(|t| t.iter().all(|i| unit_hubs[*i][2] >= cut - 1e-7)).collect();
    // Renumber the hubs in use.
    let mut map: BTreeMap<usize, usize> = BTreeMap::new();
    for t in &kept {
        for i in t {
            let n = map.len();
            map.entry(*i).or_insert(n);
        }
    }
    let mut order: Vec<(usize, usize)> = map.iter().map(|(a, b)| (*b, *a)).collect();
    order.sort_unstable();
    let panels: Vec<[usize; 3]> = kept.iter().map(|t| [map[&t[0]], map[&t[1]], map[&t[2]]]).collect();
    // The lowest kept hub stands on the base.
    let low = order.iter().map(|(_, i)| unit_hubs[*i][2]).fold(f64::MAX, f64::min);
    let base_z = level_z + r.base_offset;
    let centre = [r.center[0], r.center[1], base_z - low * r.radius];
    let (s, c) = r.rotation.sin_cos();
    let hubs: Vec<V3> = order
        .iter()
        .map(|(_, i)| {
            let p = unit_hubs[*i];
            [centre[0] + r.radius * (c * p[0] - s * p[1]), centre[1] + r.radius * (s * p[0] + c * p[1]), centre[2] + r.radius * p[2]]
        })
        .collect();
    let mut edges: BTreeMap<[usize; 2], usize> = BTreeMap::new();
    for t in &panels {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *edges.entry([a.min(b), a.max(b)]).or_default() += 1;
        }
    }
    // The base: hubs on edges only one panel has.
    let base: Vec<usize> = edges.iter().filter(|(_, n)| **n == 1).flat_map(|(e, _)| *e).collect();
    let level_base = base.iter().all(|i| (hubs[*i][2] - base_z).abs() < 1e-6);
    let base_radius = base.iter().map(|i| (hubs[*i][0] - centre[0]).hypot(hubs[*i][1] - centre[1])).fold(0.0, f64::max);
    let height = hubs.iter().map(|p| p[2]).fold(f64::MIN, f64::max) - base_z;
    DomeGeometry { struts: edges.into_keys().collect(), hubs, panels, centre, base_z, base_radius, level_base, height }
}

impl DomeGeometry {
    pub fn strut_length(&self, s: [usize; 2]) -> f64 {
        len(sub(self.hubs[s[1]], self.hubs[s[0]]))
    }

    /// The struts grouped by length (to 0.1 mm), shortest first.
    pub fn strut_classes(&self) -> Vec<StrutClass> {
        let mut lengths: Vec<f64> = self.struts.iter().map(|s| self.strut_length(*s)).collect();
        lengths.sort_by(f64::total_cmp);
        let mut out: Vec<StrutClass> = Vec::new();
        for l in lengths {
            match out.last_mut() {
                Some(c) if (l - c.length).abs() < 1e-4 => c.count += 1,
                _ => out.push(StrutClass { label: String::new(), length: l, count: 1 }),
            }
        }
        for (k, c) in out.iter_mut().enumerate() {
            c.label = label(k);
            c.length = (c.length * 1e4).round() / 1e4;
        }
        out
    }

    /// The panels' glazed area (their triangles).
    pub fn glass_area(&self) -> f64 {
        self.panels.iter().map(|t| len(cross(sub(self.hubs[t[1]], self.hubs[t[0]]), sub(self.hubs[t[2]], self.hubs[t[0]]))) / 2.0).sum()
    }

    /// The bodies: a panel per triangle, inset by half a strut's width from its edges and
    /// `thickness` deep under the face, and a strut per edge, `bar` (width across the face,
    /// depth inward toward the centre).
    pub fn meshes(&self, thickness: f64, bar: crate::curtain::Mullion) -> (Vec<Mesh>, Vec<Mesh>) {
        let prism = |outer: [V3; 3], down: V3| -> Mesh {
            let mut pts: Vec<V3> = outer.to_vec();
            pts.extend(outer.iter().map(|p| add(*p, down)));
            let idx = vec![0, 1, 2, 5, 4, 3, 0, 3, 4, 0, 4, 1, 1, 4, 5, 1, 5, 2, 2, 5, 3, 2, 3, 0];
            (pts, idx)
        };
        let panels = self
            .panels
            .iter()
            .filter_map(|t| {
                let [a, b, c] = [self.hubs[t[0]], self.hubs[t[1]], self.hubs[t[2]]];
                let (la, lb, lc) = (len(sub(b, c)), len(sub(c, a)), len(sub(a, b)));
                let p = la + lb + lc;
                let incentre = scale(add(add(scale(a, la), scale(b, lb)), scale(c, lc)), 1.0 / p);
                let area = len(cross(sub(b, a), sub(c, a))) / 2.0;
                let inradius = 2.0 * area / p;
                let k = (inradius - bar.width / 2.0) / inradius;
                if k <= 0.05 {
                    return None;
                }
                let shrink = |q: V3| add(incentre, scale(sub(q, incentre), k));
                let n = unit(cross(sub(b, a), sub(c, a)));
                Some(prism([shrink(a), shrink(b), shrink(c)], scale(n, -thickness)))
            })
            .collect();
        let struts = self
            .struts
            .iter()
            .map(|s| {
                let (a, b) = (self.hubs[s[0]], self.hubs[s[1]]);
                let radial = unit(sub(scale(add(a, b), 0.5), self.centre));
                let across = scale(unit(cross(sub(b, a), radial)), bar.width / 2.0);
                let down = scale(radial, -bar.depth);
                let ring = [sub(a, across), sub(b, across), add(b, across), add(a, across)];
                let mut pts: Vec<V3> = ring.iter().map(|p| add(*p, down)).collect();
                pts.extend(ring);
                let quads: [[u32; 4]; 6] = [[3, 2, 1, 0], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];
                (pts, quads.iter().flat_map(|q| [q[0], q[1], q[2], q[0], q[2], q[3]]).collect())
            })
            .collect();
        (panels, struts)
    }
}

fn label(k: usize) -> String {
    let letters = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    if k < 26 { (letters[k] as char).to_string() } else { format!("{}{}", label(k / 26 - 1), letters[k % 26] as char) }
}

fn glazing_of(r: &DomeRecipe) -> crate::glazing_library::Entry {
    r.glazing.as_deref().and_then(crate::glazing_library::entry).or_else(|| crate::glazing_library::entry(GLAZING)).expect("in the library")
}

fn frame_of(r: &DomeRecipe) -> crate::frame_library::Entry {
    r.frame_system.as_deref().and_then(crate::frame_library::entry).or_else(|| crate::frame_library::entry(FRAME)).expect("in the library")
}

/// Every dome's panels and struts.
pub fn all(store: &Store) -> Vec<(ElementId, Vec<Mesh>, Vec<Mesh>)> {
    store
        .iter()
        .filter_map(|e| {
            let r = recipe(e)?;
            let z = crate::levels::elevation_of(store, &r.level).unwrap_or(0.0);
            let (panels, struts) = geometry(&r, z).meshes(glazing_of(&r).glazing.thickness.max(0.004), frame_of(&r).frame.bar());
            Some((e.id(), panels, struts))
        })
        .collect()
}

fn check(r: &DomeRecipe) -> Result<()> {
    let bad = |m: &str| Err(Error::new("geodesic.bad_value", m.to_owned()));
    if !(0.5..=100.0).contains(&r.radius) {
        return bad("a geodesic dome's radius is 0.5 to 100 m");
    }
    if !(1..=12).contains(&r.frequency) {
        return bad("the frequency is 1 to 12 (1V to 12V)");
    }
    if !(0.2..=0.95).contains(&r.truncation) {
        return bad("the truncation keeps 0.2 to 0.95 of the sphere's diameter");
    }
    Ok(())
}

/// The dome's glazing and frame values on the element.
fn write(txn: &mut Transaction<'_>, id: ElementId, r: &DomeRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    crate::glazing_library::write(txn, id, &glazing_of(r).glazing)?;
    crate::frame_library::write(txn, id, &frame_of(r).frame)
}

// ---- Commands ----

fn number(store: &Store, v: &Value, data_type: &str, what: &str) -> Result<f64> {
    match v {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => crate::units::parse(s, data_type, &crate::units::settings_of(store)),
        _ => Err(Error::new("geodesic.invalid_value", format!("{what} must be a number or text with units"))),
    }
}

/// A truncation: a share (0.625) or a fraction ("5/8").
fn fraction(v: &Value) -> Result<f64> {
    let bad = || Error::new("geodesic.invalid_value", "the truncation is a share (0.625) or a fraction (\"5/8\")");
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(bad),
        Value::String(s) => match s.split_once('/') {
            Some((a, b)) => Ok(a.trim().parse::<f64>().map_err(|_| bad())? / b.trim().parse::<f64>().map_err(|_| bad())?),
            None => s.trim().parse().map_err(|_| bad()),
        },
        _ => Err(bad()),
    }
}

/// The parameters of `roof.create_geodesic` and `roof.set_geodesic`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Params {
    pub center: Option<[f64; 2]>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub radius: Option<Value>,
    /// 2 for 2V, 3 for 3V, …
    pub frequency: Option<u32>,
    /// The share of the sphere's diameter kept: 0.5 (a hemisphere), "3/8", "5/8" …
    pub truncation: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<Value>,
    /// Library names (`glazing.list_types`, `frame.list_types`); "" for the default.
    pub glazing: Option<String>,
    pub frame_system: Option<String>,
}

fn apply(store: &Store, r: &mut DomeRecipe, p: &Params) -> Result<()> {
    if let Some(c) = p.center {
        r.center = c;
    }
    if let Some(v) = &p.radius {
        r.radius = number(store, v, "length", "the radius")?;
    }
    if let Some(f) = p.frequency {
        r.frequency = f;
    }
    if let Some(v) = &p.truncation {
        r.truncation = fraction(v)?;
    }
    if let Some(v) = &p.base_offset {
        r.base_offset = number(store, v, "length", "the base offset")?;
    }
    if let Some(v) = &p.rotation {
        r.rotation = number(store, v, "angle", "the rotation")?;
    }
    if let Some(g) = &p.glazing {
        if !g.is_empty() && crate::glazing_library::entry(g).is_none() {
            return Err(Error::new("glazing.unknown", format!("no glazing {g} in the library (glazing.list_types)")));
        }
        r.glazing = (!g.is_empty()).then(|| g.clone());
    }
    if let Some(f) = &p.frame_system {
        if !f.is_empty() && crate::frame_library::entry(f).is_none() {
            return Err(Error::new("frame.unknown", format!("no frame system {f} in the library (frame.list_types)")));
        }
        r.frame_system = (!f.is_empty()).then(|| f.clone());
    }
    check(r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub level_id: ElementRef,
    #[serde(flatten)]
    pub params: Params,
}

/// `roof.create_geodesic`: a 3V dome of 5 m radius at 5/8 by default (provisional, E-53).
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let level = store
        .resolve(&p.level_id)
        .filter(|l| store.get(*l).is_some_and(|e| e.class == "IfcBuildingStorey"))
        .ok_or_else(|| Error::new("geodesic.no_level", "no such level"))?;
    let mut r = DomeRecipe {
        kind: RECIPE.into(),
        level: store.get(level).expect("resolved").guid().to_string(),
        center: [0.0, 0.0],
        base_offset: 0.0,
        rotation: 0.0,
        radius: 5.0,
        frequency: 3,
        truncation: 0.625,
        glazing: None,
        frame_system: None,
    };
    apply(&store, &mut r, &p.params)?;
    let id = txn.create(NewElement::new(CLASS).guid(GlobalId::new()).name(format!("Geodesic Dome {}V", r.frequency)).param("@PredefinedType", "DOME_ROOF"))?;
    write(txn, id, &r)?;
    Ok(id)
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, DomeRecipe)> {
    let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", "no such dome"))?;
    let rec = recipe(store.get(id).expect("resolved")).ok_or_else(|| Error::new("geodesic.not_a_dome", format!("element {id} is not a geodesic dome")))?;
    Ok((id, rec))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub roof_id: ElementRef,
    #[serde(flatten)]
    pub params: Params,
}

/// `roof.set_geodesic`.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.roof_id)?;
    crate::modify::check_unpinned(&store, id)?;
    apply(&store, &mut r, &p.params)?;
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {
    pub roof_id: ElementRef,
}

/// What `roof.geodesic` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Info {
    pub recipe: DomeRecipe,
    /// Its height above the base, the base ring's plan radius, and whether the base is level.
    pub height: f64,
    pub base_radius: f64,
    pub level_base: bool,
    pub hubs: usize,
    pub panels: usize,
    pub struts: usize,
    pub strut_classes: Vec<StrutClass>,
    pub glass_area: f64,
    /// The floor inside the base ring (its polygon seen from above, as a circle when stepped).
    pub floor_area: f64,
}

/// `roof.geodesic`: the dome's fabrication report.
pub(crate) fn info(store: &Store, p: InfoParams) -> Result<Info> {
    let (_, r) = resolve(store, &p.roof_id)?;
    let z = crate::levels::elevation_of(store, &r.level).unwrap_or(0.0);
    let g = geometry(&r, z);
    Ok(Info {
        height: g.height,
        base_radius: g.base_radius,
        level_base: g.level_base,
        hubs: g.hubs.len(),
        panels: g.panels.len(),
        struts: g.struts.len(),
        strut_classes: g.strut_classes(),
        glass_area: g.glass_area(),
        floor_area: std::f64::consts::PI * g.base_radius * g.base_radius,
        recipe: r,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dome(frequency: u32, truncation: f64) -> DomeGeometry {
        let r = DomeRecipe { kind: RECIPE.into(), level: String::new(), center: [0.0, 0.0], base_offset: 0.0, rotation: 0.0, radius: 5.0, frequency, truncation, glazing: None, frame_system: None };
        geometry(&r, 0.0)
    }

    #[test]
    fn a_whole_sphere_has_the_icosahedral_counts() {
        // 10 f² + 2 hubs, 20 f² panels, 30 f² struts.
        for f in 1..=4u32 {
            let (hubs, tris) = sphere(f);
            assert_eq!((hubs.len(), tris.len()), (10 * (f * f) as usize + 2, 20 * (f * f) as usize), "{f}V");
        }
    }

    #[test]
    fn a_2v_hemisphere_and_a_3v_five_eighths() {
        // 2V at 1/2: the well-known 26 hubs, 40 panels and 65 struts of two lengths, on a level
        // base at the equator.
        let g = dome(2, 0.5);
        assert_eq!((g.hubs.len(), g.panels.len(), g.struts.len()), (26, 40, 65));
        let classes = g.strut_classes();
        assert_eq!(classes.iter().map(|c| c.count).collect::<Vec<_>>(), vec![30, 35]);
        // 0.54653 and 0.61803 × the radius.
        assert!((classes[0].length - 5.0 * 0.546_533).abs() < 1e-3 && (classes[1].length - 5.0 * 0.618_034).abs() < 1e-3, "{classes:?}");
        assert!(g.level_base && (g.height - 5.0).abs() < 1e-9);
        // 3V at 5/8: three strut lengths, taller than a hemisphere; every strut has two panels
        // but the base ring's one, so the panels close without gaps.
        let g = dome(3, 0.625);
        let classes = g.strut_classes();
        assert_eq!(classes.len(), 3, "{classes:?}");
        assert!(g.height > 5.0 && g.height < 7.5, "{}", g.height);
        let mut uses: BTreeMap<[usize; 2], usize> = BTreeMap::new();
        for t in &g.panels {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *uses.entry([a.min(b), a.max(b)]).or_default() += 1;
            }
        }
        assert!(uses.values().all(|n| *n == 1 || *n == 2));
        let open: Vec<_> = uses.iter().filter(|(_, n)| **n == 1).collect();
        assert!(!open.is_empty() && open.iter().all(|(e, _)| e.iter().all(|i| g.hubs[*i][2] < 2.0)), "only the base is open");
        assert_eq!(fraction(&json!("5/8")).unwrap(), 0.625);
    }
}
