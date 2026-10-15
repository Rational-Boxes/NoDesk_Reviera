// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Areas and area schemes (spec Ch 3 §3.9.3, ARC-AREA-001).
//!
//! - **Schemes** (`rxd:area_schemes` on the project, *Gross Building* and *Rentable* by default)
//!   list their area types and a rule table: for the area types on the two sides of a wall
//!   boundary, whether it sits on the wall's centreline or on the face toward one of them.
//! - **Area plans** are plan views (`view.area_plan`) of a scheme on a level.
//! - **Area boundaries** (`IfcVirtualElement`, `recipe.area_boundary`) are lines of a scheme on
//!   a level: drawn (fixed), or picked from a wall (they follow it). A picked boundary with
//!   *Apply Area Rules* is placed by the rule for the areas either side of it, found with every
//!   wall boundary on its centreline first.
//! - **Areas** (`IfcSpace`, `recipe.area`) are placed at a point: their region is the closed
//!   loop of their scheme's boundaries on their level around it.
//!
//! Not built yet: the glass line, Area Boundary Pick Lines with an offset in the window,
//! unplaced areas, area tags, per-scheme area schedules, `IfcZone` groups and `IfcVirtualElement`
//! boundaries' scheme property in IFC.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::arrangement::{Edge, Region};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::{Line, RECIPE_KEY};

pub const SCHEMES_KEY: &str = "rxd:area_schemes";
pub const BOUNDARY_RECIPE: &str = "recipe.area_boundary";
pub const AREA_RECIPE: &str = "recipe.area";
pub const KIND_AREA_PLAN: &str = "view.area_plan";
/// Outside every area.
pub const OUTSIDE: &str = "";

/// Where a wall boundary sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    Centerline,
    /// The face toward the area of `type_a`.
    FaceA,
    /// The face toward `type_b` (`""` is outside every area).
    FaceB,
}

