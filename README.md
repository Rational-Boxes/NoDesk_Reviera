# NoDesk Reviera

**Open-source BIM authoring on native IFC: build anywhere, even from the beach.**

![A laptop showing a building model on a beach table, next to a "Beach Life" mug](design_documents/beach_life.jpg)

Reviera is a parametric Building Information Modeling (BIM) application for architecture,
structure and MEP. IFC is its native file format, so there is no proprietary container. Every
operation can be driven by Lua scripts and by AI agents through the Model Context Protocol (MCP).

> **Status: early development.** The specification is complete as a draft. The application
> core is being built, and the first window shows models in an interactive 3D viewport.

## Working with CADCraft

Reviera is meant to work alongside [CADCraft](https://github.com/storytold/cadcraft), an
open-source 2D drafting application in Rust. The goal is a two-way exchange through DWG and
DXF files: drawings made in CADCraft (site plans, surveys, details, consultant drawings) come
into Reviera as imported or linked CAD, and Reviera's plans and views go back to CADCraft as
DXF or DWG. Both applications read these files with the same library (`acadrust`), so a
drawing is read the same way in each. A corpus of CADCraft-made drawings will check
compatibility continuously, in both directions. The requirements are DM-IMP-013 and DM-IMP-014
in the specification (Chapter 2, §2.11.10–2.11.11). CAD import is not built yet.

## Screenshots

The KIT FZK *AC20-Institute-Var-2* sample model (`test-data/ifc/`) open in Reviera, October 2026.

![A door selected in the 3D view, with the contextual Modify | Doors tab](design_documents/application_screenshots/3D-view.png)

![The Erdgeschoss floor plan: cut walls, stairs and furniture](design_documents/application_screenshots/plan-view.png)

![A section box cutting the Institute model open, in isometric projection](design_documents/application_screenshots/section-box.png)

## What works today

- **Native IFC** through IfcOpenShell and OpenCASCADE: open, edit, save and tessellate IFC2X3,
  IFC4 and IFC4X3_ADD2 models, with lossless round trips of files from other tools.
- **Element identity** that survives save and reopen. Reviera's own data is embedded in the IFC
  file as standard `Reviera_*` property sets (RXD), so the file stays valid for every IFC tool.
- **Transactions, undo and redo**, with one command registry. Each command has a JSON Schema, a
  permission level and attribution (user, script or agent).
- **Crash recovery.** Every change is mirrored to an encrypted SQLite live store (SQLCipher, with
  the key kept in the OS keyring). After a crash, unsaved work and its undo history come back.
- **Tabbed views**: several models open side by side, each view in its own tab with a `wgpu`
  3D viewport (Window › New 3D View, Tile Views, Tab Views, Ctrl+Tab); a dockable Properties
  palette.
- **Views as model objects**: 3D views are saved in the IFC file (RXD), with a Project Browser
  to open, rename, duplicate and delete them; each view's camera and Home come back on reopen.
  `view.*` commands (create, list, rename, orient, set_camera, open, …) work from MCP too.
- **Floor plans**: View › New Floor Plan creates a plan per level, drawn as a top-down cut at
  the view range's cut plane with cut poché and lines on white; the cut plane is editable in
  Properties, and the Wall tool draws on the plan's level (`view.create_plan`,
  `view.set_view_range`, `view.get_view_range`).
- **Levels**: Architecture › Level (LL) adds a level at an elevation with its floor plan, named
  after the level below (*Level 2* → *Level 3*). Levels are listed in the Project Browser; their
  Name and Elevation are edited in Properties. Moving a level moves the walls on it and stretches
  the walls up to it, keeping offsets; renaming one renames its plans. Deleting a level lists
  what goes with it first. Saved as `IfcBuildingStorey` aggregated under the building
  (`level.create`, `level.set_elevation`, `level.rename`, `level.list`).
