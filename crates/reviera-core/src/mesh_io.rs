// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Readers for open mesh formats (DM-IMP-006, DM-IMP-007): OBJ (with its MTL colours), STL
//! (ASCII and binary), PLY (ASCII and binary) and glTF 2.0 / GLB. Reviera's own readers, written
//! from the public format descriptions; they read geometry and base colours, not textures.
//!
//! Each gives a [`RawMesh`]: triangles in the file's own units and axes, a material slot per
//! triangle, and what the file says about its units and up axis (glTF: metres, Y up).

use std::path::Path;

use crate::error::{Error, Result};

/// Base64 (standard alphabet, padded), for glTF data URIs and stored meshes.
pub(crate) mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    pub fn encode(bytes: &[u8]) -> String {
        STANDARD.encode(bytes)
    }

    pub fn decode(text: &str) -> Option<Vec<u8>> {
        STANDARD.decode(text.trim()).ok()
    }
}

/// The formats read, by file extension.
pub const EXTENSIONS: &[&str] = &["obj", "stl", "ply", "gltf", "glb"];

/// A material slot: its name and colour (RGBA, 0–1).
#[derive(Debug, Clone, PartialEq)]
pub struct RawSlot {
    pub name: String,
    pub color: [f32; 4],
}

/// A mesh as read from a file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawMesh {
    pub format: String,
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// The slot of each triangle.
    pub tri_slot: Vec<u32>,
    pub slots: Vec<RawSlot>,
    /// Metres per file unit, when the file states it.
    pub units: Option<f64>,
    /// The file's up axis is Y (glTF).
    pub y_up: Option<bool>,
}

const DEFAULT_COLOR: [f32; 4] = [0.78, 0.78, 0.78, 1.0];

fn bad(msg: impl Into<String>) -> Error {
    Error::new("mesh.unreadable", msg)
}

/// Reads a mesh file, by its extension.
pub fn read(path: &Path) -> Result<RawMesh> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| {
        Error::new(
            "mesh.not_found",
            format!("could not read {}: {e}", path.display()),
        )
    })?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut m = match ext.as_str() {
        "obj" => read_obj(&bytes, &|name| std::fs::read(dir.join(name)).ok())?,
        "stl" => read_stl(&bytes)?,
        "ply" => read_ply(&bytes)?,
        "gltf" | "glb" => read_gltf(&bytes, &|uri| std::fs::read(dir.join(uri)).ok())?,
        _ => {
            return Err(Error::new(
                "mesh.unsupported_format",
                format!(
                    "{} is not a mesh format Reviera reads (OBJ, STL, PLY, glTF, GLB)",
                    path.display()
                ),
            ));
        }
    };
    if m.triangles.is_empty() {
        return Err(bad(format!("{} has no triangles", path.display())));
    }
    if m.slots.is_empty() {
        m.slots.push(RawSlot {
            name: "Default".into(),
            color: DEFAULT_COLOR,
        });
    }
    Ok(m)
}

// ---- OBJ ----

