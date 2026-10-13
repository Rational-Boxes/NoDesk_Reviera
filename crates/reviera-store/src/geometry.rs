// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! IFC geometry of elements regenerated from Reviera recipes (Ch 2a §6, Ch 3 §3.3).
//!
//! A straight wall is written as most IFC tools write one, so every viewer shows it:
//!
//! - `ObjectPlacement`: an absolute `IfcLocalPlacement` at the wall's start and base, with the
//!   X axis along the wall.
//! - `Axis`: an `IfcPolyline` from (0, 0) to (length, 0) (`Curve2D`).
//! - `Body`: an `IfcExtrudedAreaSolid` of an `IfcRectangleProfileDef` centred on the axis,
//!   extruded up by the height (`SweptSolid`).
//!
//! Re-saving updates these entities in place, so the file keeps its STEP ids. Geometry that does
//! not have this shape (written by another tool) is replaced.

use reviera_core::GlobalId;
use reviera_core::walls::{End, Join, WallBody};
use reviera_ifc::{AttrValue, Entity, IfcModel, StepId};

use crate::Result;
use crate::psets::unsupported;

/// Representation contexts for new shapes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Contexts {
    pub body: StepId,
    pub axis: StepId,
}

impl Contexts {
    /// The `Body` and `Axis` sub-contexts when the file has them, else the 3D model context.
    pub fn find(ifc: &IfcModel) -> Result<Self> {
        let all = ifc.entities_of_type("IfcGeometricRepresentationContext")?;
        let ids: Vec<StepId> = all.iter().map(|e| e.id).collect();
        let entities = ifc.attributes_many(&ids)?;
        let sub = |name: &str| {
            ids.iter().zip(&entities).find_map(|(id, e)| {
                (e.class
                    .eq_ignore_ascii_case("IfcGeometricRepresentationSubContext")
                    && e.string("ContextIdentifier") == Some(name))
                .then_some(*id)
            })
        };
        let model = ids
            .iter()
            .zip(&entities)
            .find(|(_, e)| {
                e.class
                    .eq_ignore_ascii_case("IfcGeometricRepresentationContext")
                    && e.string("ContextType") == Some("Model")
            })
            .or_else(|| ids.iter().zip(&entities).next())
            .map(|(id, _)| *id)
            .ok_or_else(|| unsupported("the file has no geometric representation context"))?;
        Ok(Self {
            body: sub("Body").unwrap_or(model),
            axis: sub("Axis").unwrap_or(model),
        })
    }
}

fn entity(ifc: &IfcModel, id: StepId) -> Result<Entity> {
    Ok(ifc.attributes_many(&[id])?.remove(0))
}

fn reals(v: &[f64]) -> AttrValue {
    AttrValue::List(v.iter().copied().map(AttrValue::Real).collect())
}

/// The entities of a wall shape written by [`write_wall`].
struct WallShape {
    placement: StepId,
    axes: StepId,
    location: StepId,
    ref_direction: StepId,
    up: StepId,
    shape: StepId,
    axis_rep: StepId,
    polyline: StepId,
    axis_points: [StepId; 2],
    body_rep: StepId,
    solid: StepId,
    /// An `IfcRectangleProfileDef` (unjoined walls) or an `IfcArbitraryClosedProfileDef` with an
    /// `IfcPolyline` outline (joined walls).
    profile: StepId,
    extrusion: StepId,
}

/// The entities of a profile written by [`WallShape`], with the profile itself first.
fn profile_entities(ifc: &IfcModel, profile: StepId) -> Result<Option<Vec<StepId>>> {
    let p = entity(ifc, profile)?;
    let mut ids = vec![profile];
    if p.class.eq_ignore_ascii_case("IfcRectangleProfileDef") {
        let Some(axes) = p.refs("Position").first().copied() else {
            return Ok(None);
        };
        ids.push(axes);
        ids.extend(entity(ifc, axes)?.refs("Location"));
    } else if p.class.eq_ignore_ascii_case("IfcArbitraryClosedProfileDef") {
        let Some(curve) = p.refs("OuterCurve").first().copied() else {
            return Ok(None);
        };
        let c = entity(ifc, curve)?;
        if !c.class.eq_ignore_ascii_case("IfcPolyline") {
            return Ok(None);
        }
        ids.push(curve);
        let mut points = c.refs("Points");
        points.dedup();
        points.sort();
        points.dedup();
        ids.extend(points);
    } else {
        return Ok(None);
    }
    Ok(Some(ids))
}

