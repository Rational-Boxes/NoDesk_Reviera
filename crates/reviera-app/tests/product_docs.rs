// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Materials documented on FileEngine (COL-FE-006, COL-FE-007): a material linked to a folder of
//! its EPD and HPD, values read from them with the (fake) extraction service, accepted onto the
//! material in one undo step with their sources, the EPD's GWP as a carbon factor, the folder's
//! metadata, and the IFC export. Against the in-memory FileEngine here; against a real one in
//! `a_real_product_folder` when `REVIERA_FILEENGINE_URL`, `_KEY`, `_SECRET` and
//! `REVIERA_FILEENGINE_PRODUCT_FOLDER` (a folder holding an EPD and an HPD, typed) are set.

use reviera_app::fileengine::Connections;
use reviera_app::{App, AppConfig};
use reviera_core::{Caller, Origin};
use reviera_fileengine::Client;
use reviera_fileengine::fake::Fake;
use serde_json::{Value, json};

fn new_app(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig { live: None, untitled_dir: dir.join("untitled") });
    *app.fileengine_mut() = Connections::isolated(dir.join("fileengine.json"), dir.join("copies"));
    app
}

fn run(app: &mut App, name: &str, args: Value) -> reviera_core::Result<Value> {
    app.execute(name, args, &Caller::user()).map(|o| o.result)
}

fn add_wall(app: &mut App) {
    let level = run(app, "level.list", json!({})).unwrap()[0]["id"].clone();
    run(app, "wall.create", json!({ "curve": { "line": { "start": [0, 0], "end": [6, 0] } }, "base_level": level, "top_constraint": { "unconnected": 3.0 } })).unwrap();
}

const EPD: &str = "Environmental Product Declaration\n\
manufacturer = Acme Gypsum\n\
product = Type X 5/8 in Board\n\
epd_number = EPD-TEST-0001\n\
program_operator = Test Programme\n\
declared_unit = 1,000 ft2\n\
gwp_a1a3 = 126 | kg CO2 eq\n\
valid_until = 2030-12-15\n";

const HPD: &str = "Health Product Declaration\n\
hpd_id = 123456789\n\
standard_version = v2.3\n\
classification = 09 29 00\n\
threshold_ppm = 100 | ppm\n\
valid_until = 2027-10-17\n";

/// A product folder under Engineering holding an EPD and an HPD (typed), and a datasheet (not).
fn product_folder(fake: &Fake) -> (String, [String; 3]) {
    let c = Client::new(&fake.url, true).unwrap();
    c.sign_in("fesk_test", "fesks_secret").unwrap();
    let eng = fake.uid_of("Engineering").unwrap();
    let folder = c.create_dir(&eng, "Acme Type X Board").unwrap();
    let add = |name: &str, text: &str, kind: Option<&str>| {
        let uid = c.create_file(&folder, name).unwrap();
        c.write(&uid, text.as_bytes()).unwrap();
        if let Some(k) = kind {
            c.set_metadata(&uid, "doc.type", k).unwrap();
        }
        uid
    };
    let epd = add("epd.txt", EPD, Some("EPD"));
    let hpd = add("hpd.txt", HPD, None);
    let sheet = add("datasheet.txt", "gwp_a1a3 = 999\n", None);
    (folder, [epd, hpd, sheet])
}

