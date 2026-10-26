// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The structural model distillation (ANA-FEA-001, DM-RPC-003): the analytical model, its
//! supports, load cases, combinations and loads as the neutral structural model
//! (`reviera.structural.v1.Model`, [`crate::frame::Model`]) that analysis providers read. Members
//! take their section properties from their section type and their moduli from the materials
//! library; a self-weight case adds each member's weight as a line load. Elements are keyed by
//! opaque keys; the map back to GlobalIds stays in Reviera.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::frame::{Case, Combination, Load, Member, Model, Node};
use crate::id::GlobalId;
use crate::store::Store;

/// Gravity (m/s²).
const G: f64 = 9.80665;

/// The model and the map from its keys to GlobalIds.
#[derive(Debug, Clone)]
pub struct Distilled {
    pub model: Model,
    pub key_map: BTreeMap<String, String>,
}

fn guid_of(store: &Store, id: crate::id::ElementId) -> String {
    store
        .get(id)
        .map(|e| e.guid().to_string())
        .unwrap_or_default()
}

/// Distils the model's analytical model, supports, cases, combinations and loads.
pub fn distill(store: &Store) -> Result<Distilled> {
    let mut key_map = BTreeMap::new();
    let mut node_key: BTreeMap<String, String> = BTreeMap::new();
    let mut nodes = Vec::new();
    for (i, (id, p)) in crate::analytical::nodes(store).into_iter().enumerate() {
        let key = format!("n{}", i + 1);
        let guid = guid_of(store, id);
        let support = store
            .get(id)
            .and_then(crate::analytical::node_recipe)
            .and_then(|r| r.support)
            .map(|s| s.dof);
        nodes.push(Node {
            key: key.clone(),
            xyz: p,
            support,
        });
        node_key.insert(guid.clone(), key.clone());
        key_map.insert(key, guid);
    }
    let mut member_key: BTreeMap<String, String> = BTreeMap::new();
    let mut members = Vec::new();
    let mut weight: Vec<(String, f64)> = Vec::new();
    for (i, (id, m)) in crate::analytical::members(store).into_iter().enumerate() {
        let key = format!("m{}", i + 1);
        let guid = guid_of(store, id);
        let ty = m
            .recipe
            .section_type
            .as_deref()
            .and_then(GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|t| store.get(t))
            .and_then(crate::columns::section_type)
            .ok_or_else(|| {
                Error::new(
                    "structural.no_section",
                    format!("analytical member {id} has no section type"),
                )
            })?;
        let material = m
            .recipe
            .material
            .clone()
            .or(ty.material.clone())
            .unwrap_or_default();
        let lib = crate::materials_library::find(&material).ok_or_else(|| {
            Error::new(
                "structural.no_material",
                format!(
                    "analytical member {id}'s material ({material}) is not in the materials library, so its stiffness is unknown"
                ),
            )
        })?;
        let e = lib.properties.get("E_MPa").copied().ok_or_else(|| {
            Error::new(
                "structural.no_modulus",
                format!("{material} has no modulus of elasticity in the library"),
            )
        })? * 1e6;
        let g = lib.properties.get("G_MPa").map_or(e / 2.6, |g| g * 1e6);
        let props = ty.properties();
        let n = |g: &str| node_key.get(g).cloned().unwrap_or_default();
        let a = guid_of(store, m.nodes[0]);
        let b = guid_of(store, m.nodes[1]);
        let d = [m.end[0] - m.start[0], m.end[1] - m.start[1]];
        let up = if d[0].hypot(d[1]) < 1e-6 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        members.push(Member {
            key: key.clone(),
            nodes: [n(&a), n(&b)],
            e,
            g,
            a: props.area,
            // The section's strong axis (its x, across its width) is the member's local y.
            iy: props.ix,
            iz: props.iy,
            j: ty.torsion_constant(),
            releases: m.recipe.releases,
            up,
        });
        if let Some(rho) = lib.properties.get("density_kg_m3") {
            weight.push((key.clone(), rho * G * props.area));
        }
        member_key.insert(guid.clone(), key.clone());
        key_map.insert(key, guid);
    }
    let mut case_key: BTreeMap<String, String> = BTreeMap::new();
    let mut cases = Vec::new();
    let mut loads = Vec::new();
    for (i, e) in store
        .iter()
        .filter(|e| crate::loads::case_recipe(e).is_some())
        .enumerate()
    {
        let key = format!("c{}", i + 1);
        let r = crate::loads::case_recipe(e).expect("filtered");
        if r.self_weight {
            for (m, w) in &weight {
                loads.push(Load::Line {
                    case: key.clone(),
                    member: m.clone(),
                    start: [0.0, 0.0, -w],
                    end: [0.0, 0.0, -w],
                    projected: false,
                    from: 0.0,
                    to: 1.0,
                    from_area: None,
                });
            }
        }
        cases.push(Case {
            key: key.clone(),
            name: e.name.clone().unwrap_or_default(),
        });
        case_key.insert(e.guid().to_string(), key.clone());
        key_map.insert(key, e.guid().to_string());
    }
    let mut combinations = Vec::new();
    for (i, e) in store
        .iter()
        .filter(|e| crate::loads::combination_recipe(e).is_some())
        .enumerate()
    {
        let key = format!("k{}", i + 1);
        let r = crate::loads::combination_recipe(e).expect("filtered");
        let name = e.name.clone().unwrap_or_default();
        let factors = crate::loads::factors(store, &r.formula, Some(&name))?
            .into_iter()
            .filter_map(|(case, f)| Some((case_key.get(&guid_of(store, case))?.clone(), f)))
            .collect();
        combinations.push(Combination {
            key: key.clone(),
            name,
            factors,
        });
        key_map.insert(key, e.guid().to_string());
    }
    // Members' axes by GlobalId, for loads in a host's local axes and for area loads.
    let geometry: BTreeMap<String, ([f64; 3], [f64; 3])> = crate::analytical::members(store)
        .into_iter()
        .map(|(id, m)| (guid_of(store, id), (m.start, m.end)))
        .collect();
    let to_global = |host: &str, v: [f64; 3]| -> [f64; 3] {
        let Some((a, b)) = geometry.get(host) else {
            return v;
        };
        let (_, ax) = crate::results::axes(*a, *b);
        [0, 1, 2].map(|k| ax[0][k] * v[0] + ax[1][k] * v[1] + ax[2][k] * v[2])
    };
    let mut notes = Vec::new();
    for e in store.iter() {
        if let Some(l) = crate::loads::point_load(e) {
            let Some(case) = case_key.get(&l.case).cloned() else {
                continue;
            };
            let mut forces = l.forces;
            if l.orientation == crate::loads::Orientation::HostLocal {
                let f = to_global(&l.host, [forces[0], forces[1], forces[2]]);
                let m = to_global(&l.host, [forces[3], forces[4], forces[5]]);
                forces = [f[0], f[1], f[2], m[0], m[1], m[2]];
            }
            loads.push(Load::Point {
                case,
                node: node_key.get(&l.host).cloned(),
                member: member_key.get(&l.host).cloned(),
                at: l.at,
                forces,
            });
        } else if let Some(l) = crate::loads::line_load(e) {
            let (Some(case), Some(member)) = (
                case_key.get(&l.case).cloned(),
                member_key.get(&l.host).cloned(),
            ) else {
                continue;
            };
            let (start, end) = if l.orientation == crate::loads::Orientation::HostLocal {
                (to_global(&l.host, l.start), to_global(&l.host, l.end))
            } else {
                (l.start, l.end)
            };
            loads.push(Load::Line {
                case,
                member,
                start,
                end,
                projected: l.projected,
                from: 0.0,
                to: 1.0,
                from_area: None,
            });
        } else if let Some(l) = crate::loads::area_load(e) {
            let Some(case) = case_key.get(&l.case).cloned() else {
                continue;
            };
            let Some(pg) = crate::loads::find_panel(store, &l.host) else {
                continue;
            };
            let key = format!("a{}", loads.len() + 1);
            key_map.insert(key.clone(), e.guid().to_string());
            let value = area_value(&pg, &l);
            loads.push(Load::Area {
                key: key.clone(),
                case: case.clone(),
                panel: l.host.clone(),
                value,
                projected: false,
            });
            let (carried, note) = carry_area(store, &pg, value, &geometry);
            if let Some(n) = note {
                notes.push(n);
            }
            for c in carried {
                if let Some(member) = member_key.get(&c.member).cloned() {
                    loads.push(Load::Line {
                        case: case.clone(),
                        member,
                        start: c.start,
                        end: c.end,
                        projected: false,
                        from: c.from,
                        to: c.to,
                        from_area: Some(key.clone()),
                    });
                }
            }
        }
    }
    // Panels go to providers that analyse surfaces.
    let mut panels = Vec::new();
    for (i, (id, pg)) in crate::analytical_panels::panels(store)
        .into_iter()
        .enumerate()
    {
        let key = format!("p{}", i + 1);
        key_map.insert(key.clone(), guid_of(store, id));
        let lib = pg
            .recipe
            .material
            .as_deref()
            .and_then(crate::materials_library::find);
        let e = lib
            .as_ref()
            .and_then(|l| l.properties.get("E_MPa").copied())
            .map(|v| v * 1e6);
        let nu = lib
            .as_ref()
            .and_then(|l| l.properties.get("nu").copied())
            .or_else(|| {
                let g = lib.as_ref()?.properties.get("G_MPa").copied()?;
                Some(e? / (2.0 * g * 1e6) - 1.0)
            });
        panels.push(crate::frame::Panel {
            key,
            nodes: pg
                .nodes
                .iter()
                .map(|n| {
                    node_key
                        .get(&guid_of(store, *n))
                        .cloned()
                        .unwrap_or_default()
                })
                .collect(),
            outline: pg.outline.clone(),
            openings: pg.recipe.openings.clone(),
            thickness: pg.recipe.thickness,
            e,
            nu,
            analyze_as: serde_json::to_value(pg.recipe.analyze_as)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            curved: pg.circle.is_some(),
            cladding: pg.recipe.cladding,
        });
    }
    if members.is_empty() {
        return Err(Error::new(
            "structural.empty",
            "the model has no analytical members (Physical to Analytical makes them)",
        ));
    }
    Ok(Distilled {
        model: Model {
            units: "SI".into(),
            nodes,
            members,
            cases,
            combinations,
            loads,
            panels,
            notes,
        },
        key_map,
    })
}