impl WallShape {
    /// Recognises the shape of a wall, or `None` when it has another form.
    fn read(ifc: &IfcModel, wall: &Entity) -> Result<Option<Self>> {
        let one = |e: &Entity, attr: &str| e.refs(attr).first().copied();
        let class_is = |id: StepId, class: &str| -> Result<Option<Entity>> {
            let e = entity(ifc, id)?;
            Ok(e.class.eq_ignore_ascii_case(class).then_some(e))
        };
        macro_rules! get {
            ($e:expr) => {
                match $e {
                    Some(v) => v,
                    None => return Ok(None),
                }
            };
        }
        let placement = get!(one(wall, "ObjectPlacement"));
        let p = get!(class_is(placement, "IfcLocalPlacement")?);
        if !p.refs("PlacementRelTo").is_empty() {
            return Ok(None);
        }
        let axes = get!(one(&p, "RelativePlacement"));
        let a = get!(class_is(axes, "IfcAxis2Placement3D")?);
        let (location, up, ref_direction) = (
            get!(one(&a, "Location")),
            get!(one(&a, "Axis")),
            get!(one(&a, "RefDirection")),
        );
        let shape = get!(one(wall, "Representation"));
        let s = get!(class_is(shape, "IfcProductDefinitionShape")?);
        let reps = s.refs("Representations");
        if reps.len() != 2 {
            return Ok(None);
        }
        let (r0, r1) = (entity(ifc, reps[0])?, entity(ifc, reps[1])?);
        let (axis_rep, body_rep, ar, br) = match (
            r0.string("RepresentationIdentifier"),
            r1.string("RepresentationIdentifier"),
        ) {
            (Some("Axis"), Some("Body")) => (reps[0], reps[1], r0, r1),
            (Some("Body"), Some("Axis")) => (reps[1], reps[0], r1, r0),
            _ => return Ok(None),
        };
        let polyline = get!(one(&ar, "Items"));
        let pl = get!(class_is(polyline, "IfcPolyline")?);
        let points = pl.refs("Points");
        if points.len() != 2 {
            return Ok(None);
        }
        let solid = get!(one(&br, "Items"));
        let so = get!(class_is(solid, "IfcExtrudedAreaSolid")?);
        let profile = get!(one(&so, "SweptArea"));
        get!(profile_entities(ifc, profile)?);
        Ok(Some(Self {
            placement,
            axes,
            location,
            ref_direction,
            up,
            shape,
            axis_rep,
            polyline,
            axis_points: [points[0], points[1]],
            body_rep,
            solid,
            profile,
            extrusion: get!(one(&so, "ExtrudedDirection")),
        }))
    }

