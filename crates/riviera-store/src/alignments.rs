// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Alignments in IFC 4.3 (ARC-ALN-002). In an IFC4X3 project an alignment (ARC-ALN-001) is written
//! as an `IfcAlignment`, not an `IfcAnnotation`, with its layouts nested in it: an
//! `IfcAlignmentHorizontal` of `IfcAlignmentSegment`s whose design parameters are
//! `IfcAlignmentHorizontalSegment`s (lines and circular arcs), and, with a profile, an
//! `IfcAlignmentVertical` of `IfcAlignmentVerticalSegment`s (constant gradients and parabolic
//! arcs). The layouts are derived from the recipe each time the model is saved, and are not read
//! back as elements of their own.
//!
//! The alignment also gets (ARC-ALN-002): a placement at the origin and an `Axis` (with a
//! profile, `FootPrint`) / `Curve2D` representation, an `IfcCompositeCurve` of `IfcCurveSegment`s
//! (an `IfcLine`, `IfcCircle` or `IfcClothoid`, its point at `SegmentStart` and the tangent there
//! carried to the segment's start point and direction, a clockwise arc's length negative); with
//! a profile, an `Axis` / `Curve3D` `IfcGradientCurve` over it; an `IfcReferent` (STATION) at its
//! start with `Pset_Stationing.Station`; and aggregation in the project.

use std::collections::HashMap;

use riviera_core::ElementId;
use riviera_core::alignments::{self, Alignment, Piece};
use riviera_core::{Document, GlobalId};
use riviera_ifc::{AttrValue, IfcModel, StepId};

use crate::Result;

/// The Description that marks the layouts, segments and nesting written for an alignment.
pub(crate) const TAG: &str = "rxd:alignment";

/// Whether alignments are written as `IfcAlignment` in this model.
pub(crate) fn native(ifc: &IfcModel) -> bool {
    ifc.schema().to_ascii_uppercase().starts_with("IFC4X3")
}

fn r(id: StepId) -> AttrValue {
    AttrValue::Ref(id.0)
}

fn s(v: &str) -> AttrValue {
    AttrValue::String(v.into())
}

fn e(v: &str) -> AttrValue {
    AttrValue::Enum(v.into())
}

fn create(ifc: &mut IfcModel, class: &str, attrs: &[(&str, AttrValue)]) -> Result<StepId> {
    let id = ifc.create(class)?;
    for (name, value) in attrs {
        ifc.set_attribute(id, name, value)?;
    }
    Ok(id)
}

fn rooted(ifc: &mut IfcModel, class: &str, owner_history: Option<StepId>, name: &str) -> Result<StepId> {
    let id = ifc.create_rooted(class, &GlobalId::new())?;
    if let Some(oh) = owner_history {
        ifc.set_attribute(id, "OwnerHistory", &r(oh))?;
    }
    ifc.set_attribute(id, "Name", &s(name))?;
    ifc.set_attribute(id, "Description", &s(TAG))?;
    Ok(id)
}

fn ours(ifc: &IfcModel, id: StepId) -> bool {
    matches!(ifc.attribute(id, "Description"), Ok(AttrValue::String(d)) if d == TAG)
}

/// The geometry classes an alignment's representation is built from: removed with it.
const GEOMETRY: &[&str] = &[
    "IFCPRODUCTDEFINITIONSHAPE",
    "IFCSHAPEREPRESENTATION",
    "IFCCOMPOSITECURVE",
    "IFCCURVESEGMENT",
    "IFCAXIS2PLACEMENT2D",
    "IFCCARTESIANPOINT",
    "IFCDIRECTION",
    "IFCLINE",
    "IFCVECTOR",
    "IFCCIRCLE",
    "IFCCLOTHOID",
    "IFCGRADIENTCURVE",
    "IFCPOLYNOMIALCURVE",
];

/// The classes of the placements written for referents and facilities: removed with them.
const PLACEMENT: &[&str] = &[
    "IFCLOCALPLACEMENT",
    "IFCLINEARPLACEMENT",
    "IFCAXIS2PLACEMENTLINEAR",
    "IFCPOINTBYDISTANCEEXPRESSION",
    "IFCAXIS2PLACEMENT3D",
    "IFCCARTESIANPOINT",
    "IFCDIRECTION",
];

/// Removes a representation tree written for an alignment.
fn remove_tree(ifc: &mut IfcModel, root: StepId) -> Result<()> {
    remove_from(ifc, root, GEOMETRY)
}

