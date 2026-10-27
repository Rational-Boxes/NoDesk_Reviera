// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The radial menu (UI-SEL-007, experimental): with the setting on, a middle click opens a ring
//! of at most eight wedges round the pointer instead of the popup quick menu.
//!
//! - On an element the wedges are the quick modify menu's items (UI-SEL-006), grouped into
//!   tiers; on the background they mirror the ribbon (tabs, then panels, then tools) with a
//!   **Menu** wedge for the menu bar.
//! - Touching a wedge that opens a tier re-centres the ring on the pointer and shows that tier;
//!   moving back into the centre (after leaving it) returns to the tier above. Touching a leaf
//!   highlights it, and a left click runs it. A left click elsewhere, a right click or Esc
//!   dismisses the ring. A tier of more than eight shows seven and **More…**.
//! - Arrow keys move between wedges, Enter enters or runs, Backspace goes back a tier.
//! - The ring is drawn by the renderer over the view (`reviera_render::radial`); the pointer is
//!   never moved, and near the edges the ring is shifted to fit.

use std::path::PathBuf;

use reviera_render::radial::{self, Touch, Wedge};

use crate::ribbon::{Catalog, PanelDef};
use crate::select::{QuickGroup, QuickMenu};

/// Most wedges in a tier.
pub(crate) const MAX: usize = 8;

/// An entry of the ring: a command (a wx id run as a menu item) or a further tier.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Leaf { label: String, id: i32, enabled: bool },
    Tier { label: String, children: Vec<Node> },
}

impl Node {
    fn label(&self) -> &str {
        match self {
            Node::Leaf { label, .. } | Node::Tier { label, .. } => label,
        }
    }

    /// As drawn: a tier with nothing that can run is dimmed.
    fn wedge(&self) -> Wedge {
        Wedge {
            label: self.label().to_owned(),
            submenu: matches!(self, Node::Tier { .. }),
            enabled: self.enabled(),
        }
    }

    fn enabled(&self) -> bool {
        match self {
            Node::Leaf { enabled, .. } => *enabled,
            Node::Tier { children, .. } => children.iter().any(Node::enabled),
        }
    }
}

/// `nodes` as one tier: past [`MAX`], the first seven and a **More…** wedge at the bottom with
/// the rest (paged the same way).
pub(crate) fn paged(mut nodes: Vec<Node>) -> Vec<Node> {
    if nodes.len() <= MAX {
        return nodes;
    }
    let rest = nodes.split_off(MAX - 1);
    let more = Node::Tier {
        label: "More…".into(),
        children: paged(rest),
    };
    // The bottom of eight is the fifth wedge.
    nodes.insert(MAX / 2, more);
    nodes
}

/// A tier node with its children paged; `None` when it has none.
fn tier(label: &str, children: Vec<Node>) -> Option<Node> {
    (!children.is_empty()).then(|| Node::Tier {
        label: label.to_owned(),
        children: paged(children),
    })
}

/// A ribbon panel's tools as leaves.
fn panel_tools(
    catalog: &Catalog,
    panel: &PanelDef,
    enabled: &dyn Fn(&str) -> bool,
) -> Vec<Node> {
    panel
        .tools
        .iter()
        .filter_map(|t| {
            let a = catalog.action(&t.action)?;
            Some(Node::Leaf {
                label: a.label.clone(),
                id: catalog.id_of(&t.action)?,
                enabled: enabled(&t.action),
            })
        })
        .collect()
}

/// A tab's panels as tiers.
fn tab_panels(catalog: &Catalog, panels: &[PanelDef], enabled: &dyn Fn(&str) -> bool) -> Vec<Node> {
    panels
        .iter()
        .filter_map(|p| tier(&p.label, panel_tools(catalog, p, enabled)))
        .collect()
}

