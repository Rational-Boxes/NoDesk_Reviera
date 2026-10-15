// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Property lines (spec Ch 3 §3.11.10, ARC-SITE-001): a site's legal boundary, sketched or
//! entered as distances and bearings.
//!
//! A property line is an `IfcAnnotation` (`ObjectType = "PropertyLine"`) with
//! `recipe.property_line`: its points in plan, and, when entered as a table, the rows
//! (distance, bearing, line or arc with radius and side) from an origin. Bearings read like
//! `N 45°30'00" E` (from north or south toward east or west) or as a plain azimuth in degrees
//! clockwise from north; north is the project's (true north is not applied yet). A closed line
//! reports its area.
//!
//! Not built yet: true north, property-line tags, Convert to table, `IfcSite` containment.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const RECIPE: &str = "recipe.property_line";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Line,
    Arc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub enum Side {
    #[default]
    L,
    R,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Row {
    /// Metres (the chord, for an arc).
    pub distance: f64,
    /// `N 45°30'00" E`, or an azimuth in degrees clockwise from north.
    pub bearing: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// An arc bulges to the left or right of its chord.
    #[serde(default)]
    pub side: Side,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PropertyLineRecipe {
    pub kind: String,
    pub points: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub table: Vec<Row>,
    #[serde(default)]
    pub origin: [f64; 2],
}

pub fn recipe(e: &Element) -> Option<PropertyLineRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A bearing's direction (unit vector, x east, y north).
pub fn bearing(text: &str) -> Result<[f64; 2]> {
    let t = text.trim().to_ascii_uppercase();
    let bad = || Error::new("propertyline.bearing", format!("`{text}` is not a bearing"));
    let azimuth = if let Ok(a) = t.trim_end_matches('°').trim().parse::<f64>() {
        a
    } else {
        let from = t.chars().next().ok_or_else(bad)?;
        let to = t.chars().last().ok_or_else(bad)?;
        if !matches!(from, 'N' | 'S') || !matches!(to, 'E' | 'W') {
            return Err(bad());
        }
        let inner = &t[1..t.len() - 1];
        // Degrees, minutes, seconds: the numbers in order.
        let nums: Vec<f64> = inner
            .split(|c: char| !(c.is_ascii_digit() || c == '.'))
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<f64>().map_err(|_| bad()))
            .collect::<Result<_>>()?;
        let angle = nums.first().copied().unwrap_or(0.0)
            + nums.get(1).copied().unwrap_or(0.0) / 60.0
            + nums.get(2).copied().unwrap_or(0.0) / 3600.0;
        match (from, to) {
            ('N', 'E') => angle,
            ('S', 'E') => 180.0 - angle,
            ('S', 'W') => 180.0 + angle,
            _ => 360.0 - angle,
        }
    };
    let a = azimuth.to_radians();
    Ok([a.sin(), a.cos()])
}

/// The points of a table from an origin (arcs sampled).
pub fn table_points(origin: [f64; 2], rows: &[Row]) -> Result<Vec<[f64; 2]>> {
    let mut pts = vec![origin];
    let mut at = origin;
    for r in rows {
        let d = bearing(&r.bearing)?;
        let end = [at[0] + d[0] * r.distance, at[1] + d[1] * r.distance];
        if r.kind == Kind::Arc {
            let radius = r.radius.unwrap_or(0.0);
            let half = r.distance / 2.0;
            if radius < half {
                return Err(Error::new(
                    "propertyline.radius",
                    "an arc's radius must be at least half its chord",
                ));
            }
            let mid = [(at[0] + end[0]) / 2.0, (at[1] + end[1]) / 2.0];
            // Left of the chord: (−dy, dx).
            let left = [-d[1], d[0]];
            let s = if r.side == Side::L { 1.0 } else { -1.0 };
            let sag = radius - (radius * radius - half * half).sqrt();
            let through = [mid[0] + left[0] * sag * s, mid[1] + left[1] * sag * s];
            let line = crate::walls::Line { start: at, end };
            pts.extend(crate::sketch::interior(line, &[through]));
        }
        pts.push(end);
        at = end;
    }
    Ok(pts)
}

/// Whether the line closes, and its area if so.
pub fn area(r: &PropertyLineRecipe) -> Option<f64> {
    let (a, b) = (r.points.first()?, r.points.last()?);
    if (a[0] - b[0]).hypot(a[1] - b[1]) > 0.001 {
        return None;
    }
    Some(crate::polygons::area(&r.points[..r.points.len() - 1]).abs())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// Sketched: its points (repeat the first to close it) …
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    /// … or a table of distances and bearings from `origin`.
    #[serde(default)]
    pub table: Vec<Row>,
    #[serde(default)]
    pub origin: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Created {
    pub id: ElementId,
    /// The area if it closes (m²).
    pub area: Option<f64>,
}

/// `propertyline.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Created> {
    let points = if p.table.is_empty() {
        p.points
    } else {
        table_points(p.origin, &p.table)?
    };
    if points.len() < 2 {
        return Err(Error::new(
            "propertyline.empty",
            "a property line needs two points",
        ));
    }
    let r = PropertyLineRecipe {
        kind: RECIPE.into(),
        points,
        table: p.table,
        origin: p.origin,
    };
    let a = area(&r);
    if a.is_none() {
        txn.warn(crate::txn::Warning {
            id: "propertyline.open".into(),
            message: "The property line is not closed: no area can be computed".into(),
            element_ids: Vec::new(),
        });
    }
    let id = txn.create(
        NewElement::new("IfcAnnotation")
            .name("Property Line")
            .param("@ObjectType", "PropertyLine")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    Ok(Created { id, area: a })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AreaParams {
    pub element_id: ElementRef,
}

/// `propertyline.area`.
pub(crate) fn area_of(store: &Store, p: AreaParams) -> Result<Option<f64>> {
    let id = store
        .resolve(&p.element_id)
        .ok_or_else(|| Error::new("element.not_found", "no such property line"))?;
    let r = store
        .get(id)
        .and_then(recipe)
        .ok_or_else(|| Error::new("propertyline.not", "not a property line"))?;
    Ok(area(&r))
}

/// Moves, turns or mirrors a property line.
pub(crate) fn transform(
    txn: &mut Transaction<'_>,
    id: ElementId,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = recipe(store.get(id).expect("exists")).expect("a property line");
    r.points = r.points.iter().map(|p| point(*p)).collect();
    r.origin = point(r.origin);
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}
