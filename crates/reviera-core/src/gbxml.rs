// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! gbXML exchange (ANA-EM-004, ANA-ENV-009). [`export`] writes the energy model (ANA-EM-003) as
//! gbXML 8.01: the campus with its location, the building with its storeys and spaces (area,
//! volume, people, lighting and equipment from the space types), one surface per wall, floor,
//! ceiling or roof face with its adjacent spaces (shared interior surfaces once, with both),
//! windows and doors as openings of their walls, and the U-values as constructions and window
//! types (valid against the 8.01 schema; see [`VERSION`]). Space ids are `sp-` and the space's GlobalId with `$` as `-`, so they map back.
//! [`read_loads`] reads the results a third-party tool wrote into a gbXML file: each space's
//! design heating and cooling loads and airflows, matched by id, then by name and number.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use quick_xml::Reader;
use quick_xml::events::Event;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::energy_model::{self, SurfaceClass};
use crate::error::{Error, Result};
use crate::id::ElementId;
use crate::store::Store;
use crate::txn::Transaction;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The schema written to (gbXML 8.01) and the `version` attribute: the 8.01 schema's
/// `versionEnum` still ends at 6.01, so files that declare 8.01 fail validation against it.
pub const SCHEMA: &str = "8.01";
pub const VERSION: &str = "6.01";
pub const IMPORT_KEY: &str = "rxd:gbxml_import";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A space's gbXML id from its GlobalId.
pub fn space_id(guid: &str) -> String {
    format!("sp-{}", guid.replace('$', "-"))
}

/// The GlobalId a gbXML space id was made from, if it is one of Reviera's.
pub fn guid_of(id: &str) -> Option<String> {
    id.strip_prefix("sp-").map(|g| g.replace('-', "$"))
}

fn loop_xml(out: &mut String, indent: &str, pts: &[[f64; 3]]) {
    let _ = writeln!(out, "{indent}<PlanarGeometry>\n{indent}  <PolyLoop>");
    for p in pts {
        let _ = writeln!(out, "{indent}    <CartesianPoint><Coordinate>{:.4}</Coordinate><Coordinate>{:.4}</Coordinate><Coordinate>{:.4}</Coordinate></CartesianPoint>", p[0], p[1], p[2]);
    }
    let _ = writeln!(out, "{indent}  </PolyLoop>\n{indent}</PlanarGeometry>");
}

/// What an export wrote.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ExportReport {
    pub spaces: usize,
    pub surfaces: usize,
    pub openings: usize,
    /// Spaces left out (unplaced, unenclosed) and surfaces on default U-values.
    pub warnings: Vec<String>,
}

