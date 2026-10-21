// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Pressure drop (MEP-CALC-002) and the pressure loss report (MEP-CALC-004): a supply air system
//! of three diffusers calculated from its air handling unit, the critical path to the farthest
//! diffuser and a schedule filtered to it, stale results after a resize, overrides, the CSV
//! report and its totals, a Flow Only system refused, and the results on the IFC ports.

use std::collections::{HashMap, HashSet};

use reviera_core::mep_network::NetworkResult;
use reviera_core::{Caller, CommandRegistry, Document, ElementId};
use reviera_ifc::Schema;
use reviera_store::LocalProject;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::with_core();
    reviera_store::commands::register(&mut r);
    r
}

fn run(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> reviera_core::Result<reviera_core::command::Outcome> {
    r.execute(doc, name, args, &Caller::user())
}

fn ok(r: &CommandRegistry, doc: &mut Document, name: &str, args: Value) -> Value {
    run(r, doc, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn id(v: &Value) -> ElementId {
    ElementId(v.as_u64().unwrap_or_else(|| panic!("{v}")))
}

fn list(r: &CommandRegistry, doc: &mut Document, sys: ElementId) -> NetworkResult {
    serde_json::from_value(ok(r, doc, "analytical_segment.list", json!({ "system_id": sys.0 }))).unwrap()
}

#[test]
fn supply_air_pressure_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("net.ifc");
    let (mut project, mut doc) = LocalProject::new(Schema::Ifc4, &path).unwrap();
    let r = registry();
    let l1 = doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let t = ok(&r, &mut doc, "mep_type.load_starters", json!({ "names": ["Air Handling Unit", "Supply Diffuser 600"] }));
    // Each diffuser loses 25 Pa.
    ok(&r, &mut doc, "connector.set", json!({ "element_id": t["Supply Diffuser 600"], "connector_index": 0, "loss_method": "specific_loss", "loss": 25.0 }));
    let ahu = id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Air Handling Unit"], "point": [0, 0], "level": l1.0 })));
    let xs = [6.0, 9.0, 12.0];
    let diffusers: Vec<ElementId> =
        xs.iter().map(|x| id(&ok(&r, &mut doc, "mesh_object.place", json!({ "type_id": t["Supply Diffuser 600"], "point": [x, 3.0], "level": l1.0, "offset": 2.7 })))).collect();
    let sys = id(&ok(&r, &mut doc, "system.create", json!({ "kind": "duct", "element_ids": diffusers.iter().map(|d| d.0).collect::<Vec<_>>() })));
    ok(&r, &mut doc, "system.select_equipment", json!({ "system_id": sys.0, "equipment_id": ahu.0 }));
    ok(&r, &mut doc, "duct.create", json!({ "points": [[1.5, 0, 0.9], [3, 0, 0.9], [3, 0, 3.6], [14, 0, 3.6]], "level": l1.0, "shape": "round", "diameter": 0.4, "system_type_id": "supply_air" }));
    for x in xs {
        ok(&r, &mut doc, "duct.create", json!({ "points": [[x, 0, 3.6], [x, 3, 3.6], [x, 3, 2.85]], "level": l1.0, "shape": "round", "diameter": 0.25, "system_type_id": "supply_air" }));
    }
    // Not calculated yet.
    assert_eq!(run(&r, &mut doc, "analytical_segment.list", json!({ "system_id": sys.0 })).unwrap_err().code, "mep_network.not_calculated");

    let rep = ok(&r, &mut doc, "mep_network.calculate", json!({ "system_id": sys.0 }));
    assert_eq!(rep["systems"][0]["system"], sys.0);
    let n = list(&r, &mut doc, sys);
    assert!(!n.stale);
    assert!((n.total_flow - 0.3).abs() < 1e-9, "{}", n.total_flow);
    // The critical path: from the AHU to the farthest diffuser.
    assert_eq!(n.critical_path.first(), Some(&ahu));
    assert_eq!(n.critical_path.last(), Some(&diffusers[2]), "{:?}", n.terminals);
    let crit = n.critical_loss.unwrap();
    // It is the sum of the losses along it: segments, the branch leaving each fitting or tap
    // toward the next element, and the diffuser's 25 Pa.
    let by: HashMap<ElementId, _> = n.segments.iter().map(|a| (a.element, a)).collect();
    let mut sum = 25.0;
    for w in n.critical_path.windows(2) {
        if let Some(a) = by.get(&w[0]) {
            if a.kind == "segment" {
                sum += a.loss.unwrap();
            }
            sum += a.branches.iter().find(|b| b.to == w[1]).map_or(0.0, |b| b.loss);
        }
    }
    assert!((sum - crit).abs() < 1e-6, "{sum} {crit}");
    // Fittings on the path with their coefficients: the elbows at 1.2 (mitred 90°).
    assert!(n.segments.iter().any(|a| a.critical && a.part.as_deref() == Some("Elbow") && (a.coefficient.unwrap() - 1.2).abs() < 1e-9));
    // Excess pressure: the critical-path loss less each terminal's, within 1 Pa.
    for t in &n.terminals {
        assert!((t.excess - (crit - t.path_loss)).abs() < 1.0);
        assert!(t.excess >= -1e-9);
    }
    assert!(n.terminals.iter().find(|t| t.element == diffusers[0]).unwrap().excess > 0.0);
    // Segment friction and loss agree with Darcy–Weisbach over its length.
    let s0 = n.segments.iter().find(|a| a.kind == "segment" && a.critical).unwrap();
    assert!((s0.loss.unwrap() - s0.friction.unwrap() * s0.length.unwrap()).abs() < 1e-9);

    // A schedule of ducts filtered by Critical Path = Yes lists exactly the path's segments.
    let sch = id(&ok(&r, &mut doc, "schedule.create", json!({ "category": "cat.ducts", "fields": ["mep.size", "mep.flow", "mep.pressure_drop", "mep.critical_path"] })));
    ok(&r, &mut doc, "schedule.set_filters", json!({ "schedule_id": sch.0, "filters": [{ "field": "mep.critical_path", "op": "equals", "value": "Yes" }] }));
    let table = reviera_core::schedules::table(doc.store(), doc.store().get(sch).unwrap(), &HashMap::new()).unwrap();
    let listed: HashSet<ElementId> = table.rows.iter().flat_map(|r| r.elements.clone()).collect();
    let on_path: HashSet<ElementId> = n.critical_path.iter().copied().filter(|e| by.get(e).is_some_and(|a| a.kind == "segment")).collect();
    assert_eq!(listed, on_path);

    // A resize makes the results stale until calculated again.
    ok(&r, &mut doc, "duct.set_size", json!({ "element_ids": [s0.element.0], "diameter": 0.5 }));
    assert!(list(&r, &mut doc, sys).stale);
    ok(&r, &mut doc, "mep_network.calculate", json!({}));
    let n = list(&r, &mut doc, sys);
    assert!(!n.stale);

    // An override: a measured 40 Pa on one branch segment.
    let branch = n.segments.iter().find(|a| a.kind == "segment" && !a.critical).unwrap().element;
    ok(&r, &mut doc, "analytical_segment.override", json!({ "element_id": branch.0, "loss": 40.0 }));
    assert!(list(&r, &mut doc, sys).stale);
    assert_eq!(run(&r, &mut doc, "analytical_segment.override", json!({ "element_id": branch.0, "coefficient": 1.0 })).unwrap_err().code, "analytical_segment.coefficient");

    // The CSV report: it recalculates first, lists every segment and fitting, totals as listed.
    let csv = dir.path().join("loss.csv");
    let out = ok(&r, &mut doc, "pressure_loss_report.export", json!({ "system_ids": [sys.0], "path": csv, "format": "csv" }));
    assert_eq!(out["recalculated"], json!([sys.0]));
    let n = list(&r, &mut doc, sys);
    let text = std::fs::read_to_string(&csv).unwrap();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    assert!(header.contains(&"flow (m3/s)") && header.contains(&"loss (Pa)"));
    let col = |h: &str| header.iter().position(|x| *x == h).unwrap();
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
    let detail: Vec<&Vec<&str>> = rows.iter().filter(|r| r[0] == "segments" || r[0] == "fittings").collect();
    assert_eq!(detail.len(), n.segments.len());
    for a in &n.segments {
        let row = detail.iter().find(|r| r[col("element")] == a.element.0.to_string()).unwrap();
        assert!(!row[col("size")].is_empty() && !row[col("velocity (m/s)")].is_empty());
        assert_eq!(row[col("overridden")] == "yes", a.element == branch);
    }
    let total: f64 = rows.iter().find(|r| r[0] == "total").unwrap()[col("loss (Pa)")].parse().unwrap();
    assert!((total - n.critical_loss.unwrap()).abs() < 1e-5);
    // A second export over it needs overwrite.
    assert_eq!(run(&r, &mut doc, "pressure_loss_report.export", json!({ "system_ids": [sys.0], "path": csv })).unwrap_err().code, "file.exists");
    let html = dir.path().join("loss.html");
    ok(&r, &mut doc, "pressure_loss_report.save_preset", json!({ "name": "Short", "settings": { "without_fittings": true } }));
    ok(&r, &mut doc, "pressure_loss_report.export", json!({ "system_ids": [sys.0], "path": html, "format": "html", "settings": { "preset_id": "short" } }));
    let page = std::fs::read_to_string(&html).unwrap();
    assert!(page.contains("Critical path") && !page.contains("Fitting and accessory detail"));

    // The results on the segments' IFC ports.
    project.save(&doc).unwrap();
    let ifc = std::fs::read_to_string(&path).unwrap();
    assert!(ifc.contains("'VolumetricFlowRate'") && ifc.contains("IFCVOLUMETRICFLOWRATEMEASURE"));

    // The System Inspector (MEP-CALC-003): the main's far end is open, so it is refused.
    let e = run(&r, &mut doc, "system_inspector.start", json!({ "system_id": sys.0 })).unwrap_err();
    assert_eq!(e.message, "System has open connectors: 1. Run Show Disconnects");
    ok(&r, &mut doc, "duct.cap_open_ends", json!({}));
    // Capped, the results are stale until calculated.
    assert_eq!(run(&r, &mut doc, "system_inspector.start", json!({ "system_id": sys.0 })).unwrap_err().code, "system_inspector.stale");
    ok(&r, &mut doc, "mep_network.calculate", json!({ "system_id": sys.0 }));
    let n: NetworkResult = serde_json::from_value(ok(&r, &mut doc, "system_inspector.start", json!({ "system_id": sys.0 }))).unwrap();
    let crit = n.critical_loss.unwrap();
    // Every joined segment and fitting is in exactly one section.
    let mut seen: Vec<ElementId> = n.sections.iter().flat_map(|s| s.elements.clone()).collect();
    seen.sort();
    let mut all: Vec<ElementId> = n.segments.iter().filter(|a| a.connected).map(|a| a.element).collect();
    all.sort();
    assert_eq!(seen, all);
    // The first section leaves the AHU with all the flow at its full pressure; each section
    // starts where the one before it ended, less the junction's branch loss.
    let first = &n.sections[0];
    assert!(first.critical && (first.flow - 0.3).abs() < 1e-9 && (first.pressure_in.unwrap() - crit).abs() < 1e-9);
    // The critical chain's losses add up to the critical loss with the diffuser's 25 Pa, and the
    // last critical section ends with the 25 Pa the diffuser takes.
    let chain: Vec<_> = n.sections.iter().filter(|s| s.critical).collect();
    let total: f64 = chain.iter().map(|s| s.loss.unwrap()).sum::<f64>() + 25.0;
    assert!((total - crit).abs() < 1e-6, "{total} {crit}");
    // (The 40 Pa override above may have moved the critical path to another diffuser.)
    let ct = *n.critical_path.last().unwrap();
    let last = chain.last().unwrap();
    assert_eq!(last.terminal, Some(ct));
    assert!((last.pressure_out.unwrap() - 25.0).abs() < 1e-6);
    // Branch sections carry a diffuser's 0.1 m³/s; the terminals' excess pressure.
    let other = *diffusers.iter().find(|d| **d != ct).unwrap();
    assert!(n.sections.iter().any(|s| !s.critical && (s.flow - 0.1).abs() < 1e-9 && s.terminal == Some(other)));
    assert!(n.sections.iter().all(|s| s.points.len() >= 2));
    let t0 = n.terminals.iter().find(|t| t.element == other).unwrap();
    assert!((t0.excess - (crit - t0.path_loss)).abs() < 1.0 && t0.excess > 0.0);

    // Flow Only: flows without losses, and no report.
    ok(&r, &mut doc, "system_type.set", json!({ "type_id": "supply_air", "calculations": "flow_only" }));
    ok(&r, &mut doc, "mep_network.calculate", json!({ "system_id": sys.0 }));
    let n = list(&r, &mut doc, sys);
    // (The main's stub past the last branch carries none.)
    assert!(n.critical_loss.is_none() && n.segments.iter().all(|a| a.loss.is_none()));
    assert!(n.segments.iter().filter(|a| a.flow > 0.0).count() + 2 >= n.segments.len());
    let e = run(&r, &mut doc, "pressure_loss_report.export", json!({ "system_ids": [sys.0], "path": dir.path().join("x.csv") })).unwrap_err();
    assert!(e.message.contains("Flow Only"), "{}", e.message);
}
