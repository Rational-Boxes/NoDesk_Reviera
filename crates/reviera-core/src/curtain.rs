// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Curtain walls (ARC-CW-001 … 006, ARC-WALL-013): a surface divided by grid lines into cells,
//! each filled by a panel, with mullions along the grid lines and the borders.
//!
//! A curtain wall is an `IfcCurtainWall` with a `recipe.curtain_wall`: its base line in plan
//! (straight), its vertical extent between levels ([`Extent`]), and per direction a justification
//! and offset for the type's layout. Its type (`IfcCurtainWallType` with a `recipe.curtain_type`)
//! gives the layout rules of the vertical grid lines (along the wall, U) and the horizontal ones
//! (up it, V) — None, Fixed Distance, Fixed Number, Maximum or Minimum Spacing ([`solve`]) — the
//! interior and border mullions' size, and the panels (glazed, solid or empty) and their
//! thickness.
//!
//! A curtain wall drawn along a basic wall is embedded in it: it cuts its own opening through
//! the wall (ARC-WALL-013). In IFC the curtain wall has its base line as its axis and aggregates
//! its panels (`IfcPlate`, `CURTAIN_PANEL`) and mullions (`IfcMember`, `MULLION`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::levels::Extent;
use crate::store::Store;
use crate::walls::{Line, RECIPE_KEY, TopConstraint};

pub const CURTAIN_RECIPE: &str = "recipe.curtain_wall";
pub const CURTAIN_TYPE_RECIPE: &str = "recipe.curtain_type";
pub const DEFAULT_TYPE_NAME: &str = "Storefront";
/// Grid lines this close to the ends are left out (§3.0 `TOL_POINT`, generously).
const TOL: f64 = 0.001;

/// How a direction's grid lines are laid out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    #[default]
    None,
    FixedDistance,
    FixedNumber,
    MaximumSpacing,
    MinimumSpacing,
}

/// Where Fixed Distance lines are measured from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Justification {
    #[default]
    Beginning,
    Center,
    End,
}

/// One direction's layout rule (type) with its spacing or number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rule {
    #[serde(default)]
    pub layout: Layout,
    /// Fixed Distance, Maximum and Minimum Spacing (metres).
    #[serde(default)]
    pub spacing: f64,
    /// Fixed Number: the interior grid lines.
    #[serde(default)]
    pub number: u32,
}

/// The positions of the interior grid lines over a length `d` (ARC-CW-003), from 0.
pub fn solve(rule: Rule, d: f64, justification: Justification, offset: f64) -> Vec<f64> {
    if d <= TOL {
        return Vec::new();
    }
    let inside = |x: f64| x > TOL && x < d - TOL;
    match rule.layout {
        Layout::None => Vec::new(),
        Layout::FixedDistance => {
            let s = rule.spacing;
            if s <= TOL {
                return Vec::new();
            }
            let n = (d / s).floor() as i64;
            let leftover = d - n as f64 * s;
            let start = match justification {
                Justification::Beginning => offset,
                Justification::Center => leftover / 2.0 + offset,
                Justification::End => d + offset,
            };
            // Every k for which the line is inside, from either end.
            let reach = (d / s).ceil() as i64 + 2;
            let mut out: Vec<f64> = (-reach..=reach)
                .map(|k| start + k as f64 * s)
                .filter(|x| inside(*x))
                .collect();
            out.sort_by(f64::total_cmp);
            out
        }
        Layout::FixedNumber => (1..=rule.number.min(200))
            .map(|k| d * f64::from(k) / f64::from(rule.number + 1))
            .collect(),
        Layout::MaximumSpacing | Layout::MinimumSpacing => {
            let s = rule.spacing;
            if s <= TOL {
                return Vec::new();
            }
            let m = if rule.layout == Layout::MaximumSpacing {
                (d / s - 1e-9).ceil().max(1.0)
            } else {
                (d / s + 1e-9).floor().max(1.0)
            } as u32;
            (1..m).map(|k| d * f64::from(k) / f64::from(m)).collect()
        }
    }
}

/// A panel kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    #[default]
    Glazed,
    Solid,
    Empty,
}

/// A mullion's section: across the wall's face (`width`) and through it (`depth`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Mullion {
    pub width: f64,
    pub depth: f64,
}

