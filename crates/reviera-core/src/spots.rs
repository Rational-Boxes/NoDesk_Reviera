// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Spot elevations, coordinates and slopes (spec Ch 5 §5.13, DOC-ANN-006 … 008): view annotation
//! (`anno.spot`, see `annotations`) that reads the height, the plan coordinates or the slope of a
//! point on a model element.
//!
//! - **Where:** plans, ceiling plans, sections and elevations. A spot keeps the element, the
//!   point picked on it (model coordinates) and the direction it was picked along (down in a
//!   plan, along the view in a section or elevation), so the application can find the point
//!   again on the element's surface as it is now (`reviera_app::spots`): on a sloped or moved
//!   surface the value follows. Its text stands at `text` in the view's 2D coordinates, with a
//!   leader from the point.
//! - **Elevations** are measured from the project base point, the survey point (shared
//!   height) or a level (`base`); *Display* picks the picked point, the element's top or bottom
//!   there, or both, each with its indicator text (`T`, `B`).
//! - **Coordinates** read north and east (and, when asked, the elevation) from the project base
//!   point along project north, or from the survey point in shared coordinates, turned with
//!   true north (DM-LOC-001).
//! - **Slopes** read the slope of the face at the point: in a plan its steepest, with an arrow
//!   pointing downhill; in a section or elevation along the face's line as the view sees it, as
//!   an arrow or a triangle. A vertical face has none: `spot.create_slope` (the application,
//!   which has the faces) refuses it. Percent, ratio (1 : n) or degrees.
//!
//! Not yet: spot types (`dimtype`, symbols, text styles), the Spot Dimensions settings dialog,
//! linked elements and locked 3D views.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::annotations::{ANNO_KEY, CLASS_PREFIX, is_annotation};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;

pub const KIND_SPOT: &str = "anno.spot";

/// What a spot reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpotKind {
    #[default]
    Elevation,
    Coordinate,
    Slope,
}

/// How a spot slope is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Representation {
    /// An arrow pointing downhill, the value beside it.
    #[default]
    Arrow,
    /// A right triangle along the slope (sections and elevations).
    Triangle,
}

/// The units a spot slope reads in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlopeFormat {
    /// `2.5 %`.
    #[default]
    Percent,
    /// `1 : 40` (run per unit of rise).
    Ratio,
    /// `1.43°`.
    Degrees,
}

/// Which heights a spot elevation shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Display {
    /// The picked point.
    #[default]
    Actual,
    /// The element's top there.
    Top,
    /// The element's bottom there.
    Bottom,
    /// Both, top first.
    TopAndBottom,
}

/// What values are measured from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Base {
    /// The project base point (project coordinates).
    #[default]
    Project,
    /// The survey point (shared coordinates).
    Survey,
    /// A level (GlobalId): elevations relative to it.
    Level(String),
}

/// Stored data of a spot (`anno.spot`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpotData {
    pub kind: String,
    pub spot_kind: SpotKind,
    /// The view (GlobalId) that owns the spot.
    pub view: String,
    /// The element it reads (GlobalId); `None` for a point on nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// The point picked, in model coordinates (metres).
    pub point: [f64; 3],
    /// The direction it was picked along (unit; down in plans, the view direction in sections).
    pub direction: [f64; 3],
    /// Where the text stands: the leader's shoulder, in the view's 2D coordinates.
    pub text: [f64; 2],
    #[serde(default)]
    pub base: Base,
    #[serde(default)]
    pub display: Display,
    /// Coordinates: the elevation as a third line.
    #[serde(default)]
    pub include_elevation: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    /// Slopes: arrow or triangle.
    #[serde(default)]
    pub representation: Representation,
    /// Slopes: the units.
    #[serde(default)]
    pub slope_format: SlopeFormat,
}

