// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! DSTV NC1 files: the Deutscher Stahlbau-Verband's *Standard Description for Steel Structure
//! Pieces for the Numerical Controls* (7th edition, 1998), an open, machine-neutral interchange
//! format describing one steel piece per text file for saws, drills, punches and cutters.
//!
//! A file starts with `ST` and its header lines (order, drawing, phase, piece, steel quality,
//! quantity, profile, profile code, length, height, flange width and thickness, web thickness,
//! radius, weight and painting surface per metre, the four skew cut angles, and text lines), then
//! blocks in any order (`BO` holes, `AK` external and `IK` internal contours, `SI` numbering) and
//! ends with `EN`. Block codes are in columns 1 and 2; data lines start with two spaces. Lengths
//! are millimetres, angles degrees. Faces: `o` top flange, `u` bottom flange, `v` the web's front,
//! `h` behind. The smallest x of a piece is 0; plates lie in their smallest rectangle.
//!
//! This crate writes pieces and reads back the parts it writes (for checking); it knows nothing
//! about any model. It is MIT-licensed so any program can use it.

use std::fmt::Write as _;

/// The DSTV profile codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileCode {
    /// I and H sections.
    I,
    L,
    U,
    /// Plates and sheets.
    B,
    /// Round bar.
    RU,
    /// Round tube.
    RO,
    /// Rectangular tube.
    M,
    C,
    T,
    /// Any other section.
    SO,
}

impl ProfileCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileCode::I => "I",
            ProfileCode::L => "L",
            ProfileCode::U => "U",
            ProfileCode::B => "B",
            ProfileCode::RU => "RU",
            ProfileCode::RO => "RO",
            ProfileCode::M => "M",
            ProfileCode::C => "C",
            ProfileCode::T => "T",
            ProfileCode::SO => "SO",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "I" => ProfileCode::I,
            "L" => ProfileCode::L,
            "U" => ProfileCode::U,
            "B" => ProfileCode::B,
            "RU" => ProfileCode::RU,
            "RO" => ProfileCode::RO,
            "M" => ProfileCode::M,
            "C" => ProfileCode::C,
            "T" => ProfileCode::T,
            "SO" => ProfileCode::SO,
            _ => return None,
        })
    }
}

/// The face of a piece a feature is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// Top flange.
    O,
    /// Bottom flange.
    U,
    /// The web's front (and a plate's face).
    V,
    /// Behind the web.
    H,
}

impl Face {
    pub fn letter(self) -> char {
        match self {
            Face::O => 'o',
            Face::U => 'u',
            Face::V => 'v',
            Face::H => 'h',
        }
    }

    pub fn parse(c: char) -> Option<Self> {
        Some(match c {
            'o' => Face::O,
            'u' => Face::U,
            'v' => Face::V,
            'h' => Face::H,
            _ => return None,
        })
    }
}

/// A hole: its face, x along the piece, y across the face, diameter and depth (0 through).
#[derive(Debug, Clone, PartialEq)]
pub struct Hole {
    pub face: Face,
    pub x: f64,
    pub y: f64,
    pub diameter: f64,
    pub depth: f64,
}

/// A closed contour on a face: its points (x, y) with the radius of the arc to the next.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    pub face: Face,
    /// External (`AK`, anticlockwise) or internal (`IK`, clockwise).
    pub external: bool,
    pub points: Vec<(f64, f64, f64)>,
}

/// A piece: its header data and features.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub order: String,
    pub drawing: String,
    pub phase: String,
    /// The piece's identification (its part mark).
    pub piece: String,
    pub steel: String,
    pub quantity: u32,
    /// The profile's designation (`W12X53`, `PL15`, …).
    pub profile: String,
    pub code: ProfileCode,
    pub length: f64,
    /// A plate's width.
    pub height: f64,
    pub flange_width: f64,
    pub flange_thickness: f64,
    /// A plate's thickness.
    pub web_thickness: f64,
    pub radius: f64,
    /// kg/m (kg/m² for plates).
    pub weight: f64,
    /// m²/m (m²/m² for plates).
    pub paint: f64,
    /// Web start, web end, flange start and flange end cut angles.
    pub skew: [f64; 4],
    pub text: Vec<String>,
    pub holes: Vec<Hole>,
    pub contours: Vec<Contour>,
}

