// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The application session: open documents, each with its project file and live store, and the
//! application-level commands (`file.*`, `app.exit`, `live.*`, `recovery.*`).
//!
//! [`App::execute`] is the single entry point for the UI, Lua, MCP and the command shell:
//! application commands act on the [`App`]; every other command acts on the active document.

pub mod bcf_files;
pub mod bcf_topics;
pub mod clash;
pub mod cobie;
mod steel_nc;
mod connection_check;
mod commands;
pub mod compute;
pub mod csg;
pub mod inspect;
pub mod notes;
pub mod output;
pub mod overlaps;
pub mod schedules;
pub mod scripts;
pub mod sheets;
pub mod paramlib;
pub mod classification_files;
pub mod standards_transfer;
pub mod system_zones;
pub mod weather;
pub mod carbon_files;
pub mod solar;
pub mod hc_loads;
pub mod annual_energy;
pub mod daylight;
pub mod gbxml_files;
pub mod connectors;
pub mod site_connectors;
pub mod settings_dialogs;
pub mod structural;
pub mod subscriptions;
pub mod views;
pub mod view_image;

use std::collections::BTreeMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use reviera_core::{Caller, CommandRegistry, Document, Error, Outcome, Result, walls};
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
    /// The current selection (session state, not saved or undone).
    pub selection: std::collections::BTreeSet<reviera_core::ElementId>,
    /// The latest camera and Home of each view (session state, saved with the next save).
    pub view_states: views::ViewStates,
    /// The view the UI shows as active.
    pub active_view: Option<reviera_core::ElementId>,
    /// Temporary hide, isolate and translucency of each view (session state, not saved).
    pub temp: std::collections::HashMap<Option<reviera_core::ElementId>, inspect::TempState>,
    /// The IFC file's parent relationships and the data revision they were read at.
    spatial: std::sync::Mutex<Option<(i64, Arc<reviera_core::inspect::Imported>)>>,
    /// Tessellated IFC geometry and the data revision it was made at.
    tessellation: std::sync::Mutex<Option<(i64, std::sync::Arc<Vec<reviera_render::SceneMesh>>)>>,
    /// Boolean results (ARC-MOD-028).
    pub(crate) csg: csg::Cache,
}

impl OpenDocument {
    /// The IFC geometry of the file as last opened or saved, tessellated once per save.
    /// Meshes of IFC objects that are not elements of the document are left out.
    fn tessellation(&self) -> Result<std::sync::Arc<Vec<reviera_render::SceneMesh>>> {
        let revision = self.project.data_revision();
        let mut cache = self.tessellation.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((r, meshes)) = &*cache
            && *r == revision
        {
            return Ok(meshes.clone());
        }
        let meshes = Arc::new(scene_meshes(
            tessellate(&self.project, &self.doc)?,
            &self.doc,
        ));
        *cache = Some((revision, meshes.clone()));
        Ok(meshes)
    }

    /// The IFC geometry of the products whose GlobalIds are given (spaces included), as last
    /// saved; tessellated on every call, so for rare work such as converting legacy zones.
    pub(crate) fn product_meshes(&self, guids: &std::collections::HashSet<String>) -> Result<Vec<reviera_ifc::Mesh>> {
        Ok(tessellate(&self.project, &self.doc)?
            .into_iter()
            .filter(|m| m.global_id.as_ref().is_some_and(|g| guids.contains(g.as_str())))
            .collect())
    }

    pub fn is_dirty(&self) -> bool {
        self.untitled || self.project.is_dirty(&self.doc)
    }
}

/// Tessellates a project's IFC geometry with OpenCASCADE, on every core: the elements Reviera
/// does not regenerate itself (their bodies come from their recipes instead).
fn tessellate(project: &LocalProject, doc: &Document) -> Result<Vec<reviera_ifc::Mesh>> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    let skip: Vec<reviera_ifc::StepId> = doc
        .store()
        .iter()
        .filter(|e| regenerated(e))
        .filter_map(|e| project.step_of(e.id()))
        .collect();
    project
        .ifc()
        .tessellate_skipping(threads, &skip)
        .map_err(|e| err("render.tessellation_failed", e))
}

/// Scene meshes of tessellated IFC geometry, mapped to the document's elements. Spaces and
/// openings are left out of the default 3D view.
fn scene_meshes(meshes: Vec<reviera_ifc::Mesh>, doc: &Document) -> Vec<reviera_render::SceneMesh> {
    let ids: std::collections::HashMap<_, _> = doc
        .store()
        .iter()
        .map(|e| (e.guid().clone(), e.id()))
        .collect();
    meshes
        .into_iter()
        .filter(|m| m.class != "IfcSpace" && m.class != "IfcOpeningElement" && !m.faces.is_empty())
        .filter_map(|m| {
            // What is not an element is made from one (a curtain wall's panels and mullions, a
            // shaft's openings) and drawn from its element.
            let element = Some(*ids.get(m.global_id.as_ref()?)?);
            Some(reviera_render::SceneMesh {
                element,
                color: reviera_render::class_color(&m.class),
                positions: m
                    .verts
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|[x, y, z]| [*x as f32, *y as f32, *z as f32])
                    .collect(),
                indices: m.faces,
                class: m.class,
            })
        })
        .collect()
}

