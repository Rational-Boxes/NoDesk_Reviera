// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Annotation in 3D views (DOC-ANN-025): the text notes, tags and dimensions of the model's plans,
//! shown in a 3D view as floating labels at their place in the model, drawn flat to the screen
//! like tooltips (the window draws them, `reviera_render::tooltips`).
//!
//! - **Where:** a plan's annotation stands on its level: a text note 1 m above it, a door's or
//!   window's tag at the insert's centre, a wall's tag 1.5 m up its middle, a room's tag 0.3 m
//!   above its tag point, a dimension's values on its line at the level. A host tagged in several
//!   plans is labelled once.
//! - **Classes:** each label has a class: `text`, `dimensions`, or `tags:<category>` (Doors,
//!   Windows, Rooms, …), so a view can show some and not others.
//! - **Per view:** a 3D view keeps whether it shows annotation at all and which classes it hides
//!   (`rxd:annotations_3d`): shown, with dimensions hidden, unless set (`view.set_3d_annotations`).
//! - **Behind the model:** a label whose point the model hides from the eye (by more than 0.4 m,
//!   so a tag inside its door still shows) is left out unless the view says otherwise
//!   (`occlude`); what the section box and slices cut away and what is hidden do not hide it.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;

pub const VIEW_KEY: &str = "rxd:annotations_3d";

/// A floating label: its text lines at a point of the model.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Tip {
    /// The annotation element.
    pub id: ElementId,
    pub class: String,
    pub anchor: [f64; 3],
    pub lines: Vec<String>,
    /// The GlobalId of the plan the annotation is drawn in.
    pub plan: String,
}

/// How a 3D view shows annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Settings {
    pub shown: bool,
    /// Classes not shown.
    pub hidden: BTreeSet<String>,
    /// Labels whose point is behind the model are left out.
    #[serde(default = "yes")]
    pub occlude: bool,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self { shown: true, hidden: ["dimensions".to_owned()].into(), occlude: true }
    }
}

impl Settings {
    pub fn shows(&self, class: &str) -> bool {
        self.shown && !self.hidden.contains(class)
    }
}

pub fn settings(e: &Element) -> Settings {
    e.param(VIEW_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// A class's name for people: `tags:Doors` is "Door tags".
pub fn label(class: &str) -> String {
    match class {
        "text" => "Text notes".into(),
        "dimensions" => "Dimensions".into(),
        c => match c.strip_prefix("tags:") {
            Some(cat) => format!("{} tags", cat.strip_suffix('s').unwrap_or(cat)),
            None => c.to_owned(),
        },
    }
}

/// The plans' annotation as floating labels, every class.
pub fn tips(store: &Store) -> Vec<Tip> {
    // Each plan's level elevation, by the plan's GlobalId.
    let mut levels: BTreeMap<String, f64> = BTreeMap::new();
    for e in store.iter() {
        if let Some(d) = crate::views::view_data(e)
            && crate::plans::is_plan(&d.kind)
            && let Some(l) = &d.level
            && let Ok(z) = crate::levels::elevation_of(store, l)
        {
            levels.insert(e.guid().to_string(), z);
        }
    }
    let parents = std::collections::HashMap::new();
    let reader = crate::schedules::FieldReader::new(store, &parents);
    let settings = crate::units::settings_of(store);
    let mut out = Vec::new();
    let mut tagged: HashSet<(String, String)> = HashSet::new();
    for e in store.iter() {
        if let Some(t) = crate::notes::text_data(e) {
            let Some(z) = levels.get(&t.view) else { continue };
            let lines: Vec<String> = t.content.lines().map(str::to_owned).filter(|l| !l.trim().is_empty()).collect();
            if !lines.is_empty() {
                out.push(Tip { id: e.id(), class: "text".into(), anchor: [t.position[0], t.position[1], z + 1.0], lines, plan: t.view.clone() });
            }
        } else if let Some(d) = crate::notes::tag_data(e) {
            let Some(z) = levels.get(&d.view) else { continue };
            let r = crate::notes::resolve_tag(store, &reader, e.id(), d);
            let Some(host) = r.host.and_then(|h| store.get(h)) else { continue };
            let cat = crate::categories::category_of(host).map_or("Other", |c| c.name);
            let class = format!("tags:{cat}");
            if !tagged.insert((class.clone(), r.data.host.clone())) {
                continue;
            }
            let anchor = match crate::inserts::resolve(store, host) {
                Ok(p) => p.world([0.0, 0.0, 0.0]),
                Err(_) if crate::rooms::room_recipe(host).is_some() => [r.anchor[0], r.anchor[1], z + 0.3],
                Err(_) => [r.anchor[0], r.anchor[1], z + 1.5],
            };
            out.push(Tip { id: e.id(), class, anchor, lines: r.lines, plan: r.data.view.clone() });
        } else if let Some(d) = crate::annotations::dimension_data(e) {
            let Some(z) = levels.get(&d.view) else { continue };
            let Some(m) = crate::annotations::measure(store, &d) else { continue };
            let base = d.line[0] * m.across[0] + d.line[1] * m.across[1];
            for (k, v) in m.values.iter().enumerate() {
                let mid = (m.positions[k] + m.positions[k + 1]) / 2.0 - base;
                let p = [d.line[0] + m.across[0] * mid, d.line[1] + m.across[1] * mid];
                let text = crate::units::format(*v, "length", &settings, None).unwrap_or_else(|_| format!("{v:.3}"));
                out.push(Tip { id: e.id(), class: "dimensions".into(), anchor: [p[0], p[1], *z], lines: vec![text], plan: d.view.clone() });
            }
        }
    }
    out
}

/// The labels a 3D view shows.
pub fn shown(store: &Store, view: &Element) -> Vec<Tip> {
    let s = settings(view);
    if !s.shown {
        return Vec::new();
    }
    tips(store).into_iter().filter(|t| s.shows(&t.class)).collect()
}

/// One class in `view.annotations_3d`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ClassInfo {
    pub class: String,
    pub label: String,
    /// Labels of the class in the model.
    pub count: usize,
    pub visible: bool,
}

/// `view.annotations_3d`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Info {
    pub shown: bool,
    /// Labels behind the model are left out.
    pub occlude: bool,
    pub classes: Vec<ClassInfo>,
}

