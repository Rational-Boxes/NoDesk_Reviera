// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! MCP acceptance tests (spec Ch 2 §2.16), with a real MCP client over an in-memory pipe.

use std::time::Duration;

use reviera_app::{App, AppConfig};
use reviera_core::{
    Caller, Command, CommandContext, CommandMeta, Effect, ElementId, NewElement, Permission, Result,
};
use reviera_mcp::{AppHandle, McpConfig, McpServer};
use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation,
    ReadResourceRequestParams, ResourceContents,
};
use rmcp::service::{RoleClient, RunningService};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize, JsonSchema)]
struct WallParams {
    name: String,
}

struct AddWall;

impl Command for AddWall {
    type Params = WallParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "wall.add_test",
        version: 1,
        summary: "Add a test wall",
        description: "Creates an IfcWall with the given name.",
        effects: &[Effect::Edit],
        transaction_name: "Wall",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: WallParams) -> Result<ElementId> {
        cx.edit()?.create(NewElement::new("IfcWall").name(p.name))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteParams {
    id: ElementId,
}

struct DeleteElement;

impl Command for DeleteElement {
    type Params = DeleteParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "element.delete_test",
        version: 1,
        summary: "Delete an element",
        description: "",
        effects: &[Effect::Edit, Effect::Destructive],
        transaction_name: "Delete",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: DeleteParams) -> Result<()> {
        cx.edit()?.delete(p.id)
    }
}

struct Harness {
    client: RunningService<RoleClient, ClientConfig>,
    _dir: tempfile::TempDir,
}

