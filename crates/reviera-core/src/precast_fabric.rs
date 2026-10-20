// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Custom fabric sheets for precast pieces (STR-PRE-004).
//!
//! A rule (in the precast configuration) gives the wires' types and spacings, the edge distance
//! at a piece's outer edges and at its joints, the clearance round openings, the shortest wire
//! kept, the largest sheet and the lap between sheets, and which face(s) to fill. Each piece gets
//! sheets shaped to it: one when it fits the largest sheet, else as many as it takes, split square
//! to the main wires (and across them when too wide) and lapped. Wires stop at the clearance line
//! round each opening and at the edge distance from the piece's edges; wires shorter than the
//! minimum are left out and counted.
//!
//! A sheet is an `IfcReinforcingMesh` with `recipe.custom_fabric_sheet`, which keeps the rule's
//! values as used, its place in its host's plane and the piece's region; its wires are worked out
//! from the host each time. It is numbered like a fabric sheet (identical sheets share a number),
//! carries `rxd:precast_generated` and belongs to the piece's assembly.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::precast::{self, PieceKind};
use crate::rebar::{self, Bar, HostKind};
use crate::store::Store;
use crate::txn::Transaction;
use crate::NewElement;

pub const RECIPE: &str = "recipe.custom_fabric_sheet";

/// A custom fabric sheet rule (part of the precast configuration).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FabricRule {
    pub id: String,
    pub name: String,
    /// Wire types (`fabric_wire_type.list`) of the main wires and the cross wires.
    pub major_wire: String,
    pub minor_wire: String,
    pub major_spacing: f64,
    pub minor_spacing: f64,
    /// From the piece's outer edges, and from its joints with other pieces, to the wires' ends
    /// and the outermost wires (metres).
    pub edge_outer: f64,
    pub edge_joint: f64,
    /// Round openings.
    pub opening_clearance: f64,
    /// Wires shorter than this are left out.
    pub min_wire_length: f64,
    pub max_length: f64,
    pub max_width: f64,
    /// Between sheets of one piece.
    pub lap: f64,
    /// `outer` (a wall's exterior, a slab's bottom), `inner` or `both`.
    pub layer: String,
}

impl FabricRule {
    /// The provisional starting rule (EXPERT_REVIEW.md E-14).
    pub fn default_rule() -> Self {
        Self {
            id: "C8-150".into(),
            name: "Ø8 @ 150 both ways".into(),
            major_wire: "w8".into(),
            minor_wire: "w8".into(),
            major_spacing: 0.15,
            minor_spacing: 0.15,
            edge_outer: 0.05,
            edge_joint: 0.05,
            opening_clearance: 0.05,
            min_wire_length: 0.3,
            max_length: 6.0,
            max_width: 2.5,
            lap: 0.3,
            layer: "both".into(),
        }
    }
}

/// What makes a rule unusable, by its field.
pub fn rule_problems(r: &FabricRule) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut bad = |k: &str, m: &str| out.push((format!("fabric_rules.{}.{k}", r.id), m.to_owned()));
    for (k, v) in [("major_spacing", r.major_spacing), ("minor_spacing", r.minor_spacing), ("max_length", r.max_length), ("max_width", r.max_width), ("min_wire_length", r.min_wire_length)] {
        if v <= 0.0 {
            bad(k, "must be more than 0");
        }
    }
    for (k, v) in [("edge_outer", r.edge_outer), ("edge_joint", r.edge_joint), ("opening_clearance", r.opening_clearance), ("lap", r.lap)] {
        if v < 0.0 {
            bad(k, "cannot be negative");
        }
    }
    if r.lap >= r.max_length.min(r.max_width) {
        bad("lap", "the lap must be shorter than the largest sheet");
    }
    if !["outer", "inner", "both"].contains(&r.layer.as_str()) {
        bad("layer", "the layer is outer, inner or both");
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CustomSheetRecipe {
    pub kind: String,
    pub host: String,
    pub face: String,
    /// The piece's assembly (GlobalId) and the rule as used.
    pub assembly: String,
    pub rule: FabricRule,
    /// The sheet's first corner in the host's plane, its main wires' direction, length and width.
    pub origin: [f64; 2],
    pub u: [f64; 2],
    pub length: f64,
    pub width: f64,
    /// The piece in the host's plane.
    pub region: Vec<[f64; 2]>,
    /// Which sides of the sheet are the piece's edges: start, end (along), and the two sides.
    pub at_piece_edge: [bool; 4],
}

pub fn recipe(e: &Element) -> Option<CustomSheetRecipe> {
    let v = e.param(crate::walls::RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A sheet's wires as worked out.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Wires {
    pub major: Vec<Bar>,
    pub minor: Vec<Bar>,
    pub major_diameter: f64,
    pub minor_diameter: f64,
    /// Wires left out as shorter than the minimum.
    pub dropped: usize,
}

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i))
}

