// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Custom objects in IFC (DM-IMP-015).
//!
//! - A custom object type's mesh is written once, as its `IfcRepresentationMap`: one
//!   `IfcTriangulatedFaceSet` per material slot, each styled with the slot's colour
//!   (`IfcStyledItem` → `IfcSurfaceStyle` → `IfcSurfaceStyleShading`).
//! - Each copy is placed by its `ObjectPlacement` (point, level height, rotation) and its body is
//!   an `IfcMappedItem` of the type's map, scaled by the type's scale and mirrored when the copy
//!   is.
//!
//! The first styled item's Name (`rxd:mesh:<signature>`) and each copy's
//! `IfcProductDefinitionShape` Description (`rxd:mesh_object:<signature>`) record what was
//! written, so unchanged types and copies are left as they are.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use reviera_core::mesh_objects::{self, MeshObjectGeometry, MeshObjectType};
use reviera_core::{Document, ElementId, GlobalId};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::{Result, geometry};

const TYPE_TAG: &str = "rxd:mesh";
const COPY_TAG: &str = "rxd:mesh_object";

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

fn hash_hex(f: impl FnOnce(&mut std::collections::hash_map::DefaultHasher)) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f(&mut h);
    format!("{:016x}", h.finish())
}

fn type_signature(t: &MeshObjectType, scale: f64) -> String {
    hash_hex(|h| {
        t.mesh.hash.hash(h);
        for s in &t.slots {
            s.name.hash(h);
            for c in s.color {
                c.to_bits().hash(h);
            }
        }
        scale.to_bits().hash(h);
    })
}

fn copy_signature(g: &MeshObjectGeometry, map: StepId, scale: f64) -> String {
    hash_hex(|h| {
        for v in g.placement.origin {
            v.to_bits().hash(h);
        }
        g.placement.rotation.to_bits().hash(h);
        g.placement.mirror.hash(h);
        g.placement.scale.to_bits().hash(h);
        map.0.hash(h);
        scale.to_bits().hash(h);
    })
}

/// Entities reachable from `root` through its attributes that belong to it alone: not rooted
/// objects, contexts, owner histories, nor anything in `keep`.
fn owned_tree(ifc: &IfcModel, root: StepId, keep: &HashSet<StepId>) -> Result<Vec<StepId>> {
    const SHARED: &[&str] = &[
        "IfcGeometricRepresentationContext",
        "IfcGeometricRepresentationSubContext",
        "IfcOwnerHistory",
        "IfcRepresentationMap",
    ];
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) || keep.contains(&id) {
            continue;
        }
        let class = ifc.class_of(id)?;
        if SHARED.contains(&class.as_str()) {
            continue;
        }
        let attrs = ifc.attributes(id)?;
        if attrs.iter().any(|(n, _)| n == "GlobalId") {
            continue;
        }
        out.push(id);
        let mut refs = Vec::new();
        for (_, v) in &attrs {
            collect_refs(v, &mut refs);
        }
        stack.extend(refs.into_iter().map(StepId));
    }
    Ok(out)
}

fn collect_refs(v: &AttrValue, out: &mut Vec<u32>) {
    match v {
        AttrValue::Ref(i) => out.push(*i),
        AttrValue::List(items) => items.iter().for_each(|i| collect_refs(i, out)),
        AttrValue::Typed { value, .. } => collect_refs(value, out),
        _ => {}
    }
}

/// The type's representation maps written before.
pub(crate) fn maps_of(ifc: &IfcModel, ty: StepId) -> Result<Vec<StepId>> {
    let mut refs = Vec::new();
    collect_refs(&ifc.attribute(ty, "RepresentationMaps")?, &mut refs);
    Ok(refs.into_iter().map(StepId).collect())
}

/// The signature written with a type's map, if Reviera wrote it.
fn written_type_signature(ifc: &IfcModel, map: StepId) -> Result<Option<String>> {
    let AttrValue::Ref(rep) = ifc.attribute(map, "MappedRepresentation")? else {
        return Ok(None);
    };
    let mut items = Vec::new();
    collect_refs(&ifc.attribute(StepId(rep), "Items")?, &mut items);
    for item in items {
        for styled in ifc.referenced_by(StepId(item), "IfcStyledItem")? {
            if let AttrValue::String(n) = ifc.attribute(styled, "Name")?
                && let Some(sig) = n.strip_prefix(&format!("{TYPE_TAG}:"))
            {
                return Ok(Some(sig.to_owned()));
            }
        }
    }
    Ok(None)
}

