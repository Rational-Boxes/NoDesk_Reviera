// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Output (spec Ch 5 §5.15): plan views as vector PDF at their scale (DOC-OUT-003).
//!
//! Each view is one page: the smallest ISO landscape sheet that holds the drawing at its scale,
//! cut poché filled, lines at their pens' paper widths, dimension values as real (searchable)
//! text, and a title strip. A sheet is one page exactly its paper size (see `sheets`). The geometry is the plan drawing of the GPU viewport (the exact
//! OCCT hidden-line drawing of §5.12 replaces it later).

use std::path::{Path, PathBuf};

use pdf_writer::types::{LineCapStyle, LineJoinStyle};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use reviera_core::{
    Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Result,
};
use reviera_core::{annotations, grids, plans, standards, units, views};
use reviera_render::annotation::{
    Dimension, GridMark, GridStyle, Label, dimension_parts, grid_parts,
};
use reviera_render::plan::{Lines, PlanGraphics, Weights, feature_edges, plan_graphics, visible_edges};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{App, DocumentId};

/// Floors and slabs below a plan's bottom still show within this depth (DOC-VR-002 step 6).
pub const FLOOR_TOLERANCE: f64 = 1.22;

/// A plan view's drawing: the plan graphics, its annotation lines and labels.
#[derive(Debug, Clone)]
pub struct PlanDrawing {
    pub name: String,
    pub scale: f32,
    pub graphics: PlanGraphics,
    pub annotation: Vec<Lines>,
    pub labels: Vec<Label>,
    /// A sheet: the paper's name and size (mm). The drawing is in paper metres from the lower-left
    /// corner at scale 1 and fills the page exactly, without a title strip.
    pub paper: Option<Paper>,
}

/// A sheet's paper.
#[derive(Debug, Clone, PartialEq)]
pub struct Paper {
    pub size: &'static str,
    pub width: f32,
    pub height: f32,
}

impl App {
    /// The drawing of plan view `view` of `doc`.
    pub fn plan_drawing(&self, doc: DocumentId, view: ElementId) -> Result<PlanDrawing> {
        self.plan_drawing_of(&self.render_scene(doc)?, doc, view)
    }

