// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Embodied carbon (ANA-CARB-001): global warming potential (kg CO₂e) from material quantities
//! and emission factors, by life-cycle stage, a built-in provider (family `carbon`).
//!
//! - **Quantities**: walls (each layer's thickness × the wall's area less its openings), floors
//!   and ceilings (each layer's thickness × the area), roofs (the volume, as one material of the
//!   type's). Each line is an element, a material, its volume and area. Other elements are not
//!   quantified yet and are listed as such.
//! - **Factors** come from libraries the user imports (Reviera ships none): each factor has a
//!   unit (per kg, m³, m² or piece), values per stage (A1–A3, A4, A5, C1–C4, module D,
//!   biogenic), and optionally a density and a reference service life. The documented CSV
//!   format is in [`parse_csv`].
//! - **Matching**: each project material is matched to a factor by hand (`embodied_carbon.match`);
//!   `embodied_carbon.auto_match` proposes factors by name and class, to confirm. Unmatched
//!   materials are reported, never guessed.
//! - **Calculation**: GWP = quantity × factor per stage; mass = volume × density (the factor's,
//!   else none: the line is excluded with the reason); B4 = (A1–A3 + C1–C4) × replacements,
//!   replacements = ceil(study period ÷ service life) − 1. Module D and biogenic carbon are
//!   reported apart and never netted into the A–C total.
//! - **Runs** are compute jobs (`analysis.carbon`, ANA-ENV-001): the editor distils the
//!   quantities and matched factors ([`distill`]), the built-in runner calculates ([`compute`]),
//!   and the editor records the results (`embodied_carbon.record`) with the factors they used
//!   and each element's inputs, so a run reports itself stale for exactly the elements whose
//!   quantities or matches changed.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::ElementId;
use crate::store::Store;
use crate::txn::Transaction;

pub const KEY: &str = "rxd:carbon";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// What a factor is declared per.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Kg,
    M3,
    M2,
    Piece,
}

/// An emission factor (kg CO₂e per unit) by stage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Stages {
    pub a1_a3: f64,
    pub a4: Option<f64>,
    pub a5: Option<f64>,
    pub c1_c4: Option<f64>,
    pub d: Option<f64>,
    pub biogenic: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Factor {
    pub id: String,
    pub name: String,
    /// A class to match by (concrete, steel, timber…).
    #[serde(default)]
    pub class: String,
    pub unit: Unit,
    pub stages: Stages,
    /// kg/m³, for factors per kg.
    #[serde(default)]
    pub density: Option<f64>,
    /// Years, for replacements (B4).
    #[serde(default)]
    pub service_life: Option<f64>,
    /// Generic or product-specific source, for data quality.
    #[serde(default)]
    pub source: String,
    /// The EPD's validity (YYYY-MM-DD).
    #[serde(default)]
    pub valid_until: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Library {
    pub id: String,
    pub name: String,
    /// The file it was read from.
    #[serde(default)]
    pub file: String,
    pub factors: Vec<Factor>,
}

/// The options of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Options {
    /// Report A4, A5, B4 and C1–C4 (A1–A3, D and biogenic are always reported).
    pub stages: Vec<String>,
    /// Years.
    pub study_period: f64,
    /// m², for the per-m² total; none: not normalised.
    pub gross_floor_area: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Options { stages: vec!["a4".into(), "a5".into(), "b4".into(), "c1_c4".into()], study_period: 50.0, gross_floor_area: None }
    }
}

/// GWP by stage (kg CO₂e).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Gwp {
    pub a1_a3: f64,
    pub a4: f64,
    pub a5: f64,
    pub b4: f64,
    pub c1_c4: f64,
    /// A1–A3 + A4 + A5 + B4 + C1–C4.
    pub total: f64,
    pub d: f64,
    pub biogenic: f64,
}

impl Gwp {
    fn add(&mut self, o: &Gwp) {
        self.a1_a3 += o.a1_a3;
        self.a4 += o.a4;
        self.a5 += o.a5;
        self.b4 += o.b4;
        self.c1_c4 += o.c1_c4;
        self.total += o.total;
        self.d += o.d;
        self.biogenic += o.biogenic;
    }
}

