// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Application-level commands (spec Ch 1 UI-FILE-001, Ch 2 DM-FILE-001/002/006, Ch 2b §12).

use std::path::PathBuf;

use reviera_core::{Caller, CommandMeta, CommandRegistry, Effect, Error, HostCommand, Result};
use reviera_ifc::Schema;
use reviera_livestore::{LiveStatus, Recoverable, RestoreReport, TxnSummary};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{App, DocumentId, err};

pub(crate) fn register(registry: &mut CommandRegistry) {
    let results = [
        registry.add_host(FileNew),
        registry.add_host(FileOpen),
        registry.add_host(FileSave),
        registry.add_host(FileSaveAs),
        registry.add_host(FileClose),
        registry.add_host(AppExit),
        registry.add_host(LiveStatusCmd),
        registry.add_host(LiveFlush),
        registry.add_host(LiveHistory),
        registry.add_host(LiveEncryptionStatus),
        registry.add_host(RecoveryList),
        registry.add_host(RecoveryRestore),
        registry.add_host(RecoveryShowChanges),
        registry.add_host(RecoveryDiscard),
        registry.add_host(SelectionGet),
        registry.add_host(SelectionSet),
        registry.add_host(SelectionClear),
        registry.add_host(crate::views::ViewGetCamera),
        registry.add_host(crate::views::ViewOrient),
        registry.add_host(crate::views::ViewSetProjection),
        registry.add_host(crate::views::ViewSetHome),
        registry.add_host(crate::views::ViewOpen),
        registry.add_host(crate::inspect::ViewTempHide),
        registry.add_host(crate::inspect::ViewTempIsolate),
        registry.add_host(crate::inspect::ViewTempTranslucent),
        registry.add_host(crate::inspect::ViewTempShow),
        registry.add_host(crate::inspect::ViewTempReset),
        registry.add_host(crate::inspect::ViewGetTempState),
        registry.add_host(crate::inspect::ModelTree),
        registry.add_host(crate::inspect::ViewFitSectionBox),
        registry.add_host(crate::output::ExportPdf),
        registry.add_host(SelectionAdd),
        registry.add_host(SelectionRemove),
    ];
    for r in results {
        r.expect("application commands are valid");
    }
}

