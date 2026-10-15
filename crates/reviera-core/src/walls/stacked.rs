// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Stacked walls (spec Ch 3 §3.3.11, ARC-STKW-001): one wall whose type stacks basic wall types
//! vertically.
//!
//! The type's `recipe.stacked_wall` lists rows from top to bottom, each a basic wall type with a
//! fixed height (or the one *Variable* row), an offset from the type's offset reference (positive
//! toward the exterior) and a flip. The stacked wall itself is an ordinary Reviera wall (it is
//! drawn, moved, joined and hosts doors like any other); its type's width is the envelope of the
//! rows. Its body is one sub-wall per row: the row's band across the wall, cut at the wall's
//! joined ends, between the row's heights. Sub-walls have GlobalIds derived from the wall's, so
//! they stay the same from one save to the next and Break Up keeps them.
//!
//! Not built yet: selecting sub-walls with Tab and their own parameters, joins per height
//! interval (each row is cut at the stacked wall's own ends), top and base extensions, curved
//! stacked walls.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bodies::Mesh;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::joins::WallBody;
use crate::walls::{
    RECIPE_KEY, WALL_TYPE_RECIPE, WallGeometry, WallRecipe, type_width, wall_recipe,
};

/// Where the rows' offsets are measured from, and so each sub-wall's location line: the same
/// choices as a wall's location line.
pub use crate::walls::LocationLine as OffsetReference;

/// One row of a stacked wall type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Row {
    /// The basic wall type (GlobalId).
    pub wall_type: String,
    /// The row's height (metres); `None` for the Variable row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    /// From the offset reference, positive toward the exterior (metres).
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub flip: bool,
}

/// A stacked wall type's structure (`recipe.stacked_wall`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Stacked {
    #[serde(default)]
    pub offset_reference: OffsetReference,
    /// Rows from top to bottom.
    pub rows: Vec<Row>,
}

pub const KEY: &str = "stacked_wall";

/// A stacked wall type's rows.
pub fn of_type(t: &Element) -> Option<Stacked> {
    let v = t.param(RECIPE_KEY)?;
    if v.get("kind")?.as_str()? != WALL_TYPE_RECIPE {
        return None;
    }
    serde_json::from_value(v.get(KEY)?.clone()).ok()
}

fn type_element<'a>(store: &'a Store, e: &Element) -> Option<&'a Element> {
    let g = GlobalId::parse(e.param("@type")?.as_str()?)?;
    store.get(store.find_by_guid(&g)?)
}

/// A wall's stack, if its type is a stacked wall type.
pub fn of_wall(store: &Store, e: &Element) -> Option<Stacked> {
    wall_recipe(e)?;
    of_type(type_element(store, e)?)
}

/// One sub-wall of a stacked wall as laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct SubWall {
    /// The row (0 is the top row).
    pub row: usize,
    pub guid: GlobalId,
    pub wall_type: ElementId,
    /// Bottom and top elevations.
    pub z: [f64; 2],
    /// The sub-wall's centre across the wall, along the wall's normal (metres).
    pub centre: f64,
    pub width: f64,
    /// Whether the sub-wall is flipped relative to the drawing direction.
    pub flip: bool,
}

/// A sub-wall's GlobalId, derived from its stacked wall's.
pub fn sub_guid(parent: &GlobalId, row: usize) -> GlobalId {
    // SplitMix64 of the row, spread over 128 bits, mixed into the parent's UUID.
    let mut x = (row as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    let mix = (u128::from(x) << 64) | u128::from(x.rotate_left(17) ^ 0x5354_4B57);
    GlobalId::from_uuid(uuid::Uuid::from_u128(parent.to_uuid().as_u128() ^ mix))
}

/// Each row's centre across the wall toward its exterior, and width, for a stack.
fn bands(store: &Store, s: &Stacked) -> Result<Vec<(ElementId, f64, f64)>> {
    s.rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let id = GlobalId::parse(&row.wall_type)
                .and_then(|g| store.find_by_guid(&g))
                .ok_or_else(|| {
                    Error::new(
                        "stacked.no_type",
                        format!("row {}: its wall type is missing", i + 1),
                    )
                })?;
            let t = store.get(id).expect("found");
            let w = type_width(t).ok_or_else(|| {
                Error::new(
                    "stacked.no_type",
                    format!("row {}: not a Reviera wall type", i + 1),
                )
            })?;
            // A flipped row has its exterior inside, so its reference is mirrored.
            let mut at = crate::walls::line_offset(t, s.offset_reference);
            if row.flip {
                at = -at;
            }
            Ok((id, row.offset - at, w))
        })
        .collect()
}

