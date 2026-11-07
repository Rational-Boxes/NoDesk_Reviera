// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! `rules.check` (ARC-RULE-002): the use zones of the shared rule packs in use, each element's
//! body inside one an intrusion, and any door swinging into a zone whose rule forbids it.

use riviera_core::bath_rules::{Confidence, Layer};
use riviera_core::overlap::{self, Options};
use riviera_core::{Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Result, rules};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// Only these elements' zones.
    pub element_ids: Option<Vec<ElementRef>>,
    /// Only this domain's packs (`interiors`, …).
    pub domain: Option<String>,
    #[schemars(extend("x-riviera-data-type" = "length"))]
    pub tolerance: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    Intrusion,
    DoorSwing,
    /// Too little clear height under a hanging element (ADA 307.4).
    Headroom,
    /// Projecting too far from its wall (ADA 307.2).
    Projection,
    /// Too high or too low over the element under it.
    HeightOver,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Finding {
    pub severity: Severity,
    pub kind: FindingKind,
    pub element: ElementId,
    pub domain: String,
    pub pack: String,
    pub edition: String,
    pub layer: Layer,
    pub rule: String,
    pub description: String,
    pub confidence: Confidence,
    pub source: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<ElementId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub centroid: Option<[f64; 3]>,
    /// For a mounting rule: what was measured and the rule's range (metres).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

pub(crate) struct RulesCheck;

impl HostCommand for RulesCheck {
    type Host = App;
    type Params = Option<CheckParams>;
    type Output = Vec<Finding>;
    const META: CommandMeta = CommandMeta {
        name: "rules.check",
        version: 1,
        summary: "Check furniture and equipment against the rule packs",
        description: "Checks the use zones the shared rule packs in use give placed furniture and equipment \
                      (ARC-RULE-002), for every element, those given, or one domain's packs: every other \
                      element's body inside a zone (floors, floor coverings, accessories and the element itself \
                      excepted), and any door swinging into a zone whose rule forbids it. Each finding names \
                      the domain, pack, edition and layer, the rule, its source and confidence, and the \
                      intruding element; it is an error, or a note when it comes from guidance or a rule \
                      marked advisory. Errors first.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: Option<CheckParams>) -> Result<Vec<Finding>> {
        check(app, p.unwrap_or_default())
    }
}

pub(crate) fn check(app: &mut App, p: CheckParams) -> Result<Vec<Finding>> {
    let bodies = app.bodies()?;
    let doc = app.active().expect("bodies checked");
    let store = app.get(doc)?.doc.store();
    let only: Option<Vec<ElementId>> = p
        .element_ids
        .as_deref()
        .map(|refs| refs.iter().map(|r| store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))).collect::<Result<Vec<_>>>())
        .transpose()?;
    let o = Options { tolerance: p.tolerance.unwrap_or(0.001), ..Options::default() };
    let packs: Vec<rules::Pack> = rules::all_active(store).into_iter().filter(|pk| p.domain.as_deref().is_none_or(|d| d == pk.domain)).collect();
    let label = |id: ElementId| {
        store.get(id).map_or_else(
            || format!("#{}", id.0),
            |e| {
                let name = e.name.clone().filter(|n| !n.is_empty()).or_else(|| riviera_core::mesh_objects::type_of(store, e).and_then(|(t, _)| store.get(t)?.name.clone()));
                format!("{} {}", name.unwrap_or_else(|| e.class.trim_start_matches("Ifc").to_owned()), id.0)
            },
        )
    };
    let swings: Vec<_> = riviera_core::clearances::all(store).into_iter().filter(|c| c.kind == riviera_core::clearances::ClearanceKind::DoorSwing).collect();
    let mut out = Vec::new();
    for e in store.iter().filter(|e| only.as_ref().is_none_or(|ids| ids.contains(&e.id()))) {
        for z in rules::zones_of(store, e, &packs) {
            let pack = packs.iter().find(|pk| pk.id == z.pack).expect("a zone's pack is in use");
            let what = label(e.id());
            let finding = |kind: FindingKind, other: ElementId, depth: f64, centroid: [f64; 3], message: String| Finding {
                severity: if pack.layer == Layer::Recommended || z.zone.confidence == Confidence::Advisory { Severity::Note } else { Severity::Error },
                kind,
                element: e.id(),
                domain: pack.domain.clone(),
                pack: pack.id.clone(),
                edition: pack.edition.clone(),
                layer: pack.layer,
                rule: z.zone.id.clone(),
                description: z.zone.description.clone(),
                confidence: z.zone.confidence,
                source: z.zone.source.clone(),
                message,
                other: Some(other),
                depth: Some(depth),
                centroid: Some(centroid),
                measured: None,
                min: None,
                max: None,
            };
            let skip = |a: ElementId, b: ElementId| {
                let other = if a == e.id() { b } else { a };
                other == e.id()
                    || riviera_core::clearances::may_share(store, z.clearance.kind, other)
                    || store.get(other).is_some_and(|o| z.zone.allows(store, o))
                    || riviera_core::bodies::related(store, a, b)
                    || riviera_core::overlap_accept::is_accepted(store, a, b)
            };
            for ov in overlap::find(&[z.clearance.body()], Some(&bodies), &skip, &o) {
                let other = if ov.a == e.id() { ov.b } else { ov.a };
                out.push(finding(FindingKind::Intrusion, other, ov.depth, ov.centroid, format!("{} is in the use zone of {what} ({})", label(other), z.zone.id)));
            }
            if z.clearance.door_swing_forbidden {
                for s in &swings {
                    if let Some(ov) = overlap::between(&z.clearance.body(), &s.body(), &o) {
                        out.push(finding(FindingKind::DoorSwing, s.element, ov.depth, ov.centroid, format!("{} swings into the use zone of {what}, which {} forbids", label(s.element), z.zone.id)));
                    }
                }
            }
        }
    }
    // Mounting rules: headroom, projection, height over.
    let inch = |v: f64| format!("{:.1} in", v / 0.0254);
    for e in store.iter().filter(|e| only.as_ref().is_none_or(|ids| ids.contains(&e.id()))) {
        for m in rules::mountings_of(store, e, &packs) {
            let pack = packs.iter().find(|pk| pk.id == m.pack).expect("a finding's pack is in use");
            let what = label(e.id());
            let (kind, message) = match m.kind.as_str() {
                "headroom" => (FindingKind::Headroom, format!("{what} hangs {} above the floor; {} needs {} ({})", inch(m.measured), m.rule, inch(m.min.unwrap_or(0.0)), m.description)),
                "projection" => (FindingKind::Projection, format!("{what} projects {} from its wall; {} allows {}", inch(m.measured), m.rule, inch(m.max.unwrap_or(0.0)))),
                _ => (
                    FindingKind::HeightOver,
                    format!("{what} is {} above {}; {} asks {}–{}", inch(m.measured), m.other.map_or_else(String::new, &label), m.rule, m.min.map_or_else(|| "…".into(), inch), m.max.map_or_else(|| "…".into(), inch)),
                ),
            };
            out.push(Finding {
                severity: if pack.layer == Layer::Recommended || m.confidence == Confidence::Advisory { Severity::Note } else { Severity::Error },
                kind,
                element: e.id(),
                domain: pack.domain.clone(),
                pack: pack.id.clone(),
                edition: pack.edition.clone(),
                layer: pack.layer,
                rule: m.rule.clone(),
                description: m.description.clone(),
                confidence: m.confidence,
                source: m.source.clone(),
                message,
                other: m.other,
                depth: None,
                centroid: None,
                measured: Some(m.measured),
                min: m.min,
                max: m.max,
            });
        }
    }
    out.sort_by_key(|f| f.severity);
    Ok(out)
}
