// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Default display colours by IFC class. Riviera's views draw with them, and saving writes them
//! as surface styles onto geometry that has none, so other viewers (xeokit, BIMvision, …) show
//! the same colours instead of white on white.

/// `rrggbb` or `rrggbbaa` as RGBA 0–1.
pub fn parse_hex(hex: &str) -> Option<[f32; 4]> {
    let b = |i: usize| {
        u8::from_str_radix(hex.get(i..i + 2)?, 16)
            .ok()
            .map(|v| v as f32 / 255.0)
    };
    match hex.len() {
        6 => Some([b(0)?, b(2)?, b(4)?, 1.0]),
        8 => Some([b(0)?, b(2)?, b(4)?, b(6)?]),
        _ => None,
    }
}

/// Default display colours by IFC class (RGBA 0–1, alpha 1 opaque), until object styles and
/// materials drive them.
pub fn class_color(class: &str) -> [f32; 4] {
    // A part's material after the class (`IfcDoor:glass`).
    let (class, material) = class.split_once(':').unwrap_or((class, ""));
    // An explicit colour (`IfcFurniture:#rrggbbaa`): a custom object's material slot.
    if let Some(hex) = material.strip_prefix('#')
        && let Some(c) = parse_hex(hex)
    {
        return c;
    }
    match material.split('/').next().unwrap_or("") {
        "glass" => return [0.62, 0.80, 0.92, 0.35],
        // Masses show as translucent volumes (ARC-MASS-001).
        "mass" => return [0.78, 0.68, 0.50, 0.45],
        "metal" => return [0.66, 0.68, 0.71, 1.0],
        // A window's frame and sash, solid round its translucent glass.
        "frame" => return [0.93, 0.93, 0.91, 1.0],
        // Toposolids (ARC-TOPO-001).
        "earth" => return [0.56, 0.62, 0.40, 1.0],
        // A wall layer's cell (`IfcWall:layer/Brick`): a colour after its material's name.
        "layer" => {
            if let Some(c) = layer_color(material.split_once('/').map_or("", |x| x.1)) {
                return c;
            }
        }
        _ => {}
    }
    let c = class.strip_suffix("StandardCase").unwrap_or(class);
    match c {
        "IfcWall" => [0.80, 0.74, 0.64, 1.0],
        "IfcSlab" => [0.58, 0.58, 0.62, 1.0],
        "IfcCovering" => [0.93, 0.93, 0.90, 1.0],
        "IfcRoof" => [0.62, 0.30, 0.22, 1.0],
        "IfcWindow" => [0.55, 0.75, 0.90, 0.55],
        "IfcDoor" => [0.60, 0.42, 0.25, 1.0],
        "IfcColumn" | "IfcBeam" | "IfcMember" => [0.55, 0.57, 0.62, 1.0],
        "IfcStair" | "IfcStairFlight" | "IfcRailing" => [0.65, 0.60, 0.52, 1.0],
        "IfcSpace" => [0.40, 0.80, 0.50, 0.15],
        "IfcFurniture" | "IfcFurnishingElement" => [0.75, 0.55, 0.35, 1.0],
        "IfcPlate" | "IfcCurtainWall" => [0.50, 0.70, 0.85, 0.5],
        "IfcRamp" | "IfcRampFlight" => [0.65, 0.60, 0.52, 1.0],
        "IfcFooting" | "IfcPile" => [0.58, 0.58, 0.55, 1.0],
        "IfcReinforcingBar" | "IfcReinforcingMesh" => [0.45, 0.26, 0.20, 1.0],
        "IfcSite" | "IfcGeographicElement" => [0.56, 0.62, 0.40, 1.0],
        "IfcBuildingElementProxy" => [0.72, 0.72, 0.75, 1.0],
        // Plumbing: white ware, and pipes blue.
        "IfcSanitaryTerminal" | "IfcWasteTerminal" => [0.96, 0.96, 0.97, 1.0],
        "IfcPipeSegment" | "IfcPipeFitting" | "IfcValve" | "IfcPump" => [0.22, 0.47, 0.75, 1.0],
        // Air: galvanised ducts.
        "IfcDuctSegment" | "IfcDuctFitting" | "IfcAirTerminal" | "IfcDamper" | "IfcAirTerminalBox" => [0.70, 0.73, 0.77, 1.0],
        "IfcCableCarrierSegment" | "IfcCableCarrierFitting" | "IfcCableSegment" | "IfcCableFitting" => [0.85, 0.62, 0.18, 1.0],
        "IfcLightFixture" | "IfcLamp" => [0.98, 0.92, 0.62, 1.0],
        "IfcElectricDistributionBoard" | "IfcSwitchingDevice" | "IfcOutlet" => [0.40, 0.42, 0.46, 1.0],
        "IfcUnitaryEquipment" | "IfcBoiler" | "IfcChiller" | "IfcFan" | "IfcCoil" | "IfcSpaceHeater" | "IfcTank" => [0.52, 0.58, 0.55, 1.0],
        "IfcFireSuppressionTerminal" => [0.80, 0.18, 0.15, 1.0],
        _ => [0.78, 0.78, 0.78, 1.0],
    }
}

/// A wall layer's colour from words in its material's name.
fn layer_color(name: &str) -> Option<[f32; 4]> {
    let n = name.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    Some(if has(&["brick", "clay", "terracotta"]) {
        [0.66, 0.36, 0.28, 1.0]
    } else if has(&["stone", "granite", "limestone", "sandstone"]) {
        [0.74, 0.71, 0.64, 1.0]
    } else if has(&["concrete", "cement", "block", "cmu"]) {
        [0.68, 0.68, 0.68, 1.0]
    } else if has(&["timber", "wood", "osb", "ply", "stud"]) {
        [0.74, 0.57, 0.36, 1.0]
    } else if has(&["insulation", "mineral", "wool", "eps", "xps"]) {
        [0.95, 0.86, 0.45, 1.0]
    } else if has(&["render", "plaster", "stucco", "gypsum", "board"]) {
        [0.93, 0.91, 0.86, 1.0]
    } else if has(&["metal", "steel", "alumin"]) {
        [0.66, 0.68, 0.71, 1.0]
    } else {
        return None;
    })
}

