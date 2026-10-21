// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! View state of the session (spec Ch 1 UI-NAV-001, UI-NAV-002): the latest camera and Home of
//! each view. Navigation changes it without undo entries and without marking the model modified;
//! it is saved in the RXD document `views/state` along with the next save.
//!
//! A view's camera is its latest navigated camera, unless its saved camera (`view.set_camera`, an
//! undoable edit) changed since: then the saved camera wins.

use std::collections::BTreeMap;

use reviera_core::views::{self, Preset, Projection, ViewCamera};
use reviera_core::{
    Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Result,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{App, DocumentId};

/// The RXD document holding view state.
pub const STATE_PATH: &str = "views/state";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewState {
    pub camera: ViewCamera,
    /// The saved camera the state was navigated from.
    #[serde(default)]
    pub base: Option<ViewCamera>,
    #[serde(default)]
    pub home: Option<ViewCamera>,
}

/// View state of one document, by view GlobalId.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewStates {
    #[serde(default)]
    pub views: BTreeMap<String, ViewState>,
    #[serde(skip)]
    pub changed: bool,
}

impl App {
    fn view_guid(&self, doc: DocumentId, view: ElementId) -> Result<(String, Option<ViewCamera>)> {
        let open = self.get(doc)?;
        let e = open
            .doc
            .store()
            .get(view)
            .ok_or_else(|| Error::new("view.not_found", format!("no view {view}")))?;
        let data = views::view_data(e).ok_or_else(|| {
            Error::new("view.not_a_view", format!("element {view} is not a view"))
        })?;
        Ok((e.guid().to_string(), data.camera))
    }

    /// The current camera of a view: navigated, else saved; `None` if it never had one.
    pub fn view_camera(&self, doc: DocumentId, view: ElementId) -> Option<ViewCamera> {
        let (guid, saved) = self.view_guid(doc, view).ok()?;
        let state = self.get(doc).ok()?.view_states.views.get(&guid);
        match state {
            Some(s) if s.base == saved => Some(s.camera),
            _ => saved,
        }
    }

    /// Records a view's navigated camera (no undo, not a model change).
    pub fn set_view_camera(&mut self, doc: DocumentId, view: ElementId, camera: ViewCamera) {
        let Ok((guid, saved)) = self.view_guid(doc, view) else {
            return;
        };
        let Ok(open) = self.get_mut(doc) else { return };
        let entry = open.view_states.views.entry(guid).or_insert(ViewState {
            camera,
            base: saved,
            home: None,
        });
        if entry.camera != camera || entry.base != saved {
            entry.camera = camera;
            entry.base = saved;
            open.view_states.changed = true;
        }
    }

    /// A view's Home: set in the session, else saved with the view.
    pub fn view_home(&self, doc: DocumentId, view: ElementId) -> Option<ViewCamera> {
        let (guid, _) = self.view_guid(doc, view).ok()?;
        let open = self.get(doc).ok()?;
        open.view_states
            .views
            .get(&guid)
            .and_then(|s| s.home)
            .or_else(|| views::view_data(open.doc.store().get(view)?)?.home)
    }

    pub fn set_view_home(&mut self, doc: DocumentId, view: ElementId, home: ViewCamera) {
        let Ok((guid, saved)) = self.view_guid(doc, view) else {
            return;
        };
        let current = self.view_camera(doc, view).unwrap_or(home);
        let Ok(open) = self.get_mut(doc) else { return };
        let entry = open.view_states.views.entry(guid).or_insert(ViewState {
            camera: current,
            base: saved,
            home: None,
        });
        entry.home = Some(home);
        open.view_states.changed = true;
    }

    /// The view the UI shows as active in a document (the default for view commands).
    pub fn active_view(&self, doc: DocumentId) -> Option<ElementId> {
        self.get(doc).ok()?.active_view
    }

    pub fn set_active_view(&mut self, doc: DocumentId, view: Option<ElementId>) {
        if let Ok(open) = self.get_mut(doc) {
            open.active_view = view;
        }
    }

    /// Views that commands asked the UI to open (`view.open`), oldest first.
    pub fn take_view_requests(&mut self) -> Vec<(DocumentId, ElementId)> {
        std::mem::take(&mut self.view_requests)
    }

    /// Writes changed view state into the project before a save.
    pub(crate) fn stage_view_state(&mut self, doc: DocumentId) -> Result<()> {
        let open = self.get_mut(doc)?;
        if !open.view_states.changed {
            return Ok(());
        }
        // State of views that no longer exist is dropped.
        let live: std::collections::HashSet<String> = open
            .doc
            .store()
            .iter()
            .filter(|e| views::is_view(e))
            .map(|e| e.guid().to_string())
            .collect();
        open.view_states.views.retain(|g, _| live.contains(g));
        let value = serde_json::to_value(&open.view_states).expect("view state serialises");
        open.project.set_document(STATE_PATH, value);
        open.view_states.changed = false;
        Ok(())
    }

