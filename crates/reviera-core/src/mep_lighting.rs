// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Electrical equipment, devices and lighting fixtures (MEP-ELEC-001).
//!
//! - **Electrical equipment** (panelboards, switchboards, motor control centres, transformers)
//!   goes on the nearest wall face or stands free, as control devices are placed.
//! - **Devices** (receptacles, data and telephone outlets, switches, fire alarm and security
//!   devices) go on a wall face, 300 mm up by default.
//! - **Lighting fixtures** hang under the ceiling over the point (and follow it), go on a wall
//!   face when one is given, or stand at an offset.
//! - **Room calculation point**: a type may carry a point in its own frame that decides which
//!   room each copy is in, for schedules (the Room field) and calculations; the insertion point
//!   otherwise. A fixture whose body crosses into the next room still belongs where its point is.
//! - **Photometry**: a lighting fixture type may name a photometric web file, IES LM-63 (and
//!   EULUMDAT as an addition). Its lumens and peak intensity are read from the file; on export
//!   each fixture gets an `IfcLightSourceGoniometric` whose distribution is an
//!   `IfcDocumentReference` to the file.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mesh_objects::{self, Category};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A photometric web file's format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhotometryFormat {
    /// IES LM-63.
    Ies,
    /// EULUMDAT (.ldt).
    Eulumdat,
}

/// A lighting fixture type's photometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Photometry {
    /// The file as given (exported as the light source's document reference).
    pub file: String,
    pub format: PhotometryFormat,
    /// The lamps' total luminous flux (lm), when the file states it.
    pub lumens: Option<f64>,
    /// The peak luminous intensity (cd).
    pub max_candela: Option<f64>,
    /// Kelvin.
    #[serde(default = "warm")]
    pub colour_temperature: f64,
}

fn warm() -> f64 {
    3000.0
}

/// Reads an IES LM-63 file: the lamps' flux (none for absolute photometry) and the peak intensity.
pub fn parse_ies(text: &str) -> Result<(Option<f64>, f64)> {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("").trim_start_matches('\u{feff}');
    if !first.starts_with("IESNA") && !text.lines().any(|l| l.trim_start().starts_with("TILT=")) {
        return Err(bad("photometry.format", "not an IES LM-63 file (no IESNA header or TILT line)"));
    }
    let after: String = text.lines().skip_while(|l| !l.trim_start().starts_with("TILT=")).collect::<Vec<_>>().join("\n");
    let mut rest = after.lines();
    let tilt = rest.next().unwrap_or("").trim().trim_start_matches("TILT=").to_owned();
    let tokens: Vec<f64> = rest.flat_map(|l| l.split([' ', ',', '\t']).filter(|t| !t.is_empty()).map(str::to_owned).collect::<Vec<_>>()).filter_map(|t| t.parse().ok()).collect();
    // TILT=INCLUDE: lamp-to-luminaire geometry, the number of pairs, then angles and factors.
    let mut k = 0;
    if tilt.eq_ignore_ascii_case("INCLUDE") {
        let pairs = *tokens.get(1).ok_or_else(|| bad("photometry.format", "the TILT data is cut short"))? as usize;
        k = 2 + 2 * pairs;
    }
    let need = |i: usize| tokens.get(k + i).copied().ok_or_else(|| bad("photometry.format", "the photometric data is cut short"));
    let lamps = need(0)?;
    let per_lamp = need(1)?;
    let multiplier = need(2)?;
    let (nv, nh) = (need(3)? as usize, need(4)? as usize);
    if nv == 0 || nh == 0 {
        return Err(bad("photometry.format", "the file has no angles"));
    }
    // 10 header values, then ballast factor, future use and input watts.
    let start = k + 13 + nv + nh;
    let candela = tokens.get(start..start + nv * nh).ok_or_else(|| bad("photometry.format", format!("expected {} candela values", nv * nh)))?;
    let peak = candela.iter().copied().fold(0.0, f64::max) * multiplier;
    let lumens = (per_lamp > 0.0).then_some(lamps * per_lamp);
    Ok((lumens, peak))
}

/// Reads an EULUMDAT file: its lamps' flux (the first lamp set's) and the peak intensity.
pub fn parse_ldt(text: &str) -> Result<(Option<f64>, f64)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    if lines.len() < 30 {
        return Err(bad("photometry.format", "not a EULUMDAT file (too few lines)"));
    }
    let num = |i: usize| lines.get(i).and_then(|l| l.replace(',', ".").parse::<f64>().ok());
    // Lines 4 (Mc), 6 (Nc) and 8 (Ng): planes and angles; 26: lamp sets; then 6 lines per set.
    let (mc, nc, ng) = (num(3).unwrap_or(0.0) as usize, num(5).unwrap_or(0.0) as usize, num(7).unwrap_or(0.0) as usize);
    let sets = num(25).unwrap_or(0.0) as usize;
    if mc == 0 || nc == 0 || ng == 0 || sets == 0 {
        return Err(bad("photometry.format", "not a EULUMDAT file (no planes, angles or lamps)"));
    }
    let flux = num(25 + 1 + 2 * sets);
    let start = 26 + 6 * sets + 10 + nc + ng;
    let peak = lines.iter().skip(start).filter_map(|l| l.replace(',', ".").parse::<f64>().ok()).fold(0.0, f64::max);
    // Intensities are cd per 1000 lm.
    Ok((flux, flux.map_or(peak, |f| peak * f / 1000.0)))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PhotometryParams {
    /// A lighting fixture type.
    pub type_id: ElementRef,
    /// The .ies or .ldt file; none clears it.
    pub file: Option<String>,
    /// Kelvin (3000 by default).
    pub colour_temperature: Option<f64>,
}

