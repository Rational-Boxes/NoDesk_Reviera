// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The category registry (spec Ch 2 §2.2.2, DM-CATG-001): a fixed classification derived from the
//! IFC class, `PredefinedType` and `ObjectType`.

use serde::Serialize;
use serde_json::Value;

use crate::element::Element;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Category {
    pub key: &'static str,
    pub name: &'static str,
    /// IFC occurrence class (`None` for categories stored only in RXD).
    pub ifc_class: Option<&'static str>,
    pub ifc_type_class: Option<&'static str>,
    /// The `Pset_<Class>Common` set holding common properties such as FireRating.
    pub common_pset: Option<&'static str>,
    pub is_model: bool,
}

const fn cat(
    key: &'static str,
    name: &'static str,
    ifc_class: Option<&'static str>,
    ifc_type_class: Option<&'static str>,
    common_pset: Option<&'static str>,
    is_model: bool,
) -> Category {
    Category {
        key,
        name,
        ifc_class,
        ifc_type_class,
        common_pset,
        is_model,
    }
}

/// The baseline registry; domain chapters add rows.
pub const CATEGORIES: &[Category] = &[
    cat(
        "cat.walls",
        "Walls",
        Some("IfcWall"),
        Some("IfcWallType"),
        Some("Pset_WallCommon"),
        true,
    ),
    cat(
        "cat.curtain_walls",
        "Curtain Walls",
        Some("IfcCurtainWall"),
        Some("IfcCurtainWallType"),
        Some("Pset_CurtainWallCommon"),
        true,
    ),
    cat(
        "cat.curtain_panels",
        "Curtain Panels",
        Some("IfcPlate"),
        Some("IfcPlateType"),
        Some("Pset_PlateCommon"),
        true,
    ),
    cat(
        "cat.curtain_mullions",
        "Curtain Wall Mullions",
        Some("IfcMember"),
        Some("IfcMemberType"),
        Some("Pset_MemberCommon"),
        true,
    ),
    cat(
        "cat.floors",
        "Floors",
        Some("IfcSlab"),
        Some("IfcSlabType"),
        Some("Pset_SlabCommon"),
        true,
    ),
    cat(
        "cat.roofs",
        "Roofs",
        Some("IfcRoof"),
        Some("IfcRoofType"),
        Some("Pset_RoofCommon"),
        true,
    ),
    cat(
        "cat.ceilings",
        "Ceilings",
        Some("IfcCovering"),
        Some("IfcCoveringType"),
        Some("Pset_CoveringCommon"),
        true,
    ),
    cat(
        "cat.doors",
        "Doors",
        Some("IfcDoor"),
        Some("IfcDoorType"),
        Some("Pset_DoorCommon"),
        true,
    ),
    cat(
        "cat.windows",
        "Windows",
        Some("IfcWindow"),
        Some("IfcWindowType"),
        Some("Pset_WindowCommon"),
        true,
    ),
    cat(
        "cat.stairs",
        "Stairs",
        Some("IfcStair"),
        Some("IfcStairType"),
        Some("Pset_StairCommon"),
        true,
    ),
    cat(
        "cat.railings",
        "Railings",
        Some("IfcRailing"),
        Some("IfcRailingType"),
        Some("Pset_RailingCommon"),
        true,
    ),
    cat(
        "cat.ramps",
        "Ramps",
        Some("IfcRamp"),
        Some("IfcRampType"),
        Some("Pset_RampCommon"),
        true,
    ),
    cat(
        "cat.columns",
        "Columns",
        Some("IfcColumn"),
        Some("IfcColumnType"),
        Some("Pset_ColumnCommon"),
        true,
    ),
    cat(
        "cat.structural_columns",
        "Structural Columns",
        Some("IfcColumn"),
        Some("IfcColumnType"),
        Some("Pset_ColumnCommon"),
        true,
    ),
    cat(
        "cat.structural_framing",
        "Structural Framing",
        Some("IfcBeam"),
        Some("IfcBeamType"),
        Some("Pset_BeamCommon"),
        true,
    ),
    cat(
        "cat.structural_foundations",
        "Structural Foundations",
        Some("IfcFooting"),
        Some("IfcFootingType"),
        Some("Pset_FootingCommon"),
        true,
    ),
    // A steel connection and its plates, bolts and welds (STR-STL-002).
    cat(
        "cat.structural_connections",
        "Structural Connections",
        None,
        None,
        None,
        true,
    ),
    // Rebar sets (STR-RBR-003).
    cat(
        "cat.structural_rebar",
        "Structural Rebar",
        Some("IfcReinforcingBar"),
        Some("IfcReinforcingBarType"),
        None,
        true,
    ),
    // Rebar couplers (STR-RBR-015).
    cat("cat.structural_rebar_couplers", "Structural Rebar Couplers", None, None, None, true),
    // Area, path and fabric reinforcement (STR-RBR-010 … 012).
    cat("cat.structural_area_reinforcement", "Structural Area Reinforcement", None, None, None, true),
    cat("cat.structural_path_reinforcement", "Structural Path Reinforcement", None, None, None, true),
    cat("cat.structural_fabric_areas", "Structural Fabric Areas", None, None, None, true),
    cat(
        "cat.structural_fabric_reinforcement",
        "Structural Fabric Reinforcement",
        Some("IfcReinforcingMesh"),
        Some("IfcReinforcingMeshType"),
        None,
        true,
    ),
    cat(
        "cat.furniture",
        "Furniture",
        Some("IfcFurniture"),
        Some("IfcFurnitureType"),
        Some("Pset_FurnitureTypeCommon"),
        true,
    ),
    cat(
        "cat.casework",
        "Casework",
        Some("IfcFurniture"),
        Some("IfcFurnitureType"),
        Some("Pset_FurnitureTypeCommon"),
        true,
    ),
    cat(
        "cat.plumbing_fixtures",
        "Plumbing Fixtures",
        Some("IfcSanitaryTerminal"),
        Some("IfcSanitaryTerminalType"),
        Some("Pset_SanitaryTerminalTypeCommon"),
        true,
    ),
    cat(
        "cat.generic_models",
        "Generic Models",
        Some("IfcBuildingElementProxy"),
        Some("IfcBuildingElementProxyType"),
        Some("Pset_BuildingElementProxyCommon"),
        true,
    ),
    cat(
        "cat.specialty_equipment",
        "Specialty Equipment",
        Some("IfcBuildingElementProxy"),
        Some("IfcBuildingElementProxyType"),
        Some("Pset_BuildingElementProxyCommon"),
        true,
    ),
    cat(
        "cat.mass",
        "Mass",
        Some("IfcBuildingElementProxy"),
        Some("IfcBuildingElementProxyType"),
        None,
        true,
    ),
    cat(
        "cat.rooms",
        "Rooms",
        Some("IfcSpace"),
        Some("IfcSpaceType"),
        Some("Pset_SpaceCommon"),
        true,
    ),
    cat(
        "cat.areas",
        "Areas",
        Some("IfcSpace"),
        None,
        Some("Pset_SpaceCommon"),
        true,
    ),
    cat(
        "cat.levels",
        "Levels",
        Some("IfcBuildingStorey"),
        None,
        Some("Pset_BuildingStoreyCommon"),
        false,
    ),
    cat("cat.grids", "Grids", Some("IfcGrid"), None, None, false),
    cat(
        "cat.reference_planes",
        "Reference Planes",
        Some("IfcAnnotation"),
        None,
        None,
        false,
    ),
    cat(
        "cat.scope_boxes",
        "Scope Boxes",
        Some("IfcAnnotation"),
        None,
        None,
        false,
    ),
    cat(
        "cat.lines",
        "Lines",
        Some("IfcAnnotation"),
        None,
        None,
        true,
    ),
    cat(
        "cat.openings",
        "Openings",
        Some("IfcOpeningElement"),
        None,
        None,
        true,
    ),
    cat(
        "cat.toposolid",
        "Toposolid",
        Some("IfcGeographicElement"),
        Some("IfcGeographicElementType"),
        None,
        true,
    ),
    cat(
        "cat.mechanical_equipment",
        "Mechanical Equipment",
        Some("IfcUnitaryEquipment"),
        Some("IfcUnitaryEquipmentType"),
        Some("Pset_UnitaryEquipmentTypeCommon"),
        true,
    ),
    cat(
        "cat.air_terminals",
        "Air Terminals",
        Some("IfcAirTerminal"),
        Some("IfcAirTerminalType"),
        Some("Pset_AirTerminalTypeCommon"),
        true,
    ),
    cat(
        "cat.electrical_equipment",
        "Electrical Equipment",
        Some("IfcElectricDistributionBoard"),
        Some("IfcElectricDistributionBoardType"),
        Some("Pset_ElectricDistributionBoardTypeCommon"),
        true,
    ),
    cat(
        "cat.electrical_fixtures",
        "Electrical Fixtures",
        Some("IfcOutlet"),
        Some("IfcOutletType"),
        Some("Pset_OutletTypeCommon"),
        true,
    ),
    cat(
        "cat.lighting_fixtures",
        "Lighting Fixtures",
        Some("IfcLightFixture"),
        Some("IfcLightFixtureType"),
        Some("Pset_LightFixtureTypeCommon"),
        true,
    ),
    cat(
        "cat.sprinklers",
        "Sprinklers",
        Some("IfcFireSuppressionTerminal"),
        Some("IfcFireSuppressionTerminalType"),
        Some("Pset_FireSuppressionTerminalTypeCommon"),
        true,
    ),
    cat(
        "cat.mep_systems",
        "MEP Systems",
        Some("IfcDistributionSystem"),
        None,
        Some("Pset_DistributionSystemCommon"),
        false,
    ),
    cat(
        "cat.ducts",
        "Ducts",
        Some("IfcDuctSegment"),
        Some("IfcDuctSegmentType"),
        Some("Pset_DuctSegmentTypeCommon"),
        true,
    ),
    cat(
        "cat.pipes",
        "Pipes",
        Some("IfcPipeSegment"),
        Some("IfcPipeSegmentType"),
        Some("Pset_PipeSegmentTypeCommon"),
        true,
    ),
    cat(
        "cat.parts",
        "Parts",
        Some("IfcBuildingElementPart"),
        Some("IfcBuildingElementPartType"),
        None,
        true,
    ),
    cat(
        "cat.assemblies",
        "Assemblies",
        Some("IfcElementAssembly"),
        Some("IfcElementAssemblyType"),
        None,
        true,
    ),
];

