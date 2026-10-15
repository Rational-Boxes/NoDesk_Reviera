// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Annotations whose shape is curves: reference planes (an `Axis` representation, ARC-REFP-001)
//! and model lines (a styled `Annotation` representation, ARC-MLIN-001).
//!
//! Each is an `IfcAnnotation` placed at the world origin, with an `IfcProductDefinitionShape` of
//! one `IfcShapeRepresentation` (`Curve3D`) of `IfcPolyline`s in world coordinates. A model line's
//! polyline is styled (`IfcStyledItem` → `IfcCurveStyle` with its colour, named after its line
//! style). The shape's Description carries a signature of what was written, so an unchanged
//! annotation is not rewritten. It is contained in its level's storey when it has one.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use reviera_core::{Document, ElementId, GlobalId, model_lines, ref_planes, work_planes};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::{Result, geometry};

const TAG: &str = "rxd:curves";

fn r(id: StepId) -> AttrValue {
    AttrValue::Ref(id.0)
}

fn reals(v: &[f64]) -> AttrValue {
    AttrValue::List(v.iter().copied().map(AttrValue::Real).collect())
}

fn create(ifc: &mut IfcModel, class: &str, attrs: &[(&str, AttrValue)]) -> Result<StepId> {
    let id = ifc.create(class)?;
    for (name, value) in attrs {
        ifc.set_attribute(id, name, value)?;
    }
    Ok(id)
}

/// What an annotation's shape is.
struct Curves<'a> {
    /// `Axis` or `Annotation`.
    identifier: &'a str,
    polylines: Vec<Vec<[f64; 3]>>,
    /// A curve style: name and colour (0–1).
    style: Option<(&'a str, [f64; 3])>,
}

impl Curves<'_> {
    fn signature(&self, scale: f64) -> String {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.identifier.hash(&mut h);
        for p in self.polylines.iter().flatten() {
            for v in p {
                ((v / scale) * 1e6).round().to_bits().hash(&mut h);
            }
        }
        if let Some((name, c)) = self.style {
            name.hash(&mut h);
            for v in c {
                (v * 1e6).round().to_bits().hash(&mut h);
            }
        }
        format!("{TAG}:{:016x}", h.finish())
    }
}

fn written_signature(ifc: &IfcModel, element: StepId) -> Result<Option<String>> {
    let AttrValue::Ref(shape) = ifc.attribute(element, "Representation")? else {
        return Ok(None);
    };
    Ok(match ifc.attribute(StepId(shape), "Description")? {
        AttrValue::String(s) if s.starts_with(TAG) => Some(s),
        _ => None,
    })
}

/// Removes an annotation's placement and shape written by [`write`], with their styles.
pub(crate) fn remove(ifc: &mut IfcModel, element: StepId) -> Result<()> {
    let mut doomed = Vec::new();
    let keep = HashSet::new();
    for attr in ["ObjectPlacement", "Representation"] {
        if let AttrValue::Ref(root) = ifc.attribute(element, attr)? {
            let tree = crate::mesh_objects::owned_tree(ifc, StepId(root), &keep)?;
            for id in &tree {
                if ifc.class_of(*id)?.eq_ignore_ascii_case("IfcPolyline") {
                    for styled in ifc.referenced_by(*id, "IfcStyledItem")? {
                        doomed.extend(crate::mesh_objects::owned_tree(ifc, styled, &keep)?);
                    }
                }
            }
            doomed.extend(tree);
            ifc.set_attribute(element, attr, &AttrValue::Null)?;
        }
    }
    doomed.sort_by_key(|s| s.0);
    doomed.dedup();
    for id in doomed {
        ifc.remove(id)?;
    }
    Ok(())
}

