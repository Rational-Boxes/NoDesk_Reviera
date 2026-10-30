// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The MCP request handler.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use reviera_app::App;

/// Whether agents may see and call a command: not those that take, test or remove credentials.
fn for_agents(command: &str) -> bool {
    !reviera_app::subscriptions::NOT_FOR_AGENTS.contains(&command) && !reviera_app::bimserver::NOT_FOR_AGENTS.contains(&command) && !reviera_app::fileengine::NOT_FOR_AGENTS.contains(&command)
}
use reviera_core::{
    Caller, CommandSpec, Effect, Element, ElementRef, GroupEnd, Origin, Permission,
};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, JsonObject,
    ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
    ResourceContents, ResourceTemplate, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::{Value, json};

use crate::queue::AppHandle;
use crate::toolsets::{TOOLSETS, toolset_of};

/// Session settings (per stdio process or, later, per client token).
/// Asks the user to approve a destructive call: the session's name, the command and a summary of
/// what it would change; the answer arrives on the returned channel.
pub type Approver = Arc<dyn Fn(Approval) -> tokio::sync::oneshot::Receiver<bool> + Send + Sync>;

/// A destructive call waiting for the user.
#[derive(Debug, Clone)]
pub struct Approval {
    pub client: String,
    pub command: String,
    pub summary: String,
}

#[derive(Clone)]
pub struct McpConfig {
    pub permission: Permission,
    /// Toolsets enabled at start; `core` is always included.
    pub toolsets: Vec<String>,
    /// Destructive commands return a preview token instead of running (default on for `model_edit`).
    pub confirm_mode: bool,
    /// Whether the agent itself may confirm a preview. With no UI attached (stdio), the user who
    /// started the server delegates confirmation to the agent.
    pub agent_may_confirm: bool,
    /// Expose `wall.create` as `wall_create`, for clients that reject `.` in tool names.
    pub underscore_names: bool,
    /// With a window attached: destructive calls in confirm mode wait for the user's answer here
    /// instead of returning a preview token (UI-AGENT-004).
    pub approver: Option<Approver>,
    /// The session's name, shown in approvals and undo attribution.
    pub client_name: Option<String>,
}

impl std::fmt::Debug for McpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpConfig")
            .field("permission", &self.permission)
            .field("toolsets", &self.toolsets)
            .field("confirm_mode", &self.confirm_mode)
            .field("agent_may_confirm", &self.agent_may_confirm)
            .field("underscore_names", &self.underscore_names)
            .field("approver", &self.approver.is_some())
            .field("client_name", &self.client_name)
            .finish()
    }
}

impl McpConfig {
    pub fn new(permission: Permission) -> Self {
        Self {
            permission,
            toolsets: vec!["core".into()],
            confirm_mode: permission == Permission::ModelEdit,
            agent_may_confirm: true,
            underscore_names: false,
            approver: None,
            client_name: None,
        }
    }
}

#[derive(Debug)]
struct Session {
    client: String,
    enabled: BTreeSet<String>,
    previews: HashMap<String, (String, Value)>,
}

#[derive(Debug, Clone)]
pub struct McpServer {
    app: AppHandle,
    config: Arc<McpConfig>,
    session_id: String,
    session: Arc<Mutex<Session>>,
}

/// A tool result: structured content an agent can read, or a structured error it can act on.
type ToolOutcome = Result<Value, Value>;

impl McpServer {
    pub fn new(app: AppHandle, config: McpConfig) -> Self {
        let mut enabled: BTreeSet<String> = config.toolsets.iter().cloned().collect();
        enabled.insert("core".into());
        let session = Session {
            client: config.client_name.clone().unwrap_or_else(|| "agent".into()),
            enabled,
            previews: HashMap::new(),
        };
        Self {
            app,
            config: Arc::new(config),
            session_id: uuid::Uuid::new_v4().to_string(),
            session: Arc::new(Mutex::new(session)),
        }
    }

