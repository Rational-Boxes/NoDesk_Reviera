// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Path shape distribution and morphed distribution (STR-RBR-014).
//!
//! - **Path:** a main set's first bar, drawn in a plane square to the path where it starts, is
//!   carried along a 3D path (points, or a host's named path) at the layout's spacing. A frame is
//!   carried with it (each segment's frame the previous one turned onto it, so it does not twist
//!   more than the path does), and every bar's plane is square to the path's tangent where it
//!   lies. The bars follow the main set and the path, and go with the main set.
//! - **Morphed:** longitudinal bars through a driver set of closed stirrups: points spread round
//!   each stirrup (by count or spacing along it), each moved in to touch it (half the stirrup's
//!   bar and half its own), joined from the first stirrup to the last. They follow the stirrups'
//!   shape, size and bar, and go with them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::rebar::{self, Bar, BarType, RebarRecipe, SetGeometry, V3, add, dot, len, mul, sub, unit};
use crate::store::Store;
use crate::NewElement;

/// A path: its points, or a host's named path (`axis`, `along`, `up`; STR-RBR-013).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PathSpec {
    Points(Vec<V3>),
    Host { host: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "distribution", rename_all = "snake_case")]
pub enum DistributedPlacement {
    /// The main set's bar along a path, from `range.0` to `range.1` metres along it.
    Path {
        main: String,
        path: PathSpec,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        range: Option<[f64; 2]>,
    },
    /// Longitudinal bars through a driver set's stirrups.
    Morphed {
        driver: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing: Option<f64>,
    },
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i))
}

/// The set a distribution depends on (GlobalId).
pub fn source(r: &RebarRecipe) -> Option<&str> {
    match &r.placement {
        rebar::Placement::Distributed(DistributedPlacement::Path { main, .. }) => Some(main),
        rebar::Placement::Distributed(DistributedPlacement::Morphed { driver, .. }) => Some(driver),
        _ => None,
    }
}

fn source_geometry(store: &Store, guid: &str, what: &str) -> Result<(SetGeometry, BarType)> {
    let r = by_guid(store, guid).and_then(rebar::rebar_recipe).ok_or_else(|| Error::new("rebar.source", format!("the {what} set is gone")))?;
    let g = rebar::geometry(store, &r)?;
    let bt = rebar::bar_type(store, &r.bar_type)?;
    Ok((g, bt))
}

fn resolve(store: &Store, p: &PathSpec) -> Result<Vec<V3>> {
    match p {
        PathSpec::Points(pts) => Ok(pts.clone()),
        PathSpec::Host { host, name } => {
            let h = by_guid(store, host).ok_or_else(|| Error::new("rebar.path", "the path's host is gone"))?;
            crate::rebar_free::named_path(store, h, name)
        }
    }
}

