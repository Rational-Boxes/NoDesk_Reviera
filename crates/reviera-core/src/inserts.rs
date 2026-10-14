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

pub mod shape;
pub use shape::{HeadProfile, Shape};

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
    /// The outline (ARC-WIN-002); a rectangle unless set.
    #[serde(default, skip_serializing_if = "is_default")]
    pub shape: Shape,
    /// The arch of a round top.
    #[serde(default, skip_serializing_if = "is_default")]
    pub head_profile: HeadProfile,
    /// The head's rise above the spring line (segmental, elliptical and Tudor heads; gables).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rise: Option<f64>,
    /// A Gothic arch's radius as a multiple of its span (default 1: equilateral).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius_ratio: Option<f64>,
    /// A polygon's number of sides (default 8) and rotation (radians; default flat-topped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sides: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    /// A trapezoid's left and right heights (default the height).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_height: Option<f64>,
    /// Glazing bars: radiating in round tops, fans and round windows, upright in the rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muntins: Option<u32>,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
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
                ..Self::rectangle()
            },
            Kind::Window => Self {
                kind: WINDOW_TYPE_RECIPE.into(),
                width: 1.2,
                height: 1.2,
                frame_width: 0.06,
                panel_thickness: 0.0,
                frame_depth: 0.07,
                default_sill: 0.9,
                ..Self::rectangle()
            },
        }
    }

    /// The shape values of a plain rectangle (the rest zero).
    fn rectangle() -> Self {
        Self {
            kind: String::new(),
            width: 0.0,
            height: 0.0,
            frame_width: 0.0,
            panel_thickness: 0.0,
            frame_depth: 0.0,
            default_sill: 0.0,
            shape: Shape::Rectangle,
            head_profile: HeadProfile::Semicircular,
            rise: None,
            radius_ratio: None,
            sides: None,
            rotation: None,
            left_height: None,
            right_height: None,
            muntins: None,
        }
    }

    /// Whether the outline is not a rectangle.
    pub fn shaped(&self) -> bool {
        self.shape != Shape::Rectangle
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
            Some(Kind::Door) => &[
                "width",
                "height",
                "frame_width",
                "panel_thickness",
                "shape",
                "head_profile",
                "rise",
                "radius_ratio",
            ],
            _ => &[
                "width",
                "height",
                "frame_width",
                "frame_depth",
                "default_sill",
                "shape",
                "head_profile",
                "rise",
                "radius_ratio",
                "sides",
                "rotation",
                "left_height",
                "right_height",
                "muntins",
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
    /// This insert's own width and height, overriding its type's (set when it was fitted to
    /// its wall, or resized).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
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
    /// The host's curve, when it is curved (ARC-WALL-002): `along` is measured on it.
    pub path: Option<walls::curved::Path>,
    pub ty: InsertType,
    pub recipe: InsertRecipe,
    /// Bottom elevation (absolute).
    pub z0: f64,
}

impl Placed {
    /// The length of the host's location curve (along a curved wall's curve).
    pub fn span(&self) -> f64 {
        self.path
            .as_ref()
            .map_or(self.wall.length(), walls::curved::Path::length)
    }

    /// The distance along the host of the point nearest to `p` (plan).
    pub fn along_of(&self, p: [f64; 2]) -> f64 {
        match &self.path {
            Some(path) => path.project(p),
            None => {
                let (u, s) = (self.wall.direction(), self.wall.start);
                (p[0] - s[0]) * u[0] + (p[1] - s[1]) * u[1]
            }
        }
    }

    /// The opening through the host, in the wall's local frame (its outline when shaped).
    pub fn cut(&self) -> openings::Cut {
        let half = self.ty.width / 2.0;
        let outline = if self.ty.shaped() {
            self.outline()
                .into_iter()
                .map(|[x, z]| [self.recipe.along + x, self.z0 + z])
                .collect()
        } else {
            Vec::new()
        };
        openings::Cut {
            x: [self.recipe.along - half, self.recipe.along + half],
            z: [self.z0, self.z0 + self.ty.height],
            outline,
        }
    }

    /// The outline in the insert's own plane: x from its centre, z from its bottom
    /// (ARC-WIN-002).
    pub fn outline(&self) -> Vec<[f64; 2]> {
        shape::outline(&self.ty, self.ty.width, self.ty.height)
    }

    /// A shaped insert's body as prisms through the wall (ARC-WIN-002): the frame follows the
    /// outline at the frame width (a door's is open at the floor), then the glass with its
    /// glazing bars, or the door panel. Empty for rectangles, which are [`Placed::boxes`].
    pub fn solids(&self) -> Vec<Solid> {
        if !self.ty.shaped() {
            return Vec::new();
        }
        let t = &self.ty;
        let outline = self.outline();
        let f = t.frame_width;
        let mut out = Vec::new();
        match self.kind {
            Kind::Window => {
                let d = t.frame_depth.min(self.wall.width) / 2.0;
                let glass = shape::inset(&outline, f, false);
                if glass.is_empty() {
                    return vec![Solid::new(outline, Vec::new(), [-d, d], false)];
                }
                let bars =
                    shape::muntins(t, t.width, t.height, &glass, t.muntins.unwrap_or(0), 0.03);
                out.push(Solid::new(outline, vec![glass.clone()], [-d, d], false));
                out.push(Solid::new(glass, Vec::new(), [-0.005, 0.005], false));
                for bar in bars {
                    out.push(Solid::new(bar, Vec::new(), [-0.012, 0.012], false));
                }
            }
            Kind::Door => {
                let half = self.wall.width / 2.0;
                let outer = shape::from_bottom_left(&outline);
                let inner = shape::from_bottom_left(&shape::inset(&outline, f, true));
                if inner.len() < 3 {
                    return vec![Solid::new(outline, Vec::new(), [-half, half], false)];
                }
                // Up one side of the outline, over the head and down; back up inside it.
                let mut frame: Vec<[f64; 2]> = outer[1..].to_vec();
                frame.push(outer[0]);
                frame.push(inner[0]);
                frame.extend(inner[1..].iter().rev());
                out.push(Solid::new(frame, Vec::new(), [-half, half], false));
                let s = self.facing();
                let face = s * half;
                let panel_y = if s > 0.0 {
                    [face - t.panel_thickness, face]
                } else {
                    [face, face + t.panel_thickness]
                };
                out.push(Solid::new(inner, Vec::new(), panel_y, true));
            }
        }
        out
    }

    /// Local frame point (x along the wall from the insert's centre, y toward the exterior, z
    /// from the bottom) to project coordinates.
    pub fn world(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        // On a curved wall the insert stays straight, square to the curve where it stands.
        if let Some(p) = &self.path {
            let (c, u) = p.at(self.recipe.along);
            let n = [-u[1], u[0]];
            return [
                c[0] + u[0] * x + n[0] * y,
                c[1] + u[1] * x + n[1] * y,
                self.z0 + z,
            ];
        }
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
        for sol in self.solids() {
            // Extruded in (x, z, y) and turned into the insert's frame: swapping two axes
            // mirrors, so the triangles turn round.
            let (v, i) = crate::bodies::extrude(&sol.outer, &sol.holes, sol.y[0], sol.y[1]);
            let target: &mut Mesh = if sol.panel { &mut panel } else { &mut frame };
            let base = target.0.len() as u32;
            target
                .0
                .extend(v.into_iter().map(|[x, z, y]| self.world([x, y, z])));
            for t in i.chunks(3) {
                target.1.extend([t[0] + base, t[2] + base, t[1] + base]);
            }
        }
        for ([x, y, z], is_panel) in self.boxes().into_iter().filter(|_| !self.ty.shaped()) {
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

/// A prism of a shaped insert's body: a profile in its plane (x from its centre, z from its
/// bottom) with holes, through `y` (across the wall, toward its exterior).
#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    pub outer: Vec<[f64; 2]>,
    pub holes: Vec<Vec<[f64; 2]>>,
    pub y: [f64; 2],
    /// The door panel (drawn in 3D only).
    pub panel: bool,
}

impl Solid {
    fn new(outer: Vec<[f64; 2]>, holes: Vec<Vec<[f64; 2]>>, y: [f64; 2], panel: bool) -> Self {
        Self {
            outer,
            holes,
            y,
            panel,
        }
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
    let ty = sized(ty, &recipe);
    let path = walls::wall_recipe(wall_e).and_then(|r| walls::curved::path_of(&r));
    Ok(Placed {
        id: e.id(),
        kind,
        host,
        wall,
        path,
        ty,
        z0: level_z + recipe.sill,
        recipe,
    })
}

/// A door or window as it would be placed (for tool previews): `kind` of type `type_id` (or
/// the default values) in `host` with its centre `along` it, and the type's default sill.
pub fn preview(
    store: &Store,
    kind: Kind,
    type_id: Option<ElementId>,
    host: ElementId,
    along: f64,
    flip_hand: bool,
    flip_facing: bool,
) -> Option<Placed> {
    let host_e = store.get(host)?;
    let wall = walls::regenerate(store, host_e).ok()?;
    let level = walls::wall_recipe(host_e)?.base_level;
    let level_z = find(store, &level)
        .and_then(|l| store.get(l))
        .map_or(wall.base_z, crate::levels::elevation);
    let ty = type_id
        .and_then(|t| store.get(t))
        .and_then(insert_type)
        .filter(|t| t.kind() == Some(kind))
        .unwrap_or_else(|| InsertType::default_for(kind));
    let sill = match kind {
        Kind::Door => 0.0,
        Kind::Window => ty.default_sill,
    };
    Some(Placed {
        id: ElementId(0),
        kind,
        host,
        wall,
        path: walls::wall_recipe(host_e).and_then(|r| walls::curved::path_of(&r)),
        z0: level_z + sill,
        ty,
        recipe: InsertRecipe {
            kind: INSERT_RECIPE.into(),
            host: host_e.guid().to_string(),
            along,
            sill,
            flip_hand,
            flip_facing,
            width: None,
            height: None,
        },
    })
}

/// A type with an insert's own width and height applied.
fn sized(mut ty: InsertType, r: &InsertRecipe) -> InsertType {
    if let Some(w) = r.width {
        ty.width = w;
    }
    if let Some(h) = r.height {
        ty.height = h;
    }
    ty
}

/// The open face of an insert's wall over the insert's span, both faces: its bottom (the wall's
/// base, or the top of a roof the wall rises out of, plus 100 mm) and its top (the underside of
/// the roof it is attached to, less 50 mm, else the wall's top).
fn vertical_room(store: &Store, p: &Placed) -> (f64, f64) {
    let (u, n, s) = (p.wall.direction(), p.wall.normal(), p.wall.start);
    let half = p.ty.width / 2.0;
    let points: Vec<[f64; 2]> = [-half, 0.0, half]
        .iter()
        .flat_map(|dx| {
            let a = p.recipe.along + dx;
            [-1.0, 1.0].map(|side| {
                let o = side * p.wall.width / 2.0;
                [s[0] + u[0] * a + n[0] * o, s[1] + u[1] * a + n[1] * o]
            })
        })
        .collect();
    let mut bottom = p.wall.base_z;
    let mut top = p.wall.base_z + p.wall.height;
    let attached = store
        .get(p.host)
        .and_then(walls::wall_recipe)
        .and_then(|r| r.attach_top);
    for e in store.iter() {
        let Some(_) = crate::roofs::roof_recipe(e) else {
            continue;
        };
        let Ok(g) = crate::roofs::regenerate(store, e) else {
            continue;
        };
        if attached.as_deref() == Some(e.guid().as_str()) {
            if let Some(z) = points
                .iter()
                .filter_map(|q| g.underside(*q))
                .reduce(f64::min)
            {
                top = top.min(z - 0.05);
            }
            continue;
        }
        // The roof's top surface where it meets the wall.
        let tops = points.iter().filter_map(|q| {
            let f = g.face_at(*q)?;
            Some(g.underside(*q)? + g.thickness / f.normal[2].max(1e-6))
        });
        if let Some(z) = tops.reduce(f64::max)
            && z > bottom
            && z < top
        {
            bottom = z + 0.1;
        }
    }
    (bottom, top)
}

/// The smallest a fitted insert may get: its frame each side and 100 mm between.
fn min_size(ty: &InsertType) -> f64 {
    2.0 * ty.frame_width + 0.1
}

/// Fits an insert to its wall where it stands (ARC-WIN-001, fit to host): the free gap around
/// its centre (the wall's clear span, less the other doors and windows at its height) and the
/// wall's height. It slides to fit when the gap is wide enough, else shrinks to the gap; it
/// shrinks to the wall's top. Sets its own width and height only when they differ from the
/// type's. Returns false when even the smallest size does not fit.
pub fn fit(store: &Store, p: &mut Placed) -> bool {
    let solved = walls::solve(store);
    let span = solved
        .bodies
        .get(&p.host)
        .map(openings::clear_span)
        .unwrap_or([0.0, p.span()]);
    let min = min_size(&p.ty);
    // Height: on the wall's open face, above any roof it rises out of and under the roof it
    // is attached to (a dormer's front wall), else within its own height.
    let (bottom, top) = vertical_room(store, p);
    if p.z0 < bottom - 1e-9 {
        let dz = bottom - p.z0;
        p.z0 += dz;
        p.recipe.sill += dz;
    }
    if p.z0 + p.ty.height > top + 1e-9 {
        let h = top - p.z0;
        if h < min {
            return false;
        }
        p.ty.height = h;
        p.recipe.height = Some(h);
        // A round window's diameter, a semicircular or Gothic head's span shrink with it
        // (ARC-WIN-002): the head keeps its shape.
        if let Some(w) = shape::width_for_height(&p.ty, h)
            && w < p.ty.width
        {
            p.ty.width = w;
            p.recipe.width = Some(w);
        }
    }
    let (z0, z1) = (p.z0, p.z0 + p.ty.height);
    // The free gap around the centre.
    let along = p.recipe.along.clamp(span[0], span[1]);
    let (mut lo, mut hi) = (span[0], span[1]);
    for other in placed(store) {
        if other.id == p.id || other.host != p.host {
            continue;
        }
        let o = other.cut();
        if o.z[0] >= z1 - 1e-9 || o.z[1] <= z0 + 1e-9 {
            continue;
        }
        if o.x[1] <= along + 1e-9 {
            lo = lo.max(o.x[1]);
        } else if o.x[0] >= along - 1e-9 {
            hi = hi.min(o.x[0]);
        } else {
            // The centre is inside another: no gap here.
            return false;
        }
    }
    let gap = hi - lo;
    if gap < min - 1e-9 {
        return false;
    }
    let mut w = p.ty.width;
    if w > gap {
        w = gap;
        p.ty.width = w;
        p.recipe.width = Some(w);
        // A round window stays round.
        if p.ty.shape == Shape::Round && p.ty.height > w {
            p.ty.height = w;
            p.recipe.height = Some(w);
        }
    }
    let half = w / 2.0;
    p.recipe.along = along.clamp(lo + half, hi - half);
    true
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
        .unwrap_or([0.0, p.span()]);
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
    let bad = |what: &str| Error::new("type.invalid_value", format!("`{param}` must be {what}"));
    let settings = crate::units::settings_of(txn.store());
    let number = |data_type: &str| -> Result<f64> {
        match value {
            Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) if data_type == "number" => Ok(s.trim().parse().unwrap_or(f64::NAN)),
            Value::String(s) => crate::units::parse(s, data_type, &settings),
            _ => Ok(f64::NAN),
        }
    };
    // The shape's own values (ARC-WIN-002).
    match param {
        "shape" => {
            let shape = value.as_str().and_then(Shape::parse).ok_or_else(|| {
                let names: Vec<&str> = Shape::ALL.iter().map(|s| s.name()).collect();
                bad(&format!("one of {}", names.join(", ")))
            })?;
            if t.kind() == Some(Kind::Door) && !shape.for_doors() {
                return Err(bad("rectangle or round_top for doors"));
            }
            t.shape = shape;
            // A round window's diameter is both its width and its height.
            if shape == Shape::Round {
                t.height = t.width;
            }
            return write_type(txn, id, &t);
        }
        "head_profile" => {
            t.head_profile = value.as_str().and_then(HeadProfile::parse).ok_or_else(|| {
                let names: Vec<&str> = HeadProfile::ALL.iter().map(|s| s.name()).collect();
                bad(&format!("one of {}", names.join(", ")))
            })?;
            return write_type(txn, id, &t);
        }
        "rise" | "left_height" | "right_height" | "radius_ratio" | "sides" | "rotation"
        | "muntins" => {
            let cleared = value.is_null() || value.as_str().is_some_and(|s| s.trim().is_empty());
            let v = if cleared {
                None
            } else {
                let data_type = match param {
                    "rotation" => "angle",
                    "radius_ratio" | "sides" | "muntins" => "number",
                    _ => "length",
                };
                Some(number(data_type)?)
            };
            match (param, v) {
                (_, Some(v)) if !v.is_finite() => return Err(bad("a number")),
                ("rise" | "left_height" | "right_height", Some(v)) if v <= 0.0 => {
                    return Err(bad("a length greater than 0"));
                }
                ("radius_ratio", Some(v)) if v < 0.5 => {
                    return Err(bad("at least 0.5 (a semicircle)"));
                }
                ("sides", Some(v)) if !(3.0..=64.0).contains(&v.round()) => {
                    return Err(bad("from 3 to 64"));
                }
                ("muntins", Some(v)) if !(0.0..=24.0).contains(&v.round()) => {
                    return Err(bad("from 0 to 24"));
                }
                _ => {}
            }
            match param {
                "rise" => t.rise = v,
                "left_height" => t.left_height = v,
                "right_height" => t.right_height = v,
                "radius_ratio" => t.radius_ratio = v,
                "sides" => t.sides = v.map(|v| v.round() as u32),
                "rotation" => t.rotation = v,
                _ => t.muntins = v.map(|v| v.round() as u32),
            }
            return write_type(txn, id, &t);
        }
        _ => {}
    }
    let v = number("length")?;
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
        // A round window keeps its width and height equal.
        "width" | "height" if t.shape == Shape::Round => {
            t.width = v;
            t.height = v;
        }
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
    write_type(txn, id, &t)
}

/// Stores a door or window type's values; its inserts must still fit (a wider door can run into
/// its neighbour).
fn write_type(txn: &mut crate::txn::Transaction<'_>, id: ElementId, t: &InsertType) -> Result<()> {
    let value = serde_json::to_value(t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
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
    /// This insert's own width and height (default the type's).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    /// Fit the wall where it is placed (default true): slide, raise onto the wall's open face,
    /// or shrink to the free gap and the wall's height.
    pub fit: Option<bool>,
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
            match walls::wall_recipe(host_e).and_then(|r| walls::curved::path_of(&r)) {
                Some(path) => path.project(pt),
                None => {
                    let u = g.direction();
                    (pt[0] - g.start[0]) * u[0] + (pt[1] - g.start[1]) * u[1]
                }
            }
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
        width: p.width,
        height: p.height,
    };
    for (v, what) in [(p.width, "width"), (p.height, "height")] {
        if let Some(v) = v
            && !(v.is_finite() && v > min_size(&ty))
        {
            return Err(Error::new(
                "insert.too_small",
                format!("the {what} must be over {:.0} mm", min_size(&ty) * 1000.0),
            ));
        }
    }
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
    let mut placed = resolve(txn.store(), txn.store().get(id).expect("created"))?;
    if p.fit != Some(false) {
        if !fit(txn.store(), &mut placed) {
            return Err(Error::new(
                "insert.no_room",
                format!("there is no room for a {} here on the wall", kind.noun()),
            ));
        }
        if placed.recipe != recipe {
            let value = serde_json::to_value(&placed.recipe).expect("serialises");
            txn.modify(id, |e| {
                e.params.insert(RECIPE_KEY.into(), value);
            })?;
        }
    }
    check_fit(txn.store(), &placed)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSizeParams {
    pub element_ids: Vec<ElementRef>,
    /// The insert's own width; `null` returns it to the type's.
    #[serde(default, deserialize_with = "some_f64")]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<Option<f64>>,
    #[serde(default, deserialize_with = "some_f64")]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<Option<f64>>,
    /// Fit the wall afterwards (shrink to the free gap and open face); default false.
    #[serde(default)]
    pub fit: bool,
}

fn some_f64<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Option<f64>>, D::Error> {
    Option::<f64>::deserialize(d).map(Some)
}

/// Sets doors' and windows' own width and height (`insert.set_size`); centred where they are.
pub(crate) fn set_size(txn: &mut crate::txn::Transaction<'_>, p: SetSizeParams) -> Result<()> {
    for r in &p.element_ids {
        let store = txn.store().clone();
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        let before = resolve(&store, e)?;
        let mut recipe = before.recipe.clone();
        if let Some(w) = p.width {
            recipe.width = w;
        }
        if let Some(h) = p.height {
            recipe.height = h;
        }
        let ty = sized(before.ty.clone(), &recipe);
        if ty.width <= min_size(&ty) || ty.height <= min_size(&ty) {
            return Err(Error::new(
                "insert.too_small",
                format!(
                    "the {} must be over {:.0} mm each way",
                    before.kind.noun(),
                    min_size(&ty) * 1000.0
                ),
            ));
        }
        let mut placed = Placed {
            ty,
            recipe: recipe.clone(),
            ..before
        };
        if p.fit && !fit(&store, &mut placed) {
            return Err(Error::new(
                "insert.no_room",
                "there is no room for it here on the wall",
            ));
        }
        let value = serde_json::to_value(&placed.recipe).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), value);
        })?;
        let now = resolve(txn.store(), txn.store().get(id).expect("exists"))?;
        check_fit(txn.store(), &now)?;
    }
    Ok(())
}