/// Reads Wavefront OBJ; `load` reads files next to it (its MTL libraries).
pub fn read_obj(bytes: &[u8], load: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<RawMesh> {
    let text = String::from_utf8_lossy(bytes);
    let mut m = RawMesh {
        format: "OBJ".into(),
        ..Default::default()
    };
    let mut colors: Vec<(String, [f32; 4])> = Vec::new();
    let mut slot: Option<u32> = None;
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let v: Vec<f64> = it.take(3).filter_map(|t| t.parse().ok()).collect();
                if v.len() != 3 {
                    return Err(bad(format!(
                        "OBJ line {}: a vertex needs x, y and z",
                        n + 1
                    )));
                }
                m.positions.push([v[0], v[1], v[2]]);
            }
            Some("f") => {
                let count = m.positions.len() as i64;
                let idx: Vec<u32> = it
                    .map(|t| {
                        let i: i64 =
                            t.split('/').next().unwrap_or("").parse().map_err(|_| {
                                bad(format!("OBJ line {}: bad face index {t}", n + 1))
                            })?;
                        let i = if i < 0 { count + i } else { i - 1 };
                        if i < 0 || i >= count {
                            return Err(bad(format!(
                                "OBJ line {}: face index out of range",
                                n + 1
                            )));
                        }
                        Ok(i as u32)
                    })
                    .collect::<Result<_>>()?;
                let s = match slot {
                    Some(s) => s,
                    None => {
                        m.slots.push(RawSlot {
                            name: "Default".into(),
                            color: DEFAULT_COLOR,
                        });
                        let s = m.slots.len() as u32 - 1;
                        slot = Some(s);
                        s
                    }
                };
                // Polygons as fans.
                for k in 1..idx.len().saturating_sub(1) {
                    m.triangles.push([idx[0], idx[k], idx[k + 1]]);
                    m.tri_slot.push(s);
                }
            }
            Some("mtllib") => {
                for lib in line["mtllib".len()..].split_whitespace() {
                    if let Some(b) = load(lib) {
                        colors.extend(read_mtl(&String::from_utf8_lossy(&b)));
                    }
                }
            }
            Some("usemtl") => {
                let name = line["usemtl".len()..].trim().to_owned();
                let existing = m.slots.iter().position(|s| s.name == name);
                slot = Some(match existing {
                    Some(i) => i as u32,
                    None => {
                        let color = colors
                            .iter()
                            .find(|(n, _)| *n == name)
                            .map_or(DEFAULT_COLOR, |(_, c)| *c);
                        m.slots.push(RawSlot { name, color });
                        m.slots.len() as u32 - 1
                    }
                });
            }
            _ => {}
        }
    }
    Ok(m)
}

/// The diffuse colour and opacity of each material in an MTL file.
fn read_mtl(text: &str) -> Vec<(String, [f32; 4])> {
    let mut out: Vec<(String, [f32; 4])> = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut it = line.split_whitespace();
        match (it.next(), out.last_mut()) {
            (Some("newmtl"), _) => {
                out.push((line["newmtl".len()..].trim().to_owned(), DEFAULT_COLOR));
            }
            (Some("Kd"), Some((_, c))) => {
                let v: Vec<f32> = it.take(3).filter_map(|t| t.parse().ok()).collect();
                if v.len() == 3 {
                    c[..3].copy_from_slice(&v);
                }
            }
            (Some("d"), Some((_, c))) => {
                if let Some(d) = it.next().and_then(|t| t.parse().ok()) {
                    c[3] = d;
                }
            }
            (Some("Tr"), Some((_, c))) => {
                if let Some(t) = it.next().and_then(|t| t.parse::<f32>().ok()) {
                    c[3] = 1.0 - t;
                }
            }
            _ => {}
        }
    }
    out
}

// ---- STL ----

/// Reads STL, binary or ASCII.
pub fn read_stl(bytes: &[u8]) -> Result<RawMesh> {
    let mut m = RawMesh {
        format: "STL".into(),
        ..Default::default()
    };
    // Binary when the size matches the triangle count in its header (some binary files begin
    // with "solid" too).
    let binary = bytes.len() >= 84 && {
        let n = u32::from_le_bytes(bytes[80..84].try_into().expect("4 bytes")) as usize;
        bytes.len() == 84 + n * 50
    };
    if binary {
        let n = (bytes.len() - 84) / 50;
        for t in 0..n {
            let rec = &bytes[84 + t * 50..84 + t * 50 + 50];
            let f =
                |k: usize| f32::from_le_bytes(rec[k..k + 4].try_into().expect("4 bytes")) as f64;
            let base = m.positions.len() as u32;
            for v in 0..3 {
                let o = 12 + v * 12;
                m.positions.push([f(o), f(o + 4), f(o + 8)]);
            }
            m.triangles.push([base, base + 1, base + 2]);
            m.tri_slot.push(0);
        }
    } else {
        let text = String::from_utf8_lossy(bytes);
        let mut corner = Vec::new();
        for line in text.lines() {
            let mut it = line.split_whitespace();
            if it.next() == Some("vertex") {
                let v: Vec<f64> = it.take(3).filter_map(|t| t.parse().ok()).collect();
                if v.len() == 3 {
                    corner.push([v[0], v[1], v[2]]);
                }
            } else if line.trim_start().starts_with("endfacet") {
                // Facets with more than three corners are fans.
                let base = m.positions.len() as u32;
                for k in 1..corner.len().saturating_sub(1) {
                    m.triangles
                        .push([base, base + k as u32, base + k as u32 + 1]);
                    m.tri_slot.push(0);
                }
                m.positions.append(&mut corner);
            }
        }
        if m.triangles.is_empty() {
            return Err(bad("not an STL file (no facets found)"));
        }
    }
    Ok(m)
}

