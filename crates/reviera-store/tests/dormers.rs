// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Dormers (ARC-ROOF-007): gable, hipped, shed and flat dormers on a sloped roof face, their
//! gap walls filling down to the main roof, edits rebuilding them, and their parts going with
//! them.

use reviera_core::{Caller, CommandRegistry, Document, ElementId, dormers, roofs, walls};
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

/// The dormer's parts: its roof and its walls.
fn parts(doc: &Document, dormer: ElementId) -> (ElementId, Vec<ElementId>) {
    let r = dormers::dormer_recipe(doc.store().get(dormer).unwrap()).unwrap();
    let ids: Vec<ElementId> = r
        .parts
        .iter()
        .map(|g| {
            doc.store()
                .find_by_guid(&reviera_core::GlobalId::parse(g).unwrap())
                .unwrap()
        })
        .collect();
    let roof = *ids
        .iter()
        .find(|i| roofs::roof_recipe(doc.store().get(**i).unwrap()).is_some())
        .unwrap();
    let ws = ids.into_iter().filter(|i| *i != roof).collect();
    (roof, ws)
}

#[test]
fn dormers_of_every_style_fill_their_gaps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dormers.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let level = doc
        .store()
        .iter()
        .find(|e| e.class == "IfcBuildingStorey")
        .unwrap()
        .id();
    // A 10 × 6 m gable roof at 45° on a 3 m plate: the south face rises northward.
    let outer = [[-0.1, -0.1], [10.1, -0.1], [10.1, 6.1], [-0.1, 6.1]];
    let curves: Vec<Value> = (0..4)
        .map(|i| {
            json!({
                "line": { "start": outer[i], "end": outer[(i + 1) % 4] },
                "defines_slope": i % 2 == 0,
            })
        })
        .collect();
    let main = id(run(
        &r,
        &mut doc,
        "roof.create_footprint",
        json!({ "curves": curves, "level_id": level.0, "base_offset": 3.0, "slope": "45°" }),
    )
    .unwrap());
    let main_g = roofs::regenerate(doc.store(), doc.store().get(main).unwrap()).unwrap();
    // The main roof's top surface over a point of the south face.
    let main_top = |p: [f64; 2]| main_g.underside(p).unwrap() + main_g.thickness * 2f64.sqrt();

    // Placed too high, the dormer would run over the ridge: refused.
    let err = run(
        &r,
        &mut doc,
        "dormer.create",
        json!({
            "roof_id": main.0, "point": [5.0, 1.0], "style": "gable",
            "width": 2.0, "front_height": 1.2,
        }),
    )
    .unwrap_err();
    assert_eq!(err.code, "dormer.does_not_fit");
    // A gable dormer 2 m wide, its eave 1 m above the roof, at 45°.
    let dormer = id(run(
        &r,
        &mut doc,
        "dormer.create",
        json!({
            "roof_id": main.0, "point": [5.0, 0.5], "style": "gable",
            "width": 2.0, "front_height": 1.0,
        }),
    )
    .unwrap());
    let (droof, ws) = parts(&doc, dormer);
    assert_eq!(ws.len(), 3);
    // The walls stop at the main roof's underside: no point of them is below it, so the cheek
    // walls follow the slope instead of hanging into the room.
    let bodies = reviera_core::bodies::all(doc.store());
    for w in &ws {
        let points: Vec<[f64; 3]> = bodies
            .iter()
            .filter(|b| b.element == *w)
            .flat_map(|b| b.parts.iter().flat_map(|p| p.mesh.0.clone()))
            .collect();
        assert!(!points.is_empty());
        for p in points {
            let under = main_g.underside([p[0], p[1]]).unwrap_or(f64::NEG_INFINITY);
            assert!(
                p[2] >= under - 1e-6,
                "{p:?} is below the roof's underside {under}"
            );
        }
    }
    let g = roofs::regenerate(doc.store(), doc.store().get(droof).unwrap()).unwrap();
    assert_eq!(g.predefined, "GABLE_ROOF");
    let z_eave = main_top([5.0, 0.5]) + 1.0;
    assert!((g.plate - z_eave).abs() < 1e-6);
    // The ridge, 1 m above the eave (half the width at 45°), runs back until it meets the main
    // roof's top surface.
    let p = dormers::placement(doc.store(), doc.store().get(main).unwrap(), [5.0, 0.5]).unwrap();
    let (depth, cheek) = dormers::depths(
        &dormers::dormer_recipe(doc.store().get(dormer).unwrap()).unwrap(),
        &p,
        0.3,
    )
    .unwrap();
    // Its top meets the main roof's top at the back: the ridge's underside is the dormer roof's
    // plumb depth below it.
    let back = [5.0, 0.5 + depth];
    let plumb = 0.3 * 2f64.sqrt();
    assert!(
        (g.underside([5.0, 0.5 + depth - 1e-6]).unwrap() + plumb - main_top(back)).abs() < 1e-4
    );
    // Cut along the main roof's top: nothing of the dormer roof lies below it, so the two
    // surfaces meet flush and rain runs off the dormer into the valleys.
    let main_top_at = |p: [f64; 3]| main_top([p[0], p[1]]);
    for f in &g.faces {
        for p in &f.slab.0 {
            if main_g.underside([p[0], p[1]]).is_some() {
                assert!(p[2] >= main_top_at(*p) - 1e-6, "{p:?} below the main roof");
            }
        }
    }
    // The gap walls: tops on the dormer roof (the front gable reaching the ridge), bases under
    // the main roof.
    let solved = walls::solve(doc.store());
    for w in &ws {
        let body = &solved.bodies[w];
        let a = walls::attach::attached_body(doc.store(), *w, body)
            .unwrap()
            .unwrap();
        assert!(body.geometry.base_z < main_top([5.0, 0.5]));
        assert!(a.z_min >= z_eave - 1e-6, "{} below the eave", a.z_min);
    }
    let front_top = ws
        .iter()
        .map(|w| {
            walls::attach::attached_body(doc.store(), *w, &solved.bodies[w])
                .unwrap()
                .unwrap()
                .z_max
        })
        .fold(f64::MIN, f64::max);
    assert!((front_top - (z_eave + 1.0)).abs() < 0.11, "{front_top}");
    // The cheek walls end where the dormer eave meets the main roof.
    assert!((main_top([5.0, 0.5 + cheek]) - z_eave).abs() < 1e-6);

    // The opening (ARC-OPEN-005): through the main roof inside the walls' interior faces (200 mm
    // walls), from the front wall back to the cheeks' ends.
    let opening = |doc: &Document| {
        let g = roofs::regenerate(doc.store(), doc.store().get(main).unwrap()).unwrap();
        let holes: Vec<Vec<[f64; 3]>> = g.faces.iter().flat_map(|f| f.holes.clone()).collect();
        assert_eq!(holes.len(), 1);
        let xs = holes[0].iter().map(|p| p[0]);
        let ys = holes[0].iter().map(|p| p[1]);
        (
            xs.clone().fold(f64::MAX, f64::min),
            xs.fold(f64::MIN, f64::max),
            ys.clone().fold(f64::MAX, f64::min),
            ys.fold(f64::MIN, f64::max),
            g,
        )
    };
    let (x0, x1, y0, y1, g) = opening(&doc);
    assert!(
        (x0 - 4.2).abs() < 1e-9 && (x1 - 5.8).abs() < 1e-9,
        "{x0} {x1}"
    );
    assert!(
        (y0 - 0.7).abs() < 1e-9 && (y1 - (0.5 + cheek)).abs() < 1e-9,
        "{y0} {y1}"
    );
    assert!(g.underside([5.0, 1.0]).is_none());
    assert!(g.underside([3.0, 1.0]).is_some());
    // Less roof by the opening: its plan area.
    assert!(
        (main_g.projected_area() - g.projected_area() - 1.6 * (y1 - y0)).abs() < 1e-6,
        "{}",
        main_g.projected_area() - g.projected_area()
    );
    // The slab stays a closed solid: less by the opening's plan area times its plumb depth.
    let vol = |g: &roofs::RoofGeometry| -> f64 {
        g.faces
            .iter()
            .map(|f| {
                let p = &f.slab.0;
                f.slab
                    .1
                    .chunks(3)
                    .map(|t| {
                        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
                        (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                            + a[2] * (b[0] * c[1] - b[1] * c[0]))
                            / 6.0
                    })
                    .sum::<f64>()
            })
            .sum()
    };
    let cut = vol(&main_g) - vol(&g);
    let expected = 1.6 * (y1 - y0) * 0.3 * 2f64.sqrt();
    assert!((cut - expected).abs() < 1e-6, "{cut} vs {expected}");
    // Moving the dormer walls 300 mm along the roof moves the opening.
    let ids: Vec<u64> = ws.iter().map(|w| w.0).collect();
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": ids, "vector": [0.3, 0.0, 0.0] }),
    )
    .unwrap();
    let (x0, x1, ..) = opening(&doc);
    assert!(
        (x0 - 4.5).abs() < 1e-9 && (x1 - 6.1).abs() < 1e-9,
        "{x0} {x1}"
    );
    run(
        &r,
        &mut doc,
        "element.move",
        json!({ "element_ids": ids, "vector": [-0.3, 0.0, 0.0] }),
    )
    .unwrap();

    // A window in the dormer's front wall: the default 1.2 × 1.2 m window would start 900 mm
    // above the floor level and stand taller than the wall's open face; it rises above the
    // main roof and shrinks under the dormer roof.
    let front = ws[0];
    let win = id(run(
        &r,
        &mut doc,
        "window.place",
        json!({ "host_wall_id": front.0, "point_along": 0.9 }),
    )
    .unwrap());
    let p = reviera_core::inserts::resolve(doc.store(), doc.store().get(win).unwrap()).unwrap();
    let c = p.cut();
    // Above the main roof's top where it meets the wall's exterior face (inside, the main roof
    // is cut away under the dormer).
    assert!(
        (c.z[0] - (main_top([5.0, 0.5]) + 0.1)).abs() < 1e-9,
        "{:?}",
        c
    );
    // Under the dormer roof's underside: the front wall runs from x = 4.1.
    let droof_g = roofs::regenerate(doc.store(), doc.store().get(droof).unwrap()).unwrap();
    let under = [c.x[0], c.x[1]]
        .iter()
        .map(|x| droof_g.underside([4.1 + x, 0.6]).unwrap())
        .fold(f64::MAX, f64::min);
    assert!(c.z[1] <= under + 1e-6, "{:?} under {under}", c);
    assert!(p.ty.height > 0.3, "{}", p.ty.height);
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [win.0] }),
    )
    .unwrap();

    // The other styles.
    for (style, kind) in [
        ("hip", "HIPPED_GABLE_ROOF"),
        ("shed", "SHED_ROOF"),
        ("flat", "FLAT_ROOF"),
    ] {
        run(
            &r,
            &mut doc,
            "dormer.set",
            json!({ "dormer_id": dormer.0, "style": style }),
        )
        .unwrap();
        let (droof, ws) = parts(&doc, dormer);
        assert_eq!(ws.len(), 3, "{style}");
        let g = roofs::regenerate(doc.store(), doc.store().get(droof).unwrap()).unwrap();
        assert_eq!(g.predefined, kind, "{style}");
        let solved = walls::solve(doc.store());
        for w in &ws {
            walls::attach::attached_body(doc.store(), *w, &solved.bodies[w])
                .unwrap()
                .unwrap();
        }
    }
    // The flat dormer (1 m front) is lower: a window shrinks under its roof.
    let (_, ws) = parts(&doc, dormer);
    let win = id(run(
        &r,
        &mut doc,
        "window.place",
        json!({ "host_wall_id": ws[0].0, "point_along": 0.9 }),
    )
    .unwrap());
    let p = reviera_core::inserts::resolve(doc.store(), doc.store().get(win).unwrap()).unwrap();
    assert!(p.ty.height < 1.2 - 1e-6, "{}", p.ty.height);
    assert_eq!(p.recipe.height, Some(p.ty.height));
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [win.0] }),
    )
    .unwrap();
    // Only one set of parts exists at a time.
    let count = doc
        .store()
        .iter()
        .filter(|e| dormers::dormer_of(e).is_some())
        .count();
    assert_eq!(count, 4);
    // A shed steeper than the roof is refused, and the dormer stays as it was.
    let err = run(
        &r,
        &mut doc,
        "dormer.set",
        json!({ "dormer_id": dormer.0, "style": "shed", "slope": "50°" }),
    )
    .unwrap_err();
    assert_eq!(err.code, "dormer.shed_too_steep");
    // Off the roof is refused.
    let err = run(
        &r,
        &mut doc,
        "dormer.create",
        json!({ "roof_id": main.0, "point": [30.0, 1.0] }),
    )
    .unwrap_err();
    assert_eq!(err.code, "dormer.off_roof");

    // Saved and reopened, the parts are there.
    project.save(&doc).unwrap();
    let ifc = IfcModel::open(&path).unwrap();
    assert_eq!(ifc.entities_of_type("IfcRoof").unwrap().len(), 2);
    assert_eq!(ifc.entities_of_type("IfcElementAssembly").unwrap().len(), 1);
    let (_, reopened, _) = LocalProject::open(&path).unwrap();
    assert_eq!(parts(&reopened, dormer).1.len(), 3);

    // Deleting the dormer deletes its parts; deleting the roof deletes its dormers.
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [dormer.0] }),
    )
    .unwrap();
    assert!(doc.store().iter().all(|e| dormers::dormer_of(e).is_none()));
    let again = id(run(
        &r,
        &mut doc,
        "dormer.create",
        json!({ "roof_id": main.0, "point": [3.0, 0.5], "style": "shed" }),
    )
    .unwrap());
    run(
        &r,
        &mut doc,
        "element.delete",
        json!({ "element_ids": [main.0] }),
    )
    .unwrap();
    assert!(doc.store().get(again).is_none());
    assert!(doc.store().iter().all(|e| dormers::dormer_of(e).is_none()));
}
