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
    /// The section box of a 3D view (DOC-VIEW-008).
    #[serde(
        default,
        rename = "sectionBox",
        skip_serializing_if = "Option::is_none"
    )]
    pub section_box: Option<SectionBox>,
    /// The slice planes of a 3D view (DOC-VIEW-014).
    #[serde(default, rename = "slicePlanes", skip_serializing_if = "Vec::is_empty")]
    pub slice_planes: Vec<SlicePlane>,
    /// The line and depth of a section or elevation (DOC-VIEW-003/004).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<crate::sections::SectionDef>,
}

/// A section box (DOC-VIEW-008): `min`..`max` in the box's own coordinates, placed by
/// `transform` (its x, y and z axes, then its origin; identity when absent).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SectionBox {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<[[f64; 3]; 4]>,
    pub min: [f64; 3],
    pub max: [f64; 3],
    #[serde(default = "yes")]
    pub show_cut_faces: bool,
}

/// The smallest extent of a section box in every direction, metres.
pub const MIN_BOX: f64 = 0.01;
/// The most slice planes a 3D view has.
pub const MAX_SLICES: usize = 8;

impl SectionBox {
    /// The box's axes and origin.
    pub fn frame(&self) -> [[f64; 3]; 4] {
        self.transform
            .unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3]])
    }

    /// The six planes that keep the inside of the box, as (unit normal toward the inside,
    /// offset): a point `p` is kept when `normal · p >= offset` for all of them.
    pub fn planes(&self) -> [([f64; 3], f64); 6] {
        let [x, y, z, o] = self.frame();
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let unit = |a: [f64; 3]| {
            let l = dot(a, a).sqrt().max(1e-12);
            a.map(|v| v / l)
        };
        let axes = [unit(x), unit(y), unit(z)];
        let mut out = [([0.0; 3], 0.0); 6];
        for k in 0..3 {
            let n = axes[k];
            let at = dot(n, o);
            out[2 * k] = (n, at + self.min[k]);
            out[2 * k + 1] = (n.map(|v| -v), -(at + self.max[k]));
        }
        out
    }
}

/// A slice plane (DOC-VIEW-014): what lies on the side `normal` points to is kept.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SlicePlane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    #[serde(default = "yes")]
    pub enabled: bool,
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
        section_box: None,
        slice_planes: Vec::new(),
        section: None,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSectionBoxParams {
    pub view_id: ElementRef,
    pub enabled: bool,
    /// The box's x, y and z axes, then its origin; kept when omitted.
    pub transform: Option<[[f64; 3]; 4]>,
    /// Corners in the box's coordinates, metres; kept when omitted (the model's extents for a
    /// box that never had any are given by the caller).
    pub min: Option<[f64; 3]>,
    pub max: Option<[f64; 3]>,
    pub show_cut_faces: Option<bool>,
}

fn three_d(data: &ViewData) -> Result<()> {
    if data.kind != KIND_3D {
        return Err(Error::new(
            "view.not_3d",
            "section boxes and slice planes belong to 3D views",
        ));
    }
    Ok(())
}

pub(crate) fn set_section_box(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetSectionBoxParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut data) = resolve_view(&store, &p.view_id)?;
    three_d(&data)?;
    let old = data.section_box;
    let (min, max) = match (p.min.or(old.map(|b| b.min)), p.max.or(old.map(|b| b.max))) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            return Err(Error::new(
                "view.bad_section_box",
                "a new section box needs its min and max corners",
            ));
        }
    };
    if (0..3)
        .any(|k| max[k] - min[k] < MIN_BOX - 1e-12 || !min[k].is_finite() || !max[k].is_finite())
    {
        return Err(Error::new(
            "view.bad_section_box",
            "the section box must be at least 10 mm in every direction",
        ));
    }
    let transform = p.transform.or(old.and_then(|b| b.transform));
    if let Some([x, y, z, _]) = transform {
        let cross = |a: [f64; 3], b: [f64; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let c = cross(x, y);
        if (c[0] * z[0] + c[1] * z[1] + c[2] * z[2]).abs() < 1e-9 {
            return Err(Error::new(
                "view.bad_section_box",
                "the section box's axes must not be flat",
            ));
        }
    }
    data.section_box = Some(SectionBox {
        enabled: p.enabled,
        transform,
        min,
        max,
        show_cut_faces: p
            .show_cut_faces
            .or(old.map(|b| b.show_cut_faces))
            .unwrap_or(true),
    });
    write(txn, id, &data)
}

/// A BCF viewpoint clipping plane: a point on it and its direction, which points to the side
/// that is hidden (COL-BCF-003).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClippingPlane {
    pub location: [f64; 3],
    pub direction: [f64; 3],
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit3(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = dot3(a, a).sqrt();
    (l > 1e-12 && l.is_finite()).then(|| a.map(|v| v / l))
}

