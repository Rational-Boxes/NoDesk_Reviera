// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sloped glazing (spec Ch 3 §3.5.8, ARC-CW-008): a footprint or extrusion roof whose type is a
//! *Sloped Glazing* type is built as a curtain surface on each of its planes instead of a slab.
//!
//! The roof type's recipe carries `glazing`, a curtain layout: its `v` rule lays out **Grid 1**,
//! lines parallel to each plane's eave measured up the slope from the eave, and its `u` rule
//! **Grid 2**, lines up the slope measured along the eave (centred). Each cell is clipped to the
//! plane into a panel; interior mullions run along the grid lines within the plane; border
//! mullions sit on the planes' edges, once on a hip, valley or ridge shared by two planes and
//! turned to the bisector of their normals. The roof keeps its sketch, slopes and joins.
//!
//! Not built yet: manual grid lines and panel swaps on glazing planes, Rafter Cut and Fascia
//! Depth, the warning when changing back to a basic type, an `IfcCurtainWall` per plane in IFC
//! (the roof's body is the glazing's B-rep).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::curtain::{CurtainType, Justification, Mullion, Panel, Rule, solve};
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

use super::{ROOF_TYPE_RECIPE, RoofGeometry};

/// A roof type's glazing layout, if it is a sloped glazing type.
pub fn of_type(t: &Element) -> Option<CurtainType> {
    let v = t.param(RECIPE_KEY)?;
    if v.get("kind")?.as_str()? != ROOF_TYPE_RECIPE {
        return None;
    }
    serde_json::from_value(v.get("glazing")?.clone()).ok()
}

/// A roof's glazing layout, if its type is sloped glazing.
pub fn of_roof(store: &Store, e: &Element) -> Option<CurtainType> {
    let g = GlobalId::parse(e.param("@type")?.as_str()?)?;
    of_type(store.get(store.find_by_guid(&g)?)?)
}

/// One plane's grid lines.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaneGrid {
    /// The footprint curve the plane rises from.
    pub key: Option<u32>,
    /// Grid 1, up the slope from the eave (metres along the slope).
    pub grid1: Vec<f64>,
    /// Grid 2, along the eave.
    pub grid2: Vec<f64>,
}

/// How far a vent pane is drawn open, about its upper edge.
pub const VENT_OPEN: f64 = 15.0 * std::f64::consts::PI / 180.0;

/// A pane of one plane instead of the type's: the plane by the footprint curve it rises from,
/// the cell by column (along the eave, from the plane's start) and row (up the slope, from the
/// eave).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaneOverride {
    pub plane: u32,
    pub cell: [usize; 2],
    pub panel: Panel,
}

