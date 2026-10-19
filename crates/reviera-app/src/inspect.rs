// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Inspection state of the session: temporary hide, isolate and translucency per view window
//! (DOC-GFX-008, UI-VCB-001), the model tree (UI-TREE-001), and fitting a section box to elements
//! (DOC-VIEW-008).
//!
//! Temporary state is not a model change: no undo, not saved, not printed, cleared when the model
//! closes. It is kept per view; the unsaved default 3D view of a model with no views is the view
//! `None`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use reviera_core::inspect::{self, Scope, TreeNode};
use reviera_core::views;
use reviera_core::{
    Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Result,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{App, DocumentId};

/// The default opacity of temporarily translucent elements.
pub const TRANSLUCENT: f32 = 0.25;

/// The temporary visibility of one view.
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct TempState {
    pub hidden: BTreeSet<ElementId>,
    /// Translucent elements and their opacity.
    pub translucent: BTreeMap<ElementId, f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolated: Option<Isolated>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Isolated {
    pub elements: BTreeSet<ElementId>,
    /// Elements created after isolating (this id and later) stay visible.
    pub after: ElementId,
    /// Others are drawn at this opacity rather than hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ghost_others: Option<f32>,
}

impl TempState {
    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty() && self.translucent.is_empty() && self.isolated.is_none()
    }

    /// The state as the renderer takes it.
    pub fn visibility(&self) -> reviera_render::Visibility {
        reviera_render::Visibility {
            hidden: self.hidden.iter().copied().collect(),
            translucent: self.translucent.iter().map(|(k, v)| (*k, *v)).collect(),
            isolated: self.isolated.as_ref().map(|i| reviera_render::Isolation {
                elements: i.elements.iter().copied().collect(),
                after: i.after,
                ghost: i.ghost_others,
            }),
            virtual_volumes: reviera_render::VirtualMode::Hidden,
            ..Default::default()
        }
    }
}

/// What a temporary visibility command does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TempAction {
    Hide,
    Isolate { ghost_others: bool },
    Translucent { opacity: f32 },
    Show,
}

impl App {
    /// The parent of each element in the model tree: Reviera elements by their data, others by
    /// the IFC file's relationships (read once per save).
    pub fn model_parents(&self, doc: DocumentId) -> Result<HashMap<ElementId, ElementId>> {
        let open = self.get(doc)?;
        let imported = open.spatial()?;
        Ok(inspect::parents(open.doc.store(), &imported))
    }

    /// The model tree (UI-TREE-001).
    pub fn model_tree(&self, doc: DocumentId) -> Result<TreeNode> {
        let open = self.get(doc)?;
        let imported = open.spatial()?;
        Ok(inspect::tree(open.doc.store(), &imported))
    }

    /// The temporary state of a view (empty when it has none).
    pub fn temp_state(&self, doc: DocumentId, view: Option<ElementId>) -> TempState {
        self.get(doc)
            .ok()
            .and_then(|o| o.temp.get(&view).cloned())
            .unwrap_or_default()
    }

    /// Whether any view of a document has temporary state.
    pub fn temp_active(&self, doc: DocumentId, view: Option<ElementId>) -> bool {
        !self.temp_state(doc, view).is_empty()
    }

    /// Applies a temporary visibility action to `ids` expanded by `scope`.
    pub fn temp_apply(
        &mut self,
        doc: DocumentId,
        view: Option<ElementId>,
        action: TempAction,
        ids: &[ElementId],
        scope: Scope,
    ) -> Result<usize> {
        let parents = self.model_parents(doc)?;
        let open = self.get_mut(doc)?;
        let store = open.doc.store();
        let reached = inspect::expand(store, &parents, ids, scope);
        if reached.is_empty() {
            return Err(Error::new(
                "view.nothing_to_change",
                "none of those elements is in the model",
            ));
        }
        let after = ElementId(store.iter().map(|e| e.id().0).max().unwrap_or(0) + 1);
        let state = open.temp.entry(view).or_default();
        match action {
            TempAction::Hide => {
                for id in &reached {
                    state.translucent.remove(id);
                }
                state.hidden.extend(&reached);
            }
            TempAction::Isolate { ghost_others } => {
                state.isolated = Some(Isolated {
                    elements: reached.clone(),
                    after,
                    ghost_others: ghost_others.then_some(TRANSLUCENT),
                });
                for id in &reached {
                    state.hidden.remove(id);
                }
            }
            TempAction::Translucent { opacity } => {
                for id in &reached {
                    state.hidden.remove(id);
                    state.translucent.insert(*id, opacity);
                }
            }
            TempAction::Show => {
                for id in &reached {
                    state.hidden.remove(id);
                    state.translucent.remove(id);
                }
                if let Some(i) = &mut state.isolated {
                    i.elements.extend(&reached);
                }
            }
        }
        if state.is_empty() {
            open.temp.remove(&view);
        }
        Ok(reached.len())
    }

