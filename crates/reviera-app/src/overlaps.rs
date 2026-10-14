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
    /// Include overlaps accepted as intended (`overlap.accept`); default false.
    #[serde(default)]
    pub include_accepted: bool,
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
    /// Every element's body in the active document: regenerated, then tessellated from the file,
    /// with the model's booleans applied.
    pub fn bodies(&self) -> Result<Vec<Body>> {
        let id = self
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        self.bodies_of(id, |_| true)
    }

    /// A document's bodies: Reviera's own, and those tessellated from the file that `imported`
    /// asks for; the model's booleans applied.
    pub(crate) fn bodies_of(
        &self,
        id: crate::DocumentId,
        imported: impl Fn(ElementId) -> bool,
    ) -> Result<Vec<Body>> {
        let open = self.get(id)?;
        let store = open.doc.store();
        let mut out = reviera_core::bodies::all(store);
        for m in open.tessellation()?.iter() {
            let Some(e) = m.element else { continue };
            if store.get(e).is_none_or(crate::regenerated) || m.indices.is_empty() || !imported(e) {
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
        crate::csg::apply(&open.csg, store, &mut out);
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
            (!p.include_related && reviera_core::bodies::related(store, a, b))
                || (!p.include_accepted && reviera_core::overlap_accept::is_accepted(store, a, b))
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
        let skip = |a: ElementId, b: ElementId| {
            reviera_core::bodies::related(store, a, b)
                || reviera_core::overlap_accept::is_accepted(store, a, b)
        };
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

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResolveMode {
    /// Mark the overlap intended.
    Accept,
    /// Move one element out of the other along the shorter plan axis.
    MoveApart,
    CutA,
    CutB,
    Join,
    Subtract,
    Intersect,
    Union,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Which {
    A,
    #[default]
    B,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveParams {
    pub a: ElementRef,
    pub b: ElementRef,
    pub mode: ResolveMode,
    /// For `move_apart`: the element that moves (default `b`).
    #[serde(default)]
    pub moves: Which,
    /// For `accept`: why it is intended.
    pub note: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ResolveResult {
    /// For `move_apart`: how far the element moved (metres, plan).
    pub moved: Option<[f64; 2]>,
}

pub(crate) struct OverlapResolve;

impl HostCommand for OverlapResolve {
    type Host = App;
    type Params = ResolveParams;
    type Output = ResolveResult;
    const META: CommandMeta = CommandMeta {
        name: "overlap.resolve",
        version: 1,
        summary: "Resolve an overlap",
        description: "Resolves the overlap between `a` and `b` by a mode (ARC-MOD-032). `accept` marks it intended \
                      (as `overlap.accept`). `move_apart` moves `moves` (default `b`) by the shortest of the four \
                      plan moves (±x, ±y) that clears the other's box, re-checking until they no longer overlap, \
                      as one undo step. `cut_a` / `subtract` cuts `a` by `b`, `cut_b` cuts `b` by `a`, `join` cuts by precedence \
                      (floors, ceilings and roofs cut walls; walls cut columns; else `a` cuts `b`), `intersect` \
                      keeps what `a` shares with `b`, `union` fuses `b` into `a`: each as a kept boolean \
                      (`geometry.boolean`).",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: ResolveParams) -> Result<ResolveResult> {
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let (a, b) = {
            let store = app.get(doc)?.doc.store();
            let id = |r: &ElementRef| {
                store
                    .resolve(r)
                    .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
            };
            (id(&p.a)?, id(&p.b)?)
        };
        match p.mode {
            ResolveMode::Accept => {
                app.execute(
                    "overlap.accept",
                    serde_json::json!({ "a": a, "b": b, "note": p.note }),
                    caller,
                )?;
                Ok(ResolveResult { moved: None })
            }
            ResolveMode::MoveApart => {
                let (mover, other) = match p.moves {
                    Which::A => (a, b),
                    Which::B => (b, a),
                };
                app.group_begin("Move Apart", caller.origin.clone())?;
                match move_apart(app, caller, mover, other) {
                    Ok(total) => {
                        app.group_end(reviera_core::GroupEnd::Assimilate)?;
                        Ok(ResolveResult { moved: Some(total) })
                    }
                    Err(e) => {
                        let _ = app.group_end(reviera_core::GroupEnd::Rollback);
                        Err(e)
                    }
                }
            }
            mode => {
                // The target, its tool and the operation (ARC-MOD-016/017/028).
                let (target, tool, op) = match mode {
                    ResolveMode::CutA | ResolveMode::Subtract => (a, b, "subtract"),
                    ResolveMode::CutB => (b, a, "subtract"),
                    ResolveMode::Intersect => (a, b, "intersect"),
                    ResolveMode::Union => (a, b, "union"),
                    _ => {
                        // Join: floors, ceilings and roofs cut walls; walls cut columns;
                        // otherwise the first cuts the second.
                        let store = app.get(doc)?.doc.store();
                        let class = |id: ElementId| {
                            store.get(id).map(|e| e.class.clone()).unwrap_or_default()
                        };
                        let rank = |c: &str| match c {
                            "IfcSlab" | "IfcRoof" | "IfcCovering" => 2,
                            "IfcWall" | "IfcWallStandardCase" => 1,
                            "IfcColumn" => 0,
                            _ => -1,
                        };
                        let (ra, rb) = (rank(&class(a)), rank(&class(b)));
                        if ra >= 0 && rb >= 0 && rb > ra {
                            (a, b, "subtract")
                        } else {
                            (b, a, "subtract")
                        }
                    }
                };
                app.execute(
                    "geometry.boolean",
                    serde_json::json!({ "target_id": target, "tool_ids": [tool], "op": op }),
                    caller,
                )?;
                Ok(ResolveResult { moved: None })
            }
        }
    }
}

/// Moves `mover` out of `other` along the plan axis needing the shorter move, re-checking (up to
/// four moves) until they no longer overlap. Returns the total move.
fn move_apart(
    app: &mut App,
    caller: &Caller,
    mover: ElementId,
    other: ElementId,
) -> Result<[f64; 2]> {
    let o = Options::default();
    let mut total = [0.0, 0.0];
    for _ in 0..4 {
        let bodies = app.bodies()?;
        let body = |id: ElementId| {
            bodies
                .iter()
                .find(|b| b.element == id)
                .cloned()
                .ok_or_else(|| {
                    Error::new("overlap.no_body", format!("element {} has no body", id.0))
                })
        };
        let (m, t) = (body(mover)?, body(other)?);
        if overlap::between(&m, &t, &o).is_none() {
            return Ok(total);
        }
        let (Some((ml, mh)), Some((tl, th))) = (m.bounds(), t.bounds()) else {
            return Ok(total);
        };
        // The shortest of the four plan moves (±x, ±y) that clears the other's box.
        let candidates = [
            [th[0] - ml[0] + 0.002, 0.0],
            [-(mh[0] - tl[0] + 0.002), 0.0],
            [0.0, th[1] - ml[1] + 0.002],
            [0.0, -(mh[1] - tl[1] + 0.002)],
        ];
        let v = candidates
            .into_iter()
            .min_by(|p, q| p[0].hypot(p[1]).total_cmp(&q[0].hypot(q[1])))
            .expect("four");
        app.execute(
            "element.move",
            serde_json::json!({ "element_ids": [mover], "vector": [v[0], v[1], 0.0] }),
            caller,
        )?;
        total = [total[0] + v[0], total[1] + v[1]];
    }
    let bodies = app.bodies()?;
    let find = |id: ElementId| bodies.iter().find(|b| b.element == id);
    match (find(mover), find(other)) {
        (Some(m), Some(t)) if overlap::between(m, t, &o).is_some() => Err(Error::new(
            "overlap.still_overlapping",
            "the elements still overlap after moving; move them by hand",
        )),
        _ => Ok(total),
    }
}
