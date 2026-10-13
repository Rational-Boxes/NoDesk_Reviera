// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Plan views and their view range (spec Ch 5 §5.3, DOC-VIEW-002 and DOC-VR-001).
//!
//! A floor plan is a view of kind `view.plan` associated with a level. Its view range has four
//! planes (top, cut, bottom, view depth), each a level reference plus an offset; the references
//! `associated`, `above` and `below` resolve against the sorted level list each time, so adding a
//! level in between changes the result.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::NewElement;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::views::{self, CLASS_PREFIX, VIEW_KEY, ViewData};

pub const KIND_PLAN: &str = "view.plan";
/// A reflected ceiling plan: cut at its cut plane, looking up to the top of its range.
pub const KIND_CEILING_PLAN: &str = "view.ceiling_plan";
/// Default cut plane height of a ceiling plan above the level.
pub const DEFAULT_CEILING_CUT: f64 = 2.3;

/// Whether a view kind is a plan (floor or ceiling plan).
pub fn is_plan(kind: &str) -> bool {
    kind == KIND_PLAN || kind == KIND_CEILING_PLAN
}
/// Default cut plane height above the level (DOC-VR-001).
pub const DEFAULT_CUT: f64 = 1.2;
/// Default top of the range when there is no level above.
pub const DEFAULT_TOP: f64 = 2.3;

/// One plane of a view range: a level reference and an offset in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Plane {
    /// `associated`, `above`, `below`, `unlimited` (top and view depth only), or a level's
    /// GlobalId.
    pub level: String,
    #[serde(default)]
    pub offset: f64,
}

impl Plane {
    fn new(level: &str, offset: f64) -> Self {
        Self {
            level: level.into(),
            offset,
        }
    }
}

/// A plan's view range (`viewRange` block of the view payload).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewRange {
    pub top: Plane,
    pub cut: Plane,
    pub bottom: Plane,
    pub depth: Plane,
}

impl ViewRange {
    /// The defaults of a new ceiling plan: cut 2300 mm above the level, looking up to the level
    /// above (or 4 m up without one).
    pub fn ceiling_plan_default(has_level_above: bool) -> Self {
        let top = if has_level_above {
            Plane::new("above", 0.0)
        } else {
            Plane::new("associated", 4.0)
        };
        Self {
            top: top.clone(),
            cut: Plane::new("associated", DEFAULT_CEILING_CUT),
            bottom: Plane::new("associated", DEFAULT_CEILING_CUT),
            depth: top,
        }
    }

    /// The defaults of a new floor plan.
    pub fn floor_plan_default(has_level_above: bool) -> Self {
        Self {
            top: if has_level_above {
                Plane::new("above", 0.0)
            } else {
                Plane::new("associated", DEFAULT_TOP)
            },
            cut: Plane::new("associated", DEFAULT_CUT),
            bottom: Plane::new("associated", 0.0),
            depth: Plane::new("associated", 0.0),
        }
    }
}

/// Resolved elevations of a view range, in metres (infinite for `unlimited`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct Resolved {
    pub top: f64,
    pub cut: f64,
    pub bottom: f64,
    pub depth: f64,
}

/// The levels of a model, lowest first: (elevation, GlobalId, element).
pub fn levels(store: &Store) -> Vec<(f64, String, ElementId)> {
    let mut levels: Vec<_> = store
        .iter()
        .filter(|e| e.class == "IfcBuildingStorey")
        .map(|e| {
            let z = e.param("@Elevation").and_then(Value::as_f64).unwrap_or(0.0);
            (z, e.guid().to_string(), e.id())
        })
        .collect();
    levels.sort_by(|a, b| a.0.total_cmp(&b.0));
    levels
}

/// Resolves a view range for the plan of `level` (GlobalId).
pub fn resolve(store: &Store, level: &str, range: &ViewRange) -> Result<Resolved> {
    let levels = levels(store);
    let index = levels
        .iter()
        .position(|l| l.1 == level)
        .ok_or_else(|| Error::new("view.level_missing", "the plan's level no longer exists"))?;
    let elevation = levels[index].0;
    let plane = |p: &Plane, unlimited: f64| -> Result<f64> {
        let base = match p.level.as_str() {
            "associated" => elevation,
            "above" => match levels.get(index + 1) {
                Some(l) => l.0,
                None => return Ok(f64::INFINITY),
            },
            "below" => match index.checked_sub(1) {
                Some(i) => levels[i].0,
                None => return Ok(f64::NEG_INFINITY),
            },
            "unlimited" => return Ok(unlimited),
            guid => {
                levels
                    .iter()
                    .find(|l| l.1 == guid)
                    .ok_or_else(|| Error::new("view_range.bad_level", format!("no level {guid}")))?
                    .0
            }
        };
        Ok(base + p.offset)
    };
    for (name, p) in [("cut", &range.cut), ("bottom", &range.bottom)] {
        if p.level == "unlimited" {
            return Err(Error::new(
                "view_range.invalid_level",
                format!("the {name} plane cannot be unlimited"),
            ));
        }
    }
    Ok(Resolved {
        top: plane(&range.top, f64::INFINITY)?,
        cut: plane(&range.cut, f64::NAN)?,
        bottom: plane(&range.bottom, f64::NAN)?,
        depth: plane(&range.depth, f64::NEG_INFINITY)?,
    })
}

