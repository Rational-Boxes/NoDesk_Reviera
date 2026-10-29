// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Glazed enclosures (spec Ch 3 §3.6a, ARC-GLZ-001/002): one element that lays out a glazed
//! building from a form and a few parameters, made of Reviera's ordinary elements, and builds
//! them again when a parameter changes.
//!
//! Forms built so far, all as rows of gable spans side by side along the enclosure's length:
//! - **Gable**: one span;
//! - **Multispan** (gutter-connected): `spans` spans sharing valley gutters;
//! - **Venlo**: `trusses` trusses of `peaks` narrow roofs each.
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
//! Not yet: hoop, gothic and sawtooth forms; rafters, purlins and Venlo trusses; side
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
/// The glazing bars of both glass types.
const ROOF_BAR: Mullion = Mullion { width: 0.04, depth: 0.08 };
const WALL_BAR: Mullion = Mullion { width: 0.04, depth: 0.08 };
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
const DOOR_HEIGHT: f64 = 2.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    Gable,
    Multispan,
    Venlo,
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
    /// Ridge vents' target area, percent of the floor area (0 for none).
    #[serde(default = "vent_percent")]
    pub vents: f64,
    /// Steel posts at the bays.
    #[serde(default = "yes")]
    pub frame: bool,
    /// The parts (GlobalIds).
    #[serde(default)]
    pub parts: Vec<String>,
}

fn yes() -> bool {
    true
}

