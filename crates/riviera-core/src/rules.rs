// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Shared rule packs (ARC-RULE-001) and use zones (ARC-RULE-002).
//!
//! A pack follows `schemas/rules/riviera.rules-1.schema.json` (MIT) and governs one **domain**
//! (interiors first; kitchens, retail, checkouts and gyms later). Its first rule kind is
//! **zones**: use zones kept clear round placed furniture and equipment, selected by IFC class,
//! predefined type or ObjectType, and checked as virtual volumes with the other clearances.
//!
//! The `rules.*` pack commands also reach the bathroom (ARC-BATH-001) and vertical-transport
//! (ARC-VT-001) packs, which keep their own formats and stores: a pack is routed by its
//! `format`, so the `bath.*` and `vt.*` pack commands see the same packs.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::bath_rules::{Confidence, DoorSwing, Layer, RuleOccupancy};
use crate::bodies::{Mesh, extrude};
use crate::clearances::{Clearance, ClearanceKind};
use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::mesh_objects::{self, Placement};
use crate::store::Store;
use crate::txn::Transaction;

/// The shared format's name.
pub const FORMAT: &str = "riviera.rules-1";
/// The project's own shared-format packs (a list), on the project.
pub const PACKS_KEY: &str = "rxd:rule_packs";
/// The packs chosen per domain (`{domain: [id]}`), on the project; a domain not named is
/// chosen automatically.
pub const CHOICE_KEY: &str = "rxd:rule_choice";
/// `false` on an element the accessible layer's rules do not apply to (ARC-RULE-002).
pub const ACCESSIBLE_KEY: &str = "rxd:accessible";
/// The published schema.
pub const SCHEMA: &str = include_str!("../../../schemas/rules/riviera.rules-1.schema.json");
const SHIPPED: &[&str] = &[
    include_str!("../../../schemas/rules/packs/ada_2010_interiors.json"),
    include_str!("../../../schemas/rules/packs/practice_interiors.json"),
];
/// A zone's height above the floor when its rule gives none (metres).
pub const DEFAULT_HEIGHT: f64 = 1.0;
/// The bathroom and vertical-transport domains, whose packs keep their own formats.
pub const BATH: &str = "bath";
pub const VT: &str = "vt";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The elements a rule applies to: each list names alternatives, and an element must match
/// every list given (compared without case).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Selector {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predefined_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_types: Vec<String>,
}

impl Selector {
    /// Whether it selects an element of `class` with this predefined type and ObjectType.
    pub fn selects(&self, class: &str, predefined: Option<&str>, object_type: Option<&str>) -> bool {
        let any = |list: &[String], v: Option<&str>| list.is_empty() || v.is_some_and(|v| list.iter().any(|x| x.eq_ignore_ascii_case(v)));
        any(&self.classes, Some(class)) && any(&self.predefined_types, predefined) && any(&self.object_types, object_type)
    }

    /// Whether it selects `e`: its own predefined type and ObjectType, else its type's (as IFC
    /// applies a type's predefined type and ElementType to its occurrences).
    pub fn selects_element(&self, store: &Store, e: &Element) -> bool {
        let ty = e.param("@type").and_then(Value::as_str).and_then(crate::id::GlobalId::parse).and_then(|g| store.find_by_guid(&g)).and_then(|t| store.get(t));
        fn text<'a>(el: Option<&'a Element>, k: &str) -> Option<&'a str> {
            el.and_then(|el| el.param(k)).and_then(Value::as_str).filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("NOTDEFINED"))
        }
        let predefined = text(Some(e), "@PredefinedType").or_else(|| text(ty, "@PredefinedType"));
        let object_type = text(Some(e), "@ObjectType").or_else(|| text(ty, "@ElementType"));
        self.selects(&e.class, predefined, object_type)
    }
}

/// Where a zone's side is across the element, as one faces its front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Across {
    Centre,
    Left,
    Right,
}

