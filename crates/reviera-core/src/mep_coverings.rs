// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Insulation and duct lining (MEP-DUCT-004): coverings on ducts, pipes, their fittings and
//! accessories.
//!
//! - **Types**: ordinary types (`IfcCoveringType` INSULATION) with a material and, for lining, an
//!   air-side roughness; the built-in *Duct Insulation* and *Duct Lining* are made on first use.
//! - **Occurrences**: an `IfcCovering` (INSULATION) per host and kind, holding the host
//!   (GlobalId) and the thickness. Its body is found afresh from the host, so it follows the
//!   host's size and position; it goes with the host when the host is deleted, and a split
//!   host's new part gets a copy. In IFC it covers its host (`IfcRelCoversBldgElements`), with
//!   its thickness as `Qto_CoveringBaseQuantities.Width`, written when the model is saved.
//! - **Lining** sits inside a duct: the pressure-drop calculation (MEP-CALC-002) uses the duct's
//!   free section (each side less twice the thickness) and the lining's roughness. Insulation
//!   leaves the calculation as it is.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::Domain;
use crate::mep_runs::{self, Size, Solid};
use crate::store::Store;
use crate::txn::{Transaction, Warning};
use crate::walls::RECIPE_KEY;

pub const TYPE_RECIPE: &str = "recipe.mep_covering_type";
pub const COVERING: &str = "recipe.mep_covering";
/// Default thicknesses (metres): insulation 25 mm, lining 25 mm.
pub const DEFAULT_THICKNESS: f64 = 0.025;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// Outside insulation, or lining inside a duct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoveringKind {
    Insulation,
    Lining,
}

impl CoveringKind {
    pub fn label(self) -> &'static str {
        match self {
            CoveringKind::Insulation => "Insulation",
            CoveringKind::Lining => "Duct Lining",
        }
    }

    fn command(self) -> &'static str {
        match self {
            CoveringKind::Insulation => "mep_insulation",
            CoveringKind::Lining => "duct_lining",
        }
    }
}

/// An insulation or lining type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CoveringType {
    pub kind: String,
    pub covering: CoveringKind,
    pub material: String,
    /// Lining: the air side's absolute roughness (metres).
    #[serde(default)]
    pub roughness: f64,
}

/// An insulation or lining occurrence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CoveringRecipe {
    pub kind: String,
    pub covering: CoveringKind,
    /// What it covers (GlobalId).
    pub host: String,
    pub thickness: f64,
}

pub fn covering_type(e: &Element) -> Option<CoveringType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == TYPE_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn covering_recipe(e: &Element) -> Option<CoveringRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == COVERING).then(|| serde_json::from_value(v.clone()).ok())?
}

/// The coverings of a host.
pub fn of_host(store: &Store, host: ElementId) -> Vec<ElementId> {
    let Some(g) = store.get(host).map(|e| e.guid().to_string()) else { return Vec::new() };
    store.iter().filter(|e| covering_recipe(e).is_some_and(|r| r.host == g)).map(|e| e.id()).collect()
}

fn find(store: &Store, host: ElementId, kind: CoveringKind) -> Option<ElementId> {
    of_host(store, host).into_iter().find(|c| store.get(*c).and_then(covering_recipe).is_some_and(|r| r.covering == kind))
}

fn host_of(store: &Store, r: &CoveringRecipe) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(&r.host)?)
}

/// A covering's host domain and body, found from its host as the model is now.
pub fn solids(store: &Store, e: &Element) -> Option<(Domain, Vec<Solid>)> {
    let r = covering_recipe(e)?;
    let h = store.get(host_of(store, &r)?)?;
    let domain = mep_runs::segment_recipe(h).map(|s| s.domain).or_else(|| mep_runs::fitting_recipe(h).map(|f| f.domain))?;
    let t = 2.0 * r.thickness;
    let solids = match r.covering {
        CoveringKind::Insulation => mep_runs::covering_solids(h, 0.0, t),
        CoveringKind::Lining => mep_runs::covering_solids(h, -t, 0.0),
    };
    Some((domain, solids))
}

/// The level a covering is drawn on: its host's.
pub fn level(store: &Store, e: &Element) -> Option<String> {
    let h = store.get(host_of(store, &covering_recipe(e)?)?)?;
    mep_runs::segment_recipe(h).map(|s| s.level).or_else(|| mep_runs::fitting_recipe(h).map(|f| f.level))
}

