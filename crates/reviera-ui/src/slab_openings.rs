// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Openings in plans (ARC-OPEN-002/003/004, ARC-STAIR-007): each opening in a floor, ceiling or
//! roof of the plan's level, and each shaft through the level, is drawn as its outline crossed
//! corner to corner, the usual symbol for an opening; a wall opening in a wall the plan cuts as
//! its outline. Clicking the outline or the cross selects it; a selected opening shows the
//! footprint grips (corners and edge middles), which drag it (`opening.set_loops`).

use reviera_core::ElementId;
use reviera_render::plan::Lines;

use super::Gui;

const NEAR_PX: f32 = 8.0;

impl Gui {
    /// The openings drawn in plan `index`, with their segments: the vertical openings of the
    /// floors, ceilings and roofs on its level, the shafts through its level (outline crossed
    /// corner to corner), and the wall openings in walls the plan cuts (their outline).
    fn plan_openings(&self, index: usize) -> Vec<(ElementId, Vec<[[f64; 2]; 2]>)> {
        let v = &self.views[index];
        let Some(plan) = v.plan.as_ref() else {
            return Vec::new();
        };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store()) else {
            return Vec::new();
        };
        let Some(level) = store.get(plan.level).map(|l| l.guid().to_string()) else {
            return Vec::new();
        };
        let crossed = |loops: &[Vec<[f64; 2]>]| {
            let mut segments = Vec::new();
            for ring in loops {
                let n = ring.len();
                for i in 0..n {
                    segments.push([ring[i], ring[(i + 1) % n]]);
                }
                // The cross: between the extreme corners of the loop.
                let (lo, hi) = ring
                    .iter()
                    .fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
                        (
                            [lo[0].min(p[0]), lo[1].min(p[1])],
                            [hi[0].max(p[0]), hi[1].max(p[1])],
                        )
                    });
                if n == 4 {
                    segments.push([ring[0], ring[2]]);
                    segments.push([ring[1], ring[3]]);
                } else if hi[0] > lo[0] {
                    segments.push([lo, hi]);
                    segments.push([[lo[0], hi[1]], [hi[0], lo[1]]]);
                }
            }
            segments
        };
        let mut out: Vec<(ElementId, Vec<[[f64; 2]; 2]>)> = store
            .iter()
            .filter_map(|e| {
                let r = reviera_core::slab_openings::opening_recipe(e)?;
                let host = reviera_core::GlobalId::parse(&r.host)
                    .and_then(|g| store.find_by_guid(&g))
                    .and_then(|h| store.get(h))?;
                let host_level = match reviera_core::floors::slab_recipe(host) {
                    Some((_, f)) => f.level,
                    None => reviera_core::roofs::roof_recipe(host)?.level,
                };
                (host_level == level).then(|| (e.id(), crossed(&r.loops)))
            })
            .collect();
        out.extend(
            reviera_core::slab_openings::shafts_at(store, plan.elevation)
                .into_iter()
                .map(|(id, r)| (id, crossed(&r.loops))),
        );
        let cut = plan.range.cut;
        for o in reviera_core::wall_openings::all(store) {
            if cut < o.wall.base_z || cut > o.wall.base_z + o.wall.height {
                continue;
            }
            let ring = o.outline();
            let n = ring.len();
            out.push((o.id, (0..n).map(|i| [ring[i], ring[(i + 1) % n]]).collect()));
        }
        out
    }

    /// The opening symbols of plan `index` (the selected ones highlighted).
    pub(crate) fn opening_lines(&mut self, index: usize) -> Vec<Lines> {
        let selected = self.selection();
        let z = self.views[index]
            .plan
            .as_ref()
            .map_or(0.0, |p| p.range.cut as f32 + 0.002);
        let mut normal = Lines {
            color: reviera_render::plan::PROJECTION_LINE,
            width: 0.25,
            segments: Vec::new(),
        };
        let mut lit = Lines {
            color: reviera_render::HIGHLIGHT,
            width: 0.35,
            segments: Vec::new(),
        };
        for (id, segments) in self.plan_openings(index) {
            let target = if selected.contains(&id) {
                &mut lit
            } else {
                &mut normal
            };
            target.segments.extend(
                segments
                    .iter()
                    .map(|[a, b]| [[a[0] as f32, a[1] as f32, z], [b[0] as f32, b[1] as f32, z]]),
            );
        }
        vec![normal, lit]
    }

    /// The opening whose outline or cross is under pixel (`x`, `y`) of the active plan.
    pub(crate) fn opening_at(&self, x: i32, y: i32) -> Option<ElementId> {
        let index = self.views.iter().position(|v| Some(v.id) == self.active)?;
        let z = self.views[index].plan.as_ref()?.range.cut as f32;
        self.plan_openings(index)
            .into_iter()
            .map(|(id, segments)| {
                let d = segments
                    .iter()
                    .map(|s| self.pixel_distance(x, y, *s, z))
                    .fold(f32::MAX, f32::min);
                (id, d)
            })
            .filter(|(_, d)| *d <= NEAR_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
}
