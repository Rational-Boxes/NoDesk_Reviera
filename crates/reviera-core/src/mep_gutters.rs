// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Roof drainage: gutters, outlets and downspouts (MEP-PIPE-010), part of the plumbing
//! subsystem. The eave gives the line; the gutter is a run of a *Storm Drainage* system.
//!
//! - A **gutter** (`IfcPipeSegment`, GUTTER, `recipe.gutter`) names its roof and the eave curves
//!   it runs along. Its line is found afresh from the roof each time: each eave's outer edge (the
//!   lowest edge of the face rising from it), chained corner to corner, mitred, and set out from
//!   the eave by the fascia's thickness (when the eave has one) and half the gutter's width, its
//!   top just under the roof. So it moves when the overhang, fascia or bearing changes. It falls
//!   (1:500 by default) toward its outlets: highest midway between them (or at its ends), lowest
//!   at each outlet. Its body is its profile (half-round, K-style or box, a thin shell) swept
//!   along the line, mitred at corners, closed at its ends.
//! - An **outlet** is a point along the gutter: a port (down, of the downspout's diameter) and
//!   an `IfcPipeFitting` (CONNECTOR) under it. A **downspout** is an ordinary pipe run from the
//!   outlet: down a little, back toward the wall by the eave's offset (with bends), then down to
//!   a level or a point. Gutter, outlets and downspouts join one storm system.
//! - **Sizing** (`gutter.size`): the plan area of the roof faces draining to the gutter is shared
//!   between its outlets by the length of gutter nearest each; at a design rainfall intensity
//!   (75 mm/h by default) each outlet's flow is checked against the gutter's capacity (Manning,
//!   flowing full, n = 0.012, at its fall) and the downspout's (an orifice at the outlet under
//!   half the gutter's depth), flagged when too small.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{self, Classification, Connector, Domain, FlowDirection, Port, Shape};
use crate::mep_runs::Solid;
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const GUTTER: &str = "recipe.gutter";
type V3 = [f64; 3];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    #[default]
    HalfRound,
    KStyle,
    Box,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Outlet {
    /// Metres along the gutter from its start.
    pub at: f64,
    /// The downspout's diameter (metres).
    pub diameter: f64,
}

/// The stored recipe of a gutter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GutterRecipe {
    pub kind: String,
    /// Its roof (GlobalId).
    pub roof: String,
    /// The eave curves it runs along, in order.
    pub keys: Vec<u32>,
    #[serde(default)]
    pub profile: Profile,
    /// Metres.
    pub width: f64,
    pub depth: f64,
    /// Fall toward the outlets (rise over run).
    pub fall: f64,
    #[serde(default)]
    pub outlets: Vec<Outlet>,
    /// The roof's level (GlobalId).
    pub level: String,
}

pub fn gutter_recipe(e: &Element) -> Option<GutterRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == GUTTER).then(|| serde_json::from_value(v.clone()).ok())?
}

/// Shell thickness of the profile.
const SHELL: f64 = 0.0015;

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// A gutter as it is now.
#[derive(Debug, Clone)]
pub struct GutterGeometry {
    /// The top centre line at each break (corners, outlets, high points, ends).
    pub line: Vec<V3>,
    /// The outward mitre at each break (across the gutter, scaled at corners).
    pub across: Vec<[f64; 2]>,
    /// Distance along at each break.
    pub along: Vec<f64>,
    pub length: f64,
    pub solids: Vec<Solid>,
    /// Each outlet's bottom point.
    pub outlets: Vec<V3>,
    /// How far the gutter's centre lies outside the wall's face (for downspouts).
    pub offset_from_wall: f64,
}

/// The profile as paired outer and inner points (across, up) from the top centre.
fn profile_points(p: Profile, w: f64, d: f64) -> Vec<([f64; 2], [f64; 2])> {
    let outer: Vec<[f64; 2]> = match p {
        Profile::HalfRound => (0..=12)
            .map(|k| {
                let t = std::f64::consts::PI * (1.0 + k as f64 / 12.0);
                [w / 2.0 * t.cos(), d * t.sin()]
            })
            .collect(),
        Profile::Box => vec![[-w / 2.0, 0.0], [-w / 2.0, -d], [w / 2.0, -d], [w / 2.0, 0.0]],
        // An ogee front: back, bottom, then the front bulging out and up.
        Profile::KStyle => vec![[-w / 2.0, 0.0], [-w / 2.0, -d], [w * 0.25, -d], [w * 0.4, -0.75 * d], [w * 0.38, -0.45 * d], [w / 2.0, -0.2 * d], [w / 2.0, 0.0]],
    };
    let (sx, sy) = ((w - 2.0 * SHELL) / w, (d - SHELL) / d);
    outer.into_iter().map(|o| (o, [o[0] * sx, o[1] * sy])).collect()
}

