// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! System-zones (MEP-ZONE-001): regions sketched per level, or covering a whole level, that mark
//! the area served by an air system, a water loop or a piece of zone equipment. They also do
//! what HVAC zones used to do.
//!
//! - A system-zone is an `IfcSpatialZone` (THERMAL) with `recipe.system_zone`: its level, its
//!   boundary (none: the whole level, the box round everything on it, kept up to date) and its
//!   height (none: up to the next level, or 3 m on the top level).
//! - **Spaces**: every MEP space (MEP-SPACE-001) on its level whose region is inside or touches
//!   the boundary is assigned to it. Until the energy model exists (ANA-EM-003) this is worked
//!   out from the geometry whenever it is asked for.
//! - **Served by** (`rxd:system_zone`): one piece of mechanical equipment or one MEP system
//!   (an air system or a water loop). Its heating and cooling set points, humidity limits and
//!   outdoor-air rates are kept so legacy zones convert without loss.
//! - **Legacy zones**: an `IfcZone` from another tool, its members listed as read-only
//!   `@GroupMembers` (their GlobalIds), converts into a system-zone whose region envelopes its
//!   spaces (`system_zone.convert_legacy`, which works out the footprints and calls
//!   `system_zone.apply_convert`).
//! - In IFC each system-zone has a body and is referenced in its storey; on save it is also
//!   written as an `IfcZone` (ObjectType "SystemZone") grouping its spaces and its equipment,
//!   with `Pset_ZoneCommon`, the system that serves it by `IfcRelServicesBuildings`, and its set
//!   points in `Pset_SpaceThermalRequirements` on each space.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mesh_objects::{self, Category};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const CLASS: &str = "IfcSpatialZone";
pub const RECIPE: &str = "recipe.system_zone";
pub const PROPS_KEY: &str = "rxd:system_zone";
/// On an imported `IfcZone`: the GlobalIds of what its `IfcRelAssignsToGroup` lists.
pub const MEMBERS_KEY: &str = "@GroupMembers";
/// Height of a zone on the top level (m).
pub const DEFAULT_HEIGHT: f64 = 3.0;
/// A space within this distance of the boundary touches it (m).
const TOUCH: f64 = 1e-4;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ZoneRecipe {
    pub kind: String,
    /// The level (GlobalId).
    pub level: String,
    /// The boundary in plan, counter-clockwise; none covers the whole level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<[f64; 2]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
}

/// What serves a zone and its comfort requirements.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ZoneProps {
    /// The equipment or MEP system that serves it (GlobalId).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub served_by: Option<String>,
    /// °C.
    pub heating_set_point: Option<f64>,
    pub cooling_set_point: Option<f64>,
    /// Relative humidity, %.
    pub humidity_min: Option<f64>,
    pub humidity_max: Option<f64>,
    /// m³/s per person and per m² of floor.
    pub outside_air_per_person: Option<f64>,
    pub outside_air_per_area: Option<f64>,
    /// The legacy zone it was converted from (its GlobalId).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub converted_from: Option<String>,
}

pub fn recipe(e: &Element) -> Option<ZoneRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn props(e: &Element) -> ZoneProps {
    e.param(PROPS_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// An `IfcZone` from another tool that can be converted.
pub fn is_legacy(e: &Element) -> bool {
    e.class == "IfcZone"
}

fn level_of(store: &Store, r: &ZoneRecipe) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(&r.level)?)
}

/// A zone's region now: its level, outline (counter-clockwise), base elevation and height.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneGeometry {
    pub level: ElementId,
    pub outline: Vec<[f64; 2]>,
    pub base: f64,
    pub height: f64,
}

impl ZoneGeometry {
    pub fn area(&self) -> f64 {
        crate::sketch::signed_area(&self.outline).abs()
    }
}

/// The box round everything on a level: its walls' outlines and its spaces.
fn level_box(store: &Store, level: ElementId) -> Option<Vec<[f64; 2]>> {
    let z = crate::levels::elevation(store.get(level)?);
    let guid = store.get(level)?.guid().to_string();
    let mut pts: Vec<[f64; 2]> = crate::rooms::space_edges(store, z).iter().flat_map(|e| [e.line.start, e.line.end]).collect();
    for (_, r, g) in crate::mep_spaces::all(store) {
        if r.level == guid
            && let Some(reg) = g.as_ref().and_then(|g| g.region.as_ref())
        {
            pts.extend(reg.outer_ring());
        }
    }
    if pts.is_empty() {
        return None;
    }
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &pts {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (hi[0] - lo[0] > 1e-6 && hi[1] - lo[1] > 1e-6).then(|| vec![lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]])
}

