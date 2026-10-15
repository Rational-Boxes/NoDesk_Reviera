// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Architecture commands added for the first pre-release: wall openings and shafts
//! (ARC-OPEN-003, ARC-OPEN-004), and the tools after them. Registered with the core commands
//! by [`crate::builtin::register`].

use crate::command::{Command, CommandContext, CommandMeta, CommandRegistry, Effect};
use crate::error::Result;
use crate::id::ElementId;

/// A command that edits the model through one function of a transaction.
macro_rules! edit_command {
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, $txn:literal, $params:ty, $out:ty, $f:expr) => {
        #[derive(Debug)]
        struct $ty;

        impl Command for $ty {
            type Params = $params;
            type Output = $out;
            const META: CommandMeta = CommandMeta {
                name: $name,
                version: 1,
                summary: $summary,
                description: $desc,
                effects: &[Effect::Edit],
                transaction_name: $txn,
            };

            fn run(&self, cx: &mut CommandContext<'_>, p: $params) -> Result<$out> {
                let f: fn(&mut crate::txn::Transaction<'_>, $params) -> Result<$out> = $f;
                f(cx.edit()?, p)
            }
        }
    };
}

pub(crate) fn register(registry: &mut CommandRegistry) {
    let results = [
        registry.add(OpeningCreateWall),
        registry.add(OpeningEditWall),
        registry.add(OpeningCreateShaft),
        registry.add(OpeningEditShaft),
        registry.add(ColumnPlace),
        registry.add(ColumnSet),
        registry.add(ColumnAttach),
        registry.add(ColumnDetach),
        registry.add(ElementArrayLinear),
        registry.add(ElementArrayRadial),
        registry.add(CompoundGet),
        registry.add(CompoundSetLayers),
        registry.add(CurtainCreate),
        registry.add(CurtainSet),
        registry.add(CeilingSetShape),
        registry.add(RampCreate),
        registry.add(RampSet),
        registry.add(MeasureBetween),
        registry.add(MeasureChain),
        registry.add(MeasureAlong),
        registry.add(SkylightCreate),
        registry.add(SkylightSet),
        registry.add(MeshObjectInspect),
        registry.add(MeshObjectImport),
        registry.add(MeshObjectReload),
        registry.add(MeshObjectPlace),
        registry.add(MeshObjectSet),
        registry.add(MeshObjectSetType),
        registry.add(MeshObjectTypes),
    ];
    for r in results {
        r.expect("architecture commands are valid");
    }
}

edit_command!(
    OpeningCreateWall,
    "opening.create_wall",
    "Cut a rectangular opening through a wall",
    "Cuts a plain rectangular opening through a Reviera wall, straight or curved (ARC-WALL-012, \
     ARC-OPEN-003): from `u0` to `u1` along its location curve from its start, from its base level (the \
     wall's by default) and offset to a top level and offset or a height (2.1 m by default). It goes \
     through the whole thickness, square to the curve; it follows its levels, slides along the wall, \
     moves and is copied with the wall, and goes with it. It may not overlap the wall's doors, windows or \
     other openings. Written as an IfcOpeningElement voiding the wall. Returns the opening.",
    "Wall Opening",
    crate::wall_openings::CreateParams,
    ElementId,
    crate::wall_openings::create
);

edit_command!(
    OpeningEditWall,
    "opening.edit_wall",
    "Change a wall opening",
    "Changes a wall opening's span along its wall (`u0`, `u1`) or its extent (base level and offset, top \
     constraint and offset); omitted values are kept.",
    "Edit Wall Opening",
    crate::wall_openings::EditParams,
    (),
    crate::wall_openings::edit
);

edit_command!(
    OpeningCreateShaft,
    "opening.create_shaft",
    "Make a shaft through several levels",
    "Makes a shaft (ARC-OPEN-004): closed `loops` in plan (corners), or sketch `curves`, from a base level \
     and offset to a top level and offset or a height. It cuts every Reviera floor, ceiling and footprint \
     roof its extent reaches into, plumb, including ones added later; walls are not cut. In IFC it is an \
     IfcBuildingElementProxy (PROVISIONFORVOID) with each host voided by its own IfcOpeningElement. \
     Returns the shaft.",
    "Shaft Opening",
    crate::slab_openings::CreateShaftParams,
    ElementId,
    crate::slab_openings::create_shaft
);

