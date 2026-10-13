// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The application session: open documents, each with its project file and live store, and the
//! application-level commands (`file.*`, `app.exit`, `live.*`, `recovery.*`).
//!
//! [`App::execute`] is the single entry point for the UI, Lua, MCP and the command shell:
//! application commands act on the [`App`]; every other command acts on the active document.

mod commands;

use std::collections::BTreeMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use reviera_core::{Caller, CommandRegistry, Document, Error, Outcome, Result};
use reviera_ifc::Schema;
use reviera_livestore::{LiveError, LiveStore, Options as LiveOptions, Recoverable};
use reviera_store::LocalProject;
use schemars::JsonSchema;
use serde::Serialize;

pub use commands::SaveChoice;

pub type DocumentId = u64;

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Live-store settings; `None` turns the live store off (tests, read-only tools).
    pub live: Option<LiveOptions>,
    /// Where untitled projects are kept until Save As.
    pub untitled_dir: PathBuf,
}

impl Default for AppConfig {
    fn default() -> Self {
        let live = LiveOptions::default();
        let untitled_dir = live
            .dir
            .parent()
            .map(|p| p.join("untitled"))
            .unwrap_or_else(std::env::temp_dir);
        Self {
            live: Some(live),
            untitled_dir,
        }
    }
}

/// One open project.
#[derive(Debug)]
pub struct OpenDocument {
    pub project: LocalProject,
    pub doc: Document,
    live: Option<LiveStore>,
    pub read_only: bool,
    /// Never saved: Save needs a path (Save As).
    pub untitled: bool,
    /// What opening the file found (RXD problems are listed before editing begins).
    pub open_report: Option<reviera_store::OpenReport>,
}

impl OpenDocument {
    pub fn is_dirty(&self) -> bool {
        self.untitled || self.project.is_dirty(&self.doc)
    }

    pub fn live(&self) -> Option<&LiveStore> {
        self.live.as_ref()
    }
}

/// A row of the open-documents list (MCP resource `reviera://documents`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DocumentInfo {
    pub id: DocumentId,
    pub title: String,
    pub path: Option<PathBuf>,
    pub schema: String,
    pub read_only: bool,
    pub dirty: bool,
    pub active: bool,
}

