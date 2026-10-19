// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Connection code checking through providers (STR-STL-009).
//!
//! A connection's **design forces** are kept with it, per load case or combination: axial force
//! (tension positive), shear in the member's depth direction, shear across, and moment (N, N·m).
//! A **provider** gets a [`CheckInput`]: the code, the connection's type and parameters, its bolt
//! group (count, diameter, area, grade with fub and the AISC nominal shear stress), the plies the
//! bolts bear on (thickness, fy, fu), its weld lines (size, length, electrode strength), its
//! members' sections and materials, and the forces. It answers a [`ProviderOutput`]: check items,
//! each with its action, resistance, utilisation ratio, pass or fail and, optionally, the
//! parameter that would make it stronger. Providers never edit the model; the application calls
//! them (Lua plugins with `capability = "connection_check"`) and records their answer here.
//!
//! The record keeps a hash of the input it answered: once the connection, its members or its
//! forces change, the input differs and the status reads *not calculated* again.
//!
//! **Presize** grows a failing item's parameter one step at a time through the bolt, plate and
//! weld size tables (or a bolt count by one), at most 20 times.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::connections::{self, CodeCheck};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::steel_fab::{self, Part};
use crate::store::Store;
use crate::walls::RECIPE_KEY;

const IN: f64 = 0.0254;
const MPA: f64 = 1.0e6;

/// Design forces for one load case or combination (N, N·m).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesignForce {
    /// The load case or combination's name.
    pub group: String,
    /// Axial force, tension positive.
    #[serde(default)]
    pub n: f64,
    /// Shear in the connected member's depth direction (a beam's end reaction).
    #[serde(default)]
    pub v: f64,
    /// Shear across the member.
    #[serde(default)]
    pub vh: f64,
    #[serde(default)]
    pub m: f64,
}

/// One check item of a provider's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    pub name: String,
    /// The load group it governs for.
    #[serde(default)]
    pub group: String,
    pub action: f64,
    pub resistance: f64,
    pub ratio: f64,
    pub ok: bool,
    /// The connection parameter that would make it stronger (`bolt_diameter`, `plate_thickness`,
    /// `weld_size`, `rows`, `bolts`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A provider's answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderOutput {
    pub items: Vec<Item>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A recorded check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckRecord {
    pub provider: String,
    pub code: String,
    pub date: String,
    pub ok: bool,
    /// The largest ratio.
    pub utilisation: f64,
    pub items: Vec<Item>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The hash of the input answered.
    pub input_hash: String,
}