/// Removes the tree of `classes` under `root`, not following what it is placed relative to or
/// measured along.
fn remove_from(ifc: &mut IfcModel, root: StepId, classes: &[&str]) -> Result<()> {
    let mut stack = vec![root];
    let mut seen = Vec::new();
    while let Some(id) = stack.pop() {
        if seen.contains(&id) {
            continue;
        }
        let Ok(class) = ifc.class_of(id) else { continue };
        if !classes.contains(&class.to_ascii_uppercase().as_str()) {
            continue;
        }
        seen.push(id);
        let e = ifc.attributes_many(&[id])?.remove(0);
        for (name, v) in &e.attributes {
            if name == "PlacementRelTo" || name == "BasisCurve" {
                continue;
            }
            match v {
                AttrValue::Ref(r) => stack.push(StepId(*r)),
                AttrValue::List(items) => stack.extend(items.iter().filter_map(|i| if let AttrValue::Ref(r) = i { Some(StepId(*r)) } else { None })),
                _ => {}
            }
        }
    }
    for id in seen {
        let _ = ifc.remove(id);
    }
    Ok(())
}

/// Removes what an earlier save wrote: the nesting, aggregation and stationing, the layouts and
/// segments, and their design parameters.
fn remove_earlier(ifc: &mut IfcModel) -> Result<()> {
    for class in ["IfcRailway", "IfcFacility"] {
        for f in ifc.entities_of_type(class)? {
            if !ours(ifc, f.id) || !ifc.class_of(f.id)?.eq_ignore_ascii_case(class) {
                continue;
            }
            if let AttrValue::Ref(p) = ifc.attribute(f.id, "ObjectPlacement")? {
                remove_from(ifc, StepId(p), PLACEMENT)?;
            }
            ifc.remove(f.id)?;
        }
    }
    for class in ["IfcRelNests", "IfcRelAggregates", "IfcRelDefinesByProperties", "IfcRelReferencedInSpatialStructure"] {
        for rel in ifc.entities_of_type(class)? {
            if ours(ifc, rel.id) {
                ifc.remove(rel.id)?;
            }
        }
    }
    for set in ifc.entities_of_type("IfcPropertySet")? {
        if !ours(ifc, set.id) {
            continue;
        }
        if let AttrValue::List(props) = ifc.attribute(set.id, "HasProperties")? {
            for p in props {
                if let AttrValue::Ref(p) = p {
                    let _ = ifc.remove(StepId(p));
                }
            }
        }
        ifc.remove(set.id)?;
    }
    for referent in ifc.entities_of_type("IfcReferent")? {
        if ours(ifc, referent.id) {
            if let AttrValue::Ref(p) = ifc.attribute(referent.id, "ObjectPlacement")? {
                remove_from(ifc, StepId(p), PLACEMENT)?;
            }
            ifc.remove(referent.id)?;
        }
    }
    for seg in ifc.entities_of_type("IfcAlignmentSegment")? {
        if !ours(ifc, seg.id) {
            continue;
        }
        if let AttrValue::Ref(p) = ifc.attribute(seg.id, "DesignParameters")? {
            if let Ok(AttrValue::Ref(pt)) = ifc.attribute(StepId(p), "StartPoint") {
                let _ = ifc.remove(StepId(pt));
            }
            let _ = ifc.remove(StepId(p));
        }
        ifc.remove(seg.id)?;
    }
    for class in ["IfcAlignmentHorizontal", "IfcAlignmentVertical"] {
        for layout in ifc.entities_of_type(class)? {
            if ours(ifc, layout.id) {
                ifc.remove(layout.id)?;
            }
        }
    }
    Ok(())
}

fn nest(ifc: &mut IfcModel, owner_history: Option<StepId>, host: StepId, parts: Vec<StepId>) -> Result<()> {
    if parts.is_empty() {
        return Ok(());
    }
    let rel = ifc.create_rooted("IfcRelNests", &GlobalId::new())?;
    if let Some(oh) = owner_history {
        ifc.set_attribute(rel, "OwnerHistory", &r(oh))?;
    }
    ifc.set_attribute(rel, "Description", &s(TAG))?;
    ifc.set_attribute(rel, "RelatingObject", &r(host))?;
    ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(parts.into_iter().map(r).collect()))?;
    Ok(())
}

