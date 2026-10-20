// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Components in the window (DM-LIB-002 … 004, DM-IMP-015): the Component Library panel, the
//! Place Component tool and Import Object.
//!
//! - **Component Library** (docked on the right): the libraries the user has open (a folder with
//!   a `reviera-library.json`; the list is kept per user), the selected library's folders as a
//!   tree, and the selected folder's components as thumbnails. *Open…* opens a folder as a
//!   library (offering to make one when it has no manifest), *New…* makes one, *Import…* copies
//!   3D files into the folder shown. Clicking a component picks it up: the Place Component tool
//!   starts with it, like a stamp in a paint program.
//! - **Place Component** (Architecture › Component, CM): its outline follows the cursor in a plan
//!   (snapping), or the surface under it in a 3D view; each click places a copy until Esc;
//!   Spacebar turns the next copy 90°. A library component is loaded into the project as a type
//!   on its first copy and reused after. Properties shows the component's details (kept in the
//!   library's manifest, not in its file) and the placement's rotation, mirror and offset.
//! - **Import Object** (Architecture › Component › Import Object): a mesh file read with chosen
//!   units, up axis, insertion point and category into a custom object type; the Place
//!   Component tool then starts with it.
//!
//! Thumbnails are rendered offscreen and cached per user, keyed by the file, its date, size and
//! import options.

use std::path::{Path, PathBuf};

use reviera_core::library::{self, ItemInfo, ItemKind};
use reviera_core::mesh_objects::{self, Category, ImportOptions, Insertion, Units, UpAxis};
use reviera_core::{ElementId, Store};
use reviera_render::plan::Lines;
use reviera_render::{Camera, RenderBackend, RenderScene, SceneMesh};
use reviera_render_wgpu::WgpuViewport;
use serde_json::{Value, json};
use wxdragon::prelude::*;
use wxdragon::widgets::list_ctrl::{ListCtrlEventData, image_list_type};

use super::{Gui, error, with};

const PREVIEW: [f32; 4] = [0.05, 0.35, 0.95, 1.0];
/// Thumbnail size, pixels.
pub(crate) const THUMB: u32 = 96;

/// Where per-user settings live: `%APPDATA%\Reviera`, else `$XDG_CONFIG_HOME/reviera` (or
/// `~/.config/reviera`).
pub(crate) fn config_dir() -> Option<PathBuf> {
    if let Some(a) = std::env::var_os("APPDATA") {
        return Some(Path::new(&a).join("Reviera"));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))?;
    Some(base.join("reviera"))
}

