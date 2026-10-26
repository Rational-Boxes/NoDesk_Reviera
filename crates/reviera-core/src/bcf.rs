// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! BCF viewpoints (COL-BCF-003): a 3D view's camera, cut and components as a BCF
//! *visualization info* document (`.bcfv`, BCF 3.0, read from 2.1 as well), and back.
//!
//! - **Camera:** a perspective view's eye, direction, up and *vertical* field of view in degrees
//!   (Reviera keeps the horizontal one, at 16:10, which is written as the aspect ratio); a parallel
//!   view's `ViewToWorldScale`, the view's height in metres.
//! - **Clipping planes:** the section box's six faces and each slice plane, by [`views::to_bcf`]
//!   and back by [`views::from_bcf`] (a plane's direction points to the hidden side).
//! - **Components:** the selected elements, and the visible ones as `DefaultVisibility` with its
//!   exceptions, by IFC GlobalId. The window's temporary hide and isolate are kept by the
//!   application, which fills these in ([`Viewpoint::selection`], [`Viewpoint::visibility`]).
//!
//! Not built yet: component colouring, snapshots, `ViewSetupHints`, BCF topics and their `.bcf`
//! archives (COL-BCF-001), 2D views' viewpoints.

use quick_xml::Reader;
use quick_xml::events::Event;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::views::{self, ClippingPlane, ViewCamera, ViewData};

/// Reviera's views are drawn at 16:10.
pub const ASPECT: f64 = 1.6;

/// A BCF camera.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Camera {
    Perspective {
        eye: [f64; 3],
        direction: [f64; 3],
        up: [f64; 3],
        /// Vertical, degrees.
        field_of_view: f64,
        aspect_ratio: f64,
    },
    Orthogonal {
        eye: [f64; 3],
        direction: [f64; 3],
        up: [f64; 3],
        /// The view's height, metres.
        view_to_world_scale: f64,
        aspect_ratio: f64,
    },
}

/// Which components a viewpoint shows: all but the exceptions, or only the exceptions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Visibility {
    pub default_visibility: bool,
    /// IFC GlobalIds.
    pub exceptions: Vec<String>,
}

impl Default for Visibility {
    fn default() -> Self {
        Self { default_visibility: true, exceptions: Vec::new() }
    }
}

/// A BCF viewpoint.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Viewpoint {
    pub guid: Option<String>,
    pub camera: Option<Camera>,
    pub clipping_planes: Vec<ClippingPlane>,
    /// Selected components (IFC GlobalIds).
    pub selection: Vec<String>,
    pub visibility: Visibility,
}

/// `view.apply_viewpoint`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyParams {
    pub view_id: ElementRef,
    pub viewpoint: Viewpoint,
}

