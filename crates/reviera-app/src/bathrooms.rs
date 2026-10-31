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
}

/// One thing the check found.
#[derive(Debug, Serialize, JsonSchema)]
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
                      distance rules, such as a WC's centreline from its side wall. Each finding names the pack, \
                      its edition and layer, the rule, its source and confidence, and is an error, or a note \
                      when it comes from guidance or a rule marked advisory. Errors first.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<CheckParams>) -> Result<Vec<Finding>> {
        let p = p.unwrap_or_default();
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
