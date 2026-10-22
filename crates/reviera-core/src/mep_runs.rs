// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Ducts, pipes, cable tray, conduit and their fittings (MEP-DUCT-001, MEP-DUCT-002,
//! MEP-DUCT-003, MEP-PIPE-001, MEP-ELEC-016, MEP-ELEC-017).
//!
//! - Cable tray and conduit are `IfcCableCarrierSegment` and `…Fitting` (boxes
//!   `IfcJunctionBox`). Their bends have a radius (tray: width × the type's multiplier; conduit:
//!   the standard's radius for the diameter, else diameter × multiplier). A type **without
//!   fittings** draws its bends as curved segments, and refuses fittings placed by hand.
//!   Conduit sizes come from the type's standard (Electrical Settings).
//!
//! - A **segment type** (`IfcDuctSegmentType`, `IfcPipeSegmentType` with
//!   `recipe.mep_segment_type`) has a domain, a shape and **routing preferences**: per role
//!   (elbow, junction, cross, transition, cap) an ordered list of parts with a size range, tried
//!   from the top; and a preferred junction (tee or tap).
//! - A **segment** (`IfcDuctSegment`, `IfcPipeSegment`, RIGIDSEGMENT) runs straight from `start`
//!   to `end` with a size, and has a port at each end, plus one per tap on its side.
//! - A **fitting** (`IfcDuctFitting`, `IfcPipeFitting`) is one of Reviera's own parts: a mitred
//!   elbow, a tee, a tap, a cross, a transition or a cap. Its legs run from its centre; the end
//!   of each leg is a port.
//! - **Drawing** a run along points puts an elbow at each corner (a change of elevation is a
//!   riser between two elbows), segments between them, and a tee or tap where the run ends on
//!   an existing segment (chosen by the junction rows); where no row fits, nothing is inserted,
//!   the ends are left open and a warning names the role and size. Its ends connect to the
//!   ports they meet, and it joins the system of what it connects to. Pipes may slope.
//!
//! The parts' sizes are Reviera's own: an elbow's and a tee's legs are 0.6 of the size plus
//! 50 mm long; a transition is as long as the size change times 2.5 (at least 150 mm).
//!
//! Not built yet: fittings and segments from component types, radius elbows, the Angles
//! setting, offsets and justification other than the centre, size and elevation locks, moving
//! and stretching runs with their fittings (flex duct is in `mep_flex`), flip and rotate, the fitting controls in
//! the view ("+" and "−" are commands: [`upgrade`], [`downgrade`]).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{self, Classification, Connector, Domain, FlowDirection, Port, Shape};
use crate::store::Store;
use crate::txn::{Transaction, Warning};
use crate::walls::RECIPE_KEY;

pub const TYPE_RECIPE: &str = "recipe.mep_segment_type";
pub const SEGMENT: &str = "recipe.mep_segment";
pub const FITTING: &str = "recipe.mep_fitting";

type V3 = [f64; 3];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> Option<V3> {
    let l = len(a);
    (l > 1e-9).then(|| mul(a, 1.0 / l))
}

// ---- Sizes ----

/// A duct or pipe size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Size {
    pub shape: Shape,
    /// Round (metres).
    #[serde(default)]
    pub diameter: f64,
    /// Rectangular and oval: across (horizontal on level runs) and up (metres).
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
}

impl Size {
    /// The size routing preferences compare: the diameter, or the larger side.
    pub fn nominal(&self) -> f64 {
        match self.shape {
            Shape::Round => self.diameter,
            _ => self.width.max(self.height),
        }
    }

    fn check(&self, domain: Domain) -> Result<()> {
        let ok = match self.shape {
            Shape::Round => self.diameter > 0.0 && self.diameter.is_finite(),
            _ => self.width > 0.0 && self.height > 0.0 && self.width.is_finite() && self.height.is_finite(),
        };
        if !ok {
            return Err(bad("mep_run.size", "give a diameter, or a width and height, above zero"));
        }
        if matches!(domain, Domain::Pipe | Domain::Conduit) && self.shape != Shape::Round {
            return Err(bad("mep_run.shape", format!("{} is round", domain.label().to_lowercase())));
        }
        if domain == Domain::CableTray && self.shape != Shape::Rectangular {
            return Err(bad("mep_run.shape", "cable tray is rectangular"));
        }
        Ok(())
    }

    pub fn text(&self) -> String {
        match self.shape {
            Shape::Round => format!("Ø{:.0}", self.diameter * 1000.0),
            _ => format!("{:.0}×{:.0}", self.width * 1000.0, self.height * 1000.0),
        }
    }

    fn connector(&self, domain: Domain) -> Connector {
        Connector {
            shape: self.shape,
            diameter: self.diameter,
            width: self.width,
            height: self.height,
            ..mep::new_connector(domain)
        }
    }
}

// ---- Types and routing preferences ----

/// The parts Reviera makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Elbow,
    Tee,
    Tap,
    Cross,
    Transition,
    Cap,
    /// A coupling (union) joining two lengths.
    Coupling,
    /// A pull or junction box (conduit).
    Box,
    /// Pipe accessories (MEP-PIPE-004): inline valves, meters and strainers, and flanges.
    Valve,
    Meter,
    Strainer,
    Flange,
    /// Duct accessories (MEP-DUCT-004): dampers, filters, coils and sound attenuators.
    Damper,
    Filter,
    Coil,
    Silencer,
}

impl Part {
    /// Inline accessories, not fittings (IfcValve, IfcFlowMeter, IfcFilter).
    pub fn is_accessory(self) -> bool {
        matches!(self, Part::Valve | Part::Meter | Part::Strainer | Part::Damper | Part::Filter | Part::Coil | Part::Silencer)
    }

    pub fn label(self) -> &'static str {
        match self {
            Part::Valve => "Valve",
            Part::Meter => "Meter",
            Part::Strainer => "Strainer",
            Part::Flange => "Flange",
            Part::Damper => "Damper",
            Part::Filter => "Filter",
            Part::Coil => "Coil",
            Part::Silencer => "Sound Attenuator",
            Part::Elbow => "Elbow",
            Part::Tee => "Tee",
            Part::Tap => "Tap",
            Part::Cross => "Cross",
            Part::Transition => "Transition",
            Part::Cap => "Cap",
            Part::Coupling => "Coupling",
            Part::Box => "Junction Box",
        }
    }

    /// The predefined type and object type of a fitting of this part in a domain.
    pub fn ifc_in(self, domain: Domain) -> (&'static str, Option<&'static str>) {
        if matches!(domain, Domain::CableTray | Domain::Conduit) {
            return match self {
                Part::Elbow => ("BEND", None),
                Part::Tee | Part::Tap => ("TEE", None),
                Part::Cross => ("CROSS", None),
                Part::Transition => ("REDUCER", None),
                Part::Cap => ("USERDEFINED", Some("CAP")),
                Part::Coupling => ("USERDEFINED", Some("COUPLING")),
                Part::Box => ("POWER", None),
                _ => ("NOTDEFINED", None),
            };
        }
        match self {
            Part::Coupling => ("CONNECTOR", None),
            Part::Flange => ("CONNECTOR", Some("FLANGE")),
            Part::Valve => ("ISOLATING", None),
            Part::Meter => ("WATERMETER", None),
            Part::Strainer => ("STRAINER", None),
            p => (p.ifc(), None),
        }
    }

    /// `IfcDuctFittingTypeEnum` / `IfcPipeFittingTypeEnum`.
    pub fn ifc(self) -> &'static str {
        match self {
            Part::Elbow => "BEND",
            Part::Tee | Part::Tap | Part::Cross => "JUNCTION",
            Part::Transition => "TRANSITION",
            Part::Cap => "OBSTRUCTION",
            Part::Coupling | Part::Flange => "CONNECTOR",
            Part::Box => "JUNCTION",
            Part::Valve | Part::Meter | Part::Strainer | Part::Damper | Part::Filter | Part::Coil | Part::Silencer => "NOTDEFINED",
        }
    }
}

/// One row of a role: a part for sizes from `min` to `max` (metres; `max` 0 for no limit).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Row {
    pub part: Part,
    #[serde(default)]
    pub min: f64,
    #[serde(default)]
    pub max: f64,
}

impl Row {
    fn fits(&self, size: f64) -> bool {
        size + 1e-9 >= self.min && (self.max <= 0.0 || size <= self.max + 1e-9)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Junction {
    #[default]
    Tee,
    Tap,
}

/// Routing preferences (MEP-DUCT-002).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Routing {
    pub elbow: Vec<Row>,
    pub junction: Vec<Row>,
    pub preferred_junction: Junction,
    pub cross: Vec<Row>,
    pub transition: Vec<Row>,
    pub cap: Vec<Row>,
    /// Couplings where a straight is split; none: the halves join directly.
    pub union: Vec<Row>,
}

impl Default for Routing {
    fn default() -> Self {
        let any = |part| vec![Row { part, min: 0.0, max: 0.0 }];
        Self {
            elbow: any(Part::Elbow),
            junction: any(Part::Tee),
            preferred_junction: Junction::Tee,
            cross: any(Part::Cross),
            transition: any(Part::Transition),
            cap: any(Part::Cap),
            union: Vec::new(),
        }
    }
}

impl Routing {
    fn rows(&self, role: Role) -> &[Row] {
        match role {
            Role::Elbow => &self.elbow,
            Role::Junction => &self.junction,
            Role::Cross => &self.cross,
            Role::Transition => &self.transition,
            Role::Cap => &self.cap,
            Role::Union => &self.union,
        }
    }

    /// The part for a role and size: the first row that fits, the preferred junction kind first
    /// among junction rows that fit.
    pub fn choose(&self, role: Role, size: f64) -> Option<Part> {
        let fitting: Vec<&Row> = self.rows(role).iter().filter(|r| r.fits(size)).collect();
        if role == Role::Junction {
            let want = match self.preferred_junction {
                Junction::Tee => Part::Tee,
                Junction::Tap => Part::Tap,
            };
            if let Some(r) = fitting.iter().find(|r| r.part == want) {
                return Some(r.part);
            }
        }
        fitting.first().map(|r| r.part)
    }

    fn check(&self) -> Result<()> {
        for (role, rows, allowed) in [
            ("elbow", &self.elbow, &[Part::Elbow][..]),
            ("junction", &self.junction, &[Part::Tee, Part::Tap][..]),
            ("cross", &self.cross, &[Part::Cross][..]),
            ("transition", &self.transition, &[Part::Transition][..]),
            ("cap", &self.cap, &[Part::Cap][..]),
            ("union", &self.union, &[Part::Coupling, Part::Flange][..]),
        ] {
            for r in rows {
                if !allowed.contains(&r.part) {
                    return Err(bad("routing.part", format!("a {} cannot be a {role} row", r.part.label().to_lowercase())));
                }
                if r.min < 0.0 || (r.max > 0.0 && r.max < r.min) {
                    return Err(bad("routing.range", format!("a {role} row's size range runs from {} to {}", r.min, r.max)));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Elbow,
    Junction,
    Cross,
    Transition,
    Cap,
    Union,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Role::Elbow => "elbow",
            Role::Junction => "junction",
            Role::Cross => "cross",
            Role::Transition => "transition",
            Role::Cap => "cap",
            Role::Union => "union",
        }
    }
}

/// The stored recipe of a segment type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentType {
    pub kind: String,
    pub domain: Domain,
    pub shape: Shape,
    #[serde(default)]
    pub routing: Routing,
    #[serde(default)]
    pub material: String,
    /// Absolute roughness (metres).
    #[serde(default)]
    pub roughness: f64,
    /// Tray and conduit: bends are fittings; without, they are curved segments.
    #[serde(default = "yes")]
    pub with_fittings: bool,
    /// Cable tray: ladder, channel, solid bottom or trough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tray_kind: Option<TrayKind>,
    /// Tray and conduit: bend radius over width or diameter (where the standard gives none).
    #[serde(default = "one")]
    pub bend_radius_multiplier: f64,
    /// Conduit: its standard's id (Electrical Settings).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard: Option<String>,
    /// The sizes sizing chooses from (metres: diameters, or rectangle sides); the domain's
    /// standard series when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<f64>,
    /// Flexible duct (MEP-DUCT-005): drawn as a spline (`flex_duct.create`);
    /// `bend_radius_multiplier` is then its minimum bend radius over the diameter (or width).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flexible: bool,
    /// Flexible: the friction loss multiplier for compressed flex (MEP-CALC-002).
    #[serde(default = "one")]
    pub loss_multiplier: f64,
}

fn yes() -> bool {
    true
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrayKind {
    #[default]
    Ladder,
    Channel,
    SolidBottom,
    Trough,
}

impl TrayKind {
    /// `IfcCableCarrierSegmentTypeEnum`.
    pub fn ifc(self) -> &'static str {
        match self {
            TrayKind::Ladder => "CABLELADDERSEGMENT",
            TrayKind::Channel | TrayKind::SolidBottom => "CABLETRAYSEGMENT",
            TrayKind::Trough => "CABLETRUNKINGSEGMENT",
        }
    }
}

impl SegmentType {
    /// The sizes sizing chooses from: the type's table, or the domain's series (round ducts
    /// EN 1506's 100–1250 mm, rectangle sides 100–2000 mm, pipes DN 10–300).
    pub fn sizing_sizes(&self, shape: Shape) -> Vec<f64> {
        if !self.sizes.is_empty() {
            return self.sizes.clone();
        }
        let mm: &[f64] = match (self.domain, shape) {
            (Domain::Duct, Shape::Round) => &[100.0, 125.0, 150.0, 160.0, 200.0, 250.0, 315.0, 355.0, 400.0, 450.0, 500.0, 560.0, 630.0, 710.0, 800.0, 900.0, 1000.0, 1120.0, 1250.0],
            (Domain::Duct, _) => &[100.0, 150.0, 200.0, 250.0, 300.0, 350.0, 400.0, 450.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0, 1200.0, 1400.0, 1600.0, 1800.0, 2000.0],
            _ => &[10.0, 15.0, 20.0, 25.0, 32.0, 40.0, 50.0, 65.0, 80.0, 100.0, 125.0, 150.0, 200.0, 250.0, 300.0],
        };
        mm.iter().map(|v| v / 1000.0).collect()
    }

    /// The segment's predefined type.
    fn predefined(&self) -> &'static str {
        if self.flexible {
            return "FLEXIBLESEGMENT";
        }
        match self.domain {
            Domain::CableTray => self.tray_kind.unwrap_or_default().ifc(),
            Domain::Conduit => "CONDUITSEGMENT",
            _ => "RIGIDSEGMENT",
        }
    }

    /// Whether its bends have a radius (tray and conduit).
    fn radius_bends(&self) -> bool {
        matches!(self.domain, Domain::CableTray | Domain::Conduit)
    }

    /// Checks a size against the type: its shape, and for conduit its standard.
    pub(crate) fn check_size(&self, store: &Store, size: &Size) -> Result<()> {
        size.check(self.domain)?;
        if self.domain == Domain::Conduit
            && let Some(id) = &self.standard
        {
            let s = crate::mep_electrical::settings(store);
            let std = s
                .conduit_standards
                .iter()
                .find(|c| &c.id == id)
                .ok_or_else(|| bad("conduit_standard.not_found", format!("the type's conduit standard {id:?} is not in the Electrical Settings")))?;
            if std.size(size.diameter).is_none() {
                let list: Vec<String> = std.sizes.iter().map(|z| format!("{:.1}", z.diameter * 1000.0).trim_end_matches(".0").to_owned()).collect();
                return Err(bad(
                    "conduit.size",
                    format!("{} conduit comes in {} mm, not {:.1} mm", std.name, list.join(", "), size.diameter * 1000.0),
                ));
            }
        }
        Ok(())
    }

    /// The default bend radius of a size (metres).
    pub fn bend_radius(&self, store: &Store, size: &Size) -> f64 {
        if self.domain == Domain::Conduit
            && let Some(r) = self.standard.as_ref().and_then(|id| {
                crate::mep_electrical::settings(store).conduit_standards.into_iter().find(|c| &c.id == id)?.size(size.diameter)?.bend_radius
            })
        {
            return r;
        }
        let base = if size.shape == Shape::Round { size.diameter } else { size.width };
        base * self.bend_radius_multiplier
    }
}

pub fn segment_type(e: &Element) -> Option<SegmentType> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == TYPE_RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

pub(crate) fn type_class(domain: Domain) -> &'static str {
    match domain {
        Domain::Pipe => "IfcPipeSegmentType",
        Domain::CableTray | Domain::Conduit => "IfcCableCarrierSegmentType",
        _ => "IfcDuctSegmentType",
    }
}

pub(crate) fn segment_class(domain: Domain) -> &'static str {
    match domain {
        Domain::Pipe => "IfcPipeSegment",
        Domain::CableTray | Domain::Conduit => "IfcCableCarrierSegment",
        _ => "IfcDuctSegment",
    }
}

fn fitting_class(domain: Domain, part: Part) -> &'static str {
    match (domain, part) {
        (_, Part::Box) => "IfcJunctionBox",
        (_, Part::Valve) => "IfcValve",
        (_, Part::Meter) => "IfcFlowMeter",
        (_, Part::Strainer | Part::Filter) => "IfcFilter",
        (_, Part::Damper) => "IfcDamper",
        (_, Part::Coil) => "IfcCoil",
        (_, Part::Silencer) => "IfcDuctSilencer",
        (Domain::Pipe, _) => "IfcPipeFitting",
        (Domain::CableTray | Domain::Conduit, _) => "IfcCableCarrierFitting",
        _ => "IfcDuctFitting",
    }
}

/// The built-in type names, by domain and shape.
fn default_name(domain: Domain, shape: Shape) -> &'static str {
    match (domain, shape) {
        (Domain::Pipe, _) => "Standard Pipe",
        (Domain::CableTray, _) => "Ladder Cable Tray",
        (Domain::Conduit, _) => "Conduit",
        (_, Shape::Round) => "Round Duct",
        (_, Shape::Rectangular) => "Rectangular Duct",
        (_, Shape::Oval) => "Oval Duct",
    }
}