/// A glazing part: a panel or a mullion.
#[derive(Debug, Clone, PartialEq)]
pub struct GlazingPart {
    pub mullion: bool,
    pub panel: Panel,
    pub mesh: Mesh,
    /// A mullion's axis (for checks), start and end.
    pub axis: Option<[[f64; 3]; 2]>,
    /// A panel's cell: along the eave and up the slope (a vent's hinge length and height).
    pub size: Option<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Glazing {
    pub planes: Vec<PlaneGrid>,
    pub parts: Vec<GlazingPart>,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A box along `a`→`b`, `width` across (square to `up`) and `depth` along `up`, centred.
pub(super) fn bar(a: [f64; 3], b: [f64; 3], up: [f64; 3], width: f64, depth: f64) -> Mesh {
    let d = unit(sub(b, a));
    let up = unit(sub(
        up,
        [d[0] * dot(up, d), d[1] * dot(up, d), d[2] * dot(up, d)],
    ));
    let side = cross(d, up);
    let (w, h) = (width / 2.0, depth / 2.0);
    let corner = |p: [f64; 3], s: f64, u: f64| {
        [
            p[0] + side[0] * s + up[0] * u,
            p[1] + side[1] * s + up[1] * u,
            p[2] + side[2] * s + up[2] * u,
        ]
    };
    let mut pts = Vec::with_capacity(8);
    for p in [a, b] {
        for (s, u) in [(-w, -h), (w, -h), (w, h), (-w, h)] {
            pts.push(corner(p, s, u));
        }
    }
    // Ends and the four sides, wound outward (side × up = −d at `a`).
    let quads: [[u32; 4]; 6] = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    let idx = quads
        .iter()
        .flat_map(|q| [q[0], q[1], q[2], q[0], q[2], q[3]])
        .collect();
    (pts, idx)
}

/// A plane edge and the normals of the planes it bounds.
type SharedEdge = ([f64; 3], [f64; 3], Vec<[f64; 3]>);

/// The glazing of a roof of a sloped glazing type, with its swapped panes.
pub fn regenerate(g: &RoofGeometry, ty: &CurtainType, panes: &[PaneOverride]) -> Glazing {
    let mut planes = Vec::new();
    let mut parts = Vec::new();
    let border = ty.border;
    let interior = ty.interior;
    let t = ty.panel_thickness.max(0.001);
    // Edges of all planes, to share border mullions: (a, b, normals).
    let mut edges: Vec<SharedEdge> = Vec::new();
    for f in &g.faces {
        if f.bottom.len() < 3 {
            continue;
        }
        let n = unit(f.normal);
        // Along the eave (horizontal in the plane), and up the slope.
        let h = if n[2].abs() > 1.0 - 1e-9 {
            [1.0, 0.0, 0.0]
        } else {
            unit(cross(n, [0.0, 0.0, 1.0]))
        };
        let mut e2 = cross(n, h);
        if e2[2] < 0.0 {
            e2 = [-e2[0], -e2[1], -e2[2]];
        }
        let h = cross(e2, n);
        let o = f.bottom[0];
        let local: Vec<[f64; 2]> = f
            .bottom
            .iter()
            .map(|p| [dot(sub(*p, o), h), dot(sub(*p, o), e2)])
            .collect();
        let (smin, smax) = local
            .iter()
            .fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p[0]), a.1.max(p[0])));
        let (tmin, tmax) = local
            .iter()
            .fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p[1]), a.1.max(p[1])));
        let grid1: Vec<f64> = solve(ty.v, tmax - tmin, Justification::Beginning, 0.0);
        let grid2: Vec<f64> = solve(ty.u, smax - smin, Justification::Center, 0.0);
        let world = |s: f64, tt: f64, z: f64| {
            [
                o[0] + h[0] * s + e2[0] * tt + n[0] * z,
                o[1] + h[1] * s + e2[1] * tt + n[1] * z,
                o[2] + h[2] * s + e2[2] * tt + n[2] * z,
            ]
        };
        // Panels: each cell clipped to the plane.
        if ty.panel != Panel::Empty {
            let mut ss = vec![smin];
            ss.extend(grid2.iter().map(|x| smin + x));
            ss.push(smax);
            let mut ts = vec![tmin];
            ts.extend(grid1.iter().map(|x| tmin + x));
            ts.push(tmax);
            let face = crate::polygons::union(std::slice::from_ref(&local));
            for i in 0..ss.len() - 1 {
                for j in 0..ts.len() - 1 {
                    let cell = crate::polygons::union(&[vec![
                        [ss[i], ts[j]],
                        [ss[i + 1], ts[j]],
                        [ss[i + 1], ts[j + 1]],
                        [ss[i], ts[j + 1]],
                    ]]);
                    let kind = f
                        .key
                        .and_then(|k| panes.iter().rev().find(|o| o.plane == k && o.cell == [i, j]))
                        .map_or(ty.panel, |o| o.panel);
                    if kind == Panel::Empty {
                        continue;
                    }
                    for piece in crate::polygons::intersection(&face, &cell) {
                        if piece.area() < 1e-6 {
                            continue;
                        }
                        let (pts, idx) = crate::bodies::extrude(&piece.outer, &piece.holes, 0.0, t);
                        // A vent hangs from its upper edge, opened out of the plane.
                        let hinge = ts[j + 1];
                        let (c, sn) = (VENT_OPEN.cos(), VENT_OPEN.sin());
                        let place = |p: &[f64; 3]| {
                            if kind != Panel::Vent {
                                return world(p[0], p[1], p[2]);
                            }
                            let (dt, dz) = (p[1] - hinge, p[2]);
                            world(p[0], hinge + dt * c - dz * sn, dt * -sn + dz * c)
                        };
                        parts.push(GlazingPart {
                            mullion: false,
                            panel: kind,
                            mesh: (pts.iter().map(place).collect(), idx),
                            axis: None,
                            size: Some([ss[i + 1] - ss[i], ts[j + 1] - ts[j]]),
                        });
                    }
                }
            }
        }
        // Interior mullions along the grid lines, within the plane.
        if let Some(m) = interior {
            let along = |s: Option<f64>, tt: Option<f64>| -> Option<([f64; 2], [f64; 2])> {
                // The chord of the plane's polygon on a line s = const or t = const.
                let k = local.len();
                let mut hits: Vec<f64> = Vec::new();
                for q in 0..k {
                    let (a, b) = (local[q], local[(q + 1) % k]);
                    let (va, vb, ua, ub) = match (s, tt) {
                        (Some(s), _) => (a[0] - s, b[0] - s, a[1], b[1]),
                        (_, Some(t)) => (a[1] - t, b[1] - t, a[0], b[0]),
                        _ => return None,
                    };
                    if (va > 0.0) != (vb > 0.0) {
                        hits.push(ua + (ub - ua) * va / (va - vb));
                    }
                }
                let (lo, hi) = hits
                    .iter()
                    .fold((f64::MAX, f64::MIN), |a, v| (a.0.min(*v), a.1.max(*v)));
                (hi - lo > 1e-6).then(|| match (s, tt) {
                    (Some(s), _) => ([s, lo], [s, hi]),
                    _ => ([lo, tt.unwrap_or(0.0)], [hi, tt.unwrap_or(0.0)]),
                })
            };
            let lines = grid1
                .iter()
                .map(|x| along(None, Some(tmin + x)))
                .chain(grid2.iter().map(|x| along(Some(smin + x), None)));
            for (a, b) in lines.flatten() {
                let (pa, pb) = (world(a[0], a[1], t / 2.0), world(b[0], b[1], t / 2.0));
                parts.push(GlazingPart {
                    mullion: true,
                    panel: ty.panel,
                    mesh: bar(pa, pb, n, m.width, m.depth),
                    axis: Some([pa, pb]),
                    size: None,
                });
            }
        }
        for q in 0..f.bottom.len() {
            let (a, b) = (f.bottom[q], f.bottom[(q + 1) % f.bottom.len()]);
            match edges.iter_mut().find(|e| {
                let near = |p: [f64; 3], r: [f64; 3]| dot(sub(p, r), sub(p, r)) < 1e-8;
                (near(e.0, a) && near(e.1, b)) || (near(e.0, b) && near(e.1, a))
            }) {
                Some(e) => e.2.push(n),
                None => edges.push((a, b, vec![n])),
            }
        }
        planes.push(PlaneGrid {
            key: f.key,
            grid1,
            grid2,
        });
    }
    // Border mullions: once per edge, turned to the bisector of the planes it bounds.
    if let Some(Mullion { width, depth }) = border {
        for (a, b, normals) in edges {
            if dot(sub(b, a), sub(b, a)) < 1e-8 {
                continue;
            }
            let up = unit(
                normals
                    .iter()
                    .fold([0.0; 3], |s, n| [s[0] + n[0], s[1] + n[1], s[2] + n[2]]),
            );
            let lift = |p: [f64; 3]| {
                [
                    p[0] + up[0] * t / 2.0,
                    p[1] + up[1] * t / 2.0,
                    p[2] + up[2] * t / 2.0,
                ]
            };
            let (pa, pb) = (lift(a), lift(b));
            parts.push(GlazingPart {
                mullion: true,
                panel: ty.panel,
                mesh: bar(pa, pb, up, width, depth),
                axis: Some([a, b]),
                size: None,
            });
        }
    }
    Glazing { planes, parts }
}

