// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! wxWidgets UI shell through wxDragon (spec Ch 1).
//!
//! The main window (UI-VIEWWIN-001/002): menus, a status bar, the Properties palette as a dockable
//! pane, and view windows as tabs in the centre (`wxAuiNotebook`), several models side by side.
//! Each view has its own camera and `wgpu` surface; all share one graphics device. Clicking selects
//! an element and shows its parameters; edits in the palette go through `param.set`. The Wall tool
//! draws chains of walls on a level with `wall.create`, and they show at once. Every action goes
//! through `App::execute`, like Lua and MCP. The ribbon and browsers come next.

mod analytical;
mod analytical_checks;
mod analytical_panels;
mod areas;
mod assembly;
mod beams;
mod browser;
mod clash;
mod columns;
mod components;
mod crash;
mod curtain;
mod curtain_grid;
mod curved;
mod datum_tools;
mod decks;
mod dims;
mod dormers;
mod drawing_aids;
mod footprint;
mod foundations;
mod grids;
mod groups;
mod help;
mod icons;
mod in_place;
mod inserts;
mod inspect;
mod levels;
mod linings;
mod loading;
mod masses;
mod model_props;
mod modify;
mod native;
mod nav;
mod notes;
mod openings;
mod overlaps;
mod parts;
mod path_arrays;
mod radial;
mod mep_ui;
mod remote_analysis_ui;
mod terminal_ui;
mod mep_electrical_ui;
mod mep_cables_ui;
mod mep_sizing_ui;
mod mep_network_ui;
mod mep_calculator_ui;
mod view_capture;
mod mep_gutters_ui;
mod mep_loads_ui;
mod mep_runs_ui;
mod fullscreen;
mod plans;
mod railings;
mod ramps;
mod results_explorer;
mod loads_ui;
mod analytical_props;
mod adjust;
mod steel_auto_ui;
mod steel_detail_ui;
mod steel_cut_ui;
mod custom_ui;
mod coords;
mod precast_ui;
mod rebar_edit_ui;
mod rebar_ui;
mod rebar_view_ui;
mod reinforcement_ui;
mod steel_ui;
mod ribbon;
mod roofs;
mod rooms;
mod schedules;
mod scripts;
mod sections;
mod select;
mod sheets;
mod site;
mod sketch;
mod sketch_edit;
mod skylights;
mod slab_openings;
mod snap;
mod stack;
mod stairs;
mod structural_settings;
mod wall_profile;
mod wall_sweeps;
mod walls;
mod wx;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use reviera_app::{App, AppConfig, DocumentId};
use reviera_core::{Caller, ElementId};
use reviera_render::{Camera, RenderBackend, RenderScene};
use reviera_render_wgpu::{Gpu, WgpuViewport};
use serde_json::{Value, json};
use wxdragon::prelude::*;

const ID_HOME: i32 = ID_HIGHEST + 16;
const ID_SET_HOME: i32 = ID_HIGHEST + 17;
const ID_PERSPECTIVE: i32 = ID_HIGHEST + 18;
const ID_BROWSER_OPEN: i32 = ID_HIGHEST + 24;
const ID_BROWSER_RENAME: i32 = ID_HIGHEST + 25;
const ID_BROWSER_DUPLICATE: i32 = ID_HIGHEST + 26;
const ID_BROWSER_DELETE: i32 = ID_HIGHEST + 27;
const ID_BROWSER_ELEVATION: i32 = ID_HIGHEST + 32;
/// The orientation presets of `nav::PRESETS`, one id each.
const ID_ORIENT_FIRST: i32 = ID_HIGHEST + 40;

/// Height of walls drawn on the top level (they reach the level above otherwise).
const WALL_HEIGHT: f64 = 3.0;

/// The colour of a wall being drawn (linear RGBA).
const PREVIEW: [f32; 4] = [0.1, 0.45, 0.95, 0.45];
/// The colour of snap markers and the exterior-side arrow.
const SNAP: [f32; 4] = [0.95, 0.55, 0.05, 1.0];

/// A press that moves less than this many pixels is a click (select), not a drag.
const CLICK_SLOP: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// Orbit about a pivot (the selection's centre, the point under the cursor, or the view's
    /// target).
    Orbit(glam::Vec3),
    Pan,
    /// Turning the Orientation Cube (or, on its ring, turning about the vertical only).
    Cube {
        ring: bool,
    },
    /// Looking around in walk mode.
    Look,
    /// Dragging a selected door or window along its wall, from a plan point.
    MoveInsert {
        id: ElementId,
        from: [f64; 2],
    },
    /// Dragging an end grip of a selected wall or grid (the other end stays).
    Grip {
        id: ElementId,
        end: usize,
        other: [f64; 2],
    },
    /// Dragging the selected walls and grids, from a plan point.
    MoveSelection {
        from: [f64; 2],
    },
    /// Dragging a section box face or a slice plane (3D views).
    Clip(inspect::ClipGrip),
    /// Dragging a viewport or schedule on a sheet, from a paper point (metres).
    SheetItem {
        item: u32,
        from: [f64; 2],
    },
    /// Dragging a sketch curve or corner (sketch Modify).
    Sketch,
    /// Dragging a footprint grip of a roof, floor or ceiling.
    Footprint(footprint::FootprintDrag),
    /// Dragging a selection box from a pixel (UI-SEL-002).
    SelectBox {
        from: (i32, i32),
        combine: select::Combine,
    },
}

/// The Wall tool: clicks on the level's plane add walls end to end.
struct WallTool {
    level: ElementId,
    level_name: String,
    elevation: f64,
    /// The level above, which new walls reach.
    top: Option<ElementId>,
    /// The end of the last wall, where the next one starts.
    last: Option<[f64; 2]>,
    /// The (snapped) point under the cursor.
    cursor: Option<snap::Snap>,
    /// Exterior on the right of the drawing direction (Spacebar).
    flip: bool,
    /// A length being typed, committed with Enter.
    typed: String,
    /// Location lines of the walls on the level, for snapping.
    lines: Vec<([f64; 2], [f64; 2])>,
    /// Straight, arc or spline walls (ARC-WALL-002).
    shape: WallShape,
    /// Points clicked for the curved wall being drawn: an arc's end; a spline's points.
    pending: Vec<[f64; 2]>,
}

/// The shape the Wall tool draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WallShape {
    Line,
    /// Click the end, then a point the arc passes through.
    Arc,
    /// Click points; Enter ends at the last.
    Spline,
}

/// The parameter behind one palette row.
struct Row {
    element: ElementId,
    reference: String,
    data_type: String,
}

/// A view window: one tab showing a 3D view of a model.
struct View {
    id: u32,
    doc: DocumentId,
    /// The model view shown; `None` for the unsaved `{3D}` view of a model that has no views.
    element: Option<ElementId>,
    name: String,
    /// The camera last exchanged with the session's view state.
    synced: Option<reviera_core::views::ViewCamera>,
    /// Set for plan views.
    plan: Option<plans::PlanState>,
    /// Set for section and elevation views.
    section: Option<sections::SectionState>,
    /// Set for schedule views (a grid instead of a viewport).
    schedule: Option<schedules::ScheduleState>,
    /// Set for sheet views.
    sheet: Option<sheets::SheetState>,
    panel: Panel,
    viewport: Option<WgpuViewport>,
    /// The native surface under the viewport; declared after it so it is dropped after it.
    native: Option<native::NativeView>,
    error: Option<String>,
    camera: Camera,
    drag: Option<(Drag, i32, i32)>,
    press: Option<(i32, i32)>,
    nav: nav::Nav,
    /// A section box or slice plane being dragged.
    clip_edit: Option<inspect::ClipEdit>,
    /// The rebar segments this view draws, for picking (STR-RBR-017).
    rebar_picks: rebar_view_ui::RebarPicks,
}

struct Gui {
    app: App,
    /// The shared handle of this state, for handlers of new views.
    this: Weak<RefCell<Gui>>,
    frame: Frame,
    notebook: AuiNotebook,
    aui: AuiManager,
    /// The document of the active view.
    doc: Option<DocumentId>,
    views: Vec<View>,
    active: Option<u32>,
    next_view: u32,
    gpu: Option<Gpu>,
    /// The scene of each open document, shared by its views.
    scenes: HashMap<DocumentId, RenderScene>,
    rows: Vec<Row>,
    properties_stale: bool,
    tool: Option<WallTool>,
    dim_tool: Option<dims::DimTool>,
    grid_tool: Option<grids::GridTool>,
    line_tool: Option<datum_tools::LineTool>,
    /// Tab cycling under the cursor (ARC-KEY-001).
    tab: Option<drawing_aids::TabCycle>,
    insert_tool: Option<inserts::InsertTool>,
    modify_tool: Option<modify::ModifyTool>,
    /// A sketch session (floor boundaries).
    sketch: Option<sketch::SketchSession>,
    ceiling_tool: Option<sketch::CeilingTool>,
    room_tool: Option<rooms::RoomTool>,
    section_tool: Option<sections::SectionTool>,
    /// The Tag, Room Tag or Text tool.
    note_tool: Option<notes::NoteTool>,
    /// The Stair tool.
    stair_tool: Option<stairs::StairTool>,
    /// The Dormer tool.
    dormer_tool: Option<dormers::DormerTool>,
    /// The Railing tool.
    railing_tool: Option<railings::RailingTool>,
    /// The Duct or Pipe tool (MEP-DUCT-001, MEP-PIPE-001).
    run_tool: Option<mep_runs_ui::RunTool>,
    feature_tool: Option<railings::FeatureTool>,
    dome_tool: Option<curved::DomeTool>,
    column_tool: Option<columns::ColumnTool>,
    beam_tool: Option<beams::BeamTool>,
    /// Structure › Analytical › Show Analytical: the analytical model drawn over 3D views.
    show_analytical: bool,
    /// A structural analysis being followed (Analyze › Run Analysis).
    analysis_job: Option<analytical::AnalysisJob>,
    /// Structure › Wall: the Wall tool makes structural (bearing) walls.
    structural_walls: bool,
    footing_tool: Option<foundations::FootingTool>,
    beam_system_tool: Option<beams::BeamSystemTool>,
    truss_tool: Option<beams::TrussTool>,
    panel_tool: Option<analytical_panels::PanelTool>,
    load_tool: Option<loads_ui::LoadTool>,
    /// Steel › Detailing: Plate, Bolts or Weld in progress.
    steel_tool: Option<steel_detail_ui::SteelTool>,
    /// Concrete › Area, Path, Fabric Area or Fabric Sheet being sketched.
    rebar_sketch: Option<reinforcement_ui::RebarSketch>,
    /// Select Bars in progress (STR-RBR-017).
    bar_pick: Option<rebar_view_ui::BarPick>,
    /// A rebar annotation waiting for its point (STR-RBR-018): the tool, the sets and the plan.
    rebar_anno: Option<(rebar_view_ui::AnnoTool, Vec<ElementId>, ElementId)>,
    /// A sketched set's bar and references while Edit Constraints is open (STR-RBR-007).
    constraint_lines: Vec<reviera_render::plan::Lines>,
    /// Propagate or Pick New Host in progress (STR-RBR-006).
    rebar_host_pick: Option<rebar_edit_ui::HostPick>,
    /// Coupler in progress (STR-RBR-015).
    end_pick: Option<rebar_edit_ui::EndPick>,
    /// Contour Cut: the element the sketch cuts.
    contour_target: Option<ElementId>,
    adjust: Option<adjust::AdjustSession>,
    /// A connection automation preview's report, marked in 3D views.
    auto_preview: Option<serde_json::Value>,
    curtain_tool: Option<curtain::CurtainTool>,
    ramp_tool: Option<ramps::RampTool>,
    measure_tool: Option<ramps::MeasureTool>,
    skylight_tool: Option<skylights::SkylightTool>,
    /// The Place Component tool (a component picked up from the library, or a project type).
    place_tool: Option<components::PlaceTool>,
    /// The Array along Path tool.
    array_tool: Option<path_arrays::ArrayTool>,
    /// The Component Library panel.
    library_pane: Option<components::LibraryPane>,
    /// The Clash Detection pane, once opened.
    clash_pane: Option<clash::ClashPane>,
    checks_pane: Option<analytical_checks::ChecksPane>,
    results_pane: Option<results_explorer::ResultsPane>,
    /// Issue keys the automatic analytical checks last saw (STR-AM-007).
    auto_check_keys: Option<std::collections::HashSet<String>>,
    /// Renders component thumbnails offscreen (made when first needed).
    thumb_renderer: Option<WgpuViewport>,
    /// The last array's members and Move To (Last when true).
    array_count: u32,
    array_last: bool,
    /// Whether the Ceiling tool's ceilings follow the roof over them.
    ceiling_follow_roof: bool,
    /// The floor type new floors take.
    floor_type: Option<ElementId>,
    /// The last pointer position in the active view (Tab in sketch mode picks there).
    mouse: (i32, i32),
    /// The Offset tool's distance (metres), kept for the session.
    offset_distance: f64,
    /// The door and window types chosen for the tools, by IFC class.
    insert_types: std::collections::HashMap<&'static str, ElementId>,
    /// The grid type chosen for the Grid tool.
    grid_type: Option<ElementId>,
    /// The grid type whose values the palette shows.
    type_rows: Option<ElementId>,
    /// The type new walls get (Wall tool); `None` for the default.
    wall_type: Option<ElementId>,
    /// The wall type whose values the palette shows.
    wall_type_row: Option<ElementId>,
    /// The two elements the quick modify menu offers booleans for (UI-SEL-006).
    quick_pair: Option<select::QuickPair>,
    /// The help tab, while open (UI-HELP-001).
    help: Option<help::HelpTab>,
    /// The units of the model a script runs on, for its forms.
    script_units: Rc<RefCell<reviera_core::units::UnitSettings>>,
    /// Drives animated view changes and walking.
    timer: Option<Timer<Frame>>,
    /// Models being opened in the background, and the timer that animates their tabs.
    loading: Vec<loading::Loading>,
    load_timer: Option<Timer<Frame>>,
    /// The Project Browser.
    browser: TreeCtrl,
    /// The ribbon (built, like the menus and the Quick Access Toolbar, from the catalog).
    ribbon: ribbon::Ribbon,
    browser_stale: bool,
    /// The view a Project Browser context menu acts on.
    browser_target: Option<ElementId>,
    /// Feature edges of each model's scene, for plan graphics (computed when a plan needs them).
    plan_edges: HashMap<DocumentId, Rc<Vec<reviera_render::plan::Edge>>>,
    /// Letters typed toward a keyboard sequence such as `ZF` (§1.26.4), and when.
    keys: (String, std::time::Instant),
    /// A slice tool waiting for its click.
    slice_pick: Option<inspect::SliceKind>,
    /// The tiled arrangement Tab Views left, for Tile to restore.
    tiled: Option<wx::Layout>,
    /// Walls waiting for the roof they attach to (Attach Top/Base).
    attach_pick: Option<Vec<ElementId>>,
    /// A wall waiting for its new host (Pick New Host).
    host_pick: Option<ElementId>,
    /// Model Text waiting for its click: text, height, depth.
    text_pick: Option<(String, f64, f64)>,
    /// Place Group: the group type placed at each click.
    group_pick: Option<String>,
    /// Site Component: the component type being placed.
    site_pick: Option<ElementId>,
    /// The Area tool: its area plan and the area type it places.
    area_pick: Option<(ElementId, String)>,
    /// The Location Line the Wall tool draws on (kept between uses).
    wall_location: reviera_core::walls::LocationLine,
    /// The Model Tree pane (UI-TREE-001) and its state.
    model_tree: TreeCtrl,
    tree: inspect::ModelTree,
    /// Runs a menu id as the menus do (set once the menus are bound).
    menu_runner: Option<MenuRunner>,
    /// The open radial menu (UI-SEL-007).
    radial: Option<radial::RadialMenu>,
    /// Per-user interface settings (the experimental radial menu, the reveal dwell).
    ui_settings: radial::UiSettings,
    /// Full-screen and focus mode (UI-DOCK-004).
    chrome: fullscreen::Chrome,
    /// The System Browser (MEP-SYS-003), made on first use.
    system_browser: Option<mep_ui::SystemBrowser>,
    /// MEP check markers turned on, by view.
    mep_marks: HashMap<u32, mep_ui::MepMarks>,
    /// The System Inspector session (MEP-CALC-003), in one view.
    inspector: Option<mep_network_ui::Inspector>,
    /// The Sizing Calculator palette (MEP-CALC-005), made on first use.
    calculator: Option<mep_calculator_ui::SizingCalculator>,
    /// The Terminal pane (UI-AGENT-004), made on first use.
    terminal: Option<terminal_ui::TerminalPane>,
}

impl View {
    /// A plan, section, elevation or sheet: a 2D drawing navigated by pan and zoom.
    fn is_2d(&self) -> bool {
        self.plan.is_some() || self.section.is_some() || self.sheet.is_some()
    }
}

/// An empty scene and a default camera, for when no view is open.
static EMPTY: std::sync::LazyLock<RenderScene> = std::sync::LazyLock::new(RenderScene::default);
static DEFAULT_CAMERA: std::sync::LazyLock<Camera> = std::sync::LazyLock::new(Camera::default);

fn user() -> Caller {
    Caller::user()
}

impl Gui {
    fn run(&mut self, name: &str, args: Value) -> Result<Value, String> {
        self.app
            .execute(name, args, &user())
            .map(|o| o.result)
            .map_err(|e| e.to_string())
    }

    fn view(&self, id: u32) -> Option<&View> {
        self.views.iter().find(|v| v.id == id)
    }

    fn active_view(&self) -> Option<&View> {
        self.active.and_then(|id| self.view(id))
    }

    fn active_view_mut(&mut self) -> Option<&mut View> {
        self.active
            .and_then(|id| self.views.iter_mut().find(|v| v.id == id))
    }

    /// The camera of the active view.
    fn camera(&self) -> &Camera {
        self.active_view().map_or(&DEFAULT_CAMERA, |v| &v.camera)
    }

    /// The viewport of the active view.
    fn viewport(&mut self) -> Option<&mut WgpuViewport> {
        self.active_view_mut()?.viewport.as_mut()
    }

    /// The scene of the active model.
    fn scene(&self) -> &RenderScene {
        self.doc.and_then(|d| self.scenes.get(&d)).unwrap_or(&EMPTY)
    }

    /// Draws a view, creating its surface on first use (the panel must be realised by then).
    fn draw(&mut self, id: u32) {
        let frame = self.frame;
        let Some(index) = self.views.iter().position(|v| v.id == id) else {
            return;
        };
        // A schedule is a grid, not a viewport.
        if self.views[index].schedule.is_some() {
            return;
        }
        let doc = self.views[index].doc;
        let needs_surface =
            self.views[index].viewport.is_none() && self.views[index].error.is_none();
        let (scene, highlight) = if needs_surface {
            let highlight = if Some(doc) == self.doc {
                self.selection()
            } else {
                Vec::new()
            };
            (
                self.scenes.get(&doc).cloned().unwrap_or_default(),
                highlight,
            )
        } else {
            (RenderScene::default(), Vec::new())
        };
        let gpu = &mut self.gpu;
        let view = &mut self.views[index];
        let size = view.panel.get_client_size();
        if needs_surface {
            let created = native::NativeView::new(view.panel.get_handle()).and_then(|native| {
                let (w, h) = physical(size, native.scale_factor());
                let viewport = unsafe {
                    WgpuViewport::for_window_shared(gpu, native.display, native.window, w, h)
                }?;
                Ok((native, viewport))
            });
            match created {
                Ok((native, mut v)) => {
                    v.set_scene(&scene);
                    v.set_highlight(&highlight);
                    frame.set_status_text(
                        &format!("Graphics: {} on {}", v.adapter(), surface_kind(&native)),
                        1,
                    );
                    view.viewport = Some(v);
                    view.native = Some(native);
                }
                Err(e) => {
                    frame.set_status_text(&format!("3D viewport unavailable: {e}"), 0);
                    view.error = Some(e);
                }
            }
        }
        let Some(scale) = view.native.as_ref().map(native::NativeView::scale_factor) else {
            return;
        };
        if let (Some(v), Some(native)) = (&mut view.viewport, &view.native) {
            native.update();
            let (w, h) = physical(size, scale);
            v.resize(w, h);
        }
        self.update_plan_graphics(index);
        self.update_section_graphics(index);
        self.update_sheet_graphics(index);
        self.update_inspection(index);
        self.update_gizmo(index, scale);
        self.update_hud(index, scale);
        let view = &mut self.views[index];
        if let Some(v) = &mut view.viewport
            && let Err(e) = v.render(&view.camera)
        {
            frame.set_status_text(&e, 0);
        }
        // Navigation is view state of the session, saved with the next save (UI-NAV-001).
        let aspect = size.width.max(1) as f32 / size.height.max(1) as f32;
        let stored = view.camera.to_view(aspect);
        if let Some(element) = view.element
            && view.synced != Some(stored)
        {
            view.synced = Some(stored);
            let doc = view.doc;
            self.app.set_view_camera(doc, element, stored);
        }
    }

