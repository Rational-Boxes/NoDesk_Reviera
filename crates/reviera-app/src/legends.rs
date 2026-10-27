// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Legend drawings (DOC-VIEW-010): each legend component's type sample
//! (`reviera_core::legends::sample`) drawn in its direction and placed with its lower-left corner
//! at the component's point, with the legend's text notes. In plan the sample is cut at 1.2 m
//! (a door's swing drawn too); from the front, the back or in section it is turned into a view
//! frame like an elevation's or a section's and drawn with its hidden edges left out.

use reviera_core::legends::{self, Direction, PLAN_CUT, TypeKind};
use reviera_core::sections::SectionDef;
use reviera_core::{Caller, CommandMeta, Effect, Element, ElementId, ElementRef, Error, HostCommand, Result, Store, standards, views};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use reviera_render::plan::{Lines, PlanGraphics, Weights, feature_edges, plan_graphics, visible_edges};
use reviera_render::{RenderScene, SceneMesh};

use crate::output::PlanDrawing;

/// A legend component as drawn: its element and its drawing's extent in the legend (metres).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Item {
    pub id: ElementId,
    pub lo: [f64; 2],
    pub hi: [f64; 2],
}

/// A legend's drawing, and where each component is.
#[derive(Debug, Clone)]
pub struct LegendDrawing {
    pub drawing: PlanDrawing,
    pub items: Vec<Item>,
    /// Components that could not be drawn, with why.
    pub failed: Vec<(ElementId, String)>,
}

/// The sample's meshes as a render scene.
fn scene_of(sample: &legends::Sample) -> RenderScene {
    let mut meshes = Vec::new();
    for b in &sample.bodies {
        for part in &b.parts {
            meshes.push(SceneMesh {
                element: Some(b.element),
                color: reviera_render::class_color(&part.class),
                class: part.class.clone(),
                positions: part.mesh.0.iter().map(|[x, y, z]| [*x as f32, *y as f32, *z as f32]).collect(),
                indices: part.mesh.1.clone(),
            });
        }
    }
    RenderScene { meshes }
}

/// The frame a direction looks at the sample in (`None` for plan): a section or elevation line
/// placed around it.
fn frame_of(direction: Direction, kind: TypeKind, lo: [f32; 3], hi: [f32; 3]) -> Option<SectionDef> {
    let (lo, hi) = ([f64::from(lo[0]), f64::from(lo[1])], [f64::from(hi[0]), f64::from(hi[1])]);
    let pad = 1.0;
    let far = (hi[0] - lo[0]).max(hi[1] - lo[1]) + 2.0 * pad;
    let def = |start: [f64; 2], end: [f64; 2]| Some(SectionDef { start, end, far, flipped: false });
    match direction {
        Direction::Plan => None,
        // Looking north (+y) from in front.
        Direction::Front => def([hi[0] + pad, lo[1] - pad], [lo[0] - pad, lo[1] - pad]),
        // Looking south (−y) from behind.
        Direction::Back => def([lo[0] - pad, hi[1] + pad], [hi[0] + pad, hi[1] + pad]),
        // Across a wall (looking along it, cut at its middle); across a floor (cut at its middle,
        // looking north).
        Direction::Section => match kind {
            TypeKind::Floor => {
                let y = (lo[1] + hi[1]) / 2.0;
                def([hi[0] + pad, y], [lo[0] - pad, y])
            }
            _ => {
                let x = (lo[0] + hi[0]) / 2.0;
                def([x, lo[1] - pad], [x, hi[1] + pad])
            }
        },
    }
}

fn shift_graphics(g: &mut PlanGraphics, d: [f32; 2]) {
    for l in &mut g.lines {
        for s in &mut l.segments {
            for p in s.iter_mut() {
                p[0] += d[0];
                p[1] += d[1];
            }
        }
    }
    for m in &mut g.poche {
        for p in &mut m.positions {
            p[0] += d[0];
            p[1] += d[1];
        }
    }
    for (_, s) in &mut g.cut_edges {
        for p in s.iter_mut() {
            p[0] += d[0];
            p[1] += d[1];
        }
    }
}

