// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The single command registry (DM-CMD-001, DM-CMD-002). Each command is described once and
//! surfaced automatically to the ribbon and shortcuts, to Lua as `reviera.<command>(...)` and to MCP
//! as one tool per command. [`CommandRegistry::execute`] is the one execution path for every caller.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::change::ChangedSummary;
use crate::document::{Commit, Document};
use crate::error::{Error, Result};
use crate::origin::{Caller, Permission};
use crate::store::Store;
use crate::txn::{Transaction, Warning};

/// What a command may do. Permissions and undo behaviour are derived from these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Read,
    Edit,
    FileRead,
    FileWrite,
    Network,
    Script,
    Ui,
    Destructive,
    Settings,
}

/// Domains reserved for Reviera's own commands; add-in commands cannot use them (DM-CMD-002).
pub const RESERVED_DOMAINS: &[&str] = &[
    "ui",
    "enclosure",
    "glazing",
    "frame",
    "growing",
    "vent",
    "view",
    "sheet",
    "schedule",
    "annotate",
    "wall",
    "floor",
    "roof",
    "door",
    "modify",
    "str",
    "mep",
    "analysis",
    "bimserver",
    "fileengine",
    "docs",
    "merge",
    "stash",
    "ownership",
    "profiler",
    "link",
    "workset",
    "element",
    "instance",
    "type",
    "family",
    "family_doc",
    "cme",
    "param",
    "paramlib",
    "global",
    "units",
    "material",
    "object_style",
    "line_pattern",
    "line_style",
    "line_weight",
    "fill_pattern",
    "project",
    "location",
    "coordinates",
    "position",
    "site",
    "building",
    "spatial",
    "phase",
    "standards",
    "purge",
    "classification",
    "keynotes",
    "keynote",
    "spot",
    "legend",
    "file",
    "import",
    "export",
    "validate",
    "rxd",
    "edit",
    "warnings",
    "failure",
    "regen",
    "registry",
    "script",
    "addin",
    "graph",
    "mcp",
    "model",
    "settings",
    "snaps",
    "image",
    "pset",
    "qto",
    "host",
    "subcategory",
    "formula",
    "journal",
    "external_props",
    "library",
    "numbering",
    "decal",
    "txn",
];

/// A command's published definition: the source of its UI, Lua, MCP and documentation surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct CommandSpec {
    /// `domain.verb` or `domain.sub.verb`, lower snake case.
    pub name: String,
    /// Increased on any breaking change to parameters or result.
    pub version: u32,
    pub summary: String,
    pub description: String,
    /// JSON Schema (draft 2020-12) of the arguments.
    pub params: Value,
    /// JSON Schema of the result.
    pub result: Value,
    pub effects: Vec<Effect>,
    pub undoable: bool,
    pub transaction_name: String,
}

impl CommandSpec {
    /// The lowest permission level that may run the command (DM-MCP-005). The `model_edit` limits on
    /// `file_write` (output folder only) and `destructive` (confirm mode) are enforced by the MCP layer.
    pub fn min_permission(&self) -> Permission {
        use Effect::*;
        if self.effects.iter().all(|e| *e == Read) {
            Permission::ReadOnly
        } else if self
            .effects
            .iter()
            .all(|e| matches!(e, Read | Edit | Settings | Ui | FileWrite | Destructive))
        {
            Permission::ModelEdit
        } else {
            Permission::FullScripting
        }
    }

    fn edits(&self) -> bool {
        self.effects.contains(&Effect::Edit)
    }
}

/// Static description of a Rust command; the schemas come from its parameter and output types.
#[derive(Debug, Clone, Copy)]
pub struct CommandMeta {
    pub name: &'static str,
    pub version: u32,
    pub summary: &'static str,
    pub description: &'static str,
    pub effects: &'static [Effect],
    /// Undo name, for example "Wall" or "Modify type attributes".
    pub transaction_name: &'static str,
}

/// A typed command implemented in Rust.
pub trait Command: Send + Sync + 'static {
    type Params: DeserializeOwned + JsonSchema;
    type Output: Serialize + JsonSchema;
    const META: CommandMeta;

    fn run(&self, cx: &mut CommandContext<'_>, params: Self::Params) -> Result<Self::Output>;
}

/// The result of every command call: `{result, warnings[], changed}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outcome {
    pub result: Value,
    pub warnings: Vec<Warning>,
    pub changed: ChangedSummary,
}

