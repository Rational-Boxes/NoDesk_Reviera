// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Global parameters (spec Ch 2 §2.9.6, DM-GPAR-001): named project-wide values that drive
//! dimensions and element parameters.
//!
//! - Kept beside the project parameters in the RXD document `standards/parameters`
//!   (`globals`): name, data type, group, value or formula, reporting flag, tooltip and its
//!   associations (element GlobalId with a parameter reference, or a labelled dimension).
//! - **Formulas** use the other globals by name (optionally `@Name`), numbers with units (`mm`,
//!   `cm`, `m`, `in`, `ft`, `°`), `+ - * /` and parentheses. Each value carries its length
//!   dimension (length 1, area 2, volume 3, plain 0), so `CorridorWidth * 2 + 300 mm` is a length
//!   and `CorridorWidth + 2` is refused. Cycles are refused.
//! - A **driving** global labels dimensions (a locked distance that follows it: its first
//!   reference stays, the second moves) and is associated with instance parameters of elements:
//!   any writable parameter (`param.set` references) or a door's or window's `insert.sill`.
//! - A **reporting** global reads its value from the one dimension it labels; it cannot have a
//!   formula or a typed value.
//! - Every change is one transaction: the values, then everything they drive.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::NewElement;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;

const GLOBALS_KEY: &str = "globals";
/// The parameter reference for doors' and windows' sill height.
pub const SILL: &str = "insert.sill";

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// What a global drives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Association {
    /// An instance parameter of an element (GlobalId).
    Parameter { element: String, param_ref: String },
    /// A dimension (GlobalId) and the constraint (GlobalId) the label made (none for reporting).
    Dimension { dimension: String, constraint: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Global {
    pub id: String,
    pub name: String,
    /// length, area, volume, angle, number, integer, yes_no, text, material.
    pub data_type: String,
    #[serde(default)]
    pub group: String,
    /// Metres, m², m³, radians, a number, true/false or text.
    #[serde(default)]
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    #[serde(default)]
    pub reporting: bool,
    #[serde(default)]
    pub tooltip: String,
    #[serde(default)]
    pub associations: Vec<Association>,
}

fn power_of(data_type: &str) -> Option<i32> {
    match data_type {
        "length" => Some(1),
        "area" => Some(2),
        "volume" => Some(3),
        "angle" | "number" | "integer" => Some(0),
        _ => None,
    }
}

fn numeric(data_type: &str) -> bool {
    power_of(data_type).is_some()
}

// ---- Storage ----

pub fn globals(store: &Store) -> Vec<Global> {
    store.get(ElementId::PARAMETERS).and_then(|e| e.param(GLOBALS_KEY)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

fn save(txn: &mut Transaction<'_>, list: &[Global]) -> Result<()> {
    let value = serde_json::to_value(list).expect("serialises");
    if txn.store().contains(ElementId::PARAMETERS) {
        txn.modify(ElementId::PARAMETERS, |e| {
            e.params.insert(GLOBALS_KEY.into(), value);
        })
    } else {
        let init = NewElement::new(crate::parameters::PARAMETERS_CLASS).name("Project Parameters").param(GLOBALS_KEY, value);
        txn.create_with_id(ElementId::PARAMETERS, init).map(|_| ())
    }
}

fn find(list: &[Global], r: &str) -> Result<usize> {
    list.iter().position(|g| g.id == r || g.name.eq_ignore_ascii_case(r)).ok_or_else(|| bad("global.not_found", format!("no global parameter {r:?}")))
}

// ---- Formulas ----

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64, i32),
    Name(String),
    Op(char),
}

fn unit(s: &str) -> Option<(f64, i32)> {
    Some(match s {
        "mm" => (0.001, 1),
        "cm" => (0.01, 1),
        "m" => (1.0, 1),
        "in" | "\"" => (0.0254, 1),
        "ft" | "'" => (0.3048, 1),
        "m2" | "m²" => (1.0, 2),
        "m3" | "m³" => (1.0, 3),
        "°" | "deg" => (std::f64::consts::PI / 180.0, 0),
        _ => return None,
    })
}

fn lex(f: &str) -> Result<Vec<Tok>> {
    let cs: Vec<char> = f.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let s = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            let n: f64 = cs[s..i].iter().collect::<String>().parse().map_err(|_| bad("formula.number", "a malformed number"))?;
            // A unit right after the number (spaces allowed).
            let mut j = i;
            while j < cs.len() && cs[j] == ' ' {
                j += 1;
            }
            let k = j;
            while j < cs.len() && (cs[j].is_alphabetic() || matches!(cs[j], '°' | '²' | '³' | '"' | '\'') || (j > k && cs[j].is_ascii_digit())) {
                j += 1;
            }
            let word: String = cs[k..j].iter().collect();
            match unit(&word) {
                Some((f, p)) => {
                    out.push(Tok::Num(n * f, p));
                    i = j;
                }
                None => out.push(Tok::Num(n, 0)),
            }
        } else if c.is_alphabetic() || c == '_' || c == '@' {
            let s = i;
            i += 1;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push(Tok::Name(cs[s..i].iter().collect::<String>().trim_start_matches('@').to_owned()));
        } else if "+-*/()".contains(c) {
            out.push(Tok::Op(c));
            i += 1;
        } else {
            return Err(bad("formula.character", format!("{c:?} is not allowed in a formula")));
        }
    }
    Ok(out)
}