/// The type of a domain and shape: an existing one of the built-in name, or a new one.
fn default_type(txn: &mut Transaction<'_>, domain: Domain, shape: Shape) -> Result<ElementId> {
    let name = default_name(domain, shape);
    let store = txn.store().clone();
    if let Some(e) = store
        .iter()
        .find(|e| e.name.as_deref() == Some(name) && segment_type(e).is_some_and(|t| t.domain == domain && t.shape == shape))
    {
        return Ok(e.id());
    }
    let t = SegmentType {
        kind: TYPE_RECIPE.into(),
        domain,
        shape,
        routing: default_routing(domain),
        material: if domain == Domain::Pipe { "Copper".into() } else { "Galvanised Steel".into() },
        roughness: match domain {
            Domain::Pipe => 1.5e-6,
            Domain::Duct => 9.0e-5,
            _ => 0.0,
        },
        with_fittings: true,
        tray_kind: (domain == Domain::CableTray).then_some(TrayKind::Ladder),
        bend_radius_multiplier: default_multiplier(domain),
        standard: (domain == Domain::Conduit).then(|| "iec_61386".into()),
        sizes: Vec::new(),
        flexible: false,
        loss_multiplier: 1.0,
    };
    txn.create(
        NewElement::new(type_class(domain))
            .name(name)
            .param("@PredefinedType", t.predefined())
            .param(RECIPE_KEY, serde_json::to_value(&t).expect("serialises")),
    )
}

/// A domain's default routing: conduit splits with a coupling.
fn default_routing(domain: Domain) -> Routing {
    let mut r = Routing::default();
    if domain == Domain::Conduit {
        r.union = vec![Row { part: Part::Coupling, min: 0.0, max: 0.0 }];
    }
    // Duct: a tap where flex duct starts on a duct's side (MEP-DUCT-005); tees stay preferred.
    if domain == Domain::Duct {
        r.junction.push(Row { part: Part::Tap, min: 0.0, max: 0.0 });
    }
    // Pipe: flanges at flanged accessories from DN 50 (E-26).
    if domain == Domain::Pipe {
        r.union = vec![Row { part: Part::Flange, min: 0.05, max: 0.0 }];
    }
    r
}

/// The default bend radius multiplier: 1 × width for tray, 6 × diameter for conduit.
fn default_multiplier(domain: Domain) -> f64 {
    if domain == Domain::Conduit { 6.0 } else { 1.0 }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTypeParams {
    pub domain: Domain,
    pub name: String,
    #[serde(default)]
    pub shape: Shape,
    #[serde(default)]
    pub routing: Option<Routing>,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub roughness: Option<f64>,
    /// Tray and conduit: bends are fittings (default) or curved segments.
    pub with_fittings: Option<bool>,
    /// Cable tray: ladder (default), channel, solid_bottom, trough.
    pub tray_kind: Option<TrayKind>,
    /// Tray and conduit: bend radius over width or diameter (default 1 for tray, 6 for conduit).
    pub bend_radius_multiplier: Option<f64>,
    /// Conduit: a standard's id (default `iec_61386`).
    pub standard: Option<String>,
    /// Duct and pipe: the sizes sizing chooses from (metres; diameters, or rectangle sides).
    pub sizes: Option<Vec<f64>>,
    /// Duct: a flexible duct type (drawn with `flex_duct.create`; its bend radius multiplier is
    /// the minimum bend radius over the diameter, default 1).
    pub flexible: Option<bool>,
    /// Flexible: the friction loss multiplier for compressed flex (default 1).
    pub loss_multiplier: Option<f64>,
}

/// Makes a duct, pipe, cable tray or conduit type (`mep_segment_type.create`).
pub(crate) fn create_type(txn: &mut Transaction<'_>, p: CreateTypeParams) -> Result<ElementId> {
    if p.domain == Domain::Electrical {
        return Err(bad("mep_segment_type.domain", "segment types are duct, pipe, cable tray or conduit types"));
    }
    let shape = match p.domain {
        Domain::Conduit | Domain::Pipe => Shape::Round,
        Domain::CableTray => Shape::Rectangular,
        _ => p.shape,
    };
    if shape != p.shape && p.shape != Shape::default() {
        return Err(bad("mep_run.shape", format!("{} is {}", p.domain.label().to_lowercase(), if shape == Shape::Round { "round" } else { "rectangular" })));
    }
    let m = p.bend_radius_multiplier.unwrap_or(default_multiplier(p.domain));
    if !(m > 0.0 && m <= 100.0) {
        return Err(bad("mep_segment_type.bend_radius", "the bend radius multiplier runs above 0 to 100"));
    }
    let standard = match (p.domain, p.standard) {
        (Domain::Conduit, Some(id)) => {
            if !crate::mep_electrical::settings(txn.store()).conduit_standards.iter().any(|c| c.id == id) {
                return Err(bad("conduit_standard.not_found", format!("no conduit standard {id:?}")));
            }
            Some(id)
        }
        (Domain::Conduit, None) => Some("iec_61386".into()),
        (_, Some(_)) => return Err(bad("mep_segment_type.standard", "only conduit types have a standard")),
        (_, None) => None,
    };
    let name = p.name.trim().to_owned();
    if name.is_empty() {
        return Err(bad("mep_segment_type.name", "a type needs a name"));
    }
    if txn.store().iter().any(|e| segment_type(e).is_some() && e.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&name))) {
        return Err(bad("mep_segment_type.duplicate", format!("there is already a type named {name:?}")));
    }
    let routing = p.routing.unwrap_or_else(|| default_routing(p.domain));
    routing.check()?;
    let flexible = p.flexible.unwrap_or(false);
    if flexible && p.domain != Domain::Duct {
        return Err(bad("mep_segment_type.flexible", "flexible types are duct types (flex pipe is not built yet)"));
    }
    let loss_multiplier = p.loss_multiplier.unwrap_or(1.0);
    if !(1.0..=10.0).contains(&loss_multiplier) {
        return Err(bad("mep_segment_type.loss_multiplier", "the loss multiplier runs from 1 to 10"));
    }
    if p.loss_multiplier.is_some() && !flexible {
        return Err(bad("mep_segment_type.loss_multiplier", "only flexible types have a loss multiplier"));
    }
    let radius = matches!(p.domain, Domain::CableTray | Domain::Conduit);
    let t = SegmentType {
        kind: TYPE_RECIPE.into(),
        domain: p.domain,
        shape,
        routing,
        material: p.material.unwrap_or_default(),
        roughness: p.roughness.unwrap_or(0.0),
        with_fittings: !radius || p.with_fittings.unwrap_or(true),
        tray_kind: (p.domain == Domain::CableTray).then(|| p.tray_kind.unwrap_or_default()),
        bend_radius_multiplier: m,
        standard,
        sizes: {
            let mut v = p.sizes.unwrap_or_default();
            if v.iter().any(|x| !(*x > 0.0 && *x < 10.0)) {
                return Err(bad("mep_segment_type.sizes", "sizes run above 0 to 10 m"));
            }
            v.sort_by(f64::total_cmp);
            v.dedup();
            v
        },
        flexible,
        loss_multiplier,
    };
    txn.create(
        NewElement::new(type_class(p.domain))
            .name(name)
            .param("@PredefinedType", t.predefined())
            .param(RECIPE_KEY, serde_json::to_value(&t).expect("serialises")),
    )
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListTypesParams {
    pub domain: Option<Domain>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TypeInfo {
    pub id: ElementId,
    pub name: String,
    #[serde(flatten)]
    pub segment_type: SegmentType,
}

/// `mep_segment_type.list`.
pub(crate) fn list_types(store: &Store, p: ListTypesParams) -> Result<Vec<TypeInfo>> {
    Ok(store
        .iter()
        .filter_map(|e| {
            let t = segment_type(e)?;
            (p.domain.is_none_or(|d| d == t.domain)).then(|| TypeInfo { id: e.id(), name: e.name.clone().unwrap_or_default(), segment_type: t })
        })
        .collect())
}

pub(crate) fn resolve_type(store: &Store, r: &ElementRef) -> Result<(ElementId, SegmentType)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such duct or pipe type"))?;
    let t = segment_type(store.get(id).expect("resolved"))
        .ok_or_else(|| bad("mep_segment_type.not_a_type", format!("element {id} is not a duct or pipe type")))?;
    Ok((id, t))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutingGetParams {
    pub type_id: ElementRef,
}

/// `routing_preferences.get`.
pub(crate) fn routing_get(store: &Store, p: RoutingGetParams) -> Result<Routing> {
    Ok(resolve_type(store, &p.type_id)?.1.routing)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutingSetParams {
    pub type_id: ElementRef,
    pub routing: Routing,
}

/// `routing_preferences.set`: used for runs drawn from now on (`mep_run.reapply_type` refits
/// existing ones).
pub(crate) fn routing_set(txn: &mut Transaction<'_>, p: RoutingSetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut t) = resolve_type(&store, &p.type_id)?;
    p.routing.check()?;
    t.routing = p.routing;
    let v = serde_json::to_value(&t).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

// ---- Segments and fittings ----

/// A tap on a segment's side: its point on the axis and outward direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapPort {
    pub at: V3,
    pub direction: V3,
    pub size: Size,
}

/// The stored recipe of a segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentRecipe {
    pub kind: String,
    pub domain: Domain,
    /// Its type (GlobalId).
    pub segment_type: String,
    pub start: V3,
    pub end: V3,
    pub size: Size,
    /// The level it is drawn from (GlobalId).
    pub level: String,
    /// Its system classification, when drawn with a system type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taps: Vec<TapPort>,
    /// Pipes: the slope it was drawn with (rise over run).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub slope: f64,
    /// The section's across direction, where it is not the level one square to the run
    /// (vertical runs keep their neighbours').
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub across: Option<V3>,
    /// A curved segment (a bend of a tray or conduit type without fittings): the corner its
    /// arc turns about, tangent to the lines to `start` and `end`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner: Option<V3>,
    /// Sizing leaves it as it is (MEP-CALC-001).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub size_locked: bool,
    /// A level-bound riser: its `end` stays at this level's elevation plus the offset
    /// (MEP-DUCT-011).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub riser: Option<RiserTop>,
    /// Flex duct (MEP-DUCT-005): the spline through its points; `start` and `end` are its first
    /// and last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex: Option<crate::mep_flex::FlexPath>,
}

/// A riser's binding of its end to a level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RiserTop {
    /// The level (GlobalId).
    pub level: String,
    /// Metres above it.
    pub offset: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// A fitting's leg: from the centre along `direction` for `length`, of `size`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Leg {
    pub direction: V3,
    pub length: f64,
    pub size: Size,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub across: Option<V3>,
}

/// The stored recipe of a fitting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FittingRecipe {
    pub kind: String,
    pub domain: Domain,
    pub part: Part,
    pub centre: V3,
    pub legs: Vec<Leg>,
    pub level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    /// A bend's radius (tray and conduit); none: mitred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// A valve's handle direction (square to the pipe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<V3>,
    /// An accessory's kind (`gate_valve`, `check_valve`, `strainer`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessory: Option<String>,
}

pub fn segment_recipe(e: &Element) -> Option<SegmentRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == SEGMENT).then(|| serde_json::from_value(v.clone()).ok())?
}

pub fn fitting_recipe(e: &Element) -> Option<FittingRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == FITTING).then(|| serde_json::from_value(v.clone()).ok())?
}

/// Whether an element is a segment or a fitting.
pub fn is_run_element(e: &Element) -> bool {
    matches!(
        e.param(RECIPE_KEY).and_then(|v| v.get("kind")).and_then(Value::as_str),
        Some(SEGMENT | FITTING)
    )
}

/// How many ports a segment or fitting has (0 for other elements).
pub fn port_count(e: &Element) -> usize {
    if let Some(s) = segment_recipe(e) {
        return 2 + s.taps.len();
    }
    if let Some(g) = crate::mep_gutters::gutter_recipe(e) {
        return g.outlets.len();
    }
    fitting_recipe(e).map_or(0, |f| f.legs.len())
}

fn port_connector(domain: Domain, size: &Size, c: Option<Classification>, name: &str) -> Connector {
    let carrier = matches!(domain, Domain::CableTray | Domain::Conduit);
    Connector {
        name: name.into(),
        classification: if carrier { None } else { Some(c.unwrap_or(Classification::Global)) },
        flow_direction: FlowDirection::Bidirectional,
        ..size.connector(domain)
    }
}

/// A segment's or fitting's ports.
pub fn ports(_: &Store, e: &Element) -> Vec<Port> {
    if let Some(s) = segment_recipe(e) {
        let Some(axis) = unit(sub(s.end, s.start)) else { return Vec::new() };
        // A curved segment leaves along the lines to its corner.
        let (out_start, out_end) = match (s.corner, &s.flex) {
            (_, Some(f)) => (mul(f.start_tangent, -1.0), f.end_tangent),
            (Some(c), None) => (unit(sub(s.start, c)).unwrap_or(mul(axis, -1.0)), unit(sub(s.end, c)).unwrap_or(axis)),
            (None, None) => (mul(axis, -1.0), axis),
        };
        let mut out = vec![
            Port { element: e.id(), index: 0, connector: port_connector(s.domain, &s.size, s.classification, "Start"), position: s.start, direction: out_start },
            Port { element: e.id(), index: 1, connector: port_connector(s.domain, &s.size, s.classification, "End"), position: s.end, direction: out_end },
        ];
        for (k, t) in s.taps.iter().enumerate() {
            out.push(Port {
                element: e.id(),
                index: 2 + k,
                connector: port_connector(s.domain, &t.size, s.classification, "Tap"),
                position: t.at,
                direction: t.direction,
            });
        }
        return out;
    }
    let Some(f) = fitting_recipe(e) else { return Vec::new() };
    f.legs
        .iter()
        .enumerate()
        .map(|(k, l)| Port {
            element: e.id(),
            index: k,
            connector: port_connector(f.domain, &l.size, f.classification, &format!("Leg {}", k + 1)),
            position: add(f.centre, mul(l.direction, l.length)),
            direction: l.direction,
        })
        .collect()
}

// ---- Geometry ----

/// A solid as a polyhedron: points and outward polygons.
#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    pub points: Vec<V3>,
    pub faces: Vec<Vec<usize>>,
}

impl Solid {
    /// Triangles, fanned from each face's first point.
    pub fn mesh(&self) -> (Vec<[f64; 3]>, Vec<u32>) {
        let mut idx = Vec::new();
        for f in &self.faces {
            for k in 1..f.len().saturating_sub(1) {
                idx.extend([f[0] as u32, f[k] as u32, f[k + 1] as u32]);
            }
        }
        (self.points.clone(), idx)
    }
}

/// The across (`w`) and up (`h`) directions of a section square to `u`: across is level for
/// level and sloped runs, X for vertical ones.
pub(crate) fn frame(u: V3) -> (V3, V3) {
    let w = if u[2].abs() < 0.999 { unit(cross([0.0, 0.0, 1.0], u)).expect("not vertical") } else { [1.0, 0.0, 0.0] };
    (w, cross(u, w))
}

/// The section's across and up directions for a run along `u`, `across` given or the default.
fn axes(u: V3, across: Option<V3>) -> (V3, V3) {
    match across.and_then(|a| unit(sub(a, mul(u, dot(a, u))))) {
        Some(w) => (w, cross(u, w)),
        None => frame(u),
    }
}

/// The section outline of a size at `c`, square to `u`.
pub(crate) fn section(c: V3, u: V3, s: &Size, across: Option<V3>) -> Vec<V3> {
    let (w, h) = axes(u, across);
    match s.shape {
        Shape::Round => (0..16)
            .map(|k| {
                let t = std::f64::consts::TAU * k as f64 / 16.0;
                add(c, add(mul(w, t.cos() * s.diameter / 2.0), mul(h, t.sin() * s.diameter / 2.0)))
            })
            .collect(),
        Shape::Rectangular | Shape::Oval => {
            let (a, b) = (s.width / 2.0, s.height / 2.0);
            [(-a, -b), (a, -b), (a, b), (-a, b)].iter().map(|(x, y)| add(c, add(mul(w, *x), mul(h, *y)))).collect()
        }
    }
}

/// A prism (or a frustum, with two sizes of one shape) from `a` to `b`.
fn loft(a: V3, b: V3, sa: &Size, sb: &Size, across: Option<V3>) -> Option<Solid> {
    let u = unit(sub(b, a))?;
    let p = section(a, u, sa, across);
    let mut q = section(b, u, sb, across);
    if q.len() != p.len() {
        q = section(b, u, &Size { shape: sa.shape, ..*sb }, across);
    }
    let n = p.len();
    let mut points = p;
    points.extend(q);
    let mut faces = vec![(0..n).rev().collect::<Vec<_>>(), (n..2 * n).collect()];
    for k in 0..n {
        let j = (k + 1) % n;
        faces.push(vec![k, j, n + j, n + k]);
    }
    Some(Solid { points, faces })
}

/// The arc from `a` to `b` turning about `corner` (tangent to the lines from the corner):
/// its points and tangents, `n` steps; none when the three are in line.
fn arc(a: V3, corner: V3, b: V3, n: usize) -> Option<Vec<(V3, V3)>> {
    let t0 = unit(sub(corner, a))?;
    let t1 = unit(sub(b, corner))?;
    let theta = dot(t0, t1).clamp(-1.0, 1.0).acos();
    if theta < 1e-6 {
        return None;
    }
    let axis = unit(cross(t0, t1))?;
    let r = len(sub(corner, a)) / (theta / 2.0).tan();
    let inward = cross(axis, t0);
    let centre = add(a, mul(inward, r));
    let v0 = sub(a, centre);
    Some(
        (0..=n)
            .map(|k| {
                let f = theta * k as f64 / n as f64;
                let (c, sn) = (f.cos(), f.sin());
                let p = add(centre, add(mul(v0, c), mul(cross(axis, v0), sn)));
                let t = add(mul(t0, c), mul(cross(axis, t0), sn));
                (p, t)
            })
            .collect(),
    )
}

/// The radius of the arc from `a` about `corner` to `b`.
pub fn arc_radius(a: V3, corner: V3, b: V3) -> Option<f64> {
    let (t0, t1) = (unit(sub(corner, a))?, unit(sub(b, corner))?);
    let theta = dot(t0, t1).clamp(-1.0, 1.0).acos();
    (theta > 1e-6).then(|| len(sub(corner, a)) / (theta / 2.0).tan())
}

/// A section swept along the arc from `a` about `corner` to `b`: one solid of rings.
fn sweep(a: V3, corner: V3, b: V3, s: &Size, across: Option<V3>) -> Option<Solid> {
    let t0 = unit(sub(corner, a))?;
    let t1 = unit(sub(b, corner))?;
    let theta = dot(t0, t1).clamp(-1.0, 1.0).acos();
    let steps = ((theta / (std::f64::consts::PI / 16.0)).ceil() as usize).max(2);
    let path = arc(a, corner, b, steps)?;
    // A level bend keeps its section level; one in a vertical plane keeps its across
    // direction square to that plane.
    let normal = unit(cross(t0, t1))?;
    let across = if normal[2].abs() > 0.7 { None } else { across.or(Some(normal)) };
    let rings: Vec<Vec<V3>> = path.iter().map(|(p, t)| section(*p, *t, s, across)).collect();
    let m = rings[0].len();
    let mut points = Vec::new();
    for r in &rings {
        points.extend(r.iter().copied());
    }
    let last = (rings.len() - 1) * m;
    let mut faces = vec![(0..m).rev().collect::<Vec<_>>(), (last..last + m).collect()];
    for k in 0..rings.len() - 1 {
        for j in 0..m {
            let (a, b) = (k * m + j, k * m + (j + 1) % m);
            faces.push(vec![a, b, b + m, a + m]);
        }
    }
    Some(Solid { points, faces })
}