/// A model read off the GUI thread: parsed, and its geometry tessellated (the slow parts of
/// opening). [`App::open_loaded`] then attaches it on the application thread.
pub struct Loaded {
    pub path: PathBuf,
    project: LocalProject,
    doc: Document,
    report: reviera_store::OpenReport,
    meshes: Vec<reviera_render::SceneMesh>,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded")
            .field("path", &self.path)
            .field("meshes", &self.meshes.len())
            .finish()
    }
}

/// The stage a background load has reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadStage {
    Reading = 0,
    Geometry = 1,
}

/// Reads and tessellates a model; safe to run on any thread. `stage` is told when each stage
/// starts.
pub fn load(path: &Path, stage: &dyn Fn(LoadStage)) -> Result<Loaded> {
    stage(LoadStage::Reading);
    let (project, doc, report) =
        LocalProject::open(path).map_err(|e| err("file.open_failed", e))?;
    stage(LoadStage::Geometry);
    let meshes = scene_meshes(tessellate(&project, &doc)?, &doc);
    Ok(Loaded {
        path: path.to_owned(),
        project,
        doc,
        report,
        meshes,
    })
}

impl OpenDocument {
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
    /// Views that commands asked the UI to open.
    view_requests: Vec<(DocumentId, reviera_core::ElementId)>,
    /// Settings dialogs that commands asked the UI to open (`mep_settings.show`).
    settings_requests: Vec<settings_dialogs::SettingsKind>,
    /// The parameter libraries open in this session (DM-SPAR-001), by id.
    libraries: BTreeMap<String, paramlib::OpenLibrary>,
    paramlib_requested: bool,
    /// Shows scripts' forms as dialogs (set by the window).
    form_handler: Option<scripts::Handler>,
    /// The remote computation client, created on first use (`compute.*`).
    compute: Option<reviera_compute::ComputeClient>,
    /// Remote analysis subscriptions: settings, secrets and connections (`subscription.*`).
    subscriptions: subscriptions::Subscriptions,
    /// A window is there to capture views (DM-MCP-004).
    window: bool,
    /// View captures asked of the window.
    image_requests: Vec<view_image::ImageRequest>,
}

