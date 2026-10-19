// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Connection automation (STR-STL-004): placing library connections on many joins at once from
//! rules. Joins are found from the framing: a column's foot (where no column stands below it), a
//! beam's end on the column or girder it rests on, and a brace's end at the column or beam its
//! axis meets. Each join gets the first rule that matches its kind, its members' sections and the
//! connected member's depth; a rule with no depth range takes its type's range of applicability.
//! Only steel members are connected. Joins that already have a connection on the same members are
//! skipped, or, with `replace_generic`, a generic one is given the matched type. A type whose
//! generator fails leaves a generic connection marked Failed. A preview changes nothing; its
//! report can be applied later as it stands.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::columns::{MaterialKind, Section};
use crate::connections::{self, CodeCheck};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::element::Element;
use crate::store::Store;
use crate::walls::RECIPE_KEY;

/// The project's rule sets.
pub const RULES_KEY: &str = "rxd:connection_rules";
/// The project's automation runs.
pub const RUNS_KEY: &str = "rxd:connection_runs";

type V3 = [f64; 3];

/// What kind of join: a column's foot, a beam's end at a column, a beam's end on a beam, a brace's
/// end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JoinKind {
    ColumnBase,
    BeamColumn,
    BeamBeam,
    Brace,
}

/// A rule: a connection type for joins of a kind, optionally limited to sections and a depth
/// range of the connected member (the column, for a column's foot).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rule {
    pub type_id: String,
    pub join: JoinKind,
    /// The main member's sections (any when empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub main_sections: Vec<Section>,
    /// The connected member's sections (any when empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<Section>,
    /// The connected member's depth range (metres); the type's range of applicability when
    /// neither is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth_min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth_max: Option<f64>,
}

/// An ordered list of rules: the first that matches a join gives its type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuleSet {
    pub name: String,
    pub rules: Vec<Rule>,
}

/// A type's range of applicability: the connected member's depth (metres).
pub fn applicability(type_id: &str) -> Option<(f64, f64)> {
    match type_id {
        // A single plate takes beams from 6" to 36" deep.
        "shear_plate" => Some((0.15, 0.92)),
        // An extended end plate takes beams from 8" to 36" deep.
        "end_plate" => Some((0.20, 0.92)),
        _ => None,
    }
}

fn rule(type_id: &str, join: JoinKind) -> Rule {
    Rule {
        type_id: type_id.into(),
        join,
        main_sections: Vec::new(),
        sections: Vec::new(),
        depth_min: None,
        depth_max: None,
    }
}

/// The default rule set: base plates under columns, shear plates where beams meet columns or
/// girders, gussets at braces' ends.
pub fn default_rules() -> RuleSet {
    RuleSet {
        name: "Default".into(),
        rules: vec![
            rule("base_plate", JoinKind::ColumnBase),
            rule("shear_plate", JoinKind::BeamColumn),
            rule("shear_plate", JoinKind::BeamBeam),
            rule("gusset", JoinKind::Brace),
        ],
    }
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// The default rule set and the project's.
pub fn rule_sets(store: &Store) -> Vec<RuleSet> {
    let mut out = vec![default_rules()];
    if let Some(v) = project(store).and_then(|p| p.param(RULES_KEY)) {
        out.extend(serde_json::from_value::<Vec<RuleSet>>(v.clone()).unwrap_or_default());
    }
    out
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetRuleSetParams {
    pub name: String,
    /// Its rules; null removes the rule set.
    pub rules: Option<Vec<Rule>>,
}

/// Saves or removes one of the project's rule sets (`structural_connection.set_rule_set`).
pub(crate) fn set_rule_set(txn: &mut crate::txn::Transaction<'_>, p: SetRuleSetParams) -> Result<usize> {
    if p.name == "Default" {
        return Err(Error::new("connection.rules", "the Default rule set is built in"));
    }
    let store = txn.store().clone();
    let pid = project(&store)
        .map(|e| e.id())
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let mut sets: Vec<RuleSet> = rule_sets(&store).into_iter().skip(1).collect();
    sets.retain(|s| s.name != p.name);
    if let Some(rules) = p.rules {
        for r in &rules {
            if crate::steel_fab::connection_type(&r.type_id).is_none() {
                return Err(Error::new("connection.unknown_type", format!("no connection type {}", r.type_id)));
            }
        }
        sets.push(RuleSet { name: p.name, rules });
    }
    let n = sets.len();
    txn.set_param(pid, RULES_KEY, serde_json::to_value(&sets).expect("serialises"))?;
    Ok(n)
}

// ---- Joins ----

/// A join found in the framing: its kind, where it is and its members, the main member first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Join {
    pub kind: JoinKind,
    pub point: V3,
    pub members: Vec<ElementId>,
}

/// A member's section, depth and whether it is steel.
fn section_of(store: &Store, id: ElementId) -> Option<(Section, f64, bool)> {
    let e = store.get(id)?;
    let ty = if crate::columns::column_recipe(e).is_some() {
        crate::columns::regenerate(store, e).ok()?.ty
    } else {
        crate::beams::regenerate(store, e).ok()?.ty
    };
    let steel = match ty.material_kind {
        Some(k) => k == MaterialKind::Steel,
        None => !matches!(ty.section, Section::Rectangular | Section::Round),
    };
    Some((ty.section, ty.depth, steel))
}

fn dist(a: V3, b: V3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn dist_to_segment(p: V3, (a, b): (V3, V3)) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l2: f64 = ab.iter().map(|x| x * x).sum();
    let t = if l2 < 1e-18 {
        0.0
    } else {
        (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2]) / l2).clamp(0.0, 1.0)
    };
    dist(p, [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t])
}

