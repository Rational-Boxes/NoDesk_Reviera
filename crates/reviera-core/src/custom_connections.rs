// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Custom connections (STR-STL-003): connection types made from what is at a join.
//!
//! **Customize** takes standard connections at one join and loose plates, fastener patterns and
//! welds there, and makes a named type of them. It records, relative to the join (its point, the
//! main member's axis, and the direction the connected member leaves in), each nested standard
//! connection with its own parameters and the members it joins, each plate (its contour, plane
//! and thickness; the member face it sits on, if any), each pattern and each weld; and the join's
//! signature: up to three members, their roles (column, beam, brace) and section families. The
//! connections and elements chosen become the first **instance**: they are kept, not copied.
//!
//! **Placing** a custom type on another join with the same signature makes an instance there:
//! the nested connections are made for the new members, and the plates, patterns and welds are
//! made where the type puts them (a plate on a member face is hosted by the matching face of the
//! new member, so it follows it). **Add** and **Remove** change the type and remake every
//! instance; with more than one instance they need `confirm`, and the refusal states the count.
//! **Exposed parameters** (`<sub-element name>.<parameter>`: a plate's `thickness`, a pattern's
//! `diameter`, a weld's `a`, a nested connection's parameters) become the type's parameters; a
//! new value remakes every instance.
//!
//! Not yet: profiles and parametric cuts as sub-elements, the edit mode with half-toning and
//! journalled picks, patterns and welds following members after placement (they are placed in
//! the join's frame), exporting types as library packages.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::columns::Section;
use crate::connections;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::steel_detail::{self, Justification, Layout, Location, PatternKind};
use crate::store::Store;
use crate::walls::RECIPE_KEY;
use crate::work_planes::{ExplicitPlane, PlaneArgs, WorkPlaneRef};

pub const TYPES_KEY: &str = "rxd:custom_connection_types";
pub const PART_KEY: &str = "rxd:custom_part";
pub const PREFIX: &str = "custom:";

type V3 = [f64; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l < 1e-12 { a } else { mul(a, 1.0 / l) }
}

// ---- The join's frame ----

/// A join's frame: its point, the main member's axis (x), the way the connected member leaves
/// (y, square to x), and z = x × y.
#[derive(Debug, Clone, Copy)]
struct JoinFrame {
    o: V3,
    x: V3,
    y: V3,
    z: V3,
}

impl JoinFrame {
    fn local(&self, p: V3) -> V3 {
        let d = sub(p, self.o);
        [dot(d, self.x), dot(d, self.y), dot(d, self.z)]
    }
    fn local_dir(&self, v: V3) -> V3 {
        [dot(v, self.x), dot(v, self.y), dot(v, self.z)]
    }
    fn world(&self, l: V3) -> V3 {
        add(self.o, add(mul(self.x, l[0]), add(mul(self.y, l[1]), mul(self.z, l[2]))))
    }
    fn world_dir(&self, l: V3) -> V3 {
        add(mul(self.x, l[0]), add(mul(self.y, l[1]), mul(self.z, l[2])))
    }
}

fn join_frame(store: &Store, members: &[ElementId], point: V3) -> Option<JoinFrame> {
    let main = steel_detail::member_frame(store, store.get(*members.first()?)?)?;
    let x = main.axis;
    let leave = members.get(1).and_then(|m| steel_detail::member_frame(store, store.get(*m)?)).map(|f| {
        // Into the connected member from the join.
        let d = |q: V3| dot(sub(q, point), sub(q, point));
        if d(f.start) <= d(f.end) { f.axis } else { mul(f.axis, -1.0) }
    });
    let y = leave
        .map(|v| sub(v, mul(x, dot(v, x))))
        .filter(|v| dot(*v, *v) > 1e-8)
        .map(unit)
        .unwrap_or(main.across);
    Some(JoinFrame { o: point, x, y, z: cross(x, y) })
}

/// The point of a connection's join.
fn join_point(store: &Store, e: &Element) -> Option<V3> {
    connections::regenerate(store, e).ok().map(|g| g.point)
}

// ---- The type ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemberSig {
    /// `column`, `beam` or `brace`.
    pub role: String,
    pub section: Section,
    /// For a connected member: which of the main member's faces it meets (`flange` or `web`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meets: Option<String>,
}