/// Removes a type's map, its face sets and their styles.
pub(crate) fn remove_type_geometry(ifc: &mut IfcModel, ty: StepId) -> Result<()> {
    for map in maps_of(ifc, ty)? {
        let mut doomed = Vec::new();
        if let AttrValue::Ref(rep) = ifc.attribute(map, "MappedRepresentation")? {
            let mut items = Vec::new();
            collect_refs(&ifc.attribute(StepId(rep), "Items")?, &mut items);
            for item in items {
                for styled in ifc.referenced_by(StepId(item), "IfcStyledItem")? {
                    doomed.extend(owned_tree(ifc, styled, &HashSet::new())?);
                }
            }
        }
        doomed.push(map);
        let keep = HashSet::new();
        // The map itself is "shared" to the walker; walk from its parts.
        for (_, v) in ifc.attributes(map)? {
            let mut refs = Vec::new();
            collect_refs(&v, &mut refs);
            for id in refs {
                doomed.extend(owned_tree(ifc, StepId(id), &keep)?);
            }
        }
        doomed.sort_by_key(|s| s.0);
        doomed.dedup();
        for id in doomed {
            ifc.remove(id)?;
        }
    }
    ifc.set_attribute(ty, "RepresentationMaps", &AttrValue::Null)?;
    Ok(())
}

/// Removes a copy's placement and body (not the type's map it uses).
pub(crate) fn remove_copy_geometry(ifc: &mut IfcModel, element: StepId) -> Result<()> {
    let mut doomed = Vec::new();
    for attr in ["Representation", "ObjectPlacement"] {
        if let AttrValue::Ref(id) = ifc.attribute(element, attr)? {
            doomed.extend(owned_tree(ifc, StepId(id), &HashSet::new())?);
        }
    }
    ifc.set_attribute(element, "Representation", &AttrValue::Null)?;
    ifc.set_attribute(element, "ObjectPlacement", &AttrValue::Null)?;
    doomed.sort_by_key(|s| s.0);
    doomed.dedup();
    for id in doomed {
        ifc.remove(id)?;
    }
    Ok(())
}