/// The background tiers: a **Menu** wedge with the menu bar's menus (first, at the top), the
/// contextual tab when shown, and every ribbon tab. `reset` adds Reset Hide/Isolate first.
pub(crate) fn background(
    catalog: &Catalog,
    context: Option<(&str, &[PanelDef])>,
    enabled: &dyn Fn(&str) -> bool,
    reset: Option<i32>,
) -> Vec<Node> {
    let mut nodes = Vec::new();
    if let Some(id) = reset {
        nodes.push(Node::Leaf {
            label: "Reset Hide/Isolate".into(),
            id,
            enabled: true,
        });
    }
    let menus: Vec<Node> = catalog
        .menus
        .iter()
        .filter_map(|m| {
            let items = m
                .items
                .iter()
                .filter(|i| *i != "-" && !i.starts_with('@'))
                .filter_map(|i| {
                    Some(Node::Leaf {
                        label: catalog.action(i)?.label.clone(),
                        id: catalog.id_of(i)?,
                        enabled: enabled(i),
                    })
                })
                .collect();
            tier(&radial::plain(&m.label), items)
        })
        .collect();
    // The menu bar first, at the top of the ring.
    nodes.extend(tier("Menu", menus));
    if let Some((label, panels)) = context {
        nodes.extend(tier(label, tab_panels(catalog, panels, enabled)));
    }
    for tab in &catalog.tabs {
        nodes.extend(tier(&tab.label, tab_panels(catalog, &tab.panels, enabled)));
    }
    paged(nodes)
}

/// The element tiers from the quick menu's items: Transform and Visibility as tiers, the
/// element's own items (Create Component, Pin, Delete) as wedges, and the overlap booleans as a
/// tier named by the overlap.
pub(crate) fn element(q: &QuickMenu) -> Vec<Node> {
    let leaf = |i: &crate::select::QuickItem| Node::Leaf {
        label: radial::plain(&i.label),
        id: i.id,
        enabled: i.enabled,
    };
    let of = |g: QuickGroup| q.items.iter().filter(move |i| i.group == g);
    let mut nodes = Vec::new();
    nodes.extend(tier("Transform", of(QuickGroup::Transform).map(leaf).collect()));
    nodes.extend(tier("Visibility", of(QuickGroup::Visibility).map(leaf).collect()));
    nodes.extend(of(QuickGroup::Element).map(leaf));
    let overlap: Vec<_> = of(QuickGroup::Overlap).collect();
    match overlap.split_first() {
        // The heading names the tier ("Overlap of 0.36 m³:").
        Some((head, rest)) if !rest.is_empty() => {
            let label = radial::plain(head.label.trim_end_matches(':'));
            nodes.extend(tier(&label, rest.iter().map(|i| leaf(i)).collect()));
        }
        Some((none, _)) => nodes.push(leaf(none)),
        None => {}
    }
    paged(nodes)
}

/// The open ring of one view.
#[derive(Debug, Clone)]
pub(crate) struct RadialMenu {
    pub view: u32,
    pub root: Vec<Node>,
    pub title: String,
    /// The wedges entered from the root, by index.
    pub path: Vec<usize>,
    /// Where the ring is drawn, logical pixels.
    pub centre: [f32; 2],
    pub hot: Option<usize>,
    /// The pointer has left the centre since the ring last moved (so returning to it goes back).
    pub armed: bool,
    pub large: bool,
}

/// What a step of the ring does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Nothing for the window to do beyond redrawing.
    Redraw,
    Nothing,
    /// Run this wx id, and close.
    Run(i32),
    Close,
}

impl RadialMenu {
    pub(crate) fn new(view: u32, title: String, root: Vec<Node>, large: bool) -> Self {
        Self {
            view,
            root,
            title,
            path: Vec::new(),
            centre: [0.0; 2],
            hot: None,
            armed: false,
            large,
        }
    }

    /// The tier shown now.
    pub(crate) fn tier(&self) -> &[Node] {
        let mut nodes = &self.root[..];
        for k in &self.path {
            match nodes.get(*k) {
                Some(Node::Tier { children, .. }) => nodes = children,
                _ => break,
            }
        }
        nodes
    }

    /// The path of tiers entered, as "Architecture › Build".
    pub(crate) fn breadcrumb(&self) -> String {
        let mut nodes = &self.root[..];
        let mut names = Vec::new();
        for k in &self.path {
            if let Some(Node::Tier { label, children }) = nodes.get(*k) {
                names.push(label.clone());
                nodes = children;
            }
        }
        names.join(" › ")
    }

