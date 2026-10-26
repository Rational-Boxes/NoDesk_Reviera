// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The simplified energy model (ANA-EM-003, ANA-ENV-003): analytical spaces and their
//! heat-transfer surfaces, derived from the MEP spaces whenever it is asked for (so it is never
//! out of date).
//!
//! - **Analytical spaces**: every placed, enclosed MEP space (MEP-SPACE-001), with its floor area,
//!   volume, height and system-zone.
//! - **Walls**: each edge of a space's boundary that a wall face makes is a wall surface of the
//!   edge's length by the space's height. It is **interior** when another space lies just beyond
//!   the wall (that space's matching surface is its corresponding boundary), else **exterior**.
//!   Windows in the wall whose width overlaps the edge become **window** surfaces (their area
//!   taken off the wall), doors **door** surfaces; on interior walls they stay part of the wall.
//!   Edges made by separation lines are **air** boundaries.
//! - **Floors**: a space's floor is **interior** when a space on a lower level covers its centre,
//!   else **slab on grade**. **Ceilings**: **interior** when a space on a higher level covers it,
//!   else **roof**.
//! - **U-values** (W/m²K): the element type's `ThermalTransmittance` in its common property set
//!   (`Pset_WallCommon`, `Pset_WindowCommon`, `Pset_DoorCommon`, `Pset_SlabCommon`,
//!   `Pset_RoofCommon`), set with `energy_model.set_thermal`; else the defaults by surface class
//!   (EXPERT_REVIEW E-37). Windows also carry a solar heat gain coefficient
//!   (`Pset_DoorWindowGlazingType.SolarHeatGainTransmittance`).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A surface's class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceClass {
    ExteriorWall,
    InteriorWall,
    Roof,
    InteriorCeiling,
    SlabOnGrade,
    InteriorFloor,
    Window,
    Door,
    Air,
}

impl SurfaceClass {
    pub fn exterior(self) -> bool {
        matches!(self, SurfaceClass::ExteriorWall | SurfaceClass::Roof | SurfaceClass::SlabOnGrade | SurfaceClass::Window | SurfaceClass::Door)
    }

    /// The default U-value (W/m²K) of a class (E-37).
    pub fn default_u(self) -> f64 {
        match self {
            SurfaceClass::ExteriorWall => 0.28,
            SurfaceClass::InteriorWall => 1.8,
            SurfaceClass::Roof => 0.18,
            SurfaceClass::InteriorCeiling | SurfaceClass::InteriorFloor => 1.5,
            SurfaceClass::SlabOnGrade => 0.25,
            SurfaceClass::Window => 1.4,
            SurfaceClass::Door => 1.8,
            SurfaceClass::Air => 0.0,
        }
    }
}

/// The default solar heat gain coefficient of glazing (E-37).
pub const DEFAULT_SHGC: f64 = 0.6;

/// A heat-transfer surface of an analytical space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Surface {
    pub class: SurfaceClass,
    /// m².
    pub area: f64,
    /// W/m²K.
    pub u: f64,
    /// Whether the U-value is the element type's (else the class default).
    pub u_from_type: bool,
    /// Outward unit normal (out of the space).
    pub normal: [f64; 3],
    /// The element making it (wall, window, door, floor…), if any.
    pub element: Option<ElementId>,
    /// The space beyond it (interior surfaces).
    pub adjacent: Option<ElementId>,
    /// Windows: the solar heat gain coefficient.
    pub shgc: Option<f64>,
    /// The surface's outline (walls, windows: a vertical rectangle; floors and ceilings: the
    /// space's boundary at that height), for solar and daylight providers.
    pub outline: Vec<[f64; 3]>,
}

/// An analytical space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnalyticalSpace {
    pub space: ElementId,
    pub number: String,
    pub name: String,
    pub level: Option<ElementId>,
    pub zone: Option<ElementId>,
    pub area: f64,
    pub volume: f64,
    pub height: f64,
    pub surfaces: Vec<Surface>,
}