/// One element's material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Quantity {
    pub element: ElementId,
    pub category: String,
    pub material: String,
    /// m³ and m².
    pub volume: f64,
    pub area: f64,
}

/// An element's or material's result line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Line {
    pub key: String,
    pub gwp: Gwp,
    pub volume: f64,
}

/// A material left out, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Excluded {
    pub material: String,
    pub volume: f64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Run {
    pub id: String,
    /// Milliseconds since 1970 (kept for ordering; set by the caller).
    pub at: i64,
    pub options: Options,
    pub totals: Gwp,
    /// kg CO₂e per m² of gross floor area, A–C.
    pub per_m2: Option<f64>,
    pub by_category: Vec<Line>,
    pub by_material: Vec<Line>,
    /// By element GlobalId: A1–A3 and the A–C total.
    pub by_element: BTreeMap<String, Gwp>,
    pub excluded: Vec<Excluded>,
    /// The share of the quantified volume that is unmatched (0–1).
    pub unmatched_share: f64,
    /// The factors it used, by material.
    pub factors: BTreeMap<String, Factor>,
    /// Each element's inputs (materials and volumes), to tell when it is stale.
    pub inputs: BTreeMap<String, String>,
}

/// The project's carbon data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CarbonData {
    pub libraries: Vec<Library>,
    /// Material name → `library_id/factor_id`.
    pub matches: BTreeMap<String, String>,
    pub runs: Vec<Run>,
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub fn data(store: &Store) -> CarbonData {
    project(store).and_then(|p| p.param(KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

fn put(txn: &mut Transaction<'_>, d: &CarbonData) -> Result<()> {
    let p = project(txn.store()).map(Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(d).expect("serialises");
    txn.modify(p, |e| {
        e.params.insert(KEY.into(), v);
    })
}

fn type_of<'a>(store: &'a Store, e: &Element) -> Option<&'a Element> {
    let g = crate::id::GlobalId::parse(e.param("@type")?.as_str()?)?;
    store.get(store.find_by_guid(&g)?)
}

/// Every quantified element's materials, and the elements not quantified.
pub fn quantities(store: &Store) -> (Vec<Quantity>, Vec<ElementId>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let cuts = crate::inserts::cuts_by_host(store);
    let layers = |c: &crate::compound::Compound| -> Vec<(String, f64)> { c.layers.iter().filter(|l| l.thickness > 0.0).map(|l| (crate::compound::layer_material(l), l.thickness)).collect() };
    for e in store.iter() {
        let cat = crate::categories::category_of(e).map(|c| c.name.to_owned()).unwrap_or_else(|| e.class.trim_start_matches("Ifc").to_owned());
        if crate::walls::wall_recipe(e).is_some() {
            let (Ok(g), Some(t)) = (crate::walls::regenerate(store, e), type_of(store, e)) else { continue };
            let Some(c) = crate::compound::of_wall_type(t) else { continue };
            let open: f64 = cuts.get(&e.id()).map_or(0.0, |v| v.iter().map(|k| (k.x[1] - k.x[0]).abs() * (k.z[1] - k.z[0]).abs()).sum());
            let area = (g.length() * g.height - open).max(0.0);
            for (m, th) in layers(&c) {
                out.push(Quantity { element: e.id(), category: cat.clone(), material: m, volume: th * area, area });
            }
        } else if crate::floors::slab_recipe(e).is_some() {
            let (Ok(g), Some(t)) = (crate::floors::regenerate(store, e), type_of(store, e)) else { continue };
            let Some(c) = crate::compound::of_slab_type(t) else { continue };
            let area = g.area();
            for (m, th) in layers(&c) {
                out.push(Quantity { element: e.id(), category: cat.clone(), material: m, volume: th * area, area });
            }
        } else if crate::roofs::roof_recipe(e).is_some() {
            let Ok(g) = crate::roofs::regenerate(store, e) else { continue };
            let material = type_of(store, e)
                .and_then(|t| t.param(crate::walls::RECIPE_KEY)?.get("material")?.as_str().map(str::to_owned))
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| "Roof".into());
            out.push(Quantity { element: e.id(), category: cat.clone(), material, volume: g.volume(), area: g.area() });
        } else if crate::categories::category_of(e).is_some_and(|c| c.is_model) && e.param("@type").is_some() && !crate::annotations::is_annotation(e) {
            skipped.push(e.id());
        }
    }
    (out, skipped)
}

/// The factor a material is matched to.
pub fn factor_for<'a>(d: &'a CarbonData, material: &str) -> Option<&'a Factor> {
    let r = d.matches.get(material)?;
    let (lib, id) = r.split_once('/')?;
    d.libraries.iter().find(|l| l.id == lib)?.factors.iter().find(|f| f.id == id)
}

