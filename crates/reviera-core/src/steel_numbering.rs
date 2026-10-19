// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Steel numbering and fabrication output (STR-STL-010).
//!
//! **Parts** are the steel columns, beams and braces, the plates placed by hand and the plates
//! of connections. Identical parts get the same **part mark**: a member by its section, length,
//! holes and cuts in its own frame (the same whichever way round it was drawn); a plate by its
//! thickness and outline with its holes, whatever its position or turn. Marks are a prefix by
//! kind and a number: `c1` columns, `b1` beams, `br1` braces, `p1` plates.
//!
//! **Assemblies** are a main member and everything shop-welded to it: by its own shop welds
//! (STR-STL-007), and a connection's plates welded to their member (a shear plate or gusset to
//! the main member, an end plate to the beam, a base plate to the column). Site welds and bolts
//! do not join. Identical assemblies (the same main mark and the same parts in the same places)
//! get the same **assembly mark**, the prefix in capitals: `C1`, `B1`, `BR1`, `P1`.
//!
//! The part mark is written to each element's `Tag`, and both marks to `rxd:steel_marks`; each
//! assembly is an IfcElementAssembly (USERDEFINED STEEL_ASSEMBLY) with its mark as `Tag`, made
//! again on every run.
//!
//! The **NC export** turns each part mark into one DSTV NC1 piece (`reviera-dstv`): a member's
//! profile, length and holes on the web (`v`) and flanges (`o`, `u`); a plate's outline (`AK`)
//! and holes in its smallest rectangle.
//!
//! Not yet: member copes and cuts in NC contours (members are written with their length and
//! holes), skew cut angles, numbering options per kind beyond prefixes, keeping marks when the
//! model changes between runs.

use std::collections::{BTreeMap, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::Result;
use crate::id::{ElementId, GlobalId};
use crate::steel_fab::Part;
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const MARKS_KEY: &str = "rxd:steel_marks";
pub const ASSEMBLY_RECIPE: &str = "recipe.steel_assembly";

type V3 = [f64; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l < 1e-12 { a } else { mul(a, 1.0 / l) }
}

/// To the half millimetre, as text.
fn mm(v: f64) -> String {
    let h = (v * 2000.0).round() / 2.0;
    format!("{}", if h == 0.0 { 0.0 } else { h })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Column,
    Beam,
    Brace,
    Plate,
}

/// Numbering options: each kind's prefix (part marks lowercase, assembly marks uppercase) and the
/// first number.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Options {
    #[serde(default = "column")]
    pub column: String,
    #[serde(default = "beam")]
    pub beam: String,
    #[serde(default = "brace")]
    pub brace: String,
    #[serde(default = "plate")]
    pub plate: String,
    #[serde(default = "one")]
    pub start: usize,
}

fn column() -> String {
    "c".into()
}
fn beam() -> String {
    "b".into()
}
fn brace() -> String {
    "br".into()
}
fn plate() -> String {
    "p".into()
}
fn one() -> usize {
    1
}

impl Default for Options {
    fn default() -> Self {
        Self { column: column(), beam: beam(), brace: brace(), plate: plate(), start: 1 }
    }
}

impl Options {
    fn prefix(&self, k: PartKind) -> &str {
        match k {
            PartKind::Column => &self.column,
            PartKind::Beam => &self.beam,
            PartKind::Brace => &self.brace,
            PartKind::Plate => &self.plate,
        }
    }
}

/// A hole in a part: where its axis crosses the part (world), its direction and diameter.
#[derive(Debug, Clone)]
struct HoleAt {
    point: V3,
    dir: V3,
    diameter: f64,
}

/// What a part is, in its own frame.
#[derive(Debug, Clone)]
enum Shape {
    Member { frame: crate::steel_detail::Frame, symmetric: bool },
    Plate { origin: V3, x: V3, y: V3, normal: V3, outline: Vec<[f64; 2]>, thickness: f64 },
}

#[derive(Debug, Clone)]
struct PartData {
    id: ElementId,
    kind: PartKind,
    shape: Shape,
    holes: Vec<HoleAt>,
    /// Cutter vertices (world), for member cuts and plate contour cuts.
    cuts: Vec<Vec<V3>>,
    material: Option<String>,
}

fn is_steel(ty: &crate::columns::ColumnType) -> bool {
    use crate::columns::{MaterialKind, Section};
    match ty.material_kind {
        Some(k) => k == MaterialKind::Steel,
        None => !matches!(ty.section, Section::Rectangular | Section::Round),
    }
}

