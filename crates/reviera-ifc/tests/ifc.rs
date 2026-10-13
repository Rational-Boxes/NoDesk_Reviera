// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! IfcOpenShell integration: create, write, reopen, edit and tessellate IFC models, and round-trip
//! a third-party file without loss (spec DM-CORE-001, DM-ID-001).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use reviera_core::GlobalId;
use reviera_ifc::{AttrValue, IfcModel, Schema, StepId};

fn r(id: StepId) -> AttrValue {
    AttrValue::Ref(id.0)
}

fn s(v: &str) -> AttrValue {
    AttrValue::String(v.into())
}

fn e(v: &str) -> AttrValue {
    AttrValue::Enum(v.into())
}

fn reals(v: &[f64]) -> AttrValue {
    AttrValue::List(v.iter().copied().map(AttrValue::Real).collect())
}

fn create(m: &mut IfcModel, class: &str, attrs: &[(&str, AttrValue)]) -> StepId {
    let id = m.create(class).unwrap();
    for (name, value) in attrs {
        m.set_attribute(id, name, value)
            .unwrap_or_else(|err| panic!("{class}.{name}: {err}"));
    }
    id
}

struct WallModel {
    model: IfcModel,
    project: GlobalId,
    wall: GlobalId,
}

/// IFC4 model with one 5 m × 0.2 m × 3 m wall, as an extruded rectangle.
fn wall_model() -> WallModel {
    let mut m = IfcModel::new(Schema::Ifc4).unwrap();
    let unit = create(
        &mut m,
        "IfcSIUnit",
        &[("UnitType", e("LENGTHUNIT")), ("Name", e("METRE"))],
    );
    let units = create(
        &mut m,
        "IfcUnitAssignment",
        &[("Units", AttrValue::List(vec![r(unit)]))],
    );
    let origin = create(
        &mut m,
        "IfcCartesianPoint",
        &[("Coordinates", reals(&[0.0, 0.0, 0.0]))],
    );
    let axes = create(&mut m, "IfcAxis2Placement3D", &[("Location", r(origin))]);
    let ctx = create(
        &mut m,
        "IfcGeometricRepresentationContext",
        &[
            ("ContextType", s("Model")),
            ("CoordinateSpaceDimension", AttrValue::Int(3)),
            ("Precision", AttrValue::Real(1e-5)),
            ("WorldCoordinateSystem", r(axes)),
        ],
    );
    let (project, wall) = (GlobalId::new(), GlobalId::new());
    let p = m.create_rooted("IfcProject", &project).unwrap();
    m.set_attribute(p, "Name", &s("Test project")).unwrap();
    m.set_attribute(p, "RepresentationContexts", &AttrValue::List(vec![r(ctx)]))
        .unwrap();
    m.set_attribute(p, "UnitsInContext", &r(units)).unwrap();

    let up = create(
        &mut m,
        "IfcDirection",
        &[("DirectionRatios", reals(&[0.0, 0.0, 1.0]))],
    );
    let profile = create(
        &mut m,
        "IfcRectangleProfileDef",
        &[
            ("ProfileType", e("AREA")),
            ("XDim", AttrValue::Real(5.0)),
            ("YDim", AttrValue::Real(0.2)),
        ],
    );
    let solid = create(
        &mut m,
        "IfcExtrudedAreaSolid",
        &[
            ("SweptArea", r(profile)),
            ("ExtrudedDirection", r(up)),
            ("Depth", AttrValue::Real(3.0)),
        ],
    );
    let body = create(
        &mut m,
        "IfcShapeRepresentation",
        &[
            ("ContextOfItems", r(ctx)),
            ("RepresentationIdentifier", s("Body")),
            ("RepresentationType", s("SweptSolid")),
            ("Items", AttrValue::List(vec![r(solid)])),
        ],
    );
    let shape = create(
        &mut m,
        "IfcProductDefinitionShape",
        &[("Representations", AttrValue::List(vec![r(body)]))],
    );
    let placement = create(
        &mut m,
        "IfcLocalPlacement",
        &[("RelativePlacement", r(axes))],
    );
    let w = m.create_rooted("IfcWall", &wall).unwrap();
    for (name, value) in [
        ("Name", s("Wall 1")),
        ("ObjectPlacement", r(placement)),
        ("Representation", r(shape)),
        ("PredefinedType", e("STANDARD")),
    ] {
        m.set_attribute(w, name, &value).unwrap();
    }
    WallModel {
        model: m,
        project,
        wall,
    }
}

#[test]
fn reports_linked_version() {
    assert!(
        reviera_ifc::ifcopenshell_version().starts_with("0.8"),
        "{}",
        reviera_ifc::ifcopenshell_version()
    );
}