/// The horizontal layout's segments: lines and circular arcs (radius positive turning left).
fn horizontal(ifc: &mut IfcModel, a: &Alignment, owner_history: Option<StepId>, scale: f64) -> Result<StepId> {
    let layout = rooted(ifc, "IfcAlignmentHorizontal", owner_history, "Horizontal")?;
    let mut segments = Vec::new();
    for (k, p) in a.pieces.iter().enumerate() {
        let (start, dir, (r0, r1), length, kind) = match p {
            Piece::Line { start, end, length } => (*start, (end[1] - start[1]).atan2(end[0] - start[0]), (0.0, 0.0), *length, "LINE"),
            // A spiral's radii (0: a straight end), positive turning left.
            Piece::Spiral { start, heading, k0, k1, length, .. } => {
                let inv = |k: f64| if k.abs() < 1e-12 { 0.0 } else { 1.0 / k };
                (*start, *heading, (inv(*k0), inv(*k1)), *length, "CLOTHOID")
            }
            Piece::Arc { start, centre, radius, left, length, .. } => {
                let a0 = (start[1] - centre[1]).atan2(start[0] - centre[0]);
                let dir = if *left { a0 + std::f64::consts::FRAC_PI_2 } else { a0 - std::f64::consts::FRAC_PI_2 };
                (*start, dir, if *left { (*radius, *radius) } else { (-*radius, -*radius) }, *length, "CIRCULARARC")
            }
        };
        let point = create(ifc, "IfcCartesianPoint", &[("Coordinates", AttrValue::List(vec![AttrValue::Real(start[0] / scale), AttrValue::Real(start[1] / scale)]))])?;
        let params = create(
            ifc,
            "IfcAlignmentHorizontalSegment",
            &[
                ("StartPoint", r(point)),
                ("StartDirection", AttrValue::Real(dir)),
                ("StartRadiusOfCurvature", AttrValue::Real(r0 / scale)),
                ("EndRadiusOfCurvature", AttrValue::Real(r1 / scale)),
                ("SegmentLength", AttrValue::Real(length / scale)),
                ("PredefinedType", e(kind)),
            ],
        )?;
        let seg = rooted(ifc, "IfcAlignmentSegment", owner_history, &format!("H{}", k + 1))?;
        ifc.set_attribute(seg, "DesignParameters", &r(params))?;
        segments.push(seg);
    }
    // The zero-length segment that ends the list (concept template "Alignment Geometry").
    let (end, d, _, _) = a.at(a.end_station());
    let point = create(ifc, "IfcCartesianPoint", &[("Coordinates", AttrValue::List(vec![AttrValue::Real(end[0] / scale), AttrValue::Real(end[1] / scale)]))])?;
    let params = create(
        ifc,
        "IfcAlignmentHorizontalSegment",
        &[
            ("StartPoint", r(point)),
            ("StartDirection", AttrValue::Real(d[1].atan2(d[0]))),
            ("StartRadiusOfCurvature", AttrValue::Real(0.0)),
            ("EndRadiusOfCurvature", AttrValue::Real(0.0)),
            ("SegmentLength", AttrValue::Real(0.0)),
            ("PredefinedType", e("LINE")),
        ],
    )?;
    let seg = rooted(ifc, "IfcAlignmentSegment", owner_history, "End")?;
    ifc.set_attribute(seg, "DesignParameters", &r(params))?;
    segments.push(seg);
    nest(ifc, owner_history, layout, segments)?;
    Ok(layout)
}

/// Where each stretch of an alignment's profile starts and ends, and whether it is a vertical
/// curve; cut to the plan, which the layouts and the gradient curve may not run past.
fn stretches(a: &Alignment) -> Vec<(f64, f64, bool)> {
    let (from, to) = (a.recipe.start_station, a.end_station());
    profile_stretches(&a.recipe.profile)
        .into_iter()
        .filter_map(|(s0, s1, c)| {
            let (s0, s1) = (s0.max(from), s1.min(to));
            (s1 > s0 + 1e-9).then_some((s0, s1, c))
        })
        .collect()
}

/// Where the profile's last stretch ends (at the latest, the plan's end).
fn profile_end(a: &Alignment) -> f64 {
    stretches(a).last().map_or(a.recipe.start_station, |s| s.1)
}

fn profile_stretches(profile: &[alignments::Pvi]) -> Vec<(f64, f64, bool)> {
    let mut out = Vec::new();
    let mut from = profile[0].station;
    for (i, pvi) in profile.iter().enumerate().skip(1) {
        let l = if i + 1 < profile.len() { pvi.curve.unwrap_or(0.0) } else { 0.0 };
        let bvc = pvi.station - l / 2.0;
        if bvc > from + 1e-9 {
            out.push((from, bvc, false));
        }
        if l > 0.0 {
            out.push((bvc, bvc + l, true));
            from = bvc + l;
        } else {
            from = pvi.station;
        }
    }
    out
}

