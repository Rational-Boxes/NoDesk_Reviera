// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Work planes (spec Ch 3 §3.2.6, ARC-WP-001): the planes that model lines (and later model text
//! and work-plane-based components) are drawn on.
//!
//! A work plane is named by what defines it ([`WorkPlaneRef`]): a level (horizontal, at its
//! elevation plus an offset), a grid or a reference plane (vertical planes through their lines),
//! or an explicit frame. An element on a work plane stores the reference with local coordinates
//! in it, so it follows its host: raise the level or move the reference plane, and it moves too.
//! [`WorkPlaneRef::Explicit`] is a plane fixed in space (what *Dissociate* leaves).
//!
//! Every view has an active work plane (`rxd:work_plane` on the view): one set with
//! `workplane.set`, else a plan's own level. Sections, elevations and 3D views have none until one
//! is set. `workplane.show` turns its grid on in the view (drawn by the window).
//!
//! Not built yet: picking a face as the plane, Go To View for vertical planes, and the Work Plane
//! Viewer.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

/// Parameter on a view holding its active work plane.
pub const VIEW_KEY: &str = "rxd:work_plane";
/// The default spacing of a shown work plane's grid (metres).
pub const DEFAULT_GRID: f64 = 1.0;

/// What defines a work plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "by")]
pub enum WorkPlaneRef {
    /// A level's horizontal plane (GlobalId), raised by `offset`.
    Level {
        level: String,
        #[serde(default)]
        offset: f64,
    },
    /// The vertical plane through a grid line (GlobalId).
    Grid { grid: String },
    /// A reference plane (GlobalId).
    RefPlane { ref_plane: String },
    /// A plane fixed in space: origin, x direction and normal (its y is normal × x).
    Explicit {
        origin: [f64; 3],
        x_dir: [f64; 3],
        normal: [f64; 3],
    },
}

/// A resolved plane: an origin and orthonormal x, y and normal (x × y).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Plane {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub normal: [f64; 3],
}

pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = dot(a, a).sqrt();
    (l > 1e-9).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

impl Plane {
    /// The plane through `origin` with x direction `x` (made square to the normal) and `normal`.
    pub fn new(origin: [f64; 3], x: [f64; 3], normal: [f64; 3]) -> Result<Self> {
        let bad = || {
            Error::new(
                "workplane.degenerate",
                "the plane's directions are degenerate",
            )
        };
        let n = unit(normal).ok_or_else(bad)?;
        let x = unit(sub(x, scale(n, dot(x, n)))).ok_or_else(bad)?;
        let y = cross(n, x);
        Ok(Self {
            origin,
            x,
            y,
            normal: n,
        })
    }

    /// The world point at local (`u`, `v`).
    pub fn to_world(&self, p: [f64; 2]) -> [f64; 3] {
        [
            self.origin[0] + self.x[0] * p[0] + self.y[0] * p[1],
            self.origin[1] + self.x[1] * p[0] + self.y[1] * p[1],
            self.origin[2] + self.x[2] * p[0] + self.y[2] * p[1],
        ]
    }

    /// The local coordinates of a world point (projected onto the plane).
    pub fn to_local(&self, p: [f64; 3]) -> [f64; 2] {
        let d = sub(p, self.origin);
        [dot(d, self.x), dot(d, self.y)]
    }

    /// Whether the plane is horizontal.
    pub fn is_horizontal(&self) -> bool {
        self.normal[2].abs() > 1.0 - 1e-9
    }

    /// Whether the plane is vertical.
    pub fn is_vertical(&self) -> bool {
        self.normal[2].abs() < 1e-9
    }
}

pub fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
}

