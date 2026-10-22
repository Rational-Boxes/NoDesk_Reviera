// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Custom objects from meshes (DM-IMP-015): a mesh file (OBJ, STL, PLY, glTF, GLB) becomes a
//! **custom object type** holding the mesh once, and **custom objects** are its placed copies.
//!
//! - The type is the category's IFC type class (`IfcFurnitureType`, `IfcColumnType`, …, or
//!   `IfcBuildingElementProxyType` for Generic Model) with a `recipe.mesh_object_type`: the
//!   category, the material slots (name and colour), the source file (name, format, SHA-256,
//!   path), the import options and the mesh itself, in metres, Z up, with its insertion point at
//!   the origin. The mesh is stored compactly (base64 of little-endian `f32` positions, `u32`
//!   triangles and `u16` slots) and decoded once per content hash.
//! - A copy is an element of the category's class with a `recipe.mesh_object`: its point in plan,
//!   its level and offset, its rotation and whether it is mirrored. All copies share the type's
//!   mesh: in IFC, one `IfcRepresentationMap` and an `IfcMappedItem` per copy.
//!
//! Import options: the units (from the file when it states them, glTF being in metres; otherwise
//! chosen), the up axis (glTF is Y up; Z otherwise, with a flip), the insertion point (bottom
//! centre, the file's origin, or the bounding box's corner) and clean-up (welding coincident
//! vertices, dropping degenerate triangles). The inspection reports open and non-manifold edges.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mesh_io::{self, RawMesh, b64};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const TYPE_RECIPE: &str = "recipe.mesh_object_type";
pub const RECIPE: &str = "recipe.mesh_object";
/// Above this many triangles an import needs `allow_large`.
pub const LARGE_TRIANGLES: usize = 1_000_000;
/// Above this many an import is refused.
pub const MAX_TRIANGLES: usize = 5_000_000;

/// What a custom object is, which decides its IFC classes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    #[default]
    GenericModel,
    Furniture,
    Column,
    PlumbingFixture,
    LightingFixture,
    Planting,
    MechanicalEquipment,
    AirTerminal,
    ElectricalEquipment,
    ElectricalFixture,
    Sprinkler,
    Transformer,
    /// Thermostats and humidistats (MEP-DUCT-012).
    ControlElement,
    Sensor,
    Controller,
    /// VAV and constant-volume boxes.
    AirTerminalBox,
    /// Floor drains and gullies (MEP-PIPE-006).
    WasteTerminal,
    /// Pumps, boilers and chillers (MEP-PIPE-008): what hydronic network analysis looks for.
    Pump,
    Boiler,
    Chiller,
    /// Fans and radiators (MEP-DUCT-009).
    Fan,
    SpaceHeater,
}

impl Category {
    pub const ALL: [Category; 22] = [
        Category::GenericModel,
        Category::Furniture,
        Category::Column,
        Category::PlumbingFixture,
        Category::LightingFixture,
        Category::Planting,
        Category::MechanicalEquipment,
        Category::AirTerminal,
        Category::ElectricalEquipment,
        Category::ElectricalFixture,
        Category::Sprinkler,
        Category::Transformer,
        Category::ControlElement,
        Category::Sensor,
        Category::Controller,
        Category::AirTerminalBox,
        Category::WasteTerminal,
        Category::Pump,
        Category::Boiler,
        Category::Chiller,
        Category::Fan,
        Category::SpaceHeater,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::GenericModel => "Generic Model",
            Category::Furniture => "Furniture",
            Category::Column => "Column",
            Category::PlumbingFixture => "Plumbing Fixture",
            Category::LightingFixture => "Lighting Fixture",
            Category::Planting => "Planting",
            Category::MechanicalEquipment => "Mechanical Equipment",
            Category::AirTerminal => "Air Terminal",
            Category::ElectricalEquipment => "Electrical Equipment",
            Category::ElectricalFixture => "Electrical Fixture",
            Category::Sprinkler => "Sprinkler",
            Category::Transformer => "Transformer",
            Category::ControlElement => "Mechanical Control Device",
            Category::Sensor => "Sensor",
            Category::Controller => "Controller",
            Category::AirTerminalBox => "Air Terminal Box",
            Category::WasteTerminal => "Waste Terminal",
            Category::Pump => "Pump",
            Category::Boiler => "Boiler",
            Category::Chiller => "Chiller",
            Category::Fan => "Fan",
            Category::SpaceHeater => "Space Heater",
        }
    }

    /// The occurrence class.
    pub fn class(self) -> &'static str {
        match self {
            Category::GenericModel => "IfcBuildingElementProxy",
            Category::Furniture => "IfcFurniture",
            Category::Column => "IfcColumn",
            Category::PlumbingFixture => "IfcSanitaryTerminal",
            Category::LightingFixture => "IfcLightFixture",
            Category::Planting => "IfcGeographicElement",
            Category::MechanicalEquipment => "IfcUnitaryEquipment",
            Category::AirTerminal => "IfcAirTerminal",
            Category::ElectricalEquipment => "IfcElectricDistributionBoard",
            Category::ElectricalFixture => "IfcOutlet",
            Category::Sprinkler => "IfcFireSuppressionTerminal",
            Category::Transformer => "IfcTransformer",
            Category::ControlElement => "IfcUnitaryControlElement",
            Category::Sensor => "IfcSensor",
            Category::Controller => "IfcController",
            Category::AirTerminalBox => "IfcAirTerminalBox",
            Category::WasteTerminal => "IfcWasteTerminal",
            Category::Pump => "IfcPump",
            Category::Boiler => "IfcBoiler",
            Category::Chiller => "IfcChiller",
            Category::Fan => "IfcFan",
            Category::SpaceHeater => "IfcSpaceHeater",
        }
    }

    /// The type class.
    pub fn type_class(self) -> &'static str {
        match self {
            Category::GenericModel => "IfcBuildingElementProxyType",
            Category::Furniture => "IfcFurnitureType",
            Category::Column => "IfcColumnType",
            Category::PlumbingFixture => "IfcSanitaryTerminalType",
            Category::LightingFixture => "IfcLightFixtureType",
            Category::Planting => "IfcGeographicElementType",
            Category::MechanicalEquipment => "IfcUnitaryEquipmentType",
            Category::AirTerminal => "IfcAirTerminalType",
            Category::ElectricalEquipment => "IfcElectricDistributionBoardType",
            Category::ElectricalFixture => "IfcOutletType",
            Category::Sprinkler => "IfcFireSuppressionTerminalType",
            Category::Transformer => "IfcTransformerType",
            Category::ControlElement => "IfcUnitaryControlElementType",
            Category::Sensor => "IfcSensorType",
            Category::Controller => "IfcControllerType",
            Category::AirTerminalBox => "IfcAirTerminalBoxType",
            Category::WasteTerminal => "IfcWasteTerminalType",
            Category::Pump => "IfcPumpType",
            Category::Boiler => "IfcBoilerType",
            Category::Chiller => "IfcChillerType",
            Category::Fan => "IfcFanType",
            Category::SpaceHeater => "IfcSpaceHeaterType",
        }
    }

    /// Plumbing fixtures and waste terminals (MEP-PIPE-006).
    pub fn is_plumbing_fixture(self) -> bool {
        matches!(self, Category::PlumbingFixture | Category::WasteTerminal)
    }

    /// Mechanical equipment (MEP-DUCT-009): air handlers and other unitary equipment, VAV boxes,
    /// fans, boilers, chillers, pumps and radiators.
    pub fn is_mechanical_equipment(self) -> bool {
        matches!(
            self,
            Category::MechanicalEquipment | Category::AirTerminalBox | Category::Fan | Category::Boiler | Category::Chiller | Category::Pump | Category::SpaceHeater
        )
    }

    /// Control devices: thermostats, humidistats, sensors and controllers (MEP-DUCT-012).
    pub fn is_control_device(self) -> bool {
        matches!(self, Category::ControlElement | Category::Sensor | Category::Controller)
    }

    fn predefined(self) -> &'static str {
        match self {
            Category::Column => "COLUMN",
            Category::Sprinkler => "SPRINKLER",
            _ => "NOTDEFINED",
        }
    }
}

