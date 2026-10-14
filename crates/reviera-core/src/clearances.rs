// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Virtual volumes (ARC-MOD-031): spaces elements need kept clear. They take part in overlap
//! checks ([`crate::overlap`]) but are never built, scheduled or exported as physical bodies,
//! and views show them hidden, translucent or solid.
//!
//! Built so far:
//! - **Door swings**: each door leaf's swept volume (a quarter circle of the leaf's width about
//!   its hinge, on the swing side), from the sill to the leaf's top. Always present.
//! - **Door maneuvering clearances** under the project's accessibility rule set. ADA 2010
//!   404.2.4.1 (front approach) gives:
//!   - on the pull (swing) side, 1525 mm (60 in) deep and 455 mm (18 in) beyond the latch;
//!   - on the push side, 1220 mm (48 in) deep, the door's clear width.
//!
//!   Both are floor spaces taken to 2030 mm (80 in) high.
//!
//! The rule set is held on the project (`rxd:accessibility`). Fixture clear floor spaces,
//! turning spaces and element-defined service clearances come with components and accessible
//! rooms.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bodies::{Body, Mesh, Part, extrude};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::inserts::{Kind, Placed};
use crate::store::Store;

/// The parameter holding the project's accessibility rule set.
pub const RULES_KEY: &str = "rxd:accessibility";
/// The parameter holding how a view shows virtual volumes.
pub const VIEW_KEY: &str = "rxd:virtual_volumes";

/// An accessibility rule set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleSet {
    /// Door swings only.
    #[default]
    None,
    /// The 2010 ADA Standards for Accessible Design.
    #[serde(rename = "ada_2010")]
    Ada2010,
}

/// What a virtual volume is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClearanceKind {
    DoorSwing,
    /// The maneuvering clearance on a door's pull (swing) side.
    PullSide,
    /// The maneuvering clearance on a door's push side.
    PushSide,
}

impl ClearanceKind {
    /// The render class of its volume (a virtual volume, not cut in plans).
    pub fn class(self) -> &'static str {
        match self {
            Self::DoorSwing => "Clearance:door_swing/3d",
            Self::PullSide => "Clearance:pull_side/3d",
            Self::PushSide => "Clearance:push_side/3d",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DoorSwing => "door swing",
            Self::PullSide => "pull-side clearance",
            Self::PushSide => "push-side clearance",
        }
    }
}

/// A virtual volume.
#[derive(Debug, Clone, PartialEq)]
pub struct Clearance {
    /// The element it belongs to.
    pub element: ElementId,
    pub kind: ClearanceKind,
    /// The rule it comes from (empty for a door swing).
    pub rule: &'static str,
    pub mesh: Mesh,
}

impl Clearance {
    /// As a body, for the overlap engine.
    pub fn body(&self) -> Body {
        Body {
            element: self.element,
            parts: vec![Part {
                mesh: self.mesh.clone(),
                class: self.kind.class().to_owned(),
                plan_hidden: true,
            }],
        }
    }
}