    fn wedges(&self) -> Vec<Wedge> {
        self.tier().iter().map(Node::wedge).collect()
    }

    /// Places the ring for a pointer at `p` in a `w` × `h` view.
    pub(crate) fn place(&mut self, p: [f32; 2], w: f32, h: f32) {
        self.centre = radial::fit(p, &self.wedges(), self.large, w, h);
        // Shifted at an edge, the pointer may already be outside the centre.
        self.armed = radial::touch(self.centre, p, 1, None, self.large) != Touch::Centre;
    }

    /// The ring as drawn.
    pub(crate) fn drawn(&self, dark: bool) -> radial::Ring {
        radial::Ring {
            centre: self.centre,
            wedges: self.wedges(),
            hot: self.hot,
            title: self.title.clone(),
            breadcrumb: self.breadcrumb(),
            back: !self.path.is_empty(),
            dark,
            large: self.large,
        }
    }

    /// The pointer moved to `p` in a `w` × `h` view.
    pub(crate) fn motion(&mut self, p: [f32; 2], w: f32, h: f32) -> Step {
        let n = self.tier().len();
        match radial::touch(self.centre, p, n, self.hot, self.large) {
            Touch::Centre => {
                if self.armed && !self.path.is_empty() {
                    self.path.pop();
                    self.hot = None;
                    self.place(p, w, h);
                    self.armed = false;
                    return Step::Redraw;
                }
                let changed = self.hot.take().is_some();
                if changed { Step::Redraw } else { Step::Nothing }
            }
            Touch::Wedge { k, reached } => {
                self.armed = true;
                let changed = self.hot != Some(k);
                self.hot = Some(k);
                if reached
                    && let Some(node @ Node::Tier { .. }) = self.tier().get(k)
                    && node.enabled()
                {
                    self.enter(k, p, w, h);
                    return Step::Redraw;
                }
                if changed { Step::Redraw } else { Step::Nothing }
            }
        }
    }

    fn enter(&mut self, k: usize, p: [f32; 2], w: f32, h: f32) {
        self.path.push(k);
        self.hot = None;
        self.place(p, w, h);
        self.armed = false;
    }

    /// A left click at `p`: runs the highlighted leaf under the pointer, enters a highlighted
    /// tier, or closes.
    pub(crate) fn click(&mut self, p: [f32; 2], w: f32, h: f32) -> Step {
        let n = self.tier().len();
        let Touch::Wedge { k, .. } = radial::touch(self.centre, p, n, self.hot, self.large) else {
            return Step::Close;
        };
        if self.hot != Some(k) {
            return Step::Close;
        }
        self.activate(k, p, w, h)
    }

    fn activate(&mut self, k: usize, p: [f32; 2], w: f32, h: f32) -> Step {
        match self.tier().get(k) {
            Some(Node::Leaf { id, enabled: true, .. }) => Step::Run(*id),
            Some(node @ Node::Tier { .. }) if node.enabled() => {
                self.enter(k, p, w, h);
                Step::Redraw
            }
            _ => Step::Nothing,
        }
    }

    /// A key while the ring is open.
    pub(crate) fn key(&mut self, key: RingKey, w: f32, h: f32) -> Step {
        let n = self.tier().len();
        if n == 0 {
            return Step::Close;
        }
        match key {
            RingKey::Next | RingKey::Previous => {
                let next = key == RingKey::Next;
                let step = if next { 1 } else { n - 1 };
                let first = if next { 0 } else { n - 1 };
                self.hot = Some(self.hot.map_or(first, |k| (k + step) % n));
                Step::Redraw
            }
            RingKey::Enter => match self.hot {
                // A tier entered by key opens where the ring is.
                Some(k) => self.activate(k, self.centre, w, h),
                None => Step::Nothing,
            },
            RingKey::Back => {
                if self.path.pop().is_some() {
                    self.hot = None;
                    Step::Redraw
                } else {
                    Step::Close
                }
            }
            RingKey::Escape => Step::Close,
        }
    }

