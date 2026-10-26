// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! HBJSON export (ANA-ENV-009): the energy model (ANA-EM-003) as a honeybee-schema `Model` for
//! Ladybug Tools (honeybee-schema is BSD-3; nothing of it is bundled, the file is written from
//! its published field names). Each analytical space is a `Room` (identifier `sp-` and its
//! GlobalId with `$` as `-`, as in gbXML), each surface a `Face` with its vertices
//! counter-clockwise seen from outside the room: walls, floors and roofs or ceilings, separation
//! lines as `AirBoundary`. Exterior faces are `Outdoors`, slabs on grade `Ground`; an interior
//! face is a `Surface` boundary naming its twin in the other room (the face of the same element
//! and area facing back, a wall thickness away), else `Adiabatic` (listed in the report). Windows and doors of exterior
//! walls are its `Aperture`s and `Door`s. Constructions carry the U-values: one no-mass layer of
//! R = 1/U less the surface resistances (E-41); windows a simple glazing system (U, SHGC, visible
//! transmittance). Loads and schedules (program types) are not written.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::energy_model::{self, Surface, SurfaceClass};
use crate::error::{Error, Result};
use crate::id::ElementId;
use crate::store::Store;

/// The honeybee-schema release the file is written to (its `version`).
pub const SCHEMA_VERSION: &str = "2.3.0";

/// Inside plus outside surface resistance (m²K/W) taken off 1/U for a class's no-mass layer
/// (ISO 6946 values; E-41): EnergyPlus adds its own surface heat transfer.
pub fn film_resistance(class: SurfaceClass) -> f64 {
    match class {
        SurfaceClass::ExteriorWall | SurfaceClass::Door => 0.13 + 0.04,
        SurfaceClass::Roof => 0.10 + 0.04,
        SurfaceClass::SlabOnGrade => 0.17,
        SurfaceClass::InteriorWall => 0.13 + 0.13,
        SurfaceClass::InteriorCeiling | SurfaceClass::InteriorFloor => 0.10 + 0.17,
        SurfaceClass::Window | SurfaceClass::Air => 0.0,
    }
}

/// What an export wrote.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ExportReport {
    pub rooms: usize,
    pub faces: usize,
    pub apertures: usize,
    pub doors: usize,
    /// Interior faces written adiabatic because the room beyond has no matching face.
    pub adiabatic: usize,
    pub warnings: Vec<String>,
}

