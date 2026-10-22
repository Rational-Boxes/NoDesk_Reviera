// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Schedules (spec Ch 5 §5.14, DOC-SCH-001…005, 010, 012): tables of the elements of one
//! category, one row per element (or per group of identical rows), kept live with the model.
//!
//! A schedule is an RXD-only element (class `rxd:schedules/<guid>`, RXD document
//! `schedules/<guid>`). It is named by its element name and holds its definition in
//! `rxd:schedule`:
//! - the category;
//! - the fields, in column order;
//! - the sort levels;
//! - Itemize every instance;
//! - grand totals.
//!
//! A field is one of:
//! - a computed field (`count`, `type`, `level`, `room.area`, `insert.width`, …);
//! - any parameter, as `param:<reference>` ([`crate::parameters`]).
//!
//! Editing a parameter cell writes the parameter through the normal parameter path (undoable).
//!
//! Built so far: component schedules of one category, sorting (natural, unit-aware), itemize and
//! grand totals, editing and CSV export. Filters, grouping headers and footers, calculated
//! fields, key schedules and schedules on sheets come later.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;

pub const CLASS_PREFIX: &str = "rxd:schedules/";
pub const SCHEDULE_KEY: &str = "rxd:schedule";
pub const KIND_COMPONENT: &str = "schedule.component";

fn yes() -> bool {
    true
}

/// A column of a schedule: a field key and an optional heading (else the field's name).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Field {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SortLevel {
    /// A field key of the schedule.
    pub field: String,
    #[serde(default)]
    pub descending: bool,
}

/// A schedule's definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleDef {
    pub kind: String,
    /// The category key (`cat.doors`).
    pub category: String,
    pub fields: Vec<Field>,
    #[serde(default)]
    pub sort: Vec<SortLevel>,
    /// One row per element; when off, rows with the same sorted values are one row.
    #[serde(default = "yes")]
    pub itemize: bool,
    #[serde(default = "yes")]
    pub grand_totals: bool,
    /// A panel schedule's panel (GlobalId, MEP-ELEC-013).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<String>,
    /// Rows must pass every filter.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Filter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Equals,
    NotEquals,
    Contains,
    GreaterThan,
    LessThan,
}

/// A schedule filter: a field of the schedule compared with a value (numbers by value, text
/// ignoring case).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Filter {
    pub field: String,
    pub op: FilterOp,
    pub value: String,
}

impl Filter {
    fn passes(&self, c: &Cell) -> bool {
        use std::cmp::Ordering;
        let order = match (c.value, self.value.trim().parse::<f64>()) {
            (Some(x), Ok(y)) => x.total_cmp(&y),
            _ => c.text.to_lowercase().cmp(&self.value.to_lowercase()),
        };
        match self.op {
            FilterOp::Equals => order == Ordering::Equal,
            FilterOp::NotEquals => order != Ordering::Equal,
            FilterOp::Contains => c.text.to_lowercase().contains(&self.value.to_lowercase()),
            FilterOp::GreaterThan => order == Ordering::Greater,
            FilterOp::LessThan => order == Ordering::Less,
        }
    }
}

pub fn is_schedule(e: &Element) -> bool {
    e.class.starts_with(CLASS_PREFIX)
}

pub fn schedule_def(e: &Element) -> Option<ScheduleDef> {
    if !is_schedule(e) {
        return None;
    }
    serde_json::from_value(e.param(SCHEDULE_KEY)?.clone()).ok()
}

/// A field that can be scheduled: its key, its name, and whether its totals add up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FieldInfo {
    pub key: String,
    pub name: String,
    /// Values add up in totals (counts, lengths, areas, volumes).
    pub summed: bool,
}

fn info(key: &str, name: &str, summed: bool) -> FieldInfo {
    FieldInfo {
        key: key.into(),
        name: name.into(),
        summed,
    }
}

