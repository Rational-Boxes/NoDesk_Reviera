// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Spot elevations and coordinates as they read now (DOC-ANN-006/007): each spot's point found
//! again on its element's surface in the model's render scene, so its value follows the
//! surface. The spot keeps the point picked and the direction it was picked along
//! (`reviera_core::spots`); a ray along that direction through the stored point finds the
//! element's surface there now. *Top* and *Bottom* are the first and last surface of the element
//! on a vertical line through that point. A spot whose element is gone, or no longer under its
//! point, keeps its stored point and is orphaned (drawn in orange, DOC-ANN-000). A spot slope
//! reads its element's top surface at its point (DOC-ANN-008), the first from above: in a plan
//! the face's steepest slope and its downhill direction, in a section or elevation the slope of
//! the face's line in the view plane (a roof's or ramp's profile as cut or seen); a point well
//! below the top is on a vertical face, which has none, and `spot.create_slope` refuses it. `spot.list` reports a view's
//! spots.

use reviera_core::spots::{self, CreateSpotParams, Display, SpotData, SpotKind};
use reviera_core::{Caller, CommandMeta, Effect, ElementId, ElementRef, Error, GlobalId, HostCommand, Result, Store};
use reviera_render::RenderScene;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;

/// How far back along its direction a spot's ray starts, and how far it looks (metres).
const REACH: f64 = 200.0;

/// A spot as it reads now.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Measured {
    pub id: ElementId,
    pub data: SpotData,
    /// The picked point found again on the surface now (where the spot's symbol stands).
    pub picked: [f64; 3],
    /// The point read: the picked point, or the element's top there.
    pub point: [f64; 3],
    /// The element's bottom there, for *Bottom* and *Top and Bottom*.
    pub bottom: Option<[f64; 3]>,
    /// Slopes: rise over run, as the view reads it.
    pub slope: Option<f64>,
    /// Slopes: downhill, a unit vector in the view's 2D coordinates.
    pub downhill: Option<[f64; 2]>,
    /// The text, one value per line.
    pub lines: Vec<String>,
    /// Its element is gone, or not under its point any more.
    pub orphaned: bool,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Where the ray `origin + dir t` (t ≥ 0) meets the triangle, as t.
