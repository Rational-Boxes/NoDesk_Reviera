// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sheets (spec Ch 5 §5.15, DOC-SHT-001…003, DOC-SCH-011): paper-space views carrying a title
//! block, viewports of model views, and placed schedules.
//!
//! A sheet is an RXD-only element (class `rxd:sheets/<guid>`, RXD document `sheets/<guid>`).
//! - **Name and data:** it is named by its element name, with its data in `rxd:sheet`: number,
//!   paper size, title-block values, viewports and schedule instances.
//! - **Coordinates:** paper coordinates are millimetres from the sheet's lower-left corner.
//! - **Viewports:** a viewport's position is the centre of its view's drawing on the paper.
//!
//! Built so far: Reviera's built-in ISO A0–A4 title blocks (landscape), viewports of plans,
//! sections and elevations at their scale, and schedules on sheets. Title block families,
//! collections, revisions, guide grids and viewport types come later.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;

pub const CLASS_PREFIX: &str = "rxd:sheets/";
pub const SHEET_KEY: &str = "rxd:sheet";

/// ISO A sheets, landscape: name, width and height in millimetres.
pub const SIZES: &[(&str, f64, f64)] = &[
    ("A0", 1189.0, 841.0),
    ("A1", 841.0, 594.0),
    ("A2", 594.0, 420.0),
    ("A3", 420.0, 297.0),
    ("A4", 297.0, 210.0),
];

/// The title-block values a sheet holds itself.
pub const PARAMETERS: &[&str] = &[
    "Issue Date",
    "Designed By",
    "Drawn By",
    "Checked By",
    "Approved By",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub id: u32,
    /// The view (GlobalId).
    pub view: String,
    /// The centre of the view's drawing on the paper, millimetres.
    pub position: [f64; 2],
    pub detail_number: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInstance {
    pub id: u32,
    /// The schedule (GlobalId).
    pub schedule: String,
    /// The top-left corner of the table on the paper, millimetres.
    pub position: [f64; 2],
}

/// A sheet's data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SheetData {
    pub kind: String,
    pub number: String,
    /// `A0` … `A4`.
    pub size: String,
    #[serde(default)]
    pub parameters: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub viewports: Vec<Viewport>,
    #[serde(default)]
    pub schedules: Vec<ScheduleInstance>,
}

impl SheetData {
    /// The paper's width and height, millimetres.
    pub fn paper(&self) -> (f64, f64) {
        SIZES
            .iter()
            .find(|s| s.0 == self.size)
            .map_or((841.0, 594.0), |s| (s.1, s.2))
    }

    fn next_id(&self) -> u32 {
        self.viewports
            .iter()
            .map(|v| v.id)
            .chain(self.schedules.iter().map(|s| s.id))
            .max()
            .unwrap_or(0)
            + 1
    }
}

pub fn is_sheet(e: &Element) -> bool {
    e.class.starts_with(CLASS_PREFIX)
}

pub fn sheet_data(e: &Element) -> Option<SheetData> {
    if !is_sheet(e) {
        return None;
    }
    serde_json::from_value(e.param(SHEET_KEY)?.clone()).ok()
}

/// The sheet a view is placed on, if any.
pub fn sheet_of_view(store: &Store, view: &GlobalId) -> Option<ElementId> {
    store.iter().find_map(|e| {
        sheet_data(e)?
            .viewports
            .iter()
            .any(|v| v.view == view.as_str())
            .then(|| e.id())
    })
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, SheetData)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("sheet.not_found", "no such sheet"))?;
    let data = sheet_data(store.get(id).expect("resolved"))
        .ok_or_else(|| Error::new("sheet.not_found", format!("element {id} is not a sheet")))?;
    Ok((id, data))
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, data: &SheetData) -> Result<()> {
    let value = serde_json::to_value(data).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(SHEET_KEY.into(), value);
    })
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// Default: the last sheet number in use, incremented (A101 → A102), or A101.
    pub number: Option<String>,
    /// Default `Unnamed`.
    pub name: Option<String>,
    /// ISO size `A0` … `A4` (landscape); default `A1`.
    pub size: Option<String>,
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let existing: Vec<String> = store
        .iter()
        .filter_map(sheet_data)
        .map(|d| d.number)
        .collect();
    let number = match p.number {
        Some(n) => {
            if existing.contains(&n) {
                return Err(Error::new(
                    "sheet.number_in_use",
                    format!("sheet number {n} is already in use"),
                ));
            }
            n
        }
        None => {
            let last = existing
                .iter()
                .max_by(|a, b| crate::schedules::natural(a, b))
                .cloned();
            let mut n = last.map_or("A101".to_owned(), |l| crate::levels::next_name(&l));
            while existing.contains(&n) {
                n = crate::levels::next_name(&n);
            }
            n
        }
    };
    let size = p.size.unwrap_or_else(|| "A1".into());
    if !SIZES.iter().any(|s| s.0 == size) {
        return Err(Error::new(
            "sheet.bad_size",
            format!("`{size}` is not a sheet size (A0 … A4)"),
        ));
    }
    let data = SheetData {
        kind: "sheet".into(),
        number,
        size,
        parameters: Default::default(),
        viewports: Vec::new(),
        schedules: Vec::new(),
    };
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{}", guid.as_str()))
            .guid(guid)
            .name(p.name.unwrap_or_else(|| "Unnamed".into()))
            .param(SHEET_KEY, serde_json::to_value(&data).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub sheet_id: ElementRef,
    pub number: Option<String>,
    pub name: Option<String>,
    pub size: Option<String>,
    /// Title-block values by name (`Drawn By`, `Issue Date`, …); an empty text clears one.
    #[serde(default)]
    pub parameters: std::collections::BTreeMap<String, String>,
}

