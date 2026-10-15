// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Roofs by extrusion (spec Ch 3 §3.6.5, ARC-ROOF-002): an open profile of lines and arcs,
//! sketched on a vertical work plane, is the roof's top; it is offset down by the roof's
//! thickness and extruded across the plane from *Extrusion Start* to *Extrusion End*.
//!
//! The profile is in the work plane's coordinates: along the plane, and up from the reference
//! level plus its offset, so the roof follows its level and its plane (a grid or a reference
//! plane moves it). It is the roof recipe's `extrusion`; the roof has no footprint sketch.
//!
//! The body is exact: the underside is the profile moved square to itself by the thickness
//! (mitred at corners), and the shell is its top, underside, ends and eaves. Each profile segment
//! that looks up is a roof face, which takes skylights and attached walls like any roof's.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{RoofGeometry, RoofRecipe};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::model_lines::LocalCurve;
use crate::store::Store;
use crate::walls::CurveArg;
use crate::work_planes::{self, PlaneArgs, WorkPlaneRef};

/// A roof by extrusion's profile and extent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Extrusion {
    /// The vertical work plane the profile is on.
    pub plane: WorkPlaneRef,
    /// The profile's curves, in order (each continues from the last), in plane coordinates: along
    /// the plane, and up from the reference level plus offset.
    pub profile: Vec<LocalCurve>,
    /// Extrusion Start and End, across the plane (metres, along its normal).
    pub start: f64,
    pub end: f64,
}

impl Extrusion {
    /// The profile sampled as one polyline (plane coordinates).
    fn points(&self) -> Vec<[f64; 2]> {
        let mut out: Vec<[f64; 2]> = Vec::new();
        for c in &self.profile {
            for p in c.points() {
                if out
                    .last()
                    .is_none_or(|q| (q[0] - p[0]).hypot(q[1] - p[1]) > 1e-9)
                {
                    out.push(p);
                }
            }
        }
        out
    }

    /// `IfcRoof.PredefinedType` from the profile's shape.
    fn predefined(&self) -> &'static str {
        if self.profile.iter().any(|c| !c.through.is_empty()) {
            return "BARREL_ROOF";
        }
        match self.profile.len() {
            1 => "SHED_ROOF",
            2 => "GABLE_ROOF",
            _ => "FREEFORM",
        }
    }
}

fn check(x: &Extrusion) -> Result<()> {
    if x.profile.is_empty() {
        return Err(Error::new("roof.no_profile", "the profile has no curves"));
    }
    for w in x.profile.windows(2) {
        if (w[0].end[0] - w[1].start[0]).hypot(w[0].end[1] - w[1].start[1]) > 1e-6 {
            return Err(Error::new(
                "roof.profile_open",
                "the profile's curves must follow on, each starting where the last ends",
            ));
        }
    }
    let pts = x.points();
    if pts.len() < 2 {
        return Err(Error::new("roof.no_profile", "the profile is too short"));
    }
    let first = pts[0];
    let last = pts[pts.len() - 1];
    if (first[0] - last[0]).hypot(first[1] - last[1]) < 1e-6 {
        return Err(Error::new(
            "roof.profile_closed",
            "a roof by extrusion's profile is open",
        ));
    }
    if (x.end - x.start).abs() < 0.01 {
        return Err(Error::new(
            "roof.no_depth",
            "Extrusion Start and End must differ",
        ));
    }
    Ok(())
}