    /// The tiers as `ui.radial.tree` describes them: label, kind, enabled and children.
    #[cfg(test)]
    pub(crate) fn tree(nodes: &[Node]) -> serde_json::Value {
        serde_json::Value::Array(
            nodes
                .iter()
                .map(|n| match n {
                    Node::Leaf { label, enabled, .. } => serde_json::json!({
                        "label": label, "kind": "command", "enabled": enabled,
                    }),
                    Node::Tier { label, children } => serde_json::json!({
                        "label": label, "kind": "tier",
                        "children": Self::tree(children),
                    }),
                })
                .collect(),
        )
    }
}

/// Keys the ring takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RingKey {
    Next,
    Previous,
    Enter,
    Back,
    Escape,
}

// ---- In the window ----

impl crate::Gui {
    /// A middle click at (`x`, `y`) with the setting on: opens the ring there. False when there is
    /// nothing to show (no model, a tool running, a sheet).
    pub(crate) fn open_radial(&mut self, x: i32, y: i32, panel: &wxdragon::prelude::Panel, add: bool) -> bool {
        use wxdragon::prelude::WxWidget;
        let Some(view) = self.active else {
            return false;
        };
        if self.doc.is_none() || self.any_tool() || self.in_sheet() {
            return false;
        }
        let (title, root) = match self.quick_items(x, y, panel, add) {
            Some(q) if !q.title.is_empty() => (q.title.clone(), element(&q)),
            other => {
                let reset = other.and_then(|q| q.items.first().map(|i| i.id));
                let r = &self.ribbon;
                let root = background(r.catalog(), r.context_shown(), &|a| r.enabled(a), reset);
                (String::new(), root)
            }
        };
        if root.is_empty() {
            return false;
        }
        let mut ring = RadialMenu::new(view, title, root, false);
        let size = panel.get_client_size();
        ring.place([x as f32, y as f32], size.width as f32, size.height as f32);
        self.radial = Some(ring);
        true
    }

    /// The pointer moved with the ring open; true when it needs drawing again.
    pub(crate) fn radial_motion(&mut self, x: i32, y: i32, panel: &wxdragon::prelude::Panel) -> bool {
        use wxdragon::prelude::WxWidget;
        let size = panel.get_client_size();
        let Some(r) = &mut self.radial else {
            return false;
        };
        r.motion([x as f32, y as f32], size.width as f32, size.height as f32) != Step::Nothing
    }

    /// A left click with the ring open: the id to run, if any; the ring closes unless the click
    /// entered a tier.
    pub(crate) fn radial_click(&mut self, x: i32, y: i32, panel: &wxdragon::prelude::Panel) -> Option<i32> {
        use wxdragon::prelude::WxWidget;
        let size = panel.get_client_size();
        let r = self.radial.as_mut()?;
        match r.click([x as f32, y as f32], size.width as f32, size.height as f32) {
            Step::Run(id) => {
                self.radial = None;
                Some(id)
            }
            Step::Close => {
                self.radial = None;
                None
            }
            Step::Redraw | Step::Nothing => None,
        }
    }

    /// A key with the ring open: whether the ring took it, and an id to run.
    pub(crate) fn radial_key(&mut self, code: i32, panel: &wxdragon::prelude::Panel) -> (bool, Option<i32>) {
        use wxdragon::prelude::*;
        let key = match code {
            WXK_RIGHT | WXK_DOWN => RingKey::Next,
            WXK_LEFT | WXK_UP => RingKey::Previous,
            WXK_RETURN | WXK_NUMPAD_ENTER => RingKey::Enter,
            WXK_BACK => RingKey::Back,
            WXK_ESCAPE => RingKey::Escape,
            _ => return (false, None),
        };
        let size = panel.get_client_size();
        let Some(r) = &mut self.radial else {
            return (false, None);
        };
        match r.key(key, size.width as f32, size.height as f32) {
            Step::Run(id) => {
                self.radial = None;
                (true, Some(id))
            }
            Step::Close => {
                self.radial = None;
                (true, None)
            }
            _ => (true, None),
        }
    }

