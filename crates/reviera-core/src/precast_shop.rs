// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Precast shop drawings (STR-PRE-006).
//!
//! `precast.shop_drawings` makes one sheet per piece, or per group of identical pieces, from the
//! built-in template: an A3 sheet carrying a *shop drawing* item that is drawn from the model each
//! time the sheet is shown or printed, so it follows the piece. The item draws, in the piece's own
//! frame (along and up a wall piece; along and across a slab piece) at the largest standard scale
//! that fits:
//!
//! - the front view: the outline, openings and lifting anchors, chain dimensions of the ends,
//!   openings and anchors and the overall height, and each opening's size;
//! - the centre-of-gravity symbol, dimensioned from the piece's start and bottom;
//! - a section through the thickness;
//! - a bill of materials: concrete (volume, weight), rebar by number (shape, bar, quantity, length,
//!   mass), fabric sheets and mounting parts;
//! - the piece marks it stands for.
//!
//! Identical pieces share a sheet: same kind, outline, openings, reinforcement numbers, sheets
//! and anchors (to the millimetre, in the piece's frame).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::precast::{self, AssemblyRecipe, PieceKind};
use crate::store::Store;
use crate::txn::Transaction;

type V3 = [f64; 3];

/// A map from the world to a piece's own frame.
type Frame = Box<dyn Fn(&V3) -> [f64; 2]>;

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i))
}

/// A piece in its own frame (metres): its size, openings, centre of gravity and anchors, and
/// its bill of materials.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PieceDrawing {
    pub mark: String,
    pub piece: PieceKind,
    /// Along and up (walls) or along and across (slabs), from the piece's lower-left corner.
    pub length: f64,
    pub height: f64,
    pub thickness: f64,
    pub openings: Vec<[f64; 4]>,
    pub centre: [f64; 2],
    pub anchors: Vec<[f64; 2]>,
    pub volume: f64,
    pub weight: f64,
    /// Rows: number, shape, bar, quantity, length (m), mass (kg).
    pub rebar: Vec<(String, String, String, usize, f64, f64)>,
    /// Rows: number, wires, size (m).
    pub sheets: Vec<(String, String, [f64; 2])>,
    /// Rows: part name, count.
    pub parts: Vec<(String, usize)>,
}