/// The file's length unit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    /// As the file states (glTF: metres); metres when it does not.
    #[default]
    Auto,
    Mm,
    Cm,
    M,
    In,
    Ft,
}

impl Units {
    fn metres(self) -> Option<f64> {
        match self {
            Units::Auto => None,
            Units::Mm => Some(0.001),
            Units::Cm => Some(0.01),
            Units::M => Some(1.0),
            Units::In => Some(0.0254),
            Units::Ft => Some(0.3048),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpAxis {
    /// As the file states (glTF: Y); Z when it does not.
    #[default]
    Auto,
    Z,
    Y,
}

/// Where the object's origin (the point placed) is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Insertion {
    /// The middle of its footprint, at its lowest point.
    #[default]
    BottomCentre,
    /// The file's own origin.
    Origin,
    /// The bounding box's lowest corner.
    Corner,
}

fn yes() -> bool {
    true
}

/// How a file is read into a type.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportOptions {
    #[serde(default)]
    pub units: Units,
    #[serde(default)]
    pub up_axis: UpAxis,
    #[serde(default)]
    pub insertion: Insertion,
    /// Weld coincident vertices and drop degenerate triangles.
    #[serde(default = "yes")]
    pub clean: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            units: Units::Auto,
            up_axis: UpAxis::Auto,
            insertion: Insertion::BottomCentre,
            clean: true,
        }
    }
}

/// A material slot of a type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Slot {
    pub name: String,
    /// RGBA, 0–1.
    pub color: [f32; 4],
}

/// Where a type's mesh came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    /// The file's name.
    pub file: String,
    /// Its full path when imported (for Reload from File).
    #[serde(default)]
    pub path: String,
    pub format: String,
    pub sha256: String,
}

/// A mesh stored in a recipe: base64 of little-endian `f32` xyz positions, `u32` triangle
/// indices and `u16` slot numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StoredMesh {
    pub positions: String,
    pub triangles: String,
    pub slots: String,
    /// SHA-256 of the three strings: the cache key.
    pub hash: String,
}

/// The stored recipe of a custom object type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MeshObjectType {
    pub kind: String,
    pub category: Category,
    pub slots: Vec<Slot>,
    pub source: Source,
    pub options: ImportOptions,
    /// A uniform scale applied to every copy.
    #[serde(default = "one")]
    pub scale: f64,
    /// The mesh's size (x, y, z), metres, before `scale`.
    pub size: [f64; 3],
    pub triangle_count: usize,
    pub mesh: StoredMesh,
    /// The Component Library item it was loaded from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<crate::library::LibraryLink>,
    /// MEP connectors (MEP-CONN-001), in the type's frame.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<crate::mep::Connector>,
    /// The IFC predefined type of its copies (THERMOSTAT, CO2SENSOR, …); the category's when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predefined: Option<String>,
}

impl MeshObjectType {
    /// The IFC predefined type of its copies.
    pub fn predefined_type(&self) -> &str {
        self.predefined.as_deref().unwrap_or(self.category.predefined())
    }
}

fn one() -> f64 {
    1.0
}