/// The width of a stack's envelope about the stacked wall's location line.
fn envelope(store: &Store, s: &Stacked) -> Result<f64> {
    Ok(bands(store, s)?
        .iter()
        .map(|(_, c, w)| (c.abs() + w / 2.0) * 2.0)
        .fold(0.0, f64::max))
}

/// Lays out a stacked wall's sub-walls for its geometry.
pub fn layout(store: &Store, e: &Element, g: &WallGeometry) -> Result<Vec<SubWall>> {
    let s = of_wall(store, e)
        .ok_or_else(|| Error::new("stacked.not_stacked", "the wall is not a stacked wall"))?;
    let recipe = wall_recipe(e).expect("a wall");
    let fixed: f64 = s.rows.iter().filter_map(|r| r.height).sum();
    let variable = g.height - fixed;
    if variable <= 1e-9 {
        return Err(Error::new(
            "stacked.too_short",
            "Stacked wall too short for its fixed rows",
        ));
    }
    let ext = if recipe.flip { -1.0 } else { 1.0 };
    let bands = bands(store, &s)?;
    // Bottom row first.
    let mut z = g.base_z;
    let mut out = Vec::new();
    for (i, row) in s.rows.iter().enumerate().rev() {
        let h = row.height.unwrap_or(variable);
        let (wall_type, centre, width) = bands[i];
        out.push(SubWall {
            row: i,
            guid: sub_guid(e.guid(), i),
            wall_type,
            z: [z, z + h],
            centre: centre * ext,
            width,
            flip: recipe.flip ^ row.flip,
        });
        z += h;
    }
    out.reverse();
    Ok(out)
}

/// A sub-wall's body: its band across the stacked wall's joined outline, between its heights.
pub fn sub_body(body: &WallBody, s: &SubWall) -> WallBody {
    band(
        body,
        s.centre - s.width / 2.0,
        s.centre + s.width / 2.0,
        s.z[0],
        s.z[1],
    )
}

/// A band of a wall's joined body: across from `lo` to `hi` along its normal, from `z0` to
/// `z1`, its ends on the body's end lines.
pub fn band(body: &WallBody, lo: f64, hi: f64, z0: f64, z1: f64) -> WallBody {
    let g = &body.geometry;
    let f = body.local_footprint();
    let (u, n) = (g.direction(), g.normal());
    // The x of an end line (through two corners) at a given y.
    let at = |a: [f64; 2], b: [f64; 2], y: f64| {
        if (b[1] - a[1]).abs() < 1e-12 {
            a[0]
        } else {
            a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        }
    };
    // Corners 0 and 1 are on one long side, 2 and 3 on the other.
    let (y01, y23) = if f[0][1] <= f[3][1] {
        (lo, hi)
    } else {
        (hi, lo)
    };
    let local = [
        [at(f[0], f[3], y01), y01],
        [at(f[1], f[2], y01), y01],
        [at(f[1], f[2], y23), y23],
        [at(f[0], f[3], y23), y23],
    ];
    let world = |p: [f64; 2]| {
        [
            g.start[0] + u[0] * p[0] + n[0] * p[1],
            g.start[1] + u[1] * p[0] + n[1] * p[1],
        ]
    };
    WallBody {
        geometry: WallGeometry {
            base_z: z0,
            height: z1 - z0,
            width: hi - lo,
            ..*g
        },
        footprint: local.map(world),
    }
}

/// A stacked wall's sub-wall meshes, cut by its openings.
pub fn meshes(
    store: &Store,
    e: &Element,
    body: &WallBody,
    cuts: &[crate::walls::openings::Cut],
) -> Vec<(SubWall, Mesh)> {
    let Ok(subs) = layout(store, e, &body.geometry) else {
        return Vec::new();
    };
    subs.into_iter()
        .map(|s| {
            let b = sub_body(body, &s);
            let (z0, z1) = (s.z[0], s.z[1]);
            let clipped: Vec<_> = cuts
                .iter()
                .filter(|c| c.z[1] > z0 + 1e-6 && c.z[0] < z1 - 1e-6)
                .map(|c| crate::walls::openings::Cut {
                    x: c.x,
                    z: [c.z[0].max(z0), c.z[1].min(z1)],
                    outline: c.outline.clone(),
                })
                .collect();
            let mesh = crate::walls::openings::cut_mesh(&b, &clipped);
            (s, mesh)
        })
        .collect()
}

