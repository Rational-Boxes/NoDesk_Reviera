// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The model tree (UI-TREE-001) and the scopes of temporary visibility (DOC-GFX-008).
//!
//! Reviera elements are placed by their own data: a wall under its base level, a floor or
//! ceiling under its level, a door or window under its host wall, a grid line under its grid
//! system. Other elements are placed by the IFC file's relationships (containment, aggregation,
//! voids and fills), given as a map from child to parent GlobalId; openings are skipped, so a
//! door read from IFC sits under the wall it fills.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::Element;
use crate::id::{ElementId, GlobalId};
use crate::store::Store;

/// The parent of each IFC object as the file records it, by GlobalId.
pub type Imported = HashMap<GlobalId, GlobalId>;

const SPATIAL: &[&str] = &["IfcProject", "IfcSite", "IfcBuilding", "IfcBuildingStorey"];

fn is_spatial(e: &Element) -> bool {
    SPATIAL.contains(&e.class.as_str())
}

/// Whether an element belongs in the model tree: spatial structure and physical elements, not
/// views, types, openings, groups or systems.
pub fn in_tree(e: &Element) -> bool {
    if crate::grids::is_grid(e) {
        return true;
    }
    let c = e.class.as_str();
    c.starts_with("Ifc")
        && !crate::categories::is_type(e)
        && c != "IfcOpeningElement"
        && !c.starts_with("IfcRel")
        && ![
            "System",
            "Group",
            "Zone",
            "Inventory",
            "Model",
            "Actor",
            "Control",
        ]
        .iter()
        .any(|s| c.ends_with(s))
        && !c.starts_with("IfcStructural")
        && !c.starts_with("IfcTask")
        && !c.starts_with("IfcProcedure")
        && !c.starts_with("IfcEvent")
}

/// The parent element of `e` in the tree, before openings are skipped.
fn raw_parent(store: &Store, e: &Element, imported: &Imported) -> Option<ElementId> {
    let by_guid = |g: &str| GlobalId::parse(g).and_then(|g| store.find_by_guid(&g));
    if let Some(r) = crate::walls::wall_recipe(e) {
        return by_guid(&r.base_level);
    }
    if let Some((_, r)) = crate::floors::slab_recipe(e) {
        return by_guid(&r.level);
    }
    if let Some(r) = crate::roofs::roof_recipe(e) {
        return by_guid(&r.level);
    }
    if let Some(r) = crate::rooms::room_recipe(e) {
        return by_guid(&r.level);
    }
    if let Some(r) = crate::stairs::stair_recipe(e) {
        return by_guid(&r.base_level);
    }
    if let Some(r) = crate::inserts::insert_recipe(e) {
        return by_guid(&r.host);
    }
    if let Some(insert) = crate::inserts::opening_of(e) {
        return by_guid(&insert);
    }
    if let Some(g) = crate::grids::grid_data(e) {
        return by_guid(&g.system);
    }
    imported.get(e.guid()).and_then(|p| store.find_by_guid(p))
}

/// The parent of every element of the tree that has one. Openings are passed through (an
/// element filling an opening gets the opening's host); spatial elements without one hang under
/// the next level of structure (a storey under the first building, and so on).
pub fn parents(store: &Store, imported: &Imported) -> HashMap<ElementId, ElementId> {
    let first = |class: &str| {
        store
            .iter()
            .filter(|e| e.class == class)
            .map(Element::id)
            .min()
    };
    let fallback = |class: &str| match class {
        "IfcBuildingStorey" => first("IfcBuilding"),
        "IfcBuilding" => first("IfcSite"),
        "IfcSite" => first("IfcProject"),
        _ => None,
    };
    let mut out = HashMap::new();
    for e in store.iter().filter(|e| in_tree(e)) {
        let mut parent = raw_parent(store, e, imported);
        // Through openings (at most a few steps; a cycle in bad data stops here).
        for _ in 0..4 {
            match parent.and_then(|p| store.get(p)) {
                Some(p) if p.class == "IfcOpeningElement" => {
                    parent = raw_parent(store, p, imported);
                }
                _ => break,
            }
        }
        let parent = parent
            .filter(|p| *p != e.id() && store.get(*p).is_some_and(in_tree))
            .or_else(|| fallback(&e.class));
        if let Some(p) = parent {
            out.insert(e.id(), p);
        }
    }
    out
}