pub fn mesh_object_type(e: &Element) -> Option<MeshObjectType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == TYPE_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// The stored recipe of a custom object (a placed copy).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MeshObjectRecipe {
    pub kind: String,
    /// The insertion point in plan (metres).
    pub point: [f64; 2],
    /// The level it stands on (GlobalId).
    pub level: String,
    /// Height of the insertion point above the level (metres).
    #[serde(default)]
    pub offset: f64,
    /// Counter-clockwise from the x axis (radians).
    #[serde(default)]
    pub rotation: f64,
    /// Mirrored across its local x axis (before turning).
    #[serde(default)]
    pub mirror: bool,
    /// Its own uniform scale, times its type's.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: f64,
    /// A toposolid it stands on (GlobalId, ARC-SITE-001): its offset is then above the
    /// surface there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// A wall face it is mounted on (MEP-DUCT-012): it follows the face, its point and rotation
    /// derived from the wall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<FaceHost>,
    /// A ceiling it hangs from (GlobalId, MEP-PIPE-007): its offset is then from the ceiling's
    /// underside, and it follows the ceiling up and down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<String>,
}

/// A copy mounted on a wall's face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FaceHost {
    /// The wall (GlobalId).
    pub wall: String,
    /// 1 on the exterior (left of the wall's direction), −1 on the interior.
    pub side: f64,
    /// Metres along the wall's location line from its start.
    pub along: f64,
}

/// A face-hosted copy's point and rotation from its wall as it is now, if the wall is there:
/// on the face, its back to the wall (local +y toward the wall, as fixtures' connectors are).
pub fn face_point(store: &Store, f: &FaceHost) -> Option<([f64; 2], f64)> {
    let w = store.find_by_guid(&GlobalId::parse(&f.wall)?).and_then(|id| store.get(id))?;
    let g = crate::walls::regenerate(store, w).ok()?;
    let (d, n) = (g.direction(), g.normal());
    let s = f.side.signum();
    let at = [
        g.start[0] + d[0] * f.along + n[0] * s * g.width / 2.0,
        g.start[1] + d[1] * f.along + n[1] * s * g.width / 2.0,
    ];
    // Facing away from the wall: local +y toward it, against the face's outward normal.
    let out = [n[0] * s, n[1] * s];
    Some((at, out[1].atan2(out[0]) + std::f64::consts::FRAC_PI_2))
}

/// Re-reads a face-hosted copy's place along its wall from its point (after a move), keeping the
/// side.
fn rehost(store: &Store, r: &mut MeshObjectRecipe) {
    let Some(f) = &mut r.face else { return };
    let Some(w) = GlobalId::parse(&f.wall).and_then(|g| store.find_by_guid(&g)).and_then(|id| store.get(id)) else { return };
    let Ok(g) = crate::walls::regenerate(store, w) else { return };
    let d = g.direction();
    f.along = ((r.point[0] - g.start[0]) * d[0] + (r.point[1] - g.start[1]) * d[1]).clamp(0.0, g.length());
}

fn is_one(v: &f64) -> bool {
    *v == 1.0
}

pub fn mesh_object_recipe(e: &Element) -> Option<MeshObjectRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// A decoded mesh: positions (metres, insertion point at the origin), triangles, slots.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub tri_slot: Vec<u16>,
}

fn cache() -> &'static Mutex<HashMap<String, Arc<Decoded>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Decoded>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

impl StoredMesh {
    fn encode(positions: &[[f64; 3]], triangles: &[[u32; 3]], tri_slot: &[u32]) -> Self {
        let p: Vec<u8> = positions
            .iter()
            .flatten()
            .flat_map(|v| (*v as f32).to_le_bytes())
            .collect();
        let t: Vec<u8> = triangles
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let s: Vec<u8> = tri_slot
            .iter()
            .flat_map(|v| (*v as u16).to_le_bytes())
            .collect();
        let (positions, triangles, slots) = (b64::encode(&p), b64::encode(&t), b64::encode(&s));
        let mut h = Sha256::new();
        for part in [&positions, &triangles, &slots] {
            h.update(part.as_bytes());
        }
        Self {
            hash: hex(&h.finalize()),
            positions,
            triangles,
            slots,
        }
    }