/// Sets a sheet's number, name, size and title-block values (`sheet.set_parameter`).
pub(crate) fn set(txn: &mut crate::txn::Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut data) = resolve(&store, &p.sheet_id)?;
    if let Some(n) = p.number {
        if store
            .iter()
            .filter(|e| e.id() != id)
            .filter_map(sheet_data)
            .any(|d| d.number == n)
        {
            return Err(Error::new(
                "sheet.number_in_use",
                format!("sheet number {n} is already in use"),
            ));
        }
        data.number = n;
    }
    if let Some(s) = p.size {
        if !SIZES.iter().any(|z| z.0 == s) {
            return Err(Error::new(
                "sheet.bad_size",
                format!("`{s}` is not a sheet size"),
            ));
        }
        data.size = s;
    }
    for (k, v) in p.parameters {
        if !PARAMETERS.contains(&k.as_str()) {
            return Err(Error::new(
                "sheet.unknown_parameter",
                format!("`{k}` is not a sheet parameter"),
            ));
        }
        if v.is_empty() {
            data.parameters.remove(&k);
        } else {
            data.parameters.insert(k, v);
        }
    }
    write(txn, id, &data)?;
    if let Some(n) = p.name {
        txn.modify(id, |e| e.name = Some(n))?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceViewParams {
    pub sheet_id: ElementRef,
    pub view_id: ElementRef,
    /// The centre of the view on the paper (millimetres); default the drawing area's centre.
    pub position: Option<[f64; 2]>,
}

/// Places a plan, section or elevation on a sheet (DOC-SHT-003). A view is on one sheet only.
pub(crate) fn place_view(txn: &mut crate::txn::Transaction<'_>, p: PlaceViewParams) -> Result<u32> {
    let store = txn.store().clone();
    let (id, mut data) = resolve(&store, &p.sheet_id)?;
    let view = store
        .resolve(&p.view_id)
        .and_then(|v| store.get(v))
        .ok_or_else(|| Error::new("view.not_found", "no such view"))?;
    let kind = crate::views::view_data(view)
        .map(|d| d.kind)
        .unwrap_or_default();
    if !(crate::plans::is_plan(&kind) || crate::sections::is_section(&kind)) {
        return Err(Error::new(
            "sheet.not_placeable",
            "plans, sections and elevations can be placed on sheets (3D views come later)",
        ));
    }
    if let Some(other) = sheet_of_view(&store, view.guid()) {
        let number = store
            .get(other)
            .and_then(sheet_data)
            .map(|d| d.number)
            .unwrap_or_default();
        return Err(Error::new(
            "view.already_on_sheet",
            format!("the view is already on sheet {number}"),
        )
        .with_details(serde_json::json!({ "sheet_id": other })));
    }
    let (w, h) = data.paper();
    let vp = Viewport {
        id: data.next_id(),
        view: view.guid().as_str().to_owned(),
        position: p.position.unwrap_or([w / 2.0 - 40.0, h / 2.0 + 10.0]),
        detail_number: (data.viewports.len() + 1).to_string(),
    };
    let vid = vp.id;
    data.viewports.push(vp);
    write(txn, id, &data)?;
    Ok(vid)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceScheduleParams {
    pub sheet_id: ElementRef,
    pub schedule_id: ElementRef,
    /// The table's top-left corner on the paper (millimetres); default near the top left.
    pub position: Option<[f64; 2]>,
}

/// Places a schedule on a sheet (DOC-SCH-011); a schedule may be on several sheets.
pub(crate) fn place_schedule(
    txn: &mut crate::txn::Transaction<'_>,
    p: PlaceScheduleParams,
) -> Result<u32> {
    let store = txn.store().clone();
    let (id, mut data) = resolve(&store, &p.sheet_id)?;
    let schedule = store
        .resolve(&p.schedule_id)
        .and_then(|s| store.get(s))
        .filter(|e| crate::schedules::is_schedule(e))
        .ok_or_else(|| Error::new("schedule.not_found", "no such schedule"))?;
    let (_, h) = data.paper();
    let inst = ScheduleInstance {
        id: data.next_id(),
        schedule: schedule.guid().as_str().to_owned(),
        position: p.position.unwrap_or([30.0, h - 20.0]),
    };
    let sid = inst.id;
    data.schedules.push(inst);
    write(txn, id, &data)?;
    Ok(sid)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveParams {
    pub sheet_id: ElementRef,
    /// A viewport's or placed schedule's id on the sheet.
    pub viewport_id: u32,
    pub position: [f64; 2],
}

/// Moves a viewport or placed schedule on its sheet (`viewport.move`).
pub(crate) fn move_viewport(txn: &mut crate::txn::Transaction<'_>, p: MoveParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut data) = resolve(&store, &p.sheet_id)?;
    if let Some(v) = data.viewports.iter_mut().find(|v| v.id == p.viewport_id) {
        v.position = p.position;
    } else if let Some(s) = data.schedules.iter_mut().find(|s| s.id == p.viewport_id) {
        s.position = p.position;
    } else {
        return Err(Error::new(
            "viewport.not_found",
            format!("the sheet has no viewport {}", p.viewport_id),
        ));
    }
    write(txn, id, &data)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveParams {
    pub sheet_id: ElementRef,
    pub viewport_ids: Vec<u32>,
}

/// Removes viewports or placed schedules from a sheet; the views and schedules are kept.
pub(crate) fn remove_viewports(
    txn: &mut crate::txn::Transaction<'_>,
    p: RemoveParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut data) = resolve(&store, &p.sheet_id)?;
    data.viewports.retain(|v| !p.viewport_ids.contains(&v.id));
    data.schedules.retain(|s| !p.viewport_ids.contains(&s.id));
    write(txn, id, &data)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    pub sheet_ids: Vec<ElementRef>,
}

/// Deletes sheets with their viewports; the views and schedules are kept.
pub(crate) fn delete(txn: &mut crate::txn::Transaction<'_>, p: DeleteParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.sheet_ids {
        let (id, _) = resolve(&store, r)?;
        txn.delete(id)?;
    }
    Ok(())
}

/// Removes viewports and placed schedules whose view or schedule no longer exists.
pub(crate) fn drop_missing_viewports(txn: &mut crate::txn::Transaction<'_>) -> Result<()> {
    let store = txn.store().clone();
    let exists = |g: &str| {
        GlobalId::parse(g)
            .and_then(|g| store.find_by_guid(&g))
            .is_some()
    };
    for e in store.iter() {
        let Some(mut data) = sheet_data(e) else {
            continue;
        };
        let before = (data.viewports.len(), data.schedules.len());
        data.viewports.retain(|v| exists(&v.view));
        data.schedules.retain(|s| exists(&s.schedule));
        if before != (data.viewports.len(), data.schedules.len()) {
            write(txn, e.id(), &data)?;
        }
    }
    Ok(())
}

/// The viewports whose view exists, with the view.
pub fn live_viewports(store: &Store, data: &SheetData) -> Vec<(Viewport, ElementId)> {
    data.viewports
        .iter()
        .filter_map(|v| {
            let id = store.find_by_guid(&GlobalId::parse(&v.view)?)?;
            Some((v.clone(), id))
        })
        .collect()
}

/// The values a title block shows: sheet values, then project information.
pub fn title_values(store: &Store, sheet: &Element) -> Vec<(String, String)> {
    let data = sheet_data(sheet).unwrap_or(SheetData {
        kind: "sheet".into(),
        number: String::new(),
        size: "A1".into(),
        parameters: Default::default(),
        viewports: Vec::new(),
        schedules: Vec::new(),
    });
    let project = store.iter().find(|e| e.class == "IfcProject");
    let text = |e: Option<&Element>, key: &str| {
        e.and_then(|e| e.param(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let project_name = text(project, "@LongName")
        .or_else(|| project.and_then(|p| p.name.clone()))
        .unwrap_or_default();
    let project_number = project.and_then(|p| p.name.clone()).unwrap_or_default();
    let mut out = vec![
        ("Project".to_owned(), project_name),
        ("Project Number".to_owned(), project_number),
        (
            "Sheet Name".to_owned(),
            sheet.name.clone().unwrap_or_default(),
        ),
        ("Sheet Number".to_owned(), data.number.clone()),
    ];
    for k in PARAMETERS {
        out.push((
            (*k).to_owned(),
            data.parameters.get(*k).cloned().unwrap_or_default(),
        ));
    }
    out
}