// ---- Results (ANA-FEA-001, STR-RES-001) ----

/// The project parameter holding the last analysis's results.
pub const RESULTS_KEY: &str = "rxd:analysis_results";

/// A finished analysis kept with the model: its job, when, the model it ran on (a hash of the
/// distilled model, to tell when the results are stale), warnings, and the results with keys
/// mapped back to GlobalIds.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Stored {
    pub job_id: String,
    pub at: String,
    pub snapshot: String,
    pub provider: String,
    pub results: crate::frame::Results,
    /// Pre-analysis check errors the run went ahead despite (issue keys, STR-AM-007).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignored_check_errors: Vec<String>,
}

/// A hash of the model's current distillation (empty when it has none).
pub fn snapshot(store: &Store) -> String {
    distill(store)
        .ok()
        .and_then(|d| serde_json::to_vec(&d.model).ok())
        .map(|raw| {
            // FNV-1a, 64-bit: stable across platforms and versions.
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in raw {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            format!("{h:016x}")
        })
        .unwrap_or_default()
}

fn project(store: &Store) -> Option<&crate::element::Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// The stored results, if any.
pub fn stored(store: &Store) -> Option<Stored> {
    serde_json::from_value(project(store)?.param(RESULTS_KEY)?.clone()).ok()
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordParams {
    pub job_id: String,
    pub provider: String,
    /// The provider's results, keys still opaque.
    pub results: crate::frame::Results,
    /// Opaque key → GlobalId.
    pub key_map: BTreeMap<String, String>,
    /// Pre-analysis check errors the run went ahead despite.
    #[serde(default)]
    pub ignored_check_errors: Vec<String>,
    /// The model hash the results were computed for (an imported package keeps its own);
    /// default the model as it is now.
    pub snapshot: Option<String>,
    /// When they were computed; default now.
    pub at: Option<String>,
}

/// Keeps a finished analysis's results with the model (`structural_analysis.record`).
pub(crate) fn record(txn: &mut crate::txn::Transaction<'_>, p: RecordParams) -> Result<()> {
    let store = txn.store().clone();
    let id = project(&store)
        .map(crate::element::Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let map = |k: &str| p.key_map.get(k).cloned().unwrap_or_else(|| k.to_owned());
    let mut results = p.results;
    for r in &mut results.results {
        r.key = map(&r.key);
        for d in &mut r.displacements {
            d.0 = map(&d.0);
        }
        for d in &mut r.reactions {
            d.0 = map(&d.0);
        }
        for m in &mut r.members {
            m.key = map(&m.key);
        }
    }
    let s = Stored {
        job_id: p.job_id,
        at: p.at.unwrap_or_else(crate::overlap_accept::now_rfc3339),
        snapshot: p.snapshot.unwrap_or_else(|| snapshot(&store)),
        provider: p.provider,
        results,
        ignored_check_errors: p.ignored_check_errors,
    };
    let v = serde_json::to_value(&s).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RESULTS_KEY.into(), v);
    })
}

