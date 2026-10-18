// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Loads, load cases, natures and combinations (STR-LOAD-001, STR-LC-001) and point boundary
//! conditions (STR-BC-001) on the analytical model.
//!
//! **Natures** and the code template they came from are kept on the project (`rxd:analysis`).
//! **Load cases** are `IfcStructuralLoadCase` elements (`recipe.load_case`: nature, category,
//! self-weight); **combinations** are `IfcStructuralLoadGroup` elements (`LOAD_COMBINATION`,
//! `recipe.load_combination`: formula, limit state, usages) whose formula is resolved to a
//! factor per case, nested combinations expanded and circular references refused. Templates
//! (`data/load_templates.json`: ASCE 7-22 strength combinations, EN 1990 expression 6.10 with the
//! recommended factors) generate combinations offline.
//!
//! **Loads** are actions with exactly one analytical host, deleted with it or with their case:
//! a point load (`IfcStructuralPointAction`) on a node or at a ratio along a member, and a line
//! load (`IfcStructuralCurveAction`) along a member, uniform or varying linearly. Forces are in N
//! (line loads N/m) and moments in N·m, in project axes.
//!
//! A **boundary condition** at a node (Fixed, Pinned, Roller or User, each degree of freedom
//! fixed, released or a spring) is kept on the node and written as its `AppliedCondition`.

use std::collections::{BTreeMap, HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

pub const ANALYSIS_KEY: &str = "rxd:analysis";
pub const CASE_RECIPE: &str = "recipe.load_case";
pub const COMBINATION_RECIPE: &str = "recipe.load_combination";
pub const POINT_LOAD_RECIPE: &str = "recipe.load_point";
pub const LINE_LOAD_RECIPE: &str = "recipe.load_line";
pub const AREA_LOAD_RECIPE: &str = "recipe.load_area";
const TEMPLATES: &str = include_str!("../data/load_templates.json");

// ---- Natures and templates ----

/// A load nature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Nature {
    pub name: String,
    /// The short name formulas and templates use (`D`, `L`, `G`, `Q` …).
    pub symbol: String,
    /// `permanent`, `variable` or `extraordinary`.
    pub action_type: String,
    /// The IFC action source (`DEAD_LOAD_G`, `LIVE_LOAD_Q`, `SNOW_S`, `WIND_W` …).
    pub action_source: String,
    /// EN 1990 combination factors, where the template gives them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psi0: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psi1: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psi2: Option<f64>,
    /// `code` (from a template) or `user`.
    #[serde(default = "user")]
    pub source: String,
}

fn user() -> String {
    "user".into()
}

/// A combination rule of a template: terms of nature symbols (alternatives as `Lr|S|R`, one
/// combination each) and factors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rule {
    pub name: String,
    pub limit_state: String,
    pub terms: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Eurocode {
    pub gamma_g_sup: f64,
    pub gamma_g_inf: f64,
    pub gamma_q: f64,
}

/// A code template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub regions: Vec<String>,
    #[serde(default)]
    pub source_ids: Vec<String>,
    pub natures: Vec<Nature>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eurocode: Option<Eurocode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Deserialize)]
struct TemplateFile {
    templates: Vec<Template>,
}

/// The templates shipped with Reviera.
pub fn templates() -> Vec<Template> {
    serde_json::from_str::<TemplateFile>(TEMPLATES)
        .map(|f| f.templates)
        .unwrap_or_default()
}