fn height_above(store: &Store, level: ElementId) -> f64 {
    let z = store.get(level).map(crate::levels::elevation).unwrap_or(0.0);
    crate::levels::list(store).into_iter().find(|l| l.elevation > z + 1e-6).map_or(DEFAULT_HEIGHT, |l| l.elevation - z)
}

pub fn geometry(store: &Store, e: &Element) -> Option<ZoneGeometry> {
    let r = recipe(e)?;
    let level = level_of(store, &r)?;
    let outline = match &r.boundary {
        Some(b) => b.clone(),
        None => level_box(store, level)?,
    };
    Some(ZoneGeometry {
        level,
        outline,
        base: crate::levels::elevation(store.get(level)?),
        height: r.height.unwrap_or_else(|| height_above(store, level)),
    })
}

/// Every system-zone.
pub fn all(store: &Store) -> Vec<ElementId> {
    store.iter().filter(|e| recipe(e).is_some()).map(|e| e.id()).collect()
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

fn near_ring(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    (0..ring.len()).any(|i| seg_dist(p, ring[i], ring[(i + 1) % ring.len()]) <= TOUCH)
}

fn cross(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn segments_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

/// Whether two outlines overlap or touch.
pub fn overlaps_or_touches(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    if a.iter().any(|p| inside(*p, b) || near_ring(*p, b)) || b.iter().any(|p| inside(*p, a) || near_ring(*p, a)) {
        return true;
    }
    (0..a.len()).any(|i| (0..b.len()).any(|j| segments_cross(a[i], a[(i + 1) % a.len()], b[j], b[(j + 1) % b.len()])))
}

/// The MEP spaces assigned to a zone: on its level, inside or touching its boundary.
pub fn spaces(store: &Store, zone: ElementId) -> Vec<ElementId> {
    let Some(g) = store.get(zone).and_then(|e| geometry(store, e)) else { return Vec::new() };
    let Some(level) = store.get(g.level).map(|l| l.guid().to_string()) else { return Vec::new() };
    let mut out: Vec<(String, ElementId)> = crate::mep_spaces::all(store)
        .into_iter()
        .filter(|(_, r, _)| r.level == level)
        .filter_map(|(id, _, sg)| {
            let ring = sg?.region?.outer_ring();
            overlaps_or_touches(&g.outline, &ring).then(|| (store.get(id).and_then(|e| e.name.clone()).unwrap_or_default(), id))
        })
        .collect();
    out.sort_by(|a, b| natural(&a.0, &b.0).then(a.1.cmp(&b.1)));
    out.into_iter().map(|s| s.1).collect()
}

fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x.total_cmp(&y),
        _ => a.cmp(b),
    }
}

/// What serves a zone, if it still exists: the element and whether it is a system.
pub fn served_by(store: &Store, e: &Element) -> Option<(ElementId, bool)> {
    let id = store.find_by_guid(&GlobalId::parse(props(e).served_by.as_deref()?)?)?;
    let el = store.get(id)?;
    Some((id, el.class == crate::mep_systems::CLASS))
}

/// Whether an element can serve a zone: a piece of mechanical equipment or an MEP system.
fn can_serve(store: &Store, e: &Element) -> bool {
    if e.class == crate::mep_systems::CLASS && crate::mep_systems::recipe(e).is_some() {
        return true;
    }
    mesh_objects::mesh_object_recipe(e).is_some() && mesh_objects::type_of(store, e).is_some_and(|(_, t)| t.category == Category::MechanicalEquipment)
}

fn resolve_zone(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such system-zone"))?;
    store.get(id).and_then(recipe).ok_or_else(|| bad("system_zone.not_a_zone", format!("element {id} is not a system-zone")))?;
    Ok(id)
}