fn clean(s: &str) -> String {
    // Free text: one line, at most 80 characters.
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let one = one.trim().to_owned();
    let out: String = one.chars().take(80).collect();
    if out.is_empty() { "-".into() } else { out }
}

/// The piece as an NC1 file.
pub fn write(p: &Piece) -> String {
    let mut s = String::new();
    s.push_str("ST\n");
    s.push_str("** BIM Reviera\n");
    for t in [&p.order, &p.drawing, &p.phase, &p.piece, &p.steel] {
        let _ = writeln!(s, "  {}", clean(t));
    }
    let _ = writeln!(s, "  {}", p.quantity);
    let _ = writeln!(s, "  {}", clean(&p.profile));
    let _ = writeln!(s, "  {}", p.code.as_str());
    for v in [p.length, p.height, p.flange_width, p.flange_thickness, p.web_thickness, p.radius] {
        let _ = writeln!(s, "  {v:10.2}");
    }
    for v in [p.weight, p.paint] {
        let _ = writeln!(s, "  {v:10.3}");
    }
    for v in p.skew {
        let _ = writeln!(s, "  {v:10.3}");
    }
    for k in 0..4 {
        let _ = writeln!(s, "  {}", clean(p.text.get(k).map_or("", String::as_str)));
    }
    if !p.holes.is_empty() {
        s.push_str("BO\n");
        for h in &p.holes {
            let _ = writeln!(s, "  {}{:10.2}u{:10.2}{:8.2}{:8.2}", h.face.letter(), h.x, h.y, h.diameter, h.depth);
        }
    }
    for c in &p.contours {
        if c.points.is_empty() {
            continue;
        }
        s.push_str(if c.external { "AK\n" } else { "IK\n" });
        // Closed: the last point repeats the first.
        let mut pts = c.points.clone();
        if pts.first().map(|f| (f.0, f.1)) != pts.last().map(|l| (l.0, l.1)) {
            pts.push((pts[0].0, pts[0].1, 0.0));
        }
        for (x, y, r) in pts {
            let _ = writeln!(s, "  {}{:10.2}u{:10.2}{:10.2}", c.face.letter(), x, y, r);
        }
    }
    s.push_str("EN\n");
    s
}

/// Errors reading a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

/// The numbers of a data line after its face letter, letters taken as separators.
fn numbers(rest: &str) -> Vec<f64> {
    rest.split(|c: char| c.is_ascii_alphabetic() || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<f64>().ok())
        .collect()
}