fn polygon_frame(outline: &[V3], normal: V3) -> Option<(V3, V3, V3)> {
    let o = *outline.first()?;
    let x = unit(sub(*outline.get(1)?, o));
    let y = cross(unit(normal), x);
    Some((o, x, y))
}

fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

/// The parts of the model.
fn parts(store: &Store) -> Vec<PartData> {
    let mut out = Vec::new();
    for e in store.iter() {
        let kind = if crate::columns::column_recipe(e).is_some() {
            Some(PartKind::Column)
        } else if let Some(r) = crate::beams::beam_recipe(e) {
            Some(if r.brace { PartKind::Brace } else { PartKind::Beam })
        } else {
            None
        };
        if let Some(kind) = kind {
            let Some(frame) = crate::steel_detail::member_frame(store, e) else { continue };
            if !is_steel(&frame.ty) {
                continue;
            }
            use crate::columns::Section;
            let symmetric = matches!(
                frame.ty.section,
                Section::IShape | Section::RectangularHollow | Section::CircularHollow | Section::Rectangular | Section::Round
            );
            let material = frame.ty.material.clone();
            out.push(PartData { id: e.id(), kind, shape: Shape::Member { frame, symmetric }, holes: Vec::new(), cuts: Vec::new(), material });
        } else if let Some(r) = crate::steel_detail::plate_recipe(e) {
            let Ok(info) = crate::steel_detail::plate_info(
                store,
                crate::steel_detail::IdsParams { element_ids: vec![crate::id::ElementRef::Id(e.id())] },
            ) else {
                continue;
            };
            let i = &info[0];
            let pl = crate::work_planes::resolve(store, &r.plane)
                .or_else(|_| crate::work_planes::Plane::new(r.fallback[0], r.fallback[1], r.fallback[2]));
            let Ok(pl) = pl else { continue };
            let (lo, hi) = r.justification.span(r.thickness);
            let origin = add(pl.origin, mul(pl.normal, (lo + hi) / 2.0));
            let outline: Vec<[f64; 2]> = i.outline.iter().map(|p| pl.to_local(*p)).collect();
            out.push(PartData {
                id: e.id(),
                kind: PartKind::Plate,
                shape: Shape::Plate { origin, x: pl.x, y: pl.y, normal: pl.normal, outline, thickness: r.thickness },
                holes: Vec::new(),
                cuts: Vec::new(),
                material: r.material.clone(),
            });
        }
    }
    // Connection plates.
    for (id, part) in crate::steel_fab::all(store) {
        let Part::Plate { outline, normal, thickness } = part else { continue };
        let Some((o, x, y)) = polygon_frame(&outline, normal) else { continue };
        let local: Vec<[f64; 2]> = outline.iter().map(|p| [dot(sub(*p, o), x), dot(sub(*p, o), y)]).collect();
        out.push(PartData {
            id,
            kind: PartKind::Plate,
            shape: Shape::Plate { origin: o, x, y, normal: unit(normal), outline: local, thickness },
            holes: Vec::new(),
            cuts: Vec::new(),
            material: None,
        });
    }
    out
}