/// Every join in the framing, or those with a member in `scope`.
pub fn joins(store: &Store, scope: Option<&[ElementId]>) -> Vec<Join> {
    let mut out = Vec::new();
    let columns: Vec<(ElementId, (V3, V3))> = store
        .iter()
        .filter(|e| crate::columns::column_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), crate::columns::regenerate(store, e).ok()?.axis())))
        .collect();
    // Columns' feet, unless a column stands below.
    for (c, (base, _)) in &columns {
        let on_column = columns
            .iter()
            .any(|(o, (_, top))| o != c && dist(*top, *base) < 0.05);
        if !on_column {
            out.push(Join { kind: JoinKind::ColumnBase, point: *base, members: vec![*c] });
        }
    }
    let beams = crate::beams::all(store);
    let braces: Vec<ElementId> = store
        .iter()
        .filter(|e| crate::beams::beam_recipe(e).is_some_and(|r| r.brace))
        .map(|e| e.id())
        .collect();
    for (b, g) in &beams {
        if braces.contains(b) {
            // A brace's ends: at the column, else the beam, its axis meets.
            for p in [g.a, g.b] {
                let near = |(id, ax): &(ElementId, (V3, V3))| {
                    let d = dist_to_segment(p, *ax);
                    (d <= 0.5).then_some((*id, d))
                };
                let column = columns.iter().filter_map(near).min_by(|x, y| x.1.total_cmp(&y.1));
                let beam = || {
                    beams
                        .iter()
                        .filter(|(o, _)| o != b && !braces.contains(o))
                        .map(|(o, og)| (*o, (og.a, og.b)))
                        .filter_map(|x| near(&x))
                        .min_by(|x, y| x.1.total_cmp(&y.1))
                };
                if let Some((m, _)) = column.or_else(beam) {
                    out.push(Join { kind: JoinKind::Brace, point: p, members: vec![m, *b] });
                }
            }
            continue;
        }
        for (k, s) in g.supports.iter().enumerate() {
            let p = if k == 0 { g.a } else { g.b };
            match s {
                Some(crate::beams::Support::Column(c)) => {
                    out.push(Join { kind: JoinKind::BeamColumn, point: p, members: vec![*c, *b] })
                }
                Some(crate::beams::Support::Beam(m)) => {
                    out.push(Join { kind: JoinKind::BeamBeam, point: p, members: vec![*m, *b] })
                }
                _ => {}
            }
        }
    }
    if let Some(scope) = scope {
        out.retain(|j| j.members.iter().any(|m| scope.contains(m)));
    }
    out
}

// ---- Automation ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Existing {
    /// Joins that have a connection are left alone.
    #[default]
    Skip,
    /// A generic connection on a join is given the matched type.
    ReplaceGeneric,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AutomateParams {
    /// Members whose joins are automated (every join when omitted).
    #[serde(default)]
    pub element_ids: Option<Vec<ElementRef>>,
    /// A rule set by name (`Default` when omitted and no `rules`).
    #[serde(default)]
    pub rule_set: Option<String>,
    /// Rules given here, in place of a rule set.
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub existing: Existing,
    /// Report what would be done without changing the model.
    #[serde(default)]
    pub preview: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Placed,
    Skipped,
    Failed,
}

/// A join's line in the report.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JoinReport {
    pub kind: JoinKind,
    pub point: V3,
    pub members: Vec<ElementId>,
    /// The type matched (or given), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_id: Option<String>,
    pub status: Status,
    pub reason: String,
    /// The connection placed or retyped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<ElementId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    pub rule_set: String,
    pub preview: bool,
    pub existing: Existing,
    pub placed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub joins: Vec<JoinReport>,
}

