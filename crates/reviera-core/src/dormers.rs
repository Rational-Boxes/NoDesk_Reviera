// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Dormers, automated (spec Ch 3 §3.6.8a, ARC-ROOF-007): one command places a dormer on a sloped
//! face of a footprint roof, with its own roof and the walls that fill the gaps down to the
//! main roof.
//!
//! The click point is the middle of the dormer's front. The dormer runs up the face's fall line:
//! - **Roof.** A Reviera footprint roof on the dormer's outline (the walls' outer faces), with
//!   its plate (eave) `front_height` above the main roof's top surface at the front:
//!   - *gable*: the sides slope and the front is a gable, the ridge running into the main roof;
//!   - *hip*: the front slopes too;
//!   - *shed*: only the front slopes, at a slope shallower than the main roof's;
//!   - *flat*: level.
//!
//!   The dormer reaches back until its ridge (gable, hip), its rising plane (shed) or its top
//!   (flat) meets the main roof's top surface. The front and sides overhang; the back does not.
//! - **Gap walls.** A front wall and two cheek walls, their outer faces on the dormer's outline,
//!   from the main roof's underside at the front up to the dormer roof (attached to it, so the
//!   gable and the sloped tops fill exactly). Cheek walls end where the dormer eave meets the main
//!   roof.
//!
//! The dormer is an `IfcElementAssembly` (ObjectType "DORMER") whose recipe lists its parts; each
//! part names its dormer, and goes with it. Editing regenerates the parts.
//!
//! **Opening** (ARC-OPEN-005): the main roof is cut through, plumb, inside the dormer walls'
//! interior faces, from the front wall back to where the cheek walls end (where the main roof's
//! top meets the dormer eave). The opening is taken from the walls as they are, so it follows
//! them. Wall bases that follow the main roof, and regeneration when the main roof changes,
//! come later.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::roofs::{self, RoofCurveArg};
use crate::store::Store;
use crate::walls::{self, Line, RECIPE_KEY};

pub const CLASS: &str = "IfcElementAssembly";
pub const DORMER_RECIPE: &str = "recipe.dormer";
/// The parameter by which a part names its dormer (GlobalId).
pub const PART_KEY: &str = "rxd:dormer";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DormerStyle {
    /// A pitched roof with a gable front; the ridge runs into the main roof.
    Gable,
    /// A pitched roof hipped at the front.
    Hip,
    /// One plane rising from the front, shallower than the main roof (a flat lift).
    Shed,
    /// A level roof.
    Flat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DormerRecipe {
    pub kind: String,
    /// The main roof (GlobalId).
    pub roof: String,
    /// The middle of the dormer's front (plan, metres).
    pub point: [f64; 2],
    pub style: DormerStyle,
    pub width: f64,
    /// The dormer eave's height above the main roof's top surface at the front.
    pub front_height: f64,
    /// The dormer roof's slope (radians): pitched sides, or the shed's front.
    pub slope: f64,
    pub overhang: f64,
    /// The parts (GlobalIds): walls and the dormer roof.
    pub parts: Vec<String>,
}

pub fn dormer_recipe(e: &Element) -> Option<DormerRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == DORMER_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// The dormer a part belongs to (GlobalId).
pub fn dormer_of(e: &Element) -> Option<&str> {
    e.param(PART_KEY)?.as_str()
}

/// Where a dormer goes on the main roof: its frame and heights.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// Up the main roof's fall line, and across it (to the left looking up), unit plan vectors.
    pub up: [f64; 2],
    pub across: [f64; 2],
    /// The main roof's slope there (tan) and its vertical thickness.
    pub tan_main: f64,
    pub thickness: f64,
    /// The main roof's underside and top surface at the point.
    pub z_under: f64,
    pub z_top: f64,
    pub level: String,
    pub level_z: f64,
}

