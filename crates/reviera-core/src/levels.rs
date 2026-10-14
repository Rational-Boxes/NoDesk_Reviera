// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Levels (spec Ch 3 §3.2.1, ARC-LVL-001): named horizontal planes that host level-based
//! elements and define stories.
//!
//! A level is an `IfcBuildingStorey` element; its elevation is the `@Elevation` parameter in
//! metres. Walls refer to their base and top levels by GlobalId and regenerate from the current
//! elevations, so changing an elevation moves the walls on that level and keeps their offsets.
//! Plan views name and follow their level.
//!
//! Built so far: story levels with `level.create`, `level.set_elevation` and `level.rename`.
//! Reference levels (`IfcAnnotation`), level types and datum extents come with section and
//! elevation views.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::NewElement;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::plans::{self, CreatePlanParams, PlanKind};
use crate::store::Store;
use crate::views;
use crate::walls;

pub const CLASS: &str = "IfcBuildingStorey";
pub const ELEVATION_KEY: &str = "@Elevation";
/// The name of the first level of a project.
pub const FIRST_NAME: &str = "Level 1";

/// A level's elevation in metres (0 when the file does not state one).
pub fn elevation(e: &crate::Element) -> f64 {
    e.param(ELEVATION_KEY)
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

/// A vertical extent between levels, as walls, wall openings, shafts and columns store it: a
/// base level (GlobalId) and offset, and a top that is a level plus an offset or a height above
/// the base (`{"level": guid}` or `{"unconnected": height}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Extent {
    pub base_level: String,
    #[serde(default)]
    pub base_offset: f64,
    pub top: Value,
    #[serde(default)]
    pub top_offset: f64,
}

/// The elevation of the level with GlobalId `guid`.
pub fn elevation_of(store: &Store, guid: &str) -> Result<f64> {
    crate::id::GlobalId::parse(guid)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|id| store.get(id))
        .filter(|e| e.class == CLASS)
        .map(elevation)
        .ok_or_else(|| Error::new("level.missing", format!("level {guid} no longer exists")))
}

impl Extent {
    /// From command arguments: a base level and offset, and a top constraint and offset.
    pub fn from_args(
        store: &Store,
        base_level: &ElementRef,
        base_offset: f64,
        top: walls::TopConstraint,
        top_offset: f64,
    ) -> Result<Extent> {
        let base = store
            .get(resolve_level(store, base_level)?)
            .expect("resolved");
        let top = match top {
            walls::TopConstraint::Unconnected(h) if h > 0.0 => {
                serde_json::json!({ "unconnected": h })
            }
            walls::TopConstraint::Unconnected(_) => {
                return Err(Error::new(
                    "extent.non_positive_height",
                    "the height must be greater than 0",
                ));
            }
            walls::TopConstraint::Level(walls::ElementRefValue::Id(id)) => {
                let l = store
                    .get(resolve_level(store, &ElementRef::Id(id))?)
                    .expect("resolved");
                serde_json::json!({ "level": l.guid().as_str() })
            }
        };
        let e = Extent {
            base_level: base.guid().to_string(),
            base_offset,
            top,
            top_offset,
        };
        e.z(store)?;
        Ok(e)
    }

    /// The bottom and top elevations, refused when the top is not above the bottom.
    pub fn z(&self, store: &Store) -> Result<(f64, f64)> {
        let z0 = elevation_of(store, &self.base_level)? + self.base_offset;
        let z1 = if let Some(h) = self.top.get("unconnected").and_then(Value::as_f64) {
            z0 + h
        } else if let Some(g) = self.top.get("level").and_then(Value::as_str) {
            elevation_of(store, g)? + self.top_offset
        } else {
            return Err(Error::new(
                "extent.bad_top",
                "the top is neither a level nor a height",
            ));
        };
        if z1 <= z0 + 1e-6 {
            return Err(Error::new(
                "extent.non_positive_height",
                "the top is at or below the base",
            ));
        }
        Ok((z0, z1))
    }

    /// The top level (GlobalId), if the top follows one.
    pub fn top_level(&self) -> Option<&str> {
        self.top.get("level").and_then(Value::as_str)
    }