/// The stored recipe of a curtain wall type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CurtainType {
    pub kind: String,
    /// Vertical grid lines, along the wall.
    #[serde(default)]
    pub u: Rule,
    /// Horizontal grid lines, up the wall.
    #[serde(default)]
    pub v: Rule,
    /// Mullions on interior grid lines; none when absent.
    #[serde(default)]
    pub interior: Option<Mullion>,
    /// Mullions round the border; none when absent.
    #[serde(default)]
    pub border: Option<Mullion>,
    #[serde(default)]
    pub panel: Panel,
    pub panel_thickness: f64,
}

impl Default for CurtainType {
    /// A storefront: vertical lines at most 1.5 m apart, a transom 2.4 m up, 50 × 150 mm
    /// mullions, glazed panels.
    fn default() -> Self {
        Self {
            kind: CURTAIN_TYPE_RECIPE.into(),
            u: Rule {
                layout: Layout::MaximumSpacing,
                spacing: 1.5,
                number: 0,
            },
            v: Rule {
                layout: Layout::FixedDistance,
                spacing: 2.4,
                number: 0,
            },
            interior: Some(Mullion {
                width: 0.05,
                depth: 0.15,
            }),
            border: Some(Mullion {
                width: 0.05,
                depth: 0.15,
            }),
            panel: Panel::Glazed,
            panel_thickness: 0.025,
        }
    }
}

pub fn curtain_type(e: &Element) -> Option<CurtainType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == CURTAIN_TYPE_RECIPE)
        .then(|| serde_json::from_value(v.clone()).ok())?
}

/// A direction's instance settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Placement {
    #[serde(default)]
    pub justification: Justification,
    #[serde(default)]
    pub offset: f64,
}

/// The stored recipe of a curtain wall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CurtainRecipe {
    pub kind: String,
    pub curve: Line,
    #[serde(flatten)]
    pub extent: Extent,
    #[serde(default)]
    pub u: Placement,
    #[serde(default)]
    pub v: Placement,
}

pub fn curtain_recipe(e: &Element) -> Option<CurtainRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == CURTAIN_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// One part of a curtain wall: a panel or a mullion, as a box in the wall's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct CurtainPart {
    pub mullion: bool,
    /// Along the wall from its start, through it (+ toward its left, the exterior), and up
    /// (absolute).
    pub x: [f64; 2],
    pub y: [f64; 2],
    pub z: [f64; 2],
}

/// A curtain wall as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct CurtainGeometry {
    pub ty: CurtainType,
    pub line: Line,
    pub z: [f64; 2],
    /// The grid lines along the wall and up it (from its start and base).
    pub u_lines: Vec<f64>,
    pub v_lines: Vec<f64>,
    pub parts: Vec<CurtainPart>,
}

impl CurtainGeometry {
    pub fn length(&self) -> f64 {
        self.line.length()
    }

    fn frame(&self) -> ([f64; 2], [f64; 2], [f64; 2]) {
        let l = self.length().max(f64::EPSILON);
        let (a, b) = (self.line.start, self.line.end);
        let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
        (a, u, [-u[1], u[0]])
    }

    /// A part's closed mesh in project coordinates.
    pub fn mesh(&self, p: &CurtainPart) -> crate::bodies::Mesh {
        let (o, u, n) = self.frame();
        let at = |x: f64, y: f64| [o[0] + u[0] * x + n[0] * y, o[1] + u[1] * x + n[1] * y];
        let f = [
            at(p.x[0], p.y[0]),
            at(p.x[1], p.y[0]),
            at(p.x[1], p.y[1]),
            at(p.x[0], p.y[1]),
        ];
        crate::walls::prism(&f, p.z[0], p.z[1])
    }

    /// The plan outline: the deepest part's band over the wall's length.
    pub fn footprint(&self) -> [[f64; 2]; 4] {
        let half = self
            .parts
            .iter()
            .map(|p| p.y[1].abs().max(p.y[0].abs()))
            .fold(0.0, f64::max);
        let (o, u, n) = self.frame();
        let l = self.length();
        let at = |x: f64, y: f64| [o[0] + u[0] * x + n[0] * y, o[1] + u[1] * x + n[1] * y];
        [at(0.0, -half), at(l, -half), at(l, half), at(0.0, half)]
    }
}

