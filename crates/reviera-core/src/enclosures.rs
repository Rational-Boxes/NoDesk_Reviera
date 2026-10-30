// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Glazed enclosures (spec Ch 3 §3.6a, ARC-GLZ-001/002): one element that lays out a glazed
//! building from a form and a few parameters, made of Reviera's ordinary elements, and builds
//! them again when a parameter changes.
//!
//! Forms built so far:
//! - as rows of gable spans side by side along the enclosure's length:
//!   - **Gable**: one span;
//!   - **Multispan** (gutter-connected): `spans` spans sharing valley gutters;
//!   - **Venlo**: `trusses` trusses of `peaks` narrow roofs each;
//!   - **Sawtooth**: `spans` single-slope roofs rising across the span to a glazed vertical face
//!     at the high side, its top band vents (`vents` percent of the span's floor), the next
//!     span's gutter at its foot;
//! - as tunnels, one roof by extrusion (ARC-ROOF-002) whose profile is the arch in 16 straight
//!   facets, glazed with a film-like Sloped Glazing type whose bars stand for the hoops, on
//!   sidewalls `eave` high (0: ground to ground), without gutters, vents or posts:
//!   - **Hoop** (Quonset): a semicircle across the span;
//!   - **Gothic**: two arcs of radius `arc` × the span meeting in a pointed crown.
//!
//! Parts:
//! - each span's roof: a footprint roof of a Sloped Glazing type (ARC-CW-008), its two long
//!   sides sloping at `pitch` from the eave (gutter) height, gable ends at the enclosure's ends.
//!   At a valley each roof stops half a gutter's width short, the valley gutter between them;
//!   the outer eaves reach past the side walls' glass so their gutters hang clear;
//! - glazed side walls and end walls: curtain walls, the ends attached to every span's roof so
//!   their gables fill (`curtain` attached tops), dropping under the valley gutters;
//! - knee walls under the glazing when `knee` is set;
//! - a door in the middle of each end wall (`doors`): the panes round its middle merged into one
//!   curtain-wall door at least 1.5 m wide and 2.1 m high (EXPERT_REVIEW E-50); with a knee
//!   wall, the end walls are glazed to the ground so the doors stand on the floor;
//! - gutters, laid level, on the outer eaves and in each valley (MEP-PIPE-010);
//! - a frame (`frame`): structural steel posts of square hollow section at every bay on each
//!   gutter line, the outer ones just inside the glass and knee walls, the valley ones under the
//!   valley gutters (on a Venlo only the truss lines), and on each line a gutter beam of the
//!   same section along the posts' tops, carrying the gutter just above it (EXPERT_REVIEW E-51);
//! - ridge vents (ARC-GLZ-004): panes of each roof's top row swapped for vents (top-hung,
//!   drawn open), alternating side to side along the ridge and spread evenly, as many as reach
//!   `vents` percent of the roof's floor area (default 18 %, EXPERT_REVIEW E-47).
//!
//! In the enclosure's own frame `x` runs along its length (the ridges) from `origin`, turned by
//! `rotation` from the project's x axis, and `y` across the spans. The enclosure is an
//! `IfcElementAssembly` (ObjectType `GlazedEnclosure`) whose recipe lists its parts; each part
//! names its enclosure (`rxd:enclosure`) and goes with it.
//!
//! The glazing's bars come from the frame system (`frame_system`, `crate::frame_library`); the
//! enclosure reports the share of the roof they shade, and warns when the roof is flatter than
//! its glazing allows or than the condensate guideline.
//!
//! A **headhouse** (`headhouse`): a solid-walled workroom across one end (walls of the default
//! type on its three outer sides, a flat roof, a door in its front wall), the range's end-wall
//! door on that side widened for equipment (E-56).
//!
//! Not yet: rafters, purlins and Venlo trusses; side
//! vents and roll-up sides; vents as their own `IfcWindow` in a glazed roof's IFC (the roof's
//! glazing is one body there); keeping a user's edits to a part when the enclosure regenerates; a headhouse.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::curtain::{CurtainType, Layout, Mullion, Rule};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::roofs::{self, RoofCurveArg};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::{self, Line, RECIPE_KEY};

pub const CLASS: &str = "IfcElementAssembly";
pub const OBJECT_TYPE: &str = "GlazedEnclosure";
pub const RECIPE: &str = "recipe.enclosure";
/// The parameter by which a part names its enclosure (GlobalId).
pub const PART_KEY: &str = "rxd:enclosure";
/// The glazing types the enclosure makes on first use (provisional, EXPERT_REVIEW E-46).
pub const ROOF_GLAZING: &str = "Greenhouse Roof Glass";
pub const WALL_GLAZING: &str = "Greenhouse Wall Glass";
pub const TUNNEL_FILM: &str = "Greenhouse Tunnel Film";
/// The facets of a tunnel's arch.
const ARCH_FACETS: usize = 16;
/// The glazing bars of both glass types, and the tunnel film's hoops (the frame library's
/// defaults, ARC-GLZ-003).
const ROOF_BAR: Mullion = Mullion { width: 0.04, depth: 0.08 };
const WALL_BAR: Mullion = Mullion { width: 0.04, depth: 0.08 };
const HOOP: Mullion = Mullion { width: 0.05, depth: 0.05 };
/// How far below the eave a gutter's bottom hangs: its top 20 mm under the roof and its depth
/// (MEP-PIPE-010's default, half its 125 mm width), laid level (EXPERT_REVIEW E-49).
const GUTTER_DROP: f64 = 0.02 + 0.0625;
/// The gutter's width: the gap between two spans' roofs at a valley, the gutter in it.
const GUTTER_WIDTH: f64 = 0.125;
/// The posts' sections: light for a span under 6 m, heavier for commercial ranges
/// (provisional, EXPERT_REVIEW E-51).
const POST_LIGHT: &str = "HSS3X3X1/4";
const POST_HEAVY: &str = "HSS4X4X1/4";
/// The least clear width and height of an end wall's door (provisional, EXPERT_REVIEW E-50).
const DOOR_WIDTH: f64 = 1.5;
/// An end wall's door into a headhouse: wide enough for equipment (research/11 §B7, E-56).
const HEADHOUSE_DOOR: f64 = 3.0;
const DOOR_HEIGHT: f64 = 2.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    Gable,
    Multispan,
    Venlo,
    Hoop,
    Gothic,
    Sawtooth,
}

impl Form {
    /// A tunnel: one arched roof by extrusion.
    pub fn tunnel(self) -> bool {
        matches!(self, Form::Hoop | Form::Gothic)
    }
}

/// The stored recipe of a glazed enclosure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EnclosureRecipe {
    pub kind: String,
    pub form: Form,
    /// The level (GlobalId).
    pub level: String,
    /// The corner the enclosure is laid out from (plan, metres), and the direction of its length.
    pub origin: [f64; 2],
    #[serde(default)]
    pub rotation: f64,
    /// Along the ridges.
    pub length: f64,
    /// Each roof's span across, valley to valley (a Venlo truss is `peaks` of them).
    pub span: f64,
    /// How many spans side by side (Venlo: trusses × peaks).
    pub spans: u32,
    /// Venlo: roofs per truss.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peaks: Option<u32>,
    /// The spacing of the bays along the length (frame and wall glazing).
    pub bay: f64,
    /// The eave (gutter) height above the level.
    pub eave: f64,
    /// The roof slope (radians).
    pub pitch: f64,
    /// A knee wall's height under the glazing, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knee: Option<f64>,
    #[serde(default = "yes")]
    pub gutters: bool,
    /// A door in each end wall.
    #[serde(default = "yes")]
    pub doors: bool,
    /// Ridge vents' target area, percent of the floor area (0 for none); a sawtooth's face vents.
    #[serde(default = "vent_percent")]
    pub vents: f64,
    /// Side vents along the side walls' tops, percent of the floor area (0 for none).
    #[serde(default)]
    pub side_vents: f64,
    /// Roll-up sides: the opening's height up each side wall from its foot (0 for none).
    #[serde(default)]
    pub roll_up: f64,
    /// Gable vents: a band of vents this deep under the top of each end wall (0 for none).
    #[serde(default)]
    pub gable_vents: f64,
    /// How the vents open (ARC-GLZ-004).
    #[serde(default)]
    pub vent: crate::vents::VentSettings,
    /// A headhouse across one end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headhouse: Option<Headhouse>,
    /// Steel posts at the bays.
    #[serde(default = "yes")]
    pub frame: bool,
    /// Gothic: each arc's radius as a share of the span.
    #[serde(default = "gothic_arc")]
    pub arc: f64,
    /// The roof's and the walls' glazing: library entries (`glazing.list_types`); the
    /// greenhouse glass (a tunnel's film) when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roof_glazing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_glazing: Option<String>,
    /// The glazing bars' frame system: a library entry (`frame.list_types`); aluminium 40 × 80 mm
    /// bars (a tunnel's 50 mm steel hoops) when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_system: Option<String>,
    /// Energy, shade and blackout screens under the gutters (ARC-GLZ-010).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screens: Vec<crate::hothouse::Screen>,
    /// The parts (GlobalIds).
    #[serde(default)]
    pub parts: Vec<String>,
}