/// One component's drawing (in its own coordinates) and its 2D extent there.
fn component_graphics(sample: &legends::Sample, direction: Direction, scale: f32) -> Option<(PlanGraphics, [f32; 2], [f32; 2])> {
    let scene = scene_of(sample);
    let (lo, hi) = scene.bounds()?;
    let pens = standards::pen_set(&sample.store);
    let styles = standards::object_styles(&sample.store);
    let weights = |element: Option<ElementId>| {
        let (cut, projection) = standards::element_widths(&sample.store, &pens, &styles, element, f64::from(scale));
        Weights { cut: cut.map(|w| w as f32), projection: projection as f32, opacity: 1.0, above: Default::default() }
    };
    match frame_of(direction, sample.kind, lo.to_array(), hi.to_array()) {
        None => {
            let edges = feature_edges(&scene);
            let mut g = plan_graphics(&scene, &edges, PLAN_CUT as f32, lo.z - 0.01, scale, &weights);
            // A door's swing, in the doors' projection pen.
            for p in reviera_core::inserts::placed(&sample.store) {
                let swing = p.swing();
                if !swing.is_empty() {
                    g.lines.push(Lines {
                        color: reviera_render::plan::PROJECTION_LINE,
                        width: weights(Some(p.id)).projection,
                        segments: swing.into_iter().map(|s| s.map(|[x, y]| [x as f32, y as f32, PLAN_CUT as f32])).collect(),
                    });
                }
            }
            let (mut a, mut b) = ([lo.x, lo.y], [hi.x, hi.y]);
            for l in &g.lines {
                for s in &l.segments {
                    for p in s {
                        a = [a[0].min(p[0]), a[1].min(p[1])];
                        b = [b[0].max(p[0]), b[1].max(p[1])];
                    }
                }
            }
            Some((g, a, b))
        }
        Some(def) => {
            let frame = def.frame();
            let meshes: Vec<SceneMesh> = scene
                .meshes
                .iter()
                .map(|m| SceneMesh {
                    positions: m
                        .positions
                        .iter()
                        .map(|p| {
                            let q = frame.to_local([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
                            [q[0] as f32, q[1] as f32, q[2] as f32]
                        })
                        .collect(),
                    ..m.clone()
                })
                .collect();
            let local = RenderScene { meshes };
            let (llo, lhi) = local.bounds()?;
            let edges = visible_edges(&local, &feature_edges(&local), 0.0, 0.05);
            let g = plan_graphics(&local, &edges, 0.0, -frame.far as f32, scale, &weights);
            Some((g, [llo.x, llo.y], [lhi.x, lhi.y]))
        }
    }
}

/// The drawing of legend `view`: its components and text notes, at its scale.
pub fn legend_drawing(store: &Store, view: &Element) -> Result<LegendDrawing> {
    if !legends::is_legend(view) {
        return Err(Error::new("legend.not_legend", format!("view {} is not a legend", view.id())));
    }
    let scale = view.param(views::VIEW_KEY).and_then(|v| v.get("scale")?.as_f64()).unwrap_or(50.0) as f32;
    let mut graphics = PlanGraphics { scale, top: 1.0, bottom: -1.0, ..PlanGraphics::default() };
    let (mut items, mut failed) = (Vec::new(), Vec::new());
    for (id, d) in legends::of_view(store, view.guid().as_str()) {
        let sample = match legends::sample(store, &d.type_guid, d.host_length) {
            Ok(s) => s,
            Err(e) => {
                failed.push((id, e.to_string()));
                continue;
            }
        };
        let Some((mut g, lo, hi)) = component_graphics(&sample, d.direction, scale) else {
            failed.push((id, "the type has no geometry to draw".into()));
            continue;
        };
        // Flat in the legend's plane: the drawing at its point.
        let shift = [d.point[0] as f32 - lo[0], d.point[1] as f32 - lo[1]];
        shift_graphics(&mut g, shift);
        for l in &mut g.lines {
            for s in &mut l.segments {
                for p in s.iter_mut() {
                    p[2] = 0.0;
                }
            }
        }
        for m in &mut g.poche {
            for p in &mut m.positions {
                p[2] = -0.001;
            }
        }
        graphics.lines.append(&mut g.lines);
        graphics.poche.append(&mut g.poche);
        graphics.cut_edges.append(&mut g.cut_edges);
        let size = [f64::from(hi[0] - lo[0]), f64::from(hi[1] - lo[1])];
        items.push(Item { id, lo: d.point, hi: [d.point[0] + size[0], d.point[1] + size[1]] });
    }
    // The legend's text notes.
    let mut note_lines = Lines { color: [0.0, 0.0, 0.0, 1.0], width: standards::pen_set(store).annotation_width(1) as f32, segments: Vec::new() };
    let mut labels = Vec::new();
    for n in crate::notes::view_notes(store, view, scale, 0.002, None) {
        note_lines.segments.extend(n.parts.segments);
        labels.extend(n.parts.labels);
    }
    Ok(LegendDrawing {
        drawing: PlanDrawing { name: view.name.clone().unwrap_or_default(), scale, graphics, annotation: vec![note_lines], labels, paper: None },
        items,
        failed,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub view_id: ElementRef,
}

/// `legend.list`'s answer.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListOut {
    /// Each component drawn: its element and extent (metres, in the legend).
    pub items: Vec<Item>,
    /// Components that could not be drawn, and why.
    pub failed: Vec<(ElementId, String)>,
}

/// `legend.list`.
#[derive(Debug)]
pub struct LegendList;

impl HostCommand for LegendList {
    type Host = crate::App;
    type Params = ListParams;
    type Output = ListOut;
    const META: CommandMeta = CommandMeta {
        name: "legend.list",
        version: 1,
        summary: "A legend's components as drawn",
        description: "The legend components of a legend view (DOC-VIEW-010) as drawn now: each one's extent in the \
                      legend (metres at full size), and those that could not be drawn (a deleted or unsupported \
                      type) with why.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut crate::App, _: &Caller, p: ListParams) -> Result<ListOut> {
        let doc = app.resolve(None)?;
        let store = app.get(doc)?.doc.store();
        let view = store.resolve(&p.view_id).and_then(|v| store.get(v)).ok_or_else(|| Error::new("view.not_found", "no such view"))?;
        let d = legend_drawing(store, view)?;
        Ok(ListOut { items: d.items, failed: d.failed })
    }
}
