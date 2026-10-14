// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `overlap.find`: the volume overlap engine (`reviera_core::overlap`) over the open model:
//! - Reviera's own elements as they regenerate (`bodies`);
//! - elements known only from the IFC file, as tessellated for the 3D view.

use reviera_core::bodies::{Body, Part};
use reviera_core::overlap::{self, Options};
use reviera_core::{
    Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Result,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindParams {
    /// The elements to check (default every element with a body).
    pub element_ids: Option<Vec<ElementRef>>,
    /// Check them only against these (default against each other and everything else).
    pub against: Option<Vec<ElementRef>>,
    /// Overlaps thinner than this are contact (default 1 mm).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub tolerance: Option<f64>,
    /// Smallest overlap reported, cubic metres (default 1 cm³).
    pub min_volume: Option<f64>,
    /// Measuring lines across each overlap, each way (default 48; more is finer and slower).
    pub resolution: Option<usize>,
    /// Include pairs that belong together by design (a window and its wall, a dormer's parts and
    /// their roof, a gate and its railing); default false.
    #[serde(default)]
    pub include_related: bool,
    /// At most this many results, largest first (default 500).
    pub limit: Option<usize>,
}

/// One overlap, with its elements described.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Found {
    #[serde(flatten)]
    pub overlap: overlap::Overlap,
    pub a_label: String,
    pub b_label: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FindResult {
    pub overlaps: Vec<Found>,
    /// Bodies checked.
    pub bodies: usize,
}

impl App {
    /// Every element's body in the active document: regenerated, then tessellated from the file.
    pub fn bodies(&self) -> Result<Vec<Body>> {
        let id = self
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let open = self.get(id)?;
        let store = open.doc.store();
        let mut out = reviera_core::bodies::all(store);
        for m in open.tessellation()?.iter() {
            let Some(e) = m.element else { continue };
            if store.get(e).is_none_or(crate::regenerated) || m.indices.is_empty() {
                continue;
            }
            let part = Part {
                mesh: (
                    m.positions.iter().map(|p| p.map(f64::from)).collect(),
                    m.indices.clone(),
                ),
                class: m.class.clone(),
                plan_hidden: false,
            };
            match out.iter_mut().find(|b| b.element == e) {
                Some(b) => b.parts.push(part),
                None => out.push(Body {
                    element: e,
                    parts: vec![part],
                }),
            }
        }
        Ok(out)
    }
}

pub(crate) struct OverlapFind;

impl HostCommand for OverlapFind {
    type Host = App;
    type Params = Option<FindParams>;
    type Output = FindResult;
    const META: CommandMeta = CommandMeta {
        name: "overlap.find",
        version: 1,
        summary: "Find overlapping elements",
        description: "Finds where elements' bodies overlap (ARC-MOD-030): each pair once, largest first, with \
                      the overlap's volume, its depth (the longest vertical run through both), its box and its \
                      centroid. Touching faces are not overlaps. `element_ids` limits the check, `against` checks \
                      only against those; pairs that belong together by design (a window and its wall, a dormer \
                      and its roof) are left out unless `include_related`. Covers Reviera's own elements and \
                      those known only from the IFC file.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<FindParams>) -> Result<FindResult> {
        let p = p.unwrap_or_default();
        let bodies = app.bodies()?;
        let doc = app.active().expect("bodies checked");
        let store = app.get(doc)?.doc.store();
        let resolve = |refs: &[ElementRef]| -> Result<Vec<ElementId>> {
            refs.iter()
                .map(|r| {
                    store
                        .resolve(r)
                        .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
                })
                .collect()
        };
        let chosen = p.element_ids.as_deref().map(resolve).transpose()?;
        let other = p.against.as_deref().map(resolve).transpose()?;
        let pick = |ids: &Option<Vec<ElementId>>| -> Vec<Body> {
            bodies
                .iter()
                .filter(|b| ids.as_ref().is_none_or(|ids| ids.contains(&b.element)))
                .cloned()
                .collect()
        };
        let first = pick(&chosen);
        let second = other.as_ref().map(|_| pick(&other));
        // Checking chosen elements without `against`: against everything.
        let second = match (&chosen, second) {
            (Some(_), None) => Some(bodies.clone()),
            (_, s) => s,
        };
        let o = Options {
            tolerance: p.tolerance.unwrap_or(0.001),
            min_volume: p.min_volume.unwrap_or(1e-6),
            resolution: p.resolution.unwrap_or(48).clamp(4, 1024),
        };
        let skip = |a: ElementId, b: ElementId| {
            !p.include_related && reviera_core::bodies::related(store, a, b)
        };
        let mut found = overlap::find(&first, second.as_deref(), &skip, &o);
        found.truncate(p.limit.unwrap_or(500));
        let label = |id: ElementId| {
            store.get(id).map_or_else(
                || format!("#{}", id.0),
                |e| match &e.name {
                    Some(n) => format!("{} {} ({n})", e.class, id.0),
                    None => format!("{} {}", e.class, id.0),
                },
            )
        };
        Ok(FindResult {
            bodies: first.len(),
            overlaps: found
                .into_iter()
                .map(|o| Found {
                    a_label: label(o.a),
                    b_label: label(o.b),
                    overlap: o,
                })
                .collect(),
        })
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// Check these elements' virtual volumes (default all).
    pub element_ids: Option<Vec<ElementRef>>,
    /// Intrusions thinner than this are contact (default 1 mm).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub tolerance: Option<f64>,
}

/// Something intruding into a virtual volume.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Intrusion {
    /// The element whose clearance it is, and which.
    pub element: ElementId,
    pub kind: reviera_core::clearances::ClearanceKind,
    pub rule: String,
    /// What intrudes: an element's body, or another door's swing.
    pub other: ElementId,
    pub other_is_swing: bool,
    pub label: String,
    pub volume: f64,
    pub depth: f64,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub centroid: [f64; 3],
}

pub(crate) struct ClearanceCheck;

impl HostCommand for ClearanceCheck {
    type Host = App;
    type Params = Option<CheckParams>;
    type Output = Vec<Intrusion>;
    const META: CommandMeta = CommandMeta {
        name: "clearance.check",
        version: 1,
        summary: "Check virtual volumes for intrusions",
        description: "Checks each virtual volume (ARC-MOD-031: door swings, and the accessibility rule set's \
                      maneuvering clearances) against every element's body, and door swings against each \
                      other. A door's own body and its wall are allowed. Returns each intrusion with its volume, \
                      depth, box and centroid, largest first.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<CheckParams>) -> Result<Vec<Intrusion>> {
        let p = p.unwrap_or_default();
        let bodies = app.bodies()?;
        let doc = app.active().expect("bodies checked");
        let store = app.get(doc)?.doc.store();
        let only: Option<Vec<ElementId>> = p
            .element_ids
            .as_deref()
            .map(|refs| {
                refs.iter()
                    .map(|r| {
                        store.resolve(r).ok_or_else(|| {
                            Error::new("element.not_found", format!("no element {r:?}"))
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let o = Options {
            tolerance: p.tolerance.unwrap_or(0.001),
            ..Options::default()
        };
        let clearances: Vec<_> = reviera_core::clearances::all(store)
            .into_iter()
            .filter(|c| only.as_ref().is_none_or(|ids| ids.contains(&c.element)))
            .collect();
        let label = |id: ElementId| {
            store
                .get(id)
                .map_or_else(|| format!("#{}", id.0), |e| format!("{} {}", e.class, id.0))
        };
        let skip = |a: ElementId, b: ElementId| reviera_core::bodies::related(store, a, b);
        let mut out = Vec::new();
        for c in &clearances {
            for ov in overlap::find(&[c.body()], Some(&bodies), &skip, &o) {
                let other = if ov.a == c.element { ov.b } else { ov.a };
                out.push(Intrusion {
                    element: c.element,
                    kind: c.kind,
                    rule: c.rule.to_owned(),
                    other,
                    other_is_swing: false,
                    label: label(other),
                    volume: ov.volume,
                    depth: ov.depth,
                    min: ov.min,
                    max: ov.max,
                    centroid: ov.centroid,
                });
            }
        }
        // Doors that would hit each other.
        let swings: Vec<_> = reviera_core::clearances::all(store)
            .into_iter()
            .filter(|c| c.kind == reviera_core::clearances::ClearanceKind::DoorSwing)
            .collect();
        for (i, a) in swings.iter().enumerate() {
            for b in &swings[i + 1..] {
                if only
                    .as_ref()
                    .is_some_and(|ids| !ids.contains(&a.element) && !ids.contains(&b.element))
                {
                    continue;
                }
                if let Some(ov) = overlap::between(&a.body(), &b.body(), &o) {
                    out.push(Intrusion {
                        element: a.element,
                        kind: a.kind,
                        rule: String::new(),
                        other: b.element,
                        other_is_swing: true,
                        label: format!("{}'s swing", label(b.element)),
                        volume: ov.volume,
                        depth: ov.depth,
                        min: ov.min,
                        max: ov.max,
                        centroid: ov.centroid,
                    });
                }
            }
        }
        out.sort_by(|x, y| y.volume.total_cmp(&x.volume));
        Ok(out)
    }
}
