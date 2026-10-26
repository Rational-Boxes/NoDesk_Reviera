// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The energy model (ANA-EM-003, ANA-ENV-003): a two-storey box of 8 rooms gives 8 analytical
//! spaces, and every interior wall surface has a corresponding surface on the space beyond it
//! (same wall, pointing back); floors on the ground level are slabs on grade and ceilings under
//! the upper level interior, the upper ceilings roofs; a window comes off the exterior wall area;
//! a wall type's U-value is used once set and goes to IFC.

use reviera_core::energy_model::{EnergyModel, SurfaceClass};
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    r.execute(doc, name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

#[test]
fn energy_model() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("energy.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let l2 = id(&ok(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })));
    let mut front = None;
    for level in [l1, l2] {
        // An 8 m square in four 4 m rooms: the outer walls and a cross.
        for (a, b) in [([0, 0], [8, 0]), ([8, 0], [8, 8]), ([8, 8], [0, 8]), ([0, 8], [0, 0]), ([4, 0], [4, 8]), ([0, 4], [8, 4])] {
            let w = id(&ok(&r, &mut doc, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } })));
            if level == l1 && a == [0, 0] && b == [8, 0] {
                front = Some(w);
            }
        }
        ok(&r, &mut doc, "space.place_auto", json!({ "level_id": level.0 }));
    }
    ok(&r, &mut doc, "window.place", json!({ "host_wall_id": front.unwrap().0, "point_along": 2.0 }));

    let m: EnergyModel = serde_json::from_value(ok(&r, &mut doc, "energy_model.get", json!({}))).unwrap();
    assert_eq!(m.spaces.len(), 8);
    for s in &m.spaces {
        for f in s.surfaces.iter().filter(|f| f.class == SurfaceClass::InteriorWall) {
            let other = m.spaces.iter().find(|o| Some(o.space) == f.adjacent).expect("the adjacent space");
            assert!(
                other.surfaces.iter().any(|g| g.class == SurfaceClass::InteriorWall && g.adjacent == Some(s.space) && g.element == f.element && (g.normal[0] + f.normal[0]).abs() < 1e-9 && (g.normal[1] + f.normal[1]).abs() < 1e-9),
                "a corresponding boundary for space {} on {:?}",
                s.number,
                f.element
            );
        }
        // Two exterior and two interior walls in each corner room.
        let n = |c: SurfaceClass| s.surfaces.iter().filter(|f| f.class == c).count();
        assert_eq!((n(SurfaceClass::ExteriorWall), n(SurfaceClass::InteriorWall)), (2, 2), "space {}", s.number);
        let lower = s.level == Some(l1);
        assert_eq!(n(SurfaceClass::SlabOnGrade), usize::from(lower));
        assert_eq!(n(SurfaceClass::InteriorFloor), usize::from(!lower));
        assert_eq!(n(SurfaceClass::InteriorCeiling), usize::from(lower));
        assert_eq!(n(SurfaceClass::Roof), usize::from(!lower));
    }
    // The window: its area comes off the wall.
    let win = m.spaces.iter().flat_map(|s| &s.surfaces).find(|f| f.class == SurfaceClass::Window).expect("a window surface");
    let host = m.spaces.iter().find(|s| s.surfaces.iter().any(|f| f.class == SurfaceClass::Window)).unwrap();
    let wall = host.surfaces.iter().find(|f| f.class == SurfaceClass::ExteriorWall && f.element == front).unwrap();
    let full = host.surfaces.iter().find(|f| f.class == SurfaceClass::ExteriorWall && f.element != front).unwrap();
    assert!((wall.area + win.area - full.area).abs() < 1e-6, "{} + {} against {}", wall.area, win.area, full.area);
    assert_eq!(win.shgc, Some(0.6));

    // The wall type's U-value, once set, is used and written to IFC.
    let wall_type = doc.store().iter().find(|e| reviera_core::compound::of_wall_type(e).is_some()).unwrap().id();
    let s0 = ok(&r, &mut doc, "energy_model.summary", json!({}));
    ok(&r, &mut doc, "energy_model.set_thermal", json!({ "type_id": wall_type.0, "u_value": 0.15 }));
    let m: EnergyModel = serde_json::from_value(ok(&r, &mut doc, "energy_model.get", json!({}))).unwrap();
    assert!(m.spaces.iter().flat_map(|s| &s.surfaces).filter(|f| f.class == SurfaceClass::ExteriorWall).all(|f| f.u == 0.15 && f.u_from_type));
    let s1 = ok(&r, &mut doc, "energy_model.summary", json!({}));
    assert!(s1["ua_exterior"].as_f64().unwrap() < s0["ua_exterior"].as_f64().unwrap());
    assert!(s1["window_to_wall"].as_f64().unwrap() > 0.0);
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let found: Vec<&str> = rows.iter().filter(|r| r.attributes.contains("ThermalTransmittance") || r.attributes.contains("Pset_WallCommon")).map(|r| r.attributes.as_str()).collect();
    assert!(rows.iter().any(|r| r.attributes.contains("'ThermalTransmittance'") && r.attributes.contains("IFCTHERMALTRANSMITTANCEMEASURE(0.1")), "{found:?}");
}
