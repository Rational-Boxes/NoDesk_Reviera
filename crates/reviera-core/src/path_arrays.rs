// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Array along a path (ARC-MOD-033): a component repeated along a path, staying parametric even
//! when the component is a plain mesh.
//!
//! A path array is one element (an `IfcElementAssembly`, `ObjectType` "PathArray") with a
//! `recipe.path_array`: its source components (custom object types, with weights when there are
//! several), its path, the distribution and its options. Its members are generated from them
//! whenever it regenerates, so they follow the path element when it changes:
//!
//! - **Paths**: drawn (a polyline, possibly closed; an arc through three points; a spline through
//!   points), or picked: a wall's location line (straight or curved), a grid line, or the edges of
//!   a host with a boundary sketch (a floor, ceiling or deck), one closed loop, with the copies on
//!   the host's top.
//! - **Distribution**: a number of copies, a fixed distance (the remainder at the start, the end
//!   or both), or a maximum or minimum spacing. **Corners**: spaced through them, a copy at every
//!   corner with the rest spaced along each side (with a count per side), or corners left empty.
//! - **Orientation**: along the path, fixed, or facing outward or inward (perpendicular; for a
//!   closed path, away from or towards its inside), plus an offset.
//! - **Variation** (seeded, stable): a random turn within a range, a random scale within a range,
//!   a weighted choice among the sources, and a random offset across the path.
//! - **Members** can be excluded by index; Unarray turns them into independent copies.
//!
//! In IFC each member is an element of its component's class with an `IfcMappedItem` of the
//! type's representation map, aggregated by the array.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mesh_objects::{self, MeshObjectGeometry, Placement};
use crate::store::Store;
use crate::walls::{Line, RECIPE_KEY};

pub const RECIPE: &str = "recipe.path_array";
pub const CLASS: &str = "IfcElementAssembly";
/// Most members an array may have.
pub const MAX_MEMBERS: usize = 10_000;

/// A drawn path's shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// Straight segments between the points (closed: back to the first).
    #[default]
    Polyline,
    /// An arc from the first point through the second to the third.
    Arc,
    /// A smooth curve through the points.
    Spline,
}

/// Where the path comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PathSpec {
    /// Drawn in plan.
    Drawn {
        points: Vec<[f64; 2]>,
        #[serde(default)]
        shape: Shape,
        #[serde(default)]
        closed: bool,
    },
    /// A wall's location line or a grid line (GlobalId).
    Element { element: String },
    /// The edges of a host with a boundary (a floor, ceiling or deck; GlobalId), one loop.
    HostEdges {
        element: String,
        #[serde(default)]
        r#loop: usize,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// `value` copies (per side with corners, or `counts`).
    #[default]
    Number,
    /// Every `value` metres.
    Fixed,
    /// The fewest equal spaces no larger than `value`.
    Max,
    /// The most equal spaces no smaller than `value`.
    Min,
}

/// Where a fixed distance's remainder goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Justify {
    /// The copies start at the start; the remainder is at the end.
    #[default]
    Start,
    End,
    /// Split at both ends.
    Centre,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Distribution {
    #[serde(default)]
    pub mode: Mode,
    pub value: f64,
    /// With corners, a count for each side in turn (Number), repeating.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counts: Vec<u32>,
    #[serde(default)]
    pub justify: Justify,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Corners {
    /// Spaced along the whole path.
    #[default]
    Through,
    /// A copy on every corner, the rest spaced along each side.
    EveryCorner,
    /// Spaced along each side, corners left empty.
    Skip,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    /// The copy's x along the path.
    #[default]
    Follow,
    /// Every copy turned the same way, corners included: the source's own rotation plus the
    /// rotation offset ("same" in scripts too).
    #[serde(alias = "same")]
    Fixed,
    /// The copy's x away from the path's inside (the right of travel on an open path).
    Outward,
    Inward,
}

// Written by hand so that `same`, the alias of `fixed`, is accepted too.
impl JsonSchema for Orientation {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Orientation".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "enum": ["follow", "fixed", "same", "outward", "inward"],
            "description": "How each copy is turned: `follow` the path; `fixed` or `same`, every \
                copy (corners included) the same way, by the rotation offset; or `outward` or \
                `inward`, square to the path (a corner copy along the corner's bisector)."
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Height {
    /// On the array's level, plus its offset.
    #[default]
    Level,
    /// On the host's top (host edges only), plus the offset.
    HostTop,
}

/// Random variation per copy, seeded so it stays the same.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Variation {
    /// A random extra turn between these (radians).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f64; 2]>,
    /// A random uniform scale between these.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f64; 2]>,
    /// A random offset across the path up to this (metres, either side).
    #[serde(default)]
    pub jitter: f64,
    #[serde(default)]
    pub seed: u64,
}

/// A source component and its weight in a random choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    /// The custom object type (GlobalId).
    pub r#type: String,
    #[serde(default = "one")]
    pub weight: f64,
}