- **Grids**: Architecture › Grid (GR) draws grid lines in plans with snapping; names continue
  the series (1, 2, 3 …; rename one to A and the next are B, C). Grids show in every plan and in
  PDF output with their bubbles, can be selected, renamed in Properties and dimensioned; dimensions
  follow a moved grid. Saved as one `IfcGrid` with an `IfcGridAxis` per line (`grid.create`,
  `grid.rename`, `grid.set_curve`, `grid.list`). Axes moved, retagged, added or removed in
  another tool are taken over when the file opens.
- **Grid types**: each grid has a type that sets its bubble symbol, a continuous, gapped or
  custom centre segment, and the pen, colour and line pattern of its centre and end segments.
  Choose, duplicate and edit types in Properties (also for the Grid tool); `type.duplicate`,
  `type.rename`, `type.set_param`, `type.list` and `element.set_type` work for grid and wall
  types alike.
- **Floors**: Architecture › Floor (FL) opens sketch mode on a contextual *Modify | Create Floor
  Boundary* tab: Pick Walls (the face on the cursor's side, locked so the floor follows the
  wall; Tab picks a chain of joined walls), Line and Rectangle, then Finish or Cancel. The
  sketch is checked (open ends, branches, crossings and overlaps are marked), inner loops make
  holes, and Edit Boundary changes it later. Floors have types with a thickness, a height
  offset, and area, perimeter and volume in Properties; they are saved as `IfcSlab` with an
  extruded profile (`floor.create`, `floor.edit_boundary`, `floor.set_level`).
