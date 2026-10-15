// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Parts (spec Ch 3 §3.6.11 … 3.6.14, ARC-PART-001 … 004): the pieces an element is built
//! from, for scheduling and sequencing, while the element stays the source of their geometry.
//!
//! A part is an `IfcBuildingElementPart` with `recipe.part`:
//!
//! - **First-level parts** come from Create Parts: one per layer of a straight layered wall
//!   (the wall's finished body, joins and openings included, sliced at the layer boundaries, so
//!   the parts tile it exactly), or one for the whole body of a single-layer wall, a floor,
//!   ceiling, roof, column or beam. They follow the source as it changes.
//! - **Divided pieces** keep their parent part, the division's cutting planes (sketched lines
//!   swept along a work-plane normal, or levels, grids and reference planes with an offset, which
//!   they follow), its gap and which side of each plane they are on.
//! - **Merged parts** list the parts they are made of.
//!
//! A part that is divided or merged is an intermediate: it is kept but not drawn. Excluded parts
//! are kept but not drawn, scheduled or exported. Each view shows parts, the original elements
//! or both (`rxd:parts_visibility` on the view, Show Parts by default).
//!
//! Not built yet: Edit Division and Edit Merged sessions, arcs and splines as division curves,
//! division profiles, fused merged geometry (a merge keeps its pieces' faces), shape handles,
//! phase overrides, parts of stacked, curved and curtain walls and of layered floors and roofs
//! (they have no layers yet), parts that appear when a moved reference makes new pieces,
//! writing the source without its body in IFC, linked sources.

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::{Body, Mesh};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const RECIPE: &str = "recipe.part";
pub const CLASS: &str = "IfcBuildingElementPart";
/// A view's Parts Visibility.
pub const VIEW_KEY: &str = "rxd:parts_visibility";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    #[default]
    ShowParts,
    ShowOriginal,
    ShowBoth,
}

/// A division's cutting plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cutter {
    /// A plane through `point` square to `normal`.
    Plane { point: [f64; 3], normal: [f64; 3] },
    /// A level, grid or reference plane (GlobalId), moved by `offset` along its normal.
    Ref { guid: String, offset: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Division {
    /// Shared by the pieces of one division.
    pub id: String,
    /// The divided part (GlobalId).
    pub parent: String,
    pub cutters: Vec<Cutter>,
    #[serde(default)]
    pub gap: f64,
    /// The piece's side of each cutter: `+` along its normal, `-` against it.
    pub cell: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PartRecipe {
    pub kind: String,
    /// The original element (GlobalId).
    pub source: String,
    /// Its layer (a layered wall's), or the whole body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub division: Option<Division>,
    /// A merged part's parts (GlobalIds).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merged: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub excluded: bool,
    /// The material instead of the original's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// The level instead of the original's: a level's GlobalId, or empty for Non Associated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_level: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !b
}

pub fn recipe(e: &Element) -> Option<PartRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn find<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
}

fn kind_of(e: &Element) -> Option<&str> {
    e.param(RECIPE_KEY)?.get("kind")?.as_str()
}

/// A source's layers in plan: the wall's centreline start, its normal, and each layer's band
/// across it (from the centreline along the normal) with its layer index.
struct Bands {
    start: [f64; 2],
    normal: [f64; 2],
    bands: Vec<(usize, f64, f64)>,
}

/// What a source divides into: its wall layers, or `None` for one part of its whole body.
/// `Err` when the element cannot have parts.
fn layers_of(store: &Store, e: &Element) -> std::result::Result<Option<Bands>, &'static str> {
    if recipe(e).is_some() {
        return Err("a part");
    }
    if let Some(r) = crate::walls::wall_recipe(e) {
        if !e.class.starts_with("IfcWall") {
            return Err("not a basic wall");
        }
        if crate::walls::stacked::of_wall(store, e).is_some() {
            return Err("a stacked wall");
        }
        if crate::walls::curved::path_of(&r).is_some() {
            return Err("a curved wall");
        }
        let Some(c) = crate::walls::layered::compound_of(store, e) else {
            return Ok(None);
        };
        if c.layers.iter().filter(|l| l.thickness > 0.0).count() < 2 {
            return Ok(None);
        }
        let g =
            crate::walls::regenerate(store, e).map_err(|_| "a wall that does not regenerate")?;
        let half = g.width / 2.0;
        let ext = if r.flip { -1.0 } else { 1.0 };
        let b = c.boundaries();
        let bands = (0..c.layers.len())
            .filter(|i| c.layers[*i].thickness > 0.0)
            .map(|i| {
                let (ya, yb) = (ext * (half - b[i]), ext * (half - b[i + 1]));
                (i, ya.min(yb), ya.max(yb))
            })
            .collect();
        return Ok(Some(Bands {
            start: g.start,
            normal: g.normal(),
            bands,
        }));
    }
    let ok = match e.class.as_str() {
        "IfcSlab" | "IfcRoof" | "IfcColumn" | "IfcBeam" => true,
        "IfcCovering" => kind_of(e) == Some(crate::floors::CEILING_RECIPE),
        _ => false,
    };
    if ok && store.get(e.id()).is_some() {
        Ok(None)
    } else {
        Err("not a layered element")
    }
}