    fn caller(&self) -> Caller {
        let client = self.session.lock().unwrap().client.clone();
        Caller {
            origin: Origin::mcp(client, self.session_id.clone()),
            permission: self.config.permission,
        }
    }

    fn tool_name(&self, command: &str) -> String {
        if self.config.underscore_names {
            command.replace('.', "_")
        } else {
            command.to_owned()
        }
    }

    /// Commands that handle credentials are not offered to agents at all (DM-RPC-011).
    async fn specs(&self) -> Result<Vec<CommandSpec>, McpError> {
        self.app
            .call(|app| app.registry().iter().filter(|s| for_agents(&s.name)).cloned().collect())
            .await
            .map_err(internal)
    }

    async fn spec(&self, command: &str) -> Result<Option<CommandSpec>, McpError> {
        let command = command.to_owned();
        self.app
            .call(move |app| app.registry().get(&command).filter(|s| for_agents(&s.name)).cloned())
            .await
            .map_err(internal)
    }

    fn command_tool(&self, spec: &CommandSpec) -> Tool {
        let mut description = spec.summary.clone();
        if !spec.description.is_empty() && spec.description != spec.summary {
            description = format!("{description}\n\n{}", spec.description);
        }
        let read_only = spec.effects.iter().all(|e| *e == Effect::Read);
        let annotations = ToolAnnotations::new()
            .read_only(read_only)
            .destructive(spec.effects.contains(&Effect::Destructive))
            .open_world(spec.effects.contains(&Effect::Network));
        let mut tool = Tool::new(
            self.tool_name(&spec.name),
            description,
            Arc::new(object(&spec.params)),
        )
        .with_annotations(annotations);
        // MCP output schemas describe an object; results are wrapped as {result, warnings, changed}.
        tool = tool.with_raw_output_schema(Arc::new(object(&json!({
            "type": "object",
            "properties": {
                "result": spec.result,
                "warnings": {"type": "array"},
                "changed": {"type": "object"}
            }
        }))));
        tool
    }

    /// Runs a command for the agent. In confirm mode a destructive command is previewed and a token
    /// returned instead (DM-MCP-005).
    async fn run(&self, command: String, args: Value) -> Result<ToolOutcome, McpError> {
        let Some(spec) = self.spec(&command).await? else {
            return Ok(Err(
                json!({"code": "registry.unknown_command", "message": format!("no command named `{command}`")}),
            ));
        };
        let caller = self.caller();
        // With a window: the user approves or rejects in Reviera.
        if self.config.confirm_mode
            && spec.effects.contains(&Effect::Destructive)
            && let Some(approver) = self.config.approver.clone()
        {
            let (c, a, cl) = (command.clone(), args.clone(), caller.clone());
            let preview = self.app.call(move |app| app.preview(&c, a, &cl)).await.map_err(internal)?;
            let changed = match &preview {
                Ok(o) => {
                    let ch = json!(o.changed);
                    let n = |k: &str| ch.get(k).and_then(Value::as_array).map_or(0, Vec::len);
                    format!("{} added, {} changed, {} deleted", n("added"), n("modified"), n("deleted"))
                }
                Err(e) => return Ok(Err(json!(e))),
            };
            let client = self.session.lock().unwrap().client.clone();
            let answer = approver(Approval { client, command: command.clone(), summary: changed });
            if !answer.await.unwrap_or(false) {
                return Ok(Err(json!({"code": "approval.rejected", "message": "the user rejected this call in Reviera"})));
            }
            let outcome = self.app.call(move |app| app.execute(&command, args, &caller)).await.map_err(internal)?;
            return Ok(outcome.map(|o| json!(o)).map_err(|e| json!(e)));
        }
        if self.config.confirm_mode && spec.effects.contains(&Effect::Destructive) {
            let (c, a, cl) = (command.clone(), args.clone(), caller.clone());
            let preview = self
                .app
                .call(move |app| app.preview(&c, a, &cl))
                .await
                .map_err(internal)?;
            return Ok(match preview {
                Ok(outcome) => {
                    let token = uuid::Uuid::new_v4().to_string();
                    self.session
                        .lock()
                        .unwrap()
                        .previews
                        .insert(token.clone(), (command, args));
                    Ok(json!({
                        "confirmation_required": true,
                        "preview_token": token,
                        "changed": outcome.changed,
                        "warnings": outcome.warnings,
                        "message": "This command is destructive. Call `confirm` with the preview token to run it."
                    }))
                }
                Err(e) => Err(json!(e)),
            });
        }
        let outcome = self
            .app
            .call(move |app| app.execute(&command, args, &caller))
            .await
            .map_err(internal)?;
        Ok(outcome.map(|o| json!(o)).map_err(|e| json!(e)))
    }

