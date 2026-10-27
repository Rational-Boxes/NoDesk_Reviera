// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Spot elevations and coordinates as they read now (DOC-ANN-006/007): each spot's point found
//! again on its element's surface in the model's render scene, so its value follows the
//! surface. The spot keeps the point picked and the direction it was picked along
//! (`reviera_core::spots`); a ray along that direction through the stored point finds the
//! element's surface there now. *Top* and *Bottom* are the first and last surface of the element
//! on a vertical line through that point. A spot whose element is gone, or no longer under its
//! point, keeps its stored point and is orphaned (drawn in orange, DOC-ANN-000). `spot.list`
//! reports a view's spots.

use reviera_core::spots::{self, Display, SpotData, SpotKind};
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

/// Every t at which the ray meets `element`'s meshes, nearest first.
fn hits(scene: &RenderScene, element: ElementId, origin: [f64; 3], dir: [f64; 3]) -> Vec<f64> {
    let mut out = Vec::new();
    for m in scene.meshes.iter().filter(|m| m.element == Some(element) && !reviera_render::is_virtual(&m.class)) {
        let at = |k: u32| m.positions[k as usize].map(f64::from);
        for t in m.indices.as_chunks::<3>().0 {
            if let Some(d) = ray_triangle(origin, dir, at(t[0]), at(t[1]), at(t[2])) {
                out.push(d);
            }
        }
    }
    out.sort_by(f64::total_cmp);
    out
}

/// The first surface of `element` from above at plan point (`x`, `y`): where a spot picked on it
/// in a plan reads (a slab's top, or the top of a wall the plan cuts).
pub fn top_at(scene: &RenderScene, element: ElementId, x: f64, y: f64) -> Option<[f64; 3]> {
    let origin = [x, y, 1e4];
    hits(scene, element, origin, [0.0, 0.0, -1.0]).first().map(|t| [x, y, 1e4 - t])
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
            let along = hits(scene, e, origin, dir);
            along.iter().min_by(|a, b| (*a - REACH).abs().total_cmp(&(*b - REACH).abs())).map(|t| at(origin, dir, *t))
        }
        _ => None,
    };
    let orphaned = data.element.is_some() && found.is_none();
    let picked = found.unwrap_or(data.point);
    // Top and bottom on the vertical through the point.
    let (point, bottom) = match (data.spot_kind, data.display, element, scene) {
        (SpotKind::Elevation, Display::Top | Display::Bottom | Display::TopAndBottom, Some(e), Some(scene)) if !orphaned => {
            // A millimetre into the element, off the face it was picked on.
            let down = [0.0, 0.0, -1.0];
            let inside = at(picked, data.direction, 0.001);
            let origin = [inside[0], inside[1], picked[2] + REACH];
            let v = hits(scene, e, origin, down);
            match (v.first(), v.last()) {
                (Some(t), Some(b)) => (at(origin, down, *t), Some(at(origin, down, *b))),
                _ => (picked, None),
            }
        }
        _ => (picked, None),
    };
    let lines = spots::lines(store, &data, point, bottom);
    Measured { id, data, picked, point, bottom, lines, orphaned }
}

/// The spots of view `view` (GlobalId), measured against `scene`.
pub fn of_view(store: &Store, scene: Option<&RenderScene>, view: &str) -> Vec<Measured> {
    spots::of_view(store, view).into_iter().map(|(id, d)| measure(store, scene, id, d)).collect()
}

/// Every spot of the model, measured against `scene`.
pub fn all(store: &Store, scene: Option<&RenderScene>) -> Vec<Measured> {
    store.iter().filter_map(|e| Some((e.id(), spots::spot_data(e)?))).map(|(id, d)| measure(store, scene, id, d)).collect()
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
        description: "Lists a view's spots (DOC-ANN-006/007) as they read now: each one's point found again on \
                      its element's surface, the element's bottom for top-and-bottom spots, its text lines, and \
                      whether it is orphaned (its element gone or no longer under its point).",
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
