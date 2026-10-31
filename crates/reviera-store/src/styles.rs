// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Default surface styles. Geometry without a style shows white in most IFC viewers (xeokit
//! among them), so a model of white walls on white floors is hard to read. On save, every body
//! item of a product or type that has no `IfcStyledItem` gets one, with the colour Reviera's own
//! views give its class ([`reviera_core::display_colors`]): an `IfcSurfaceStyle` with an
//! `IfcSurfaceStyleShading` (and its transparency), one per colour, shared by every item.
//!
//! Styles written by other tools, or by the user's own materials, are never changed. Reviera's
//! default styled items are named [`TAG`], so a later save can recognise them; ones whose item
//! has been removed are removed too.

use std::collections::{HashMap, HashSet};

use reviera_core::display_colors::class_color;
use reviera_ifc::{AttrValue, IfcModel, StepId};

use crate::Result;

/// The `Name` of Reviera's default `IfcStyledItem`s.
pub(crate) const TAG: &str = "Reviera default";
/// The prefix of its shared `IfcSurfaceStyle` names, followed by the class (`IfcWindow:glass`).
const STYLE_PREFIX: &str = "Reviera default: ";

fn r(id: StepId) -> AttrValue {
    AttrValue::Ref(id.0)
}

fn create(ifc: &mut IfcModel, class: &str, attrs: &[(&str, AttrValue)]) -> Result<StepId> {
    let id = ifc.create(class)?;
    for (name, value) in attrs {
        ifc.set_attribute(id, name, value)?;
    }
    Ok(id)
}

/// The shared styles already in the file and those made while saving, by class.
struct Styles {
    by_class: HashMap<String, StepId>,
}

impl Styles {
    fn read(ifc: &IfcModel) -> Result<Self> {
        let all = ifc.entities_of_type("IfcSurfaceStyle")?;
        let ids: Vec<StepId> = all.iter().map(|e| e.id).collect();
        let mut by_class = HashMap::new();
        for (id, e) in ids.iter().zip(ifc.attributes_many(&ids)?) {
            if let Some(class) = e.string("Name").and_then(|n| n.strip_prefix(STYLE_PREFIX)) {
                by_class.insert(class.to_owned(), *id);
            }
        }
        Ok(Self { by_class })
    }

    /// The style for a render class (`IfcWall`, `IfcWindow:glass`), made the first time.
    fn get(&mut self, ifc: &mut IfcModel, class: &str) -> Result<StepId> {
        if let Some(id) = self.by_class.get(class) {
            return Ok(*id);
        }
        // Rounded: the table's f32 values would otherwise print as 0.9300000071525574.
        let [red, green, blue, alpha] = class_color(class).map(|v| (f64::from(v) * 1000.0).round() / 1000.0);
        let colour = create(ifc, "IfcColourRgb", &[("Red", AttrValue::Real(red)), ("Green", AttrValue::Real(green)), ("Blue", AttrValue::Real(blue))])?;
        let mut shading = vec![("SurfaceColour", r(colour))];
        if alpha < 1.0 {
            shading.push(("Transparency", AttrValue::Real(1.0 - alpha)));
        }
        let shading = create(ifc, "IfcSurfaceStyleShading", &shading)?;
        let style = create(
            ifc,
            "IfcSurfaceStyle",
            &[("Name", AttrValue::String(format!("{STYLE_PREFIX}{class}"))), ("Side", AttrValue::Enum("BOTH".into())), ("Styles", AttrValue::List(vec![r(shading)]))],
        )?;
        self.by_class.insert(class.to_owned(), style);
        Ok(style)
    }

}

/// The items of an element's body representations (its own, or its type's maps).
fn body_items(ifc: &IfcModel, element: StepId) -> Result<Vec<StepId>> {
    let e = ifc.attributes_many(&[element])?.remove(0);
    let mut reps = Vec::new();
    for shape in e.refs("Representation") {
        reps.extend(ifc.attributes_many(&[shape])?.remove(0).refs("Representations"));
    }
    for map in e.refs("RepresentationMaps") {
        reps.extend(ifc.attributes_many(&[map])?.remove(0).refs("MappedRepresentation"));
    }
    let mut items = Vec::new();
    for rep in ifc.attributes_many(&reps)? {
        if matches!(rep.string("RepresentationIdentifier"), Some("Body") | None) {
            items.extend(rep.refs("Items"));
        }
    }
    Ok(items)
}