/// The main roof's face under `point`, as a dormer frame.
pub fn placement(store: &Store, roof: &Element, point: [f64; 2]) -> Result<Placement> {
    let r = roofs::roof_recipe(roof)
        .ok_or_else(|| Error::new("dormer.not_a_roof", "dormers go on Reviera footprint roofs"))?;
    let g = roofs::regenerate(store, roof)?;
    let f = g.face_at(point).ok_or_else(|| {
        Error::new(
            "dormer.off_roof",
            "the point is not on the roof (pick a point on a sloped face)",
        )
    })?;
    let n = f.normal;
    let h = n[0].hypot(n[1]);
    if f.key.is_none() || h < 0.05 * n[2] {
        return Err(Error::new(
            "dormer.flat_face",
            "dormers go on sloped faces of a roof",
        ));
    }
    // The face falls toward its normal's horizontal part; up the slope is the other way.
    let up = [-n[0] / h, -n[1] / h];
    let across = [-up[1], up[0]];
    let tan_main = h / n[2];
    let q = f.bottom[0];
    let z_under = q[2] - (n[0] * (point[0] - q[0]) + n[1] * (point[1] - q[1])) / n[2];
    let thickness = g.thickness / n[2];
    let level_z = GlobalId::parse(&r.level)
        .and_then(|l| store.get(store.find_by_guid(&l)?))
        .map(crate::levels::elevation)
        .ok_or_else(|| Error::new("dormer.no_level", "the roof's level no longer exists"))?;
    Ok(Placement {
        up,
        across,
        tan_main,
        thickness,
        z_under,
        z_top: z_under + thickness,
        level: r.level,
        level_z,
    })
}

/// The dormer's outline depth up the slope, and the cheek walls' length. The outline reaches
/// back until the dormer roof's *top* meets the main roof's top (its ridge, rising plane or flat
/// top), so the two surfaces join flush along valleys once the dormer roof is cut by the main
/// roof's top plane; the cheek walls end where the dormer roof's underside meets it.
pub fn depths(r: &DormerRecipe, p: &Placement, roof_thickness: f64) -> Result<(f64, f64)> {
    let t = p.tan_main;
    let eave = r.front_height / t;
    // The dormer roof's plumb depth (it uses the main roof's type).
    let plumb = roof_thickness / r.slope.cos().max(1e-6);
    let depth = match r.style {
        DormerStyle::Gable | DormerStyle::Hip => {
            (r.front_height + r.width / 2.0 * r.slope.tan() + plumb) / t
        }
        DormerStyle::Shed => {
            let s = r.slope.tan();
            if s >= t - 1e-6 {
                return Err(Error::new(
                    "dormer.shed_too_steep",
                    "a shed dormer's slope must be shallower than the main roof's",
                ));
            }
            (r.front_height + plumb) / (t - s)
        }
        DormerStyle::Flat => (r.front_height + roof_thickness) / t,
    };
    let cheek = match r.style {
        DormerStyle::Shed => r.front_height / (t - r.slope.tan()),
        _ => eave,
    };
    Ok((depth, cheek))
}

fn add(p: [f64; 2], v: [f64; 2], k: f64) -> [f64; 2] {
    [p[0] + v[0] * k, p[1] + v[1] * k]
}