    /// Draws the HUD over view `index`: a 3D view's floating annotation labels (DOC-ANN-025), and
    /// the ring over them when it is open there.
    pub(crate) fn update_hud(&mut self, index: usize, scale: f64) {
        use reviera_render::RenderBackend;
        use wxdragon::prelude::WxWidget;
        let dark = crate::ribbon::dark();
        let tips = self.view_tips(index);
        let tips = self.unhidden_tips(index, tips);
        let v = &mut self.views[index];
        let ring = self.radial.as_ref().filter(|r| r.view == v.id).map(|r| r.drawn(dark));
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let labels = (!tips.is_empty()).then(|| reviera_render::tooltips::layout(&tips, &v.camera, w, h, 1.0, dark));
        let Some(viewport) = &mut v.viewport else {
            return;
        };
        let hud = match (ring, labels) {
            (None, None) => None,
            (ring, labels) => {
                let mut g = match ring {
                    Some(ring) => radial::gizmo(&ring, w, h),
                    None => reviera_render::Gizmo { rect: [0.0, 0.0, w.max(1.0), h.max(1.0)], view_proj: glam::Mat4::IDENTITY, eye: glam::Vec3::Z, meshes: Vec::new(), flat: Vec::new() },
                };
                // The labels under the ring.
                if let Some(mut l) = labels {
                    l.append(&mut g.flat);
                    g.flat = l;
                }
                g.rect = g.rect.map(|c| c * scale as f32);
                Some(g)
            }
        };
        viewport.set_hud(hud);
    }

    /// The floating annotation labels a 3D view shows now, and whether it leaves out those behind
    /// the model; made again when the model changes.
    fn view_tips(&mut self, index: usize) -> (bool, Vec<reviera_render::tooltips::Tip>) {
        let v = &self.views[index];
        if v.is_2d() {
            return (false, Vec::new());
        }
        let (id, doc, element) = (v.id, v.doc, v.element);
        let Some(d) = self.app.document(doc) else { return (false, Vec::new()) };
        let version = d.doc.version();
        if let Some((at, occlude, tips)) = self.tips.get(&id)
            && *at == version
        {
            return (*occlude, tips.clone());
        }
        let store = d.doc.store();
        // The unsaved {3D} view shows the defaults.
        let settings = element.and_then(|e| store.get(e)).map(reviera_core::annotations_3d::settings).unwrap_or_default();
        let tips: Vec<reviera_render::tooltips::Tip> = if settings.shown {
            reviera_core::annotations_3d::tips(store).into_iter().filter(|t| settings.shows(&t.class)).collect()
        } else {
            Vec::new()
        }
        .into_iter()
        .map(|t| reviera_render::tooltips::Tip { anchor: glam::Vec3::new(t.anchor[0] as f32, t.anchor[1] as f32, t.anchor[2] as f32), lines: t.lines })
        .collect();
        self.tips.insert(id, (version, settings.occlude, tips.clone()));
        (settings.occlude, tips)
    }