edit_command!(
    OpeningEditShaft,
    "opening.edit_shaft",
    "Change a shaft",
    "Changes a shaft's loops or extent (base level and offset, top constraint and offset); omitted values \
     are kept.",
    "Edit Shaft",
    crate::slab_openings::EditShaftParams,
    (),
    crate::slab_openings::edit_shaft
);

edit_command!(
    ColumnPlace,
    "column.place_architectural",
    "Place an architectural column",
    "Places a non-structural column (ARC-COL-001) of a column type (the default 400 × 400 mm type when \
     omitted) centred at `point`, rotated by `rotation`, from a base level and offset to a top level \
     and offset or a height. Written as an IfcColumn (Pset_ColumnCommon.LoadBearing false). Returns \
     the column.",
    "Column",
    crate::columns::PlaceParams,
    ElementId,
    crate::columns::place
);

edit_command!(
    ColumnSet,
    "column.set",
    "Change columns",
    "Changes columns' centre, rotation, base level and offset, or top constraint and offset; omitted \
     values are kept.",
    "Edit Column",
    crate::columns::SetParams,
    (),
    crate::columns::set
);

edit_command!(
    ColumnAttach,
    "column.attach",
    "Attach columns' tops",
    "Attaches columns' tops to a Reviera roof, floor or ceiling above them (ARC-COL-001): the top \
     follows the target's underside over the column's section at its lowest point (`minimum`), middle \
     (`midline`) or highest point (`maximum`, the default), plus `offset`; with `maximum` and \
     `midline` the top is cut by the underside.",
    "Attach Column",
    crate::columns::AttachParams,
    (),
    crate::columns::attach
);

edit_command!(
    ColumnDetach,
    "column.detach",
    "Detach columns' tops",
    "Detaches columns' tops: they go back to their top constraint.",
    "Detach Column",
    crate::columns::DetachParams,
    (),
    crate::columns::detach
);

edit_command!(
    ElementArrayLinear,
    "element.array_linear",
    "Array elements in a row",
    "Copies elements into a linear array (ARC-MOD-026): `count` members including the original, the \
     `vector` reaching the second member (`move_to: second`) or the last (`move_to: last`). The members \
     are independent copies (Group and Associate is not available yet). Returns the copies, member by \
     member.",
    "Array",
    crate::modify::ArrayLinearParams,
    Vec<Vec<ElementId>>,
    crate::modify::array_linear
);

edit_command!(
    ElementArrayRadial,
    "element.array_radial",
    "Array elements round a centre",
    "Copies elements into a radial array (ARC-MOD-026) round `center`: `count` members including the \
     original, `angle` reaching the second member (`second`) or the last (`last`; a full turn spaces \
     them evenly without doubling the original). Doors and windows cannot be rotated off their wall. \
     Returns the copies, member by member.",
    "Radial Array",
    crate::modify::ArrayRadialParams,
    Vec<Vec<ElementId>>,
    crate::modify::array_radial
);

#[derive(Debug)]
struct CompoundGet;

impl Command for CompoundGet {
    type Params = crate::compound::GetParams;
    type Output = crate::compound::CompoundReport;
    const META: CommandMeta = CommandMeta {
        name: "compound.get",
        version: 1,
        summary: "A wall type's layers",
        description: "A wall type's compound structure (ARC-CMPD-001): its layers, exterior first (function, \
                      priority, material, thickness, wraps), its core and its total thickness. A type without \
                      layers of its own reads as one Structure layer of its width.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: crate::compound::GetParams,
    ) -> Result<crate::compound::CompoundReport> {
        crate::compound::get(cx.store(), p)
    }
}

edit_command!(
    CompoundSetLayers,
    "compound.set_layers",
    "Set a wall type's layers",
    "Gives a wall type its layers (ARC-CMPD-001), exterior first: each a `function` (structure, substrate, \
     thermal_air, finish1, finish2, membrane), an optional `priority` (1 highest … 9; the function's \
     default), a `material` name, a `thickness` (accepts units), and `wraps`, `structural_material` and \
     `variable` flags; and the core `core_start..core_end` (by default the Structure layers). They are \
     validated (thicknesses, priorities not rising away from the core, one variable and one structural \
     layer at most). The type's width becomes their total and its walls regenerate; IFC gets an \
     IfcMaterialLayerSet.",
    "Edit Assembly",
    crate::compound::SetLayersParams,
    (),
    crate::compound::set_layers
);