// ---- Provider input ----

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BoltGroup {
    pub count: usize,
    pub diameter: f64,
    /// Nominal (shank) area.
    pub area: f64,
    pub grade: String,
    /// Ultimate strength (Pa).
    pub fub: f64,
    /// AISC 360 nominal shear stress, threads included (Pa).
    pub fnv: f64,
    pub hole_diameter: f64,
    pub pitch: Option<f64>,
    pub edge: Option<f64>,
    pub shear_planes: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Ply {
    /// `plate`, `web`, `flange` or `wall`.
    pub what: String,
    pub thickness: f64,
    pub fy: f64,
    pub fu: f64,
    pub material: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WeldLine {
    /// Leg size.
    pub size: f64,
    pub length: f64,
    /// Electrode strength (Pa).
    pub fexx: f64,
    /// The welded plate's fu (Pa), for EN 1993-1-8.
    pub fu: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MemberIn {
    pub role: String,
    pub designation: Option<String>,
    pub depth: f64,
    pub width: f64,
    pub web_thickness: f64,
    pub flange_thickness: f64,
    pub fy: f64,
    pub fu: f64,
    pub material: String,
}

/// What a provider is given (SI units: N, m, Pa).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CheckInput {
    pub code: String,
    pub connection: ElementId,
    pub connection_type: String,
    pub params: BTreeMap<String, f64>,
    pub bolts: Option<BoltGroup>,
    pub plies: Vec<Ply>,
    pub welds: Vec<WeldLine>,
    pub members: Vec<MemberIn>,
    pub forces: Vec<DesignForce>,
}

fn strength(name: &str, fallback: (&str, f64, f64)) -> (String, f64, f64) {
    match crate::materials_library::find(name) {
        Some(m) => (
            m.name.clone(),
            m.properties.get("fy_MPa").copied().unwrap_or(fallback.1) * MPA,
            m.properties.get("fu_MPa").copied().unwrap_or(fallback.2) * MPA,
        ),
        None => (fallback.0.to_owned(), fallback.1 * MPA, fallback.2 * MPA),
    }
}

/// The plate steel by region: ASTM A36 (fy 250, fu 400 MPa) or S275 (275, 430).
fn plate_steel(us: bool) -> (String, f64, f64) {
    if us {
        strength("Steel ASTM A36", ("Steel ASTM A36", 250.0, 400.0))
    } else {
        strength("Steel S275 (EN 10025-2)", ("Steel S275 (EN 10025-2)", 275.0, 430.0))
    }
}

fn recipe_of(store: &Store, id: ElementId) -> Result<connections::ConnectionRecipe> {
    store
        .get(id)
        .and_then(connections::recipe)
        .ok_or_else(|| Error::new("connection.not_a_connection", format!("element {id} is not a connection")))
}

/// The provider input for a connection, with parameters replaced by `overrides`.
pub fn input_with(store: &Store, id: ElementId, code: &str, overrides: &BTreeMap<String, f64>) -> Result<CheckInput> {
    let e = store.get(id).ok_or_else(|| Error::new("element.not_found", format!("no element {id}")))?;
    let mut c = recipe_of(store, id)?;
    if c.connection_type == "generic" {
        return Err(Error::new(
            "connection_check.generic",
            format!("connection {id} is generic: give it a type before checking it"),
        ));
    }
    if c.design_forces.is_empty() {
        return Err(Error::new(
            "connection_check.no_forces",
            format!("connection {id} has no design forces: set them first (structural_connection.set_design_forces)"),
        ));
    }
    for (k, v) in overrides {
        c.params.insert(k.clone(), Value::from(*v));
    }
    let params = steel_fab::params(store, &c);
    let us = steel_fab::us_region(store);
    let g = |k: &str| params.get(k).copied();
    let members: Vec<(String, crate::steel_detail::Frame)> = c
        .members
        .iter()
        .enumerate()
        .filter_map(|(k, guid)| {
            let m = GlobalId::parse(guid).and_then(|g| store.find_by_guid(&g)).and_then(|i| store.get(i))?;
            let role = if k == 0 { "main" } else { "connected" };
            Some((role.to_owned(), crate::steel_detail::member_frame(store, m)?))
        })
        .collect();
    let member_in: Vec<MemberIn> = members
        .iter()
        .map(|(role, f)| {
            let fallback = if us { ("Steel ASTM A992", 345.0, 450.0) } else { ("Steel S355 (EN 10025-2)", 355.0, 490.0) };
            let (material, fy, fu) = strength(f.ty.material.as_deref().unwrap_or(fallback.0), fallback);
            MemberIn {
                role: role.clone(),
                designation: f.ty.designation.clone(),
                depth: f.ty.depth,
                width: f.ty.width,
                web_thickness: f.ty.web_thickness,
                flange_thickness: f.ty.flange_thickness,
                fy,
                fu,
                material,
            }
        })
        .collect();
    // The connection's parts as they would be: bolts counted from the type's parameters where it
    // has them, else from its parts.
    let parts = steel_fab::parts(store, e).unwrap_or_default();
    let counted = parts.iter().filter(|p| matches!(p, Part::Bolt { anchor: false, .. })).count();
    let count = g("rows").or(g("bolts")).map_or(counted, |n| n.round().max(0.0) as usize);
    let bolts = g("bolt_diameter").filter(|_| count > 0).map(|d| {
        let (grade, fub, fnv) = if us {
            ("ASTM F3125 A325".to_owned(), 120.0 * 6.894_757 * MPA, 54.0 * 6.894_757 * MPA)
        } else {
            ("ISO 898-1 8.8".to_owned(), 800.0 * MPA, 0.45 * 800.0 * MPA)
        };
        BoltGroup {
            count,
            diameter: d,
            area: std::f64::consts::PI * d * d / 4.0,
            grade,
            fub,
            fnv,
            hole_diameter: d + if us { 0.0625 * IN } else { 0.002 },
            pitch: g("pitch").or(g("gauge")),
            edge: g("edge"),
            shear_planes: 1,
        }
    });
    let (pm, pfy, pfu) = plate_steel(us);
    let mut plies = Vec::new();
    if let Some(t) = g("plate_thickness") {
        plies.push(Ply { what: "plate".into(), thickness: t, fy: pfy, fu: pfu, material: pm.clone() });
    }
    // The member ply the bolts bear on.
    let ply = |k: usize, what: &str| {
        members.get(k).zip(member_in.get(k)).map(|((_, f), m)| Ply {
            what: what.into(),
            thickness: match what {
                "flange" => f.ty.flange_thickness,
                "wall" => f.ty.wall_thickness,
                _ => f.ty.web_thickness,
            },
            fy: m.fy,
            fu: m.fu,
            material: m.material.clone(),
        })
    };
    match c.connection_type.as_str() {
        "shear_plate" => plies.extend(ply(1, "web")),
        "end_plate" => plies.extend(ply(0, "flange")),
        "gusset" => {
            let hollow = members.get(1).is_some_and(|(_, f)| {
                matches!(f.ty.section, crate::columns::Section::RectangularHollow | crate::columns::Section::CircularHollow)
            });
            plies.extend(ply(1, if hollow { "wall" } else { "web" }));
        }
        _ => {}
    }
    let fexx = if us { 70.0 * 6.894_757 * MPA } else { pfu };
    let size = g("weld_size").unwrap_or(0.0);
    let welds = parts
        .iter()
        .filter_map(|p| match p {
            Part::Weld { path, .. } => Some(path.windows(2).map(|w| {
                let d = [w[1][0] - w[0][0], w[1][1] - w[0][1], w[1][2] - w[0][2]];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            }).sum::<f64>()),
            _ => None,
        })
        .map(|length| WeldLine { size, length, fexx, fu: pfu })
        .collect();
    Ok(CheckInput {
        code: code.to_owned(),
        connection: id,
        connection_type: c.connection_type.clone(),
        params,
        bolts,
        plies,
        welds,
        members: member_in,
        forces: c.design_forces.clone(),
    })
}

pub fn input(store: &Store, id: ElementId, code: &str) -> Result<CheckInput> {
    input_with(store, id, code, &BTreeMap::new())
}

/// A stable hash of an input (FNV-1a over its JSON), to notice when it changes.
pub fn hash(input: &CheckInput) -> String {
    let text = serde_json::to_string(input).expect("serialises");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

// ---- Sizes ----

/// The next size up of a parameter, from the bolt, plate and weld tables (a count by one).
pub fn next_size(parameter: &str, current: f64, us: bool) -> Option<f64> {
    let table: Vec<f64> = match (parameter, us) {
        ("bolt_diameter" | "anchor_diameter", true) => [0.5, 0.625, 0.75, 0.875, 1.0, 1.125, 1.25, 1.5].iter().map(|v| v * IN).collect(),
        ("bolt_diameter" | "anchor_diameter", false) => vec![0.012, 0.016, 0.020, 0.022, 0.024, 0.027, 0.030, 0.036],
        ("plate_thickness", true) => [0.25, 0.3125, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0, 1.25, 1.5, 2.0].iter().map(|v| v * IN).collect(),
        ("plate_thickness", false) => vec![0.006, 0.008, 0.010, 0.012, 0.015, 0.020, 0.025, 0.030, 0.035, 0.040, 0.050],
        ("weld_size", true) => [0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625, 0.75].iter().map(|v| v * IN).collect(),
        ("weld_size", false) => vec![0.004, 0.005, 0.006, 0.008, 0.010, 0.012],
        ("rows" | "bolts", _) => return (current < 12.0).then_some(current.round() + 1.0),
        _ => return None,
    };
    table.into_iter().find(|v| *v > current + 1e-9)
}

/// The steps Presize would take: the next overrides for a failing answer, or `None` when no item
/// can grow (or nothing fails).
pub fn presize_step(input: &CheckInput, out: &ProviderOutput, us: bool) -> Option<BTreeMap<String, f64>> {
    let mut next: BTreeMap<String, f64> = BTreeMap::new();
    for item in out.items.iter().filter(|i| !i.ok) {
        let Some(p) = &item.parameter else { continue };
        if next.contains_key(p) {
            continue;
        }
        let cur = input.params.get(p).copied()?;
        next.insert(p.clone(), next_size(p, cur, us)?);
    }
    (!next.is_empty()).then_some(next)
}

// ---- Commands ----

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ForcesParams {
    pub connection_ids: Vec<ElementRef>,
    /// Per load case or combination; an empty list clears them.
    pub forces: Vec<DesignForce>,
}

/// Sets connections' design forces (`structural_connection.set_design_forces`).
pub(crate) fn set_design_forces(txn: &mut crate::txn::Transaction<'_>, p: ForcesParams) -> Result<()> {
    for r in &p.connection_ids {
        let store = txn.store().clone();
        let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let mut c = recipe_of(&store, id)?;
        c.design_forces = p.forces.clone();
        txn.set_param(id, RECIPE_KEY, serde_json::to_value(&c).expect("serialises"))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RecordParams {
    pub connection_id: ElementRef,
    pub provider: String,
    pub code: String,
    pub output: ProviderOutput,
    /// Parameters to set first (Presize's sizes).
    #[serde(default)]
    pub params: Map<String, Value>,
    #[serde(default)]
    pub date: Option<String>,
}

/// Records a provider's answer on a connection (`structural_connection.record_check`): sets the
/// given parameters, then the status (ok when every item passes), utilisation and items, against
/// the input as it now is.
pub(crate) fn record(txn: &mut crate::txn::Transaction<'_>, p: RecordParams) -> Result<CheckRecord> {
    let store = txn.store().clone();
    let id = store
        .resolve(&p.connection_id)
        .ok_or_else(|| Error::new("element.not_found", format!("no element {:?}", p.connection_id)))?;
    if !p.params.is_empty() {
        steel_fab::override_params(
            txn,
            steel_fab::OverrideParams { connection_id: ElementRef::Id(id), params: p.params.clone() },
        )?;
    }
    let store = txn.store().clone();
    let input = input(&store, id, &p.code)?;
    let ok = !p.output.items.is_empty() && p.output.items.iter().all(|i| i.ok);
    let rec = CheckRecord {
        provider: p.provider.clone(),
        code: p.code.clone(),
        date: p.date.clone().unwrap_or_else(crate::overlap_accept::now_rfc3339),
        ok,
        utilisation: p.output.items.iter().map(|i| i.ratio).fold(0.0, f64::max),
        items: p.output.items.clone(),
        note: p.output.note.clone(),
        input_hash: hash(&input),
    };
    let mut c = recipe_of(&store, id)?;
    c.check = Some(rec.clone());
    c.code_check = if ok { CodeCheck::Passed } else { CodeCheck::Failed };
    txn.set_param(id, RECIPE_KEY, serde_json::to_value(&c).expect("serialises"))?;
    Ok(rec)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NotCalculated,
    Failed,
    Ok,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StatusInfo {
    pub connection: ElementId,
    pub status: Status,
    /// The last check, even when stale.
    pub check: Option<CheckRecord>,
    pub design_forces: Vec<DesignForce>,
    /// Why the status is not calculated, if it is not.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct StatusParams {
    pub connection_ids: Vec<ElementRef>,
}

/// Connections' check status (`structural_connection.check_status`): not calculated when never
/// checked or when the connection, its members or its forces changed since.
pub fn status(store: &Store, p: StatusParams) -> Result<Vec<StatusInfo>> {
    p.connection_ids
        .iter()
        .map(|r| {
            let id = store.resolve(r).ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            let c = recipe_of(store, id)?;
            let (status, reason) = match &c.check {
                None => (Status::NotCalculated, Some("not checked".to_owned())),
                Some(rec) => match input(store, id, &rec.code) {
                    Ok(i) if hash(&i) == rec.input_hash => (if rec.ok { Status::Ok } else { Status::Failed }, None),
                    Ok(_) => (Status::NotCalculated, Some("the connection, its members or its forces changed".into())),
                    Err(e) => (Status::NotCalculated, Some(e.message)),
                },
            };
            Ok(StatusInfo { connection: id, status, check: c.check.clone(), design_forces: c.design_forces.clone(), reason })
        })
        .collect()
}
