// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Converting legacy zones into system-zones (MEP-ZONE-001): an `IfcZone` from another tool
//! becomes a system-zone whose region envelopes its spaces.
//!
//! - **Footprints**: a Reviera space's or room's region; an imported space's tessellated body
//!   seen from above (as saved last).
//! - **Region**: the convex hull of its spaces' footprints, on the level of most of them (an
//!   imported space is on the highest level at or below its floor), as tall as the tallest.
//! - **Set points**: `Pset_SpaceThermalRequirements` on the zone, else on its first space that
//!   has them.
//! - Every zone is replaced in one undo step (`system_zone.apply_convert`).

use std::collections::{BTreeMap, HashSet};

use reviera_core::system_zones as zones;
use reviera_core::{Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Result, Store};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConvertParams {
    /// Legacy `IfcZone`s; every one in the model when empty.
    #[serde(default)]
    pub zone_ids: Vec<ElementRef>,
}

pub(crate) struct ConvertLegacy;

/// A space's footprint: its level, plan outline, and floor and top elevations.
struct Footprint {
    level: ElementId,
    ring: Vec<[f64; 2]>,
    top: f64,
}

/// The highest level at or below `z` (else the lowest).
fn level_at(store: &Store, z: f64) -> Option<ElementId> {
    let levels = reviera_core::levels::list(store);
    levels.iter().rev().find(|l| l.elevation <= z + 0.05).or(levels.first()).map(|l| l.id)
}

fn recipe_footprint(store: &Store, id: ElementId) -> Option<Footprint> {
    let e = store.get(id)?;
    let r = reviera_core::mep_spaces::space_recipe(e).or_else(|| reviera_core::rooms::room_recipe(e))?;
    let g = reviera_core::rooms::regenerate_recipe(store, &r).ok()?;
    let ring = g.region.as_ref()?.outer_ring();
    let level = store.find_by_guid(&reviera_core::GlobalId::parse(&r.level)?)?;
    Some(Footprint { level, ring, top: g.z_top })
}

impl HostCommand for ConvertLegacy {
    type Host = App;
    type Params = ConvertParams;
    type Output = Vec<ElementId>;
    const META: CommandMeta = CommandMeta {
        name: "system_zone.convert_legacy",
        version: 1,
        summary: "Convert legacy zones",
        description: "Turns legacy zones (`zone_ids`, IfcZones from imported models; every one when empty) into system-zones (MEP-ZONE-001): each region envelopes its spaces (the convex hull of their footprints, on the level of most of them, as tall as the tallest), named as the zone, with the heating and cooling set points and humidity limits of its Pset_SpaceThermalRequirements (on the zone, else on its first space that has them). The legacy zones are removed. One undo step. Returns the system-zones.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: ConvertParams) -> Result<Vec<ElementId>> {
        let doc_id = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let open = app.document(doc_id).expect("active");
        let store = open.doc.store().clone();
        let legacy: Vec<ElementId> = if p.zone_ids.is_empty() {
            store.iter().filter(|e| zones::is_legacy(e)).map(|e| e.id()).collect()
        } else {
            p.zone_ids
                .iter()
                .map(|r| {
                    let id = store.resolve(r).ok_or_else(|| bad("element.not_found", "no such zone"))?;
                    if !store.get(id).is_some_and(zones::is_legacy) {
                        return Err(bad("system_zone.not_legacy", format!("element {id} is not a legacy zone (IfcZone)")));
                    }
                    Ok(id)
                })
                .collect::<Result<_>>()?
        };
        if legacy.is_empty() {
            return Err(bad("system_zone.no_legacy", "the model has no legacy zones to convert"));
        }
        // Spaces Reviera does not regenerate are measured from their IFC bodies.
        let mut members: BTreeMap<ElementId, Vec<ElementId>> = BTreeMap::new();
        let mut imported: HashSet<String> = HashSet::new();
        for z in &legacy {
            let ms: Vec<ElementId> = zones::members(&store, store.get(*z).expect("listed")).into_iter().filter(|m| store.get(*m).is_some_and(|e| e.class == "IfcSpace")).collect();
            for m in &ms {
                if recipe_footprint(&store, *m).is_none() {
                    imported.insert(store.get(*m).expect("resolved").guid().to_string());
                }
            }
            members.insert(*z, ms);
        }
        let meshes = if imported.is_empty() { Vec::new() } else { open.product_meshes(&imported)? };
        let footprint = |id: ElementId| -> Option<Footprint> {
            if let Some(f) = recipe_footprint(&store, id) {
                return Some(f);
            }
            let guid = store.get(id)?.guid().to_string();
            let m = meshes.iter().find(|m| m.global_id.as_ref().is_some_and(|g| g.as_str() == guid))?;
            let v = m.verts.as_chunks::<3>().0;
            let lo = v.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
            let top = v.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
            let ring = zones::hull(&v.iter().map(|p| [p[0], p[1]]).collect::<Vec<_>>());
            (ring.len() >= 3).then_some(Footprint { level: level_at(&store, lo)?, ring, top })
        };
        let mut out = Vec::new();
        for z in &legacy {
            let e = store.get(*z).expect("listed");
            let fps: Vec<Footprint> = members[z].iter().filter_map(|m| footprint(*m)).collect();
            if fps.is_empty() {
                return Err(bad(
                    "system_zone.no_spaces",
                    format!("the zone {} groups no spaces with a footprint, so its region cannot be found", e.name.as_deref().unwrap_or("(unnamed)")),
                ));
            }
            let mut count: BTreeMap<ElementId, usize> = BTreeMap::new();
            for f in &fps {
                *count.entry(f.level).or_default() += 1;
            }
            let level = count.iter().max_by_key(|(l, n)| (**n, std::cmp::Reverse(**l))).map(|(l, _)| *l).expect("some");
            let base = reviera_core::levels::elevation(store.get(level).expect("a level"));
            let pts: Vec<[f64; 2]> = fps.iter().flat_map(|f| f.ring.iter().copied()).collect();
            let top = fps.iter().map(|f| f.top).fold(f64::MIN, f64::max);
            out.push(json!({
                "legacy_id": z,
                "level_id": level,
                "boundary": zones::hull(&pts),
                "height": (top > base + 1e-3).then_some(top - base),
                "props": zones::legacy_props(&store, e),
            }));
        }
        let made = app.execute("system_zone.apply_convert", json!({ "zones": out }), caller)?;
        serde_json::from_value(made.result).map_err(|e| bad("system_zone.internal", e.to_string()))
    }
}