    /// The mesh, decoded once per content.
    pub fn decode(&self) -> Result<Arc<Decoded>> {
        if let Some(d) = cache().lock().expect("cache").get(&self.hash) {
            return Ok(d.clone());
        }
        let bad = || {
            Error::new(
                "mesh_object.corrupt",
                "the stored mesh could not be decoded",
            )
        };
        let p = b64::decode(&self.positions).ok_or_else(bad)?;
        let t = b64::decode(&self.triangles).ok_or_else(bad)?;
        let s = b64::decode(&self.slots).ok_or_else(bad)?;
        if p.len() % 12 != 0 || t.len() % 12 != 0 || s.len() * 6 != t.len() {
            return Err(bad());
        }
        let f = |b: &[u8]| f32::from_le_bytes(b.try_into().expect("4"));
        let u = |b: &[u8]| u32::from_le_bytes(b.try_into().expect("4"));
        let d = Decoded {
            positions: p
                .as_chunks::<12>()
                .0
                .iter()
                .map(|c| [f(&c[0..4]), f(&c[4..8]), f(&c[8..12])])
                .collect(),
            triangles: t
                .as_chunks::<12>()
                .0
                .iter()
                .map(|c| [u(&c[0..4]), u(&c[4..8]), u(&c[8..12])])
                .collect(),
            tri_slot: s
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect(),
        };
        let n = d.positions.len() as u32;
        if d.triangles.iter().flatten().any(|i| *i >= n) {
            return Err(bad());
        }
        let d = Arc::new(d);
        cache()
            .lock()
            .expect("cache")
            .insert(self.hash.clone(), d.clone());
        Ok(d)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What an inspection or import found.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Report {
    pub format: String,
    pub triangles: usize,
    pub vertices: usize,
    pub materials: Vec<String>,
    /// Size (x, y, z) in metres with the options given.
    pub size: [f64; 3],
    /// Metres per unit used, and whether the file stated it.
    pub units: f64,
    pub units_from_file: bool,
    pub y_up: bool,
    /// Edges used by one triangle only (holes in the surface).
    pub open_edges: usize,
    /// Edges used by more than two triangles.
    pub non_manifold_edges: usize,
    /// Closed and consistently wound: it has a volume.
    pub solid: bool,
    /// Degenerate triangles dropped and vertices welded by clean-up.
    pub dropped_triangles: usize,
    pub welded_vertices: usize,
}

/// A mesh read and normalised: metres, Z up, the insertion point at the origin.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub tri_slot: Vec<u32>,
    pub slots: Vec<Slot>,
    pub report: Report,
}

/// Normalises a mesh read from a file with the import options.
pub fn prepare(raw: RawMesh, o: &ImportOptions) -> Result<Prepared> {
    let units_from_file = raw.units.is_some() && o.units == Units::Auto;
    let units = o.units.metres().or(raw.units).unwrap_or(1.0);
    let y_up = match o.up_axis {
        UpAxis::Auto => raw.y_up.unwrap_or(false),
        UpAxis::Z => false,
        UpAxis::Y => true,
    };
    let mut positions: Vec<[f64; 3]> = raw
        .positions
        .iter()
        .map(|p| {
            let p = p.map(|v| v * units);
            // Y up to Z up: x stays, y = -z, z = y (a turn about x).
            if y_up { [p[0], -p[2], p[1]] } else { p }
        })
        .collect();
    let mut triangles = raw.triangles;
    let mut tri_slot = raw.tri_slot;
    let (mut dropped, mut welded) = (0, 0);
    if o.clean {
        // Weld: vertices within 1 µm are one.
        let mut map: HashMap<[i64; 3], u32> = HashMap::new();
        let mut remap = Vec::with_capacity(positions.len());
        let mut kept: Vec<[f64; 3]> = Vec::with_capacity(positions.len());
        for p in &positions {
            let key = p.map(|v| (v * 1e6).round() as i64);
            let i = *map.entry(key).or_insert_with(|| {
                kept.push(*p);
                kept.len() as u32 - 1
            });
            remap.push(i);
        }
        welded = positions.len() - kept.len();
        positions = kept;
        let mut t2 = Vec::with_capacity(triangles.len());
        let mut s2 = Vec::with_capacity(triangles.len());
        for (t, s) in triangles.iter().zip(&tri_slot) {
            let t = t.map(|i| remap[i as usize]);
            let area = {
                let [a, b, c] = t.map(|i| positions[i as usize]);
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let n = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
            };
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] || area < 1e-14 {
                dropped += 1;
                continue;
            }
            t2.push(t);
            s2.push(*s);
        }
        triangles = t2;
        tri_slot = s2;
    }
    if triangles.is_empty() {
        return Err(Error::new(
            "mesh.unreadable",
            "the mesh has no triangles left after clean-up",
        ));
    }
    // Only the vertices used.
    let mut used = vec![u32::MAX; positions.len()];
    let mut compact = Vec::new();
    for t in triangles.iter_mut() {
        for i in t.iter_mut() {
            if used[*i as usize] == u32::MAX {
                used[*i as usize] = compact.len() as u32;
                compact.push(positions[*i as usize]);
            }
            *i = used[*i as usize];
        }
    }
    positions = compact;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let shift = match o.insertion {
        Insertion::BottomCentre => [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, lo[2]],
        Insertion::Origin => [0.0; 3],
        Insertion::Corner => lo,
    };
    for p in &mut positions {
        for k in 0..3 {
            p[k] -= shift[k];
        }
    }
    let (open_edges, non_manifold_edges, consistent) = edges(&triangles);
    let report = Report {
        format: raw.format,
        triangles: triangles.len(),
        vertices: positions.len(),
        materials: raw.slots.iter().map(|s| s.name.clone()).collect(),
        size: [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]],
        units,
        units_from_file,
        y_up,
        open_edges,
        non_manifold_edges,
        solid: open_edges == 0 && non_manifold_edges == 0 && consistent,
        dropped_triangles: dropped,
        welded_vertices: welded,
    };
    Ok(Prepared {
        positions,
        triangles,
        tri_slot,
        slots: raw
            .slots
            .into_iter()
            .map(|s| Slot {
                name: s.name,
                color: s.color,
            })
            .collect(),
        report,
    })
}

/// Open edges, non-manifold edges, and whether every shared edge is wound oppositely.
fn edges(triangles: &[[u32; 3]]) -> (usize, usize, bool) {
    let mut count: HashMap<(u32, u32), (u32, i32)> = HashMap::new();
    for t in triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let e = count.entry((a.min(b), a.max(b))).or_insert((0, 0));
            e.0 += 1;
            e.1 += if a < b { 1 } else { -1 };
        }
    }
    let open = count.values().filter(|(n, _)| *n == 1).count();
    let non_manifold = count.values().filter(|(n, _)| *n > 2).count();
    let consistent = count.values().all(|(n, w)| *n != 2 || *w == 0);
    (open, non_manifold, consistent)
}

fn sha256_file(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// The type of a copy.
pub fn type_of(store: &Store, e: &Element) -> Option<(ElementId, MeshObjectType)> {
    let guid = e.param("@type")?.as_str()?;
    let id = find(store, guid)?;
    Some((id, mesh_object_type(store.get(id)?)?))
}

/// Where a copy is: its frame (origin, rotation, mirror) and its type's scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub origin: [f64; 3],
    pub rotation: f64,
    pub mirror: bool,
    pub scale: f64,
}