// One per command call, on the stack; boxing the transaction would only add an allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum State<'a> {
    Read(&'a Document),
    Edit(Transaction<'a>),
    /// Core commands that drive the document themselves (undo, redo).
    Direct(&'a mut Document),
}

/// What a running command can see and do.
#[derive(Debug)]
pub struct CommandContext<'a> {
    registry: &'a CommandRegistry,
    caller: &'a Caller,
    state: State<'a>,
    pub(crate) direct_commit: Option<Commit>,
}

impl<'a> CommandContext<'a> {
    pub fn caller(&self) -> &Caller {
        self.caller
    }

    pub fn registry(&self) -> &CommandRegistry {
        self.registry
    }

    /// The model as the command currently sees it, including its own uncommitted edits.
    pub fn store(&self) -> &Store {
        match &self.state {
            State::Read(d) => d.store(),
            State::Edit(t) => t.store(),
            State::Direct(d) => d.store(),
        }
    }

    /// The document, for read-only commands that need more than the store (versions, history).
    pub fn document(&self) -> Result<&Document> {
        match &self.state {
            State::Read(d) => Ok(d),
            State::Direct(d) => Ok(d),
            State::Edit(_) => Err(Error::new(
                "txn.open",
                "the document is inside this command's transaction",
            )),
        }
    }

    /// The command's transaction. Only commands with the `edit` effect have one.
    pub fn edit(&mut self) -> Result<&mut Transaction<'a>> {
        match &mut self.state {
            State::Edit(t) => Ok(t),
            _ => Err(Error::new(
                "effect.read_only",
                "the command is not declared with the `edit` effect",
            )),
        }
    }

    pub(crate) fn document_mut(&mut self) -> Result<&mut Document> {
        match &mut self.state {
            State::Direct(d) => Ok(d),
            _ => Err(Error::new(
                "internal",
                "the command has no direct document access",
            )),
        }
    }
}

type DocHandler = Box<dyn Fn(&mut CommandContext<'_>, Value) -> Result<Value> + Send + Sync>;
type HostHandler = Box<dyn Fn(&mut dyn Any, &Caller, Value) -> Result<Value> + Send + Sync>;

enum Handler {
    /// Runs against one document, in a transaction when it edits.
    Document { run: DocHandler, direct: bool },
    /// Runs against the application (open, save, close, recovery …).
    Host(HostHandler),
}

struct Entry {
    spec: CommandSpec,
    validator: jsonschema::Validator,
    handler: Handler,
}

/// A command that acts on the application rather than on one document, such as `file.open`. The
/// application passes itself as the host to [`CommandRegistry::execute_host`].
pub trait HostCommand: Send + Sync + 'static {
    type Host: 'static;
    type Params: DeserializeOwned + JsonSchema;
    type Output: Serialize + JsonSchema;
    const META: CommandMeta;

    fn run(
        &self,
        host: &mut Self::Host,
        caller: &Caller,
        params: Self::Params,
    ) -> Result<Self::Output>;
}

#[derive(Default)]
pub struct CommandRegistry {
    commands: BTreeMap<String, Entry>,
}

