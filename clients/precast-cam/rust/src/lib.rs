// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! A neutral precast piece model for production systems, and CAM filename rules.
//!
//! A [`Piece`] describes one precast element as a plant needs it: its outline and openings, its
//! embedded parts, its bars and welded fabric, and its marks. Coordinates are millimetres in the
//! piece's production system: the origin at the lower-left corner of the piece's front face, X
//! along its length, Y up its height (across, for a slab), Z into the piece from the front face.
//!
//! [`json`] and [`csv`] write a piece; a CAM writer of another format receives the piece as JSON
//! (the same document) and returns the file. [`FilenameRule`] names the files.
//!
//! This crate knows nothing about any model. It is MIT-licensed so any program can use it.

use serde::{Deserialize, Serialize};

/// The neutral document's schema version.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Piece {
    pub schema: u32,
    /// The piece mark, its name and its production number.
    pub mark: String,
    pub name: String,
    pub prod_no: u32,
    /// `solid_wall`, `double_wall`, `solid_slab`, `hollow_core` or `lattice_girder`.
    pub kind: String,
    pub length: f64,
    pub height: f64,
    pub thickness: f64,
    pub volume_m3: f64,
    pub weight_kg: f64,
    /// The outline (counter-clockwise, X and Y) and the openings through it.
    pub outline: Vec<[f64; 2]>,
    pub openings: Vec<Vec<[f64; 2]>>,
    pub embeds: Vec<Embed>,
    pub bars: Vec<BarSet>,
    pub meshes: Vec<Mesh>,
}

/// A part cast in: a lifting anchor, bushing, connector …
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Embed {
    pub kind: String,
    pub name: String,
    /// Its head (X, Y, Z) and the way it runs into the piece.
    pub position: [f64; 3],
    pub direction: [f64; 3],
    pub diameter: f64,
    pub length: f64,
}

/// A set of bars: its number, shape code, bar and each bar's centreline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarSet {
    pub number: String,
    pub shape: String,
    pub bar_type: String,
    pub diameter: f64,
    pub bars: Vec<Vec<[f64; 3]>>,
}

/// A welded fabric sheet: its number and wires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub number: String,
    pub wires: Vec<Wire>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub diameter: f64,
    pub start: [f64; 3],
    pub end: [f64; 3],
}

/// The piece as a JSON document.
pub fn json(p: &Piece) -> String {
    serde_json::to_string_pretty(p).expect("a piece serialises")
}

/// The piece as CSV: one record a line, its kind first.
///
/// `piece,mark,name,prod_no,kind,length,height,thickness,volume_m3,weight_kg`; `outline,i,x,y`;
/// `opening,k,i,x,y`; `embed,kind,name,x,y,z,dx,dy,dz,diameter,length`;
/// `bar,number,shape,bar_type,diameter,bar,i,x,y,z`; `wire,mesh,number,diameter,x0,y0,z0,x1,y1,z1`.
pub fn csv(p: &Piece) -> String {
    let q = |s: &str| {
        if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
    };
    let n = |v: f64| {
        let r = (v * 10.0).round() / 10.0;
        if r == r.trunc() { format!("{r:.0}") } else { format!("{r:.1}") }
    };
    let mut out = String::new();
    out += &format!(
        "piece,{},{},{},{},{},{},{},{:.4},{:.1}\n",
        q(&p.mark),
        q(&p.name),
        p.prod_no,
        q(&p.kind),
        n(p.length),
        n(p.height),
        n(p.thickness),
        p.volume_m3,
        p.weight_kg
    );
    for (i, v) in p.outline.iter().enumerate() {
        out += &format!("outline,{i},{},{}\n", n(v[0]), n(v[1]));
    }
    for (k, o) in p.openings.iter().enumerate() {
        for (i, v) in o.iter().enumerate() {
            out += &format!("opening,{k},{i},{},{}\n", n(v[0]), n(v[1]));
        }
    }
    for e in &p.embeds {
        out += &format!(
            "embed,{},{},{},{},{},{},{},{},{},{}\n",
            q(&e.kind),
            q(&e.name),
            n(e.position[0]),
            n(e.position[1]),
            n(e.position[2]),
            e.direction[0],
            e.direction[1],
            e.direction[2],
            n(e.diameter),
            n(e.length)
        );
    }
    for s in &p.bars {
        for (b, bar) in s.bars.iter().enumerate() {
            for (i, v) in bar.iter().enumerate() {
                out += &format!("bar,{},{},{},{},{b},{i},{},{},{}\n", q(&s.number), q(&s.shape), q(&s.bar_type), n(s.diameter), n(v[0]), n(v[1]), n(v[2]));
            }
        }
    }
    for (m, mesh) in p.meshes.iter().enumerate() {
        for w in &mesh.wires {
            out += &format!(
                "wire,{m},{},{},{},{},{},{},{},{}\n",
                q(&mesh.number),
                n(w.diameter),
                n(w.start[0]),
                n(w.start[1]),
                n(w.start[2]),
                n(w.end[0]),
                n(w.end[1]),
                n(w.end[2])
            );
        }
    }
    out
}

