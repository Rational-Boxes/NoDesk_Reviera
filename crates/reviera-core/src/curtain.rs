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
//! the wall (ARC-WALL-013).
//!
//! **Attached tops** (`wall.attach`; ARC-GLZ-001): a curtain wall's top can follow the
//! underside of one or more roofs over its base line (a gable end, or a multispan greenhouse's
//! end wall under several roofs). Its grid is laid out up to the highest point of that profile;
//! panels and mullions are clipped to it ([`CurtainPart::outline`]) and the top border mullion
//! follows it. In IFC the curtain wall has its base line as its axis and aggregates
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
    /// A curtain-wall door filling the cell (ARC-CW-006).
    Door,
    /// An opening pane: a vent (ARC-GLZ-004), top-hung, drawn open.
    Vent,
}

/// A grid direction: `u` lines are vertical, along the wall; `v` lines horizontal, up it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Dir {
    U,
    V,
}

/// A grid line placed by hand (ARC-CW-004).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ManualGrid {
    pub key: u32,
    pub dir: Dir,
    /// From the wall's start (`u`) or base (`v`), metres.
    pub at: f64,
}

/// A removed segment of a grid line: the line by direction and position, and the segment
/// between the crossing lines (0 from the start or base).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentOff {
    pub dir: Dir,
    pub at: f64,
    pub segment: usize,
}

/// A cell's panel instead of the type's (ARC-CW-006): the cell by column (along) and row (up).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PanelOverride {
    pub cell: [usize; 2],
    pub panel: Panel,
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
    /// Grid lines placed by hand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grids: Vec<ManualGrid>,
    /// Grid line segments removed (their panels merge).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<SegmentOff>,
    /// Panels swapped cell by cell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<PanelOverride>,
    /// The roofs (GlobalIds) whose underside the top follows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attach_top: Vec<String>,
}

