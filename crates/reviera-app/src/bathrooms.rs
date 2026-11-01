// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `bath.check` (ARC-BATH-002): each bathroom fixture's clearance zones under the rule packs in
//! use, tested against the model's bodies and door swings, and its distance rules measured.

use reviera_core::bath_rules::{self, Confidence, FixtureKind, Layer};
use reviera_core::overlap::{self, Options};
use reviera_core::{Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// Check these fixtures (default every bathroom fixture).
    pub element_ids: Option<Vec<ElementRef>>,
    /// Intrusions thinner than this are contact (default 1 mm), and distances are measured to
    /// within it (default 5 mm).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub tolerance: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    /// From guidance, or from a rule whose reading needs checking.
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// An element's body in a zone.
    Intrusion,
    /// A door swinging into a zone that forbids it.
    DoorSwing,
    /// A distance outside its range.
    Distance,
    /// No side wall found to measure from.
    NoSideWall,
    /// A grab bar the rules ask for is not there (ARC-BATH-003).
    MissingBar,
    /// A grab bar out of its rule's range: its stretch, length, height, diameter or gap.
    Bar,
}

/// One thing the check found.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Finding {
    pub severity: Severity,
    pub kind: FindingKind,
    /// The fixture and its kind.
    pub element: ElementId,
    pub fixture: FixtureKind,
    /// The pack, its edition and layer, and the rule.
    pub pack: String,
    pub edition: String,
    pub layer: Layer,
    pub rule: String,
    pub description: String,
    pub confidence: Confidence,
    pub source: String,
    /// What it says, in a sentence.
    pub message: String,
    /// The element intruding, for an intrusion or a door swing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<ElementId>,
    /// How far in (metres), and where.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub centroid: Option<[f64; 3]>,
    /// A distance: what was measured and the range required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

fn mm(v: f64) -> String {
    format!("{:.0} mm", v * 1000.0)
}

pub(crate) struct BathCheck;

impl HostCommand for BathCheck {
    type Host = App;
    type Params = Option<CheckParams>;
    type Output = Vec<Finding>;
    const META: CommandMeta = CommandMeta {
        name: "bath.check",
        version: 1,
        summary: "Check bathroom fixtures against the rule packs",
        description: "Checks each bathroom fixture (or those given) under the rule packs in use (ARC-BATH-002): \
                      every element's body inside one of its clearance zones (the fixture itself, grab bars, rails \
                      and accessories excepted), any door swinging into a zone whose rule forbids it, and its \
                      distance rules, such as a WC's centreline from its side wall, and its grab bars \
                      (ARC-BATH-003): missing, not covering their stretch, too short, or with their \
                      height, diameter or gap out of range. Each finding names the pack, \
                      its edition and layer, the rule, its source and confidence, and is an error, or a note \
                      when it comes from guidance or a rule marked advisory. Errors first.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<CheckParams>) -> Result<Vec<Finding>> {
        check(app, p.unwrap_or_default())
    }
}