fn yes() -> bool {
    true
}

/// Which end of the range a headhouse stands at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// At the enclosure's origin.
    #[default]
    Start,
    End,
}

/// A headhouse: how deep it is along the range, how high its walls are, and which end.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Headhouse {
    pub depth: f64,
    pub height: f64,
    #[serde(default)]
    pub at: End,
}

impl EnclosureRecipe {
    /// A headhouse's defaults: 10 % of the growing floor (at least 4 m deep) and the eave height
    /// (2.7 to 4 m; 3 m on a tunnel) (provisional, E-56).
    pub fn default_headhouse(&self) -> Headhouse {
        let height = if self.form.tunnel() { 3.0 } else { self.eave.clamp(2.7, 4.0) };
        Headhouse { depth: (0.1 * self.length).max(4.0), height, at: End::Start }
    }
}

fn vent_percent() -> f64 {
    18.0
}

fn gothic_arc() -> f64 {
    0.6
}

/// Ridge vents in a glazed roof: panes of the top row of each sloping plane swapped for vents,
/// alternating plane by plane along the ridge, spread evenly, until their area reaches `target`.
fn ridge_vents(txn: &mut Transaction<'_>, roof: ElementId, target: f64) -> Result<()> {
    let store = txn.store().clone();
    let e = store.get(roof).expect("made");
    let Some(glazing) = roofs::glazing::of(&store, e) else { return Ok(()) };
    let planes: Vec<(u32, usize, usize)> = glazing
        .planes
        .iter()
        .filter_map(|p| Some((p.key?, p.grid2.len() + 1, p.grid1.len() + 1)))
        .collect();
    if planes.is_empty() || target <= 0.0 {
        return Ok(());
    }
    // The panes' area, plane by plane: the glazed area shared among its cells.
    let g = roofs::regenerate(&store, e)?;
    let mut sizes = Vec::new();
    for (key, cols, rows) in &planes {
        let area: f64 = g.faces.iter().filter(|f| f.key == Some(*key)).map(|f| {
            let ring: Vec<[f64; 2]> = f.bottom.iter().map(|q| [q[0], q[1]]).collect();
            crate::sketch::signed_area(&ring).abs() / f.normal[2].abs().max(0.1)
        }).sum();
        sizes.push(area / (*cols * *rows) as f64);
    }
    let pane = sizes.iter().copied().fold(f64::MAX, f64::min).max(1e-6);
    let most: usize = planes.iter().map(|p| p.1).sum();
    let count = ((target / pane).ceil() as usize).clamp(1, most);
    let mut panes = Vec::new();
    for (k, (key, cols, rows)) in planes.iter().enumerate() {
        // This plane's share, alternating so the vents stagger along the ridge.
        let n = count / planes.len() + usize::from(k < count % planes.len());
        let n = n.min(*cols);
        for m in 0..n {
            let shift = if k % 2 == 0 { 0.25 } else { 0.75 };
            let c = (((m as f64 + shift) * *cols as f64 / n as f64) as usize).min(cols - 1);
            panes.push(roofs::glazing::PaneOverride { plane: *key, cell: [c, rows - 1], panel: crate::curtain::Panel::Vent });
        }
    }
    let mut r = roofs::roof_recipe(e).expect("a roof");
    r.panes = panes;
    let value = serde_json::to_value(&r).expect("serialises");
    txn.modify(roof, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

/// The area of a curtain wall's vent panes.
pub fn wall_vent_area(store: &Store, wall: &Element) -> f64 {
    crate::curtain::regenerate(store, wall).map_or(0.0, |g| {
        g.parts.iter().filter(|p| !p.mullion && matches!(p.panel, crate::curtain::Panel::Vent | crate::curtain::Panel::RollUp)).map(|p| (p.x[1] - p.x[0]) * (p.z[1] - p.z[0])).sum()
    })
}

/// The area of a roof's vent panes.
pub fn vent_area(store: &Store, roof: &Element) -> f64 {
    roofs::glazing::of(store, roof).map_or(0.0, |g| {
        g.parts
            .iter()
            .filter(|p| !p.mullion && p.panel == crate::curtain::Panel::Vent)
            .map(|p| {
                let (pts, idx) = &p.mesh;
                // Both faces of a thin pane: half the surface.
                idx.chunks(3)
                    .map(|t| {
                        let [a, b, c] = [pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]];
                        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                        (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() / 2.0
                    })
                    .sum::<f64>()
                    / 2.0
            })
            .sum()
    })
}

impl EnclosureRecipe {
    pub fn width(&self) -> f64 {
        self.span * f64::from(self.spans)
    }

    /// Where a sawtooth span's roof starts across (its low eave): the next span's from the
    /// vertical face before it, a gap, then its gutter.
    fn low_side(&self, i: u32) -> f64 {
        let y = self.span * f64::from(i);
        if i == 0 { y } else { y + self.face_gap() + GUTTER_WIDTH }
    }

    /// The roof's glazing bars (and, on a frame system, the walls'), from the frame system.
    pub fn bar(&self) -> Mullion {
        match self.frame_system.as_deref().and_then(crate::frame_library::entry) {
            Some(e) => e.frame.bar(),
            None if self.form.tunnel() => HOOP,
            None => WALL_BAR,
        }
    }

    /// A sawtooth: the clearance between a vertical face's glass and the gutter at its foot, and
    /// at the end walls.
    fn face_gap(&self) -> f64 {
        let wall = if self.frame_system.is_some() { self.bar() } else { WALL_BAR };
        wall.depth / 2.0 + 0.005
    }

    /// The share of the roof the glazing bars shade, seen square to it: bars on the glazing
    /// grid's lines (across the slope `along` apart, up it `up` apart, 0 for none).
    pub fn frame_shading(&self) -> f64 {
        let w = self.bar().width;
        let (along, up) = if self.form.tunnel() { (1.5, 0.0) } else { (1.125, 2.0) };
        let open = |s: f64| if s > 0.0 { (1.0 - w / s).max(0.0) } else { 1.0 };
        1.0 - open(along) * open(up)
    }

    /// The inner gutter lines across (where the inner posts stand): the valleys, a Venlo's truss
    /// lines, a sawtooth's gutters at the foot of each vertical face.
    pub fn gutter_lines(&self) -> Vec<f64> {
        if self.form.tunnel() {
            return Vec::new();
        }
        let every = if self.form == Form::Venlo { self.peaks.unwrap_or(2).max(1) } else { 1 };
        (1..self.spans)
            .filter(|i| i % every == 0)
            .map(|i| if self.form == Form::Sawtooth { self.low_side(i) - GUTTER_WIDTH / 2.0 } else { self.span * f64::from(i) })
            .collect()
    }

    /// A point of the enclosure's frame in the project's plan.
    pub fn at(&self, x: f64, y: f64) -> [f64; 2] {
        let (s, c) = self.rotation.sin_cos();
        [self.origin[0] + c * x - s * y, self.origin[1] + s * x + c * y]
    }

    /// The highest ridge (a tunnel's crown) above the level: an outer span's, which loses half
    /// a gutter's width to its one valley (the inner ones lose a whole gutter's).
    pub fn ridge(&self) -> f64 {
        match self.form {
            Form::Hoop => self.eave + self.span / 2.0,
            Form::Gothic => {
                let rr = self.arc * self.span;
                self.eave + (rr * rr - (rr - self.span / 2.0).powi(2)).sqrt()
            }
            // The first span's high side, which has no gutter at its foot.
            Form::Sawtooth => self.eave + self.span * self.pitch.tan(),
            _ => {
                let width = if self.spans > 1 { self.span - GUTTER_WIDTH / 2.0 } else { self.span };
                self.eave + width / 2.0 * self.pitch.tan()
            }
        }
    }

    /// A tunnel's arch across the span: points (across, up from the level) at its facets' ends.
    pub fn arch(&self) -> Vec<[f64; 2]> {
        let (w, h) = (self.span, self.eave);
        let n = ARCH_FACETS;
        match self.form {
            Form::Gothic => {
                // The left arc about (R, h) from the ground up to the crown, then its mirror.
                let rr = self.arc * w;
                let top = ((w / 2.0 - rr) / rr).clamp(-1.0, 1.0).acos();
                let half: Vec<[f64; 2]> = (0..=n / 2)
                    .map(|k| {
                        let a = std::f64::consts::PI + (top - std::f64::consts::PI) * k as f64 / (n / 2) as f64;
                        [rr + rr * a.cos(), h + rr * a.sin()]
                    })
                    .collect();
                let mut pts = half.clone();
                pts.extend(half.iter().rev().skip(1).map(|p| [w - p[0], p[1]]));
                pts
            }
            _ => (0..=n)
                .map(|k| {
                    let a = std::f64::consts::PI * (1.0 - k as f64 / n as f64);
                    [w / 2.0 + w / 2.0 * a.cos(), h + w / 2.0 * a.sin()]
                })
                .collect(),
        }
    }

    fn check(&self) -> Result<()> {
        let bad = |m: &str| Err(Error::new("enclosure.bad_value", m.to_owned()));
        if !(self.length >= 1.0 && self.span >= 1.0 && self.spans >= 1 && self.spans <= 200) {
            return bad("an enclosure needs a length and span of at least 1 m, and 1 to 200 spans");
        }
        if !(self.bay >= 0.3 && self.bay <= self.length) {
            return bad("the bay spacing must be at least 300 mm and no longer than the enclosure");
        }
        if self.form.tunnel() {
            if !(0.0..=10.0).contains(&self.eave) {
                return bad("a tunnel's sidewalls are 0 to 10 m high");
            }
            if self.spans != 1 {
                return bad("a tunnel has one span");
            }
            if self.form == Form::Gothic && !(0.5..=2.0).contains(&self.arc) {
                return bad("a gothic arch's radius is 0.5 to 2 times the span");
            }
        } else if !(self.eave >= 0.5 && self.eave <= 30.0) {
            return bad("the eave height must be between 0.5 and 30 m");
        }
        if !self.form.tunnel() && !(self.pitch >= 1f64.to_radians() && self.pitch <= 60f64.to_radians()) {
            return bad("the roof pitch must be between 1° and 60°");
        }
        if self.knee.is_some_and(|k| !(k > 0.0 && (k < self.eave - 0.3 || self.form.tunnel() && k < self.ridge() - 1.0))) {
            return bad("a knee wall must be lower than the eave by at least 300 mm");
        }
        if self.form == Form::Gable && self.spans != 1 {
            return bad("a gable enclosure has one span; use the multispan form for more");
        }
        Ok(())
    }
}

pub fn recipe(e: &Element) -> Option<EnclosureRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

/// The enclosure a part belongs to (GlobalId).
pub fn enclosure_of(e: &Element) -> Option<&str> {
    e.param(PART_KEY)?.as_str()
}

/// The named type, made by `make` on first use with the glazing values of library entry
/// `glazing` (ARC-GLZ-003).
fn type_named(txn: &mut Transaction<'_>, name: &str, is: fn(&Element) -> bool, glazing: &str, make: impl FnOnce() -> NewElement) -> Result<ElementId> {
    if let Some(e) = txn.store().iter().find(|e| e.name.as_deref() == Some(name) && is(e)) {
        return Ok(e.id());
    }
    let id = txn.create(make())?;
    if let Some(entry) = crate::glazing_library::entry(glazing) {
        crate::glazing_library::write(txn, id, &entry.glazing)?;
    }
    // The bars' frame system: the film's steel hoops, the glass's aluminium bars.
    let frame = if name == TUNNEL_FILM { crate::frame_library::TUNNEL_DEFAULT } else { crate::frame_library::DEFAULT };
    if let Some(entry) = crate::frame_library::entry(frame) {
        crate::frame_library::write(txn, id, &entry.frame)?;
    }
    Ok(id)
}

fn roof_glazing(txn: &mut Transaction<'_>) -> Result<ElementId> {
    type_named(
        txn,
        ROOF_GLAZING,
        |e| roofs::glazing::of_type(e).is_some(),
        "Horticultural Float 4 mm",
        || {
            // Panes 1,125 mm wide up the slope, one per 2.0 m of slope, on 40 × 80 mm bars.
            let ty = CurtainType {
                u: Rule { layout: Layout::MaximumSpacing, spacing: 1.125, number: 0 },
                v: Rule { layout: Layout::MaximumSpacing, spacing: 2.0, number: 0 },
                interior: Some(ROOF_BAR),
                border: Some(ROOF_BAR),
                panel_thickness: 0.004,
                ..CurtainType::default()
            };
            NewElement::new(roofs::TYPE_CLASS).name(ROOF_GLAZING).param("@PredefinedType", "NOTDEFINED").param(
                RECIPE_KEY,
                json!({ "kind": roofs::ROOF_TYPE_RECIPE, "thickness": ty.panel_thickness, "glazing": serde_json::to_value(&ty).expect("serialises") }),
            )
        },
    )
}

fn tunnel_film(txn: &mut Transaction<'_>) -> Result<ElementId> {
    type_named(
        txn,
        TUNNEL_FILM,
        |e| roofs::glazing::of_type(e).is_some(),
        "Double Inflated Polyethylene",
        || {
            // Film on hoops at most 1.5 m apart (50 × 50 mm bars stand for them), no purlins.
            let hoop = HOOP;
            let ty = CurtainType {
                u: Rule { layout: Layout::MaximumSpacing, spacing: 1.5, number: 0 },
                v: Rule { layout: Layout::None, spacing: 0.0, number: 0 },
                interior: Some(hoop),
                border: Some(hoop),
                panel_thickness: 0.002,
                ..CurtainType::default()
            };
            NewElement::new(roofs::TYPE_CLASS).name(TUNNEL_FILM).param("@PredefinedType", "NOTDEFINED").param(
                RECIPE_KEY,
                json!({ "kind": roofs::ROOF_TYPE_RECIPE, "thickness": ty.panel_thickness, "glazing": serde_json::to_value(&ty).expect("serialises") }),
            )
        },
    )
}

fn wall_glazing(txn: &mut Transaction<'_>) -> Result<ElementId> {
    type_named(
        txn,
        WALL_GLAZING,
        |e| crate::curtain::curtain_type(e).is_some(),
        "Horticultural Float 4 mm",
        || {
            // Panes 800 mm wide, a transom every 1.5 m, on 40 × 80 mm bars.
            let ty = CurtainType {
                u: Rule { layout: Layout::MaximumSpacing, spacing: 0.8, number: 0 },
                v: Rule { layout: Layout::MaximumSpacing, spacing: 1.5, number: 0 },
                interior: Some(WALL_BAR),
                border: Some(WALL_BAR),
                panel_thickness: 0.004,
                ..CurtainType::default()
            };
            NewElement::new("IfcCurtainWallType")
                .name(WALL_GLAZING)
                .param("@PredefinedType", "NOTDEFINED")
                .param(RECIPE_KEY, serde_json::to_value(ty).expect("serialises"))
        },
    )
}

/// Builds an enclosure's parts from its recipe.
fn build(txn: &mut Transaction<'_>, guid: &str, r: &EnclosureRecipe) -> Result<Vec<ElementId>> {
    r.check()?;
    let store = txn.store().clone();
    let level = GlobalId::parse(&r.level)
        .and_then(|g| store.find_by_guid(&g))
        .ok_or_else(|| Error::new("enclosure.no_level", "the enclosure's level no longer exists"))?;
    use crate::glazing_library::{Use, type_for};
    // On a frame system the glazing types are made with its bars (the default glazing's when
    // none is chosen).
    let bar = r.bar();
    let frame = r.frame_system.as_deref();
    let wall_bar = if frame.is_some() { bar } else { WALL_BAR };
    let (roof_default, wall_default) = (if r.form.tunnel() { "Double Inflated Polyethylene" } else { "Horticultural Float 4 mm" }, "Horticultural Float 4 mm");
    let grid = if r.form.tunnel() { [1.5, 0.0] } else { [1.125, 2.0] };
    let roof_type = match (&r.roof_glazing, frame, r.form.tunnel()) {
        (Some(g), _, _) => type_for(txn, g, Use::Roof, grid, bar, frame)?,
        (None, Some(_), _) => type_for(txn, roof_default, Use::Roof, grid, bar, frame)?,
        (None, None, true) => tunnel_film(txn)?,
        (None, None, false) => roof_glazing(txn)?,
    };
    let wall_type = match (&r.wall_glazing, frame) {
        (Some(g), _) => type_for(txn, g, Use::Wall, [0.8, 1.5], wall_bar, frame)?,
        (None, Some(_)) => type_for(txn, wall_default, Use::Wall, [0.8, 1.5], wall_bar, frame)?,
        (None, None) => wall_glazing(txn)?,
    };
    let (l, w) = (r.length, r.width());
    let mut parts = Vec::new();
    let mut roofs_made = Vec::new();
    // A tunnel: its arch extruded along the length, on a plane at the enclosure's start.
    if r.form.tunnel() {
        let o = r.at(0.0, 0.0);
        let (s, c) = r.rotation.sin_cos();
        let arch = r.arch();
        let profile = arch
            .windows(2)
            .map(|p| walls::CurveArg { line: Line { start: p[0], end: p[1] }, through: Vec::new() })
            .collect();
        let level_z = crate::levels::elevation_of(&store, &r.level).unwrap_or(0.0);
        roofs_made.push(roofs::extrusion::create(
            txn,
            roofs::extrusion::CreateParams {
                type_id: Some(ElementRef::Id(roof_type)),
                plane: crate::work_planes::PlaneArgs {
                    plane: Some(crate::work_planes::ExplicitPlane { origin: [o[0], o[1], level_z], x_dir: [-s, c, 0.0], normal: [c, s, 0.0] }),
                    ..Default::default()
                },
                profile,
                ref_level: Some(ElementRef::Id(level)),
                level_offset: 0.0,
                start: Some(0.0),
                end: Some(l),
            },
        )?);
    }
    // The roofs: one gable span each, its eaves along the length (a sawtooth's one slope rising
    // to the high side).
    let saw = r.form == Form::Sawtooth;
    for i in (0..r.spans).filter(|_| !r.form.tunnel()) {
        // At a valley each roof stops half a gutter short, the valley gutter between them; a
        // sawtooth's starts past the gutter at the foot of the face before it.
        let half = GUTTER_WIDTH / 2.0;
        let (y0, y1) = if saw {
            (r.low_side(i), r.span * f64::from(i + 1))
        } else {
            (r.span * f64::from(i) + if i > 0 { half } else { 0.0 }, r.span * f64::from(i + 1) - if i + 1 < r.spans { half } else { 0.0 })
        };
        let corners = [r.at(0.0, y0), r.at(l, y0), r.at(l, y1), r.at(0.0, y1)];
        let curves = (0..4)
            .map(|k| RoofCurveArg {
                line: Line { start: corners[k], end: corners[(k + 1) % 4] },
                lock: None,
                defines_slope: Some(if saw { k == 0 } else { k % 2 == 0 }),
                slope: None,
                // The outer eaves reach past the side walls' glass so the gutters hang clear, and
                // a sawtooth's high side over its vertical face.
                overhang: Some(if (k == 0 && i == 0) || (k == 2 && (saw || i + 1 == r.spans)) { wall_bar.depth / 2.0 } else { 0.0 }.into()),
                eave: None,
            })
            .collect();
        let roof = roofs::create(
            txn,
            roofs::CreateParams {
                type_id: Some(ElementRef::Id(roof_type)),
                curves,
                level_id: ElementRef::Id(level),
                base_offset: Some(r.eave.into()),
                slope: Some(r.pitch.into()),
                cutoff_level_id: None,
                cutoff_offset: None,
            },
        )?;
        // The share of the floor under this span, valley gutters included (a sawtooth's vents
        // are in its vertical faces).
        if !saw {
            ridge_vents(txn, roof, r.vents / 100.0 * r.span * l)?;
        }
        roofs_made.push(roof);
    }
    parts.extend(&roofs_made);
    // Knee walls round the outside, and the glazing above them.
    let base = r.knee.unwrap_or(0.0);
    let perimeter = [
        (r.at(0.0, 0.0), r.at(l, 0.0)),
        (r.at(l, 0.0), r.at(l, w)),
        (r.at(l, w), r.at(0.0, w)),
        (r.at(0.0, w), r.at(0.0, 0.0)),
    ];
    // The end walls are glazed to the ground where they have doors, between the side knee walls.
    let base_of = |k: usize| if k % 2 == 1 && r.doors { 0.0 } else { base };
    let mut knee_half = 0.0;
    if let Some(k) = r.knee {
        let wt = walls::default_type(txn)?;
        knee_half = txn.store().get(wt).and_then(walls::type_width).unwrap_or(0.2) / 2.0;
        for (side, (a, b)) in perimeter.into_iter().enumerate() {
            if base_of(side) == 0.0 {
                continue;
            }
            parts.push(walls::create(
                txn,
                walls::CreateParams {
                    type_id: Some(ElementRef::Id(wt)),
                    curve: walls::CurveArg { line: Line { start: a, end: b }, through: Vec::new() },
                    base_level: ElementRef::Id(level),
                    top_constraint: walls::TopConstraint::Unconnected(k),
                    base_offset: 0.0,
                    top_offset: 0.0,
                    location_line: walls::LocationLine::WallCenterline,
                    flip: false,
                    auto_join: Default::default(),
                    structural: false,
                    structural_usage: None,
                },
            )?);
        }
    }
    // The end walls stand between the side walls' corner posts and rise into the gables,
    // dropping under the valley gutters between the roofs; the side walls stop under the roof's
    // eave bars.
    let inset = if r.doors { (wall_bar.depth / 2.0).max(knee_half) } else { wall_bar.depth / 2.0 };
    let under_bars = bar.depth / 2.0 / r.pitch.cos();
    // A sawtooth's vertical face under span `i`'s high side: its top (absolute) clear of the
    // roof's bars over the face's glass, and its vents (`vents` percent of the span's floor).
    let face_top = |i: u32| r.eave + (r.span * f64::from(i + 1) - r.low_side(i) - r.face_gap()) * r.pitch.tan() - under_bars - 0.005;
    let vent_band = r.vents / 100.0 * r.span;
    // Side vents: the target shared by the side walls (a sawtooth's one low side; its high side
    // is a vent face), each band along a wall's top.
    let side_walls = if saw { 1.0 } else { 2.0 };
    // The band is sized so its panes, between the bars, make the target.
    let panes = 1.0 - wall_bar.width / 0.8;
    let side_band = if r.side_vents > 0.0 { r.side_vents / 100.0 * w / side_walls / panes + wall_bar.width } else { 0.0 };
    for (k, (a, b)) in perimeter.into_iter().enumerate() {
        let base = base_of(k);
        // A tunnel's sidewalls only when it has them.
        if k % 2 == 0 && r.form.tunnel() && r.eave - base < 0.3 {
            continue;
        }
        let (a, b, top) = if k % 2 == 1 {
            let d = [(b[0] - a[0]) / w, (b[1] - a[1]) / w];
            let own = if r.form.tunnel() { r.ridge() } else if r.spans > 1 { r.eave - GUTTER_DROP } else { r.eave };
            ([a[0] + d[0] * inset, a[1] + d[1] * inset], [b[0] - d[0] * inset, b[1] - d[1] * inset], own - base)
        } else if saw && k == 2 {
            (a, b, face_top(r.spans - 1) - base)
        } else {
            (a, b, r.eave - base - if r.form.tunnel() { bar.depth } else { under_bars })
        };
        let cw = crate::curtain::create(
            txn,
            crate::curtain::CreateParams {
                type_id: Some(ElementRef::Id(wall_type)),
                curve: Line { start: a, end: b },
                base_level: ElementRef::Id(level),
                base_offset: base,
                top_constraint: walls::TopConstraint::Unconnected(top),
                top_offset: 0.0,
            },
        )?;
        if k % 2 == 1 {
            for roof in &roofs_made {
                crate::curtain::set_attached(txn, cw, Some(txn.store().get(*roof).expect("made").guid().to_string()), None)?;
            }
            // Gable vents under the top, kept above the door.
            if r.gable_vents > 0.0 {
                let g = crate::curtain::regenerate(txn.store(), txn.store().get(cw).expect("made"))?;
                if g.z[1] - g.z[0] - r.gable_vents > DOOR_HEIGHT + 0.4 {
                    crate::curtain::make_vent_band(txn, cw, r.gable_vents)?;
                }
            }
            if r.doors {
                let along = (b[0] - a[0]).hypot(b[1] - a[1]);
                // Into a headhouse: wide enough for equipment.
                let into = r.headhouse.is_some_and(|h| (h.at == End::Start) == (k == 3));
                crate::curtain::make_door(txn, cw, along / 2.0, if into { HEADHOUSE_DOOR } else { DOOR_WIDTH }, DOOR_HEIGHT)?;
            }
        }
        if saw && k == 2 && vent_band > 0.0 {
            crate::curtain::make_vent_band(txn, cw, vent_band)?;
        } else if k % 2 == 0 && !r.form.tunnel() && side_band > 0.0 {
            crate::curtain::make_vent_band(txn, cw, side_band)?;
        }
        // Roll-up sides along the side walls' feet (not a sawtooth's high face), clear of a
        // vent band over them.
        if k % 2 == 0 && r.roll_up > 0.0 && !(saw && k == 2) && (side_band == 0.0 || r.form.tunnel() || r.roll_up < top - side_band - 0.3) {
            crate::curtain::make_roll_up(txn, cw, r.roll_up)?;
        }
        parts.push(cw);
    }
    // A sawtooth's inner vertical faces, standing on the eave between the end walls.
    for i in (0..r.spans.saturating_sub(1)).filter(|_| saw) {
        let y = r.span * f64::from(i + 1);
        let cw = crate::curtain::create(
            txn,
            crate::curtain::CreateParams {
                type_id: Some(ElementRef::Id(wall_type)),
                curve: Line { start: r.at(l - r.face_gap(), y), end: r.at(r.face_gap(), y) },
                base_level: ElementRef::Id(level),
                base_offset: r.eave,
                top_constraint: walls::TopConstraint::Unconnected(face_top(i) - r.eave),
                top_offset: 0.0,
            },
        )?;
        if vent_band > 0.0 {
            crate::curtain::make_vent_band(txn, cw, vent_band)?;
        }
        parts.push(cw);
    }
    // The posts: every bay along each gutter line (a Venlo's truss lines only), the outer lines
    // inside the glass and knee walls, the ends inside the end walls.
    if r.frame && !r.form.tunnel() {
        let designation = if r.span < 6.0 { POST_LIGHT } else { POST_HEAVY };
        let post = crate::steel_sections::load_column_section(
            txn,
            crate::steel_sections::LoadParams { catalog: "aisc_hss_rect".into(), designation: designation.into(), material: None },
        )?;
        let (half, depth) = txn.store().get(post).and_then(crate::columns::column_type).map_or((0.05, 0.1), |t| (t.width.max(t.depth) / 2.0, t.depth));
        let beam = crate::beams::load_section(
            txn,
            crate::steel_sections::LoadParams { catalog: "aisc_hss_rect".into(), designation: designation.into(), material: None },
        )?;
        let clear = (wall_bar.depth / 2.0).max(knee_half) + half + 0.005;
        let mut lines = vec![clear];
        lines.extend(r.gutter_lines());
        lines.push(w - clear);
        let bays = (l / r.bay).round().max(1.0) as u32;
        // The beam's top just under the gutter, the posts up to its underside.
        let beam_top = r.eave - GUTTER_DROP - 0.005;
        let height = beam_top - depth;
        let (first, last) = (clear, l - clear);
        for y in &lines {
            parts.push(crate::beams::create(
                txn,
                crate::beams::CreateParams {
                    type_id: Some(ElementRef::Id(beam)),
                    curve: Line { start: r.at(first, *y), end: r.at(last, *y) },
                    reference_level: ElementRef::Id(level),
                    start_offset: beam_top,
                    end_offset: beam_top,
                    usage: crate::beams::Usage::Girder,
                    orientation: Default::default(),
                    y_justification: None,
                    z_justification: Some(crate::beams::ZJustification::Top),
                    rotation: 0.0,
                },
            )?);
        }
        for k in 0..=bays {
            let x = (l * f64::from(k) / f64::from(bays)).clamp(clear, l - clear);
            for y in &lines {
                parts.push(crate::columns::place_structural(
                    txn,
                    crate::columns::PlaceParams {
                        type_id: Some(ElementRef::Id(post)),
                        point: r.at(x, *y),
                        base_level: ElementRef::Id(level),
                        base_offset: 0.0,
                        top_constraint: walls::TopConstraint::Unconnected(height),
                        top_offset: 0.0,
                        rotation: r.rotation,
                    },
                )?);
            }
        }
    }
    // The headhouse: walls on its three outer sides (the range's end wall is the fourth), a flat
    // roof over them, a door in the middle of its front wall.
    if let Some(h) = r.headhouse {
        let wt = walls::default_type(txn)?;
        let half = txn.store().get(wt).and_then(walls::type_width).unwrap_or(0.2) / 2.0;
        // Clear of the end wall's glass.
        let near = wall_bar.depth / 2.0 + 0.005;
        let (x_near, x_far) = match h.at {
            End::Start => (-near, -h.depth),
            End::End => (l + near, l + h.depth),
        };
        let wall = |txn: &mut Transaction<'_>, a: [f64; 2], b: [f64; 2]| {
            walls::create(
                txn,
                walls::CreateParams {
                    type_id: Some(ElementRef::Id(wt)),
                    curve: walls::CurveArg { line: Line { start: r.at(a[0], a[1]), end: r.at(b[0], b[1]) }, through: Vec::new() },
                    base_level: ElementRef::Id(level),
                    top_constraint: walls::TopConstraint::Unconnected(h.height),
                    base_offset: 0.0,
                    top_offset: 0.0,
                    location_line: walls::LocationLine::WallCenterline,
                    flip: false,
                    auto_join: Default::default(),
                    structural: false,
                    structural_usage: None,
                },
            )
        };
        // Wound so each wall's exterior faces out of the headhouse.
        let (ends, front) = if h.at == End::Start {
            ([([x_near, 0.0], [x_far, 0.0]), ([x_far, w], [x_near, w])], ([x_far, 0.0], [x_far, w]))
        } else {
            ([([x_far, 0.0], [x_near, 0.0]), ([x_near, w], [x_far, w])], ([x_far, w], [x_far, 0.0]))
        };
        for (a, b) in ends {
            parts.push(wall(txn, a, b)?);
        }
        let front_wall = wall(txn, front.0, front.1)?;
        parts.push(front_wall);
        // The roof over the walls' outer faces, stopping short of the end wall.
        let (x0, x1) = if h.at == End::Start { (x_far - half, x_near) } else { (x_near, x_far + half) };
        let corners = [r.at(x0, -half), r.at(x1, -half), r.at(x1, w + half), r.at(x0, w + half)];
        let curves = (0..4)
            .map(|k| RoofCurveArg { line: Line { start: corners[k], end: corners[(k + 1) % 4] }, lock: None, defines_slope: Some(false), slope: None, overhang: None, eave: None })
            .collect();
        parts.push(roofs::create(
            txn,
            roofs::CreateParams { type_id: None, curves, level_id: ElementRef::Id(level), base_offset: Some(h.height.into()), slope: None, cutoff_level_id: None, cutoff_offset: None },
        )?);
        parts.push(crate::inserts::place(
            txn,
            crate::inserts::Kind::Door,
            crate::inserts::PlaceParams {
                type_id: None,
                host_wall_id: Some(ElementRef::Id(front_wall)),
                point_along: Some(w / 2.0),
                point: None,
                sill_height: None,
                flip_hand: false,
                flip_facing: false,
                width: None,
                height: None,
                fit: None,
            },
        )?);
    }
    // Gutters: both eaves of the outer spans, and one valley gutter between each pair.
    if r.gutters && !r.form.tunnel() {
        for (i, roof) in roofs_made.iter().enumerate() {
            // Curve keys: 1 the eave at y0, 3 the eave at y1; each span's y1 eave carries the
            // valley gutter it shares with the next.
            // A sawtooth's at each low eave only.
            let edges: &[u32] = if saw { &[1] } else if i == 0 { &[1, 3] } else { &[3] };
            for edge in edges {
                parts.push(crate::mep_gutters::create(
                    txn,
                    crate::mep_gutters::CreateParams {
                        roof_id: ElementRef::Id(*roof),
                        edges: vec![*edge],
                        profile: Default::default(),
                        width: None,
                        depth: None,
                        fall: Some(0.0),
                    },
                )?);
            }
        }
    }
    parts.extend(crate::hothouse::build_screens(txn, r)?);
    for id in &parts {
        txn.modify(*id, |e| {
            e.params.insert(PART_KEY.into(), json!(guid));
        })?;
        // The vents' settings and totals on the roofs and walls that have them.
        let host = txn.store().get(*id).is_some_and(|e| roofs::roof_recipe(e).is_some() || crate::curtain::curtain_recipe(e).is_some());
        if host {
            crate::vents::write_host(txn, *id, &r.vent)?;
        }
    }
    Ok(parts)
}

/// The published design values of an enclosure (ARC-GLZ-007).
pub const DESIGN_PSET: &str = "Reviera_EnclosureDesign";

fn design_params(r: &EnclosureRecipe) -> Vec<(String, Value)> {
    let k = |p: &str| format!("{DESIGN_PSET}.{p}");
    let t = |ty: &str, v: Value| json!({ "type": ty, "value": v });
    let length = |v: f64| t("IfcPositiveLengthMeasure", json!(v));
    let mut out = vec![
        (k("Form"), t("IfcLabel", serde_json::to_value(r.form).expect("serialises"))),
        (k("Length"), length(r.length)),
        (k("Width"), length(r.width())),
        (k("Span"), length(r.span)),
        (k("Spans"), t("IfcCountMeasure", json!(r.spans))),
        (k("BaySpacing"), length(r.bay)),
        (k("EaveHeight"), t("IfcLengthMeasure", json!(r.eave))),
        (k("RidgeHeight"), length(r.ridge())),
        (k("FloorArea"), t("IfcAreaMeasure", json!(r.width() * r.length))),
        (k("RoofVentTarget"), t("IfcPositiveRatioMeasure", json!(r.vents / 100.0))),
        (k("SideVentTarget"), t("IfcPositiveRatioMeasure", json!(r.side_vents / 100.0))),
    ];
    if !r.form.tunnel() {
        out.push((k("Pitch"), t("IfcPlaneAngleMeasure", json!(r.pitch))));
    }
    out
}

fn write_recipe(txn: &mut Transaction<'_>, id: ElementId, r: &EnclosureRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    let design = design_params(r);
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
        e.params.retain(|key, _| !key.starts_with(&format!("{DESIGN_PSET}.")));
        for (key, v) in design {
            e.params.insert(key, v);
        }
    })
}

