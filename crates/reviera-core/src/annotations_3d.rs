// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Annotation in 3D views (DOC-ANN-025): the text notes, tags and dimensions of the model's plans,
//! sections and elevations, shown in a 3D view as floating labels at their place in the model,
//! drawn flat to the screen like tooltips (the window draws them, `reviera_render::tooltips`).
//!
//! - **Where:** a plan's annotation stands on its level: a text note 1 m above it, a door's or
//!   window's tag at the insert's centre, a wall's tag 1.5 m up its middle, a room's tag 0.3 m
//!   above its tag point, a dimension's values on its line at the level. A section's annotation
//!   stands on its view plane; an elevation's is moved back along its view onto the model (the
//!   window casts the ray, [`Tip::probe`]); their tags stand at the host. A host tagged in
//!   several views is labelled once.
//! - **From:** a view shows the plans' annotation, and the sections' and elevations' when chosen.
//! - **Classes:** each label has a class: `text`, `dimensions`, `spots` (spot elevations and
//!   coordinates, at the point they read), `keynotes` (key and text, at their element), or
//!   `tags:<category>` (Doors, Windows, Rooms, …), so a
//!   view can show some and not others.
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
    /// The GlobalId of the view the annotation is drawn in.
    pub view: String,
    /// The kind of that view: `plans`, `sections` or `elevations`.
    pub source: String,
    /// A tag's host (GlobalId): a host tagged in several views is labelled once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// In an elevation, which stands in front of what it shows, the way to the model: the label
    /// sits where `anchor + probe · t` (t in 0..1) first meets it, or at `anchor` when nothing is
    /// met. The window does this with its model (`radial::view_tips`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe: Option<[f64; 3]>,
}

/// The kinds of views annotation comes from, in order.
pub const SOURCES: [&str; 3] = ["plans", "sections", "elevations"];

/// How a 3D view shows annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Settings {
    pub shown: bool,
    /// Classes not shown.
    pub hidden: BTreeSet<String>,
    /// Labels whose point is behind the model are left out.
    #[serde(default = "yes")]
    pub occlude: bool,
    /// Kinds of views whose annotation is not shown (of [`SOURCES`]); sections' and elevations'
    /// unless set, so a 3D view shows the plans' annotation as it did before they had any.
    #[serde(default = "plans_only")]
    pub hidden_from: BTreeSet<String>,
}

fn yes() -> bool {
    true
}

fn plans_only() -> BTreeSet<String> {
    ["sections".to_owned(), "elevations".to_owned()].into()
}

impl Default for Settings {
    fn default() -> Self {
        Self { shown: true, hidden: ["dimensions".to_owned()].into(), occlude: true, hidden_from: plans_only() }
    }
}

impl Settings {
    pub fn shows(&self, class: &str) -> bool {
        self.shown && !self.hidden.contains(class)
    }

    /// Whether the view shows label `t`: annotation on, its class and its kind of view shown.
    pub fn shows_tip(&self, t: &Tip) -> bool {
        self.shows(&t.class) && !self.hidden_from.contains(&t.source)
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
        "spots" => "Spot elevations and coordinates".into(),
        "keynotes" => "Keynotes".into(),
        c => match c.strip_prefix("tags:") {
            Some(cat) => format!("{} tags", cat.strip_suffix('s').unwrap_or(cat)),
            None => c.to_owned(),
        },
    }
}

/// A kind of view's name for people.
pub fn source_label(source: &str) -> String {
    match source {
        "plans" => "Plans".into(),
        "sections" => "Sections".into(),
        "elevations" => "Elevations".into(),
        s => s.to_owned(),
    }
}

/// Where a view's annotation stands in the model.
enum Place {
    /// A plan, at its level's elevation.
    Plan(f64),
    /// A section or elevation (`true`), through its frame.
    Section(crate::sections::SectionFrame, bool),
}

impl Place {
    fn source(&self) -> &'static str {
        match self {
            Place::Plan(_) => "plans",
            Place::Section(_, false) => "sections",
            Place::Section(_, true) => "elevations",
        }
    }

    /// A point of the view's 2D coordinates in the model: in a plan `above` its level, in a
    /// section or elevation on its plane.
    fn world(&self, q: [f64; 2], above: f64) -> [f64; 3] {
        match self {
            Place::Plan(z) => [q[0], q[1], z + above],
            Place::Section(f, _) => f.to_world([q[0], q[1], 0.0]),
        }
    }

    /// An elevation's way to the model: along its view, as deep as it sees.
    fn probe(&self) -> Option<[f64; 3]> {
        match self {
            Place::Section(f, true) => Some([f.look[0] * f.far, f.look[1] * f.far, 0.0]),
            _ => None,
        }
    }
}