fn inside(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut c = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            c = !c;
        }
    }
    c
}

/// A host's openings in its plane, as rectangles (along and up for walls; plan for floors).
fn openings(store: &Store, host: &Element, kind: HostKind, plane: &rebar::HostPlane) -> Vec<[f64; 4]> {
    let bbox = |r: &[[f64; 2]]| r.iter().fold([f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY], |b, p| [b[0].min(p[0]), b[1].max(p[0]), b[2].min(p[1]), b[3].max(p[1])]);
    match kind {
        HostKind::Wall => {
            let base = crate::walls::regenerate(store, host).map_or(0.0, |g| g.base_z);
            crate::inserts::cuts_by_host(store).remove(&host.id()).unwrap_or_default().iter().map(|c| [c.x[0], c.x[1], c.z[0] - base, c.z[1] - base]).collect()
        }
        _ => plane.shapes.iter().flat_map(|s| s.holes.iter().map(|h| bbox(h))).collect(),
    }
}

/// A custom sheet's wires.
pub fn wires(store: &Store, r: &CustomSheetRecipe) -> Result<Wires> {
    let host = by_guid(store, &r.host).ok_or_else(|| Error::new("rebar.no_host", "the sheet's host is gone"))?;
    let kind = rebar::host_kind(store, host).ok_or_else(|| Error::new("rebar.no_host", "the host cannot hold rebar"))?;
    let all = crate::rebar_systems::wire_types(store);
    let wire = |id: &str| all.iter().find(|w| w.id == id || w.name == id).cloned().ok_or_else(|| Error::new("fabric_wire_type.unknown", format!("no wire type {id}")));
    let (wa, wb) = (wire(&r.rule.major_wire)?, wire(&r.rule.minor_wire)?);
    // Main wires at the cover, cross wires inside them.
    let pa = rebar::host_plane(store, host, kind, &r.face, wa.diameter, 0.0)?;
    let pb = rebar::host_plane(store, host, kind, &r.face, wb.diameter, wa.diameter)?;
    // The piece less its openings and their clearance.
    let holes = openings(store, host, kind, &pa);
    let c = r.rule.opening_clearance;
    let grown: Vec<Vec<[f64; 2]>> = holes.iter().map(|h| vec![[h[0] - c, h[2] - c], [h[1] + c, h[2] - c], [h[1] + c, h[3] + c], [h[0] - c, h[3] + c]]).collect();
    let piece = crate::polygons::intersection(&pa.shapes, &[crate::polygons::Shape { outer: r.region.clone(), holes: Vec::new() }]);
    let clear = crate::polygons::difference(&piece, &grown);
    let rings: Vec<Vec<[f64; 2]>> = clear.into_iter().flat_map(|s| std::iter::once(s.outer).chain(s.holes)).collect();
    let solid: Vec<Vec<[f64; 2]>> = pa.shapes.iter().map(|s| s.outer.clone()).collect();
    let in_opening = |p: [f64; 2]| grown.iter().any(|g| inside(g, p));
    let in_host = |p: [f64; 2]| solid.iter().any(|g| inside(g, p));
    let (u, v) = (r.u, [-r.u[1], r.u[0]]);
    let at = |a: f64, b: f64| [r.origin[0] + u[0] * a + v[0] * b, r.origin[1] + u[1] * a + v[1] * b];
    let mut dropped = 0;
    // Wires from `p` along `dir` for `l`, within the piece. An end at a side of the sheet that is
    // not the piece's edge stays (another sheet laps there); any other end stops at the clearance
    // line round an opening (already cut out), the joint distance where the host goes on past the
    // piece, or the outer distance at the host's own edge.
    let mut run = |plane: &rebar::HostPlane, p: [f64; 2], dir: [f64; 2], l: f64, edges: (bool, bool)| -> Vec<Bar> {
        let point = |t: f64| [p[0] + dir[0] * t, p[1] + dir[1] * t];
        let trim = |t: f64, out_dir: f64, at_sheet_side: bool, piece_edge: bool| {
            if at_sheet_side && !piece_edge {
                return 0.0;
            }
            let q = point(t + out_dir * 1e-4);
            if in_opening(q) { 0.0 } else if in_host(q) { r.rule.edge_joint } else { r.rule.edge_outer }
        };
        let mut out = Vec::new();
        for (t0, t1) in rebar::chord(&rings, p, dir) {
            let (t0, t1) = (t0.max(0.0), t1.min(l));
            let a = t0 + trim(t0, -1.0, t0 <= 1e-9, edges.0);
            let b = t1 - trim(t1, 1.0, t1 >= l - 1e-9, edges.1);
            if b - a < r.rule.min_wire_length {
                if b - a > 1e-6 {
                    dropped += 1;
                }
                continue;
            }
            out.push(Bar { points: vec![plane.at(point(a)), plane.at(point(b))] });
        }
        out
    };
    // The outermost wires at the edge distance from a piece edge, else half a spacing in.
    let side = |edge: bool, s: f64| if edge { r.rule.edge_outer.max(r.rule.edge_joint) } else { s / 2.0 };
    let e = r.at_piece_edge;
    let mut major = Vec::new();
    let mut b = side(e[2], r.rule.major_spacing);
    while b <= r.width - side(e[3], r.rule.major_spacing) + 1e-9 {
        major.extend(run(&pa, at(0.0, b), u, r.length, (e[0], e[1])));
        b += r.rule.major_spacing;
    }
    let mut minor = Vec::new();
    let mut a = side(e[0], r.rule.minor_spacing);
    while a <= r.length - side(e[1], r.rule.minor_spacing) + 1e-9 {
        minor.extend(run(&pb, at(a, 0.0), v, r.width, (e[2], e[3])));
        a += r.rule.minor_spacing;
    }
    Ok(Wires { major, minor, major_diameter: wa.diameter, minor_diameter: wb.diameter, dropped })
}

