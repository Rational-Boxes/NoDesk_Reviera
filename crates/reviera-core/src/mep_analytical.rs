// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analytical pipe connections (MEP-PIPE-009): logical connections from an equipment connector
//! to a pipe, for calculation before the short final connections are modelled.
//!
//! - **The record**: an RXD element `rxd:mep/analytical_connections/<guid>` holding the
//!   equipment (GlobalId), its connector and the pipe (GlobalId), and an optional fixed loss.
//! - **Attachment**: the connector's perpendicular projection onto the pipe's centreline,
//!   computed as the model is now (so it follows the equipment and the pipe); beyond the pipe's
//!   ends, the nearer end.
//! - **Rules**: only open pipe connectors whose classification matches the pipe's (its own, or
//!   its system's) are connected; mismatches are refused, or skipped and listed when Reviera
//!   chooses the pipes. The connector counts as connected for Check Systems and Show
//!   Disconnects, joins the pipe's system, and carries flow in the hydronic analysis
//!   (MEP-PIPE-008) with no loss but its fixed one.
//! - **Lifetime**: deleting the equipment or the pipe deletes it; a physical connection on the
//!   same connector replaces it.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{self, Classification, Domain, Port};
use crate::mep_runs;
use crate::mep_systems;
use crate::store::Store;
use crate::txn::{Transaction, Warning};

type V3 = [f64; 3];

pub const CLASS_PREFIX: &str = "rxd:mep/analytical_connections/";
pub const DATA_KEY: &str = "analytical_connection";
/// Search radius when Reviera chooses the pipe (metres).
pub const DEFAULT_RADIUS: f64 = 3.0;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnalyticalConnection {
    /// The equipment (GlobalId).
    pub equipment: String,
    pub connector: usize,
    /// The pipe segment (GlobalId).
    pub pipe: String,
    /// A fixed loss (Pa), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss: Option<f64>,
}

/// The record of an analytical connection element.
pub fn record(e: &Element) -> Option<AnalyticalConnection> {
    if !e.class.starts_with(CLASS_PREFIX) {
        return None;
    }
    serde_json::from_value(e.param(DATA_KEY)?.clone()).ok()
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

/// An analytical connection as it is now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Resolved {
    pub id: ElementId,
    pub equipment: ElementId,
    pub connector: usize,
    pub pipe: ElementId,
    /// Where it attaches on the pipe (metres).
    pub point: V3,
    /// The connector's position.
    pub from: V3,
    /// Beyond the pipe: at its nearer end.
    pub at_end: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss: Option<f64>,
}

/// The perpendicular projection of `p` onto a segment's centreline, or its nearer end: (point,
/// beyond the ends).
fn attach(seg: &mep_runs::SegmentRecipe, p: V3) -> (V3, bool) {
    let legs: Vec<(V3, V3)> = match seg.corner {
        Some(c) => vec![(seg.start, c), (c, seg.end)],
        None => vec![(seg.start, seg.end)],
    };
    let mut best: Option<(f64, V3, bool)> = None;
    for (a, b) in legs {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let t = if len2 > 0.0 { ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1] + (p[2] - a[2]) * d[2]) / len2 } else { 0.0 };
        let outside = !(-1e-9..=1.0 + 1e-9).contains(&t);
        let t = t.clamp(0.0, 1.0);
        let q = [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t];
        let dist = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
        if best.as_ref().is_none_or(|b| dist < b.0) {
            best = Some((dist, q, outside));
        }
    }
    best.map_or((seg.start, true), |b| (b.1, b.2))
}

/// Every analytical connection whose equipment and pipe are still there.
pub fn all(store: &Store) -> Vec<Resolved> {
    store
        .iter()
        .filter_map(|e| {
            let r = record(e)?;
            let eq = find(store, &r.equipment)?;
            let pipe = find(store, &r.pipe)?;
            let seg = store.get(pipe).and_then(mep_runs::segment_recipe)?;
            let port = mep::ports(store, store.get(eq)?).into_iter().find(|p| p.index == r.connector)?;
            let (point, at_end) = attach(&seg, port.position);
            Some(Resolved { id: e.id(), equipment: eq, connector: r.connector, pipe, point, from: port.position, at_end, loss: r.loss })
        })
        .collect()
}

/// The equipment ports joined to pipes analytically.
pub fn connected_ports(store: &Store) -> BTreeSet<(ElementId, usize)> {
    all(store).into_iter().map(|r| (r.equipment, r.connector)).collect()
}