fn one() -> f64 {
    1.0
}

fn yes() -> bool {
    true
}

/// The stored recipe of a path array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathArrayRecipe {
    pub kind: String,
    pub sources: Vec<Source>,
    pub path: PathSpec,
    /// The level (GlobalId).
    pub level: String,
    #[serde(default)]
    pub offset: f64,
    pub distribution: Distribution,
    #[serde(default = "yes")]
    pub include_start: bool,
    #[serde(default = "yes")]
    pub include_end: bool,
    #[serde(default)]
    pub start_inset: f64,
    #[serde(default)]
    pub end_inset: f64,
    #[serde(default)]
    pub corners: Corners,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default)]
    pub rotation_offset: f64,
    #[serde(default)]
    pub variation: Variation,
    #[serde(default)]
    pub height: Height,
    /// Excluded members, by index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<usize>,
}

pub fn path_array_recipe(e: &Element) -> Option<PathArrayRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// A path as points in plan, with which points are corners, and the host's top for host edges.
#[derive(Debug, Clone, PartialEq)]
pub struct PathGeometry {
    pub points: Vec<[f64; 2]>,
    pub corner: Vec<bool>,
    pub closed: bool,
    pub host_top: Option<f64>,
}

fn smooth(points: Vec<[f64; 2]>) -> PathGeometry {
    let n = points.len();
    PathGeometry {
        corner: (0..n).map(|i| i == 0 || i + 1 == n).collect(),
        points,
        closed: false,
        host_top: None,
    }
}

/// The path as it is now.
pub fn path_geometry(store: &Store, spec: &PathSpec) -> Result<PathGeometry> {
    let bad = |m: &str| Error::new("array.bad_path", m.to_owned());
    let g = match spec {
        PathSpec::Drawn {
            points,
            shape,
            closed,
        } => match shape {
            Shape::Polyline => {
                if points.len() < 2 {
                    return Err(bad("a path needs two points or more"));
                }
                PathGeometry {
                    corner: vec![true; points.len()],
                    points: points.clone(),
                    closed: *closed && points.len() >= 3,
                    host_top: None,
                }
            }
            Shape::Arc => {
                let [a, m, b] = points.as_slice() else {
                    return Err(bad("an arc is three points: start, a point on it, end"));
                };
                let p = crate::walls::curved::path(Line { start: *a, end: *b }, &[*m])
                    .ok_or_else(|| bad("the arc's points are in line"))?;
                smooth(p.points)
            }
            Shape::Spline => {
                if points.len() < 2 {
                    return Err(bad("a spline needs two points or more"));
                }
                let line = Line {
                    start: points[0],
                    end: *points.last().expect("two"),
                };
                let inner = &points[1..points.len() - 1];
                match crate::walls::curved::path(line, inner) {
                    Some(p) => smooth(p.points),
                    None => smooth(points.clone()),
                }
            }
        },
        PathSpec::Element { element } => {
            let e = find(store, element)
                .and_then(|id| store.get(id))
                .ok_or_else(|| bad("the path's element no longer exists"))?;
            if let Some(r) = crate::walls::wall_recipe(e) {
                match crate::walls::curved::path_of(&r) {
                    Some(p) => smooth(p.points),
                    None => smooth(vec![r.curve.start, r.curve.end]),
                }
            } else if let Some(g) = crate::grids::grid_data(e) {
                smooth(vec![g.curve.start, g.curve.end])
            } else {
                return Err(bad("a path element is a wall or a grid line"));
            }
        }
        PathSpec::HostEdges { element, r#loop } => {
            let e = find(store, element)
                .and_then(|id| store.get(id))
                .ok_or_else(|| bad("the path's host no longer exists"))?;
            let sketch = crate::sketch::sketch_of(e)
                .ok_or_else(|| bad("the host has no boundary to follow"))?;
            let rings = crate::sketch::rings(store, &sketch);
            let (points, keys) = rings
                .get(*r#loop)
                .cloned()
                .ok_or_else(|| bad("the host has no such boundary loop"))?;
            // A corner where a curve starts; points along a curved edge are not corners.
            let n = keys.len();
            let corner = (0..n).map(|i| keys[i] != keys[(i + n - 1) % n]).collect();
            let host_top = crate::floors::regenerate(store, e).ok().map(|g| g.top);
            PathGeometry {
                points,
                corner,
                closed: true,
                host_top,
            }
        }
    };
    Ok(g)
}

/// One generated copy.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Member {
    pub index: usize,
    pub point: [f64; 2],
    pub z: f64,
    /// Counter-clockwise from x (radians).
    pub rotation: f64,
    /// Its extra scale (variation), times the type's own.
    pub scale: f64,
    /// Its component type (GlobalId).
    pub r#type: String,
}