pub fn category(key: &str) -> Option<&'static Category> {
    CATEGORIES.iter().find(|c| c.key == key)
}

fn by_key(key: &str) -> Option<&'static Category> {
    category(key)
}

/// Whether an element is a type (family type) rather than an occurrence.
pub fn is_type(e: &Element) -> bool {
    // IFC2X3 also has door and window "styles"; grid types are plain `IfcTypeObject`s.
    e.class.ends_with("Type") || e.class.ends_with("Style") || e.class == "IfcTypeObject"
}

fn text<'a>(e: &'a Element, key: &str) -> Option<&'a str> {
    e.param(key).and_then(Value::as_str)
}

/// The category of an element (occurrence or type), inferred from its IFC class and attributes.
pub fn category_of(e: &Element) -> Option<&'static Category> {
    if crate::grids::is_grid(e) || crate::grids::is_grid_type(e) {
        return category("cat.grids");
    }
    if crate::rebar_edit::coupler_recipe(e).is_some() {
        return category("cat.structural_rebar_couplers");
    }
    if crate::connections::recipe(e).is_some()
        || crate::steel_fab::fab_recipe(e).is_some()
        || crate::steel_detail::is_detail(e)
        || crate::steel_cuts::cut_recipe(e).is_some()
        || crate::steel_cuts::is_sub_connection(e)
        || crate::steel_numbering::assembly_parts_kind(e)
    {
        return category("cat.structural_connections");
    }
    match crate::rebar_systems::system_kind(e) {
        Some("area") => return category("cat.structural_area_reinforcement"),
        Some("path") => return category("cat.structural_path_reinforcement"),
        Some("fabric_area") => return category("cat.structural_fabric_areas"),
        _ => {}
    }
    let class = e.class.strip_suffix("StandardCase").unwrap_or(&e.class);
    let class = match class {
        "IfcDoorStyle" => "IfcDoorType",
        "IfcWindowStyle" => "IfcWindowType",
        "IfcSlabElementedCase" => "IfcSlab",
        "IfcWallElementedCase" => "IfcWall",
        "IfcStairFlight" => "IfcStair",
        "IfcRampFlight" => "IfcRamp",
        "IfcPile" => "IfcFooting",
        "IfcFurnishingElement" => "IfcFurniture",
        other => other,
    };
    let base = class.strip_suffix("Type").unwrap_or(class);
    let predefined = text(e, "@PredefinedType").unwrap_or("");
    let object_type = text(e, "@ObjectType").unwrap_or("");
    let key = match base {
        "IfcSlab" if predefined == "ROOF" => "cat.roofs",
        "IfcCovering" if predefined == "CEILING" => "cat.ceilings",
        "IfcCovering" => return None,
        "IfcMember" if predefined == "MULLION" => "cat.curtain_mullions",
        "IfcMember" => "cat.structural_framing",
        "IfcPlate" => "cat.curtain_panels",
        "IfcMechanicalFastener" | "IfcFastener" => "cat.structural_connections",
        "IfcColumn" => {
            let load_bearing = e
                .param("Pset_ColumnCommon.LoadBearing")
                .and_then(|v| v.get("value"))
                .and_then(Value::as_bool);
            if load_bearing == Some(true) {
                "cat.structural_columns"
            } else {
                "cat.columns"
            }
        }
        "IfcBuildingElementProxy" => match object_type {
            "Mass" => "cat.mass",
            "SpecialtyEquipment" => "cat.specialty_equipment",
            _ => "cat.generic_models",
        },
        "IfcFurniture" if object_type == "Casework" => "cat.casework",
        "IfcSpace" if object_type.starts_with("Area:") => "cat.areas",
        "IfcAnnotation" if object_type == crate::ref_planes::OBJECT_TYPE => "cat.reference_planes",
        "IfcAnnotation" if object_type == crate::model_lines::OBJECT_TYPE => "cat.lines",
        "IfcAnnotation" if object_type == crate::scope_boxes::OBJECT_TYPE => "cat.scope_boxes",
        "IfcAnnotation" => return None,
        _ => return CATEGORIES.iter().find(|c| c.ifc_class == Some(base)),
    };
    by_key(key)
}