/// The globals a formula names.
fn names(f: &str) -> Result<Vec<String>> {
    Ok(lex(f)?.into_iter().filter_map(|t| if let Tok::Name(n) = t { Some(n) } else { None }).collect())
}

struct Parser<'a> {
    toks: Vec<Tok>,
    at: usize,
    lookup: &'a dyn Fn(&str) -> Result<(f64, i32)>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn expr(&mut self) -> Result<(f64, i32)> {
        let mut a = self.term()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.at += 1;
            let b = self.term()?;
            if a.1 != b.1 {
                return Err(bad("formula.units", "the units do not agree (adding a length to a number?)"));
            }
            a.0 = if c == '+' { a.0 + b.0 } else { a.0 - b.0 };
        }
        Ok(a)
    }
    fn term(&mut self) -> Result<(f64, i32)> {
        let mut a = self.factor()?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek().cloned() {
            self.at += 1;
            let b = self.factor()?;
            if c == '*' {
                a = (a.0 * b.0, a.1 + b.1);
            } else {
                if b.0 == 0.0 {
                    return Err(bad("formula.division", "division by zero"));
                }
                a = (a.0 / b.0, a.1 - b.1);
            }
        }
        Ok(a)
    }
    fn factor(&mut self) -> Result<(f64, i32)> {
        match self.peek().cloned() {
            Some(Tok::Op('-')) => {
                self.at += 1;
                let (v, p) = self.factor()?;
                Ok((-v, p))
            }
            Some(Tok::Op('(')) => {
                self.at += 1;
                let v = self.expr()?;
                if self.peek() != Some(&Tok::Op(')')) {
                    return Err(bad("formula.syntax", "a bracket is not closed"));
                }
                self.at += 1;
                Ok(v)
            }
            Some(Tok::Num(v, p)) => {
                self.at += 1;
                Ok((v, p))
            }
            Some(Tok::Name(n)) => {
                self.at += 1;
                (self.lookup)(&n)
            }
            _ => Err(bad("formula.syntax", "the formula is incomplete")),
        }
    }
}

fn evaluate(f: &str, lookup: &dyn Fn(&str) -> Result<(f64, i32)>) -> Result<(f64, i32)> {
    let mut p = Parser { toks: lex(f)?, at: 0, lookup };
    let v = p.expr()?;
    if p.at != p.toks.len() {
        return Err(bad("formula.syntax", "unexpected text after the formula"));
    }
    Ok(v)
}

