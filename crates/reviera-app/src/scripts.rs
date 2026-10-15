// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Lua scripts and plugins in the application (TECH_STACK §3.5):
//! - `script.run` runs code or a `.lua` file;
//! - `plugin.list` and `plugin.run` list and run the known scripts (built-in, the user's library
//!   and `REVIERA_PLUGINS` folders);
//! - `script.import` and `script.remove` manage the library.
//!
//! A run is one undo step (its commands grouped, and rolled back if the script fails). Forms the
//! script shows go to the window's form handler when there is one (an interactive run). Other
//! runs (MCP, the shell, tests) answer forms from the call's `answers`, filled with the defaults.

use std::path::PathBuf;

use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Origin, Result};
use reviera_lua::{Form, Host};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::App;

/// Shows a script's form in the window: the form, the values to show and the last problem; the
/// raw answers, or `None` when cancelled.
pub type FormHandler =
    Box<dyn FnMut(&Form, &Map<String, Value>, Option<&str>) -> Option<Map<String, Value>>>;

/// The host a script runs against: the application, as the caller, with the call's answers.
struct AppHost<'a> {
    app: &'a mut App,
    caller: Caller,
    answers: Option<Map<String, Value>>,
    interactive: bool,
    output: Vec<String>,
}

impl Host for AppHost<'_> {
    fn execute(&mut self, command: &str, args: Value) -> std::result::Result<Value, Error> {
        if matches!(command, "script.run" | "plugin.run") {
            return Err(Error::new(
                "script.nested",
                "a script cannot run another script",
            ));
        }
        self.app
            .execute(command, args, &self.caller)
            .map(|o| o.result)
    }

    fn commands(&self) -> Vec<String> {
        self.app.registry().iter().map(|s| s.name.clone()).collect()
    }

    fn form(
        &mut self,
        form: &Form,
        current: &Map<String, Value>,
        error: Option<&str>,
    ) -> Option<Map<String, Value>> {
        if self.interactive
            && let Some(h) = self.app.form_handler.as_mut()
        {
            return h.0(form, current, error);
        }
        let mut values = current.clone();
        if let Some(a) = &self.answers {
            values.extend(a.clone());
        }
        Some(values)
    }

    fn parse_length(&self, text: &str) -> std::result::Result<f64, String> {
        let settings = self
            .app
            .active
            .and_then(|d| self.app.document(d))
            .map(|d| reviera_core::units::settings_of(d.doc.store()))
            .unwrap_or_default();
        reviera_core::units::parse(text, "length", &settings).map_err(|e| e.message)
    }

    fn print(&mut self, text: &str) {
        self.output.push(text.to_owned());
    }
}

/// The window's form handler, kept on the app.
pub(crate) struct Handler(pub FormHandler);

impl std::fmt::Debug for Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FormHandler")
    }
}

impl App {
    /// Lets scripts' forms show as dialogs (the window sets this).
    pub fn set_form_handler(&mut self, handler: FormHandler) {
        self.form_handler = Some(Handler(handler));
    }

    /// The known scripts: built-in, then the folders'.
    pub fn plugins(&self) -> Vec<reviera_lua::Plugin> {
        reviera_lua::discover(&reviera_lua::plugin_dirs())
    }

    /// Runs a script as one undo step (rolled back if it fails).
    fn run_script(
        &mut self,
        caller: &Caller,
        name: &str,
        source: &str,
        answers: Option<Map<String, Value>>,
        dir: Option<&std::path::Path>,
    ) -> Result<RunResult> {
        let origin = Origin {
            source: reviera_core::Source::Lua,
            actor: name.to_owned(),
            session_id: None,
        };
        let caller = Caller {
            origin: origin.clone(),
            permission: caller.permission,
        };
        let grouped = self.group_begin(&format!("Script: {name}"), origin).is_ok();
        let interactive = caller_is_ui(caller.permission) && answers.is_none();
        let mut host = AppHost {
            app: self,
            caller,
            answers,
            interactive,
            output: Vec::new(),
        };
        let result = reviera_lua::run_in(&mut host, source, name, dir);
        let output = std::mem::take(&mut host.output);
        match result {
            Ok(result) => {
                if grouped {
                    self.group_end(reviera_core::GroupEnd::Assimilate)?;
                }
                Ok(RunResult { result, output })
            }
            Err(e) => {
                if grouped {
                    let _ = self.group_end(reviera_core::GroupEnd::Rollback);
                }
                Err(Error::new("script.failed", e.message)
                    .with_details(serde_json::json!({ "output": output })))
            }
        }
    }
}

