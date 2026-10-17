// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The structural analytical model (STR-AM-001, 003, 004, 006): analytical members and nodes,
//! their association with physical elements, and automation between the two models.
//!
//! Reviera's analytical model is independent: its elements are stored elements in their own right,
//! associated with physical elements when wanted, and updated from them only when asked
//! (automation). An **analytical node** (`IfcStructuralPointConnection`, `recipe.analytical_node`)
//! sits at a plan point at its level plus an offset, so it follows the level. An **analytical
//! member** (`IfcStructuralCurveMember`, `recipe.analytical_member`) runs between two nodes, with
//! its structural role, section type, material, end releases (six degrees of freedom per end,
//! fixed or released), *Analyze As* and the physical elements it is associated with. Every
//! analytical element belongs to the model's `IfcStructuralAnalysisModel` ("Analytical Model",
//! made on first use).

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const NODE_RECIPE: &str = "recipe.analytical_node";
pub const MEMBER_RECIPE: &str = "recipe.analytical_member";
pub const MODEL_RECIPE: &str = "recipe.analytical_model";
pub const MODEL_NAME: &str = "Analytical Model";

type V3 = [f64; 3];

/// A member's structural role.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Beam,
    Column,
    Brace,
    Member,
    Tie,
}

/// How a member is analysed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalyzeAs {
    /// By its end releases (rigid or pin-joined).
    #[default]
    Releases,
    TensionOnly,
    CompressionOnly,
}

/// An end release preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleasePreset {
    /// Every translation and rotation fixed.
    Fixed,
    /// Translations fixed, rotations released.
    Pinned,
}

impl ReleasePreset {
    /// The six fixity flags (translations x, y, z, then rotations x, y, z; true is fixed).
    pub fn flags(self) -> [bool; 6] {
        match self {
            ReleasePreset::Fixed => [true; 6],
            ReleasePreset::Pinned => [true, true, true, false, false, false],
        }
    }
}

/// The stored recipe of a node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodeRecipe {
    pub kind: String,
    pub point: [f64; 2],
    /// Its level (GlobalId) and height above it.
    pub level: String,
    #[serde(default)]
    pub offset: f64,
    /// A point boundary condition (STR-BC-001).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support: Option<crate::loads::Support>,
}

fn fixed() -> [[bool; 6]; 2] {
    [[true; 6]; 2]
}

/// The stored recipe of a member.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemberRecipe {
    pub kind: String,
    /// Its end nodes (GlobalIds).
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub role: Role,
    /// A column or beam type (GlobalId).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Each end's six fixity flags (translations x, y, z, rotations x, y, z; true is fixed).
    #[serde(default = "fixed")]
    pub releases: [[bool; 6]; 2],
    #[serde(default)]
    pub analyze_as: AnalyzeAs,
    /// Cross-section rotation (radians).
    #[serde(default)]
    pub rotation: f64,
    /// The physical elements it is associated with (GlobalIds).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub physical: Vec<String>,
}

fn of_kind<T: for<'de> Deserialize<'de>>(e: &Element, kind: &str) -> Option<T> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == kind).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn node_recipe(e: &Element) -> Option<NodeRecipe> {
    of_kind(e, NODE_RECIPE)
}
pub fn member_recipe(e: &Element) -> Option<MemberRecipe> {
    of_kind(e, MEMBER_RECIPE)
}
pub fn is_model(e: &Element) -> bool {
    e.param(RECIPE_KEY)
        .and_then(|v| v.get("kind"))
        .and_then(Value::as_str)
        == Some(MODEL_RECIPE)
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// A node's point (metres).
pub fn node_point(store: &Store, e: &Element) -> Option<V3> {
    let r = node_recipe(e)?;
    let z = crate::levels::elevation_of(store, &r.level).ok()? + r.offset;
    Some([r.point[0], r.point[1], z])
}

/// A member as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberGeometry {
    pub start: V3,
    pub end: V3,
    pub nodes: [ElementId; 2],
    pub recipe: MemberRecipe,
}

/// Every member whose nodes exist.
pub fn members(store: &Store) -> Vec<(ElementId, MemberGeometry)> {
    store
        .iter()
        .filter_map(|e| {
            let r = member_recipe(e)?;
            let a = find(store, &r.start)?;
            let b = find(store, &r.end)?;
            Some((
                e.id(),
                MemberGeometry {
                    start: node_point(store, store.get(a)?)?,
                    end: node_point(store, store.get(b)?)?,
                    nodes: [a, b],
                    recipe: r,
                },
            ))
        })
        .collect()
}