fn number(store: &Store, v: &Value, data_type: &str, what: &str) -> Result<f64> {
    match v {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => crate::units::parse(s, data_type, &crate::units::settings_of(store)),
        _ => Err(Error::new("enclosure.invalid_value", format!("{what} must be a number or text with units"))),
    }
}

/// The parts made again from the recipe, the old ones deleted.
fn rebuild(txn: &mut Transaction<'_>, id: ElementId, mut r: EnclosureRecipe) -> Result<()> {
    let store = txn.store().clone();
    for g in &r.parts {
        if let Some(part) = GlobalId::parse(g).and_then(|g| store.find_by_guid(&g))
            && txn.store().contains(part)
        {
            txn.delete(part)?;
        }
    }
    let guid = store.get(id).expect("exists").guid().to_string();
    let parts = build(txn, &guid, &r)?;
    r.parts = parts.iter().map(|p| txn.store().get(*p).expect("made").guid().to_string()).collect();
    write_recipe(txn, id, &r)?;
    // Its growing layouts and grow lights are laid out again on the new floor.
    crate::growing::follow(txn, &guid)?;
    crate::hothouse::follow(txn, &guid)
}

// ---- Commands ----

/// The parameters of `enclosure.create` and `enclosure.set` (lengths in metres or text with
/// units, angles in radians or text).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Params {
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub length: Option<Value>,
    /// Each roof's span (Venlo: the truss width, divided among its peaks).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub span: Option<Value>,
    /// Multispan: how many spans; Venlo: how many trusses.
    pub spans: Option<u32>,
    /// Venlo: roofs per truss (default 2).
    pub peaks: Option<u32>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub bay: Option<Value>,
    /// The eave (gutter) height above the level.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub eave: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub pitch: Option<Value>,
    /// A knee wall's height; 0 for none.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub knee: Option<Value>,
    pub gutters: Option<bool>,
    /// A door in each end wall (default on).
    pub doors: Option<bool>,
    /// Ridge vents' area, percent of the floor area (default 18; 0 for none).
    pub vents: Option<f64>,
    /// Side vents' area, percent of the floor area (default 0, none).
    pub side_vents: Option<f64>,
    /// Roll-up sides' opening height up the side walls (0 for none).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub roll_up: Option<Value>,
    /// Gable vents' band depth under the end walls' tops (0 for none).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub gable_vents: Option<Value>,
    /// A headhouse across one end: its sizes (each its default when absent), or `enabled: false`
    /// to remove it.
    pub headhouse: Option<HeadhouseArgs>,
    /// Energy, shade and blackout screens (ARC-GLZ-010), replacing those it has ([] for none).
    pub screens: Option<Vec<crate::hothouse::Screen>>,
    /// The vents' design opening angle (radians or "30°"), discharge coefficient and actuator.
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub vent_angle: Option<Value>,
    pub vent_discharge: Option<f64>,
    pub vent_actuator: Option<crate::vents::Actuator>,
    /// Steel posts at the bays (default on).
    pub frame: Option<bool>,
    /// Gothic: each arc's radius as a share of the span (default 0.6).
    pub arc: Option<f64>,
    /// The roof's and walls' glazing, by library name (`glazing.list_types`); "" for the default.
    pub roof_glazing: Option<String>,
    pub wall_glazing: Option<String>,
    /// The glazing bars' frame system, by library name (`frame.list_types`); "" for the default.
    pub frame_system: Option<String>,
    /// The corner laid out from, and the direction of the length (radians or "90°").
    pub origin: Option<[f64; 2]>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<Value>,
}