fn check_boundary(b: &[[f64; 2]]) -> Result<Vec<[f64; 2]>> {
    let mut ring: Vec<[f64; 2]> = Vec::new();
    for p in b {
        if !p.iter().all(|v| v.is_finite()) {
            return Err(bad("system_zone.bad_boundary", "the boundary has a point that is not a number"));
        }
        if ring.last().is_none_or(|q| (q[0] - p[0]).hypot(q[1] - p[1]) > 1e-6) {
            ring.push(*p);
        }
    }
    if ring.len() > 1 && (ring[0][0] - ring[ring.len() - 1][0]).hypot(ring[0][1] - ring[ring.len() - 1][1]) <= 1e-6 {
        ring.pop();
    }
    if ring.len() < 3 {
        return Err(bad("system_zone.bad_boundary", "a system-zone boundary needs at least three corners"));
    }
    let n = ring.len();
    for i in 0..n {
        for j in i + 1..n {
            if (j + 1) % n == i || j == i + 1 {
                continue;
            }
            if segments_cross(ring[i], ring[(i + 1) % n], ring[j], ring[(j + 1) % n]) {
                return Err(bad("system_zone.bad_boundary", "the boundary crosses itself").with_details(serde_json::json!({ "points": [ring[i], ring[j]] })));
            }
        }
    }
    let a = crate::sketch::signed_area(&ring);
    if a.abs() < 1e-6 {
        return Err(bad("system_zone.bad_boundary", "the boundary encloses no area"));
    }
    if a < 0.0 {
        ring.reverse();
    }
    Ok(ring)
}

fn check_props(p: &ZoneProps) -> Result<()> {
    if let (Some(h), Some(c)) = (p.heating_set_point, p.cooling_set_point)
        && h >= c
    {
        return Err(bad("system_zone.bad_value", format!("the heating set point ({h} °C) must be below the cooling set point ({c} °C)")));
    }
    for (what, v) in [("humidity minimum", p.humidity_min), ("humidity maximum", p.humidity_max)] {
        if v.is_some_and(|v| !(0.0..=100.0).contains(&v)) {
            return Err(bad("system_zone.bad_value", format!("the {what} must be between 0 and 100 %")));
        }
    }
    if let (Some(lo), Some(hi)) = (p.humidity_min, p.humidity_max)
        && lo >= hi
    {
        return Err(bad("system_zone.bad_value", "the humidity minimum must be below the maximum"));
    }
    for (what, v) in [("outdoor air per person", p.outside_air_per_person), ("outdoor air per area", p.outside_air_per_area)] {
        if v.is_some_and(|v| v.is_nan() || v < 0.0) {
            return Err(bad("system_zone.bad_value", format!("the {what} cannot be negative")));
        }
    }
    Ok(())
}

fn next_name(store: &Store) -> String {
    let n = store
        .iter()
        .filter(|e| recipe(e).is_some())
        .filter_map(|e| e.name.as_deref()?.strip_prefix("System-Zone ")?.trim().parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("System-Zone {}", n + 1)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub level_id: ElementRef,
    /// The boundary's corners in plan (metres), in order; omitted with `whole_level`.
    pub boundary: Option<Vec<[f64; 2]>>,
    /// Cover the whole level (By Level): the box round everything on it, kept up to date.
    #[serde(default)]
    pub whole_level: bool,
    /// Default: "System-Zone n".
    pub name: Option<String>,
    /// Metres; default up to the next level (3 m on the top level).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<Value>,
}

fn create_zone(txn: &mut Transaction<'_>, level_id: &ElementRef, boundary: Option<Vec<[f64; 2]>>, name: Option<String>, height: Option<f64>, props: &ZoneProps) -> Result<ElementId> {
    let store = txn.store().clone();
    let (level, level_guid) = crate::rooms::resolve_level(&store, level_id)?;
    if height.is_some_and(|h| h.is_nan() || h <= 0.0) {
        return Err(bad("system_zone.bad_value", "the height must be above zero"));
    }
    let boundary = match boundary {
        Some(b) => Some(check_boundary(&b)?),
        None => {
            level_box(&store, level).ok_or_else(|| bad("system_zone.empty_level", "there is nothing on the level for a whole-level system-zone to cover; sketch its boundary"))?;
            None
        }
    };
    check_props(props)?;
    let r = ZoneRecipe { kind: RECIPE.into(), level: level_guid, boundary, height };
    txn.create(
        NewElement::new(CLASS)
            .name(name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| next_name(&store)))
            .param("@PredefinedType", "THERMAL")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))
            .param(PROPS_KEY, serde_json::to_value(props).expect("serialises")),
    )
}

