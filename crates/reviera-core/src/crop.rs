// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Crop regions and annotation crop (spec Ch 5, DOC-GFX-015): a plan's, section's or
//! elevation's *Crop View* limits what it draws to a rectangle in the view's 2D coordinates (plan
//! x and y; a section's x and height). *Crop Region Visible* draws the rectangle. The *annotation
//! crop* is a second rectangle outside it, by four offsets in paper millimetres: annotation
//! crossing it is clipped and annotation entirely outside it hidden. A tag of an element outside
//! the model crop is hidden wherever the tag is.
//!
//! Kept in the view's payload: `crop` `{enabled, visible, min, max}` and `annotationCrop`
//! `{enabled, offsets: [left, right, bottom, top]}`. The drawing is clipped by
//! `reviera_render::crop`, in the window and in output alike.
//!
//! A plan assigned to a scope box (ARC-SCOPE-001) is cropped to the box's footprint (its bounding
//! rectangle while crops cannot turn), on unless its crop has been switched off; its region then
//! cannot be set by hand.
//!
//! Not yet: non-rectangular crops (*Edit Crop*), crop breaks, rotation, dragging the edges in
//! the window.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::ElementRef;
use crate::store::Store;
use crate::txn::Transaction;
use crate::views::VIEW_KEY;

/// A view's model crop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Crop {
    pub enabled: bool,
    pub visible: bool,
    /// The rectangle's corners, in the view's 2D coordinates (metres).
    pub min: [f64; 2],
    pub max: [f64; 2],
}

/// A view's annotation crop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnnotationCrop {
    pub enabled: bool,
    /// Left, right, bottom and top, outside the model crop (paper millimetres).
    pub offsets: [f64; 4],
}

impl Default for AnnotationCrop {
    fn default() -> Self {
        Self { enabled: false, offsets: [5.0; 4] }
    }
}

/// What a view's crop clips its drawing to: the model crop's rectangle, and the annotation's
/// (the model crop grown by the offsets at the view's scale, or none when off).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropRects {
    pub model: ([f64; 2], [f64; 2]),
    pub annotation: Option<([f64; 2], [f64; 2])>,
}

impl CropRects {
    pub fn contains(&self, p: [f64; 2]) -> bool {
        let (lo, hi) = self.model;
        p[0] >= lo[0] - 1e-9 && p[0] <= hi[0] + 1e-9 && p[1] >= lo[1] - 1e-9 && p[1] <= hi[1] + 1e-9
    }
}

pub fn crop_of(e: &Element) -> Option<Crop> {
    serde_json::from_value(e.param(VIEW_KEY)?.get("crop")?.clone()).ok()
}

