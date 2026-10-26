// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! LuaJIT scripting: macros, add-ins and the `reviera.*` API generated from the command registry
//! (TECH_STACK §3.5).
//!
//! A script runs against a [`Host`], which carries out commands and asks the user for input. In
//! the script:
//! - `reviera.<domain>.<verb>(args)` runs a registered command. For example,
//!   `reviera.wall.create{...}` is `wall.create`. `reviera.call(name, args)` runs any command by
//!   name. The arguments are a table, as the command's JSON arguments; the result comes back as a
//!   table or value. A failed command raises a Lua error whose message starts with the error code,
//!   so `pcall` can catch it.
//! - `reviera.ui.form{title, message, fields}` asks the user for input (see [`Form`]) and returns
//!   a table of the values by field key, or `nil` when the user cancels.
//! - `reviera.ui.message(text)` and `reviera.print(...)` (and `print`) report to the user.
//!
//! **Plugins** are script modules: a `plugin.lua` that returns a table with `title`,
//! `description` and a `run` function. Built-in plugins ship with Reviera ([`builtin_plugins`]);
//! others are found in plugin folders ([`discover`]), one folder per plugin.
//!
//! Scripts run in a Lua state with the safe standard libraries (no `ffi`, `debug`, `io` or
//! process control).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use mlua::{Lua, LuaSerdeExt, Value as LuaValue};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// What a script runs against: the application, or a test double.
pub trait Host {
    /// Runs a command; the error's code and message are raised in the script.
    fn execute(&mut self, command: &str, args: Value) -> Result<Value, reviera_core::Error>;
    /// The commands the script may call.
    fn commands(&self) -> Vec<String>;
    /// Shows a form and returns the raw answers by key (strings or values), or `None` when
    /// cancelled. `error` is the last answers' problem, shown with the form again; `current`
    /// holds the values to show (the defaults at first).
    fn form(
        &mut self,
        form: &Form,
        current: &Map<String, Value>,
        error: Option<&str>,
    ) -> Option<Map<String, Value>>;
    /// Reads a length typed by the user (with units) into metres.
    fn parse_length(&self, text: &str) -> Result<f64, String>;
    /// Script output.
    fn print(&mut self, text: &str);
}

/// A field's kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    /// Metres; the user may type units.
    Length,
    Number,
    Integer,
    /// A checkbox.
    Bool,
    /// One of `choices`.
    Choice,
    Text,
}

/// One input of a form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub key: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(rename = "type", default = "text")]
    pub kind: FieldKind,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub help: Option<String>,
}

fn text() -> FieldKind {
    FieldKind::Text
}

impl Field {
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.key)
    }
}

/// A form a script shows: a title, an optional message, and its fields in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Form {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    pub fields: Vec<Field>,
}

impl Form {
    /// Checks the form's definition: keys present and unique, choices given, defaults valid.
    pub fn check(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for f in &self.fields {
            if f.key.is_empty() || !seen.insert(f.key.clone()) {
                return Err(format!(
                    "field keys must be present and unique (`{}`)",
                    f.key
                ));
            }
            if f.kind == FieldKind::Choice && f.choices.is_empty() {
                return Err(format!("choice field `{}` has no choices", f.key));
            }
        }
        Ok(())
    }

    /// The defaults: each field's, else zero, false, the first choice or empty text.
    pub fn defaults(&self) -> Map<String, Value> {
        self.fields
            .iter()
            .map(|f| {
                let v = f.default.clone().unwrap_or_else(|| match f.kind {
                    FieldKind::Length | FieldKind::Number => Value::from(f.min.unwrap_or(0.0)),
                    FieldKind::Integer => Value::from(f.min.unwrap_or(0.0) as i64),
                    FieldKind::Bool => Value::Bool(false),
                    FieldKind::Choice => Value::from(f.choices[0].clone()),
                    FieldKind::Text => Value::from(""),
                });
                (f.key.clone(), v)
            })
            .collect()
    }