#[test]
fn created_model_survives_write_and_reopen() {
    let WallModel {
        model,
        project,
        wall,
    } = wall_model();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wall.ifc");
    model.write(&path).unwrap();

    let reopened = IfcModel::open(&path).unwrap();
    assert_eq!(reopened.schema(), "IFC4");
    assert_eq!(reopened.entity_count(), model.entity_count());
    let rooted = reopened.rooted().unwrap();
    let guids: Vec<_> = rooted.iter().filter_map(|e| e.global_id.clone()).collect();
    assert!(guids.contains(&project) && guids.contains(&wall));

    let w = reopened.find_by_guid(&wall).unwrap();
    assert_eq!(reopened.class_of(w).unwrap(), "IfcWall");
    assert_eq!(reopened.attribute(w, "Name").unwrap(), s("Wall 1"));
    assert_eq!(
        reopened.attribute(w, "PredefinedType").unwrap(),
        e("STANDARD")
    );
    assert_eq!(reopened.attribute(w, "Tag").unwrap(), AttrValue::Null);
}

#[test]
fn wall_tessellates_to_a_box() {
    let WallModel { model, wall, .. } = wall_model();
    let meshes = model.tessellate(2).unwrap();
    let mesh = meshes
        .iter()
        .find(|m| m.global_id.as_ref() == Some(&wall))
        .expect("wall mesh");
    assert_eq!(mesh.class, "IfcWall");
    assert_eq!(mesh.triangle_count(), 12);
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for v in mesh.verts.chunks(3) {
        for i in 0..3 {
            lo[i] = lo[i].min(v[i]);
            hi[i] = hi[i].max(v[i]);
        }
    }
    let size: Vec<_> = (0..3).map(|i| hi[i] - lo[i]).collect();
    for (got, want) in size.iter().zip([5.0, 0.2, 3.0]) {
        assert!((got - want).abs() < 1e-6, "size {size:?}");
    }
}

#[test]
fn edits_are_visible_and_entities_can_be_removed() {
    let WallModel {
        mut model, wall, ..
    } = wall_model();
    let w = model.find_by_guid(&wall).unwrap();
    model.set_attribute(w, "Name", &s("Renamed")).unwrap();
    model
        .set_attribute(w, "Description", &s("Line 1\n\"quoted\" \\ é"))
        .unwrap();
    assert_eq!(model.attribute(w, "Name").unwrap(), s("Renamed"));
    assert_eq!(
        model.attribute(w, "Description").unwrap(),
        s("Line 1\n\"quoted\" \\ é")
    );
    model.set_attribute(w, "Name", &AttrValue::Null).unwrap();
    assert_eq!(model.attribute(w, "Name").unwrap(), AttrValue::Null);

    let before = model.entity_count();
    model.remove(w).unwrap();
    assert_eq!(model.entity_count(), before - 1);
    assert_eq!(model.find_by_guid(&wall), None);
}

#[test]
fn errors_are_reported_not_thrown() {
    assert!(IfcModel::open(Path::new("/nonexistent/model.ifc")).is_err());
    let WallModel {
        mut model, wall, ..
    } = wall_model();
    let w = model.find_by_guid(&wall).unwrap();
    assert!(model.set_attribute(w, "NoSuchAttribute", &s("x")).is_err());
    assert!(
        model
            .set_attribute(w, "PredefinedType", &e("NOT_A_VALUE"))
            .is_err()
    );
    assert!(model.create("IfcNotAClass").is_err());
    assert!(model.attributes(StepId(999_999)).is_err());
}

/// A third-party IFC2X3 file from the IfcOpenShell sources, present after tools/build-ifcopenshell.sh.
fn sample_file() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../third_party/IfcOpenShell/src/examples/IfcParseExamples_test.ifc");
    p.exists().then_some(p)
}

fn approx_eq(a: &AttrValue, b: &AttrValue) -> bool {
    match (a, b) {
        (AttrValue::Real(x), AttrValue::Real(y)) => {
            (x - y).abs() <= 1e-12 * x.abs().max(y.abs()).max(1.0)
        }
        (AttrValue::List(x), AttrValue::List(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| approx_eq(a, b))
        }
        (
            AttrValue::Typed {
                type_name: t1,
                value: v1,
            },
            AttrValue::Typed {
                type_name: t2,
                value: v2,
            },
        ) => t1 == t2 && approx_eq(v1, v2),
        _ => a == b,
    }
}