/// A headhouse's arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadhouseArgs {
    /// False removes the headhouse (default true).
    pub enabled: Option<bool>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub depth: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<Value>,
    pub at: Option<End>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub form: Form,
    pub level_id: ElementRef,
    #[serde(flatten)]
    pub params: Params,
}

/// A form's defaults (provisional, EXPERT_REVIEW E-44).
pub fn defaults(form: Form) -> EnclosureRecipe {
    let base = EnclosureRecipe {
        kind: RECIPE.into(),
        form,
        level: String::new(),
        origin: [0.0, 0.0],
        rotation: 0.0,
        length: 20.0,
        span: 9.6,
        spans: 1,
        peaks: None,
        bay: 3.0,
        eave: 3.0,
        pitch: (5.0f64 / 12.0).atan(),
        knee: None,
        gutters: true,
        doors: true,
        vents: vent_percent(),
        side_vents: 0.0,
        roll_up: 0.0,
        gable_vents: 0.0,
        vent: Default::default(),
        headhouse: None,
        frame: true,
        arc: gothic_arc(),
        roof_glazing: None,
        wall_glazing: None,
        frame_system: None,
        screens: Vec::new(),
        parts: Vec::new(),
    };
    match form {
        Form::Gable => base,
        Form::Multispan => EnclosureRecipe { spans: 3, bay: 4.0, eave: 4.5, length: 40.0, ..base },
        Form::Venlo => EnclosureRecipe { span: 4.0, spans: 6, peaks: Some(2), bay: 4.0, eave: 5.5, length: 40.0, pitch: 22f64.to_radians(), ..base },
        Form::Sawtooth => EnclosureRecipe { span: 6.4, spans: 3, bay: 4.0, eave: 4.0, length: 40.0, pitch: 25f64.to_radians(), ..base },
        Form::Hoop | Form::Gothic => EnclosureRecipe { span: 9.1, bay: 1.5, eave: 1.2, length: 30.0, gutters: false, frame: false, vents: 0.0, ..base },
    }
}