impl Placement {
    /// A type-space point in project coordinates.
    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let y = if self.mirror { -p[1] } else { p[1] };
        let (s, c) = self.rotation.sin_cos();
        let (x, y, z) = (p[0] * self.scale, y * self.scale, p[2] * self.scale);
        [
            self.origin[0] + x * c - y * s,
            self.origin[1] + x * s + y * c,
            self.origin[2] + z,
        ]
    }

    /// A type-space direction in project coordinates (unit length kept).
    pub fn apply_direction(&self, d: [f64; 3]) -> [f64; 3] {
        let y = if self.mirror { -d[1] } else { d[1] };
        let (s, c) = self.rotation.sin_cos();
        [d[0] * c - y * s, d[0] * s + y * c, d[2]]
    }
}

/// A copy as it is now: its placement, type, and the type's decoded mesh.
#[derive(Debug, Clone)]
pub struct MeshObjectGeometry {
    pub type_id: ElementId,
    pub ty: MeshObjectType,
    pub placement: Placement,
    pub mesh: Arc<Decoded>,
}

impl MeshObjectGeometry {
    /// The copy's mesh per slot, in project coordinates (slots without triangles left out).
    pub fn slot_meshes(&self) -> Vec<(usize, crate::bodies::Mesh)> {
        let mut out = Vec::new();
        for slot in 0..self.ty.slots.len().max(1) {
            let mut positions = Vec::new();
            let mut map: HashMap<u32, u32> = HashMap::new();
            let mut indices = Vec::new();
            for (t, s) in self.mesh.triangles.iter().zip(&self.mesh.tri_slot) {
                if *s as usize != slot {
                    continue;
                }
                let mut tri = [0u32; 3];
                for (k, i) in t.iter().enumerate() {
                    tri[k] = *map.entry(*i).or_insert_with(|| {
                        let p = self.mesh.positions[*i as usize];
                        positions.push(self.placement.apply([
                            p[0] as f64,
                            p[1] as f64,
                            p[2] as f64,
                        ]));
                        positions.len() as u32 - 1
                    });
                }
                // A mirrored copy turns its winding over, so it still faces out.
                if self.placement.mirror {
                    tri.swap(1, 2);
                }
                indices.extend(tri);
            }
            if !indices.is_empty() {
                out.push((slot, (positions, indices)));
            }
        }
        out
    }
}

/// A custom object type (by GlobalId) and its decoded mesh.
pub fn type_geometry(
    store: &Store,
    guid: &str,
) -> Result<(ElementId, MeshObjectType, Arc<Decoded>)> {
    let id = find(store, guid)
        .ok_or_else(|| Error::new("mesh_object.no_type", "the component type no longer exists"))?;
    let ty = mesh_object_type(store.get(id).expect("found"))
        .ok_or_else(|| Error::new("mesh_object.not_a_type", "not a custom object type"))?;
    let mesh = ty.mesh.decode()?;
    Ok((id, ty, mesh))
}

/// A copy as it is now.
pub fn regenerate(store: &Store, e: &Element) -> Result<MeshObjectGeometry> {
    let r = mesh_object_recipe(e)
        .ok_or_else(|| Error::new("mesh_object.not_reviera", "not a Reviera custom object"))?;
    let (type_id, ty) = type_of(store, e).ok_or_else(|| {
        Error::new(
            "mesh_object.no_type",
            "the custom object's type no longer exists",
        )
    })?;
    // On a toposolid, its surface under the point; else the level.
    let surface = r
        .host
        .as_deref()
        .and_then(GlobalId::parse)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|h| store.get(h))
        .and_then(|h| crate::toposolids::height_at(store, h, r.point));
    // Under a ceiling, its underside as it is now.
    let ceiling = r
        .ceiling
        .as_deref()
        .and_then(GlobalId::parse)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|c| store.get(c))
        .and_then(|c| crate::floors::regenerate(store, c).ok())
        .map(|g| g.top - g.thickness);
    let z = match (ceiling, surface) {
        (Some(c), _) => c + r.offset,
        (None, Some(s)) => s + r.offset,
        (None, None) => crate::levels::elevation_of(store, &r.level)? + r.offset,
    };
    let mesh = ty.mesh.decode()?;
    // On a wall face, where the face is now; unhosted (the wall gone), where it was put.
    let (point, rotation) = match r.face.as_ref().and_then(|f| face_point(store, f)) {
        // Its back against the face: a generated box is centred on its insertion point.
        Some((at, rot)) => {
            // Out from the face: against local +y.
            let half = ty.size[1] * ty.scale * r.scale / 2.0;
            ([at[0] + rot.sin() * half, at[1] - rot.cos() * half], rot)
        }
        None => (r.point, r.rotation),
    };
    Ok(MeshObjectGeometry {
        type_id,
        placement: Placement {
            origin: [point[0], point[1], z],
            rotation,
            mirror: r.mirror,
            scale: ty.scale * r.scale,
        },
        ty,
        mesh,
    })
}