/// Opens, writes and reopens `path`, checking that every rooted entity keeps its GlobalId, class
/// and attribute values (DM-CORE-001 acceptance). Returns the number of rooted entities.
fn assert_round_trip(path: &Path) -> usize {
    let original = IfcModel::open(path).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("roundtrip.ifc");
    original.write(&out).unwrap();
    let copy = IfcModel::open(&out).unwrap();
    assert_eq!(copy.schema(), original.schema());
    assert_eq!(copy.entity_count(), original.entity_count());

    let by_guid = |m: &IfcModel| -> BTreeMap<GlobalId, (String, Vec<(String, AttrValue)>)> {
        m.rooted()
            .unwrap()
            .into_iter()
            .filter_map(|e| Some((e.global_id?, (e.class, m.attributes(e.id).unwrap()))))
            .collect()
    };
    let (a, b) = (by_guid(&original), by_guid(&copy));
    assert!(!a.is_empty());
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (guid, (class, attrs)) in &a {
        let (class2, attrs2) = &b[guid];
        assert_eq!(class, class2);
        for ((n1, v1), (n2, v2)) in attrs.iter().zip(attrs2) {
            assert_eq!(n1, n2);
            // References are STEP numbers, which the writer keeps for an unedited file.
            assert!(approx_eq(v1, v2), "{class} {guid} {n1}: {v1:?} != {v2:?}");
        }
    }
    a.len()
}

#[test]
fn third_party_file_round_trips_without_loss() {
    let Some(path) = sample_file() else {
        eprintln!("skipped: sample IFC not present; run tools/build-ifcopenshell.sh");
        return;
    };
    assert_eq!(IfcModel::open(&path).unwrap().schema(), "IFC2X3");
    assert_round_trip(&path);
}

/// Every `.ifc` in `test-data/ifc` (or the directory named by `REVIERA_IFC_SAMPLES`) round-trips
/// without loss and tessellates.
#[test]
fn sample_models_round_trip_and_tessellate() {
    let dir = std::env::var_os("REVIERA_IFC_SAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/ifc"));
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ifc")))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .ifc files in {}", dir.display());
    for path in files {
        let rooted = assert_round_trip(&path);
        let model = IfcModel::open(&path).unwrap();
        let meshes = model.tessellate(4).unwrap();
        let triangles: usize = meshes.iter().map(|m| m.triangle_count()).sum();
        let products = model.entities_of_type("IfcProduct").unwrap().len();
        eprintln!(
            "{}: {} {} entities, {rooted} rooted, {products} products, {} meshes, {triangles} triangles",
            path.file_name().unwrap().to_string_lossy(),
            model.schema(),
            model.entity_count(),
            meshes.len()
        );
        assert!(
            !meshes.is_empty() && triangles > 0,
            "{} produced no geometry",
            path.display()
        );
        assert!(
            meshes.iter().all(|m| m.global_id.is_some()),
            "every mesh maps back to a GlobalId"
        );
    }
}

#[test]
fn typed_property_values_and_inverse_references() {
    let WallModel {
        mut model, wall, ..
    } = wall_model();
    let w = model.find_by_guid(&wall).unwrap();
    let text = AttrValue::Typed {
        type_name: "IfcText".into(),
        value: Box::new(s("{\"identity\":{\"iid\":1000}}")),
    };
    let count = AttrValue::Typed {
        type_name: "IfcInteger".into(),
        value: Box::new(AttrValue::Int(7)),
    };
    let prop = create(
        &mut model,
        "IfcPropertySingleValue",
        &[("Name", s("Data")), ("NominalValue", text.clone())],
    );
    let rev = create(
        &mut model,
        "IfcPropertySingleValue",
        &[("Name", s("Rev")), ("NominalValue", count.clone())],
    );
    let pset = model
        .create_rooted("IfcPropertySet", &GlobalId::new())
        .unwrap();
    model
        .set_attribute(pset, "Name", &s("Reviera_Element"))
        .unwrap();
    model
        .set_attribute(
            pset,
            "HasProperties",
            &AttrValue::List(vec![r(prop), r(rev)]),
        )
        .unwrap();
    let rel = model
        .create_rooted("IfcRelDefinesByProperties", &GlobalId::new())
        .unwrap();
    model
        .set_attribute(rel, "RelatedObjects", &AttrValue::List(vec![r(w)]))
        .unwrap();
    model
        .set_attribute(rel, "RelatingPropertyDefinition", &r(pset))
        .unwrap();
    assert_eq!(
        model
            .referenced_by(pset, "IfcRelDefinesByProperties")
            .unwrap(),
        [rel]
    );
    assert_eq!(
        model.referenced_by(w, "IfcRelDefinesByProperties").unwrap(),
        [rel]
    );

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pset.ifc");
    model.write(&path).unwrap();
    let reopened = IfcModel::open(&path).unwrap();
    assert_eq!(reopened.attribute(prop, "NominalValue").unwrap(), text);
    assert_eq!(reopened.attribute(rev, "NominalValue").unwrap(), count);
    assert!(
        model
            .set_attribute(
                prop,
                "NominalValue",
                &AttrValue::Typed {
                    type_name: "IfcWall".into(),
                    value: Box::new(s("x"))
                }
            )
            .is_err()
    );
}