// ---- Commands ----

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RowArg {
    pub wall_type_id: ElementRef,
    /// The row's height (metres); omitted for the Variable row.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    #[serde(default)]
    pub flip: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRowsParams {
    /// A wall type; it becomes a stacked wall type.
    pub type_id: ElementRef,
    #[serde(default)]
    pub offset_reference: OffsetReference,
    /// Rows from top to bottom; exactly one has no height (Variable).
    pub rows: Vec<RowArg>,
}

fn build(store: &Store, own: Option<ElementId>, p: &SetRowsParams) -> Result<Stacked> {
    if p.rows.is_empty() {
        return Err(Error::new("stacked.no_rows", "a stacked wall needs rows"));
    }
    if p.rows.iter().filter(|r| r.height.is_none()).count() != 1 {
        return Err(Error::new(
            "stacked.variable",
            "exactly one row must be Variable (no height)",
        ));
    }
    let mut rows = Vec::new();
    for (i, a) in p.rows.iter().enumerate() {
        let id = store.resolve(&a.wall_type_id).ok_or_else(|| {
            Error::new(
                "element.not_found",
                format!("row {}: no such wall type", i + 1),
            )
        })?;
        let t = store.get(id).expect("resolved");
        if type_width(t).is_none() || of_type(t).is_some() || Some(id) == own {
            return Err(Error::new(
                "stacked.not_basic",
                format!("row {}: a row's type must be a basic wall type", i + 1),
            ));
        }
        if let Some(h) = a.height
            && (h.is_nan() || h <= 0.0)
        {
            return Err(Error::new(
                "stacked.height",
                format!("row {}: the height must be greater than 0", i + 1),
            ));
        }
        rows.push(Row {
            wall_type: t.guid().to_string(),
            height: a.height,
            offset: a.offset,
            flip: a.flip,
        });
    }
    Ok(Stacked {
        offset_reference: p.offset_reference,
        rows,
    })
}

/// Every wall of a type must still lay out.
fn check_walls(store: &Store, type_guid: &str) -> Result<()> {
    for w in store.iter() {
        if wall_recipe(w).is_some() && w.param("@type").and_then(Value::as_str) == Some(type_guid) {
            let g = crate::walls::regenerate(store, w)?;
            layout(store, w, &g)?;
        }
    }
    Ok(())
}

/// `stackedwall.set_rows`.
pub(crate) fn set_rows(txn: &mut Transaction<'_>, p: SetRowsParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.type_id)
        .filter(|id| store.get(*id).and_then(type_width).is_some())
        .ok_or_else(|| Error::new("element.not_found", "no such wall type"))?;
    if store.iter().any(|e| {
        of_type(e).is_some_and(|s| {
            s.rows
                .iter()
                .any(|r| r.wall_type == store.get(id).expect("exists").guid().as_str())
        })
    }) {
        return Err(Error::new(
            "stacked.in_use",
            "the type is a row of a stacked wall type",
        ));
    }
    let s = build(&store, Some(id), &p)?;
    let width = envelope(&store, &s)?;
    let mut recipe = store
        .get(id)
        .and_then(|t| t.param(RECIPE_KEY))
        .cloned()
        .expect("a wall type");
    recipe["width"] = width.into();
    recipe[KEY] = serde_json::to_value(&s).expect("serialises");
    if let Some(o) = recipe.as_object_mut() {
        o.remove("compound");
    }
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), recipe);
        e.params
            .insert("@PredefinedType".into(), Value::from("ELEMENTEDWALL"));
    })?;
    let store = txn.store().clone();
    check_walls(&store, store.get(id).expect("exists").guid().as_str())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTypeParams {
    pub name: String,
    #[serde(default)]
    pub offset_reference: OffsetReference,
    /// Rows from top to bottom; exactly one has no height (Variable).
    pub rows: Vec<RowArg>,
}

/// `stackedwall.create_type`: a new stacked wall type.
pub(crate) fn create_type(txn: &mut Transaction<'_>, p: CreateTypeParams) -> Result<ElementId> {
    let id = txn.create(
        NewElement::new("IfcWallType")
            .name(p.name.trim())
            .param("@PredefinedType", "ELEMENTEDWALL")
            .param(
                RECIPE_KEY,
                serde_json::json!({ "kind": WALL_TYPE_RECIPE, "width": 0.2 }),
            ),
    )?;
    set_rows(
        txn,
        SetRowsParams {
            type_id: ElementRef::Id(id),
            offset_reference: p.offset_reference,
            rows: p.rows,
        },
    )?;
    Ok(id)
}