#[test]
fn a_material_documented_from_its_epd_and_hpd() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start("fesk_test", "fesks_secret", "ann@acme.test");
    let mut app = new_app(dir.path());
    let conn = run(&mut app, "fileengine.connect", json!({ "url": fake.url, "name": "Acme files" })).unwrap()["id"].as_str().unwrap().to_owned();
    run(&mut app, "fileengine.sign_in", json!({ "connection_id": conn, "key": "fesk_test", "secret": "fesks_secret" })).unwrap();
    let (folder, [epd, hpd, _sheet]) = product_folder(&fake);

    let path = dir.path().join("board.ifc");
    run(&mut app, "file.new", json!({})).unwrap();
    add_wall(&mut app);
    // The wall's layers name the materials.
    let mats = run(&mut app, "docs.materials", json!({})).unwrap();
    let material = mats.as_array().unwrap().iter().find(|m| m["in_use"] == true).expect("a wall material")["material"].as_str().unwrap().to_owned();

    let sets = run(&mut app, "docs.field_sets", json!({})).unwrap();
    assert_eq!(sets.as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect::<Vec<_>>(), ["epd", "hpd"]);
    assert_eq!(sets[0]["pset"], "Reviera_EPD");

    let link = run(&mut app, "docs.link", json!({ "material": material, "connection_id": conn, "folder_uid": folder })).unwrap();
    assert_eq!(link["folder_name"], "Acme Type X Board");
    assert!(link["url"].as_str().unwrap().ends_with("/api/"));
    // The documents, with their types; the HPD is typed next.
    let docs = run(&mut app, "docs.list", json!({ "material": material })).unwrap();
    assert_eq!(docs.as_array().unwrap().len(), 3);
    assert_eq!(run(&mut app, "docs.extract", json!({ "material": material, "set": "hpd" })).unwrap_err().code, "docs.no_documents");
    run(&mut app, "docs.set_doc_type", json!({ "connection_id": conn, "file_uid": hpd, "doc_type": "HPD" })).unwrap();
    let docs = run(&mut app, "docs.list", json!({ "material": material })).unwrap();
    let typed: Vec<_> = docs.as_array().unwrap().iter().filter_map(|d| d["doc_type"].as_str()).collect();
    assert_eq!(typed, ["EPD", "HPD"]);

    // The EPD's values, each with its quote; only the EPD is read (not the datasheet).
    let r = run(&mut app, "docs.extract", json!({ "material": material, "set": "epd" })).unwrap();
    assert_eq!(r["documents"].as_array().unwrap().len(), 1);
    assert_eq!(r["documents"][0]["file_uid"], epd.as_str());
    let field = |r: &Value, f: &str| r["documents"][0]["fields"].as_array().unwrap().iter().find(|v| v["field"] == f).unwrap().clone();
    assert_eq!(field(&r, "gwp_a1a3")["value"], 126.0);
    assert_eq!(field(&r, "gwp_a1a3")["unit"], "kg CO2 eq");
    assert_eq!(field(&r, "gwp_a1a3")["quote"], "gwp_a1a3 = 126 | kg CO2 eq");
    assert_eq!(field(&r, "epd_number")["value"], "EPD-TEST-0001");
    assert_eq!(r["documents"][0]["cached"], false);
    let epd_run = r["run_id"].as_str().unwrap().to_owned();
    // Nothing is written by extracting.
    assert!(run(&mut app, "docs.materials", json!({})).unwrap().as_array().unwrap().iter().all(|m| m["values"].as_object().unwrap().is_empty()));

    // Accepted, with the carbon factor and into the folder's metadata, in one undo step.
    let applied = run(&mut app, "docs.apply_extracted", json!({ "run_id": epd_run, "carbon": true, "to_folder": true })).unwrap();
    assert_eq!(applied["material"], material.as_str());
    assert_eq!(applied["written"].as_array().unwrap().len(), 7, "every field found but the one the EPD does not give");
    assert_eq!(applied["factor_unit"], "m2");
    assert!((applied["factor_per_unit"].as_f64().unwrap() - 126.0 / 92.903_04).abs() < 1e-9, "{applied}");
    let meta = fake.metadata(&folder);
    assert_eq!((meta["epd.number"].as_str(), meta["epd.gwp_a1a3"].as_str(), meta["product.manufacturer"].as_str()), ("EPD-TEST-0001", "126.0", "Acme Gypsum"));
    let m = |app: &mut App| run(app, "docs.materials", json!({})).unwrap().as_array().unwrap().iter().find(|m| m["material"] == material.as_str()).unwrap().clone();
    let v = m(&mut app);
    assert_eq!(v["values"]["epd"]["gwp_a1a3"]["value"], 126.0);
    assert_eq!(v["values"]["epd"]["gwp_a1a3"]["property"], "GWP_A1A3");
    assert_eq!(v["values"]["epd"]["valid_until"]["source"]["file_uid"], epd.as_str());
    assert_eq!(v["values"]["epd"]["valid_until"]["source"]["run_id"], epd_run.as_str());
    let carbon = run(&mut app, "embodied_carbon.materials", json!({})).unwrap();
    let row = carbon.as_array().unwrap().iter().find(|r| r["material"] == material.as_str()).unwrap().clone();
    assert_eq!(row["status"], "matched");
    assert!(row["factor_name"].as_str().unwrap().contains("EPD-TEST-0001"));
    run(&mut app, "edit.undo", json!({})).unwrap();
    assert!(m(&mut app)["values"].as_object().unwrap().is_empty(), "one undo step takes the values back");
    let carbon = run(&mut app, "embodied_carbon.materials", json!({})).unwrap();
    assert_eq!(carbon.as_array().unwrap().iter().find(|r| r["material"] == material.as_str()).unwrap()["status"], "unmatched");
    run(&mut app, "edit.redo", json!({})).unwrap();

    // Asked again: from the cache. The HPD's values, some fields not found.
    let again = run(&mut app, "docs.extract", json!({ "material": material, "set": "epd" })).unwrap();
    assert_eq!(again["documents"][0]["cached"], true);
    let h = run(&mut app, "docs.extract", json!({ "material": material, "set": "hpd" })).unwrap();
    assert_eq!(field(&h, "highest_concern")["found"], false);
    assert_eq!(run(&mut app, "docs.apply_extracted", json!({ "run_id": h["run_id"], "fields": ["highest_concern"] })).unwrap_err().code, "docs.not_found");
    assert_eq!(run(&mut app, "docs.apply_extracted", json!({ "run_id": h["run_id"], "carbon": true })).unwrap_err().code, "docs.invalid_input");
    let applied = run(&mut app, "docs.apply_extracted", json!({ "run_id": h["run_id"] })).unwrap();
    assert_eq!(applied["written"].as_array().unwrap().len(), 5);

    // Two documents that disagree: the user names the one to take.
    let both = run(&mut app, "docs.extract", json!({ "connection_id": conn, "files": [epd, _sheet], "set": "epd" })).unwrap();
    assert_eq!(run(&mut app, "docs.apply_extracted", json!({ "run_id": both["run_id"], "material": material, "fields": ["gwp_a1a3"] })).unwrap_err().code, "docs.ambiguous");
    run(&mut app, "docs.apply_extracted", json!({ "run_id": both["run_id"], "material": material, "fields": ["gwp_a1a3"], "file_uid": epd })).unwrap();

    // An agent may extract and apply, acting as the user.
    let mcp = Caller { origin: Origin::mcp("probe", "s1"), ..Caller::user() };
    let r = app.execute("docs.extract", json!({ "material": material, "set": "epd" }), &mcp).unwrap().result;
    app.execute("docs.apply_extracted", json!({ "run_id": r["run_id"], "fields": ["epd_number"] }), &mcp).unwrap();

    // In IFC: the folder as a document reference on the material, the values as material properties.
    run(&mut app, "file.save_as", json!({ "path": path })).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("IFCDOCUMENTREFERENCE(") && text.contains(&format!("'{folder}'")), "the folder is referenced");
    assert!(text.contains("IFCEXTERNALREFERENCERELATIONSHIP("));
    assert!(text.contains("IFCMATERIALPROPERTIES('Reviera_EPD'") && text.contains("IFCMATERIALPROPERTIES('Reviera_HPD'"));
    assert!(text.contains("IFCPROPERTYSINGLEVALUE('GWP_A1A3','rxd:product_docs kg CO2 eq',IFCREAL(126.)"), "{}", text.lines().filter(|l| l.contains("GWP")).collect::<Vec<_>>().join("\n"));
    assert!(text.contains("IFCLABEL('2030-12-15')"));
    // Saved again unchanged: the same entities, not twice as many.
    let count = |t: &str| t.matches("IFCMATERIALPROPERTIES(").count();
    add_wall(&mut app);
    run(&mut app, "file.save", json!({})).unwrap();
    assert_eq!(count(&std::fs::read_to_string(&path).unwrap()), 2);
    // Reopened: the link and values are kept.
    let id = app.active().unwrap();
    run(&mut app, "file.close", json!({ "document_id": id, "save": "no" })).unwrap();
    run(&mut app, "file.open", json!({ "path": path })).unwrap();
    let v = m(&mut app);
    assert_eq!(v["link"]["folder_uid"], folder.as_str());
    assert_eq!(v["values"]["hpd"]["valid_until"]["value"], "2027-10-17");

    // The report: an expired EPD on another material is flagged; published to the folder.
    let old = json!({ "value": "2020-01-01", "label": "Valid until", "property": "ValidUntil", "source": { "file_uid": "x", "file_name": "old epd.pdf", "version": "20200101_000000.000" } });
    let num = json!({ "value": "EPD-OLD-1", "label": "EPD number", "property": "DeclarationNumber", "source": { "file_uid": "x", "file_name": "old epd.pdf", "version": "20200101_000000.000" } });
    run(&mut app, "docs.set_values", json!({ "material": "Old board", "set": "epd", "values": { "valid_until": old, "epd_number": num } })).unwrap();
    let report_path = dir.path().join("report.html");
    let rep = run(&mut app, "docs.report", json!({ "path": report_path })).unwrap();
    let html = std::fs::read_to_string(&report_path).unwrap();
    assert!(html.contains("EPD-TEST-0001") && html.contains("123456789") && html.contains("epd.txt"), "the values and their documents");
    let warnings: Vec<&str> = rep["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(warnings.iter().any(|w| w.starts_with("Old board: the EPD EPD-OLD-1 expired")), "{warnings:?}");
    assert!(html.contains("To check") && html.contains("expired"));
    let eng = fake.uid_of("Engineering").unwrap();
    let published = run(&mut app, "fileengine.publish", json!({ "connection_id": conn, "folder_uid": eng, "path": report_path, "name": "board documentation.html", "metadata": { "doc.type": "report" } })).unwrap();
    assert_eq!((published["name"].as_str(), published["replaced"].as_bool()), (Some("board documentation.html"), Some(false)));
    let uid = published["uid"].as_str().unwrap().to_owned();
    assert_eq!(fake.content(&uid).unwrap(), html.as_bytes());
    let meta = fake.metadata(&uid);
    assert_eq!(meta["doc.type"], "report");
    assert_eq!(meta["ifc.project_guid"].len(), 22);
    assert_eq!(run(&mut app, "fileengine.publish", json!({ "connection_id": conn, "folder_uid": eng, "path": report_path, "name": "board documentation.html" })).unwrap_err().code, "fileengine.exists");
    let again = run(&mut app, "fileengine.publish", json!({ "connection_id": conn, "folder_uid": eng, "path": report_path, "name": "board documentation.html", "replace": true })).unwrap();
    assert_eq!((again["uid"].as_str(), again["replaced"].as_bool()), (Some(uid.as_str()), Some(true)));
    assert_ne!(again["version"], published["version"]);

    // Unlinking keeps the values unless asked; usage reads.
    run(&mut app, "docs.unlink", json!({ "material": material })).unwrap();
    assert!(m(&mut app)["link"].is_null());
    assert!(!m(&mut app)["values"].as_object().unwrap().is_empty());
    assert_eq!(run(&mut app, "docs.usage", json!({ "connection_id": conn })).unwrap()["state"], "ok");
}