/// The annotation of the model's plans, sections and elevations as floating labels, every class
/// and kind of view (a tagged host once per kind of view).
pub fn tips(store: &Store) -> Vec<Tip> {
    // Each view's place, by its GlobalId.
    let mut places: BTreeMap<String, Place> = BTreeMap::new();
    for e in store.iter() {
        if let Some((kind, def)) = crate::sections::section_of(e) {
            places.insert(e.guid().to_string(), Place::Section(def.frame(), kind == crate::sections::KIND_ELEVATION));
        } else if let Some(d) = crate::views::view_data(e)
            && crate::plans::is_plan(&d.kind)
            && let Some(l) = &d.level
            && let Ok(z) = crate::levels::elevation_of(store, l)
        {
            places.insert(e.guid().to_string(), Place::Plan(z));
        }
    }
    let parents = std::collections::HashMap::new();
    let reader = crate::schedules::FieldReader::new(store, &parents);
    let settings = crate::units::settings_of(store);
    let mut out = Vec::new();
    let mut tagged: HashSet<(&'static str, String, String)> = HashSet::new();
    for e in store.iter() {
        if let Some(t) = crate::notes::text_data(e) {
            let Some(place) = places.get(&t.view) else { continue };
            let lines: Vec<String> = t.content.lines().map(str::to_owned).filter(|l| !l.trim().is_empty()).collect();
            if !lines.is_empty() {
                let anchor = place.world(t.position, 1.0);
                out.push(Tip { id: e.id(), class: "text".into(), anchor, lines, view: t.view, source: place.source().into(), host: None, probe: place.probe() });
            }
        } else if let Some(d) = crate::notes::tag_data(e) {
            let Some(place) = places.get(&d.view) else { continue };
            let r = crate::notes::resolve_tag(store, &reader, e.id(), d);
            let Some(host) = r.host.and_then(|h| store.get(h)) else { continue };
            let cat = crate::categories::category_of(host).map_or("Other", |c| c.name);
            let class = format!("tags:{cat}");
            if !tagged.insert((place.source(), class.clone(), r.data.host.clone())) {
                continue;
            }
            let anchor = match place {
                Place::Plan(z) => match crate::inserts::resolve(store, host) {
                    Ok(p) => p.world([0.0, 0.0, 0.0]),
                    Err(_) if crate::rooms::room_recipe(host).is_some() => [r.anchor[0], r.anchor[1], z + 0.3],
                    Err(_) => [r.anchor[0], r.anchor[1], z + 1.5],
                },
                // At the host itself: on the model already.
                Place::Section(..) => crate::notes::host_point(store, host).unwrap_or_else(|| place.world(r.anchor, 0.0)),
            };
            out.push(Tip { id: e.id(), class, anchor, lines: r.lines, view: r.data.view.clone(), source: place.source().into(), host: Some(r.data.host.clone()), probe: None });
        } else if let Some(d) = crate::keynote_tags::keynote_data(e) {
            // At its element, else where it stands.
            let Some(place) = places.get(&d.view) else { continue };
            let k = crate::keynote_tags::resolve(store, e.id(), d);
            let at_host = k.host.and_then(|h| store.get(h)).and_then(|h| match place {
                Place::Plan(z) => crate::notes::host_point(store, h).map(|p| match crate::inserts::resolve(store, h) {
                    Ok(_) => p,
                    Err(_) => [p[0], p[1], z + 1.5],
                }),
                Place::Section(..) => crate::notes::host_point(store, h),
            });
            let anchor = at_host.unwrap_or_else(|| place.world(k.data.position, 1.0));
            let lines = match (&k.key, &k.text) {
                (Some(key), Some(text)) => vec![format!("{key} {text}")],
                _ => k.lines.clone(),
            };
            out.push(Tip { id: e.id(), class: "keynotes".into(), anchor, lines, view: k.data.view.clone(), source: place.source().into(), host: None, probe: None });
        } else if let Some(d) = crate::spots::spot_data(e) {
            // At the point it reads, as stored (the window reads it again on the model).
            let Some(place) = places.get(&d.view) else { continue };
            let lines = crate::spots::lines(store, &d, d.point, None);
            out.push(Tip { id: e.id(), class: "spots".into(), anchor: d.point, lines, view: d.view.clone(), source: place.source().into(), host: None, probe: None });
        } else if let Some(d) = crate::annotations::dimension_data(e) {
            let Some(place) = places.get(&d.view) else { continue };
            let Some(m) = crate::annotations::measure(store, &d) else { continue };
            let base = d.line[0] * m.across[0] + d.line[1] * m.across[1];
            for (k, v) in m.values.iter().enumerate() {
                let mid = (m.positions[k] + m.positions[k + 1]) / 2.0 - base;
                let p = [d.line[0] + m.across[0] * mid, d.line[1] + m.across[1] * mid];
                let text = crate::units::format(*v, "length", &settings, None).unwrap_or_else(|_| format!("{v:.3}"));
                out.push(Tip {
                    id: e.id(),
                    class: "dimensions".into(),
                    anchor: place.world(p, 0.0),
                    lines: vec![text],
                    view: d.view.clone(),
                    source: place.source().into(),
                    host: None,
                    probe: place.probe(),
                });
            }
        }
    }
    out
}

/// The labels `s` shows of `tips`: their class and kind of view shown, a host tagged in several
/// kinds of views labelled once (from the first shown: plans, then sections, then elevations).
pub fn visible(tips: Vec<Tip>, s: &Settings) -> Vec<Tip> {
    let rank = |t: &Tip| SOURCES.iter().position(|x| *x == t.source).unwrap_or(SOURCES.len());
    let mut shown: Vec<Tip> = tips.into_iter().filter(|t| s.shows_tip(t)).collect();
    shown.sort_by_key(rank);
    let mut seen: HashSet<(String, String)> = HashSet::new();
    shown.retain(|t| t.host.as_ref().is_none_or(|h| seen.insert((t.class.clone(), h.clone()))));
    shown
}

/// The labels a 3D view shows.
pub fn shown(store: &Store, view: &Element) -> Vec<Tip> {
    let s = settings(view);
    if !s.shown {
        return Vec::new();
    }
    visible(tips(store), &s)
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

/// One kind of view in `view.annotations_3d`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SourceInfo {
    /// `plans`, `sections` or `elevations`.
    pub source: String,
    pub label: String,
    /// Labels from views of the kind.
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
    /// The kinds of views annotation comes from, and whether each is shown.
    pub sources: Vec<SourceInfo>,
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
    let mut counts: BTreeMap<String, usize> = ["text", "dimensions", "spots", "keynotes", "tags:Doors", "tags:Windows", "tags:Rooms"].iter().map(|c| ((*c).to_owned(), 0)).collect();
    let mut from: BTreeMap<String, usize> = BTreeMap::new();
    for t in tips(store) {
        *from.entry(t.source).or_default() += 1;
        *counts.entry(t.class).or_default() += 1;
    }
    let sources = SOURCES
        .iter()
        .map(|src| SourceInfo { source: (*src).to_owned(), label: source_label(src), count: from.get(*src).copied().unwrap_or(0), visible: !s.hidden_from.contains(*src) })
        .collect();
    let order = |c: &str| match c {
        "text" => 0,
        "dimensions" => 1,
        "spots" => 2,
        "keynotes" => 3,
        _ => 4,
    };
    let mut classes: Vec<ClassInfo> = counts
        .into_iter()
        .map(|(class, count)| ClassInfo { label: label(&class), visible: !s.hidden.contains(&class), count, class })
        .collect();
    classes.sort_by(|a, b| (order(&a.class), &a.label).cmp(&(order(&b.class), &b.label)));
    Ok(Info { shown: s.shown, occlude: s.occlude, classes, sources })
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
    /// Kinds of views whose annotation to show (`true`) or hide: `plans`, `sections`,
    /// `elevations`.
    #[serde(default)]
    pub sources: BTreeMap<String, bool>,
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
        let known = ["text", "dimensions", "spots", "keynotes"].contains(&class.as_str()) || class.strip_prefix("tags:").is_some_and(|c| !c.is_empty());
        if !known {
            return Err(Error::new("view.bad_class", format!("{class:?}: a class is text, dimensions, spots, keynotes or tags:<category>")));
        }
        if visible {
            s.hidden.remove(&class);
        } else {
            s.hidden.insert(class);
        }
    }
    for (source, visible) in p.sources {
        if !SOURCES.contains(&source.as_str()) {
            return Err(Error::new("view.bad_source", format!("{source:?}: a kind of view is plans, sections or elevations")));
        }
        if visible {
            s.hidden_from.remove(&source);
        } else {
            s.hidden_from.insert(source);
        }
    }
    let value = serde_json::to_value(&s).expect("settings serialise");
    txn.modify(id, |e| {
        e.params.insert(VIEW_KEY.into(), value);
    })
}