/// A segment's body (a curved one swept along its arc).
pub fn segment_solid(s: &SegmentRecipe) -> Vec<Solid> {
    if let Some(f) = &s.flex {
        return crate::mep_flex::solid(f, &s.size).into_iter().collect();
    }
    if let Some(c) = s.corner {
        return sweep(s.start, c, s.end, &s.size, s.across).into_iter().collect();
    }
    loft(s.start, s.end, &s.size, &s.size, s.across).into_iter().collect()
}

/// A segment's length along its axis, arc or spline (metres).
pub fn segment_length(s: &SegmentRecipe) -> f64 {
    if let Some(f) = &s.flex {
        return f.length();
    }
    match s.corner {
        Some(c) => len(sub(c, s.start)) + len(sub(s.end, c)),
        None => len(sub(s.end, s.start)),
    }
}

/// A junction box's side: room round its largest connection.
fn box_side(size: &Size) -> f64 {
    (size.nominal() * 2.5 + 0.04).max(0.1)
}

/// A fitting's body: each leg from just behind the centre to its end; a transition as one loft;
/// a cap a short plug.
pub fn fitting_solids(f: &FittingRecipe) -> Vec<Solid> {
    match f.part {
        Part::Elbow if f.radius.is_some() && f.legs.len() == 2 => {
            let a = add(f.centre, mul(f.legs[0].direction, f.legs[0].length));
            let b = add(f.centre, mul(f.legs[1].direction, f.legs[1].length));
            sweep(a, f.centre, b, &f.legs[0].size, f.legs[0].across).into_iter().collect()
        }
        Part::Box => {
            let side = f.legs.iter().map(|l| box_side(&l.size)).fold(0.1, f64::max);
            let h = side / 2.0;
            let c = f.centre;
            let points: Vec<V3> = [(-h, -h, -h), (h, -h, -h), (h, h, -h), (-h, h, -h), (-h, -h, h), (h, -h, h), (h, h, h), (-h, h, h)]
                .iter()
                .map(|(x, y, z)| [c[0] + x, c[1] + y, c[2] + z])
                .collect();
            vec![Solid {
                points,
                faces: vec![vec![3, 2, 1, 0], vec![4, 5, 6, 7], vec![0, 1, 5, 4], vec![1, 2, 6, 5], vec![2, 3, 7, 6], vec![3, 0, 4, 7]],
            }]
        }
        Part::Coupling if f.legs.len() == 2 => {
            let grow = |s: Size| Size { diameter: s.diameter * 1.12, width: s.width * 1.06, height: s.height * 1.06, ..s };
            let a = add(f.centre, mul(f.legs[0].direction, f.legs[0].length));
            let b = add(f.centre, mul(f.legs[1].direction, f.legs[1].length));
            loft(a, b, &grow(f.legs[0].size), &grow(f.legs[0].size), f.legs[0].across).into_iter().collect()
        }
        Part::Transition if f.legs.len() == 2 => {
            let a = add(f.centre, mul(f.legs[0].direction, f.legs[0].length));
            let b = add(f.centre, mul(f.legs[1].direction, f.legs[1].length));
            loft(a, b, &f.legs[0].size, &f.legs[1].size, f.legs[1].across).into_iter().collect()
        }
        Part::Flange if f.legs.len() == 2 => {
            let grow = |s: Size| Size { diameter: s.diameter * 2.2, width: s.width * 1.6, height: s.height * 1.6, ..s };
            let a = add(f.centre, mul(f.legs[0].direction, f.legs[0].length));
            let b = add(f.centre, mul(f.legs[1].direction, f.legs[1].length));
            loft(a, b, &grow(f.legs[0].size), &grow(f.legs[0].size), f.legs[0].across).into_iter().collect()
        }
        Part::Valve | Part::Meter | Part::Strainer if f.legs.len() == 2 => accessory_solids(f),
        Part::Damper | Part::Filter | Part::Coil | Part::Silencer if f.legs.len() == 2 => duct_accessory_solids(f),
        Part::Cap if !f.legs.is_empty() => {
            let l = f.legs[0];
            let grow = |s: Size| Size { diameter: s.diameter * 1.08, width: s.width * 1.08, height: s.height * 1.08, ..s };
            let a = add(f.centre, mul(l.direction, -0.02));
            let b = add(f.centre, mul(l.direction, l.length));
            loft(a, b, &grow(l.size), &grow(l.size), l.across).into_iter().collect()
        }
        _ => f
            .legs
            .iter()
            .filter_map(|l| {
                let back = l.size.nominal() / 2.0;
                let a = add(f.centre, mul(l.direction, -back.min(l.length * 0.9)));
                // A tap's leg starts at the host's surface, not its axis.
                let a = if f.part == Part::Tap { f.centre } else { a };
                loft(a, add(f.centre, mul(l.direction, l.length)), &l.size, &l.size, l.across)
            })
            .collect(),
    }
}

/// An inline accessory's body: a valve's two cones meeting at its seat with a stem and
/// handwheel, a meter's box, a strainer's body with its basket leg (Reviera's own simple shapes).
fn accessory_solids(f: &FittingRecipe) -> Vec<Solid> {
    let (l0, l1) = (f.legs[0], f.legs[1]);
    let d = l0.size.nominal();
    let a = add(f.centre, mul(l0.direction, l0.length));
    let b = add(f.centre, mul(l1.direction, l1.length));
    let round = |dia: f64| Size { shape: Shape::Round, diameter: dia, width: dia, height: dia };
    let axis = l1.direction;
    // The handle's direction: square to the pipe, up unless turned.
    let up = f.handle.and_then(unit).unwrap_or_else(|| frame(axis).1);
    let mut out = Vec::new();
    match f.part {
        Part::Valve => {
            out.extend(loft(a, f.centre, &round(d * 1.3), &round(d * 0.7), None));
            out.extend(loft(f.centre, b, &round(d * 0.7), &round(d * 1.3), None));
            let top = add(f.centre, mul(up, d * 1.4));
            out.extend(loft(f.centre, top, &round(d * 0.25), &round(d * 0.25), None));
            out.extend(loft(top, add(top, mul(up, 0.015)), &round(d * 1.2), &round(d * 1.2), None));
        }
        Part::Meter => {
            let r = Size { shape: Shape::Rectangular, diameter: 0.0, width: d * 1.8, height: d * 1.8 };
            out.extend(loft(add(a, mul(axis, l0.length * 0.2)), add(b, mul(axis, -l1.length * 0.2)), &r, &r, Some(frame(axis).0)));
            out.extend(loft(a, b, &round(d), &round(d), None));
        }
        _ => {
            out.extend(loft(a, b, &round(d * 1.3), &round(d * 1.3), None));
            let basket = unit(add(axis, mul(up, -1.0))).unwrap_or(up);
            out.extend(loft(f.centre, add(f.centre, mul(basket, d * 1.6)), &round(d * 0.9), &round(d * 0.9), None));
        }
    }
    out
}

/// A duct accessory's body: a casing round the duct from end to end, larger for a filter, coil
/// or attenuator, and a damper's frame collar at its middle (Reviera's own simple shapes).
fn duct_accessory_solids(f: &FittingRecipe) -> Vec<Solid> {
    let (l0, l1) = (f.legs[0], f.legs[1]);
    let a = add(f.centre, mul(l0.direction, l0.length));
    let b = add(f.centre, mul(l1.direction, l1.length));
    let grow = |s: Size, by: f64| Size { diameter: s.diameter + by, width: s.width + by, height: s.height + by, ..s };
    let casing = match f.part {
        Part::Damper => 0.01,
        Part::Silencer => (l0.size.nominal() * 0.5).max(0.1),
        _ => 0.1,
    };
    let mut out: Vec<Solid> = loft(a, b, &grow(l0.size, casing), &grow(l0.size, casing), l0.across).into_iter().collect();
    if f.part == Part::Damper {
        let half = 0.02;
        out.extend(loft(add(f.centre, mul(l1.direction, -half)), add(f.centre, mul(l1.direction, half)), &grow(l0.size, 0.08), &grow(l0.size, 0.08), l0.across));
    }
    out
}

/// A hollow prism from `a` to `b`: `outer` round `inner`, of one shape.
fn tube(a: V3, b: V3, outer: &Size, inner: &Size, across: Option<V3>) -> Option<Solid> {
    let u = unit(sub(b, a))?;
    let mut points = section(a, u, outer, across);
    let n = points.len();
    points.extend(section(b, u, outer, across));
    points.extend(section(a, u, inner, across));
    points.extend(section(b, u, inner, across));
    let mut faces = Vec::new();
    for k in 0..n {
        let j = (k + 1) % n;
        faces.push(vec![k, j, n + j, n + k]);
        faces.push(vec![2 * n + k, 3 * n + k, 3 * n + j, 2 * n + j]);
        faces.push(vec![j, k, 2 * n + k, 2 * n + j]);
        faces.push(vec![n + k, n + j, 3 * n + j, 3 * n + k]);
    }
    Some(Solid { points, faces })
}

/// A covering's body round a run element (MEP-DUCT-004): a tube from the element's size grown
/// by `inner` to grown by `outer` (both across the whole section, so twice the thickness),
/// along a segment, along each of a fitting's legs, or along an accessory end to end.
pub(crate) fn covering_solids(e: &Element, inner: f64, outer: f64) -> Vec<Solid> {
    let grow = |s: Size, by: f64| Size { diameter: s.diameter + by, width: s.width + by, height: s.height + by, ..s };
    if let Some(s) = segment_recipe(e) {
        if let Some(f) = &s.flex {
            return crate::mep_flex::tube(f, &grow(s.size, outer), &grow(s.size, inner)).into_iter().collect();
        }
        if s.corner.is_some() {
            return Vec::new();
        }
        return tube(s.start, s.end, &grow(s.size, outer), &grow(s.size, inner), s.across).into_iter().collect();
    }
    let Some(f) = fitting_recipe(e) else { return Vec::new() };
    if f.part.is_accessory() && f.legs.len() == 2 {
        let (l0, l1) = (f.legs[0], f.legs[1]);
        let a = add(f.centre, mul(l0.direction, l0.length));
        let b = add(f.centre, mul(l1.direction, l1.length));
        return tube(a, b, &grow(l0.size, outer), &grow(l0.size, inner), l0.across).into_iter().collect();
    }
    f.legs
        .iter()
        .filter_map(|l| {
            let back = if f.part == Part::Tap { 0.0 } else { (l.size.nominal() / 2.0).min(l.length * 0.9) };
            let a = add(f.centre, mul(l.direction, -back));
            tube(a, add(f.centre, mul(l.direction, l.length)), &grow(l.size, outer), &grow(l.size, inner), l.across)
        })
        .collect()
}

/// Every segment and fitting (and gutter, and insulation and lining) with its solids.
pub fn all(store: &Store) -> Vec<(ElementId, Domain, bool, Vec<Solid>)> {
    store
        .iter()
        .filter_map(|e| {
            if let Some(s) = segment_recipe(e) {
                return Some((e.id(), s.domain, true, segment_solid(&s)));
            }
            if let Some(g) = crate::mep_gutters::gutter_recipe(e) {
                return Some((e.id(), Domain::Pipe, true, crate::mep_gutters::geometry(store, &g).ok()?.solids));
            }
            if let Some((host, solids)) = crate::mep_coverings::solids(store, e) {
                return Some((e.id(), host, false, solids));
            }
            let f = fitting_recipe(e)?;
            Some((e.id(), f.domain, false, fitting_solids(&f)))
        })
        .collect()
}

// ---- Drawing runs ----

pub(crate) fn leg_length(size: &Size) -> f64 {
    size.nominal() * 0.6 + 0.05
}

/// An elbow's leg length: by the section's extent in the plane of the bend (a rectangular
/// duct turning up uses its height).
fn elbow_leg(size: &Size, incoming: V3, outgoing: V3, across: Option<V3>) -> f64 {
    let extent = match size.shape {
        Shape::Round => size.diameter,
        _ => {
            let (w, h) = axes(incoming, across);
            match unit(sub(outgoing, mul(incoming, dot(outgoing, incoming)))) {
                Some(d) => dot(d, w).abs() * size.width + dot(d, h).abs() * size.height,
                None => size.nominal(),
            }
        }
    };
    extent * 0.6 + 0.05
}

pub(crate) fn level_of(store: &Store, r: &ElementRef) -> Result<(String, f64)> {
    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such level"))?;
    let e = store.get(id).expect("resolved");
    if e.class != "IfcBuildingStorey" {
        return Err(bad("level.not_a_level", format!("element {id} is not a level")));
    }
    let g = e.guid().to_string();
    let z = crate::levels::elevation_of(store, &g)?;
    Ok((g, z))
}

/// What a run is drawn with.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The duct or pipe type; the built-in type of the shape when omitted.
    pub type_id: Option<ElementRef>,
    /// The run's points in plan with their height above the level (`[x, y, z]`, metres), or in
    /// plan (`[x, y]`) at `middle_elevation`.
    pub points: Vec<Vec<f64>>,
    pub level: ElementRef,
    /// Height of the centreline above the level for points given in plan (metres).
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub middle_elevation: Option<f64>,
    #[serde(default)]
    pub shape: Option<Shape>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub diameter: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
    /// The system type the run carries (`supply_air`, `domestic_cold_water`, …).
    pub system_type_id: Option<String>,
    /// Pipes: the fall along level legs (rise over run; 0.01 is 1 % falling from the first
    /// point).
    #[serde(default)]
    pub slope: f64,
    /// Tray and conduit: the bends' radius (metres); the type's default when omitted.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub bend_radius: Option<f64>,
}

/// What a run made.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct RunReport {
    pub segments: Vec<ElementId>,
    pub fittings: Vec<ElementId>,
    /// Connections made to elements that were there before.
    pub connected_to: Vec<ElementId>,
    /// The system it joined.
    pub system: Option<ElementId>,
}

struct Ctx {
    domain: Domain,
    /// The segments' predefined type.
    predefined: &'static str,
    type_guid: String,
    routing: Routing,
    level: String,
    classification: Option<Classification>,
}

#[allow(clippy::too_many_arguments)]
fn make_segment(txn: &mut Transaction<'_>, cx: &Ctx, a: V3, b: V3, size: Size, slope: f64, across: Option<V3>) -> Result<ElementId> {
    make_segment_c(txn, cx, a, b, size, slope, across, None)
}

#[allow(clippy::too_many_arguments)]
fn make_segment_c(txn: &mut Transaction<'_>, cx: &Ctx, a: V3, b: V3, size: Size, slope: f64, across: Option<V3>, corner: Option<V3>) -> Result<ElementId> {
    let r = SegmentRecipe {
        kind: SEGMENT.into(),
        domain: cx.domain,
        segment_type: cx.type_guid.clone(),
        start: a,
        end: b,
        size,
        level: cx.level.clone(),
        classification: cx.classification,
        taps: Vec::new(),
        slope,
        across,
        corner,
        size_locked: false,
        riser: None,
        flex: None,
    };
    let mut e = NewElement::new(segment_class(cx.domain))
        .param("@PredefinedType", cx.predefined)
        .param("@type", cx.type_guid.clone())
        .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises"));
    if slope != 0.0 {
        e = e.param(
            "Pset_PipeSegmentOccurrence.Gradient",
            serde_json::json!({ "type": "IfcPositiveRatioMeasure", "value": slope.abs() }),
        );
    }
    txn.create(e)
}

fn make_fitting(txn: &mut Transaction<'_>, cx: &Ctx, part: Part, centre: V3, legs: Vec<Leg>) -> Result<ElementId> {
    make_fitting_r(txn, cx, part, centre, legs, None)
}

fn make_fitting_r(txn: &mut Transaction<'_>, cx: &Ctx, part: Part, centre: V3, legs: Vec<Leg>, radius: Option<f64>) -> Result<ElementId> {
    let r = FittingRecipe {
        kind: FITTING.into(),
        domain: cx.domain,
        part,
        centre,
        legs,
        level: cx.level.clone(),
        classification: cx.classification,
        radius,
        handle: None,
        accessory: None,
    };
    let (pre, object) = part.ifc_in(cx.domain);
    let mut e = NewElement::new(fitting_class(cx.domain, part))
        .name(part.label())
        .param("@PredefinedType", pre)
        .param(RECIPE_KEY, serde_json::to_value(&r).expect("serialises"));
    if let Some(o) = object {
        e = e.param("@ObjectType", o);
    }
    txn.create(e)
}

/// An existing segment the point lies on, away from its ends: the segment, its recipe and the
/// point on its axis.
fn host_at(store: &Store, domain: Domain, p: V3) -> Option<(ElementId, SegmentRecipe, V3)> {
    store.iter().find_map(|e| {
        let s = segment_recipe(e)?;
        if s.domain != domain || s.corner.is_some() || s.flex.is_some() {
            return None;
        }
        let d = sub(s.end, s.start);
        let l = len(d);
        let u = unit(d)?;
        let t = dot(sub(p, s.start), u);
        let q = add(s.start, mul(u, t));
        let off = len(sub(p, q));
        let margin = leg_length(&s.size);
        (t > margin && t < l - margin && off <= s.size.nominal() / 2.0 + 1e-6).then(|| (e.id(), s, q))
    })
}

fn warn(txn: &mut Transaction<'_>, id: &str, msg: String, ids: Vec<ElementId>) {
    txn.warn(Warning { id: id.into(), message: msg, element_ids: ids });
}

fn draw(txn: &mut Transaction<'_>, domain: Domain, p: CreateParams) -> Result<RunReport> {
    draw_with(txn, domain, p, None)
}

