// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Closets (ARC-INT-005, `INTERIORS_PLAN.md` §4.4): rods and a shelf along a closet's back wall,
//! at practice heights or within the 2010 ADA Standards' reach (§811, §308).
//!
//! Rods and shelves are `IfcSystemFurnitureElement` USERDEFINED (ClosetRod, ClosetShelf), in the
//! Casework category. An accessible layout's are AccessibleClosetRod and AccessibleClosetShelf,
//! which the interiors rule packs check with the surfaces rule kind: §308.3.2's 46 in for a side
//! reach over up to 24 in, and 15 in at the least (§308.3.1).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bath_components::Shape;
use crate::cabinets::{CARCASS, line_frame, unit_type};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mesh_objects::Category;
use crate::txn::Transaction;

/// The tag on a closet's parts: `{closet, part}`.
pub const CLOSET_KEY: &str = "rxd:closet";
const IN: f64 = 0.0254;
/// A rod's section, and a shelf's thickness and depth.
const ROD: f64 = 1.25 * IN;
const SHELF_T: f64 = 0.75 * IN;
const SHELF_D: f64 = 12.0 * IN;

/// How a closet is fitted (research/15 B11 and §8: S1489, S1404, S1354).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// One rod at 66 in, a shelf over it at 68 in.
    #[default]
    Single,
    /// Rods at 40 and 80 in (double hang), a shelf at 84 in.
    Double,
    /// A rod at 42 in and a shelf at 46 in, within a side reach over 24 in (ADA §308.3.2).
    Accessible,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClosetParams {
    /// The closet's back wall face: from here …
    pub start: [f64; 2],
    /// … to here.
    pub end: [f64; 2],
    pub level: ElementRef,
    #[serde(default)]
    pub layout: Layout,
    /// From the back wall to the closet's front (default 24 in); the rods are centred in it.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub depth: Option<f64>,
    /// The rods' tops above the floor, in place of the layout's.
    pub rod_heights: Option<Vec<f64>>,
    /// The shelf's top above the floor, in place of the layout's.
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub shelf_height: Option<f64>,
    /// A point in the room the closet opens to; default away from the wall behind it.
    pub toward: Option<[f64; 2]>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Closet {
    /// The closet's id (its parts' tag).
    pub closet: String,
    pub layout: Layout,
    pub length: f64,
    pub rods: Vec<ElementId>,
    pub shelves: Vec<ElementId>,
}

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// Fits out a closet (`closet.create`).
pub(crate) fn create(txn: &mut Transaction<'_>, p: ClosetParams) -> Result<Closet> {
    let store = txn.store().clone();
    let level = store.resolve(&p.level).and_then(|l| store.get(l)).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let z0 = crate::levels::elevation(level);
    let (length, d, front, rotation) = line_frame(&store, p.start, p.end, z0, p.toward)?;
    let depth = p.depth.unwrap_or(24.0 * IN);
    if depth < SHELF_D {
        return Err(bad("closet.too_shallow", format!("a closet {:.0} in deep cannot take a 12 in shelf", depth / IN)));
    }
    let (rods, shelf) = match p.layout {
        Layout::Single => (vec![66.0 * IN], 68.0 * IN),
        Layout::Double => (vec![40.0 * IN, 80.0 * IN], 84.0 * IN),
        Layout::Accessible => (vec![42.0 * IN], 46.0 * IN),
    };
    let rods = p.rod_heights.clone().unwrap_or(rods);
    let shelf = p.shelf_height.unwrap_or(shelf);
    if rods.iter().any(|h| *h + 1.0 * IN > shelf - SHELF_T + 1e-9) {
        return Err(bad("closet.no_hanger_room", "each rod needs 1 in under the shelf for hangers"));
    }
    let accessible = p.layout == Layout::Accessible;
    let closet = crate::id::GlobalId::new().as_str().to_owned();
    let at = |out: f64| [p.start[0] + d[0] * length / 2.0 + front[0] * out, p.start[1] + d[1] * length / 2.0 + front[1] * out];
    let mm = |v: f64| format!("{:.0}", v * 1000.0);
    let place = |txn: &mut Transaction<'_>, name: String, object_type: &str, sh: Shape, size: [f64; 3], out: f64, offset: f64, part: &str| -> Result<ElementId> {
        let ty = unit_type(txn, &name, Category::SystemFurniture, "USERDEFINED", object_type, &sh, size, CARCASS)?;
        let id = crate::mesh_objects::place(txn, crate::mesh_objects::PlaceParams { type_id: ElementRef::Id(ty), point: at(out), level: p.level.clone(), offset, rotation, mirror: false, scale: 1.0 })?;
        txn.modify(id, |e| {
            e.params.insert(CLOSET_KEY.into(), json!({ "closet": closet, "part": part }));
        })?;
        Ok(id)
    };
    let (rod_type, shelf_type) = if accessible { ("AccessibleClosetRod", "AccessibleClosetShelf") } else { ("ClosetRod", "ClosetShelf") };
    let named = if accessible { "Accessible closet" } else { "Closet" };
    let mut rod_ids = Vec::new();
    for h in &rods {
        let sh = Shape::default().cuboid([-length / 2.0, length / 2.0], [-ROD / 2.0, ROD / 2.0], [0.0, ROD]);
        rod_ids.push(place(txn, format!("{named} rod {}", mm(length)), rod_type, sh, [length, ROD, ROD], depth / 2.0, h - ROD, "rod")?);
    }
    let sh = Shape::default().cuboid([-length / 2.0, length / 2.0], [-SHELF_D / 2.0, SHELF_D / 2.0], [0.0, SHELF_T]);
    let shelf_id = place(txn, format!("{named} shelf {} × {}", mm(length), mm(SHELF_D)), shelf_type, sh, [length, SHELF_D, SHELF_T], SHELF_D / 2.0, shelf - SHELF_T, "shelf")?;
    Ok(Closet { closet, layout: p.layout, length, rods: rod_ids, shelves: vec![shelf_id] })
}