fn type_of(store: &Store, e: &Element) -> CurtainType {
    e.param("@type")
        .and_then(Value::as_str)
        .and_then(|g| find(store, g))
        .and_then(|t| store.get(t))
        .and_then(curtain_type)
        .unwrap_or_default()
}

/// Regenerates a curtain wall: its grid lines, panels and mullions.
pub fn regenerate(store: &Store, e: &Element) -> Result<CurtainGeometry> {
    let r = curtain_recipe(e)
        .ok_or_else(|| Error::new("curtain.not_reviera", "not a Reviera curtain wall"))?;
    let ty = type_of(store, e);
    let (z0, z1) = r.extent.z(store)?;
    let length = r.curve.length();
    if length < crate::walls::TOL_SHORT {
        return Err(Error::new(
            "curtain.too_short",
            "the curtain wall is too short",
        ));
    }
    let u_lines = solve(ty.u, length, r.u.justification, r.u.offset);
    let v_lines = solve(ty.v, z1 - z0, r.v.justification, r.v.offset);
    let mut parts = Vec::new();
    let half = |m: Option<Mullion>| m.map_or(0.0, |m| m.width / 2.0);
    let (bw, iw) = (half(ty.border), half(ty.interior));
    // The panels: each cell between grid lines, inside the mullions.
    if ty.panel != Panel::Empty {
        let t = ty.panel_thickness.max(0.001) / 2.0;
        let mut xs = vec![0.0];
        xs.extend(u_lines.iter().copied());
        xs.push(length);
        let mut zs = vec![0.0];
        zs.extend(v_lines.iter().copied());
        zs.push(z1 - z0);
        for i in 0..xs.len() - 1 {
            for j in 0..zs.len() - 1 {
                let x0 = xs[i] + if i == 0 { bw } else { iw };
                let x1 = xs[i + 1] - if i + 2 == xs.len() { bw } else { iw };
                let za = zs[j] + if j == 0 { bw } else { iw };
                let zb = zs[j + 1] - if j + 2 == zs.len() { bw } else { iw };
                if x1 - x0 > TOL && zb - za > TOL {
                    parts.push(CurtainPart {
                        mullion: false,
                        x: [x0, x1],
                        y: [-t, t],
                        z: [z0 + za, z0 + zb],
                    });
                }
            }
        }
    }
    // The mullions: vertical ones run full height; horizontal ones between them.
    let mut verticals: Vec<(f64, Mullion)> = Vec::new();
    if let Some(m) = ty.border {
        verticals.push((m.width / 2.0, m));
        verticals.push((length - m.width / 2.0, m));
    }
    if let Some(m) = ty.interior {
        verticals.extend(u_lines.iter().map(|x| (*x, m)));
    }
    verticals.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (x, m) in &verticals {
        parts.push(CurtainPart {
            mullion: true,
            x: [x - m.width / 2.0, x + m.width / 2.0],
            y: [-m.depth / 2.0, m.depth / 2.0],
            z: [z0, z1],
        });
    }
    let mut horizontals: Vec<(f64, Mullion)> = Vec::new();
    if let Some(m) = ty.border {
        horizontals.push((m.width / 2.0, m));
        horizontals.push((z1 - z0 - m.width / 2.0, m));
    }
    if let Some(m) = ty.interior {
        horizontals.extend(v_lines.iter().map(|z| (*z, m)));
    }
    for (z, m) in horizontals {
        // Between each pair of neighbouring verticals (or the ends).
        let mut stops: Vec<f64> = vec![0.0];
        stops.extend(
            verticals
                .iter()
                .flat_map(|(x, vm)| [x - vm.width / 2.0, x + vm.width / 2.0]),
        );
        stops.push(length);
        stops.sort_by(f64::total_cmp);
        for pair in stops.chunks(2) {
            let [a, b] = [pair[0], *pair.get(1).unwrap_or(&length)];
            if b - a > TOL {
                parts.push(CurtainPart {
                    mullion: true,
                    x: [a, b],
                    y: [-m.depth / 2.0, m.depth / 2.0],
                    z: [z0 + z - m.width / 2.0, z0 + z + m.width / 2.0],
                });
            }
        }
    }
    Ok(CurtainGeometry {
        ty,
        line: r.curve,
        z: [z0, z1],
        u_lines,
        v_lines,
        parts,
    })
}