    /// A keyboard sequence letter (§1.26.4); returns whether it completed a known sequence.
    fn sequence_key(&mut self, c: char) -> bool {
        if self.keys.1.elapsed() > std::time::Duration::from_millis(1500) {
            self.keys.0.clear();
        }
        self.keys.0.push(c.to_ascii_uppercase());
        self.keys.1 = std::time::Instant::now();
        let frame = self.frame;
        let done = match self.keys.0.as_str() {
            "ZF" | "ZA" | "ZE" => {
                self.zoom_to_fit();
                true
            }
            "ZP" => {
                self.history(true);
                true
            }
            "XY" => {
                self.coordinates();
                true
            }
            "ZN" => {
                self.history(false);
                true
            }
            "WA" => {
                self.start_wall_tool(&frame);
                true
            }
            "WK" => {
                self.toggle_walk();
                true
            }
            "PI" => {
                self.toggle_perspective();
                true
            }
            "DI" => {
                self.start_dim_tool(&frame);
                true
            }
            "LL" => {
                self.new_level();
                true
            }
            "GR" => {
                self.start_grid_tool();
                true
            }
            "RP" => {
                self.start_line_tool(datum_tools::LineMode::RefPlane);
                true
            }
            "LI" => {
                self.start_line_tool(datum_tools::LineMode::ModelLine);
                true
            }
            "FL" => {
                self.start_floor_sketch();
                true
            }
            "DR" => {
                self.start_insert_tool(reviera_core::inserts::Kind::Door);
                true
            }
            "MV" | "CO" | "CC" | "RO" | "MM" | "DM" | "AL" | "OF" | "TR" | "SL" => {
                let action = match self.keys.0.as_str() {
                    "MV" => "element.move",
                    "CO" | "CC" => "element.copy",
                    "RO" => "element.rotate",
                    "MM" => "element.mirror_pick",
                    "DM" => "element.mirror_draw",
                    "AL" => "element.align",
                    "OF" => "element.offset",
                    "TR" => "element.trim_corner",
                    _ => "element.split",
                };
                if let Some(kind) = modify::Kind::of_action(action) {
                    self.start_modify(kind);
                }
                true
            }
            "PN" | "UP" => {
                let pin = self.keys.0 == "PN";
                self.pin_selection(pin);
                true
            }
            "WN" => {
                self.start_insert_tool(reviera_core::inserts::Kind::Window);
                true
            }
            "HH" | "HC" | "HI" | "IC" | "HT" | "HG" => {
                let action = match self.keys.0.as_str() {
                    "HH" => "inspect.hide",
                    "HC" => "inspect.hide_category",
                    "HI" => "inspect.isolate",
                    "IC" => "inspect.isolate_category",
                    "HT" => "inspect.translucent",
                    _ => "inspect.ghost_others",
                };
                self.temp_action(action);
                true
            }
            "HR" => {
                self.temp_reset();
                true
            }
            "RM" => {
                self.start_room_tool();
                true
            }
            "BX" => {
                self.selection_box();
                true
            }
            "SX" => {
                self.toggle_section_box();
                true
            }
            "MT" => {
                self.toggle_pane("model_tree");
                true
            }
            "Z" | "W" | "P" | "D" | "L" | "G" | "M" | "C" | "R" | "A" | "O" | "T" | "S" | "U"
            | "F" | "H" | "I" | "B" | "X" => false,
            _ => {
                self.keys.0.clear();
                false
            }
        };
        if done {
            self.keys.0.clear();
        }
        done
    }

    fn new_project(&mut self, frame: &Frame) {
        match self.run("file.new", json!({})) {
            Ok(id) => self.show_document(id.as_u64().unwrap_or_default(), frame),
            Err(e) => error(frame, &format!("Could not create a project:\n{e}")),
        }
    }

    /// Shows a newly opened model: its scene, and a 3D view tab.
    fn show_document(&mut self, id: DocumentId, frame: &Frame) {
        let scene = match self.app.render_scene(id) {
            Ok(s) => s,
            Err(e) => return error(frame, &format!("Could not display the model:\n{e}")),
        };
        let triangles = scene.triangle_count();
        self.scenes.insert(id, scene);
        let views = self
            .app
            .document(id)
            .map(|d| reviera_core::views::list(d.doc.store()))
            .unwrap_or_default();
        let first = views
            .iter()
            .find(|v| v.name == reviera_core::views::DEFAULT_3D)
            .or_else(|| {
                views
                    .iter()
                    .find(|v| v.kind == reviera_core::views::KIND_3D)
            })
            .or_else(|| views.first())
            .map(|v| v.id);
        self.open_view(id, first);
        // Grids another tool edited follow the IFC data; say what changed.
        let reconciled = self
            .app
            .document(id)
            .and_then(|d| d.open_report.as_ref())
            .map(|r| r.reconciled.clone())
            .unwrap_or_default();
        if !reconciled.is_empty() {
            let dialog = MessageDialog::builder(
                frame,
                &format!(
                    "The file was edited by another tool since Reviera saved it. These changes \
                     were taken from the IFC data and are saved with the next save:\n\n{}",
                    reconciled.join("\n")
                ),
                "BIM Reviera",
            )
            .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
            .build();
            dialog.show_modal();
        }
        self.update_ribbon_context();
        let elements = self.app.document(id).map_or(0, |d| d.doc.store().len());
        frame.set_status_text(
            &format!(
                "{}: {elements} elements, {triangles} triangles",
                self.doc_title(id)
            ),
            0,
        );
    }

    fn doc_info(&self, id: DocumentId) -> Option<reviera_app::DocumentInfo> {
        self.app.documents().into_iter().find(|d| d.id == id)
    }

    fn doc_title(&self, id: DocumentId) -> String {
        self.doc_info(id).map_or("Untitled".into(), |d| d.title)
    }

    /// The tab caption of a view: `{3D} - Model`, with `*` for unsaved changes.
    fn caption(&self, view: &View) -> String {
        let dirty = self.doc_info(view.doc).is_some_and(|d| d.dirty);
        format!(
            "{} - {}{}{}",
            self.view_name(view),
            self.doc_title(view.doc),
            if dirty { " *" } else { "" },
            self.temp_mark(view),
        )
    }

    /// A view's name: its model view's name, or `{3D}` for an unsaved one.
    fn view_name(&self, view: &View) -> String {
        view.element
            .and_then(|e| {
                let e = self.app.document(view.doc)?.doc.store().get(e)?;
                let name = e.name.clone()?;
                // A sheet is known by its number and name.
                Some(match reviera_core::sheets::sheet_data(e) {
                    Some(d) => format!("{} - {name}", d.number),
                    None => name,
                })
            })
            .unwrap_or_else(|| view.name.clone())
    }

    /// Opens `element` (a model view of `doc`) in a tab and activates it; an open view's tab is
    /// brought to the front. `None` opens an unsaved `{3D}` view.
    fn open_view(&mut self, doc: DocumentId, element: Option<ElementId>) {
        if let Some(e) = element
            && let Some(id) = self
                .views
                .iter()
                // Element ids are per model: the view must be this model's.
                .find(|v| v.doc == doc && v.element == Some(e))
                .map(|v| v.id)
        {
            return self.activate_view(id);
        }
        let panel = Panel::builder(&self.notebook).build();
        wx::wants_chars(&panel);
        panel.set_background_style(BackgroundStyle::Paint);
        let id = self.next_view;
        self.next_view += 1;
        // A window's shape is not known before it is shown; views keep their horizontal extent.
        let aspect = 1.6;
        let mut camera = Camera::default();
        match self.scenes.get(&doc).and_then(RenderScene::bounds) {
            Some(b) => camera.fit(b),
            // An empty project: look at the origin from above at an angle.
            None => camera.fit((
                glam::Vec3::new(-10.0, -10.0, 0.0),
                glam::Vec3::new(10.0, 10.0, 3.0),
            )),
        }
        let plan = self.plan_info(doc, element);
        let section = self.section_info(doc, element);
        let sheet = self.sheet_info(doc, element);
        let schedule = self
            .is_schedule(doc, element)
            .then(|| Gui::schedule_grid(&panel));
        if plan.is_some() || section.is_some() || sheet.is_some() {
            camera.perspective = false;
            camera.orient(glam::Vec3::Z);
        }
        // A sheet shows its whole paper.
        if let Some(s) = &sheet {
            sheets::fit_paper(&mut camera, s.paper, aspect);
        }
        // A section or elevation frames the model as seen in its own frame.
        if let (Some(s), Some((lo, hi))) = (
            &section,
            self.scenes.get(&doc).and_then(RenderScene::bounds),
        ) {
            let f = s.def.frame();
            let corners: Vec<[f64; 3]> = (0..8)
                .map(|k| {
                    let pick = |bit: usize, a: f32, b: f32| {
                        f64::from(if k >> bit & 1 == 1 { b } else { a })
                    };
                    f.to_local([
                        pick(0, lo.x, hi.x),
                        pick(1, lo.y, hi.y),
                        pick(2, lo.z, hi.z),
                    ])
                })
                .collect();
            let min = |i: usize| corners.iter().map(|c| c[i]).fold(f64::MAX, f64::min) as f32;
            let max = |i: usize| corners.iter().map(|c| c[i]).fold(f64::MIN, f64::max) as f32;
            camera.fit((
                glam::Vec3::new(min(0).max(-1.0), min(1) - 1.0, -1.0),
                glam::Vec3::new(max(0).min(f.width as f32 + 1.0), max(1) + 2.0, 0.0),
            ));
            camera.orient(glam::Vec3::Z);
        }
        let saved = element.and_then(|e| self.app.view_camera(doc, e));
        if let Some(c) = &saved {
            camera = Camera::from_view(c, aspect);
        }
        let mut nav = nav::Nav::default();
        nav.home = element
            .and_then(|e| self.app.view_home(doc, e))
            .map(|h| Camera::from_view(&h, aspect));
        let view = View {
            id,
            doc,
            element,
            name: reviera_core::views::DEFAULT_3D.into(),
            synced: saved,
            plan,
            section,
            schedule,
            sheet,
            panel,
            viewport: None,
            native: None,
            error: None,
            camera,
            drag: None,
            clip_edit: None,
            rebar_picks: Vec::new(),
            press: None,
            nav,
        };
        let caption = self.caption(&view);
        let grid = view.schedule.as_ref().map(|s| s.grid);
        self.views.push(view);
        if let Some(gui) = self.this.upgrade() {
            attach_view_handlers(&gui, id, panel, self.frame);
            if let Some(grid) = grid {
                let g = gui.clone();
                grid.on_cell_changed(move |e| {
                    let (row, col) = (e.get_row(), e.get_col());
                    if let Ok(mut gui) = g.try_borrow_mut() {
                        gui.schedule_cell_changed(id, row, col);
                    }
                });
                let g = gui.clone();
                grid.on_select_cell(move |e| {
                    let row = e.get_row();
                    if let Ok(mut gui) = g.try_borrow_mut() {
                        gui.schedule_row_selected(id, row);
                    }
                });
            }
        }
        self.notebook.add_page(&panel, &caption, true);
        if grid.is_some() {
            self.fill_schedule(id);
        }
        self.activate_view(id);
        self.browser_stale = true;
    }

    /// Makes a view the active one; its model becomes the active document.
    fn activate_view(&mut self, id: u32) {
        let Some(doc) = self.view(id).map(|v| v.doc) else {
            return;
        };
        let frame = self.frame;
        if self.active != Some(id) {
            if self.doc != Some(doc) {
                self.end_tool(&frame);
            }
            if self.doc != Some(doc) {
                self.browser_stale = true;
            }
            self.end_dim_tool();
            self.end_grid_tool();
            self.end_line_tool();
            self.end_insert_tool();
            self.end_modify();
            self.cancel_sketch();
            self.end_ceiling_tool();
            self.end_room_tool();
            self.active = Some(id);
            self.doc = Some(doc);
            let _ = self.app.activate(doc);
            let element = self.view(id).and_then(|v| v.element);
            self.app.set_active_view(doc, element);
            self.tree.stale = true;
            self.slice_pick = None;
            self.update_title(&frame);
            self.properties_stale = true;
            self.update_ribbon_context();
        }
        if let Some(i) = self
            .view(id)
            .and_then(|v| wx::page_index(&self.notebook, &v.panel))
            && self.notebook.get_selection() != i as i32
        {
            self.notebook.set_selection(i);
        }
    }

    /// Updates the tab captions (titles and unsaved-changes marks).
    fn update_captions(&self) {
        for v in &self.views {
            if let Some(i) = wx::page_index(&self.notebook, &v.panel) {
                let caption = self.caption(v);
                if self.notebook.get_page_text(i) != caption {
                    self.notebook.set_page_text(i, &caption);
                }
            }
        }
    }

    /// Closes a view. Closing the last view of a model closes the model, asking to save unsaved
    /// changes first. Returns `false` when the user cancelled.
    fn close_view(&mut self, id: u32) -> bool {
        let Some(doc) = self.view(id).map(|v| v.doc) else {
            return true;
        };
        // The model's last view closes the model.
        if self.views.iter().filter(|v| v.doc == doc).count() == 1 && !self.close_document(doc) {
            return false;
        }
        self.close_tab(id);
        true
    }

    /// Closes model `doc`, asking to save its changes first; false when cancelled or refused.
    /// Its tabs are left to the caller.
    fn close_document(&mut self, doc: DocumentId) -> bool {
        let frame = self.frame;
        let info = self.doc_info(doc);
        let mut save = "no";
        if info.as_ref().is_some_and(|d| d.dirty) {
            let message = format!("Save changes to {} before closing?", self.doc_title(doc));
            let dialog = MessageDialog::builder(&frame, &message, "BIM Reviera")
                .with_style(
                    MessageDialogStyle::YesNo
                        | MessageDialogStyle::Cancel
                        | MessageDialogStyle::IconQuestion,
                )
                .build();
            match dialog.show_modal() {
                ID_YES => save = "yes",
                ID_NO => {}
                _ => return false,
            }
            if save == "yes" && info.as_ref().is_some_and(|d| d.path.is_none()) {
                error(&frame, "Save the new project with File > Save As first.");
                return false;
            }
        }
        if let Err(e) = self.run("file.close", json!({ "document_id": doc, "save": save })) {
            error(&frame, &format!("Could not close the model:\n{e}"));
            return false;
        }
        self.scenes.remove(&doc);
        true
    }

    /// File › Close: closes the active model and all its tabs, asking to save its changes.
    fn close_model(&mut self) {
        let Some(doc) = self.doc else {
            return self.frame.set_status_text("No model is open", 0);
        };
        let title = self.doc_title(doc);
        if !self.close_document(doc) {
            return;
        }
        let tabs: Vec<u32> = self
            .views
            .iter()
            .filter(|v| v.doc == doc)
            .map(|v| v.id)
            .collect();
        for id in tabs {
            self.close_tab(id);
        }
        // Another model's view takes over (the panes follow it), or nothing is open.
        match self.active.or_else(|| self.views.first().map(|v| v.id)) {
            Some(v) => self.activate_view(v),
            None => {
                self.doc = None;
                self.properties_stale = true;
                self.browser_stale = true;
            }
        }
        self.frame.set_status_text(&format!("Closed {title}"), 0);
    }

    /// Closes a view's tab (the model stays open).
    fn close_tab(&mut self, id: u32) {
        let frame = self.frame;
        if self.active == Some(id) {
            self.end_tool(&frame);
            self.active = None;
            self.doc = None;
        }
        let Some(index) = self.views.iter().position(|v| v.id == id) else {
            return;
        };
        // The surface goes before its window.
        let view = self.views.remove(index);
        let panel = view.panel;
        drop(view);
        if let Some(i) = wx::page_index(&self.notebook, &panel) {
            self.notebook.delete_page(i);
        }
        // The notebook selects a neighbour: make it the active view.
        let selected = usize::try_from(self.notebook.get_selection()).ok();
        match selected.and_then(|i| self.view_at(i)) {
            Some(v) => self.activate_view(v),
            None if self.views.is_empty() => {
                frame.set_title("BIM Reviera");
                self.properties_stale = true;
            }
            None => {}
        }
        self.browser_stale = true;
    }

    /// Closes every view; `false` when the user cancelled.
    fn close_all(&mut self) -> bool {
        // Agents running in the Terminal pane stop with Reviera: asked first.
        let running = self.terminal.as_ref().map_or(0, |t| t.tabs.len());
        if running > 0 && !ask(&self.frame, &format!("{running} terminal tab(s) are open. Close them and exit?")) {
            return false;
        }
        while let Some(id) = self.views.last().map(|v| v.id) {
            if !self.close_view(id) {
                return false;
            }
        }
        self.close_all_terminals();
        true
    }

    /// The view on notebook page `index`.
    fn view_at(&self, index: usize) -> Option<u32> {
        self.views
            .iter()
            .find(|v| wx::page_index(&self.notebook, &v.panel) == Some(index))
            .map(|v| v.id)
    }

    /// Activates the next (`step` 1) or previous (-1) view in tab order.
    fn cycle_view(&mut self, step: i32) {
        let n = self.notebook.page_count() as i32;
        if n < 2 {
            return;
        }
        let current = self.notebook.get_selection().max(0);
        let next = (current + step).rem_euclid(n) as usize;
        if let Some(v) = self.view_at(next) {
            self.activate_view(v);
        }
    }

    /// Tile Views: every view visible at once, in columns, then rows (UI-VIEWWIN-002).
    fn tile(&mut self) {
        // The tiling last left by Tab Views, with its dividers, while the same views are open.
        if let Some(l) = &self.tiled
            && l.covers(&self.notebook)
        {
            wx::unsplit_all(&self.notebook);
            l.restore(&self.notebook);
            return;
        }
        wx::unsplit_all(&self.notebook);
        let n = self.notebook.page_count();
        if n < 2 {
            return;
        }
        let columns = (n as f64).sqrt().ceil() as usize;
        // Splitting moves a page into a new tab group; going from the last page keeps the
        // indices of the pages still to move.
        for k in (1..n).rev() {
            let side = if k < columns {
                wx::Side::Right
            } else {
                wx::Side::Bottom
            };
            wx::split(&self.notebook, k, side);
        }
    }

    /// Tab Views: every view back in one tab group. A tiled arrangement is kept, so Tile can
    /// bring it back with its dividers where they were.
    fn tab_all(&mut self) {
        let layout = wx::save_layout(&self.notebook);
        if layout.groups > 1 {
            self.tiled = Some(layout);
        }
        wx::unsplit_all(&self.notebook);
    }

    /// The active document's file, or `None` when it has never been saved.
    fn current_path(&self) -> Option<PathBuf> {
        let id = self.doc?;
        self.app.documents().into_iter().find(|d| d.id == id)?.path
    }

    fn save(&mut self, frame: &Frame) {
        match self.run("file.save", json!({})) {
            Ok(v) => frame.set_status_text(
                &format!("Saved {}", v["path"].as_str().unwrap_or_default()),
                0,
            ),
            Err(e) => error(frame, &format!("Could not save:\n{e}")),
        }
    }

    fn save_as(&mut self, path: &Path, frame: &Frame) {
        let mut path = path.to_owned();
        if path.extension().is_none() {
            path.set_extension("ifc");
        }
        match self.run("file.save_as", json!({ "path": path })) {
            Ok(_) => {
                self.update_title(frame);
                frame.set_status_text(&format!("Saved {}", path.display()), 0);
            }
            Err(e) => error(frame, &format!("Could not save:\n{e}")),
        }
    }

    fn update_title(&self, frame: &Frame) {
        let Some(id) = self.doc else { return };
        let info = self.app.documents().into_iter().find(|d| d.id == id);
        let title = info.as_ref().map_or("Untitled".into(), |d| d.title.clone());
        let read_only = info.is_some_and(|d| d.read_only);
        frame.set_title(&format!(
            "{title}{} - BIM Reviera",
            if read_only { " (read-only)" } else { "" }
        ));
    }

    /// Rebuilds the active model's scene after it changed (edits, undo, redo), in all its views.
    fn refresh_scene(&mut self, frame: &Frame) {
        let Some(id) = self.doc else { return };
        let scene = match self.app.render_scene(id) {
            Ok(scene) => scene,
            Err(e) => {
                return frame.set_status_text(&format!("Could not display the model: {e}"), 0);
            }
        };
        let ids = self.selection();
        for view in self.views.iter_mut().filter(|v| v.doc == id) {
            if let Some(s) = &mut view.section {
                // Sections draw the model in their own frame, rebuilt when drawn.
                s.stale = true;
            } else if let Some(s) = &mut view.sheet {
                // Sheets draw their viewports, rebuilt when drawn.
                s.stale = true;
            } else if let Some(v) = &mut view.viewport {
                v.set_scene(&scene);
                v.set_highlight(&ids);
            }
            view.panel.refresh(false, None);
        }
        self.scenes.insert(id, scene);
        self.plans_stale(id);
        self.refresh_plan_states();
        self.update_captions();
        self.properties_stale = true;
        self.browser_stale = true;
        self.tree.stale = true;
        self.sync_views();
        self.refresh_schedules();
        self.auto_analytical_check();
    }