    /// The drawing of plan view `view` of `doc` with its model `scene`.
    pub(crate) fn plan_drawing_of(
        &self,
        scene: &reviera_render::RenderScene,
        doc: DocumentId,
        view: ElementId,
    ) -> Result<PlanDrawing> {
        let open = self.get(doc)?;
        let store = open.doc.store();
        let e = store
            .get(view)
            .ok_or_else(|| Error::new("view.not_found", format!("no view {view}")))?;
        let ceiling = views::view_data(e).is_some_and(|d| d.kind == plans::KIND_CEILING_PLAN);
        views::view_data(e)
            .filter(|d| plans::is_plan(&d.kind))
            .ok_or_else(|| Error::new("view.not_a_plan", format!("view {view} is not a plan")))?;
        let range = plans::plan_range(store, view)
            .ok_or_else(|| Error::new("view.no_range", "the plan's range cannot be resolved"))?;
        let scale = e
            .param(views::VIEW_KEY)
            .and_then(|v| v.get("scale")?.as_f64())
            .unwrap_or(100.0);
        // A ceiling plan looks up from its cut plane to its top.
        let bottom = if ceiling {
            range.top.min(1e6)
        } else {
            (range.bottom - FLOOR_TOLERANCE).min(range.depth).max(-1e6)
        };
        let pens = standards::pen_set(store);
        let styles = standards::object_styles(store);
        let weights = |element: Option<ElementId>| {
            let (cut, projection) =
                standards::element_widths(store, &pens, &styles, element, scale);
            Weights {
                cut: cut.map(|w| w as f32),
                projection: projection as f32,
                // Printed and exported drawings never show temporary state.
                opacity: 1.0,
                above: element
                    .and_then(|e| store.get(e))
                    .map(reviera_core::categories::above_cut)
                    .unwrap_or_default(),
            }
        };
        let edges = feature_edges(scene);
        let mut graphics = plan_graphics(
            scene,
            &edges,
            range.cut as f32,
            bottom as f32,
            scale as f32,
            &weights,
        );
        if !ceiling {
            graphics.lines.extend(reviera_render::plan::above_cut_lines(
                &edges,
                range.cut as f32,
                range.top.min(1e6) as f32,
                scale as f32,
                &weights,
            ));
        }
        // Dimensions: annotation pen 1, values in project units.
        let settings = units::settings_of(store);
        let mut lines = Lines {
            color: [0.0, 0.0, 0.0, 1.0],
            width: pens.annotation_width(1) as f32,
            segments: Vec::new(),
        };
        let mut labels = Vec::new();
        for (_, d) in annotations::of_view(store, e.guid().as_str()) {
            let Some(m) = annotations::measure(store, &d) else {
                continue;
            };
            let texts = m
                .values
                .iter()
                .map(|v| {
                    units::format(*v, "length", &settings, None)
                        .unwrap_or_else(|_| format!("{v:.3}"))
                })
                .collect();
            let dim = Dimension {
                points: m.points,
                along: m.along,
                line: d.line,
                texts,
            };
            let (l, t) = dimension_parts(&dim, scale as f32, range.cut as f32);
            lines.segments.extend(l);
            labels.extend(t);
        }
        // Grids show in every plan, styled by their grid type (ARC-GRID-001).
        let mut annotation = vec![lines];
        // Reference planes and model lines in the view range (ARC-REFP-001, ARC-MLIN-001).
        let view_guid = e.guid().to_string();
        annotation.extend(reviera_render::datums::plan_lines(
            store,
            Some(&view_guid),
            range.cut,
            bottom.min(range.cut),
            range.top.max(range.cut),
            range.cut + 0.002,
            scale,
            &Default::default(),
        ));
        annotation.extend(reviera_render::datums::scope_box_lines(
            store,
            range.cut,
            range.cut + 0.002,
            scale,
            &Default::default(),
        ));
        for (_, mark) in reviera_render::datums::plan_grids(store, Some(&view_guid), range.cut) {
            let (l, t) = grid_parts(&mark, scale as f32, range.cut as f32);
            annotation.extend(l);
            labels.extend(t);
        }
        // Door swings of the doors in walls on the plan's level, in the doors' projection pen.
        let level = views::view_data(e).and_then(|d| d.level);
        // Tags and text notes, in annotation pen 1.
        let mut note_lines = Lines {
            color: [0.0, 0.0, 0.0, 1.0],
            width: pens.annotation_width(1) as f32,
            segments: Vec::new(),
        };
        for n in crate::notes::view_notes(store, e, scale as f32, range.cut as f32, Some(scene)) {
            note_lines.segments.extend(n.parts.segments);
            labels.extend(n.parts.labels);
        }
        annotation.push(note_lines);
        // Room labels on the plan's level: Name, Number and Area (ARC-ROOM-001), for rooms
        // without a room tag in the view.
        let tagged = crate::notes::tagged_rooms(store, e);
        if !ceiling {
            for r in reviera_core::rooms::all(store)
                .into_iter()
                .filter(|r| Some(&r.level) == level.as_ref())
                .filter(|r| {
                    store
                        .get(r.id)
                        .is_none_or(|e| !tagged.contains(e.guid().as_str()))
                })
            {
                let area = if r.geometry.enclosed() {
                    units::format(r.geometry.area(), "area", &settings, None)
                        .unwrap_or_else(|_| format!("{:.2} m²", r.geometry.area()))
                } else {
                    "Not Enclosed".to_owned()
                };
                labels.extend(reviera_render::annotation::room_tag_labels(
                    r.geometry.tag_point,
                    &[r.name, r.number, area],
                    scale as f32,
                    range.cut as f32,
                ));
            }
        }
        for p in reviera_core::inserts::placed(store) {
            let on_level = store
                .get(p.host)
                .and_then(reviera_core::walls::wall_recipe)
                .is_some_and(|r| Some(&r.base_level) == level.as_ref());
            let swing = p.swing();
            if ceiling || !on_level || swing.is_empty() {
                continue;
            }
            let z = range.cut as f32;
            annotation.push(Lines {
                color: reviera_render::plan::PROJECTION_LINE,
                width: weights(Some(p.id)).projection,
                segments: swing
                    .into_iter()
                    .map(|s| s.map(|[x, y]| [x as f32, y as f32, z]))
                    .collect(),
            });
        }
        Ok(PlanDrawing {
            name: e.name.clone().unwrap_or_default(),
            scale: scale as f32,
            graphics,
            annotation,
            labels,
            paper: None,
        })
    }
}

