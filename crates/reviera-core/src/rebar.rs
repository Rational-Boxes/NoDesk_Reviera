// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Reinforcement (STR-RBR-001, 002, 003, 004, 008, 009).
//!
//! **Hosts** are concrete structural columns and beams, and structural floors and walls with a
//! concrete layer. A bar set whose host stops qualifying is kept and reported **orphaned**; it
//! goes when its host is deleted.
//!
//! **Settings** (`rxd:rebar` on the project): rounding (on or off; nearest, up or down; bar and
//! segment length increments), whether hooks are part of the shape, named **cover types** with
//! their offsets, and the project's own bar and hook types. Each host face (top, bottom,
//! exterior, interior, other) uses a cover type (`rxd:rebar_cover` on the host), defaulting by
//! the host's kind.
//!
//! **Bar types** come from the region's catalogue (ASTM A615 #3 … #18; EN 10080 6 … 50 mm) with
//! standard, hook and stirrup bend diameters (ACI 318 Tables 25.3.1/25.3.2; EN 1992-1-1 Table
//! 8.1N), or are made by the project. **Shapes** come from a shape-code catalogue (BS 8666
//! 00 straight, 11 L, 21 U, 51 closed link with 135° hooks).
//!
//! **Placement** (Expand to Host): a closed link in the member's section at each layout position
//! along it (`plane: section`); straight, L or U bars along a framing member at one of its faces
//! (`plane: face`), their legs turned into the section; straight bars in a floor's top or bottom
//! layer or a wall's face, in a direction, each clipped to the outline less the edge cover. Bars
//! keep inside the host's cover box; a shape that cannot fit is refused, naming the dimension.
//!
//! **Layouts** spread the bars of a set along its array: single, fixed number, maximum spacing,
//! number with spacing, minimum clear spacing, or a custom formula (`3x100; ?x200; 5x100` or
//! `1400@100; 900@300`, at most one `?`).
//!
//! Area, path and fabric reinforcement are in `rebar_systems`, numbering in `rebar_numbers`,
//! and visibility and presentation per view in `rebar_views`.
//!
//! Not yet: placement by two points and sketched bars, varying sets, cranks, spirals, shape
//! matching of sketched bars, the Pset_ReinforcingBarBendings property sets, swept-disk IFC
//! bodies.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const REBAR_RECIPE: &str = "recipe.rebar";
pub const SETTINGS_KEY: &str = "rxd:rebar";
pub const COVER_KEY: &str = "rxd:rebar_cover";

const IN: f64 = 0.0254;

pub(crate) type V3 = [f64; 3];

pub(crate) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub(crate) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub(crate) fn unit(a: V3) -> V3 {
    let l = len(a);
    if l < 1e-12 { a } else { mul(a, 1.0 / l) }
}

pub(crate) fn us(store: &Store) -> bool {
    crate::steel_fab::us_region(store)
}

// ---- Bar types and hooks ----

/// A bar type: its size and bends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BarType {
    pub id: String,
    pub name: String,
    pub diameter: f64,
    /// The diameter drawn (the deformed bar's outer size).
    pub model_diameter: f64,
    pub material: String,
    #[serde(default = "yes")]
    pub deformed: bool,
    /// Inside bend diameters: standard bends, hooks, and stirrups and ties.
    pub standard_bend: f64,
    pub hook_bend: f64,
    pub stirrup_bend: f64,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct SizeData {
    us: UsSizes,
    metric: MetricSizes,
}
#[derive(Deserialize)]
struct UsSizes {
    material: String,
    sizes: Vec<UsSize>,
}
#[derive(Deserialize)]
struct UsSize {
    id: String,
    diameter_in: f64,
}
#[derive(Deserialize)]
struct MetricSizes {
    material: String,
    sizes: Vec<u32>,
}

fn size_data() -> SizeData {
    serde_json::from_str(include_str!("../data/rebar/bar_sizes.json")).expect("the bar size table parses")
}

/// The region's catalogue of bar types.
pub fn catalogue(us_region: bool) -> Vec<BarType> {
    let d = size_data();
    if us_region {
        d.us.sizes
            .iter()
            .map(|s| {
                let db = s.diameter_in * IN;
                let n: u32 = s.id.trim_start_matches('#').parse().unwrap_or(0);
                // ACI 318-19 Table 25.3.1 (hooks) and 25.3.2 (stirrups and ties).
                let hook = if n <= 8 { 6.0 } else if n <= 11 { 8.0 } else { 10.0 } * db;
                let stirrup = if n <= 5 { 4.0 * db } else { 6.0 * db };
                BarType {
                    id: s.id.clone(),
                    name: s.id.clone(),
                    diameter: db,
                    model_diameter: db,
                    material: d.us.material.clone(),
                    deformed: true,
                    standard_bend: hook,
                    hook_bend: hook,
                    stirrup_bend: stirrup,
                }
            })
            .collect()
    } else {
        d.metric
            .sizes
            .iter()
            .map(|mm| {
                let db = f64::from(*mm) / 1000.0;
                // EN 1992-1-1 Table 8.1N: mandrel 4φ up to 16 mm, 7φ above.
                let bend = if *mm <= 16 { 4.0 } else { 7.0 } * db;
                BarType {
                    id: format!("d{mm}"),
                    name: format!("Ø{mm}"),
                    diameter: db,
                    model_diameter: db,
                    material: d.metric.material.clone(),
                    deformed: true,
                    standard_bend: bend,
                    hook_bend: bend,
                    stirrup_bend: bend,
                }
            })
            .collect()
    }
}

/// A hook: its angle and extension past the bend (a multiple of the diameter, at least a length).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HookType {
    pub id: String,
    pub name: String,
    /// Degrees.
    pub angle: f64,
    pub multiplier: f64,
    #[serde(default)]
    pub minimum: f64,
}

/// The region's hook types (ACI 318-19 25.3; EN 1992-1-1 8.5).
pub fn hook_types(us_region: bool) -> Vec<HookType> {
    let h = |id: &str, name: &str, angle: f64, multiplier: f64, minimum: f64| HookType {
        id: id.into(),
        name: name.into(),
        angle,
        multiplier,
        minimum,
    };
    if us_region {
        vec![
            h("standard_90", "Standard 90°", 90.0, 12.0, 0.0),
            h("standard_180", "Standard 180°", 180.0, 4.0, 2.5 * IN),
            h("stirrup_135", "Stirrup/tie 135°", 135.0, 6.0, 3.0 * IN),
        ]
    } else {
        vec![
            h("standard_90", "Standard 90°", 90.0, 10.0, 0.0),
            h("standard_180", "Standard 180°", 180.0, 5.0, 0.05),
            h("stirrup_135", "Link 135°", 135.0, 10.0, 0.07),
        ]
    }
}

pub fn extension(h: &HookType, d: f64) -> f64 {
    (h.multiplier * d).max(h.minimum)
}

// ---- Shapes ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Shape {
    pub code: String,
    pub name: String,
    pub dimensions: Vec<String>,
    /// `standard` or `stirrup_tie`.
    pub style: String,
    pub bends: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShapeCatalogue {
    pub catalogue: String,
    pub title: String,
    pub note: String,
    pub shapes: Vec<Shape>,
}

pub fn shapes() -> ShapeCatalogue {
    serde_json::from_str(include_str!("../data/rebar/shapes_bs8666.json")).expect("the shape catalogue parses")
}

fn shape(code: &str) -> Result<Shape> {
    shapes()
        .shapes
        .into_iter()
        .find(|s| s.code == code)
        .ok_or_else(|| Error::new("rebar.shape", format!("no shape {code} (00, 11, 21, 51)")))
}

// ---- Settings ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoundMethod {
    #[default]
    Nearest,
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rounding {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub method: RoundMethod,
    /// Metres.
    pub bar_length: f64,
    pub segment_length: f64,
}

impl Default for Rounding {
    fn default() -> Self {
        Self { on: false, method: RoundMethod::Nearest, bar_length: 0.001, segment_length: 0.005 }
    }
}