/// Distances along a run of length `len` for one distribution.
fn spaced(
    len: f64,
    mode: Mode,
    value: f64,
    justify: Justify,
    closed: bool,
    ends: (bool, bool),
) -> Result<Vec<f64>> {
    let eps = 1e-9;
    let bad = |m: String| Error::new("array.bad_distribution", m);
    if len <= eps {
        return Ok(if ends.0 || ends.1 {
            vec![0.0]
        } else {
            Vec::new()
        });
    }
    let even = |spaces: usize| -> Vec<f64> {
        let d = len / spaces as f64;
        let mut v: Vec<f64> = (0..=spaces).map(|i| i as f64 * d).collect();
        if closed {
            v.pop();
        }
        v
    };
    let mut out = match mode {
        Mode::Number => {
            let n = value.round();
            if n.is_nan() || n < 1.0 {
                return Err(bad("the number of copies is at least 1".into()));
            }
            let n = n as usize;
            if closed {
                return Ok((0..n).map(|i| i as f64 * len / n as f64).collect());
            }
            // The count includes the ends that are included.
            let inner = n as isize - ends.0 as isize - ends.1 as isize;
            if inner < 0 {
                return Ok(if ends.0 { vec![0.0] } else { vec![len] });
            }
            let spaces = inner as usize + 1;
            let d = len / spaces as f64;
            let mut v = Vec::new();
            if ends.0 {
                v.push(0.0);
            }
            v.extend((1..spaces).map(|i| i as f64 * d));
            if ends.1 {
                v.push(len);
            }
            return Ok(v);
        }
        Mode::Fixed => {
            if value.is_nan() || value <= eps {
                return Err(bad("the distance must be above zero".into()));
            }
            let k = (len / value + eps).floor() as usize;
            let rem = len - k as f64 * value;
            let start = match justify {
                Justify::Start => 0.0,
                Justify::End => rem,
                Justify::Centre => rem / 2.0,
            };
            let count = if closed { k.max(1) } else { k + 1 };
            (0..count)
                .map(|i| start + i as f64 * value)
                .collect::<Vec<_>>()
        }
        Mode::Max => {
            if value.is_nan() || value <= eps {
                return Err(bad("the spacing must be above zero".into()));
            }
            even(((len / value) - eps).ceil().max(1.0) as usize)
        }
        Mode::Min => {
            if value.is_nan() || value <= eps {
                return Err(bad("the spacing must be above zero".into()));
            }
            even(((len / value) + eps).floor().max(1.0) as usize)
        }
    };
    if !closed {
        out.retain(|d| (ends.0 || *d > eps) && (ends.1 || *d < len - eps));
    }
    Ok(out)
}