    /// Creates a new shape with every entity in place (values are set by [`Self::update`]).
    fn create(ifc: &mut IfcModel, contexts: Contexts) -> Result<Self> {
        let mut new = |class: &str, attrs: &[(&str, AttrValue)]| -> Result<StepId> {
            let id = ifc.create(class)?;
            for (name, value) in attrs {
                ifc.set_attribute(id, name, value)?;
            }
            Ok(id)
        };
        let r = |id: StepId| AttrValue::Ref(id.0);
        let s = |v: &str| AttrValue::String(v.into());
        let location = new(
            "IfcCartesianPoint",
            &[("Coordinates", reals(&[0.0, 0.0, 0.0]))],
        )?;
        let up = new(
            "IfcDirection",
            &[("DirectionRatios", reals(&[0.0, 0.0, 1.0]))],
        )?;
        let ref_direction = new(
            "IfcDirection",
            &[("DirectionRatios", reals(&[1.0, 0.0, 0.0]))],
        )?;
        let axes = new(
            "IfcAxis2Placement3D",
            &[
                ("Location", r(location)),
                ("Axis", r(up)),
                ("RefDirection", r(ref_direction)),
            ],
        )?;
        let placement = new("IfcLocalPlacement", &[("RelativePlacement", r(axes))])?;

        let axis_points = [
            new("IfcCartesianPoint", &[("Coordinates", reals(&[0.0, 0.0]))])?,
            new("IfcCartesianPoint", &[("Coordinates", reals(&[1.0, 0.0]))])?,
        ];
        let polyline = new(
            "IfcPolyline",
            &[(
                "Points",
                AttrValue::List(vec![r(axis_points[0]), r(axis_points[1])]),
            )],
        )?;
        let axis_rep = new(
            "IfcShapeRepresentation",
            &[
                ("ContextOfItems", r(contexts.axis)),
                ("RepresentationIdentifier", s("Axis")),
                ("RepresentationType", s("Curve2D")),
                ("Items", AttrValue::List(vec![r(polyline)])),
            ],
        )?;

        let profile = new_rectangle(ifc)?;
        let mut new = |class: &str, attrs: &[(&str, AttrValue)]| -> Result<StepId> {
            let id = ifc.create(class)?;
            for (name, value) in attrs {
                ifc.set_attribute(id, name, value)?;
            }
            Ok(id)
        };
        let solid_origin = new(
            "IfcCartesianPoint",
            &[("Coordinates", reals(&[0.0, 0.0, 0.0]))],
        )?;
        let solid_axes = new("IfcAxis2Placement3D", &[("Location", r(solid_origin))])?;
        let extrusion = new(
            "IfcDirection",
            &[("DirectionRatios", reals(&[0.0, 0.0, 1.0]))],
        )?;
        let solid = new(
            "IfcExtrudedAreaSolid",
            &[
                ("SweptArea", r(profile)),
                ("Position", r(solid_axes)),
                ("ExtrudedDirection", r(extrusion)),
                ("Depth", AttrValue::Real(1.0)),
            ],
        )?;
        let body_rep = new(
            "IfcShapeRepresentation",
            &[
                ("ContextOfItems", r(contexts.body)),
                ("RepresentationIdentifier", s("Body")),
                ("RepresentationType", s("SweptSolid")),
                ("Items", AttrValue::List(vec![r(solid)])),
            ],
        )?;
        let shape = new(
            "IfcProductDefinitionShape",
            &[(
                "Representations",
                AttrValue::List(vec![r(axis_rep), r(body_rep)]),
            )],
        )?;
        Ok(Self {
            placement,
            axes,
            location,
            ref_direction,
            up,
            shape,
            axis_rep,
            polyline,
            axis_points,
            body_rep,
            solid,
            profile,
            extrusion,
        })
    }

