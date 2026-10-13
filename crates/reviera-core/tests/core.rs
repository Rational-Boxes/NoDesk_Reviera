// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Acceptance tests for transactions, undo, identity, versions and the command executor
//! (spec Ch 2: DM-TXN-001, DM-TXN-002, DM-ID-001, DM-ID-002, DM-CMD-001, DM-CMD-002, DM-MCP-005).

use reviera_core::builtin::NoParams;
use reviera_core::document::GroupEnd;
use reviera_core::{
    Caller, Command, CommandContext, CommandMeta, CommandRegistry, CommandSpec, Document, Effect,
    ElementId, NewElement, Origin, Permission, Result, Warning,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateWalls {
    count: usize,
    #[serde(default)]
    fail_after: bool,
    #[serde(default)]
    panic_after: bool,
}

struct TestCreate;

impl Command for TestCreate {
    type Params = CreateWalls;
    type Output = Vec<ElementId>;
    const META: CommandMeta = CommandMeta {
        name: "test.create_walls",
        version: 1,
        summary: "",
        description: "",
        effects: &[Effect::Edit],
        transaction_name: "Wall",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: CreateWalls) -> Result<Vec<ElementId>> {
        let txn = cx.edit()?;
        let ids = (0..p.count)
            .map(|i| {
                txn.create(
                    NewElement::new("IfcWall")
                        .name(format!("Wall {i}"))
                        .param("height", 3.0),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        if p.fail_after {
            return Err(reviera_core::Error::new(
                "test.failed",
                "script raised an error",
            ));
        }
        if p.panic_after {
            panic!("command bug");
        }
        txn.warn(Warning {
            id: "test.created".into(),
            message: "created".into(),
            element_ids: ids.clone(),
        });
        Ok(ids)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SetParam {
    id: ElementId,
    name: String,
    value: Value,
}

struct TestSet;

impl Command for TestSet {
    type Params = SetParam;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "test.set_param",
        version: 1,
        summary: "",
        description: "",
        effects: &[Effect::Edit],
        transaction_name: "Modify",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: SetParam) -> Result<()> {
        cx.edit()?.set_param(p.id, p.name, p.value)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteParams {
    id: ElementId,
}

struct TestDelete;

impl Command for TestDelete {
    type Params = DeleteParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "test.delete",
        version: 1,
        summary: "",
        description: "",
        effects: &[Effect::Edit, Effect::Destructive],
        transaction_name: "Delete",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: DeleteParams) -> Result<()> {
        cx.edit()?.delete(p.id)
    }
}

struct TestSneakyRead;

impl Command for TestSneakyRead {
    type Params = NoParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "test.sneaky_read",
        version: 1,
        summary: "",
        description: "",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, _: NoParams) -> Result<()> {
        cx.edit()?.create(NewElement::new("IfcWall"))?;
        Ok(())
    }
}

fn setup() -> (CommandRegistry, Document) {
    let mut r = CommandRegistry::with_core();
    r.add(TestCreate).unwrap();
    r.add(TestSet).unwrap();
    r.add(TestDelete).unwrap();
    r.add(TestSneakyRead).unwrap();
    (r, Document::new())
}

fn user() -> Caller {
    Caller::user()
}

fn mcp(session: &str, permission: Permission) -> Caller {
    Caller {
        origin: Origin::mcp("agent", session),
        permission,
    }
}

fn walls(r: &CommandRegistry, d: &mut Document, n: usize) -> Vec<ElementId> {
    let out = r
        .execute(d, "test.create_walls", json!({ "count": n }), &user())
        .unwrap();
    serde_json::from_value(out.result).unwrap()
}

fn code(e: reviera_core::Error) -> String {
    e.code.into_owned()
}

#[test]
fn error_in_command_leaves_no_elements_and_no_undo_item() {
    let (r, mut d) = setup();
    let v = d.version();
    let e = r
        .execute(
            &mut d,
            "test.create_walls",
            json!({"count": 10, "fail_after": true}),
            &user(),
        )
        .unwrap_err();
    assert_eq!(code(e), "test.failed");
    assert!(d.store().is_empty());
    assert_eq!(d.undo_history().count(), 0);
    assert_eq!(d.version(), v);
}

#[test]
fn panic_in_command_rolls_back() {
    let (r, mut d) = setup();
    let e = r
        .execute(
            &mut d,
            "test.create_walls",
            json!({"count": 3, "panic_after": true}),
            &user(),
        )
        .unwrap_err();
    assert_eq!(code(e), "command.panicked");
    assert!(d.store().is_empty());
}

#[test]
fn outcome_reports_result_warnings_and_changes() {
    let (r, mut d) = setup();
    let out = r
        .execute(&mut d, "test.create_walls", json!({"count": 2}), &user())
        .unwrap();
    assert_eq!(out.changed.added, vec![ElementId(1000), ElementId(1001)]);
    assert_eq!(out.warnings.len(), 1);
    assert_eq!(out.result, json!([1000, 1001]));
}

#[test]
fn assimilated_group_is_one_undo_item() {
    let (r, mut d) = setup();
    d.group_begin("Create stair", Origin::ui()).unwrap();
    for _ in 0..3 {
        walls(&r, &mut d, 1);
    }
    d.group_end(GroupEnd::Assimilate).unwrap();
    let names: Vec<_> = d.undo_history().map(|i| i.name.clone()).collect();
    assert_eq!(names, ["Create stair"]);
    r.execute(&mut d, "edit.undo", json!({}), &user()).unwrap();
    assert!(d.store().is_empty());
}

#[test]
fn group_rollback_and_commit() {
    let (r, mut d) = setup();
    d.group_begin("Try", Origin::ui()).unwrap();
    walls(&r, &mut d, 2);
    assert!(
        r.execute(&mut d, "edit.undo", json!({}), &user()).is_err(),
        "undo is blocked inside a group"
    );
    let c = d.group_end(GroupEnd::Rollback).unwrap();
    assert_eq!(c.changed.deleted.len(), 2);
    assert!(d.store().is_empty());
    assert_eq!(d.undo_history().count(), 0);

    d.group_begin("Keep", Origin::ui()).unwrap();
    walls(&r, &mut d, 1);
    walls(&r, &mut d, 1);
    d.group_end(GroupEnd::Commit).unwrap();
    assert_eq!(d.undo_history().count(), 2);
}

#[test]
fn snapshot_reads_committed_state_during_a_transaction() {
    let (r, mut d) = setup();
    walls(&r, &mut d, 1);
    let snap = d.snapshot();
    let mut txn = d.transaction("Long", Origin::mcp("agent", "s1"));
    txn.create(NewElement::new("IfcWall")).unwrap();
    let reader = std::thread::spawn(move || snap.store().len());
    assert_eq!(reader.join().unwrap(), 1);
    assert_eq!(txn.store().len(), 2);
}

#[test]
fn savepoints_roll_back_without_ending_the_transaction() {
    let mut d = Document::new();
    let mut txn = d.transaction("Edit", Origin::ui());
    let a = txn.create(NewElement::new("IfcWall")).unwrap();
    let sp = txn.savepoint();
    txn.create(NewElement::new("IfcWall")).unwrap();
    txn.set_param(a, "height", 4.0).unwrap();
    txn.rollback_to(sp).unwrap();
    txn.commit();
    assert_eq!(d.store().len(), 1);
    assert_eq!(d.store().get(a).unwrap().param("height"), None);
    assert_eq!(d.undo_history().count(), 1, "savepoints add no undo items");
}

#[test]
fn ids_start_at_1000_and_are_never_reused() {
    let (r, mut d) = setup();
    let _ = r.execute(
        &mut d,
        "test.create_walls",
        json!({"count": 2, "fail_after": true}),
        &user(),
    );
    assert_eq!(walls(&r, &mut d, 1), [ElementId(1002)]);
}

#[test]
fn fifty_edits_undo_to_the_original_state() {
    let (r, mut d) = setup();
    let ids = walls(&r, &mut d, 5);
    let original: Vec<_> = d.store().iter().cloned().collect();
    for i in 0..50 {
        let id = ids[i % ids.len()];
        match i % 3 {
            0 => {
                r.execute(
                    &mut d,
                    "test.set_param",
                    json!({"id": id, "name": "height", "value": i}),
                    &user(),
                )
                .unwrap();
            }
            1 => {
                walls(&r, &mut d, 1);
            }
            _ => {
                let extra = walls(&r, &mut d, 1)[0];
                r.execute(&mut d, "test.delete", json!({"id": extra}), &user())
                    .unwrap();
            }
        }
    }
    let n = d.undo_history().count();
    assert!(n >= 50);
    r.execute(&mut d, "edit.undo", json!({"steps": n - 1}), &user())
        .unwrap();
    assert_eq!(d.store().iter().cloned().collect::<Vec<_>>(), original);
    r.execute(&mut d, "edit.redo", json!({"steps": 3}), &user())
        .unwrap();
    assert_eq!(d.redo_history().count(), n - 4);
    walls(&r, &mut d, 1);
    assert_eq!(d.redo_history().count(), 0, "a new transaction clears redo");
}

#[test]
fn version_stamps_and_changed_since() {
    let (r, mut d) = setup();
    let ids = walls(&r, &mut d, 3);
    let v0 = d.version();
    assert_eq!(d.changed_since(v0).changed, []);

    r.execute(
        &mut d,
        "test.set_param",
        json!({"id": ids[0], "name": "offset", "value": 1.5}),
        &user(),
    )
    .unwrap();
    let v1 = d.version();
    assert_eq!(v1.counter, v0.counter + 1);
    assert_eq!(d.changed_since(v0).changed, [ids[0]]);

    r.execute(&mut d, "edit.undo", json!({}), &user()).unwrap();
    assert_eq!(d.version().counter, v1.counter + 1, "undo is a new change");
    assert_eq!(d.changed_since(v1).changed, [ids[0]]);

    let before_delete = d.version();
    r.execute(&mut d, "test.delete", json!({"id": ids[1]}), &user())
        .unwrap();
    let since = d.changed_since(before_delete);
    assert_eq!((since.changed, since.deleted), (vec![], vec![ids[1]]));

    assert!(d.changed_since(Document::new().version()).stale_episode);
    assert_eq!(d.stamp(ids[2]).unwrap().by, "user");
}

#[test]
fn identical_results_are_not_changes() {
    let (r, mut d) = setup();
    let id = walls(&r, &mut d, 1)[0];
    let v = d.version();
    let out = r
        .execute(
            &mut d,
            "test.set_param",
            json!({"id": id, "name": "height", "value": 3.0}),
            &user(),
        )
        .unwrap();
    assert!(out.changed.is_empty());
    assert_eq!(d.version(), v);
    assert_eq!(d.undo_history().count(), 1);
}

#[test]
fn mcp_model_edit_session_cannot_undo_other_sessions() {
    let (r, mut d) = setup();
    walls(&r, &mut d, 1);
    let agent = mcp("s1", Permission::ModelEdit);
    let e = r
        .execute(&mut d, "edit.undo", json!({}), &agent)
        .unwrap_err();
    assert_eq!(code(e), "undo.not_owned");

    r.execute(&mut d, "test.create_walls", json!({"count": 1}), &agent)
        .unwrap();
    assert_eq!(
        d.undo_history()
            .next()
            .unwrap()
            .origin
            .session_id
            .as_deref(),
        Some("s1")
    );
    r.execute(&mut d, "edit.undo", json!({}), &agent).unwrap();
    assert_eq!(d.store().len(), 1);
    r.execute(
        &mut d,
        "edit.undo",
        json!({}),
        &mcp("s2", Permission::FullScripting),
    )
    .unwrap();
    assert!(d.store().is_empty());
}

#[test]
fn read_only_session_is_denied_edits() {
    let (r, mut d) = setup();
    let e = r
        .execute(
            &mut d,
            "test.create_walls",
            json!({"count": 1}),
            &mcp("s", Permission::ReadOnly),
        )
        .unwrap_err();
    assert_eq!(code(e.clone()), "permission.denied");
    assert_eq!(e.details["required"], "model_edit");
    assert!(d.store().is_empty());
    r.execute(
        &mut d,
        "model.version",
        Value::Null,
        &mcp("s", Permission::ReadOnly),
    )
    .unwrap();
}

#[test]
fn read_commands_cannot_write() {
    let (r, mut d) = setup();
    let e = r
        .execute(&mut d, "test.sneaky_read", json!({}), &user())
        .unwrap_err();
    assert_eq!(code(e), "effect.read_only");
}

#[test]
fn arguments_are_validated_against_the_schema() {
    let (r, mut d) = setup();
    let e = r
        .execute(
            &mut d,
            "test.create_walls",
            json!({"count": "many"}),
            &user(),
        )
        .unwrap_err();
    assert_eq!(code(e), "args.invalid");
    let e = r
        .execute(
            &mut d,
            "edit.undo",
            json!({"steps": 1, "extra": true}),
            &user(),
        )
        .unwrap_err();
    assert_eq!(code(e), "args.invalid");
    let e = r
        .execute(&mut d, "no.such", json!({}), &user())
        .unwrap_err();
    assert_eq!(code(e), "registry.unknown_command");
}

#[test]
fn addin_commands_cannot_use_reserved_domains() {
    let (mut r, mut d) = setup();
    let spec = |name: &str| CommandSpec {
        name: name.into(),
        version: 1,
        summary: "Renumber rooms".into(),
        description: String::new(),
        params: json!({"type": "object"}),
        result: json!({}),
        effects: vec![Effect::Read],
        undoable: false,
        transaction_name: String::new(),
    };
    let e = r
        .register_addin(spec("wall.create"), |_, _| Ok(Value::Null))
        .unwrap_err();
    assert_eq!(code(e), "registry.reserved_domain");
    r.register_addin(spec("org.example.roomtools.renumber"), |cx, _| {
        Ok(json!(cx.store().len()))
    })
    .unwrap();
    let out = r
        .execute(&mut d, "org.example.roomtools.renumber", json!({}), &user())
        .unwrap();
    assert_eq!(out.result, json!(0));
}

#[test]
fn identity_commands_accept_ids_or_guids() {
    let (r, mut d) = setup();
    let id = walls(&r, &mut d, 1)[0];
    let guid = d.store().get(id).unwrap().guid().clone();
    let out = r
        .execute(
            &mut d,
            "element.get_ids",
            json!({"element_ids": [id, guid.as_str()]}),
            &user(),
        )
        .unwrap();
    assert_eq!(
        out.result,
        json!([{"id": 1000, "guid": guid.as_str()}, {"id": 1000, "guid": guid.as_str()}])
    );
    let out = r
        .execute(
            &mut d,
            "element.find_by_guid",
            json!({"guid": guid.as_str()}),
            &user(),
        )
        .unwrap();
    assert_eq!(out.result, json!(1000));
    let e = r
        .execute(
            &mut d,
            "element.find_by_guid",
            json!({"guid": "bad"}),
            &user(),
        )
        .unwrap_err();
    assert_eq!(code(e), "args.invalid");
}

#[test]
fn registry_introspection() {
    let (r, mut d) = setup();
    let out = r
        .execute(
            &mut d,
            "registry.list",
            json!({"filter": {"prefix": "edit."}}),
            &user(),
        )
        .unwrap();
    let names: Vec<_> = out
        .result
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].clone())
        .collect();
    assert_eq!(
        names,
        [
            json!("edit.history"),
            json!("edit.redo"),
            json!("edit.undo")
        ]
    );
    let out = r
        .execute(
            &mut d,
            "registry.describe",
            json!({"name": "edit.undo"}),
            &user(),
        )
        .unwrap();
    assert_eq!(out.result["params"]["properties"]["steps"]["minimum"], 1);
    assert_eq!(out.result["undoable"], false);
    assert!(r.get("test.create_walls").unwrap().undoable);
}

#[test]
fn undo_history_is_bounded() {
    let (r, mut d) = setup();
    d.set_max_undo_items(3);
    for _ in 0..5 {
        walls(&r, &mut d, 1);
    }
    assert_eq!(d.undo_history().count(), 3);
    let out = r
        .execute(&mut d, "edit.history", json!({}), &user())
        .unwrap();
    assert_eq!(out.result.as_array().unwrap().len(), 3);
    assert_eq!(out.result[0]["origin"]["source"], "ui");
}
