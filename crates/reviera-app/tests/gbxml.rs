// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! gbXML exchange end to end (ANA-EM-004, ANA-ENV-009): four rooms and a window exported as
//! gbXML (space ids map back to GlobalIds, shared walls once, the window an opening of its
//! wall); loads written into the file by "another tool" import onto the spaces, an edited value
//! re-imports, and an id that matches nothing is listed.

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn ok(app: &mut App, name: &str, args: Value) -> Value {
    app.execute(name, args, &Caller::user()).unwrap_or_else(|e| panic!("{name}: {e:?}")).result
}

fn heat_loss(app: &App, id: u64) -> Option<f64> {
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    store.get(reviera_core::ElementId(id))?.param("Pset_SpaceThermalDesign.TotalHeatLoss")?.get("value")?.as_f64()
}

#[test]
fn gbxml_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.path().join("untitled") });
    ok(&mut app, "file.new", json!({}));
    let level = app.document(app.active().unwrap()).unwrap().doc.store().iter().find(|e| e.class == "IfcBuildingStorey").unwrap().id();
    let mut walls = Vec::new();
    for (a, b) in [([0, 0], [8, 0]), ([8, 0], [8, 8]), ([8, 8], [0, 8]), ([0, 8], [0, 0]), ([4, 0], [4, 8]), ([0, 4], [8, 4])] {
        walls.push(ok(&mut app, "wall.create", json!({ "curve": { "line": { "start": a, "end": b } }, "base_level": level.0, "top_constraint": { "unconnected": 3.0 } })));
    }
    let spaces = ok(&mut app, "space.place_auto", json!({ "level_id": level.0 }));
    ok(&mut app, "window.place", json!({ "host_wall_id": walls[0], "point_along": 2.0 }));

    let path = dir.path().join("model.xml");
    let r = ok(&mut app, "gbxml.export", json!({ "path": path }));
    assert_eq!(r["spaces"], 4);
    assert_eq!(r["openings"], 1);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("version=\"6.01\"") && text.contains("gbXML 8.01 schema") && text.contains("xmlns=\"http://www.gbxml.org/schema\""));
    // Each space id maps back to its space's GlobalId.
    let store = app.document(app.active().unwrap()).unwrap().doc.store().clone();
    let ids: Vec<(u64, String)> = spaces.as_array().unwrap().iter().map(|v| {
        let id = v.as_u64().unwrap();
        (id, reviera_core::gbxml::space_id(&store.get(reviera_core::ElementId(id)).unwrap().guid().to_string()))
    }).collect();
    for (_, sid) in &ids {
        assert!(text.contains(&format!("<Space id=\"{sid}\"")), "{sid}");
    }
    // Two crossing partitions make four shared wall surfaces, each written once.
    assert_eq!(text.matches("surfaceType=\"InteriorWall\"").count(), 4);
    assert_eq!(text.matches("surfaceType=\"ExteriorWall\"").count(), 8);
    assert_eq!(text.matches("<Opening ").count(), 1);
    assert!(text.matches("<AdjacentSpaceId").count() >= 4 * 2 + 8 + 8);

    // Another tool writes loads for three spaces and one unknown id.
    let results = |w: f64| {
        let mut s = String::new();
        for (k, (_, sid)) in ids.iter().take(3).enumerate() {
            s.push_str(&format!("  <Results id=\"h{k}\" objectIdRef=\"{sid}\" resultsType=\"HeatLoad\" unit=\"Kilowatt\"><Value>{}</Value></Results>\n", w + k as f64));
            s.push_str(&format!("  <Results id=\"c{k}\" objectIdRef=\"{sid}\" resultsType=\"CoolingLoad\" unit=\"Watt\"><Value>900</Value></Results>\n"));
        }
        s
    };
    let with = |w: f64| {
        let extra = text.replace("</Building>", "  <Space id=\"sp-NOTHERE\"><Name>Ghost</Name></Space>\n    </Building>");
        extra.replace("</gbXML>", &format!("{}  <Results id=\"g\" objectIdRef=\"sp-NOTHERE\" resultsType=\"HeatLoad\" unit=\"Watt\"><Value>5</Value></Results>\n</gbXML>", results(w)))
    };
    let back = dir.path().join("loads.xml");
    std::fs::write(&back, with(1.5)).unwrap();
    let rep = ok(&mut app, "gbxml.import_loads", json!({ "path": back }));
    assert_eq!(rep["matched"].as_array().unwrap().len(), 3, "{rep}");
    assert_eq!(rep["unmatched"], json!(["sp-NOTHERE"]));
    assert_eq!(rep["without_results"].as_array().unwrap().len(), 1);
    assert_eq!(heat_loss(&app, ids[0].0), Some(1500.0));
    assert_eq!(heat_loss(&app, ids[3].0), None);
    // Edited in the other tool: re-importing updates the value.
    std::fs::write(&back, with(4.0)).unwrap();
    ok(&mut app, "gbxml.import_loads", json!({ "path": back }));
    assert_eq!(heat_loss(&app, ids[0].0), Some(4000.0));
    assert_eq!(heat_loss(&app, ids[2].0), Some(6000.0));
}