/// The children of each element, from [`parents`].
pub fn children(parents: &HashMap<ElementId, ElementId>) -> HashMap<ElementId, Vec<ElementId>> {
    let mut out: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
    for (c, p) in parents {
        out.entry(*p).or_default().push(*c);
    }
    for list in out.values_mut() {
        list.sort();
    }
    out
}

/// What a temporary visibility tool acts on, given the picked elements (DOC-GFX-008).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// The elements themselves.
    #[default]
    Element,
    /// The elements and everything below them in the model tree.
    Subtree,
    /// Every element of their categories.
    Category,
    /// Every instance of their types (the picked elements included).
    Type,
}

fn category_key(e: &Element) -> String {
    crate::categories::category_of(e).map_or_else(|| e.class.clone(), |c| c.key.to_owned())
}

fn type_key(e: &Element) -> Option<String> {
    if crate::categories::is_type(e) {
        return Some(e.guid().as_str().to_owned());
    }
    e.param("@type").and_then(Value::as_str).map(str::to_owned)
}

/// The elements `scope` reaches from `ids` (DOC-GFX-008). Ids that are not in the store are
/// dropped. Types reach their instances.
pub fn expand(
    store: &Store,
    parents: &HashMap<ElementId, ElementId>,
    ids: &[ElementId],
    scope: Scope,
) -> BTreeSet<ElementId> {
    let picked: Vec<&Element> = ids.iter().filter_map(|id| store.get(*id)).collect();
    match scope {
        Scope::Element => picked.iter().map(|e| e.id()).collect(),
        Scope::Subtree => {
            let kids = children(parents);
            let mut out = BTreeSet::new();
            let mut queue: Vec<ElementId> = picked.iter().map(|e| e.id()).collect();
            while let Some(id) = queue.pop() {
                if out.insert(id) {
                    queue.extend(kids.get(&id).into_iter().flatten().copied());
                }
            }
            out
        }
        Scope::Category => {
            let keys: BTreeSet<String> = picked.iter().map(|e| category_key(e)).collect();
            store
                .iter()
                .filter(|e| in_tree(e) && keys.contains(&category_key(e)))
                .map(Element::id)
                .collect()
        }
        Scope::Type => {
            let keys: BTreeSet<String> = picked.iter().filter_map(|e| type_key(e)).collect();
            let mut out: BTreeSet<ElementId> = picked
                .iter()
                .filter(|e| !crate::categories::is_type(e))
                .map(|e| e.id())
                .collect();
            out.extend(
                store
                    .iter()
                    .filter(|e| !crate::categories::is_type(e))
                    .filter(|e| type_key(e).is_some_and(|k| keys.contains(&k)))
                    .map(Element::id),
            );
            out
        }
    }
}

/// The kind of a model tree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Project,
    Site,
    Building,
    Storey,
    /// The elements of one category under a storey (or under *Not on a level*).
    Category,
    Element,
    /// Elements that are on no level.
    Unplaced,
}

/// A node of the model tree.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct TreeNode {
    /// The element, for project, site, building, storey and element nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<ElementId>,
    pub label: String,
    pub kind: NodeKind,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    /// Every element at or below this node.
    pub fn elements(&self) -> Vec<ElementId> {
        let mut out = Vec::new();
        let mut stack = vec![self];
        while let Some(n) = stack.pop() {
            out.extend(n.id);
            stack.extend(&n.children);
        }
        out
    }
}

/// A label for an element: its name, else its mark, else its class and id.
pub fn label(e: &Element) -> String {
    let name = e.name.as_deref().filter(|n| !n.trim().is_empty());
    let mark = e
        .param("@Tag")
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty());
    match (name, mark) {
        (Some(n), _) => n.to_owned(),
        (None, Some(m)) => m.to_owned(),
        (None, None) => format!("{} {}", e.class.trim_start_matches("Ifc"), e.id()),
    }
}

fn category_name(e: &Element) -> String {
    if let Some(c) = crate::categories::category_of(e) {
        return c.name.to_owned();
    }
    let c = e.class.trim_start_matches("Ifc");
    let c = c.strip_suffix("StandardCase").unwrap_or(c);
    format!("{c}s")
}