    /// Sets the values of the shape from the body (in file units). The profile is replaced when
    /// the wall changes between unjoined (rectangle) and joined (polygon).
    fn update(&mut self, ifc: &mut IfcModel, body: &WallBody, scale: f64) -> Result<()> {
        let g = &body.geometry;
        let [ux, uy] = g.direction();
        let (l, w, h) = (g.length() / scale, g.width / scale, g.height / scale);
        let set = |ifc: &mut IfcModel, id: StepId, attr: &str, v: AttrValue| -> Result<()> {
            if ifc.attribute(id, attr)? != v {
                ifc.set_attribute(id, attr, &v)?;
            }
            Ok(())
        };
        let base = [g.start[0] / scale, g.start[1] / scale, g.base_z / scale];
        set(ifc, self.location, "Coordinates", reals(&base))?;
        set(ifc, self.up, "DirectionRatios", reals(&[0.0, 0.0, 1.0]))?;
        set(
            ifc,
            self.ref_direction,
            "DirectionRatios",
            reals(&[ux, uy, 0.0]),
        )?;
        set(ifc, self.axis_points[0], "Coordinates", reals(&[0.0, 0.0]))?;
        set(ifc, self.axis_points[1], "Coordinates", reals(&[l, 0.0]))?;
        let rectangle = body.is_rectangular();
        let class = ifc.class_of(self.profile)?;
        let polyline = match ifc.attribute(self.profile, "OuterCurve") {
            Ok(AttrValue::Ref(c)) => Some(entity(ifc, StepId(c))?.refs("Points")),
            _ => None,
        };
        let fits = if rectangle {
            class.eq_ignore_ascii_case("IfcRectangleProfileDef")
        } else {
            polyline
                .as_ref()
                .is_some_and(|p| p.len() == 5 && p[0] == p[4])
        };
        if !fits {
            let old = profile_entities(ifc, self.profile)?.unwrap_or_default();
            self.profile = if rectangle {
                new_rectangle(ifc)?
            } else {
                new_polygon(ifc)?
            };
            ifc.set_attribute(self.solid, "SweptArea", &AttrValue::Ref(self.profile.0))?;
            for id in old {
                ifc.remove(id)?;
            }
        }
        if rectangle {
            let axes = entity(ifc, self.profile)?.refs("Position")[0];
            let location = entity(ifc, axes)?.refs("Location")[0];
            set(ifc, location, "Coordinates", reals(&[l / 2.0, 0.0]))?;
            set(ifc, self.profile, "XDim", AttrValue::Real(l))?;
            set(ifc, self.profile, "YDim", AttrValue::Real(w))?;
        } else {
            let curve = entity(ifc, self.profile)?.refs("OuterCurve")[0];
            let points = entity(ifc, curve)?.refs("Points");
            for (point, [x, y]) in points.iter().zip(body.local_footprint()) {
                set(ifc, *point, "Coordinates", reals(&[x / scale, y / scale]))?;
            }
        }
        set(
            ifc,
            self.extrusion,
            "DirectionRatios",
            reals(&[0.0, 0.0, 1.0]),
        )?;
        set(ifc, self.solid, "Depth", AttrValue::Real(h))?;
        Ok(())
    }

    /// Every entity of the shape, for removal.
    fn entities(&self, ifc: &IfcModel) -> Result<Vec<StepId>> {
        let mut ids = vec![
            self.placement,
            self.axes,
            self.location,
            self.ref_direction,
            self.up,
            self.shape,
            self.axis_rep,
            self.polyline,
            self.axis_points[0],
            self.axis_points[1],
            self.body_rep,
            self.solid,
            self.extrusion,
        ];
        ids.extend(profile_entities(ifc, self.profile)?.unwrap_or_default());
        if let Some(axes) = entity(ifc, self.solid)?.refs("Position").first() {
            ids.push(*axes);
            ids.extend(entity(ifc, *axes)?.refs("Location"));
        }
        Ok(ids)
    }
}

fn create_with(ifc: &mut IfcModel, class: &str, attrs: &[(&str, AttrValue)]) -> Result<StepId> {
    let id = ifc.create(class)?;
    for (name, value) in attrs {
        ifc.set_attribute(id, name, value)?;
    }
    Ok(id)
}

/// A rectangle profile centred on its position (values are set by [`WallShape::update`]).
fn new_rectangle(ifc: &mut IfcModel) -> Result<StepId> {
    let location = create_with(
        ifc,
        "IfcCartesianPoint",
        &[("Coordinates", reals(&[0.0, 0.0]))],
    )?;
    let axes = create_with(
        ifc,
        "IfcAxis2Placement2D",
        &[("Location", AttrValue::Ref(location.0))],
    )?;
    create_with(
        ifc,
        "IfcRectangleProfileDef",
        &[
            ("ProfileType", AttrValue::Enum("AREA".into())),
            ("Position", AttrValue::Ref(axes.0)),
            ("XDim", AttrValue::Real(1.0)),
            ("YDim", AttrValue::Real(1.0)),
        ],
    )
}