/// The globals in formula order (each after those its formula uses); a cycle is refused.
fn order(list: &[Global]) -> Result<Vec<usize>> {
    let mut deps: Vec<Vec<usize>> = Vec::new();
    for g in list {
        let mut d = Vec::new();
        if let Some(f) = &g.formula {
            for n in names(f)? {
                let k = list.iter().position(|x| x.name.eq_ignore_ascii_case(&n)).ok_or_else(|| bad("formula.unknown", format!("{}: no global parameter {n:?}", g.name)))?;
                d.push(k);
            }
        }
        deps.push(d);
    }
    let mut out = Vec::new();
    let mut state = vec![0u8; list.len()];
    fn visit(k: usize, deps: &[Vec<usize>], state: &mut [u8], out: &mut Vec<usize>, list: &[Global]) -> Result<()> {
        match state[k] {
            2 => return Ok(()),
            1 => return Err(bad("formula.cycle", format!("{} depends on itself through its formula", list[k].name))),
            _ => {}
        }
        state[k] = 1;
        for d in &deps[k] {
            visit(*d, deps, state, out, list)?;
        }
        state[k] = 2;
        out.push(k);
        Ok(())
    }
    for k in 0..list.len() {
        visit(k, &deps, &mut state, &mut out, list)?;
    }
    Ok(out)
}

/// A reporting global's value: its dimension's measured value.
fn reported(store: &Store, g: &Global) -> Option<f64> {
    g.associations.iter().find_map(|a| match a {
        Association::Dimension { dimension, .. } => {
            let id = store.find_by_guid(&GlobalId::parse(dimension)?)?;
            let d = crate::annotations::dimension_data(store.get(id)?)?;
            crate::annotations::measure(store, &d)?.values.first().copied()
        }
        _ => None,
    })
}

/// Every global's value worked out: reporting ones measured, formulas evaluated in order.
fn resolve(store: &Store, list: &mut [Global]) -> Result<()> {
    for k in order(list)? {
        if list[k].reporting {
            if let Some(v) = reported(store, &list[k]) {
                list[k].value = json!(v);
            }
            continue;
        }
        let Some(f) = list[k].formula.clone() else { continue };
        let snapshot: Vec<Global> = list.to_vec();
        let lookup = |n: &str| -> Result<(f64, i32)> {
            let g = snapshot.iter().find(|x| x.name.eq_ignore_ascii_case(n)).ok_or_else(|| bad("formula.unknown", format!("no global parameter {n:?}")))?;
            let p = power_of(&g.data_type).ok_or_else(|| bad("formula.type", format!("{} is not a number", g.name)))?;
            Ok((g.value.as_f64().unwrap_or(0.0), p))
        };
        let (v, p) = evaluate(&f, &lookup).map_err(|e| bad("formula.invalid", format!("{}: {}", list[k].name, e.message)))?;
        let want = power_of(&list[k].data_type).ok_or_else(|| bad("formula.type", format!("{} is {}, not a number, and cannot have a formula", list[k].name, list[k].data_type)))?;
        if p != want {
            return Err(bad("formula.units", format!("{}: the formula gives {} but it is {}", list[k].name, ["a number", "a length", "an area", "a volume"].get(p.max(0) as usize).copied().unwrap_or("another unit"), list[k].data_type)));
        }
        list[k].value = if list[k].data_type == "integer" { json!(v.round()) } else { json!(v) };
    }
    Ok(())
}

