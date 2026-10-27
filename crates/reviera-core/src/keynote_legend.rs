// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Keynote legends (spec Ch 5 §5.10, DOC-VIEW-010): a schedule (`schedule.keynotelegend`) listing
//! the keynotes the project's keynote tags show (DOC-ANN-014), one row per key in natural order,
//! with its text from the keynote table. With *Filter by sheet*, a legend placed on a sheet lists
//! only the keynotes of the views placed on that sheet; elsewhere (its own tab) it lists them all.
//!
//! Its columns are *Key Value* and *Keynote Text*, or one of them; its other schedule settings
//! (filters, sorting, grouping) do not apply.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::schedules::{self, Cell, Field, Row, ScheduleDef, Table};
use crate::store::Store;
use crate::txn::Transaction;

pub const KIND_KEYNOTE_LEGEND: &str = "schedule.keynotelegend";
/// The legend's fields.
pub const FIELD_KEY: &str = "keynote.key";
pub const FIELD_TEXT: &str = "keynote.text";

pub fn is_keynote_legend(def: &ScheduleDef) -> bool {
    def.kind == KIND_KEYNOTE_LEGEND
}

fn text_cell(text: String) -> Cell {
    Cell { text, value: None, editable: false }
}

/// The legend's table: on a sheet whose placed views are `sheet_views` (GlobalIds) when it
/// filters by sheet, else of every keynote tag of the model.
pub fn table(store: &Store, schedule: &Element, def: &ScheduleDef, sheet_views: Option<&[String]>) -> Result<Table> {
    let only = sheet_views.filter(|_| def.filter_by_sheet);
    let mut keys: Vec<String> = Vec::new();
    for e in store.iter() {
        let Some(d) = crate::keynote_tags::keynote_data(e) else { continue };
        if only.is_some_and(|views| !views.contains(&d.view)) {
            continue;
        }
        if let Some(k) = crate::keynote_tags::resolve(store, e.id(), d).key
            && !keys.contains(&k)
        {
            keys.push(k);
        }
    }
    keys.sort_by(|a, b| schedules::natural(a, b));
    let rows = keys
        .into_iter()
        .map(|k| {
            let text = crate::classification::keynote_text(store, &k).unwrap_or_else(|| "(not in the keynote table)".to_owned());
            Row {
                elements: Vec::new(),
                cells: def
                    .fields
                    .iter()
                    .map(|f| match f.key.as_str() {
                        FIELD_TEXT => text_cell(text.clone()),
                        _ => text_cell(k.clone()),
                    })
                    .collect(),
            }
        })
        .collect();
    let headings = def
        .fields
        .iter()
        .map(|f| f.heading.clone().unwrap_or_else(|| if f.key == FIELD_TEXT { "Keynote Text".into() } else { "Key Value".into() }))
        .collect();
    Ok(Table {
        title: schedule.name.clone().unwrap_or_else(|| "Keynote Legend".into()),
        fields: def.fields.iter().map(|f| f.key.clone()).collect(),
        headings,
        rows,
        totals: None,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// Default "Keynote Legend" (numbered when taken).
    pub name: Option<String>,
    /// `key` and `text` (the default, both, in this order) or one of them.
    pub fields: Option<Vec<String>>,
    /// Each placed legend lists only the keynotes of its own sheet's views.
    pub filter_by_sheet: Option<bool>,
}

fn fields_of(names: Option<Vec<String>>) -> Result<Vec<Field>> {
    let names = names.unwrap_or_else(|| vec!["key".into(), "text".into()]);
    if names.is_empty() {
        return Err(Error::new("schedule.no_fields", "a keynote legend needs a column"));
    }
    names
        .iter()
        .map(|n| match n.as_str() {
            "key" | FIELD_KEY => Ok(Field { key: FIELD_KEY.into(), heading: None }),
            "text" | FIELD_TEXT => Ok(Field { key: FIELD_TEXT.into(), heading: None }),
            other => Err(Error::new("schedule.bad_field", format!("{other:?}: a keynote legend's columns are key and text"))),
        })
        .collect()
}

/// `view.create_keynote_legend`.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let base = p.name.unwrap_or_else(|| "Keynote Legend".into());
    let taken: Vec<String> = store.iter().filter(|x| schedules::is_schedule(x)).filter_map(|x| x.name.clone()).collect();
    let name = std::iter::once(base.clone()).chain((2..).map(|n| format!("{base} {n}"))).find(|n| !taken.contains(n)).expect("a free name");
    let def = ScheduleDef {
        kind: KIND_KEYNOTE_LEGEND.into(),
        category: String::new(),
        fields: fields_of(p.fields)?,
        sort: Vec::new(),
        itemize: true,
        grand_totals: false,
        panel: None,
        filters: Vec::new(),
        filter_by_sheet: p.filter_by_sheet.unwrap_or(false),
    };
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{}{}", schedules::CLASS_PREFIX, guid.as_str()))
            .guid(guid)
            .name(name)
            .param(schedules::SCHEDULE_KEY, serde_json::to_value(&def).expect("serialises")),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub schedule_id: ElementRef,
    pub filter_by_sheet: Option<bool>,
    /// `key`, `text`, or both in order.
    pub fields: Option<Vec<String>>,
}

/// `view.set_keynote_legend`.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    let id = store.resolve(&p.schedule_id).ok_or_else(|| Error::new("schedule.not_found", "no such schedule"))?;
    let mut def = schedules::schedule_def(store.get(id).expect("resolved"))
        .filter(is_keynote_legend)
        .ok_or_else(|| Error::new("schedule.not_keynote_legend", format!("element {id} is not a keynote legend")))?;
    if let Some(v) = p.filter_by_sheet {
        def.filter_by_sheet = v;
    }
    if p.fields.is_some() {
        def.fields = fields_of(p.fields)?;
    }
    let value = serde_json::to_value(&def).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(schedules::SCHEDULE_KEY.into(), value);
    })
}