    async fn call(&self, name: &str, args: Value) -> Result<ToolOutcome, McpError> {
        let str_arg = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_owned);
        match name {
            "find_commands" => {
                let query = str_arg("query").unwrap_or_default().to_lowercase();
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
                let words: Vec<&str> = query.split_whitespace().collect();
                let mut hits: Vec<(usize, CommandSpec)> = self
                    .specs()
                    .await?
                    .into_iter()
                    .filter_map(|s| {
                        let text =
                            format!("{} {} {}", s.name, s.summary, s.description).to_lowercase();
                        let score = words.iter().filter(|w| text.contains(*w)).count();
                        (score > 0 || words.is_empty()).then_some((score, s))
                    })
                    .collect();
                hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
                let found: Vec<Value> = hits
                    .into_iter()
                    .take(limit)
                    .map(|(_, s)| json!({"name": s.name, "summary": s.summary, "toolset": toolset_of(&s.name), "params": s.params}))
                    .collect();
                Ok(Ok(json!({ "commands": found })))
            }
            "describe_command" => {
                let command = str_arg("name").unwrap_or_default();
                Ok(match self.spec(&command).await? {
                    Some(s) => Ok(json!({ "command": s, "toolset": toolset_of(&s.name) })),
                    None => Err(
                        json!({"code": "registry.unknown_command", "message": format!("no command named `{command}`")}),
                    ),
                })
            }
            "run_command" => {
                let command = str_arg("name").unwrap_or_default();
                self.run(
                    command,
                    args.get("arguments").cloned().unwrap_or(Value::Null),
                )
                .await
            }
            "toolsets.list" => {
                let enabled = self.session.lock().unwrap().enabled.clone();
                let mut sets: Vec<Value> = TOOLSETS
                    .iter()
                    .map(|(n, d)| json!({"name": n, "description": d, "enabled": enabled.contains(*n)}))
                    .collect();
                let addins: BTreeSet<String> = self
                    .specs()
                    .await?
                    .iter()
                    .map(|s| toolset_of(&s.name))
                    .filter(|t| t.starts_with("addin:"))
                    .collect();
                sets.extend(addins.into_iter().map(|n| json!({"name": n, "description": "Add-in commands", "enabled": enabled.contains(&n)})));
                Ok(Ok(json!({ "toolsets": sets })))
            }
            "toolsets.enable" | "toolsets.disable" => {
                let set = str_arg("name").unwrap_or_default();
                let mut s = self.session.lock().unwrap();
                if name == "toolsets.enable" {
                    s.enabled.insert(set);
                } else if set != "core" {
                    s.enabled.remove(&set);
                }
                Ok(Ok(json!({ "enabled": s.enabled })))
            }
            "undo" | "redo" => {
                let steps = args.get("steps").cloned().unwrap_or(json!(1));
                self.run(format!("edit.{name}"), json!({ "steps": steps }))
                    .await
            }
            "history" => self.run("edit.history".into(), json!({})).await,
            "query_elements" => {
                let args2 = args.clone();
                let result = self
                    .app
                    .call(move |app| -> Result<Value, Value> {
                        let doc = active_doc(app)?;
                        let class = args2.get("class").and_then(Value::as_str);
                        let category = args2.get("category").and_then(Value::as_str);
                        // A category query returns occurrences unless types are asked for (DM-ELEM-001).
                        let want_types = args2.get("types").and_then(Value::as_bool);
                        let name_contains = args2
                            .get("name_contains")
                            .and_then(Value::as_str)
                            .map(str::to_lowercase);
                        let offset =
                            args2.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let limit = args2
                            .get("limit")
                            .and_then(Value::as_u64)
                            .unwrap_or(100)
                            .min(1000) as usize;
                        let matches: Vec<&Element> = doc
                            .store()
                            .iter()
                            .filter(|e| class.is_none_or(|c| e.class.eq_ignore_ascii_case(c)))
                            .filter(|e| {
                                category.is_none_or(|k| {
                                    reviera_core::categories::category_of(e)
                                        .is_some_and(|c| c.key == k)
                                })
                            })
                            .filter(|e| {
                                let t = reviera_core::categories::is_type(e);
                                match want_types {
                                    Some(w) => t == w,
                                    None => category.is_none() || !t,
                                }
                            })
                            .filter(|e| {
                                name_contains.as_ref().is_none_or(|n| {
                                    e.name.as_deref().unwrap_or("").to_lowercase().contains(n)
                                })
                            })
                            .collect();
                        let items: Vec<Value> = matches
                            .iter()
                            .skip(offset)
                            .take(limit)
                            .map(|e| summary(doc, e))
                            .collect();
                        Ok(json!({ "total": matches.len(), "offset": offset, "items": items }))
                    })
                    .await
                    .map_err(internal)?;
                Ok(result)
            }
            "get_element" => {
                let reference: Result<ElementRef, _> =
                    serde_json::from_value(args.get("id_or_guid").cloned().unwrap_or(Value::Null));
                let Ok(reference) = reference else {
                    return Ok(Err(
                        json!({"code": "args.invalid", "message": "id_or_guid must be an element ID or a GlobalId"}),
                    ));
                };
                self.app
                    .call(move |app| -> Result<Value, Value> {
                        let doc = active_doc(app)?;
                        let e = doc
                            .store()
                            .resolve(&reference)
                            .and_then(|id| doc.store().get(id));
                        e.map(|e| summary(doc, e)).ok_or_else(
                            || json!({"code": "element.not_found", "message": "no such element"}),
                        )
                    })
                    .await
                    .map_err(internal)
            }
            "session.begin_group" => {
                if self.config.permission < Permission::ModelEdit {
                    return Ok(Err(permission_denied("model_edit")));
                }
                let group = format!(
                    "{} (agent: {})",
                    str_arg("name").unwrap_or_else(|| "Agent task".into()),
                    self.session.lock().unwrap().client
                );
                let origin = self.caller().origin;
                self.app
                    .call(move |app| {
                        app.group_begin(&group, origin)
                            .map(|_| json!({"started": true}))
                            .map_err(|e| json!(e))
                    })
                    .await
                    .map_err(internal)
            }
            "session.end_group" => {
                if self.config.permission < Permission::ModelEdit {
                    return Ok(Err(permission_denied("model_edit")));
                }
                let mode: GroupEnd = match serde_json::from_value(
                    args.get("mode").cloned().unwrap_or(json!("assimilate")),
                ) {
                    Ok(m) => m,
                    Err(_) => {
                        return Ok(Err(
                            json!({"code": "args.invalid", "message": "mode is assimilate, commit or rollback"}),
                        ));
                    }
                };
                self.app
                    .call(move |app| {
                        app.group_end(mode)
                            .map(|changed| json!({ "changed": changed }))
                            .map_err(|e| json!(e))
                    })
                    .await
                    .map_err(internal)
            }
            "preview" => {
                let command = str_arg("command").unwrap_or_default();
                let arguments = args.get("arguments").cloned().unwrap_or(Value::Null);
                let caller = self.caller();
                self.app
                    .call(move |app| {
                        app.preview(&command, arguments, &caller)
                            .map(|o| json!(o))
                            .map_err(|e| json!(e))
                    })
                    .await
                    .map_err(internal)
            }
            "confirm" => {
                if !self.config.agent_may_confirm {
                    return Ok(Err(
                        json!({"code": "confirm.user_required", "message": "the user must confirm this in the AI Agents panel"}),
                    ));
                }
                let token = str_arg("preview_token").unwrap_or_default();
                let pending = self.session.lock().unwrap().previews.remove(&token);
                let Some((command, arguments)) = pending else {
                    return Ok(Err(
                        json!({"code": "confirm.unknown_token", "message": "no pending preview with that token"}),
                    ));
                };
                let caller = self.caller();
                self.app
                    .call(move |app| {
                        app.execute(&command, arguments, &caller)
                            .map(|o| json!(o))
                            .map_err(|e| json!(e))
                    })
                    .await
                    .map_err(internal)
            }
            tool => {
                let command = self.command_for_tool(tool).await?;
                let Some(spec) = command else {
                    return Err(McpError::invalid_params(
                        format!("unknown tool `{tool}`"),
                        None,
                    ));
                };
                if !self
                    .session
                    .lock()
                    .unwrap()
                    .enabled
                    .contains(&toolset_of(&spec.name))
                {
                    return Ok(Err(json!({
                        "code": "mcp.toolset_disabled",
                        "message": format!("`{}` is in toolset `{}`; enable it or use run_command", spec.name, toolset_of(&spec.name))
                    })));
                }
                self.run(spec.name, args).await
            }
        }
    }

    async fn command_for_tool(&self, tool: &str) -> Result<Option<CommandSpec>, McpError> {
        Ok(self
            .specs()
            .await?
            .into_iter()
            .find(|s| self.tool_name(&s.name) == tool))
    }

    async fn read(&self, uri: &str) -> Result<Value, McpError> {
        let not_found = || McpError::resource_not_found(format!("no resource {uri}"), None);
        let path = uri
            .strip_prefix("reviera://")
            .ok_or_else(not_found)?
            .to_owned();
        let parts: Vec<String> = path.split('/').map(str::to_owned).collect();
        let parts_ref: Vec<&str> = parts.iter().map(String::as_str).collect();
        match parts_ref.as_slice() {
            ["documents"] => self.app.call(|app| json!(app.documents())).await.map_err(internal),
            ["commands"] => Ok(json!(
                self.specs().await?.iter().map(|s| json!({"name": s.name, "summary": s.summary, "toolset": toolset_of(&s.name)})).collect::<Vec<_>>()
            )),
            ["command", name] => self.spec(name).await?.map(|s| json!(s)).ok_or_else(not_found),
            ["element", r] | ["doc", "active", "element", r] => self.element_resource(None, r).await?.ok_or_else(not_found),
            ["doc", doc, "element", r] => {
                let doc: u64 = doc.parse().map_err(|_| not_found())?;
                self.element_resource(Some(doc), r).await?.ok_or_else(not_found)
            }
            ["doc", doc, "history"] => {
                let doc = doc.to_string();
                self.app
                    .call(move |app| {
                        let id = if doc == "active" { app.active() } else { doc.parse().ok() };
                        id.and_then(|id| app.document(id)).map(|d| {
                            json!(d.doc.undo_history().map(|i| json!({"name": i.name, "origin": i.origin})).collect::<Vec<_>>())
                        })
                    })
                    .await
                    .map_err(internal)?
                    .ok_or_else(not_found)
            }
            _ => Err(not_found()),
        }
    }

    async fn element_resource(
        &self,
        doc: Option<u64>,
        reference: &str,
    ) -> Result<Option<Value>, McpError> {
        let reference = match reference.parse::<u64>() {
            Ok(n) => ElementRef::Id(reviera_core::ElementId(n)),
            Err(_) => match reviera_core::GlobalId::parse(reference) {
                Some(g) => ElementRef::Guid(g),
                None => return Ok(None),
            },
        };
        self.app
            .call(move |app| {
                let id = doc.or(app.active())?;
                let d = &app.document(id)?.doc;
                let e = d.store().get(d.store().resolve(&reference)?)?;
                Some(summary(d, e))
            })
            .await
            .map_err(internal)
    }
}