fn newell(pts: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// The outline counter-clockwise seen along `normal` from outside (right-hand rule outward).
fn outward(outline: &[[f64; 3]], normal: [f64; 3]) -> Vec<[f64; 3]> {
    let n = newell(outline);
    let mut pts = outline.to_vec();
    if n[0] * normal[0] + n[1] * normal[1] + n[2] * normal[2] < 0.0 {
        pts.reverse();
    }
    pts
}

/// A window's or door's vertices kept 5 mm (half the model tolerance) inside its vertical
/// wall's edges: honeybee does not take a sub-face whose edge lies on its parent's, such as a
/// door on the floor.
fn inset(sub: &[[f64; 3]], wall: &[[f64; 3]], normal: [f64; 3]) -> Vec<[f64; 3]> {
    const E: f64 = 0.005;
    let len = normal[0].hypot(normal[1]);
    let Some(o) = wall.first().copied().filter(|_| len > 0.99) else { return sub.to_vec() };
    let u = [-normal[1] / len, normal[0] / len];
    let s_of = |p: &[f64; 3]| (p[0] - o[0]) * u[0] + (p[1] - o[1]) * u[1];
    let (s0, s1) = wall.iter().map(s_of).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let (z0, z1) = wall.iter().map(|p| p[2]).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
    if s1 - s0 <= 4.0 * E || z1 - z0 <= 4.0 * E {
        return sub.to_vec();
    }
    sub.iter()
        .map(|p| {
            let (s, t) = (s_of(p), (p[0] - o[0]) * normal[0] + (p[1] - o[1]) * normal[1]);
            let s = s.clamp(s0 + E, s1 - E);
            [o[0] + s * u[0] + t * normal[0], o[1] + s * u[1] + t * normal[1], p[2].clamp(z0 + E, z1 - E)]
        })
        .collect()
}

fn centroid(pts: &[[f64; 3]]) -> [f64; 3] {
    let k = pts.len().max(1) as f64;
    [0, 1, 2].map(|a| pts.iter().map(|p| p[a]).sum::<f64>() / k)
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[cfg(test)]
fn key(pts: &[[f64; 3]]) -> Vec<[i64; 3]> {
    let mut k: Vec<[i64; 3]> = pts.iter().map(|p| [(p[0] * 1000.0).round() as i64, (p[1] * 1000.0).round() as i64, (p[2] * 1000.0).round() as i64]).collect();
    k.sort_unstable();
    k
}

fn round(pts: &[[f64; 3]]) -> Value {
    json!(pts.iter().map(|p| p.map(|c| (c * 1e4).round() / 1e4)).collect::<Vec<_>>())
}

fn opaque(class: SurfaceClass) -> bool {
    !matches!(class, SurfaceClass::Window | SurfaceClass::Door)
}

/// An opaque face: its identifier, its surface and its outward vertices.
type Face<'a> = (String, &'a Surface, Vec<[f64; 3]>);

struct Constructions {
    constructions: BTreeMap<String, Value>,
    materials: BTreeMap<String, Value>,
}

impl Constructions {
    fn opaque(&mut self, class: SurfaceClass, u: f64) -> String {
        let name = format!("{class:?}");
        let r = (1.0 / u.max(1e-3) - film_resistance(class)).max(0.001);
        let id = format!("{name}_U{u:.3}");
        let mat = format!("{id}_layer");
        self.materials.insert(mat.clone(), json!({ "type": "EnergyMaterialNoMass", "identifier": mat, "r_value": (r * 1e4).round() / 1e4 }));
        self.constructions.insert(id.clone(), json!({ "type": "OpaqueConstructionAbridged", "identifier": id, "materials": [mat] }));
        id
    }

    fn window(&mut self, u: f64, shgc: f64, vt: f64) -> String {
        let id = format!("Window_U{u:.3}_g{shgc:.2}_vt{vt:.2}");
        let mat = format!("{id}_glazing");
        self.materials.insert(mat.clone(), json!({ "type": "EnergyWindowMaterialSimpleGlazSys", "identifier": mat, "u_factor": u, "shgc": shgc, "vt": vt }));
        self.constructions.insert(id.clone(), json!({ "type": "WindowConstructionAbridged", "identifier": id, "materials": [mat] }));
        id
    }
}

fn outdoors() -> Value {
    json!({ "type": "Outdoors", "sun_exposure": true, "wind_exposure": true, "view_factor": { "type": "Autocalculate" } })
}

/// The HBJSON `Model` for the model's energy model.
pub fn export(store: &Store) -> Result<(Value, ExportReport)> {
    let model = energy_model::build(store);
    if model.spaces.is_empty() {
        return Err(Error::new("hbjson.no_spaces", "the energy model has no spaces: place enclosed MEP spaces first"));
    }
    let guid = |id: ElementId| store.get(id).map(|e| e.guid().to_string()).unwrap_or_else(|| format!("e{}", id.0));
    let room_id = |id: ElementId| crate::gbxml::space_id(&guid(id));
    let mut report = ExportReport { rooms: model.spaces.len(), ..Default::default() };
    for (id, why) in &model.skipped {
        report.warnings.push(format!("space {} left out: {why}", id.0));
    }
    // Every opaque face's identifier and outward outline first, so twins can name each other.
    let all: Vec<Vec<Face>> = model
        .spaces
        .iter()
        .map(|s| s.surfaces.iter().filter(|f| opaque(f.class) && f.outline.len() >= 3).enumerate().map(|(k, f)| (format!("{}_f{k}", room_id(s.space)), f, outward(&f.outline, f.normal))).collect())
        .collect();
    let index: BTreeMap<ElementId, usize> = model.spaces.iter().enumerate().map(|(i, s)| (s.space, i)).collect();
    // Twins: a face of the room beyond, facing back, made by the same element, of the same area
    // (the two rooms' faces of a wall lie a wall thickness apart); the nearest, when both agree.
    let best = |i: usize, k: usize| -> Option<(usize, usize)> {
        let (_, f, p) = &all[i][k];
        let j = *index.get(&f.adjacent?)?;
        let c = centroid(p);
        all[j]
            .iter()
            .enumerate()
            .filter(|(_, (_, g, _))| {
                g.adjacent == Some(model.spaces[i].space)
                    && (g.element == f.element || g.element.is_none() || f.element.is_none())
                    && g.normal.iter().zip(f.normal).map(|(a, b)| a * b).sum::<f64>() < -0.99
                    && (g.area - f.area).abs() <= 0.01 * f.area.max(g.area) + 0.01
            })
            .map(|(l, (_, _, q))| (l, dist(c, centroid(q))))
            .filter(|&(_, d)| d < 1.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(l, _)| (j, l))
    };
    let mut twins: BTreeMap<(usize, usize), (usize, usize)> = BTreeMap::new();
    for (i, faces) in all.iter().enumerate() {
        for k in 0..faces.len() {
            if let Some((j, l)) = best(i, k).filter(|&(j, l)| best(j, l) == Some((i, k))) {
                twins.insert((i, k), (j, l));
            }
        }
    }
    let levels: BTreeMap<ElementId, String> = crate::levels::list(store).into_iter().map(|l| (l.id, l.name)).collect();
    let mut cons = Constructions { constructions: BTreeMap::new(), materials: BTreeMap::new() };
    let default_vt = crate::daylight::DaylightOptions::default().glazing_transmittance;
    let mut defaults = 0usize;
    let mut rooms = Vec::new();
    for (i, s) in model.spaces.iter().enumerate() {
        let rid = room_id(s.space);
        let mut faces = Vec::new();
        for (fk, (fid, f, pts)) in all[i].iter().enumerate() {
            let twin = twins.get(&(i, fk)).map(|&(j, l)| (all[j][l].0.clone(), room_id(model.spaces[j].space)));
            let interior = matches!(f.class, SurfaceClass::InteriorWall | SurfaceClass::InteriorCeiling | SurfaceClass::InteriorFloor | SurfaceClass::Air);
            let face_type = match f.class {
                SurfaceClass::ExteriorWall | SurfaceClass::InteriorWall => "Wall",
                SurfaceClass::Roof | SurfaceClass::InteriorCeiling => "RoofCeiling",
                SurfaceClass::SlabOnGrade | SurfaceClass::InteriorFloor => "Floor",
                SurfaceClass::Air if twin.is_some() => "AirBoundary",
                _ => "Wall",
            };
            let bc = match (&twin, f.class) {
                (Some((gid, other)), _) => json!({ "type": "Surface", "boundary_condition_objects": [gid, other] }),
                (None, SurfaceClass::SlabOnGrade) => json!({ "type": "Ground" }),
                (None, _) if interior => {
                    report.adiabatic += 1;
                    json!({ "type": "Adiabatic" })
                }
                _ => outdoors(),
            };
            let mut energy = json!({ "type": "FaceEnergyPropertiesAbridged" });
            if f.class != SurfaceClass::Air {
                if !f.u_from_type {
                    defaults += 1;
                }
                energy["construction"] = json!(cons.opaque(f.class, f.u));
            }
            let mut face = json!({
                "type": "Face",
                "identifier": fid,
                "display_name": format!("{} {}", face_type, fid.rsplit('_').next().unwrap_or("")),
                "geometry": { "type": "Face3D", "boundary": round(pts) },
                "face_type": face_type,
                "boundary_condition": bc,
                "properties": { "type": "FacePropertiesAbridged", "energy": energy },
            });
            // The wall's windows and doors: those of the space in its plane.
            if f.class == SurfaceClass::ExteriorWall {
                let (mut aps, mut doors) = (Vec::new(), Vec::new());
                for o in s.surfaces.iter().filter(|o| !opaque(o.class) && o.normal == f.normal && o.outline.first().is_some_and(|p| crate::gbxml::on_plane(&f.outline, f.normal, *p))) {
                    if !o.u_from_type {
                        defaults += 1;
                    }
                    let opts = round(&inset(&outward(&o.outline, o.normal), &f.outline, f.normal));
                    if o.class == SurfaceClass::Window {
                        let vt = o.element.and_then(|id| store.get(id)).and_then(|e| crate::daylight::transmittance_of(store, e)).unwrap_or(default_vt);
                        let c = cons.window(o.u, o.shgc.unwrap_or(energy_model::DEFAULT_SHGC), vt);
                        let id = format!("{fid}_ap{}", aps.len());
                        aps.push(json!({
                            "type": "Aperture", "identifier": id, "display_name": format!("Window {}", aps.len() + 1),
                            "geometry": { "type": "Face3D", "boundary": opts }, "is_operable": false, "boundary_condition": outdoors(),
                            "properties": { "type": "AperturePropertiesAbridged", "energy": { "type": "ApertureEnergyPropertiesAbridged", "construction": c } },
                        }));
                    } else {
                        let c = cons.opaque(SurfaceClass::Door, o.u);
                        let id = format!("{fid}_dr{}", doors.len());
                        doors.push(json!({
                            "type": "Door", "identifier": id, "display_name": format!("Door {}", doors.len() + 1),
                            "geometry": { "type": "Face3D", "boundary": opts }, "is_glass": false, "boundary_condition": outdoors(),
                            "properties": { "type": "DoorPropertiesAbridged", "energy": { "type": "DoorEnergyPropertiesAbridged", "construction": c } },
                        }));
                    }
                }
                report.apertures += aps.len();
                report.doors += doors.len();
                if !aps.is_empty() {
                    face["apertures"] = json!(aps);
                }
                if !doors.is_empty() {
                    face["doors"] = json!(doors);
                }
            }
            faces.push(face);
        }
        report.faces += faces.len();
        let mut room = json!({
            "type": "Room",
            "identifier": rid,
            "display_name": format!("{} {}", s.number, s.name).trim(),
            "faces": faces,
            "multiplier": 1,
            "properties": { "type": "RoomPropertiesAbridged", "energy": { "type": "RoomEnergyPropertiesAbridged" } },
        });
        if let Some(l) = s.level.and_then(|l| levels.get(&l)) {
            room["story"] = json!(l);
        }
        if let Some(z) = s.zone {
            room["zone"] = json!(format!("zn-{}", guid(z).replace('$', "-")));
        }
        rooms.push(room);
    }
    if defaults > 0 {
        report.warnings.push(format!("{defaults} faces, windows or doors use the class's default U-value (no thermal values on their types)"));
    }
    if report.adiabatic > 0 {
        report.warnings.push(format!("{} interior faces have no matching face (same element and area) in the room beyond and are written adiabatic", report.adiabatic));
    }
    let name = store.iter().find(|e| e.class == "IfcProject").and_then(|p| p.name.clone()).unwrap_or_else(|| "Project".into());
    let ident: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).take(100).collect();
    let model = json!({
        "type": "Model",
        "identifier": if ident.is_empty() { "Model".to_owned() } else { ident },
        "display_name": name,
        "version": SCHEMA_VERSION,
        "units": "Meters",
        "tolerance": 0.01,
        "angle_tolerance": 1.0,
        "rooms": rooms,
        "properties": {
            "type": "ModelProperties",
            "energy": {
                "type": "ModelEnergyProperties",
                "constructions": cons.constructions.into_values().collect::<Vec<_>>(),
                "materials": cons.materials.into_values().collect::<Vec<_>>(),
                "construction_sets": [], "hvacs": [], "program_types": [], "schedules": [], "schedule_type_limits": [],
            },
        },
        "user_data": { "exported_by": format!("BIM Reviera {}", env!("CARGO_PKG_VERSION")) },
    });
    Ok((model, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlines_turn_outward() {
        let sq = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
        assert!(newell(&sq)[2] > 0.0);
        let down = outward(&sq, [0.0, 0.0, -1.0]);
        assert!(newell(&down)[2] < 0.0);
        assert_eq!(key(&down), key(&sq));
        assert_eq!(outward(&sq, [0.0, 0.0, 1.0]), sq.to_vec());
    }

    #[test]
    fn sub_faces_kept_inside() {
        let wall = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 0.0, 3.0], [0.0, 0.0, 3.0]];
        let door = [[1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 0.0, 2.1], [1.0, 0.0, 2.1]];
        let d = inset(&door, &wall, [0.0, -1.0, 0.0]);
        assert!((d[0][2] - 0.005).abs() < 1e-12 && d[0][1].abs() < 1e-12 && (d[0][0] - 1.0).abs() < 1e-12);
        assert_eq!(d[2], [2.0, 0.0, 2.1]);
    }

    #[test]
    fn no_mass_layer_keeps_the_u_value() {
        let r = 1.0 / 0.28 - film_resistance(SurfaceClass::ExteriorWall);
        assert!((1.0 / (r + 0.17) - 0.28).abs() < 1e-12);
        let mut c = Constructions { constructions: BTreeMap::new(), materials: BTreeMap::new() };
        let id = c.opaque(SurfaceClass::ExteriorWall, 0.28);
        assert_eq!(id, "ExteriorWall_U0.280");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')));
        assert_eq!(c.materials.len(), 1);
    }
}