fn matches(store: &Store, r: &Rule, j: &Join) -> bool {
    if r.join != j.kind {
        return false;
    }
    let main = section_of(store, j.members[0]);
    let other = section_of(store, *j.members.last().expect("a member"));
    if !r.main_sections.is_empty() && !main.is_some_and(|m| r.main_sections.contains(&m.0)) {
        return false;
    }
    if !r.sections.is_empty() && !other.is_some_and(|m| r.sections.contains(&m.0)) {
        return false;
    }
    let (lo, hi) = match (r.depth_min, r.depth_max) {
        (None, None) => applicability(&r.type_id).unwrap_or((0.0, f64::INFINITY)),
        (lo, hi) => (lo.unwrap_or(0.0), hi.unwrap_or(f64::INFINITY)),
    };
    other.is_some_and(|m| m.1 >= lo - 1e-6 && m.1 <= hi + 1e-6)
}

/// The connection already on exactly these members, with its type.
fn existing(store: &Store, members: &[ElementId]) -> Option<(ElementId, String)> {
    let mut want: Vec<String> = members
        .iter()
        .filter_map(|m| store.get(*m).map(|e| e.guid().to_string()))
        .collect();
    want.sort();
    store.iter().find_map(|e| {
        let c = connections::recipe(e)?;
        let mut have = c.members.clone();
        have.sort();
        (have == want).then(|| (e.id(), c.connection_type))
    })
}