/// Works the values out, keeps them, and drives every association.
fn apply(txn: &mut Transaction<'_>, mut list: Vec<Global>) -> Result<()> {
    let store = txn.store().clone();
    resolve(&store, &mut list)?;
    save(txn, &list)?;
    for g in &list {
        if g.reporting {
            continue;
        }
        for a in &g.associations {
            match a {
                Association::Parameter { element, param_ref } => {
                    let Some(id) = GlobalId::parse(element).and_then(|x| txn.store().find_by_guid(&x)) else { continue };
                    write_param(txn, id, param_ref, &g.value)?;
                }
                Association::Dimension { constraint: Some(c), .. } => {
                    let Some(cid) = GlobalId::parse(c).and_then(|x| txn.store().find_by_guid(&x)) else { continue };
                    let Some(mut data) = txn.store().get(cid).and_then(crate::constraints::data) else { continue };
                    let v = g.value.as_f64().unwrap_or(0.0);
                    // The value is the second reference's position less the first's.
                    let sign = data.value.map_or(1.0, f64::signum);
                    let want = sign * v;
                    if data.value.is_some_and(|x| (x - want).abs() < 1e-9) {
                        continue;
                    }
                    data.value = Some(want);
                    let dv = serde_json::to_value(&data).expect("serialises");
                    txn.modify(cid, |e| {
                        e.params.insert(crate::constraints::DATA_KEY.into(), dv);
                    })?;
                }
                Association::Dimension { .. } => {}
            }
        }
    }
    Ok(())
}

fn write_param(txn: &mut Transaction<'_>, id: ElementId, param_ref: &str, value: &Value) -> Result<()> {
    if param_ref == SILL {
        let v = value.as_f64().ok_or_else(|| bad("global.type", "a sill height is a length"))?;
        let now = txn.store().get(id).and_then(|e| crate::inserts::resolve(txn.store(), e).ok()).map(|p| p.recipe.sill);
        if now.is_some_and(|s| (s - v).abs() < 1e-9) {
            return Ok(());
        }
        return crate::inserts::set_sill(txn, crate::inserts::SetSillParams { element_ids: vec![ElementRef::Id(id)], sill_height: v });
    }
    crate::parameters::set(txn, &[id], param_ref, Some(value)).map(|_| ())
}

fn check_name(list: &[Global], name: &str, except: Option<&str>) -> Result<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 255 {
        return Err(bad("global.name", "a name of 1 to 255 characters"));
    }
    if let Some(c) = n.chars().find(|c| "[]{}|;<>@".contains(*c)) {
        return Err(bad("global.name", format!("a name cannot contain {c:?}")));
    }
    if list.iter().any(|g| Some(g.id.as_str()) != except && g.name.eq_ignore_ascii_case(n)) {
        return Err(bad("global.exists", format!("a global parameter {n:?} exists")));
    }
    Ok(n.to_owned())
}

fn check_value(data_type: &str, v: &Value) -> Result<Value> {
    Ok(match data_type {
        "yes_no" => json!(v.as_bool().ok_or_else(|| bad("global.value", "yes/no takes true or false"))?),
        "text" | "material" => json!(v.as_str().ok_or_else(|| bad("global.value", "this global is text"))?),
        "integer" => json!(v.as_f64().ok_or_else(|| bad("global.value", "this global is a number"))?.round()),
        t if numeric(t) => json!(v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad("global.value", "this global is a number (metres for lengths)"))?),
        t => return Err(bad("global.type", format!("{t:?} is not a data type (length, area, volume, angle, number, integer, yes_no, text, material)"))),
    })
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub name: String,
    /// length, area, volume, angle, number, integer, yes_no, text, material.
    pub data_type: String,
    /// SI units (metres for lengths, radians for angles).
    pub value: Option<Value>,
    pub formula: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub reporting: bool,
    #[serde(default)]
    pub tooltip: Option<String>,
}

