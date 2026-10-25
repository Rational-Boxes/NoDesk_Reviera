// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The project location in IFC (DM-LOC-001), written on every save from `rxd:location`:
//! `IfcSite.RefLatitude`, `RefLongitude` and `RefElevation`; the model context's `TrueNorth`
//! (a direction in its plane); and, with a CRS and on IFC4 and later, an `IfcMapConversion`
//! from the model context to an `IfcProjectedCRS` (both replaced each save). On open, a file
//! from another tool with a latitude and longitude on its site and no Reviera location gives
//! its location the site's values.

use reviera_core::location::{self, Location};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::Result;

fn r(id: StepId) -> AttrValue {
    AttrValue::Ref(id.0)
}

fn reals(v: &[f64]) -> AttrValue {
    AttrValue::List(v.iter().copied().map(AttrValue::Real).collect())
}

fn compound(deg: f64) -> AttrValue {
    AttrValue::List(location::compound(deg).iter().map(|v| AttrValue::Int(*v)).collect())
}

/// The 3D model context (not a sub-context).
fn model_context(ifc: &IfcModel) -> Result<Option<StepId>> {
    for e in ifc.entities_of_type("IfcGeometricRepresentationContext")? {
        if e.class.eq_ignore_ascii_case("IfcGeometricRepresentationContext") && matches!(ifc.attribute(e.id, "ContextType"), Ok(AttrValue::String(s)) if s == "Model") {
            return Ok(Some(e.id));
        }
    }
    Ok(None)
}

pub(crate) fn write_location(ifc: &mut IfcModel, l: &Location, scale: f64) -> Result<()> {
    if let Some(site) = ifc.entities_of_type("IfcSite")?.first().map(|e| e.id) {
        match (l.latitude, l.longitude) {
            (Some(lat), Some(lon)) => {
                ifc.set_attribute(site, "RefLatitude", &compound(lat))?;
                ifc.set_attribute(site, "RefLongitude", &compound(lon))?;
                ifc.set_attribute(site, "RefElevation", &AttrValue::Real(l.elevation / scale))?;
            }
            _ => {
                for a in ["RefLatitude", "RefLongitude", "RefElevation"] {
                    ifc.set_attribute(site, a, &AttrValue::Null)?;
                }
            }
        }
    }
    let Some(context) = model_context(ifc)? else { return Ok(()) };
    // True north: the old direction goes when it is ours alone.
    if let AttrValue::Ref(old) = ifc.attribute(context, "TrueNorth")? {
        ifc.set_attribute(context, "TrueNorth", &AttrValue::Null)?;
        if ifc.referenced_by(StepId(old), "IfcGeometricRepresentationContext").is_ok_and(|v| v.is_empty()) {
            let _ = ifc.remove(StepId(old));
        }
    }
    if l.true_north.abs() > 1e-12 {
        let d = l.true_north_direction();
        let dir = ifc.create("IfcDirection")?;
        ifc.set_attribute(dir, "DirectionRatios", &reals(&[d[0], d[1]]))?;
        ifc.set_attribute(context, "TrueNorth", &r(dir))?;
    }
    if ifc.schema().to_uppercase().starts_with("IFC2X3") {
        return Ok(());
    }
    // The map conversion and its CRS, written again.
    for mc in ifc.entities_of_type("IfcMapConversion")? {
        if ifc.attribute(mc.id, "SourceCRS")? == r(context) {
            if let AttrValue::Ref(crs) = ifc.attribute(mc.id, "TargetCRS")? {
                if let AttrValue::Ref(unit) = ifc.attribute(StepId(crs), "MapUnit").unwrap_or(AttrValue::Null) {
                    let _ = ifc.remove(StepId(unit));
                }
                ifc.remove(mc.id)?;
                let _ = ifc.remove(StepId(crs));
            } else {
                ifc.remove(mc.id)?;
            }
        }
    }
    let Some(code) = &l.crs else { return Ok(()) };
    let crs = ifc.create("IfcProjectedCRS")?;
    ifc.set_attribute(crs, "Name", &AttrValue::String(code.clone()))?;
    let unit = ifc.create("IfcSIUnit")?;
    ifc.set_attribute(unit, "UnitType", &AttrValue::Enum("LENGTHUNIT".into()))?;
    ifc.set_attribute(unit, "Name", &AttrValue::Enum("METRE".into()))?;
    ifc.set_attribute(crs, "MapUnit", &r(unit))?;
    let (o, x) = l.map_conversion();
    let mc = ifc.create("IfcMapConversion")?;
    ifc.set_attribute(mc, "SourceCRS", &r(context))?;
    ifc.set_attribute(mc, "TargetCRS", &r(crs))?;
    // Map units are metres; the model's lengths are in its own unit.
    ifc.set_attribute(mc, "Eastings", &AttrValue::Real(o[0]))?;
    ifc.set_attribute(mc, "Northings", &AttrValue::Real(o[1]))?;
    ifc.set_attribute(mc, "OrthogonalHeight", &AttrValue::Real(o[2]))?;
    ifc.set_attribute(mc, "XAxisAbscissa", &AttrValue::Real(x[0]))?;
    ifc.set_attribute(mc, "XAxisOrdinate", &AttrValue::Real(x[1]))?;
    ifc.set_attribute(mc, "Scale", &AttrValue::Real(scale))?;
    Ok(())
}

/// The site's latitude, longitude and elevation (metres), when the file states them.
pub(crate) fn read_site(ifc: &IfcModel, scale: f64) -> Result<Option<(f64, f64, f64)>> {
    let Some(site) = ifc.entities_of_type("IfcSite")?.first().map(|e| e.id) else { return Ok(None) };
    let ints = |v: AttrValue| -> Option<Vec<i64>> {
        match v {
            AttrValue::List(l) => l.into_iter().map(|a| if let AttrValue::Int(i) = a { Some(i) } else { None }).collect(),
            _ => None,
        }
    };
    let (Some(lat), Some(lon)) = (ints(ifc.attribute(site, "RefLatitude")?), ints(ifc.attribute(site, "RefLongitude")?)) else { return Ok(None) };
    let elev = match ifc.attribute(site, "RefElevation")? {
        AttrValue::Real(v) => v * scale,
        _ => 0.0,
    };
    Ok(Some((location::from_compound(&lat), location::from_compound(&lon), elev)))
}