// ---- PLY ----

#[derive(Debug, Clone, Copy, PartialEq)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Scalar {
    fn parse(t: &str) -> Option<Self> {
        Some(match t {
            "char" | "int8" => Self::I8,
            "uchar" | "uint8" => Self::U8,
            "short" | "int16" => Self::I16,
            "ushort" | "uint16" => Self::U16,
            "int" | "int32" => Self::I32,
            "uint" | "uint32" => Self::U32,
            "float" | "float32" => Self::F32,
            "double" | "float64" => Self::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    fn read(self, b: &[u8], big: bool) -> f64 {
        macro_rules! num {
            ($t:ty, $n:literal) => {{
                let a: [u8; $n] = b[..$n].try_into().expect("sized");
                (if big {
                    <$t>::from_be_bytes(a)
                } else {
                    <$t>::from_le_bytes(a)
                }) as f64
            }};
        }
        match self {
            Self::I8 => b[0] as i8 as f64,
            Self::U8 => b[0] as f64,
            Self::I16 => num!(i16, 2),
            Self::U16 => num!(u16, 2),
            Self::I32 => num!(i32, 4),
            Self::U32 => num!(u32, 4),
            Self::F32 => num!(f32, 4),
            Self::F64 => num!(f64, 8),
        }
    }
}

#[derive(Debug, Clone)]
struct Prop {
    name: String,
    /// `Some(count type)` for a list property.
    list: Option<Scalar>,
    ty: Scalar,
}

#[derive(Debug, Clone)]
struct PlyElement {
    name: String,
    count: usize,
    props: Vec<Prop>,
}

/// Reads PLY, ASCII or binary (either byte order): vertices (with colours when present) and
/// faces.
pub fn read_ply(bytes: &[u8]) -> Result<RawMesh> {
    if !bytes.starts_with(b"ply") {
        return Err(bad("not a PLY file"));
    }
    let end = bytes
        .windows(10)
        .position(|w| w == b"end_header")
        .ok_or_else(|| bad("PLY header has no end_header"))?;
    let mut body = end + 10;
    if bytes.get(body) == Some(&b'\r') {
        body += 1;
    }
    if bytes.get(body) == Some(&b'\n') {
        body += 1;
    }
    let header = String::from_utf8_lossy(&bytes[..end]);
    let mut format = "";
    let mut elements: Vec<PlyElement> = Vec::new();
    for line in header.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        match t.as_slice() {
            ["format", f, ..] => {
                format = if f.contains("ascii") {
                    "ascii"
                } else if f.contains("big") {
                    "big"
                } else {
                    "little"
                }
            }
            ["element", name, count] => elements.push(PlyElement {
                name: (*name).into(),
                count: count.parse().map_err(|_| bad("PLY: bad element count"))?,
                props: Vec::new(),
            }),
            ["property", "list", c, ty, name] => elements
                .last_mut()
                .ok_or_else(|| bad("PLY: property before element"))?
                .props
                .push(Prop {
                    name: (*name).into(),
                    list: Some(Scalar::parse(c).ok_or_else(|| bad("PLY: bad list type"))?),
                    ty: Scalar::parse(ty).ok_or_else(|| bad("PLY: bad property type"))?,
                }),
            ["property", ty, name] => elements
                .last_mut()
                .ok_or_else(|| bad("PLY: property before element"))?
                .props
                .push(Prop {
                    name: (*name).into(),
                    list: None,
                    ty: Scalar::parse(ty).ok_or_else(|| bad("PLY: bad property type"))?,
                }),
            _ => {}
        }
    }
    let mut m = RawMesh {
        format: "PLY".into(),
        ..Default::default()
    };
    let mut color_sum = [0.0f64; 4];
    let mut colored = 0usize;
    // One record's values: scalars, and lists as their items.
    let mut emit = |el: &PlyElement, values: &[Vec<f64>], m: &mut RawMesh| -> Result<()> {
        let get = |n: &str| {
            el.props
                .iter()
                .position(|p| p.name == n)
                .map(|i| &values[i])
        };
        if el.name == "vertex" {
            let c = |n: &str| get(n).and_then(|v| v.first()).copied();
            let (Some(x), Some(y), Some(z)) = (c("x"), c("y"), c("z")) else {
                return Err(bad("PLY vertex without x, y and z"));
            };
            m.positions.push([x, y, z]);
            if let (Some(r), Some(g), Some(b)) = (c("red"), c("green"), c("blue")) {
                let scale = if el
                    .props
                    .iter()
                    .any(|p| p.name == "red" && matches!(p.ty, Scalar::F32 | Scalar::F64))
                {
                    1.0
                } else {
                    255.0
                };
                color_sum[0] += r / scale;
                color_sum[1] += g / scale;
                color_sum[2] += b / scale;
                color_sum[3] += c("alpha").map_or(1.0, |a| a / scale);
                colored += 1;
            }
        } else if el.name == "face" {
            let idx = get("vertex_indices")
                .or_else(|| get("vertex_index"))
                .ok_or_else(|| bad("PLY face without vertex_indices"))?;
            for k in 1..idx.len().saturating_sub(1) {
                m.triangles
                    .push([idx[0] as u32, idx[k] as u32, idx[k + 1] as u32]);
                m.tri_slot.push(0);
            }
        }
        Ok(())
    };
    if format == "ascii" {
        let text = String::from_utf8_lossy(&bytes[body..]);
        let mut tokens = text.split_whitespace();
        let mut next = || -> Result<f64> {
            tokens
                .next()
                .ok_or_else(|| bad("PLY data ends early"))?
                .parse()
                .map_err(|_| bad("PLY: bad number"))
        };
        for el in &elements {
            for _ in 0..el.count {
                let mut values = Vec::with_capacity(el.props.len());
                for p in &el.props {
                    values.push(match p.list {
                        Some(_) => {
                            let n = next()? as usize;
                            (0..n).map(|_| next()).collect::<Result<Vec<_>>>()?
                        }
                        None => vec![next()?],
                    });
                }
                emit(el, &values, &mut m)?;
            }
        }
    } else {
        let big = format == "big";
        let mut at = body;
        let mut take = |ty: Scalar| -> Result<f64> {
            let b = bytes
                .get(at..at + ty.size())
                .ok_or_else(|| bad("PLY data ends early"))?;
            at += ty.size();
            Ok(ty.read(b, big))
        };
        for el in &elements {
            for _ in 0..el.count {
                let mut values = Vec::with_capacity(el.props.len());
                for p in &el.props {
                    values.push(match p.list {
                        Some(c) => {
                            let n = take(c)? as usize;
                            (0..n).map(|_| take(p.ty)).collect::<Result<Vec<_>>>()?
                        }
                        None => vec![take(p.ty)?],
                    });
                }
                emit(el, &values, &mut m)?;
            }
        }
    }
    let n = m.positions.len() as u32;
    if m.triangles.iter().flatten().any(|i| *i >= n) {
        return Err(bad("PLY face index out of range"));
    }
    let color = if colored > 0 {
        let k = colored as f64;
        color_sum.map(|c| (c / k) as f32)
    } else {
        DEFAULT_COLOR
    };
    m.slots.push(RawSlot {
        name: "Default".into(),
        color,
    });
    Ok(m)
}

// ---- glTF 2.0 / GLB ----

use serde_json::Value;

/// Reads glTF 2.0 (`.gltf` with its buffers, or binary `.glb`): the default scene's meshes,
/// placed by their nodes, with each material's base colour. `load` reads files beside it.
pub fn read_gltf(bytes: &[u8], load: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<RawMesh> {
    let (json, bin): (Value, Option<&[u8]>) = if bytes.starts_with(b"glTF") {
        if bytes.len() < 20 {
            return Err(bad("GLB file is too short"));
        }
        let u =
            |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().expect("4 bytes")) as usize;
        let mut at = 12;
        let (mut json, mut bin) = (None, None);
        while at + 8 <= bytes.len() {
            let (len, kind) = (u(at), u(at + 4));
            let chunk = bytes
                .get(at + 8..at + 8 + len)
                .ok_or_else(|| bad("GLB chunk runs past the file"))?;
            match kind {
                0x4E4F534A => json = Some(chunk),
                0x004E4942 => bin = Some(chunk),
                _ => {}
            }
            at += 8 + len;
        }
        let json = json.ok_or_else(|| bad("GLB has no JSON chunk"))?;
        (
            serde_json::from_slice(json).map_err(|e| bad(format!("GLB JSON: {e}")))?,
            bin,
        )
    } else {
        (
            serde_json::from_slice(bytes).map_err(|e| bad(format!("glTF JSON: {e}")))?,
            None,
        )
    };
    let arr = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    if let Some(ext) = arr(&json, "extensionsRequired").first() {
        return Err(Error::new(
            "mesh.unsupported_extension",
            format!(
                "the glTF file needs the extension {} (for example Draco compression), which Reviera does not read",
                ext.as_str().unwrap_or("?")
            ),
        ));
    }
    // Buffers.
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for (i, b) in arr(&json, "buffers").iter().enumerate() {
        let data = match b.get("uri").and_then(Value::as_str) {
            Some(uri) if uri.starts_with("data:") => {
                let (_, payload) = uri
                    .split_once(";base64,")
                    .ok_or_else(|| bad("glTF data URI is not base64"))?;
                b64::decode(payload).ok_or_else(|| bad("glTF data URI: bad base64"))?
            }
            Some(uri) => load(&percent_decode(uri))
                .ok_or_else(|| bad(format!("glTF buffer {uri} was not found beside the file")))?,
            None if i == 0 => bin
                .ok_or_else(|| bad("glTF buffer 0 has no data"))?
                .to_vec(),
            None => return Err(bad("glTF buffer has no data")),
        };
        buffers.push(data);
    }
    let views = arr(&json, "bufferViews");
    let accessors = arr(&json, "accessors");
    let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_u64).map(|n| n as usize);
    // An accessor's values as f64s, `width` per element.
    let read_accessor = |i: usize| -> Result<(Vec<f64>, usize)> {
        let a = accessors
            .get(i)
            .ok_or_else(|| bad("glTF accessor out of range"))?;
        if a.get("sparse").is_some() {
            return Err(bad("glTF sparse accessors are not read"));
        }
        let count = num(a, "count").ok_or_else(|| bad("glTF accessor without count"))?;
        let width = match a.get("type").and_then(Value::as_str) {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            _ => return Err(bad("glTF accessor type not read")),
        };
        let ty = match num(a, "componentType") {
            Some(5120) => Scalar::I8,
            Some(5121) => Scalar::U8,
            Some(5122) => Scalar::I16,
            Some(5123) => Scalar::U16,
            Some(5125) => Scalar::U32,
            Some(5126) => Scalar::F32,
            _ => return Err(bad("glTF component type not read")),
        };
        let Some(view) = num(a, "bufferView").and_then(|v| views.get(v)) else {
            return Ok((vec![0.0; count * width], width));
        };
        let buffer = num(view, "buffer")
            .and_then(|b| buffers.get(b))
            .ok_or_else(|| bad("glTF buffer view out of range"))?;
        let start = num(view, "byteOffset").unwrap_or(0) + num(a, "byteOffset").unwrap_or(0);
        let stride = num(view, "byteStride").unwrap_or(ty.size() * width);
        let mut out = Vec::with_capacity(count * width);
        for e in 0..count {
            for c in 0..width {
                let o = start + e * stride + c * ty.size();
                let b = buffer
                    .get(o..o + ty.size())
                    .ok_or_else(|| bad("glTF accessor runs past its buffer"))?;
                out.push(ty.read(b, false));
            }
        }
        Ok((out, width))
    };
    let mut m = RawMesh {
        format: if bin.is_some() {
            "GLB".into()
        } else {
            "glTF".into()
        },
        units: Some(1.0),
        y_up: Some(true),
        ..Default::default()
    };
    let materials = arr(&json, "materials");
    let mut slot_of_material: Vec<Option<u32>> = vec![None; materials.len()];
    let mut default_slot: Option<u32> = None;
    let meshes = arr(&json, "meshes");
    let nodes = arr(&json, "nodes");
    let mut add_mesh = |mesh: usize, matrix: &[f64; 16], m: &mut RawMesh| -> Result<()> {
        let Some(mesh) = meshes.get(mesh) else {
            return Err(bad("glTF node mesh out of range"));
        };
        for p in arr(mesh, "primitives") {
            if num(&p, "mode").unwrap_or(4) != 4 {
                continue; // points and lines
            }
            let Some(pos) = p.get("attributes").and_then(|a| num(a, "POSITION")) else {
                continue;
            };
            let (pts, _) = read_accessor(pos)?;
            let base = m.positions.len() as u32;
            for c in pts.as_chunks::<3>().0 {
                m.positions.push(transform(matrix, [c[0], c[1], c[2]]));
            }
            let n = (pts.len() / 3) as u32;
            let idx: Vec<u32> = match num(&p, "indices") {
                Some(i) => read_accessor(i)?.0.into_iter().map(|v| v as u32).collect(),
                None => (0..n).collect(),
            };
            if idx.iter().any(|i| *i >= n) {
                return Err(bad("glTF index out of range"));
            }
            let slot = match num(&p, "material") {
                Some(mi) if mi < materials.len() => match slot_of_material[mi] {
                    Some(s) => s,
                    None => {
                        let mat = &materials[mi];
                        let color = mat
                            .get("pbrMetallicRoughness")
                            .and_then(|r| r.get("baseColorFactor"))
                            .and_then(Value::as_array)
                            .filter(|c| c.len() == 4)
                            .map_or(DEFAULT_COLOR, |c| {
                                let f = |k: usize| c[k].as_f64().unwrap_or(1.0) as f32;
                                [f(0), f(1), f(2), f(3)]
                            });
                        let name = mat
                            .get("name")
                            .and_then(Value::as_str)
                            .map_or(format!("Material {mi}"), str::to_owned);
                        m.slots.push(RawSlot { name, color });
                        let s = m.slots.len() as u32 - 1;
                        slot_of_material[mi] = Some(s);
                        s
                    }
                },
                _ => *default_slot.get_or_insert_with(|| {
                    m.slots.push(RawSlot {
                        name: "Default".into(),
                        color: DEFAULT_COLOR,
                    });
                    m.slots.len() as u32 - 1
                }),
            };
            // A mirroring node transform turns the winding over.
            let flip = det3(matrix) < 0.0;
            for t in idx.as_chunks::<3>().0 {
                let tri = [base + t[0], base + t[1], base + t[2]];
                m.triangles
                    .push(if flip { [tri[0], tri[2], tri[1]] } else { tri });
                m.tri_slot.push(slot);
            }
        }
        Ok(())
    };
    // The scene's node tree, or every mesh as it is when there is none.
    let scene = num(&json, "scene").unwrap_or(0);
    let roots: Vec<usize> = arr(&json, "scenes")
        .get(scene)
        .map(|s| {
            arr(s, "nodes")
                .iter()
                .filter_map(Value::as_u64)
                .map(|n| n as usize)
                .collect()
        })
        .unwrap_or_default();
    if roots.is_empty() && nodes.is_empty() {
        for i in 0..meshes.len() {
            add_mesh(i, &IDENTITY, &mut m)?;
        }
    } else {
        let roots = if roots.is_empty() {
            // No scene: the nodes no other node names as a child.
            let children: Vec<usize> = nodes
                .iter()
                .flat_map(|n| arr(n, "children"))
                .filter_map(|c| c.as_u64().map(|c| c as usize))
                .collect();
            (0..nodes.len()).filter(|i| !children.contains(i)).collect()
        } else {
            roots
        };
        let mut stack: Vec<(usize, [f64; 16], usize)> =
            roots.into_iter().map(|r| (r, IDENTITY, 0)).collect();
        while let Some((i, parent, depth)) = stack.pop() {
            if depth > 64 {
                return Err(bad("glTF node tree is too deep (a cycle?)"));
            }
            let node = nodes.get(i).ok_or_else(|| bad("glTF node out of range"))?;
            let world = mul(&parent, &node_matrix(node));
            if let Some(mesh) = num(node, "mesh") {
                add_mesh(mesh, &world, &mut m)?;
            }
            for c in arr(node, "children").iter().filter_map(Value::as_u64) {
                stack.push((c as usize, world, depth + 1));
            }
        }
    }
    Ok(m)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Column-major 4 × 4, as glTF stores matrices.
const IDENTITY: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn mul(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    o
}

fn transform(m: &[f64; 16], p: [f64; 3]) -> [f64; 3] {
    let f = |r: usize| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r];
    [f(0), f(1), f(2)]
}