impl AnalyticalSpace {
    /// UA (W/K) of the surfaces to the outside (exterior and ground).
    pub fn ua_exterior(&self) -> f64 {
        self.surfaces.iter().filter(|s| s.class.exterior()).map(|s| s.u * s.area).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EnergyModel {
    pub spaces: Vec<AnalyticalSpace>,
    /// Spaces left out (unplaced, unenclosed), with why.
    pub skipped: Vec<(ElementId, String)>,
}

/// The common property set of an element type's class.
fn common_pset(class: &str) -> &'static str {
    match class.trim_start_matches("Ifc").trim_end_matches("Type") {
        "Window" => "Pset_WindowCommon",
        "Door" => "Pset_DoorCommon",
        "Slab" | "Covering" => "Pset_SlabCommon",
        "Roof" => "Pset_RoofCommon",
        _ => "Pset_WallCommon",
    }
}

fn type_of<'a>(store: &'a Store, e: &Element) -> Option<&'a Element> {
    store.get(store.find_by_guid(&GlobalId::parse(e.param("@type")?.as_str()?)?)?)
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.get("value")?.as_f64())
}

/// An element's U-value from its type (or itself), if set.
pub fn u_of(store: &Store, e: &Element) -> Option<f64> {
    let t = type_of(store, e).unwrap_or(e);
    let key = format!("{}.ThermalTransmittance", common_pset(&t.class));
    t.param(&key).and_then(num).or_else(|| e.param(&key).and_then(num))
}

fn shgc_of(store: &Store, e: &Element) -> Option<f64> {
    let t = type_of(store, e).unwrap_or(e);
    t.param("Pset_DoorWindowGlazingType.SolarHeatGainTransmittance").and_then(num)
}

/// The space on `level` whose region contains `p`.
fn space_at(spaces: &[(ElementId, String, crate::rooms::RoomGeometry)], level: &str, p: [f64; 2], except: ElementId) -> Option<ElementId> {
    spaces.iter().find(|(id, l, g)| *id != except && l == level && g.region.as_ref().is_some_and(|r| r.contains(p))).map(|s| s.0)
}

