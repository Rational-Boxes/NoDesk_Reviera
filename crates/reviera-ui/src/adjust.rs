// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Adjust mode (STR-AM-008): moving analytical nodes by hand in a 3D view, without moving any
//! physical element. Click a node, then where it goes: it stays at its height (or, after `Z`, on
//! its vertical; after `X` or `Y`, on that axis), snapping onto other nodes. Enter with a node
//! picked types the move in millimetres. The members using a node follow it in the preview.
//! Several moves make one session; Enter (with no node picked) finishes them as one undo step,
//! merging a node dropped on another, and Esc cancels them all.

use std::collections::BTreeMap;

use glam::Vec3;
use reviera_core::{ElementId, analytical};
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

const MOVED: [f32; 4] = [0.95, 0.75, 0.0, 1.0];
const RUBBER: [f32; 4] = [0.95, 0.4, 0.0, 1.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustAxis {
    /// In the node's horizontal plane.
    Plan,
    X,
    Y,
    Z,
}

/// An Adjust session: the moves so far, the node picked and where the cursor would put it.
#[derive(Debug, Clone)]
pub struct AdjustSession {
    moves: BTreeMap<ElementId, [f64; 3]>,
    current: Option<ElementId>,
    axis: AdjustAxis,
    target: Option<[f64; 3]>,
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

impl Gui {
    /// Analyze › Analytical › Adjust.
    pub(crate) fn start_adjust(&mut self) {
        let frame = self.frame;
        if self.active_view().is_none_or(|v| v.is_2d()) {
            return frame.set_status_text("Adjust works in 3D views: open one first", 0);
        }
        self.end_tool(&frame);
        self.end_modify();
        if !self.show_analytical {
            self.toggle_analytical();
        }
        self.adjust = Some(AdjustSession {
            moves: BTreeMap::new(),
            current: None,
            axis: AdjustAxis::Plan,
            target: None,
        });
        self.adjust_status();
    }

    fn adjust_status(&self) {
        let Some(a) = &self.adjust else { return };
        let axis = match a.axis {
            AdjustAxis::Plan => "at its height",
            AdjustAxis::X => "along x",
            AdjustAxis::Y => "along y",
            AdjustAxis::Z => "up or down",
        };
        let text = match a.current {
            None => format!(
                "Adjust: click an analytical node ({} moved so far). Enter finishes, Esc cancels.",
                a.moves.len()
            ),
            Some(n) => format!(
                "Adjust: click where node {n} goes ({axis}; X, Y or Z changes, snaps onto nodes); Enter types the move."
            ),
        };
        self.frame.set_status_text(&text, 0);
    }

    /// Where a node is now in the session.
    fn adjusted_point(&self, store: &reviera_core::Store, id: ElementId) -> Option<[f64; 3]> {
        let p = analytical::node_point(store, store.get(id)?)?;
        let d = self
            .adjust
            .as_ref()
            .and_then(|a| a.moves.get(&id).copied())
            .unwrap_or([0.0; 3]);
        Some(add(p, d))
    }

    /// Where the cursor at (`x`, `y`) puts the picked node: onto a node within 8 pixels, else on
    /// the axis or plane through the node.
    fn adjust_target(&self, x: i32, y: i32) -> Option<[f64; 3]> {
        let a = self.adjust.as_ref()?;
        let node = a.current?;
        let store = self.store_now()?;
        let p = self.adjusted_point(&store, node)?;
        let v = self.active_view()?;
        let size = v.panel.get_client_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let cam = self.camera();
        // Snap onto another node.
        let snap = analytical::nodes(&store)
            .into_iter()
            .filter(|(id, _)| *id != node)
            .filter_map(|(id, _)| {
                let q = self.adjusted_point(&store, id)?;
                let s = cam.project(Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32), w, h)?;
                let d = ((s.0 - x as f32).powi(2) + (s.1 - y as f32).powi(2)).sqrt();
                (d <= 8.0).then_some((q, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((q, _)) = snap {
            return Some(q);
        }
        let (o, d) = cam.ray(x as f32, y as f32, w, h);
        let (o, d) = (o.as_dvec3(), d.as_dvec3());
        let pv = glam::DVec3::from_array(p);
        match a.axis {
            AdjustAxis::Plan => {
                if d.z.abs() < 1e-9 {
                    return None;
                }
                let t = (pv.z - o.z) / d.z;
                (t > 0.0).then(|| {
                    let q = o + d * t;
                    [q.x, q.y, pv.z]
                })
            }
            axis => {
                // The point on the axis line nearest the ray.
                let u = match axis {
                    AdjustAxis::X => glam::DVec3::X,
                    AdjustAxis::Y => glam::DVec3::Y,
                    _ => glam::DVec3::Z,
                };
                let w0 = pv - o;
                let (b, dd, e) = (u.dot(d), d.dot(d), u.dot(w0));
                let f = d.dot(w0);
                let den = dd - b * b;
                if den.abs() < 1e-12 {
                    return None;
                }
                let s = (b * f - dd * e) / den;
                let q = pv + u * s;
                Some([q.x, q.y, q.z])
            }
        }
    }

    pub(crate) fn adjust_motion(&mut self, x: i32, y: i32) {
        let t = self.adjust_target(x, y);
        if let Some(a) = &mut self.adjust {
            a.target = t;
        }
        if let Some(i) = self.views.iter().position(|v| Some(v.id) == self.active) {
            self.update_inspection(i);
        }
    }

    pub(crate) fn adjust_click(&mut self, x: i32, y: i32) {
        let Some(a) = self.adjust.clone() else { return };
        match a.current {
            None => {
                let size = match self.active_view() {
                    Some(v) => v.panel.get_client_size(),
                    None => return,
                };
                let picked = self
                    .analytical_pick(x, y, size.width as f32, size.height as f32)
                    .filter(|id| {
                        self.store_now()
                            .and_then(|s| s.get(*id).and_then(analytical::node_recipe))
                            .is_some()
                    });
                match picked {
                    Some(n) => {
                        if let Some(a) = &mut self.adjust {
                            a.current = Some(n);
                            a.target = None;
                        }
                    }
                    None => self.frame.set_status_text("No analytical node there", 0),
                }
                if picked.is_some() {
                    self.adjust_status();
                }
            }
            Some(node) => {
                let Some(target) = self.adjust_target(x, y) else { return };
                self.adjust_by(node, target, true);
            }
        }
    }

    /// Moves the picked node to `to` (absolute) or by `to` (a delta).
    fn adjust_by(&mut self, node: ElementId, to: [f64; 3], absolute: bool) {
        let Some(store) = self.store_now() else { return };
        let Some(now) = self.adjusted_point(&store, node) else { return };
        let delta = if absolute {
            [to[0] - now[0], to[1] - now[1], to[2] - now[2]]
        } else {
            to
        };
        if let Some(a) = &mut self.adjust {
            let d = a.moves.entry(node).or_insert([0.0; 3]);
            *d = add(*d, delta);
            a.current = None;
            a.target = None;
        }
        for i in 0..self.views.len() {
            self.update_inspection(i);
        }
        self.adjust_status();
    }

    /// Keys in Adjust: X, Y, Z choose the axis; Enter types a move, or finishes.
    pub(crate) fn adjust_key(&mut self, key: i32) -> bool {
        let Some(a) = self.adjust.clone() else { return false };
        match key {
            88 | 120 => self.set_adjust_axis(AdjustAxis::X),
            89 | 121 => self.set_adjust_axis(AdjustAxis::Y),
            90 | 122 => self.set_adjust_axis(AdjustAxis::Z),
            WXK_RETURN | WXK_NUMPAD_ENTER => match a.current {
                Some(node) => {
                    let d = TextEntryDialog::builder(&self.frame, "Move by dx, dy, dz (mm):", "Adjust")
                        .with_default_value("0, 0, 0")
                        .build();
                    if d.show_modal() == ID_OK {
                        let v: Vec<f64> = d
                            .get_value()
                            .unwrap_or_default()
                            .split(',')
                            .filter_map(|s| s.trim().parse::<f64>().ok())
                            .collect();
                        if v.len() == 3 {
                            self.adjust_by(node, [v[0] / 1000.0, v[1] / 1000.0, v[2] / 1000.0], false);
                        } else {
                            self.frame.set_status_text("Type three numbers: dx, dy, dz", 0);
                        }
                    }
                }
                None => self.finish_adjust(),
            },
            _ => return false,
        }
        true
    }

    fn set_adjust_axis(&mut self, axis: AdjustAxis) {
        if let Some(a) = &mut self.adjust {
            a.axis = if a.axis == axis { AdjustAxis::Plan } else { axis };
        }
        self.adjust_status();
    }

    /// Finish: every move as one undo step, nodes dropped on others merged.
    pub(crate) fn finish_adjust(&mut self) {
        let frame = self.frame;
        let Some(a) = self.adjust.take() else { return };
        let moves: Vec<serde_json::Value> = a
            .moves
            .iter()
            .filter(|(_, d)| d.iter().any(|v| v.abs() > 1e-9))
            .map(|(id, d)| json!([id, d]))
            .collect();
        if moves.is_empty() {
            frame.set_status_text("Adjust ended; nothing moved", 0);
        } else {
            match self.run("analytical_node.move", json!({ "moves": moves, "merge": true })) {
                Ok(_) => frame.set_status_text(
                    &format!("{} node(s) adjusted (Edit › Undo undoes them together)", moves.len()),
                    0,
                ),
                Err(e) => error(&frame, &format!("The adjustment was not made:\n{e}")),
            }
        }
        self.refresh_scene(&frame);
        for i in 0..self.views.len() {
            self.update_inspection(i);
        }
    }

    /// Esc: the session's moves are dropped.
    pub(crate) fn cancel_adjust(&mut self) -> bool {
        if self.adjust.take().is_none() {
            return false;
        }
        for i in 0..self.views.len() {
            self.update_inspection(i);
        }
        true
    }

    /// The preview: members using moved nodes at their new places, and a line from the picked
    /// node to where the cursor puts it.
    pub(crate) fn adjust_lines(&self, index: usize) -> Vec<Lines> {
        let Some(a) = &self.adjust else { return Vec::new() };
        if self.views[index].is_2d() {
            return Vec::new();
        }
        let Some(store) = self.store_now() else { return Vec::new() };
        let f = |p: [f64; 3]| [p[0] as f32, p[1] as f32, p[2] as f32];
        let at = |id: ElementId| self.adjusted_point(&store, id);
        let mut moved = Vec::new();
        for (_, m) in analytical::members(&store) {
            if m.nodes.iter().any(|n| a.moves.contains_key(n))
                && let (Some(p), Some(q)) = (at(m.nodes[0]), at(m.nodes[1]))
            {
                moved.push([f(p), f(q)]);
            }
        }
        let mut rubber = Vec::new();
        if let (Some(n), Some(t)) = (a.current, a.target)
            && let Some(p) = at(n)
        {
            rubber.push([f(p), f(t)]);
            for (_, m) in analytical::members(&store) {
                for k in 0..2 {
                    if m.nodes[k] == n
                        && let Some(o) = at(m.nodes[1 - k])
                    {
                        rubber.push([f(o), f(t)]);
                    }
                }
            }
            // A cross at the target.
            let s = 0.2;
            let t = f(t);
            rubber.push([[t[0] - s, t[1], t[2]], [t[0] + s, t[1], t[2]]]);
            rubber.push([[t[0], t[1] - s, t[2]], [t[0], t[1] + s, t[2]]]);
        }
        vec![
            Lines {
                color: MOVED,
                width: 0.7,
                segments: moved,
            },
            Lines {
                color: RUBBER,
                width: 0.5,
                segments: rubber,
            },
        ]
    }
}