/// Lengths along a polyline.
fn cumulative(points: &[[f64; 2]]) -> Vec<f64> {
    let mut along = vec![0.0];
    for w in points.windows(2) {
        along.push(along.last().expect("one") + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
    }
    along
}

/// The point and unit tangent at distance `s` along a polyline (at a vertex, the bisector).
fn at(points: &[[f64; 2]], along: &[f64], s: f64, closed: bool) -> ([f64; 2], [f64; 2]) {
    let n = points.len() - 1;
    let i = along[1..]
        .iter()
        .position(|a| *a >= s - 1e-9)
        .unwrap_or(n - 1)
        .min(n - 1);
    let dir = |k: usize| {
        let (a, b) = (points[k], points[k + 1]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-12);
        [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
    };
    let seg = (along[i + 1] - along[i]).max(1e-12);
    let t = ((s - along[i]) / seg).clamp(0.0, 1.0);
    let p = [
        points[i][0] + (points[i + 1][0] - points[i][0]) * t,
        points[i][1] + (points[i + 1][1] - points[i][1]) * t,
    ];
    let mut d = dir(i);
    // At a vertex between two segments, the bisector of their directions; on a closed path the
    // start (and end) is such a vertex too, between the last segment and the first.
    if t > 1.0 - 1e-9 && (i + 1 < n || closed) {
        let e = dir(if i + 1 < n { i + 1 } else { 0 });
        d = [d[0] + e[0], d[1] + e[1]];
    } else if t < 1e-9 && (i > 0 || closed) {
        let e = dir(if i > 0 { i - 1 } else { n - 1 });
        d = [d[0] + e[0], d[1] + e[1]];
    }
    let l = d[0].hypot(d[1]).max(1e-12);
    (p, [d[0] / l, d[1] / l])
}

/// SplitMix64: a small, stable random number per (seed, index, stream).
fn random(seed: u64, index: usize, stream: u64) -> f64 {
    let mut z = seed
        .wrapping_add((index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(stream.wrapping_mul(0xD1B5_4A32_D192_ED03));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// The array's copies as it is now (excluded ones left out).
pub fn members(store: &Store, r: &PathArrayRecipe) -> Result<Vec<Member>> {
    if r.sources.is_empty() {
        return Err(Error::new("array.no_source", "the array has no component"));
    }
    let g = path_geometry(store, &r.path)?;
    let mut pts = g.points.clone();
    if g.closed {
        pts.push(pts[0]);
    }
    let along = cumulative(&pts);
    let total = *along.last().expect("points");
    // Distances along the whole path.
    let mut dist: Vec<f64> = Vec::new();
    let value_for = |side: usize| -> f64 {
        if r.distribution.mode == Mode::Number && !r.distribution.counts.is_empty() {
            r.distribution.counts[side % r.distribution.counts.len()] as f64
        } else {
            r.distribution.value
        }
    };
    let (s0, s1) = if g.closed {
        (0.0, total)
    } else {
        (r.start_inset.max(0.0), total - r.end_inset.max(0.0))
    };
    if s1 < s0 {
        return Err(Error::new(
            "array.bad_path",
            "the insets are longer than the path",
        ));
    }
    match r.corners {
        Corners::Through => {
            let ends = (r.include_start, r.include_end);
            for d in spaced(
                s1 - s0,
                r.distribution.mode,
                value_for(0),
                r.distribution.justify,
                g.closed,
                ends,
            )? {
                dist.push(s0 + d);
            }
        }
        Corners::EveryCorner | Corners::Skip => {
            // Runs between corners.
            let mut marks: Vec<f64> = (0..g.points.len())
                .filter(|i| g.corner[*i])
                .map(|i| along[i])
                .filter(|a| *a >= s0 - 1e-9 && *a <= s1 + 1e-9)
                .collect();
            if g.closed {
                if marks.is_empty() {
                    marks.push(0.0);
                }
                marks.push(marks[0] + total);
            } else {
                if marks.first().is_none_or(|m| *m > s0 + 1e-9) {
                    marks.insert(0, s0);
                }
                if marks.last().is_none_or(|m| *m < s1 - 1e-9) {
                    marks.push(s1);
                }
            }
            let runs = marks.len() - 1;
            for k in 0..runs {
                let (a, b) = (marks[k], marks[k + 1]);
                let first = k == 0;
                let last = k + 1 == runs;
                let corner_mode = r.corners == Corners::EveryCorner;
                // Each corner is placed once: by the run it starts.
                let start_in = if !g.closed && first {
                    r.include_start
                } else {
                    corner_mode
                };
                let end_in = if !g.closed && last {
                    r.include_end
                } else {
                    false
                };
                // For counting, a run's count includes both its corners in Every Corner mode.
                let ends_for_count = (
                    corner_mode || (!g.closed && first && r.include_start),
                    corner_mode || (!g.closed && last && r.include_end),
                );
                let mut ds = spaced(
                    b - a,
                    r.distribution.mode,
                    value_for(k),
                    r.distribution.justify,
                    false,
                    ends_for_count,
                )?;
                if ends_for_count.0 && !start_in {
                    ds.retain(|d| *d > 1e-9);
                }
                if ends_for_count.1 && !end_in {
                    ds.retain(|d| *d < b - a - 1e-9);
                }
                dist.extend(ds.into_iter().map(|d| (a + d) % (total + 1e-12)));
            }
        }
    }
    if dist.len() > MAX_MEMBERS {
        return Err(Error::new(
            "array.too_many",
            format!(
                "the array would have {} copies; at most {MAX_MEMBERS}",
                dist.len()
            ),
        ));
    }
    // A closed path's inside is on the left when it runs counter-clockwise.
    let ccw = !g.closed || crate::sketch::signed_area(&g.points) >= 0.0;
    let z = match (r.height, g.host_top) {
        (Height::HostTop, Some(top)) => top + r.offset,
        _ => crate::levels::elevation_of(store, &r.level)? + r.offset,
    };
    let total_weight: f64 = r.sources.iter().map(|s| s.weight.max(0.0)).sum();
    let mut out = Vec::with_capacity(dist.len());
    for (index, d) in dist.into_iter().enumerate() {
        if r.excluded.contains(&index) {
            continue;
        }
        let (mut p, t) = at(&pts, &along, d, g.closed);
        let right = [t[1], -t[0]];
        let outward = if ccw { right } else { [-right[0], -right[1]] };
        let base = match r.orientation {
            Orientation::Follow => t[1].atan2(t[0]),
            Orientation::Fixed => 0.0,
            Orientation::Outward => outward[1].atan2(outward[0]),
            Orientation::Inward => (-outward[1]).atan2(-outward[0]),
        };
        let v = &r.variation;
        let mut rotation = base + r.rotation_offset;
        if let Some([lo, hi]) = v.rotation {
            rotation += lo + (hi - lo) * random(v.seed, index, 1);
        }
        let scale = v
            .scale
            .map_or(1.0, |[lo, hi]| lo + (hi - lo) * random(v.seed, index, 2));
        if v.jitter > 0.0 {
            let j = (random(v.seed, index, 3) * 2.0 - 1.0) * v.jitter;
            p = [p[0] + right[0] * j, p[1] + right[1] * j];
        }
        let source = if r.sources.len() == 1 || total_weight <= 0.0 {
            &r.sources[0]
        } else {
            let mut pick = random(v.seed, index, 4) * total_weight;
            r.sources
                .iter()
                .find(|s| {
                    pick -= s.weight.max(0.0);
                    pick < 0.0
                })
                .unwrap_or(&r.sources[0])
        };
        out.push(Member {
            index,
            point: p,
            z,
            rotation,
            scale,
            r#type: source.r#type.clone(),
        });
    }
    Ok(out)
}

/// A member's mesh, placed.
pub fn member_geometry(store: &Store, m: &Member) -> Result<MeshObjectGeometry> {
    let (type_id, ty, mesh) = mesh_objects::type_geometry(store, &m.r#type)?;
    Ok(MeshObjectGeometry {
        type_id,
        placement: Placement {
            origin: [m.point[0], m.point[1], m.z],
            rotation: m.rotation,
            mirror: false,
            scale: ty.scale * m.scale,
        },
        ty,
        mesh,
    })
}

/// Every path array that regenerates, with its members' geometry.
pub fn all(store: &Store) -> Vec<(ElementId, Vec<(Member, MeshObjectGeometry)>)> {
    store
        .iter()
        .filter_map(|e| {
            let r = path_array_recipe(e)?;
            let ms = members(store, &r).ok()?;
            let gs = ms
                .into_iter()
                .filter_map(|m| {
                    let g = member_geometry(store, &m).ok()?;
                    Some((m, g))
                })
                .collect();
            Some((e.id(), gs))
        })
        .collect()
}

/// Whether the array uses `guid` (a source type, its level, or its path element).
pub fn uses(r: &PathArrayRecipe, guid: &str) -> bool {
    r.level == guid
        || r.sources.iter().any(|s| s.r#type == guid)
        || match &r.path {
            PathSpec::Element { element } | PathSpec::HostEdges { element, .. } => element == guid,
            PathSpec::Drawn { .. } => false,
        }
}

// ---- Commands ----

/// A source to repeat: a custom object type, or a library item (loaded as a type).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceArg {
    pub type_id: Option<ElementRef>,
    pub library: Option<String>,
    pub item: Option<String>,
    #[serde(default = "one")]
    pub weight: f64,
}

/// A path to follow.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PathArg {
    /// Drawn points (with `shape` and `closed`) …
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub shape: Shape,
    #[serde(default)]
    pub closed: bool,
    /// … or a wall or grid line to follow …
    pub element: Option<ElementRef>,
    /// … or a floor, ceiling or deck whose edges to follow (`loop`: which boundary).
    pub host_edges: Option<ElementRef>,
    #[serde(default)]
    pub r#loop: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlongPathParams {
    /// The components to repeat; with several, each copy is a weighted random choice.
    pub sources: Vec<SourceArg>,
    pub path: PathArg,
    pub level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    pub distribution: Distribution,
    #[serde(default = "yes")]
    pub include_start: bool,
    #[serde(default = "yes")]
    pub include_end: bool,
    #[serde(default)]
    pub start_inset: f64,
    #[serde(default)]
    pub end_inset: f64,
    #[serde(default)]
    pub corners: Corners,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation_offset: f64,
    #[serde(default)]
    pub variation: Variation,
    /// Default: on the host's top for host edges, else on the level.
    pub height: Option<Height>,
    pub name: Option<String>,
}

fn guid_of(store: &Store, r: &ElementRef, what: &str) -> Result<String> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", format!("no such {what}")))?;
    Ok(store.get(id).expect("resolved").guid().to_string())
}

fn resolve_sources(
    txn: &mut crate::txn::Transaction<'_>,
    sources: &[SourceArg],
) -> Result<Vec<Source>> {
    if sources.is_empty() {
        return Err(Error::new("array.no_source", "give a component to repeat"));
    }
    sources
        .iter()
        .map(|s| {
            let id = match (&s.type_id, &s.library, &s.item) {
                (Some(t), None, None) => txn
                    .store()
                    .resolve(t)
                    .ok_or_else(|| Error::new("element.not_found", "no such type"))?,
                (None, Some(l), Some(i)) => crate::library::load_item(txn, l, i)?,
                _ => {
                    return Err(Error::new(
                        "array.bad_source",
                        "a source is a type_id, or a library and an item",
                    ));
                }
            };
            let e = txn.store().get(id).expect("resolved");
            if mesh_objects::mesh_object_type(e).is_none() {
                return Err(Error::new(
                    "array.bad_source",
                    format!("element {id} is not a component type (an imported custom object)"),
                ));
            }
            Ok(Source {
                r#type: e.guid().to_string(),
                weight: s.weight,
            })
        })
        .collect()
}

fn path_spec(store: &Store, p: &PathArg) -> Result<PathSpec> {
    match (&p.element, &p.host_edges, p.points.is_empty()) {
        (Some(e), None, true) => Ok(PathSpec::Element {
            element: guid_of(store, e, "path element")?,
        }),
        (None, Some(h), true) => Ok(PathSpec::HostEdges {
            element: guid_of(store, h, "host")?,
            r#loop: p.r#loop,
        }),
        (None, None, false) => Ok(PathSpec::Drawn {
            points: p.points.clone(),
            shape: p.shape,
            closed: p.closed,
        }),
        _ => Err(Error::new(
            "array.bad_path",
            "give the path as points, an element, or host_edges (one of them)",
        )),
    }
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, r: &PathArrayRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    let store = txn.store().clone();
    members(&store, r)?;
    Ok(())
}

/// Array along Path (`array.along_path`).
pub(crate) fn along_path(
    txn: &mut crate::txn::Transaction<'_>,
    p: AlongPathParams,
) -> Result<ElementId> {
    let sources = resolve_sources(txn, &p.sources)?;
    let store = txn.store().clone();
    let path = path_spec(&store, &p.path)?;
    let level = guid_of(&store, &p.level, "level")?;
    let height = p.height.unwrap_or(match path {
        PathSpec::HostEdges { .. } => Height::HostTop,
        _ => Height::Level,
    });
    let r = PathArrayRecipe {
        kind: RECIPE.into(),
        sources,
        path,
        level,
        offset: p.offset,
        distribution: p.distribution,
        include_start: p.include_start,
        include_end: p.include_end,
        start_inset: p.start_inset,
        end_inset: p.end_inset,
        corners: p.corners,
        orientation: p.orientation,
        rotation_offset: p.rotation_offset,
        variation: p.variation,
        height,
        excluded: Vec::new(),
    };
    members(&store, &r)?;
    let name = p.name.unwrap_or_else(|| "Path Array".into());
    txn.create(
        NewElement::new(CLASS)
            .name(name)
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", "PathArray")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, PathArrayRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such array"))?;
    let rec = path_array_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "array.not_path_array",
            format!("element {id} is not a path array"),
        )
    })?;
    Ok((id, rec))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub array_id: ElementRef,
    pub sources: Option<Vec<SourceArg>>,
    pub path: Option<PathArg>,
    pub level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: Option<f64>,
    pub distribution: Option<Distribution>,
    pub include_start: Option<bool>,
    pub include_end: Option<bool>,
    pub start_inset: Option<f64>,
    pub end_inset: Option<f64>,
    pub corners: Option<Corners>,
    pub orientation: Option<Orientation>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation_offset: Option<f64>,
    pub variation: Option<Variation>,
    pub height: Option<Height>,
    /// Pick a new random arrangement (a new seed).
    #[serde(default)]
    pub reseed: bool,
}