fn wants(o: &Options, stage: &str) -> bool {
    o.stages.iter().any(|s| s == stage)
}

/// A quantity's GWP by a factor, or why it cannot be had.
fn gwp(q: &Quantity, f: &Factor, o: &Options) -> std::result::Result<Gwp, String> {
    let amount = match f.unit {
        Unit::M3 => q.volume,
        Unit::M2 => q.area,
        Unit::Piece => 1.0,
        Unit::Kg => q.volume * f.density.ok_or("the factor is per kg and gives no density")?,
    };
    let s = &f.stages;
    let mut g = Gwp { a1_a3: amount * s.a1_a3, d: amount * s.d.unwrap_or(0.0), biogenic: amount * s.biogenic.unwrap_or(0.0), ..Gwp::default() };
    if wants(o, "a4") {
        g.a4 = amount * s.a4.unwrap_or(0.0);
    }
    if wants(o, "a5") {
        g.a5 = amount * s.a5.unwrap_or(0.0);
    }
    let c = amount * s.c1_c4.unwrap_or(0.0);
    if wants(o, "c1_c4") {
        g.c1_c4 = c;
    }
    if wants(o, "b4")
        && let Some(life) = f.service_life.filter(|l| *l > 0.0)
    {
        g.b4 = (g.a1_a3 + c) * replacements(o.study_period, life) as f64;
    }
    g.total = g.a1_a3 + g.a4 + g.a5 + g.b4 + g.c1_c4;
    Ok(g)
}

/// Replacements over a study period: ceil(period ÷ life) − 1.
pub fn replacements(period: f64, life: f64) -> u32 {
    ((period / life).ceil() as i64 - 1).max(0) as u32
}

/// An element's inputs, to tell when a run is stale.
fn signature(qs: &[&Quantity], d: &CarbonData) -> String {
    qs.iter().map(|q| format!("{}:{:.6}:{:.6}:{}", q.material, q.volume, q.area, d.matches.get(&q.material).cloned().unwrap_or_default())).collect::<Vec<_>>().join(";")
}

/// One quantity line of a carbon job, keyed by an opaque element key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InputLine {
    pub key: String,
    pub category: String,
    pub material: String,
    pub volume: f64,
    pub area: f64,
}

/// What an `analysis.carbon` job needs: the quantities, the factor matched to each material
/// (unmatched materials have none) and the options. Element keys are opaque; the map back to
/// GlobalIds stays in the editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CarbonInput {
    pub lines: Vec<InputLine>,
    pub factors: BTreeMap<String, Factor>,
    pub options: Options,
}

/// What a carbon job returns: totals, breakdowns, element results by key, exclusions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CarbonResults {
    pub totals: Gwp,
    pub per_m2: Option<f64>,
    pub by_category: Vec<Line>,
    pub by_material: Vec<Line>,
    /// By element key.
    pub by_element: BTreeMap<String, Gwp>,
    pub excluded: Vec<Excluded>,
    pub unmatched_share: f64,
}

/// A distillation: the job's input, the element key → GlobalId map and each element's inputs.
#[derive(Debug, Clone)]
pub struct Distilled {
    pub input: CarbonInput,
    pub key_map: BTreeMap<String, String>,
    pub inputs: BTreeMap<String, String>,
}