/// Against a real FileEngine with the conversion service: a product folder holding an EPD and an
/// HPD of a real product (doc.type set), e.g. `Engineering/Reviera test/Product Library/<product>`.
/// Read-only on the server: it extracts and applies into a model that is never uploaded.
#[test]
fn a_real_product_folder() {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let (Some(url), Some(key), Some(secret), Some(folder)) = (var("REVIERA_FILEENGINE_URL"), var("REVIERA_FILEENGINE_KEY"), var("REVIERA_FILEENGINE_SECRET"), var("REVIERA_FILEENGINE_PRODUCT_FOLDER")) else {
        eprintln!("REVIERA_FILEENGINE_URL, _KEY, _SECRET and _PRODUCT_FOLDER are not all set: skipped");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app(dir.path());
    let conn = run(&mut app, "fileengine.connect", json!({ "url": url })).unwrap()["id"].as_str().unwrap().to_owned();
    run(&mut app, "fileengine.sign_in", json!({ "connection_id": conn, "key": key, "secret": secret })).unwrap();
    let f = run(&mut app, "fileengine.browse", json!({ "connection_id": conn, "path": folder })).unwrap();
    run(&mut app, "file.new", json!({})).unwrap();
    add_wall(&mut app);
    let material = "Gypsum board, Type X 5/8 in";
    run(&mut app, "docs.link", json!({ "material": material, "connection_id": conn, "folder_uid": f["folder_uid"] })).unwrap();
    for set in ["epd", "hpd"] {
        let r = run(&mut app, "docs.extract", json!({ "material": material, "set": set })).unwrap();
        eprintln!("{set}: {}", serde_json::to_string_pretty(&r).unwrap());
        assert!(!r["documents"].as_array().unwrap().is_empty(), "{set}: no document read: {r}");
        let found = r["documents"][0]["fields"].as_array().unwrap().iter().filter(|v| v["found"] == true).count();
        assert!(found >= 3, "{set}: only {found} values found");
        let applied = run(&mut app, "docs.apply_extracted", json!({ "run_id": r["run_id"], "carbon": set == "epd" })).unwrap();
        eprintln!("applied: {applied}");
    }
    let path = dir.path().join("documented.ifc");
    run(&mut app, "file.save_as", json!({ "path": path })).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("IFCMATERIALPROPERTIES('Reviera_EPD'"));
    eprintln!("{}", text.lines().filter(|l| l.contains("rxd:product_docs") || l.contains("IFCMATERIALPROPERTIES")).collect::<Vec<_>>().join("\n"));
}