/// Creates a dormer's parts from its recipe; returns their ids.
fn build(
    txn: &mut crate::txn::Transaction<'_>,
    dormer_guid: &str,
    r: &DormerRecipe,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let roof_id = GlobalId::parse(&r.roof)
        .and_then(|g| store.find_by_guid(&g))
        .ok_or_else(|| Error::new("dormer.no_roof", "the dormer's roof no longer exists"))?;
    let roof = store.get(roof_id).expect("found");
    let p = placement(&store, roof, r.point)?;
    if !(r.width > 0.3 && r.front_height > 0.1) {
        return Err(Error::new(
            "dormer.too_small",
            "a dormer needs a width over 300 mm and a front height over 100 mm",
        ));
    }
    if !(r.slope > 0.0 && r.slope < 80f64.to_radians()) && r.style != DormerStyle::Flat {
        return Err(Error::new(
            "dormer.bad_slope",
            "the dormer roof's slope must be between 0° and 80°",
        ));
    }
    // The dormer roof uses the main roof's type and thickness.
    let roof_type = roof
        .param("@type")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let main = roofs::regenerate(&store, roof)?;
    let (depth, cheek) = depths(r, &p, main.thickness)?;
    let (u, a, w) = (p.up, p.across, r.width);
    // The outline (the walls' outer faces): front right, back right, back left, front left.
    let fr = add(r.point, a, -w / 2.0);
    let fl = add(r.point, a, w / 2.0);
    let br = add(fr, u, depth);
    let bl = add(fl, u, depth);
    // The whole dormer stands on the face it was placed on (between its eave, ridge and hips).
    let face = main.face_at(r.point).and_then(|f| f.key);
    for (q, what) in [(fr, "front"), (fl, "front"), (br, "back"), (bl, "back")] {
        if main.face_at(q).and_then(|f| f.key) != face {
            return Err(Error::new(
                "dormer.does_not_fit",
                format!(
                    "the dormer's {what} runs off the roof face (it is {depth:.2} m deep and {w:.2} m wide); \
                     place it lower or make it smaller"
                ),
            ));
        }
    }
    let z_eave = p.z_top + r.front_height;
    let plate = z_eave - p.level_z;
    let level_id = GlobalId::parse(&p.level)
        .and_then(|g| store.find_by_guid(&g))
        .expect("checked");
    // Front, right side, back, left side: which slope, with what overhang.
    let (front, sides) = match r.style {
        DormerStyle::Gable => (false, true),
        DormerStyle::Hip => (true, true),
        DormerStyle::Shed => (true, false),
        DormerStyle::Flat => (false, false),
    };
    let edge = |a: [f64; 2], b: [f64; 2], slopes: bool, overhang: f64| RoofCurveArg {
        line: Line { start: a, end: b },
        lock: None,
        defines_slope: Some(slopes),
        slope: None,
        overhang: Some(overhang.into()),
        eave: None,
    };
    let curves = vec![
        edge(fl, fr, front, r.overhang),
        edge(fr, br, sides, r.overhang),
        edge(br, bl, false, 0.0),
        edge(bl, fl, sides, r.overhang),
    ];
    let dormer_roof = roofs::create(
        txn,
        roofs::CreateParams {
            type_id: roof_type
                .as_deref()
                .and_then(GlobalId::parse)
                .map(ElementRef::Guid),
            curves,
            level_id: ElementRef::Id(level_id),
            base_offset: Some(plate.into()),
            slope: Some(if r.style == DormerStyle::Flat {
                0.5.into()
            } else {
                r.slope.into()
            }),
            cutoff_level_id: None,
            cutoff_offset: None,
        },
    )?;
    // The walls, centred half their width inside the outline; exterior faces outward (each
    // wall's exterior is on the left of its direction).
    let type_id = walls::default_type(txn)?;
    let wt = txn
        .store()
        .get(type_id)
        .and_then(walls::type_width)
        .unwrap_or(0.2);
    let inset = wt / 2.0;
    let base = p.z_under - p.level_z - 0.01;
    let top = z_eave + w / 2.0 * r.slope.tan() + main.thickness + 1.0 - (base + p.level_z);
    let wall = |txn: &mut crate::txn::Transaction<'_>, s: [f64; 2], e: [f64; 2]| {
        walls::create(
            txn,
            walls::CreateParams {
                type_id: Some(ElementRef::Id(type_id)),
                curve: walls::CurveArg {
                    line: Line { start: s, end: e },
                },
                base_level: ElementRef::Id(level_id),
                top_constraint: walls::TopConstraint::Unconnected(top),
                base_offset: base,
                top_offset: 0.0,
                location_line: walls::LocationLine::WallCenterline,
                flip: false,
            },
        )
    };
    // Corners of the wall centrelines.
    let cfr = add(add(fr, a, inset), u, inset);
    let cfl = add(add(fl, a, -inset), u, inset);
    let cbr = add(add(fr, a, inset), u, cheek);
    let cbl = add(add(fl, a, -inset), u, cheek);
    let front_wall = wall(txn, cfr, cfl)?;
    let left = wall(txn, cfl, cbl)?;
    let right = wall(txn, cbr, cfr)?;
    let walls_made = vec![front_wall, left, right];
    walls::attach::attach(
        txn,
        walls::attach::AttachParams {
            wall_ids: walls_made.iter().map(|w| ElementRef::Id(*w)).collect(),
            end: walls::attach::End::Top,
            target_id: ElementRef::Id(dormer_roof),
        },
    )?;
    let mut parts = vec![dormer_roof];
    parts.extend(walls_made);
    for id in &parts {
        txn.modify(*id, |e| {
            e.params.insert(PART_KEY.into(), json!(dormer_guid));
        })?;
    }
    Ok(parts)
}

