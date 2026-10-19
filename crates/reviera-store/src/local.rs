// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Local mode: one `.ifc` file with embedded RXD (TECH_STACK §3.7, Ch 2a §3).
//!
//! The IFC instance population stays the model of record. Every `IfcObjectDefinition` (objects,
//! types and the project) becomes an element of the core [`Document`]; everything else is kept in
//! the IFC population untouched and written back as it was (DM-CORE-001 rule 8). Saving applies the
//! document's changes since the last save, refreshes element identity data, and replaces the file
//! atomically (Ch 2a §9).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use reviera_core::grids as core_grids;
use reviera_core::{
    Document, ElementId, GlobalId, ModelVersion, NewElement, floors, inserts, levels, railings,
    roofs, rooms, stairs, walls,
};
use reviera_ifc::{AttrValue, IfcModel, Schema, StepId};
use reviera_rxd::{
    DOC_PSET_PREFIX, DecodeLimits, ELEMENT_PSET, ElementData, IdentityDoc, MANIFEST_PSET,
    TYPE_PSET, canonical_json, encode, manifest, prop,
};
use serde_json::{Value, json};

use crate::psets::{self, Pset, PsetWriter, identifier, integer, label, text, unsupported};
use crate::{Result, StoreError};
use crate::{geometry, grids, params};

/// What opening a file found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenReport {
    pub elements: usize,
    /// Elements whose internal ID came from their `Reviera_Element` data.
    pub ids_from_rxd: usize,
    /// Elements given a new internal ID (no data, or a repeated ID).
    pub ids_assigned: usize,
    /// Data that could not be used, by property set or element.
    pub problems: Vec<String>,
    /// Changes taken from the IFC data because another tool edited it (grids moved, renamed,
    /// added or removed); written back with the next save.
    pub reconciled: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveReport {
    pub created: usize,
    pub updated: usize,
    pub removed: usize,
    pub element_data_written: usize,
    pub data_revision: i64,
}

/// A project stored in a local `.ifc` file.
pub struct LocalProject {
    ifc: IfcModel,
    path: PathBuf,
    steps: HashMap<ElementId, StepId>,
    types: HashSet<StepId>,
    element_psets: HashMap<ElementId, Pset>,
    element_data: HashMap<ElementId, ElementData>,
    doc_psets: BTreeMap<String, Pset>,
    manifest: Option<Pset>,
    project: Option<StepId>,
    owner_history: Option<StepId>,
    identity: IdentityDoc,
    data_id: Option<String>,
    data_revision: i64,
    /// Elements whose identity data has not been written yet.
    pending_identity: HashSet<ElementId>,
    /// Elements that differ from the IFC population although the document version says otherwise
    /// (after recovery, Ch 2b §6.3).
    forced_changed: BTreeSet<ElementId>,
    forced_deleted: BTreeSet<ElementId>,
    /// Parameter values as they are in the IFC population, by element.
    saved_params: HashMap<ElementId, BTreeMap<String, serde_json::Value>>,
    /// Property and quantity sets by owner.
    sets: params::SetIndex,
    /// RXD-only elements and their document paths.
    rxd_paths: HashMap<ElementId, String>,
    /// Document version that the IFC population reflects.
    synced: ModelVersion,
    /// Metres per length unit of the file.
    length_scale: f64,
    /// Project-level RXD documents to write with the next save (view state and the like), by
    /// path. Setting them does not make the project dirty.
    pending_docs: BTreeMap<String, Value>,
}

impl std::fmt::Debug for LocalProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalProject")
            .field("path", &self.path)
            .field("elements", &self.steps.len())
            .field("data_revision", &self.data_revision)
            .finish()
    }
}

impl LocalProject {
    /// A new project from the built-in template, to be saved at `path`: SI units, a 3D model
    /// context, and Site › Building › "Level 1" (DM-FILE-006).
    pub fn new(schema: Schema, path: &Path) -> Result<(Self, Document)> {
        if schema == Schema::Ifc2x3 {
            return Err(unsupported(
                "new projects are IFC4 or IFC4X3_ADD2; IFC2X3 is read-only",
            ));
        }
        let (project, doc, _) = Self::from_model(template(schema)?, path)?;
        Ok((project, doc))
    }

    pub fn open(path: &Path) -> Result<(Self, Document, OpenReport)> {
        Self::from_model(IfcModel::open(path)?, path)
    }

    fn from_model(ifc: IfcModel, path: &Path) -> Result<(Self, Document, OpenReport)> {
        let mut report = OpenReport::default();
        let limits = DecodeLimits::default();

        let mut objects = ifc.entities_of_type("IfcObjectDefinition")?;
        objects.sort_by_key(|e| e.id);
        // The openings a shaft derives for each host it cuts are written from the shaft; they
        // are not elements of their own.
        // So are a curtain wall's panels and mullions, and a path array's members (of any class
        // a component may have; only elements carry the mark).
        objects.retain(|e| !is_derived(&ifc, e.id));
        let length_scale = geometry::length_scale(&ifc)?;
        let (mut values, sets) = params::load(&ifc, &objects, length_scale)?;
        let types: HashSet<StepId> = ifc
            .entities_of_type("IfcTypeObject")?
            .into_iter()
            .map(|e| e.id)
            .collect();
        let project = ifc.entities_of_type("IfcProject")?.first().map(|e| e.id);
        let owner_history = ifc
            .entities_of_type("IfcOwnerHistory")?
            .first()
            .map(|e| e.id);

        let mut manifest = None;
        let mut doc_psets = BTreeMap::new();
        let mut data_by_step: HashMap<StepId, (ElementData, Pset)> = HashMap::new();
        for pset in psets::reviera_psets(&ifc)? {
            if pset.name == MANIFEST_PSET {
                manifest = Some(pset);
            } else if let Some(doc_path) = pset.name.strip_prefix(DOC_PSET_PREFIX) {
                doc_psets.insert(doc_path.to_owned(), pset);
            } else if pset.name == ELEMENT_PSET || pset.name == TYPE_PSET {
                match pset
                    .decode(limits)
                    .and_then(|v| Ok(ElementData::from_value(v)?))
                {
                    Ok(data) => {
                        for (target, _) in &pset.targets {
                            data_by_step.insert(*target, (data.clone(), pset.clone()));
                        }
                    }
                    Err(e) => report
                        .problems
                        .push(format!("{} {}: {e}", pset.name, pset.id)),
                }
            }
        }
        let identity = match doc_psets.get(IdentityDoc::PATH) {
            Some(p) => match p
                .decode(limits)
                .and_then(|v| serde_json::from_value(v).map_err(|e| unsupported(e.to_string())))
            {
                Ok(doc) => doc,
                Err(e) => {
                    report.problems.push(format!("{}: {e}", IdentityDoc::PATH));
                    IdentityDoc::default()
                }
            },
            None => IdentityDoc::default(),
        };

        // Elements that exist only in RXD, one document each (class `rxd:<path>`).
        let mut used = HashSet::new();
        let mut rxd_elements = Vec::new();
        let mut rxd_paths = HashMap::new();
        for (path, pset) in &doc_psets {
            if path == IdentityDoc::PATH {
                continue;
            }
            let Ok(payload) = pset.decode(limits) else {
                report.problems.push(format!(
                    "RXD document {path} could not be decoded; it is kept unchanged"
                ));
                continue;
            };
            let Some(record) = RxdRecord::from_value(&payload) else {
                continue;
            };
            if !used.insert(record.iid) {
                report.problems.push(format!(
                    "RXD document {path}: internal ID {} is repeated; it is kept unchanged",
                    record.iid
                ));
                continue;
            }
            rxd_paths.insert(ElementId(record.iid), path.clone());
            rxd_elements.push((ElementId(record.iid), record.into_element(path)));
        }

        // Internal IDs: persisted ones first; the rest in STEP order (DM-ID-001).
        let mut ids: HashMap<StepId, ElementId> = HashMap::new();
        let mut needs_id = Vec::new();
        for e in &objects {
            let iid = data_by_step
                .get(&e.id)
                .and_then(|(d, _)| d.identity().ok().flatten())
                .map(|i| i.iid);
            match iid {
                Some(i) if i != 0 && used.insert(i) => {
                    ids.insert(e.id, ElementId(i));
                    report.ids_from_rxd += 1;
                }
                Some(i) => {
                    report.problems.push(format!(
                        "{} {}: internal ID {i} is repeated; a new one was assigned",
                        e.class, e.id
                    ));
                    needs_id.push(e.id);
                }
                None => needs_id.push(e.id),
            }
        }
        let mut next = identity
            .next_iid
            .max(ElementId::FIRST.0)
            .max(used.iter().max().map_or(0, |m| m + 1));
        let mut pending_identity = HashSet::new();
        for step in needs_id {
            let is_project = Some(step) == project;
            let id = if is_project && used.insert(ElementId::PROJECT.0) {
                ElementId::PROJECT
            } else {
                next += 1;
                ElementId(next - 1)
            };
            ids.insert(step, id);
            pending_identity.insert(id);
            report.ids_assigned += 1;
        }

        let mut steps = HashMap::new();
        let mut element_psets = HashMap::new();
        let mut element_data = HashMap::new();
        let mut seen_guids = HashSet::new();
        let mut elements = Vec::new();
        let mut saved_params = HashMap::new();
        for e in objects {
            let id = ids[&e.id];
            let Some(guid) = e.global_id.filter(|g| seen_guids.insert(g.clone())) else {
                report.problems.push(format!(
                    "{} {}: missing or repeated GlobalId; kept in the file only",
                    e.class, e.id
                ));
                pending_identity.remove(&id);
                continue;
            };
            let mut element_params = values.remove(&e.id).unwrap_or_default();
            if let Some((data, pset)) = data_by_step.remove(&e.id) {
                // RXD sections other than identity are element parameters (`rxd:<section>`).
                for (name, value) in data.sections() {
                    if name != ElementData::IDENTITY {
                        element_params
                            .insert(format!("{}{name}", params::RXD_PREFIX), value.clone());
                    }
                }
                element_data.insert(id, data);
                element_psets.insert(id, pset);
            }
            steps.insert(id, e.id);
            saved_params.insert(id, element_params.clone());
            elements.push((
                id,
                NewElement {
                    class: e.class,
                    name: e.name,
                    params: element_params,
                    guid: Some(guid),
                },
            ));
        }
        report.elements = elements.len();
        elements.extend(rxd_elements);
        // Grids edited by other tools: the IFC axes win (DM-IFC-006).
        let reconciled = grids::reconcile(&ifc, &mut elements, &steps, &mut next, length_scale)?;
        report.reconciled = reconciled.notes;
        let doc = Document::from_loaded(elements, next)?;

        let data_id = manifest
            .as_ref()
            .and_then(|m| m.string(manifest::DATA_ID))
            .map(str::to_owned);
        let data_revision = manifest
            .as_ref()
            .and_then(|m| m.int(manifest::DATA_REVISION))
            .unwrap_or(0);
        let local = Self {
            ifc,
            path: path.to_owned(),
            steps,
            types,
            element_psets,
            element_data,
            doc_psets,
            manifest,
            project,
            owner_history,
            identity,
            data_id,
            data_revision,
            pending_identity,
            forced_changed: reconciled.changed,
            forced_deleted: reconciled.deleted,
            synced: doc.version(),
            saved_params,
            sets,
            rxd_paths,
            length_scale,
            pending_docs: BTreeMap::new(),
        };
        Ok((local, doc, report))
    }

    /// Rebuilds a project from `base`, an IFC file holding the state of the last open or save, and
    /// `current`, the document recovered from the live store (Ch 2b §6.3). The project is bound to
    /// `path`, and every element that differs from `base` is written on the next save.
    pub fn restore(base: &Path, path: &Path, current: &Document) -> Result<(Self, OpenReport)> {
        let (mut project, saved, report) = Self::open(base)?;
        project.path = path.to_owned();
        for e in current.store().iter() {
            if saved.store().get(e.id()) != Some(e) {
                project.forced_changed.insert(e.id());
            }
        }
        for e in saved.store().iter() {
            if !current.store().contains(e.id()) {
                project.forced_deleted.insert(e.id());
            }
        }
        project.synced = current.version();
        Ok((project, report))
    }

    /// Whether saving would write anything.
    pub fn is_dirty(&self, doc: &Document) -> bool {
        let since = doc.changed_since(self.synced);
        since.stale_episode
            || !since.changed.is_empty()
            || !since.deleted.is_empty()
            || !self.pending_identity.is_empty()
            || !self.forced_changed.is_empty()
            || !self.forced_deleted.is_empty()
    }

    /// The `DataId` of the RXD set, once the project has been saved with Reviera.
    pub fn data_id(&self) -> Option<&str> {
        self.data_id.as_deref()
    }