/// Every Reviera curtain wall that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, CurtainGeometry)> {
    store
        .iter()
        .filter(|e| curtain_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

/// The openings curtain walls cut in the basic walls they are drawn in (ARC-WALL-013): the wall,
/// the curtain wall, and the span along the wall and the heights, where a curtain wall runs
/// along the wall's location line (within half its width) and overlaps it.
pub fn embedded_cuts(store: &Store) -> Vec<(ElementId, ElementId, crate::walls::openings::Cut)> {
    let curtains = all(store);
    if curtains.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for w in store.iter() {
        let Some(wr) = crate::walls::wall_recipe(w) else {
            continue;
        };
        if !wr.through.is_empty() {
            continue;
        }
        let Ok(g) = crate::walls::regenerate(store, w) else {
            continue;
        };
        let (u, n, s) = (g.direction(), g.normal(), g.start);
        for (cid, c) in &curtains {
            let along = |p: [f64; 2]| (p[0] - s[0]) * u[0] + (p[1] - s[1]) * u[1];
            let across = |p: [f64; 2]| (p[0] - s[0]) * n[0] + (p[1] - s[1]) * n[1];
            let (a, b) = (c.line.start, c.line.end);
            if across(a).abs() > g.width / 2.0 + 1e-6 || across(b).abs() > g.width / 2.0 + 1e-6 {
                continue;
            }
            let (x0, x1) = (along(a).min(along(b)), along(a).max(along(b)));
            let (x0, x1) = (x0.max(0.0), x1.min(g.length()));
            let (z0, z1) = (c.z[0].max(g.base_z), c.z[1].min(g.base_z + g.height));
            if x1 - x0 > 0.01 && z1 - z0 > 0.01 {
                out.push((
                    w.id(),
                    *cid,
                    crate::walls::openings::Cut::rect([x0, x1], [z0, z1]),
                ));
            }
        }
    }
    out
}

/// The project's default curtain wall type, made on first use.
fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn
        .store()
        .iter()
        .filter(|e| curtain_type(e).is_some())
        .min_by_key(|e| e.name.as_deref() != Some(DEFAULT_TYPE_NAME))
    {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new("IfcCurtainWallType")
            .name(DEFAULT_TYPE_NAME)
            .param("@PredefinedType", "NOTDEFINED")
            .param(
                RECIPE_KEY,
                serde_json::to_value(CurtainType::default()).expect("serialises"),
            ),
    )
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, r: &CurtainRecipe) -> Result<()> {
    let value = serde_json::to_value(r).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })?;
    let store = txn.store().clone();
    regenerate(&store, store.get(id).expect("exists")).map(|_| ())
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// A curtain wall type; omitted, the default storefront, made on first use.
    pub type_id: Option<ElementRef>,
    /// The base line in plan (straight), metres.
    pub curve: Line,
    pub base_level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: f64,
    pub top_constraint: TopConstraint,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: f64,
}

fn resolve_type(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such curtain wall type"))?;
    if curtain_type(store.get(id).expect("resolved")).is_none() {
        return Err(Error::new(
            "curtain.not_a_type",
            format!("element {id} is not a curtain wall type"),
        ));
    }
    Ok(id)
}

/// Makes a curtain wall (`curtain.create`).
pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let type_id = match &p.type_id {
        Some(r) => resolve_type(&store, r)?,
        None => default_type(txn)?,
    };
    let store = txn.store().clone();
    let extent = Extent::from_args(
        &store,
        &p.base_level,
        p.base_offset,
        p.top_constraint,
        p.top_offset,
    )?;
    let recipe = CurtainRecipe {
        kind: CURTAIN_RECIPE.into(),
        curve: p.curve,
        extent,
        u: Placement::default(),
        v: Placement::default(),
    };
    let guid = store.get(type_id).expect("resolved").guid().to_string();
    let id = txn.create(
        NewElement::new("IfcCurtainWall")
            .param("@PredefinedType", "NOTDEFINED")
            .param("@type", guid)
            .param(
                RECIPE_KEY,
                serde_json::to_value(&recipe).expect("serialises"),
            ),
    )?;
    let store = txn.store().clone();
    regenerate(&store, store.get(id).expect("created"))?;
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub curtain_ids: Vec<ElementRef>,
    pub curve: Option<Line>,
    pub base_level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<f64>,
    pub top_constraint: Option<TopConstraint>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: Option<f64>,
    /// Where the vertical (`u_*`) and horizontal (`v_*`) Fixed Distance lines start, and an
    /// offset.
    pub u_justification: Option<Justification>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub u_offset: Option<f64>,
    pub v_justification: Option<Justification>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub v_offset: Option<f64>,
}

