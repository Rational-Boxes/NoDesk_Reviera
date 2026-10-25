// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Project location, coordinates and true north (spec Ch 2 §2.10.7, DM-LOC-001), kept in RXD on
//! the project (`rxd:location`, document `project/location`).
//!
//! - **Location**: latitude and longitude (degrees, north and east positive), elevation above
//!   sea level (metres), time zone (hours from UTC) and daylight saving.
//! - Three reference points: the **internal origin** (fixed, never moved), the **project base
//!   point** (a point in internal coordinates; project coordinates are measured from it, along
//!   project north) and the **survey point** (a point in internal coordinates with its shared,
//!   real-world coordinates: easting, northing, height).
//! - **True north**: the angle from project north (+Y) to true north, counter-clockwise
//!   positive, in degrees. Shared coordinates turn with it: shared = R(−θ)·(p − survey) +
//!   (E, N), height = z − survey z + H. Rotating true north changes no element.
//! - The **CRS** is named by an EPSG code (`EPSG:25832`); its form is checked, not its
//!   existence (no CRS database is bundled).
//! - In IFC (on save): `IfcSite.RefLatitude`, `RefLongitude` (compound plane angles) and
//!   `RefElevation`; the model context's `TrueNorth`; with a CRS and a survey point, an
//!   `IfcMapConversion` (Eastings, Northings, OrthogonalHeight of the internal origin, the local
//!   X axis as XAxisAbscissa/Ordinate, Scale 1) to an `IfcProjectedCRS`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::store::Store;
use crate::txn::Transaction;

pub const KEY: &str = "rxd:location";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A point in internal coordinates with its shared coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurveyPoint {
    /// Where it is, in internal coordinates (metres).
    pub point: [f64; 3],
    /// Easting, northing and height there (metres).
    pub shared: [f64; 3],
}

