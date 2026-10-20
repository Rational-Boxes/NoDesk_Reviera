// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! CAM export of precast pieces (STR-PRE-007).
//!
//! Each piece becomes the neutral piece model of `reviera-precast-cam` (MIT): millimetres in its
//! production system (the origin at the lower-left corner of its front face, X along its length,
//! Y up — or across a slab —, Z into it), with its outline, openings, lifting anchors, bars by
//! set and fabric wires. A writer turns it into a file: the built-in neutral JSON and CSV writers,
//! or an external program named by a manifest (`<data folder>/cam_writers/*.json`: id, name,
//! version, SPDX licence, file extension and the command, which reads the piece as JSON on its
//! input and writes the file to its output). Files are named by the filename rule
//! (`{Var:Length:MinLength:Pad:Align}`), checked before anything is written; names that would
//! collide get a number. The export reads the model and changes nothing.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use reviera_precast_cam as cam;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::precast::{self, PieceKind};
use crate::store::Store;

type V3 = [f64; 3];

/// A map from the world to a piece's production system (mm).
type Local = Box<dyn Fn(&V3) -> V3>;

fn by_guid<'a>(store: &'a Store, guid: &str) -> Option<&'a Element> {
    GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i))
}

fn kind_id(k: PieceKind) -> &'static str {
    match k {
        PieceKind::SolidWall => "solid_wall",
        PieceKind::DoubleWall => "double_wall",
        PieceKind::SolidSlab => "solid_slab",
        PieceKind::HollowCore => "hollow_core",
        PieceKind::LatticeGirder => "lattice_girder",
    }
}

/// A piece in the neutral model.
pub fn piece_model(store: &Store, assembly: ElementId, prod_no: u32) -> Result<cam::Piece> {
    let e = store.get(assembly).ok_or_else(|| Error::new("element.not_found", format!("no element {assembly}")))?;
    let a = precast::assembly_recipe(e).ok_or_else(|| Error::new("precast.not_assembly", format!("element {assembly} is not a precast assembly")))?;
    let d = crate::precast_shop::piece(store, &a, assembly)?;
    let src = by_guid(store, &a.source).ok_or_else(|| Error::new("precast.source", "the split element is gone"))?;
    let bodies = crate::bodies::all_but_reinforcement(store);
    let pts: Vec<V3> = a.parts.iter().filter_map(|g| by_guid(store, g)).flat_map(|p| crate::parts::geometry(store, p, &bodies).into_iter().flat_map(|m| m.0)).collect();
    let wall = matches!(a.piece, PieceKind::SolidWall | PieceKind::DoubleWall);
    // The production system: X along, Y up (or across), Z in from the front face (a wall's
    // exterior, a slab's underside).
    let to_local: Local = if wall {
        let g = crate::walls::regenerate(store, src)?;
        let (dir, nrm) = (g.direction(), g.normal());
        let along = |q: &V3| (q[0] - g.start[0]) * dir[0] + (q[1] - g.start[1]) * dir[1];
        let x0 = pts.iter().map(along).fold(f64::INFINITY, f64::min);
        let z0 = pts.iter().map(|q| q[2]).fold(f64::INFINITY, f64::min);
        let (sx, sy, w) = (g.start[0], g.start[1], g.width);
        Box::new(move |q: &V3| {
            let across = (q[0] - sx) * nrm[0] + (q[1] - sy) * nrm[1];
            [((q[0] - sx) * dir[0] + (q[1] - sy) * dir[1] - x0) * 1000.0, (q[2] - z0) * 1000.0, (w / 2.0 - across) * 1000.0]
        })
    } else {
        let (lo, hi) = pts.iter().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(l, h), q| ([l[0].min(q[0]), l[1].min(q[1]), l[2].min(q[2])], [h[0].max(q[0]), h[1].max(q[1]), h[2].max(q[2])]));
        let swap = hi[1] - lo[1] > hi[0] - lo[0];
        Box::new(move |q: &V3| if swap { [(q[1] - lo[1]) * 1000.0, (q[0] - lo[0]) * 1000.0, (q[2] - lo[2]) * 1000.0] } else { [(q[0] - lo[0]) * 1000.0, (q[1] - lo[1]) * 1000.0, (q[2] - lo[2]) * 1000.0] })
    };
    let mm = |v: f64| v * 1000.0;
    let numbers = crate::rebar_numbers::numbers(store);
    let number_of = |id: ElementId| numbers.iter().filter(|(i, _)| i.element == id).map(|(_, n)| n.to_string()).collect::<Vec<_>>().join("-");
    let mut bars = Vec::new();
    let mut meshes = Vec::new();
    let mut embeds = Vec::new();
    for g in a.reinforcement.iter().chain(&a.mounting) {
        let Some(x) = by_guid(store, g) else { continue };
        if let Some(r) = crate::rebar::rebar_recipe(x) {
            let Ok(geo) = crate::rebar::geometry(store, &r) else { continue };
            let bt = crate::rebar::bar_type(store, &r.bar_type)?;
            bars.push(cam::BarSet {
                number: number_of(x.id()),
                shape: r.shape.clone(),
                bar_type: bt.name.clone(),
                diameter: mm(bt.diameter),
                bars: geo.bars.iter().map(|b| b.points.iter().map(&to_local).collect()).collect(),
            });
        } else if let Some(r) = crate::precast_fabric::recipe(x) {
            let Ok(w) = crate::precast_fabric::wires(store, &r) else { continue };
            let wire = |b: &crate::rebar::Bar, d: f64| cam::Wire { diameter: mm(d), start: to_local(&b.points[0]), end: to_local(&b.points[b.points.len() - 1]) };
            meshes.push(cam::Mesh {
                number: number_of(x.id()),
                wires: w.major.iter().map(|b| wire(b, w.major_diameter)).chain(w.minor.iter().map(|b| wire(b, w.minor_diameter))).collect(),
            });
        } else if let Some(r) = crate::precast_parts::recipe(x) {
            embeds.push(cam::Embed {
                kind: r.part.clone(),
                name: r.name.clone(),
                position: to_local(&r.point),
                direction: if wall { [0.0, -1.0, 0.0] } else { [0.0, 0.0, -1.0] },
                diameter: mm(r.diameter),
                length: mm(r.depth),
            });
        }
    }
    let (l, h) = (mm(d.length), mm(d.height));
    Ok(cam::Piece {
        schema: cam::SCHEMA,
        mark: d.mark.clone(),
        name: e.name.clone().unwrap_or_else(|| d.mark.clone()),
        prod_no,
        kind: kind_id(a.piece).into(),
        length: l,
        height: h,
        thickness: mm(d.thickness),
        volume_m3: d.volume,
        weight_kg: d.weight * 1000.0,
        outline: vec![[0.0, 0.0], [l, 0.0], [l, h], [0.0, h]],
        openings: d.openings.iter().map(|o| vec![[mm(o[0]), mm(o[2])], [mm(o[1]), mm(o[2])], [mm(o[1]), mm(o[3])], [mm(o[0]), mm(o[3])]]).collect(),
        embeds,
        bars,
        meshes,
    })
}