/// The plane of the main roof's top surface under a dormer roof (a dormer part): its upward unit
/// normal and offset (`n · p = d`).
pub fn main_top_plane(store: &Store, dormer_roof: &Element) -> Option<([f64; 3], f64)> {
    let dormer = store.get(store.find_by_guid(&GlobalId::parse(dormer_of(dormer_roof)?)?)?)?;
    let r = dormer_recipe(dormer)?;
    let main = store.get(store.find_by_guid(&GlobalId::parse(&r.roof)?)?)?;
    let g = roofs::regenerate(store, main).ok()?;
    let f = g.face_at(r.point)?;
    let n = f.normal;
    let q = f.bottom[0];
    let top = [
        q[0] + n[0] * g.thickness,
        q[1] + n[1] * g.thickness,
        q[2] + n[2] * g.thickness,
    ];
    Some((n, n[0] * top[0] + n[1] * top[1] + n[2] * top[2]))
}

/// The openings the dormers on a roof (GlobalId) cut through it, in plan: each the region inside
/// its front and cheek walls' interior faces, from the front wall to the cheeks' ends.
pub fn openings(store: &Store, roof: &str) -> Vec<Vec<[f64; 2]>> {
    let lines = |id: &str| -> Option<(walls::WallGeometry, [f64; 2], [f64; 2])> {
        let w = store.get(store.find_by_guid(&GlobalId::parse(id)?)?)?;
        let g = walls::regenerate(store, w).ok()?;
        // The interior face: right of the wall's direction.
        let n = g.normal();
        let o = -g.width / 2.0;
        Some((
            g,
            [g.start[0] + n[0] * o, g.start[1] + n[1] * o],
            [g.end[0] + n[0] * o, g.end[1] + n[1] * o],
        ))
    };
    let meet = |a: ([f64; 2], [f64; 2]), b: ([f64; 2], [f64; 2])| -> Option<[f64; 2]> {
        let (p, d) = (a.0, [a.1[0] - a.0[0], a.1[1] - a.0[1]]);
        let (q, e) = (b.0, [b.1[0] - b.0[0], b.1[1] - b.0[1]]);
        let den = d[0] * e[1] - d[1] * e[0];
        if den.abs() < 1e-12 {
            return None;
        }
        let t = ((q[0] - p[0]) * e[1] - (q[1] - p[1]) * e[0]) / den;
        Some([p[0] + d[0] * t, p[1] + d[1] * t])
    };
    store
        .iter()
        .filter_map(dormer_recipe)
        .filter(|r| r.roof == roof)
        .filter_map(|r| {
            // Parts: the roof, then the front, left and right walls.
            let front = lines(r.parts.get(1)?)?;
            let left = lines(r.parts.get(2)?)?;
            let right = lines(r.parts.get(3)?)?;
            let f = (front.1, front.2);
            let (l, rt) = ((left.1, left.2), (right.1, right.2));
            // The left wall runs up the slope (its end at the back); the right one down it.
            Some(vec![meet(f, rt)?, meet(f, l)?, left.2, right.1])
        })
        .collect()
}

fn write_recipe(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    r: &DormerRecipe,
) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

fn number(store: &Store, v: &Value, data_type: &str, what: &str) -> Result<f64> {
    match v {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => crate::units::parse(s, data_type, &crate::units::settings_of(store)),
        _ => Err(Error::new(
            "dormer.invalid_value",
            format!("{what} must be a number or text with units"),
        )),
    }
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The main roof (a Reviera footprint roof).
    pub roof_id: ElementRef,
    /// The middle of the dormer's front, on a sloped face (plan, metres).
    pub point: [f64; 2],
    #[serde(default = "gable")]
    pub style: DormerStyle,
    /// Default 1.8 m.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<Value>,
    /// The dormer eave above the main roof's top surface at the front; default 0.9 m.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub front_height: Option<Value>,
    /// Default the main roof's slope (pitched) or a third of it (shed).
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub slope: Option<Value>,
    /// Default 300 mm.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub overhang: Option<Value>,
}

fn gable() -> DormerStyle {
    DormerStyle::Gable
}