/// `global.create`: returns its id.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<String> {
    let mut list = globals(txn.store());
    let name = check_name(&list, &p.name, None)?;
    if p.reporting && (p.value.is_some() || p.formula.is_some()) {
        return Err(bad("global.reporting", "a reporting global takes its value from the dimension it labels: no value or formula"));
    }
    let value = match (&p.value, &p.formula, p.reporting) {
        (_, Some(_), _) | (_, _, true) => if numeric(&p.data_type) { json!(0.0) } else { return Err(bad("global.type", "only numeric globals have formulas or report")) },
        (Some(v), None, false) => check_value(&p.data_type, v)?,
        (None, None, false) => check_value(&p.data_type, &match p.data_type.as_str() {
            "yes_no" => json!(false),
            "text" | "material" => json!(""),
            _ => json!(0.0),
        })?,
    };
    let id = GlobalId::new().to_string();
    list.push(Global {
        id: id.clone(),
        name,
        data_type: p.data_type,
        group: p.group.unwrap_or_else(|| "Other".into()),
        value,
        formula: p.formula,
        reporting: p.reporting,
        tooltip: p.tooltip.unwrap_or_default(),
        associations: Vec::new(),
    });
    apply(txn, list)?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    /// Its id or name.
    pub global_id: String,
    pub value: Option<Value>,
    /// A formula; an empty one removes it (the value then stays as it was).
    pub formula: Option<String>,
    pub name: Option<String>,
    pub group: Option<String>,
    pub tooltip: Option<String>,
}