/// Builds the energy model now.
pub fn build(store: &Store) -> EnergyModel {
    let levels = crate::levels::list(store);
    let mut placed = Vec::new();
    let mut skipped = Vec::new();
    for (id, r, g) in crate::mep_spaces::all(store) {
        match g {
            Some(g) if g.region.is_some() => placed.push((id, r.level.clone(), g)),
            _ => skipped.push((id, "not placed or not enclosed".to_owned())),
        }
    }
    let zones = crate::system_zones::zone_of_spaces(store);
    let inserts = crate::inserts::placed(store);
    let mut out = Vec::new();
    for (id, level, g) in &placed {
        let e = store.get(*id).expect("listed");
        let region = g.region.as_ref().expect("enclosed");
        let height = (g.z_top - g.z_base).max(0.0);
        let mut surfaces = Vec::new();
        for edge in &region.outer {
            let (a, b) = (edge.line.start, edge.line.end);
            let len = edge.line.length();
            if len < 1e-6 {
                continue;
            }
            // The outward normal of a counter-clockwise loop is to the right of the edge.
            let (dx, dy) = ((b[0] - a[0]) / len, (b[1] - a[1]) / len);
            let n = [dy, -dx, 0.0];
            let rect = |s: f64, t: f64, z0: f64, z1: f64| {
                let p = |u: f64, z: f64| [a[0] + dx * u, a[1] + dy * u, z];
                vec![p(s, z0), p(t, z0), p(t, z1), p(s, z1)]
            };
            let Some((wall_id, _)) = &edge.lock else { continue };
            let Some(wall) = store.get(*wall_id) else { continue };
            if crate::walls::wall_recipe(wall).is_none() {
                // A separation line.
                surfaces.push(Surface { class: SurfaceClass::Air, area: len * height, u: 0.0, u_from_type: false, normal: n, element: Some(*wall_id), adjacent: None, shgc: None, outline: rect(0.0, len, g.z_base, g.z_top) });
                continue;
            }
            let width = crate::walls::regenerate(store, wall).map(|w| w.width).unwrap_or(0.2);
            let mid = [(a[0] + b[0]) / 2.0 + n[0] * (width + 0.05), (a[1] + b[1]) / 2.0 + n[1] * (width + 0.05)];
            let adjacent = space_at(&placed, level, mid, *id);
            let mut wall_area = len * height;
            let mut openings = Vec::new();
            if adjacent.is_none() {
                // Windows and doors of this wall whose width overlaps the edge.
                for ins in inserts.iter().filter(|p| p.host == *wall_id) {
                    let w = &ins.wall;
                    let u = w.direction();
                    let along = |p: [f64; 2]| (p[0] - w.start[0]) * u[0] + (p[1] - w.start[1]) * u[1];
                    let (e0, e1) = (along(a).min(along(b)), along(a).max(along(b)));
                    let (i0, i1) = (ins.recipe.along - ins.ty.width / 2.0, ins.recipe.along + ins.ty.width / 2.0);
                    let overlap = (e1.min(i1) - e0.max(i0)).max(0.0);
                    if overlap <= 1e-6 {
                        continue;
                    }
                    let z0 = ins.z0.max(g.z_base);
                    let z1 = (ins.z0 + ins.ty.height).min(g.z_top);
                    if z1 <= z0 {
                        continue;
                    }
                    let area = overlap * (z1 - z0);
                    wall_area -= area;
                    let ie = store.get(ins.id);
                    let window = ins.kind == crate::inserts::Kind::Window;
                    let class = if window { SurfaceClass::Window } else { SurfaceClass::Door };
                    let u = ie.and_then(|x| u_of(store, x));
                    // Its position along the edge.
                    let s0 = (e0.max(i0) - along(a)).abs().min((e1.min(i1) - along(a)).abs());
                    let s1 = s0 + overlap;
                    openings.push(Surface {
                        class,
                        area,
                        u: u.unwrap_or_else(|| class.default_u()),
                        u_from_type: u.is_some(),
                        normal: n,
                        element: Some(ins.id),
                        adjacent: None,
                        shgc: window.then(|| ie.and_then(|x| shgc_of(store, x)).unwrap_or(DEFAULT_SHGC)),
                        outline: rect(s0, s1, z0, z1),
                    });
                }
            }
            let class = if adjacent.is_some() { SurfaceClass::InteriorWall } else { SurfaceClass::ExteriorWall };
            let u = u_of(store, wall);
            surfaces.push(Surface { class, area: wall_area.max(0.0), u: u.unwrap_or_else(|| class.default_u()), u_from_type: u.is_some(), normal: n, element: Some(*wall_id), adjacent, shgc: None, outline: rect(0.0, len, g.z_base, g.z_top) });
            surfaces.extend(openings);
        }
        // Floor and ceiling.
        let ring = region.outer_ring();
        let c = ring.iter().fold([0.0, 0.0], |s, p| [s[0] + p[0] / ring.len() as f64, s[1] + p[1] / ring.len() as f64]);
        let probe = crate::rooms::interior_point(region).unwrap_or(c);
        let lz = levels.iter().position(|l| l.guid == *level);
        let below = lz.and_then(|k| k.checked_sub(1)).and_then(|k| levels.get(k)).and_then(|l| space_at(&placed, &l.guid, probe, *id));
        let above = lz.and_then(|k| levels.get(k + 1)).and_then(|l| space_at(&placed, &l.guid, probe, *id));
        let area = region.area();
        let at = |z: f64| ring.iter().map(|p| [p[0], p[1], z]).collect::<Vec<_>>();
        let (fc, cc) = (if below.is_some() { SurfaceClass::InteriorFloor } else { SurfaceClass::SlabOnGrade }, if above.is_some() { SurfaceClass::InteriorCeiling } else { SurfaceClass::Roof });
        surfaces.push(Surface { class: fc, area, u: fc.default_u(), u_from_type: false, normal: [0.0, 0.0, -1.0], element: None, adjacent: below, shgc: None, outline: at(g.z_base) });
        surfaces.push(Surface { class: cc, area, u: cc.default_u(), u_from_type: false, normal: [0.0, 0.0, 1.0], element: None, adjacent: above, shgc: None, outline: at(g.z_top) });
        let (number, name) = crate::rooms::number_and_name(e);
        out.push(AnalyticalSpace {
            space: *id,
            number,
            name,
            level: GlobalId::parse(level).and_then(|gl| store.find_by_guid(&gl)),
            zone: zones.get(id).copied(),
            area,
            volume: area * height,
            height,
            surfaces,
        });
    }
    EnergyModel { spaces: out, skipped }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// `energy_model.get`.
pub(crate) fn get(store: &Store, _p: Empty) -> Result<EnergyModel> {
    Ok(build(store))
}

/// A summary of the model.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Summary {
    pub spaces: usize,
    pub skipped: usize,
    /// m² by surface class.
    pub areas: BTreeMap<String, f64>,
    /// Surfaces using a class default U-value.
    pub default_u_surfaces: usize,
    /// W/K to the outside.
    pub ua_exterior: f64,
    /// Glazing area ÷ exterior wall area including the glazing.
    pub window_to_wall: f64,
}