/// A roof's glazing from its geometry: a vault's or dome's on its curved surface (ARC-GLZ-006),
/// else plane by plane with its swapped panes.
pub fn of_geometry(g: &super::RoofGeometry, e: &Element, ty: &CurtainType) -> Glazing {
    let r = super::roof_recipe(e);
    if let Some(surface) = &g.surface {
        let grid = r.as_ref().and_then(|r| r.curved_glazing).unwrap_or_default();
        return super::curved::glazing(surface, &grid, ty);
    }
    regenerate(g, ty, &r.map(|r| r.panes).unwrap_or_default())
}

/// A roof's glazing, when its type is sloped glazing and it regenerates.
pub fn of(store: &Store, e: &Element) -> Option<Glazing> {
    let ty = of_roof(store, e)?;
    let g = super::regenerate(store, e).ok()?;
    Some(of_geometry(&g, e, &ty))
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTypeParams {
    pub name: String,
    /// Grid 1, parallel to the eaves (default Fixed Distance 1.5 m).
    pub grid1: Option<Rule>,
    /// Grid 2, up the slope (default Maximum Spacing 1.2 m).
    pub grid2: Option<Rule>,
    /// Mullions' width and depth (both interior and border); 0 for none.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub mullion_width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub mullion_depth: Option<f64>,
    pub panel: Option<Panel>,
}

fn layout(p: &CreateTypeParams, base: CurtainType) -> CurtainType {
    let mut ty = base;
    if let Some(r) = p.grid1 {
        ty.v = r;
    }
    if let Some(r) = p.grid2 {
        ty.u = r;
    }
    if p.mullion_width.is_some() || p.mullion_depth.is_some() {
        let old = ty.interior.or(ty.border).unwrap_or(Mullion {
            width: 0.06,
            depth: 0.12,
        });
        let m = Mullion {
            width: p.mullion_width.unwrap_or(old.width),
            depth: p.mullion_depth.unwrap_or(old.depth),
        };
        let some = (m.width > 0.0 && m.depth > 0.0).then_some(m);
        ty.interior = some;
        ty.border = some;
    }
    if let Some(panel) = p.panel {
        ty.panel = panel;
    }
    ty
}

/// `roof.create_glazing_type`: a Sloped Glazing roof type; give it to roofs with
/// `element.set_type`.
pub(crate) fn create_type(txn: &mut Transaction<'_>, p: CreateTypeParams) -> Result<ElementId> {
    let base = CurtainType {
        u: Rule {
            layout: crate::curtain::Layout::MaximumSpacing,
            spacing: 1.2,
            number: 0,
        },
        v: Rule {
            layout: crate::curtain::Layout::FixedDistance,
            spacing: 1.5,
            number: 0,
        },
        interior: Some(Mullion {
            width: 0.06,
            depth: 0.12,
        }),
        border: Some(Mullion {
            width: 0.06,
            depth: 0.12,
        }),
        ..CurtainType::default()
    };
    let ty = layout(&p, base);
    txn.create(
        NewElement::new(super::TYPE_CLASS)
            .name(p.name.trim())
            .param("@PredefinedType", "NOTDEFINED")
            .param(
                RECIPE_KEY,
                serde_json::json!({
                    "kind": ROOF_TYPE_RECIPE,
                    "thickness": ty.panel_thickness,
                    "glazing": serde_json::to_value(&ty).expect("serialises"),
                }),
            ),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTypeParams {
    pub type_id: ElementRef,
    pub grid1: Option<Rule>,
    pub grid2: Option<Rule>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub mullion_width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub mullion_depth: Option<f64>,
    pub panel: Option<Panel>,
}

/// `roof.set_glazing_type`: changes a Sloped Glazing type's grids, mullions or panel.
pub(crate) fn set_type(txn: &mut Transaction<'_>, p: SetTypeParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.type_id)
        .ok_or_else(|| Error::new("element.not_found", "no such roof type"))?;
    let t = store.get(id).expect("resolved");
    let base = of_type(t).ok_or_else(|| {
        Error::new(
            "roof.not_glazing_type",
            "the type is not a sloped glazing type",
        )
    })?;
    let ty = layout(
        &CreateTypeParams {
            name: String::new(),
            grid1: p.grid1,
            grid2: p.grid2,
            mullion_width: p.mullion_width,
            mullion_depth: p.mullion_depth,
            panel: p.panel,
        },
        base,
    );
    let mut recipe = t.param(RECIPE_KEY).cloned().expect("a roof type");
    recipe["glazing"] = serde_json::to_value(&ty).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), recipe);
    })
}
