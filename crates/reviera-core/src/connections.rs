// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Structural connections (STR-CONN-001): generic connections, the information carriers placed
//! at a join of two or more framing members (beams, braces, truss members, columns).
//!
//! A connection is an `IfcElementAssembly` (`USERDEFINED`, ObjectType `STRUCTURAL_CONNECTION`)
//! with a `recipe.structural_connection`: its members (GlobalIds, the main member first), approval
//! status, code-check status, fabricator and instance-override flag. It has no body; plans draw it
//! as a circle with a spoke toward each member. Its place is the join, found from its members'
//! axes, so it follows them; it goes when any of its members goes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const RECIPE: &str = "recipe.structural_connection";
/// Members whose axes pass farther apart than this do not meet.
const JOIN_TOLERANCE: f64 = 0.5;

/// A connection's code-check status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodeCheck {
    #[default]
    NotChecked,
    Passed,
    Failed,
}

/// The stored recipe of a connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectionRecipe {
    pub kind: String,
    /// Its members (GlobalIds), the main member first.
    pub members: Vec<String>,
    /// The connection type: `generic` (detailed types come with the connection library).
    #[serde(default = "generic")]
    pub connection_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_status: Option<String>,
    #[serde(default)]
    pub code_check: CodeCheck,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fabricator: Option<String>,
    #[serde(default)]
    pub override_by_instance: bool,
    /// This connection's own parameter values (STR-STL-002).
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub params: serde_json::Map<String, serde_json::Value>,
    /// Design forces per load case or combination (STR-STL-009).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub design_forces: Vec<crate::connection_check::DesignForce>,
    /// The last code check (STR-STL-009).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<crate::connection_check::CheckRecord>,
}

fn generic() -> String {
    "generic".into()
}

/// A connection recipe with no members (to read a type's parameters).
pub fn blank() -> ConnectionRecipe {
    ConnectionRecipe {
        kind: RECIPE.into(),
        members: Vec::new(),
        connection_type: generic(),
        approval_status: None,
        code_check: CodeCheck::NotChecked,
        fabricator: None,
        override_by_instance: false,
        params: serde_json::Map::new(),
        design_forces: Vec::new(),
        check: None,
    }
}

/// A lone column's foot (where a base plate goes).
fn column_foot(store: &Store, id: ElementId) -> Option<V3> {
    let e = store.get(id)?;
    crate::columns::column_recipe(e)?;
    let (a, b) = crate::columns::regenerate(store, e).ok()?.axis();
    Some(if a[2] <= b[2] { a } else { b })
}

pub fn recipe(e: &Element) -> Option<ConnectionRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

type V3 = [f64; 3];

/// A framing member's axis: a beam's, brace's or truss member's location line, a column's axis.
pub fn member_axis(store: &Store, id: ElementId) -> Option<(V3, V3)> {
    let e = store.get(id)?;
    if crate::columns::column_recipe(e).is_some() {
        return crate::columns::regenerate(store, e).ok().map(|g| g.axis());
    }
    crate::beams::beam_recipe(e)?;
    let g = crate::beams::regenerate(store, e).ok()?;
    Some((g.a, g.b))
}

fn dist_to_segment(p: V3, (a, b): (V3, V3)) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l2 = ab.iter().map(|x| x * x).sum::<f64>();
    let t = if l2 < 1e-18 {
        0.0
    } else {
        (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2]) / l2)
            .clamp(0.0, 1.0)
    };
    let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
}

/// Where members meet: the end of one of their axes that lies nearest all the others (the
/// farthest of them within the join tolerance), or `None` when they do not meet.
pub fn join_point(axes: &[(V3, V3)]) -> Option<V3> {
    if axes.len() < 2 {
        return None;
    }
    let mut best: Option<(f64, V3)> = None;
    for (i, ax) in axes.iter().enumerate() {
        for p in [ax.0, ax.1] {
            let worst = axes
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, o)| dist_to_segment(p, *o))
                .fold(0.0, f64::max);
            if best.is_none_or(|b| worst < b.0) {
                best = Some((worst, p));
            }
        }
    }
    best.filter(|b| b.0 <= JOIN_TOLERANCE).map(|b| b.1)
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// A connection as it is now: its join point and, for each member, the direction from the join
/// along it (in plan, for the symbol's spokes).
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionGeometry {
    pub point: V3,
    pub members: Vec<ElementId>,
    pub spokes: Vec<[f64; 2]>,
}