fn ray_triangle(origin: [f64; 3], dir: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<f64> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(dir, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(origin, a);
    let u = dot(s, p) * inv;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(dir, q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let t = dot(e2, q) * inv;
    (t >= 0.0).then_some(t)
}

/// Every t at which the ray meets `element`'s meshes, nearest first, with the unit normal of the
/// face met there (facing back along the ray).
fn hits(scene: &RenderScene, element: ElementId, origin: [f64; 3], dir: [f64; 3]) -> Vec<(f64, [f64; 3])> {
    let mut out = Vec::new();
    for m in scene.meshes.iter().filter(|m| m.element == Some(element) && !reviera_render::is_virtual(&m.class)) {
        let at = |k: u32| m.positions[k as usize].map(f64::from);
        for t in m.indices.as_chunks::<3>().0 {
            let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
            if let Some(d) = ray_triangle(origin, dir, a, b, c) {
                let n = cross(sub(b, a), sub(c, a));
                let l = dot(n, n).sqrt().max(1e-300);
                let n = n.map(|v| v / l);
                out.push((d, if dot(n, dir) > 0.0 { n.map(|v| -v) } else { n }));
            }
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// The surface a spot reads of the hits along its ray (which starts [`REACH`] before its point):
/// looking down, as in plans, the first from above; else the one nearest the point.
fn nearest(hits: Vec<(f64, [f64; 3])>, dir: [f64; 3]) -> Option<(f64, [f64; 3])> {
    if dir[2] < -0.99 {
        return hits.into_iter().next();
    }
    hits.into_iter().min_by(|a, b| (a.0 - REACH).abs().total_cmp(&(b.0 - REACH).abs()))
}

/// Why a spot slope has no face to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoSlope {
    /// The element has no surface there.
    NoFace,
    /// The point is on a vertical face (its element's top is well above it).
    Vertical,
}

/// How far a section's or elevation's spot slope point may be below its element's top and still
/// be on it (metres).
const ON_TOP: f64 = 0.15;

/// The face a spot slope reads, by its unit normal: the first surface of `element` from above at
/// the point, which in a section or elevation must be at the point (a point well below it is on
/// a vertical face, a wall's side).
pub fn slope_face(scene: &RenderScene, element: ElementId, point: [f64; 3], dir: [f64; 3]) -> std::result::Result<[f64; 3], NoSlope> {
    // A millimetre into what the view looks at, off an edge-on face.
    let p = if dir[2] < -0.99 { point } else { [point[0] + dir[0] * 0.001, point[1] + dir[1] * 0.001, point[2]] };
    let origin = [p[0], p[1], 1e4];
    let (t, n) = hits(scene, element, origin, [0.0, 0.0, -1.0]).into_iter().next().ok_or(NoSlope::NoFace)?;
    if dir[2] >= -0.99 && (1e4 - t) - point[2] > ON_TOP {
        return Err(NoSlope::Vertical);
    }
    Ok(n)
}

/// A face's slope as view `view` (GlobalId) reads it, from the face's unit `normal`: rise over
/// run and the downhill direction in the view's 2D coordinates. In a plan, its steepest slope;
/// in a section or elevation, the slope of its line in the view plane. `None` for a vertical
/// face (or one a section sees face on).
pub fn face_slope(store: &Store, view: &str, normal: [f64; 3]) -> Option<(f64, [f64; 2])> {
    let n = if normal[2] < 0.0 { normal.map(|v| -v) } else { normal };
    match reviera_core::sections::frame_of_view(store, view) {
        None => {
            let h = n[0].hypot(n[1]);
            if n[2] < 1e-6 {
                return None;
            }
            let downhill = if h < 1e-12 { [0.0, 0.0] } else { [n[0] / h, n[1] / h] };
            Some((h / n[2], downhill))
        }
        Some((frame, _)) => {
            let look = [frame.look[0], frame.look[1], 0.0];
            let d = cross(n, look);
            let (x, y) = (d[0] * frame.right[0] + d[1] * frame.right[1], d[2]);
            let l = x.hypot(y);
            if l < 1e-9 || x.abs() < 1e-6 * l {
                return None;
            }
            let (x, y) = if y > 0.0 { (-x, -y) } else { (x, y) };
            Some(((y / x).abs(), [x / l, y / l]))
        }
    }
}

/// The first surface of `element` from above at plan point (`x`, `y`): where a spot picked on it
/// in a plan reads (a slab's top, or the top of a wall the plan cuts).
pub fn top_at(scene: &RenderScene, element: ElementId, x: f64, y: f64) -> Option<[f64; 3]> {
    let origin = [x, y, 1e4];
    hits(scene, element, origin, [0.0, 0.0, -1.0]).first().map(|(t, _)| [x, y, 1e4 - t])
}

/// A spot measured against `scene` (`None`: as stored).
pub fn measure(store: &Store, scene: Option<&RenderScene>, id: ElementId, data: SpotData) -> Measured {
    let element = data.element.as_deref().and_then(GlobalId::parse).and_then(|g| store.find_by_guid(&g));
    let at = |o: [f64; 3], d: [f64; 3], t: f64| [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
    let found = match (element, scene) {
        (Some(e), Some(scene)) => {
            // The surface along the spot's direction, nearest the stored point.
            let dir = data.direction;
            let origin = at(data.point, dir, -REACH);
            nearest(hits(scene, e, origin, dir), dir).map(|(t, n)| (at(origin, dir, t), n))
        }
        _ => None,
    };
    let orphaned = data.element.is_some() && found.is_none();
    let picked = found.map_or(data.point, |f| f.0);
    // Top and bottom on the vertical through the point.
    let (point, bottom) = match (data.spot_kind, data.display, element, scene) {
        (SpotKind::Elevation, Display::Top | Display::Bottom | Display::TopAndBottom, Some(e), Some(scene)) if !orphaned => {
            // A millimetre into the element, off the face it was picked on.
            let down = [0.0, 0.0, -1.0];
            let inside = at(picked, data.direction, 0.001);
            let origin = [inside[0], inside[1], picked[2] + REACH];
            let v = hits(scene, e, origin, down);
            match (v.first(), v.last()) {
                (Some(t), Some(b)) => (at(origin, down, t.0), Some(at(origin, down, b.0))),
                _ => (picked, None),
            }
        }
        _ => (picked, None),
    };
    // A slope from the top surface there.
    let face = match (data.spot_kind, element, scene) {
        (SpotKind::Slope, Some(e), Some(scene)) => slope_face(scene, e, data.point, data.direction).ok(),
        _ => None,
    };
    let read = face.and_then(|n| face_slope(store, &data.view, n));
    let orphaned = if data.spot_kind == SpotKind::Slope { data.element.is_some() && face.is_none() } else { orphaned };
    let lines = match (data.spot_kind, read) {
        (SpotKind::Slope, Some((slope, _))) => spots::slope_lines(&data, slope),
        (SpotKind::Slope, None) => vec!["?".to_owned()],
        _ => spots::lines(store, &data, point, bottom),
    };
    let (slope, downhill) = (read.map(|r| r.0), read.map(|r| r.1));
    Measured { id, data, picked, point, bottom, slope, downhill, lines, orphaned }
}

/// The spots of view `view` (GlobalId), measured against `scene`.
pub fn of_view(store: &Store, scene: Option<&RenderScene>, view: &str) -> Vec<Measured> {
    spots::of_view(store, view).into_iter().map(|(id, d)| measure(store, scene, id, d)).collect()
}

/// Every spot of the model, measured against `scene`.
pub fn all(store: &Store, scene: Option<&RenderScene>) -> Vec<Measured> {
    store.iter().filter_map(|e| Some((e.id(), spots::spot_data(e)?))).map(|(id, d)| measure(store, scene, id, d)).collect()
}

/// `spot.create_slope`.
#[derive(Debug)]
pub struct SpotCreateSlope;

impl HostCommand for SpotCreateSlope {
    type Host = App;
    type Params = CreateSpotParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "spot.create_slope",
        version: 1,
        summary: "Place a spot slope",
        description: "Places a spot slope (DOC-ANN-008) at `point` (model coordinates) on the element `ref`, in a \
                      plan, section or elevation: in a plan the face's steepest slope with an arrow pointing \
                      downhill, in a section or elevation the slope of the face's line as the view sees it. \
                      `representation`: `arrow` (default) or `triangle` (sections and elevations); \
                      `slope_format`: `percent` (default), `ratio` (1 : n) or `degrees`. Refused on a vertical \
                      face. It follows the face as the model changes.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: CreateSpotParams) -> Result<ElementId> {
        let doc = app.resolve(None)?;
        let scene = app.render_scene(doc)?;
        let store = app.get(doc)?.doc.store();
        let bad = |code: &'static str, msg: &str| Error::new(code, msg);
        let element = p.element_ref.as_ref().ok_or_else(|| bad("spot.no_ref", "a spot slope reads a face of an element: give `ref`"))?;
        let element = store.resolve(element).ok_or_else(|| bad("element.not_found", "no such element for the spot"))?;
        let view = store.resolve(&p.view_id).and_then(|v| store.get(v)).ok_or_else(|| bad("view.not_found", "no such view"))?;
        let guid = view.guid().to_string();
        let frame = reviera_core::sections::frame_of_view(store, &guid);
        let dir = match (p.direction, &frame) {
            (Some(d), _) => d,
            (None, Some((f, _))) => [f.look[0], f.look[1], 0.0],
            (None, None) => [0.0, 0.0, -1.0],
        };
        let normal = match slope_face(&scene, element, p.point, dir) {
            Ok(n) => n,
            Err(NoSlope::NoFace) => return Err(bad("spot.no_face", "the element has no surface at that point")),
            Err(NoSlope::Vertical) => return Err(bad("spot.vertical", "the point is on a vertical face, which has no slope")),
        };
        if face_slope(store, &guid, normal).is_none() {
            return Err(bad("spot.vertical", if frame.is_none() { "a vertical face has no slope to show in a plan" } else { "the face is vertical (or seen face on) in this view" }));
        }
        let args = serde_json::to_value(&p).map_err(|e| bad("spot.args", &e.to_string()))?;
        let out = app.execute("spot.put_slope", args, caller)?;
        serde_json::from_value(out.result).map_err(|e| bad("spot.put", &e.to_string()))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub view_id: ElementRef,
}

/// `spot.list`.
#[derive(Debug)]
pub struct SpotList;

impl HostCommand for SpotList {
    type Host = App;
    type Params = ListParams;
    type Output = Vec<Measured>;
    const META: CommandMeta = CommandMeta {
        name: "spot.list",
        version: 1,
        summary: "The spot elevations and coordinates of a view",
        description: "Lists a view's spots (DOC-ANN-006 … 008) as they read now: each one's point found again \
                      on its element's surface, the element's bottom for top-and-bottom spots, a slope's value \
                      and downhill direction, its text lines, and whether it is orphaned (its element gone or no \
                      longer under its point).",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ListParams) -> Result<Vec<Measured>> {
        let doc = app.resolve(None)?;
        let scene = app.render_scene(doc)?;
        let store = app.get(doc)?.doc.store();
        let id = store.resolve(&p.view_id).ok_or_else(|| Error::new("view.not_found", "no such view"))?;
        let guid = store.get(id).ok_or_else(|| Error::new("view.not_found", "no such view"))?.guid().to_string();
        Ok(of_view(store, Some(&scene), &guid))
    }
}