/// The section and roughness flow sees in a duct of `size`: less its lining, if it has one.
pub fn free_size(store: &Store, host: ElementId, size: Size) -> (Size, Option<f64>) {
    let Some(c) = find(store, host, CoveringKind::Lining) else { return (size, None) };
    let e = store.get(c).expect("listed");
    let t = 2.0 * covering_recipe(e).expect("a covering").thickness;
    let roughness = e
        .param("@type")
        .and_then(|v| v.as_str())
        .and_then(GlobalId::parse)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|t| store.get(t))
        .and_then(covering_type)
        .map(|t| t.roughness);
    (Size { diameter: (size.diameter - t).max(0.0), width: (size.width - t).max(0.0), height: (size.height - t).max(0.0), ..size }, roughness)
}

fn builtin(kind: CoveringKind) -> (&'static str, CoveringType) {
    match kind {
        CoveringKind::Insulation => ("Duct Insulation", CoveringType { kind: TYPE_RECIPE.into(), covering: kind, material: "Mineral Wool".into(), roughness: 0.0 }),
        // Fibrous glass duct liner, air side faced (E-29).
        CoveringKind::Lining => ("Duct Lining", CoveringType { kind: TYPE_RECIPE.into(), covering: kind, material: "Glass Fibre Liner".into(), roughness: 0.0015 }),
    }
}

fn make_type(txn: &mut Transaction<'_>, name: &str, t: &CoveringType) -> Result<ElementId> {
    txn.create(NewElement::new("IfcCoveringType").name(name).param("@PredefinedType", "INSULATION").param(RECIPE_KEY, serde_json::to_value(t).expect("serialises")))
}

/// The type to use: the one given (of the right kind), or the built-in one.
fn resolve_type(txn: &mut Transaction<'_>, kind: CoveringKind, r: Option<&ElementRef>) -> Result<ElementId> {
    let store = txn.store().clone();
    if let Some(r) = r {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such type"))?;
        let t = store.get(id).and_then(covering_type).ok_or_else(|| bad("mep_covering.type", format!("element {id} is not an insulation or lining type")))?;
        if t.covering != kind {
            return Err(bad("mep_covering.type", format!("that is {} type, not {}", t.covering.label().to_lowercase(), kind.label().to_lowercase())));
        }
        return Ok(id);
    }
    let (name, t) = builtin(kind);
    if let Some(e) = store.iter().find(|e| e.name.as_deref() == Some(name) && covering_type(e).is_some_and(|x| x.covering == kind)) {
        return Ok(e.id());
    }
    make_type(txn, name, &t)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTypeParams {
    pub name: String,
    pub covering: CoveringKind,
    #[serde(default)]
    pub material: Option<String>,
    /// Lining: the air side's absolute roughness (metres).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub roughness: Option<f64>,
}

/// `mep_covering_type.create`.
pub(crate) fn create_type(txn: &mut Transaction<'_>, p: CreateTypeParams) -> Result<ElementId> {
    let name = p.name.trim();
    if name.is_empty() {
        return Err(bad("mep_covering.name", "give the type a name"));
    }
    let (_, base) = builtin(p.covering);
    let roughness = p.roughness.unwrap_or(base.roughness);
    if !(roughness.is_finite() && roughness >= 0.0) {
        return Err(bad("mep_covering.roughness", "the roughness cannot be negative"));
    }
    let t = CoveringType { material: p.material.unwrap_or(base.material), roughness, ..base };
    make_type(txn, name, &t)
}

/// Checks a host for a kind of covering: the host's smallest side, or why it cannot take one.
fn check_host(store: &Store, id: ElementId, kind: CoveringKind) -> std::result::Result<f64, String> {
    let e = store.get(id).ok_or_else(|| format!("element {id} does not exist"))?;
    let (domain, size, accessory) = if let Some(s) = mep_runs::segment_recipe(e) {
        if s.placeholder {
            return Err("a placeholder has no body to cover".into());
        }
        (s.domain, s.size, false)
    } else if let Some(f) = mep_runs::fitting_recipe(e) {
        (f.domain, f.legs.first().map(|l| l.size).ok_or("it has no legs")?, f.part.is_accessory())
    } else {
        return Err(format!("{} is not a duct, pipe, fitting or accessory", e.name.as_deref().unwrap_or(&e.class)));
    };
    let smallest = match size.shape {
        crate::mep::Shape::Round => size.diameter,
        _ => size.width.min(size.height),
    };
    match (kind, domain) {
        (CoveringKind::Insulation, Domain::Duct | Domain::Pipe) => Ok(smallest),
        (CoveringKind::Lining, Domain::Duct) if !accessory => Ok(smallest),
        (CoveringKind::Lining, Domain::Duct) => Err(format!("element {id} is an accessory; lining goes in ducts and duct fittings")),
        (CoveringKind::Insulation, _) => Err(format!("element {id} is not a duct or pipe")),
        (CoveringKind::Lining, _) => Err(format!("element {id} is not a duct")),
    }
}

fn check_thickness(kind: CoveringKind, t: f64, smallest: f64) -> Result<()> {
    if !(t.is_finite() && t > 0.0) {
        return Err(bad("mep_covering.thickness", "the thickness must be greater than 0"));
    }
    if kind == CoveringKind::Lining && t >= smallest / 2.0 - 1e-9 {
        return Err(bad("mep_covering.thickness", format!("lining must be thinner than half the duct's smallest side ({:.0} mm)", smallest * 500.0)));
    }
    Ok(())
}

fn write(txn: &mut Transaction<'_>, id: ElementId, r: &CoveringRecipe, type_guid: &str) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    let tg = type_guid.to_owned();
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
        e.params.insert("@type".into(), tg.into());
    })
}