/// A summary of one case's or combination's results.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct CaseSummary {
    pub name: String,
    pub combination: bool,
    /// The largest displacement (m) and its node.
    pub max_displacement: f64,
    pub max_displacement_node: Option<crate::id::ElementId>,
    /// The largest bending moment (N·m) and its member.
    pub max_moment: f64,
    pub max_moment_member: Option<crate::id::ElementId>,
    /// The sum of the vertical reactions (N).
    pub total_vertical_reaction: f64,
}

/// A member's results in one case.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct MemberSummary {
    pub member: crate::id::ElementId,
    /// The analytical member's physical elements.
    pub physical: Vec<crate::id::ElementId>,
    pub max_axial: f64,
    pub max_shear: f64,
    pub max_moment: f64,
    pub stations: Vec<crate::frame::Station>,
}

/// The results report (`structural_analysis.results`).
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct Report {
    pub job_id: String,
    pub at: String,
    pub provider: String,
    /// The model changed since the analysis ran.
    pub stale: bool,
    pub warnings: Vec<String>,
    /// Pre-analysis check errors the run went ahead despite (issue keys).
    pub ignored_check_errors: Vec<String>,
    pub cases: Vec<CaseSummary>,
    /// The members' results in `case` (the first case when not given).
    pub members: Vec<MemberSummary>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    /// A case or combination name for the members' results.
    pub case: Option<String>,
}