    /// The answers as typed values, by field: numbers in range, integers whole, choices among
    /// the choices. A field not answered takes its default. Text for lengths is read with
    /// `parse_length`.
    pub fn validate(
        &self,
        answers: &Map<String, Value>,
        parse_length: &dyn Fn(&str) -> Result<f64, String>,
    ) -> Result<Map<String, Value>, String> {
        let defaults = self.defaults();
        let mut out = Map::new();
        for f in &self.fields {
            let raw = answers.get(&f.key).unwrap_or(&defaults[&f.key]);
            let bad = |what: &str| format!("{}: {what}", f.label());
            let number = |raw: &Value| -> Result<f64, String> {
                match raw {
                    Value::Number(n) => n.as_f64().ok_or_else(|| bad("not a number")),
                    Value::String(s) if f.kind == FieldKind::Length => {
                        parse_length(s).map_err(|e| bad(&e))
                    }
                    Value::String(s) => s.trim().parse::<f64>().map_err(|_| bad("not a number")),
                    _ => Err(bad("not a number")),
                }
            };
            let in_range = |v: f64| -> Result<f64, String> {
                if !v.is_finite() {
                    return Err(bad("not a number"));
                }
                if let Some(min) = f.min
                    && v < min
                {
                    return Err(bad(&format!("must be at least {min}")));
                }
                if let Some(max) = f.max
                    && v > max
                {
                    return Err(bad(&format!("must be at most {max}")));
                }
                Ok(v)
            };
            let v = match f.kind {
                FieldKind::Length | FieldKind::Number => Value::from(in_range(number(raw)?)?),
                FieldKind::Integer => {
                    let v = in_range(number(raw)?)?;
                    if v.fract() != 0.0 {
                        return Err(bad("must be a whole number"));
                    }
                    Value::from(v as i64)
                }
                FieldKind::Bool => match raw {
                    Value::Bool(b) => Value::Bool(*b),
                    Value::String(s) => Value::Bool(matches!(s.as_str(), "true" | "1" | "yes")),
                    Value::Number(n) => Value::Bool(n.as_f64() != Some(0.0)),
                    _ => return Err(bad("must be true or false")),
                },
                FieldKind::Choice => {
                    let s = raw
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| raw.to_string());
                    if !f.choices.contains(&s) {
                        return Err(bad(&format!("must be one of {}", f.choices.join(", "))));
                    }
                    Value::from(s)
                }
                FieldKind::Text => Value::from(
                    raw.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| raw.to_string()),
                ),
            };
            out.insert(f.key.clone(), v);
        }
        Ok(out)
    }
}

/// A script's failure: a Lua error (syntax or run time, with the script's line), or a command's.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptError {
    pub message: String,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<mlua::Error> for ScriptError {
    fn from(e: mlua::Error) -> Self {
        // The innermost cause's message, without Lua's traceback noise.
        let mut message = e.to_string();
        if let Some(i) = message.find("\nstack traceback") {
            message.truncate(i);
        }
        Self { message }
    }
}

/// A new Lua state with the safe standard libraries.
fn state() -> Result<Lua, ScriptError> {
    let libs = mlua::StdLib::MATH | mlua::StdLib::STRING | mlua::StdLib::TABLE | mlua::StdLib::BIT;
    Ok(Lua::new_with(libs, mlua::LuaOptions::default())?)
}

fn to_lua(lua: &Lua, v: &Value) -> mlua::Result<LuaValue> {
    lua.to_value_with(
        v,
        mlua::serde::ser::Options::new()
            .serialize_none_to_null(false)
            .serialize_unit_to_null(false),
    )
}