/// Changes a path array (`array.set`); omitted values are kept.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.array_id)?;
    crate::modify::check_unpinned(&store, id)?;
    if let Some(s) = &p.sources {
        r.sources = resolve_sources(txn, s)?;
    }
    let store = txn.store().clone();
    if let Some(path) = &p.path {
        r.path = path_spec(&store, path)?;
    }
    if let Some(l) = &p.level {
        r.level = guid_of(&store, l, "level")?;
    }
    macro_rules! take {
        ($($f:ident),*) => { $( if let Some(v) = p.$f { r.$f = v; } )* };
    }
    take!(
        offset,
        include_start,
        include_end,
        start_inset,
        end_inset,
        corners,
        orientation,
        rotation_offset,
        height
    );
    if let Some(d) = p.distribution {
        r.distribution = d;
    }
    if let Some(v) = p.variation {
        r.variation = v;
    }
    if p.reseed {
        r.variation.seed = r.variation.seed.wrapping_add(0x9E37_79B9_7F4A_7C15) ^ 0x5DEE_CE66;
    }
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemberParams {
    pub array_id: ElementRef,
    /// The member's index, or `"all"` (restore only).
    pub index: Value,
}

/// Excludes a member, leaving a gap (`array.exclude_member`).
pub(crate) fn exclude_member(txn: &mut crate::txn::Transaction<'_>, p: MemberParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.array_id)?;
    let i = p
        .index
        .as_u64()
        .ok_or_else(|| Error::new("array.bad_index", "give the member's index"))?
        as usize;
    if !r.excluded.contains(&i) {
        r.excluded.push(i);
        r.excluded.sort_unstable();
    }
    write(txn, id, &r)
}

