// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Structural walls (STR-WALL-001) and structural floors (STR-FLOOR-001), on the architectural
//! wall and floor engines.
//!
//! A wall's **structural usage** is bearing, shear, structural combined or non-bearing. A
//! structural wall carries an RXD `structural` record with its usage (IFC cannot tell bearing
//! from structural combined), PredefinedType `SHEAR` for shear walls and `SOLIDWALL` for the
//! others, and `Pset_WallCommon.LoadBearing` true; a non-bearing wall is an ordinary wall again.
//!
//! A **structural floor** carries an RXD `structural` record with its **span direction** (a
//! plan unit vector, by default along its boundary's first edge) and `Pset_SlabCommon.LoadBearing`
//! true.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;

/// The element parameter holding the structural record.
pub const KEY: &str = "rxd:structural";

/// A wall's structural usage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WallUsage {
    #[default]
    NonBearing,
    Bearing,
    Shear,
    StructuralCombined,
}

impl WallUsage {
    pub fn label(self) -> &'static str {
        match self {
            WallUsage::NonBearing => "Non-bearing",
            WallUsage::Bearing => "Bearing",
            WallUsage::Shear => "Shear",
            WallUsage::StructuralCombined => "Structural Combined",
        }
    }
}

/// The structural record of a wall or floor.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Record {
    /// Walls: the usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<WallUsage>,
    /// Floors: the span direction in plan (a unit vector).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<[f64; 2]>,
}

pub fn record(e: &Element) -> Option<Record> {
    serde_json::from_value(e.param(KEY)?.clone()).ok()
}

/// A wall's structural usage (non-bearing when it has none).
pub fn wall_usage(e: &Element) -> WallUsage {
    record(e).and_then(|r| r.usage).unwrap_or_default()
}

/// Whether a floor is structural.
pub fn is_structural_floor(e: &Element) -> bool {
    crate::floors::slab_recipe(e).is_some() && record(e).is_some_and(|r| r.span.is_some())
}

/// A structural floor's span direction.
pub fn span(e: &Element) -> Option<[f64; 2]> {
    record(e)?.span
}

fn resolve(
    store: &Store,
    refs: &[ElementRef],
    ok: fn(&Element) -> bool,
    what: &str,
) -> Result<Vec<ElementId>> {
    refs.iter()
        .map(|r| {
            let id = store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
            if !ok(store.get(id).expect("resolved")) {
                return Err(Error::new(
                    "structural.wrong_element",
                    format!("element {id} is not a {what}"),
                ));
            }
            Ok(id)
        })
        .collect()
}

fn bearing(value: bool) -> Value {
    json!({ "type": "IfcBoolean", "value": value })
}