fn write(
    ifc: &mut IfcModel,
    element: StepId,
    curves: &Curves<'_>,
    scale: f64,
    contexts: geometry::Contexts,
) -> Result<()> {
    let signature = curves.signature(scale);
    if written_signature(ifc, element)?.as_deref() == Some(signature.as_str()) {
        return Ok(());
    }
    remove(ifc, element)?;
    let origin = create(
        ifc,
        "IfcCartesianPoint",
        &[("Coordinates", reals(&[0.0; 3]))],
    )?;
    let axes = create(ifc, "IfcAxis2Placement3D", &[("Location", r(origin))])?;
    let placement = create(ifc, "IfcLocalPlacement", &[("RelativePlacement", r(axes))])?;
    let mut items = Vec::new();
    for line in &curves.polylines {
        let points = line
            .iter()
            .map(|p| {
                create(
                    ifc,
                    "IfcCartesianPoint",
                    &[("Coordinates", reals(&p.map(|v| v / scale)))],
                )
                .map(r)
            })
            .collect::<Result<Vec<_>>>()?;
        let poly = create(ifc, "IfcPolyline", &[("Points", AttrValue::List(points))])?;
        if let Some((name, c)) = curves.style {
            let colour = create(
                ifc,
                "IfcColourRgb",
                &[
                    ("Red", AttrValue::Real(c[0])),
                    ("Green", AttrValue::Real(c[1])),
                    ("Blue", AttrValue::Real(c[2])),
                ],
            )?;
            let style = create(
                ifc,
                "IfcCurveStyle",
                &[
                    ("Name", AttrValue::String(name.to_owned())),
                    ("CurveColour", r(colour)),
                ],
            )?;
            create(
                ifc,
                "IfcStyledItem",
                &[
                    ("Item", r(poly)),
                    ("Styles", AttrValue::List(vec![r(style)])),
                ],
            )?;
        }
        items.push(r(poly));
    }
    let context = if curves.identifier == "Axis" {
        contexts.axis
    } else {
        contexts.body
    };
    let rep = create(
        ifc,
        "IfcShapeRepresentation",
        &[
            ("ContextOfItems", r(context)),
            (
                "RepresentationIdentifier",
                AttrValue::String(curves.identifier.to_owned()),
            ),
            ("RepresentationType", AttrValue::String("Curve3D".into())),
            ("Items", AttrValue::List(items)),
        ],
    )?;
    let shape = create(
        ifc,
        "IfcProductDefinitionShape",
        &[
            ("Description", AttrValue::String(signature)),
            ("Representations", AttrValue::List(vec![r(rep)])),
        ],
    )?;
    ifc.set_attribute(element, "ObjectPlacement", &r(placement))?;
    ifc.set_attribute(element, "Representation", &r(shape))?;
    Ok(())
}

/// Writes every reference plane and model line.
pub(crate) fn write_all(
    ifc: &mut IfcModel,
    doc: &Document,
    steps: &std::collections::HashMap<ElementId, StepId>,
    scale: f64,
    owner_history: Option<StepId>,
) -> Result<()> {
    let store = doc.store();
    let planes: Vec<_> = store
        .iter()
        .filter_map(|e| Some((e.id(), ref_planes::recipe(e)?)))
        .collect();
    let lines: Vec<_> = store
        .iter()
        .filter_map(|e| Some((e.id(), model_lines::recipe(e)?)))
        .collect();
    if planes.is_empty() && lines.is_empty() {
        return Ok(());
    }
    let contexts = geometry::Contexts::find(ifc)?;
    for (id, rp) in planes {
        let Some(&step) = steps.get(&id) else {
            continue;
        };
        let curves = Curves {
            identifier: "Axis",
            polylines: vec![vec![rp.start, rp.end]],
            style: None,
        };
        write(ifc, step, &curves, scale, contexts)?;
    }
    for (id, ml) in lines {
        let Some(&step) = steps.get(&id) else {
            continue;
        };
        let Ok(points) = model_lines::world_points(store, &ml) else {
            continue;
        };
        let style = model_lines::style(&ml.line_style);
        let curves = Curves {
            identifier: "Annotation",
            polylines: vec![points],
            style: style.map(|s| (s.name, s.color.map(|c| f64::from(c) / 255.0))),
        };
        write(ifc, step, &curves, scale, contexts)?;
        // On a level: contained in its storey.
        if let work_planes::WorkPlaneRef::Level { level, .. } = &ml.plane
            && let Some(storey) = GlobalId::parse(level)
                .and_then(|g| store.find_by_guid(&g))
                .and_then(|l| steps.get(&l).copied())
        {
            geometry::contain(ifc, step, storey, owner_history)?;
        }
    }
    Ok(())
}