const fn meta(
    name: &'static str,
    summary: &'static str,
    effects: &'static [Effect],
) -> CommandMeta {
    CommandMeta {
        name,
        version: 1,
        summary,
        description: summary,
        effects,
        transaction_name: "",
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NewKind {
    #[default]
    Model,
    ModelTemplate,
    Family,
    TitleBlock,
    AnnotationSymbol,
    MassFamily,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub enum SchemaChoice {
    #[serde(rename = "IFC4")]
    Ifc4,
    #[serde(rename = "IFC4X3")]
    Ifc4x3,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewParams {
    #[serde(default)]
    pub kind: NewKind,
    pub template: Option<String>,
    pub schema: Option<SchemaChoice>,
}

struct FileNew;

impl HostCommand for FileNew {
    type Host = App;
    type Params = NewParams;
    type Output = DocumentId;
    const META: CommandMeta = meta("file.new", "Create a project", &[Effect::Ui]);

    fn run(&self, app: &mut App, _: &Caller, p: NewParams) -> Result<DocumentId> {
        if !matches!(p.kind, NewKind::Model) {
            return Err(Error::new(
                "file.unsupported_kind",
                "only model projects can be created yet",
            ));
        }
        if p.template.is_some() {
            return Err(Error::new(
                "file.unsupported_template",
                "project templates are not supported yet",
            ));
        }
        let schema = match p.schema {
            Some(SchemaChoice::Ifc4x3) => Schema::Ifc4x3Add2,
            _ => Schema::Ifc4,
        };
        app.new_document(schema)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    pub path: PathBuf,
    /// Partial open (storeys, classes, worksets); not supported yet.
    pub partial: Option<Value>,
    #[serde(default)]
    pub read_only: bool,
}

struct FileOpen;

impl HostCommand for FileOpen {
    type Host = App;
    type Params = OpenParams;
    type Output = DocumentId;
    const META: CommandMeta = meta("file.open", "Open an IFC project", &[Effect::FileRead]);

    fn run(&self, app: &mut App, _: &Caller, p: OpenParams) -> Result<DocumentId> {
        if p.partial.is_some() {
            return Err(Error::new(
                "file.partial_unsupported",
                "partial open is not supported yet",
            ));
        }
        app.open(&p.path, p.read_only)
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SaveStatus {
    pub path: PathBuf,
    pub data_revision: i64,
    pub created: usize,
    pub updated: usize,
    pub removed: usize,
}

fn save_status(app: &App, id: DocumentId, r: reviera_store::SaveReport) -> Result<SaveStatus> {
    Ok(SaveStatus {
        path: app.get(id)?.project.path().to_owned(),
        data_revision: r.data_revision,
        created: r.created,
        updated: r.updated,
        removed: r.removed,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveParams {
    pub document_id: Option<DocumentId>,
}

struct FileSave;

impl HostCommand for FileSave {
    type Host = App;
    type Params = SaveParams;
    type Output = SaveStatus;
    const META: CommandMeta = meta("file.save", "Save the project", &[Effect::FileWrite]);

    fn run(&self, app: &mut App, _: &Caller, p: SaveParams) -> Result<SaveStatus> {
        let id = app.resolve(p.document_id)?;
        let report = app.save(id, None)?;
        save_status(app, id, report)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SaveFormat {
    Ifc,
    Ifczip,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Carrier {
    Embedded,
    ExternalFile,
    ExternalDir,
    Bundle,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveAsParams {
    pub document_id: Option<DocumentId>,
    pub path: PathBuf,
    pub kind: Option<String>,
    pub format: Option<SaveFormat>,
    pub carrier: Option<Carrier>,
}

struct FileSaveAs;

impl HostCommand for FileSaveAs {
    type Host = App;
    type Params = SaveAsParams;
    type Output = SaveStatus;
    const META: CommandMeta = meta(
        "file.save_as",
        "Save the project to another file",
        &[Effect::FileWrite],
    );

    fn run(&self, app: &mut App, _: &Caller, p: SaveAsParams) -> Result<SaveStatus> {
        if p.kind.as_deref().is_some_and(|k| k != "model") {
            return Err(Error::new(
                "file.unsupported_kind",
                "only model projects can be saved yet",
            ));
        }
        if matches!(p.format, Some(SaveFormat::Ifczip)) {
            return Err(Error::new(
                "file.unsupported_format",
                ".ifczip is not supported yet",
            ));
        }
        if p.carrier
            .as_ref()
            .is_some_and(|c| !matches!(c, Carrier::Embedded))
        {
            return Err(Error::new(
                "file.unsupported_carrier",
                "only the embedded RXD carrier is supported yet",
            ));
        }
        let id = app.resolve(p.document_id)?;
        let report = app.save(id, Some(&p.path))?;
        save_status(app, id, report)
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SaveChoice {
    /// Refuse to close a project with unsaved changes (the UI asks first).
    #[default]
    Ask,
    Yes,
    No,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseParams {
    pub document_id: DocumentId,
    #[serde(default)]
    pub save: SaveChoice,
}

struct FileClose;

impl HostCommand for FileClose {
    type Host = App;
    type Params = CloseParams;
    type Output = ();
    const META: CommandMeta = meta("file.close", "Close a project", &[Effect::Ui]);

    fn run(&self, app: &mut App, _: &Caller, p: CloseParams) -> Result<()> {
        app.close(p.document_id, p.save)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExitParams {
    #[serde(default)]
    pub force: bool,
}

struct AppExit;

impl HostCommand for AppExit {
    type Host = App;
    type Params = ExitParams;
    type Output = ();
    const META: CommandMeta = meta(
        "app.exit",
        "Close every project",
        &[Effect::Ui, Effect::Destructive],
    );

    fn run(&self, app: &mut App, _: &Caller, p: ExitParams) -> Result<()> {
        let dirty: Vec<_> = app
            .documents()
            .into_iter()
            .filter(|d| d.dirty)
            .map(|d| d.id)
            .collect();
        if !dirty.is_empty() && !p.force {
            return Err(
                Error::new("app.unsaved_changes", "some projects have unsaved changes")
                    .with_details(serde_json::json!({ "document_ids": dirty })),
            );
        }
        let ids: Vec<_> = app.docs.keys().copied().collect();
        for id in ids {
            app.close(id, SaveChoice::No)?;
        }
        Ok(())
    }
}

fn active_live(app: &App) -> Result<&reviera_livestore::LiveStore> {
    let id = app.resolve(None)?;
    app.get(id)?
        .live()
        .ok_or_else(|| Error::new("live.unavailable", "this project has no live store"))
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

struct LiveStatusCmd;

impl HostCommand for LiveStatusCmd {
    type Host = App;
    type Params = NoParams;
    type Output = LiveStatus;
    const META: CommandMeta = meta(
        "live.status",
        "Live store status of the active project",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, _: NoParams) -> Result<LiveStatus> {
        Ok(active_live(app)?.status())
    }
}

struct LiveFlush;

impl HostCommand for LiveFlush {
    type Host = App;
    type Params = NoParams;
    type Output = i64;
    const META: CommandMeta = meta(
        "live.flush",
        "Write pending recovery data now",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, _: NoParams) -> Result<i64> {
        active_live(app)?
            .flush()
            .map_err(|e| err("live.unavailable", e))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryParams {
    pub since_seq: Option<i64>,
    /// A source (`ui`, `lua`, `addin`, `mcp`, `sync`) or a full label such as `mcp:agent`.
    pub origin: Option<String>,
}

struct LiveHistory;

impl HostCommand for LiveHistory {
    type Host = App;
    type Params = HistoryParams;
    type Output = Vec<TxnSummary>;
    const META: CommandMeta = meta(
        "live.history",
        "Journal entries of the active project",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, p: HistoryParams) -> Result<Vec<TxnSummary>> {
        active_live(app)?
            .history(p.since_seq, p.origin.as_deref())
            .map_err(|e| err("live.unavailable", e))
    }
}

struct LiveEncryptionStatus;

impl HostCommand for LiveEncryptionStatus {
    type Host = App;
    type Params = NoParams;
    type Output = reviera_livestore::EncryptionStatus;
    const META: CommandMeta = meta(
        "live.encryption.status",
        "Whether the active project's recovery data is encrypted, and with which key",
        &[Effect::Read],
    );

    fn run(
        &self,
        app: &mut App,
        _: &Caller,
        _: NoParams,
    ) -> Result<reviera_livestore::EncryptionStatus> {
        Ok(active_live(app)?.encryption_status())
    }
}

struct RecoveryList;

impl HostCommand for RecoveryList {
    type Host = App;
    type Params = NoParams;
    type Output = Vec<Recoverable>;
    const META: CommandMeta = meta(
        "recovery.list",
        "Projects with unsaved work from a crash",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, _: NoParams) -> Result<Vec<Recoverable>> {
        app.recoverable()
    }
}

fn live_options(app: &App) -> Result<reviera_livestore::Options> {
    app.config
        .live
        .clone()
        .ok_or_else(|| Error::new("live.unavailable", "the live store is turned off"))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreParams {
    pub store_key: String,
    pub as_copy_path: Option<PathBuf>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RestoreResult {
    pub model_id: DocumentId,
    pub report: RestoreReport,
}

struct RecoveryRestore;

impl HostCommand for RecoveryRestore {
    type Host = App;
    type Params = RestoreParams;
    type Output = RestoreResult;
    const META: CommandMeta = meta(
        "recovery.restore",
        "Restore a project from its live store",
        &[Effect::FileRead, Effect::FileWrite],
    );

    fn run(&self, app: &mut App, _: &Caller, p: RestoreParams) -> Result<RestoreResult> {
        let options = live_options(app)?;
        let restored =
            reviera_livestore::restore(&p.store_key, p.as_copy_path.as_deref(), &options)
                .map_err(|e| err("recovery.failed", e))?;
        let untitled = !restored.project.path().exists() && p.as_copy_path.is_none();
        let id = app.next_doc;
        app.next_doc += 1;
        let view_states = crate::views::load(&restored.project);
        app.docs.insert(
            id,
            crate::OpenDocument {
                project: restored.project,
                doc: restored.doc,
                live: Some(restored.store),
                read_only: false,
                untitled,
                open_report: None,
                selection: Default::default(),
                view_states,
                active_view: None,
                temp: Default::default(),
                spatial: Default::default(),
                tessellation: Default::default(),
            },
        );
        app.active = Some(id);
        Ok(RestoreResult {
            model_id: id,
            report: restored.report,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreKeyParams {
    pub store_key: String,
}

struct RecoveryShowChanges;

impl HostCommand for RecoveryShowChanges {
    type Host = App;
    type Params = StoreKeyParams;
    type Output = Vec<TxnSummary>;
    const META: CommandMeta = meta(
        "recovery.show_changes",
        "Unsaved transactions in a crashed project's store",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, p: StoreKeyParams) -> Result<Vec<TxnSummary>> {
        let options = live_options(app)?;
        reviera_livestore::show_changes(&p.store_key, &options)
            .map_err(|e| err("recovery.unreadable", e))
    }
}

struct RecoveryDiscard;

impl HostCommand for RecoveryDiscard {
    type Host = App;
    type Params = StoreKeyParams;
    type Output = ();
    const META: CommandMeta = meta(
        "recovery.discard",
        "Move a crashed project's store to the recovery bin",
        &[Effect::Destructive, Effect::FileWrite],
    );

    fn run(&self, app: &mut App, _: &Caller, p: StoreKeyParams) -> Result<()> {
        let options = live_options(app)?;
        if !app.recoverable()?.iter().any(|r| r.key == p.store_key) {
            return Err(Error::new(
                "recovery.not_found",
                format!("no recoverable store {}", p.store_key),
            ));
        }
        reviera_livestore::discard(&p.store_key, &options.dir).map_err(|e| err("file.io", e))
    }
}

// ---- Selection (Ch 1 UI-SEL-002, Ch 3 ARC-SEL-001): session state of the active document ----

use reviera_core::{ElementId, ElementRef};

fn resolve_all(app: &App, refs: &[ElementRef]) -> Result<Vec<ElementId>> {
    let open = app.get(app.resolve(None)?)?;
    refs.iter()
        .map(|r| {
            open.doc.store().resolve(r).ok_or_else(|| {
                Error::new("element.not_found", "no such element")
                    .with_details(serde_json::json!({ "ref": r }))
            })
        })
        .collect()
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectionInfo {
    pub ids: Vec<ElementId>,
    /// Selected elements per category key (`uncategorised` when none applies).
    pub by_category: std::collections::BTreeMap<String, usize>,
}

fn selection_info(app: &App) -> Result<SelectionInfo> {
    let open = app.get(app.resolve(None)?)?;
    let mut by_category = std::collections::BTreeMap::new();
    for id in &open.selection {
        let key = open
            .doc
            .store()
            .get(*id)
            .and_then(reviera_core::categories::category_of)
            .map_or("uncategorised", |c| c.key);
        *by_category.entry(key.to_owned()).or_insert(0) += 1;
    }
    Ok(SelectionInfo {
        ids: open.selection.iter().copied().collect(),
        by_category,
    })
}

struct SelectionGet;

impl HostCommand for SelectionGet {
    type Host = App;
    type Params = NoParams;
    type Output = SelectionInfo;
    const META: CommandMeta = meta(
        "selection.get",
        "The selected elements of the active document",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, _: NoParams) -> Result<SelectionInfo> {
        selection_info(app)
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SelectMode {
    #[default]
    Replace,
    Add,
    Remove,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionSetParams {
    /// Internal IDs or GlobalIds.
    pub ids: Vec<ElementRef>,
    #[serde(default)]
    pub mode: SelectMode,
}

struct SelectionSet;

impl HostCommand for SelectionSet {
    type Host = App;
    type Params = SelectionSetParams;
    type Output = SelectionInfo;
    const META: CommandMeta = meta(
        "selection.set",
        "Set, extend or reduce the selection",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: SelectionSetParams) -> Result<SelectionInfo> {
        let ids = resolve_all(app, &p.ids)?;
        let open = app.active_mut()?;
        match p.mode {
            SelectMode::Replace => open.selection = ids.into_iter().collect(),
            SelectMode::Add => open.selection.extend(ids),
            SelectMode::Remove => ids.iter().for_each(|i| {
                open.selection.remove(i);
            }),
        }
        selection_info(app)
    }
}

struct SelectionClear;

impl HostCommand for SelectionClear {
    type Host = App;
    type Params = NoParams;
    type Output = ();
    const META: CommandMeta = meta("selection.clear", "Clear the selection", &[Effect::Ui]);

    fn run(&self, app: &mut App, _: &Caller, _: NoParams) -> Result<()> {
        app.active_mut()?.selection.clear();
        Ok(())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionIdsParams {
    pub element_ids: Vec<ElementRef>,
}

struct SelectionAdd;

impl HostCommand for SelectionAdd {
    type Host = App;
    type Params = SelectionIdsParams;
    type Output = ();
    const META: CommandMeta = meta(
        "selection.add",
        "Add elements to the selection",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: SelectionIdsParams) -> Result<()> {
        let ids = resolve_all(app, &p.element_ids)?;
        app.active_mut()?.selection.extend(ids);
        Ok(())
    }
}

struct SelectionRemove;

impl HostCommand for SelectionRemove {
    type Host = App;
    type Params = SelectionIdsParams;
    type Output = ();
    const META: CommandMeta = meta(
        "selection.remove",
        "Remove elements from the selection",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: SelectionIdsParams) -> Result<()> {
        let ids = resolve_all(app, &p.element_ids)?;
        let open = app.active_mut()?;
        for id in ids {
            open.selection.remove(&id);
        }
        Ok(())
    }
}
