// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Legend views and legend components (spec Ch 5 §5.10, DOC-VIEW-010): a legend (`view.legend`)
//! is a drawing at a scale that holds legend components (`anno.legendcomponent`, annotation of
//! the legend) and text notes. A legend component draws a **type's** geometry, not a model
//! element: Reviera builds a sample of the type, alone, in a scratch copy of the model's types,
//! levels and settings ([`sample`]):
//!
//! - a wall type: a wall `host_length` long (default 2 m), 3 m high;
//! - a floor type: a floor `host_length` × 1 m;
//! - a door or window type: one in the middle of a wall `host_length` long (default its width
//!   plus 1.2 m), so a plan shows its swing and its wall;
//! - a component type: one placed at the origin.
//!
//! The application draws the sample in its direction: in plan (cut at 1.2 m), from the front or
//! the back, or in section across it (a wall's layers, a floor's thickness), and places the
//! drawing with its lower-left corner at the component's point. Editing the type changes every
//! legend component of it. Legend components are annotation: they appear in no schedule, count
//! or model export.
//!
//! Not yet: roof, ceiling, column, beam and stair types, detail levels other than the legend's,
//! legend components in model views.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::annotations::{ANNO_KEY, CLASS_PREFIX as ANNO_PREFIX, is_annotation};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::views::{CLASS_PREFIX, VIEW_KEY, ViewData};

pub const KIND_LEGEND: &str = "view.legend";
pub const KIND_COMPONENT: &str = "anno.legendcomponent";

/// The height a legend's plan cuts at (metres above the sample's base).
pub const PLAN_CUT: f64 = 1.2;

/// How a legend component looks at its type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// From above, cut at 1.2 m.
    #[default]
    Plan,
    /// From the front (looking north at the sample).
    Front,
    /// From the back.
    Back,
    /// Cut across: a wall's layers, a floor's thickness.
    Section,
}

/// What a type is, for its sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TypeKind {
    Wall,
    Floor,
    Door,
    Window,
    Component,
}

/// Stored data of a legend component (`anno.legendcomponent`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComponentData {
    pub kind: String,
    /// The legend (GlobalId).
    pub view: String,
    /// The type it draws (GlobalId).
    #[serde(rename = "type")]
    pub type_guid: String,
    pub direction: Direction,
    /// The sample's length along its wall (walls, floors, and the host of doors and windows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_length: Option<f64>,
    /// Where the drawing's lower-left corner goes, in the legend (metres at full size).
    pub point: [f64; 2],
}

pub fn component_data(e: &Element) -> Option<ComponentData> {
    if !is_annotation(e) {
        return None;
    }
    let v = e.param(ANNO_KEY)?;
    if v.get("kind")?.as_str()? != KIND_COMPONENT {
        return None;
    }
    serde_json::from_value(v.clone()).ok()
}

/// The legend components of a legend (by its GlobalId).
pub fn of_view(store: &Store, view_guid: &str) -> Vec<(ElementId, ComponentData)> {
    store
        .iter()
        .filter_map(|e| {
            let d = component_data(e)?;
            (d.view == view_guid).then(|| (e.id(), d))
        })
        .collect()
}

pub fn is_legend(e: &Element) -> bool {
    crate::views::view_data(e).is_some_and(|d| d.kind == KIND_LEGEND)
}

/// What a type element is, for a legend.
pub fn type_kind(e: &Element) -> Option<TypeKind> {
    if crate::mesh_objects::mesh_object_type(e).is_some() {
        return Some(TypeKind::Component);
    }
    match e.class.as_str() {
        "IfcWallType" => Some(TypeKind::Wall),
        "IfcSlabType" => Some(TypeKind::Floor),
        "IfcDoorType" | "IfcDoorStyle" => Some(TypeKind::Door),
        "IfcWindowType" | "IfcWindowStyle" => Some(TypeKind::Window),
        _ => None,
    }
}

/// A sample of a type: its elements' bodies, alone, with its base at z = 0, and the scratch
/// model they were built in (for their pens).
#[derive(Debug, Clone)]
pub struct Sample {
    pub kind: TypeKind,
    pub bodies: Vec<crate::bodies::Body>,
    pub store: Store,
    /// The sample's length along x (its host's, for doors and windows) and depth along y.
    pub size: [f64; 2],
}

/// Whether an element is copied into a scratch model: everything but placed model elements
/// (types, levels, materials, settings stay), annotation and views.
fn kept(e: &Element) -> bool {
    if is_annotation(e) || crate::views::is_view(e) {
        return false;
    }
    if e.class == crate::levels::CLASS || crate::categories::is_type(e) {
        return true;
    }
    crate::categories::category_of(e).is_none()
}

