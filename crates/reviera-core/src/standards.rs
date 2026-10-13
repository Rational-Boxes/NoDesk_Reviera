// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Graphic standards (spec Ch 5 §5.11): line weights (DOC-GFX-009) and object styles
//! (DOC-GFX-002).
//!
//! - **Line weights**: 16 logical pens mapped to paper millimetres. Model pens depend on the view
//!   scale: a view uses the column of the nearest listed scale, by ratio (ties go to the coarser
//!   scale). Annotation and perspective pens do not depend on scale.
//! - **Object styles**: the projection and cut pen of each category.
//!
//! Both are project standards, stored as RXD documents `standards/line_weights` and
//! `standards/object_styles` (RXD-only elements with those classes), with built-in defaults until
//! a project changes them.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::element::NewElement;
use crate::error::{Error, Result};
use crate::store::Store;

pub const LINE_WEIGHTS_CLASS: &str = "rxd:standards/line_weights";
pub const OBJECT_STYLES_CLASS: &str = "rxd:standards/object_styles";
const KEY: &str = "rxd:standard";
pub const PENS: usize = 16;

/// Pen widths at 1:100, in paper millimetres.
const BASE: [f64; PENS] = [
    0.10, 0.13, 0.18, 0.25, 0.35, 0.50, 0.70, 1.00, 1.40, 2.00, 2.50, 3.00, 3.50, 4.00, 4.50, 5.00,
];

/// The pen widths of a project (`standards/line_weights`, kind `penset`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PenSet {
    /// Model pens by view scale denominator.
    pub model: BTreeMap<u32, Vec<f64>>,
    pub perspective: Vec<f64>,
    pub annotation: Vec<f64>,
}

impl Default for PenSet {
    fn default() -> Self {
        let column = |factor: f64| {
            BASE.iter()
                .map(|w| ((w * factor * 100.0).round() / 100.0).max(0.1))
                .collect::<Vec<_>>()
        };
        Self {
            model: [(20, 1.8), (50, 1.4), (100, 1.0), (200, 0.8), (500, 0.7)]
                .into_iter()
                .map(|(s, f)| (s, column(f)))
                .collect(),
            perspective: column(1.0),
            annotation: column(1.0),
        }
    }
}

impl PenSet {
    /// The scale column used for a view at 1:`scale`: the nearest by ratio, ties to the coarser.
    pub fn column_for(&self, scale: f64) -> Option<u32> {
        self.model.keys().copied().min_by(|a, b| {
            let ratio = |s: u32| {
                let s = f64::from(s);
                s.max(scale) / s.min(scale)
            };
            ratio(*a)
                .total_cmp(&ratio(*b))
                // Ties go to the coarser scale (the larger denominator).
                .then(b.cmp(a))
        })
    }

    /// The width in paper millimetres of model pen `pen` (1–16) at 1:`scale`.
    pub fn model_width(&self, pen: u8, scale: f64) -> f64 {
        let column = self.column_for(scale).and_then(|c| self.model.get(&c));
        let i = usize::from(pen.clamp(1, PENS as u8)) - 1;
        column.and_then(|c| c.get(i)).copied().unwrap_or(BASE[i])
    }

    pub fn annotation_width(&self, pen: u8) -> f64 {
        let i = usize::from(pen.clamp(1, PENS as u8)) - 1;
        self.annotation.get(i).copied().unwrap_or(BASE[i])
    }
}

/// The style of one category (`standards/object_styles`, kind `objectstyle`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObjectStyle {
    pub projection_pen: u8,
    /// `None` for categories that are not cut.
    pub cut_pen: Option<u8>,
}

/// Object styles by category key.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStyles {
    pub styles: BTreeMap<String, ObjectStyle>,
}