/// What the application thread did with a view image request.
enum ImageStep {
    Done(reviera_core::Result<reviera_app::view_image::ViewImage>),
    /// The window captures it.
    Wait(std::sync::mpsc::Receiver<reviera_core::Result<reviera_app::view_image::ViewImage>>),
}

impl McpServer {
    /// `get_view_image`: the window captures the view, or it is rendered offscreen.
    async fn view_image(&self, args: Value) -> Result<CallToolResult, McpError> {
        use base64::Engine;
        let view: Option<reviera_core::ElementRef> = match args.get("view_id") {
            Some(Value::Number(n)) => n.as_u64().map(|v| reviera_core::ElementRef::Id(reviera_core::ElementId(v))),
            Some(Value::String(g)) => match reviera_core::GlobalId::parse(g) {
                Some(g) => Some(reviera_core::ElementRef::Guid(g)),
                None => return Ok(CallToolResult::structured_error(json!({ "error": { "code": "view.not_found", "message": format!("{g:?} is not a GlobalId") } }))),
            },
            _ => None,
        };
        let max = args.get("max_size").and_then(Value::as_u64).unwrap_or(1024).clamp(64, 4096) as u32;
        let gizmos = args.get("gizmos").and_then(Value::as_bool).unwrap_or(false);
        let step = self
            .app
            .call(move |app| {
                let Some(doc) = app.active() else {
                    return ImageStep::Done(Err(reviera_core::Error::new("document.none", "no document is open")));
                };
                let view = match &view {
                    Some(r) => match app.document(doc).and_then(|d| d.doc.store().resolve(r)) {
                        Some(v) => Some(v),
                        None => return ImageStep::Done(Err(reviera_core::Error::new("view.not_found", "no such view"))),
                    },
                    None => None,
                };
                if app.has_window() {
                    let (reply, rx) = std::sync::mpsc::channel();
                    app.request_view_image(reviera_app::view_image::ImageRequest { doc, view, max_size: max, gizmos, reply });
                    ImageStep::Wait(rx)
                } else {
                    ImageStep::Done(app.render_view_image(doc, view, max))
                }
            })
            .await
            .map_err(internal)?;
        let result = match step {
            ImageStep::Done(r) => r,
            ImageStep::Wait(rx) => tokio::task::spawn_blocking(move || {
                rx.recv_timeout(std::time::Duration::from_secs(30))
                    .unwrap_or_else(|_| Err(reviera_core::Error::new("view_image.timeout", "the window did not capture the view in time")))
            })
            .await
            .map_err(internal)?,
        };
        Ok(match result {
            Ok(img) => CallToolResult::success(vec![
                ContentBlock::image(base64::engine::general_purpose::STANDARD.encode(&img.png), "image/png"),
                ContentBlock::text(json!({ "view": img.view, "width": img.width, "height": img.height, "source": img.source }).to_string()),
            ]),
            Err(e) => CallToolResult::structured_error(json!({ "error": { "code": e.code, "message": e.message } })),
        })
    }
}

