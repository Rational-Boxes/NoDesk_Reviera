// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Skylights (owner request, 2026-10-14; ARC-WIN-001's roof-hosted windows): glazed units set in a
//! roof, cutting their own opening.
//!
//! A skylight is an `IfcWindow` (`SKYLIGHT`) with a `recipe.skylight`: its roof (GlobalId), its
//! centre in plan, its style and size. Its style:
//! - **Roof window** (pitched roofs): a frame and glass lying in the roof's slope, its length up
//!   the slope and its width across it, standing proud of the roof's top.
//! - On flat roofs, on a curb of `curb` height round the opening:
//!   - **Flat**: a glass sheet on the curb;
//!   - **Pyramid**: four glass faces rising at `pitch` to an apex;
//!   - **Ridge lantern**: two glass slopes rising at `pitch` to a ridge along its length, with
//!     glazed gable ends. Its panes are re-mapped from its length: as many equal panes as keep
//!     each within `max_pane` (so lengthening it adds panes), with a glazing bar (rafter) between
//!     each two, a ridge bar and eaves bars.
//!
//! The opening is cut plumb through the roof under the skylight's outline (a roof window's
//! outline is its rectangle in the slope, seen from above). The skylight moves with its roof and
//! goes with it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::roofs::{RoofFace, RoofGeometry};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const SKYLIGHT_RECIPE: &str = "recipe.skylight";
/// A face this close to level is a flat roof (its normal's z).
const FLAT: f64 = 0.995;
const FRAME: f64 = 0.06;
const GLASS: f64 = 0.02;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkylightStyle {
    /// In the slope of a pitched roof.
    #[default]
    RoofWindow,
    /// A glass sheet on a curb (flat roofs).
    Flat,
    /// Four glass faces to an apex (flat roofs).
    Pyramid,
    /// Two pitched glass slopes to a ridge along its length, panes re-mapped from the length (flat
    /// roofs).
    RidgeLantern,
}

impl SkylightStyle {
    pub fn label(self) -> &'static str {
        match self {
            SkylightStyle::RoofWindow => "Roof Window",
            SkylightStyle::Flat => "Flat",
            SkylightStyle::Pyramid => "Pyramid",
            SkylightStyle::RidgeLantern => "Ridge Lantern",
        }
    }

    pub const ALL: [SkylightStyle; 4] = [
        SkylightStyle::RoofWindow,
        SkylightStyle::Flat,
        SkylightStyle::Pyramid,
        SkylightStyle::RidgeLantern,
    ];

    fn on_flat_roofs(self) -> bool {
        self != SkylightStyle::RoofWindow
    }
}

/// The stored recipe of a skylight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkylightRecipe {
    pub kind: String,
    /// The roof it is set in (GlobalId).
    pub roof: String,
    /// Its centre in plan (metres).
    pub center: [f64; 2],
    pub style: SkylightStyle,
    /// Across the slope (roof window), or its short side (flat roofs).
    pub width: f64,
    /// Up the slope, measured in the slope (roof window), or its long side.
    pub length: f64,
    /// Of its length from the x axis on a flat roof (radians); a roof window follows the slope.
    #[serde(default)]
    pub rotation: f64,
    /// The curb's height above a flat roof's top.
    #[serde(default = "default_curb")]
    pub curb: f64,
    /// The pyramid's or lantern's glass slope (radians).
    #[serde(default = "default_pitch")]
    pub pitch: f64,
    /// A lantern's widest pane along its length.
    #[serde(default = "default_pane")]
    pub max_pane: f64,
}

fn default_curb() -> f64 {
    0.15
}

fn default_pitch() -> f64 {
    30f64.to_radians()
}

fn default_pane() -> f64 {
    0.9
}

pub fn skylight_recipe(e: &Element) -> Option<SkylightRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == SKYLIGHT_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

type V3 = [f64; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: V3) -> V3 {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(f64::EPSILON);
    mul(a, 1.0 / l)
}

/// A box from `o` along three edges `a`, `b`, `c`, its faces pointing out (the first two edges
/// swap when the three are left-handed).
fn boxed(o: V3, a: V3, b: V3, c: V3) -> Mesh {
    let t = cross(a, b);
    let (a, b) = if t[0] * c[0] + t[1] * c[1] + t[2] * c[2] < 0.0 {
        (b, a)
    } else {
        (a, b)
    };
    let p = |i: f64, j: f64, k: f64| add(add(add(o, mul(a, i)), mul(b, j)), mul(c, k));
    let mut pts = vec![
        p(0.0, 0.0, 0.0),
        p(1.0, 0.0, 0.0),
        p(1.0, 1.0, 0.0),
        p(0.0, 1.0, 0.0),
    ];
    pts.extend([p(0.0, 0.0, 1.0), p(1.0, 0.0, 1.0), p(1.0, 1.0, 1.0), p(0.0, 1.0, 1.0)]);
    let idx = vec![
        0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0,
        4, 3, 4, 7,
    ];
    (pts, idx)
}