fn vent_percent() -> f64 {
    18.0
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

    /// A point of the enclosure's frame in the project's plan.
    fn at(&self, x: f64, y: f64) -> [f64; 2] {
        let (s, c) = self.rotation.sin_cos();
        [self.origin[0] + c * x - s * y, self.origin[1] + s * x + c * y]
    }

    /// The highest ridge above the level: an outer span's, which loses half a gutter's width
    /// to its one valley (the inner ones lose a whole gutter's).
    pub fn ridge(&self) -> f64 {
        let width = if self.spans > 1 { self.span - GUTTER_WIDTH / 2.0 } else { self.span };
        self.eave + width / 2.0 * self.pitch.tan()
    }

    fn check(&self) -> Result<()> {
        let bad = |m: &str| Err(Error::new("enclosure.bad_value", m.to_owned()));
        if !(self.length >= 1.0 && self.span >= 1.0 && self.spans >= 1 && self.spans <= 200) {
            return bad("an enclosure needs a length and span of at least 1 m, and 1 to 200 spans");
        }
        if !(self.bay >= 0.3 && self.bay <= self.length) {
            return bad("the bay spacing must be at least 300 mm and no longer than the enclosure");
        }
        if !(self.eave >= 0.5 && self.eave <= 30.0) {
            return bad("the eave height must be between 0.5 and 30 m");
        }
        if !(self.pitch >= 1f64.to_radians() && self.pitch <= 60f64.to_radians()) {
            return bad("the roof pitch must be between 1° and 60°");
        }
        if self.knee.is_some_and(|k| !(k > 0.0 && k < self.eave - 0.3)) {
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

/// The named type, made by `make` on first use.
fn type_named(txn: &mut Transaction<'_>, name: &str, is: fn(&Element) -> bool, make: impl FnOnce() -> NewElement) -> Result<ElementId> {
    if let Some(e) = txn.store().iter().find(|e| e.name.as_deref() == Some(name) && is(e)) {
        return Ok(e.id());
    }
    txn.create(make())
}

fn roof_glazing(txn: &mut Transaction<'_>) -> Result<ElementId> {
    type_named(
        txn,
        ROOF_GLAZING,
        |e| roofs::glazing::of_type(e).is_some(),
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

fn wall_glazing(txn: &mut Transaction<'_>) -> Result<ElementId> {
    type_named(
        txn,
        WALL_GLAZING,
        |e| crate::curtain::curtain_type(e).is_some(),
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
    let roof_type = roof_glazing(txn)?;
    let wall_type = wall_glazing(txn)?;
    let (l, w) = (r.length, r.width());
    let mut parts = Vec::new();
    // The roofs: one gable span each, its eaves along the length.
    let mut roofs_made = Vec::new();
    for i in 0..r.spans {
        // At a valley each roof stops half a gutter short, the valley gutter between them.
        let half = GUTTER_WIDTH / 2.0;
        let y0 = r.span * f64::from(i) + if i > 0 { half } else { 0.0 };
        let y1 = r.span * f64::from(i + 1) - if i + 1 < r.spans { half } else { 0.0 };
        let corners = [r.at(0.0, y0), r.at(l, y0), r.at(l, y1), r.at(0.0, y1)];
        let curves = (0..4)
            .map(|k| RoofCurveArg {
                line: Line { start: corners[k], end: corners[(k + 1) % 4] },
                lock: None,
                defines_slope: Some(k % 2 == 0),
                slope: None,
                // The outer eaves reach past the side walls' glass so the gutters hang clear.
                overhang: Some(if (k == 0 && i == 0) || (k == 2 && i + 1 == r.spans) { WALL_BAR.depth / 2.0 } else { 0.0 }.into()),
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
        // The share of the floor under this span, valley gutters included.
        ridge_vents(txn, roof, r.vents / 100.0 * r.span * l)?;
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
    let inset = if r.doors { (WALL_BAR.depth / 2.0).max(knee_half) } else { WALL_BAR.depth / 2.0 };
    let under_bars = ROOF_BAR.depth / 2.0 / r.pitch.cos();
    for (k, (a, b)) in perimeter.into_iter().enumerate() {
        let base = base_of(k);
        let (a, b, top) = if k % 2 == 1 {
            let d = [(b[0] - a[0]) / w, (b[1] - a[1]) / w];
            let own = if r.spans > 1 { r.eave - GUTTER_DROP } else { r.eave };
            ([a[0] + d[0] * inset, a[1] + d[1] * inset], [b[0] - d[0] * inset, b[1] - d[1] * inset], own - base)
        } else {
            (a, b, r.eave - base - under_bars)
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
            if r.doors {
                let along = (b[0] - a[0]).hypot(b[1] - a[1]);
                crate::curtain::make_door(txn, cw, along / 2.0, DOOR_WIDTH, DOOR_HEIGHT)?;
            }
        }
        parts.push(cw);
    }
    // The posts: every bay along each gutter line (a Venlo's truss lines only), the outer lines
    // inside the glass and knee walls, the ends inside the end walls.
    if r.frame {
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
        let clear = (WALL_BAR.depth / 2.0).max(knee_half) + half + 0.005;
        let every = if r.form == Form::Venlo { r.peaks.unwrap_or(2).max(1) } else { 1 };
        let lines: Vec<f64> = (0..=r.spans)
            .filter(|i| i % every == 0)
            .map(|i| match i {
                0 => clear,
                i if i == r.spans => w - clear,
                i => r.span * f64::from(i),
            })
            .collect();
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
    // Gutters: both eaves of the outer spans, and one valley gutter between each pair.
    if r.gutters {
        for (i, roof) in roofs_made.iter().enumerate() {
            // Curve keys: 1 the eave at y0, 3 the eave at y1; each span's y1 eave carries the
            // valley gutter it shares with the next.
            let edges: &[u32] = if i == 0 { &[1, 3] } else { &[3] };
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
    for id in &parts {
        txn.modify(*id, |e| {
            e.params.insert(PART_KEY.into(), json!(guid));
        })?;
    }
    Ok(parts)
}

fn write_recipe(txn: &mut Transaction<'_>, id: ElementId, r: &EnclosureRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
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
    write_recipe(txn, id, &r)
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
    /// Steel posts at the bays (default on).
    pub frame: Option<bool>,
    /// The corner laid out from, and the direction of the length (radians or "90°").
    pub origin: Option<[f64; 2]>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<Value>,
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
        frame: true,
        parts: Vec::new(),
    };
    match form {
        Form::Gable => base,
        Form::Multispan => EnclosureRecipe { spans: 3, bay: 4.0, eave: 4.5, length: 40.0, ..base },
        Form::Venlo => EnclosureRecipe { span: 4.0, spans: 6, peaks: Some(2), bay: 4.0, eave: 5.5, length: 40.0, pitch: 22f64.to_radians(), ..base },
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
    if let Some(v) = p.vents {
        if !(0.0..=60.0).contains(&v) {
            return Err(Error::new("enclosure.bad_value", "the vent area is 0 to 60 % of the floor area"));
        }
        r.vents = v;
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
    /// The ridge vents' area, and as a share of the floor area (percent).
    pub vent_area: f64,
    pub vent_percent: f64,
    /// The parts that still exist, by kind.
    pub roofs: Vec<ElementId>,
    pub curtain_walls: Vec<ElementId>,
    pub walls: Vec<ElementId>,
    pub gutters: Vec<ElementId>,
    pub posts: Vec<ElementId>,
    pub beams: Vec<ElementId>,
}

/// `enclosure.get`.
pub(crate) fn get(store: &Store, p: GetParams) -> Result<Info> {
    let (_, r) = resolve(store, &p.enclosure_id)?;
    let mut info = Info { width: r.width(), ridge: r.ridge(), floor_area: r.width() * r.length, vent_area: 0.0, vent_percent: 0.0, recipe: r.clone(), roofs: vec![], curtain_walls: vec![], walls: vec![], gutters: vec![], posts: vec![], beams: vec![] };
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
        } else {
            &mut info.gutters
        };
        list.push(e.id());
    }
    info.vent_area = info.roofs.iter().filter_map(|id| store.get(*id)).map(|e| vent_area(store, e)).sum();
    info.vent_percent = 100.0 * info.vent_area / info.floor_area.max(1e-9);
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
    Ok([Form::Gable, Form::Multispan, Form::Venlo].into_iter().map(|form| FormInfo { form, defaults: defaults(form) }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