fn active_doc(app: &App) -> Result<&reviera_core::Document, Value> {
    app.active()
        .and_then(|id| app.document(id))
        .map(|d| &d.doc)
        .ok_or_else(|| json!({"code": "document.none", "message": "no document is open"}))
}

/// The element summary used by `get_element`, `query_elements` and element resources.
fn summary(doc: &reviera_core::Document, e: &Element) -> Value {
    let stamp = doc
        .stamp(e.id())
        .map(|s| json!({"counter": s.counter, "by": s.by}));
    // Measures also get a display value in the project's units.
    let units = reviera_core::units::settings_of(doc.store());
    let params: serde_json::Map<String, Value> = e
        .params
        .iter()
        .map(|(k, v)| {
            let display = v
                .get("type")
                .and_then(Value::as_str)
                .and_then(reviera_core::units::data_type_for_ifc)
                .zip(v.get("value").and_then(Value::as_f64))
                .and_then(|(dt, n)| reviera_core::units::format(n, dt.key, &units, None).ok());
            let mut v = v.clone();
            if let (Some(d), Some(obj)) = (display, v.as_object_mut()) {
                obj.insert("display".into(), json!(d));
            }
            (k.clone(), v)
        })
        .collect();
    json!({
        "id": e.id(),
        "guid": e.guid(),
        "class": e.class,
        "category": reviera_core::categories::category_of(e).map(|c| c.key),
        "is_type": reviera_core::categories::is_type(e),
        "name": e.name,
        "params": params,
        "last_modified": stamp,
    })
}