fn create(txn: &mut Transaction<'_>, r: &CoveringRecipe, type_guid: &str) -> Result<ElementId> {
    let id = txn.create(NewElement::new("IfcCovering").name(r.covering.label()).param("@PredefinedType", "INSULATION"))?;
    write(txn, id, r, type_guid)?;
    Ok(id)
}

/// Applies `f` to each element that can take the kind, warning about the rest; refused when none
/// can.
fn each_host(txn: &mut Transaction<'_>, kind: CoveringKind, ids: &[ElementRef], mut f: impl FnMut(&mut Transaction<'_>, ElementId, f64) -> Result<Option<ElementId>>) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let mut done = Vec::new();
    let (mut skipped, mut why) = (Vec::new(), Vec::new());
    for r in ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
        match check_host(&store, id, kind) {
            Ok(smallest) => done.extend(f(txn, id, smallest)?),
            Err(m) => {
                skipped.push(id);
                why.push(m);
            }
        }
    }
    if done.is_empty() && !skipped.is_empty() {
        return Err(bad("mep_covering.host", why.join("; ")));
    }
    if !skipped.is_empty() {
        txn.warn(Warning { id: format!("{}.skipped", kind.command()), message: format!("Skipped: {}", why.join("; ")), element_ids: skipped });
    }
    Ok(done)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddParams {
    /// Ducts, pipes, fittings and accessories (lining: ducts and duct fittings).
    pub element_ids: Vec<ElementRef>,
    /// The insulation or lining type; the built-in one when omitted.
    pub type_id: Option<ElementRef>,
    /// Metres; 25 mm when omitted.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub thickness: Option<f64>,
}