/// Whether an element can be divided into parts.
pub fn eligible(store: &Store, e: &Element) -> bool {
    layers_of(store, e).is_ok() && !has_parts(store, e)
}

/// Whether an element has parts.
pub fn has_parts(store: &Store, e: &Element) -> bool {
    let g = e.guid().to_string();
    store
        .iter()
        .any(|p| recipe(p).is_some_and(|r| r.source == g))
}

/// The parts that are divided or merged (kept, not drawn).
pub fn intermediates(store: &Store) -> HashSet<String> {
    let mut out = HashSet::new();
    for e in store.iter() {
        if let Some(r) = recipe(e) {
            if let Some(d) = &r.division {
                out.insert(d.parent.clone());
            }
            out.extend(r.merged.iter().cloned());
        }
    }
    out
}

/// The leaf parts: drawn, scheduled and exported unless excluded.
pub fn leaves(store: &Store) -> Vec<(ElementId, PartRecipe)> {
    let mid = intermediates(store);
    store
        .iter()
        .filter(|e| !mid.contains(e.guid().as_str()))
        .filter_map(|e| Some((e.id(), recipe(e)?)))
        .collect()
}

// ---- Geometry ----

/// A cutter as `(unit normal, distance)`: the plane `n · p = d`.
fn resolve(store: &Store, c: &Cutter) -> Option<([f64; 3], f64)> {
    use crate::work_planes::{dot, unit};
    match c {
        Cutter::Plane { point, normal } => {
            let n = unit(*normal)?;
            Some((n, dot(n, *point)))
        }
        Cutter::Ref { guid, offset } => {
            let e = find(store, guid)?;
            if e.class == crate::levels::CLASS {
                return Some(([0.0, 0.0, 1.0], crate::levels::elevation(e) + offset));
            }
            if let Some(g) = crate::grids::grid_data(e) {
                let (a, b) = (g.curve.start, g.curve.end);
                let n = unit([-(b[1] - a[1]), b[0] - a[0], 0.0])?;
                return Some((n, n[0] * a[0] + n[1] * a[1] + offset));
            }
            let r = crate::ref_planes::recipe(e)?;
            let p = crate::ref_planes::plane_of(&r).ok()?;
            Some((p.normal, dot(p.normal, p.origin) + offset))
        }
    }
}

fn clip(meshes: &[Mesh], n: [f64; 3], d: f64) -> Vec<Mesh> {
    meshes
        .iter()
        .map(|m| crate::roofs::eaves::clip_plane(m, n, d))
        .filter(|m| volume(std::slice::from_ref(m)) > 1e-9)
        .collect()
}

/// One side of a plane: `+` keeps `n · p ≥ d + gap/2`, `-` keeps `n · p ≤ d − gap/2`.
fn side(meshes: &[Mesh], (n, d): ([f64; 3], f64), sign: char, gap: f64) -> Vec<Mesh> {
    if sign == '+' {
        clip(meshes, n, d + gap / 2.0)
    } else {
        clip(meshes, n.map(|v| -v), -(d - gap / 2.0))
    }
}

/// The volume of closed meshes (m³).
pub fn volume(meshes: &[Mesh]) -> f64 {
    meshes
        .iter()
        .map(|(pts, idx)| {
            idx.chunks(3)
                .map(|c| {
                    let [a, b, c] = [pts[c[0] as usize], pts[c[1] as usize], pts[c[2] as usize]];
                    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                        + a[2] * (b[0] * c[1] - b[1] * c[0]))
                        / 6.0
                })
                .sum::<f64>()
        })
        .sum()
}