pub fn regenerate(store: &Store, e: &Element) -> Result<ConnectionGeometry> {
    let r = recipe(e)
        .ok_or_else(|| Error::new("connection.not_reviera", "not a Reviera connection"))?;
    let members: Vec<ElementId> = r.members.iter().filter_map(|g| find(store, g)).collect();
    let axes: Vec<(V3, V3)> = members
        .iter()
        .filter_map(|m| member_axis(store, *m))
        .collect();
    let point = match members.as_slice() {
        [one] => column_foot(store, *one),
        _ => join_point(&axes),
    }
    .ok_or_else(|| {
        Error::new(
            "connection.no_join",
            "the connection's members no longer meet",
        )
    })?;
    let spokes = axes
        .iter()
        .map(|(a, b)| {
            // Toward the far end of the member.
            let far = if dist2(*a, point) > dist2(*b, point) {
                a
            } else {
                b
            };
            let d = [far[0] - point[0], far[1] - point[1]];
            let l = d[0].hypot(d[1]);
            if l < 1e-9 {
                [0.0, 0.0]
            } else {
                [d[0] / l, d[1] / l]
            }
        })
        .collect();
    Ok(ConnectionGeometry {
        point,
        members,
        spokes,
    })
}

fn dist2(a: V3, b: V3) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Every connection that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, ConnectionGeometry)> {
    store
        .iter()
        .filter(|e| recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

/// Whether a connection depends on `guid` (it goes with any of its members).
pub fn uses(e: &Element, guid: &str) -> bool {
    recipe(e).is_some_and(|r| r.members.iter().any(|m| m == guid))
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// Two or more beams, braces, truss members or columns that meet; the first is the main
    /// member.
    pub member_ids: Vec<ElementRef>,
    /// The connection type: `generic` (the default) or one of `connection_type.list` (a base
    /// plate takes one column).
    pub type_id: Option<String>,
}

/// Places a generic connection at the join of members (`structural_connection.create`).
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    if let Some(t) = p.type_id.as_deref().filter(|t| t.starts_with(crate::custom_connections::PREFIX)) {
        return crate::custom_connections::place(txn, &p.member_ids, t);
    }
    let store = txn.store().clone();
    let ty = match p.type_id.as_deref() {
        None | Some("generic") => None,
        Some(t) => Some(crate::steel_fab::connection_type(t).ok_or_else(|| {
            Error::new("connection.unknown_type", format!("no connection type {t}"))
        })?),
    };
    let mut ids = Vec::new();
    for r in &p.member_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        if member_axis(&store, id).is_none() {
            return Err(Error::new(
                "connection.not_a_member",
                format!("element {id} is not a beam, brace, truss member or column"),
            ));
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let single = ty.as_ref().is_some_and(|t| t.inputs.len() == 1);
    if ids.len() < 2 && !single {
        return Err(Error::new(
            "connection.members",
            "a connection joins two or more members",
        ));
    }
    // One connection of a type per joint: the same type on the same members is refused.
    if let Some(t) = &ty {
        let mut want: Vec<String> = ids
            .iter()
            .filter_map(|m| store.get(*m).map(|e| e.guid().to_string()))
            .collect();
        want.sort();
        let exists = store.iter().filter_map(recipe).any(|c| {
            let mut have = c.members.clone();
            have.sort();
            c.connection_type == t.id && have == want
        });
        if exists {
            return Err(Error::new(
                "connection.exists",
                format!("these members already have a {} connection", t.name),
            ));
        }
    }
    let axes: Vec<(V3, V3)> = ids.iter().filter_map(|m| member_axis(&store, *m)).collect();
    if !single && join_point(&axes).is_none() {
        return Err(Error::new(
            "connection.no_join",
            "the members do not meet (their axes pass more than 500 mm apart)",
        ));
    }
    let recipe = ConnectionRecipe {
        kind: RECIPE.into(),
        members: ids
            .iter()
            .map(|i| store.get(*i).expect("resolved").guid().to_string())
            .collect(),
        connection_type: generic(),
        approval_status: None,
        code_check: CodeCheck::NotChecked,
        fabricator: None,
        override_by_instance: false,
        params: serde_json::Map::new(),
        design_forces: Vec::new(),
        check: None,
    };
    if let Some(t) = &ty {
        crate::steel_fab::check_inputs(&store, t, &recipe.members)?;
    }
    let recipe = ConnectionRecipe {
        connection_type: ty.as_ref().map_or_else(generic, |t| t.id.to_owned()),
        ..recipe
    };
    let id = txn.create(
        NewElement::new("IfcElementAssembly")
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", "STRUCTURAL_CONNECTION")
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    if ty.is_some() {
        crate::steel_fab::relayout(txn, id)?;
    }
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub connection_ids: Vec<ElementRef>,
    /// Empty clears it.
    pub approval_status: Option<String>,
    pub code_check: Option<CodeCheck>,
    pub fabricator: Option<String>,
    pub override_by_instance: Option<bool>,
}

/// Sets connections' information (`structural_connection.set`).
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.connection_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let mut c = store.get(id).and_then(recipe).ok_or_else(|| {
            Error::new(
                "connection.not_a_connection",
                format!("element {id} is not a connection"),
            )
        })?;
        let text = |v: &Option<String>| {
            v.as_ref()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        };
        if p.approval_status.is_some() {
            c.approval_status = text(&p.approval_status);
        }
        if p.fabricator.is_some() {
            c.fabricator = text(&p.fabricator);
        }
        if let Some(v) = p.code_check {
            c.code_check = v;
        }
        if let Some(v) = p.override_by_instance {
            c.override_by_instance = v;
        }
        let v = serde_json::to_value(&c).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {
    pub connection_ids: Vec<ElementRef>,
}

/// A connection as reported (`structural_connection.info`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Info {
    pub connection_id: ElementId,
    pub connection_type: String,
    /// Its join point (metres).
    pub point: Option<[f64; 3]>,
    /// Its members, the main member first.
    pub members: Vec<ElementId>,
    pub approval_status: Option<String>,
    pub code_check: CodeCheck,
    pub fabricator: Option<String>,
    pub override_by_instance: bool,
    /// The parameters in effect (a typed connection's).
    pub params: std::collections::BTreeMap<String, f64>,
    /// Its fabrication elements: plates, bolts, anchor rods, welds.
    pub plates: Vec<ElementId>,
    pub bolts: Vec<ElementId>,
    pub welds: Vec<ElementId>,
}

/// Reports connections (`structural_connection.info`).
pub fn info(store: &Store, p: &InfoParams) -> Result<Vec<Info>> {
    p.connection_ids
        .iter()
        .map(|r| {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let e = store.get(id).expect("resolved");
            let c = recipe(e).ok_or_else(|| {
                Error::new(
                    "connection.not_a_connection",
                    format!("element {id} is not a connection"),
                )
            })?;
            let g = regenerate(store, e).ok();
            let guid = e.guid().to_string();
            let fab = |class: &str| -> Vec<ElementId> {
                store
                    .iter()
                    .filter(|f| {
                        f.class == class
                            && crate::steel_fab::fab_recipe(f).is_some_and(|r| r.connection == guid)
                    })
                    .map(Element::id)
                    .collect()
            };
            Ok(Info {
                params: crate::steel_fab::params(store, &c),
                plates: fab("IfcPlate"),
                bolts: fab("IfcMechanicalFastener"),
                welds: fab("IfcFastener"),
                connection_id: id,
                connection_type: c.connection_type,
                point: g.as_ref().map(|g| g.point),
                members: c.members.iter().filter_map(|m| find(store, m)).collect(),
                approval_status: c.approval_status,
                code_check: c.code_check,
                fabricator: c.fabricator,
                override_by_instance: c.override_by_instance,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins() {
        let beam = ([0.0, 0.0, 4.0], [6.0, 0.0, 4.0]);
        let column = ([0.0, 0.0, 0.0], [0.0, 0.0, 4.0]);
        assert_eq!(join_point(&[beam, column]), Some([0.0, 0.0, 4.0]));
        let far = ([0.0, 3.0, 0.0], [0.0, 3.0, 4.0]);
        assert_eq!(join_point(&[beam, far]), None);
    }
}
