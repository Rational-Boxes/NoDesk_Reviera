// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! BCF viewpoint files (COL-BCF-003): `bcf.export_viewpoint` writes a 3D view as a `.bcfv`
//! (camera, section box and slices as clipping planes, the view's temporary hide or isolate as
//! component visibility and the selection), `bcf.import_viewpoint` reads one back into a 3D view
//! (`view.apply_viewpoint` for the camera and cut, one undoable step; then the temporary state and
//! the selection, which are not edits). Components are matched by IFC GlobalId.

use reviera_core::bcf::{self, Visibility};
use reviera_core::id::GlobalId;
use reviera_core::inspect::Scope;
use reviera_core::views;
use reviera_core::{Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::App;
use crate::inspect::TempAction;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The 3D view named, else the active view when it is 3D.
fn three_d(app: &App, view: &Option<ElementRef>) -> Result<Option<ElementId>> {
    let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
    let store = app.get(doc)?.doc.store();
    match view {
        Some(r) => {
            let (id, data) = views::resolve_view(store, r)?;
            if data.kind != views::KIND_3D {
                return Err(bad("bcf.not_3d", "viewpoints are taken from and shown in 3D views"));
            }
            Ok(Some(id))
        }
        None => Ok(app
            .active_view(doc)
            .filter(|v| store.get(*v).and_then(views::view_data).is_some_and(|d| d.kind == views::KIND_3D))),
    }
}

/// A 3D view as a viewpoint: its camera and cut, the selection, and its temporary hide or
/// isolate as component visibility.
pub(crate) fn capture(app: &App, view: ElementId) -> Result<bcf::Viewpoint> {
    let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
    let open = app.get(doc)?;
    let store = open.doc.store();
    let (_, mut vp) = bcf::capture(store, &ElementRef::Id(view))?;
    let guid = |id: &ElementId| store.get(*id).map(|e| e.guid().to_string());
    vp.guid = Some(uuid::Uuid::new_v4().to_string());
    vp.selection = open.selection.iter().filter_map(guid).collect();
    let temp = app.temp_state(doc, Some(view));
    vp.visibility = match &temp.isolated {
        Some(i) => Visibility {
            default_visibility: false,
            exceptions: i.elements.iter().filter(|e| !temp.hidden.contains(e)).filter_map(guid).collect(),
        },
        None => Visibility { default_visibility: true, exceptions: temp.hidden.iter().filter_map(guid).collect() },
    };
    Ok(vp)
}

/// Shows a viewpoint in a 3D view (the one named, the active one when 3D, else the default 3D
/// view): camera and cut as one undoable step, then its components hidden or isolated and
/// selected.
pub(crate) fn show(app: &mut App, caller: &Caller, vp: &bcf::Viewpoint, view: &Option<ElementRef>) -> Result<ImportReport> {
    let view = match three_d(app, view)? {
        Some(v) => v,
        None => {
            let out = app.execute("view.create_3d", json!({}), caller)?;
            serde_json::from_value(out.result).map_err(|e| bad("view.not_found", e.to_string()))?
        }
    };
    let doc = app.active().expect("checked");
    app.execute("view.apply_viewpoint", json!({ "view_id": view, "viewpoint": vp }), caller)?;
    // Components by GlobalId.
    let mut not_found = Vec::new();
    let (selection, exceptions) = {
        let store = app.get(doc)?.doc.store();
        let mut find = |gs: &[String]| -> Vec<ElementId> {
            gs.iter()
                .filter_map(|g| {
                    let id = GlobalId::parse(g).and_then(|id| store.find_by_guid(&id));
                    if id.is_none() && !not_found.contains(g) {
                        not_found.push(g.clone());
                    }
                    id
                })
                .collect()
        };
        (find(&vp.selection), find(&vp.visibility.exceptions))
    };
    app.temp_reset(doc, Some(view));
    let isolated = !vp.visibility.default_visibility;
    if !exceptions.is_empty() {
        let action = if isolated { TempAction::Isolate { ghost_others: false } } else { TempAction::Hide };
        app.temp_apply(doc, Some(view), action, &exceptions, Scope::Element)?;
    }
    app.get_mut(doc)?.selection = selection.iter().copied().collect();
    let data = app.get(doc)?.doc.store().get(view).and_then(views::view_data);
    Ok(ImportReport {
        view_id: view,
        section_box: data.as_ref().is_some_and(|d| d.section_box.as_ref().is_some_and(|b| b.enabled)),
        slice_planes: data.map_or(0, |d| d.slice_planes.len()),
        selected: selection.len(),
        exceptions: exceptions.len(),
        isolated,
        not_found,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    /// A 3D view; the active view when omitted (it must be 3D).
    pub view_id: Option<ElementRef>,
    /// The `.bcfv` file to write.
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportReport {
    pub view_id: ElementId,
    pub clipping_planes: usize,
    pub selected: usize,
    /// Whether elements are shown unless listed (`true`) or only the listed ones (`false`, an
    /// isolated view).
    pub default_visibility: bool,
    pub exceptions: usize,
}

pub(crate) struct BcfExportViewpoint;

impl HostCommand for BcfExportViewpoint {
    type Host = App;
    type Params = ExportParams;
    type Output = ExportReport;
    const META: CommandMeta = CommandMeta {
        name: "bcf.export_viewpoint",
        version: 1,
        summary: "Export a 3D view as a BCF viewpoint",
        description: "Writes a 3D view as a BCF 3.0 viewpoint file (.bcfv, COL-BCF-003) at `path`: its camera \
                      (perspective with the vertical field of view, or orthogonal with the view's height as \
                      ViewToWorldScale), its section box's six faces and its slice planes as clipping planes \
                      (directions pointing to the hidden side), the selected elements, and its temporary \
                      hide or isolate as component visibility (hidden elements as exceptions to a visible \
                      default; an isolated view as a hidden default with the isolated elements as \
                      exceptions), by IFC GlobalId.",
        effects: &[Effect::FileWrite],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ExportParams) -> Result<ExportReport> {
        let view = three_d(app, &p.view_id)?.ok_or_else(|| bad("bcf.not_3d", "the active view is not a 3D view: give view_id"))?;
        let vp = capture(app, view)?;
        std::fs::write(&p.path, bcf::to_xml(&vp)).map_err(|e| bad("bcf.write", format!("{}: {e}", p.path)))?;
        Ok(ExportReport {
            view_id: view,
            clipping_planes: vp.clipping_planes.len(),
            selected: vp.selection.len(),
            default_visibility: vp.visibility.default_visibility,
            exceptions: vp.visibility.exceptions.len(),
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    /// The `.bcfv` file to read.
    pub path: String,
    /// The 3D view to show it in; the active view when it is 3D, else the default 3D view.
    pub view_id: Option<ElementRef>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportReport {
    pub view_id: ElementId,
    pub section_box: bool,
    pub slice_planes: usize,
    pub selected: usize,
    /// Elements hidden, or (with `isolated`) the ones left shown.
    pub exceptions: usize,
    pub isolated: bool,
    /// GlobalIds the viewpoint names that are not in the model.
    pub not_found: Vec<String>,
}

pub(crate) struct BcfImportViewpoint;

impl HostCommand for BcfImportViewpoint {
    type Host = App;
    type Params = ImportParams;
    type Output = ImportReport;
    const META: CommandMeta = CommandMeta {
        name: "bcf.import_viewpoint",
        version: 1,
        summary: "Show a BCF viewpoint in a 3D view",
        description: "Reads a BCF viewpoint file (.bcfv, BCF 2.1 or 3.0) and shows it in a 3D view (COL-BCF-003): \
                      the camera, the clipping planes as a section box (six that form a box) and slice planes \
                      (`view.apply_viewpoint`, one undoable step), then, as temporary state, the component \
                      visibility (a visible default hides the exceptions; a hidden default isolates them) and \
                      the selection. Without view_id it uses the active view when that is 3D, else the default \
                      3D view (created if needed). Returns what it applied and the GlobalIds not found.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: ImportParams) -> Result<ImportReport> {
        let text = std::fs::read_to_string(&p.path).map_err(|e| bad("bcf.read", format!("{}: {e}", p.path)))?;
        let vp = bcf::from_xml(&text)?;
        show(app, caller, &vp, &p.view_id)
    }
}