/// Every node and its point.
pub fn nodes(store: &Store) -> Vec<(ElementId, V3)> {
    store
        .iter()
        .filter_map(|e| Some((e.id(), node_point(store, e)?)))
        .collect()
}

/// Whether an element depends on `guid`: a member on its nodes.
pub fn uses(e: &Element, guid: &str) -> bool {
    member_recipe(e).is_some_and(|r| r.start == guid || r.end == guid)
        || node_recipe(e).is_some_and(|r| r.level == guid)
}

fn dist(a: V3, b: V3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The model's `IfcStructuralAnalysisModel`, made on first use.
pub(crate) fn ensure_model(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn.store().iter().find(|e| is_model(e)) {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new("IfcStructuralAnalysisModel")
            .name(MODEL_NAME)
            .param("@PredefinedType", "LOADING_3D")
            .param(RECIPE_KEY, serde_json::json!({ "kind": MODEL_RECIPE })),
    )
}

/// The node at `p` (within `tolerance`), or a new one there, on the highest level at or below it.
fn node_at(txn: &mut crate::txn::Transaction<'_>, p: V3, tolerance: f64) -> Result<String> {
    let store = txn.store().clone();
    if let Some((id, _)) = nodes(&store)
        .into_iter()
        .filter(|(_, q)| dist(*q, p) <= tolerance)
        .min_by(|a, b| dist(a.1, p).total_cmp(&dist(b.1, p)))
    {
        return Ok(store.get(id).expect("listed").guid().to_string());
    }
    let levels = crate::levels::list(&store);
    let level = levels
        .iter()
        .filter(|l| l.elevation <= p[2] + 1e-6)
        .max_by(|a, b| a.elevation.total_cmp(&b.elevation))
        .or_else(|| {
            levels
                .iter()
                .min_by(|a, b| a.elevation.total_cmp(&b.elevation))
        })
        .ok_or_else(|| Error::new("level.missing", "the model has no levels"))?;
    ensure_model(txn)?;
    let r = NodeRecipe {
        kind: NODE_RECIPE.into(),
        point: [p[0], p[1]],
        level: level.guid.clone(),
        offset: p[2] - level.elevation,
        support: None,
    };
    let id = txn.create(
        NewElement::new("IfcStructuralPointConnection")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    Ok(txn.store().get(id).expect("created").guid().to_string())
}

fn tolerance(store: &Store) -> f64 {
    crate::structural::settings(store).node_merge_distance
}

fn new_member(
    txn: &mut crate::txn::Transaction<'_>,
    a: V3,
    b: V3,
    r: MemberRecipe,
) -> Result<ElementId> {
    if dist(a, b) < 1e-3 {
        return Err(Error::new(
            "analytical.too_short",
            "a member's ends are in the same place",
        ));
    }
    let tol = tolerance(txn.store());
    let start = node_at(txn, a, tol)?;
    let end = node_at(txn, b, tol)?;
    if start == end {
        return Err(Error::new(
            "analytical.too_short",
            "a member's ends merge into one node",
        ));
    }
    let r = MemberRecipe { start, end, ..r };
    txn.create(
        NewElement::new("IfcStructuralCurveMember")
            .param("@PredefinedType", predefined(&r))
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

/// A member's PredefinedType: pin-joined when both ends release every rotation, tension or
/// compression members by *Analyze As*, otherwise rigid-joined.
pub fn predefined(r: &MemberRecipe) -> &'static str {
    match r.analyze_as {
        AnalyzeAs::TensionOnly => "TENSION_MEMBER",
        AnalyzeAs::CompressionOnly => "COMPRESSION_MEMBER",
        AnalyzeAs::Releases if r.releases.iter().all(|e| !e[3] && !e[4] && !e[5]) => {
            "PIN_JOINED_MEMBER"
        }
        AnalyzeAs::Releases => "RIGID_JOINED_MEMBER",
    }
}

fn type_guid(store: &Store, r: &Option<ElementRef>) -> Result<Option<String>> {
    r.as_ref()
        .map(|r| {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let e = store.get(id).expect("resolved");
            crate::columns::section_type(e).ok_or_else(|| {
                Error::new(
                    "analytical.not_a_section",
                    format!("element {id} is not a column or beam type"),
                )
            })?;
            Ok(e.guid().to_string())
        })
        .transpose()
}

fn write_member(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    r: &MemberRecipe,
) -> Result<()> {
    let v = serde_json::to_value(r).expect("serialises");
    let pre = predefined(r);
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
        e.params.insert("@PredefinedType".into(), pre.into());
    })
}

fn resolve_member(store: &Store, r: &ElementRef) -> Result<(ElementId, MemberRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
    let m = store.get(id).and_then(member_recipe).ok_or_else(|| {
        Error::new(
            "analytical.not_a_member",
            format!("element {id} is not an analytical member"),
        )
    })?;
    Ok((id, m))
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// Its ends (metres); nodes are made there, or existing ones within the merge distance used.
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub section_type_id: Option<ElementRef>,
    pub material: Option<String>,
    #[serde(default)]
    pub role: Role,
    /// Both ends' release preset (default fixed).
    pub releases: Option<ReleasePreset>,
}

/// Creates an analytical member (`analytical_member.create`, STR-AM-001).
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let flags = p.releases.unwrap_or(ReleasePreset::Fixed).flags();
    let r = MemberRecipe {
        kind: MEMBER_RECIPE.into(),
        start: String::new(),
        end: String::new(),
        role: p.role,
        section_type: type_guid(&store, &p.section_type_id)?,
        material: p.material,
        releases: [flags, flags],
        analyze_as: AnalyzeAs::Releases,
        rotation: 0.0,
        physical: Vec::new(),
    };
    new_member(txn, p.start, p.end, r)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerticalParams {
    /// Its top's plan point, at `level`; it runs down to the level below.
    pub point: [f64; 2],
    pub level: ElementRef,
    pub section_type_id: Option<ElementRef>,
}

/// Creates a vertical analytical member from a level down to the one below
/// (`analytical_member.create_vertical`).
pub(crate) fn create_vertical(
    txn: &mut crate::txn::Transaction<'_>,
    p: VerticalParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.level)
        .filter(|id| {
            store
                .get(*id)
                .is_some_and(|e| e.class == crate::levels::CLASS)
        })
        .ok_or_else(|| Error::new("level.not_found", format!("no level {:?}", p.level)))?;
    let top = crate::levels::elevation(store.get(id).expect("resolved"));
    let below = crate::levels::list(&store)
        .into_iter()
        .filter(|l| l.elevation < top - 1e-6)
        .map(|l| l.elevation)
        .fold(f64::NEG_INFINITY, f64::max);
    if !below.is_finite() {
        return Err(Error::new(
            "analytical.no_level_below",
            "there is no level below this one",
        ));
    }
    create(
        txn,
        CreateParams {
            start: [p.point[0], p.point[1], below],
            end: [p.point[0], p.point[1], top],
            section_type_id: p.section_type_id,
            material: None,
            role: Role::Column,
            releases: None,
        },
    )
}

