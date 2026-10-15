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
    crate::stacked_walls::remove_subs(ifc, wall)?;
    remove_breps(ifc, wall)?;
    remove_wall_shape(ifc, wall)
}

/// Removes a wall's extruded shape written by [`write_wall`], keeping its connections (the wall
/// gets a B-rep body when it attaches to a roof).
pub(crate) fn remove_wall_shape(ifc: &mut IfcModel, wall: StepId) -> Result<()> {
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

/// Makes `element` a part of `whole` (`IfcRelAggregates`: a space under its storey), taking it
/// out of any other aggregate or spatial container.
pub(crate) fn aggregate(
    ifc: &mut IfcModel,
    element: StepId,
    whole: StepId,
    owner_history: Option<StepId>,
) -> Result<()> {
    const REL: &str = "IfcRelAggregates";
    // A space is never also contained.
    for rel in ifc.referenced_by(element, "IfcRelContainedInSpatialStructure")? {
        let e = entity(ifc, rel)?;
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
    for rel in ifc.referenced_by(element, REL)? {
        let e = entity(ifc, rel)?;
        if !e.refs("RelatedObjects").contains(&element) {
            continue;
        }
        if e.refs("RelatingObject") == [whole] {
            return Ok(());
        }
        let kept: Vec<_> = e
            .refs("RelatedObjects")
            .into_iter()
            .filter(|o| *o != element)
            .map(|o| AttrValue::Ref(o.0))
            .collect();
        if kept.is_empty() {
            ifc.remove(rel)?;
        } else {
            ifc.set_attribute(rel, "RelatedObjects", &AttrValue::List(kept))?;
        }
    }
    let existing = ifc.referenced_by(whole, REL)?.into_iter().find(|r| {
        ifc.attribute(*r, "RelatingObject")
            .is_ok_and(|v| v == AttrValue::Ref(whole.0))
    });
    let (rel, mut parts) = match existing {
        Some(rel) => (rel, entity(ifc, rel)?.refs("RelatedObjects")),
        None => {
            let rel = ifc.create_rooted(REL, &GlobalId::new())?;
            if let Some(oh) = owner_history {
                ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
            }
            ifc.set_attribute(rel, "RelatingObject", &AttrValue::Ref(whole.0))?;
            (rel, Vec::new())
        }
    };
    parts.push(element);
    let list = AttrValue::List(parts.into_iter().map(|o| AttrValue::Ref(o.0)).collect());
    Ok(ifc.set_attribute(rel, "RelatedObjects", &list)?)
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

/// Places a level written by Reviera (ARC-LVL-001): aggregates a new storey under the building
/// of the other storeys (`IfcRelAggregates`), gives it a placement relative to the building at
/// its elevation, and keeps that placement's height equal to the elevation (in file units) when
/// nothing else is placed relative to it. Storeys of other tools with dependent placements keep
/// theirs, so their contents do not move in other viewers while Reviera shows them in place.
pub(crate) fn write_storey(
    ifc: &mut IfcModel,
    storey: StepId,
    elevation: f64,
    owner_history: Option<StepId>,
) -> Result<()> {
    const REL: &str = "IfcRelAggregates";
    let aggregated = ifc
        .referenced_by(storey, REL)?
        .into_iter()
        .any(|r| entity(ifc, r).is_ok_and(|e| e.refs("RelatedObjects").contains(&storey)));
    let building = building_of_storeys(ifc, storey)?;
    if !aggregated && let Some(building) = building {
        let existing = ifc.referenced_by(building, REL)?.into_iter().find(|r| {
            ifc.attribute(*r, "RelatingObject")
                .is_ok_and(|v| v == AttrValue::Ref(building.0))
        });
        let (rel, mut objects) = match existing {
            Some(rel) => (rel, entity(ifc, rel)?.refs("RelatedObjects")),
            None => {
                let rel = ifc.create_rooted(REL, &GlobalId::new())?;
                if let Some(oh) = owner_history {
                    ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
                }
                ifc.set_attribute(rel, "RelatingObject", &AttrValue::Ref(building.0))?;
                (rel, Vec::new())
            }
        };
        objects.push(storey);
        let list = AttrValue::List(objects.into_iter().map(|o| AttrValue::Ref(o.0)).collect());
        ifc.set_attribute(rel, "RelatedObjects", &list)?;
    }
    if ifc.attribute(storey, "CompositionType")? == AttrValue::Null {
        ifc.set_attribute(
            storey,
            "CompositionType",
            &AttrValue::Enum("ELEMENT".into()),
        )?;
    }

    let placement = match ifc.attribute(storey, "ObjectPlacement")? {
        AttrValue::Ref(p) => StepId(p),
        _ => {
            let point = create_with(
                ifc,
                "IfcCartesianPoint",
                &[("Coordinates", reals(&[0.0, 0.0, elevation]))],
            )?;
            let axes = create_with(
                ifc,
                "IfcAxis2Placement3D",
                &[("Location", AttrValue::Ref(point.0))],
            )?;
            let mut attrs = vec![("RelativePlacement", AttrValue::Ref(axes.0))];
            if let Some(b) = building
                && let AttrValue::Ref(rel_to) = ifc.attribute(b, "ObjectPlacement")?
            {
                attrs.push(("PlacementRelTo", AttrValue::Ref(rel_to)));
            }
            let placement = create_with(ifc, "IfcLocalPlacement", &attrs)?;
            ifc.set_attribute(storey, "ObjectPlacement", &AttrValue::Ref(placement.0))?;
            return Ok(());
        }
    };
    if !ifc
        .referenced_by(placement, "IfcLocalPlacement")?
        .is_empty()
    {
        return Ok(());
    }
    let AttrValue::Ref(axes) = ifc.attribute(placement, "RelativePlacement")? else {
        return Ok(());
    };
    let axes = StepId(axes);
    if !ifc
        .class_of(axes)?
        .eq_ignore_ascii_case("IfcAxis2Placement3D")
    {
        return Ok(());
    }
    let AttrValue::Ref(point) = ifc.attribute(axes, "Location")? else {
        return Ok(());
    };
    let point = StepId(point);
    let AttrValue::List(coords) = ifc.attribute(point, "Coordinates")? else {
        return Ok(());
    };
    let xyz: Vec<f64> = coords
        .iter()
        .filter_map(|c| match c {
            AttrValue::Real(r) => Some(*r),
            _ => None,
        })
        .collect();
    let [x, y, z] = xyz[..] else {
        return Ok(());
    };
    if z == elevation {
        return Ok(());
    }
    let moved = reals(&[x, y, elevation]);
    if ifc.referenced_by(point, "IfcRepresentationItem")?.len() == 1 {
        ifc.set_attribute(point, "Coordinates", &moved)?;
    } else {
        let new = create_with(ifc, "IfcCartesianPoint", &[("Coordinates", moved)])?;
        ifc.set_attribute(axes, "Location", &AttrValue::Ref(new.0))?;
    }
    Ok(())
}

/// The building that aggregates the model's storeys (other than `except`), else the first
/// `IfcBuilding`.
fn building_of_storeys(ifc: &IfcModel, except: StepId) -> Result<Option<StepId>> {
    for s in ifc.entities_of_type("IfcBuildingStorey")? {
        if s.id == except {
            continue;
        }
        for rel in ifc.referenced_by(s.id, "IfcRelAggregates")? {
            let e = entity(ifc, rel)?;
            if !e.refs("RelatedObjects").contains(&s.id) {
                continue;
            }
            if let Some(&parent) = e.refs("RelatingObject").first()
                && ifc.class_of(parent)?.eq_ignore_ascii_case("IfcBuilding")
            {
                return Ok(Some(parent));
            }
        }
    }
    Ok(ifc.entities_of_type("IfcBuilding")?.first().map(|b| b.id))
}

/// One grid line to write as an `IfcGridAxis`: its tag and its line in plan (metres).
pub(crate) struct GridAxis<'a> {
    pub tag: &'a str,
    pub start: [f64; 2],
    pub end: [f64; 2],
}

/// Writes the axes of a grid system written by Reviera (DM-IFC-006): an absolute placement,
/// containment in `storey`, and one `IfcGridAxis` per line with an `IfcPolyline` curve. Lines
/// within ±45° of project X are `UAxes`, the others `VAxes`; when every line runs one way, the
/// last goes to the other list, which IFC4 requires to be non-empty. Earlier axes are replaced.
pub(crate) fn write_grid(
    ifc: &mut IfcModel,
    grid: StepId,
    axes: &[GridAxis<'_>],
    scale: f64,
    storey: Option<StepId>,
    owner_history: Option<StepId>,
) -> Result<()> {
    remove_grid_axes(ifc, grid)?;
    if ifc.attribute(grid, "ObjectPlacement")? == AttrValue::Null {
        let point = create_with(
            ifc,
            "IfcCartesianPoint",
            &[("Coordinates", reals(&[0.0, 0.0, 0.0]))],
        )?;
        let axes = create_with(
            ifc,
            "IfcAxis2Placement3D",
            &[("Location", AttrValue::Ref(point.0))],
        )?;
        let placement = create_with(
            ifc,
            "IfcLocalPlacement",
            &[("RelativePlacement", AttrValue::Ref(axes.0))],
        )?;
        ifc.set_attribute(grid, "ObjectPlacement", &AttrValue::Ref(placement.0))?;
    }
    let (mut u, mut v): (Vec<&GridAxis<'_>>, Vec<&GridAxis<'_>>) = axes
        .iter()
        .partition(|a| (a.end[0] - a.start[0]).abs() >= (a.end[1] - a.start[1]).abs());
    if v.is_empty() && u.len() > 1 {
        v.extend(u.pop());
    } else if u.is_empty() && v.len() > 1 {
        u.extend(v.pop());
    }
    for (attr, list) in [("UAxes", u), ("VAxes", v)] {
        let mut refs = Vec::new();
        for a in list {
            let points = [a.start, a.end]
                .into_iter()
                .map(|p| {
                    create_with(
                        ifc,
                        "IfcCartesianPoint",
                        &[("Coordinates", reals(&[p[0] / scale, p[1] / scale]))],
                    )
                    .map(|id| AttrValue::Ref(id.0))
                })
                .collect::<Result<Vec<_>>>()?;
            let curve = create_with(ifc, "IfcPolyline", &[("Points", AttrValue::List(points))])?;
            let axis = create_with(
                ifc,
                "IfcGridAxis",
                &[
                    ("AxisTag", AttrValue::String(a.tag.to_owned())),
                    ("AxisCurve", AttrValue::Ref(curve.0)),
                    ("SameSense", AttrValue::Bool(true)),
                ],
            )?;
            refs.push(AttrValue::Ref(axis.0));
        }
        ifc.set_attribute(grid, attr, &AttrValue::List(refs))?;
    }
    if let Some(storey) = storey {
        contain(ifc, grid, storey, owner_history)?;
    }
    Ok(())
}

/// Removes the axes of a grid written by [`write_grid`], with their curves and points.
pub(crate) fn remove_grid_axes(ifc: &mut IfcModel, grid: StepId) -> Result<()> {
    let e = entity(ifc, grid)?;
    for attr in ["UAxes", "VAxes", "WAxes"] {
        for axis in e.refs(attr) {
            let a = entity(ifc, axis)?;
            for curve in a.refs("AxisCurve") {
                let points = entity(ifc, curve)?.refs("Points");
                ifc.remove(curve)?;
                for p in points {
                    if ifc.referenced_by(p, "IfcRepresentationItem")?.is_empty() {
                        ifc.remove(p)?;
                    }
                }
            }
            ifc.remove(axis)?;
        }
    }
    Ok(())
}

/// A box in an element's local frame: x, y and z intervals (metres).
pub(crate) type LocalBox = [[f64; 2]; 3];

/// The entities of a shape of boxes written by [`write_boxes`]: its placement and one
/// `IfcExtrudedAreaSolid` of an `IfcRectangleProfileDef` per box.
struct BoxShape {
    placement: StepId,
    axes: StepId,
    location: StepId,
    axis: StepId,
    ref_direction: StepId,
    shape: StepId,
    body_rep: StepId,
    items: Vec<BoxItem>,
}

struct BoxItem {
    solid: StepId,
    profile: StepId,
    profile_axes: StepId,
    profile_location: StepId,
    axes: StepId,
    location: StepId,
    extrusion: StepId,
}

impl BoxShape {
    /// The shape of `element` if it has this structure.
    fn read(ifc: &IfcModel, element: StepId) -> Result<Option<Self>> {
        let e = entity(ifc, element)?;
        let one = |e: &Entity, attr: &str| e.refs(attr).first().copied();
        macro_rules! get {
            ($v:expr) => {
                match $v {
                    Some(v) => v,
                    None => return Ok(None),
                }
            };
        }
        let placement = get!(one(&e, "ObjectPlacement"));
        let p = entity(ifc, placement)?;
        let axes = get!(one(&p, "RelativePlacement"));
        let a = entity(ifc, axes)?;
        let (location, axis, ref_direction) = (
            get!(one(&a, "Location")),
            get!(one(&a, "Axis")),
            get!(one(&a, "RefDirection")),
        );
        let shape = get!(one(&e, "Representation"));
        let reps = entity(ifc, shape)?.refs("Representations");
        if reps.len() != 1 {
            return Ok(None);
        }
        let body_rep = reps[0];
        let mut items = Vec::new();
        for solid in entity(ifc, body_rep)?.refs("Items") {
            let s = entity(ifc, solid)?;
            if !s.class.eq_ignore_ascii_case("IfcExtrudedAreaSolid") {
                return Ok(None);
            }
            let profile = get!(one(&s, "SweptArea"));
            let pr = entity(ifc, profile)?;
            if !pr.class.eq_ignore_ascii_case("IfcRectangleProfileDef") {
                return Ok(None);
            }
            let profile_axes = get!(one(&pr, "Position"));
            let solid_axes = get!(one(&s, "Position"));
            items.push(BoxItem {
                solid,
                profile,
                profile_axes,
                profile_location: get!(one(&entity(ifc, profile_axes)?, "Location")),
                axes: solid_axes,
                location: get!(one(&entity(ifc, solid_axes)?, "Location")),
                extrusion: get!(one(&s, "ExtrudedDirection")),
            });
        }
        Ok(Some(Self {
            placement,
            axes,
            location,
            axis,
            ref_direction,
            shape,
            body_rep,
            items,
        }))
    }

    fn entities(&self) -> Vec<StepId> {
        let mut ids = vec![
            self.placement,
            self.axes,
            self.location,
            self.axis,
            self.ref_direction,
            self.shape,
            self.body_rep,
        ];
        for i in &self.items {
            ids.extend([
                i.solid,
                i.profile,
                i.profile_axes,
                i.profile_location,
                i.axes,
                i.location,
                i.extrusion,
            ]);
        }
        ids
    }

    fn create(ifc: &mut IfcModel, element: StepId, count: usize, body: StepId) -> Result<Self> {
        let r = |id: StepId| AttrValue::Ref(id.0);
        let location = create_with(
            ifc,
            "IfcCartesianPoint",
            &[("Coordinates", reals(&[0.0; 3]))],
        )?;
        let axis = create_with(
            ifc,
            "IfcDirection",
            &[("DirectionRatios", reals(&[0.0, 0.0, 1.0]))],
        )?;
        let ref_direction = create_with(
            ifc,
            "IfcDirection",
            &[("DirectionRatios", reals(&[1.0, 0.0, 0.0]))],
        )?;
        let axes = create_with(
            ifc,
            "IfcAxis2Placement3D",
            &[
                ("Location", r(location)),
                ("Axis", r(axis)),
                ("RefDirection", r(ref_direction)),
            ],
        )?;
        let placement = create_with(ifc, "IfcLocalPlacement", &[("RelativePlacement", r(axes))])?;
        let mut items = Vec::new();
        for _ in 0..count {
            let profile_location = create_with(
                ifc,
                "IfcCartesianPoint",
                &[("Coordinates", reals(&[0.0, 0.0]))],
            )?;
            let profile_axes = create_with(
                ifc,
                "IfcAxis2Placement2D",
                &[("Location", r(profile_location))],
            )?;
            let profile = create_with(
                ifc,
                "IfcRectangleProfileDef",
                &[
                    ("ProfileType", AttrValue::Enum("AREA".into())),
                    ("Position", r(profile_axes)),
                    ("XDim", AttrValue::Real(1.0)),
                    ("YDim", AttrValue::Real(1.0)),
                ],
            )?;
            let location = create_with(
                ifc,
                "IfcCartesianPoint",
                &[("Coordinates", reals(&[0.0; 3]))],
            )?;
            let axes = create_with(ifc, "IfcAxis2Placement3D", &[("Location", r(location))])?;
            let extrusion = create_with(
                ifc,
                "IfcDirection",
                &[("DirectionRatios", reals(&[0.0, 0.0, 1.0]))],
            )?;
            let solid = create_with(
                ifc,
                "IfcExtrudedAreaSolid",
                &[
                    ("SweptArea", r(profile)),
                    ("Position", r(axes)),
                    ("ExtrudedDirection", r(extrusion)),
                    ("Depth", AttrValue::Real(1.0)),
                ],
            )?;
            items.push(BoxItem {
                solid,
                profile,
                profile_axes,
                profile_location,
                axes,
                location,
                extrusion,
            });
        }
        let body_rep = create_with(
            ifc,
            "IfcShapeRepresentation",
            &[
                ("ContextOfItems", r(body)),
                ("RepresentationIdentifier", AttrValue::String("Body".into())),
                ("RepresentationType", AttrValue::String("SweptSolid".into())),
                (
                    "Items",
                    AttrValue::List(items.iter().map(|i| r(i.solid)).collect()),
                ),
            ],
        )?;
        let shape = create_with(
            ifc,
            "IfcProductDefinitionShape",
            &[("Representations", AttrValue::List(vec![r(body_rep)]))],
        )?;
        ifc.set_attribute(element, "ObjectPlacement", &r(placement))?;
        ifc.set_attribute(element, "Representation", &r(shape))?;
        Ok(Self {
            placement,
            axes,
            location,
            axis,
            ref_direction,
            shape,
            body_rep,
            items,
        })
    }
}

/// Where a box shape is placed: relative to another placement (or absolute), at `origin` with
/// its X axis along `x_dir` (file units for `origin`).
pub(crate) struct Frame {
    pub relative_to: Option<StepId>,
    pub origin: [f64; 3],
    pub x_dir: [f64; 2],
}

/// Writes an element's placement and a body of boxes (metres, in the placement's frame),
/// updating the existing shape in place when it has the same number of boxes.
pub(crate) fn write_boxes(
    ifc: &mut IfcModel,
    element: StepId,
    frame: &Frame,
    boxes: &[LocalBox],
    scale: f64,
    contexts: Contexts,
) -> Result<()> {
    // A profile shape from before (the insert was shaped) goes.
    if let Some(old) = ProfileShape::read(ifc, element)? {
        for id in old.placement.into_iter().chain(old.rep) {
            ifc.remove(id)?;
        }
    }
    let shape = match BoxShape::read(ifc, element)? {
        Some(s) if s.items.len() == boxes.len() => s,
        old => {
            if let Some(old) = old {
                for id in old.entities() {
                    ifc.remove(id)?;
                }
            }
            BoxShape::create(ifc, element, boxes.len(), contexts.body)?
        }
    };
    let set = |ifc: &mut IfcModel, id: StepId, attr: &str, v: AttrValue| -> Result<()> {
        if ifc.attribute(id, attr)? != v {
            ifc.set_attribute(id, attr, &v)?;
        }
        Ok(())
    };
    let rel = frame
        .relative_to
        .map_or(AttrValue::Null, |p| AttrValue::Ref(p.0));
    set(ifc, shape.placement, "PlacementRelTo", rel)?;
    set(ifc, shape.location, "Coordinates", reals(&frame.origin))?;
    set(ifc, shape.axis, "DirectionRatios", reals(&[0.0, 0.0, 1.0]))?;
    set(
        ifc,
        shape.ref_direction,
        "DirectionRatios",
        reals(&[frame.x_dir[0], frame.x_dir[1], 0.0]),
    )?;
    for (item, [x, y, z]) in shape.items.iter().zip(boxes) {
        let s = |v: f64| v / scale;
        set(
            ifc,
            item.profile_location,
            "Coordinates",
            reals(&[s((x[0] + x[1]) / 2.0), s((y[0] + y[1]) / 2.0)]),
        )?;
        set(ifc, item.profile, "XDim", AttrValue::Real(s(x[1] - x[0])))?;
        set(ifc, item.profile, "YDim", AttrValue::Real(s(y[1] - y[0])))?;
        set(
            ifc,
            item.location,
            "Coordinates",
            reals(&[0.0, 0.0, s(z[0])]),
        )?;
        set(
            ifc,
            item.extrusion,
            "DirectionRatios",
            reals(&[0.0, 0.0, 1.0]),
        )?;
        set(ifc, item.solid, "Depth", AttrValue::Real(s(z[1] - z[0])))?;
    }
    Ok(())
}

/// Removes a shape written by [`write_boxes`] and the element's voids and fills relationships
/// (before the element itself is removed).
pub(crate) fn remove_boxes(ifc: &mut IfcModel, element: StepId) -> Result<()> {
    for class in ["IfcRelVoidsElement", "IfcRelFillsElement"] {
        for rel in ifc.referenced_by(element, class)? {
            ifc.remove(rel)?;
        }
    }
    if let Some(shape) = BoxShape::read(ifc, element)? {
        for id in shape.entities() {
            ifc.remove(id)?;
        }
    }
    if let Some(shape) = ProfileShape::read(ifc, element)? {
        for id in shape.placement.into_iter().chain(shape.rep) {
            ifc.remove(id)?;
        }
    }
    Ok(())
}

/// A profile of a shaped door, window or opening (ARC-WIN-002), in the element's plane: x along
/// it and z up (metres).
pub(crate) enum Profile<'a> {
    Circle {
        centre: [f64; 2],
        radius: f64,
    },
    Rings {
        outer: &'a [[f64; 2]],
        holes: Vec<&'a [[f64; 2]]>,
    },
}

/// A profile extruded across the element's plane, through `y` (metres); or, with `plan`, a
/// plan profile (x, y) extruded up through the heights `y`.
pub(crate) struct ProfileSolid<'a> {
    pub profile: Profile<'a>,
    pub y: [f64; 2],
    pub plan: bool,
}

/// What a profile shape holds, in file units, to compare a written shape with: per solid its
/// depth and start, then a circle's centre and radius or each ring's points (closed).
fn profile_signature(solids: &[ProfileSolid<'_>], scale: f64) -> Vec<f64> {
    let mut s = vec![solids.len() as f64];
    for sol in solids {
        let (y, z) = if sol.plan {
            (0.0, sol.y[0] / scale)
        } else {
            (sol.y[1] / scale, 0.0)
        };
        s.extend([
            (sol.y[1] - sol.y[0]) / scale,
            y,
            z,
            f64::from(u8::from(sol.plan)),
        ]);
        match &sol.profile {
            Profile::Circle { centre, radius } => {
                s.extend([1.0, centre[0] / scale, centre[1] / scale, radius / scale]);
            }
            Profile::Rings { outer, holes } => {
                s.extend([2.0, 1.0 + holes.len() as f64]);
                for ring in std::iter::once(outer).chain(holes.iter()) {
                    s.push(ring.len() as f64 + 1.0);
                    for p in ring.iter().chain(ring.first()) {
                        s.extend([p[0] / scale, p[1] / scale]);
                    }
                }
            }
        }
    }
    s
}

fn coords(ifc: &IfcModel, id: StepId) -> Result<Vec<f64>> {
    Ok(match ifc.attribute(id, "Coordinates")? {
        AttrValue::List(c) => c
            .iter()
            .filter_map(|v| match v {
                AttrValue::Real(r) => Some(*r),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

/// The entities of a shape written by [`write_profiles`]: its placement (placement, axes,
/// location, axis, ref direction), the rest of its representation, and its signature.
struct ProfileShape {
    placement: [StepId; 5],
    rep: Vec<StepId>,
    signature: Vec<f64>,
}

impl ProfileShape {
    fn read(ifc: &IfcModel, element: StepId) -> Result<Option<Self>> {
        let e = entity(ifc, element)?;
        let one = |e: &Entity, attr: &str| e.refs(attr).first().copied();
        macro_rules! get {
            ($v:expr) => {
                match $v {
                    Some(v) => v,
                    None => return Ok(None),
                }
            };
        }
        let placement = get!(one(&e, "ObjectPlacement"));
        let axes = get!(one(&entity(ifc, placement)?, "RelativePlacement"));
        let a = entity(ifc, axes)?;
        let placement = [
            placement,
            axes,
            get!(one(&a, "Location")),
            get!(one(&a, "Axis")),
            get!(one(&a, "RefDirection")),
        ];
        let shape = get!(one(&e, "Representation"));
        let reps = entity(ifc, shape)?.refs("Representations");
        if reps.len() != 1 {
            return Ok(None);
        }
        let mut rep = vec![shape, reps[0]];
        let solids = entity(ifc, reps[0])?.refs("Items");
        let mut signature = vec![solids.len() as f64];
        for solid in solids {
            let s = entity(ifc, solid)?;
            if !s.class.eq_ignore_ascii_case("IfcExtrudedAreaSolid") {
                return Ok(None);
            }
            let profile = get!(one(&s, "SweptArea"));
            let pr = entity(ifc, profile)?;
            let position = get!(one(&s, "Position"));
            let pos = entity(ifc, position)?;
            let pos_location = get!(one(&pos, "Location"));
            let depth = match s.get("Depth") {
                AttrValue::Real(d) => *d,
                _ => 0.0,
            };
            let at = coords(ifc, pos_location)?;
            let plan = match one(&pos, "Axis") {
                Some(a) => match ifc.attribute(a, "DirectionRatios")? {
                    AttrValue::List(d) => matches!(d.get(2), Some(AttrValue::Real(z)) if *z > 0.5),
                    _ => false,
                },
                None => false,
            };
            signature.extend([
                depth,
                at.get(1).copied().unwrap_or(0.0),
                at.get(2).copied().unwrap_or(0.0),
                f64::from(u8::from(plan)),
            ]);
            rep.extend([solid, profile, position, pos_location]);
            rep.extend(one(&pos, "Axis"));
            rep.extend(one(&pos, "RefDirection"));
            rep.extend(one(&s, "ExtrudedDirection"));
            if pr.class.eq_ignore_ascii_case("IfcCircleProfileDef") {
                let p2 = get!(one(&pr, "Position"));
                let centre = get!(one(&entity(ifc, p2)?, "Location"));
                let c = coords(ifc, centre)?;
                let radius = match pr.get("Radius") {
                    AttrValue::Real(r) => *r,
                    _ => 0.0,
                };
                signature.extend([1.0, c.first().copied().unwrap_or(0.0)]);
                signature.extend([c.get(1).copied().unwrap_or(0.0), radius]);
                rep.extend([p2, centre]);
            } else if pr
                .class
                .eq_ignore_ascii_case("IfcArbitraryClosedProfileDef")
                || pr
                    .class
                    .eq_ignore_ascii_case("IfcArbitraryProfileDefWithVoids")
            {
                let curves: Vec<StepId> = pr
                    .refs("OuterCurve")
                    .into_iter()
                    .chain(pr.refs("InnerCurves"))
                    .collect();
                signature.extend([2.0, curves.len() as f64]);
                for c in curves {
                    let points = entity(ifc, c)?.refs("Points");
                    rep.push(c);
                    signature.push(points.len() as f64);
                    for p in &points {
                        signature.extend(coords(ifc, *p)?);
                    }
                    rep.extend(points);
                }
            } else {
                return Ok(None);
            }
        }
        rep.sort_by_key(|s| s.0);
        rep.dedup();
        Ok(Some(Self {
            placement,
            rep,
            signature,
        }))
    }
}

/// Writes an element's placement and a body of profiles extruded across its plane (a shaped
/// door, window or opening, ARC-WIN-002), unless it already has exactly this geometry. A box
/// shape written before is replaced.
pub(crate) fn write_profiles(
    ifc: &mut IfcModel,
    element: StepId,
    frame: &Frame,
    solids: &[ProfileSolid<'_>],
    scale: f64,
    contexts: Contexts,
) -> Result<()> {
    if let Some(old) = BoxShape::read(ifc, element)? {
        for id in old.entities() {
            ifc.remove(id)?;
        }
    }
    let r = |id: StepId| AttrValue::Ref(id.0);
    let existing = ProfileShape::read(ifc, element)?;
    let [placement, _, location, axis, ref_direction] = match &existing {
        Some(e) => e.placement,
        None => {
            let location = create_with(
                ifc,
                "IfcCartesianPoint",
                &[("Coordinates", reals(&[0.0; 3]))],
            )?;
            let axis = create_with(
                ifc,
                "IfcDirection",
                &[("DirectionRatios", reals(&[0.0, 0.0, 1.0]))],
            )?;
            let ref_direction = create_with(
                ifc,
                "IfcDirection",
                &[("DirectionRatios", reals(&[1.0, 0.0, 0.0]))],
            )?;
            let axes = create_with(
                ifc,
                "IfcAxis2Placement3D",
                &[
                    ("Location", r(location)),
                    ("Axis", r(axis)),
                    ("RefDirection", r(ref_direction)),
                ],
            )?;
            let placement =
                create_with(ifc, "IfcLocalPlacement", &[("RelativePlacement", r(axes))])?;
            ifc.set_attribute(element, "ObjectPlacement", &r(placement))?;
            [placement, axes, location, axis, ref_direction]
        }
    };
    let set = |ifc: &mut IfcModel, id: StepId, attr: &str, v: AttrValue| -> Result<()> {
        if ifc.attribute(id, attr)? != v {
            ifc.set_attribute(id, attr, &v)?;
        }
        Ok(())
    };
    let rel = frame
        .relative_to
        .map_or(AttrValue::Null, |p| AttrValue::Ref(p.0));
    set(ifc, placement, "PlacementRelTo", rel)?;
    set(ifc, location, "Coordinates", reals(&frame.origin))?;
    set(ifc, axis, "DirectionRatios", reals(&[0.0, 0.0, 1.0]))?;
    set(
        ifc,
        ref_direction,
        "DirectionRatios",
        reals(&[frame.x_dir[0], frame.x_dir[1], 0.0]),
    )?;
    let target = profile_signature(solids, scale);
    if let Some(e) = &existing {
        if e.signature.len() == target.len()
            && e.signature
                .iter()
                .zip(&target)
                .all(|(a, b)| (a - b).abs() < 1e-9)
        {
            return Ok(());
        }
        for id in &e.rep {
            ifc.remove(*id)?;
        }
    }
    let point = |ifc: &mut IfcModel, c: &[f64]| {
        create_with(ifc, "IfcCartesianPoint", &[("Coordinates", reals(c))])
    };
    let direction = |ifc: &mut IfcModel, d: [f64; 3]| {
        create_with(ifc, "IfcDirection", &[("DirectionRatios", reals(&d))])
    };
    let mut items = Vec::new();
    for sol in solids {
        let profile = match &sol.profile {
            Profile::Circle { centre, radius } => {
                let c = point(ifc, &[centre[0] / scale, centre[1] / scale])?;
                let p2 = create_with(ifc, "IfcAxis2Placement2D", &[("Location", r(c))])?;
                create_with(
                    ifc,
                    "IfcCircleProfileDef",
                    &[
                        ("ProfileType", AttrValue::Enum("AREA".into())),
                        ("Position", r(p2)),
                        ("Radius", AttrValue::Real(radius / scale)),
                    ],
                )?
            }
            Profile::Rings { outer, holes } => {
                let polyline = |ifc: &mut IfcModel, ring: &[[f64; 2]]| -> Result<StepId> {
                    let points = ring
                        .iter()
                        .chain(ring.first())
                        .map(|p| point(ifc, &[p[0] / scale, p[1] / scale]).map(r))
                        .collect::<Result<Vec<_>>>()?;
                    create_with(ifc, "IfcPolyline", &[("Points", AttrValue::List(points))])
                };
                let outer_curve = polyline(ifc, outer)?;
                if holes.is_empty() {
                    create_with(
                        ifc,
                        "IfcArbitraryClosedProfileDef",
                        &[
                            ("ProfileType", AttrValue::Enum("AREA".into())),
                            ("OuterCurve", r(outer_curve)),
                        ],
                    )?
                } else {
                    let inner = holes
                        .iter()
                        .map(|h| polyline(ifc, h).map(r))
                        .collect::<Result<Vec<_>>>()?;
                    create_with(
                        ifc,
                        "IfcArbitraryProfileDefWithVoids",
                        &[
                            ("ProfileType", AttrValue::Enum("AREA".into())),
                            ("OuterCurve", r(outer_curve)),
                            ("InnerCurves", AttrValue::List(inner)),
                        ],
                    )?
                }
            }
        };
        // The profile's x along the element, its y up; extruded from `y[1]` toward -y. A plan
        // profile lies flat and rises from `y[0]`.
        let (start, z) = if sol.plan {
            (
                point(ifc, &[0.0, 0.0, sol.y[0] / scale])?,
                direction(ifc, [0.0, 0.0, 1.0])?,
            )
        } else {
            (
                point(ifc, &[0.0, sol.y[1] / scale, 0.0])?,
                direction(ifc, [0.0, -1.0, 0.0])?,
            )
        };
        let x = direction(ifc, [1.0, 0.0, 0.0])?;
        let position = create_with(
            ifc,
            "IfcAxis2Placement3D",
            &[
                ("Location", r(start)),
                ("Axis", r(z)),
                ("RefDirection", r(x)),
            ],
        )?;
        let up = direction(ifc, [0.0, 0.0, 1.0])?;
        items.push(r(create_with(
            ifc,
            "IfcExtrudedAreaSolid",
            &[
                ("SweptArea", r(profile)),
                ("Position", r(position)),
                ("ExtrudedDirection", r(up)),
                ("Depth", AttrValue::Real((sol.y[1] - sol.y[0]) / scale)),
            ],
        )?));
    }
    let body = create_with(
        ifc,
        "IfcShapeRepresentation",
        &[
            ("ContextOfItems", r(contexts.body)),
            ("RepresentationIdentifier", AttrValue::String("Body".into())),
            ("RepresentationType", AttrValue::String("SweptSolid".into())),
            ("Items", AttrValue::List(items)),
        ],
    )?;
    let shape = create_with(
        ifc,
        "IfcProductDefinitionShape",
        &[("Representations", AttrValue::List(vec![r(body)]))],
    )?;
    ifc.set_attribute(element, "Representation", &r(shape))?;
    Ok(())
}

/// Makes `relating` the only relating element of the one-to-one relationship `class` that
/// `related` takes part in (voids and fills), creating it when missing.
pub(crate) fn relate_one(
    ifc: &mut IfcModel,
    class: &str,
    relating_attr: &str,
    relating: StepId,
    related_attr: &str,
    related: StepId,
    owner_history: Option<StepId>,
) -> Result<()> {
    let existing = ifc.referenced_by(related, class)?.into_iter().find(|r| {
        ifc.attribute(*r, related_attr)
            .is_ok_and(|v| v == AttrValue::Ref(related.0))
    });
    let rel = match existing {
        Some(rel) => rel,
        None => {
            let rel = ifc.create_rooted(class, &GlobalId::new())?;
            if let Some(oh) = owner_history {
                ifc.set_attribute(rel, "OwnerHistory", &AttrValue::Ref(oh.0))?;
            }
            ifc.set_attribute(rel, related_attr, &AttrValue::Ref(related.0))?;
            rel
        }
    };
    if ifc.attribute(rel, relating_attr)? != AttrValue::Ref(relating.0) {
        ifc.set_attribute(rel, relating_attr, &AttrValue::Ref(relating.0))?;
    }
    Ok(())
}

/// The entities of a slab body written by [`write_slab`], and its geometry in file units
/// (placement height, depth, and each solid's rings of points) to compare with.
struct SlabShape {
    entities: Vec<StepId>,
    signature: Vec<f64>,
}

impl SlabShape {
    fn read(ifc: &IfcModel, element: StepId) -> Result<Option<Self>> {
        let e = entity(ifc, element)?;
        let one = |e: &Entity, attr: &str| e.refs(attr).first().copied();
        macro_rules! get {
            ($v:expr) => {
                match $v {
                    Some(v) => v,
                    None => return Ok(None),
                }
            };
        }
        let coords = |id: StepId| -> Result<Vec<f64>> {
            Ok(match ifc.attribute(id, "Coordinates")? {
                AttrValue::List(c) => c
                    .iter()
                    .filter_map(|v| match v {
                        AttrValue::Real(r) => Some(*r),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            })
        };
        let placement = get!(one(&e, "ObjectPlacement"));
        let axes = get!(one(&entity(ifc, placement)?, "RelativePlacement"));
        let location = get!(one(&entity(ifc, axes)?, "Location"));
        let shape = get!(one(&e, "Representation"));
        let reps = entity(ifc, shape)?.refs("Representations");
        if reps.len() != 1 {
            return Ok(None);
        }
        let mut entities = vec![placement, axes, location, shape, reps[0]];
        let z = coords(location)?.get(2).copied().unwrap_or(0.0);
        let mut signature = vec![z];
        let solids = entity(ifc, reps[0])?.refs("Items");
        signature.push(solids.len() as f64);
        for solid in solids {
            let s = entity(ifc, solid)?;
            if !s.class.eq_ignore_ascii_case("IfcExtrudedAreaSolid") {
                return Ok(None);
            }
            let profile = get!(one(&s, "SweptArea"));
            let pr = entity(ifc, profile)?;
            let position = get!(one(&s, "Position"));
            let position_location = get!(one(&entity(ifc, position)?, "Location"));
            let direction = get!(one(&s, "ExtrudedDirection"));
            entities.extend([solid, profile, position, position_location, direction]);
            if let AttrValue::Real(d) = s.get("Depth") {
                signature.push(*d);
            }
            let curves: Vec<StepId> = pr
                .refs("OuterCurve")
                .into_iter()
                .chain(pr.refs("InnerCurves"))
                .collect();
            signature.push(curves.len() as f64);
            for c in curves {
                let points = entity(ifc, c)?.refs("Points");
                entities.push(c);
                signature.push(points.len() as f64);
                for p in &points {
                    signature.extend(coords(*p)?);
                }
                entities.extend(points);
            }
        }
        entities.sort_by_key(|s| s.0);
        entities.dedup();
        Ok(Some(Self {
            entities,
            signature,
        }))
    }
}

/// An outer boundary and its holes (points in metres).
pub(crate) type Rings<'a> = (&'a [[f64; 2]], Vec<&'a [[f64; 2]]>);

/// A slab's body: one solid per outer boundary (points in metres, rings closed by repeating the
/// first point), extruded down by `depth` from a placement at `top`.
pub(crate) struct SlabBody<'a> {
    pub rings: Vec<Rings<'a>>,
    pub top: f64,
    pub depth: f64,
}

impl SlabBody<'_> {
    fn signature(&self, scale: f64) -> Vec<f64> {
        let mut s = vec![self.top / scale, self.rings.len() as f64];
        for (outer, holes) in &self.rings {
            s.push(self.depth / scale);
            s.push(1.0 + holes.len() as f64);
            for ring in std::iter::once(outer).chain(holes.iter()) {
                s.push(ring.len() as f64 + 1.0);
                for p in ring.iter().chain(ring.first()) {
                    s.extend([p[0] / scale, p[1] / scale]);
                }
            }
        }
        s
    }
}

/// Writes a slab's placement and body (a Reviera floor), unless it already has exactly this
/// geometry.
pub(crate) fn write_slab(
    ifc: &mut IfcModel,
    element: StepId,
    body: &SlabBody<'_>,
    scale: f64,
    contexts: Contexts,
) -> Result<()> {
    let existing = SlabShape::read(ifc, element)?;
    let target = body.signature(scale);
    if let Some(e) = &existing
        && e.signature.len() == target.len()
        && e.signature
            .iter()
            .zip(&target)
            .all(|(a, b)| (a - b).abs() < 1e-9)
    {
        return Ok(());
    }
    if let Some(old) = existing {
        for id in old.entities {
            ifc.remove(id)?;
        }
    }
    let r = |id: StepId| AttrValue::Ref(id.0);
    let location = create_with(
        ifc,
        "IfcCartesianPoint",
        &[("Coordinates", reals(&[0.0, 0.0, body.top / scale]))],
    )?;
    let axes = create_with(ifc, "IfcAxis2Placement3D", &[("Location", r(location))])?;
    let placement = create_with(ifc, "IfcLocalPlacement", &[("RelativePlacement", r(axes))])?;
    let mut solids = Vec::new();
    for (outer, holes) in &body.rings {
        let polyline = |ifc: &mut IfcModel, ring: &[[f64; 2]]| -> Result<StepId> {
            let points = ring
                .iter()
                .chain(ring.first())
                .map(|p| {
                    create_with(
                        ifc,
                        "IfcCartesianPoint",
                        &[("Coordinates", reals(&[p[0] / scale, p[1] / scale]))],
                    )
                    .map(r)
                })
                .collect::<Result<Vec<_>>>()?;
            create_with(ifc, "IfcPolyline", &[("Points", AttrValue::List(points))])
        };
        let outer_curve = polyline(ifc, outer)?;
        let profile = if holes.is_empty() {
            create_with(
                ifc,
                "IfcArbitraryClosedProfileDef",
                &[
                    ("ProfileType", AttrValue::Enum("AREA".into())),
                    ("OuterCurve", r(outer_curve)),
                ],
            )?
        } else {
            let inner = holes
                .iter()
                .map(|h| polyline(ifc, h).map(r))
                .collect::<Result<Vec<_>>>()?;
            create_with(
                ifc,
                "IfcArbitraryProfileDefWithVoids",
                &[
                    ("ProfileType", AttrValue::Enum("AREA".into())),
                    ("OuterCurve", r(outer_curve)),
                    ("InnerCurves", AttrValue::List(inner)),
                ],
            )?
        };
        let position_location = create_with(
            ifc,
            "IfcCartesianPoint",
            &[("Coordinates", reals(&[0.0, 0.0, 0.0]))],
        )?;
        let position = create_with(
            ifc,
            "IfcAxis2Placement3D",
            &[("Location", r(position_location))],
        )?;
        let direction = create_with(
            ifc,
            "IfcDirection",
            &[("DirectionRatios", reals(&[0.0, 0.0, -1.0]))],
        )?;
        solids.push(r(create_with(
            ifc,
            "IfcExtrudedAreaSolid",
            &[
                ("SweptArea", r(profile)),
                ("Position", r(position)),
                ("ExtrudedDirection", r(direction)),
                ("Depth", AttrValue::Real(body.depth / scale)),
            ],
        )?));
    }
    let rep = create_with(
        ifc,
        "IfcShapeRepresentation",
        &[
            ("ContextOfItems", r(contexts.body)),
            ("RepresentationIdentifier", AttrValue::String("Body".into())),
            ("RepresentationType", AttrValue::String("SweptSolid".into())),
            ("Items", AttrValue::List(solids)),
        ],
    )?;
    let shape = create_with(
        ifc,
        "IfcProductDefinitionShape",
        &[("Representations", AttrValue::List(vec![r(rep)]))],
    )?;
    ifc.set_attribute(element, "ObjectPlacement", &r(placement))?;
    ifc.set_attribute(element, "Representation", &r(shape))?;
    Ok(())
}

/// Removes a slab body written by [`write_slab`] (before the slab itself is removed).
pub(crate) fn remove_slab(ifc: &mut IfcModel, element: StepId) -> Result<()> {
    if let Some(shape) = SlabShape::read(ifc, element)? {
        for id in shape.entities {
            ifc.remove(id)?;
        }
    }
    Ok(())
}

/// A polyhedron: points (metres) and faces as loops of point indices, each counter-clockwise
/// seen from outside.
pub(crate) struct Polyhedron {
    pub points: Vec<[f64; 3]>,
    pub faces: Vec<Vec<usize>>,
}

impl Polyhedron {
    fn signature(&self, scale: f64) -> Vec<f64> {
        let mut s = vec![self.faces.len() as f64];
        for f in &self.faces {
            s.push(f.len() as f64);
            for i in f {
                s.extend(self.points[*i].map(|v| v / scale));
            }
        }
        s
    }
}

/// The entities of a body written by [`write_breps`] and its geometry in file units.
struct BrepShape {
    entities: Vec<StepId>,
    signature: Vec<f64>,
}

impl BrepShape {
    fn read(ifc: &IfcModel, element: StepId) -> Result<Option<Self>> {
        let e = entity(ifc, element)?;
        let one = |e: &Entity, attr: &str| e.refs(attr).first().copied();
        macro_rules! get {
            ($v:expr) => {
                match $v {
                    Some(v) => v,
                    None => return Ok(None),
                }
            };
        }
        let placement = get!(one(&e, "ObjectPlacement"));
        let axes = get!(one(&entity(ifc, placement)?, "RelativePlacement"));
        let ae = entity(ifc, axes)?;
        let location = get!(one(&ae, "Location"));
        let shape = get!(one(&e, "Representation"));
        let reps = entity(ifc, shape)?.refs("Representations");
        if reps.len() != 1 {
            return Ok(None);
        }
        let mut entities = vec![placement, axes, location, shape, reps[0]];
        let breps = entity(ifc, reps[0])?.refs("Items");
        let mut signature: Vec<f64> = match ifc.attribute(location, "Coordinates")? {
            AttrValue::List(c) => c
                .iter()
                .filter_map(|v| match v {
                    AttrValue::Real(r) => Some(*r),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let mut x_dir = [1.0, 0.0];
        if let Some(d) = one(&ae, "RefDirection") {
            if let AttrValue::List(c) = ifc.attribute(d, "DirectionRatios")? {
                let v: Vec<f64> = c
                    .iter()
                    .filter_map(|v| match v {
                        AttrValue::Real(r) => Some(*r),
                        _ => None,
                    })
                    .collect();
                if v.len() >= 2 {
                    x_dir = [v[0], v[1]];
                }
            }
            entities.push(d);
        }
        entities.extend(one(&ae, "Axis"));
        signature.extend(x_dir);
        for b in breps {
            let be = entity(ifc, b)?;
            if !be.class.eq_ignore_ascii_case("IfcFacetedBrep") {
                return Ok(None);
            }
            let shell = get!(one(&be, "Outer"));
            let faces = entity(ifc, shell)?.refs("CfsFaces");
            entities.extend([b, shell]);
            signature.push(faces.len() as f64);
            let mut points = Vec::new();
            for f in faces {
                let bound = get!(one(&entity(ifc, f)?, "Bounds"));
                let poly = get!(one(&entity(ifc, bound)?, "Bound"));
                let pts = entity(ifc, poly)?.refs("Polygon");
                entities.extend([f, bound, poly]);
                signature.push(pts.len() as f64);
                for p in &pts {
                    if let AttrValue::List(c) = ifc.attribute(*p, "Coordinates")? {
                        signature.extend(c.iter().filter_map(|v| match v {
                            AttrValue::Real(r) => Some(*r),
                            _ => None,
                        }));
                    }
                }
                points.extend(pts);
            }
            entities.extend(points);
        }
        entities.sort_by_key(|s| s.0);
        entities.dedup();
        Ok(Some(Self {
            entities,
            signature,
        }))
    }
}

/// Where a B-rep body is placed: its origin (metres) and X direction in plan. Solids are given
/// in this frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BrepFrame {
    pub origin: [f64; 3],
    pub x_dir: [f64; 2],
}

impl BrepFrame {
    pub const WORLD: BrepFrame = BrepFrame {
        origin: [0.0; 3],
        x_dir: [1.0, 0.0],
    };
}

/// Writes an element's body as faceted B-reps (a Reviera roof, or a wall attached to one)
/// placed in `frame`, unless it already has exactly this geometry.
pub(crate) fn write_breps(
    ifc: &mut IfcModel,
    element: StepId,
    frame: BrepFrame,
    solids: &[Polyhedron],
    scale: f64,
    contexts: Contexts,
) -> Result<()> {
    let existing = BrepShape::read(ifc, element)?;
    let mut target: Vec<f64> = frame.origin.iter().map(|v| v / scale).collect();
    target.extend(frame.x_dir);
    target.extend(solids.iter().flat_map(|s| s.signature(scale)));
    if let Some(e) = &existing
        && e.signature.len() == target.len()
        && e.signature
            .iter()
            .zip(&target)
            .all(|(a, b)| (a - b).abs() < 1e-9)
    {
        return Ok(());
    }
    if let Some(old) = existing {
        for id in old.entities {
            ifc.remove(id)?;
        }
    } else {
        remove_slab(ifc, element)?;
    }
    let r = |id: StepId| AttrValue::Ref(id.0);
    let location = create_with(
        ifc,
        "IfcCartesianPoint",
        &[("Coordinates", reals(&frame.origin.map(|v| v / scale)))],
    )?;
    let axes = if frame.x_dir == [1.0, 0.0] {
        create_with(ifc, "IfcAxis2Placement3D", &[("Location", r(location))])?
    } else {
        let up = create_with(
            ifc,
            "IfcDirection",
            &[("DirectionRatios", reals(&[0.0, 0.0, 1.0]))],
        )?;
        let x = create_with(
            ifc,
            "IfcDirection",
            &[(
                "DirectionRatios",
                reals(&[frame.x_dir[0], frame.x_dir[1], 0.0]),
            )],
        )?;
        create_with(
            ifc,
            "IfcAxis2Placement3D",
            &[
                ("Location", r(location)),
                ("Axis", r(up)),
                ("RefDirection", r(x)),
            ],
        )?
    };
    let placement = create_with(ifc, "IfcLocalPlacement", &[("RelativePlacement", r(axes))])?;
    let mut items = Vec::new();
    for s in solids {
        let points = s
            .points
            .iter()
            .map(|p| {
                create_with(
                    ifc,
                    "IfcCartesianPoint",
                    &[("Coordinates", reals(&p.map(|v| v / scale)))],
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let mut faces = Vec::new();
        for f in &s.faces {
            let poly = create_with(
                ifc,
                "IfcPolyLoop",
                &[(
                    "Polygon",
                    AttrValue::List(f.iter().map(|i| r(points[*i])).collect()),
                )],
            )?;
            let bound = create_with(
                ifc,
                "IfcFaceOuterBound",
                &[("Bound", r(poly)), ("Orientation", AttrValue::Bool(true))],
            )?;
            faces.push(r(create_with(
                ifc,
                "IfcFace",
                &[("Bounds", AttrValue::List(vec![r(bound)]))],
            )?));
        }
        let shell = create_with(
            ifc,
            "IfcClosedShell",
            &[("CfsFaces", AttrValue::List(faces))],
        )?;
        items.push(r(create_with(
            ifc,
            "IfcFacetedBrep",
            &[("Outer", r(shell))],
        )?));
    }
    let rep = create_with(
        ifc,
        "IfcShapeRepresentation",
        &[
            ("ContextOfItems", r(contexts.body)),
            ("RepresentationIdentifier", AttrValue::String("Body".into())),
            ("RepresentationType", AttrValue::String("Brep".into())),
            ("Items", AttrValue::List(items)),
        ],
    )?;
    let shape = create_with(
        ifc,
        "IfcProductDefinitionShape",
        &[("Representations", AttrValue::List(vec![r(rep)]))],
    )?;
    ifc.set_attribute(element, "ObjectPlacement", &r(placement))?;
    ifc.set_attribute(element, "Representation", &r(shape))?;
    Ok(())
}

/// Removes a body written by [`write_breps`] (before the element itself is removed).
pub(crate) fn remove_breps(ifc: &mut IfcModel, element: StepId) -> Result<()> {
    if let Some(shape) = BrepShape::read(ifc, element)? {
        for id in shape.entities {
            ifc.remove(id)?;
        }
    }
    Ok(())
}