/// Styles an element's body items by class, in order (an insert's frame, glass and panel):
/// replaces Reviera's default style on each, and adds one where there is none. Items styled by
/// anything else are left alone.
pub(crate) fn style_items(ifc: &mut IfcModel, element: StepId, classes: &[String]) -> Result<()> {
    let items = body_items(ifc, element)?;
    if items.len() != classes.len() {
        return Ok(());
    }
    let mut styles = Styles::read(ifc)?;
    for (item, class) in items.into_iter().zip(classes) {
        let style = styles.get(ifc, class)?;
        let existing = ifc.referenced_by(item, "IfcStyledItem")?;
        let ours: Vec<StepId> = ifc.attributes_many(&existing)?.iter().zip(&existing).filter(|(e, _)| e.string("Name") == Some(TAG)).map(|(_, id)| *id).collect();
        if existing.len() > ours.len() {
            continue;
        }
        let value = AttrValue::List(vec![r(style)]);
        match ours.first() {
            Some(styled) => ifc.set_attribute(*styled, "Styles", &value)?,
            None => {
                create(ifc, "IfcStyledItem", &[("Item", r(item)), ("Styles", value), ("Name", AttrValue::String(TAG.into()))])?;
            }
        }
    }
    Ok(())
}

/// The render class whose colour an element's items take: its IFC class, a type's occurrence
/// class (`IfcDoorType` → `IfcDoor`).
fn render_class(class: &str) -> String {
    let c = class.strip_suffix("StandardCase").unwrap_or(class);
    // IFC names: IfcWallType, IfcDoorStyle (IFC2X3), IfcSanitaryTerminalType.
    let c = c.strip_suffix("Type").or_else(|| c.strip_suffix("Style")).unwrap_or(c);
    c.to_owned()
}

/// Gives every unstyled body item of a product or type its class's default style, and removes
/// Reviera's default styled items whose item is gone.
pub(crate) fn write_default_styles(ifc: &mut IfcModel) -> Result<()> {
    // What is styled already, and Reviera's own items left without an item.
    let styled = ifc.entities_of_type("IfcStyledItem")?;
    let ids: Vec<StepId> = styled.iter().map(|e| e.id).collect();
    let mut has_style = HashSet::new();
    for (id, e) in ids.iter().zip(ifc.attributes_many(&ids)?) {
        match e.refs("Item").first() {
            Some(item) => {
                has_style.insert(*item);
            }
            None if e.string("Name") == Some(TAG) => ifc.remove(*id)?,
            None => {}
        }
    }
    let mut owners: Vec<(StepId, String)> = Vec::new();
    for class in ["IfcProduct", "IfcTypeProduct"] {
        owners.extend(ifc.entities_of_type(class)?.into_iter().map(|e| (e.id, e.class)));
    }
    let mut styles = Styles::read(ifc)?;
    let mut done = HashSet::new();
    for (owner, class) in owners {
        // Openings are cut away, not shown.
        if class.eq_ignore_ascii_case("IfcOpeningElement") || class.eq_ignore_ascii_case("IfcOpeningStandardCase") {
            continue;
        }
        let items = body_items(ifc, owner)?;
        if items.is_empty() {
            continue;
        }
        let classes = ifc.attributes_many(&items)?;
        let class = render_class(&class);
        for (item, e) in items.into_iter().zip(classes) {
            // A mapped item takes its style from the map's own items.
            if e.class.eq_ignore_ascii_case("IfcMappedItem") || has_style.contains(&item) || !done.insert(item) {
                continue;
            }
            let style = styles.get(ifc, &class)?;
            let value = AttrValue::List(vec![r(style)]);
            create(ifc, "IfcStyledItem", &[("Item", r(item)), ("Styles", value), ("Name", AttrValue::String(TAG.into()))])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::render_class;

    #[test]
    fn types_take_their_occurrences_colours() {
        assert_eq!(render_class("IfcWallStandardCase"), "IfcWall");
        assert_eq!(render_class("IfcDoorType"), "IfcDoor");
        assert_eq!(render_class("IfcDoorStyle"), "IfcDoor");
        assert_eq!(render_class("IfcSanitaryTerminalType"), "IfcSanitaryTerminal");
    }
}
