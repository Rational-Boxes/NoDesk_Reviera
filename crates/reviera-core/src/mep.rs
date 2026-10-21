// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! MEP connectors and the connectivity graph (MEP-CONN-001, MEP-CONN-002).
//!
//! - A **connector** is defined on a component type (a custom object type,
//!   `recipe.mesh_object_type.connectors`): a domain (Duct, Pipe, Electrical, Cable Tray,
//!   Conduit), a point and an outward direction in the type's own frame, a shape and size, and
//!   the domain's parameters (system classification, flow direction and configuration, flow,
//!   loss method, fixture units, connection type; or the electrical system, voltage, poles,
//!   load and power factor).
//! - Each placed copy has a **port** per connector: the connector moved into place with the copy.
//!   A copy may override its connectors' system and flow values (`rxd:connector_overrides`);
//!   the geometry is the type's.
//! - **Connections** join two ports (`rxd:mep_connections` on the project, by GlobalId and port
//!   index). Ports of different domains never connect; electrical ports join through circuits,
//!   not here; a duct, pipe, tray or conduit port has one partner, except a conduit surface
//!   connector. Shapes and sizes must match (transitions come with fittings). Placing or moving
//!   a component connects its open ports that coincide with another's (within 0.5 mm, facing
//!   each other within 0.5°).
//! - A **network** is a connected component of the graph; [`network_of`] walks it.
//!
//! In IFC each port is an `IfcDistributionPort` nested in its element (`IfcRelNests`) and each
//! connection an `IfcRelConnectsPorts`, written when the model is saved; the definitions stay in
//! the RXD recipe.
//!
//! Not built yet: connectors placed on faces in a component editor, connector parameters linked
//! to type parameters, fittings and their part types, transitions on size mismatch, dragging
//! connected neighbours, reading connections from IFC files Reviera did not write.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mesh_objects::{self, MeshObjectType};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

/// On the project: the connections.
pub const CONNECTIONS_KEY: &str = "rxd:mep_connections";
/// On a copy: its connectors' overridden values, by connector index.
pub const OVERRIDES_KEY: &str = "rxd:connector_overrides";
/// Ports coincide within this distance (metres) …
pub const POSITION_TOLERANCE: f64 = 0.0005;
/// … facing each other within this angle (degrees).
pub const ANGLE_TOLERANCE: f64 = 0.5;

type V3 = [f64; 3];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    #[default]
    Duct,
    Pipe,
    Electrical,
    CableTray,
    Conduit,
}

impl Domain {
    pub fn label(self) -> &'static str {
        match self {
            Domain::Duct => "Duct",
            Domain::Pipe => "Pipe",
            Domain::Electrical => "Electrical",
            Domain::CableTray => "Cable Tray",
            Domain::Conduit => "Conduit",
        }
    }

    /// `IfcDistributionPortTypeEnum`.
    pub fn ifc_port_type(self) -> &'static str {
        match self {
            Domain::Duct => "DUCT",
            Domain::Pipe => "PIPE",
            Domain::Electrical => "CABLE",
            Domain::CableTray | Domain::Conduit => "CABLECARRIER",
        }
    }
}

/// The fixed system classifications of duct and pipe connectors (MEP-CONN-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    SupplyAir,
    ReturnAir,
    ExhaustAir,
    OtherAir,
    HydronicSupply,
    HydronicReturn,
    DomesticHotWater,
    DomesticColdWater,
    Sanitary,
    Vent,
    /// Rainwater from roofs: gutters and downspouts (MEP-PIPE-010).
    StormDrainage,
    FireProtectionWet,
    FireProtectionDry,
    FireProtectionPreAction,
    FireProtectionOther,
    Other,
    /// Fittings only: takes the system of what it joins.
    Fitting,
    Global,
}