/// A thin slab under a planar polygon (corners counter-clockwise seen from its outside), `t` thick
/// along `-normal`.
fn sheet(corners: &[V3], t: f64) -> Mesh {
    let n = norm(cross(
        add(corners[1], mul(corners[0], -1.0)),
        add(corners[corners.len() - 1], mul(corners[0], -1.0)),
    ));
    let k = corners.len();
    let mut pts: Vec<V3> = corners.iter().map(|c| add(*c, mul(n, -t))).collect();
    pts.extend(corners.iter().copied());
    let mut idx = Vec::new();
    let k32 = k as u32;
    for i in 1..k32 - 1 {
        idx.extend([0, i + 1, i]);
        idx.extend([k32, k32 + i, k32 + i + 1]);
    }
    for i in 0..k32 {
        let j = (i + 1) % k32;
        idx.extend([i, j, k32 + j, i, k32 + j, k32 + i]);
    }
    (pts, idx)
}

/// The frame a skylight lies in: its centre on the roof's top, the unit vectors along its length
/// and width (in the slope for a roof window, level on a flat roof) and up out of it.
#[derive(Debug, Clone, Copy)]
struct Frame {
    o: V3,
    along: V3,
    across: V3,
    up: V3,
    flat: bool,
}

fn frame_on(r: &SkylightRecipe, face: &RoofFace, thickness: f64) -> Frame {
    let n = face.normal;
    let a = face.bottom[0];
    let p = r.center;
    let z_under = a[2] - (n[0] * (p[0] - a[0]) + n[1] * (p[1] - a[1])) / n[2];
    // The roof's top over the centre (thickness along the normal).
    let z_top = z_under + thickness / n[2];
    let flat = n[2] >= FLAT;
    if flat {
        let (s, c) = r.rotation.sin_cos();
        return Frame {
            o: [p[0], p[1], z_top],
            along: [c, s, 0.0],
            across: [-s, c, 0.0],
            up: [0.0, 0.0, 1.0],
            flat: true,
        };
    }
    // Up the slope: the steepest ascent in the face's plane.
    let horiz = norm([-n[0], -n[1], 0.0]);
    let along = norm(cross(cross(n, horiz), n));
    let along = if along[2] < 0.0 { mul(along, -1.0) } else { along };
    let across = norm(cross(n, along));
    Frame {
        o: [p[0], p[1], z_top],
        along,
        across,
        up: n,
        flat: false,
    }
}