/// The bathroom check (`bath.check`).
pub(crate) fn check(app: &mut App, p: CheckParams) -> Result<Vec<Finding>> {
    {
        let bodies = app.bodies()?;
        let doc = app.active().expect("bodies checked");
        let store = app.get(doc)?.doc.store();
        let only: Option<Vec<ElementId>> = p
            .element_ids
            .as_deref()
            .map(|refs| refs.iter().map(|r| store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))).collect::<Result<Vec<_>>>())
            .transpose()?;
        let o = Options { tolerance: p.tolerance.unwrap_or(0.001), ..Options::default() };
        let tolerance = p.tolerance.unwrap_or(0.005);
        let packs = bath_rules::active(store);
        // Its name, else its type's (a placed component), else its class; and its id.
        let label = |id: ElementId| {
            store.get(id).map_or_else(
                || format!("#{}", id.0),
                |e| {
                    let name = e.name.clone().filter(|n| !n.is_empty()).or_else(|| reviera_core::mesh_objects::type_of(store, e).and_then(|(t, _)| store.get(t)?.name.clone()));
                    format!("{} {}", name.unwrap_or_else(|| e.class.trim_start_matches("Ifc").to_owned()), id.0)
                },
            )
        };
        let swings: Vec<_> = reviera_core::clearances::all(store).into_iter().filter(|c| c.kind == reviera_core::clearances::ClearanceKind::DoorSwing).collect();
        let mut out = Vec::new();
        for f in bath_rules::fixtures(store).iter().filter(|f| only.as_ref().is_none_or(|ids| ids.contains(&f.element))) {
            let what = label(f.element);
            let finding = |pack: &bath_rules::Pack, rule: &str, description: &str, confidence: Confidence, source: &str, kind: FindingKind, message: String| Finding {
                severity: if pack.layer == Layer::Recommended || confidence == Confidence::Advisory { Severity::Note } else { Severity::Error },
                kind,
                element: f.element,
                fixture: f.kind,
                pack: pack.id.clone(),
                edition: pack.edition.clone(),
                layer: pack.layer,
                rule: rule.to_owned(),
                description: description.to_owned(),
                confidence,
                source: source.to_owned(),
                message,
                other: None,
                depth: None,
                centroid: None,
                measured: None,
                min: None,
                max: None,
            };
            for space in bath_rules::spaces_of(store, f, &packs) {
                let pack = packs.iter().find(|p| p.id == space.pack).expect("from these packs");
                let z = &space.zone;
                let skip = |a: ElementId, b: ElementId| {
                    let other = if a == f.element { b } else { a };
                    other == f.element
                        || reviera_core::clearances::may_share(store, space.clearance.kind, other)
                        || reviera_core::bodies::related(store, a, b)
                        || reviera_core::overlap_accept::is_accepted(store, a, b)
                };
                for ov in overlap::find(&[space.clearance.body()], Some(&bodies), &skip, &o) {
                    let other = if ov.a == f.element { ov.b } else { ov.a };
                    out.push(Finding {
                        other: Some(other),
                        depth: Some(ov.depth),
                        centroid: Some(ov.centroid),
                        ..finding(pack, &z.id, &z.description, z.confidence, &z.source, FindingKind::Intrusion, format!("{} is in the clear space of {what} ({})", label(other), z.id))
                    });
                }
                if space.clearance.door_swing_forbidden {
                    for s in &swings {
                        if let Some(ov) = overlap::between(&space.clearance.body(), &s.body(), &o) {
                            out.push(Finding {
                                other: Some(s.element),
                                depth: Some(ov.depth),
                                centroid: Some(ov.centroid),
                                ..finding(pack, &z.id, &z.description, z.confidence, &z.source, FindingKind::DoorSwing, format!("{} swings into the clear space of {what}, which {} forbids", label(s.element), z.id))
                            });
                        }
                    }
                }
            }
            // Grab bars (ARC-BATH-003): on the rule's wall and face, over its stretch.
            let bars = reviera_core::grab_bars::all(store);
            for (pack, b) in bath_rules::bars_of(store, f, &packs) {
                let st = match f.bar_stretch(store, b) {
                    Ok(st) => st,
                    Err(why) => {
                        out.push(finding(pack, &b.id, &b.description, b.confidence, &b.source, FindingKind::NoSideWall, format!("{} cannot be checked at {what}: {why}", b.id)));
                        continue;
                    }
                };
                let wall_guid = store.get(st.wall).map(|w| w.guid().to_string()).unwrap_or_default();
                // The bars on that face overlapping the stretch, the most overlapping first.
                let mut on: Vec<(ElementId, reviera_core::grab_bars::GrabBarRecipe, [f64; 2])> = bars
                    .iter()
                    .filter_map(|(id, _)| {
                        let r = store.get(*id).and_then(reviera_core::grab_bars::recipe)?;
                        let lo = r.points.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
                        let hi = r.points.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
                        (r.wall == wall_guid && (r.side - st.side).abs() < 0.5 && hi > st.along[0] && lo < st.along[1]).then_some((*id, r, [lo, hi]))
                    })
                    .collect();
                let overlap = |u: &[f64; 2]| u[1].min(st.along[1]) - u[0].max(st.along[0]);
                on.sort_by(|a, b| overlap(&b.2).total_cmp(&overlap(&a.2)));
                let Some((bar, r, u)) = on.into_iter().next() else {
                    out.push(finding(pack, &b.id, &b.description, b.confidence, &b.source, FindingKind::MissingBar, format!("{what} has no grab bar where {} asks for one", b.id)));
                    continue;
                };
                let g = bars.iter().find(|(id, _)| *id == bar).map(|(_, g)| g).expect("listed");
                let mut problems = Vec::new();
                if u[0] > st.along[0] + tolerance || u[1] < st.along[1] - tolerance {
                    problems.push("does not cover the stretch the rule asks for".to_owned());
                }
                if let Some(min) = b.length_min
                    && g.length < min - tolerance
                {
                    problems.push(format!("is {} long, at least {} asked", mm(g.length), mm(min)));
                }
                let range = |r: &bath_rules::Range| match (r.min, r.max) {
                    (Some(a), Some(z)) if (a - z).abs() < 1e-9 => mm(a),
                    (Some(a), Some(z)) => format!("{:.0}–{}", a * 1000.0, mm(z)),
                    (Some(a), None) => format!("at least {}", mm(a)),
                    (None, Some(z)) => format!("at most {}", mm(z)),
                    (None, None) => "any".into(),
                };
                if !b.top.holds(g.top, tolerance) {
                    problems.push(format!("has its top {} up, {} asked", mm(g.top), range(&b.top)));
                }
                if let Some(d) = &b.diameter
                    && !d.holds(r.diameter, 0.0005)
                {
                    problems.push(format!("is {} thick, {} asked", mm(r.diameter), range(d)));
                }
                if let Some(gp) = &b.gap
                    && !gp.holds(r.gap, 0.0005)
                {
                    problems.push(format!("stands {} off the wall, {} asked", mm(r.gap), range(gp)));
                }
                if !problems.is_empty() {
                    out.push(Finding {
                        other: Some(bar),
                        ..finding(pack, &b.id, &b.description, b.confidence, &b.source, FindingKind::Bar, format!("{}'s grab bar ({}) {}", what, b.id, problems.join("; ")))
                    });
                }
            }
            for (pack, d) in bath_rules::distances_of(store, f, &packs) {
                let Some(m) = f.side_distance else {
                    out.push(finding(pack, &d.id, &d.description, d.confidence, &d.source, FindingKind::NoSideWall, format!("No side wall within {} of {what} to measure {} from", mm(bath_rules::SIDE_SEARCH), d.id)));
                    continue;
                };
                let low = d.min.is_some_and(|v| m < v - tolerance);
                let high = d.max.is_some_and(|v| m > v + tolerance);
                if low || high {
                    let range = match (d.min, d.max) {
                        (Some(a), Some(b)) if (a - b).abs() < 1e-9 => mm(a),
                        (Some(a), Some(b)) => format!("{:.0}–{}", a * 1000.0, mm(b)),
                        (Some(a), None) => format!("at least {}", mm(a)),
                        (None, Some(b)) => format!("at most {}", mm(b)),
                        (None, None) => String::new(),
                    };
                    out.push(Finding {
                        measured: Some(m),
                        min: d.min,
                        max: d.max,
                        ..finding(pack, &d.id, &d.description, d.confidence, &d.source, FindingKind::Distance, format!("{what}'s centreline is {} from its side wall; {} asks for {range}", mm(m), d.id))
                    });
                }
            }
        }
        out.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.element.0.cmp(&b.element.0)));
        Ok(out)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SwapParams {
    /// The fixtures to change.
    pub element_ids: Vec<ElementRef>,
    /// The product (or other component) type to give them.
    pub type_id: ElementRef,
}