/// `system_zone.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let boundary = match (p.boundary, p.whole_level) {
        (Some(_), true) => return Err(bad("system_zone.bad_args", "give a boundary or whole_level, not both")),
        (None, false) => return Err(bad("system_zone.bad_args", "give a boundary, or whole_level for the whole level")),
        (b, _) => b,
    };
    let height = p.height.as_ref().map(|v| crate::rooms::length(txn.store(), v, "the height")).transpose()?;
    create_zone(txn, &p.level_id, boundary, p.name, height, &ZoneProps::default())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignParams {
    pub zone_id: ElementRef,
    /// A piece of mechanical equipment or an MEP system (air system or water loop); null clears.
    pub equipment_or_system_id: Option<ElementRef>,
}

/// `system_zone.assign`.
pub(crate) fn assign(txn: &mut Transaction<'_>, p: AssignParams) -> Result<()> {
    let store = txn.store().clone();
    let zone = resolve_zone(&store, &p.zone_id)?;
    let guid = match &p.equipment_or_system_id {
        None => None,
        Some(r) => {
            let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such equipment or system"))?;
            let e = store.get(id).expect("resolved");
            if !can_serve(&store, e) {
                return Err(bad("system_zone.cannot_serve", format!("element {id} is neither mechanical equipment nor an MEP system")));
            }
            Some(e.guid().to_string())
        }
    };
    let mut pr = props(store.get(zone).expect("resolved"));
    pr.served_by = guid;
    let v = serde_json::to_value(&pr).expect("serialises");
    txn.modify(zone, |e| {
        e.params.insert(PROPS_KEY.into(), v);
    })
}

/// A value to set, keep (omitted) or clear (null).
fn patch(slot: &mut Option<f64>, v: &Option<Option<f64>>) {
    if let Some(v) = v {
        *slot = *v;
    }
}

fn double_option<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Option<f64>>, D::Error> {
    Option::<f64>::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub zone_ids: Vec<ElementRef>,
    pub name: Option<String>,
    /// A new boundary (metres), or `whole_level`.
    pub boundary: Option<Vec<[f64; 2]>>,
    #[serde(default)]
    pub whole_level: bool,
    /// Metres; null follows the levels again.
    #[serde(default, deserialize_with = "double_option")]
    pub height: Option<Option<f64>>,
    /// °C; null clears.
    #[serde(default, deserialize_with = "double_option")]
    pub heating_set_point: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub cooling_set_point: Option<Option<f64>>,
    /// Relative humidity, %.
    #[serde(default, deserialize_with = "double_option")]
    pub humidity_min: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub humidity_max: Option<Option<f64>>,
    /// m³/s per person.
    #[serde(default, deserialize_with = "double_option")]
    pub outside_air_per_person: Option<Option<f64>>,
    /// m³/s per m².
    #[serde(default, deserialize_with = "double_option")]
    pub outside_air_per_area: Option<Option<f64>>,
}

/// `system_zone.set`.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    if p.boundary.is_some() && p.whole_level {
        return Err(bad("system_zone.bad_args", "give a boundary or whole_level, not both"));
    }
    let boundary = p.boundary.as_deref().map(check_boundary).transpose()?;
    if p.zone_ids.is_empty() {
        return Err(bad("system_zone.bad_args", "no system-zones given"));
    }
    for r in &p.zone_ids {
        let store = txn.store().clone();
        let id = resolve_zone(&store, r)?;
        let e = store.get(id).expect("resolved");
        let mut rec = recipe(e).expect("a zone");
        if let Some(b) = &boundary {
            rec.boundary = Some(b.clone());
        } else if p.whole_level {
            let level = level_of(&store, &rec).ok_or_else(|| bad("system_zone.no_level", "the system-zone's level is gone"))?;
            level_box(&store, level).ok_or_else(|| bad("system_zone.empty_level", "there is nothing on the level to cover"))?;
            rec.boundary = None;
        }
        if let Some(h) = p.height {
            if h.is_some_and(|h| h.is_nan() || h <= 0.0) {
                return Err(bad("system_zone.bad_value", "the height must be above zero"));
            }
            rec.height = h;
        }
        let mut pr = props(e);
        patch(&mut pr.heating_set_point, &p.heating_set_point);
        patch(&mut pr.cooling_set_point, &p.cooling_set_point);
        patch(&mut pr.humidity_min, &p.humidity_min);
        patch(&mut pr.humidity_max, &p.humidity_max);
        patch(&mut pr.outside_air_per_person, &p.outside_air_per_person);
        patch(&mut pr.outside_air_per_area, &p.outside_air_per_area);
        check_props(&pr)?;
        let (rv, pv) = (serde_json::to_value(&rec).expect("serialises"), serde_json::to_value(&pr).expect("serialises"));
        let name = p.name.clone().filter(|n| !n.trim().is_empty());
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), rv);
            e.params.insert(PROPS_KEY.into(), pv);
            if let Some(n) = name {
                e.name = Some(n);
            }
        })?;
    }
    Ok(())
}