/// A custom sheet's body.
pub fn mesh(store: &Store, e: &Element) -> Option<crate::bodies::Mesh> {
    let r = recipe(e)?;
    let w = wires(store, &r).ok()?;
    let a = rebar::bars_mesh(&w.major.iter().collect::<Vec<_>>(), w.major_diameter);
    let b = rebar::bars_mesh(&w.minor.iter().collect::<Vec<_>>(), w.minor_diameter);
    Some(crate::steel_fab::join(vec![a, b]))
}

/// The sheet as a sheet type, for IFC (MeshLength, MeshWidth, wires and spacings).
pub fn as_sheet_type(store: &Store, e: &Element) -> Option<(crate::rebar_systems::SheetType, f64, f64)> {
    let r = recipe(e)?;
    let w = wires(store, &r).ok()?;
    Some((
        crate::rebar_systems::SheetType {
            id: format!("custom:{}", r.rule.id),
            name: format!("Custom {} {:.0} × {:.0}", r.rule.name, r.length * 1000.0, r.width * 1000.0),
            major_wire: r.rule.major_wire.clone(),
            minor_wire: r.rule.minor_wire.clone(),
            major_spacing: r.rule.major_spacing,
            minor_spacing: r.rule.minor_spacing,
            length: r.length,
            width: r.width,
            major_overhang: None,
            minor_overhang: None,
        },
        w.major_diameter,
        w.minor_diameter,
    ))
}

/// A sheet's numbering identity: its wires and their lengths and places (so identical pieces'
/// sheets share a number).
pub fn identity(store: &Store, e: &Element) -> Option<String> {
    let r = recipe(e)?;
    let w = wires(store, &r).ok()?;
    let local = |b: &Bar| {
        let (p, q) = (b.points[0], b.points[b.points.len() - 1]);
        format!("{:.0}", (p[0] - q[0]).hypot(p[1] - q[1]).hypot(p[2] - q[2]) * 1000.0)
    };
    let lengths: Vec<String> = w.major.iter().chain(&w.minor).map(local).collect();
    let text = format!("{}|{}|{}|{}|{}", r.rule.major_wire, r.rule.minor_wire, r.rule.major_spacing, r.rule.minor_spacing, lengths.join(","));
    Some(format!("custom|{:016x}", text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))))
}