/// The project's place on Earth and its coordinate systems.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Location {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// Metres above sea level.
    pub elevation: f64,
    /// Hours from UTC.
    pub time_zone: Option<f64>,
    pub daylight_saving: bool,
    /// A place name (city), for display.
    pub place: Option<String>,
    /// The project base point in internal coordinates.
    pub project_base_point: [f64; 3],
    pub survey_point: SurveyPoint,
    /// Degrees from project north to true north, counter-clockwise.
    pub true_north: f64,
    /// `EPSG:nnnn`.
    pub crs: Option<String>,
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub fn get(store: &Store) -> Location {
    project(store).and_then(|p| p.param(KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

fn put(txn: &mut Transaction<'_>, l: &Location) -> Result<()> {
    let p = project(txn.store()).map(Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(l).expect("serialises");
    txn.modify(p, |e| {
        e.params.insert(KEY.into(), v);
    })
}

/// Rotates a plan vector by `deg` degrees counter-clockwise.
fn rot(v: [f64; 2], deg: f64) -> [f64; 2] {
    let (s, c) = deg.to_radians().sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

impl Location {
    /// Shared coordinates (easting, northing, height) of an internal point.
    pub fn shared(&self, p: [f64; 3]) -> [f64; 3] {
        let s = self.survey_point;
        let d = rot([p[0] - s.point[0], p[1] - s.point[1]], -self.true_north);
        [d[0] + s.shared[0], d[1] + s.shared[1], p[2] - s.point[2] + s.shared[2]]
    }

    /// Project coordinates of an internal point: from the project base point, along project
    /// north.
    pub fn project(&self, p: [f64; 3]) -> [f64; 3] {
        let b = self.project_base_point;
        [p[0] - b[0], p[1] - b[1], p[2] - b[2]]
    }

    /// True north as a direction in internal coordinates.
    pub fn true_north_direction(&self) -> [f64; 2] {
        rot([0.0, 1.0], self.true_north)
    }

    /// The map conversion: the internal origin's easting, northing and height, and the local X
    /// axis in map coordinates.
    pub fn map_conversion(&self) -> ([f64; 3], [f64; 2]) {
        (self.shared([0.0; 3]), rot([1.0, 0.0], -self.true_north))
    }
}

/// An angle in degrees as IFC's compound measure: degrees, minutes, seconds, millionths of a
/// second, all of the angle's sign.
pub fn compound(deg: f64) -> [i64; 4] {
    let sign = if deg < 0.0 { -1 } else { 1 };
    let micro = (deg.abs() * 3600.0 * 1e6).round() as i64;
    let (d, rest) = (micro / 3_600_000_000, micro % 3_600_000_000);
    let (m, rest) = (rest / 60_000_000, rest % 60_000_000);
    let (s, u) = (rest / 1_000_000, rest % 1_000_000);
    [sign * d, sign * m, sign * s, sign * u]
}

/// The degrees of a compound measure.
pub fn from_compound(c: &[i64]) -> f64 {
    let part = |k: usize| c.get(k).copied().unwrap_or(0) as f64;
    part(0) + part(1) / 60.0 + part(2) / 3600.0 + part(3) / 3.6e9
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    /// Degrees, −90 to 90 (north positive).
    pub lat: f64,
    /// Degrees, −180 to 180 (east positive).
    pub lon: f64,
    /// Metres above sea level.
    pub elevation: Option<f64>,
    /// Hours from UTC, −12 to 14.
    pub time_zone: Option<f64>,
    pub dst: Option<bool>,
    pub place: Option<String>,
}

/// `location.set`.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<Location> {
    if !(-90.0..=90.0).contains(&p.lat) || !(-180.0..=180.0).contains(&p.lon) {
        return Err(bad("location.bad", "latitude runs from −90 to 90 and longitude from −180 to 180 degrees"));
    }
    if p.time_zone.is_some_and(|z| !(-12.0..=14.0).contains(&z)) {
        return Err(bad("location.bad", "the time zone runs from −12 to +14 hours"));
    }
    if p.elevation.is_some_and(|e| !e.is_finite()) {
        return Err(bad("location.bad", "the elevation is a number of metres"));
    }
    let mut l = get(txn.store());
    l.latitude = Some(p.lat);
    l.longitude = Some(p.lon);
    if let Some(e) = p.elevation {
        l.elevation = e;
    }
    if let Some(z) = p.time_zone {
        l.time_zone = Some(z);
    }
    if let Some(d) = p.dst {
        l.daylight_saving = d;
    }
    if let Some(n) = p.place {
        l.place = (!n.trim().is_empty()).then(|| n.trim().to_owned());
    }
    put(txn, &l)?;
    Ok(l)
}

fn finite(v: &[f64], what: &str) -> Result<()> {
    if v.iter().all(|x| x.is_finite()) { Ok(()) } else { Err(bad("coordinates.bad", format!("the {what} has a value that is not a number"))) }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurveyParams {
    /// Internal coordinates (metres).
    pub point: [f64; 3],
    /// Easting, northing, height there (metres).
    pub shared_coords: [f64; 3],
}

/// `coordinates.set_survey_point`.
pub(crate) fn set_survey(txn: &mut Transaction<'_>, p: SurveyParams) -> Result<()> {
    finite(&p.point, "point")?;
    finite(&p.shared_coords, "shared coordinates")?;
    let mut l = get(txn.store());
    l.survey_point = SurveyPoint { point: p.point, shared: p.shared_coords };
    put(txn, &l)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BaseParams {
    /// Internal coordinates (metres).
    pub point: [f64; 3],
    /// Degrees from project north to true north, counter-clockwise; kept when omitted.
    pub angle_to_true_north: Option<f64>,
}

/// `coordinates.set_project_base_point`.
pub(crate) fn set_base(txn: &mut Transaction<'_>, p: BaseParams) -> Result<()> {
    finite(&p.point, "point")?;
    let mut l = get(txn.store());
    l.project_base_point = p.point;
    if let Some(a) = p.angle_to_true_north {
        finite(&[a], "angle")?;
        l.true_north = normalise(a);
    }
    put(txn, &l)
}

fn normalise(a: f64) -> f64 {
    let r = a.rem_euclid(360.0);
    if r > 180.0 { r - 360.0 } else { r }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrsParams {
    /// `EPSG:nnnn`; null clears it.
    pub epsg_code: Option<String>,
}

/// `coordinates.set_crs`.
pub(crate) fn set_crs(txn: &mut Transaction<'_>, p: CrsParams) -> Result<()> {
    let code = match p.epsg_code.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(c) => {
            let digits = c.strip_prefix("EPSG:").or_else(|| c.strip_prefix("epsg:")).unwrap_or(c);
            if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad("coordinates.crs", format!("{c:?} is not an EPSG code such as EPSG:25832")));
            }
            Some(format!("EPSG:{digits}"))
        }
    };
    let mut l = get(txn.store());
    l.crs = code;
    put(txn, &l)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RotateParams {
    /// Degrees to turn true north by, counter-clockwise.
    pub angle: f64,
}

/// `position.rotate_true_north`: no element moves.
pub(crate) fn rotate_true_north(txn: &mut Transaction<'_>, p: RotateParams) -> Result<f64> {
    finite(&[p.angle], "angle")?;
    let mut l = get(txn.store());
    l.true_north = normalise(l.true_north + p.angle);
    put(txn, &l)?;
    Ok(l.true_north)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    /// Internal coordinates (metres).
    pub point: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Report {
    pub internal: [f64; 3],
    pub project: [f64; 3],
    /// Easting, northing, height.
    pub shared: [f64; 3],
    pub crs: Option<String>,
}

/// `coordinates.report`.
pub(crate) fn report(store: &Store, p: ReportParams) -> Result<Report> {
    finite(&p.point, "point")?;
    let l = get(store);
    Ok(Report { internal: p.point, project: l.project(p.point), shared: l.shared(p.point), crs: l.crs.clone() })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetAllParams {
    pub location: Location,
}

/// `location.set_all`: every value at once, as the Location dialog's OK (one undo step).
pub(crate) fn set_all(txn: &mut Transaction<'_>, p: SetAllParams) -> Result<Location> {
    let mut l = p.location;
    if l.latitude.is_some() != l.longitude.is_some() {
        return Err(bad("location.bad", "give both latitude and longitude, or neither"));
    }
    if l.latitude.is_some_and(|v| !(-90.0..=90.0).contains(&v)) || l.longitude.is_some_and(|v| !(-180.0..=180.0).contains(&v)) {
        return Err(bad("location.bad", "latitude runs from −90 to 90 and longitude from −180 to 180 degrees"));
    }
    if l.time_zone.is_some_and(|z| !(-12.0..=14.0).contains(&z)) {
        return Err(bad("location.bad", "the time zone runs from −12 to +14 hours"));
    }
    finite(&[l.elevation, l.true_north], "location")?;
    finite(&l.project_base_point, "project base point")?;
    finite(&l.survey_point.point, "survey point")?;
    finite(&l.survey_point.shared, "survey point's shared coordinates")?;
    l.true_north = normalise(l.true_north);
    if let Some(c) = l.crs.take() {
        let digits = c.trim().trim_start_matches("EPSG:").trim_start_matches("epsg:").to_owned();
        if !digits.is_empty() {
            if digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad("coordinates.crs", format!("{c:?} is not an EPSG code such as EPSG:25832")));
            }
            l.crs = Some(format!("EPSG:{digits}"));
        }
    }
    put(txn, &l)?;
    Ok(l)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// `location.get`.
pub(crate) fn get_cmd(store: &Store, _p: Empty) -> Result<Location> {
    Ok(get(store))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compound_angles() {
        assert_eq!(compound(51.5), [51, 30, 0, 0]);
        assert_eq!(compound(-0.1276), [0, -7, -39, -360_000]);
        assert!((from_compound(&compound(-33.868_82)) + 33.868_82).abs() < 1e-9);
    }

    #[test]
    fn shared_coordinates_turn_with_true_north() {
        let l = Location { survey_point: SurveyPoint { point: [10.0, 0.0, 0.0], shared: [500_000.0, 5_700_000.0, 30.0] }, true_north: 90.0, ..Default::default() };
        // True north points along internal −X; a point 1 m along −X from the survey point is
        // 1 m north of it.
        let s = l.shared([9.0, 0.0, 2.0]);
        assert!((s[0] - 500_000.0).abs() < 1e-9 && (s[1] - 5_700_001.0).abs() < 1e-9 && (s[2] - 32.0).abs() < 1e-12, "{s:?}");
        let d = l.true_north_direction();
        assert!((d[0] + 1.0).abs() < 1e-12 && d[1].abs() < 1e-12);
        let (o, x) = l.map_conversion();
        assert!((o[0] - 500_000.0).abs() < 1e-9 && (o[1] - 5_700_010.0).abs() < 1e-9, "{o:?}");
        assert!(x[0].abs() < 1e-12 && (x[1] + 1.0).abs() < 1e-12, "{x:?}");
    }
}