/// Finds each part's holes (fastener patterns' and connections' bolts) and cuts.
fn features(store: &Store, parts: &mut [PartData]) {
    let index: HashMap<ElementId, usize> = parts.iter().enumerate().map(|(k, p)| (p.id, k)).collect();
    // Pattern holes: the elements they pierce, from bodies.
    for e in store.iter() {
        if crate::steel_detail::pattern_recipe(e).is_none() {
            continue;
        }
        for (id, a, b, d) in crate::steel_detail::holes(store, e) {
            if let Some(&k) = index.get(&id) {
                parts[k].holes.push(HoleAt { point: mul(add(a, b), 0.5), dir: unit(sub(b, a)), diameter: d });
            }
        }
    }
    // Connection bolts through the parts they cross, in standard holes (2 mm or 1/16" over).
    let clearance = if crate::steel_fab::us_region(store) { 0.0625 * 0.0254 } else { 0.002 };
    let bolts: Vec<(V3, V3, f64)> = crate::steel_fab::all(store)
        .into_iter()
        .filter_map(|(_, p)| match p {
            Part::Bolt { start, end, diameter, anchor: false } => Some((start, end, diameter + clearance)),
            _ => None,
        })
        .collect();
    for part in parts.iter_mut() {
        for &(s, e, d) in &bolts {
            let dir = unit(sub(e, s));
            let hole = match &part.shape {
                Shape::Member { frame: f, .. } => {
                    let len = dot(sub(f.end, f.start), f.axis);
                    let local = |p: V3| [dot(sub(p, f.start), f.axis), dot(sub(p, f.start), f.across), dot(sub(p, f.start), f.deep)];
                    let (ls, le) = (local(s), local(e));
                    // Through the web (across) or a flange (deep).
                    let (k, planes): (usize, Vec<f64>) = if dot(dir, f.across).abs() > dot(dir, f.deep).abs() {
                        (1, vec![0.0])
                    } else {
                        let h = f.ty.depth / 2.0 - f.ty.flange_thickness / 2.0;
                        (2, vec![h, -h])
                    };
                    planes.into_iter().find_map(|c| {
                        let (a, b) = (ls[k] - c, le[k] - c);
                        if a * b > 0.0 || (a - b).abs() < 1e-12 {
                            return None;
                        }
                        let t = a / (a - b);
                        let p = [ls[0] + (le[0] - ls[0]) * t, ls[1] + (le[1] - ls[1]) * t, ls[2] + (le[2] - ls[2]) * t];
                        let ok = p[0] >= -1e-6
                            && p[0] <= len + 1e-6
                            && p[1].abs() <= f.ty.width / 2.0 + 1e-6
                            && p[2].abs() <= f.ty.depth / 2.0 + 1e-6;
                        ok.then(|| add(s, mul(sub(e, s), t)))
                    })
                }
                Shape::Plate { origin, x, y, normal, outline, .. } => {
                    let (a, b) = (dot(sub(s, *origin), *normal), dot(sub(e, *origin), *normal));
                    if a * b > 0.0 || (a - b).abs() < 1e-12 {
                        None
                    } else {
                        let t = a / (a - b);
                        let p = add(s, mul(sub(e, s), t));
                        let q = [dot(sub(p, *origin), *x), dot(sub(p, *origin), *y)];
                        inside(outline, q).then_some(p)
                    }
                }
            };
            if let Some(p) = hole {
                part.holes.push(HoleAt { point: p, dir, diameter: d });
            }
        }
    }
    // Cuts.
    let bodies = crate::bodies::all(store);
    let body_of = |id: ElementId| -> Option<Vec<Mesh>> {
        bodies.iter().find(|b| b.element == id).map(|b| b.parts.iter().map(|p| p.mesh.clone()).collect())
    };
    for (_, target, r) in crate::steel_cuts::all(store) {
        let Some(&k) = index.get(&target) else { continue };
        if let Some(tools) = crate::steel_cuts::cutter(store, &r, &body_of) {
            for (pts, _) in tools {
                parts[k].cuts.push(pts);
            }
        }
    }
}

/// A member's signature in one of its frames (`flip_end` reverses it, `flip_side` mirrors it).
fn member_signature(p: &PartData, f: &crate::steel_detail::Frame, flip_end: bool, flip_side: bool) -> String {
    let len = dot(sub(f.end, f.start), f.axis);
    let (o, ax) = if flip_end { (f.end, mul(f.axis, -1.0)) } else { (f.start, f.axis) };
    let across = if flip_end != flip_side { mul(f.across, -1.0) } else { f.across };
    let local = |q: V3| [dot(sub(q, o), ax), dot(sub(q, o), across), dot(sub(q, o), f.deep)];
    let mut holes: Vec<String> = p
        .holes
        .iter()
        .map(|h| {
            let l = local(h.point);
            let web = dot(h.dir, f.across).abs() > dot(h.dir, f.deep).abs();
            format!("{}:{}:{}:{}:{}", if web { "w" } else { "f" }, mm(l[0]), mm(l[1]), mm(l[2]), mm(h.diameter))
        })
        .collect();
    holes.sort();
    let mut cuts: Vec<String> = p
        .cuts
        .iter()
        .map(|c| {
            let mut v: Vec<String> = c.iter().map(|q| {
                let l = local(*q);
                format!("{},{},{}", mm(l[0]), mm(l[1]), mm(l[2]))
            }).collect();
            v.sort();
            v.dedup();
            v.join(";")
        })
        .collect();
    cuts.sort();
    let name = f
        .ty
        .designation
        .clone()
        .unwrap_or_else(|| format!("{:?} {}x{}", f.ty.section, mm(f.ty.depth), mm(f.ty.width)));
    format!("M|{name}|{}|H[{}]|C[{}]", mm(len), holes.join(" "), cuts.join(" "))
}