pub fn annotation_crop_of(e: &Element) -> AnnotationCrop {
    e.param(VIEW_KEY)
        .and_then(|v| v.get("annotationCrop"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn scale_of(e: &Element) -> f64 {
    e.param(VIEW_KEY).and_then(|v| v.get("scale")?.as_f64()).unwrap_or(100.0)
}

/// The scope box a view follows, if it has one that exists.
fn scope_box(store: &Store, e: &Element) -> Option<crate::scope_boxes::ScopeBox> {
    if !croppable(e) {
        return None;
    }
    let guid = e.param(crate::scope_boxes::VIEW_KEY)?.as_str()?;
    let b = store.get(store.find_by_guid(&crate::id::GlobalId::parse(guid)?)?)?;
    crate::scope_boxes::recipe(b)
}

/// The crop a view uses: its own, or (for a view assigned to a scope box) the rectangle round
/// the box as the view sees it (a plan its footprint, a section or elevation the box seen from
/// the side, from its bottom to its top), on unless the view's crop was switched off.
pub fn effective(store: &Store, e: &Element) -> Option<Crop> {
    let own = crop_of(e);
    let Some(b) = scope_box(store, e) else { return own };
    let frame = crate::sections::section_of(e).map(|(_, d)| d.frame());
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for c in b.corners() {
        for z in [b.bottom, b.bottom + b.height] {
            let q = match &frame {
                Some(f) => {
                    let l = f.to_local([c[0], c[1], z]);
                    [l[0], l[1]]
                }
                None => c,
            };
            lo = [lo[0].min(q[0]), lo[1].min(q[1])];
            hi = [hi[0].max(q[0]), hi[1].max(q[1])];
        }
    }
    Some(Crop { enabled: own.is_none_or(|c| c.enabled), visible: own.is_some_and(|c| c.visible), min: lo, max: hi })
}

/// Whether a view's crop follows a scope box.
pub fn follows_scope_box(store: &Store, e: &Element) -> bool {
    scope_box(store, e).is_some()
}

/// The rectangles a view is cropped to, when its crop is on.
pub fn rects(store: &Store, e: &Element) -> Option<CropRects> {
    let c = effective(store, e).filter(|c| c.enabled)?;
    let a = annotation_crop_of(e);
    let mm = scale_of(e) / 1000.0;
    let annotation = a.enabled.then(|| {
        let o = a.offsets.map(|v| v * mm);
        ([c.min[0] - o[0], c.min[1] - o[2]], [c.max[0] + o[1], c.max[1] + o[3]])
    });
    Some(CropRects { model: (c.min, c.max), annotation })
}

/// Whether a view kind can be cropped.
fn croppable(e: &Element) -> bool {
    let kind = crate::views::view_data(e).map(|d| d.kind).unwrap_or_default();
    crate::plans::is_plan(&kind) || crate::sections::is_section(&kind) || kind == crate::areas::KIND_AREA_PLAN
}

/// The model's extent as view `e` sees it, in its 2D coordinates, with a 1 m margin: the bodies of
/// Reviera's elements (and, in a section or elevation, only what lies in its depth and width).
pub fn model_extent(store: &Store, e: &Element) -> Option<([f64; 2], [f64; 2])> {
    let frame = crate::sections::section_of(e).map(|(_, d)| d.frame());
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    for b in crate::bodies::all(store) {
        for part in &b.parts {
            for p in &part.mesh.0 {
                let q = match &frame {
                    Some(f) => {
                        let l = f.to_local(*p);
                        if l[2] > 1e-6 || l[2] < -f.far || l[0] < 0.0 || l[0] > f.width {
                            continue;
                        }
                        [l[0], l[1]]
                    }
                    None => [p[0], p[1]],
                };
                lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                hi = [hi[0].max(q[0]), hi[1].max(q[1])];
            }
        }
    }
    (lo[0] <= hi[0]).then(|| ([lo[0] - 1.0, lo[1] - 1.0], [hi[0] + 1.0, hi[1] + 1.0]))
}

fn resolve(store: &Store, r: &ElementRef) -> Result<crate::id::ElementId> {
    let id = store.resolve(r).ok_or_else(|| Error::new("view.not_found", "no such view"))?;
    let e = store.get(id).expect("resolved");
    if !croppable(e) {
        return Err(Error::new("view.not_croppable", "plans, sections and elevations can be cropped"));
    }
    Ok(id)
}

fn write(txn: &mut Transaction<'_>, id: crate::id::ElementId, key: &str, value: Value) -> Result<()> {
    txn.modify(id, |e| {
        if let Some(Value::Object(o)) = e.params.get_mut(VIEW_KEY) {
            o.insert(key.to_owned(), value);
        }
    })
}

/// A rectangle argument.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    /// One corner, in the view's 2D coordinates (metres).
    pub min: [f64; 2],
    /// The opposite corner.
    pub max: [f64; 2],
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCropParams {
    pub view_id: ElementRef,
    /// Crop View: the view draws only what lies inside the crop region.
    pub enabled: Option<bool>,
    /// Crop Region Visible: the rectangle is drawn.
    pub visible: Option<bool>,
    /// The crop region; turning the crop on without one fits the model.
    pub shape: Option<Rect>,
}

/// `view.set_crop`.
pub(crate) fn set_crop(txn: &mut Transaction<'_>, p: SetCropParams) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.view_id)?;
    let e = store.get(id).expect("resolved");
    if p.shape.is_some() && follows_scope_box(&store, e) {
        return Err(Error::new("view.crop_follows_scope_box", "the view's crop follows its scope box; move or resize the box"));
    }
    let current = effective(&store, e);
    let (min, max) = match (p.shape, current) {
        (Some(r), _) => {
            let (lo, hi) = ([r.min[0].min(r.max[0]), r.min[1].min(r.max[1])], [r.min[0].max(r.max[0]), r.min[1].max(r.max[1])]);
            if hi[0] - lo[0] < 0.01 || hi[1] - lo[1] < 0.01 {
                return Err(Error::new("view.bad_crop", "the crop region must be at least 10 mm each way"));
            }
            (lo, hi)
        }
        (None, Some(c)) => (c.min, c.max),
        (None, None) => model_extent(&store, e).unwrap_or(([-10.0, -10.0], [10.0, 10.0])),
    };
    let c = Crop {
        enabled: p.enabled.or(current.map(|c| c.enabled)).unwrap_or(true),
        visible: p.visible.or(current.map(|c| c.visible)).unwrap_or(true),
        min,
        max,
    };
    write(txn, id, "crop", serde_json::to_value(c).expect("serialises"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResetCropParams {
    pub view_id: ElementRef,
}

/// `view.reset_crop`: the crop region fits the model again (as the view sees it).
pub(crate) fn reset_crop(txn: &mut Transaction<'_>, p: ResetCropParams) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.view_id)?;
    let e = store.get(id).expect("resolved");
    if follows_scope_box(&store, e) {
        return Err(Error::new("view.crop_follows_scope_box", "the view's crop follows its scope box; move or resize the box"));
    }
    let (min, max) = model_extent(&store, e).ok_or_else(|| Error::new("view.empty", "the view shows nothing to fit"))?;
    let current = crop_of(e);
    let c = Crop { enabled: current.is_none_or(|c| c.enabled), visible: current.is_none_or(|c| c.visible), min, max };
    write(txn, id, "crop", serde_json::to_value(c).expect("serialises"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetAnnotationCropParams {
    pub view_id: ElementRef,
    pub enabled: bool,
    /// Left, right, bottom and top, outside the crop region (paper millimetres, 0–100).
    pub offsets: Option<[f64; 4]>,
}

/// `view.set_annotation_crop`.
pub(crate) fn set_annotation_crop(txn: &mut Transaction<'_>, p: SetAnnotationCropParams) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.view_id)?;
    let mut a = annotation_crop_of(store.get(id).expect("resolved"));
    a.enabled = p.enabled;
    if let Some(o) = p.offsets {
        if o.iter().any(|v| !(0.0..=100.0).contains(v)) {
            return Err(Error::new("view.bad_offsets", "annotation crop offsets are 0 to 100 mm on paper"));
        }
        a.offsets = o;
    }
    write(txn, id, "annotationCrop", serde_json::to_value(a).expect("serialises"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SizeCropParams {
    pub view_id: ElementRef,
    /// The crop region's width (metres in the model); kept when absent.
    pub width: Option<f64>,
    /// Its height (metres in the model); kept when absent.
    pub height: Option<f64>,
    /// The annotation crop's offsets outside the region (left, right, bottom, top; paper
    /// millimetres, 0–100); kept when absent.
    pub annotation_offsets: Option<[f64; 4]>,
}

/// `view.size_crop` (*Size Crop*): the crop region resized about its centre, and the annotation
/// crop's offsets, in one step.
pub(crate) fn size_crop(txn: &mut Transaction<'_>, p: SizeCropParams) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(&store, &p.view_id)?;
    let e = store.get(id).expect("resolved");
    let current = effective(&store, e);
    if p.width.is_some() || p.height.is_some() {
        if follows_scope_box(&store, e) {
            return Err(Error::new("view.crop_follows_scope_box", "the view's crop follows its scope box; move or resize the box"));
        }
        let (lo, hi) = current.map(|c| (c.min, c.max)).or_else(|| model_extent(&store, e)).unwrap_or(([-10.0, -10.0], [10.0, 10.0]));
        let size = [p.width.unwrap_or(hi[0] - lo[0]), p.height.unwrap_or(hi[1] - lo[1])];
        if size.iter().any(|s| !s.is_finite() || *s < 0.01) {
            return Err(Error::new("view.bad_crop", "the crop region must be at least 10 mm each way"));
        }
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let crop = Crop {
            enabled: current.is_none_or(|c| c.enabled),
            visible: current.is_none_or(|c| c.visible),
            min: [c[0] - size[0] / 2.0, c[1] - size[1] / 2.0],
            max: [c[0] + size[0] / 2.0, c[1] + size[1] / 2.0],
        };
        write(txn, id, "crop", serde_json::to_value(crop).expect("serialises"))?;
    }
    if let Some(o) = p.annotation_offsets {
        let enabled = annotation_crop_of(e).enabled;
        set_annotation_crop(txn, SetAnnotationCropParams { view_id: p.view_id, enabled, offsets: Some(o) })?;
    }
    Ok(())
}
