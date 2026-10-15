// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Slab edges (spec Ch 3 §3.6.2, ARC-SLED-001): a profile swept along a floor's edges, such as
//! an edge beam, a thickened edge or a curb.
//!
//! A slab edge is an `IfcSlab` (`USERDEFINED`, `ObjectType = "SLABEDGE"`) with a recipe
//! (`recipe.slab_edge`): its floor (GlobalId), the floor's footprint curves it runs along (by
//! key; consecutive curves form one mitred run, a whole loop a closed run), and its profile: a
//! rectangle `width` × `depth`, placed at the edge's top outer corner with its x away from the
//! slab and down from the top, moved by the horizontal and vertical offsets and flipped on
//! either axis. It follows its floor.
//!
//! Not built yet: profile families (other shapes), Angle, Restart (separate runs in one
//! element), toposolid and model-line edges, cutting the host where they overlap.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const RECIPE: &str = "recipe.slab_edge";
pub const OBJECT_TYPE: &str = "SLABEDGE";

/// The stored recipe of a slab edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SlabEdgeRecipe {
    pub kind: String,
    /// The floor (GlobalId).
    pub host: String,
    /// The footprint curves it runs along, by key.
    pub keys: Vec<u32>,
    pub width: f64,
    pub depth: f64,
    #[serde(default)]
    pub horizontal_offset: f64,
    #[serde(default)]
    pub vertical_offset: f64,
    /// Mirrored across the edge (into the slab) and upside down (up from the top).
    #[serde(default)]
    pub flip_horizontal: bool,
    #[serde(default)]
    pub flip_vertical: bool,
}

pub fn recipe(e: &Element) -> Option<SlabEdgeRecipe> {
    let r: SlabEdgeRecipe = serde_json::from_value(e.param(RECIPE_KEY)?.clone()).ok()?;
    (r.kind == RECIPE).then_some(r)
}

/// A slab edge as it is now.
#[derive(Debug, Clone)]
pub struct SlabEdgeGeometry {
    pub mesh: Mesh,
    /// The swept path's length along the profile's centroid.
    pub length: f64,
    pub volume: f64,
}

fn host<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    store.get(store.find_by_guid(&GlobalId::parse(guid)?)?)
}

/// The run: path corners along the floor's outer edge (counter-clockwise), and whether it is
/// closed.
fn run(store: &Store, r: &SlabEdgeRecipe) -> Result<(Vec<[f64; 2]>, bool, f64)> {
    let floor = host(store, &r.host)
        .ok_or_else(|| Error::new("slabedge.no_host", "the slab edge's floor no longer exists"))?;
    let g = crate::floors::regenerate(store, floor)?;
    let sketch = crate::sketch::sketch_of(floor)
        .ok_or_else(|| Error::new("slabedge.no_host", "the floor has no boundary"))?;
    let (ring, keys) = crate::sketch::rings(store, &sketch)
        .into_iter()
        .max_by(|a, b| {
            crate::sketch::signed_area(&a.0)
                .abs()
                .total_cmp(&crate::sketch::signed_area(&b.0).abs())
        })
        .ok_or_else(|| Error::new("slabedge.no_host", "the floor has no boundary"))?;
    // Counter-clockwise, corner i starting curve keys[i].
    let (ring, keys) = if crate::sketch::signed_area(&ring) < 0.0 {
        let n = ring.len();
        (
            (0..n).map(|j| ring[(n - j) % n]).collect::<Vec<_>>(),
            (0..n)
                .map(|j| keys[(2 * n - j - 1) % n])
                .collect::<Vec<_>>(),
        )
    } else {
        (ring, keys)
    };
    let n = ring.len();
    let picked: Vec<usize> = (0..n).filter(|i| r.keys.contains(&keys[*i])).collect();
    if picked.len() != r.keys.len() || picked.is_empty() {
        return Err(Error::new(
            "slabedge.edges",
            "the slab edge's curves are not all edges of its floor's outline",
        ));
    }
    if picked.len() == n {
        return Ok((ring, true, g.top));
    }
    // One consecutive chain: find where it starts (a picked edge whose previous is not picked).
    let start = picked
        .iter()
        .copied()
        .find(|i| !picked.contains(&((i + n - 1) % n)))
        .expect("an open chain has a start");
    let mut path = vec![ring[start]];
    let mut i = start;
    for _ in 0..picked.len() {
        if !picked.contains(&i) {
            return Err(Error::new(
                "slabedge.not_chained",
                "a slab edge's curves must follow one another (one run)",
            ));
        }
        i = (i + 1) % n;
        path.push(ring[i]);
    }
    Ok((path, false, g.top))
}

