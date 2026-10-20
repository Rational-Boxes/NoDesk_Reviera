// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The ribbon, Quick Access Toolbar and menus (spec Ch 1 §1.7–1.9), built from one catalog,
//! `ribbon-default.toml` (UI-RIBCAT-001).
//!
//! The ribbon follows the tabbed tool-ribbon convention of design applications: tabs of titled
//! panels with large and small icon buttons, the Select panel first on every tab, a permanent
//! Modify tab, and a contextual "Modify | <category>" tab, highlighted, while elements are
//! selected. It is a `wxRibbonBar` (bound through `native/wxshim.cpp`), which also collapses
//! panels as the window narrows. The Quick Access Toolbar is a slim `wxAuiToolBar` above it.
//! Menus list the same actions with their shortcuts and key sequences.

use std::ffi::{CString, c_int, c_void};

use toml_edit::{DocumentMut, Item, Value};
use wxdragon::prelude::*;

use crate::icons;

/// The catalog shipped with Reviera.
pub const DEFAULT: &str = include_str!("../ribbon-default.toml");

/// Actions the window performs itself (tools, navigation, panes) rather than one registered
/// command run as it is.
#[cfg(test)]
pub const UI_ACTIONS: &[&str] = &[
    "app.exit",
    "ui.model_properties",
    "ui.fullscreen.toggle",
    "ui.focus_mode.toggle",
    "ui.radial.toggle",
    "ui.pane.system_browser",
    "ui.pane.terminal",
    "ui.remote_analysis",
    "load_classification.manage",
    "demand_factor.manage",
    "wiring.settings",
    "mep.place_component",
    "system_type.edit",
    "mep_check.duct",
    "mep_check.pipe",
    "mep_check.circuits",
    "panel.settings",
    "ui.pane.clash",
    "ui.pane.analytical_checks",
    "ui.pane.results",
    "load_case.manage",
    "analytical_model.adjust",
    "ui.analytical_panel_curved",
    "help.home",
    "wall.edit_assembly",
    "tool.measure",
    "coordinates.edit",
    "rebar_view.all",
    "rebar_view.first_last",
    "rebar_view.middle",
    "rebar_view.select",
    "rebar_view.states",
    "precast_config.show",
    "rebar_view.multi_linear",
    "reinforcement_numbering.show",
    "help.about",
    "tool.modify",
    "wall.create_structural",
    "structural_settings.show",
    "reinforcement_settings.show",
    "ui.toggle_analytical",
    "floor.create_structural",
    "ui.pane.properties",
    "ui.pane.browser",
    "view.zoom_fit",
    "view.zoom_previous",
    "view.zoom_next",
    "view.home",
    "view.walk",
    "window.next",
    "window.previous",
    "window.tile",
    "window.tab_all",
    "window.close",
    "view.default_3d",
    "element.mirror_pick",
    "element.mirror_draw",
    "sketch.pick_walls",
    "sketch.line",
    "sketch.select",
    "stair.spiral",
    "stair.winder",
    "wall.create_arc",
    "wall.create_spline",
    "railing.add_double_gate",
    "sketch.rectangle",
    "sketch.arc",
    "sketch.circle",
    "sketch.finish",
    "sketch.cancel",
    "ceiling.sketch",
    "sketch.defines_slope",
    "view.building_elevations",
    "ui.scripts.manage",
    "view.new_schedule",
    "view.new_sheet",
    "sheet.open_view",
    "schedule.edit_fields",
    "schedule.edit_sort",
    "schedule.itemize",
    "sketch.overhang",
    "view.create_ceiling_plan",
    "inspect.section_box",
    "inspect.selection_box",
    "inspect.slice_horizontal",
    "inspect.slice_vertical",
    "inspect.clear_slices",
    "inspect.hide",
    "inspect.hide_subtree",
    "inspect.hide_category",
    "inspect.hide_type",
    "inspect.isolate",
    "inspect.isolate_subtree",
    "inspect.isolate_category",
    "inspect.isolate_type",
    "inspect.ghost_others",
    "inspect.translucent",
    "inspect.translucent_subtree",
    "inspect.reset",
    "ui.pane.model_tree",
    "ui.pane.components",
];