/// The skylight's outline on the roof (corners in 3D, counter-clockwise from above).
fn outline(r: &SkylightRecipe, f: &Frame) -> [V3; 4] {
    let (hl, hw) = (r.length / 2.0, r.width / 2.0);
    let at = |i: f64, j: f64| add(add(f.o, mul(f.along, i * hl)), mul(f.across, j * hw));
    let c = [at(-1.0, -1.0), at(1.0, -1.0), at(1.0, 1.0), at(-1.0, 1.0)];
    // Counter-clockwise in plan.
    let area: f64 = (0..4)
        .map(|i| {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    if area < 0.0 {
        [c[0], c[3], c[2], c[1]]
    } else {
        c
    }
}

/// The plan hole a skylight cuts in roof geometry `g` (before its holes are cut), for
/// [`crate::slab_openings::roof_holes`].
pub fn hole(r: &SkylightRecipe, g: &RoofGeometry) -> Option<Vec<[f64; 2]>> {
    let face = g.face_at(r.center)?;
    let f = frame_on(r, face, g.thickness);
    Some(outline(r, &f).iter().map(|p| [p[0], p[1]]).collect())
}

/// The skylights set in roof `roof` (GlobalId).
pub fn of_roof(store: &Store, roof: &str) -> Vec<(ElementId, SkylightRecipe)> {
    store
        .iter()
        .filter_map(|e| Some((e.id(), skylight_recipe(e)?)))
        .filter(|(_, r)| r.roof == roof)
        .collect()
}

/// A skylight as it is now: its parts (glass, and frame or curb), and its lantern's panes.
#[derive(Debug, Clone, PartialEq)]
pub struct SkylightGeometry {
    /// Each part's mesh, and whether it is glass.
    pub parts: Vec<(Mesh, bool)>,
    /// A ridge lantern's panes along its length.
    pub panes: usize,
}

/// Regenerates a skylight on its roof as the roof is now.
pub fn regenerate(store: &Store, e: &Element) -> Result<SkylightGeometry> {
    let r = skylight_recipe(e)
        .ok_or_else(|| Error::new("skylight.not_reviera", "not a Reviera skylight"))?;
    let roof = find(store, &r.roof)
        .and_then(|id| store.get(id))
        .ok_or_else(|| Error::new("skylight.no_roof", "the skylight's roof no longer exists"))?;
    let g = crate::roofs::regenerate(store, roof)?;
    let face = g.face_at(r.center).ok_or_else(|| {
        Error::new("skylight.off_roof", "the skylight is not over its roof any more")
    })?;
    let f = frame_on(&r, face, g.thickness);
    if f.flat != r.style.on_flat_roofs() {
        return Err(Error::new(
            "skylight.wrong_roof",
            if f.flat {
                "a roof window goes in a pitched roof: on a flat roof use a flat, pyramid or ridge lantern skylight"
            } else {
                "a flat, pyramid or ridge lantern skylight goes on a flat roof: in a pitched roof use a roof window"
            },
        ));
    }
    Ok(build(&r, &f))
}

fn build(r: &SkylightRecipe, f: &Frame) -> SkylightGeometry {
    let (l, w) = (r.length, r.width);
    let corner = add(add(f.o, mul(f.along, -l / 2.0)), mul(f.across, -w / 2.0));
    let mut parts: Vec<(Mesh, bool)> = Vec::new();
    // The frame (roof window) or curb (flat roofs): four bars round the opening, outward.
    let (h, base) = if f.flat {
        (r.curb, 0.0)
    } else {
        (0.12, -0.02)
    };
    let o = add(corner, mul(f.up, base));
    let bars = [
        (o, mul(f.along, l), mul(f.across, FRAME)),
        (add(o, mul(f.across, w - FRAME)), mul(f.along, l), mul(f.across, FRAME)),
        (add(o, mul(f.across, FRAME)), mul(f.along, FRAME), mul(f.across, w - 2.0 * FRAME)),
        (
            add(add(o, mul(f.across, FRAME)), mul(f.along, l - FRAME)),
            mul(f.along, FRAME),
            mul(f.across, w - 2.0 * FRAME),
        ),
    ];
    for (p, a, b) in bars {
        parts.push((boxed(p, a, b, mul(f.up, h - base)), false));
    }
    let top = add(corner, mul(f.up, h));
    let at = |i: f64, j: f64, k: f64| add(add(add(top, mul(f.along, i)), mul(f.across, j)), mul(f.up, k));
    let mut panes = 1;
    match r.style {
        SkylightStyle::RoofWindow | SkylightStyle::Flat => {
            let g = [
                at(FRAME, FRAME, 0.0),
                at(l - FRAME, FRAME, 0.0),
                at(l - FRAME, w - FRAME, 0.0),
                at(FRAME, w - FRAME, 0.0),
            ];
            parts.push((sheet(&g, GLASS), true));
        }
        SkylightStyle::Pyramid => {
            let rise = r.pitch.tan() * w.min(l) / 2.0;
            let apex = at(l / 2.0, w / 2.0, rise);
            let c = [at(0.0, 0.0, 0.0), at(l, 0.0, 0.0), at(l, w, 0.0), at(0.0, w, 0.0)];
            for i in 0..4 {
                parts.push((sheet(&[c[i], c[(i + 1) % 4], apex], GLASS), true));
            }
        }
        SkylightStyle::RidgeLantern => {
            // Panes re-mapped from the length: equal, none wider than max_pane.
            panes = (l / r.max_pane.max(0.2) - 1e-9).ceil().max(1.0) as usize;
            let rise = r.pitch.tan() * w / 2.0;
            let ridge = |i: f64| at(i, w / 2.0, rise);
            for k in 0..panes {
                let (i0, i1) = (l * k as f64 / panes as f64, l * (k + 1) as f64 / panes as f64);
                // The two slopes of this bay.
                parts.push((sheet(&[at(i0, 0.0, 0.0), at(i1, 0.0, 0.0), ridge(i1), ridge(i0)], GLASS), true));
                parts.push((sheet(&[at(i1, w, 0.0), at(i0, w, 0.0), ridge(i0), ridge(i1)], GLASS), true));
            }
            // Glazed gable ends.
            parts.push((sheet(&[at(0.0, w, 0.0), at(0.0, 0.0, 0.0), ridge(0.0)], GLASS), true));
            parts.push((sheet(&[at(l, 0.0, 0.0), at(l, w, 0.0), ridge(l)], GLASS), true));
            // Glazing bars: rafters between the panes and at the ends, the ridge, the eaves.
            let bar = 0.04;
            let slope_len = (w / 2.0).hypot(rise);
            let down = norm(add(at(0.0, 0.0, 0.0), mul(ridge(0.0), -1.0)));
            let down2 = norm(add(at(0.0, w, 0.0), mul(ridge(0.0), -1.0)));
            for k in 0..=panes {
                let i = (l * k as f64 / panes as f64).clamp(bar / 2.0, l - bar / 2.0);
                let start = add(ridge(i), mul(f.along, -bar / 2.0));
                for d in [down, down2] {
                    parts.push((
                        boxed(start, mul(f.along, bar), mul(d, slope_len), mul(f.up, bar)),
                        false,
                    ));
                }
            }
            parts.push((
                boxed(add(ridge(0.0), mul(f.across, -bar / 2.0)), mul(f.along, l), mul(f.across, bar), mul(f.up, bar)),
                false,
            ));
        }
    }
    SkylightGeometry { parts, panes }
}

/// Every Reviera skylight that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, SkylightGeometry)> {
    store
        .iter()
        .filter(|e| skylight_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A Reviera footprint roof.
    pub roof_id: ElementRef,
    /// The centre in plan (metres).
    pub point: [f64; 2],
    /// Default: a roof window in a pitched roof, a ridge lantern on a flat one.
    pub style: Option<SkylightStyle>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub length: Option<f64>,
    /// On a flat roof, of its length from the x axis; accepts units ("90°").
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub curb: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub pitch: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub max_pane: Option<f64>,
}

fn check(r: &SkylightRecipe) -> Result<()> {
    if !(0.3..=20.0).contains(&r.width) || !(0.3..=60.0).contains(&r.length) {
        return Err(Error::new(
            "skylight.bad_size",
            "a skylight is 0.3 m to 20 m wide and 0.3 m to 60 m long",
        ));
    }
    if !(5f64.to_radians()..=60f64.to_radians()).contains(&r.pitch) {
        return Err(Error::new(
            "skylight.bad_pitch",
            "the glass slope must be between 5° and 60°",
        ));
    }
    if !(0.0..=1.0).contains(&r.curb) {
        return Err(Error::new("skylight.bad_curb", "the curb is 0 to 1 m high"));
    }
    Ok(())
}

fn resolve_roof(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such roof"))?;
    let e = store.get(id).expect("resolved");
    match crate::roofs::roof_recipe(e) {
        Some(rr) if rr.arch.is_none() && rr.dome.is_none() => Ok(id),
        Some(_) => Err(Error::new(
            "skylight.bad_roof",
            "skylights go in footprint roofs, not arched roofs or domes",
        )),
        None => Err(Error::new(
            "skylight.bad_roof",
            format!("element {id} is not a Reviera roof"),
        )),
    }
}

fn write_and_check(txn: &mut crate::txn::Transaction<'_>, id: ElementId, r: &SkylightRecipe) -> Result<()> {
    check(r)?;
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    let store = txn.store().clone();
    regenerate(&store, store.get(id).expect("exists"))?;
    // It must cut the roof (inside one face, clear of the other openings): no more openings are
    // left uncut than without it.
    let roof = find(&store, &r.roof).and_then(|i| store.get(i)).expect("checked");
    let with = crate::roofs::regenerate(&store, roof)?.uncut;
    let mut without = store.clone();
    without.remove(id);
    let before = crate::roofs::regenerate(&without, without.get(roof.id()).expect("exists"))?.uncut;
    if with > before {
        return Err(Error::new(
            "skylight.does_not_fit",
            "the skylight must lie within one roof face, clear of its other openings",
        ));
    }
    Ok(())
}

/// Places a skylight in a roof (`skylight.create`).
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let roof_id = resolve_roof(&store, &p.roof_id)?;
    let roof = store.get(roof_id).expect("resolved");
    let g = crate::roofs::regenerate(&store, roof)?;
    let face = g
        .face_at(p.point)
        .ok_or_else(|| Error::new("skylight.off_roof", "the point is not over the roof"))?;
    let flat = face.normal[2] >= FLAT;
    let style = p.style.unwrap_or(if flat {
        SkylightStyle::RidgeLantern
    } else {
        SkylightStyle::RoofWindow
    });
    let (dw, dl) = match style {
        SkylightStyle::RoofWindow => (0.78, 1.18),
        SkylightStyle::Flat | SkylightStyle::Pyramid => (1.2, 1.2),
        SkylightStyle::RidgeLantern => (1.5, 3.0),
    };
    let recipe = SkylightRecipe {
        kind: SKYLIGHT_RECIPE.into(),
        roof: roof.guid().to_string(),
        center: p.point,
        style,
        width: p.width.unwrap_or(dw),
        length: p.length.unwrap_or(dl),
        rotation: p.rotation.unwrap_or(0.0),
        curb: p.curb.unwrap_or_else(default_curb),
        pitch: p.pitch.unwrap_or_else(default_pitch),
        max_pane: p.max_pane.unwrap_or_else(default_pane),
    };
    let id = txn.create(
        NewElement::new("IfcWindow")
            .name(format!("Skylight ({})", style.label()))
            .param("@PredefinedType", "SKYLIGHT"),
    )?;
    write_and_check(txn, id, &recipe)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub skylight_ids: Vec<ElementRef>,
    pub point: Option<[f64; 2]>,
    pub style: Option<SkylightStyle>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub length: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub curb: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub pitch: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub max_pane: Option<f64>,
}

/// Changes skylights (`skylight.set`); omitted values are kept.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.skylight_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such skylight"))?;
        let mut s = skylight_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
            Error::new("skylight.not_reviera", format!("element {id} is not a Reviera skylight"))
        })?;
        crate::modify::check_unpinned(&store, id)?;
        if let Some(v) = p.point {
            s.center = v;
        }
        if let Some(v) = p.style {
            s.style = v;
        }
        if let Some(v) = p.width {
            s.width = v;
        }
        if let Some(v) = p.length {
            s.length = v;
        }
        if let Some(v) = p.rotation {
            s.rotation = v;
        }
        if let Some(v) = p.curb {
            s.curb = v;
        }
        if let Some(v) = p.pitch {
            s.pitch = v;
        }
        if let Some(v) = p.max_pane {
            s.max_pane = v;
        }
        write_and_check(txn, id, &s)?;
        let label = format!("Skylight ({})", s.style.label());
        txn.modify(id, |e| e.name = Some(label))?;
    }
    Ok(())
}

