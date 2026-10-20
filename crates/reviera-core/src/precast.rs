// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Precast configuration and segmentation (STR-PRE-001, STR-PRE-002).
//!
//! **Configuration.** One versioned document (`rxd:fabrication` on the project) holds the
//! precast rules as a tree: concrete walls (solid, double) and slabs (solid, hollow-core,
//! lattice-girder), each with segmentation, reinforcement, connections and shop-drawing nodes;
//! annotations, dimensioning, CAM export and the data folder. A dotted path addresses a value
//! (`wall.solid.segmentation.max_length`); every change is validated, naming the rule it breaks,
//! and raises the version. The whole document is exported and imported as a JSON file with a
//! schema version.
//!
//! **Split.** A straight wall or a floor is divided into pieces by the parts engine (ARC-PART):
//! the fewest pieces that keep every piece within the length (or width), height and weight
//! limits, joints the configured gap wide and no joint nearer an opening than the configured
//! distance; pieces as even as those rules allow. Each piece (the layer parts of that stretch)
//! goes in an `IfcElementAssembly` (USERDEFINED `PRECAST_ASSEMBLY`) with its piece mark, the
//! configuration version and a hash of the rules used. Weights are volume × the configured
//! density (2.5 t/m³ by default: reinforced concrete).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::NewElement;

pub const CONFIG_KEY: &str = "rxd:fabrication";
pub const ASSEMBLY_RECIPE: &str = "recipe.precast_assembly";
const SCHEMA: u32 = 1;

// ---- Configuration ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct WallSegmentation {
    pub min_length: f64,
    pub max_length: f64,
    pub min_height: f64,
    pub max_height: f64,
    /// Tonnes.
    pub max_weight: f64,
    pub joint_gap_vertical: f64,
    pub joint_gap_horizontal: f64,
    pub min_opening_distance: f64,
    pub profile_overlap: f64,
}