/// `global.set`: a value or formula (and name, group, tooltip); everything it drives follows in
/// the same transaction.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let mut list = globals(txn.store());
    let k = find(&list, &p.global_id)?;
    let id = list[k].id.clone();
    if let Some(n) = &p.name {
        list[k].name = check_name(&list, n, Some(&id))?;
    }
    if list[k].reporting && (p.value.is_some() || p.formula.as_deref().is_some_and(|f| !f.trim().is_empty())) {
        return Err(bad("global.reporting", "a reporting global takes its value from its dimension"));
    }
    if let Some(f) = &p.formula {
        if f.trim().is_empty() {
            list[k].formula = None;
        } else {
            if !numeric(&list[k].data_type) {
                return Err(bad("global.type", "only numeric globals have formulas"));
            }
            list[k].formula = Some(f.trim().to_owned());
        }
    }
    if let Some(v) = &p.value {
        if list[k].formula.is_some() {
            return Err(bad("global.formula", format!("{} is set by its formula", list[k].name)));
        }
        list[k].value = check_value(&list[k].data_type, v)?;
    }
    if let Some(g) = p.group {
        list[k].group = g;
    }
    if let Some(t) = p.tooltip {
        list[k].tooltip = t;
    }
    apply(txn, list)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdParams {
    pub global_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteReport {
    /// The uses it had: parameters now plain values, dimensions unlabelled.
    pub uses: usize,
}

/// `global.delete`: its parameters keep their values, its dimensions are unlabelled; refused
/// while another global's formula uses it.
pub(crate) fn delete(txn: &mut Transaction<'_>, p: IdParams) -> Result<DeleteReport> {
    let mut list = globals(txn.store());
    let k = find(&list, &p.global_id)?;
    let g = list.remove(k);
    for other in &list {
        if let Some(f) = &other.formula
            && names(f)?.iter().any(|n| n.eq_ignore_ascii_case(&g.name))
        {
            return Err(bad("global.in_formula", format!("{}'s formula uses {}", other.name, g.name)));
        }
    }
    for a in &g.associations {
        if let Association::Dimension { constraint: Some(c), .. } = a
            && let Some(cid) = GlobalId::parse(c).and_then(|x| txn.store().find_by_guid(&x))
        {
            txn.delete(cid)?;
        }
    }
    save(txn, &list)?;
    Ok(DeleteReport { uses: g.associations.len() })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssociateParams {
    pub global_id: String,
    pub element_ids: Vec<ElementRef>,
    /// A writable instance parameter (as `param.set` takes it), or `insert.sill` for doors and
    /// windows.
    pub param_ref: String,
}

/// `global.associate`: the elements' parameter takes the global's value now and whenever it
/// changes.
pub(crate) fn associate(txn: &mut Transaction<'_>, p: AssociateParams) -> Result<()> {
    let store = txn.store().clone();
    let mut list = globals(&store);
    let k = find(&list, &p.global_id)?;
    if list[k].reporting {
        return Err(bad("global.reporting", "a reporting global drives nothing"));
    }
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
        let e = store.get(id).expect("resolved");
        if p.param_ref == SILL {
            crate::inserts::resolve(&store, e).map_err(|_| bad("global.param", format!("element {id} is not a door or window")))?;
            if list[k].data_type != "length" {
                return Err(bad("global.type", "a sill height takes a length global"));
            }
        } else {
            let def = crate::parameters::definition(&store, &p.param_ref)?;
            if def.read_only {
                return Err(bad("global.param", format!("{} is read-only", def.name)));
            }
            if def.binding == crate::parameters::Binding::Type {
                return Err(bad("global.param", format!("{} is a type parameter; globals drive instance parameters", def.name)));
            }
            if def.data_type != list[k].data_type {
                return Err(bad("global.type", format!("{} is {}, the global is {}", def.name, def.data_type, list[k].data_type)));
            }
        }
        // One global per element parameter.
        let guid = e.guid().to_string();
        for g in list.iter_mut() {
            g.associations.retain(|a| !matches!(a, Association::Parameter { element, param_ref } if *element == guid && *param_ref == p.param_ref));
        }
        list[k].associations.push(Association::Parameter { element: guid, param_ref: p.param_ref.clone() });
    }
    apply(txn, list)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DissociateParams {
    pub element_ids: Vec<ElementRef>,
    pub param_ref: String,
}

/// `global.dissociate`: the elements' parameter keeps its value, no longer driven.
pub(crate) fn dissociate(txn: &mut Transaction<'_>, p: DissociateParams) -> Result<()> {
    let store = txn.store().clone();
    let guids: BTreeSet<String> = p.element_ids.iter().filter_map(|r| store.resolve(r)).filter_map(|i| store.get(i)).map(|e| e.guid().to_string()).collect();
    let mut list = globals(&store);
    for g in list.iter_mut() {
        g.associations.retain(|a| !matches!(a, Association::Parameter { element, param_ref } if guids.contains(element) && *param_ref == p.param_ref));
    }
    save(txn, &list)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LabelParams {
    pub global_id: String,
    /// A dimension of one segment.
    pub dimension_id: ElementRef,
}

/// `global.label_dimension`: a driving global locks the dimension at its value (the dimension's
/// first reference stays, the second moves); a reporting one reads it.
pub(crate) fn label_dimension(txn: &mut Transaction<'_>, p: LabelParams) -> Result<()> {
    let store = txn.store().clone();
    let mut list = globals(&store);
    let k = find(&list, &p.global_id)?;
    let did = store.resolve(&p.dimension_id).ok_or_else(|| bad("element.not_found", "no such dimension"))?;
    let de = store.get(did).expect("resolved");
    let d = crate::annotations::dimension_data(de).ok_or_else(|| bad("global.dimension", format!("element {did} is not a dimension")))?;
    let m = crate::annotations::measure(&store, &d).ok_or_else(|| bad("dim.unmeasurable", "the dimension cannot be measured"))?;
    if m.values.len() != 1 {
        return Err(bad("global.dimension", "label a dimension of one segment"));
    }
    if list[k].data_type != "length" {
        return Err(bad("global.type", "a dimension takes a length global"));
    }
    let dguid = de.guid().to_string();
    if list.iter().any(|g| g.associations.iter().any(|a| matches!(a, Association::Dimension { dimension, .. } if *dimension == dguid))) {
        return Err(bad("global.labelled", "the dimension is labelled already"));
    }
    if list[k].reporting {
        if list[k].associations.iter().any(|a| matches!(a, Association::Dimension { .. })) {
            return Err(bad("global.reporting", "a reporting global labels one dimension"));
        }
        list[k].associations.push(Association::Dimension { dimension: dguid, constraint: None });
        return apply(txn, list);
    }
    // A locked distance that follows the global: made at the measured value, then set.
    let existing = crate::constraints::of_dimension(&store, &dguid);
    for (id, c) in existing {
        if c.kind == crate::constraints::Kind::Distance {
            txn.delete(id)?;
        }
    }
    let c = crate::constraints::ConstraintData {
        kind: crate::constraints::Kind::Distance,
        refs: vec![d.refs[0].clone(), d.refs[1].clone()],
        across: m.across,
        value: Some(m.positions[1] - m.positions[0]),
        dimension: Some(dguid.clone()),
        segment: Some(0),
        global: Some(list[k].id.clone()),
    };
    let cid = crate::constraints::create(txn, &c)?;
    let cguid = txn.store().get(cid).expect("made").guid().to_string();
    list[k].associations.push(Association::Dimension { dimension: dguid, constraint: Some(cguid) });
    apply(txn, list)
}

/// A global as listed: its value worked out now, and how many things it drives.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GlobalInfo {
    #[serde(flatten)]
    pub global: Global,
    pub uses: usize,
}

/// `global.list`.
pub(crate) fn list(store: &Store, _: crate::builtin::NoParams) -> Result<Vec<GlobalInfo>> {
    let mut l = globals(store);
    resolve(store, &mut l)?;
    Ok(l.into_iter().map(|g| GlobalInfo { uses: g.associations.len(), global: g }).collect())
}

/// `global.select_driven`: the elements and dimensions a global drives.
pub(crate) fn select_driven(store: &Store, p: IdParams) -> Result<Vec<ElementId>> {
    let l = globals(store);
    let g = &l[find(&l, &p.global_id)?];
    let mut out: BTreeMap<ElementId, ()> = BTreeMap::new();
    for a in &g.associations {
        let guid = match a {
            Association::Parameter { element, .. } => element,
            Association::Dimension { dimension, .. } => dimension,
        };
        if let Some(id) = GlobalId::parse(guid).and_then(|x| store.find_by_guid(&x)) {
            out.insert(id, ());
        }
    }
    Ok(out.into_keys().collect())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateParams {
    pub formula: String,
    /// The global it would belong to (its own name is then a cycle).
    pub global_id: Option<String>,
}

/// `formula.validate`: whether a formula works over the globals (names, units, cycles); returns
/// its value now.
pub(crate) fn validate(store: &Store, p: ValidateParams) -> Result<f64> {
    let mut l = globals(store);
    let k = match &p.global_id {
        Some(r) => find(&l, r)?,
        None => {
            l.push(Global { id: String::new(), name: "\u{0}formula".into(), data_type: "number".into(), group: String::new(), value: json!(0.0), formula: None, reporting: false, tooltip: String::new(), associations: Vec::new() });
            l.len() - 1
        }
    };
    let mut test = l.clone();
    test[k].formula = Some(p.formula.clone());
    // Checked for names and cycles, then evaluated without the unit check.
    order(&test)?;
    resolve(store, &mut l)?;
    let snapshot = l.clone();
    let lookup = |n: &str| -> Result<(f64, i32)> {
        let g = snapshot.iter().find(|x| x.name.eq_ignore_ascii_case(n)).ok_or_else(|| bad("formula.unknown", format!("no global parameter {n:?}")))?;
        Ok((g.value.as_f64().unwrap_or(0.0), power_of(&g.data_type).unwrap_or(0)))
    };
    Ok(evaluate(&p.formula, &lookup)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas() {
        let none = |_: &str| -> Result<(f64, i32)> { Err(bad("x", "no")) };
        assert_eq!(evaluate("300 mm", &none).unwrap(), (0.3, 1));
        let w = |n: &str| -> Result<(f64, i32)> { if n == "W" { Ok((1.8, 1)) } else { Err(bad("x", "no")) } };
        let (v, p) = evaluate("W * 2 + 300 mm", &w).unwrap();
        assert!((v - 3.9).abs() < 1e-12 && p == 1);
        assert_eq!(evaluate("@W * W", &w).unwrap().1, 2);
        assert!(evaluate("W + 2", &w).is_err());
        assert!(evaluate("(W + 1 m", &w).is_err());
        assert!((evaluate("-(2 ft) / 2", &none).unwrap().0 + 0.3048).abs() < 1e-12);
    }
}