impl Rounding {
    pub fn round(&self, v: f64, step: f64) -> f64 {
        if !self.on || step <= 0.0 {
            return v;
        }
        let q = v / step;
        // To the micrometre first, so 2.437 / 0.010 does not round up from 243.70000001.
        let q = (q * 1e6).round() / 1e6;
        step * match self.method {
            RoundMethod::Nearest => q.round(),
            RoundMethod::Up => q.ceil(),
            RoundMethod::Down => q.floor(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CoverType {
    pub id: String,
    pub name: String,
    pub offset: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Settings {
    #[serde(default)]
    pub rounding: Rounding,
    #[serde(default = "yes")]
    pub hooks_in_shape: bool,
    #[serde(default)]
    pub area_path_rebar: bool,
    pub cover_types: Vec<CoverType>,
    #[serde(default)]
    pub bar_types: Vec<BarType>,
    #[serde(default)]
    pub hook_types: Vec<HookType>,
    /// The project's own fabric wire and sheet types (STR-RBR-012).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fabric_wires: Vec<crate::rebar_systems::WireType>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fabric_sheets: Vec<crate::rebar_systems::SheetType>,
    /// How sets show in views that give them no presentation of their own (STR-RBR-017).
    #[serde(default)]
    pub presentation: crate::rebar_views::Mode,
}

fn default_covers(us_region: bool) -> Vec<CoverType> {
    let c = |id: &str, name: &str, offset: f64| CoverType { id: id.into(), name: name.into(), offset };
    if us_region {
        vec![
            c("interior", "Interior 3/4\"", 0.75 * IN),
            c("beams", "Beams and columns 1 1/2\"", 1.5 * IN),
            c("exposed", "Exposed 2\"", 2.0 * IN),
            c("earth", "Cast against earth 3\"", 3.0 * IN),
        ]
    } else {
        vec![
            c("interior", "Interior 25", 0.025),
            c("beams", "Beams and columns 35", 0.035),
            c("exposed", "Exterior 40", 0.040),
            c("earth", "Against ground 75", 0.075),
        ]
    }
}

pub(crate) fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub fn settings(store: &Store) -> Settings {
    project(store)
        .and_then(|p| p.param(SETTINGS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| Settings {
            rounding: Rounding::default(),
            hooks_in_shape: true,
            area_path_rebar: false,
            cover_types: default_covers(us(store)),
            bar_types: Vec::new(),
            hook_types: Vec::new(),
            fabric_wires: Vec::new(),
            fabric_sheets: Vec::new(),
            presentation: crate::rebar_views::Mode::All,
        })
}

pub(crate) fn save(txn: &mut crate::txn::Transaction<'_>, s: &Settings) -> Result<()> {
    let store = txn.store().clone();
    let pid = project(&store).map(|e| e.id()).ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    txn.set_param(pid, SETTINGS_KEY, serde_json::to_value(s).expect("serialises"))
}

/// Every bar type: the region's catalogue and the project's own.
pub fn bar_types(store: &Store) -> Vec<BarType> {
    let mut v = catalogue(us(store));
    v.extend(settings(store).bar_types);
    v
}

pub fn all_hook_types(store: &Store) -> Vec<HookType> {
    let mut v = hook_types(us(store));
    v.extend(settings(store).hook_types);
    v
}

pub(crate) fn bar_type(store: &Store, id: &str) -> Result<BarType> {
    bar_types(store)
        .into_iter()
        .find(|b| b.id == id || b.name == id)
        .ok_or_else(|| Error::new("rebar.bar_type", format!("no bar type {id}")))
}

pub(crate) fn hook_type(store: &Store, id: &str) -> Result<HookType> {
    all_hook_types(store)
        .into_iter()
        .find(|h| h.id == id || h.name == id)
        .ok_or_else(|| Error::new("rebar.hook_type", format!("no hook type {id}")))
}

fn hook(store: &Store, id: &str) -> HookType {
    all_hook_types(store).into_iter().find(|h| h.id == id).expect("a standard hook")
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetSettingParams {
    /// `rounding.on`, `rounding.method`, `rounding.bar_length`, `rounding.segment_length`,
    /// `hooks_in_shape`, `area_path_rebar` or `presentation` (all, first_last, middle).
    pub key: String,
    pub value: Value,
}

/// Changes a reinforcement setting (`reinforcement_settings.set`).
pub(crate) fn set_setting(txn: &mut crate::txn::Transaction<'_>, p: SetSettingParams) -> Result<()> {
    let store = txn.store().clone();
    let mut s = settings(&store);
    let any_rebar = store.iter().any(|e| rebar_recipe(e).is_some() || crate::rebar_systems::system_kind(e).is_some());
    let bad = |m: &str| Error::new("reinforcement_settings.value", m.to_owned());
    let num = |v: &Value| v.as_f64().ok_or_else(|| bad("a number of metres"));
    match p.key.as_str() {
        "rounding.on" => s.rounding.on = p.value.as_bool().ok_or_else(|| bad("true or false"))?,
        "rounding.method" => {
            s.rounding.method = serde_json::from_value(p.value.clone()).map_err(|_| bad("nearest, up or down"))?
        }
        "rounding.bar_length" | "rounding.segment_length" => {
            let v = num(&p.value)?;
            if !(v > 0.0 && v <= 0.1 + 1e-12) {
                return Err(bad("a rounding increment is more than 0 and at most 100 mm"));
            }
            if p.key.ends_with("bar_length") {
                s.rounding.bar_length = v;
            } else {
                s.rounding.segment_length = v;
            }
        }
        "hooks_in_shape" => {
            if any_rebar {
                return Err(Error::new(
                    "reinforcement_settings.locked",
                    "hooks in shape definitions is decided before the first rebar is placed",
                ));
            }
            s.hooks_in_shape = p.value.as_bool().ok_or_else(|| bad("true or false"))?;
        }
        "area_path_rebar" => {
            if crate::rebar_systems::any_area_or_path(&store) {
                return Err(Error::new(
                    "reinforcement_settings.locked",
                    "area and path rebar as real rebar is decided before the first area or path system",
                ));
            }
            s.area_path_rebar = p.value.as_bool().ok_or_else(|| bad("true or false"))?;
        }
        "presentation" => {
            s.presentation = serde_json::from_value(p.value.clone()).map_err(|_| bad("all, first_last, middle or select"))?;
        }
        k => return Err(Error::new("reinforcement_settings.key", format!("no reinforcement setting {k}"))),
    }
    save(txn, &s)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CoverTypeParams {
    pub name: String,
    pub offset: f64,
}

fn check_offset(v: f64) -> Result<()> {
    if !(0.0..=0.5 + 1e-12).contains(&v) {
        return Err(Error::new("rebar_cover_type.offset", "a cover offset is 0 to 500 mm"));
    }
    Ok(())
}

/// Makes a cover type (`rebar_cover_type.create`).
pub(crate) fn cover_type_create(txn: &mut crate::txn::Transaction<'_>, p: CoverTypeParams) -> Result<String> {
    let store = txn.store().clone();
    let mut s = settings(&store);
    let name = p.name.trim();
    if name.is_empty() || s.cover_types.iter().any(|c| c.name == name) {
        return Err(Error::new("rebar_cover_type.name", "a cover type's name is unique and not empty"));
    }
    check_offset(p.offset)?;
    let mut n = s.cover_types.len() + 1;
    while s.cover_types.iter().any(|c| c.id == format!("cover{n}")) {
        n += 1;
    }
    let id = format!("cover{n}");
    s.cover_types.push(CoverType { id: id.clone(), name: name.into(), offset: p.offset });
    save(txn, &s)?;
    Ok(id)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CoverTypeSetParams {
    pub cover_type_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub offset: Option<f64>,
}

/// Renames a cover type or changes its offset (`rebar_cover_type.set`); bars on faces of the type
/// follow.
pub(crate) fn cover_type_set(txn: &mut crate::txn::Transaction<'_>, p: CoverTypeSetParams) -> Result<()> {
    let store = txn.store().clone();
    let mut s = settings(&store);
    let k = s
        .cover_types
        .iter()
        .position(|c| c.id == p.cover_type_id)
        .ok_or_else(|| Error::new("rebar_cover_type.unknown", format!("no cover type {}", p.cover_type_id)))?;
    if let Some(o) = p.offset {
        check_offset(o)?;
        s.cover_types[k].offset = o;
    }
    if let Some(n) = p.name {
        if n.trim().is_empty() || s.cover_types.iter().enumerate().any(|(i, c)| i != k && c.name == n) {
            return Err(Error::new("rebar_cover_type.name", "a cover type's name is unique and not empty"));
        }
        s.cover_types[k].name = n;
    }
    save(txn, &s)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CoverTypeDeleteParams {
    pub cover_type_id: String,
    /// Where the faces using it go; required when any do.
    #[serde(default)]
    pub replacement: Option<String>,
}

/// Deletes a cover type, moving its faces to a replacement (`rebar_cover_type.delete`). Returns
/// the faces moved.
pub(crate) fn cover_type_delete(txn: &mut crate::txn::Transaction<'_>, p: CoverTypeDeleteParams) -> Result<usize> {
    let store = txn.store().clone();
    let mut s = settings(&store);
    if !s.cover_types.iter().any(|c| c.id == p.cover_type_id) {
        return Err(Error::new("rebar_cover_type.unknown", format!("no cover type {}", p.cover_type_id)));
    }
    if s.cover_types.len() == 1 {
        return Err(Error::new("rebar_cover_type.last", "a project keeps at least one cover type"));
    }
    // Faces using it: assigned, or by default.
    let mut moved = 0;
    let mut changes: Vec<(ElementId, Value)> = Vec::new();
    for e in store.iter() {
        let Some(kind) = host_kind(&store, e) else { continue };
        let mut faces = faces_of(&store, e, kind);
        let n = faces.values_mut().filter(|v| **v == p.cover_type_id).count();
        if n == 0 {
            continue;
        }
        let Some(rep) = &p.replacement else {
            return Err(Error::new(
                "rebar_cover_type.used",
                "the cover type is used by host faces: give a replacement",
            )
            .with_details(json!({ "used": usage(&store, &p.cover_type_id) })));
        };
        for v in faces.values_mut() {
            if *v == p.cover_type_id {
                *v = rep.clone();
            }
        }
        moved += n;
        changes.push((e.id(), serde_json::to_value(&faces).expect("serialises")));
    }
    if let Some(r) = &p.replacement
        && (!s.cover_types.iter().any(|c| &c.id == r) || r == &p.cover_type_id)
    {
        return Err(Error::new("rebar_cover_type.unknown", format!("no other cover type {r}")));
    }
    for (id, v) in changes {
        txn.set_param(id, COVER_KEY, v)?;
    }
    s.cover_types.retain(|c| c.id != p.cover_type_id);
    save(txn, &s)?;
    Ok(moved)
}

/// How many host faces use a cover type.
pub fn usage(store: &Store, id: &str) -> usize {
    store
        .iter()
        .filter_map(|e| host_kind(store, e).map(|k| faces_of(store, e, k)))
        .map(|f| f.values().filter(|v| v.as_str() == id).count())
        .sum()
}

// ---- Hosts ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    Column,
    Beam,
    Floor,
    Wall,
}

/// What kind of host an element would be (whatever its material).
pub(crate) fn host_kind(store: &Store, e: &Element) -> Option<HostKind> {
    let _ = store;
    if crate::columns::column_recipe(e).is_some() {
        Some(HostKind::Column)
    } else if crate::beams::beam_recipe(e).is_some_and(|r| !r.brace) {
        Some(HostKind::Beam)
    } else if crate::floors::slab_recipe(e).is_some_and(|(k, _)| k == crate::floors::SlabKind::Floor) {
        Some(HostKind::Floor)
    } else if crate::walls::wall_recipe(e).is_some() {
        Some(HostKind::Wall)
    } else {
        None
    }
}

fn concrete_name(n: &str) -> bool {
    crate::materials_library::find(n).map_or_else(
        || n.to_ascii_lowercase().contains("concrete"),
        |m| matches!(m.kind, crate::columns::MaterialKind::Concrete),
    )
}

/// Whether an element can host rebar, and why not.
pub fn host_check(store: &Store, e: &Element) -> std::result::Result<HostKind, String> {
    use crate::columns::MaterialKind;
    let kind = host_kind(store, e).ok_or_else(|| "not a column, beam, floor or wall".to_owned())?;
    let layers_concrete = |t: Option<&Element>| -> bool {
        t.and_then(crate::compound::of_type)
            .is_some_and(|c| {
                c.layers.iter().any(|l| {
                    // A generic type's structure layer, with no material named, counts as concrete.
                    l.thickness > 0.0
                        && (concrete_name(&l.material)
                            || (l.material.trim().is_empty() && l.function == crate::compound::Function::Structure))
                })
            })
    };
    let type_of = |e: &Element| {
        e.param("@type")
            .and_then(Value::as_str)
            .and_then(GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|i| store.get(i))
    };
    match kind {
        HostKind::Column | HostKind::Beam => {
            let f = crate::steel_detail::member_frame(store, e).ok_or_else(|| "it does not regenerate".to_owned())?;
            if kind == HostKind::Column && !f.ty.structural {
                return Err("an architectural column".into());
            }
            match f.ty.material_kind {
                Some(MaterialKind::Concrete | MaterialKind::PrecastConcrete) => Ok(kind),
                _ => Err("its material is not concrete".into()),
            }
        }
        HostKind::Floor => {
            if !crate::structural::is_structural_floor(e) {
                return Err("not a structural floor".into());
            }
            if layers_concrete(type_of(e)) { Ok(kind) } else { Err("its type has no concrete layer".into()) }
        }
        HostKind::Wall => {
            let structural = crate::structural::record(e).is_some()
                && crate::structural::wall_usage(e) != crate::structural::WallUsage::NonBearing;
            if !structural {
                return Err("not a structural wall".into());
            }
            if layers_concrete(type_of(e)) { Ok(kind) } else { Err("its type has no concrete layer".into()) }
        }
    }
}

/// The faces of a host and their cover types: assigned, else by the host's kind.
pub(crate) fn faces_of(store: &Store, e: &Element, kind: HostKind) -> std::collections::BTreeMap<String, String> {
    let _ = store;
    let default = match kind {
        HostKind::Column | HostKind::Beam => [("top", "beams"), ("bottom", "beams"), ("other", "beams")].as_slice(),
        HostKind::Floor => &[("top", "interior"), ("bottom", "interior"), ("other", "interior")],
        HostKind::Wall => &[("exterior", "exposed"), ("interior", "interior"), ("top", "interior"), ("bottom", "interior"), ("other", "interior")],
    };
    let mut out: std::collections::BTreeMap<String, String> =
        default.iter().map(|(f, c)| ((*f).to_owned(), (*c).to_owned())).collect();
    if let Some(m) = e.param(COVER_KEY).and_then(Value::as_object) {
        for (k, v) in m {
            if let Some(v) = v.as_str() {
                out.insert(k.clone(), v.to_owned());
            }
        }
    }
    out
}

/// A host face's cover (metres).
pub(crate) fn cover(store: &Store, e: &Element, kind: HostKind, face: &str) -> f64 {
    let faces = faces_of(store, e, kind);
    let id = faces.get(face).or_else(|| faces.get("other")).cloned().unwrap_or_default();
    let s = settings(store);
    s.cover_types
        .iter()
        .find(|c| c.id == id)
        .or_else(|| s.cover_types.first())
        .map_or(0.025, |c| c.offset)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AssignParams {
    pub host_ids: Vec<ElementRef>,
    /// `top`, `bottom`, `exterior`, `interior` or `other`; every face when omitted.
    #[serde(default)]
    pub face: Option<String>,
    pub cover_type_id: String,
}

/// Gives host faces a cover type (`rebar_cover.assign`); their bars follow.
pub(crate) fn assign(txn: &mut crate::txn::Transaction<'_>, p: AssignParams) -> Result<()> {
    let store = txn.store().clone();
    if !settings(&store).cover_types.iter().any(|c| c.id == p.cover_type_id) {
        return Err(Error::new("rebar_cover_type.unknown", format!("no cover type {}", p.cover_type_id)));
    }
    for r in &p.host_ids {
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        let kind = host_kind(&store, e).ok_or_else(|| Error::new("rebar.not_a_host", format!("element {id} cannot host rebar")))?;
        let mut faces = faces_of(&store, e, kind);
        match &p.face {
            Some(f) if faces.contains_key(f) => {
                faces.insert(f.clone(), p.cover_type_id.clone());
            }
            Some(f) => return Err(Error::new("rebar_cover.face", format!("element {id} has no {f} face ({})", faces.keys().cloned().collect::<Vec<_>>().join(", ")))),
            None => {
                for v in faces.values_mut() {
                    *v = p.cover_type_id.clone();
                }
            }
        }
        txn.set_param(id, COVER_KEY, serde_json::to_value(&faces).expect("serialises"))?;
    }
    Ok(())
}

// ---- Layouts ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    Single,
    FixedNumber,
    #[default]
    MaximumSpacing,
    NumberWithSpacing,
    MinimumClearSpacing,
    Custom,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Layout {
    #[serde(default)]
    pub rule: Rule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
}

/// Parses a custom spacing formula into its spaces, for an array `length` long.
pub fn formula_spaces(formula: &str, length: f64) -> Result<Vec<f64>> {
    let bad = |m: String| Error::new("rebar.formula", m);
    // Each part: `count x spacing` or `distance @ spacing`, `?` for the one count that absorbs the
    // rest (millimetres).
    enum Part {
        Fixed(usize, f64),
        Rest(f64),
    }
    let mut parts = Vec::new();
    let mut rests = 0;
    for raw in formula.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (a, s, at) = if let Some((a, s)) = raw.split_once(['x', 'X', '×']) {
            (a.trim(), s.trim(), false)
        } else if let Some((a, s)) = raw.split_once('@') {
            (a.trim(), s.trim(), true)
        } else {
            return Err(bad(format!("`{raw}` is neither `count x spacing` nor `distance @ spacing`")));
        };
        let sp = s.parse::<f64>().map_err(|_| bad(format!("`{s}` is not a spacing in mm")))? / 1000.0;
        if sp <= 0.0 {
            return Err(bad(format!("the spacing in `{raw}` is not more than 0")));
        }
        if a == "?" {
            rests += 1;
            parts.push(Part::Rest(sp));
            continue;
        }
        let v = a.parse::<f64>().map_err(|_| bad(format!("`{a}` is not a number")))?;
        let n = if at { (v / 1000.0 / sp).round() as usize } else { v.round() as usize };
        parts.push(Part::Fixed(n, sp));
    }
    if rests > 1 {
        return Err(bad("only one `?` may absorb the rest".into()));
    }
    let fixed: f64 = parts.iter().map(|p| if let Part::Fixed(n, s) = p { *n as f64 * s } else { 0.0 }).sum();
    if fixed > length + 1e-6 {
        return Err(bad(format!(
            "the spaces add up to {:.0} mm, more than the {:.0} mm the set spans",
            fixed * 1000.0,
            length * 1000.0
        )));
    }
    let mut out = Vec::new();
    for p in parts {
        match p {
            Part::Fixed(n, s) => out.extend(std::iter::repeat_n(s, n)),
            Part::Rest(s) => {
                let n = ((length - fixed) / s + 1e-6).floor().max(0.0) as usize;
                out.extend(std::iter::repeat_n(s, n));
            }
        }
    }
    Ok(out)
}

/// The bars' positions along an array `length` long, for bars of diameter `d`.
pub fn positions(l: &Layout, length: f64, d: f64) -> Result<Vec<f64>> {
    let even = |n: usize| -> Vec<f64> {
        match n {
            0 => Vec::new(),
            1 => vec![0.0],
            _ => (0..n).map(|i| length * i as f64 / (n - 1) as f64).collect(),
        }
    };
    let need = |v: Option<f64>, what: &str| v.filter(|x| *x > 0.0).ok_or_else(|| Error::new("rebar.layout", format!("the layout needs {what} (more than 0)")));
    Ok(match l.rule {
        Rule::Single => vec![0.0],
        Rule::FixedNumber => {
            let n = l.n.filter(|n| *n > 0).ok_or_else(|| Error::new("rebar.layout", "the layout needs n (at least 1)"))?;
            even(n)
        }
        Rule::MaximumSpacing => {
            let s = need(l.spacing, "a spacing")?;
            even((length / s - 1e-9).ceil().max(0.0) as usize + 1)
        }
        Rule::NumberWithSpacing => {
            let n = l.n.filter(|n| *n > 0).ok_or_else(|| Error::new("rebar.layout", "the layout needs n (at least 1)"))?;
            let s = need(l.spacing, "a spacing")?;
            if (n - 1) as f64 * s > length + 1e-6 {
                return Err(Error::new(
                    "rebar.layout",
                    format!("{n} bars at {:.0} mm span {:.0} mm, more than the {:.0} mm there is", s * 1000.0, (n - 1) as f64 * s * 1000.0, length * 1000.0),
                ));
            }
            (0..n).map(|i| i as f64 * s).collect()
        }
        Rule::MinimumClearSpacing => {
            let c = need(l.clear, "a clear spacing")?;
            even((length / (c + d) + 1e-9).floor() as usize + 1)
        }
        Rule::Custom => {
            let f = l.formula.as_deref().ok_or_else(|| Error::new("rebar.layout", "the layout needs a formula"))?;
            let mut out = vec![0.0];
            for s in formula_spaces(f, length)? {
                out.push(out.last().copied().unwrap_or(0.0) + s);
            }
            out
        }
    })
}

// ---- Placement ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Placement {
    /// In a framing member's section, spread along it (links).
    Section,
    /// Along a framing member at a face (`top`, `bottom`, `left`, `right`), spread across it.
    Face { face: String },
    /// A floor's top or bottom layer, or a wall's exterior or interior face, the bars in a
    /// direction: an angle in plan (degrees) for floors; `vertical` or `horizontal` for walls.
    Layer {
        face: String,
        #[serde(default)]
        angle: f64,
        #[serde(default)]
        vertical: bool,
        /// Further in from the cover (metres), for a second layer.
        #[serde(default)]
        offset: f64,
        /// Only inside this region of the host's plane (plan x, y for floors; along the wall and
        /// up from its base for walls), as area reinforcement lays its layers (STR-RBR-010).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        boundary: Option<Vec<[f64; 2]>>,
        /// A hook type at both ends, turned into the host (or out of the face with `hook_flip`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hook: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        hook_flip: bool,
    },
    /// Straight bars through a wall's thickness, between its exterior and interior covers, at
    /// `along` metres from the wall's start, spread up the wall (STR-RBR-005).
    Through { along: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RebarRecipe {
    pub kind: String,
    pub host: String,
    pub shape: String,
    pub bar_type: String,
    pub placement: Placement,
    pub layout: Layout,
    /// Bars removed, moved, kept to a range or with segments set (STR-RBR-006).
    #[serde(default, skip_serializing_if = "BarEdits::is_empty")]
    pub edits: BarEdits,
    /// The set's part of a splice (STR-RBR-015).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub splice: Option<Splice>,
    /// Each bar fitted to the host at its own place (STR-RBR-005); a standard set's bars are
    /// all the first one's length.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub varying: bool,
}

/// Edits to a set's bars (STR-RBR-006), by layout position.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BarEdits {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<usize>,
    /// Moves along the array (metres), by bar.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub moved: std::collections::BTreeMap<usize, f64>,
    /// The layout positions this set keeps, `from..to` (a split set's part).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<(usize, usize)>,
    /// Segment values set by hand (outside dimensions, metres): A, B, ….
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub segments: std::collections::BTreeMap<String, f64>,
}

impl BarEdits {
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.moved.is_empty() && self.range.is_none() && self.segments.is_empty()
    }
}

/// Which part of a lap splice a set is: its bars end (`first`) or start (`second`) around the
/// splice point, the two overlapping by the lap; every second bar's splice point moves by the
/// stagger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Splice {
    pub first: bool,
    /// The splice point's distance from the bars' start (metres).
    pub at: f64,
    pub lap: f64,
    #[serde(default)]
    pub stagger: f64,
    /// The other part (GlobalId).
    pub partner: String,
}

pub fn rebar_recipe(e: &Element) -> Option<RebarRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == REBAR_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// One bar: its centreline.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Bar {
    pub points: Vec<V3>,
}

impl Bar {
    pub fn length(&self) -> f64 {
        self.points.windows(2).map(|w| len(sub(w[1], w[0]))).sum()
    }
}

/// A polyline with its corners rounded (radius `r` on the centreline).
pub(crate) fn rounded(points: &[V3], r: f64) -> Vec<V3> {
    if points.len() < 3 || r <= 0.0 {
        return points.to_vec();
    }
    let mut out = vec![points[0]];
    for i in 1..points.len() - 1 {
        let (p, a, b) = (points[i], unit(sub(points[i - 1], points[i])), unit(sub(points[i + 1], points[i])));
        let cos = dot(a, b).clamp(-1.0, 1.0);
        let theta = cos.acos();
        if theta < 1e-6 || (std::f64::consts::PI - theta) < 1e-6 {
            out.push(p);
            continue;
        }
        // Tangent points at r / tan(θ/2) from the corner.
        let t = r / (theta / 2.0).tan();
        let room = len(sub(points[i - 1], p)).min(len(sub(points[i + 1], p))) * 0.999;
        let t = t.min(room);
        let r = t * (theta / 2.0).tan();
        let (ta, tb) = (add(p, mul(a, t)), add(p, mul(b, t)));
        let bis = unit(add(a, b));
        let c = add(p, mul(bis, r / (theta / 2.0).sin()));
        let (u, v) = (unit(sub(ta, c)), unit(sub(tb, c)));
        let sweep = dot(u, v).clamp(-1.0, 1.0).acos();
        let w = unit(sub(v, mul(u, dot(u, v))));
        let n = ((sweep / (std::f64::consts::PI / 12.0)).ceil() as usize).max(2);
        for k in 0..=n {
            let a = sweep * k as f64 / n as f64;
            out.push(add(c, add(mul(u, r * a.cos()), mul(w, r * a.sin()))));
        }
    }
    out.push(*points.last().expect("points"));
    out
}

/// What a set's bars come to: their centrelines, the shape's dimensions and the set's role.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SetGeometry {
    pub bars: Vec<Bar>,
    /// The shape's dimensions A, B, C… (outside, metres).
    pub dimensions: Vec<(String, f64)>,
    /// `main` or `ligature`.
    pub role: String,
    pub array_length: f64,
    /// Each bar's index among the set's bars before its edits (what bar edits refer to).
    pub indices: Vec<usize>,
}

pub(crate) fn chord(polys: &[Vec<[f64; 2]>], o: [f64; 2], d: [f64; 2]) -> Vec<(f64, f64)> {
    // Where the line o + t d crosses the outlines, as inside intervals.
    let mut ts = Vec::new();
    for poly in polys {
        let n = poly.len();
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let e = [b[0] - a[0], b[1] - a[1]];
            let den = d[0] * e[1] - d[1] * e[0];
            if den.abs() < 1e-12 {
                continue;
            }
            let w = [a[0] - o[0], a[1] - o[1]];
            let t = (w[0] * e[1] - w[1] * e[0]) / den;
            let s = (w[0] * d[1] - w[1] * d[0]) / den;
            if (0.0..1.0).contains(&s) {
                ts.push(t);
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect()
}

/// A floor's or wall's plane at a layer of bars: its outline less its openings in the plane's
/// coordinates (plan x, y for floors; along the wall and up from its base for walls), where the
/// plane lies, and which way is into the host.
pub(crate) struct HostPlane {
    pub shapes: Vec<crate::polygons::Shape>,
    pub o: V3,
    pub ex: V3,
    pub ey: V3,
    /// From the layer towards the opposite face.
    pub inward: V3,
    /// From the layer's bar centres to the opposite face's cover, for hooks and legs.
    pub room: f64,
}

impl HostPlane {
    pub fn at(&self, q: [f64; 2]) -> V3 {
        add(self.o, add(mul(self.ex, q[0]), mul(self.ey, q[1])))
    }

    /// The outline's rings, cut to a boundary.
    pub fn clipped(&self, boundary: Option<&[[f64; 2]]>) -> Vec<Vec<[f64; 2]>> {
        let shapes = match boundary {
            Some(b) => crate::polygons::intersection(&self.shapes, &[crate::polygons::Shape { outer: b.to_vec(), holes: Vec::new() }]),
            None => self.shapes.clone(),
        };
        shapes.into_iter().flat_map(|s| std::iter::once(s.outer).chain(s.holes)).collect()
    }
}

/// The plane of a floor's `top` or `bottom` layer, or a wall's `exterior` or `interior` one, for
/// bars of diameter `d` an extra `offset` in from the cover.
pub(crate) fn host_plane(store: &Store, host: &Element, kind: HostKind, face: &str, d: f64, offset: f64) -> Result<HostPlane> {
    let cov = |f: &str| cover(store, host, kind, f);
    match kind {
        HostKind::Floor => {
            let g = crate::floors::regenerate(store, host)?;
            let z = match face {
                "top" => g.top - cov("top") - d / 2.0 - offset,
                "bottom" => g.top - g.thickness + cov("bottom") + d / 2.0 + offset,
                other => return Err(Error::new("rebar.face", format!("a floor's layers are top or bottom, not {other}"))),
            };
            if cov("top") + cov("bottom") + 2.0 * d > g.thickness {
                return Err(Error::new("rebar.no_fit", format!("the bars do not fit: the floor is {:.0} mm thick", g.thickness * 1000.0)));
            }
            let shapes = crate::slab_openings::cut_profiles(store, host, &g.profiles)
                .into_iter()
                .map(|p| crate::polygons::Shape { outer: p.outer, holes: p.holes })
                .collect();
            Ok(HostPlane {
                shapes,
                o: [0.0, 0.0, z],
                ex: [1.0, 0.0, 0.0],
                ey: [0.0, 1.0, 0.0],
                inward: [0.0, 0.0, if face == "top" { -1.0 } else { 1.0 }],
                room: g.thickness - cov("top") - cov("bottom") - d - offset,
            })
        }
        HostKind::Wall => {
            let g = crate::walls::regenerate(store, host)?;
            let (dir, nrm) = (g.direction(), g.normal());
            let side = match face {
                "exterior" => 1.0,
                "interior" => -1.0,
                other => return Err(Error::new("rebar.face", format!("a wall's layers are exterior or interior, not {other}"))),
            };
            let inset = g.width / 2.0 - cov(face) - d / 2.0 - offset;
            if inset < d / 2.0 {
                return Err(Error::new("rebar.no_fit", format!("the bars do not fit: the wall is {:.0} mm thick", g.width * 1000.0)));
            }
            let l = g.length();
            Ok(HostPlane {
                shapes: vec![crate::polygons::Shape { outer: vec![[0.0, 0.0], [l, 0.0], [l, g.height], [0.0, g.height]], holes: Vec::new() }],
                o: [g.start[0] + nrm[0] * side * inset, g.start[1] + nrm[1] * side * inset, g.base_z],
                ex: [dir[0], dir[1], 0.0],
                ey: [0.0, 0.0, 1.0],
                inward: [-nrm[0] * side, -nrm[1] * side, 0.0],
                room: g.width - cov("exterior") - cov("interior") - d - offset,
            })
        }
        _ => Err(Error::new("rebar.placement", "only floors and walls have layers")),
    }
}

/// A segment's value set by hand, refused past what the host leaves room for.
fn segment_override(r: &RebarRecipe, letter: &str, room: f64) -> Result<f64> {
    match r.edits.segments.get(letter) {
        Some(v) if *v > room + 1e-9 => Err(Error::new(
            "rebar.segment",
            format!("segment {letter} can be at most {:.0} mm here, inside the cover", room * 1000.0),
        )),
        Some(v) => Ok(*v),
        None => Ok(room),
    }
}

/// A hook at a bar's end: the points from the end (where the bar turns) along the tail. `out`
/// runs out of the bar there; the tail turns `inward`, kept within `room`.
pub(crate) fn hook_points(end: V3, out: V3, inward: V3, h: &HookType, bt: &BarType, room: f64) -> Vec<V3> {
    let d = bt.diameter;
    let r = bt.hook_bend / 2.0 + d / 2.0;
    let mut ext = extension(h, d);
    let th = h.angle.to_radians();
    if th < 1e-6 {
        return vec![end, add(end, mul(out, ext))];
    }
    if h.angle <= 90.0 + 1e-9 {
        let tail = unit(add(mul(out, th.cos()), mul(inward, th.sin())));
        let reach = (r + ext) * th.sin();
        if reach > room && room > 0.0 {
            ext = (room / th.sin() - r).max(d);
        }
        return vec![end, add(end, mul(tail, r + ext))];
    }
    // Past a right angle: down a bend's width, then the tail turned back.
    let w = 2.0 * r;
    let tail = unit(add(mul(out, th.cos()), mul(inward, th.sin())));
    let reach = w + ext * th.sin();
    if reach > room && room > w {
        ext = ((room - w) / th.sin().max(1e-6)).max(d);
    }
    let corner = add(end, mul(inward, w));
    vec![end, corner, add(corner, mul(tail, ext))]
}

/// Works out a set's bars from its host.
/// Works out a set's bars from its host, with its edits and splice applied.
pub fn geometry(store: &Store, r: &RebarRecipe) -> Result<SetGeometry> {
    let mut g = base_geometry(store, r)?;
    let n = g.bars.len();
    // The array direction, from the first bar's start to the last's.
    let along = if n > 1 { unit(sub(g.bars[n - 1].points[0], g.bars[0].points[0])) } else { [0.0, 0.0, 0.0] };
    let (from, to) = r.edits.range.unwrap_or((0, n));
    let mut bars = Vec::new();
    let mut indices = Vec::new();
    for (i, mut b) in g.bars.into_iter().enumerate() {
        if i < from || i >= to.min(n) || r.edits.removed.contains(&i) {
            continue;
        }
        indices.push(i);
        if let Some(dt) = r.edits.moved.get(&i) {
            b.points.iter_mut().for_each(|p| *p = add(*p, mul(along, *dt)));
        }
        if let Some(sp) = &r.splice {
            let shift = if i % 2 == 1 { sp.stagger } else { 0.0 };
            spliced(&mut b, sp.at + shift, sp.lap, sp.first);
        }
        bars.push(b);
    }
    if bars.is_empty() {
        return Err(Error::new("rebar.no_bars", "every bar of the set is removed"));
    }
    if let Some(sp) = &r.splice {
        let full = g.dimensions.iter().find(|(k, _)| k == "A").map_or(0.0, |x| x.1);
        let part = if sp.first { sp.at + sp.lap / 2.0 } else { full - sp.at + sp.lap / 2.0 };
        g.dimensions = vec![("A".into(), part.min(full))];
    }
    g.bars = bars;
    g.indices = indices;
    Ok(g)
}

/// Cuts a straight bar at a splice: the first part runs to `at` plus half the lap, the second
/// from `at` less half the lap.
fn spliced(b: &mut Bar, at: f64, lap: f64, first: bool) {
    let (a, e) = (b.points[0], b.points[b.points.len() - 1]);
    let l = len(sub(e, a));
    if l < 1e-9 {
        return;
    }
    let dir = unit(sub(e, a));
    let (s0, s1) = if first { (0.0, (at + lap / 2.0).min(l)) } else { ((at - lap / 2.0).max(0.0), l) };
    b.points = vec![add(a, mul(dir, s0)), add(a, mul(dir, s1))];
}

fn base_geometry(store: &Store, r: &RebarRecipe) -> Result<SetGeometry> {
    let host = GlobalId::parse(&r.host)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|i| store.get(i))
        .ok_or_else(|| Error::new("rebar.no_host", "the bar set's host is gone"))?;
    let kind = host_kind(store, host).ok_or_else(|| Error::new("rebar.no_host", "the host cannot hold rebar"))?;
    let bt = bar_type(store, &r.bar_type)?;
    let sh = shape(&r.shape)?;
    let d = bt.diameter;
    let cov = |f: &str| cover(store, host, kind, f);
    let fit = |what: &str, v: f64| -> Result<f64> {
        if v <= d {
            Err(Error::new("rebar.no_fit", format!("the bars do not fit: the {what} inside the cover is {:.0} mm", v.max(0.0) * 1000.0)))
        } else {
            Ok(v)
        }
    };
    match (&r.placement, kind) {
        (Placement::Section, HostKind::Column | HostKind::Beam) => {
            if sh.code != "51" {
                return Err(Error::new("rebar.shape", "links in a section take shape 51 (closed link)"));
            }
            let f = crate::steel_detail::member_frame(store, host).ok_or_else(|| Error::new("rebar.no_host", "the host does not regenerate"))?;
            if f.ty.section != crate::columns::Section::Rectangular {
                return Err(Error::new("rebar.shape", "links need a rectangular section (round columns take spirals, not yet)"));
            }
            let (top, bottom, side) = if kind == HostKind::Beam { (cov("top"), cov("bottom"), cov("other")) } else { (cov("other"), cov("other"), cov("other")) };
            let w = segment_override(r, "A", fit("width", f.ty.width - 2.0 * side)?)?;
            let h = segment_override(r, "B", fit("depth", f.ty.depth - top - bottom)?)?;
            let axis_len = len(sub(f.end, f.start));
            let (c0, c1) = if kind == HostKind::Column { (cov("bottom"), cov("top")) } else { (cov("other"), cov("other")) };
            let span = fit("length", axis_len - c0 - c1)? - d;
            let pos = positions(&r.layout, span, d)?;
            // The link's centreline, inset half a bar from its outside.
            let (hw, hh) = (w / 2.0 - d / 2.0, h / 2.0 - d / 2.0);
            let mid = (bottom - top) / 2.0;
            let rad = bt.stirrup_bend / 2.0 + d / 2.0;
            let hk = hook(store, "stirrup_135");
            let ext = extension(&hk, d);
            let bars = pos
                .iter()
                .map(|t| {
                    let o = add(add(f.start, mul(f.axis, c0 + d / 2.0 + t)), mul(f.deep, mid));
                    let c = |sx: f64, sy: f64| add(o, add(mul(f.across, sx * hw), mul(f.deep, sy * hh)));
                    // From the top corner round and back to it, a bar apart along the member,
                    // with both 135° hooks turned into the core.
                    let (p0, p1, p2, p3) = (c(-1.0, 1.0), c(-1.0, -1.0), c(1.0, -1.0), c(1.0, 1.0));
                    let into = unit(sub(f.across, f.deep));
                    let (a0, a1) = (mul(f.axis, -d / 2.0), mul(f.axis, d / 2.0));
                    let pts = vec![
                        add(add(p0, mul(into, ext)), a0),
                        add(p0, a0),
                        p1,
                        p2,
                        p3,
                        add(p0, a1),
                        add(add(p0, mul(into, ext)), a1),
                    ];
                    Bar { points: rounded(&pts, rad) }
                })
                .collect();
            Ok(SetGeometry {
                bars,
                dimensions: vec![("A".into(), w), ("B".into(), h), ("C".into(), ext), ("D".into(), ext)],
                role: "ligature".into(),
                array_length: span,
                indices: Vec::new(),
            })
        }
        (Placement::Face { face }, HostKind::Column | HostKind::Beam) => {
            let f = crate::steel_detail::member_frame(store, host).ok_or_else(|| Error::new("rebar.no_host", "the host does not regenerate"))?;
            let (top, bottom, side) = if kind == HostKind::Beam { (cov("top"), cov("bottom"), cov("other")) } else { (cov("other"), cov("other"), cov("other")) };
            // The face's normal and the direction the bars spread in.
            let (n, across, half_n, half_a, c_n, c_a) = match face.as_str() {
                "top" => (f.deep, f.across, f.ty.depth / 2.0, f.ty.width / 2.0, top, side),
                "bottom" => (mul(f.deep, -1.0), f.across, f.ty.depth / 2.0, f.ty.width / 2.0, bottom, side),
                "left" => (f.across, f.deep, f.ty.width / 2.0, f.ty.depth / 2.0, side, top.max(bottom)),
                "right" => (mul(f.across, -1.0), f.deep, f.ty.width / 2.0, f.ty.depth / 2.0, side, top.max(bottom)),
                other => return Err(Error::new("rebar.face", format!("no face {other} (top, bottom, left, right)"))),
            };
            let spread = fit("width", 2.0 * half_a - 2.0 * c_a)? - d;
            let axis_len = len(sub(f.end, f.start));
            let (c0, c1) = if kind == HostKind::Column { (cov("bottom"), cov("top")) } else { (cov("other"), cov("other")) };
            let run = fit("length", axis_len - c0 - c1)?;
            let pos = positions(&r.layout, spread, d)?;
            let depth_in = 2.0 * half_n - c_n - if face == "top" { bottom } else if face == "bottom" { top } else { side };
            let legs = match sh.code.as_str() {
                "00" => 0,
                "11" => 1,
                "21" => 2,
                _ => return Err(Error::new("rebar.shape", "bars at a face take shapes 00, 11 or 21")),
            };
            let leg = if legs > 0 {
                let want = extension(&hook(store, "standard_90"), d) + bt.standard_bend / 2.0 + d;
                let room = fit("depth", depth_in)?;
                let letter = if legs == 1 { "B" } else { "A" };
                segment_override(r, letter, room.min(want))? .min(room) - d / 2.0
            } else {
                0.0
            };
            // A run set by hand starts where the bar starts.
            let run = segment_override(r, if legs == 2 { "B" } else { "A" }, run)?;
            let rad = bt.standard_bend / 2.0 + d / 2.0;
            let bars = pos
                .iter()
                .map(|t| {
                    let base = add(add(f.start, mul(n, half_n - c_n - d / 2.0)), mul(across, -half_a + c_a + d / 2.0 + t));
                    let a = add(base, mul(f.axis, c0));
                    let b = add(a, mul(f.axis, run));
                    let down = mul(n, -leg);
                    let pts = match legs {
                        0 => vec![a, b],
                        1 => vec![a, b, add(b, down)],
                        _ => vec![add(a, down), a, b, add(b, down)],
                    };
                    Bar { points: rounded(&pts, rad) }
                })
                .collect();
            let outside = |v: f64| v + d / 2.0;
            let dims = match legs {
                0 => vec![("A".into(), run)],
                1 => vec![("A".into(), run), ("B".into(), outside(leg))],
                _ => vec![("A".into(), outside(leg)), ("B".into(), run), ("C".into(), outside(leg))],
            };
            Ok(SetGeometry { bars, dimensions: dims, role: "main".into(), array_length: spread, indices: Vec::new() })
        }
        (Placement::Layer { face, angle, vertical, offset, boundary, hook: hook_id, hook_flip }, HostKind::Floor | HostKind::Wall) => {
            if sh.code != "00" {
                return Err(Error::new("rebar.shape", "layers in floors and walls take shape 00 (straight), with hooks if wanted"));
            }
            let pl = host_plane(store, host, kind, face, d, *offset)?;
            let polys = pl.clipped(boundary.as_deref());
            if polys.is_empty() {
                return Err(Error::new("rebar.no_fit", "the boundary does not overlap the host"));
            }
            // The bars' direction in the plane, and the direction they spread in.
            let a = if kind == HostKind::Wall && *vertical { 90.0f64 } else { *angle }.to_radians();
            let wall_vertical = kind == HostKind::Wall && (a.sin().abs() > 1.0 - 1e-9);
            let wall_horizontal = kind == HostKind::Wall && (a.cos().abs() > 1.0 - 1e-9);
            let (bd, sd) = ([a.cos(), a.sin()], [-a.sin(), a.cos()]);
            let proj = |p: &[f64; 2], v: [f64; 2]| p[0] * v[0] + p[1] * v[1];
            let (lo, hi) = polys.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(proj(p, sd)), b.max(proj(p, sd))));
            let edge = cov("other");
            let (wall_top, wall_bottom) = (cov("top"), cov("bottom"));
            let (s0, s1) = if wall_horizontal { (lo + wall_bottom, hi - wall_top) } else { (lo + edge, hi - edge) };
            let spread = fit("width", s1 - s0)? - d;
            let pos = positions(&r.layout, spread, d)?;
            let hk = hook_id.as_deref().map(|h| hook_type(store, h)).transpose()?;
            let inward = if *hook_flip { mul(pl.inward, -1.0) } else { pl.inward };
            let mut bars = Vec::new();
            for t in pos {
                let at = s0 + d / 2.0 + t;
                let p = [sd[0] * at, sd[1] * at];
                for (t0, t1) in chord(&polys, p, bd) {
                    let (e0, e1) = if wall_vertical { (wall_bottom, wall_top) } else { (edge, edge) };
                    let (t0, t1) = (t0 + e0, t1 - e1);
                    if t1 - t0 <= d {
                        continue;
                    }
                    let (a3, b3) = (pl.at([p[0] + bd[0] * t0, p[1] + bd[1] * t0]), pl.at([p[0] + bd[0] * t1, p[1] + bd[1] * t1]));
                    let pts = match &hk {
                        Some(h) => {
                            let dir = unit(sub(b3, a3));
                            let mut v = hook_points(a3, mul(dir, -1.0), inward, h, &bt, pl.room);
                            v.reverse();
                            v.extend(hook_points(b3, dir, inward, h, &bt, pl.room));
                            rounded(&v, bt.hook_bend / 2.0 + d / 2.0)
                        }
                        None => vec![a3, b3],
                    };
                    bars.push(Bar { points: pts });
                }
            }
            // A length set by hand caps every bar from its start.
            if let Some(cap) = r.edits.segments.get("A") {
                for b in &mut bars {
                    let (a, e) = (b.points[0], b.points[b.points.len() - 1]);
                    if b.points.len() == 2 && len(sub(e, a)) > *cap {
                        b.points[1] = add(a, mul(unit(sub(e, a)), *cap));
                    }
                }
            }
            let longest = bars.iter().map(Bar::length).fold(0.0, f64::max);
            Ok(SetGeometry { bars, dimensions: vec![("A".into(), longest)], role: "main".into(), array_length: spread, indices: Vec::new() })
        }
        (Placement::Through { along }, HostKind::Wall) => {
            if sh.code != "00" {
                return Err(Error::new("rebar.shape", "bars through a wall take shape 00 (straight)"));
            }
            let w = crate::walls::wall_recipe(host).ok_or_else(|| Error::new("rebar.no_host", "the host is not a Reviera wall"))?;
            if !w.through.is_empty() {
                return Err(Error::new(
                    "rebar.curved",
                    "the wall is curved: bars through it need straight faces (inclined faces are fine, arcs and splines are not yet)",
                ));
            }
            let g = crate::walls::regenerate(store, host)?;
            let (dir, nrm) = (g.direction(), g.normal());
            let ext = if w.flip { -1.0 } else { 1.0 };
            let l = g.length();
            let edge = cov("other") + d / 2.0;
            if *along < edge - 1e-9 || *along > l - edge + 1e-9 {
                return Err(Error::new(
                    "rebar.no_fit",
                    format!("{:.0} mm along is outside the wall's {:.0} mm less its cover", along * 1000.0, l * 1000.0),
                ));
            }
            let (cb, ct) = (cov("bottom"), cov("top"));
            let span = fit("height", g.height - cb - ct)? - d;
            let pos = positions(&r.layout, span, d)?;
            // Each face's place across the wall (toward its exterior from the location line) at
            // a height, and its lean from vertical.
            let half = g.width / 2.0;
            let (lean_e, lean_i) = match w.cross_section {
                Some(crate::walls::cross_section::CrossSection::Slanted { angle }) => (angle, angle),
                Some(crate::walls::cross_section::CrossSection::Tapered { exterior, interior }) => (exterior, interior),
                None => (0.0, 0.0),
            };
            let faces = |h: f64| match w.cross_section {
                Some(crate::walls::cross_section::CrossSection::Slanted { angle }) => {
                    let shift = -h * angle.tan() * ext;
                    (half + shift, -half + shift)
                }
                Some(crate::walls::cross_section::CrossSection::Tapered { exterior, interior }) => (half - h * exterior.tan(), -half + h * interior.tan()),
                None => (half, -half),
            };
            // The covers measured across the wall, square to the leaning faces.
            let (ce, ci) = (cov("exterior") / lean_e.cos(), cov("interior") / lean_i.cos());
            let fitted: Vec<(f64, f64, f64)> = pos
                .iter()
                .map(|t| {
                    let h = cb + d / 2.0 + t;
                    let (se, si) = faces(h);
                    let (s0, s1) = (si + ci, se - ce);
                    if s1 - s0 <= d {
                        return Err(Error::new(
                            "rebar.no_fit",
                            format!("the bars do not fit: {:.0} mm up the wall is {:.0} mm inside the cover", h * 1000.0, (s1 - s0).max(0.0) * 1000.0),
                        ));
                    }
                    Ok((h, s0, s1))
                })
                .collect::<Result<_>>()?;
            // A standard set's bars all take the first one's length, centred in their section.
            let first = fitted.first().map_or(0.0, |(_, s0, s1)| s1 - s0);
            let first = segment_override(r, "A", first)?.min(first);
            let at = |h: f64, s: f64| {
                let q = [g.start[0] + dir[0] * along + nrm[0] * ext * s, g.start[1] + dir[1] * along + nrm[1] * ext * s];
                [q[0], q[1], g.base_z + h]
            };
            let bars = fitted
                .iter()
                .map(|&(h, s0, s1)| {
                    let (s0, s1) = if r.varying {
                        (s0, s1)
                    } else {
                        let m = (s0 + s1) / 2.0;
                        (m - first / 2.0, m + first / 2.0)
                    };
                    Bar { points: vec![at(h, s0), at(h, s1)] }
                })
                .collect();
            Ok(SetGeometry { bars, dimensions: vec![("A".into(), first)], role: "main".into(), array_length: span, indices: Vec::new() })
        }
        (p, k) => Err(Error::new(
            "rebar.placement",
            format!("a {k:?} takes {}, not {p:?}", match k { HostKind::Floor => "layers", HostKind::Wall => "layers or bars through it", _ => "section or face placements" }),
        )),
    }
}

// ---- Commands ----

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PlaceParams {
    pub host_id: ElementRef,
    /// A shape code from `rebar_shape.list` (00, 11, 21, 51).
    pub shape: String,
    /// A bar type from `rebar_bar_type.list` (`#5`, `d16`, …).
    pub bar_type: String,
    /// Only `expand` (Expand to Host) yet.
    #[serde(default = "expand")]
    pub method: String,
    pub placement: Placement,
    #[serde(default)]
    pub layout: Layout,
    /// A varying set: each bar fitted to the host at its own place (STR-RBR-005).
    #[serde(default)]
    pub varying: bool,
}

fn expand() -> String {
    "expand".into()
}

/// Places a rebar set (`rebar.place`).
pub(crate) fn place(txn: &mut crate::txn::Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    if p.method != "expand" {
        return Err(Error::new("rebar.method", format!("placement by {} is not built yet (only expand)", p.method)));
    }
    let id = store.resolve(&p.host_id).ok_or_else(|| Error::new("element.not_found", format!("no element {:?}", p.host_id)))?;
    let host = store.get(id).expect("resolved");
    host_check(&store, host).map_err(|why| Error::new("rebar.not_a_host", format!("element {id} cannot host rebar: {why}")))?;
    let bt = bar_type(&store, &p.bar_type)?;
    let r = RebarRecipe {
        kind: REBAR_RECIPE.into(),
        host: host.guid().to_string(),
        shape: p.shape.clone(),
        bar_type: bt.id.clone(),
        placement: p.placement.clone(),
        layout: p.layout.clone(),
        edits: BarEdits::default(),
        splice: None,
        varying: p.varying,
    };
    if p.varying {
        varying_check(&r)?;
    }
    let g = geometry(&store, &r)?;
    let predefined = if g.role == "ligature" { "LIGATURE" } else { "MAIN" };
    let id = txn.create(
        NewElement::new("IfcReinforcingBar")
            .param("@PredefinedType", predefined)
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    crate::rebar_numbers::persist(txn)?;
    Ok(id)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct LayoutParams {
    pub rebar_ids: Vec<ElementRef>,
    #[serde(flatten)]
    pub layout: Layout,
}

/// Changes sets' layouts (`rebar.set_layout`).
pub(crate) fn set_layout(txn: &mut crate::txn::Transaction<'_>, p: LayoutParams) -> Result<()> {
    for r in &p.rebar_ids {
        let store = txn.store().clone();
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let mut rec = store.get(id).and_then(rebar_recipe).ok_or_else(|| Error::new("rebar.not_rebar", format!("element {id} is not a rebar set")))?;
        rec.layout = p.layout.clone();
        geometry(&store, &rec)?;
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
        // A spliced set's other part changes with it (STR-RBR-015).
        if let Some(pid) = crate::rebar_edit::partner(&store, &rec)
            && let Some(mut other) = store.get(pid).and_then(rebar_recipe)
        {
            other.layout = rec.layout.clone();
            other.bar_type = rec.bar_type.clone();
            other.placement = rec.placement.clone();
            other.host = rec.host.clone();
            geometry(&store, &other)?;
            txn.set_param(pid, RECIPE_KEY, serde_json::to_value(&other).expect("serialises"))?;
        }
    }
    crate::rebar_numbers::persist(txn)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetParams {
    pub rebar_ids: Vec<ElementRef>,
    #[serde(default)]
    pub bar_type: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub placement: Option<Placement>,
}

/// Changes sets' bar type, shape or placement (`rebar.set`).
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    for r in &p.rebar_ids {
        let store = txn.store().clone();
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let mut rec = store.get(id).and_then(rebar_recipe).ok_or_else(|| Error::new("rebar.not_rebar", format!("element {id} is not a rebar set")))?;
        if let Some(b) = &p.bar_type {
            rec.bar_type = bar_type(&store, b)?.id;
        }
        if let Some(s) = &p.shape {
            rec.shape = s.clone();
        }
        if let Some(pl) = &p.placement {
            rec.placement = pl.clone();
        }
        geometry(&store, &rec)?;
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
        // A spliced set's other part changes with it (STR-RBR-015).
        if let Some(pid) = crate::rebar_edit::partner(&store, &rec)
            && let Some(mut other) = store.get(pid).and_then(rebar_recipe)
        {
            other.layout = rec.layout.clone();
            other.bar_type = rec.bar_type.clone();
            other.placement = rec.placement.clone();
            other.host = rec.host.clone();
            geometry(&store, &other)?;
            txn.set_param(pid, RECIPE_KEY, serde_json::to_value(&other).expect("serialises"))?;
        }
    }
    crate::rebar_numbers::persist(txn)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RehostParams {
    pub rebar_ids: Vec<ElementRef>,
    pub host_id: ElementRef,
}

/// Moves sets to another host (`rebar.rehost`).
pub(crate) fn rehost(txn: &mut crate::txn::Transaction<'_>, p: RehostParams) -> Result<()> {
    let store = txn.store().clone();
    let hid = store.resolve(&p.host_id).ok_or_else(|| Error::new("element.not_found", format!("no element {:?}", p.host_id)))?;
    let host = store.get(hid).expect("resolved");
    host_check(&store, host).map_err(|why| Error::new("rebar.not_a_host", format!("element {hid} cannot host rebar: {why}")))?;
    for r in &p.rebar_ids {
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let mut rec = store.get(id).and_then(rebar_recipe).ok_or_else(|| Error::new("rebar.not_rebar", format!("element {id} is not a rebar set")))?;
        rec.host = host.guid().to_string();
        geometry(&store, &rec)?;
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
        // A spliced set's other part changes with it (STR-RBR-015).
        if let Some(pid) = crate::rebar_edit::partner(&store, &rec)
            && let Some(mut other) = store.get(pid).and_then(rebar_recipe)
        {
            other.layout = rec.layout.clone();
            other.bar_type = rec.bar_type.clone();
            other.placement = rec.placement.clone();
            other.host = rec.host.clone();
            geometry(&store, &other)?;
            txn.set_param(pid, RECIPE_KEY, serde_json::to_value(&other).expect("serialises"))?;
        }
    }
    crate::rebar_numbers::persist(txn)
}

// ---- Varying sets (STR-RBR-005) ----

/// Whether a set can vary: only bars through a wall's thickness yet.
fn varying_check(r: &RebarRecipe) -> Result<()> {
    match r.placement {
        Placement::Through { .. } => Ok(()),
        _ => Err(Error::new(
            "rebar.not_varying",
            "only bars through a wall's thickness can vary yet; layers already follow the host's outline bar by bar",
        )),
    }
}

/// A bar length as schedules and numbers take it: bar-length rounding, else to the millimetre.
pub fn rounded_length(store: &Store, l: f64) -> f64 {
    let s = settings(store);
    if s.rounding.on { s.rounding.round(l, s.rounding.bar_length) } else { (l * 1000.0).round() / 1000.0 }
}

/// A set's bars grouped by rounded length, in the order the lengths first appear: each group's
/// length and its bars' places in the set.
pub fn length_groups(store: &Store, g: &SetGeometry) -> Vec<(f64, Vec<usize>)> {
    let mut out: Vec<(f64, Vec<usize>)> = Vec::new();
    for (k, b) in g.bars.iter().enumerate() {
        let l = rounded_length(store, b.length());
        match out.iter_mut().find(|(m, _)| (m - l).abs() < 1e-9) {
            Some((_, v)) => v.push(k),
            None => out.push((l, vec![k])),
        }
    }
    out
}

/// Whether a set is varying.
pub fn is_varying(e: &Element) -> bool {
    rebar_recipe(e).is_some_and(|r| r.varying)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ConvertParams {
    pub rebar_ids: Vec<ElementRef>,
}

/// Makes sets varying (`rebar.convert_to_varying`).
pub(crate) fn convert_to_varying(txn: &mut crate::txn::Transaction<'_>, p: ConvertParams) -> Result<()> {
    for r in &p.rebar_ids {
        let store = txn.store().clone();
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let mut rec = store.get(id).and_then(rebar_recipe).ok_or_else(|| Error::new("rebar.not_rebar", format!("element {id} is not a rebar set")))?;
        varying_check(&rec)?;
        rec.varying = true;
        geometry(&store, &rec)?;
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
    }
    crate::rebar_numbers::persist(txn)
}

/// Makes varying sets standard (`rebar.convert_to_standard`), refused while their bars have
/// more than one length.
pub(crate) fn convert_to_standard(txn: &mut crate::txn::Transaction<'_>, p: ConvertParams) -> Result<()> {
    for r in &p.rebar_ids {
        let store = txn.store().clone();
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let mut rec = store.get(id).and_then(rebar_recipe).ok_or_else(|| Error::new("rebar.not_rebar", format!("element {id} is not a rebar set")))?;
        if !rec.varying {
            continue;
        }
        let groups = length_groups(&store, &geometry(&store, &rec)?);
        if groups.len() > 1 {
            let mut ls: Vec<f64> = groups.iter().map(|(l, _)| *l).collect();
            ls.sort_by(f64::total_cmp);
            let list: Vec<String> = ls.iter().map(|l| format!("{:.0}", l * 1000.0)).collect();
            return Err(Error::new(
                "rebar.lengths_differ",
                format!("set {id}'s bars have {} different lengths ({} mm); a standard set needs one length", groups.len(), list.join(", ")),
            )
            .with_details(serde_json::json!({ "lengths": groups.len() })));
        }
        rec.varying = false;
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&rec).expect("serialises"))?;
    }
    crate::rebar_numbers::persist(txn)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RebarInfo {
    pub id: ElementId,
    pub host: Option<ElementId>,
    /// The host no longer qualifies (the bars are kept).
    pub orphaned: bool,
    pub orphan_reason: Option<String>,
    pub shape: String,
    pub bar_type: String,
    pub diameter: f64,
    pub count: usize,
    pub layout: Layout,
    pub array_length: f64,
    /// Each bar's length, exact and rounded (metres).
    pub lengths: Vec<f64>,
    pub rounded_lengths: Vec<f64>,
    /// The first bar's length as Properties show it: "exact [rounded]" in mm.
    pub length_text: String,
    pub total_length: f64,
    pub mass: f64,
    pub dimensions: Vec<(String, f64)>,
    pub role: String,
    pub bars: Vec<Bar>,
    /// Each bar's index before the set's edits (for removing, moving and splitting).
    pub bar_indices: Vec<usize>,
    /// The rebar number (STR-RBR-016) and partition ("" for none).
    pub number: Option<u32>,
    pub partition: String,
    /// Each bar fitted at its own place (STR-RBR-005).
    pub varying: bool,
    /// A varying set's length groups: rounded length, bar count and number.
    pub length_groups: Vec<LengthGroup>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LengthGroup {
    pub length: f64,
    pub count: usize,
    pub number: Option<u32>,
}

impl RebarInfo {
    /// The shortest and longest rounded length, when they differ.
    pub fn length_range(&self) -> Option<(f64, f64)> {
        let lo = self.rounded_lengths.iter().copied().reduce(f64::min)?;
        let hi = self.rounded_lengths.iter().copied().reduce(f64::max)?;
        (hi - lo > 1e-9).then_some((lo, hi))
    }

    /// The numbers of a varying set's groups, lowest and highest, when they differ.
    pub fn number_range(&self) -> Option<(u32, u32)> {
        let lo = self.length_groups.iter().filter_map(|g| g.number).min()?;
        let hi = self.length_groups.iter().filter_map(|g| g.number).max()?;
        (hi > lo).then_some((lo, hi))
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct InfoParams {
    pub rebar_ids: Vec<ElementRef>,
}

pub fn info(store: &Store, p: InfoParams) -> Result<Vec<RebarInfo>> {
    let s = settings(store);
    let numbers = crate::rebar_numbers::numbers(store);
    p.rebar_ids
        .iter()
        .map(|r| {
            let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let rec = store.get(id).and_then(rebar_recipe).ok_or_else(|| Error::new("rebar.not_rebar", format!("element {id} is not a rebar set")))?;
            let host_id = GlobalId::parse(&rec.host).and_then(|g| store.find_by_guid(&g));
            let orphan = host_id
                .and_then(|h| store.get(h))
                .map_or(Some("the host is gone".to_owned()), |h| host_check(store, h).err());
            let bt = bar_type(store, &rec.bar_type)?;
            let g = geometry(store, &rec)?;
            let lengths: Vec<f64> = g.bars.iter().map(Bar::length).collect();
            let rounded: Vec<f64> = lengths.iter().map(|l| s.rounding.round(*l, s.rounding.bar_length)).collect();
            let lo = rounded.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = rounded.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let text = match (lengths.first(), rounded.first()) {
                _ if rec.varying && hi - lo > 1e-9 => format!("{:.0}–{:.0}", lo * 1000.0, hi * 1000.0),
                (Some(l), Some(r)) if s.rounding.on => format!("{:.0} [{:.0}]", l * 1000.0, r * 1000.0),
                (Some(l), _) => format!("{:.0}", l * 1000.0),
                _ => String::new(),
            };
            let total: f64 = rounded.iter().sum();
            let mine: Vec<u32> = numbers.iter().filter(|(i, _)| i.element == id).map(|(_, n)| *n).collect();
            let groups = if rec.varying {
                length_groups(store, &g)
                    .into_iter()
                    .enumerate()
                    .map(|(k, (length, bars))| LengthGroup { length, count: bars.len(), number: mine.get(k).copied() })
                    .collect()
            } else {
                Vec::new()
            };
            let area = std::f64::consts::PI * bt.diameter * bt.diameter / 4.0;
            Ok(RebarInfo {
                id,
                host: host_id,
                orphaned: orphan.is_some(),
                orphan_reason: orphan,
                shape: rec.shape.clone(),
                bar_type: bt.name.clone(),
                diameter: bt.diameter,
                count: g.bars.len(),
                layout: rec.layout.clone(),
                array_length: g.array_length,
                lengths,
                rounded_lengths: rounded,
                length_text: text,
                total_length: total,
                mass: total * area * 7850.0,
                dimensions: g.dimensions.clone(),
                role: g.role.clone(),
                bar_indices: g.indices.clone(),
                bars: g.bars,
                number: mine.first().copied(),
                partition: store.get(id).map(crate::rebar_numbers::partition).unwrap_or_default(),
                varying: rec.varying,
                length_groups: groups,
            })
        })
        .collect()
}

/// A set's body: a tube along each bar.
pub fn mesh(store: &Store, e: &Element) -> Option<crate::bodies::Mesh> {
    let r = rebar_recipe(e)?;
    let bt = bar_type(store, &r.bar_type).ok()?;
    let g = geometry(store, &r).ok()?;
    Some(bars_mesh(&g.bars.iter().collect::<Vec<_>>(), bt.model_diameter))
}

/// Tubes along bars of a diameter.
pub fn bars_mesh(bars: &[&Bar], diameter: f64) -> crate::bodies::Mesh {
    let mut parts = Vec::new();
    for b in bars {
        for w in b.points.windows(2) {
            if len(sub(w[1], w[0])) > 1e-6 {
                parts.push(crate::steel_fab::cylinder(w[0], w[1], diameter / 2.0, 8));
            }
        }
    }
    crate::steel_fab::join(parts)
}

/// A varying set's length groups for IFC (STR-RBR-005): each group's rounded length, number and
/// body.
pub fn varying_parts(store: &Store, e: &Element) -> Option<Vec<(f64, Option<u32>, crate::bodies::Mesh)>> {
    let r = rebar_recipe(e).filter(|r| r.varying)?;
    let bt = bar_type(store, &r.bar_type).ok()?;
    let g = geometry(store, &r).ok()?;
    let numbers = crate::rebar_numbers::number_of(store, e.id());
    Some(
        length_groups(store, &g)
            .into_iter()
            .enumerate()
            .map(|(k, (l, idx))| (l, numbers.get(k).copied(), bars_mesh(&idx.iter().map(|i| &g.bars[*i]).collect::<Vec<_>>(), bt.model_diameter)))
            .collect(),
    )
}

/// Whether an element goes with `guid`: a set in it.
pub fn uses(e: &Element, guid: &str) -> bool {
    rebar_recipe(e).is_some_and(|r| r.host == guid)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CanHostParams {
    pub element_id: ElementRef,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CanHost {
    pub can_host: bool,
    pub kind: Option<HostKind>,
    pub reason: Option<String>,
    /// Each face's cover type.
    pub faces: std::collections::BTreeMap<String, String>,
}

pub fn can_host(store: &Store, p: CanHostParams) -> Result<CanHost> {
    let id = store.resolve(&p.element_id).ok_or_else(|| Error::new("element.not_found", format!("no element {:?}", p.element_id)))?;
    let e = store.get(id).expect("resolved");
    let check = host_check(store, e);
    Ok(CanHost {
        can_host: check.is_ok(),
        kind: host_kind(store, e),
        reason: check.err(),
        faces: host_kind(store, e).map(|k| faces_of(store, e, k)).unwrap_or_default(),
    })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BarTypeParams {
    pub name: String,
    pub diameter: f64,
    #[serde(default)]
    pub material: Option<String>,
    /// Inside bend diameter (default the region's rule for the size).
    #[serde(default)]
    pub bend_diameter: Option<f64>,
}

/// Makes a project bar type (`rebar_bar_type.create`).
pub(crate) fn bar_type_create(txn: &mut crate::txn::Transaction<'_>, p: BarTypeParams) -> Result<String> {
    let store = txn.store().clone();
    let mut s = settings(&store);
    if p.name.trim().is_empty() || bar_types(&store).iter().any(|b| b.name == p.name || b.id == p.name) {
        return Err(Error::new("rebar_bar_type.name", "a bar type's name is unique and not empty"));
    }
    if !(0.003..=0.08).contains(&p.diameter) {
        return Err(Error::new("rebar_bar_type.diameter", "a bar is 3 to 80 mm in diameter"));
    }
    let bend = p.bend_diameter.unwrap_or(if us(&store) { 6.0 } else if p.diameter <= 0.016 { 4.0 } else { 7.0 } * p.diameter);
    let material = p.material.unwrap_or_else(|| catalogue(us(&store))[0].material.clone());
    s.bar_types.push(BarType {
        id: p.name.clone(),
        name: p.name.clone(),
        diameter: p.diameter,
        model_diameter: p.diameter,
        material,
        deformed: true,
        standard_bend: bend,
        hook_bend: bend,
        stirrup_bend: bend,
    });
    save(txn, &s)?;
    Ok(p.name)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct HookTypeParams {
    pub name: String,
    pub angle: f64,
    pub extension_multiplier: f64,
    #[serde(default)]
    pub minimum: f64,
}

/// Makes a project hook type (`rebar_hook_type.create`).
pub(crate) fn hook_type_create(txn: &mut crate::txn::Transaction<'_>, p: HookTypeParams) -> Result<String> {
    let store = txn.store().clone();
    let mut s = settings(&store);
    if !(0.0..=180.0).contains(&p.angle) || p.extension_multiplier < 0.0 {
        return Err(Error::new("rebar_hook_type.value", "a hook's angle is 0 to 180° and its multiplier not negative"));
    }
    let id = p.name.to_ascii_lowercase().replace(' ', "_");
    if all_hook_types(&store).iter().any(|h| h.id == id) {
        return Err(Error::new("rebar_hook_type.name", format!("there is already a hook type {id}")));
    }
    s.hook_types.push(HookType { id: id.clone(), name: p.name, angle: p.angle, multiplier: p.extension_multiplier, minimum: p.minimum });
    save(txn, &s)?;
    Ok(id)
}

/// The settings with the bar and hook types (`reinforcement_settings.get`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SettingsInfo {
    #[serde(flatten)]
    pub settings: Settings,
    /// Each cover type's uses.
    pub cover_usage: std::collections::BTreeMap<String, usize>,
    /// Locked settings, with why.
    pub locked: std::collections::BTreeMap<String, String>,
}

pub fn settings_info(store: &Store) -> SettingsInfo {
    let s = settings(store);
    let mut locked = std::collections::BTreeMap::new();
    if store.iter().any(|e| rebar_recipe(e).is_some()) {
        locked.insert("hooks_in_shape".into(), "rebar has been placed".into());
    }
    if crate::rebar_systems::any_area_or_path(store) {
        locked.insert("area_path_rebar".into(), "an area or path system exists".into());
    }
    SettingsInfo {
        cover_usage: s.cover_types.iter().map(|c| (c.id.clone(), usage(store, &c.id))).collect(),
        locked,
        settings: s,
    }
}