/// The vertical layout's segments: constant gradients between the vertical curves, which are
/// parabolic arcs (their radius the curve's length over its change of grade).
fn vertical(ifc: &mut IfcModel, a: &Alignment, owner_history: Option<StepId>, scale: f64) -> Result<Option<StepId>> {
    let profile = &a.recipe.profile;
    if profile.len() < 2 {
        return Ok(None);
    }
    let layout = rooted(ifc, "IfcAlignmentVertical", owner_history, "Vertical")?;
    let mut segments = Vec::new();
    for (k, (s0, s1, curve)) in stretches(a).into_iter().enumerate() {
        let (z0, g0) = alignments::elevation(profile, s0 + 1e-9);
        let (_, g1) = alignments::elevation(profile, s1 - 1e-9);
        let mut attrs = vec![
            // Along the horizontal layout from its start, not the chainage.
            ("StartDistAlong", AttrValue::Real((s0 - a.recipe.start_station) / scale)),
            ("HorizontalLength", AttrValue::Real((s1 - s0) / scale)),
            ("StartHeight", AttrValue::Real(z0 / scale)),
            ("StartGradient", AttrValue::Real(g0)),
            ("EndGradient", AttrValue::Real(if curve { g1 } else { g0 })),
        ];
        if curve && (g1 - g0).abs() > 1e-12 {
            attrs.push(("RadiusOfCurvature", AttrValue::Real((s1 - s0) / (g1 - g0) / scale)));
        }
        attrs.push(("PredefinedType", e(if curve { "PARABOLICARC" } else { "CONSTANTGRADIENT" })));
        let params = create(ifc, "IfcAlignmentVerticalSegment", &attrs)?;
        let seg = rooted(ifc, "IfcAlignmentSegment", owner_history, &format!("V{}", k + 1))?;
        ifc.set_attribute(seg, "DesignParameters", &r(params))?;
        segments.push(seg);
    }
    // The zero-length segment that ends the list, where the profile or the plan ends.
    let end = profile_end(a);
    let last = (end - a.recipe.start_station).max(0.0);
    let (z, g) = alignments::elevation(profile, end);
    let params = create(
        ifc,
        "IfcAlignmentVerticalSegment",
        &[
            ("StartDistAlong", AttrValue::Real(last / scale)),
            ("HorizontalLength", AttrValue::Real(0.0)),
            ("StartHeight", AttrValue::Real(z / scale)),
            ("StartGradient", AttrValue::Real(g)),
            ("EndGradient", AttrValue::Real(g)),
            ("PredefinedType", e("CONSTANTGRADIENT")),
        ],
    )?;
    let seg = rooted(ifc, "IfcAlignmentSegment", owner_history, "End")?;
    ifc.set_attribute(seg, "DesignParameters", &r(params))?;
    segments.push(seg);
    nest(ifc, owner_history, layout, segments)?;
    Ok(Some(layout))
}

fn point2(ifc: &mut IfcModel, p: [f64; 2], scale: f64) -> Result<StepId> {
    create(ifc, "IfcCartesianPoint", &[("Coordinates", AttrValue::List(vec![AttrValue::Real(p[0] / scale), AttrValue::Real(p[1] / scale)]))])
}

fn dir2(ifc: &mut IfcModel, d: [f64; 2]) -> Result<StepId> {
    create(ifc, "IfcDirection", &[("DirectionRatios", AttrValue::List(vec![AttrValue::Real(d[0]), AttrValue::Real(d[1])]))])
}

fn length(v: f64) -> AttrValue {
    AttrValue::Typed { type_name: "IfcLengthMeasure".into(), value: Box::new(AttrValue::Real(v)) }
}