/// Changes curtain walls (`curtain.set`); omitted values are kept.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.curtain_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such curtain wall"))?;
        let mut c = curtain_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
            Error::new(
                "curtain.not_reviera",
                format!("element {id} is not a Reviera curtain wall"),
            )
        })?;
        crate::modify::check_unpinned(&store, id)?;
        if let Some(l) = p.curve {
            c.curve = l;
        }
        let base = match &p.base_level {
            Some(b) => b.clone(),
            None => ElementRef::Id(find(&store, &c.extent.base_level).ok_or_else(|| {
                Error::new("level.missing", "the curtain wall's base level is gone")
            })?),
        };
        let top = match p.top_constraint {
            Some(t) => t,
            None => match c.extent.top_level().and_then(|g| find(&store, g)) {
                Some(l) => TopConstraint::Level(crate::walls::ElementRefValue::Id(l)),
                None => {
                    TopConstraint::Unconnected(c.extent.top["unconnected"].as_f64().unwrap_or(3.0))
                }
            },
        };
        c.extent = Extent::from_args(
            &store,
            &base,
            p.base_offset.unwrap_or(c.extent.base_offset),
            top,
            p.top_offset.unwrap_or(c.extent.top_offset),
        )?;
        if let Some(j) = p.u_justification {
            c.u.justification = j;
        }
        if let Some(o) = p.u_offset {
            c.u.offset = o;
        }
        if let Some(j) = p.v_justification {
            c.v.justification = j;
        }
        if let Some(o) = p.v_offset {
            c.v.offset = o;
        }
        write(txn, id, &c)?;
    }
    Ok(())
}

/// Moves, rotates or mirrors a curtain wall's base line by `x`, up by `dz`.
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    mirror: bool,
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = curtain_recipe(store.get(id).expect("exists")).expect("a curtain wall");
    let (a, b) = (x(r.curve.start), x(r.curve.end));
    // Mirrored, the line runs the other way so its exterior stays outside.
    r.curve = if mirror {
        Line { start: b, end: a }
    } else {
        Line { start: a, end: b }
    };
    r.extent.base_offset += dz;
    if r.extent.top_level().is_some() {
        r.extent.top_offset += dz;
    }
    write(txn, id, &r)
}