/// The clipping planes of a BCF viewpoint for a view's enabled section box and slice planes
/// (DOC-VIEW-014): six for the box (located at its face centres), one per slice plane.
pub fn to_bcf(section_box: Option<&SectionBox>, slices: &[SlicePlane]) -> Vec<ClippingPlane> {
    let mut out = Vec::new();
    if let Some(b) = section_box.filter(|b| b.enabled) {
        let [x, y, z, o] = b.frame();
        let mid = [0, 1, 2].map(|k| (b.min[k] + b.max[k]) / 2.0);
        let centre = [0, 1, 2].map(|k| o[k] + x[k] * mid[0] + y[k] * mid[1] + z[k] * mid[2]);
        for (n, d) in b.planes() {
            let t = dot3(n, centre) - d;
            out.push(ClippingPlane {
                location: [0, 1, 2].map(|k| centre[k] - n[k] * t),
                direction: n.map(|v| -v),
            });
        }
    }
    out.extend(slices.iter().filter(|s| s.enabled).map(|s| ClippingPlane {
        location: s.origin,
        direction: s.normal.map(|v| -v),
    }));
    out
}

/// A section box and slice planes from the clipping planes of a BCF viewpoint (DOC-VIEW-014):
/// six planes that pair up as opposite, mutually perpendicular faces become the box, the others
/// slice planes. Planes with no direction are dropped.
pub fn from_bcf(planes: &[ClippingPlane]) -> (Option<SectionBox>, Vec<SlicePlane>) {
    // Kept-side normals and offsets: a point p is kept when n · p >= d.
    let kept: Vec<([f64; 3], f64, ClippingPlane)> = planes
        .iter()
        .filter_map(|c| {
            let n = unit3(c.direction.map(|v| -v))?;
            Some((n, dot3(n, c.location), *c))
        })
        .collect();
    const PARALLEL: f64 = 1e-8;
    const PERPENDICULAR: f64 = 1e-4;
    // Opposite pairs that leave a slab at least MIN_BOX thick: (plus, minus) indices.
    let mut pairs = Vec::new();
    for i in 0..kept.len() {
        for j in 0..kept.len() {
            let (a, b) = (&kept[i], &kept[j]);
            if i != j && dot3(a.0, b.0) < -1.0 + PARALLEL && -b.1 - a.1 >= MIN_BOX - 1e-9 {
                pairs.push((i, j));
            }
        }
    }
    // Several boxes are possible when slices are parallel to box faces (they look the same);
    // writers list a box's six planes together, so the box whose planes are closest together in
    // the list is taken, as captured.
    let mut found: Option<([(usize, usize); 3], usize)> = None;
    for a in 0..pairs.len() {
        for b in a + 1..pairs.len() {
            for c in b + 1..pairs.len() {
                let trio = [pairs[a], pairs[b], pairs[c]];
                let used: std::collections::HashSet<usize> =
                    trio.iter().flat_map(|&(p, q)| [p, q]).collect();
                let n = trio.map(|(p, _)| kept[p].0);
                if used.len() == 6
                    && dot3(n[0], n[1]).abs() < PERPENDICULAR
                    && dot3(n[0], n[2]).abs() < PERPENDICULAR
                    && dot3(n[1], n[2]).abs() < PERPENDICULAR
                {
                    let span = used.iter().max().unwrap_or(&0) - used.iter().min().unwrap_or(&0);
                    if found.is_none_or(|(_, s)| span < s) {
                        found = Some((trio, span));
                    }
                }
            }
        }
    }
    let Some((trio, _)) = found else {
        return (None, kept.iter().map(|k| slice_of(&k.2)).collect());
    };
    // Axes: z the most vertical pair, pointing up; x the pair nearest world x, pointing +x;
    // y completes a right-handed frame.
    let mut trio = trio.to_vec();
    let flip = |(p, q): (usize, usize), toward: [f64; 3]| {
        if dot3(kept[p].0, toward) >= 0.0 {
            (p, q)
        } else {
            (q, p)
        }
    };
    let zi = (0..3)
        .max_by(|&i, &j| {
            kept[trio[i].0].0[2]
                .abs()
                .total_cmp(&kept[trio[j].0].0[2].abs())
        })
        .expect("three pairs");
    let zp = flip(trio.remove(zi), [0.0, 0.0, 1.0]);
    let xi = (0..2)
        .max_by(|&i, &j| {
            kept[trio[i].0].0[0]
                .abs()
                .total_cmp(&kept[trio[j].0].0[0].abs())
        })
        .expect("two pairs");
    let xp = flip(trio.remove(xi), [1.0, 0.0, 0.0]);
    let (z, x) = (kept[zp.0].0, kept[xp.0].0);
    let y = cross3(z, x);
    let yp = flip(trio[0], y);
    let axes = [xp, yp, zp];
    let frame = [kept[xp.0].0, kept[yp.0].0, z];
    let min = axes.map(|(p, _)| kept[p].1);
    let max = axes.map(|(_, q)| -kept[q].1);
    let identity = (0..3).all(|k| (frame[k][k] - 1.0).abs() < 1e-12);
    let section_box = SectionBox {
        enabled: true,
        transform: (!identity).then_some([frame[0], frame[1], frame[2], [0.0; 3]]),
        min,
        max,
        show_cut_faces: true,
    };
    let used: std::collections::HashSet<usize> = axes.iter().flat_map(|&(p, q)| [p, q]).collect();
    let slices = (0..kept.len())
        .filter(|i| !used.contains(i))
        .map(|i| slice_of(&kept[i].2))
        .collect();
    (Some(section_box), slices)
}

