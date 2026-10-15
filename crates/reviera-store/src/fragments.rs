// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Components that are model fragments (DM-LIB-003): Create Component writes any selection as an
//! IFC file of that fragment; placing one copies its elements into the model.
//!
//! - **Create Component** (`component.create`) takes the selection, the elements hosted by it
//!   (doors and windows in its walls, their openings, skylights in its roofs, …) and everything
//!   they depend on (hosts, types, levels and any other element a recipe names). It writes them
//!   to a standalone IFC4 model with the base point at the origin and the lowest level at 0, into
//!   a Component Library folder (adding its manifest entry) or to any path.
//! - **Placing a fragment** (`component.place_fragment`) opens it and copies its elements in with
//!   new GlobalIds, every reference inside their recipes rewritten to the new ones. Its levels
//!   map onto the model's: the lowest to the level clicked, the others to levels at the same
//!   height above it (made when missing). Types the model has already (the same GlobalId, or the
//!   same class and name) are reused. The copies are then turned and moved into place.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use reviera_core::modify::{self, Xform};
use reviera_core::{
    Command, CommandContext, CommandMeta, Effect, Element, ElementId, ElementRef, Error, GlobalId,
    NewElement, Origin, Result, Store,
};
use reviera_ifc::Schema;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::LocalProject;

/// Every GlobalId string in a value.
fn guids_in(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if s.len() == 22 && GlobalId::parse(s).is_some() => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| guids_in(x, out)),
        Value::Object(o) => o.values().for_each(|x| guids_in(x, out)),
        _ => {}
    }
}

/// The elements `e`'s parameters name.
fn refs_of(store: &Store, e: &Element) -> Vec<ElementId> {
    let mut gs = Vec::new();
    for v in e.params.values() {
        guids_in(v, &mut gs);
    }
    gs.iter()
        .filter_map(|g| store.find_by_guid(&GlobalId::parse(g)?))
        .filter(|id| *id != e.id())
        .collect()
}

/// Elements a fragment never carries: the project's spatial structure, views, sheets and other
/// Reviera-only records.
fn excluded(e: &Element) -> bool {
    matches!(e.class.as_str(), "IfcProject" | "IfcSite" | "IfcBuilding")
        || !e.class.starts_with("Ifc")
        || e.param("rxd:view").is_some()
}

fn is_level(e: &Element) -> bool {
    e.class == "IfcBuildingStorey"
}

/// Whether an element moves with another rather than on its own (a door's opening, a dormer's
/// parts, a railing's gates).
fn carried(e: &Element) -> bool {
    reviera_core::inserts::opening_of(e).is_some()
        || reviera_core::dormers::dormer_of(e).is_some()
        || reviera_core::railings::gate_of(e).is_some()
}

/// The elements to move: those that do not move with another.
fn movers(txn: &reviera_core::Transaction<'_>, ids: &[ElementId]) -> Vec<ElementRef> {
    ids.iter()
        .filter(|i| txn.store().get(**i).is_some_and(|e| !carried(e)))
        .map(|i| ElementRef::Id(*i))
        .collect()
}

