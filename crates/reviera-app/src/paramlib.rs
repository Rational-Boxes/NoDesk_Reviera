// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Parameter library commands (spec Ch 2 §2.9.7, DM-SPAR-001): the libraries open in this
//! session, editing their files, legacy import, IFC templates, and adding a definition to the
//! project.
//!
//! - Library files are edited directly: each change is written at once, atomically (a temporary
//!   file renamed over the old one), with sorted keys and 2-space indentation. These edits are
//!   not model changes and are not undone with the project.
//! - Opening a library compares it with the copy cached when it was last opened (in the
//!   `paramlib-cache` folder beside the untitled-projects folder): a definition removed or a data
//!   type changed is refused. A definition whose GUID the open project already has with another
//!   data type is refused too, naming both.
//! - Adding a definition to the project embeds a full copy with the library's id, name and
//!   version (`param.project.add`), so the project never needs the file. When the project has the
//!   GUID already, `on_conflict` keeps the project's or updates it from the library.
//!
//! Not built yet: opening libraries from a URL or git repository, remembering the active
//! libraries between sessions, exporting to the legacy text format.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use reviera_core::paramlib::{self, Definition, Group, IfcHome, Library};
use reviera_core::{Caller, CommandMeta, Effect, Error, GlobalId, HostCommand, Result};
use reviera_ifc::{AttrValue, IfcModel, Schema};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A library open in the session.
#[derive(Debug, Clone)]
pub struct OpenLibrary {
    pub path: PathBuf,
    pub lib: Library,
    pub read_only: bool,
}

const fn meta(name: &'static str, summary: &'static str, description: &'static str, effects: &'static [Effect]) -> CommandMeta {
    CommandMeta { name, version: 1, summary, description, effects, transaction_name: "" }
}

fn cache_dir(app: &App) -> PathBuf {
    app.config.untitled_dir.with_file_name("paramlib-cache")
}

fn write_file(path: &Path, lib: &Library) -> Result<()> {
    paramlib::check(lib)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, paramlib::write(lib)).map_err(|e| bad("paramlib.write", format!("{}: {e}", path.display())))?;
    std::fs::rename(&tmp, path).map_err(|e| bad("paramlib.write", format!("{}: {e}", path.display())))
}

impl App {
    /// The libraries open in this session.
    pub fn libraries(&self) -> Vec<OpenLibrary> {
        self.libraries.values().cloned().collect()
    }

    fn library_mut(&mut self, id: &str) -> Result<&mut OpenLibrary> {
        let l = self.libraries.get_mut(id).ok_or_else(|| bad("paramlib.not_open", format!("no open library {id}")))?;
        if l.read_only {
            return Err(bad("paramlib.read_only", format!("{} is read-only", l.lib.library.name)));
        }
        Ok(l)
    }