// ---- Writers ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WriterInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    /// SPDX licence expression.
    pub license: String,
    pub extension: String,
    /// `built_in`, or the manifest's path.
    pub source: String,
    /// An external writer's command and arguments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
}

/// The writers there are (`cam_writer.list`).
pub fn writers(store: &Store) -> Vec<WriterInfo> {
    let built = |id: &str, name: &str, ext: &str| WriterInfo {
        id: id.into(),
        name: name.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        license: "MIT".into(),
        extension: ext.into(),
        source: "built_in".into(),
        command: Vec::new(),
    };
    let mut out = vec![built("neutral_json", "Neutral piece model (JSON)", "json"), built("neutral_csv", "Neutral piece model (CSV)", "csv")];
    let folder = precast::config(store).data_folder;
    if !folder.is_empty()
        && let Ok(entries) = std::fs::read_dir(std::path::Path::new(&folder).join("cam_writers"))
    {
        let mut paths: Vec<std::path::PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        paths.sort();
        for p in paths {
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            // The manifest gives everything but its own source.
            let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            v["source"] = serde_json::json!(p.display().to_string());
            let Ok(w) = serde_json::from_value::<WriterInfo>(v) else { continue };
            if !w.command.is_empty() && !out.iter().any(|o| o.id == w.id) {
                out.push(w);
            }
        }
    }
    out
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ListParams {}

pub fn list(store: &Store, _p: ListParams) -> Result<Vec<WriterInfo>> {
    Ok(writers(store))
}

fn run_writer(w: &WriterInfo, p: &cam::Piece) -> Result<Vec<u8>> {
    match w.id.as_str() {
        "neutral_json" if w.source == "built_in" => Ok(cam::json(p).into_bytes()),
        "neutral_csv" if w.source == "built_in" => Ok(cam::csv(p).into_bytes()),
        _ => {
            use std::io::Write;
            let mut child = std::process::Command::new(&w.command[0])
                .args(&w.command[1..])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| Error::new("cam.writer", format!("writer {} did not start: {e}", w.id)))?;
            // Fed from another thread, so a writer that answers as it reads does not block on a
            // full pipe while the piece is still being sent.
            let mut stdin = child.stdin.take().expect("piped");
            let doc = cam::json(p);
            let feeder = std::thread::spawn(move || stdin.write_all(doc.as_bytes()));
            let out = child.wait_with_output().map_err(|e| Error::new("cam.writer", e.to_string()))?;
            feeder
                .join()
                .map_err(|_| Error::new("cam.writer", "the writer's input thread failed"))?
                .map_err(|e| Error::new("cam.writer", format!("writer {} did not take the piece: {e}", w.id)))?;
            if !out.status.success() {
                return Err(Error::new("cam.writer", format!("writer {} failed on {}: {}", w.id, p.mark, String::from_utf8_lossy(&out.stderr).trim())));
            }
            Ok(out.stdout)
        }
    }
}