/// Sets a wall's usage: its record, PredefinedType and LoadBearing.
pub(crate) fn apply_wall_usage(
    txn: &mut crate::txn::Transaction<'_>,
    id: ElementId,
    usage: WallUsage,
) -> Result<()> {
    txn.modify(id, |e| {
        if usage == WallUsage::NonBearing {
            e.params.remove(KEY);
            e.params.insert("@PredefinedType".into(), "STANDARD".into());
            e.params
                .insert("Pset_WallCommon.LoadBearing".into(), bearing(false));
        } else {
            let r = Record {
                usage: Some(usage),
                span: None,
            };
            e.params
                .insert(KEY.into(), serde_json::to_value(r).expect("serialises"));
            let predefined = if usage == WallUsage::Shear {
                "SHEAR"
            } else {
                "SOLIDWALL"
            };
            e.params.insert("@PredefinedType".into(), predefined.into());
            e.params
                .insert("Pset_WallCommon.LoadBearing".into(), bearing(true));
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallUsageParams {
    pub wall_ids: Vec<ElementRef>,
    pub usage: WallUsage,
}

/// Sets walls' structural usage (`wall.set_structural_usage`, STR-WALL-001).
pub(crate) fn set_wall_usage(
    txn: &mut crate::txn::Transaction<'_>,
    p: WallUsageParams,
) -> Result<()> {
    let ids = resolve(
        txn.store(),
        &p.wall_ids,
        |e| crate::walls::wall_recipe(e).is_some(),
        "Reviera wall",
    )?;
    for id in ids {
        apply_wall_usage(txn, id, p.usage)?;
    }
    Ok(())
}

/// A floor's first boundary edge, as a unit vector.
fn first_edge(store: &Store, e: &Element) -> Option<[f64; 2]> {
    let g = crate::floors::regenerate(store, e).ok()?;
    let ring = &g.profiles.first()?.outer;
    let (a, b) = (ring.first()?, ring.get(1)?);
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]);
    (l > 1e-9).then(|| [d[0] / l, d[1] / l])
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FloorStructuralParams {
    pub floor_ids: Vec<ElementRef>,
    pub structural: bool,
}

/// Makes floors structural or not (`floor.set_structural`, STR-FLOOR-001); a new structural floor
/// spans along its boundary's first edge.
pub(crate) fn set_floor_structural(
    txn: &mut crate::txn::Transaction<'_>,
    p: FloorStructuralParams,
) -> Result<()> {
    let store = txn.store().clone();
    let ids = resolve(
        &store,
        &p.floor_ids,
        |e| {
            matches!(
                crate::floors::slab_recipe(e),
                Some((crate::floors::SlabKind::Floor, _))
            )
        },
        "Reviera floor",
    )?;
    for id in ids {
        let e = store.get(id).expect("resolved");
        let span = if p.structural {
            Some(
                span(e)
                    .or_else(|| first_edge(&store, e))
                    .unwrap_or([1.0, 0.0]),
            )
        } else {
            None
        };
        txn.modify(id, |e| {
            match span {
                Some(s) => {
                    let r = Record {
                        usage: None,
                        span: Some(s),
                    };
                    e.params
                        .insert(KEY.into(), serde_json::to_value(r).expect("serialises"));
                }
                None => {
                    e.params.remove(KEY);
                }
            }
            e.params.insert(
                "Pset_SlabCommon.LoadBearing".into(),
                bearing(span.is_some()),
            );
        })?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpanParams {
    pub floor_id: ElementRef,
    /// Along this boundary edge (0 the first, in the order of the floor's outline) …
    pub edge_index: Option<usize>,
    /// … or along this plan vector.
    pub vector: Option<[f64; 2]>,
}

/// Sets a structural floor's span direction (`floor.set_span_direction`); a floor that is not
/// structural becomes structural.
pub(crate) fn set_span(txn: &mut crate::txn::Transaction<'_>, p: SpanParams) -> Result<()> {
    let store = txn.store().clone();
    let id = resolve(
        &store,
        std::slice::from_ref(&p.floor_id),
        |e| {
            matches!(
                crate::floors::slab_recipe(e),
                Some((crate::floors::SlabKind::Floor, _))
            )
        },
        "Reviera floor",
    )?[0];
    let e = store.get(id).expect("resolved");
    let d = match (p.edge_index, p.vector) {
        (_, Some(v)) => v,
        (Some(k), None) => {
            let g = crate::floors::regenerate(&store, e)?;
            let ring = &g
                .profiles
                .first()
                .ok_or_else(|| Error::new("floor.empty", "the floor has no boundary"))?
                .outer;
            if k >= ring.len() {
                return Err(Error::new(
                    "floor.no_edge",
                    format!("the floor's outline has {} edges", ring.len()),
                ));
            }
            let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
            [b[0] - a[0], b[1] - a[1]]
        }
        (None, None) => {
            return Err(Error::new(
                "floor.no_span",
                "give an edge_index or a vector",
            ));
        }
    };
    let l = d[0].hypot(d[1]);
    if l < 1e-9 {
        return Err(Error::new(
            "floor.no_span",
            "the span direction has no length",
        ));
    }
    let r = Record {
        usage: None,
        span: Some([d[0] / l, d[1] / l]),
    };
    txn.modify(id, |e| {
        e.params
            .insert(KEY.into(), serde_json::to_value(r).expect("serialises"));
        e.params
            .insert("Pset_SlabCommon.LoadBearing".into(), bearing(true));
    })
}

// ---- Structural Settings (STR-SET-001) ----

/// The project parameter holding the structural settings.
pub const SETTINGS_KEY: &str = "rxd:structural_settings";

/// How braces are drawn in Coarse plans.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BraceSymbol {
    /// A line parallel to the brace, offset to one side.
    #[default]
    ParallelLine,
    None,
}

/// The project's structural settings (paper lengths in millimetres; model lengths in metres).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Settings {
    /// Symbolic representation: the brace symbol in Coarse plans and its offset (paper mm).
    pub brace_symbol: BraceSymbol,
    pub brace_offset: f64,
    /// The kicker brace symbol (a brace with one end on a beam's span).
    pub kicker_symbol: BraceSymbol,
    /// The gap left at joined ends of beams, braces and truss members in Coarse views (paper mm).
    pub coarse_cutback: f64,
    /// Point-to-point steel (STR-BEAM-003): steel members are not shortened where they join.
    pub point_to_point_steel: bool,
    /// Analytical model: automatic checks.
    pub member_supports_check: bool,
    pub consistency_check: bool,
    pub circular_references_check: bool,
    /// The consistency criteria checked (STR-AM-007): C1–C7 by default, C8 off.
    pub consistency_criteria: Vec<String>,
    /// Analytical tolerances (metres).
    pub support_distance: f64,
    pub analytical_distance: f64,
    pub node_merge_distance: f64,
    pub alignment_distance: f64,
    /// Boundary condition symbols' spacing along lines and areas (metres).
    pub boundary_spacing: f64,
    /// Load display: arrow length (paper mm) per kN, per kN/m, per kN/m² and per kN·m; labels.
    pub force_scale: f64,
    pub linear_force_scale: f64,
    pub area_force_scale: f64,
    pub moment_scale: f64,
    pub label_loads: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            brace_symbol: BraceSymbol::ParallelLine,
            brace_offset: 2.0,
            kicker_symbol: BraceSymbol::ParallelLine,
            coarse_cutback: 3.0,
            point_to_point_steel: false,
            member_supports_check: false,
            consistency_check: false,
            circular_references_check: true,
            consistency_criteria: ["C1", "C2", "C3", "C4", "C5", "C6", "C7"]
                .map(String::from)
                .to_vec(),
            support_distance: 0.3,
            analytical_distance: 0.15,
            node_merge_distance: 0.005,
            alignment_distance: 0.3,
            boundary_spacing: 1.0,
            force_scale: 1.0,
            linear_force_scale: 1.0,
            area_force_scale: 1.0,
            moment_scale: 1.0,
            label_loads: true,
        }
    }
}

impl Settings {
    /// The rules the dialog enforces: offsets and cutbacks 0–100 mm, tolerances above zero and at
    /// most 2 m, the node merge distance below the support distance, spacing and scales above zero.
    pub fn check(&self) -> std::result::Result<(), (&'static str, String)> {
        for (k, v) in [
            ("brace_offset", self.brace_offset),
            ("coarse_cutback", self.coarse_cutback),
        ] {
            if !(0.0..=100.0).contains(&v) {
                return Err((k, format!("`{k}` is 0 to 100 mm")));
            }
        }
        for (k, v) in [
            ("support_distance", self.support_distance),
            ("analytical_distance", self.analytical_distance),
            ("node_merge_distance", self.node_merge_distance),
            ("alignment_distance", self.alignment_distance),
        ] {
            if !(v > 0.0 && v <= 2.0) {
                return Err((k, format!("`{k}` is above 0 and at most 2000 mm")));
            }
        }
        if let Some(c) = self
            .consistency_criteria
            .iter()
            .find(|c| !crate::analytical_checks::CRITERIA.contains(&c.as_str()))
        {
            return Err((
                "consistency_criteria",
                format!("`{c}` is not a consistency criterion (C1 to C8)"),
            ));
        }
        if self.node_merge_distance >= self.support_distance {
            return Err((
                "node_merge_distance",
                "the node merge distance must be smaller than the support distance".into(),
            ));
        }
        for (k, v) in [
            ("boundary_spacing", self.boundary_spacing),
            ("force_scale", self.force_scale),
            ("linear_force_scale", self.linear_force_scale),
            ("area_force_scale", self.area_force_scale),
            ("moment_scale", self.moment_scale),
        ] {
            if v <= 0.0 {
                return Err((k, format!("`{k}` must be above 0")));
            }
        }
        Ok(())
    }
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// The model's structural settings (the defaults where none are stored).
pub fn settings(store: &Store) -> Settings {
    project(store)
        .and_then(|p| p.param(SETTINGS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSettingsParams {
    /// One setting (a field of `structural_settings.get`) …
    pub key: Option<String>,
    pub value: Option<Value>,
    /// … or several at once.
    pub values: Option<serde_json::Map<String, Value>>,
}

/// Changes structural settings (`structural_settings.set`): the same rules as the dialog; an
/// error names the key and the allowed range. Several values are one undo step.
pub(crate) fn set_settings(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetSettingsParams,
) -> Result<()> {
    let store = txn.store().clone();
    let id = project(&store)
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let mut v = serde_json::to_value(settings(&store)).expect("serialises");
    let mut changes = p.values.unwrap_or_default();
    if let Some(k) = p.key {
        changes.insert(k, p.value.unwrap_or(Value::Null));
    }
    let units = crate::units::settings_of(&store);
    for (k, value) in changes {
        let Some(slot) = v.get_mut(&k) else {
            return Err(Error::new(
                "structural_settings.unknown_key",
                format!("there is no structural setting `{k}`"),
            ));
        };
        // Model lengths accept units ("300 mm").
        let value = match (&value, slot.is_f64()) {
            (Value::String(s), true) if k.ends_with("_distance") || k == "boundary_spacing" => {
                Value::from(crate::units::parse(s, "length", &units)?)
            }
            (Value::String(s), true) => Value::from(
                s.trim()
                    .trim_end_matches("mm")
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| {
                        Error::new("structural_settings.invalid", format!("`{k}` is a number"))
                    })?,
            ),
            _ => value,
        };
        *slot = value;
    }
    let s: Settings = serde_json::from_value(v).map_err(|e| {
        Error::new(
            "structural_settings.invalid",
            format!("a setting has the wrong kind of value: {e}"),
        )
    })?;
    s.check().map_err(|(k, m)| {
        Error::new("structural_settings.out_of_range", m).with_details(json!({ "key": k }))
    })?;
    let value = serde_json::to_value(&s).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(SETTINGS_KEY.into(), value);
    })
}