/// The gbXML document for the model's energy model.
pub fn export(store: &Store) -> Result<(String, ExportReport)> {
    let model = energy_model::build(store);
    if model.spaces.is_empty() {
        return Err(bad("gbxml.no_spaces", "the energy model has no spaces: place enclosed MEP spaces first"));
    }
    let guid = |id: ElementId| store.get(id).map(|e| e.guid().to_string()).unwrap_or_else(|| format!("e{}", id.0));
    let mut report = ExportReport { spaces: model.spaces.len(), ..Default::default() };
    for (id, why) in &model.skipped {
        report.warnings.push(format!("space {} left out: {why}", id.0));
    }
    let loc = crate::location::get(store);
    let mut x = String::new();
    let _ = writeln!(x, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- gbXML {SCHEMA} schema, written by BIM Reviera {} -->", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(x, "<gbXML xmlns=\"http://www.gbxml.org/schema\" version=\"{VERSION}\" temperatureUnit=\"C\" lengthUnit=\"Meters\" areaUnit=\"SquareMeters\" volumeUnit=\"CubicMeters\" useSIUnitsForResults=\"true\">");
    let _ = writeln!(x, "  <Campus id=\"campus-1\">");
    let _ = writeln!(x, "    <Name>{}</Name>", esc(store.iter().find(|e| e.class == "IfcProject").and_then(|p| p.name.clone()).as_deref().unwrap_or("Project")));
    if let (Some(lat), Some(lon)) = (loc.latitude, loc.longitude) {
        let _ = writeln!(x, "    <Location>\n      <Name>{}</Name>\n      <Latitude>{lat:.6}</Latitude>\n      <Longitude>{lon:.6}</Longitude>\n      <Elevation>{:.1}</Elevation>\n    </Location>", esc(loc.place.as_deref().unwrap_or("Site")), loc.elevation);
    }
    let area: f64 = model.spaces.iter().map(|s| s.area).sum();
    let _ = writeln!(x, "    <Building id=\"building-1\" buildingType=\"Unknown\">\n      <Name>Building</Name>\n      <Area>{area:.3}</Area>");
    let levels: BTreeSet<ElementId> = model.spaces.iter().filter_map(|s| s.level).collect();
    for l in crate::levels::list(store).into_iter().filter(|l| levels.contains(&l.id)) {
        let _ = writeln!(x, "      <BuildingStorey id=\"st-{}\">\n        <Name>{}</Name>\n        <Level>{:.3}</Level>\n      </BuildingStorey>", l.guid.replace('$', "-"), esc(&l.name), l.elevation);
    }
    for s in &model.spaces {
        let v = crate::mep_spaces::info(store, s.space).map(|i| i.values).unwrap_or_default();
        let people = if v.area_per_person > 0.0 { s.area / v.area_per_person } else { 0.0 };
        let storey = s.level.map(|l| format!(" buildingStoreyIdRef=\"st-{}\"", guid(l).replace('$', "-"))).unwrap_or_default();
        let name = format!("{} {}", s.number, s.name).trim().to_owned();
        let _ = writeln!(x, "      <Space id=\"{}\"{storey}>", space_id(&guid(s.space)));
        let _ = writeln!(x, "        <Name>{}</Name>", esc(&name));
        let _ = writeln!(x, "        <PeopleNumber unit=\"NumberOfPeople\">{people:.2}</PeopleNumber>");
        let _ = writeln!(x, "        <LightPowerPerArea unit=\"WattPerSquareMeter\">{:.3}</LightPowerPerArea>", v.lighting_load_density);
        let _ = writeln!(x, "        <EquipPowerPerArea unit=\"WattPerSquareMeter\">{:.3}</EquipPowerPerArea>", v.power_load_density);
        let _ = writeln!(x, "        <Area>{:.3}</Area>\n        <Volume>{:.3}</Volume>", s.area, s.volume);
        let _ = writeln!(x, "      </Space>");
    }
    let _ = writeln!(x, "    </Building>");
    // Constructions and window types by U-value (and SHGC).
    let mut constructions: BTreeMap<String, f64> = BTreeMap::new();
    let mut window_types: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    let mut seen: BTreeSet<(String, Option<ElementId>, ElementId, Option<ElementId>)> = BTreeSet::new();
    let (mut n_surf, mut n_open) = (0usize, 0usize);
    let mut defaults = 0usize;
    for s in &model.spaces {
        for f in s.surfaces.iter().filter(|f| !matches!(f.class, SurfaceClass::Window | SurfaceClass::Door)) {
            // A shared surface once: keyed by its element and its two spaces.
            if let Some(adj) = f.adjacent {
                let (a, b) = if s.space <= adj { (s.space, adj) } else { (adj, s.space) };
                let kind = if matches!(f.class, SurfaceClass::InteriorFloor | SurfaceClass::InteriorCeiling) { "h" } else { "v" };
                if !seen.insert((kind.into(), f.element, a, Some(b))) {
                    continue;
                }
            }
            let ty = match f.class {
                SurfaceClass::ExteriorWall => "ExteriorWall",
                SurfaceClass::InteriorWall => "InteriorWall",
                SurfaceClass::Roof => "Roof",
                SurfaceClass::InteriorCeiling => "Ceiling",
                SurfaceClass::SlabOnGrade => "SlabOnGrade",
                SurfaceClass::InteriorFloor => "InteriorFloor",
                SurfaceClass::Air => "Air",
                _ => continue,
            };
            n_surf += 1;
            if !f.u_from_type && f.class != SurfaceClass::Air {
                defaults += 1;
            }
            let cons = format!("co-{}-{:.3}", ty.to_ascii_lowercase(), f.u).replace('.', "_");
            if f.class != SurfaceClass::Air {
                constructions.insert(cons.clone(), f.u);
            }
            let cref = if f.class == SurfaceClass::Air { String::new() } else { format!(" constructionIdRef=\"{cons}\"") };
            let _ = writeln!(x, "    <Surface id=\"su-{n_surf}\" surfaceType=\"{ty}\"{cref}>");
            let _ = writeln!(x, "      <Name>{ty} {n_surf}</Name>");
            let _ = writeln!(x, "      <AdjacentSpaceId spaceIdRef=\"{}\"/>", space_id(&guid(s.space)));
            if let Some(adj) = f.adjacent {
                let _ = writeln!(x, "      <AdjacentSpaceId spaceIdRef=\"{}\"/>", space_id(&guid(adj)));
            }
            loop_xml(&mut x, "      ", &f.outline);
            // The wall's windows and doors: those of the space in its plane.
            if f.class == SurfaceClass::ExteriorWall {
                for o in s.surfaces.iter().filter(|o| matches!(o.class, SurfaceClass::Window | SurfaceClass::Door) && o.normal == f.normal && o.outline.first().is_some_and(|p| on_plane(&f.outline, f.normal, *p))) {
                    n_open += 1;
                    let (kind, tref) = if o.class == SurfaceClass::Window {
                        let shgc = o.shgc.unwrap_or(energy_model::DEFAULT_SHGC);
                        let wt = format!("wt-{:.3}-{:.3}", o.u, shgc).replace('.', "_");
                        window_types.insert(wt.clone(), (o.u, shgc));
                        ("FixedWindow", format!(" windowTypeIdRef=\"{wt}\""))
                    } else {
                        let c = format!("co-door-{:.3}", o.u).replace('.', "_");
                        constructions.insert(c.clone(), o.u);
                        ("NonSlidingDoor", format!(" constructionIdRef=\"{c}\""))
                    };
                    if !o.u_from_type {
                        defaults += 1;
                    }
                    let _ = writeln!(x, "      <Opening id=\"op-{n_open}\" openingType=\"{kind}\"{tref}>");
                    let _ = writeln!(x, "        <Name>{} {n_open}</Name>", if kind == "FixedWindow" { "Window" } else { "Door" });
                    loop_xml(&mut x, "        ", &o.outline);
                    let _ = writeln!(x, "      </Opening>");
                }
            }
            let _ = writeln!(x, "    </Surface>");
        }
    }
    let _ = writeln!(x, "  </Campus>");
    for (id, u) in &constructions {
        let _ = writeln!(x, "  <Construction id=\"{id}\">\n    <U-value unit=\"WPerSquareMeterK\">{u:.4}</U-value>\n    <Name>U {u:.3}</Name>\n  </Construction>");
    }
    for (id, (u, g)) in &window_types {
        let _ = writeln!(x, "  <WindowType id=\"{id}\">\n    <Name>Glazing U {u:.3} g {g:.2}</Name>\n    <U-value unit=\"WPerSquareMeterK\">{u:.4}</U-value>\n    <SolarHeatGainCoeff unit=\"Fraction\">{g:.3}</SolarHeatGainCoeff>\n  </WindowType>");
    }
    let _ = writeln!(x, "</gbXML>");
    report.surfaces = n_surf;
    report.openings = n_open;
    if defaults > 0 {
        report.warnings.push(format!("{defaults} surfaces or openings use the class's default U-value (no thermal values on their types)"));
    }
    Ok((x, report))
}