/// A piece's drawing data.
pub fn piece(store: &Store, a: &AssemblyRecipe, assembly: ElementId) -> Result<PieceDrawing> {
    let src = by_guid(store, &a.source).ok_or_else(|| Error::new("precast.source", "the split element is gone"))?;
    let bodies = crate::bodies::all_but_reinforcement(store);
    let meshes: Vec<crate::bodies::Mesh> = a.parts.iter().filter_map(|g| by_guid(store, g)).flat_map(|e| crate::parts::geometry(store, e, &bodies)).collect();
    let pts: Vec<V3> = meshes.iter().flat_map(|m| m.0.iter().copied()).collect();
    if pts.is_empty() {
        return Err(Error::new("precast.source", format!("piece {} has no body", a.mark)));
    }
    let (volume, cg) = crate::precast_parts::volume_centroid(&meshes);
    let density = precast::config(store).density;
    let wall = matches!(a.piece, PieceKind::SolidWall | PieceKind::DoubleWall);
    // The piece's frame: a map to (along, up or across) and the openings in it.
    let (frame, holes, thickness): (Frame, Vec<[f64; 4]>, f64) = if wall {
        let g = crate::walls::regenerate(store, src)?;
        let dir = g.direction();
        let (sx, sy) = (g.start[0], g.start[1]);
        let cuts = crate::inserts::cuts_by_host(store).remove(&src.id()).unwrap_or_default();
        (Box::new(move |q: &V3| [(q[0] - sx) * dir[0] + (q[1] - sy) * dir[1], q[2]]), cuts.iter().map(|c| [c.x[0], c.x[1], c.z[0], c.z[1]]).collect(), g.width)
    } else {
        let f = crate::floors::regenerate(store, src)?;
        let (lo, hi) = pts.iter().fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(l, h), q| ([l[0].min(q[0]), l[1].min(q[1])], [h[0].max(q[0]), h[1].max(q[1])]));
        let swap = hi[1] - lo[1] > hi[0] - lo[0];
        let holes = crate::slab_openings::cut_profiles(store, src, &f.profiles)
            .iter()
            .flat_map(|p| p.holes.iter())
            .map(|h| h.iter().fold([f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY], |b, q| [b[0].min(q[0]), b[1].max(q[0]), b[2].min(q[1]), b[3].max(q[1])]))
            .map(|b| if swap { [b[2], b[3], b[0], b[1]] } else { b })
            .collect();
        (Box::new(move |q: &V3| if swap { [q[1], q[0]] } else { [q[0], q[1]] }), holes, f.thickness)
    };
    let flat: Vec<[f64; 2]> = pts.iter().map(&frame).collect();
    let (x0, x1, y0, y1) = flat.iter().fold((f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY), |b, p| (b.0.min(p[0]), b.1.max(p[0]), b.2.min(p[1]), b.3.max(p[1])));
    let local = |p: [f64; 2]| [p[0] - x0, p[1] - y0];
    let openings: Vec<[f64; 4]> = holes
        .iter()
        .map(|h| [h[0].max(x0) - x0, h[1].min(x1) - x0, h[2].max(y0) - y0, h[3].min(y1) - y0])
        .filter(|h| h[1] > h[0] + 1e-6 && h[3] > h[2] + 1e-6)
        .collect();
    let anchors: Vec<[f64; 2]> = a.mounting.iter().filter_map(|g| by_guid(store, g)).filter_map(crate::precast_parts::recipe).map(|r| local(frame(&r.point))).collect();
    // The bill of materials.
    let numbers = crate::rebar_numbers::numbers(store);
    let mut rebar: BTreeMap<String, (String, String, usize, f64, f64)> = BTreeMap::new();
    let mut sheets = Vec::new();
    let mut parts: BTreeMap<String, usize> = BTreeMap::new();
    let number_of = |id: ElementId| numbers.iter().filter(|(i, _)| i.element == id).map(|(_, n)| n.to_string()).collect::<Vec<_>>().join("–");
    for g in a.reinforcement.iter().chain(&a.mounting) {
        let Some(e) = by_guid(store, g) else { continue };
        if crate::rebar::rebar_recipe(e).is_some() {
            let Ok(i) = crate::rebar::info(store, crate::rebar::InfoParams { rebar_ids: vec![ElementRef::Id(e.id())] }) else { continue };
            let i = &i[0];
            let key = number_of(e.id());
            let row = rebar.entry(key).or_insert((i.shape.clone(), i.bar_type.clone(), 0, 0.0, 0.0));
            row.2 += i.count;
            row.3 = row.3.max(i.rounded_lengths.iter().copied().fold(0.0, f64::max));
            row.4 += i.mass;
        } else if let Some(r) = crate::precast_fabric::recipe(e) {
            sheets.push((number_of(e.id()), format!("{} / {} @ {:.0} / {:.0}", r.rule.major_wire, r.rule.minor_wire, r.rule.major_spacing * 1000.0, r.rule.minor_spacing * 1000.0), [r.length, r.width]));
        } else if let Some(r) = crate::precast_parts::recipe(e) {
            *parts.entry(r.name.clone()).or_default() += 1;
        }
    }
    let _ = assembly;
    Ok(PieceDrawing {
        mark: a.mark.clone(),
        piece: a.piece,
        length: x1 - x0,
        height: y1 - y0,
        thickness,
        openings,
        centre: local(frame(&cg)),
        anchors,
        volume,
        weight: volume * density,
        rebar: rebar.into_iter().map(|(n, (s, b, q, l, m))| (n, s, b, q, l, m)).collect(),
        sheets,
        parts: parts.into_iter().collect(),
    })
}

/// What makes pieces identical: the drawing to the millimetre, less the mark.
fn signature(p: &PieceDrawing) -> String {
    let mm = |v: f64| (v * 1000.0).round() as i64;
    let mut s = format!("{:?}|{}|{}|{}", p.piece, mm(p.length), mm(p.height), mm(p.thickness));
    for o in &p.openings {
        s += &format!("|o{},{},{},{}", mm(o[0]), mm(o[1]), mm(o[2]), mm(o[3]));
    }
    let mut anchors: Vec<(i64, i64)> = p.anchors.iter().map(|a| (mm(a[0]), mm(a[1]))).collect();
    anchors.sort();
    s += &format!("|a{anchors:?}");
    for r in &p.rebar {
        s += &format!("|r{}:{}:{}:{}:{}", r.0, r.1, r.2, r.3, mm(r.4));
    }
    for r in &p.sheets {
        s += &format!("|s{}:{}:{}x{}", r.0, r.1, mm(r.2[0]), mm(r.2[1]));
    }
    s
}

// ---- Drawing ----