/// Reads a file of the kind [`write`] makes: the header, holes and contours.
pub fn parse(text: &str) -> Result<Piece, ParseError> {
    let bad = |m: &str| ParseError(m.into());
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with("**")).collect();
    if lines.first().map(|l| l.trim()) != Some("ST") {
        return Err(bad("no ST block"));
    }
    let header: Vec<&str> = lines.iter().skip(1).take(24).map(|l| l.trim()).collect();
    if header.len() < 24 {
        return Err(bad("the header is short"));
    }
    let f = |k: usize| header[k].parse::<f64>().map_err(|_| ParseError(format!("header line {} is not a number", k + 1)));
    let mut p = Piece {
        order: header[0].into(),
        drawing: header[1].into(),
        phase: header[2].into(),
        piece: header[3].into(),
        steel: header[4].into(),
        quantity: header[5].parse().map_err(|_| bad("the quantity is not a number"))?,
        profile: header[6].into(),
        code: ProfileCode::parse(header[7]).ok_or_else(|| bad("unknown profile code"))?,
        length: f(8)?,
        height: f(9)?,
        flange_width: f(10)?,
        flange_thickness: f(11)?,
        web_thickness: f(12)?,
        radius: f(13)?,
        weight: f(14)?,
        paint: f(15)?,
        skew: [f(16)?, f(17)?, f(18)?, f(19)?],
        text: header[20..24].iter().map(|s| s.to_string()).collect(),
        holes: Vec::new(),
        contours: Vec::new(),
    };
    let mut block = "";
    for l in lines.iter().skip(25) {
        if !l.starts_with("  ") {
            block = l.trim();
            if matches!(block, "AK" | "IK") {
                p.contours.push(Contour { face: Face::V, external: block == "AK", points: Vec::new() });
            }
            continue;
        }
        let t = l.trim_start();
        let mut chars = t.chars();
        let Some(face) = chars.next().and_then(Face::parse) else { continue };
        let n = numbers(chars.as_str());
        match block {
            "BO" if n.len() >= 3 => p.holes.push(Hole {
                face,
                x: n[0],
                y: n[1],
                diameter: n[2],
                depth: n.get(3).copied().unwrap_or(0.0),
            }),
            "AK" | "IK" if n.len() >= 2 => {
                let c = p.contours.last_mut().expect("opened");
                c.face = face;
                c.points.push((n[0], n[1], n.get(2).copied().unwrap_or(0.0)));
            }
            _ => {}
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beam() -> Piece {
        Piece {
            order: "Job 7".into(),
            drawing: "S-101".into(),
            phase: "1".into(),
            piece: "b1".into(),
            steel: "A992".into(),
            quantity: 2,
            profile: "W12X26".into(),
            code: ProfileCode::I,
            length: 4500.0,
            height: 310.4,
            flange_width: 164.6,
            flange_thickness: 9.7,
            web_thickness: 5.8,
            radius: 0.0,
            weight: 38.7,
            paint: 1.24,
            skew: [0.0; 4],
            text: vec!["Level 2".into()],
            holes: vec![Hole { face: Face::V, x: 50.0, y: 120.0, diameter: 22.0, depth: 0.0 }],
            contours: Vec::new(),
        }
    }

    #[test]
    fn the_header_has_its_lines_in_order() {
        let t = write(&beam());
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "ST");
        assert!(lines[1].starts_with("**"));
        assert_eq!(lines[5].trim(), "b1");
        assert_eq!(lines[8].trim(), "W12X26");
        assert_eq!(lines[9].trim(), "I");
        assert_eq!(lines[10].trim(), "4500.00");
        assert!(t.contains("\nBO\n  v     50.00u    120.00   22.00    0.00\n"));
        assert!(t.ends_with("EN\n"));
    }

    #[test]
    fn written_files_read_back() {
        let mut p = beam();
        p.contours.push(Contour {
            face: Face::V,
            external: true,
            points: vec![(0.0, 0.0, 0.0), (300.0, 0.0, 0.0), (300.0, 200.0, 0.0), (0.0, 200.0, 0.0)],
        });
        let back = parse(&write(&p)).unwrap();
        assert_eq!(back.piece, "b1");
        assert_eq!(back.quantity, 2);
        assert_eq!(back.code, ProfileCode::I);
        assert_eq!(back.holes, p.holes);
        // The contour comes back closed.
        assert_eq!(back.contours[0].points.len(), 5);
        assert_eq!(back.contours[0].points[4], (0.0, 0.0, 0.0));
    }

    #[test]
    fn text_is_one_line_of_at_most_80_characters() {
        let mut p = beam();
        p.order = format!("a\nb{}", "x".repeat(100));
        let t = write(&p);
        let line = t.lines().nth(2).unwrap();
        assert_eq!(line.len(), 2 + 80);
        assert!(!line.contains('\n'));
    }
}
