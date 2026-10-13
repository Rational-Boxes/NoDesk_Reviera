// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Views as model objects (spec Ch 5 §5.1–5.2, DOC-VIEW-001 and DOC-VIEW-006).
//!
//! A view is an RXD-only element: class `rxd:views/<viewId>` (stored as the RXD document
//! `views/<viewId>`), named by its element name, with its data in the `rxd:view` parameter:
//!
//! ```json
//! { "kind": "view.3d",
//!   "camera": { "eye": [..], "target": [..], "up": [..], "perspective": true,
//!               "fieldOfView": 1.2, "parallelScale": 8.3 },
//!   "home": { ... } }
//! ```
//!
//! `camera` is the saved orientation, set by `view.set_camera` (an undoable edit). Navigation is
//! not an edit: the application session keeps each view's latest camera and Home as view state
//! (`view.orient`, `view.set_projection`, `view.set_home`), saved in the RXD document
//! `views/state` with the next save (UI-NAV-001).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;

/// Element class prefix of views.
pub const CLASS_PREFIX: &str = "rxd:views/";
/// Parameter holding the view's data.
pub const VIEW_KEY: &str = "rxd:view";
pub const KIND_3D: &str = "view.3d";
/// The name of the default 3D view.
pub const DEFAULT_3D: &str = "{3D}";

/// A stored camera (DOC-VIEW-006 `camera` block), in model coordinates (metres).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewCamera {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    #[serde(default = "yes")]
    pub perspective: bool,
    /// Horizontal field of view, radians.
    #[serde(default = "default_fov")]
    pub field_of_view: f64,
    /// Half the view's height at the target in parallel projection, metres.
    #[serde(default = "default_scale")]
    pub parallel_scale: f64,
}

fn yes() -> bool {
    true
}
fn default_fov() -> f64 {
    // 45° vertical at 16:10.
    2.0 * ((45f64.to_radians() / 2.0).tan() * 1.6).atan()
}
fn default_scale() -> f64 {
    10.0
}

impl ViewCamera {
    /// A south-east isometric of the box `lo`..`hi`.
    pub fn fitting(lo: [f64; 3], hi: [f64; 3]) -> Self {
        let target = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
        let radius = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt() / 2.0;
        let radius = radius.max(0.5);
        let distance = radius / (45f64.to_radians() / 2.0).sin() * 1.1;
        let dir = [1.0, -1.0, 1.0].map(|v: f64| v / 3f64.sqrt());
        Self {
            eye: [0, 1, 2].map(|k| target[k] + dir[k] * distance),
            target,
            up: [-0.408_248_29, 0.408_248_29, 0.816_496_58],
            perspective: true,
            field_of_view: default_fov(),
            parallel_scale: radius * 1.1,
        }
    }

    /// The same camera looking from `dir` (toward the eye) at the same distance.
    pub fn oriented(&self, dir: [f64; 3]) -> Self {
        let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let distance = len([0, 1, 2].map(|k| self.eye[k] - self.target[k])).max(1e-6);
        let n = len(dir).max(1e-12);
        let dir = dir.map(|v| v / n);
        // Up: the vertical made perpendicular to the view; north for straight up or down.
        let up = if dir[0].hypot(dir[1]) < 1e-9 {
            [0.0, dir[2].signum(), 0.0]
        } else {
            let d = dir[2];
            let up = [-dir[0] * d, -dir[1] * d, 1.0 - d * d];
            let l = len(up);
            up.map(|v| v / l)
        };
        Self {
            eye: [0, 1, 2].map(|k| self.target[k] + dir[k] * distance),
            up,
            ..*self
        }
    }
}

/// The stored data of a view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewData {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<ViewCamera>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<ViewCamera>,
    /// The associated level of a plan (GlobalId of the `IfcBuildingStorey`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// The view range of a plan (DOC-VR-001).
    #[serde(default, rename = "viewRange", skip_serializing_if = "Option::is_none")]
    pub view_range: Option<crate::plans::ViewRange>,
}

/// Whether an element is a view.
pub fn is_view(e: &Element) -> bool {
    e.class.starts_with(CLASS_PREFIX)
}

/// The data of a view element.
pub fn view_data(e: &Element) -> Option<ViewData> {
    if !is_view(e) {
        return None;
    }
    serde_json::from_value(e.param(VIEW_KEY)?.clone()).ok()
}

/// One line of `view.list`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ViewSummary {
    pub id: ElementId,
    pub guid: String,
    pub name: String,
    pub kind: String,
}