/// The eave edge of a face: its lowest edge, as it runs with the face to its left (so outward
/// is to its right).
fn eave_edge(face: &crate::roofs::RoofFace) -> Option<(V3, V3)> {
    let b = &face.bottom;
    let n = b.len();
    (0..n)
        .map(|k| (b[k], b[(k + 1) % n]))
        .filter(|(a, c)| (a[0] - c[0]).hypot(a[1] - c[1]) > 0.05)
        .min_by(|x, y| (x.0[2] + x.1[2]).total_cmp(&(y.0[2] + y.1[2])))
}

/// The gutter's geometry from its roof.
pub fn geometry(store: &Store, r: &GutterRecipe) -> Result<GutterGeometry> {
    let roof = GlobalId::parse(&r.roof).and_then(|g| store.find_by_guid(&g)).ok_or_else(|| bad("gutter.no_roof", "the gutter's roof is gone"))?;
    let re = store.get(roof).expect("found");
    let rr = crate::roofs::roof_recipe(re).ok_or_else(|| bad("gutter.no_roof", "the gutter's host is not a roof"))?;
    let g = crate::roofs::regenerate(store, re)?;
    // Each eave's line, with what it hangs on.
    let mut lines: Vec<(V3, V3, f64, f64)> = Vec::new();
    for k in &r.keys {
        let face = g.faces.iter().find(|f| f.key == Some(*k)).ok_or_else(|| bad("gutter.edge", format!("curve {k} of the roof has no eave")))?;
        let (a, b) = eave_edge(face).ok_or_else(|| bad("gutter.edge", format!("curve {k} has no eave edge")))?;
        let edge = rr.edges.get(&k.to_string());
        let fascia = edge.and_then(|e| e.eave.as_ref()).is_some_and(|e| e.fascia);
        let overhang = edge.map_or(0.0, |e| e.overhang);
        lines.push((a, b, if fascia { crate::roofs::eaves::FASCIA } else { 0.0 }, overhang));
    }
    // Chain them corner to corner, in the order given.
    for w in lines.windows(2) {
        let gap = (w[0].1[0] - w[1].0[0]).hypot(w[0].1[1] - w[1].0[1]);
        if gap > 0.05 {
            return Err(bad("gutter.not_contiguous", "the eaves must follow one another around the roof (pick them in order)"));
        }
    }
    let off = |k: usize| lines[k].2 + r.width / 2.0;
    let dir = |k: usize| {
        let d = sub(lines[k].1, lines[k].0);
        let l = d[0].hypot(d[1]);
        [d[0] / l, d[1] / l]
    };
    let right = |d: [f64; 2]| [d[1], -d[0]];
    let mut corners: Vec<[f64; 2]> = Vec::new();
    let mut across: Vec<[f64; 2]> = Vec::new();
    let n = lines.len();
    // Round the whole roof, the ends meet at a mitre too.
    let closed = n > 2 && (lines[n - 1].1[0] - lines[0].0[0]).hypot(lines[n - 1].1[1] - lines[0].0[1]) < 0.05;
    let mitre = |a: usize, b: usize| {
        let (n0, n1) = (right(dir(a)), right(dir(b)));
        let o = (off(a) + off(b)) / 2.0;
        let s = o / (1.0 + n0[0] * n1[0] + n0[1] * n1[1]).max(1e-6);
        [(n0[0] + n1[0]) * s, (n0[1] + n1[1]) * s]
    };
    for k in 0..=n {
        let (pt, m) = if closed && (k == 0 || k == n) {
            ([lines[0].0[0], lines[0].0[1]], mitre(n - 1, 0))
        } else if k == 0 {
            let nn = right(dir(0));
            ([lines[0].0[0], lines[0].0[1]], [nn[0] * off(0), nn[1] * off(0)])
        } else if k == n {
            let nn = right(dir(n - 1));
            ([lines[n - 1].1[0], lines[n - 1].1[1]], [nn[0] * off(n - 1), nn[1] * off(n - 1)])
        } else {
            let (n0, n1) = (right(dir(k - 1)), right(dir(k)));
            let o = (off(k - 1) + off(k)) / 2.0;
            let s = o / (1.0 + n0[0] * n1[0] + n0[1] * n1[1]).max(1e-6);
            ([lines[k].0[0], lines[k].0[1]], [(n0[0] + n1[0]) * s, (n0[1] + n1[1]) * s])
        };
        corners.push([pt[0] + m[0], pt[1] + m[1]]);
        across.push(m);
    }
    let top = lines.iter().map(|l| l.0[2].min(l.1[2])).fold(f64::INFINITY, f64::min) - 0.02;
    // Distance along at each corner.
    let mut cs = vec![0.0];
    for w in corners.windows(2) {
        cs.push(cs.last().copied().unwrap_or(0.0) + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
    }
    let length = *cs.last().expect("corners");
    // Breaks: corners, outlets and the high points between them.
    let outs: Vec<f64> = r.outlets.iter().map(|o| o.at.clamp(0.0, length)).collect();
    let mut breaks: Vec<f64> = cs.clone();
    breaks.extend(outs.iter().copied());
    let mut sorted = outs.clone();
    sorted.sort_by(f64::total_cmp);
    for w in sorted.windows(2) {
        breaks.push((w[0] + w[1]) / 2.0);
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let d_near = |s: f64| outs.iter().map(|o| (s - o).abs()).fold(f64::INFINITY, f64::min);
    let dmax = breaks.iter().map(|s| d_near(*s)).filter(|d| d.is_finite()).fold(0.0, f64::max);
    let z_at = |s: f64| if outs.is_empty() { top } else { top - r.fall * (dmax - d_near(s)) };
    // A point and its mitre at a distance along.
    let at = |s: f64| -> ([f64; 2], [f64; 2]) {
        let k = (1..cs.len()).find(|k| s <= cs[*k] + 1e-9).unwrap_or(cs.len() - 1);
        let t = ((s - cs[k - 1]) / (cs[k] - cs[k - 1]).max(1e-12)).clamp(0.0, 1.0);
        let p = [corners[k - 1][0] + (corners[k][0] - corners[k - 1][0]) * t, corners[k - 1][1] + (corners[k][1] - corners[k - 1][1]) * t];
        // Inside a leg the across is the leg's own; at a corner, the mitre.
        let m = if t < 1e-9 { across[k - 1] } else if t > 1.0 - 1e-9 { across[k] } else {
            let nn = right(dir(k - 1));
            [nn[0] * off(k - 1), nn[1] * off(k - 1)]
        };
        (p, m)
    };
    let mut line = Vec::new();
    let mut acr = Vec::new();
    for s in &breaks {
        let (p, m) = at(*s);
        line.push([p[0], p[1], z_at(*s)]);
        // The across direction, longer at a mitre by the corner's factor.
        let o = ((off(0) + off(n - 1)) / 2.0).max(1e-9);
        acr.push([m[0] / o, m[1] / o]);
    }
    // The body: rings of the profile at each break, joined.
    let prof = profile_points(r.profile, r.width, r.depth);
    let m = prof.len();
    let mut points: Vec<V3> = Vec::new();
    for (c, a) in line.iter().zip(&acr) {
        for (o, i) in &prof {
            points.push([c[0] + a[0] * o[0], c[1] + a[1] * o[0], c[2] + o[1]]);
            points.push([c[0] + a[0] * i[0], c[1] + a[1] * i[0], c[2] + i[1]]);
        }
    }
    // Index of the outer (0) or inner (1) point `j` of ring `r`.
    let idx = |ring: usize, j: usize, inner: usize| ring * 2 * m + 2 * j + inner;
    let mut faces = Vec::new();
    for ring in 0..line.len().saturating_sub(1) {
        for j in 0..m - 1 {
            faces.push(vec![idx(ring, j, 0), idx(ring + 1, j, 0), idx(ring + 1, j + 1, 0), idx(ring, j + 1, 0)]);
            faces.push(vec![idx(ring, j, 1), idx(ring, j + 1, 1), idx(ring + 1, j + 1, 1), idx(ring + 1, j, 1)]);
        }
        // The lips.
        for j in [0, m - 1] {
            faces.push(vec![idx(ring, j, 0), idx(ring, j, 1), idx(ring + 1, j, 1), idx(ring + 1, j, 0)]);
        }
    }
    // End caps.
    for (ring, flip) in [(0, false), (line.len() - 1, true)] {
        for j in 0..m - 1 {
            let mut f = vec![idx(ring, j, 0), idx(ring, j + 1, 0), idx(ring, j + 1, 1), idx(ring, j, 1)];
            if flip {
                f.reverse();
            }
            faces.push(f);
        }
    }
    let outlets = outs
        .iter()
        .map(|s| {
            let (p, _) = at(*s);
            [p[0], p[1], z_at(*s) - r.depth]
        })
        .collect();
    let offset_from_wall = lines.iter().map(|l| l.3 + l.2 + r.width / 2.0).fold(0.0, f64::max);
    Ok(GutterGeometry { line, across: acr, along: breaks, length, solids: vec![Solid { points, faces }], outlets, offset_from_wall })
}

/// A gutter's ports: one per outlet, down, Storm Drainage.
pub fn ports(store: &Store, e: &Element) -> Vec<Port> {
    let Some(r) = gutter_recipe(e) else { return Vec::new() };
    let Ok(g) = geometry(store, &r) else { return Vec::new() };
    g.outlets
        .iter()
        .zip(&r.outlets)
        .enumerate()
        .map(|(k, (p, o))| Port {
            element: e.id(),
            index: k,
            connector: Connector {
                name: format!("Outlet {}", k + 1),
                classification: Some(Classification::StormDrainage),
                flow_direction: FlowDirection::Out,
                shape: Shape::Round,
                diameter: o.diameter,
                ..mep::new_connector(Domain::Pipe)
            },
            position: *p,
            direction: [0.0, 0.0, -1.0],
        })
        .collect()
}

/// Every gutter with its solids.
pub fn all(store: &Store) -> Vec<(ElementId, Vec<Solid>)> {
    store
        .iter()
        .filter_map(|e| {
            let r = gutter_recipe(e)?;
            Some((e.id(), geometry(store, &r).ok()?.solids))
        })
        .collect()
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub roof_id: ElementRef,
    /// The eave curves (keys from `roof.eaves`), in order around the roof; every eave when empty.
    #[serde(default)]
    pub edges: Vec<u32>,
    #[serde(default)]
    pub profile: Profile,
    /// Metres; 125 mm by default.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    /// Metres; half the width by default.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub depth: Option<f64>,
    /// Rise over run toward the outlets; 1:500 (0.002) by default.
    pub fall: Option<f64>,
}

/// `gutter.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let roof = store.resolve(&p.roof_id).ok_or_else(|| bad("element.not_found", "no such roof"))?;
    let re = store.get(roof).expect("resolved");
    let rr = crate::roofs::roof_recipe(re).ok_or_else(|| bad("gutter.not_a_roof", format!("element {roof} is not a Reviera roof")))?;
    let mut keys = p.edges.clone();
    if keys.is_empty() {
        // Every eave, in footprint order.
        let g = crate::roofs::regenerate(&store, re)?;
        keys = g.keys.iter().copied().filter(|k| rr.edges.get(&k.to_string()).is_some_and(|e| e.eave.is_some() || e.defines_slope)).collect();
        if keys.is_empty() {
            return Err(bad("gutter.edge", "the roof has no eaves"));
        }
    }
    let width = p.width.unwrap_or(0.125);
    let depth = p.depth.unwrap_or(width / 2.0);
    let fall = p.fall.unwrap_or(0.002);
    if !(0.03..=0.6).contains(&width) || !(0.02..=0.4).contains(&depth) || !(0.0..=0.05).contains(&fall) {
        return Err(bad("gutter.size", "a gutter is 30–600 mm wide, 20–400 mm deep, falling 0 to 1:20"));
    }
    let r = GutterRecipe { kind: GUTTER.into(), roof: re.guid().to_string(), keys, profile: p.profile, width, depth, fall, outlets: Vec::new(), level: rr.level.clone() };
    geometry(&store, &r)?;
    txn.create(
        NewElement::new("IfcPipeSegment")
            .name("Gutter")
            .param("@PredefinedType", "GUTTER")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &GutterRecipe) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, GutterRecipe)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such gutter"))?;
    let g = store.get(id).and_then(gutter_recipe).ok_or_else(|| bad("gutter.not_a_gutter", format!("element {id} is not a gutter")))?;
    Ok((id, g))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub gutter_id: ElementRef,
    pub profile: Option<Profile>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub depth: Option<f64>,
    pub fall: Option<f64>,
}