/// Copies a door or window into `host` (GlobalId) at `along`, with its type, sill and flips (the
/// facing flipped with `flip_facing`), a new opening and the next Mark.
pub(crate) fn copy_to(
    txn: &mut crate::txn::Transaction<'_>,
    insert: ElementId,
    host: &str,
    along: f64,
    flip_facing: bool,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let e = store.get(insert).expect("checked by the caller");
    let kind = kind_of(e).expect("an insert");
    let mut recipe = insert_recipe(e).expect("an insert");
    recipe.host = host.to_owned();
    recipe.along = along;
    recipe.flip_facing ^= flip_facing;
    let mark = next_mark(&store, kind);
    let mut copy = NewElement::new(kind.class());
    copy.params = e
        .params
        .iter()
        .filter(|(k, _)| k.as_str() != "rxd:identity")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    copy.params.insert("@Tag".into(), mark.into());
    copy.params.insert(
        RECIPE_KEY.into(),
        serde_json::to_value(&recipe).expect("serialises"),
    );
    let id = txn.create(copy)?;
    let guid = txn.store().get(id).expect("created").guid().to_string();
    txn.create(
        NewElement::new("IfcOpeningElement")
            .param("@PredefinedType", "OPENING")
            .param(
                RECIPE_KEY,
                json!({ "kind": OPENING_RECIPE, "insert": guid }),
            ),
    )?;
    Ok(id)
}

