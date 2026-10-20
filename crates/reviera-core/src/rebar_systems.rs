// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Area, path and fabric reinforcement (STR-RBR-010, 011, 012).
//!
//! Each system is one `IfcElementAssembly` (`REINFORCEMENT_UNIT`) in a structural floor or wall,
//! whose recipe works its bars or sheets out from the host whenever they are needed. The IFC
//! writer adds the bars (`IfcReinforcingBar`) and sheets (`IfcReinforcingMesh`) under it.
//!
//! **Area reinforcement** lays up to four layers (top and bottom, or exterior and interior, each
//! major and minor) over a closed boundary or the whole host. The major direction follows a
//! boundary edge; each face's minor layer sits inside its major one, and extra offsets stack the
//! layers further in. Host openings and the boundary cut the bars. With area and path rebar set
//! to real rebar (Reinforcement Settings), *Remove Area System* turns the layers into ordinary
//! rebar sets.
//!
//! **Path reinforcement** lays bars square to an open path, on one side of it, spaced along it:
//! primary bars, with alternating bars between them that may be of another type, length or shape
//! and shifted along their length.
//!
//! **Fabric** comes as sheet types made of wire types (BS 4483 and WRI catalogues, or the
//! project's own). A fabric area lays sheets over a boundary from its major edge, lapped by the
//! set lengths, the laps of every second row staggered by half a sheet when asked, each sheet cut
//! by the boundary and by the host's openings and cover. A single sheet is placed at a point and
//! rotation, cut by the host's cover when asked.
//!
//! Not yet: sketching boundaries and paths in the view, reinforcement symbols and tags, bent and
//! custom fabric sheets, the start-edge choice of fabric areas, wall openings cutting wall layers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::rebar::{self, Bar, HostKind, Layout, Placement, RebarRecipe, Rule, add, len, mul, sub, unit};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const AREA_RECIPE: &str = "recipe.area_reinforcement";
pub const PATH_RECIPE: &str = "recipe.path_reinforcement";
pub const FABRIC_AREA_RECIPE: &str = "recipe.fabric_area";
pub const FABRIC_SHEET_RECIPE: &str = "recipe.fabric_sheet";

const STEEL: f64 = 7850.0;

fn yes() -> bool {
    true
}

fn area_of(d: f64) -> f64 {
    std::f64::consts::PI * d * d / 4.0
}

fn recipe_kind(e: &Element) -> Option<&str> {
    e.param(RECIPE_KEY)?.get("kind")?.as_str()
}

fn parse<T: serde::de::DeserializeOwned>(e: &Element, kind: &str) -> Option<T> {
    (recipe_kind(e)? == kind).then(|| serde_json::from_value(e.param(RECIPE_KEY)?.clone()).ok())?
}

/// What kind of reinforcement system an element is: `area`, `path`, `fabric_area` or
/// `fabric_sheet`.
pub fn system_kind(e: &Element) -> Option<&'static str> {
    match recipe_kind(e)? {
        AREA_RECIPE => Some("area"),
        PATH_RECIPE => Some("path"),
        FABRIC_AREA_RECIPE => Some("fabric_area"),
        FABRIC_SHEET_RECIPE => Some("fabric_sheet"),
        _ => None,
    }
}

fn host_guid(e: &Element) -> Option<String> {
    e.param(RECIPE_KEY)?.get("host")?.as_str().map(str::to_owned)
}

/// Whether an element goes with `guid`: a system in it.
pub fn uses(e: &Element, guid: &str) -> bool {
    system_kind(e).is_some() && host_guid(e).as_deref() == Some(guid)
}

/// Whether an area or path system exists (which locks the real-rebar setting).
pub fn any_area_or_path(store: &Store) -> bool {
    store.iter().any(|e| matches!(system_kind(e), Some("area" | "path")))
}

fn resolve<'a>(store: &'a Store, r: &ElementRef) -> Result<&'a Element> {
    let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
    Ok(store.get(id).expect("resolved"))
}

/// A host for a system: a structural floor or wall that can hold rebar.
fn layer_host<'a>(store: &'a Store, r: &ElementRef) -> Result<(&'a Element, HostKind)> {
    let e = resolve(store, r)?;
    let kind = rebar::host_check(store, e).map_err(|why| Error::new("rebar.not_a_host", format!("element {} cannot host rebar: {why}", e.id())))?;
    if !matches!(kind, HostKind::Floor | HostKind::Wall) {
        return Err(Error::new("rebar.not_a_host", "area, path and fabric reinforcement go in floors and walls"));
    }
    Ok((e, kind))
}

fn host_of<'a>(store: &'a Store, guid: &str) -> Result<(&'a Element, HostKind)> {
    let e = GlobalId::parse(guid)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|i| store.get(i))
        .ok_or_else(|| Error::new("rebar.no_host", "the system's host is gone"))?;
    let kind = rebar::host_kind(store, e).ok_or_else(|| Error::new("rebar.no_host", "the host cannot hold rebar"))?;
    Ok((e, kind))
}

fn faces(kind: HostKind) -> (&'static str, &'static str) {
    if kind == HostKind::Wall { ("exterior", "interior") } else { ("top", "bottom") }
}

fn check_face(kind: HostKind, face: &str) -> Result<()> {
    let (a, b) = faces(kind);
    if face != a && face != b {
        return Err(Error::new("rebar.face", format!("this host's faces are {a} and {b}, not {face}")));
    }
    Ok(())
}

/// A boundary given as points, or `"whole_host"` (or nothing) for the host's outline.
fn boundary_of(v: &Option<Value>) -> Result<Option<Vec<[f64; 2]>>> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s == "whole_host" => Ok(None),
        Some(v) => {
            let pts: Vec<[f64; 2]> = serde_json::from_value(v.clone())
                .map_err(|_| Error::new("rebar.boundary", "a boundary is a list of [x, y] points or \"whole_host\""))?;
            let mut a = 0.0;
            for i in 0..pts.len() {
                let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                a += p[0] * q[1] - q[0] * p[1];
            }
            if pts.len() < 3 || a.abs() / 2.0 < 1e-6 {
                return Err(Error::new("rebar.boundary", "a boundary is a closed loop of at least three points around an area"));
            }
            Ok(Some(pts))
        }
    }
}

/// The loop the major edge is on: the boundary, else the host's outline.
fn major_angle(plane: &rebar::HostPlane, boundary: Option<&[[f64; 2]]>, edge: usize) -> Result<f64> {
    let ring: Vec<[f64; 2]> = match boundary {
        Some(b) => b.to_vec(),
        None => plane.shapes.first().map(|s| s.outer.clone()).unwrap_or_default(),
    };
    if edge >= ring.len() {
        return Err(Error::new("rebar.major_edge", format!("the loop has {} edges; the major edge is 0 to {}", ring.len(), ring.len().saturating_sub(1))));
    }
    let (p, q) = (ring[edge], ring[(edge + 1) % ring.len()]);
    Ok((q[1] - p[1]).atan2(q[0] - p[0]).to_degrees())
}

fn orphan(store: &Store, guid: &str) -> Option<String> {
    let h = GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i));
    h.map_or(Some("the host is gone".to_owned()), |h| rebar::host_check(store, h).err())
}