/// `energy_model.summary`.
pub(crate) fn summary(store: &Store, _p: Empty) -> Result<Summary> {
    let m = build(store);
    let mut areas: BTreeMap<String, f64> = BTreeMap::new();
    let mut defaults = 0;
    for s in m.spaces.iter().flat_map(|s| &s.surfaces) {
        let k = serde_json::to_value(s.class).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        *areas.entry(k).or_default() += s.area;
        if !s.u_from_type && s.class != SurfaceClass::Air {
            defaults += 1;
        }
    }
    let glazing = areas.get("window").copied().unwrap_or(0.0);
    let walls = areas.get("exterior_wall").copied().unwrap_or(0.0) + glazing + areas.get("door").copied().unwrap_or(0.0);
    Ok(Summary {
        spaces: m.spaces.len(),
        skipped: m.skipped.len(),
        default_u_surfaces: defaults,
        ua_exterior: m.spaces.iter().map(AnalyticalSpace::ua_exterior).sum(),
        window_to_wall: if walls > 0.0 { glazing / walls } else { 0.0 },
        areas,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThermalParams {
    /// An element type (wall, window, door, floor or roof type).
    pub type_id: ElementRef,
    /// W/m²K; null clears it (the class default is used).
    pub u_value: Option<f64>,
    /// Windows: the solar heat gain coefficient (0–1); null clears it.
    #[serde(default, deserialize_with = "some")]
    pub shgc: Option<Option<f64>>,
}

fn some<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Option<f64>>, D::Error> {
    Option::<f64>::deserialize(d).map(Some)
}

/// `energy_model.set_thermal`: a type's U-value (written to its common property set, so it goes
/// to IFC) and, for windows, its SHGC.
pub(crate) fn set_thermal(txn: &mut Transaction<'_>, p: ThermalParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.type_id).ok_or_else(|| bad("element.not_found", "no such type"))?;
    let t = store.get(id).expect("resolved");
    if !crate::categories::is_type(t) {
        return Err(bad("energy_model.not_a_type", format!("element {id} is not a type")));
    }
    if p.u_value.is_some_and(|u| !(u > 0.0 && u < 20.0)) {
        return Err(bad("energy_model.u_value", "a U-value runs from above 0 to 20 W/m²K"));
    }
    if p.shgc.flatten().is_some_and(|g| !(0.0..=1.0).contains(&g)) {
        return Err(bad("energy_model.shgc", "the solar heat gain coefficient runs from 0 to 1"));
    }
    let key = format!("{}.ThermalTransmittance", common_pset(&t.class));
    let (u, g) = (p.u_value, p.shgc);
    txn.modify(id, |e| {
        match u {
            Some(u) => {
                e.params.insert(key, json!({ "type": "IfcThermalTransmittanceMeasure", "value": u }));
            }
            None => {
                e.params.remove(&key);
            }
        }
        match g {
            Some(Some(g)) => {
                e.params.insert("Pset_DoorWindowGlazingType.SolarHeatGainTransmittance".into(), json!({ "type": "IfcNormalisedRatioMeasure", "value": g }));
            }
            Some(None) => {
                e.params.remove("Pset_DoorWindowGlazingType.SolarHeatGainTransmittance");
            }
            None => {}
        }
    })
}