/// Builds a sample of the type `type_guid` (see the module's notes), alone in a scratch model.
pub fn sample(store: &Store, type_guid: &str, host_length: Option<f64>) -> Result<Sample> {
    let t = GlobalId::parse(type_guid)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
        .ok_or_else(|| Error::new("legend.no_type", format!("the type {type_guid} is not in the model")))?;
    let kind = type_kind(t).ok_or_else(|| Error::new("legend.unsupported_type", format!("a legend cannot draw {} yet", t.class)))?;
    let next = store.iter().map(|e| e.id().0).max().unwrap_or(0) + 1;
    let elements = store.iter().filter(|e| kept(e)).map(|e| {
        (e.id(), NewElement { class: e.class.clone(), name: e.name.clone(), params: e.params.clone(), guid: Some(e.guid().clone()) })
    });
    let mut doc = crate::document::Document::from_loaded(elements, next)?;
    let levels = crate::levels::list(doc.store());
    let level = levels.first().ok_or_else(|| Error::new("legend.no_level", "the model has no level"))?;
    let (level_id, z0) = (level.id, level.elevation);
    let type_id = t.id();
    let width = crate::inserts::insert_type(t).map(|it| it.width);
    let mut txn = doc.transaction("Legend sample", crate::origin::Origin::ui());
    let wall = |txn: &mut Transaction<'_>, length: f64, wall_type: Option<ElementId>| -> Result<ElementId> {
        let mut p = json!({
            "curve": { "line": { "start": [0.0, 0.0], "end": [length, 0.0] } },
            "base_level": level_id,
            "top_constraint": { "unconnected": 3.0 },
        });
        if let Some(w) = wall_type {
            p["type_id"] = json!(w);
        }
        crate::walls::create(txn, parse(p)?)
    };
    let (ids, size) = match kind {
        TypeKind::Wall => {
            let l = host_length.unwrap_or(2.0);
            (vec![wall(&mut txn, l, Some(type_id))?], [l, 0.0])
        }
        TypeKind::Floor => {
            let l = host_length.unwrap_or(2.0);
            let rect = |a: [f64; 2], b: [f64; 2]| json!({ "line": { "start": a, "end": b } });
            let p = json!({
                "type_id": type_id,
                "loops": [[rect([0.0, 0.0], [l, 0.0]), rect([l, 0.0], [l, 1.0]), rect([l, 1.0], [0.0, 1.0]), rect([0.0, 1.0], [0.0, 0.0])]],
                "level_id": level_id,
            });
            (vec![crate::floors::create(&mut txn, parse(p)?)?], [l, 1.0])
        }
        TypeKind::Door | TypeKind::Window => {
            let l = host_length.unwrap_or(width.unwrap_or(0.9) + 1.2);
            let host = wall(&mut txn, l, None)?;
            let p = json!({ "type_id": type_id, "host_wall_id": host, "point_along": l / 2.0 });
            let insert_kind = if kind == TypeKind::Door { crate::inserts::Kind::Door } else { crate::inserts::Kind::Window };
            let insert = crate::inserts::place(&mut txn, insert_kind, parse(p)?)?;
            (vec![host, insert], [l, 0.0])
        }
        TypeKind::Component => {
            let p = json!({ "type_id": type_id, "point": [0.0, 0.0], "level": level_id });
            (vec![crate::mesh_objects::place(&mut txn, parse(p)?)?], [0.0, 0.0])
        }
    };
    txn.commit();
    let scratch = doc.store().clone();
    let mut bodies: Vec<crate::bodies::Body> = crate::bodies::all(&scratch).into_iter().filter(|b| ids.contains(&b.element)).collect();
    for b in &mut bodies {
        for p in &mut b.parts {
            for q in &mut p.mesh.0 {
                q[2] -= z0;
            }
        }
    }
    // Walls have a thickness; components their extent.
    let size = match bodies.iter().filter_map(crate::bodies::Body::bounds).reduce(|a, b| ([0, 1, 2].map(|k| a.0[k].min(b.0[k])), [0, 1, 2].map(|k| a.1[k].max(b.1[k])))) {
        Some((lo, hi)) if size[0] == 0.0 => [hi[0] - lo[0], hi[1] - lo[1]],
        Some((lo, hi)) => [size[0], hi[1] - lo[1]],
        None => size,
    };
    Ok(Sample { kind, bodies, store: scratch, size })
}

