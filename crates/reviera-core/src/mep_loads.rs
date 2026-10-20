// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Load classifications and demand factors (MEP-ELEC-009, MEP-ELEC-010).
//!
//! - A **load classification** (Lighting, Power, HVAC, Motor, Other by default) groups loads for
//!   panel schedules and names the demand factor applied to them. Connectors name their
//!   classification (`electrical.load_classification`, matched without case); a name that is not
//!   defined counts as *Other*. The five defaults can be renamed but not deleted.
//! - A **demand factor** is constant, by quantity (the number of loads) or by load (their total,
//!   VA), each range at a percentage; *incremental* applies each range's percentage to the part
//!   of the quantity or load that falls in it, otherwise the whole total takes the percentage of
//!   the range it falls in. An optional largest-motor rule weights motor loads in descending
//!   order (for example 125 %, 100 %, 75 %).
//!
//! Both are kept in the electrical settings (`rxd:electrical` on the project).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::mep_electrical::{ElectricalSettings, save_settings, settings};
use crate::store::Store;
use crate::txn::Transaction;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The id of the default constant 100 % demand factor.
pub const FULL: &str = "constant_100";
/// The classification undefined names count as.
pub const OTHER: &str = "other";
const DEFAULTS: [&str; 5] = ["lighting", "power", "hvac", "motor", "other"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoadClassification {
    pub id: String,
    pub name: String,
    /// A demand factor's id.
    pub demand_factor: String,
    /// Counts towards spaces' electrical load densities.
    #[serde(default)]
    pub space_load: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    #[default]
    Constant,
    /// By the number of loads.
    Quantity,
    /// By the loads' total (VA).
    Load,
}

/// From `from` up to (not including) `to`, at `percent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Range {
    #[serde(default)]
    pub from: f64,
    /// None: no upper end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<f64>,
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DemandFactor {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub method: Method,
    /// A constant factor has one range (its percentage).
    pub ranges: Vec<Range>,
    #[serde(default)]
    pub incremental: bool,
    /// The largest-motor rule: motor loads in descending order take these percentages (the last
    /// for the rest); empty when off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub motor_percents: Vec<f64>,
}

pub(crate) fn default_classifications() -> Vec<LoadClassification> {
    ["Lighting", "Power", "HVAC", "Motor", "Other"]
        .iter()
        .zip(DEFAULTS)
        .map(|(n, id)| LoadClassification { id: id.into(), name: (*n).into(), demand_factor: FULL.into(), space_load: matches!(id, "lighting" | "power") })
        .collect()
}

pub(crate) fn default_demand_factors() -> Vec<DemandFactor> {
    vec![DemandFactor {
        id: FULL.into(),
        name: "100 %".into(),
        method: Method::Constant,
        ranges: vec![Range { from: 0.0, to: None, percent: 100.0 }],
        incremental: false,
        motor_percents: Vec::new(),
    }]
}

/// A load to be demanded: VA, and whether it is a motor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Load {
    pub va: f64,
    #[serde(default)]
    pub motor: bool,
}

fn in_range(r: &Range, x: f64) -> bool {
    x >= r.from && r.to.is_none_or(|t| x < t)
}

/// The demand of `loads` under a factor (VA).
pub fn evaluate(f: &DemandFactor, loads: &[Load]) -> f64 {
    let (motors, others): (Vec<Load>, Vec<Load>) = if f.motor_percents.is_empty() {
        (Vec::new(), loads.to_vec())
    } else {
        loads.iter().partition(|l| l.motor)
    };
    let mut motor_va: Vec<f64> = motors.iter().map(|l| l.va).collect();
    motor_va.sort_by(|a, b| b.total_cmp(a));
    let motor: f64 = motor_va.iter().enumerate().map(|(k, v)| v * f.motor_percents[k.min(f.motor_percents.len() - 1)] / 100.0).sum();
    let total: f64 = others.iter().map(|l| l.va).sum();
    let pct = |x: f64| f.ranges.iter().find(|r| in_range(r, x)).or(f.ranges.last()).map_or(100.0, |r| r.percent) / 100.0;
    let rest = match (f.method, f.incremental) {
        (Method::Constant, _) => total * f.ranges.first().map_or(100.0, |r| r.percent) / 100.0,
        (Method::Load, false) => total * pct(total),
        (Method::Load, true) => f
            .ranges
            .iter()
            .map(|r| {
                let hi = r.to.unwrap_or(f64::INFINITY).min(total);
                (hi - r.from).max(0.0) * r.percent / 100.0
            })
            .sum(),
        (Method::Quantity, false) => total * pct(others.len() as f64),
        (Method::Quantity, true) => {
            // The n-th largest load takes the percentage of the range n falls in.
            let mut va: Vec<f64> = others.iter().map(|l| l.va).collect();
            va.sort_by(|a, b| b.total_cmp(a));
            va.iter().enumerate().map(|(k, v)| v * pct((k + 1) as f64)).sum()
        }
    };
    rest + motor
}