/// Where thumbnails are cached: `%LOCALAPPDATA%\Reviera\thumbnails`, else
/// `$XDG_CACHE_HOME/reviera/thumbnails` (or `~/.cache/reviera/thumbnails`).
fn thumb_dir() -> Option<PathBuf> {
    if let Some(a) = std::env::var_os("LOCALAPPDATA") {
        return Some(Path::new(&a).join("Reviera").join("thumbnails"));
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cache")))?;
    Some(base.join("reviera").join("thumbnails"))
}

/// The sample library shipped beside the samples (`test-data/library` in the source tree).
fn sample_library() -> Option<PathBuf> {
    let samples = super::help::samples_dir()?;
    let lib = samples.join("../library");
    lib.join(library::MANIFEST)
        .is_file()
        .then(|| std::fs::canonicalize(&lib).unwrap_or(lib))
}

/// The libraries the user has open, kept in `libraries.json`; the sample library the first time.
fn load_libraries() -> Vec<PathBuf> {
    let saved: Option<Vec<PathBuf>> = config_dir()
        .and_then(|d| std::fs::read(d.join("libraries.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok());
    match saved {
        Some(list) => list
            .into_iter()
            .filter(|p| p.join(library::MANIFEST).is_file())
            .collect(),
        None => sample_library().into_iter().collect(),
    }
}

fn save_libraries(list: &[PathBuf]) {
    if let Some(d) = config_dir() {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(
            d.join("libraries.json"),
            serde_json::to_vec_pretty(list).expect("serialises"),
        );
    }
}

/// The Component Library panel and what it shows.
pub(crate) struct LibraryPane {
    choice: Choice,
    tree: TreeCtrl,
    list: ListCtrl,
    info: StaticText,
    /// Open libraries: folder and name.
    libraries: Vec<(PathBuf, String)>,
    current: Option<usize>,
    /// The folder shown (relative to the library).
    folder: String,
    items: Vec<ItemInfo>,
}

/// What the Place Component tool places.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Source {
    /// A library component (loaded as a type on its first copy).
    Item { library: PathBuf, item: String },
    /// A custom object type in the project.
    Type(ElementId),
    /// A component that is a model fragment (an IFC file in a library).
    Fragment { library: PathBuf, item: String },
}

/// The Place Component tool.
#[derive(Debug, Clone)]
pub(crate) struct PlaceTool {
    pub source: Source,
    pub name: String,
    rotation: f64,
    mirror: bool,
    offset: f64,
    /// The component's outline in plan, in its own coordinates (metres).
    outline: Vec<[f64; 2]>,
    scale: f64,
}

/// A convex hull (counter-clockwise) of points in plan.
fn hull(mut pts: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut lower: Vec<[f64; 2]> = Vec::new();
    for p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0.0 {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<[f64; 2]> = Vec::new();
    for p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0.0 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// A library item's mesh, read with its manifest's (or folder's) import options.
fn item_mesh(lib: &Path, item: &str) -> Result<mesh_objects::Prepared, String> {
    let meta = library::get_meta(
        &Store::default(),
        library::GetMetaParams {
            library: lib.to_string_lossy().into_owned(),
            item: item.to_owned(),
        },
    )
    .map_err(|e| e.message)?;
    let options = meta
        .details
        .import
        .or(meta.folder.import)
        .unwrap_or_default();
    let raw = reviera_core::mesh_io::read(&lib.join(item)).map_err(|e| e.message)?;
    mesh_objects::prepare(raw, &options).map_err(|e| e.message)
}

/// A fragment's plan outline: the hull of its bodies (its base point at the origin).
fn fragment_outline(path: &Path) -> Result<Vec<[f64; 2]>, String> {
    let (_p, doc, _) = reviera_store::LocalProject::open(path).map_err(|e| e.to_string())?;
    let bodies = reviera_core::bodies::all(doc.store());
    let pts = bodies
        .iter()
        .flat_map(|b| b.parts.iter().flat_map(|p| p.mesh.0.iter().copied()))
        .collect::<Vec<_>>();
    Ok(outline_of(pts.into_iter()))
}

/// A fragment's bodies as one mesh with a slot per IFC class, coloured as the 3D view does.
fn fragment_mesh(path: &Path) -> Result<mesh_objects::Prepared, String> {
    let (_p, doc, _) = reviera_store::LocalProject::open(path).map_err(|e| e.to_string())?;
    let bodies = reviera_core::bodies::all(doc.store());
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    let mut tri_slot = Vec::new();
    let mut slots: Vec<mesh_objects::Slot> = Vec::new();
    for part in bodies.iter().flat_map(|b| b.parts.iter()) {
        let slot = match slots.iter().position(|s| s.name == part.class) {
            Some(k) => k,
            None => {
                slots.push(mesh_objects::Slot {
                    name: part.class.clone(),
                    color: reviera_render::class_color(&part.class),
                });
                slots.len() - 1
            }
        };
        let base = positions.len() as u32;
        positions.extend(part.mesh.0.iter().copied());
        for t in part.mesh.1.as_chunks::<3>().0 {
            triangles.push([base + t[0], base + t[1], base + t[2]]);
            tri_slot.push(slot as u32);
        }
    }
    if triangles.is_empty() {
        return Err("the component has no geometry Reviera draws".into());
    }
    let report = mesh_objects::Report {
        format: "IFC".into(),
        triangles: triangles.len(),
        vertices: positions.len(),
        materials: Vec::new(),
        size: [0.0; 3],
        units: 1.0,
        units_from_file: true,
        y_up: false,
        open_edges: 0,
        non_manifold_edges: 0,
        solid: false,
        dropped_triangles: 0,
        welded_vertices: 0,
    };
    Ok(mesh_objects::Prepared {
        positions,
        triangles,
        tri_slot,
        slots,
        report,
    })
}

/// A mesh's plan outline (a hull of a sample of its points).
fn outline_of(points: impl Iterator<Item = [f64; 3]>) -> Vec<[f64; 2]> {
    let all: Vec<[f64; 2]> = points.map(|p| [p[0], p[1]]).collect();
    let step = (all.len() / 4000).max(1);
    hull(all.into_iter().step_by(step).collect())
}

impl Gui {
    // ---- The panel ----

    /// Builds the Component Library panel (docked right) and wires its events.
    pub(crate) fn build_library_pane(gui: &std::rc::Rc<std::cell::RefCell<Gui>>) {
        let (frame, aui) = {
            let g = gui.borrow();
            (g.frame, g.aui)
        };
        let panel = Panel::builder(&frame).build();
        let choice = Choice::builder(&panel).build();
        let open = Button::builder(&panel).with_label("Open…").build();
        let new = Button::builder(&panel).with_label("New…").build();
        let import = Button::builder(&panel).with_label("Import…").build();
        open.set_tooltip("Open a folder as a component library");
        new.set_tooltip("Make a new component library in a folder");
        import.set_tooltip("Copy 3D files (OBJ, STL, PLY, glTF, GLB, IFC) into the folder shown");
        let tree = TreeCtrl::builder(&panel)
            .with_style(
                TreeCtrlStyle::Default | TreeCtrlStyle::HasButtons | TreeCtrlStyle::LinesAtRoot,
            )
            .build();
        let list = ListCtrl::builder(&panel)
            .with_style(ListCtrlStyle::Icon | ListCtrlStyle::SingleSel | ListCtrlStyle::AutoArrange)
            .build();
        let info = StaticText::builder(&panel)
            .with_label("Click a component to place it; Esc ends.")
            .build();
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(
            &choice,
            1,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            4,
        );
        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        buttons.add(&open, 1, SizerFlag::Right, 2);
        buttons.add(&new, 1, SizerFlag::Right, 2);
        buttons.add(&import, 1, SizerFlag::empty(), 0);
        let sizer = BoxSizer::builder(Orientation::Vertical).build();
        sizer.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::All, 4);
        sizer.add_sizer(
            &buttons,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            4,
        );
        sizer.add(&tree, 1, SizerFlag::Expand | SizerFlag::All, 4);
        sizer.add(
            &list,
            2,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            4,
        );
        sizer.add(&info, 0, SizerFlag::Expand | SizerFlag::All, 4);
        panel.set_sizer(sizer, true);
        aui.add_pane_with_info(
            &panel,
            AuiPaneInfo::new()
                .with_name("components")
                .with_caption("Component Library")
                .right()
                .layer(1)
                .best_size(300, 600)
                .min_size(220, 200)
                .close_button(true)
                .floatable(true),
        );
        aui.update();
        let libraries = load_libraries()
            .into_iter()
            .filter_map(|p| {
                let m = library::Manifest::read(&p).ok().flatten()?;
                Some((p, m.name))
            })
            .collect();
        gui.borrow_mut().library_pane = Some(LibraryPane {
            choice,
            tree,
            list,
            info,
            libraries,
            current: None,
            folder: String::new(),
            items: Vec::new(),
        });
        {
            let gui = gui.clone();
            choice.on_selection_changed(move |_| {
                with(&gui, |g| {
                    let at = g
                        .library_pane
                        .as_ref()
                        .and_then(|p| p.choice.get_selection());
                    g.show_library(at.map(|i| i as usize));
                });
            });
        }
        {
            let gui = gui.clone();
            tree.on_selection_changed(move |e| {
                let Some(item) = e.get_item() else { return };
                with(&gui, |g| {
                    let folder = g.library_pane.as_ref().and_then(|p| {
                        p.tree
                            .get_custom_data(&item)
                            .and_then(|d| d.downcast_ref::<String>().cloned())
                    });
                    if let Some(folder) = folder {
                        g.show_folder(&folder);
                    }
                });
            });
        }
        for event in [true, false] {
            let gui = gui.clone();
            let handler = move |e: ListCtrlEventData| {
                let i = e.get_item_index();
                with(&gui, |g| g.pick_library_item(i));
            };
            if event {
                list.on_item_selected(handler);
            } else {
                list.on_item_activated(handler);
            }
        }
        {
            let gui = gui.clone();
            open.on_click(move |_| with(&gui, Gui::open_library_folder));
        }
        {
            let gui = gui.clone();
            new.on_click(move |_| with(&gui, Gui::new_library));
        }
        {
            let gui = gui.clone();
            import.on_click(move |_| with(&gui, Gui::import_into_library));
        }
        let mut g = gui.borrow_mut();
        g.fill_library_choice();
        let first = (!g.library_pane.as_ref().expect("built").libraries.is_empty()).then_some(0);
        g.show_library(first);
    }

    fn fill_library_choice(&mut self) {
        let Some(p) = &self.library_pane else { return };
        p.choice.clear();
        for (_, name) in &p.libraries {
            p.choice.append(name);
        }
        if let Some(c) = p.current {
            p.choice.set_selection(c as u32);
        }
    }

    /// Shows library `at` (its folder tree and root items).
    fn show_library(&mut self, at: Option<usize>) {
        let Some(p) = &mut self.library_pane else {
            return;
        };
        p.current = at.filter(|i| *i < p.libraries.len());
        p.tree.cleanup_custom_data();
        p.tree.delete_all_items();
        p.list.delete_all_items();
        p.items.clear();
        let Some(i) = p.current else {
            p.info.set_label("No library open: Open… or New…");
            return;
        };
        p.choice.set_selection(i as u32);
        let (root_path, name) = p.libraries[i].clone();
        let Some(root) = p.tree.add_root_with_data(&name, String::new(), None, None) else {
            return;
        };
        // Folders, nested, with their item counts.
        fn add(tree: &TreeCtrl, parent: &TreeItemId, lib: &Path, folder: &str) {
            let Ok(listing) = library::list(
                &Store::default(),
                library::ListParams {
                    library: lib.to_string_lossy().into_owned(),
                    folder: folder.to_owned(),
                    recursive: false,
                },
            ) else {
                return;
            };
            for f in listing.folders {
                let label = format!("{} ({})", f.name, f.items);
                if let Some(node) =
                    tree.append_item_with_data(parent, &label, f.path.clone(), None, None)
                {
                    add(tree, &node, lib, &f.path);
                }
            }
        }
        add(&p.tree, &root, &root_path, "");
        p.tree.expand(&root);
        p.tree.select_item(&root);
        self.show_folder("");
    }

    /// Shows a folder's components, with thumbnails.
    fn show_folder(&mut self, folder: &str) {
        let Some(p) = &mut self.library_pane else {
            return;
        };
        let Some(i) = p.current else { return };
        let lib = p.libraries[i].0.clone();
        p.folder = folder.to_owned();
        let listing = library::list(
            &Store::default(),
            library::ListParams {
                library: lib.to_string_lossy().into_owned(),
                folder: folder.to_owned(),
                recursive: false,
            },
        );
        let items = listing.map(|l| l.items).unwrap_or_default();
        let images = ImageList::new(THUMB as i32, THUMB as i32, false, items.len() as i32);
        let mut thumbs = Vec::new();
        for it in &items {
            thumbs.push(self.thumbnail(&lib, it));
        }
        let Some(p) = &mut self.library_pane else {
            return;
        };
        p.list.delete_all_items();
        let mut index = Vec::new();
        for t in &thumbs {
            index.push(
                t.as_ref()
                    .and_then(|rgba| Bitmap::from_rgba(rgba, THUMB, THUMB))
                    .map(|b| images.add_bitmap(&b)),
            );
        }
        p.list.set_image_list(images, image_list_type::NORMAL);
        for (k, (it, image)) in items.iter().zip(index).enumerate() {
            p.list.insert_item(k as i64, &it.name, image);
        }
        p.info.set_label(&format!(
            "{} component{} in {} — click one to place it",
            items.len(),
            if items.len() == 1 { "" } else { "s" },
            if folder.is_empty() {
                p.libraries[i].1.as_str()
            } else {
                folder
            }
        ));
        p.items = items;
    }

    /// A component's thumbnail (RGBA, `THUMB` square), from the cache or rendered now.
    fn thumbnail(&mut self, lib: &Path, it: &ItemInfo) -> Option<Vec<u8>> {
        let path = lib.join(&it.path);
        let md = std::fs::metadata(&path).ok()?;
        let options = library::get_meta(
            &Store::default(),
            library::GetMetaParams {
                library: lib.to_string_lossy().into_owned(),
                item: it.path.clone(),
            },
        )
        .ok()
        .map(|m| m.details.import.or(m.folder.import).unwrap_or_default());
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            path.hash(&mut h);
            md.len().hash(&mut h);
            md.modified().ok().hash(&mut h);
            serde_json::to_string(&options)
                .unwrap_or_default()
                .hash(&mut h);
            THUMB.hash(&mut h);
            format!("{:016x}.rgba", h.finish())
        };
        let cache = thumb_dir().map(|d| d.join(&key));
        if let Some(c) = &cache
            && let Ok(b) = std::fs::read(c)
            && b.len() == (THUMB * THUMB * 4) as usize
        {
            return Some(b);
        }
        let prepared = match it.kind {
            ItemKind::Object => item_mesh(lib, &it.path).ok()?,
            ItemKind::Fragment => fragment_mesh(&path).ok()?,
        };
        let rgba = self.render_thumbnail(&prepared)?;
        if let Some(c) = &cache {
            let _ = std::fs::create_dir_all(c.parent().expect("a folder"));
            let _ = std::fs::write(c, &rgba);
        }
        Some(rgba)
    }

    /// Renders a mesh into a thumbnail: a three-quarter view, fitted.
    fn render_thumbnail(&mut self, m: &mesh_objects::Prepared) -> Option<Vec<u8>> {
        if self.thumb_renderer.is_none() {
            self.thumb_renderer = WgpuViewport::offscreen(THUMB, THUMB).ok();
        }
        let r = self.thumb_renderer.as_mut()?;
        let mut meshes = Vec::new();
        for (slot, s) in m.slots.iter().enumerate() {
            let mut map = std::collections::HashMap::new();
            let (mut positions, mut indices) = (Vec::new(), Vec::new());
            for (t, ts) in m.triangles.iter().zip(&m.tri_slot) {
                if *ts as usize != slot {
                    continue;
                }
                for i in t {
                    let n = *map.entry(*i).or_insert_with(|| {
                        let p = m.positions[*i as usize];
                        positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
                        positions.len() as u32 - 1
                    });
                    indices.push(n);
                }
            }
            if !indices.is_empty() {
                meshes.push(SceneMesh {
                    element: None,
                    class: "IfcBuildingElementProxy".into(),
                    positions,
                    indices,
                    color: s.color,
                });
            }
        }
        r.set_scene(&RenderScene { meshes });
        let mut lo = glam::Vec3::splat(f32::INFINITY);
        let mut hi = glam::Vec3::splat(f32::NEG_INFINITY);
        for p in &m.positions {
            let v = glam::Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        let mut camera = Camera {
            yaw: (-55f32).to_radians(),
            pitch: 22f32.to_radians(),
            ..Camera::default()
        };
        camera.fit((lo, hi));
        r.render(&camera).ok()?;
        r.read_pixels()
    }

    /// A component clicked in the panel: the Place Component tool picks it up.
    fn pick_library_item(&mut self, index: i32) {
        let Some(p) = &self.library_pane else { return };
        let (Some(i), Some(it)) = (
            p.current,
            usize::try_from(index).ok().and_then(|k| p.items.get(k)),
        ) else {
            return;
        };
        let lib = p.libraries[i].0.clone();
        let it = it.clone();
        if it.kind == ItemKind::Fragment {
            return self.start_place_tool(
                Source::Fragment {
                    library: lib,
                    item: it.path.clone(),
                },
                it.name.clone(),
            );
        }
        self.start_place_tool(
            Source::Item {
                library: lib,
                item: it.path.clone(),
            },
            it.name.clone(),
        );
    }

    /// The panel's Open…: a folder as a library (offering to make one).
    fn open_library_folder(&mut self) {
        let frame = self.frame;
        let d = DirDialog::builder(&frame, "Open Component Library", "").build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(path) = d.get_path() else { return };
        let path = PathBuf::from(path);
        if !path.join(library::MANIFEST).is_file() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ask = MessageDialog::builder(
                &frame,
                &format!(
                    "{} has no {}. Make it a component library named \"{name}\"? Its files are not changed; only the manifest is added.",
                    path.display(),
                    library::MANIFEST
                ),
                "Open Component Library",
            )
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
            .build();
            if ask.show_modal() != ID_YES {
                return;
            }
            if let Err(e) = library::create(
                &Store::default(),
                library::CreateParams {
                    path: path.to_string_lossy().into_owned(),
                    name,
                },
            ) {
                return error(&frame, &e.message);
            }
        }
        self.add_library(path);
    }

    /// The panel's New…: makes a library in a folder.
    fn new_library(&mut self) {
        let frame = self.frame;
        let d = DirDialog::builder(
            &frame,
            "New Component Library: choose or make its folder",
            "",
        )
        .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(path) = d.get_path() else { return };
        let path = PathBuf::from(path);
        let default = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ask = TextEntryDialog::builder(&frame, "Library name:", "New Component Library")
            .with_default_value(&default)
            .build();
        if ask.show_modal() != ID_OK {
            return;
        }
        let name = ask.get_value().unwrap_or(default);
        if let Err(e) = library::create(
            &Store::default(),
            library::CreateParams {
                path: path.to_string_lossy().into_owned(),
                name,
            },
        ) {
            return error(&frame, &e.message);
        }
        self.add_library(path);
    }

    fn add_library(&mut self, path: PathBuf) {
        let Some(p) = &mut self.library_pane else {
            return;
        };
        let Ok(Some(m)) = library::Manifest::read(&path) else {
            return;
        };
        let at = match p.libraries.iter().position(|(l, _)| *l == path) {
            Some(k) => k,
            None => {
                p.libraries.push((path, m.name));
                p.libraries.len() - 1
            }
        };
        save_libraries(
            &p.libraries
                .iter()
                .map(|(l, _)| l.clone())
                .collect::<Vec<_>>(),
        );
        p.current = Some(at);
        self.fill_library_choice();
        self.show_library(Some(at));
    }

    /// The panel's Import…: copies 3D files into the folder shown.
    pub(crate) fn import_into_library(&mut self) {
        let frame = self.frame;
        let Some(p) = &self.library_pane else { return };
        let Some(i) = p.current else {
            return error(
                &frame,
                "Open or make a component library first (Open… or New…).",
            );
        };
        let (lib, folder) = (p.libraries[i].0.clone(), p.folder.clone());
        let d = FileDialog::builder(&frame)
            .with_message("Import into Library")
            .with_wildcard(
                "3D files (*.obj;*.stl;*.ply;*.gltf;*.glb;*.ifc)|*.obj;*.stl;*.ply;*.gltf;*.glb;*.ifc|All files (*.*)|*.*",
            )
            .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist | FileDialogStyle::Multiple)
            .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let files = d.get_paths();
        if files.is_empty() {
            return;
        }
        // The units and up axis the files are in (shown before anything is copied).
        let Some(options) = import_options_dialog(&frame, &files, None) else {
            return;
        };
        let report = library::import_files(
            &Store::default(),
            library::ImportFilesParams {
                library: lib.to_string_lossy().into_owned(),
                folder: folder.clone(),
                files,
                details: library::ItemDetails {
                    import: Some(options.options),
                    category: Some(options.category),
                    ..Default::default()
                },
                import_duplicates: false,
            },
        );
        match report {
            Ok(r) => {
                if !r.skipped.is_empty() {
                    let lines: Vec<String> = r
                        .skipped
                        .iter()
                        .map(|s| format!("{}: {}", s.file, s.reason))
                        .collect();
                    error(&frame, &format!("Not imported:\n{}", lines.join("\n")));
                }
                frame.set_status_text(
                    &format!("Imported {} file(s) into the library", r.imported.len()),
                    0,
                );
                let current = self.library_pane.as_ref().and_then(|p| p.current);
                self.show_library(current);
                self.show_folder(&folder);
            }
            Err(e) => error(&frame, &e.message),
        }
    }

    /// Create Component: the selection as an IFC component, in the library folder shown (or a
    /// file).
    pub(crate) fn create_component(&mut self) {
        let frame = self.frame;
        let ids = self.selection();
        if ids.is_empty() {
            return frame.set_status_text("Create Component: select the elements first", 0);
        }
        let ask = TextEntryDialog::builder(&frame, "Component name:", "Create Component")
            .with_default_value("New Component")
            .build();
        if ask.show_modal() != ID_OK {
            return;
        }
        let Some(name) = ask.get_value().filter(|n| !n.trim().is_empty()) else {
            return;
        };
        let destination = self.library_pane.as_ref().and_then(|p| {
            p.current.map(|i| {
                (
                    p.libraries[i].0.clone(),
                    p.folder.clone(),
                    p.libraries[i].1.clone(),
                )
            })
        });
        let mut args = json!({ "element_ids": ids, "name": name });
        let shown = match &destination {
            Some((lib, folder, lib_name)) => {
                args["library"] = json!(lib);
                args["folder"] = json!(folder);
                format!(
                    "{lib_name} › {}",
                    if folder.is_empty() { "(top)" } else { folder }
                )
            }
            None => {
                let d = FileDialog::builder(&frame)
                    .with_message("Create Component")
                    .with_wildcard("IFC components (*.ifc)|*.ifc")
                    .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
                    .build();
                if d.show_modal() != ID_OK {
                    return;
                }
                let Some(path) = d.get_path() else { return };
                args["path"] = json!(path);
                path
            }
        };
        match self.run("component.create", args) {
            Ok(v) => {
                let more = v["brought_in"].as_array().map_or(0, Vec::len);
                frame.set_status_text(
                    &format!(
                        "Component \"{name}\" saved to {shown} ({} elements; {more} brought in with the selection)",
                        v["elements"]
                    ),
                    0,
                );
                if let Some((_, folder, _)) = destination {
                    let current = self.library_pane.as_ref().and_then(|p| p.current);
                    self.show_library(current);
                    self.show_folder(&folder);
                }
            }
            Err(e) => error(&frame, &format!("The component was not made:\n{e}")),
        }
    }

    // ---- Place Component ----

    /// Starts the Place Component tool with `source`.
    pub(crate) fn start_place_tool(&mut self, source: Source, name: String) {
        let frame = self.frame;
        if self.doc.is_none() {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        }
        let (outline, scale) = match &source {
            Source::Item { library, item } => match item_mesh(library, item) {
                Ok(m) => (outline_of(m.positions.iter().copied()), 1.0),
                Err(e) => return error(&frame, &e),
            },
            Source::Type(t) => {
                let Some(store) = self.store_now() else {
                    return;
                };
                let Some(ty) = store.get(*t).and_then(mesh_objects::mesh_object_type) else {
                    return;
                };
                let Ok(mesh) = ty.mesh.decode() else { return };
                let pts = mesh
                    .positions
                    .iter()
                    .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]);
                (outline_of(pts), ty.scale)
            }
            Source::Fragment { library, item } => match fragment_outline(&library.join(item)) {
                Ok(o) => (o, 1.0),
                Err(e) => return error(&frame, &e),
            },
        };
        let keep = self
            .place_tool
            .as_ref()
            .map(|t| (t.rotation, t.mirror, t.offset));
        self.end_tool(&frame);
        self.end_insert_tool();
        self.end_modify();
        self.clear_selection();
        let (rotation, mirror, offset) = keep.unwrap_or((0.0, false, 0.0));
        self.place_tool = Some(PlaceTool {
            source,
            name: name.clone(),
            rotation,
            mirror,
            offset,
            outline,
            scale,
        });
        self.properties_stale = true;
        frame.set_status_text(
            &format!(
                "Place {name}: click to place a copy (Spacebar turns it 90°). Esc ends the tool."
            ),
            0,
        );
    }

    pub(crate) fn end_place_tool(&mut self) -> bool {
        if self.place_tool.take().is_none() {
            return false;
        }
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[]);
        }
        self.properties_stale = true;
        true
    }

    /// Where a copy would go: the plan point (snapped) on the plan's level, or the surface hit in
    /// 3D (its level the highest at or below it, and the height above that level).
    pub(crate) fn place_at(&self, x: i32, y: i32) -> Option<([f64; 2], ElementId, f64)> {
        let v = self.active_view()?;
        if let Some(plan) = &v.plan {
            let lines = self.snap_lines();
            let p = self.snapped_at(x, y, None, &lines)?.point;
            return Some((p, plan.level, 0.0));
        }
        let size = v.panel.get_client_size();
        let (o, d) = v
            .camera
            .ray(x as f32, y as f32, size.width as f32, size.height as f32);
        let store = self.store_now()?;
        let levels = reviera_core::levels::list(&store);
        // The surface under the cursor; over nothing, the lowest level's plane.
        let hit = match self
            .scene()
            .pick_within(o, d, f32::NEG_INFINITY, f32::INFINITY)
        {
            Some((_, t)) => o + d * t,
            None => {
                let ground = levels.first().map_or(0.0, |l| l.elevation) as f32;
                if d.z.abs() < 1e-6 {
                    return None;
                }
                let t = (ground - o.z) / d.z;
                if t <= 0.0 {
                    return None;
                }
                o + d * t
            }
        };
        let z = f64::from(hit.z);
        let level = levels
            .iter()
            .filter(|l| l.elevation <= z + 1e-6)
            .max_by(|a, b| a.elevation.total_cmp(&b.elevation))
            .or(levels.first())?;
        Some((
            [f64::from(hit.x), f64::from(hit.y)],
            level.id,
            z - level.elevation,
        ))
    }

    pub(crate) fn place_motion(&mut self, x: i32, y: i32) {
        let at = self.place_at(x, y);
        let plan_z = self
            .active_view()
            .and_then(|v| v.plan.as_ref())
            .map(|p| p.range.cut);
        let Some(t) = &self.place_tool else { return };
        let Some((p, level, height)) = at else {
            if let Some(v) = self.viewport() {
                v.set_overlay_lines(&[]);
            }
            return;
        };
        let z = match plan_z {
            Some(cut) => cut + 0.004,
            None => {
                let base = self
                    .store_now()
                    .and_then(|s| s.get(level).map(reviera_core::levels::elevation))
                    .unwrap_or(0.0);
                base + height + 0.01
            }
        };
        let (s, c) = t.rotation.sin_cos();
        let k = t.scale;
        let ring: Vec<[f32; 3]> = t
            .outline
            .iter()
            .map(|[u, v]| {
                let v = if t.mirror { -v } else { *v };
                [
                    (p[0] + (u * c - v * s) * k) as f32,
                    (p[1] + (u * s + v * c) * k) as f32,
                    z as f32,
                ]
            })
            .collect();
        let n = ring.len();
        let mut segments: Vec<[[f32; 3]; 2]> =
            (0..n).map(|i| [ring[i], ring[(i + 1) % n]]).collect();
        // The insertion point.
        let (px, py, r) = (p[0] as f32, p[1] as f32, 0.15f32);
        segments.push([[px - r, py, z as f32], [px + r, py, z as f32]]);
        segments.push([[px, py - r, z as f32], [px, py + r, z as f32]]);
        if let Some(v) = self.viewport() {
            v.set_overlay_lines(&[Lines {
                color: PREVIEW,
                width: 0.5,
                segments,
            }]);
        }
    }

    pub(crate) fn place_click(&mut self, x: i32, y: i32) {
        let frame = self.frame;
        let Some(t) = self.place_tool.clone() else {
            return;
        };
        let Some((p, level, height)) = self.place_at(x, y) else {
            return frame
                .set_status_text("Place Component: click in a plan, or on a surface in 3D", 0);
        };
        if let Source::Fragment { library, item } = &t.source {
            let args = json!({
                "library": library, "item": item, "point": p, "level": level,
                "rotation": t.rotation,
            });
            match self.run("component.place_fragment", args) {
                Ok(_) => {
                    self.refresh_scene(&frame);
                    self.properties_stale = true;
                    self.place_motion(x, y);
                }
                Err(e) => frame.set_status_text(&e, 0),
            }
            return;
        }
        let mut args = json!({
            "points": [p], "level": level, "offset": height + t.offset,
            "rotation": t.rotation, "mirror": t.mirror,
        });
        match &t.source {
            Source::Item { library, item } => {
                args["library"] = json!(library);
                args["item"] = json!(item);
            }
            Source::Type(ty) => args["type_id"] = json!(ty),
            Source::Fragment { .. } => unreachable!("placed above"),
        }
        match self.run("component.place", args) {
            Ok(v) => {
                // After its first copy a library component is a type in the project: place that.
                if matches!(t.source, Source::Item { .. })
                    && let Some(first) =
                        v.as_array().and_then(|a| a.first()).and_then(Value::as_u64)
                    && let Some(store) = self.store_now()
                    && let Some(ty) = store
                        .get(ElementId(first))
                        .and_then(|e| mesh_objects::type_of(&store, e))
                        .map(|(id, _)| id)
                    && let Some(tool) = &mut self.place_tool
                {
                    tool.source = Source::Type(ty);
                }
                self.refresh_scene(&frame);
                self.properties_stale = true;
                self.place_motion(x, y);
            }
            Err(e) => frame.set_status_text(&e, 0),
        }
    }

    /// Spacebar in Place Component turns the next copy by 90°.
    pub(crate) fn place_key(&mut self, key: i32, x: i32, y: i32) -> bool {
        if key != WXK_SPACE {
            return false;
        }
        let Some(t) = &mut self.place_tool else {
            return false;
        };
        t.rotation = (t.rotation + std::f64::consts::FRAC_PI_2) % std::f64::consts::TAU;
        self.place_motion(x, y);
        true
    }

    /// The palette while Place Component runs: the placement, and the component's details.
    pub(crate) fn place_tool_properties(&mut self, grid: &PropertyGrid) -> bool {
        let Some(t) = self.place_tool.clone() else {
            return false;
        };
        grid.append(Property::category("Place Component", "c_place"));
        grid.append(Property::string("Component", "pc_name", t.name.clone()));
        grid.set_property_read_only("pc_name", true);
        grid.append(Property::string(
            "Rotation",
            "pc_rotation",
            format!("{:.1}°", t.rotation.to_degrees()),
        ));
        grid.append(Property::boolean("Mirrored", "pc_mirror", t.mirror));
        let off = self.format_length(t.offset);
        grid.append(Property::string("Offset", "pc_offset", off));
        grid.set_help_string(
            "pc_offset",
            "Height above the level (in 3D, above the surface clicked)",
        );
        match &t.source {
            Source::Item { library: lib, item } => {
                let Ok(meta) = library::get_meta(
                    &Store::default(),
                    library::GetMetaParams {
                        library: lib.to_string_lossy().into_owned(),
                        item: item.clone(),
                    },
                ) else {
                    return true;
                };
                grid.append(Property::category(
                    "Component details (library)",
                    "c_place_item",
                ));
                let d = &meta.details;
                grid.append(Property::string("Name", "pci_name", meta.info.name.clone()));
                grid.append(Property::string(
                    "Description",
                    "pci_description",
                    d.description.clone().unwrap_or_default(),
                ));
                category_row(grid, "pci_category", meta.info.category);
                let o = d.import.or(meta.folder.import).unwrap_or_default();
                units_rows(grid, "pci", &o);
                grid.append(Property::string(
                    "Tags",
                    "pci_tags",
                    meta.info.tags.join(", "),
                ));
                grid.append(Property::string(
                    "File",
                    "pci_file",
                    format!("{} ({})", item, meta.info.format),
                ));
                grid.set_property_read_only("pci_file", true);
                if let Some(prov) = &d.provenance {
                    for (k, v) in prov {
                        let name = format!("pci_prov_{k}");
                        let text = v.as_str().map_or_else(|| v.to_string(), str::to_owned);
                        grid.append(Property::string(k.replace('_', " "), &name, text));
                        grid.set_property_read_only(&name, true);
                    }
                }
            }
            Source::Type(ty) => {
                self.mesh_type_rows(grid, *ty);
            }
            Source::Fragment { item, .. } => {
                grid.append(Property::category(
                    "Component (IFC fragment)",
                    "c_place_frag",
                ));
                grid.append(Property::string("File", "pcf_file", item.clone()));
                grid.set_property_read_only("pcf_file", true);
                grid.set_help_string("pcf_file", "Its lowest level goes on the level clicked; its other levels on the levels at the same height above");
            }
        }
        true
    }

    /// An edit in Place Component's palette; `false` if `property` is not one.
    pub(crate) fn place_edit(&mut self, property: &str, text: Option<String>) -> bool {
        let Some(t) = self.place_tool.clone() else {
            return false;
        };
        let text = text.unwrap_or_default();
        match property {
            "pc_rotation" => {
                let v = self
                    .run("units.parse", json!({ "text": text, "data_type": "angle" }))
                    .ok()
                    .and_then(|v| v.as_f64());
                if let (Some(v), Some(t)) = (v, &mut self.place_tool) {
                    t.rotation = v;
                }
            }
            "pc_mirror" => {
                if let Some(t) = &mut self.place_tool {
                    t.mirror = text == "true" || text == "1";
                }
            }
            "pc_offset" => {
                let v = self
                    .run(
                        "units.parse",
                        json!({ "text": text, "data_type": "length" }),
                    )
                    .ok()
                    .and_then(|v| v.as_f64());
                if let (Some(v), Some(t)) = (v, &mut self.place_tool) {
                    t.offset = v;
                }
            }
            p if p.starts_with("pci_") => {
                let Source::Item { library: lib, item } = &t.source else {
                    return true;
                };
                let field = &p["pci_".len()..];
                let details = match field {
                    "name" => json!({ "name": text }),
                    "description" => json!({ "description": text }),
                    "tags" => {
                        json!({ "tags": text.split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>() })
                    }
                    "category" => json!({ "category": category_value(&text) }),
                    "units" | "up_axis" | "insertion" => {
                        let meta = library::get_meta(
                            &Store::default(),
                            library::GetMetaParams {
                                library: lib.to_string_lossy().into_owned(),
                                item: item.clone(),
                            },
                        )
                        .ok();
                        let mut o = meta
                            .map(|m| m.details.import.or(m.folder.import).unwrap_or_default())
                            .unwrap_or_default();
                        apply_option(&mut o, field, &text);
                        json!({ "import": o })
                    }
                    _ => return true,
                };
                let r = library::set_meta(
                    &Store::default(),
                    library::SetMetaParams {
                        library: lib.to_string_lossy().into_owned(),
                        paths: vec![item.clone()],
                        details: details.as_object().cloned().unwrap_or_default(),
                        settings: None,
                    },
                );
                if let Err(e) = r {
                    error(&self.frame, &e.message);
                }
                // The outline and thumbnail follow new units or axes.
                if matches!(field, "units" | "up_axis" | "insertion" | "name") {
                    let name = if field == "name" {
                        text.clone()
                    } else {
                        t.name.clone()
                    };
                    self.start_place_tool(t.source.clone(), name);
                    let folder = self
                        .library_pane
                        .as_ref()
                        .map(|p| p.folder.clone())
                        .unwrap_or_default();
                    self.show_folder(&folder);
                }
            }
            _ => return false,
        }
        self.properties_stale = true;
        true
    }

    // ---- Selected custom objects ----

    /// A custom object type's rows (`mt_`): name, category, scale, slot colours, source.
    fn mesh_type_rows(&mut self, grid: &PropertyGrid, ty: ElementId) {
        let Some(store) = self.store_now() else {
            return;
        };
        let Some(e) = store.get(ty) else { return };
        let Some(t) = mesh_objects::mesh_object_type(e) else {
            return;
        };
        grid.append(Property::category("Type", "c_mt"));
        grid.append(Property::string(
            "Type Name",
            "mt_name",
            e.name.clone().unwrap_or_default(),
        ));
        category_row(grid, "mt_category", t.category);
        grid.append(Property::string(
            "Scale (type)",
            "mt_scale",
            format!("{}", t.scale),
        ));
        let size = [
            t.size[0] * t.scale,
            t.size[1] * t.scale,
            t.size[2] * t.scale,
        ];
        let text = format!(
            "{} × {} × {}",
            self.format_length(size[0]),
            self.format_length(size[1]),
            self.format_length(size[2])
        );
        grid.append(Property::string("Size", "mt_size", text));
        grid.set_property_read_only("mt_size", true);
        for (k, s) in t.slots.iter().enumerate() {
            let name = format!("mt_slot_{k}");
            let hex = mesh_objects::color_hex(s.color);
            grid.append(Property::string(
                format!("Colour: {}", s.name),
                &name,
                hex[..7].to_owned(),
            ));
            grid.set_help_string(&name, "A colour as #rrggbb");
        }
        let triangles = format!("{}", t.triangle_count);
        grid.append(Property::string("Triangles", "mt_tris", triangles));
        grid.set_property_read_only("mt_tris", true);
        let source = match &t.library {
            Some(l) => format!("{} ({})", l.item, l.library),
            None => t.source.file.clone(),
        };
        grid.append(Property::string("Source", "mt_source", source));
        grid.set_property_read_only("mt_source", true);
        self.type_rows = Some(ty);
    }

    /// The rows of a selected custom object; `false` if `id` is not one.
    pub(crate) fn mesh_object_rows(&mut self, grid: &PropertyGrid, id: ElementId) -> bool {
        let Some(store) = self.store_now() else {
            return false;
        };
        let Some(e) = store.get(id) else { return false };
        let Some(r) = mesh_objects::mesh_object_recipe(e) else {
            return false;
        };
        let Some((ty, _)) = mesh_objects::type_of(&store, e) else {
            return false;
        };
        grid.append(Property::category("Custom Object", "c_mo"));
        let level = reviera_core::GlobalId::parse(&r.level)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|l| store.get(l))
            .and_then(|l| l.name.clone())
            .unwrap_or_default();
        grid.append(Property::string("Level", "mo_level", level));
        grid.set_property_read_only("mo_level", true);
        let offset = self.format_length(r.offset);
        grid.append(Property::string("Offset", "mo_offset", offset));
        grid.append(Property::string(
            "Rotation",
            "mo_rotation",
            format!("{:.1}°", r.rotation.to_degrees()),
        ));
        grid.append(Property::boolean("Mirrored", "mo_mirror", r.mirror));
        grid.append(Property::string(
            "Scale",
            "mo_scale",
            format!("{}", r.scale),
        ));
        self.mesh_type_rows(grid, ty);
        true
    }

    /// An edit of a custom object's or its type's row; `false` if `property` is not one.
    pub(crate) fn mesh_object_edit(&mut self, property: &str, text: Option<String>) -> bool {
        if !property.starts_with("mo_") && !property.starts_with("mt_") {
            return false;
        }
        let frame = self.frame;
        let text = text.unwrap_or_default();
        let ids: Vec<ElementId> = self.selection();
        let result = if let Some(row) = property.strip_prefix("mo_") {
            let args = match row {
                "offset" => json!({ "offset": text }),
                "rotation" => {
                    json!({ "rotation": if text.trim().ends_with('°') { text.clone() } else { format!("{}°", text.trim()) } })
                }
                "mirror" => json!({ "mirror": text == "true" || text == "1" }),
                "scale" => json!({ "scale": text.trim().parse::<f64>().unwrap_or(1.0) }),
                _ => return true,
            };
            let mut a = args;
            a["element_ids"] = json!(ids);
            self.run("mesh_object.set", a)
        } else {
            let Some(ty) =
                self.type_rows
                    .or_else(|| match self.place_tool.as_ref().map(|t| &t.source) {
                        Some(Source::Type(t)) => Some(*t),
                        _ => None,
                    })
            else {
                return true;
            };
            let row = &property["mt_".len()..];
            let mut a = json!({ "type_id": ty });
            match row {
                "name" => a["name"] = json!(text),
                "scale" => a["scale"] = json!(text.trim().parse::<f64>().unwrap_or(1.0)),
                "category" => a["category"] = json!(category_value(&text)),
                r if r.starts_with("slot_") => {
                    let k: usize = r["slot_".len()..].parse().unwrap_or(0);
                    let store = self.store_now();
                    let slot = store
                        .as_ref()
                        .and_then(|s| s.get(ty))
                        .and_then(mesh_objects::mesh_object_type)
                        .and_then(|t| t.slots.get(k).map(|s| s.name.clone()));
                    let Some(color) = parse_colour(&text) else {
                        error(&frame, "A colour is #rrggbb, for example #c8b89a");
                        return true;
                    };
                    a["colors"] = json!([{ "slot": slot.unwrap_or_default(), "color": color }]);
                }
                _ => return true,
            }
            self.run("mesh_object.set_type", a)
        };
        if let Err(e) = result {
            error(&frame, &format!("The change was not made:\n{e}"));
        }
        self.properties_stale = true;
        self.refresh_scene(&frame);
        true
    }

    // ---- Import Object ----

    /// Architecture › Component › Import Object: a mesh file as a custom object type, then
    /// Place Component with it.
    pub(crate) fn import_object(&mut self) {
        let frame = self.frame;
        if self.doc.is_none() {
            return error(&frame, "Open or create a project first.");
        }
        let d = FileDialog::builder(&frame)
            .with_message("Import Object")
            .with_wildcard("3D models (*.obj;*.stl;*.ply;*.gltf;*.glb)|*.obj;*.stl;*.ply;*.gltf;*.glb|All files (*.*)|*.*")
            .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
            .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(path) = d.get_path() else { return };
        let Some(choice) = import_options_dialog(&frame, std::slice::from_ref(&path), {
            let file = path.clone();
            Some(Box::new(move |o: &ImportOptions| {
                let raw = reviera_core::mesh_io::read(Path::new(&file)).map_err(|e| e.message)?;
                mesh_objects::prepare(raw, o)
                    .map(|p| p.report)
                    .map_err(|e| e.message)
            }))
        }) else {
            return;
        };
        let args = json!({
            "path": path, "category": choice.category, "options": choice.options,
            "name": choice.name, "allow_large": true,
        });
        match self.run("mesh_object.import", args) {
            Ok(v) => {
                let Some(ty) = v.as_u64().map(ElementId) else {
                    return;
                };
                let name = self
                    .store_now()
                    .and_then(|s| s.get(ty).and_then(|e| e.name.clone()))
                    .unwrap_or_default();
                self.start_place_tool(Source::Type(ty), name);
            }
            Err(e) => error(&frame, &format!("The object was not imported:\n{e}")),
        }
    }

    /// The custom object types in the project, for Place Component without a library.
    pub(crate) fn place_component(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return frame.set_status_text("Open or create a project first (File > New)", 0);
        };
        let types: Vec<(ElementId, String)> = mesh_objects::types(&store)
            .into_iter()
            .map(|(id, _, _)| {
                (
                    id,
                    store
                        .get(id)
                        .and_then(|e| e.name.clone())
                        .unwrap_or_default(),
                )
            })
            .collect();
        // The last one placed, else a choice; none yet: the library panel.
        if let Some(Source::Type(t)) = self.place_tool.as_ref().map(|t| t.source.clone()) {
            let name = types
                .iter()
                .find(|x| x.0 == t)
                .map(|x| x.1.clone())
                .unwrap_or_default();
            return self.start_place_tool(Source::Type(t), name);
        }
        if types.is_empty() {
            self.aui.set_pane_shown("components", true);
            self.aui.update();
            return frame.set_status_text(
                "Place Component: pick a component in the Component Library, or Import Object",
                0,
            );
        }
        let labels: Vec<&str> = types.iter().map(|t| t.1.as_str()).collect();
        let d =
            SingleChoiceDialog::builder(&frame, "Component to place:", "Place Component", &labels)
                .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let k = d.get_selection();
        if let Some((id, name)) = usize::try_from(k).ok().and_then(|k| types.get(k)).cloned() {
            self.start_place_tool(Source::Type(id), name);
        }
    }
}