/// A rule: area types either side (`*` any, `""` outside) and the boundary's position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rule {
    pub type_a: String,
    pub type_b: String,
    pub position: Position,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AreaScheme {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub types: Vec<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

fn rule(a: &str, b: &str, position: Position) -> Rule {
    Rule {
        type_a: a.into(),
        type_b: b.into(),
        position,
    }
}

/// The two schemes every project starts with.
fn defaults() -> Vec<AreaScheme> {
    use Position::*;
    vec![
        AreaScheme {
            id: "gross_building".into(),
            name: "Gross Building".into(),
            description: "Total constructed area of a building".into(),
            types: vec!["Gross Building Area".into(), "Exterior Area".into()],
            rules: vec![
                rule("Gross Building Area", OUTSIDE, FaceB),
                rule("Exterior Area", "Gross Building Area", FaceA),
                rule("Exterior Area", "Exterior Area", Centerline),
            ],
        },
        AreaScheme {
            id: "rentable".into(),
            name: "Rentable".into(),
            description: "Area measurements based on the standard BOMA method".into(),
            types: vec![
                "Office Area".into(),
                "Building Common Area".into(),
                "Store Area".into(),
                "Major Vertical Penetration".into(),
                "Exterior Area".into(),
            ],
            rules: vec![
                rule("Office Area", "Office Area", Centerline),
                rule("Office Area", "Exterior Area", FaceA),
                rule("Office Area", OUTSIDE, FaceA),
                rule("Office Area", "Building Common Area", Centerline),
                rule("Store Area", "*", Centerline),
                rule("Major Vertical Penetration", "*", FaceA),
            ],
        },
    ]
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// The model's area schemes (the defaults until any is changed).
pub fn schemes(store: &Store) -> Vec<AreaScheme> {
    project(store)
        .and_then(|p| p.param(SCHEMES_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(defaults)
}

pub fn scheme(store: &Store, r: &str) -> Option<AreaScheme> {
    schemes(store)
        .into_iter()
        .find(|s| s.id == r || s.name.eq_ignore_ascii_case(r))
}

fn write_schemes(txn: &mut Transaction<'_>, list: &[AreaScheme]) -> Result<()> {
    let p = project(txn.store())
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(list).expect("serialises");
    txn.modify(p, |e| {
        e.params.insert(SCHEMES_KEY.into(), v);
    })
}

/// The boundary position for area types `a` (one side) and `b` (the other); centreline when no
/// rule applies. `true` in the result means "toward `a`".
fn position(s: &AreaScheme, a: &str, b: &str) -> (Position, bool) {
    let fits = |pattern: &str, t: &str| pattern == "*" || pattern == t;
    for r in &s.rules {
        if fits(&r.type_a, a) && fits(&r.type_b, b) {
            return (r.position, true);
        }
        if fits(&r.type_a, b) && fits(&r.type_b, a) {
            return (r.position, false);
        }
    }
    (Position::Centerline, true)
}

// ---- Boundaries ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoundaryRecipe {
    pub kind: String,
    pub scheme: String,
    /// The level (GlobalId).
    pub level: String,
    /// A drawn boundary's line (also kept for a picked one, as last placed).
    pub line: Line,
    /// The wall it was picked from (GlobalId).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall: Option<String>,
    #[serde(default)]
    pub apply_rules: bool,
    /// A picked boundary without rules: across from the wall's centreline, toward its exterior.
    #[serde(default)]
    pub offset: f64,
}

pub fn boundary_recipe(e: &Element) -> Option<BoundaryRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == BOUNDARY_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AreaRecipe {
    pub kind: String,
    pub scheme: String,
    pub level: String,
    pub point: [f64; 2],
    pub area_type: String,
}

pub fn area_recipe(e: &Element) -> Option<AreaRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == AREA_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// An area as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct AreaInfo {
    pub id: ElementId,
    pub scheme: String,
    pub level: String,
    pub area_type: String,
    pub region: Option<Region>,
    pub area: f64,
    pub perimeter: f64,
}

fn wall_of<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    store.get(store.find_by_guid(&GlobalId::parse(guid)?)?)
}

/// A wall's line across at `along` from its centreline (toward its exterior), as long as the
/// wall.
fn wall_line(store: &Store, guid: &str, across: f64) -> Option<Line> {
    let e = wall_of(store, guid)?;
    let g = crate::walls::regenerate(store, e).ok()?;
    let r = crate::walls::wall_recipe(e)?;
    let ext = if r.flip { -1.0 } else { 1.0 };
    let n = g.normal();
    let d = across * ext;
    // Run on past the ends by the wall's width, so that lines off the centreline still meet
    // their neighbours' (the region search leaves the loose ends out).
    let u = g.direction();
    let x = g.width;
    Some(Line {
        start: [
            g.start[0] + n[0] * d - u[0] * x,
            g.start[1] + n[1] * d - u[1] * x,
        ],
        end: [
            g.end[0] + n[0] * d + u[0] * x,
            g.end[1] + n[1] * d + u[1] * x,
        ],
    })
}

/// The boundaries of a scheme on a level, as edges for the regions; `centre` puts every rules
/// boundary on its wall's centreline (the first pass).
fn edges(store: &Store, scheme: &AreaScheme, level: &str, centre: bool) -> Vec<(ElementId, Line)> {
    let all: Vec<(ElementId, BoundaryRecipe)> = store
        .iter()
        .filter_map(|e| Some((e.id(), boundary_recipe(e)?)))
        .filter(|(_, b)| b.scheme == scheme.id && b.level == level)
        .collect();
    let first: Vec<(ElementId, Line)> = all
        .iter()
        .filter_map(|(id, b)| {
            let line = match &b.wall {
                Some(w) if b.apply_rules => wall_line(store, w, 0.0)?,
                Some(w) => wall_line(store, w, b.offset)?,
                None => b.line,
            };
            Some((*id, line))
        })
        .collect();
    if centre {
        return first;
    }
    // The areas either side of each rules boundary, from the first pass.
    let areas: Vec<(AreaRecipe, Option<Region>)> = areas_on(store, scheme, level)
        .into_iter()
        .map(|(_, r)| {
            let region = crate::arrangement::region_around(&as_edges(&first), r.point);
            (r, region)
        })
        .collect();
    let type_at = |p: [f64; 2]| -> String {
        areas
            .iter()
            .filter(|(_, reg)| reg.as_ref().is_some_and(|g| g.contains(p)))
            .map(|(r, _)| r.area_type.clone())
            .next()
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for ((id, b), (_, line)) in all.iter().zip(first.iter()) {
        let Some(w) = b.wall.as_ref().filter(|_| b.apply_rules) else {
            out.push((*id, *line));
            continue;
        };
        let Some(e) = wall_of(store, w) else { continue };
        let (Ok(g), Some(r)) = (
            crate::walls::regenerate(store, e),
            crate::walls::wall_recipe(e),
        ) else {
            continue;
        };
        let ext = if r.flip { -1.0 } else { 1.0 };
        let (n, u, len) = (g.normal(), g.direction(), g.length());
        // Split where the other first-pass boundaries cross the wall's centreline: each piece
        // is placed by the areas beside it.
        let mut cuts: Vec<f64> = first
            .iter()
            .filter(|(other, _)| other != id)
            .filter_map(|(_, l)| crossing(g.start, g.end, l.start, l.end))
            .map(|t| t * len)
            .filter(|t| *t > 1e-6 && *t < len - 1e-6)
            .collect();
        cuts.push(0.0);
        cuts.push(len);
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        let face = g.width / 2.0;
        let probe = face + 0.05;
        let point = |t: f64, s: f64| {
            [
                g.start[0] + u[0] * t + n[0] * ext * s,
                g.start[1] + u[1] * t + n[1] * ext * s,
            ]
        };
        let acrosses: Vec<f64> = cuts
            .windows(2)
            .map(|w2| {
                let mid = (w2[0] + w2[1]) / 2.0;
                // Side A: the wall's exterior; side B: its interior.
                let (ta, tb) = (type_at(point(mid, probe)), type_at(point(mid, -probe)));
                match position(scheme, &ta, &tb) {
                    (Position::Centerline, _) => 0.0,
                    (Position::FaceA, true) | (Position::FaceB, false) => face,
                    (Position::FaceA, false) | (Position::FaceB, true) => -face,
                }
            })
            .collect();
        let last = acrosses.len().saturating_sub(1);
        for (k, (w2, a)) in cuts.windows(2).zip(&acrosses).enumerate() {
            // The outer ends run on past the wall by its width, to meet the neighbours.
            let t0 = if k == 0 { w2[0] - g.width } else { w2[0] };
            let t1 = if k == last { w2[1] + g.width } else { w2[1] };
            out.push((
                *id,
                Line {
                    start: point(t0, *a),
                    end: point(t1, *a),
                },
            ));
            // A step where the next piece sits on another face.
            if let Some(next) = acrosses.get(k + 1)
                && (next - a).abs() > 1e-9
            {
                out.push((
                    *id,
                    Line {
                        start: point(w2[1], *a),
                        end: point(w2[1], *next),
                    },
                ));
            }
        }
    }
    out
}

/// Where segment `a`–`b` crosses segment `c`–`d`, as a fraction along `a`–`b`.
fn crossing(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Option<f64> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let q = [c[0] - a[0], c[1] - a[1]];
    let t = (q[0] * s[1] - q[1] * s[0]) / den;
    let v = (q[0] * r[1] - q[1] * r[0]) / den;
    ((-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&v)).then_some(t)
}

fn as_edges(lines: &[(ElementId, Line)]) -> Vec<Edge> {
    lines
        .iter()
        .map(|(id, l)| Edge {
            line: *l,
            lock: Some((*id, "line".into())),
        })
        .collect()
}

fn areas_on(store: &Store, scheme: &AreaScheme, level: &str) -> Vec<(ElementId, AreaRecipe)> {
    store
        .iter()
        .filter_map(|e| Some((e.id(), area_recipe(e)?)))
        .filter(|(_, a)| a.scheme == scheme.id && a.level == level)
        .collect()
}

/// A boundary's line as placed now.
pub fn boundary_line(store: &Store, e: &Element) -> Option<Line> {
    let b = boundary_recipe(e)?;
    let s = scheme(store, &b.scheme)?;
    edges(store, &s, &b.level, false)
        .into_iter()
        .find(|(id, _)| *id == e.id())
        .map(|(_, l)| l)
}

/// Every area, with its region.
pub fn all(store: &Store) -> Vec<AreaInfo> {
    let mut out = Vec::new();
    let mut cache: std::collections::BTreeMap<(String, String), Vec<Edge>> = Default::default();
    for e in store.iter() {
        let Some(a) = area_recipe(e) else { continue };
        let Some(s) = scheme(store, &a.scheme) else {
            continue;
        };
        let edges = cache
            .entry((s.id.clone(), a.level.clone()))
            .or_insert_with(|| as_edges(&edges(store, &s, &a.level, false)));
        let region = crate::arrangement::region_around(edges, a.point);
        let (area, perimeter) = region
            .as_ref()
            .map_or((0.0, 0.0), |r| (r.area(), r.perimeter()));
        out.push(AreaInfo {
            id: e.id(),
            scheme: a.scheme,
            level: a.level,
            area_type: a.area_type,
            region,
            area,
            perimeter,
        });
    }
    out
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSchemeParams {
    pub name: String,
    pub description: Option<String>,
    /// Its area types; omitted, one ("Area").
    #[serde(default)]
    pub types: Vec<String>,
}

/// `areascheme.create`: its key.
pub(crate) fn create_scheme(txn: &mut Transaction<'_>, p: CreateSchemeParams) -> Result<String> {
    let mut list = schemes(txn.store());
    let name = p.name.trim().to_owned();
    if name.is_empty() || list.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
        return Err(Error::new(
            "areascheme.name",
            "an area scheme needs a name of its own",
        ));
    }
    let id = GlobalId::new().to_string();
    list.push(AreaScheme {
        id: id.clone(),
        name,
        description: p.description.unwrap_or_default(),
        types: if p.types.is_empty() {
            vec!["Area".into()]
        } else {
            p.types
        },
        rules: Vec::new(),
    });
    write_schemes(txn, &list)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRulesParams {
    pub scheme_id: String,
    pub rules: Vec<Rule>,
    /// Replace the area types too.
    pub types: Option<Vec<String>>,
}

/// `areascheme.set_rules`.
pub(crate) fn set_rules(txn: &mut Transaction<'_>, p: SetRulesParams) -> Result<()> {
    let mut list = schemes(txn.store());
    let s = list
        .iter_mut()
        .find(|s| s.id == p.scheme_id || s.name.eq_ignore_ascii_case(&p.scheme_id))
        .ok_or_else(|| Error::new("areascheme.not_found", "no such area scheme"))?;
    s.rules = p.rules;
    if let Some(t) = p.types {
        s.types = t;
    }
    write_schemes(txn, &list)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePlanParams {
    pub scheme_id: String,
    pub level_ids: Vec<ElementRef>,
    pub scale: Option<f64>,
    /// Boundaries on the walls round the building's outside (rules applied).
    #[serde(default)]
    pub auto_boundaries: bool,
}

/// `areaplan.create`: area plans of a scheme on levels, optionally with boundaries along the
/// outside walls.
pub(crate) fn create_plan(
    txn: &mut Transaction<'_>,
    p: CreatePlanParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let s = scheme(&store, &p.scheme_id)
        .ok_or_else(|| Error::new("areascheme.not_found", "no such area scheme"))?;
    let mut out = Vec::new();
    for l in &p.level_ids {
        let ids = crate::plans::create_plan(
            txn,
            crate::plans::CreatePlanParams {
                kind: crate::plans::PlanKind::FloorPlan,
                level_ids: vec![l.clone()],
                allow_duplicates: true,
                scale: p.scale,
            },
        )?;
        for id in ids {
            let store = txn.store().clone();
            let mut data =
                crate::views::view_data(store.get(id).expect("created")).expect("a view");
            data.kind = KIND_AREA_PLAN.into();
            data.area_scheme = Some(s.id.clone());
            let level_name = store
                .resolve(l)
                .and_then(|lid| store.get(lid))
                .and_then(|e| e.name.clone())
                .unwrap_or_default();
            crate::views::write_data(txn, id, &data)?;
            txn.modify(id, |e| e.name = Some(format!("{level_name} ({})", s.name)))?;
            out.push(id);
        }
        if p.auto_boundaries {
            let store = txn.store().clone();
            let level = store
                .resolve(l)
                .and_then(|lid| store.get(lid))
                .map(|e| e.guid().to_string())
                .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
            for w in outside_walls(&store, &level) {
                make_boundary(txn, &s.id, &level, None, Some(w), true, 0.0)?;
            }
        }
    }
    Ok(out)
}

/// The walls on a level whose faces make the building's outline.
fn outside_walls(store: &Store, level: &str) -> Vec<String> {
    let solved = crate::walls::solve(store);
    let walls: Vec<(String, [[f64; 2]; 4])> = solved
        .bodies
        .iter()
        .filter_map(|(id, b)| {
            let e = store.get(*id)?;
            let r = crate::walls::wall_recipe(e)?;
            (r.base_level == level).then(|| (e.guid().to_string(), b.footprint))
        })
        .collect();
    let outline =
        crate::polygons::union(&walls.iter().map(|(_, f)| f.to_vec()).collect::<Vec<_>>());
    // A wall is outside when one of its long faces lies on an outer ring.
    let on_ring = |a: [f64; 2], b: [f64; 2]| {
        outline.iter().any(|s| {
            let r = &s.outer;
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            (0..r.len()).any(|k| {
                let (p, q) = (r[k], r[(k + 1) % r.len()]);
                let l = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2))
                    .sqrt()
                    .max(1e-12);
                let d =
                    ((q[0] - p[0]) * (mid[1] - p[1]) - (q[1] - p[1]) * (mid[0] - p[0])).abs() / l;
                let t =
                    ((mid[0] - p[0]) * (q[0] - p[0]) + (mid[1] - p[1]) * (q[1] - p[1])) / (l * l);
                d < 1e-4 && (-1e-6..=1.0 + 1e-6).contains(&t)
            })
        })
    };
    walls
        .into_iter()
        .filter(|(_, f)| on_ring(f[0], f[1]) || on_ring(f[3], f[2]))
        .map(|(g, _)| g)
        .collect()
}

fn make_boundary(
    txn: &mut Transaction<'_>,
    scheme: &str,
    level: &str,
    line: Option<Line>,
    wall: Option<String>,
    apply_rules: bool,
    offset: f64,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let line = match (&line, &wall) {
        (Some(l), _) => *l,
        (None, Some(w)) => wall_line(&store, w, 0.0)
            .ok_or_else(|| Error::new("element.not_found", "the wall does not regenerate"))?,
        _ => return Err(Error::new("areaboundary.nothing", "give a line or a wall")),
    };
    let r = BoundaryRecipe {
        kind: BOUNDARY_RECIPE.into(),
        scheme: scheme.into(),
        level: level.into(),
        line,
        wall,
        apply_rules,
        offset,
    };
    txn.create(
        NewElement::new("IfcVirtualElement")
            .name("Area Boundary")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateBoundaryParams {
    /// An area plan (its scheme and level).
    pub view_id: ElementRef,
    /// Drawn lines …
    #[serde(default)]
    pub lines: Vec<Line>,
    /// … or walls picked (they follow them).
    #[serde(default)]
    pub wall_ids: Vec<ElementRef>,
    /// Picked walls: place each by the scheme's rules.
    #[serde(default)]
    pub apply_rules: bool,
    /// Picked walls without rules: across from the centreline toward the exterior (metres).
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
}

fn area_plan(store: &Store, r: &ElementRef) -> Result<(String, String)> {
    let (_, data) = crate::views::resolve_view(store, r)?;
    match (data.area_scheme, data.level) {
        (Some(s), Some(l)) => Ok((s, l)),
        _ => Err(Error::new(
            "view.not_area_plan",
            "the view is not an area plan",
        )),
    }
}

/// `areaboundary.create`.
pub(crate) fn create_boundary(
    txn: &mut Transaction<'_>,
    p: CreateBoundaryParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (scheme, level) = area_plan(&store, &p.view_id)?;
    let mut out = Vec::new();
    for l in &p.lines {
        if l.length() < 1e-6 {
            continue;
        }
        out.push(make_boundary(
            txn,
            &scheme,
            &level,
            Some(*l),
            None,
            false,
            0.0,
        )?);
    }
    for w in &p.wall_ids {
        let id = store
            .resolve(w)
            .filter(|id| store.get(*id).and_then(crate::walls::wall_recipe).is_some())
            .ok_or_else(|| Error::new("element.not_found", "no such wall"))?;
        let guid = store.get(id).expect("resolved").guid().to_string();
        out.push(make_boundary(
            txn,
            &scheme,
            &level,
            None,
            Some(guid),
            p.apply_rules,
            p.offset,
        )?);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    pub view_id: ElementRef,
    pub point: [f64; 2],
    /// One of the scheme's area types; omitted, its first.
    pub area_type: Option<String>,
    pub name: Option<String>,
    pub number: Option<String>,
}

/// `area.place`.
pub(crate) fn place(txn: &mut Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (scheme_id, level) = area_plan(&store, &p.view_id)?;
    let s = scheme(&store, &scheme_id)
        .ok_or_else(|| Error::new("areascheme.not_found", "the plan's area scheme is gone"))?;
    let area_type = match p.area_type {
        Some(t) if s.types.contains(&t) => t,
        Some(t) => {
            return Err(Error::new(
                "area.type",
                format!(
                    "`{t}` is not an area type of {} ({})",
                    s.name,
                    s.types.join(", ")
                ),
            ));
        }
        None => s.types.first().cloned().unwrap_or_else(|| "Area".into()),
    };
    let count = store.iter().filter(|e| area_recipe(e).is_some()).count();
    let r = AreaRecipe {
        kind: AREA_RECIPE.into(),
        scheme: s.id.clone(),
        level,
        point: p.point,
        area_type: area_type.clone(),
    };
    let predefined = if s.id == "gross_building" {
        "GFA"
    } else {
        "USERDEFINED"
    };
    // Like a room: the element's name is its number, and its name a parameter.
    txn.create(
        NewElement::new("IfcSpace")
            .name(p.number.unwrap_or_else(|| (count + 1).to_string()))
            .param(
                crate::rooms::NAME_KEY,
                p.name.unwrap_or_else(|| area_type.clone()),
            )
            .param("@PredefinedType", predefined)
            .param("@ObjectType", s.name.as_str())
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTypeParams {
    pub area_ids: Vec<ElementRef>,
    pub area_type: String,
}

/// `area.set_type`: changes areas' area type (picked boundaries with rules move).
pub(crate) fn set_type(txn: &mut Transaction<'_>, p: SetTypeParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.area_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such area"))?;
        let mut a = store
            .get(id)
            .and_then(area_recipe)
            .ok_or_else(|| Error::new("area.not_area", "not an area"))?;
        let s = scheme(&store, &a.scheme)
            .ok_or_else(|| Error::new("areascheme.not_found", "the area's scheme is gone"))?;
        if !s.types.contains(&p.area_type) {
            return Err(Error::new(
                "area.type",
                format!("`{}` is not an area type of {}", p.area_type, s.name),
            ));
        }
        a.area_type = p.area_type.clone();
        let v = serde_json::to_value(&a).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct AreaListItem {
    pub id: ElementId,
    pub scheme: String,
    pub area_type: String,
    pub name: String,
    pub number: String,
    pub area: f64,
    pub enclosed: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// One scheme's areas; omitted, all.
    pub scheme_id: Option<String>,
}

/// `area.list`.
pub(crate) fn list(store: &Store, p: ListParams) -> Result<Vec<AreaListItem>> {
    let want = match &p.scheme_id {
        Some(s) => Some(
            scheme(store, s)
                .ok_or_else(|| Error::new("areascheme.not_found", "no such area scheme"))?
                .id,
        ),
        None => None,
    };
    Ok(all(store)
        .into_iter()
        .filter(|a| want.as_ref().is_none_or(|w| *w == a.scheme))
        .map(|a| {
            let e = store.get(a.id).expect("listed");
            let (number, name) = crate::rooms::number_and_name(e);
            AreaListItem {
                id: a.id,
                scheme: a.scheme,
                area_type: a.area_type,
                name,
                number,
                area: a.area,
                enclosed: a.region.is_some(),
            }
        })
        .collect())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchemesParams {}

/// `areascheme.list`.
pub(crate) fn list_schemes(store: &Store, _: SchemesParams) -> Result<Vec<AreaScheme>> {
    Ok(schemes(store))
}