/// The alignment's horizontal geometry: a composite curve of line and arc segments.
/// Also returns the axis curve: the gradient curve with a profile, the plan curve without.
fn representation(ifc: &mut IfcModel, a: &Alignment, context: StepId, scale: f64) -> Result<(StepId, StepId)> {
    let mut segments = Vec::new();
    for p in &a.pieces {
        // Where along its parent curve the segment starts (a placement carries that point and
        // its tangent to the segment's start).
        let mut cut_from = 0.0;
        let (start, dir, parent, len) = match p {
            Piece::Line { start, end, length } => {
                let d = [(end[0] - start[0]) / length, (end[1] - start[1]) / length];
                let o = point2(ifc, [0.0, 0.0], 1.0)?;
                let x = dir2(ifc, [1.0, 0.0])?;
                let v = create(ifc, "IfcVector", &[("Orientation", r(x)), ("Magnitude", AttrValue::Real(1.0))])?;
                let line = create(ifc, "IfcLine", &[("Pnt", r(o)), ("Dir", r(v))])?;
                (*start, d, line, *length)
            }
            Piece::Arc { start, centre, radius, left, length, .. } => {
                let a0 = (start[1] - centre[1]).atan2(start[0] - centre[0]);
                let d = if *left { [-a0.sin(), a0.cos()] } else { [a0.sin(), -a0.cos()] };
                let o = point2(ifc, [0.0, 0.0], 1.0)?;
                let x = dir2(ifc, [1.0, 0.0])?;
                let pos = create(ifc, "IfcAxis2Placement2D", &[("Location", r(o)), ("RefDirection", r(x))])?;
                let circle = create(ifc, "IfcCircle", &[("Position", r(pos)), ("Radius", AttrValue::Real(radius / scale))])?;
                // Anticlockwise along the circle; a clockwise arc runs against it.
                (*start, d, circle, if *left { *length } else { -*length })
            }
            // A clothoid at the origin, its curvature s / (A·|A|) at s from its inflection point,
            // cut where the curvature is k0 (bSI sample "linear-placement-of-signal": an exit
            // spiral starts at −L with A negative turning left).
            Piece::Spiral { start, heading, k0, k1, length, .. } => {
                let c = length / (k1 - k0);
                let o = point2(ifc, [0.0, 0.0], 1.0)?;
                let x = dir2(ifc, [1.0, 0.0])?;
                let pos = create(ifc, "IfcAxis2Placement2D", &[("Location", r(o)), ("RefDirection", r(x))])?;
                let clothoid = create(ifc, "IfcClothoid", &[("Position", r(pos)), ("ClothoidConstant", AttrValue::Real(c.signum() * c.abs().sqrt() / scale))])?;
                cut_from = k0 * c;
                (*start, [heading.cos(), heading.sin()], clothoid, *length)
            }
        };
        let loc = point2(ifc, start, scale)?;
        let refd = dir2(ifc, dir)?;
        let placement = create(ifc, "IfcAxis2Placement2D", &[("Location", r(loc)), ("RefDirection", r(refd))])?;
        let transition = "CONTSAMEGRADIENT";
        let seg = create(
            ifc,
            "IfcCurveSegment",
            &[("Transition", e(transition)), ("Placement", r(placement)), ("SegmentStart", length(cut_from / scale)), ("SegmentLength", length(len / scale)), ("ParentCurve", r(parent))],
        )?;
        segments.push(seg);
    }
    // The zero-length segment that ends it, at the end point.
    {
        let (end, d, _, _) = a.at(a.end_station());
        let o = point2(ifc, [0.0, 0.0], 1.0)?;
        let x = dir2(ifc, [1.0, 0.0])?;
        let v = create(ifc, "IfcVector", &[("Orientation", r(x)), ("Magnitude", AttrValue::Real(1.0))])?;
        let line = create(ifc, "IfcLine", &[("Pnt", r(o)), ("Dir", r(v))])?;
        let loc = point2(ifc, end, scale)?;
        let refd = dir2(ifc, d)?;
        let placement = create(ifc, "IfcAxis2Placement2D", &[("Location", r(loc)), ("RefDirection", r(refd))])?;
        segments.push(create(
            ifc,
            "IfcCurveSegment",
            &[("Transition", e("DISCONTINUOUS")), ("Placement", r(placement)), ("SegmentStart", length(0.0)), ("SegmentLength", length(0.0)), ("ParentCurve", r(line))],
        )?);
    }
    let curve = create(ifc, "IfcCompositeCurve", &[("Segments", AttrValue::List(segments.into_iter().map(r).collect())), ("SelfIntersect", AttrValue::Bool(false))])?;
    let profiled = a.recipe.profile.len() >= 2;
    let rep = create(
        ifc,
        "IfcShapeRepresentation",
        // With a vertical layout the plan curve is the footprint and the 3D gradient curve the
        // axis (concept template "Alignment Geometry - Horizontal and Vertical").
        &[("ContextOfItems", r(context)), ("RepresentationIdentifier", s(if profiled { "FootPrint" } else { "Axis" })), ("RepresentationType", s("Curve2D")), ("Items", AttrValue::List(vec![r(curve)]))],
    )?;
    let mut reps = vec![r(rep)];
    let mut axis = curve;
    if profiled {
        let gradient = gradient_curve(ifc, a, curve, scale)?;
        axis = gradient;
        reps.push(r(create(
            ifc,
            "IfcShapeRepresentation",
            &[("ContextOfItems", r(context)), ("RepresentationIdentifier", s("Axis")), ("RepresentationType", s("Curve3D")), ("Items", AttrValue::List(vec![r(gradient)]))],
        )?));
    }
    Ok((create(ifc, "IfcProductDefinitionShape", &[("Representations", AttrValue::List(reps))])?, axis))
}

/// Signed arc length of y = a·x² from its vertex to x.
fn parabola_length(a: f64, x: f64) -> f64 {
    let q = 2.0 * a * x;
    x * (1.0 + q * q).sqrt() / 2.0 + q.asinh() / (4.0 * a)
}