fn permission_denied(required: &str) -> Value {
    json!({"code": "permission.denied", "message": "this session's permission level does not allow it", "details": {"required": required}})
}

fn object(v: &Value) -> JsonObject {
    match v {
        Value::Object(m) => m.clone(),
        _ => JsonObject::new(),
    }
}

fn internal(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

fn schema(properties: Value, required: &[&str]) -> Arc<JsonObject> {
    Arc::new(object(
        &json!({"type": "object", "properties": properties, "required": required}),
    ))
}

/// Discovery and session tools, always present (DM-MCP-003, DM-MCP-004).
fn session_tools() -> Vec<Tool> {
    let ro = || ToolAnnotations::new().read_only(true);
    let rw = || ToolAnnotations::new().read_only(false);
    vec![
        Tool::new("find_commands", "Search every command, including disabled toolsets, by name and description.",
            schema(json!({"query": {"type": "string"}, "limit": {"type": "integer", "minimum": 1}}), &["query"])).with_annotations(ro()),
        Tool::new("describe_command", "A command's full definition with parameter and result schemas.",
            schema(json!({"name": {"type": "string"}}), &["name"])).with_annotations(ro()),
        Tool::new("run_command", "Run any command the session may run, even if its toolset is not enabled.",
            schema(json!({"name": {"type": "string"}, "arguments": {"type": "object"}}), &["name"])).with_annotations(rw()),
        Tool::new("toolsets.list", "List toolsets and whether they are enabled.", schema(json!({}), &[])).with_annotations(ro()),
        Tool::new("toolsets.enable", "Enable a toolset for this session.", schema(json!({"name": {"type": "string"}}), &["name"])).with_annotations(ro()),
        Tool::new("toolsets.disable", "Disable a toolset for this session.", schema(json!({"name": {"type": "string"}}), &["name"])).with_annotations(ro()),
        Tool::new("undo", "Undo this session's most recent operations.", schema(json!({"steps": {"type": "integer", "minimum": 1}}), &[])).with_annotations(rw()),
        Tool::new("redo", "Redo this session's undone operations.", schema(json!({"steps": {"type": "integer", "minimum": 1}}), &[])).with_annotations(rw()),
        Tool::new("history", "The undo history with the origin of each item.", schema(json!({}), &[])).with_annotations(ro()),
        Tool::new("query_elements", "Find elements by category (cat.doors …), IFC class and name, with paging. Category queries return occurrences unless types=true.",
            schema(json!({"category": {"type": "string"}, "types": {"type": "boolean"}, "class": {"type": "string"}, "name_contains": {"type": "string"}, "limit": {"type": "integer", "minimum": 1, "maximum": 1000}, "offset": {"type": "integer", "minimum": 0}}), &[])).with_annotations(ro()),
        Tool::new("get_element", "One element's summary: class, name, parameters and last change.",
            schema(json!({"id_or_guid": {"type": ["integer", "string"]}}), &["id_or_guid"])).with_annotations(ro()),
        Tool::new("session.begin_group", "Group the next calls so that one undo reverts them all.",
            schema(json!({"name": {"type": "string"}}), &["name"])).with_annotations(rw()),
        Tool::new("session.end_group", "End the group: assimilate into one undo item, commit separately, or roll back.",
            schema(json!({"mode": {"enum": ["assimilate", "commit", "rollback"]}}), &["mode"])).with_annotations(rw()),
        Tool::new("preview", "Run a command and roll it back, reporting what it would change.",
            schema(json!({"command": {"type": "string"}, "arguments": {"type": "object"}}), &["command"])).with_annotations(ro()),
        Tool::new("get_view_image", "A PNG image of a view as it is now, so you can look at the model: the view as the user sees it in the window (its camera, plan graphics, annotation, selection), or rendered offscreen without a window (plans and 3D views). view_id: the view's id or GlobalId (the active view when omitted; view.list finds views); max_size: the longest side in pixels (1024 by default); gizmos: keep the view cube.",
            schema(json!({"view_id": {"type": ["integer", "string"]}, "max_size": {"type": "integer", "minimum": 64, "maximum": 4096}, "gizmos": {"type": "boolean"}}), &[])).with_annotations(ro()),
        Tool::new("confirm", "Run a destructive command previewed in confirm mode.",
            schema(json!({"preview_token": {"type": "string"}}), &["preview_token"])).with_annotations(ToolAnnotations::new().destructive(true)),
    ]
}

const SESSION_TOOLS: &[&str] = &[
    "find_commands",
    "describe_command",
    "run_command",
    "toolsets.list",
    "toolsets.enable",
    "toolsets.disable",
    "undo",
    "redo",
    "history",
    "query_elements",
    "get_element",
    "session.begin_group",
    "session.end_group",
    "preview",
    "confirm",
    "get_view_image",
];

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_server_info(Implementation::new("reviera", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "BIM Reviera: BIM authoring on native IFC. Tools are Reviera commands; use find_commands \
                 to discover commands in toolsets that are not enabled, and run_command to call them. Results \
                 are {result, warnings, changed}. Undo only reverts this session's own changes. \
                 get_view_image returns a PNG of a view, to look at what you did.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let enabled = self.session.lock().unwrap().enabled.clone();
        let mut tools = session_tools();
        tools.extend(
            self.specs()
                .await?
                .iter()
                .filter(|s| {
                    enabled.contains(&toolset_of(&s.name))
                        && !SESSION_TOOLS.contains(&s.name.as_str())
                })
                .map(|s| self.command_tool(s)),
        );
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if let Some(info) = context.peer.peer_info() {
            // A session the window named (UI-AGENT-004) keeps its name; others take the client's.
            if self.config.client_name.is_none() {
                self.session.lock().unwrap().client = info.client_info.name.clone();
            }
        }
        let args = Value::Object(request.arguments.clone().unwrap_or_default());
        // Images are content blocks, not structured results (DM-MCP-004).
        if request.name == "get_view_image" {
            return Ok(self.view_image(args).await?.into());
        }
        let outcome = self.call(&request.name, args).await?;
        Ok(match outcome {
            Ok(v) => CallToolResult::structured(v),
            Err(e) => CallToolResult::structured_error(json!({ "error": e })),
        }
        .into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let docs = self
            .app
            .call(|app| app.documents())
            .await
            .map_err(internal)?;
        let mut resources = vec![
            Resource::new("reviera://documents", "documents").with_mime_type("application/json"),
            Resource::new("reviera://commands", "commands").with_mime_type("application/json"),
        ];
        for d in docs {
            resources.push(
                Resource::new(
                    format!("reviera://doc/{}/history", d.id),
                    format!("{} history", d.title),
                )
                .with_mime_type("application/json"),
            );
        }
        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let templates = [
            (
                "reviera://element/{id_or_guid}",
                "element (active document)",
            ),
            ("reviera://doc/{doc}/element/{id_or_guid}", "element"),
            ("reviera://doc/{doc}/history", "undo history"),
            ("reviera://command/{name}", "command definition"),
        ]
        .into_iter()
        .map(|(uri, name)| ResourceTemplate::new(uri, name).with_mime_type("application/json"))
        .collect();
        Ok(ListResourceTemplatesResult::with_all_items(templates))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let value = self.read(&request.uri).await?;
        let text = serde_json::to_string_pretty(&value).map_err(internal)?;
        let contents = ResourceContents::text(text, request.uri).with_mime_type("application/json");
        Ok(ReadResourceResult::new(vec![contents]).into())
    }
}