/// The built-in style of a category.
pub fn default_style(category: &str) -> ObjectStyle {
    let (projection, cut) = match category {
        "cat.walls" | "cat.columns" | "cat.structural_columns" => (2, Some(5)),
        "cat.floors" | "cat.roofs" | "cat.structural_foundations" | "cat.toposolid" => (2, Some(4)),
        "cat.structural_framing" | "cat.ceilings" | "cat.curtain_walls" => (2, Some(3)),
        "cat.doors"
        | "cat.windows"
        | "cat.stairs"
        | "cat.railings"
        | "cat.ramps"
        | "cat.curtain_mullions"
        | "cat.curtain_panels"
        | "cat.generic_models"
        | "cat.casework" => (1, Some(3)),
        "cat.furniture" | "cat.plumbing_fixtures" | "cat.specialty_equipment" => (1, None),
        _ => (1, Some(2)),
    };
    ObjectStyle {
        projection_pen: projection,
        cut_pen: cut,
    }
}

impl ObjectStyles {
    pub fn style(&self, category: &str) -> ObjectStyle {
        self.styles
            .get(category)
            .copied()
            .unwrap_or_else(|| default_style(category))
    }
}

/// The paper widths (mm) of an element's cut lines (`None` if its category is not cut) and
/// projection lines in a view at 1:`scale`.
pub fn element_widths(
    store: &Store,
    pens: &PenSet,
    styles: &ObjectStyles,
    element: Option<crate::id::ElementId>,
    scale: f64,
) -> (Option<f64>, f64) {
    let category = element
        .and_then(|e| store.get(e))
        .and_then(crate::categories::category_of)
        .map_or("", |c| c.key);
    let style = styles.style(category);
    (
        style.cut_pen.map(|p| pens.model_width(p, scale)),
        pens.model_width(style.projection_pen, scale),
    )
}

fn stored<T: serde::de::DeserializeOwned + Default>(store: &Store, class: &str) -> T {
    store
        .iter()
        .find(|e| e.class == class)
        .and_then(|e| serde_json::from_value(e.param(KEY)?.clone()).ok())
        .unwrap_or_default()
}

pub fn pen_set(store: &Store) -> PenSet {
    stored(store, LINE_WEIGHTS_CLASS)
}

pub fn object_styles(store: &Store) -> ObjectStyles {
    stored(store, OBJECT_STYLES_CLASS)
}

fn write<T: Serialize>(
    txn: &mut crate::txn::Transaction<'_>,
    class: &str,
    value: &T,
) -> Result<()> {
    let value = serde_json::to_value(value).expect("standards serialise");
    let existing = txn
        .store()
        .iter()
        .find(|e| e.class == class)
        .map(|e| e.id());
    match existing {
        Some(id) => txn.modify(id, |e| {
            e.params.insert(KEY.into(), value);
        }),
        None => txn
            .create(NewElement::new(class).param(KEY, value))
            .map(|_| ()),
    }
}

fn check_pen(pen: u8) -> Result<()> {
    if (1..=PENS as u8).contains(&pen) {
        Ok(())
    } else {
        Err(Error::new(
            "lineweight.bad_pen",
            format!("pens are 1 to {PENS}, not {pen}"),
        ))
    }
}

// ---- Commands ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PenTable {
    Model,
    Perspective,
    Annotation,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLineWeightParams {
    pub table: PenTable,
    pub pen: u8,
    /// Paper millimetres.
    pub width_mm: f64,
    /// The scale column of a model pen (100 for 1:100).
    pub scale: Option<u32>,
}