/// The profile offset by `t` below itself (to its underside side), mitred at its corners: each
/// segment's line moved square to it, neighbours meeting where their moved lines cross.
fn offset_below(pts: &[[f64; 2]], t: f64) -> Vec<[f64; 2]> {
    let n = pts.len();
    // Each segment's unit normal, kept up.
    let normals: Vec<[f64; 2]> = pts
        .windows(2)
        .map(|w| {
            let d = [w[1][0] - w[0][0], w[1][1] - w[0][1]];
            let l = d[0].hypot(d[1]).max(1e-12);
            let mut nn = [-d[1] / l, d[0] / l];
            if nn[1] < 0.0 || (nn[1] == 0.0 && nn[0] < 0.0) {
                nn = [-nn[0], -nn[1]];
            }
            nn
        })
        .collect();
    (0..n)
        .map(|i| {
            let p = pts[i];
            let at = |k: usize| [p[0] - normals[k][0] * t, p[1] - normals[k][1] * t];
            if i == 0 {
                return at(0);
            }
            if i == n - 1 {
                return at(n - 2);
            }
            // The mitre: along the bisector, t / cos(half the turn).
            let (a, b) = (normals[i - 1], normals[i]);
            let m = [a[0] + b[0], a[1] + b[1]];
            let ml = m[0].hypot(m[1]);
            if ml < 1e-9 {
                return at(i);
            }
            let m = [m[0] / ml, m[1] / ml];
            let cos = (m[0] * a[0] + m[1] * a[1]).max(0.2);
            [p[0] - m[0] * t / cos, p[1] - m[1] * t / cos]
        })
        .collect()
}

/// A roof by extrusion as it is now: `base` is the reference level's elevation plus its offset.
pub fn geometry(store: &Store, x: &Extrusion, base: f64, thickness: f64) -> Result<RoofGeometry> {
    check(x)?;
    let plane = work_planes::resolve(store, &x.plane)?;
    if !plane.is_vertical() {
        return Err(Error::new(
            "roof.plane_not_vertical",
            "a roof by extrusion is sketched on a vertical work plane",
        ));
    }
    let top = x.points();
    let under = offset_below(&top, thickness);
    let n = top.len();
    // Plane coordinates to world: along the plane's x, up from the base, across by `depth`.
    let world = |p: [f64; 2], depth: f64| {
        let w = plane.to_world([p[0], 0.0]);
        [
            w[0] + plane.normal[0] * depth,
            w[1] + plane.normal[1] * depth,
            base + p[1],
        ]
    };
    // Points: underside at start, underside at end, top at start, top at end.
    let mut positions: Vec<[f64; 3]> = Vec::with_capacity(4 * n);
    for (ring, depth) in [
        (&under, x.start),
        (&under, x.end),
        (&top, x.start),
        (&top, x.end),
    ] {
        positions.extend(ring.iter().map(|p| world(*p, depth)));
    }
    let (us, ue, ts, te) = (0u32, n as u32, 2 * n as u32, 3 * n as u32);
    let mut indices: Vec<u32> = Vec::new();
    let mut faces = Vec::new();
    // A triangle wound to face away from `inside`.
    let tri = |indices: &mut Vec<u32>, a: u32, b: u32, c: u32, out: [f64; 3]| {
        let (pa, pb, pc) = (
            positions[a as usize],
            positions[b as usize],
            positions[c as usize],
        );
        let nrm = work_planes::cross(work_planes::sub(pb, pa), work_planes::sub(pc, pa));
        if work_planes::dot(nrm, out) >= 0.0 {
            indices.extend([a, b, c]);
        } else {
            indices.extend([a, c, b]);
        }
    };
    let across = plane.normal;
    let towards = if x.end >= x.start {
        across
    } else {
        work_planes::scale(across, -1.0)
    };
    for k in 0..n as u32 - 1 {
        let j = k + 1;
        // The segment's upward normal in the world.
        let a = world(top[k as usize], x.start);
        let b = world(top[j as usize], x.start);
        let c = world(top[k as usize], x.end);
        let mut up = work_planes::cross(work_planes::sub(b, a), work_planes::sub(c, a));
        if up[2] < 0.0
            || (up[2] == 0.0
                && work_planes::dot(up, work_planes::sub(a, world(under[k as usize], x.start)))
                    < 0.0)
        {
            up = work_planes::scale(up, -1.0);
        }
        let up = work_planes::unit(up).unwrap_or([0.0, 0.0, 1.0]);
        let down = work_planes::scale(up, -1.0);
        tri(&mut indices, us + k, us + j, ue + j, down);
        tri(&mut indices, us + k, ue + j, ue + k, down);
        tri(&mut indices, ts + k, ts + j, te + j, up);
        tri(&mut indices, ts + k, te + j, te + k, up);
        // End caps of this segment's cross-section (a quad at each end).
        let back = work_planes::scale(towards, -1.0);
        tri(&mut indices, us + k, us + j, ts + j, back);
        tri(&mut indices, us + k, ts + j, ts + k, back);
        tri(&mut indices, ue + k, ue + j, te + j, towards);
        tri(&mut indices, ue + k, te + j, te + k, towards);
        if up[2] > 1e-9 {
            let bottom = vec![
                positions[(us + k) as usize],
                positions[(us + j) as usize],
                positions[(ue + j) as usize],
                positions[(ue + k) as usize],
            ];
            // Counter-clockwise from above.
            let flat: Vec<[f64; 2]> = bottom.iter().map(|q| [q[0], q[1]]).collect();
            let bottom = if crate::sketch::signed_area(&flat) < 0.0 {
                bottom.into_iter().rev().collect()
            } else {
                bottom
            };
            faces.push(super::RoofFace {
                key: None,
                bottom,
                normal: up,
                slab: (Vec::new(), Vec::new()),
                holes: Vec::new(),
                faceted: true,
            });
        }
    }
    // The strips closing the profile's two ends (eaves).
    for (k, inward) in [(0u32, 1i64), (n as u32 - 1, -1)] {
        let next = (k as i64 + inward) as usize;
        let out2 = [
            top[k as usize][0] - top[next][0],
            top[k as usize][1] - top[next][1],
        ];
        let out = work_planes::sub(world(out2, 0.0), world([0.0, 0.0], 0.0));
        tri(&mut indices, us + k, ue + k, te + k, out);
        tri(&mut indices, us + k, te + k, ts + k, out);
    }
    if let Some(f) = faces.first_mut() {
        f.slab = (positions, indices);
    }
    let corner = |p: [f64; 2], d: f64| {
        let w = world(p, d);
        [w[0], w[1]]
    };
    let footprint = vec![
        corner(top[0], x.start),
        corner(top[n - 1], x.start),
        corner(top[n - 1], x.end),
        corner(top[0], x.end),
    ];
    let keys = (1..=footprint.len() as u32).collect();
    Ok(RoofGeometry {
        faces,
        plate: base,
        thickness,
        footprint,
        keys,
        predefined: x.predefined(),
        trims: Vec::new(),
        uncut: 0,
    })
}

