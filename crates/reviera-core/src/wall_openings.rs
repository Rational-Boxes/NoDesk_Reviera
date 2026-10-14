// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Wall openings (ARC-WALL-012, ARC-OPEN-003): a plain rectangular hole through a wall, without
//! a door or window.
//!
//! A wall opening is an `IfcOpeningElement` with a `recipe.wall_opening`: its host wall
//! (GlobalId), the interval along the wall's location curve it spans (metres from the start, on
//! the curve for a curved wall) and its vertical extent between levels ([`Extent`]): a base level
//! and offset, and a top level and offset or a height. It cuts the wall through its whole
//! thickness, square to the location curve; the part above the wall's top or below its base cuts
//! nothing. It follows its levels (raising the top level makes it taller), moves along the wall,
//! moves and is copied with its wall, and goes when the wall goes.
//!
//! Reviera's display cuts the wall's body; the IFC keeps the wall's body whole and voids it with
//! the opening's box, as for doors and windows.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::levels::Extent;
use crate::store::Store;
use crate::walls::{self, RECIPE_KEY, TopConstraint, WallGeometry, openings::Cut};

pub const WALL_OPENING_RECIPE: &str = "recipe.wall_opening";
/// The height of a new opening whose top is not given.
pub const DEFAULT_HEIGHT: f64 = 2.1;
/// The narrowest and lowest opening.
const MIN_SIZE: f64 = 0.01;

/// The stored recipe of a wall opening.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WallOpeningRecipe {
    pub kind: String,
    /// The wall it cuts (GlobalId).
    pub host: String,
    /// From and to, along the wall's location curve from its start (metres).
    pub u: [f64; 2],
    #[serde(flatten)]
    pub extent: Extent,
}

pub fn wall_opening_recipe(e: &Element) -> Option<WallOpeningRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == WALL_OPENING_RECIPE)
        .then(|| serde_json::from_value(v.clone()).ok())?
}

/// A wall opening as it is now: its wall and the part of it that cuts the wall.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub id: ElementId,
    pub host: ElementId,
    pub wall: WallGeometry,
    /// The wall's curve when it is curved (`u` is measured on it).
    pub path: Option<walls::curved::Path>,
    pub recipe: WallOpeningRecipe,
    /// Its bottom and top elevations, as its levels give them.
    pub z: [f64; 2],
}

impl Resolved {
    /// The hole it makes in its wall (within the wall's height).
    pub fn cut(&self) -> Cut {
        let (b, t) = (self.wall.base_z, self.wall.base_z + self.wall.height);
        Cut::rect(self.recipe.u, [self.z[0].max(b), self.z[1].min(t)])
    }

    /// Its width along the wall.
    pub fn width(&self) -> f64 {
        self.recipe.u[1] - self.recipe.u[0]
    }

    /// The point (plan) and unit tangent at distance `s` along the wall.
    pub fn at(&self, s: f64) -> ([f64; 2], [f64; 2]) {
        match &self.path {
            Some(p) => p.at(s),
            None => {
                let (u, a) = (self.wall.direction(), self.wall.start);
                ([a[0] + u[0] * s, a[1] + u[1] * s], u)
            }
        }
    }

    /// Its plan outline: the wall's thickness over the opening's span (corners along the curve
    /// on a curved wall), counter-clockwise.
    pub fn outline(&self) -> Vec<[f64; 2]> {
        let half = self.wall.width / 2.0;
        let [u0, u1] = self.recipe.u;
        let steps = if self.path.is_some() {
            (((u1 - u0) / 0.1).ceil() as usize).clamp(1, 32)
        } else {
            1
        };
        let side = |k: f64| -> Vec<[f64; 2]> {
            (0..=steps)
                .map(|i| {
                    let s = u0 + (u1 - u0) * i as f64 / steps as f64;
                    let (p, t) = self.at(s);
                    [p[0] - t[1] * half * k, p[1] + t[0] * half * k]
                })
                .collect()
        };
        let mut out = side(-1.0);
        out.extend(side(1.0).into_iter().rev());
        out
    }

    /// The distance along the wall of the point nearest to `p`.
    pub fn along_of(&self, p: [f64; 2]) -> f64 {
        match &self.path {
            Some(path) => path.project(p),
            None => {
                let (u, s) = (self.wall.direction(), self.wall.start);
                (p[0] - s[0]) * u[0] + (p[1] - s[1]) * u[1]
            }
        }
    }