/// Lines (width mm, from, to) and texts (baseline-left, height mm, text) on paper, millimetres.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct Graphics {
    pub lines: Vec<(f64, [f64; 2], [f64; 2])>,
    pub texts: Vec<([f64; 2], f64, String)>,
}

impl Graphics {
    fn line(&mut self, w: f64, a: [f64; 2], b: [f64; 2]) {
        self.lines.push((w, a, b));
    }

    fn rect(&mut self, w: f64, a: [f64; 2], b: [f64; 2]) {
        self.line(w, a, [b[0], a[1]]);
        self.line(w, [b[0], a[1]], b);
        self.line(w, b, [a[0], b[1]]);
        self.line(w, [a[0], b[1]], a);
    }

    fn text(&mut self, at: [f64; 2], h: f64, t: impl Into<String>) {
        self.texts.push((at, h, t.into()));
    }

    /// A horizontal chain dimension at height `y` of the stations `xs`, the extension lines from
    /// `from_y`.
    fn chain_x(&mut self, xs: &[f64], y: f64, from_y: f64, scale: f64) {
        let up = if y > from_y { 1.0 } else { -1.0 };
        for x in xs {
            self.line(0.18, [*x, from_y + up * 1.0], [*x, y + up * 1.5]);
            self.line(0.25, [x - 1.0, y - 1.0], [x + 1.0, y + 1.0]);
        }
        if let (Some(a), Some(b)) = (xs.first(), xs.last()) {
            self.line(0.18, [*a, y], [*b, y]);
        }
        for w in xs.windows(2) {
            let v = format!("{:.0}", (w[1] - w[0]) * scale);
            let tw = v.len() as f64 * 1.6;
            self.text([(w[0] + w[1]) / 2.0 - tw / 2.0, y + 0.8], 2.5, v);
        }
    }

    /// A vertical chain dimension at `x` of the stations `ys`, the extension lines from `from_x`.
    fn chain_y(&mut self, ys: &[f64], x: f64, from_x: f64, scale: f64) {
        let out = if x > from_x { 1.0 } else { -1.0 };
        for y in ys {
            self.line(0.18, [from_x + out * 1.0, *y], [x + out * 1.5, *y]);
            self.line(0.25, [x - 1.0, y - 1.0], [x + 1.0, y + 1.0]);
        }
        if let (Some(a), Some(b)) = (ys.first(), ys.last()) {
            self.line(0.18, [x, *a], [x, *b]);
        }
        for w in ys.windows(2) {
            let v = format!("{:.0}", (w[1] - w[0]) * scale);
            self.text([x - 1.0 - v.len() as f64 * 1.6 * if out < 0.0 { 1.0 } else { 0.0 } + if out > 0.0 { 1.5 } else { -0.5 }, (w[0] + w[1]) / 2.0 - 1.2], 2.5, v);
        }
    }
}

const SCALES: [f64; 6] = [5.0, 10.0, 20.0, 25.0, 50.0, 100.0];

