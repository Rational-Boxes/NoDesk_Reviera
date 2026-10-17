// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Columns: architectural ones (ARC-COL-001: non-structural columns, casings and pilasters) and
//! structural ones (STR-COL-001: load-bearing, with a structural material and a rolled, hollow or
//! cast section).
//!
//! A column is an `IfcColumn` with a `recipe.column`: its centre in plan, its rotation, and its
//! vertical extent between levels ([`Extent`]). Its type (`IfcColumnType` with a
//! `recipe.column_type`) gives its section: rectangular (width × depth), round (diameter), or a
//! structural section (I or H, channel, angle, tee, rectangular or round hollow) with its
//! thicknesses, and for structural types its material and designation (for example `W12x40`).
//!
//! Its top can be attached to a roof, floor or ceiling above it (`column.attach`): the top then
//! follows the target's underside over the column's section, at its lowest point (*minimum*), its
//! middle (*midline*) or its highest point (*maximum*), plus an offset; with *maximum* and
//! *midline* the top is cut by the target's underside, so it meets a sloped roof flush. The
//! column regenerates when the target changes; when the target goes, it takes its own top.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::levels::Extent;
use crate::store::Store;
use crate::walls::{RECIPE_KEY, TopConstraint};

pub const COLUMN_RECIPE: &str = "recipe.column";
pub const COLUMN_TYPE_RECIPE: &str = "recipe.column_type";
pub const DEFAULT_TYPE_NAME: &str = "Rectangular Column 400 x 400";
/// The default structural column type, made on first use.
pub const DEFAULT_STRUCTURAL_TYPE_NAME: &str = "Concrete Column 400 x 400";
/// Sides of a round column's section.
const ROUND_SIDES: usize = 32;

/// A column type's section. The structural ones are drawn without their root fillets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    #[default]
    Rectangular,
    Round,
    /// I or H: two flanges `width` wide on a web, `depth` overall (W, IPE, HEA, HEB, UB, UC).
    IShape,
    /// A channel: a web on the left, flanges reaching `width` to the right (C, UPN, PFC).
    Channel,
    /// An angle: legs `width` along x and `depth` along y, `wall_thickness` thick (L).
    Angle,
    /// A tee: a flange `width` wide at the top of a stem, `depth` overall (WT).
    Tee,
    /// A rectangular hollow section, `wall_thickness` thick (HSS, RHS, SHS).
    RectangularHollow,
    /// A round hollow section of diameter `width`, `wall_thickness` thick (HSS round, CHS, pipe).
    CircularHollow,
}

/// What a structural material is, for its behaviour in the model and in analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaterialKind {
    Concrete,
    Steel,
    PrecastConcrete,
    Wood,
    Other,
}

fn web() -> f64 {
    0.01
}
fn flange() -> f64 {
    0.015
}
fn wall() -> f64 {
    0.008
}

/// The stored recipe of a column type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnType {
    pub kind: String,
    #[serde(default)]
    pub section: Section,
    /// Across its local x (metres): a flange's width, a round section's diameter.
    pub width: f64,
    /// Across its local y (metres): the overall depth; ignored for round sections.
    pub depth: f64,
    /// I, channel and tee sections: the web's thickness.
    #[serde(default = "web")]
    pub web_thickness: f64,
    /// I, channel and tee sections: each flange's thickness.
    #[serde(default = "flange")]
    pub flange_thickness: f64,
    /// Hollow sections and angles: the wall's (leg's) thickness.
    #[serde(default = "wall")]
    pub wall_thickness: f64,
    /// Load-bearing: a structural column (STR-COL-001).
    #[serde(default)]
    pub structural: bool,
    /// The structural material's name (for example `Steel ASTM A992`, `Concrete C30/37`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_kind: Option<MaterialKind>,
    /// The section's standard designation, when it is one (for example `W12x40`, `HEB 300`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub designation: Option<String>,
}

impl Default for ColumnType {
    fn default() -> Self {
        Self {
            kind: COLUMN_TYPE_RECIPE.into(),
            section: Section::Rectangular,
            width: 0.4,
            depth: 0.4,
            web_thickness: web(),
            flange_thickness: flange(),
            wall_thickness: wall(),
            structural: false,
            material: None,
            material_kind: None,
            designation: None,
        }
    }
}

/// A section's properties about its centroid (metres): area, second moments about its local x and
/// y axes, radii of gyration, and the least radius of gyration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct SectionProperties {
    pub area: f64,
    pub ix: f64,
    pub iy: f64,
    pub rx: f64,
    pub ry: f64,
    pub r_min: f64,
}

fn circle(r: f64, ccw: bool) -> Vec<[f64; 2]> {
    let mut v: Vec<[f64; 2]> = (0..ROUND_SIDES)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / ROUND_SIDES as f64;
            [r * a.cos(), r * a.sin()]
        })
        .collect();
    if !ccw {
        v.reverse();
    }
    v
}