/// A plate's outline, holes and cuts in a frame starting at corner `k`, along or against its
/// order, possibly mirrored: the canonical form is the smallest of these.
/// A plate form: its signature, outline and holes (x, y, diameter).
type PlateForm = (String, Vec<[f64; 2]>, Vec<(f64, f64, f64)>);

fn plate_forms(p: &PartData) -> Vec<PlateForm> {
    let Shape::Plate { origin, x, y, outline, thickness, .. } = &p.shape else { return Vec::new() };
    let n = outline.len();
    let to2 = |q: V3| [dot(sub(q, *origin), *x), dot(sub(q, *origin), *y)];
    let holes2: Vec<([f64; 2], f64)> = p.holes.iter().map(|h| (to2(h.point), h.diameter)).collect();
    let cuts2: Vec<Vec<[f64; 2]>> = p.cuts.iter().map(|c| c.iter().map(|q| to2(*q)).collect()).collect();
    let mut out = Vec::new();
    for mirror in [false, true] {
        for rev in [false, true] {
            for k in 0..n {
                let idx = |i: usize| if rev { (k + n - i) % n } else { (k + i) % n };
                let m = |q: [f64; 2]| if mirror { [q[0], -q[1]] } else { q };
                let a = m(outline[idx(0)]);
                let b = m(outline[idx(1)]);
                let ex = { let d = [b[0] - a[0], b[1] - a[1]]; let l = d[0].hypot(d[1]).max(1e-12); [d[0] / l, d[1] / l] };
                let ey = [-ex[1], ex[0]];
                let f = |q: [f64; 2]| { let q = m(q); let d = [q[0] - a[0], q[1] - a[1]]; [d[0] * ex[0] + d[1] * ex[1], d[0] * ey[0] + d[1] * ey[1]] };
                let pts: Vec<[f64; 2]> = (0..n).map(|i| f(outline[idx(i)])).collect();
                // Into the first quadrant (the smallest x and y 0).
                let (mx, my) = pts.iter().fold((f64::INFINITY, f64::INFINITY), |(a, b), q| (a.min(q[0]), b.min(q[1])));
                let sh = |q: [f64; 2]| [q[0] - mx, q[1] - my];
                let pts: Vec<[f64; 2]> = pts.into_iter().map(sh).collect();
                let mut hs: Vec<(f64, f64, f64)> = holes2.iter().map(|(q, d)| { let q = sh(f(*q)); (q[0], q[1], *d) }).collect();
                hs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let mut cs: Vec<String> = cuts2
                    .iter()
                    .map(|c| { let mut v: Vec<String> = c.iter().map(|q| { let q = sh(f(*q)); format!("{},{}", mm(q[0]), mm(q[1])) }).collect(); v.sort(); v.dedup(); v.join(";") })
                    .collect();
                cs.sort();
                let sig = format!(
                    "P|{}|{}|H[{}]|C[{}]",
                    mm(*thickness),
                    pts.iter().map(|q| format!("{},{}", mm(q[0]), mm(q[1]))).collect::<Vec<_>>().join(" "),
                    hs.iter().map(|h| format!("{},{},{}", mm(h.0), mm(h.1), mm(h.2))).collect::<Vec<_>>().join(" "),
                    cs.join(" ")
                );
                out.push((sig, pts, hs));
            }
        }
    }
    out
}

fn signature(p: &PartData) -> String {
    match &p.shape {
        Shape::Member { frame, symmetric } => {
            let mut v = vec![member_signature(p, frame, false, false), member_signature(p, frame, true, false)];
            if *symmetric {
                v.push(member_signature(p, frame, false, true));
                v.push(member_signature(p, frame, true, true));
            }
            v.into_iter().min().unwrap_or_default()
        }
        Shape::Plate { .. } => plate_forms(p).into_iter().map(|f| f.0).min().unwrap_or_default(),
    }
}

fn centre(p: &PartData) -> V3 {
    match &p.shape {
        Shape::Member { frame, .. } => mul(add(frame.start, frame.end), 0.5),
        Shape::Plate { origin, x, y, outline, .. } => {
            let n = outline.len().max(1) as f64;
            let (cx, cy) = outline.iter().fold((0.0, 0.0), |(a, b), q| (a + q[0], b + q[1]));
            add(*origin, add(mul(*x, cx / n), mul(*y, cy / n)))
        }
    }
}

