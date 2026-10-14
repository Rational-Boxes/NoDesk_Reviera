// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Boolean geometry between elements (ARC-MOD-028): a *target* element's body is replaced by
//! its subtraction, intersection or union with *tool* elements' bodies, and re-made whenever
//! either changes. The relations are held on the project (`rxd:csg`) by GlobalId. The
//! application applies them with the solid kernel (`reviera-geom`) wherever bodies are built,
//! so the 3D view and the overlap engine both see the result.
//!
//! A union fuses the tool into the target, so the tool's own body is no longer drawn; a
//! subtraction or intersection keeps the tool unless `tools: hide` is asked for.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;

pub const KEY: &str = "rxd:csg";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CsgOp {
    Subtract,
    Intersect,
    Union,
}

/// One boolean relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Relation {
    /// GlobalIds.
    pub target: String,
    pub tool: String,
    pub op: CsgOp,
    /// The tool's own body is not drawn (always for a union).
    #[serde(default)]
    pub hide_tool: bool,
}

fn project(store: &Store) -> Option<&crate::element::Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// The relations, in the order they apply.
pub fn relations(store: &Store) -> Vec<Relation> {
    project(store)
        .and_then(|p| p.param(KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// The relations whose elements both exist: target, tool, operation and whether the tool hides.
pub fn resolved(store: &Store) -> Vec<(ElementId, ElementId, CsgOp, bool)> {
    let id = |g: &str| GlobalId::parse(g).and_then(|g| store.find_by_guid(&g));
    relations(store)
        .iter()
        .filter_map(|r| Some((id(&r.target)?, id(&r.tool)?, r.op, r.hide_tool)))
        .collect()
}

/// Whether two elements are in a boolean relation.
pub fn related(store: &Store, a: ElementId, b: ElementId) -> bool {
    resolved(store)
        .iter()
        .any(|(t, u, _, _)| (*t == a && *u == b) || (*t == b && *u == a))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tools {
    #[default]
    Keep,
    Hide,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BooleanParams {
    pub target_id: ElementRef,
    pub tool_ids: Vec<ElementRef>,
    pub op: CsgOp,
    /// Whether the tools' own bodies still show (a union always hides them).
    #[serde(default)]
    pub tools: Tools,
}

fn write(txn: &mut crate::txn::Transaction<'_>, list: &[Relation]) -> Result<()> {
    let project = project(txn.store())
        .map(crate::element::Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let value = serde_json::to_value(list).expect("serialises");
    txn.modify(project, |e| {
        e.params.insert(KEY.into(), value);
    })
}

/// Adds boolean relations (`geometry.boolean`); a pair already related is replaced.
pub(crate) fn boolean(txn: &mut crate::txn::Transaction<'_>, p: BooleanParams) -> Result<()> {
    let store = txn.store().clone();
    let guid = |r: &ElementRef| {
        store
            .resolve(r)
            .and_then(|id| store.get(id))
            .map(|e| e.guid().to_string())
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
    };
    let target = guid(&p.target_id)?;
    if p.tool_ids.is_empty() {
        return Err(Error::new("csg.no_tools", "give at least one tool"));
    }
    let mut list = relations(&store);
    for t in &p.tool_ids {
        let tool = guid(t)?;
        if tool == target {
            return Err(Error::new("csg.self", "an element cannot be its own tool"));
        }
        // A cycle (the tool already shaped by the target) would never settle.
        if list.iter().any(|r| r.target == tool && r.tool == target) {
            return Err(Error::new(
                "csg.cycle",
                "the tool is already shaped by the target; remove that boolean first",
            ));
        }
        list.retain(|r| !(r.target == target && r.tool == tool));
        list.push(Relation {
            target: target.clone(),
            tool,
            op: p.op,
            hide_tool: p.op == CsgOp::Union || p.tools == Tools::Hide,
        });
    }
    write(txn, &list)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveParams {
    pub target_id: ElementRef,
    /// Only these tools (default all of the target's).
    pub tool_ids: Option<Vec<ElementRef>>,
}

/// Removes boolean relations (`geometry.remove_boolean`): the target's own body returns.
pub(crate) fn remove(txn: &mut crate::txn::Transaction<'_>, p: RemoveParams) -> Result<()> {
    let store = txn.store().clone();
    let guid = |r: &ElementRef| {
        store
            .resolve(r)
            .and_then(|id| store.get(id))
            .map(|e| e.guid().to_string())
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
    };
    let target = guid(&p.target_id)?;
    let tools: Option<Vec<String>> = p
        .tool_ids
        .as_ref()
        .map(|ts| ts.iter().map(guid).collect::<Result<_>>())
        .transpose()?;
    let mut list = relations(&store);
    let before = list.len();
    list.retain(|r| !(r.target == target && tools.as_ref().is_none_or(|ts| ts.contains(&r.tool))));
    if list.len() == before {
        return Err(Error::new("csg.none", "the element has no such boolean"));
    }
    write(txn, &list)
}