/// The project's accessibility rule set.
pub fn rule_set(store: &Store) -> RuleSet {
    store
        .iter()
        .find(|e| e.class == "IfcProject")
        .and_then(|e| e.param(RULES_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// A plan polygon in a door's local frame (x along the wall, y toward its facing side) extruded
/// from `z0` to `z1` above its bottom, in project coordinates.
fn local_prism(p: &Placed, poly: &[[f64; 2]], z0: f64, z1: f64) -> Mesh {
    let world: Vec<[f64; 2]> = poly
        .iter()
        .map(|[x, y]| {
            let w = p.world([*x, *y, 0.0]);
            [w[0], w[1]]
        })
        .collect();
    extrude(&world, &[], p.z0 + z0, p.z0 + z1)
}

/// The virtual volumes of one door.
pub fn of_door(p: &Placed, rules: RuleSet) -> Vec<Clearance> {
    if p.kind != Kind::Door {
        return Vec::new();
    }
    let t = &p.ty;
    let r = t.width - 2.0 * t.frame_width;
    let s = p.facing();
    let face = s * p.wall.width / 2.0;
    let back = -face;
    let side = if p.recipe.flip_hand { 1.0 } else { -1.0 };
    let hinge = [side * (t.width / 2.0 - t.frame_width), face];
    let leaf_top = t.height - t.frame_width;
    // The swing: the hinge, then the arc from the open leaf round to the closed one.
    let steps = 24;
    let mut swing = vec![hinge];
    for k in 0..=steps {
        let a = std::f64::consts::FRAC_PI_2 * k as f64 / steps as f64;
        swing.push([hinge[0] - side * r * a.sin(), face + s * r * a.cos()]);
    }
    let mut out = vec![Clearance {
        element: p.id,
        kind: ClearanceKind::DoorSwing,
        rule: "",
        mesh: local_prism(p, &swing, 0.0, leaf_top),
    }];
    if rules == RuleSet::Ada2010 {
        let half = r / 2.0;
        let height = 2.03;
        // Pull side: the swing side, past the latch (opposite the hinge).
        let (x0, x1) = if side < 0.0 {
            (-half, half + 0.455)
        } else {
            (-half - 0.455, half)
        };
        let pull = [
            [x0, face],
            [x1, face],
            [x1, face + s * 1.525],
            [x0, face + s * 1.525],
        ];
        out.push(Clearance {
            element: p.id,
            kind: ClearanceKind::PullSide,
            rule: "ADA 2010 404.2.4.1",
            mesh: local_prism(p, &pull, 0.0, height),
        });
        let push = [
            [-half, back],
            [half, back],
            [half, back - s * 1.22],
            [-half, back - s * 1.22],
        ];
        out.push(Clearance {
            element: p.id,
            kind: ClearanceKind::PushSide,
            rule: "ADA 2010 404.2.4.1",
            mesh: local_prism(p, &push, 0.0, height),
        });
    }
    out
}

/// Every virtual volume in the model.
pub fn all(store: &Store) -> Vec<Clearance> {
    let rules = rule_set(store);
    crate::inserts::placed(store)
        .iter()
        .flat_map(|p| of_door(p, rules))
        .collect()
}

/// How a view shows virtual volumes: `hidden`, `translucent` or `solid`.
pub fn view_mode(store: &Store, view: ElementId) -> Option<String> {
    store
        .get(view)?
        .param(VIEW_KEY)?
        .as_str()
        .map(str::to_owned)
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleSetParams {
    pub standard: RuleSet,
}

/// Sets the project's accessibility rule set (`clearance.set_rule_set`).
pub(crate) fn set_rule_set(txn: &mut crate::txn::Transaction<'_>, p: RuleSetParams) -> Result<()> {
    let project = txn
        .store()
        .iter()
        .find(|e| e.class == "IfcProject")
        .map(crate::element::Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let value = serde_json::to_value(p.standard).expect("serialises");
    txn.modify(project, |e| {
        e.params.insert(RULES_KEY.into(), value);
    })
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Hidden,
    Translucent,
    Solid,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewModeParams {
    pub view_id: ElementRef,
    pub mode: Mode,
}

/// Sets how a view shows virtual volumes (`view.set_virtual_volumes`).
pub(crate) fn set_view_mode(
    txn: &mut crate::txn::Transaction<'_>,
    p: ViewModeParams,
) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.view_id)
        .filter(|id| store.get(*id).is_some_and(crate::views::is_view))
        .ok_or_else(|| Error::new("view.not_found", "no such view"))?;
    let mode = match p.mode {
        Mode::Hidden => "hidden",
        Mode::Translucent => "translucent",
        Mode::Solid => "solid",
    };
    txn.modify(id, |e| {
        e.params.insert(VIEW_KEY.into(), json!(mode));
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ClearanceInfo {
    pub element: ElementId,
    pub kind: ClearanceKind,
    pub rule: String,
    /// Its box.
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub element_ids: Option<Vec<ElementRef>>,
}

/// The virtual volumes, or those of some elements (`clearance.list`).
pub fn list(store: &Store, p: ListParams) -> Result<Vec<ClearanceInfo>> {
    let ids: Option<Vec<ElementId>> = p
        .element_ids
        .map(|refs| {
            refs.iter()
                .map(|r| {
                    store
                        .resolve(r)
                        .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
                })
                .collect()
        })
        .transpose()?;
    Ok(all(store)
        .into_iter()
        .filter(|c| ids.as_ref().is_none_or(|ids| ids.contains(&c.element)))
        .filter_map(|c| {
            let (min, max) = c.body().bounds()?;
            Some(ClearanceInfo {
                element: c.element,
                kind: c.kind,
                rule: c.rule.to_owned(),
                min,
                max,
            })
        })
        .collect())
}