/// Whether a point lies in a vertical surface's plane.
pub(crate) fn on_plane(outline: &[[f64; 3]], n: [f64; 3], p: [f64; 3]) -> bool {
    let Some(o) = outline.first() else { return false };
    ((p[0] - o[0]) * n[0] + (p[1] - o[1]) * n[1] + (p[2] - o[2]) * n[2]).abs() < 1e-3
}

/// A space's loads as read from a file (W and L/s).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpaceLoads {
    pub id: String,
    pub name: String,
    pub heating: Option<f64>,
    pub cooling: Option<f64>,
    pub cooling_sensible: Option<f64>,
    pub heating_airflow: Option<f64>,
    pub cooling_airflow: Option<f64>,
}

fn to_watts(v: f64, unit: &str) -> f64 {
    match unit.to_ascii_lowercase().as_str() {
        "kilowatt" | "kw" => v * 1000.0,
        "btuperhour" | "btuh" => v * 0.293_071,
        "kbtuperhour" => v * 293.071,
        "ton" | "tons" => v * 3516.85,
        _ => v,
    }
}

fn to_ls(v: f64, unit: &str) -> f64 {
    match unit.to_ascii_lowercase().as_str() {
        "cubicmeterspersecond" | "cubicmeterpersecond" => v * 1000.0,
        "cubicmetersperhour" | "cubicmeterperhour" => v / 3.6,
        "cfm" | "cubicfeetperminute" => v * 0.471_947,
        _ => v,
    }
}