/// Reports the stored results (`structural_analysis.results`).
pub fn report(store: &Store, p: &ReportParams) -> Result<Report> {
    let s = stored(store).ok_or_else(|| {
        Error::new(
            "structural.no_results",
            "no analysis has been run and collected yet",
        )
    })?;
    let id = |g: &str| GlobalId::parse(g).and_then(|g| store.find_by_guid(&g));
    let moment = |f: &[f64; 6]| f[4].abs().max(f[5].abs());
    let cases = s
        .results
        .results
        .iter()
        .map(|r| {
            let (dn, dmax) = r
                .displacements
                .iter()
                .map(|(k, d)| (k, (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map_or((None, 0.0), |(k, v)| (id(k), v));
            let (mm, mmax) = r
                .members
                .iter()
                .map(|m| {
                    (
                        &m.key,
                        m.stations
                            .iter()
                            .map(|s| moment(&s.forces))
                            .fold(0.0, f64::max),
                    )
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map_or((None, 0.0), |(k, v)| (id(k), v));
            CaseSummary {
                name: r.name.clone(),
                combination: r.combination,
                max_displacement: dmax,
                max_displacement_node: dn,
                max_moment: mmax,
                max_moment_member: mm,
                total_vertical_reaction: r.reactions.iter().map(|(_, f)| f[2]).sum(),
            }
        })
        .collect();
    let chosen = match &p.case {
        Some(name) => s
            .results
            .results
            .iter()
            .find(|r| &r.name == name)
            .ok_or_else(|| Error::new("structural.no_case", format!("no results for {name}")))?,
        None => s
            .results
            .results
            .first()
            .ok_or_else(|| Error::new("structural.no_results", "the results are empty"))?,
    };
    let members = chosen
        .members
        .iter()
        .filter_map(|m| {
            let mid = id(&m.key)?;
            let physical = store
                .get(mid)
                .and_then(crate::analytical::member_recipe)
                .map(|r| r.physical.iter().filter_map(|g| id(g)).collect())
                .unwrap_or_default();
            let max = |f: &dyn Fn(&[f64; 6]) -> f64| {
                m.stations.iter().map(|s| f(&s.forces)).fold(0.0, f64::max)
            };
            Some(MemberSummary {
                member: mid,
                physical,
                max_axial: max(&|f| f[0].abs()),
                max_shear: max(&|f| f[1].abs().max(f[2].abs())),
                max_moment: max(&|f| moment(f)),
                stations: m.stations.clone(),
            })
        })
        .collect();
    Ok(Report {
        stale: snapshot(store) != s.snapshot,
        job_id: s.job_id,
        at: s.at,
        provider: s.provider,
        warnings: s.results.warnings,
        ignored_check_errors: s.ignored_check_errors,
        cases,
        members,
    })
}

/// An area load's force per true area, global (N/m²): turned from the panel's local axes, and from
/// a projected area to the true one.
fn area_value(pg: &crate::analytical_panels::PanelGeometry, l: &crate::loads::AreaLoadRecipe) -> [f64; 3] {
    let n = pg.normal.unwrap_or([0.0, 0.0, 1.0]);
    let mut v = l.value;
    if l.orientation == crate::loads::Orientation::HostLocal {
        // Local z the normal, x along the first edge, y = z × x.
        let e = [0, 1, 2].map(|k| pg.corners[1][k] - pg.corners[0][k]);
        let le = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt().max(1e-12);
        let x = e.map(|c| c / le);
        let y = [
            n[1] * x[2] - n[2] * x[1],
            n[2] * x[0] - n[0] * x[2],
            n[0] * x[1] - n[1] * x[0],
        ];
        v = [0, 1, 2].map(|k| x[k] * l.value[0] + y[k] * l.value[1] + n[k] * l.value[2]);
    }
    if l.projected {
        let m = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if m > 0.0 {
            let c = ((n[0] * v[0] + n[1] * v[1] + n[2] * v[2]) / m).abs();
            v = v.map(|x| x * c);
        }
    }
    v
}

/// A line load carried from an area load: on `member`, from `from` to `to` (0 to 1 along it),
/// varying from `start` to `end` (N/m, global).
#[derive(Debug, Clone, PartialEq)]
struct Carried {
    member: String,
    from: f64,
    to: f64,
    start: [f64; 3],
    end: [f64; 3],
}

/// The width of a member's tributary strip along it, less the panel's openings: pieces
/// `(t0, t1, w0, w1)` (metres along the member from `a`, widths at their ends), the width varying
/// linearly over each. The strip runs across the member, in the `span` direction, from span
/// coordinate `lo` to `hi`; one piece covers the member where no opening crosses its strip.
fn strip_widths(pg: &crate::analytical_panels::PanelGeometry, span: [f64; 3], a: [f64; 3], u: [f64; 3], l: f64, lo: f64, hi: f64) -> Vec<(f64, f64, f64, f64)> {
    let dot = |x: [f64; 3], y: [f64; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let o = pg.corners[0];
    let c = dot(u, span);
    let det = 1.0 - c * c;
    // Points of the panel as (α along the member from `a`, β along the span), p = a + α u + β span.
    let to2 = |p: [f64; 3]| {
        let d = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
        let (du, ds) = (dot(d, u), dot(d, span));
        [(du - c * ds) / det, (ds - c * du) / det]
    };
    let sa = dot([a[0] - o[0], a[1] - o[1], a[2] - o[2]], span);
    // The strip's bounds in β at α = t: s(a + t u + β span) = sa + t c + β.
    let bounds = |t: f64| (lo - sa - t * c, hi - sa - t * c);
    let holes: Vec<Vec<[f64; 2]>> = pg.recipe.openings.iter().filter(|h| h.len() >= 3).map(|h| h.iter().map(|p| to2(*p)).collect()).collect();
    let full = hi - lo;
    if holes.is_empty() || det < 1e-9 {
        return vec![(0.0, l, full, full)];
    }
    // The strip's width at t: its length less what the openings cover there.
    let width = |t: f64| -> f64 {
        let (b0, b1) = bounds(t);
        let mut cover: Vec<(f64, f64)> = Vec::new();
        for h in &holes {
            let mut xs: Vec<f64> = Vec::new();
            for k in 0..h.len() {
                let (p, q) = (h[k], h[(k + 1) % h.len()]);
                if (p[0] <= t) != (q[0] <= t) {
                    xs.push(p[1] + (q[1] - p[1]) * (t - p[0]) / (q[0] - p[0]));
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks(2) {
                if let [y0, y1] = pair {
                    let (y0, y1) = (y0.max(b0), y1.min(b1));
                    if y1 > y0 {
                        cover.push((y0, y1));
                    }
                }
            }
        }
        cover.sort_by(|x, y| x.0.total_cmp(&y.0));
        let (mut covered, mut end) = (0.0, f64::NEG_INFINITY);
        for (y0, y1) in cover {
            let y0 = y0.max(end);
            if y1 > y0 {
                covered += y1 - y0;
            }
            end = end.max(y1);
        }
        (full - covered).max(0.0)
    };
    // Where the width can change its slope: the openings' corners, and where their edges cross
    // the strip's bounds.
    let mut ts = vec![0.0, l];
    for h in &holes {
        for k in 0..h.len() {
            let (p, q) = (h[k], h[(k + 1) % h.len()]);
            ts.push(p[0]);
            for bound in [lo, hi] {
                // β = bound − sa − t c along the bound; the edge p + λ (q − p).
                let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
                let den = dy + c * dx;
                if den.abs() > 1e-12 {
                    let lam = (bound - sa - c * p[0] - p[1]) / den;
                    if (0.0..=1.0).contains(&lam) {
                        ts.push(p[0] + lam * dx);
                    }
                }
            }
        }
    }
    let mut ts: Vec<f64> = ts.into_iter().filter(|t| (0.0..=l).contains(t)).collect();
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|x, y| (*x - *y).abs() < 1e-6);
    // Each interval's width, linear in it: sampled just inside its ends and extended to them.
    let mut out: Vec<(f64, f64, f64, f64)> = Vec::new();
    for w in ts.windows(2) {
        let (t0, t1) = (w[0], w[1]);
        let e = (t1 - t0) * 1e-3;
        let (wa, wb) = (width(t0 + e), width(t1 - e));
        let slope = (wb - wa) / (t1 - t0 - 2.0 * e);
        let (w0, w1) = ((wa - slope * e).max(0.0), (wb + slope * e).max(0.0));
        // Joined to the last piece when it carries on the same line.
        if let Some(last) = out.last_mut()
            && (last.3 - w0).abs() < 1e-9
            && ((last.3 - last.2) / (last.1 - last.0) - (w1 - w0) / (t1 - t0)).abs() < 1e-9
        {
            last.1 = t1;
            last.3 = w1;
            continue;
        }
        out.push((t0, t1, w0, w1));
    }
    out.retain(|p| p.2 > 0.0 || p.3 > 0.0);
    out
}

/// Carries an area load to the members lying on its panel (STR-LOAD-001): the panel spans one way
/// (its floor's span direction, else along its first edge); members across the span carry the
/// strip halfway to their neighbours (or to the panel's edge), less the panel's openings, as line
/// loads: uniform where nothing is open, and in pieces varying with the strip's width where an
/// opening crosses it. Returns the line loads and a note when some or all of the load could not
/// be carried.
fn carry_area(
    store: &Store,
    pg: &crate::analytical_panels::PanelGeometry,
    value: [f64; 3],
    geometry: &BTreeMap<String, ([f64; 3], [f64; 3])>,
) -> (Vec<Carried>, Option<String>) {
    let d = crate::structural::settings(store).support_distance;
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let unit = |a: [f64; 3]| {
        let l = dot(a, a).sqrt().max(1e-12);
        a.map(|x| x / l)
    };
    let n = pg.normal.unwrap_or([0.0, 0.0, 1.0]);
    let in_plane = |v: [f64; 3]| {
        let k = dot(v, n);
        unit([v[0] - n[0] * k, v[1] - n[1] * k, v[2] - n[2] * k])
    };
    // The span direction.
    let floor_span = pg
        .recipe
        .physical
        .iter()
        .filter_map(|g| GlobalId::parse(g).and_then(|g| store.find_by_guid(&g)))
        .filter_map(|id| crate::structural::span(store.get(id)?))
        .next();
    let span = match floor_span {
        Some(s) => in_plane([s[0], s[1], 0.0]),
        None => in_plane([0, 1, 2].map(|k| pg.corners[1][k] - pg.corners[0][k])),
    };
    let name = |s: &str| format!("the area load on analytical panel {s}");
    let total = dot(value, value).sqrt() * pg.area();
    if pg.circle.is_some() {
        return (
            Vec::new(),
            Some(format!("{}: curved panels do not carry area loads to members yet", name("(curved)"))),
        );
    }
    // Members lying on the panel, across its span.
    // Each member: its GlobalId, span coordinate, length, start and direction.
    type OnPanel = (String, f64, f64, [f64; 3], [f64; 3]);
    let mut on: Vec<OnPanel> = Vec::new();
    for (guid, (a, b)) in geometry {
        if !(pg.contains(*a, d) && pg.contains(*b, d)) {
            continue;
        }
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l = dot(ab, ab).sqrt();
        // In the panel's plane (not a stub running up to it), and across its span.
        if l < 1e-6 || dot(unit(ab), n).abs() > 0.2 || dot(unit(ab), span).abs() > 0.5 {
            continue;
        }
        let mid = [0, 1, 2].map(|k| (a[k] + b[k]) / 2.0);
        let s = dot([mid[0] - pg.corners[0][0], mid[1] - pg.corners[0][1], mid[2] - pg.corners[0][2]], span);
        on.push((guid.clone(), s, l, *a, unit(ab)));
    }
    if on.is_empty() {
        return (
            Vec::new(),
            Some(format!(
                "{}: no members lie on the panel across its span, so the load is not analysed",
                name("")
            )),
        );
    }
    let ss: Vec<f64> = pg
        .outline
        .iter()
        .map(|c| dot([c[0] - pg.corners[0][0], c[1] - pg.corners[0][1], c[2] - pg.corners[0][2]], span))
        .collect();
    let (smin, smax) = (
        ss.iter().cloned().fold(f64::INFINITY, f64::min),
        ss.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    );
    let mut lines: Vec<f64> = on.iter().map(|(_, s, ..)| *s).collect();
    lines.sort_by(|a, b| a.total_cmp(b));
    lines.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    // A member's strip, in its panel's span coordinate.
    let strip = |s: f64| -> (f64, f64) {
        let k = lines
            .iter()
            .position(|x| (x - s).abs() < 0.01)
            .unwrap_or(0);
        let lo = if k == 0 { smin } else { (lines[k - 1] + lines[k]) / 2.0 };
        let hi = if k + 1 == lines.len() { smax } else { (lines[k] + lines[k + 1]) / 2.0 };
        (lo, hi.max(lo))
    };
    let q = dot(value, value).sqrt();
    let mut carried = 0.0;
    let mut out = Vec::new();
    for (g, s, l, a, u) in &on {
        let (lo, hi) = strip(*s);
        let pieces = strip_widths(pg, span, *a, *u, *l, lo, hi);
        for (t0, t1, w0, w1) in pieces {
            carried += q * (w0 + w1) / 2.0 * (t1 - t0);
            out.push(Carried {
                member: g.clone(),
                from: t0 / l,
                to: t1 / l,
                start: value.map(|v| v * w0),
                end: value.map(|v| v * w1),
            });
        }
    }
    let note = (total > 0.0 && (carried - total).abs() / total > 0.05).then(|| {
        format!(
            "{}: the members carry {:.1} kN of its {:.1} kN (members short of the edges, or an irregular \
             outline)",
            name(""),
            carried / 1000.0,
            total / 1000.0
        )
    });
    (out, note)
}