    /// Whether either end follows level `guid`.
    pub fn uses_level(&self, guid: &str) -> bool {
        self.base_level == guid || self.top_level() == Some(guid)
    }
}

/// One level, for listings.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct LevelInfo {
    pub id: ElementId,
    pub guid: String,
    pub name: String,
    /// Metres.
    pub elevation: f64,
}

/// The levels of a model, lowest first.
pub fn list(store: &Store) -> Vec<LevelInfo> {
    plans::levels(store)
        .into_iter()
        .filter_map(|(z, guid, id)| {
            Some(LevelInfo {
                id,
                guid,
                name: store.get(id)?.name.clone().unwrap_or_default(),
                elevation: z,
            })
        })
        .collect()
}

/// The name after `previous`: its trailing number or letters incremented, keeping zero padding
/// (`Level 2` → `Level 3`, `L09` → `L10`, `A` → `B`, `Z` → `AA`). A name without either gets
/// ` 2`.
pub fn next_name(previous: &str) -> String {
    let digits = previous
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .count();
    if digits > 0 {
        let (head, tail) = previous.split_at(previous.len() - digits);
        if let Ok(n) = tail.parse::<u64>() {
            return format!("{head}{:0width$}", n + 1, width = digits);
        }
    }
    let letters = previous
        .chars()
        .rev()
        .take_while(char::is_ascii_alphabetic)
        .count();
    // Only a short trailing code counts (`Level B`, `AB`); a word such as `Roof` does not.
    let code = &previous[previous.len() - letters..];
    let separated = letters == previous.len()
        || previous[..previous.len() - letters].ends_with([' ', '-', '_', '.']);
    if letters > 0 && letters <= 2 && separated && code.chars().all(|c| c.is_ascii_uppercase()) {
        let head = &previous[..previous.len() - letters];
        let mut chars: Vec<u8> = code.bytes().collect();
        let mut i = chars.len();
        loop {
            if i == 0 {
                chars.insert(0, b'A');
                break;
            }
            i -= 1;
            if chars[i] == b'Z' {
                chars[i] = b'A';
            } else {
                chars[i] += 1;
                break;
            }
        }
        return format!("{head}{}", String::from_utf8(chars).expect("ASCII"));
    }
    format!("{previous} 2")
}

/// The automatic name of a new level at `elevation`: the next name after the nearest level below
/// (or the lowest level), not used by another level.
fn auto_name(store: &Store, elevation: f64) -> String {
    let all = list(store);
    let previous = all
        .iter()
        .rev()
        .find(|l| l.elevation <= elevation)
        .or(all.first());
    let mut name = match previous {
        Some(l) if !l.name.is_empty() => next_name(&l.name),
        _ => FIRST_NAME.to_owned(),
    };
    while all.iter().any(|l| l.name == name) {
        name = next_name(&name);
    }
    name
}

fn resolve_level(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such level"))?;
    let e = store.get(id).expect("resolved");
    if e.class != CLASS {
        return Err(Error::new(
            "level.not_a_level",
            format!("element {id} ({}) is not a level", e.class),
        ));
    }
    Ok(id)
}

fn check_name(store: &Store, name: &str, except: Option<ElementId>) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::new("level.bad_name", "a level needs a name"));
    }
    if store
        .iter()
        .any(|e| e.class == CLASS && Some(e.id()) != except && e.name.as_deref() == Some(name))
    {
        return Err(Error::new(
            "level.name_taken",
            format!("another level is already named {name}"),
        ));
    }
    Ok(name.to_owned())
}

/// Checks that every wall based on or bounded by `level` still regenerates (its top above its
/// base), after the level moved.
pub(crate) fn check_walls(store: &Store, level: ElementId) -> Result<()> {
    let Some(guid) = store.get(level).map(|e| e.guid().to_string()) else {
        return Ok(());
    };
    for e in store.iter() {
        let Some(r) = walls::wall_recipe(e) else {
            continue;
        };
        let top = r.top.get("level").and_then(Value::as_str);
        if r.base_level != guid && top != Some(guid.as_str()) {
            continue;
        }
        if let Err(err) = walls::regenerate(store, e) {
            return Err(Error::new(
                "level.wall_height",
                format!(
                    "wall {} would not regenerate at this elevation: {}",
                    e.id(),
                    err.message
                ),
            ));
        }
    }
    Ok(())
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub elevation: f64,
    /// Omitted, the name continues the series of the level below (`Level 2` → `Level 3`).
    pub name: Option<String>,
    /// Also create a floor plan for the level (default on).
    #[serde(default = "yes")]
    pub make_plan_view: bool,
}

