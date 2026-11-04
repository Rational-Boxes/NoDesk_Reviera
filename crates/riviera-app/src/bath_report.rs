// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! The bathroom compliance report (ARC-BATH-019): Check Bathrooms' findings by room, written as
//! an HTML page (`bath.report`) or made into BCF topics (`bcf.from_bath_check`).

use std::collections::BTreeMap;

use riviera_core::bath_rules::{self, Confidence, Layer};
use riviera_core::issues::{self, Topic, TopicFields, TopicViewpoint};
use riviera_core::{Caller, CommandMeta, Effect, ElementId, Error, HostCommand, Result, bcf};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::App;
use crate::bathrooms::{CheckParams, Finding, FindingKind, Severity, check};

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The room a fixture stands in, as `number name`, or `No room`.
fn room_name(store: &riviera_core::Store, id: ElementId) -> String {
    riviera_core::mep_lighting::room_of(store, store.get(id).expect("a fixture"))
        .and_then(|r| store.get(r))
        .map(|r| {
            let (number, name) = riviera_core::rooms::number_and_name(r);
            format!("{number} {name}").trim().to_owned()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "No room".to_owned())
}

fn label(store: &riviera_core::Store, id: ElementId) -> String {
    store.get(id).map_or_else(
        || format!("#{}", id.0),
        |e| {
            let name = e.name.clone().filter(|n| !n.is_empty()).or_else(|| riviera_core::mesh_objects::type_of(store, e).and_then(|(t, _)| store.get(t)?.name.clone()));
            format!("{} {}", name.unwrap_or_else(|| e.class.trim_start_matches("Ifc").to_owned()), id.0)
        },
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A finding's value in its unit: falls and slopes in percent, loads in kPa, a sauna's volumes
/// in m³, else lengths in mm.
fn value(f: &Finding, v: Option<f64>) -> String {
    let Some(v) = v else { return String::new() };
    let slope = f.kind == FindingKind::Falls || (f.kind == FindingKind::Steam && f.message.contains("slopes"));
    if slope {
        format!("{:.1} %", v * 100.0)
    } else if f.kind == FindingKind::Load {
        format!("{v:.1} kPa")
    } else if f.kind == FindingKind::Sauna && f.message.contains("m³") {
        format!("{v:.1} m³")
    } else if f.kind == FindingKind::Access && f.min.is_none() && f.max.is_none() {
        String::new()
    } else {
        format!("{:.0} mm", v * 1000.0)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    /// Where to write the HTML report.
    #[schemars(extend("x-riviera-file" = "write"))]
    pub path: String,
    /// The report's title (default `Bathroom compliance`).
    pub title: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Report {
    pub path: String,
    pub errors: usize,
    pub notes: usize,
    /// The rooms with findings, in order.
    pub rooms: Vec<String>,
}

pub(crate) struct BathReport;

impl HostCommand for BathReport {
    type Host = App;
    type Params = ReportParams;
    type Output = Report;
    const META: CommandMeta = CommandMeta {
        name: "bath.report",
        version: 1,
        summary: "Write the bathroom compliance report",
        description: "Runs Check Bathrooms and writes its findings as an HTML report (ARC-BATH-019): the rule packs \
                      in use with their editions and layers, then each room's findings, errors first: the fixture, \
                      the rule with its pack, edition and layer, what was found, the value measured against the \
                      rule's range, the rule's confidence and its sources. Fixtures in no room are listed last.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ReportParams) -> Result<Report> {
        let findings = check(app, CheckParams::default())?;
        let doc = app.active().ok_or_else(|| bad("doc.none", "no model is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let packs = bath_rules::active(&store);
        let mut by_room: BTreeMap<String, Vec<&Finding>> = BTreeMap::new();
        for f in &findings {
            by_room.entry(room_name(&store, f.element)).or_default().push(f);
        }
        // Rooms by name, `No room` last.
        let mut rooms: Vec<String> = by_room.keys().filter(|r| *r != "No room").cloned().collect();
        if by_room.contains_key("No room") {
            rooms.push("No room".into());
        }
        let title = p.title.clone().unwrap_or_else(|| "Bathroom compliance".to_owned());
        let (errors, notes) = (findings.iter().filter(|f| f.severity == Severity::Error).count(), findings.len() - findings.iter().filter(|f| f.severity == Severity::Error).count());
        let mut h = String::new();
        h.push_str(&format!(
            "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>{t}</title><style>\
             body{{font-family:sans-serif;margin:2em;color:#222}}table{{border-collapse:collapse;width:100%;margin-bottom:1.5em}}\
             th,td{{border:1px solid #bbb;padding:.3em .5em;text-align:left;vertical-align:top;font-size:.9em}}th{{background:#eee}}\
             .error{{color:#a00;font-weight:bold}}.note{{color:#666}}</style></head><body>\n<h1>{t}</h1>\n<p>{e} error(s), {n} note(s) in {r} room(s).</p>\n",
            t = esc(&title),
            e = errors,
            n = notes,
            r = rooms.len()
        ));
        h.push_str("<h2>Rule packs</h2>\n<table><tr><th>Pack</th><th>Edition</th><th>Layer</th></tr>\n");
        for pk in &packs {
            let layer = match pk.layer {
                Layer::Minimum => "code minimum",
                Layer::Accessible => "accessibility",
                Layer::Recommended => "guidance",
            };
            h.push_str(&format!("<tr><td>{} ({})</td><td>{}</td><td>{layer}</td></tr>\n", esc(&pk.title), esc(&pk.id), esc(&pk.edition)));
        }
        h.push_str("</table>\n");
        if findings.is_empty() {
            h.push_str("<p>No findings.</p>\n");
        }
        for room in &rooms {
            let mut list = by_room[room].clone();
            list.sort_by_key(|f| (f.severity, f.element.0));
            h.push_str(&format!("<h2>{}</h2>\n<table><tr><th></th><th>Fixture</th><th>Rule</th><th>Finding</th><th>Measured</th><th>Range</th><th>Confidence</th><th>Source</th></tr>\n", esc(room)));
            for f in list {
                let (class, word) = if f.severity == Severity::Error { ("error", "Error") } else { ("note", "Note") };
                let range = match (f.min, f.max) {
                    (Some(a), Some(b)) => format!("{} – {}", value(f, Some(a)), value(f, Some(b))),
                    (Some(a), None) => format!("≥ {}", value(f, Some(a))),
                    (None, Some(b)) => format!("≤ {}", value(f, Some(b))),
                    (None, None) => String::new(),
                };
                let confidence = match f.confidence {
                    Confidence::Verified => "verified",
                    Confidence::Secondary => "secondary",
                    Confidence::Advisory => "advisory",
                };
                h.push_str(&format!(
                    "<tr><td class=\"{class}\">{word}</td><td>{}</td><td>{}<br><small>{} · {}</small></td><td>{}</td><td>{}</td><td>{}</td><td>{confidence}</td><td>{}</td></tr>\n",
                    esc(&label(&store, f.element)),
                    esc(&f.rule),
                    esc(&f.pack),
                    esc(&f.edition),
                    esc(&f.message),
                    value(f, f.measured.or(f.depth)),
                    esc(&range),
                    esc(&f.source),
                ));
            }
            h.push_str("</table>\n");
        }
        h.push_str("<p><small>Sources are the rule packs' source register entries (SOURCE_REGISTER.md). Advisory and secondary rules give notes; they are for checking, not for compliance on their own.</small></p>\n</body></html>\n");
        std::fs::write(&p.path, h).map_err(|e| bad("bath.report_write", format!("{}: {e}", p.path)))?;
        Ok(Report { path: p.path, errors, notes, rooms })
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FromCheckParams {
    /// `error` (the default): a topic per error; `all`: notes too.
    pub severity: Option<String>,
    /// Space round the fixture in its section box, metres (default 1).
    pub margin: Option<f64>,
    pub fields: Option<TopicFields>,
    pub author: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FromCheckReport {
    /// The topics made (their GUIDs).
    pub topic_ids: Vec<String>,
    /// Findings that already had a topic.
    pub already: usize,
    /// Open Bathroom topics whose findings have gone, now Resolved.
    pub resolved: Vec<String>,
}

pub(crate) struct BcfFromBathCheck;

impl HostCommand for BcfFromBathCheck {
    type Host = App;
    type Params = Option<FromCheckParams>;
    type Output = FromCheckReport;
    const META: CommandMeta = CommandMeta {
        name: "bcf.from_bath_check",
        version: 1,
        summary: "Make BCF issues from Check Bathrooms",
        description: "Runs Check Bathrooms and makes a BCF topic (ARC-BATH-019, COL-BCF-005) for each error (or each \
                      finding with `severity: \"all\"`): type Issue, status Open, titled by the rule and the fixture, \
                      described by the finding with its pack, edition, confidence and source, labelled Bathroom and \
                      with the pack's id, with a viewpoint boxed round the fixture (`margin`, default 1 m), the \
                      fixture and the element in its way selected, and its snapshot. A finding that already has a \
                      topic is skipped, and an open topic it made whose finding has gone is set Resolved. One \
                      undoable step.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: Option<FromCheckParams>) -> Result<FromCheckReport> {
        let p = p.unwrap_or_default();
        let all = match p.severity.as_deref() {
            None | Some("error") => false,
            Some("all") => true,
            Some(o) => return Err(bad("bcf.bad_severity", format!("severity is error or all, not {o}"))),
        };
        let margin = p.margin.unwrap_or(1.0).clamp(0.0, 50.0);
        let findings: Vec<Finding> = check(app, CheckParams::default())?.into_iter().filter(|f| all || f.severity == Severity::Error).collect();
        let doc = app.active().ok_or_else(|| bad("doc.none", "no model is open"))?;
        let store = app.get(doc)?.doc.store().clone();
        let existing: std::collections::HashSet<String> = issues::all(&store).into_iter().map(|(_, t)| t.guid.to_lowercase()).collect();
        let (mut topics, mut snapshots, mut made, mut already) = (Vec::new(), issues::Snapshots::new(), Vec::new(), 0);
        let guid_of = |id: ElementId| store.get(id).map_or_else(|| id.0.to_string(), |e| e.guid().to_string());
        let key_of = |f: &Finding| crate::bcf_topics::clash_guid(&[f.pack.as_str(), f.rule.as_str(), &guid_of(f.element), &f.other.map(guid_of).unwrap_or_default()]);
        // Every finding's topic GUID, notes included: a topic stays open while its finding does.
        let current: std::collections::HashSet<String> = check(app, CheckParams::default())?.iter().map(key_of).collect();
        let resolved: Vec<String> = issues::all(&store)
            .into_iter()
            .map(|(_, t)| t)
            .filter(|t| t.labels.iter().any(|l| l == "Bathroom") && t.status.as_deref() == Some("Open") && !current.contains(&t.guid.to_lowercase()))
            .map(|t| t.guid)
            .collect();
        for f in &findings {
            let other = f.other.map(guid_of).unwrap_or_default();
            let guid = key_of(f);
            if existing.contains(&guid) || made.contains(&guid) {
                already += 1;
                continue;
            }
            // The fixture's box, and the element in its way.
            let Some(fx) = store.get(f.element).and_then(|e| bath_rules::fixture(&store, e)) else { continue };
            let outline = bath_rules::outline(&fx);
            let (mut lo, mut hi) = ([f64::MAX, f64::MAX, fx.floor], [f64::MIN, f64::MIN, fx.floor + 2.0]);
            for q in &outline {
                lo = [lo[0].min(q[0]), lo[1].min(q[1]), lo[2]];
                hi = [hi[0].max(q[0]), hi[1].max(q[1]), hi[2]];
            }
            if let Some(c) = f.centroid {
                lo = [lo[0].min(c[0]), lo[1].min(c[1]), lo[2].min(c[2])];
                hi = [hi[0].max(c[0]), hi[1].max(c[1]), hi[2].max(c[2])];
            }
            let (lo, hi) = (lo.map(|v| v - margin), hi.map(|v| v + margin));
            let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
            let radius = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt() / 2.0;
            let dir = [1.0, -1.0, 0.8].map(|v: f64| v / (2.64f64).sqrt());
            let distance = radius.max(1.0) / (22.5f64.to_radians()).sin() * 1.1;
            let eye = [0, 1, 2].map(|k| centre[k] + dir[k] * distance);
            let section = riviera_core::views::SectionBox { enabled: true, transform: None, min: lo, max: hi, show_cut_faces: true };
            let mut selection = vec![guid_of(f.element)];
            if !other.is_empty() {
                selection.push(other.clone());
            }
            let vp = bcf::Viewpoint {
                guid: Some(issues::new_guid()),
                camera: Some(bcf::Camera::Perspective { eye, direction: dir.map(|v| -v), up: [0.0, 0.0, 1.0], field_of_view: 45.0, aspect_ratio: bcf::ASPECT }),
                clipping_planes: riviera_core::views::to_bcf(Some(&section), &[]),
                selection,
                visibility: bcf::Visibility::default(),
            };
            let mut t = Topic {
                guid: guid.clone(),
                title: format!("{}: {}", f.rule, label(&store, f.element)),
                description: Some(format!("{}\n{} ({}), {} rule; source {}.", f.message, f.pack, f.edition, match f.confidence {
                    Confidence::Verified => "verified",
                    Confidence::Secondary => "secondary",
                    Confidence::Advisory => "advisory",
                }, f.source)),
                topic_type: Some("Issue".into()),
                status: Some("Open".into()),
                labels: vec!["Bathroom".into(), f.pack.clone()],
                creation_date: issues::now(),
                creation_author: p.author.clone().unwrap_or_else(issues::default_author),
                ..Topic::default()
            };
            if let Some(fields) = p.fields.clone() {
                fields.apply_to(&mut t);
            }
            let vguid = vp.guid.clone().expect("set");
            if let Ok(img) = app.render_viewpoint(doc, &vp, 1024) {
                use base64::Engine;
                snapshots.insert(vguid.clone(), base64::engine::general_purpose::STANDARD.encode(img.png));
            }
            t.viewpoints.push(TopicViewpoint { guid: vguid, viewpoint: vp, snapshot: None });
            made.push(guid);
            topics.push(t);
        }
        if !topics.is_empty() {
            app.execute("bcf.merge_topics", json!({ "topics": topics, "snapshots": snapshots }), caller)?;
        }
        for g in &resolved {
            app.execute("bcf.update_topic", json!({ "topic_id": g, "fields": { "status": "Resolved" } }), caller)?;
        }
        Ok(FromCheckReport { topic_ids: made, already, resolved })
    }
}