/// The default extent across the plane: over the walls under the profile, else 0 to 6 m.
fn default_extent(store: &Store, plane: &work_planes::Plane, profile: &[[f64; 2]]) -> (f64, f64) {
    let (lo_u, hi_u) = profile
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p[0]), b.max(p[0]))
        });
    let mut across: Option<(f64, f64)> = None;
    for e in store.iter() {
        let Some(r) = crate::walls::wall_recipe(e) else {
            continue;
        };
        for p in [r.curve.start, r.curve.end] {
            let l = plane.to_local([p[0], p[1], plane.origin[2]]);
            if l[0] < lo_u - 1e-6 || l[0] > hi_u + 1e-6 {
                continue;
            }
            let d = work_planes::dot(
                work_planes::sub([p[0], p[1], plane.origin[2]], plane.origin),
                plane.normal,
            );
            across = Some(across.map_or((d, d), |(a, b)| (a.min(d), b.max(d))));
        }
    }
    match across {
        Some((a, b)) if b - a > 0.01 => (a, b),
        _ => (0.0, 6.0),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A roof type; the default one if omitted.
    pub type_id: Option<ElementRef>,
    /// The vertical work plane: one of `grid`, `ref_plane` or `plane`.
    #[serde(flatten)]
    pub plane: PlaneArgs,
    /// The open profile, in plane coordinates (along the plane; up from the reference level plus
    /// `level_offset`); `through` points make arcs.
    pub profile: Vec<CurveArg>,
    /// The Reference Level; the highest level if omitted.
    pub ref_level: Option<ElementRef>,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub level_offset: f64,
    /// Extrusion Start and End across the plane (metres); by default over the walls under the
    /// profile, else 0 to 6 m.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub start: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub end: Option<f64>,
}