    /// The labels whose points the view's model does not hide from the eye (DOC-ANN-025): a ray
    /// from the eye (or, in a parallel view, from far back along the view) meeting what one cannot
    /// see through more than 0.4 m before a point hides it; what the section box and slices cut
    /// away and what is hidden in the view do not.
    fn unhidden_tips(&mut self, index: usize, (occlude, tips): (bool, Vec<reviera_render::tooltips::Tip>)) -> Vec<reviera_render::tooltips::Tip> {
        if !occlude || tips.is_empty() {
            return tips;
        }
        let doc = self.views[index].doc;
        let Some(version) = self.app.document(doc).map(|d| d.doc.version()) else { return tips };
        let bvh = match self.occluders.get(&doc) {
            Some((at, b)) if *at == version => b.clone(),
            _ => {
                let Some(scene) = self.scenes.get(&doc) else { return tips };
                let b = std::rc::Rc::new(reviera_render::bvh::Bvh::build_opaque(scene));
                self.occluders.insert(doc, (version, b.clone()));
                b
            }
        };
        let v = &self.views[index];
        let (section_box, slices) = self.view_clip(v);
        let clipping = reviera_render::Clipping::of(section_box.as_ref(), &slices);
        let visibility = self.view_visibility(v);
        let Some(scene) = self.scenes.get(&doc) else { return tips };
        let camera = &v.camera;
        let back = camera.direction();
        // A label stands for a room, an opening or a note's place: it shows while any of three
        // points up from its anchor (at it, 1.2 m and 2.4 m above) can be seen.
        let seen = |p: glam::Vec3| {
            let origin = if camera.perspective { camera.eye() } else { p + back * (camera.distance + 1000.0) };
            let to = p - origin;
            let length = to.length();
            if length < 0.5 {
                return true;
            }
            let before = 1.0 - 0.4 / length;
            let accept = |mesh: usize, q: glam::Vec3| clipping.keeps(q) && scene.meshes.get(mesh).is_some_and(|m| visibility.opacity(m.element).is_some());
            bvh.nearest(origin, to, before, &accept).is_none()
        };
        tips.into_iter()
            .filter(|t| [0.0, 1.2, 2.4].iter().any(|h| seen(t.anchor + glam::Vec3::Z * *h)))
            .collect()
    }
}

// ---- The experimental setting ----

/// Per-user interface settings kept in `ui-settings.json` beside the other settings.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct UiSettings {
    /// The radial menu on middle click (experimental, off by default).
    pub radial_menu: bool,
    /// The edge-reveal dwell in full-screen and focus mode, milliseconds (150 when 0).
    pub reveal_dwell_ms: u64,
}

fn settings_path() -> Option<PathBuf> {
    crate::components::config_dir().map(|d| d.join("ui-settings.json"))
}