/// Where a zone's end is along the element: from its front (positive in front) or its back
/// (positive behind).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Out {
    Front,
    Back,
}

/// A use zone kept clear round an element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Zone {
    pub id: String,
    pub description: String,
    pub applies_to: Selector,
    #[serde(default)]
    pub occupancy: RuleOccupancy,
    pub source: String,
    pub confidence: Confidence,
    pub across: [(Across, f64); 2],
    pub out: [(Out, f64); 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door_swing: Option<DoorSwing>,
    /// Elements that may stand in it (a dining table's chairs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allows: Option<Selector>,
}

impl Zone {
    /// Whether `e` may stand in it.
    pub fn allows(&self, store: &Store, e: &Element) -> bool {
        self.allows.as_ref().is_some_and(|s| s.selects_element(store, e))
    }
}

/// A shared-format rule pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Pack {
    pub format: String,
    pub id: String,
    pub title: String,
    pub domain: String,
    pub jurisdiction: String,
    pub edition: String,
    pub layer: Layer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<Zone>,
}

fn validator() -> &'static jsonschema::Validator {
    static V: OnceLock<jsonschema::Validator> = OnceLock::new();
    V.get_or_init(|| {
        let schema: Value = serde_json::from_str(SCHEMA).expect("the published schema is JSON");
        jsonschema::validator_for(&schema).expect("the published schema compiles")
    })
}

/// Reads and checks a shared-format pack.
pub fn parse(v: &Value) -> Result<Pack> {
    let errors: Vec<String> = validator().iter_errors(v).take(5).map(|e| format!("{} at {}", e, e.instance_path())).collect();
    if !errors.is_empty() {
        return Err(bad("rules.pack_schema", format!("the pack does not follow {FORMAT}: {}", errors.join("; "))));
    }
    serde_json::from_value(v.clone()).map_err(|e| bad("rules.pack_schema", e.to_string()))
}

/// The shared-format packs that ship with Riviera.
pub fn shipped() -> &'static [Pack] {
    static P: OnceLock<Vec<Pack>> = OnceLock::new();
    P.get_or_init(|| SHIPPED.iter().map(|t| parse(&serde_json::from_str(t).expect("a shipped pack is JSON")).expect("a shipped pack is valid")).collect())
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

fn project_id(txn: &Transaction<'_>) -> Result<ElementId> {
    project(txn.store()).map(Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))
}