    /// Binds the project to another file; the next save writes there.
    pub fn set_path(&mut self, path: &Path) {
        self.path = path.to_owned();
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn ifc(&self) -> &IfcModel {
        &self.ifc
    }

    pub fn step_of(&self, id: ElementId) -> Option<StepId> {
        self.steps.get(&id).copied()
    }

    pub fn element_data(&self, id: ElementId) -> Option<&ElementData> {
        self.element_data.get(&id)
    }

    /// The payload chunks of an element's `Reviera_Element` (or `Reviera_Type`) property set.
    pub fn element_payload(&self, id: ElementId) -> Option<Vec<String>> {
        self.element_psets.get(&id).map(Pset::chunks)
    }

    /// A project-level RXD document that is not an element (for example `views/state`): the
    /// version to be saved if one is pending, else the one in the file.
    pub fn document(&self, path: &str) -> Option<Value> {
        if let Some(v) = self.pending_docs.get(path) {
            return Some(v.clone());
        }
        self.doc_psets
            .get(path)?
            .decode(DecodeLimits::default())
            .ok()
    }

    /// Sets a project-level RXD document, written with the next save. It does not make the
    /// project dirty: saving for another reason writes it (UI-NAV-001 view state).
    pub fn set_document(&mut self, path: &str, value: Value) {
        self.pending_docs.insert(path.to_owned(), value);
    }

    /// The parent of each IFC object in the file as last opened or saved (spatial containment,
    /// aggregation, voids and fills), by GlobalId (UI-TREE-001).
    pub fn spatial_parents(&self) -> Result<HashMap<GlobalId, GlobalId>> {
        crate::spatial::parents(&self.ifc)
    }

    pub fn data_revision(&self) -> i64 {
        self.data_revision
    }

    pub fn save(&mut self, doc: &Document) -> Result<SaveReport> {
        let path = self.path.clone();
        self.save_as(doc, &path)
    }

    pub fn save_as(&mut self, doc: &Document, path: &Path) -> Result<SaveReport> {
        let mut since = doc.changed_since(self.synced);
        if since.stale_episode {
            return Err(unsupported("the document was not opened from this project"));
        }
        let changed: BTreeSet<_> = since
            .changed
            .drain(..)
            .chain(self.forced_changed.iter().copied())
            .collect();
        let deleted: BTreeSet<_> = since
            .deleted
            .drain(..)
            .chain(self.forced_deleted.iter().copied())
            .collect();
        let project = self
            .project
            .ok_or_else(|| unsupported("the file has no IfcProject"))?;
        let mut report = SaveReport::default();
        let mut dirty: BTreeSet<ElementId> = self.pending_identity.iter().copied().collect();
        // Grid lines are RXD records; their IFC axes belong to the grid system's `IfcGrid`.
        let grid_path = core_grids::CLASS_PREFIX
            .strip_prefix("rxd:")
            .expect("an RXD class");
        let grids_touched = deleted.iter().any(|id| {
            self.rxd_paths
                .get(id)
                .is_some_and(|p| p.starts_with(grid_path))
        }) || changed.iter().any(|id| {
            doc.store()
                .get(*id)
                .is_some_and(|e| core_grids::is_grid(e) || core_grids::is_system(e))
        });

        for id in &deleted {
            dirty.remove(id);
            if let Some(path) = self.rxd_paths.remove(id) {
                if let Some(pset) = self.doc_psets.remove(&path) {
                    self.remove_element_pset(&pset, project)?;
                }
                report.removed += 1;
                continue;
            }
            self.element_data.remove(id);
            let Some(step) = self.steps.remove(id) else {
                continue;
            };
            if let Some(pset) = self.element_psets.remove(id) {
                self.remove_element_pset(&pset, step)?;
            }
            let saved = self.saved_params.remove(id).unwrap_or_default();
            if saved
                .get(walls::RECIPE_KEY)
                .and_then(|r| r.get("kind"))
                .is_some_and(|k| k == walls::WALL_RECIPE)
            {
                geometry::remove_wall(&mut self.ifc, step)?;
            }
            if saved.contains_key(core_grids::SYSTEM_KEY) {
                geometry::remove_grid_axes(&mut self.ifc, step)?;
            }
            let recipe_kind = saved
                .get(walls::RECIPE_KEY)
                .and_then(|r| r.get("kind"))
                .and_then(|k| k.as_str());
            if matches!(
                recipe_kind,
                Some(inserts::INSERT_RECIPE | inserts::OPENING_RECIPE)
            ) {
                geometry::remove_boxes(&mut self.ifc, step)?;
            }
            if recipe_kind == Some(reviera_core::wall_openings::WALL_OPENING_RECIPE) {
                for rel in self.ifc.referenced_by(step, "IfcRelVoidsElement")? {
                    self.ifc.remove(rel)?;
                }
                geometry::remove_boxes(&mut self.ifc, step)?;
            }
            if recipe_kind == Some(reviera_core::curtain::CURTAIN_RECIPE) {
                crate::curtains::remove_parts(&mut self.ifc, step)?;
            }
            if recipe_kind == Some(reviera_core::slab_openings::SHAFT_RECIPE) {
                geometry::remove_slab(&mut self.ifc, step)?;
            }
            if recipe_kind == Some(reviera_core::slab_openings::OPENING_RECIPE) {
                for rel in self.ifc.referenced_by(step, "IfcRelVoidsElement")? {
                    self.ifc.remove(rel)?;
                }
                geometry::remove_slab(&mut self.ifc, step)?;
                // An opening by face's prisms.
                geometry::remove_breps(&mut self.ifc, step)?;
            }
            if matches!(
                recipe_kind,
                Some(floors::FLOOR_RECIPE | floors::CEILING_RECIPE)
            ) {
                geometry::remove_slab(&mut self.ifc, step)?;
                // A deck's body is its boards and framing.
                geometry::remove_breps(&mut self.ifc, step)?;
            }
            if recipe_kind == Some(roofs::ROOF_RECIPE)
                || recipe_kind == Some(reviera_core::columns::COLUMN_RECIPE)
                || recipe_kind == Some(reviera_core::beams::BEAM_RECIPE)
                || recipe_kind == Some(reviera_core::foundations::FOOTING_RECIPE)
                || recipe_kind == Some(reviera_core::foundations::WALL_FOUNDATION_RECIPE)
                || recipe_kind == Some(reviera_core::ramps::RAMP_RECIPE)
                || recipe_kind == Some(reviera_core::skylights::SKYLIGHT_RECIPE)
                || recipe_kind == Some(stairs::STAIR_RECIPE)
                || recipe_kind == Some(railings::RAILING_RECIPE)
                || recipe_kind == Some(railings::GATE_RECIPE)
                || recipe_kind == Some(reviera_core::slab_edges::RECIPE)
                || recipe_kind == Some(reviera_core::wall_sweeps::RECIPE)
                || recipe_kind == Some(reviera_core::model_text::RECIPE)
                || recipe_kind == Some(reviera_core::in_place::RECIPE)
                || recipe_kind == Some(reviera_core::in_place::VOID_RECIPE)
                || recipe_kind == Some(reviera_core::masses::RECIPE)
                || recipe_kind == Some(reviera_core::masses::SYSTEM_RECIPE)
                || recipe_kind == Some(reviera_core::toposolids::RECIPE)
                || recipe_kind == Some(reviera_core::toposolids::CUT_RECIPE)
                || recipe_kind == Some(reviera_core::parts::RECIPE)
            {
                geometry::remove_breps(&mut self.ifc, step)?;
            }
            if recipe_kind == Some(rooms::ROOM_RECIPE) {
                geometry::remove_slab(&mut self.ifc, step)?;
            }
            if recipe_kind == Some(reviera_core::mesh_objects::RECIPE) {
                crate::mesh_objects::remove_copy_geometry(&mut self.ifc, step)?;
            }
            if matches!(
                recipe_kind,
                Some(
                    reviera_core::ref_planes::RECIPE
                        | reviera_core::model_lines::RECIPE
                        | reviera_core::scope_boxes::RECIPE
                )
            ) {
                crate::curves::remove(&mut self.ifc, step)?;
            }
            if recipe_kind == Some(reviera_core::path_arrays::RECIPE) {
                crate::path_arrays::remove_members(&mut self.ifc, step)?;
                crate::mesh_objects::remove_copy_geometry(&mut self.ifc, step)?;
            }
            if recipe_kind == Some(reviera_core::mesh_objects::TYPE_RECIPE) {
                crate::mesh_objects::remove_type_geometry(&mut self.ifc, step)?;
            }
            // The element's own property and quantity sets go with it.
            params::remove_all(&mut self.ifc, &mut self.sets, step)?;
            self.types.remove(&step);
            self.ifc.remove(step)?;
            report.removed += 1;
        }

        // New IFC entities first, so relationships between new elements (a wall and its new type)
        // are written whatever their order.
        let mut ifc_changed = Vec::new();
        for id in &changed {
            let e = doc.store().get(*id).expect("changed elements exist");
            if let Some(path) = e.class.strip_prefix("rxd:") {
                let value = RxdRecord::of(e).to_value();
                let existing = self.doc_psets.get(path).cloned();
                let name = format!("{DOC_PSET_PREFIX}{path}");
                let pset =
                    self.upsert_payload(existing, &name, project, false, "element", &value)?;
                if self.rxd_paths.insert(*id, path.to_owned()).is_some() {
                    report.updated += 1;
                } else {
                    report.created += 1;
                }
                self.doc_psets.insert(path.to_owned(), pset);
                continue;
            }
            let step = match self.steps.get(id) {
                Some(step) => {
                    let class = self.ifc.class_of(*step)?;
                    if !class.eq_ignore_ascii_case(&e.class) {
                        return Err(unsupported(format!(
                            "element {id}: changing {class} to {} is not supported yet",
                            e.class
                        )));
                    }
                    report.updated += 1;
                    *step
                }
                None => {
                    if !e.class.starts_with("Ifc") {
                        return Err(unsupported(format!(
                            "element {id} ({}) is neither an IFC entity nor an `rxd:` document",
                            e.class
                        )));
                    }
                    let step = self.ifc.create_rooted(&e.class, e.guid())?;
                    if let Some(oh) = self.new_owner_history()? {
                        self.ifc
                            .set_attribute(step, "OwnerHistory", &AttrValue::Ref(oh.0))?;
                    }
                    if self
                        .ifc
                        .entities_of_type("IfcTypeObject")?
                        .iter()
                        .any(|t| t.id == step)
                    {
                        self.types.insert(step);
                    }
                    self.steps.insert(*id, step);
                    report.created += 1;
                    step
                }
            };
            ifc_changed.push((*id, step));
        }
        for (id, step) in ifc_changed {
            let id = &id;
            let e = doc.store().get(*id).expect("changed elements exist");
            let name = e.name.clone().map_or(AttrValue::Null, AttrValue::String);
            if self.ifc.attribute(step, "Name")? != name {
                self.ifc.set_attribute(step, "Name", &name)?;
            }
            let before = self.saved_params.get(id).cloned().unwrap_or_default();
            if before != e.params {
                let owner_history = self.new_owner_history()?;
                let is_type = self.types.contains(&step);
                params::ValueWriter {
                    ifc: &mut self.ifc,
                    index: &mut self.sets,
                    owner_history,
                    length_scale: self.length_scale,
                }
                .apply(step, is_type, &before, &e.params)?;
                self.saved_params.insert(*id, e.params.clone());
            }
            // Attributes IFC4 requires of door and window types.
            if let Some(t) = inserts::insert_type(e) {
                let (attr, value) = match t.kind() {
                    // A door type's operation (a single swing's hand is the occurrence's).
                    Some(inserts::Kind::Door) => (
                        "OperationType",
                        match t.operation {
                            inserts::Operation::SingleSwing => "NOTDEFINED",
                            op => op.ifc(true),
                        },
                    ),
                    _ => ("PartitioningType", "SINGLE_PANEL"),
                };
                let current = self.ifc.attribute(step, attr)?;
                let wanted = AttrValue::Enum(value.into());
                if current == AttrValue::Null || (attr == "OperationType" && current != wanted) {
                    self.ifc.set_attribute(step, attr, &wanted)?;
                }
            }
            if core_grids::is_grid_type(e) {
                let applies = AttrValue::String(core_grids::SYSTEM_CLASS.into());
                if self.ifc.attribute(step, "ApplicableOccurrence")? != applies {
                    self.ifc
                        .set_attribute(step, "ApplicableOccurrence", &applies)?;
                }
            }
            if e.class == levels::CLASS {
                let owner_history = self.new_owner_history()?;
                let elevation = levels::elevation(e) / self.length_scale;
                geometry::write_storey(&mut self.ifc, step, elevation, owner_history)?;
            }
            // `rxd:<section>` parameters are sections of the element's RXD data.
            let data = self.element_data.entry(*id).or_default();
            let stale: Vec<String> = data
                .sections()
                .map(|(name, _)| name.clone())
                .filter(|name| {
                    name != ElementData::IDENTITY
                        && !e
                            .params
                            .contains_key(&format!("{}{name}", params::RXD_PREFIX))
                })
                .collect();
            for name in stale {
                data.set_section(&name, None);
            }
            for (key, value) in &e.params {
                if let Some(name) = key.strip_prefix(params::RXD_PREFIX) {
                    data.set_section(name, Some(value.clone()));
                }
            }
            dirty.insert(*id);
        }
        let touched: BTreeSet<ElementId> = changed.union(&deleted).copied().collect();
        self.write_regenerated(doc, &touched)?;
        if !touched.is_empty() {
            self.write_inserts(doc)?;
            self.write_wall_openings(doc)?;
            let owner_history = self.new_owner_history()?;
            crate::materials::write_layer_sets(
                &mut self.ifc,
                doc,
                &self.steps,
                owner_history,
                self.length_scale,
            )?;
            self.write_floors(doc)?;
            self.write_slab_openings(doc)?;
            self.write_roofs(doc)?;
            self.write_stairs(doc)?;
            self.write_columns(doc)?;
            self.write_beams(doc)?;
            self.write_beam_systems(doc)?;
            self.write_analytical(doc)?;
            self.write_loads(doc)?;
            self.write_foundations(doc)?;
            self.write_fabrication(doc)?;
            self.write_rebar(doc)?;
            self.write_ramps(doc)?;
            self.write_slab_edges(doc)?;
            self.write_wall_sweeps(doc)?;
            self.write_model_texts(doc)?;
            self.write_parts(doc)?;
            self.write_skylights(doc)?;
            let owner_history = self.new_owner_history()?;
            crate::mesh_objects::write_mesh_objects(
                &mut self.ifc,
                doc,
                &self.steps,
                owner_history,
                self.length_scale,
            )?;
            crate::path_arrays::write_path_arrays(
                &mut self.ifc,
                doc,
                &self.steps,
                owner_history,
                self.length_scale,
            )?;
            crate::groups::write_groups(&mut self.ifc, doc, &self.steps, owner_history)?;
            let owner_history = self.new_owner_history()?;
            crate::curtains::write_curtains(
                &mut self.ifc,
                doc,
                &self.steps,
                owner_history,
                self.length_scale,
            )?;
            self.write_railings(doc)?;
            self.write_rooms(doc)?;
            let owner_history = self.new_owner_history()?;
            crate::curves::write_all(
                &mut self.ifc,
                doc,
                &self.steps,
                self.length_scale,
                owner_history,
            )?;
        }
        if grids_touched {
            self.write_grids(doc)?;
        }
        // Booleans last: they reshape bodies the writers above made.
        if !touched.is_empty() {
            self.write_csg(doc)?;
        }

        for id in dirty {
            let Some(&step) = self.steps.get(&id) else {
                continue;
            };
            let data = self.element_data.entry(id).or_default();
            let mut identity = data.identity()?.unwrap_or_default();
            identity.iid = id.0;
            if let Some(stamp) = doc.stamp(id) {
                identity.last_modified_txn =
                    Some(format!("{}:{}", doc.version().episode, stamp.counter));
                identity.last_modified_by = Some(stamp.by.clone());
                identity.last_modified_at = Some(millis(stamp.at));
            }
            data.set_identity(&identity);
            let (value, kind) = (data.to_value(), data.kind());
            let is_type = self.types.contains(&step);
            let name = if is_type { TYPE_PSET } else { ELEMENT_PSET };
            let existing = self.element_psets.get(&id).cloned();
            let pset = self.upsert_payload(existing, name, step, is_type, &kind, &value)?;
            self.element_psets.insert(id, pset);
            report.element_data_written += 1;
        }

        for (path, value) in std::mem::take(&mut self.pending_docs) {
            let existing = self.doc_psets.get(&path).cloned();
            let name = format!("{DOC_PSET_PREFIX}{path}");
            let kind = path.replace('/', ".");
            let pset = self.upsert_payload(existing, &name, project, false, &kind, &value)?;
            self.doc_psets.insert(path, pset);
        }

        let identity = IdentityDoc {
            next_iid: doc.next_id().0,
            ..self.identity.clone()
        };
        let value = serde_json::to_value(&identity).expect("identity serialises");
        let existing = self.doc_psets.get(IdentityDoc::PATH).cloned();
        let name = format!("{DOC_PSET_PREFIX}{}", IdentityDoc::PATH);
        let pset = self.upsert_payload(existing, &name, project, false, "identity", &value)?;
        self.doc_psets.insert(IdentityDoc::PATH.to_owned(), pset);
        self.identity = identity;

        let revision = self.data_revision + 1;
        self.write_manifest(project, revision)?;
        self.write_atomic(path)?;

        self.data_revision = revision;
        self.path = path.to_owned();
        self.pending_identity.clear();
        self.forced_changed.clear();
        self.forced_deleted.clear();
        self.synced = doc.version();
        report.data_revision = revision;
        Ok(report)
    }

    /// Writes the IFC geometry of Reviera walls that changed, or whose type or levels changed.
    fn write_regenerated(&mut self, doc: &Document, changed: &BTreeSet<ElementId>) -> Result<()> {
        let store = doc.store();
        // Joins tie walls to their neighbours, so all walls are solved together. Values that did
        // not change are not rewritten, so unaffected walls keep identical bytes.
        if changed.is_empty() || !store.iter().any(|e| walls::wall_recipe(e).is_some()) {
            return Ok(());
        }
        let solved = walls::solve(store);
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        let mut wall_steps = HashSet::new();
        let layer_cuts = reviera_core::inserts::cuts_by_host(store);
        for (id, body) in &solved.bodies {
            let e = store.get(*id).expect("solved walls exist");
            let recipe = walls::wall_recipe(e).expect("solved walls have recipes");
            let storey = GlobalId::parse(&recipe.base_level)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|b| self.steps.get(&b).copied());
            let (Some(&step), Some(storey)) = (self.steps.get(id), storey) else {
                continue;
            };
            // A stacked wall (ARC-STKW-001): no body of its own, its sub-walls aggregated.
            if walls::stacked::of_wall(store, e).is_some() {
                let cuts = reviera_core::inserts::cuts_by_host(store);
                let c = cuts.get(id).map_or(&[][..], Vec::as_slice);
                let subs = walls::stacked::meshes(store, e, body, c);
                geometry::remove_wall_shape(&mut self.ifc, step)?;
                geometry::remove_breps(&mut self.ifc, step)?;
                crate::stacked_walls::write_subs(
                    &mut self.ifc,
                    doc,
                    &self.steps,
                    owner_history,
                    self.length_scale,
                    *id,
                    &subs,
                )?;
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
                wall_steps.insert(step);
                continue;
            }
            crate::stacked_walls::remove_subs(&mut self.ifc, step)?;
            // A wall built layer by layer (ARC-CMPD-003/004, ARC-WRAP-001): its cells as B-reps
            // in its own frame, cut by its openings (the wrap pockets are part of the body).
            if let Some(cells) = walls::layered::meshes(
                store,
                e,
                body,
                &solved,
                layer_cuts.get(id).map_or(&[][..], Vec::as_slice),
            ) {
                let g = &body.geometry;
                let (u, n) = (g.direction(), g.normal());
                let local = |p: &[f64; 3]| {
                    let d = [p[0] - g.start[0], p[1] - g.start[1]];
                    [
                        d[0] * u[0] + d[1] * u[1],
                        d[0] * n[0] + d[1] * n[1],
                        p[2] - g.base_z,
                    ]
                };
                let solids: Vec<geometry::Polyhedron> = cells
                    .iter()
                    .map(|(_, mesh)| geometry::Polyhedron {
                        points: mesh.0.iter().map(local).collect(),
                        faces: mesh
                            .1
                            .chunks(3)
                            .map(|t| t.iter().map(|i| *i as usize).collect())
                            .collect(),
                    })
                    .collect();
                geometry::remove_wall_shape(&mut self.ifc, step)?;
                geometry::write_breps(
                    &mut self.ifc,
                    step,
                    geometry::BrepFrame {
                        origin: [g.start[0], g.start[1], g.base_z],
                        x_dir: u,
                    },
                    &solids,
                    self.length_scale,
                    contexts,
                )?;
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
                wall_steps.insert(step);
                continue;
            }
            // A slanted or tapered wall (ARC-WALL-005) or an edited profile (ARC-WALL-009): its
            // body as a B-rep in its own frame.
            if let Some(Ok(mesh)) = walls::profile::special_mesh(e, body, &[]) {
                let g = &body.geometry;
                let (u, n) = (g.direction(), g.normal());
                let local = |p: &[f64; 3]| {
                    let d = [p[0] - g.start[0], p[1] - g.start[1]];
                    [
                        d[0] * u[0] + d[1] * u[1],
                        d[0] * n[0] + d[1] * n[1],
                        p[2] - g.base_z,
                    ]
                };
                let solid = geometry::Polyhedron {
                    points: mesh.0.iter().map(local).collect(),
                    faces: mesh
                        .1
                        .chunks(3)
                        .map(|t| t.iter().map(|i| *i as usize).collect())
                        .collect(),
                };
                geometry::remove_wall_shape(&mut self.ifc, step)?;
                geometry::write_breps(
                    &mut self.ifc,
                    step,
                    geometry::BrepFrame {
                        origin: [g.start[0], g.start[1], g.base_z],
                        x_dir: u,
                    },
                    &[solid],
                    self.length_scale,
                    contexts,
                )?;
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
                wall_steps.insert(step);
                continue;
            }
            // A wall attached to a roof gets a B-rep body in its own frame (where its openings are
            // placed); others keep their extruded body.
            match walls::attach::attached_body(store, *id, body) {
                Some(Ok(mut a)) => {
                    // A dormer's walls stop at the main roof's underside.
                    if let Some((n, d)) = reviera_core::dormers::wall_base_plane(store, e) {
                        a.solid = reviera_core::roofs::eaves::clip_plane(&a.solid, n, d);
                    }
                    let g = &body.geometry;
                    let (u, n) = (g.direction(), g.normal());
                    let local = |p: &[f64; 3]| {
                        let d = [p[0] - g.start[0], p[1] - g.start[1]];
                        [
                            d[0] * u[0] + d[1] * u[1],
                            d[0] * n[0] + d[1] * n[1],
                            p[2] - g.base_z,
                        ]
                    };
                    let solid = geometry::Polyhedron {
                        points: a.solid.0.iter().map(local).collect(),
                        faces: a
                            .solid
                            .1
                            .chunks(3)
                            .map(|t| t.iter().map(|i| *i as usize).collect())
                            .collect(),
                    };
                    geometry::remove_wall_shape(&mut self.ifc, step)?;
                    geometry::write_breps(
                        &mut self.ifc,
                        step,
                        geometry::BrepFrame {
                            origin: [g.start[0], g.start[1], g.base_z],
                            x_dir: u,
                        },
                        &[solid],
                        self.length_scale,
                        contexts,
                    )?;
                }
                _ => {
                    geometry::remove_breps(&mut self.ifc, step)?;
                    geometry::write_wall(&mut self.ifc, step, body, self.length_scale, contexts)?;
                }
            }
            geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            wall_steps.insert(step);
        }
        // Curved walls (ARC-WALL-002): a faceted B-rep of the curve's pieces, in a frame at the
        // start along the chord (where its openings are placed).
        for e in store.iter() {
            let Some(r) = walls::wall_recipe(e) else {
                continue;
            };
            let (Some(path), Ok(g)) = (walls::curved::path_of(&r), walls::regenerate(store, e))
            else {
                continue;
            };
            let storey = GlobalId::parse(&r.base_level)
                .and_then(|l| store.find_by_guid(&l))
                .and_then(|b| self.steps.get(&b).copied());
            let (Some(&step), Some(storey)) = (self.steps.get(&e.id()), storey) else {
                continue;
            };
            let (u, n) = (g.direction(), g.normal());
            let local = |p: &[f64; 3]| {
                let d = [p[0] - g.start[0], p[1] - g.start[1]];
                [
                    d[0] * u[0] + d[1] * u[1],
                    d[0] * n[0] + d[1] * n[1],
                    p[2] - g.base_z,
                ]
            };
            let solids: Vec<geometry::Polyhedron> = walls::curved::body(
                &path,
                g.width,
                g.base_z,
                g.base_z + g.height,
                &[],
                walls::attach::top_target(store, e.id())
                    .map(|(_, r)| r)
                    .as_ref(),
                walls::curved::ends_of(&solved, e.id()),
            )
            .iter()
            .map(|(pts, idx)| geometry::Polyhedron {
                points: pts.iter().map(local).collect(),
                faces: idx
                    .chunks(3)
                    .map(|t| t.iter().map(|i| *i as usize).collect())
                    .collect(),
            })
            .collect();
            geometry::remove_wall_shape(&mut self.ifc, step)?;
            geometry::remove_breps(&mut self.ifc, step)?;
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame {
                    origin: [g.start[0], g.start[1], g.base_z],
                    x_dir: u,
                },
                &solids,
                self.length_scale,
                contexts,
            )?;
            geometry::contain(&mut self.ifc, step, storey, owner_history)?;
        }
        let joins: Vec<_> = solved
            .joins
            .iter()
            .filter_map(|j| Some((*self.steps.get(&j.wall)?, *j, *self.steps.get(&j.other)?)))
            .collect();
        geometry::write_connections(&mut self.ifc, &wall_steps, &joins, owner_history)
    }

    /// Writes every Reviera door and window (DM-IFC-007): its opening, placed relative to the
    /// host wall and voiding it, and the insert, placed relative to its opening and filling it,
    /// with its overall size and its frame, panel and glass as box solids. The wall's own body
    /// stays uncut. Values that did not change are not rewritten.
    fn write_inserts(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let placed = inserts::placed(store);
        if placed.is_empty() {
            return Ok(());
        }
        let openings: HashMap<String, ElementId> = store
            .iter()
            .filter_map(|e| Some((inserts::opening_of(e)?, e.id())))
            .collect();
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        let scale = self.length_scale;
        let ifc4 = self.ifc.schema() != "IFC2X3";
        for p in placed {
            let guid = store.get(p.id).expect("placed").guid().to_string();
            let (Some(&step), Some(&wall), Some(&opening)) = (
                self.steps.get(&p.id),
                self.steps.get(&p.host),
                openings.get(&guid).and_then(|o| self.steps.get(o)),
            ) else {
                continue;
            };
            let placement_of = |ifc: &IfcModel, id: StepId| -> Result<Option<StepId>> {
                Ok(match ifc.attribute(id, "ObjectPlacement")? {
                    AttrValue::Ref(r) => Some(StepId(r)),
                    _ => None,
                })
            };
            let (half_w, half_t) = (p.ty.width / 2.0, p.wall.width / 2.0 + 0.001);
            // In the wall's frame (its start, along its chord): on a curved wall, the insert's
            // point and tangent on the curve.
            let (origin, x_dir) = match &p.path {
                Some(path) => {
                    let (c, t) = path.at(p.recipe.along);
                    let (u, n, s) = (p.wall.direction(), p.wall.normal(), p.wall.start);
                    let d = [c[0] - s[0], c[1] - s[1]];
                    (
                        [d[0] * u[0] + d[1] * u[1], d[0] * n[0] + d[1] * n[1]],
                        [t[0] * u[0] + t[1] * u[1], t[0] * n[0] + t[1] * n[1]],
                    )
                }
                None => ([p.recipe.along, 0.0], [1.0, 0.0]),
            };
            let frame = geometry::Frame {
                relative_to: placement_of(&self.ifc, wall)?,
                origin: [
                    origin[0] / scale,
                    origin[1] / scale,
                    (p.z0 - p.wall.base_z) / scale,
                ],
                x_dir,
            };
            // A shaped one (ARC-WIN-002): its outline extruded through the wall (a round one's
            // a circle), and its frame, glass and panel as profiles; else boxes.
            let outline = p.outline();
            if p.ty.shaped() {
                let profile = if p.ty.shape == inserts::Shape::Round {
                    geometry::Profile::Circle {
                        centre: [0.0, p.ty.height / 2.0],
                        radius: p.ty.width.min(p.ty.height) / 2.0,
                    }
                } else {
                    geometry::Profile::Rings {
                        outer: &outline,
                        holes: Vec::new(),
                    }
                };
                let void = geometry::ProfileSolid {
                    profile,
                    y: [-half_t, half_t],
                    plan: false,
                };
                geometry::write_profiles(&mut self.ifc, opening, &frame, &[void], scale, contexts)?;
            } else {
                let void = [[-half_w, half_w], [-half_t, half_t], [0.0, p.ty.height]];
                geometry::write_boxes(&mut self.ifc, opening, &frame, &[void], scale, contexts)?;
            }
            let frame = geometry::Frame {
                relative_to: placement_of(&self.ifc, opening)?,
                origin: [0.0; 3],
                x_dir: [1.0, 0.0],
            };
            let solids = p.solids();
            if solids.is_empty() {
                let boxes: Vec<geometry::LocalBox> =
                    p.boxes().into_iter().map(|(b, _)| b).collect();
                geometry::write_boxes(&mut self.ifc, step, &frame, &boxes, scale, contexts)?;
            } else {
                let profiles: Vec<geometry::ProfileSolid<'_>> = solids
                    .iter()
                    .map(|s| geometry::ProfileSolid {
                        profile: geometry::Profile::Rings {
                            outer: &s.outer,
                            holes: s.holes.iter().map(Vec::as_slice).collect(),
                        },
                        y: s.y,
                        plan: s.plan,
                    })
                    .collect();
                geometry::write_profiles(&mut self.ifc, step, &frame, &profiles, scale, contexts)?;
            }
            geometry::relate_one(
                &mut self.ifc,
                "IfcRelVoidsElement",
                "RelatingBuildingElement",
                wall,
                "RelatedOpeningElement",
                opening,
                owner_history,
            )?;
            geometry::relate_one(
                &mut self.ifc,
                "IfcRelFillsElement",
                "RelatingOpeningElement",
                opening,
                "RelatedBuildingElement",
                step,
                owner_history,
            )?;
            let storey = walls::wall_recipe(store.get(p.host).expect("host"))
                .and_then(|r| GlobalId::parse(&r.base_level))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
            let mut attrs = vec![
                ("OverallWidth", AttrValue::Real(p.ty.width / scale)),
                ("OverallHeight", AttrValue::Real(p.ty.height / scale)),
            ];
            // A shaped window says so (its shape's values are in the type's RXD recipe).
            if ifc4 && p.kind == inserts::Kind::Window {
                let (kind, user) = if p.ty.shaped() {
                    let mut name = p.ty.shape.label().to_owned();
                    if p.ty.shape == inserts::Shape::RoundTop {
                        name = format!("{name} ({})", p.ty.head_profile.label());
                    }
                    ("USERDEFINED", AttrValue::String(name))
                } else {
                    ("SINGLE_PANEL", AttrValue::Null)
                };
                attrs.push(("PartitioningType", AttrValue::Enum(kind.into())));
                attrs.push(("UserDefinedPartitioningType", user));
            }
            if ifc4 && p.kind == inserts::Kind::Door {
                // Hinge on the left seen from the swing side; double, revolving and sliding by
                // their operation (ARC-DOOR-002).
                let left = p.recipe.flip_hand == p.recipe.flip_facing;
                let op = p.ty.operation.ifc(left);
                attrs.push(("OperationType", AttrValue::Enum(op.into())));
            }
            for (attr, v) in attrs {
                if self.ifc.attribute(step, attr)? != v {
                    self.ifc.set_attribute(step, attr, &v)?;
                }
            }
        }
        Ok(())
    }

    /// Writes every wall opening (ARC-OPEN-003): a box through the wall (plus 1 mm each side) in
    /// the wall's frame, voiding the wall, and contained in the wall's base level.
    fn write_wall_openings(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = reviera_core::wall_openings::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        let scale = self.length_scale;
        for o in all {
            let (Some(&step), Some(&wall)) = (self.steps.get(&o.id), self.steps.get(&o.host))
            else {
                continue;
            };
            let relative_to = match self.ifc.attribute(wall, "ObjectPlacement")? {
                AttrValue::Ref(r) => Some(StepId(r)),
                _ => None,
            };
            let c = o.cut();
            if c.z[1] <= c.z[0] {
                continue;
            }
            let mid = (o.recipe.u[0] + o.recipe.u[1]) / 2.0;
            // In the wall's frame (its start, along its chord): on a curved wall, the opening's
            // middle and tangent on the curve.
            let (origin, x_dir) = match &o.path {
                Some(_) => {
                    let (p, t) = o.at(mid);
                    let (u, n, s) = (o.wall.direction(), o.wall.normal(), o.wall.start);
                    let d = [p[0] - s[0], p[1] - s[1]];
                    (
                        [d[0] * u[0] + d[1] * u[1], d[0] * n[0] + d[1] * n[1]],
                        [t[0] * u[0] + t[1] * u[1], t[0] * n[0] + t[1] * n[1]],
                    )
                }
                None => ([mid, 0.0], [1.0, 0.0]),
            };
            let frame = geometry::Frame {
                relative_to,
                origin: [
                    origin[0] / scale,
                    origin[1] / scale,
                    (c.z[0] - o.wall.base_z) / scale,
                ],
                x_dir,
            };
            let (half_w, half_t) = (o.width() / 2.0, o.wall.width / 2.0 + 0.001);
            let void = [[-half_w, half_w], [-half_t, half_t], [0.0, c.z[1] - c.z[0]]];
            geometry::write_boxes(&mut self.ifc, step, &frame, &[void], scale, contexts)?;
            geometry::relate_one(
                &mut self.ifc,
                "IfcRelVoidsElement",
                "RelatingBuildingElement",
                wall,
                "RelatedOpeningElement",
                step,
                owner_history,
            )?;
        }
        Ok(())
    }

    /// Writes every vertical opening (ARC-OPEN-002): its loops extruded plumb through its host
    /// (plus 1 mm each way), voiding the host. A floor's own body stays whole (a roof's has the
    /// hole). Then the shafts (ARC-OPEN-004).
    fn write_slab_openings(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for e in store.iter() {
            let Some(r) = reviera_core::slab_openings::opening_recipe(e) else {
                continue;
            };
            let Some(host) = GlobalId::parse(&r.host).and_then(|g| store.find_by_guid(&g)) else {
                continue;
            };
            let (Some(&step), Some(&host_step)) = (self.steps.get(&e.id()), self.steps.get(&host))
            else {
                continue;
            };
            let Some((lo, hi)) = host_span(store, host) else {
                continue;
            };
            // An opening by face in a roof (ARC-OPEN-001): prisms along the face's normal.
            if let Some(prisms) = reviera_core::slab_openings::face_prisms(store, &r) {
                let solids: Vec<geometry::Polyhedron> = prisms
                    .into_iter()
                    .map(|(mut bottom, mut top)| {
                        let flat: Vec<[f64; 2]> = bottom.iter().map(|q| [q[0], q[1]]).collect();
                        if reviera_core::sketch::signed_area(&flat) < 0.0 {
                            bottom.reverse();
                            top.reverse();
                        }
                        let n = bottom.len();
                        let mut faces =
                            vec![(0..n).rev().collect::<Vec<_>>(), (n..2 * n).collect()];
                        for i in 0..n {
                            let j = (i + 1) % n;
                            faces.push(vec![i, j, n + j, n + i]);
                        }
                        let mut points = bottom;
                        points.extend(top);
                        geometry::Polyhedron { points, faces }
                    })
                    .collect();
                geometry::write_breps(
                    &mut self.ifc,
                    step,
                    geometry::BrepFrame::WORLD,
                    &solids,
                    self.length_scale,
                    contexts,
                )?;
            } else {
                let body = geometry::SlabBody {
                    rings: r.loops.iter().map(|l| (l.as_slice(), Vec::new())).collect(),
                    top: hi + 0.001,
                    depth: hi - lo + 0.002,
                };
                geometry::write_slab(&mut self.ifc, step, &body, self.length_scale, contexts)?;
            }
            geometry::relate_one(
                &mut self.ifc,
                "IfcRelVoidsElement",
                "RelatingBuildingElement",
                host_step,
                "RelatedOpeningElement",
                step,
                owner_history,
            )?;
        }
        self.write_shafts(doc)
    }

    /// Writes every shaft (ARC-OPEN-004): its body the whole prism, and for each host it cuts an
    /// `IfcOpeningElement` of the prism through that host (plus 1 mm), voiding it. The derived
    /// openings carry `rxd:shaft <shaft> <host>` in their Description; ones no longer cut are
    /// removed.
    fn write_shafts(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let mut derived: HashMap<String, StepId> = HashMap::new();
        for o in self.ifc.entities_of_type("IfcOpeningElement")? {
            if let AttrValue::String(d) = self.ifc.attribute(o.id, "Description")?
                && d.starts_with(DERIVED_OPENING)
            {
                derived.insert(d, o.id);
            }
        }
        let shafts = reviera_core::slab_openings::shafts(store);
        if shafts.is_empty() && derived.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        let ifc4 = self.ifc.schema() != "IFC2X3";
        for (id, r, (z0, z1)) in shafts {
            let Some(&step) = self.steps.get(&id) else {
                continue;
            };
            let rings = || r.loops.iter().map(|l| (l.as_slice(), Vec::new())).collect();
            let body = geometry::SlabBody {
                rings: rings(),
                top: z1,
                depth: z1 - z0,
            };
            geometry::write_slab(&mut self.ifc, step, &body, self.length_scale, contexts)?;
            if let Some(storey) = GlobalId::parse(&r.extent.base_level)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied())
            {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
            let shaft_guid = store.get(id).expect("listed").guid().to_string();
            for (host, lo, hi) in reviera_core::slab_openings::shaft_hosts(store, &r) {
                let Some(&host_step) = self.steps.get(&host) else {
                    continue;
                };
                let host_guid = store.get(host).expect("listed").guid().to_string();
                let tag = format!("{DERIVED_OPENING} {shaft_guid} {host_guid}");
                let opening = match derived.remove(&tag) {
                    Some(o) => o,
                    None => {
                        let o = self
                            .ifc
                            .create_rooted("IfcOpeningElement", &GlobalId::new())?;
                        self.ifc.set_attribute(
                            o,
                            "Name",
                            &AttrValue::String("Shaft Opening".into()),
                        )?;
                        self.ifc
                            .set_attribute(o, "Description", &AttrValue::String(tag))?;
                        if let Some(h) = owner_history {
                            self.ifc
                                .set_attribute(o, "OwnerHistory", &AttrValue::Ref(h.0))?;
                        }
                        if ifc4 {
                            self.ifc.set_attribute(
                                o,
                                "PredefinedType",
                                &AttrValue::Enum("OPENING".into()),
                            )?;
                        }
                        o
                    }
                };
                let top = z1.min(hi) + 0.001;
                let body = geometry::SlabBody {
                    rings: rings(),
                    top,
                    depth: top - z0.max(lo) + 0.001,
                };
                geometry::write_slab(&mut self.ifc, opening, &body, self.length_scale, contexts)?;
                geometry::relate_one(
                    &mut self.ifc,
                    "IfcRelVoidsElement",
                    "RelatingBuildingElement",
                    host_step,
                    "RelatedOpeningElement",
                    opening,
                    owner_history,
                )?;
            }
        }
        // Openings of shafts that are gone, or of hosts they no longer cut.
        for (_, o) in derived {
            for rel in self.ifc.referenced_by(o, "IfcRelVoidsElement")? {
                self.ifc.remove(rel)?;
            }
            geometry::remove_slab(&mut self.ifc, o)?;
            self.ifc.remove(o)?;
        }
        Ok(())
    }

    /// Writes every Reviera floor (ARC-FLR-001): its body, extruded down from its top, contained
    /// in its level. Floors whose geometry did not change are left as they are.
    fn write_floors(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = floors::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, f) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            // A deck (ARC-DECK-001): one faceted B-rep per board, joist, beam, post and footing.
            let e = store.get(*id).expect("listed");
            if reviera_core::decks::deck_settings(e).is_some() {
                if let Ok(g) = reviera_core::decks::regenerate(store, e) {
                    geometry::remove_slab(&mut self.ifc, step)?;
                    let solids: Vec<geometry::Polyhedron> = g
                        .parts
                        .iter()
                        .map(|(points, _)| {
                            let n = points.len() / 2;
                            let mut faces =
                                vec![(0..n).rev().collect::<Vec<_>>(), (n..2 * n).collect()];
                            faces.extend(
                                (0..n).map(|i| vec![i, (i + 1) % n, n + (i + 1) % n, n + i]),
                            );
                            geometry::Polyhedron {
                                points: points.clone(),
                                faces,
                            }
                        })
                        .collect();
                    geometry::write_breps(
                        &mut self.ifc,
                        step,
                        geometry::BrepFrame::WORLD,
                        &solids,
                        self.length_scale,
                        contexts,
                    )?;
                }
                let storey = floors::slab_recipe(e)
                    .and_then(|(_, r)| GlobalId::parse(&r.level))
                    .and_then(|g| store.find_by_guid(&g))
                    .and_then(|l| self.steps.get(&l).copied());
                if let Some(storey) = storey {
                    geometry::contain(&mut self.ifc, step, storey, owner_history)?;
                }
                continue;
            }
            // A ceiling that follows its roof, or a floor with a warped top (ARC-SHAPE-001): its
            // pieces as faceted B-reps.
            let shaped = reviera_core::shape_edit::shape_of(e).and_then(|s| {
                let p = f.profiles.first()?;
                reviera_core::shape_edit::slab_mesh(p, &s, f.top, f.thickness)
                    .ok()
                    .map(|m| vec![m])
            });
            if let Some(meshes) = shaped.or_else(|| reviera_core::vaults::meshes(store, e)) {
                geometry::remove_slab(&mut self.ifc, step)?;
                let solids: Vec<geometry::Polyhedron> = meshes
                    .into_iter()
                    .map(|(points, idx)| geometry::Polyhedron {
                        points,
                        faces: idx
                            .chunks(3)
                            .map(|t| t.iter().map(|i| *i as usize).collect())
                            .collect(),
                    })
                    .collect();
                geometry::write_breps(
                    &mut self.ifc,
                    step,
                    geometry::BrepFrame::WORLD,
                    &solids,
                    self.length_scale,
                    contexts,
                )?;
                let storey = floors::slab_recipe(e)
                    .and_then(|(_, r)| GlobalId::parse(&r.level))
                    .and_then(|g| store.find_by_guid(&g))
                    .and_then(|l| self.steps.get(&l).copied());
                if let Some(storey) = storey {
                    geometry::contain(&mut self.ifc, step, storey, owner_history)?;
                }
                continue;
            }
            geometry::remove_breps(&mut self.ifc, step)?;
            let body = geometry::SlabBody {
                rings: f
                    .profiles
                    .iter()
                    .map(|p| {
                        (
                            p.outer.as_slice(),
                            p.holes.iter().map(Vec::as_slice).collect(),
                        )
                    })
                    .collect(),
                top: f.top,
                depth: f.thickness,
            };
            geometry::write_slab(&mut self.ifc, step, &body, self.length_scale, contexts)?;
            let storey = floors::slab_recipe(store.get(*id).expect("listed"))
                .and_then(|(_, r)| GlobalId::parse(&r.level))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes every Reviera roof (ARC-ROOF-001): one faceted B-rep per face slab, contained in
    /// its base level. Roofs whose geometry did not change are left as they are.
    fn write_roofs(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = roofs::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, g) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            // Sloped glazing (ARC-CW-008): the panels' and mullions' B-reps.
            let glazing = store
                .get(*id)
                .and_then(|e| roofs::glazing::of_roof(store, e))
                .map(|ty| roofs::glazing::regenerate(g, &ty));
            let glazed: Option<Vec<geometry::Polyhedron>> = glazing.map(|gl| {
                gl.parts
                    .iter()
                    .map(|p| geometry::Polyhedron {
                        points: p.mesh.0.clone(),
                        faces: p
                            .mesh
                            .1
                            .chunks(3)
                            .map(|t| t.iter().map(|i| *i as usize).collect())
                            .collect(),
                    })
                    .collect()
            });
            let solids: Vec<geometry::Polyhedron> = if let Some(s) = glazed {
                s
            } else {
                g.faces
                    .iter()
                    // A curved roof's body is one shell, on its first face.
                    .filter(|f| !f.slab.1.is_empty())
                    .map(|f| {
                        // A slab with courtyards through it, or clipped, is written as its
                        // triangles.
                        if !f.holes.is_empty() || f.faceted {
                            return geometry::Polyhedron {
                                points: f.slab.0.clone(),
                                faces: f
                                    .slab
                                    .1
                                    .chunks(3)
                                    .map(|t| t.iter().map(|i| *i as usize).collect())
                                    .collect(),
                            };
                        }
                        let n = f.slab.0.len() / 2;
                        let mut faces =
                            vec![(0..n).rev().collect::<Vec<_>>(), (n..2 * n).collect()];
                        faces.extend((0..n).map(|i| vec![i, (i + 1) % n, n + (i + 1) % n, n + i]));
                        geometry::Polyhedron {
                            points: f.slab.0.clone(),
                            faces,
                        }
                    })
                    .chain(g.trims.iter().map(|t| {
                        geometry::Polyhedron {
                            points: t.mesh.0.clone(),
                            faces: t
                                .mesh
                                .1
                                .chunks(3)
                                .map(|t| t.iter().map(|i| *i as usize).collect())
                                .collect(),
                        }
                    }))
                    .collect()
            };
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &solids,
                self.length_scale,
                contexts,
            )?;
            let storey = roofs::roof_recipe(store.get(*id).expect("listed"))
                .and_then(|r| GlobalId::parse(&r.level))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes every Reviera column (ARC-COL-001, STR-COL-001): its body as a faceted B-rep (its top
    /// may follow an attached roof), contained in its base level.
    fn write_columns(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = reviera_core::columns::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, g) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            // The body's triangles: any section, with holes, and a top that may follow a roof.
            let (points, indices) = g.mesh();
            let faces: Vec<Vec<usize>> = indices
                .chunks(3)
                .map(|t| t.iter().map(|i| *i as usize).collect())
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &[geometry::Polyhedron { points, faces }],
                self.length_scale,
                contexts,
            )?;
            let storey = reviera_core::columns::column_recipe(store.get(*id).expect("listed"))
                .and_then(|r| GlobalId::parse(&r.extent.base_level))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes every Reviera beam (STR-BEAM-001): its body, swept and joined, as a faceted B-rep,
    /// contained in its reference level.
    fn write_beams(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = reviera_core::beams::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, g) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let (points, indices) = g.mesh();
            let faces: Vec<Vec<usize>> = indices
                .chunks(3)
                .map(|t| t.iter().map(|i| *i as usize).collect())
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &[geometry::Polyhedron { points, faces }],
                self.length_scale,
                contexts,
            )?;
            // A system's beams are aggregated by it, not contained.
            let recipe = reviera_core::beams::beam_recipe(store.get(*id).expect("listed"));
            if recipe.as_ref().is_some_and(|r| r.system.is_some()) {
                continue;
            }
            let storey = recipe
                .and_then(|r| GlobalId::parse(&r.level))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes the analytical model (STR-AM-001, 003): each node's vertex and each member's axis and
    /// edge between its nodes' vertices (`Reference` topology representations), each member end's
    /// `IfcRelConnectsStructuralMember` with its releases as an `IfcBoundaryNodeCondition`, and the
    /// members and nodes assigned to the `IfcStructuralAnalysisModel`. Reviera's relations are
    /// marked `rxd:analytical` and rewritten on every save.
    fn write_analytical(&mut self, doc: &Document) -> Result<()> {
        const TAG: &str = "rxd:analytical";
        let store = doc.store();
        let nodes = reviera_core::analytical::nodes(store);
        let members = reviera_core::analytical::members(store);
        let panels = reviera_core::analytical_panels::panels(store);
        let model = store
            .iter()
            .find(|e| reviera_core::analytical::is_model(e))
            .and_then(|e| self.steps.get(&e.id()).copied());
        if nodes.is_empty() && members.is_empty() && panels.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let scale = self.length_scale;
        let r = |id: StepId| AttrValue::Ref(id.0);
        // Reviera's earlier relations and representations go first.
        // Members before nodes: their edges use the nodes' vertices.
        let mut steps: Vec<StepId> = Vec::new();
        for (id, _) in &members {
            steps.extend(self.steps.get(id).copied());
        }
        for (id, _) in &panels {
            steps.extend(self.steps.get(id).copied());
        }
        for (id, _) in &nodes {
            steps.extend(self.steps.get(id).copied());
        }
        for &step in &steps {
            for class in ["IfcRelConnectsStructuralMember", "IfcRelAssignsToGroup"] {
                for rel in self.ifc.referenced_by(step, class)? {
                    if matches!(self.ifc.attribute(rel, "Description")?, AttrValue::String(d) if d == TAG)
                    {
                        if let AttrValue::Ref(c) = self
                            .ifc
                            .attribute(rel, "AppliedCondition")
                            .unwrap_or(AttrValue::Null)
                        {
                            let _ = self.ifc.remove(StepId(c));
                        }
                        let _ = self.ifc.remove(rel);
                    }
                }
            }
            if let AttrValue::Ref(shape) = self.ifc.attribute(step, "Representation")? {
                self.remove_topology(StepId(shape))?;
                self.ifc
                    .set_attribute(step, "Representation", &AttrValue::Null)?;
            }
        }
        let point = |ifc: &mut IfcModel, p: [f64; 3]| -> Result<StepId> {
            let id = ifc.create("IfcCartesianPoint")?;
            ifc.set_attribute(
                id,
                "Coordinates",
                &AttrValue::List(p.iter().map(|v| AttrValue::Real(v / scale)).collect()),
            )?;
            Ok(id)
        };
        let topology = |ifc: &mut IfcModel, kind: &str, item: StepId| -> Result<StepId> {
            let rep = ifc.create("IfcTopologyRepresentation")?;
            ifc.set_attribute(rep, "ContextOfItems", &AttrValue::Ref(contexts.axis.0))?;
            ifc.set_attribute(
                rep,
                "RepresentationIdentifier",
                &AttrValue::String("Reference".into()),
            )?;
            ifc.set_attribute(rep, "RepresentationType", &AttrValue::String(kind.into()))?;
            ifc.set_attribute(rep, "Items", &AttrValue::List(vec![AttrValue::Ref(item.0)]))?;
            let shape = ifc.create("IfcProductDefinitionShape")?;
            ifc.set_attribute(
                shape,
                "Representations",
                &AttrValue::List(vec![AttrValue::Ref(rep.0)]),
            )?;
            Ok(shape)
        };
        let mut vertex: HashMap<ElementId, StepId> = HashMap::new();
        for (id, p) in &nodes {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let pt = point(&mut self.ifc, *p)?;
            let v = self.ifc.create("IfcVertexPoint")?;
            self.ifc.set_attribute(v, "VertexGeometry", &r(pt))?;
            vertex.insert(*id, v);
            let shape = topology(&mut self.ifc, "Vertex", v)?;
            self.ifc.set_attribute(step, "Representation", &r(shape))?;
        }
        let stiff = |fixed: bool| AttrValue::Typed {
            type_name: "IfcBoolean".into(),
            value: Box::new(AttrValue::Bool(fixed)),
        };
        let mut grouped: Vec<AttrValue> = Vec::new();
        for (id, m) in &members {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let (Some(&va), Some(&vb)) = (vertex.get(&m.nodes[0]), vertex.get(&m.nodes[1])) else {
                continue;
            };
            let d = [m.end[0] - m.start[0], m.end[1] - m.start[1]];
            // The section's local z: up for members that run across, along x for vertical ones.
            let axis = if d[0].hypot(d[1]) < 1e-6 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 0.0, 1.0]
            };
            let dir = self.ifc.create("IfcDirection")?;
            self.ifc.set_attribute(
                dir,
                "DirectionRatios",
                &AttrValue::List(axis.iter().map(|v| AttrValue::Real(*v)).collect()),
            )?;
            self.ifc.set_attribute(step, "Axis", &r(dir))?;
            let edge = self.ifc.create("IfcEdge")?;
            self.ifc.set_attribute(edge, "EdgeStart", &r(va))?;
            self.ifc.set_attribute(edge, "EdgeEnd", &r(vb))?;
            let shape = topology(&mut self.ifc, "Edge", edge)?;
            self.ifc.set_attribute(step, "Representation", &r(shape))?;
            for (k, node) in m.nodes.iter().enumerate() {
                let Some(&ns) = self.steps.get(node) else {
                    continue;
                };
                let f = m.recipe.releases[k];
                let cond = self.ifc.create("IfcBoundaryNodeCondition")?;
                for (i, attr) in [
                    "TranslationalStiffnessX",
                    "TranslationalStiffnessY",
                    "TranslationalStiffnessZ",
                    "RotationalStiffnessX",
                    "RotationalStiffnessY",
                    "RotationalStiffnessZ",
                ]
                .into_iter()
                .enumerate()
                {
                    self.ifc.set_attribute(cond, attr, &stiff(f[i]))?;
                }
                let rel = self
                    .ifc
                    .create_rooted("IfcRelConnectsStructuralMember", &GlobalId::new())?;
                self.ifc
                    .set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
                self.ifc
                    .set_attribute(rel, "RelatingStructuralMember", &r(step))?;
                self.ifc
                    .set_attribute(rel, "RelatedStructuralConnection", &r(ns))?;
                self.ifc.set_attribute(rel, "AppliedCondition", &r(cond))?;
            }
            grouped.push(r(step));
        }
        // Panels (STR-AM-002): a face on a plane or a cylinder, bounded by a polyloop through its
        // outline (arcs sampled), its openings inner bounds; connected to its corner nodes.
        for (id, pg) in &panels {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            self.ifc.set_attribute(
                step,
                "Thickness",
                &AttrValue::Real(pg.recipe.thickness / scale),
            )?;
            let dir = |ifc: &mut IfcModel, v: [f64; 3]| -> Result<StepId> {
                let d = ifc.create("IfcDirection")?;
                ifc.set_attribute(
                    d,
                    "DirectionRatios",
                    &AttrValue::List(v.iter().map(|x| AttrValue::Real(*x)).collect()),
                )?;
                Ok(d)
            };
            let placement =
                |ifc: &mut IfcModel, o: [f64; 3], z: [f64; 3], x: [f64; 3]| -> Result<StepId> {
                    let a = ifc.create("IfcAxis2Placement3D")?;
                    let l = point(ifc, o)?;
                    ifc.set_attribute(a, "Location", &AttrValue::Ref(l.0))?;
                    let zd = dir(ifc, z)?;
                    ifc.set_attribute(a, "Axis", &AttrValue::Ref(zd.0))?;
                    let xd = dir(ifc, x)?;
                    ifc.set_attribute(a, "RefDirection", &AttrValue::Ref(xd.0))?;
                    Ok(a)
                };
            let surface = if let Some((c, radius)) = pg.circle {
                let z0 = pg.corners[0][2];
                let pos = placement(
                    &mut self.ifc,
                    [c[0], c[1], z0],
                    [0.0, 0.0, 1.0],
                    [1.0, 0.0, 0.0],
                )?;
                let cyl = self.ifc.create("IfcCylindricalSurface")?;
                self.ifc.set_attribute(cyl, "Position", &r(pos))?;
                self.ifc
                    .set_attribute(cyl, "Radius", &AttrValue::Real(radius / scale))?;
                cyl
            } else {
                let n = pg.normal.unwrap_or([0.0, 0.0, 1.0]);
                let o = pg.corners[0];
                let e = [0, 1, 2].map(|k| pg.corners[1][k] - o[k]);
                let l = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt().max(1e-12);
                let pos = placement(&mut self.ifc, o, n, e.map(|v| v / l))?;
                let plane = self.ifc.create("IfcPlane")?;
                self.ifc.set_attribute(plane, "Position", &r(pos))?;
                plane
            };
            let mut bounds = Vec::new();
            let mut rings = vec![(pg.outline.clone(), true)];
            rings.extend(pg.recipe.openings.iter().map(|o| (o.clone(), false)));
            for (ring, outer) in rings {
                let mut pts = Vec::new();
                for q in &ring {
                    pts.push(AttrValue::Ref(point(&mut self.ifc, *q)?.0));
                }
                let lp = self.ifc.create("IfcPolyLoop")?;
                self.ifc
                    .set_attribute(lp, "Polygon", &AttrValue::List(pts))?;
                let b = self.ifc.create(if outer {
                    "IfcFaceOuterBound"
                } else {
                    "IfcFaceBound"
                })?;
                self.ifc.set_attribute(b, "Bound", &r(lp))?;
                self.ifc
                    .set_attribute(b, "Orientation", &AttrValue::Bool(true))?;
                bounds.push(r(b));
            }
            let face = self.ifc.create("IfcFaceSurface")?;
            self.ifc
                .set_attribute(face, "Bounds", &AttrValue::List(bounds))?;
            self.ifc.set_attribute(face, "FaceSurface", &r(surface))?;
            self.ifc
                .set_attribute(face, "SameSense", &AttrValue::Bool(true))?;
            let shape = topology(&mut self.ifc, "Face", face)?;
            self.ifc.set_attribute(step, "Representation", &r(shape))?;
            for node in &pg.nodes {
                let Some(&ns) = self.steps.get(node) else {
                    continue;
                };
                let rel = self
                    .ifc
                    .create_rooted("IfcRelConnectsStructuralMember", &GlobalId::new())?;
                self.ifc
                    .set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
                self.ifc
                    .set_attribute(rel, "RelatingStructuralMember", &r(step))?;
                self.ifc
                    .set_attribute(rel, "RelatedStructuralConnection", &r(ns))?;
            }
            grouped.push(r(step));
        }
        grouped.extend(
            nodes
                .iter()
                .filter_map(|(id, _)| self.steps.get(id).map(|s| r(*s))),
        );
        if let (Some(model), false) = (model, grouped.is_empty()) {
            let rel = self
                .ifc
                .create_rooted("IfcRelAssignsToGroup", &GlobalId::new())?;
            self.ifc
                .set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
            self.ifc
                .set_attribute(rel, "RelatedObjects", &AttrValue::List(grouped))?;
            self.ifc.set_attribute(rel, "RelatingGroup", &r(model))?;
        }
        Ok(())
    }

    /// Writes loads, load groups and supports (STR-LOAD-001, STR-LC-001, STR-BC-001): each supported
    /// node's `AppliedCondition` (an `IfcBoundaryNodeCondition`: `IfcBoolean` for fixed or released,
    /// stiffness measures for springs); each combination's cases by `IfcRelAssignsToGroupByFactor`;
    /// every load group as `LoadedBy` the analysis model; and each load's applied load (a single
    /// force, a linear force, or a linear configuration of two), its host by
    /// `IfcRelConnectsStructuralActivity` and its case by `IfcRelAssignsToGroup`. Reviera's relations
    /// are marked `rxd:analytical` and rewritten on every save.
    fn write_loads(&mut self, doc: &Document) -> Result<()> {
        const TAG: &str = "rxd:analytical";
        let store = doc.store();
        let r = |id: StepId| AttrValue::Ref(id.0);
        let tagged = |ifc: &IfcModel, rel: StepId| -> bool {
            matches!(ifc.attribute(rel, "Description"), Ok(AttrValue::String(d)) if d == TAG)
        };
        // Supports.
        for e in store.iter() {
            let Some(n) = reviera_core::analytical::node_recipe(e) else {
                continue;
            };
            let Some(&step) = self.steps.get(&e.id()) else {
                continue;
            };
            if let AttrValue::Ref(old) = self.ifc.attribute(step, "AppliedCondition")? {
                self.ifc
                    .set_attribute(step, "AppliedCondition", &AttrValue::Null)?;
                self.ifc.remove(StepId(old))?;
            }
            let Some(sup) = n.support else { continue };
            let cond = self.ifc.create("IfcBoundaryNodeCondition")?;
            self.ifc
                .set_attribute(cond, "Name", &AttrValue::String(format!("{:?}", sup.state)))?;
            for (i, attr) in [
                "TranslationalStiffnessX",
                "TranslationalStiffnessY",
                "TranslationalStiffnessZ",
                "RotationalStiffnessX",
                "RotationalStiffnessY",
                "RotationalStiffnessZ",
            ]
            .into_iter()
            .enumerate()
            {
                let v = match sup.dof[i] {
                    reviera_core::loads::Dof::Fixed => ("IfcBoolean", AttrValue::Bool(true)),
                    reviera_core::loads::Dof::Released => ("IfcBoolean", AttrValue::Bool(false)),
                    reviera_core::loads::Dof::Spring(k) if i < 3 => {
                        ("IfcLinearStiffnessMeasure", AttrValue::Real(k))
                    }
                    reviera_core::loads::Dof::Spring(k) => {
                        ("IfcRotationalStiffnessMeasure", AttrValue::Real(k))
                    }
                };
                self.ifc.set_attribute(
                    cond,
                    attr,
                    &AttrValue::Typed {
                        type_name: v.0.into(),
                        value: Box::new(v.1),
                    },
                )?;
            }
            self.ifc.set_attribute(step, "AppliedCondition", &r(cond))?;
        }
        // Combinations' factors, and the load groups the model is loaded by.
        let mut groups = Vec::new();
        for e in store.iter() {
            let is_case = reviera_core::loads::case_recipe(e).is_some();
            let combination = reviera_core::loads::combination_recipe(e);
            if !is_case && combination.is_none() {
                continue;
            }
            let Some(&step) = self.steps.get(&e.id()) else {
                continue;
            };
            groups.push(r(step));
            for rel in self
                .ifc
                .referenced_by(step, "IfcRelAssignsToGroupByFactor")?
            {
                if tagged(&self.ifc, rel) {
                    self.ifc.remove(rel)?;
                }
            }
            let Some(c) = combination else { continue };
            let name = e.name.clone().unwrap_or_default();
            let Ok(factors) = reviera_core::loads::factors(store, &c.formula, Some(&name)) else {
                continue;
            };
            for (case, f) in factors {
                let Some(&cs) = self.steps.get(&case) else {
                    continue;
                };
                let rel = self
                    .ifc
                    .create_rooted("IfcRelAssignsToGroupByFactor", &GlobalId::new())?;
                self.ifc
                    .set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
                self.ifc
                    .set_attribute(rel, "RelatedObjects", &AttrValue::List(vec![r(cs)]))?;
                self.ifc.set_attribute(rel, "RelatingGroup", &r(step))?;
                self.ifc.set_attribute(rel, "Factor", &AttrValue::Real(f))?;
            }
        }
        if let Some(model) = store
            .iter()
            .find(|e| reviera_core::analytical::is_model(e))
            .and_then(|e| self.steps.get(&e.id()).copied())
        {
            self.ifc.set_attribute(
                model,
                "LoadedBy",
                &if groups.is_empty() {
                    AttrValue::Null
                } else {
                    AttrValue::List(groups)
                },
            )?;
        }
        // Loads.
        let members = reviera_core::analytical::members(store);
        let mut by_case: HashMap<String, Vec<AttrValue>> = HashMap::new();
        for e in store.iter() {
            let point = reviera_core::loads::point_load(e);
            let line = reviera_core::loads::line_load(e);
            let area = reviera_core::loads::area_load(e);
            if point.is_none() && line.is_none() && area.is_none() {
                continue;
            }
            let Some(&step) = self.steps.get(&e.id()) else {
                continue;
            };
            for class in ["IfcRelConnectsStructuralActivity", "IfcRelAssignsToGroup"] {
                for rel in self.ifc.referenced_by(step, class)? {
                    if tagged(&self.ifc, rel) {
                        self.ifc.remove(rel)?;
                    }
                }
            }
            if let AttrValue::Ref(old) = self.ifc.attribute(step, "AppliedLoad")? {
                self.ifc
                    .set_attribute(step, "AppliedLoad", &AttrValue::Null)?;
                let inner = self
                    .ifc
                    .attribute(StepId(old), "Values")
                    .unwrap_or(AttrValue::Null);
                self.ifc.remove(StepId(old))?;
                if let AttrValue::List(items) = inner {
                    for i in items {
                        if let AttrValue::Ref(i) = i {
                            self.ifc.remove(StepId(i))?;
                        }
                    }
                }
            }
            let reals = |v: &[f64]| v.iter().map(|x| AttrValue::Real(*x)).collect::<Vec<_>>();
            let (host, case, load) = if let Some(p) = point {
                let load = self.ifc.create("IfcStructuralLoadSingleForce")?;
                for (i, attr) in [
                    "ForceX", "ForceY", "ForceZ", "MomentX", "MomentY", "MomentZ",
                ]
                .into_iter()
                .enumerate()
                {
                    self.ifc
                        .set_attribute(load, attr, &AttrValue::Real(p.forces[i]))?;
                }
                (p.host, p.case, load)
            } else if let Some(a) = area {
                let load = self.ifc.create("IfcStructuralLoadPlanarForce")?;
                for (i, attr) in ["PlanarForceX", "PlanarForceY", "PlanarForceZ"]
                    .into_iter()
                    .enumerate()
                {
                    self.ifc
                        .set_attribute(load, attr, &AttrValue::Real(a.value[i]))?;
                }
                (a.host, a.case, load)
            } else {
                let l = line.expect("a line load");
                let linear = |ifc: &mut IfcModel, v: [f64; 3]| -> Result<StepId> {
                    let id = ifc.create("IfcStructuralLoadLinearForce")?;
                    for (i, attr) in ["LinearForceX", "LinearForceY", "LinearForceZ"]
                        .into_iter()
                        .enumerate()
                    {
                        ifc.set_attribute(id, attr, &AttrValue::Real(v[i]))?;
                    }
                    Ok(id)
                };
                let load = if l.start == l.end {
                    linear(&mut self.ifc, l.start)?
                } else {
                    let a = linear(&mut self.ifc, l.start)?;
                    let b = linear(&mut self.ifc, l.end)?;
                    let length = members
                        .iter()
                        .find(|(m, _)| {
                            store
                                .get(*m)
                                .is_some_and(|me| me.guid().to_string() == l.host)
                        })
                        .map_or(1.0, |(_, m)| {
                            let d = [
                                m.end[0] - m.start[0],
                                m.end[1] - m.start[1],
                                m.end[2] - m.start[2],
                            ];
                            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                        });
                    let cfg = self.ifc.create("IfcStructuralLoadConfiguration")?;
                    self.ifc
                        .set_attribute(cfg, "Values", &AttrValue::List(vec![r(a), r(b)]))?;
                    self.ifc.set_attribute(
                        cfg,
                        "Locations",
                        &AttrValue::List(vec![
                            AttrValue::List(reals(&[0.0])),
                            AttrValue::List(reals(&[length / self.length_scale])),
                        ]),
                    )?;
                    cfg
                };
                (l.host, l.case, load)
            };
            self.ifc.set_attribute(step, "AppliedLoad", &r(load))?;
            if let Some(&hs) = GlobalId::parse(&host)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|h| self.steps.get(&h))
            {
                let rel = self
                    .ifc
                    .create_rooted("IfcRelConnectsStructuralActivity", &GlobalId::new())?;
                self.ifc
                    .set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
                self.ifc.set_attribute(rel, "RelatingElement", &r(hs))?;
                self.ifc
                    .set_attribute(rel, "RelatedStructuralActivity", &r(step))?;
            }
            by_case.entry(case).or_default().push(r(step));
        }
        for (case, actions) in by_case {
            let Some(&cs) = GlobalId::parse(&case)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|c| self.steps.get(&c))
            else {
                continue;
            };
            let rel = self
                .ifc
                .create_rooted("IfcRelAssignsToGroup", &GlobalId::new())?;
            self.ifc
                .set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
            self.ifc
                .set_attribute(rel, "RelatedObjects", &AttrValue::List(actions))?;
            self.ifc.set_attribute(rel, "RelatingGroup", &r(cs))?;
        }
        Ok(())
    }

    /// Removes a topology representation written by [`Self::write_analytical`]: the shape, its
    /// representations and their vertex and edge items with their points.
    fn remove_topology(&mut self, shape: StepId) -> Result<()> {
        let refs = |v: AttrValue| -> Vec<StepId> {
            match v {
                AttrValue::Ref(i) => vec![StepId(i)],
                AttrValue::List(l) => l
                    .into_iter()
                    .filter_map(|x| match x {
                        AttrValue::Ref(i) => Some(StepId(i)),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            }
        };
        for rep in refs(self.ifc.attribute(shape, "Representations")?) {
            for item in refs(self.ifc.attribute(rep, "Items")?) {
                let class = self.ifc.class_of(item)?;
                if class.eq_ignore_ascii_case("IfcEdge") {
                    // Its vertices are the nodes', removed with them.
                    self.ifc.remove(item)?;
                } else if class.eq_ignore_ascii_case("IfcVertexPoint") {
                    for p in refs(self.ifc.attribute(item, "VertexGeometry")?) {
                        self.ifc.remove(p)?;
                    }
                    self.ifc.remove(item)?;
                } else if class.eq_ignore_ascii_case("IfcFaceSurface") {
                    // A panel's face: its bounds, loops and points, and its surface and placement.
                    for b in refs(self.ifc.attribute(item, "Bounds")?) {
                        for lp in refs(self.ifc.attribute(b, "Bound")?) {
                            for p in refs(self.ifc.attribute(lp, "Polygon")?) {
                                self.ifc.remove(p)?;
                            }
                            self.ifc.remove(lp)?;
                        }
                        self.ifc.remove(b)?;
                    }
                    for sf in refs(self.ifc.attribute(item, "FaceSurface")?) {
                        for pos in refs(self.ifc.attribute(sf, "Position")?) {
                            for attr in ["Location", "Axis", "RefDirection"] {
                                for x in refs(self.ifc.attribute(pos, attr)?) {
                                    self.ifc.remove(x)?;
                                }
                            }
                            self.ifc.remove(pos)?;
                        }
                        self.ifc.remove(sf)?;
                    }
                    self.ifc.remove(item)?;
                }
            }
            self.ifc.remove(rep)?;
        }
        Ok(self.ifc.remove(shape)?)
    }

    /// Writes steel connections' fabrication elements (STR-STL-002): each plate, bolt, anchor rod
    /// and weld as a faceted body (bolts with their nominal diameter and length), aggregated by
    /// its connection through an `IfcRelAggregates` marked `rxd:connection`.
    fn write_fabrication(&mut self, doc: &Document) -> Result<()> {
        const TAG: &str = "rxd:connection";
        let store = doc.store();
        let all = reviera_core::steel_fab::all(store);
        let owner_history = self.new_owner_history()?;
        if !all.is_empty() {
            let contexts = geometry::Contexts::find(&self.ifc)?;
            for (id, part) in &all {
                let Some(&step) = self.steps.get(id) else {
                    continue;
                };
                let (points, indices) = part.mesh();
                let faces: Vec<Vec<usize>> = indices
                    .chunks(3)
                    .map(|t| t.iter().map(|i| *i as usize).collect())
                    .collect();
                geometry::write_breps(
                    &mut self.ifc,
                    step,
                    geometry::BrepFrame::WORLD,
                    &[geometry::Polyhedron { points, faces }],
                    self.length_scale,
                    contexts,
                )?;
                if let reviera_core::steel_fab::Part::Bolt {
                    start,
                    end,
                    diameter,
                    ..
                }
                | reviera_core::steel_fab::Part::Stud {
                    start,
                    end,
                    diameter,
                } = part
                {
                    let len = ((end[0] - start[0]).powi(2)
                        + (end[1] - start[1]).powi(2)
                        + (end[2] - start[2]).powi(2))
                    .sqrt();
                    self.ifc.set_attribute(
                        step,
                        "NominalDiameter",
                        &AttrValue::Real(diameter / self.length_scale),
                    )?;
                    self.ifc.set_attribute(
                        step,
                        "NominalLength",
                        &AttrValue::Real(len / self.length_scale),
                    )?;
                }
            }
        }
        // Plates and welds placed by hand (STR-STL-005, STR-STL-007).
        let mut contexts = None;
        for e in store.iter() {
            let mesh = if let Some(r) = reviera_core::steel_detail::plate_recipe(e) {
                reviera_core::steel_detail::plate_mesh(store, &r)
            } else if let Some(r) = reviera_core::steel_detail::weld_recipe(e) {
                reviera_core::steel_detail::weld_mesh(&r)
            } else {
                continue;
            };
            let Some(&step) = self.steps.get(&e.id()) else {
                continue;
            };
            let c = match contexts {
                Some(c) => c,
                None => *contexts.insert(geometry::Contexts::find(&self.ifc)?),
            };
            let (points, indices) = mesh;
            let faces: Vec<Vec<usize>> = indices
                .chunks(3)
                .map(|t| t.iter().map(|i| *i as usize).collect())
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &[geometry::Polyhedron { points, faces }],
                self.length_scale,
                c,
            )?;
        }
        self.write_holes(doc)?;
        self.write_cuts(doc)?;
        // Each steel assembly (STR-STL-010) aggregates its members and plates placed by hand;
        // connection plates stay with their connections (an object has one aggregation).
        for e in store.iter() {
            let Some(parts) = reviera_core::steel_numbering::assembly_parts(store, e) else {
                continue;
            };
            let Some(&step) = self.steps.get(&e.id()) else {
                continue;
            };
            let parts: Vec<AttrValue> = parts
                .into_iter()
                // Beam system and truss members are aggregated by those.
                .filter(|p| {
                    store.get(*p).is_some_and(|f| {
                        reviera_core::steel_fab::fab_recipe(f).is_none()
                            && reviera_core::beams::beam_recipe(f).is_none_or(|r| r.system.is_none())
                    })
                })
                .filter_map(|p| self.steps.get(&p).map(|s| AttrValue::Ref(s.0)))
                .collect();
            for rel in self.ifc.referenced_by(step, "IfcRelAggregates")? {
                self.ifc.remove(rel)?;
            }
            if parts.is_empty() {
                continue;
            }
            let rel = self.ifc.create_rooted("IfcRelAggregates", &GlobalId::new())?;
            self.ifc.set_attribute(rel, "Description", &AttrValue::String("rxd:steel_assembly".into()))?;
            if let Some(h) = owner_history {
                self.ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(h.0))?;
            }
            self.ifc.set_attribute(rel, "RelatingObject", &AttrValue::Ref(step.0))?;
            self.ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(parts))?;
        }
        // Each connection (and fastener pattern) aggregates its fabrication elements.
        for e in store.iter() {
            if reviera_core::connections::recipe(e).is_none()
                && reviera_core::steel_detail::pattern_recipe(e).is_none()
            {
                continue;
            }
            let Some(&step) = self.steps.get(&e.id()) else {
                continue;
            };
            for rel in self.ifc.referenced_by(step, "IfcRelAggregates")? {
                if matches!(self.ifc.attribute(rel, "Description")?, AttrValue::String(d) if d == TAG)
                {
                    self.ifc.remove(rel)?;
                }
            }
            let guid = e.guid().to_string();
            let parts: Vec<AttrValue> = store
                .iter()
                .filter(|f| reviera_core::steel_fab::fab_recipe(f).is_some_and(|r| r.connection == guid))
                .filter_map(|f| self.steps.get(&f.id()).map(|s| AttrValue::Ref(s.0)))
                .collect();
            if parts.is_empty() {
                continue;
            }
            let rel = self
                .ifc
                .create_rooted("IfcRelAggregates", &GlobalId::new())?;
            self.ifc
                .set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
            if let Some(h) = owner_history {
                self.ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(h.0))?;
            }
            self.ifc.set_attribute(rel, "RelatingObject", &AttrValue::Ref(step.0))?;
            self.ifc
                .set_attribute(rel, "RelatedObjects", &AttrValue::List(parts))?;
        }
        Ok(())
    }

    /// Writes rebar sets (STR-RBR-004): each `IfcReinforcingBar` gets a tube body along its bars,
    /// its diameter, cross-section area and bar length, and a synthesized `IfcReinforcingBarType`
    /// (marked `rxd:rebar_type`, rewritten on every save) with its shape code and dimensions as
    /// `BendingShapeCode` and `BendingParameters`; each host aggregates its sets (`rxd:rebar`).
    fn write_rebar(&mut self, doc: &Document) -> Result<()> {
        const TAG: &str = "rxd:rebar";
        let store = doc.store();
        for t in self.ifc.entities_of_type("IfcReinforcingBarType")? {
            let t = t.id;
            if matches!(self.ifc.attribute(t, "Description")?, AttrValue::String(d) if d == REBAR_TYPE_TAG) {
                for rel in self.ifc.referenced_by(t, "IfcRelDefinesByType")? {
                    self.ifc.remove(rel)?;
                }
                self.ifc.remove(t)?;
            }
        }
        self.remove_rebar_parts()?;
        let sets: Vec<ElementId> = store.iter().filter(|e| reviera_core::rebar::rebar_recipe(e).is_some()).map(|e| e.id()).collect();
        let systems: Vec<ElementId> = store.iter().filter(|e| reviera_core::rebar_systems::system_kind(e).is_some()).map(|e| e.id()).collect();
        if sets.is_empty() && systems.is_empty() {
            return Ok(());
        }
        let owner_history = self.new_owner_history()?;
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let infos = reviera_core::rebar::info(store, reviera_core::rebar::InfoParams { rebar_ids: sets.iter().map(|i| reviera_core::ElementRef::Id(*i)).collect() })
            .unwrap_or_default();
        let len = |v: f64| AttrValue::Typed { type_name: "IfcPositiveLengthMeasure".into(), value: Box::new(AttrValue::Real(v)) };
        let mut by_host: BTreeMap<ElementId, Vec<AttrValue>> = BTreeMap::new();
        for i in &infos {
            let Some(&step) = self.steps.get(&i.id) else { continue };
            let e = store.get(i.id).expect("a set");
            if let Some(mesh) = reviera_core::rebar::mesh(store, e) {
                let (points, indices) = mesh;
                let faces: Vec<Vec<usize>> = indices.chunks(3).map(|t| t.iter().map(|k| *k as usize).collect()).collect();
                geometry::write_breps(&mut self.ifc, step, geometry::BrepFrame::WORLD, &[geometry::Polyhedron { points, faces }], self.length_scale, contexts)?;
            }
            let sc = self.length_scale;
            let area = std::f64::consts::PI * i.diameter * i.diameter / 4.0;
            self.ifc.set_attribute(step, "NominalDiameter", &AttrValue::Real(i.diameter / sc))?;
            self.ifc.set_attribute(step, "CrossSectionArea", &AttrValue::Real(area / sc / sc))?;
            if let Some(l) = i.rounded_lengths.first() {
                self.ifc.set_attribute(step, "BarLength", &AttrValue::Real(l / sc))?;
            }
            let predefined = if i.role == "ligature" { "LIGATURE" } else { "MAIN" };
            self.ifc.set_attribute(step, "PredefinedType", &AttrValue::Enum(predefined.into()))?;
            self.ifc.set_attribute(step, "BarSurface", &AttrValue::Enum("TEXTURED".into()))?;
            // The rebar number (STR-RBR-016).
            if let Some(n) = i.number {
                self.ifc.set_attribute(step, "Tag", &AttrValue::String(n.to_string()))?;
            }
            let ty = self.ifc.create_rooted("IfcReinforcingBarType", &GlobalId::new())?;
            self.ifc.set_attribute(ty, "Name", &AttrValue::String(format!("{} shape {}", i.bar_type, i.shape)))?;
            self.ifc.set_attribute(ty, "Description", &AttrValue::String(REBAR_TYPE_TAG.into()))?;
            if let Some(h) = owner_history {
                self.ifc.set_attribute(ty, "OwnerHistory", &AttrValue::Ref(h.0))?;
            }
            self.ifc.set_attribute(ty, "PredefinedType", &AttrValue::Enum(predefined.into()))?;
            self.ifc.set_attribute(ty, "NominalDiameter", &AttrValue::Real(i.diameter / sc))?;
            self.ifc.set_attribute(ty, "CrossSectionArea", &AttrValue::Real(area / sc / sc))?;
            if let Some(l) = i.rounded_lengths.first() {
                self.ifc.set_attribute(ty, "BarLength", &AttrValue::Real(l / sc))?;
            }
            self.ifc.set_attribute(ty, "BarSurface", &AttrValue::Enum("TEXTURED".into()))?;
            self.ifc.set_attribute(ty, "BendingShapeCode", &AttrValue::String(i.shape.clone()))?;
            let params: Vec<AttrValue> = i.dimensions.iter().map(|(_, v)| len(v / sc)).collect();
            self.ifc.set_attribute(ty, "BendingParameters", &AttrValue::List(params))?;
            let typing = self.ifc.create_rooted("IfcRelDefinesByType", &GlobalId::new())?;
            self.ifc.set_attribute(typing, "Description", &AttrValue::String(REBAR_TYPE_TAG.into()))?;
            if let Some(h) = owner_history {
                self.ifc.set_attribute(typing, "OwnerHistory", &AttrValue::Ref(h.0))?;
            }
            self.ifc.set_attribute(typing, "RelatedObjects", &AttrValue::List(vec![AttrValue::Ref(step.0)]))?;
            self.ifc.set_attribute(typing, "RelatingType", &AttrValue::Ref(ty.0))?;
            if let Some(h) = i.host {
                by_host.entry(h).or_default().push(AttrValue::Ref(step.0));
            }
        }
        // Area, path and fabric systems and single sheets (STR-RBR-010 … 012).
        for id in systems {
            let Some(&step) = self.steps.get(&id) else { continue };
            let e = store.get(id).expect("a system");
            self.write_rebar_parts(store, e, step, owner_history, contexts)?;
            if let Some(h) = reviera_core::rebar_systems::info(store, reviera_core::rebar_systems::InfoParams { system_ids: vec![reviera_core::ElementRef::Id(id)] })
                .ok()
                .and_then(|i| i.first().and_then(|i| i.host))
            {
                by_host.entry(h).or_default().push(AttrValue::Ref(step.0));
            }
        }
        for (host, bars) in by_host {
            let Some(&step) = self.steps.get(&host) else { continue };
            for rel in self.ifc.referenced_by(step, "IfcRelAggregates")? {
                if matches!(self.ifc.attribute(rel, "Description")?, AttrValue::String(d) if d == TAG) {
                    self.ifc.remove(rel)?;
                }
            }
            let rel = self.ifc.create_rooted("IfcRelAggregates", &GlobalId::new())?;
            self.ifc.set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
            if let Some(h) = owner_history {
                self.ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(h.0))?;
            }
            self.ifc.set_attribute(rel, "RelatingObject", &AttrValue::Ref(step.0))?;
            self.ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(bars))?;
        }
        Ok(())
    }

    /// Removes the bars and sheets written under reinforcement systems, with their types.
    fn remove_rebar_parts(&mut self) -> Result<()> {
        for rel in self.ifc.entities_of_type("IfcRelAggregates")? {
            if !matches!(self.ifc.attribute(rel.id, "Description")?, AttrValue::String(d) if d == REBAR_PART_TAG) {
                continue;
            }
            if let AttrValue::List(parts) = self.ifc.attribute(rel.id, "RelatedObjects")? {
                for p in parts {
                    let AttrValue::Ref(p) = p else { continue };
                    let p = StepId(p);
                    for typing in self.ifc.referenced_by(p, "IfcRelDefinesByType")? {
                        self.ifc.remove(typing)?;
                    }
                    geometry::remove_breps(&mut self.ifc, p)?;
                    self.ifc.remove(p)?;
                }
            }
            self.ifc.remove(rel.id)?;
        }
        if self.ifc.schema() != "IFC2X3" {
            for t in self.ifc.entities_of_type("IfcReinforcingMeshType")? {
                if matches!(self.ifc.attribute(t.id, "Description")?, AttrValue::String(d) if d == REBAR_TYPE_TAG) {
                    for rel in self.ifc.referenced_by(t.id, "IfcRelDefinesByType")? {
                        self.ifc.remove(rel)?;
                    }
                    self.ifc.remove(t.id)?;
                }
            }
        }
        Ok(())
    }

    /// Sets a fabric sheet's attributes, and types it by a sheet type made for it (IFC4).
    fn write_mesh_attributes(&mut self, step: StepId, t: &reviera_core::rebar_systems::SheetType, da: f64, db: f64, owner_history: Option<StepId>) -> Result<()> {
        let sc = self.length_scale;
        let area = |d: f64| std::f64::consts::PI * d * d / 4.0 / sc / sc;
        let ifc4 = self.ifc.schema() != "IFC2X3";
        let attrs = [
            ("MeshLength", AttrValue::Real(t.length / sc)),
            ("MeshWidth", AttrValue::Real(t.width / sc)),
            ("LongitudinalBarNominalDiameter", AttrValue::Real(da / sc)),
            ("TransverseBarNominalDiameter", AttrValue::Real(db / sc)),
            ("LongitudinalBarCrossSectionArea", AttrValue::Real(area(da))),
            ("TransverseBarCrossSectionArea", AttrValue::Real(area(db))),
            ("LongitudinalBarSpacing", AttrValue::Real(t.major_spacing / sc)),
            ("TransverseBarSpacing", AttrValue::Real(t.minor_spacing / sc)),
        ];
        for (k, v) in &attrs {
            self.ifc.set_attribute(step, k, v)?;
        }
        if !ifc4 {
            return Ok(());
        }
        self.ifc.set_attribute(step, "PredefinedType", &AttrValue::Enum("NOTDEFINED".into()))?;
        let ty = self.ifc.create_rooted("IfcReinforcingMeshType", &GlobalId::new())?;
        self.ifc.set_attribute(ty, "Name", &AttrValue::String(t.name.clone()))?;
        self.ifc.set_attribute(ty, "Description", &AttrValue::String(REBAR_TYPE_TAG.into()))?;
        if let Some(h) = owner_history {
            self.ifc.set_attribute(ty, "OwnerHistory", &AttrValue::Ref(h.0))?;
        }
        self.ifc.set_attribute(ty, "PredefinedType", &AttrValue::Enum("NOTDEFINED".into()))?;
        for (k, v) in &attrs {
            self.ifc.set_attribute(ty, k, v)?;
        }
        let typing = self.ifc.create_rooted("IfcRelDefinesByType", &GlobalId::new())?;
        self.ifc.set_attribute(typing, "Description", &AttrValue::String(REBAR_TYPE_TAG.into()))?;
        if let Some(h) = owner_history {
            self.ifc.set_attribute(typing, "OwnerHistory", &AttrValue::Ref(h.0))?;
        }
        self.ifc.set_attribute(typing, "RelatedObjects", &AttrValue::List(vec![AttrValue::Ref(step.0)]))?;
        self.ifc.set_attribute(typing, "RelatingType", &AttrValue::Ref(ty.0))?;
        Ok(())
    }

    /// Writes a reinforcement system's bars (`IfcReinforcingBar` by group) or sheets
    /// (`IfcReinforcingMesh`) aggregated under it, or a single sheet's body and attributes.
    fn write_rebar_parts(
        &mut self,
        store: &reviera_core::Store,
        e: &reviera_core::Element,
        step: StepId,
        owner_history: Option<StepId>,
        contexts: geometry::Contexts,
    ) -> Result<()> {
        use reviera_core::rebar_systems::{self as rs, Part};
        let sc = self.length_scale;
        let poly = |(points, indices): reviera_core::bodies::Mesh| geometry::Polyhedron {
            points,
            faces: indices.chunks(3).map(|t| t.iter().map(|k| *k as usize).collect()).collect(),
        };
        let numbers = reviera_core::rebar_numbers::number_of(store, e.id());
        if let Some((t, da, db)) = rs::single_sheet_type(store, e) {
            if let Some(m) = rs::mesh(store, e) {
                geometry::write_breps(&mut self.ifc, step, geometry::BrepFrame::WORLD, &[poly(m)], sc, contexts)?;
            }
            if let Some(n) = numbers.first() {
                self.ifc.set_attribute(step, "Tag", &AttrValue::String(n.to_string()))?;
            }
            return self.write_mesh_attributes(step, &t, da, db, owner_history);
        }
        let mut parts = Vec::new();
        let mut sheet = 0;
        for part in rs::parts(store, e) {
            match part {
                Part::Bars { group, mesh } => {
                    if group.bars.is_empty() {
                        continue;
                    }
                    let p = self.ifc.create_rooted("IfcReinforcingBar", &GlobalId::new())?;
                    self.ifc.set_attribute(p, "Name", &AttrValue::String(format!("{} {}", group.name, group.bar_type)))?;
                    self.ifc.set_attribute(p, "Description", &AttrValue::String(REBAR_PART_TAG.into()))?;
                    if let Some(h) = owner_history {
                        self.ifc.set_attribute(p, "OwnerHistory", &AttrValue::Ref(h.0))?;
                    }
                    geometry::write_breps(&mut self.ifc, p, geometry::BrepFrame::WORLD, &[poly(mesh)], sc, contexts)?;
                    let area = std::f64::consts::PI * group.diameter * group.diameter / 4.0;
                    self.ifc.set_attribute(p, "NominalDiameter", &AttrValue::Real(group.diameter / sc))?;
                    self.ifc.set_attribute(p, "CrossSectionArea", &AttrValue::Real(area / sc / sc))?;
                    if let Some(l) = group.lengths.iter().copied().reduce(f64::max) {
                        self.ifc.set_attribute(p, "BarLength", &AttrValue::Real(l / sc))?;
                    }
                    if self.ifc.schema() != "IFC2X3" {
                        self.ifc.set_attribute(p, "PredefinedType", &AttrValue::Enum("MAIN".into()))?;
                    } else {
                        self.ifc.set_attribute(p, "BarRole", &AttrValue::Enum("MAIN".into()))?;
                    }
                    self.ifc.set_attribute(p, "BarSurface", &AttrValue::Enum("TEXTURED".into()))?;
                    parts.push(AttrValue::Ref(p.0));
                }
                Part::Sheet { sheet_type, major_diameter, minor_diameter, mesh, name } => {
                    let p = self.ifc.create_rooted("IfcReinforcingMesh", &GlobalId::new())?;
                    self.ifc.set_attribute(p, "Name", &AttrValue::String(name))?;
                    self.ifc.set_attribute(p, "Description", &AttrValue::String(REBAR_PART_TAG.into()))?;
                    if let Some(h) = owner_history {
                        self.ifc.set_attribute(p, "OwnerHistory", &AttrValue::Ref(h.0))?;
                    }
                    geometry::write_breps(&mut self.ifc, p, geometry::BrepFrame::WORLD, &[poly(mesh)], sc, contexts)?;
                    if let Some(n) = numbers.get(sheet) {
                        self.ifc.set_attribute(p, "Tag", &AttrValue::String(n.to_string()))?;
                    }
                    sheet += 1;
                    self.write_mesh_attributes(p, &sheet_type, major_diameter, minor_diameter, owner_history)?;
                    parts.push(AttrValue::Ref(p.0));
                }
            }
        }
        if parts.is_empty() {
            return Ok(());
        }
        let rel = self.ifc.create_rooted("IfcRelAggregates", &GlobalId::new())?;
        self.ifc.set_attribute(rel, "Description", &AttrValue::String(REBAR_PART_TAG.into()))?;
        if let Some(h) = owner_history {
            self.ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(h.0))?;
        }
        self.ifc.set_attribute(rel, "RelatingObject", &AttrValue::Ref(step.0))?;
        self.ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(parts))?;
        Ok(())
    }

    /// Writes steel cuts (STR-STL-008): each cut's opening gets its cutter solid and voids its
    /// target (`IfcRelVoidsElement`).
    fn write_cuts(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let cuts = reviera_core::steel_cuts::all(store);
        if cuts.is_empty() {
            return Ok(());
        }
        let bodies = reviera_core::bodies::all(store);
        let body_of = |id: ElementId| -> Option<Vec<reviera_core::bodies::Mesh>> {
            bodies.iter().find(|b| b.element == id).map(|b| b.parts.iter().map(|p| p.mesh.clone()).collect())
        };
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (cut, target, r) in cuts {
            let (Some(&opening), Some(&host)) = (self.steps.get(&cut), self.steps.get(&target)) else {
                continue;
            };
            let Some(tools) = reviera_core::steel_cuts::cutter(store, &r, &body_of) else {
                continue;
            };
            if tools.is_empty() {
                continue;
            }
            let solids: Vec<geometry::Polyhedron> = tools
                .into_iter()
                .map(|(points, indices)| geometry::Polyhedron {
                    points,
                    faces: indices.chunks(3).map(|t| t.iter().map(|i| *i as usize).collect()).collect(),
                })
                .collect();
            geometry::write_breps(&mut self.ifc, opening, geometry::BrepFrame::WORLD, &solids, self.length_scale, contexts)?;
            geometry::relate_one(
                &mut self.ifc,
                "IfcRelVoidsElement",
                "RelatingBuildingElement",
                host,
                "RelatedOpeningElement",
                opening,
                owner_history,
            )?;
        }
        Ok(())
    }

    /// Writes the holes fastener patterns make (STR-STL-006): an `IfcOpeningElement` (marked
    /// `rxd:hole`) voiding each pierced element, rewritten on every save.
    fn write_holes(&mut self, doc: &Document) -> Result<()> {
        const TAG: &str = "rxd:hole";
        for o in self.ifc.entities_of_type("IfcOpeningElement")? {
            if !matches!(self.ifc.attribute(o.id, "Description")?, AttrValue::String(d) if d == TAG) {
                continue;
            }
            for rel in self.ifc.referenced_by(o.id, "IfcRelVoidsElement")? {
                self.ifc.remove(rel)?;
            }
            geometry::remove_breps(&mut self.ifc, o.id)?;
            self.ifc.remove(o.id)?;
        }
        let store = doc.store();
        let holes: Vec<_> = store
            .iter()
            .flat_map(|e| reviera_core::steel_detail::holes(store, e))
            .collect();
        if holes.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        let ifc4 = self.ifc.schema() != "IFC2X3";
        for (element, a, b, d) in holes {
            let Some(&host) = self.steps.get(&element) else {
                continue;
            };
            let opening = self.ifc.create_rooted("IfcOpeningElement", &GlobalId::new())?;
            self.ifc.set_attribute(opening, "Name", &AttrValue::String("Hole".into()))?;
            self.ifc.set_attribute(opening, "Description", &AttrValue::String(TAG.into()))?;
            if ifc4 {
                self.ifc.set_attribute(opening, "PredefinedType", &AttrValue::Enum("OPENING".into()))?;
            }
            if let Some(h) = owner_history {
                self.ifc.set_attribute(opening, "OwnerHistory", &AttrValue::Ref(h.0))?;
            }
            // A little longer than the element is thick, so it cuts clean through.
            let axis = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt().max(1e-9);
            let ext = |p: [f64; 3], s: f64| [p[0] + axis[0] / l * s, p[1] + axis[1] / l * s, p[2] + axis[2] / l * s];
            let (points, indices) = reviera_core::steel_fab::cylinder(ext(a, -0.001), ext(b, 0.001), d / 2.0, 12);
            let faces: Vec<Vec<usize>> = indices.chunks(3).map(|t| t.iter().map(|i| *i as usize).collect()).collect();
            geometry::write_breps(
                &mut self.ifc,
                opening,
                geometry::BrepFrame::WORLD,
                &[geometry::Polyhedron { points, faces }],
                self.length_scale,
                contexts,
            )?;
            geometry::relate_one(
                &mut self.ifc,
                "IfcRelVoidsElement",
                "RelatingBuildingElement",
                host,
                "RelatedOpeningElement",
                opening,
                owner_history,
            )?;
        }
        Ok(())
    }

    /// Writes every beam system (STR-BSYS-001): the `IfcElementAssembly` contained in its level,
    /// aggregating its beams through an `IfcRelAggregates` marked `rxd:beam_system`.
    fn write_beam_systems(&mut self, doc: &Document) -> Result<()> {
        const TAG: &str = "rxd:beam_system";
        let store = doc.store();
        let owner_history = self.new_owner_history()?;
        for e in store.iter() {
            let Some(level) = reviera_core::beams::system_recipe(e)
                .map(|r| r.level)
                .or_else(|| reviera_core::beams::truss_recipe(e).map(|t| t.level))
            else {
                continue;
            };
            let Some(&step) = self.steps.get(&e.id()) else {
                continue;
            };
            for rel in self.ifc.referenced_by(step, "IfcRelAggregates")? {
                if matches!(self.ifc.attribute(rel, "Description")?, AttrValue::String(d) if d == TAG)
                {
                    self.ifc.remove(rel)?;
                }
            }
            let guid = e.guid().to_string();
            let parts: Vec<AttrValue> = store
                .iter()
                .filter(|b| {
                    reviera_core::beams::beam_recipe(b)
                        .and_then(|r| r.system)
                        .is_some_and(|s| s.system == guid)
                })
                .filter_map(|b| self.steps.get(&b.id()).map(|s| AttrValue::Ref(s.0)))
                .collect();
            if !parts.is_empty() {
                let rel = self
                    .ifc
                    .create_rooted("IfcRelAggregates", &GlobalId::new())?;
                self.ifc
                    .set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
                if let Some(oh) = owner_history {
                    self.ifc
                        .set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
                }
                self.ifc
                    .set_attribute(rel, "RelatingObject", &AttrValue::Ref(step.0))?;
                self.ifc
                    .set_attribute(rel, "RelatedObjects", &AttrValue::List(parts))?;
            }
            let storey = GlobalId::parse(&level)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        // Structural connections (STR-CONN-001): contained in their main member's level.
        for e in store.iter() {
            let Some(c) = reviera_core::connections::recipe(e) else {
                continue;
            };
            let Some(&step) = self.steps.get(&e.id()) else {
                continue;
            };
            let level = c
                .members
                .first()
                .and_then(|g| GlobalId::parse(g))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|m| store.get(m))
                .and_then(|m| {
                    reviera_core::beams::beam_recipe(m)
                        .map(|r| r.level)
                        .or_else(|| {
                            reviera_core::columns::column_recipe(m).map(|r| r.extent.base_level)
                        })
                });
            let storey = level
                .and_then(|g| GlobalId::parse(&g))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes every Reviera footing and wall foundation (STR-FND-001, 002): its body as a faceted
    /// B-rep, contained in its level.
    fn write_foundations(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = reviera_core::foundations::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, g) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let (points, indices) = g.mesh();
            let faces: Vec<Vec<usize>> = indices
                .chunks(3)
                .map(|t| t.iter().map(|i| *i as usize).collect())
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &[geometry::Polyhedron { points, faces }],
                self.length_scale,
                contexts,
            )?;
            // A pile cap's piles are aggregated by it, not contained.
            let e = store.get(*id).expect("listed");
            if reviera_core::foundations::pile_recipe(e).is_some_and(|r| r.cap.is_some()) {
                continue;
            }
            let storey = reviera_core::foundations::level_of(store, e)
                .and_then(|g| GlobalId::parse(&g))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        // Each pile cap aggregates its piles (STR-FND-004).
        const TAG: &str = "rxd:pile_cap";
        for (id, _) in &all {
            let e = store.get(*id).expect("listed");
            if reviera_core::foundations::footing_recipe(e).is_none() {
                continue;
            }
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            for rel in self.ifc.referenced_by(step, "IfcRelAggregates")? {
                if matches!(self.ifc.attribute(rel, "Description")?, AttrValue::String(d) if d == TAG)
                {
                    self.ifc.remove(rel)?;
                }
            }
            let guid = e.guid().to_string();
            let parts: Vec<AttrValue> = store
                .iter()
                .filter(|p| {
                    reviera_core::foundations::pile_recipe(p)
                        .and_then(|r| r.cap)
                        .is_some_and(|c| c.cap == guid)
                })
                .filter_map(|p| self.steps.get(&p.id()).map(|s| AttrValue::Ref(s.0)))
                .collect();
            if parts.is_empty() {
                continue;
            }
            let rel = self
                .ifc
                .create_rooted("IfcRelAggregates", &GlobalId::new())?;
            self.ifc
                .set_attribute(rel, "Description", &AttrValue::String(TAG.into()))?;
            if let Some(oh) = owner_history {
                self.ifc
                    .set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
            }
            self.ifc
                .set_attribute(rel, "RelatingObject", &AttrValue::Ref(step.0))?;
            self.ifc
                .set_attribute(rel, "RelatedObjects", &AttrValue::List(parts))?;
        }
        Ok(())
    }

    /// Writes every Reviera ramp (ARC-RAMP-001): its flights and landings as faceted B-reps,
    /// contained in its base level.
    fn write_ramps(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = reviera_core::ramps::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, g) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let solids: Vec<geometry::Polyhedron> = g
                .pieces
                .iter()
                .map(|p| {
                    let (points, _) = g.mesh(p);
                    geometry::Polyhedron {
                        points,
                        faces: vec![
                            vec![3, 2, 1, 0],
                            vec![4, 5, 6, 7],
                            vec![0, 1, 5, 4],
                            vec![1, 2, 6, 5],
                            vec![2, 3, 7, 6],
                            vec![3, 0, 4, 7],
                        ],
                    }
                })
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &solids,
                self.length_scale,
                contexts,
            )?;
            let storey = reviera_core::ramps::ramp_recipe(store.get(*id).expect("listed"))
                .and_then(|r| GlobalId::parse(&r.extent.base_level))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes every slab edge (ARC-SLED-001): its swept body as a faceted B-rep, contained in its
    /// floor's level.
    fn write_slab_edges(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = reviera_core::slab_edges::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, g) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let (points, indices) = &g.mesh;
            let solid = geometry::Polyhedron {
                points: points.clone(),
                faces: indices
                    .chunks(3)
                    .map(|t| t.iter().map(|i| *i as usize).collect())
                    .collect(),
            };
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &[solid],
                self.length_scale,
                contexts,
            )?;
            let storey = reviera_core::slab_edges::recipe(store.get(*id).expect("listed"))
                .and_then(|r| GlobalId::parse(&r.host))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|f| store.get(f))
                .and_then(reviera_core::floors::floor_recipe)
                .and_then(|f| GlobalId::parse(&f.level))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes every wall sweep and reveal (ARC-WALL-010) as a faceted B-rep, contained in its
    /// level.
    fn write_wall_sweeps(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = reviera_core::wall_sweeps::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, _, meshes) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let solids: Vec<geometry::Polyhedron> = meshes
                .iter()
                .map(|(points, indices)| geometry::Polyhedron {
                    points: points.clone(),
                    faces: indices
                        .chunks(3)
                        .map(|t| t.iter().map(|i| *i as usize).collect())
                        .collect(),
                })
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &solids,
                self.length_scale,
                contexts,
            )?;
            let storey = reviera_core::wall_sweeps::recipe(store.get(*id).expect("listed"))
                .and_then(|r| GlobalId::parse(&r.level))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes every model text (ARC-MTXT-001): its letters as faceted B-reps, contained in its
    /// work plane's level when it is on one.
    fn write_model_texts(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        // In-place elements and their voids are written the same way (ARC-INPL-001).
        let mut all = reviera_core::model_text::all(store);
        all.extend(
            reviera_core::in_place::all(store)
                .into_iter()
                .map(|(id, _, meshes)| (id, meshes)),
        );
        // Toposolids and their excavations (ARC-TOPO-001).
        all.extend(
            reviera_core::toposolids::all(store)
                .into_iter()
                .map(|(id, _, mesh)| (id, vec![mesh])),
        );
        // Masses and curtain systems by face (ARC-MASS-001, ARC-CW-007).
        all.extend(reviera_core::masses::all(store));
        all.extend(store.iter().filter_map(|e| {
            let r = reviera_core::masses::system_recipe(e)?;
            Some((
                e.id(),
                reviera_core::masses::system_parts(&r)
                    .parts
                    .into_iter()
                    .map(|p| p.mesh)
                    .collect(),
            ))
        }));
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, meshes) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let solids: Vec<geometry::Polyhedron> = meshes
                .iter()
                .map(|(points, indices)| geometry::Polyhedron {
                    points: points.clone(),
                    faces: indices
                        .chunks(3)
                        .map(|t| t.iter().map(|i| *i as usize).collect())
                        .collect(),
                })
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &solids,
                self.length_scale,
                contexts,
            )?;
            let e = store.get(*id).expect("listed");
            let plane = reviera_core::model_text::recipe(e)
                .map(|r| r.plane)
                .or_else(|| {
                    reviera_core::in_place::recipe(e)
                        .and_then(|r| r.forms.first().map(|f| f.plane.clone()))
                });
            let storey = plane
                .and_then(|plane| match plane {
                    reviera_core::work_planes::WorkPlaneRef::Level { level, .. } => Some(level),
                    _ => None,
                })
                .and_then(|g| GlobalId::parse(&g))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes every part (ARC-PART-001) that is drawn as a faceted B-rep, aggregated by its
    /// original element; excluded parts and divided or merged ones have no body.
    fn write_parts(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        if !store
            .iter()
            .any(|e| reviera_core::parts::recipe(e).is_some())
        {
            return Ok(());
        }
        let bodies = reviera_core::bodies::all(store);
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for e in store.iter() {
            let Some(r) = reviera_core::parts::recipe(e) else {
                continue;
            };
            let Some(&step) = self.steps.get(&e.id()) else {
                continue;
            };
            geometry::remove_breps(&mut self.ifc, step)?;
            let Some(b) = bodies.iter().find(|b| b.element == e.id()) else {
                continue;
            };
            let solids: Vec<geometry::Polyhedron> = b
                .parts
                .iter()
                .map(|p| geometry::Polyhedron {
                    points: p.mesh.0.clone(),
                    faces: p
                        .mesh
                        .1
                        .chunks(3)
                        .map(|t| t.iter().map(|i| *i as usize).collect())
                        .collect(),
                })
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &solids,
                self.length_scale,
                contexts,
            )?;
            let source = GlobalId::parse(&r.source)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|s| self.steps.get(&s).copied());
            if let Some(source) = source {
                geometry::aggregate(&mut self.ifc, step, source, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes every skylight: its glass and frame as faceted B-reps, contained in its roof's level.
    fn write_skylights(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = reviera_core::skylights::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, g) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let solids: Vec<geometry::Polyhedron> = g
                .parts
                .iter()
                .map(|((points, idx), _)| geometry::Polyhedron {
                    points: points.clone(),
                    faces: idx
                        .chunks(3)
                        .map(|t| t.iter().map(|i| *i as usize).collect())
                        .collect(),
                })
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &solids,
                self.length_scale,
                contexts,
            )?;
            let storey = reviera_core::skylights::skylight_recipe(store.get(*id).expect("listed"))
                .and_then(|s| GlobalId::parse(&s.roof))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|r| roofs::roof_recipe(store.get(r)?))
                .and_then(|r| GlobalId::parse(&r.level))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes every Reviera stair (ARC-STAIR-001): one faceted B-rep per run and landing,
    /// contained in its base level.
    fn write_stairs(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = stairs::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, g) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let solids: Vec<geometry::Polyhedron> = g
                .runs
                .iter()
                .chain(&g.landings)
                .map(|(points, _)| {
                    let n = points.len() / 2;
                    let mut faces = vec![(0..n).rev().collect::<Vec<_>>(), (n..2 * n).collect()];
                    faces.extend((0..n).map(|i| vec![i, (i + 1) % n, n + (i + 1) % n, n + i]));
                    geometry::Polyhedron {
                        points: points.clone(),
                        faces,
                    }
                })
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &solids,
                self.length_scale,
                contexts,
            )?;
            let storey = stairs::stair_recipe(store.get(*id).expect("listed"))
                .and_then(|r| GlobalId::parse(&r.base_level))
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes the bodies of boolean targets (ARC-MOD-028) as their shaped result: faceted
    /// B-reps (a wall's in its own frame, so its openings keep their placement), and each
    /// relation as an `IfcRelInterferesElements` (IFC4: InterferenceType `Subtract`,
    /// `Intersect` or `Union`).
    fn write_csg(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let relations = reviera_core::csg::resolved(store);
        if relations.is_empty() {
            return Ok(());
        }
        let mut bodies = reviera_core::bodies::all(store);
        reviera_geom::csg::apply(&Default::default(), store, &mut bodies);
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        let mut targets: Vec<ElementId> = relations.iter().map(|r| r.0).collect();
        targets.sort();
        targets.dedup();
        for target in targets {
            let (Some(&step), Some(body), Some(e)) = (
                self.steps.get(&target),
                bodies.iter().find(|b| b.element == target),
                store.get(target),
            ) else {
                continue;
            };
            // The frame: a wall's own (origin, x along it, y across it), else the world's.
            let wall = walls::wall_recipe(e).and_then(|_| walls::regenerate(store, e).ok());
            let (frame, axes) = match wall {
                Some(g) => (
                    geometry::BrepFrame {
                        origin: [g.start[0], g.start[1], g.base_z],
                        x_dir: g.direction(),
                    },
                    (
                        [g.start[0], g.start[1], g.base_z],
                        g.direction(),
                        g.normal(),
                    ),
                ),
                None => (
                    geometry::BrepFrame::WORLD,
                    ([0.0; 3], [1.0, 0.0], [0.0, 1.0]),
                ),
            };
            let local = |p: &[f64; 3]| {
                let (o, u, n) = axes;
                let d = [p[0] - o[0], p[1] - o[1]];
                [
                    d[0] * u[0] + d[1] * u[1],
                    d[0] * n[0] + d[1] * n[1],
                    p[2] - o[2],
                ]
            };
            let solids: Vec<geometry::Polyhedron> = body
                .parts
                .iter()
                .map(|p| geometry::Polyhedron {
                    points: p.mesh.0.iter().map(local).collect(),
                    faces: p
                        .mesh
                        .1
                        .chunks(3)
                        .map(|t| t.iter().map(|i| *i as usize).collect())
                        .collect(),
                })
                .collect();
            if walls::wall_recipe(e).is_some() {
                geometry::remove_wall_shape(&mut self.ifc, step)?;
            }
            if floors::slab_recipe(e).is_some() {
                geometry::remove_slab(&mut self.ifc, step)?;
            }
            geometry::remove_breps(&mut self.ifc, step)?;
            geometry::write_breps(
                &mut self.ifc,
                step,
                frame,
                &solids,
                self.length_scale,
                contexts,
            )?;
        }
        if self.ifc.schema() == "IFC2X3" {
            return Ok(());
        }
        for (target, tool, op, _) in relations {
            let (Some(&a), Some(&b)) = (self.steps.get(&target), self.steps.get(&tool)) else {
                continue;
            };
            let kind = match op {
                reviera_core::csg::CsgOp::Subtract => "Subtract",
                reviera_core::csg::CsgOp::Intersect => "Intersect",
                reviera_core::csg::CsgOp::Union => "Union",
            };
            // One relation per pair: a tool may shape several targets.
            let mut existing = None;
            for rel in self.ifc.referenced_by(b, "IfcRelInterferesElements")? {
                if self.ifc.attribute(rel, "RelatedElement")? == AttrValue::Ref(b.0)
                    && self.ifc.attribute(rel, "RelatingElement")? == AttrValue::Ref(a.0)
                {
                    existing = Some(rel);
                    break;
                }
            }
            let rel = match existing {
                Some(rel) => rel,
                None => {
                    let rel = self
                        .ifc
                        .create_rooted("IfcRelInterferesElements", &GlobalId::new())?;
                    if let Some(oh) = owner_history {
                        self.ifc
                            .set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
                    }
                    self.ifc
                        .set_attribute(rel, "RelatingElement", &AttrValue::Ref(a.0))?;
                    self.ifc
                        .set_attribute(rel, "RelatedElement", &AttrValue::Ref(b.0))?;
                    rel
                }
            };
            self.ifc
                .set_attribute(rel, "InterferenceType", &AttrValue::String(kind.into()))?;
            self.ifc
                .set_attribute(rel, "ImpliedOrder", &AttrValue::Bool(true))?;
        }
        Ok(())
    }

    /// Writes every Reviera railing (ARC-RAIL-001): one faceted B-rep per rail, post and
    /// baluster, contained in its level (a free railing's, or its host's); and each gate
    /// (ARC-RAIL-003) as its leaves, in its railing's level.
    fn write_railings(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let mut all: Vec<(ElementId, Vec<railings::Mesh>, ElementId)> = railings::all(store)
            .into_iter()
            .map(|(id, g)| (id, g.parts, id))
            .collect();
        for (id, parts) in railings::all_gates(store) {
            let railing = store
                .get(id)
                .and_then(railings::gate_of)
                .and_then(|(r, _)| store.find_by_guid(&GlobalId::parse(&r)?));
            if let Some(railing) = railing {
                all.push((id, parts, railing));
            }
        }
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for (id, parts, railing) in &all {
            let Some(&step) = self.steps.get(id) else {
                continue;
            };
            let solids: Vec<geometry::Polyhedron> = parts
                .iter()
                .map(|(points, _)| {
                    let n = points.len() / 2;
                    let mut faces = vec![(0..n).rev().collect::<Vec<_>>(), (n..2 * n).collect()];
                    faces.extend((0..n).map(|i| vec![i, (i + 1) % n, n + (i + 1) % n, n + i]));
                    geometry::Polyhedron {
                        points: points.clone(),
                        faces,
                    }
                })
                .collect();
            geometry::write_breps(
                &mut self.ifc,
                step,
                geometry::BrepFrame::WORLD,
                &solids,
                self.length_scale,
                contexts,
            )?;
            let level = railings::railing_recipe(store.get(*railing).expect("listed")).and_then(
                |r| match r.path {
                    railings::RailPath::Free { level, .. } => GlobalId::parse(&level),
                    railings::RailPath::Host { host, .. } => {
                        let h = store.get(store.find_by_guid(&GlobalId::parse(&host)?)?)?;
                        let level = reviera_core::floors::slab_recipe(h)
                            .map(|(_, r)| r.level)
                            .or_else(|| roofs::roof_recipe(h).map(|r| r.level))?;
                        GlobalId::parse(&level)
                    }
                    railings::RailPath::Stair { stair, .. } => {
                        let h = store.get(store.find_by_guid(&GlobalId::parse(&stair)?)?)?;
                        GlobalId::parse(&stairs::stair_recipe(h)?.base_level)
                    }
                },
            );
            let storey = level
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::contain(&mut self.ifc, step, storey, owner_history)?;
            }
            // A gate's size and operation, as a door's.
            if let Some((op, width, height)) =
                railings::gate_operation(store, store.get(*id).expect("listed"))
            {
                let mut attrs = vec![
                    ("OverallWidth", AttrValue::Real(width / self.length_scale)),
                    ("OverallHeight", AttrValue::Real(height / self.length_scale)),
                ];
                if self.ifc.schema() != "IFC2X3" {
                    attrs.push(("OperationType", AttrValue::Enum(op.into())));
                }
                for (attr, v) in attrs {
                    if self.ifc.attribute(step, attr)? != v {
                        self.ifc.set_attribute(step, attr, &v)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Writes every Reviera room (ARC-ROOM-001) as an `IfcSpace` aggregated under its level:
    /// enclosed rooms get a body from their base to their upper limit; others have none.
    fn write_rooms(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let all = rooms::all(store);
        if all.is_empty() {
            return Ok(());
        }
        let contexts = geometry::Contexts::find(&self.ifc)?;
        let owner_history = self.new_owner_history()?;
        for room in &all {
            let Some(&step) = self.steps.get(&room.id) else {
                continue;
            };
            let g = &room.geometry;
            match &g.region {
                Some(region) => {
                    let outer = region.outer_ring();
                    let holes = region.hole_rings();
                    let body = geometry::SlabBody {
                        rings: vec![(outer.as_slice(), holes.iter().map(Vec::as_slice).collect())],
                        top: g.z_top,
                        depth: g.unbounded_height(),
                    };
                    geometry::write_slab(&mut self.ifc, step, &body, self.length_scale, contexts)?;
                }
                None => geometry::remove_slab(&mut self.ifc, step)?,
            }
            let storey = GlobalId::parse(&room.level)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| self.steps.get(&l).copied());
            if let Some(storey) = storey {
                geometry::aggregate(&mut self.ifc, step, storey, owner_history)?;
            }
        }
        Ok(())
    }

    /// Writes the axes of every Reviera grid system, contained in the lowest level.
    fn write_grids(&mut self, doc: &Document) -> Result<()> {
        let store = doc.store();
        let lowest = levels::list(store)
            .first()
            .and_then(|l| self.steps.get(&l.id).copied());
        let owner_history = self.new_owner_history()?;
        let all = core_grids::list(store);
        for system in store.iter().filter(|e| core_grids::is_system(e)) {
            let Some(&step) = self.steps.get(&system.id()) else {
                continue;
            };
            let guid = system.guid().to_string();
            let axes: Vec<geometry::GridAxis<'_>> = all
                .iter()
                .filter(|g| g.system == guid)
                .map(|g| geometry::GridAxis {
                    tag: &g.name,
                    start: g.curve.start,
                    end: g.curve.end,
                })
                .collect();
            geometry::write_grid(
                &mut self.ifc,
                step,
                &axes,
                self.length_scale,
                lowest,
                owner_history,
            )?;
        }
        Ok(())
    }

    /// Owner history for new rooted entities: required in IFC2X3, left unset in IFC4.
    fn new_owner_history(&self) -> Result<Option<StepId>> {
        if self.ifc.schema() != "IFC2X3" {
            return Ok(None);
        }
        self.owner_history.map(Some).ok_or_else(|| {
            unsupported("IFC2X3 files without an IfcOwnerHistory cannot be extended")
        })
    }

    /// Writes an RXD payload into a property set on `target`. An unchanged payload is left as it is,
    /// so re-saving an unchanged project gives identical bytes (Ch 2a §11).
    fn upsert_payload(
        &mut self,
        existing: Option<Pset>,
        name: &str,
        target: StepId,
        target_is_type: bool,
        kind: &str,
        value: &Value,
    ) -> Result<Pset> {
        let encoded = encode(value);
        if let Some(p) = &existing
            && p.targets.len() == 1
            && p.string(prop::SHA256) == Some(encoded.sha256.as_str())
            && p.string(prop::KIND) == Some(kind)
        {
            return Ok(p.clone());
        }
        let rev = existing
            .as_ref()
            .and_then(|p| p.int(prop::REV))
            .unwrap_or(0)
            + 1;
        let props = psets::payload_props(kind, rev, &encoded);
        let owner_history = self.new_owner_history()?;
        let mut w = PsetWriter {
            ifc: &mut self.ifc,
            owner_history,
        };
        let id = match existing {
            Some(p) if p.targets.len() == 1 => {
                w.update(&p, &props)?;
                p.id
            }
            // Shared with other objects (for example copied by another tool): give `target` its own.
            Some(p) => {
                w.detach(&p, target)?;
                w.create(name, target, target_is_type, &props)?
            }
            None => w.create(name, target, target_is_type, &props)?,
        };
        psets::read_pset(&self.ifc, id)
    }

    fn write_manifest(&mut self, project: StepId, revision: i64) -> Result<()> {
        let data_id = self
            .data_id
            .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone();
        let index: Vec<Value> = self
            .doc_psets
            .iter()
            .map(|(path, p)| {
                json!({
                    "path": path,
                    "kind": p.string(prop::KIND),
                    "schema_version": p.string(prop::SCHEMA_VERSION),
                    "sha256": p.string(prop::SHA256),
                })
            })
            .collect();
        let props = vec![
            (manifest::FORMAT.to_owned(), label(reviera_rxd::FORMAT)),
            (
                manifest::FORMAT_VERSION.to_owned(),
                label(reviera_rxd::FORMAT_VERSION),
            ),
            (manifest::DATA_ID.to_owned(), identifier(&data_id)),
            (manifest::DATA_REVISION.to_owned(), integer(revision)),
            (
                manifest::WRITER.to_owned(),
                label(&format!("Reviera {}", env!("CARGO_PKG_VERSION"))),
            ),
            (manifest::CARRIER.to_owned(), label("embedded")),
            (
                manifest::INDEX.to_owned(),
                text(&canonical_json(&Value::Array(index))),
            ),
        ];
        let owner_history = self.new_owner_history()?;
        let mut w = PsetWriter {
            ifc: &mut self.ifc,
            owner_history,
        };
        let id = match self.manifest.take() {
            Some(m) => {
                w.update(&m, &props)?;
                m.id
            }
            None => w.create(MANIFEST_PSET, project, false, &props)?,
        };
        self.manifest = Some(psets::read_pset(&self.ifc, id)?);
        Ok(())
    }

    fn remove_element_pset(&mut self, pset: &Pset, target: StepId) -> Result<()> {
        if pset.targets.len() > 1 {
            let owner_history = self.new_owner_history()?;
            return PsetWriter {
                ifc: &mut self.ifc,
                owner_history,
            }
            .detach(pset, target);
        }
        for (_, link) in &pset.targets {
            if let psets::Link::Rel(rel) = link {
                self.ifc.remove(*rel)?;
            }
        }
        for (prop_id, _) in pset.props.values() {
            self.ifc.remove(*prop_id)?;
        }
        self.ifc.remove(pset.id)?;
        Ok(())
    }

    /// Writes to a temporary file in the same folder, syncs it, then renames it over `path`.
    fn write_atomic(&self, path: &Path) -> Result<()> {
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let file_name = path
            .file_name()
            .ok_or_else(|| unsupported(format!("{} is not a file path", path.display())))?;
        let tmp = dir.join(format!(
            ".{}.{}.tmp",
            file_name.to_string_lossy(),
            std::process::id()
        ));
        let result = (|| -> Result<()> {
            self.ifc.write(&tmp)?;
            // Flushing needs write access on Windows (FlushFileBuffers).
            fs::OpenOptions::new().write(true).open(&tmp)?.sync_all()?;
            fs::rename(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result?;
        if let Ok(d) = fs::File::open(dir) {
            // Persist the rename; not supported on every platform, so failure is not an error.
            let _ = d.sync_all();
        }
        Ok(())
    }
}

/// An RXD-only element as stored in its document: `{iid, guid, name, params}`.
struct RxdRecord {
    iid: u64,
    guid: GlobalId,
    name: Option<String>,
    params: BTreeMap<String, Value>,
}

impl RxdRecord {
    fn of(e: &reviera_core::Element) -> Self {
        Self {
            iid: e.id().0,
            guid: e.guid().clone(),
            name: e.name.clone(),
            params: e.params.clone(),
        }
    }

    fn from_value(v: &Value) -> Option<Self> {
        Some(Self {
            iid: v.get("iid")?.as_u64()?,
            guid: GlobalId::parse(v.get("guid")?.as_str()?)?,
            name: v.get("name").and_then(Value::as_str).map(str::to_owned),
            params: serde_json::from_value(v.get("params")?.clone()).ok()?,
        })
    }

    fn to_value(&self) -> Value {
        json!({ "iid": self.iid, "guid": self.guid.as_str(), "name": self.name, "params": self.params })
    }

    fn into_element(self, path: &str) -> NewElement {
        NewElement {
            class: format!("rxd:{path}"),
            name: self.name,
            params: self.params,
            guid: Some(self.guid),
        }
    }
}

/// The Description that marks a bar type synthesized for rebar sets (STR-RBR-003).
const REBAR_TYPE_TAG: &str = "rxd:rebar_type";
/// Bars and sheets written under area, path and fabric systems (STR-RBR-010 … 012).
const REBAR_PART_TAG: &str = "rxd:rebar_part";

/// The built-in project template.
/// The Description that marks an opening a shaft derived for one of its hosts.
const DERIVED_OPENING: &str = "rxd:shaft";

/// Whether an entity is one Reviera derives from another element: a shaft's opening or a
/// curtain wall's part.
fn is_derived(ifc: &IfcModel, id: StepId) -> bool {
    matches!(ifc.attribute(id, "Description"), Ok(AttrValue::String(d))
        if d.starts_with(DERIVED_OPENING)
            || d == crate::curtains::PART_TAG
            || d == crate::path_arrays::PART_TAG
            || d == crate::stacked_walls::PART_TAG
            || d == REBAR_TYPE_TAG
            || d == REBAR_PART_TAG)
}

/// The lowest and highest points of a floor's, ceiling's or roof's body.
fn host_span(store: &reviera_core::Store, host: ElementId) -> Option<(f64, f64)> {
    let e = store.get(host)?;
    if roofs::roof_recipe(e).is_some() {
        let g = roofs::regenerate(store, e).ok()?;
        let (lo, hi) = reviera_core::slab_openings::roof_span(&g);
        return (lo <= hi).then_some((lo, hi));
    }
    let f = floors::regenerate(store, e).ok()?;
    Some((f.top - f.thickness, f.top))
}

fn template(schema: Schema) -> Result<IfcModel> {
    let mut m = IfcModel::new(schema)?;
    let s = |v: &str| AttrValue::String(v.into());
    let e = |v: &str| AttrValue::Enum(v.into());
    let r = |id: StepId| AttrValue::Ref(id.0);
    let reals = |v: &[f64]| AttrValue::List(v.iter().copied().map(AttrValue::Real).collect());
    let create = |m: &mut IfcModel, class: &str, attrs: &[(&str, AttrValue)]| -> Result<StepId> {
        let id = m.create(class)?;
        for (name, value) in attrs {
            m.set_attribute(id, name, value)?;
        }
        Ok(id)
    };
    let mut units = Vec::new();
    for (unit_type, name) in [
        ("LENGTHUNIT", "METRE"),
        ("AREAUNIT", "SQUARE_METRE"),
        ("VOLUMEUNIT", "CUBIC_METRE"),
        ("PLANEANGLEUNIT", "RADIAN"),
    ] {
        units.push(r(create(
            &mut m,
            "IfcSIUnit",
            &[("UnitType", e(unit_type)), ("Name", e(name))],
        )?));
    }
    let units = create(
        &mut m,
        "IfcUnitAssignment",
        &[("Units", AttrValue::List(units))],
    )?;
    let origin = create(
        &mut m,
        "IfcCartesianPoint",
        &[("Coordinates", reals(&[0.0, 0.0, 0.0]))],
    )?;
    let axes = create(&mut m, "IfcAxis2Placement3D", &[("Location", r(origin))])?;
    let context = create(
        &mut m,
        "IfcGeometricRepresentationContext",
        &[
            ("ContextType", s("Model")),
            ("CoordinateSpaceDimension", AttrValue::Int(3)),
            ("Precision", AttrValue::Real(1e-5)),
            ("WorldCoordinateSystem", r(axes)),
        ],
    )?;
    let project = m.create_rooted("IfcProject", &GlobalId::new())?;
    m.set_attribute(project, "Name", &s("Project"))?;
    m.set_attribute(
        project,
        "RepresentationContexts",
        &AttrValue::List(vec![r(context)]),
    )?;
    m.set_attribute(project, "UnitsInContext", &r(units))?;
    let mut parent = project;
    for (class, name) in [
        ("IfcSite", "Site"),
        ("IfcBuilding", "Building"),
        ("IfcBuildingStorey", "Level 1"),
    ] {
        let child = m.create_rooted(class, &GlobalId::new())?;
        m.set_attribute(child, "Name", &s(name))?;
        m.set_attribute(child, "CompositionType", &e("ELEMENT"))?;
        if class == "IfcBuildingStorey" {
            m.set_attribute(child, "Elevation", &AttrValue::Real(0.0))?;
        }
        let rel = m.create_rooted("IfcRelAggregates", &GlobalId::new())?;
        m.set_attribute(rel, "RelatingObject", &r(parent))?;
        m.set_attribute(rel, "RelatedObjects", &AttrValue::List(vec![r(child)]))?;
        parent = child;
    }
    Ok(m)
}

fn millis(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Unsupported(e.to_string())
    }
}