/// A closed four-sided polyline profile; the first point closes the outline.
fn new_polygon(ifc: &mut IfcModel) -> Result<StepId> {
    let mut points = Vec::new();
    for c in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
        points.push(AttrValue::Ref(
            create_with(ifc, "IfcCartesianPoint", &[("Coordinates", reals(&c))])?.0,
        ));
    }
    points.push(points[0].clone());
    let curve = create_with(ifc, "IfcPolyline", &[("Points", AttrValue::List(points))])?;
    create_with(
        ifc,
        "IfcArbitraryClosedProfileDef",
        &[
            ("ProfileType", AttrValue::Enum("AREA".into())),
            ("OuterCurve", AttrValue::Ref(curve.0)),
        ],
    )
}

/// Writes a wall's placement and representations, updating its existing shape in place.
pub(crate) fn write_wall(
    ifc: &mut IfcModel,
    wall: StepId,
    body: &WallBody,
    scale: f64,
    contexts: Contexts,
) -> Result<()> {
    let e = entity(ifc, wall)?;
    let mut shape = match WallShape::read(ifc, &e)? {
        Some(s) => s,
        None => {
            let s = WallShape::create(ifc, contexts)?;
            ifc.set_attribute(wall, "ObjectPlacement", &AttrValue::Ref(s.placement.0))?;
            ifc.set_attribute(wall, "Representation", &AttrValue::Ref(s.shape.0))?;
            s
        }
    };
    shape.update(ifc, body, scale)
}

/// Removes the shape of a wall written by [`write_wall`] (before the wall itself is removed).
pub(crate) fn remove_wall(ifc: &mut IfcModel, wall: StepId) -> Result<()> {
    for rel in ifc.referenced_by(wall, "IfcRelConnectsPathElements")? {
        ifc.remove(rel)?;
    }
    let e = entity(ifc, wall)?;
    if let Some(shape) = WallShape::read(ifc, &e)? {
        for id in shape.entities(ifc)? {
            ifc.remove(id)?;
        }
    }
    Ok(())
}

/// Places `element` in `storey` (`IfcRelContainedInSpatialStructure`), moving it out of any
/// other container.
pub(crate) fn contain(
    ifc: &mut IfcModel,
    element: StepId,
    storey: StepId,
    owner_history: Option<StepId>,
) -> Result<()> {
    const REL: &str = "IfcRelContainedInSpatialStructure";
    for rel in ifc.referenced_by(element, REL)? {
        let e = entity(ifc, rel)?;
        if !e.refs("RelatedElements").contains(&element) {
            continue;
        }
        if e.refs("RelatingStructure") == [storey] {
            return Ok(());
        }
        let kept: Vec<_> = e
            .refs("RelatedElements")
            .into_iter()
            .filter(|o| *o != element)
            .map(|o| AttrValue::Ref(o.0))
            .collect();
        if kept.is_empty() {
            ifc.remove(rel)?;
        } else {
            ifc.set_attribute(rel, "RelatedElements", &AttrValue::List(kept))?;
        }
    }
    let existing = ifc.referenced_by(storey, REL)?.into_iter().find(|r| {
        ifc.attribute(*r, "RelatingStructure")
            .is_ok_and(|v| v == AttrValue::Ref(storey.0))
    });
    let (rel, mut elements) = match existing {
        Some(rel) => (rel, entity(ifc, rel)?.refs("RelatedElements")),
        None => {
            let rel = ifc.create_rooted(REL, &GlobalId::new())?;
            if let Some(oh) = owner_history {
                ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
            }
            ifc.set_attribute(rel, "RelatingStructure", &AttrValue::Ref(storey.0))?;
            (rel, Vec::new())
        }
    };
    elements.push(element);
    let list = AttrValue::List(elements.into_iter().map(|o| AttrValue::Ref(o.0)).collect());
    Ok(ifc.set_attribute(rel, "RelatedElements", &list)?)
}

