// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Architectural columns (ARC-COL-001): placement between levels, sections, attaching tops to a
//! sloped roof, the modify tools, and IFC.

use reviera_core::columns::{self, Justification};
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::{IfcModel, Schema};
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(
    r: &CommandRegistry,
    doc: &mut Document,
    name: &str,
    args: Value,
) -> reviera_core::Result<Value> {
    r.execute(doc, name, args, &Caller::user())
        .map(|o| o.result)
}

fn id(v: Value) -> ElementId {
    ElementId(v.as_u64().expect("an element id"))
}

fn geometry(doc: &Document, c: ElementId) -> columns::ColumnGeometry {
    columns::regenerate(doc.store(), doc.store().get(c).unwrap()).unwrap()
}

#[test]
fn columns_between_levels_attached_to_a_roof_and_in_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 3.0 })).unwrap());
    let c = id(run(
        &r,
        &mut doc,
        "column.place_architectural",
        json!({ "point": [2.0, 2.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    )
    .unwrap());
    let g = geometry(&doc, c);
    assert_eq!((g.z0, g.top()), (0.0, 3.0));
    assert!((g.ring[0][0] - 1.8).abs() < 1e-9);
    // It follows its top level.
    run(
        &r,
        &mut doc,
        "level.set_elevation",
        json!({ "level_id": l2.0, "elevation": 3.5 }),
    )
    .unwrap();
    assert_eq!(geometry(&doc, c).top(), 3.5);

    // A round type: duplicate the default and change its section.
    let t = doc
        .store()
        .iter()
        .find(|e| columns::column_type(e).is_some())
        .unwrap()
        .id();
    let round = id(run(
        &r,
        &mut doc,
        "type.duplicate",
        json!({ "type_id": t.0, "new_name": "Round Column 500" }),
    )
    .unwrap());
    for (param, value) in [("section", json!("round")), ("diameter", json!("500 mm"))] {
        run(
            &r,
            &mut doc,
            "type.set_param",
            json!({ "type_id": round.0, "param": param, "value": value }),
        )
        .unwrap();
    }
    run(
        &r,
        &mut doc,
        "element.set_type",
        json!({ "element_ids": [c.0], "type_id": round.0 }),
    )
    .unwrap();
    let g = geometry(&doc, c);
    assert_eq!(g.ring.len(), 32);
    assert!((g.ring[0][0] - 2.25).abs() < 1e-9);

    // Under a hip roof at 6 m, unconnected 10 m high, attached: the maximum reaches the highest
    // point of the underside over it, the minimum the lowest.
    let square = |x0: f64, y0: f64, x1: f64, y1: f64| {
        json!([
            { "line": { "start": [x0, y0], "end": [x1, y0] } },
            { "line": { "start": [x1, y0], "end": [x1, y1] } },
            { "line": { "start": [x1, y1], "end": [x0, y1] } },
            { "line": { "start": [x0, y1], "end": [x0, y0] } },
        ])
    };
    let roof = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": square(0.0, 0.0, 10.0, 10.0), "level_id": l1.0, "base_offset": 6.0 }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "column.set",
        json!({ "column_ids": [c.0], "top_constraint": { "unconnected": 10.0 } }),
    )
    .unwrap();
    // Attach Top in the window attaches to what is directly above.
    assert_eq!(columns::target_above(doc.store(), c), Some(roof));
    run(
        &r,
        &mut doc,
        "column.attach",
        json!({ "column_ids": [c.0], "target_id": roof.0 }),
    )
    .unwrap();
    let rg = reviera_core::roofs::regenerate(doc.store(), doc.store().get(roof).unwrap()).unwrap();
    let g = geometry(&doc, c);
    let over: Vec<f64> = g.ring.iter().map(|p| rg.underside(*p).unwrap()).collect();
    let hi = over.iter().copied().fold(f64::MIN, f64::max);
    let lo = over.iter().copied().fold(f64::MAX, f64::min);
    assert!(hi > lo + 0.05, "{lo} {hi}");
    // Cut by the underside: each corner of the top is on it.
    for (t, u) in g.tops.iter().zip(&over) {
        assert!((t - u).abs() < 1e-6, "{t} {u}");
    }
    assert!(g.top() <= hi + 1e-6);
    run(
        &r,
        &mut doc,
        "column.attach",
        json!({ "column_ids": [c.0], "target_id": roof.0, "justification": "minimum" }),
    )
    .unwrap();
    let g = geometry(&doc, c);
    assert!(
        g.tops.iter().all(|t| (t - lo).abs() < 0.05),
        "{:?} {lo}",
        g.tops
    );
    assert_eq!(
        columns::column_recipe(doc.store().get(c).unwrap())
            .unwrap()
            .attach_top
            .unwrap()
            .justification,
        Justification::Minimum
    );

    // Moved, rotated and copied.
    run(
        &r,
        &mut doc,
        "column.detach",
        json!({ "column_ids": [c.0] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": [c.0], "vector": [1.0, 0.0, 0.0] }),
    )
    .unwrap();
    run(
        &r,
        &mut doc,
        "element.rotate",
        json!({ "element_ids": [c.0], "center": [0.0, 0.0], "angle": "90°" }),
    )
    .unwrap();
    let rc = columns::column_recipe(doc.store().get(c).unwrap()).unwrap();
    assert!((rc.point[0] + 2.0).abs() < 1e-9 && (rc.point[1] - 3.0).abs() < 1e-9);
    assert!((rc.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    let copies = run(
        &r,
        &mut doc,
        "element.copy",
        json!({ "element_ids": [c.0], "vectors": [[0.0, 2.0, 0.0], [0.0, 4.0, 0.0]] }),
    )
    .unwrap();
    assert_eq!(copies.as_array().unwrap().len(), 2);
    assert_eq!(columns::all(doc.store()).len(), 3);

    // IFC: three IfcColumns of two IfcColumnTypes, with bodies; reopened, still columns.
    project.save(&doc).unwrap();
    let rows = IfcModel::open(&path).unwrap().step_rows().unwrap();
    let count = |class: &str| rows.iter().filter(|r| r.class == class).count();
    assert_eq!(count("IFCCOLUMN"), 3);
    assert_eq!(count("IFCCOLUMNTYPE"), 2);
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    assert_eq!(columns::all(doc2.store()).len(), 3);
    assert_eq!(
        reviera_core::categories::category_of(doc2.store().get(c).unwrap()).map(|c| c.name),
        Some("Columns")
    );

    // Deleting Level 2, which they no longer use (their tops are unconnected), leaves them.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [l2.0], "confirm_dependents": true }),
    )
    .unwrap();
    assert_eq!(columns::all(doc.store()).len(), 3);
}

/// The volume of a closed mesh.
fn volume(m: &reviera_core::bodies::Mesh) -> f64 {
    m.1.chunks(3)
        .map(|t| {
            let [a, b, c] = [m.0[t[0] as usize], m.0[t[1] as usize], m.0[t[2] as usize]];
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

#[test]
fn structural_columns_sections_grids_marks_and_ifc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    let l2 = id(run(&r, &mut doc, "level.create", json!({ "elevation": 4.0 })).unwrap());

    // The default structural type: 400 x 400 concrete, load-bearing.
    let c = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "point": [0.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    )
    .unwrap());
    let e = doc.store().get(c).unwrap();
    assert_eq!(
        e.param("Pset_ColumnCommon.LoadBearing").unwrap()["value"],
        true
    );
    let g = geometry(&doc, c);
    assert!(g.ty.structural && g.ty.material_kind == Some(columns::MaterialKind::Concrete));
    assert!((volume(&g.mesh()) - 0.4 * 0.4 * 4.0).abs() < 1e-9);
    let concrete = id(json!(
        doc.store()
            .get(c)
            .unwrap()
            .param("@type")
            .and_then(|g| g.as_str())
            .and_then(|g| {
                reviera_core::GlobalId::parse(g).and_then(|g| doc.store().find_by_guid(&g))
            })
            .unwrap()
            .0
    ));

    // A steel I section (generic sizes): its body is the section's area times its height.
    let steel = id(run(
        &r,
        &mut doc,
        "type.duplicate",
        json!({ "type_id": concrete.0, "new_name": "Steel I 300 x 300" }),
    )
    .unwrap());
    for (param, value) in [
        ("section", json!("i_shape")),
        ("width", json!("300 mm")),
        ("depth", json!("300 mm")),
        ("web_thickness", json!("11 mm")),
        ("flange_thickness", json!("19 mm")),
        ("material", json!("Steel")),
        ("material_kind", json!("steel")),
    ] {
        run(
            &r,
            &mut doc,
            "type.set_param",
            json!({ "type_id": steel.0, "param": param, "value": value }),
        )
        .unwrap_or_else(|e| panic!("{param}: {e:?}"));
    }
    // A web thicker than the flange is wide is refused.
    assert!(
        run(
            &r,
            &mut doc,
            "type.set_param",
            json!({ "type_id": steel.0, "param": "web_thickness", "value": "400 mm" })
        )
        .is_err()
    );
    let i = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "type_id": steel.0, "point": [6.0, 0.0], "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    )
    .unwrap());
    let g = geometry(&doc, i);
    let area = 2.0 * 0.3 * 0.019 + (0.3 - 2.0 * 0.019) * 0.011;
    assert!((g.ty.area() - area).abs() < 1e-12);
    assert!((volume(&g.mesh()) - area * 4.0).abs() < 1e-9);
    assert_eq!(g.ring.len(), 12);

    // A round hollow section: a tube, its hole through.
    let tube = id(run(
        &r,
        &mut doc,
        "type.duplicate",
        json!({ "type_id": steel.0, "new_name": "Round Hollow 219 x 8" }),
    )
    .unwrap());
    for (param, value) in [
        ("section", json!("circular_hollow")),
        ("diameter", json!("219.1 mm")),
        ("wall_thickness", json!("8 mm")),
    ] {
        run(
            &r,
            &mut doc,
            "type.set_param",
            json!({ "type_id": tube.0, "param": param, "value": value }),
        )
        .unwrap();
    }
    let h = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "type_id": tube.0, "point": [12.0, 0.0], "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    let g = geometry(&doc, h);
    assert_eq!(g.holes.len(), 1);
    assert!((volume(&g.mesh()) - g.ty.area() * 3.0).abs() < 1e-9);

    // An architectural type is refused for a structural column.
    let arch = id(run(
        &r,
        &mut doc,
        "column.place_architectural",
        json!({ "point": [20.0, 20.0], "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap());
    let arch_type = reviera_core::GlobalId::parse(
        doc.store()
            .get(arch)
            .unwrap()
            .param("@type")
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .and_then(|g| doc.store().find_by_guid(&g))
    .unwrap();
    let e = run(
        &r,
        &mut doc,
        "column.place",
        json!({ "type_id": arch_type.0, "point": [21.0, 21.0], "base_level": l1.0, "top_constraint": { "unconnected": 3.0 } }),
    )
    .unwrap_err();
    assert_eq!(e.code, "column.not_structural_type");

    // At grids: 3 lettered by 2 numbered lines make 6 columns, in one undo step.
    let mut grids = Vec::new();
    for (k, x) in [0.0, 6.0, 12.0].into_iter().enumerate() {
        grids.push(id(run(
            &r,
            &mut doc,
            "grid.create",
            json!({ "curve": { "line": { "start": [x, -2.0], "end": [x, 10.0] } }, "name": (["A", "B", "C"][k]) }),
        )
        .unwrap()));
    }
    for (k, y) in [2.0, 8.0].into_iter().enumerate() {
        grids.push(id(run(
            &r,
            &mut doc,
            "grid.create",
            json!({ "curve": { "line": { "start": [-2.0, y], "end": [14.0, y] } }, "name": (["1", "2"][k]) }),
        )
        .unwrap()));
    }
    let before = columns::all(doc.store()).len();
    let placed = run(
        &r,
        &mut doc,
        "column.place_at_grids",
        json!({ "type_id": steel.0, "grid_ids": grids.iter().map(|g| g.0).collect::<Vec<_>>(), "base_level": l1.0, "top_constraint": { "level": l2.0 } }),
    )
    .unwrap();
    let placed: Vec<u64> = placed
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(placed.len(), 6);
    assert_eq!(columns::all(doc.store()).len(), before + 6);
    run(&r, &mut doc, "edit.undo", json!({})).unwrap();
    assert_eq!(columns::all(doc.store()).len(), before);
    run(&r, &mut doc, "edit.redo", json!({})).unwrap();

    // Location marks: lettered line first; off a crossing, the offset in millimetres.
    let marks = run(
        &r,
        &mut doc,
        "column.location_mark",
        json!({ "column_ids": [placed[0], c.0] }),
    )
    .unwrap();
    let m0 = marks[0]["mark"].as_str().unwrap();
    assert!(m0.starts_with(['A', 'B', 'C']) && !m0.contains('('), "{m0}");
    assert_eq!(marks[1]["mark"], "A-1 (+0, -2000)");

    // IFC: load-bearing columns; hollow and I bodies saved and reopened.
    project.save(&doc).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("LoadBearing"));
    drop(project);
    let (_p, doc2, _) = LocalProject::open(&path).unwrap();
    let g2 = columns::regenerate(doc2.store(), doc2.store().get(h).unwrap()).unwrap();
    assert_eq!(g2.holes.len(), 1);
    assert!(
        doc2.store()
            .get(i)
            .unwrap()
            .param("Pset_ColumnCommon.LoadBearing")
            .unwrap()["value"]
            == true
    );
}

#[test]
fn catalogue_sections_regional_defaults_and_slenderness() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("k.ifc");
    let (_project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();

    // A US model: AISC catalogues first, A992 steel, f'c concrete, AISC 360 and ACI 318.
    run(&r, &mut doc, "code.set", json!({ "code": "ibc" })).unwrap();
    let s = run(&r, &mut doc, "structural.standards", json!({})).unwrap();
    assert_eq!(s["region"], "united_states");
    assert_eq!(s["steel_standard"], "AISC 360");
    let cats = run(&r, &mut doc, "structural.catalogs", json!({})).unwrap();
    assert_eq!(cats[0]["id"], "aisc_w");
    assert_eq!(cats[0]["preferred"], true);
    // A W shape from the catalogue: its type carries the designation and the usual grade.
    let w = id(run(
        &r,
        &mut doc,
        "column.load_section",
        json!({ "catalog": "aisc_w", "designation": "W10X49" }),
    )
    .unwrap());
    // Loading it again reuses the type.
    assert_eq!(
        id(run(
            &r,
            &mut doc,
            "column.load_section",
            json!({ "catalog": "aisc_w", "designation": "W10X49" })
        )
        .unwrap()),
        w
    );
    let t = columns::column_type(doc.store().get(w).unwrap()).unwrap();
    assert_eq!(t.designation.as_deref(), Some("W10X49"));
    assert_eq!(t.material.as_deref(), Some("Steel ASTM A992"));
    let c = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "type_id": w.0, "point": [0.0, 0.0], "base_level": l1.0, "top_constraint": { "unconnected": 4.0 } }),
    )
    .unwrap());
    // A default concrete column in a US model is f'c 4000 psi.
    let k = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "point": [5.0, 0.0], "base_level": l1.0, "top_constraint": { "unconnected": 4.0 } }),
    )
    .unwrap());
    assert_eq!(
        geometry(&doc, k).ty.material.as_deref(),
        Some("Concrete f'c 4000 psi")
    );
    // A slender pipe column (Pipe 2 STD, 12 m) exceeds the steel guidance; the W10 does not.
    let pipe = id(run(
        &r,
        &mut doc,
        "column.load_section",
        json!({ "catalog": "aisc_pipe_std", "designation": "Pipe2STD" }),
    )
    .unwrap());
    let slender = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "type_id": pipe.0, "point": [10.0, 0.0], "base_level": l1.0, "top_constraint": { "unconnected": 12.0 } }),
    )
    .unwrap());
    let adv = run(&r, &mut doc, "column.check", json!({})).unwrap();
    let find = |c: ElementId| {
        adv.as_array()
            .unwrap()
            .iter()
            .find(|a| a["column_id"] == c.0)
            .cloned()
            .unwrap()
    };
    assert_eq!(find(slender)["limit"], 200.0);
    assert!(find(slender)["note"].as_str().unwrap().contains("AISC 360"));
    assert!(find(c)["limit"].is_null());
    // A concrete column 12 m tall: k·lu/r over 22 under ACI 318.
    let tall = id(run(
        &r,
        &mut doc,
        "column.place",
        json!({ "point": [15.0, 0.0], "base_level": l1.0, "top_constraint": { "unconnected": 12.0 } }),
    )
    .unwrap());
    let adv = run(
        &r,
        &mut doc,
        "column.check",
        json!({ "column_ids": [tall.0] }),
    )
    .unwrap();
    assert_eq!(adv[0]["limit"], 22.0);

    // A German model: European sections first, S355 and C30/37, Eurocodes.
    run(&r, &mut doc, "code.set", json!({ "code": "din_18065" })).unwrap();
    let s = run(&r, &mut doc, "structural.standards", json!({})).unwrap();
    assert_eq!(s["concrete"], "Concrete C30/37");
    assert_eq!(s["steel_standard"], "EN 1993");
    let cats = run(&r, &mut doc, "structural.catalogs", json!({})).unwrap();
    assert_eq!(cats[0]["id"], "eu_heb");
    let adv = run(
        &r,
        &mut doc,
        "column.check",
        json!({ "column_ids": [tall.0] }),
    )
    .unwrap();
    assert!(adv[0]["limit"].is_null());
    assert!(adv[0]["note"].as_str().unwrap().contains("EN 1992"));
}