/// The analytical connections that go with deleted elements.
pub fn orphaned(store: &Store, deleted: &BTreeSet<ElementId>) -> Vec<ElementId> {
    let guids: BTreeSet<String> = deleted.iter().filter_map(|d| store.get(*d)).map(|e| e.guid().to_string()).collect();
    store
        .iter()
        .filter(|e| !deleted.contains(&e.id()))
        .filter(|e| record(e).is_some_and(|r| guids.contains(&r.equipment) || guids.contains(&r.pipe)))
        .map(|e| e.id())
        .collect()
}

/// Analytical connections replaced by physical connections on the same connector.
pub(crate) fn replaced(store: &Store, joined: &BTreeSet<(String, usize)>) -> Vec<ElementId> {
    store.iter().filter(|e| record(e).is_some_and(|r| joined.contains(&(r.equipment.clone(), r.connector)))).map(|e| e.id()).collect()
}

/// A pipe's classification: its own, else its system's.
fn pipe_classification(store: &Store, pipe: ElementId, seg: &mep_runs::SegmentRecipe) -> Option<Classification> {
    seg.classification
        .filter(|c| !c.is_wildcard())
        .or_else(|| mep_systems::systems(store).into_iter().find(|s| s.elements.contains(&pipe)).map(|s| s.classification))
}

/// Why `port` cannot be joined to `pipe`, if it cannot.
fn refusal(store: &Store, port: &Port, pipe: ElementId, seg: &mep_runs::SegmentRecipe) -> Option<String> {
    let name = format!("Connector {}", port.index + 1);
    if port.connector.domain != Domain::Pipe {
        return Some(format!("{name} is not a pipe connector"));
    }
    if seg.domain != Domain::Pipe {
        return Some(format!("element {pipe} is not a pipe"));
    }
    let taken = mep::partners(store).contains_key(&(port.element, port.index)) || connected_ports(store).contains(&(port.element, port.index));
    if taken {
        return Some(format!("{name} is already connected"));
    }
    let c = port.connector.classification.filter(|c| !c.is_wildcard());
    match (c, pipe_classification(store, pipe, seg)) {
        (Some(c), Some(pc)) if c != pc => Some(format!("{name}: {}, pipe is {}", c.label(), pc.label())),
        _ => None,
    }
}

fn create_one(txn: &mut Transaction<'_>, eq: ElementId, port: &Port, pipe: ElementId) -> Result<ElementId> {
    let store = txn.store().clone();
    let guid = GlobalId::new();
    let rec = AnalyticalConnection {
        equipment: store.get(eq).expect("exists").guid().to_string(),
        connector: port.index,
        pipe: store.get(pipe).expect("exists").guid().to_string(),
        loss: None,
    };
    let id = txn.create(NewElement::new(format!("{CLASS_PREFIX}{}", guid.as_str())).guid(guid).param(DATA_KEY, serde_json::to_value(&rec).expect("serialises")))?;
    // The equipment joins the pipe's system, where it can.
    if let Some(sys) = mep_systems::systems(&store).into_iter().find(|s| s.elements.contains(&pipe) && port.connector.classification.is_none_or(|c| c == s.classification || c.is_wildcard()))
        && !sys.elements.contains(&eq)
        && sys.equipment != Some(eq)
        && let Err(e) = mep_systems::add_members(txn, sys.id, &[eq])
    {
        txn.warn(Warning { id: "analytical_connection.system".into(), message: format!("Not added to {}: {}", sys.name, e.message), element_ids: vec![eq] });
    }
    Ok(id)
}

fn equipment_port(store: &Store, r: &ElementRef, k: usize) -> Result<(ElementId, Port)> {
    let eq = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such equipment"))?;
    let e = store.get(eq).expect("resolved");
    if mep_runs::is_run_element(e) {
        return Err(bad("analytical_connection.not_equipment", format!("element {eq} is a pipe or fitting, not equipment")));
    }
    let port = mep::ports(store, e).into_iter().find(|p| p.index == k).ok_or_else(|| bad("analytical_connection.no_connector", format!("element {eq} has no connector {k}")))?;
    Ok((eq, port))
}