/// Interactive runs come from the window (full permission, no answers given).
fn caller_is_ui(p: reviera_core::Permission) -> bool {
    p == reviera_core::Permission::FullScripting
}

const fn meta(
    name: &'static str,
    summary: &'static str,
    description: &'static str,
    effects: &'static [Effect],
) -> CommandMeta {
    CommandMeta {
        name,
        version: 1,
        summary,
        description,
        effects,
        transaction_name: "",
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunResult {
    /// What the script returned.
    pub result: Value,
    /// What it printed.
    pub output: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    /// Lua source to run.
    pub code: Option<String>,
    /// Or a `.lua` file.
    pub file: Option<PathBuf>,
    /// A name for messages and the undo step (default the file's name, or "script").
    pub name: Option<String>,
    /// Answers to the script's forms, by field key; without them a run from the window asks the
    /// user, and other runs take the defaults.
    pub answers: Option<Map<String, Value>>,
}

pub(crate) struct ScriptRun;

impl HostCommand for ScriptRun {
    type Host = App;
    type Params = RunParams;
    type Output = RunResult;
    const META: CommandMeta = meta(
        "script.run",
        "Run a Lua script",
        "Runs Lua `code` or a `.lua` `file` against the open model (TECH_STACK §3.5): `reviera.<domain>.<verb>(args)` \
         calls any command, `reviera.ui.form{title, message, fields}` asks for input (typed fields: length, number, \
         integer, bool, choice, text, with defaults, min and max), and `print` reports. The run is one undo step, \
         rolled back if the script fails. Forms take `answers` (by field key) and their defaults when given or \
         when not run from the window. Returns what the script returns and what it printed.",
        &[Effect::Edit],
    );

    fn run(&self, app: &mut App, caller: &Caller, p: RunParams) -> Result<RunResult> {
        let (source, name) = match (p.code, &p.file) {
            (Some(code), _) => (code, p.name.unwrap_or_else(|| "script".into())),
            (None, Some(file)) => (
                std::fs::read_to_string(file).map_err(|e| {
                    Error::new("script.unreadable", format!("{}: {e}", file.display()))
                })?,
                p.name.unwrap_or_else(|| {
                    file.file_stem()
                        .map_or("script".into(), |s| s.to_string_lossy().into_owned())
                }),
            ),
            (None, None) => return Err(Error::new("script.no_source", "give `code` or `file`")),
        };
        let dir = p.file.as_deref().and_then(std::path::Path::parent);
        app.run_script(caller, &name, &source, p.answers, dir)
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PluginInfo {
    pub name: String,
    pub title: String,
    pub description: String,
    /// Where it was found; none for a built-in plugin.
    pub path: Option<PathBuf>,
    /// In the user's library (it can be removed).
    pub in_library: bool,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

pub(crate) struct PluginList;

impl HostCommand for PluginList {
    type Host = App;
    type Params = Option<NoParams>;
    type Output = Vec<PluginInfo>;
    const META: CommandMeta = meta(
        "plugin.list",
        "List scripts and plugins",
        "Lists the known scripts: Reviera's built-in plugins, the user's script library and the folders in \
         `REVIERA_PLUGINS`. A plugin is a folder whose `plugin.lua` returns `{title, description, run}`; a single \
         `.lua` file is a script, titled by its `-- title:` comment.",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, _: Option<NoParams>) -> Result<Vec<PluginInfo>> {
        let library = reviera_lua::library_dir();
        Ok(app
            .plugins()
            .into_iter()
            .map(|p| PluginInfo {
                in_library: matches!((&p.path, &library), (Some(path), Some(l)) if path.starts_with(l)),
                name: p.name,
                title: p.title,
                description: p.description,
                path: p.path,
            })
            .collect())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginRunParams {
    pub name: String,
    /// Answers to its forms, by field key.
    pub answers: Option<Map<String, Value>>,
}

pub(crate) struct PluginRun;

impl HostCommand for PluginRun {
    type Host = App;
    type Params = PluginRunParams;
    type Output = RunResult;
    const META: CommandMeta = meta(
        "plugin.run",
        "Run a script or plugin by name",
        "Runs a known script or plugin (`plugin.list`) as `script.run` does: one undo step, forms answered by \
         the user in the window, else from `answers` and the defaults.",
        &[Effect::Edit],
    );

    fn run(&self, app: &mut App, caller: &Caller, p: PluginRunParams) -> Result<RunResult> {
        let plugin = app
            .plugins()
            .into_iter()
            .find(|x| x.name == p.name)
            .ok_or_else(|| {
                Error::new("plugin.not_found", format!("no script named `{}`", p.name))
            })?;
        // A plugin's folder (or a script's): files beside it are at `reviera.script_dir`.
        let dir = plugin.path.as_deref().map(|p| {
            if p.is_dir() {
                p
            } else {
                p.parent().unwrap_or(p)
            }
        });
        app.run_script(caller, &plugin.name, &plugin.source, p.answers, dir)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    /// A `.lua` script, or a plugin folder (holding `plugin.lua`).
    pub path: PathBuf,
    /// Replace one of the same name.
    #[serde(default)]
    pub overwrite: bool,
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

pub(crate) struct ScriptImport;

impl HostCommand for ScriptImport {
    type Host = App;
    type Params = ImportParams;
    type Output = PluginInfo;
    const META: CommandMeta = meta(
        "script.import",
        "Import a script into the library",
        "Copies a `.lua` script or a plugin folder into the user's script library, so it is listed and runnable \
         by name. Refuses a name already there unless `overwrite`.",
        &[Effect::Edit],
    );

    fn run(&self, _: &mut App, _: &Caller, p: ImportParams) -> Result<PluginInfo> {
        let library = reviera_lua::library_dir()
            .ok_or_else(|| Error::new("script.no_library", "no place for the script library"))?;
        let io = |e: std::io::Error| Error::new("script.io", e.to_string());
        std::fs::create_dir_all(&library).map_err(io)?;
        let name = p
            .path
            .file_name()
            .ok_or_else(|| Error::new("script.bad_path", "the path has no file name"))?;
        let target = library.join(name);
        if target.exists() && !p.overwrite {
            return Err(Error::new(
                "script.exists",
                format!("`{}` is already in the library", name.to_string_lossy()),
            ));
        }
        if p.path.is_dir() {
            if !p.path.join("plugin.lua").is_file() {
                return Err(Error::new(
                    "script.not_plugin",
                    "a plugin folder holds a plugin.lua",
                ));
            }
            copy_dir(&p.path, &target).map_err(io)?;
        } else if p.path.extension().is_some_and(|e| e == "lua") {
            std::fs::copy(&p.path, &target).map_err(io)?;
        } else {
            return Err(Error::new("script.not_lua", "scripts are .lua files"));
        }
        let found = reviera_lua::discover(std::slice::from_ref(&library))
            .into_iter()
            .find(|x| x.path.as_ref().is_some_and(|q| q.starts_with(&target)))
            .ok_or_else(|| {
                Error::new(
                    "script.not_plugin",
                    "the plugin did not load (plugin.lua must return {title, run})",
                )
            })?;
        Ok(PluginInfo {
            name: found.name,
            title: found.title,
            description: found.description,
            path: found.path,
            in_library: true,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveParams {
    pub name: String,
}

pub(crate) struct ScriptRemove;

impl HostCommand for ScriptRemove {
    type Host = App;
    type Params = RemoveParams;
    type Output = ();
    const META: CommandMeta = meta(
        "script.remove",
        "Remove a script from the library",
        "Deletes a script or plugin from the user's script library (built-in plugins cannot be removed).",
        &[Effect::Edit],
    );

    fn run(&self, app: &mut App, _: &Caller, p: RemoveParams) -> Result<()> {
        let library = reviera_lua::library_dir()
            .ok_or_else(|| Error::new("script.no_library", "no script library"))?;
        let found = app
            .plugins()
            .into_iter()
            .find(|x| x.name == p.name)
            .ok_or_else(|| {
                Error::new("plugin.not_found", format!("no script named `{}`", p.name))
            })?;
        let Some(path) = found.path.filter(|q| q.starts_with(&library)) else {
            return Err(Error::new(
                "script.not_in_library",
                "only scripts in the library can be removed",
            ));
        };
        let io = |e: std::io::Error| Error::new("script.io", e.to_string());
        // A plugin's folder, or the script file.
        let target = if path.file_name().is_some_and(|n| n == "plugin.lua") {
            path.parent().expect("a folder").to_owned()
        } else {
            path
        };
        if target.is_dir() {
            std::fs::remove_dir_all(&target).map_err(io)
        } else {
            std::fs::remove_file(&target).map_err(io)
        }
    }
}