/// Whether an element goes with `guid`: a sheet of that assembly or host.
pub fn uses(e: &Element, guid: &str) -> bool {
    recipe(e).is_some_and(|r| r.assembly == guid || r.host == guid)
}

// ---- Command ----

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GenerateParams {
    pub assembly_ids: Vec<ElementRef>,
    /// A rule's id; the configuration's first when omitted.
    #[serde(default)]
    pub rule_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SheetReport {
    pub assembly: ElementId,
    pub mark: String,
    pub sheets: Vec<ElementId>,
    pub removed: usize,
    pub kept: Vec<crate::precast_rebar::Kept>,
    /// Wires left out as shorter than the minimum.
    pub dropped_wires: usize,
    pub skipped: Vec<String>,
}

fn recipe_hash(e: &Element) -> String {
    let s = e.param(crate::walls::RECIPE_KEY).map(serde_json::Value::to_string).unwrap_or_default();
    format!("{:016x}", s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)))
}

/// Custom fabric sheets for precast pieces (`precast.generate_custom_fabric`).
pub(crate) fn generate(txn: &mut Transaction<'_>, p: GenerateParams) -> Result<Vec<SheetReport>> {
    let store = txn.store().clone();
    let c = precast::config(&store);
    let rule = match &p.rule_id {
        Some(id) => c.fabric_rules.iter().find(|r| r.id == *id).cloned().ok_or_else(|| {
            Error::new("precast.rule", format!("no custom fabric rule {id} (rules: {})", c.fabric_rules.iter().map(|r| r.id.as_str()).collect::<Vec<_>>().join(", ")))
        })?,
        None => c.fabric_rules.first().cloned().ok_or_else(|| Error::new("precast.rule", "the configuration has no custom fabric rule"))?,
    };
    if let Some((k, m)) = rule_problems(&rule).first() {
        return Err(Error::new("precast_config.invalid", format!("{k}: {m}")));
    }
    let mut out = Vec::new();
    for r in &p.assembly_ids {
        let store = txn.store().clone();
        let aid = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let a = store.get(aid).and_then(precast::assembly_recipe).ok_or_else(|| Error::new("precast.not_assembly", format!("element {aid} is not a precast assembly")))?;
        if matches!(a.piece, PieceKind::HollowCore) {
            return Err(Error::new("precast.kind", format!("{} is a hollow-core slab: prestressed, it takes no fabric", a.mark)));
        }
        let aguid = store.get(aid).expect("resolved").guid().to_string();
        let mut rep = SheetReport { assembly: aid, mark: a.mark.clone(), sheets: Vec::new(), removed: 0, kept: Vec::new(), dropped_wires: 0, skipped: Vec::new() };
        // The last run's sheets: removed if as made, kept if edited.
        let mut held: Vec<String> = a.reinforcement.iter().filter(|g| by_guid(&store, g).is_some_and(|e| recipe(e).is_none())).cloned().collect();
        for e in store.iter().filter(|e| recipe(e).is_some_and(|x| x.assembly == aguid)) {
            let made = e.param(crate::precast_rebar::GENERATED_KEY).and_then(|g| g.get("hash")).and_then(serde_json::Value::as_str).unwrap_or("");
            if made == recipe_hash(e) {
                txn.delete(e.id())?;
                rep.removed += 1;
            } else {
                rep.kept.push(crate::precast_rebar::Kept { id: e.id(), why: "generated, then edited: kept".into() });
                held.push(e.guid().to_string());
            }
        }
        let src = by_guid(&store, &a.source).ok_or_else(|| Error::new("precast.source", "the split element is gone"))?;
        let kind = rebar::host_kind(&store, src).ok_or_else(|| Error::new("precast.source", "the split element cannot hold rebar"))?;
        let wall = kind == HostKind::Wall;
        // The piece's extent in the host's plane.
        let bodies = crate::bodies::all_but_reinforcement(&store);
        let pts: Vec<[f64; 3]> = a.parts.iter().filter_map(|g| by_guid(&store, g)).flat_map(|e| crate::parts::geometry(&store, e, &bodies).into_iter().flat_map(|m| m.0)).collect();
        if pts.is_empty() {
            rep.skipped.push("the piece has no body".into());
            out.push(rep);
            continue;
        }
        // The piece's points in the host's plane: along and up a wall, plan for a floor.
        let wall_frame = if wall { Some(crate::walls::regenerate(&store, src)?) } else { None };
        let flat: Vec<[f64; 2]> = pts
            .iter()
            .map(|q| match &wall_frame {
                Some(g) => {
                    let dir = g.direction();
                    [(q[0] - g.start[0]) * dir[0] + (q[1] - g.start[1]) * dir[1], q[2] - g.base_z]
                }
                None => [q[0], q[1]],
            })
            .collect();
        let (x0, x1, y0, y1) = flat.iter().fold((f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY), |b, p| (b.0.min(p[0]), b.1.max(p[0]), b.2.min(p[1]), b.3.max(p[1])));
        let region = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        // Main wires along the piece's longer side.
        let along_x = x1 - x0 >= y1 - y0;
        let (u, len_l, len_w) = if along_x { ([1.0, 0.0], x1 - x0, y1 - y0) } else { ([0.0, 1.0], y1 - y0, x1 - x0) };
        let v = [-u[1], u[0]];
        let origin0 = if along_x { [x0, y0] } else { [x1, y0] };
        let count = |total: f64, max: f64| if total <= max + 1e-9 { 1 } else { ((total - rule.lap) / (max - rule.lap) - 1e-9).ceil().max(1.0) as usize };
        let (nl, nw) = (count(len_l, rule.max_length), count(len_w, rule.max_width));
        let (sl, sw) = ((len_l + (nl - 1) as f64 * rule.lap) / nl as f64, (len_w + (nw - 1) as f64 * rule.lap) / nw as f64);
        let faces: Vec<&str> = match (rule.layer.as_str(), wall) {
            ("outer", true) => vec!["exterior"],
            ("inner", true) => vec!["interior"],
            (_, true) => vec!["exterior", "interior"],
            ("outer", false) => vec!["bottom"],
            ("inner", false) => vec!["top"],
            (_, false) => vec!["bottom", "top"],
        };
        for face in faces {
            for i in 0..nl {
                for j in 0..nw {
                    let (a0, b0) = (i as f64 * (sl - rule.lap), j as f64 * (sw - rule.lap));
                    let origin = [origin0[0] + u[0] * a0 + v[0] * b0, origin0[1] + u[1] * a0 + v[1] * b0];
                    let rec = CustomSheetRecipe {
                        kind: RECIPE.into(),
                        host: a.source.clone(),
                        face: face.into(),
                        assembly: aguid.clone(),
                        rule: rule.clone(),
                        origin,
                        u,
                        length: sl,
                        width: sw,
                        region: region.clone(),
                        at_piece_edge: [i == 0, i + 1 == nl, j == 0, j + 1 == nw],
                    };
                    let w = match wires(txn.store(), &rec) {
                        Ok(w) => w,
                        Err(e) => {
                            rep.skipped.push(format!("{face} sheet {}: {}", i * nw + j + 1, e.message));
                            continue;
                        }
                    };
                    rep.dropped_wires += w.dropped;
                    if w.major.is_empty() && w.minor.is_empty() {
                        rep.skipped.push(format!("{face} sheet {}: no wires left", i * nw + j + 1));
                        continue;
                    }
                    let id = txn.create(
                        NewElement::new("IfcReinforcingMesh")
                            .name(format!("{} {face} {}", a.mark, i * nw + j + 1))
                            .param(crate::walls::RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))
                            .param(crate::rebar_numbers::PARTITION_KEY, json!(a.mark)),
                    )?;
                    let e = txn.store().get(id).expect("made");
                    let hash = recipe_hash(e);
                    let guid = e.guid().to_string();
                    txn.set_param(id, crate::precast_rebar::GENERATED_KEY, json!({ "assembly": aguid, "type": rule.id, "hash": hash }))?;
                    rep.sheets.push(id);
                    held.push(guid);
                }
            }
        }
        let mut rec = txn.store().get(aid).and_then(precast::assembly_recipe).expect("an assembly");
        rec.reinforcement = held;
        txn.set_param(aid, crate::walls::RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
        out.push(rep);
    }
    crate::rebar_numbers::persist(txn)?;
    Ok(out)
}