/// `mep_insulation.add` / `duct_lining.add`: a covering on each element (one already there is
/// changed to this type and thickness). Returns the coverings.
pub(crate) fn add(txn: &mut Transaction<'_>, kind: CoveringKind, p: AddParams) -> Result<Vec<ElementId>> {
    let ty = resolve_type(txn, kind, p.type_id.as_ref())?;
    let tg = txn.store().get(ty).expect("made").guid().to_string();
    let t = p.thickness.unwrap_or(DEFAULT_THICKNESS);
    each_host(txn, kind, &p.element_ids, |txn, id, smallest| {
        check_thickness(kind, t, smallest)?;
        let store = txn.store().clone();
        let r = CoveringRecipe { kind: COVERING.into(), covering: kind, host: store.get(id).expect("checked").guid().to_string(), thickness: t };
        match find(&store, id, kind) {
            Some(c) => write(txn, c, &r, &tg).map(|_| Some(c)),
            None => create(txn, &r, &tg).map(Some),
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    /// The covered elements (or the coverings themselves).
    pub element_ids: Vec<ElementRef>,
    pub type_id: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub thickness: Option<f64>,
}

/// A covering given itself: its host.
fn host_ref(store: &Store, r: &ElementRef) -> ElementRef {
    store.resolve(r).and_then(|id| store.get(id)).and_then(covering_recipe).and_then(|c| host_of(store, &c)).map_or_else(|| r.clone(), ElementRef::Id)
}

/// `mep_insulation.set` / `duct_lining.set` (*Edit*): a new type or thickness on the elements'
/// coverings; elements without one are skipped.
pub(crate) fn set(txn: &mut Transaction<'_>, kind: CoveringKind, p: SetParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let ty = p.type_id.as_ref().map(|r| resolve_type(txn, kind, Some(r))).transpose()?;
    let tg = ty.map(|t| txn.store().get(t).expect("resolved").guid().to_string());
    let hosts: Vec<ElementRef> = p.element_ids.iter().map(|r| host_ref(&store, r)).collect();
    each_host(txn, kind, &hosts, |txn, id, smallest| {
        let store = txn.store().clone();
        let Some(c) = find(&store, id, kind) else { return Ok(None) };
        let e = store.get(c).expect("found");
        let mut r = covering_recipe(e).expect("a covering");
        if let Some(t) = p.thickness {
            check_thickness(kind, t, smallest)?;
            r.thickness = t;
        }
        let g = tg.clone().or_else(|| e.param("@type").and_then(|v| v.as_str()).map(str::to_owned)).unwrap_or_default();
        write(txn, c, &r, &g)?;
        Ok(Some(c))
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveParams {
    /// The covered elements (or the coverings themselves).
    pub element_ids: Vec<ElementRef>,
}

/// `mep_insulation.remove` / `duct_lining.remove`: deletes the elements' coverings of the kind.
/// Returns the deleted coverings.
pub(crate) fn remove(txn: &mut Transaction<'_>, kind: CoveringKind, p: RemoveParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let mut gone = Vec::new();
    for r in &p.element_ids {
        let id = store.resolve(&host_ref(&store, r)).ok_or_else(|| bad("element.not_found", "no such element"))?;
        if let Some(c) = find(&store, id, kind) {
            txn.delete(c)?;
            gone.push(c);
        }
    }
    Ok(gone)
}

/// A split host's new parts get copies of its coverings (insulation only on an accessory).
pub(crate) fn follow_split(txn: &mut Transaction<'_>, host: ElementId, new: &[ElementId]) -> Result<()> {
    let store = txn.store().clone();
    for c in of_host(&store, host) {
        let e = store.get(c).expect("listed");
        let r = covering_recipe(e).expect("a covering");
        let tg = e.param("@type").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
        for &n in new {
            if check_host(&store, n, r.covering).is_err() || find(&store, n, r.covering).is_some() {
                continue;
            }
            let g = store.get(n).expect("made").guid().to_string();
            create(txn, &CoveringRecipe { host: g, ..r.clone() }, &tg)?;
        }
    }
    Ok(())
}

/// One covering as listed.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CoveringInfo {
    pub id: ElementId,
    pub covering: CoveringKind,
    pub host: ElementId,
    pub type_name: Option<String>,
    pub thickness: f64,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// Only these hosts' coverings.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
}

/// `mep_covering.list`.
pub(crate) fn list(store: &Store, p: ListParams) -> Result<Vec<CoveringInfo>> {
    let only: Vec<ElementId> = p.element_ids.iter().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))).collect::<Result<_>>()?;
    Ok(store
        .iter()
        .filter_map(|e| {
            let r = covering_recipe(e)?;
            let host = host_of(store, &r)?;
            if !only.is_empty() && !only.contains(&host) {
                return None;
            }
            let type_name = e.param("@type").and_then(|v| v.as_str()).and_then(GlobalId::parse).and_then(|g| store.find_by_guid(&g)).and_then(|t| store.get(t)).and_then(|t| t.name.clone());
            Some(CoveringInfo { id: e.id(), covering: r.covering, host, type_name, thickness: r.thickness })
        })
        .collect())
}

pub(crate) fn add_insulation(txn: &mut Transaction<'_>, p: AddParams) -> Result<Vec<ElementId>> {
    add(txn, CoveringKind::Insulation, p)
}
pub(crate) fn set_insulation(txn: &mut Transaction<'_>, p: SetParams) -> Result<Vec<ElementId>> {
    set(txn, CoveringKind::Insulation, p)
}
pub(crate) fn remove_insulation(txn: &mut Transaction<'_>, p: RemoveParams) -> Result<Vec<ElementId>> {
    remove(txn, CoveringKind::Insulation, p)
}
pub(crate) fn add_lining(txn: &mut Transaction<'_>, p: AddParams) -> Result<Vec<ElementId>> {
    add(txn, CoveringKind::Lining, p)
}
pub(crate) fn set_lining(txn: &mut Transaction<'_>, p: SetParams) -> Result<Vec<ElementId>> {
    set(txn, CoveringKind::Lining, p)
}
pub(crate) fn remove_lining(txn: &mut Transaction<'_>, p: RemoveParams) -> Result<Vec<ElementId>> {
    remove(txn, CoveringKind::Lining, p)
}