/// Resolves a work plane reference to a plane.
pub fn resolve(store: &Store, r: &WorkPlaneRef) -> Result<Plane> {
    match r {
        WorkPlaneRef::Level { level, offset } => {
            let z = crate::levels::elevation_of(store, level)? + offset;
            Plane::new([0.0, 0.0, z], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0])
        }
        WorkPlaneRef::Grid { grid } => {
            let g = by_guid(store, grid)
                .and_then(crate::grids::grid_data)
                .ok_or_else(|| Error::new("workplane.missing", format!("grid {grid} is gone")))?;
            let (a, b) = (g.curve.start, g.curve.end);
            let x = [b[0] - a[0], b[1] - a[1], 0.0];
            // With its normal x × z, the plane's y (normal × x) is up.
            Plane::new([a[0], a[1], 0.0], x, cross(x, [0.0, 0.0, 1.0]))
        }
        WorkPlaneRef::RefPlane { ref_plane } => {
            let e = by_guid(store, ref_plane).ok_or_else(|| {
                Error::new(
                    "workplane.missing",
                    format!("reference plane {ref_plane} is gone"),
                )
            })?;
            let r = crate::ref_planes::recipe(e).ok_or_else(|| {
                Error::new(
                    "workplane.not_a_plane",
                    format!("{ref_plane} is not a reference plane"),
                )
            })?;
            crate::ref_planes::plane_of(&r)
        }
        WorkPlaneRef::Explicit {
            origin,
            x_dir,
            normal,
        } => Plane::new(*origin, *x_dir, *normal),
    }
}

/// The element a work plane reference depends on, if any (its GlobalId).
pub fn host_of(r: &WorkPlaneRef) -> Option<&str> {
    match r {
        WorkPlaneRef::Level { level, .. } => Some(level),
        WorkPlaneRef::Grid { grid } => Some(grid),
        WorkPlaneRef::RefPlane { ref_plane } => Some(ref_plane),
        WorkPlaneRef::Explicit { .. } => None,
    }
}

/// The plane fixed in space that `r` is now (Dissociate).
pub fn dissociated(store: &Store, r: &WorkPlaneRef) -> Result<WorkPlaneRef> {
    let p = resolve(store, r)?;
    Ok(WorkPlaneRef::Explicit {
        origin: p.origin,
        x_dir: p.x,
        normal: p.normal,
    })
}

/// A short label for a work plane: "Level: Level 1", "Reference Plane: Axis A", …
pub fn label(store: &Store, r: &WorkPlaneRef) -> String {
    let name = |guid: &str| {
        by_guid(store, guid)
            .and_then(|e| e.name.clone())
            .unwrap_or_else(|| "(unnamed)".to_owned())
    };
    match r {
        WorkPlaneRef::Level { level, offset } if *offset != 0.0 => {
            format!("Level: {} {offset:+.3} m", name(level))
        }
        WorkPlaneRef::Level { level, .. } => format!("Level: {}", name(level)),
        WorkPlaneRef::Grid { grid } => format!("Grid: {}", name(grid)),
        WorkPlaneRef::RefPlane { ref_plane } => format!("Reference Plane: {}", name(ref_plane)),
        WorkPlaneRef::Explicit { .. } => "<not associated>".to_owned(),
    }
}

/// A work plane given in command arguments: one of `level` (with `offset`), `grid`, `ref_plane`
/// or an explicit `plane`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PlaneArgs {
    pub level: Option<ElementRef>,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    pub grid: Option<ElementRef>,
    pub ref_plane: Option<ElementRef>,
    /// An explicit plane: `{origin, x_dir, normal}`.
    pub plane: Option<ExplicitPlane>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ExplicitPlane {
    pub origin: [f64; 3],
    pub x_dir: [f64; 3],
    pub normal: [f64; 3],
}

impl PlaneArgs {
    pub fn given(&self) -> bool {
        self.level.is_some()
            || self.grid.is_some()
            || self.ref_plane.is_some()
            || self.plane.is_some()
    }

    /// The reference these arguments name (exactly one of them).
    pub fn to_ref(&self, store: &Store) -> Result<WorkPlaneRef> {
        let n = [
            self.level.is_some(),
            self.grid.is_some(),
            self.ref_plane.is_some(),
            self.plane.is_some(),
        ]
        .iter()
        .filter(|b| **b)
        .count();
        if n != 1 {
            return Err(Error::new(
                "workplane.args",
                "give exactly one of `level`, `grid`, `ref_plane` or `plane`",
            ));
        }
        let guid = |r: &ElementRef, what: &str, ok: &dyn Fn(&Element) -> bool| -> Result<String> {
            store
                .resolve(r)
                .and_then(|id| store.get(id))
                .filter(|e| ok(e))
                .map(|e| e.guid().to_string())
                .ok_or_else(|| Error::new("workplane.not_found", format!("no {what} {r:?}")))
        };
        let r = if let Some(l) = &self.level {
            WorkPlaneRef::Level {
                level: guid(l, "level", &|e| e.class == crate::levels::CLASS)?,
                offset: self.offset,
            }
        } else if let Some(g) = &self.grid {
            WorkPlaneRef::Grid {
                grid: guid(g, "grid", &|e| crate::grids::is_grid(e))?,
            }
        } else if let Some(p) = &self.ref_plane {
            WorkPlaneRef::RefPlane {
                ref_plane: guid(p, "reference plane", &|e| {
                    crate::ref_planes::recipe(e).is_some()
                })?,
            }
        } else {
            let p = self.plane.as_ref().expect("counted");
            WorkPlaneRef::Explicit {
                origin: p.origin,
                x_dir: p.x_dir,
                normal: p.normal,
            }
        };
        resolve(store, &r)?;
        Ok(r)
    }
}