/// Every custom object that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, MeshObjectGeometry)> {
    store
        .iter()
        .filter(|e| mesh_object_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

/// Every custom object type, with how many copies it has.
pub fn types(store: &Store) -> Vec<(ElementId, MeshObjectType, usize)> {
    let mut copies: HashMap<String, usize> = HashMap::new();
    for e in store.iter().filter(|e| mesh_object_recipe(e).is_some()) {
        if let Some(g) = e.param("@type").and_then(Value::as_str) {
            *copies.entry(g.to_owned()).or_default() += 1;
        }
    }
    store
        .iter()
        .filter_map(|e| {
            let t = mesh_object_type(e)?;
            let n = copies.get(e.guid().as_str()).copied().unwrap_or(0);
            Some((e.id(), t, n))
        })
        .collect()
}

/// A colour as `#rrggbbaa` (a body part's material: `IfcFurniture:#rrggbbaa`).
pub fn color_hex(c: [f32; 4]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        b(c[0]),
        b(c[1]),
        b(c[2]),
        b(c[3])
    )
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectParams {
    /// The mesh file (OBJ, STL, PLY, glTF, GLB).
    pub path: String,
    #[serde(default)]
    pub options: ImportOptions,
}

/// Reads a mesh file and reports what it holds, without importing it (`mesh_object.inspect`).
pub(crate) fn inspect(_: &Store, p: InspectParams) -> Result<Report> {
    let raw = mesh_io::read(Path::new(&p.path))?;
    Ok(prepare(raw, &p.options)?.report)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    /// The mesh file (OBJ, STL, PLY, glTF, GLB).
    pub path: String,
    /// The type's name; the file's name without extension when omitted.
    pub name: Option<String>,
    #[serde(default)]
    pub category: Category,
    #[serde(default)]
    pub options: ImportOptions,
    /// Allows a mesh above 1,000,000 triangles (up to 5,000,000).
    #[serde(default)]
    pub allow_large: bool,
}

fn read_prepared(path: &Path, o: &ImportOptions, allow_large: bool) -> Result<(Prepared, Source)> {
    let bytes = std::fs::read(path).map_err(|e| {
        Error::new(
            "mesh.not_found",
            format!("could not read {}: {e}", path.display()),
        )
    })?;
    let raw = mesh_io::read(path)?;
    let n = raw.triangles.len();
    if n > MAX_TRIANGLES {
        return Err(Error::new(
            "mesh_object.too_large",
            format!("the mesh has {n} triangles; Reviera imports up to {MAX_TRIANGLES}"),
        ));
    }
    if n > LARGE_TRIANGLES && !allow_large {
        return Err(Error::new(
            "mesh_object.large",
            format!(
                "the mesh has {n} triangles, more than {LARGE_TRIANGLES}: reduce it first, or import with allow_large"
            ),
        ));
    }
    let format = raw.format.clone();
    let prepared = prepare(raw, o)?;
    let source = Source {
        file: path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: std::fs::canonicalize(path)
            .unwrap_or_else(|_| PathBuf::from(path))
            .to_string_lossy()
            .into_owned(),
        format,
        sha256: sha256_file(&bytes),
    };
    Ok((prepared, source))
}

fn type_recipe(
    prepared: &Prepared,
    source: Source,
    category: Category,
    options: ImportOptions,
    scale: f64,
) -> MeshObjectType {
    MeshObjectType {
        kind: TYPE_RECIPE.into(),
        category,
        slots: prepared.slots.clone(),
        source,
        options,
        scale,
        size: prepared.report.size,
        triangle_count: prepared.report.triangles,
        mesh: StoredMesh::encode(&prepared.positions, &prepared.triangles, &prepared.tri_slot),
        library: None,
        connectors: Vec::new(),
        predefined: None,
    }
}

/// Makes a type from a mesh Reviera generated (metres, Z up, insertion point at the origin), one
/// slot of `color`, with `connectors` (MEP starter and box types).
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_generated_type(
    txn: &mut crate::txn::Transaction<'_>,
    name: &str,
    category: Category,
    positions: &[[f64; 3]],
    triangles: &[[u32; 3]],
    color: [f32; 4],
    connectors: Vec<crate::mep::Connector>,
    predefined: Option<&str>,
) -> Result<ElementId> {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for p in positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let recipe = MeshObjectType {
        kind: TYPE_RECIPE.into(),
        category,
        slots: vec![Slot {
            name: "Body".into(),
            color,
        }],
        source: Source {
            file: String::new(),
            path: String::new(),
            format: "generated".into(),
            sha256: String::new(),
        },
        options: ImportOptions::default(),
        scale: 1.0,
        size: [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]],
        triangle_count: triangles.len(),
        mesh: StoredMesh::encode(positions, triangles, &vec![0; triangles.len()]),
        library: None,
        connectors,
        predefined: predefined.map(str::to_owned),
    };
    txn.create(
        NewElement::new(category.type_class())
            .name(name)
            .param("@PredefinedType", predefined.unwrap_or(category.predefined()).to_owned())
            .param(RECIPE_KEY, serde_json::to_value(&recipe).expect("serialises")),
    )
}

/// Imports a mesh file as a custom object type (`mesh_object.import`).
pub(crate) fn import(txn: &mut crate::txn::Transaction<'_>, p: ImportParams) -> Result<ElementId> {
    let path = Path::new(&p.path);
    let (prepared, source) = read_prepared(path, &p.options, p.allow_large)?;
    let name = p.name.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Custom Object".into())
    });
    let recipe = type_recipe(&prepared, source, p.category, p.options, 1.0);
    txn.create(
        NewElement::new(p.category.type_class())
            .name(name)
            .param("@PredefinedType", p.category.predefined())
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReloadParams {
    pub type_id: ElementRef,
    /// Another file; the type's source file when omitted.
    pub path: Option<String>,
    /// New import options; the type's own when omitted.
    pub options: Option<ImportOptions>,
}

fn resolve_type(store: &Store, r: &ElementRef) -> Result<(ElementId, MeshObjectType)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such custom object type"))?;
    let t = mesh_object_type(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "mesh_object.not_a_type",
            format!("element {id} is not a custom object type"),
        )
    })?;
    Ok((id, t))
}