edit_command!(
    CurtainCreate,
    "curtain.create",
    "Make a curtain wall",
    "Makes a curtain wall (ARC-CW-001/002) along a straight base line, from a base level and offset to a \
     top level and offset or a height, of a curtain wall type (the default Storefront when omitted: \
     vertical grid lines at most 1.5 m apart, a transom at 2.4 m, 50 × 150 mm mullions, glazed panels). \
     Drawn along a basic wall, it cuts its opening through it (ARC-WALL-013). IFC: an IfcCurtainWall \
     aggregating IfcPlate panels and IfcMember mullions. Returns the curtain wall.",
    "Curtain Wall",
    crate::curtain::CreateParams,
    ElementId,
    crate::curtain::create
);

edit_command!(
    CurtainSet,
    "curtain.set",
    "Change curtain walls",
    "Changes curtain walls' base line, base and top, and the justification (`beginning`, `center`, \
     `end`) and offset of their vertical (`u_*`) and horizontal (`v_*`) grid layouts; omitted values are \
     kept. The type's layout rules, mullions and panels are type parameters (`type.set_param`).",
    "Edit Curtain Wall",
    crate::curtain::SetParams,
    (),
    crate::curtain::set
);

edit_command!(
    CeilingSetShape,
    "ceiling.set_shape",
    "Make ceilings flat or follow the roof",
    "Makes ceilings `flat`, or `follow_roof`: shaped to the underside of the roof over them (vaulted under \
     a pitched roof, curved under an arched roof or a dome), piece by piece under its faces; what the roof \
     does not cover stays flat. The roof is `roof_id`, or by default the roof over each ceiling.",
    "Ceiling Shape",
    crate::vaults::SetShapeParams,
    (),
    crate::vaults::set_shape
);

edit_command!(
    RampCreate,
    "ramp.create",
    "Make a ramp",
    "Makes a ramp (ARC-RAMP-001) along a centreline `path` (from the bottom up; each segment a run, a \
     square landing at each corner), `width` wide (1.2 m by default), from a base level and offset to a \
     top level and offset or a height, of a ramp type (the default 1:12, 9 m maximum incline, 150 mm \
     thick). Runs longer than the maximum incline length get landings as deep as the ramp is wide. A \
     ramp steeper than its type allows is made, with a warning. Returns the ramp.",
    "Ramp",
    crate::ramps::CreateParams,
    ElementId,
    crate::ramps::create
);

edit_command!(
    RampSet,
    "ramp.set",
    "Change ramps",
    "Changes ramps' path, width, base and top; omitted values are kept. The slope limit, maximum incline \
     length, thickness and shape (thick, solid) are type parameters (`type.set_param`).",
    "Edit Ramp",
    crate::ramps::SetParams,
    (),
    crate::ramps::set
);

/// A read-only command of a pure function.
macro_rules! read_command {
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, $params:ty, $out:ty, $f:expr) => {
        #[derive(Debug)]
        struct $ty;

        impl Command for $ty {
            type Params = $params;
            type Output = $out;
            const META: CommandMeta = CommandMeta {
                name: $name,
                version: 1,
                summary: $summary,
                description: $desc,
                effects: &[Effect::Read],
                transaction_name: "",
            };

            fn run(&self, cx: &mut CommandContext<'_>, p: $params) -> Result<$out> {
                let f: fn(&crate::store::Store, $params) -> Result<$out> = $f;
                f(cx.store(), p)
            }
        }
    };
}

read_command!(
    MeasureBetween,
    "measure.between",
    "Measure between two points",
    "The distance between two points ([x, y] or [x, y, z], metres) (ARC-MOD-023): its x, y and z \
     components, the plan distance and its angle from the x axis (degrees). Changes nothing.",
    crate::measure::BetweenParams,
    crate::measure::Between,
    |_, p| crate::measure::between(p)
);

read_command!(
    MeasureChain,
    "measure.chain",
    "Measure a chain of points",
    "The lengths of the segments of a chain of points, and their total (ARC-MOD-023).",
    crate::measure::ChainParams,
    crate::measure::Chain,
    |_, p| crate::measure::chain(p)
);

read_command!(
    MeasureAlong,
    "measure.along",
    "Measure along an element",
    "The length of a wall (along its location curve), curtain wall, ramp (its path) or grid line \
     (ARC-MOD-023).",
    crate::measure::AlongParams,
    f64,
    crate::measure::along
);

