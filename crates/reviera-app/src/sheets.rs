// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sheet drawings (DOC-SHT-001…003, DOC-SCH-011).
//!
//! A sheet draws in paper metres from its lower-left corner, at scale 1. It has:
//! - a border and Reviera's built-in title block;
//! - each viewport's plan or section drawing, scaled down by its view scale and centred at the
//!   viewport's position, with a view title under it;
//! - each placed schedule as a ruled table.
//!
//! The same drawing feeds the sheet view and the PDF (one page exactly the sheet's size).

use reviera_core::schedules::Table;
use reviera_core::sheets::{self, SheetData};
use reviera_core::{ElementId, Error, Result};
use reviera_render::SceneMesh;
use reviera_render::annotation::Label;
use reviera_render::plan::{Lines, PlanGraphics};

use crate::output::{Paper, PlanDrawing, extents};
use crate::{App, DocumentId};

/// The left (binding) and other margins of the border, millimetres.
pub const BINDING_MM: f32 = 20.0;
pub const MARGIN_MM: f32 = 10.0;
/// The title block in the lower-right corner, millimetres.
pub const TITLE_W_MM: f32 = 180.0;
pub const TITLE_H_MM: f32 = 48.0;

const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const MM: f32 = 0.001;

/// What a sheet item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Viewport,
    Schedule,
    /// A precast shop drawing (STR-PRE-006).
    ShopDrawing,
}

/// A viewport or placed schedule on a sheet and where it is drawn (paper metres).
#[derive(Debug, Clone, PartialEq)]
pub struct SheetItem {
    pub id: u32,
    pub kind: ItemKind,
    /// The view or schedule.
    pub element: ElementId,
    /// Its position as stored (millimetres): a viewport's centre, a schedule's top-left corner.
    pub position: [f64; 2],
    pub min: [f32; 2],
    pub max: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct SheetDrawing {
    pub drawing: PlanDrawing,
    pub items: Vec<SheetItem>,
}

/// Drawing on paper: lines grouped by width, and text.
#[derive(Default)]
struct Paperwork {
    lines: Vec<Lines>,
    labels: Vec<Label>,
    poche: Vec<SceneMesh>,
}

impl Paperwork {
    /// A line from (x0, y0) to (x1, y1), millimetres, `width` mm wide.
    fn line(&mut self, width: f32, x0: f32, y0: f32, x1: f32, y1: f32) {
        let seg = [[x0 * MM, y0 * MM, 0.0], [x1 * MM, y1 * MM, 0.0]];
        match self
            .lines
            .iter_mut()
            .find(|l| l.width == width && l.color == BLACK)
        {
            Some(l) => l.segments.push(seg),
            None => self.lines.push(Lines {
                color: BLACK,
                width,
                segments: vec![seg],
            }),
        }
    }

    fn rect(&mut self, width: f32, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.line(width, x0, y0, x1, y0);
        self.line(width, x1, y0, x1, y1);
        self.line(width, x1, y1, x0, y1);
        self.line(width, x0, y1, x0, y0);
    }