/// A section's or elevation's drawing in its own frame (DOC-VIEW-003/004): the model turned so
/// the view looks down its z, the cut and projection graphics of that model, and the levels and
/// the grids crossing the view plane.
#[derive(Debug, Clone)]
pub struct SectionParts {
    /// The model in the section frame (for the viewport).
    pub scene: reviera_render::RenderScene,
    pub graphics: PlanGraphics,
    pub annotation: Vec<Lines>,
    pub labels: Vec<Label>,
}

/// The parts of a section of `scene` along `def`, at 1:`scale`. `shown` filters elements
/// (temporary hiding).
pub fn section_parts(
    store: &reviera_core::Store,
    view: Option<&str>,
    scene: &reviera_render::RenderScene,
    def: &reviera_core::sections::SectionDef,
    scale: f32,
    shown: &dyn Fn(Option<ElementId>) -> bool,
) -> SectionParts {
    let frame = def.frame();
    // The model in the frame. Door panels hidden in plans show in sections.
    let meshes: Vec<reviera_render::SceneMesh> = scene
        .meshes
        .iter()
        .filter(|m| shown(m.element) && !reviera_render::is_virtual(&m.class))
        .map(|m| reviera_render::SceneMesh {
            element: m.element,
            class: m
                .class
                .strip_suffix(reviera_render::PLAN_HIDDEN)
                .unwrap_or(&m.class)
                .to_owned(),
            positions: m
                .positions
                .iter()
                .map(|p| {
                    let q = frame.to_local([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
                    [q[0] as f32, q[1] as f32, q[2] as f32]
                })
                .collect(),
            indices: m.indices.clone(),
            color: m.color,
        })
        .collect();
    let local = reviera_render::RenderScene { meshes };
    let pens = standards::pen_set(store);
    let styles = standards::object_styles(store);
    let weights = |element: Option<ElementId>| {
        let (cut, projection) =
            standards::element_widths(store, &pens, &styles, element, f64::from(scale));
        Weights {
            cut: cut.map(|w| w as f32),
            projection: projection as f32,
            opacity: 1.0,
            above: reviera_core::categories::AboveCut::Hide,
        }
    };
    // Hidden edges are left out (DOC-VIEW-003, DOC-VIEW-004; on the CPU until DOC-HLR-003).
    let edges = visible_edges(&local, &feature_edges(&local), 0.0, 0.05);
    let graphics = plan_graphics(&local, &edges, 0.0, -frame.far as f32, scale, &weights);
    // Levels: a line across the view at each elevation, with the name and elevation at the
    // right end.
    let mm = f64::from(scale) / 1000.0;
    let settings = units::settings_of(store);
    let z = 0.002f32;
    let levels = reviera_core::levels::list(store);
    let mut level_lines = Lines {
        color: [0.0, 0.0, 0.0, 1.0],
        width: pens.annotation_width(1) as f32,
        segments: Vec::new(),
    };
    let mut labels = Vec::new();
    let (x0, x1) = (-5.0 * mm, frame.width + 5.0 * mm);
    for l in &levels {
        // A level's extent across the view: its 2D or 3D extent, else the whole view
        // (ARC-DATUM-001); none outside its scope box.
        let Some(le) = store.get(l.id) else { continue };
        if !reviera_core::datum_extents::shows_in_section(store, le, &frame) {
            continue;
        }
        let (x0, x1) = reviera_core::datum_extents::level_range(store, le, view, &frame)
            .map_or((x0, x1), |r| (r[0], r[1]));
        let y = l.elevation as f32;
        level_lines
            .segments
            .push([[x0 as f32, y, z], [x1 as f32, y, z]]);
        let height = (2.5 * mm) as f32;
        let elevation = units::format(l.elevation, "length", &settings, None)
            .unwrap_or_else(|_| format!("{:.3}", l.elevation));
        for (k, text) in [l.name.clone(), elevation].into_iter().enumerate() {
            labels.push(Label {
                origin: [
                    (x1 + 1.5 * mm) as f32,
                    y + (1.0 * mm) as f32 - k as f32 * (4.0 * mm) as f32,
                    z,
                ],
                right: [1.0, 0.0, 0.0],
                up: [0.0, 1.0, 0.0],
                height,
                text,
            });
        }
    }
    let mut annotation = vec![level_lines];
    // Grids crossing the view plane: vertical lines with a bubble at the top.
    // From below the lowest level to above the highest of the levels and the model.
    let model = local.bounds();
    let (lo, hi) = levels
        .iter()
        .map(|l| l.elevation)
        .chain(model.map(|(a, b)| f64::from(a.y).min(f64::from(b.y))))
        .chain(model.map(|(_, b)| f64::from(b.y)))
        .fold((0.0f64, 0.0f64), |(a, b), e| (a.min(e), b.max(e)));
    for g in grids::list(store) {
        let Some(ge) = store.get(g.id) else { continue };
        if !reviera_core::datum_extents::shows_in_section(store, ge, &frame) {
            continue;
        }
        let Some(x) = frame.grid_crossing(g.curve.start, g.curve.end) else {
            continue;
        };
        // Its 2D or 3D heights (ARC-DATUM-001), else from below the levels to above them.
        let [bottom, top] = reviera_core::datum_extents::heights(store, ge, view)
            .unwrap_or([lo - 5.0 * mm, hi + 12.0 * mm]);
        let bubbles = view
            .and_then(|v| reviera_core::datum_extents::in_view(ge, v).bubbles)
            .unwrap_or([false, true]);
        let mark = GridMark {
            start: [x, bottom],
            end: [x, top],
            name: g.name,
            bubbles,
            style: GridStyle::of(&g.style, &|p| pens.annotation_width(p) as f32),
            elbows: [None, None],
        };
        let (l, t) = grid_parts(&mark, scale, z);
        annotation.extend(l);
        labels.extend(t);
    }
    // Reference planes it looks along and model lines in its depth (ARC-REFP-001, ARC-MLIN-001).
    annotation.extend(reviera_render::datums::section_lines(
        store,
        view,
        &frame,
        (lo - 5.0 * mm, hi + 5.0 * mm),
        f64::from(z),
        f64::from(scale),
        &Default::default(),
    ));
    SectionParts {
        scene: local,
        graphics,
        annotation,
        labels,
    }
}

/// The dimensions, tags, text notes and spots of section or elevation `view` in its frame at 1:`scale`,
/// drawn at depth `z`, in annotation pen 1 (DOC-ANN-001, DOC-ANN-009…011).
pub fn section_annotation(store: &reviera_core::Store, view: &reviera_core::Element, scale: f32, z: f32, scene: Option<&reviera_render::RenderScene>) -> (Lines, Vec<Label>) {
    let pens = standards::pen_set(store);
    let settings = units::settings_of(store);
    let mut lines = Lines { color: [0.0, 0.0, 0.0, 1.0], width: pens.annotation_width(1) as f32, segments: Vec::new() };
    let mut labels = Vec::new();
    for (_, d) in annotations::of_view(store, view.guid().as_str()) {
        let Some(m) = annotations::measure(store, &d) else { continue };
        let texts = m.values.iter().map(|v| units::format(*v, "length", &settings, None).unwrap_or_else(|_| format!("{v:.3}"))).collect();
        let dim = Dimension { points: m.points, along: m.along, line: d.line, texts };
        let (l, t) = dimension_parts(&dim, scale, z);
        lines.segments.extend(l);
        labels.extend(t);
    }
    for n in crate::notes::view_notes(store, view, scale, z, scene) {
        lines.segments.extend(n.parts.segments);
        labels.extend(n.parts.labels);
    }
    (lines, labels)
}

impl App {
    /// The drawing of section or elevation view `view` of `doc`, for output.
    pub fn section_drawing(&self, doc: DocumentId, view: ElementId) -> Result<PlanDrawing> {
        self.section_drawing_of(&self.render_scene(doc)?, doc, view)
    }

    /// The drawing of section or elevation view `view` of `doc` with its model `scene`.
    pub(crate) fn section_drawing_of(
        &self,
        scene: &reviera_render::RenderScene,
        doc: DocumentId,
        view: ElementId,
    ) -> Result<PlanDrawing> {
        let open = self.get(doc)?;
        let store = open.doc.store();
        let e = store
            .get(view)
            .ok_or_else(|| Error::new("view.not_found", format!("no view {view}")))?;
        let (_, def) = reviera_core::sections::section_of(e).ok_or_else(|| {
            Error::new(
                "view.not_a_section",
                format!("view {view} is not a section"),
            )
        })?;
        let scale = e
            .param(views::VIEW_KEY)
            .and_then(|v| v.get("scale")?.as_f64())
            .unwrap_or(100.0) as f32;
        let parts = section_parts(store, Some(e.guid().as_str()), scene, &def, scale, &|_| {
            true
        });
        let (mut annotation, mut labels) = (parts.annotation, parts.labels);
        let (lines, more) = section_annotation(store, e, scale, 0.002, Some(scene));
        annotation.push(lines);
        labels.extend(more);
        Ok(PlanDrawing {
            name: e.name.clone().unwrap_or_default(),
            scale,
            graphics: parts.graphics,
            annotation,
            labels,
            paper: None,
        })
    }
}

/// ISO A sheets, landscape, in millimetres.
const SHEETS: [(&str, f32, f32); 5] = [
    ("A4", 297.0, 210.0),
    ("A3", 420.0, 297.0),
    ("A2", 594.0, 420.0),
    ("A1", 841.0, 594.0),
    ("A0", 1189.0, 841.0),
];
const MARGIN_MM: f32 = 10.0;
const TITLE_MM: f32 = 18.0;
const PT_PER_MM: f32 = 72.0 / 25.4;
/// Helvetica: cap height and the average advance of digits, per em.
const CAP_HEIGHT: f32 = 0.718;
const DIGIT_ADVANCE: f32 = 0.556;

/// Linear colour to the sRGB values PDF uses.
fn srgb(c: f32) -> f32 {
    c.clamp(0.0, 1.0).powf(1.0 / 2.2)
}

/// The plan extents of a drawing: (min, max) of x and y.
pub(crate) fn extents(d: &PlanDrawing) -> Option<([f32; 2], [f32; 2])> {
    let mut points = d
        .graphics
        .poche
        .iter()
        .flat_map(|m| m.positions.iter().map(|p| [p[0], p[1]]))
        .chain(d.graphics.lines.iter().chain(&d.annotation).flat_map(|l| {
            l.segments
                .iter()
                .flat_map(|s| [[s[0][0], s[0][1]], [s[1][0], s[1][1]]])
        }))
        .chain(d.labels.iter().map(|l| [l.origin[0], l.origin[1]]));
    let first = points.next()?;
    Some(points.fold((first, first), |(lo, hi), p| {
        (
            [lo[0].min(p[0]), lo[1].min(p[1])],
            [hi[0].max(p[0]), hi[1].max(p[1])],
        )
    }))
}

/// What a page shows besides the drawing.
#[derive(Debug, Clone)]
pub struct PageInfo {
    pub project: String,
    pub date: String,
}

/// Writes drawings as a PDF, one page each. Returns the sheet name of each page.
pub fn write_pdf(drawings: &[(PlanDrawing, PageInfo)], path: &Path) -> Result<Vec<&'static str>> {
    let mut pdf = Pdf::new();
    let catalog = Ref::new(1);
    let tree = Ref::new(2);
    let font = Ref::new(3);
    let info = Ref::new(4);
    let mut next = 5;
    let mut alloc = || {
        next += 1;
        Ref::new(next - 1)
    };
    let pages: Vec<(Ref, Ref)> = drawings.iter().map(|_| (alloc(), alloc())).collect();
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree)
        .kids(pages.iter().map(|p| p.0))
        .count(pages.len() as i32);
    pdf.type1_font(font)
        .base_font(Name(b"Helvetica"))
        .encoding_predefined(Name(b"WinAnsiEncoding"));
    let title = drawings
        .first()
        .map_or("Reviera drawing".to_owned(), |(d, i)| {
            format!("{} - {}", i.project, d.name)
        });
    let creator = format!("BIM Reviera {}", env!("CARGO_PKG_VERSION"));
    pdf.document_info(info)
        .title(TextStr(&title))
        .creator(TextStr(&creator))
        .producer(TextStr(&creator));
    let mut sheets = Vec::new();
    for ((d, page_info), (page_id, content_id)) in drawings.iter().zip(&pages) {
        let (lo, hi) = extents(d).unwrap_or(([0.0, 0.0], [1.0, 1.0]));
        let mm_per_m = 1000.0 / d.scale;
        let (w_mm, h_mm) = ((hi[0] - lo[0]) * mm_per_m, (hi[1] - lo[1]) * mm_per_m);
        let (sheet, paper_w, paper_h) = match &d.paper {
            Some(p) => (p.size, p.width, p.height),
            None => SHEETS
                .iter()
                .copied()
                .find(|(_, pw, ph)| {
                    w_mm + 2.0 * MARGIN_MM <= *pw && h_mm + 2.0 * MARGIN_MM + TITLE_MM <= *ph
                })
                .unwrap_or(SHEETS[SHEETS.len() - 1]),
        };
        sheets.push(sheet);
        // Model (x, y) in metres to page points, the drawing centred above the title strip; a
        // sheet's paper metres map onto the page as they are.
        let (cx, cy, ox, oy) = match d.paper {
            Some(_) => (0.0, 0.0, 0.0, 0.0),
            None => (
                (lo[0] + hi[0]) / 2.0,
                (lo[1] + hi[1]) / 2.0,
                paper_w / 2.0,
                (paper_h + TITLE_MM) / 2.0,
            ),
        };
        let to_page = move |x: f32, y: f32| {
            (
                ((x - cx) * mm_per_m + ox) * PT_PER_MM,
                ((y - cy) * mm_per_m + oy) * PT_PER_MM,
            )
        };
        let mut c = Content::new();
        c.set_line_cap(LineCapStyle::RoundCap);
        c.set_line_join(LineJoinStyle::RoundJoin);
        // Cut poché, each triangle also stroked thinly so no seams show between them.
        for m in &d.graphics.poche {
            let [r, g, b, _] = m.color.map(srgb);
            c.set_fill_rgb(r, g, b)
                .set_stroke_rgb(r, g, b)
                .set_line_width(0.1);
            for t in m.indices.as_chunks::<3>().0 {
                let p = t.map(|i| m.positions[i as usize]);
                let (x0, y0) = to_page(p[0][0], p[0][1]);
                c.move_to(x0, y0);
                for q in &p[1..] {
                    let (x, y) = to_page(q[0], q[1]);
                    c.line_to(x, y);
                }
                c.close_path().fill_nonzero_and_stroke();
            }
        }
        // Lines at their pens' paper widths.
        for l in d.graphics.lines.iter().chain(&d.annotation) {
            let [r, g, b, _] = l.color.map(srgb);
            c.set_stroke_rgb(r, g, b)
                .set_line_width(l.width.max(0.05) * PT_PER_MM);
            for s in &l.segments {
                let (x0, y0) = to_page(s[0][0], s[0][1]);
                let (x1, y1) = to_page(s[1][0], s[1][1]);
                c.move_to(x0, y0).line_to(x1, y1);
            }
            c.stroke();
        }
        // Dimension values as text, centred where the stroke-font text would be.
        c.set_fill_rgb(0.0, 0.0, 0.0);
        for l in &d.labels {
            let height_mm = l.height * mm_per_m;
            let size = height_mm / CAP_HEIGHT * PT_PER_MM;
            let stroke_width = reviera_render::font::width(&l.text, l.height);
            let text_width =
                l.text.len() as f32 * DIGIT_ADVANCE * (height_mm / CAP_HEIGHT) / mm_per_m;
            let shift = (stroke_width - text_width) / 2.0;
            let (x, y) = to_page(
                l.origin[0] + l.right[0] * shift,
                l.origin[1] + l.right[1] * shift,
            );
            c.begin_text()
                .set_font(Name(b"F1"), size)
                .set_text_matrix([l.right[0], l.right[1], l.up[0], l.up[1], x, y])
                .show(Str(&win_ansi(&l.text)))
                .end_text();
        }
        if d.paper.is_none() {
            title_strip(&mut c, d, page_info, sheet, paper_w);
        }
        let mut page = pdf.page(*page_id);
        page.media_box(Rect::new(
            0.0,
            0.0,
            paper_w * PT_PER_MM,
            paper_h * PT_PER_MM,
        ))
        .parent(tree)
        .contents(*content_id);
        page.resources().fonts().pair(Name(b"F1"), font);
        page.finish();
        pdf.stream(*content_id, &c.finish());
    }
    std::fs::write(path, pdf.finish())
        .map_err(|e| Error::new("export.write_failed", format!("{}: {e}", path.display())))?;
    Ok(sheets)
}