/// First wx id of the catalog's actions (one id per action, in catalog order).
pub const ID_ACTION_FIRST: i32 = ID_HIGHEST + 200;

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub icon: String,
    /// A menu accelerator (`Ctrl+S`).
    pub accel: Option<String>,
    /// A key sequence typed in a view (`WA`).
    pub keys: Option<String>,
    pub help: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub action: String,
    pub large: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelDef {
    pub label: String,
    pub tools: Vec<Tool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TabDef {
    pub id: String,
    pub label: String,
    pub panels: Vec<PanelDef>,
}

/// A contextual tab: for selections of one category, its own panels after the Modify tab's.
#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    pub category: String,
    pub label: String,
    pub panels: Vec<PanelDef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuDef {
    pub label: String,
    /// Action ids, `-` for a separator, or `@name` for a submenu the window supplies.
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Catalog {
    pub actions: Vec<Action>,
    pub quick_access: Vec<String>,
    pub tabs: Vec<TabDef>,
    pub contexts: Vec<Context>,
    pub menus: Vec<MenuDef>,
}

fn text(v: Option<&Item>) -> Option<String> {
    v?.as_str().map(str::to_owned)
}

fn strings(v: Option<&Item>) -> Vec<String> {
    v.and_then(Item::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn tools(v: Option<&Value>) -> Vec<Tool> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let t = t.as_inline_table()?;
                    Some(Tool {
                        action: t.get("action")?.as_str()?.to_owned(),
                        large: t.get("size").and_then(Value::as_str) != Some("small"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn panel_of(label: Option<&str>, tool_list: Option<&Value>) -> PanelDef {
    PanelDef {
        label: label.unwrap_or_default().to_owned(),
        tools: tools(tool_list),
    }
}

impl Catalog {
    /// Parses a catalog; `Err` names what is wrong.
    pub fn parse(source: &str) -> Result<Self, String> {
        let doc: DocumentMut = source.parse().map_err(|e| format!("ribbon catalog: {e}"))?;
        let mut c = Catalog::default();
        for a in doc
            .get("action")
            .and_then(Item::as_array_of_tables)
            .into_iter()
            .flatten()
        {
            let get = |k: &str| text(a.get(k));
            c.actions.push(Action {
                id: get("id").ok_or("an action without an id")?,
                label: get("label").unwrap_or_default(),
                icon: get("icon").unwrap_or_default(),
                accel: get("accel"),
                keys: get("keys"),
                help: get("help").unwrap_or_default(),
            });
        }
        c.quick_access = strings(doc.get("quick_access").and_then(|q| q.get("actions")));
        for t in doc
            .get("tab")
            .and_then(Item::as_array_of_tables)
            .into_iter()
            .flatten()
        {
            let panels = t
                .get("panel")
                .and_then(Item::as_array_of_tables)
                .into_iter()
                .flatten()
                .map(|p| {
                    panel_of(
                        p.get("label").and_then(Item::as_str),
                        p.get("tools").and_then(Item::as_value),
                    )
                })
                .collect();
            c.tabs.push(TabDef {
                id: text(t.get("id")).unwrap_or_default(),
                label: text(t.get("label")).unwrap_or_default(),
                panels,
            });
        }
        for x in doc
            .get("context")
            .and_then(Item::as_array_of_tables)
            .into_iter()
            .flatten()
        {
            let panels = x
                .get("panels")
                .and_then(Item::as_array)
                .into_iter()
                .flatten()
                .filter_map(|p| {
                    let p = p.as_inline_table()?;
                    Some(panel_of(
                        p.get("label").and_then(Value::as_str),
                        p.get("tools"),
                    ))
                })
                .collect();
            c.contexts.push(Context {
                category: text(x.get("category")).unwrap_or_default(),
                label: text(x.get("label")).unwrap_or_default(),
                panels,
            });
        }
        for m in doc
            .get("menu")
            .and_then(Item::as_array_of_tables)
            .into_iter()
            .flatten()
        {
            c.menus.push(MenuDef {
                label: text(m.get("label")).unwrap_or_default(),
                items: strings(m.get("items")),
            });
        }
        Ok(c)
    }

    /// The wx id of an action.
    pub fn id_of(&self, action: &str) -> Option<i32> {
        self.actions
            .iter()
            .position(|a| a.id == action)
            .map(|i| ID_ACTION_FIRST + i as i32)
    }

    /// The action of a wx id.
    pub fn action_of(&self, id: i32) -> Option<&Action> {
        usize::try_from(id - ID_ACTION_FIRST)
            .ok()
            .and_then(|i| self.actions.get(i))
    }

    pub fn action(&self, id: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.id == id)
    }

    /// Every action a tab, menu or the Quick Access Toolbar uses.
    #[cfg(test)]
    pub fn used_actions(&self) -> Vec<&str> {
        let panels = self
            .tabs
            .iter()
            .flat_map(|t| &t.panels)
            .chain(self.contexts.iter().flat_map(|c| &c.panels));
        panels
            .flat_map(|p| p.tools.iter().map(|t| t.action.as_str()))
            .chain(self.quick_access.iter().map(String::as_str))
            .chain(
                self.menus
                    .iter()
                    .flat_map(|m| m.items.iter().map(String::as_str))
                    .filter(|i| *i != "-" && !i.starts_with('@')),
            )
            .collect()
    }
}

/// The tooltip of an action: its help, shortcut and key sequence.
pub fn tooltip(a: &Action) -> String {
    let mut t = a.help.clone();
    if let Some(k) = &a.keys {
        t.push_str(&format!(" ({k})"));
    }
    if let Some(k) = &a.accel {
        t.push_str(&format!(" [{k}]"));
    }
    t
}

// ---- Native ribbon ----

unsafe extern "C" {
    fn reviera_rb_new(
        parent: *mut c_void,
        callback: extern "C" fn(*mut c_void, c_int),
        user: *mut c_void,
    ) -> *mut c_void;
    fn reviera_rb_page(bar: *mut c_void, label: *const i8) -> *mut c_void;
    fn reviera_rb_panel(page: *mut c_void, label: *const i8) -> *mut c_void;
    fn reviera_rb_button(
        bar: *mut c_void,
        id: c_int,
        label: *const i8,
        svg: *const i8,
        help: *const i8,
        large: c_int,
        kind: c_int,
    );
    fn reviera_rb_clear_page(page: *mut c_void);
    fn reviera_rb_set_page_label(page: *mut c_void, label: *const i8);
    fn reviera_rb_realize(bar: *mut c_void);
    fn reviera_rb_enable(id: c_int, enable: c_int);
    fn reviera_rb_show_page(bar: *mut c_void, index: c_int, show: c_int, highlight: c_int);
    fn reviera_rb_set_active(bar: *mut c_void, index: c_int);
    fn reviera_rb_active(bar: *mut c_void) -> c_int;
    fn reviera_rb_best_height(bar: *mut c_void) -> c_int;
    fn reviera_aui_tool_svg(toolbar: *mut c_void, id: c_int, svg: *const i8, size: c_int);
}

/// Whether the chrome is dark (icons are drawn to match).
pub(crate) fn dark() -> bool {
    wxdragon::appearance::is_system_dark_mode()
}

fn c(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

type Handler = Box<dyn Fn(i32)>;

extern "C" fn trampoline(user: *mut c_void, id: c_int) {
    // SAFETY: `user` is the handler leaked in `Ribbon::new`, alive for the program.
    let handler = unsafe { &*(user as *const Handler) };
    handler(id);
}

/// The ribbon of the main window.
pub struct Ribbon {
    bar: *mut c_void,
    catalog: Catalog,
    /// The contextual page (the last page) and what it shows now.
    context_page: *mut c_void,
    context_index: i32,
    context_key: Option<String>,
    /// The page that was active before the contextual tab appeared.
    previous: i32,
    menu_bar: Option<MenuBar>,
    /// Actions disabled now, for the radial menu (UI-SEL-007).
    disabled: std::cell::RefCell<std::collections::HashSet<String>>,
    /// The contextual tab shown now: its label and panels.
    context_shown: Option<(String, Vec<PanelDef>)>,
}

impl Ribbon {
    /// Builds the ribbon in `parent`; `on_click` receives the action wx ids.
    pub fn new(parent: &Panel, catalog: Catalog, on_click: impl Fn(i32) + 'static) -> Self {
        let user = Box::into_raw(Box::new(Box::new(on_click) as Handler));
        let bar = unsafe {
            reviera_rb_new(
                parent.handle_ptr() as *mut c_void,
                trampoline,
                user as *mut c_void,
            )
        };
        let mut r = Self {
            bar,
            catalog,
            context_page: std::ptr::null_mut(),
            context_index: 0,
            context_key: None,
            previous: 0,
            menu_bar: None,
            disabled: Default::default(),
            context_shown: None,
        };
        for tab in r.catalog.tabs.clone() {
            let page = unsafe { reviera_rb_page(bar, c(&tab.label).as_ptr()) };
            r.fill(page, &tab.panels);
        }
        r.context_index = r.catalog.tabs.len() as i32;
        r.context_page = unsafe { reviera_rb_page(bar, c("Modify").as_ptr()) };
        unsafe {
            reviera_rb_realize(bar);
            reviera_rb_show_page(bar, r.context_index, 0, 0);
            reviera_rb_set_active(bar, 0);
        }
        r
    }

    fn fill(&self, page: *mut c_void, panels: &[PanelDef]) {
        for p in panels {
            let bar = unsafe { reviera_rb_panel(page, c(&p.label).as_ptr()) };
            for t in &p.tools {
                let (Some(a), Some(id)) = (
                    self.catalog.action(&t.action),
                    self.catalog.id_of(&t.action),
                ) else {
                    continue;
                };
                let svg = c(&icons::svg(&a.icon, dark()));
                let label = c(&a.label);
                let help = c(&tooltip(a));
                unsafe {
                    reviera_rb_button(
                        bar,
                        id,
                        label.as_ptr(),
                        svg.as_ptr(),
                        help.as_ptr(),
                        c_int::from(t.large),
                        0,
                    );
                }
            }
        }
    }

    /// The ribbon's height for its pane.
    pub fn height(&self) -> i32 {
        unsafe { reviera_rb_best_height(self.bar) }
    }

    /// Shows the contextual tab for a selection: `None` hides it; otherwise its label is
    /// "Modify | <label>" with the Modify tab's panels and the category's own (`category`).
    pub fn set_context(&mut self, context: Option<(&str, Option<&str>)>) {
        let key = context.map(|(label, cat)| format!("{label}|{}", cat.unwrap_or_default()));
        if key == self.context_key {
            return;
        }
        let was_shown = self.context_key.is_some();
        self.context_key = key;
        self.context_shown = None;
        let Some((label, category)) = context else {
            // Back to the tab before the selection, unless another was chosen meanwhile.
            unsafe {
                let active = reviera_rb_active(self.bar);
                reviera_rb_show_page(self.bar, self.context_index, 0, 0);
                let back = if active == self.context_index {
                    self.previous
                } else {
                    active
                };
                reviera_rb_set_active(self.bar, back);
                reviera_rb_realize(self.bar);
            }
            return;
        };
        if !was_shown {
            self.previous = unsafe { reviera_rb_active(self.bar) };
        }
        // Sketch sessions show only their own panels; selections start with the Modify tab's.
        let session = category.is_some_and(|k| k.starts_with("session."));
        let mut panels: Vec<PanelDef> = if session {
            Vec::new()
        } else {
            self.catalog
                .tabs
                .iter()
                .find(|t| t.id == "modify")
                .map(|t| t.panels.clone())
                .unwrap_or_default()
        };
        if let Some(cx) =
            category.and_then(|k| self.catalog.contexts.iter().find(|c| c.category == k))
        {
            panels.extend(cx.panels.clone());
        }
        unsafe {
            reviera_rb_clear_page(self.context_page);
            reviera_rb_set_page_label(self.context_page, c(&format!("Modify | {label}")).as_ptr());
        }
        self.fill(self.context_page, &panels);
        self.context_shown = Some((format!("Modify | {label}"), panels));
        unsafe {
            reviera_rb_show_page(self.bar, self.context_index, 1, 1);
            reviera_rb_realize(self.bar);
            reviera_rb_set_active(self.bar, self.context_index);
        }
    }

    /// The label of a selection's contextual tab: its category ("Walls"), or "Multi-Select".
    pub fn context_label(&self, category: Option<&str>) -> Option<String> {
        let cx = category.and_then(|k| self.catalog.contexts.iter().find(|c| c.category == k));
        cx.map(|c| c.label.clone())
    }

    /// Enables or disables an action's ribbon buttons and menu items.
    pub fn enable(&self, action: &str, enable: bool) {
        if let Some(id) = self.catalog.id_of(action) {
            let mut disabled = self.disabled.borrow_mut();
            if enable {
                disabled.remove(action);
            } else {
                disabled.insert(action.to_owned());
            }
            unsafe { reviera_rb_enable(id, c_int::from(enable)) }
            if let Some(bar) = &self.menu_bar {
                bar.enable_item(id, enable);
            }
        }
    }

    /// The wx id of an action.
    pub fn id_of(&self, action: &str) -> Option<i32> {
        self.catalog.id_of(action)
    }

    /// Whether an action is enabled now.
    pub fn enabled(&self, action: &str) -> bool {
        !self.disabled.borrow().contains(action)
    }

    /// The catalog the ribbon was built from.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// The contextual tab shown now: its label and panels.
    pub fn context_shown(&self) -> Option<(&str, &[PanelDef])> {
        self.context_shown.as_ref().map(|(l, p)| (l.as_str(), p.as_slice()))
    }

    /// The menu bar whose items follow the ribbon's enabled state.
    pub fn set_menu_bar(&mut self, bar: MenuBar) {
        self.menu_bar = Some(bar);
    }
}

/// Builds the Quick Access Toolbar: the catalog's actions, with their icons.
pub fn quick_access(frame: &Frame, catalog: &Catalog) -> AuiToolBar {
    let tb = AuiToolBar::builder(frame).build();
    tb.set_tool_bitmap_size(Size::new(16, 16));
    for action in &catalog.quick_access {
        let (Some(a), Some(id)) = (catalog.action(action), catalog.id_of(action)) else {
            continue;
        };
        tb.add_tool(id, &a.label, &tooltip(a), WidgetItemKind::Normal);
        let svg = c(&icons::svg(&a.icon, dark()));
        unsafe { reviera_aui_tool_svg(tb.handle_ptr() as *mut c_void, id, svg.as_ptr(), 16) };
    }
    tb.realize();
    tb
}

/// Sets an AUI toolbar tool's icon from the icon set.
pub fn set_tool_icon(tb: &AuiToolBar, id: i32, icon: &str) {
    let svg = c(&icons::svg(icon, dark()));
    unsafe { reviera_aui_tool_svg(tb.handle_ptr() as *mut c_void, id, svg.as_ptr(), 16) };
}

/// Builds the menu bar from the catalog; `submenu` supplies `@name` submenus.
pub fn menu_bar(
    catalog: &Catalog,
    submenu: &dyn Fn(&str) -> Option<(wxdragon::menus::Menu, String)>,
) -> MenuBar {
    let mut bar = MenuBar::builder();
    for m in &catalog.menus {
        let menu = wxdragon::menus::Menu::builder().build();
        for item in &m.items {
            if item == "-" {
                menu.append_separator();
                continue;
            }
            if let Some(name) = item.strip_prefix('@') {
                if let Some((sub, label)) = submenu(name) {
                    menu.append_submenu(sub, &label, "");
                }
                continue;
            }
            let (Some(a), Some(id)) = (catalog.action(item), catalog.id_of(item)) else {
                continue;
            };
            let mut label = a.label.clone();
            if let Some(k) = &a.keys {
                label.push_str(&format!(" ({k})"));
            }
            if let Some(k) = &a.accel {
                label.push_str(&format!("\t{k}"));
            }
            menu.append(id, &label, &a.help, ItemKind::Normal);
        }
        bar = bar.append(menu, &m.label);
    }
    bar.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_catalog_uses_known_actions_and_icons() {
        let c = Catalog::parse(DEFAULT).unwrap();
        // Registered commands: the core's, the store's and the application's.
        let app = reviera_app::App::new(reviera_app::AppConfig::default());
        let known = |id: &str| UI_ACTIONS.contains(&id) || app.registry().get(id).is_some();
        for a in &c.actions {
            assert!(known(&a.id), "unknown action {}", a.id);
            assert!(icons::exists(&a.icon), "no icon {} for {}", a.icon, a.id);
        }
        for used in c.used_actions() {
            assert!(c.action(used).is_some(), "{used} is used but not defined");
        }
        // Every permanent tab starts with the Select panel (UI-RIBCAT-002); Modify is last.
        for t in &c.tabs {
            assert_eq!(t.panels[0].label, "Select", "{}", t.id);
        }
        assert_eq!(c.tabs.last().map(|t| t.id.as_str()), Some("modify"));
    }
}