// ---- Filename rules ----

/// The variables a filename rule may use.
pub const VARIABLES: [&str; 5] = ["Name", "ProdNo", "Date", "Time", "Mark"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// `{Var:Length:MinLength:Pad:Align}`: at most `length` characters, at least `min` padded with
/// `pad` on the side away from `align`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub var: String,
    pub length: Option<usize>,
    pub min: Option<usize>,
    pub pad: char,
    pub align: Align,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    Field(Field),
}

/// A malformed rule: where (characters from 0) and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleError {
    pub position: usize,
    pub message: String,
}

impl std::fmt::Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "at character {}: {}", self.position + 1, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilenameRule {
    pub parts: Vec<Part>,
}

/// The values a rule's variables take for one file.
#[derive(Debug, Clone, Default)]
pub struct Values {
    pub name: String,
    pub prod_no: u32,
    /// `YYYYMMDD` and `HHMMSS`.
    pub date: String,
    pub time: String,
    pub mark: String,
}

impl FilenameRule {
    pub fn parse(rule: &str) -> Result<Self, RuleError> {
        let chars: Vec<char> = rule.chars().collect();
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '{' => {
                    let start = i;
                    let end = chars[i..].iter().position(|c| *c == '}').map(|k| i + k).ok_or_else(|| RuleError { position: start, message: "a field opened with { is not closed".into() })?;
                    let inner: String = chars[i + 1..end].iter().collect();
                    let cells: Vec<&str> = inner.split(':').collect();
                    if cells.len() > 5 {
                        return Err(RuleError { position: start, message: "a field has at most five parts: Var:Length:MinLength:Pad:Align".into() });
                    }
                    if !VARIABLES.contains(&cells[0]) {
                        return Err(RuleError { position: start + 1, message: format!("unknown variable {:?} (Name, ProdNo, Date, Time, Mark)", cells[0]) });
                    }
                    // Where each cell starts, for errors.
                    let mut at = start + 1;
                    let mut starts = Vec::new();
                    for c in &cells {
                        starts.push(at);
                        at += c.chars().count() + 1;
                    }
                    let number = |k: usize| -> Result<Option<usize>, RuleError> {
                        match cells.get(k).copied() {
                            None | Some("") => Ok(None),
                            Some(c) => c.parse::<usize>().map(Some).map_err(|_| RuleError { position: starts[k], message: format!("{c:?} is not a whole number") }),
                        }
                    };
                    let length = number(1)?;
                    let min = number(2)?;
                    if let (Some(l), Some(m)) = (length, min)
                        && m > l
                    {
                        return Err(RuleError { position: starts[2], message: "the minimum length is more than the length".into() });
                    }
                    let pad = match cells.get(3).copied() {
                        None | Some("") => ' ',
                        Some(c) if c.chars().count() == 1 => c.chars().next().expect("one"),
                        Some(c) => return Err(RuleError { position: starts[3], message: format!("the padding {c:?} is not one character") }),
                    };
                    let align = match cells.get(4).copied() {
                        None | Some("") | Some("left") => Align::Left,
                        Some("right") => Align::Right,
                        Some(c) => return Err(RuleError { position: starts[4], message: format!("the alignment {c:?} is left or right") }),
                    };
                    if !text.is_empty() {
                        parts.push(Part::Text(std::mem::take(&mut text)));
                    }
                    parts.push(Part::Field(Field { var: cells[0].to_owned(), length, min, pad, align }));
                    i = end + 1;
                }
                '}' => return Err(RuleError { position: i, message: "a } closes no field".into() }),
                c if "/\\\0".contains(c) => return Err(RuleError { position: i, message: format!("{c:?} cannot be in a file name") }),
                c => {
                    text.push(c);
                    i += 1;
                }
            }
        }
        if !text.is_empty() {
            parts.push(Part::Text(text));
        }
        Ok(Self { parts })
    }

    /// The name for one file (without the extension).
    pub fn render(&self, v: &Values) -> String {
        let mut out = String::new();
        for p in &self.parts {
            match p {
                Part::Text(t) => out += t,
                Part::Field(f) => {
                    let raw = match f.var.as_str() {
                        "Name" => v.name.clone(),
                        "ProdNo" => v.prod_no.to_string(),
                        "Date" => v.date.clone(),
                        "Time" => v.time.clone(),
                        _ => v.mark.clone(),
                    };
                    let mut s: String = match f.length {
                        Some(l) => raw.chars().take(l).collect(),
                        None => raw,
                    };
                    if let Some(m) = f.min {
                        let fill: String = std::iter::repeat_n(f.pad, m.saturating_sub(s.chars().count())).collect();
                        s = match f.align {
                            Align::Left => s + &fill,
                            Align::Right => fill + &s,
                        };
                    }
                    out += &s;
                }
            }
        }
        // Characters a file system refuses.
        out.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_parse_and_render() {
        let r = FilenameRule::parse("{Name:12:4:0:left}_{ProdNo}").unwrap();
        let v = |name: &str| Values { name: name.into(), prod_no: 7, ..Default::default() };
        assert_eq!(r.render(&v("W1")), "W100_7");
        assert_eq!(r.render(&v("WALL-PANEL-NORTH-01")), "WALL-PANEL-N_7");
        let r = FilenameRule::parse("{ProdNo::5:0:right}").unwrap();
        assert_eq!(r.render(&v("x")), "00007");
        let e = FilenameRule::parse("{Nme}_{ProdNo}").unwrap_err();
        assert_eq!(e.position, 1);
        assert!(e.message.contains("Nme"));
        assert_eq!(FilenameRule::parse("a{Name:x}").unwrap_err().position, 7);
        assert!(FilenameRule::parse("{Name").is_err());
    }

    #[test]
    fn csv_quotes_and_rounds() {
        let p = Piece {
            schema: SCHEMA,
            mark: "W1-1".into(),
            name: "a, b".into(),
            prod_no: 1,
            kind: "solid_wall".into(),
            length: 5000.0,
            height: 3000.04,
            thickness: 200.0,
            volume_m3: 3.0,
            weight_kg: 7500.0,
            outline: vec![[0.0, 0.0], [5000.0, 0.0]],
            openings: Vec::new(),
            embeds: Vec::new(),
            bars: Vec::new(),
            meshes: Vec::new(),
        };
        let c = csv(&p);
        assert!(c.starts_with("piece,W1-1,\"a, b\",1,solid_wall,5000,3000,200,3.0000,7500.0\n"), "{c}");
        assert!(c.contains("outline,1,5000,0\n"));
    }
}