/// The area of the largest planar face of meshes, and its normal.
pub fn largest_face(meshes: &[Mesh]) -> (f64, [f64; 3]) {
    let mut faces: Vec<([i64; 4], f64, [f64; 3])> = Vec::new();
    for (pts, idx) in meshes {
        for c in idx.chunks(3) {
            let [a, b, c] = [pts[c[0] as usize], pts[c[1] as usize], pts[c[2] as usize]];
            let u = crate::work_planes::sub(b, a);
            let v = crate::work_planes::sub(c, a);
            let x = crate::work_planes::cross(u, v);
            let l = crate::work_planes::dot(x, x).sqrt();
            if l < 1e-12 {
                continue;
            }
            let n = x.map(|k| k / l);
            let d = crate::work_planes::dot(n, a);
            let key = [
                (n[0] * 1e4).round() as i64,
                (n[1] * 1e4).round() as i64,
                (n[2] * 1e4).round() as i64,
                (d * 1e4).round() as i64,
            ];
            match faces.iter_mut().find(|f| f.0 == key) {
                Some(f) => f.1 += l / 2.0,
                None => faces.push((key, l / 2.0, n)),
            }
        }
    }
    faces
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or((0.0, [0.0, 0.0, 1.0]), |f| (f.1, f.2))
}

/// A part's solids, from its source's body in `bodies` (empty when it no longer exists).
pub fn geometry(store: &Store, e: &Element, bodies: &[Body]) -> Vec<Mesh> {
    geometry_at(store, e, bodies, 0)
}

fn geometry_at(store: &Store, e: &Element, bodies: &[Body], depth: usize) -> Vec<Mesh> {
    let Some(r) = recipe(e) else {
        return Vec::new();
    };
    if depth > 64 {
        return Vec::new();
    }
    if !r.merged.is_empty() {
        return r
            .merged
            .iter()
            .filter_map(|g| find(store, g))
            .flat_map(|p| geometry_at(store, p, bodies, depth + 1))
            .collect();
    }
    if let Some(d) = &r.division {
        let Some(parent) = find(store, &d.parent) else {
            return Vec::new();
        };
        let mut m = geometry_at(store, parent, bodies, depth + 1);
        for (c, sign) in d.cutters.iter().zip(d.cell.chars()) {
            if let Some(plane) = resolve(store, c) {
                m = side(&m, plane, sign, d.gap);
            }
        }
        return m;
    }
    let Some(source) = find(store, &r.source) else {
        return Vec::new();
    };
    let whole: Vec<Mesh> = bodies
        .iter()
        .filter(|b| b.element == source.id())
        .flat_map(|b| b.parts.iter().filter(|p| !p.plan_hidden))
        .map(|p| p.mesh.clone())
        .collect();
    let Some(i) = r.layer else {
        return whole;
    };
    let Ok(Some(b)) = layers_of(store, source) else {
        return Vec::new();
    };
    let Some(&(_, lo, hi)) = b.bands.iter().find(|x| x.0 == i) else {
        return Vec::new();
    };
    let n = [b.normal[0], b.normal[1], 0.0];
    let at = b.normal[0] * b.start[0] + b.normal[1] * b.start[1];
    // The outermost layers keep everything beyond the wall's faces (wrapped ends, sweeps).
    let first = b.bands.iter().map(|x| x.1).fold(f64::MAX, f64::min);
    let last = b.bands.iter().map(|x| x.2).fold(f64::MIN, f64::max);
    let mut m = whole;
    if lo > first + 1e-9 {
        m = clip(&m, n, at + lo);
    }
    if hi < last - 1e-9 {
        m = clip(&m, n.map(|v| -v), -(at + hi));
    }
    m
}