/// Turns doors and windows to face the other side of their wall (after their host is mirrored).
pub(crate) fn toggle_facing(
    txn: &mut crate::txn::Transaction<'_>,
    ids: &[ElementId],
) -> Result<()> {
    for id in ids {
        let store = txn.store().clone();
        let Some(mut recipe) = store.get(*id).and_then(insert_recipe) else {
            continue;
        };
        recipe.flip_facing = !recipe.flip_facing;
        let value = serde_json::to_value(&recipe).expect("serialises");
        txn.modify(*id, |e| {
            e.params.insert(RECIPE_KEY.into(), value);
        })?;
    }
    Ok(())
}

/// Keeps the doors and windows of `wall` (GlobalId) where they are in the model when its
/// location line changes from `old` to `new` (dragging or trimming an end): each one's distance
/// along the wall is measured again from the new start.
pub(crate) fn keep_in_place(
    txn: &mut crate::txn::Transaction<'_>,
    wall: &str,
    old: walls::Line,
    new: walls::Line,
) -> Result<()> {
    let unit = |l: walls::Line| {
        let len = l.length().max(f64::EPSILON);
        [(l.end[0] - l.start[0]) / len, (l.end[1] - l.start[1]) / len]
    };
    let (u0, u1) = (unit(old), unit(new));
    for id in hosted_by(txn.store(), wall) {
        let Some(mut r) = txn.store().get(id).and_then(insert_recipe) else {
            continue;
        };
        let w = [
            old.start[0] + u0[0] * r.along,
            old.start[1] + u0[1] * r.along,
        ];
        let along = (w[0] - new.start[0]) * u1[0] + (w[1] - new.start[1]) * u1[1];
        if (along - r.along).abs() < 1e-12 {
            continue;
        }
        r.along = along;
        let value = serde_json::to_value(&r).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), value);
        })?;
    }
    Ok(())
}