fn tubes(bars: &[(f64, &Bar)], sides: usize) -> crate::bodies::Mesh {
    let mut parts = Vec::new();
    for (d, b) in bars {
        for w in b.points.windows(2) {
            if len(sub(w[1], w[0])) > 1e-6 {
                parts.push(crate::steel_fab::cylinder(w[0], w[1], d / 2.0, sides));
            }
        }
    }
    crate::steel_fab::join(parts)
}

// ---- Area reinforcement (STR-RBR-010) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Major,
    Minor,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AreaRule {
    #[default]
    MaximumSpacing,
    FixedNumber,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AreaLayer {
    /// `top` or `bottom` for floors; `exterior` or `interior` for walls.
    pub face: String,
    pub direction: Direction,
    #[serde(default = "yes")]
    pub on: bool,
    pub bar_type: String,
    /// A hook type at both ends of each bar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook: Option<String>,
    /// Hooks turned out of the face instead of into the host.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hook_flip: bool,
    /// The maximum spacing (metres), under that rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f64>,
    /// The number of bars, under the fixed number rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AreaRecipe {
    pub kind: String,
    pub host: String,
    /// In the host's plane; none for the whole host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<[f64; 2]>>,
    /// The boundary's (or the host outline's) edge the major bars run along.
    #[serde(default)]
    pub major_edge: usize,
    #[serde(default)]
    pub layout_rule: AreaRule,
    /// Further in than the cover at the top or exterior face, and at the bottom or interior one.
    #[serde(default)]
    pub additional_top: f64,
    #[serde(default)]
    pub additional_bottom: f64,
    pub layers: Vec<AreaLayer>,
}

pub fn area_recipe(e: &Element) -> Option<AreaRecipe> {
    parse(e, AREA_RECIPE)
}

/// The rebar sets an area system's layers come to, by layer.
pub fn area_sets(store: &Store, r: &AreaRecipe) -> Result<Vec<(usize, RebarRecipe)>> {
    let (host, kind) = host_of(store, &r.host)?;
    let (outer_face, _) = faces(kind);
    let first = r.layers.first().ok_or_else(|| Error::new("area_reinforcement.layers", "an area system has at least one layer"))?;
    let probe = rebar::host_plane(store, host, kind, &first.face, 0.0, 0.0)?;
    let major = major_angle(&probe, r.boundary.as_deref(), r.major_edge)?;
    let mut out = Vec::new();
    for (i, l) in r.layers.iter().enumerate() {
        check_face(kind, &l.face)?;
        if !l.on {
            continue;
        }
        let extra = if l.face == outer_face { r.additional_top } else { r.additional_bottom };
        // A face's minor layer sits inside its major one.
        let inside = if l.direction == Direction::Minor {
            r.layers
                .iter()
                .find(|m| m.on && m.face == l.face && m.direction == Direction::Major)
                .map(|m| rebar::bar_types(store).into_iter().find(|b| b.id == m.bar_type || b.name == m.bar_type).map_or(0.0, |b| b.diameter))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let angle = if l.direction == Direction::Major { major } else { major + 90.0 };
        let layout = match r.layout_rule {
            AreaRule::MaximumSpacing => Layout { rule: Rule::MaximumSpacing, spacing: l.spacing, ..Layout::default() },
            AreaRule::FixedNumber => Layout { rule: Rule::FixedNumber, n: l.n, ..Layout::default() },
        };
        out.push((
            i,
            RebarRecipe {
                kind: rebar::REBAR_RECIPE.into(),
                host: r.host.clone(),
                shape: "00".into(),
                bar_type: l.bar_type.clone(),
                placement: Placement::Layer {
                    face: l.face.clone(),
                    angle,
                    vertical: false,
                    offset: extra + inside,
                    boundary: r.boundary.clone(),
                    hook: l.hook.clone(),
                    hook_flip: l.hook_flip,
                },
                layout,
                edits: rebar::BarEdits::default(),
                splice: None,
            },
        ));
    }
    Ok(out)
}

fn check_area(store: &Store, r: &AreaRecipe) -> Result<()> {
    for (_, set) in area_sets(store, r)? {
        rebar::geometry(store, &set)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AreaCreateParams {
    pub host_id: ElementRef,
    /// A closed loop of [x, y] points in the host's plane (plan for floors; along the wall and
    /// up from its base for walls), or `"whole_host"` (the default).
    #[serde(default)]
    pub boundary: Option<Value>,
    #[serde(default)]
    pub major_edge: usize,
    pub layers: Vec<AreaLayer>,
    #[serde(default)]
    pub layout_rule: AreaRule,
    #[serde(default)]
    pub additional_top: f64,
    #[serde(default)]
    pub additional_bottom: f64,
}

/// Lays area reinforcement (`area_reinforcement.create`).
pub(crate) fn area_create(txn: &mut crate::txn::Transaction<'_>, p: AreaCreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (host, _) = layer_host(&store, &p.host_id)?;
    let r = AreaRecipe {
        kind: AREA_RECIPE.into(),
        host: host.guid().to_string(),
        boundary: boundary_of(&p.boundary)?,
        major_edge: p.major_edge,
        layout_rule: p.layout_rule,
        additional_top: p.additional_top,
        additional_bottom: p.additional_bottom,
        layers: p.layers,
    };
    check_area(&store, &r)?;
    txn.create(
        NewElement::new("IfcElementAssembly")
            .name("Area Reinforcement")
            .param("@PredefinedType", "REINFORCEMENT_UNIT")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AreaSetParams {
    pub system_ids: Vec<ElementRef>,
    #[serde(default)]
    pub layers: Option<Vec<AreaLayer>>,
    #[serde(default)]
    pub layout_rule: Option<AreaRule>,
    #[serde(default)]
    pub boundary: Option<Value>,
    #[serde(default)]
    pub major_edge: Option<usize>,
    #[serde(default)]
    pub additional_top: Option<f64>,
    #[serde(default)]
    pub additional_bottom: Option<f64>,
}

/// Changes area systems (`area_reinforcement.set`).
pub(crate) fn area_set(txn: &mut crate::txn::Transaction<'_>, p: AreaSetParams) -> Result<()> {
    for s in &p.system_ids {
        let store = txn.store().clone();
        let e = resolve(&store, s)?;
        let mut r = area_recipe(e).ok_or_else(|| Error::new("area_reinforcement.not_area", format!("element {} is not area reinforcement", e.id())))?;
        if let Some(l) = &p.layers {
            r.layers = l.clone();
        }
        if let Some(v) = p.layout_rule {
            r.layout_rule = v;
        }
        if p.boundary.is_some() {
            r.boundary = boundary_of(&p.boundary)?;
        }
        if let Some(v) = p.major_edge {
            r.major_edge = v;
        }
        if let Some(v) = p.additional_top {
            r.additional_top = v;
        }
        if let Some(v) = p.additional_bottom {
            r.additional_bottom = v;
        }
        check_area(&store, &r)?;
        txn.set_param(e.id(), RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RemoveSystemParams {
    pub system_id: ElementRef,
}

/// Removes an area system, keeping its bars as rebar sets (`area_reinforcement.remove_system`).
pub(crate) fn area_remove_system(txn: &mut crate::txn::Transaction<'_>, p: RemoveSystemParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let e = resolve(&store, &p.system_id)?;
    let r = area_recipe(e).ok_or_else(|| Error::new("area_reinforcement.not_area", format!("element {} is not area reinforcement", e.id())))?;
    if !rebar::settings(&store).area_path_rebar {
        return Err(Error::new(
            "area_reinforcement.not_real",
            "Remove Area System needs area and path reinforcement as real rebar (Reinforcement Settings)",
        ));
    }
    let mut ids = Vec::new();
    for (_, set) in area_sets(&store, &r)? {
        rebar::geometry(&store, &set)?;
        ids.push(txn.create(
            NewElement::new("IfcReinforcingBar")
                .param("@PredefinedType", "MAIN")
                .param(RECIPE_KEY, serde_json::to_value(&set).expect("serialises")),
        )?);
    }
    txn.delete(e.id())?;
    crate::rebar_numbers::persist(txn)?;
    Ok(ids)
}

// ---- Path reinforcement (STR-RBR-011) ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathBar {
    pub bar_type: String,
    /// The bar's length square to the path (metres).
    pub length: f64,
    /// Segments turned into the host at both ends (metres; 0 for a single straight segment).
    #[serde(default)]
    pub legs: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_hook: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_hook: Option<String>,
    /// How far from the path the bar starts (metres), to shift alternating bars.
    #[serde(default)]
    pub offset: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathRecipe {
    pub kind: String,
    pub host: String,
    /// An open polyline in the host's plane.
    pub path: Vec<[f64; 2]>,
    /// Bars to the right of the path (walking along it) instead of the left.
    #[serde(default)]
    pub flip: bool,
    /// `top` or `bottom` for floors; `exterior` or `interior` for walls.
    pub face: String,
    /// Further in than the cover (metres).
    #[serde(default)]
    pub additional_offset: f64,
    /// Spacing or number along the path (maximum spacing, fixed number or number with spacing).
    pub layout: Layout,
    pub primary: PathBar,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternating: Option<PathBar>,
}

pub fn path_recipe(e: &Element) -> Option<PathRecipe> {
    parse(e, PATH_RECIPE)
}

/// A bar of a path system: whether it is an alternating one.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PathBarOut {
    pub alternating: bool,
    pub bar: Bar,
}

fn polyline_length(p: &[[f64; 2]]) -> f64 {
    p.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum()
}

/// The point and direction at a distance along a polyline.
fn walk(p: &[[f64; 2]], s: f64) -> ([f64; 2], [f64; 2]) {
    let mut left = s;
    for (k, w) in p.windows(2).enumerate() {
        let l = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if l < 1e-12 {
            continue;
        }
        let t = [(w[1][0] - w[0][0]) / l, (w[1][1] - w[0][1]) / l];
        if left <= l + 1e-9 || k + 2 == p.len() {
            return ([w[0][0] + t[0] * left, w[0][1] + t[1] * left], t);
        }
        left -= l;
    }
    (p[0], [1.0, 0.0])
}

/// A path system's bars, in order along the path.
pub fn path_bars(store: &Store, r: &PathRecipe) -> Result<Vec<PathBarOut>> {
    let (host, kind) = host_of(store, &r.host)?;
    check_face(kind, &r.face)?;
    if r.path.len() < 2 || polyline_length(&r.path) < 1e-6 {
        return Err(Error::new("path_reinforcement.path", "a path is an open polyline of at least two points"));
    }
    if !matches!(r.layout.rule, Rule::MaximumSpacing | Rule::FixedNumber | Rule::NumberWithSpacing) {
        return Err(Error::new("path_reinforcement.layout", "path bars take maximum spacing, fixed number or number with spacing"));
    }
    let edge = rebar::cover(store, host, kind, "other");
    let lp = polyline_length(&r.path);
    let pt = rebar::bar_types(store);
    let find = |b: &PathBar| {
        pt.iter().find(|t| t.id == b.bar_type || t.name == b.bar_type).cloned().ok_or_else(|| Error::new("rebar.bar_type", format!("no bar type {}", b.bar_type)))
    };
    let primary = find(&r.primary)?;
    let alternating = r.alternating.as_ref().map(|a| Ok::<_, Error>((a, find(a)?))).transpose()?;
    for b in std::iter::once(&r.primary).chain(&r.alternating) {
        if b.length <= 0.0 {
            return Err(Error::new("path_reinforcement.length", "a path bar's length is more than 0"));
        }
    }
    let span = lp - 2.0 * edge - primary.diameter;
    if span <= 0.0 {
        return Err(Error::new("rebar.no_fit", format!("the path is {:.0} mm long, too short for its cover", lp * 1000.0)));
    }
    let pos = rebar::positions(&r.layout, span, primary.diameter)?;
    let mut out = Vec::new();
    for (i, t) in pos.iter().enumerate() {
        let (spec, bt, is_alt) = match &alternating {
            Some((a, bt)) if i % 2 == 1 => (*a, bt, true),
            _ => (&r.primary, &primary, false),
        };
        let d = bt.diameter;
        let plane = rebar::host_plane(store, host, kind, &r.face, d, r.additional_offset)?;
        let rings = plane.clipped(None);
        let (p, tan) = walk(&r.path, edge + primary.diameter / 2.0 + t);
        let side = if r.flip { -1.0 } else { 1.0 };
        let n = [-tan[1] * side, tan[0] * side];
        let (a0, a1) = (spec.offset, spec.offset + spec.length);
        for (t0, t1) in rebar::chord(&rings, p, n) {
            let (t0, t1) = ((t0 + edge).max(a0), (t1 - edge).min(a1));
            if t1 - t0 <= d {
                continue;
            }
            let at = |s: f64| plane.at([p[0] + n[0] * s, p[1] + n[1] * s]);
            let (a3, b3) = (at(t0), at(t1));
            let dir = unit(sub(b3, a3));
            let leg = spec.legs.min(plane.room).max(0.0);
            let mut start = if leg > 0.0 { vec![add(a3, mul(plane.inward, leg)), a3] } else { vec![a3] };
            let mut end = if leg > 0.0 { vec![b3, add(b3, mul(plane.inward, leg))] } else { vec![b3] };
            if let Some(h) = &spec.start_hook {
                let h = rebar::hook_type(store, h)?;
                let (tip, out_dir) = if leg > 0.0 { (start[0], plane.inward) } else { (a3, mul(dir, -1.0)) };
                let into = if leg > 0.0 { dir } else { plane.inward };
                let mut v = rebar::hook_points(tip, out_dir, into, &h, bt, plane.room);
                v.reverse();
                v.pop();
                v.extend(start);
                start = v;
            }
            if let Some(h) = &spec.end_hook {
                let h = rebar::hook_type(store, h)?;
                let (tip, out_dir) = if leg > 0.0 { (*end.last().expect("an end"), plane.inward) } else { (b3, dir) };
                let into = if leg > 0.0 { mul(dir, -1.0) } else { plane.inward };
                let v = rebar::hook_points(tip, out_dir, into, &h, bt, plane.room);
                end.extend(v.into_iter().skip(1));
            }
            let mut pts = start;
            pts.extend(end);
            out.push(PathBarOut { alternating: is_alt, bar: Bar { points: rebar::rounded(&pts, bt.standard_bend / 2.0 + d / 2.0) } });
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PathCreateParams {
    pub host_id: ElementRef,
    /// An open polyline of [x, y] points in the host's plane.
    pub path: Vec<[f64; 2]>,
    /// Bars to the right of the path instead of the left.
    #[serde(default)]
    pub flip: bool,
    pub face: String,
    pub primary: PathBar,
    #[serde(default)]
    pub alternating: Option<PathBar>,
    /// Bar spacing along the path (metres), for maximum spacing.
    #[serde(default)]
    pub spacing: Option<f64>,
    /// A number of bars, for a fixed number.
    #[serde(default)]
    pub count: Option<usize>,
    #[serde(default)]
    pub additional_offset: f64,
}

/// Lays path reinforcement (`path_reinforcement.create`).
pub(crate) fn path_create(txn: &mut crate::txn::Transaction<'_>, p: PathCreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (host, _) = layer_host(&store, &p.host_id)?;
    let layout = match (p.spacing, p.count) {
        (Some(s), None) => Layout { rule: Rule::MaximumSpacing, spacing: Some(s), ..Layout::default() },
        (None, Some(n)) => Layout { rule: Rule::FixedNumber, n: Some(n), ..Layout::default() },
        (Some(s), Some(n)) => Layout { rule: Rule::NumberWithSpacing, n: Some(n), spacing: Some(s), ..Layout::default() },
        (None, None) => return Err(Error::new("path_reinforcement.layout", "give a spacing or a count of bars")),
    };
    let r = PathRecipe {
        kind: PATH_RECIPE.into(),
        host: host.guid().to_string(),
        path: p.path,
        flip: p.flip,
        face: p.face,
        additional_offset: p.additional_offset,
        layout,
        primary: p.primary,
        alternating: p.alternating,
    };
    if path_bars(&store, &r)?.is_empty() {
        return Err(Error::new("rebar.no_fit", "no bars fit: is the path on the host, with the bars on the host's side?"));
    }
    txn.create(
        NewElement::new("IfcElementAssembly")
            .name("Path Reinforcement")
            .param("@PredefinedType", "REINFORCEMENT_UNIT")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PathSetParams {
    pub system_ids: Vec<ElementRef>,
    /// A new path (an open polyline in the host's plane).
    #[serde(default)]
    pub path: Option<Vec<[f64; 2]>>,
    #[serde(default)]
    pub flip: Option<bool>,
    #[serde(default)]
    pub layout: Option<Layout>,
    #[serde(default)]
    pub primary: Option<PathBar>,
    /// Alternating bars; `null` removes them.
    #[serde(default, deserialize_with = "some_value")]
    pub alternating: Option<Option<PathBar>>,
}

fn some_value<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> std::result::Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

/// Changes path systems (`path_reinforcement.set`).
pub(crate) fn path_set(txn: &mut crate::txn::Transaction<'_>, p: PathSetParams) -> Result<()> {
    for s in &p.system_ids {
        let store = txn.store().clone();
        let e = resolve(&store, s)?;
        let mut r = path_recipe(e).ok_or_else(|| Error::new("path_reinforcement.not_path", format!("element {} is not path reinforcement", e.id())))?;
        if let Some(v) = &p.path {
            r.path = v.clone();
        }
        if let Some(v) = p.flip {
            r.flip = v;
        }
        if let Some(v) = &p.layout {
            r.layout = v.clone();
        }
        if let Some(v) = &p.primary {
            r.primary = v.clone();
        }
        if let Some(v) = &p.alternating {
            r.alternating = v.clone();
        }
        path_bars(&store, &r)?;
        txn.set_param(e.id(), RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))?;
    }
    Ok(())
}

// ---- Fabric (STR-RBR-012) ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WireType {
    pub id: String,
    pub name: String,
    pub diameter: f64,
    pub material: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SheetType {
    pub id: String,
    pub name: String,
    /// Wire types of the major wires (along the sheet's length) and the minor ones across it.
    pub major_wire: String,
    pub minor_wire: String,
    /// Between major wires, and between minor wires (metres).
    pub major_spacing: f64,
    pub minor_spacing: f64,
    pub length: f64,
    pub width: f64,
    /// Past the last minor wire at the sheet's ends, and past the last major wire at its sides;
    /// half the crossing wires' spacing when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub major_overhang: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minor_overhang: Option<f64>,
}

impl SheetType {
    pub fn overhangs(&self) -> (f64, f64) {
        (self.major_overhang.unwrap_or(self.minor_spacing / 2.0), self.minor_overhang.unwrap_or(self.major_spacing / 2.0))
    }
}

#[derive(Deserialize)]
struct FabricData {
    metric: MetricFabric,
    us: UsFabric,
}
#[derive(Deserialize)]
struct MetricFabric {
    material: String,
    length: f64,
    width: f64,
    wires_mm: Vec<u32>,
    sheets: Vec<MetricSheet>,
}
#[derive(Deserialize)]
struct MetricSheet {
    name: String,
    major: u32,
    minor: u32,
    major_spacing: f64,
    minor_spacing: f64,
}
#[derive(Deserialize)]
struct UsFabric {
    material: String,
    length_ft: f64,
    width_ft: f64,
    wires: Vec<String>,
    sheets: Vec<UsSheet>,
}
#[derive(Deserialize)]
struct UsSheet {
    name: String,
    major: String,
    minor: String,
    spacing_in: f64,
}

fn fabric_data() -> FabricData {
    serde_json::from_str(include_str!("../data/rebar/fabric.json")).expect("the fabric table parses")
}

const IN: f64 = 0.0254;

/// The region's wire and sheet types.
pub fn fabric_catalogue(us_region: bool) -> (Vec<WireType>, Vec<SheetType>) {
    let d = fabric_data();
    if us_region {
        let wires = d
            .us
            .wires
            .iter()
            .map(|w| {
                // W number: the wire's area in hundredths of a square inch.
                let a: f64 = w.trim_start_matches(['W', 'D']).parse::<f64>().unwrap_or(1.4) / 100.0 * IN * IN;
                WireType { id: w.clone(), name: w.clone(), diameter: (4.0 * a / std::f64::consts::PI).sqrt(), material: d.us.material.clone() }
            })
            .collect();
        let sheets = d
            .us
            .sheets
            .iter()
            .map(|s| SheetType {
                id: s.name.clone(),
                name: s.name.clone(),
                major_wire: s.major.clone(),
                minor_wire: s.minor.clone(),
                major_spacing: s.spacing_in * IN,
                minor_spacing: s.spacing_in * IN,
                length: d.us.length_ft * 12.0 * IN,
                width: d.us.width_ft * 12.0 * IN,
                major_overhang: None,
                minor_overhang: None,
            })
            .collect();
        (wires, sheets)
    } else {
        let wires = d
            .metric
            .wires_mm
            .iter()
            .map(|w| WireType { id: format!("w{w}"), name: format!("Ø{w} wire"), diameter: *w as f64 / 1000.0, material: d.metric.material.clone() })
            .collect();
        let sheets = d
            .metric
            .sheets
            .iter()
            .map(|s| SheetType {
                id: s.name.clone(),
                name: s.name.clone(),
                major_wire: format!("w{}", s.major),
                minor_wire: format!("w{}", s.minor),
                major_spacing: s.major_spacing / 1000.0,
                minor_spacing: s.minor_spacing / 1000.0,
                length: d.metric.length,
                width: d.metric.width,
                major_overhang: None,
                minor_overhang: None,
            })
            .collect();
        (wires, sheets)
    }
}

/// Every wire type: the region's and the project's.
pub fn wire_types(store: &Store) -> Vec<WireType> {
    let mut v = fabric_catalogue(rebar::us(store)).0;
    v.extend(rebar::settings(store).fabric_wires);
    v
}

/// Every sheet type, with its mass per square metre.
pub fn sheet_types(store: &Store) -> Vec<SheetType> {
    let mut v = fabric_catalogue(rebar::us(store)).1;
    v.extend(rebar::settings(store).fabric_sheets);
    v
}

fn wire(store: &Store, id: &str) -> Result<WireType> {
    wire_types(store)
        .into_iter()
        .find(|w| w.id == id || w.name == id)
        .ok_or_else(|| Error::new("fabric_wire_type.unknown", format!("no wire type {id}")))
}

fn sheet_type(store: &Store, id: &str) -> Result<SheetType> {
    sheet_types(store)
        .into_iter()
        .find(|s| s.id == id || s.name == id)
        .ok_or_else(|| Error::new("fabric_sheet_type.unknown", format!("no sheet type {id}")))
}

/// A sheet type's mass per square metre (kg).
pub fn unit_mass(store: &Store, t: &SheetType) -> Result<f64> {
    let (a, b) = (wire(store, &t.major_wire)?, wire(store, &t.minor_wire)?);
    Ok((area_of(a.diameter) / t.major_spacing + area_of(b.diameter) / t.minor_spacing) * STEEL)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LapPosition {
    #[default]
    Aligned,
    Staggered,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FabricAreaRecipe {
    pub kind: String,
    pub host: String,
    pub face: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<[f64; 2]>>,
    /// The edge sheets' lengths run along, and from which they are laid.
    #[serde(default)]
    pub major_edge: usize,
    pub sheet_type: String,
    #[serde(default)]
    pub lap_position: LapPosition,
    /// Laps where sheets meet end to end, and side by side (metres).
    pub major_lap: f64,
    pub minor_lap: f64,
    /// Further in than the cover (metres).
    #[serde(default)]
    pub offset: f64,
}

pub fn fabric_area_recipe(e: &Element) -> Option<FabricAreaRecipe> {
    parse(e, FABRIC_AREA_RECIPE)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FabricSheetRecipe {
    pub kind: String,
    pub host: String,
    pub face: String,
    pub sheet_type: String,
    /// The sheet's first corner in the host's plane, and its rotation (degrees) from the plane's
    /// x axis.
    pub point: [f64; 2],
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub cut_by_cover: bool,
    #[serde(default)]
    pub offset: f64,
}

pub fn fabric_sheet_recipe(e: &Element) -> Option<FabricSheetRecipe> {
    parse(e, FABRIC_SHEET_RECIPE)
}

/// A sheet as laid: its row and column in the area, its corners in the host's plane, its wires
/// and whether anything cut it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Sheet {
    pub row: usize,
    pub column: usize,
    pub corners: Vec<[f64; 2]>,
    pub major: Vec<Bar>,
    pub minor: Vec<Bar>,
    pub cut: bool,
    pub mass: f64,
}

/// Lays one sheet with its first corner at `o` and its length along `u`, clipped to `rings` (with
/// the edge cover) when given.
#[allow(clippy::too_many_arguments)]
fn lay_sheet(
    t: &SheetType,
    wires: (&WireType, &WireType),
    planes: (&rebar::HostPlane, &rebar::HostPlane),
    o: [f64; 2],
    u: [f64; 2],
    clip: Option<(&[Vec<[f64; 2]>], f64)>,
    row: usize,
    column: usize,
) -> Option<Sheet> {
    let v = [-u[1], u[0]];
    let (oh_major, oh_minor) = t.overhangs();
    let at = |a: f64, b: f64| [o[0] + u[0] * a + v[0] * b, o[1] + u[1] * a + v[1] * b];
    let mut cut = false;
    // Wires from `p` along `dir` for `l`, kept where they are inside.
    let mut run = |plane: &rebar::HostPlane, p: [f64; 2], dir: [f64; 2], l: f64, d: f64| -> Vec<Bar> {
        let Some((rings, edge)) = clip else {
            return vec![Bar { points: vec![plane.at(p), plane.at([p[0] + dir[0] * l, p[1] + dir[1] * l])] }];
        };
        let mut out = Vec::new();
        let mut kept = 0.0;
        for (t0, t1) in rebar::chord(rings, p, dir) {
            let (t0, t1) = ((t0 + edge).max(0.0), (t1 - edge).min(l));
            if t1 - t0 > d {
                kept += t1 - t0;
                out.push(Bar { points: vec![plane.at([p[0] + dir[0] * t0, p[1] + dir[1] * t0]), plane.at([p[0] + dir[0] * t1, p[1] + dir[1] * t1])] });
            }
        }
        if kept < l - 1e-6 {
            cut = true;
        }
        out
    };
    let mut major = Vec::new();
    let mut b = oh_minor;
    while b <= t.width - oh_minor + 1e-9 {
        major.extend(run(planes.0, at(0.0, b), u, t.length, wires.0.diameter));
        b += t.major_spacing;
    }
    let mut minor = Vec::new();
    let mut a = oh_major;
    while a <= t.length - oh_major + 1e-9 {
        minor.extend(run(planes.1, at(a, 0.0), v, t.width, wires.1.diameter));
        a += t.minor_spacing;
    }
    if major.is_empty() && minor.is_empty() {
        return None;
    }
    let mass = (major.iter().map(Bar::length).sum::<f64>() * area_of(wires.0.diameter)
        + minor.iter().map(Bar::length).sum::<f64>() * area_of(wires.1.diameter))
        * STEEL;
    Some(Sheet { row, column, corners: vec![at(0.0, 0.0), at(t.length, 0.0), at(t.length, t.width), at(0.0, t.width)], major, minor, cut, mass })
}

fn fabric_planes(store: &Store, host: &Element, kind: HostKind, face: &str, offset: f64, wires: (&WireType, &WireType)) -> Result<(rebar::HostPlane, rebar::HostPlane)> {
    check_face(kind, face)?;
    // Major wires at the cover, the minor ones inside them.
    let a = rebar::host_plane(store, host, kind, face, wires.0.diameter, offset)?;
    let b = rebar::host_plane(store, host, kind, face, wires.1.diameter, offset + wires.0.diameter)?;
    Ok((a, b))
}

/// A fabric area's sheets.
pub fn fabric_area_sheets(store: &Store, r: &FabricAreaRecipe) -> Result<Vec<Sheet>> {
    let (host, kind) = host_of(store, &r.host)?;
    let t = sheet_type(store, &r.sheet_type)?;
    let (wa, wb) = (wire(store, &t.major_wire)?, wire(store, &t.minor_wire)?);
    let (pa, pb) = fabric_planes(store, host, kind, &r.face, r.offset, (&wa, &wb))?;
    if r.major_lap < 0.0 || r.minor_lap < 0.0 || r.major_lap >= t.length || r.minor_lap >= t.width {
        return Err(Error::new("fabric_area.lap", "laps are at least 0 and shorter than the sheet"));
    }
    let rings = pa.clipped(r.boundary.as_deref());
    if rings.is_empty() {
        return Err(Error::new("rebar.no_fit", "the boundary does not overlap the host"));
    }
    let edge = rebar::cover(store, host, kind, "other");
    let ang = major_angle(&pa, r.boundary.as_deref(), r.major_edge)?.to_radians();
    let (u, v) = ([ang.cos(), ang.sin()], [-ang.sin(), ang.cos()]);
    // The region's extent along and across the major direction.
    let proj = |p: &[f64; 2], w: [f64; 2]| p[0] * w[0] + p[1] * w[1];
    let ext = |w: [f64; 2]| rings.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(proj(p, w)), b.max(proj(p, w))));
    let ((x0, x1), (y0, y1)) = (ext(u), ext(v));
    let (step_x, step_y) = (t.length - r.major_lap, t.width - r.minor_lap);
    let mut out = Vec::new();
    let mut row = 0;
    let mut y = y0;
    while y < y1 - 1e-6 {
        // Every second row's end laps move half a sheet along.
        let shift = if r.lap_position == LapPosition::Staggered && row % 2 == 1 { -t.length / 2.0 } else { 0.0 };
        let mut x = x0 + shift;
        let mut column = 0;
        while x < x1 - 1e-6 {
            let o = [u[0] * x + v[0] * y, u[1] * x + v[1] * y];
            if let Some(s) = lay_sheet(&t, (&wa, &wb), (&pa, &pb), o, u, Some((&rings, edge)), row, column) {
                out.push(s);
            }
            x += step_x;
            column += 1;
        }
        y += step_y;
        row += 1;
    }
    Ok(out)
}

/// A single sheet as placed.
pub fn fabric_sheet(store: &Store, r: &FabricSheetRecipe) -> Result<Sheet> {
    let (host, kind) = host_of(store, &r.host)?;
    let t = sheet_type(store, &r.sheet_type)?;
    let (wa, wb) = (wire(store, &t.major_wire)?, wire(store, &t.minor_wire)?);
    let (pa, pb) = fabric_planes(store, host, kind, &r.face, r.offset, (&wa, &wb))?;
    let rings = pa.clipped(None);
    let edge = rebar::cover(store, host, kind, "other");
    let a = r.rotation.to_radians();
    lay_sheet(&t, (&wa, &wb), (&pa, &pb), r.point, [a.cos(), a.sin()], r.cut_by_cover.then_some((rings.as_slice(), edge)), 0, 0)
        .ok_or_else(|| Error::new("rebar.no_fit", "the sheet is wholly outside the host's cover"))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FabricAreaParams {
    pub host_id: ElementRef,
    /// A closed loop of [x, y] points in the host's plane, or `"whole_host"`.
    #[serde(default)]
    pub boundary: Option<Value>,
    #[serde(default)]
    pub major_edge: usize,
    pub sheet_type_id: String,
    #[serde(default)]
    pub lap_position: LapPosition,
    /// Laps end to end and side by side (metres); by default a minor spacing and a major spacing
    /// plus 50 mm.
    #[serde(default)]
    pub major_lap: Option<f64>,
    #[serde(default)]
    pub minor_lap: Option<f64>,
    /// `top` or `bottom` (floors, default `bottom`); `exterior` or `interior` (walls).
    #[serde(default)]
    pub face: Option<String>,
    #[serde(default)]
    pub offset: f64,
}

/// Lays a fabric area (`fabric_area.create`).
pub(crate) fn fabric_area_create(txn: &mut crate::txn::Transaction<'_>, p: FabricAreaParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (host, kind) = layer_host(&store, &p.host_id)?;
    let t = sheet_type(&store, &p.sheet_type_id)?;
    let r = FabricAreaRecipe {
        kind: FABRIC_AREA_RECIPE.into(),
        host: host.guid().to_string(),
        face: p.face.unwrap_or_else(|| faces(kind).1.to_owned()),
        boundary: boundary_of(&p.boundary)?,
        major_edge: p.major_edge,
        sheet_type: t.id.clone(),
        lap_position: p.lap_position,
        major_lap: p.major_lap.unwrap_or(t.minor_spacing + 0.05),
        minor_lap: p.minor_lap.unwrap_or(t.major_spacing + 0.05),
        offset: p.offset,
    };
    if fabric_area_sheets(&store, &r)?.is_empty() {
        return Err(Error::new("rebar.no_fit", "no sheet fits inside the boundary and the cover"));
    }
    let id = txn.create(
        NewElement::new("IfcElementAssembly")
            .name("Fabric Area")
            .param("@PredefinedType", "REINFORCEMENT_UNIT")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    crate::rebar_numbers::persist(txn)?;
    Ok(id)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FabricAreaSetParams {
    pub system_ids: Vec<ElementRef>,
    #[serde(default)]
    pub sheet_type_id: Option<String>,
    #[serde(default)]
    pub lap_position: Option<LapPosition>,
    #[serde(default)]
    pub major_lap: Option<f64>,
    #[serde(default)]
    pub minor_lap: Option<f64>,
    #[serde(default)]
    pub major_edge: Option<usize>,
    /// A new boundary (points), or `"whole_host"`.
    #[serde(default)]
    pub boundary: Option<Value>,
}

/// Changes fabric areas (`fabric_area.set`).
pub(crate) fn fabric_area_set(txn: &mut crate::txn::Transaction<'_>, p: FabricAreaSetParams) -> Result<()> {
    for s in &p.system_ids {
        let store = txn.store().clone();
        let e = resolve(&store, s)?;
        let mut r = fabric_area_recipe(e).ok_or_else(|| Error::new("fabric_area.not_fabric", format!("element {} is not a fabric area", e.id())))?;
        if let Some(t) = &p.sheet_type_id {
            r.sheet_type = sheet_type(&store, t)?.id;
        }
        if let Some(v) = p.lap_position {
            r.lap_position = v;
        }
        if let Some(v) = p.major_lap {
            r.major_lap = v;
        }
        if let Some(v) = p.minor_lap {
            r.minor_lap = v;
        }
        if let Some(v) = p.major_edge {
            r.major_edge = v;
        }
        if p.boundary.is_some() {
            r.boundary = boundary_of(&p.boundary)?;
        }
        fabric_area_sheets(&store, &r)?;
        txn.set_param(e.id(), RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))?;
    }
    crate::rebar_numbers::persist(txn)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FabricSheetParams {
    pub host_id: ElementRef,
    pub sheet_type_id: String,
    /// The sheet's first corner, [x, y] in the host's plane.
    pub point: [f64; 2],
    /// Degrees from the plane's x axis.
    #[serde(default)]
    pub rotation: f64,
    /// Cut the sheet to the host's outline less the cover.
    #[serde(default)]
    pub cut_by_cover: bool,
    #[serde(default)]
    pub face: Option<String>,
    #[serde(default)]
    pub offset: f64,
}

/// Places one fabric sheet (`fabric_sheet.place`).
pub(crate) fn fabric_sheet_place(txn: &mut crate::txn::Transaction<'_>, p: FabricSheetParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (host, kind) = layer_host(&store, &p.host_id)?;
    let t = sheet_type(&store, &p.sheet_type_id)?;
    let r = FabricSheetRecipe {
        kind: FABRIC_SHEET_RECIPE.into(),
        host: host.guid().to_string(),
        face: p.face.unwrap_or_else(|| faces(kind).1.to_owned()),
        sheet_type: t.id,
        point: p.point,
        rotation: p.rotation,
        cut_by_cover: p.cut_by_cover,
        offset: p.offset,
    };
    fabric_sheet(&store, &r)?;
    let id = txn.create(
        NewElement::new("IfcReinforcingMesh")
            .name(t.name)
            .param("@PredefinedType", "NOTDEFINED")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    crate::rebar_numbers::persist(txn)?;
    Ok(id)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WireTypeParams {
    pub name: String,
    pub diameter: f64,
    #[serde(default)]
    pub material: Option<String>,
}

/// Makes a project wire type (`fabric_wire_type.create`).
pub(crate) fn wire_type_create(txn: &mut crate::txn::Transaction<'_>, p: WireTypeParams) -> Result<String> {
    let store = txn.store().clone();
    let mut s = rebar::settings(&store);
    if p.name.trim().is_empty() || wire_types(&store).iter().any(|w| w.id == p.name || w.name == p.name) {
        return Err(Error::new("fabric_wire_type.name", "a wire type's name is unique and not empty"));
    }
    if !(0.002..=0.025).contains(&p.diameter) {
        return Err(Error::new("fabric_wire_type.diameter", "a fabric wire is 2 to 25 mm in diameter"));
    }
    let material = p.material.unwrap_or_else(|| fabric_catalogue(rebar::us(&store)).0[0].material.clone());
    s.fabric_wires.push(WireType { id: p.name.clone(), name: p.name.clone(), diameter: p.diameter, material });
    rebar::save(txn, &s)?;
    Ok(p.name)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SheetTypeParams {
    pub name: String,
    pub major_wire: String,
    pub minor_wire: String,
    pub major_spacing: f64,
    pub minor_spacing: f64,
    pub length: f64,
    pub width: f64,
    #[serde(default)]
    pub major_overhang: Option<f64>,
    #[serde(default)]
    pub minor_overhang: Option<f64>,
}

/// Makes a project sheet type (`fabric_sheet_type.create`).
pub(crate) fn sheet_type_create(txn: &mut crate::txn::Transaction<'_>, p: SheetTypeParams) -> Result<String> {
    let store = txn.store().clone();
    let mut s = rebar::settings(&store);
    if p.name.trim().is_empty() || sheet_types(&store).iter().any(|t| t.id == p.name || t.name == p.name) {
        return Err(Error::new("fabric_sheet_type.name", "a sheet type's name is unique and not empty"));
    }
    let (a, b) = (wire(&store, &p.major_wire)?, wire(&store, &p.minor_wire)?);
    if p.major_spacing <= 0.0 || p.minor_spacing <= 0.0 || p.length <= p.minor_spacing || p.width <= p.major_spacing {
        return Err(Error::new("fabric_sheet_type.size", "spacings are more than 0 and the sheet bigger than them"));
    }
    s.fabric_sheets.push(SheetType {
        id: p.name.clone(),
        name: p.name.clone(),
        major_wire: a.id,
        minor_wire: b.id,
        major_spacing: p.major_spacing,
        minor_spacing: p.minor_spacing,
        length: p.length,
        width: p.width,
        major_overhang: p.major_overhang,
        minor_overhang: p.minor_overhang,
    });
    rebar::save(txn, &s)?;
    Ok(p.name)
}

/// A sheet type with its mass per square metre (`fabric_sheet_type.list`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SheetTypeInfo {
    #[serde(flatten)]
    pub sheet: SheetType,
    pub unit_mass: f64,
}

pub fn sheet_type_list(store: &Store) -> Vec<SheetTypeInfo> {
    sheet_types(store)
        .into_iter()
        .map(|t| SheetTypeInfo { unit_mass: unit_mass(store, &t).unwrap_or(0.0), sheet: t })
        .collect()
}

// ---- Info and bodies ----

/// One group of bars in a system: an area layer, or a path's primary or alternating bars.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BarGroup {
    /// `top major`, `bottom minor`, `primary`, `alternating`, …
    pub name: String,
    pub bar_type: String,
    pub diameter: f64,
    pub count: usize,
    pub lengths: Vec<f64>,
    pub total_length: f64,
    pub mass: f64,
    pub bars: Vec<Bar>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SystemInfo {
    pub id: ElementId,
    /// `area`, `path`, `fabric_area` or `fabric_sheet`.
    pub kind: String,
    pub host: Option<ElementId>,
    pub orphaned: bool,
    pub orphan_reason: Option<String>,
    /// Area and path systems: whether their bars count as real rebar.
    pub real_rebar: bool,
    pub groups: Vec<BarGroup>,
    pub sheets: Vec<Sheet>,
    pub sheet_type: Option<String>,
    pub bar_count: usize,
    pub total_length: f64,
    pub mass: f64,
    /// The steel's volume (cubic metres).
    pub volume: f64,
    pub recipe: Value,
}

fn group(store: &Store, name: String, bar_type: &str, bars: Vec<Bar>) -> Result<BarGroup> {
    let bt = rebar::bar_types(store)
        .into_iter()
        .find(|b| b.id == bar_type || b.name == bar_type)
        .ok_or_else(|| Error::new("rebar.bar_type", format!("no bar type {bar_type}")))?;
    let s = rebar::settings(store);
    let lengths: Vec<f64> = bars.iter().map(Bar::length).collect();
    let total: f64 = lengths.iter().map(|l| s.rounding.round(*l, s.rounding.bar_length)).sum();
    Ok(BarGroup {
        name,
        bar_type: bt.name.clone(),
        diameter: bt.diameter,
        count: bars.len(),
        lengths,
        total_length: total,
        mass: total * area_of(bt.diameter) * STEEL,
        bars,
    })
}

fn groups_of(store: &Store, e: &Element) -> Result<Vec<BarGroup>> {
    if let Some(r) = area_recipe(e) {
        return area_sets(store, &r)?
            .into_iter()
            .map(|(i, set)| {
                let l = &r.layers[i];
                let g = rebar::geometry(store, &set)?;
                group(store, format!("{} {}", l.face, if l.direction == Direction::Major { "major" } else { "minor" }), &l.bar_type, g.bars)
            })
            .collect();
    }
    if let Some(r) = path_recipe(e) {
        let bars = path_bars(store, &r)?;
        let (alt, prim): (Vec<_>, Vec<_>) = bars.into_iter().partition(|b| b.alternating);
        let mut out = vec![group(store, "primary".into(), &r.primary.bar_type, prim.into_iter().map(|b| b.bar).collect())?];
        if let Some(a) = &r.alternating {
            out.push(group(store, "alternating".into(), &a.bar_type, alt.into_iter().map(|b| b.bar).collect())?);
        }
        return Ok(out);
    }
    Ok(Vec::new())
}

fn sheets_of(store: &Store, e: &Element) -> Result<(Option<String>, Vec<Sheet>)> {
    if let Some(r) = fabric_area_recipe(e) {
        return Ok((Some(r.sheet_type.clone()), fabric_area_sheets(store, &r)?));
    }
    if let Some(r) = fabric_sheet_recipe(e) {
        return Ok((Some(r.sheet_type.clone()), vec![fabric_sheet(store, &r)?]));
    }
    Ok((None, Vec::new()))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct InfoParams {
    pub system_ids: Vec<ElementRef>,
}

/// Each system's bars or sheets (`reinforcement_system.info`).
pub fn info(store: &Store, p: InfoParams) -> Result<Vec<SystemInfo>> {
    p.system_ids
        .iter()
        .map(|s| {
            let e = resolve(store, s)?;
            let kind = system_kind(e).ok_or_else(|| Error::new("reinforcement_system.not_system", format!("element {} is not area, path or fabric reinforcement", e.id())))?;
            let guid = host_guid(e).unwrap_or_default();
            let host = GlobalId::parse(&guid).and_then(|g| store.find_by_guid(&g));
            let orphan = orphan(store, &guid);
            let groups = groups_of(store, e)?;
            let (sheet_type, sheets) = sheets_of(store, e)?;
            let mut count = groups.iter().map(|g| g.count).sum::<usize>();
            let mut total = groups.iter().map(|g| g.total_length).sum::<f64>();
            let mut mass = groups.iter().map(|g| g.mass).sum::<f64>();
            for sh in &sheets {
                count += sh.major.len() + sh.minor.len();
                total += sh.major.iter().chain(&sh.minor).map(Bar::length).sum::<f64>();
                mass += sh.mass;
            }
            Ok(SystemInfo {
                id: e.id(),
                kind: kind.into(),
                host,
                orphaned: orphan.is_some(),
                orphan_reason: orphan,
                real_rebar: matches!(kind, "area" | "path") && rebar::settings(store).area_path_rebar,
                groups,
                sheets,
                sheet_type,
                bar_count: count,
                total_length: total,
                mass,
                volume: mass / STEEL,
                recipe: e.param(RECIPE_KEY).cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

/// A sheet type's major and minor wire diameters.
pub fn wire_diameters(store: &Store, id: &str) -> Option<(f64, f64)> {
    sheet_type_lookup(store, id)
}

fn sheet_type_lookup(store: &Store, id: &str) -> Option<(f64, f64)> {
    let t = sheet_type(store, id).ok()?;
    Some((wire(store, &t.major_wire).ok()?.diameter, wire(store, &t.minor_wire).ok()?.diameter))
}

/// A system's body: a tube along each bar or wire.
pub fn mesh(store: &Store, e: &Element) -> Option<crate::bodies::Mesh> {
    system_kind(e)?;
    if let Ok(groups) = groups_of(store, e)
        && !groups.is_empty()
    {
        let bars: Vec<(f64, &Bar)> = groups.iter().flat_map(|g| g.bars.iter().map(move |b| (g.diameter, b))).collect();
        return Some(tubes(&bars, 8));
    }
    let (t, sheets) = sheets_of(store, e).ok()?;
    let (da, db) = sheet_type_lookup(store, t.as_deref()?)?;
    let bars: Vec<(f64, &Bar)> = sheets
        .iter()
        .flat_map(|s| s.major.iter().map(move |b| (da, b)).chain(s.minor.iter().map(move |b| (db, b))))
        .collect();
    Some(tubes(&bars, 6))
}

/// The IFC parts a system's export writes under it: bars by group (`IfcReinforcingBar`) or
/// sheets (`IfcReinforcingMesh`), each with its body and attributes.
#[derive(Debug)]
pub enum Part {
    Bars { group: BarGroup, mesh: crate::bodies::Mesh },
    Sheet { sheet_type: SheetType, major_diameter: f64, minor_diameter: f64, mesh: crate::bodies::Mesh, name: String },
}

pub fn parts(store: &Store, e: &Element) -> Vec<Part> {
    let mut out = Vec::new();
    if let Ok(groups) = groups_of(store, e) {
        for g in groups {
            let bars: Vec<(f64, &Bar)> = g.bars.iter().map(|b| (g.diameter, b)).collect();
            let mesh = tubes(&bars, 8);
            out.push(Part::Bars { group: g, mesh });
        }
    }
    if fabric_area_recipe(e).is_some()
        && let Ok((Some(t), sheets)) = sheets_of(store, e)
        && let (Ok(t), Some((da, db))) = (sheet_type(store, &t), sheet_type_lookup(store, &t))
    {
        for s in sheets {
            let bars: Vec<(f64, &Bar)> = s.major.iter().map(|b| (da, b)).chain(s.minor.iter().map(|b| (db, b))).collect();
            out.push(Part::Sheet {
                sheet_type: t.clone(),
                major_diameter: da,
                minor_diameter: db,
                mesh: tubes(&bars, 6),
                name: format!("{} r{} c{}", t.name, s.row + 1, s.column + 1),
            });
        }
    }
    out
}

/// A single sheet's type and wire sizes, for its IFC attributes.
pub fn single_sheet_type(store: &Store, e: &Element) -> Option<(SheetType, f64, f64)> {
    let r = fabric_sheet_recipe(e)?;
    let t = sheet_type(store, &r.sheet_type).ok()?;
    let (a, b) = sheet_type_lookup(store, &t.id)?;
    Some((t, a, b))
}

/// Which system an element's bars sit in, for the clash check: a system and its host belong
/// together.
pub fn reinforces(store: &Store, bar: ElementId, host: ElementId) -> bool {
    let (Some(x), Some(h)) = (store.get(bar), store.get(host)) else { return false };
    uses(x, h.guid().as_str())
}

/// A floor's or wall's outline in its plane (plan x, y for floors; along the wall and up from its
/// base for walls), for tools to start a boundary or path from.
pub fn outline(store: &Store, host: &Element) -> Option<Vec<[f64; 2]>> {
    let kind = rebar::host_kind(store, host)?;
    let (a, _) = faces(kind);
    rebar::host_plane(store, host, kind, a, 0.0, 0.0).ok()?.shapes.into_iter().next().map(|s| s.outer)
}

/// The plane boundaries, paths and points are sketched on for a floor or wall: its top for
/// floors (plan x, y), its exterior face for walls (along the wall, up from its base), in the
/// same coordinates as the commands take.
pub fn sketch_plane(store: &Store, host: &Element) -> Option<crate::work_planes::Plane> {
    match rebar::host_kind(store, host)? {
        HostKind::Floor => {
            let g = crate::floors::regenerate(store, host).ok()?;
            crate::work_planes::Plane::new([0.0, 0.0, g.top], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]).ok()
        }
        HostKind::Wall => {
            let g = crate::walls::regenerate(store, host).ok()?;
            let (d, n) = (g.direction(), g.normal());
            let o = [g.start[0] + n[0] * g.width / 2.0, g.start[1] + n[1] * g.width / 2.0, g.base_z];
            // y = normal × x is up.
            crate::work_planes::Plane::new(o, [d[0], d[1], 0.0], [d[1], -d[0], 0.0]).ok()
        }
        _ => None,
    }
}