fn det3(m: &[f64; 16]) -> f64 {
    m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2])
        + m[8] * (m[1] * m[6] - m[5] * m[2])
}

/// A node's local matrix: `matrix`, or translation × rotation × scale.
fn node_matrix(node: &Value) -> [f64; 16] {
    let nums = |k: &str| -> Option<Vec<f64>> {
        node.get(k)?.as_array()?.iter().map(Value::as_f64).collect()
    };
    if let Some(m) = nums("matrix").filter(|m| m.len() == 16) {
        return m.try_into().expect("16");
    }
    let t = nums("translation")
        .filter(|v| v.len() == 3)
        .unwrap_or(vec![0.0; 3]);
    let q = nums("rotation")
        .filter(|v| v.len() == 4)
        .unwrap_or(vec![0.0, 0.0, 0.0, 1.0]);
    let s = nums("scale")
        .filter(|v| v.len() == 3)
        .unwrap_or(vec![1.0; 3]);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let r = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
    ];
    [
        r[0] * s[0],
        r[1] * s[0],
        r[2] * s[0],
        0.0,
        r[3] * s[1],
        r[4] * s[1],
        r[5] * s[1],
        0.0,
        r[6] * s[2],
        r[7] * s[2],
        r[8] * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obj_with_materials_and_quads() {
        let obj = b"mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nusemtl red\nf 1 2 3 4\nusemtl blue\nf 1/1/1 2/2/2 -1\n";
        let mtl = b"newmtl red\nKd 1 0 0\nnewmtl blue\nKd 0 0 1\nd 0.5\n";
        let m = read_obj(obj, &|n| (n == "m.mtl").then(|| mtl.to_vec())).unwrap();
        assert_eq!(m.triangles.len(), 3);
        assert_eq!(m.tri_slot, vec![0, 0, 1]);
        assert_eq!(m.slots[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(m.slots[1].color, [0.0, 0.0, 1.0, 0.5]);
        assert_eq!(m.triangles[2], [0, 1, 4]);
    }

    #[test]
    fn stl_ascii_and_binary() {
        let ascii = b"solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
        let a = read_stl(ascii).unwrap();
        assert_eq!(a.triangles.len(), 1);
        let mut bin = vec![0u8; 80];
        bin.extend(1u32.to_le_bytes());
        bin.extend([0u8; 12]);
        for v in [[0f32, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]] {
            for c in v {
                bin.extend(c.to_le_bytes());
            }
        }
        bin.extend([0u8; 2]);
        let b = read_stl(&bin).unwrap();
        assert_eq!(b.positions[1], [2.0, 0.0, 0.0]);
    }

    #[test]
    fn ply_ascii_and_binary() {
        let ascii = b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 255 0 0\n1 0 0 255 0 0\n0 1 0 255 0 0\n3 0 1 2\n";
        let a = read_ply(ascii).unwrap();
        assert_eq!(a.triangles, vec![[0, 1, 2]]);
        assert_eq!(a.slots[0].color, [1.0, 0.0, 0.0, 1.0]);
        let mut bin = b"ply\nformat binary_little_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar uint vertex_indices\nend_header\n".to_vec();
        for v in [[0f32, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0, 3.0, 0.0]] {
            for c in v {
                bin.extend(c.to_le_bytes());
            }
        }
        bin.push(3);
        for i in [0u32, 1, 2] {
            bin.extend(i.to_le_bytes());
        }
        let b = read_ply(&bin).unwrap();
        assert_eq!(b.positions[1], [3.0, 0.0, 0.0]);
        assert_eq!(b.triangles, vec![[0, 1, 2]]);
    }

    #[test]
    fn gltf_embedded_buffer_with_node_transform() {
        // One triangle (floats) and its indices (u16), translated by the node.
        let mut buf = Vec::new();
        for v in [[0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in v {
                buf.extend(c.to_le_bytes());
            }
        }
        for i in [0u16, 1, 2] {
            buf.extend(i.to_le_bytes());
        }
        buf.extend([0u8; 2]);
        let uri = format!("data:application/octet-stream;base64,{}", b64::encode(&buf));
        let json = serde_json::json!({
            "asset": {"version": "2.0"},
            "buffers": [{"byteLength": buf.len(), "uri": uri}],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 6}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"},
                {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}
            ],
            "materials": [{"name": "Stone", "pbrMetallicRoughness": {"baseColorFactor": [0.5, 0.5, 0.4, 1.0]}}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "material": 0}]}],
            "nodes": [{"mesh": 0, "translation": [10.0, 0.0, 0.0]}],
            "scenes": [{"nodes": [0]}],
            "scene": 0
        });
        let m = read_gltf(json.to_string().as_bytes(), &|_| None).unwrap();
        assert_eq!(m.positions[1], [11.0, 0.0, 0.0]);
        assert_eq!(m.slots[0].name, "Stone");
        assert_eq!(m.y_up, Some(true));
        // The same as a GLB.
        let j = json.to_string().into_bytes();
        let mut glb = b"glTF".to_vec();
        glb.extend(2u32.to_le_bytes());
        glb.extend(0u32.to_le_bytes());
        glb.extend((j.len() as u32).to_le_bytes());
        glb.extend(0x4E4F534Au32.to_le_bytes());
        glb.extend(&j);
        assert_eq!(read_gltf(&glb, &|_| None).unwrap().triangles.len(), 1);
    }
}