/// The profile as an `IfcGradientCurve` over the plan curve: segments in the plane of distance
/// along the plan (from its start) and elevation, their lengths measured along them (bSI sample
/// "linear-placement-of-signal": a 250.00375 m stretch at −1 % is 250.01625 long). Gradients are
/// lines, vertical curves `IfcPolynomialCurve`s y = a·x² cut from the vertex (template
/// "Parabolic Transition Segment").
fn gradient_curve(ifc: &mut IfcModel, a: &Alignment, base: StepId, scale: f64) -> Result<StepId> {
    let profile = &a.recipe.profile;
    let line = |ifc: &mut IfcModel| -> Result<StepId> {
        let o = point2(ifc, [0.0, 0.0], 1.0)?;
        let x = dir2(ifc, [1.0, 0.0])?;
        let v = create(ifc, "IfcVector", &[("Orientation", r(x)), ("Magnitude", AttrValue::Real(1.0))])?;
        create(ifc, "IfcLine", &[("Pnt", r(o)), ("Dir", r(v))])
    };
    let mut segments = Vec::new();
    for (s0, s1, curve) in stretches(a) {
        let (z0, g0) = alignments::elevation(profile, s0 + 1e-9);
        let (_, g1) = alignments::elevation(profile, s1 - 1e-9);
        let l = s1 - s0;
        let (parent, from, len) = if curve && (g1 - g0).abs() > 1e-12 {
            // In file units: y = a·x², the segment starting where its slope is g0.
            let k = (g1 - g0) / (2.0 * l) * scale;
            let x0 = g0 / (2.0 * k);
            let x1 = x0 + l / scale;
            let o = point2(ifc, [0.0, 0.0], 1.0)?;
            let x = dir2(ifc, [1.0, 0.0])?;
            let pos = create(ifc, "IfcAxis2Placement2D", &[("Location", r(o)), ("RefDirection", r(x))])?;
            let reals = |v: &[f64]| AttrValue::List(v.iter().copied().map(AttrValue::Real).collect());
            let poly = create(ifc, "IfcPolynomialCurve", &[("Position", r(pos)), ("CoefficientsX", reals(&[0.0, 1.0])), ("CoefficientsY", reals(&[0.0, 0.0, k]))])?;
            (poly, parabola_length(k, x0), parabola_length(k, x1) - parabola_length(k, x0))
        } else {
            (line(ifc)?, 0.0, l * (1.0 + g0 * g0).sqrt() / scale)
        };
        let loc = point2(ifc, [s0 - a.recipe.start_station, z0], scale)?;
        let n = (1.0 + g0 * g0).sqrt();
        let refd = dir2(ifc, [1.0 / n, g0 / n])?;
        let placement = create(ifc, "IfcAxis2Placement2D", &[("Location", r(loc)), ("RefDirection", r(refd))])?;
        segments.push(create(
            ifc,
            "IfcCurveSegment",
            &[("Transition", e("CONTSAMEGRADIENT")), ("Placement", r(placement)), ("SegmentStart", length(from)), ("SegmentLength", length(len)), ("ParentCurve", r(parent))],
        )?);
    }
    // The zero-length segment that ends it.
    let end = profile_end(a);
    let (z, g) = alignments::elevation(profile, end);
    let parent = line(ifc)?;
    let loc = point2(ifc, [(end - a.recipe.start_station).max(0.0), z], scale)?;
    let n = (1.0 + g * g).sqrt();
    let refd = dir2(ifc, [1.0 / n, g / n])?;
    let placement = create(ifc, "IfcAxis2Placement2D", &[("Location", r(loc)), ("RefDirection", r(refd))])?;
    segments.push(create(
        ifc,
        "IfcCurveSegment",
        &[("Transition", e("DISCONTINUOUS")), ("Placement", r(placement)), ("SegmentStart", length(0.0)), ("SegmentLength", length(0.0)), ("ParentCurve", r(parent))],
    )?);
    create(
        ifc,
        "IfcGradientCurve",
        &[("Segments", AttrValue::List(segments.into_iter().map(r).collect())), ("SelfIntersect", AttrValue::Bool(false)), ("BaseCurve", r(base))],
    )
}

/// A placement at the origin, for an alignment that has none.
fn origin(ifc: &mut IfcModel) -> Result<StepId> {
    let reals = |v: &[f64]| AttrValue::List(v.iter().copied().map(AttrValue::Real).collect());
    let o = create(ifc, "IfcCartesianPoint", &[("Coordinates", reals(&[0.0, 0.0, 0.0]))])?;
    let a = create(ifc, "IfcAxis2Placement3D", &[("Location", r(o))])?;
    create(ifc, "IfcLocalPlacement", &[("RelativePlacement", r(a))])
}