/// The model tree (UI-TREE-001): Project › Site › Building › storeys (highest first) › one node
/// per category › elements, with hosted elements and parts under their host; elements on no
/// level under a final *Not on a level* node.
pub fn tree(store: &Store, imported: &Imported) -> TreeNode {
    let parents = parents(store, imported);
    let kids = children(&parents);
    let elevation = |id: ElementId| {
        store
            .get(id)
            .and_then(|e| e.param("@Elevation"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    fn element_node(
        store: &Store,
        kids: &HashMap<ElementId, Vec<ElementId>>,
        id: ElementId,
        depth: usize,
    ) -> TreeNode {
        let e = store.get(id).expect("listed");
        let mut children: Vec<TreeNode> = if depth > 16 {
            Vec::new()
        } else {
            kids.get(&id)
                .into_iter()
                .flatten()
                .map(|c| element_node(store, kids, *c, depth + 1))
                .collect()
        };
        children.sort_by(|a, b| a.label.cmp(&b.label));
        TreeNode {
            id: Some(id),
            label: label(e),
            kind: NodeKind::Element,
            children,
        }
    }
    // Groups a list of elements into category nodes.
    let by_category = |ids: &[ElementId]| -> Vec<TreeNode> {
        let mut groups: BTreeMap<String, Vec<ElementId>> = BTreeMap::new();
        for id in ids {
            if let Some(e) = store.get(*id) {
                groups.entry(category_name(e)).or_default().push(*id);
            }
        }
        groups
            .into_iter()
            .map(|(name, ids)| {
                let mut children: Vec<TreeNode> = ids
                    .iter()
                    .map(|id| element_node(store, &kids, *id, 0))
                    .collect();
                children.sort_by(|a, b| a.label.cmp(&b.label));
                TreeNode {
                    id: None,
                    label: format!("{name} ({})", children.len()),
                    kind: NodeKind::Category,
                    children,
                }
            })
            .collect()
    };
    fn spatial_kind(class: &str) -> NodeKind {
        match class {
            "IfcProject" => NodeKind::Project,
            "IfcSite" => NodeKind::Site,
            "IfcBuilding" => NodeKind::Building,
            _ => NodeKind::Storey,
        }
    }
    // A spatial node: spatial children first (storeys highest first), then its elements by
    // category.
    fn spatial_node(
        store: &Store,
        kids: &HashMap<ElementId, Vec<ElementId>>,
        id: ElementId,
        by_category: &dyn Fn(&[ElementId]) -> Vec<TreeNode>,
        elevation: &dyn Fn(ElementId) -> f64,
        depth: usize,
    ) -> TreeNode {
        let e = store.get(id).expect("listed");
        let (mut spatial, elements): (Vec<ElementId>, Vec<ElementId>) = kids
            .get(&id)
            .into_iter()
            .flatten()
            .copied()
            .partition(|c| store.get(*c).is_some_and(is_spatial));
        spatial.sort_by(|a, b| elevation(*b).total_cmp(&elevation(*a)).then(a.cmp(b)));
        let mut children: Vec<TreeNode> = if depth > 8 {
            Vec::new()
        } else {
            spatial
                .iter()
                .map(|c| spatial_node(store, kids, *c, by_category, elevation, depth + 1))
                .collect()
        };
        children.extend(by_category(&elements));
        TreeNode {
            id: Some(id),
            label: label(e),
            kind: spatial_kind(&e.class),
            children,
        }
    }
    let roots: Vec<ElementId> = store
        .iter()
        .filter(|e| in_tree(e) && !parents.contains_key(&e.id()))
        .map(Element::id)
        .collect();
    let (mut spatial_roots, unplaced): (Vec<ElementId>, Vec<ElementId>) = roots
        .into_iter()
        .partition(|id| store.get(*id).is_some_and(is_spatial));
    spatial_roots.sort();
    let mut top: Vec<TreeNode> = spatial_roots
        .iter()
        .map(|id| spatial_node(store, &kids, *id, &by_category, &elevation, 0))
        .collect();
    if !unplaced.is_empty() {
        top.push(TreeNode {
            id: None,
            label: "Not on a level".into(),
            kind: NodeKind::Unplaced,
            children: by_category(&unplaced),
        });
    }
    if top.len() == 1 && top[0].kind == NodeKind::Project {
        return top.remove(0);
    }
    TreeNode {
        id: None,
        label: "Model".into(),
        kind: NodeKind::Project,
        children: top,
    }
}
