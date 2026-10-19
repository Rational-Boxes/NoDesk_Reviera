// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Connection code checking (STR-STL-009): `connection_check.providers` lists the connection
//! design providers (Lua plugins with `capability = "connection_check"`, the built-in sample
//! among them); `structural_connection.check` and `structural_connection.presize` give a
//! provider each connection's input (`reviera_core::connection_check`) and record its answers;
//! `structural_connection.check_report` writes the last checks as an HTML report.
//!
//! Providers run against a read-only host: they cannot edit the model. Presize asks again with
//! the next sizes until every item passes (at most 20 rounds), then sets the sizes and records
//! the answer; each command is one undo step.

use std::collections::BTreeMap;
use std::path::PathBuf;

use reviera_core::connection_check::{self, CheckRecord, ProviderOutput};
use reviera_core::{Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Origin, Result};
use reviera_lua::{Form, Host, Plugin};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::App;

/// The host a provider runs against: nothing in the model can be changed or read through it.
struct ReadOnly {
    output: Vec<String>,
}

impl Host for ReadOnly {
    fn execute(&mut self, command: &str, _: Value) -> std::result::Result<Value, Error> {
        Err(Error::new(
            "connection_check.read_only",
            format!("a connection design provider cannot run commands ({command})"),
        ))
    }
    fn commands(&self) -> Vec<String> {
        Vec::new()
    }
    fn form(&mut self, _: &Form, _: &Map<String, Value>, _: Option<&str>) -> Option<Map<String, Value>> {
        None
    }
    fn parse_length(&self, _: &str) -> std::result::Result<f64, String> {
        Err("no units in a provider".into())
    }
    fn print(&mut self, text: &str) {
        self.output.push(text.to_owned());
    }
}

fn providers(app: &App) -> Vec<Plugin> {
    app.plugins()
        .into_iter()
        .filter(|p| p.capability.as_deref() == Some("connection_check"))
        .collect()
}

fn provider(app: &App, id: &str) -> Result<Plugin> {
    providers(app).into_iter().find(|p| p.name == id).ok_or_else(|| {
        Error::new("connection_check.no_provider", format!("no connection design provider `{id}`"))
    })
}