pub fn spot_data(e: &Element) -> Option<SpotData> {
    if !is_annotation(e) {
        return None;
    }
    let v = e.param(ANNO_KEY)?;
    if v.get("kind")?.as_str()? != KIND_SPOT {
        return None;
    }
    serde_json::from_value(v.clone()).ok()
}

/// The spots of a view (by its GlobalId).
pub fn of_view(store: &Store, view_guid: &str) -> Vec<(ElementId, SpotData)> {
    store
        .iter()
        .filter_map(|e| {
            let d = spot_data(e)?;
            (d.view == view_guid).then(|| (e.id(), d))
        })
        .collect()
}

/// A point of the model in view `view`'s 2D coordinates (GlobalId): plan x and y, or a section's
/// or elevation's x and height.
pub fn in_view(store: &Store, view: &str, p: [f64; 3]) -> [f64; 2] {
    match crate::sections::frame_of_view(store, view) {
        Some((frame, _)) => {
            let q = frame.to_local(p);
            [q[0], q[1]]
        }
        None => [p[0], p[1]],
    }
}

/// A signed elevation in project units: `+3250`, `−450`, `±0`.
fn signed(store: &Store, v: f64) -> String {
    let settings = crate::units::settings_of(store);
    let text = crate::units::format(v.abs(), "length", &settings, None).unwrap_or_else(|_| format!("{:.3}", v.abs()));
    if v.abs() < 5e-4 {
        format!("±{text}")
    } else if v > 0.0 {
        format!("+{text}")
    } else {
        format!("\u{2212}{text}")
    }
}

/// The height of a model point measured from `base`.
pub fn elevation(store: &Store, base: &Base, p: [f64; 3]) -> f64 {
    let loc = crate::location::get(store);
    match base {
        Base::Project => loc.project(p)[2],
        Base::Survey => loc.shared(p)[2],
        Base::Level(guid) => p[2] - crate::levels::elevation_of(store, guid).unwrap_or(0.0),
    }
}

/// A spot's text, one line per value: elevations at `top` (the picked point, or the element's
/// top) and, for *Top and Bottom* or *Bottom*, `bottom`; coordinates of `top`.
pub fn lines(store: &Store, d: &SpotData, top: [f64; 3], bottom: Option<[f64; 3]>) -> Vec<String> {
    let wrap = |s: String| format!("{}{s}{}", d.prefix, d.suffix);
    match d.spot_kind {
        SpotKind::Elevation => {
            let at = |p: [f64; 3]| signed(store, elevation(store, &d.base, p));
            match (d.display, bottom) {
                (Display::Actual, _) => vec![wrap(at(top))],
                (Display::Top, _) => vec![wrap(format!("T {}", at(top)))],
                (Display::Bottom, b) => vec![wrap(format!("B {}", at(b.unwrap_or(top))))],
                (Display::TopAndBottom, b) => vec![wrap(format!("T {}", at(top))), wrap(format!("B {}", at(b.unwrap_or(top))))],
            }
        }
        // The application reads a slope from its face (`slope_lines`).
        SpotKind::Slope => Vec::new(),
        SpotKind::Coordinate => {
            let loc = crate::location::get(store);
            let c = match d.base {
                Base::Survey => loc.shared(top),
                _ => loc.project(top),
            };
            let settings = crate::units::settings_of(store);
            let len = |v: f64| crate::units::format(v, "length", &settings, None).unwrap_or_else(|_| format!("{v:.3}"));
            let mut out = vec![wrap(format!("N {}", len(c[1]))), wrap(format!("E {}", len(c[0])))];
            if d.include_elevation {
                out.push(wrap(format!("EL {}", signed(store, elevation(store, &d.base, top)))));
            }
            out
        }
    }
}