fn slice_of(c: &ClippingPlane) -> SlicePlane {
    SlicePlane {
        origin: c.location,
        normal: unit3(c.direction.map(|v| -v)).unwrap_or([0.0, 0.0, 1.0]),
        enabled: true,
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSlicePlanesParams {
    pub view_id: ElementRef,
    /// The view's slice planes (at most eight), replacing the ones it has.
    pub planes: Vec<SlicePlane>,
}

pub(crate) fn set_slice_planes(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetSlicePlanesParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut data) = resolve_view(&store, &p.view_id)?;
    three_d(&data)?;
    if p.planes.len() > MAX_SLICES {
        return Err(Error::new(
            "view.too_many_slices",
            format!("a 3D view has at most {MAX_SLICES} slice planes"),
        ));
    }
    let mut planes = Vec::new();
    for s in p.planes {
        let len = (s.normal.iter().map(|v| v * v).sum::<f64>()).sqrt();
        if len < 1e-9 || !len.is_finite() || s.origin.iter().any(|v| !v.is_finite()) {
            return Err(Error::new(
                "view.bad_plane",
                "a slice plane needs a finite origin and a non-zero normal",
            ));
        }
        planes.push(SlicePlane {
            normal: s.normal.map(|v| v / len),
            ..s
        });
    }
    data.slice_planes = planes;
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
    fn bcf_clipping_planes_round_trip() {
        let (c, sn) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
        let rotated = SectionBox {
            enabled: true,
            transform: Some([[c, sn, 0.0], [-sn, c, 0.0], [0.0, 0.0, 1.0], [0.0; 3]]),
            min: [-1.0, 2.0, 0.5],
            max: [4.0, 6.0, 3.0],
            show_cut_faces: true,
        };
        let slices = [
            SlicePlane {
                origin: [1.0, 2.0, 1.2],
                normal: [0.0, 0.0, -1.0],
                enabled: true,
            },
            SlicePlane {
                origin: [3.0, 0.0, 0.0],
                normal: [0.6, 0.8, 0.0],
                enabled: true,
            },
        ];
        let mut planes = to_bcf(Some(&rotated), &slices);
        assert_eq!(planes.len(), 8);
        // A horizontal slice keeping what is below: BCF direction points up, to the hidden side.
        assert_eq!(planes[6].direction, [0.0, 0.0, 1.0]);
        planes.reverse();
        let (b, back) = from_bcf(&planes);
        let b = b.expect("six planes form a box");
        for (k, (got, want)) in b.planes().iter().zip(rotated.planes()).enumerate() {
            let same = (0..3).all(|i| (got.0[i] - want.0[i]).abs() < 1e-9);
            assert!(
                same && (got.1 - want.1).abs() < 1e-9,
                "plane {k}: {got:?} {want:?}"
            );
        }
        assert_eq!(back.len(), 2);
        assert!(back.iter().any(|s| s.normal == [0.0, 0.0, -1.0]));
        // Axis-aligned boxes come back without a transform; planes alone stay slices.
        let plain = SectionBox {
            transform: None,
            ..rotated
        };
        let (b, rest) = from_bcf(&to_bcf(Some(&plain), &[]));
        assert_eq!(b, Some(plain));
        assert!(rest.is_empty());
        let (b, rest) = from_bcf(&to_bcf(None, &slices));
        assert!(b.is_none() && rest == slices.to_vec());
    }

    #[test]
    fn section_box_planes_keep_the_inside() {
        let b = SectionBox {
            enabled: true,
            transform: Some([
                [0.0, 1.0, 0.0],
                [-1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [10.0, 0.0, 0.0],
            ]),
            min: [0.0, 0.0, 0.0],
            max: [2.0, 3.0, 4.0],
            show_cut_faces: true,
        };
        let kept = |p: [f64; 3]| {
            b.planes()
                .iter()
                .all(|(n, d)| n[0] * p[0] + n[1] * p[1] + n[2] * p[2] >= *d)
        };
        // Box x runs along world +Y from (10, 0); box y along world −X.
        assert!(kept([9.0, 1.0, 2.0]));
        assert!(!kept([11.0, 1.0, 2.0]));
        assert!(!kept([9.0, 2.5, 2.0]));
        assert!(!kept([9.0, 1.0, 4.5]));
    }

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