fn pipe_of(store: &Store, r: &ElementRef) -> Result<(ElementId, mep_runs::SegmentRecipe)> {
    let pipe = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such pipe"))?;
    let seg = store.get(pipe).and_then(mep_runs::segment_recipe).filter(|s| s.domain == Domain::Pipe).ok_or_else(|| bad("analytical_connection.not_pipe", format!("element {pipe} is not a pipe")))?;
    Ok((pipe, seg))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub equipment_id: ElementRef,
    pub connector_index: usize,
    pub pipe_id: ElementRef,
}

/// `analytical_pipe_connection.create`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let (eq, port) = equipment_port(&store, &p.equipment_id, p.connector_index)?;
    let (pipe, seg) = pipe_of(&store, &p.pipe_id)?;
    if let Some(why) = refusal(&store, &port, pipe, &seg) {
        return Err(bad("analytical_connection.refused", why));
    }
    create_one(txn, eq, &port, pipe)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutoParams {
    pub equipment_ids: Vec<ElementRef>,
    /// How far from a connector a pipe may be (metres; 3 by default).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub radius: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AutoReport {
    pub created: Vec<ElementId>,
    /// Connectors left unconnected, and why.
    pub skipped: Vec<String>,
}

/// `analytical_pipe_connection.create_auto`: each open pipe connector of the equipment to the
/// nearest pipe of its classification within the radius.
pub(crate) fn create_auto(txn: &mut Transaction<'_>, p: AutoParams) -> Result<AutoReport> {
    let radius = p.radius.unwrap_or(DEFAULT_RADIUS);
    let mut report = AutoReport { created: Vec::new(), skipped: Vec::new() };
    for r in &p.equipment_ids {
        let store = txn.store().clone();
        let eq = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such equipment"))?;
        let e = store.get(eq).expect("resolved");
        let label = e.name.clone().unwrap_or_else(|| e.class.clone());
        let partners = mep::partners(&store);
        let analytical = connected_ports(&store);
        for port in mep::ports(&store, e).into_iter().filter(|p| p.connector.domain == Domain::Pipe) {
            if partners.contains_key(&(eq, port.index)) || analytical.contains(&(eq, port.index)) {
                continue;
            }
            let store = txn.store().clone();
            // Pipes within reach, nearest first.
            let mut near: Vec<(f64, ElementId, mep_runs::SegmentRecipe)> = store
                .iter()
                .filter_map(|x| {
                    let s = mep_runs::segment_recipe(x).filter(|s| s.domain == Domain::Pipe)?;
                    let (q, _) = attach(&s, port.position);
                    let d = ((q[0] - port.position[0]).powi(2) + (q[1] - port.position[1]).powi(2) + (q[2] - port.position[2]).powi(2)).sqrt();
                    (d <= radius).then_some((d, x.id(), s))
                })
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            match near.iter().find(|(_, pipe, s)| refusal(&store, &port, *pipe, s).is_none()) {
                Some((_, pipe, _)) => report.created.push(create_one(txn, eq, &port, *pipe)?),
                None => report.skipped.push(match near.first() {
                    Some((_, pipe, s)) => format!("{label} {}: {}", eq.0, refusal(&store, &port, *pipe, s).unwrap_or_default()),
                    None => format!("{label} {}: Connector {}: no pipe within {radius} m", eq.0, port.index + 1),
                }),
            }
        }
    }
    if !report.skipped.is_empty() {
        txn.warn(Warning { id: "analytical_connection.skipped".into(), message: format!("Not connected: {}", report.skipped.join("; ")), element_ids: Vec::new() });
    }
    Ok(report)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub connection_id: ElementRef,
    /// A fixed loss (Pa); null clears it.
    pub loss: Option<f64>,
}

/// `analytical_pipe_connection.set`.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.connection_id).ok_or_else(|| bad("element.not_found", "no such connection"))?;
    let mut r = store.get(id).and_then(record).ok_or_else(|| bad("analytical_connection.not_found", format!("element {id} is not an analytical connection")))?;
    if p.loss.is_some_and(|l| !(l >= 0.0 && l.is_finite())) {
        return Err(bad("analytical_connection.bad_loss", "the loss must be zero or more"));
    }
    r.loss = p.loss;
    txn.set_param(id, DATA_KEY, serde_json::to_value(&r).expect("serialises"))
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {}

/// `analytical_pipe_connection.list`.
pub(crate) fn list(store: &Store, _: ListParams) -> Result<Vec<Resolved>> {
    Ok(all(store))
}