/// The alignment's start chainage: a STATION referent with `Pset_Stationing.Station`.
/// It is placed linearly at distance 0 along the axis curve, relative to the alignment's
/// placement, with its Cartesian position (as in bSI's sample models; IfcPositioningElement
/// requires a placement).
fn stationing(ifc: &mut IfcModel, a: &Alignment, owner_history: Option<StepId>, scale: f64, relative_to: &AttrValue, axis: StepId) -> Result<StepId> {
    let referent = rooted(ifc, "IfcReferent", owner_history, "Start")?;
    ifc.set_attribute(referent, "PredefinedType", &e("STATION"))?;
    let zero = || AttrValue::Real(0.0);
    let at = create(
        ifc,
        "IfcPointByDistanceExpression",
        &[("DistanceAlong", length(0.0)), ("OffsetLateral", zero()), ("OffsetVertical", zero()), ("OffsetLongitudinal", zero()), ("BasisCurve", r(axis))],
    )?;
    let linear = create(ifc, "IfcAxis2PlacementLinear", &[("Location", r(at))])?;
    let (p, d, _, _) = a.at(a.recipe.start_station);
    let z = if a.recipe.profile.len() >= 2 { alignments::elevation(&a.recipe.profile, a.recipe.start_station).0 } else { 0.0 };
    let reals = |v: &[f64]| AttrValue::List(v.iter().copied().map(AttrValue::Real).collect());
    let o = create(ifc, "IfcCartesianPoint", &[("Coordinates", reals(&[p[0] / scale, p[1] / scale, z / scale]))])?;
    let x = create(ifc, "IfcDirection", &[("DirectionRatios", reals(&[d[0], d[1], 0.0]))])?;
    let up = create(ifc, "IfcDirection", &[("DirectionRatios", reals(&[0.0, 0.0, 1.0]))])?;
    let cartesian = create(ifc, "IfcAxis2Placement3D", &[("Location", r(o)), ("Axis", r(up)), ("RefDirection", r(x))])?;
    let placement = create(ifc, "IfcLinearPlacement", &[("PlacementRelTo", relative_to.clone()), ("RelativePlacement", r(linear)), ("CartesianPosition", r(cartesian))])?;
    ifc.set_attribute(referent, "ObjectPlacement", &r(placement))?;
    let value = AttrValue::Typed { type_name: "IfcLengthMeasure".into(), value: Box::new(AttrValue::Real(a.recipe.start_station / scale)) };
    let prop = create(ifc, "IfcPropertySingleValue", &[("Name", s("Station")), ("NominalValue", value)])?;
    let set = rooted(ifc, "IfcPropertySet", owner_history, "Pset_Stationing")?;
    ifc.set_attribute(set, "HasProperties", &AttrValue::List(vec![r(prop)]))?;
    let rel = rooted(ifc, "IfcRelDefinesByProperties", owner_history, "Stationing")?;
    ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(vec![r(referent)]))?;
    ifc.set_attribute(rel, "RelatingPropertyDefinition", &r(set))?;
    Ok(referent)
}

/// A facility's GlobalId, fixed by the device it stands for so it survives every save.
fn facility_guid(whole: &GlobalId) -> GlobalId {
    let mut b = *whole.to_uuid().as_bytes();
    for x in &mut b[8..] {
        *x ^= 0xa5;
    }
    GlobalId::from_uuid(uuid::Uuid::from_bytes(b))
}