impl Classification {
    pub const ALL: [Classification; 18] = [
        Classification::SupplyAir,
        Classification::ReturnAir,
        Classification::ExhaustAir,
        Classification::OtherAir,
        Classification::HydronicSupply,
        Classification::HydronicReturn,
        Classification::DomesticHotWater,
        Classification::DomesticColdWater,
        Classification::Sanitary,
        Classification::Vent,
        Classification::StormDrainage,
        Classification::FireProtectionWet,
        Classification::FireProtectionDry,
        Classification::FireProtectionPreAction,
        Classification::FireProtectionOther,
        Classification::Other,
        Classification::Fitting,
        Classification::Global,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Classification::SupplyAir => "Supply Air",
            Classification::ReturnAir => "Return Air",
            Classification::ExhaustAir => "Exhaust Air",
            Classification::OtherAir => "Other Air",
            Classification::HydronicSupply => "Hydronic Supply",
            Classification::HydronicReturn => "Hydronic Return",
            Classification::DomesticHotWater => "Domestic Hot Water",
            Classification::DomesticColdWater => "Domestic Cold Water",
            Classification::Sanitary => "Sanitary",
            Classification::Vent => "Vent",
            Classification::StormDrainage => "Storm Drainage",
            Classification::FireProtectionWet => "Fire Protection Wet",
            Classification::FireProtectionDry => "Fire Protection Dry",
            Classification::FireProtectionPreAction => "Fire Protection Pre-Action",
            Classification::FireProtectionOther => "Fire Protection Other",
            Classification::Other => "Other",
            Classification::Fitting => "Fitting",
            Classification::Global => "Global",
        }
    }

    /// Air classifications are duct systems; the others (but Other, Fitting and Global, which
    /// go with either) piping systems.
    pub fn is_air(self) -> bool {
        matches!(
            self,
            Classification::SupplyAir
                | Classification::ReturnAir
                | Classification::ExhaustAir
                | Classification::OtherAir
        )
    }

    /// Whether a connector of this classification may be in a `domain` connector.
    pub fn fits(self, domain: Domain) -> bool {
        match domain {
            Domain::Duct => self.is_air() || matches!(self, Classification::Other | Classification::Fitting | Classification::Global),
            Domain::Pipe => !self.is_air(),
            _ => false,
        }
    }

    /// Fittings and Global take the system of what they join.
    pub fn is_wildcard(self) -> bool {
        matches!(self, Classification::Fitting | Classification::Global)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlowDirection {
    In,
    Out,
    #[default]
    Bidirectional,
}

impl FlowDirection {
    /// `IfcFlowDirectionEnum`.
    pub fn ifc(self) -> &'static str {
        match self {
            FlowDirection::In => "SINK",
            FlowDirection::Out => "SOURCE",
            FlowDirection::Bidirectional => "SOURCEANDSINK",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlowConfig {
    /// Sums the flow downstream.
    #[default]
    Calculated,
    /// A fixed value.
    Preset,
    /// A share of the system's flow (`flow_factor`).
    System,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LossMethod {
    #[default]
    NotDefined,
    Coefficient,
    SpecificLoss,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    #[default]
    Round,
    Rectangular,
    Oval,
}

/// The electrical system of an electrical connector.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ElectricalSystem {
    #[default]
    PowerBalanced,
    PowerUnbalanced,
    Data,
    Telephone,
    Security,
    FireAlarm,
    NurseCall,
    Controls,
    Communication,
}

/// An electrical connector's values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Electrical {
    pub system: ElectricalSystem,
    /// Volts.
    pub voltage: f64,
    pub poles: u8,
    /// Apparent load, VA (per phase where unbalanced: up to three).
    pub load: Vec<f64>,
    pub power_factor: f64,
    /// Lagging (inductive) rather than leading.
    pub lagging: bool,
    pub load_classification: String,
    pub motor: bool,
}

impl Default for Electrical {
    fn default() -> Self {
        Self {
            system: ElectricalSystem::PowerBalanced,
            voltage: 230.0,
            poles: 1,
            load: vec![0.0],
            power_factor: 1.0,
            lagging: true,
            load_classification: "Power".into(),
            motor: false,
        }
    }
}

/// A connector of a component type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Connector {
    #[serde(default)]
    pub name: String,
    pub domain: Domain,
    /// In the type's frame (metres; the insertion point at the origin).
    pub origin: V3,
    /// Outward, in the type's frame (unit).
    pub direction: V3,
    #[serde(default)]
    pub shape: Shape,
    /// Round and oval sizes; rectangular use `width` × `height` (metres).
    #[serde(default)]
    pub diameter: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
    /// Duct and pipe connectors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    #[serde(default)]
    pub flow_direction: FlowDirection,
    #[serde(default)]
    pub flow_config: FlowConfig,
    /// m³/s.
    #[serde(default)]
    pub flow: f64,
    /// 0–1, for `System` flow.
    #[serde(default)]
    pub flow_factor: f64,
    #[serde(default)]
    pub loss_method: LossMethod,
    /// The loss through it: a coefficient (Coefficient, of the velocity pressure at the
    /// connector) or a pressure drop in Pa (Specific Loss).
    #[serde(default)]
    pub loss: f64,
    #[serde(default)]
    pub utility: bool,
    #[serde(default)]
    pub fixture_units: f64,
    /// Pipe connection type (threaded, welded, flanged, …).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub connection_type: String,
    /// A conduit surface connector: any number of conduits anywhere on it.
    #[serde(default)]
    pub surface: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electrical: Option<Electrical>,
}

/// The values of a connector that may be given, added or changed; omitted ones are kept (or
/// defaulted when adding).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectorParams {
    pub name: Option<String>,
    pub domain: Option<Domain>,
    /// In the type's frame (metres).
    pub origin: Option<V3>,
    /// Outward, in the type's frame; scaled to unit length.
    pub direction: Option<V3>,
    pub shape: Option<Shape>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub diameter: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    pub classification: Option<Classification>,
    pub flow_direction: Option<FlowDirection>,
    pub flow_config: Option<FlowConfig>,
    /// m³/s.
    pub flow: Option<f64>,
    pub flow_factor: Option<f64>,
    pub loss_method: Option<LossMethod>,
    /// A coefficient (Coefficient) or Pa (Specific Loss).
    pub loss: Option<f64>,
    pub utility: Option<bool>,
    pub fixture_units: Option<f64>,
    pub connection_type: Option<String>,
    pub surface: Option<bool>,
    pub electrical: Option<Electrical>,
}

impl ConnectorParams {
    /// Only these may be set on a placed copy; the rest belong to the type.
    fn instance_only(&self) -> std::result::Result<(), &'static str> {
        let geometric = [
            ("name", self.name.is_some()),
            ("domain", self.domain.is_some()),
            ("origin", self.origin.is_some()),
            ("direction", self.direction.is_some()),
            ("shape", self.shape.is_some()),
            ("diameter", self.diameter.is_some()),
            ("width", self.width.is_some()),
            ("height", self.height.is_some()),
            ("surface", self.surface.is_some()),
            ("connection_type", self.connection_type.is_some()),
        ];
        match geometric.iter().find(|(_, set)| *set) {
            Some((name, _)) => Err(name),
            None => Ok(()),
        }
    }

    fn apply(&self, c: &mut Connector) {
        macro_rules! take {
            ($($f:ident),*) => { $( if let Some(v) = &self.$f { c.$f = v.clone(); } )* };
        }
        take!(name, domain, origin, shape, diameter, width, height, flow_direction, flow_config, flow, flow_factor, loss_method, loss, utility, fixture_units, connection_type, surface);
        if let Some(d) = self.direction {
            c.direction = d;
        }
        if let Some(v) = self.classification {
            c.classification = Some(v);
        }
        if let Some(v) = &self.electrical {
            c.electrical = Some(v.clone());
        }
    }
}

fn norm(v: V3) -> Option<V3> {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (l > 1e-9 && l.is_finite()).then(|| [v[0] / l, v[1] / l, v[2] / l])
}

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// Checks a connector and puts it in order (unit direction; the domain's own values only).
pub fn validate(mut c: Connector) -> Result<Connector> {
    c.direction = norm(c.direction)
        .ok_or_else(|| bad("connector.direction", "the connector's direction has no length"))?;
    if c.origin.iter().any(|v| !v.is_finite()) {
        return Err(bad("connector.origin", "the connector's point is not a number"));
    }
    let carries_size = !matches!(c.domain, Domain::Electrical);
    if carries_size {
        match c.shape {
            Shape::Round => {
                if c.diameter <= 0.0 {
                    return Err(bad("connector.size", "a round connector needs a diameter above zero"));
                }
            }
            Shape::Rectangular | Shape::Oval => {
                if c.width <= 0.0 || c.height <= 0.0 {
                    return Err(bad("connector.size", "a rectangular or oval connector needs a width and height above zero"));
                }
            }
        }
        if matches!(c.domain, Domain::Pipe | Domain::CableTray | Domain::Conduit) && c.shape == Shape::Oval {
            return Err(bad("connector.shape", format!("{} connectors are not oval", c.domain.label())));
        }
    }
    if c.domain == Domain::Pipe && c.shape != Shape::Round {
        return Err(bad("connector.shape", "pipe connectors are round"));
    }
    if !(c.loss.is_finite() && c.loss >= 0.0) {
        return Err(bad("connector.loss", "a loss is a number of zero or more"));
    }
    match c.domain {
        Domain::Duct | Domain::Pipe => {
            let cl = c.classification.unwrap_or(if c.domain == Domain::Duct {
                Classification::SupplyAir
            } else {
                Classification::DomesticColdWater
            });
            if !cl.fits(c.domain) {
                return Err(bad(
                    "connector.classification",
                    format!("a {} connector cannot be {}", c.domain.label().to_lowercase(), cl.label()),
                ));
            }
            c.classification = Some(cl);
            c.electrical = None;
        }
        Domain::Electrical => {
            c.classification = None;
            let e = c.electrical.get_or_insert_with(Electrical::default);
            if e.voltage.is_nan() || e.voltage < 0.0 || !(0.0..=1.0).contains(&e.power_factor) || !(1..=3).contains(&e.poles) {
                return Err(bad("connector.electrical", "an electrical connector needs a voltage of 0 or more, 1 to 3 poles and a power factor from 0 to 1"));
            }
            if e.load.is_empty() {
                e.load = vec![0.0];
            }
        }
        Domain::CableTray | Domain::Conduit => {
            c.classification = None;
            c.electrical = None;
        }
    }
    if c.surface && c.domain != Domain::Conduit {
        return Err(bad("connector.surface", "only conduit connectors can be surface connectors"));
    }
    if !(0.0..=1.0).contains(&c.flow_factor) {
        return Err(bad("connector.flow_factor", "the flow factor is from 0 to 1"));
    }
    if c.flow < 0.0 || c.fixture_units < 0.0 {
        return Err(bad("connector.flow", "flow and fixture units cannot be negative"));
    }
    Ok(c)
}