fn curves(p: &[CurveArg]) -> Vec<LocalCurve> {
    p.iter()
        .map(|c| LocalCurve {
            start: c.line.start,
            end: c.line.end,
            through: c.through.clone(),
        })
        .collect()
}

/// `roof.create_extrusion`.
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let plane_ref = p.plane.to_ref(&store)?;
    let plane = work_planes::resolve(&store, &plane_ref)?;
    if !plane.is_vertical() {
        return Err(Error::new(
            "roof.plane_not_vertical",
            "a roof by extrusion is sketched on a vertical work plane (a grid or a reference plane)",
        ));
    }
    let level = match &p.ref_level {
        Some(r) => super::resolve_level(&store, r)?,
        None => crate::levels::list(&store)
            .last()
            .map(|l| l.guid.clone())
            .ok_or_else(|| Error::new("level.none", "the model has no level"))?,
    };
    let profile = curves(&p.profile);
    let pts: Vec<[f64; 2]> = profile.iter().flat_map(LocalCurve::points).collect();
    let (ds, de) = default_extent(&store, &plane, &pts);
    let x = Extrusion {
        plane: plane_ref,
        profile,
        start: p.start.unwrap_or(ds),
        end: p.end.unwrap_or(de),
    };
    check(&x)?;
    let type_id = match &p.type_id {
        Some(r) => store
            .resolve(r)
            .filter(|id| store.get(*id).is_some_and(super::is_roof_type))
            .ok_or_else(|| Error::new("type.wrong_family", "not a roof type"))?,
        None => super::default_type(txn)?,
    };
    let recipe = RoofRecipe {
        kind: super::ROOF_RECIPE.into(),
        level,
        base_offset: p.level_offset,
        slope: super::DEFAULT_SLOPE,
        edges: Default::default(),
        cutoff: None,
        arch: None,
        dome: None,
        extrusion: Some(x.clone()),
    };
    let type_guid = txn.store().get(type_id).expect("exists").guid().to_string();
    let id = txn.create(
        crate::element::NewElement::new(super::CLASS)
            .param("@PredefinedType", x.predefined())
            .param("@type", type_guid)
            .param(
                crate::walls::RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    super::check(txn, id)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub roof_id: ElementRef,
    pub profile: Option<Vec<CurveArg>>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub start: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub end: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub level_offset: Option<f64>,
}

/// `roof.set_extrusion`.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = super::resolve_roof(&store, &p.roof_id)?;
    let Some(x) = &mut r.extrusion else {
        return Err(Error::new(
            "roof.not_extrusion",
            "the roof is not a roof by extrusion",
        ));
    };
    if let Some(pr) = &p.profile {
        x.profile = curves(pr);
    }
    if let Some(s) = p.start {
        x.start = s;
    }
    if let Some(e) = p.end {
        x.end = e;
    }
    check(x)?;
    let predefined = x.predefined();
    if let Some(o) = p.level_offset {
        r.base_offset = o;
    }
    let value = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(crate::walls::RECIPE_KEY.into(), value);
        e.params.insert("@PredefinedType".into(), predefined.into());
    })?;
    super::check(txn, id)
}

/// Moves, turns or mirrors a roof by extrusion: its plane is fixed where it is moved to.
pub(crate) fn transform(
    store: &Store,
    r: &mut RoofRecipe,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<()> {
    if let Some(x) = &mut r.extrusion {
        x.plane = work_planes::transform_frame(store, &x.plane, point, 0.0)?;
    }
    r.base_offset += dz;
    Ok(())
}