/// A part's material: its own, its layer's, or its source's class.
pub fn material(store: &Store, r: &PartRecipe) -> String {
    if let Some(m) = &r.material {
        return m.clone();
    }
    if let Some(first) = r
        .merged
        .first()
        .and_then(|g| find(store, g))
        .and_then(recipe)
    {
        return material(store, &first);
    }
    if let Some(p) = r
        .division
        .as_ref()
        .and_then(|d| find(store, &d.parent))
        .and_then(recipe)
    {
        return material(store, &p);
    }
    let Some(source) = find(store, &r.source) else {
        return String::new();
    };
    match (r.layer, crate::walls::layered::compound_of(store, source)) {
        (Some(i), Some(c)) if i < c.layers.len() => {
            let l = &c.layers[i];
            if l.material.trim().is_empty() {
                l.function.name().to_owned()
            } else {
                l.material.clone()
            }
        }
        _ => String::new(),
    }
}

/// Whether a part is in its source's core (`Core`) or not (`Finish`).
pub fn construction(store: &Store, r: &PartRecipe) -> &'static str {
    let first = r
        .merged
        .first()
        .and_then(|g| find(store, g))
        .and_then(recipe);
    let parent = r
        .division
        .as_ref()
        .and_then(|d| find(store, &d.parent))
        .and_then(recipe);
    if let Some(p) = first.or(parent) {
        return construction(store, &p);
    }
    let core = find(store, &r.source)
        .and_then(|s| crate::walls::layered::compound_of(store, s))
        .map(|c| c.core);
    match (r.layer, core) {
        (Some(i), Some([a, b])) if !(a..b).contains(&i) => "Finish",
        _ => "Core",
    }
}

/// Every drawn part's solids and the class it is coloured by.
pub fn all(store: &Store, bodies: &[Body]) -> Vec<(ElementId, String, Vec<Mesh>)> {
    leaves(store)
        .into_iter()
        .filter(|(_, r)| !r.excluded)
        .map(|(id, r)| {
            let e = store.get(id).expect("listed");
            let m = material(store, &r);
            let class = if m.is_empty() {
                // A whole-body part looks like its source.
                find(store, &r.source)
                    .and_then(|s| bodies.iter().find(|b| b.element == s.id()))
                    .and_then(|b| b.parts.first())
                    .map_or_else(|| CLASS.to_owned(), |p| p.class.clone())
            } else {
                format!("{CLASS}:layer/{m}")
            };
            (id, class, geometry(store, e, bodies))
        })
        .collect()
}

/// The elements a view does not draw because of its Parts Visibility: the sources with parts
/// (Show Parts), or the parts (Show Original).
pub fn hidden_in(store: &Store, view: Option<ElementId>) -> HashSet<ElementId> {
    match view_mode(store, view) {
        Visibility::ShowBoth => HashSet::new(),
        Visibility::ShowOriginal => store
            .iter()
            .filter(|e| recipe(e).is_some())
            .map(Element::id)
            .collect(),
        Visibility::ShowParts => {
            let sources: HashSet<String> =
                store.iter().filter_map(recipe).map(|r| r.source).collect();
            store
                .iter()
                .filter(|e| sources.contains(e.guid().as_str()))
                .map(Element::id)
                .collect()
        }
    }
}

