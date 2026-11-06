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
//! The alignment also gets (ARC-ALN-002): a placement at the origin and an `Axis` / `Curve2D`
//! representation, an `IfcCompositeCurve` of `IfcCurveSegment`s (an `IfcLine` or an `IfcCircle`
//! cut from its start, placed at the segment's start point and direction, a clockwise arc's
//! length negative); an `IfcReferent` (STATION) at its start with `Pset_Stationing.Station`; and
//! aggregation in the project.

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
];

/// Removes a representation tree written for an alignment.
fn remove_tree(ifc: &mut IfcModel, root: StepId) -> Result<()> {
    let mut stack = vec![root];
    let mut seen = Vec::new();
    while let Some(id) = stack.pop() {
        if seen.contains(&id) {
            continue;
        }
        let Ok(class) = ifc.class_of(id) else { continue };
        if !GEOMETRY.contains(&class.to_ascii_uppercase().as_str()) {
            continue;
        }
        seen.push(id);
        let e = ifc.attributes_many(&[id])?.remove(0);
        for v in e.attributes.values() {
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
    for class in ["IfcRelNests", "IfcRelAggregates", "IfcRelDefinesByProperties"] {
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
        let (start, dir, radius, length, kind) = match p {
            Piece::Line { start, end, length } => (*start, (end[1] - start[1]).atan2(end[0] - start[0]), 0.0, *length, "LINE"),
            Piece::Arc { start, centre, radius, left, length, .. } => {
                let a0 = (start[1] - centre[1]).atan2(start[0] - centre[0]);
                let dir = if *left { a0 + std::f64::consts::FRAC_PI_2 } else { a0 - std::f64::consts::FRAC_PI_2 };
                (*start, dir, if *left { *radius } else { -*radius }, *length, "CIRCULARARC")
            }
        };
        let point = create(ifc, "IfcCartesianPoint", &[("Coordinates", AttrValue::List(vec![AttrValue::Real(start[0] / scale), AttrValue::Real(start[1] / scale)]))])?;
        let params = create(
            ifc,
            "IfcAlignmentHorizontalSegment",
            &[
                ("StartPoint", r(point)),
                ("StartDirection", AttrValue::Real(dir)),
                ("StartRadiusOfCurvature", AttrValue::Real(radius / scale)),
                ("EndRadiusOfCurvature", AttrValue::Real(radius / scale)),
                ("SegmentLength", AttrValue::Real(length / scale)),
                ("PredefinedType", e(kind)),
            ],
        )?;
        let seg = rooted(ifc, "IfcAlignmentSegment", owner_history, &format!("H{}", k + 1))?;
        ifc.set_attribute(seg, "DesignParameters", &r(params))?;
        segments.push(seg);
    }
    nest(ifc, owner_history, layout, segments)?;
    Ok(layout)
}

/// The vertical layout's segments: constant gradients between the vertical curves, which are
/// parabolic arcs (their radius the curve's length over its change of grade).
fn vertical(ifc: &mut IfcModel, a: &Alignment, owner_history: Option<StepId>, scale: f64) -> Result<Option<StepId>> {
    let profile = &a.recipe.profile;
    if profile.len() < 2 {
        return Ok(None);
    }
    // Where each stretch starts and ends along the profile, and whether it is a curve.
    let mut stretches: Vec<(f64, f64, bool)> = Vec::new();
    let mut from = profile[0].station;
    for (i, pvi) in profile.iter().enumerate().skip(1) {
        let l = if i + 1 < profile.len() { pvi.curve.unwrap_or(0.0) } else { 0.0 };
        let bvc = pvi.station - l / 2.0;
        if bvc > from + 1e-9 {
            stretches.push((from, bvc, false));
        }
        if l > 0.0 {
            stretches.push((bvc, bvc + l, true));
            from = bvc + l;
        } else {
            from = pvi.station;
        }
    }
    let layout = rooted(ifc, "IfcAlignmentVertical", owner_history, "Vertical")?;
    let mut segments = Vec::new();
    for (k, (s0, s1, curve)) in stretches.into_iter().enumerate() {
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
fn representation(ifc: &mut IfcModel, a: &Alignment, context: StepId, scale: f64) -> Result<StepId> {
    let n = a.pieces.len();
    let mut segments = Vec::new();
    for (k, p) in a.pieces.iter().enumerate() {
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
        };
        let loc = point2(ifc, start, scale)?;
        let refd = dir2(ifc, dir)?;
        let placement = create(ifc, "IfcAxis2Placement2D", &[("Location", r(loc)), ("RefDirection", r(refd))])?;
        let transition = if k + 1 == n { "DISCONTINUOUS" } else { "CONTSAMEGRADIENT" };
        let seg = create(
            ifc,
            "IfcCurveSegment",
            &[("Transition", e(transition)), ("Placement", r(placement)), ("SegmentStart", length(0.0)), ("SegmentLength", length(len / scale)), ("ParentCurve", r(parent))],
        )?;
        segments.push(seg);
    }
    let curve = create(ifc, "IfcCompositeCurve", &[("Segments", AttrValue::List(segments.into_iter().map(r).collect())), ("SelfIntersect", AttrValue::Bool(false))])?;
    let rep = create(
        ifc,
        "IfcShapeRepresentation",
        &[("ContextOfItems", r(context)), ("RepresentationIdentifier", s("Axis")), ("RepresentationType", s("Curve2D")), ("Items", AttrValue::List(vec![r(curve)]))],
    )?;
    create(ifc, "IfcProductDefinitionShape", &[("Representations", AttrValue::List(vec![r(rep)]))])
}

/// A placement at the origin, for an alignment that has none.
fn origin(ifc: &mut IfcModel) -> Result<StepId> {
    let reals = |v: &[f64]| AttrValue::List(v.iter().copied().map(AttrValue::Real).collect());
    let o = create(ifc, "IfcCartesianPoint", &[("Coordinates", reals(&[0.0, 0.0, 0.0]))])?;
    let a = create(ifc, "IfcAxis2Placement3D", &[("Location", r(o))])?;
    create(ifc, "IfcLocalPlacement", &[("RelativePlacement", r(a))])
}

/// The alignment's start chainage: a STATION referent with `Pset_Stationing.Station`.
fn stationing(ifc: &mut IfcModel, a: &Alignment, owner_history: Option<StepId>, scale: f64) -> Result<StepId> {
    let referent = rooted(ifc, "IfcReferent", owner_history, "Start")?;
    ifc.set_attribute(referent, "PredefinedType", &e("STATION"))?;
    let value = AttrValue::Typed { type_name: "IfcLengthMeasure".into(), value: Box::new(AttrValue::Real(a.recipe.start_station / scale)) };
    let prop = create(ifc, "IfcPropertySingleValue", &[("Name", s("Station")), ("NominalValue", value)])?;
    let set = rooted(ifc, "IfcPropertySet", owner_history, "Pset_Stationing")?;
    ifc.set_attribute(set, "HasProperties", &AttrValue::List(vec![r(prop)]))?;
    let rel = rooted(ifc, "IfcRelDefinesByProperties", owner_history, "Stationing")?;
    ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(vec![r(referent)]))?;
    ifc.set_attribute(rel, "RelatingPropertyDefinition", &r(set))?;
    Ok(referent)
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
        let referent = stationing(ifc, &a, owner_history, scale)?;
        nest(ifc, owner_history, step, vec![referent])?;
        // Its geometry, replaced at every save.
        if let AttrValue::Ref(old) = ifc.attribute(step, "Representation")? {
            remove_tree(ifc, StepId(old))?;
        }
        let shape = representation(ifc, &a, context, scale)?;
        ifc.set_attribute(step, "Representation", &r(shape))?;
        if ifc.attribute(step, "ObjectPlacement")? == AttrValue::Null {
            let placement = origin(ifc)?;
            ifc.set_attribute(step, "ObjectPlacement", &r(placement))?;
        }
    }
    // In the project.
    if !native.is_empty()
        && let Some(project) = ifc.entities_of_type("IfcProject")?.first().map(|p| p.id)
    {
        let rel = rooted(ifc, "IfcRelAggregates", owner_history, "Alignments")?;
        ifc.set_attribute(rel, "RelatingObject", &r(project))?;
        ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(native.into_iter().map(r).collect()))?;
    }
    Ok(())
}