/// Metres per length unit of the file (from its `IfcUnitAssignment`; 1 when not stated).
pub(crate) fn length_scale(ifc: &IfcModel) -> Result<f64> {
    let Some(project) = ifc.entities_of_type("IfcProject")?.first().map(|e| e.id) else {
        return Ok(1.0);
    };
    let Some(units) = entity(ifc, project)?
        .refs("UnitsInContext")
        .first()
        .copied()
    else {
        return Ok(1.0);
    };
    let ids = entity(ifc, units)?.refs("Units");
    for u in ifc.attributes_many(&ids)? {
        if u.get("UnitType") != &AttrValue::Enum("LENGTHUNIT".into()) {
            continue;
        }
        if u.class.eq_ignore_ascii_case("IfcSIUnit") {
            return Ok(match u.get("Prefix") {
                AttrValue::Enum(p) => si_prefix(p),
                _ => 1.0,
            });
        }
        // A conversion-based unit (for example FOOT): its factor in metres.
        if let Some(m) = u.refs("ConversionFactor").first()
            && let AttrValue::Typed { value, .. } = entity(ifc, *m)?.get("ValueComponent")
            && let AttrValue::Real(f) = value.as_ref()
        {
            return Ok(*f);
        }
    }
    Ok(1.0)
}

fn si_prefix(p: &str) -> f64 {
    match p {
        "KILO" => 1e3,
        "HECTO" => 1e2,
        "DECA" => 1e1,
        "DECI" => 1e-1,
        "CENTI" => 1e-2,
        "MILLI" => 1e-3,
        "MICRO" => 1e-6,
        _ => 1.0,
    }
}

/// Writes the connections between Reviera walls as `IfcRelConnectsPathElements` (ARC-JOIN-001):
/// L joins connect `ATSTART`/`ATEND` to `ATSTART`/`ATEND`, T joins an end to `ATPATH`.
/// Connections between `walls` that are no longer joined are removed; others are left alone.
pub(crate) fn write_connections(
    ifc: &mut IfcModel,
    walls: &std::collections::HashSet<StepId>,
    joins: &[(StepId, Join, StepId)],
    owner_history: Option<StepId>,
) -> Result<()> {
    const REL: &str = "IfcRelConnectsPathElements";
    let kind = |e: Option<End>| match e {
        Some(End::Start) => "ATSTART",
        Some(End::End) => "ATEND",
        None => "ATPATH",
    };
    let key = |a: StepId, ka: &str, b: StepId, kb: &str| (a, ka.to_owned(), b, kb.to_owned());
    let mut wanted: std::collections::BTreeSet<_> = joins
        .iter()
        .map(|(a, j, b)| key(*a, kind(Some(j.end)), *b, kind(j.other_end)))
        .collect();
    let existing: Vec<StepId> = ifc
        .entities_of_type(REL)?
        .into_iter()
        .map(|e| e.id)
        .collect();
    for (rel, e) in existing.iter().zip(ifc.attributes_many(&existing)?) {
        let (Some(a), Some(b)) = (
            e.refs("RelatingElement").first().copied(),
            e.refs("RelatedElement").first().copied(),
        ) else {
            continue;
        };
        if !walls.contains(&a) || !walls.contains(&b) {
            continue;
        }
        let enum_of = |name: &str| match e.get(name) {
            AttrValue::Enum(v) => v.clone(),
            _ => String::new(),
        };
        let k = key(
            a,
            &enum_of("RelatingConnectionType"),
            b,
            &enum_of("RelatedConnectionType"),
        );
        if !wanted.remove(&k) {
            ifc.remove(*rel)?;
        }
    }
    for (a, ka, b, kb) in wanted {
        let rel = ifc.create_rooted(REL, &GlobalId::new())?;
        if let Some(oh) = owner_history {
            ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
        }
        ifc.set_attribute(rel, "RelatingElement", &AttrValue::Ref(a.0))?;
        ifc.set_attribute(rel, "RelatedElement", &AttrValue::Ref(b.0))?;
        // One layer each, and it passes through the join.
        ifc.set_attribute(
            rel,
            "RelatingPriorities",
            &AttrValue::List(vec![AttrValue::Int(0)]),
        )?;
        ifc.set_attribute(
            rel,
            "RelatedPriorities",
            &AttrValue::List(vec![AttrValue::Int(0)]),
        )?;
        ifc.set_attribute(rel, "RelatingConnectionType", &AttrValue::Enum(ka))?;
        ifc.set_attribute(rel, "RelatedConnectionType", &AttrValue::Enum(kb))?;
    }
    Ok(())
}
