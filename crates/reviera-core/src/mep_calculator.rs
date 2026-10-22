// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Sizing Calculator (MEP-CALC-005): candidate sizes for one duct or pipe, or for a flow
//! typed in, against friction, velocity and aspect-ratio targets, each with a ± tolerance.
//!
//! - **Inputs**: with an element, its flow (the kept network result, else its system's design
//!   flow), domain, shape, type (its sizing table) and roughness; a typed flow wins. Without one,
//!   the domain and shape given, the built-in type's roughness and sizes.
//! - **Bases**: friction (1 Pa/m for ducts, 300 Pa/m for pipes by default) and velocity (5 m/s,
//!   1.5 m/s) are bands, target ± tolerance (0–50 %, 10 % by default); the aspect ratio (1:3 by
//!   default) is a maximum. A candidate must meet every basis given; with none, friction.
//! - **Candidates**: the type's sizing table (rectangles wider than high, or higher than wide
//!   with `swap`), within the restrictions, sorted by closeness to the friction target (else the
//!   velocity target). When none meets every basis, the three nearest are listed, marked, with
//!   "No size within tolerance".
//!
//! Nothing is stored; Apply is `duct.set_size` or `pipe.set_size` (refused on size-locked
//! segments).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::mep::{Domain, Shape};
use crate::mep_runs::{self, Size};
use crate::mep_sizing::{self, Equation, Restrict, flow_in, section, water};
use crate::store::Store;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn ten() -> f64 {
    0.1
}

fn three() -> f64 {
    3.0
}