/// The project's own shared-format packs.
pub fn project_packs(store: &Store) -> Vec<Pack> {
    project(store).and_then(|e| e.param(PACKS_KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// Every shared-format pack of `domain` the project can use: the shipped ones, each replaced by
/// a project pack of its id, then the project's others.
pub fn available(store: &Store, domain: &str) -> Vec<Pack> {
    let own = project_packs(store);
    let mut out: Vec<Pack> = shipped().iter().map(|s| own.iter().find(|p| p.id == s.id).unwrap_or(s).clone()).collect();
    out.extend(own.into_iter().filter(|p| !shipped().iter().any(|s| s.id == p.id)));
    out.retain(|p| p.domain == domain);
    out
}

/// The shared-format domains the project has packs for.
pub fn domains(store: &Store) -> Vec<String> {
    let mut out: Vec<String> = shipped().iter().map(|p| p.domain.clone()).chain(project_packs(store).into_iter().map(|p| p.domain)).collect();
    out.sort();
    out.dedup();
    out
}

fn choice(store: &Store) -> BTreeMap<String, Vec<String>> {
    project(store).and_then(|e| e.param(CHOICE_KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// The packs the project names for `domain`, if it names any.
pub fn chosen(store: &Store, domain: &str) -> Option<Vec<String>> {
    choice(store).remove(domain)
}

/// The packs chosen for a domain the project names none for: for interiors, the ADA pack with
/// the ADA rule set.
pub fn automatic(store: &Store, domain: &str) -> Vec<String> {
    let mut out = Vec::new();
    if domain == "interiors" && crate::clearances::rule_set(store) == crate::clearances::RuleSet::Ada2010 {
        out.push("ada_2010_interiors".to_owned());
    }
    out
}

/// The packs of `domain` in use.
pub fn active(store: &Store, domain: &str) -> Vec<Pack> {
    let ids = chosen(store, domain).unwrap_or_else(|| automatic(store, domain));
    let all = available(store, domain);
    ids.iter().filter_map(|id| all.iter().find(|p| &p.id == id).cloned()).collect()
}

/// Every shared-format pack in use, in every domain.
pub fn all_active(store: &Store) -> Vec<Pack> {
    domains(store).iter().flat_map(|d| active(store, d)).collect()
}

// ---- Use zones (ARC-RULE-002) ----

/// A placed element as zones see it: its frame and extents (metres, in its own frame: x across,
/// its front at −y).
#[derive(Debug, Clone)]
pub struct Item {
    pub element: ElementId,
    /// Its frame without scale: extents are already scaled.
    pub frame: Placement,
    pub x: [f64; 2],
    pub front: f64,
    pub back: f64,
    /// The floor it stands on.
    pub floor: f64,
    /// Whether the accessible layer's rules apply to it.
    pub accessible: bool,
}

impl Item {
    fn across(&self, (from, d): (Across, f64)) -> f64 {
        match from {
            Across::Centre => d,
            Across::Left => self.x[0] + d,
            Across::Right => self.x[1] + d,
        }
    }

    fn out(&self, (from, d): (Out, f64)) -> f64 {
        match from {
            Out::Front => self.front - d,
            Out::Back => self.back + d,
        }
    }

    /// A zone's volume, in project coordinates.
    pub fn zone_mesh(&self, z: &Zone) -> Mesh {
        let xs = [self.across(z.across[0]), self.across(z.across[1])];
        let ys = [self.out(z.out[0]), self.out(z.out[1])];
        let (x0, x1) = (xs[0].min(xs[1]), xs[0].max(xs[1]));
        let (y0, y1) = (ys[0].min(ys[1]), ys[0].max(ys[1]));
        let world: Vec<[f64; 2]> = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
            .iter()
            .map(|[x, y]| {
                let w = self.frame.apply([*x, *y, 0.0]);
                [w[0], w[1]]
            })
            .collect();
        // Just above the floor, so the floor itself is not an intrusion.
        extrude(&world, &[], self.floor + 0.005, self.floor + z.height.unwrap_or(DEFAULT_HEIGHT))
    }
}

/// A placed custom object or component's frame and extents.
pub fn item(store: &Store, e: &Element) -> Option<Item> {
    let r = mesh_objects::mesh_object_recipe(e)?;
    let g = mesh_objects::regenerate(store, e).ok()?;
    let s = g.placement.scale;
    let (mut x, mut y) = ([f64::MAX, f64::MIN], [f64::MAX, f64::MIN]);
    for p in &g.mesh.positions {
        x = [x[0].min(f64::from(p[0]) * s), x[1].max(f64::from(p[0]) * s)];
        y = [y[0].min(f64::from(p[1]) * s), y[1].max(f64::from(p[1]) * s)];
    }
    if x[0] > x[1] {
        return None;
    }
    let floor = crate::levels::elevation_of(store, &r.level).unwrap_or(g.placement.origin[2]);
    Some(Item {
        element: e.id(),
        frame: Placement { scale: 1.0, ..g.placement },
        x,
        front: y[0],
        back: y[1],
        floor,
        accessible: e.param(ACCESSIBLE_KEY).and_then(Value::as_bool) != Some(false),
    })
}

/// A use zone and the rule it comes from.
#[derive(Debug, Clone)]
pub struct UseZone {
    pub clearance: Clearance,
    pub pack: String,
    pub domain: String,
    pub zone: Zone,
}

/// The zones the packs give one element, if it is a placed copy.
pub fn zones_of(store: &Store, e: &Element, packs: &[Pack]) -> Vec<UseZone> {
    let occupancy = crate::codes::settings(store).occupancy;
    let rules: Vec<(&Pack, &Zone)> = packs
        .iter()
        .flat_map(|p| p.zones.iter().map(move |z| (p, z)))
        .filter(|(_, z)| z.applies_to.selects_element(store, e) && z.occupancy.applies(occupancy))
        .collect();
    if rules.is_empty() {
        return Vec::new();
    }
    let Some(it) = item(store, e) else { return Vec::new() };
    rules
        .into_iter()
        .filter(|(p, _)| p.layer != Layer::Accessible || it.accessible)
        .map(|(p, z)| UseZone {
            clearance: Clearance { element: it.element, kind: ClearanceKind::UseZone, rule: z.id.clone(), mesh: it.zone_mesh(z), door_swing_forbidden: z.door_swing == Some(DoorSwing::Forbidden) },
            pack: p.id.clone(),
            domain: p.domain.clone(),
            zone: z.clone(),
        })
        .collect()
}

/// Every use zone under the packs in use.
pub fn use_zones(store: &Store) -> Vec<UseZone> {
    let packs = all_active(store);
    if packs.iter().all(|p| p.zones.is_empty()) {
        return Vec::new();
    }
    store.iter().flat_map(|e| zones_of(store, e, &packs)).collect()
}

/// The use zones as virtual volumes (`clearances::all`).
pub fn zones(store: &Store) -> Vec<Clearance> {
    use_zones(store).into_iter().map(|z| z.clearance).collect()
}

/// Whether the use zone `rule` of `owner` lets `other` stand in it (its rule's `allows`).
pub fn zone_allows(store: &Store, owner: ElementId, rule: &str, other: ElementId) -> bool {
    let (Some(o), Some(e)) = (store.get(owner), store.get(other)) else { return false };
    all_active(store).iter().flat_map(|p| &p.zones).any(|z| z.id == rule && z.applies_to.selects_element(store, o) && z.allows(store, e))
}

/// What may stand in a use zone besides the element itself: floors and floor coverings (a rug),
/// accessories, and other zones.
pub fn may_share(store: &Store, other: ElementId) -> bool {
    store.get(other).is_some_and(|e| match e.class.as_str() {
        "IfcSlab" | "IfcDiscreteAccessory" | "IfcSpace" | "IfcSpatialZone" => true,
        "IfcCovering" => e.param("@PredefinedType").and_then(Value::as_str).is_none_or(|t| t.eq_ignore_ascii_case("FLOORING")),
        _ => false,
    })
}

// ---- Commands ----

/// A pack of any domain, for listing (`rules.packs`).
#[derive(Debug, Serialize, JsonSchema)]
pub struct PackSummary {
    pub domain: String,
    pub id: String,
    pub title: String,
    pub format: String,
    pub layer: Value,
    pub jurisdiction: String,
    pub edition: String,
    /// How many rules it holds.
    pub rules: usize,
    pub in_use: bool,
    /// It ships with Riviera (and is not replaced by the project's own).
    pub shipped: bool,
}

/// How many rules a pack of any format holds: every list item and every rule block.
fn rule_count(pack: &Value) -> usize {
    const ENVELOPE: &[&str] = &["format", "id", "title", "domain", "jurisdiction", "edition", "layer", "notes"];
    pack.as_object().map_or(0, |o| {
        o.iter()
            .filter(|(k, _)| !ENVELOPE.contains(&k.as_str()))
            .map(|(_, v)| match v {
                Value::Array(a) => a.len(),
                Value::Null => 0,
                _ => 1,
            })
            .sum()
    })
}

fn summary(domain: &str, pack: &impl Serialize, in_use: bool, shipped: bool) -> PackSummary {
    let v = serde_json::to_value(pack).expect("serialises");
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    PackSummary {
        domain: domain.to_owned(),
        id: text("id"),
        title: text("title"),
        format: text("format"),
        layer: v.get("layer").cloned().unwrap_or(Value::Null),
        jurisdiction: text("jurisdiction"),
        edition: text("edition"),
        rules: rule_count(&v),
        in_use,
        shipped,
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PacksParams {
    /// Only this domain's packs (`bath`, `vt`, `interiors`, …).
    pub domain: Option<String>,
}

/// The packs of every domain, or of one (`rules.packs`).
pub fn packs(store: &Store, p: Option<PacksParams>) -> Result<Vec<PackSummary>> {
    let only = p.and_then(|p| p.domain);
    let wanted = |d: &str| only.as_deref().is_none_or(|o| o == d);
    let mut out = Vec::new();
    if wanted(BATH) {
        out.extend(crate::bath_rules::rule_packs(store).into_iter().map(|i| summary(BATH, &i.pack, i.in_use, i.shipped)));
    }
    if wanted(VT) {
        let list = crate::elevators::list_rule_packs(store, None)?;
        out.extend(list.into_iter().map(|i| summary(VT, &i.pack, i.in_use, i.shipped)));
    }
    let own = project_packs(store);
    let mut domains = domains(store);
    if let Some(o) = &only
        && !matches!(o.as_str(), BATH | VT)
        && !domains.contains(o)
    {
        domains.push(o.clone());
    }
    for d in domains.iter().filter(|d| wanted(d)) {
        let in_use: Vec<String> = active(store, d).into_iter().map(|p| p.id).collect();
        out.extend(available(store, d).iter().map(|p| summary(d, p, in_use.contains(&p.id), !own.iter().any(|o| o.id == p.id))));
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPacksParams {
    pub domain: String,
    /// The packs to use, by id; none to choose them automatically.
    pub packs: Option<Vec<String>>,
}

/// Chooses a domain's packs (`rules.set_packs`).
pub(crate) fn set_packs(txn: &mut Transaction<'_>, p: SetPacksParams) -> Result<()> {
    match p.domain.as_str() {
        BATH => return crate::bath_rules::set_rule_packs(txn, crate::bath_rules::SetPacksParams { packs: p.packs }),
        VT => return crate::elevators::set_rule_packs(txn, crate::elevators::SetPacksParams { packs: p.packs }),
        _ => {}
    }
    let all = available(txn.store(), &p.domain);
    if let Some(ids) = &p.packs {
        for id in ids {
            if !all.iter().any(|a| &a.id == id) {
                let known: Vec<&str> = all.iter().map(|a| a.id.as_str()).collect();
                return Err(bad("rules.no_pack", format!("no {} rule pack {id:?}; there are: {}", p.domain, known.join(", "))));
            }
        }
    }
    let mut c = choice(txn.store());
    match p.packs {
        Some(ids) => {
            c.insert(p.domain, ids);
        }
        None => {
            c.remove(&p.domain);
        }
    }
    let project = project_id(txn)?;
    txn.modify(project, |e| {
        if c.is_empty() {
            e.params.remove(CHOICE_KEY);
        } else {
            e.params.insert(CHOICE_KEY.into(), serde_json::to_value(&c).expect("serialises"));
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddPackParams {
    /// A pack following `riviera.rules-1`, `riviera.bath-rules-1` or `riviera.vt-rules-1`.
    pub pack: Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Added {
    pub domain: String,
    pub id: String,
}

/// Adds a pack of any format to the project, or replaces its pack (or a shipped one) of that id
/// (`rules.add_pack`).
pub(crate) fn add_pack(txn: &mut Transaction<'_>, p: AddPackParams) -> Result<Added> {
    let format = p.pack.get("format").and_then(Value::as_str).unwrap_or_default().to_owned();
    match format.as_str() {
        "riviera.bath-rules-1" | "reviera.bath-rules-1" => {
            let id = crate::bath_rules::add_rule_pack(txn, crate::bath_rules::AddPackParams { pack: p.pack })?;
            return Ok(Added { domain: BATH.into(), id });
        }
        "riviera.vt-rules-1" | "reviera.vt-rules-1" => {
            let id = crate::elevators::add_rule_pack(txn, crate::elevators::AddPackParams { pack: p.pack })?;
            return Ok(Added { domain: VT.into(), id });
        }
        FORMAT => {}
        other => return Err(bad("rules.pack_schema", format!("unknown pack format {other:?}: expected {FORMAT}, riviera.bath-rules-1 or riviera.vt-rules-1"))),
    }
    let pack = parse(&p.pack)?;
    let mut own = project_packs(txn.store());
    own.retain(|o| o.id != pack.id);
    let added = Added { domain: pack.domain.clone(), id: pack.id.clone() };
    own.push(pack);
    let project = project_id(txn)?;
    let value = serde_json::to_value(&own).expect("serialises");
    txn.modify(project, |e| {
        e.params.insert(PACKS_KEY.into(), value);
    })?;
    Ok(added)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemovePackParams {
    pub domain: String,
    pub id: String,
}

/// Removes one of the project's own packs (`rules.remove_pack`); a shipped pack it replaced comes
/// back.
pub(crate) fn remove_pack(txn: &mut Transaction<'_>, p: RemovePackParams) -> Result<()> {
    match p.domain.as_str() {
        BATH => return crate::bath_rules::remove_rule_pack(txn, crate::bath_rules::RemovePackParams { id: p.id }),
        VT => return crate::elevators::remove_rule_pack(txn, crate::elevators::RemovePackParams { id: p.id }),
        _ => {}
    }
    let mut own = project_packs(txn.store());
    let before = own.len();
    own.retain(|o| !(o.id == p.id && o.domain == p.domain));
    if own.len() == before {
        return Err(bad("rules.no_pack", format!("the project has no {} pack {:?} of its own", p.domain, p.id)));
    }
    let project = project_id(txn)?;
    txn.modify(project, |e| {
        if own.is_empty() {
            e.params.remove(PACKS_KEY);
        } else {
            e.params.insert(PACKS_KEY.into(), serde_json::to_value(&own).expect("serialises"));
        }
    })
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchemaParams {}

/// The published shared-format schema (`rules.schema`).
pub fn schema(_: &Store, _: Option<SchemaParams>) -> Result<Value> {
    Ok(serde_json::from_str(SCHEMA).expect("the published schema is JSON"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetAccessibleParams {
    pub element_ids: Vec<ElementRef>,
    /// Whether the accessible layer's rules apply to them.
    pub accessible: bool,
}

/// Marks elements as subject to the accessible layer's rules or not (`rules.set_accessible`).
pub(crate) fn set_accessible(txn: &mut Transaction<'_>, p: SetAccessibleParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", format!("no element {r:?}")))?;
        txn.modify(id, |e| {
            if p.accessible {
                e.params.remove(ACCESSIBLE_KEY);
            } else {
                e.params.insert(ACCESSIBLE_KEY.into(), Value::Bool(false));
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_packs_follow_the_schema_and_name_their_sources() {
        let register = include_str!("../../../design_documents/SOURCE_REGISTER.md");
        assert!(shipped().len() >= 2);
        for p in shipped() {
            assert_eq!(p.format, FORMAT);
            for z in &p.zones {
                for s in z.source.split(' ') {
                    assert!(register.contains(&format!("| {s} |")), "{}: {s} is not in SOURCE_REGISTER.md", z.id);
                }
            }
        }
    }

    #[test]
    fn a_selector_needs_every_list_it_names() {
        let s = Selector { classes: vec!["IfcFurniture".into()], predefined_types: vec!["TABLE".into()], object_types: vec!["DiningTable".into()] };
        assert!(s.selects("IfcFurniture", Some("table"), Some("diningtable")));
        assert!(!s.selects("IfcFurniture", Some("TABLE"), None));
        assert!(!s.selects("IfcFurniture", Some("DESK"), Some("DiningTable")));
        assert!(Selector { classes: vec!["IfcFurniture".into()], ..Selector::default() }.selects("IfcFurniture", None, None));
    }
}