/// Re-reads a type's mesh from its file, or another (`mesh_object.reload`): every copy changes;
/// slot colours are kept where the slot names still match.
pub(crate) fn reload(txn: &mut crate::txn::Transaction<'_>, p: ReloadParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, old) = resolve_type(&store, &p.type_id)?;
    let path = p.path.clone().unwrap_or_else(|| old.source.path.clone());
    if path.is_empty() {
        let from = old.library.as_ref().map_or_else(String::new, |l| {
            format!(" (it came from the library {}, item {})", l.library, l.item)
        });
        return Err(Error::new(
            "mesh_object.no_source",
            format!("the type does not record its file{from}; give the file's path"),
        ));
    }
    let options = p.options.unwrap_or(old.options);
    let (prepared, source) = read_prepared(Path::new(&path), &options, true)?;
    let mut recipe = type_recipe(&prepared, source, old.category, options, old.scale);
    recipe.library = old.library.clone();
    recipe.connectors = old.connectors.clone();
    for s in &mut recipe.slots {
        if let Some(o) = old.slots.iter().find(|o| o.name == s.name) {
            s.color = o.color;
        }
    }
    let value = serde_json::to_value(&recipe).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    pub type_id: ElementRef,
    /// The insertion point in plan (metres).
    pub point: [f64; 2],
    pub level: ElementRef,
    /// Height above the level (metres).
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
    /// Counter-clockwise; accepts units ("90°").
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
    #[serde(default)]
    pub mirror: bool,
    /// Its own uniform scale (times the type's).
    #[serde(default = "one")]
    pub scale: f64,
}

fn level_guid(store: &Store, r: &ElementRef) -> Result<String> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
    let e = store.get(id).expect("resolved");
    if e.class != "IfcBuildingStorey" {
        return Err(Error::new(
            "level.not_a_level",
            format!("element {id} is not a level"),
        ));
    }
    Ok(e.guid().to_string())
}

/// Places a copy of a custom object type (`mesh_object.place`).
pub(crate) fn place(txn: &mut crate::txn::Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (type_id, ty) = resolve_type(&store, &p.type_id)?;
    let recipe = MeshObjectRecipe {
        kind: RECIPE.into(),
        point: p.point,
        level: level_guid(&store, &p.level)?,
        offset: p.offset,
        rotation: p.rotation,
        mirror: p.mirror,
        scale: p.scale,
        host: None,
        face: None,
        ceiling: None,
    };
    if !(p.scale > 0.0 && p.scale.is_finite()) {
        return Err(Error::new(
            "mesh_object.bad_scale",
            "the scale must be above zero",
        ));
    }
    let type_guid = store.get(type_id).expect("resolved").guid().to_string();
    let id = txn.create(
        NewElement::new(ty.category.class())
            .param("@PredefinedType", ty.predefined_type().to_owned())
            .param("@type", type_guid)
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    let store = txn.store().clone();
    regenerate(&store, store.get(id).expect("created"))?;
    crate::mep::connect_coincident(txn, &[id])?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub element_ids: Vec<ElementRef>,
    pub point: Option<[f64; 2]>,
    pub level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<f64>,
    pub mirror: Option<bool>,
    pub scale: Option<f64>,
}

fn copies(store: &Store, refs: &[ElementRef]) -> Result<Vec<(ElementId, MeshObjectRecipe)>> {
    refs.iter()
        .map(|r| {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such element"))?;
            let e = store.get(id).expect("resolved");
            crate::modify::check_unpinned(store, id)?;
            let r = mesh_object_recipe(e).ok_or_else(|| {
                Error::new(
                    "mesh_object.not_reviera",
                    format!("element {id} is not a custom object"),
                )
            })?;
            Ok((id, r))
        })
        .collect()
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, r: &MeshObjectRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    let store = txn.store().clone();
    regenerate(&store, store.get(id).expect("exists")).map(|_| ())
}

/// Changes custom objects (`mesh_object.set`); omitted values are kept.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let level = p
        .level
        .as_ref()
        .map(|l| level_guid(&store, l))
        .transpose()?;
    for (id, mut r) in copies(&store, &p.element_ids)? {
        if let Some(v) = p.point {
            r.point = v;
            rehost(&store, &mut r);
        }
        if let Some(l) = &level {
            r.level = l.clone();
        }
        if let Some(v) = p.offset {
            r.offset = v;
        }
        if let Some(v) = p.rotation {
            r.rotation = v;
        }
        if let Some(v) = p.mirror {
            r.mirror = v;
        }
        if let Some(v) = p.scale {
            if !(v > 0.0 && v.is_finite()) {
                return Err(Error::new(
                    "mesh_object.bad_scale",
                    "the scale must be above zero",
                ));
            }
            r.scale = v;
        }
        write(txn, id, &r)?;
        crate::mep::connect_coincident(txn, &[id])?;
    }
    Ok(())
}

/// A slot's new colour.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SlotColor {
    /// The slot's name.
    pub slot: String,
    pub color: [f32; 4],
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTypeParams {
    pub type_id: ElementRef,
    pub name: Option<String>,
    /// A uniform scale for every copy.
    pub scale: Option<f64>,
    #[serde(default)]
    pub colors: Vec<SlotColor>,
    /// Only while the type has no copies (the category decides the copies' IFC class).
    pub category: Option<Category>,
}