impl ColumnType {
    /// The section's outline about its centre, counter-clockwise (without its holes).
    pub fn outline(&self) -> Vec<[f64; 2]> {
        self.profile().0
    }

    /// The section about its centre: its outline, counter-clockwise, and its holes (hollow
    /// sections), clockwise.
    pub fn profile(&self) -> (Vec<[f64; 2]>, Vec<Vec<[f64; 2]>>) {
        let (b, h) = (self.width / 2.0, self.depth / 2.0);
        let (tw, tf, t) = (
            self.web_thickness,
            self.flange_thickness,
            self.wall_thickness,
        );
        match self.section {
            Section::Rectangular => (vec![[-b, -h], [b, -h], [b, h], [-b, h]], Vec::new()),
            Section::Round => (circle(b, true), Vec::new()),
            Section::IShape => {
                let w = tw / 2.0;
                (
                    vec![
                        [-b, -h],
                        [b, -h],
                        [b, -h + tf],
                        [w, -h + tf],
                        [w, h - tf],
                        [b, h - tf],
                        [b, h],
                        [-b, h],
                        [-b, h - tf],
                        [-w, h - tf],
                        [-w, -h + tf],
                        [-b, -h + tf],
                    ],
                    Vec::new(),
                )
            }
            Section::Channel => (
                vec![
                    [-b, -h],
                    [b, -h],
                    [b, -h + tf],
                    [-b + tw, -h + tf],
                    [-b + tw, h - tf],
                    [b, h - tf],
                    [b, h],
                    [-b, h],
                ],
                Vec::new(),
            ),
            Section::Angle => (
                vec![
                    [-b, -h],
                    [b, -h],
                    [b, -h + t],
                    [-b + t, -h + t],
                    [-b + t, h],
                    [-b, h],
                ],
                Vec::new(),
            ),
            Section::Tee => {
                let w = tw / 2.0;
                (
                    vec![
                        [-w, -h],
                        [w, -h],
                        [w, h - tf],
                        [b, h - tf],
                        [b, h],
                        [-b, h],
                        [-b, h - tf],
                        [-w, h - tf],
                    ],
                    Vec::new(),
                )
            }
            Section::RectangularHollow => (
                vec![[-b, -h], [b, -h], [b, h], [-b, h]],
                vec![vec![
                    [-b + t, -h + t],
                    [-b + t, h - t],
                    [b - t, h - t],
                    [b - t, -h + t],
                ]],
            ),
            Section::CircularHollow => (circle(b, true), vec![circle(b - t, false)]),
        }
    }

    /// The section's area (square metres), from its outline and holes.
    pub fn area(&self) -> f64 {
        let ring_area = |r: &[[f64; 2]]| {
            (0..r.len())
                .map(|i| {
                    let (a, b) = (r[i], r[(i + 1) % r.len()]);
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum::<f64>()
                .abs()
                / 2.0
        };
        let (outer, holes) = self.profile();
        ring_area(&outer) - holes.iter().map(|h| ring_area(h)).sum::<f64>()
    }

    /// Its section properties about its centroid, from its outline and holes (fillets left out):
    /// area, second moments about the local x and y axes, and radii of gyration.
    pub fn properties(&self) -> SectionProperties {
        let (outer, holes) = self.profile();
        // Green's theorem over each ring (signed: the outline counter-clockwise, holes clockwise).
        let mut a = 0.0;
        let (mut sx, mut sy) = (0.0, 0.0);
        let (mut ixx, mut iyy) = (0.0, 0.0);
        for ring in std::iter::once(&outer).chain(holes.iter()) {
            let n = ring.len();
            for i in 0..n {
                let ([x0, y0], [x1, y1]) = (ring[i], ring[(i + 1) % n]);
                let c = x0 * y1 - x1 * y0;
                a += c / 2.0;
                sx += (x0 + x1) * c / 6.0;
                sy += (y0 + y1) * c / 6.0;
                ixx += (y0 * y0 + y0 * y1 + y1 * y1) * c / 12.0;
                iyy += (x0 * x0 + x0 * x1 + x1 * x1) * c / 12.0;
            }
        }
        let (cx, cy) = (sx / a, sy / a);
        // About the centroid (parallel axes).
        let ix = ixx - a * cy * cy;
        let iy = iyy - a * cx * cx;
        let (rx, ry) = ((ix / a).sqrt(), (iy / a).sqrt());
        SectionProperties {
            area: a,
            ix,
            iy,
            rx,
            ry,
            r_min: rx.min(ry),
        }
    }

    /// Whether its sizes make a section: thicknesses positive and leaving room for the rest.
    pub fn check(&self) -> std::result::Result<(), String> {
        let positive = |v: f64, what: &str| {
            if v > 0.0 && v.is_finite() {
                Ok(())
            } else {
                Err(format!("the {what} must be greater than 0"))
            }
        };
        positive(self.width, "width")?;
        if self.section != Section::Round && self.section != Section::CircularHollow {
            positive(self.depth, "depth")?;
        }
        match self.section {
            Section::IShape | Section::Channel | Section::Tee => {
                positive(self.web_thickness, "web thickness")?;
                positive(self.flange_thickness, "flange thickness")?;
                if self.web_thickness >= self.width {
                    return Err("the web must be thinner than the flange is wide".into());
                }
                let flanges = if self.section == Section::Tee {
                    1.0
                } else {
                    2.0
                };
                if self.flange_thickness * flanges >= self.depth {
                    return Err("the flanges must be thinner than the section is deep".into());
                }
            }
            Section::Angle | Section::RectangularHollow | Section::CircularHollow => {
                positive(self.wall_thickness, "wall thickness")?;
                let limit = if self.section == Section::Angle {
                    self.width.min(self.depth)
                } else if self.section == Section::CircularHollow {
                    self.width / 2.0
                } else {
                    self.width.min(self.depth) / 2.0
                };
                if self.wall_thickness >= limit {
                    return Err("the wall is too thick for the section".into());
                }
            }
            Section::Rectangular | Section::Round => {}
        }
        Ok(())
    }
}

pub fn column_type(e: &Element) -> Option<ColumnType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == COLUMN_TYPE_RECIPE)
        .then(|| serde_json::from_value(v.clone()).ok())?
}