/// The resolved range of a plan view element.
pub fn plan_range(store: &Store, view: ElementId) -> Option<Resolved> {
    let data = views::view_data(store.get(view)?)?;
    resolve(store, data.level.as_deref()?, data.view_range.as_ref()?).ok()
}

fn check_order(r: &Resolved, ceiling: bool) -> Result<()> {
    if ceiling {
        // A ceiling plan looks up from its cut plane to its top.
        return if r.top >= r.cut {
            Ok(())
        } else {
            Err(Error::new(
                "view_range.invalid_order",
                format!(
                    "a ceiling plan's top must be at or above its cut plane (resolved: {}, {})",
                    r.top, r.cut
                ),
            ))
        };
    }
    if r.top >= r.cut && r.cut >= r.bottom && r.bottom >= r.depth {
        Ok(())
    } else {
        Err(Error::new(
            "view_range.invalid_order",
            format!(
                "the planes must be in order Top ≥ Cut ≥ Bottom ≥ View Depth (resolved: {}, {}, {}, {})",
                r.top, r.cut, r.bottom, r.depth
            ),
        ))
    }
}

// ---- Commands ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    #[default]
    FloorPlan,
    CeilingPlan,
    StructuralPlan,
    AreaPlan,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePlanParams {
    #[serde(default)]
    pub kind: PlanKind,
    pub level_ids: Vec<ElementRef>,
    /// Also create plans for levels that already have one (named `<level> (2)`, …).
    #[serde(default)]
    pub allow_duplicates: bool,
    /// Drawing scale denominator (100 for 1:100).
    pub scale: Option<f64>,
}

pub(crate) fn create_plan(
    txn: &mut crate::txn::Transaction<'_>,
    p: CreatePlanParams,
) -> Result<Vec<ElementId>> {
    let kind = match p.kind {
        PlanKind::FloorPlan => KIND_PLAN,
        PlanKind::CeilingPlan => KIND_CEILING_PLAN,
        _ => {
            return Err(Error::new(
                "view.unsupported_kind",
                "only floor and ceiling plans can be created so far",
            ));
        }
    };
    let mut created = Vec::new();
    for r in &p.level_ids {
        let store = txn.store().clone();
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
        let level = store.get(id).expect("resolved");
        if level.class != "IfcBuildingStorey" {
            return Err(Error::new(
                "view.not_a_level",
                format!("element {id} is not a level"),
            ));
        }
        let guid = level.guid().to_string();
        let base = level.name.clone().unwrap_or_else(|| format!("Level {id}"));
        let plans: Vec<(String, Option<String>)> = store
            .iter()
            .filter_map(|e| {
                let d = views::view_data(e)?;
                (d.kind == kind).then(|| (e.name.clone().unwrap_or_default(), d.level))
            })
            .collect();
        if !p.allow_duplicates
            && plans
                .iter()
                .any(|(_, l)| l.as_deref() == Some(guid.as_str()))
        {
            continue;
        }
        let existing: Vec<String> = plans.into_iter().map(|(n, _)| n).collect();
        let name = (1..)
            .map(|n| {
                if n == 1 {
                    base.clone()
                } else {
                    format!("{base} ({n})")
                }
            })
            .find(|n| !existing.contains(n))
            .expect("a free name");
        let all = levels(&store);
        let index = all.iter().position(|l| l.1 == guid).unwrap_or(0);
        let data = ViewData {
            kind: kind.into(),
            camera: None,
            home: None,
            level: Some(guid),
            view_range: Some(if kind == KIND_CEILING_PLAN {
                ViewRange::ceiling_plan_default(index + 1 < all.len())
            } else {
                ViewRange::floor_plan_default(index + 1 < all.len())
            }),
            section_box: None,
            slice_planes: Vec::new(),
            section: None,
        };
        let mut value = serde_json::to_value(&data).expect("view data serialises");
        if let Some(scale) = p.scale {
            value["scale"] = scale.into();
        }
        let view_guid = GlobalId::new();
        created.push(
            txn.create(
                NewElement::new(format!("{CLASS_PREFIX}{}", view_guid.as_str()))
                    .guid(view_guid)
                    .name(name)
                    .param(VIEW_KEY, value),
            )?,
        );
    }
    Ok(created)
}