    fn save_library(&mut self, id: &str) -> Result<()> {
        let l = self.libraries.get(id).expect("open");
        write_file(&l.path, &l.lib)?;
        let dir = cache_dir(self);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{id}.json")), paramlib::write(&l.lib));
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibrarySummary {
    pub library_id: String,
    pub name: String,
    pub version: String,
    pub path: String,
    pub definitions: usize,
    pub read_only: bool,
}

fn summary(l: &OpenLibrary) -> LibrarySummary {
    LibrarySummary {
        library_id: l.lib.library.id.clone(),
        name: l.lib.library.name.clone(),
        version: l.lib.library.version.clone(),
        path: l.path.display().to_string(),
        definitions: l.lib.definitions.len(),
        read_only: l.read_only,
    }
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    /// A `.rplib.json` file.
    pub path_or_url: String,
}

pub(crate) struct ParamlibOpen;

impl HostCommand for ParamlibOpen {
    type Host = App;
    type Params = OpenParams;
    type Output = LibrarySummary;
    const META: CommandMeta = meta(
        "paramlib.open",
        "Open a parameter library",
        "Opens a Reviera parameter library (`.rplib.json`, DM-SPAR-001): checked against the published JSON Schema and the library rules, and against the copy cached when it was last opened (a removed definition or a changed data type is refused) and the open project's shared parameters (the same GUID with another data type is refused, naming both). Returns its id, name, version and size.",
        &[Effect::FileRead, Effect::Settings],
    );

    fn run(&self, app: &mut App, _: &Caller, p: OpenParams) -> Result<LibrarySummary> {
        if p.path_or_url.starts_with("http://") || p.path_or_url.starts_with("https://") {
            return Err(bad("paramlib.url", "opening libraries from a URL is not built yet; download the file and open it"));
        }
        let path = PathBuf::from(&p.path_or_url);
        let text = std::fs::read_to_string(&path).map_err(|e| bad("paramlib.read", format!("{}: {e}", path.display())))?;
        let lib = paramlib::parse(&text)?;
        let id = lib.library.id.clone();
        let cached = cache_dir(app).join(format!("{id}.json"));
        if let Ok(old) = std::fs::read_to_string(&cached).map_err(|_| ()).and_then(|t| paramlib::parse(&t).map_err(|_| ())) {
            paramlib::check_against(&lib, &old)?;
        }
        if let Some(doc) = app.active().and_then(|d| app.document(d)) {
            for pp in reviera_core::parameters::project_parameters(doc.doc.store()) {
                if let Some(d) = lib.definition(&pp.guid)
                    && d.data_type != pp.data_type
                {
                    return Err(bad(
                        "paramlib.data_type_conflict",
                        format!("{} ({}) is {} in {} but {} in the project", d.name, d.guid, d.data_type, lib.library.name, pp.data_type),
                    ));
                }
            }
        }
        let read_only = std::fs::metadata(&path).map(|m| m.permissions().readonly()).unwrap_or(false);
        let open = OpenLibrary { path, lib, read_only };
        let out = summary(&open);
        app.libraries.insert(id.clone(), open);
        let dir = cache_dir(app);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{id}.json")), text);
        Ok(out)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub path: String,
    pub name: String,
}

pub(crate) struct ParamlibCreate;

impl HostCommand for ParamlibCreate {
    type Host = App;
    type Params = CreateParams;
    type Output = String;
    const META: CommandMeta = meta("paramlib.create", "Make a parameter library", "Makes an empty parameter library file at `path` named `name` and opens it; returns its id. An existing file is not overwritten.", &[Effect::FileWrite]);

    fn run(&self, app: &mut App, _: &Caller, p: CreateParams) -> Result<String> {
        let path = PathBuf::from(&p.path);
        if path.exists() {
            return Err(bad("paramlib.exists", format!("{} exists; open it instead", path.display())));
        }
        let lib = Library::new(p.name.trim());
        let id = lib.library.id.clone();
        app.libraries.insert(id.clone(), OpenLibrary { path, lib, read_only: false });
        app.save_library(&id)?;
        Ok(id)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdParams {
    pub library_id: String,
}

pub(crate) struct ParamlibClose;

impl HostCommand for ParamlibClose {
    type Host = App;
    type Params = IdParams;
    type Output = ();
    const META: CommandMeta = meta("paramlib.close", "Close a parameter library", "Closes an open library (its file stays as it is).", &[Effect::Settings]);

    fn run(&self, app: &mut App, _: &Caller, p: IdParams) -> Result<()> {
        app.libraries.remove(&p.library_id).map(|_| ()).ok_or_else(|| bad("paramlib.not_open", format!("no open library {}", p.library_id)))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

pub(crate) struct ParamlibList;

impl HostCommand for ParamlibList {
    type Host = App;
    type Params = Empty;
    type Output = Vec<LibrarySummary>;
    const META: CommandMeta = meta("paramlib.list", "The open parameter libraries", "The libraries open in this session.", &[Effect::Read]);

    fn run(&self, app: &mut App, _: &Caller, _: Empty) -> Result<Vec<LibrarySummary>> {
        Ok(app.libraries.values().map(summary).collect())
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DefinitionInput {
    pub name: Option<String>,
    /// A data type key; fixed once saved.
    pub data_type: Option<String>,
    /// A group id.
    pub group: Option<String>,
    pub param_group: Option<String>,
    pub tooltip: Option<String>,
    pub labels: Option<Vec<String>>,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<Value>>,
    pub ifc: Option<IfcHome>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Keep a GUID (otherwise a new one).
    pub guid: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddParams {
    pub library_id: String,
    pub def: DefinitionInput,
}

pub(crate) struct ParamlibDefinitionAdd;

impl HostCommand for ParamlibDefinitionAdd {
    type Host = App;
    type Params = AddParams;
    type Output = String;
    const META: CommandMeta = meta(
        "paramlib.definition.add",
        "Add a definition to a parameter library",
        "Adds a definition (`def`: name, data_type, group, param_group, tooltip, labels, enum, ifc {pset, property}, min, max, guid?) to an open library and writes the file. Returns its GUID.",
        &[Effect::FileWrite],
    );

    fn run(&self, app: &mut App, _: &Caller, p: AddParams) -> Result<String> {
        let l = app.library_mut(&p.library_id)?;
        let d = p.def;
        let guid = match &d.guid {
            Some(g) => paramlib::canonical(g).ok_or_else(|| bad("paramlib.guid", "the GUID is not a UUID"))?,
            None => uuid::Uuid::new_v4().to_string(),
        };
        let def = Definition {
            guid: guid.clone(),
            name: d.name.ok_or_else(|| bad("paramlib.name", "a definition needs a name"))?,
            data_type: d.data_type.ok_or_else(|| bad("paramlib.data_type", "a definition needs a data type"))?,
            enum_values: d.enum_values,
            group: d.group.unwrap_or_default(),
            param_group: d.param_group.unwrap_or_else(|| "other".into()),
            discipline: "common".into(),
            tooltip: d.tooltip.unwrap_or_default(),
            visible: true,
            user_modifiable: true,
            hide_when_no_value: false,
            ifc: d.ifc,
            labels: d.labels.unwrap_or_default(),
            deprecated: false,
            superseded_by: None,
            min: d.min,
            max: d.max,
        };
        let mut next = l.lib.clone();
        next.definitions.push(def);
        paramlib::check(&next)?;
        l.lib = next;
        app.save_library(&p.library_id)?;
        Ok(guid)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditParams {
    pub library_id: String,
    pub guid: String,
    pub changes: DefinitionInput,
}

pub(crate) struct ParamlibDefinitionEdit;

impl HostCommand for ParamlibDefinitionEdit {
    type Host = App;
    type Params = EditParams;
    type Output = ();
    const META: CommandMeta = meta(
        "paramlib.definition.edit",
        "Change a library definition",
        "Changes a definition's name, group, parameter group, tooltip, labels, enum, IFC home, min or max and writes the file. Its data type cannot change.",
        &[Effect::FileWrite],
    );

    fn run(&self, app: &mut App, _: &Caller, p: EditParams) -> Result<()> {
        let l = app.library_mut(&p.library_id)?;
        let g = paramlib::canonical(&p.guid).ok_or_else(|| bad("paramlib.guid", "the GUID is not a UUID"))?;
        let mut next = l.lib.clone();
        let d = next.definitions.iter_mut().find(|d| d.guid == g).ok_or_else(|| bad("paramlib.not_found", format!("no definition {g}")))?;
        let c = p.changes;
        if c.data_type.as_ref().is_some_and(|t| *t != d.data_type) {
            return Err(bad("paramlib.data_type_locked", format!("{}'s data type ({}) cannot change", d.name, d.data_type)));
        }
        if let Some(v) = c.name {
            d.name = v;
        }
        if let Some(v) = c.group {
            d.group = v;
        }
        if let Some(v) = c.param_group {
            d.param_group = v;
        }
        if let Some(v) = c.tooltip {
            d.tooltip = v;
        }
        if let Some(v) = c.labels {
            d.labels = v;
        }
        if c.enum_values.is_some() {
            d.enum_values = c.enum_values;
        }
        if c.ifc.is_some() {
            d.ifc = c.ifc;
        }
        if c.min.is_some() {
            d.min = c.min;
        }
        if c.max.is_some() {
            d.max = c.max;
        }
        paramlib::check(&next)?;
        l.lib = next;
        app.save_library(&p.library_id)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeprecateParams {
    pub library_id: String,
    pub guid: String,
    pub superseded_by: Option<String>,
}

pub(crate) struct ParamlibDefinitionDeprecate;

impl HostCommand for ParamlibDefinitionDeprecate {
    type Host = App;
    type Params = DeprecateParams;
    type Output = ();
    const META: CommandMeta = meta(
        "paramlib.definition.deprecate",
        "Deprecate a library definition",
        "Marks a definition deprecated (definitions are never removed), optionally `superseded_by` a current definition of the same data type, and writes the file.",
        &[Effect::FileWrite],
    );

    fn run(&self, app: &mut App, _: &Caller, p: DeprecateParams) -> Result<()> {
        let l = app.library_mut(&p.library_id)?;
        let g = paramlib::canonical(&p.guid).ok_or_else(|| bad("paramlib.guid", "the GUID is not a UUID"))?;
        let mut next = l.lib.clone();
        let d = next.definitions.iter_mut().find(|d| d.guid == g).ok_or_else(|| bad("paramlib.not_found", format!("no definition {g}")))?;
        d.deprecated = true;
        d.superseded_by = p.superseded_by.as_deref().map(|s| paramlib::canonical(s).unwrap_or_else(|| s.to_owned()));
        paramlib::check(&next)?;
        l.lib = next;
        app.save_library(&p.library_id)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupParams {
    pub library_id: String,
    pub name: String,
    /// Renaming: the group's id.
    pub group_id: Option<String>,
}

pub(crate) struct ParamlibGroupAdd;

impl HostCommand for ParamlibGroupAdd {
    type Host = App;
    type Params = GroupParams;
    type Output = String;
    const META: CommandMeta = meta("paramlib.group.add", "Add a library group", "Adds a group (for organising definitions) and writes the file; returns its id.", &[Effect::FileWrite]);

    fn run(&self, app: &mut App, _: &Caller, p: GroupParams) -> Result<String> {
        let l = app.library_mut(&p.library_id)?;
        let base: String = p.name.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let mut id = format!("g-{base}");
        let mut n = 2;
        while l.lib.groups.iter().any(|g| g.id == id) {
            id = format!("g-{base}-{n}");
            n += 1;
        }
        l.lib.groups.push(Group { id: id.clone(), name: p.name.trim().to_owned() });
        app.save_library(&p.library_id)?;
        Ok(id)
    }
}

pub(crate) struct ParamlibGroupRename;

impl HostCommand for ParamlibGroupRename {
    type Host = App;
    type Params = GroupParams;
    type Output = ();
    const META: CommandMeta = meta("paramlib.group.rename", "Rename a library group", "Renames the group `group_id` and writes the file.", &[Effect::FileWrite]);

    fn run(&self, app: &mut App, _: &Caller, p: GroupParams) -> Result<()> {
        let l = app.library_mut(&p.library_id)?;
        let gid = p.group_id.ok_or_else(|| bad("paramlib.group", "name the group to rename (group_id)"))?;
        let g = l.lib.groups.iter_mut().find(|g| g.id == gid).ok_or_else(|| bad("paramlib.group", format!("no group {gid}")))?;
        g.name = p.name.trim().to_owned();
        app.save_library(&p.library_id)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    /// A legacy shared-parameter text file.
    pub path: String,
    pub target_library_id: String,
    /// Data types for tokens Reviera does not know (`{"FOO": "number"}`).
    #[serde(default)]
    pub mapping: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImportReport {
    pub added: usize,
    /// GUIDs the library had already.
    pub skipped: usize,
}

pub(crate) struct ParamlibImportLegacy;

impl HostCommand for ParamlibImportLegacy {
    type Host = App;
    type Params = ImportParams;
    type Output = ImportReport;
    const META: CommandMeta = meta(
        "paramlib.import_legacy",
        "Import a legacy shared-parameter file",
        "Imports a legacy shared-parameter text file (tab-separated *GROUP and *PARAM sections) into an open library, keeping GUIDs, names, groups, visibility, descriptions and data types (mapped to Reviera's; `mapping` gives unknown tokens, else they are listed and nothing is imported). Definitions whose GUID the library has are skipped.",
        &[Effect::FileRead, Effect::FileWrite],
    );

    fn run(&self, app: &mut App, _: &Caller, p: ImportParams) -> Result<ImportReport> {
        let bytes = std::fs::read(&p.path).map_err(|e| bad("paramlib.read", format!("{}: {e}", p.path)))?;
        // Legacy files are often UTF-16.
        let text = if bytes.starts_with(&[0xFF, 0xFE]) {
            String::from_utf16_lossy(&bytes[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect::<Vec<_>>())
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let (groups, defs) = paramlib::import_legacy(&text, &p.mapping)?;
        let l = app.library_mut(&p.target_library_id)?;
        let mut next = l.lib.clone();
        for g in groups {
            if !next.groups.iter().any(|x| x.id == g.id) {
                next.groups.push(g);
            }
        }
        let (mut added, mut skipped) = (0, 0);
        for d in defs {
            if next.definitions.iter().any(|x| x.guid == d.guid) {
                skipped += 1;
            } else {
                next.definitions.push(d);
                added += 1;
            }
        }
        paramlib::check(&next)?;
        l.lib = next;
        app.save_library(&p.target_library_id)?;
        Ok(ImportReport { added, skipped })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    pub library_id: String,
    /// The `.ifc` file to write.
    pub path: String,
}

pub(crate) struct ParamlibExportIfc;

impl HostCommand for ParamlibExportIfc {
    type Host = App;
    type Params = ExportParams;
    type Output = usize;
    const META: CommandMeta = meta(
        "paramlib.export_ifc_templates",
        "Export a library as IFC property templates",
        "Writes an open library as an IFC4 file: an IfcProjectLibrary declaring one IfcPropertySetTemplate per property set (a definition's IFC home, else the library's default property set or name) holding an IfcSimplePropertyTemplate per definition, whose GlobalId is the definition's GUID and whose PrimaryMeasureType is its data type's IFC type. Returns the number of templates.",
        &[Effect::FileWrite],
    );

    fn run(&self, app: &mut App, _: &Caller, p: ExportParams) -> Result<usize> {
        let l = app.libraries.get(&p.library_id).ok_or_else(|| bad("paramlib.not_open", format!("no open library {}", p.library_id)))?;
        let lib = &l.lib;
        let ie = |e: reviera_ifc::IfcError| bad("paramlib.ifc", e.to_string());
        let mut m = IfcModel::new(Schema::Ifc4).map_err(ie)?;
        let s = |v: &str| AttrValue::String(v.into());
        let guid_of = |g: &str| uuid::Uuid::parse_str(g).map(GlobalId::from_uuid).unwrap_or_default();
        let library = m.create_rooted("IfcProjectLibrary", &guid_of(&lib.library.id)).map_err(ie)?;
        m.set_attribute(library, "Name", &s(&lib.library.name)).map_err(ie)?;
        if !lib.library.version.is_empty() {
            m.set_attribute(library, "LongName", &s(&format!("{} {}", lib.library.name, lib.library.version))).map_err(ie)?;
        }
        let fallback = if lib.library.default_pset.is_empty() { lib.library.name.clone() } else { lib.library.default_pset.clone() };
        let mut by_pset: BTreeMap<String, Vec<&Definition>> = BTreeMap::new();
        for d in lib.definitions.iter().filter(|d| !d.deprecated) {
            let pset = d.ifc.as_ref().map_or(fallback.clone(), |h| h.pset.clone());
            by_pset.entry(pset).or_default().push(d);
        }
        let mut psets = Vec::new();
        let mut count = 0;
        for (pset, defs) in by_pset {
            let mut props = Vec::new();
            for d in defs {
                let t = m.create_rooted("IfcSimplePropertyTemplate", &guid_of(&d.guid)).map_err(ie)?;
                let name = d.ifc.as_ref().map_or(d.name.clone(), |h| h.property.clone());
                m.set_attribute(t, "Name", &s(&name)).map_err(ie)?;
                if !d.tooltip.is_empty() {
                    m.set_attribute(t, "Description", &s(&d.tooltip)).map_err(ie)?;
                }
                let kind = if d.enum_values.is_some() { "P_ENUMERATEDVALUE" } else { "P_SINGLEVALUE" };
                m.set_attribute(t, "TemplateType", &AttrValue::Enum(kind.into())).map_err(ie)?;
                let measure = reviera_core::units::data_type(&d.data_type).map_or("IfcLabel", |t| t.ifc_type);
                m.set_attribute(t, "PrimaryMeasureType", &s(measure)).map_err(ie)?;
                props.push(AttrValue::Ref(t.0));
                count += 1;
            }
            let ps = m.create_rooted("IfcPropertySetTemplate", &GlobalId::new()).map_err(ie)?;
            m.set_attribute(ps, "Name", &s(&pset)).map_err(ie)?;
            m.set_attribute(ps, "TemplateType", &AttrValue::Enum("PSET_OCCURRENCEDRIVEN".into())).map_err(ie)?;
            m.set_attribute(ps, "HasPropertyTemplates", &AttrValue::List(props)).map_err(ie)?;
            psets.push(AttrValue::Ref(ps.0));
        }
        let rel = m.create_rooted("IfcRelDeclares", &GlobalId::new()).map_err(ie)?;
        m.set_attribute(rel, "RelatingContext", &AttrValue::Ref(library.0)).map_err(ie)?;
        m.set_attribute(rel, "RelatedDefinitions", &AttrValue::List(psets)).map_err(ie)?;
        m.write(Path::new(&p.path)).map_err(ie)?;
        Ok(count)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    pub text: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub data_type: Option<String>,
    /// Include deprecated definitions.
    #[serde(default)]
    pub deprecated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Found {
    pub library_id: String,
    pub library: String,
    pub definition: Definition,
    /// The open project has it.
    pub in_project: bool,
}

pub(crate) struct ParamlibSearch;

impl HostCommand for ParamlibSearch {
    type Host = App;
    type Params = SearchParams;
    type Output = Vec<Found>;
    const META: CommandMeta = meta(
        "paramlib.search",
        "Search the parameter libraries",
        "Definitions in the open libraries matching `text` (name, tooltip, labels), any of `labels` and a `data_type`; deprecated ones only with `deprecated`. Each says whether the open project has it.",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, p: SearchParams) -> Result<Vec<Found>> {
        let in_project: Vec<String> = app.active().and_then(|d| app.document(d)).map(|d| reviera_core::parameters::project_parameters(d.doc.store()).into_iter().map(|p| p.guid).collect()).unwrap_or_default();
        let mut out = Vec::new();
        for l in app.libraries.values() {
            for d in &l.lib.definitions {
                if (p.deprecated || !d.deprecated) && paramlib::matches(d, p.text.as_deref(), &p.labels, p.data_type.as_deref()) {
                    out.push(Found { library_id: l.lib.library.id.clone(), library: l.lib.library.name.clone(), definition: d.clone(), in_project: in_project.contains(&d.guid) });
                }
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnConflict {
    /// Refuse, saying the project has it (the default).
    #[default]
    Ask,
    Keep,
    Update,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToProjectParams {
    pub library_id: String,
    pub guid: String,
    /// Category keys (`cat.walls`, …).
    pub categories: Vec<String>,
    pub is_instance: bool,
    pub pset_name: Option<String>,
    #[serde(default)]
    pub on_conflict: OnConflict,
}

pub(crate) struct ParamlibAddToProject;

impl HostCommand for ParamlibAddToProject {
    type Host = App;
    type Params = ToProjectParams;
    type Output = String;
    const META: CommandMeta = meta(
        "paramlib.add_to_project",
        "Add a library definition to the project",
        "Binds a library definition to categories of the open project as a shared parameter (`param.project.add` with its GUID), embedding a full copy with the library's id, name and version so the project never needs the file. When the project has the GUID already: a different data type is refused, naming both; otherwise `on_conflict` keep leaves the project's, update takes the library's name and tooltip, and ask (default) refuses saying so. Returns the GUID.",
        &[Effect::Edit],
    );

    fn run(&self, app: &mut App, caller: &Caller, p: ToProjectParams) -> Result<String> {
        let l = app.libraries.get(&p.library_id).ok_or_else(|| bad("paramlib.not_open", format!("no open library {}", p.library_id)))?;
        let d = l.lib.definition(&p.guid).cloned().ok_or_else(|| bad("paramlib.not_found", format!("no definition {}", p.guid)))?;
        let source = json!({ "id": l.lib.library.id, "name": l.lib.library.name, "version": l.lib.library.version });
        let lib_name = l.lib.library.name.clone();
        let pset = p.pset_name.clone().or_else(|| d.ifc.as_ref().map(|h| h.pset.clone())).or_else(|| (!l.lib.library.default_pset.is_empty()).then(|| l.lib.library.default_pset.clone()));
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let existing = app.document(doc).and_then(|o| reviera_core::parameters::project_parameters(o.doc.store()).into_iter().find(|x| x.guid == d.guid));
        if let Some(e) = existing {
            if e.data_type != d.data_type {
                return Err(bad("paramlib.data_type_conflict", format!("{} ({}) is {} in {lib_name} but {} in the project", d.name, d.guid, d.data_type, e.data_type)));
            }
            let same = e.name == d.name && e.tooltip.as_deref().unwrap_or("") == d.tooltip;
            return match p.on_conflict {
                OnConflict::Keep => Ok(e.guid),
                OnConflict::Update if same => Ok(e.guid),
                OnConflict::Update => {
                    app.execute("param.project.modify", json!({ "param_id": e.guid, "changes": { "name": d.name, "tooltip": d.tooltip } }), caller)?;
                    Ok(e.guid)
                }
                OnConflict::Ask if same => Ok(e.guid),
                OnConflict::Ask => Err(bad("paramlib.conflict", format!("the project has {} as {:?}; keep the project's or update from {lib_name} (on_conflict)", d.guid, e.name))),
            };
        }
        let mut args = json!({
            "def": { "name": d.name, "data_type": d.data_type, "group": d.param_group, "tooltip": d.tooltip, "guid": d.guid, "library": source },
            "categories": p.categories,
            "is_instance": p.is_instance,
        });
        if let Some(ps) = pset {
            args["pset_name"] = json!(ps);
        }
        let out = app.execute("param.project.add", args, caller)?;
        Ok(out.result.as_str().unwrap_or_default().to_owned())
    }
}

pub(crate) struct ParamlibManage;

impl HostCommand for ParamlibManage {
    type Host = App;
    type Params = Empty;
    type Output = Value;
    const META: CommandMeta = meta("paramlib.manage", "Open the Parameter Libraries dialog", "Asks the window to open the Parameter Libraries dialog; without a window it returns `shown: false` with `reason: \"ui_unavailable\"`.", &[Effect::Ui]);

    fn run(&self, app: &mut App, _: &Caller, _: Empty) -> Result<Value> {
        if !app.has_window() {
            return Ok(json!({ "shown": false, "reason": "ui_unavailable" }));
        }
        app.paramlib_requested = true;
        Ok(json!({ "shown": true }))
    }
}

impl App {
    /// Whether a command asked the window to open the Parameter Libraries dialog.
    pub fn take_paramlib_request(&mut self) -> bool {
        std::mem::take(&mut self.paramlib_requested)
    }
}