/// Writes a type's map; returns it.
fn write_type(
    ifc: &mut IfcModel,
    ty_step: StepId,
    t: &MeshObjectType,
    sig: &str,
    scale: f64,
    contexts: geometry::Contexts,
) -> Result<StepId> {
    remove_type_geometry(ifc, ty_step)?;
    let mesh = t.mesh.decode()?;
    let origin_point = create(
        ifc,
        "IfcCartesianPoint",
        &[("Coordinates", reals(&[0.0, 0.0, 0.0]))],
    )?;
    let origin = create(ifc, "IfcAxis2Placement3D", &[("Location", r(origin_point))])?;
    let mut items = Vec::new();
    let mut first_style = true;
    for slot in 0..t.slots.len().max(1) {
        let mut map: HashMap<u32, i64> = HashMap::new();
        let mut coords: Vec<AttrValue> = Vec::new();
        let mut index: Vec<AttrValue> = Vec::new();
        for (tri, s) in mesh.triangles.iter().zip(&mesh.tri_slot) {
            if *s as usize != slot {
                continue;
            }
            let mut row = Vec::with_capacity(3);
            for i in tri {
                let n = *map.entry(*i).or_insert_with(|| {
                    let p = mesh.positions[*i as usize];
                    coords.push(reals(&[
                        p[0] as f64 / scale,
                        p[1] as f64 / scale,
                        p[2] as f64 / scale,
                    ]));
                    coords.len() as i64
                });
                row.push(AttrValue::Int(n));
            }
            index.push(AttrValue::List(row));
        }
        if index.is_empty() {
            continue;
        }
        let list = create(
            ifc,
            "IfcCartesianPointList3D",
            &[("CoordList", AttrValue::List(coords))],
        )?;
        let set = create(
            ifc,
            "IfcTriangulatedFaceSet",
            &[
                ("Coordinates", r(list)),
                ("CoordIndex", AttrValue::List(index)),
            ],
        )?;
        let (name, c) = t
            .slots
            .get(slot)
            .map_or(("Default".to_owned(), [0.78, 0.78, 0.78, 1.0]), |s| {
                (s.name.clone(), s.color)
            });
        let colour = create(
            ifc,
            "IfcColourRgb",
            &[
                ("Red", AttrValue::Real(c[0] as f64)),
                ("Green", AttrValue::Real(c[1] as f64)),
                ("Blue", AttrValue::Real(c[2] as f64)),
            ],
        )?;
        let mut shading_attrs = vec![("SurfaceColour", r(colour))];
        if c[3] < 1.0 {
            shading_attrs.push(("Transparency", AttrValue::Real(1.0 - c[3] as f64)));
        }
        let shading = create(ifc, "IfcSurfaceStyleShading", &shading_attrs)?;
        let style = create(
            ifc,
            "IfcSurfaceStyle",
            &[
                ("Name", AttrValue::String(name)),
                ("Side", AttrValue::Enum("BOTH".into())),
                ("Styles", AttrValue::List(vec![r(shading)])),
            ],
        )?;
        let mut styled = vec![
            ("Item", r(set)),
            ("Styles", AttrValue::List(vec![r(style)])),
        ];
        if first_style {
            styled.push(("Name", AttrValue::String(format!("{TYPE_TAG}:{sig}"))));
            first_style = false;
        }
        create(ifc, "IfcStyledItem", &styled)?;
        items.push(r(set));
    }
    let rep = create(
        ifc,
        "IfcShapeRepresentation",
        &[
            ("ContextOfItems", r(contexts.body)),
            ("RepresentationIdentifier", AttrValue::String("Body".into())),
            (
                "RepresentationType",
                AttrValue::String("Tessellation".into()),
            ),
            ("Items", AttrValue::List(items)),
        ],
    )?;
    let map = create(
        ifc,
        "IfcRepresentationMap",
        &[
            ("MappingOrigin", r(origin)),
            ("MappedRepresentation", r(rep)),
        ],
    )?;
    ifc.set_attribute(
        ty_step,
        "RepresentationMaps",
        &AttrValue::List(vec![r(map)]),
    )?;
    // IfcFurnitureType needs its assembly place.
    if ifc.class_of(ty_step)? == "IfcFurnitureType"
        && matches!(ifc.attribute(ty_step, "AssemblyPlace")?, AttrValue::Null)
    {
        ifc.set_attribute(
            ty_step,
            "AssemblyPlace",
            &AttrValue::Enum("NOTDEFINED".into()),
        )?;
    }
    Ok(map)
}

/// Writes a copy's placement and mapped body.
fn write_copy(
    ifc: &mut IfcModel,
    step: StepId,
    g: &MeshObjectGeometry,
    map: StepId,
    sig: &str,
    scale: f64,
    contexts: geometry::Contexts,
) -> Result<()> {
    remove_copy_geometry(ifc, step)?;
    let description = format!("{COPY_TAG}:{sig}");
    write_mapped(ifc, step, g.placement, map, &description, scale, contexts)
}