impl CurtainRecipe {
    fn off(&self, dir: Dir, at: f64, segment: usize) -> bool {
        self.removed
            .iter()
            .any(|o| o.dir == dir && (o.at - at).abs() < 1e-6 && o.segment == segment)
    }
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
    /// A panel's kind (mullions: the type's).
    pub panel: Panel,
    /// The cell (column, row) of a panel's lowest, first cell.
    pub cell: [usize; 2],
    /// Along the wall from its start, through it (+ toward its left, the exterior), and up
    /// (absolute).
    pub x: [f64; 2],
    pub y: [f64; 2],
    pub z: [f64; 2],
    /// Where an attached top clips the part: its outline in the wall's plane (along the wall,
    /// absolute height), counter-clockwise; `None` for the plain box.
    pub outline: Option<Vec<[f64; 2]>>,
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
        if let Some(outline) = &p.outline {
            return extrude_outline(outline, p.y, &|x, y, z| {
                let q = at(x, y);
                [q[0], q[1], z]
            });
        }
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
    let (z0, mut z1) = r.extent.z(store)?;
    let length = r.curve.length();
    // An attached top: the roofs' underside along the base line, the grid up to its highest point.
    let profile = top_profile(store, &r, length, z1).filter(|p| p.iter().any(|q| q[1] > z0 + TOL));
    if let Some(p) = &profile {
        z1 = p.iter().map(|q| q[1]).fold(f64::MIN, f64::max);
    }
    if length < crate::walls::TOL_SHORT {
        return Err(Error::new(
            "curtain.too_short",
            "the curtain wall is too short",
        ));
    }
    let height = z1 - z0;
    // The type's lines and the manual ones.
    let merge = |mut lines: Vec<f64>, dir: Dir, max: f64| {
        lines.extend(
            r.grids
                .iter()
                .filter(|g| g.dir == dir && g.at > TOL && g.at < max - TOL)
                .map(|g| g.at),
        );
        lines.sort_by(f64::total_cmp);
        lines.dedup_by(|a, b| (*a - *b).abs() < TOL);
        lines
    };
    let u_lines = merge(
        solve(ty.u, length, r.u.justification, r.u.offset),
        Dir::U,
        length,
    );
    let v_lines = merge(
        solve(ty.v, height, r.v.justification, r.v.offset),
        Dir::V,
        height,
    );
    let mut xs = vec![0.0];
    xs.extend(u_lines.iter().copied());
    xs.push(length);
    let mut zs = vec![0.0];
    zs.extend(v_lines.iter().copied());
    zs.push(height);
    let (nx, nz) = (xs.len() - 1, zs.len() - 1);
    // A vertical line between columns i and i+1 is open at row j; a horizontal line between
    // rows j and j+1 at column i.
    let v_open = |i: usize, j: usize| r.off(Dir::U, xs[i + 1], j);
    let h_open = |i: usize, j: usize| r.off(Dir::V, zs[j + 1], i);
    let mut parts = Vec::new();
    let half = |m: Option<Mullion>| m.map_or(0.0, |m| m.width / 2.0);
    let (bw, iw) = (half(ty.border), half(ty.interior));
    // Cells joined across removed segments form one panel.
    let mut group: Vec<usize> = (0..nx * nz).collect();
    fn root(g: &mut [usize], k: usize) -> usize {
        let mut k = k;
        while g[k] != k {
            g[k] = g[g[k]];
            k = g[k];
        }
        k
    }
    for i in 0..nx {
        for j in 0..nz {
            if i + 1 < nx && v_open(i, j) {
                let (a, b) = (
                    root(&mut group, i * nz + j),
                    root(&mut group, (i + 1) * nz + j),
                );
                group[a.max(b)] = a.min(b);
            }
            if j + 1 < nz && h_open(i, j) {
                let (a, b) = (
                    root(&mut group, i * nz + j),
                    root(&mut group, i * nz + j + 1),
                );
                group[a.max(b)] = a.min(b);
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<[usize; 2]>> = Default::default();
    for i in 0..nx {
        for j in 0..nz {
            let g = root(&mut group, i * nz + j);
            groups.entry(g).or_default().push([i, j]);
        }
    }
    let inset_x = |i: usize, left: bool| {
        if left {
            if i == 0 { bw } else { iw }
        } else if i + 1 == nx {
            bw
        } else {
            iw
        }
    };
    let inset_z = |j: usize, low: bool| {
        if low {
            if j == 0 { bw } else { iw }
        } else if j + 1 == nz {
            bw
        } else {
            iw
        }
    };
    for cells in groups.values() {
        let first = *cells.iter().min().expect("a cell");
        let kind = r
            .panels
            .iter()
            .rev()
            .find(|o| cells.contains(&o.cell))
            .map_or(ty.panel, |o| o.panel);
        if kind == Panel::Empty {
            continue;
        }
        let t = if kind == Panel::Door {
            0.025
        } else {
            ty.panel_thickness.max(0.001) / 2.0
        };
        let (i0, i1) = (
            cells.iter().map(|c| c[0]).min().expect("a cell"),
            cells.iter().map(|c| c[0]).max().expect("a cell"),
        );
        let (j0, j1) = (
            cells.iter().map(|c| c[1]).min().expect("a cell"),
            cells.iter().map(|c| c[1]).max().expect("a cell"),
        );
        let rectangular = (i1 - i0 + 1) * (j1 - j0 + 1) == cells.len();
        let boxes: Vec<([usize; 2], [usize; 2])> = if rectangular {
            vec![([i0, i1], [j0, j1])]
        } else {
            cells.iter().map(|c| ([c[0], c[0]], [c[1], c[1]])).collect()
        };
        for (ci, cj) in boxes {
            // A door stands on the cell's bottom: no sill mullion gap below it.
            let x0 = xs[ci[0]] + inset_x(ci[0], true);
            let x1 = xs[ci[1] + 1] - inset_x(ci[1], false);
            let za = zs[cj[0]]
                + if kind == Panel::Door && cj[0] == 0 {
                    0.0
                } else {
                    inset_z(cj[0], true)
                };
            let zb = zs[cj[1] + 1] - inset_z(cj[1], false);
            if x1 - x0 > TOL && zb - za > TOL {
                parts.push(CurtainPart {
                    mullion: false,
                    panel: kind,
                    cell: first,
                    x: [x0, x1],
                    y: [-t, t],
                    z: [z0 + za, z0 + zb],
                    outline: None,
                });
            }
        }
    }
    let mullion = |x: [f64; 2], m: Mullion, z: [f64; 2]| CurtainPart {
        mullion: true,
        panel: ty.panel,
        cell: [0, 0],
        x,
        y: [-m.depth / 2.0, m.depth / 2.0],
        z,
        outline: None,
    };
    // Vertical mullions: the borders full height, interior lines segment by segment.
    if let Some(m) = ty.border {
        for x in [m.width / 2.0, length - m.width / 2.0] {
            parts.push(mullion([x - m.width / 2.0, x + m.width / 2.0], m, [z0, z1]));
        }
    }
    if let Some(m) = ty.interior {
        for i in 0..nx.saturating_sub(1) {
            let x = xs[i + 1];
            for j in 0..nz {
                if !v_open(i, j) {
                    parts.push(mullion(
                        [x - m.width / 2.0, x + m.width / 2.0],
                        m,
                        [z0 + zs[j], z0 + zs[j + 1]],
                    ));
                }
            }
        }
    }
    // Horizontal mullions, cell by cell between the verticals.
    let mut horizontals: Vec<(Option<usize>, f64, Mullion)> = Vec::new();
    if let Some(m) = ty.border {
        horizontals.push((None, m.width / 2.0, m));
        if profile.is_none() {
            horizontals.push((None, height - m.width / 2.0, m));
        }
    }
    if let Some(m) = ty.interior {
        horizontals.extend((0..nz.saturating_sub(1)).map(|j| (Some(j), zs[j + 1], m)));
    }
    for (line, z, m) in horizontals {
        for i in 0..nx {
            if line.is_some_and(|j| h_open(i, j)) {
                continue;
            }
            let a = xs[i] + inset_x(i, true);
            let b = xs[i + 1] - inset_x(i, false);
            if b - a > TOL {
                parts.push(mullion(
                    [a, b],
                    m,
                    [z0 + z - m.width / 2.0, z0 + z + m.width / 2.0],
                ));
            }
        }
    }
    if let Some(top) = &profile {
        // Panels stop under the sloped border mullion; mullions at the profile itself.
        let band = ty.border.map_or(0.0, |m| m.width);
        parts = parts
            .into_iter()
            .filter_map(|p| {
                let under = if p.mullion { 0.0 } else { band };
                clip_to_profile(p, top, under)
            })
            .collect();
        if let Some(m) = ty.border {
            for w in top.windows(2) {
                let ([xa, za], [xb, zb]) = (w[0], w[1]);
                if xb - xa > TOL && za.max(zb) > z0 + m.width {
                    parts.push(CurtainPart {
                        mullion: true,
                        panel: ty.panel,
                        cell: [0, 0],
                        x: [xa, xb],
                        y: [-m.depth / 2.0, m.depth / 2.0],
                        z: [za.min(zb) - m.width, za.max(zb)],
                        outline: Some(vec![[xa, za - m.width], [xb, zb - m.width], [xb, zb], [xa, za]]),
                    });
                }
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

/// A door in a curtain wall (ARC-CW-006) made of the cells round `centre` along it: as many
/// columns as reach `width` and rows as reach `height`, merged into one door panel. Returns the
/// door's width and height.
pub(crate) fn make_door(txn: &mut crate::txn::Transaction<'_>, id: ElementId, centre: f64, width: f64, height: f64) -> Result<(f64, f64)> {
    let store = txn.store().clone();
    let e = store.get(id).expect("exists");
    let mut r = curtain_recipe(e).expect("a curtain wall");
    let g = regenerate(&store, e)?;
    let mut xs = vec![0.0];
    xs.extend(g.u_lines.iter().copied());
    xs.push(g.length());
    let mut zs = vec![0.0];
    zs.extend(g.v_lines.iter().copied());
    zs.push(g.z[1] - g.z[0]);
    let (nx, nz) = (xs.len() - 1, zs.len() - 1);
    let c = (0..nx).find(|i| xs[i + 1] > centre).unwrap_or(nx - 1);
    // Grow toward the side whose next column keeps the door most nearly centred.
    let (mut i0, mut i1) = (c, c);
    while xs[i1 + 1] - xs[i0] < width - 1e-9 && (i0 > 0 || i1 + 1 < nx) {
        let left = (i0 > 0).then(|| ((xs[i0 - 1] + xs[i1 + 1]) / 2.0 - centre).abs());
        let right = (i1 + 1 < nx).then(|| ((xs[i0] + xs[i1 + 2]) / 2.0 - centre).abs());
        match (left, right) {
            (Some(l), Some(rr)) if l < rr => i0 -= 1,
            (_, Some(_)) => i1 += 1,
            _ => i0 -= 1,
        }
    }
    let k = (0..nz).find(|j| zs[j + 1] >= height - 1e-9).unwrap_or(nz - 1);
    for i in i0..i1 {
        for segment in 0..=k {
            r.removed.push(SegmentOff { dir: Dir::U, at: xs[i + 1], segment });
        }
    }
    for j in 0..k {
        for segment in i0..=i1 {
            r.removed.push(SegmentOff { dir: Dir::V, at: zs[j + 1], segment });
        }
    }
    r.panels.retain(|o| !(o.cell[0] >= i0 && o.cell[0] <= i1 && o.cell[1] <= k));
    r.panels.push(PanelOverride { cell: [i0, 0], panel: Panel::Door });
    write(txn, id, &r)?;
    Ok((xs[i1 + 1] - xs[i0], zs[k + 1]))
}

/// Attaches a curtain wall's top to a roof as well as those it follows (`wall.attach`), or (with
/// `None`) detaches it from `only` roofs or all.
pub(crate) fn set_attached(txn: &mut crate::txn::Transaction<'_>, id: ElementId, add: Option<String>, only: Option<&[String]>) -> Result<()> {
    let mut r = curtain_recipe(txn.store().get(id).expect("exists")).expect("a curtain wall");
    match add {
        Some(g) if !r.attach_top.contains(&g) => r.attach_top.push(g),
        Some(_) => return Ok(()),
        None => r.attach_top.retain(|g| only.is_some_and(|o| !o.contains(g))),
    }
    write(txn, id, &r)?;
    if r.attach_top.is_empty() {
        return Ok(());
    }
    let store = txn.store();
    let length = r.curve.length();
    let (_, own) = r.extent.z(store)?;
    if top_profile(store, &r, length, own).is_none() {
        return Err(Error::new(
            "wall.attach_not_covered",
            format!("curtain wall {id}: no roof it is attached to covers it"),
        ));
    }
    Ok(())
}

/// The underside of the roofs a curtain wall's top is attached to, along its base line: points
/// (distance along the wall, absolute height) where the profile bends, from 0 to `length`.
/// Stretches no roof covers keep the wall's own top `own`. `None` when it is attached to nothing
/// that regenerates, or no roof covers any of it.
fn top_profile(store: &Store, r: &CurtainRecipe, length: f64, own: f64) -> Option<Vec<[f64; 2]>> {
    // Each roof with how far below its underside the top stops: a glazed roof's bars hang
    // below its plane by half their depth (measured square to the plane).
    let roofs: Vec<(crate::roofs::RoofGeometry, f64)> = r
        .attach_top
        .iter()
        .filter_map(|g| store.get(find(store, g)?))
        .filter_map(|e| {
            let g = crate::roofs::regenerate(store, e).ok()?;
            let bars = crate::roofs::glazing::of_roof(store, e).map_or(0.0, |t| {
                t.border.iter().chain(&t.interior).map(|m| m.depth).fold(0.0, f64::max) / 2.0
            });
            let steepest = g.faces.iter().map(|f| f.normal[2].abs()).fold(1.0, f64::min).max(0.1);
            Some((g, bars / steepest))
        })
        .collect();
    if roofs.is_empty() {
        return None;
    }
    let (a, b) = (r.curve.start, r.curve.end);
    let d = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
    let at = |x: f64| [a[0] + d[0] * x, a[1] + d[1] * x];
    // The faces are planar, so the profile bends only where the line crosses a face's edge.
    let mut xs = vec![0.0, length];
    for (g, _) in &roofs {
        for f in &g.faces {
            let ring: Vec<[f64; 2]> = f.bottom.iter().map(|q| [q[0], q[1]]).collect();
            for k in 0..ring.len() {
                let (p, q) = (ring[k], ring[(k + 1) % ring.len()]);
                let e = [q[0] - p[0], q[1] - p[1]];
                let den = d[0] * e[1] - d[1] * e[0];
                if den.abs() < 1e-12 {
                    continue;
                }
                let w = [p[0] - a[0], p[1] - a[1]];
                let t = (w[0] * e[1] - w[1] * e[0]) / den;
                let s = (w[0] * d[1] - w[1] * d[0]) / den;
                if (-1e-9..=1.0 + 1e-9).contains(&s) && t > 0.0 && t < length {
                    // Either side too, so the profile steps where a roof's cover ends.
                    xs.extend([t - 1e-4, t, t + 1e-4].into_iter().filter(|x| *x > 0.0 && *x < length));
                }
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|p, q| (*p - *q).abs() < 1e-6);
    // Between bends a stretch is wholly covered or not: its middle tells.
    let mids: Vec<f64> = xs.windows(2).map(|w| (w[0] + w[1]) / 2.0).collect();
    xs.extend(mids);
    xs.sort_by(f64::total_cmp);
    let mut out: Vec<[f64; 2]> = Vec::new();
    let mut covered = false;
    let under = |x: f64| roofs.iter().filter_map(|(g, drop)| Some(g.underside(at(x))? - drop)).reduce(f64::min);
    for x in xs {
        // Just inside each end, so a gable wall on the roof's edge is still covered, and
        // carried out to the end along the face.
        let probe = x.clamp(1e-5, length - 1e-5);
        let inward = if probe > x { 1e-5 } else { -1e-5 };
        let z = match (under(probe), under(probe + inward)) {
            (Some(z1), Some(z2)) if probe != x => Some(z1 + (z1 - z2) * (probe - x) / inward),
            (z, _) => z,
        };
        covered |= z.is_some();
        out.push([x, z.unwrap_or(own)]);
    }
    covered.then_some(out)
}

/// The profile's height at `x` (linear between its points).
fn profile_at(top: &[[f64; 2]], x: f64) -> f64 {
    let k = top.partition_point(|q| q[0] < x).clamp(1, top.len() - 1);
    let ([xa, za], [xb, zb]) = (top[k - 1], top[k]);
    if xb - xa < 1e-12 { za.min(zb) } else { za + (zb - za) * (x - xa) / (xb - xa) }
}

/// A part clipped to `under` below the profile: kept as it is when it fits, given an outline
/// when the profile cuts it, dropped when it lies above.
fn clip_to_profile(mut p: CurtainPart, top: &[[f64; 2]], under: f64) -> Option<CurtainPart> {
    let limit = |x: f64| profile_at(top, x) - under;
    let mut xs = vec![p.x[0]];
    xs.extend(top.iter().map(|q| q[0]).filter(|x| *x > p.x[0] + TOL && *x < p.x[1] - TOL));
    xs.push(p.x[1]);
    if xs.iter().all(|x| limit(*x) >= p.z[1] - 1e-9) {
        return Some(p);
    }
    if xs.iter().all(|x| limit(*x) <= p.z[0] + TOL) {
        return None;
    }
    // Where the limit crosses the part's top or bottom, the outline bends too.
    let mut cuts = xs.clone();
    for w in xs.windows(2) {
        let (fa, fb) = (limit(w[0]), limit(w[1]));
        for level in [p.z[0], p.z[1]] {
            if (fa - level) * (fb - level) < 0.0 {
                cuts.push(w[0] + (w[1] - w[0]) * (level - fa) / (fb - fa));
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    // Only the stretch where the part stands above its bottom.
    let tops: Vec<[f64; 2]> = cuts.iter().map(|x| [*x, limit(*x).clamp(p.z[0], p.z[1])]).collect();
    let first = tops.iter().position(|q| q[1] > p.z[0] + 1e-9)?;
    let last = tops.iter().rposition(|q| q[1] > p.z[0] + 1e-9)?;
    let (from, to) = (first.saturating_sub(1), (last + 1).min(tops.len() - 1));
    let run = &tops[from..=to];
    let mut outline = vec![[run[0][0], p.z[0]], [run[run.len() - 1][0], p.z[0]]];
    outline.extend(run.iter().rev().copied());
    outline.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    let area = outline.iter().zip(outline.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum::<f64>() / 2.0;
    if area.abs() < 1e-6 {
        return None;
    }
    p.x = [run[0][0], run[run.len() - 1][0]];
    p.z[1] = run.iter().map(|q| q[1]).fold(p.z[0], f64::max);
    p.outline = Some(outline);
    Some(p)
}

/// A closed mesh of an outline in a wall's plane (along, up), between two depths through it;
/// `at` places (along, through, up) in the project.
fn extrude_outline(outline: &[[f64; 2]], y: [f64; 2], at: &dyn Fn(f64, f64, f64) -> [f64; 3]) -> crate::bodies::Mesh {
    let n = outline.len();
    let mut tris: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(outline.iter().copied(), &[], &mut tris);
    let mut positions: Vec<[f64; 3]> = outline.iter().map(|q| at(q[0], y[0], q[1])).collect();
    positions.extend(outline.iter().map(|q| at(q[0], y[1], q[1])));
    let ccw = outline.iter().zip(outline.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum::<f64>() > 0.0;
    let mut indices: Vec<u32> = Vec::new();
    let nn = n as u32;
    for t in tris.chunks(3) {
        // The face at y[0] looks toward -y, the one at y[1] toward +y.
        let (a, b, c) = (t[0], t[1], t[2]);
        if ccw {
            indices.extend([a, b, c, a + nn, c + nn, b + nn]);
        } else {
            indices.extend([a, c, b, a + nn, b + nn, c + nn]);
        }
    }
    for k in 0..nn {
        let j = (k + 1) % nn;
        if ccw {
            indices.extend([k, j + nn, j, k, k + nn, j + nn]);
        } else {
            indices.extend([k, j, j + nn, k, j + nn, k + nn]);
        }
    }
    (positions, indices)
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
        grids: Vec::new(),
        removed: Vec::new(),
        panels: Vec::new(),
        attach_top: Vec::new(),
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

// ---- Manual grid lines, segments and panels (ARC-CW-004, ARC-CW-006) ----

fn editable(store: &Store, r: &ElementRef) -> Result<(ElementId, CurtainRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such curtain wall"))?;
    let c = curtain_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "curtain.not_reviera",
            format!("element {id} is not a Reviera curtain wall"),
        )
    })?;
    Ok((id, c))
}

/// How many segments a line has: the cells across it.
fn segment_count(store: &Store, id: ElementId, dir: Dir) -> Result<usize> {
    let g = regenerate(store, store.get(id).expect("exists"))?;
    Ok(match dir {
        Dir::U => g.v_lines.len() + 1,
        Dir::V => g.u_lines.len() + 1,
    })
}

/// Which segments of a new line are placed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GridMode {
    /// All segments.
    #[default]
    All,
    /// Only `segments[0]`.
    One,
    /// All but `segments`.
    AllExcept,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddGridParams {
    pub curtain_id: ElementRef,
    /// `u` a vertical line, `v` a horizontal one.
    pub direction: Dir,
    /// From the wall's start (`u`) or base (`v`), metres.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub position: f64,
    #[serde(default)]
    pub mode: GridMode,
    /// Segments for `one` and `all_except` (0 from the start or base).
    #[serde(default)]
    pub segments: Vec<usize>,
}

/// `curtain.add_grid`: a manual grid line; its key.
pub(crate) fn add_grid(txn: &mut crate::txn::Transaction<'_>, p: AddGridParams) -> Result<u32> {
    let store = txn.store().clone();
    let (id, mut c) = editable(&store, &p.curtain_id)?;
    crate::modify::check_unpinned(&store, id)?;
    let g = regenerate(&store, store.get(id).expect("resolved"))?;
    let max = match p.direction {
        Dir::U => g.length(),
        Dir::V => g.z[1] - g.z[0],
    };
    if !(p.position > TOL && p.position < max - TOL) {
        return Err(Error::new(
            "curtain.grid_outside",
            "the grid line must be inside the curtain wall",
        ));
    }
    let key = c.grids.iter().map(|g| g.key + 1).max().unwrap_or(1);
    c.grids.push(ManualGrid {
        key,
        dir: p.direction,
        at: p.position,
    });
    let n = segment_count(&store, id, p.direction)?;
    let off: Vec<usize> = match p.mode {
        GridMode::All => Vec::new(),
        GridMode::One => {
            let keep = *p
                .segments
                .first()
                .ok_or_else(|| Error::new("curtain.segment", "give the segment to place"))?;
            (0..n).filter(|s| *s != keep).collect()
        }
        GridMode::AllExcept => p.segments.clone(),
    };
    for segment in off {
        c.removed.push(SegmentOff {
            dir: p.direction,
            at: p.position,
            segment,
        });
    }
    write(txn, id, &c)?;
    Ok(key)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SegmentsParams {
    pub curtain_id: ElementRef,
    pub direction: Dir,
    /// The line's position (a type's line or a manual one).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub position: f64,
    /// Each segment on (true) or removed, from the start or base.
    pub segments_on: Vec<bool>,
}

/// `curtain.set_grid_segments`: removed segments merge the panels on either side.
pub(crate) fn set_grid_segments(
    txn: &mut crate::txn::Transaction<'_>,
    p: SegmentsParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut c) = editable(&store, &p.curtain_id)?;
    let g = regenerate(&store, store.get(id).expect("resolved"))?;
    let lines = match p.direction {
        Dir::U => &g.u_lines,
        Dir::V => &g.v_lines,
    };
    let at = lines
        .iter()
        .copied()
        .find(|l| (l - p.position).abs() < 1e-4)
        .ok_or_else(|| Error::new("curtain.no_grid", "there is no grid line there"))?;
    c.removed
        .retain(|o| !(o.dir == p.direction && (o.at - at).abs() < 1e-6));
    for (segment, on) in p.segments_on.iter().enumerate() {
        if !on {
            c.removed.push(SegmentOff {
                dir: p.direction,
                at,
                segment,
            });
        }
    }
    write(txn, id, &c)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveGridParams {
    pub curtain_id: ElementRef,
    pub grid_key: u32,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub position: f64,
}

/// `curtain.move_grid`: a manual line to a new position, its removed segments with it.
pub(crate) fn move_grid(txn: &mut crate::txn::Transaction<'_>, p: MoveGridParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut c) = editable(&store, &p.curtain_id)?;
    let g = c
        .grids
        .iter_mut()
        .find(|g| g.key == p.grid_key)
        .ok_or_else(|| Error::new("curtain.no_grid", "no manual grid line with that key"))?;
    let (dir, old) = (g.dir, g.at);
    g.at = p.position;
    for o in c.removed.iter_mut() {
        if o.dir == dir && (o.at - old).abs() < 1e-6 {
            o.at = p.position;
        }
    }
    write(txn, id, &c)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteGridParams {
    pub curtain_id: ElementRef,
    pub grid_key: u32,
}

/// `curtain.delete_grid`: removes a manual line.
pub(crate) fn delete_grid(
    txn: &mut crate::txn::Transaction<'_>,
    p: DeleteGridParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut c) = editable(&store, &p.curtain_id)?;
    let Some(k) = c.grids.iter().position(|g| g.key == p.grid_key) else {
        return Err(Error::new(
            "curtain.no_grid",
            "no manual grid line with that key",
        ));
    };
    let g = c.grids.remove(k);
    c.removed
        .retain(|o| !(o.dir == g.dir && (o.at - g.at).abs() < 1e-6));
    write(txn, id, &c)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPanelParams {
    pub curtain_id: ElementRef,
    /// Cells by (column along the wall, row up it), from 0 at the start and base.
    pub cells: Vec<[usize; 2]>,
    /// `glazed`, `solid`, `empty` or `door` (a curtain-wall door); `null` returns to the type's.
    pub panel: Option<Panel>,
}

/// `curtain.set_panel_type`: swaps the panels of cells (ARC-CW-006).
pub(crate) fn set_panel_type(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetPanelParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut c) = editable(&store, &p.curtain_id)?;
    let g = regenerate(&store, store.get(id).expect("resolved"))?;
    let (nx, nz) = (g.u_lines.len() + 1, g.v_lines.len() + 1);
    for cell in &p.cells {
        if cell[0] >= nx || cell[1] >= nz {
            return Err(Error::new(
                "curtain.no_cell",
                format!("there is no cell {cell:?} ({nx} × {nz} cells)"),
            ));
        }
        c.panels.retain(|o| o.cell != *cell);
        if let Some(panel) = p.panel {
            c.panels.push(PanelOverride { cell: *cell, panel });
        }
    }
    write(txn, id, &c)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLayoutParams {
    pub curtain_id: ElementRef,
    /// The manual grid lines (keys are kept or given).
    pub grids: Vec<ManualGrid>,
    pub removed: Vec<SegmentOff>,
    pub panels: Vec<PanelOverride>,
}

/// `curtain.set_layout`: a curtain wall's manual lines, removed segments and swapped panels at
/// once (the window's Edit Grid).
pub(crate) fn set_layout(txn: &mut crate::txn::Transaction<'_>, p: SetLayoutParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut c) = editable(&store, &p.curtain_id)?;
    c.grids = p.grids;
    c.removed = p.removed;
    c.panels = p.panels;
    write(txn, id, &c)?;
    regenerate(txn.store(), txn.store().get(id).expect("exists")).map(|_| ())
}
