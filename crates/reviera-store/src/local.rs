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

use reviera_core::{Document, ElementId, GlobalId, ModelVersion, NewElement};
use reviera_ifc::{AttrValue, IfcModel, Schema, StepId};
use reviera_rxd::{
    DOC_PSET_PREFIX, DecodeLimits, ELEMENT_PSET, ElementData, IdentityDoc, MANIFEST_PSET,
    TYPE_PSET, canonical_json, encode, manifest, prop,
};
use serde_json::{Value, json};

use crate::psets::{self, Pset, PsetWriter, identifier, integer, label, text, unsupported};
use crate::{Result, StoreError};

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
    /// Document version that the IFC population reflects.
    synced: ModelVersion,
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

        // Internal IDs: persisted ones first; the rest in STEP order (DM-ID-001).
        let mut used = HashSet::new();
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
            if let Some((data, pset)) = data_by_step.remove(&e.id) {
                element_data.insert(id, data);
                element_psets.insert(id, pset);
            }
            steps.insert(id, e.id);
            elements.push((
                id,
                NewElement {
                    class: e.class,
                    name: e.name,
                    params: BTreeMap::new(),
                    guid: Some(guid),
                },
            ));
        }
        report.elements = elements.len();
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
            forced_changed: BTreeSet::new(),
            forced_deleted: BTreeSet::new(),
            synced: doc.version(),
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

        for id in &deleted {
            dirty.remove(id);
            self.element_data.remove(id);
            let Some(step) = self.steps.remove(id) else {
                continue;
            };
            if let Some(pset) = self.element_psets.remove(id) {
                self.remove_element_pset(&pset, step)?;
            }
            self.types.remove(&step);
            self.ifc.remove(step)?;
            report.removed += 1;
        }

        for id in &changed {
            let e = doc.store().get(*id).expect("changed elements exist");
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
                            "element {id} ({}) has no IFC entity; RXD-only elements are not stored yet",
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
            let name = e.name.clone().map_or(AttrValue::Null, AttrValue::String);
            if self.ifc.attribute(step, "Name")? != name {
                self.ifc.set_attribute(step, "Name", &name)?;
            }
            dirty.insert(*id);
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
            fs::File::open(&tmp)?.sync_all()?;
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

/// The built-in project template.
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