/// The fields a schedule of `category` can show: computed fields, then every parameter
/// definition of the category.
pub fn available_fields(store: &Store, category: &str) -> Vec<FieldInfo> {
    let mut out = vec![info("count", "Count", true)];
    match category {
        "cat.rooms" => out.extend([
            info("room.number", "Number", false),
            info("room.name", "Name", false),
            info("room.area", "Area", true),
            info("room.perimeter", "Perimeter", true),
            info("room.height", "Unbounded Height", false),
            info("room.volume", "Volume", true),
            info("room.supply_airflow", "Actual Supply Airflow", true),
            info("room.return_airflow", "Actual Return Airflow", true),
            info("room.exhaust_airflow", "Actual Exhaust Airflow", true),
        ]),
        "cat.doors" | "cat.windows" => out.extend([
            info("insert.width", "Width", false),
            info("insert.height", "Height", false),
            info("insert.sill", "Sill Height", false),
        ]),
        "cat.walls" => out.extend([
            info("wall.length", "Length", true),
            info("wall.width", "Width", false),
            info("wall.height", "Unconnected Height", false),
            info("wall.area", "Area", true),
        ]),
        "cat.floors" | "cat.ceilings" => out.extend([
            info("slab.area", "Area", true),
            info("slab.perimeter", "Perimeter", true),
            info("slab.thickness", "Thickness", false),
            info("slab.volume", "Volume", true),
        ]),
        // Components: the room their calculation point is in (MEP-ELEC-001).
        "cat.lighting_fixtures"
        | "cat.electrical_fixtures"
        | "cat.electrical_equipment"
        | "cat.switching_devices"
        | "cat.alarms"
        | "cat.sensors"
        | "cat.air_terminals"
        | "cat.mechanical_equipment"
        | "cat.plumbing_fixtures"
        | "cat.furniture" => out.push(info("component.room", "Room", false)),
        "cat.roofs" => out.extend([
            info("roof.area", "Area", true),
            info("roof.volume", "Volume", true),
            info("roof.slope", "Slope", false),
        ]),
        // Ducts and pipes with their kept network results (MEP-CALC-002).
        "cat.ducts" | "cat.pipes" => out.extend([
            info("mep.size", "Size", false),
            info("mep.length", "Length", true),
            info("mep.flow", "Flow", false),
            info("mep.velocity", "Velocity", false),
            info("mep.friction", "Friction", false),
            info("mep.pressure_drop", "Pressure Drop", true),
            info("mep.network", "Network ID", false),
            info("mep.critical_path", "Critical Path", false),
        ]),
        // Rebar sets (STR-RBR-018): number, shape, segments A–R, quantity, lengths and mass.
        "cat.structural_rebar" => {
            out.extend(crate::rebar_docs::FIELDS.iter().map(|(k, n, sum)| info(k, n, *sum)));
            out.extend(
                crate::rebar_docs::SEGMENTS
                    .chars()
                    .map(|c| info(&format!("rebar.{c}"), &c.to_string(), false)),
            );
        }
        _ => {}
    }
    if category != "cat.rooms" {
        out.push(info("type", "Type", false));
    }
    out.extend([
        info("level", "Level", false),
        info("ifc_class", "IFC Class", false),
        info("guid", "IFC GlobalId", false),
    ]);
    if let Ok(defs) = crate::parameters::list(store, Some(category), None) {
        for d in defs {
            if out.iter().any(|f| f.name == d.name) {
                continue;
            }
            out.push(info(&format!("param:{}", d.reference), &d.name, false));
        }
    }
    out
}

/// The default columns of a new schedule of `category`.
pub fn default_fields(category: &str) -> Vec<&'static str> {
    match category {
        "cat.rooms" => vec![
            "room.number",
            "room.name",
            "level",
            "room.area",
            "room.perimeter",
            "room.volume",
        ],
        "cat.doors" => vec![
            "param:elem.mark",
            "type",
            "level",
            "insert.width",
            "insert.height",
        ],
        "cat.windows" => vec![
            "param:elem.mark",
            "type",
            "level",
            "insert.width",
            "insert.height",
            "insert.sill",
        ],
        "cat.walls" => vec![
            "type",
            "level",
            "wall.length",
            "wall.width",
            "wall.height",
            "wall.area",
        ],
        "cat.floors" | "cat.ceilings" => vec![
            "type",
            "level",
            "slab.area",
            "slab.thickness",
            "slab.volume",
        ],
        "cat.roofs" => vec!["type", "level", "roof.slope", "roof.area", "roof.volume"],
        "cat.structural_rebar" => vec![
            "rebar.number",
            "rebar.shape",
            "rebar.bar_type",
            "rebar.A",
            "rebar.B",
            "rebar.C",
            "rebar.D",
            "rebar.quantity",
            "rebar.total_length",
            "rebar.mass",
        ],
        _ => vec!["type", "level", "count"],
    }
}