/// Something a sub-element refers to: one of the join's members, or one of the type's plates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Ref {
    Member(usize),
    Plate(usize),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Nested {
    pub name: String,
    pub connection_type: String,
    /// Its own parameter values.
    #[serde(default)]
    pub params: Map<String, Value>,
    /// The join's members it joins, by index.
    pub members: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubPlate {
    pub name: String,
    /// The contour's corners and the plane's origin, x and normal, in the join's frame.
    pub contour: Vec<V3>,
    pub origin: V3,
    pub x: V3,
    pub normal: V3,
    pub thickness: f64,
    pub justification: Justification,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub corner_cuts: Vec<crate::steel_cuts::CornerCut>,
    /// The member it sits on, if on a member's face.
    #[serde(default)]
    pub host: Option<usize>,
    #[serde(default)]
    pub offset: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubPattern {
    pub name: String,
    pub pattern: PatternKind,
    pub elements: Vec<Ref>,
    pub origin: V3,
    pub x: V3,
    pub normal: V3,
    pub layout: Layout,
    pub diameter: f64,
    pub standard: String,
    #[serde(default)]
    pub grade: Option<String>,
    #[serde(default)]
    pub grip_increase: f64,
    pub hole_tolerance: f64,
    #[serde(default)]
    pub slot: Option<f64>,
    #[serde(default)]
    pub inverted: bool,
    #[serde(default)]
    pub location: Location,
    #[serde(default)]
    pub length: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubWeld {
    pub name: String,
    pub elements: Vec<Ref>,
    pub edge: [V3; 2],
    pub weld_type: String,
    #[serde(default)]
    pub weld_type_2: Option<String>,
    pub a: f64,
    #[serde(default)]
    pub length: Option<f64>,
    pub surface: String,
    #[serde(default)]
    pub preparation: Option<String>,
    #[serde(default)]
    pub location: Location,
    #[serde(default)]
    pub intermittent: Option<steel_detail::Intermittent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Exposed {
    /// `<sub-element name>.<parameter>`.
    pub path: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CustomType {
    /// `custom:<n>`.
    pub id: String,
    pub name: String,
    pub members: Vec<MemberSig>,
    #[serde(default)]
    pub nested: Vec<Nested>,
    #[serde(default)]
    pub plates: Vec<SubPlate>,
    #[serde(default)]
    pub patterns: Vec<SubPattern>,
    #[serde(default)]
    pub welds: Vec<SubWeld>,
    #[serde(default)]
    pub exposed: Vec<Exposed>,
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub fn types(store: &Store) -> Vec<CustomType> {
    project(store)
        .and_then(|p| p.param(TYPES_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub fn custom_type(store: &Store, id: &str) -> Option<CustomType> {
    types(store).into_iter().find(|t| t.id == id)
}

fn save_types(txn: &mut crate::txn::Transaction<'_>, all: &[CustomType]) -> Result<()> {
    let store = txn.store().clone();
    let pid = project(&store)
        .map(|e| e.id())
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    txn.set_param(pid, TYPES_KEY, serde_json::to_value(all).expect("serialises"))
}

fn by_guid(store: &Store, g: &str) -> Option<ElementId> {
    GlobalId::parse(g).and_then(|g| store.find_by_guid(&g))
}

fn guid(store: &Store, id: ElementId) -> String {
    store.get(id).expect("an element").guid().to_string()
}

fn signature(store: &Store, id: ElementId) -> Option<MemberSig> {
    let e = store.get(id)?;
    let role = if crate::columns::column_recipe(e).is_some() {
        "column"
    } else if crate::beams::beam_recipe(e)?.brace {
        "brace"
    } else {
        "beam"
    };
    let f = steel_detail::member_frame(store, e)?;
    Some(MemberSig { role: role.into(), section: f.ty.section, meets: None })
}

/// The members' signatures at a join: roles, sections, and which face of the main member each
/// connected member meets (its flanges face along the main member's depth direction).
fn signatures(store: &Store, members: &[ElementId], point: V3) -> Option<Vec<MemberSig>> {
    let main = steel_detail::member_frame(store, store.get(*members.first()?)?)?;
    members
        .iter()
        .enumerate()
        .map(|(k, m)| {
            let mut s = signature(store, *m)?;
            if k > 0 {
                let f = steel_detail::member_frame(store, store.get(*m)?)?;
                let d = |q: V3| dot(sub(q, point), sub(q, point));
                let leave = if d(f.start) <= d(f.end) { f.axis } else { mul(f.axis, -1.0) };
                let along = sub(leave, mul(main.axis, dot(leave, main.axis)));
                if dot(along, along) > 1e-6 {
                    s.meets = Some(if dot(along, main.deep).abs() >= dot(along, main.across).abs() { "flange" } else { "web" }.into());
                }
            }
            Some(s)
        })
        .collect()
}

fn resolve(store: &Store, r: &ElementRef) -> Result<ElementId> {
    store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
}

/// What a custom instance's child is: its instance and sub-element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PartTag {
    pub instance: String,
    /// `nested:<n>`, `plate:<n>`, `pattern:<n>`, `weld:<n>`.
    pub sub: String,
}

pub fn part_tag(e: &Element) -> Option<PartTag> {
    e.param(PART_KEY).and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// Whether an element goes with `guid`: a custom instance's child.
pub fn uses(e: &Element, guid: &str) -> bool {
    part_tag(e).is_some_and(|t| t.instance == guid)
}

/// The instances of a custom type.
pub fn instances(store: &Store, type_id: &str) -> Vec<ElementId> {
    store
        .iter()
        .filter(|e| connections::recipe(e).is_some_and(|r| r.connection_type == type_id))
        .map(|e| e.id())
        .collect()
}

// ---- Recording ----

/// What `customize` and `add` record from elements, given the join's members and frame.
struct Recorder<'a> {
    store: &'a Store,
    members: Vec<ElementId>,
    frame: JoinFrame,
    /// The plates recorded, by element.
    plates: Vec<ElementId>,
}

impl Recorder<'_> {
    fn reference(&self, id: ElementId) -> Option<Ref> {
        if let Some(k) = self.members.iter().position(|m| *m == id) {
            return Some(Ref::Member(k));
        }
        self.plates.iter().position(|p| *p == id).map(Ref::Plate)
    }

    fn plate(&mut self, id: ElementId, name: String) -> Result<SubPlate> {
        let e = self.store.get(id).expect("listed");
        let r = steel_detail::plate_recipe(e).expect("a plate");
        let pl = steel_detail::plate_plane(self.store, &r);
        let host = match &r.plane {
            WorkPlaneRef::Face { element, offset, .. } => by_guid(self.store, element)
                .and_then(|h| self.members.iter().position(|m| *m == h))
                .map(|k| (k, *offset)),
            _ => None,
        };
        self.plates.push(id);
        let f = self.frame;
        Ok(SubPlate {
            name,
            contour: r.contour.iter().map(|q| f.local(pl.to_world(*q))).collect(),
            origin: f.local(pl.origin),
            x: f.local_dir(pl.x),
            normal: f.local_dir(pl.normal),
            thickness: r.thickness,
            justification: r.justification,
            material: r.material.clone(),
            role: r.role.clone(),
            corner_cuts: r.corner_cuts.clone(),
            host: host.map(|h| h.0),
            offset: host.map_or(0.0, |h| h.1),
        })
    }

    fn pattern(&self, id: ElementId, name: String) -> Result<SubPattern> {
        let e = self.store.get(id).expect("listed");
        let r = steel_detail::pattern_recipe(e).expect("a pattern");
        let pl = steel_detail::pattern_plane(self.store, &r);
        let elements = r
            .elements
            .iter()
            .map(|g| {
                by_guid(self.store, g).and_then(|i| self.reference(i)).ok_or_else(|| {
                    Error::new("custom.outside", format!("pattern {id} joins an element outside the connection"))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let f = self.frame;
        Ok(SubPattern {
            name,
            pattern: r.pattern,
            elements,
            origin: f.local(pl.origin),
            x: f.local_dir(pl.x),
            normal: f.local_dir(pl.normal),
            layout: r.layout.clone(),
            diameter: r.diameter,
            standard: r.standard.clone(),
            grade: r.grade.clone(),
            grip_increase: r.grip_increase,
            hole_tolerance: r.hole_tolerance,
            slot: r.slot,
            inverted: r.inverted,
            location: r.location,
            length: r.length,
        })
    }

    fn weld(&self, id: ElementId, name: String) -> Result<SubWeld> {
        let e = self.store.get(id).expect("listed");
        let r = steel_detail::weld_recipe(e).expect("a weld");
        let elements = r
            .elements
            .iter()
            .map(|g| {
                by_guid(self.store, g).and_then(|i| self.reference(i)).ok_or_else(|| {
                    Error::new("custom.outside", format!("weld {id} joins an element outside the connection"))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let f = self.frame;
        Ok(SubWeld {
            name,
            elements,
            edge: [f.local(r.edge[0]), f.local(r.edge[1])],
            weld_type: r.weld_type.clone(),
            weld_type_2: r.weld_type_2.clone(),
            a: r.a,
            length: r.length,
            surface: r.surface.clone(),
            preparation: r.preparation.clone(),
            location: r.location,
            intermittent: r.intermittent.clone(),
        })
    }
}

/// The elements given, sorted by kind: plates, patterns, welds.
fn sort_elements(store: &Store, ids: &[ElementId]) -> Result<(Vec<ElementId>, Vec<ElementId>, Vec<ElementId>)> {
    let (mut plates, mut patterns, mut welds) = (Vec::new(), Vec::new(), Vec::new());
    for id in ids {
        let e = store.get(*id).expect("resolved");
        if steel_detail::plate_recipe(e).is_some() {
            plates.push(*id);
        } else if steel_detail::pattern_recipe(e).is_some() {
            patterns.push(*id);
        } else if steel_detail::weld_recipe(e).is_some() {
            welds.push(*id);
        } else {
            return Err(Error::new(
                "custom.element",
                format!("element {id} is not a plate, fastener pattern or weld (connections go in connection_ids)"),
            ));
        }
    }
    Ok((plates, patterns, welds))
}

fn name_of(store: &Store, id: ElementId, kind: &str, n: usize) -> String {
    store.get(id).and_then(|e| e.name.clone()).unwrap_or_else(|| format!("{kind} {n}"))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CustomizeParams {
    /// Standard or generic connections at one join.
    pub connection_ids: Vec<ElementRef>,
    /// Loose plates, fastener patterns and welds at the join.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
    /// The new type's name (unique among connection types).
    pub name: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Customized {
    pub type_id: String,
    /// The first instance (it holds what was chosen).
    pub instance: ElementId,
}

/// Makes a custom connection type of what is at a join (`structural_connection.customize`).
pub(crate) fn customize(txn: &mut crate::txn::Transaction<'_>, p: CustomizeParams) -> Result<Customized> {
    let store = txn.store().clone();
    let name = p.name.trim().to_owned();
    if name.is_empty() {
        return Err(Error::new("custom.name", "give the connection type a name"));
    }
    let all = types(&store);
    if all.iter().any(|t| t.name.eq_ignore_ascii_case(&name))
        || crate::steel_fab::library().iter().any(|t| t.name.eq_ignore_ascii_case(&name))
    {
        return Err(Error::new("custom.name", format!("there is already a connection type named {name}")));
    }
    let conns: Vec<ElementId> = p.connection_ids.iter().map(|r| resolve(&store, r)).collect::<Result<_>>()?;
    let mut recipes = Vec::new();
    for c in &conns {
        let r = store
            .get(*c)
            .and_then(connections::recipe)
            .ok_or_else(|| Error::new("connection.not_a_connection", format!("element {c} is not a connection")))?;
        if r.connection_type.starts_with(PREFIX) {
            return Err(Error::new("custom.nested", format!("connection {c} is already a custom connection")));
        }
        recipes.push(r);
    }
    let elements: Vec<ElementId> = p.element_ids.iter().map(|r| resolve(&store, r)).collect::<Result<_>>()?;
    let (plates, patterns, welds) = sort_elements(&store, &elements)?;
    // The join's members: the connections' members in order, then members the elements touch.
    let mut members: Vec<ElementId> = Vec::new();
    for r in &recipes {
        for g in &r.members {
            if let Some(m) = by_guid(&store, g)
                && !members.contains(&m)
            {
                members.push(m);
            }
        }
    }
    let touched = |e: &Element| -> Vec<String> {
        if let Some(r) = steel_detail::plate_recipe(e) {
            crate::work_planes::host_of(&r.plane).map(str::to_owned).into_iter().collect()
        } else if let Some(r) = steel_detail::pattern_recipe(e) {
            r.elements
        } else if let Some(r) = steel_detail::weld_recipe(e) {
            r.elements
        } else {
            Vec::new()
        }
    };
    for id in &elements {
        for g in touched(store.get(*id).expect("resolved")) {
            if let Some(m) = by_guid(&store, &g)
                && signature(&store, m).is_some()
                && !members.contains(&m)
            {
                members.push(m);
            }
        }
    }
    if members.is_empty() {
        return Err(Error::new("custom.members", "the selection joins no members"));
    }
    if members.len() > 3 {
        return Err(Error::new(
            "custom.members",
            format!("a custom connection joins at most three members; the selection joins {}", members.len()),
        ));
    }
    // The join's point: the first connection's, else the main member's start.
    let point = conns
        .first()
        .and_then(|c| join_point(&store, store.get(*c)?))
        .or_else(|| steel_detail::member_frame(&store, store.get(members[0])?).map(|f| f.start))
        .expect("a member");
    let sigs = signatures(&store, &members, point)
        .ok_or_else(|| Error::new("custom.members", "every member must be a column, beam or brace"))?;
    let frame = join_frame(&store, &members, point).ok_or_else(|| Error::new("custom.members", "the main member has no frame"))?;
    let mut rec = Recorder { store: &store, members: members.clone(), frame, plates: Vec::new() };
    let nested: Vec<Nested> = recipes
        .iter()
        .enumerate()
        .map(|(k, r)| Nested {
            name: format!(
                "{} {}",
                crate::steel_fab::connection_type(&r.connection_type).map_or("Generic", |t| t.name),
                k + 1
            ),
            connection_type: r.connection_type.clone(),
            params: r.params.clone(),
            members: r.members.iter().filter_map(|g| by_guid(&store, g)).filter_map(|m| members.iter().position(|x| *x == m)).collect(),
        })
        .collect();
    let sub_plates = plates
        .iter()
        .enumerate()
        .map(|(k, id)| rec.plate(*id, name_of(&store, *id, "Plate", k + 1)))
        .collect::<Result<Vec<_>>>()?;
    let sub_patterns = patterns
        .iter()
        .enumerate()
        .map(|(k, id)| rec.pattern(*id, name_of(&store, *id, "Bolts", k + 1)))
        .collect::<Result<Vec<_>>>()?;
    let sub_welds = welds
        .iter()
        .enumerate()
        .map(|(k, id)| rec.weld(*id, name_of(&store, *id, "Weld", k + 1)))
        .collect::<Result<Vec<_>>>()?;
    let n = all.iter().filter_map(|t| t.id.strip_prefix(PREFIX)?.parse::<usize>().ok()).max().unwrap_or(0) + 1;
    let ty = CustomType {
        id: format!("{PREFIX}{n}"),
        name,
        members: sigs,
        nested,
        plates: sub_plates,
        patterns: sub_patterns,
        welds: sub_welds,
        exposed: Vec::new(),
    };
    let mut all = all;
    all.push(ty.clone());
    save_types(txn, &all)?;
    // The first instance holds what was chosen.
    let instance = make_instance(txn, &ty, &members)?;
    let ig = guid(txn.store(), instance);
    let tag = |sub: String| serde_json::to_value(PartTag { instance: ig.clone(), sub }).expect("serialises");
    for (k, c) in conns.iter().enumerate() {
        txn.set_param(*c, PART_KEY, tag(format!("nested:{k}")))?;
    }
    for (kind, list) in [("plate", &plates), ("pattern", &patterns), ("weld", &welds)] {
        for (k, id) in list.iter().enumerate() {
            txn.set_param(*id, PART_KEY, tag(format!("{kind}:{k}")))?;
        }
    }
    Ok(Customized { type_id: ty.id, instance })
}

/// Makes an empty instance element of a type on members.
fn make_instance(txn: &mut crate::txn::Transaction<'_>, ty: &CustomType, members: &[ElementId]) -> Result<ElementId> {
    let id = connections::create(
        txn,
        connections::CreateParams { member_ids: members.iter().map(|m| ElementRef::Id(*m)).collect(), type_id: None },
    )
    .or_else(|e| {
        // A single member (a custom column base) has no join of two: make it by hand.
        if members.len() == 1 {
            let store = txn.store().clone();
            let mut r = connections::blank();
            r.members = vec![guid(&store, members[0])];
            txn.create(
                NewElement::new("IfcElementAssembly")
                    .param("@PredefinedType", "USERDEFINED")
                    .param("@ObjectType", "STRUCTURAL_CONNECTION")
                    .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
            )
        } else {
            Err(e)
        }
    })?;
    let store = txn.store().clone();
    let mut r = store.get(id).and_then(connections::recipe).expect("made");
    r.connection_type = ty.id.clone();
    txn.set_param(id, RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))?;
    let name = ty.name.clone();
    txn.modify(id, |e| e.name = Some(name))?;
    Ok(id)
}

// ---- Placing ----

/// Makes an instance's children from its type, on its members.
fn build(txn: &mut crate::txn::Transaction<'_>, ty: &CustomType, instance: ElementId, members: &[ElementId]) -> Result<()> {
    let store = txn.store().clone();
    let ie = store.get(instance).expect("an instance");
    let point = join_point(&store, ie).ok_or_else(|| Error::new("custom.join", "the instance's members do not meet"))?;
    let frame = join_frame(&store, members, point).ok_or_else(|| Error::new("custom.join", "the main member has no frame"))?;
    let ig = ie.guid().to_string();
    let tag = |sub: String| serde_json::to_value(PartTag { instance: ig.clone(), sub }).expect("serialises");
    for (k, n) in ty.nested.iter().enumerate() {
        let ms: Vec<ElementRef> = n.members.iter().filter_map(|i| members.get(*i)).map(|m| ElementRef::Id(*m)).collect();
        let c = connections::create(
            txn,
            connections::CreateParams { member_ids: ms, type_id: Some(n.connection_type.clone()) },
        )?;
        if !n.params.is_empty() {
            crate::steel_fab::override_params(
                txn,
                crate::steel_fab::OverrideParams { connection_id: ElementRef::Id(c), params: n.params.clone() },
            )?;
        }
        txn.set_param(c, PART_KEY, tag(format!("nested:{k}")))?;
    }
    let mut plates: Vec<ElementId> = Vec::new();
    for (k, sp) in ty.plates.iter().enumerate() {
        let world: Vec<V3> = sp.contour.iter().map(|q| frame.world(*q)).collect();
        let (origin, x, normal) = (frame.world(sp.origin), frame.world_dir(sp.x), frame.world_dir(sp.normal));
        // On a member's face: the new member's face that faces the same way.
        let hosted = sp.host.and_then(|h| members.get(h)).and_then(|m| {
            let e = store.get(*m)?;
            steel_detail::faces_of(&store, e)
                .into_iter()
                .filter_map(|f| Some((f, steel_detail::face_plane(&store, e, f).ok()?)))
                .max_by(|a, b| dot(a.1.normal, normal).total_cmp(&dot(b.1.normal, normal)))
                .filter(|(_, pl)| dot(pl.normal, normal) > 0.99)
                .map(|(f, _)| (e.guid().to_string(), f))
        });
        let plane = match &hosted {
            Some((g, f)) => WorkPlaneRef::Face { element: g.clone(), face: (*f).to_owned(), offset: sp.offset },
            None => WorkPlaneRef::Explicit { origin, x_dir: x, normal },
        };
        let pl = crate::work_planes::resolve(&store, &plane)?;
        let contour: Vec<[f64; 2]> = world.iter().map(|w| pl.to_local(*w)).collect();
        let r = steel_detail::PlateRecipe {
            kind: steel_detail::PLATE_RECIPE.into(),
            plane,
            fallback: [pl.origin, pl.x, pl.normal],
            contour,
            thickness: sp.thickness,
            justification: sp.justification,
            material: sp.material.clone(),
            coating: None,
            role: sp.role.clone(),
            unhosted: false,
            corner_cuts: sp.corner_cuts.clone(),
        };
        let id = txn.create(
            NewElement::new("IfcPlate")
                .name(sp.name.clone())
                .param("@PredefinedType", "SHEET")
                .param("Pset_PlateCommon.LoadBearing", json!({ "type": "IfcBoolean", "value": true }))
                .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
        )?;
        txn.set_param(id, PART_KEY, tag(format!("plate:{k}")))?;
        plates.push(id);
    }
    let refer = |r: &Ref| -> Option<ElementId> {
        match r {
            Ref::Member(i) => members.get(*i).copied(),
            Ref::Plate(i) => plates.get(*i).copied(),
        }
    };
    for (k, sp) in ty.patterns.iter().enumerate() {
        let element_ids: Vec<ElementRef> = sp.elements.iter().filter_map(refer).map(ElementRef::Id).collect();
        let params = steel_detail::PatternCreateParams {
            element_ids,
            placement: PlaneArgs {
                plane: Some(ExplicitPlane {
                    origin: frame.world(sp.origin),
                    x_dir: frame.world_dir(sp.x),
                    normal: frame.world_dir(sp.normal),
                }),
                ..Default::default()
            },
            layout: sp.layout.clone(),
            diameter: Some(sp.diameter),
            standard: Some(sp.standard.clone()),
            grade: sp.grade.clone(),
            grip_increase: sp.grip_increase,
            hole_tolerance: Some(sp.hole_tolerance),
            slot: sp.slot,
            inverted: sp.inverted,
            location: sp.location,
            coating: None,
            length: sp.length,
        };
        let id = steel_detail::create_pattern(txn, params, sp.pattern)?;
        txn.set_param(id, PART_KEY, tag(format!("pattern:{k}")))?;
    }
    for (k, sw) in ty.welds.iter().enumerate() {
        let element_ids: Vec<ElementRef> = sw.elements.iter().filter_map(refer).map(ElementRef::Id).collect();
        let id = steel_detail::weld_create(
            txn,
            steel_detail::WeldCreateParams {
                element_ids,
                edge: [frame.world(sw.edge[0]), frame.world(sw.edge[1])],
                weld_type: Some(sw.weld_type.clone()),
                weld_type_2: sw.weld_type_2.clone(),
                a: Some(sw.a),
                length: sw.length,
                surface: Some(sw.surface.clone()),
                preparation: sw.preparation.clone(),
                location: sw.location,
                intermittent: sw.intermittent.clone(),
            },
        )?;
        txn.set_param(id, PART_KEY, tag(format!("weld:{k}")))?;
    }
    Ok(())
}

/// Places a custom connection on members (`structural_connection.create` with a `custom:` type).
pub(crate) fn place(txn: &mut crate::txn::Transaction<'_>, member_ids: &[ElementRef], type_id: &str) -> Result<ElementId> {
    let store = txn.store().clone();
    let ty = custom_type(&store, type_id)
        .ok_or_else(|| Error::new("connection.unknown_type", format!("no connection type {type_id}")))?;
    let members: Vec<ElementId> = member_ids.iter().map(|r| resolve(&store, r)).collect::<Result<_>>()?;
    if members.len() != ty.members.len() {
        return Err(Error::new(
            "custom.inputs",
            format!("{} joins {} member(s); {} given", ty.name, ty.members.len(), members.len()),
        ));
    }
    // Where they meet: the main member's end or axis nearest the others.
    let axes: Vec<(V3, V3)> = members.iter().filter_map(|m| connections::member_axis(&store, *m)).collect();
    let point = connections::join_point(&axes)
        .or_else(|| (members.len() == 1).then(|| steel_detail::member_frame(&store, store.get(members[0])?).map(|f| f.start)).flatten())
        .ok_or_else(|| Error::new("connection.no_join", "the members do not meet"))?;
    let sigs = signatures(&store, &members, point)
        .ok_or_else(|| Error::new("custom.inputs", "every member must be a column, beam or brace"))?;
    for (k, (have, want)) in sigs.iter().zip(&ty.members).enumerate() {
        if have.role != want.role {
            return Err(Error::new("custom.inputs", format!("{}: member {} must be a {}, not a {}", ty.name, k + 1, want.role, have.role)));
        }
        if have.section != want.section {
            return Err(Error::new(
                "custom.inputs",
                format!("{}: member {} must be a {:?} section, not {:?}", ty.name, k + 1, want.section, have.section),
            ));
        }
        if have.meets != want.meets {
            return Err(Error::new(
                "custom.inputs",
                format!(
                    "{}: member {} must meet the main member's {}, not its {}",
                    ty.name,
                    k + 1,
                    want.meets.as_deref().unwrap_or("end"),
                    have.meets.as_deref().unwrap_or("end")
                ),
            ));
        }
    }
    let instance = make_instance(txn, &ty, &members)?;
    build(txn, &ty, instance, &members)?;
    Ok(instance)
}

/// Remakes every instance of a type from it.
fn remake_all(txn: &mut crate::txn::Transaction<'_>, ty: &CustomType) -> Result<usize> {
    let store = txn.store().clone();
    let list = instances(&store, &ty.id);
    for inst in &list {
        let store = txn.store().clone();
        let ig = guid(&store, *inst);
        let children: Vec<ElementId> = store.iter().filter(|e| uses(e, &ig)).map(|e| e.id()).collect();
        for id in crate::builtin::delete_closure(&store, &children) {
            if store.get(id).is_some() && id != *inst {
                txn.delete(id)?;
            }
        }
        let r = store.get(*inst).and_then(connections::recipe).expect("an instance");
        let members: Vec<ElementId> = r.members.iter().filter_map(|g| by_guid(&store, g)).collect();
        build(txn, ty, *inst, &members)?;
    }
    Ok(list.len())
}

fn confirm(store: &Store, ty: &CustomType, confirm: bool) -> Result<()> {
    let n = instances(store, &ty.id).len();
    if n > 1 && !confirm {
        return Err(Error::new(
            "custom.confirm",
            format!("this changes all {n} instances of {}; pass confirm to go ahead", ty.name),
        )
        .with_details(json!({ "instances": n })));
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditParams {
    pub type_id: String,
    pub element_ids: Vec<ElementRef>,
    /// Required when the type has more than one instance.
    #[serde(default)]
    pub confirm: bool,
}

fn type_or(store: &Store, id: &str) -> Result<(Vec<CustomType>, usize)> {
    let all = types(store);
    let k = all
        .iter()
        .position(|t| t.id == id)
        .ok_or_else(|| Error::new("custom.unknown", format!("no custom connection type {id}")))?;
    Ok((all, k))
}

/// Adds plates, patterns or welds to a type and every instance (`custom_connection.add_elements`):
/// the elements are taken as placed at one of its instances' joins.
pub(crate) fn add_elements(txn: &mut crate::txn::Transaction<'_>, p: EditParams) -> Result<usize> {
    let store = txn.store().clone();
    let (mut all, k) = type_or(&store, &p.type_id)?;
    confirm(&store, &all[k], p.confirm)?;
    let ids: Vec<ElementId> = p.element_ids.iter().map(|r| resolve(&store, r)).collect::<Result<_>>()?;
    let (plates, patterns, welds) = sort_elements(&store, &ids)?;
    // The instance whose members the elements touch.
    let touches = |inst: ElementId| -> bool {
        let r = store.get(inst).and_then(connections::recipe).expect("an instance");
        ids.iter().any(|id| {
            let e = store.get(*id).expect("resolved");
            let gs: Vec<String> = steel_detail::plate_recipe(e)
                .and_then(|r| crate::work_planes::host_of(&r.plane).map(|g| vec![g.to_owned()]))
                .or_else(|| steel_detail::pattern_recipe(e).map(|r| r.elements))
                .or_else(|| steel_detail::weld_recipe(e).map(|r| r.elements))
                .unwrap_or_default();
            gs.iter().any(|g| r.members.contains(g))
        })
    };
    let inst = instances(&store, &all[k].id)
        .into_iter()
        .find(|i| touches(*i))
        .ok_or_else(|| Error::new("custom.outside", "the elements touch no instance's members"))?;
    let r = store.get(inst).and_then(connections::recipe).expect("an instance");
    let members: Vec<ElementId> = r.members.iter().filter_map(|g| by_guid(&store, g)).collect();
    let point = join_point(&store, store.get(inst).expect("an instance")).expect("a join");
    let frame = join_frame(&store, &members, point).expect("a frame");
    // The instance's plates come first, so the new elements can refer to them.
    let ig = guid(&store, inst);
    let mut existing: Vec<(usize, ElementId)> = store
        .iter()
        .filter_map(|e| {
            let t = part_tag(e).filter(|t| t.instance == ig)?;
            let n = t.sub.strip_prefix("plate:")?.parse::<usize>().ok()?;
            Some((n, e.id()))
        })
        .collect();
    existing.sort();
    let mut rec = Recorder { store: &store, members, frame, plates: existing.into_iter().map(|(_, i)| i).collect() };
    let ty = &mut all[k];
    for id in &plates {
        let n = ty.plates.len() + 1;
        ty.plates.push(rec.plate(*id, name_of(&store, *id, "Plate", n))?);
    }
    for id in &patterns {
        let n = ty.patterns.len() + 1;
        ty.patterns.push(rec.pattern(*id, name_of(&store, *id, "Bolts", n))?);
    }
    for id in &welds {
        let n = ty.welds.len() + 1;
        ty.welds.push(rec.weld(*id, name_of(&store, *id, "Weld", n))?);
    }
    let ty = ty.clone();
    save_types(txn, &all)?;
    // The loose elements are absorbed: the instance is made again with them in the type.
    for id in &ids {
        txn.delete(*id)?;
    }
    remake_all(txn, &ty)
}

/// Removes sub-elements (picked in any instance) from a type and every instance
/// (`custom_connection.remove_elements`).
pub(crate) fn remove_elements(txn: &mut crate::txn::Transaction<'_>, p: EditParams) -> Result<usize> {
    let store = txn.store().clone();
    let (mut all, k) = type_or(&store, &p.type_id)?;
    confirm(&store, &all[k], p.confirm)?;
    let insts = instances(&store, &all[k].id);
    let inst_guids: Vec<String> = insts.iter().map(|i| guid(&store, *i)).collect();
    let mut gone: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for r in &p.element_ids {
        let id = resolve(&store, r)?;
        let t = store
            .get(id)
            .and_then(part_tag)
            .filter(|t| inst_guids.contains(&t.instance))
            .ok_or_else(|| Error::new("custom.not_a_part", format!("element {id} is not part of a {} instance", all[k].name)))?;
        let (kind, n) = t.sub.split_once(':').unwrap_or(("", "0"));
        gone.entry(kind.to_owned()).or_default().push(n.parse().unwrap_or(usize::MAX));
    }
    let ty = &mut all[k];
    let drop = |kind: &str, len: usize| -> Vec<bool> {
        (0..len).map(|i| gone.get(kind).is_some_and(|v| v.contains(&i))).collect()
    };
    // Patterns and welds on a removed plate go with it.
    let removed_plates = drop("plate", ty.plates.len());
    let plate_map: Vec<Option<usize>> = {
        let mut next = 0;
        removed_plates.iter().map(|r| if *r { None } else { next += 1; Some(next - 1) }).collect()
    };
    let remap = |refs: &[Ref]| -> Option<Vec<Ref>> {
        refs.iter()
            .map(|r| match r {
                Ref::Member(i) => Some(Ref::Member(*i)),
                Ref::Plate(i) => plate_map.get(*i).copied().flatten().map(Ref::Plate),
            })
            .collect()
    };
    let keep = |v: &[bool], i: usize| !v.get(i).copied().unwrap_or(false);
    let (dn, dp, dw) = (drop("nested", ty.nested.len()), drop("pattern", ty.patterns.len()), drop("weld", ty.welds.len()));
    ty.nested = ty.nested.iter().enumerate().filter(|(i, _)| keep(&dn, *i)).map(|(_, n)| n.clone()).collect();
    ty.plates = ty.plates.iter().enumerate().filter(|(i, _)| keep(&removed_plates, *i)).map(|(_, n)| n.clone()).collect();
    ty.patterns = ty
        .patterns
        .iter()
        .enumerate()
        .filter(|(i, _)| keep(&dp, *i))
        .filter_map(|(_, s)| Some(SubPattern { elements: remap(&s.elements)?, ..s.clone() }))
        .collect();
    ty.welds = ty
        .welds
        .iter()
        .enumerate()
        .filter(|(i, _)| keep(&dw, *i))
        .filter_map(|(_, s)| Some(SubWeld { elements: remap(&s.elements)?, ..s.clone() }))
        .collect();
    let names: Vec<String> = sub_names(ty);
    ty.exposed.retain(|e| names.iter().any(|n| e.path.starts_with(&format!("{n}."))));
    let ty = ty.clone();
    save_types(txn, &all)?;
    remake_all(txn, &ty)
}

fn sub_names(ty: &CustomType) -> Vec<String> {
    ty.nested
        .iter()
        .map(|n| n.name.clone())
        .chain(ty.plates.iter().map(|p| p.name.clone()))
        .chain(ty.patterns.iter().map(|p| p.name.clone()))
        .chain(ty.welds.iter().map(|w| w.name.clone()))
        .collect()
}

/// A path's current value in a type, and a setter.
fn path_value(ty: &mut CustomType, path: &str, set: Option<f64>) -> Result<f64> {
    let bad = || Error::new("custom.path", format!("{path} is not a parameter of a sub-element (<name>.<parameter>)"));
    let (name, param) = path.rsplit_once('.').ok_or_else(bad)?;
    let param = param.to_ascii_lowercase();
    if let Some(p) = ty.plates.iter_mut().find(|p| p.name == name) {
        if param != "thickness" {
            return Err(bad());
        }
        if let Some(v) = set {
            if !(0.001..=0.2).contains(&v) {
                return Err(Error::new("plate.thickness", "a plate is 1 to 200 mm thick"));
            }
            p.thickness = v;
        }
        return Ok(p.thickness);
    }
    if let Some(p) = ty.patterns.iter_mut().find(|p| p.name == name) {
        if param != "diameter" {
            return Err(bad());
        }
        if let Some(v) = set {
            p.diameter = v;
        }
        return Ok(p.diameter);
    }
    if let Some(w) = ty.welds.iter_mut().find(|w| w.name == name) {
        if param != "a" {
            return Err(bad());
        }
        if let Some(v) = set {
            w.a = v;
        }
        return Ok(w.a);
    }
    if let Some(n) = ty.nested.iter_mut().find(|n| n.name == name) {
        let t = crate::steel_fab::connection_type(&n.connection_type).ok_or_else(bad)?;
        let d = t.params.iter().find(|d| d.key == param).ok_or_else(bad)?;
        if let Some(v) = set {
            n.params.insert(param.clone(), json!(v));
        }
        return Ok(n.params.get(&param).and_then(Value::as_f64).unwrap_or(d.metric));
    }
    Err(bad())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ExposeParams {
    pub type_id: String,
    /// `<sub-element name>.<parameter>`, for example `Stiffener 1.thickness`.
    pub path: String,
}

/// Makes a sub-element's parameter a type parameter (`custom_connection.expose_parameter`).
pub(crate) fn expose(txn: &mut crate::txn::Transaction<'_>, p: ExposeParams) -> Result<f64> {
    let store = txn.store().clone();
    let (mut all, k) = type_or(&store, &p.type_id)?;
    let v = path_value(&mut all[k], &p.path, None)?;
    if !all[k].exposed.iter().any(|e| e.path == p.path) {
        all[k].exposed.push(Exposed { path: p.path.clone(), value: v });
    }
    save_types(txn, &all)?;
    Ok(v)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetParams {
    pub type_id: String,
    pub path: String,
    pub value: f64,
}

/// Sets a type parameter and remakes every instance (`custom_connection.set_parameter`).
pub(crate) fn set_parameter(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<usize> {
    let store = txn.store().clone();
    let (mut all, k) = type_or(&store, &p.type_id)?;
    let Some(e) = all[k].exposed.iter().position(|e| e.path == p.path) else {
        return Err(Error::new("custom.not_exposed", format!("{} is not a parameter of {}: expose it first", p.path, all[k].name)));
    };
    path_value(&mut all[k], &p.path, Some(p.value))?;
    all[k].exposed[e].value = p.value;
    let ty = all[k].clone();
    save_types(txn, &all)?;
    remake_all(txn, &ty)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TypeInfo {
    #[serde(flatten)]
    pub ty: CustomType,
    pub instances: Vec<ElementId>,
    /// The sub-elements' names.
    pub sub_elements: Vec<String>,
}

/// The project's custom connection types (`custom_connection.list`).
pub fn list(store: &Store) -> Vec<TypeInfo> {
    types(store)
        .into_iter()
        .map(|t| TypeInfo { instances: instances(store, &t.id), sub_elements: sub_names(&t), ty: t })
        .collect()
}