/// A shop drawing on paper, its top-left corner at `at` (mm): the views and the bill of
/// materials for `pieces` (identical; the first drawn), and its size.
pub fn draw(pieces: &[PieceDrawing], at: [f64; 2]) -> (Graphics, [f64; 2]) {
    let mut g = Graphics::default();
    let Some(p) = pieces.first() else { return (g, [0.0, 0.0]) };
    // The front view: 240 × 130 mm at most.
    let s = SCALES.iter().copied().find(|s| p.length * 1000.0 / s <= 240.0 && p.height * 1000.0 / s <= 130.0).unwrap_or(200.0);
    let k = 1000.0 / s;
    let (ox, oy) = (at[0] + 25.0, at[1] - 42.0 - p.height * k);
    let to = |q: [f64; 2]| [ox + q[0] * k, oy + q[1] * k];
    let marks: Vec<&str> = pieces.iter().map(|q| q.mark.as_str()).collect();
    g.text([at[0], at[1] - 6.0], 5.0, format!("Shop drawing {}", marks[0]));
    g.text([at[0], at[1] - 12.0], 2.5, format!("{} x {}: {}", p.piece.name(), pieces.len(), marks.join(", ")));
    g.text([ox, oy + p.height * k + 17.0], 3.5, format!("Front view 1 : {s:.0}"));
    g.rect(0.5, to([0.0, 0.0]), to([p.length, p.height]));
    for o in &p.openings {
        g.rect(0.35, to([o[0], o[2]]), to([o[1], o[3]]));
        g.line(0.18, to([o[0], o[2]]), to([o[1], o[3]]));
        g.line(0.18, to([o[0], o[3]]), to([o[1], o[2]]));
        let label = format!("{:.0} x {:.0}", (o[1] - o[0]) * 1000.0, (o[3] - o[2]) * 1000.0);
        let c = to([(o[0] + o[1]) / 2.0, (o[2] + o[3]) / 2.0]);
        g.text([c[0] - label.len() as f64 * 0.8, c[1] + 1.5], 2.5, label);
    }
    // Lifting anchors: a mark on the edge each.
    for a in &p.anchors {
        let q = to(*a);
        g.line(0.35, [q[0] - 1.5, q[1] + 2.5], [q[0], q[1]]);
        g.line(0.35, [q[0] + 1.5, q[1] + 2.5], [q[0], q[1]]);
    }
    // Chains: ends and openings below; anchors above; the height and openings at the left.
    let mut xs: Vec<f64> = vec![0.0, p.length];
    xs.extend(p.openings.iter().flat_map(|o| [o[0], o[1]]));
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    g.chain_x(&xs.iter().map(|x| ox + x * k).collect::<Vec<_>>(), oy - 8.0, oy, s);
    g.chain_x(&[ox, ox + p.length * k], oy - 16.0, oy, s);
    if !p.anchors.is_empty() {
        let mut ax: Vec<f64> = vec![0.0, p.length];
        ax.extend(p.anchors.iter().map(|a| a[0]));
        ax.sort_by(f64::total_cmp);
        ax.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        g.chain_x(&ax.iter().map(|x| ox + x * k).collect::<Vec<_>>(), oy + p.height * k + 8.0, oy + p.height * k, s);
    }
    let mut ys: Vec<f64> = vec![0.0, p.height];
    ys.extend(p.openings.iter().flat_map(|o| [o[2], o[3]]));
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    g.chain_y(&ys.iter().map(|y| oy + y * k).collect::<Vec<_>>(), ox - 8.0, ox, s);
    // The centre of gravity: a circle with two quarters marked, and its distances.
    let c = to(p.centre);
    let r = 2.5;
    let n = 24;
    for i in 0..n {
        let (a0, a1) = (i as f64 / n as f64 * std::f64::consts::TAU, (i + 1) as f64 / n as f64 * std::f64::consts::TAU);
        g.line(0.25, [c[0] + r * a0.cos(), c[1] + r * a0.sin()], [c[0] + r * a1.cos(), c[1] + r * a1.sin()]);
    }
    g.line(0.25, [c[0] - r, c[1]], [c[0] + r, c[1]]);
    g.line(0.25, [c[0], c[1] - r], [c[0], c[1] + r]);
    for q in 0..2 {
        // The first and third quarters filled with hatch.
        for t in 1..5 {
            let a = (q as f64 * 2.0 + t as f64 / 5.0) * std::f64::consts::FRAC_PI_2;
            g.line(0.18, c, [c[0] + r * a.cos(), c[1] + r * a.sin()]);
        }
    }
    g.text([c[0] + 3.5, c[1] + 1.0], 2.5, format!("CoG {:.0} / {:.0}", p.centre[0] * 1000.0, p.centre[1] * 1000.0));
    // A section through the thickness, to the right.
    let sx = ox + p.length * k + 25.0;
    let tk = p.thickness * k;
    g.text([sx, oy + p.height * k + 17.0], 3.5, "Section A-A");
    g.rect(0.5, [sx, oy], [sx + tk, oy + p.height * k]);
    g.chain_x(&[sx, sx + tk], oy - 8.0, oy, s);
    // The bill of materials, below.
    let mut rows: Vec<[String; 6]> = vec![["Item".into(), "Number".into(), "Description".into(), "Qty".into(), "Length".into(), "Mass / volume".into()]];
    rows.push(["Concrete".into(), String::new(), format!("{:.0} mm", p.thickness * 1000.0), "1".into(), String::new(), format!("{:.3} m3  {:.2} t", p.volume, p.weight)]);
    for (n, shape, bar, q, l, m) in &p.rebar {
        rows.push(["Rebar".into(), n.clone(), format!("{bar} shape {shape}"), q.to_string(), format!("{:.0}", l * 1000.0), format!("{m:.1} kg")]);
    }
    for (n, wires, size) in &p.sheets {
        rows.push(["Fabric".into(), n.clone(), wires.clone(), "1".into(), format!("{:.0} x {:.0}", size[0] * 1000.0, size[1] * 1000.0), String::new()]);
    }
    for (name, q) in &p.parts {
        rows.push(["Mounting part".into(), String::new(), name.clone(), q.to_string(), String::new(), String::new()]);
    }
    let widths = [28.0, 18.0, 55.0, 12.0, 28.0, 42.0];
    let (tx, ty) = (at[0], oy - 28.0);
    g.text([tx, ty + 2.0], 3.5, "Bill of materials");
    let row_h = 5.5;
    let total: f64 = widths.iter().sum();
    for (i, row) in rows.iter().enumerate() {
        let top = ty - i as f64 * row_h;
        g.line(if i <= 1 { 0.35 } else { 0.18 }, [tx, top], [tx + total, top]);
        let mut x = tx;
        for (c, w) in row.iter().zip(widths) {
            g.text([x + 1.2, top - row_h + 1.6], 2.5, c.clone());
            x += w;
        }
    }
    let bottom = ty - rows.len() as f64 * row_h;
    g.line(0.35, [tx, bottom], [tx + total, bottom]);
    let mut x = tx;
    for w in std::iter::once(0.0).chain(widths) {
        x += w;
        g.line(0.18, [x, ty], [x, bottom]);
    }
    let size = [(sx + tk + 20.0 - at[0]).max(total), at[1] - bottom];
    (g, size)
}