/// What automation would do at each join.
fn plan(store: &Store, p: &AutomateParams) -> Result<Report> {
    let (name, rules) = match (&p.rules, &p.rule_set) {
        (Some(r), _) => ("(given rules)".to_owned(), r.clone()),
        (None, name) => {
            let name = name.clone().unwrap_or_else(|| "Default".into());
            let set = rule_sets(store)
                .into_iter()
                .find(|s| s.name == name)
                .ok_or_else(|| Error::new("connection.rules", format!("no rule set named {name}")))?;
            (name, set.rules)
        }
    };
    let scope = p
        .element_ids
        .as_ref()
        .map(|refs| {
            refs.iter()
                .map(|r| {
                    store
                        .resolve(r)
                        .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;
    let mut lines = Vec::new();
    for j in joins(store, scope.as_deref()) {
        let line = |type_id: Option<String>, status: Status, reason: &str| JoinReport {
            kind: j.kind,
            point: j.point,
            members: j.members.clone(),
            type_id,
            status,
            reason: reason.into(),
            connection: None,
        };
        if j.members.iter().any(|m| !section_of(store, *m).is_some_and(|s| s.2)) {
            lines.push(line(None, Status::Skipped, "not all members are steel"));
            continue;
        }
        let Some(r) = rules.iter().find(|r| matches(store, r, &j)) else {
            lines.push(line(None, Status::Skipped, "no rule matches"));
            continue;
        };
        let ty = Some(r.type_id.clone());
        match existing(store, &j.members) {
            Some((_, t)) if t != "generic" => {
                lines.push(line(ty, Status::Skipped, &format!("already connected ({t})")))
            }
            Some(_) if p.existing == Existing::Skip => {
                lines.push(line(ty, Status::Skipped, "already connected (generic)"))
            }
            Some(_) => lines.push(line(ty, Status::Placed, "generic connection given the type")),
            None => lines.push(line(ty, Status::Placed, "placed")),
        }
    }
    Ok(tally(Report {
        rule_set: name,
        preview: p.preview,
        existing: p.existing,
        placed: 0,
        skipped: 0,
        failed: 0,
        joins: lines,
    }))
}

fn tally(mut r: Report) -> Report {
    let n = |s: Status| r.joins.iter().filter(|j| j.status == s).count();
    (r.placed, r.skipped, r.failed) = (n(Status::Placed), n(Status::Skipped), n(Status::Failed));
    r
}

/// Places the report's planned connections; failures become generic connections marked Failed.
fn apply(txn: &mut crate::txn::Transaction<'_>, mut report: Report) -> Result<Report> {
    for line in report.joins.iter_mut().filter(|l| l.status == Status::Placed) {
        let Some(ty) = line.type_id.clone() else { continue };
        let store = txn.store().clone();
        if line.members.iter().any(|m| store.get(*m).is_none()) {
            line.status = Status::Skipped;
            line.reason = "a member is gone".into();
            continue;
        }
        let members: Vec<ElementRef> = line.members.iter().map(|m| ElementRef::Id(*m)).collect();
        let done = match existing(&store, &line.members) {
            Some((c, t)) if t == "generic" => crate::steel_fab::set_type(
                txn,
                crate::steel_fab::SetTypeParams { connection_ids: vec![ElementRef::Id(c)], connection_type: ty.clone() },
            )
            .map(|_| c),
            Some((_, t)) => {
                line.status = Status::Skipped;
                line.reason = format!("already connected ({t})");
                continue;
            }
            None => connections::create(
                txn,
                connections::CreateParams { member_ids: members.clone(), type_id: Some(ty.clone()) },
            ),
        };
        match done {
            Ok(c) => line.connection = Some(c),
            Err(e) => {
                // The generator failed: a generic connection, marked Failed, holds the join.
                line.status = Status::Failed;
                line.reason = e.message.clone();
                let store = txn.store().clone();
                let c = match existing(&store, &line.members) {
                    Some((c, t)) => {
                        if t != "generic" {
                            crate::steel_fab::set_type(
                                txn,
                                crate::steel_fab::SetTypeParams {
                                    connection_ids: vec![ElementRef::Id(c)],
                                    connection_type: "generic".into(),
                                },
                            )?;
                        }
                        Some(c)
                    }
                    None if line.members.len() >= 2 => connections::create(
                        txn,
                        connections::CreateParams { member_ids: members, type_id: None },
                    )
                    .ok(),
                    None => None,
                };
                if let Some(c) = c {
                    mark_failed(txn, c)?;
                    line.connection = Some(c);
                }
            }
        }
    }
    let report = tally(Report { preview: false, ..report });
    log_run(txn, &report)?;
    Ok(report)
}

fn mark_failed(txn: &mut crate::txn::Transaction<'_>, c: ElementId) -> Result<()> {
    let store = txn.store().clone();
    let Some(mut r) = store.get(c).and_then(connections::recipe) else { return Ok(()) };
    r.code_check = CodeCheck::Failed;
    txn.set_param(c, RECIPE_KEY, serde_json::to_value(&r).expect("serialises"))
}

/// A run kept with the project: when, the rule set and its counts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Run {
    pub at: String,
    pub rule_set: String,
    pub placed: usize,
    pub skipped: usize,
    pub failed: usize,
}

pub fn runs(store: &Store) -> Vec<Run> {
    project(store)
        .and_then(|p| p.param(RUNS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn log_run(txn: &mut crate::txn::Transaction<'_>, r: &Report) -> Result<()> {
    let store = txn.store().clone();
    let Some(pid) = project(&store).map(|e| e.id()) else { return Ok(()) };
    let mut all = runs(&store);
    all.push(Run {
        at: crate::overlap_accept::now_rfc3339(),
        rule_set: r.rule_set.clone(),
        placed: r.placed,
        skipped: r.skipped,
        failed: r.failed,
    });
    txn.set_param(pid, RUNS_KEY, serde_json::to_value(&all).expect("serialises"))
}

/// Automates connections (`structural_connection.automate`): a preview reports, otherwise the
/// connections are placed in one undo step.
pub(crate) fn automate(txn: &mut crate::txn::Transaction<'_>, p: AutomateParams) -> Result<Report> {
    let store = txn.store().clone();
    let report = plan(&store, &p)?;
    if p.preview {
        return Ok(report);
    }
    apply(txn, report)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ApplyParams {
    /// A preview's report, as `structural_connection.automate` returned it.
    pub report: Report,
}

/// Applies a preview's report as it stands (`structural_connection.apply_automation`).
pub(crate) fn apply_report(txn: &mut crate::txn::Transaction<'_>, p: ApplyParams) -> Result<Report> {
    apply(txn, p.report)
}

/// The report as CSV: one row per join with its status and reason.
pub fn csv(r: &Report) -> String {
    let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut out = String::from("kind,x,y,z,members,type,status,reason,connection\n");
    for j in &r.joins {
        let kind = serde_json::to_value(j.kind).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        let status = serde_json::to_value(j.status).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        let members: Vec<String> = j.members.iter().map(|m| m.0.to_string()).collect();
        out.push_str(&format!(
            "{kind},{:.3},{:.3},{:.3},{},{},{status},{},{}\n",
            j.point[0],
            j.point[1],
            j.point[2],
            q(&members.join(" ")),
            j.type_id.as_deref().unwrap_or(""),
            q(&j.reason),
            j.connection.map(|c| c.0.to_string()).unwrap_or_default(),
        ));
    }
    out
}

/// The rule sets and runs (`structural_connection.rule_sets`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RulesInfo {
    pub rule_sets: Vec<RuleSet>,
    pub ranges: BTreeMap<String, (f64, f64)>,
    pub runs: Vec<Run>,
}

pub fn rules_info(store: &Store) -> RulesInfo {
    RulesInfo {
        rule_sets: rule_sets(store),
        ranges: crate::steel_fab::library()
            .iter()
            .filter_map(|t| Some((t.id.to_owned(), applicability(t.id)?)))
            .collect(),
        runs: runs(store),
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CsvParams {
    pub report: Report,
}

pub fn report_csv(p: CsvParams) -> Value {
    Value::String(csv(&p.report))
}