/// A member's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Start,
    End,
    Both,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseParams {
    pub member_ids: Vec<ElementRef>,
    pub end: End,
    /// `fixed` or `pinned` …
    pub preset: Option<ReleasePreset>,
    /// … or six flags (translations x, y, z, rotations x, y, z; true is fixed).
    pub flags: Option<[bool; 6]>,
}

/// Sets members' end releases (`analytical_member.set_releases`).
pub(crate) fn set_releases(txn: &mut crate::txn::Transaction<'_>, p: ReleaseParams) -> Result<()> {
    let flags = match (p.flags, p.preset) {
        (Some(f), _) => f,
        (None, Some(pr)) => pr.flags(),
        (None, None) => {
            return Err(Error::new(
                "analytical.releases",
                "give a preset or six flags",
            ));
        }
    };
    let store = txn.store().clone();
    for r in &p.member_ids {
        let (id, mut m) = resolve_member(&store, r)?;
        match p.end {
            End::Start => m.releases[0] = flags,
            End::End => m.releases[1] = flags,
            End::Both => m.releases = [flags, flags],
        }
        write_member(txn, id, &m)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemberSetParams {
    pub member_ids: Vec<ElementRef>,
    pub section_type_id: Option<ElementRef>,
    pub material: Option<String>,
    pub role: Option<Role>,
    pub analyze_as: Option<AnalyzeAs>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<f64>,
}

/// Changes members' section type, material, role, *Analyze As* or rotation
/// (`analytical_member.set`); the physical model changes only through automation.
pub(crate) fn set_member(txn: &mut crate::txn::Transaction<'_>, p: MemberSetParams) -> Result<()> {
    let store = txn.store().clone();
    let section = type_guid(&store, &p.section_type_id)?;
    for r in &p.member_ids {
        let (id, mut m) = resolve_member(&store, r)?;
        if section.is_some() {
            m.section_type = section.clone();
        }
        if let Some(v) = &p.material {
            m.material = Some(v.clone()).filter(|s| !s.is_empty());
        }
        if let Some(v) = p.role {
            m.role = v;
        }
        if let Some(v) = p.analyze_as {
            m.analyze_as = v;
        }
        if let Some(v) = p.rotation {
            m.rotation = v;
        }
        write_member(txn, id, &m)?;
    }
    Ok(())
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeParams {
    /// Default the Structural Settings' node merge distance.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub tolerance: Option<f64>,
}

/// What a merge did.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct MergeReport {
    pub merged: usize,
    /// Nodes no member used, removed.
    pub removed_unused: usize,
}

/// Merges nodes within the tolerance and removes nodes no member uses (`analytical_node.merge`).
pub(crate) fn merge(txn: &mut crate::txn::Transaction<'_>, p: MergeParams) -> Result<MergeReport> {
    let store = txn.store().clone();
    let tol = p.tolerance.unwrap_or_else(|| tolerance(&store));
    let all = nodes(&store);
    let mut keep: Vec<(ElementId, V3)> = Vec::new();
    let mut map: Vec<(String, String)> = Vec::new();
    let mut report = MergeReport::default();
    for (id, p) in &all {
        let guid = store.get(*id).expect("listed").guid().to_string();
        match keep.iter().find(|(_, q)| dist(*q, *p) <= tol) {
            Some((k, _)) => {
                map.push((guid, store.get(*k).expect("kept").guid().to_string()));
                txn.delete(*id)?;
                report.merged += 1;
            }
            None => keep.push((*id, *p)),
        }
    }
    let mut used = HashSet::new();
    for e in store.iter() {
        let Some(mut m) = member_recipe(e) else {
            continue;
        };
        let mut changed = false;
        for (from, to) in &map {
            if m.start == *from {
                m.start = to.clone();
                changed = true;
            }
            if m.end == *from {
                m.end = to.clone();
                changed = true;
            }
        }
        used.insert(m.start.clone());
        used.insert(m.end.clone());
        if changed {
            write_member(txn, e.id(), &m)?;
        }
    }
    for (id, _) in keep {
        let guid = store.get(id).expect("kept").guid().to_string();
        if !used.contains(&guid) {
            txn.delete(id)?;
            report.removed_unused += 1;
        }
    }
    Ok(report)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssociationParams {
    pub physical_ids: Vec<ElementRef>,
    pub analytical_ids: Vec<ElementRef>,
}

/// The analytical member a physical element is already associated with, if any.
fn associated_with(store: &Store, guid: &str) -> Option<ElementId> {
    store
        .iter()
        .find(|e| member_recipe(e).is_some_and(|m| m.physical.iter().any(|p| p == guid)))
        .map(Element::id)
}

/// Associates physical elements with analytical members (`analytical_association.add`,
/// STR-AM-004). An element already in an association is refused, naming it.
pub(crate) fn associate(
    txn: &mut crate::txn::Transaction<'_>,
    p: AssociationParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let mut physical = Vec::new();
    for r in &p.physical_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let guid = store.get(id).expect("resolved").guid().to_string();
        if let Some(m) = associated_with(&store, &guid) {
            return Err(Error::new(
                "analytical.already_associated",
                format!("element {id} already belongs to the association of analytical member {m}"),
            ));
        }
        physical.push(guid);
    }
    let mut out = Vec::new();
    for r in &p.analytical_ids {
        let (id, mut m) = resolve_member(&store, r)?;
        if !m.physical.is_empty() {
            return Err(Error::new(
                "analytical.already_associated",
                format!("analytical member {id} already belongs to an association"),
            ));
        }
        m.physical = physical.clone();
        write_member(txn, id, &m)?;
        out.push(id);
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DisconnectParams {
    /// Analytical members or physical elements; omitted, every association.
    pub element_ids: Option<Vec<ElementRef>>,
}

/// Removes associations, leaving both models as they are (`analytical_association.remove`,
/// `analytical_automation.disconnect`). Returns the members that were disconnected.
pub(crate) fn disconnect(
    txn: &mut crate::txn::Transaction<'_>,
    p: DisconnectParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let picked: Option<HashSet<String>> = p.element_ids.as_ref().map(|refs| {
        refs.iter()
            .filter_map(|r| store.resolve(r))
            .filter_map(|id| store.get(id).map(|e| e.guid().to_string()))
            .collect()
    });
    let mut out = Vec::new();
    for e in store.iter() {
        let Some(mut m) = member_recipe(e) else {
            continue;
        };
        if m.physical.is_empty() {
            continue;
        }
        let hit = picked.as_ref().is_none_or(|set| {
            set.contains(&e.guid().to_string()) || m.physical.iter().any(|g| set.contains(g))
        });
        if hit {
            m.physical.clear();
            write_member(txn, e.id(), &m)?;
            out.push(e.id());
        }
    }
    Ok(out)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutomationParams {
    /// Physical (or, for analytical to physical, analytical) elements; omitted, every one that
    /// applies (physical elements without an association).
    pub element_ids: Option<Vec<ElementRef>>,
}

/// An automation run's report: what it created or updated, and what it skipped and why.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct AutomationReport {
    pub created: Vec<ElementId>,
    pub updated: Vec<ElementId>,
    pub skipped: Vec<(ElementId, String)>,
}

/// A physical framing element's axis and its member defaults: role, section type, material and
/// end releases.
fn physical_member(
    store: &Store,
    e: &Element,
) -> std::result::Result<(V3, V3, MemberRecipe), String> {
    let type_guid = e.param("@type").and_then(Value::as_str).map(str::to_owned);
    let (a, b, role, pinned, material) = if crate::columns::column_recipe(e).is_some() {
        let g = crate::columns::regenerate(store, e).map_err(|x| x.message)?;
        if !g.ty.structural {
            return Err("an architectural column carries no load".into());
        }
        let (a, b) = g.axis();
        (a, b, Role::Column, false, g.ty.material)
    } else if let Some(r) = crate::beams::beam_recipe(e) {
        let g = crate::beams::regenerate(store, e).map_err(|x| x.message)?;
        let role = if r.brace {
            Role::Brace
        } else if r.system.as_ref().is_some_and(|s| {
            find(store, &s.system)
                .and_then(|t| store.get(t))
                .and_then(crate::beams::truss_recipe)
                .is_some()
        }) {
            Role::Member
        } else {
            Role::Beam
        };
        (g.a, g.b, role, r.brace, g.ty.material)
    } else {
        return Err("not a column, beam, brace or truss member".into());
    };
    let flags = if pinned {
        ReleasePreset::Pinned.flags()
    } else {
        ReleasePreset::Fixed.flags()
    };
    Ok((
        a,
        b,
        MemberRecipe {
            kind: MEMBER_RECIPE.into(),
            start: String::new(),
            end: String::new(),
            role,
            section_type: type_guid,
            material,
            releases: [flags, flags],
            analyze_as: AnalyzeAs::Releases,
            rotation: 0.0,
            physical: vec![e.guid().to_string()],
        },
    ))
}

/// Physical to analytical (`analytical_automation.physical_to_analytical`, STR-AM-006): a member
/// for each structural column, beam, brace and truss member (the given ones, or every one without
/// an association) along its axis, its nodes merged within the node merge distance, associated
/// with it.
pub(crate) fn physical_to_analytical(
    txn: &mut crate::txn::Transaction<'_>,
    p: AutomationParams,
) -> Result<AutomationReport> {
    let store = txn.store().clone();
    let candidates: Vec<ElementId> = match &p.element_ids {
        Some(refs) => refs.iter().filter_map(|r| store.resolve(r)).collect(),
        None => store
            .iter()
            .filter(|e| {
                crate::columns::column_recipe(e).is_some() || crate::beams::beam_recipe(e).is_some()
            })
            .map(Element::id)
            .collect(),
    };
    let mut report = AutomationReport::default();
    for id in candidates {
        let e = store.get(id).expect("resolved");
        let guid = e.guid().to_string();
        if let Some(m) = associated_with(txn.store(), &guid) {
            report
                .skipped
                .push((id, format!("already associated with analytical member {m}")));
            continue;
        }
        match physical_member(&store, e) {
            Ok((a, b, r)) => match new_member(txn, a, b, r) {
                Ok(m) => report.created.push(m),
                Err(err) => report.skipped.push((id, err.message)),
            },
            Err(why) => report.skipped.push((id, why)),
        }
    }
    Ok(report)
}

/// Analytical to physical (`analytical_automation.analytical_to_physical`): each associated
/// member's section type given to its physical elements where it differs (a beam type to beams,
/// a column type to columns).
pub(crate) fn analytical_to_physical(
    txn: &mut crate::txn::Transaction<'_>,
    p: AutomationParams,
) -> Result<AutomationReport> {
    let store = txn.store().clone();
    let picked: Vec<(ElementId, MemberRecipe)> = match &p.element_ids {
        Some(refs) => refs
            .iter()
            .map(|r| resolve_member(&store, r))
            .collect::<Result<_>>()?,
        None => store
            .iter()
            .filter_map(|e| Some((e.id(), member_recipe(e)?)))
            .collect(),
    };
    let mut report = AutomationReport::default();
    for (mid, m) in picked {
        let Some(section) = m.section_type.clone() else {
            report.skipped.push((mid, "no section type".into()));
            continue;
        };
        let Some(ty) = find(&store, &section).and_then(|t| store.get(t)) else {
            report
                .skipped
                .push((mid, "its section type is gone".into()));
            continue;
        };
        for g in &m.physical {
            let Some(pid) = find(&store, g) else { continue };
            let pe = store.get(pid).expect("found");
            if pe.param("@type").and_then(Value::as_str) == Some(section.as_str()) {
                continue;
            }
            let fits = (crate::beams::beam_recipe(pe).is_some()
                && crate::beams::beam_type(ty).is_some())
                || (crate::columns::column_recipe(pe).is_some()
                    && crate::columns::column_type(ty).is_some());
            if !fits {
                report.skipped.push((
                    pid,
                    "the section type does not fit the physical element".into(),
                ));
                continue;
            }
            let guid = ty.guid().to_string();
            txn.modify(pid, |e| {
                e.params.insert("@type".into(), Value::String(guid));
            })?;
            report.updated.push(pid);
        }
    }
    Ok(report)
}

/// The analytical model as reported (`analytical_model.info`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ModelInfo {
    pub members: Vec<MemberInfo>,
    pub nodes: usize,
    /// Nodes used by only one member and without a boundary condition (free ends).
    pub free_nodes: Vec<ElementId>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MemberInfo {
    pub id: ElementId,
    pub role: Role,
    pub predefined_type: String,
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub nodes: [ElementId; 2],
    pub section_type: Option<ElementId>,
    pub material: Option<String>,
    pub releases: [[bool; 6]; 2],
    pub physical: Vec<ElementId>,
}

/// Reports the analytical model (`analytical_model.info`).
pub fn info(store: &Store) -> ModelInfo {
    let ms = members(store);
    let mut count: std::collections::HashMap<ElementId, usize> = Default::default();
    for (_, m) in &ms {
        for n in m.nodes {
            *count.entry(n).or_default() += 1;
        }
    }
    // A supported node is not free.
    let supported = |n: &ElementId| {
        store
            .get(*n)
            .and_then(node_recipe)
            .is_some_and(|r| r.support.is_some())
    };
    let mut free: Vec<ElementId> = count
        .iter()
        .filter(|(n, c)| **c == 1 && !supported(n))
        .map(|(n, _)| *n)
        .collect();
    free.sort();
    ModelInfo {
        nodes: nodes(store).len(),
        free_nodes: free,
        members: ms
            .into_iter()
            .map(|(id, m)| MemberInfo {
                id,
                role: m.recipe.role,
                predefined_type: predefined(&m.recipe).into(),
                start: m.start,
                end: m.end,
                nodes: m.nodes,
                section_type: m
                    .recipe
                    .section_type
                    .as_deref()
                    .and_then(|g| find(store, g)),
                material: m.recipe.material.clone(),
                releases: m.recipe.releases,
                physical: m
                    .recipe
                    .physical
                    .iter()
                    .filter_map(|g| find(store, g))
                    .collect(),
            })
            .collect(),
    }
}