/// A connector that moved in a swap.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Moved {
    pub name: String,
    /// How far (metres), and from and to (project coordinates).
    pub distance: f64,
    pub from: [f64; 3],
    pub to: [f64; 3],
}

/// What a swap did to one fixture.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Swapped {
    pub element: ElementId,
    pub size_before: [f64; 3],
    pub size_after: [f64; 3],
    pub connectors_moved: Vec<Moved>,
    /// Connectors the old type had that the new one has not (by name).
    pub connectors_lost: Vec<String>,
    pub findings_new: Vec<Finding>,
    pub findings_gone: Vec<Finding>,
}

pub(crate) struct BathSwapProduct;

impl HostCommand for BathSwapProduct {
    type Host = App;
    type Params = SwapParams;
    type Output = Vec<Swapped>;
    const META: CommandMeta = CommandMeta {
        name: "bath.swap_product",
        version: 1,
        summary: "Put a product in place of placeholder fixtures",
        description: "Gives fixtures another component type of the same category, typically a manufacturer's \
                      product made with `bath.make_product` in place of a generic placeholder (ARC-BATH-006), \
                      through `element.set_type`, so each keeps its GlobalId, place and own data. Reports for each \
                      its size before and after, the connectors that moved and how far, those the new type lacks, \
                      and the `bath.check` findings that appeared or went away. Undo restores the old type.",
        effects: &[Effect::Edit],
        transaction_name: "Swap Product",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: SwapParams) -> Result<Vec<Swapped>> {
        let doc = app.active().ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let ids: Vec<ElementId> = {
            let store = app.get(doc)?.doc.store();
            p.element_ids.iter().map(|r| store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))).collect::<Result<_>>()?
        };
        let state = |app: &mut App| -> Result<Vec<([f64; 3], Vec<(String, [f64; 3])>)>> {
            let store = app.get(doc)?.doc.store();
            Ok(ids
                .iter()
                .map(|id| {
                    let e = store.get(*id);
                    let size = e.and_then(|e| reviera_core::mesh_objects::type_of(store, e)).map_or([0.0; 3], |(_, t)| t.size.map(|v| v * t.scale));
                    let ports = e.map(|e| reviera_core::mep::ports(store, e).into_iter().map(|q| (q.connector.name, q.position)).collect()).unwrap_or_default();
                    (size, ports)
                })
                .collect())
        };
        let only = Some(ids.iter().map(|id| ElementRef::Id(*id)).collect::<Vec<_>>());
        let before = state(app)?;
        let found_before = check(app, CheckParams { element_ids: only.clone(), tolerance: None })?;
        app.execute("element.set_type", serde_json::json!({ "element_ids": ids.iter().map(|i| i.0).collect::<Vec<_>>(), "type_id": p.type_id }), caller)?;
        let after = state(app)?;
        let found_after = check(app, CheckParams { element_ids: only, tolerance: None })?;
        let key = |f: &Finding| (f.element, f.rule.clone(), format!("{:?}", f.kind), f.other);
        Ok(ids
            .iter()
            .enumerate()
            .map(|(k, id)| {
                let (sb, pb) = &before[k];
                let (sa, pa) = &after[k];
                let mut moved = Vec::new();
                let mut lost = Vec::new();
                for (name, from) in pb {
                    match pa.iter().find(|(n, _)| n == name) {
                        Some((_, to)) => {
                            let d = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2) + (to[2] - from[2]).powi(2)).sqrt();
                            if d > 0.0005 {
                                moved.push(Moved { name: name.clone(), distance: d, from: *from, to: *to });
                            }
                        }
                        None => lost.push(name.clone()),
                    }
                }
                let mine = |v: &Vec<Finding>, other: &Vec<Finding>| -> Vec<Finding> {
                    v.iter().filter(|f| f.element == *id && !other.iter().any(|o| key(o) == key(f))).cloned().collect()
                };
                Swapped { element: *id, size_before: *sb, size_after: *sa, connectors_moved: moved, connectors_lost: lost, findings_new: mine(&found_after, &found_before), findings_gone: mine(&found_before, &found_after) }
            })
            .collect())
    }
}