/// Today's date and time (UTC) as `YYYYMMDD` and `HHMMSS`.
fn now() -> (String, String) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (format!("{y:04}{m:02}{d:02}"), format!("{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ExportParams {
    /// The assemblies; every precast assembly when empty.
    #[serde(default)]
    pub assembly_ids: Vec<ElementRef>,
    pub writer_id: String,
    pub folder: String,
    /// Overrides the configuration's rule for this export.
    #[serde(default)]
    pub filename_rule: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ExportedFile {
    pub mark: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ExportReport {
    pub files: Vec<ExportedFile>,
    /// Names that would have collided and got a number.
    pub renamed: Vec<String>,
    pub warnings: Vec<String>,
}

/// Exports precast pieces for production (`precast.cam_export`).
pub fn export(store: &Store, p: ExportParams) -> Result<ExportReport> {
    let c = precast::config(store);
    let text = p.filename_rule.clone().unwrap_or_else(|| c.cam.filename.clone());
    let rule = cam::FilenameRule::parse(&text).map_err(|e| Error::new("cam.filename_rule", format!("{text:?} {e}")).with_details(serde_json::json!({ "position": e.position })))?;
    let w = writers(store).into_iter().find(|w| w.id == p.writer_id).ok_or_else(|| {
        Error::new("cam.writer", format!("no CAM writer {} (writers: {})", p.writer_id, writers(store).iter().map(|w| w.id.clone()).collect::<Vec<_>>().join(", ")))
    })?;
    let ids: Vec<ElementId> = if p.assembly_ids.is_empty() {
        let mut v: Vec<(String, ElementId)> = store.iter().filter_map(|e| precast::assembly_recipe(e).map(|a| (a.mark, e.id()))).collect();
        v.sort();
        v.into_iter().map(|x| x.1).collect()
    } else {
        p.assembly_ids.iter().map(|r| store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))).collect::<Result<_>>()?
    };
    if ids.is_empty() {
        return Err(Error::new("precast.not_assembly", "the model has no precast assemblies"));
    }
    let mut warnings = Vec::new();
    let mut pieces = Vec::new();
    for (k, id) in ids.iter().enumerate() {
        let piece = piece_model(store, *id, k as u32 + 1)?;
        if piece.bars.is_empty() && piece.meshes.is_empty() {
            warnings.push(format!("{} has no reinforcement", piece.mark));
        }
        if piece.embeds.is_empty() {
            warnings.push(format!("{} has no mounting parts", piece.mark));
        }
        pieces.push(piece);
    }
    // Every name first, so nothing is written when one fails.
    let (date, time) = now();
    let mut names: Vec<String> = Vec::new();
    let mut renamed = Vec::new();
    for pc in &pieces {
        let base = rule.render(&cam::Values { name: pc.name.clone(), prod_no: pc.prod_no, date: date.clone(), time: time.clone(), mark: pc.mark.clone() });
        let base = if base.trim().is_empty() { pc.mark.clone() } else { base };
        let mut name = base.clone();
        let mut n = 2;
        while names.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
            name = format!("{base}_{n}");
            n += 1;
        }
        if name != base {
            renamed.push(format!("{} → {name}", pc.mark));
        }
        names.push(name);
    }
    std::fs::create_dir_all(&p.folder).map_err(|e| Error::new("cam.folder", format!("{}: {e}", p.folder)))?;
    let mut files = Vec::new();
    for (pc, name) in pieces.iter().zip(&names) {
        let bytes = run_writer(&w, pc)?;
        let path = std::path::Path::new(&p.folder).join(format!("{name}.{}", w.extension));
        std::fs::write(&path, bytes).map_err(|e| Error::new("cam.write", format!("{}: {e}", path.display())))?;
        files.push(ExportedFile { mark: pc.mark.clone(), path: path.display().to_string() });
    }
    Ok(ExportReport { files, renamed, warnings })
}