fn yes() -> bool {
    true
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    if !p.elevation.is_finite() {
        return Err(Error::new(
            "level.bad_elevation",
            "the elevation must be a number",
        ));
    }
    let store = txn.store().clone();
    let name = match &p.name {
        Some(n) => check_name(&store, n, None)?,
        None => auto_name(&store, p.elevation),
    };
    let id = txn.create(
        NewElement::new(CLASS)
            .name(name)
            .param(ELEVATION_KEY, p.elevation),
    )?;
    if p.make_plan_view {
        plans::create_plan(
            txn,
            CreatePlanParams {
                kind: PlanKind::FloorPlan,
                level_ids: vec![ElementRef::Id(id)],
                allow_duplicates: false,
                scale: None,
            },
        )?;
    }
    Ok(id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetElevationParams {
    pub level_id: ElementRef,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub elevation: f64,
}

pub(crate) fn set_elevation(
    txn: &mut crate::txn::Transaction<'_>,
    p: SetElevationParams,
) -> Result<()> {
    if !p.elevation.is_finite() {
        return Err(Error::new(
            "level.bad_elevation",
            "the elevation must be a number",
        ));
    }
    let id = resolve_level(txn.store(), &p.level_id)?;
    txn.modify(id, |e| {
        e.params.insert(ELEVATION_KEY.into(), p.elevation.into());
    })?;
    check_walls(txn.store(), id)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameParams {
    pub level_id: ElementRef,
    pub name: String,
    /// Also rename the level's plan views that carry its name (default on).
    #[serde(default = "yes")]
    pub rename_views: bool,
}

/// Renames a level and returns the views renamed with it.
pub(crate) fn rename(
    txn: &mut crate::txn::Transaction<'_>,
    p: RenameParams,
) -> Result<Vec<ElementId>> {
    let store = txn.store().clone();
    let id = resolve_level(&store, &p.level_id)?;
    let level = store.get(id).expect("resolved");
    let name = check_name(&store, &p.name, Some(id))?;
    let old = level.name.clone().unwrap_or_default();
    let guid = level.guid().to_string();
    txn.modify(id, |e| e.name = Some(name.clone()))?;
    let mut renamed = Vec::new();
    if !p.rename_views || old.is_empty() {
        return Ok(renamed);
    }
    // `Level 2` and its copies `Level 2 (2)`, … follow the level.
    let taken: Vec<String> = store
        .iter()
        .filter(|e| views::view_data(e).is_some())
        .filter_map(|e| e.name.clone())
        .collect();
    for e in store.iter() {
        let Some(d) = views::view_data(e) else {
            continue;
        };
        if d.level.as_deref() != Some(guid.as_str()) {
            continue;
        }
        let Some(current) = e.name.as_deref() else {
            continue;
        };
        let Some(suffix) = current
            .strip_prefix(old.as_str())
            .filter(|s| s.is_empty() || s.starts_with(" ("))
        else {
            continue;
        };
        let new = format!("{name}{suffix}");
        if taken.contains(&new) {
            continue;
        }
        txn.modify(e.id(), |v| v.name = Some(new))?;
        renamed.push(e.id());
    }
    Ok(renamed)
}

#[cfg(test)]
mod tests {
    use super::next_name;

    #[test]
    fn names_continue_their_series() {
        assert_eq!(next_name("Level 1"), "Level 2");
        assert_eq!(next_name("Level 9"), "Level 10");
        assert_eq!(next_name("L02"), "L03");
        assert_eq!(next_name("L09"), "L10");
        assert_eq!(next_name("A"), "B");
        assert_eq!(next_name("Level Z"), "Level AA");
        assert_eq!(next_name("Roof"), "Roof 2");
        assert_eq!(next_name("Ground floor"), "Ground floor 2");
    }
}