fn ask(p: &Plugin, input: &connection_check::CheckInput) -> Result<ProviderOutput> {
    let mut host = ReadOnly { output: Vec::new() };
    let arg = serde_json::to_value(input).expect("serialises");
    let dir = p.path.as_ref().and_then(|f| f.parent());
    let v = reviera_lua::call(&mut host, &p.source, &p.name, dir, "check", &arg)
        .map_err(|e| Error::new("connection_check.provider_failed", format!("{}: {}", p.name, e.message)))?;
    serde_json::from_value(v).map_err(|e| {
        Error::new("connection_check.bad_answer", format!("{} answered in an unexpected form: {e}", p.name))
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProviderInfo {
    pub id: String,
    pub title: String,
    pub description: String,
    pub codes: Vec<String>,
}

pub(crate) struct ProvidersList;

impl HostCommand for ProvidersList {
    type Host = App;
    type Params = Option<Value>;
    type Output = Vec<ProviderInfo>;
    const META: CommandMeta = CommandMeta {
        name: "connection_check.providers",
        version: 1,
        summary: "Connection design providers",
        description: "The connection design providers (STR-STL-009): Lua plugins with `capability = \
                      \"connection_check\"` and a `check(input)` function, the built-in sample among them, with \
                      the design codes each offers.",
        effects: &[Effect::Read],
        transaction_name: "",
    };
    fn run(&self, app: &mut App, _: &Caller, _: Option<Value>) -> Result<Vec<ProviderInfo>> {
        Ok(providers(app)
            .into_iter()
            .map(|p| ProviderInfo { id: p.name, title: p.title, description: p.description, codes: p.codes })
            .collect())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckParams {
    pub connection_ids: Vec<ElementRef>,
    /// A provider from `connection_check.providers` (default the sample).
    #[serde(default)]
    pub provider_id: Option<String>,
    /// One of the provider's codes (default its first).
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Checked {
    pub connection: ElementId,
    pub record: CheckRecord,
    /// The sizes Presize set.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub sizes: Map<String, Value>,
    /// Presize could not make it pass: the items still failing.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub still_failing: Vec<String>,
}

fn setup(app: &App, p: &CheckParams) -> Result<(Plugin, String, Vec<ElementId>)> {
    let plugin = provider(app, p.provider_id.as_deref().unwrap_or("connection_check_sample"))?;
    let code = match &p.code {
        Some(c) if plugin.codes.is_empty() || plugin.codes.contains(c) => c.clone(),
        Some(c) => {
            return Err(Error::new(
                "connection_check.code",
                format!("{} offers {}, not {c}", plugin.name, plugin.codes.join(", ")),
            ));
        }
        None => plugin.codes.first().cloned().unwrap_or_else(|| "default".into()),
    };
    let doc = app.active().ok_or_else(|| Error::new("document.none", "no document is open"))?;
    let store = app.get(doc)?.doc.store();
    let ids = p
        .connection_ids
        .iter()
        .map(|r| store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}"))))
        .collect::<Result<Vec<_>>>()?;
    // Every connection must have its forces before any is checked.
    for id in &ids {
        connection_check::input(store, *id, &code)?;
    }
    Ok((plugin, code, ids))
}

fn grouped<T>(app: &mut App, name: &str, caller: &Caller, f: impl FnOnce(&mut App) -> Result<T>) -> Result<T> {
    let origin = Origin { source: caller.origin.source, actor: caller.origin.actor.clone(), session_id: None };
    let grouped = app.group_begin(name, origin).is_ok();
    match f(app) {
        Ok(v) => {
            if grouped {
                app.group_end(reviera_core::GroupEnd::Assimilate)?;
            }
            Ok(v)
        }
        Err(e) => {
            if grouped {
                let _ = app.group_end(reviera_core::GroupEnd::Rollback);
            }
            Err(e)
        }
    }
}

fn record(app: &mut App, caller: &Caller, plugin: &Plugin, code: &str, id: ElementId, out: &ProviderOutput, sizes: &Map<String, Value>) -> Result<CheckRecord> {
    let r = app.execute(
        "structural_connection.record_check",
        json!({ "connection_id": id, "provider": plugin.name, "code": code, "output": out, "params": sizes }),
        caller,
    )?;
    serde_json::from_value(r.result).map_err(|e| Error::new("connection_check.record", e.to_string()))
}

pub(crate) struct Check;

impl HostCommand for Check {
    type Host = App;
    type Params = CheckParams;
    type Output = Vec<Checked>;
    const META: CommandMeta = CommandMeta {
        name: "structural_connection.check",
        version: 1,
        summary: "Check connections",
        description: "Checks connections with a connection design provider (STR-STL-009): each connection's input \
                      (type and parameters, bolt group, plies, welds, members, design forces) goes to the \
                      provider's `check`, and its answer is recorded (status ok when every item passes, \
                      utilisation the largest ratio). Connections without design forces are refused, naming them. \
                      One undo step. Returns each connection's record.",
        effects: &[Effect::Read, Effect::Edit, Effect::Script],
        transaction_name: "Check Connections",
    };
    fn run(&self, app: &mut App, caller: &Caller, p: CheckParams) -> Result<Vec<Checked>> {
        let (plugin, code, ids) = setup(app, &p)?;
        grouped(app, "Check Connections", caller, |app| {
            let mut out = Vec::new();
            for id in ids {
                let doc = app.active().expect("checked");
                let input = connection_check::input(app.get(doc)?.doc.store(), id, &code)?;
                let answer = ask(&plugin, &input)?;
                let rec = record(app, caller, &plugin, &code, id, &answer, &Map::new())?;
                out.push(Checked { connection: id, record: rec, sizes: Map::new(), still_failing: Vec::new() });
            }
            Ok(out)
        })
    }
}

pub(crate) struct Presize;

impl HostCommand for Presize {
    type Host = App;
    type Params = CheckParams;
    type Output = Vec<Checked>;
    const META: CommandMeta = CommandMeta {
        name: "structural_connection.presize",
        version: 1,
        summary: "Presize connections",
        description: "Sizes connections with a connection design provider (STR-STL-009): while an item fails, the \
                      parameter it names grows one step (bolt diameter, plate thickness and weld size through \
                      their tables; a bolt count by one), at most 20 times; then the sizes are set and the answer \
                      recorded. A connection that still fails (no larger size, or 20 rounds) is recorded as \
                      failed with the items still failing. One undo step.",
        effects: &[Effect::Read, Effect::Edit, Effect::Script],
        transaction_name: "Presize Connections",
    };
    fn run(&self, app: &mut App, caller: &Caller, p: CheckParams) -> Result<Vec<Checked>> {
        let (plugin, code, ids) = setup(app, &p)?;
        grouped(app, "Presize Connections", caller, |app| {
            let mut out = Vec::new();
            for id in ids {
                let doc = app.active().expect("checked");
                let store = app.get(doc)?.doc.store();
                let us = reviera_core::steel_fab::us_region(store);
                let mut overrides: BTreeMap<String, f64> = BTreeMap::new();
                let mut answer = ask(&plugin, &connection_check::input_with(store, id, &code, &overrides)?)?;
                for _ in 0..20 {
                    if !answer.items.is_empty() && answer.items.iter().all(|i| i.ok) {
                        break;
                    }
                    let input = connection_check::input_with(store, id, &code, &overrides)?;
                    let Some(next) = connection_check::presize_step(&input, &answer, us) else { break };
                    overrides.extend(next);
                    answer = ask(&plugin, &connection_check::input_with(store, id, &code, &overrides)?)?;
                }
                let sizes: Map<String, Value> = overrides.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                let still_failing = answer.items.iter().filter(|i| !i.ok).map(|i| i.name.clone()).collect();
                let rec = record(app, caller, &plugin, &code, id, &answer, &sizes)?;
                out.push(Checked { connection: id, record: rec, sizes, still_failing });
            }
            Ok(out)
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReportParams {
    pub connection_ids: Vec<ElementRef>,
    /// The HTML file to write.
    pub path: PathBuf,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub(crate) struct Report;

impl HostCommand for Report {
    type Host = App;
    type Params = ReportParams;
    type Output = PathBuf;
    const META: CommandMeta = CommandMeta {
        name: "structural_connection.check_report",
        version: 1,
        summary: "Write a connection check report",
        description: "Writes the connections' last code checks as an HTML report (STR-STL-009): each connection's \
                      status (not calculated when it changed since), provider, code and date, design forces, and \
                      every item's action, resistance and utilisation ratio. Returns the file.",
        effects: &[Effect::Read, Effect::FileWrite],
        transaction_name: "",
    };
    fn run(&self, app: &mut App, _: &Caller, p: ReportParams) -> Result<PathBuf> {
        let doc = app.active().ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store();
        let infos = connection_check::status(store, connection_check::StatusParams { connection_ids: p.connection_ids.clone() })?;
        let mut h = String::from(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Connection checks</title><style>\
             body{font-family:sans-serif;margin:2em}table{border-collapse:collapse;margin:.5em 0 1.5em}\
             td,th{border:1px solid #bbb;padding:.25em .6em;text-align:right}th:first-child,td:first-child{text-align:left}\
             .fail{color:#b00;font-weight:bold}.ok{color:#070}</style></head><body><h1>Connection checks</h1>",
        );
        for i in infos {
            let status = match i.status {
                connection_check::Status::Ok => "<span class=\"ok\">OK</span>",
                connection_check::Status::Failed => "<span class=\"fail\">Failed</span>",
                connection_check::Status::NotCalculated => "Not calculated",
            };
            h.push_str(&format!("<h2>Connection {}</h2><p>Status: {status}", i.connection.0));
            if let Some(r) = &i.reason {
                h.push_str(&format!(" ({})", esc(r)));
            }
            h.push_str("</p>");
            if let Some(c) = &i.check {
                h.push_str(&format!(
                    "<p>Provider {}, code {}, {}; utilisation {:.2}</p>",
                    esc(&c.provider),
                    esc(&c.code),
                    esc(&c.date),
                    c.utilisation
                ));
                h.push_str("<table><tr><th>Check</th><th>Group</th><th>Action (kN)</th><th>Resistance (kN)</th><th>Ratio</th><th></th></tr>");
                for it in &c.items {
                    h.push_str(&format!(
                        "<tr><td>{}</td><td>{}</td><td>{:.1}</td><td>{:.1}</td><td>{:.2}</td><td class=\"{}\">{}</td></tr>",
                        esc(&it.name),
                        esc(&it.group),
                        it.action / 1000.0,
                        it.resistance / 1000.0,
                        it.ratio,
                        if it.ok { "ok" } else { "fail" },
                        if it.ok { "OK" } else { "Fails" }
                    ));
                }
                h.push_str("</table>");
                if let Some(n) = &c.note {
                    h.push_str(&format!("<p><i>{}</i></p>", esc(n)));
                }
            }
            if !i.design_forces.is_empty() {
                h.push_str("<table><tr><th>Group</th><th>N (kN)</th><th>V (kN)</th><th>Vh (kN)</th><th>M (kN·m)</th></tr>");
                for f in &i.design_forces {
                    h.push_str(&format!(
                        "<tr><td>{}</td><td>{:.1}</td><td>{:.1}</td><td>{:.1}</td><td>{:.1}</td></tr>",
                        esc(&f.group),
                        f.n / 1000.0,
                        f.v / 1000.0,
                        f.vh / 1000.0,
                        f.m / 1000.0
                    ));
                }
                h.push_str("</table>");
            }
        }
        h.push_str("</body></html>\n");
        std::fs::write(&p.path, h).map_err(|e| Error::new("export.write_failed", format!("{}: {e}", p.path.display())))?;
        Ok(p.path)
    }
}