/// Sets a curtain wall type's value (`type.set_param`): `u_layout`, `u_spacing`, `u_number`,
/// `v_layout`, `v_spacing`, `v_number`, `mullion_width`, `mullion_depth` (both interior and
/// border; 0 removes them), `border_mullions` (true or false), `panel` (glazed, solid, empty),
/// `panel_thickness`.
pub(crate) fn set_type_param(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    param: &str,
    value: &Value,
) -> Result<()> {
    let store = txn.store().clone();
    let mut t = curtain_type(store.get(id).expect("checked")).expect("a curtain type");
    let bad = |what: &str| Error::new("type.invalid_value", format!("{param}: {what}"));
    let length = |v: &Value| -> Result<f64> {
        let x = match v {
            Value::Number(n) => n.as_f64().unwrap_or(-1.0),
            Value::String(s) => {
                crate::units::parse(s, "length", &crate::units::settings_of(&store))?
            }
            _ => -1.0,
        };
        if x >= 0.0 && x.is_finite() {
            Ok(x)
        } else {
            Err(bad("a length of 0 or more"))
        }
    };
    let layout = |v: &Value| -> Result<Layout> {
        serde_json::from_value(v.clone()).map_err(|_| {
            bad("none, fixed_distance, fixed_number, maximum_spacing or minimum_spacing")
        })
    };
    let mullion = |m: Option<Mullion>| {
        m.unwrap_or(Mullion {
            width: 0.05,
            depth: 0.15,
        })
    };
    match param {
        "u_layout" => t.u.layout = layout(value)?,
        "v_layout" => t.v.layout = layout(value)?,
        "u_spacing" => t.u.spacing = length(value)?,
        "v_spacing" => t.v.spacing = length(value)?,
        "u_number" | "v_number" => {
            let n = value
                .as_u64()
                .or_else(|| value.as_str()?.trim().parse().ok())
                .filter(|n| *n <= 200)
                .ok_or_else(|| bad("a number from 0 to 200"))? as u32;
            if param == "u_number" {
                t.u.number = n;
            } else {
                t.v.number = n;
            }
        }
        "mullion_width" | "mullion_depth" => {
            let x = length(value)?;
            let border = t.border.is_some();
            let mut m = mullion(t.interior.or(t.border));
            if param == "mullion_width" {
                m.width = x;
            } else {
                m.depth = x;
            }
            let none = m.width <= 0.0 || m.depth <= 0.0;
            t.interior = (!none).then_some(m);
            t.border = (!none && border).then_some(m);
        }
        "border_mullions" => {
            let on = value
                .as_bool()
                .or_else(|| match value.as_str() {
                    Some("true" | "yes" | "1") => Some(true),
                    Some("false" | "no" | "0") => Some(false),
                    _ => None,
                })
                .ok_or_else(|| bad("true or false"))?;
            t.border = on.then(|| mullion(t.interior));
        }
        "panel" => {
            t.panel =
                serde_json::from_value(value.clone()).map_err(|_| bad("glazed, solid or empty"))?;
        }
        "panel_thickness" => {
            let x = length(value)?;
            if x <= 0.0 {
                return Err(bad("greater than 0"));
            }
            t.panel_thickness = x;
        }
        _ => {
            let value = (!value.is_null()).then_some(value);
            return crate::parameters::set(txn, &[id], param, value).map(|_| ());
        }
    }
    let v = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn the_layout_solver_follows_the_specification() {
        let fixed = Rule {
            layout: Layout::FixedDistance,
            spacing: 1.5,
            number: 0,
        };
        // Beginning: 1.5 … 9.0 with 1.0 left at the end.
        let b = solve(fixed, 10.0, Justification::Beginning, 0.0);
        assert!(close(&b, &[1.5, 3.0, 4.5, 6.0, 7.5, 9.0]), "{b:?}");
        // Center: six full panels and 0.5 m at each end.
        let c = solve(fixed, 10.0, Justification::Center, 0.0);
        assert!(close(&c, &[0.5, 2.0, 3.5, 5.0, 6.5, 8.0, 9.5]), "{c:?}");
        // End: from the end back.
        let e = solve(fixed, 10.0, Justification::End, 0.0);
        assert!((e[0] - 1.0).abs() < 1e-9 && (e.last().unwrap() - 8.5).abs() < 1e-9);
        // Maximum Spacing 1.2 on 10 m: 9 equal panels.
        let m = solve(
            Rule {
                layout: Layout::MaximumSpacing,
                spacing: 1.2,
                number: 0,
            },
            10.0,
            Justification::Beginning,
            0.0,
        );
        assert_eq!(m.len(), 8);
        assert!((m[0] - 10.0 / 9.0).abs() < 1e-9);
        // Minimum Spacing 1.2 on 10 m: 8 panels.
        let n = solve(
            Rule {
                layout: Layout::MinimumSpacing,
                spacing: 1.2,
                number: 0,
            },
            10.0,
            Justification::Beginning,
            0.0,
        );
        assert_eq!(n.len(), 7);
        // Fixed Number: interior lines.
        let f = solve(
            Rule {
                layout: Layout::FixedNumber,
                spacing: 0.0,
                number: 3,
            },
            6.0,
            Justification::Beginning,
            0.0,
        );
        assert!(close(&f, &[1.5, 3.0, 4.5]));
        assert!(solve(Rule::default(), 6.0, Justification::Beginning, 0.0).is_empty());
    }
}