fn view_3d(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let (id, data) = crate::views::resolve_view(store, r)?;
    if data.kind != crate::views::KIND_3D {
        return Err(Error::new("view.not_3d", "annotation floats in 3D views; plans draw their own"));
    }
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {
    pub view_id: ElementRef,
}

/// The classes of the model's annotation and whether a 3D view shows each: the text notes, the
/// dimensions and each tagged category, always listed (with a count of 0 when there are none).
pub fn info(store: &Store, p: InfoParams) -> Result<Info> {
    let id = view_3d(store, &p.view_id)?;
    let s = settings(store.get(id).expect("resolved"));
    let mut counts: BTreeMap<String, usize> = ["text", "dimensions", "tags:Doors", "tags:Windows", "tags:Rooms"].iter().map(|c| ((*c).to_owned(), 0)).collect();
    for t in tips(store) {
        *counts.entry(t.class).or_default() += 1;
    }
    let order = |c: &str| match c {
        "text" => 0,
        "dimensions" => 1,
        _ => 2,
    };
    let mut classes: Vec<ClassInfo> = counts
        .into_iter()
        .map(|(class, count)| ClassInfo { label: label(&class), visible: !s.hidden.contains(&class), count, class })
        .collect();
    classes.sort_by(|a, b| (order(&a.class), &a.label).cmp(&(order(&b.class), &b.label)));
    Ok(Info { shown: s.shown, occlude: s.occlude, classes })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub view_id: ElementRef,
    /// Annotation on or off in the view.
    pub shown: Option<bool>,
    /// Leave out labels whose point is behind the model (on unless set).
    pub occlude: Option<bool>,
    /// Classes to show (`true`) or hide (`false`): `text`, `dimensions`, `tags:<category>`.
    #[serde(default)]
    pub classes: BTreeMap<String, bool>,
}

/// `view.set_3d_annotations`.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let id = view_3d(&store, &p.view_id)?;
    let mut s = settings(store.get(id).expect("resolved"));
    if let Some(v) = p.shown {
        s.shown = v;
    }
    if let Some(v) = p.occlude {
        s.occlude = v;
    }
    for (class, visible) in p.classes {
        let known = class == "text" || class == "dimensions" || class.strip_prefix("tags:").is_some_and(|c| !c.is_empty());
        if !known {
            return Err(Error::new("view.bad_class", format!("{class:?}: a class is text, dimensions or tags:<category>")));
        }
        if visible {
            s.hidden.remove(&class);
        } else {
            s.hidden.insert(class);
        }
    }
    let value = serde_json::to_value(&s).expect("settings serialise");
    txn.modify(id, |e| {
        e.params.insert(VIEW_KEY.into(), value);
    })
}