/// One cell: its text, its number when it has one (for sorting and totals), and whether it can
/// be edited.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Cell {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Row {
    /// The elements of the row (several when itemize is off).
    pub elements: Vec<ElementId>,
    pub cells: Vec<Cell>,
}

/// A schedule's table as it is now (`schedule.get_table`).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Table {
    pub title: String,
    pub fields: Vec<String>,
    pub headings: Vec<String>,
    pub rows: Vec<Row>,
    /// The grand totals row, when on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totals: Option<Vec<String>>,
}

/// What a computed field needs about an element, found once per table.
struct Context<'a> {
    store: &'a Store,
    parents: &'a HashMap<ElementId, ElementId>,
    rooms: HashMap<ElementId, crate::rooms::RoomGeometry>,
    walls: crate::walls::joins::Solved,
    settings: crate::units::UnitSettings,
    /// Rebar sets' info, worked out once (STR-RBR-018).
    rebar: HashMap<ElementId, crate::rebar::RebarInfo>,
    /// Rooms' air terminal flows (MEP-DUCT-009).
    airflow: std::collections::BTreeMap<ElementId, crate::mep_equipment::SpaceAirflow>,
}

/// Reads field values of elements as schedules show them (tags label with them, DOC-ANN-010).
pub struct FieldReader<'a>(Context<'a>);

impl std::fmt::Debug for FieldReader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FieldReader")
    }
}

impl<'a> FieldReader<'a> {
    /// `parents` is the model tree (for the Level field); an empty map leaves Level blank.
    pub fn new(store: &'a Store, parents: &'a HashMap<ElementId, ElementId>) -> Self {
        FieldReader(Context::new(store, parents))
    }

    /// The value of field `key` (`type`, `room.area`, `param:elem.mark`, …) of `e`.
    pub fn cell(&self, e: &Element, key: &str) -> Cell {
        self.0.cell(e, key)
    }
}

impl<'a> Context<'a> {
    fn new(store: &'a Store, parents: &'a HashMap<ElementId, ElementId>) -> Self {
        Context {
            store,
            parents,
            rooms: crate::rooms::all(store)
                .into_iter()
                .map(|r| (r.id, r.geometry))
                .collect(),
            walls: crate::walls::solve(store),
            settings: crate::units::settings_of(store),
            rebar: crate::rebar_docs::infos(store),
            airflow: crate::mep_equipment::airflows(store),
        }
    }
}