/// `p` applied to `r` (a Venlo's `span` and `spans` given per truss).
fn apply(store: &Store, r: &mut EnclosureRecipe, p: &Params) -> Result<()> {
    let num = |v: &Option<Value>, t: &str, what: &str| v.as_ref().map(|v| number(store, v, t, what)).transpose();
    if let Some(v) = num(&p.length, "length", "the length")? {
        r.length = v;
    }
    let peaks = p.peaks.or(r.peaks).unwrap_or(2).max(1);
    if r.form == Form::Venlo {
        let trusses = r.spans / r.peaks.unwrap_or(2).max(1);
        let truss = r.span * f64::from(r.peaks.unwrap_or(2).max(1));
        let truss = num(&p.span, "length", "the truss width")?.unwrap_or(truss);
        let trusses = p.spans.unwrap_or(trusses.max(1));
        r.peaks = Some(peaks);
        r.span = truss / f64::from(peaks);
        r.spans = trusses * peaks;
    } else {
        if let Some(v) = num(&p.span, "length", "the span")? {
            r.span = v;
        }
        if let Some(n) = p.spans {
            r.spans = n;
        }
    }
    if let Some(v) = num(&p.bay, "length", "the bay spacing")? {
        r.bay = v;
    }
    if let Some(v) = num(&p.eave, "length", "the eave height")? {
        r.eave = v;
    }
    if let Some(v) = num(&p.pitch, "angle", "the pitch")? {
        r.pitch = v;
    }
    if let Some(v) = num(&p.knee, "length", "the knee wall height")? {
        r.knee = (v > 0.0).then_some(v);
    }
    if let Some(g) = p.gutters {
        r.gutters = g;
    }
    if let Some(d) = p.doors {
        r.doors = d;
    }
    if let Some(f) = p.frame {
        r.frame = f;
    }
    if let Some(a) = p.arc {
        r.arc = a;
    }
    for (given, slot) in [(&p.roof_glazing, &mut r.roof_glazing), (&p.wall_glazing, &mut r.wall_glazing)] {
        if let Some(g) = given {
            if !g.is_empty() && crate::glazing_library::entry(g).is_none() {
                return Err(Error::new("glazing.unknown", format!("no glazing {g} in the library (glazing.list_types)")));
            }
            *slot = (!g.is_empty()).then(|| g.clone());
        }
    }
    if let Some(f) = &p.frame_system {
        if !f.is_empty() && crate::frame_library::entry(f).is_none() {
            return Err(Error::new("frame.unknown", format!("no frame system {f} in the library (frame.list_types)")));
        }
        r.frame_system = (!f.is_empty()).then(|| f.clone());
    }
    if let Some(v) = p.vents {
        if !(0.0..=60.0).contains(&v) {
            return Err(Error::new("enclosure.bad_value", "the vent area is 0 to 60 % of the floor area"));
        }
        r.vents = v;
    }
    if let Some(v) = p.side_vents {
        if !(0.0..=60.0).contains(&v) {
            return Err(Error::new("enclosure.bad_value", "the side vent area is 0 to 60 % of the floor area"));
        }
        r.side_vents = v;
    }
    if let Some(sc) = &p.screens {
        crate::hothouse::check_screens(r, sc)?;
        r.screens = sc.clone();
    }
    if let Some(a) = &p.headhouse {
        if a.enabled == Some(false) {
            r.headhouse = None;
        } else {
            let mut h = r.headhouse.unwrap_or_else(|| r.default_headhouse());
            if let Some(v) = num(&a.depth, "length", "the headhouse depth")? {
                h.depth = v;
            }
            if let Some(v) = num(&a.height, "length", "the headhouse height")? {
                h.height = v;
            }
            if let Some(at) = a.at {
                h.at = at;
            }
            if !(2.0..=50.0).contains(&h.depth) || !(2.4..=12.0).contains(&h.height) {
                return Err(Error::new("enclosure.bad_value", "a headhouse is 2 to 50 m deep and 2.4 to 12 m high"));
            }
            r.headhouse = Some(h);
        }
    }
    if let Some(v) = num(&p.roll_up, "length", "the roll-up height")? {
        if !(v == 0.0 || (0.2..=5.0).contains(&v)) {
            return Err(Error::new("enclosure.bad_value", "a roll-up side opens 0.2 to 5 m (0 for none)"));
        }
        r.roll_up = v;
    }
    if let Some(v) = num(&p.gable_vents, "length", "the gable vents' depth")? {
        if !(v == 0.0 || (0.2..=3.0).contains(&v)) {
            return Err(Error::new("enclosure.bad_value", "gable vents are a band 0.2 to 3 m deep (0 for none)"));
        }
        r.gable_vents = v;
    }
    if let Some(v) = num(&p.vent_angle, "angle", "the vent opening angle")? {
        if !(v > 0.0 && v <= std::f64::consts::FRAC_PI_2) {
            return Err(Error::new("enclosure.bad_value", "vents open 0 to 90°"));
        }
        r.vent.opening_angle = v;
    }
    if let Some(c) = p.vent_discharge {
        if !(c > 0.0 && c <= 1.0) {
            return Err(Error::new("enclosure.bad_value", "the discharge coefficient is above 0 and at most 1"));
        }
        r.vent.discharge = c;
    }
    if let Some(a) = p.vent_actuator {
        r.vent.actuator = a;
    }
    if let Some(o) = p.origin {
        r.origin = o;
    }
    if let Some(v) = num(&p.rotation, "angle", "the rotation")? {
        r.rotation = v;
    }
    Ok(())
}