/// The classification a connector's name counts as.
pub fn classify<'a>(s: &'a ElectricalSettings, name: &str) -> &'a LoadClassification {
    s.load_classifications
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name.trim()) || c.id == name.trim())
        .or_else(|| s.load_classifications.iter().find(|c| c.id == OTHER))
        .or(s.load_classifications.first())
        .expect("the defaults are kept")
}

fn check_ranges(method: Method, ranges: &[Range]) -> Result<()> {
    if ranges.is_empty() {
        return Err(bad("demand_factor.ranges", "a demand factor needs at least one range"));
    }
    if method == Method::Constant && ranges.len() != 1 {
        return Err(bad("demand_factor.ranges", "a constant demand factor has one percentage"));
    }
    for r in ranges {
        if !(0.0..=1000.0).contains(&r.percent) {
            return Err(bad("demand_factor.percent", "percentages run from 0 to 1000"));
        }
        if r.to.is_some_and(|t| t <= r.from) {
            return Err(bad("demand_factor.ranges", format!("the range from {} must end above where it starts", r.from)));
        }
    }
    Ok(())
}

fn slug(name: &str) -> String {
    name.trim().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect()
}

fn free_id(base: &str, taken: impl Fn(&str) -> bool) -> String {
    std::iter::once(base.to_owned()).chain((2..).map(|n| format!("{base}_{n}"))).find(|id| !taken(id)).expect("a free id")
}

fn check_name(s: &ElectricalSettings, name: &str, except: Option<&str>) -> Result<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 64 {
        return Err(bad("load_classification.name", "a load classification's name has 1 to 64 characters"));
    }
    if s.load_classifications.iter().any(|c| Some(c.id.as_str()) != except && c.name.eq_ignore_ascii_case(n)) {
        return Err(bad("load_classification.duplicate", format!("there is already a load classification named {n:?}")));
    }
    Ok(n.to_owned())
}

/// Renames the classification connectors name, on types and placed copies.
fn reclassify(txn: &mut Transaction<'_>, from: &[String], to: &str) -> Result<usize> {
    let store = txn.store().clone();
    let hit = |n: &str| from.iter().any(|f| f.eq_ignore_ascii_case(n.trim()));
    let mut n = 0;
    for e in store.iter() {
        if let Some(mut t) = crate::mesh_objects::mesh_object_type(e) {
            let mut changed = false;
            for c in &mut t.connectors {
                if let Some(el) = c.electrical.as_mut().filter(|el| hit(&el.load_classification)) {
                    el.load_classification = to.to_owned();
                    changed = true;
                    n += 1;
                }
            }
            if changed {
                crate::mep::write_type(txn, e.id(), &t)?;
            }
        }
        if let Some(v) = e.param(crate::mep::OVERRIDES_KEY) {
            let mut over: std::collections::BTreeMap<usize, crate::mep::ConnectorParams> = serde_json::from_value(v.clone()).unwrap_or_default();
            let mut changed = false;
            for o in over.values_mut() {
                if let Some(el) = o.electrical.as_mut().filter(|el| hit(&el.load_classification)) {
                    el.load_classification = to.to_owned();
                    changed = true;
                    n += 1;
                }
            }
            if changed {
                txn.set_param(e.id(), crate::mep::OVERRIDES_KEY, serde_json::to_value(&over).expect("serialises"))?;
            }
        }
    }
    Ok(n)
}