/// The selection, what it hosts, and everything they depend on.
pub fn closure(store: &Store, selected: &[ElementId], include_hosted: bool) -> BTreeSet<ElementId> {
    let mut set: BTreeSet<ElementId> = selected.iter().copied().collect();
    // Hosted by the selection (and by what is hosted by it): elements that name a member and are
    // not types or levels.
    if include_hosted {
        loop {
            let guids: BTreeSet<String> = set
                .iter()
                .filter_map(|i| store.get(*i))
                .filter(|e| !is_level(e) && !reviera_core::categories::is_type(e))
                .map(|e| e.guid().to_string())
                .collect();
            let more: Vec<ElementId> = store
                .iter()
                .filter(|e| !set.contains(&e.id()) && !excluded(e) && !is_level(e))
                .filter(|e| !reviera_core::categories::is_type(e))
                .filter(|e| {
                    let mut gs = Vec::new();
                    e.params.values().for_each(|v| guids_in(v, &mut gs));
                    gs.iter().any(|g| guids.contains(g))
                })
                .map(Element::id)
                .collect();
            if more.is_empty() {
                break;
            }
            set.extend(more);
        }
    }
    // What they depend on: hosts, types, levels and other named elements.
    let mut queue: Vec<ElementId> = set.iter().copied().collect();
    while let Some(id) = queue.pop() {
        let Some(e) = store.get(id) else { continue };
        for r in refs_of(store, e) {
            if let Some(re) = store.get(r)
                && !excluded(re)
                && set.insert(r)
            {
                queue.push(r);
            }
        }
    }
    set
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub element_ids: Vec<ElementRef>,
    /// The component's name (the file's name too).
    pub name: String,
    /// A Component Library to save into, and a folder in it …
    pub library: Option<String>,
    #[serde(default)]
    pub folder: String,
    /// … or a file path.
    pub path: Option<String>,
    /// The base point in plan (the point placed later); the selection's bottom centre when
    /// omitted.
    pub base_point: Option<[f64; 2]>,
    /// Bring the elements the selection hosts (default on).
    #[serde(default = "yes")]
    pub include_hosted: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Created {
    /// The IFC file written.
    pub path: String,
    /// Its path in the library, when saved into one.
    pub item: Option<String>,
    pub elements: usize,
    /// Elements brought in beyond the selection (hosted, hosts, types, levels …), by label.
    pub brought_in: Vec<String>,
}

fn label(e: &Element) -> String {
    match &e.name {
        Some(n) if !n.is_empty() => format!("{} {n}", e.class),
        _ => format!("{} {}", e.class, e.id()),
    }
}

/// Create Component (`component.create`).
pub fn create(store: &Store, p: CreateParams) -> Result<Created> {
    let selected: Vec<ElementId> = p
        .element_ids
        .iter()
        .map(|r| {
            store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such element"))
        })
        .collect::<Result<_>>()?;
    if selected.is_empty() {
        return Err(Error::new(
            "component.empty",
            "select the elements of the component",
        ));
    }
    let set = closure(store, &selected, p.include_hosted);
    // Where to write it.
    let safe: String = p
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_().".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let (path, item) = match (&p.library, &p.path) {
        (Some(lib), None) => {
            let dir = Path::new(lib).join(&p.folder);
            std::fs::create_dir_all(&dir)
                .map_err(|e| Error::new("library.io", format!("{}: {e}", dir.display())))?;
            let mut name = PathBuf::from(format!("{safe}.ifc"));
            let mut k = 2;
            while dir.join(&name).exists() {
                name = PathBuf::from(format!("{safe} ({k}).ifc"));
                k += 1;
            }
            let rel = Path::new(&p.folder).join(&name);
            (
                dir.join(&name),
                Some(rel.to_string_lossy().replace('\\', "/")),
            )
        }
        (None, Some(path)) => (PathBuf::from(path), None),
        _ => {
            return Err(Error::new(
                "component.where",
                "give a library (and folder), or a path",
            ));
        }
    };
    // The base point: given, or the selection's bottom centre.
    let base = p.base_point.or_else(|| {
        let bodies = reviera_core::bodies::all(store);
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for b in bodies.iter().filter(|b| set.contains(&b.element)) {
            if let Some((l, h)) = b.bounds() {
                for k in 0..2 {
                    lo[k] = lo[k].min(l[k]);
                    hi[k] = hi[k].max(h[k]);
                }
            }
        }
        (lo[0] <= hi[0]).then(|| [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0])
    });
    let base = base.unwrap_or([0.0, 0.0]);
    let tmp = path.with_extension("ifc.part");
    let _ = std::fs::remove_file(&tmp);
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &tmp)
        .map_err(|e| Error::new("component.write", e.to_string()))?;
    let levels: Vec<ElementId> = set
        .iter()
        .copied()
        .filter(|i| store.get(*i).is_some_and(is_level))
        .collect();
    let low = levels
        .iter()
        .filter_map(|i| store.get(*i))
        .map(reviera_core::levels::elevation)
        .fold(f64::INFINITY, f64::min);
    let low = if low.is_finite() { low } else { 0.0 };
    {
        let mut txn = doc.transaction("Create Component", Origin::ui());
        // The new model's own first level gives way to the fragment's levels.
        if !levels.is_empty() {
            let own: Vec<ElementId> = txn
                .store()
                .iter()
                .filter(|e| is_level(e))
                .map(Element::id)
                .collect();
            for l in own {
                txn.delete(l)?;
            }
        }
        let mut copied: Vec<ElementId> = Vec::new();
        for id in &set {
            let e = store.get(*id).expect("listed");
            let mut n = NewElement::new(e.class.clone()).guid(e.guid().clone());
            if let Some(name) = &e.name {
                n = n.name(name.clone());
            }
            for (k, v) in &e.params {
                let v = if is_level(e) && k == "@Elevation" {
                    Value::from(v.as_f64().unwrap_or(0.0) - low)
                } else {
                    v.clone()
                };
                n = n.param(k.clone(), v);
            }
            let new = txn.create(n)?;
            if !is_level(e) && !reviera_core::categories::is_type(e) {
                copied.push(new);
            }
        }
        // The base point to the origin.
        if base != [0.0, 0.0] && !copied.is_empty() {
            let refs = movers(&txn, &copied);
            modify::transform(
                &mut txn,
                &refs,
                Xform::Translate([-base[0], -base[1], 0.0]),
                false,
            )?;
        }
        txn.commit();
    }
    project
        .save(&doc)
        .map_err(|e| Error::new("component.write", e.to_string()))?;
    drop(project);
    std::fs::rename(&tmp, &path)
        .map_err(|e| Error::new("component.write", format!("{}: {e}", path.display())))?;
    // A library item gets its manifest entry.
    if let (Some(lib), Some(item)) = (&p.library, &item) {
        let mut details = serde_json::Map::new();
        details.insert("name".into(), Value::from(p.name.clone()));
        reviera_core::library::set_meta(
            store,
            reviera_core::library::SetMetaParams {
                library: lib.clone(),
                paths: vec![item.clone()],
                details,
                settings: None,
            },
        )?;
    }
    let brought_in = set
        .iter()
        .filter(|i| !selected.contains(i))
        .filter_map(|i| store.get(*i))
        .map(label)
        .collect();
    Ok(Created {
        path: path.to_string_lossy().into_owned(),
        item,
        elements: set.len(),
        brought_in,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    /// A Component Library and the fragment's path in it …
    pub library: Option<String>,
    pub item: Option<String>,
    /// … or an IFC file.
    pub path: Option<String>,
    /// Where its base point goes (plan, metres).
    pub point: [f64; 2],
    /// The level its lowest level goes to.
    pub level: ElementRef,
    /// Counter-clockwise; accepts units ("90°").
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

/// The fragment's file.
fn fragment_path(p: &PlaceParams) -> Result<PathBuf> {
    match (&p.library, &p.item, &p.path) {
        (Some(l), Some(i), None) => Ok(Path::new(l).join(i)),
        (None, None, Some(path)) => Ok(PathBuf::from(path)),
        _ => Err(Error::new(
            "component.what",
            "give a library and an item, or a path",
        )),
    }
}

/// Places a fragment (`component.place_fragment`); returns the elements placed (not types or
/// levels).
pub fn place(txn: &mut reviera_core::Transaction<'_>, p: PlaceParams) -> Result<Vec<ElementId>> {
    let file = fragment_path(&p)?;
    let (_fp, fdoc, _) = LocalProject::open(&file)
        .map_err(|e| Error::new("component.unreadable", format!("{}: {e}", file.display())))?;
    let fstore = fdoc.store().clone();
    let store = txn.store().clone();
    let target = store
        .resolve(&p.level)
        .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
    let target_z = reviera_core::levels::elevation(store.get(target).expect("resolved"));
    let mut map: HashMap<String, String> = HashMap::new();
    // Levels: by height above the fragment's lowest.
    let mut flevels: Vec<&Element> = fstore.iter().filter(|e| is_level(e)).collect();
    flevels.sort_by(|a, b| {
        reviera_core::levels::elevation(a).total_cmp(&reviera_core::levels::elevation(b))
    });
    let low = flevels
        .first()
        .map_or(0.0, |l| reviera_core::levels::elevation(l));
    for fl in &flevels {
        let z = target_z + reviera_core::levels::elevation(fl) - low;
        let existing = txn
            .store()
            .iter()
            .filter(|e| is_level(e))
            .find(|e| (reviera_core::levels::elevation(e) - z).abs() < 1e-3)
            .map(|e| e.guid().to_string());
        let guid = match existing {
            Some(g) => g,
            None => {
                let id = txn.create(
                    NewElement::new("IfcBuildingStorey")
                        .name(
                            fl.name
                                .clone()
                                .unwrap_or_else(|| format!("Level at {z:.2}")),
                        )
                        .param("@Elevation", z),
                )?;
                txn.store().get(id).expect("created").guid().to_string()
            }
        };
        map.insert(fl.guid().to_string(), guid);
    }
    // Types: reused when the model has them; elements: new GlobalIds.
    let mut to_copy: Vec<&Element> = Vec::new();
    for e in fstore.iter().filter(|e| !excluded(e) && !is_level(e)) {
        if reviera_core::categories::is_type(e) {
            let same = store.find_by_guid(e.guid()).or_else(|| {
                store
                    .iter()
                    .find(|t| t.class == e.class && t.name.is_some() && t.name == e.name)
                    .map(Element::id)
            });
            if let Some(t) = same {
                map.insert(
                    e.guid().to_string(),
                    store.get(t).expect("found").guid().to_string(),
                );
                continue;
            }
        }
        // A type keeps its GlobalId when the model does not have it yet, so the next placement
        // finds it; other elements get new ones.
        let keep = reviera_core::categories::is_type(e) && store.find_by_guid(e.guid()).is_none();
        let guid = if keep {
            e.guid().to_string()
        } else {
            GlobalId::new().to_string()
        };
        map.insert(e.guid().to_string(), guid);
        to_copy.push(e);
    }
    let rewrite = |v: &Value| rewrite_value(v, &map);
    let mut placed = Vec::new();
    // Types first, so their elements find them.
    to_copy.sort_by_key(|e| !reviera_core::categories::is_type(e));
    for e in to_copy {
        let guid = GlobalId::parse(&map[e.guid().as_str()]).expect("a guid");
        let mut n = NewElement::new(e.class.clone()).guid(guid);
        if let Some(name) = &e.name {
            n = n.name(name.clone());
        }
        for (k, v) in &e.params {
            if k == "rxd:identity" {
                continue;
            }
            n = n.param(k.clone(), rewrite(v));
        }
        let id = txn.create(n)?;
        if !reviera_core::categories::is_type(e) {
            placed.push(id);
        }
    }
    // Turned about its base point, then moved there.
    if !placed.is_empty() {
        let refs = movers(txn, &placed);
        if p.rotation != 0.0 {
            modify::transform(
                txn,
                &refs,
                Xform::Rotate {
                    center: [0.0, 0.0],
                    angle: p.rotation,
                },
                false,
            )?;
        }
        if p.point != [0.0, 0.0] {
            modify::transform(
                txn,
                &refs,
                Xform::Translate([p.point[0], p.point[1], 0.0]),
                false,
            )?;
        }
    }
    Ok(placed)
}

fn rewrite_value(v: &Value, map: &HashMap<String, String>) -> Value {
    match v {
        Value::String(s) => Value::String(map.get(s).cloned().unwrap_or_else(|| s.clone())),
        Value::Array(a) => Value::Array(a.iter().map(|x| rewrite_value(x, map)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), rewrite_value(x, map)))
                .collect(),
        ),
        other => other.clone(),
    }
}