edit_command!(
    SkylightCreate,
    "skylight.create",
    "Place a skylight in a roof",
    "Places a skylight in a Reviera footprint roof at plan `point`, cutting its own opening: a `roof_window` \
     in a pitched roof (in the slope; `width` across it, `length` up it), or on a flat roof on a `curb` \
     a `flat` glass sheet, a `pyramid`, or a `ridge_lantern` (two glass slopes at `pitch` to a ridge along \
     its `length`, `rotation` from the x axis), whose panes are re-mapped from its length so none is wider \
     than `max_pane`. Returns the skylight (an IfcWindow, SKYLIGHT).",
    "Skylight",
    crate::skylights::CreateParams,
    ElementId,
    crate::skylights::create
);

edit_command!(
    SkylightSet,
    "skylight.set",
    "Change skylights",
    "Changes skylights' centre, style, size, rotation, curb, pitch or largest pane; omitted values are kept.",
    "Edit Skylight",
    crate::skylights::SetParams,
    (),
    crate::skylights::set
);

read_command!(
    MeshObjectInspect,
    "mesh_object.inspect",
    "Inspect a mesh file",
    "Reads a mesh file (OBJ with its MTL, STL, PLY, glTF, GLB) with import `options` (units, up axis, \
     insertion point, clean-up) and reports what it holds without importing it (DM-IMP-015): format, \
     triangles, vertices, materials, its size in metres, the units and up axis used and whether the file \
     stated them, open and non-manifold edges and whether it is a closed solid.",
    crate::mesh_objects::InspectParams,
    crate::mesh_objects::Report,
    crate::mesh_objects::inspect
);

edit_command!(
    MeshObjectImport,
    "mesh_object.import",
    "Import a mesh as a custom object type",
    "Imports a mesh file (OBJ, STL, PLY, glTF, GLB) as a custom object type (DM-IMP-015): the mesh is \
     stored once in the type, in metres and Z up, with its insertion point (bottom centre by default) at \
     the origin; each material becomes a slot with its colour. `category` (generic_model, furniture, \
     column, plumbing_fixture, lighting_fixture, planting) decides the IFC classes. Meshes above \
     1,000,000 triangles need `allow_large`; above 5,000,000 they are refused. Returns the type; place \
     copies with mesh_object.place.",
    "Import Object",
    crate::mesh_objects::ImportParams,
    ElementId,
    crate::mesh_objects::import
);

edit_command!(
    MeshObjectReload,
    "mesh_object.reload",
    "Reload a custom object type from its file",
    "Re-reads a custom object type's mesh from its source file, or from `path`, with its own or new \
     import `options`. Every copy changes; slot colours are kept where slot names still match.",
    "Reload from File",
    crate::mesh_objects::ReloadParams,
    (),
    crate::mesh_objects::reload
);

edit_command!(
    MeshObjectPlace,
    "mesh_object.place",
    "Place a custom object",
    "Places a copy of a custom object type at plan `point` on `level`, `offset` above it, turned by \
     `rotation` and optionally mirrored. Copies share the type's mesh. Returns the copy.",
    "Place Object",
    crate::mesh_objects::PlaceParams,
    ElementId,
    crate::mesh_objects::place
);

edit_command!(
    MeshObjectSet,
    "mesh_object.set",
    "Change custom objects",
    "Changes custom objects' point, level, offset, rotation or mirror; omitted values are kept.",
    "Edit Object",
    crate::mesh_objects::SetParams,
    (),
    crate::mesh_objects::set
);

edit_command!(
    MeshObjectSetType,
    "mesh_object.set_type",
    "Change a custom object type",
    "Changes a custom object type's name, uniform `scale` (every copy) and slot colours, and its \
     `category` while it has no copies.",
    "Edit Object Type",
    crate::mesh_objects::SetTypeParams,
    (),
    crate::mesh_objects::set_type
);

read_command!(
    MeshObjectTypes,
    "mesh_object.types",
    "List custom object types",
    "The project's custom object types: name, category, triangles, size, scale, material slots, source \
     file and how many copies each has.",
    crate::mesh_objects::TypesParams,
    Vec<crate::mesh_objects::TypeInfo>,
    crate::mesh_objects::list_types
);