fn from_lua(v: LuaValue) -> mlua::Result<Value> {
    Ok(match v {
        LuaValue::Nil => Value::Null,
        LuaValue::Boolean(b) => Value::Bool(b),
        LuaValue::Integer(i) => Value::from(i),
        LuaValue::Number(n) => serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number),
        LuaValue::String(s) => Value::from(s.to_str()?.to_string()),
        // An error caught with pcall: its message.
        LuaValue::Error(e) => Value::from(ScriptError::from((*e).clone()).message),
        LuaValue::Table(t) => {
            // A sequence 1..n is an array; anything else (an empty table too) an object.
            let n = t.raw_len();
            if n > 0 && t.clone().pairs::<LuaValue, LuaValue>().count() == n {
                let mut out = Vec::with_capacity(n);
                for v in t.sequence_values::<LuaValue>() {
                    out.push(from_lua(v?)?);
                }
                Value::Array(out)
            } else {
                let mut out = Map::new();
                for pair in t.pairs::<LuaValue, LuaValue>() {
                    let (k, v) = pair?;
                    let key = match k {
                        LuaValue::String(s) => s.to_str()?.to_string(),
                        LuaValue::Integer(i) => i.to_string(),
                        LuaValue::Number(n) => n.to_string(),
                        _ => continue,
                    };
                    out.insert(key, from_lua(v)?);
                }
                Value::Object(out)
            }
        }
        other => Value::from(other.to_string()?),
    })
}

/// Asks the host for a form's values until they are valid or the user cancels.
fn ask(host: &mut dyn Host, form: &Form) -> Result<Option<Map<String, Value>>, String> {
    form.check()?;
    let mut current = form.defaults();
    let mut error: Option<String> = None;
    loop {
        let Some(answers) = host.form(form, &current, error.as_deref()) else {
            return Ok(None);
        };
        let result = {
            let h: &dyn Host = host;
            form.validate(&answers, &|s| h.parse_length(s))
        };
        match result {
            Ok(values) => return Ok(Some(values)),
            Err(e) => {
                // A host that cannot ask again (no user) fails with the problem.
                if error.as_deref() == Some(e.as_str()) && answers == current {
                    return Err(e);
                }
                current = answers;
                error = Some(e);
            }
        }
    }
}

/// Runs `source` (named `name` in error messages) against `host`; returns what it returns.
pub fn run(host: &mut dyn Host, source: &str, name: &str) -> Result<Value, ScriptError> {
    run_in(host, source, name, None)
}

/// As [`run`], for a script read from a file in `dir`: `reviera.script_dir` is that folder, so
/// the script can use files beside it.
pub fn run_in(
    host: &mut dyn Host,
    source: &str,
    name: &str,
    dir: Option<&std::path::Path>,
) -> Result<Value, ScriptError> {
    run_entry(host, source, name, dir, None)
}

/// Loads a plugin module and calls its function `function` with `arg` (for providers: a
/// connection check provider's `check(input)`); returns what it returns.
pub fn call(
    host: &mut dyn Host,
    source: &str,
    name: &str,
    dir: Option<&std::path::Path>,
    function: &str,
    arg: &Value,
) -> Result<Value, ScriptError> {
    run_entry(host, source, name, dir, Some((function, arg)))
}