#[derive(Debug)]
pub struct App {
    registry: Arc<CommandRegistry>,
    docs: BTreeMap<DocumentId, OpenDocument>,
    next_doc: DocumentId,
    active: Option<DocumentId>,
    config: AppConfig,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let mut registry = CommandRegistry::with_core();
        commands::register(&mut registry);
        Self {
            registry: Arc::new(registry),
            docs: BTreeMap::new(),
            next_doc: 1,
            active: None,
            config,
        }
    }

    pub fn registry(&self) -> &CommandRegistry {
        &self.registry
    }

    /// For registering add-in and plug-in commands at start-up. `None` while a command is running.
    pub fn registry_mut(&mut self) -> Option<&mut CommandRegistry> {
        Arc::get_mut(&mut self.registry)
    }

    /// Runs any command: application commands against the app, the rest against the active document.
    pub fn execute(
        &mut self,
        name: &str,
        args: serde_json::Value,
        caller: &Caller,
    ) -> Result<Outcome> {
        let registry = self.registry.clone();
        if registry.is_host(name) {
            return registry.execute_host(self, name, args, caller);
        }
        let id = self
            .active
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let open = self.docs.get_mut(&id).expect("the active document is open");
        registry.execute(&mut open.doc, name, args, caller)
    }

    pub fn active(&self) -> Option<DocumentId> {
        self.active
    }

    pub fn activate(&mut self, id: DocumentId) -> Result<()> {
        self.get(id)?;
        self.active = Some(id);
        Ok(())
    }

    pub fn document(&self, id: DocumentId) -> Option<&OpenDocument> {
        self.docs.get(&id)
    }

    pub fn documents(&self) -> Vec<DocumentInfo> {
        self.docs
            .iter()
            .map(|(id, d)| DocumentInfo {
                id: *id,
                title: title(d),
                path: (!d.untitled).then(|| d.project.path().to_owned()),
                schema: d.project.ifc().schema(),
                read_only: d.read_only,
                dirty: d.is_dirty(),
                active: self.active == Some(*id),
            })
            .collect()
    }

    /// Makes all pending recovery data durable, for example before the process exits without
    /// closing projects, so their unsaved work can be recovered at the next start.
    pub fn flush_all(&self) {
        for d in self.docs.values() {
            if let Some(live) = &d.live {
                let _ = live.flush();
            }
        }
    }

    /// Stores left by a crash, for the recovery dialog at start-up (DM-LIVE-004).
    pub fn recoverable(&self) -> Result<Vec<Recoverable>> {
        match &self.config.live {
            Some(live) => {
                reviera_livestore::scan(&live.dir).map_err(|e| err("recovery.scan_failed", e))
            }
            None => Ok(Vec::new()),
        }
    }

    fn get(&self, id: DocumentId) -> Result<&OpenDocument> {
        self.docs
            .get(&id)
            .ok_or_else(|| Error::new("document.not_found", format!("no open document {id}")))
    }

    fn get_mut(&mut self, id: DocumentId) -> Result<&mut OpenDocument> {
        self.docs
            .get_mut(&id)
            .ok_or_else(|| Error::new("document.not_found", format!("no open document {id}")))
    }

    fn resolve(&self, id: Option<DocumentId>) -> Result<DocumentId> {
        id.or(self.active)
            .ok_or_else(|| Error::new("document.none", "no document is open"))
    }

    pub(crate) fn new_document(&mut self, schema: Schema) -> Result<DocumentId> {
        let path = self
            .config
            .untitled_dir
            .join(format!("Untitled-{}.ifc", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&self.config.untitled_dir).map_err(|e| err("file.io", e))?;
        let (project, doc) =
            LocalProject::new(schema, &path).map_err(|e| err("file.new_failed", e))?;
        self.add(project, doc, false, true)
    }

    pub(crate) fn open(&mut self, path: &Path, read_only: bool) -> Result<DocumentId> {
        if let Some((id, _)) = self
            .docs
            .iter()
            .find(|(_, d)| !d.untitled && d.project.path() == path)
        {
            return Err(Error::new(
                "file.already_open",
                format!("{} is already open", path.display()),
            )
            .with_details(serde_json::json!({ "document_id": id })));
        }
        let (project, doc, report) =
            LocalProject::open(path).map_err(|e| err("file.open_failed", e))?;
        let id = self.add(project, doc, read_only, false)?;
        self.get_mut(id)?.open_report = Some(report);
        Ok(id)
    }

    /// Registers an open project and attaches its live store (DM-LIVE-001).
    pub(crate) fn add(
        &mut self,
        project: LocalProject,
        mut doc: Document,
        read_only: bool,
        untitled: bool,
    ) -> Result<DocumentId> {
        let live = match (&self.config.live, read_only) {
            (Some(options), false) => {
                Some(LiveStore::create(&project, &mut doc, options).map_err(|e| {
                    match e {
                        LiveError::Locked { .. } => err("file.locked", e).with_details(
                            serde_json::json!({ "offer": ["read_only", "open_copy"] }),
                        ),
                        LiveError::NeedsRecovery(key) => Error::new(
                            "recovery.pending",
                            format!(
                                "{} has unsaved work from a crashed session",
                                project.path().display()
                            ),
                        )
                        .with_details(serde_json::json!({ "store_key": key })),
                        other => err("live.unavailable", other),
                    }
                })?)
            }
            _ => None,
        };
        let id = self.next_doc;
        self.next_doc += 1;
        self.docs.insert(
            id,
            OpenDocument {
                project,
                doc,
                live,
                read_only,
                untitled,
                open_report: None,
            },
        );
        self.active = Some(id);
        Ok(id)
    }

    pub(crate) fn save(
        &mut self,
        id: DocumentId,
        path: Option<&Path>,
    ) -> Result<reviera_store::SaveReport> {
        let live_options = self.config.live.clone();
        let open = self.get_mut(id)?;
        let target = match path {
            Some(p) => p.to_owned(),
            None if open.untitled => {
                return Err(Error::new(
                    "file.untitled",
                    "the project has never been saved; use file.save_as",
                ));
            }
            None if open.read_only => {
                return Err(Error::new(
                    "file.read_only",
                    "the project was opened read-only; use file.save_as",
                ));
            }
            None => open.project.path().to_owned(),
        };
        let moved = target != open.project.path();
        let report = open
            .project
            .save_as(&open.doc, &target)
            .map_err(|e| err("file.save_failed", e))?;
        open.untitled = false;
        open.read_only = false;
        if moved {
            // Save As re-keys the live store to the new path (Ch 2b §7).
            if let Some(old) = open.live.take() {
                let key = old.key().to_owned();
                old.close(&mut open.doc)
                    .map_err(|e| err("live.unavailable", e))?;
                if let Some(o) = &live_options {
                    let _ = reviera_livestore::discard(&key, &o.dir);
                }
            }
            if let Some(o) = &live_options {
                open.live = Some(
                    LiveStore::create(&open.project, &mut open.doc, o)
                        .map_err(|e| err("live.unavailable", e))?,
                );
            }
        }
        if let Some(live) = &open.live {
            live.saved(&open.project, &open.doc)
                .map_err(|e| err("live.unavailable", e))?;
        }
        Ok(report)
    }

    pub(crate) fn close(&mut self, id: DocumentId, save: SaveChoice) -> Result<()> {
        let dirty = self.get(id)?.is_dirty();
        match save {
            SaveChoice::Ask if dirty => {
                return Err(Error::new(
                    "file.unsaved_changes",
                    "the project has unsaved changes; close with save \"yes\" or \"no\"",
                ));
            }
            SaveChoice::Yes if dirty => {
                self.save(id, None)?;
            }
            _ => {}
        }
        let mut open = self.docs.remove(&id).expect("checked above");
        if let Some(live) = open.live.take() {
            live.close(&mut open.doc)
                .map_err(|e| err("live.unavailable", e))?;
        }
        if self.active == Some(id) {
            self.active = self.docs.keys().next_back().copied();
        }
        Ok(())
    }
}

fn title(d: &OpenDocument) -> String {
    if d.untitled {
        return "Untitled".into();
    }
    d.project
        .path()
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Wraps a lower-level error as a command error with a stable code.
pub(crate) fn err(code: &'static str, e: impl Display) -> Error {
    Error::new(code, e.to_string())
}
