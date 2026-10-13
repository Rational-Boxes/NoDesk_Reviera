// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Grids edited by other tools (DM-IFC-006): when a file is opened, Reviera's grid records are
//! compared with the `IfcGridAxis` entities of their `IfcGrid`, and the IFC data wins.
//!
//! Records and axes are matched by tag (the grid name). Then:
//!
//! - an axis whose line differs from its record's moves the grid;
//! - an axis and a record left unmatched with the same line are a renamed grid;
//! - an axis left unmatched is a grid added by another tool: it becomes a new grid, with a
//!   GlobalId derived from the grid system and the tag, so opening the same file again gives
//!   the same grid;
//! - a record left unmatched is a grid another tool removed, and so is every grid of an
//!   `IfcGrid` that is gone.
//!
//! Axes whose curve Reviera cannot read (not a polyline) are left alone, and so are their
//! records. The changes are written back with the next save.

use std::collections::{BTreeSet, HashMap};

use reviera_core::grids::{self, GridData};
use reviera_core::walls::Line;
use reviera_core::{ElementId, GlobalId, NewElement};
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::Result;

/// Lines closer than this (metres) are the same.
const TOLERANCE: f64 = 1e-6;

/// An axis as written in the file.
#[derive(Debug, Clone, PartialEq)]
struct Axis {
    tag: String,
    line: Line,
}

/// What reconciling changed: elements to write and to remove with the next save, and a note
/// for each change.
#[derive(Debug, Default)]
pub(crate) struct Reconciled {
    pub changed: BTreeSet<ElementId>,
    pub deleted: BTreeSet<ElementId>,
    pub notes: Vec<String>,
}

fn point(ifc: &IfcModel, id: StepId, scale: f64) -> Result<Option<[f64; 2]>> {
    let AttrValue::List(c) = ifc.attribute(id, "Coordinates")? else {
        return Ok(None);
    };
    let v: Vec<f64> = c
        .iter()
        .filter_map(|a| match a {
            AttrValue::Real(r) => Some(*r * scale),
            _ => None,
        })
        .collect();
    Ok((v.len() >= 2).then(|| [v[0], v[1]]))
}

/// The axes of `grid` (polylines, first to last point, in SameSense order), and the tags of
/// axes that could not be read.
fn read_axes(ifc: &IfcModel, grid: StepId, scale: f64) -> Result<(Vec<Axis>, Vec<String>)> {
    let mut axes = Vec::new();
    let mut unreadable = Vec::new();
    for attr in ["UAxes", "VAxes", "WAxes"] {
        let AttrValue::List(list) = ifc.attribute(grid, attr)? else {
            continue;
        };
        for a in list {
            let AttrValue::Ref(axis) = a else { continue };
            let axis = StepId(axis);
            let tag = match ifc.attribute(axis, "AxisTag")? {
                AttrValue::String(s) => s,
                _ => continue,
            };
            let line = match ifc.attribute(axis, "AxisCurve")? {
                AttrValue::Ref(c)
                    if ifc.class_of(StepId(c))?.eq_ignore_ascii_case("IfcPolyline") =>
                {
                    let pts = match ifc.attribute(StepId(c), "Points")? {
                        AttrValue::List(p) => p,
                        _ => Vec::new(),
                    };
                    let ends = match (pts.first(), pts.last()) {
                        (Some(AttrValue::Ref(a)), Some(AttrValue::Ref(b))) if pts.len() >= 2 => (
                            point(ifc, StepId(*a), scale)?,
                            point(ifc, StepId(*b), scale)?,
                        ),
                        _ => (None, None),
                    };
                    match ends {
                        (Some(start), Some(end)) => Some(Line { start, end }),
                        _ => None,
                    }
                }
                _ => None,
            };
            let Some(mut line) = line else {
                unreadable.push(tag);
                continue;
            };
            if ifc.attribute(axis, "SameSense")? == AttrValue::Bool(false) {
                line = Line {
                    start: line.end,
                    end: line.start,
                };
            }
            axes.push(Axis { tag, line });
        }
    }
    Ok((axes, unreadable))
}

fn same(a: &Line, b: &Line) -> bool {
    let close = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) <= TOLERANCE;
    close(a.start, b.start) && close(a.end, b.end)
}

/// A GlobalId that depends only on the grid system and the tag (FNV-1a, 128 bits, marked as a
/// version 8 UUID).
fn derived_guid(system: &str, tag: &str) -> GlobalId {
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013B;
    let mut h: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for b in system.bytes().chain([0]).chain(tag.bytes()) {
        h ^= u128::from(b);
        h = h.wrapping_mul(PRIME);
    }
    let h = (h & !(0xF << 76)) | (0x8 << 76);
    let h = (h & !(0x3 << 62)) | (0x2 << 62);
    GlobalId::from_uuid(uuid::Uuid::from_u128(h))
}

fn record(e: &NewElement) -> Option<GridData> {
    if !e.class.starts_with(grids::CLASS_PREFIX) {
        return None;
    }
    let d: GridData = serde_json::from_value(e.params.get(grids::DATUM_KEY)?.clone()).ok()?;
    (d.kind == grids::KIND_GRID).then_some(d)
}