/// Whether an element's body is regenerated from its Reviera recipe (`bodies`), rather than
/// tessellated from the IFC file: every element with a recipe (walls, slabs, roofs, stairs,
/// railings, columns, curtain walls, shafts, dormers …), and the parts and openings of inserts.
pub(crate) fn regenerated(e: &reviera_core::Element) -> bool {
    e.param(walls::RECIPE_KEY).is_some()
        || walls::wall_recipe(e).is_some()
        || reviera_core::floors::slab_recipe(e).is_some()
        || reviera_core::roofs::roof_recipe(e).is_some()
        || reviera_core::stairs::stair_recipe(e).is_some()
        || reviera_core::railings::railing_recipe(e).is_some()
        || reviera_core::railings::gate_of(e).is_some()
        || reviera_core::inserts::kind_of(e).is_some()
        || reviera_core::inserts::opening_of(e).is_some()
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let mut registry = CommandRegistry::with_core();
        commands::register(&mut registry);
        reviera_store::commands::register(&mut registry);
        Self {
            registry: Arc::new(registry),
            docs: BTreeMap::new(),
            next_doc: 1,
            active: None,
            config,
            view_requests: Vec::new(),
            settings_requests: Vec::new(),
            libraries: BTreeMap::new(),
            paramlib_requested: false,
            form_handler: None,
            compute: None,
            subscriptions: subscriptions::Subscriptions::default(),
            window: false,
            image_requests: Vec::new(),
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

    /// Runs a document command and rolls it back, reporting what it would change.
    pub fn preview(
        &mut self,
        name: &str,
        args: serde_json::Value,
        caller: &Caller,
    ) -> Result<Outcome> {
        if self.registry.is_host(name) {
            return Err(Error::new(
                "preview.unsupported",
                format!("`{name}` acts on the application and cannot be previewed"),
            ));
        }
        let registry = self.registry.clone();
        let open = self.active_mut()?;
        registry.preview(&mut open.doc, name, args, caller)
    }

    /// Starts a transaction group on the active document, so one undo reverts several calls.
    pub fn group_begin(&mut self, name: &str, origin: reviera_core::Origin) -> Result<()> {
        self.active_mut()?.doc.group_begin(name, origin)
    }

    pub fn group_end(
        &mut self,
        mode: reviera_core::GroupEnd,
    ) -> Result<reviera_core::ChangedSummary> {
        Ok(self.active_mut()?.doc.group_end(mode)?.changed)
    }

    pub(crate) fn active_mut(&mut self) -> Result<&mut OpenDocument> {
        let id = self
            .active
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        self.get_mut(id)
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

    /// The Render Scene of a document, coloured by class. Elements built from a Reviera recipe
    /// (walls) are regenerated from the current model, so edits show at once. Other elements come
    /// from the IFC population, tessellated with OpenCASCADE and cached until the next save;
    /// deleted elements disappear at once. Spaces and openings are left out of the default 3D view.
    pub fn render_scene(&self, id: DocumentId) -> Result<reviera_render::RenderScene> {
        let open = self.get(id)?;
        let store = open.doc.store();
        // Elements in booleans are drawn from their (shaped) bodies.
        let involved = csg::involved(store);
        let mut meshes: Vec<reviera_render::SceneMesh> = open
            .tessellation()?
            .iter()
            .filter(|m| match m.element {
                Some(e) => store.get(e).is_some_and(|e| !regenerated(e)) && !involved.contains(&e),
                None => true,
            })
            .cloned()
            .collect();
        // Reviera's own elements, regenerated (`bodies`): walls joined and cut by their openings,
        // floors, ceilings and decks, roofs, railings, gates, stairs, doors and windows. One that
        // cannot regenerate is not drawn.
        for body in self.bodies_of(id, |e| involved.contains(&e))? {
            for part in body.parts {
                meshes.push(reviera_render::SceneMesh {
                    element: Some(body.element),
                    color: reviera_render::class_color(&part.class),
                    class: if part.plan_hidden {
                        format!("{}{}", part.class, reviera_render::PLAN_HIDDEN)
                    } else {
                        part.class
                    },
                    positions: part
                        .mesh
                        .0
                        .iter()
                        .map(|[x, y, z]| [*x as f32, *y as f32, *z as f32])
                        .collect(),
                    indices: part.mesh.1,
                });
            }
        }
        // Virtual volumes: door swings and clearances, drawn by each view's mode (hidden by
        // default; translucent or solid), never cut in plans (ARC-MOD-031).
        for c in reviera_core::clearances::all(store) {
            let color = match c.kind {
                reviera_core::clearances::ClearanceKind::DoorSwing => [1.0, 0.62, 0.08, 0.3],
                _ => [0.1, 0.62, 1.0, 0.22],
            };
            meshes.push(reviera_render::SceneMesh {
                element: Some(c.element),
                color,
                class: c.kind.class().to_owned(),
                positions: c
                    .mesh
                    .0
                    .iter()
                    .map(|[x, y, z]| [*x as f32, *y as f32, *z as f32])
                    .collect(),
                indices: c.mesh.1,
            });
        }
        Ok(reviera_render::RenderScene { meshes })
    }

    /// Stores left by a crash, for the recovery dialog at start-up (DM-LIVE-004).
    pub fn recoverable(&self) -> Result<Vec<Recoverable>> {
        match &self.config.live {
            Some(live) => reviera_livestore::scan(live).map_err(|e| err("recovery.scan_failed", e)),
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

    /// Attaches a model read by [`load`], like `file.open` (live store, recovery and lock checks).
    pub fn open_loaded(&mut self, loaded: Loaded, read_only: bool) -> Result<DocumentId> {
        self.check_not_open(&loaded.path)?;
        let revision = loaded.project.data_revision();
        let id = self.add(loaded.project, loaded.doc, read_only, false)?;
        let open = self.get_mut(id)?;
        open.open_report = Some(loaded.report);
        *open.tessellation.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((revision, Arc::new(loaded.meshes)));
        Ok(id)
    }

    /// The document already open from `path`, if any.
    pub fn document_at(&self, path: &Path) -> Option<DocumentId> {
        self.docs
            .iter()
            .find(|(_, d)| !d.untitled && d.project.path() == path)
            .map(|(id, _)| *id)
    }

    fn check_not_open(&self, path: &Path) -> Result<()> {
        match self.document_at(path) {
            Some(id) => Err(Error::new(
                "file.already_open",
                format!("{} is already open", path.display()),
            )
            .with_details(serde_json::json!({ "document_id": id }))),
            None => Ok(()),
        }
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
        let view_states = views::load(&project);
        self.docs.insert(
            id,
            OpenDocument {
                project,
                doc,
                live,
                read_only,
                untitled,
                open_report: None,
                selection: Default::default(),
                view_states,
                active_view: None,
                temp: Default::default(),
                spatial: Default::default(),
                tessellation: Default::default(),
                csg: Default::default(),
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
        self.stage_view_state(id)?;
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