impl UiSettings {
    pub(crate) fn load() -> Self {
        settings_path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub(crate) fn save(&self) {
        if let Some(p) = settings_path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, serde_json::to_vec_pretty(self).expect("serialises"));
        }
    }

    pub(crate) fn dwell(&self) -> std::time::Duration {
        std::time::Duration::from_millis(if self.reveal_dwell_ms == 0 {
            150
        } else {
            self.reveal_dwell_ms
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaves(n: usize) -> Vec<Node> {
        (0..n)
            .map(|k| Node::Leaf {
                label: format!("Tool {k}"),
                id: k as i32,
                enabled: true,
            })
            .collect()
    }

    #[test]
    fn eleven_items_page_into_seven_and_more() {
        let t = paged(leaves(11));
        assert_eq!(t.len(), 8);
        let Node::Tier { label, children } = &t[4] else {
            panic!("More… at the bottom")
        };
        assert_eq!(label, "More…");
        assert_eq!(children.len(), 4);
        assert_eq!(t[5].label(), "Tool 4");
        // Twenty: 7, then 7 more and a further More… with the last 6.
        let t = paged(leaves(20));
        let Node::Tier { children, .. } = &t[4] else { panic!() };
        assert_eq!(children.len(), 8);
        let Node::Tier { children, .. } = &children[4] else { panic!() };
        assert_eq!(children.len(), 6);
    }

    #[test]
    fn the_background_mirrors_the_ribbon_and_menus() {
        let catalog = Catalog::parse(crate::ribbon::DEFAULT).unwrap();
        let root = background(&catalog, None, &|_| true, None);
        fn check(nodes: &[Node]) {
            assert!(nodes.len() <= MAX);
            for n in nodes {
                if let Node::Tier { children, .. } = n {
                    check(children);
                }
            }
        }
        check(&root);
        // Every tab is reachable, with More… where there are more than eight.
        let mut names = Vec::new();
        fn top(nodes: &[Node], names: &mut Vec<String>) {
            for n in nodes {
                match n {
                    Node::Tier { label, children } if label == "More…" => top(children, names),
                    n => names.push(n.label().to_owned()),
                }
            }
        }
        top(&root, &mut names);
        for tab in &catalog.tabs {
            assert!(names.contains(&tab.label), "{} in {names:?}", tab.label);
        }
        assert!(names.contains(&"Menu".to_owned()));
        let tree = RadialMenu::tree(&root);
        assert!(tree.as_array().unwrap().len() <= MAX);
        assert_eq!(tree[0]["label"], "Menu");
        assert!(tree.to_string().contains("Room & Area"), "ribbon labels keep their ampersands");
    }

    fn ring(root: Vec<Node>) -> RadialMenu {
        let mut r = RadialMenu::new(1, "Title".into(), root, false);
        r.place([400.0, 400.0], 800.0, 800.0);
        r
    }

    #[test]
    fn touching_a_tier_recentres_and_the_centre_returns() {
        let root = vec![
            Node::Tier { label: "Architecture".into(), children: vec![
                Node::Tier { label: "Build".into(), children: leaves(3) },
            ] },
            Node::Leaf { label: "Other".into(), id: 99, enabled: true },
        ];
        let mut r = ring(root);
        assert!(!r.armed);
        // Up to the first wedge's label: it opens, centred on the pointer.
        assert_eq!(r.motion([400.0, 330.0], 800.0, 800.0), Step::Redraw);
        assert_eq!(r.hot, Some(0));
        assert_eq!(r.motion([400.0, 300.0], 800.0, 800.0), Step::Redraw);
        assert_eq!(r.path, vec![0]);
        assert_eq!(r.centre, [400.0, 300.0]);
        assert_eq!(r.breadcrumb(), "Architecture");
        // Still in the new centre: nothing goes back until the pointer has left it.
        assert_eq!(r.motion([402.0, 300.0], 800.0, 800.0), Step::Nothing);
        assert_eq!(r.path, vec![0]);
        // Into Build: a tier of three leaves.
        r.motion([400.0, 200.0], 800.0, 800.0);
        assert_eq!(r.breadcrumb(), "Architecture › Build");
        assert_eq!(r.tier().len(), 3);
        // Out (highlighting a leaf) and back to the centre: up a tier.
        r.motion([460.0, 200.0], 800.0, 800.0);
        assert!(r.hot.is_some());
        assert_eq!(r.motion([400.0, 200.0], 800.0, 800.0), Step::Redraw);
        assert_eq!(r.breadcrumb(), "Architecture");
    }

    #[test]
    fn a_leaf_highlights_on_touch_and_runs_on_click() {
        let mut r = ring(leaves(4));
        // Touching wedge 1 (right) only highlights it, even at the label.
        assert_eq!(r.motion([500.0, 400.0], 800.0, 800.0), Step::Redraw);
        assert_eq!(r.hot, Some(1));
        assert!(r.path.is_empty());
        assert_eq!(r.click([500.0, 400.0], 800.0, 800.0), Step::Run(1));
        // A click in the centre closes.
        assert_eq!(r.click([400.0, 400.0], 800.0, 800.0), Step::Close);
        // A disabled leaf does not run.
        let mut r = ring(vec![Node::Leaf { label: "No".into(), id: 5, enabled: false }]);
        r.motion([400.0, 330.0], 800.0, 800.0);
        assert_eq!(r.click([400.0, 330.0], 800.0, 800.0), Step::Nothing);
    }

    #[test]
    fn keys_move_enter_and_go_back() {
        let root = vec![
            Node::Tier { label: "A".into(), children: leaves(2) },
            Node::Leaf { label: "B".into(), id: 7, enabled: true },
        ];
        let mut r = ring(root);
        r.key(RingKey::Next, 800.0, 800.0);
        assert_eq!(r.hot, Some(0));
        r.key(RingKey::Enter, 800.0, 800.0);
        assert_eq!(r.path, vec![0]);
        assert_eq!(r.key(RingKey::Back, 800.0, 800.0), Step::Redraw);
        r.key(RingKey::Previous, 800.0, 800.0);
        assert_eq!(r.hot, Some(1));
        assert_eq!(r.key(RingKey::Enter, 800.0, 800.0), Step::Run(7));
        assert_eq!(r.key(RingKey::Back, 800.0, 800.0), Step::Close);
    }
}