/// A style's slope when none is given: the main roof's for pitched dormers, a third of it for a
/// shed.
fn default_slope(style: DormerStyle, main: f64) -> f64 {
    match style {
        DormerStyle::Shed => main / 3.0,
        _ => main,
    }
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let roof_id = store
        .resolve(&p.roof_id)
        .ok_or_else(|| Error::new("element.not_found", "no such roof"))?;
    let roof = store.get(roof_id).expect("resolved");
    let num = |v: &Option<Value>, t: &str, what: &str| {
        v.as_ref().map(|v| number(&store, v, t, what)).transpose()
    };
    // A pitched dormer matches the main roof's slope; a shed is a third of it.
    let main_slope = roofs::roof_recipe(roof).map_or(30f64.to_radians(), |r| r.slope);
    let default_slope = default_slope(p.style, main_slope);
    let guid = GlobalId::new();
    let mut recipe = DormerRecipe {
        kind: DORMER_RECIPE.into(),
        roof: roof.guid().to_string(),
        point: p.point,
        style: p.style,
        width: num(&p.width, "length", "the width")?.unwrap_or(1.8),
        front_height: num(&p.front_height, "length", "the front height")?.unwrap_or(0.9),
        slope: num(&p.slope, "angle", "the slope")?.unwrap_or(default_slope),
        overhang: num(&p.overhang, "length", "the overhang")?.unwrap_or(0.3),
        parts: Vec::new(),
    };
    let id = txn.create(
        NewElement::new(CLASS)
            .guid(guid.clone())
            .name("Dormer")
            .param("@ObjectType", "DORMER")
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    let parts = build(txn, guid.as_str(), &recipe)?;
    recipe.parts = parts
        .iter()
        .map(|p| txn.store().get(*p).expect("made").guid().to_string())
        .collect();
    write_recipe(txn, id, &recipe)?;
    Ok(id)
}

fn resolve_dormer(store: &Store, r: &ElementRef) -> Result<(ElementId, DormerRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such dormer"))?;
    let recipe = dormer_recipe(store.get(id).expect("resolved"))
        .ok_or_else(|| Error::new("dormer.not_found", format!("element {id} is not a dormer")))?;
    Ok((id, recipe))
}

/// Deletes a dormer's parts and builds them again from its recipe.
fn rebuild(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    mut r: DormerRecipe,
) -> Result<()> {
    let store = txn.store().clone();
    for g in &r.parts {
        if let Some(part) = GlobalId::parse(g).and_then(|g| store.find_by_guid(&g))
            && txn.store().contains(part)
        {
            txn.delete(part)?;
        }
    }
    let guid = store.get(id).expect("exists").guid().to_string();
    let parts = build(txn, &guid, &r)?;
    r.parts = parts
        .iter()
        .map(|p| txn.store().get(*p).expect("made").guid().to_string())
        .collect();
    write_recipe(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub dormer_id: ElementRef,
    pub style: Option<DormerStyle>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub front_height: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub slope: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub overhang: Option<Value>,
}

/// Changes a dormer's style, width, front height, slope or overhang; its parts are rebuilt.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve_dormer(&store, &p.dormer_id)?;
    let num = |v: &Option<Value>, t: &str, what: &str| {
        v.as_ref().map(|v| number(&store, v, t, what)).transpose()
    };
    if let Some(s) = p.style {
        // A shed starts at a shallow slope, a pitched dormer at 45°, unless one is given.
        if s != r.style && p.slope.is_none() {
            let main = GlobalId::parse(&r.roof)
                .and_then(|g| store.get(store.find_by_guid(&g)?))
                .and_then(roofs::roof_recipe)
                .map_or(30f64.to_radians(), |m| m.slope);
            r.slope = default_slope(s, main);
        }
        r.style = s;
    }
    if let Some(w) = num(&p.width, "length", "the width")? {
        r.width = w;
    }
    if let Some(h) = num(&p.front_height, "length", "the front height")? {
        r.front_height = h;
    }
    if let Some(s) = num(&p.slope, "angle", "the slope")? {
        r.slope = s;
    }
    if let Some(o) = num(&p.overhang, "length", "the overhang")? {
        r.overhang = o;
    }
    rebuild(txn, id, r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveParams {
    pub dormer_id: ElementRef,
    /// The new middle of the dormer's front (plan, metres).
    pub point: [f64; 2],
}

/// Moves a dormer on its roof; its parts are rebuilt there.
pub(crate) fn move_dormer(txn: &mut crate::txn::Transaction<'_>, p: MoveParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve_dormer(&store, &p.dormer_id)?;
    r.point = p.point;
    rebuild(txn, id, r)
}