    /// The view a command names, or the active view of the active document.
    fn command_view(&self, r: &Option<ElementRef>) -> Result<(DocumentId, ElementId)> {
        let doc = self
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let store = self.get(doc)?.doc.store();
        let view = match r {
            Some(r) => store
                .resolve(r)
                .ok_or_else(|| Error::new("view.not_found", "no such view"))?,
            None => self
                .active_view(doc)
                .ok_or_else(|| Error::new("view.none", "no view is active; pass view_id"))?,
        };
        views::resolve_view(store, &ElementRef::Id(view))?;
        Ok((doc, view))
    }

    /// A camera framing the model, for views that never had one.
    pub(crate) fn fitting_camera(&self, doc: DocumentId) -> ViewCamera {
        match self.render_scene(doc).ok().and_then(|s| s.bounds()) {
            Some((lo, hi)) => {
                ViewCamera::fitting(lo.to_array().map(f64::from), hi.to_array().map(f64::from))
            }
            None => ViewCamera::fitting([-10.0, -10.0, 0.0], [10.0, 10.0, 3.0]),
        }
    }
}

/// Loads the view state of a newly opened project.
pub(crate) fn load(project: &reviera_store::LocalProject) -> ViewStates {
    project
        .document(STATE_PATH)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
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

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptionalViewParams {
    /// The view; the active view when omitted.
    pub view_id: Option<ElementRef>,
}

pub(crate) struct ViewGetCamera;

impl HostCommand for ViewGetCamera {
    type Host = App;
    type Params = OptionalViewParams;
    type Output = ViewCamera;
    const META: CommandMeta = meta(
        "view.get_camera",
        "The current camera of a view (eye, target, up in metres; projection; horizontal field of view)",
        &[Effect::Read],
    );

    fn run(&self, app: &mut App, _: &Caller, p: OptionalViewParams) -> Result<ViewCamera> {
        let (doc, view) = app.command_view(&p.view_id)?;
        Ok(app
            .view_camera(doc, view)
            .unwrap_or_else(|| app.fitting_camera(doc)))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrientParams {
    pub view_id: Option<ElementRef>,
    pub preset: Preset,
}

pub(crate) struct ViewOrient;

impl HostCommand for ViewOrient {
    type Host = App;
    type Params = OrientParams;
    type Output = ();
    const META: CommandMeta = meta(
        "view.orient",
        "Turn a view to a standard orientation (top, front, …, isometrics, home); navigation, not an edit",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: OrientParams) -> Result<()> {
        let (doc, view) = app.command_view(&p.view_id)?;
        let current = app
            .view_camera(doc, view)
            .unwrap_or_else(|| app.fitting_camera(doc));
        let camera = match p.preset.direction() {
            Some(dir) => current.oriented(dir),
            None => app.view_home(doc, view).unwrap_or_else(|| ViewCamera {
                perspective: current.perspective,
                ..app.fitting_camera(doc)
            }),
        };
        app.set_view_camera(doc, view, camera);
        Ok(())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionParams {
    pub view_id: Option<ElementRef>,
    pub projection: Projection,
}

pub(crate) struct ViewSetProjection;

impl HostCommand for ViewSetProjection {
    type Host = App;
    type Params = ProjectionParams;
    type Output = ();
    const META: CommandMeta = meta(
        "view.set_projection",
        "Show a view in perspective or orthographic (isometric) projection; navigation, not an edit",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: ProjectionParams) -> Result<()> {
        let (doc, view) = app.command_view(&p.view_id)?;
        let mut camera = app
            .view_camera(doc, view)
            .unwrap_or_else(|| app.fitting_camera(doc));
        camera.perspective = p.projection == Projection::Perspective;
        app.set_view_camera(doc, view, camera);
        Ok(())
    }
}

pub(crate) struct ViewSetHome;

impl HostCommand for ViewSetHome {
    type Host = App;
    type Params = OptionalViewParams;
    type Output = ();
    const META: CommandMeta = meta(
        "view.set_home",
        "Make a view's current camera its Home orientation",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: OptionalViewParams) -> Result<()> {
        let (doc, view) = app.command_view(&p.view_id)?;
        let camera = app
            .view_camera(doc, view)
            .unwrap_or_else(|| app.fitting_camera(doc));
        app.set_view_home(doc, view, camera);
        Ok(())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenViewParams {
    pub view_id: ElementRef,
}

pub(crate) struct ViewOpen;

impl HostCommand for ViewOpen {
    type Host = App;
    type Params = OpenViewParams;
    type Output = ();
    const META: CommandMeta = meta(
        "view.open",
        "Open a view in a window (or bring its window to the front)",
        &[Effect::Ui],
    );

    fn run(&self, app: &mut App, _: &Caller, p: OpenViewParams) -> Result<()> {
        let (doc, view) = app.command_view(&Some(p.view_id))?;
        app.set_active_view(doc, Some(view));
        app.view_requests.push((doc, view));
        Ok(())
    }
}