    fn undo_redo(&mut self, command: &str, frame: &Frame) {
        if self.doc.is_none() {
            return;
        }
        match self.run(command, json!({})) {
            Ok(_) => self.refresh_scene(frame),
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Starts the Wall tool on the lowest level.
    fn start_wall_tool(&mut self, frame: &Frame) {
        self.start_shaped_wall_tool(frame, WallShape::Line);
    }

    fn start_shaped_wall_tool(&mut self, frame: &Frame, shape: WallShape) {
        self.structural_walls = false;
        self.leave_walk();
        self.end_dim_tool();
        self.end_grid_tool();
        self.end_line_tool();
        self.end_insert_tool();
        self.end_modify();
        let Some(doc) = self.doc.and_then(|id| self.app.document(id)) else {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        };
        let mut levels: Vec<(f64, ElementId, String)> = doc
            .doc
            .store()
            .iter()
            .filter(|e| e.class == "IfcBuildingStorey")
            .map(|e| {
                let z = e.param("@Elevation").and_then(Value::as_f64).unwrap_or(0.0);
                let name = e
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Level {}", e.id()));
                (z, e.id(), name)
            })
            .collect();
        levels.sort_by(|a, b| a.0.total_cmp(&b.0));
        let in_plan = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| (p.elevation, p.level, p.level_name.clone()));
        let chosen = in_plan.or_else(|| levels.first().cloned());
        let Some((elevation, level, level_name)) = chosen else {
            return frame.set_status_text("The model has no levels to draw walls on", 0);
        };
        let top = levels.iter().find(|l| l.0 > elevation + 1e-6).map(|l| l.1);
        self.tool = Some(WallTool {
            level,
            level_name,
            elevation,
            top,
            last: None,
            cursor: None,
            flip: false,
            typed: String::new(),
            lines: Vec::new(),
            shape,
            pending: Vec::new(),
        });
        self.update_wall_lines();
        self.clear_selection();
        self.properties_stale = true;
        self.tool_prompt(frame);
    }

    /// Caches the location lines of the walls on the tool's level, for snapping.
    fn update_wall_lines(&mut self) {
        let Some(t) = &self.tool else { return };
        let Some(doc) = self.doc.and_then(|id| self.app.document(id)) else {
            return;
        };
        let store = doc.doc.store();
        let level_guid = store.get(t.level).map(|e| e.guid().as_str().to_owned());
        let mut lines: Vec<([f64; 2], [f64; 2])> = store
            .iter()
            .filter_map(reviera_core::walls::wall_recipe)
            .filter(|r| Some(&r.base_level) == level_guid.as_ref())
            .map(|r| (r.curve.start, r.curve.end))
            .collect();
        // In a plan, the cut edges of every element too (imported walls, columns, ...).
        if let Some(plan) = self.active_view().and_then(|v| v.plan.as_ref()) {
            let p = |q: [f32; 2]| [f64::from(q[0]), f64::from(q[1])];
            lines.extend(plan.cut_edges.iter().map(|(_, s)| (p(s[0]), p(s[1]))));
        }
        self.tool.as_mut().expect("tool").lines = lines;
    }

    fn tool_prompt(&mut self, frame: &Frame) {
        let Some(t) = &self.tool else { return };
        let exterior = if t.flip { "right" } else { "left" };
        let text = if t.last.is_some() && t.shape == WallShape::Arc && !t.pending.is_empty() {
            "Arc wall: click a point the arc passes through (Esc cancels)".to_owned()
        } else if t.last.is_some() && t.shape == WallShape::Arc {
            "Arc wall: click the arc's end".to_owned()
        } else if t.last.is_some() && t.shape == WallShape::Spline {
            format!(
                "Spline wall: click the points it passes through; Enter ends at the last ({} so far)",
                t.pending.len()
            )
        } else if !t.typed.is_empty() {
            format!("Wall length: {}_  (Enter to place, Esc to cancel)", t.typed)
        } else if t.last.is_none() {
            format!(
                "Wall on {}: click the start point. Exterior on the {exterior} (Spacebar flips).",
                t.level_name
            )
        } else {
            let mut text = String::from("Click the next point or type a length");
            if let (Some(start), Some(c)) = (t.last, t.cursor) {
                let length = (c.point[0] - start[0]).hypot(c.point[1] - start[1]);
                let shown = self
                    .app
                    .execute(
                        "units.format",
                        json!({"value": length, "data_type": "length"}),
                        &user(),
                    )
                    .ok()
                    .and_then(|o| o.result.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("{length:.3} m"));
                text = format!("Length {shown}");
                if c.kind != snap::SnapKind::None {
                    text += &format!(" · {}", c.kind.label());
                }
            }
            format!("{text}. Exterior on the {exterior} (Spacebar flips). Esc ends the chain.")
        };
        frame.set_status_text(&text, 0);
    }

    fn end_tool(&mut self, frame: &Frame) {
        self.end_stair_tool();
        self.end_dormer_tool();
        self.end_railing_tool();
        self.end_run_tool();
        self.end_feature_tool();
        self.end_dome_tool();
        self.end_column_tool();
        self.end_beam_tool();
        self.end_footing_tool();
        self.end_beam_system_tool();
        self.end_truss_tool();
        self.end_panel_tool();
        self.end_load_tool();
        self.end_steel_tool();
        self.end_rebar_sketch();
        self.end_bar_pick();
        self.end_rebar_annotation();
        self.end_rebar_host_pick();
        self.end_coupler();
        self.end_curtain_tool();
        self.end_ramp_tool();
        self.end_measure_tool();
        self.end_skylight_tool();
        self.end_place_tool();
        self.end_array_tool();
        if self.tool.take().is_some() {
            if let Some(v) = self.viewport() {
                v.set_overlay(&[]);
            }
            frame.set_status_text("Ready", 0);
        }
    }

    /// Esc: clears a typed length, then ends the wall chain, then the tool; without a tool, clears
    /// the selection.
    fn escape(&mut self, frame: &Frame) {
        if self.slice_pick.take().is_some() {
            return frame.set_status_text("Slice cancelled", 0);
        }
        if self.end_section_tool() {
            return frame.set_status_text("Section cancelled", 0);
        }
        if self.end_note_tool() {
            return frame.set_status_text("Ready", 0);
        }
        if self.stair_tool.is_some() {
            return self.stair_escape();
        }
        if self.end_dormer_tool() {
            return frame.set_status_text("Ready", 0);
        }
        if self.railing_tool.is_some() {
            return self.railing_escape();
        }
        if self.run_tool.is_some() {
            return self.run_escape();
        }
        if self.curtain_escape()
            || self.measure_escape()
            || self.ramp_key(WXK_ESCAPE)
            || self.end_skylight_tool()
            || self.end_place_tool()
            || self.end_array_tool()
        {
            return;
        }
        if self.end_feature_tool()
            || self.end_dome_tool()
            || self.end_column_tool()
            || self.end_beam_tool()
            || self.end_footing_tool()
            || self.end_beam_system_tool()
            || self.end_truss_tool()
            || self.end_panel_tool()
            || self.end_load_tool()
            || self.end_steel_tool()
            || self.end_rebar_sketch()
            || self.end_bar_pick()
            || self.end_rebar_annotation()
            || self.end_rebar_host_pick()
            || self.end_coupler()
            || self.cancel_adjust()
        {
            return frame.set_status_text("Ready", 0);
        }
        if self.attach_pick.take().is_some() {
            return frame.set_status_text("Attach cancelled", 0);
        }
        if self.host_pick.take().is_some() {
            return frame.set_status_text("Pick New Host cancelled", 0);
        }
        if self.area_pick.take().is_some()
            || self.group_pick.take().is_some()
            || self.site_pick.take().is_some()
            || self.text_pick.take().is_some()
        {
            return frame.set_status_text("Ready", 0);
        }
        match &mut self.tool {
            Some(t) if !t.typed.is_empty() => {
                t.typed.clear();
                self.tool_prompt(frame);
            }
            Some(t) if t.last.is_some() => {
                t.last = None;
                self.update_overlay();
                self.tool_prompt(frame);
            }
            Some(_) => self.end_tool(frame),
            // A reveal goes first; with nothing selected, Esc leaves full-screen or focus mode.
            None if self.chrome.revealed.is_some() => self.reveal(None),
            None => {
                if self.chrome.mode != fullscreen::Mode::Off && self.selection().is_empty() {
                    self.leave_chrome();
                } else {
                    self.clear_selection();
                }
            }
        }
    }

    /// Whether a tool that takes clicked points is in progress.
    pub(crate) fn tool_active(&self) -> bool {
        self.adjust.is_some()
            || self.bar_pick.is_some()
            || self.rebar_anno.is_some()
            || self.rebar_host_pick.is_some()
            || self.end_pick.is_some()
            || self.slice_pick.is_some()
            || self.attach_pick.is_some()
            || self.host_pick.is_some()
            || self.area_pick.is_some()
            || self.group_pick.is_some()
            || self.site_pick.is_some()
            || self.text_pick.is_some()
            || self.sketch.is_some()
            || self.ceiling_tool.is_some()
            || self.room_tool.is_some()
            || self.section_tool.is_some()
            || self.note_tool.is_some()
            || self.stair_tool.is_some()
            || self.dormer_tool.is_some()
            || self.railing_tool.is_some()
            || self.run_tool.is_some()
            || self.feature_tool.is_some()
            || self.dome_tool.is_some()
            || self.column_tool.is_some()
            || self.beam_tool.is_some()
            || self.footing_tool.is_some()
            || self.beam_system_tool.is_some()
            || self.truss_tool.is_some()
            || self.panel_tool.is_some()
            || self.load_tool.is_some()
            || self.steel_tool.is_some()
            || self.rebar_sketch.is_some()
            || self.curtain_tool.is_some()
            || self.ramp_tool.is_some()
            || self.measure_tool.is_some()
            || self.skylight_tool.is_some()
            || self.place_tool.is_some()
            || self.array_tool.is_some()
            || self.dim_tool.is_some()
            || self.grid_tool.is_some()
            || self.line_tool.is_some()
            || self.insert_tool.is_some()
            || self.modify_tool.is_some()
            || self.tool.is_some()
    }

    /// A key handed to the tools in progress; `true` when one used it.
    pub(crate) fn tools_key(&mut self, key: i32, shift: bool, f: &Frame) -> bool {
        let mut used = self.run_key(key) || self.adjust_key(key) || self.bar_pick_key(key) || self.rebar_host_key(key) || self.coupler_key(key) || self.steel_key(key) || self.rebar_sketch_key(key) || self.walk_key(key, true, shift);
        if !used && self.sketch.is_some() {
            let (mx, my) = self.mouse;
            used = self.sketch_key(key, mx, my);
        }
        if !used && matches!(key, WXK_BACK | WXK_RETURN | WXK_NUMPAD_ENTER) {
            used =
                self.modify_key(key) || self.typed_length_key(key) || self.line_enter(key);
        }
        if !used {
            let (mx, my) = self.mouse;
            used = self.column_key(key, mx, my) || self.place_key(key, mx, my);
        }
        if !used && matches!(key, WXK_RETURN | WXK_NUMPAD_ENTER) {
            used = self.ramp_key(key) || self.array_key(key);
        }
        if !used {
            used = self.insert_key(key);
        }
        if !used
            && matches!(key, WXK_SPACE | WXK_BACK | WXK_RETURN | WXK_NUMPAD_ENTER)
        {
            used = self.wall_key(key, f);
        }
        used
    }

    /// A click handed to the tool in progress (or a typed point fed to it, as if clicked);
    /// `false` when no tool takes clicks.
    pub(crate) fn tools_click(&mut self, x: i32, y: i32, p: &Panel, f: &Frame) -> bool {
        if self.adjust.is_some() {
            self.adjust_click(x, y);
        } else if self.bar_pick.is_some() {
            self.bar_pick_click(x, y);
        } else if self.rebar_anno.is_some() {
            self.rebar_annotation_click(x, y);
        } else if self.rebar_host_pick.is_some() {
            self.rebar_host_click(x, y, p);
        } else if self.end_pick.is_some() {
            self.coupler_click(x, y);
        } else if self.slice_pick.is_some() {
            self.slice_click(x, y);
        } else if self.attach_pick.is_some() {
            self.attach_click(x, y, p);
        } else if self.host_pick.is_some() {
            self.host_click(x, y, p);
        } else if self.area_pick.is_some() {
            self.area_click(x, y);
        } else if self.group_pick.is_some() {
            self.group_click(x, y);
        } else if self.site_pick.is_some() {
            self.site_click(x, y);
        } else if self.text_pick.is_some() {
            self.model_text_click(x, y);
        } else if self.sketch.is_some() {
            self.sketch_click(x, y);
        } else if self.ceiling_tool.is_some() {
            self.ceiling_click(x, y);
        } else if self.room_tool.is_some() {
            self.room_click(x, y);
        } else if self.section_tool.is_some() {
            self.section_click(x, y);
        } else if self.note_tool.is_some() {
            self.note_click(x, y, p);
        } else if self.stair_tool.is_some() {
            self.stair_click(x, y);
        } else if self.dormer_tool.is_some() {
            self.dormer_click(x, y, p);
        } else if self.railing_tool.is_some() {
            self.railing_click(x, y);
        } else if self.run_tool.is_some() {
            self.run_click(x, y);
        } else if self.feature_tool.is_some() {
            self.feature_click(x, y, p);
        } else if self.dome_tool.is_some() {
            self.dome_click(x, y);
        } else if self.column_tool.is_some() {
            self.column_click(x, y);
        } else if self.beam_tool.is_some() {
            self.beam_click(x, y);
        } else if self.footing_tool.is_some() {
            self.footing_click(x, y);
        } else if self.beam_system_tool.is_some() {
            self.beam_system_click(x, y);
        } else if self.truss_tool.is_some() {
            self.truss_click(x, y);
        } else if self.panel_tool.is_some() {
            self.panel_click(x, y);
        } else if self.load_tool.is_some() {
            self.load_click(x, y);
        } else if self.steel_tool.is_some() {
            self.steel_click(x, y);
        } else if self.rebar_sketch.is_some() {
            self.rebar_sketch_click(x, y);
        } else if self.curtain_tool.is_some() {
            self.curtain_click(x, y);
        } else if self.ramp_tool.is_some() {
            self.ramp_click(x, y);
        } else if self.measure_tool.is_some() {
            self.measure_click(x, y);
        } else if self.skylight_tool.is_some() {
            self.skylight_click(x, y);
        } else if self.place_tool.is_some() {
            self.place_click(x, y);
        } else if self.array_tool.is_some() {
            self.array_click(x, y, p);
        } else if self.dim_tool.is_some() {
            self.dim_click(x, y);
        } else if self.grid_tool.is_some() {
            self.grid_click(x, y);
        } else if self.line_tool.is_some() {
            self.line_click(x, y);
        } else if self.insert_tool.is_some() {
            self.insert_click(x, y);
        } else if self.modify_tool.is_some() {
            self.modify_click(x, y);
        } else if self.active_view().is_some_and(|v| v.nav.walk.is_some()) {
        } else if self.tool.is_some() {
            self.wall_click(x, y, p, f);
        } else {
            return false;
        }
        true
    }

    /// Mouse movement handed to the tools in progress; `true` when the view needs redrawing.
    pub(crate) fn tools_motion(&mut self, x: i32, y: i32, p: &Panel, f: &Frame) -> bool {
        let mut redraw = false;
        if self.inspector.is_some() {
            redraw |= self.inspector_motion(x, y, p);
        }
        if self.sketch.is_some() {
            self.sketch_motion(x, y);
            redraw = true;
        }
        if self.ceiling_tool.is_some() {
            self.ceiling_motion(x, y);
            redraw = true;
        }
        if self.room_tool.is_some() {
            self.room_motion(x, y);
            redraw = true;
        }
        if self.section_tool.is_some() {
            self.section_motion(x, y);
            redraw = true;
        }
        if self.stair_tool.is_some() {
            self.stair_motion(x, y);
            redraw = true;
        }
        if self.railing_tool.is_some() {
            self.railing_motion(x, y);
            redraw = true;
        }
        if self.run_tool.is_some() {
            self.run_motion(x, y);
            redraw = true;
        }
        if self.dome_tool.is_some() {
            self.dome_motion(x, y);
            redraw = true;
        }
        if self.column_tool.is_some() {
            self.column_motion(x, y);
            redraw = true;
        }
        if self.steel_tool.is_some() {
            self.steel_motion(x, y);
        }
        if self.rebar_sketch.is_some() {
            self.rebar_sketch_motion(x, y);
        }
        if self.beam_tool.is_some() {
            self.beam_motion(x, y);
            redraw = true;
        }
        if self.adjust.is_some() {
            self.adjust_motion(x, y);
            redraw = true;
        }
        if self.curtain_tool.is_some() {
            self.curtain_motion(x, y);
            redraw = true;
        }
        if self.ramp_tool.is_some() {
            self.ramp_motion(x, y);
            redraw = true;
        }
        if self.measure_tool.is_some() {
            self.measure_motion(x, y);
            redraw = true;
        }
        if self.place_tool.is_some() {
            self.place_motion(x, y);
            redraw = true;
        }
        if self.array_tool.is_some() {
            self.array_motion(x, y);
            redraw = true;
        }
        if self.dim_tool.is_some() {
            self.dim_motion(x, y);
            redraw = true;
        }
        if self.grid_tool.is_some() {
            self.grid_motion(x, y);
            redraw = true;
        }
        if self.line_tool.is_some() {
            self.line_motion(x, y);
            redraw = true;
        }
        if self.insert_tool.is_some() {
            self.insert_motion(x, y);
            redraw = true;
        }
        if self.modify_tool.is_some() {
            self.modify_motion(x, y);
            redraw = true;
        }
        if self.tool.is_some() {
            self.wall_motion(x, y, p, f);
            redraw = true;
        }
        redraw
    }

    /// The point on the tool's level plane under pixel (`x`, `y`).
    fn plane_point(&self, x: i32, y: i32, panel: &Panel) -> Option<[f64; 2]> {
        if let Some(p) = crate::coords::typed() {
            return Some(p);
        }
        let t = self.tool.as_ref()?;
        let size = panel.get_client_size();
        let (o, d) = self
            .camera()
            .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        if d.z.abs() < 1e-6 {
            return None;
        }
        let s = (t.elevation as f32 - o.z) / d.z;
        (s > 0.0).then(|| {
            let p = o + d * s;
            [f64::from(p.x), f64::from(p.y)]
        })
    }

    /// The snapped point under pixel (`x`, `y`).
    fn snapped(&self, x: i32, y: i32, panel: &Panel) -> Option<snap::Snap> {
        let t = self.tool.as_ref()?;
        let raw = self.plane_point(x, y, panel)?;
        let size = panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let z = t.elevation as f32;
        let to_screen = |p: [f64; 2]| {
            self.camera()
                .project(glam::Vec3::new(p[0] as f32, p[1] as f32, z), w, h)
        };
        let mut s = snap::snap(raw, (x as f32, y as f32), t.last, &t.lines, to_screen);
        if s.kind == snap::SnapKind::None {
            // Free points to the millimetre.
            s.point = s.point.map(|v| (v * 1000.0).round() / 1000.0);
        }
        Some(s)
    }

    /// Mouse movement in the Wall tool: snap, preview and prompt.
    fn wall_motion(&mut self, x: i32, y: i32, panel: &Panel, frame: &Frame) {
        let cursor = self.snapped(x, y, panel);
        let Some(t) = &mut self.tool else { return };
        t.cursor = cursor;
        self.update_overlay();
        self.tool_prompt(frame);
    }

    fn wall_click(&mut self, x: i32, y: i32, panel: &Panel, frame: &Frame) {
        let Some(p) = self.snapped(x, y, panel) else {
            return;
        };
        let Some(t) = &mut self.tool else { return };
        t.cursor = Some(p);
        if t.last.is_none() {
            t.last = Some(p.point);
            self.update_overlay();
            return self.tool_prompt(frame);
        }
        match t.shape {
            WallShape::Line => self.place_wall(p.point, frame),
            // An arc: its end, then a point on it.
            WallShape::Arc if t.pending.is_empty() => {
                t.pending.push(p.point);
                self.update_overlay();
                self.tool_prompt(frame);
            }
            WallShape::Arc => {
                let end = t.pending[0];
                self.place_curve(end, vec![p.point], frame);
            }
            // A spline: more points until Enter.
            WallShape::Spline => {
                t.pending.push(p.point);
                self.update_overlay();
                self.tool_prompt(frame);
            }
        }
    }

    /// Enter in the Spline tool: the wall through the clicked points, ending at the last.
    fn finish_spline(&mut self, frame: &Frame) -> bool {
        let Some(t) = &self.tool else { return false };
        if t.shape != WallShape::Spline || t.pending.is_empty() {
            return false;
        }
        let mut pts = t.pending.clone();
        let end = pts.pop().expect("one");
        self.place_curve(end, pts, frame);
        true
    }

    /// Creates a wall from the chain's last point to `end`, and continues the chain from `end`.
    fn place_wall(&mut self, end: [f64; 2], frame: &Frame) {
        self.place_curve(end, Vec::new(), frame);
    }

    /// Creates a wall from the chain's last point to `end` through `through` (straight when
    /// empty), and continues the chain from `end`.
    fn place_curve(&mut self, end: [f64; 2], through: Vec<[f64; 2]>, frame: &Frame) {
        let Some(t) = &self.tool else { return };
        let Some(start) = t.last else { return };
        let top = match t.top {
            Some(level) => json!({ "level": level }),
            None => json!({ "unconnected": WALL_HEIGHT }),
        };
        let mut args = json!({
            "curve": {"line": {"start": start, "end": end}, "through": through},
            "base_level": t.level,
            "top_constraint": top,
            "flip": t.flip,
            "location_line": if through.is_empty() { self.wall_location } else { Default::default() },
        });
        if let Some(ty) = self.tool_wall_type() {
            args["type_id"] = json!(ty);
        }
        if self.structural_walls {
            args["structural"] = json!(true);
        }
        match self.run("wall.create", args) {
            Ok(_) => {
                let t = self.tool.as_mut().expect("tool");
                t.last = Some(end);
                t.typed.clear();
                t.pending.clear();
                self.update_wall_lines();
                self.refresh_scene(frame);
                self.update_overlay();
                self.tool_prompt(frame);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Keys in the Wall tool: Spacebar flips, digits and units type a length, Enter places it,
    /// Backspace edits it. Returns whether the key was used.
    fn wall_key(&mut self, key: i32, frame: &Frame) -> bool {
        let Some(t) = &mut self.tool else {
            // Without a tool, Spacebar flips the selected walls.
            if key == WXK_SPACE && self.doc.is_some() {
                let ids = self.selection();
                if !ids.is_empty() {
                    match self.run("wall.flip", json!({ "wall_ids": ids })) {
                        Ok(_) => frame.set_status_text("Flipped", 0),
                        Err(e) => frame.set_status_text(&e, 0),
                    }
                    self.refresh_scene(frame);
                    return true;
                }
            }
            return false;
        };
        match key {
            WXK_SPACE if t.typed.is_empty() => {
                t.flip = !t.flip;
                self.update_overlay();
            }
            WXK_BACK if !t.typed.is_empty() => {
                t.typed.pop();
            }
            WXK_RETURN | WXK_NUMPAD_ENTER if !t.typed.is_empty() => {
                self.commit_typed_length(frame);
                return true;
            }
            WXK_RETURN | WXK_NUMPAD_ENTER if t.shape == WallShape::Spline => {
                return self.finish_spline(frame);
            }
            _ => {
                let Some(c) = u32::try_from(key).ok().and_then(char::from_u32) else {
                    return false;
                };
                // A length can start with a digit or a decimal point; after that, units and
                // separators too (`4.5 m`, `12' 6 1/2"`).
                let starts = c.is_ascii_digit() || c == '.';
                let continues = !t.typed.is_empty() && (c.is_ascii_graphic() || c == ' ');
                if t.last.is_none() || !(starts || continues) {
                    return false;
                }
                t.typed.push(c);
            }
        }
        self.tool_prompt(frame);
        true
    }

    /// Places a wall of the typed length toward the cursor.
    fn commit_typed_length(&mut self, frame: &Frame) {
        let Some(t) = &self.tool else { return };
        let (Some(start), Some(cursor)) = (t.last, t.cursor) else {
            return;
        };
        let text = t.typed.clone();
        let length = match self.run("units.parse", json!({"text": text, "data_type": "length"})) {
            Ok(v) => v.as_f64().unwrap_or_default(),
            Err(e) => return frame.set_status_text(&format!("Invalid length: {e}"), 0),
        };
        let (dx, dy) = (cursor.point[0] - start[0], cursor.point[1] - start[1]);
        let d = dx.hypot(dy);
        if d < 1e-9 {
            return frame
                .set_status_text("Move the cursor to show the direction, then press Enter", 0);
        }
        let end = [start[0] + dx / d * length, start[1] + dy / d * length];
        self.place_wall(end.map(|v| (v * 1e6).round() / 1e6), frame);
    }

    /// The transient geometry of the Wall tool: the wall to be placed, its exterior side, and the
    /// snap marker.
    fn update_overlay(&mut self) {
        let Some(t) = &self.tool else { return };
        let mut overlay = Vec::new();
        let mesh = |(verts, indices): (Vec<[f64; 3]>, Vec<u32>), color| reviera_render::SceneMesh {
            element: None,
            class: "IfcWall".into(),
            positions: verts
                .iter()
                .map(|[x, y, z]| [*x as f32, *y as f32, *z as f32])
                .collect(),
            indices,
            color,
        };
        let marker = f64::from(self.camera().distance) * 0.008;
        if let Some(c) = t.cursor {
            let [x, y] = c.point;
            let size = if c.kind == snap::SnapKind::None {
                marker * 0.5
            } else {
                marker
            };
            let square = [
                [x - size, y - size],
                [x + size, y - size],
                [x + size, y + size],
                [x - size, y + size],
            ];
            let color = if c.kind == snap::SnapKind::None {
                PREVIEW
            } else {
                SNAP
            };
            overlay.push(mesh(
                reviera_core::walls::prism(&square, t.elevation, t.elevation + size),
                color,
            ));
        }
        // A curved wall being drawn: the curve through the clicked points and the cursor.
        let curve = match (t.shape, t.last, t.cursor) {
            (WallShape::Arc, Some(start), Some(c)) if !t.pending.is_empty() => {
                reviera_core::walls::curved::path(
                    reviera_core::walls::Line {
                        start,
                        end: t.pending[0],
                    },
                    &[c.point],
                )
            }
            (WallShape::Spline, Some(start), Some(c)) if !t.pending.is_empty() => {
                reviera_core::walls::curved::path(
                    reviera_core::walls::Line {
                        start,
                        end: c.point,
                    },
                    &t.pending,
                )
            }
            _ => None,
        };
        if let Some(path) = &curve {
            let z = t.elevation;
            for m in reviera_core::walls::curved::body(
                path,
                0.2,
                z,
                z + WALL_HEIGHT,
                &[],
                None,
                [None, None],
            ) {
                overlay.push(mesh(m, PREVIEW));
            }
        }
        if let (Some(start), Some(c)) = (t.last, t.cursor)
            && t.typed.is_empty()
            && curve.is_none()
        {
            let height = t
                .top
                .and_then(|l| {
                    self.doc
                        .and_then(|d| self.app.document(d))?
                        .doc
                        .store()
                        .get(l)?
                        .param("@Elevation")?
                        .as_f64()
                })
                .map_or(WALL_HEIGHT, |z| z - t.elevation);
            let g = reviera_core::walls::WallGeometry {
                start,
                end: c.point,
                base_z: t.elevation,
                height,
                width: 0.2,
            };
            if g.length() > reviera_core::walls::TOL_SHORT {
                overlay.push(mesh(g.mesh(), PREVIEW));
                // A flat arrow at mid-length on the exterior face.
                let side = if t.flip { -1.0 } else { 1.0 };
                let [nx, ny] = g.normal().map(|v| v * side);
                let [ux, uy] = g.direction();
                let mid = [(start[0] + c.point[0]) / 2.0, (start[1] + c.point[1]) / 2.0];
                let base = [mid[0] + nx * 0.1, mid[1] + ny * 0.1];
                let a = 0.25f64.min(g.length() / 4.0);
                let tip = [base[0] + nx * a, base[1] + ny * a];
                let z = t.elevation + 0.01;
                let positions = [
                    [base[0] - ux * a, base[1] - uy * a, z],
                    [base[0] + ux * a, base[1] + uy * a, z],
                    [tip[0], tip[1], z],
                ];
                overlay.push(mesh((positions.to_vec(), vec![0, 1, 2, 0, 2, 1]), SNAP));
            }
        }
        if let Some(v) = self.viewport() {
            v.set_overlay(&overlay);
        }
    }

    fn selection(&mut self) -> Vec<ElementId> {
        self.run("selection.get", json!({}))
            .ok()
            .and_then(|v| serde_json::from_value(v["ids"].clone()).ok())
            .unwrap_or_default()
    }

    /// Selects what is under pixel (`x`, `y`), or clears the selection over empty space; with
    /// `combine` Add or Remove, adds it to the selection or takes it out (UI-SEL-002).
    fn pick(&mut self, x: i32, y: i32, panel: &Panel, combine: select::Combine) {
        if self.doc.is_none() {
            return;
        }
        // The candidate Tab shows here (ARC-KEY-001).
        if let Some(ids) = self.tab_pick(x, y) {
            return self.select_ids(&ids, combine);
        }
        // In plans a room is picked by its label, or by its inside where nothing else is.
        let hit = self
            .room_label_at(x, y)
            .or_else(|| self.rebar_line_at(x, y))
            .or_else(|| self.element_at(x, y, panel))
            .or_else(|| self.room_inside(x, y));
        match hit {
            Some(id) => self.select_ids(&[id], combine),
            None if combine == select::Combine::Replace => self.select_ids(&[], combine),
            None => {}
        }
    }

    /// The element under pixel (`x`, `y`): in plans dimensions and grids first, then the model.
    fn element_at(&self, x: i32, y: i32, panel: &Panel) -> Option<ElementId> {
        if self.active_view().is_some_and(|v| v.section.is_some()) {
            return self.section_pick(x, y);
        }
        let size = panel.get_client_size();
        let (origin, dir) =
            self.camera()
                .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        let (bottom, top) = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map_or((f32::NEG_INFINITY, f32::INFINITY), plans::PlanState::drawn);
        // In plans, annotation, grids, doors and windows are picked before the model; in 3D
        // views showing it, the analytical model is.
        self.analytical_pick(x, y, size.width as f32, size.height as f32)
            .or_else(|| self.note_at(x, y))
            .or_else(|| self.dim_at(x, y))
            .or_else(|| self.rebar_annotation_at(x, y))
            .or_else(|| self.grid_at(x, y))
            .or_else(|| self.datum_at(x, y))
            .or_else(|| self.insert_at(x, y))
            .or_else(|| self.opening_at(x, y))
            .or_else(|| {
                // What the view cuts away or hides cannot be picked.
                let v = self.active_view()?;
                let (b, slices) = self.view_clip(v);
                let clipping = reviera_render::Clipping::of(b.as_ref(), &slices);
                let keep = |p: glam::Vec3| (bottom..=top).contains(&p.z) && clipping.keeps(p);
                self.scene()
                    .pick_visible(origin, dir, &keep, &self.view_visibility(v))
                    .and_then(|(i, _)| self.scene().meshes[i].element)
            })
    }

    fn clear_selection(&mut self) {
        if self.doc.is_some() && self.run("selection.clear", json!({})).is_ok() {
            self.selection_changed();
        }
    }

    /// Shows the contextual "Modify | <category>" tab while elements are selected (§1.9.1).
    /// Enables the actions that depend on the active view (3D-only navigation).
    fn update_view_actions(&mut self) {
        let three_d = self
            .active_view()
            .is_some_and(|v| !v.is_2d() && v.schedule.is_none());
        for action in ["view.set_projection", "view.walk"] {
            self.ribbon.enable(action, three_d);
        }
    }

    fn update_ribbon_context(&mut self) {
        self.update_view_actions();
        if self.active_view().is_some_and(|v| v.schedule.is_some()) {
            let panel = self.active_view().and_then(|v| {
                let def = reviera_core::schedules::schedule_def(self.app.document(v.doc)?.doc.store().get(v.element?)?)?;
                Some(def.kind == reviera_core::mep_panel_schedule::KIND_PANEL)
            });
            let context = if panel == Some(true) {
                ("Panel Schedule", Some("session.panel_schedule"))
            } else {
                ("Schedule", Some("session.schedule"))
            };
            self.ribbon.set_context(Some(context));
            return;
        }
        if self.inspector.as_ref().is_some_and(|i| Some(i.view) == self.active) {
            self.ribbon
                .set_context(Some(("System Inspector", Some("session.system_inspector"))));
            return;
        }
        if self.in_sheet() {
            self.ribbon
                .set_context(Some(("Sheet", Some("session.sheet"))));
            let selected = self.sheet_item_selected();
            self.ribbon.enable("sheet.open_view", selected);
            return;
        }
        if let Some(s) = &self.sketch {
            use reviera_core::floors::SlabKind;
            use sketch::SketchKind;
            let (label, session) = match (s.host, s.kind) {
                (Some(_), SketchKind::Roof) => ("Edit Footprint", "session.roof_sketch"),
                (None, SketchKind::Roof) => ("Create Roof Footprint", "session.roof_sketch"),
                (Some(_), _) => ("Edit Boundary", "session.floor_sketch"),
                (None, SketchKind::Slab(SlabKind::Floor)) => {
                    ("Create Floor Boundary", "session.floor_sketch")
                }
                (None, SketchKind::Slab(SlabKind::Ceiling)) => {
                    ("Create Ceiling Boundary", "session.floor_sketch")
                }
                (None, SketchKind::Deck) => ("Create Deck Outline", "session.floor_sketch"),
                (None, SketchKind::Opening) => ("Vertical Opening", "session.floor_sketch"),
                (None, SketchKind::Shaft) => ("Shaft Opening", "session.floor_sketch"),
            };
            self.ribbon.set_context(Some((label, Some(session))));
            return;
        }
        if self.ceiling_tool.is_some() {
            self.ribbon
                .set_context(Some(("Place Ceiling", Some("session.ceiling"))));
            return;
        }
        if self.room_tool.is_some() {
            self.ribbon
                .set_context(Some(("Place Room", Some("session.room"))));
            return;
        }
        // The group editor (ARC-GRP-002).
        if self
            .store_now()
            .is_some_and(|s| reviera_core::groups::editing(&s).is_some())
        {
            self.ribbon
                .set_context(Some(("Edit Group", Some("session.group"))));
            return;
        }
        let ids = self.selection();
        let store = self
            .doc
            .and_then(|d| self.app.document(d))
            .map(|d| d.doc.store().clone());
        let context = store.filter(|_| !ids.is_empty()).map(|store| {
            let cats: Vec<Option<&'static str>> = ids
                .iter()
                .map(|id| {
                    store.get(*id).and_then(|e| {
                        if reviera_core::annotations::dimension_data(e).is_some() {
                            return Some("cat.dimensions");
                        }
                        if reviera_core::notes::tag_data(e).is_some() {
                            return Some("cat.tags");
                        }
                        if reviera_core::notes::text_data(e).is_some() {
                            return Some("cat.text_notes");
                        }
                        // Placed components and path arrays have their own tools, whatever
                        // category their component gives them.
                        if reviera_core::mesh_objects::mesh_object_recipe(e).is_some() {
                            return Some("ctx.components");
                        }
                        if reviera_core::path_arrays::path_array_recipe(e).is_some() {
                            return Some("ctx.path_arrays");
                        }
                        if reviera_core::groups::instance_of(e).is_some() {
                            return Some("ctx.groups");
                        }
                        reviera_core::categories::category_of(e).map(|c| c.key)
                    })
                })
                .collect();
            let one = cats
                .first()
                .copied()
                .flatten()
                .filter(|c| cats.iter().all(|x| *x == Some(*c)));
            let label = match one {
                Some("cat.dimensions") => "Dimensions".to_owned(),
                Some("cat.tags") => "Tags".to_owned(),
                Some("cat.text_notes") => "Text Notes".to_owned(),
                Some(k) => self
                    .ribbon
                    .context_label(Some(k))
                    .or_else(|| reviera_core::categories::category(k).map(|c| c.name.to_owned()))
                    .unwrap_or_else(|| "Multi-Select".to_owned()),
                None => "Multi-Select".to_owned(),
            };
            (label, one)
        });
        self.ribbon
            .set_context(context.as_ref().map(|(l, c)| (l.as_str(), *c)));
        // Tools that act on the selection are disabled without one (UI-RIB-001: they stay
        // visible).
        for action in [
            "element.delete",
            "wall.flip",
            "insert.flip_hand",
            "insert.flip_facing",
            "element.move",
            "element.copy",
            "element.rotate",
            "element.mirror_pick",
            "element.mirror_draw",
            "element.pin",
            "element.unpin",
        ] {
            self.ribbon.enable(action, !ids.is_empty());
        }
    }

    fn selection_changed(&mut self) {
        self.update_ribbon_context();
        self.update_grips();
        let ids = self.selection();
        if let Some(v) = self.viewport() {
            v.set_highlight(&ids);
        }
        // Plans draw selected annotation highlighted.
        if let Some(doc) = self.doc {
            self.plans_redraw(doc);
        }
        self.properties_stale = true;
    }

    /// Fills the Properties palette for the current selection.
    fn show_properties(&mut self, grid: &PropertyGrid) {
        self.fill_properties(grid);
        // Room for labels such as "Thermal Transmittance (type)".
        grid.set_splitter_position(150, 0);
    }

    fn fill_properties(&mut self, grid: &PropertyGrid) {
        self.properties_stale = false;
        grid.clear();
        self.rows.clear();
        let Some(doc) = self.doc else { return };
        let ids = self.selection();
        self.type_rows = None;
        self.wall_type_row = None;
        if self.sketch_rows(grid) {
            return;
        }
        if ids.is_empty()
            && (self.grid_tool_properties(grid)
                || self.line_tool_properties(grid)
                || self.column_tool_properties(grid)
                || self.beam_tool_properties(grid)
                || self.curtain_tool_properties(grid)
                || self.ceiling_tool_properties(grid)
                || self.ramp_tool_properties(grid)
                || self.skylight_tool_properties(grid)
                || self.place_tool_properties(grid)
                || self.array_tool_properties(grid)
                || self.insert_tool_properties(grid)
                || self.wall_tool_properties(grid)
                || self.view_properties(grid))
        {
            return;
        }
        if ids.len() != 1 {
            let label = if ids.is_empty() {
                "No selection".to_owned()
            } else {
                format!("{} elements selected", ids.len())
            };
            grid.append(Property::category(label, "c_sel"));
            if let Ok(info) = self.run("selection.get", json!({})) {
                for (i, (cat, n)) in info["by_category"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    let name = format!("s{i}");
                    grid.append(Property::string(category_label(cat), &name, n.to_string()));
                    grid.set_property_read_only(&name, true);
                }
            }
            return;
        }
        let id = ids[0];
        let Some(e) = self
            .app
            .document(doc)
            .and_then(|d| d.doc.store().get(id).cloned())
        else {
            return;
        };
        let store = &self.app.document(doc).expect("open").doc;
        let dimension = reviera_core::annotations::dimension_data(&e);
        let category = reviera_core::categories::category_of(&e)
            .map(|c| c.name.to_owned())
            .or_else(|| dimension.as_ref().map(|_| "Dimensions".to_owned()))
            .or_else(|| {
                reviera_core::notes::tag_data(&e)
                    .map(|_| "Tags".to_owned())
                    .or_else(|| reviera_core::notes::text_data(&e).map(|_| "Text Notes".to_owned()))
            })
            .unwrap_or_else(|| "—".to_owned());
        let type_name = e
            .param("@type")
            .and_then(Value::as_str)
            .and_then(reviera_core::GlobalId::parse)
            .and_then(|g| store.store().find_by_guid(&g))
            .and_then(|t| store.store().get(t))
            .and_then(|t| t.name.clone())
            .unwrap_or_default();
        grid.append(Property::category("Element", "c_element"));
        let is_level = e.class == reviera_core::levels::CLASS;
        let is_grid = reviera_core::grids::is_grid(&e);
        let is_insert = reviera_core::inserts::kind_of(&e).is_some();
        let is_wall = reviera_core::walls::wall_recipe(&e).is_some()
            || reviera_core::columns::column_recipe(&e).is_some()
            || reviera_core::curtain::curtain_recipe(&e).is_some();
        for (i, (label, value)) in [
            ("Category", category),
            // A grid line is written as an axis of the grid system.
            (
                "IFC class",
                if is_grid {
                    "IfcGridAxis".to_owned()
                } else {
                    e.class.clone()
                },
            ),
            ("Name", e.name.clone().unwrap_or_default()),
            ("Type", type_name),
            ("ID", id.to_string()),
            ("GlobalId", e.guid().to_string()),
        ]
        .into_iter()
        .enumerate()
        {
            // A level's name is edited here (`level.rename`).
            if is_level && label == "Name" {
                grid.append(Property::string(label, "level_name", value));
                continue;
            }
            // And a reference plane's (`refplane.rename`).
            if reviera_core::ref_planes::recipe(&e).is_some() && label == "Name" {
                grid.append(Property::string(label, "dt_rp_name", value));
                continue;
            }
            // So is a grid's (`grid.rename`).
            if is_grid && label == "Name" {
                grid.append(Property::string(label, "grid_name", value));
                continue;
            }
            // A grid's, wall's, door's or window's type is chosen below, with its values.
            if (is_grid || is_insert || is_wall) && label == "Type" {
                continue;
            }
            let name = format!("e{i}");
            grid.append(Property::string(label, &name, value));
            grid.set_property_read_only(&name, true);
        }
        // A dormer's parts show the dormer's own rows, which rebuild it.
        self.dormer_rows(grid, id);
        if is_grid {
            let current = self
                .app
                .document(doc)
                .and_then(|d| {
                    reviera_core::grids::list(d.doc.store())
                        .into_iter()
                        .find(|g| g.id == id)
                })
                .and_then(|g| g.type_id);
            self.datum_view_rows(grid, id);
            self.grid_type_rows(grid, current);
            return;
        }
        if is_level {
            self.datum_view_rows(grid, id);
        }
        if is_insert {
            self.insert_rows(grid, id);
        }
        // Walls go on to their parameters (identity data, type values).
        self.wall_cross_rows(grid, id);
        self.wall_layer_rows(grid, id);
        self.wall_rows(grid, id);
        if self.deck_rows(grid, id) {
            return;
        }
        if reviera_core::floors::slab_recipe(&e).is_some() {
            self.floor_rows(grid, id);
            return;
        }
        if reviera_core::roofs::roof_recipe(&e).is_some() {
            self.roof_rows(grid, id);
            return;
        }
        if reviera_core::rooms::room_recipe(&e).is_some() {
            self.room_rows(grid, id);
            return;
        }
        self.steel_mark_rows(grid, id);
        if self.note_rows(grid, id)
            || self.stair_rows(grid, id)
            || self.opening_rows(grid, id)
            || self.column_rows(grid, id)
            || self.beam_rows(grid, id)
            || self.analytical_rows(grid, id)
            || self.connection_rows(grid, id)
            || self.rebar_rows(grid, id)
            || self.reinforcement_system_rows(grid, id)
            || self.detail_rows(grid, id)
            || self.curtain_rows(grid, id)
            || self.ramp_rows(grid, id)
            || self.skylight_rows(grid, id)
            || self.mesh_object_rows(grid, id)
            || self.part_rows(grid, id)
            || self.path_array_rows(grid, id)
            || self.datum_rows(grid, id)
        {
            return;
        }
        if let Some(d) = dimension {
            let store = self.app.document(doc).expect("open").doc.store().clone();
            grid.append(Property::category("Dimension", "c_dim"));
            let measured = reviera_core::annotations::measure(&store, &d);
            for (i, v) in measured.iter().flat_map(|m| m.values.clone()).enumerate() {
                let name = format!("d{i}");
                let text = self.format_length(v);
                grid.append(Property::string(format!("Segment {}", i + 1), &name, text));
                grid.set_property_read_only(&name, true);
            }
            let segments = measured.map_or(0, |m| m.values.len());
            self.dimension_constraint_rows(grid, id, segments);
            return;
        }

        let defs = self
            .run("param.list", json!({ "element_id": id }))
            .unwrap_or(json!([]));
        let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
        for d in defs.as_array().into_iter().flatten() {
            let group = d["group"].as_str().unwrap_or("other").to_owned();
            match groups.iter_mut().find(|(g, _)| *g == group) {
                Some((_, list)) => list.push(d.clone()),
                None => groups.push((group, vec![d.clone()])),
            }
        }
        for (gi, (group, list)) in groups.into_iter().enumerate() {
            let mut header_added = false;
            for d in list {
                let reference = d["ref"].as_str().unwrap_or_default().to_owned();
                // Type parameters of an element without a type, and the like, are not shown.
                let Ok(value) = self.run(
                    "param.get",
                    json!({ "element_id": id, "param_ref": reference }),
                ) else {
                    continue;
                };
                if !header_added {
                    grid.append(Property::category(category_label(&group), format!("g{gi}")));
                    header_added = true;
                }
                let data_type = d["data_type"].as_str().unwrap_or("text").to_owned();
                let mut label = d["name"].as_str().unwrap_or_default().to_owned();
                if d["binding"] == "type" {
                    label.push_str(" (type)");
                }
                let name = format!("p{}", self.rows.len());
                let property = match (&value["value"], data_type.as_str()) {
                    (Value::Bool(b), _) => Property::boolean(&label, &name, *b),
                    (Value::Null, "yes_no") => Property::boolean(&label, &name, false),
                    _ => Property::string(
                        &label,
                        &name,
                        value["display"].as_str().unwrap_or_default(),
                    ),
                };
                grid.append(property);
                if d["read_only"].as_bool() == Some(true)
                    || value["read_only"].as_bool() == Some(true)
                {
                    grid.set_property_read_only(&name, true);
                }
                if let Some(tip) = d["tooltip"].as_str() {
                    grid.set_help_string(&name, tip);
                }
                self.rows.push(Row {
                    element: id,
                    reference,
                    data_type,
                });
            }
        }
    }

    /// The active view's properties; `false` when the active view is not a model view.
    fn view_properties(&mut self, grid: &PropertyGrid) -> bool {
        if self.section_view_rows(grid) || self.sheet_view_rows(grid) {
            return true;
        }
        if let Some(v) = self.active_view().filter(|v| v.schedule.is_some()) {
            let name = self.view_name(v);
            let category = v
                .element
                .and_then(|e| self.app.document(v.doc)?.doc.store().get(e).cloned())
                .and_then(|e| reviera_core::schedules::schedule_def(&e))
                .and_then(|d| reviera_core::categories::category(&d.category))
                .map_or(String::new(), |c| c.name.to_owned());
            grid.append(Property::category("Schedule", "c_view"));
            grid.append(Property::string("View Name", "view_name", name));
            grid.append(Property::string("Category", "sch_cat", category));
            grid.set_property_read_only("sch_cat", true);
            return true;
        }
        let Some(v) = self.active_view() else {
            return false;
        };
        let Some(element) = v.element else {
            return false;
        };
        let name = self.view_name(v);
        let camera = v.camera;
        if let Some(plan) = v.plan.clone() {
            let title = if plan.ceiling {
                "Ceiling Plan"
            } else {
                "Floor Plan"
            };
            grid.append(Property::category(title, "c_view"));
            grid.append(Property::string("View Name", "view_name", name));
            let fmt = |g: &mut Gui, z: f64| {
                if !z.is_finite() {
                    return "Unlimited".to_owned();
                }
                g.run(
                    "units.format",
                    json!({ "value": z - plan.elevation, "data_type": "length" }),
                )
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{:.3} m", z - plan.elevation))
            };
            let cut = fmt(self, plan.range.cut);
            grid.append(Property::string("Cut Plane", "view_cut", cut));
            grid.set_help_string(
                "view_cut",
                "Height of the cut plane above the associated level",
            );
            // DOC-GFX-010.
            let detail = self
                .store_now()
                .and_then(|st| st.get(element).map(reviera_core::views::detail_level))
                .unwrap_or_default();
            grid.append(Property::enumeration(
                "Detail Level",
                "view_detail",
                vec![
                    ("Coarse".to_owned(), 0),
                    ("Medium".to_owned(), 1),
                    ("Fine".to_owned(), 2),
                ],
                match detail {
                    reviera_core::views::DetailLevel::Coarse => 0,
                    reviera_core::views::DetailLevel::Medium => 1,
                    reviera_core::views::DetailLevel::Fine => 2,
                },
            ));
            grid.set_help_string(
                "view_detail",
                "Coarse draws structural framing as single lines",
            );
            let rows = [
                ("Associated Level", plan.level_name.clone()),
                ("Top", fmt(self, plan.range.top)),
                ("Bottom", fmt(self, plan.range.bottom)),
                ("View Depth", fmt(self, plan.range.depth)),
                ("ID", element.to_string()),
            ];
            for (i, (label, value)) in rows.into_iter().enumerate() {
                let key = format!("v{i}");
                grid.append(Property::string(label, &key, value));
                grid.set_property_read_only(&key, true);
            }
            return true;
        }
        grid.append(Property::category("3D View", "c_view"));
        grid.append(Property::string("View Name", "view_name", name));
        let projection = if camera.perspective {
            "Perspective"
        } else {
            "Isometric (parallel)"
        };
        for (i, (label, value)) in [
            ("Projection", projection.to_owned()),
            (
                "Field of View",
                format!("{:.0}° vertical", camera.fov.to_degrees()),
            ),
            ("ID", element.to_string()),
        ]
        .into_iter()
        .enumerate()
        {
            let key = format!("v{i}");
            grid.append(Property::string(label, &key, value));
            grid.set_property_read_only(&key, true);
        }
        // DOC-GFX-010: Fine shows steel connections' plates, bolts and welds.
        let detail = self
            .store_now()
            .and_then(|st| st.get(element).map(reviera_core::views::detail_level))
            .unwrap_or_default();
        grid.append(Property::enumeration(
            "Detail Level",
            "view_detail",
            vec![
                ("Coarse".to_owned(), 0),
                ("Medium".to_owned(), 1),
                ("Fine".to_owned(), 2),
            ],
            match detail {
                reviera_core::views::DetailLevel::Coarse => 0,
                reviera_core::views::DetailLevel::Medium => 1,
                reviera_core::views::DetailLevel::Fine => 2,
            },
        ));
        grid.set_help_string(
            "view_detail",
            "Fine shows steel connections' plates, bolts and welds",
        );
        // Extents (DOC-VIEW-008, DOC-VIEW-014).
        let (section_box, slices) = self.view_clip(self.active_view().expect("checked"));
        grid.append(Property::category("Extents", "c_extents"));
        grid.append(Property::boolean(
            "Section Box",
            "view_box",
            section_box.is_some_and(|b| b.enabled),
        ));
        grid.set_help_string(
            "view_box",
            "Clip the view to a box (drag its grips to resize it)",
        );
        let count = slices.iter().filter(|s| s.enabled).count();
        grid.append(Property::string(
            "Slice Planes",
            "view_slices",
            count.to_string(),
        ));
        grid.set_property_read_only("view_slices", true);
        true
    }

    /// Applies an edit made in the palette.
    fn edit(&mut self, property: &str, text: Option<String>, frame: &Frame) {
        if property == "view_box" {
            return self.toggle_section_box();
        }
        if property == "view_cut" {
            if let Some(t) = text {
                self.set_cut_plane(&t, frame);
            }
            return;
        }
        if property == "view_detail" {
            let view = self.active_view().and_then(|v| v.element);
            if let (Some(view), Some(t)) = (view, text) {
                let level = t.to_lowercase();
                if let Err(e) = self.run(
                    "view.set_detail_level",
                    json!({ "view_id": view, "level": level }),
                ) {
                    error(frame, &format!("The change was not made:\n{e}"));
                }
                self.refresh_scene(frame);
            }
            return;
        }
        if property == "ceil_tool_shape" {
            self.ceiling_follow_roof = text.as_deref() == Some("Follow Roof");
            return;
        }
        if property == "level_name" {
            let level = self.selection().first().copied();
            if let (Some(level), Some(name)) = (level, text) {
                self.rename_level(level, Some(name));
            }
            return;
        }
        if self.grid_type_edit(property, text.clone())
            || self.opening_edit(property, text.clone())
            || self.beam_edit(property, text.clone())
            || self.analytical_edit(property, text.clone())
            || self.connection_edit(property, text.clone())
            || self.rebar_edit(property, text.clone())
            || self.reinforcement_system_edit(property, text.clone())
            || self.detail_edit(property, text.clone())
            || self.column_edit(property, text.clone())
            || self.curtain_edit(property, text.clone())
            || self.ramp_edit(property, text.clone())
            || self.skylight_edit(property, text.clone())
            || self.place_edit(property, text.clone())
            || self.mesh_object_edit(property, text.clone())
            || self.part_edit(property, text.clone())
            || self.path_array_edit(property, text.clone())
            || self.datum_edit(property, text.clone())
            || self.datum_view_edit(property, text.clone())
            || self.dimension_constraint_edit(property, text.clone())
            || self.wall_cross_edit(property, text.clone())
            || self.wall_layer_edit(property, text.clone())
            || self.insert_edit(property, text.clone())
            || self.wall_edit(property, text.clone())
            || self.floor_edit(property, text.clone())
            || self.roof_edit(property, text.clone())
            || self.room_edit(property, text.clone())
            || self.section_view_edit(property, text.clone())
            || self.sheet_view_edit(property, text.clone())
            || self.note_edit(property, text.clone())
            || self.sketch_edit(property, text.clone())
            || self.stair_edit(property, text.clone())
            || self.dormer_edit(property, text.clone())
            || self.curved_roof_edit(property, text.clone())
            || self.deck_edit(property, text.clone())
        {
            return;
        }
        if property == "grid_name" {
            let target = self.selection().first().copied();
            if let (Some(g), Some(name)) = (target, text) {
                if let Err(e) = self.run("grid.rename", json!({ "grid_id": g, "name": name })) {
                    error(frame, &format!("The grid was not renamed:\n{e}"));
                }
                self.refresh_scene(frame);
            }
            return;
        }
        if property == "view_name" {
            let element = self.active_view().and_then(|v| v.element);
            if let (Some(view), Some(name)) = (element, text) {
                if let Err(e) =
                    self.run("view.rename", json!({ "view_id": view, "new_name": name }))
                {
                    error(frame, &format!("The view was not renamed:\n{e}"));
                }
                self.after_view_change();
            }
            return;
        }
        let Some(index) = property
            .strip_prefix('p')
            .and_then(|i| i.parse::<usize>().ok())
        else {
            return;
        };
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let text = text.unwrap_or_default();
        let value = match row.data_type.as_str() {
            _ if text.trim().is_empty() => Value::Null,
            "yes_no" => json!(matches!(text.to_lowercase().as_str(), "true" | "1" | "yes")),
            "integer" => text.trim().parse::<i64>().map_or(json!(text), |n| json!(n)),
            "number" => text.trim().parse::<f64>().map_or(json!(text), |n| json!(n)),
            _ => json!(text),
        };
        let args =
            json!({ "element_ids": [row.element], "param_ref": row.reference, "value": value });
        if let Err(e) = self.run("param.set", args) {
            error(frame, &format!("The value was not changed:\n{e}"));
        }
        // A level's elevation or a type's values can move walls.
        self.refresh_scene(frame);
    }
}

/// The viewport's size in device pixels; 0 when the window is minimized.
fn physical(size: Size, scale: f64) -> (u32, u32) {
    let px = |v: i32| ((v.max(0) as f64) * scale).round() as u32;
    (px(size.width), px(size.height))
}

fn surface_kind(view: &native::NativeView) -> &'static str {
    match view.window {
        raw_window_handle::RawWindowHandle::Xlib(_) => "X11",
        raw_window_handle::RawWindowHandle::Wayland(_) => "Wayland",
        raw_window_handle::RawWindowHandle::Win32(_) => "Windows",
        raw_window_handle::RawWindowHandle::AppKit(_) => "macOS",
        _ => "native",
    }
}

/// A display name for a category key or parameter group (`identity_data` → "Identity Data").
fn category_label(key: &str) -> String {
    if let Some(c) = reviera_core::categories::category(key) {
        return c.name.to_owned();
    }
    if key.contains(' ') || key.chars().any(|c| c.is_uppercase()) {
        return key.to_owned();
    }
    key.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| {
                f.to_uppercase().collect::<String>() + c.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs `f` on the UI state unless another handler is using it (modal dialogs run nested event
/// loops, so paint and idle events can arrive while an action is in progress).
fn with(gui: &Rc<RefCell<Gui>>, f: impl FnOnce(&mut Gui)) {
    if let Ok(mut g) = gui.try_borrow_mut() {
        f(&mut g);
    }
}

fn ask(frame: &Frame, message: &str) -> bool {
    let dialog = MessageDialog::builder(frame, message, "BIM Reviera")
        .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
        .build();
    dialog.show_modal() == ID_YES
}

fn error(frame: &Frame, message: &str) {
    let dialog = MessageDialog::builder(frame, message, "BIM Reviera")
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconError)
        .build();
    dialog.show_modal();
}

fn position(data: &WindowEventData) -> Option<(i32, i32)> {
    let p = match data {
        WindowEventData::MouseButton(e) => e.event.get_position(),
        WindowEventData::MouseMotion(e) => e.event.get_position(),
        _ => None,
    }?;
    Some((p.x, p.y))
}

fn wheel(data: &WindowEventData) -> i32 {
    match data {
        WindowEventData::MouseButton(e) => e.event.get_wheel_rotation(),
        WindowEventData::MouseMotion(e) => e.event.get_wheel_rotation(),
        WindowEventData::General(e) => e.get_wheel_rotation(),
        _ => 0,
    }
}

/// Runs `f` on the view `id` made active first (input in a view applies to that view).
fn with_view(gui: &Rc<RefCell<Gui>>, id: u32, f: impl FnOnce(&mut Gui)) {
    with(gui, |g| {
        if g.view(id).is_some() {
            g.activate_view(id);
            f(g);
        }
    });
}

/// Mouse, keyboard and paint handlers of one view's panel.
fn attach_view_handlers(gui: &Rc<RefCell<Gui>>, id: u32, panel: Panel, frame: Frame) {
    {
        let (gui, p) = (gui.clone(), panel);
        panel.on_paint(move |_| {
            let _dc = PaintDC::new(&p);
            with(&gui, |g| g.draw(id));
        });
    }
    {
        let p = panel;
        panel.on_size(move |e| {
            p.refresh(false, None);
            e.skip(true);
        });
    }
    // Left: the Orientation Cube, else orbit (look in walk mode). Shift+middle: orbit.
    // Middle and right: pan. Right on the cube: its menu.
    for button in 0..3 {
        let (gui, p) = (gui.clone(), panel);
        let handler = move |e: WindowEventData| {
            let (shift, ctrl) = modifiers();
            let mut menu = None;
            if let Some((x, y)) = position(&e) {
                with_view(&gui, id, |g| {
                    // With the ring open, left and right act on release; a middle press closes
                    // it (a middle click opens a new one). A press in the view dismisses a
                    // full-screen reveal.
                    if g.radial.is_some() {
                        if button != 1 {
                            return;
                        }
                        g.radial = None;
                    }
                    if button == 0 && g.chrome_press() {
                        return;
                    }
                    let cube = g.cube_pick(x, y);
                    if button == 2 && cube.is_some() {
                        menu = Some(g.cube_menu());
                        return;
                    }
                    let walking = g.active_view().is_some_and(|v| v.nav.walk.is_some());
                    let plan = g.in_2d();
                    let left = button == 0 && cube.is_none();
                    let editing = if !left {
                        None
                    } else if g.sketch.is_some() && g.sketch_drag_start(x, y).is_some() {
                        Some(Drag::Sketch)
                    } else if let Some((item, from)) = g.sheet_drag_start(x, y) {
                        Some(Drag::SheetItem { item, from })
                    } else if let Some(grip) = g.clip_grip_at(x, y) {
                        g.clip_drag_start(grip, x, y);
                        Some(Drag::Clip(grip))
                    } else if let Some((id, from)) = g.insert_drag_start(x, y, &p) {
                        Some(Drag::MoveInsert { id, from })
                    } else if let Some(d) = g.footprint_grip_at(x, y) {
                        Some(Drag::Footprint(d))
                    } else if let Some((id, end, other)) = g.grip_at(x, y) {
                        Some(Drag::Grip { id, end, other })
                    } else if let Some(from) = g.selection_drag_start(x, y, &p) {
                        Some(Drag::MoveSelection { from })
                    } else if g.picking_free() && cube.is_none() && (plan || ctrl || shift) {
                        // A box: from empty space in plans, with Ctrl or Shift in 3D.
                        Some(Drag::SelectBox {
                            from: (x, y),
                            combine: select::Combine::of(ctrl, shift),
                        })
                    } else {
                        None
                    };
                    let kind = match (button, cube) {
                        _ if editing.is_some() => editing.expect("checked"),
                        _ if plan => Drag::Pan,
                        (0, Some(hit)) => {
                            if let Some(v) = g.active_view_mut() {
                                v.nav.cube_press = Some(hit);
                            }
                            Drag::Cube {
                                ring: hit == reviera_render::viewcube::CubeHit::Ring,
                            }
                        }
                        (0, None) if walking => Drag::Look,
                        (0, None) => Drag::Orbit(g.pivot(x, y)),
                        (1, _) if shift && !walking => Drag::Orbit(g.pivot(x, y)),
                        _ => Drag::Pan,
                    };
                    if !matches!(kind, Drag::Cube { .. }) {
                        g.remember();
                    }
                    if let Some(v) = g.active_view_mut() {
                        v.drag = Some((kind, x, y));
                        if button <= 1 {
                            v.press = Some((x, y));
                        }
                    }
                });
            }
            p.set_focus();
            if let Some(mut menu) = menu {
                p.popup_menu(&mut menu, None);
            }
        };
        match button {
            0 => panel.on_mouse_left_down(handler),
            1 => panel.on_mouse_middle_down(handler),
            _ => panel.on_mouse_right_down(handler),
        };
    }
    for button in 0..3 {
        let (gui, p, f) = (gui.clone(), panel, frame);
        let handler = move |e: WindowEventData| {
            let (shift, ctrl) = modifiers();
            let mut menu = None;
            let mut run: Option<(MenuRunner, i32)> = None;
            with_view(&gui, id, |g| {
                if button == 0 && std::mem::take(&mut g.chrome.swallow_up) {
                    return;
                }
                if g.radial.is_some() && button != 1 {
                    if let Some((x, y)) = position(&e) {
                        let chosen = if button == 0 {
                            g.radial_click(x, y, &p)
                        } else {
                            g.radial = None;
                            None
                        };
                        run = chosen.and_then(|i| Some((g.menu_runner.clone()?, i)));
                    }
                    return;
                }
                let Some(v) = g.active_view_mut() else { return };
                let drag = v.drag.take();
                let press = v.press.take();
                let cube = v.nav.cube_press.take();
                let Some((x, y)) = position(&e) else { return };
                let click = press.is_some_and(|(x0, y0)| {
                    (x - x0).abs() <= CLICK_SLOP && (y - y0).abs() <= CLICK_SLOP
                });
                match drag {
                    Some((Drag::MoveInsert { id: insert, from }, _, _)) => {
                        g.insert_drag_end(insert, from, x, y, click);
                        if !click {
                            return;
                        }
                    }
                    Some((Drag::Grip { id: e, end, other }, _, _)) => {
                        g.grip_drag_end(e, end, other, x, y, click);
                        if !click {
                            return;
                        }
                    }
                    Some((Drag::MoveSelection { from }, _, _)) => {
                        g.selection_drag_end(from, x, y, click);
                        if !click {
                            return;
                        }
                    }
                    Some((Drag::Clip(_), _, _)) => {
                        g.clip_drag_end(click);
                        if !click {
                            return;
                        }
                    }
                    Some((Drag::SheetItem { item, from }, _, _)) => {
                        g.sheet_drag_end(item, from, x, y, click);
                        return;
                    }
                    Some((Drag::Sketch, _, _)) => {
                        g.sketch_drag_end(x, y, click);
                        return;
                    }
                    Some((Drag::Footprint(d), _, _)) => {
                        g.footprint_drag_end(d, x, y, click);
                        if !click {
                            return;
                        }
                    }
                    Some((Drag::SelectBox { from, combine }, _, _)) => {
                        if !click {
                            return g.box_select(from, (x, y), combine, &p);
                        }
                        if let Some(v) = g.viewport() {
                            v.set_overlay_lines(&[]);
                        }
                    }
                    _ => {}
                }
                // A middle click (a middle drag pans): the quick modify menu.
                if button == 1 && click {
                    if !(g.ui_settings.radial_menu && g.open_radial(x, y, &p, ctrl)) {
                        menu = g.quick_menu(x, y, &p, ctrl);
                    }
                    return;
                }
                if button != 0 || !click {
                    return;
                }
                match cube {
                    Some(hit) => g.cube_click(hit),
                    None if g.tools_click(x, y, &p, &f) => {}
                    None if g.in_sheet() => g.sheet_click(x, y),
                    None if g.temp_dim_click(x, y) => {}
                    None => g.pick(x, y, &p, select::Combine::of(ctrl, shift)),
                }
            });
            p.refresh(false, None);
            if let Some(mut menu) = menu {
                p.popup_menu(&mut menu, None);
            }
            if let Some((runner, id)) = run {
                runner(id);
            }
        };
        match button {
            0 => panel.on_mouse_left_up(handler),
            1 => panel.on_mouse_middle_up(handler),
            _ => panel.on_mouse_right_up(handler),
        };
    }
    {
        let (gui, p, f) = (gui.clone(), panel, frame);
        panel.on_mouse_motion(move |e| {
            let Some((x, y)) = position(&e) else { return };
            let mut redraw = false;
            with(&gui, |g| {
                // The radial menu (UI-SEL-007) takes the pointer while open.
                if g.radial.as_ref().is_some_and(|r| r.view == id) {
                    redraw = g.radial_motion(x, y, &p);
                    return;
                }
                let Some(v) = g.views.iter().find(|v| v.id == id) else {
                    return;
                };
                let Some((kind, x0, y0)) = v.drag else {
                    if g.active != Some(id) {
                        return;
                    }
                    redraw |= g.cube_hover(Some((x, y)));
                    g.mouse = (x, y);
                    redraw |= g.tools_motion(x, y, &p, &f);
                    return;
                };
                match kind {
                    Drag::MoveInsert { id: insert, from } => {
                        g.insert_drag_motion(insert, from, x, y);
                        redraw = true;
                        return;
                    }
                    Drag::Grip { id: e, end, other } => {
                        g.grip_drag_motion(e, end, other, x, y);
                        redraw = true;
                        return;
                    }
                    Drag::MoveSelection { from } => {
                        g.selection_drag_motion(from, x, y);
                        redraw = true;
                        return;
                    }
                    Drag::Clip(_) => {
                        g.clip_drag_motion(x, y);
                        redraw = true;
                        return;
                    }
                    Drag::SheetItem { item, from } => {
                        g.sheet_drag_motion(item, from, x, y);
                        redraw = true;
                        return;
                    }
                    Drag::Sketch => {
                        g.sketch_drag_motion(x, y);
                        redraw = true;
                        return;
                    }
                    Drag::Footprint(d) => {
                        g.footprint_drag_motion(d, x, y);
                        redraw = true;
                        return;
                    }
                    Drag::SelectBox { from, .. } => {
                        g.box_motion(from, (x, y), &p);
                        redraw = true;
                        return;
                    }
                    _ => {}
                }
                let v = g.views.iter_mut().find(|v| v.id == id).expect("found");
                let (dx, dy) = ((x - x0) as f32, (y - y0) as f32);
                match kind {
                    Drag::Orbit(pivot) => v.camera.orbit_about(dx, dy, pivot),
                    Drag::Cube { ring: false } => v.camera.orbit(dx, dy),
                    Drag::Cube { ring: true } => v.camera.orbit(dx, 0.0),
                    Drag::Look => v.camera.look(dx, dy),
                    Drag::Pan => {
                        let h = p.get_client_size().height as f32;
                        v.camera.pan(dx, dy, h);
                    }
                    Drag::MoveInsert { .. }
                    | Drag::Grip { .. }
                    | Drag::MoveSelection { .. }
                    | Drag::Clip(_)
                    | Drag::SheetItem { .. }
                    | Drag::Sketch
                    | Drag::Footprint(_)
                    | Drag::SelectBox { .. } => {}
                }
                v.drag = Some((kind, x, y));
                redraw = true;
            });
            if redraw {
                p.refresh(false, None);
            }
        });
    }
    {
        let (gui, p) = (gui.clone(), panel);
        panel.on_mouse_leave(move |e| {
            let mut redraw = false;
            with(&gui, |g| {
                if g.active == Some(id) {
                    redraw = g.cube_hover(None);
                }
            });
            if redraw {
                p.refresh(false, None);
            }
            e.skip(true);
        });
    }
    {
        let (gui, p) = (gui.clone(), panel);
        panel.on_mouse_wheel(move |e| {
            let steps = wheel(&e) as f32 / 120.0;
            let at = position(&e);
            with_view(&gui, id, |g| {
                if g.walk_speed(steps) {
                    return;
                }
                g.remember_wheel();
                let size = p.get_client_size();
                let Some(v) = g.active_view_mut() else { return };
                match at {
                    Some((x, y)) => v.camera.zoom_at(
                        steps,
                        x as f32,
                        y as f32,
                        size.width as f32,
                        size.height as f32,
                    ),
                    None => v.camera.zoom(steps),
                }
            });
            p.refresh(false, None);
        });
    }
    {
        let (gui, p, f) = (gui.clone(), panel, frame);
        panel.on_key_down(move |e| {
            let (code, ctrl, shift) = match &e {
                WindowEventData::Keyboard(k) => {
                    (k.get_key_code(), k.control_down(), k.shift_down())
                }
                _ => (None, false, false),
            };
            let mut used = false;
            // The radial menu takes arrows, Enter, Backspace and Esc while open.
            let mut ring: Option<(bool, Option<i32>, Option<MenuRunner>)> = None;
            if let Some(code) = code {
                with(&gui, |g| {
                    if g.radial.is_some() {
                        let (took, run) = g.radial_key(code, &p);
                        ring = Some((took, run, g.menu_runner.clone()));
                    }
                });
            }
            if let Some((true, run, runner)) = ring {
                p.refresh(false, None);
                if let (Some(id), Some(runner)) = (run, runner) {
                    runner(id);
                }
                return;
            }
            match code {
                // Ctrl+Tab / Ctrl+Shift+Tab switch views (GTK menus cannot bind them).
                Some(WXK_TAB) if ctrl => {
                    with(&gui, |g| g.cycle_view(if shift { -1 } else { 1 }));
                    used = true;
                }
                Some(WXK_DELETE) => {
                    with_view(&gui, id, |g| {
                        let typing = g.tool.as_ref().is_some_and(|t| !t.typed.is_empty());
                        if g.sketch.is_some() {
                            // In sketch mode Delete removes the last curve.
                            let (mx, my) = g.mouse;
                            g.sketch_key(WXK_DELETE, mx, my);
                        } else if !typing && !g.sheet_delete() {
                            g.delete_selection();
                        }
                    });
                    used = true;
                }
                Some(WXK_ESCAPE) => {
                    with_view(&gui, id, |g| {
                        if g.typed_active() {
                            g.typed_length_key(WXK_ESCAPE);
                        } else if g.active_view().is_some_and(|v| v.nav.walk.is_some()) {
                            g.toggle_walk();
                        } else if g.sketch.is_some() {
                            let (mx, my) = g.mouse;
                            g.sketch_key(WXK_ESCAPE, mx, my);
                        } else if g.ceiling_tool.is_some() {
                            g.end_ceiling_tool();
                            f.set_status_text("Ready", 0);
                        } else if g.room_tool.is_some() {
                            g.end_room_tool();
                            f.set_status_text("Ready", 0);
                        } else if g.dim_tool.is_some() {
                            g.dim_escape();
                        } else if g.grid_tool.is_some() {
                            g.grid_escape();
                        } else if g.line_tool.is_some() {
                            g.line_escape();
                        } else if g.insert_tool.is_some() {
                            g.end_insert_tool();
                        } else if g.modify_tool.is_some() {
                            g.modify_escape();
                        } else {
                            g.escape(&f);
                        }
                    });
                    used = true;
                }
                Some(key) => {
                    with_view(&gui, id, |g| {
                        used = g.tools_key(key, shift, &f);
                    });
                }
                None => {}
            }
            if used {
                p.refresh(false, None);
            } else {
                // Lets the character event follow (typed lengths, key sequences).
                e.skip(true);
            }
        });
    }
    {
        let gui = gui.clone();
        panel.on_key_up(move |e| {
            if let WindowEventData::Keyboard(k) = &e
                && let Some(key) = k.get_key_code()
            {
                let shift = k.shift_down();
                with(&gui, |g| {
                    g.walk_key(key, false, shift);
                });
            }
            e.skip(true);
        });
    }
    {
        let (gui, p, f) = (gui.clone(), panel, frame);
        panel.on_char(move |e| {
            let key = match &e {
                WindowEventData::Keyboard(k) => k.get_unicode_key(),
                _ => None,
            };
            let mut used = false;
            if let Some(key) = key.filter(|k| *k > 32 && *k != 127) {
                with_view(&gui, id, |g| {
                    if g.active_view().is_some_and(|v| v.nav.walk.is_some()) {
                        // Movement keys are handled on key down; other letters may end the
                        // walk with a sequence (WK, ZF, ...).
                        if let Some(c) = u32::try_from(key).ok().and_then(char::from_u32)
                            && c.is_ascii_alphabetic()
                        {
                            g.sequence_key(c);
                        }
                        used = true;
                        return;
                    }
                    used = g.modify_key(key) || g.wall_key(key, &f) || g.typed_length_key(key);
                    let typing = g.tool.as_ref().is_some_and(|t| !t.typed.is_empty())
                        || g.modify_typing()
                        || g.typed_active();
                    if !used
                        && !typing
                        && let Some(c) = u32::try_from(key).ok().and_then(char::from_u32)
                        && c.is_ascii_alphabetic()
                    {
                        used = g.sequence_key(c);
                    }
                });
            }
            if used {
                p.refresh(false, None);
            } else {
                e.skip(true);
            }
        });
    }
}

impl Gui {
    /// Redraws the active view.
    fn refresh_active(&self) {
        if let Some(v) = self.active_view() {
            v.panel.refresh(false, None);
        }
    }

    /// Shows or hides a dockable pane.
    fn toggle_pane(&mut self, pane: &str) {
        let shown = self.aui.is_pane_shown(pane);
        self.aui.set_pane_shown(pane, !shown);
        self.aui.update();
        if pane == "model_tree" && !shown {
            self.tree.stale = true;
        }
    }
}

/// Runs the main window until it is closed, opening `file` if given.
pub fn run(file: Option<PathBuf>) -> Result<(), String> {
    crash::install();
    wxdragon::main(move |_| {
        let frame = Frame::builder()
            .with_title("BIM Reviera")
            .with_size(Size::new(1400, 860))
            .build();

        // Menus, the Quick Access Toolbar and the ribbon come from one catalog (UI-RIBCAT-001).
        let catalog = ribbon::Catalog::parse(ribbon::DEFAULT).expect("the built-in ribbon catalog");
        let menu_bar = ribbon::menu_bar(&catalog, &|name| {
            (name == "orient").then(|| {
                let mut orient_menu = Menu::builder();
                for (k, (label, _)) in nav::PRESETS.iter().enumerate() {
                    orient_menu = orient_menu.append_item(ID_ORIENT_FIRST + k as i32, label, "");
                }
                (orient_menu.build(), "&Orient".to_owned())
            })
        });
        frame.set_menu_bar(menu_bar);
        frame.create_status_bar(2, 0, -1, "status");
        frame.set_status_text("Open an IFC file to begin (File > Open)", 0);

        // Docking layout: Quick Access Toolbar and ribbon on top, Properties on the left, view
        // tabs in the centre (Ch 1 §1.4).
        let aui = AuiManager::builder(&frame).build();
        let qat = ribbon::quick_access(&frame, &catalog);
        aui.add_pane_with_info(
            &qat,
            AuiPaneInfo::new()
                .with_name("quick_access")
                .toolbar_pane()
                .top()
                .layer(2)
                .row(0)
                .gripper(false)
                .floatable(false)
                .dockable(false)
                .movable(false),
        );
        // Ribbon clicks reach the window's dispatcher once it exists.
        let dispatch: Rc<RefCell<Option<Dispatch>>> = Rc::new(RefCell::new(None));
        let ribbon_panel = Panel::builder(&frame).build();
        let mut ribbon_bar = {
            let dispatch = dispatch.clone();
            ribbon::Ribbon::new(&ribbon_panel, catalog.clone(), move |id| {
                if let Ok(d) = dispatch.try_borrow()
                    && let Some(d) = d.as_ref()
                {
                    d(id);
                }
            })
        };
        aui.add_pane_with_info(
            &ribbon_panel,
            AuiPaneInfo::new()
                .with_name("ribbon")
                .top()
                .layer(2)
                .row(1)
                .caption_visible(false)
                .pane_border(false)
                .floatable(false)
                .dockable(false)
                .movable(false)
                .resizable(false)
                .min_size(-1, ribbon_bar.height()),
        );
        // Menu items follow the ribbon's enabled state.
        if let Some(bar) = frame.get_menu_bar() {
            ribbon_bar.set_menu_bar(bar);
        }
        let grid = PropertyGrid::builder(&frame)
            .with_style(PropertyGridStyle::Tooltips)
            .build();
        let notebook = AuiNotebook::builder(&frame).build();
        wx::enable_tab_dragging(&notebook);
        let browser = TreeCtrl::builder(&frame)
            .with_style(
                TreeCtrlStyle::Default
                    | TreeCtrlStyle::HasButtons
                    | TreeCtrlStyle::LinesAtRoot
                    | TreeCtrlStyle::HideRoot,
            )
            .build();
        aui.add_pane_with_info(
            &grid,
            AuiPaneInfo::new()
                .with_name("properties")
                .with_caption("Properties")
                .left()
                .layer(1)
                .best_size(340, 600)
                .min_size(220, 200)
                .close_button(true)
                .floatable(true),
        );
        aui.add_pane_with_info(
            &browser,
            AuiPaneInfo::new()
                .with_name("browser")
                .with_caption("Project Browser")
                .left()
                .layer(1)
                .row(0)
                .position(1)
                .best_size(340, 260)
                .min_size(220, 120)
                .close_button(true)
                .floatable(true),
        );
        let model_tree = TreeCtrl::builder(&frame)
            .with_style(
                TreeCtrlStyle::Default | TreeCtrlStyle::HasButtons | TreeCtrlStyle::LinesAtRoot,
            )
            .build();
        aui.add_pane_with_info(
            &model_tree,
            AuiPaneInfo::new()
                .with_name("model_tree")
                .with_caption("Model Tree")
                .left()
                .layer(1)
                .row(0)
                .position(2)
                .best_size(340, 260)
                .min_size(220, 120)
                .close_button(true)
                .floatable(true),
        );
        aui.add_pane_with_info(
            &notebook,
            AuiPaneInfo::new().with_name("views").center_pane(),
        );
        aui.update();

        let gui = Rc::new_cyclic(|this| {
            RefCell::new(Gui {
                app: {
                    // The window captures views for agents (DM-MCP-004).
                    let mut app = App::new(AppConfig::default());
                    app.set_window(true);
                    app
                },
                this: this.clone(),
                frame,
                notebook,
                aui,
                doc: None,
                views: Vec::new(),
                active: None,
                next_view: 1,
                gpu: None,
                scenes: HashMap::new(),
                rows: Vec::new(),
                properties_stale: true,
                tool: None,
                dim_tool: None,
                grid_tool: None,
                line_tool: None,
                tab: None,
                insert_tool: None,
                modify_tool: None,
                sketch: None,
                ceiling_tool: None,
                room_tool: None,
                section_tool: None,
                note_tool: None,
                stair_tool: None,
                dormer_tool: None,
                railing_tool: None,
                run_tool: None,
                feature_tool: None,
                dome_tool: None,
                column_tool: None,
                beam_tool: None,
                show_analytical: false,
                analysis_job: None,
                structural_walls: false,
                footing_tool: None,
                beam_system_tool: None,
                truss_tool: None,
                panel_tool: None,
                load_tool: None,
                steel_tool: None,
                rebar_sketch: None,
                bar_pick: None,
                rebar_anno: None,
                constraint_lines: Vec::new(),
                rebar_host_pick: None,
                end_pick: None,
                contour_target: None,
                adjust: None,
                auto_preview: None,
                curtain_tool: None,
                ramp_tool: None,
                measure_tool: None,
                skylight_tool: None,
                place_tool: None,
                array_tool: None,
                library_pane: None,
                clash_pane: None,
                checks_pane: None,
                results_pane: None,
                auto_check_keys: None,
                thumb_renderer: None,
                array_count: 4,
                array_last: false,
                ceiling_follow_roof: false,
                floor_type: None,
                mouse: (0, 0),
                offset_distance: 1.0,
                insert_types: std::collections::HashMap::new(),
                grid_type: None,
                type_rows: None,
                wall_type: None,
                wall_type_row: None,
                quick_pair: None,
                menu_runner: None,
                radial: None,
                ui_settings: radial::UiSettings::load(),
                chrome: fullscreen::Chrome::default(),
                system_browser: None,
                mep_marks: HashMap::new(),
                inspector: None,
                calculator: None,
                terminal: None,
                help: None,
                script_units: Rc::new(RefCell::new(Default::default())),
                timer: Some(Timer::new(&frame)),
                loading: Vec::new(),
                load_timer: Some(Timer::new(&frame)),
                browser,
                ribbon: ribbon_bar,
                browser_stale: true,
                browser_target: None,
                plan_edges: HashMap::new(),
                keys: (String::new(), std::time::Instant::now()),
                slice_pick: None,
                tiled: None,
                attach_pick: None,
                host_pick: None,
                area_pick: None,
                group_pick: None,
                site_pick: None,
                text_pick: None,
                wall_location: Default::default(),
                model_tree,
                tree: inspect::ModelTree {
                    stale: true,
                    ..Default::default()
                },
            })
        });
        // Scripts' forms show as dialogs; the Scripts menu lists the known scripts.
        {
            let mut g = gui.borrow_mut();
            let units = g.script_units.clone();
            scripts::install_form_handler(&mut g, units);
            g.refresh_scripts_menu();
        }
        {
            let g = gui.clone();
            let state = gui.borrow();
            state
                .load_timer
                .as_ref()
                .expect("created")
                .on_tick(move |_| {
                    with(&g, |g| {
                        if !g.poll_loading()
                            && let Some(t) = &g.load_timer
                        {
                            t.stop();
                        }
                    });
                });
        }
        {
            let g = gui.clone();
            let timer = gui.borrow();
            timer.timer.as_ref().expect("created").on_tick(move |_| {
                with(&g, |g| {
                    if !g.tick()
                        && let Some(t) = &g.timer
                    {
                        t.stop();
                    }
                });
            });
        }

        {
            let gui = gui.clone();
            wx::on_tab_event(&notebook, move |event| {
                with(&gui, |g| match event {
                    wx::TabEvent::Changed(i) => {
                        if let Some(v) = g.view_at(i) {
                            g.activate_view(v);
                        }
                    }
                    wx::TabEvent::Close(i) | wx::TabEvent::MiddleClick(i) => {
                        if let Some(v) = g.view_at(i) {
                            g.close_view(v);
                        } else if !g.close_help(i) {
                            g.cancel_loading(i);
                        }
                    }
                    wx::TabEvent::RightClick(_) => {}
                });
            });
        }
        {
            let gui = gui.clone();
            browser.on_item_activated(move |e| {
                if let Some(item) = e.get_item() {
                    with(&gui, |g| match g.browser_view(&item) {
                        Some(level) if g.is_level(level) => g.select_level(level),
                        Some(view) => g.open_model_view(view),
                        None => {}
                    });
                }
            });
        }
        {
            let (gui, b) = (gui.clone(), browser);
            browser.on_item_right_click(move |e| {
                let Some(item) = e.get_item() else { return };
                let (mut target, mut level) = (None, false);
                with(&gui, |g| {
                    target = g.browser_view(&item);
                    level = target.is_some_and(|t| g.is_level(t));
                    g.browser_target = target;
                });
                if level {
                    let mut menu = Menu::builder()
                        .append_item(ID_BROWSER_RENAME, "&Rename...", "")
                        .append_item(ID_BROWSER_ELEVATION, "Change &Elevation...", "")
                        .append_separator()
                        .append_item(ID_BROWSER_DELETE, "De&lete", "")
                        .build();
                    b.popup_menu(&mut menu, None);
                } else if target.is_some() {
                    let mut menu = Menu::builder()
                        .append_item(ID_BROWSER_OPEN, "&Open", "")
                        .append_item(ID_BROWSER_RENAME, "&Rename...", "")
                        .append_item(ID_BROWSER_DUPLICATE, "&Duplicate", "")
                        .append_separator()
                        .append_item(ID_BROWSER_DELETE, "De&lete", "")
                        .build();
                    b.popup_menu(&mut menu, None);
                }
            });
        }
        {
            let gui = gui.clone();
            model_tree.on_selection_changed(move |e| {
                if let Some(item) = e.get_item()
                    && let Ok(mut g) = gui.try_borrow_mut()
                    && let Some(node) = g.tree_elements(&item)
                {
                    g.tree_selected(node);
                }
            });
        }
        {
            let gui = gui.clone();
            model_tree.on_item_activated(move |e| {
                if let Some(item) = e.get_item() {
                    with(&gui, |g| {
                        if let Some(node) = g.tree_elements(&item) {
                            g.tree.target = Some(node);
                            g.tree_command("zoom");
                        }
                    });
                }
            });
        }
        {
            let (gui, t) = (gui.clone(), model_tree);
            model_tree.on_item_right_click(move |e| {
                let Some(item) = e.get_item() else { return };
                let mut found = false;
                with(&gui, |g| {
                    g.tree.target = g.tree_elements(&item);
                    found = g.tree.target.is_some();
                });
                if found {
                    let mut menu = Menu::builder();
                    for (i, (label, _)) in inspect::TREE_MENU.iter().enumerate() {
                        menu = if *label == "-" {
                            menu.append_separator()
                        } else {
                            menu.append_item(inspect::ID_TREE_FIRST + i as i32, label, "")
                        };
                    }
                    t.popup_menu(&mut menu.build(), None);
                }
            });
        }
        {
            let (gui, f, g) = (gui.clone(), frame, grid);
            grid.on_changed(move |e| {
                if let Some(name) = e.property_name() {
                    let text = g.get_value_as_string(&name);
                    with(&gui, |gui| gui.edit(&name, text, &f));
                }
            });
        }
        {
            // Refresh the palette outside its own event handlers.
            let (gui, g) = (gui.clone(), grid);
            frame.on_idle(move |e| {
                if let Ok(mut gui) = gui.try_borrow_mut()
                    && gui.properties_stale
                {
                    gui.show_properties(&g);
                    gui.update_captions();
                }
                if let Ok(mut gui) = gui.try_borrow_mut() {
                    // A view that has never drawn missed its first paint (for example while
                    // a dialog ran inside a command); it is drawn now.
                    for v in &gui.views {
                        if v.viewport.is_none() && v.error.is_none() && v.schedule.is_none() {
                            v.panel.refresh(false, None);
                        }
                    }
                    gui.sync_views();
                    gui.system_browser_idle();
                    gui.inspector_idle();
                    gui.calculator_sync(false);
                    if gui.browser_stale {
                        gui.rebuild_browser();
                    }
                    if gui.tree.stale && gui.aui.is_pane_shown("model_tree") {
                        gui.rebuild_model_tree();
                    }
                }
                e.skip(true);
            });
        }
        {
            let gui = gui.clone();
            frame.on_close(move |e| {
                let mut closed = false;
                with(&gui, |g| closed = g.close_all());
                if closed {
                    e.skip(true);
                } else if let WindowEventData::General(e) = &e {
                    e.veto();
                }
            });
        }
        {
            let gui = gui.clone();
            {
                let (gui, f, catalog) = (gui.clone(), frame, catalog.clone());
                *dispatch.borrow_mut() = Some(Box::new(move |id| {
                    if let Some(a) = catalog.action_of(id) {
                        perform(&gui, &f, &a.id);
                    }
                }));
            }
            let dispatch = dispatch.clone();
            let gui_runner = gui.clone();
            // Menu ids run here, from menus and from the radial menu (UI-SEL-007); false when the
            // id is not one of ours.
            let run_id: MenuRunner = Rc::new(move |id| {
                match id {
                id if id >= ribbon::ID_ACTION_FIRST => {
                    if let Ok(d) = dispatch.try_borrow()
                        && let Some(d) = d.as_ref()
                    {
                        d(id);
                    }
                }
                id if (ID_ORIENT_FIRST..ID_ORIENT_FIRST + nav::PRESETS.len() as i32)
                    .contains(&id) =>
                {
                    let dir = nav::PRESETS[(id - ID_ORIENT_FIRST) as usize].1;
                    with(&gui, |g| g.orient(glam::Vec3::from(dir)));
                }
                select::ID_QUICK_TRANSLUCENT => with(&gui, Gui::quick_translucent),
                id if (select::ID_QUICK_FIRST
                    ..select::ID_QUICK_FIRST + select::BOOLEANS.len() as i32)
                    .contains(&id) =>
                {
                    with(&gui, |g| g.quick_command(id));
                }
                id if (scripts::ID_PLUGIN_FIRST
                    ..scripts::ID_PLUGIN_FIRST + scripts::MAX_PLUGINS as i32)
                    .contains(&id) =>
                {
                    with(&gui, |g| g.run_plugin_item(id));
                }
                id if (inspect::ID_TREE_FIRST
                    ..inspect::ID_TREE_FIRST + inspect::TREE_MENU.len() as i32)
                    .contains(&id) =>
                {
                    let what = inspect::TREE_MENU[(id - inspect::ID_TREE_FIRST) as usize].1;
                    with(&gui, |g| g.tree_command(what));
                }
                ID_BROWSER_OPEN | ID_BROWSER_RENAME | ID_BROWSER_DUPLICATE | ID_BROWSER_DELETE
                | ID_BROWSER_ELEVATION => {
                    with(&gui, |g| {
                        let Some(view) = g.browser_target.take() else {
                            return;
                        };
                        if g.is_level(view) {
                            return match id {
                                ID_BROWSER_RENAME => g.rename_level(view, None),
                                ID_BROWSER_ELEVATION => g.move_level(view),
                                ID_BROWSER_DELETE => g.delete_level(view),
                                _ => {}
                            };
                        }
                        match id {
                            ID_BROWSER_OPEN => g.open_model_view(view),
                            ID_BROWSER_RENAME => g.rename_view(view),
                            ID_BROWSER_DUPLICATE => g.duplicate_view(view),
                            _ => g.delete_view(view),
                        }
                    });
                }
                _ => return false,
                }
                true
            });
            gui_runner.borrow_mut().menu_runner = Some(run_id.clone());
            frame.on_menu(move |e| {
                if !run_id(e.get_id()) {
                    e.skip(true);
                }
            });
        }

        frame.show(true);
        frame.centre();
        // The help opens at start (its home page lists the samples); a model opened from the
        // command line comes in front of it.
        Gui::build_library_pane(&gui);
        gui.borrow_mut().show_help();
        // Reports of failures: one left by a session that closed unexpectedly, now; this
        // session's, when the window is next idle after them.
        crash::show_earlier(&frame);
        terminal_ui::remove_stale_sessions();
        // Tab in a view cycles what is under the cursor (ARC-KEY-001).
        {
            let gui = gui.clone();
            wx::on_view_tab(&frame, move |shift| {
                with(&gui, |g| {
                    if let Some(panel) = g.active_view().map(|v| v.panel) {
                        g.tab_cycle(shift, &panel);
                        panel.refresh(false, None);
                    }
                });
            });
        }
        let mut test = crash::test_failure();
        frame.on_idle(move |e| {
            if let Some(kind) = test.take() {
                crash::fail(&kind);
            }
            crash::show_pending(&frame);
            e.skip(true);
        });
        if let Some(path) = &file {
            gui.borrow_mut().open(path, &frame);
        }
    })
    .map_err(|e| e.to_string())
}

/// Whether Shift and Ctrl are held now. Mouse events do not carry them through wxDragon (its
/// modifier accessors read key events only), so the keyboard is asked.
fn modifiers() -> (bool, bool) {
    (
        get_key_state(wxdragon::keycode::WXK_SHIFT),
        get_key_state(wxdragon::keycode::WXK_CONTROL),
    )
}

/// Runs the action of a wx id (ribbon, Quick Access Toolbar and menus share one dispatcher).
type Dispatch = Box<dyn Fn(i32)>;
/// Runs a menu id as the frame's menus do; false when the id is not one of ours.
type MenuRunner = Rc<dyn Fn(i32) -> bool>;

/// The elevation of a level element.
fn levels_elevation(store: &reviera_core::Store, level: ElementId) -> Option<f64> {
    store.get(level).map(reviera_core::levels::elevation)
}

/// Runs a catalog action (from the ribbon, the Quick Access Toolbar or a menu).
fn perform(gui: &Rc<RefCell<Gui>>, f: &Frame, action: &str) {
    // `REVIERA_TRACE_ACTIONS=1` logs every action the ribbon, toolbar and menus run.
    if std::env::var_os("REVIERA_TRACE_ACTIONS").is_some() {
        let mut view = String::new();
        with(gui, |g| {
            view = g
                .active_view()
                .map(|v| {
                    format!(
                        "{} ({})",
                        g.view_name(v),
                        if v.plan.is_some() { "plan" } else { "3D" }
                    )
                })
                .unwrap_or_default();
        });
        eprintln!("action {action} in view {view}");
    }
    // While a sketch is open, only sketch, view and file actions run.
    let mut sketching = false;
    with(gui, |g| sketching = g.sketch.is_some());
    let allowed = [
        "sketch.", "coordinates.", "view.", "window.", "ui.", "file.", "edit.", "app.",
    ];
    if sketching && !allowed.iter().any(|p| action.starts_with(p)) {
        return f.set_status_text("Finish ✓ or Cancel ✗ the sketch first", 0);
    }
    // A command started from the top reveal hides it (UI-DOCK-004).
    if !action.starts_with("ui.") {
        with(gui, |g| {
            if g.chrome.revealed == Some(fullscreen::Edge::Top) {
                g.reveal(None);
            }
        });
    }
    let focus = |g: &mut Gui| {
        if let Some(v) = g.active_view() {
            v.panel.set_focus();
        }
    };
    match action {
        "file.open" => {
            let dialog = FileDialog::builder(f)
                .with_message("Open IFC project")
                .with_wildcard("IFC files (*.ifc)|*.ifc;*.IFC|All files (*.*)|*.*")
                .build();
            if dialog.show_modal() == ID_OK
                && let Some(path) = dialog.get_path()
            {
                with(gui, |g| g.open(Path::new(&path), f));
            }
        }
        "file.new" => with(gui, |g| g.new_project(f)),
        "file.save" | "file.save_as" => {
            // A project that was never saved has no file yet: Save asks for one.
            let mut current = None;
            with(gui, |g| {
                if g.doc.is_some() {
                    current = Some(g.current_path());
                }
            });
            let Some(current) = current else { return };
            if action == "file.save" && current.is_some() {
                return with(gui, |g| {
                    g.save(f);
                    g.update_captions();
                });
            }
            let name = current
                .as_ref()
                .and_then(|p| p.file_name())
                .map_or("Untitled.ifc".into(), |n| n.to_string_lossy().into_owned());
            let mut dialog = FileDialog::builder(f)
                .with_message("Save IFC project as")
                .with_wildcard("IFC files (*.ifc)|*.ifc;*.IFC")
                .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
                .with_default_file(&name);
            if let Some(dir) = current.as_ref().and_then(|p| p.parent()) {
                dialog = dialog.with_default_dir(&dir.to_string_lossy());
            }
            let dialog = dialog.build();
            if dialog.show_modal() == ID_OK
                && let Some(path) = dialog.get_path()
            {
                with(gui, |g| {
                    g.save_as(Path::new(&path), f);
                    g.update_captions();
                });
            }
        }
        "export.pdf" => export_pdf(gui, f),
        "app.exit" => {
            f.close(false);
        }
        "edit.undo" | "edit.redo" => with(gui, |g| g.undo_redo(action, f)),
        "tool.modify" => with(gui, |g| {
            g.end_tool(f);
            g.end_dim_tool();
            g.end_grid_tool();
            g.end_line_tool();
            g.end_insert_tool();
            g.end_modify();
            g.end_room_tool();
            g.leave_walk();
            g.update_ribbon_context();
        }),
        "element.delete" => with(gui, |g| {
            if !g.sheet_delete() {
                g.delete_selection();
            }
        }),
        "floor.create" => with(gui, |g| {
            g.start_floor_sketch();
            focus(g);
        }),
        "ceiling.create" => with(gui, |g| {
            g.start_ceiling_tool();
            focus(g);
        }),
        "ceiling.create_auto" => with(gui, |g| {
            g.start_ceiling_tool();
            focus(g);
        }),
        "ceiling.sketch" => with(gui, |g| {
            g.start_ceiling_sketch();
            focus(g);
        }),
        "view.create_ceiling_plan" => with(gui, Gui::new_ceiling_plans),
        "view.create_section" => with(gui, |g| {
            g.start_section_tool();
            focus(g);
        }),
        "view.building_elevations" => with(gui, Gui::building_elevations),
        "view.new_schedule" => with(gui, Gui::new_schedule),
        "view.new_sheet" => with(gui, Gui::new_sheet),
        "tag.create" => with(gui, |g| {
            g.start_note_tool(notes::NoteTool::Tag);
            focus(g);
        }),
        "tag.create_room" => with(gui, |g| {
            g.start_note_tool(notes::NoteTool::RoomTag);
            focus(g);
        }),
        "text.create" => with(gui, |g| {
            g.start_note_tool(notes::NoteTool::Text);
            focus(g);
        }),
        "tag.tag_all" => with(gui, Gui::tag_all),
        "stair.create" => with(gui, |g| {
            g.start_stair_tool(false);
            focus(g);
        }),
        "deck.create" => with(gui, |g| {
            g.start_deck_sketch();
            focus(g);
        }),
        "column.place_architectural" => with(gui, |g| {
            g.start_column_tool(false);
            focus(g);
        }),
        "column.place" => with(gui, |g| {
            g.start_column_tool(true);
            focus(g);
        }),
        "column.place_at_grids" => with(gui, Gui::columns_at_grids),
        "column.load_section" => with(gui, Gui::load_column_section),
        "column.attach" => with(gui, Gui::attach_column_tops),
        "beam.create" => with(gui, |g| {
            g.start_beam_tool(false);
            focus(g);
        }),
        "brace.create" => with(gui, |g| {
            g.start_beam_tool(true);
            focus(g);
        }),
        "brace.load_section" => with(gui, Gui::load_brace_section),
        "footing.place" => with(gui, |g| {
            g.start_footing_tool("footing.place");
            focus(g);
        }),
        "pile.place" => with(gui, |g| {
            g.start_footing_tool("pile.place");
            focus(g);
        }),
        "pile_cap.place" => with(gui, |g| {
            g.start_footing_tool("pile_cap.place");
            focus(g);
        }),
        "structural_connection.create" => with(gui, Gui::connect_members),
        "structural_connection.automate" => with(gui, Gui::automate_connections),
        "plate.create" => with(gui, Gui::start_plate_tool),
        "bolt_pattern.create" => with(gui, Gui::start_pattern_tool),
        "weld.create" => with(gui, Gui::start_weld_tool),
        "steel_cut.cope" => with(gui, |g| g.pair_cut("steel_cut.cope")),
        "steel_cut.miter" => with(gui, |g| g.pair_cut("steel_cut.miter")),
        "steel_cut.saw" => with(gui, |g| g.pair_cut("steel_cut.saw")),
        "steel_cut.cut_through" => with(gui, |g| g.pair_cut("steel_cut.cut_through")),
        "steel_cut.cut_by" => with(gui, |g| g.pair_cut("steel_cut.cut_by")),
        "steel_cut.uncope" => with(gui, Gui::uncope),
        "steel_modifier.shorten" => with(gui, Gui::shorten_member),
        "steel_modifier.corner_cut" => with(gui, Gui::corner_cut),
        "steel_modifier.contour_cut" => with(gui, Gui::start_contour_cut),
        "steel_numbering.run" => with(gui, Gui::steel_numbering),
        "steel_nc.export" => with(gui, Gui::steel_nc_export),
        "structural_connection.check" => with(gui, |g| g.check_connections(false)),
        "structural_connection.presize" => with(gui, |g| g.check_connections(true)),
        "structural_connection.check_report" => with(gui, Gui::check_report),
        "structural_connection.customize" => with(gui, Gui::customize_connection),
        "custom_connection.add_elements" => with(gui, Gui::add_to_custom),
        "custom_connection.remove_elements" => with(gui, Gui::remove_from_custom),
        "custom_connection.set_parameter" => with(gui, Gui::custom_parameter),
        "rebar.place" => with(gui, Gui::place_rebar),
        "rebar_cover.assign" => with(gui, Gui::rebar_cover),
        "area_reinforcement.create" => with(gui, Gui::place_area_reinforcement),
        "area_reinforcement.remove_system" => with(gui, Gui::remove_area_system),
        "path_reinforcement.create" => with(gui, Gui::place_path_reinforcement),
        "fabric_area.create" => with(gui, Gui::place_fabric_area),
        "fabric_sheet.place" => with(gui, Gui::place_fabric_sheet),
        "reinforcement_settings.show" => with(gui, Gui::reinforcement_settings),
        "ui.toggle_analytical" => with(gui, Gui::toggle_analytical),
        "structural_analysis.run" => Gui::start_structural_analysis(gui),
        "structural_analysis.results" => with(gui, Gui::structural_results),
        "analytical_automation.physical_to_analytical" => with(gui, |g| {
            g.analytical_automation("analytical_automation.physical_to_analytical")
        }),
        "analytical_automation.analytical_to_physical" => with(gui, |g| {
            g.analytical_automation("analytical_automation.analytical_to_physical")
        }),
        "analytical_automation.disconnect" => with(gui, |g| {
            g.analytical_automation("analytical_automation.disconnect")
        }),
        "analytical_node.merge" => with(gui, |g| g.analytical_automation("analytical_node.merge")),
        "boundary_condition.at_column_bases" => with(gui, |g| {
            g.analytical_automation("boundary_condition.at_column_bases")
        }),
        "load_combination.generate" => with(gui, |g| {
            g.analytical_automation("load_combination.generate")
        }),
        "footing.place_at_columns" => with(gui, Gui::footings_at_columns),
        "beam_system.create_auto" => with(gui, |g| {
            g.start_beam_system_tool();
            focus(g);
        }),
        "beam_system.remove" => with(gui, Gui::remove_beam_systems),
        "truss.create" => with(gui, |g| {
            g.start_truss_tool();
            focus(g);
        }),
        "truss.remove" => with(gui, Gui::remove_trusses),
        "wall_foundation.create" => with(gui, Gui::wall_foundations),
        "foundation_slab.create" => with(gui, |g| {
            g.start_foundation_slab_sketch();
            focus(g);
        }),
        "column.place_slanted" => with(gui, |g| {
            g.start_slanted_column_tool();
            focus(g);
        }),
        "beam.create_on_grids" => with(gui, Gui::beams_on_grids),
        "beam.load_section" => with(gui, Gui::load_beam_section),
        "ramp.create" => with(gui, |g| {
            g.start_ramp_tool();
            focus(g);
        }),
        "component.place" => with(gui, |g| {
            g.place_component();
            focus(g);
        }),
        "mesh_object.import" => with(gui, |g| {
            g.import_object();
            focus(g);
        }),
        "ui.pane.components" => with(gui, |g| g.toggle_pane("components")),
        "library.import_files" => with(gui, Gui::import_into_library),
        "array.along_path" => with(gui, |g| {
            g.start_array_tool();
            focus(g);
        }),
        "array.unarray" => with(gui, Gui::unarray_selection),
        "component.create" => with(gui, Gui::create_component),
        "skylight.create" => with(gui, |g| {
            g.start_skylight_tool();
            focus(g);
        }),
        "tool.measure" => with(gui, |g| {
            g.start_measure_tool();
            focus(g);
        }),
        "curtain.create" => with(gui, |g| {
            g.start_curtain_tool();
            focus(g);
        }),
        "wall.edit_assembly" => with(gui, Gui::edit_assembly),
        "column.detach" => with(gui, Gui::detach_column_tops),
        "opening.create_wall" => with(gui, |g| {
            g.start_wall_opening_tool();
            focus(g);
        }),
        "opening.create_vertical" => with(gui, |g| {
            g.start_vertical_opening();
            focus(g);
        }),
        "opening.create_by_face" => with(gui, |g| {
            g.start_opening_by_face();
            focus(g);
        }),
        "shape.add_point" => with(gui, |g| {
            g.start_line_tool(datum_tools::LineMode::ShapePoint);
            focus(g);
        }),
        "shape.reset" => with(gui, Gui::reset_shape),
        "element.split_with_gap" => with(gui, |g| {
            g.start_line_tool(datum_tools::LineMode::SplitGap);
            focus(g);
        }),
        "wall.split_horizontal" => with(gui, Gui::split_horizontal),
        "stackedwall.set_rows" => with(gui, Gui::edit_stack),
        "wall.break_up" => with(gui, Gui::break_up),
        "wall.edit_profile" => with(gui, Gui::edit_wall_profile),
        "curtain.set_layout" => with(gui, Gui::edit_curtain_grid),
        "areaplan.create" => with(gui, Gui::area_plan_dialog),
        "areaboundary.create" => with(gui, Gui::area_boundaries),
        "area.place" => with(gui, |g| {
            g.start_area_tool();
            focus(g);
        }),
        "colorscheme.create" => with(gui, Gui::color_scheme_dialog),
        "group.create" => with(gui, Gui::create_group),
        "inplace.create" => with(gui, Gui::model_in_place),
        "stair.number_treads" => with(gui, Gui::number_treads),
        "mass.create" => with(gui, Gui::in_place_mass),
        "mass.set_floors" => with(gui, Gui::mass_floors),
        "wall.create_by_face" => with(gui, |g| g.by_face("wall.create_by_face")),
        "floor.create_by_mass_floor" => with(gui, |g| g.by_face("floor.create_by_mass_floor")),
        "roof.create_by_face" => with(gui, |g| g.by_face("roof.create_by_face")),
        "curtainsystem.create_by_face" => with(gui, |g| g.by_face("curtainsystem.create_by_face")),
        "element.update_to_face" => with(gui, Gui::update_to_face),
        "parts.create" => with(gui, Gui::create_parts),
        "parts.divide" => with(gui, Gui::divide_parts),
        "parts.merge" => with(gui, Gui::merge_parts),
        "parts.exclude" => with(gui, Gui::exclude_parts),
        "parts.restore" => with(gui, Gui::restore_parts),
        "view.set_parts_visibility" => with(gui, Gui::parts_visibility),
        "toposolid.create" => with(gui, Gui::create_toposolid),
        "toposolid.subdivide" => with(gui, Gui::subdivide_toposolid),
        "toposolid.create_graded_region" => with(gui, Gui::graded_region),
        "toposolid.cut_fill" => with(gui, Gui::cut_fill),
        "toposolid.simplify" => with(gui, Gui::simplify_toposolid),
        "site.set_settings" => with(gui, Gui::site_settings),
        "propertyline.create" => with(gui, Gui::property_line),
        "sitecomponent.place" => with(gui, |g| {
            g.start_site_component();
            focus(g);
        }),
        "modeltext.create" => with(gui, |g| {
            g.start_model_text();
            focus(g);
        }),
        "group.place" => with(gui, |g| {
            g.start_place_group();
            focus(g);
        }),
        "group.edit" => with(gui, Gui::edit_group),
        "group.add_members" => with(gui, Gui::group_add),
        "group.remove_members" => with(gui, Gui::group_remove),
        "group.finish" => with(gui, Gui::group_finish),
        "group.cancel" => with(gui, Gui::group_cancel),
        "group.exclude_member" => with(gui, Gui::group_exclude),
        "group.restore_member" => with(gui, Gui::group_restore),
        "group.ungroup" => with(gui, Gui::ungroup),
        "selectionset.save" => with(gui, Gui::save_selection),
        "selectionset.load" => with(gui, Gui::load_selection),
        "roof.create_glazing_type" => with(gui, Gui::sloped_glazing),
        "wallsweep.create" => with(gui, Gui::wall_sweep),
        "wall.create_by_room" => with(gui, Gui::linings),
        "wall.set_host" => with(gui, |g| {
            g.start_host_pick();
            focus(g);
        }),
        "wall.remove_host" => with(gui, Gui::remove_host),
        "wall.select_main_host" => with(gui, Gui::select_main_host),
        "wallreveal.create" => with(gui, Gui::wall_reveal),
        "slabedge.create" => with(gui, |g| {
            g.start_line_tool(datum_tools::LineMode::SlabEdge);
            focus(g);
        }),
        "roof.join" => with(gui, |g| {
            g.start_line_tool(datum_tools::LineMode::JoinRoof);
            focus(g);
        }),
        "roof.set_slope_arrow" => with(gui, |g| {
            g.start_line_tool(datum_tools::LineMode::SlopeArrow);
            focus(g);
        }),
        "roof.create_extrusion" => with(gui, |g| {
            g.start_line_tool(datum_tools::LineMode::RoofExtrusion);
            focus(g);
        }),
        "opening.create_shaft" => with(gui, |g| {
            g.start_shaft();
            focus(g);
        }),
        "railing.create" => with(gui, |g| {
            g.start_railing_tool();
            focus(g);
        }),
        "railing.create_on_edges" => with(gui, Gui::guard_edges),
        "railing.create_on_stair" => with(gui, Gui::rail_stairs),
        "overlap.find" => with(gui, Gui::check_overlaps),
        "ui.pane.clash" => Gui::toggle_clash_pane(gui),
        "ui.pane.system_browser" => Gui::toggle_system_browser(gui),
        "ui.pane.terminal" => Gui::toggle_terminal(gui),
        "ui.pane.sizing_calculator" => Gui::toggle_sizing_calculator(gui),
        "ui.remote_analysis" => with(gui, Gui::remote_analysis),
        "mep_type.load_starters" => with(gui, Gui::load_mep_types),
        "mep.place_component" => with(gui, Gui::place_mep_component),
        "mep.connect_touching" => with(gui, Gui::connect_touching),
        "duct.create" => with(gui, |g| {
            g.start_run_tool(reviera_core::mep::Domain::Duct);
            focus(g);
        }),
        "pipe.create" => with(gui, |g| {
            g.start_run_tool(reviera_core::mep::Domain::Pipe);
            focus(g);
        }),
        "duct.cap_open_ends" => with(gui, Gui::cap_open_ends),
        "gutter.create" => with(gui, Gui::create_gutter),
        "gutter.add_outlet" => with(gui, Gui::gutter_outlet),
        "cable_tray.create" => with(gui, |g| {
            g.start_run_tool(reviera_core::mep::Domain::CableTray);
            focus(g);
        }),
        "conduit.create" => with(gui, |g| {
            g.start_run_tool(reviera_core::mep::Domain::Conduit);
            focus(g);
        }),
        "cable_tray_fitting.place" => with(gui, |g| {
            g.start_fitting_tool(reviera_core::mep::Domain::CableTray);
            focus(g);
        }),
        "conduit_fitting.place" => with(gui, |g| {
            g.start_fitting_tool(reviera_core::mep::Domain::Conduit);
            focus(g);
        }),
        "mep_fitting.upgrade" => with(gui, |g| g.fitting_grade(true)),
        "mep_fitting.downgrade" => with(gui, |g| g.fitting_grade(false)),
        "conduit.create_parallel" => with(gui, Gui::parallel_conduits),
        "system.create" => with(gui, Gui::create_system),
        "system.add" => with(gui, |g| g.system_members(true)),
        "system.remove" => with(gui, |g| g.system_members(false)),
        "system.select_equipment" => with(gui, Gui::select_system_equipment),
        "system.disconnect_equipment" => with(gui, |g| g.system_action("system.disconnect_equipment", "Disconnect Equipment")),
        "system.divide" => with(gui, |g| g.system_action("system.divide", "Divide System")),
        "system_type.edit" => with(gui, Gui::edit_system_types),
        "mep_check.duct" => with(gui, |g| g.toggle_mep_check(reviera_core::mep_systems::CheckKind::Duct)),
        "mep_check.pipe" => with(gui, |g| g.toggle_mep_check(reviera_core::mep_systems::CheckKind::Pipe)),
        "mep_check.show_disconnects" => with(gui, Gui::show_disconnects),
        "mep_check.circuits" => with(gui, |g| g.toggle_mep_check(reviera_core::mep_systems::CheckKind::Circuit)),
        "circuit.create" => with(gui, Gui::create_circuit),
        "circuit.add" => with(gui, |g| g.circuit_members(true)),
        "circuit.remove" => with(gui, |g| g.circuit_members(false)),
        "circuit.select_panel" => with(gui, Gui::select_circuit_panel),
        "circuit.disconnect_panel" => with(gui, Gui::disconnect_circuit_panel),
        "panel.settings" => with(gui, Gui::panel_settings),
        "panel.set_supply" => with(gui, Gui::select_supply),
        "circuit.properties" => with(gui, Gui::circuit_properties),
        "mep.size" => with(gui, Gui::size_runs),
        "mep_run.create_riser" => with(gui, Gui::create_riser),
        "mech_control_device.place" => with(gui, Gui::place_control_device),
        "mech_control_device.assign" => with(gui, Gui::assign_controlled),
        "mep_run.bind_riser" => with(gui, Gui::bind_riser),
        "mep_network.calculate" => with(gui, Gui::calculate_networks),
        "analytical_segment.list" => with(gui, Gui::analytical_segments),
        "system_inspector.start" => with(gui, Gui::start_inspector),
        "system_inspector.properties" => with(gui, Gui::inspector_properties),
        "system_inspector.stop" => with(gui, |g| g.stop_inspector("System Inspector closed")),
        "pressure_loss_report.export" => with(gui, Gui::pressure_loss_report),
        "mep.penetrations" => with(gui, Gui::penetrations),
        "fixture_unit_method.set" => with(gui, Gui::import_fixture_table),
        "wiring.settings" => with(gui, Gui::wiring_settings),
        "panel.hierarchy" => with(gui, Gui::panel_hierarchy),
        "panel_schedule.create" => with(gui, Gui::create_panel_schedules),
        "panel_schedule.change_panel" => with(gui, Gui::change_schedule_panel),
        "load_classification.manage" => with(gui, Gui::load_classifications),
        "demand_factor.manage" => with(gui, Gui::demand_factors),
        "ui.pane.analytical_checks" => Gui::toggle_checks_pane(gui),
        "ui.pane.results" => Gui::toggle_results_pane(gui),
        "load_case.manage" => Gui::load_cases_dialog(gui, "cases"),
        "analytical_model.reset" => with(gui, Gui::reset_analytical),
        "analytical_model.adjust" => with(gui, Gui::start_adjust),
        "load.create_point" => with(gui, |g| g.start_load_tool("point")),
        "load.create_line" => with(gui, |g| g.start_load_tool("line")),
        "load.create_area" => with(gui, |g| g.start_load_tool("area")),
        "boundary_condition.create" => with(gui, |g| g.start_load_tool("support")),
        "analytical_panel.create_by_boundary" => with(gui, Gui::start_analytical_panel_sketch),
        "analytical_panel.create_by_extrusion" => with(gui, |g| {
            g.start_panel_tool(analytical_panels::PanelMode::Extrude)
        }),
        "ui.analytical_panel_curved" => with(gui, |g| {
            g.start_panel_tool(analytical_panels::PanelMode::Curved)
        }),
        "analytical_panel.split" => with(gui, |g| {
            g.start_panel_tool(analytical_panels::PanelMode::Split)
        }),
        "analytical_opening.create" => with(gui, Gui::start_analytical_opening_sketch),
        "analytical_panel.set_cladding" => with(gui, Gui::toggle_panel_cladding),
        "analytical_model.check_supports" => Gui::run_analytical_check(gui, true, false),
        "analytical_model.check_consistency" => Gui::run_analytical_check(gui, false, true),
        "script.run" => with(gui, Gui::open_script),
        "script.import" => with(gui, Gui::import_script),
        "ui.scripts.manage" => with(gui, Gui::manage_scripts),
        "geometry.remove_boolean" => with(gui, Gui::remove_booleans),
        "view.set_virtual_volumes" => with(gui, Gui::cycle_virtual_volumes),
        "clearance.set_rule_set" => with(gui, Gui::choose_rule_set),
        "ui.model_properties" => with(gui, Gui::model_properties),
        // F11 also leaves focus mode (the Exit Full Screen button runs it in either mode).
        "ui.fullscreen.toggle" => with(gui, |g| {
            if g.chrome.mode == fullscreen::Mode::Focus {
                g.leave_chrome();
            } else {
                g.toggle_chrome(true);
            }
        }),
        "ui.focus_mode.toggle" => with(gui, |g| g.toggle_chrome(false)),
        "ui.radial.toggle" => with(gui, |g| {
            g.ui_settings.radial_menu = !g.ui_settings.radial_menu;
            g.ui_settings.save();
            let text = if g.ui_settings.radial_menu {
                "Radial menu on: middle click opens the ring (experimental)"
            } else {
                "Radial menu off: middle click opens the quick menu"
            };
            g.frame.set_status_text(text, 0);
        }),
        "structural_settings.show" => with(gui, Gui::structural_settings),
        "file.close" => with(gui, Gui::close_model),
        "help.home" => with(gui, Gui::show_help),
        "help.about" => with(gui, |g| g.about()),
        "stair.create_openings" => with(gui, |g| {
            let store = g.store_now();
            let stairs: Vec<ElementId> = g
                .selection()
                .into_iter()
                .filter(|i| {
                    store
                        .as_ref()
                        .and_then(|s| s.get(*i))
                        .and_then(reviera_core::stairs::stair_recipe)
                        .is_some()
                })
                .collect();
            if stairs.is_empty() {
                return g.frame.set_status_text("Select a stair first", 0);
            }
            for s in stairs {
                g.offer_stair_openings(s, true);
            }
        }),
        "stair.spiral" => with(gui, |g| {
            g.start_stair_tool(true);
            focus(g);
        }),
        "stair.winder" => with(gui, |g| {
            g.start_winder_tool();
            focus(g);
        }),
        "sheet.place_view" => with(gui, Gui::place_view_on_sheet),
        "sheet.place_schedule" => with(gui, Gui::place_schedule_on_sheet),
        "sheet.open_view" => with(gui, Gui::open_sheet_view),
        "schedule.edit_fields" => with(gui, Gui::schedule_fields),
        "schedule.edit_sort" => with(gui, Gui::schedule_sort),
        "schedule.edit_filter" => with(gui, Gui::schedule_filter),
        "schedule.itemize" => with(gui, Gui::schedule_itemize),
        "schedule.export" => with(gui, Gui::export_schedule),
        "export.cobie" => with(gui, Gui::export_cobie),
        "room.place" => with(gui, |g| {
            g.start_room_tool();
            focus(g);
        }),
        "room.place_automatically" => with(gui, |g| {
            g.place_rooms_automatically();
            focus(g);
        }),
        "roof.create_footprint" => with(gui, |g| {
            g.start_roof_sketch();
            focus(g);
        }),
        "roof.create_arched" => with(gui, |g| {
            g.start_arched_roof_sketch();
            focus(g);
        }),
        "roof.create_dome" => with(gui, |g| {
            g.start_dome_tool();
            focus(g);
        }),
        "roof.edit_footprint" => with(gui, |g| {
            g.edit_roof_footprint();
            focus(g);
        }),
        "wall.attach" => with(gui, |g| {
            g.start_attach();
            focus(g);
        }),
        "wall.detach" => with(gui, Gui::detach_selection),
        "roof.attach_walls" => with(gui, Gui::attach_walls_below),
        "roof.set_eaves" => with(gui, Gui::set_roof_eaves),
        "railing.add_opening" => with(gui, |g| {
            g.start_feature_tool(railings::FeatureKind::Opening);
            focus(g);
        }),
        "railing.add_gate" => with(gui, |g| {
            g.start_feature_tool(railings::FeatureKind::Gate { leaves: 1 });
            focus(g);
        }),
        "railing.add_double_gate" => with(gui, |g| {
            g.start_feature_tool(railings::FeatureKind::Gate { leaves: 2 });
            focus(g);
        }),
        "dormer.create" => with(gui, |g| {
            g.start_dormer_tool();
            focus(g);
        }),
        "sketch.defines_slope" => with(gui, |g| {
            g.set_draw_mode(sketch::DrawMode::DefinesSlope);
            focus(g);
        }),
        "sketch.overhang" => with(gui, |g| {
            g.set_roof_overhang();
            focus(g);
        }),
        "floor.edit_boundary" | "ceiling.edit_boundary" => with(gui, |g| {
            g.edit_floor_boundary();
            focus(g);
        }),
        "sketch.finish" => with(gui, Gui::finish_sketch),
        "sketch.cancel" => with(gui, Gui::cancel_sketch),
        "sketch.select" | "sketch.pick_walls" | "sketch.line" | "sketch.rectangle"
        | "sketch.arc" | "sketch.circle" => with(gui, |g| {
            let mode = match action {
                "sketch.select" => sketch::DrawMode::Select,
                "sketch.pick_walls" => sketch::DrawMode::PickWalls,
                "sketch.line" => sketch::DrawMode::Line,
                "sketch.arc" => sketch::DrawMode::Arc,
                "sketch.circle" => sketch::DrawMode::Circle,
                _ => sketch::DrawMode::Rectangle,
            };
            g.set_draw_mode(mode);
            focus(g);
        }),
        "element.pin" | "element.unpin" => with(gui, |g| g.pin_selection(action == "element.pin")),
        a if modify::Kind::of_action(a).is_some() => with(gui, |g| {
            g.start_modify(modify::Kind::of_action(a).expect("checked"));
            focus(g);
        }),
        "wall.create" => with(gui, |g| {
            g.start_wall_tool(f);
            focus(g);
        }),
        "wall.create_structural" => with(gui, |g| {
            g.start_wall_tool(f);
            if g.tool.is_some() {
                g.structural_walls = true;
                f.set_status_text("Structural Wall: click the start (bearing walls; change the usage in Properties)", 0);
            }
            focus(g);
        }),
        "floor.create_structural" => with(gui, |g| {
            g.start_structural_floor_sketch();
            focus(g);
        }),
        "wall.create_arc" => with(gui, |g| {
            g.start_shaped_wall_tool(f, WallShape::Arc);
            focus(g);
        }),
        "wall.create_spline" => with(gui, |g| {
            g.start_shaped_wall_tool(f, WallShape::Spline);
            focus(g);
        }),
        "door.place" | "window.place" => {
            let kind = if action == "door.place" {
                reviera_core::inserts::Kind::Door
            } else {
                reviera_core::inserts::Kind::Window
            };
            with(gui, |g| {
                g.start_insert_tool(kind);
                focus(g);
            });
        }
        "level.create" => with(gui, Gui::new_level),
        "grid.create" => with(gui, |g| {
            g.start_grid_tool();
            focus(g);
        }),
        "refplane.create" => with(gui, |g| {
            g.start_line_tool(datum_tools::LineMode::RefPlane);
            focus(g);
        }),
        "modelline.create" => with(gui, |g| {
            g.start_line_tool(datum_tools::LineMode::ModelLine);
            focus(g);
        }),
        "roomseparator.create" => with(gui, |g| {
            g.start_line_tool(datum_tools::LineMode::RoomSeparator);
            focus(g);
        }),
        "workplane.set" => with(gui, Gui::set_work_plane),
        "scopebox.create" => with(gui, |g| {
            g.start_line_tool(datum_tools::LineMode::ScopeBox);
            focus(g);
        }),
        "datum.reset_to_3d" => with(gui, Gui::datum_reset_to_3d),
        "view.reveal_constraints" => with(gui, Gui::toggle_reveal_constraints),
        "constraint.lock_dimension" => with(gui, Gui::lock_selected_dimensions),
        "constraint.set_equality" => with(gui, Gui::toggle_dimension_eq),
        "datum.maximize_3d" => with(gui, Gui::datum_maximize_3d),
        "datum.propagate_extents" => with(gui, Gui::datum_propagate),
        "workplane.show" => with(gui, Gui::toggle_work_plane),
        "coordinates.edit" => with(gui, Gui::coordinates),
        "rebar_view.all" => with(gui, |g| g.rebar_presentation(reviera_core::rebar_views::Mode::All)),
        "rebar_view.first_last" => with(gui, |g| g.rebar_presentation(reviera_core::rebar_views::Mode::FirstLast)),
        "rebar_view.middle" => with(gui, |g| g.rebar_presentation(reviera_core::rebar_views::Mode::Middle)),
        "rebar_view.select" => with(gui, |g| g.rebar_presentation(reviera_core::rebar_views::Mode::Select)),
        "rebar_view.states" => with(gui, Gui::rebar_view_states),
        "reinforcement_numbering.show" => with(gui, Gui::reinforcement_numbers),
        "rebar_tag.place" => with(gui, |g| g.start_rebar_annotation(rebar_view_ui::AnnoTool::Tag)),
        "multi_rebar_annotation.place" => with(gui, |g| g.start_rebar_annotation(rebar_view_ui::AnnoTool::Aligned)),
        "rebar_view.multi_linear" => with(gui, |g| g.start_rebar_annotation(rebar_view_ui::AnnoTool::Linear)),
        "bending_detail.place" => with(gui, |g| g.start_rebar_annotation(rebar_view_ui::AnnoTool::Bending)),
        "rebar_schedule.create" => with(gui, Gui::rebar_schedule),
        "rebar.remove_bar" => with(gui, |g| g.start_bar_edit(rebar_view_ui::BarPurpose::Remove)),
        "rebar.split_set" => with(gui, |g| g.start_bar_edit(rebar_view_ui::BarPurpose::Split)),
        "rebar.move_bar_in_set" => with(gui, |g| g.start_bar_edit(rebar_view_ui::BarPurpose::Move)),
        "rebar.reset_bars" => with(gui, Gui::reset_rebar_bars),
        "rebar.convert_to_varying" => with(gui, |g| g.convert_rebar_sets(true)),
        "rebar.sketch" => with(gui, Gui::sketch_rebar),
        "rebar.place_free_form" => with(gui, Gui::place_free_form_rebar),
        "precast.split" => with(gui, Gui::precast_split),
        "precast.resplit" => with(gui, Gui::precast_resplit),
        "precast.reinforce" => with(gui, Gui::precast_reinforce),
        "precast.shop_drawings" => with(gui, Gui::precast_shop_drawings),
        "precast.cam_export" => with(gui, Gui::precast_cam_export),
        "precast.place_mounting_parts" => with(gui, Gui::precast_mounting_parts),
        "precast.generate_custom_fabric" => with(gui, Gui::precast_custom_fabric),
        "precast_config.show" => with(gui, Gui::precast_configuration),
        "rebar.distribute_along_path" => with(gui, Gui::distribute_along_path),
        "rebar.distribute_morphed" => with(gui, Gui::distribute_morphed),
        "rebar.constrain" => with(gui, Gui::edit_rebar_constraints),
        "rebar.reset_constraints" => with(gui, Gui::reset_rebar_constraints),
        "rebar.convert_to_standard" => with(gui, |g| g.convert_rebar_sets(false)),
        "rebar.propagate" => with(gui, |g| g.start_rebar_host_pick(true)),
        "rebar.pick_new_host" => with(gui, |g| g.start_rebar_host_pick(false)),
        "rebar.splice" => with(gui, Gui::splice_rebar),
        "rebar_coupler.place" => with(gui, Gui::start_coupler),
        "dim.create_aligned" => with(gui, |g| {
            g.start_dim_tool(f);
            focus(g);
        }),
        "wall.flip" | "insert.flip_hand" | "insert.flip_facing" => with(gui, |g| {
            let ids = g.selection();
            let key = if action == "wall.flip" {
                "wall_ids"
            } else {
                "element_ids"
            };
            let store = g
                .doc
                .and_then(|d| g.app.document(d))
                .map(|d| d.doc.store().clone());
            let ids: Vec<ElementId> = ids
                .into_iter()
                .filter(|id| {
                    store.as_ref().and_then(|s| s.get(*id)).is_some_and(|e| {
                        if action == "wall.flip" {
                            reviera_core::walls::wall_recipe(e).is_some()
                        } else {
                            reviera_core::inserts::kind_of(e).is_some()
                        }
                    })
                })
                .collect();
            if ids.is_empty() {
                return f.set_status_text("Select the elements to flip first", 0);
            }
            if let Err(e) = g.run(action, json!({ key: ids })) {
                f.set_status_text(&e, 0);
            }
            g.refresh_scene(f);
        }),
        "view.zoom_fit" => with(gui, Gui::zoom_to_fit),
        "view.zoom_previous" => with(gui, |g| g.history(true)),
        "view.zoom_next" => with(gui, |g| g.history(false)),
        "view.home" => with(gui, Gui::go_home),
        "view.set_home" => with(gui, Gui::set_home),
        "view.set_projection" => with(gui, Gui::toggle_perspective),
        "view.walk" => with(gui, Gui::toggle_walk),
        "view.create_3d" => with(gui, Gui::new_3d_view),
        "view.default_3d" => with(gui, Gui::default_3d_view),
        "view.create_plan" => with(gui, Gui::new_floor_plans),
        "window.close" => with(gui, |g| {
            if let Some(v) = g.active {
                g.close_view(v);
            }
        }),
        "window.next" => with(gui, |g| g.cycle_view(1)),
        "window.previous" => with(gui, |g| g.cycle_view(-1)),
        "window.tile" => with(gui, Gui::tile),
        "window.tab_all" => with(gui, Gui::tab_all),
        "ui.pane.properties" | "ui.pane.browser" | "ui.pane.model_tree" => {
            let pane = action.trim_start_matches("ui.pane.");
            with(gui, |g| g.toggle_pane(pane));
        }
        "inspect.section_box" => with(gui, Gui::toggle_section_box),
        "inspect.selection_box" => with(gui, Gui::selection_box),
        "inspect.slice_horizontal" | "inspect.slice_vertical" => with(gui, |g| {
            g.start_slice(if action == "inspect.slice_horizontal" {
                inspect::SliceKind::Horizontal
            } else {
                inspect::SliceKind::Vertical
            });
            focus(g);
        }),
        "inspect.clear_slices" => with(gui, Gui::clear_slices),
        "inspect.reset" => with(gui, Gui::temp_reset),
        a if a.starts_with("inspect.") => with(gui, |g| g.temp_action(a)),
        other => f.set_status_text(&format!("{other} is not available yet"), 0),
    }
}

/// File › Export PDF: the active plan, at its scale, to a chosen file.
fn export_pdf(gui: &Rc<RefCell<Gui>>, f: &Frame) {
    let mut target = None;
    with(gui, |g| {
        target = g
            .active_view()
            .filter(|v| v.is_2d())
            .and_then(|v| Some((v.element?, g.view_name(v), g.current_path())));
    });
    let Some((view, name, model_path)) = target else {
        return error(f, "Open a plan, section or elevation to export it as PDF.");
    };
    let mut dialog = FileDialog::builder(f)
        .with_message("Export PDF")
        .with_wildcard("PDF files (*.pdf)|*.pdf")
        .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
        .with_default_file(&format!("{name}.pdf"));
    if let Some(dir) = model_path.as_ref().and_then(|p| p.parent()) {
        dialog = dialog.with_default_dir(&dir.to_string_lossy());
    }
    let dialog = dialog.build();
    if dialog.show_modal() == ID_OK
        && let Some(path) = dialog.get_path()
    {
        let mut path = PathBuf::from(path);
        if path.extension().is_none() {
            path.set_extension("pdf");
        }
        with(gui, |g| {
            let args = json!({ "range": { "views": [view] }, "path_or_folder": path });
            match g.run("export.pdf", args) {
                Ok(v) => f.set_status_text(
                    &format!(
                        "Exported {} ({})",
                        path.display(),
                        v["sheets"][0].as_str().unwrap_or_default()
                    ),
                    0,
                ),
                Err(e) => error(f, &format!("The PDF was not written:\n{e}")),
            }
        });
    }
}
