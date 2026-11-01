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
    /// Electrical equipment where a bathroom's electrical zones do not allow it (ARC-BATH-010).
    Electrical,
    /// A level-access shower's floor falling too little, too much or not at all (ARC-BATH-011).
    Falls,
    /// A hot tub's load on the floor, or the pad it needs (ARC-BATH-013).
    Load,
    /// A sauna's heater, height or door (ARC-BATH-014).
    Sauna,
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
            let product = bath_rules::product_pack();
            for space in bath_rules::spaces_of(store, f, &packs) {
                let pack = packs.iter().find(|p| p.id == space.pack).unwrap_or(&product);
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
        out.extend(electrical_findings(store, only.as_deref()));
        out.extend(falls_findings(store, only.as_deref()));
        out.extend(spa_findings(store, only.as_deref()));
        out.extend(sauna_findings(store, only.as_deref()));
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

/// Electrical devices in bathrooms' electrical zones, or sockets too near them (ARC-BATH-010).
fn electrical_findings(store: &reviera_core::Store, only: Option<&[ElementId]>) -> Vec<Finding> {
    use reviera_core::mesh_objects::{self as mo, Category};
    let zones = bath_rules::electrical_zones(store);
    if zones.is_empty() {
        return Vec::new();
    }
    let packs = bath_rules::active(store);
    let fixtures = bath_rules::fixtures(store);
    let mut out = Vec::new();
    for e in store.iter().filter(|e| mo::mesh_object_recipe(e).is_some()) {
        let Some((_, ty)) = mo::type_of(store, e) else { continue };
        let electrical = ty.category.is_electrical_device() || matches!(ty.category, Category::LightingFixture | Category::Fan | Category::SpaceHeater | Category::ElectricAppliance);
        if !electrical {
            continue;
        }
        let Ok(g) = mo::regenerate(store, e) else { continue };
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for (_, (pts, _)) in g.slot_meshes() {
            for q in pts {
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
        }
        if lo[0] > hi[0] {
            continue;
        }
        let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        let socket = ty.category == Category::ElectricalFixture && ty.predefined_type() == "POWEROUTLET";
        let name = e.name.clone().or_else(|| mo::type_of(store, e).and_then(|(t, _)| store.get(t)?.name.clone())).unwrap_or_else(|| e.class.clone());
        let label = format!("{name} {}", e.id().0);
        for pack in packs.iter().filter(|p| p.electrical.is_some()) {
            let rules = pack.electrical.as_ref().expect("filtered");
            // Per fixture: the innermost zone the device's middle is in.
            for f in &fixtures {
                if only.is_some_and(|ids| !ids.contains(&f.element)) {
                    continue;
                }
                let mine: Vec<&bath_rules::ElectricalZone> = zones.iter().filter(|z| z.element == f.element && z.pack == pack.id).collect();
                let Some(zone1) = mine.iter().find(|z| z.zone == 1) else { continue };
                let inner = mine.iter().filter(|z| z.contains(mid)).map(|z| z.zone).min();
                let fixture_label = store.get(f.element).and_then(|x| mo::type_of(store, x)).and_then(|(t, _)| store.get(t)?.name.clone()).unwrap_or_default();
                let base = |message: String, severity: Severity| Finding {
                    severity,
                    kind: FindingKind::Electrical,
                    element: f.element,
                    fixture: f.kind,
                    pack: pack.id.clone(),
                    edition: pack.edition.clone(),
                    layer: pack.layer,
                    rule: rules.id.clone(),
                    description: rules.description.clone(),
                    confidence: rules.confidence,
                    source: rules.source.clone(),
                    message,
                    other: Some(e.id()),
                    depth: None,
                    centroid: Some(mid),
                    measured: None,
                    min: None,
                    max: None,
                };
                let sev = if pack.layer == Layer::Recommended || rules.confidence == Confidence::Advisory { Severity::Note } else { Severity::Error };
                if socket {
                    if let Some(z) = inner {
                        out.push(base(format!("{label} is a socket outlet in zone {z} of {fixture_label} {}", f.element.0), sev));
                    } else {
                        let d = bath_rules::plan_distance(&zone1.outline, [mid[0], mid[1]]);
                        if d < rules.socket_distance - 1e-6 {
                            out.push(Finding { measured: Some(d), min: Some(rules.socket_distance), ..base(format!("{label} is a socket outlet {} from zone 1 of {fixture_label} {}; {} asks for at least {}", mm(d), f.element.0, rules.id, mm(rules.socket_distance)), sev) });
                        }
                    }
                    continue;
                }
                let Some(z) = inner else { continue };
                let need = match z {
                    0 => &rules.ip.zone0,
                    1 => &rules.ip.zone1,
                    _ => &rules.ip.zone2,
                };
                match bath_rules::device_ip(store, e) {
                    None => out.push(base(format!("{label} is in zone {z} of {fixture_label} {} and has no IP rating; the zone needs {need} (Pset_ElectricalDeviceCommon.IP_Code)", f.element.0), Severity::Note)),
                    Some(ip) => {
                        if bath_rules::ip_water(&ip).unwrap_or(0) < bath_rules::ip_water(need).unwrap_or(0) {
                            out.push(base(format!("{label} ({ip}) is in zone {z} of {fixture_label} {}, which needs {need}", f.element.0), sev));
                        }
                    }
                }
            }
        }
    }
    out
}

/// Level-access showers whose floor falls too little, too much or not at all (ARC-BATH-011).
fn falls_findings(store: &reviera_core::Store, only: Option<&[ElementId]>) -> Vec<Finding> {
    let packs: Vec<bath_rules::Pack> = bath_rules::active(store).into_iter().filter(|p| p.falls.is_some()).collect();
    if packs.is_empty() {
        return Vec::new();
    }
    let pct = |g: f64| format!("{:.1} %", g * 100.0);
    let mut out = Vec::new();
    for f in bath_rules::fixtures(store) {
        if only.is_some_and(|ids| !ids.contains(&f.element)) || !bath_rules::level_access(store, &f) {
            continue;
        }
        let area = bath_rules::shower_area(store, &f);
        let n = area.len() as f64;
        let centre = [area.iter().map(|q| q[0]).sum::<f64>() / n, area.iter().map(|q| q[1]).sum::<f64>() / n];
        let Some(floor) = bath_rules::floor_under(store, &f.level, centre) else { continue };
        let range = bath_rules::falls_in(store, floor, &area);
        let what = format!("Shower {}", f.element.0);
        for pack in &packs {
            let rules = pack.falls.as_ref().expect("filtered");
            let sev = if pack.layer == Layer::Recommended || rules.confidence == Confidence::Advisory { Severity::Note } else { Severity::Error };
            let base = |message: String, measured: Option<f64>| Finding {
                severity: sev,
                kind: FindingKind::Falls,
                element: f.element,
                fixture: f.kind,
                pack: pack.id.clone(),
                edition: pack.edition.clone(),
                layer: pack.layer,
                rule: rules.id.clone(),
                description: rules.description.clone(),
                confidence: rules.confidence,
                source: rules.source.clone(),
                message,
                other: Some(floor),
                depth: None,
                centroid: Some([centre[0], centre[1], f.floor]),
                measured,
                min: Some(rules.min),
                max: rules.max,
            };
            match range {
                None => out.push(base(format!("{what}'s floor has no fall to a drain; {} asks for at least {} (bath.shower_falls lays one)", rules.id, pct(rules.min)), None)),
                Some((least, steepest)) => {
                    if least < rules.min - 1e-4 {
                        out.push(base(format!("{what}'s floor falls {} at its flattest; {} asks for at least {}", pct(least), rules.id, pct(rules.min)), Some(least)));
                    }
                    if let Some(max) = rules.max.filter(|m| steepest > m + 1e-4) {
                        out.push(base(format!("{what}'s floor falls {}, steeper than the {} {} allows", pct(steepest), pct(max), rules.id), Some(steepest)));
                    }
                }
            }
        }
    }
    out
}

/// Hot tubs: devices too near them, and their load on the floor (ARC-BATH-013).
fn spa_findings(store: &reviera_core::Store, only: Option<&[ElementId]>) -> Vec<Finding> {
    use reviera_core::mesh_objects::{self as mo, Category};
    let packs: Vec<bath_rules::Pack> = bath_rules::active(store).into_iter().filter(|p| p.spa.is_some()).collect();
    let spas: Vec<bath_rules::Fixture> = bath_rules::fixtures(store).into_iter().filter(|f| f.kind == FixtureKind::Spa && !only.is_some_and(|ids| !ids.contains(&f.element))).collect();
    if packs.is_empty() || spas.is_empty() {
        return Vec::new();
    }
    // Devices: their middle and lowest point, and what they are.
    #[derive(PartialEq)]
    enum Dev {
        Receptacle,
        Switch,
        Light,
    }
    let mut devices = Vec::new();
    for e in store.iter().filter(|e| mo::mesh_object_recipe(e).is_some()) {
        let Some((_, ty)) = mo::type_of(store, e) else { continue };
        let dev = match ty.category {
            Category::ElectricalFixture if ty.predefined_type() == "POWEROUTLET" => Dev::Receptacle,
            Category::SwitchingDevice => Dev::Switch,
            Category::LightingFixture | Category::Fan => Dev::Light,
            _ => continue,
        };
        let Ok(g) = mo::regenerate(store, e) else { continue };
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for (_, (pts, _)) in g.slot_meshes() {
            for q in pts {
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
        }
        if lo[0] > hi[0] {
            continue;
        }
        let name = e.name.clone().or_else(|| store.get(mo::type_of(store, e)?.0)?.name.clone()).unwrap_or_else(|| e.class.clone());
        devices.push((e.id(), dev, [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0], lo[2], format!("{name} {}", e.id().0)));
    }
    let lowest = reviera_core::levels::list(store).iter().map(|l| l.elevation).fold(f64::MAX, f64::min);
    let mut out = Vec::new();
    for f in &spas {
        let Some(el) = store.get(f.element) else { continue };
        let outline = bath_rules::outline(f);
        let rim = mo::regenerate(store, el).ok().map_or(f.floor + 0.9, |g| g.placement.origin[2] + g.mesh.positions.iter().map(|q| f64::from(q[2]) * g.placement.scale).fold(f64::MIN, f64::max));
        let what = format!("Hot tub {}", f.element.0);
        for pack in &packs {
            let rules = pack.spa.as_ref().expect("filtered");
            let sev = if pack.layer == Layer::Recommended || rules.confidence == Confidence::Advisory { Severity::Note } else { Severity::Error };
            let base = |kind: FindingKind, message: String, other: Option<ElementId>, measured: Option<f64>, min: Option<f64>, max: Option<f64>| Finding {
                severity: sev,
                kind,
                element: f.element,
                fixture: f.kind,
                pack: pack.id.clone(),
                edition: pack.edition.clone(),
                layer: pack.layer,
                rule: rules.id.clone(),
                description: rules.description.clone(),
                confidence: rules.confidence,
                source: rules.source.clone(),
                message,
                other,
                depth: None,
                centroid: None,
                measured,
                min,
                max,
            };
            let water = rim - rules.water_below_rim;
            for (id, dev, mid, low, label) in &devices {
                let d = bath_rules::plan_distance(&outline, *mid);
                let (need, what_dev) = match dev {
                    Dev::Receptacle => (rules.receptacle_distance, "a receptacle"),
                    Dev::Switch => (rules.switch_distance, "a wall switch"),
                    Dev::Light => {
                        if d < rules.receptacle_distance - 1e-6 && *low < water + rules.luminaire_height - 1e-6 {
                            out.push(base(FindingKind::Electrical, format!("{label} is {} above {what}'s water; {} asks for at least {}", mm(*low - water), rules.id, mm(rules.luminaire_height)), Some(*id), Some(*low - water), Some(rules.luminaire_height), None));
                        }
                        continue;
                    }
                };
                if d < need - 1e-6 {
                    out.push(base(FindingKind::Electrical, format!("{label} is {what_dev} {} from {what}; {} asks for at least {}", mm(d), rules.id, mm(need)), Some(*id), Some(d), Some(need), None));
                }
            }
            // The load on the floor: the filled weight and a bather a seat over the footprint.
            if let Some((kg, seats)) = bath_rules::spa_weight(store, el) {
                let area = (f.x[1] - f.x[0]) * (f.back - f.front);
                let kpa = (kg + seats * rules.occupant_mass) * 9.81 / area.max(1e-6) / 1000.0;
                if f.floor > lowest + 0.01 {
                    if kpa > rules.floor_live_load {
                        out.push(base(FindingKind::Load, format!("{what} loads its floor with {kpa:.1} kPa (filled, {seats} bathers), above the {:.2} kPa the floor is commonly designed for; check the structure", rules.floor_live_load), None, Some(kpa), None, Some(rules.floor_live_load)));
                    }
                } else {
                    out.push(base(FindingKind::Load, format!("{what} ({kpa:.1} kPa filled) needs a pad of at least {} of reinforced concrete", mm(rules.pad_thickness)), None, Some(kpa), None, None));
                }
            }
        }
    }
    out
}

/// Saunas: the heater's power for the room, the ceiling over it, the room's height and its
/// door (ARC-BATH-014).
fn sauna_findings(store: &reviera_core::Store, only: Option<&[ElementId]>) -> Vec<Finding> {
    let heaters: Vec<bath_rules::Fixture> = bath_rules::fixtures(store).into_iter().filter(|f| f.kind == FixtureKind::SaunaHeater && !only.is_some_and(|ids| !ids.contains(&f.element))).collect();
    if heaters.is_empty() {
        return Vec::new();
    }
    let rules = bath_rules::sauna_rules(store);
    let pack = bath_rules::active(store).into_iter().find(|p| p.sauna.is_some()).or_else(|| bath_rules::shipped().iter().find(|p| p.sauna.is_some()).cloned()).expect("a pack has sauna rules");
    let swings: Vec<_> = reviera_core::clearances::all(store).into_iter().filter(|c| c.kind == reviera_core::clearances::ClearanceKind::DoorSwing).collect();
    let mut out = Vec::new();
    for f in &heaters {
        let Some(el) = store.get(f.element) else { continue };
        let what = format!("Sauna heater {}", f.element.0);
        let base = |message: String, other: Option<ElementId>, measured: Option<f64>, min: Option<f64>, max: Option<f64>| Finding {
            severity: if pack.layer == Layer::Recommended || rules.confidence == Confidence::Advisory { Severity::Note } else { Severity::Error },
            kind: FindingKind::Sauna,
            element: f.element,
            fixture: f.kind,
            pack: pack.id.clone(),
            edition: pack.edition.clone(),
            layer: pack.layer,
            rule: rules.id.clone(),
            description: rules.description.clone(),
            confidence: rules.confidence,
            source: rules.source.clone(),
            message,
            other,
            depth: None,
            centroid: None,
            measured,
            min,
            max,
        };
        let Some(room) = bath_rules::room_around(store, f) else { continue };
        // The room's height and the heater's power for its volume.
        if room.height < rules.ceiling_min - 1e-6 || room.height > rules.ceiling_max + 1e-6 {
            out.push(base(format!("The sauna round {what} is {} high; {} gives {}–{}", mm(room.height), rules.id, mm(rules.ceiling_min), mm(rules.ceiling_max)), None, Some(room.height), Some(rules.ceiling_min), Some(rules.ceiling_max)));
        }
        let settings = el.param(bath_rules::SAUNA_KEY).cloned().unwrap_or_default();
        let sizing = bath_rules::size_heater(&rules, room.area * room.height, settings["uninsulated"].as_f64().unwrap_or(0.0), settings["log"].as_bool().unwrap_or(false));
        if let Some(kw) = bath_rules::product_number(store, el, bath_rules::HEATER_POWER).map(|w| w / 1000.0) {
            let suits: Vec<&[f64; 3]> = rules.heaters.iter().filter(|h| (h[0] - kw).abs() < 1e-6).collect();
            let fits = suits.iter().any(|h| sizing.design_volume >= h[1] - 1e-9 && sizing.design_volume <= h[2] + 1e-9);
            if !suits.is_empty() && !fits {
                let h = suits[0];
                out.push(base(format!("{what} ({kw} kW) suits {}–{} m³; the room's design volume is {:.1} m³{}", h[1], h[2], sizing.design_volume, sizing.kw.map(|k| format!(" (a {k} kW heater)")).unwrap_or_default()), None, Some(sizing.design_volume), Some(h[1]), Some(h[2])));
            }
        }
        // The ceiling over the heater.
        if let (Some(need), Ok(g)) = (bath_rules::product_number(store, el, bath_rules::HEATER_CEILING), reviera_core::mesh_objects::regenerate(store, el)) {
            let top = g.placement.origin[2] + g.mesh.positions.iter().map(|q| f64::from(q[2]) * g.placement.scale).fold(f64::MIN, f64::max) - f.floor;
            if room.height - top < need - 1e-6 {
                out.push(base(format!("{what} has {} above it to the ceiling; its maker asks for {}", mm(room.height - top), mm(need)), None, Some(room.height - top), Some(need), None));
            }
        }
        // Doors swinging into the room.
        for s in &swings {
            let (mut c, mut n) = ([0.0, 0.0], 0.0);
            for q in &s.mesh.0 {
                c[0] += q[0];
                c[1] += q[1];
                n += 1.0;
            }
            if n > 0.0 && bath_rules::plan_distance(&room.outline, [c[0] / n, c[1] / n]) == 0.0 {
                out.push(base(format!("Door {} opens into the sauna round {what}; sauna doors open outward, with no lock", s.element.0), Some(s.element), None, None, None));
            }
        }
    }
    out
}