/// The swept body: the profile at every path corner, mitred, joined segment to segment.
pub fn regenerate(store: &Store, e: &Element) -> Result<SlabEdgeGeometry> {
    let r = recipe(e).ok_or_else(|| Error::new("slabedge.not", "not a slab edge"))?;
    if r.width <= 0.0 || r.depth <= 0.0 {
        return Err(Error::new(
            "slabedge.profile",
            "the profile needs a width and a depth",
        ));
    }
    let (path, closed, top) = run(store, &r)?;
    let m = path.len();
    // The profile in (outward, up) coordinates, counter-clockwise.
    let (sx, sz) = (
        if r.flip_horizontal { -1.0 } else { 1.0 },
        if r.flip_vertical { 1.0 } else { -1.0 },
    );
    let (h, v) = (r.horizontal_offset, r.vertical_offset);
    let mut profile = vec![
        [h, v],
        [h + sx * r.width, v],
        [h + sx * r.width, v + sz * r.depth],
        [h, v + sz * r.depth],
    ];
    let area_sign = {
        let a: f64 = (0..4)
            .map(|k| {
                let (p, q) = (profile[k], profile[(k + 1) % 4]);
                p[0] * q[1] - q[0] * p[1]
            })
            .sum();
        a.signum()
    };
    if area_sign < 0.0 {
        profile.reverse();
    }
    let centroid_out = profile.iter().map(|p| p[0]).sum::<f64>() / 4.0;
    // Each corner's outward miter: the outward normals of its segments, meeting.
    let seg_out = |k: usize| {
        let (a, b) = (path[k], path[(k + 1) % m]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-12);
        [(b[1] - a[1]) / l, -(b[0] - a[0]) / l]
    };
    let segments = if closed { m } else { m - 1 };
    let miter = |k: usize| -> [f64; 2] {
        let (prev, next) = match (closed, k) {
            (false, 0) => (seg_out(0), seg_out(0)),
            (false, k) if k == m - 1 => (seg_out(m - 2), seg_out(m - 2)),
            (_, k) => (seg_out((k + m - 1) % m), seg_out(k % m)),
        };
        let s = [prev[0] + next[0], prev[1] + next[1]];
        let sl = s[0].hypot(s[1]);
        if sl < 1e-9 {
            return next;
        }
        let u = [s[0] / sl, s[1] / sl];
        let cos = (u[0] * next[0] + u[1] * next[1]).max(0.2);
        [u[0] / cos, u[1] / cos]
    };
    let mut positions = Vec::with_capacity(m * 4);
    for (k, corner) in path.iter().enumerate() {
        let mv = miter(k);
        for p in &profile {
            positions.push([
                corner[0] + mv[0] * p[0],
                corner[1] + mv[1] * p[0],
                top + p[1],
            ]);
        }
    }
    let at = |k: usize, j: usize| ((k % m) * 4 + j) as u32;
    let mut indices: Vec<u32> = Vec::new();
    for k in 0..segments {
        for j in 0..4 {
            let j1 = (j + 1) % 4;
            let (a, b, c, d) = (at(k, j), at(k, j1), at(k + 1, j1), at(k + 1, j));
            indices.extend([a, c, b, a, d, c]);
        }
    }
    if !closed {
        indices.extend([at(0, 0), at(0, 1), at(0, 2), at(0, 0), at(0, 2), at(0, 3)]);
        let l = m - 1;
        indices.extend([at(l, 0), at(l, 2), at(l, 1), at(l, 0), at(l, 3), at(l, 2)]);
    }
    // The length along the profile's centroid, and the volume.
    let mut length = 0.0;
    for k in 0..segments {
        let (a, b) = (path[k], path[(k + 1) % m]);
        let (ma, mb) = (miter(k), miter(k + 1));
        let pa = [a[0] + ma[0] * centroid_out, a[1] + ma[1] * centroid_out];
        let pb = [b[0] + mb[0] * centroid_out, b[1] + mb[1] * centroid_out];
        length += (pb[0] - pa[0]).hypot(pb[1] - pa[1]);
    }
    let volume = mesh_volume(&positions, &indices).abs();
    Ok(SlabEdgeGeometry {
        mesh: (positions, indices),
        length,
        volume,
    })
}