/// The doors and windows hosted by `wall` (GlobalId).
pub fn hosted_by(store: &Store, wall: &str) -> Vec<ElementId> {
    store
        .iter()
        .filter(|e| insert_recipe(e).is_some_and(|r| r.host == wall))
        .map(Element::id)
        .collect()
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
        crate::modify::check_unpinned(&store, id)?;
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
pub struct SetPositionParams {
    pub element_id: ElementRef,
    /// The centre's distance from the host wall's start, along the wall (accepts units).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub along: Option<f64>,
    /// Or the centre's distance from the wall's end.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub from_end: Option<f64>,
}

/// Places a door or window at a distance along its wall (`insert.set_position`).
pub(crate) fn set_position(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetPositionParams,
) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.element_id).ok_or_else(|| {
        Error::new(
            "element.not_found",
            format!("no element {:?}", p.element_id),
        )
    })?;
    crate::modify::check_unpinned(&store, id)?;
    let e = store.get(id).expect("resolved");
    let placed = resolve(&store, e)?;
    let along = match (p.along, p.from_end) {
        (Some(a), None) => a,
        (None, Some(b)) => placed.span() - b,
        _ => {
            return Err(Error::new(
                "insert.bad_position",
                "give either `along` or `from_end`",
            ));
        }
    };
    let mut recipe = placed.recipe;
    recipe.along = along;
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