/// Reads the spaces and their result values from a gbXML document: `Results` elements (by
/// `objectIdRef` or an `ObjectId` child naming a space; `resultsType` HeatLoad, CoolingLoad,
/// SensibleCoolingLoad, HeatingAirflow or CoolingAirflow, or names containing them; units
/// converted from W, kW, Btu/h, kBtu/h and tons; m³/s, m³/h and cfm). Geometry is ignored.
pub fn read_loads(text: &str) -> Result<Vec<SpaceLoads>> {
    let mut r = Reader::from_str(text);
    r.config_mut().trim_text(true);
    let mut spaces: Vec<SpaceLoads> = Vec::new();
    let mut by_id: BTreeMap<String, usize> = BTreeMap::new();
    // The open elements' names, the current element's text and the current result.
    let mut path: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut result: Option<(String, String, Option<String>, Option<f64>)> = None;
    let mut saw_root = false;
    loop {
        let ev = r.read_event().map_err(|e| bad("gbxml.parse", format!("at byte {}: {e}", r.buffer_position())))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = e.local_name().as_ref().to_owned();
                let attr = |k: &str| e.attributes().flatten().find(|a| a.key.local_name().as_ref() == k).and_then(|a| a.normalized_value(quick_xml::XmlVersion::default()).ok().map(|v| v.into_owned()));
                if name == "gbXML" {
                    saw_root = true;
                }
                if name == "Space" && path.last().is_some_and(|p| p == "Building") {
                    let id = attr("id").unwrap_or_default();
                    by_id.insert(id.clone(), spaces.len());
                    spaces.push(SpaceLoads { id, ..Default::default() });
                }
                if name == "Results" {
                    result = Some((attr("resultsType").unwrap_or_default(), attr("unit").unwrap_or_default(), attr("objectIdRef"), None));
                }
                buf.clear();
                if matches!(ev, Event::Start(_)) {
                    path.push(name);
                }
            }
            Event::Text(t) => buf.push_str(&t.xml10_content()),
            Event::GeneralRef(g) => {
                let c = match &*g {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    n => n.strip_prefix("#x").and_then(|h| u32::from_str_radix(h, 16).ok()).or_else(|| n.strip_prefix('#').and_then(|d| d.parse().ok())).and_then(char::from_u32),
                };
                buf.extend(c);
            }
            Event::End(e) => {
                let name = e.local_name().as_ref().to_owned();
                path.pop();
                let text = buf.trim().to_owned();
                buf.clear();
                match name.as_str() {
                    "Name" if path.last().is_some_and(|p| p == "Space") => {
                        if let Some(sp) = spaces.last_mut() {
                            sp.name = text;
                        }
                    }
                    "ObjectId" => {
                        if let Some(r) = result.as_mut() {
                            r.2 = Some(text);
                        }
                    }
                    "Value" => {
                        if let Some(r) = result.as_mut() {
                            r.3 = text.parse().ok();
                        }
                    }
                    "Results" => {
                        if let Some((ty, unit, Some(obj), Some(v))) = result.take()
                            && let Some(&k) = by_id.get(&obj)
                        {
                            let t = ty.to_ascii_lowercase();
                            let sp = &mut spaces[k];
                            if t.contains("airflow") {
                                let q = to_ls(v, &unit);
                                if t.contains("heat") {
                                    sp.heating_airflow = Some(q);
                                } else if t.contains("cool") {
                                    sp.cooling_airflow = Some(q);
                                }
                            } else if t.contains("sensible") && t.contains("cool") {
                                sp.cooling_sensible = Some(to_watts(v, &unit));
                            } else if t.contains("heat") && t.contains("load") {
                                sp.heating = Some(to_watts(v, &unit));
                            } else if t.contains("cool") && t.contains("load") {
                                sp.cooling = Some(to_watts(v, &unit));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_root {
        return Err(bad("gbxml.not_gbxml", "the file has no gbXML root element"));
    }
    Ok(spaces)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ImportReport {
    pub file: String,
    /// Spaces updated: (space element, the file's id).
    pub matched: Vec<(ElementId, String)>,
    /// The file's space ids that match nothing.
    pub unmatched: Vec<String>,
    /// Matched spaces with no result values in the file.
    pub without_results: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyParams {
    /// The file it came from (for the record).
    pub file: String,
    pub spaces: Vec<SpaceLoads>,
    #[serde(default)]
    pub at: i64,
}

/// `gbxml.apply_loads`: writes read loads onto the matching spaces (by id, then by name and
/// number) as `Pset_SpaceThermalDesign` values and keeps the import's record.
pub(crate) fn apply(txn: &mut Transaction<'_>, p: ApplyParams) -> Result<ImportReport> {
    let store = txn.store().clone();
    let all: Vec<(ElementId, String, String)> = crate::mep_spaces::all(&store)
        .into_iter()
        .filter_map(|(id, _, _)| {
            let e = store.get(id)?;
            let (n, m) = crate::rooms::number_and_name(e);
            Some((id, e.guid().to_string(), format!("{n} {m}").trim().to_owned()))
        })
        .collect();
    let mut report = ImportReport { file: p.file.clone(), ..Default::default() };
    for s in &p.spaces {
        let by_id = guid_of(&s.id).and_then(|g| all.iter().find(|x| x.1 == g));
        let hit = by_id.or_else(|| (!s.name.is_empty()).then(|| all.iter().find(|x| x.2 == s.name)).flatten());
        let Some((id, _, _)) = hit else {
            report.unmatched.push(s.id.clone());
            continue;
        };
        let ty = |t: &str, v: f64| json!({ "type": t, "value": v });
        let mut vals = Vec::new();
        if let Some(v) = s.heating {
            vals.push(("Pset_SpaceThermalDesign.TotalHeatLoss", ty("IfcPowerMeasure", v)));
        }
        if let Some(v) = s.cooling {
            vals.push(("Pset_SpaceThermalDesign.TotalHeatGain", ty("IfcPowerMeasure", v)));
        }
        if let Some(v) = s.cooling_sensible {
            vals.push(("Pset_SpaceThermalDesign.TotalSensibleHeatGain", ty("IfcPowerMeasure", v)));
        }
        if let Some(v) = s.heating_airflow {
            vals.push(("Pset_SpaceThermalDesign.HeatingDesignAirflow", ty("IfcVolumetricFlowRateMeasure", v / 1000.0)));
        }
        if let Some(v) = s.cooling_airflow {
            vals.push(("Pset_SpaceThermalDesign.CoolingDesignAirflow", ty("IfcVolumetricFlowRateMeasure", v / 1000.0)));
        }
        if vals.is_empty() {
            report.without_results.push(s.id.clone());
            continue;
        }
        txn.modify(*id, |e| {
            for (k, v) in vals {
                e.params.insert(k.into(), v);
            }
        })?;
        report.matched.push((*id, s.id.clone()));
    }
    let pid = store.iter().find(|e| e.class == "IfcProject").map(crate::element::Element::id).ok_or_else(|| bad("project.missing", "the model has no project"))?;
    let rec = json!({ "file": p.file, "at": p.at, "matched": report.matched.len(), "unmatched": report.unmatched });
    txn.modify(pid, |e| {
        e.params.insert(IMPORT_KEY.into(), rec);
    })?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        let g = "0BIlCFh7LDgfykf3K$1bjD";
        assert_eq!(space_id(g), "sp-0BIlCFh7LDgfykf3K-1bjD");
        assert_eq!(guid_of(&space_id(g)).as_deref(), Some(g));
        assert_eq!(crate::id::GlobalId::parse(g).map(|x| x.to_string()).as_deref(), Some(g));
    }

    #[test]
    fn reads_results_in_units() {
        let text = r#"<?xml version="1.0"?>
<gbXML xmlns="http://www.gbxml.org/schema" version="8.01">
  <Campus id="c"><Building id="b" buildingType="Office">
    <Space id="sp-A"><Name>101 Office</Name></Space>
    <Space id="sp-B"><Name>102 Store</Name></Space>
  </Building></Campus>
  <Results id="r1" objectIdRef="sp-A" resultsType="HeatLoad" unit="Kilowatt"><Value>2.5</Value></Results>
  <Results id="r2" resultsType="CoolingLoad" unit="BtuPerHour"><ObjectId>sp-A</ObjectId><Value>10000</Value></Results>
  <Results id="r3" objectIdRef="sp-A" resultsType="CoolingAirflow" unit="CFM"><Value>100</Value></Results>
  <Results id="r4" objectIdRef="sp-X" resultsType="HeatLoad" unit="Watt"><Value>1</Value></Results>
</gbXML>"#;
        let s = read_loads(text).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "101 Office");
        assert_eq!(s[0].heating, Some(2500.0));
        assert!((s[0].cooling.unwrap() - 2930.71).abs() < 0.01);
        assert!((s[0].cooling_airflow.unwrap() - 47.1947).abs() < 1e-3);
        assert_eq!(s[1].heating, None);
        assert!(read_loads("<html/>").is_err());
    }
}