/// A view page's title strip along the bottom.
fn title_strip(c: &mut Content, d: &PlanDrawing, page_info: &PageInfo, sheet: &str, paper_w: f32) {
    let (sx, sy) = (MARGIN_MM * PT_PER_MM, MARGIN_MM * PT_PER_MM);
    let strip_w = (paper_w - 2.0 * MARGIN_MM) * PT_PER_MM;
    let strip_h = (TITLE_MM - 4.0) * PT_PER_MM;
    c.set_stroke_rgb(0.0, 0.0, 0.0)
        .set_line_width(0.5 * PT_PER_MM);
    c.rect(sx, sy, strip_w, strip_h).stroke();
    let lines = [
        (12.0, format!("{}  -  {}", page_info.project, d.name)),
        (
            8.0,
            format!(
                "Scale 1:{}    {}    {}    BIM Reviera",
                d.scale, sheet, page_info.date
            ),
        ),
    ];
    for (k, (size, text)) in lines.iter().enumerate() {
        c.begin_text()
            .set_font(Name(b"F1"), *size)
            .next_line(
                sx + 4.0 * PT_PER_MM,
                sy + strip_h - (k as f32 + 1.0) * 5.5 * PT_PER_MM,
            )
            .show(Str(&win_ansi(text)))
            .end_text();
    }
}