impl fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandRegistry")
            .field("commands", &self.commands.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry holding the core commands defined in this crate.
    pub fn with_core() -> Self {
        let mut registry = Self::new();
        crate::builtin::register(&mut registry);
        registry
    }

    /// Registers a Rust command.
    pub fn add<C: Command>(&mut self, command: C) -> Result<()> {
        self.add_typed(command, false)
    }

    pub(crate) fn add_direct<C: Command>(&mut self, command: C) -> Result<()> {
        self.add_typed(command, true)
    }

    /// Registers an application-level command.
    pub fn add_host<C: HostCommand>(&mut self, command: C) -> Result<()> {
        let spec = typed_spec::<C::Params, C::Output>(C::META, false);
        let handler: HostHandler = Box::new(move |host, caller, args| {
            let host = host.downcast_mut::<C::Host>().ok_or_else(|| {
                Error::new(
                    "internal",
                    format!("`{}` was given the wrong host", C::META.name),
                )
            })?;
            let params = parse_params::<C::Params>(C::META.name, args)?;
            to_result(command.run(host, caller, params)?)
        });
        self.insert(spec, Handler::Host(handler))
    }

    /// Registers a command defined at run time by an add-in, named `<addin-id>.<verb>`.
    pub fn register_addin(
        &mut self,
        spec: CommandSpec,
        handler: impl Fn(&mut CommandContext<'_>, Value) -> Result<Value> + Send + Sync + 'static,
    ) -> Result<()> {
        let domain = spec.name.split('.').next().unwrap_or_default();
        if RESERVED_DOMAINS.contains(&domain) {
            return Err(Error::new(
                "registry.reserved_domain",
                format!("`{}` uses the reserved domain `{domain}`", spec.name),
            ));
        }
        self.insert(
            spec,
            Handler::Document {
                run: Box::new(handler),
                direct: false,
            },
        )
    }

    pub fn get(&self, name: &str) -> Option<&CommandSpec> {
        self.commands.get(name).map(|e| &e.spec)
    }

    /// Whether `name` is an application-level command, run with [`execute_host`](Self::execute_host).
    pub fn is_host(&self, name: &str) -> bool {
        self.commands
            .get(name)
            .is_some_and(|e| matches!(e.handler, Handler::Host(_)))
    }

    /// Commands in name order.
    pub fn iter(&self) -> impl Iterator<Item = &CommandSpec> {
        self.commands.values().map(|e| &e.spec)
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Runs a document command: checks permission, validates the arguments, opens a transaction for
    /// editing commands, and commits on success or rolls back on error or panic.
    pub fn execute(
        &self,
        doc: &mut Document,
        name: &str,
        args: Value,
        caller: &Caller,
    ) -> Result<Outcome> {
        self.run_document(doc, name, args, caller, true)
    }

    /// Runs a document command in a transaction that is then rolled back, and reports what it would
    /// have changed (MCP `preview`, confirm mode). The model is left untouched.
    pub fn preview(
        &self,
        doc: &mut Document,
        name: &str,
        args: Value,
        caller: &Caller,
    ) -> Result<Outcome> {
        if matches!(
            self.commands.get(name).map(|e| &e.handler),
            Some(Handler::Document { direct: true, .. })
        ) {
            return Err(Error::new(
                "preview.unsupported",
                format!("`{name}` cannot be previewed"),
            ));
        }
        self.run_document(doc, name, args, caller, false)
    }

    fn run_document(
        &self,
        doc: &mut Document,
        name: &str,
        args: Value,
        caller: &Caller,
        commit: bool,
    ) -> Result<Outcome> {
        let (entry, args) =
            self.prepare(name, args, caller, &crate::units::settings_of(doc.store()))?;
        let Handler::Document { run, direct } = &entry.handler else {
            return Err(Error::new(
                "registry.host_command",
                format!("`{name}` acts on the application, not a document"),
            ));
        };
        let state = if *direct {
            State::Direct(doc)
        } else if entry.spec.edits() {
            State::Edit(doc.transaction(entry.spec.transaction_name.clone(), caller.origin.clone()))
        } else {
            State::Read(doc)
        };
        let mut cx = CommandContext {
            registry: self,
            caller,
            state,
            direct_commit: None,
        };
        let result = guard(name, || run(&mut cx, args))?;
        // Constraints hold after every edit (ARC-CNST-002): the solver moves what the edit did
        // not, or the edit fails.
        // Hosted walls follow their hosts (ARC-WALL-016) first.
        if let State::Edit(txn) = &mut cx.state {
            crate::walls::linings::follow_hosts(txn)?;
            crate::parts::prune(txn)?;
            crate::constraints::enforce(txn)?;
        }
        if !commit {
            let (warnings, changed) = match &cx.state {
                State::Edit(txn) => (txn.warnings().to_vec(), txn.pending_changes()),
                _ => Default::default(),
            };
            // Dropping the context drops the transaction, which rolls it back.
            return Ok(Outcome {
                result,
                warnings,
                changed,
            });
        }
        let commit = match cx.state {
            State::Edit(txn) => Some(txn.commit()),
            _ => cx.direct_commit,
        };
        let (warnings, changed) = commit.map(|c| (c.warnings, c.changed)).unwrap_or_default();
        Ok(Outcome {
            result,
            warnings,
            changed,
        })
    }

    /// Runs an application-level command against `host` (the application), with the same
    /// permission and argument checks as [`execute`](Self::execute).
    pub fn execute_host(
        &self,
        host: &mut dyn Any,
        name: &str,
        args: Value,
        caller: &Caller,
    ) -> Result<Outcome> {
        let (entry, args) =
            self.prepare(name, args, caller, &crate::units::UnitSettings::default())?;
        let Handler::Host(run) = &entry.handler else {
            return Err(Error::new(
                "registry.document_command",
                format!("`{name}` acts on a document"),
            ));
        };
        let result = guard(name, || run(host, caller, args))?;
        Ok(Outcome {
            result,
            warnings: Vec::new(),
            changed: ChangedSummary::default(),
        })
    }

    fn prepare(
        &self,
        name: &str,
        args: Value,
        caller: &Caller,
        units: &crate::units::UnitSettings,
    ) -> Result<(&Entry, Value)> {
        let entry = self.commands.get(name).ok_or_else(|| {
            Error::new(
                "registry.unknown_command",
                format!("no command named `{name}`"),
            )
        })?;
        let required = entry.spec.min_permission();
        if caller.permission < required {
            return Err(Error::new(
                "permission.denied",
                format!("`{name}` needs a higher permission level"),
            )
            .with_details(json!({ "required": required })));
        }
        let mut args = if args.is_null() { json!({}) } else { args };
        crate::units::convert_arguments(&entry.spec.params, &mut args, units)?;
        let problems: Vec<String> = entry
            .validator
            .iter_errors(&args)
            .map(|e| e.to_string())
            .collect();
        if !problems.is_empty() {
            return Err(
                Error::new("args.invalid", format!("invalid arguments for `{name}`"))
                    .with_details(json!({ "errors": problems })),
            );
        }
        Ok((entry, args))
    }

    fn add_typed<C: Command>(&mut self, command: C, direct: bool) -> Result<()> {
        let spec = typed_spec::<C::Params, C::Output>(C::META, direct);
        let run: DocHandler = Box::new(move |cx, args| {
            let params = parse_params::<C::Params>(C::META.name, args)?;
            to_result(command.run(cx, params)?)
        });
        self.insert(spec, Handler::Document { run, direct })
    }

    fn insert(&mut self, spec: CommandSpec, handler: Handler) -> Result<()> {
        if !valid_name(&spec.name) {
            return Err(Error::new(
                "registry.invalid_name",
                format!("`{}` is not a dotted lower-snake-case name", spec.name),
            ));
        }
        if self.commands.contains_key(&spec.name) {
            return Err(Error::new(
                "registry.duplicate",
                format!("`{}` is already registered", spec.name),
            ));
        }
        let validator = jsonschema::validator_for(&spec.params)
            .map_err(|e| Error::new("registry.invalid_schema", format!("`{}`: {e}", spec.name)))?;
        self.commands.insert(
            spec.name.clone(),
            Entry {
                spec,
                validator,
                handler,
            },
        );
        Ok(())
    }
}

fn typed_spec<P: JsonSchema, O: JsonSchema>(m: CommandMeta, direct: bool) -> CommandSpec {
    CommandSpec {
        name: m.name.into(),
        version: m.version,
        summary: m.summary.into(),
        description: m.description.into(),
        params: schema_value(schemars::schema_for!(P)),
        result: schema_value(schemars::schema_for!(O)),
        effects: m.effects.to_vec(),
        undoable: m.effects.contains(&Effect::Edit) && !direct,
        transaction_name: m.transaction_name.into(),
    }
}

fn parse_params<P: DeserializeOwned>(name: &str, args: Value) -> Result<P> {
    serde_json::from_value(args).map_err(|e| {
        Error::new(
            "args.invalid",
            format!("invalid arguments for `{name}`: {e}"),
        )
    })
}

fn to_result<O: Serialize>(output: O) -> Result<Value> {
    serde_json::to_value(output).map_err(|e| Error::new("internal", e.to_string()))
}

thread_local! {
    /// How many commands are running on this thread inside [`guard`].
    static GUARDED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Whether a panic on this thread now would be caught and reported as a command's error (for a
/// panic hook deciding whether the application is failing).
pub fn panic_is_recovered() -> bool {
    GUARDED.with(|g| g.get() > 0)
}

/// Turns a panic inside a command into an error; for editing commands the transaction is dropped,
/// which rolls it back.
fn guard(name: &str, f: impl FnOnce() -> Result<Value>) -> Result<Value> {
    GUARDED.with(|g| g.set(g.get() + 1));
    let result = catch_unwind(AssertUnwindSafe(f));
    GUARDED.with(|g| g.set(g.get() - 1));
    result.unwrap_or_else(|panic| {
        let msg = panic
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        Err(Error::new(
            "command.panicked",
            format!("`{name}` failed: {msg}"),
        ))
    })
}

fn schema_value(schema: schemars::Schema) -> Value {
    serde_json::to_value(schema).expect("JSON Schemas always serialise")
}

fn valid_name(name: &str) -> bool {
    name.split('.').count() >= 2
        && name.split('.').all(|part| {
            part.starts_with(|c: char| c.is_ascii_lowercase())
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_dotted_lower_snake_case() {
        for good in [
            "wall.create",
            "view.set_scale",
            "family_doc.form.extrusion",
            "org.example.tools.renumber",
        ] {
            assert!(valid_name(good), "{good}");
        }
        for bad in [
            "wall",
            "Wall.create",
            "wall..create",
            "wall.2d",
            "wall.create-new",
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