impl Default for WallSegmentation {
    fn default() -> Self {
        Self {
            min_length: 0.6,
            max_length: 8.0,
            min_height: 0.3,
            max_height: 4.0,
            max_weight: 12.0,
            joint_gap_vertical: 0.02,
            joint_gap_horizontal: 0.02,
            min_opening_distance: 0.2,
            profile_overlap: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SlabSegmentation {
    pub min_width: f64,
    pub max_width: f64,
    /// Tonnes.
    pub max_weight: f64,
    pub joint_gap: f64,
    /// Hollow-core: the slab component, division profile and strand diameter.
    pub family: String,
    pub division_profile: String,
    pub strand_diameter: f64,
}

impl Default for SlabSegmentation {
    fn default() -> Self {
        Self { min_width: 0.6, max_width: 2.5, max_weight: 12.0, joint_gap: 0.02, family: String::new(), division_profile: String::new(), strand_diameter: 0.0 }
    }
}

/// One kind of piece: its segmentation and the nodes the later steps fill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, bound(deserialize = "S: Deserialize<'de> + Default"))]
#[schemars(bound = "S: JsonSchema + Default")]
pub struct KindConfig<S> {
    pub segmentation: S,
    pub reinforcement: Value,
    pub connections: Value,
    pub shop_drawing: Value,
}

impl<S: Default> Default for KindConfig<S> {
    fn default() -> Self {
        Self { segmentation: S::default(), reinforcement: json!({}), connections: json!({}), shop_drawing: json!({}) }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct WallKinds {
    pub solid: KindConfig<WallSegmentation>,
    pub double: KindConfig<WallSegmentation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SlabKinds {
    pub solid: KindConfig<SlabSegmentation>,
    pub hollow_core: KindConfig<SlabSegmentation>,
    pub lattice_girder: KindConfig<SlabSegmentation>,
}

impl Default for SlabKinds {
    fn default() -> Self {
        let mut hollow = KindConfig::<SlabSegmentation>::default();
        hollow.segmentation.min_width = 0.3;
        hollow.segmentation.max_width = 1.2;
        hollow.segmentation.strand_diameter = 0.0125;
        Self { solid: KindConfig::default(), hollow_core: hollow, lattice_girder: KindConfig::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CamConfig {
    pub format: String,
    pub folder: String,
    /// `{mark}`, `{project}`, `{source}` and `{index}` are filled in.
    pub filename: String,
}

impl Default for CamConfig {
    fn default() -> Self {
        Self { format: "unitechnik".into(), folder: String::new(), filename: "{mark}".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Config {
    /// Raised by every change.
    pub version: u32,
    /// Concrete density (t/m³).
    pub density: f64,
    /// Re-create mounting parts and reinforcement when pieces change (STR-PRE-002).
    pub auto_update: bool,
    pub wall: WallKinds,
    pub slab: SlabKinds,
    pub annotations: Value,
    pub dimensioning: Value,
    pub cam: CamConfig,
    pub data_folder: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            density: 2.5,
            auto_update: true,
            wall: WallKinds::default(),
            slab: SlabKinds::default(),
            annotations: json!({}),
            dimensioning: json!({}),
            cam: CamConfig::default(),
            data_folder: String::new(),
        }
    }
}

/// The project's configuration (the default until one is set).
pub fn config(store: &Store) -> Config {
    store
        .iter()
        .find(|e| e.class == "IfcProject")
        .and_then(|p| p.param(CONFIG_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// What is wrong with a configuration: each broken rule by the path it is on.
pub fn problems(c: &Config) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut bad = |path: &str, rule: &str| out.push((path.to_owned(), rule.to_owned()));
    if c.density <= 0.0 {
        bad("density", "the density must be more than 0");
    }
    for (kind, w) in [("solid", &c.wall.solid.segmentation), ("double", &c.wall.double.segmentation)] {
        let p = |k: &str| format!("wall.{kind}.segmentation.{k}");
        for (k, v) in [("min_length", w.min_length), ("max_length", w.max_length), ("min_height", w.min_height), ("max_height", w.max_height)] {
            if v <= 0.0 {
                bad(&p(k), "lengths and heights must be more than 0");
            }
        }
        if w.min_length > w.max_length {
            bad(&p("min_length"), "the minimum length must not be more than the maximum");
        }
        if w.min_height > w.max_height {
            bad(&p("min_height"), "the minimum height must not be more than the maximum");
        }
        if w.max_weight <= 0.0 {
            bad(&p("max_weight"), "the maximum weight must be more than 0 t");
        }
        for (k, v) in [("joint_gap_vertical", w.joint_gap_vertical), ("joint_gap_horizontal", w.joint_gap_horizontal)] {
            if !(0.0..=0.1).contains(&v) {
                bad(&p(k), "joint gaps are 0–100 mm");
            }
        }
        if w.min_opening_distance < 0.0 || w.profile_overlap < 0.0 {
            bad(&p("min_opening_distance"), "distances cannot be negative");
        }
    }
    for (kind, s) in [("solid", &c.slab.solid.segmentation), ("hollow_core", &c.slab.hollow_core.segmentation), ("lattice_girder", &c.slab.lattice_girder.segmentation)] {
        let p = |k: &str| format!("slab.{kind}.segmentation.{k}");
        if s.min_width <= 0.0 || s.max_width <= 0.0 {
            bad(&p("max_width"), "widths must be more than 0");
        }
        if s.min_width > s.max_width {
            bad(&p("min_width"), "the minimum width must not be more than the maximum");
        }
        if s.max_weight <= 0.0 {
            bad(&p("max_weight"), "the maximum weight must be more than 0 t");
        }
        if !(0.0..=0.1).contains(&s.joint_gap) {
            bad(&p("joint_gap"), "joint gaps are 0–100 mm");
        }
        if s.strand_diameter < 0.0 {
            bad(&p("strand_diameter"), "the strand diameter cannot be negative");
        }
    }
    // The filename rule: balanced braces around known names.
    let mut rest = c.cam.filename.as_str();
    while let Some(i) = rest.find('{') {
        match rest[i..].find('}') {
            Some(j) if ["mark", "project", "source", "index"].contains(&&rest[i + 1..i + j]) => rest = &rest[i + j + 1..],
            _ => {
                bad("cam.filename", "the filename rule takes {mark}, {project}, {source} and {index}, each in braces");
                break;
            }
        }
    }
    if c.cam.filename.contains('}') && !c.cam.filename.contains('{') {
        bad("cam.filename", "the filename rule takes {mark}, {project}, {source} and {index}, each in braces");
    }
    if !c.data_folder.is_empty() && !std::path::Path::new(&c.data_folder).is_dir() {
        bad("data_folder", "the data folder must exist and be a folder");
    }
    out
}

fn checked(c: &Config) -> Result<()> {
    match problems(c).first() {
        Some((path, rule)) => Err(Error::new("precast_config.invalid", format!("{path}: {rule}")).with_details(json!({ "path": path }))),
        None => Ok(()),
    }
}

fn save(txn: &mut Transaction<'_>, c: &Config) -> Result<()> {
    let store = txn.store().clone();
    let pid = store.iter().find(|e| e.class == "IfcProject").map(|e| e.id()).ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    txn.set_param(pid, CONFIG_KEY, serde_json::to_value(c).expect("serialises"))
}

fn node<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(v);
    }
    path.split('.').try_fold(v, |v, k| v.get(k))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetParams {
    /// A dotted path (`wall.solid.segmentation.max_length`); empty for the whole configuration.
    #[serde(default)]
    pub path: String,
}

/// A configuration value (`precast_config.get`).
pub fn get(store: &Store, p: GetParams) -> Result<Value> {
    let v = serde_json::to_value(config(store)).expect("serialises");
    node(&v, &p.path).cloned().ok_or_else(|| Error::new("precast_config.path", format!("no configuration value {}", p.path)))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetParams {
    pub path: String,
    pub value: Value,
}

/// Sets a configuration value (`precast_config.set`), refused with the rule it breaks.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<u32> {
    let store = txn.store().clone();
    let current = config(&store);
    let mut v = serde_json::to_value(&current).expect("serialises");
    let (head, last) = p.path.rsplit_once('.').unwrap_or(("", p.path.as_str()));
    if last == "version" && head.is_empty() {
        return Err(Error::new("precast_config.path", "the version is raised by each change, not set"));
    }
    let parent = if head.is_empty() { Some(&mut v) } else { head.split('.').try_fold(&mut v, |v, k| v.get_mut(k)) };
    let slot = parent
        .and_then(|o| o.as_object_mut())
        .and_then(|o| o.get_mut(last))
        .ok_or_else(|| Error::new("precast_config.path", format!("no configuration value {}", p.path)))?;
    let same_kind = matches!((&*slot, &p.value), (Value::Number(_), Value::Number(_)) | (Value::String(_), Value::String(_)) | (Value::Bool(_), Value::Bool(_)))
        || slot.is_object()
        || slot.is_null();
    if !same_kind {
        return Err(Error::new("precast_config.type", format!("{} takes a {}", p.path, match slot { Value::Number(_) => "number", Value::String(_) => "text", Value::Bool(_) => "true or false", _ => "value" })));
    }
    *slot = p.value.clone();
    let mut c: Config = serde_json::from_value(v).map_err(|e| Error::new("precast_config.type", format!("{}: {e}", p.path)))?;
    checked(&c)?;
    c.version = current.version + 1;
    save(txn, &c)?;
    Ok(c.version)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ApplyParams {
    /// The whole configuration, as the dialog edits it.
    pub config: Config,
}

/// Replaces the configuration (`precast_config.apply`): the dialog's OK, as one step.
pub(crate) fn apply(txn: &mut Transaction<'_>, p: ApplyParams) -> Result<u32> {
    let current = config(txn.store());
    let mut c = p.config;
    checked(&c)?;
    c.version = current.version + 1;
    save(txn, &c)?;
    Ok(c.version)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FileParams {
    pub file: String,
}

/// Writes the configuration to a JSON file (`precast_config.export`).
pub fn export(store: &Store, p: FileParams) -> Result<()> {
    let doc = json!({ "schema": SCHEMA, "kind": "reviera.precast_configuration", "configuration": config(store) });
    std::fs::write(&p.file, serde_json::to_string_pretty(&doc).expect("serialises")).map_err(|e| Error::new("precast_config.write", format!("{}: {e}", p.file)))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Imported {
    pub version: u32,
    /// Components it names that the project does not have.
    pub missing: Vec<String>,
}

/// Replaces the configuration with a file's (`precast_config.import`).
pub(crate) fn import(txn: &mut Transaction<'_>, p: FileParams) -> Result<Imported> {
    let text = std::fs::read_to_string(&p.file).map_err(|e| Error::new("precast_config.read", format!("{}: {e}", p.file)))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| Error::new("precast_config.read", format!("{}: {e}", p.file)))?;
    if v.get("kind").and_then(Value::as_str) != Some("reviera.precast_configuration") {
        return Err(Error::new("precast_config.read", "the file is not a Reviera precast configuration"));
    }
    if v.get("schema").and_then(Value::as_u64).is_some_and(|s| s > u64::from(SCHEMA)) {
        return Err(Error::new("precast_config.read", "the file is from a newer Reviera"));
    }
    let mut c: Config = serde_json::from_value(v.get("configuration").cloned().unwrap_or_default()).map_err(|e| Error::new("precast_config.read", e.to_string()))?;
    checked(&c)?;
    let store = txn.store().clone();
    c.version = config(&store).version + 1;
    let names: Vec<&str> = [&c.slab.solid, &c.slab.hollow_core, &c.slab.lattice_girder]
        .iter()
        .flat_map(|k| [k.segmentation.family.as_str(), k.segmentation.division_profile.as_str()])
        .filter(|n| !n.is_empty())
        .collect();
    let missing = names.into_iter().filter(|n| !store.iter().any(|e| e.name.as_deref() == Some(*n))).map(str::to_owned).collect();
    save(txn, &c)?;
    Ok(Imported { version: c.version, missing })
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Affected {
    pub version: u32,
    /// Assemblies made with an older configuration.
    pub older: Vec<ElementId>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AffectedParams {}

pub fn affected_cmd(store: &Store, _p: AffectedParams) -> Result<Affected> {
    Ok(affected(store))
}

/// Which assemblies an older configuration made (`precast_config.affected`).
pub fn affected(store: &Store) -> Affected {
    let v = config(store).version;
    Affected { version: v, older: store.iter().filter(|e| assembly_recipe(e).is_some_and(|r| r.version < v)).map(|e| e.id()).collect() }
}

// ---- Assemblies ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PieceKind {
    SolidWall,
    DoubleWall,
    SolidSlab,
    HollowCore,
    LatticeGirder,
}

impl PieceKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::SolidWall => "Solid Wall",
            Self::DoubleWall => "Double Wall",
            Self::SolidSlab => "Solid Slab",
            Self::HollowCore => "Hollow-core Slab",
            Self::LatticeGirder => "Lattice-girder Slab",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AssemblyRecipe {
    pub kind: String,
    /// The wall or floor split (GlobalId).
    pub source: String,
    pub piece: PieceKind,
    /// The parts of this piece (GlobalIds).
    pub parts: Vec<String>,
    pub mark: String,
    /// Along the source, from 0; and the row, from the bottom.
    pub index: usize,
    #[serde(default)]
    pub row: usize,
    /// The configuration version and a hash of the rules used.
    pub version: u32,
    pub rules: String,
    /// The divisions the split made, and whether it made the source's first parts.
    pub divisions: Vec<String>,
    pub created_parts: bool,
}

pub fn assembly_recipe(e: &Element) -> Option<AssemblyRecipe> {
    let v = e.param(crate::walls::RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == ASSEMBLY_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// Whether an element goes with `guid`: an assembly of it or of one of its parts.
pub fn uses(e: &Element, guid: &str) -> bool {
    assembly_recipe(e).is_some_and(|r| r.source == guid || r.parts.iter().any(|p| p == guid))
}

/// The assembly a part is in, if any.
pub fn assembly_of(store: &Store, part: &str) -> Option<ElementId> {
    store.iter().find(|e| assembly_recipe(e).is_some_and(|r| r.parts.iter().any(|p| p == part))).map(|e| e.id())
}

fn fxhash(s: &str) -> String {
    format!("{:016x}", s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)))
}

// ---- Joints ----

/// Where to put the joints along a run `length` long: the fewest pieces with every piece no
/// shorter than `min`, no longer than `max`, no heavier than `max_weight` (`weight(a, b)`), and
/// no joint inside a forbidden interval; as even as those allow.
pub fn joints(length: f64, min: f64, max: f64, max_weight: f64, weight: &dyn Fn(f64, f64) -> f64, forbidden: &[(f64, f64)]) -> std::result::Result<Vec<f64>, String> {
    let allowed = |x: f64| x > 1e-9 && x < length - 1e-9 && !forbidden.iter().any(|(a, b)| x > *a + 1e-9 && x < *b - 1e-9);
    let fits = |a: f64, b: f64| b - a <= max + 1e-9 && b - a >= min - 1e-9 && weight(a, b) <= max_weight + 1e-9;
    let fits_all = |xs: &[f64]| {
        let mut ends = vec![0.0];
        ends.extend_from_slice(xs);
        ends.push(length);
        ends.windows(2).all(|w| fits(w[0], w[1])) && xs.iter().all(|x| allowed(*x))
    };
    if fits(0.0, length) {
        return Ok(Vec::new());
    }
    // Greedy: each joint as far along as the limits allow, stepping back off forbidden spans.
    let mut greedy = Vec::new();
    let mut s = 0.0;
    while !fits(s, length) {
        let mut e = (s + max).min(length);
        // The weight grows along the run: back off to the heaviest that fits.
        if weight(s, e) > max_weight {
            let (mut lo, mut hi) = (s, e);
            for _ in 0..60 {
                let m = (lo + hi) / 2.0;
                if weight(s, m) <= max_weight { lo = m } else { hi = m }
            }
            e = lo;
        }
        // Off any forbidden span, toward the start.
        while let Some((a, _)) = forbidden.iter().find(|(a, b)| e > *a + 1e-9 && e < *b - 1e-9) {
            e = *a;
        }
        if e - s < min - 1e-9 || e <= s {
            return Err(format!("no joint fits between {:.0} and {:.0} mm along it: openings, the weight limit and the minimum length leave no place", s * 1000.0, (s + max) * 1000.0));
        }
        greedy.push(e);
        s = e;
    }
    // The last piece too short: move the last joint back.
    if length - s < min - 1e-9
        && let Some(last) = greedy.last_mut()
    {
        *last = length - min;
    }
    if !fits_all(&greedy) {
        return Err("the pieces cannot satisfy the length, weight and opening rules together".into());
    }
    // As even as allowed: equal shares moved to the nearest allowed place.
    let n = greedy.len() + 1;
    let even: Vec<f64> = (1..n)
        .map(|k| {
            let x = length * k as f64 / n as f64;
            match forbidden.iter().find(|(a, b)| x > *a + 1e-9 && x < *b - 1e-9) {
                Some((a, b)) => if x - a <= b - x && *a > 1e-9 { *a } else { *b },
                None => x,
            }
        })
        .collect();
    Ok(if fits_all(&even) { even } else { greedy })
}

// ---- Split ----

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SplitParams {
    pub element_ids: Vec<ElementRef>,
    /// Floors: `solid` (default), `hollow_core` or `lattice_girder`. Walls: `solid` (default) or
    /// `double`.
    #[serde(default)]
    pub slab_kind: Option<String>,
}

/// A division pass: cutting lines (start, end), the sketch normal and the gap.
type Pass = (Vec<(V3, V3)>, V3, f64);

struct Plan {
    piece: PieceKind,
    /// Cutting lines (start, end) with the sketch normal, and the gap, per division pass.
    passes: Vec<Pass>,
    rules: String,
    /// The direction pieces are counted along (plan).
    along: [f64; 2],
}

type V3 = [f64; 3];

fn wall_plan(store: &Store, e: &Element, c: &Config, double: bool) -> Result<Plan> {
    let w = crate::walls::wall_recipe(e).ok_or_else(|| Error::new("precast.source", "not a wall"))?;
    if !w.through.is_empty() {
        return Err(Error::new("precast.source", "curved walls are not split yet"));
    }
    let g = crate::walls::regenerate(store, e)?;
    let kind = if double { &c.wall.double } else { &c.wall.solid };
    let s = &kind.segmentation;
    let (l, h, t) = (g.length(), g.height, g.width);
    let cuts = crate::inserts::cuts_by_host(store).remove(&e.id()).unwrap_or_default();
    // Rows of equal height, the fewest within the maximum.
    let rows = ((h / s.max_height) - 1e-9).ceil().max(1.0) as usize;
    let row_h = h / rows as f64;
    if row_h < s.min_height - 1e-9 {
        return Err(Error::new("precast.unsplittable", format!("the wall's {:.0} mm height cannot make rows between the minimum and maximum height", h * 1000.0)));
    }
    let open_area = |a: f64, b: f64, z0: f64, z1: f64| -> f64 {
        cuts.iter()
            .map(|c| ((c.x[1].min(b) - c.x[0].max(a)).max(0.0)) * ((c.z[1].min(z1) - c.z[0].max(z0)).max(0.0)))
            .sum()
    };
    let density = c.density;
    let weight = |a: f64, b: f64| -> f64 {
        (0..rows)
            .map(|r| {
                let (z0, z1) = (g.base_z + row_h * r as f64, g.base_z + row_h * (r + 1) as f64);
                ((b - a) * row_h - open_area(a, b, z0, z1)) * t * density
            })
            .fold(0.0, f64::max)
    };
    let forbidden: Vec<(f64, f64)> = cuts.iter().map(|c| (c.x[0] - s.min_opening_distance, c.x[1] + s.min_opening_distance)).collect();
    let xs = joints(l, s.min_length, s.max_length, s.max_weight, &weight, &forbidden).map_err(|m| Error::new("precast.unsplittable", m))?;
    let (dir, nrm) = (g.direction(), g.normal());
    let at = |x: f64, z: f64| [g.start[0] + dir[0] * x, g.start[1] + dir[1] * x, z];
    let mut passes = Vec::new();
    if !xs.is_empty() {
        passes.push((xs.iter().map(|x| (at(*x, g.base_z), at(*x, g.base_z + h))).collect(), [nrm[0], nrm[1], 0.0], s.joint_gap_vertical));
    }
    if rows > 1 {
        let zs: Vec<f64> = (1..rows).map(|r| g.base_z + row_h * r as f64).collect();
        passes.push((zs.iter().map(|z| (at(0.0, *z), at(l, *z))).collect(), [nrm[0], nrm[1], 0.0], s.joint_gap_horizontal));
    }
    Ok(Plan { piece: if double { PieceKind::DoubleWall } else { PieceKind::SolidWall }, passes, rules: fxhash(&serde_json::to_string(kind).expect("serialises")), along: dir })
}

fn slab_plan(store: &Store, e: &Element, c: &Config, kind: PieceKind) -> Result<Plan> {
    let g = crate::floors::regenerate(store, e)?;
    let k = match kind {
        PieceKind::HollowCore => &c.slab.hollow_core,
        PieceKind::LatticeGirder => &c.slab.lattice_girder,
        _ => &c.slab.solid,
    };
    let s = &k.segmentation;
    let profiles = crate::slab_openings::cut_profiles(store, e, &g.profiles);
    let outer = &g.profiles.first().ok_or_else(|| Error::new("precast.source", "the floor has no outline"))?.outer;
    // The pieces are strips along the span, side by side along the outline's longest edge.
    let (mut axis, mut best) = ([1.0, 0.0], 0.0);
    for i in 0..outer.len() {
        let (a, b) = (outer[i], outer[(i + 1) % outer.len()]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let l = d[0].hypot(d[1]);
        if l > best {
            best = l;
            axis = [d[0] / l, d[1] / l];
        }
    }
    let proj = |p: [f64; 2]| p[0] * axis[0] + p[1] * axis[1];
    let (lo, hi) = outer.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(proj(*p)), h.max(proj(*p))));
    let perp = [-axis[1], axis[0]];
    let (plo, phi) = outer.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| {
        let v = p[0] * perp[0] + p[1] * perp[1];
        (l.min(v), h.max(v))
    });
    let shapes: Vec<crate::polygons::Shape> = profiles.iter().map(|p| crate::polygons::Shape { outer: p.outer.clone(), holes: p.holes.clone() }).collect();
    let density = c.density;
    let thickness = g.thickness;
    let weight = |a: f64, b: f64| -> f64 {
        let at = |u: f64, v: f64| [axis[0] * (lo + u) + perp[0] * v, axis[1] * (lo + u) + perp[1] * v];
        let strip = crate::polygons::Shape { outer: vec![at(a, plo - 1.0), at(b, plo - 1.0), at(b, phi + 1.0), at(a, phi + 1.0)], holes: Vec::new() };
        let area: f64 = crate::polygons::intersection(&shapes, &[strip])
            .iter()
            .map(|s| crate::polygons::area(&s.outer).abs() - s.holes.iter().map(|h| crate::polygons::area(h).abs()).sum::<f64>())
            .sum();
        area * thickness * density
    };
    let length = hi - lo;
    let xs = if kind == PieceKind::HollowCore {
        // Standard widths, the last piece the rest (or the last two shared if it is too narrow).
        let n = ((length / s.max_width) - 1e-9).ceil().max(1.0) as usize;
        let mut xs: Vec<f64> = (1..n).map(|k| s.max_width * k as f64).collect();
        if let Some(last) = xs.last().copied()
            && length - last < s.min_width
        {
            let start = if xs.len() >= 2 { xs[xs.len() - 2] } else { 0.0 };
            *xs.last_mut().expect("some") = (start + length) / 2.0;
        }
        let mut ends = vec![0.0];
        ends.extend_from_slice(&xs);
        ends.push(length);
        if let Some(w) = ends.windows(2).find(|w| weight(w[0], w[1]) > s.max_weight + 1e-9) {
            return Err(Error::new("precast.unsplittable", format!("a {:.0} mm hollow-core piece weighs {:.1} t, over the {:.1} t limit", (w[1] - w[0]) * 1000.0, weight(w[0], w[1]), s.max_weight)));
        }
        xs
    } else {
        joints(length, s.min_width, s.max_width, s.max_weight, &weight, &[]).map_err(|m| Error::new("precast.unsplittable", m))?
    };
    let z = g.top;
    let at = |u: f64, v: f64| [axis[0] * (lo + u) + perp[0] * v, axis[1] * (lo + u) + perp[1] * v, z];
    let passes = if xs.is_empty() { Vec::new() } else { vec![(xs.iter().map(|u| (at(*u, plo - 1.0), at(*u, phi + 1.0))).collect(), [0.0, 0.0, 1.0], s.joint_gap)] };
    Ok(Plan { piece: kind, passes, rules: fxhash(&serde_json::to_string(k).expect("serialises")), along: axis })
}

/// A piece's cells in the split's divisions (the same for each layer's part of it).
fn cells_key(store: &Store, part: &crate::parts::PartRecipe, divisions: &[String]) -> Vec<String> {
    let mut out = vec![String::new(); divisions.len()];
    let mut d = part.division.clone();
    let mut depth = 0;
    while let Some(div) = d {
        if let Some(k) = divisions.iter().position(|x| *x == div.id) {
            out[k] = div.cell.clone();
        }
        d = by_guid(store, &div.parent).and_then(crate::parts::recipe).and_then(|r| r.division);
        depth += 1;
        if depth > 16 {
            break;
        }
    }
    out
}

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i))
}

/// Splits walls and floors into precast pieces (`precast.split`).
pub(crate) fn split(txn: &mut Transaction<'_>, p: SplitParams) -> Result<Vec<ElementId>> {
    let mut out = Vec::new();
    for r in &p.element_ids {
        let store = txn.store().clone();
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        let guid = e.guid().to_string();
        let existing: Vec<ElementId> = store.iter().filter(|a| assembly_recipe(a).is_some_and(|x| x.source == guid)).map(|a| a.id()).collect();
        if !existing.is_empty() {
            return Err(Error::new(
                "precast.already_split",
                format!("element {id} is already split into assemblies {}: re-split those instead", existing.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ")),
            )
            .with_details(json!({ "assemblies": existing })));
        }
        let c = config(&store);
        let kind = p.slab_kind.as_deref().unwrap_or("solid");
        let plan = if crate::walls::wall_recipe(e).is_some() {
            match kind {
                "solid" | "double" => wall_plan(&store, e, &c, kind == "double")?,
                k => return Err(Error::new("precast.kind", format!("a wall is split as solid or double, not {k}"))),
            }
        } else if crate::floors::slab_recipe(e).is_some_and(|(k, _)| k == crate::floors::SlabKind::Floor) {
            let piece = match kind {
                "solid" => PieceKind::SolidSlab,
                "hollow_core" => PieceKind::HollowCore,
                "lattice_girder" => PieceKind::LatticeGirder,
                k => return Err(Error::new("precast.kind", format!("a floor is split as solid, hollow_core or lattice_girder, not {k}"))),
            };
            slab_plan(&store, e, &c, piece)?
        } else {
            return Err(Error::new("precast.source", format!("element {id} is not a wall or floor")));
        };
        // Its first parts: the existing ones, else made now.
        let had = crate::parts::has_parts(&store, e);
        if !had {
            crate::parts::create(txn, crate::parts::CreateParams { host_ids: vec![ElementRef::Id(id)] })?;
        }
        let mut leaves: Vec<ElementId> = crate::parts::leaves(txn.store()).into_iter().filter(|(_, r)| r.source == guid).map(|(i, _)| i).collect();
        let mut divisions = Vec::new();
        for (lines, normal, gap) in &plan.passes {
            let d = crate::parts::divide(
                txn,
                crate::parts::DivideParams {
                    part_ids: leaves.iter().map(|i| ElementRef::Id(*i)).collect(),
                    lines: lines.iter().map(|(a, b)| crate::parts::LineArg { start: *a, end: *b }).collect(),
                    normal: Some(*normal),
                    refs: Vec::new(),
                    gap: Some(*gap),
                },
            )?;
            divisions.push(d.division_id.clone());
            leaves = crate::parts::leaves(txn.store()).into_iter().filter(|(_, r)| r.source == guid).map(|(i, _)| i).collect();
        }
        // The pieces: the leaf parts grouped by their cells, ordered up by row and then along.
        let store = txn.store().clone();
        let bodies = crate::bodies::all_but_reinforcement(&store);
        let mut groups: BTreeMap<Vec<String>, (Vec<String>, V3)> = BTreeMap::new();
        for pid in &leaves {
            let pe = store.get(*pid).expect("listed");
            let pr = crate::parts::recipe(pe).expect("a part");
            let mesh = crate::parts::geometry(&store, pe, &bodies);
            let pts: Vec<V3> = mesh.iter().flat_map(|m| m.0.iter().copied()).collect();
            let mid = if pts.is_empty() { [0.0; 3] } else { pts.iter().fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]).map(|v| v / pts.len() as f64) };
            let g = groups.entry(cells_key(&store, &pr, &divisions)).or_insert_with(|| (Vec::new(), mid));
            g.0.push(pe.guid().to_string());
        }
        let along = plan.along;
        let mut zs: Vec<f64> = groups.values().map(|g| g.1[2]).collect();
        zs.sort_by(f64::total_cmp);
        zs.dedup_by(|a, b| (*a - *b).abs() < 0.05);
        let row_of = |z: f64| zs.iter().position(|r| (r - z).abs() < 0.05).unwrap_or(0);
        let mut ordered: Vec<(usize, f64, Vec<String>)> = groups.into_values().map(|(parts, mid)| (row_of(mid[2]), mid[0] * along[0] + mid[1] * along[1], parts)).collect();
        ordered.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut placed: Vec<((usize, usize), Vec<String>)> = Vec::new();
        for (row, _, parts) in ordered {
            let i = placed.iter().filter(|((_, r), _)| *r == row).count();
            placed.push(((i, row), parts));
        }
        let letter = if matches!(plan.piece, PieceKind::SolidWall | PieceKind::DoubleWall) { "W" } else { "S" };
        let sources: std::collections::BTreeSet<String> = store.iter().filter_map(assembly_recipe).filter(|a| a.mark.starts_with(letter)).map(|a| a.source).collect();
        let n = sources.len() + 1;
        let rows = zs.len().max(1);
        for ((i, row), parts) in placed {
            let mark = if rows > 1 { format!("{letter}{n}-{}{}", i + 1, (b'A' + row as u8) as char) } else { format!("{letter}{n}-{}", i + 1) };
            let rec = AssemblyRecipe {
                kind: ASSEMBLY_RECIPE.into(),
                source: guid.clone(),
                piece: plan.piece,
                parts: parts.clone(),
                mark: mark.clone(),
                index: i,
                row,
                version: c.version,
                rules: plan.rules.clone(),
                divisions: divisions.clone(),
                created_parts: !had,
            };
            let label = |v: &str| json!({ "type": "IfcLabel", "value": v });
            let a = txn.create(
                NewElement::new("IfcElementAssembly")
                    .name(&mark)
                    .param("@PredefinedType", "USERDEFINED")
                    .param("@ObjectType", "PRECAST_ASSEMBLY")
                    .param("Pset_PrecastConcreteElementFabrication.PieceMark", label(&mark))
                    .param("Pset_PrecastConcreteElementFabrication.TypeDesignator", label(plan.piece.name()))
                    .param(crate::walls::RECIPE_KEY, serde_json::to_value(&rec).expect("serialises")),
            )?;
            for pg in &parts {
                if let Some(pid) = GlobalId::parse(pg).and_then(|g| store.find_by_guid(&g)) {
                    txn.set_param(pid, "Pset_PrecastConcreteElementFabrication.PieceMark", label(&mark))?;
                    txn.set_param(pid, "Pset_PrecastConcreteElementFabrication.TypeDesignator", label(plan.piece.name()))?;
                }
            }
            out.push(a);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ResplitParams {
    pub assembly_ids: Vec<ElementRef>,
}

/// Splits the sources of assemblies again with the current rules (`precast.resplit`).
pub(crate) fn resplit(txn: &mut Transaction<'_>, p: ResplitParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let mut sources: Vec<(String, PieceKind)> = Vec::new();
    for r in &p.assembly_ids {
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let a = store.get(id).and_then(assembly_recipe).ok_or_else(|| Error::new("precast.not_assembly", format!("element {id} is not a precast assembly")))?;
        if !sources.iter().any(|(s, _)| *s == a.source) {
            sources.push((a.source, a.piece));
        }
    }
    let mut out = Vec::new();
    for (source, piece) in sources {
        let store = txn.store().clone();
        let assemblies: Vec<(ElementId, AssemblyRecipe)> = store.iter().filter_map(|e| assembly_recipe(e).filter(|r| r.source == source).map(|r| (e.id(), r))).collect();
        let created = assemblies.iter().any(|(_, r)| r.created_parts);
        let divisions: Vec<String> = assemblies.iter().flat_map(|(_, r)| r.divisions.clone()).collect();
        // The split's pieces, and its first parts if it made them; then its assemblies.
        let doomed: Vec<ElementId> = store
            .iter()
            .filter(|e| {
                crate::parts::recipe(e).is_some_and(|r| {
                    r.source == source && (r.division.as_ref().is_some_and(|d| divisions.contains(&d.id)) || (created && r.division.is_none() && r.merged.is_empty()))
                })
            })
            .map(|e| e.id())
            .chain(assemblies.iter().map(|(i, _)| *i))
            .collect();
        for id in doomed {
            if txn.store().get(id).is_some() {
                txn.delete(id)?;
            }
        }
        let src = by_guid(txn.store(), &source).map(|e| e.id()).ok_or_else(|| Error::new("precast.source", "the split element is gone"))?;
        let kind = match piece {
            PieceKind::SolidWall | PieceKind::SolidSlab => "solid",
            PieceKind::DoubleWall => "double",
            PieceKind::HollowCore => "hollow_core",
            PieceKind::LatticeGirder => "lattice_girder",
        };
        out.extend(split(txn, SplitParams { element_ids: vec![ElementRef::Id(src)], slab_kind: Some(kind.into()) })?);
    }
    Ok(out)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AutoUpdateParams {
    pub on: bool,
}

/// Turns automatic updates of pieces' reinforcement and mounting parts on or off
/// (`precast.set_auto_update`).
pub(crate) fn set_auto_update(txn: &mut Transaction<'_>, p: AutoUpdateParams) -> Result<u32> {
    set(txn, SetParams { path: "auto_update".into(), value: json!(p.on) })
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AssemblyInfo {
    pub id: ElementId,
    pub mark: String,
    pub piece: PieceKind,
    pub source: Option<ElementId>,
    pub parts: Vec<ElementId>,
    /// Volume (m³) and weight (t).
    pub volume: f64,
    pub weight: f64,
    pub version: u32,
    pub index: usize,
    pub row: usize,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct InfoParams {
    /// The assemblies; every precast assembly when empty.
    #[serde(default)]
    pub assembly_ids: Vec<ElementRef>,
}

/// Precast assemblies with their pieces' volume and weight (`precast.info`).
pub fn info(store: &Store, p: InfoParams) -> Result<Vec<AssemblyInfo>> {
    let ids: Vec<ElementId> = if p.assembly_ids.is_empty() {
        store.iter().filter(|e| assembly_recipe(e).is_some()).map(|e| e.id()).collect()
    } else {
        p.assembly_ids.iter().map(|r| store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))).collect::<Result<_>>()?
    };
    let bodies = crate::bodies::all_but_reinforcement(store);
    let density = config(store).density;
    ids.into_iter()
        .map(|id| {
            let r = store.get(id).and_then(assembly_recipe).ok_or_else(|| Error::new("precast.not_assembly", format!("element {id} is not a precast assembly")))?;
            let parts: Vec<ElementId> = r.parts.iter().filter_map(|g| by_guid(store, g).map(|e| e.id())).collect();
            let volume: f64 = parts.iter().filter_map(|pid| store.get(*pid)).map(|pe| crate::parts::volume(&crate::parts::geometry(store, pe, &bodies))).sum();
            Ok(AssemblyInfo {
                id,
                mark: r.mark.clone(),
                piece: r.piece,
                source: by_guid(store, &r.source).map(|e| e.id()),
                parts,
                volume,
                weight: volume * density,
                version: r.version,
                index: r.index,
                row: r.row,
            })
        })
        .collect()
}