fn set_record(e: &mut NewElement, d: &GridData) {
    e.params.insert(
        grids::DATUM_KEY.into(),
        serde_json::to_value(d).expect("grid data serialises"),
    );
}

/// Brings the grid records among `elements` in line with the file's axes. New grids get IDs
/// from `next`.
pub(crate) fn reconcile(
    ifc: &IfcModel,
    elements: &mut Vec<(ElementId, NewElement)>,
    steps: &HashMap<ElementId, StepId>,
    next: &mut u64,
    scale: f64,
) -> Result<Reconciled> {
    let mut out = Reconciled::default();
    let systems: Vec<(String, StepId)> = elements
        .iter()
        .filter(|(_, e)| e.class == grids::SYSTEM_CLASS && e.params.contains_key(grids::SYSTEM_KEY))
        .filter_map(|(id, e)| Some((e.guid.as_ref()?.to_string(), *steps.get(id)?)))
        .collect();
    let name = |e: &NewElement| e.name.clone().unwrap_or_default();

    // Grids of an IfcGrid that is gone.
    for (id, e) in elements.iter() {
        if let Some(d) = record(e)
            && !systems.iter().any(|(g, _)| *g == d.system)
        {
            out.deleted.insert(*id);
            out.notes.push(format!(
                "Grid {}: its IfcGrid was removed by another tool; the grid is removed",
                name(e)
            ));
        }
    }

    let mut added = Vec::new();
    for (system, step) in &systems {
        let (axes, unreadable) = read_axes(ifc, *step, scale)?;
        let mut records: Vec<usize> = elements
            .iter()
            .enumerate()
            .filter(|(_, (_, e))| record(e).is_some_and(|d| d.system == *system))
            .map(|(i, _)| i)
            .collect();
        records.retain(|i| !unreadable.contains(&name(&elements[*i].1)));
        let mut unmatched_axes = Vec::new();
        let mut seen = BTreeSet::new();
        // By tag.
        for axis in axes {
            if !seen.insert(axis.tag.clone()) {
                out.notes.push(format!(
                    "Grid {}: the tag is used by more than one axis; the repeat is ignored",
                    axis.tag
                ));
                continue;
            }
            match records
                .iter()
                .position(|i| name(&elements[*i].1) == axis.tag)
            {
                Some(k) => {
                    let i = records.remove(k);
                    let (id, e) = &mut elements[i];
                    let mut d = record(e).expect("a grid record");
                    if !same(&d.curve, &axis.line) {
                        d.curve = axis.line;
                        set_record(e, &d);
                        out.changed.insert(*id);
                        out.notes.push(format!(
                            "Grid {}: moved by another tool; the grid follows",
                            axis.tag
                        ));
                    }
                }
                None => unmatched_axes.push(axis),
            }
        }
        // A record and an axis on the same line: renamed.
        unmatched_axes.retain(|axis| {
            let Some(k) = records
                .iter()
                .position(|i| record(&elements[*i].1).is_some_and(|d| same(&d.curve, &axis.line)))
            else {
                return true;
            };
            let i = records.remove(k);
            let (id, e) = &mut elements[i];
            out.notes.push(format!(
                "Grid {}: renamed {} by another tool",
                name(e),
                axis.tag
            ));
            e.name = Some(axis.tag.clone());
            out.changed.insert(*id);
            false
        });
        // Removed by another tool.
        for i in records {
            let (id, e) = &elements[i];
            out.deleted.insert(*id);
            out.notes.push(format!(
                "Grid {}: removed by another tool; the grid is removed",
                name(e)
            ));
        }
        // Added by another tool.
        for axis in unmatched_axes {
            let guid = derived_guid(system, &axis.tag);
            let id = ElementId(*next);
            *next += 1;
            let d = GridData {
                kind: grids::KIND_GRID.into(),
                system: system.clone(),
                curve: axis.line,
                bubbles: [false, true],
            };
            let mut e = NewElement::new(format!("{}{}", grids::CLASS_PREFIX, guid.as_str()))
                .guid(guid)
                .name(axis.tag.clone());
            set_record(&mut e, &d);
            out.changed.insert(id);
            out.notes.push(format!(
                "Grid {}: added by another tool; it is now a Reviera grid",
                axis.tag
            ));
            added.push((id, e));
        }
    }
    elements.retain(|(id, _)| !out.deleted.contains(id));
    elements.extend(added);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::derived_guid;

    #[test]
    fn derived_guids_are_stable_and_distinct() {
        let a = derived_guid("3l7bYlY9nB4hRvMnsPgfed", "A");
        assert_eq!(a, derived_guid("3l7bYlY9nB4hRvMnsPgfed", "A"));
        assert_ne!(a, derived_guid("3l7bYlY9nB4hRvMnsPgfed", "B"));
        assert_eq!(a.to_uuid().get_version_num(), 8);
    }
}