impl Context<'_> {
    /// Ducts' and pipes' fields: size and length, then the kept network results.
    fn mep(&self, e: &Element, key: &str) -> Cell {
        let Some(s) = crate::mep_runs::segment_recipe(e) else {
            return Context::text("");
        };
        let kept = |f: &str| crate::mep_network::kept_value(e, f);
        let number = |f: &str, unit: &str, places: usize| match kept(f).and_then(Value::as_f64) {
            Some(v) => Cell { text: format!("{v:.places$} {unit}"), value: Some(v), editable: false },
            None => Context::text(""),
        };
        match key {
            "mep.size" => Context::text(s.size.text()),
            "mep.length" => {
                self.fmt(crate::mep_runs::segment_length(&s), "length")
            }
            "mep.flow" => number("flow", "m³/s", 4),
            "mep.velocity" => number("velocity", "m/s", 2),
            "mep.friction" => number("friction", "Pa/m", 2),
            "mep.pressure_drop" => number("loss", "Pa", 1),
            "mep.network" => match kept("network_id").and_then(Value::as_u64) {
                Some(n) => Cell { text: n.to_string(), value: Some(n as f64), editable: false },
                None => Context::text(""),
            },
            "mep.critical_path" => match kept("critical").and_then(Value::as_bool) {
                Some(c) => Context::text(if c { "Yes" } else { "No" }),
                None => Context::text(""),
            },
            _ => Context::text(""),
        }
    }

    fn fmt(&self, v: f64, data_type: &str) -> Cell {
        Cell {
            text: crate::units::format(v, data_type, &self.settings, None)
                .unwrap_or_else(|_| format!("{v:.3}")),
            value: Some(v),
            editable: false,
        }
    }

    fn text(s: impl Into<String>) -> Cell {
        Cell {
            text: s.into(),
            value: None,
            editable: false,
        }
    }

    /// The storey an element is on: its own data, else up the model tree.
    fn level(&self, e: &Element) -> Option<String> {
        let mut id = e.id();
        for _ in 0..8 {
            id = *self.parents.get(&id)?;
            let p = self.store.get(id)?;
            if p.class == crate::levels::CLASS {
                return p.name.clone();
            }
        }
        None
    }

    fn cell(&self, e: &Element, key: &str) -> Cell {
        let none = || Context::text("");
        match key {
            "count" => Cell {
                text: "1".into(),
                value: Some(1.0),
                editable: false,
            },
            "type" => Context::text(
                e.param("@type")
                    .and_then(Value::as_str)
                    .and_then(GlobalId::parse)
                    .and_then(|g| self.store.find_by_guid(&g))
                    .and_then(|t| self.store.get(t)?.name.clone())
                    .unwrap_or_default(),
            ),
            "level" => Context::text(self.level(e).unwrap_or_default()),
            "ifc_class" => Context::text(e.class.clone()),
            "guid" => Context::text(e.guid().as_str()),
            "room.number" | "room.name" => {
                let (number, name) = crate::rooms::number_and_name(e);
                Cell {
                    text: if key == "room.number" { number } else { name },
                    value: None,
                    editable: crate::rooms::room_recipe(e).is_some(),
                }
            }
            "component.room" => match crate::mep_lighting::room_of(self.store, e).and_then(|r| self.store.get(r)) {
                Some(r) => {
                    let (number, name) = crate::rooms::number_and_name(r);
                    Context::text(format!("{number} {name}").trim().to_owned())
                }
                None => none(),
            },
            "room.supply_airflow" | "room.return_airflow" | "room.exhaust_airflow" => {
                let a = self.airflow.get(&e.id());
                let v = a.map_or(0.0, |a| match key {
                    "room.supply_airflow" => a.supply,
                    "room.return_airflow" => a.return_air,
                    _ => a.exhaust,
                }) * 1000.0;
                Cell { text: format!("{v:.1} L/s"), value: Some(v), editable: false }
            }
            "room.area" | "room.perimeter" | "room.height" | "room.volume" => {
                let Some(g) = self.rooms.get(&e.id()) else {
                    return none();
                };
                if !g.enclosed() && key != "room.height" {
                    return Context::text("Not Enclosed");
                }
                match key {
                    "room.area" => self.fmt(g.area(), "area"),
                    "room.perimeter" => self.fmt(g.perimeter(), "length"),
                    "room.height" => self.fmt(g.unbounded_height(), "length"),
                    _ => self.fmt(g.volume(), "volume"),
                }
            }
            "insert.width" | "insert.height" | "insert.sill" => {
                match crate::inserts::resolve(self.store, e) {
                    Ok(p) => match key {
                        "insert.width" => self.fmt(p.ty.width, "length"),
                        "insert.height" => self.fmt(p.ty.height, "length"),
                        _ => self.fmt(p.recipe.sill, "length"),
                    },
                    Err(_) => none(),
                }
            }
            "wall.length" | "wall.width" | "wall.height" | "wall.area" => {
                match self.walls.bodies.get(&e.id()) {
                    Some(b) => {
                        let g = &b.geometry;
                        match key {
                            "wall.length" => self.fmt(g.length(), "length"),
                            "wall.width" => self.fmt(g.width, "length"),
                            "wall.height" => self.fmt(g.height, "length"),
                            _ => self.fmt(g.length() * g.height, "area"),
                        }
                    }
                    None => none(),
                }
            }
            "slab.area" | "slab.perimeter" | "slab.thickness" | "slab.volume" => {
                match crate::floors::regenerate(self.store, e) {
                    Ok(g) => match key {
                        "slab.area" => self.fmt(g.area(), "area"),
                        "slab.perimeter" => self.fmt(g.perimeter(), "length"),
                        "slab.thickness" => self.fmt(g.thickness, "length"),
                        _ => self.fmt(g.volume(), "volume"),
                    },
                    Err(_) => none(),
                }
            }
            "roof.area" | "roof.volume" | "roof.slope" => {
                match (
                    crate::roofs::regenerate(self.store, e),
                    crate::roofs::roof_recipe(e),
                ) {
                    (Ok(g), Some(r)) => match key {
                        "roof.area" => self.fmt(g.area(), "area"),
                        "roof.volume" => self.fmt(g.volume(), "volume"),
                        _ => self.fmt(r.slope, "angle"),
                    },
                    _ => none(),
                }
            }
            k if k.starts_with("mep.") => self.mep(e, k),
            k if k.starts_with("rebar.") => match self.rebar.get(&e.id()) {
                Some(i) => match crate::rebar_docs::field(self.store, i, k) {
                    Some((text, value)) => Cell {
                        text,
                        value,
                        editable: false,
                    },
                    None => none(),
                },
                None => none(),
            },
            other => match other.strip_prefix("param:") {
                Some(reference) => match crate::parameters::get(self.store, e.id(), reference) {
                    Ok(v) => Cell {
                        text: v.display.unwrap_or_default(),
                        value: v.value.as_f64(),
                        editable: !v.read_only,
                    },
                    Err(_) => none(),
                },
                None => none(),
            },
        }
    }
}