/// A slope (rise over run) as a spot slope reads it: `0 %` when flat.
pub fn slope_lines(d: &SpotData, slope: f64) -> Vec<String> {
    let trim = |v: f64, places: usize| {
        let t = format!("{v:.places$}");
        if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_owned() } else { t }
    };
    let text = match d.slope_format {
        SlopeFormat::Percent => format!("{} %", trim(slope * 100.0, 1)),
        SlopeFormat::Ratio if slope < 1e-9 => "0".to_owned(),
        SlopeFormat::Ratio => format!("1 : {}", trim(1.0 / slope, 1)),
        SlopeFormat::Degrees => format!("{}°", trim(slope.atan().to_degrees(), 2)),
    };
    vec![format!("{}{text}{}", d.prefix, d.suffix)]
}

// ---- Commands ----

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (l > 1e-9).then(|| v.map(|c| c / l))
}

/// A view a spot can go in: a plan, section or elevation (GlobalId), and the default direction
/// to pick along there.
fn spot_view(store: &Store, r: &ElementRef) -> Result<(String, [f64; 3])> {
    let (id, data) = crate::views::resolve_view(store, r)?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    if crate::plans::is_plan(&data.kind) {
        return Ok((guid, [0.0, 0.0, -1.0]));
    }
    match crate::sections::frame_of_view(store, &guid) {
        Some((frame, _)) => Ok((guid, [frame.look[0], frame.look[1], 0.0])),
        None => Err(Error::new("anno.view_not_supported", "spots are placed in plans, sections and elevations")),
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSpotParams {
    /// A floor or ceiling plan, a section or an elevation.
    pub view_id: ElementRef,
    /// The element whose surface the point is on.
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub element_ref: Option<ElementRef>,
    /// The point on it, in model coordinates (metres).
    pub point: [f64; 3],
    /// Where the text goes (the leader's shoulder), in the view's 2D coordinates; default a
    /// little up and to the right of the point.
    pub leader_point: Option<[f64; 2]>,
    /// The direction the point was picked along; default down in plans, along the view in
    /// sections and elevations.
    pub direction: Option<[f64; 3]>,
    /// `project` (default), `survey`, or `{"level": <level>}`.
    pub base: Option<BaseArg>,
    /// Spot elevations: `actual` (default), `top`, `bottom`, `top_and_bottom`.
    pub display: Option<Display>,
    /// Spot coordinates: the elevation as a third line.
    pub include_elevation: Option<bool>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    /// Spot slopes: `arrow` (default) or `triangle` (sections and elevations).
    pub representation: Option<Representation>,
    /// Spot slopes: `percent` (default), `ratio` or `degrees`.
    pub slope_format: Option<SlopeFormat>,
}

/// A `base` argument: `project`, `survey`, or a level.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BaseArg {
    Project,
    Survey,
    Level(ElementRef),
}

fn base_of(store: &Store, a: Option<BaseArg>) -> Result<Base> {
    Ok(match a {
        None | Some(BaseArg::Project) => Base::Project,
        Some(BaseArg::Survey) => Base::Survey,
        Some(BaseArg::Level(r)) => {
            let e = store
                .resolve(&r)
                .and_then(|id| store.get(id))
                .filter(|e| e.class == crate::levels::CLASS)
                .ok_or_else(|| Error::new("level.not_found", "no such level"))?;
            Base::Level(e.guid().to_string())
        }
    })
}

fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateSpotParams, kind: SpotKind) -> Result<ElementId> {
    let store = txn.store().clone();
    let (view, default_dir) = spot_view(&store, &p.view_id)?;
    let element = match &p.element_ref {
        Some(r) => {
            let e = store.resolve(r).and_then(|id| store.get(id)).ok_or_else(|| Error::new("element.not_found", "no such element for the spot"))?;
            if is_annotation(e) || crate::views::view_data(e).is_some() {
                return Err(Error::new("spot.bad_ref", "a spot reads a point on a model element"));
            }
            Some(e.guid().to_string())
        }
        None => None,
    };
    if p.point.iter().any(|c| !c.is_finite()) {
        return Err(Error::new("spot.bad_point", "the point must be finite"));
    }
    let direction = match p.direction {
        Some(d) => unit(d).ok_or_else(|| Error::new("spot.bad_direction", "the direction must not be zero"))?,
        None => default_dir,
    };
    let at = in_view(&store, &view, p.point);
    // 3 mm and 4 mm on paper at the view's scale.
    let scale = crate::views::resolve_view(&store, &p.view_id)
        .ok()
        .and_then(|(id, _)| store.get(id)?.param(crate::views::VIEW_KEY)?.get("scale")?.as_f64())
        .unwrap_or(100.0);
    // A slope's value stands just above its arrow; the others' text up and to the right.
    let text = p.leader_point.unwrap_or(match kind {
        SpotKind::Slope => [at[0], at[1] + 0.003 * scale],
        _ => [at[0] + 0.003 * scale, at[1] + 0.004 * scale],
    });
    if p.representation == Some(Representation::Triangle) && crate::sections::frame_of_view(&store, &view).is_none() {
        return Err(Error::new("spot.bad_option", "the triangle form is for sections and elevations"));
    }
    let data = SpotData {
        kind: KIND_SPOT.into(),
        spot_kind: kind,
        view: view.clone(),
        element,
        point: p.point,
        direction,
        text,
        base: base_of(&store, p.base)?,
        display: p.display.unwrap_or_default(),
        include_elevation: p.include_elevation.unwrap_or(false),
        prefix: p.prefix.unwrap_or_default(),
        suffix: p.suffix.unwrap_or_default(),
        representation: p.representation.unwrap_or_default(),
        slope_format: p.slope_format.unwrap_or_default(),
    };
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{view}/{}", guid.as_str()))
            .guid(guid)
            .param(ANNO_KEY, serde_json::to_value(&data).expect("serialises")),
    )
}