/// Restores an excluded member, or all (`array.restore_member`).
pub(crate) fn restore_member(txn: &mut crate::txn::Transaction<'_>, p: MemberParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.array_id)?;
    if p.index.as_str() == Some("all") {
        r.excluded.clear();
    } else {
        let i =
            p.index.as_u64().ok_or_else(|| {
                Error::new("array.bad_index", "give the member's index or \"all\"")
            })? as usize;
        r.excluded.retain(|x| *x != i);
    }
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArrayParams {
    pub array_id: ElementRef,
}

/// The members of a path array as it is now (`array.members`).
pub(crate) fn list_members(store: &Store, p: ArrayParams) -> Result<Vec<Member>> {
    let (_, r) = resolve(store, &p.array_id)?;
    members(store, &r)
}

/// Unarray: the members become independent custom objects, and the array goes
/// (`array.unarray`).
pub(crate) fn unarray(
    txn: &mut crate::txn::Transaction<'_>,
    p: ArrayParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let (id, r) = resolve(&store, &p.array_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let level_z = crate::levels::elevation_of(&store, &r.level)?;
    let mut out = Vec::new();
    for m in members(&store, &r)? {
        let (type_id, _, _) = mesh_objects::type_geometry(&store, &m.r#type)?;
        out.push(mesh_objects::place(
            txn,
            mesh_objects::PlaceParams {
                type_id: ElementRef::Id(type_id),
                point: m.point,
                level: ElementRef::Guid(GlobalId::parse(&r.level).expect("a guid")),
                offset: m.z - level_z,
                rotation: m.rotation,
                mirror: false,
                scale: m.scale,
            },
        )?);
    }
    txn.delete(id)?;
    Ok(out)
}

/// Moves, turns or mirrors a path array with a drawn path (Move, Copy, Rotate, Mirror). An array
/// on a picked path follows its element instead.
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    turn: f64,
    mirror: Option<f64>,
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = path_array_recipe(store.get(id).expect("exists")).expect("a path array");
    match &mut r.path {
        PathSpec::Drawn { points, .. } => {
            for p in points.iter_mut() {
                *p = x(*p);
            }
            if mirror.is_some() {
                // The path now runs the other way round; reversing keeps "outward" outward.
                points.reverse();
                std::mem::swap(&mut r.include_start, &mut r.include_end);
                std::mem::swap(&mut r.start_inset, &mut r.end_inset);
            }
        }
        _ => {
            return Err(Error::new(
                "array.follows_path",
                "this array follows its path element: move the element, or redraw the path",
            ));
        }
    }
    if mirror.is_none() && r.orientation == Orientation::Fixed {
        r.rotation_offset += turn;
    }
    r.offset += dz;
    write(txn, id, &r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_modes() {
        let d = |m, v, closed, ends| spaced(10.0, m, v, Justify::Start, closed, ends).unwrap();
        assert_eq!(
            d(Mode::Number, 3.0, false, (true, true)),
            vec![0.0, 5.0, 10.0]
        );
        assert_eq!(
            d(Mode::Number, 2.0, false, (false, false)),
            vec![10.0 / 3.0, 20.0 / 3.0]
        );
        assert_eq!(
            d(Mode::Number, 4.0, true, (true, true)),
            vec![0.0, 2.5, 5.0, 7.5]
        );
        assert_eq!(
            d(Mode::Fixed, 3.0, false, (true, true)),
            vec![0.0, 3.0, 6.0, 9.0]
        );
        let c = spaced(10.0, Mode::Fixed, 3.0, Justify::Centre, false, (true, true)).unwrap();
        assert!((c[0] - 0.5).abs() < 1e-12);
        assert_eq!(d(Mode::Max, 3.0, false, (true, true)).len(), 5); // 4 spaces of 2.5
        assert_eq!(d(Mode::Min, 3.0, false, (true, true)).len(), 4); // 3 spaces of 3.33
    }

    #[test]
    fn random_is_stable_and_spread() {
        assert_eq!(random(7, 3, 1), random(7, 3, 1));
        assert_ne!(random(7, 3, 1), random(8, 3, 1));
        let v: Vec<f64> = (0..1000).map(|i| random(1, i, 1)).collect();
        assert!(v.iter().all(|x| (0.0..1.0).contains(x)));
        let mean = v.iter().sum::<f64>() / 1000.0;
        assert!((mean - 0.5).abs() < 0.05);
    }
}