/// `enclosure.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let level = store
        .resolve(&p.level_id)
        .filter(|l| store.get(*l).is_some_and(|e| e.class == "IfcBuildingStorey"))
        .ok_or_else(|| Error::new("enclosure.no_level", "no such level"))?;
    let mut r = defaults(p.form);
    r.level = store.get(level).expect("resolved").guid().to_string();
    apply(&store, &mut r, &p.params)?;
    r.check()?;
    let guid = GlobalId::new();
    let name = match p.form {
        Form::Gable => "Gable Greenhouse",
        Form::Multispan => "Multispan Greenhouse",
        Form::Venlo => "Venlo Greenhouse",
        Form::Hoop => "Hoop Tunnel",
        Form::Gothic => "Gothic Tunnel",
        Form::Sawtooth => "Sawtooth Greenhouse",
    };
    let id = txn.create(
        NewElement::new(CLASS)
            .guid(guid.clone())
            .name(name)
            .param("@PredefinedType", "USERDEFINED")
            .param("@ObjectType", OBJECT_TYPE)
            .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    let parts = build(txn, guid.as_str(), &r)?;
    r.parts = parts.iter().map(|p| txn.store().get(*p).expect("made").guid().to_string()).collect();
    write_recipe(txn, id, &r)?;
    Ok(id)
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, EnclosureRecipe)> {
    let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", "no such enclosure"))?;
    let recipe = recipe(store.get(id).expect("resolved"))
        .ok_or_else(|| Error::new("enclosure.not_found", format!("element {id} is not a glazed enclosure")))?;
    Ok((id, recipe))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub enclosure_id: ElementRef,
    #[serde(flatten)]
    pub params: Params,
}