fn run_entry(
    host: &mut dyn Host,
    source: &str,
    name: &str,
    dir: Option<&std::path::Path>,
    entry: Option<(&str, &Value)>,
) -> Result<Value, ScriptError> {
    let lua = state()?;
    let host = RefCell::new(host);
    let commands = host.borrow().commands();
    let result = lua.scope(|scope| {
        let reviera = lua.create_table()?;
        let call = scope.create_function(|lua, (name, args): (String, LuaValue)| {
            let args = from_lua(args)?;
            let out = host
                .borrow_mut()
                .execute(&name, args)
                .map_err(|e| mlua::Error::RuntimeError(format!("{}: {}", e.code, e.message)))?;
            to_lua(lua, &out)
        })?;
        reviera.set("call", call.clone())?;
        // reviera.<domain>.<verb> (and reviera.<a>.<b>.<verb> for longer names), each calling its
        // command. A name that is both a command and a prefix of others is a table that can be
        // called.
        let callable = |f: mlua::Function| -> mlua::Result<mlua::Table> {
            let t = lua.create_table()?;
            let meta = lua.create_table()?;
            meta.set("__call", lua.create_function(move |_, (_, args): (LuaValue, LuaValue)| f.call::<LuaValue>(args))?)?;
            t.set_metatable(Some(meta))?;
            Ok(t)
        };
        for name in &commands {
            let parts: Vec<&str> = name.split('.').collect();
            let Some((verb, path)) = parts.split_last() else { continue };
            if path.is_empty() {
                continue;
            }
            let mut table = reviera.clone();
            for part in path {
                table = match table.get::<LuaValue>(*part)? {
                    LuaValue::Table(t) => t,
                    LuaValue::Function(f) => {
                        let t = callable(f)?;
                        table.set(*part, t.clone())?;
                        t
                    }
                    _ => {
                        let t = lua.create_table()?;
                        table.set(*part, t.clone())?;
                        t
                    }
                };
            }
            let call = call.clone();
            let name = name.clone();
            let f = lua.create_function(move |_, args: LuaValue| {
                call.call::<LuaValue>((name.clone(), args))
            })?;
            match table.get::<LuaValue>(*verb)? {
                // Already a prefix of longer names: calling the table runs this command.
                LuaValue::Table(t) => {
                    let meta = lua.create_table()?;
                    meta.set("__call", lua.create_function(move |_, (_, args): (LuaValue, LuaValue)| f.call::<LuaValue>(args))?)?;
                    t.set_metatable(Some(meta))?;
                }
                _ => table.set(*verb, f)?,
            }
        }
        let ui = match reviera.get::<Option<mlua::Table>>("ui")? {
            Some(t) => t,
            None => lua.create_table()?,
        };
        ui.set(
            "form",
            scope.create_function(|lua, spec: LuaValue| {
                let form: Form = lua
                    .from_value(spec)
                    .map_err(|e| mlua::Error::RuntimeError(format!("ui.form: {e}")))?;
                let mut h = host.borrow_mut();
                match ask(&mut **h, &form) {
                    Ok(Some(values)) => to_lua(lua, &Value::Object(values)),
                    Ok(None) => Ok(LuaValue::Nil),
                    Err(e) => Err(mlua::Error::RuntimeError(format!("form.invalid: {e}"))),
                }
            })?,
        )?;
        let print = scope.create_function(|_, args: mlua::Variadic<LuaValue>| {
            let text: Vec<String> = args
                .iter()
                .map(|v| v.to_string().unwrap_or_else(|_| format!("{v:?}")))
                .collect();
            host.borrow_mut().print(&text.join("\t"));
            Ok(())
        })?;
        ui.set("message", print.clone())?;
        reviera.set("ui", ui)?;
        reviera.set("print", print.clone())?;
        lua.globals().set("print", print)?;
        if let Some(dir) = dir {
            reviera.set("script_dir", dir.to_string_lossy().into_owned())?;
        }
        lua.globals().set("reviera", reviera)?;
        let v: LuaValue = lua.load(source).set_name(name).eval()?;
        let v = match (entry, &v) {
            // A named function of the module, with its argument.
            (Some((function, arg)), LuaValue::Table(t)) => {
                let f = t.get::<Option<mlua::Function>>(function)?.ok_or_else(|| {
                    mlua::Error::RuntimeError(format!("the plugin has no `{function}` function"))
                })?;
                f.call::<LuaValue>(to_lua(&lua, arg)?)?
            }
            (Some((function, _)), _) => {
                return Err(mlua::Error::RuntimeError(format!(
                    "the script is not a plugin module with a `{function}` function"
                )));
            }
            // A plugin module returns its table: run it.
            (None, LuaValue::Table(t)) => match t.get::<Option<mlua::Function>>("run")? {
                Some(f) => f.call::<LuaValue>(())?,
                None => v,
            },
            _ => v,
        };
        from_lua(v)
    });
    Ok(result?)
}

/// A plugin: a script module with a title and description.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plugin {
    /// Its folder's name (built-in plugins: their own).
    pub name: String,
    pub title: String,
    pub description: String,
    /// Where it was found; `None` for a built-in plugin.
    pub path: Option<PathBuf>,
    /// What else it provides (`connection_check` for a connection design provider).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    /// The design codes a provider offers.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub codes: Vec<String>,
    #[serde(skip)]
    pub source: String,
}

/// The plugins that ship with Reviera.
pub fn builtin_plugins() -> Vec<Plugin> {
    [
        ("turret", include_str!("../plugins/turret/plugin.lua")),
        ("connection_check_sample", include_str!("../plugins/connection_check_sample/plugin.lua")),
    ]
        .into_iter()
        .filter_map(|(name, source)| describe(name, source, None))
        .collect()
}