pub(crate) fn new_connector(domain: Domain) -> Connector {
    Connector {
        name: String::new(),
        domain,
        origin: [0.0; 3],
        direction: [0.0, 0.0, 1.0],
        shape: Shape::Round,
        diameter: 0.0,
        width: 0.0,
        height: 0.0,
        classification: None,
        flow_direction: FlowDirection::Bidirectional,
        flow_config: FlowConfig::Calculated,
        flow: 0.0,
        flow_factor: 0.0,
        loss_method: LossMethod::NotDefined,
        loss: 0.0,
        utility: false,
        fixture_units: 0.0,
        connection_type: String::new(),
        surface: false,
        electrical: None,
    }
}

// ---- Ports ----

/// A placed copy's connector: where it is now and its values.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Port {
    pub element: ElementId,
    pub index: usize,
    pub connector: Connector,
    /// Project coordinates (metres).
    pub position: V3,
    /// Outward (unit).
    pub direction: V3,
}

/// The connectors of a type element (a custom object type).
pub fn type_connectors(e: &Element) -> Option<Vec<Connector>> {
    mesh_objects::mesh_object_type(e).map(|t| t.connectors)
}

fn overrides(e: &Element) -> BTreeMap<usize, ConnectorParams> {
    e.param(OVERRIDES_KEY)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// An element's ports: a component copy's connectors, or a duct or pipe segment's or fitting's
/// ends (none for other elements).
pub fn ports(store: &Store, e: &Element) -> Vec<Port> {
    if crate::mep_gutters::gutter_recipe(e).is_some() {
        return crate::mep_gutters::ports(store, e);
    }
    if mesh_objects::mesh_object_recipe(e).is_none() {
        return crate::mep_runs::ports(store, e);
    }
    let Ok(g) = mesh_objects::regenerate(store, e) else {
        return Vec::new();
    };
    if g.ty.connectors.is_empty() {
        return Vec::new();
    }
    let over = overrides(e);
    g.ty.connectors
        .iter()
        .enumerate()
        .map(|(index, c)| {
            let mut c = c.clone();
            if let Some(o) = over.get(&index) {
                o.apply(&mut c);
            }
            Port {
                element: e.id(),
                index,
                position: g.placement.apply(c.origin),
                direction: g.placement.apply_direction(c.direction),
                connector: c,
            }
        })
        .collect()
}

/// Every port of the model.
pub fn all_ports(store: &Store) -> Vec<Port> {
    store
        .iter()
        .filter(|e| mesh_objects::mesh_object_recipe(e).is_some() || crate::mep_runs::is_run_element(e) || crate::mep_gutters::gutter_recipe(e).is_some())
        .flat_map(|e| ports(store, e))
        .collect()
}

/// One port by element and index.
pub fn port(store: &Store, element: ElementId, index: usize) -> Option<Port> {
    ports(store, store.get(element)?).into_iter().nth(index)
}

// ---- Connections ----

/// A port by its element's GlobalId and connector index (as stored).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct StoredPort {
    pub element: String,
    pub port: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    pub a: StoredPort,
    pub b: StoredPort,
}

/// A port as commands take it.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortRef {
    pub element: ElementRef,
    /// The connector index (0 is the first).
    pub port: usize,
}