/// A system-zone as it is now.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ZoneInfo {
    pub id: ElementId,
    pub name: String,
    pub level_id: Option<ElementId>,
    pub level: String,
    pub whole_level: bool,
    /// The outline now (metres).
    pub outline: Vec<[f64; 2]>,
    pub area: f64,
    pub height: f64,
    /// Its assigned spaces, in number order.
    pub spaces: Vec<ElementId>,
    pub space_numbers: Vec<String>,
    pub served_by: Option<ElementId>,
    pub served_by_name: Option<String>,
    /// "equipment" or "system".
    pub served_by_kind: Option<String>,
    pub props: ZoneProps,
}

pub fn info(store: &Store, id: ElementId) -> Option<ZoneInfo> {
    let e = store.get(id)?;
    let r = recipe(e)?;
    let g = geometry(store, e);
    let level = level_of(store, &r);
    let spaces = spaces(store, id);
    let served = served_by(store, e);
    let label = |id: ElementId| {
        let el = store.get(id)?;
        el.name.clone().filter(|n| !n.is_empty()).or_else(|| {
            let (t, _) = mesh_objects::type_of(store, el)?;
            store.get(t)?.name.clone()
        })
    };
    Some(ZoneInfo {
        id,
        name: e.name.clone().unwrap_or_default(),
        level_id: level,
        level: level.and_then(|l| store.get(l)?.name.clone()).unwrap_or_default(),
        whole_level: r.boundary.is_none(),
        area: g.as_ref().map_or(0.0, ZoneGeometry::area),
        height: g.as_ref().map_or(0.0, |g| g.height),
        outline: g.map(|g| g.outline).unwrap_or_default(),
        space_numbers: spaces.iter().map(|s| store.get(*s).and_then(|e| e.name.clone()).unwrap_or_default()).collect(),
        spaces,
        served_by: served.map(|s| s.0),
        served_by_name: served.and_then(|s| label(s.0)),
        served_by_kind: served.map(|s| if s.1 { "system".into() } else { "equipment".into() }),
        props: props(e),
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {
    /// System-zones; every one when empty.
    #[serde(default)]
    pub zone_ids: Vec<ElementRef>,
}

/// `system_zone.info`.
pub(crate) fn info_cmd(store: &Store, p: InfoParams) -> Result<Vec<ZoneInfo>> {
    let ids: Vec<ElementId> = if p.zone_ids.is_empty() { all(store) } else { p.zone_ids.iter().map(|r| resolve_zone(store, r)).collect::<Result<_>>()? };
    Ok(ids.into_iter().filter_map(|id| info(store, id)).collect())
}

/// Every system-zone's information.
pub fn infos(store: &Store) -> Vec<ZoneInfo> {
    all(store).into_iter().filter_map(|id| info(store, id)).collect()
}

/// The system-zone each space is assigned to (the first by id when several cover it).
pub fn zone_of_spaces(store: &Store) -> std::collections::BTreeMap<ElementId, ElementId> {
    let mut m = std::collections::BTreeMap::new();
    for z in all(store) {
        for s in spaces(store, z) {
            m.entry(s).or_insert(z);
        }
    }
    m
}

/// A legacy zone to convert, as `system_zone.convert_legacy` worked it out.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Converted {
    /// The legacy `IfcZone`.
    pub legacy_id: ElementRef,
    pub level_id: ElementRef,
    /// The outline enveloping its spaces (metres).
    pub boundary: Vec<[f64; 2]>,
    pub height: Option<f64>,
    #[serde(default)]
    pub props: ZoneProps,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyConvertParams {
    pub zones: Vec<Converted>,
}

/// `system_zone.apply_convert`: each legacy zone is replaced by a system-zone of its name.
pub(crate) fn apply_convert(txn: &mut Transaction<'_>, p: ApplyConvertParams) -> Result<Vec<ElementId>> {
    let mut out = Vec::new();
    for c in p.zones {
        let store = txn.store().clone();
        let legacy = store.resolve(&c.legacy_id).ok_or_else(|| bad("element.not_found", "no such zone"))?;
        let e = store.get(legacy).expect("resolved");
        if !is_legacy(e) {
            return Err(bad("system_zone.not_legacy", format!("element {legacy} is not an IfcZone")));
        }
        let mut pr = c.props.clone();
        pr.converted_from = Some(e.guid().to_string());
        let name = e.name.clone();
        let id = create_zone(txn, &c.level_id, Some(c.boundary), name, c.height, &pr)?;
        txn.delete(legacy)?;
        out.push(id);
    }
    Ok(out)
}

/// The convex hull of points, counter-clockwise.
pub fn hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut p: Vec<[f64; 2]> = points.to_vec();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9);
    if p.len() < 3 {
        return p;
    }
    let mut lower: Vec<[f64; 2]> = Vec::new();
    for q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *q) <= 0.0 {
            lower.pop();
        }
        lower.push(*q);
    }
    let mut upper: Vec<[f64; 2]> = Vec::new();
    for q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *q) <= 0.0 {
            upper.pop();
        }
        upper.push(*q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The set points a legacy zone carries, from `Pset_SpaceThermalRequirements` on the zone or,
/// failing that, on its first member that has them. Temperatures above 150 are taken as kelvin;
/// humidity at or below 1 as a ratio.
pub fn legacy_props(store: &Store, zone: &Element) -> ZoneProps {
    let read = |e: &Element, names: &[&str]| -> Option<f64> {
        names.iter().find_map(|n| {
            let v = e.param(&format!("Pset_SpaceThermalRequirements.{n}"))?;
            v.as_f64().or_else(|| v.get("value")?.as_f64())
        })
    };
    let temp = |v: f64| if v > 150.0 { v - 273.15 } else { v };
    let hum = |v: f64| if v <= 1.0 { v * 100.0 } else { v };
    let from = |e: &Element| ZoneProps {
        heating_set_point: read(e, &["SpaceTemperatureMin", "SpaceTemperatureWinterMin", "SpaceTemperatureWinter"]).map(temp),
        cooling_set_point: read(e, &["SpaceTemperatureMax", "SpaceTemperatureSummerMax", "SpaceTemperatureSummer"]).map(temp),
        humidity_min: read(e, &["SpaceHumidityMin"]).map(hum),
        humidity_max: read(e, &["SpaceHumidityMax"]).map(hum),
        ..ZoneProps::default()
    };
    let mut p = from(zone);
    if p == ZoneProps::default() {
        for m in members(store, zone) {
            let q = store.get(m).map(from).unwrap_or_default();
            if q != ZoneProps::default() {
                p = q;
                break;
            }
        }
    }
    // Inverted or out-of-range values from another tool are dropped, not refused.
    if let (Some(h), Some(c)) = (p.heating_set_point, p.cooling_set_point)
        && h >= c
    {
        p.cooling_set_point = None;
    }
    for v in [&mut p.humidity_min, &mut p.humidity_max] {
        if v.is_some_and(|x| !(0.0..=100.0).contains(&x)) {
            *v = None;
        }
    }
    if let (Some(lo), Some(hi)) = (p.humidity_min, p.humidity_max)
        && lo >= hi
    {
        p.humidity_max = None;
    }
    p
}

/// The members of a legacy zone that are in the model.
pub fn members(store: &Store, zone: &Element) -> Vec<ElementId> {
    zone.param(MEMBERS_KEY)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|g| store.find_by_guid(&GlobalId::parse(g.as_str()?)?)).collect())
        .unwrap_or_default()
}