/// A plugin's title and description, read by loading its module (without running it).
fn describe(name: &str, source: &str, path: Option<PathBuf>) -> Option<Plugin> {
    let lua = state().ok()?;
    lua.globals()
        .set("reviera", lua.create_table().ok()?)
        .ok()?;
    let t: mlua::Table = lua.load(source).set_name(name).eval().ok()?;
    t.get::<mlua::Function>("run").ok()?;
    Some(Plugin {
        name: name.to_owned(),
        title: t.get::<Option<String>>("title").ok()??.clone(),
        description: t
            .get::<Option<String>>("description")
            .ok()
            .flatten()
            .unwrap_or_default(),
        path,
        capability: t.get::<Option<String>>("capability").ok().flatten(),
        codes: t.get::<Option<Vec<String>>>("codes").ok().flatten().unwrap_or_default(),
        source: source.to_owned(),
    })
}

/// A single-file script's title and description, from its `-- title:` and `-- description:`
/// header comments (else its file name). The script is not run.
fn describe_file(path: &Path) -> Option<Plugin> {
    let source = std::fs::read_to_string(path).ok()?;
    let name = path.file_stem()?.to_str()?.to_owned();
    let header = |key: &str| {
        source
            .lines()
            .take_while(|l| l.trim_start().starts_with("--") || l.trim().is_empty())
            .find_map(|l| {
                l.trim_start()
                    .trim_start_matches('-')
                    .trim()
                    .strip_prefix(key)
                    .map(|v| v.trim().to_owned())
            })
    };
    Some(Plugin {
        title: header("title:").unwrap_or_else(|| name.replace(['_', '-'], " ")),
        description: header("description:").unwrap_or_default(),
        name,
        path: Some(path.to_owned()),
        capability: None,
        codes: Vec::new(),
        source,
    })
}

/// The scripts in `dirs`: each sub-folder holding a `plugin.lua` that returns a module table
/// with a `run` function, and each single `.lua` script. Built-in plugins come first; one of the
/// same name in a folder replaces one.
pub fn discover(dirs: &[PathBuf]) -> Vec<Plugin> {
    let mut out = builtin_plugins();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for folder in entries {
            if folder.extension().is_some_and(|e| e == "lua") && folder.is_file() {
                if let Some(p) = describe_file(&folder) {
                    out.retain(|o| o.name != p.name);
                    out.push(p);
                }
                continue;
            }
            let file = folder.join("plugin.lua");
            let (Some(name), Ok(source)) = (
                folder
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_owned),
                std::fs::read_to_string(&file),
            ) else {
                continue;
            };
            if let Some(p) = describe(&name, &source, Some(file)) {
                out.retain(|o| o.name != p.name);
                out.push(p);
            }
        }
    }
    out
}

/// The user's script library: `REVIERA_LIBRARY`, else `$XDG_CONFIG_HOME/reviera/plugins` (or
/// `~/.config/reviera/plugins`; `%APPDATA%\Reviera\plugins` on Windows).
pub fn library_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("REVIERA_LIBRARY") {
        return Some(PathBuf::from(d));
    }
    if let Some(a) = std::env::var_os("APPDATA") {
        return Some(Path::new(&a).join("Reviera").join("plugins"));
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))?;
    Some(config.join("reviera").join("plugins"))
}