    /// Clears a view's temporary state.
    pub fn temp_reset(&mut self, doc: DocumentId, view: Option<ElementId>) {
        if let Ok(open) = self.get_mut(doc) {
            open.temp.remove(&view);
        }
    }

    /// The bounds of the geometry of `ids`, metres.
    pub fn element_bounds(
        &self,
        doc: DocumentId,
        ids: &[ElementId],
    ) -> Result<([f64; 3], [f64; 3])> {
        let wanted: BTreeSet<ElementId> = ids.iter().copied().collect();
        let scene = self.render_scene(doc)?;
        let mut points = scene
            .meshes
            .iter()
            .filter(|m| m.element.is_some_and(|e| wanted.contains(&e)))
            .flat_map(|m| m.positions.iter())
            .map(|p| p.map(f64::from));
        let first = points.next().ok_or_else(|| {
            Error::new(
                "view.no_geometry",
                "the elements have no geometry to fit a section box to",
            )
        })?;
        Ok(points.fold((first, first), |(lo, hi), p| {
            (
                [0, 1, 2].map(|k| lo[k].min(p[k])),
                [0, 1, 2].map(|k| hi[k].max(p[k])),
            )
        }))
    }

    /// The view a temporary-state command acts on: the given view, else the active one (which
    /// may be the unsaved default 3D view, `None`).
    fn temp_view(&self, r: &Option<ElementRef>) -> Result<(DocumentId, Option<ElementId>)> {
        let doc = self
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        match r {
            Some(r) => {
                let store = self.get(doc)?.doc.store();
                let (id, _) = views::resolve_view(store, r)?;
                Ok((doc, Some(id)))
            }
            None => Ok((doc, self.active_view(doc))),
        }
    }
}

impl crate::OpenDocument {
    /// The IFC file's parent relationships, read once per save.
    fn spatial(&self) -> Result<Arc<inspect::Imported>> {
        let revision = self.project.data_revision();
        let mut cache = self.spatial.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((r, map)) = &*cache
            && *r == revision
        {
            return Ok(map.clone());
        }
        let map = Arc::new(
            self.project
                .spatial_parents()
                .map_err(|e| Error::new("model.tree_failed", e.to_string()))?,
        );
        *cache = Some((revision, map.clone()));
        Ok(map)
    }
}