impl Harness {
    async fn start(permission: Permission) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (live_dir, untitled) = (dir.path().join("live"), dir.path().join("untitled"));
        let app = AppHandle::spawn(move || {
            let live = reviera_livestore::Options {
                dir: live_dir,
                group_commit: Duration::from_millis(20),
                encryption: reviera_livestore::Encryption::Passphrase("test".into()),
                ..Default::default()
            };
            let mut app = App::new(AppConfig {
                live: Some(live),
                untitled_dir: untitled,
            });
            let registry = app.registry_mut().unwrap();
            registry.add(AddWall).unwrap();
            registry.add(DeleteElement).unwrap();
            app.execute("file.new", json!({}), &Caller::user()).unwrap();
            app
        });
        let (server_io, client_io) = tokio::io::duplex(1 << 20);
        let server = McpServer::new(app, McpConfig::new(permission));
        tokio::spawn(async move {
            let running = server.serve(server_io).await.unwrap();
            let _ = running.waiting().await;
        });
        let info = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("test-agent", "1.0"),
        );
        let client = info.serve(client_io).await.unwrap();
        Self { client, _dir: dir }
    }

    /// Calls a tool; returns Ok(structured content) or Err(structured error).
    async fn call(&self, tool: &str, args: Value) -> std::result::Result<Value, Value> {
        let mut params = CallToolRequestParams::new(tool.to_owned());
        if let Value::Object(map) = args {
            params = params.with_arguments(map);
        }
        let r = self.client.call_tool(params).await.unwrap();
        let content = r.structured_content.clone().unwrap_or(Value::Null);
        if r.is_error == Some(true) {
            Err(content["error"].clone())
        } else {
            Ok(content)
        }
    }

    async fn read(&self, uri: &str) -> Value {
        let r = self
            .client
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .unwrap();
        match &r.contents[0] {
            ResourceContents::TextResourceContents { text, .. } => {
                serde_json::from_str(text).unwrap()
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    async fn tool_names(&self) -> Vec<String> {
        self.client
            .list_all_tools()
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.name.into_owned())
            .collect()
    }
}

#[tokio::test]
async fn tools_are_generated_from_the_registry_in_toolsets() {
    let h = Harness::start(Permission::ModelEdit).await;
    let names = h.tool_names().await;
    for t in [
        "edit.undo",
        "model.version",
        "find_commands",
        "run_command",
        "get_element",
        "session.begin_group",
    ] {
        assert!(names.contains(&t.to_owned()), "{t} missing");
    }
    assert!(
        !names.contains(&"wall.add_test".to_owned()),
        "modeling is not enabled by default"
    );
    assert!(!names.contains(&"file.open".to_owned()));

    let tools = h.client.list_all_tools().await.unwrap();
    let version = tools.iter().find(|t| t.name == "model.version").unwrap();
    assert_eq!(
        version.annotations.as_ref().unwrap().read_only_hint,
        Some(true)
    );

    // Disabled toolsets are still discoverable and runnable.
    let found = h
        .call("find_commands", json!({"query": "add test wall"}))
        .await
        .unwrap();
    assert_eq!(found["commands"][0]["name"], "wall.add_test");
    assert_eq!(found["commands"][0]["toolset"], "modeling");
    let direct = h
        .call("wall.add_test", json!({"name": "W"}))
        .await
        .unwrap_err();
    assert_eq!(direct["code"], "mcp.toolset_disabled");
    let via_run = h
        .call(
            "run_command",
            json!({"name": "wall.add_test", "arguments": {"name": "W1"}}),
        )
        .await
        .unwrap();
    assert_eq!(via_run["changed"]["added"].as_array().unwrap().len(), 1);

    h.call("toolsets.enable", json!({"name": "modeling"}))
        .await
        .unwrap();
    assert!(h.tool_names().await.contains(&"wall.add_test".to_owned()));
    let described = h
        .call("describe_command", json!({"name": "wall.add_test"}))
        .await
        .unwrap();
    assert_eq!(described["command"]["params"]["required"], json!(["name"]));
}

#[tokio::test]
async fn read_only_sessions_cannot_edit() {
    let h = Harness::start(Permission::ReadOnly).await;
    let e = h
        .call(
            "run_command",
            json!({"name": "wall.add_test", "arguments": {"name": "W"}}),
        )
        .await
        .unwrap_err();
    assert_eq!(e["code"], "permission.denied");
    assert_eq!(e["details"]["required"], "model_edit");
    let v = h.call("model.version", json!({})).await.unwrap();
    assert_eq!(v["result"]["counter"], 0, "the model is unchanged");
    assert_eq!(
        h.call("session.begin_group", json!({"name": "x"}))
            .await
            .unwrap_err()["code"],
        "permission.denied"
    );
}

#[tokio::test]
async fn an_agent_task_is_one_undo_item_attributed_to_the_agent() {
    let h = Harness::start(Permission::ModelEdit).await;
    h.call("session.begin_group", json!({"name": "Create room"}))
        .await
        .unwrap();
    for i in 0..4 {
        h.call(
            "run_command",
            json!({"name": "wall.add_test", "arguments": {"name": format!("Wall {i}")}}),
        )
        .await
        .unwrap();
    }
    h.call("session.end_group", json!({"mode": "assimilate"}))
        .await
        .unwrap();

    let history = h.call("history", json!({})).await.unwrap();
    assert_eq!(
        history["result"][0]["name"],
        "Create room (agent: test-agent)"
    );
    assert_eq!(history["result"][0]["origin"]["source"], "mcp");
    assert_eq!(history["result"][0]["origin"]["actor"], "test-agent");
    let walls = h
        .call("query_elements", json!({"class": "IfcWall"}))
        .await
        .unwrap();
    assert_eq!(walls["total"], 4);
    assert_eq!(walls["items"][0]["last_modified"]["by"], "mcp:test-agent");

    h.call("undo", json!({})).await.unwrap();
    assert_eq!(
        h.call("query_elements", json!({"class": "IfcWall"}))
            .await
            .unwrap()["total"],
        0
    );
}

#[tokio::test]
async fn destructive_commands_need_confirmation_in_model_edit() {
    let h = Harness::start(Permission::ModelEdit).await;
    let id = h
        .call(
            "run_command",
            json!({"name": "wall.add_test", "arguments": {"name": "Doomed"}}),
        )
        .await
        .unwrap()["result"]
        .clone();
    let before = h.call("model.version", json!({})).await.unwrap()["result"].clone();

    let preview = h
        .call(
            "run_command",
            json!({"name": "element.delete_test", "arguments": {"id": id}}),
        )
        .await
        .unwrap();
    assert_eq!(preview["confirmation_required"], true);
    assert_eq!(preview["changed"]["deleted"], json!([id]));
    assert_eq!(
        h.call("model.version", json!({})).await.unwrap()["result"],
        before,
        "nothing changed yet"
    );
    assert!(
        h.call("get_element", json!({"id_or_guid": id}))
            .await
            .is_ok()
    );

    let token = preview["preview_token"].as_str().unwrap().to_owned();
    h.call("confirm", json!({"preview_token": token}))
        .await
        .unwrap();
    assert_eq!(
        h.call("get_element", json!({"id_or_guid": id}))
            .await
            .unwrap_err()["code"],
        "element.not_found"
    );
    assert_eq!(
        h.call("confirm", json!({"preview_token": token}))
            .await
            .unwrap_err()["code"],
        "confirm.unknown_token"
    );

    // Explicit preview leaves the model unchanged.
    let id2 = h
        .call(
            "run_command",
            json!({"name": "wall.add_test", "arguments": {"name": "Kept"}}),
        )
        .await
        .unwrap()["result"]
        .clone();
    let v = h.call("model.version", json!({})).await.unwrap()["result"].clone();
    let p = h
        .call(
            "preview",
            json!({"command": "element.delete_test", "arguments": {"id": id2}}),
        )
        .await
        .unwrap();
    assert_eq!(p["changed"]["deleted"], json!([id2]));
    assert_eq!(
        h.call("model.version", json!({})).await.unwrap()["result"],
        v
    );
}

#[tokio::test]
async fn resources_expose_documents_elements_and_commands() {
    let h = Harness::start(Permission::FullScripting).await;
    let docs = h.read("reviera://documents").await;
    assert_eq!(docs.as_array().unwrap().len(), 1);
    assert_eq!(docs[0]["title"], "Untitled");

    let storey = h
        .call("query_elements", json!({"class": "IfcBuildingStorey"}))
        .await
        .unwrap()["items"][0]
        .clone();
    assert_eq!(storey["name"], "Level 1");
    assert_eq!(storey["category"], "cat.levels");
    let levels = h
        .call("query_elements", json!({"category": "cat.levels"}))
        .await
        .unwrap();
    assert_eq!(levels["total"], 1);
    // Measures carry a display value in project units; unit strings are accepted.
    h.call("run_command", json!({"name": "pset.set", "arguments": {
        "element_id": storey["id"], "pset_name": "Pset_BuildingStoreyCommon", "property_name": "GrossHeight",
        "value": "3 m", "ifc_type": "IfcLengthMeasure"}})).await.unwrap();
    let got = h
        .call("get_element", json!({"id_or_guid": storey["id"]}))
        .await
        .unwrap();
    assert_eq!(
        got["params"]["Pset_BuildingStoreyCommon.GrossHeight"]["value"],
        3.0
    );
    assert_eq!(
        got["params"]["Pset_BuildingStoreyCommon.GrossHeight"]["display"],
        "3000"
    );
    let by_guid = h
        .read(&format!(
            "reviera://element/{}",
            storey["guid"].as_str().unwrap()
        ))
        .await;
    assert_eq!(by_guid["id"], storey["id"]);
    let by_id = h
        .read(&format!("reviera://doc/1/element/{}", storey["id"]))
        .await;
    assert_eq!(by_id["guid"], storey["guid"]);

    let commands = h.read("reviera://commands").await;
    assert!(
        commands
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "file.open" && c["toolset"] == "io")
    );
    let one = h.read("reviera://command/edit.undo").await;
    assert_eq!(one["name"], "edit.undo");
    let templates = h.client.list_all_resource_templates().await.unwrap();
    assert!(
        templates
            .iter()
            .any(|t| t.uri_template == "reviera://element/{id_or_guid}")
    );
}