    /// The length of the wall's location curve.
    pub fn span(&self) -> f64 {
        self.path
            .as_ref()
            .map_or(self.wall.length(), walls::curved::Path::length)
    }
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// Resolves a wall opening against its wall and levels.
pub fn resolve(store: &Store, e: &Element) -> Result<Resolved> {
    let recipe = wall_opening_recipe(e).ok_or_else(|| {
        Error::new(
            "opening.not_wall",
            format!("element {} is not a wall opening", e.id()),
        )
    })?;
    let host = find(store, &recipe.host)
        .ok_or_else(|| Error::new("opening.no_host", "the opening's wall no longer exists"))?;
    let w = store.get(host).expect("found");
    let wall = walls::regenerate(store, w)?;
    let path = walls::wall_recipe(w).and_then(|r| walls::curved::path_of(&r));
    let (z0, z1) = recipe.extent.z(store)?;
    Ok(Resolved {
        id: e.id(),
        host,
        wall,
        path,
        recipe,
        z: [z0, z1],
    })
}

/// Every wall opening that resolves.
pub fn all(store: &Store) -> Vec<Resolved> {
    store
        .iter()
        .filter(|e| wall_opening_recipe(e).is_some())
        .filter_map(|e| resolve(store, e).ok())
        .collect()
}

/// The wall openings of each wall, with their cuts.
pub fn cuts_by_host(store: &Store) -> BTreeMap<ElementId, Vec<(ElementId, Cut)>> {
    let mut out: BTreeMap<ElementId, Vec<(ElementId, Cut)>> = BTreeMap::new();
    for r in all(store) {
        let c = r.cut();
        if c.z[1] > c.z[0] + 1e-9 {
            out.entry(r.host).or_default().push((r.id, c));
        }
    }
    out
}

/// The wall openings in wall `wall` (GlobalId).
pub fn of_host(store: &Store, wall: &str) -> Vec<ElementId> {
    store
        .iter()
        .filter(|e| wall_opening_recipe(e).is_some_and(|r| r.host == wall))
        .map(Element::id)
        .collect()
}

/// Whether two cuts overlap (more than touching).
pub(crate) fn overlap(a: &Cut, ax: [f64; 2], b: &Cut, bx: [f64; 2]) -> bool {
    bx[0] < ax[1] - 1e-9 && ax[0] < bx[1] - 1e-9 && b.z[0] < a.z[1] - 1e-9 && a.z[0] < b.z[1] - 1e-9
}

/// Checks that an opening fits its wall: within its clear span, reaching into its height, and
/// clear of its doors, windows and other openings.
fn check_fit(store: &Store, r: &Resolved) -> Result<()> {
    let [u0, u1] = r.recipe.u;
    if u1 - u0 < MIN_SIZE {
        return Err(Error::new(
            "opening.too_narrow",
            "the opening must be at least 10 mm wide",
        ));
    }
    let solved = walls::solve(store);
    let span = solved
        .bodies
        .get(&r.host)
        .map(walls::openings::clear_span)
        .unwrap_or([0.0, r.span()]);
    if u0 < span[0] - 1e-9 || u1 > span[1] + 1e-9 {
        return Err(Error::new(
            "opening.outside_host",
            "the opening does not fit along its wall",
        ));
    }
    let c = r.cut();
    if c.z[1] - c.z[0] < MIN_SIZE {
        return Err(Error::new(
            "opening.outside_host",
            "the opening does not reach into its wall's height",
        ));
    }
    for p in crate::inserts::placed(store) {
        if p.host == r.host && overlap(&c, r.recipe.u, &p.cut(), p.extent()) {
            return Err(Error::new(
                "opening.overlaps",
                format!("the opening overlaps {} {}", p.kind.noun(), p.id),
            ));
        }
    }
    for (other, oc) in cuts_by_host(store).remove(&r.host).unwrap_or_default() {
        if other != r.id && overlap(&c, r.recipe.u, &oc, oc.x) {
            return Err(Error::new(
                "opening.overlaps",
                format!("the opening overlaps opening {other}"),
            ));
        }
    }
    Ok(())
}

fn wall_of(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such wall"))?;
    if walls::wall_recipe(store.get(id).expect("resolved")).is_none() {
        return Err(Error::new(
            "opening.bad_host",
            format!("element {id} is not a Reviera wall"),
        ));
    }
    Ok(id)
}

fn write(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    r: &WallOpeningRecipe,
) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    let store = txn.store().clone();
    check_fit(&store, &resolve(&store, store.get(id).expect("exists"))?)
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A Reviera wall (straight or curved).
    pub wall_id: ElementRef,
    /// From and to, along the wall's location curve from its start (accept units).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub u0: f64,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub u1: f64,
    /// Omitted: the wall's base level.
    pub base_level: Option<ElementRef>,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: f64,
    /// A level, or a height above the base; omitted: 2.1 m high.
    pub top_constraint: Option<TopConstraint>,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: f64,
}