/// Changes a custom object type (`mesh_object.set_type`): its name, scale, slot colours, and its
/// category while it has no copies.
pub(crate) fn set_type(txn: &mut crate::txn::Transaction<'_>, p: SetTypeParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut t) = resolve_type(&store, &p.type_id)?;
    if let Some(s) = p.scale {
        if !(s > 0.0 && s.is_finite()) {
            return Err(Error::new(
                "mesh_object.bad_scale",
                "the scale must be above zero",
            ));
        }
        t.scale = s;
    }
    for c in &p.colors {
        let slot = t
            .slots
            .iter_mut()
            .find(|s| s.name == c.slot)
            .ok_or_else(|| {
                Error::new(
                    "mesh_object.no_slot",
                    format!("the type has no material slot {}", c.slot),
                )
            })?;
        slot.color = c.color.map(|v| v.clamp(0.0, 1.0));
    }
    let mut class = None;
    if let Some(cat) = p.category
        && cat != t.category
    {
        let guid = store.get(id).expect("resolved").guid().to_string();
        let placed = store
            .iter()
            .any(|e| e.param("@type").and_then(Value::as_str) == Some(guid.as_str()));
        if placed {
            return Err(Error::new(
                "mesh_object.category_in_use",
                "the category can only change while the type has no copies",
            ));
        }
        t.category = cat;
        class = Some(cat);
    }
    let value = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
        if let Some(n) = p.name {
            e.name = Some(n);
        }
        if let Some(c) = class {
            e.class = c.type_class().into();
            e.params
                .insert("@PredefinedType".into(), c.predefined().into());
        }
    })?;
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypesParams {}

/// A custom object type, as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TypeInfo {
    pub id: ElementId,
    pub name: String,
    pub category: Category,
    pub triangles: usize,
    pub size: [f64; 3],
    pub scale: f64,
    pub slots: Vec<Slot>,
    pub source_file: String,
    pub copies: usize,
}

/// The project's custom object types (`mesh_object.types`).
pub(crate) fn list_types(store: &Store, _: TypesParams) -> Result<Vec<TypeInfo>> {
    Ok(types(store)
        .into_iter()
        .map(|(id, t, copies)| TypeInfo {
            id,
            name: store
                .get(id)
                .and_then(|e| e.name.clone())
                .unwrap_or_default(),
            category: t.category,
            triangles: t.triangle_count,
            size: t.size,
            scale: t.scale,
            slots: t.slots,
            source_file: t.source.file,
            copies,
        })
        .collect())
}

/// Records the library item a type was loaded from.
pub(crate) fn set_library_link(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    link: crate::library::LibraryLink,
) -> Result<()> {
    let store = txn.store().clone();
    let mut t = mesh_object_type(store.get(id).expect("exists")).expect("a custom object type");
    t.library = Some(link);
    let value = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    Ok(())
}

/// Drops a type's source path (a library-loaded type finds its file through the library).
pub(crate) fn forget_source_path(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
) -> Result<()> {
    let store = txn.store().clone();
    let mut t = mesh_object_type(store.get(id).expect("exists")).expect("a custom object type");
    t.source.path.clear();
    let value = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    Ok(())
}

/// Moves, turns or mirrors a copy (Move, Copy, Rotate, Mirror). `mirror` is the mirror line's
/// angle.
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    turn: f64,
    mirror: Option<f64>,
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = mesh_object_recipe(store.get(id).expect("exists")).expect("a custom object");
    // A face-hosted copy moves from where it is drawn, and stays on its face.
    if let Some((at, rot)) = r.face.as_ref().and_then(|f| face_point(&store, f)) {
        r.point = at;
        r.rotation = rot;
    }
    r.point = x(r.point);
    rehost(&store, &mut r);
    match mirror {
        Some(axis) => {
            r.rotation = 2.0 * axis - r.rotation;
            r.mirror = !r.mirror;
        }
        None => r.rotation += turn,
    }
    r.offset += dz;
    write(txn, id, &r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> RawMesh {
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let quads = [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [1, 2, 6, 5],
            [2, 3, 7, 6],
            [3, 0, 4, 7],
        ];
        let triangles: Vec<[u32; 3]> = quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect();
        RawMesh {
            format: "test".into(),
            tri_slot: vec![0; triangles.len()],
            triangles,
            positions,
            slots: vec![mesh_io::RawSlot {
                name: "Default".into(),
                color: [1.0; 4],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn cube_is_solid_and_placed_at_its_bottom_centre() {
        let o = ImportOptions {
            units: Units::Cm,
            ..Default::default()
        };
        let p = prepare(cube(), &o).unwrap();
        assert!(p.report.solid, "{:?}", p.report);
        assert!((p.report.size[0] - 0.01).abs() < 1e-12);
        let lo_z = p
            .positions
            .iter()
            .map(|q| q[2])
            .fold(f64::INFINITY, f64::min);
        assert!(lo_z.abs() < 1e-12);
        let cx: f64 = p.positions.iter().map(|q| q[0]).sum::<f64>() / p.positions.len() as f64;
        assert!(cx.abs() < 1e-12);
    }

    #[test]
    fn y_up_turns_upright_and_open_edges_counted() {
        let mut m = cube();
        m.triangles.truncate(10); // two faces missing
        m.tri_slot.truncate(10);
        m.y_up = Some(true);
        let p = prepare(m, &ImportOptions::default()).unwrap();
        assert!(!p.report.solid);
        assert!(p.report.open_edges > 0);
        assert!(p.report.y_up);
    }

    #[test]
    fn stored_mesh_round_trips() {
        let p = prepare(cube(), &ImportOptions::default()).unwrap();
        let s = StoredMesh::encode(&p.positions, &p.triangles, &p.tri_slot);
        let d = s.decode().unwrap();
        assert_eq!(d.triangles.len(), 12);
        assert_eq!(d.positions.len(), 8);
    }

    #[test]
    fn placement_mirrors_and_turns() {
        let pl = Placement {
            origin: [10.0, 0.0, 3.0],
            rotation: std::f64::consts::FRAC_PI_2,
            mirror: true,
            scale: 2.0,
        };
        let q = pl.apply([1.0, 1.0, 1.0]);
        // Mirrored: (1, -1); doubled: (2, -2); turned 90°: (2, 2); moved.
        assert!(
            (q[0] - 12.0).abs() < 1e-9 && (q[1] - 2.0).abs() < 1e-9 && (q[2] - 5.0).abs() < 1e-9
        );
    }
}
