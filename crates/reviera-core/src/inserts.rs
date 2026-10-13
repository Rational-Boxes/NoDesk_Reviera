// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Doors and windows hosted in walls (spec Ch 3 §3.4.2–3.4.3, ARC-DOOR-001, ARC-WIN-001;
//! hosting ARC-HOST-001; IFC DM-IFC-007).
//!
//! A door or window is an `IfcDoor` / `IfcWindow` with a `recipe.insert` (`rxd:recipe`): its host
//! wall (GlobalId), the distance of its centre from the wall's start, its sill height above the
//! host's base level, and its hand and facing. Its type (`IfcDoorType` / `IfcWindowType` with a
//! `recipe.door_type` / `recipe.window_type`) gives the width, height and frame. Each insert has
//! an `IfcOpeningElement` (a `recipe.opening` pointing at the insert) that goes with it; saving
//! writes the opening as voiding the wall and the insert as filling it.
//!
//! Geometry is regenerated: the insert's frame, panel and glass from its type and host, and the
//! host's body cut by the openings (`walls::openings`). Inserts move with their wall, and go
//! when it is deleted.
//!
//! Built so far: single-swing doors and fixed windows in Reviera walls, as built-in parametric
//! types. Loadable families, tags on placement, From/To Room and roof-hosted skylights come
//! later.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::levels::next_name;
use crate::store::Store;
use crate::walls::{self, RECIPE_KEY, WallGeometry, openings};

pub const INSERT_RECIPE: &str = "recipe.insert";
pub const OPENING_RECIPE: &str = "recipe.opening";
pub const DOOR_TYPE_RECIPE: &str = "recipe.door_type";
pub const WINDOW_TYPE_RECIPE: &str = "recipe.window_type";
pub const DEFAULT_DOOR_TYPE: &str = "Single Door 900 x 2100";
pub const DEFAULT_WINDOW_TYPE: &str = "Window 1200 x 1200";

/// Doors or windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Door,
    Window,
}

impl Kind {
    pub fn class(self) -> &'static str {
        match self {
            Kind::Door => "IfcDoor",
            Kind::Window => "IfcWindow",
        }
    }

    pub fn type_class(self) -> &'static str {
        match self {
            Kind::Door => "IfcDoorType",
            Kind::Window => "IfcWindowType",
        }
    }

    pub fn type_recipe(self) -> &'static str {
        match self {
            Kind::Door => DOOR_TYPE_RECIPE,
            Kind::Window => WINDOW_TYPE_RECIPE,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Kind::Door => "door",
            Kind::Window => "window",
        }
    }
}

/// The values of a door or window type (metres).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InsertType {
    pub kind: String,
    /// Overall (rough) width and height.
    pub width: f64,
    pub height: f64,
    /// Width of the frame members.
    pub frame_width: f64,
    /// Door panel thickness.
    #[serde(default)]
    pub panel_thickness: f64,
    /// Window frame depth (across the wall).
    #[serde(default)]
    pub frame_depth: f64,
    /// Sill height of new windows above the level.
    #[serde(default)]
    pub default_sill: f64,
}

impl InsertType {
    pub fn default_for(kind: Kind) -> Self {
        match kind {
            Kind::Door => Self {
                kind: DOOR_TYPE_RECIPE.into(),
                width: 0.9,
                height: 2.1,
                frame_width: 0.05,
                panel_thickness: 0.04,
                frame_depth: 0.0,
                default_sill: 0.0,
            },
            Kind::Window => Self {
                kind: WINDOW_TYPE_RECIPE.into(),
                width: 1.2,
                height: 1.2,
                frame_width: 0.06,
                panel_thickness: 0.0,
                frame_depth: 0.07,
                default_sill: 0.9,
            },
        }
    }

    pub fn kind(&self) -> Option<Kind> {
        match self.kind.as_str() {
            DOOR_TYPE_RECIPE => Some(Kind::Door),
            WINDOW_TYPE_RECIPE => Some(Kind::Window),
            _ => None,
        }
    }