/// `enclosure.set`: the parameters changed, the parts built again.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut r) = resolve(&store, &p.enclosure_id)?;
    apply(&store, &mut r, &p.params)?;
    rebuild(txn, id, r)
}

/// Which vents `enclosure.set_vents` lays out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VentKind {
    /// Ridge vents (a sawtooth's vertical faces).
    Roof,
    /// Bands along the side walls' tops.
    Side,
    /// Roll-up sides along the side walls' feet, `height` up.
    RollUp,
    /// Bands `height` deep under the end walls' tops.
    Gable,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetVentsParams {
    pub enclosure_id: ElementRef,
    pub kind: VentKind,
    /// Roof and side vents: percent of the floor area (0 for none).
    pub target_percent: Option<f64>,
    /// Roll-up sides and gable vents: the opening's height or the band's depth (0 for none).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<Value>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub opening_angle: Option<Value>,
    pub discharge: Option<f64>,
    pub actuator: Option<crate::vents::Actuator>,
}

/// `enclosure.set_vents`: a kind of vents laid out to a target, and how all the vents open.
pub(crate) fn set_vents(txn: &mut Transaction<'_>, p: SetVentsParams) -> Result<()> {
    let mut params = Params { vent_angle: p.opening_angle, vent_discharge: p.discharge, vent_actuator: p.actuator, ..Default::default() };
    match p.kind {
        VentKind::Roof => params.vents = p.target_percent,
        VentKind::Side => params.side_vents = p.target_percent,
        VentKind::RollUp => params.roll_up = p.height,
        VentKind::Gable => params.gable_vents = p.height,
    }
    set(txn, SetParams { enclosure_id: p.enclosure_id, params })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetParams {
    pub enclosure_id: ElementRef,
}

/// What `enclosure.get` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Info {
    pub recipe: EnclosureRecipe,
    pub width: f64,
    pub ridge: f64,
    pub floor_area: f64,
    /// The vents' area (ridge vents, a sawtooth's face vents), and as a share of the floor
    /// area (percent).
    pub vent_area: f64,
    pub vent_percent: f64,
    /// The vents open at their design angle: free and effective (× the discharge coefficient)
    /// area, and the side walls' share of the free area.
    pub free_vent_area: f64,
    pub effective_vent_area: f64,
    pub side_vent_area: f64,
    /// The end walls' gable vents' pane area.
    pub gable_vent_area: f64,
    /// The share of the roof its glazing bars shade (for the crop-light and daylight analyses).
    pub frame_shading: f64,
    /// The parts that still exist, by kind.
    pub roofs: Vec<ElementId>,
    pub curtain_walls: Vec<ElementId>,
    pub walls: Vec<ElementId>,
    pub gutters: Vec<ElementId>,
    pub posts: Vec<ElementId>,
    pub beams: Vec<ElementId>,
    pub doors: Vec<ElementId>,
    /// Energy, shade and blackout screen sheets (ARC-GLZ-010).
    pub screens: Vec<ElementId>,
    /// The headhouse's floor, and as a share of the growing floor (percent).
    pub headhouse_area: f64,
    pub headhouse_percent: f64,
    /// What the enclosure should be checked for: a roof flatter than its glazing allows, or
    /// flat enough for condensate to drip (ARC-GLZ-002, EXPERT_REVIEW E-45).
    pub warnings: Vec<String>,
}