/// Compares two cells: numbers by value, text naturally ("Door 2" before "Door 10").
fn compare(a: &Cell, b: &Cell) -> std::cmp::Ordering {
    if let (Some(x), Some(y)) = (a.value, b.value) {
        return x.total_cmp(&y);
    }
    natural(&a.text, &b.text)
}

/// Natural order: runs of digits compare as numbers, the rest case-insensitively.
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut n = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    n.push(c);
                    x.next();
                }
                let mut m = String::new();
                while let Some(d) = y.peek().copied().filter(char::is_ascii_digit) {
                    m.push(d);
                    y.next();
                }
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = n.len().cmp(&m.len()).then(n.cmp(m));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// The table of a schedule, with the model tree's parents (to find elements' levels).
pub fn table(
    store: &Store,
    schedule: &Element,
    parents: &HashMap<ElementId, ElementId>,
) -> Result<Table> {
    let def =
        schedule_def(schedule).ok_or_else(|| Error::new("schedule.not_found", "not a schedule"))?;
    if def.kind == crate::mep_panel_schedule::KIND_PANEL {
        return crate::mep_panel_schedule::table(store, schedule, &def);
    }
    let elements: Vec<&Element> = store
        .iter()
        .filter(|e| !crate::categories::is_type(e) && crate::inspect::in_tree(e))
        .filter(|e| crate::categories::category_of(e).is_some_and(|c| c.key == def.category))
        .collect();
    let cx = Context::new(store, parents);
    let names: HashMap<String, FieldInfo> = available_fields(store, &def.category)
        .into_iter()
        .map(|f| (f.key.clone(), f))
        .collect();
    let headings: Vec<String> = def
        .fields
        .iter()
        .map(|f| {
            f.heading
                .clone()
                .or_else(|| names.get(&f.key).map(|i| i.name.clone()))
                .unwrap_or_else(|| f.key.clone())
        })
        .collect();
    let mut rows: Vec<Row> = elements
        .iter()
        .filter(|e| def.filters.iter().all(|f| f.passes(&cx.cell(e, &f.field))))
        .map(|e| Row {
            elements: vec![e.id()],
            cells: def.fields.iter().map(|f| cx.cell(e, &f.key)).collect(),
        })
        .collect();
    // Sort by the sort levels, then by element order for stability.
    let sort_cols: Vec<(usize, bool)> = def
        .sort
        .iter()
        .filter_map(|s| {
            def.fields
                .iter()
                .position(|f| f.key == s.field)
                .map(|i| (i, s.descending))
        })
        .collect();
    rows.sort_by(|a, b| {
        for (i, desc) in &sort_cols {
            let o = compare(&a.cells[*i], &b.cells[*i]);
            let o = if *desc { o.reverse() } else { o };
            if o != std::cmp::Ordering::Equal {
                return o;
            }
        }
        a.elements.cmp(&b.elements)
    });
    let summed = |i: usize| names.get(&def.fields[i].key).is_some_and(|f| f.summed);
    if !def.itemize {
        // Rows with the same values in every sorted field are one row: differing cells are
        // blank, summed fields add up.
        let mut grouped: Vec<Row> = Vec::new();
        for r in rows {
            let same = grouped.last().is_some_and(|g: &Row| {
                sort_cols
                    .iter()
                    .all(|(i, _)| g.cells[*i].text == r.cells[*i].text)
            });
            if !same {
                grouped.push(r);
                continue;
            }
            let g = grouped.last_mut().expect("checked");
            g.elements.extend(r.elements);
            for (i, c) in r.cells.into_iter().enumerate() {
                let gc = &mut g.cells[i];
                if summed(i) {
                    if let (Some(a), Some(b)) = (gc.value, c.value) {
                        gc.value = Some(a + b);
                    }
                } else if gc.text != c.text {
                    *gc = Context::text("");
                }
            }
        }
        // Summed cells show their sums.
        for g in &mut grouped {
            for (i, c) in g.cells.iter_mut().enumerate() {
                if summed(i)
                    && let Some(v) = c.value
                {
                    *c = match def.fields[i].key.as_str() {
                        "count" => Cell {
                            text: format!("{v}"),
                            value: Some(v),
                            editable: false,
                        },
                        k if k.starts_with("rebar.") => Cell {
                            text: crate::rebar_docs::total_text(k, v),
                            value: Some(v),
                            editable: false,
                        },
                        k if k.ends_with("area") => cx.fmt(v, "area"),
                        k if k.ends_with("volume") => cx.fmt(v, "volume"),
                        _ => cx.fmt(v, "length"),
                    };
                }
            }
        }
        rows = grouped;
    }
    let totals = def.grand_totals.then(|| {
        let n: usize = rows.iter().map(|r| r.elements.len()).sum();
        (0..def.fields.len())
            .map(|i| {
                if i == 0 {
                    return format!("Grand total: {n}");
                }
                if !summed(i) {
                    return String::new();
                }
                let sum: f64 = rows.iter().filter_map(|r| r.cells[i].value).sum();
                match def.fields[i].key.as_str() {
                    "count" => format!("{n}"),
                    k if k.starts_with("rebar.") => crate::rebar_docs::total_text(k, sum),
                    k if k.ends_with("area") => cx.fmt(sum, "area").text,
                    k if k.ends_with("volume") => cx.fmt(sum, "volume").text,
                    _ => cx.fmt(sum, "length").text,
                }
            })
            .collect()
    });
    Ok(Table {
        title: schedule.name.clone().unwrap_or_default(),
        fields: def.fields.iter().map(|f| f.key.clone()).collect(),
        headings,
        rows,
        totals,
    })
}