/// `gutter.set`.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.gutter_id)?;
    if let Some(v) = p.profile {
        r.profile = v;
    }
    if let Some(v) = p.width {
        r.width = v;
    }
    if let Some(v) = p.depth {
        r.depth = v;
    }
    if let Some(v) = p.fall {
        r.fall = v;
    }
    if !(0.03..=0.6).contains(&r.width) || !(0.02..=0.4).contains(&r.depth) || !(0.0..=0.05).contains(&r.fall) {
        return Err(bad("gutter.size", "a gutter is 30–600 mm wide, 20–400 mm deep, falling 0 to 1:20"));
    }
    write(txn, id, &r)
}

/// Where a downspout ends.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum DownspoutTo {
    /// A level's elevation.
    Level(ElementRef),
    /// A point `[x, y, z]` (absolute elevation): the downspout ends above it at that height.
    Point(V3),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutletParams {
    pub gutter_id: ElementRef,
    /// Metres along the gutter from its start; or give `point`.
    pub at: Option<f64>,
    /// A plan point `[x, y]`: the outlet goes at the nearest point of the gutter.
    pub point: Option<[f64; 2]>,
    /// The downspout's diameter (metres); 68 mm by default.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub diameter: Option<f64>,
    /// Where its downspout goes; none for an outlet only.
    pub downspout_to: Option<DownspoutTo>,
}