/// The project's analysis settings: its template and natures.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Settings {
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub natures: Vec<Nature>,
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// The project's analysis settings; without any, the region's template's natures.
pub fn settings(store: &Store) -> Settings {
    if let Some(s) = project(store)
        .and_then(|p| p.param(ANALYSIS_KEY))
        .and_then(|v| serde_json::from_value::<Settings>(v.clone()).ok())
        .filter(|s| !s.natures.is_empty())
    {
        return s;
    }
    let t = region_template(store);
    Settings {
        template: t.as_ref().map(|t| t.id.clone()),
        natures: t
            .map(|t| {
                t.natures
                    .into_iter()
                    .map(|n| Nature {
                        source: "code".into(),
                        ..n
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn region_template(store: &Store) -> Option<Template> {
    let region = serde_json::to_value(crate::codes::settings(store).code.structural().region)
        .ok()?
        .as_str()?
        .to_owned();
    templates()
        .into_iter()
        .find(|t| t.regions.contains(&region))
}

fn write_settings(txn: &mut crate::txn::Transaction<'_>, s: &Settings) -> Result<()> {
    let id = project(txn.store())
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let v = serde_json::to_value(s).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(ANALYSIS_KEY.into(), v);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeParams {
    /// `asce7` or `en1990`.
    pub template: String,
}

/// Chooses the code template (`structural_code.set`): its natures replace the code natures; user
/// natures stay.
pub(crate) fn set_code(txn: &mut crate::txn::Transaction<'_>, p: CodeParams) -> Result<()> {
    let t = templates()
        .into_iter()
        .find(|t| t.id == p.template)
        .ok_or_else(|| {
            Error::new(
                "load.no_template",
                format!("no code template {}", p.template),
            )
        })?;
    let mut s = settings(txn.store());
    s.natures.retain(|n| n.source == "user");
    for n in t.natures {
        if !s.natures.iter().any(|x| x.name == n.name) {
            s.natures.push(Nature {
                source: "code".into(),
                ..n
            });
        }
    }
    s.template = Some(t.id);
    write_settings(txn, &s)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NatureParams {
    pub name: String,
    pub symbol: String,
    /// `permanent`, `variable` or `extraordinary`.
    pub action_type: String,
    pub action_source: String,
}

/// Adds a user nature (`load_nature.create`).
pub(crate) fn create_nature(txn: &mut crate::txn::Transaction<'_>, p: NatureParams) -> Result<()> {
    check_name(&p.name)?;
    let mut s = settings(txn.store());
    if s.natures
        .iter()
        .any(|n| n.name == p.name || n.symbol == p.symbol)
    {
        return Err(Error::new(
            "load.name_taken",
            format!("a nature is already named {}", p.name),
        ));
    }
    if !["permanent", "variable", "extraordinary"].contains(&p.action_type.as_str()) {
        return Err(Error::new(
            "load.action_type",
            "the action type is permanent, variable or extraordinary",
        ));
    }
    s.natures.push(Nature {
        name: p.name,
        symbol: p.symbol,
        action_type: p.action_type,
        action_source: p.action_source,
        psi0: None,
        psi1: None,
        psi2: None,
        source: user(),
    });
    write_settings(txn, &s)
}

fn check_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.contains(['*', '+', '-']) {
        return Err(Error::new(
            "load.bad_name",
            "names are not empty and contain no *, + or -, so formulas parse",
        ));
    }
    Ok(())
}

// ---- Cases ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaseRecipe {
    pub kind: String,
    /// Its nature's name.
    pub nature: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub self_weight: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CombinationRecipe {
    pub kind: String,
    pub formula: String,
    /// `ultimate`, `serviceability`, `quasi_permanent`, `frequent`, `rare`, `accidental` or
    /// `special`.
    pub limit_state: String,
    #[serde(default)]
    pub usages: Vec<String>,
}

fn of_kind<T: for<'de> Deserialize<'de>>(e: &Element, kind: &str) -> Option<T> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == kind).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn case_recipe(e: &Element) -> Option<CaseRecipe> {
    of_kind(e, CASE_RECIPE)
}
pub fn combination_recipe(e: &Element) -> Option<CombinationRecipe> {
    of_kind(e, COMBINATION_RECIPE)
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

fn named(store: &Store, name: &str) -> Option<ElementId> {
    store
        .iter()
        .find(|e| {
            (case_recipe(e).is_some() || combination_recipe(e).is_some())
                && e.name.as_deref() == Some(name)
        })
        .map(Element::id)
}

fn ifc_action_type(t: &str) -> &'static str {
    match t {
        "permanent" => "PERMANENT_G",
        "extraordinary" => "EXTRAORDINARY_A",
        _ => "VARIABLE_Q",
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaseParams {
    pub name: String,
    /// A nature's name or symbol.
    pub nature: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub self_weight: bool,
}

/// Creates a load case (`load_case.create`).
pub(crate) fn create_case(
    txn: &mut crate::txn::Transaction<'_>,
    p: CaseParams,
) -> Result<ElementId> {
    check_name(&p.name)?;
    let s = settings(txn.store());
    let nature = s
        .natures
        .iter()
        .find(|n| n.name == p.nature || n.symbol == p.nature)
        .ok_or_else(|| Error::new("load.no_nature", format!("no load nature {}", p.nature)))?
        .clone();
    if named(txn.store(), &p.name).is_some() {
        return Err(Error::new(
            "load.name_taken",
            format!("a case or combination is already named {}", p.name),
        ));
    }
    // The settings become the project's own on first use.
    write_settings(txn, &s)?;
    crate::analytical::ensure_model(txn)?;
    let r = CaseRecipe {
        kind: CASE_RECIPE.into(),
        nature: nature.name.clone(),
        category: p.category,
        self_weight: p.self_weight,
    };
    txn.create(
        NewElement::new("IfcStructuralLoadCase")
            .name(&p.name)
            .param("@PredefinedType", "LOAD_CASE")
            .param("@ActionType", ifc_action_type(&nature.action_type))
            .param("@ActionSource", nature.action_source.as_str())
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteCaseParams {
    pub case_id: ElementRef,
}

/// What deleting a case affected.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct CaseDeletion {
    pub loads: usize,
    pub combinations: usize,
}

/// Deletes a load case (`load_case.delete`): its loads go with it, and it is taken out of the
/// combinations that use it.
pub(crate) fn delete_case(
    txn: &mut crate::txn::Transaction<'_>,
    p: DeleteCaseParams,
) -> Result<CaseDeletion> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.case_id)
        .filter(|i| store.get(*i).and_then(case_recipe).is_some())
        .ok_or_else(|| Error::new("load.no_case", format!("no load case {:?}", p.case_id)))?;
    let e = store.get(id).expect("resolved");
    let (guid, name) = (e.guid().to_string(), e.name.clone().unwrap_or_default());
    let mut out = CaseDeletion::default();
    for l in store.iter() {
        if load_case_of(l).as_deref() == Some(guid.as_str()) {
            txn.delete(l.id())?;
            out.loads += 1;
        }
    }
    for c in store.iter() {
        let Some(mut r) = combination_recipe(c) else {
            continue;
        };
        let terms = parse(&r.formula)?;
        if terms.iter().any(|(_, n)| *n == name) {
            r.formula = format_terms(
                &terms
                    .into_iter()
                    .filter(|(_, n)| *n != name)
                    .collect::<Vec<_>>(),
            );
            let v = serde_json::to_value(&r).expect("serialises");
            txn.modify(c.id(), |e| {
                e.params.insert(RECIPE_KEY.into(), v);
            })?;
            out.combinations += 1;
        }
    }
    txn.delete(id)?;
    Ok(out)
}

// ---- Combinations ----

/// A formula's terms: factor and case or combination name.
pub fn parse(formula: &str) -> Result<Vec<(f64, String)>> {
    let bad = |m: &str| Error::new("load.formula", format!("{m}: {formula}"));
    let mut out = Vec::new();
    let text = formula.replace(' ', "");
    if text.is_empty() {
        return Ok(out);
    }
    let mut sign = 1.0;
    let mut cur = String::new();
    let mut parts = Vec::new();
    for (i, ch) in text.char_indices() {
        if (ch == '+' || ch == '-') && i > 0 && !text[..i].ends_with(['e', 'E', '*']) {
            parts.push((sign, std::mem::take(&mut cur)));
            sign = if ch == '-' { -1.0 } else { 1.0 };
        } else if (ch == '+' || ch == '-') && i == 0 {
            sign = if ch == '-' { -1.0 } else { 1.0 };
        } else {
            cur.push(ch);
        }
    }
    parts.push((sign, cur));
    for (sign, part) in parts {
        let (factor, name) = match part.split_once('*') {
            Some((f, n)) => (
                f.parse::<f64>()
                    .map_err(|_| bad("a factor is not a number"))?,
                n,
            ),
            None => (1.0, part.as_str()),
        };
        if name.is_empty() {
            return Err(bad("a term has no name"));
        }
        out.push((sign * factor, name.to_owned()));
    }
    Ok(out)
}

fn format_terms(terms: &[(f64, String)]) -> String {
    let mut s = String::new();
    for (i, (f, n)) in terms.iter().enumerate() {
        let negative = *f < 0.0;
        match (i, negative) {
            (0, true) => s.push('-'),
            (0, false) => {}
            (_, true) => s.push_str(" - "),
            (_, false) => s.push_str(" + "),
        }
        let f = (f.abs() * 1e6).round() / 1e6;
        s.push_str(&format!("{f}*{n}"));
    }
    s
}

/// A combination's factor for each case, nested combinations expanded; circular references and
/// unknown names refused.
pub fn factors(
    store: &Store,
    formula: &str,
    own: Option<&str>,
) -> Result<BTreeMap<ElementId, f64>> {
    fn walk(
        store: &Store,
        formula: &str,
        scale: f64,
        seen: &mut Vec<String>,
        out: &mut BTreeMap<ElementId, f64>,
    ) -> Result<()> {
        for (f, name) in parse(formula)? {
            if seen.contains(&name) {
                return Err(Error::new(
                    "load.circular",
                    format!("the combination refers to itself through {name}"),
                ));
            }
            let id = named(store, &name).ok_or_else(|| {
                Error::new(
                    "load.unknown_name",
                    format!("no case or combination {name}"),
                )
            })?;
            let e = store.get(id).expect("named");
            if case_recipe(e).is_some() {
                *out.entry(id).or_default() += scale * f;
            } else if let Some(c) = combination_recipe(e) {
                seen.push(name.clone());
                walk(store, &c.formula, scale * f, seen, out)?;
                seen.pop();
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    let mut seen: Vec<String> = own.map(|o| vec![o.to_owned()]).unwrap_or_default();
    walk(store, formula, 1.0, &mut seen, &mut out)?;
    Ok(out)
}

const LIMIT_STATES: [&str; 7] = [
    "ultimate",
    "serviceability",
    "quasi_permanent",
    "frequent",
    "rare",
    "accidental",
    "special",
];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CombinationParams {
    pub name: String,
    /// For example `1.35*G1 + 1.5*Q1`: factors and case or combination names.
    pub formula: String,
    #[serde(default = "ultimate")]
    pub limit_state: String,
    #[serde(default)]
    pub usages: Vec<String>,
}

fn ultimate() -> String {
    "ultimate".into()
}

/// Creates a load combination (`load_combination.create`).
pub(crate) fn create_combination(
    txn: &mut crate::txn::Transaction<'_>,
    p: CombinationParams,
) -> Result<ElementId> {
    check_name(&p.name)?;
    if named(txn.store(), &p.name).is_some() {
        return Err(Error::new(
            "load.name_taken",
            format!("a case or combination is already named {}", p.name),
        ));
    }
    if !LIMIT_STATES.contains(&p.limit_state.as_str()) {
        return Err(Error::new(
            "load.limit_state",
            format!("the limit state is one of {}", LIMIT_STATES.join(", ")),
        ));
    }
    factors(txn.store(), &p.formula, Some(&p.name))?;
    crate::analytical::ensure_model(txn)?;
    let r = CombinationRecipe {
        kind: COMBINATION_RECIPE.into(),
        formula: p.formula,
        limit_state: p.limit_state.clone(),
        usages: p.usages,
    };
    txn.create(
        NewElement::new("IfcStructuralLoadGroup")
            .name(&p.name)
            .param("@PredefinedType", "LOAD_COMBINATION")
            .param("@ActionType", "NOTDEFINED")
            .param("@ActionSource", "NOTDEFINED")
            .param("@Purpose", p.limit_state.as_str())
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateParams {
    /// Default the project's template.
    pub template: Option<String>,
    /// Create the proposals as combinations (default false: only propose them).
    #[serde(default)]
    pub add: bool,
}

/// A proposed combination.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Proposal {
    pub name: String,
    pub formula: String,
    pub limit_state: String,
}

/// What generation proposed (and made).
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct Generated {
    pub proposals: Vec<Proposal>,
    /// Cases whose nature the template does not know.
    pub flagged: Vec<String>,
    pub created: Vec<ElementId>,
}

/// Generates combinations from a template (`load_combination.generate`), offline: the template's
/// rules over the model's cases by nature (cases of one nature summed with the same factor;
/// alternatives one combination each); EN 1990 expression 6.10 with each variable nature leading
/// in turn (ULS) and the characteristic and quasi-permanent SLS combinations.
pub(crate) fn generate(
    txn: &mut crate::txn::Transaction<'_>,
    p: GenerateParams,
) -> Result<Generated> {
    let store = txn.store().clone();
    let s = settings(&store);
    let id = p
        .template
        .clone()
        .or(s.template.clone())
        .unwrap_or_default();
    let t = templates()
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| Error::new("load.no_template", format!("no code template {id}")))?;
    // Cases by nature symbol.
    let symbol_of = |nature: &str| {
        s.natures
            .iter()
            .find(|n| n.name == nature)
            .map(|n| n.symbol.clone())
    };
    let mut by_symbol: HashMap<String, Vec<String>> = HashMap::new();
    let mut out = Generated::default();
    let known: HashSet<String> = t.natures.iter().map(|n| n.symbol.clone()).collect();
    let mut cases: Vec<(String, CaseRecipe)> = store
        .iter()
        .filter_map(|e| Some((e.name.clone()?, case_recipe(e)?)))
        .collect();
    cases.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, c) in &cases {
        match symbol_of(&c.nature).filter(|sym| known.contains(sym)) {
            Some(sym) => by_symbol.entry(sym).or_default().push(name.clone()),
            None => out.flagged.push(name.clone()),
        }
    }
    let term = |f: f64, sym: &str| -> Vec<(f64, String)> {
        by_symbol
            .get(sym)
            .map(|v| v.iter().map(|n| (f, n.clone())).collect())
            .unwrap_or_default()
    };
    let mut push = |name: String, limit: &str, terms: Vec<(f64, String)>| {
        if terms.is_empty() {
            return;
        }
        let formula = format_terms(&terms);
        if !out
            .proposals
            .iter()
            .any(|p| p.formula == formula && p.limit_state == limit)
        {
            out.proposals.push(Proposal {
                name,
                formula,
                limit_state: limit.into(),
            });
        }
    };
    if let Some(ec) = &t.eurocode {
        let perm: Vec<String> = t
            .natures
            .iter()
            .filter(|n| n.action_type == "permanent")
            .map(|n| n.symbol.clone())
            .collect();
        let vars: Vec<&Nature> = t
            .natures
            .iter()
            .filter(|n| n.action_type == "variable" && by_symbol.contains_key(&n.symbol))
            .collect();
        let g = |f: f64| perm.iter().flat_map(|sym| term(f, sym)).collect::<Vec<_>>();
        push("ULS 6.10 G".into(), "ultimate", g(ec.gamma_g_sup));
        for lead in &vars {
            let mut uls = g(ec.gamma_g_sup);
            uls.extend(term(ec.gamma_q, &lead.symbol));
            let mut sls = g(1.0);
            sls.extend(term(1.0, &lead.symbol));
            for other in vars.iter().filter(|o| o.symbol != lead.symbol) {
                let psi0 = other.psi0.unwrap_or(1.0);
                uls.extend(term(ec.gamma_q * psi0, &other.symbol));
                sls.extend(term(psi0, &other.symbol));
            }
            push(format!("ULS 6.10 {}", lead.symbol), "ultimate", uls);
            push(format!("SLS char {}", lead.symbol), "rare", sls);
        }
        let mut qp = g(1.0);
        for v in &vars {
            let psi2 = v.psi2.unwrap_or(0.0);
            if psi2 > 0.0 {
                qp.extend(term(psi2, &v.symbol));
            }
        }
        push("SLS quasi-permanent".into(), "quasi_permanent", qp);
    }
    for rule in &t.rules {
        // Expand alternatives into one combination each.
        let mut variants: Vec<(String, Vec<(f64, String)>)> = vec![(rule.name.clone(), Vec::new())];
        for (syms, f) in &rule.terms {
            let alts: Vec<&str> = syms
                .split('|')
                .filter(|a| by_symbol.contains_key(*a))
                .collect();
            if alts.is_empty() {
                continue;
            }
            let mut next = Vec::new();
            for (name, terms) in &variants {
                for a in &alts {
                    let mut t2 = terms.clone();
                    t2.extend(term(*f, a));
                    let n = if alts.len() > 1 {
                        format!("{name} {a}")
                    } else {
                        name.clone()
                    };
                    next.push((n, t2));
                }
            }
            variants = next;
        }
        for (name, terms) in variants {
            push(
                format!("{} {name}", t.id.to_uppercase()),
                &rule.limit_state,
                terms,
            );
        }
    }
    if p.add {
        for pr in out.proposals.clone() {
            if named(txn.store(), &pr.name).is_some() {
                continue;
            }
            out.created.push(create_combination(
                txn,
                CombinationParams {
                    name: pr.name.replace(['-', '+', '*'], "."),
                    formula: pr.formula,
                    limit_state: pr.limit_state,
                    usages: Vec::new(),
                },
            )?);
        }
    }
    Ok(out)
}

// ---- Loads ----

/// The stored recipe of a point load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PointLoadRecipe {
    pub kind: String,
    /// Its host: a node or a member (GlobalId).
    pub host: String,
    /// On a member: where along it, 0 at its start to 1 at its end.
    #[serde(default)]
    pub at: f64,
    /// Its case (GlobalId).
    pub case: String,
    /// Fx, Fy, Fz (N) and Mx, My, Mz (N·m), project axes (or the host's local axes).
    pub forces: [f64; 6],
    #[serde(default)]
    pub orientation: Orientation,
    /// A reaction applied as a load (from another model), kept apart in reports.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_reaction: bool,
}

/// The stored recipe of a line load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineLoadRecipe {
    pub kind: String,
    /// Its member (GlobalId).
    pub host: String,
    pub case: String,
    /// Force per length at the start and the end (N/m, project axes).
    pub start: [f64; 3],
    pub end: [f64; 3],
    /// On the projected length rather than the true length.
    #[serde(default)]
    pub projected: bool,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_reaction: bool,
}

pub fn point_load(e: &Element) -> Option<PointLoadRecipe> {
    of_kind(e, POINT_LOAD_RECIPE)
}
pub fn line_load(e: &Element) -> Option<LineLoadRecipe> {
    of_kind(e, LINE_LOAD_RECIPE)
}

fn load_case_of(e: &Element) -> Option<String> {
    point_load(e)
        .map(|l| l.case)
        .or_else(|| line_load(e).map(|l| l.case))
        .or_else(|| area_load(e).map(|l| l.case))
}

fn load_host_of(e: &Element) -> Option<String> {
    point_load(e)
        .map(|l| l.host)
        .or_else(|| line_load(e).map(|l| l.host))
        .or_else(|| area_load(e).map(|l| l.host))
}

/// Whether an element depends on `guid`: a load on its host or its case.
pub fn uses(e: &Element, guid: &str) -> bool {
    load_host_of(e).as_deref() == Some(guid) || load_case_of(e).as_deref() == Some(guid)
}

fn resolve_case(store: &Store, r: &ElementRef) -> Result<String> {
    store
        .resolve(r)
        .and_then(|i| store.get(i))
        .filter(|e| case_recipe(e).is_some())
        .map(|e| e.guid().to_string())
        .ok_or_else(|| Error::new("load.no_case", format!("no load case {r:?}")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PointParams {
    /// An analytical node or member.
    pub host_id: ElementRef,
    /// On a member: where along it (0 to 1, default mid-span).
    pub position: Option<f64>,
    pub case_id: ElementRef,
    /// Fx, Fy, Fz (N), Mx, My, Mz (N·m); project axes (gravity is negative Fz).
    pub forces: [f64; 6],
    #[serde(default)]
    pub orientation: Orientation,
}

/// Places a point load (`load.create_point`).
pub(crate) fn create_point(
    txn: &mut crate::txn::Transaction<'_>,
    p: PointParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let host = store
        .resolve(&p.host_id)
        .and_then(|i| store.get(i))
        .filter(|e| {
            crate::analytical::node_recipe(e).is_some()
                || crate::analytical::member_recipe(e).is_some()
        })
        .ok_or_else(|| {
            Error::new(
                "load.no_host",
                "a point load's host is an analytical node or member",
            )
        })?;
    let at = p.position.unwrap_or(0.5);
    if !(0.0..=1.0).contains(&at) {
        return Err(Error::new(
            "load.position",
            "the position along the member is 0 to 1",
        ));
    }
    let r = PointLoadRecipe {
        kind: POINT_LOAD_RECIPE.into(),
        host: host.guid().to_string(),
        at,
        case: resolve_case(&store, &p.case_id)?,
        forces: p.forces,
        orientation: p.orientation,
        is_reaction: false,
    };
    txn.create(
        NewElement::new("IfcStructuralPointAction")
            .param("@GlobalOrLocal", global_or_local(p.orientation))
            .param("@DestabilizingLoad", false)
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineParams {
    /// An analytical member.
    pub host_id: ElementRef,
    pub case_id: ElementRef,
    /// N/m at the start (and at the end, default the same: uniform).
    pub start: [f64; 3],
    pub end: Option<[f64; 3]>,
    #[serde(default)]
    pub projected: bool,
    #[serde(default)]
    pub orientation: Orientation,
}

/// Places a line load along a member (`load.create_line`).
pub(crate) fn create_line(
    txn: &mut crate::txn::Transaction<'_>,
    p: LineParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let host = store
        .resolve(&p.host_id)
        .and_then(|i| store.get(i))
        .filter(|e| crate::analytical::member_recipe(e).is_some())
        .ok_or_else(|| Error::new("load.no_host", "a line load's host is an analytical member"))?;
    let end = p.end.unwrap_or(p.start);
    let r = LineLoadRecipe {
        kind: LINE_LOAD_RECIPE.into(),
        host: host.guid().to_string(),
        case: resolve_case(&store, &p.case_id)?,
        start: p.start,
        end,
        projected: p.projected,
        orientation: p.orientation,
        is_reaction: false,
    };
    txn.create(
        NewElement::new("IfcStructuralCurveAction")
            .param("@GlobalOrLocal", global_or_local(p.orientation))
            .param("@DestabilizingLoad", false)
            .param(
                "@ProjectedOrTrue",
                if p.projected {
                    "PROJECTED_LENGTH"
                } else {
                    "TRUE_LENGTH"
                },
            )
            .param(
                "@PredefinedType",
                if end == p.start { "CONST" } else { "LINEAR" },
            )
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

// ---- Area loads, editing loads ----

/// How a load's values are oriented.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    /// Project axes (gravity is negative z).
    #[default]
    Project,
    /// The host's local axes: a member's x along it, z toward up (toward x when vertical); a
    /// panel's z its normal.
    HostLocal,
}

/// The stored recipe of an area load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AreaLoadRecipe {
    pub kind: String,
    /// Its panel (GlobalId).
    pub host: String,
    pub case: String,
    /// Force per area (N/m²), uniform over the panel.
    pub value: [f64; 3],
    #[serde(default)]
    pub orientation: Orientation,
    /// On the panel's area projected on the plane normal to the force, rather than its true area.
    #[serde(default)]
    pub projected: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_reaction: bool,
}

pub fn area_load(e: &Element) -> Option<AreaLoadRecipe> {
    of_kind(e, AREA_LOAD_RECIPE)
}

/// The panel with this GlobalId, as it is now.
pub fn find_panel(store: &Store, guid: &str) -> Option<crate::analytical_panels::PanelGeometry> {
    let id = find(store, guid)?;
    crate::analytical_panels::geometry(store, store.get(id)?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AreaParams {
    /// An analytical panel.
    pub host_id: ElementRef,
    pub case_id: ElementRef,
    /// N/m² (gravity is negative z in project axes; in host local axes negative z presses on the
    /// panel's face against its normal).
    pub value: [f64; 3],
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default)]
    pub projected: bool,
}

/// Places an area load on a panel (`load.create_area`).
pub(crate) fn create_area(txn: &mut crate::txn::Transaction<'_>, p: AreaParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let host = store
        .resolve(&p.host_id)
        .and_then(|i| store.get(i))
        .filter(|e| crate::analytical_panels::panel_recipe(e).is_some())
        .ok_or_else(|| Error::new("load.no_host", "an area load's host is an analytical panel"))?;
    let r = AreaLoadRecipe {
        kind: AREA_LOAD_RECIPE.into(),
        host: host.guid().to_string(),
        case: resolve_case(&store, &p.case_id)?,
        value: p.value,
        orientation: p.orientation,
        projected: p.projected,
        is_reaction: false,
    };
    txn.create(
        NewElement::new("IfcStructuralPlanarAction")
            .param("@GlobalOrLocal", global_or_local(p.orientation))
            .param("@DestabilizingLoad", false)
            .param(
                "@ProjectedOrTrue",
                if p.projected {
                    "PROJECTED_LENGTH"
                } else {
                    "TRUE_LENGTH"
                },
            )
            .param("@PredefinedType", "CONST")
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )
}

fn global_or_local(o: Orientation) -> &'static str {
    match o {
        Orientation::Project => "GLOBAL_COORDS",
        Orientation::HostLocal => "LOCAL_COORDS",
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLoadParams {
    pub load_id: ElementRef,
    /// Move it to another case (its nature follows).
    pub case_id: Option<ElementRef>,
    /// Point loads: Fx, Fy, Fz, Mx, My, Mz; and where along a member (0 to 1).
    pub forces: Option<[f64; 6]>,
    pub position: Option<f64>,
    /// Line loads: start and end (N/m).
    pub start: Option<[f64; 3]>,
    pub end: Option<[f64; 3]>,
    /// Area loads (N/m²).
    pub value: Option<[f64; 3]>,
    pub orientation: Option<Orientation>,
    pub projected: Option<bool>,
    pub is_reaction: Option<bool>,
}

/// Changes a load (`load.set`): its case, values, position, orientation or flags.
pub(crate) fn set_load(txn: &mut crate::txn::Transaction<'_>, p: SetLoadParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.load_id)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {:?}", p.load_id)))?;
    let e = store.get(id).expect("resolved");
    let case = p.case_id.as_ref().map(|c| resolve_case(&store, c)).transpose()?;
    let (value, global, projected, linear) = if let Some(mut r) = point_load(e) {
        if let Some(c) = case {
            r.case = c;
        }
        if let Some(f) = p.forces {
            r.forces = f;
        }
        if let Some(at) = p.position {
            if !(0.0..=1.0).contains(&at) {
                return Err(Error::new("load.position", "the position along the member is 0 to 1"));
            }
            r.at = at;
        }
        if let Some(o) = p.orientation {
            r.orientation = o;
        }
        if let Some(b) = p.is_reaction {
            r.is_reaction = b;
        }
        (serde_json::to_value(&r).expect("serialises"), global_or_local(r.orientation), None, None)
    } else if let Some(mut r) = line_load(e) {
        if let Some(c) = case {
            r.case = c;
        }
        if let Some(s) = p.start {
            r.start = s;
            if p.end.is_none() {
                r.end = s;
            }
        }
        if let Some(s) = p.end {
            r.end = s;
        }
        if let Some(o) = p.orientation {
            r.orientation = o;
        }
        if let Some(b) = p.projected {
            r.projected = b;
        }
        if let Some(b) = p.is_reaction {
            r.is_reaction = b;
        }
        let linear = r.start != r.end;
        (
            serde_json::to_value(&r).expect("serialises"),
            global_or_local(r.orientation),
            Some(r.projected),
            Some(linear),
        )
    } else if let Some(mut r) = area_load(e) {
        if let Some(c) = case {
            r.case = c;
        }
        if let Some(v) = p.value {
            r.value = v;
        }
        if let Some(o) = p.orientation {
            r.orientation = o;
        }
        if let Some(b) = p.projected {
            r.projected = b;
        }
        if let Some(b) = p.is_reaction {
            r.is_reaction = b;
        }
        (
            serde_json::to_value(&r).expect("serialises"),
            global_or_local(r.orientation),
            Some(r.projected),
            Some(false),
        )
    } else {
        return Err(Error::new("load.not_a_load", format!("element {id} is not a load")));
    };
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
        e.params.insert("@GlobalOrLocal".into(), global.into());
        if let Some(pr) = projected {
            e.params.insert(
                "@ProjectedOrTrue".into(),
                if pr { "PROJECTED_LENGTH" } else { "TRUE_LENGTH" }.into(),
            );
        }
        if let Some(l) = linear {
            e.params
                .insert("@PredefinedType".into(), if l { "LINEAR" } else { "CONST" }.into());
        }
    })
}

/// The loads in a case (load id and kind), for the window's lists.
pub fn loads_in(store: &Store, case: ElementId) -> Vec<(ElementId, &'static str)> {
    let Some(guid) = store.get(case).map(|e| e.guid().to_string()) else {
        return Vec::new();
    };
    store
        .iter()
        .filter(|e| load_case_of(e).as_deref() == Some(guid.as_str()))
        .map(|e| {
            let kind = if point_load(e).is_some() {
                "point"
            } else if line_load(e).is_some() {
                "line"
            } else {
                "area"
            };
            (e.id(), kind)
        })
        .collect()
}

// ---- Editing cases and combinations ----

fn rename_in_formulas(
    txn: &mut crate::txn::Transaction<'_>,
    store: &Store,
    from: &str,
    to: &str,
) -> Result<usize> {
    let mut n = 0;
    for c in store.iter() {
        let Some(mut r) = combination_recipe(c) else {
            continue;
        };
        let Ok(terms) = parse(&r.formula) else {
            continue;
        };
        if !terms.iter().any(|(_, t)| t == from) {
            continue;
        }
        r.formula = format_terms(
            &terms
                .into_iter()
                .map(|(f, t)| (f, if t == from { to.to_owned() } else { t }))
                .collect::<Vec<_>>(),
        );
        let v = serde_json::to_value(&r).expect("serialises");
        txn.modify(c.id(), |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
        n += 1;
    }
    Ok(n)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCaseParams {
    pub case_id: ElementRef,
    /// A new name; formulas that use the old one follow.
    pub name: Option<String>,
    pub nature: Option<String>,
    pub category: Option<String>,
    pub self_weight: Option<bool>,
}

/// Changes a load case (`load_case.set`): renaming it renames it in every formula.
pub(crate) fn set_case(txn: &mut crate::txn::Transaction<'_>, p: SetCaseParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.case_id)
        .filter(|i| store.get(*i).and_then(case_recipe).is_some())
        .ok_or_else(|| Error::new("load.no_case", format!("no load case {:?}", p.case_id)))?;
    let e = store.get(id).expect("resolved");
    let mut r = case_recipe(e).expect("checked");
    let old = e.name.clone().unwrap_or_default();
    let mut source = None;
    if let Some(n) = &p.nature {
        let s = settings(&store);
        let nature = s
            .natures
            .iter()
            .find(|x| x.name == *n || x.symbol == *n)
            .ok_or_else(|| Error::new("load.no_nature", format!("no load nature {n}")))?;
        r.nature = nature.name.clone();
        source = Some((
            ifc_action_type(&nature.action_type),
            nature.action_source.clone(),
        ));
    }
    if let Some(c) = p.category {
        r.category = c;
    }
    if let Some(b) = p.self_weight {
        r.self_weight = b;
    }
    let name = match p.name.map(|n| n.trim().to_owned()).filter(|n| *n != old) {
        Some(n) => {
            check_name(&n)?;
            if named(&store, &n).is_some() {
                return Err(Error::new(
                    "load.name_taken",
                    format!("a case or combination is already named {n}"),
                ));
            }
            rename_in_formulas(txn, &store, &old, &n)?;
            Some(n)
        }
        None => None,
    };
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
        if let Some((t, s)) = source {
            e.params.insert("@ActionType".into(), t.into());
            e.params.insert("@ActionSource".into(), s.into());
        }
        if let Some(n) = name {
            e.name = Some(n);
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCombinationParams {
    pub combination_id: ElementRef,
    pub name: Option<String>,
    pub formula: Option<String>,
    pub limit_state: Option<String>,
    /// Usage tags (Gravity, Lateral, Combined …).
    pub usages: Option<Vec<String>>,
}

fn resolve_combination(store: &Store, r: &ElementRef) -> Result<(ElementId, CombinationRecipe)> {
    let id = store
        .resolve(r)
        .filter(|i| store.get(*i).and_then(combination_recipe).is_some())
        .ok_or_else(|| Error::new("load.no_combination", format!("no load combination {r:?}")))?;
    let c = store.get(id).and_then(combination_recipe).expect("checked");
    Ok((id, c))
}

/// Changes a load combination (`load_combination.set`): formulas are checked as on creation.
pub(crate) fn set_combination(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetCombinationParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve_combination(&store, &p.combination_id)?;
    let old = store.get(id).and_then(|e| e.name.clone()).unwrap_or_default();
    let name = p
        .name
        .map(|n| n.trim().to_owned())
        .filter(|n| *n != old);
    if let Some(n) = &name {
        check_name(n)?;
        if named(&store, n).is_some() {
            return Err(Error::new(
                "load.name_taken",
                format!("a case or combination is already named {n}"),
            ));
        }
    }
    if let Some(ls) = &p.limit_state {
        if !LIMIT_STATES.contains(&ls.as_str()) {
            return Err(Error::new(
                "load.limit_state",
                format!("the limit state is one of {}", LIMIT_STATES.join(", ")),
            ));
        }
        r.limit_state = ls.clone();
    }
    if let Some(f) = p.formula {
        factors(&store, &f, Some(name.as_deref().unwrap_or(&old)))?;
        r.formula = f;
    }
    if let Some(u) = p.usages {
        let mut u: Vec<String> = u
            .into_iter()
            .map(|x| x.trim().to_owned())
            .filter(|x| !x.is_empty())
            .collect();
        u.dedup();
        r.usages = u;
    }
    if let Some(n) = &name {
        rename_in_formulas(txn, &store, &old, n)?;
    }
    let v = serde_json::to_value(&r).expect("serialises");
    let purpose = r.limit_state.clone();
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
        e.params.insert("@Purpose".into(), purpose.into());
        if let Some(n) = name {
            e.name = Some(n);
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteCombinationParams {
    pub combination_id: ElementRef,
}

/// Deletes a load combination (`load_combination.delete`); one another combination's formula
/// uses is refused, naming it.
pub(crate) fn delete_combination(
    txn: &mut crate::txn::Transaction<'_>,
    p: DeleteCombinationParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, _) = resolve_combination(&store, &p.combination_id)?;
    let name = store.get(id).and_then(|e| e.name.clone()).unwrap_or_default();
    if let Some(user) = store.iter().find(|c| {
        c.id() != id
            && combination_recipe(c)
                .and_then(|r| parse(&r.formula).ok())
                .is_some_and(|t| t.iter().any(|(_, n)| *n == name))
    }) {
        return Err(Error::new(
            "load.in_use",
            format!(
                "combination {} uses {name}: change its formula first",
                user.name.clone().unwrap_or_default()
            ),
        ));
    }
    txn.delete(id)
}

// ---- Boundary conditions ----

/// A degree of freedom's condition.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Dof {
    Fixed,
    Released,
    /// A spring: N/m for translations, N·m/rad for rotations.
    Spring(f64),
}

/// A support's named state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportState {
    Fixed,
    Pinned,
    /// Vertical translation fixed, the rest released.
    Roller,
    User,
}

/// A point boundary condition at a node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Support {
    pub state: SupportState,
    /// Translations x, y, z, rotations x, y, z.
    pub dof: [Dof; 6],
}

impl SupportState {
    fn dof(self) -> [Dof; 6] {
        use Dof::{Fixed, Released};
        match self {
            SupportState::Fixed | SupportState::User => [Fixed; 6],
            SupportState::Pinned => [Fixed, Fixed, Fixed, Released, Released, Released],
            SupportState::Roller => [Released, Released, Fixed, Released, Released, Released],
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundaryParams {
    /// `point` (line and area conditions come with analytical panels).
    pub kind: String,
    /// A node, or a member with `location` start or end.
    pub host_id: ElementRef,
    pub location: Option<String>,
    pub state: SupportState,
    /// A User state's six conditions.
    pub dof: Option<[Dof; 6]>,
}

fn node_of(store: &Store, host: &ElementRef, location: Option<&str>) -> Result<ElementId> {
    let id = store
        .resolve(host)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {host:?}")))?;
    let e = store.get(id).expect("resolved");
    if crate::analytical::node_recipe(e).is_some() {
        return Ok(id);
    }
    let m = crate::analytical::member_recipe(e).ok_or_else(|| {
        Error::new(
            "boundary.no_host",
            "a point condition's host is a node or a member end",
        )
    })?;
    let g = match location {
        Some("end") => m.end,
        Some("start") | None => m.start,
        Some(other) => {
            return Err(Error::new(
                "boundary.location",
                format!("the location is start or end, not {other}"),
            ));
        }
    };
    find(store, &g).ok_or_else(|| Error::new("boundary.no_host", "the member's node is gone"))
}

fn set_support(
    txn: &mut crate::txn::Transaction<'_>,
    node: ElementId,
    s: Option<Support>,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = crate::analytical::node_recipe(store.get(node).expect("a node")).expect("a node");
    r.support = s;
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(node, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

/// Places a point boundary condition (`boundary_condition.create`, STR-BC-001). Returns the
/// node.
pub(crate) fn create_boundary(
    txn: &mut crate::txn::Transaction<'_>,
    p: BoundaryParams,
) -> Result<ElementId> {
    if p.kind != "point" {
        return Err(Error::new(
            "boundary.kind",
            "only point conditions until analytical panels are built",
        ));
    }
    let node = node_of(txn.store(), &p.host_id, p.location.as_deref())?;
    let dof = match (p.state, p.dof) {
        (SupportState::User, Some(d)) => d,
        (SupportState::User, None) => {
            return Err(Error::new(
                "boundary.dof",
                "a User condition needs its six dof",
            ));
        }
        (s, _) => s.dof(),
    };
    set_support(
        txn,
        node,
        Some(Support {
            state: p.state,
            dof,
        }),
    )?;
    Ok(node)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ColumnBasesParams {
    #[serde(default = "pinned")]
    pub state: SupportState,
}

fn pinned() -> SupportState {
    SupportState::Pinned
}

/// Places a point condition at the foot of every analytical column whose lower node no other
/// member reaches from below (`boundary_condition.at_column_bases`). Returns the nodes.
pub(crate) fn at_column_bases(
    txn: &mut crate::txn::Transaction<'_>,
    p: ColumnBasesParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let members = crate::analytical::members(&store);
    let mut out = Vec::new();
    for (_, m) in &members {
        if m.recipe.role != crate::analytical::Role::Column {
            continue;
        }
        let (foot, z) = if m.start[2] <= m.end[2] {
            (m.nodes[0], m.start[2])
        } else {
            (m.nodes[1], m.end[2])
        };
        let below = members.iter().any(|(_, o)| {
            (o.nodes[0] == foot && o.end[2] < z - 1e-6)
                || (o.nodes[1] == foot && o.start[2] < z - 1e-6)
        });
        if !below && !out.contains(&foot) {
            set_support(
                txn,
                foot,
                Some(Support {
                    state: p.state,
                    dof: p.state.dof(),
                }),
            )?;
            out.push(foot);
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveBoundaryParams {
    pub node_ids: Vec<ElementRef>,
}

/// Removes point conditions (`boundary_condition.remove`).
pub(crate) fn remove_boundary(
    txn: &mut crate::txn::Transaction<'_>,
    p: RemoveBoundaryParams,
) -> Result<()> {
    for r in &p.node_ids {
        let node = node_of(txn.store(), r, None)?;
        set_support(txn, node, None)?;
    }
    Ok(())
}

// ---- Reporting ----

/// The load model as reported (`load_case.list`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Report {
    pub template: Option<String>,
    pub natures: Vec<Nature>,
    pub cases: Vec<CaseInfo>,
    pub combinations: Vec<CombinationInfo>,
    /// Supported nodes and their state.
    pub supports: Vec<(ElementId, SupportState)>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CaseInfo {
    pub id: ElementId,
    pub name: String,
    pub nature: String,
    pub category: String,
    pub self_weight: bool,
    pub loads: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CombinationInfo {
    pub id: ElementId,
    pub name: String,
    pub formula: String,
    pub limit_state: String,
    pub usages: Vec<String>,
    /// Each case's factor (nested combinations expanded); empty when the formula is broken.
    pub factors: Vec<(ElementId, f64)>,
}

/// Reports natures, cases (with their load counts), combinations and supports (`load_case.list`).
pub fn report(store: &Store) -> Report {
    let s = settings(store);
    let mut counts: HashMap<String, usize> = HashMap::new();
    for e in store.iter() {
        if let Some(c) = load_case_of(e) {
            *counts.entry(c).or_default() += 1;
        }
    }
    Report {
        template: s.template.clone(),
        natures: s.natures,
        cases: store
            .iter()
            .filter_map(|e| {
                let c = case_recipe(e)?;
                Some(CaseInfo {
                    id: e.id(),
                    name: e.name.clone().unwrap_or_default(),
                    nature: c.nature,
                    category: c.category,
                    self_weight: c.self_weight,
                    loads: counts.get(&e.guid().to_string()).copied().unwrap_or(0),
                })
            })
            .collect(),
        combinations: store
            .iter()
            .filter_map(|e| {
                let c = combination_recipe(e)?;
                let name = e.name.clone().unwrap_or_default();
                Some(CombinationInfo {
                    id: e.id(),
                    factors: factors(store, &c.formula, Some(&name))
                        .map(|m| m.into_iter().collect())
                        .unwrap_or_default(),
                    name,
                    formula: c.formula,
                    limit_state: c.limit_state,
                    usages: c.usages,
                })
            })
            .collect(),
        supports: store
            .iter()
            .filter_map(|e| Some((e.id(), crate::analytical::node_recipe(e)?.support?.state)))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_parse_and_format() {
        let t = parse("1.35*G1 + 1.5*Q1 - 0.5*W").unwrap();
        assert_eq!(
            t,
            vec![(1.35, "G1".into()), (1.5, "Q1".into()), (-0.5, "W".into())]
        );
        assert_eq!(format_terms(&t), "1.35*G1 + 1.5*Q1 - 0.5*W");
        assert_eq!(parse("DL").unwrap(), vec![(1.0, "DL".into())]);
        assert!(parse("x*DL").is_err());
        assert_eq!(templates().len(), 2);
    }
}