/// A sample element's arguments, from JSON.
fn parse<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T> {
    serde_json::from_value(v).map_err(|e| Error::new("legend.sample", e.to_string()))
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateLegendParams {
    /// Default "Legend" (numbered when taken).
    pub name: Option<String>,
    /// The scale denominator (default 50).
    pub scale: Option<f64>,
}

/// `view.create_legend`.
pub(crate) fn create_legend(txn: &mut Transaction<'_>, p: CreateLegendParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let base = p.name.unwrap_or_else(|| "Legend".into());
    let taken: Vec<String> = store.iter().filter(|e| crate::views::is_view(e)).filter_map(|e| e.name.clone()).collect();
    let name = std::iter::once(base.clone()).chain((2..).map(|n| format!("{base} {n}"))).find(|n| !taken.contains(n)).expect("a free name");
    let scale = p.scale.unwrap_or(50.0);
    if !(1.0..=10000.0).contains(&scale) {
        return Err(Error::new("view.bad_scale", "the scale is 1 : 1 to 1 : 10000"));
    }
    let data = ViewData {
        kind: KIND_LEGEND.into(),
        camera: None,
        home: None,
        level: None,
        view_range: None,
        section_box: None,
        slice_planes: Vec::new(),
        section: None,
        color_scheme: None,
        area_scheme: None,
        detail_level: None,
    };
    let mut value = serde_json::to_value(&data).expect("view data serialises");
    value["scale"] = scale.into();
    let guid = GlobalId::new();
    txn.create(NewElement::new(format!("{CLASS_PREFIX}{}", guid.as_str())).guid(guid).name(name).param(VIEW_KEY, value))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceComponentParams {
    /// A legend.
    pub view_id: ElementRef,
    /// A wall, floor, door, window or component type.
    pub type_id: ElementRef,
    /// `plan` (default), `front`, `back` or `section`.
    pub direction: Option<Direction>,
    /// The sample's length (walls and floors, and the wall a door or window sits in).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub host_length: Option<f64>,
    /// Where the drawing's lower-left corner goes, in the legend (metres at full size).
    pub point: [f64; 2],
}

/// `legend.place_component`.
pub(crate) fn place_component(txn: &mut Transaction<'_>, p: PlaceComponentParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let view = store
        .resolve(&p.view_id)
        .and_then(|id| store.get(id))
        .filter(|e| is_legend(e))
        .ok_or_else(|| Error::new("legend.not_legend", "legend components go in legend views (view.create_legend)"))?;
    let t = store.resolve(&p.type_id).and_then(|id| store.get(id)).ok_or_else(|| Error::new("element.not_found", "no such type"))?;
    if type_kind(t).is_none() {
        return Err(Error::new("legend.unsupported_type", format!("a legend cannot draw {} yet (walls, floors, doors, windows and components)", t.class)));
    }
    if p.host_length.is_some_and(|l| !(0.1..=100.0).contains(&l)) {
        return Err(Error::new("legend.bad_length", "the host length is 0.1 to 100 m"));
    }
    let direction = p.direction.unwrap_or_default();
    if direction == Direction::Section && type_kind(t) == Some(TypeKind::Component) {
        return Err(Error::new("legend.bad_direction", "a component type is drawn in plan, front or back"));
    }
    let view_guid = view.guid().to_string();
    let data = ComponentData {
        kind: KIND_COMPONENT.into(),
        view: view_guid.clone(),
        type_guid: t.guid().to_string(),
        direction,
        host_length: p.host_length,
        point: p.point,
    };
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{ANNO_PREFIX}{view_guid}/{}", guid.as_str()))
            .guid(guid)
            .param(ANNO_KEY, serde_json::to_value(&data).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetComponentParams {
    pub component_ids: Vec<ElementRef>,
    pub direction: Option<Direction>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub host_length: Option<f64>,
    /// Moves it.
    pub point: Option<[f64; 2]>,
}

/// `legend.set_component`.
pub(crate) fn set_component(txn: &mut Transaction<'_>, p: SetComponentParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.component_ids {
        let id = store.resolve(r).ok_or_else(|| Error::new("legend.not_found", "no such legend component"))?;
        let mut d = component_data(store.get(id).expect("resolved")).ok_or_else(|| Error::new("legend.not_found", format!("element {id} is not a legend component")))?;
        if let Some(v) = p.direction {
            d.direction = v;
        }
        if let Some(v) = p.host_length {
            if !(0.1..=100.0).contains(&v) {
                return Err(Error::new("legend.bad_length", "the host length is 0.1 to 100 m"));
            }
            d.host_length = Some(v);
        }
        if let Some(v) = p.point {
            d.point = v;
        }
        let value = serde_json::to_value(&d).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(ANNO_KEY.into(), value);
        })?;
    }
    Ok(())
}

/// Types a legend can draw, for the Legend Component tool: id, name and kind.
pub fn drawable_types(store: &Store) -> Vec<(ElementId, String, TypeKind)> {
    let mut out: Vec<(ElementId, String, TypeKind)> =
        store.iter().filter_map(|e| Some((e.id(), e.name.clone().unwrap_or_default(), type_kind(e)?))).collect();
    out.sort_by_key(|a| (a.2 as u8, a.1.to_lowercase()));
    out
}