/// Moves (or turns) skylights: their centre by `x`, their rotation by `turn`.
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    turn: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut s = skylight_recipe(store.get(id).expect("exists")).expect("a skylight");
    s.center = x(s.center);
    s.rotation += turn;
    write_and_check(txn, id, &s)
}

/// Moves the skylights of roof `roof` (GlobalId) with it.
pub(crate) fn follow_roof(
    txn: &mut crate::txn::Transaction<'_>,
    roof: &str,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
) -> Result<()> {
    for (id, mut s) in of_roof(txn.store(), roof) {
        s.center = x(s.center);
        let value = serde_json::to_value(&s).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), value);
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lantern_re_maps_its_panes_from_its_length() {
        let r = |length: f64| SkylightRecipe {
            kind: SKYLIGHT_RECIPE.into(),
            roof: String::new(),
            center: [0.0, 0.0],
            style: SkylightStyle::RidgeLantern,
            width: 1.5,
            length,
            rotation: 0.0,
            curb: 0.15,
            pitch: default_pitch(),
            max_pane: 0.9,
        };
        let f = Frame {
            o: [0.0, 0.0, 3.0],
            along: [1.0, 0.0, 0.0],
            across: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, 1.0],
            flat: true,
        };
        assert_eq!(build(&r(3.0), &f).panes, 4);
        assert_eq!(build(&r(3.6), &f).panes, 4);
        assert_eq!(build(&r(3.7), &f).panes, 5);
        assert_eq!(build(&r(0.9), &f).panes, 1);
        // Glass: two slopes per pane and two gable ends.
        let g = build(&r(3.6), &f);
        assert_eq!(g.parts.iter().filter(|(_, glass)| *glass).count(), 4 * 2 + 2);
        // Every part is closed and faces out (a positive volume), in every style.
        for style in SkylightStyle::ALL {
            let mut s = r(2.0);
            s.style = style;
            for (m, _) in build(&s, &f).parts {
                let v: f64 = m
                    .1
                    .chunks(3)
                    .map(|t| {
                        let [a, b, c] = [m.0[t[0] as usize], m.0[t[1] as usize], m.0[t[2] as usize]];
                        (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                            + a[2] * (b[0] * c[1] - b[1] * c[0]))
                            / 6.0
                    })
                    .sum();
                assert!(v > 0.0, "{style:?}: {v}");
            }
        }
    }
}