/// A connection between two ports, as reported.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
pub struct Connection {
    pub a: (ElementId, usize),
    pub b: (ElementId, usize),
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub(crate) fn stored_links(store: &Store) -> Vec<Link> {
    project(store)
        .and_then(|p| p.param(CONNECTIONS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn resolve(store: &Store, p: &StoredPort) -> Option<(ElementId, usize)> {
    let id = store.find_by_guid(&GlobalId::parse(&p.element)?)?;
    let e = store.get(id)?;
    let n = match mesh_objects::type_of(store, e) {
        Some((_, t)) => t.connectors.len(),
        None => crate::mep_runs::port_count(e),
    };
    (p.port < n).then_some((id, p.port))
}

/// The connections whose ports both still exist.
pub fn connections(store: &Store) -> Vec<Connection> {
    stored_links(store)
        .iter()
        .filter_map(|l| {
            Some(Connection {
                a: resolve(store, &l.a)?,
                b: resolve(store, &l.b)?,
            })
        })
        .collect()
}

/// Each port's partners.
pub fn partners(store: &Store) -> HashMap<(ElementId, usize), Vec<(ElementId, usize)>> {
    let mut m: HashMap<(ElementId, usize), Vec<(ElementId, usize)>> = HashMap::new();
    for c in connections(store) {
        m.entry(c.a).or_default().push(c.b);
        m.entry(c.b).or_default().push(c.a);
    }
    m
}

pub(crate) fn save_links(txn: &mut Transaction<'_>, links: Vec<Link>) -> Result<()> {
    let store = txn.store().clone();
    // Links to elements since deleted are dropped as the list is written.
    let mut keep: Vec<Link> = links
        .into_iter()
        .filter(|l| resolve(&store, &l.a).is_some() && resolve(&store, &l.b).is_some())
        .collect();
    keep.sort();
    keep.dedup();
    let pid = project(&store)
        .map(|p| p.id())
        .ok_or_else(|| bad("project.missing", "the model has no project"))?;
    txn.set_param(pid, CONNECTIONS_KEY, serde_json::to_value(keep).expect("serialises"))
}

pub(crate) fn stored(store: &Store, id: ElementId, port: usize) -> StoredPort {
    StoredPort {
        element: store.get(id).map(|e| e.guid().to_string()).unwrap_or_default(),
        port,
    }
}

pub(crate) fn same_size(a: &Connector, b: &Connector) -> bool {
    let eq = |x: f64, y: f64| (x - y).abs() <= POSITION_TOLERANCE;
    if a.domain == Domain::Electrical {
        return true;
    }
    a.shape == b.shape
        && match a.shape {
            Shape::Round => eq(a.diameter, b.diameter),
            Shape::Rectangular | Shape::Oval => eq(a.width, b.width) && eq(a.height, b.height),
        }
}

fn size_text(c: &Connector) -> String {
    match c.shape {
        Shape::Round => format!("Ø{:.0} mm", c.diameter * 1000.0),
        Shape::Rectangular | Shape::Oval => format!("{:.0}×{:.0} mm", c.width * 1000.0, c.height * 1000.0),
    }
}

/// Whether two ports may be joined, and why not.
fn check_pair(a: &Port, b: &Port, taken: &HashMap<(ElementId, usize), Vec<(ElementId, usize)>>) -> Result<()> {
    if a.element == b.element && a.index == b.index {
        return Err(bad("mep.same_port", "a port cannot connect to itself"));
    }
    if a.connector.domain != b.connector.domain {
        return Err(bad(
            "mep.domain",
            format!(
                "a {} connector cannot connect to a {} connector",
                a.connector.domain.label().to_lowercase(),
                b.connector.domain.label().to_lowercase()
            ),
        ));
    }
    if a.connector.domain == Domain::Electrical {
        return Err(bad("mep.electrical", "electrical connectors join through circuits, not connections"));
    }
    for p in [a, b] {
        let used = taken.get(&(p.element, p.index)).map_or(0, Vec::len);
        if used > 0 && !p.connector.surface {
            return Err(bad(
                "mep.taken",
                format!("port {} of element {} already has a partner", p.index, p.element),
            ));
        }
    }
    if !same_size(&a.connector, &b.connector) {
        return Err(bad(
            "mep.size",
            format!(
                "Connectors differ in size or shape and no transition is available ({} and {})",
                size_text(&a.connector),
                size_text(&b.connector)
            ),
        ));
    }
    Ok(())
}

fn port_of(store: &Store, r: &PortRef) -> Result<Port> {
    let id = store
        .resolve(&r.element)
        .ok_or_else(|| bad("element.not_found", "no such element"))?;
    let e = store.get(id).expect("resolved");
    let ps = ports(store, e);
    if ps.is_empty() {
        return Err(bad("mep.no_connectors", format!("element {id} has no connectors")));
    }
    ps.into_iter().nth(r.port).ok_or_else(|| {
        bad("mep.no_port", format!("element {id} has no port {}", r.port))
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectParams {
    pub port_a: PortRef,
    pub port_b: PortRef,
}

/// Joins two ports (`mep.connect`).
pub(crate) fn connect(txn: &mut Transaction<'_>, p: ConnectParams) -> Result<Connection> {
    let store = txn.store().clone();
    let (a, b) = (port_of(&store, &p.port_a)?, port_of(&store, &p.port_b)?);
    check_pair(&a, &b, &partners(&store))?;
    let mut links = stored_links(&store);
    links.push(Link {
        a: stored(&store, a.element, a.index),
        b: stored(&store, b.element, b.index),
    });
    save_links(txn, links)?;
    Ok(Connection {
        a: (a.element, a.index),
        b: (b.element, b.index),
    })
}

/// Separates two ports (`mep.disconnect`); refused when they are not connected.
pub(crate) fn disconnect(txn: &mut Transaction<'_>, p: ConnectParams) -> Result<()> {
    let store = txn.store().clone();
    let (a, b) = (port_of(&store, &p.port_a)?, port_of(&store, &p.port_b)?);
    let (sa, sb) = (stored(&store, a.element, a.index), stored(&store, b.element, b.index));
    let links = stored_links(&store);
    let n = links.len();
    let left: Vec<Link> = links
        .into_iter()
        .filter(|l| !((l.a == sa && l.b == sb) || (l.a == sb && l.b == sa)))
        .collect();
    if left.len() == n {
        return Err(bad("mep.not_connected", "the two ports are not connected"));
    }
    save_links(txn, left)
}

fn coincide(a: &Port, b: &Port) -> bool {
    let d = (0..3).map(|k| (a.position[k] - b.position[k]).powi(2)).sum::<f64>().sqrt();
    let dot: f64 = (0..3).map(|k| a.direction[k] * b.direction[k]).sum();
    d <= POSITION_TOLERANCE && dot <= -(ANGLE_TOLERANCE.to_radians().cos())
}

/// Connects the open ports of `ids` to open ports of other elements that coincide with them;
/// returns the connections made.
pub(crate) fn connect_coincident(txn: &mut Transaction<'_>, ids: &[ElementId]) -> Result<Vec<Connection>> {
    let store = txn.store().clone();
    let mine: Vec<Port> = ids
        .iter()
        .filter_map(|i| store.get(*i))
        .flat_map(|e| ports(&store, e))
        .filter(|p| p.connector.domain != Domain::Electrical)
        .collect();
    if mine.is_empty() {
        return Ok(Vec::new());
    }
    let mut taken = partners(&store);
    let others: Vec<Port> = all_ports(&store)
        .into_iter()
        .filter(|p| p.connector.domain != Domain::Electrical)
        .collect();
    let mut links = stored_links(&store);
    let mut made = Vec::new();
    for a in &mine {
        for b in &others {
            if b.element == a.element || !coincide(a, b) || check_pair(a, b, &taken).is_err() {
                continue;
            }
            taken.entry((a.element, a.index)).or_default().push((b.element, b.index));
            taken.entry((b.element, b.index)).or_default().push((a.element, a.index));
            links.push(Link {
                a: stored(&store, a.element, a.index),
                b: stored(&store, b.element, b.index),
            });
            made.push(Connection {
                a: (a.element, a.index),
                b: (b.element, b.index),
            });
        }
    }
    if !made.is_empty() {
        save_links(txn, links)?;
    }
    Ok(made)
}

/// After a move: a connection whose two ports met before and no longer do is dropped, with a
/// warning listing the newly open ports; moved copies connect where they meet.
pub(crate) fn after_move(txn: &mut Transaction<'_>, before: &Store, moved: &[ElementId], copy: bool) -> Result<()> {
    if copy {
        connect_coincident(txn, moved)?;
        return Ok(());
    }
    let now = txn.store().clone();
    let links = stored_links(&now);
    let mut keep = Vec::new();
    let mut opened: Vec<ElementId> = Vec::new();
    let mut opened_ports = Vec::new();
    for l in links {
        let (Some(a), Some(b)) = (resolve(&now, &l.a), resolve(&now, &l.b)) else {
            keep.push(l);
            continue;
        };
        if !moved.contains(&a.0) && !moved.contains(&b.0) {
            keep.push(l);
            continue;
        }
        let met = |s: &Store| match (port(s, a.0, a.1), port(s, b.0, b.1)) {
            (Some(p), Some(q)) => coincide(&p, &q),
            _ => false,
        };
        if met(before) && !met(&now) {
            for (e, k) in [a, b] {
                if !opened.contains(&e) {
                    opened.push(e);
                }
                opened_ports.push(format!("{} port {}", e.0, k));
            }
        } else {
            keep.push(l);
        }
    }
    if !opened.is_empty() {
        save_links(txn, keep)?;
        txn.warn(crate::txn::Warning {
            id: "mep.connection_dropped".into(),
            message: format!("The move separated connected connectors; now open: {}", opened_ports.join(", ")),
            element_ids: opened,
        });
    }
    // What moved may now meet other open ports.
    connect_coincident(txn, moved)?;
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectTouchingParams {
    /// The elements whose open ports connect; every element with connectors when empty.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
}

/// Connects coinciding open ports (`mep.connect_touching`).
pub(crate) fn connect_touching(txn: &mut Transaction<'_>, p: ConnectTouchingParams) -> Result<Vec<Connection>> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = if p.element_ids.is_empty() {
        all_ports(&store).iter().map(|p| p.element).collect::<BTreeSet<_>>().into_iter().collect()
    } else {
        p.element_ids
            .iter()
            .map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element")))
            .collect::<Result<_>>()?
    };
    connect_coincident(txn, &ids)
}

/// The elements of `start`'s network (it included), in the order reached.
pub fn network(store: &Store, start: ElementId) -> Vec<ElementId> {
    let mut adj: HashMap<ElementId, BTreeSet<ElementId>> = HashMap::new();
    for c in connections(store) {
        adj.entry(c.a.0).or_default().insert(c.b.0);
        adj.entry(c.b.0).or_default().insert(c.a.0);
    }
    let mut seen = BTreeSet::from([start]);
    let mut order = vec![start];
    let mut queue = VecDeque::from([start]);
    while let Some(e) = queue.pop_front() {
        for n in adj.get(&e).into_iter().flatten() {
            if seen.insert(*n) {
                order.push(*n);
                queue.push_back(*n);
            }
        }
    }
    order
}

/// The networks (connected components) of the elements with ports; each sorted.
pub fn networks(store: &Store) -> Vec<Vec<ElementId>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let mut ids: Vec<ElementId> = all_ports(store).iter().map(|p| p.element).collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        if seen.contains(&id) {
            continue;
        }
        let mut n = network(store, id);
        n.sort();
        seen.extend(n.iter().copied());
        out.push(n);
    }
    out
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NetworkParams {
    pub element_id: ElementRef,
}

/// `mep.network_of`.
pub(crate) fn network_of(store: &Store, p: NetworkParams) -> Result<Vec<ElementId>> {
    let id = store
        .resolve(&p.element_id)
        .ok_or_else(|| bad("element.not_found", "no such element"))?;
    Ok(network(store, id))
}

/// Ports without a partner, optionally of some domains. Surface connectors are never open;
/// electrical ports join through circuits and are not listed.
pub fn open_ports(store: &Store, domains: &[Domain]) -> Vec<Port> {
    let taken = partners(store);
    all_ports(store)
        .into_iter()
        .filter(|p| domains.is_empty() || domains.contains(&p.connector.domain))
        .filter(|p| p.connector.domain != Domain::Electrical)
        .filter(|p| !p.connector.surface && !taken.contains_key(&(p.element, p.index)))
        .collect()
}

// ---- Connector commands ----

fn resolve_type(store: &Store, r: &ElementRef) -> Result<(ElementId, MeshObjectType)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| bad("element.not_found", "no such component type"))?;
    let t = mesh_objects::mesh_object_type(store.get(id).expect("resolved")).ok_or_else(|| {
        bad("connector.not_a_type", format!("element {id} is not a component type (connectors are defined on types)"))
    })?;
    Ok((id, t))
}

pub(crate) fn write_type(txn: &mut Transaction<'_>, id: ElementId, t: &MeshObjectType) -> Result<()> {
    let value = serde_json::to_value(t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddParams {
    /// The component type.
    pub type_id: ElementRef,
    pub domain: Domain,
    #[serde(flatten)]
    pub params: ConnectorParams,
}

/// Adds a connector to a component type (`connector.add`); returns its index.
pub(crate) fn add(txn: &mut Transaction<'_>, p: AddParams) -> Result<usize> {
    let store = txn.store().clone();
    let (id, mut t) = resolve_type(&store, &p.type_id)?;
    let mut c = new_connector(p.domain);
    let mut params = p.params;
    params.domain = None;
    params.apply(&mut c);
    t.connectors.push(validate(c)?);
    write_type(txn, id, &t)?;
    Ok(t.connectors.len() - 1)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    /// A component type (its connector's definition) or a placed copy (its port's system and
    /// flow values only).
    pub element_id: ElementRef,
    pub connector_index: usize,
    #[serde(flatten)]
    pub params: ConnectorParams,
}

/// Changes a connector (`connector.set`).
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.element_id)
        .ok_or_else(|| bad("element.not_found", "no such element"))?;
    let e = store.get(id).expect("resolved");
    if let Some(mut t) = mesh_objects::mesh_object_type(e) {
        let n = t.connectors.len();
        let c = t.connectors.get_mut(p.connector_index).ok_or_else(|| {
            bad("connector.no_index", format!("the type has {n} connectors"))
        })?;
        let mut next = c.clone();
        p.params.apply(&mut next);
        *c = validate(next)?;
        return write_type(txn, id, &t);
    }
    let (_, t) = mesh_objects::type_of(&store, e)
        .ok_or_else(|| bad("mep.no_connectors", format!("element {id} has no connectors")))?;
    let base = t.connectors.get(p.connector_index).cloned().ok_or_else(|| {
        bad("connector.no_index", format!("the element's type has {} connectors", t.connectors.len()))
    })?;
    if let Err(field) = p.params.instance_only() {
        return Err(bad(
            "connector.type_value",
            format!("`{field}` belongs to the connector's type; change it on the type"),
        ));
    }
    let mut over = overrides(e);
    let entry = over.entry(p.connector_index).or_default();
    // Merge the new values over the copy's earlier ones, then check the result.
    macro_rules! merge {
        ($($f:ident),*) => { $( if p.params.$f.is_some() { entry.$f = p.params.$f.clone(); } )* };
    }
    merge!(classification, flow_direction, flow_config, flow, flow_factor, loss_method, loss, utility, fixture_units, electrical);
    let mut check = base;
    entry.apply(&mut check);
    validate(check)?;
    let value = serde_json::to_value(&over).expect("serialises");
    txn.set_param(id, OVERRIDES_KEY, value)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveParams {
    pub type_id: ElementRef,
    pub connector_index: usize,
}

/// Removes a connector from a type (`connector.remove`): connections to it are dropped and the
/// copies' later connectors keep their connections and values.
pub(crate) fn remove(txn: &mut Transaction<'_>, p: RemoveParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut t) = resolve_type(&store, &p.type_id)?;
    let k = p.connector_index;
    if k >= t.connectors.len() {
        return Err(bad("connector.no_index", format!("the type has {} connectors", t.connectors.len())));
    }
    t.connectors.remove(k);
    write_type(txn, id, &t)?;
    let type_guid = store.get(id).expect("resolved").guid().to_string();
    let copies: Vec<&Element> = store
        .iter()
        .filter(|e| e.param("@type").and_then(Value::as_str) == Some(type_guid.as_str()))
        .collect();
    let guids: BTreeSet<String> = copies.iter().map(|e| e.guid().to_string()).collect();
    let shift = |s: &StoredPort| -> Option<StoredPort> {
        if !guids.contains(&s.element) {
            return Some(s.clone());
        }
        match s.port.cmp(&k) {
            std::cmp::Ordering::Less => Some(s.clone()),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(StoredPort { element: s.element.clone(), port: s.port - 1 }),
        }
    };
    let links: Vec<Link> = stored_links(&store)
        .iter()
        .filter_map(|l| Some(Link { a: shift(&l.a)?, b: shift(&l.b)? }))
        .collect();
    // Links are written before the copies' overrides so `save_links` sees the new count.
    let pid = project(&store).map(|p| p.id()).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    txn.set_param(pid, CONNECTIONS_KEY, serde_json::to_value(links).expect("serialises"))?;
    for e in copies {
        let over = overrides(e);
        if over.is_empty() {
            continue;
        }
        let moved: BTreeMap<usize, ConnectorParams> = over
            .into_iter()
            .filter(|(i, _)| *i != k)
            .map(|(i, v)| (if i > k { i - 1 } else { i }, v))
            .collect();
        txn.set_param(e.id(), OVERRIDES_KEY, serde_json::to_value(moved).expect("serialises"))?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// A component type or a placed copy.
    pub element_id: ElementRef,
}

/// A connector as listed: for a copy, where its port is and what it joins.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ConnectorInfo {
    pub index: usize,
    pub connector: Connector,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<V3>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<V3>,
    /// The ports it is connected to (element, index).
    pub connected_to: Vec<(ElementId, usize)>,
    /// The systems it belongs to.
    pub systems: Vec<ElementId>,
}

/// `connector.list`.
pub(crate) fn list(store: &Store, p: ListParams) -> Result<Vec<ConnectorInfo>> {
    let id = store
        .resolve(&p.element_id)
        .ok_or_else(|| bad("element.not_found", "no such element"))?;
    let e = store.get(id).expect("resolved");
    if let Some(t) = mesh_objects::mesh_object_type(e) {
        return Ok(t
            .connectors
            .into_iter()
            .enumerate()
            .map(|(index, connector)| ConnectorInfo {
                index,
                connector,
                position: None,
                direction: None,
                connected_to: Vec::new(),
                systems: Vec::new(),
            })
            .collect());
    }
    let taken = partners(store);
    let systems = crate::mep_systems::port_systems(store);
    Ok(ports(store, e)
        .into_iter()
        .map(|p| ConnectorInfo {
            index: p.index,
            position: Some(p.position),
            direction: Some(p.direction),
            connected_to: taken.get(&(p.element, p.index)).cloned().unwrap_or_default(),
            systems: systems.get(&(p.element, p.index)).cloned().unwrap_or_default(),
            connector: p.connector,
        })
        .collect())
}

// ---- Starter component types ----

/// A box-shaped component type made by Reviera, with its connectors (`mep_type.create`).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTypeParams {
    pub name: String,
    pub category: mesh_objects::Category,
    /// Width (x), depth (y) and height (z), metres; the insertion point is the bottom centre.
    pub size: V3,
    /// RGBA, 0–1.
    #[serde(default)]
    pub color: Option<[f32; 4]>,
    #[serde(default)]
    pub connectors: Vec<AddConnector>,
    /// The IFC predefined type of its copies (THERMOSTAT, CO2SENSOR, …).
    #[serde(default)]
    pub predefined: Option<String>,
}

/// A connector of a new type.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddConnector {
    pub domain: Domain,
    #[serde(flatten)]
    pub params: ConnectorParams,
}

fn box_mesh(size: V3) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let [w, d, h] = size;
    let (x, y) = (w / 2.0, d / 2.0);
    let p = vec![
        [-x, -y, 0.0],
        [x, -y, 0.0],
        [x, y, 0.0],
        [-x, y, 0.0],
        [-x, -y, h],
        [x, -y, h],
        [x, y, h],
        [-x, y, h],
    ];
    let t = vec![
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    (p, t)
}

/// Makes a box-shaped component type with connectors (`mep_type.create`).
pub(crate) fn create_type(txn: &mut Transaction<'_>, p: CreateTypeParams) -> Result<ElementId> {
    if p.size.iter().any(|v| !(*v > 0.0 && v.is_finite())) {
        return Err(bad("mep_type.size", "the width, depth and height must be above zero"));
    }
    let connectors = p
        .connectors
        .into_iter()
        .map(|a| {
            let mut c = new_connector(a.domain);
            let mut params = a.params;
            params.domain = None;
            params.apply(&mut c);
            validate(c)
        })
        .collect::<Result<Vec<_>>>()?;
    let (positions, triangles) = box_mesh(p.size);
    let color = p.color.unwrap_or([0.72, 0.74, 0.78, 1.0]);
    mesh_objects::create_generated_type(txn, &p.name, p.category, &positions, &triangles, color, connectors, p.predefined.as_deref())
}

/// One starter type: name, category, size, colour and connectors.
struct Starter {
    name: &'static str,
    category: mesh_objects::Category,
    size: V3,
    color: [f32; 4],
    connectors: Vec<Connector>,
}

#[allow(clippy::too_many_arguments)]
fn duct(name: &str, origin: V3, direction: V3, cl: Classification, dir: FlowDirection, config: FlowConfig, flow: f64, d: f64) -> Connector {
    Connector {
        name: name.into(),
        origin,
        direction,
        diameter: d,
        classification: Some(cl),
        flow_direction: dir,
        flow_config: config,
        flow,
        ..new_connector(Domain::Duct)
    }
}

fn pipe(name: &str, origin: V3, direction: V3, cl: Classification, dir: FlowDirection, d: f64, fu: f64) -> Connector {
    Connector {
        name: name.into(),
        origin,
        direction,
        diameter: d,
        classification: Some(cl),
        flow_direction: dir,
        fixture_units: fu,
        ..new_connector(Domain::Pipe)
    }
}

#[allow(clippy::too_many_arguments)]
fn power(name: &str, origin: V3, direction: V3, dir: FlowDirection, voltage: f64, poles: u8, va: f64, class: &str) -> Connector {
    Connector {
        name: name.into(),
        origin,
        direction,
        flow_direction: dir,
        electrical: Some(Electrical {
            voltage,
            poles,
            load: vec![va],
            load_classification: class.into(),
            ..Electrical::default()
        }),
        ..new_connector(Domain::Electrical)
    }
}

/// The starter types: Reviera's own simple shapes and provisional sizes and flows, for laying out
/// systems before manufacturers' content is loaded.
fn starters() -> Vec<Starter> {
    use mesh_objects::Category as C;
    use Classification as K;
    use FlowConfig as F;
    use FlowDirection as D;
    let up = [0.0, 0.0, 1.0];
    let down = [0.0, 0.0, -1.0];
    let grey = [0.70, 0.72, 0.76, 1.0];
    let white = [0.93, 0.93, 0.92, 1.0];
    vec![
        Starter {
            name: "Air Handling Unit",
            category: C::MechanicalEquipment,
            size: [3.0, 1.2, 1.5],
            color: grey,
            connectors: vec![
                duct("Supply", [1.5, 0.0, 0.9], [1.0, 0.0, 0.0], K::SupplyAir, D::Out, F::Calculated, 0.0, 0.4),
                duct("Return", [-1.5, 0.0, 0.9], [-1.0, 0.0, 0.0], K::ReturnAir, D::In, F::Calculated, 0.0, 0.4),
                power("Power", [0.0, -0.6, 0.3], [0.0, -1.0, 0.0], D::In, 400.0, 3, 7500.0, "HVAC"),
            ],
        },
        Starter {
            name: "Supply Diffuser 600",
            category: C::AirTerminal,
            size: [0.6, 0.6, 0.15],
            color: white,
            connectors: vec![duct("Supply", [0.0, 0.0, 0.15], up, K::SupplyAir, D::In, F::Preset, 0.1, 0.25)],
        },
        Starter {
            name: "Return Grille 600",
            category: C::AirTerminal,
            size: [0.6, 0.6, 0.1],
            color: white,
            connectors: vec![duct("Return", [0.0, 0.0, 0.1], up, K::ReturnAir, D::Out, F::Preset, 0.1, 0.25)],
        },
        Starter {
            name: "Exhaust Fan",
            category: C::MechanicalEquipment,
            size: [0.5, 0.5, 0.4],
            color: grey,
            connectors: vec![
                duct("Inlet", [0.0, 0.0, 0.0], down, K::ExhaustAir, D::In, F::Calculated, 0.0, 0.2),
                duct("Outlet", [0.0, 0.0, 0.4], up, K::ExhaustAir, D::Out, F::Calculated, 0.0, 0.2),
            ],
        },
        Starter {
            name: "Water Heater",
            category: C::MechanicalEquipment,
            size: [0.6, 0.6, 1.5],
            color: white,
            connectors: vec![
                pipe("Cold In", [-0.1, 0.0, 1.5], up, K::DomesticColdWater, D::In, 0.022, 0.0),
                pipe("Hot Out", [0.1, 0.0, 1.5], up, K::DomesticHotWater, D::Out, 0.022, 0.0),
            ],
        },
        Starter {
            name: "Circulation Pump",
            category: C::MechanicalEquipment,
            size: [0.4, 0.25, 0.3],
            color: [0.20, 0.36, 0.62, 1.0],
            connectors: vec![
                pipe("Suction", [-0.2, 0.0, 0.15], [-1.0, 0.0, 0.0], K::HydronicReturn, D::In, 0.05, 0.0),
                pipe("Discharge", [0.2, 0.0, 0.15], [1.0, 0.0, 0.0], K::HydronicSupply, D::Out, 0.05, 0.0),
            ],
        },
        Starter {
            name: "Lavatory",
            category: C::PlumbingFixture,
            size: [0.55, 0.45, 0.85],
            color: white,
            connectors: vec![
                pipe("Cold", [-0.08, 0.2, 0.55], [0.0, 1.0, 0.0], K::DomesticColdWater, D::In, 0.015, 1.0),
                pipe("Hot", [0.08, 0.2, 0.55], [0.0, 1.0, 0.0], K::DomesticHotWater, D::In, 0.015, 1.0),
                pipe("Waste", [0.0, 0.2, 0.4], [0.0, 1.0, 0.0], K::Sanitary, D::Out, 0.04, 1.0),
            ],
        },
        Starter {
            name: "Water Closet",
            category: C::PlumbingFixture,
            size: [0.4, 0.7, 0.8],
            color: white,
            connectors: vec![
                pipe("Cold", [0.1, 0.33, 0.2], [0.0, 1.0, 0.0], K::DomesticColdWater, D::In, 0.015, 2.5),
                pipe("Waste", [0.0, 0.2, 0.0], down, K::Sanitary, D::Out, 0.1, 4.0),
            ],
        },
        Starter {
            name: "Distribution Board",
            category: C::ElectricalEquipment,
            size: [0.6, 0.2, 1.0],
            color: grey,
            connectors: vec![
                power("Supply", [0.0, 0.0, 1.0], up, D::Out, 400.0, 3, 0.0, "Power"),
                power("Feed", [0.0, 0.0, 0.0], down, D::In, 400.0, 3, 0.0, "Power"),
            ],
        },
        Starter {
            name: "Transformer",
            category: C::Transformer,
            size: [1.2, 0.8, 1.4],
            color: grey,
            connectors: vec![
                power("Primary", [0.0, 0.0, 0.0], down, D::In, 400.0, 3, 0.0, "Power"),
                power("Secondary", [0.0, 0.0, 1.4], up, D::Out, 208.0, 3, 0.0, "Power"),
            ],
        },
        Starter {
            name: "Receptacle",
            category: C::ElectricalFixture,
            size: [0.08, 0.04, 0.08],
            color: white,
            connectors: vec![power("Power", [0.0, 0.02, 0.04], [0.0, 1.0, 0.0], D::In, 230.0, 1, 180.0, "Power")],
        },
        Starter {
            name: "Ceiling Luminaire",
            category: C::LightingFixture,
            size: [0.6, 0.6, 0.08],
            color: white,
            connectors: vec![power("Power", [0.0, 0.0, 0.08], up, D::In, 230.0, 1, 36.0, "Lighting")],
        },
        // Control devices (MEP-DUCT-012): their insertion point at the back, against the wall,
        // facing +y; a controls connector at the back.
        Starter { name: "Thermostat", category: C::ControlElement, size: [0.08, 0.025, 0.12], color: white, connectors: vec![controls([0.0, 0.0, 0.06])] },
        Starter { name: "Humidistat", category: C::ControlElement, size: [0.08, 0.025, 0.12], color: white, connectors: vec![controls([0.0, 0.0, 0.06])] },
        Starter { name: "Temperature Sensor", category: C::Sensor, size: [0.07, 0.025, 0.07], color: white, connectors: vec![controls([0.0, 0.0, 0.035])] },
        Starter { name: "CO2 Sensor", category: C::Sensor, size: [0.09, 0.03, 0.09], color: white, connectors: vec![controls([0.0, 0.0, 0.045])] },
        Starter { name: "Humidity Sensor", category: C::Sensor, size: [0.07, 0.025, 0.07], color: white, connectors: vec![controls([0.0, 0.0, 0.035])] },
        Starter { name: "Occupancy Sensor", category: C::Sensor, size: [0.1, 0.04, 0.1], color: white, connectors: vec![controls([0.0, 0.0, 0.05])] },
        Starter { name: "Pressure Sensor", category: C::Sensor, size: [0.08, 0.05, 0.08], color: grey, connectors: vec![controls([0.0, 0.0, 0.04])] },
        Starter { name: "DDC Controller", category: C::Controller, size: [0.3, 0.1, 0.4], color: grey, connectors: vec![controls([0.0, 0.0, 0.2])] },
        Starter {
            name: "VAV Box",
            category: C::AirTerminalBox,
            size: [0.9, 0.5, 0.35],
            color: grey,
            connectors: vec![
                duct("Inlet", [-0.45, 0.0, 0.175], [-1.0, 0.0, 0.0], K::SupplyAir, D::In, F::Calculated, 0.0, 0.25),
                duct("Outlet", [0.45, 0.0, 0.175], [1.0, 0.0, 0.0], K::SupplyAir, D::Out, F::Calculated, 0.0, 0.25),
            ],
        },
    ]
}

/// A control device's controls connector, at its back.
fn controls(origin: V3) -> Connector {
    Connector {
        name: "Controls".into(),
        origin,
        direction: [0.0, -1.0, 0.0],
        flow_direction: FlowDirection::In,
        electrical: Some(Electrical {
            system: ElectricalSystem::Controls,
            voltage: 24.0,
            poles: 1,
            load: vec![2.0],
            load_classification: "Other".into(),
            ..Electrical::default()
        }),
        ..new_connector(Domain::Electrical)
    }
}

/// A starter type's IFC predefined type, where its category's is not enough.
fn starter_predefined(name: &str) -> Option<&'static str> {
    Some(match name {
        "Thermostat" => "THERMOSTAT",
        "Humidistat" => "HUMIDISTAT",
        "Temperature Sensor" => "TEMPERATURESENSOR",
        "CO2 Sensor" => "CO2SENSOR",
        "Humidity Sensor" => "HUMIDITYSENSOR",
        "Occupancy Sensor" => "MOVEMENTSENSOR",
        "Pressure Sensor" => "PRESSURESENSOR",
        "DDC Controller" => "PROGRAMMABLE",
        "VAV Box" => "VARIABLEFLOWPRESSUREDEPENDANT",
        _ => return None,
    })
}

/// The starter types' names.
pub fn starter_names() -> Vec<&'static str> {
    starters().iter().map(|s| s.name).collect()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoadStartersParams {
    /// Which (by name); all when empty.
    #[serde(default)]
    pub names: Vec<String>,
}

/// Loads starter MEP component types (`mep_type.load_starters`); a type of the same name already
/// in the model is reused. Returns the types by name.
pub(crate) fn load_starters(txn: &mut Transaction<'_>, p: LoadStartersParams) -> Result<BTreeMap<String, ElementId>> {
    let all = starters();
    for n in &p.names {
        if !all.iter().any(|s| s.name == n) {
            return Err(bad("mep_type.no_starter", format!("no starter type {n:?}; there are: {}", starter_names().join(", "))));
        }
    }
    let mut out = BTreeMap::new();
    for s in all.into_iter().filter(|s| p.names.is_empty() || p.names.iter().any(|n| n == s.name)) {
        let store = txn.store().clone();
        let existing = store.iter().find(|e| {
            e.name.as_deref() == Some(s.name) && mesh_objects::mesh_object_type(e).is_some()
        });
        let id = match existing {
            Some(e) => e.id(),
            None => {
                let connectors = s.connectors.into_iter().map(validate).collect::<Result<Vec<_>>>()?;
                let (positions, triangles) = box_mesh(s.size);
                mesh_objects::create_generated_type(txn, s.name, s.category, &positions, &triangles, s.color, connectors, starter_predefined(s.name))?
            }
        };
        out.insert(s.name.to_owned(), id);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifications_fit_their_domains() {
        assert!(Classification::SupplyAir.fits(Domain::Duct));
        assert!(!Classification::SupplyAir.fits(Domain::Pipe));
        assert!(Classification::Sanitary.fits(Domain::Pipe));
        assert!(!Classification::Sanitary.fits(Domain::Duct));
        assert!(!Classification::Sanitary.fits(Domain::Electrical));
    }

    #[test]
    fn a_connector_is_checked() {
        let mut c = new_connector(Domain::Duct);
        assert_eq!(validate(c.clone()).unwrap_err().code, "connector.size");
        c.diameter = 0.25;
        c.direction = [0.0, 0.0, 2.0];
        let v = validate(c.clone()).unwrap();
        assert_eq!(v.direction, [0.0, 0.0, 1.0]);
        assert_eq!(v.classification, Some(Classification::SupplyAir));
        c.classification = Some(Classification::Sanitary);
        assert_eq!(validate(c).unwrap_err().code, "connector.classification");
        let mut p = new_connector(Domain::Pipe);
        p.shape = Shape::Rectangular;
        p.width = 0.1;
        p.height = 0.1;
        assert_eq!(validate(p).unwrap_err().code, "connector.shape");
        for s in starters() {
            for c in s.connectors {
                validate(c).unwrap_or_else(|e| panic!("{}: {e:?}", s.name));
            }
        }
    }
}