/// A column or beam type's section.
pub fn section_type(e: &Element) -> Option<ColumnType> {
    column_type(e).or_else(|| crate::beams::beam_type(e))
}

/// Where along a target's underside an attached top goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Justification {
    /// The lowest point of the underside over the section.
    Minimum,
    /// Its middle (between the lowest and highest).
    Midline,
    /// Its highest point; the top is cut by the underside.
    #[default]
    Maximum,
}

/// A column top attached to a roof, floor or ceiling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Attach {
    /// The target (GlobalId).
    pub target: String,
    #[serde(default)]
    pub justification: Justification,
    #[serde(default)]
    pub offset: f64,
}

/// The stored recipe of a column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnRecipe {
    pub kind: String,
    /// Its centre in plan (metres).
    pub point: [f64; 2],
    /// Counter-clockwise from the x axis (radians).
    #[serde(default)]
    pub rotation: f64,
    #[serde(flatten)]
    pub extent: Extent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_top: Option<Attach>,
}

pub fn column_recipe(e: &Element) -> Option<ColumnRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == COLUMN_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// A column as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnGeometry {
    pub ty: ColumnType,
    /// The section's outline in plan, counter-clockwise.
    pub ring: Vec<[f64; 2]>,
    /// Its holes (hollow sections), clockwise.
    pub holes: Vec<Vec<[f64; 2]>>,
    pub z0: f64,
    /// The top's height at each corner of `ring`.
    pub tops: Vec<f64>,
    /// The top's height at each corner of each hole.
    pub hole_tops: Vec<Vec<f64>>,
}