    /// The parameters `type.set_param` accepts for this kind.
    pub fn params(&self) -> &'static [&'static str] {
        match self.kind() {
            Some(Kind::Door) => &["width", "height", "frame_width", "panel_thickness"],
            _ => &[
                "width",
                "height",
                "frame_width",
                "frame_depth",
                "default_sill",
            ],
        }
    }
}

/// The values of a door or window type element.
pub fn insert_type(e: &Element) -> Option<InsertType> {
    let v = e.param(RECIPE_KEY)?;
    let t: InsertType = serde_json::from_value(v.clone()).ok()?;
    t.kind().map(|_| t)
}

/// The stored recipe of a door or window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InsertRecipe {
    pub kind: String,
    /// The host wall (GlobalId).
    pub host: String,
    /// Distance of the insert's centre from the wall's start, along the wall (metres).
    pub along: f64,
    /// Bottom of the insert above the host's base level (metres).
    pub sill: f64,
    /// The hinge on the other side (doors).
    #[serde(default)]
    pub flip_hand: bool,
    /// Facing (the swing side for doors) toward the wall's interior instead of its exterior.
    #[serde(default)]
    pub flip_facing: bool,
}

pub fn insert_recipe(e: &Element) -> Option<InsertRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")? == INSERT_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn kind_of(e: &Element) -> Option<Kind> {
    insert_recipe(e)?;
    match e.class.as_str() {
        "IfcDoor" => Some(Kind::Door),
        "IfcWindow" => Some(Kind::Window),
        _ => None,
    }
}

/// The insert (GlobalId) an opening element belongs to.
pub fn opening_of(e: &Element) -> Option<String> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")? == OPENING_RECIPE).then(|| Some(v.get("insert")?.as_str()?.to_owned()))?
}

/// Triangle mesh: positions and index triples.
pub type Mesh = (Vec<[f64; 3]>, Vec<u32>);

/// A door or window resolved against its host and type.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub id: ElementId,
    pub kind: Kind,
    pub host: ElementId,
    pub wall: WallGeometry,
    pub ty: InsertType,
    pub recipe: InsertRecipe,
    /// Bottom elevation (absolute).
    pub z0: f64,
}

impl Placed {
    /// The opening through the host, in the wall's local frame.
    pub fn cut(&self) -> openings::Cut {
        let half = self.ty.width / 2.0;
        openings::Cut {
            x: [self.recipe.along - half, self.recipe.along + half],
            z: [self.z0, self.z0 + self.ty.height],
        }
    }

    /// Local frame point (x along the wall from the insert's centre, y toward the exterior, z
    /// from the bottom) to project coordinates.
    pub fn world(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        let (u, n, s) = (self.wall.direction(), self.wall.normal(), self.wall.start);
        let along = self.recipe.along + x;
        [
            s[0] + u[0] * along + n[0] * y,
            s[1] + u[1] * along + n[1] * y,
            self.z0 + z,
        ]
    }

    /// +1 when the insert faces the wall's exterior, -1 when flipped.
    pub fn facing(&self) -> f64 {
        if self.recipe.flip_facing { -1.0 } else { 1.0 }
    }