/// Text in the Windows-1252 encoding the PDF's Helvetica uses; other characters become `?`.
fn win_ansi(text: &str) -> Vec<u8> {
    const HIGH: &str = "€\u{81}‚ƒ„…†‡ˆ‰Š‹Œ\u{8d}Ž\u{8f}\u{90}‘’“”•–—˜™š›œ\u{9d}žŸ";
    text.chars()
        .map(|ch| match ch as u32 {
            0x20..=0x7e | 0xa0..=0xff => ch as u8,
            _ => HIGH
                .chars()
                .position(|h| h == ch)
                .map_or(b'?', |i| 0x80 + i as u8),
        })
        .collect()
}

/// Today's date as YYYY-MM-DD (UTC).
pub(crate) fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

// ---- export.pdf ----

/// The views to export.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ExportRange {
    /// `"current"`: the active view.
    Current(CurrentRange),
    /// `{"views": [...]}`.
    Views { views: Vec<ElementRef> },
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CurrentRange {
    Current,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportPdfParams {
    pub range: ExportRange,
    /// The PDF file (combined), or the folder for separate files.
    pub path_or_folder: PathBuf,
    /// One file with a page per view (default), or one file per view named after it.
    #[serde(default = "yes")]
    pub combine: bool,
    /// Reserved: setups and naming rules come with the export dialog.
    pub set_id: Option<String>,
    pub setup_id: Option<String>,
    pub naming_rule: Option<String>,
    /// Exports run in the foreground for now.
    pub background: Option<bool>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportResult {
    pub job_id: String,
    pub files: Vec<PathBuf>,
    /// The sheet size of each page (A4 to A0).
    pub sheets: Vec<String>,
}

pub(crate) struct ExportPdf;

impl HostCommand for ExportPdf {
    type Host = App;
    type Params = ExportPdfParams;
    type Output = ExportResult;
    const META: CommandMeta = CommandMeta {
        name: "export.pdf",
        version: 1,
        summary: "Export views as PDF",
        description: "Writes plan, section and elevation views as vector PDF at their scale, one page per view on the smallest ISO \
                      sheet that holds it, and sheets one page each at their paper size: `range` is \"current\" or \
                      {\"views\": [...]} (views and sheets).",
        effects: &[Effect::FileWrite],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ExportPdfParams) -> Result<ExportResult> {
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store();
        let ids: Vec<ElementId> = match &p.range {
            ExportRange::Current(_) => vec![
                app.active_view(doc)
                    .ok_or_else(|| Error::new("view.none", "no view is active"))?,
            ],
            ExportRange::Views { views } => views
                .iter()
                .map(|r| {
                    store
                        .resolve(r)
                        .ok_or_else(|| Error::new("view.not_found", "no such view"))
                })
                .collect::<Result<_>>()?,
        };
        if ids.is_empty() {
            return Err(Error::new("export.nothing", "no views to export"));
        }
        let info = PageInfo {
            project: app
                .documents()
                .into_iter()
                .find(|d| d.id == doc)
                .map_or("Project".into(), |d| d.title),
            date: today(),
        };
        let drawings: Vec<(PlanDrawing, PageInfo)> = ids
            .iter()
            .map(|v| {
                let e = store.get(*v);
                let section = e.and_then(reviera_core::sections::section_of).is_some();
                let d = if e.is_some_and(reviera_core::sheets::is_sheet) {
                    app.sheet_drawing(doc, *v)?.drawing
                } else if section {
                    app.section_drawing(doc, *v)?
                } else {
                    app.plan_drawing(doc, *v)?
                };
                Ok((d, info.clone()))
            })
            .collect::<Result<_>>()?;
        let mut files = Vec::new();
        let mut sheets = Vec::new();
        if p.combine {
            sheets.extend(
                write_pdf(&drawings, &p.path_or_folder)?
                    .into_iter()
                    .map(String::from),
            );
            files.push(p.path_or_folder.clone());
        } else {
            std::fs::create_dir_all(&p.path_or_folder).map_err(|e| {
                Error::new(
                    "export.write_failed",
                    format!("{}: {e}", p.path_or_folder.display()),
                )
            })?;
            for d in drawings {
                let name: String =
                    d.0.name
                        .chars()
                        .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
                        .collect();
                let path = p.path_or_folder.join(format!("{name}.pdf"));
                sheets.extend(
                    write_pdf(std::slice::from_ref(&d), &path)?
                        .into_iter()
                        .map(String::from),
                );
                files.push(path);
            }
        }
        Ok(ExportResult {
            job_id: uuid::Uuid::new_v4().to_string(),
            files,
            sheets,
        })
    }
}