- **Ceilings and ceiling plans**: Architecture › Ceiling places an automatic ceiling in the room
  under the cursor (its edges follow the walls' finish faces, and move with them), or Sketch
  Ceiling. Ceilings have types with a thickness and sit 2600 mm above their level by default.
  View › Ceiling Plan makes reflected ceiling plans, cut at 2300 mm and looking up. Saved as
  `IfcCovering` (CEILING) (`ceiling.create_auto`, `ceiling.create`, `ceiling.edit_boundary`).
- **Sections and elevations**: View › Section draws a section line in a plan with two clicks
  (the view looks to the line's right). Building Elevations makes North, South, East and West
  elevations around the model. These views draw like plans of the model turned into their frame:
  cut walls and floors as poché, everything up to the far clip in projection (door panels
  included), level lines with names and elevations, and grid lines with their bubbles. Plans show
  section lines and elevation markers. Properties set the far clip and flip the view; the views
  export to PDF (`view.create_section`, `view.create_elevation`, `section.edit`). Not yet:
  crop regions, jogged sections, multi-direction and interior elevation markers, and opening a
  view from its marker.
- **Rooms**: Architecture › Room places a room in the region of wall faces under the cursor,
  shown as you hover. Place Rooms Automatically fills every enclosed region of the plan's level,
  numbered in reading order. Rooms measure to the walls' finish faces and leave out islands such
  as free-standing walls. A door does not join the rooms on its two sides. A room follows its
  walls, and becomes Not Enclosed if they open up. Plans label each room with its name, number
  and area (in PDF too); click a label to select the room. Saved as `IfcSpace` (Name = Number,
  LongName = Name) under its storey, with a body up to its limit (`room.place`,
  `room.place_automatically`, `room.set`). Not yet: room separation lines, room tags as
  elements, other area rules, and volumes clipped by floors and roofs.
- **Roofs**: Architecture › Roof sketches a footprint in a plan. Pick Walls takes the walls'
  exterior faces (Tab takes the whole chain) with a 600 mm overhang and puts the roof on the
  walls' tops. Line and Rectangle work too. Defines Slope switches curves between sloping and
  gable. A weighted straight skeleton makes hip, gable, shed, pavilion and cut-off (mansard)
  roofs from any simple footprint, including different slopes per edge, and each face gets a
  mitred slab of the roof type's thickness. Properties edit the base offset, slope, cutoff and
  type, and Edit Footprint reopens the sketch. Saved as `IfcRoof` with a faceted B-rep body
  (`roof.create_footprint`, `roof.edit_footprint`, `roof.set_params`, `roof.set_edge`,
  `roof.set_slope`). Walls attach their tops to a roof (offered when a roof is made from picked
  walls, or Modify | Walls › Attach Top/Base, then click the roof), so gable walls rise to the
  ridge. Saved as a faceted B-rep in the wall's own frame, so doors and windows stay cut
  (`wall.attach`, `wall.detach`). Not yet: attaching bases or to floors and ceilings, slope
  arrows, and openings in roofs.
- **Inspecting the model**: View › Section Box clips a 3D view to a box whose face grips drag in
  real time; Selection Box (BX) fits it around the selection. Horizontal and Vertical Slice cut
  the view at a clicked point (up to eight planes, each with a grip). Cut solids are capped.
  Section boxes and slice planes are saved with the view and map exactly to BCF clipping planes,
  for issues raised by FileEngine. Hide, Isolate, Isolate with ghosting and Translucent work on
  elements, on elements with their children (a storey's contents, a wall's doors and windows),
  by category or by type (HH, HI, HC, IC, HG, HT; HR resets). This is temporary per-view state
  that is never saved or printed. The Model Tree pane (MT) shows Project › Site › Building ›
  storeys › categories › elements, with hosted elements under their hosts, and hides, isolates or
  boxes whole branches from its context menu (`view.set_section_box`, `view.set_slice_planes`,
  `view.fit_section_box`, `view.temp_*`, `model.tree`).
- **Doors and windows**: Architecture › Door (DR) and Window (WN) place them in Reviera walls in
  plans or 3D: the preview follows the cursor along the wall, the cursor's side sets the facing
  and Spacebar flips a door's hand. Walls are cut, plans show door swings, and each new door or
  window gets the next Mark. Drag a selected one along its wall; edit its sill height, flips
  and type (with door and window types to duplicate and resize) in Properties. Saved as
  `IfcDoor`/`IfcWindow` filling an `IfcOpeningElement` that voids the wall (`door.place`,
  `window.place`, `insert.flip_hand`, `insert.flip_facing`, `insert.set_sill_height`,
  `element.move`).
- **Dimensions**: Annotate › Aligned Dimension (DI) in plans dimensions parallel wall faces,
  centrelines and cut edges; dimensions to Reviera walls follow them when they move
  (`dim.create_aligned`, `anno.list`). The Wall tool snaps to cut edges in plans.
- **Line weights**: plans draw cut and projection lines with each category's pens
  (`objectstyle.set`) at paper widths for the view scale (`lineweight.set`, `add_scale`),
  so a plan on screen at 100 % matches its print.
- **PDF**: File › Export PDF writes plan views as vector PDF at their scale on the smallest ISO
  sheet (A4–A0), with pen widths in paper millimetres, dimension values as real text and a title
  strip (`export.pdf`).
- **Ribbon and menus**: a tool ribbon with Architecture, Annotate, View and Modify tabs (large
  and small icon buttons in titled panels, the Select panel first), a Quick Access Toolbar
  above it, and a contextual *Modify | <category>* tab while elements are selected (for
  example Flip Hand and Flip Facing for doors). The ribbon, toolbar and menus are built from
  one catalog, `ribbon-default.toml`, checked against the command registry.
- **Editing**: the Modify tools Align (AL), Offset (OF), Mirror by picked or drawn axis (MM,
  DM), Move (MV), Copy (CO), Rotate (RO), Trim/Extend to Corner (TR), Trim/Extend to a
  boundary, Split (SL), Pin (PN) and Unpin (UP), in plans, with previews, snapping, and typed
  distances and angles. Selected walls and grids show end grips to drag, and can be dragged
  to move; doors and windows slide along their wall. Walls carry their doors and windows when
  they move, are copied or are mirrored, and reshaping a wall leaves its doors in place
  (`element.move`, `element.copy`, `element.rotate`, `element.mirror`, `element.offset`,
  `element.split`, `element.trim_corner`, `element.trim_extend`, `element.align`,
  `element.pin`, `element.unpin`).
- **Navigation**: an Orientation Cube in each 3D view (26 click regions, compass, Home, and a
  PERSP/ISO projection toggle), standard and isometric views with animated transitions, wheel
  zoom about the cursor, orbit about the selection, Previous/Next pan/zoom, and walk mode
  (View › Walk: WASD or arrow keys, drag to look). Click to select; drag to orbit; middle or right
  button to pan.
- **Walls**: *Architecture > Wall* draws chains of straight walls on a level, snapping to wall ends,
  midpoints, wall lines and 15° angles; type a length (`4.5 m`, `12' 6"`) and press Enter, and
  Spacebar flips the exterior side. Walls appear at once,
  regenerated from their recipe (`wall.create`, `wall.set_location`). Walls that meet join at
  mitred L corners and T junctions (`wall.set_join_allowed` turns a join off). Saving writes standard IFC
  (`IfcWall` with an `Axis` and a `SweptSolid` body, typed, contained in its storey and
  connected to its neighbours with `IfcRelConnectsPathElements`), so other
  tools see them, and Reviera reopens them as editable walls.
- **Units and parameters**: SI storage with project display units (metric and imperial,
  feet-and-inches), built-in, project and shared parameters routed to types or instances.
- **MCP server** for AI agents: every command is a tool, with toolsets, preview and confirm for
  destructive changes, and resources for documents and elements.

## Building

Linux (Fedora) is the development platform today; Windows 10+ and macOS are targets.

```sh
# Toolchain and libraries (Fedora)
sudo dnf install rustup cmake ninja-build gcc-c++ opencascade-devel boost-devel eigen3-devel \
    libxml2-devel json-devel perl-FindBin perl-IPC-Cmd perl-Time-Piece
rustup-init

# The first build also downloads and compiles wxWidgets 3.3.3 (through wxDragon).
# IfcOpenShell 0.8.5, built into third_party/ (about 15 minutes)
tools/build-ifcopenshell.sh

cargo build
cargo test --workspace
```

## Trying it

```sh
cargo run -- gui model.ifc             # the main window with the 3D viewport
cargo run -- model.ifc                 # open a file and print a summary
cargo run -- shell model.ifc           # run commands: `:commands`, `file.save`, `edit.undo {"steps": 2}`
cargo run                              # list unsaved work left by a crash
```

To connect an AI agent over MCP, for example Claude Code:

```sh
claude mcp add reviera -- /path/to/reviera --mcp-stdio --permission model_edit /path/to/model.ifc
```

Permission levels are `read_only` (the default), `model_edit` and `full_scripting`.

## Repository layout

| Path | Contents |
|---|---|
| `apps/reviera` | The `reviera` program |
| `crates/reviera-core` | Element store, transactions, undo, command registry |
| `crates/reviera-ifc` | IfcOpenShell bridge (C++ shim through `cxx`) |
| `crates/reviera-rxd` | Reviera Extension Data encoding |
| `crates/reviera-store` | Local `.ifc` projects with embedded RXD |
| `crates/reviera-livestore` | Encrypted live store and crash recovery |
| `crates/reviera-app` | Application session and file, live-store and recovery commands |
| `crates/reviera-mcp` | MCP server |
| `crates/…` | Rendering, Godot, UI, Lua, BIMserver and OpenCDE crates (placeholders) |
| `design_documents/` | The specification (`spec/`), technology stack, research log and source register |
| `schemas/`, `clients/rsp/` | Open data formats and protocol clients (MIT) |
| `test-data/ifc/` | Sample IFC models for tests (see their README for provenance) |

The specification is in `design_documents/spec/`, collected into
`design_documents/Revit_Application_Specification.md`.

## Licence

Reviera is licensed under the **GNU General Public License v3.0 or later** (see `LICENSE`). The
RXD schemas (`schemas/rxd/`), the Reviera Scene Protocol (`schemas/rsp/`) and its clients
(`clients/rsp/`) are MIT-licensed, so any tool can adopt them.

## Clean-room statement

Reviera's functional reference is a commercial BIM product, described only from publicly
available sources (online help, public blogs, tutorials, forums and open-source repositories),
all listed in `design_documents/SOURCE_REGISTER.md`. No proprietary, confidential or leaked
material, vendor code, binaries or file formats were used. Autodesk and Revit are trademarks of
Autodesk, Inc. Reviera is not affiliated with or endorsed by Autodesk.