/// Draws a run; `radii` gives each corner's bend radius (parallel conduits).
fn draw_with(txn: &mut Transaction<'_>, domain: Domain, p: CreateParams, radii: Option<Vec<f64>>) -> Result<RunReport> {
    let store = txn.store().clone();
    let (level, z0) = level_of(&store, &p.level)?;
    let given_type = p.type_id.as_ref().map(|t| resolve_type(&store, t)).transpose()?;
    if let Some((_, t)) = &given_type
        && t.domain != domain
    {
        return Err(bad("mep_segment_type.domain", format!("that is a {} type", t.domain.label().to_lowercase())));
    }
    let shape = p.shape.or(given_type.as_ref().map(|t| t.1.shape)).unwrap_or(match domain {
        Domain::Pipe | Domain::Conduit => Shape::Round,
        _ => Shape::Rectangular,
    });
    let size = Size {
        shape,
        diameter: p.diameter.unwrap_or(0.0),
        width: p.width.unwrap_or(0.0),
        height: p.height.unwrap_or(0.0),
    };
    size.check(domain)?;
    let type_id = match given_type {
        Some((id, _)) => id,
        None => default_type(txn, domain, shape)?,
    };
    let store = txn.store().clone();
    let t = segment_type(store.get(type_id).expect("made")).expect("a type");
    t.check_size(&store, &size)?;
    if t.flexible {
        return Err(bad("mep_segment_type.flexible", "a flexible type is drawn with Flex Duct (flex_duct.create)"));
    }
    if p.system_type_id.is_some() && t.radius_bends() {
        return Err(bad("mep_run.classification", "cable tray and conduit carry no system type"));
    }
    let classification = p
        .system_type_id
        .as_deref()
        .map(|id| {
            crate::mep_systems::system_type(&store, id)
                .ok_or_else(|| bad("system_type.not_found", format!("no system type {id:?}")))
                .and_then(|st| {
                    if st.classification.fits(domain) {
                        Ok(st.classification)
                    } else {
                        Err(bad("mep_run.classification", format!("a {} cannot carry {}", domain.label().to_lowercase(), st.classification.label())))
                    }
                })
        })
        .transpose()?;
    let cx = Ctx {
        domain,
        predefined: t.predefined(),
        type_guid: store.get(type_id).expect("made").guid().to_string(),
        routing: t.routing.clone(),
        level,
        classification,
    };
    // The points in project coordinates, collinear ones dropped.
    let mid = p.middle_elevation.unwrap_or(match domain {
        Domain::Pipe => 2.4,
        Domain::CableTray => 3.0,
        Domain::Conduit => 2.8,
        _ => 2.7,
    });
    let mut pts: Vec<V3> = Vec::new();
    for q in &p.points {
        let v = match q.as_slice() {
            [x, y] => [*x, *y, z0 + mid],
            [x, y, z] => [*x, *y, z0 + z],
            _ => return Err(bad("mep_run.point", "each point is [x, y] or [x, y, z]")),
        };
        if pts.last().is_some_and(|l| len(sub(v, *l)) < 1e-6) {
            continue;
        }
        pts.push(v);
    }
    if pts.len() < 2 {
        return Err(bad("mep_run.points", "a run needs two different points"));
    }
    // Pipes fall along level legs.
    if p.slope != 0.0 {
        if domain != Domain::Pipe {
            return Err(bad("mep_run.slope", "only pipes slope"));
        }
        let mut drop = 0.0;
        let original = pts.clone();
        for k in 1..pts.len() {
            let d = sub(original[k], original[k - 1]);
            if d[2].abs() < 1e-9 {
                drop += p.slope * (d[0] * d[0] + d[1] * d[1]).sqrt();
            }
            pts[k][2] = original[k][2] - drop;
        }
    }
    let mut i = 1;
    while i + 1 < pts.len() {
        let (a, b) = (unit(sub(pts[i], pts[i - 1])).expect("distinct"), unit(sub(pts[i + 1], pts[i])).expect("distinct"));
        if dot(a, b) > (0.5f64).to_radians().cos() {
            pts.remove(i);
        } else {
            i += 1;
        }
    }
    let mut report = RunReport::default();
    // A run ending on an existing segment's side: a junction there.
    let ends_on = host_at(&store, domain, *pts.last().expect("two"));
    let starts_on = host_at(&store, domain, pts[0]);
    // Where each segment starts and ends, after the fittings take their legs.
    let n = pts.len();
    let mut starts = pts[..n - 1].to_vec();
    let mut ends = pts[1..].to_vec();
    let leg = leg_length(&size);
    // Each leg's across direction: its own level one, or for a vertical leg its nearest level
    // neighbour's, so a rectangular section does not turn going up or down.
    let dirs: Vec<V3> = (0..n - 1).map(|k| unit(sub(pts[k + 1], pts[k])).expect("distinct")).collect();
    let vertical = |d: V3| d[2].abs() >= 0.999;
    let across: Vec<Option<V3>> = (0..n - 1)
        .map(|k| {
            if !vertical(dirs[k]) {
                return None;
            }
            let near = (0..n - 1)
                .filter(|j| !vertical(dirs[*j]))
                .min_by_key(|j| (*j as i64 - k as i64).abs())?;
            Some(frame(dirs[near]).0)
        })
        .collect();
    let across_of = |k: usize| across[k].unwrap_or_else(|| frame(dirs[k]).0);
    let mut corners_made: Vec<(V3, V3, V3, Option<V3>)> = Vec::new();
    for k in 1..n - 1 {
        let (a, b) = (mul(dirs[k - 1], -1.0), dirs[k]);
        if t.radius_bends() {
            // A radius bend: a fitting, or for a type without fittings a curved segment.
            let r = radii.as_ref().and_then(|r| r.get(k - 1).copied()).or(p.bend_radius).unwrap_or_else(|| t.bend_radius(&store, &size));
            if r.is_nan() || r <= 0.0 || (domain == Domain::CableTray && r < size.width / 2.0 - 1e-9) {
                return Err(bad("mep_run.bend_radius", format!("a bend radius of {:.0} mm is below half the tray's width", r * 1000.0)));
            }
            let theta = dot(dirs[k - 1], dirs[k]).clamp(-1.0, 1.0).acos();
            let leg = r * (theta / 2.0).tan();
            if t.with_fittings {
                let Some(part) = cx.routing.choose(Role::Elbow, size.nominal()) else {
                    warn(txn, "mep.routing.no_part", format!("no bend in the routing preferences fits {}; the corner is left open", size.text()), Vec::new());
                    continue;
                };
                let id = make_fitting_r(
                    txn,
                    &cx,
                    part,
                    pts[k],
                    vec![
                        Leg { direction: a, length: leg, size, across: Some(across_of(k - 1)) },
                        Leg { direction: b, length: leg, size, across: Some(across_of(k)) },
                    ],
                    Some(r),
                )?;
                report.fittings.push(id);
            } else {
                corners_made.push((add(pts[k], mul(a, leg)), pts[k], add(pts[k], mul(b, leg)), across[k - 1].or(across[k])));
            }
            ends[k - 1] = add(pts[k], mul(a, leg));
            starts[k] = add(pts[k], mul(b, leg));
            continue;
        }
        match cx.routing.choose(Role::Elbow, size.nominal()) {
            Some(part) => {
                let leg = elbow_leg(&size, dirs[k - 1], dirs[k], Some(across_of(k - 1)));
                let id = make_fitting(
                    txn,
                    &cx,
                    part,
                    pts[k],
                    vec![
                        Leg { direction: a, length: leg, size, across: Some(across_of(k - 1)) },
                        Leg { direction: b, length: leg, size, across: Some(across_of(k)) },
                    ],
                )?;
                report.fittings.push(id);
                ends[k - 1] = add(pts[k], mul(a, leg));
                starts[k] = add(pts[k], mul(b, leg));
            }
            None => warn(
                txn,
                "mep.routing.no_part",
                format!("no elbow in the routing preferences fits {}; the corner is left open", size.text()),
                Vec::new(),
            ),
        }
    }
    for (end_at, at_end) in [(ends_on, true), (starts_on, false)] {
        let Some((host, hs, q)) = end_at else { continue };
        let k = if at_end { n - 1 } else { 0 };
        let other = if at_end { pts[n - 2] } else { pts[1] };
        let branch = unit(sub(other, q)).expect("distinct");
        let axis = unit(sub(hs.end, hs.start)).expect("a segment");
        if dot(branch, axis).abs() > (1.0f64).to_radians().sin() {
            return Err(bad("mep_run.branch", "a branch must meet the duct or pipe square to it"));
        }
        let Some(part) = cx.routing.choose(Role::Junction, size.nominal()) else {
            warn(
                txn,
                "mep.routing.no_part",
                format!("no junction in the routing preferences fits {}; the branch is left open", size.text()),
                vec![host],
            );
            continue;
        };
        let _ = k;
        let surface = hs.size.nominal() / 2.0;
        match part {
            Part::Tap => {
                // The host stays whole and gets a tap port; the tap runs out from its side.
                let base = add(q, mul(branch, surface));
                let tap_len = leg.max(0.05);
                let id = make_fitting(
                    txn,
                    &cx,
                    Part::Tap,
                    base,
                    vec![Leg { direction: mul(branch, -1.0), length: 0.0, size, across: None }, Leg { direction: branch, length: tap_len, size, across: None }],
                )?;
                report.fittings.push(id);
                let mut h = hs.clone();
                h.taps.push(TapPort { at: base, direction: branch, size });
                let v = serde_json::to_value(&h).expect("serialises");
                txn.modify(host, |e| {
                    e.params.insert(RECIPE_KEY.into(), v);
                })?;
                let tip = add(base, mul(branch, tap_len));
                if at_end { ends[n - 2] = tip } else { starts[0] = tip }
            }
            _ => {
                // The host is cut in two either side of a tee.
                let hl = leg_length(&hs.size);
                let tee = make_fitting(
                    txn,
                    &cx,
                    Part::Tee,
                    q,
                    vec![
                        Leg { direction: mul(axis, -1.0), length: hl, size: hs.size, across: hs.across },
                        Leg { direction: axis, length: hl, size: hs.size, across: hs.across },
                        Leg { direction: branch, length: surface + leg, size, across: None },
                    ],
                )?;
                report.fittings.push(tee);
                let mut first = hs.clone();
                first.end = add(q, mul(axis, -hl));
                let mut second = hs.clone();
                second.start = add(q, mul(axis, hl));
                // Taps stay with the half they are on.
                let along = |p: V3| dot(sub(p, hs.start), axis);
                let cut = along(q);
                first.taps.retain(|t| along(t.at) < cut);
                second.taps.retain(|t| along(t.at) > cut);
                let (v1, v2) = (serde_json::to_value(&first).expect("serialises"), serde_json::to_value(&second).expect("serialises"));
                txn.modify(host, |e| {
                    e.params.insert(RECIPE_KEY.into(), v1);
                })?;
                let mut el = store.get(host).expect("host").params.clone();
                el.insert(RECIPE_KEY.into(), v2);
                let host_class = store.get(host).expect("host").class.clone();
                let new_half = txn.create(NewElement { class: host_class, name: None, params: el, guid: None })?;
                report.segments.push(new_half);
                // The host's end connection moves to the new half.
                retarget_end(txn, host, new_half)?;
                let tip = add(q, mul(branch, surface + leg));
                if at_end { ends[n - 2] = tip } else { starts[0] = tip }
            }
        }
    }
    for k in 0..n - 1 {
        if len(sub(ends[k], starts[k])) < 0.01 || dot(sub(ends[k], starts[k]), sub(pts[k + 1], pts[k])) <= 0.0 {
            return Err(bad("mep_run.too_short", "a leg of the run is too short for its fittings"));
        }
        let slope = if domain == Domain::Pipe { p.slope } else { 0.0 };
        report.segments.push(make_segment(txn, &cx, starts[k], ends[k], size, slope, across[k])?);
    }
    for (a, c, b, across) in corners_made {
        report.segments.push(make_segment_c(txn, &cx, a, b, size, 0.0, across, Some(c))?);
    }
    // Connect everything made, then to what it meets.
    let made: Vec<ElementId> = report.segments.iter().chain(&report.fittings).copied().collect();
    let before: Vec<ElementId> = store.iter().map(|e| e.id()).collect();
    for c in mep::connect_coincident(txn, &made)? {
        for e in [c.a.0, c.b.0] {
            if before.contains(&e) && !made.contains(&e) && !report.connected_to.contains(&e) {
                report.connected_to.push(e);
            }
        }
    }
    report.system = join_system(txn, &made, classification)?;
    Ok(report)
}