    /// Text with its baseline's left end at (x, y), `height` mm capitals.
    fn text(&mut self, x: f32, y: f32, height: f32, text: &str) {
        if text.is_empty() {
            return;
        }
        self.labels.push(Label {
            origin: [x * MM, y * MM, 0.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            height: height * MM,
            text: text.to_owned(),
        });
    }

    /// A caption over a value in the cell whose top-left corner is (x, top).
    fn field(&mut self, x: f32, top: f32, caption: &str, value: &str, height: f32) {
        self.text(x + 1.5, top - 3.2, 1.8, caption);
        self.text(x + 1.5, top - 5.0 - height, height, value);
    }
}

/// The width of `text` in millimetres at `height` mm.
fn text_width(text: &str, height: f32) -> f32 {
    reviera_render::font::width(text, height)
}

impl App {
    /// The drawing of sheet `sheet` of `doc` (paper metres) and where its items are.
    pub fn sheet_drawing(&self, doc: DocumentId, sheet: ElementId) -> Result<SheetDrawing> {
        let open = self.get(doc)?;
        let store = open.doc.store();
        let e = store
            .get(sheet)
            .ok_or_else(|| Error::new("sheet.not_found", format!("no sheet {sheet}")))?;
        let data = sheets::sheet_data(e).ok_or_else(|| {
            Error::new("sheet.not_found", format!("element {sheet} is not a sheet"))
        })?;
        let (w, h) = data.paper();
        let (w, h) = (w as f32, h as f32);
        let mut out = Paperwork::default();
        let mut items = Vec::new();

        // Viewports, each drawing scaled from the model onto the paper.
        let viewports = sheets::live_viewports(store, &data);
        let scene = if viewports.is_empty() {
            None
        } else {
            Some(self.render_scene(doc)?)
        };
        let mut scales = Vec::new();
        for (vp, view) in viewports {
            let scene = scene.as_ref().expect("rendered");
            let section = store
                .get(view)
                .and_then(reviera_core::sections::section_of)
                .is_some();
            let legend = store.get(view).filter(|e| reviera_core::legends::is_legend(e));
            let drawing = if let Some(l) = legend {
                crate::legends::legend_drawing(store, l).map(|d| d.drawing)
            } else if section {
                self.section_drawing_of(scene, doc, view)
            } else {
                self.plan_drawing_of(scene, doc, view)
            };
            let Ok(d) = drawing else { continue };
            let Some((lo, hi)) = extents(&d) else {
                continue;
            };
            let s = d.scale;
            scales.push(s);
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            let at = [vp.position[0] as f32 * MM, vp.position[1] as f32 * MM];
            let place =
                move |p: [f32; 3]| [(p[0] - c[0]) / s + at[0], (p[1] - c[1]) / s + at[1], 0.0];
            for m in &d.graphics.poche {
                out.poche.push(SceneMesh {
                    positions: m.positions.iter().map(|p| place(*p)).collect(),
                    ..m.clone()
                });
            }
            for l in d.graphics.lines.iter().chain(&d.annotation) {
                out.lines.push(Lines {
                    segments: l.segments.iter().map(|q| q.map(place)).collect(),
                    ..l.clone()
                });
            }
            for l in &d.labels {
                out.labels.push(Label {
                    origin: place(l.origin),
                    height: l.height / s,
                    ..l.clone()
                });
            }
            // The view title: detail number in a circle, name underlined, scale below.
            let (x0, y0) = (
                ((lo[0] - c[0]) / s + at[0]) / MM,
                ((lo[1] - c[1]) / s + at[1]) / MM,
            );
            let (x1, y1) = (
                ((hi[0] - c[0]) / s + at[0]) / MM,
                ((hi[1] - c[1]) / s + at[1]) / MM,
            );
            let (bx, by) = (x0 + 5.0, y0 - 10.0);
            let circle: Vec<[f32; 2]> = (0..=32)
                .map(|i| {
                    let a = i as f32 / 32.0 * std::f32::consts::TAU;
                    [bx + 5.0 * a.cos(), by + 5.0 * a.sin()]
                })
                .collect();
            for p in circle.windows(2) {
                out.line(0.25, p[0][0], p[0][1], p[1][0], p[1][1]);
            }
            let number_w = text_width(&vp.detail_number, 3.0);
            out.text(bx - number_w / 2.0, by - 1.5, 3.0, &vp.detail_number);
            let title_w = text_width(&d.name, 3.5).max(40.0);
            out.line(0.5, bx + 5.0, by, bx + 8.0 + title_w, by);
            out.text(bx + 8.0, by + 1.5, 3.5, &d.name);
            out.text(bx + 8.0, by - 4.0, 2.0, &format!("Scale 1 : {}", d.scale));
            items.push(SheetItem {
                id: vp.id,
                kind: ItemKind::Viewport,
                element: view,
                position: vp.position,
                min: [x0.min(bx - 5.0) * MM, (by - 6.0) * MM],
                max: [x1.max(bx + 8.0 + title_w) * MM, y1 * MM],
            });
        }

        // Placed schedules.
        for inst in &data.schedules {
            let Some(id) =
                reviera_core::GlobalId::parse(&inst.schedule).and_then(|g| store.find_by_guid(&g))
            else {
                continue;
            };
            let Ok(table) = self.schedule_table_on(doc, id, Some(sheet)) else {
                continue;
            };
            let (x, y) = (inst.position[0] as f32, inst.position[1] as f32);
            let (tw, th) = schedule_table(&mut out, &table, x, y);
            items.push(SheetItem {
                id: inst.id,
                kind: ItemKind::Schedule,
                element: id,
                position: inst.position,
                min: [x * MM, (y - th) * MM],
                max: [(x + tw) * MM, y * MM],
            });
        }

        // Precast shop drawings, drawn from their pieces now (STR-PRE-006).
        for item in &data.shop_drawings {
            let Some((g, size)) = reviera_core::precast_shop::sheet_graphics(store, item) else {
                continue;
            };
            for (width, a, b) in &g.lines {
                out.line(*width as f32, a[0] as f32, a[1] as f32, b[0] as f32, b[1] as f32);
            }
            for (at, height, text) in &g.texts {
                out.text(at[0] as f32, at[1] as f32, *height as f32, text);
            }
            let (x, y) = (item.position[0] as f32, item.position[1] as f32);
            items.push(SheetItem {
                id: item.id,
                kind: ItemKind::ShopDrawing,
                element: e.id(),
                position: item.position,
                min: [x * MM, (y - size[1] as f32) * MM],
                max: [(x + size[0] as f32) * MM, y * MM],
            });
        }

        title_block(&mut out, store, e, &data, &scales, w);
        // The border.
        out.rect(0.5, BINDING_MM, MARGIN_MM, w - MARGIN_MM, h - MARGIN_MM);

        let drawing = PlanDrawing {
            name: format!("{} - {}", data.number, e.name.clone().unwrap_or_default()),
            scale: 1.0,
            graphics: PlanGraphics {
                top: 1.0,
                bottom: -1.0,
                poche: out.poche,
                lines: out.lines,
                scale: 1.0,
                cut_edges: Vec::new(),
                look_up: false,
            },
            annotation: Vec::new(),
            labels: out.labels,
            paper: Some(Paper {
                size: sheets::SIZES
                    .iter()
                    .find(|s| s.0 == data.size)
                    .map_or("A1", |s| s.0),
                width: w,
                height: h,
            }),
        };
        Ok(SheetDrawing { drawing, items })
    }
}

/// Reviera's title block in the lower-right corner of a sheet `w` mm wide.
fn title_block(
    out: &mut Paperwork,
    store: &reviera_core::Store,
    sheet: &reviera_core::Element,
    data: &SheetData,
    scales: &[f32],
    w: f32,
) {
    let values: std::collections::HashMap<String, String> =
        sheets::title_values(store, sheet).into_iter().collect();
    let v = |k: &str| values.get(k).map_or("", String::as_str);
    let (x0, x1) = (w - MARGIN_MM - TITLE_W_MM, w - MARGIN_MM);
    let (y0, y1) = (MARGIN_MM, MARGIN_MM + TITLE_H_MM);
    let split = x1 - 60.0;
    out.rect(0.5, x0, y0, x1, y1);
    // Left column: project, sheet name, people, then the program and project number.
    let rows = [y1, y1 - 14.0, y1 - 28.0, y1 - 38.0, y0];
    for r in &rows[1..4] {
        out.line(0.25, x0, *r, split, *r);
    }
    out.line(0.5, split, y0, split, y1);
    out.field(x0, rows[0], "PROJECT", v("Project"), 4.0);
    out.field(x0, rows[1], "SHEET NAME", v("Sheet Name"), 4.0);
    let people = ["Drawn By", "Checked By", "Designed By", "Approved By"];
    let cell = (split - x0) / people.len() as f32;
    for (i, k) in people.iter().enumerate() {
        let x = x0 + i as f32 * cell;
        if i > 0 {
            out.line(0.25, x, rows[2], x, rows[3]);
        }
        out.field(x, rows[2], &k.to_uppercase(), v(k), 2.5);
    }
    let mid = x0 + 2.0 * cell;
    out.line(0.25, mid, rows[3], mid, y0);
    out.field(x0, rows[3], "PROJECT NUMBER", v("Project Number"), 2.5);
    out.field(mid, rows[3], "SOFTWARE", "BIM Reviera", 2.5);
    // Right column: date and scale, then the sheet number large.
    let scale = match scales {
        [] => String::new(),
        [s, rest @ ..] if rest.iter().all(|r| r == s) => format!("1 : {s}"),
        _ => "As indicated".into(),
    };
    let half = split + 30.0;
    let r = y1 - 12.0;
    out.line(0.25, split, r, x1, r);
    out.line(0.25, half, r, half, y1);
    out.field(split, y1, "DATE", v("Issue Date"), 2.5);
    out.field(half, y1, "SCALE", &scale, 2.5);
    out.text(split + 1.5, r - 3.2, 1.8, "SHEET");
    let number_w = text_width(&data.number, 10.0);
    out.text((split + x1 - number_w) / 2.0, y0 + 9.0, 10.0, &data.number);
}

/// Draws `table` with its top-left corner at (x, y) mm; returns its width and height, mm.
fn schedule_table(out: &mut Paperwork, table: &Table, x: f32, y: f32) -> (f32, f32) {
    const TEXT: f32 = 2.5;
    const ROW: f32 = 6.0;
    const TITLE_ROW: f32 = 9.0;
    const PAD: f32 = 2.0;
    let n = table.headings.len();
    let mut widths: Vec<f32> = table
        .headings
        .iter()
        .map(|t| text_width(t, TEXT) + 2.0 * PAD)
        .collect();
    for row in &table.rows {
        for (i, c) in row.cells.iter().enumerate().take(n) {
            widths[i] = widths[i].max(text_width(&c.text, TEXT) + 2.0 * PAD);
        }
    }
    if let Some(t) = &table.totals {
        for (i, c) in t.iter().enumerate().take(n) {
            widths[i] = widths[i].max(text_width(c, TEXT) + 2.0 * PAD);
        }
    }
    for w in &mut widths {
        *w = w.max(15.0);
    }
    let sum: f32 = widths.iter().sum();
    let total_w = sum.max(text_width(&table.title, 3.5) + 2.0 * PAD);
    if let Some(last) = widths.last_mut() {
        *last += total_w - sum;
    }
    let lines = 1 + table.rows.len() + usize::from(table.totals.is_some());
    let height = TITLE_ROW + lines as f32 * ROW;
    // The title, centred.
    out.text(
        x + (total_w - text_width(&table.title, 3.5)) / 2.0,
        y - TITLE_ROW + 2.75,
        3.5,
        &table.title,
    );
    let mut row_top = y - TITLE_ROW;
    let draw_row = |out: &mut Paperwork, texts: Vec<(&str, bool)>, top: f32| {
        let mut cx = x;
        for (i, (t, right)) in texts.into_iter().enumerate().take(n) {
            let tx = if right {
                cx + widths[i] - PAD - text_width(t, TEXT)
            } else {
                cx + PAD
            };
            out.text(tx, top - ROW + (ROW - TEXT) / 2.0, TEXT, t);
            cx += widths[i];
        }
    };
    draw_row(
        out,
        table.headings.iter().map(|h| (h.as_str(), false)).collect(),
        row_top,
    );
    row_top -= ROW;
    out.line(0.35, x, row_top, x + total_w, row_top);
    for row in &table.rows {
        draw_row(
            out,
            row.cells
                .iter()
                .map(|c| (c.text.as_str(), c.value.is_some()))
                .collect(),
            row_top,
        );
        row_top -= ROW;
        out.line(0.13, x, row_top, x + total_w, row_top);
    }
    if let Some(t) = &table.totals {
        out.line(0.35, x, row_top, x + total_w, row_top);
        draw_row(
            out,
            t.iter().map(|c| (c.as_str(), false)).collect(),
            row_top,
        );
    }
    // Rules: the outline, under the title, and between columns.
    out.rect(0.35, x, y - height, x + total_w, y);
    out.line(0.35, x, y - TITLE_ROW, x + total_w, y - TITLE_ROW);
    let mut cx = x;
    for w in &widths[..n.saturating_sub(1)] {
        cx += w;
        out.line(0.13, cx, y - TITLE_ROW, cx, y - height);
    }
    (total_w, height)
}