/// The path's segments with a frame carried along it: start, direction, length, and the
/// frame's u (v is direction × u).
fn frames(path: &[V3]) -> Result<Vec<(V3, V3, f64, V3)>> {
    let segs: Vec<(V3, V3, f64)> = path.windows(2).filter(|w| len(sub(w[1], w[0])) > 1e-9).map(|w| (w[0], unit(sub(w[1], w[0])), len(sub(w[1], w[0])))).collect();
    let Some(&(_, t0, _)) = segs.first() else {
        return Err(Error::new("rebar.path", "the path needs at least two distinct points"));
    };
    let helper = if t0[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let mut u = unit(cross(helper, t0));
    let mut out = Vec::new();
    let mut prev = t0;
    for (a, t, l) in segs {
        // Turn the frame by the rotation taking the last direction to this one.
        let axis = cross(prev, t);
        let s = len(axis);
        let c = dot(prev, t);
        if s > 1e-12 {
            let k = mul(axis, 1.0 / s);
            u = add(add(mul(u, c), mul(cross(k, u), s)), mul(k, dot(k, u) * (1.0 - c)));
        }
        u = unit(sub(u, mul(t, dot(u, t))));
        out.push((a, t, l, u));
        prev = t;
    }
    Ok(out)
}

/// The point, direction and frame at `s` along the path.
fn at(frames: &[(V3, V3, f64, V3)], s: f64) -> (V3, V3, V3) {
    let mut run = 0.0;
    for (i, (a, t, l, u)) in frames.iter().enumerate() {
        if s <= run + l || i + 1 == frames.len() {
            return (add(*a, mul(*t, s - run)), *t, *u);
        }
        run += l;
    }
    unreachable!("frames are not empty")
}

/// A closed stirrup's loop without its hooks: the bar less its first and last segments (and
/// any near-zero ones), in order round it.
fn stirrup_loop(b: &Bar) -> Vec<V3> {
    let n = b.points.len();
    if n < 5 {
        return b.points.clone();
    }
    let mut pts: Vec<V3> = b.points[1..n - 1].to_vec();
    pts.dedup_by(|a, b| len(sub(*a, *b)) < 1e-9);
    pts
}

pub(crate) fn geometry(store: &Store, r: &RebarRecipe, bt: &BarType, p: &DistributedPlacement) -> Result<SetGeometry> {
    match p {
        DistributedPlacement::Path { main, path, range } => {
            let (mg, _) = source_geometry(store, main, "main")?;
            let bar = mg.bars.first().ok_or_else(|| Error::new("rebar.source", "the main set has no bars"))?;
            let pts = resolve(store, path)?;
            let fr = frames(&pts)?;
            let total: f64 = fr.iter().map(|f| f.2).sum();
            let (p0, t0, u0) = at(&fr, 0.0);
            let v0 = cross(t0, u0);
            // The main bar in the start's frame; it must lie square to the path there.
            let local: Vec<[f64; 3]> = bar.points.iter().map(|q| { let w = sub(*q, p0); [dot(w, u0), dot(w, v0), dot(w, t0)] }).collect();
            let off = local.iter().map(|l| l[2].abs()).fold(0.0, f64::max);
            if off > 0.01 {
                return Err(Error::new(
                    "rebar.path",
                    format!("the main bar is not square to the path where it starts: it reaches {:.0} mm along it", off * 1000.0),
                ));
            }
            let (from, to) = range.map_or((0.0, total), |[a, b]| (a.clamp(0.0, total), b.clamp(0.0, total)));
            if to <= from {
                return Err(Error::new("rebar.path", "the range is empty"));
            }
            let pos = rebar::positions(&r.layout, to - from, bt.diameter)?;
            let bars = pos
                .iter()
                .map(|t| {
                    let (q, tan, u) = at(&fr, from + t);
                    let v = cross(tan, u);
                    Bar { points: local.iter().map(|l| add(q, add(mul(u, l[0]), mul(v, l[1])))).collect() }
                })
                .collect();
            Ok(SetGeometry { bars, dimensions: mg.dimensions.clone(), role: mg.role.clone(), array_length: to - from, indices: Vec::new() })
        }
        DistributedPlacement::Morphed { driver, count, spacing } => {
            let (dg, dbt) = source_geometry(store, driver, "driver")?;
            if dg.role != "ligature" || dg.bars.len() < 2 {
                return Err(Error::new("rebar.driver", "the driver must be a set of at least two stirrups or ties"));
            }
            let loops: Vec<Vec<V3>> = dg.bars.iter().map(stirrup_loop).collect();
            let perimeter = |l: &[V3]| -> f64 { l.windows(2).map(|w| len(sub(w[1], w[0]))).sum::<f64>() + len(sub(l[0], l[l.len() - 1])) };
            let first = perimeter(&loops[0]);
            let n = match (count, spacing) {
                (Some(c), _) if *c > 0 => *c,
                (_, Some(s)) if *s > 0.0 => ((first / s).floor() as usize).max(1),
                _ => return Err(Error::new("rebar.layout", "give a count or a spacing round the stirrups")),
            };
            let inset = dbt.diameter / 2.0 + bt.diameter / 2.0;
            // Each stirrup's point at a share of the way round, moved in to touch it.
            let point = |l: &[V3], f: f64| -> V3 {
                let closed: Vec<V3> = l.iter().copied().chain(std::iter::once(l[0])).collect();
                let target = f * perimeter(l);
                let mut run = 0.0;
                let (mut q, mut dir) = (l[0], unit(sub(closed[1], closed[0])));
                for w in closed.windows(2) {
                    let s = len(sub(w[1], w[0]));
                    if s > 1e-12 {
                        dir = unit(sub(w[1], w[0]));
                    }
                    if run + s >= target {
                        q = add(w[0], mul(sub(w[1], w[0]), if s > 0.0 { (target - run) / s } else { 0.0 }));
                        break;
                    }
                    run += s;
                }
                // In, square to the stirrup in its plane (Newell's normal), toward its middle.
                let mut nrm = [0.0; 3];
                for w in closed.windows(2) {
                    nrm = add(nrm, cross(w[0], w[1]));
                }
                let mid = mul(l.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / l.len() as f64);
                let mut inward = unit(cross(unit(nrm), dir));
                if dot(inward, sub(mid, q)) < 0.0 {
                    inward = mul(inward, -1.0);
                }
                add(q, mul(inward, inset))
            };
            let bars: Vec<Bar> = (0..n)
                .map(|j| {
                    let f = j as f64 / n as f64;
                    Bar { points: loops.iter().map(|l| point(l, f)).collect() }
                })
                .collect();
            let longest = bars.iter().map(Bar::length).fold(0.0, f64::max);
            Ok(SetGeometry { bars, dimensions: vec![("A".into(), longest)], role: "main".into(), array_length: first, indices: Vec::new() })
        }
    }
}

// ---- Commands ----

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PathParams {
    pub main_rebar_id: ElementRef,
    /// The path's points; or `path_ref` a named path of the main set's host (`axis` for beams and
    /// columns, `along` or `up` for walls).
    #[serde(default)]
    pub path: Option<Vec<V3>>,
    #[serde(default)]
    pub path_ref: Option<String>,
    /// From and to, metres along the path.
    #[serde(default)]
    pub range: Option<[f64; 2]>,
    #[serde(default)]
    pub layout: rebar::Layout,
}

fn create(txn: &mut crate::txn::Transaction<'_>, r: RebarRecipe) -> Result<ElementId> {
    let g = rebar::geometry(txn.store(), &r)?;
    let predefined = if g.role == "ligature" { "LIGATURE" } else { "MAIN" };
    let id = txn.create(
        NewElement::new("IfcReinforcingBar").param("@PredefinedType", predefined).param(crate::walls::RECIPE_KEY, serde_json::to_value(&r).expect("serialises")),
    )?;
    crate::rebar_numbers::persist(txn)?;
    Ok(id)
}

fn set_of(store: &Store, r: &ElementRef) -> Result<(ElementId, RebarRecipe)> {
    let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
    let rec = store.get(id).and_then(rebar::rebar_recipe).ok_or_else(|| Error::new("rebar.not_rebar", format!("element {id} is not a rebar set")))?;
    Ok((id, rec))
}

/// Distributes a main set's bar along a path (`rebar.distribute_along_path`).
pub(crate) fn along_path(txn: &mut crate::txn::Transaction<'_>, p: PathParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (mid, main) = set_of(&store, &p.main_rebar_id)?;
    let path = match (&p.path, &p.path_ref) {
        (Some(pts), _) => PathSpec::Points(pts.clone()),
        (None, Some(n)) => PathSpec::Host { host: main.host.clone(), name: n.clone() },
        (None, None) => return Err(Error::new("rebar.path", "give the path's points or a named path")),
    };
    let r = RebarRecipe {
        kind: rebar::REBAR_RECIPE.into(),
        host: main.host.clone(),
        shape: main.shape.clone(),
        bar_type: main.bar_type.clone(),
        placement: rebar::Placement::Distributed(DistributedPlacement::Path { main: store.get(mid).expect("resolved").guid().to_string(), path, range: p.range }),
        layout: p.layout.clone(),
        edits: rebar::BarEdits::default(),
        splice: None,
        varying: false,
    };
    create(txn, r)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MorphedParams {
    pub driver_stirrup_set_id: ElementRef,
    pub bar_type: String,
    /// How many bars round the stirrups, or their spacing along the first stirrup (metres).
    #[serde(default)]
    pub count: Option<usize>,
    #[serde(default)]
    pub spacing: Option<f64>,
}

/// Longitudinal bars through a stirrup set (`rebar.distribute_morphed`).
pub(crate) fn morphed(txn: &mut crate::txn::Transaction<'_>, p: MorphedParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (did, driver) = set_of(&store, &p.driver_stirrup_set_id)?;
    let bt = rebar::bar_type(&store, &p.bar_type)?;
    let r = RebarRecipe {
        kind: rebar::REBAR_RECIPE.into(),
        host: driver.host.clone(),
        shape: "99".into(),
        bar_type: bt.id.clone(),
        placement: rebar::Placement::Distributed(DistributedPlacement::Morphed { driver: store.get(did).expect("resolved").guid().to_string(), count: p.count, spacing: p.spacing }),
        layout: rebar::Layout { rule: rebar::Rule::Single, ..Default::default() },
        edits: rebar::BarEdits::default(),
        splice: None,
        varying: true,
    };
    let g = rebar::geometry(&store, &r)?;
    let straight = g.bars.iter().all(|b| {
        let (a, e) = (b.points[0], b.points[b.points.len() - 1]);
        let d = unit(sub(e, a));
        b.points.iter().all(|q| len(cross(sub(*q, a), d)) < 1e-6)
    });
    let r = RebarRecipe { shape: if straight { "00".into() } else { "99".into() }, ..r };
    create(txn, r)
}