    /// The boxes of the insert in its local frame: (x, y, z) intervals, and whether the box is
    /// the door panel (drawn in 3D only; plans show the swing instead).
    pub fn boxes(&self) -> Vec<([[f64; 2]; 3], bool)> {
        let t = &self.ty;
        let (w, h, f) = (t.width / 2.0, t.height, t.frame_width);
        match self.kind {
            Kind::Door => {
                let half = self.wall.width / 2.0;
                let depth = [-half, half];
                let s = self.facing();
                let face = s * half;
                let panel_y = if s > 0.0 {
                    [face - t.panel_thickness, face]
                } else {
                    [face, face + t.panel_thickness]
                };
                vec![
                    ([[-w, -w + f], depth, [0.0, h]], false),
                    ([[w - f, w], depth, [0.0, h]], false),
                    ([[-w + f, w - f], depth, [h - f, h]], false),
                    ([[-w + f, w - f], panel_y, [0.0, h - f]], true),
                ]
            }
            Kind::Window => {
                let d = t.frame_depth.min(self.wall.width) / 2.0;
                let depth = [-d, d];
                vec![
                    ([[-w, -w + f], depth, [0.0, h]], false),
                    ([[w - f, w], depth, [0.0, h]], false),
                    ([[-w + f, w - f], depth, [0.0, f]], false),
                    ([[-w + f, w - f], depth, [h - f, h]], false),
                    ([[-w + f, w - f], [-0.005, 0.005], [f, h - f]], false),
                ]
            }
        }
    }

    /// The insert's meshes in project coordinates: the frame (and glass), and the door panel.
    pub fn meshes(&self) -> Vec<(Mesh, bool)> {
        let mut frame = (Vec::new(), Vec::new());
        let mut panel = (Vec::new(), Vec::new());
        for ([x, y, z], is_panel) in self.boxes() {
            let quad = [[x[0], y[0]], [x[1], y[0]], [x[1], y[1]], [x[0], y[1]]];
            let world = quad.map(|[a, b]| {
                let p = self.world([a, b, 0.0]);
                [p[0], p[1]]
            });
            let (v, i) = walls::prism(&world, self.z0 + z[0], self.z0 + z[1]);
            let target: &mut Mesh = if is_panel { &mut panel } else { &mut frame };
            let base = target.0.len() as u32;
            target.0.extend(v);
            target.1.extend(i.into_iter().map(|k| k + base));
        }
        let mut out = vec![(frame, false)];
        if !panel.0.is_empty() {
            out.push((panel, true));
        }
        out
    }