/// A view's Parts Visibility.
pub fn view_mode(store: &Store, view: Option<ElementId>) -> Visibility {
    view.and_then(|v| store.get(v))
        .and_then(|e| e.param(VIEW_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Whether deleting `target` (GlobalId) deletes `e` too: a source's parts; a first-level part's
/// siblings (every part of the source); a piece's siblings; a part's pieces and the merges that
/// use it.
pub fn depends_on(store: &Store, e: &Element, target: &str) -> bool {
    let Some(r) = recipe(e) else {
        return false;
    };
    if r.division.as_ref().is_some_and(|d| d.parent == target)
        || r.merged.iter().any(|m| m == target)
    {
        return true;
    }
    let Some(t) = find(store, target) else {
        return false;
    };
    let Some(tr) = recipe(t) else {
        return r.source == target;
    };
    match (&tr.division, tr.merged.is_empty()) {
        (Some(td), _) => r
            .division
            .as_ref()
            .is_some_and(|d| d.id == td.id && d.parent == td.parent),
        (None, true) => r.source == tr.source,
        (None, false) => false,
    }
}

/// After every edit: parts whose layer is gone (a type without it) are deleted with their
/// pieces and merges, and reported.
pub(crate) fn prune(txn: &mut Transaction<'_>) -> Result<()> {
    let store = txn.store().clone();
    let mut gone: HashSet<String> = HashSet::new();
    let mut reported = Vec::new();
    for e in store.iter() {
        let Some(r) = recipe(e) else {
            continue;
        };
        let (Some(i), None, true) = (r.layer, &r.division, r.merged.is_empty()) else {
            continue;
        };
        let Some(source) = find(&store, &r.source) else {
            continue;
        };
        let keep =
            matches!(layers_of(&store, source), Ok(Some(b)) if b.bands.iter().any(|x| x.0 == i));
        if !keep {
            gone.insert(e.guid().to_string());
            reported.push(e.id());
        }
    }
    if gone.is_empty() {
        return Ok(());
    }
    // With their pieces and the merges that use them.
    loop {
        let more: Vec<String> = store
            .iter()
            .filter(|e| !gone.contains(e.guid().as_str()))
            .filter(|e| {
                recipe(e).is_some_and(|r| {
                    r.division
                        .as_ref()
                        .is_some_and(|d| gone.contains(&d.parent))
                        || r.merged.iter().any(|m| gone.contains(m))
                })
            })
            .map(|e| e.guid().to_string())
            .collect();
        if more.is_empty() {
            break;
        }
        gone.extend(more);
    }
    let ids: Vec<ElementId> = store
        .iter()
        .filter(|e| gone.contains(e.guid().as_str()))
        .map(Element::id)
        .collect();
    for id in &ids {
        txn.delete(*id)?;
    }
    txn.warn(crate::txn::Warning {
        id: "parts.layer_gone".into(),
        message: format!(
            "{} part(s) deleted: their layers are no longer in the type",
            reported.len()
        ),
        element_ids: reported,
    });
    Ok(())
}

// ---- Commands ----

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &PartRecipe) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

fn new_part(
    txn: &mut Transaction<'_>,
    name: &str,
    r: &PartRecipe,
    like: Option<&Element>,
) -> Result<ElementId> {
    let mut n = NewElement::new(CLASS).name(name);
    match like {
        Some(p) => {
            for k in ["@PredefinedType", "@ObjectType"] {
                if let Some(v) = p.param(k) {
                    n = n.param(k, v.clone());
                }
            }
        }
        None => {
            n = n.param("@PredefinedType", "NOTDEFINED");
        }
    }
    txn.create(n.param(RECIPE_KEY, serde_json::to_value(r).expect("serialises")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The elements to divide into parts.
    pub host_ids: Vec<ElementRef>,
}

/// `parts.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for h in &p.host_ids {
        let Some(e) = store.resolve(h).and_then(|id| store.get(id)) else {
            return Err(Error::new("element.not_found", "no such element"));
        };
        if has_parts(&store, e) {
            skipped.push(e.id());
            continue;
        }
        let Ok(layers) = layers_of(&store, e) else {
            skipped.push(e.id());
            continue;
        };
        let source = e.guid().to_string();
        let base = e.name.clone().unwrap_or_else(|| e.class.clone());
        let rows: Vec<(Option<usize>, String, &str)> = match &layers {
            None => vec![(None, base.clone(), "NOTDEFINED")],
            Some(b) => {
                let c = crate::walls::layered::compound_of(&store, e).expect("layered");
                b.bands
                    .iter()
                    .map(|(i, _, _)| {
                        let l = &c.layers[*i];
                        let label = if l.material.trim().is_empty() {
                            l.function.name().to_owned()
                        } else {
                            format!("{} ({})", l.material, l.function.name())
                        };
                        let ty = if l.function == crate::compound::Function::ThermalAir {
                            "INSULATION"
                        } else {
                            "USERDEFINED"
                        };
                        (Some(*i), label, ty)
                    })
                    .collect()
            }
        };
        for (layer, name, ty) in rows {
            let r = PartRecipe {
                kind: RECIPE.into(),
                source: source.clone(),
                layer,
                division: None,
                merged: Vec::new(),
                excluded: false,
                material: None,
                base_level: None,
            };
            let mut n = NewElement::new(CLASS)
                .name(&name)
                .param("@PredefinedType", ty);
            if ty == "USERDEFINED" {
                let function = name
                    .rsplit_once(" (")
                    .map_or(name.as_str(), |x| x.1.trim_end_matches(')'));
                n = n.param("@ObjectType", function);
            }
            out.push(
                txn.create(n.param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")))?,
            );
        }
    }
    if out.is_empty() {
        return Err(Error::new(
            "parts.none",
            "No element can be divided into parts",
        ));
    }
    if !skipped.is_empty() {
        txn.warn(crate::txn::Warning {
            id: "parts.skipped".into(),
            message: format!(
                "{} element(s) cannot be divided into parts (or already are)",
                skipped.len()
            ),
            element_ids: skipped,
        });
    }
    Ok(out)
}

fn leaf_parts(store: &Store, ids: &[ElementRef]) -> Result<Vec<(ElementId, PartRecipe)>> {
    let mid = intermediates(store);
    ids.iter()
        .map(|r| {
            let e = store
                .resolve(r)
                .and_then(|id| store.get(id))
                .ok_or_else(|| Error::new("element.not_found", "no such part"))?;
            let p = recipe(e).ok_or_else(|| Error::new("parts.not", "not a part"))?;
            if mid.contains(e.guid().as_str()) {
                return Err(Error::new(
                    "parts.intermediate",
                    "the part is divided or merged: work on its pieces",
                ));
            }
            Ok((e.id(), p))
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineArg {
    pub start: [f64; 3],
    pub end: [f64; 3],
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefArg {
    /// A level, grid or reference plane.
    pub ref_id: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DivideParams {
    pub part_ids: Vec<ElementRef>,
    /// Sketched division lines; each cuts along the work plane's normal through it.
    #[serde(default)]
    pub lines: Vec<LineArg>,
    /// The sketch's work-plane normal (default: the normal of the first part's largest face).
    pub normal: Option<[f64; 3]>,
    /// Intersecting references.
    #[serde(default)]
    pub refs: Vec<RefArg>,
    /// Divider gap (default 0).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub gap: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Divided {
    pub division_id: String,
    pub parts: Vec<ElementId>,
}

/// `parts.divide`.
pub(crate) fn divide(txn: &mut Transaction<'_>, p: DivideParams) -> Result<Divided> {
    let store = txn.store().clone();
    let parts = leaf_parts(&store, &p.part_ids)?;
    if parts.is_empty() {
        return Err(Error::new("parts.none_selected", "Select parts to divide"));
    }
    let gap = p.gap.unwrap_or(0.0);
    if gap < 0.0 {
        return Err(Error::new("parts.gap", "the gap cannot be negative"));
    }
    let bodies = crate::bodies::all(&store);
    let mut cutters = Vec::new();
    if !p.lines.is_empty() {
        let first = store.get(parts[0].0).expect("listed");
        let wn = match p.normal {
            Some(n) => n,
            None => largest_face(&geometry(&store, first, &bodies)).1,
        };
        for l in &p.lines {
            let dir = crate::work_planes::sub(l.end, l.start);
            let n =
                crate::work_planes::unit(crate::work_planes::cross(dir, wn)).ok_or_else(|| {
                    Error::new(
                        "parts.line",
                        "a division line runs along the work plane's normal",
                    )
                })?;
            cutters.push(Cutter::Plane {
                point: l.start,
                normal: n,
            });
        }
    }
    for r in &p.refs {
        let e = store
            .resolve(&r.ref_id)
            .and_then(|id| store.get(id))
            .ok_or_else(|| Error::new("element.not_found", "no such reference"))?;
        let c = Cutter::Ref {
            guid: e.guid().to_string(),
            offset: r.offset,
        };
        if resolve(&store, &c).is_none() {
            return Err(Error::new(
                "parts.reference",
                "divide by levels, grids or reference planes",
            ));
        }
        cutters.push(c);
    }
    if cutters.is_empty() {
        return Err(Error::new(
            "parts.no_cutters",
            "Give division lines or references",
        ));
    }
    let planes: Vec<([f64; 3], f64)> = cutters
        .iter()
        .map(|c| resolve(&store, c).expect("checked"))
        .collect();
    let division_id = GlobalId::new().to_string();
    let mut made = Vec::new();
    for (id, _) in &parts {
        let e = store.get(*id).expect("listed");
        let mut cells: Vec<(String, Vec<Mesh>)> =
            vec![(String::new(), geometry(&store, e, &bodies))];
        for plane in &planes {
            let mut next = Vec::new();
            for (cell, m) in cells {
                for sign in ['+', '-'] {
                    let piece = side(&m, *plane, sign, gap);
                    if !piece.is_empty() {
                        next.push((format!("{cell}{sign}"), piece));
                    }
                }
            }
            cells = next;
        }
        if cells.len() < 2 {
            continue;
        }
        let name = e.name.clone().unwrap_or_default();
        for (cell, _) in cells {
            let r = PartRecipe {
                kind: RECIPE.into(),
                source: recipe(e).expect("a part").source,
                layer: None,
                division: Some(Division {
                    id: division_id.clone(),
                    parent: e.guid().to_string(),
                    cutters: cutters.clone(),
                    gap,
                    cell,
                }),
                merged: Vec::new(),
                excluded: false,
                material: None,
                base_level: None,
            };
            made.push(new_part(txn, &name, &r, Some(e))?);
        }
    }
    if made.is_empty() {
        return Err(Error::new(
            "parts.no_cut",
            "The division does not cut the parts",
        ));
    }
    Ok(Divided {
        division_id,
        parts: made,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdsParams {
    pub part_ids: Vec<ElementRef>,
}

/// `parts.merge`.
pub(crate) fn merge(txn: &mut Transaction<'_>, p: IdsParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let parts = leaf_parts(&store, &p.part_ids)?;
    if parts.len() < 2 {
        return Err(Error::new("parts.merge_few", "Select at least two parts"));
    }
    let m0 = material(&store, &parts[0].1);
    if parts.iter().any(|(_, r)| material(&store, r) != m0) {
        return Err(Error::new(
            "parts.merge_material",
            "Parts with different materials cannot be merged",
        ));
    }
    // Connected: their boxes touch, part to part, across the whole set.
    let bodies = crate::bodies::all(&store);
    let boxes: Vec<([f64; 3], [f64; 3])> = parts
        .iter()
        .map(|(id, _)| {
            let m = geometry(&store, store.get(*id).expect("listed"), &bodies);
            Body {
                element: *id,
                parts: m
                    .into_iter()
                    .map(|mesh| crate::bodies::Part {
                        mesh,
                        class: String::new(),
                        plan_hidden: false,
                    })
                    .collect(),
            }
            .bounds()
            .unwrap_or(([0.0; 3], [0.0; 3]))
        })
        .collect();
    let touch = |a: &([f64; 3], [f64; 3]), b: &([f64; 3], [f64; 3])| {
        (0..3).all(|k| a.0[k] <= b.1[k] + 1e-4 && b.0[k] <= a.1[k] + 1e-4)
    };
    let mut reached = vec![false; parts.len()];
    reached[0] = true;
    let mut queue = vec![0];
    while let Some(i) = queue.pop() {
        for j in 0..parts.len() {
            if !reached[j] && touch(&boxes[i], &boxes[j]) {
                reached[j] = true;
                queue.push(j);
            }
        }
    }
    if reached.iter().any(|r| !r) {
        return Err(Error::new(
            "parts.merge_apart",
            "Merged part would have disconnected pieces",
        ));
    }
    let first = store.get(parts[0].0).expect("listed");
    let r0 = &parts[0].1;
    let r = PartRecipe {
        kind: RECIPE.into(),
        source: r0.source.clone(),
        layer: None,
        division: None,
        merged: parts
            .iter()
            .map(|(id, _)| store.get(*id).expect("listed").guid().to_string())
            .collect(),
        excluded: false,
        material: r0.material.clone(),
        base_level: r0.base_level.clone(),
    };
    let name = first.name.clone().unwrap_or_default();
    new_part(txn, &name, &r, Some(first))
}

/// `parts.exclude`.
pub(crate) fn exclude(txn: &mut Transaction<'_>, p: IdsParams) -> Result<()> {
    set_excluded(txn, p, true)
}

/// `parts.restore`.
pub(crate) fn restore(txn: &mut Transaction<'_>, p: IdsParams) -> Result<()> {
    set_excluded(txn, p, false)
}

fn set_excluded(txn: &mut Transaction<'_>, p: IdsParams, excluded: bool) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.part_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such part"))?;
        let mut pr = store
            .get(id)
            .and_then(recipe)
            .ok_or_else(|| Error::new("parts.not", "not a part"))?;
        pr.excluded = excluded;
        write(txn, id, &pr)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LevelChoice {
    ByOriginal,
    NonAssociated,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverridesParams {
    pub part_ids: Vec<ElementRef>,
    /// A material name, or empty for Material By Original.
    pub material: Option<String>,
    /// Base Level: a level …
    pub base_level_id: Option<ElementRef>,
    /// … or By Original or Non Associated.
    pub base_level: Option<LevelChoice>,
}

/// `parts.set_overrides`.
pub(crate) fn set_overrides(txn: &mut Transaction<'_>, p: OverridesParams) -> Result<()> {
    let store = txn.store().clone();
    let level = match (&p.base_level_id, p.base_level) {
        (Some(l), _) => {
            let e = store
                .resolve(l)
                .and_then(|id| store.get(id))
                .filter(|e| e.class == crate::levels::CLASS)
                .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
            Some(Some(e.guid().to_string()))
        }
        (None, Some(LevelChoice::ByOriginal)) => Some(None),
        (None, Some(LevelChoice::NonAssociated)) => Some(Some(String::new())),
        (None, None) => None,
    };
    for r in &p.part_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such part"))?;
        let mut pr = store
            .get(id)
            .and_then(recipe)
            .ok_or_else(|| Error::new("parts.not", "not a part"))?;
        if let Some(m) = &p.material {
            pr.material = (!m.trim().is_empty()).then(|| m.trim().to_owned());
        }
        if let Some(l) = &level {
            pr.base_level = l.clone();
        }
        write(txn, id, &pr)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisibilityParams {
    pub view_id: ElementRef,
    pub mode: Visibility,
}

/// `view.set_parts_visibility`.
pub(crate) fn set_visibility(txn: &mut Transaction<'_>, p: VisibilityParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.view_id)
        .filter(|id| store.get(*id).is_some_and(crate::views::is_view))
        .ok_or_else(|| Error::new("element.not_found", "no such view"))?;
    let v = serde_json::to_value(p.mode).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(VIEW_KEY.into(), v);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {
    pub part_id: ElementRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct PartInfo {
    pub volume: f64,
    /// The largest face's area.
    pub area: f64,
    pub material: String,
    pub material_by_original: bool,
    /// `Core` or `Finish`.
    pub construction: String,
    pub original_category: String,
    pub original_type: Option<String>,
    /// The level's GlobalId; empty when Non Associated.
    pub base_level: Option<String>,
    pub excluded: bool,
    /// Divided or merged: kept, not drawn.
    pub intermediate: bool,
}

/// `parts.info`.
pub(crate) fn info(store: &Store, p: InfoParams) -> Result<PartInfo> {
    let e = store
        .resolve(&p.part_id)
        .and_then(|id| store.get(id))
        .ok_or_else(|| Error::new("element.not_found", "no such part"))?;
    let r = recipe(e).ok_or_else(|| Error::new("parts.not", "not a part"))?;
    let bodies = crate::bodies::all(store);
    let m = geometry(store, e, &bodies);
    let source = find(store, &r.source);
    let base_level = match &r.base_level {
        Some(l) => Some(l.clone()),
        None => source.and_then(|s| {
            let v = s.param(RECIPE_KEY)?;
            v.get("level")
                .or_else(|| v.get("base_level"))?
                .as_str()
                .map(str::to_owned)
        }),
    };
    Ok(PartInfo {
        volume: volume(&m),
        area: largest_face(&m).0,
        material: material(store, &r),
        material_by_original: r.material.is_none(),
        construction: construction(store, &r).into(),
        original_category: source.map(|s| s.class.clone()).unwrap_or_default(),
        original_type: source
            .and_then(|s| s.param("@type")?.as_str().map(str::to_owned))
            .and_then(|g| find(store, &g)?.name.clone()),
        base_level,
        excluded: r.excluded,
        intermediate: intermediates(store).contains(e.guid().as_str()),
    })
}

/// The elements to move or copy without the parts whose originals go with them; a part on
/// its own is refused.
pub(crate) fn without_carried(store: &Store, ids: Vec<ElementId>) -> Result<Vec<ElementId>> {
    let guids: HashSet<String> = ids
        .iter()
        .filter_map(|id| store.get(*id))
        .map(|e| e.guid().to_string())
        .collect();
    let mut out = Vec::new();
    for id in ids {
        match store.get(id).and_then(recipe) {
            Some(r) if guids.contains(&r.source) => {}
            Some(_) => {
                return Err(Error::new(
                    "parts.cannot_move",
                    "Parts follow their original element: move it instead",
                ));
            }
            None => out.push(id),
        }
    }
    Ok(out)
}