/// Keeps stacked wall types' widths in step with their rows' types (after a layer edit).
pub(crate) fn refresh_widths(txn: &mut Transaction<'_>) -> Result<()> {
    let store = txn.store().clone();
    for t in store.iter() {
        let Some(s) = of_type(t) else { continue };
        let width = envelope(&store, &s)?;
        if type_width(t).is_some_and(|w| (w - width).abs() > 1e-12) {
            txn.modify(t.id(), |e| {
                if let Some(r) = e.params.get_mut(RECIPE_KEY) {
                    r["width"] = width.into();
                }
            })?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BreakUpParams {
    pub stacked_wall_ids: Vec<ElementRef>,
}

/// `wall.break_up`: each stacked wall becomes its sub-walls as basic walls, keeping their
/// GlobalIds; doors and windows go to the sub-wall their centre is in.
pub(crate) fn break_up(txn: &mut Transaction<'_>, p: BreakUpParams) -> Result<Vec<ElementId>> {
    let mut out = Vec::new();
    for r in &p.stacked_wall_ids {
        let store = txn.store().clone();
        let id = store
            .resolve(r)
            .filter(|id| store.get(*id).and_then(|e| of_wall(&store, e)).is_some())
            .ok_or_else(|| Error::new("element.not_found", format!("no stacked wall {r:?}")))?;
        crate::modify::check_unpinned(&store, id)?;
        let e = store.get(id).expect("resolved");
        let recipe: WallRecipe = wall_recipe(e).expect("a wall");
        let g = crate::walls::regenerate(&store, e)?;
        let subs = layout(&store, e, &g)?;
        let level_z = g.base_z - recipe.base_offset;
        let n = g.normal();
        let mut made = Vec::new();
        for s in &subs {
            let t = store.get(s.wall_type).expect("laid out");
            let shift = |p: [f64; 2]| [p[0] + n[0] * s.centre, p[1] + n[1] * s.centre];
            let mut sub = recipe.clone();
            // Each sub-wall on its own centreline.
            sub.curve.start = shift(g.start);
            sub.curve.end = shift(g.end);
            sub.location_line = crate::walls::LocationLine::WallCenterline;
            sub.base_offset = s.z[0] - level_z;
            sub.flip = s.flip;
            sub.top_offset = 0.0;
            sub.attach_top = None;
            // The top row keeps the wall's top constraint; the others are unconnected.
            if s.row == 0 {
                sub.top_offset = recipe.top_offset;
                sub.attach_top = recipe.attach_top.clone();
            } else {
                sub.top = serde_json::json!({ "unconnected": s.z[1] - s.z[0] });
            }
            if s.row == 0 && recipe.top.get("unconnected").is_some() {
                sub.top = serde_json::json!({ "unconnected": s.z[1] - s.z[0] });
            }
            let new = txn.create(
                NewElement::new("IfcWall")
                    .guid(s.guid.clone())
                    .param("@PredefinedType", "STANDARD")
                    .param("@type", t.guid().as_str())
                    .param(
                        RECIPE_KEY,
                        serde_json::to_value(&sub).expect("recipes serialise"),
                    ),
            )?;
            made.push((s.clone(), new));
        }
        // Doors and windows: to the sub-wall their centre is in.
        for i in crate::inserts::hosted_by(&store, e.guid().as_str()) {
            let mut ir =
                crate::inserts::insert_recipe(store.get(i).expect("hosted")).expect("insert");
            let placed = crate::inserts::resolve(&store, store.get(i).expect("hosted"))?;
            let mid = placed.z0 + placed.ty.height / 2.0;
            let Some((_, new)) = made
                .iter()
                .find(|(s, _)| mid >= s.z[0] - 1e-9 && mid <= s.z[1] + 1e-9)
                .or(made.last())
            else {
                continue;
            };
            // Sills are measured from the base level, which the sub-walls keep.
            ir.host = txn.store().get(*new).expect("made").guid().to_string();
            let value = serde_json::to_value(&ir).expect("serialises");
            txn.modify(i, |el| {
                el.params.insert(RECIPE_KEY.into(), value);
            })?;
        }
        txn.delete(id)?;
        out.extend(made.into_iter().map(|(_, id)| id));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_guids_are_distinct_and_stable() {
        let p = GlobalId::new();
        let a = sub_guid(&p, 0);
        assert_eq!(a, sub_guid(&p, 0));
        assert_ne!(a, sub_guid(&p, 1));
        assert_ne!(a, p);
    }
}