fn norm(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// A 3D view's camera as BCF's.
pub fn camera_of(c: &ViewCamera) -> Camera {
    let d = [0, 1, 2].map(|k| c.target[k] - c.eye[k]);
    let l = norm(d).max(1e-12);
    let direction = d.map(|v| v / l);
    if c.perspective {
        let vertical = 2.0 * ((c.field_of_view / 2.0).tan() / ASPECT).atan();
        Camera::Perspective { eye: c.eye, direction, up: c.up, field_of_view: vertical.to_degrees(), aspect_ratio: ASPECT }
    } else {
        Camera::Orthogonal { eye: c.eye, direction, up: c.up, view_to_world_scale: 2.0 * c.parallel_scale, aspect_ratio: ASPECT }
    }
}

/// A BCF camera as a 3D view's, its target `distance` ahead of the eye (BCF has none).
pub fn view_camera(c: &Camera, distance: f64) -> Result<ViewCamera> {
    let (eye, direction, up) = match c {
        Camera::Perspective { eye, direction, up, .. } | Camera::Orthogonal { eye, direction, up, .. } => (*eye, *direction, *up),
    };
    let l = norm(direction);
    if l < 1e-12 {
        return Err(Error::new("bcf.bad_camera", "the camera has no direction"));
    }
    let target = [0, 1, 2].map(|k| eye[k] + direction[k] / l * distance);
    let base = ViewCamera::fitting([-1.0; 3], [1.0; 3]);
    let up = if norm(up) > 1e-12 { up } else { base.oriented(direction.map(|v| -v)).up };
    Ok(match c {
        Camera::Perspective { field_of_view, aspect_ratio, .. } => {
            let aspect = if *aspect_ratio > 0.0 { *aspect_ratio } else { ASPECT };
            let horizontal = 2.0 * ((field_of_view.to_radians() / 2.0).tan() * aspect).atan();
            ViewCamera { eye, target, up, perspective: true, field_of_view: horizontal.clamp(0.01, 3.1), parallel_scale: base.parallel_scale }
        }
        Camera::Orthogonal { view_to_world_scale, .. } => {
            ViewCamera { eye, target, up, perspective: false, field_of_view: base.field_of_view, parallel_scale: (view_to_world_scale / 2.0).max(1e-3) }
        }
    })
}

/// A 3D view's camera and cut as a viewpoint (its components left for the caller).
pub fn capture(store: &Store, view: &ElementRef) -> Result<(ElementId, Viewpoint)> {
    let (id, data) = views::resolve_view(store, view)?;
    if data.kind != views::KIND_3D {
        return Err(Error::new("bcf.not_3d", "viewpoints are taken from 3D views"));
    }
    Ok((
        id,
        Viewpoint {
            guid: None,
            camera: data.camera.as_ref().map(camera_of),
            clipping_planes: views::to_bcf(data.section_box.as_ref(), &data.slice_planes),
            ..Viewpoint::default()
        },
    ))
}

/// Sets a 3D view's camera, section box and slice planes from a viewpoint: the planes that form a
/// box become its section box, the rest slices (at most [`views::MAX_SLICES`]; more are an error).
/// The camera's target is put at the section box's centre when there is one, else 10 m ahead.
pub fn apply(txn: &mut crate::txn::Transaction<'_>, view: &ElementRef, v: &Viewpoint) -> Result<ElementId> {
    let store = txn.store().clone();
    let (id, mut data): (ElementId, ViewData) = views::resolve_view(&store, view)?;
    if data.kind != views::KIND_3D {
        return Err(Error::new("bcf.not_3d", "viewpoints are shown in 3D views"));
    }
    let (section_box, slices) = views::from_bcf(&v.clipping_planes);
    if slices.len() > views::MAX_SLICES {
        return Err(Error::new("bcf.too_many_planes", format!("{} slice planes; a view has at most {}", slices.len(), views::MAX_SLICES)));
    }
    if let Some(c) = &v.camera {
        let eye = match c {
            Camera::Perspective { eye, .. } | Camera::Orthogonal { eye, .. } => *eye,
        };
        let distance = section_box
            .as_ref()
            .map(|b| {
                let [x, y, z, o] = b.frame();
                let m = [0, 1, 2].map(|k| (b.min[k] + b.max[k]) / 2.0);
                let centre = [0, 1, 2].map(|k| o[k] + x[k] * m[0] + y[k] * m[1] + z[k] * m[2]);
                norm([0, 1, 2].map(|k| centre[k] - eye[k]))
            })
            .filter(|d| *d > 0.1)
            .unwrap_or(10.0);
        data.camera = Some(view_camera(c, distance)?);
    }
    data.section_box = section_box;
    data.slice_planes = slices;
    let value = serde_json::to_value(&data).expect("view data serialises");
    txn.modify(id, |e| {
        e.params.insert(views::VIEW_KEY.into(), value);
    })?;
    Ok(id)
}

fn xyz(tag: &str, p: [f64; 3]) -> String {
    format!("<{tag}><X>{}</X><Y>{}</Y><Z>{}</Z></{tag}>", p[0], p[1], p[2])
}

fn components(tag: &str, guids: &[String]) -> String {
    let mut s = format!("<{tag}>");
    for g in guids {
        s.push_str(&format!("<Component IfcGuid=\"{}\"/>", escape(g)));
    }
    s.push_str(&format!("</{tag}>"));
    s
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The viewpoint as a BCF 3.0 `.bcfv` document.
pub fn to_xml(v: &Viewpoint) -> String {
    write_xml(v, true)
}

/// The viewpoint as a BCF 2.1 `.bcfv` document (no aspect ratio, which 2.1 does not have).
pub fn to_xml_2_1(v: &Viewpoint) -> String {
    write_xml(v, false)
}

fn write_xml(v: &Viewpoint, aspect: bool) -> String {
    let ratio = |r: f64| if aspect { format!("<AspectRatio>{r}</AspectRatio>") } else { String::new() };
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    match &v.guid {
        Some(g) => s.push_str(&format!("<VisualizationInfo Guid=\"{}\">", escape(g))),
        None => s.push_str("<VisualizationInfo>"),
    }
    s.push_str("<Components>");
    if !v.selection.is_empty() {
        s.push_str(&components("Selection", &v.selection));
    }
    s.push_str(&format!("<Visibility DefaultVisibility=\"{}\">", v.visibility.default_visibility));
    if !v.visibility.exceptions.is_empty() {
        s.push_str(&components("Exceptions", &v.visibility.exceptions));
    }
    s.push_str("</Visibility></Components>");
    match &v.camera {
        Some(Camera::Perspective { eye, direction, up, field_of_view, aspect_ratio }) => s.push_str(&format!(
            "<PerspectiveCamera>{}{}{}<FieldOfView>{field_of_view}</FieldOfView>{}</PerspectiveCamera>",
            xyz("CameraViewPoint", *eye),
            xyz("CameraDirection", *direction),
            xyz("CameraUpVector", *up),
            ratio(*aspect_ratio)
        )),
        Some(Camera::Orthogonal { eye, direction, up, view_to_world_scale, aspect_ratio }) => s.push_str(&format!(
            "<OrthogonalCamera>{}{}{}<ViewToWorldScale>{view_to_world_scale}</ViewToWorldScale>{}</OrthogonalCamera>",
            xyz("CameraViewPoint", *eye),
            xyz("CameraDirection", *direction),
            xyz("CameraUpVector", *up),
            ratio(*aspect_ratio)
        )),
        None => {}
    }
    if !v.clipping_planes.is_empty() {
        s.push_str("<ClippingPlanes>");
        for c in &v.clipping_planes {
            s.push_str(&format!("<ClippingPlane>{}{}</ClippingPlane>", xyz("Location", c.location), xyz("Direction", c.direction)));
        }
        s.push_str("</ClippingPlanes>");
    }
    s.push_str("</VisualizationInfo>\n");
    s
}

/// Reads a `.bcfv` document (BCF 2.1 or 3.0).
pub fn from_xml(text: &str) -> Result<Viewpoint> {
    let bad = |m: String| Error::new("bcf.parse", m);
    let mut r = Reader::from_str(text);
    r.config_mut().trim_text(true);
    let mut v = Viewpoint::default();
    let mut path: Vec<String> = Vec::new();
    let mut buf = String::new();
    // The camera being read: perspective or not, eye, direction, up, field of view or scale,
    // aspect.
    type Reading = (bool, [f64; 3], [f64; 3], [f64; 3], f64, f64);
    let mut cam: Option<Reading> = None;
    let mut plane: Option<ClippingPlane> = None;
    let mut saw_root = false;
    loop {
        let ev = r.read_event().map_err(|e| bad(format!("at byte {}: {e}", r.buffer_position())))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = e.local_name().as_ref().to_owned();
                let attr = |k: &str| {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == k)
                        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::default()).ok().map(|v| v.into_owned()))
                };
                match name.as_str() {
                    "VisualizationInfo" => {
                        saw_root = true;
                        v.guid = attr("Guid");
                    }
                    "Visibility" => {
                        v.visibility.default_visibility = attr("DefaultVisibility").is_none_or(|d| d != "false" && d != "0");
                    }
                    "Component" => {
                        if let Some(g) = attr("IfcGuid") {
                            let parent = path.last().map(String::as_str);
                            if parent == Some("Selection") {
                                v.selection.push(g);
                            } else if parent == Some("Exceptions") {
                                v.visibility.exceptions.push(g);
                            }
                        }
                    }
                    "PerspectiveCamera" => cam = Some((true, [0.0; 3], [0.0; 3], [0.0; 3], 60.0, 0.0)),
                    "OrthogonalCamera" => cam = Some((false, [0.0; 3], [0.0; 3], [0.0; 3], 1.0, 0.0)),
                    "ClippingPlane" => plane = Some(ClippingPlane { location: [0.0; 3], direction: [0.0; 3] }),
                    _ => {}
                }
                buf.clear();
                if matches!(ev, Event::Start(_)) {
                    path.push(name);
                }
            }
            Event::Text(t) => buf.push_str(&t.xml10_content()),
            Event::End(_) => {
                let name = path.pop().unwrap_or_default();
                let parent = path.last().map(String::as_str).unwrap_or("");
                let num = || buf.trim().parse::<f64>().map_err(|_| bad(format!("{name}: not a number: {buf:?}")));
                let axis = match name.as_str() {
                    "X" => Some(0),
                    "Y" => Some(1),
                    "Z" => Some(2),
                    _ => None,
                };
                if let Some(k) = axis {
                    let x = num()?;
                    if let Some(c) = cam.as_mut() {
                        match parent {
                            "CameraViewPoint" => c.1[k] = x,
                            "CameraDirection" => c.2[k] = x,
                            "CameraUpVector" => c.3[k] = x,
                            _ => {}
                        }
                    }
                    if let Some(p) = plane.as_mut() {
                        match parent {
                            "Location" => p.location[k] = x,
                            "Direction" => p.direction[k] = x,
                            _ => {}
                        }
                    }
                }
                match name.as_str() {
                    "FieldOfView" | "ViewToWorldScale" => {
                        if let Some(c) = cam.as_mut() {
                            c.4 = num()?;
                        }
                    }
                    "AspectRatio" => {
                        if let Some(c) = cam.as_mut() {
                            c.5 = num()?;
                        }
                    }
                    "PerspectiveCamera" | "OrthogonalCamera" => {
                        if let Some((perspective, eye, direction, up, value, aspect)) = cam.take() {
                            let aspect_ratio = if aspect > 0.0 { aspect } else { ASPECT };
                            v.camera = Some(if perspective {
                                Camera::Perspective { eye, direction, up, field_of_view: value, aspect_ratio }
                            } else {
                                Camera::Orthogonal { eye, direction, up, view_to_world_scale: value, aspect_ratio }
                            });
                        }
                    }
                    "ClippingPlane" => {
                        if let Some(p) = plane.take() {
                            v.clipping_planes.push(p);
                        }
                    }
                    _ => {}
                }
                buf.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_root {
        return Err(bad("not a BCF viewpoint (no VisualizationInfo)".into()));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_viewpoint_round_trips_through_xml() {
        let v = Viewpoint {
            guid: Some("8dc86298-9737-40b4-a448-98a9e953293a".into()),
            camera: Some(Camera::Perspective { eye: [12.0, -8.0, 6.5], direction: [-0.6, 0.6, -0.5], up: [0.0, 0.0, 1.0], field_of_view: 45.0, aspect_ratio: 1.6 }),
            clipping_planes: vec![ClippingPlane { location: [0.0, 0.0, 1.5], direction: [0.0, 0.0, 1.0] }],
            selection: vec!["2BZPsLK5T2IQpUgT7vLWdi".into()],
            visibility: Visibility { default_visibility: false, exceptions: vec!["1xS3BCk291UvhgP2a6eflN".into(), "0$abc".into()] },
        };
        let back = from_xml(&to_xml(&v)).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn cameras_convert_both_ways() {
        let c = ViewCamera { eye: [10.0, -10.0, 8.0], target: [0.0, 0.0, 2.0], up: [0.0, 0.0, 1.0], perspective: true, field_of_view: 1.2, parallel_scale: 7.0 };
        let b = camera_of(&c);
        let Camera::Perspective { field_of_view, .. } = b else { panic!() };
        // 1.2 rad across at 16:10 is about 41.8° high.
        assert!((field_of_view - 2.0 * ((0.6f64).tan() / 1.6).atan().to_degrees()).abs() < 1e-9);
        let d = norm([0, 1, 2].map(|k| c.target[k] - c.eye[k]));
        let back = view_camera(&b, d).unwrap();
        for k in 0..3 {
            assert!((back.target[k] - c.target[k]).abs() < 1e-9 && (back.eye[k] - c.eye[k]).abs() < 1e-12);
        }
        assert!((back.field_of_view - 1.2).abs() < 1e-9);
        let o = camera_of(&ViewCamera { perspective: false, ..c });
        assert!(matches!(o, Camera::Orthogonal { view_to_world_scale, .. } if (view_to_world_scale - 14.0).abs() < 1e-12));
        assert!((view_camera(&o, d).unwrap().parallel_scale - 7.0).abs() < 1e-12);
    }

    /// A BCF 2.1 viewpoint written by another tool: no aspect ratio, components with other
    /// attributes, a selection and hidden exceptions.
    #[test]
    fn reads_bcf_2_1() {
        let text = r#"<?xml version="1.0" encoding="utf-8"?>
<VisualizationInfo xmlns:xsd="http://www.w3.org/2001/XMLSchema" Guid="a1">
  <Components>
    <ViewSetupHints SpacesVisible="false"/>
    <Selection><Component IfcGuid="3$Gk1kSGv0rRqxVXxGqY2b" OriginatingSystem="Other"/></Selection>
    <Visibility DefaultVisibility="true"><Exceptions><Component IfcGuid="0abc"/></Exceptions></Visibility>
  </Components>
  <OrthogonalCamera>
    <CameraViewPoint><X>1</X><Y>2</Y><Z>3</Z></CameraViewPoint>
    <CameraDirection><X>0</X><Y>0</Y><Z>-1</Z></CameraDirection>
    <CameraUpVector><X>0</X><Y>1</Y><Z>0</Z></CameraUpVector>
    <ViewToWorldScale>20</ViewToWorldScale>
  </OrthogonalCamera>
</VisualizationInfo>"#;
        let v = from_xml(text).unwrap();
        assert_eq!(v.selection, ["3$Gk1kSGv0rRqxVXxGqY2b"]);
        assert_eq!(v.visibility, Visibility { default_visibility: true, exceptions: vec!["0abc".into()] });
        assert!(matches!(v.camera, Some(Camera::Orthogonal { view_to_world_scale, aspect_ratio, .. }) if view_to_world_scale == 20.0 && aspect_ratio == ASPECT));
        assert!(from_xml("<Markup/>").is_err());
    }
}