/// Every view of the model, by kind then name.
pub fn list(store: &Store) -> Vec<ViewSummary> {
    let mut views: Vec<ViewSummary> = store
        .iter()
        .filter_map(|e| {
            let data = view_data(e)?;
            Some(ViewSummary {
                id: e.id(),
                guid: e.guid().to_string(),
                name: e.name.clone().unwrap_or_default(),
                kind: data.kind,
            })
        })
        .collect();
    views.sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
    views
}

pub fn resolve_view(store: &Store, r: &ElementRef) -> Result<(ElementId, ViewData)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("view.not_found", "no such view"))?;
    let data = store
        .get(id)
        .and_then(view_data)
        .ok_or_else(|| Error::new("view.not_a_view", format!("element {id} is not a view")))?;
    Ok((id, data))
}

/// Rejects a name already used by another view of the same kind (DOC-VIEW-001).
fn check_name(store: &Store, kind: &str, name: &str, except: Option<ElementId>) -> Result<()> {
    if name.trim().is_empty() {
        return Err(Error::new("view.bad_name", "a view needs a name"));
    }
    let clash = store.iter().find(|e| {
        Some(e.id()) != except
            && e.name.as_deref() == Some(name)
            && view_data(e).is_some_and(|d| d.kind == kind)
    });
    match clash {
        Some(e) => Err(Error::new(
            "view.name_taken",
            format!(
                "another view is already named \"{name}\" (element {})",
                e.id()
            ),
        )),
        None => Ok(()),
    }
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, data: &ViewData) -> Result<()> {
    let value = serde_json::to_value(data).expect("view data serialises");
    txn.modify(id, |e| {
        e.params.insert(VIEW_KEY.into(), value);
    })
}

// ---- Commands ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Create3dKind {
    /// The default 3D view `{3D}`: opened if it exists, created otherwise.
    #[default]
    Default,
    /// A new named 3D view.
    Named,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Create3dParams {
    #[serde(default)]
    pub kind: Create3dKind,
    /// The name of a named view; `{3D 2}`, `{3D 3}`… when omitted.
    pub name: Option<String>,
    /// The starting camera; a south-east isometric of the model when omitted (the UI frames it).
    pub camera: Option<ViewCamera>,
}