pub(crate) fn set_line_weight(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetLineWeightParams,
) -> Result<()> {
    check_pen(p.pen)?;
    if !(0.01..=10.0).contains(&p.width_mm) {
        return Err(Error::new(
            "lineweight.bad_width",
            "widths are 0.01 to 10 mm",
        ));
    }
    let mut set = pen_set(txn.store());
    let i = usize::from(p.pen) - 1;
    match p.table {
        PenTable::Model => {
            let scale = p
                .scale
                .ok_or_else(|| Error::new("lineweight.no_scale", "model pens are set per scale"))?;
            let column = set.model.get_mut(&scale).ok_or_else(|| {
                Error::new(
                    "lineweight.no_scale",
                    format!("there is no 1:{scale} column; add it first"),
                )
            })?;
            column[i] = p.width_mm;
        }
        PenTable::Perspective => set.perspective[i] = p.width_mm,
        PenTable::Annotation => set.annotation[i] = p.width_mm,
    }
    write(txn, LINE_WEIGHTS_CLASS, &set)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScaleParams {
    pub scale: u32,
}

pub(crate) fn add_scale(txn: &mut crate::txn::Transaction<'_>, p: ScaleParams) -> Result<()> {
    if p.scale == 0 {
        return Err(Error::new(
            "lineweight.bad_scale",
            "the scale must be positive",
        ));
    }
    let mut set = pen_set(txn.store());
    if set.model.contains_key(&p.scale) {
        return Err(Error::new(
            "lineweight.scale_exists",
            format!("1:{} is already listed", p.scale),
        ));
    }
    // The new column starts as a copy of the nearest one.
    let nearest = set
        .column_for(f64::from(p.scale))
        .and_then(|c| set.model.get(&c).cloned())
        .unwrap_or_else(|| BASE.to_vec());
    set.model.insert(p.scale, nearest);
    write(txn, LINE_WEIGHTS_CLASS, &set)
}

pub(crate) fn remove_scale(txn: &mut crate::txn::Transaction<'_>, p: ScaleParams) -> Result<()> {
    let mut set = pen_set(txn.store());
    if set.model.len() == 1 {
        return Err(Error::new(
            "lineweight.last_scale",
            "the last scale column cannot be removed",
        ));
    }
    set.model.remove(&p.scale).ok_or_else(|| {
        Error::new(
            "lineweight.no_scale",
            format!("1:{} is not listed", p.scale),
        )
    })?;
    write(txn, LINE_WEIGHTS_CLASS, &set)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetObjectStyleParams {
    /// A category key, for example `cat.walls`.
    pub category: String,
    /// Sub-categories are not supported yet.
    pub subcategory: Option<String>,
    /// Projection pen (1–16).
    pub projection_weight: Option<u8>,
    /// Cut pen (1–16; cuttable categories only).
    pub cut_weight: Option<u8>,
}

pub(crate) fn set_object_style(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetObjectStyleParams,
) -> Result<()> {
    if crate::categories::category(&p.category).is_none() {
        return Err(Error::new(
            "objectstyle.bad_category",
            format!("no category `{}`", p.category),
        ));
    }
    if p.subcategory.is_some() {
        return Err(Error::new(
            "objectstyle.unsupported",
            "sub-categories are not supported yet",
        ));
    }
    let mut styles = object_styles(txn.store());
    let mut style = styles.style(&p.category);
    if let Some(pen) = p.projection_weight {
        check_pen(pen)?;
        style.projection_pen = pen;
    }
    if let Some(pen) = p.cut_weight {
        check_pen(pen)?;
        if style.cut_pen.is_none() {
            return Err(Error::new(
                "objectstyle.not_cuttable",
                format!("`{}` is not cut in plans and sections", p.category),
            ));
        }
        style.cut_pen = Some(pen);
    }
    styles.styles.insert(p.category, style);
    write(txn, OBJECT_STYLES_CLASS, &styles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_scale_column_by_ratio_ties_to_the_coarser() {
        // The acceptance example of DOC-GFX-009.
        let mut set = PenSet {
            model: [(50, vec![0.5; PENS]), (200, vec![0.35; PENS])]
                .into_iter()
                .collect(),
            ..PenSet::default()
        };
        assert_eq!(set.model_width(5, 50.0), 0.5);
        assert_eq!(set.model_width(5, 200.0), 0.35);
        assert_eq!(set.column_for(75.0), Some(50));
        set.model.insert(100, vec![0.42; PENS]);
        assert_eq!(set.column_for(75.0), Some(100), "1.33 is nearer than 1.5");
        // 1:100 between 1:50 and 1:200 is a tie (2 and 2): the coarser wins.
        set.model.remove(&100);
        assert_eq!(set.column_for(100.0), Some(200));
    }
}