/// Refuses the options of other kinds of spot.
fn only(p: &CreateSpotParams, kind: SpotKind) -> Result<()> {
    let wrong = match kind {
        SpotKind::Elevation => p.include_elevation.is_some() || p.representation.is_some() || p.slope_format.is_some(),
        SpotKind::Coordinate => p.display.is_some() || p.representation.is_some() || p.slope_format.is_some(),
        SpotKind::Slope => p.display.is_some() || p.include_elevation.is_some() || p.base.is_some(),
    };
    if wrong {
        return Err(Error::new("spot.bad_option", "display is for spot elevations, include_elevation for coordinates, representation and slope_format for slopes, base not for slopes"));
    }
    Ok(())
}

pub(crate) fn create_elevation(txn: &mut crate::txn::Transaction<'_>, p: CreateSpotParams) -> Result<ElementId> {
    only(&p, SpotKind::Elevation)?;
    create(txn, p, SpotKind::Elevation)
}

pub(crate) fn create_coordinate(txn: &mut crate::txn::Transaction<'_>, p: CreateSpotParams) -> Result<ElementId> {
    only(&p, SpotKind::Coordinate)?;
    create(txn, p, SpotKind::Coordinate)
}

/// `spot.put_slope`: stores a spot slope as given, unchecked against the model's faces
/// (`spot.create_slope` checks it first).
pub(crate) fn put_slope(txn: &mut crate::txn::Transaction<'_>, p: CreateSpotParams) -> Result<ElementId> {
    only(&p, SpotKind::Slope)?;
    create(txn, p, SpotKind::Slope)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSpotParams {
    pub annotation_ids: Vec<ElementRef>,
    pub base: Option<BaseArg>,
    pub display: Option<Display>,
    pub include_elevation: Option<bool>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    /// Moves the text (the leader's shoulder), in the view's 2D coordinates.
    pub leader_point: Option<[f64; 2]>,
    pub representation: Option<Representation>,
    pub slope_format: Option<SlopeFormat>,
}

/// `spot.set`: changes spots' base, display, prefix and suffix, or moves their text.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetSpotParams) -> Result<()> {
    let store = txn.store().clone();
    let base = p.base.map(|b| base_of(&store, Some(b))).transpose()?;
    for r in &p.annotation_ids {
        let id = store.resolve(r).ok_or_else(|| Error::new("spot.not_found", "no such spot"))?;
        let mut d = spot_data(store.get(id).expect("resolved")).ok_or_else(|| Error::new("spot.not_found", format!("element {id} is not a spot")))?;
        if let Some(b) = &base {
            d.base = b.clone();
        }
        if let Some(v) = p.display {
            if d.spot_kind != SpotKind::Elevation {
                return Err(Error::new("spot.bad_option", "display is for spot elevations"));
            }
            d.display = v;
        }
        if let Some(v) = p.include_elevation {
            d.include_elevation = v;
        }
        if let Some(v) = &p.prefix {
            d.prefix = v.clone();
        }
        if let Some(v) = &p.suffix {
            d.suffix = v.clone();
        }
        if let Some(v) = p.leader_point {
            d.text = v;
        }
        if p.representation.is_some() || p.slope_format.is_some() {
            if d.spot_kind != SpotKind::Slope {
                return Err(Error::new("spot.bad_option", "representation and slope_format are for spot slopes"));
            }
            if p.representation == Some(Representation::Triangle) && crate::sections::frame_of_view(&store, &d.view).is_none() {
                return Err(Error::new("spot.bad_option", "the triangle form is for sections and elevations"));
            }
            d.representation = p.representation.unwrap_or(d.representation);
            d.slope_format = p.slope_format.unwrap_or(d.slope_format);
        }
        let value = serde_json::to_value(&d).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(ANNO_KEY.into(), value);
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(kind: SpotKind, base: Base, display: Display) -> SpotData {
        SpotData {
            kind: KIND_SPOT.into(),
            spot_kind: kind,
            view: String::new(),
            element: None,
            point: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
            text: [0.0; 2],
            base,
            display,
            include_elevation: true,
            prefix: String::new(),
            suffix: String::new(),
            representation: Representation::Arrow,
            slope_format: SlopeFormat::Percent,
        }
    }

    #[test]
    fn values_read_from_the_chosen_base_with_their_indicators() {
        let store = Store::default();
        let d = data(SpotKind::Elevation, Base::Project, Display::TopAndBottom);
        // Metres in a model without unit settings: three decimals, signed.
        let l = lines(&store, &d, [1.0, 2.0, 3.25], Some([1.0, 2.0, 3.0]));
        assert_eq!(l.len(), 2);
        assert!(l[0].starts_with("T +") && l[1].starts_with("B +"), "{l:?}");
        let zero = lines(&store, &data(SpotKind::Elevation, Base::Project, Display::Actual), [0.0; 3], None);
        assert!(zero[0].starts_with('±'), "{zero:?}");
        let below = lines(&store, &data(SpotKind::Elevation, Base::Project, Display::Actual), [0.0, 0.0, -0.45], None);
        assert!(below[0].starts_with('\u{2212}'), "{below:?}");
        let c = lines(&store, &data(SpotKind::Coordinate, Base::Project, Display::Actual), [4.0, 7.0, 1.0], None);
        assert_eq!(c.len(), 3);
        assert!(c[0].starts_with("N ") && c[1].starts_with("E ") && c[2].starts_with("EL +"), "{c:?}");
        let mut s = data(SpotKind::Slope, Base::Project, Display::Actual);
        assert_eq!(slope_lines(&s, 0.02), ["2 %"]);
        assert_eq!(slope_lines(&s, 0.0), ["0 %"]);
        s.slope_format = SlopeFormat::Ratio;
        assert_eq!(slope_lines(&s, 0.025), ["1 : 40"]);
        s.slope_format = SlopeFormat::Degrees;
        assert_eq!(slope_lines(&s, 1.0), ["45°"]);
    }
}