fn mesh_volume(p: &[[f64; 3]], idx: &[u32]) -> f64 {
    idx.chunks(3)
        .map(|t| {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

/// Every slab edge that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, SlabEdgeGeometry)> {
    store
        .iter()
        .filter(|e| recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A Reviera floor.
    pub host_id: ElementRef,
    /// Its footprint curves to run along (consecutive); all of its outline if omitted.
    pub keys: Option<Vec<u32>>,
    /// The profile's width (away from the slab) and depth (down), metres; 0.3 × 0.45 by default.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub depth: Option<f64>,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub horizontal_offset: f64,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub vertical_offset: f64,
}

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &SlabEdgeRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    let store = txn.store().clone();
    regenerate(&store, store.get(id).expect("exists")).map(|_| ())
}

/// `slabedge.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let host_id = store
        .resolve(&p.host_id)
        .filter(|id| {
            store
                .get(*id)
                .and_then(crate::floors::floor_recipe)
                .is_some()
        })
        .ok_or_else(|| Error::new("slabedge.host", "a slab edge runs along a Reviera floor"))?;
    let floor = store.get(host_id).expect("resolved");
    let keys = match p.keys {
        Some(k) if !k.is_empty() => k,
        _ => crate::sketch::sketch_of(floor)
            .map(|s| {
                crate::sketch::rings(&store, &s)
                    .into_iter()
                    .max_by(|a, b| {
                        crate::sketch::signed_area(&a.0)
                            .abs()
                            .total_cmp(&crate::sketch::signed_area(&b.0).abs())
                    })
                    .map(|r| r.1)
                    .unwrap_or_default()
            })
            .unwrap_or_default(),
    };
    let r = SlabEdgeRecipe {
        kind: RECIPE.into(),
        host: floor.guid().to_string(),
        keys,
        width: p.width.unwrap_or(0.3),
        depth: p.depth.unwrap_or(0.45),
        horizontal_offset: p.horizontal_offset,
        vertical_offset: p.vertical_offset,
        flip_horizontal: false,
        flip_vertical: false,
    };
    let id = txn.create(
        NewElement::new("IfcSlab")
            .name("Slab Edge")
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", OBJECT_TYPE),
    )?;
    write(txn, id, &r)?;
    Ok(id)
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, SlabEdgeRecipe)> {
    store
        .resolve(r)
        .and_then(|id| Some((id, recipe(store.get(id)?)?)))
        .ok_or_else(|| Error::new("slabedge.not_found", format!("no slab edge {r:?}")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub slab_edge_id: ElementRef,
    pub keys: Option<Vec<u32>>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub depth: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub horizontal_offset: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub vertical_offset: Option<f64>,
}

/// `slabedge.set`.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.slab_edge_id)?;
    if let Some(k) = p.keys {
        r.keys = k;
    }
    if let Some(w) = p.width {
        r.width = w;
    }
    if let Some(d) = p.depth {
        r.depth = d;
    }
    if let Some(h) = p.horizontal_offset {
        r.horizontal_offset = h;
    }
    if let Some(v) = p.vertical_offset {
        r.vertical_offset = v;
    }
    write(txn, id, &r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SegmentsParams {
    pub id: ElementRef,
    /// Footprint curve keys of its floor.
    pub edge_refs: Vec<u32>,
}

/// `hostsweep.add_segments`.
pub(crate) fn add_segments(txn: &mut Transaction<'_>, p: SegmentsParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.id)?;
    for k in p.edge_refs {
        if !r.keys.contains(&k) {
            r.keys.push(k);
        }
    }
    write(txn, id, &r)
}

/// `hostsweep.remove_segments`.
pub(crate) fn remove_segments(txn: &mut Transaction<'_>, p: SegmentsParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.id)?;
    r.keys.retain(|k| !p.edge_refs.contains(k));
    if r.keys.is_empty() {
        return Err(Error::new(
            "slabedge.empty",
            "a slab edge keeps at least one segment (delete it instead)",
        ));
    }
    write(txn, id, &r)
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlipParams {
    pub id: ElementRef,
    pub axis: Axis,
}

/// `hostsweep.flip`.
pub(crate) fn flip(txn: &mut Transaction<'_>, p: FlipParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.id)?;
    match p.axis {
        Axis::Horizontal => r.flip_horizontal = !r.flip_horizontal,
        Axis::Vertical => r.flip_vertical = !r.flip_vertical,
    }
    write(txn, id, &r)
}

/// Whether a slab edge runs along the floor with GlobalId `guid`.
pub fn uses(r: &SlabEdgeRecipe, guid: &str) -> bool {
    r.host == guid
}