/// What adding an outlet made.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OutletReport {
    /// Metres along the gutter.
    pub at: f64,
    pub fitting: ElementId,
    pub downspout: Vec<ElementId>,
    /// The storm system the gutter is in.
    pub system: ElementId,
}

/// `gutter.add_outlet`.
pub(crate) fn add_outlet(txn: &mut Transaction<'_>, p: OutletParams) -> Result<OutletReport> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.gutter_id)?;
    let g = geometry(&store, &r)?;
    let at = match (p.at, p.point) {
        (Some(a), _) => a,
        (None, Some(q)) => {
            // The nearest point of the gutter's line.
            let mut best = (f64::INFINITY, 0.0);
            for k in 1..g.line.len() {
                let (a, b) = (g.line[k - 1], g.line[k]);
                let d = [b[0] - a[0], b[1] - a[1]];
                let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
                let t = (((q[0] - a[0]) * d[0] + (q[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
                let dist = (a[0] + d[0] * t - q[0]).hypot(a[1] + d[1] * t - q[1]);
                if dist < best.0 {
                    best = (dist, g.along[k - 1] + (g.along[k] - g.along[k - 1]) * t);
                }
            }
            best.1
        }
        (None, None) => g.length / 2.0,
    };
    if !(0.0..=g.length + 1e-9).contains(&at) {
        return Err(bad("gutter.outlet", format!("an outlet goes along the gutter, 0 to {:.2} m", g.length)));
    }
    let diameter = p.diameter.unwrap_or(0.068);
    if !(0.03..=0.3).contains(&diameter) || diameter > r.width - 0.01 {
        return Err(bad("gutter.outlet", "a downspout is 30–300 mm, narrower than its gutter"));
    }
    if r.outlets.iter().any(|o| (o.at - at).abs() < diameter) {
        return Err(bad("gutter.outlet", "there is already an outlet there"));
    }
    r.outlets.push(Outlet { at, diameter });
    let index = r.outlets.len() - 1;
    write(txn, id, &r)?;
    let store = txn.store().clone();
    let g = geometry(&store, &r)?;
    let bottom = g.outlets[index];
    // The outlet fitting under the gutter, then the downspout from it.
    let level = r.level.clone();
    let fitting = crate::mep_runs::make_outlet(txn, &level, bottom, diameter)?;
    let tail = [bottom[0], bottom[1], bottom[2] - crate::mep_runs::OUTLET_LENGTH];
    let mut downspout = Vec::new();
    if let Some(to) = p.downspout_to {
        let (target_z, target_xy, level_ref) = match to {
            DownspoutTo::Level(l) => {
                let lid = store.resolve(&l).ok_or_else(|| bad("element.not_found", "no such level"))?;
                let g = store.get(lid).expect("resolved").guid().to_string();
                (crate::levels::elevation_of(&store, &g)?, None, ElementRef::Id(lid))
            }
            DownspoutTo::Point(q) => (q[2], Some([q[0], q[1]]), ElementRef::Guid(GlobalId::parse(&level).ok_or_else(|| bad("level.not_found", "the gutter's level"))?)),
        };
        if target_z > tail[2] - 0.3 {
            return Err(bad("gutter.downspout", "the downspout's end must be at least 300 mm below the outlet"));
        }
        let lz = match &level_ref {
            ElementRef::Id(i) => crate::levels::elevation_of(&store, store.get(*i).expect("level").guid().as_str())?,
            ElementRef::Guid(gu) => crate::levels::elevation_of(&store, gu.as_str())?,
        };
        // Back toward the wall by the eave's offset, less the pipe and a little room.
        let k = (1..g.along.len()).find(|k| at <= g.along[*k] + 1e-9).unwrap_or(1);
        let a = g.across[k];
        let al = a[0].hypot(a[1]).max(1e-9);
        let inward = [-a[0] / al, -a[1] / al];
        let back = (g.offset_from_wall - diameter / 2.0 - 0.03).max(0.0);
        let mut pts: Vec<V3> = vec![tail, [tail[0], tail[1], tail[2] - 0.15]];
        let wall_pt = [tail[0] + inward[0] * back, tail[1] + inward[1] * back];
        if back > 0.05 {
            pts.push([wall_pt[0], wall_pt[1], tail[2] - 0.15 - back]);
        }
        let end_xy = target_xy.unwrap_or(wall_pt);
        let last = *pts.last().expect("points");
        if (end_xy[0] - last[0]).hypot(end_xy[1] - last[1]) > 0.01 {
            pts.push([end_xy[0], end_xy[1], last[2]]);
        }
        pts.push([end_xy[0], end_xy[1], target_z]);
        let params = crate::mep_runs::CreateParams {
            type_id: None,
            points: pts.iter().map(|q| vec![q[0], q[1], q[2] - lz]).collect(),
            level: level_ref,
            middle_elevation: None,
            shape: Some(Shape::Round),
            diameter: Some(diameter),
            width: None,
            height: None,
            system_type_id: Some("storm_drainage".into()),
            slope: 0.0,
            bend_radius: None,
        };
        let rep = crate::mep_runs::create_pipe(txn, params)?;
        downspout.extend(rep.segments);
        downspout.extend(rep.fittings);
    }
    mep::connect_coincident(txn, &[id, fitting])?;
    // The storm system: the gutter's, or a new one.
    let store = txn.store().clone();
    let system = match crate::mep_systems::systems(&store).into_iter().find(|s| s.classification == Classification::StormDrainage && s.elements.contains(&id)) {
        Some(s) => {
            let mut members: Vec<ElementId> = vec![id, fitting];
            members.extend(downspout.iter().copied());
            crate::mep_systems::add_members(txn, s.id, &members)?;
            s.id
        }
        None => {
            let mut members: Vec<ElementRef> = vec![ElementRef::Id(id), ElementRef::Id(fitting)];
            members.extend(downspout.iter().map(|e| ElementRef::Id(*e)));
            crate::mep_systems::create(
                txn,
                crate::mep_systems::CreateParams { kind: crate::mep_systems::SystemKind::Piping, element_ids: members, system_type_id: Some("storm_drainage".into()), name: None, skip_incompatible: false },
            )?
        }
    };
    Ok(OutletReport { at, fitting, downspout, system })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SizeParams {
    pub gutter_id: ElementRef,
    /// Design rainfall intensity (mm/h); 75 by default.
    pub rainfall: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OutletCheck {
    pub at: f64,
    /// Plan area of roof draining to it (m²).
    pub area: f64,
    /// l/s.
    pub flow: f64,
    pub gutter_capacity: f64,
    pub downspout_capacity: f64,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SizeReport {
    pub rainfall: f64,
    /// Plan area of the roof faces draining to the gutter (m²).
    pub area: f64,
    pub outlets: Vec<OutletCheck>,
    pub warnings: Vec<String>,
}

/// The gutter's capacity flowing full (l/s), by Manning at its fall.
pub fn gutter_capacity(r: &GutterRecipe) -> f64 {
    let (w, d) = (r.width, r.depth);
    let (area, wetted) = match r.profile {
        Profile::HalfRound => (std::f64::consts::PI * (w / 2.0) * d / 2.0, std::f64::consts::PI * ((w / 2.0 + d) / 2.0)),
        Profile::Box => (w * d, w + 2.0 * d),
        Profile::KStyle => (0.85 * w * d, w + 2.0 * d),
    };
    let rh = area / wetted;
    let s = r.fall.max(0.001);
    1000.0 * area * rh.powf(2.0 / 3.0) * s.sqrt() / 0.012
}

/// A downspout's capacity at its outlet (l/s): an orifice under half the gutter's depth.
pub fn downspout_capacity(diameter: f64, gutter_depth: f64) -> f64 {
    1000.0 * 0.6 * std::f64::consts::PI * diameter * diameter / 4.0 * (2.0 * 9.81 * gutter_depth / 2.0).sqrt()
}

/// `gutter.size`.
pub(crate) fn size(store: &Store, p: SizeParams) -> Result<SizeReport> {
    let (_, r) = resolve(store, &p.gutter_id)?;
    let rain = p.rainfall.unwrap_or(75.0);
    if !(1.0..=1000.0).contains(&rain) {
        return Err(bad("gutter.rainfall", "a rainfall intensity runs from 1 to 1000 mm/h"));
    }
    let g = geometry(store, &r)?;
    let roof = GlobalId::parse(&r.roof).and_then(|gu| store.find_by_guid(&gu)).ok_or_else(|| bad("gutter.no_roof", "the roof is gone"))?;
    let rg = crate::roofs::regenerate(store, store.get(roof).expect("found"))?;
    let area: f64 = rg
        .faces
        .iter()
        .filter(|f| f.key.is_some_and(|k| r.keys.contains(&k)))
        .map(|f| {
            let b = &f.bottom;
            (0..b.len()).map(|k| b[k][0] * b[(k + 1) % b.len()][1] - b[(k + 1) % b.len()][0] * b[k][1]).sum::<f64>().abs() / 2.0
        })
        .sum();
    let mut warnings = Vec::new();
    if r.outlets.is_empty() {
        warnings.push("The gutter has no outlet".into());
    }
    // Each outlet drains the length of gutter nearer to it than to the others.
    let outs: Vec<f64> = r.outlets.iter().map(|o| o.at).collect();
    let share = |j: usize| -> f64 {
        let steps = 400;
        (0..steps)
            .filter(|i| {
                let s = (*i as f64 + 0.5) / steps as f64 * g.length;
                let near = outs.iter().enumerate().min_by(|a, b| (s - a.1).abs().total_cmp(&(s - b.1).abs())).map(|x| x.0);
                near == Some(j)
            })
            .count() as f64
            / steps as f64
    };
    let cap = gutter_capacity(&r);
    let outlets: Vec<OutletCheck> = r
        .outlets
        .iter()
        .enumerate()
        .map(|(j, o)| {
            let a = area * share(j);
            let flow = a * rain / 3600.0;
            let dc = downspout_capacity(o.diameter, r.depth);
            OutletCheck { at: o.at, area: a, flow, gutter_capacity: cap, downspout_capacity: dc, ok: flow <= cap && flow <= dc }
        })
        .collect();
    for o in outlets.iter().filter(|o| !o.ok) {
        warnings.push(format!(
            "The outlet at {:.2} m takes {:.2} l/s: more than the {} ({:.2} l/s)",
            o.at,
            o.flow,
            if o.flow > o.gutter_capacity { "gutter carries" } else { "downspout carries" },
            o.gutter_capacity.min(o.downspout_capacity)
        ));
    }
    Ok(SizeReport { rainfall: rain, area, outlets, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_and_capacities() {
        for p in [Profile::HalfRound, Profile::KStyle, Profile::Box] {
            let pts = profile_points(p, 0.125, 0.0625);
            assert!(pts.len() >= 4);
            assert!(pts.iter().all(|(o, i)| o[1] <= 1e-12 && i[1] >= o[1] - 1e-12));
        }
        let r = GutterRecipe { kind: GUTTER.into(), roof: String::new(), keys: vec![], profile: Profile::HalfRound, width: 0.125, depth: 0.0625, fall: 0.002, outlets: vec![], level: String::new() };
        // A 125 mm half-round at 1:500 carries about 1.3 l/s.
        let c = gutter_capacity(&r);
        assert!((0.8..2.5).contains(&c), "{c}");
        // A 68 mm downspout under 31 mm of water takes about 1.7 l/s.
        let d = downspout_capacity(0.068, 0.0625);
        assert!((1.0..2.5).contains(&d), "{d}");
    }
}