/// How many connectors (on types and copies' overrides) name a classification.
pub fn users(store: &Store, c: &LoadClassification) -> usize {
    let hit = |n: &str| n.trim().eq_ignore_ascii_case(&c.name) || n.trim() == c.id;
    let mut n = 0;
    for e in store.iter() {
        if let Some(t) = crate::mesh_objects::mesh_object_type(e) {
            n += t.connectors.iter().filter(|k| k.electrical.as_ref().is_some_and(|el| hit(&el.load_classification))).count();
        }
        if let Some(v) = e.param(crate::mep::OVERRIDES_KEY) {
            let over: std::collections::BTreeMap<usize, crate::mep::ConnectorParams> = serde_json::from_value(v.clone()).unwrap_or_default();
            n += over.values().filter(|o| o.electrical.as_ref().is_some_and(|el| hit(&el.load_classification))).count();
        }
    }
    n
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassificationCreateParams {
    pub name: String,
    /// A demand factor's id; default constant 100 %.
    pub demand_factor_id: Option<String>,
    #[serde(default)]
    pub space_load: bool,
}

fn check_factor(s: &ElectricalSettings, id: &str) -> Result<()> {
    if s.demand_factors.iter().any(|d| d.id == id) {
        Ok(())
    } else {
        Err(bad("demand_factor.not_found", format!("no demand factor {id}")))
    }
}

/// `load_classification.create`; returns its id.
pub(crate) fn create_classification(txn: &mut Transaction<'_>, p: ClassificationCreateParams) -> Result<String> {
    let mut s = settings(txn.store());
    if s.load_classifications.len() >= 256 {
        return Err(bad("load_classification.limit", "a project has at most 256 load classifications"));
    }
    let name = check_name(&s, &p.name, None)?;
    let df = p.demand_factor_id.unwrap_or_else(|| FULL.into());
    check_factor(&s, &df)?;
    let id = free_id(&slug(&name), |id| s.load_classifications.iter().any(|c| c.id == id));
    s.load_classifications.push(LoadClassification { id: id.clone(), name, demand_factor: df, space_load: p.space_load });
    save_settings(txn, &s)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassificationSetParams {
    pub id: String,
    pub name: Option<String>,
    pub demand_factor_id: Option<String>,
    pub space_load: Option<bool>,
}

/// `load_classification.set`: renaming also renames it on every connector that names it.
pub(crate) fn set_classification(txn: &mut Transaction<'_>, p: ClassificationSetParams) -> Result<()> {
    let mut s = settings(txn.store());
    let at = s
        .load_classifications
        .iter()
        .position(|c| c.id == p.id)
        .ok_or_else(|| bad("load_classification.not_found", format!("no load classification {}", p.id)))?;
    if let Some(df) = &p.demand_factor_id {
        check_factor(&s, df)?;
        s.load_classifications[at].demand_factor = df.clone();
    }
    if let Some(sl) = p.space_load {
        s.load_classifications[at].space_load = sl;
    }
    if let Some(n) = &p.name {
        let n = check_name(&s, n, Some(&p.id))?;
        let old = std::mem::replace(&mut s.load_classifications[at].name, n.clone());
        if !old.eq_ignore_ascii_case(&n) {
            reclassify(txn, &[old], &n)?;
        }
    }
    save_settings(txn, &s)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassificationDeleteParams {
    pub id: String,
    /// The classification its connectors take instead (required when connectors use it).
    pub replace_with: Option<String>,
}

/// `load_classification.delete`; returns how many connectors moved to the replacement.
pub(crate) fn delete_classification(txn: &mut Transaction<'_>, p: ClassificationDeleteParams) -> Result<usize> {
    let mut s = settings(txn.store());
    let c = s
        .load_classifications
        .iter()
        .find(|c| c.id == p.id)
        .cloned()
        .ok_or_else(|| bad("load_classification.not_found", format!("no load classification {}", p.id)))?;
    if DEFAULTS.contains(&c.id.as_str()) {
        return Err(bad("load_classification.default", format!("{} is a default load classification: it can be renamed but not deleted", c.name)));
    }
    let used = users(txn.store(), &c);
    let moved = match (&p.replace_with, used) {
        (_, 0) => 0,
        (None, n) => {
            return Err(bad("load_classification.in_use", format!("{n} connector(s) use {}; choose a classification to replace it", c.name)));
        }
        (Some(r), _) => {
            let to = s
                .load_classifications
                .iter()
                .find(|x| x.id == *r && x.id != c.id)
                .ok_or_else(|| bad("load_classification.not_found", format!("no other load classification {r}")))?
                .name
                .clone();
            reclassify(txn, &[c.name.clone(), c.id.clone()], &to)?
        }
    };
    s.load_classifications.retain(|x| x.id != c.id);
    save_settings(txn, &s)?;
    Ok(moved)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DemandFactorParams {
    pub name: String,
    pub method: Method,
    pub ranges: Vec<Range>,
    #[serde(default)]
    pub incremental: bool,
    #[serde(default)]
    pub motor_percents: Vec<f64>,
}

/// `demand_factor.create`; returns its id.
pub(crate) fn create_demand_factor(txn: &mut Transaction<'_>, p: DemandFactorParams) -> Result<String> {
    let mut s = settings(txn.store());
    let name = p.name.trim().to_owned();
    if name.is_empty() {
        return Err(bad("demand_factor.name", "a demand factor needs a name"));
    }
    if s.demand_factors.iter().any(|d| d.name.eq_ignore_ascii_case(&name)) {
        return Err(bad("demand_factor.duplicate", format!("there is already a demand factor named {name:?}")));
    }
    check_ranges(p.method, &p.ranges)?;
    let id = free_id(&slug(&name), |id| s.demand_factors.iter().any(|d| d.id == id));
    s.demand_factors.push(DemandFactor { id: id.clone(), name, method: p.method, ranges: p.ranges, incremental: p.incremental, motor_percents: p.motor_percents });
    save_settings(txn, &s)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DemandFactorSetParams {
    pub id: String,
    pub name: Option<String>,
    pub method: Option<Method>,
    pub ranges: Option<Vec<Range>>,
    pub incremental: Option<bool>,
    pub motor_percents: Option<Vec<f64>>,
}

/// `demand_factor.set`.
pub(crate) fn set_demand_factor(txn: &mut Transaction<'_>, p: DemandFactorSetParams) -> Result<()> {
    let mut s = settings(txn.store());
    let others: Vec<String> = s.demand_factors.iter().filter(|d| d.id != p.id).map(|d| d.name.clone()).collect();
    let d = s.demand_factors.iter_mut().find(|d| d.id == p.id).ok_or_else(|| bad("demand_factor.not_found", format!("no demand factor {}", p.id)))?;
    if let Some(n) = p.name.map(|n| n.trim().to_owned()) {
        if n.is_empty() || others.iter().any(|o| o.eq_ignore_ascii_case(&n)) {
            return Err(bad("demand_factor.duplicate", format!("the name {n:?} is empty or taken")));
        }
        d.name = n;
    }
    if let Some(m) = p.method {
        d.method = m;
    }
    if let Some(r) = p.ranges {
        d.ranges = r;
    }
    if let Some(i) = p.incremental {
        d.incremental = i;
    }
    if let Some(m) = p.motor_percents {
        d.motor_percents = m;
    }
    check_ranges(d.method, &d.ranges)?;
    save_settings(txn, &s)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DemandFactorDeleteParams {
    pub id: String,
}

/// `demand_factor.delete`: refused while a classification uses it, and for the default.
pub(crate) fn delete_demand_factor(txn: &mut Transaction<'_>, p: DemandFactorDeleteParams) -> Result<()> {
    let mut s = settings(txn.store());
    if p.id == FULL {
        return Err(bad("demand_factor.default", "the 100 % demand factor is kept"));
    }
    if let Some(c) = s.load_classifications.iter().find(|c| c.demand_factor == p.id) {
        return Err(bad("demand_factor.in_use", format!("{} uses it; give it another demand factor first", c.name)));
    }
    let n = s.demand_factors.len();
    s.demand_factors.retain(|d| d.id != p.id);
    if s.demand_factors.len() == n {
        return Err(bad("demand_factor.not_found", format!("no demand factor {}", p.id)));
    }
    save_settings(txn, &s)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluateParams {
    pub id: String,
    /// VA each (numbers), or {va, motor}.
    pub loads: Vec<LoadArg>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum LoadArg {
    Va(f64),
    Load(Load),
}

/// `demand_factor.evaluate`: the demand (VA).
pub(crate) fn evaluate_command(store: &Store, p: EvaluateParams) -> Result<f64> {
    let s = settings(store);
    let d = s.demand_factors.iter().find(|d| d.id == p.id).ok_or_else(|| bad("demand_factor.not_found", format!("no demand factor {}", p.id)))?;
    let loads: Vec<Load> = p.loads.into_iter().map(|l| match l {
        LoadArg::Va(va) => Load { va, motor: false },
        LoadArg::Load(l) => l,
    }).collect();
    Ok(evaluate(d, &loads))
}

/// A load classification as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ClassificationInfo {
    #[serde(flatten)]
    pub classification: LoadClassification,
    /// The demand factor's name.
    pub demand_factor_name: String,
    /// Connectors (on types and copies) that name it.
    pub connectors: usize,
    /// A default: it can be renamed but not deleted.
    pub default: bool,
}

/// `load_classification.list`.
pub(crate) fn list_classifications(store: &Store, _: crate::mep_electrical::Empty) -> Result<Vec<ClassificationInfo>> {
    let s = settings(store);
    Ok(s.load_classifications
        .iter()
        .map(|c| ClassificationInfo {
            demand_factor_name: s.demand_factors.iter().find(|d| d.id == c.demand_factor).map_or_else(|| c.demand_factor.clone(), |d| d.name.clone()),
            connectors: users(store, c),
            default: DEFAULTS.contains(&c.id.as_str()),
            classification: c.clone(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn df(method: Method, incremental: bool, ranges: &[(f64, Option<f64>, f64)]) -> DemandFactor {
        DemandFactor {
            id: "t".into(),
            name: "t".into(),
            method,
            ranges: ranges.iter().map(|&(from, to, percent)| Range { from, to, percent }).collect(),
            incremental,
            motor_percents: Vec::new(),
        }
    }

    fn va(v: &[f64]) -> Vec<Load> {
        v.iter().map(|&va| Load { va, motor: false }).collect()
    }

    #[test]
    fn the_methods() {
        // MEP-ELEC-010's acceptance: incremental by load, 10 kVA at 100 % then 50 %: 25 → 17.5.
        let inc = df(Method::Load, true, &[(0.0, Some(10_000.0), 100.0), (10_000.0, None, 50.0)]);
        assert!((evaluate(&inc, &va(&[25_000.0])) - 17_500.0).abs() < 1e-9);
        // Constant 80 %: 10 → 8.
        assert!((evaluate(&df(Method::Constant, false, &[(0.0, None, 80.0)]), &va(&[4_000.0, 6_000.0])) - 8_000.0).abs() < 1e-9);
        // By quantity, whole: 1–2 items 100 %, 3 and more 65 %.
        let q = df(Method::Quantity, false, &[(1.0, Some(3.0), 100.0), (3.0, None, 65.0)]);
        assert!((evaluate(&q, &va(&[1000.0, 1000.0])) - 2000.0).abs() < 1e-9);
        assert!((evaluate(&q, &va(&[1000.0, 1000.0, 1000.0, 1000.0])) - 2600.0).abs() < 1e-9);
        // Incremental by quantity: the two largest at 100 %, the rest at 65 %.
        let qi = df(Method::Quantity, true, &[(1.0, Some(3.0), 100.0), (3.0, None, 65.0)]);
        assert!((evaluate(&qi, &va(&[500.0, 2000.0, 1000.0, 1000.0])) - (3000.0 + 0.65 * 1500.0)).abs() < 1e-9);
        // Whole by load: 25 kVA falls in the second range.
        assert!((evaluate(&df(Method::Load, false, &[(0.0, Some(10_000.0), 100.0), (10_000.0, None, 50.0)]), &va(&[25_000.0])) - 12_500.0).abs() < 1e-9);
    }

    #[test]
    fn the_largest_motor_rule() {
        let mut f = df(Method::Constant, false, &[(0.0, None, 100.0)]);
        f.motor_percents = vec![125.0, 100.0, 75.0];
        let loads = [Load { va: 2000.0, motor: true }, Load { va: 5000.0, motor: true }, Load { va: 1000.0, motor: true }, Load { va: 800.0, motor: true }, Load { va: 3000.0, motor: false }];
        // 5000 × 1.25 + 2000 × 1.0 + 1000 × 0.75 + 800 × 0.75 + 3000.
        assert!((evaluate(&f, &loads) - (6250.0 + 2000.0 + 750.0 + 600.0 + 3000.0)).abs() < 1e-9);
    }
}