/// A tap (takeoff) on a straight segment's side at `q` on its axis, leaving along `branch`
/// (square to the axis) with `size`: made in the host's level and the given classification; the
/// host gets a tap port. Returns the tap and the point its outer leg ends at.
pub(crate) fn make_tap(txn: &mut Transaction<'_>, host: ElementId, q: V3, branch: V3, size: Size, classification: Option<Classification>) -> Result<(ElementId, V3)> {
    let hs = txn.store().get(host).and_then(segment_recipe).ok_or_else(|| bad("mep_run.not_a_segment", format!("element {host} is not a segment")))?;
    let cx = Ctx {
        domain: hs.domain,
        predefined: "",
        type_guid: hs.segment_type.clone(),
        routing: Routing::default(),
        level: hs.level.clone(),
        classification,
    };
    let base = add(q, mul(branch, hs.size.nominal() / 2.0));
    let tap_len = leg_length(&size).max(0.05);
    let id = make_fitting(
        txn,
        &cx,
        Part::Tap,
        base,
        vec![Leg { direction: mul(branch, -1.0), length: 0.0, size, across: None }, Leg { direction: branch, length: tap_len, size, across: None }],
    )?;
    let mut h = hs.clone();
    h.taps.push(TapPort { at: base, direction: branch, size });
    let v = serde_json::to_value(&h).expect("serialises");
    txn.modify(host, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    Ok((id, add(base, mul(branch, tap_len))))
}

/// The type of a segment (its stored GlobalId).
pub fn type_of(store: &Store, s: &SegmentRecipe) -> Option<SegmentType> {
    type_of_segment(store, s).map(|t| t.1)
}

/// Moves the connection on a split segment's end port (index 1) to the new second half.
fn retarget_end(txn: &mut Transaction<'_>, from: ElementId, to: ElementId) -> Result<()> {
    let store = txn.store().clone();
    let (a, b) = (mep::stored(&store, from, 1), mep::stored(&store, to, 1));
    let links: Vec<mep::Link> = mep::stored_links(&store)
        .into_iter()
        .map(|mut l| {
            if l.a == a {
                l.a = b.clone();
            }
            if l.b == a {
                l.b = b.clone();
            }
            l
        })
        .collect();
    // Written straight: the new half exists, so the links resolve.
    let pid = store.iter().find(|e| e.class == "IfcProject").map(|e| e.id()).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    txn.set_param(pid, mep::CONNECTIONS_KEY, serde_json::to_value(links).expect("serialises"))
}

/// Puts the run in the system of what it is connected to, when one carries its classification
/// (or the run has none).
pub(crate) fn join_system(txn: &mut Transaction<'_>, made: &[ElementId], c: Option<Classification>) -> Result<Option<ElementId>> {
    let store = txn.store().clone();
    let Some(first) = made.first() else { return Ok(None) };
    let net: Vec<ElementId> = mep::network(&store, *first);
    let system = crate::mep_systems::systems(&store).into_iter().find(|s| {
        c.is_none_or(|c| c == s.classification) && s.elements.iter().any(|e| net.contains(e) && !made.contains(e))
    });
    let Some(s) = system else { return Ok(None) };
    let e = store.get(s.id).expect("listed");
    let mut r = crate::mep_systems::recipe(e).expect("a system");
    for id in made {
        let guid = store.get(*id).expect("made").guid().to_string();
        for p in mep::ports(&store, store.get(*id).expect("made")) {
            let sp = mep::StoredPort { element: guid.clone(), port: p.index };
            if !r.members.contains(&sp) {
                r.members.push(sp);
            }
        }
    }
    let v = serde_json::to_value(&r).expect("serialises");
    txn.modify(s.id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    Ok(Some(s.id))
}

/// Draws a duct run (`duct.create`).
pub(crate) fn create_duct(txn: &mut Transaction<'_>, p: CreateParams) -> Result<RunReport> {
    draw(txn, Domain::Duct, p)
}

/// Draws a pipe run (`pipe.create`).
pub(crate) fn create_pipe(txn: &mut Transaction<'_>, p: CreateParams) -> Result<RunReport> {
    draw(txn, Domain::Pipe, p)
}

/// How long an outlet fitting under a gutter is.
pub const OUTLET_LENGTH: f64 = 0.06;

/// An outlet under a gutter (MEP-PIPE-010): an `IfcPipeFitting` (CONNECTOR) with a leg up to the
/// gutter's port at `top` and one down to the downspout.
pub(crate) fn make_outlet(txn: &mut Transaction<'_>, level: &str, top: V3, diameter: f64) -> Result<ElementId> {
    let size = Size { shape: Shape::Round, diameter, width: 0.0, height: 0.0 };
    let cx = Ctx { domain: Domain::Pipe, predefined: "RIGIDSEGMENT", type_guid: String::new(), routing: Routing::default(), level: level.to_owned(), classification: Some(Classification::StormDrainage) };
    let centre = [top[0], top[1], top[2] - OUTLET_LENGTH / 2.0];
    make_fitting(
        txn,
        &cx,
        Part::Coupling,
        centre,
        vec![
            Leg { direction: [0.0, 0.0, 1.0], length: OUTLET_LENGTH / 2.0, size, across: None },
            Leg { direction: [0.0, 0.0, -1.0], length: OUTLET_LENGTH / 2.0, size, across: None },
        ],
    )
}

/// Draws a cable tray run (`cable_tray.create`).
pub(crate) fn create_tray(txn: &mut Transaction<'_>, p: CreateParams) -> Result<RunReport> {
    draw(txn, Domain::CableTray, p)
}

/// Draws a conduit run (`conduit.create`).
pub(crate) fn create_conduit(txn: &mut Transaction<'_>, p: CreateParams) -> Result<RunReport> {
    draw(txn, Domain::Conduit, p)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSizeParams {
    pub element_ids: Vec<ElementRef>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub diameter: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub width: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub height: Option<f64>,
}

/// Resizes segments and the legs of fittings joined to them (`duct.set_size`, `pipe.set_size`).
pub(crate) fn set_size(txn: &mut Transaction<'_>, p: SetSizeParams) -> Result<()> {
    let store = txn.store().clone();
    let partners = mep::partners(&store);
    for r in &p.element_ids {
        let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such segment"))?;
        let mut s = store
            .get(id)
            .and_then(segment_recipe)
            .ok_or_else(|| bad("mep_run.not_a_segment", format!("element {id} is not a duct or pipe segment")))?;
        if s.size_locked {
            return Err(bad("mep_run.size_locked", "Element is size-locked"));
        }
        if let Some(d) = p.diameter {
            s.size.diameter = d;
        }
        if let Some(w) = p.width {
            s.size.width = w;
        }
        if let Some(h) = p.height {
            s.size.height = h;
        }
        match type_of_segment(&store, &s) {
            Some((_, t)) => t.check_size(&store, &s.size)?,
            None => s.size.check(s.domain)?,
        }
        let v = serde_json::to_value(&s).expect("serialises");
        txn.modify(id, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
        for end in [0usize, 1] {
            for (fid, leg) in partners.get(&(id, end)).into_iter().flatten() {
                let Some(mut f) = txn.store().get(*fid).and_then(fitting_recipe) else { continue };
                if let Some(l) = f.legs.get_mut(*leg) {
                    l.size = s.size;
                }
                let v = serde_json::to_value(&f).expect("serialises");
                txn.modify(*fid, |e| {
                    e.params.insert(RECIPE_KEY.into(), v);
                })?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapParams {
    /// Segments and fittings; every one when empty.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
}

/// Caps the open ends of segments and fittings (`duct.cap_open_ends`, `pipe.cap_open_ends`);
/// returns the caps.
pub(crate) fn cap_open_ends(txn: &mut Transaction<'_>, p: CapParams) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = if p.element_ids.is_empty() {
        store.iter().filter(|e| is_run_element(e)).map(|e| e.id()).collect()
    } else {
        p.element_ids.iter().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))).collect::<Result<_>>()?
    };
    let open = mep::open_ports(&store, &[]);
    let mut caps = Vec::new();
    for port in open.iter().filter(|o| ids.contains(&o.element)) {
        let e = store.get(port.element).expect("listed");
        if !is_run_element(e) {
            continue;
        }
        let (domain, level, class_, type_guid) = match (segment_recipe(e), fitting_recipe(e)) {
            (Some(s), _) => (s.domain, s.level, s.classification, Some(s.segment_type)),
            (_, Some(f)) => (f.domain, f.level, f.classification, None),
            _ => continue,
        };
        let routing = type_guid
            .as_deref()
            .and_then(GlobalId::parse)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|t| store.get(t))
            .and_then(segment_type)
            .map(|t| t.routing)
            .unwrap_or_default();
        let size = Size { shape: port.connector.shape, diameter: port.connector.diameter, width: port.connector.width, height: port.connector.height };
        let Some(part) = routing.choose(Role::Cap, size.nominal()) else {
            warn(txn, "mep.routing.no_part", format!("no cap in the routing preferences fits {}", size.text()), vec![port.element]);
            continue;
        };
        let cx = Ctx { domain, predefined: "RIGIDSEGMENT", type_guid: String::new(), routing, level, classification: class_ };
        // The cap's one leg points back into the end it closes, so the two ports face.
        let centre = add(port.position, mul(port.direction, 0.03));
        let id = make_fitting(txn, &cx, part, centre, vec![Leg { direction: mul(port.direction, -1.0), length: 0.03, size, across: None }])?;
        mep::connect_coincident(txn, &[id])?;
        caps.push(id);
    }
    Ok(caps)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReapplyParams {
    /// Fittings (or segments, for the fittings joined to them); every fitting when empty.
    #[serde(default)]
    pub element_ids: Vec<ElementRef>,
}

/// What a reapply did.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ReapplyReport {
    pub replaced: Vec<ElementId>,
    /// Fittings whose role has no row that fits: removed, their connections left open.
    pub removed: Vec<ElementId>,
}

/// Re-chooses fittings by their segments' routing preferences (`mep_run.reapply_type`): a
/// fitting whose part changes is remade; one whose role has no fitting row is removed with a
/// warning, its ends left open.
pub(crate) fn reapply_type(txn: &mut Transaction<'_>, p: ReapplyParams) -> Result<ReapplyReport> {
    let store = txn.store().clone();
    let partners = mep::partners(&store);
    let mut fittings: Vec<ElementId> = Vec::new();
    if p.element_ids.is_empty() {
        fittings = store.iter().filter(|e| fitting_recipe(e).is_some()).map(|e| e.id()).collect();
    } else {
        for r in &p.element_ids {
            let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))?;
            let e = store.get(id).expect("resolved");
            if fitting_recipe(e).is_some() {
                fittings.push(id);
            } else if segment_recipe(e).is_some() {
                for end in [0, 1] {
                    for (f, _) in partners.get(&(id, end)).into_iter().flatten() {
                        if store.get(*f).and_then(fitting_recipe).is_some() && !fittings.contains(f) {
                            fittings.push(*f);
                        }
                    }
                }
            }
        }
    }
    let mut report = ReapplyReport::default();
    for fid in fittings {
        let f = store.get(fid).and_then(fitting_recipe).expect("a fitting");
        let role = match f.part {
            Part::Elbow => Role::Elbow,
            Part::Tee | Part::Tap => Role::Junction,
            Part::Cross => Role::Cross,
            Part::Transition => Role::Transition,
            Part::Cap => Role::Cap,
            Part::Coupling | Part::Flange => Role::Union,
            // Boxes and accessories are placed by hand.
            Part::Box | Part::Valve | Part::Meter | Part::Strainer | Part::Damper | Part::Filter | Part::Coil | Part::Silencer => continue,
        };
        // The routing of a segment it joins.
        let routing = (0..f.legs.len())
            .flat_map(|k| partners.get(&(fid, k)).into_iter().flatten())
            .find_map(|(s, _)| {
                let sr = store.get(*s).and_then(segment_recipe)?;
                let t = store.find_by_guid(&GlobalId::parse(&sr.segment_type)?)?;
                // A flex duct's tap follows the rigid duct's routing.
                store.get(t).and_then(segment_type).filter(|t| !t.flexible).map(|t| t.routing)
            })
            .unwrap_or_default();
        let size = match role {
            Role::Junction => f.legs.last().map_or(0.0, |l| l.size.nominal()),
            _ => f.legs.first().map_or(0.0, |l| l.size.nominal()),
        };
        match routing.choose(role, size) {
            Some(part) if part == f.part => {}
            Some(part) if role == Role::Junction && (part == Part::Tap) != (f.part == Part::Tap) => {
                // A tee becoming a tap (or back) changes the host segments; left as it is.
                warn(txn, "mep.routing.junction_kind", format!("the junction would now be a {}; redraw the branch to change it", part.label().to_lowercase()), vec![fid]);
            }
            Some(part) => {
                let mut nf = f.clone();
                nf.part = part;
                let v = serde_json::to_value(&nf).expect("serialises");
                let (pre, _) = part.ifc_in(f.domain);
                txn.modify(fid, |e| {
                    e.params.insert(RECIPE_KEY.into(), v);
                    e.name = Some(part.label().into());
                    e.params.insert("@PredefinedType".into(), Value::from(pre));
                })?;
                report.replaced.push(fid);
            }
            None => {
                warn(
                    txn,
                    "mep.routing.no_part",
                    format!("no {} in the routing preferences fits {:.0} mm; the fitting is removed and its ends left open", role.label(), size * 1000.0),
                    vec![fid],
                );
                txn.delete(fid)?;
                report.removed.push(fid);
            }
        }
    }
    // Links to removed fittings drop out.
    let store = txn.store().clone();
    let links = mep::stored_links(&store);
    mep::save_links(txn, links)?;
    Ok(report)
}

// ---- Fittings by hand, split, "+" and "−", parallel conduits ----

/// A segment's type and recipe.
fn type_of_segment(store: &Store, s: &SegmentRecipe) -> Option<(ElementId, SegmentType)> {
    let t = store.find_by_guid(&GlobalId::parse(&s.segment_type)?)?;
    Some((t, segment_type(store.get(t)?)?))
}

/// The type a run element belongs to: a segment's own, or a fitting's neighbours'.
fn type_of_element(store: &Store, id: ElementId) -> Option<(ElementId, SegmentType)> {
    let e = store.get(id)?;
    if let Some(s) = segment_recipe(e) {
        return type_of_segment(store, &s);
    }
    let f = fitting_recipe(e)?;
    let partners = mep::partners(store);
    (0..f.legs.len())
        .flat_map(|k| partners.get(&(id, k)).into_iter().flatten())
        .find_map(|(o, _)| type_of_segment(store, &store.get(*o).and_then(segment_recipe)?))
}

/// The level-left direction square to `axis` (X for a vertical one).
fn side_of(axis: V3) -> V3 {
    unit(cross([0.0, 0.0, 1.0], axis)).unwrap_or([1.0, 0.0, 0.0])
}

/// Cuts a straight segment at `q` (on its axis) with a gap of `half` each side: the first part
/// keeps the element, the second is new and takes over the end's connection. Returns the second.
fn cut(txn: &mut Transaction<'_>, host: ElementId, hs: &SegmentRecipe, q: V3, half: f64) -> Result<ElementId> {
    let store = txn.store().clone();
    let axis = unit(sub(hs.end, hs.start)).expect("a segment");
    let mut first = hs.clone();
    first.end = add(q, mul(axis, -half));
    let mut second = hs.clone();
    second.start = add(q, mul(axis, half));
    let along = |p: V3| dot(sub(p, hs.start), axis);
    let at = along(q);
    first.taps.retain(|t| along(t.at) < at);
    second.taps.retain(|t| along(t.at) > at);
    if len(sub(first.end, first.start)) < 0.01 || len(sub(second.end, second.start)) < 0.01 {
        return Err(bad("mep_run.too_short", "the point is too near the segment's end"));
    }
    let v1 = serde_json::to_value(&first).expect("serialises");
    txn.modify(host, |e| {
        e.params.insert(RECIPE_KEY.into(), v1);
    })?;
    let he = store.get(host).expect("host");
    let mut params = he.params.clone();
    params.insert(RECIPE_KEY.into(), serde_json::to_value(&second).expect("serialises"));
    let new_half = txn.create(NewElement { class: he.class.clone(), name: he.name.clone(), params, guid: None })?;
    retarget_end(txn, host, new_half)?;
    crate::mep_coverings::follow_split(txn, host, &[new_half])?;
    Ok(new_half)
}

/// Where on a run a point is: the open port nearest it, or a straight segment it lies over.
enum Target {
    End(Port),
    Along(ElementId, SegmentRecipe, V3),
}

fn target_at(store: &Store, domain: Domain, host: Option<ElementId>, point: Option<V3>) -> Result<Target> {
    let open = mep::open_ports(store, &[domain]);
    let ours = |id: ElementId| host.is_none_or(|h| h == id);
    // In plan the height does not count, unless the point gives one.
    let dist = |a: V3, b: V3, plan: bool| if plan { ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt() } else { len(sub(a, b)) };
    let (p, plan) = match point {
        Some(p) => (p, p[2].is_nan()),
        None => {
            // The host's middle, or its one open end.
            let h = host.ok_or_else(|| bad("mep_fitting.where", "give a point or an element"))?;
            if let Some(s) = store.get(h).and_then(segment_recipe)
                && s.corner.is_none()
                && s.flex.is_none()
            {
                let q = mul(add(s.start, s.end), 0.5);
                return Ok(Target::Along(h, s, q));
            }
            let port = open.iter().find(|o| o.element == h).cloned().ok_or_else(|| bad("mep_fitting.where", "the element has no open end"))?;
            return Ok(Target::End(port));
        }
    };
    let best_end = open
        .iter()
        .filter(|o| ours(o.element) && store.get(o.element).is_some_and(is_run_element))
        .map(|o| (dist(o.position, p, plan), o))
        .filter(|(d, o)| *d <= o.connector.width.max(o.connector.diameter).max(0.1) + 0.15)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let best_along = store
        .iter()
        .filter(|e| ours(e.id()))
        .filter_map(|e| {
            let s = segment_recipe(e)?;
            if s.domain != domain || s.corner.is_some() || s.flex.is_some() {
                return None;
            }
            let u = unit(sub(s.end, s.start))?;
            let l = len(sub(s.end, s.start));
            // Along the axis to the point's foot: in plan, by the plan length.
            let t = if plan {
                let lh = (u[0] * u[0] + u[1] * u[1]).sqrt();
                if lh < 1e-6 {
                    return None;
                }
                ((p[0] - s.start[0]) * u[0] + (p[1] - s.start[1]) * u[1]) / (lh * lh)
            } else {
                dot(sub(p, s.start), u)
            };
            if !(0.0..=l).contains(&t) {
                return None;
            }
            let pp = p;
            let q = add(s.start, mul(u, t));
            let d = dist(q, pp, plan);
            (d <= s.size.nominal() / 2.0 + 0.1).then(|| (d, e.id(), s, q))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    match (best_end, best_along) {
        (Some((de, port)), Some((da, ..))) if de <= da + 0.05 => Ok(Target::End(port.clone())),
        (_, Some((_, id, s, q))) => Ok(Target::Along(id, s, q)),
        (Some((_, port)), None) => Ok(Target::End(port.clone())),
        (None, None) => Err(bad("mep_fitting.where", format!("there is no {} at that point", domain.label().to_lowercase()))),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceFittingParams {
    /// elbow, tee, cross, coupling, cap or (conduit) box.
    pub part: Part,
    /// A tray or conduit (or an element with an open end); found from `point` when omitted.
    pub host_id: Option<ElementRef>,
    /// Where: `[x, y]` in plan (any height) or `[x, y, z]`; the host's middle (or open end)
    /// when omitted.
    pub point: Option<Vec<f64>>,
    /// Elbows: the turn in degrees (default 90).
    pub angle: Option<f64>,
    /// The direction a tee's branch or an elbow's turn goes (default level, to the left).
    pub direction: Option<V3>,
    /// Boxes: how many connectors (2–6; default 2).
    pub connectors: Option<usize>,
    /// Elbows: the bend radius (default the type's).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub bend_radius: Option<f64>,
}

/// What a placed fitting made.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlaceReport {
    pub fitting: ElementId,
    /// The new part of a split segment.
    pub split: Option<ElementId>,
}

fn place_fitting(txn: &mut Transaction<'_>, domain: Domain, p: PlaceFittingParams) -> Result<PlaceReport> {
    let store = txn.store().clone();
    let host = p.host_id.as_ref().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such element"))).transpose()?;
    if p.part == Part::Box && domain != Domain::Conduit {
        return Err(bad("mep_fitting.part", "boxes go on conduit"));
    }
    if matches!(p.part, Part::Tap | Part::Transition) {
        return Err(bad("mep_fitting.part", "taps and transitions are placed by drawing runs"));
    }
    if p.part.is_accessory() || p.part == Part::Flange {
        return Err(bad("mep_fitting.part", "valves, meters, strainers and flanges are placed with pipe_accessory.place"));
    }
    let point = match p.point.as_deref() {
        None => None,
        Some([x, y]) => Some([*x, *y, f64::NAN]),
        Some([x, y, z]) => Some([*x, *y, *z]),
        Some(_) => return Err(bad("mep_fitting.point", "a point is [x, y] or [x, y, z]")),
    };
    let target = target_at(&store, domain, host, point)?;
    let on = match &target {
        Target::End(port) => port.element,
        Target::Along(id, ..) => *id,
    };
    let (_, ty) = type_of_element(&store, on).ok_or_else(|| bad("mep_fitting.where", "the element has no tray or conduit type"))?;
    if ty.domain != domain {
        return Err(bad("mep_fitting.where", format!("that is a {}", ty.domain.label().to_lowercase())));
    }
    if !ty.with_fittings {
        let name = store
            .iter()
            .find(|e| segment_type(e).is_some_and(|t| t == ty))
            .and_then(|e| e.name.clone())
            .unwrap_or_default();
        return Err(bad(
            "mep_fitting.no_fittings",
            format!("{name} is a {} type without fittings: its bends are part of its segments, and fittings cannot be placed on it", domain.label().to_lowercase()),
        ));
    }
    let (level, classification) = match store.get(on).and_then(segment_recipe) {
        Some(s) => (s.level, s.classification),
        None => {
            let f = store.get(on).and_then(fitting_recipe).expect("a run element");
            (f.level, f.classification)
        }
    };
    let cx = Ctx { domain, predefined: ty.predefined(), type_guid: String::new(), routing: ty.routing.clone(), level, classification };
    let short = |size: &Size| (size.nominal() * 0.3 + 0.02).max(0.03);
    let (fitting, split) = match target {
        Target::Along(id, hs, q) => {
            let axis = unit(sub(hs.end, hs.start)).expect("a segment");
            let side = p.direction.and_then(unit).map(|d| unit(sub(d, mul(axis, dot(d, axis)))).unwrap_or(side_of(axis))).unwrap_or_else(|| side_of(axis));
            let size = hs.size;
            let (half, mut legs) = match p.part {
                Part::Tee | Part::Cross => {
                    let l = leg_length(&size);
                    let mut legs = vec![
                        Leg { direction: mul(axis, -1.0), length: l, size, across: hs.across },
                        Leg { direction: axis, length: l, size, across: hs.across },
                        Leg { direction: side, length: l, size, across: None },
                    ];
                    if p.part == Part::Cross {
                        legs.push(Leg { direction: mul(side, -1.0), length: l, size, across: None });
                    }
                    (l, legs)
                }
                Part::Coupling => {
                    let l = short(&size);
                    (l, vec![Leg { direction: mul(axis, -1.0), length: l, size, across: hs.across }, Leg { direction: axis, length: l, size, across: hs.across }])
                }
                Part::Box => {
                    let h = box_side(&size) / 2.0;
                    (h, vec![Leg { direction: mul(axis, -1.0), length: h, size, across: hs.across }, Leg { direction: axis, length: h, size, across: hs.across }])
                }
                Part::Elbow => return Err(bad("mep_fitting.elbow_along", "Elbows must be placed at an open end")),
                _ => return Err(bad("mep_fitting.part", format!("a {} goes on an open end, not along a run", p.part.label().to_lowercase()))),
            };
            if p.part == Part::Box {
                add_box_legs(&mut legs, p.connectors.unwrap_or(2), axis, side, size)?;
            }
            let second = cut(txn, id, &hs, q, half)?;
            let f = make_fitting(txn, &cx, p.part, q, legs)?;
            (f, Some(second))
        }
        Target::End(port) => {
            let d = port.direction;
            let size = Size { shape: port.connector.shape, diameter: port.connector.diameter, width: port.connector.width, height: port.connector.height };
            let side = p.direction.and_then(unit).map(|x| unit(sub(x, mul(d, dot(x, d)))).unwrap_or(side_of(d))).unwrap_or_else(|| side_of(d));
            match p.part {
                Part::Cap => {
                    let c = add(port.position, mul(d, 0.03));
                    (make_fitting(txn, &cx, Part::Cap, c, vec![Leg { direction: mul(d, -1.0), length: 0.03, size, across: None }])?, None)
                }
                Part::Coupling => {
                    let l = short(&size);
                    let c = add(port.position, mul(d, l));
                    (make_fitting(txn, &cx, Part::Coupling, c, vec![Leg { direction: mul(d, -1.0), length: l, size, across: None }, Leg { direction: d, length: l, size, across: None }])?, None)
                }
                Part::Elbow => {
                    let theta = p.angle.unwrap_or(90.0);
                    if !(1.0..=179.0).contains(&theta) {
                        return Err(bad("mep_fitting.angle", "an elbow turns from 1° to 179°"));
                    }
                    let th = theta.to_radians();
                    let out = unit(add(mul(d, th.cos()), mul(side, th.sin()))).expect("turned");
                    // Ducts and pipes: a mitred elbow by the routing preferences.
                    if !ty.radius_bends() {
                        if cx.routing.choose(Role::Elbow, size.nominal()).is_none() {
                            return Err(bad("mep_fitting.no_part", format!("no elbow in the routing preferences fits {}", size.text())));
                        }
                        let l = elbow_leg(&size, d, out, None);
                        let c = add(port.position, mul(d, l));
                        let legs = vec![Leg { direction: mul(d, -1.0), length: l, size, across: None }, Leg { direction: out, length: l, size, across: None }];
                        let f = make_fitting(txn, &cx, Part::Elbow, c, legs)?;
                        let mut made = vec![f, on];
                        made.dedup();
                        mep::connect_coincident(txn, &made)?;
                        return Ok(PlaceReport { fitting: f, split: None });
                    }
                    let r = p.bend_radius.unwrap_or_else(|| ty.bend_radius(&store, &size));
                    if domain == Domain::CableTray && r < size.width / 2.0 - 1e-9 {
                        return Err(bad("mep_run.bend_radius", format!("a bend radius of {:.0} mm is below half the tray's width", r * 1000.0)));
                    }
                    let l = r * (th / 2.0).tan();
                    let c = add(port.position, mul(d, l));
                    (make_fitting_r(txn, &cx, Part::Elbow, c, vec![Leg { direction: mul(d, -1.0), length: l, size, across: None }, Leg { direction: out, length: l, size, across: None }], Some(r))?, None)
                }
                Part::Tee | Part::Cross => {
                    let l = leg_length(&size);
                    let c = add(port.position, mul(d, l));
                    let mut legs = vec![Leg { direction: mul(d, -1.0), length: l, size, across: None }, Leg { direction: d, length: l, size, across: None }, Leg { direction: side, length: l, size, across: None }];
                    if p.part == Part::Cross {
                        legs.push(Leg { direction: mul(side, -1.0), length: l, size, across: None });
                    }
                    (make_fitting(txn, &cx, p.part, c, legs)?, None)
                }
                Part::Box => {
                    let h = box_side(&size) / 2.0;
                    let c = add(port.position, mul(d, h));
                    let mut legs = vec![Leg { direction: mul(d, -1.0), length: h, size, across: None }, Leg { direction: d, length: h, size, across: None }];
                    add_box_legs(&mut legs, p.connectors.unwrap_or(2), d, side, size)?;
                    (make_fitting(txn, &cx, Part::Box, c, legs)?, None)
                }
                _ => unreachable!("refused above"),
            }
        }
    };
    let mut made = vec![fitting];
    made.extend(split);
    made.push(on);
    mep::connect_coincident(txn, &made)?;
    Ok(PlaceReport { fitting, split })
}

/// A box's further connectors, on its other faces (2 to 6 in all).
fn add_box_legs(legs: &mut Vec<Leg>, n: usize, axis: V3, side: V3, size: Size) -> Result<()> {
    if !(2..=6).contains(&n) {
        return Err(bad("mep_fitting.connectors", "a box has 2 to 6 connectors"));
    }
    let h = legs[0].length;
    let up = unit(cross(axis, side)).unwrap_or([0.0, 0.0, 1.0]);
    for d in [side, mul(side, -1.0), up, mul(up, -1.0)].into_iter().take(n - 2) {
        legs.push(Leg { direction: d, length: h, size, across: None });
    }
    Ok(())
}

/// Places a cable tray fitting (`cable_tray_fitting.place`).
pub(crate) fn place_tray_fitting(txn: &mut Transaction<'_>, p: PlaceFittingParams) -> Result<PlaceReport> {
    place_fitting(txn, Domain::CableTray, p)
}

/// Places a pipe fitting (`pipe_fitting.place`): on an open end (elbow, tee, cross, coupling,
/// cap) or along a pipe (tee, cross, coupling), splitting it.
pub(crate) fn place_pipe_fitting(txn: &mut Transaction<'_>, p: PlaceFittingParams) -> Result<PlaceReport> {
    place_fitting(txn, Domain::Pipe, p)
}

/// The inline accessories Reviera makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccessoryKind {
    GateValve,
    BallValve,
    ButterflyValve,
    GlobeValve,
    CheckValve,
    PressureReducingValve,
    ReliefValve,
    Strainer,
    WaterMeter,
}

impl AccessoryKind {
    fn part(self) -> Part {
        match self {
            AccessoryKind::Strainer => Part::Strainer,
            AccessoryKind::WaterMeter => Part::Meter,
            _ => Part::Valve,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AccessoryKind::GateValve => "Gate Valve",
            AccessoryKind::BallValve => "Ball Valve",
            AccessoryKind::ButterflyValve => "Butterfly Valve",
            AccessoryKind::GlobeValve => "Globe Valve",
            AccessoryKind::CheckValve => "Check Valve",
            AccessoryKind::PressureReducingValve => "Pressure Reducing Valve",
            AccessoryKind::ReliefValve => "Relief Valve",
            AccessoryKind::Strainer => "Strainer",
            AccessoryKind::WaterMeter => "Water Meter",
        }
    }

    /// `IfcValveTypeEnum` (IFC4 has no gate or globe value: gate, ball and butterfly valves
    /// isolate, a globe valve regulates), `IfcFlowMeterTypeEnum`, `IfcFilterTypeEnum`.
    fn predefined(self) -> &'static str {
        match self {
            AccessoryKind::GateValve | AccessoryKind::BallValve | AccessoryKind::ButterflyValve => "ISOLATING",
            AccessoryKind::GlobeValve => "REGULATING",
            AccessoryKind::CheckValve => "CHECK",
            AccessoryKind::PressureReducingValve => "PRESSUREREDUCING",
            AccessoryKind::ReliefValve => "PRESSURERELIEF",
            AccessoryKind::Strainer => "STRAINER",
            AccessoryKind::WaterMeter => "WATERMETER",
        }
    }

    fn key(self) -> String {
        serde_json::to_value(self).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Flanged,
    Threaded,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccessoryParams {
    pub kind: AccessoryKind,
    /// The pipe it goes in; found from `point` when omitted.
    pub pipe_id: Option<ElementRef>,
    /// Where along it (`[x, y]` in plan or `[x, y, z]`); the pipe's middle when omitted.
    pub point: Option<Vec<f64>>,
    /// The handle turned about the pipe, degrees (90° steps; 0: up).
    #[serde(default)]
    pub rotation: f64,
    /// Its ends; flanged from 50 mm, threaded below, by default.
    pub connection: Option<Connection>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccessoryReport {
    pub accessory: ElementId,
    pub flanges: Vec<ElementId>,
    /// The pipe's new second half.
    pub split: ElementId,
}

/// `pipe_accessory.place`: a valve, strainer or meter inserted in a pipe, which is split; a
/// flanged accessory gets the routing preferences' union-role flange on each side.
pub(crate) fn place_accessory(txn: &mut Transaction<'_>, p: AccessoryParams) -> Result<AccessoryReport> {
    let store = txn.store().clone();
    let host = p.pipe_id.as_ref().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such pipe"))).transpose()?;
    let point = match p.point.as_deref() {
        None => None,
        Some([x, y]) => Some([*x, *y, f64::NAN]),
        Some([x, y, z]) => Some([*x, *y, *z]),
        Some(_) => return Err(bad("mep_fitting.point", "a point is [x, y] or [x, y, z]")),
    };
    // Along a pipe: an end is not a place for an inline accessory.
    let target = match (host, point) {
        (Some(h), None) => {
            let hs = store.get(h).and_then(segment_recipe).filter(|s| s.domain == Domain::Pipe && s.corner.is_none()).ok_or_else(|| bad("pipe_accessory.where", format!("element {h} is not a straight pipe")))?;
            let mid = mul(add(hs.start, hs.end), 0.5);
            Target::Along(h, hs, mid)
        }
        _ => target_at(&store, Domain::Pipe, host, point)?,
    };
    let Target::Along(id, hs, q) = target else {
        return Err(bad("pipe_accessory.where", "accessories go along a pipe, not on an open end"));
    };
    let (_, ty) = type_of_segment(&store, &hs).ok_or_else(|| bad("pipe_accessory.where", "the pipe has no type"))?;
    let size = hs.size;
    let d = size.nominal();
    let axis = unit(sub(hs.end, hs.start)).expect("a segment");
    let flanged = p.connection.unwrap_or(if d >= 0.05 - 1e-9 { Connection::Flanged } else { Connection::Threaded }) == Connection::Flanged;
    let flange = flanged.then(|| ty.routing.choose(Role::Union, d)).flatten().filter(|part| *part == Part::Flange);
    // The body's half length (face to face about three diameters) and the flanges' thickness.
    let half = (d * 1.5).max(0.04);
    let t = (d * 0.25).max(0.016);
    let total = half + if flange.is_some() { t } else { 0.0 };
    if len(sub(hs.end, hs.start)) < 2.0 * total + 0.02 {
        return Err(bad("pipe_accessory.short", format!("the pipe is too short for a {}", p.kind.label().to_lowercase())));
    }
    let along = dot(sub(q, hs.start), axis);
    let total_len = len(sub(hs.end, hs.start));
    let q = add(hs.start, mul(axis, along.clamp(total + 0.01, total_len - total - 0.01)));
    let cx = Ctx { domain: Domain::Pipe, predefined: ty.predefined(), type_guid: String::new(), routing: ty.routing.clone(), level: hs.level.clone(), classification: hs.classification };
    let second = cut(txn, id, &hs, q, total)?;
    let part = p.kind.part();
    let acc = make_fitting(txn, &cx, part, q, vec![Leg { direction: mul(axis, -1.0), length: half, size, across: hs.across }, Leg { direction: axis, length: half, size, across: hs.across }])?;
    // Its kind, name, predefined type and handle.
    let up = frame(axis).1;
    let w = frame(axis).0;
    let turn = p.rotation.to_radians();
    let handle = add(mul(up, turn.cos()), mul(w, turn.sin()));
    let mut f = fitting_recipe(txn.store().get(acc).expect("made")).expect("a fitting");
    f.accessory = Some(p.kind.key());
    f.handle = Some(handle);
    let v = serde_json::to_value(&f).expect("serialises");
    let (label, pre) = (p.kind.label(), p.kind.predefined());
    txn.modify(acc, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
        e.params.insert("@PredefinedType".into(), pre.into());
        e.name = Some(label.into());
    })?;
    let mut flanges = Vec::new();
    if let Some(part) = flange {
        for sgn in [-1.0, 1.0] {
            let c = add(q, mul(axis, sgn * (half + t / 2.0)));
            flanges.push(make_fitting(txn, &cx, part, c, vec![Leg { direction: mul(axis, -1.0), length: t / 2.0, size, across: hs.across }, Leg { direction: axis, length: t / 2.0, size, across: hs.across }])?);
        }
    }
    let mut made = vec![acc, id, second];
    made.extend(&flanges);
    mep::connect_coincident(txn, &made)?;
    Ok(AccessoryReport { accessory: acc, flanges, split: second })
}

/// The inline duct accessories Reviera makes (MEP-DUCT-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DuctAccessoryKind {
    FireDamper,
    SmokeDamper,
    ControlDamper,
    BalancingDamper,
    BackdraftDamper,
    Filter,
    HeatingCoil,
    CoolingCoil,
    SoundAttenuator,
}

impl DuctAccessoryKind {
    fn part(self) -> Part {
        match self {
            DuctAccessoryKind::Filter => Part::Filter,
            DuctAccessoryKind::HeatingCoil | DuctAccessoryKind::CoolingCoil => Part::Coil,
            DuctAccessoryKind::SoundAttenuator => Part::Silencer,
            _ => Part::Damper,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DuctAccessoryKind::FireDamper => "Fire Damper",
            DuctAccessoryKind::SmokeDamper => "Smoke Damper",
            DuctAccessoryKind::ControlDamper => "Control Damper",
            DuctAccessoryKind::BalancingDamper => "Balancing Damper",
            DuctAccessoryKind::BackdraftDamper => "Backdraft Damper",
            DuctAccessoryKind::Filter => "Filter",
            DuctAccessoryKind::HeatingCoil => "Heating Coil",
            DuctAccessoryKind::CoolingCoil => "Cooling Coil",
            DuctAccessoryKind::SoundAttenuator => "Sound Attenuator",
        }
    }

    /// The length along the duct, face to face (E-29).
    pub fn default_length(self) -> f64 {
        match self {
            DuctAccessoryKind::FireDamper | DuctAccessoryKind::SmokeDamper => 0.3,
            DuctAccessoryKind::ControlDamper | DuctAccessoryKind::BalancingDamper | DuctAccessoryKind::BackdraftDamper => 0.2,
            DuctAccessoryKind::Filter | DuctAccessoryKind::HeatingCoil | DuctAccessoryKind::CoolingCoil => 0.3,
            DuctAccessoryKind::SoundAttenuator => 0.9,
        }
    }

    /// `IfcDamperTypeEnum`, `IfcFilterTypeEnum`, `IfcCoilTypeEnum`, `IfcDuctSilencerTypeEnum`
    /// (by the duct's shape).
    fn predefined(self, shape: Shape) -> &'static str {
        match self {
            DuctAccessoryKind::FireDamper => "FIREDAMPER",
            DuctAccessoryKind::SmokeDamper => "SMOKEDAMPER",
            DuctAccessoryKind::ControlDamper => "CONTROLDAMPER",
            DuctAccessoryKind::BalancingDamper => "BALANCINGDAMPER",
            DuctAccessoryKind::BackdraftDamper => "BACKDRAFTDAMPER",
            DuctAccessoryKind::Filter => "AIRPARTICLEFILTER",
            DuctAccessoryKind::HeatingCoil => "WATERHEATINGCOIL",
            DuctAccessoryKind::CoolingCoil => "WATERCOOLINGCOIL",
            DuctAccessoryKind::SoundAttenuator => match shape {
                Shape::Round => "ROUND",
                Shape::Rectangular => "RECTANGULAR",
                Shape::Oval => "FLATOVAL",
            },
        }
    }

    fn key(self) -> String {
        serde_json::to_value(self).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DuctAccessoryParams {
    pub kind: DuctAccessoryKind,
    /// The duct it goes in; found from `point` when omitted.
    pub duct_id: Option<ElementRef>,
    /// Where along it, or an open duct end it goes on (`[x, y]` in plan or `[x, y, z]`); the
    /// duct's middle when omitted.
    pub point: Option<Vec<f64>>,
    /// Face to face along the duct (metres); the kind's default when omitted.
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub length: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DuctAccessoryReport {
    pub accessory: ElementId,
    /// The duct's new second half, when it went along a duct.
    pub split: Option<ElementId>,
}

/// `duct_accessory.place`: a damper, filter, coil or sound attenuator of the duct's size and
/// shape, inserted along a straight duct (which is split, both parts connected through it) or
/// on an open duct end.
pub(crate) fn place_duct_accessory(txn: &mut Transaction<'_>, p: DuctAccessoryParams) -> Result<DuctAccessoryReport> {
    let store = txn.store().clone();
    let host = p.duct_id.as_ref().map(|r| store.resolve(r).ok_or_else(|| bad("element.not_found", "no such duct"))).transpose()?;
    let point = match p.point.as_deref() {
        None => None,
        Some([x, y]) => Some([*x, *y, f64::NAN]),
        Some([x, y, z]) => Some([*x, *y, *z]),
        Some(_) => return Err(bad("mep_fitting.point", "a point is [x, y] or [x, y, z]")),
    };
    let length = p.length.unwrap_or(p.kind.default_length());
    if !(length.is_finite() && length > 0.0) {
        return Err(bad("duct_accessory.length", "the length must be above zero"));
    }
    let half = length / 2.0;
    let part = p.kind.part();
    let finish = |txn: &mut Transaction<'_>, acc: ElementId, shape: Shape| -> Result<()> {
        let mut f = fitting_recipe(txn.store().get(acc).expect("made")).expect("a fitting");
        f.accessory = Some(p.kind.key());
        let v = serde_json::to_value(&f).expect("serialises");
        let (label, pre) = (p.kind.label(), p.kind.predefined(shape));
        txn.modify(acc, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
            e.params.insert("@PredefinedType".into(), pre.into());
            e.name = Some(label.into());
        })
    };
    match target_at(&store, Domain::Duct, host, point)? {
        Target::Along(id, hs, q) => {
            let axis = unit(sub(hs.end, hs.start)).expect("a segment");
            let total = len(sub(hs.end, hs.start));
            // The straight duct between its fittings must hold it, with a stub each side.
            if total < length + 0.02 {
                return Err(bad("duct_accessory.short", "Not enough straight duct for this accessory"));
            }
            let along = dot(sub(q, hs.start), axis).clamp(half + 0.01, total - half - 0.01);
            let q = add(hs.start, mul(axis, along));
            let cx = Ctx { domain: Domain::Duct, predefined: "", type_guid: String::new(), routing: Routing::default(), level: hs.level.clone(), classification: hs.classification };
            let second = cut(txn, id, &hs, q, half)?;
            let legs = vec![Leg { direction: mul(axis, -1.0), length: half, size: hs.size, across: hs.across }, Leg { direction: axis, length: half, size: hs.size, across: hs.across }];
            let acc = make_fitting(txn, &cx, part, q, legs)?;
            finish(txn, acc, hs.size.shape)?;
            mep::connect_coincident(txn, &[acc, id, second])?;
            crate::mep_coverings::follow_split(txn, id, &[acc])?;
            Ok(DuctAccessoryReport { accessory: acc, split: Some(second) })
        }
        Target::End(port) => {
            let e = store.get(port.element).expect("listed");
            let (level, classification) = match (segment_recipe(e), fitting_recipe(e)) {
                (Some(s), _) => (s.level, s.classification),
                (_, Some(f)) => (f.level, f.classification),
                _ => return Err(bad("duct_accessory.where", "accessories go on a duct")),
            };
            let size = crate::mep_network::size_of(&port.connector);
            let dir = port.direction;
            let across = segment_recipe(e).and_then(|s| s.across);
            let cx = Ctx { domain: Domain::Duct, predefined: "", type_guid: String::new(), routing: Routing::default(), level, classification };
            let c = add(port.position, mul(dir, half));
            let legs = vec![Leg { direction: mul(dir, -1.0), length: half, size, across }, Leg { direction: dir, length: half, size, across }];
            let acc = make_fitting(txn, &cx, part, c, legs)?;
            finish(txn, acc, size.shape)?;
            mep::connect_coincident(txn, &[acc, port.element])?;
            Ok(DuctAccessoryReport { accessory: acc, split: None })
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HealParams {
    pub first_id: ElementRef,
    pub second_id: ElementRef,
}

/// `mep_segment.heal`: two straight segments in line, of one size, whose facing ends are open
/// (as an inline accessory's deletion leaves them), joined into the first; the second is
/// deleted and the far ends keep their connections.
pub(crate) fn heal(txn: &mut Transaction<'_>, p: HealParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let a = store.resolve(&p.first_id).ok_or_else(|| bad("element.not_found", "no such segment"))?;
    let b = store.resolve(&p.second_id).ok_or_else(|| bad("element.not_found", "no such segment"))?;
    let seg = |id: ElementId| store.get(id).and_then(segment_recipe).filter(|s| s.corner.is_none() && s.flex.is_none()).ok_or_else(|| bad("mep_run.not_a_segment", format!("element {id} is not a straight segment")));
    let (sa, sb) = (seg(a)?, seg(b)?);
    if a == b || sa.domain != sb.domain || sa.size != sb.size {
        return Err(bad("mep_segment.heal", "the segments must be two of one kind and one size"));
    }
    if !sa.taps.is_empty() || !sb.taps.is_empty() {
        return Err(bad("mep_segment.heal", "segments with taps cannot be joined"));
    }
    let ends = |s: &SegmentRecipe| [s.start, s.end];
    // The facing ends: the nearest pair.
    let (ia, ib) = [(0, 0), (0, 1), (1, 0), (1, 1)].into_iter().min_by(|x, y| len(sub(ends(&sa)[x.0], ends(&sb)[x.1])).total_cmp(&len(sub(ends(&sa)[y.0], ends(&sb)[y.1])))).expect("four pairs");
    let (far_a, far_b) = (ends(&sa)[1 - ia], ends(&sb)[1 - ib]);
    let u = unit(sub(far_b, far_a)).ok_or_else(|| bad("mep_segment.heal", "the segments overlap"))?;
    let in_line = |q: V3| len(sub(sub(q, far_a), mul(u, dot(sub(q, far_a), u)))) < 1e-4;
    if !in_line(ends(&sa)[ia]) || !in_line(ends(&sb)[ib]) {
        return Err(bad("mep_segment.heal", "the segments are not in line"));
    }
    let partners = mep::partners(&store);
    if partners.contains_key(&(a, ia)) || partners.contains_key(&(b, ib)) {
        return Err(bad("mep_segment.heal", "the facing ends are connected; delete what is between them first"));
    }
    // The joined segment runs from the first's far end (port 0) to the second's (port 1).
    let (ga, gb) = (mep::stored(&store, a, 0).element, mep::stored(&store, b, 0).element);
    let links: Vec<mep::Link> = mep::stored_links(&store)
        .into_iter()
        .filter(|l| l.a.element != gb && l.b.element != gb || [&l.a, &l.b].iter().any(|s| s.element == gb && s.port == 1 - ib))
        .map(|mut l| {
            for s in [&mut l.a, &mut l.b] {
                if s.element == ga && s.port == 1 - ia {
                    s.port = 0;
                } else if s.element == gb && s.port == 1 - ib {
                    s.element = ga.clone();
                    s.port = 1;
                }
            }
            l
        })
        .collect();
    let mut joined = sa.clone();
    joined.start = far_a;
    joined.end = far_b;
    let v = serde_json::to_value(&joined).expect("serialises");
    txn.modify(a, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    for c in crate::mep_coverings::of_host(&store, b) {
        txn.delete(c)?;
    }
    txn.delete(b)?;
    mep::save_links(txn, links)?;
    Ok(a)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TeeToCrossParams {
    pub fitting_id: ElementRef,
}

/// `mep_fitting.tee_to_cross`: a tee gains a fourth leg opposite its branch, of the run's size,
/// left open.
pub(crate) fn tee_to_cross(txn: &mut Transaction<'_>, p: TeeToCrossParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.fitting_id).ok_or_else(|| bad("element.not_found", "no such fitting"))?;
    let mut f = store.get(id).and_then(fitting_recipe).filter(|f| f.part == Part::Tee && f.legs.len() == 3).ok_or_else(|| bad("mep_fitting.not_a_tee", format!("element {id} is not a tee")))?;
    let (run, branch) = (f.legs[0], f.legs[2]);
    f.legs.push(Leg { direction: mul(branch.direction, -1.0), length: branch.length, size: run.size, across: branch.across });
    f.part = Part::Cross;
    let v = serde_json::to_value(&f).expect("serialises");
    let (pre, _) = Part::Cross.ifc_in(f.domain);
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
        e.params.insert("@PredefinedType".into(), pre.into());
        e.name = Some(Part::Cross.label().into());
    })
}

/// Places a conduit fitting or box (`conduit_fitting.place`).
pub(crate) fn place_conduit_fitting(txn: &mut Transaction<'_>, p: PlaceFittingParams) -> Result<PlaceReport> {
    place_fitting(txn, Domain::Conduit, p)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitParams {
    pub id: ElementRef,
    /// Where (`[x, y]` in plan or `[x, y, z]`); the middle when omitted.
    pub point: Option<Vec<f64>>,
}

/// What a split made.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SplitReport {
    pub segments: Vec<ElementId>,
    /// The coupling, when the type's routing has one.
    pub coupling: Option<ElementId>,
}

/// Splits a straight segment in two, with a coupling where the type's routing gives one
/// (`cable_tray.split`, `conduit.split`).
pub(crate) fn split(txn: &mut Transaction<'_>, p: SplitParams) -> Result<SplitReport> {
    let store = txn.store().clone();
    let id = store.resolve(&p.id).ok_or_else(|| bad("element.not_found", "no such segment"))?;
    let hs = store.get(id).and_then(segment_recipe).filter(|s| s.corner.is_none()).ok_or_else(|| bad("mep_run.not_a_segment", format!("element {id} is not a straight segment")))?;
    let axis = unit(sub(hs.end, hs.start)).expect("a segment");
    let q = match p.point.as_deref() {
        None => mul(add(hs.start, hs.end), 0.5),
        Some([x, y]) | Some([x, y, _]) => {
            let pp = [*x, *y, hs.start[2]];
            let uh = unit([axis[0], axis[1], 0.0]);
            let t = match uh {
                Some(uh) => dot(sub(pp, hs.start), uh) / dot(axis, uh),
                None => p.point.as_ref().and_then(|v| v.get(2)).map_or(len(sub(hs.end, hs.start)) / 2.0, |z| z - hs.start[2]),
            };
            add(hs.start, mul(axis, t))
        }
        Some(_) => return Err(bad("mep_fitting.point", "a point is [x, y] or [x, y, z]")),
    };
    let routing = type_of_segment(&store, &hs).map(|t| t.1.routing).unwrap_or_default();
    let coupling_part = routing.choose(Role::Union, hs.size.nominal());
    let half = if coupling_part.is_some() { (hs.size.nominal() * 0.3 + 0.02).max(0.03) } else { 0.0 };
    let second = cut(txn, id, &hs, q, half)?;
    let mut report = SplitReport { segments: vec![id, second], coupling: None };
    if let Some(part) = coupling_part {
        let cx = Ctx { domain: hs.domain, predefined: "RIGIDSEGMENT", type_guid: String::new(), routing, level: hs.level.clone(), classification: hs.classification };
        let f = make_fitting(txn, &cx, part, q, vec![Leg { direction: mul(axis, -1.0), length: half, size: hs.size, across: hs.across }, Leg { direction: axis, length: half, size: hs.size, across: hs.across }])?;
        report.coupling = Some(f);
        mep::connect_coincident(txn, &[f, id, second])?;
    } else {
        mep::connect_coincident(txn, &[id, second])?;
    }
    Ok(report)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FittingParams {
    pub fitting_id: ElementRef,
    /// Downgrade: the open leg to remove (by index); the last open one when omitted.
    pub leg: Option<usize>,
}

fn write_fitting(txn: &mut Transaction<'_>, id: ElementId, f: &FittingRecipe) -> Result<()> {
    let v = serde_json::to_value(f).expect("serialises");
    let (pre, object) = f.part.ifc_in(f.domain);
    let class = fitting_class(f.domain, f.part).to_owned();
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
        e.name = Some(f.part.label().into());
        e.class = class;
        e.params.insert("@PredefinedType".into(), Value::from(pre));
        match object {
            Some(o) => e.params.insert("@ObjectType".into(), Value::from(o)),
            None => e.params.remove("@ObjectType"),
        };
    })
}

/// "+" (`mep_fitting.upgrade`): an elbow becomes a tee, its run continuing straight with an
/// open leg; a tee becomes a cross.
pub(crate) fn upgrade(txn: &mut Transaction<'_>, p: FittingParams) -> Result<Part> {
    let store = txn.store().clone();
    let id = store.resolve(&p.fitting_id).ok_or_else(|| bad("element.not_found", "no such fitting"))?;
    let mut f = store.get(id).and_then(fitting_recipe).ok_or_else(|| bad("mep_fitting.not_a_fitting", format!("element {id} is not a fitting")))?;
    match f.part {
        Part::Elbow if f.legs.len() == 2 => {
            // The new leg carries on from the incoming one; leg indices stay, so connections do.
            let a = f.legs[0];
            f.legs.push(Leg { direction: mul(a.direction, -1.0), ..a });
            f.part = Part::Tee;
            f.radius = None;
        }
        Part::Tee if f.legs.len() == 3 => {
            // Opposite the branch: the leg with no opposite.
            let opp = |k: usize| f.legs.iter().enumerate().any(|(j, l)| j != k && dot(l.direction, f.legs[k].direction) < -0.999);
            let b = (0..3).find(|k| !opp(*k)).ok_or_else(|| bad("mep_fitting.upgrade", "the tee has no branch to oppose"))?;
            let l = f.legs[b];
            f.legs.push(Leg { direction: mul(l.direction, -1.0), ..l });
            f.part = Part::Cross;
        }
        other => return Err(bad("mep_fitting.upgrade", format!("a {} cannot be upgraded", other.label().to_lowercase()))),
    }
    write_fitting(txn, id, &f)?;
    Ok(f.part)
}

/// "−" (`mep_fitting.downgrade`): removes an open leg: a cross becomes a tee, a tee whose two
/// other legs turn becomes an elbow. A box has no "−".
pub(crate) fn downgrade(txn: &mut Transaction<'_>, p: FittingParams) -> Result<Part> {
    let store = txn.store().clone();
    let id = store.resolve(&p.fitting_id).ok_or_else(|| bad("element.not_found", "no such fitting"))?;
    let mut f = store.get(id).and_then(fitting_recipe).ok_or_else(|| bad("mep_fitting.not_a_fitting", format!("element {id} is not a fitting")))?;
    if !matches!(f.part, Part::Cross | Part::Tee) {
        return Err(bad("mep_fitting.downgrade", format!("a {} cannot be downgraded", f.part.label().to_lowercase())));
    }
    let partners = mep::partners(&store);
    let open: Vec<usize> = (0..f.legs.len()).filter(|k| partners.get(&(id, *k)).is_none_or(Vec::is_empty)).collect();
    let k = match p.leg {
        Some(k) if open.contains(&k) => k,
        Some(k) => return Err(bad("mep_fitting.downgrade", format!("leg {k} is connected"))),
        None => *open.last().ok_or_else(|| bad("mep_fitting.downgrade", "every leg is connected"))?,
    };
    f.legs.remove(k);
    f.part = match f.part {
        Part::Cross => Part::Tee,
        _ => {
            if dot(f.legs[0].direction, f.legs[1].direction) < -0.999 {
                return Err(bad("mep_fitting.downgrade", "the tee's other legs are in line; delete it and join the segments"));
            }
            Part::Elbow
        }
    };
    if f.part == Part::Elbow && matches!(f.domain, Domain::CableTray | Domain::Conduit) {
        let theta = (-dot(f.legs[0].direction, f.legs[1].direction)).clamp(-1.0, 1.0).acos();
        f.radius = Some(f.legs[0].length / (theta / 2.0).tan().max(1e-9));
    }
    // Later legs move down one: their connections follow.
    let guid = store.get(id).expect("found").guid().to_string();
    let links: Vec<mep::Link> = mep::stored_links(&store)
        .into_iter()
        .map(|mut l| {
            for end in [&mut l.a, &mut l.b] {
                if end.element == guid && end.port > k {
                    end.port -= 1;
                }
            }
            l
        })
        .collect();
    write_fitting(txn, id, &f)?;
    mep::save_links(txn, links)?;
    Ok(f.part)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BendMode {
    /// Every conduit bends at the same radius.
    #[default]
    Same,
    /// Bends share their centre: radii grow by the offset outward (types without fittings).
    Concentric,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParallelParams {
    /// Elements of the conduit run (any one finds the whole run).
    pub run_ids: Vec<ElementRef>,
    /// Conduits across, the original included (at least 1).
    pub h_count: usize,
    /// Rows up, the original included (at least 1).
    #[serde(default = "one_usize")]
    pub v_count: usize,
    /// Between conduits across, to the left of the run's direction (metres; negative: right).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub h_offset: f64,
    /// Between rows, upward (metres).
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub v_offset: f64,
    #[serde(default)]
    pub bend: BendMode,
}

fn one_usize() -> usize {
    1
}

/// A run's centreline: its ends and corners, from one open end to the other.
fn run_line(store: &Store, start: ElementId, domain: Domain) -> Result<(Vec<V3>, Vec<ElementId>)> {
    let partners = mep::partners(store);
    let in_run = |id: ElementId| store.get(id).is_some_and(|e| {
        segment_recipe(e).is_some_and(|s| s.domain == domain) || fitting_recipe(e).is_some_and(|f| f.domain == domain)
    });
    let count = |id: ElementId| store.get(id).map_or(0, port_count);
    // Walk to one end first.
    let mut members = vec![start];
    let mut stack = vec![start];
    while let Some(x) = stack.pop() {
        for k in 0..count(x) {
            for (o, _) in partners.get(&(x, k)).into_iter().flatten() {
                if in_run(*o) && !members.contains(o) {
                    members.push(*o);
                    stack.push(*o);
                }
            }
        }
    }
    for m in &members {
        let e = store.get(*m).expect("member");
        if let Some(f) = fitting_recipe(e).filter(|f| f.part.is_accessory()) {
            let name = e.name.clone().unwrap_or_else(|| f.part.label().to_owned());
            return Err(bad("mep_run.parallel_accessory", format!("{name} {m} is in the run: parallel runs follow a run without valves, meters or strainers")));
        }
        if let Some(f) = fitting_recipe(e).filter(|f| f.legs.len() > 2) {
            return Err(bad("mep_run.parallel_branches", format!("{} {m} branches the run: parallel runs follow a run without tees, crosses or taps", f.part.label())));
        }
        if segment_recipe(e).is_some_and(|s| !s.taps.is_empty()) {
            return Err(bad("mep_run.parallel_branches", format!("segment {m} has a tap: parallel runs follow a run without tees, crosses or taps")));
        }
    }
    let linked = |x: ElementId, k: usize| partners.get(&(x, k)).into_iter().flatten().find(|(o, _)| members.contains(o)).copied();
    let first = *members
        .iter()
        .find(|m| (0..count(**m)).any(|k| linked(**m, k).is_none()))
        .ok_or_else(|| bad("conduit.parallel_loop", "the run closes on itself"))?;
    let mut pts: Vec<V3> = Vec::new();
    let mut at = first;
    let mut entered = (0..count(first)).find(|k| linked(first, *k).is_none()).expect("an open end");
    let mut seen = Vec::new();
    loop {
        seen.push(at);
        let e = store.get(at).expect("member");
        let exit = if count(at) == 2 { 1 - entered } else { entered };
        if let Some(s) = segment_recipe(e) {
            let (a, b) = if entered == 0 { (s.start, s.end) } else { (s.end, s.start) };
            pts.push(a);
            if let Some(c) = s.corner {
                pts.push(c);
            }
            pts.push(b);
        } else if let Some(f) = fitting_recipe(e) {
            pts.push(f.centre);
        }
        match linked(at, exit) {
            Some((next, port)) if !seen.contains(&next) => {
                at = next;
                entered = port;
            }
            _ => break,
        }
    }
    // Drop repeats and points in line.
    pts.dedup_by(|a, b| len(sub(*a, *b)) < 1e-6);
    let mut i = 1;
    while i + 1 < pts.len() {
        let (a, b) = (unit(sub(pts[i], pts[i - 1])), unit(sub(pts[i + 1], pts[i])));
        match (a, b) {
            (Some(a), Some(b)) if dot(a, b) > 0.9999 => {
                pts.remove(i);
            }
            _ => i += 1,
        }
    }
    Ok((pts, members))
}

/// Parallel conduits (`conduit.create_parallel`): copies of a run offset across and up, bent
/// at the same radius or concentrically. Returns every element made.
pub(crate) fn create_parallel(txn: &mut Transaction<'_>, p: ParallelParams) -> Result<Vec<ElementId>> {
    parallel(txn, p, Domain::Conduit)
}

/// Parallel pipes (`pipe.create_parallel`, MEP-PIPE-003): copies of a level or sloped pipe run
/// offset across and up, with elbows by its type's routing preferences, the same size, type,
/// system type and slope; 1–20 each way, offsets at least the pipe's outside size.
pub(crate) fn create_parallel_pipes(txn: &mut Transaction<'_>, p: ParallelParams) -> Result<Vec<ElementId>> {
    parallel(txn, p, Domain::Pipe)
}

fn parallel(txn: &mut Transaction<'_>, p: ParallelParams, domain: Domain) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let pipe = domain == Domain::Pipe;
    let noun = if pipe { "pipe" } else { "conduit" };
    if pipe {
        if !(1..=20).contains(&p.h_count) || !(1..=20).contains(&p.v_count) || p.h_count * p.v_count < 2 {
            return Err(bad("pipe.parallel_count", "give 1 to 20 pipes across and up, the original included, 2 or more in all"));
        }
    } else if p.h_count == 0 || p.v_count == 0 || p.h_count * p.v_count < 2 || p.h_count * p.v_count > 64 {
        return Err(bad("conduit.parallel_count", "give 1 or more across and up, 2 to 64 conduits in all"));
    }
    if (p.h_count > 1 && p.h_offset.abs() < 1e-6) || (p.v_count > 1 && p.v_offset.abs() < 1e-6) {
        return Err(bad("conduit.parallel_offset", format!("give the offset between {noun}s")));
    }
    let first = p.run_ids.first().and_then(|r| store.resolve(r)).ok_or_else(|| bad("element.not_found", format!("no such {noun}")))?;
    let seg_of = |id: ElementId| store.get(id).and_then(segment_recipe);
    let (pts, members) = run_line(&store, first, domain)?;
    let s0 = members.iter().find_map(|m| seg_of(*m)).ok_or_else(|| bad("mep_run.not_a_run", format!("the elements are not a {noun} run")))?;
    if s0.domain != domain {
        return Err(bad("mep_run.not_a_run", format!("parallel runs here are {noun}")));
    }
    if pipe {
        // Centre to centre, at least the outside size (no insulation is modelled yet).
        let outside = s0.size.diameter;
        if (p.h_count > 1 && p.h_offset.abs() < outside - 1e-9) || (p.v_count > 1 && p.v_offset.abs() < outside - 1e-9) {
            return Err(bad("pipe.parallel_offset", format!("Offset smaller than pipe outside size ({:.0} mm)", outside * 1000.0)));
        }
    }
    let (type_id, ty) = type_of_segment(&store, &s0).ok_or_else(|| bad("mep_run.not_a_run", format!("the {noun} has no type")))?;
    if !pipe && p.bend == BendMode::Concentric && ty.with_fittings {
        return Err(bad("conduit.concentric", "concentric bends are for conduit types without fittings, whose bends are part of the conduit"));
    }
    let dirs: Vec<V3> = pts.windows(2).map(|w| unit(sub(w[1], w[0])).expect("distinct")).collect();
    if !pipe && dirs.iter().any(|d| d[2].abs() > 1e-6) {
        return Err(bad("conduit.parallel_level", "parallel conduits follow level runs"));
    }
    if pipe && dirs.iter().any(|d| d[0].hypot(d[1]) < 0.7) {
        return Err(bad("pipe.parallel_level", "parallel pipes follow level or sloped runs, not risers"));
    }
    // Across, in plan, to the left of each leg.
    let left = |d: V3| {
        let h = d[0].hypot(d[1]).max(1e-12);
        [-d[1] / h, d[0] / h, 0.0]
    };
    // A pipe run's system type, carried over (its system only through equipment).
    let system_type = if pipe {
        crate::mep_systems::systems(&store).into_iter().find(|x| x.elements.contains(&first)).map(|x| x.system_type)
    } else {
        None
    };
    let base_r = ty.bend_radius(&store, &s0.size);
    // The level's elevation: points are given above it.
    let lid = store.find_by_guid(&GlobalId::parse(&s0.level).ok_or_else(|| bad("level.not_found", "the run's level"))?).ok_or_else(|| bad("level.not_found", "the run's level"))?;
    let z0 = crate::levels::elevation_of(&store, &s0.level)?;
    let mut made = Vec::new();
    for j in 0..p.v_count {
        for i in 0..p.h_count {
            if i == 0 && j == 0 {
                continue;
            }
            let d = p.h_offset * i as f64;
            let up = p.v_offset * j as f64;
            let mut line: Vec<V3> = Vec::new();
            let mut radii = Vec::new();
            for k in 0..pts.len() {
                let off = if k == 0 {
                    mul(left(dirs[0]), d)
                } else if k == pts.len() - 1 {
                    mul(left(dirs[k - 1]), d)
                } else {
                    let (n0, n1) = (left(dirs[k - 1]), left(dirs[k]));
                    let turn = dirs[k - 1][0] * dirs[k][1] - dirs[k - 1][1] * dirs[k][0];
                    // Inside the turn the radius shrinks by the offset, outside it grows.
                    let r = if p.bend == BendMode::Concentric { base_r - turn.signum() * d } else { base_r };
                    if !pipe && r <= s0.size.diameter / 2.0 {
                        return Err(bad("conduit.concentric", format!("conduit {} would bend tighter than its size allows; offset it the other way", i + 1)));
                    }
                    radii.push(r);
                    mul(add(n0, n1), d / (1.0 + dot(n0, n1)))
                };
                let q = add(pts[k], off);
                line.push([q[0], q[1], q[2] - z0 + up]);
            }
            // Sloped legs keep their slope over their own (offset) length in plan.
            for k in 1..line.len() {
                let (a, b) = (pts[k - 1], pts[k]);
                let h = (b[0] - a[0]).hypot(b[1] - a[1]);
                if h > 1e-9 {
                    let g = (b[2] - a[2]) / h;
                    let hc = (line[k][0] - line[k - 1][0]).hypot(line[k][1] - line[k - 1][1]);
                    line[k][2] = line[k - 1][2] + g * hc;
                }
            }
            let params = CreateParams {
                type_id: Some(ElementRef::Id(type_id)),
                points: line.iter().map(|v| v.to_vec()).collect(),
                level: ElementRef::Id(lid),
                middle_elevation: None,
                shape: Some(Shape::Round),
                diameter: Some(s0.size.diameter),
                width: None,
                height: None,
                system_type_id: system_type.clone(),
                // The points carry the fall; the slope is recorded on the copies.
                slope: if pipe { s0.slope } else { 0.0 },
                bend_radius: None,
            };
            let r = draw_with(txn, domain, params, (!pipe).then_some(radii))?;
            made.extend(r.segments);
            made.extend(r.fittings);
        }
    }
    if pipe && system_type.is_some() {
        txn.warn(crate::txn::Warning {
            id: "pipe.parallel_unassigned".into(),
            message: "The parallel pipes are not in a system: they join one when connected to its equipment".into(),
            element_ids: made.clone(),
        });
    }
    Ok(made)
}

/// Moves a segment or fitting: its points through `point` in plan and `dz` up; its directions
/// turned (and mirrored) with them.
pub(crate) fn transform(txn: &mut Transaction<'_>, id: ElementId, point: &dyn Fn([f64; 2]) -> [f64; 2], dz: f64) -> Result<()> {
    let e = txn.store().get(id).cloned().ok_or_else(|| bad("element.not_found", "no such element"))?;
    let p3 = |p: V3| {
        let q = point([p[0], p[1]]);
        [q[0], q[1], p[2] + dz]
    };
    // A direction through the plan map: the image of its tip less the image of its tail.
    let d3 = |d: V3| {
        let (o, t) = (point([0.0, 0.0]), point([d[0], d[1]]));
        let v = [t[0] - o[0], t[1] - o[1], d[2]];
        unit(v).unwrap_or(d)
    };
    let value = if let Some(mut s) = segment_recipe(&e) {
        s.start = p3(s.start);
        s.end = p3(s.end);
        s.corner = s.corner.map(p3);
        s.across = s.across.map(d3);
        if let Some(f) = &mut s.flex {
            for q in &mut f.points {
                *q = p3(*q);
            }
            f.start_tangent = d3(f.start_tangent);
            f.end_tangent = d3(f.end_tangent);
        }
        for t in &mut s.taps {
            t.at = p3(t.at);
            t.direction = d3(t.direction);
        }
        serde_json::to_value(&s).expect("serialises")
    } else if let Some(mut f) = fitting_recipe(&e) {
        f.centre = p3(f.centre);
        for l in &mut f.legs {
            l.direction = d3(l.direction);
            l.across = l.across.map(d3);
        }
        serde_json::to_value(&f).expect("serialises")
    } else {
        return Err(bad("mep_run.not_a_run", format!("element {id} is not a duct, pipe or fitting")));
    };
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), value);
    })
}

/// The colour each segment and fitting shows: its system type's, else a domain grey.
pub fn colors(store: &Store) -> BTreeMap<ElementId, [f32; 4]> {
    let by_system = crate::mep_systems::element_colors(store);
    store
        .iter()
        .filter(|e| is_run_element(e))
        .map(|e| {
            let base = match segment_recipe(e).map(|s| s.domain).or(fitting_recipe(e).map(|f| f.domain)) {
                Some(Domain::Duct) => [0.74, 0.76, 0.80, 1.0],
                Some(Domain::CableTray) => [0.70, 0.62, 0.48, 1.0],
                Some(Domain::Conduit) => [0.60, 0.60, 0.66, 1.0],
                _ => [0.62, 0.66, 0.60, 1.0],
            };
            (e.id(), by_system.get(&e.id()).copied().unwrap_or(base))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junction_rows_choose_by_size_and_preference() {
        let r = Routing {
            junction: vec![Row { part: Part::Tap, min: 0.0, max: 0.3 }, Row { part: Part::Tee, min: 0.3, max: 0.0 }],
            preferred_junction: Junction::Tee,
            ..Routing::default()
        };
        assert_eq!(r.choose(Role::Junction, 0.25), Some(Part::Tap));
        assert_eq!(r.choose(Role::Junction, 0.4), Some(Part::Tee));
        let none = Routing { elbow: vec![Row { part: Part::Elbow, min: 0.0, max: 0.2 }], ..Routing::default() };
        assert_eq!(none.choose(Role::Elbow, 0.3), None);
        assert!(Routing { elbow: vec![Row { part: Part::Tee, min: 0.0, max: 0.0 }], ..Routing::default() }.check().is_err());
    }

    #[test]
    fn a_rectangular_section_is_level_across() {
        let s = Size { shape: Shape::Rectangular, diameter: 0.0, width: 0.6, height: 0.3 };
        let pts = section([0.0; 3], [0.0, 1.0, 0.0], &s, None);
        // Across is x for a run along y; up is z.
        assert!((pts[1][0] - pts[0][0]).abs() - 0.6 < 1e-9 && (pts[2][2] - pts[1][2] - 0.3).abs() < 1e-9, "{pts:?}");
    }
}

// ---- Risers (MEP-DUCT-011) ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RiserParams {
    /// A duct, pipe, cable tray or conduit segment, or an element with a connector.
    pub element_id: ElementRef,
    /// The open connector to rise from (a segment's 0 start, 1 end).
    #[serde(default)]
    pub connector_index: usize,
    pub top_level: ElementRef,
    /// Metres above the top level.
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: f64,
    /// Keep the end at the level as it moves (default on).
    #[serde(default = "yes")]
    pub bind: bool,
}

/// `mep_run.create_riser`: an elbow at an open connector and one vertical segment of the same
/// type, size and system to a level plus an offset; its far end is left open.
pub(crate) fn create_riser(txn: &mut Transaction<'_>, p: RiserParams) -> Result<RunReport> {
    let store = txn.store().clone();
    let id = store.resolve(&p.element_id).ok_or_else(|| bad("element.not_found", "no such element"))?;
    let e = store.get(id).expect("resolved");
    let port = mep::port(&store, id, p.connector_index).ok_or_else(|| bad("riser.connector", format!("element {id} has no connector {}", p.connector_index)))?;
    let c = &port.connector;
    if !matches!(c.domain, Domain::Duct | Domain::Pipe | Domain::CableTray | Domain::Conduit) {
        return Err(bad("riser.domain", "risers rise from duct, pipe, cable tray and conduit connectors"));
    }
    if c.surface {
        return Err(bad("riser.surface", "a surface connector cannot rise: rise from a run's open end"));
    }
    if mep::partners(&store).contains_key(&(id, p.connector_index)) {
        return Err(bad("riser.not_open", format!("connector {} is not open", p.connector_index)));
    }
    let size = Size { shape: c.shape, diameter: c.diameter, width: c.width, height: c.height };
    let (top_guid, top_z) = level_of(&store, &p.top_level)?;
    let target = top_z + p.top_offset;
    if !target.is_finite() {
        return Err(bad("riser.offset", "the offset is not a number"));
    }
    let at = port.position;
    if (target - at[2]).abs() < 1e-6 {
        return Err(bad("riser.zero", "Riser length is zero"));
    }
    let up: V3 = if target > at[2] { [0.0, 0.0, 1.0] } else { [0.0, 0.0, -1.0] };
    let d = port.direction;
    let vertical = d[2].abs() >= 0.999;
    if vertical && dot(d, up) < 0.0 {
        return Err(bad("riser.direction", "the connector points the other way"));
    }
    // Its type, level and system: the segment's own, else the built-in type's.
    let seg = segment_recipe(e);
    let known = seg.as_ref().and_then(|s| type_of_segment(&store, s));
    if let Some((_, t)) = &known {
        t.check_size(&store, &size)?;
    }
    let routing = known.as_ref().map(|(_, t)| t.routing.clone()).unwrap_or_else(|| default_routing(c.domain));
    let radius = known.as_ref().filter(|(_, t)| t.radius_bends()).map(|(_, t)| (t.bend_radius(&store, &size), t.with_fittings));
    let elbow = if vertical {
        None
    } else {
        match radius {
            Some((_, false)) => None,
            _ => Some(routing.choose(Role::Elbow, size.nominal()).ok_or_else(|| {
                bad("riser.no_elbow", format!("No Elbow in routing preferences for {:.0} mm", size.nominal() * 1000.0))
            })?),
        }
    };
    let across = seg.as_ref().and_then(|s| s.across).or_else(|| (!vertical).then(|| frame(d).0));
    let leg = if vertical {
        0.0
    } else {
        match radius {
            Some((r, _)) => r,
            None => elbow_leg(&size, d, up, across),
        }
    };
    let corner = add(at, mul(d, leg));
    let bottom = add(corner, mul(up, leg));
    let top = [corner[0], corner[1], target];
    if dot(sub(top, bottom), up) < 0.01 {
        return Err(bad("riser.too_short", "the target is too close for the elbow: the riser would have no length"));
    }
    // Everything checked: nothing has been made yet.
    let type_id = match &known {
        Some((t, _)) => *t,
        None => default_type(txn, c.domain, size.shape)?,
    };
    let store = txn.store().clone();
    let t = segment_type(store.get(type_id).expect("made")).expect("a type");
    let level = match &seg {
        Some(s) => s.level.clone(),
        None => crate::plans::levels(&store).into_iter().rev().find(|(z, _, _)| *z <= at[2] + 1e-6).map(|(_, g, _)| g).unwrap_or_else(|| top_guid.clone()),
    };
    let cx = Ctx {
        domain: c.domain,
        predefined: t.predefined(),
        type_guid: store.get(type_id).expect("made").guid().to_string(),
        routing: t.routing.clone(),
        level,
        classification: seg.as_ref().and_then(|s| s.classification).or(c.classification.filter(|cl| cl.fits(c.domain))),
    };
    let mut report = RunReport::default();
    let legs = vec![Leg { direction: mul(d, -1.0), length: leg, size, across }, Leg { direction: up, length: leg, size, across }];
    match (elbow, radius) {
        (Some(part), Some((r, true))) => report.fittings.push(make_fitting_r(txn, &cx, part, corner, legs, Some(r))?),
        (Some(part), _) => report.fittings.push(make_fitting(txn, &cx, part, corner, legs)?),
        (None, Some((_, false))) if !vertical => {
            // A type without fittings bends in a curved segment.
            report.segments.push(make_segment_c(txn, &cx, at, bottom, size, 0.0, across, Some(corner))?);
        }
        _ => {}
    }
    let riser = make_segment(txn, &cx, bottom, top, size, 0.0, across)?;
    if p.bind {
        let mut r = segment_recipe(txn.store().get(riser).expect("made")).expect("a segment");
        r.riser = Some(RiserTop { level: top_guid, offset: p.top_offset });
        let v = serde_json::to_value(&r).expect("serialises");
        txn.modify(riser, |e| {
            e.params.insert(RECIPE_KEY.into(), v);
        })?;
    }
    report.segments.push(riser);
    let made: Vec<ElementId> = report.segments.iter().chain(&report.fittings).copied().collect();
    for cn in mep::connect_coincident(txn, &made)? {
        for e in [cn.a.0, cn.b.0] {
            if !made.contains(&e) && !report.connected_to.contains(&e) {
                report.connected_to.push(e);
            }
        }
    }
    report.system = join_system(txn, &made, cx.classification)?;
    Ok(report)
}

/// Moves a bound riser's end to its level and offset, and the runs joined beyond it with it.
fn follow_riser(txn: &mut Transaction<'_>, id: ElementId) -> Result<()> {
    let store = txn.store().clone();
    let Some(mut s) = store.get(id).and_then(segment_recipe) else { return Ok(()) };
    let Some(b) = s.riser.clone() else { return Ok(()) };
    let target = crate::levels::elevation_of(&store, &b.level)? + b.offset;
    let dz = target - s.end[2];
    if dz.abs() < 1e-9 {
        return Ok(());
    }
    if (target - s.start[2]) * (s.end[2] - s.start[2]) < 1e-4 {
        return Err(bad("riser.inverted", format!("the riser {id} would turn back on itself: its level moved past its other end")));
    }
    s.end[2] = target;
    let v = serde_json::to_value(&s).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    // Segments and fittings beyond its end move up or down with it.
    let partners = mep::partners(&store);
    let mut seen = std::collections::HashSet::from([id]);
    let mut queue: Vec<ElementId> = partners.get(&(id, 1)).into_iter().flatten().map(|(o, _)| *o).collect();
    while let Some(x) = queue.pop() {
        if !seen.insert(x) || !store.get(x).is_some_and(is_run_element) {
            continue;
        }
        transform(txn, x, &|p| p, dz)?;
        let n = store.get(x).map_or(0, port_count);
        for k in 0..n {
            queue.extend(partners.get(&(x, k)).into_iter().flatten().map(|(o, _)| *o));
        }
    }
    Ok(())
}

/// Bound risers follow a level whose elevation changed (`level.set_elevation`).
pub(crate) fn follow_level(txn: &mut Transaction<'_>, level: &str) -> Result<()> {
    let ids: Vec<ElementId> = txn
        .store()
        .iter()
        .filter(|e| segment_recipe(e).and_then(|s| s.riser).is_some_and(|r| r.level == level))
        .map(|e| e.id())
        .collect();
    for id in ids {
        follow_riser(txn, id)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BindRiserParams {
    /// A vertical segment.
    pub element_id: ElementRef,
    /// The level its upper (or, for a drop, lower) end keeps to; none unbinds it, keeping its
    /// geometry.
    pub top_level: Option<ElementRef>,
    /// Metres above the level (default 0).
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub top_offset: f64,
}

/// `mep_run.bind_riser`: binds a vertical segment's end to a level (moving it there) or unbinds
/// it.
pub(crate) fn bind_riser(txn: &mut Transaction<'_>, p: BindRiserParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.element_id).ok_or_else(|| bad("element.not_found", "no such segment"))?;
    let mut s = store.get(id).and_then(segment_recipe).ok_or_else(|| bad("mep_run.not_a_segment", format!("element {id} is not a segment")))?;
    let d = sub(s.end, s.start);
    if d[2].abs() < 0.999 * len(d) {
        return Err(bad("riser.not_vertical", "only vertical segments are risers"));
    }
    s.riser = match &p.top_level {
        Some(l) => Some(RiserTop { level: level_of(&store, l)?.0, offset: p.top_offset }),
        None => None,
    };
    let v = serde_json::to_value(&s).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })?;
    follow_riser(txn, id)
}