/// Distils the model for a carbon job.
pub fn distill(store: &Store, o: &Options) -> Distilled {
    let d = data(store);
    let (qs, _) = quantities(store);
    let mut keys: BTreeMap<ElementId, String> = BTreeMap::new();
    let mut key_map = BTreeMap::new();
    let mut lines = Vec::new();
    let mut factors = BTreeMap::new();
    for q in &qs {
        let n = keys.len();
        let key = keys.entry(q.element).or_insert_with(|| format!("e{}", n + 1)).clone();
        if let Some(e) = store.get(q.element) {
            key_map.insert(key.clone(), e.guid().to_string());
        }
        if let Some(f) = factor_for(&d, &q.material) {
            factors.insert(q.material.clone(), f.clone());
        }
        lines.push(InputLine { key, category: q.category.clone(), material: q.material.clone(), volume: q.volume, area: q.area });
    }
    let mut per: BTreeMap<ElementId, Vec<&Quantity>> = BTreeMap::new();
    for q in &qs {
        per.entry(q.element).or_default().push(q);
    }
    let inputs = per.into_iter().filter_map(|(id, v)| Some((store.get(id)?.guid().to_string(), signature(&v, &d)))).collect();
    Distilled { input: CarbonInput { lines, factors, options: o.clone() }, key_map, inputs }
}

/// Calculates a carbon job (the runner's side; pure).
pub fn compute(input: &CarbonInput) -> CarbonResults {
    let o = &input.options;
    let mut totals = Gwp::default();
    let mut by_cat: BTreeMap<String, Line> = BTreeMap::new();
    let mut by_mat: BTreeMap<String, Line> = BTreeMap::new();
    let mut by_el: BTreeMap<String, Gwp> = BTreeMap::new();
    let mut excluded: BTreeMap<(String, String), f64> = BTreeMap::new();
    let (mut vol, mut unmatched) = (0.0, 0.0);
    for l in &input.lines {
        vol += l.volume;
        let q = Quantity { element: ElementId(0), category: l.category.clone(), material: l.material.clone(), volume: l.volume, area: l.area };
        let Some(f) = input.factors.get(&l.material) else {
            unmatched += l.volume;
            *excluded.entry((l.material.clone(), "unmatched".into())).or_default() += l.volume;
            continue;
        };
        match gwp(&q, f, o) {
            Ok(g) => {
                totals.add(&g);
                by_el.entry(l.key.clone()).or_default().add(&g);
                for (map, key) in [(&mut by_cat, l.category.clone()), (&mut by_mat, l.material.clone())] {
                    let line = map.entry(key.clone()).or_insert_with(|| Line { key, gwp: Gwp::default(), volume: 0.0 });
                    line.gwp.add(&g);
                    line.volume += l.volume;
                }
            }
            Err(why) => {
                unmatched += l.volume;
                *excluded.entry((l.material.clone(), why)).or_default() += l.volume;
            }
        }
    }
    let sort = |m: BTreeMap<String, Line>| {
        let mut v: Vec<Line> = m.into_values().collect();
        v.sort_by(|a, b| b.gwp.total.total_cmp(&a.gwp.total));
        v
    };
    CarbonResults {
        per_m2: o.gross_floor_area.filter(|a| *a > 0.0).map(|a| totals.total / a),
        totals,
        by_category: sort(by_cat),
        by_material: sort(by_mat),
        by_element: by_el,
        excluded: excluded.into_iter().map(|((material, reason), volume)| Excluded { material, volume, reason }).collect(),
        unmatched_share: if vol > 0.0 { unmatched / vol } else { 0.0 },
    }
}

/// A run from a job's results and its distillation.
pub fn to_run(r: CarbonResults, key_map: &BTreeMap<String, String>, inputs: BTreeMap<String, String>, input: &CarbonInput) -> Run {
    Run {
        id: String::new(),
        at: 0,
        options: input.options.clone(),
        totals: r.totals,
        per_m2: r.per_m2,
        by_category: r.by_category,
        by_material: r.by_material,
        by_element: r.by_element.into_iter().filter_map(|(k, g)| Some((key_map.get(&k)?.clone(), g))).collect(),
        excluded: r.excluded,
        unmatched_share: r.unmatched_share,
        factors: input.factors.clone(),
        inputs,
    }
}