pub(crate) fn create_3d(
    txn: &mut crate::txn::Transaction<'_>,
    p: Create3dParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let name = match (p.kind, p.name) {
        (Create3dKind::Default, _) => {
            let existing = store.iter().find(|e| {
                e.name.as_deref() == Some(DEFAULT_3D)
                    && view_data(e).is_some_and(|d| d.kind == KIND_3D)
            });
            if let Some(e) = existing {
                return Ok(e.id());
            }
            DEFAULT_3D.to_owned()
        }
        (Create3dKind::Named, Some(n)) => n,
        (Create3dKind::Named, None) => (2..)
            .map(|n| format!("{{3D {n}}}"))
            .find(|n| check_name(&store, KIND_3D, n, None).is_ok())
            .expect("a free name"),
    };
    check_name(&store, KIND_3D, &name, None)?;
    let guid = GlobalId::new();
    let data = ViewData {
        kind: KIND_3D.into(),
        camera: p.camera,
        home: None,
        level: None,
        view_range: None,
    };
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{}", guid.as_str()))
            .guid(guid)
            .name(name)
            .param(
                VIEW_KEY,
                serde_json::to_value(&data).expect("view data serialises"),
            ),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameParams {
    pub view_id: ElementRef,
    pub new_name: String,
}

pub(crate) fn rename(txn: &mut crate::txn::Transaction<'_>, p: RenameParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, data) = resolve_view(&store, &p.view_id)?;
    check_name(&store, &data.kind, &p.new_name, Some(id))?;
    txn.modify(id, |e| e.name = Some(p.new_name))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DuplicateParams {
    pub view_id: ElementRef,
    #[serde(default)]
    pub mode: DuplicateMode,
    #[serde(alias = "new_name")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateMode {
    /// The view without its annotation.
    #[default]
    Plain,
    /// The view with its annotation (3D views have none yet, so the same as `plain`).
    WithDetailing,
    /// A dependent view (plans, sections and elevations only).
    Dependent,
}

pub(crate) fn duplicate(
    txn: &mut crate::txn::Transaction<'_>,
    p: DuplicateParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let (id, data) = resolve_view(&store, &p.view_id)?;
    if p.mode == DuplicateMode::Dependent {
        return Err(Error::new(
            "view.no_dependents",
            "only plans, sections and elevations can have dependent views",
        ));
    }
    let base = store
        .get(id)
        .and_then(|e| e.name.clone())
        .unwrap_or_default();
    let name = match p.name {
        Some(n) => n,
        None => (1..)
            .map(|n| {
                if n == 1 {
                    format!("{base} Copy")
                } else {
                    format!("{base} Copy {n}")
                }
            })
            .find(|n| check_name(&store, &data.kind, n, None).is_ok())
            .expect("a free name"),
    };
    check_name(&store, &data.kind, &name, None)?;
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{}", guid.as_str()))
            .guid(guid)
            .name(name)
            .param(
                VIEW_KEY,
                serde_json::to_value(&data).expect("view data serialises"),
            ),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewIdsParams {
    pub view_ids: Vec<ElementRef>,
}

pub(crate) fn delete(txn: &mut crate::txn::Transaction<'_>, p: ViewIdsParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.view_ids {
        let (id, _) = resolve_view(&store, r)?;
        txn.delete(id)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewIdParams {
    pub view_id: ElementRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    Perspective,
    Orthographic,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCameraParams {
    pub view_id: ElementRef,
    pub eye: [f64; 3],
    pub target: [f64; 3],
    /// Screen up; the vertical made perpendicular to the view when omitted.
    pub up: Option<[f64; 3]>,
    pub projection: Option<Projection>,
    /// Horizontal field of view, radians.
    pub fov: Option<f64>,
}

pub(crate) fn set_camera(txn: &mut crate::txn::Transaction<'_>, p: SetCameraParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut data) = resolve_view(&store, &p.view_id)?;
    let dir = [0, 1, 2].map(|k| p.eye[k] - p.target[k]);
    if dir.iter().all(|v| v.abs() < 1e-9) {
        return Err(Error::new(
            "view.bad_camera",
            "the eye and target are the same point",
        ));
    }
    let base = data
        .camera
        .unwrap_or_else(|| ViewCamera::fitting([-10.0, -10.0, 0.0], [10.0, 10.0, 3.0]));
    let mut camera = ViewCamera {
        eye: p.eye,
        target: p.target,
        ..base
    }
    .oriented(dir);
    if let Some(up) = p.up {
        camera.up = up;
    }
    if let Some(proj) = p.projection {
        camera.perspective = proj == Projection::Perspective;
    }
    if let Some(fov) = p.fov {
        if !(0.01..3.1).contains(&fov) {
            return Err(Error::new(
                "view.bad_camera",
                "the field of view must be between 0.01 and 3.1 radians",
            ));
        }
        camera.field_of_view = fov;
    }
    data.camera = Some(camera);
    write(txn, id, &data)
}

/// The preset directions of `view.orient` (toward the eye).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
    NeIso,
    NwIso,
    SeIso,
    SwIso,
    Home,
}

impl Preset {
    pub fn direction(self) -> Option<[f64; 3]> {
        Some(match self {
            Preset::Top => [0.0, 0.0, 1.0],
            Preset::Bottom => [0.0, 0.0, -1.0],
            Preset::Front => [0.0, -1.0, 0.0],
            Preset::Back => [0.0, 1.0, 0.0],
            Preset::Left => [-1.0, 0.0, 0.0],
            Preset::Right => [1.0, 0.0, 0.0],
            Preset::NeIso => [1.0, 1.0, 1.0],
            Preset::NwIso => [-1.0, 1.0, 1.0],
            Preset::SeIso => [1.0, -1.0, 1.0],
            Preset::SwIso => [-1.0, -1.0, 1.0],
            Preset::Home => return None,
        })
    }
}

/// A view's data as JSON, for `view.get`.
pub fn summary_json(e: &Element) -> Value {
    json!({ "id": e.id(), "guid": e.guid().as_str(), "name": e.name, "data": e.param(VIEW_KEY) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oriented_cameras_keep_distance_and_have_a_perpendicular_up() {
        let c = ViewCamera::fitting([0.0, 0.0, 0.0], [10.0, 8.0, 6.0]);
        for dir in [[0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [1.0, 1.0, 1.0]] {
            let o = c.oriented(dir);
            let d: Vec<f64> = (0..3).map(|k| o.eye[k] - o.target[k]).collect();
            let dist = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
            let before: Vec<f64> = (0..3).map(|k| c.eye[k] - c.target[k]).collect();
            assert!((dist(&d) - dist(&before)).abs() < 1e-9);
            let dot: f64 = (0..3).map(|k| d[k] * o.up[k]).sum();
            assert!(dot.abs() < 1e-9, "{dir:?}");
        }
        assert_eq!(c.oriented([0.0, 0.0, 1.0]).up, [0.0, 1.0, 0.0]);
    }
}