// ---- Commands ----

pub(crate) struct ComponentCreate;

impl Command for ComponentCreate {
    type Params = CreateParams;
    type Output = Created;
    const META: CommandMeta = CommandMeta {
        name: "component.create",
        version: 1,
        summary: "Create a component from a selection",
        description: "Create Component (DM-LIB-003): writes the selected elements, what they host (doors and \
                      windows in their walls, openings, skylights …) unless `include_hosted` is false, and \
                      everything they depend on (hosts, types, levels) as a standalone IFC4 file of that model \
                      fragment, with the base point (default: the selection's bottom centre) at the origin and \
                      the lowest level at 0. Into a Component Library `folder` (adding its manifest entry, \
                      named `name`) or to `path`. The model is not changed. Returns the file and what was \
                      brought in beyond the selection.",
        effects: &[Effect::Read, Effect::FileWrite],
        transaction_name: "",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: CreateParams) -> Result<Created> {
        create(cx.store(), p)
    }
}

pub(crate) struct ComponentPlaceFragment;

impl Command for ComponentPlaceFragment {
    type Params = PlaceParams;
    type Output = Vec<ElementId>;
    const META: CommandMeta = CommandMeta {
        name: "component.place_fragment",
        version: 1,
        summary: "Place an IFC component",
        description: "Places a component that is a model fragment (an IFC file, from a Component Library or a \
                      path) with its base point at `point` and its lowest level on `level`, turned by \
                      `rotation`: its elements are copied in with new GlobalIds and their references \
                      rewritten; its other levels map to the model's levels at the same height above (made \
                      when missing); types the model has (same GlobalId, or class and name) are reused. \
                      Returns the elements placed.",
        effects: &[Effect::Edit, Effect::FileRead],
        transaction_name: "Place Component",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: PlaceParams) -> Result<Vec<ElementId>> {
        place(cx.edit()?, p)
    }
}
