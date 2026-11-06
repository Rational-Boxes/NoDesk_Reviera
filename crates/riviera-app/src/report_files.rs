// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Report files (DOC-OUT-016): any read-only command's result written as a report file (HTML,
//! Markdown, CSV or JSON) with a header naming the model, the date, Riviera's version and the
//! command. The path may be a `fileengine://` address: `path` is a written file, so FileEngine
//! uploads it (COL-FE-012).

use std::path::Path;

use riviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The checks and reports Save Report… offers: (command, label). Any other read-only command can
/// be written with `report.write` too.
pub const REPORTS: &[(&str, &str)] = &[
    ("clearance.check", "Clearances (Check Overlaps)"),
    ("bath.check", "Bathrooms"),
    ("rules.check", "Interiors rules"),
    ("elevator.check", "Elevators"),
    ("elevator.check_lobbies", "Evacuation elevator lobbies"),
    ("escalator.check", "Escalators and moving walks"),
    ("stairlift.check", "Stairlifts"),
    ("inclined_elevator.check", "Inclined elevators"),
    ("funicular.check", "Funiculars"),
    ("apm.check", "People movers"),
    ("ropeway.check", "Ropeways"),
    ("stair.headroom_check", "Stair headroom"),
    ("column.check", "Columns"),
    ("analytical_model.check", "Analytical model"),
    ("energy_model.summary", "Energy model summary"),
    ("panel.loads_summary", "Panel loads"),
    ("growing.summary", "Growing areas"),
    ("tile.summary", "Tiles"),
    ("coordinates.report", "Coordinates"),
    ("hc_loads.report", "Heating and cooling loads"),
    ("energy.results", "Annual energy"),
    ("daylight.results", "Daylight"),
    ("solar.results", "Solar radiation"),
    ("embodied_carbon.results", "Embodied carbon"),
    ("structural_analysis.results", "Structural analysis"),
    ("path_of_travel.list", "Paths of travel"),
    ("rules.packs", "Rule packs in use"),
    ("furnishing.catalogue", "Furniture and equipment catalogue"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Html,
    Markdown,
    Csv,
    Json,
}

impl Format {
    fn from_path(path: &str) -> Format {
        let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "md" | "markdown" => Format::Markdown,
            "csv" => Format::Csv,
            "json" => Format::Json,
            _ => Format::Html,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteParams {
    /// The command whose result is the report (`rules.check`, `bath.check`, …): read-only.
    pub command: String,
    /// Its arguments (default none).
    #[serde(default)]
    pub args: Option<Value>,
    /// Where to write it: a local path, or a `fileengine://` address.
    #[schemars(extend("x-riviera-file" = "write"))]
    pub path: String,
    /// `html`, `markdown`, `csv` or `json`; from the path's extension when omitted (HTML
    /// otherwise).
    pub format: Option<Format>,
    /// The report's title (default: the command's summary).
    pub title: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Written {
    pub path: String,
    pub format: Format,
    pub command: String,
    /// The tables written, and their rows.
    pub tables: usize,
    pub rows: usize,
}

/// A table: its heading, columns and rows of cells.
#[derive(Debug, Default)]
struct Table {
    heading: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}

/// A value as a cell: text and numbers as they are (numbers to at most four decimals), the rest
/// as compact JSON.
fn cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => {
                let s = format!("{f:.4}");
                s.trim_end_matches('0').trim_end_matches('.').to_owned()
            }
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

fn is_records(v: &Value) -> bool {
    v.as_array().is_some_and(|a| !a.is_empty() && a.iter().all(Value::is_object))
}

/// Columns a finding's reader looks for first, in this order; the others follow by name.
const FIRST: &[&str] = &["severity", "kind", "element", "name", "rule", "message", "description"];

/// A list of records as a table.
fn records(heading: &str, list: &[Value]) -> Table {
    let mut columns: Vec<String> = Vec::new();
    for r in list {
        for k in r.as_object().expect("records").keys() {
            if !columns.contains(k) {
                columns.push(k.clone());
            }
        }
    }
    columns.sort_by_key(|c| (FIRST.iter().position(|f| f == c).unwrap_or(FIRST.len()), c.clone()));
    let rows = list.iter().map(|r| columns.iter().map(|c| cell(r.get(c).unwrap_or(&Value::Null))).collect()).collect();
    Table { heading: heading.to_owned(), columns, rows }
}

/// The result as tables: a list of records is one table; a record's plain values a summary
/// table, and each list in it a table of its own.
fn tables(result: &Value) -> Vec<Table> {
    match result {
        Value::Array(list) if is_records(result) => vec![records("Results", list)],
        Value::Array(list) => vec![Table { heading: "Results".into(), columns: vec!["Value".into()], rows: list.iter().map(|v| vec![cell(v)]).collect() }],
        Value::Object(o) => {
            let mut out = Vec::new();
            let mut summary = Table { heading: "Summary".into(), columns: vec!["Item".into(), "Value".into()], rows: Vec::new() };
            for (k, v) in o {
                if is_records(v) {
                    out.push(records(k, v.as_array().expect("a list")));
                } else {
                    summary.rows.push(vec![k.clone(), cell(v)]);
                }
            }
            if !summary.rows.is_empty() {
                out.insert(0, summary);
            }
            out
        }
        Value::Null => Vec::new(),
        other => vec![Table { heading: "Result".into(), columns: vec!["Value".into()], rows: vec![vec![cell(other)]] }],
    }
}

/// UTC now as `YYYY-MM-DD HH:MM:SS UTC`.
fn now_utc() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

struct Header {
    title: String,
    model: String,
    generated: String,
    command: String,
    args: Value,
}

impl Header {
    fn lines(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Model", self.model.clone()),
            ("Generated", self.generated.clone()),
            ("Riviera", env!("CARGO_PKG_VERSION").to_owned()),
            ("Command", format!("{} {}", self.command, self.args)),
        ]
    }
}

fn html(h: &Header, ts: &[Table]) -> String {
    let mut s = format!(
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>{t}</title><style>\
         body{{font-family:sans-serif;margin:2em;color:#222}}table{{border-collapse:collapse;width:100%;margin-bottom:1.5em}}\
         th,td{{border:1px solid #bbb;padding:.3em .5em;text-align:left;vertical-align:top;font-size:.9em}}th{{background:#eee}}\
         dl{{display:grid;grid-template-columns:max-content auto;gap:.2em 1em;color:#555}}dt{{font-weight:bold}}dd{{margin:0}}\
         tr.error td{{color:#a00}}tr.note td{{color:#666}}</style></head><body>\n<h1>{t}</h1>\n<dl>\n",
        t = esc(&h.title)
    );
    for (k, v) in h.lines() {
        s.push_str(&format!("<dt>{k}</dt><dd>{}</dd>\n", esc(&v)));
    }
    s.push_str("</dl>\n");
    if ts.is_empty() || ts.iter().all(|t| t.rows.is_empty()) {
        s.push_str("<p>Nothing to report.</p>\n");
    }
    for t in ts {
        s.push_str(&format!("<h2>{}</h2>\n<table><tr>", esc(&t.heading)));
        for c in &t.columns {
            s.push_str(&format!("<th>{}</th>", esc(c)));
        }
        s.push_str("</tr>\n");
        let severity = t.columns.iter().position(|c| c == "severity");
        for r in &t.rows {
            let class = severity.map(|k| r[k].as_str()).filter(|v| matches!(*v, "error" | "note"));
            match class {
                Some(c) => s.push_str(&format!("<tr class=\"{c}\">")),
                None => s.push_str("<tr>"),
            }
            for v in r {
                s.push_str(&format!("<td>{}</td>", esc(v)));
            }
            s.push_str("</tr>\n");
        }
        s.push_str("</table>\n");
    }
    s.push_str("</body></html>\n");
    s
}

fn markdown(h: &Header, ts: &[Table]) -> String {
    let md = |v: &str| v.replace('|', "\\|").replace('\n', " ");
    let mut s = format!("# {}\n\n", h.title);
    for (k, v) in h.lines() {
        s.push_str(&format!("- **{k}:** {}\n", md(&v)));
    }
    s.push('\n');
    if ts.is_empty() || ts.iter().all(|t| t.rows.is_empty()) {
        s.push_str("Nothing to report.\n");
    }
    for t in ts {
        s.push_str(&format!("## {}\n\n| {} |\n|{}\n", t.heading, t.columns.iter().map(|c| md(c)).collect::<Vec<_>>().join(" | "), "---|".repeat(t.columns.len())));
        for r in &t.rows {
            s.push_str(&format!("| {} |\n", r.iter().map(|c| md(c)).collect::<Vec<_>>().join(" | ")));
        }
        s.push('\n');
    }
    s
}

/// The report's main table, the one with the most rows, as CSV (RFC 4180 quoting).
fn csv(ts: &[Table]) -> String {
    let q = |v: &str| if v.contains([',', '"', '\n', '\r']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v.to_owned() };
    let Some(t) = ts.iter().max_by_key(|t| t.rows.len()) else { return String::new() };
    let mut s = t.columns.iter().map(|c| q(c)).collect::<Vec<_>>().join(",");
    s.push_str("\r\n");
    for r in &t.rows {
        s.push_str(&r.iter().map(|c| q(c)).collect::<Vec<_>>().join(","));
        s.push_str("\r\n");
    }
    s
}

pub(crate) struct ReportWrite;

impl HostCommand for ReportWrite {
    type Host = App;
    type Params = WriteParams;
    type Output = Written;
    const META: CommandMeta = CommandMeta {
        name: "report.write",
        version: 1,
        summary: "Write any check or report to a file",
        description: "Runs a read-only command (a check, report, list or summary: `rules.check`, `bath.check`, \
                      `elevator.check`, … see report.list) with its `args` and writes its result as a report file \
                      (DOC-OUT-016): HTML (default), Markdown, CSV (the main table) or JSON, chosen by `format` or the \
                      path's extension. A header names the title, the model, the date (UTC), Riviera's version and \
                      the command; the result follows as tables, error and note rows marked in HTML. `path` is local \
                      or a `fileengine://` address, uploaded into that FileEngine folder. Commands that change the \
                      model or write files are refused.",
        effects: &[Effect::Read, Effect::FileWrite],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: WriteParams) -> Result<Written> {
        let spec = app.registry().get(&p.command).ok_or_else(|| bad("report.no_command", format!("no command {:?}", p.command)))?;
        if p.command.starts_with("report.") || spec.effects.iter().any(|e| !matches!(e, Effect::Read | Effect::FileRead)) {
            return Err(bad("report.not_a_report", format!("{} changes the model or writes files; only read-only commands make reports", p.command)));
        }
        let summary = spec.summary.clone();
        let args = p.args.clone().unwrap_or_else(|| json!({}));
        let result = app.execute(&p.command, args.clone(), caller)?.result;
        let model = app.documents().into_iter().find(|d| d.active).map_or_else(|| "No model".to_owned(), |d| d.title);
        let header = Header { title: p.title.clone().unwrap_or(summary), model, generated: now_utc(), command: p.command.clone(), args };
        let format = p.format.unwrap_or_else(|| Format::from_path(&p.path));
        let ts = tables(&result);
        let text = match format {
            Format::Html => html(&header, &ts),
            Format::Markdown => markdown(&header, &ts),
            Format::Csv => csv(&ts),
            Format::Json => {
                let mut head = Map::new();
                head.insert("title".into(), json!(header.title));
                for (k, v) in header.lines() {
                    head.insert(k.to_ascii_lowercase(), json!(v));
                }
                head.insert("args".into(), header.args.clone());
                serde_json::to_string_pretty(&json!({ "report": head, "result": result })).expect("serialises") + "\n"
            }
        };
        let path = Path::new(&p.path);
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| bad("report.write_failed", format!("{}: {e}", dir.display())))?;
        }
        std::fs::write(path, text).map_err(|e| bad("report.write_failed", format!("{}: {e}", path.display())))?;
        Ok(Written { path: p.path, format, command: p.command, tables: ts.len(), rows: ts.iter().map(|t| t.rows.len()).sum() })
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReportInfo {
    pub command: String,
    pub label: String,
    pub summary: String,
}

pub(crate) struct ReportList;

impl HostCommand for ReportList {
    type Host = App;
    type Params = Option<ListParams>;
    type Output = Vec<ReportInfo>;
    const META: CommandMeta = CommandMeta {
        name: "report.list",
        version: 1,
        summary: "List the checks and reports that can be saved",
        description: "Lists the checks and reports Save Report… offers (DOC-OUT-016), each with its command and a \
                      label. report.write also takes any other read-only command.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, _: Option<ListParams>) -> Result<Vec<ReportInfo>> {
        Ok(REPORTS
            .iter()
            .filter_map(|(c, l)| app.registry().get(c).map(|s| ReportInfo { command: (*c).to_owned(), label: (*l).to_owned(), summary: s.summary.clone() }))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_become_tables() {
        let t = tables(&json!([{ "severity": "error", "rule": "A" }, { "severity": "note", "rule": "B", "x": 1.23456 }]));
        assert_eq!((t.len(), t[0].columns.clone(), t[0].rows[1].clone()), (1, vec!["severity".to_owned(), "rule".to_owned(), "x".to_owned()], vec!["note".to_owned(), "B".to_owned(), "1.2346".to_owned()]));
        let t = tables(&json!({ "capacity": 3600.0, "findings": [{ "rule": "headway" }] }));
        assert_eq!((t[0].heading.as_str(), t[0].rows.clone(), t[1].heading.as_str()), ("Summary", vec![vec!["capacity".to_owned(), "3600".to_owned()]], "findings"));
        assert_eq!(csv(&tables(&json!([{ "a": "x, \"y\"" }]))), "a\r\n\"x, \"\"y\"\"\"\r\n");
    }

    #[test]
    fn the_date_is_civil_utc() {
        let s = now_utc();
        assert!(s.len() == 23 && s.ends_with(" UTC") && s.starts_with("20"), "{s}");
    }
}