/// The guideway devices as IFC 4.3 facilities (ARC-ALN-002): a funicular an `IfcRailway`
/// (USERDEFINED, "Funicular"); a people mover and a ropeway an `IfcFacility` ("AutomatedPeopleMover",
/// "AerialRopeway"), since neither runs on rails in the IfcRailway sense. Each is aggregated in the
/// site and references (`IfcRelReferencedInSpatialStructure`) the device, its parts and its
/// alignment; the elements stay contained in their storeys.
fn write_facilities(ifc: &mut IfcModel, doc: &Document, steps: &HashMap<ElementId, StepId>, owner_history: Option<StepId>) -> Result<()> {
    use riviera_core::{apm, funiculars, mesh_objects, ropeways};
    let store = doc.store();
    // (whole, class, object type, alignment guid, other wholes whose parts go with it)
    let mut devices: Vec<(ElementId, &str, &str, String, Vec<ElementId>)> = Vec::new();
    for (id, f) in funiculars::funiculars(store) {
        devices.push((id, "IfcRailway", "Funicular", f.alignment, Vec::new()));
    }
    let stations = apm::stations(store);
    for (id, g) in apm::guideways(store) {
        let Some(guid) = store.get(id).map(|e| e.guid().as_str().to_owned()) else { continue };
        let own: Vec<ElementId> = stations.iter().filter(|(_, s)| s.guideway == guid).map(|(sid, _)| *sid).collect();
        devices.push((id, "IfcFacility", "AutomatedPeopleMover", g.alignment, own));
    }
    for (id, w) in ropeways::ropeways(store) {
        devices.push((id, "IfcFacility", "AerialRopeway", w.alignment, Vec::new()));
    }
    if devices.is_empty() {
        return Ok(());
    }
    let site = ifc.entities_of_type("IfcSite")?.first().map(|s| s.id);
    let Some(host) = site.or(ifc.entities_of_type("IfcProject")?.first().map(|p| p.id)) else { return Ok(()) };
    let host_placement = match site {
        Some(s) => ifc.attribute(s, "ObjectPlacement")?,
        None => AttrValue::Null,
    };
    let mut facilities = Vec::new();
    for (whole, class, kind, alignment, more) in devices {
        let Some(device) = store.get(whole) else { continue };
        let wholes: Vec<ElementId> = std::iter::once(whole).chain(more).collect();
        let guids: Vec<String> = wholes.iter().filter_map(|w| store.get(*w)).map(|w| w.guid().as_str().to_owned()).collect();
        let mut members: Vec<StepId> = wholes.iter().filter_map(|w| steps.get(w).copied()).collect();
        members.extend(
            store
                .iter()
                .filter(|p| p.param(mesh_objects::PART_OF_KEY).and_then(serde_json::Value::as_str).is_some_and(|g| guids.iter().any(|w| w == g)))
                .filter_map(|p| steps.get(&p.id()).copied()),
        );
        if let Some(a) = GlobalId::parse(&alignment).and_then(|g| store.find_by_guid(&g)).and_then(|a| steps.get(&a)) {
            members.push(*a);
        }
        let name = device.name.clone().unwrap_or_else(|| kind.to_owned());
        let id = ifc.create_rooted(class, &facility_guid(device.guid()))?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(id, "OwnerHistory", &r(oh))?;
        }
        ifc.set_attribute(id, "Name", &s(&name))?;
        ifc.set_attribute(id, "Description", &s(TAG))?;
        ifc.set_attribute(id, "ObjectType", &s(kind))?;
        ifc.set_attribute(id, "CompositionType", &e("ELEMENT"))?;
        if class == "IfcRailway" {
            ifc.set_attribute(id, "PredefinedType", &e("USERDEFINED"))?;
        }
        let o = create(ifc, "IfcCartesianPoint", &[("Coordinates", AttrValue::List(vec![AttrValue::Real(0.0); 3]))])?;
        let axis = create(ifc, "IfcAxis2Placement3D", &[("Location", r(o))])?;
        let placement = create(ifc, "IfcLocalPlacement", &[("PlacementRelTo", host_placement.clone()), ("RelativePlacement", r(axis))])?;
        ifc.set_attribute(id, "ObjectPlacement", &r(placement))?;
        if !members.is_empty() {
            let rel = rooted(ifc, "IfcRelReferencedInSpatialStructure", owner_history, &name)?;
            ifc.set_attribute(rel, "RelatedElements", &AttrValue::List(members.into_iter().map(r).collect()))?;
            ifc.set_attribute(rel, "RelatingStructure", &r(id))?;
        }
        facilities.push(id);
    }
    let rel = rooted(ifc, "IfcRelAggregates", owner_history, "Facilities")?;
    ifc.set_attribute(rel, "RelatingObject", &r(host))?;
    ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(facilities.into_iter().map(r).collect()))?;
    Ok(())
}

/// Writes every alignment's layouts (IFC4X3 projects only).
pub(crate) fn write_alignments(ifc: &mut IfcModel, doc: &Document, steps: &HashMap<ElementId, StepId>, owner_history: Option<StepId>, scale: f64) -> Result<()> {
    if !native(ifc) {
        return Ok(());
    }
    remove_earlier(ifc)?;
    let context = crate::geometry::Contexts::find(ifc)?.axis;
    let mut native = Vec::new();
    for (id, a) in alignments::alignments(doc.store()) {
        let Some(&step) = steps.get(&id) else { continue };
        if !ifc.class_of(step)?.eq_ignore_ascii_case("IfcAlignment") {
            continue;
        }
        native.push(step);
        let mut parts = vec![horizontal(ifc, &a, owner_history, scale)?];
        if let Some(v) = vertical(ifc, &a, owner_history, scale)? {
            parts.push(v);
        }
        nest(ifc, owner_history, step, parts)?;
        // Its geometry, replaced at every save.
        if let AttrValue::Ref(old) = ifc.attribute(step, "Representation")? {
            remove_tree(ifc, StepId(old))?;
        }
        let (shape, axis) = representation(ifc, &a, context, scale)?;
        ifc.set_attribute(step, "Representation", &r(shape))?;
        if ifc.attribute(step, "ObjectPlacement")? == AttrValue::Null {
            let placement = origin(ifc)?;
            ifc.set_attribute(step, "ObjectPlacement", &r(placement))?;
        }
        let placed = ifc.attribute(step, "ObjectPlacement")?;
        let referent = stationing(ifc, &a, owner_history, scale, &placed, axis)?;
        nest(ifc, owner_history, step, vec![referent])?;
    }
    // In the project.
    if !native.is_empty()
        && let Some(project) = ifc.entities_of_type("IfcProject")?.first().map(|p| p.id)
    {
        let rel = rooted(ifc, "IfcRelAggregates", owner_history, "Alignments")?;
        ifc.set_attribute(rel, "RelatingObject", &r(project))?;
        ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(native.into_iter().map(r).collect()))?;
    }
    write_facilities(ifc, doc, steps, owner_history)
}