fn write_type(txn: &mut Transaction<'_>, id: ElementId, t: &mesh_objects::MeshObjectType) -> Result<()> {
    let v = serde_json::to_value(t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

fn component_type(store: &Store, r: &ElementRef) -> Result<(ElementId, mesh_objects::MeshObjectType)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such type"))?;
    let t = store.get(id).and_then(mesh_objects::mesh_object_type).ok_or_else(|| bad("mesh_object.not_a_type", format!("element {id} is not a component type")))?;
    Ok((id, t))
}

/// `lighting_fixture_type.set_photometry`: reads the file and keeps its name and values.
pub(crate) fn set_photometry(txn: &mut Transaction<'_>, p: PhotometryParams) -> Result<Option<Photometry>> {
    let (id, mut t) = component_type(txn.store(), &p.type_id)?;
    if t.category != Category::LightingFixture {
        return Err(bad("lighting_fixture.category", "photometry belongs to lighting fixture types"));
    }
    let Some(file) = p.file else {
        t.photometry = None;
        write_type(txn, id, &t)?;
        return Ok(None);
    };
    let text = std::fs::read(&file).map_err(|e| bad("photometry.read", format!("{file}: {e}")))?;
    let text = String::from_utf8_lossy(&text);
    let lower = file.to_lowercase();
    let (format, (lumens, peak)) = if lower.ends_with(".ldt") { (PhotometryFormat::Eulumdat, parse_ldt(&text)?) } else { (PhotometryFormat::Ies, parse_ies(&text)?) };
    let ct = p.colour_temperature.unwrap_or(3000.0);
    if !(1000.0..=20000.0).contains(&ct) {
        return Err(bad("photometry.colour_temperature", "a colour temperature from 1000 to 20000 K"));
    }
    let ph = Photometry { file, format, lumens, max_candela: Some(peak), colour_temperature: ct };
    t.photometry = Some(ph.clone());
    write_type(txn, id, &t)?;
    Ok(Some(ph))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalcPointParams {
    pub type_id: ElementRef,
    /// In the type's frame (metres); none: the insertion point.
    pub point: Option<[f64; 3]>,
}

/// `component_type.set_calc_point`: a type's room calculation point.
pub(crate) fn set_calc_point(txn: &mut Transaction<'_>, p: CalcPointParams) -> Result<()> {
    let (id, mut t) = component_type(txn.store(), &p.type_id)?;
    if p.point.is_some_and(|q| q.iter().any(|v| !v.is_finite())) {
        return Err(bad("calc_point.value", "the point's coordinates are numbers"));
    }
    t.calc_point = p.point;
    write_type(txn, id, &t)
}

/// A placed component's room calculation point in project coordinates.
pub fn calc_point(store: &Store, e: &crate::element::Element) -> Option<[f64; 3]> {
    let g = mesh_objects::regenerate(store, e).ok()?;
    Some(g.placement.apply(g.ty.calc_point.unwrap_or([0.0; 3])))
}

/// The room a placed component is in, by its calculation point.
pub fn room_of(store: &Store, e: &crate::element::Element) -> Option<ElementId> {
    let r = mesh_objects::mesh_object_recipe(e)?;
    let level = GlobalId::parse(&r.level).and_then(|g| store.find_by_guid(&g))?;
    let p = calc_point(store, e)?;
    crate::rooms::room_at(store, level, [p[0], p[1]])
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContentsParams {
    pub room_id: ElementRef,
    /// Only these categories (`lighting_fixture`, `electrical_fixture`, …); every component when
    /// empty.
    #[serde(default)]
    pub categories: Vec<Category>,
}

/// `space.elements`: the components in a room, by their calculation points.
pub(crate) fn contents(store: &Store, p: ContentsParams) -> Result<Vec<ElementId>> {
    let room = store.resolve(&p.room_id).ok_or_else(|| bad("element.not_found", "no such room"))?;
    Ok(store
        .iter()
        .filter(|e| mesh_objects::mesh_object_recipe(e).is_some())
        .filter(|e| p.categories.is_empty() || mesh_objects::type_of(store, e).is_some_and(|(_, t)| p.categories.contains(&t.category)))
        .filter(|e| room_of(store, e) == Some(room))
        .map(|e| e.id())
        .collect())
}

// ---- Placing ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    pub type_id: ElementRef,
    pub point: [f64; 2],
    pub level: ElementRef,
    /// Height above the level (metres); the kind's default when omitted.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: Option<f64>,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
    /// The wall whose face it goes on (else the nearest within 300 mm; lighting fixtures only
    /// when given).
    pub host_wall: Option<ElementRef>,
    /// Lighting fixtures: the ceiling it hangs from (else the one over the point).
    pub host_ceiling: Option<ElementRef>,
    /// Stand free at `offset`.
    #[serde(default)]
    pub work_plane: bool,
}

fn hosted(txn: &mut Transaction<'_>, p: &PlaceParams, accepts: fn(Category) -> bool, code: &'static str, what: &str, height: f64) -> Result<ElementId> {
    let args = json!({ "type_id": p.type_id, "point": p.point, "level": p.level, "offset": p.offset.unwrap_or(height), "rotation": p.rotation, "host_wall": p.host_wall, "work_plane": p.work_plane });
    let cp = serde_json::from_value(args).map_err(|e| bad("place.params", e.to_string()))?;
    crate::mep_controls::place_hosted(txn, cp, accepts, code, what, height)
}

/// `electrical_equipment.place`: on the nearest wall face, or free.
pub(crate) fn place_equipment(txn: &mut Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    if p.host_ceiling.is_some() {
        return Err(bad("electrical_equipment.ceiling", "electrical equipment goes on a wall or the floor"));
    }
    hosted(txn, &p, Category::is_electrical_equipment, "electrical_equipment.category", "electrical equipment", 0.0)
}

/// `device.place`: on a wall face, 300 mm up by default.
pub(crate) fn place_device(txn: &mut Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    if p.host_ceiling.is_some() {
        return Err(bad("device.ceiling", "a device goes on a wall; detectors on ceilings are placed as lighting fixtures are not, use work_plane with an offset"));
    }
    hosted(txn, &p, Category::is_electrical_device, "device.category", "a device (receptacle, outlet, switch, alarm or detector)", 0.3)
}

/// `lighting_fixture.place`: under the ceiling over the point (following it), on a wall face
/// when one is given, or at an offset.
pub(crate) fn place_fixture(txn: &mut Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (type_id, ty) = component_type(&store, &p.type_id)?;
    if ty.category != Category::LightingFixture {
        return Err(bad("lighting_fixture.category", format!("{} is a {}, not a lighting fixture", store.get(type_id).and_then(|e| e.name.clone()).unwrap_or_default(), ty.category.label())));
    }
    if p.host_wall.is_some() {
        return hosted(txn, &p, |c| c == Category::LightingFixture, "lighting_fixture.category", "a lighting fixture", 2.1);
    }
    let level_id = store.resolve(&p.level).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let level = store.get(level_id).filter(|e| e.class == "IfcBuildingStorey").ok_or_else(|| bad("level.not_a_level", format!("element {level_id} is not a level")))?;
    let height = ty.size[2] * ty.scale;
    let ceiling = match (&p.host_ceiling, p.work_plane) {
        (Some(c), _) => {
            let c = store.resolve(c).ok_or_else(|| bad("element.not_found", "no such ceiling"))?;
            store.get(c).filter(|e| crate::floors::slab_recipe(e).is_some_and(|(k, _)| k == crate::floors::SlabKind::Ceiling)).ok_or_else(|| bad("lighting_fixture.host", format!("element {c} is not a ceiling")))?;
            Some(c)
        }
        (None, true) => None,
        (None, false) => crate::mep_sprinklers::ceilings_over(&store, level.guid().to_string().as_str(), p.point).into_iter().next(),
    };
    let offset = match (p.offset, ceiling) {
        (Some(o), _) => o,
        (None, Some(_)) => -height,
        (None, None) => crate::floors::DEFAULT_CEILING_OFFSET - height,
    };
    let args = json!({ "type_id": type_id.0, "point": p.point, "level": level_id.0, "offset": offset, "rotation": p.rotation });
    let id = mesh_objects::place(txn, serde_json::from_value(args).map_err(|e| bad("place.params", e.to_string()))?)?;
    if let Some(c) = ceiling {
        let mut r = mesh_objects::mesh_object_recipe(txn.store().get(id).expect("placed")).expect("a copy");
        r.ceiling = Some(store.get(c).expect("resolved").guid().to_string());
        let v = serde_json::to_value(&r).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
        crate::mep::connect_coincident(txn, &[id])?;
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ies() {
        // Two lamps of 1000 lm, multiplier 1, 3 vertical × 1 horizontal angles.
        let ies = "IESNA:LM-63-2002\n[TEST] made up\nTILT=NONE\n2 1000 1 3 1 1 2 0.6 0.6 0.1\n1 1 40\n0 45 90\n0\n500 320 10\n";
        let (lm, cd) = parse_ies(ies).unwrap();
        assert_eq!(lm, Some(2000.0));
        assert_eq!(cd, 500.0);
        let abs = ies.replace("2 1000 1", "1 -1 2");
        assert_eq!(parse_ies(&abs).unwrap(), (None, 1000.0));
        assert!(parse_ies("hello\nworld\n").is_err());
        assert!(parse_ies("IESNA:LM-63-2002\nTILT=NONE\n1 1000 1 3 1 1 2 0.6 0.6 0.1\n1 1 40\n0 45 90\n0\n500\n").is_err());
    }
}