/// A plane argument: a level reference and an offset (lengths accept units).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaneArg {
    /// `associated`, `above`, `below`, `unlimited`, a level's internal ID or its GlobalId.
    pub level_ref: Value,
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub offset: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetViewRangeParams {
    pub view_id: ElementRef,
    pub top: PlaneArg,
    pub cut: PlaneArg,
    pub bottom: PlaneArg,
    pub depth: PlaneArg,
}

fn plane_of(store: &Store, a: &PlaneArg) -> Result<Plane> {
    let level = match &a.level_ref {
        Value::String(s) if ["associated", "above", "below", "unlimited"].contains(&s.as_str()) => {
            s.clone()
        }
        Value::String(s) => GlobalId::parse(s)
            .and_then(|g| store.find_by_guid(&g))
            .and_then(|id| store.get(id))
            .filter(|e| e.class == "IfcBuildingStorey")
            .map(|e| e.guid().to_string())
            .ok_or_else(|| Error::new("view_range.bad_level", format!("no level {s}")))?,
        Value::Number(n) => n
            .as_u64()
            .and_then(|id| store.get(ElementId(id)))
            .filter(|e| e.class == "IfcBuildingStorey")
            .map(|e| e.guid().to_string())
            .ok_or_else(|| Error::new("view_range.bad_level", format!("no level {n}")))?,
        other => {
            return Err(Error::new(
                "view_range.bad_level",
                format!("{other} is not a level reference"),
            ));
        }
    };
    Ok(Plane {
        level,
        offset: a.offset,
    })
}

fn plan_view(store: &Store, r: &ElementRef) -> Result<(ElementId, ViewData, String)> {
    let (id, data) = views::resolve_view(store, r)?;
    let level = data
        .level
        .clone()
        .filter(|_| is_plan(&data.kind))
        .ok_or_else(|| Error::new("view.not_a_plan", format!("view {id} is not a plan")))?;
    Ok((id, data, level))
}

pub(crate) fn set_view_range(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetViewRangeParams,
) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut data, level) = plan_view(&store, &p.view_id)?;
    let range = ViewRange {
        top: plane_of(&store, &p.top)?,
        cut: plane_of(&store, &p.cut)?,
        bottom: plane_of(&store, &p.bottom)?,
        depth: plane_of(&store, &p.depth)?,
    };
    check_order(
        &resolve(&store, &level, &range)?,
        data.kind == KIND_CEILING_PLAN,
    )?;
    data.view_range = Some(range);
    let value = serde_json::to_value(&data).expect("view data serialises");
    txn.modify(id, |e| {
        // Keep fields this version does not know (scale and the like).
        let mut merged = e.param(VIEW_KEY).cloned().unwrap_or(Value::Null);
        if let (Some(m), Some(v)) = (merged.as_object_mut(), value.as_object()) {
            for (k, val) in v {
                m.insert(k.clone(), val.clone());
            }
        } else {
            merged = value;
        }
        e.params.insert(VIEW_KEY.into(), merged);
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ViewRangeReport {
    pub planes: ViewRange,
    /// Elevations in metres; `null` for unlimited.
    pub resolved_elevations: ResolvedReport,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ResolvedReport {
    pub top: Option<f64>,
    pub cut: Option<f64>,
    pub bottom: Option<f64>,
    pub depth: Option<f64>,
}

pub(crate) fn get_view_range(store: &Store, view: &ElementRef) -> Result<ViewRangeReport> {
    let (_, data, level) = plan_view(store, view)?;
    let planes = data
        .view_range
        .ok_or_else(|| Error::new("view.no_range", "the plan has no view range"))?;
    let r = resolve(store, &level, &planes)?;
    let finite = |v: f64| v.is_finite().then_some(v);
    Ok(ViewRangeReport {
        resolved_elevations: ResolvedReport {
            top: finite(r.top),
            cut: finite(r.cut),
            bottom: finite(r.bottom),
            depth: finite(r.depth),
        },
        planes,
    })
}