/// A target with a ± tolerance (a fraction, 0–0.5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Band {
    /// The target; the domain's default when omitted.
    #[serde(default)]
    pub target: Option<f64>,
    #[serde(default = "ten")]
    pub tolerance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Aspect {
    /// The largest long-to-short side ratio (3 for 1:3).
    #[serde(default = "three")]
    pub max: f64,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidatesParams {
    /// A duct or pipe segment: its flow, domain, shape, type and roughness.
    pub element_id: Option<ElementRef>,
    /// m³/s: a value typed in, in place of the element's.
    pub flow: Option<f64>,
    /// Without an element: duct (default) or pipe.
    pub domain: Option<Domain>,
    /// Without an element: round (default), rectangular or oval (ducts).
    pub shape: Option<Shape>,
    /// Pa/m.
    pub friction: Option<Band>,
    /// m/s.
    pub velocity: Option<Band>,
    /// Rectangular and oval ducts.
    pub aspect: Option<Aspect>,
    /// Metres; the element's type's, else the built-in type's.
    pub roughness: Option<f64>,
    #[serde(default)]
    pub restrict: Restrict,
    /// The project's setting when omitted.
    pub equation: Option<Equation>,
    /// Pipes: the water temperature (°C); the element's system type's, else 20.
    pub fluid_temperature: Option<f64>,
    /// Rectangles higher than wide.
    #[serde(default)]
    pub swap: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Candidate {
    pub size: String,
    pub shape: Shape,
    pub diameter: f64,
    pub width: f64,
    pub height: f64,
    /// Pa/m.
    pub friction: f64,
    /// m/s.
    pub velocity: f64,
    /// Long side over short (rectangles and ovals).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect: Option<f64>,
    /// The round size of equal friction (Huebscher for rectangles; the hydraulic diameter for
    /// ovals).
    pub equivalent_diameter: f64,
    /// (value − target) / target, for the bases given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friction_deviation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity_deviation: Option<f64>,
    /// Meets every basis.
    pub within: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CandidatesResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<ElementId>,
    /// Its size now, and whether sizing may change it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    pub size_locked: bool,
    pub domain: Domain,
    pub shape: Shape,
    /// m³/s.
    pub flow: f64,
    pub roughness: f64,
    pub friction_target: Option<f64>,
    pub velocity_target: Option<f64>,
    pub candidates: Vec<Candidate>,
    /// "No size within tolerance" when the candidates are only the nearest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The round diameter of equal friction.
fn equivalent(s: &Size) -> f64 {
    match s.shape {
        Shape::Round => s.diameter,
        Shape::Rectangular => 1.30 * (s.width * s.height).powf(0.625) / (s.width + s.height).powf(0.25),
        Shape::Oval => section(s).1,
    }
}

/// The flow an element carries: the kept network result, else its system's design flow.
fn element_flow(store: &Store, id: ElementId) -> f64 {
    if let Some(q) = store.get(id).and_then(|e| crate::mep_network::kept_value(e, "flow")).and_then(serde_json::Value::as_f64) {
        return q;
    }
    crate::mep_systems::systems(store)
        .into_iter()
        .filter(|s| s.elements.contains(&id))
        .find_map(|s| mep_sizing::segment_flows(store, s.id).ok()?.get(&id).copied())
        .unwrap_or(0.0)
}

/// `sizing_calculator.candidates`.
pub(crate) fn candidates(store: &Store, p: CandidatesParams) -> Result<CandidatesResult> {
    let mut element = None;
    let mut current = None;
    let mut size_locked = false;
    let mut flow = 0.0;
    let mut temperature = p.fluid_temperature;
    let (domain, shape, ty) = match &p.element_id {
        Some(r) => {
            let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such segment"))?;
            let s = store
                .get(id)
                .and_then(mep_runs::segment_recipe)
                .filter(|s| matches!(s.domain, Domain::Duct | Domain::Pipe))
                .ok_or_else(|| bad("sizing_calculator.not_a_segment", format!("element {id} is not a duct or pipe segment")))?;
            element = Some(id);
            current = Some(s.size.text());
            size_locked = s.size_locked;
            flow = element_flow(store, id);
            if temperature.is_none() && s.domain == Domain::Pipe {
                temperature = crate::mep_systems::systems(store)
                    .into_iter()
                    .find(|x| x.elements.contains(&id))
                    .and_then(|x| crate::mep_systems::system_type(store, &x.system_type))
                    .and_then(|t| t.fluid_temperature);
            }
            let ty = GlobalId::parse(&s.segment_type).and_then(|g| store.find_by_guid(&g)).and_then(|t| store.get(t)).and_then(mep_runs::segment_type);
            (s.domain, s.size.shape, ty)
        }
        None => (p.domain.unwrap_or(Domain::Duct), p.shape.unwrap_or(Shape::Round), None),
    };
    if !matches!(domain, Domain::Duct | Domain::Pipe) {
        return Err(bad("sizing_calculator.domain", "the calculator sizes ducts and pipes"));
    }
    if domain == Domain::Pipe && shape != Shape::Round {
        return Err(bad("sizing_calculator.shape", "pipes are round"));
    }
    if let Some(q) = p.flow {
        flow = q;
    }
    if !(flow.is_finite() && flow > 0.0) {
        return Err(bad("sizing_calculator.flow", "give a flow above zero (the element carries none)"));
    }
    let ty = match ty {
        Some(t) => t,
        None => serde_json::from_value(serde_json::json!({
            "kind": "recipe.mep_segment_type", "domain": domain, "shape": shape,
            "roughness": if domain == Domain::Pipe { 1.5e-6 } else { 9.0e-5 },
        }))
        .expect("a type"),
    };
    let roughness = p.roughness.unwrap_or(ty.roughness);
    if !(roughness.is_finite() && roughness >= 0.0) {
        return Err(bad("sizing_calculator.roughness", "roughness is a length of zero or more"));
    }
    for b in [p.friction, p.velocity].into_iter().flatten() {
        if !(0.0..=0.5).contains(&b.tolerance) {
            return Err(bad("sizing_calculator.tolerance", "a tolerance is 0 to 50 %"));
        }
        if b.target.is_some_and(|t| !(t.is_finite() && t > 0.0)) {
            return Err(bad("sizing_calculator.target", "a target is above zero"));
        }
    }
    let pipe = domain == Domain::Pipe;
    // With no basis, friction.
    let friction = p.friction.or((p.velocity.is_none() && p.aspect.is_none()).then_some(Band { target: None, tolerance: 0.1 }));
    let ft = friction.map(|b| b.target.unwrap_or(if pipe { 300.0 } else { 1.0 }));
    let vt = p.velocity.map(|b| b.target.unwrap_or(if pipe { 1.5 } else { 5.0 }));
    let fluid = if pipe { water(temperature.unwrap_or(20.0))? } else { crate::mep_mech_settings::air(store) };
    let equation = p.equation.unwrap_or_else(|| crate::mep_mech_settings::equation(store, !pipe));
    let aspect_max = p.aspect.filter(|_| shape != Shape::Round).map(|a| a.max);
    if aspect_max.is_some_and(|m| m.is_nan() || m < 1.0) {
        return Err(bad("sizing_calculator.aspect", "the aspect ratio is 1 or more (3 for 1:3)"));
    }

    // The table's sizes, one way round, within the restrictions.
    let mut sizes: Vec<Size> = Vec::new();
    for mut z in mep_sizing::candidates(store, &ty, shape, &Restrict::default()) {
        if shape != Shape::Round {
            let (long, short) = (z.width.max(z.height), z.width.min(z.height));
            (z.width, z.height) = if p.swap { (short, long) } else { (long, short) };
        }
        let r = &p.restrict;
        let fits = match shape {
            Shape::Round => r.inner_diameter.is_none_or(|m| z.diameter <= m + 1e-9),
            _ => r.width.is_none_or(|m| z.width <= m + 1e-9) && r.height.is_none_or(|m| z.height <= m + 1e-9),
        };
        if fits && !sizes.contains(&z) {
            sizes.push(z);
        }
    }
    let mut rows: Vec<(Candidate, f64)> = sizes
        .into_iter()
        .map(|z| {
            let (v, f) = flow_in(flow, &z, roughness, fluid, equation);
            let aspect = (shape != Shape::Round).then(|| z.width.max(z.height) / z.width.min(z.height));
            let fd = ft.map(|t| (f - t) / t);
            let vd = vt.map(|t| (v - t) / t);
            // How far outside the bands (0 when within).
            let mut miss = 0.0;
            if let (Some(d), Some(b)) = (fd, friction) {
                miss += (d.abs() - b.tolerance).max(0.0);
            }
            if let (Some(d), Some(b)) = (vd, p.velocity) {
                miss += (d.abs() - b.tolerance).max(0.0);
            }
            if let (Some(a), Some(m)) = (aspect, aspect_max) {
                miss += ((a - m) / m).max(0.0);
            }
            let c = Candidate {
                size: z.text(),
                shape,
                diameter: z.diameter,
                width: z.width,
                height: z.height,
                friction: f,
                velocity: v,
                aspect,
                equivalent_diameter: equivalent(&z),
                friction_deviation: fd,
                velocity_deviation: vd,
                within: miss <= 1e-12,
            };
            (c, miss)
        })
        .collect();
    let closeness = |c: &Candidate| c.friction_deviation.or(c.velocity_deviation).map_or(0.0, f64::abs);
    let any = rows.iter().any(|(c, _)| c.within);
    let note = if any {
        rows.retain(|(c, _)| c.within);
        rows.sort_by(|a, b| closeness(&a.0).total_cmp(&closeness(&b.0)));
        None
    } else {
        rows.sort_by(|a, b| a.1.total_cmp(&b.1).then(closeness(&a.0).total_cmp(&closeness(&b.0))));
        rows.truncate(3);
        Some("No size within tolerance".to_owned())
    };
    Ok(CandidatesResult {
        element,
        current,
        size_locked,
        domain,
        shape,
        flow,
        roughness,
        friction_target: ft,
        velocity_target: vt,
        candidates: rows.into_iter().map(|r| r.0).collect(),
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huebscher_equivalent_of_a_square() {
        // A 300 mm square: about 328 mm round.
        let s = Size { shape: Shape::Rectangular, diameter: 0.0, width: 0.3, height: 0.3 };
        assert!((equivalent(&s) - 0.328).abs() < 0.001, "{}", equivalent(&s));
    }
}