/// Works out a run in the editor (for staleness checks; runs proper are compute jobs).
pub fn calculate(store: &Store, o: &Options) -> Run {
    let d = distill(store, o);
    let r = compute(&d.input);
    to_run(r, &d.key_map, d.inputs, &d.input)
}

/// The elements whose inputs changed since a run (GlobalIds), and whether the factors did.
pub fn stale(store: &Store, run: &Run) -> (Vec<String>, bool) {
    let now = calculate(store, &run.options);
    let keys: BTreeSet<&String> = run.inputs.keys().chain(now.inputs.keys()).collect();
    let changed = keys.into_iter().filter(|k| run.inputs.get(*k) != now.inputs.get(*k)).cloned().collect();
    let d = data(store);
    let factors_changed = run.factors.iter().any(|(m, f)| factor_for(&d, m) != Some(f));
    (changed, factors_changed)
}

/// Reads factors from the documented CSV format: a header row naming the columns, then a row
/// per factor. Columns (case-insensitive; `id`, `name`, `unit` and `a1_a3` required):
/// `id, name, class, unit (kg|m3|m2|piece), a1_a3, a4, a5, c1_c4, d, biogenic, density,
/// service_life, source, valid_until`. Values in kg CO₂e per unit; empty cells are none.
pub fn parse_csv(text: &str) -> Result<Vec<Factor>> {
    let mut lines = text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
    let (_, head) = lines.next().ok_or_else(|| bad("carbon.csv", "the file is empty"))?;
    let cols: Vec<String> = split(head).into_iter().map(|c| c.trim().to_lowercase()).collect();
    let at = |name: &str| cols.iter().position(|c| c == name);
    let (Some(ci), Some(cn), Some(cu), Some(ca)) = (at("id"), at("name"), at("unit"), at("a1_a3")) else {
        return Err(bad("carbon.csv", "line 1: the header needs id, name, unit and a1_a3 columns"));
    };
    let mut out: Vec<Factor> = Vec::new();
    for (k, line) in lines {
        let n = k + 1;
        let c = split(line);
        let cell = |i: Option<usize>| i.and_then(|i| c.get(i)).map(|s| s.trim()).filter(|s| !s.is_empty());
        let num = |i: Option<usize>, what: &str| -> Result<Option<f64>> {
            cell(i).map(|s| s.parse::<f64>().map_err(|_| bad("carbon.csv", format!("line {n}: {what} {s:?} is not a number")))).transpose()
        };
        let id = cell(Some(ci)).ok_or_else(|| bad("carbon.csv", format!("line {n}: a factor needs an id")))?.to_owned();
        if out.iter().any(|f| f.id == id) {
            return Err(bad("carbon.csv", format!("line {n}: the id {id:?} is used twice")));
        }
        let unit = match cell(Some(cu)).map(str::to_lowercase).as_deref() {
            Some("kg") => Unit::Kg,
            Some("m3" | "m³") => Unit::M3,
            Some("m2" | "m²") => Unit::M2,
            Some("piece" | "pcs" | "unit") => Unit::Piece,
            other => return Err(bad("carbon.csv", format!("line {n}: the unit {:?} is not kg, m3, m2 or piece", other.unwrap_or("")))),
        };
        out.push(Factor {
            name: cell(Some(cn)).unwrap_or(&id).to_owned(),
            class: cell(at("class")).unwrap_or("").to_owned(),
            unit,
            stages: Stages {
                a1_a3: num(Some(ca), "a1_a3")?.ok_or_else(|| bad("carbon.csv", format!("line {n}: a1_a3 is required")))?,
                a4: num(at("a4"), "a4")?,
                a5: num(at("a5"), "a5")?,
                c1_c4: num(at("c1_c4"), "c1_c4")?,
                d: num(at("d"), "d")?,
                biogenic: num(at("biogenic"), "biogenic")?,
            },
            density: num(at("density"), "density")?,
            service_life: num(at("service_life"), "service_life")?,
            source: cell(at("source")).unwrap_or("").to_owned(),
            valid_until: cell(at("valid_until")).map(str::to_owned),
            id,
        });
    }
    if out.is_empty() {
        return Err(bad("carbon.csv", "the file has no factors"));
    }
    Ok(out)
}