// ---- Host commands ----

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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TempParams {
    /// The view; the active view when omitted.
    pub view_id: Option<ElementRef>,
    pub element_ids: Vec<ElementRef>,
    /// What the elements reach: themselves, their subtree in the model tree, their categories
    /// or their types.
    #[serde(default)]
    pub by: Scope,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TempIsolateParams {
    pub view_id: Option<ElementRef>,
    pub element_ids: Vec<ElementRef>,
    #[serde(default)]
    pub by: Scope,
    /// Draw the other elements translucent instead of hiding them.
    #[serde(default)]
    pub ghost_others: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TempTranslucentParams {
    pub view_id: Option<ElementRef>,
    pub element_ids: Vec<ElementRef>,
    #[serde(default)]
    pub by: Scope,
    /// Opacity, 0.05 to 0.95 (default 0.25).
    pub opacity: Option<f32>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TempResult {
    /// How many elements the command reached.
    pub count: usize,
}

fn resolve_ids(app: &App, doc: DocumentId, ids: &[ElementRef]) -> Result<Vec<ElementId>> {
    let store = app.get(doc)?.doc.store();
    ids.iter()
        .map(|r| {
            store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
        })
        .collect()
}

fn temp_run(
    app: &mut App,
    view: &Option<ElementRef>,
    ids: &[ElementRef],
    by: Scope,
    action: TempAction,
) -> Result<TempResult> {
    let (doc, view) = app.temp_view(view)?;
    let ids = resolve_ids(app, doc, ids)?;
    let count = app.temp_apply(doc, view, action, &ids, by)?;
    Ok(TempResult { count })
}

pub(crate) struct ViewTempHide;

impl HostCommand for ViewTempHide {
    type Host = App;
    type Params = TempParams;
    type Output = TempResult;
    const META: CommandMeta = meta(
        "view.temp_hide",
        "Temporarily hide elements in a view (by element, subtree, category or type); not an edit",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: TempParams) -> Result<TempResult> {
        temp_run(app, &p.view_id, &p.element_ids, p.by, TempAction::Hide)
    }
}

pub(crate) struct ViewTempIsolate;

impl HostCommand for ViewTempIsolate {
    type Host = App;
    type Params = TempIsolateParams;
    type Output = TempResult;
    const META: CommandMeta = meta(
        "view.temp_isolate",
        "Temporarily show only these elements in a view, hiding or ghosting the rest; not an edit",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: TempIsolateParams) -> Result<TempResult> {
        let action = TempAction::Isolate {
            ghost_others: p.ghost_others,
        };
        temp_run(app, &p.view_id, &p.element_ids, p.by, action)
    }
}

pub(crate) struct ViewTempTranslucent;

impl HostCommand for ViewTempTranslucent {
    type Host = App;
    type Params = TempTranslucentParams;
    type Output = TempResult;
    const META: CommandMeta = meta(
        "view.temp_translucent",
        "Temporarily draw elements translucent in a view; not an edit",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: TempTranslucentParams) -> Result<TempResult> {
        let opacity = p.opacity.unwrap_or(TRANSLUCENT);
        if !(0.05..=0.95).contains(&opacity) {
            return Err(Error::new(
                "view.bad_opacity",
                "opacity must be between 0.05 and 0.95",
            ));
        }
        temp_run(
            app,
            &p.view_id,
            &p.element_ids,
            p.by,
            TempAction::Translucent { opacity },
        )
    }
}

pub(crate) struct ViewTempShow;

impl HostCommand for ViewTempShow {
    type Host = App;
    type Params = TempParams;
    type Output = TempResult;
    const META: CommandMeta = meta(
        "view.temp_show",
        "Clear temporary hiding and translucency of elements, adding them to an active isolation",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: TempParams) -> Result<TempResult> {
        temp_run(app, &p.view_id, &p.element_ids, p.by, TempAction::Show)
    }
}

pub(crate) struct ViewTempReset;

impl HostCommand for ViewTempReset {
    type Host = App;
    type Params = crate::views::OptionalViewParams;
    type Output = ();
    const META: CommandMeta = meta(
        "view.temp_reset",
        "Clear a view's temporary hide, isolate and translucency",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: crate::views::OptionalViewParams) -> Result<()> {
        let (doc, view) = app.temp_view(&p.view_id)?;
        app.temp_reset(doc, view);
        Ok(())
    }
}

pub(crate) struct ViewGetTempState;

impl HostCommand for ViewGetTempState {
    type Host = App;
    type Params = crate::views::OptionalViewParams;
    type Output = TempState;
    const META: CommandMeta = meta(
        "view.get_temp_state",
        "A view's temporary hidden, isolated and translucent elements",
        &[Effect::Read],
    );

    fn run(
        &self,
        app: &mut App,
        _: &Caller,
        p: crate::views::OptionalViewParams,
    ) -> Result<TempState> {
        let (doc, view) = app.temp_view(&p.view_id)?;
        Ok(app.temp_state(doc, view))
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

pub(crate) struct ModelTree;

impl HostCommand for ModelTree {
    type Host = App;
    type Params = NoParams;
    type Output = TreeNode;
    const META: CommandMeta = meta(
        "model.tree",
        "The model tree: project, site, building, storeys, categories and elements, with hosted elements under their hosts",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, _: NoParams) -> Result<TreeNode> {
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        app.model_tree(doc)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FitSectionBoxParams {
    pub element_ids: Vec<ElementRef>,
    /// The 3D view; the active view if it is 3D, else the default 3D view.
    pub view_id: Option<ElementRef>,
    /// Space around the elements, metres (default 0.3).
    pub margin: Option<f64>,
}

pub(crate) struct ViewFitSectionBox;

impl HostCommand for ViewFitSectionBox {
    type Host = App;
    type Params = FitSectionBoxParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "view.fit_section_box",
        version: 1,
        summary: "Fit a 3D view's section box around elements (Selection Box)",
        description: "Turns on the section box of a 3D view, axis-aligned around the elements with a margin \
                      (default 300 mm), and returns the view. Without view_id it uses the active view when \
                      that is 3D, else the default 3D view (created if needed).",
        effects: &[Effect::Edit],
        transaction_name: "Section Box",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: FitSectionBoxParams) -> Result<ElementId> {
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let ids = resolve_ids(app, doc, &p.element_ids)?;
        let margin = p.margin.unwrap_or(0.3);
        if !(0.0..=100.0).contains(&margin) {
            return Err(Error::new(
                "view.bad_margin",
                "the margin must be 0 to 100 m",
            ));
        }
        let (lo, hi) = app.element_bounds(doc, &ids)?;
        let store = app.get(doc)?.doc.store();
        let is_3d = |id: ElementId| {
            store
                .get(id)
                .and_then(views::view_data)
                .is_some_and(|d| d.kind == views::KIND_3D)
        };
        let view = match &p.view_id {
            Some(r) => views::resolve_view(store, r)?.0,
            None => match app.active_view(doc).filter(|v| is_3d(*v)) {
                Some(v) => v,
                None => {
                    let out = app.execute("view.create_3d", json!({}), caller)?;
                    serde_json::from_value(out.result)
                        .map_err(|e| Error::new("view.not_found", e.to_string()))?
                }
            },
        };
        app.execute(
            "view.set_section_box",
            json!({
                "view_id": view,
                "enabled": true,
                "transform": [[1, 0, 0], [0, 1, 0], [0, 0, 1], [0, 0, 0]],
                "min": lo.map(|v| v - margin),
                "max": hi.map(|v| v + margin),
            }),
            caller,
        )?;
        Ok(view)
    }
}