/// A sheet's shop drawing as drawn now (for the app).
pub fn sheet_graphics(store: &Store, item: &crate::sheets::ShopDrawingItem) -> Option<(Graphics, [f64; 2])> {
    let pieces: Vec<PieceDrawing> = item
        .assemblies
        .iter()
        .filter_map(|g| {
            let e = by_guid(store, g)?;
            let a = precast::assembly_recipe(e)?;
            piece(store, &a, e.id()).ok()
        })
        .collect();
    (!pieces.is_empty()).then(|| draw(&pieces, item.position))
}

// ---- Command ----

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ShopParams {
    pub assembly_ids: Vec<ElementRef>,
    /// The template: only the built-in `default` (A3) yet.
    #[serde(default)]
    pub template_id: Option<String>,
    /// One sheet per group of identical pieces (default), else one per piece.
    #[serde(default = "yes")]
    pub group_identical: bool,
}

fn yes() -> bool {
    true
}

/// Makes shop drawing sheets for precast pieces (`precast.shop_drawings`).
pub(crate) fn shop_drawings(txn: &mut Transaction<'_>, p: ShopParams) -> Result<Vec<ElementId>> {
    if let Some(t) = &p.template_id
        && t != "default"
    {
        return Err(Error::new("precast.template", format!("no shop-drawing template {t}: only the built-in default (A3) yet")));
    }
    let store = txn.store().clone();
    // Each piece's drawing, grouped by what makes them identical.
    let mut groups: Vec<(String, Vec<(String, PieceDrawing)>)> = Vec::new();
    for r in &p.assembly_ids {
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        let a = precast::assembly_recipe(e).ok_or_else(|| Error::new("precast.not_assembly", format!("element {id} is not a precast assembly")))?;
        let d = piece(&store, &a, id)?;
        let key = if p.group_identical { signature(&d) } else { format!("{}#{}", signature(&d), e.guid()) };
        let guid = e.guid().to_string();
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push((guid, d)),
            None => groups.push((key, vec![(guid, d)])),
        }
    }
    // Sheets drawn for these pieces before go first.
    let guids: Vec<String> = groups.iter().flat_map(|g| g.1.iter().map(|x| x.0.clone())).collect();
    for e in store.iter() {
        if let Some(d) = crate::sheets::sheet_data(e)
            && !d.shop_drawings.is_empty()
            && d.shop_drawings.iter().all(|s| s.assemblies.iter().all(|a| guids.contains(a)))
        {
            txn.delete(e.id())?;
        }
    }
    let mut out = Vec::new();
    for (_, members) in groups {
        let marks: Vec<&str> = members.iter().map(|m| m.1.mark.as_str()).collect();
        let name = if marks.len() > 1 { format!("Shop drawing {} (and {} more)", marks[0], marks.len() - 1) } else { format!("Shop drawing {}", marks[0]) };
        let sheet = crate::sheets::create(txn, serde_json::from_value(serde_json::json!({ "number": format!("PS-{}", marks[0]), "name": name, "size": "A3" })).expect("valid"))?;
        crate::sheets::add_shop_drawing(txn, sheet, members.iter().map(|m| m.0.clone()).collect(), [30.0, 287.0])?;
        out.push(sheet);
    }
    Ok(out)
}