/// Gives `step` a placement and an `IfcMappedItem` body of `map`; `description` marks the
/// product definition shape.
pub(crate) fn write_mapped(
    ifc: &mut IfcModel,
    step: StepId,
    p: reviera_core::mesh_objects::Placement,
    map: StepId,
    description: &str,
    scale: f64,
    contexts: geometry::Contexts,
) -> Result<()> {
    let location = create(
        ifc,
        "IfcCartesianPoint",
        &[("Coordinates", reals(&p.origin.map(|v| v / scale)))],
    )?;
    let up = create(
        ifc,
        "IfcDirection",
        &[("DirectionRatios", reals(&[0.0, 0.0, 1.0]))],
    )?;
    let x = create(
        ifc,
        "IfcDirection",
        &[(
            "DirectionRatios",
            reals(&[p.rotation.cos(), p.rotation.sin(), 0.0]),
        )],
    )?;
    let axes = create(
        ifc,
        "IfcAxis2Placement3D",
        &[
            ("Location", r(location)),
            ("Axis", r(up)),
            ("RefDirection", r(x)),
        ],
    )?;
    let placement = create(ifc, "IfcLocalPlacement", &[("RelativePlacement", r(axes))])?;
    let zero = create(
        ifc,
        "IfcCartesianPoint",
        &[("Coordinates", reals(&[0.0, 0.0, 0.0]))],
    )?;
    let target = if p.mirror {
        create(
            ifc,
            "IfcCartesianTransformationOperator3DnonUniform",
            &[
                ("LocalOrigin", r(zero)),
                ("Scale", AttrValue::Real(p.scale)),
                ("Scale2", AttrValue::Real(-p.scale)),
                ("Scale3", AttrValue::Real(p.scale)),
            ],
        )?
    } else {
        create(
            ifc,
            "IfcCartesianTransformationOperator3D",
            &[
                ("LocalOrigin", r(zero)),
                ("Scale", AttrValue::Real(p.scale)),
            ],
        )?
    };
    let item = create(
        ifc,
        "IfcMappedItem",
        &[("MappingSource", r(map)), ("MappingTarget", r(target))],
    )?;
    let rep = create(
        ifc,
        "IfcShapeRepresentation",
        &[
            ("ContextOfItems", r(contexts.body)),
            ("RepresentationIdentifier", AttrValue::String("Body".into())),
            (
                "RepresentationType",
                AttrValue::String("MappedRepresentation".into()),
            ),
            ("Items", AttrValue::List(vec![r(item)])),
        ],
    )?;
    let shape = create(
        ifc,
        "IfcProductDefinitionShape",
        &[
            ("Description", AttrValue::String(description.to_owned())),
            ("Representations", AttrValue::List(vec![r(rep)])),
        ],
    )?;
    ifc.set_attribute(step, "ObjectPlacement", &r(placement))?;
    ifc.set_attribute(step, "Representation", &r(shape))?;
    Ok(())
}

fn written_copy_signature(ifc: &IfcModel, step: StepId) -> Result<Option<String>> {
    let AttrValue::Ref(shape) = ifc.attribute(step, "Representation")? else {
        return Ok(None);
    };
    Ok(match ifc.attribute(StepId(shape), "Description")? {
        AttrValue::String(d) => d.strip_prefix(&format!("{COPY_TAG}:")).map(str::to_owned),
        _ => None,
    })
}

/// Writes every custom object type's map and every copy's body, leaving unchanged ones as they
/// are, and contains each copy in its level.
pub(crate) fn write_mesh_objects(
    ifc: &mut IfcModel,
    doc: &Document,
    steps: &HashMap<ElementId, StepId>,
    owner_history: Option<StepId>,
    scale: f64,
) -> Result<()> {
    let store = doc.store();
    let types = mesh_objects::types(store);
    if types.is_empty() {
        return Ok(());
    }
    let contexts = geometry::Contexts::find(ifc)?;
    let mut maps: HashMap<ElementId, StepId> = HashMap::new();
    for (id, t, _) in &types {
        let Some(&step) = steps.get(id) else { continue };
        let sig = type_signature(t, scale);
        let existing = maps_of(ifc, step)?;
        let map = match existing.as_slice() {
            [m] if written_type_signature(ifc, *m)?.as_deref() == Some(sig.as_str()) => *m,
            _ => write_type(ifc, step, t, &sig, scale, contexts)?,
        };
        maps.insert(*id, map);
    }
    for (id, g) in mesh_objects::all(store) {
        let (Some(&step), Some(&map)) = (steps.get(&id), maps.get(&g.type_id)) else {
            continue;
        };
        let sig = copy_signature(&g, map, scale);
        if written_copy_signature(ifc, step)?.as_deref() != Some(sig.as_str()) {
            write_copy(ifc, step, &g, map, &sig, scale, contexts)?;
        }
        let storey = mesh_objects::mesh_object_recipe(store.get(id).expect("listed"))
            .and_then(|r| GlobalId::parse(&r.level))
            .and_then(|l| store.find_by_guid(&l))
            .and_then(|l| steps.get(&l).copied());
        if let Some(storey) = storey {
            geometry::contain(ifc, step, storey, owner_history)?;
        }
    }
    Ok(())
}