/// The import settings chosen in the Import dialog.
pub(crate) struct ImportChoice {
    pub options: ImportOptions,
    pub category: Category,
    pub name: Option<String>,
}

const UNITS: [(Units, &str); 6] = [
    (Units::Auto, "From the file (metres when it does not say)"),
    (Units::Mm, "Millimetres"),
    (Units::Cm, "Centimetres"),
    (Units::M, "Metres"),
    (Units::In, "Inches"),
    (Units::Ft, "Feet"),
];
const AXES: [(UpAxis, &str); 3] = [
    (UpAxis::Auto, "From the file (glTF: Y)"),
    (UpAxis::Z, "Z"),
    (UpAxis::Y, "Y"),
];
const INSERTIONS: [(Insertion, &str); 3] = [
    (Insertion::BottomCentre, "Bottom centre"),
    (Insertion::Origin, "The file's origin"),
    (Insertion::Corner, "Lowest corner"),
];

/// The dialog for importing: units, up axis, insertion point, category and (for one file) a
/// name. With `inspect`, the file's size and a report follow the choices.
#[allow(clippy::type_complexity)]
fn import_options_dialog(
    frame: &Frame,
    files: &[String],
    mut inspect: Option<Box<dyn FnMut(&ImportOptions) -> Result<mesh_objects::Report, String>>>,
) -> Option<ImportChoice> {
    let title = if files.len() == 1 {
        format!(
            "Import {}",
            Path::new(&files[0])
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()
        )
    } else {
        format!("Import {} files", files.len())
    };
    let dialog = Dialog::builder(frame, &title)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(6)
        .with_hgap(10)
        .build();
    grid.add_growable_col(1, 1);
    let labelled = |label: &str, choices: Vec<String>| {
        let l = StaticText::builder(&dialog).with_label(label).build();
        let c = Choice::builder(&dialog).with_choices(choices).build();
        c.set_selection(0);
        grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
        grid.add(&c, 1, SizerFlag::Expand, 0);
        c
    };
    let units = labelled("Units", UNITS.iter().map(|u| u.1.to_owned()).collect());
    let axis = labelled("Up axis", AXES.iter().map(|u| u.1.to_owned()).collect());
    let insertion = labelled(
        "Insertion point",
        INSERTIONS.iter().map(|u| u.1.to_owned()).collect(),
    );
    let category = labelled(
        "Category",
        Category::ALL.iter().map(|c| c.label().to_owned()).collect(),
    );
    let name = (files.len() == 1).then(|| {
        let l = StaticText::builder(&dialog).with_label("Name").build();
        let stem = Path::new(&files[0])
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let t = TextCtrl::builder(&dialog).with_value(&stem).build();
        grid.add(&l, 0, SizerFlag::AlignCenterVertical, 0);
        grid.add(&t, 1, SizerFlag::Expand, 0);
        t
    });
    sizer.add_sizer(&grid, 0, SizerFlag::All | SizerFlag::Expand, 10);
    let report = StaticText::builder(&dialog).with_label(" ").build();
    report.set_min_size(Size::new(420, 70));
    sizer.add(
        &report,
        1,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand,
        10,
    );
    let ok = Button::builder(&dialog)
        .with_id(ID_OK)
        .with_label("Import")
        .build();
    let cancel = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label("Cancel")
        .build();
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    buttons.add_stretch_spacer(1);
    buttons.add(&cancel, 0, SizerFlag::All, 6);
    buttons.add(&ok, 0, SizerFlag::All, 6);
    sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
    dialog.set_sizer_and_fit(sizer, true);
    dialog.set_affirmative_id(ID_OK);
    ok.set_default();
    let read = |units: &Choice, axis: &Choice, insertion: &Choice| ImportOptions {
        units: UNITS[units.get_selection().unwrap_or(0) as usize].0,
        up_axis: AXES[axis.get_selection().unwrap_or(0) as usize].0,
        insertion: INSERTIONS[insertion.get_selection().unwrap_or(0) as usize].0,
        clean: true,
    };
    // The report now, and whenever units or the axis change.
    let show = std::rc::Rc::new(std::cell::RefCell::new(
        move |o: &ImportOptions| -> Option<String> {
            let f = inspect.as_mut()?;
            Some(match f(o) {
                Ok(r) => format!(
                    "{} {}: {} triangles, {} material{}. Size {:.3} × {:.3} × {:.3} m{}{}.",
                    r.format,
                    if r.units_from_file {
                        "(units from the file)"
                    } else {
                        ""
                    },
                    r.triangles,
                    r.materials.len(),
                    if r.materials.len() == 1 { "" } else { "s" },
                    r.size[0],
                    r.size[1],
                    r.size[2],
                    if r.y_up {
                        ", turned upright from Y up"
                    } else {
                        ""
                    },
                    if r.solid {
                        ", closed"
                    } else {
                        ", not closed (fine for display)"
                    },
                ),
                Err(e) => format!("⚠ {e}"),
            })
        },
    ));
    let refresh = {
        let show = show.clone();
        move || {
            let o = read(&units, &axis, &insertion);
            if let Some(text) = (show.borrow_mut())(&o) {
                report.set_label(&text);
                report.wrap(420);
            }
        }
    };
    refresh();
    {
        let refresh = refresh.clone();
        units.on_selection_changed(move |_| refresh());
    }
    {
        let refresh = refresh.clone();
        axis.on_selection_changed(move |_| refresh());
    }
    let answer = (dialog.show_modal() == ID_OK).then(|| ImportChoice {
        options: read(&units, &axis, &insertion),
        category: Category::ALL[category.get_selection().unwrap_or(0) as usize],
        name: name.map(|t| t.get_value()).filter(|n| !n.trim().is_empty()),
    });
    dialog.destroy();
    answer
}