/// Below this pitch condensate drips from the glazing instead of running down it (provisional,
/// EXPERT_REVIEW E-45).
const DRIP_PITCH: f64 = 10.0;

/// The roof pitch checked against its glazing's least pitch and the condensate guideline.
fn pitch_warnings(store: &Store, r: &EnclosureRecipe, roof: Option<&Element>) -> Vec<String> {
    // A tunnel's arch is steep but at its crown; its film sheds on the curve.
    if r.form.tunnel() {
        return Vec::new();
    }
    let deg = r.pitch.to_degrees();
    let mut out = Vec::new();
    let ty = roof.and_then(|e| store.find_by_guid(&GlobalId::parse(e.param("@type")?.as_str()?)?)).and_then(|t| store.get(t));
    if let Some(t) = ty
        && let Some(g) = crate::glazing_library::of_type(t)
        && g.min_pitch > 0.0
        && r.pitch < g.min_pitch - 1e-9
    {
        out.push(format!(
            "the roof pitch {deg:.1}° is below the {:.1}° its glazing ({}) may be laid at",
            g.min_pitch.to_degrees(),
            t.name.as_deref().unwrap_or("its type")
        ));
    }
    if deg < DRIP_PITCH {
        out.push(format!("at {deg:.1}° condensate may drip from the roof onto the crop; {DRIP_PITCH}° or more lets it run to the gutters"));
    }
    out
}

/// `enclosure.get`.
pub(crate) fn get(store: &Store, p: GetParams) -> Result<Info> {
    let (_, r) = resolve(store, &p.enclosure_id)?;
    let mut info = Info { width: r.width(), ridge: r.ridge(), floor_area: r.width() * r.length, frame_shading: r.frame_shading(), vent_area: 0.0, free_vent_area: 0.0, effective_vent_area: 0.0, side_vent_area: 0.0, gable_vent_area: 0.0, vent_percent: 0.0, recipe: r.clone(), roofs: vec![], curtain_walls: vec![], walls: vec![], gutters: vec![], posts: vec![], beams: vec![], doors: vec![], screens: vec![], headhouse_area: 0.0, headhouse_percent: 0.0, warnings: vec![] };
    for g in &r.parts {
        let Some(e) = GlobalId::parse(g).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i)) else { continue };
        let list = if roofs::roof_recipe(e).is_some() {
            &mut info.roofs
        } else if crate::curtain::curtain_recipe(e).is_some() {
            &mut info.curtain_walls
        } else if walls::wall_recipe(e).is_some() {
            &mut info.walls
        } else if e.class == "IfcColumn" {
            &mut info.posts
        } else if e.class == "IfcBeam" {
            &mut info.beams
        } else if e.class == "IfcDoor" {
            &mut info.doors
        } else if crate::hothouse::screen_recipe(e).is_some() {
            &mut info.screens
        } else {
            &mut info.gutters
        };
        list.push(e.id());
    }
    info.vent_area = info.roofs.iter().filter_map(|id| store.get(*id)).map(|e| vent_area(store, e)).sum::<f64>()
        + info.curtain_walls.iter().filter_map(|id| store.get(*id)).map(|e| wall_vent_area(store, e)).sum::<f64>();
    info.vent_percent = 100.0 * info.vent_area / info.floor_area.max(1e-9);
    for id in info.roofs.iter().chain(&info.curtain_walls) {
        let Some(e) = store.get(*id) else { continue };
        let t = crate::vents::totals(store, e, &r.vent);
        info.free_vent_area += t.free_area;
        info.effective_vent_area += t.effective_area;
        // A side wall: a curtain wall along the length, low (not a sawtooth's high face).
        if let Some(c) = crate::curtain::curtain_recipe(e) {
            let along = (c.curve.end[0] - c.curve.start[0]) * r.rotation.cos() + (c.curve.end[1] - c.curve.start[1]) * r.rotation.sin();
            let across = r.rotation.cos() * (c.curve.start[1] - r.origin[1]) - r.rotation.sin() * (c.curve.start[0] - r.origin[0]);
            let side = along.abs() > 0.5 * r.length && !(r.form == Form::Sawtooth && across > 1e-6);
            if side {
                info.side_vent_area += t.pane_area;
            } else if !c.attach_top.is_empty() {
                info.gable_vent_area += t.pane_area;
            }
        }
    }
    if let Some(h) = r.headhouse {
        info.headhouse_area = h.depth * r.width();
        info.headhouse_percent = 100.0 * info.headhouse_area / info.floor_area.max(1e-9);
    }
    info.warnings = pitch_warnings(store, &r, info.roofs.first().and_then(|id| store.get(*id)));
    Ok(info)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DetachParams {
    pub part_ids: Vec<ElementRef>,
}

/// `enclosure.detach`: parts left as they are, no longer rebuilt by their enclosure.
pub(crate) fn detach(txn: &mut Transaction<'_>, p: DetachParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.part_ids {
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", "no such element"))?;
        let e = store.get(id).expect("resolved");
        let Some(owner) = enclosure_of(e).and_then(GlobalId::parse).and_then(|g| store.find_by_guid(&g)) else {
            return Err(Error::new("enclosure.not_a_part", format!("element {id} is not part of an enclosure")));
        };
        let guid = e.guid().to_string();
        txn.modify(id, |e| {
            e.params.remove(PART_KEY);
        })?;
        let mut rec = recipe(txn.store().get(owner).expect("found")).expect("an enclosure");
        rec.parts.retain(|g| *g != guid);
        write_recipe(txn, owner, &rec)?;
    }
    Ok(())
}

/// A form as `enclosure.forms` lists it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FormInfo {
    pub form: Form,
    pub defaults: EnclosureRecipe,
}

/// `enclosure.forms`.
pub(crate) fn forms(_: &Store, _: crate::builtin::NoParams) -> Result<Vec<FormInfo>> {
    Ok([Form::Gable, Form::Multispan, Form::Venlo, Form::Sawtooth, Form::Hoop, Form::Gothic].into_iter().map(|form| FormInfo { form, defaults: defaults(form) }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_published_definitions_cover_the_design_values() {
        let defs: Value = serde_json::from_str(crate::glazing_library::DEFINITIONS).unwrap();
        let schema: Value = serde_json::from_str(crate::glazing_library::DEFINITIONS_SCHEMA).unwrap();
        assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&defs));
        let set = defs["property_sets"].as_array().unwrap().iter().find(|s| s["name"] == DESIGN_PSET).expect("Reviera_EnclosureDesign");
        let props = set["properties"].as_array().unwrap();
        for (key, value) in design_params(&defaults(Form::Venlo)) {
            let name = key.strip_prefix(&format!("{DESIGN_PSET}.")).unwrap();
            let def = props.iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("{name} is not published"));
            assert_eq!(def["measure"], value["type"], "{name}");
        }
    }

    #[test]
    fn a_venlo_is_trusses_of_narrow_spans() {
        let store = Store::default();
        let mut r = defaults(Form::Venlo);
        apply(&store, &mut r, &Params { span: Some(json!(8.0)), spans: Some(6), ..Default::default() }).unwrap();
        assert_eq!((r.spans, r.span, r.width()), (12, 4.0, 48.0));
        r.check().unwrap();
        // A frame turned 90°: the length runs up the project's y axis.
        r.rotation = std::f64::consts::FRAC_PI_2;
        let p = r.at(10.0, 0.0);
        assert!(p[0].abs() < 1e-9 && (p[1] - 10.0).abs() < 1e-9);
        assert!(EnclosureRecipe { spans: 2, ..defaults(Form::Gable) }.check().is_err());
    }
}
