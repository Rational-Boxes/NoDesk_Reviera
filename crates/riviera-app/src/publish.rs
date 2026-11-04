// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! `publish.run` (COL-FE-015): a publish set's outputs, or those given, written to their
//! FileEngine folder in one step. Each output's command runs with its destination (the parameter
//! its schema marks `write`, `write_any` or `write_folder`, COL-FE-012) set to a FileEngine
//! address, so each file goes up as a new version of the one of its name there.

use riviera_core::publish_sets::{Output, sets};
use riviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::App;
use crate::fe_paths::{Role, marked_parameters};

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    /// A publish set kept with the model.
    #[serde(default)]
    pub set_id: Option<String>,
    /// Or the outputs to publish.
    #[serde(default)]
    pub outputs: Vec<Output>,
    /// The FileEngine folder, if not the set's.
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub folder_uid: Option<String>,
}

/// What one output published.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Published {
    pub command: String,
    pub name: String,
    /// The files uploaded: uid, name, version, and whether each replaced an earlier version.
    pub uploads: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The destination parameter of `command`, and whether it is a folder.
fn destination(app: &App, command: &str) -> Result<(String, Role)> {
    let spec = app.registry().get(command).ok_or_else(|| bad("publish.invalid", format!("no command {command}")))?;
    let mut found = Vec::new();
    marked_parameters(&spec.params, &spec.params, "", &mut found, 0);
    found
        .into_iter()
        .find(|(p, r)| !p.contains('.') && !p.contains('[') && matches!(r, Role::Write | Role::WriteAny | Role::WriteFolder))
        .ok_or_else(|| bad("publish.invalid", format!("{command} does not write a file")))
}

pub(crate) struct PublishRun;

impl HostCommand for PublishRun {
    type Host = App;
    type Params = RunParams;
    type Output = Vec<Published>;
    const META: CommandMeta = CommandMeta {
        name: "publish.run",
        version: 1,
        summary: "Publish outputs to FileEngine",
        description: "Publishes a publish set kept with the model (`set_id`), or the `outputs` given, into the set's FileEngine \
                      folder or the `connection_id` and `folder_uid` given (COL-FE-015): each output's command runs with its \
                      destination set to the folder and the output's name, so each file becomes a new version of the one of \
                      that name there, with metadata naming the command and the model. An output that fails is reported and \
                      the others still run. Returns, per output, the files uploaded or the error.",
        effects: &[Effect::Network, Effect::FileWrite],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: RunParams) -> Result<Vec<Published>> {
        let set = match &p.set_id {
            Some(id) => {
                let store = app.document(app.active().ok_or_else(|| bad("document.none", "no document is open"))?).expect("active").doc.store().clone();
                Some(sets(&store).into_iter().find(|s| &s.id == id).ok_or_else(|| bad("publish.not_found", "no such publish set"))?)
            }
            None => None,
        };
        let outputs = match &set {
            Some(s) if p.outputs.is_empty() => s.outputs.clone(),
            _ => p.outputs.clone(),
        };
        if outputs.is_empty() {
            return Err(bad("publish.invalid", "name a publish set, or the outputs to publish"));
        }
        let conn = p.connection_id.clone().or_else(|| set.as_ref().map(|s| s.connection_id.clone())).ok_or_else(|| bad("publish.invalid", "give the FileEngine connection_id"))?;
        let folder = p.folder_uid.clone().or_else(|| set.as_ref().map(|s| s.folder_uid.clone())).ok_or_else(|| bad("publish.invalid", "give the FileEngine folder_uid"))?;
        app.fileengine.known(&conn)?;
        let mut out = Vec::new();
        for o in outputs {
            let mut done = Published { command: o.command.clone(), name: o.name.clone(), uploads: Vec::new(), error: None };
            let run = (|| -> Result<Value> {
                let (param, role) = destination(app, &o.command)?;
                let mut args = Value::Object(o.args.clone());
                let address = match role {
                    Role::WriteFolder => format!("{}{conn}/{folder}", crate::fileengine::SCHEME),
                    _ => format!("{}{conn}/{folder}/{}", crate::fileengine::SCHEME, o.name),
                };
                args[param] = json!(address);
                app.execute(&o.command, args, caller).map(|r| r.result)
            })();
            match run {
                Ok(r) => done.uploads = r["fileengine"].as_array().cloned().unwrap_or_default(),
                Err(e) => done.error = Some(format!("{}: {}", e.code, e.message)),
            }
            out.push(done);
        }
        Ok(out)
    }
}