fn category_row(grid: &PropertyGrid, name: &str, current: Category) {
    let labels: Vec<(String, i32)> = Category::ALL
        .iter()
        .enumerate()
        .map(|(i, c)| (c.label().to_owned(), i as i32))
        .collect();
    let at = Category::ALL
        .iter()
        .position(|c| *c == current)
        .unwrap_or(0) as i32;
    grid.append(Property::enumeration("Category", name, labels, at));
}

fn category_value(label: &str) -> Category {
    Category::ALL
        .into_iter()
        .find(|c| c.label() == label)
        .unwrap_or_default()
}

fn units_rows(grid: &PropertyGrid, prefix: &str, o: &ImportOptions) {
    let list = |v: &[&str]| {
        v.iter()
            .enumerate()
            .map(|(i, s)| ((*s).to_owned(), i as i32))
            .collect::<Vec<_>>()
    };
    let u: Vec<&str> = UNITS.iter().map(|x| x.1).collect();
    let a: Vec<&str> = AXES.iter().map(|x| x.1).collect();
    let n: Vec<&str> = INSERTIONS.iter().map(|x| x.1).collect();
    let ui = UNITS.iter().position(|x| x.0 == o.units).unwrap_or(0) as i32;
    let ai = AXES.iter().position(|x| x.0 == o.up_axis).unwrap_or(0) as i32;
    let ni = INSERTIONS
        .iter()
        .position(|x| x.0 == o.insertion)
        .unwrap_or(0) as i32;
    grid.append(Property::enumeration(
        "Units",
        format!("{prefix}_units"),
        list(&u),
        ui,
    ));
    grid.append(Property::enumeration(
        "Up Axis",
        format!("{prefix}_up_axis"),
        list(&a),
        ai,
    ));
    grid.append(Property::enumeration(
        "Insertion Point",
        format!("{prefix}_insertion"),
        list(&n),
        ni,
    ));
}

fn apply_option(o: &mut ImportOptions, field: &str, label: &str) {
    match field {
        "units" => o.units = UNITS.iter().find(|x| x.1 == label).map_or(o.units, |x| x.0),
        "up_axis" => {
            o.up_axis = AXES
                .iter()
                .find(|x| x.1 == label)
                .map_or(o.up_axis, |x| x.0)
        }
        "insertion" => {
            o.insertion = INSERTIONS
                .iter()
                .find(|x| x.1 == label)
                .map_or(o.insertion, |x| x.0)
        }
        _ => {}
    }
}

fn parse_colour(text: &str) -> Option<[f32; 4]> {
    let h = text.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let b = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16)
            .ok()
            .map(|v| f32::from(v) / 255.0)
    };
    Some([b(0)?, b(2)?, b(4)?, 1.0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_of_a_square_and_inner_points() {
        let h = hull(vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [0.5, 0.5],
            [0.2, 0.7],
        ]);
        assert_eq!(h.len(), 4);
    }

    #[test]
    fn colours_parse() {
        assert_eq!(parse_colour("#ff0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_colour("nope"), None);
    }
}