    /// The door swing drawn in plan: the open panel and its arc (segments in plan).
    pub fn swing(&self) -> Vec<[[f64; 2]; 2]> {
        if self.kind != Kind::Door {
            return Vec::new();
        }
        let t = &self.ty;
        let r = t.width - 2.0 * t.frame_width;
        let s = self.facing();
        let face = s * self.wall.width / 2.0;
        let side = if self.recipe.flip_hand { 1.0 } else { -1.0 };
        let hinge = [side * (t.width / 2.0 - t.frame_width), face];
        let open = [hinge[0], face + s * r];
        let p = |[x, y]: [f64; 2]| {
            let w = self.world([x, y, 0.0]);
            [w[0], w[1]]
        };
        let mut out = vec![[p(hinge), p(open)]];
        // From the open panel to the closed position, a quarter circle about the hinge.
        let steps = 16;
        let point = |k: usize| {
            let a = std::f64::consts::FRAC_PI_2 * k as f64 / steps as f64;
            [hinge[0] - side * r * a.sin(), face + s * r * a.cos()]
        };
        for k in 0..steps {
            out.push([p(point(k)), p(point(k + 1))]);
        }
        out
    }
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// Resolves a door or window against its host and type.
pub fn resolve(store: &Store, e: &Element) -> Result<Placed> {
    let kind = kind_of(e)
        .ok_or_else(|| Error::new("insert.not_insert", "not a Reviera door or window"))?;
    let recipe = insert_recipe(e).expect("checked");
    let host = find(store, &recipe.host)
        .ok_or_else(|| Error::new("insert.no_host", "the insert's host wall no longer exists"))?;
    let wall_e = store.get(host).expect("found");
    let wall = walls::regenerate(store, wall_e)?;
    let level = walls::wall_recipe(wall_e)
        .expect("a Reviera wall")
        .base_level;
    let level_z = find(store, &level)
        .and_then(|l| store.get(l))
        .map(crate::levels::elevation)
        .unwrap_or(wall.base_z);
    let ty = e
        .param("@type")
        .and_then(Value::as_str)
        .and_then(|g| find(store, g))
        .and_then(|t| store.get(t))
        .and_then(insert_type)
        .filter(|t| t.kind() == Some(kind))
        .unwrap_or_else(|| InsertType::default_for(kind));
    Ok(Placed {
        id: e.id(),
        kind,
        host,
        wall,
        ty,
        z0: level_z + recipe.sill,
        recipe,
    })
}

/// The doors and windows of a model that resolve.
pub fn placed(store: &Store) -> Vec<Placed> {
    store
        .iter()
        .filter(|e| kind_of(e).is_some())
        .filter_map(|e| resolve(store, e).ok())
        .collect()
}

/// The openings of each Reviera wall, for its cut body.
pub fn cuts_by_host(store: &Store) -> std::collections::BTreeMap<ElementId, Vec<openings::Cut>> {
    let mut out: std::collections::BTreeMap<ElementId, Vec<openings::Cut>> = Default::default();
    for p in placed(store) {
        out.entry(p.host).or_default().push(p.cut());
    }
    out
}

/// Checks that an insert fits its host: inside the wall's clear span and height, clear of the
/// other inserts in it.
pub(crate) fn check_fit(store: &Store, p: &Placed) -> Result<()> {
    let solved = walls::solve(store);
    let span = solved
        .bodies
        .get(&p.host)
        .map(openings::clear_span)
        .unwrap_or([0.0, p.wall.length()]);
    let c = p.cut();
    let (bottom, top) = (p.wall.base_z, p.wall.base_z + p.wall.height);
    if c.x[0] < span[0] - 1e-9 || c.x[1] > span[1] + 1e-9 {
        return Err(Error::new(
            "insert.outside_host",
            format!("the {} does not fit along its wall", p.kind.noun()),
        ));
    }
    if c.z[0] < bottom - 1e-9 || c.z[1] > top + 1e-9 {
        return Err(Error::new(
            "insert.outside_host",
            format!("the {} does not fit in the wall's height", p.kind.noun()),
        ));
    }
    for other in placed(store) {
        if other.id == p.id || other.host != p.host {
            continue;
        }
        let o = other.cut();
        if o.x[0] < c.x[1] - 1e-9
            && c.x[0] < o.x[1] - 1e-9
            && o.z[0] < c.z[1] - 1e-9
            && c.z[0] < o.z[1] - 1e-9
        {
            return Err(Error::new(
                "insert.overlaps",
                format!(
                    "the {} overlaps {} {}",
                    p.kind.noun(),
                    other.kind.noun(),
                    other.id
                ),
            ));
        }
    }
    Ok(())
}

/// The project's default type of `kind`, created when first needed.
fn default_type(txn: &mut crate::txn::Transaction<'_>, kind: Kind) -> Result<ElementId> {
    let name = match kind {
        Kind::Door => DEFAULT_DOOR_TYPE,
        Kind::Window => DEFAULT_WINDOW_TYPE,
    };
    let of_kind = |e: &&Element| insert_type(e).is_some_and(|t| t.kind() == Some(kind));
    if let Some(e) = txn
        .store()
        .iter()
        .filter(of_kind)
        .min_by_key(|e| e.name.as_deref() != Some(name))
    {
        return Ok(e.id());
    }
    let predefined = match kind {
        Kind::Door => "DOOR",
        Kind::Window => "WINDOW",
    };
    txn.create(
        NewElement::new(kind.type_class())
            .name(name)
            .param("@PredefinedType", predefined)
            .param(
                RECIPE_KEY,
                serde_json::to_value(InsertType::default_for(kind)).expect("serialises"),
            ),
    )
}

/// The next Mark of `kind`: the last placed one's increment (`1` → `2`), unused.
fn next_mark(store: &Store, kind: Kind) -> String {
    let marks: Vec<(ElementId, String)> = store
        .iter()
        .filter(|e| kind_of(e) == Some(kind))
        .filter_map(|e| Some((e.id(), e.param("@Tag")?.as_str()?.to_owned())))
        .collect();
    let mut mark = marks
        .iter()
        .max_by_key(|(id, _)| *id)
        .map_or("1".to_owned(), |(_, m)| next_name(m));
    while marks.iter().any(|(_, m)| *m == mark) {
        mark = next_name(&mark);
    }
    mark
}

/// Sets the length of a door or window type value (`type.set_param`): a number of metres or a
/// length with units.
pub(crate) fn set_type_param(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    param: &str,
    value: &Value,
) -> Result<()> {
    let e = txn.store().get(id).expect("resolved");
    let mut t = insert_type(e).expect("a door or window type");
    if !t.params().contains(&param) {
        return Err(Error::new(
            "type.unknown_param",
            format!(
                "this type has no parameter `{param}` ({})",
                t.params().join(", ")
            ),
        ));
    }
    let v = match value {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => {
            crate::units::parse(s, "length", &crate::units::settings_of(txn.store()))?
        }
        _ => f64::NAN,
    };
    let positive = param != "default_sill";
    if !v.is_finite() || (positive && v <= 0.0) || v < 0.0 {
        return Err(Error::new(
            "type.invalid_value",
            format!(
                "`{param}` must be a length greater than {}",
                if positive { "0" } else { "or equal to 0" }
            ),
        ));
    }
    match param {
        "width" => t.width = v,
        "height" => t.height = v,
        "frame_width" => t.frame_width = v,
        "panel_thickness" => t.panel_thickness = v,
        "frame_depth" => t.frame_depth = v,
        _ => t.default_sill = v,
    }
    if 2.0 * t.frame_width >= t.width || t.frame_width >= t.height {
        return Err(Error::new(
            "type.invalid_value",
            "the frame must be narrower than the opening",
        ));
    }
    let value = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    // Existing inserts must still fit (a wider door can run into its neighbour).
    check_all(txn.store(), |p| types_guid(txn.store(), p.id) == Some(id))
}

fn types_guid(store: &Store, insert: ElementId) -> Option<ElementId> {
    find(store, store.get(insert)?.param("@type")?.as_str()?)
}

/// Checks the fit of every insert matching `which`.
fn check_all(store: &Store, which: impl Fn(&Placed) -> bool) -> Result<()> {
    for p in placed(store).iter().filter(|p| which(p)) {
        check_fit(store, p)?;
    }
    Ok(())
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    /// A door type (window type for `window.place`); omitted, the project's default.
    pub type_id: Option<ElementRef>,
    /// The host wall (a Reviera wall).
    #[serde(alias = "host_id")]
    pub host_wall_id: ElementRef,
    /// Distance of the centre from the wall's start, along the wall (accepts units).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub point_along: Option<f64>,
    /// Or a plan point, projected onto the wall.
    pub point: Option<[f64; 2]>,
    /// Bottom above the host's base level; default 0 for doors, the type's default sill for
    /// windows.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub sill_height: Option<f64>,
    #[serde(default)]
    pub flip_hand: bool,
    #[serde(default)]
    pub flip_facing: bool,
}

pub(crate) fn place(
    txn: &mut crate::txn::Transaction<'_>,
    kind: Kind,
    p: PlaceParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let host = store
        .resolve(&p.host_wall_id)
        .ok_or_else(|| Error::new("element.not_found", "no such wall"))?;
    let host_e = store.get(host).expect("resolved");
    if walls::wall_recipe(host_e).is_none() {
        return Err(Error::new(
            "insert.bad_host",
            format!("{}s are placed in Reviera walls", kind.noun()),
        ));
    }
    let g = walls::regenerate(&store, host_e)?;
    let along = match (p.point_along, p.point) {
        (Some(a), _) => a,
        (None, Some(pt)) => {
            let u = g.direction();
            (pt[0] - g.start[0]) * u[0] + (pt[1] - g.start[1]) * u[1]
        }
        (None, None) => {
            return Err(Error::new(
                "insert.no_position",
                "give point_along or point",
            ));
        }
    };
    let type_id = match &p.type_id {
        Some(r) => {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such type"))?;
            if store.get(id).and_then(insert_type).and_then(|t| t.kind()) != Some(kind) {
                return Err(Error::new(
                    "type.wrong_family",
                    format!("element {id} is not a {} type", kind.noun()),
                ));
            }
            id
        }
        None => default_type(txn, kind)?,
    };
    let ty_e = txn.store().get(type_id).expect("exists").clone();
    let ty = insert_type(&ty_e).expect("an insert type");
    let sill = p.sill_height.unwrap_or(match kind {
        Kind::Door => 0.0,
        Kind::Window => ty.default_sill,
    });
    let recipe = InsertRecipe {
        kind: INSERT_RECIPE.into(),
        host: host_e.guid().to_string(),
        along,
        sill,
        flip_hand: p.flip_hand,
        flip_facing: p.flip_facing,
    };
    let mark = next_mark(txn.store(), kind);
    let predefined = match kind {
        Kind::Door => "DOOR",
        Kind::Window => "WINDOW",
    };
    let id = txn.create(
        NewElement::new(kind.class())
            .param("@PredefinedType", predefined)
            .param("@type", ty_e.guid().as_str())
            .param("@Tag", mark)
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    let guid = txn.store().get(id).expect("created").guid().to_string();
    txn.create(
        NewElement::new("IfcOpeningElement")
            .param("@PredefinedType", "OPENING")
            .param(
                RECIPE_KEY,
                json!({ "kind": OPENING_RECIPE, "insert": guid }),
            ),
    )?;
    let placed = resolve(txn.store(), txn.store().get(id).expect("created"))?;
    check_fit(txn.store(), &placed)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlipParams {
    pub element_ids: Vec<ElementRef>,
}

/// Flips the hand (`hand`) or the facing of doors and windows.
pub(crate) fn flip(txn: &mut crate::txn::Transaction<'_>, p: FlipParams, hand: bool) -> Result<()> {
    for r in &p.element_ids {
        let store = txn.store().clone();
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let mut recipe = insert_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
            Error::new(
                "insert.not_insert",
                format!("element {id} is not a door or window"),
            )
        })?;
        if hand {
            recipe.flip_hand = !recipe.flip_hand;
        } else {
            recipe.flip_facing = !recipe.flip_facing;
        }
        let value = serde_json::to_value(&recipe).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), value);
        })?;
    }
    Ok(())
}

/// Moves an insert along its host by the plan vector's component along the wall, and its sill
/// by the vertical component (`element.move`).
pub(crate) fn move_by(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    v: [f64; 3],
) -> Result<()> {
    let store = txn.store().clone();
    let e = store.get(id).expect("checked by the caller");
    let p = resolve(&store, e)?;
    let u = p.wall.direction();
    let mut recipe = p.recipe;
    recipe.along += v[0] * u[0] + v[1] * u[1];
    recipe.sill += v[2];
    let value = serde_json::to_value(&recipe).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    let moved = resolve(txn.store(), txn.store().get(id).expect("exists"))?;
    check_fit(txn.store(), &moved)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSillParams {
    pub element_ids: Vec<ElementRef>,
    /// Bottom above the host's base level (accepts units).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub sill_height: f64,
}

/// Sets the sill height of doors and windows (it moves them; it does not resize them).
pub(crate) fn set_sill(txn: &mut crate::txn::Transaction<'_>, p: SetSillParams) -> Result<()> {
    for r in &p.element_ids {
        let store = txn.store().clone();
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        let sill = resolve(&store, e)?.recipe.sill;
        move_by(txn, id, [0.0, 0.0, p.sill_height - sill])?;
    }
    Ok(())
}