/// A table as delimited text (DOC-SCH-012): title, headings, rows and totals, with text that a
/// spreadsheet would read as a formula escaped.
pub fn delimited(t: &Table, delimiter: char) -> String {
    let quote = |s: &str| {
        let s = if s.starts_with(['=', '+', '-', '@']) && s.parse::<f64>().is_err() {
            format!("'{s}")
        } else {
            s.to_owned()
        };
        if s.contains(delimiter) || s.contains('"') || s.contains('\n') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s
        }
    };
    let line = |cells: Vec<String>| {
        cells
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(&delimiter.to_string())
    };
    let mut out = vec![quote(&t.title), line(t.headings.clone())];
    out.extend(
        t.rows
            .iter()
            .map(|r| line(r.cells.iter().map(|c| c.text.clone()).collect())),
    );
    if let Some(totals) = &t.totals {
        out.push(line(totals.clone()));
    }
    out.join("\n") + "\n"
}

fn resolve(store: &Store, r: &ElementRef) -> Result<(ElementId, ScheduleDef)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("schedule.not_found", "no such schedule"))?;
    let def = schedule_def(store.get(id).expect("resolved")).ok_or_else(|| {
        Error::new(
            "schedule.not_found",
            format!("element {id} is not a schedule"),
        )
    })?;
    if def.kind == crate::mep_panel_schedule::KIND_PANEL {
        return Err(Error::new(
            "schedule.panel_schedule",
            "a panel schedule's columns are set by its template",
        ));
    }
    Ok((id, def))
}

fn write(txn: &mut crate::txn::Transaction<'_>, id: ElementId, def: &ScheduleDef) -> Result<()> {
    let value = serde_json::to_value(def).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(SCHEDULE_KEY.into(), value);
    })
}