/// A view's stored work plane setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewWorkPlane {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<WorkPlaneRef>,
    #[serde(default)]
    pub show: bool,
    /// The shown grid's spacing (metres).
    #[serde(default = "default_grid")]
    pub grid_spacing: f64,
}

fn default_grid() -> f64 {
    DEFAULT_GRID
}

fn view_setting(e: &Element) -> ViewWorkPlane {
    e.param(VIEW_KEY)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(ViewWorkPlane {
            plane: None,
            show: false,
            grid_spacing: DEFAULT_GRID,
        })
}

/// A view's active work plane: the one set, else a plan's level.
pub fn of_view(store: &Store, view: &Element) -> Option<WorkPlaneRef> {
    view_setting(view)
        .plane
        .or_else(|| {
            crate::views::view_data(view)
                .and_then(|d| d.level)
                .map(|level| WorkPlaneRef::Level { level, offset: 0.0 })
        })
        .filter(|r| resolve(store, r).is_ok())
}

fn view_element<'a>(store: &'a Store, r: &ElementRef) -> Result<&'a Element> {
    store
        .resolve(r)
        .and_then(|id| store.get(id))
        .filter(|e| crate::views::is_view(e))
        .ok_or_else(|| Error::new("view.not_found", format!("no view {r:?}")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub view_id: ElementRef,
    #[serde(flatten)]
    pub plane: PlaneArgs,
}

/// `workplane.set`: sets a view's active work plane.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<WorkPlaneInfo> {
    let store = txn.store().clone();
    let view = view_element(&store, &p.view_id)?;
    let r = p.plane.to_ref(&store)?;
    let mut s = view_setting(view);
    s.plane = Some(r);
    let id = view.id();
    let value = serde_json::to_value(&s).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(VIEW_KEY.into(), value);
    })?;
    get(
        txn.store(),
        ViewParams {
            view_id: ElementRef::Id(id),
        },
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowParams {
    pub view_id: ElementRef,
    pub on: bool,
    /// The grid's spacing (metres).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub grid_spacing: Option<f64>,
}

/// `workplane.show`: shows or hides a view's work plane grid.
pub(crate) fn show(txn: &mut Transaction<'_>, p: ShowParams) -> Result<()> {
    let store = txn.store().clone();
    let view = view_element(&store, &p.view_id)?;
    let mut s = view_setting(view);
    s.show = p.on;
    if let Some(g) = p.grid_spacing {
        if g.is_nan() || g <= 0.0 {
            return Err(Error::new(
                "workplane.grid",
                "the grid spacing must be positive",
            ));
        }
        s.grid_spacing = g;
    }
    let value = serde_json::to_value(&s).expect("serialises");
    txn.modify(view.id(), |e| {
        e.params.insert(VIEW_KEY.into(), value);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewParams {
    pub view_id: ElementRef,
}

/// A view's work plane, as `workplane.get` reports it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkPlaneInfo {
    /// What defines it (`None`: the view has no work plane).
    pub plane: Option<WorkPlaneRef>,
    pub label: String,
    pub resolved: Option<Plane>,
    pub show: bool,
    pub grid_spacing: f64,
}

/// `workplane.get`.
pub(crate) fn get(store: &Store, p: ViewParams) -> Result<WorkPlaneInfo> {
    let view = view_element(store, &p.view_id)?;
    let s = view_setting(view);
    let plane = of_view(store, view);
    Ok(WorkPlaneInfo {
        label: plane
            .as_ref()
            .map_or_else(|| "<none>".to_owned(), |r| label(store, r)),
        resolved: plane.as_ref().and_then(|r| resolve(store, r).ok()),
        plane,
        show: s.show,
        grid_spacing: s.grid_spacing,
    })
}

/// Named work planes a user can choose (the Name list): levels, grids and named reference planes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NamedPlane {
    pub label: String,
    pub plane: WorkPlaneRef,
}

pub fn named(store: &Store) -> Vec<NamedPlane> {
    let mut out: Vec<NamedPlane> = crate::levels::list(store)
        .into_iter()
        .map(|l| NamedPlane {
            label: format!("Level: {}", l.name),
            plane: WorkPlaneRef::Level {
                level: l.guid,
                offset: 0.0,
            },
        })
        .collect();
    out.extend(crate::grids::list(store).into_iter().map(|g| NamedPlane {
        label: format!("Grid: {}", g.name),
        plane: WorkPlaneRef::Grid {
            grid: store.get(g.id).expect("listed").guid().to_string(),
        },
    }));
    out.extend(
        crate::ref_planes::list(store)
            .into_iter()
            .filter(|r| r.name.is_some())
            .map(|r| NamedPlane {
                label: format!("Reference Plane: {}", r.name.clone().unwrap_or_default()),
                plane: WorkPlaneRef::RefPlane { ref_plane: r.guid },
            }),
    );
    out
}

/// `workplane.list`: the named planes.
pub(crate) fn list_named(store: &Store, _: crate::builtin::NoParams) -> Result<Vec<NamedPlane>> {
    Ok(named(store))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetElementParams {
    pub element_ids: Vec<ElementRef>,
    #[serde(flatten)]
    pub plane: PlaneArgs,
}

/// `element.set_work_plane`: moves elements onto another work plane, keeping their shape in
/// local coordinates of the plane (so they move to it).
pub(crate) fn set_element(txn: &mut Transaction<'_>, p: SetElementParams) -> Result<()> {
    let store = txn.store().clone();
    let r = p.plane.to_ref(&store)?;
    for e in &p.element_ids {
        let id = resolve_hosted(&store, e)?;
        crate::model_lines::set_plane(txn, id, r.clone())?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsParams {
    pub element_ids: Vec<ElementRef>,
}

/// `element.dissociate_work_plane`: fixes elements where they are, without a host.
pub(crate) fn dissociate(txn: &mut Transaction<'_>, p: ElementsParams) -> Result<()> {
    let store = txn.store().clone();
    for e in &p.element_ids {
        let id = resolve_hosted(&store, e)?;
        let r = crate::model_lines::recipe(store.get(id).expect("resolved"))
            .expect("checked")
            .plane;
        let fixed = dissociated(&store, &r)?;
        crate::model_lines::set_plane(txn, id, fixed)?;
    }
    Ok(())
}

fn resolve_hosted(store: &Store, r: &ElementRef) -> Result<ElementId> {
    store
        .resolve(r)
        .filter(|id| {
            store
                .get(*id)
                .is_some_and(|e| crate::model_lines::recipe(e).is_some())
        })
        .ok_or_else(|| {
            Error::new(
                "workplane.not_hosted",
                format!("{r:?} is not an element on a work plane"),
            )
        })
}

/// The work plane reference after a plan transformation of its elements: a level's plane stays
/// (its elements' local points move instead); other planes become explicit, transformed.
pub(crate) fn transform_frame(
    store: &Store,
    r: &WorkPlaneRef,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    dz: f64,
) -> Result<WorkPlaneRef> {
    let p = resolve(store, r)?;
    let o = point([p.origin[0], p.origin[1]]);
    let vector = |v: [f64; 3]| {
        let a = point([0.0, 0.0]);
        let b = point([v[0], v[1]]);
        [b[0] - a[0], b[1] - a[1], v[2]]
    };
    let x = vector(p.x);
    let y = vector(p.y);
    Ok(WorkPlaneRef::Explicit {
        origin: [o[0], o[1], p.origin[2] + dz],
        x_dir: x,
        normal: cross(x, y),
    })
}

/// JSON for a work plane in reports.
pub fn to_json(store: &Store, r: &WorkPlaneRef) -> Value {
    json!({ "label": label(store, r), "plane": r })
}