// ---- Assemblies ----

struct Dsu(Vec<usize>);

impl Dsu {
    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }
    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[b] = a;
        }
    }
}

fn by_guid(store: &Store, g: &str) -> Option<ElementId> {
    GlobalId::parse(g).and_then(|g| store.find_by_guid(&g))
}

/// The shop-welded groups of parts (indices into `parts`).
fn assemblies(store: &Store, parts: &[PartData]) -> Vec<Vec<usize>> {
    let index: HashMap<ElementId, usize> = parts.iter().enumerate().map(|(k, p)| (p.id, k)).collect();
    let mut dsu = Dsu((0..parts.len()).collect());
    for e in store.iter() {
        if let Some(w) = crate::steel_detail::weld_recipe(e) {
            if w.location != crate::steel_detail::Location::Shop {
                continue;
            }
            let ids: Vec<usize> = w.elements.iter().filter_map(|g| by_guid(store, g)).filter_map(|i| index.get(&i).copied()).collect();
            for pair in ids.windows(2) {
                dsu.join(pair[0], pair[1]);
            }
        }
        // A connection's plates are welded to their member.
        if let Some(c) = crate::connections::recipe(e) {
            let welded_to = match c.connection_type.as_str() {
                "end_plate" => c.members.get(1),
                "shear_plate" | "gusset" | "base_plate" => c.members.first(),
                _ => None,
            };
            let Some(m) = welded_to.and_then(|g| by_guid(store, g)).and_then(|i| index.get(&i).copied()) else { continue };
            let guid = e.guid().to_string();
            for f in store.iter() {
                if crate::steel_fab::fab_recipe(f).is_some_and(|r| r.connection == guid)
                    && let Some(&k) = index.get(&f.id())
                {
                    dsu.join(m, k);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for k in 0..parts.len() {
        let r = dsu.find(k);
        groups.entry(r).or_default().push(k);
    }
    groups.into_values().collect()
}

/// The main part of an assembly: the longest member, else the biggest plate.
fn main_of(parts: &[PartData], group: &[usize]) -> usize {
    let size = |k: usize| match &parts[k].shape {
        Shape::Member { frame, .. } => (1, dot(sub(frame.end, frame.start), frame.axis) * frame.ty.depth.max(frame.ty.width)),
        Shape::Plate { outline, .. } => {
            let n = outline.len();
            (0, (0..n).map(|i| outline[i][0] * outline[(i + 1) % n][1] - outline[(i + 1) % n][0] * outline[i][1]).sum::<f64>().abs())
        }
    };
    *group
        .iter()
        .max_by(|a, b| {
            let (sa, sb) = (size(**a), size(**b));
            sa.0.cmp(&sb.0).then(sa.1.total_cmp(&sb.1)).then(b.cmp(a))
        })
        .expect("a group has a part")
}

// ---- Numbering ----

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PartMark {
    pub mark: String,
    pub kind: PartKind,
    pub count: usize,
    pub elements: Vec<ElementId>,
    /// The section, or the plate's thickness.
    pub description: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AssemblyMark {
    pub mark: String,
    pub count: usize,
    pub main_mark: String,
    /// The part marks in each assembly of the mark.
    pub parts: Vec<String>,
    /// The assemblies' elements.
    pub assemblies: Vec<Vec<ElementId>>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Report {
    pub parts: Vec<PartMark>,
    pub assemblies: Vec<AssemblyMark>,
}

struct Numbered {
    parts: Vec<PartData>,
    part_marks: Vec<String>,
    part_report: Vec<PartMark>,
    /// For each assembly: its mark and members.
    assemblies: Vec<(String, usize, Vec<usize>)>,
    assembly_report: Vec<AssemblyMark>,
}

fn number(store: &Store, o: &Options) -> Numbered {
    let mut parts = parts(store);
    features(store, &mut parts);
    // Part marks: by kind, in order of each signature's first element.
    let sigs: Vec<String> = parts.iter().map(signature).collect();
    let mut order: Vec<usize> = (0..parts.len()).collect();
    order.sort_by_key(|k| (parts[*k].kind, parts[*k].id));
    let mut by_sig: HashMap<(PartKind, String), String> = HashMap::new();
    let mut next: HashMap<PartKind, usize> = HashMap::new();
    let mut marks = vec![String::new(); parts.len()];
    let mut report: BTreeMap<String, PartMark> = BTreeMap::new();
    for k in order {
        let kind = parts[k].kind;
        let key = (kind, sigs[k].clone());
        let mark = by_sig
            .entry(key)
            .or_insert_with(|| {
                let n = next.entry(kind).or_insert(o.start);
                let m = format!("{}{n}", o.prefix(kind));
                *n += 1;
                m
            })
            .clone();
        marks[k] = mark.clone();
        let description = match &parts[k].shape {
            Shape::Member { frame, .. } => frame.ty.designation.clone().unwrap_or_else(|| format!("{:?}", frame.ty.section)),
            Shape::Plate { thickness, .. } => format!("PL {} mm", mm(*thickness)),
        };
        let r = report.entry(mark.clone()).or_insert(PartMark { mark, kind, count: 0, elements: Vec::new(), description });
        r.count += 1;
        r.elements.push(parts[k].id);
    }
    // Assemblies.
    let groups = assemblies(store, &parts);
    let mut asm_sig: HashMap<String, String> = HashMap::new();
    let mut asm_next: HashMap<PartKind, usize> = HashMap::new();
    let mut asm: Vec<(String, usize, Vec<usize>)> = Vec::new();
    let mut asm_report: BTreeMap<String, AssemblyMark> = BTreeMap::new();
    let mut groups: Vec<Vec<usize>> = groups;
    groups.sort_by_key(|g| g.iter().map(|k| parts[*k].id).min());
    for g in groups {
        let main = main_of(&parts, &g);
        // The other parts' marks and places in the main part's frame (each way round).
        let places = |flip: bool| -> String {
            let mut v: Vec<String> = g
                .iter()
                .filter(|k| **k != main)
                .map(|k| {
                    let c = centre(&parts[*k]);
                    let l = match &parts[main].shape {
                        Shape::Member { frame: f, .. } => {
                            let (o, ax) = if flip { (f.end, mul(f.axis, -1.0)) } else { (f.start, f.axis) };
                            let ac = if flip { mul(f.across, -1.0) } else { f.across };
                            [dot(sub(c, o), ax), dot(sub(c, o), ac), dot(sub(c, o), f.deep)]
                        }
                        Shape::Plate { .. } => {
                            let m = centre(&parts[main]);
                            [dot(sub(c, m), sub(c, m)).sqrt(), 0.0, 0.0]
                        }
                    };
                    format!("{}@{},{},{}", marks[*k], mm(l[0]), mm(l[1]), mm(l[2]))
                })
                .collect();
            v.sort();
            v.join(" ")
        };
        let sig = format!("{}|{}", marks[main], places(false).min(places(true)));
        let kind = parts[main].kind;
        let mark = asm_sig
            .entry(sig)
            .or_insert_with(|| {
                let n = asm_next.entry(kind).or_insert(o.start);
                let m = format!("{}{n}", o.prefix(kind).to_uppercase());
                *n += 1;
                m
            })
            .clone();
        let mut part_marks: Vec<String> = g.iter().map(|k| marks[*k].clone()).collect();
        part_marks.sort();
        let r = asm_report.entry(mark.clone()).or_insert(AssemblyMark {
            mark: mark.clone(),
            count: 0,
            main_mark: marks[main].clone(),
            parts: part_marks,
            assemblies: Vec::new(),
        });
        r.count += 1;
        r.assemblies.push(g.iter().map(|k| parts[*k].id).collect());
        asm.push((mark, main, g));
    }
    let natural = |m: &str| {
        let (p, n): (String, String) = m.chars().partition(|c| !c.is_ascii_digit());
        (p, n.parse::<usize>().unwrap_or(0))
    };
    let mut part_report: Vec<PartMark> = report.into_values().collect();
    part_report.sort_by_key(|p| (p.kind, natural(&p.mark)));
    let mut assembly_report: Vec<AssemblyMark> = asm_report.into_values().collect();
    assembly_report.sort_by_key(|a| natural(&a.mark));
    Numbered { parts, part_marks: marks, part_report, assemblies: asm, assembly_report }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RunParams {
    #[serde(default)]
    pub options: Option<Options>,
}

/// Numbers the model's steel (`steel_numbering.run`).
pub(crate) fn run(txn: &mut crate::txn::Transaction<'_>, p: RunParams) -> Result<Report> {
    let store = txn.store().clone();
    let o = p.options.unwrap_or_default();
    let n = number(&store, &o);
    // The assemblies made before go.
    for e in store.iter() {
        if e.param(RECIPE_KEY).and_then(|v| v.get("kind")).and_then(|k| k.as_str()) == Some(ASSEMBLY_RECIPE) {
            txn.delete(e.id())?;
        }
    }
    let mut assembly_of: HashMap<usize, String> = HashMap::new();
    for (mark, main, group) in &n.assemblies {
        let guids: Vec<String> = group.iter().filter_map(|k| store.get(n.parts[*k].id).map(|e| e.guid().to_string())).collect();
        let main_guid = store.get(n.parts[*main].id).map(|e| e.guid().to_string());
        txn.create(
            NewElement::new("IfcElementAssembly")
                .param("@PredefinedType", "USERDEFINED")
                .param("@ObjectType", "STEEL_ASSEMBLY")
                .param("@Tag", mark.clone())
                .param(RECIPE_KEY, json!({ "kind": ASSEMBLY_RECIPE, "mark": mark, "main": main_guid, "parts": guids })),
        )?;
        for k in group {
            assembly_of.insert(*k, mark.clone());
        }
    }
    let options = serde_json::to_value(&o).expect("serialises");
    for (k, part) in n.parts.iter().enumerate() {
        let mark = &n.part_marks[k];
        txn.set_param(part.id, "@Tag", mark.clone())?;
        txn.set_param(
            part.id,
            MARKS_KEY,
            json!({ "part": mark, "assembly": assembly_of.get(&k), "options": options }),
        )?;
    }
    Ok(Report { parts: n.part_report, assemblies: n.assembly_report })
}

/// The marks as they would be, without changing the model (`steel_numbering.preview`).
pub fn preview(store: &Store, p: RunParams) -> Result<Report> {
    let n = number(store, &p.options.unwrap_or_default());
    Ok(Report { parts: n.part_report, assemblies: n.assembly_report })
}

/// An element's marks (part, assembly), if numbered.
pub fn marks_of(e: &Element) -> Option<(String, Option<String>)> {
    let v = e.param(MARKS_KEY)?;
    Some((v.get("part")?.as_str()?.to_owned(), v.get("assembly").and_then(|a| a.as_str()).map(str::to_owned)))
}

/// The assembly elements' parts, for IFC aggregation.
pub fn assembly_parts(store: &Store, e: &Element) -> Option<Vec<ElementId>> {
    let v = e.param(RECIPE_KEY)?;
    if v.get("kind")?.as_str()? != ASSEMBLY_RECIPE {
        return None;
    }
    Some(v.get("parts")?.as_array()?.iter().filter_map(|g| g.as_str().and_then(|g| by_guid(store, g))).collect())
}

// ---- NC ----

fn profile_code(s: crate::columns::Section) -> reviera_dstv::ProfileCode {
    use crate::columns::Section;
    use reviera_dstv::ProfileCode;
    match s {
        Section::IShape => ProfileCode::I,
        Section::Channel => ProfileCode::U,
        Section::Angle => ProfileCode::L,
        Section::Tee => ProfileCode::T,
        Section::RectangularHollow => ProfileCode::M,
        Section::CircularHollow => ProfileCode::RO,
        Section::Round => ProfileCode::RU,
        Section::Rectangular => ProfileCode::B,
    }
}

const STEEL_DENSITY: f64 = 7850.0;

fn perimeter(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    (0..n).map(|i| {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        (b[0] - a[0]).hypot(b[1] - a[1])
    })
    .sum()
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct NcParams {
    /// The order and drawing written in each file's header.
    #[serde(default)]
    pub order: Option<String>,
    #[serde(default)]
    pub drawing: Option<String>,
    #[serde(default)]
    pub options: Option<Options>,
}

/// One DSTV NC1 piece per part mark, with its quantity.
pub fn nc_pieces(store: &Store, p: &NcParams) -> Vec<reviera_dstv::Piece> {
    use reviera_dstv::{Contour, Face, Hole, Piece};
    let n = number(store, &p.options.clone().unwrap_or_default());
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (k, m) in n.part_marks.iter().enumerate() {
        seen.entry(m.clone()).or_insert(k);
    }
    let order = p.order.clone().unwrap_or_else(|| "BIM Reviera".into());
    let drawing = p.drawing.clone().unwrap_or_else(|| "-".into());
    let count = |m: &str| n.part_report.iter().find(|r| r.mark == m).map_or(1, |r| r.count) as u32;
    let mut out = Vec::new();
    for (mark, k) in seen {
        let part = &n.parts[k];
        let steel = part.material.clone().unwrap_or_else(|| "S355".into());
        let piece = match &part.shape {
            Shape::Member { frame: f, .. } => {
                let len = dot(sub(f.end, f.start), f.axis);
                let (d, b) = (f.ty.depth, f.ty.width);
                let local = |q: V3| [dot(sub(q, f.start), f.axis), dot(sub(q, f.start), f.across), dot(sub(q, f.start), f.deep)];
                let holes = part
                    .holes
                    .iter()
                    .map(|h| {
                        let l = local(h.point);
                        let web = dot(h.dir, f.across).abs() > dot(h.dir, f.deep).abs();
                        let (face, y) = if web {
                            (if dot(h.dir, f.across) >= 0.0 { Face::V } else { Face::H }, l[2] + d / 2.0)
                        } else {
                            (if l[2] >= 0.0 { Face::O } else { Face::U }, l[1] + b / 2.0)
                        };
                        Hole { face, x: l[0] * 1000.0, y: y * 1000.0, diameter: h.diameter * 1000.0, depth: 0.0 }
                    })
                    .collect();
                Piece {
                    order: order.clone(),
                    drawing: drawing.clone(),
                    phase: "1".into(),
                    piece: mark.clone(),
                    steel,
                    quantity: count(&mark),
                    profile: f.ty.designation.clone().unwrap_or_else(|| format!("{}x{}", mm(d), mm(b))),
                    code: profile_code(f.ty.section),
                    length: len * 1000.0,
                    height: d * 1000.0,
                    flange_width: b * 1000.0,
                    flange_thickness: f.ty.flange_thickness * 1000.0,
                    web_thickness: f.ty.web_thickness * 1000.0,
                    radius: 0.0,
                    weight: f.ty.area() * STEEL_DENSITY,
                    paint: perimeter(&f.ty.outline()),
                    skew: [0.0; 4],
                    text: Vec::new(),
                    holes,
                    contours: Vec::new(),
                }
            }
            Shape::Plate { thickness, .. } => {
                // The plate in its canonical frame (the smallest x and y 0).
                let forms = plate_forms(part);
                let Some((_, pts, hs)) = forms.into_iter().min_by(|a, b| a.0.cmp(&b.0)) else { continue };
                let (w, h) = pts.iter().fold((0.0f64, 0.0f64), |(a, b), q| (a.max(q[0]), b.max(q[1])));
                // Lengthwise along x: turn it if it is taller than long.
                let turn = h > w;
                let t = |q: [f64; 2]| if turn { [q[1], w - q[0]] } else { q };
                let (len, wid) = if turn { (h, w) } else { (w, h) };
                let mut contour: Vec<(f64, f64, f64)> = pts.iter().map(|q| { let q = t(*q); (q[0] * 1000.0, q[1] * 1000.0, 0.0) }).collect();
                // External contours run anticlockwise.
                let area: f64 = (0..contour.len()).map(|i| {
                    let (a, b) = (contour[i], contour[(i + 1) % contour.len()]);
                    a.0 * b.1 - b.0 * a.1
                }).sum();
                if area < 0.0 {
                    contour.reverse();
                }
                Piece {
                    order: order.clone(),
                    drawing: drawing.clone(),
                    phase: "1".into(),
                    piece: mark.clone(),
                    steel,
                    quantity: count(&mark),
                    profile: format!("PL{}", mm(*thickness)),
                    code: reviera_dstv::ProfileCode::B,
                    length: len * 1000.0,
                    height: wid * 1000.0,
                    flange_width: 0.0,
                    flange_thickness: 0.0,
                    web_thickness: thickness * 1000.0,
                    radius: 0.0,
                    weight: thickness * STEEL_DENSITY,
                    paint: 2.0,
                    skew: [0.0; 4],
                    text: Vec::new(),
                    holes: hs
                        .iter()
                        .map(|(x, y, d)| { let q = t([*x, *y]); Hole { face: Face::V, x: q[0] * 1000.0, y: q[1] * 1000.0, diameter: d * 1000.0, depth: 0.0 } })
                        .collect(),
                    contours: vec![Contour { face: Face::V, external: true, points: contour }],
                }
            }
        };
        out.push(piece);
    }
    out
}

/// Whether an element is a steel assembly.
pub fn assembly_parts_kind(e: &Element) -> bool {
    e.param(RECIPE_KEY).and_then(|v| v.get("kind")).and_then(|k| k.as_str()) == Some(ASSEMBLY_RECIPE)
}