/// Cuts a rectangular opening through a wall (`opening.create_wall`).
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let host = wall_of(&store, &p.wall_id)?;
    let w = store.get(host).expect("resolved");
    let wr = walls::wall_recipe(w).expect("a wall");
    let base = match &p.base_level {
        Some(r) => r.clone(),
        None => ElementRef::Id(
            find(&store, &wr.base_level)
                .ok_or_else(|| Error::new("level.missing", "the wall's base level is gone"))?,
        ),
    };
    let extent = Extent::from_args(
        &store,
        &base,
        p.base_offset,
        p.top_constraint
            .unwrap_or(TopConstraint::Unconnected(DEFAULT_HEIGHT)),
        p.top_offset,
    )?;
    let recipe = WallOpeningRecipe {
        kind: WALL_OPENING_RECIPE.into(),
        host: w.guid().to_string(),
        u: [p.u0.min(p.u1), p.u0.max(p.u1)],
        extent,
    };
    let id = txn.create(
        NewElement::new("IfcOpeningElement")
            .name("Wall Opening")
            .param("@PredefinedType", "OPENING")
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    let store = txn.store().clone();
    check_fit(&store, &resolve(&store, store.get(id).expect("created"))?)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditParams {
    pub opening_id: ElementRef,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub u0: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub u1: Option<f64>,
    pub base_level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<f64>,
    pub top_constraint: Option<TopConstraint>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: Option<f64>,
}

fn resolve_opening(store: &Store, r: &ElementRef) -> Result<(ElementId, WallOpeningRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such opening"))?;
    let recipe = wall_opening_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "opening.not_wall",
            format!("element {id} is not a wall opening"),
        )
    })?;
    Ok((id, recipe))
}

/// Changes a wall opening's span or extent (`opening.edit_wall`); omitted values are kept.
pub(crate) fn edit(txn: &mut crate::txn::Transaction<'_>, p: EditParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve_opening(&store, &p.opening_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let u0 = p.u0.unwrap_or(r.u[0]);
    let u1 = p.u1.unwrap_or(r.u[1]);
    r.u = [u0.min(u1), u0.max(u1)];
    if p.base_level.is_some() || p.top_constraint.is_some() {
        let base = match &p.base_level {
            Some(b) => b.clone(),
            None => ElementRef::Id(
                find(&store, &r.extent.base_level)
                    .ok_or_else(|| Error::new("level.missing", "the base level is gone"))?,
            ),
        };
        let top = match p.top_constraint {
            Some(t) => t,
            None => match r.extent.top_level().and_then(|g| find(&store, g)) {
                Some(l) => TopConstraint::Level(walls::ElementRefValue::Id(l)),
                None => TopConstraint::Unconnected(
                    r.extent.top["unconnected"]
                        .as_f64()
                        .unwrap_or(DEFAULT_HEIGHT),
                ),
            },
        };
        r.extent = Extent::from_args(
            &store,
            &base,
            p.base_offset.unwrap_or(r.extent.base_offset),
            top,
            p.top_offset.unwrap_or(r.extent.top_offset),
        )?;
    } else {
        if let Some(b) = p.base_offset {
            r.extent.base_offset = b;
        }
        if let Some(t) = p.top_offset {
            r.extent.top_offset = t;
        }
        r.extent.z(&store)?;
    }
    write(txn, id, &r)
}

/// How far along its wall an opening slides for a move by `v` (its component along the wall).
fn shift_for(store: &Store, id: ElementId, v: [f64; 3]) -> Result<f64> {
    let res = resolve(store, store.get(id).expect("exists"))?;
    let mid = (res.recipe.u[0] + res.recipe.u[1]) / 2.0;
    let (p, _) = res.at(mid);
    Ok(res.along_of([p[0] + v[0], p[1] + v[1]]) - mid)
}

/// `r` slid along its wall by `shift` and raised by `dz`: a height above the base moves with
/// the base; a top level's offset moves too.
fn shifted(mut r: WallOpeningRecipe, shift: f64, dz: f64) -> WallOpeningRecipe {
    r.u = [r.u[0] + shift, r.u[1] + shift];
    r.extent.base_offset += dz;
    if r.extent.top_level().is_some() {
        r.extent.top_offset += dz;
    }
    r
}

/// Slides an opening along its wall by `v`'s component along the wall there, and up by its
/// height (`element.move`).
pub(crate) fn move_by(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    v: [f64; 3],
) -> Result<()> {
    let store = txn.store().clone();
    crate::modify::check_unpinned(&store, id)?;
    let shift = shift_for(&store, id, v)?;
    let r = shifted(
        wall_opening_recipe(store.get(id).expect("exists")).expect("checked"),
        shift,
        v[2],
    );
    write(txn, id, &r)
}

/// Copies opening `id` into wall `host` (GlobalId): at the same place along it (a copied wall's
/// openings), or moved by `v` along its own wall (a copied opening).
pub(crate) fn copy_to(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    host: &str,
    v: [f64; 3],
) -> Result<ElementId> {
    let store = txn.store().clone();
    let e = store.get(id).expect("checked by the caller");
    let shift = if v == [0.0; 3] {
        0.0
    } else {
        shift_for(&store, id, v)?
    };
    let mut r = shifted(wall_opening_recipe(e).expect("a wall opening"), shift, v[2]);
    r.host = host.to_owned();
    let new = txn.create(
        NewElement::new("IfcOpeningElement")
            .name(e.name.clone().unwrap_or_else(|| "Wall Opening".into()))
            .param("@PredefinedType", "OPENING")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    let store = txn.store().clone();
    check_fit(&store, &resolve(&store, store.get(new).expect("created"))?)?;
    Ok(new)
}