fn check_fields(store: &Store, category: &str, keys: &[String]) -> Result<()> {
    let known = available_fields(store, category);
    for k in keys {
        if !known.iter().any(|f| &f.key == k) {
            return Err(Error::new(
                "schedule.unknown_field",
                format!("`{k}` is not a field of this category"),
            ));
        }
    }
    Ok(())
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The category (`cat.doors`, `cat.rooms`, …).
    pub category: String,
    /// Default: the category's name, then `… 2`, `… 3`.
    pub name: Option<String>,
    /// Field keys in column order; default the category's usual fields.
    pub fields: Option<Vec<String>>,
}

pub(crate) fn create(txn: &mut crate::txn::Transaction<'_>, p: CreateParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let cat = crate::categories::category(&p.category).ok_or_else(|| {
        Error::new(
            "schedule.unknown_category",
            format!("no category `{}`", p.category),
        )
    })?;
    let keys: Vec<String> = p.fields.unwrap_or_else(|| {
        default_fields(&p.category)
            .into_iter()
            .map(String::from)
            .collect()
    });
    check_fields(&store, &p.category, &keys)?;
    let base = p
        .name
        .unwrap_or_else(|| format!("{} Schedule", cat.name.trim_end_matches('s')));
    let existing: Vec<String> = store
        .iter()
        .filter(|e| is_schedule(e))
        .filter_map(|e| e.name.clone())
        .collect();
    let name = std::iter::once(base.clone())
        .chain((2..).map(|n| format!("{base} {n}")))
        .find(|n| !existing.contains(n))
        .expect("a free name");
    // Sorted by the first field by default.
    let sort = keys
        .first()
        .map(|k| {
            vec![SortLevel {
                field: k.clone(),
                descending: false,
            }]
        })
        .unwrap_or_default();
    let def = ScheduleDef {
        kind: KIND_COMPONENT.into(),
        category: p.category,
        fields: keys
            .into_iter()
            .map(|key| Field { key, heading: None })
            .collect(),
        sort,
        itemize: true,
        grand_totals: true,
        panel: None,
        filters: Vec::new(),
    };
    let guid = GlobalId::new();
    txn.create(
        NewElement::new(format!("{CLASS_PREFIX}{}", guid.as_str()))
            .guid(guid)
            .name(name)
            .param(
                SCHEDULE_KEY,
                serde_json::to_value(&def).expect("serialises"),
            ),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetFieldsParams {
    pub schedule_id: ElementRef,
    /// Field keys in column order.
    pub fields: Vec<String>,
}

/// Sets a schedule's columns; sort levels on removed fields go with them.
pub(crate) fn set_fields(txn: &mut crate::txn::Transaction<'_>, p: SetFieldsParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut def) = resolve(&store, &p.schedule_id)?;
    if p.fields.is_empty() {
        return Err(Error::new(
            "schedule.no_fields",
            "a schedule needs at least one field",
        ));
    }
    check_fields(&store, &def.category, &p.fields)?;
    let old: HashMap<String, Option<String>> = def
        .fields
        .iter()
        .map(|f| (f.key.clone(), f.heading.clone()))
        .collect();
    def.fields = p
        .fields
        .into_iter()
        .map(|key| Field {
            heading: old.get(&key).cloned().flatten(),
            key,
        })
        .collect();
    def.sort
        .retain(|s| def.fields.iter().any(|f| f.key == s.field));
    write(txn, id, &def)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSortParams {
    pub schedule_id: ElementRef,
    /// Sort levels: `Sort by`, then each `Then by`.
    pub levels: Vec<SortLevel>,
    pub itemize: Option<bool>,
    pub grand_totals: Option<bool>,
}

pub(crate) fn set_sort(txn: &mut crate::txn::Transaction<'_>, p: SetSortParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut def) = resolve(&store, &p.schedule_id)?;
    for l in &p.levels {
        if !def.fields.iter().any(|f| f.key == l.field) {
            return Err(Error::new(
                "schedule.unknown_field",
                format!("`{}` is not a field of the schedule", l.field),
            ));
        }
    }
    def.sort = p.levels;
    if let Some(i) = p.itemize {
        def.itemize = i;
    }
    if let Some(g) = p.grand_totals {
        def.grand_totals = g;
    }
    write(txn, id, &def)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetFiltersParams {
    pub schedule_id: ElementRef,
    /// Every filter must pass; none: every element.
    pub filters: Vec<Filter>,
}

/// Sets a schedule's filters (fields of its category, shown or not).
pub(crate) fn set_filters(txn: &mut crate::txn::Transaction<'_>, p: SetFiltersParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut def) = resolve(&store, &p.schedule_id)?;
    let known = available_fields(&store, &def.category);
    if let Some(f) = p.filters.iter().find(|f| !known.iter().any(|k| k.key == f.field)) {
        return Err(Error::new("schedule.unknown_field", format!("`{}` is not a field of {}", f.field, def.category)));
    }
    def.filters = p.filters;
    write(txn, id, &def)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCellParams {
    /// The row's elements (all of a grouped row).
    pub element_ids: Vec<ElementRef>,
    /// The column's field key.
    pub field: String,
    /// The new value (text with units for measures).
    pub value: Value,
}

/// Edits a cell: writes the field's parameter on the row's elements (DOC-SCH-010).
pub(crate) fn set_cell(txn: &mut crate::txn::Transaction<'_>, p: SetCellParams) -> Result<()> {
    let store = txn.store().clone();
    let ids: Vec<ElementId> = p
        .element_ids
        .iter()
        .map(|r| {
            store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", "no such element"))
        })
        .collect::<Result<_>>()?;
    match p.field.as_str() {
        "room.number" | "room.name" => {
            let text = match &p.value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let key = if p.field == "room.number" {
                "number"
            } else {
                "name"
            };
            let mut args = json!({ "room_ids": ids });
            args[key] = json!(text);
            let params: crate::rooms::SetParams = serde_json::from_value(args)
                .map_err(|e| Error::new("schedule.bad_value", e.to_string()))?;
            crate::rooms::set(txn, params)
        }
        "circuit.load_name" | "circuit.rating" => {
            let text = match &p.value {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            for id in &ids {
                let mut params = crate::mep_electrical::CircuitSetParams {
                    circuit_id: ElementRef::Id(*id),
                    load_name: None,
                    rating: None,
                    cable_type: None,
                    cable_size: None,
                    runs: None,
                };
                if p.field == "circuit.load_name" {
                    params.load_name = Some(text.clone());
                } else {
                    let t = text.trim().trim_end_matches(['A', 'a']).trim();
                    params.rating = Some(if t.is_empty() {
                        0.0
                    } else {
                        t.parse().map_err(|_| {
                            Error::new("schedule.bad_value", format!("{text:?} is not a rating in amperes"))
                        })?
                    });
                }
                crate::mep_electrical::set_circuit(txn, params)?;
            }
            Ok(())
        }
        f => match f.strip_prefix("param:") {
            Some(reference) => {
                crate::parameters::set(txn, &ids, reference, Some(&p.value)).map(|_| ())
            }
            None => Err(Error::new(
                "schedule.read_only_field",
                format!("`{f}` is computed and cannot be edited"),
            )),
        },
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldsParams {
    pub category: String,
}

/// The fields a category's schedules can show.
pub(crate) fn fields(store: &Store, p: FieldsParams) -> Vec<FieldInfo> {
    available_fields(store, &p.category)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_compares_numbers_by_value() {
        let mut v = vec!["Door 10", "door 2", "Door 1", "A", "Door 2a"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["A", "Door 1", "door 2", "Door 2a", "Door 10"]);
    }

    #[test]
    fn delimited_text_escapes_formulas_and_quotes() {
        let t = Table {
            title: "Rooms".into(),
            fields: vec!["a".into(), "b".into()],
            headings: vec!["Name".into(), "Area".into()],
            rows: vec![Row {
                elements: vec![],
                cells: vec![
                    Cell {
                        text: "=SUM(1)".into(),
                        value: None,
                        editable: true,
                    },
                    Cell {
                        text: "1,5 m²".into(),
                        value: Some(1.5),
                        editable: false,
                    },
                ],
            }],
            totals: Some(vec!["Grand total: 1".into(), "1,5 m²".into()]),
        };
        let csv = delimited(&t, ',');
        assert_eq!(
            csv,
            "Rooms\nName,Area\n'=SUM(1),\"1,5 m²\"\nGrand total: 1,\"1,5 m²\"\n"
        );
    }
}