/// Splits a CSV line (double quotes around cells with commas).
fn split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddLibraryParams {
    pub name: String,
    #[serde(default)]
    pub file: String,
    pub factors: Vec<Factor>,
}

/// `embodied_carbon.add_library`: its id.
pub(crate) fn add_library(txn: &mut Transaction<'_>, p: AddLibraryParams) -> Result<String> {
    let mut d = data(txn.store());
    let name = p.name.trim().to_owned();
    if name.is_empty() || p.factors.is_empty() {
        return Err(bad("carbon.library", "a library needs a name and factors"));
    }
    // The same name replaces the library (importing a file again), keeping its id and matches.
    let id = match d.libraries.iter().position(|l| l.name == name) {
        Some(k) => {
            d.libraries[k].factors = p.factors;
            d.libraries[k].file = p.file;
            d.libraries[k].id.clone()
        }
        None => {
            let id = format!("lib{}", d.libraries.len() + 1);
            d.libraries.push(Library { id: id.clone(), name, file: p.file, factors: p.factors });
            id
        }
    };
    put(txn, &d)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatchParams {
    /// The material's name.
    pub material: String,
    /// `library_id/factor_id`; null unmatches it.
    pub factor_ref: Option<String>,
}

/// `embodied_carbon.match`.
pub(crate) fn match_material(txn: &mut Transaction<'_>, p: MatchParams) -> Result<()> {
    let mut d = data(txn.store());
    match p.factor_ref {
        None => {
            d.matches.remove(&p.material);
        }
        Some(r) => {
            d.matches.insert(p.material.clone(), r.clone());
            if factor_for(&d, &p.material).is_none() {
                return Err(bad("carbon.factor", format!("no factor {r:?} (library_id/factor_id)")));
            }
        }
    }
    put(txn, &d)
}

/// A material of the model with its quantity and match.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MaterialRow {
    pub material: String,
    pub volume: f64,
    pub elements: usize,
    /// `matched` or `unmatched`.
    pub status: String,
    pub factor_ref: Option<String>,
    pub factor_name: Option<String>,
    /// A proposal by name and class, when unmatched.
    pub proposal: Option<String>,
}

fn words(s: &str) -> BTreeSet<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(str::to_owned).collect()
}

