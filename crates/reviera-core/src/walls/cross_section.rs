// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Slanted and tapered walls (spec Ch 3 §3.3.10, ARC-WALL-005): walls whose faces are not
//! vertical.
//!
//! - **Slanted**: the whole cross-section leans by *Angle From Vertical*; at height `h` above the
//!   base every point moves `h · tan α` across the wall, toward the right of its drawing direction
//!   for a positive angle (the interior when the wall is not flipped).
//! - **Tapered**: the exterior and interior faces lean inward by their own angles; the wall
//!   thins with height, its type's variable layer taking the difference, which must stay thicker
//!   than zero.
//!
//! The wall's body (joined and cut) is sheared accordingly; in IFC it is a B-rep.
//!
//! Not built yet: in-canvas angle controls, inserts kept vertical (they lean with the wall),
//! Width Measured At top, sweeps' orientation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bodies::Mesh;
use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::ElementRef;
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::{RECIPE_KEY, WallGeometry, WallRecipe, wall_recipe};

/// A wall's cross-section when it is not vertical.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum CrossSection {
    /// Angle From Vertical (radians).
    Slanted { angle: f64 },
    /// Exterior and interior face angles (radians; positive leans the face's top inward).
    Tapered { exterior: f64, interior: f64 },
}

/// The largest lean (89°).
const MAX: f64 = 89.0 * std::f64::consts::PI / 180.0;

/// Moves a wall body's points for its cross-section: `g` is its geometry, `flip` whether it is
/// flipped (the exterior then on its right).
pub fn shear(mesh: &Mesh, g: &WallGeometry, flip: bool, cs: CrossSection) -> Mesh {
    let n = g.normal();
    let ext = if flip { -1.0 } else { 1.0 };
    let half = g.width / 2.0;
    let points = mesh
        .0
        .iter()
        .map(|p| {
            let h = p[2] - g.base_z;
            // Across the wall from its location line, toward its exterior.
            let s = ((p[0] - g.start[0]) * n[0] + (p[1] - g.start[1]) * n[1]) * ext;
            let ds = match cs {
                // Right of the drawing direction is −n: toward the exterior that is −ext.
                CrossSection::Slanted { angle } => -h * angle.tan() * ext,
                CrossSection::Tapered { exterior, interior } => {
                    let w = g.width.max(1e-9);
                    // The exterior face moves in by h·tan αₑ, the interior one by h·tan αᵢ, the
                    // points between them in proportion.
                    let t = (s + half) / w;
                    -h * exterior.tan() * t + h * interior.tan() * (1.0 - t)
                }
            };
            // `ds` is toward the exterior: along the normal times `ext`.
            let move_by = ds * ext;
            [p[0] + n[0] * move_by, p[1] + n[1] * move_by, p[2]]
        })
        .collect();
    (points, mesh.1.clone())
}

/// The variable layer's thickness at the top of a tapered wall, if its type has one.
fn variable_at_top(store: &Store, e: &Element, height: f64, ext: f64, int: f64) -> Option<f64> {
    let t = e
        .param("@type")
        .and_then(serde_json::Value::as_str)
        .and_then(crate::id::GlobalId::parse)
        .and_then(|g| store.find_by_guid(&g))
        .and_then(|t| store.get(t))?;
    let c = crate::compound::of_wall_type(t)?;
    let v = c.layers.iter().find(|l| l.variable)?;
    Some(v.thickness - height * (ext.tan() + int.tan()))
}

/// Checks a cross-section against a wall.
pub fn check(store: &Store, e: &Element, cs: CrossSection) -> Result<()> {
    let r = wall_recipe(e).expect("a wall");
    if !r.through.is_empty() {
        return Err(Error::new(
            "wall.cross_section_curved",
            "slanted and tapered walls are straight for now",
        ));
    }
    match cs {
        CrossSection::Slanted { angle } => {
            if angle.is_nan() || angle.abs() >= MAX {
                return Err(Error::new("wall.angle", "the angle must be within ±89°"));
            }
        }
        CrossSection::Tapered { exterior, interior } => {
            if [exterior, interior]
                .iter()
                .any(|a| a.is_nan() || a.abs() >= MAX)
            {
                return Err(Error::new("wall.angle", "the angles must be within ±89°"));
            }
            let g = crate::walls::regenerate(store, e)?;
            match variable_at_top(store, e, g.height, exterior, interior) {
                None => {
                    return Err(Error::new(
                        "wall.tapered_needs_variable",
                        "a tapered wall's type needs a variable layer (Edit Assembly)",
                    ));
                }
                Some(t) if t <= 0.0 => {
                    return Err(Error::new(
                        "wall.taper_negative",
                        format!(
                            "Variable layer thickness would reach zero ({:.4} m at the top)",
                            t
                        ),
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Vertical,
    Slanted,
    Tapered,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub wall_ids: Vec<ElementRef>,
    pub mode: Mode,
    /// Slanted: Angle From Vertical (radians or "10°").
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub angle: Option<f64>,
    /// Tapered: the exterior and interior face angles.
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub exterior_angle: Option<f64>,
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub interior_angle: Option<f64>,
}

/// `wall.set_cross_section`.
pub(crate) fn set(txn: &mut Transaction<'_>, p: SetParams) -> Result<()> {
    let store = txn.store().clone();
    for r in &p.wall_ids {
        let id = store
            .resolve(r)
            .filter(|id| store.get(*id).and_then(wall_recipe).is_some())
            .ok_or_else(|| Error::new("element.not_found", format!("no Reviera wall {r:?}")))?;
        let e = store.get(id).expect("resolved");
        let mut recipe: WallRecipe = wall_recipe(e).expect("checked");
        let cs = match p.mode {
            Mode::Vertical => None,
            Mode::Slanted => Some(CrossSection::Slanted {
                angle: p.angle.unwrap_or(0.0),
            }),
            Mode::Tapered => Some(CrossSection::Tapered {
                exterior: p.exterior_angle.unwrap_or(0.0),
                interior: p.interior_angle.unwrap_or(0.0),
            }),
        };
        if let Some(cs) = cs {
            check(&store, e, cs)?;
        }
        recipe.cross_section = cs;
        let value = serde_json::to_value(&recipe).expect("serialises");
        txn.modify(id, |el| {
            el.params.insert(RECIPE_KEY.into(), value);
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slanted_wall_leans_its_top() {
        let g = WallGeometry {
            start: [0.0, 0.0],
            end: [10.0, 0.0],
            base_z: 0.0,
            height: 3.0,
            width: 0.2,
        };
        let mesh = (vec![[5.0, 0.0, 3.0], [5.0, 0.0, 0.0]], vec![]);
        let m = shear(
            &mesh,
            &g,
            false,
            CrossSection::Slanted {
                angle: 10f64.to_radians(),
            },
        );
        // The top moves 3 · tan 10° = 0.529 m to the right of +x (−y).
        assert!((m.0[0][1] + 0.528_985).abs() < 1e-5, "{:?}", m.0[0]);
        assert!(m.0[1][1].abs() < 1e-12);
    }
}