impl ColumnGeometry {
    /// The closed body: bottom, top (following an attached target) and sides, holes through.
    pub fn mesh(&self) -> crate::bodies::Mesh {
        let key = |p: [f64; 2]| ((p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64);
        let mut heights = std::collections::HashMap::new();
        for (p, z) in self.ring.iter().zip(&self.tops) {
            heights.insert(key(*p), *z);
        }
        for (h, zs) in self.holes.iter().zip(&self.hole_tops) {
            for (p, z) in h.iter().zip(zs) {
                heights.insert(key(*p), *z);
            }
        }
        let top = self.top();
        crate::bodies::extrude_between(&self.ring, &self.holes, &|_| self.z0, &|p| {
            heights.get(&key(p)).copied().unwrap_or(top)
        })
    }

    /// The highest point of its top.
    pub fn top(&self) -> f64 {
        self.tops.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    }
}

/// The underside of a roof, floor or ceiling (GlobalId) over plan point `p`, if it covers it.
fn underside(store: &Store, target: &str, p: [f64; 2]) -> Option<f64> {
    let e = store.get(find(store, target)?)?;
    if crate::roofs::roof_recipe(e).is_some() {
        // The lowest of the roof over `p`: its faces' underside, or an eave's fascia or soffit.
        let g = crate::roofs::regenerate(store, e).ok()?;
        let mut z = g.underside(p);
        for t in &g.trims {
            let (pts, idx) = &t.mesh;
            for tri in idx.chunks(3) {
                let [a, b, c] = [
                    pts[tri[0] as usize],
                    pts[tri[1] as usize],
                    pts[tri[2] as usize],
                ];
                if let Some(h) = height_in(p, a, b, c) {
                    z = Some(z.map_or(h, |z: f64| z.min(h)));
                }
            }
        }
        return z;
    }
    let f = crate::floors::regenerate(store, e).ok()?;
    f.profiles
        .iter()
        .any(|pr| inside(p, &pr.outer) && !pr.holes.iter().any(|h| inside(p, h)))
        .then_some(f.top - f.thickness)
}

/// The height over plan point `p` of triangle `a b c`, if `p` is inside its plan (and it is not
/// vertical).
fn height_in(p: [f64; 2], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<f64> {
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1e-12 {
        return None;
    }
    let l1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
    let l2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
    let l3 = 1.0 - l1 - l2;
    (l1 >= -1e-9 && l2 >= -1e-9 && l3 >= -1e-9).then(|| l1 * a[2] + l2 * b[2] + l3 * c[2])
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

fn type_of(store: &Store, e: &Element) -> ColumnType {
    e.param("@type")
        .and_then(Value::as_str)
        .and_then(|g| find(store, g))
        .and_then(|t| store.get(t))
        .and_then(column_type)
        .unwrap_or_default()
}

/// The roof, floor or ceiling directly over a column (the lowest underside above its base over
/// its centre), to attach its top to.
pub fn target_above(store: &Store, column: ElementId) -> Option<ElementId> {
    let e = store.get(column)?;
    let r = column_recipe(e)?;
    let (z0, _) = r.extent.z(store).ok()?;
    let candidates = crate::roofs::all(store)
        .into_iter()
        .map(|(id, _)| id)
        .chain(crate::floors::all(store).into_iter().map(|(id, _)| id));
    candidates
        .filter_map(|id| {
            let guid = store.get(id)?.guid().to_string();
            let z = underside(store, &guid, r.point)?;
            (z > z0 + 0.1).then_some((id, z))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Regenerates a column from its recipe, type, levels and attach target.
pub fn regenerate(store: &Store, e: &Element) -> Result<ColumnGeometry> {
    let r =
        column_recipe(e).ok_or_else(|| Error::new("column.not_reviera", "not a Reviera column"))?;
    let ty = type_of(store, e);
    let (s, c) = r.rotation.sin_cos();
    let place = |pts: Vec<[f64; 2]>| -> Vec<[f64; 2]> {
        pts.into_iter()
            .map(|[x, y]| [r.point[0] + x * c - y * s, r.point[1] + x * s + y * c])
            .collect()
    };
    let (outline, holes) = ty.profile();
    let ring = place(outline);
    let holes: Vec<Vec<[f64; 2]>> = holes.into_iter().map(place).collect();
    let (z0, z1) = r.extent.z(store)?;
    let mut tops = vec![z1; ring.len()];
    let mut hole_tops: Vec<Vec<f64>> = holes.iter().map(|h| vec![z1; h.len()]).collect();
    if let Some(a) = &r.attach_top {
        // The underside over the corners and the centre.
        let mut samples: Vec<[f64; 2]> = ring.clone();
        samples.push(r.point);
        let heights: Vec<Option<f64>> = samples
            .iter()
            .map(|p| underside(store, &a.target, *p))
            .collect();
        let known: Vec<f64> = heights.iter().flatten().copied().collect();
        if !known.is_empty() {
            let lo = known.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = known.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let z = match a.justification {
                Justification::Minimum => lo,
                Justification::Midline => (lo + hi) / 2.0,
                Justification::Maximum => hi,
            } + a.offset;
            // Cut by the underside, up to the justified height.
            let at = |h: Option<f64>| match (a.justification, h) {
                (Justification::Minimum, _) | (_, None) => z,
                (_, Some(h)) => z.min(h + a.offset),
            };
            tops = heights[..ring.len()].iter().map(|h| at(*h)).collect();
            hole_tops = holes
                .iter()
                .map(|h| {
                    h.iter()
                        .map(|p| at(underside(store, &a.target, *p)))
                        .collect()
                })
                .collect();
        }
    }
    if tops.iter().any(|t| *t <= z0 + 1e-6) {
        return Err(Error::new(
            "column.non_positive_height",
            "the column's top is at or below its base",
        ));
    }
    Ok(ColumnGeometry {
        ty,
        ring,
        holes,
        z0,
        tops,
        hole_tops,
    })
}

/// Every Reviera column that regenerates.
pub fn all(store: &Store) -> Vec<(ElementId, ColumnGeometry)> {
    store
        .iter()
        .filter(|e| column_recipe(e).is_some())
        .filter_map(|e| Some((e.id(), regenerate(store, e).ok()?)))
        .collect()
}

/// The project's default column type, created when first needed.
fn default_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn
        .store()
        .iter()
        .filter(|e| column_type(e).is_some_and(|t| !t.structural))
        .min_by_key(|e| e.name.as_deref() != Some(DEFAULT_TYPE_NAME))
    {
        return Ok(e.id());
    }
    txn.create(
        NewElement::new("IfcColumnType")
            .name(DEFAULT_TYPE_NAME)
            .param("@PredefinedType", "COLUMN")
            .param(
                RECIPE_KEY,
                serde_json::to_value(ColumnType::default()).expect("serialises"),
            ),
    )
}

/// The project's default structural column type, created when first needed: a 400 × 400 mm
/// concrete column.
fn default_structural_type(txn: &mut crate::txn::Transaction<'_>) -> Result<ElementId> {
    if let Some(e) = txn.store().iter().find(|e| {
        column_type(e).is_some_and(|t| t.structural)
            && e.name.as_deref() == Some(DEFAULT_STRUCTURAL_TYPE_NAME)
    }) {
        return Ok(e.id());
    }
    let concrete = crate::codes::settings(txn.store())
        .code
        .structural()
        .concrete;
    let t = ColumnType {
        structural: true,
        material: Some(concrete.into()),
        material_kind: Some(MaterialKind::Concrete),
        ..ColumnType::default()
    };
    txn.create(
        NewElement::new("IfcColumnType")
            .name(DEFAULT_STRUCTURAL_TYPE_NAME)
            .param("@PredefinedType", "COLUMN")
            .param(RECIPE_KEY, serde_json::to_value(t).expect("serialises")),
    )
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, r: &ColumnRecipe) -> Result<()> {
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
pub struct PlaceParams {
    /// A column type; omitted, the default (400 × 400 mm), made on first use.
    pub type_id: Option<ElementRef>,
    /// The column's centre in plan (metres).
    pub point: [f64; 2],
    pub base_level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: f64,
    /// A level, or a height above the base.
    pub top_constraint: TopConstraint,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: f64,
    /// Counter-clockwise; accepts units ("30°").
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

fn resolve_type(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such column type"))?;
    if column_type(store.get(id).expect("resolved")).is_none() {
        return Err(Error::new(
            "column.not_a_type",
            format!("element {id} is not a column type"),
        ));
    }
    Ok(id)
}

/// Places an architectural column (`column.place_architectural`).
pub(crate) fn place(txn: &mut crate::txn::Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    place_as(txn, p, false)
}

/// Places a structural column (`column.place`, STR-COL-001): load-bearing, of a structural
/// type (the default 400 × 400 mm concrete one, made on first use).
pub(crate) fn place_structural(
    txn: &mut crate::txn::Transaction<'_>,
    p: PlaceParams,
) -> Result<ElementId> {
    place_as(txn, p, true)
}

fn place_as(
    txn: &mut crate::txn::Transaction<'_>,
    p: PlaceParams,
    structural: bool,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let type_id = match (&p.type_id, structural) {
        (Some(r), _) => resolve_type(&store, r)?,
        (None, false) => default_type(txn)?,
        (None, true) => default_structural_type(txn)?,
    };
    if structural
        && !column_type(txn.store().get(type_id).expect("resolved")).is_some_and(|t| t.structural)
    {
        return Err(Error::new(
            "column.not_structural_type",
            format!("column type {type_id} is not a structural type"),
        ));
    }
    let store = txn.store().clone();
    let extent = Extent::from_args(
        &store,
        &p.base_level,
        p.base_offset,
        p.top_constraint,
        p.top_offset,
    )?;
    let recipe = ColumnRecipe {
        kind: COLUMN_RECIPE.into(),
        point: p.point,
        rotation: p.rotation,
        extent,
        attach_top: None,
    };
    let guid = store.get(type_id).expect("resolved").guid().to_string();
    let id = txn.create(
        NewElement::new("IfcColumn")
            .param("@PredefinedType", "COLUMN")
            .param("@type", guid)
            .param(
                "Pset_ColumnCommon.LoadBearing",
                json!({ "type": "IfcBoolean", "value": structural }),
            )
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
pub struct PlaceAtGridsParams {
    /// A structural column type; omitted, the default.
    pub type_id: Option<ElementRef>,
    /// Grid lines; a column goes at every point where two of them cross.
    pub grid_ids: Vec<ElementRef>,
    pub base_level: ElementRef,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: f64,
    pub top_constraint: TopConstraint,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: f64,
    /// Added to the grids' direction at each crossing; accepts units ("90°").
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

/// Where two straight segments cross (within their extents), if they do.
fn crossing(a: &crate::walls::Line, b: &crate::walls::Line) -> Option<[f64; 2]> {
    let (p, r) = (a.start, [a.end[0] - a.start[0], a.end[1] - a.start[1]]);
    let (q, t) = (b.start, [b.end[0] - b.start[0], b.end[1] - b.start[1]]);
    let den = r[0] * t[1] - r[1] * t[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let qp = [q[0] - p[0], q[1] - p[1]];
    let u = (qp[0] * t[1] - qp[1] * t[0]) / den;
    let v = (qp[0] * r[1] - qp[1] * r[0]) / den;
    let tol = 1e-9;
    ((-tol..=1.0 + tol).contains(&u) && (-tol..=1.0 + tol).contains(&v))
        .then(|| [p[0] + u * r[0], p[1] + u * r[1]])
}

/// The crossings of a model's grid lines (all of them, or `only` these), with the two lines'
/// names and the first line's direction.
pub fn grid_crossings(
    store: &Store,
    only: Option<&[ElementId]>,
) -> Vec<([f64; 2], String, String, f64)> {
    let lines: Vec<_> = crate::grids::list(store)
        .into_iter()
        .filter(|g| only.is_none_or(|o| o.contains(&g.id)))
        .collect();
    let mut out: Vec<([f64; 2], String, String, f64)> = Vec::new();
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            if let Some(p) = crossing(&lines[i].curve, &lines[j].curve) {
                let c = &lines[i].curve;
                let angle = (c.end[1] - c.start[1]).atan2(c.end[0] - c.start[0]);
                if !out
                    .iter()
                    .any(|(q, ..)| (q[0] - p[0]).hypot(q[1] - p[1]) < 1e-3)
                {
                    out.push((p, lines[i].name.clone(), lines[j].name.clone(), angle));
                }
            }
        }
    }
    out
}

/// Places a structural column at every crossing of the given grid lines
/// (`column.place_at_grids`), turned to the grids; one undo step.
pub(crate) fn place_at_grids(
    txn: &mut crate::txn::Transaction<'_>,
    p: PlaceAtGridsParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = p
        .grid_ids
        .iter()
        .map(|r| {
            store
                .resolve(r)
                .filter(|id| store.get(*id).is_some_and(crate::grids::is_grid))
                .ok_or_else(|| Error::new("grid.not_found", format!("no grid line {r:?}")))
        })
        .collect::<Result<_>>()?;
    let points = grid_crossings(&store, Some(&ids));
    if points.is_empty() {
        return Err(Error::new(
            "column.no_crossings",
            "the chosen grid lines do not cross",
        ));
    }
    let mut out = Vec::new();
    for (point, _, _, angle) in points {
        out.push(place_structural(
            txn,
            PlaceParams {
                type_id: p.type_id.clone(),
                point,
                base_level: p.base_level.clone(),
                base_offset: p.base_offset,
                top_constraint: p.top_constraint,
                top_offset: p.top_offset,
                rotation: angle + p.rotation,
            },
        )?);
    }
    Ok(out)
}

/// A column's location mark (STR-COL-001): the grid crossing nearest its centre, as its two
/// lines' names, lettered line first ("C-4"), with the offset from it when it is off the
/// crossing ("C-4 (+250, -100)", millimetres along x and y). `None` without crossings.
pub fn location_mark(store: &Store, point: [f64; 2]) -> Option<String> {
    let (p, a, b, _) = grid_crossings(store, None).into_iter().min_by(|x, y| {
        let d = |q: [f64; 2]| (q[0] - point[0]).hypot(q[1] - point[1]);
        d(x.0).total_cmp(&d(y.0))
    })?;
    let lettered = |n: &str| n.chars().next().is_some_and(char::is_alphabetic);
    let (first, second) = if lettered(&b) && !lettered(&a) {
        (b, a)
    } else {
        (a, b)
    };
    let (dx, dy) = (
        ((point[0] - p[0]) * 1000.0).round(),
        ((point[1] - p[1]) * 1000.0).round(),
    );
    Some(if dx == 0.0 && dy == 0.0 {
        format!("{first}-{second}")
    } else {
        format!("{first}-{second} ({dx:+}, {dy:+})")
    })
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// The columns to check; default every structural column.
    pub column_ids: Option<Vec<ElementRef>>,
}

/// A modelling-time advisory on a structural column (not a design check).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Advisory {
    pub column_id: ElementId,
    /// Its unbraced length (its height), metres.
    pub length: f64,
    /// Length over the least radius of gyration (effective length factor 1).
    pub slenderness: f64,
    /// The limit it exceeds, when it exceeds one.
    pub limit: Option<f64>,
    /// What the limit is and where it comes from.
    pub note: String,
}

/// Slenderness advisories for structural columns (`column.check`, STR-COL-001): each column's
/// height over its section's least radius of gyration, with K = 1, against public guidance for
/// the model's region: steel L/r preferably not over 200 (AISC 360, user note to E2); concrete
/// k·lu/r over 22 means slenderness may need to be considered (ACI 318, sway frames); under the
/// Eurocodes the limit depends on the axial load (EN 1992-1-1 5.8.3.1), so λ is reported only.
/// These are advisories for modelling, not design checks.
pub fn check(store: &Store, p: &CheckParams) -> Result<Vec<Advisory>> {
    let ids: Vec<ElementId> = match &p.column_ids {
        Some(refs) => resolve_columns(store, refs)?
            .into_iter()
            .map(|(id, _)| id)
            .collect(),
        None => all(store).into_iter().map(|(id, _)| id).collect(),
    };
    let region = crate::codes::settings(store).code.structural();
    let mut out = Vec::new();
    for id in ids {
        let g = regenerate(store, store.get(id).expect("listed"))?;
        if !g.ty.structural {
            continue;
        }
        let length = g.top() - g.z0;
        let r = g.ty.properties().r_min;
        if r <= 0.0 {
            continue;
        }
        let slenderness = length / r;
        let (limit, note) = match (g.ty.material_kind, region.region) {
            (Some(MaterialKind::Steel), _) => (
                Some(200.0),
                "Steel: L/r preferably not over 200 (AISC 360, user note to E2; guidance, not a \
                 requirement)",
            ),
            (
                Some(MaterialKind::Concrete | MaterialKind::PrecastConcrete),
                crate::codes::Region::UnitedStates | crate::codes::Region::Canada,
            ) => (
                Some(22.0),
                "Concrete: k·lu/r over 22 (k = 1) — slenderness effects may need to be considered \
                 (ACI 318, sway frames)",
            ),
            (Some(MaterialKind::Concrete | MaterialKind::PrecastConcrete), _) => (
                None,
                "Concrete: λ = l0/i; its limit depends on the axial load (EN 1992-1-1 5.8.3.1)",
            ),
            _ => (
                None,
                "No public slenderness guidance applied to this material",
            ),
        };
        out.push(Advisory {
            column_id: id,
            length,
            slenderness,
            limit: limit.filter(|l| slenderness > *l),
            note: note.into(),
        });
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkParams {
    pub column_ids: Vec<ElementRef>,
}

/// A column's location mark.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Mark {
    pub column_id: ElementId,
    pub mark: Option<String>,
}

/// The location marks of columns (`column.location_mark`).
pub fn location_marks(store: &Store, p: &MarkParams) -> Result<Vec<Mark>> {
    resolve_columns(store, &p.column_ids).map(|cols| {
        cols.into_iter()
            .map(|(id, r)| Mark {
                column_id: id,
                mark: location_mark(store, r.point),
            })
            .collect()
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub column_ids: Vec<ElementRef>,
    pub point: Option<[f64; 2]>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: Option<f64>,
    pub base_level: Option<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub base_offset: Option<f64>,
    pub top_constraint: Option<TopConstraint>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: Option<f64>,
}

fn resolve_columns(store: &Store, refs: &[ElementRef]) -> Result<Vec<(ElementId, ColumnRecipe)>> {
    refs.iter()
        .map(|r| {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such column"))?;
            let recipe = column_recipe(store.get(id).expect("resolved")).ok_or_else(|| {
                Error::new(
                    "column.not_reviera",
                    format!("element {id} is not a Reviera column"),
                )
            })?;
            Ok((id, recipe))
        })
        .collect()
}

/// Changes columns' place, rotation or extent (`column.set`); omitted values are kept.
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    for (id, mut r) in resolve_columns(&store, &p.column_ids)? {
        crate::modify::check_unpinned(&store, id)?;
        if let Some(pt) = p.point {
            r.point = pt;
        }
        if let Some(a) = p.rotation {
            r.rotation = a;
        }
        let base =
            match &p.base_level {
                Some(b) => b.clone(),
                None => ElementRef::Id(find(&store, &r.extent.base_level).ok_or_else(|| {
                    Error::new("level.missing", "the column's base level is gone")
                })?),
            };
        let top = match p.top_constraint {
            Some(t) => t,
            None => match r.extent.top_level().and_then(|g| find(&store, g)) {
                Some(l) => TopConstraint::Level(crate::walls::ElementRefValue::Id(l)),
                None => {
                    TopConstraint::Unconnected(r.extent.top["unconnected"].as_f64().unwrap_or(3.0))
                }
            },
        };
        r.extent = Extent::from_args(
            &store,
            &base,
            p.base_offset.unwrap_or(r.extent.base_offset),
            top,
            p.top_offset.unwrap_or(r.extent.top_offset),
        )?;
        write(txn, id, &r)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachParams {
    pub column_ids: Vec<ElementRef>,
    /// A Reviera roof, floor or ceiling above the columns.
    pub target_id: ElementRef,
    #[serde(default)]
    pub justification: Justification,
    /// Above (or, negative, below) the attachment.
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
}

/// Attaches columns' tops to a roof, floor or ceiling (`column.attach`).
pub(crate) fn attach(txn: &mut crate::txn::Transaction<'_>, p: AttachParams) -> Result<()> {
    let store = txn.store().clone();
    let target = store
        .resolve(&p.target_id)
        .ok_or_else(|| Error::new("element.not_found", "no such roof, floor or ceiling"))?;
    let t = store.get(target).expect("resolved");
    if crate::roofs::roof_recipe(t).is_none() && crate::floors::slab_recipe(t).is_none() {
        return Err(Error::new(
            "column.bad_target",
            format!("element {target} is not a Reviera roof, floor or ceiling"),
        ));
    }
    let guid = t.guid().to_string();
    for (id, mut r) in resolve_columns(&store, &p.column_ids)? {
        crate::modify::check_unpinned(&store, id)?;
        r.attach_top = Some(Attach {
            target: guid.clone(),
            justification: p.justification,
            offset: p.offset,
        });
        // It must cover the column.
        if underside(&store, &guid, r.point).is_none() {
            return Err(Error::new(
                "column.not_covered",
                format!("element {target} is not over column {id}"),
            ));
        }
        write(txn, id, &r)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DetachParams {
    pub column_ids: Vec<ElementRef>,
}

/// Detaches columns' tops (`column.detach`): they go back to their top constraint.
pub(crate) fn detach(txn: &mut crate::txn::Transaction<'_>, p: DetachParams) -> Result<()> {
    let store = txn.store().clone();
    for (id, mut r) in resolve_columns(&store, &p.column_ids)? {
        r.attach_top = None;
        write(txn, id, &r)?;
    }
    Ok(())
}

/// Moves, rotates or mirrors a column: its centre by `x`, its rotation by `turn` (or reflected
/// about a line at angle `mirror`), up by `dz`.
pub(crate) fn transform(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    x: &dyn Fn([f64; 2]) -> [f64; 2],
    turn: f64,
    mirror: Option<f64>,
    dz: f64,
) -> Result<()> {
    let store = txn.store().clone();
    let mut r = column_recipe(store.get(id).expect("exists")).expect("a column");
    r.point = x(r.point);
    r.rotation = match mirror {
        Some(axis) => 2.0 * axis - r.rotation,
        None => r.rotation + turn,
    };
    r.extent.base_offset += dz;
    if r.extent.top_level().is_some() {
        r.extent.top_offset += dz;
    }
    write(txn, id, &r)
}

/// Sets a column type's section value (`type.set_param`: `section`, `width`, `depth` or
/// `diameter`).
pub(crate) fn set_type_param(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    param: &str,
    value: &Value,
) -> Result<()> {
    let store = txn.store().clone();
    let mut t = section_type(store.get(id).expect("checked")).expect("a column or beam type");
    let length = |v: &Value| -> Result<f64> {
        let x = match v {
            Value::Number(n) => n.as_f64().unwrap_or(-1.0),
            Value::String(s) => {
                crate::units::parse(s, "length", &crate::units::settings_of(&store))?
            }
            _ => -1.0,
        };
        if x > 0.0 && x.is_finite() {
            Ok(x)
        } else {
            Err(Error::new(
                "type.invalid_value",
                "the size must be greater than 0",
            ))
        }
    };
    match param {
        "section" => {
            t.section = serde_json::from_value(value.clone()).map_err(|_| {
                Error::new(
                    "type.invalid_value",
                    "the section is `rectangular`, `round`, `i_shape`, `channel`, `angle`, `tee`, \
                     `rectangular_hollow` or `circular_hollow`",
                )
            })?;
        }
        "width" | "diameter" => t.width = length(value)?,
        "depth" => t.depth = length(value)?,
        "web_thickness" => t.web_thickness = length(value)?,
        "flange_thickness" => t.flange_thickness = length(value)?,
        "wall_thickness" => t.wall_thickness = length(value)?,
        "structural" => {
            t.structural = value
                .as_bool()
                .ok_or_else(|| Error::new("type.invalid_value", "`structural` is true or false"))?;
        }
        "material" => t.material = value.as_str().map(str::to_owned).filter(|s| !s.is_empty()),
        "material_kind" => {
            t.material_kind = if value.is_null() {
                None
            } else {
                Some(serde_json::from_value(value.clone()).map_err(|_| {
                    Error::new(
                        "type.invalid_value",
                        "the material kind is `concrete`, `steel`, `precast_concrete`, `wood` or \
                         `other`",
                    )
                })?)
            };
        }
        "designation" => {
            t.designation = value.as_str().map(str::to_owned).filter(|s| !s.is_empty());
        }
        _ => {
            let value = (!value.is_null()).then_some(value);
            return crate::parameters::set(txn, &[id], param, value).map(|_| ());
        }
    }
    t.check().map_err(|m| Error::new("type.invalid_value", m))?;
    let v = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_and_meshes() {
        let t = ColumnType::default();
        assert_eq!(t.outline().len(), 4);
        let round = ColumnType {
            section: Section::Round,
            width: 0.5,
            ..ColumnType::default()
        };
        assert_eq!(round.outline().len(), ROUND_SIDES);
        let g = ColumnGeometry {
            ty: t.clone(),
            ring: t.outline(),
            holes: Vec::new(),
            z0: 0.0,
            tops: vec![3.0; 4],
            hole_tops: Vec::new(),
        };
        let (_, idx) = g.mesh();
        assert_eq!(idx.len(), 36);
    }
}