/// The script folders: `REVIERA_PLUGINS` (a path list), then the user's library.
pub fn plugin_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("REVIERA_PLUGINS")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    dirs.extend(library_dir());
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host that records calls and answers forms from a fixed map.
    struct Fake {
        calls: Vec<(String, Value)>,
        answers: Option<Map<String, Value>>,
        out: Vec<String>,
    }

    impl Host for Fake {
        fn execute(&mut self, command: &str, args: Value) -> Result<Value, reviera_core::Error> {
            self.calls.push((command.to_owned(), args.clone()));
            match command {
                "wall.create" => Ok(Value::from(self.calls.len() as u64)),
                "level.list" => {
                    Ok(serde_json::json!([{ "id": 7, "name": "Level 1", "elevation": 0.0 }]))
                }
                "param.project.add" | "zone.edit" | "zone.edit.undo" => Ok(Value::from(command)),
                _ => Err(reviera_core::Error::new(
                    "registry.unknown_command",
                    "no such command",
                )),
            }
        }
        fn commands(&self) -> Vec<String> {
            ["wall.create", "level.list", "param.project.add", "zone.edit", "zone.edit.undo"].map(String::from).to_vec()
        }
        fn form(
            &mut self,
            _: &Form,
            _: &Map<String, Value>,
            _: Option<&str>,
        ) -> Option<Map<String, Value>> {
            self.answers.clone()
        }
        fn parse_length(&self, text: &str) -> Result<f64, String> {
            text.trim_end_matches(" mm")
                .parse::<f64>()
                .map(|v| v / 1000.0)
                .map_err(|_| "not a length".into())
        }
        fn print(&mut self, text: &str) {
            self.out.push(text.to_owned());
        }
    }

    fn fake(answers: Option<Value>) -> Fake {
        Fake {
            calls: Vec::new(),
            answers: answers.and_then(|v| v.as_object().cloned()),
            out: Vec::new(),
        }
    }

    /// Commands of three parts are reached through nested tables; a name that is also another's
    /// prefix can still be called.
    #[test]
    fn commands_with_longer_names() {
        let mut h = fake(None);
        let v = run(
            &mut h,
            "return { reviera.param.project.add{}, reviera.zone.edit{}, reviera.zone.edit.undo{} }",
            "test",
        )
        .unwrap();
        assert_eq!(v, serde_json::json!(["param.project.add", "zone.edit", "zone.edit.undo"]));
    }

    #[test]
    fn commands_results_and_errors() {
        let mut h = fake(None);
        let v = run(
            &mut h,
            r#"
            local levels = reviera.level.list()
            local id = reviera.wall.create{ curve = { line = { start = {0, 0}, ["end"] = {4, 0} } }, base_level = levels[1].id }
            print("made", id)
            local ok, err = pcall(reviera.call, "nope.nothing", {})
            return { id = id, ok = ok, err = err }
            "#,
            "test",
        )
        .unwrap();
        assert_eq!(v["id"], 2);
        assert_eq!(v["ok"], false);
        assert!(
            v["err"]
                .as_str()
                .unwrap()
                .contains("registry.unknown_command")
        );
        assert_eq!(h.calls[1].1["base_level"], 7);
        assert_eq!(h.out, vec!["made\t2"]);
        // A run-time error names the script and line.
        let err = run(&mut h, "local x = nil\nreturn x.y", "broken").unwrap_err();
        assert!(err.message.contains("broken"), "{}", err.message);
    }

    #[test]
    fn forms_validate_and_cancel() {
        let script = r#"
            local t = reviera.ui.form{ title = "T", fields = {
                { key = "radius", type = "length", default = 2.0, min = 0.5 },
                { key = "sides", type = "integer", default = 12, min = 6, max = 48 },
                { key = "roof", type = "choice", choices = {"Cone", "Dome"} },
                { key = "flag", type = "bool", default = true },
            } }
            if t == nil then return "cancelled" end
            return t
        "#;
        let mut h = fake(Some(
            serde_json::json!({ "radius": "2500 mm", "sides": 16 }),
        ));
        let v = run(&mut h, script, "form").unwrap();
        assert_eq!(v["radius"], 2.5);
        assert_eq!(v["sides"], 16);
        assert_eq!(v["roof"], "Cone");
        assert_eq!(v["flag"], true);
        let mut h = fake(None);
        assert_eq!(run(&mut h, script, "form").unwrap(), "cancelled");
        // Out of range, asked again with the same answer: the script gets an error.
        let mut h = fake(Some(serde_json::json!({ "sides": 3 })));
        let err = run(&mut h, script, "form").unwrap_err();
        assert!(err.message.contains("at least 6"), "{}", err.message);
    }

    #[test]
    fn the_builtin_turret_plugin_loads() {
        let p = builtin_plugins();
        assert!(p.iter().any(|p| p.name == "turret" && !p.title.is_empty()));
    }
}