/// The factor whose name or class shares the most words with a material's name (ties: the
/// first), if any shares one.
fn propose(d: &CarbonData, material: &str) -> Option<String> {
    let mw = words(material);
    let mut best: Option<(usize, String)> = None;
    for l in &d.libraries {
        for f in &l.factors {
            let fw: BTreeSet<String> = words(&f.name).union(&words(&f.class)).cloned().collect();
            let n = mw.intersection(&fw).count();
            if n > 0 && best.as_ref().is_none_or(|b| n > b.0) {
                best = Some((n, format!("{}/{}", l.id, f.id)));
            }
        }
    }
    best.map(|b| b.1)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// `embodied_carbon.materials`: the matching grid.
pub(crate) fn materials(store: &Store, _p: Empty) -> Result<Vec<MaterialRow>> {
    let d = data(store);
    let (qs, _) = quantities(store);
    let mut rows: BTreeMap<String, (f64, BTreeSet<ElementId>)> = BTreeMap::new();
    for q in &qs {
        let r = rows.entry(q.material.clone()).or_default();
        r.0 += q.volume;
        r.1.insert(q.element);
    }
    Ok(rows
        .into_iter()
        .map(|(m, (v, els))| {
            let f = factor_for(&d, &m);
            MaterialRow {
                status: if f.is_some() { "matched".into() } else { "unmatched".into() },
                factor_ref: f.and(d.matches.get(&m).cloned()),
                factor_name: f.map(|f| f.name.clone()),
                proposal: if f.is_none() { propose(&d, &m) } else { None },
                material: m,
                volume: v,
                elements: els.len(),
            }
        })
        .collect())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutoMatchParams {
    /// Apply the proposals (otherwise they are only listed).
    #[serde(default)]
    pub apply: bool,
}

/// `embodied_carbon.auto_match`: proposals for the unmatched materials (material → factor).
pub(crate) fn auto_match(txn: &mut Transaction<'_>, p: AutoMatchParams) -> Result<BTreeMap<String, String>> {
    let store = txn.store().clone();
    let mut d = data(&store);
    let rows = materials(&store, Empty {})?;
    let props: BTreeMap<String, String> = rows.into_iter().filter_map(|r| Some((r.material, r.proposal?))).collect();
    if p.apply && !props.is_empty() {
        d.matches.extend(props.clone());
        put(txn, &d)?;
    }
    Ok(props)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordParams {
    /// The compute job it came from.
    #[serde(default)]
    pub job_id: Option<String>,
    pub results: CarbonResults,
    /// Element key → GlobalId (the distillation's).
    pub key_map: BTreeMap<String, String>,
    /// Each element's inputs at distillation (GlobalId → signature).
    pub inputs: BTreeMap<String, String>,
    /// The factors used, by material.
    pub factors: BTreeMap<String, Factor>,
    #[serde(default)]
    pub options: Options,
    /// Milliseconds since 1970.
    #[serde(default)]
    pub at: i64,
}

/// `embodied_carbon.record`: keeps a finished run (the last ten are kept).
pub(crate) fn record(txn: &mut Transaction<'_>, p: RecordParams) -> Result<Run> {
    let input = CarbonInput { lines: Vec::new(), factors: p.factors, options: p.options };
    let mut r = to_run(p.results, &p.key_map, p.inputs, &input);
    let mut d = data(txn.store());
    r.id = p.job_id.unwrap_or_else(|| format!("run{}", d.runs.iter().filter_map(|x| x.id.strip_prefix("run")?.parse::<u64>().ok()).max().unwrap_or(0) + 1));
    r.at = p.at;
    d.runs.retain(|x| x.id != r.id);
    d.runs.push(r.clone());
    if d.runs.len() > 10 {
        let n = d.runs.len() - 10;
        d.runs.drain(..n);
    }
    put(txn, &d)?;
    Ok(r)
}

/// Checks a run's options.
pub fn check_options(o: &Options) -> Result<()> {
    if !(o.study_period > 0.0 && o.study_period <= 200.0) {
        return Err(bad("carbon.options", "the study period runs from 1 to 200 years"));
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResultsParams {
    /// A run's id; the latest when omitted.
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Results {
    pub run: Run,
    /// Elements (GlobalIds) whose inputs changed since.
    pub stale_elements: Vec<String>,
    pub factors_changed: bool,
    pub stale: bool,
}

/// `embodied_carbon.results`.
pub(crate) fn results(store: &Store, p: ResultsParams) -> Result<Results> {
    let d = data(store);
    let run = match &p.run_id {
        Some(id) => d.runs.iter().find(|r| &r.id == id),
        None => d.runs.last(),
    }
    .cloned()
    .ok_or_else(|| bad("carbon.no_run", "no embodied carbon run yet"))?;
    let (stale_elements, factors_changed) = stale(store, &run);
    Ok(Results { stale: !stale_elements.is_empty() || factors_changed, run, stale_elements, factors_changed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacements_over_a_study_period() {
        assert_eq!(replacements(60.0, 25.0), 2);
        assert_eq!(replacements(50.0, 50.0), 0);
        assert_eq!(replacements(50.0, 60.0), 0);
    }

    #[test]
    fn factors_from_csv() {
        let f = parse_csv("id,name,class,unit,a1_a3,d,density,service_life\nc30,\"Concrete C30/37, generic\",concrete,kg,0.10,-0.01,2400,\nmw,Mineral wool,insulation,m3,45,,,25\n").unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].name, "Concrete C30/37, generic");
        assert_eq!((f[0].unit, f[0].stages.a1_a3, f[0].density), (Unit::Kg, 0.10, Some(2400.0)));
        assert_eq!(f[1].service_life, Some(25.0));
        assert!(parse_csv("id,name\nx,y\n").is_err());
        let e = parse_csv("id,name,unit,a1_a3\nx,X,litre,1\n").unwrap_err();
        assert!(e.message.starts_with("line 2"), "{}", e.message);
    }
}
