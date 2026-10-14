# NoDesk Reviera

**Open-source BIM authoring on native IFC: build anywhere, even from the beach.**

![A laptop showing a building model on a beach table, next to a "Beach Life" mug](design_documents/beach_life.jpg)

Reviera is a parametric Building Information Modeling (BIM) application for architecture,
structure and MEP. IFC is its native file format, so there is no proprietary container. Every
operation can be driven by Lua scripts and by AI agents through the Model Context Protocol (MCP).

> **Status: early development.** The specification is complete as a draft. The application
> models walls, doors and windows, floors, ceilings, roofs, rooms, levels and grids; draws plans,
> ceiling plans, sections and elevations; lays them out with schedules on sheets with title blocks
> and prints them to PDF; tags and annotates plans; keeps live schedules; and has a ribbon-based
> window with 3D inspection tools. Stairs, railings and layered walls are next.

## Screenshots

The KIT FZK *AC20-Institute-Var-2* sample model (`test-data/ifc/`) open in Reviera, October 2026.

![A door selected in the 3D view, with the contextual Modify | Doors tab](design_documents/application_screenshots/3D-view.png)

![The Erdgeschoss floor plan: cut walls, stairs and furniture](design_documents/application_screenshots/plan-view.png)

![A section box cutting the Institute model open, in isometric projection](design_documents/application_screenshots/section-box.png)

## What works today

### Platform

- **Native IFC** through IfcOpenShell and OpenCASCADE: open, edit, save and tessellate IFC2X3,
  IFC4 and IFC4X3_ADD2 models, with lossless round trips of files from other tools.
- **Element identity** that survives save and reopen. Reviera's own data is embedded in the IFC
  file as standard `Reviera_*` property sets (RXD), so the file stays valid for every IFC tool.
- **Transactions, undo and redo**, with one command registry. Each command has a JSON Schema, a
  permission level and attribution (user, script or agent).
- **Crash recovery.** Every change is mirrored to an encrypted SQLite live store (SQLCipher, with
  the key kept in the OS keyring). After a crash, unsaved work and its undo history come back.
- **Units and parameters**: SI storage with project display units (metric and imperial,
  feet-and-inches), built-in, project and shared parameters routed to types or instances.
- **MCP server** for AI agents: every command is a tool, with toolsets, preview and confirm for
  destructive changes, and resources for documents and elements.

### Modelling

- **Walls**: *Architecture > Wall* draws chains of straight walls on a level, snapping to wall ends,
  midpoints, wall lines and 15° angles; type a length (`4.5 m`, `12' 6"`) and press Enter, and
  Spacebar flips the exterior side. Walls appear at once,
  regenerated from their recipe (`wall.create`, `wall.set_location`). Walls that meet join at
  mitred L corners and T junctions (`wall.set_join_allowed` turns a join off). Saving writes standard IFC
  (`IfcWall` with an `Axis` and a `SweptSolid` body, typed, contained in its storey and
  connected to its neighbours with `IfcRelConnectsPathElements`), so other
  tools see them, and Reviera reopens them as editable walls.
- **Doors and windows**: Architecture › Door (DR) and Window (WN) place them in Reviera walls in
  plans or 3D: the preview follows the cursor along the wall, the cursor's side sets the facing
  and Spacebar flips a door's hand. Walls are cut, plans show door swings, and each new door or
  window gets the next Mark. A new one fits the wall where it lands: it slides or shrinks into
  the free gap, and on a dormer it rises above the main roof and shrinks under the dormer roof
  (the preview shows the fitted size). Drag a selected one along its wall; edit its sill height,
  own width and height, flips and type (with door and window types to duplicate and resize) in
  Properties (`insert.set_size`). Saved as
  `IfcDoor`/`IfcWindow` filling an `IfcOpeningElement` that voids the wall (`door.place`,
  `window.place`, `insert.flip_hand`, `insert.flip_facing`, `insert.set_sill_height`,
  `element.move`).
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
- **Roofs**: Architecture › Roof sketches a footprint in a plan. Pick Walls takes the walls'
  exterior faces (Tab takes the whole chain) with a 600 mm overhang and puts the roof on the
  walls' tops. Line and Rectangle work too. Defines Slope switches curves between sloping and
  gable. A weighted straight skeleton makes hip, gable, shed, pavilion and cut-off (mansard)
  roofs from any simple footprint, including different slopes per edge, and each face gets a
  mitred slab of the roof type's thickness. Properties choose the Roof Shape (Hip, Gable,
  Gable rotated, Shed, Shed reversed or Flat), the overhang all round, the base offset, slope,
  cutoff level and type, and Edit Footprint reopens the sketch. A selected
  roof, floor or ceiling shows its outline with grips: drag an edge or corner to change the
  footprint in place. In the sketch, Modify selects curves to drag, delete or edit (length,
  slope, overhang). Saved as `IfcRoof` with a faceted B-rep body
  (`roof.create_footprint`, `roof.edit_footprint`, `roof.set_params`, `roof.set_edge`,
  `roof.set_slope`, `roof.set_shape`). Walls attach their tops to a roof (offered when a roof is made from picked
  walls, or Modify | Walls › Attach Top/Base, then click the roof), so gable walls rise to the
  ridge. Modify | Roofs › Attach Walls attaches the perimeter walls under a roof at once (interior walls
  stay as they are), and a new roof
  offers it however it was drawn (`roof.attach_walls`). Saved as a faceted B-rep in the wall's own frame, so doors and windows stay cut
  (`wall.attach`, `wall.detach`). Not yet: attaching bases or to floors and ceilings, slope
  arrows, and openings in roofs. Footprints may have courtyards: a building around a central
  courtyard gets a roof that falls to the street and to the courtyard (or a flat slab with the
  courtyard as a hole).
- **Eaves**: Modify | Roofs › Eaves makes a roof's sloped edges eaves that bear on the walls
  below (`roof.set_eaves`, `roof.eaves`). The overhang is measured from the wall's exterior
  face. The bearing is the exterior face, the core centre or the interior face, and a heel height
  raises the roof. The rafter cut is plumb, square or two-cut. A fascia and a soffit (boxed,
  sloped or open) return to the wall's face, mitred at hips, and the perimeter walls are
  attached. A roof made from picked walls gets eaves by default. Not yet: picking single edges
  in the window, and cladding stopping at the soffit. Gutters and downspouts are planned as part
  of plumbing (MEP-PIPE-010), not built yet.
- **Railings**: Architecture › Railing sketches a guard railing's path in a plan (sketched over
  a deck, floor or flat roof, it stands on its top), and Modify |
  Floors / Roofs › Guard Edges puts railings along every open edge of a deck, patio, floor or
  flat roof (edges with a wall along them are left out). Each has a top and a bottom rail, posts
  at the ends and corners, and balusters spaced by the type. Railings on edges follow the host's
  outline (`railing.create`, `railing.create_on_edges`). Modify | Railings › Opening, Gate and
  Double Gate add features to a laid-out railing: click on it where each goes. An opening gets a
  post each side; a gate fills it with one or two leaves and is its own `IfcDoor` (GATE) element
  (`railing.add_opening`, `railing.add_gate`, `railing.move_feature`, `railing.remove_feature`).
  Stairs get railings on both sides, following the nosing line and flat across landings
  (round the outside of a turn); Modify | Stairs › Railings puts them back
  (`railing.create_on_stair`). Not yet: continuous handrails, baluster families, and gate swing
  arcs in plan.
- **Decks**: Architecture › Deck sketches a deck's outline. Its boards are laid out one by one:
  courses across from the start edge (the last ripped, or balanced), every end joint on a joist
  and staggered, picture-frame borders mitred at the corners, and pieces cut from the shortest
  stock length that fits, with offcuts reused. The framing is laid out too: joists, beams, posts
  and footings. Properties change the board width, gap, direction, stock lengths, frame, stagger
  and spacings and show the cut list (boards, stock to buy, framing, screws). A deck is a floor,
  so its outline grips, Edit Boundary and Guard Edges railings work on it (`deck.create`,
  `deck.set_boards`, `deck.set_framing`, `deck.boards`, `deck.cut_list`). Not yet: a ledger,
  span tables, breakers and board overrides.
- **Dormers**: with a roof selected, Modify | Roofs › Dormer places gable, hipped, shed (flat
  lift) or flat dormers where you click on the roof (in a plan or in 3D). Each comes with its
  roof, a front wall and cheek walls that fill the gaps down to the main roof, and runs up the
  slope until it meets the main roof. A dormer that would run off its roof face is refused.
  Properties change the style, width, front height, slope and overhang, and the dormer rebuilds;
  deleting it (or its roof) removes its parts (`dormer.create`, `dormer.set`, `dormer.move`).
  The main roof is cut open below each dormer, inside its walls, and the opening follows the
  walls. Not yet: the opening as a separate IFC opening element.
- **Stairs**: Architecture › Stair (ST) draws a cast-in-place stair in a plan: click the start
  and end of each run while the status bar counts the risers down; the next run starts after an
  automatic landing (straight, L and U arrangements). The riser count is the fewest that keep
  each riser within the type's maximum (180 mm by default; 3 m makes 17 risers), and runs hold
  what their length allows at the tread depth. Properties set the desired risers, tread depth and
  width and show the actual riser height and warnings. Saved as `IfcStair` (`stair.create`,
  `stair.set_params`). Architecture › Spiral Stair makes a full-step spiral from its centre and a
  point on its walking line (a run with a `center` in `stair.create`). Not yet: winders,
  railings, supports, and editing runs with handles.
- **Rooms**: Architecture › Room places a room in the region of wall faces under the cursor,
  shown as you hover. Place Rooms Automatically fills every enclosed region of the plan's level,
  numbered in reading order. Rooms measure to the walls' finish faces and leave out islands such
  as free-standing walls. A door does not join the rooms on its two sides. A room follows its
  walls, and becomes Not Enclosed if they open up. Plans label each room with its name, number
  and area (in PDF too); click a label to select the room. Saved as `IfcSpace` (Name = Number,
  LongName = Name) under its storey, with a body up to its limit (`room.place`,
  `room.place_automatically`, `room.set`). Not yet: room separation lines, room tags as
  elements, other area rules, and volumes clipped by floors and roofs.
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

### Views and documentation

- **Views as model objects**: 3D views are saved in the IFC file (RXD), with a Project Browser
  to open, rename, duplicate and delete them; each view's camera and Home come back on reopen.
  `view.*` commands (create, list, rename, orient, set_camera, open, …) work from MCP too.
- **Floor plans**: View › New Floor Plan creates a plan per level, drawn as a top-down cut at
  the view range's cut plane with cut poché and lines on white; the cut plane is editable in
  Properties, and the Wall tool draws on the plan's level (`view.create_plan`,
  `view.set_view_range`, `view.get_view_range`).
- **Sections and elevations**: View › Section draws a section line in a plan with two clicks
  (the view looks to the line's right). Building Elevations makes North, South, East and West
  elevations around the model. These views draw like plans of the model turned into their frame:
  cut walls and floors as poché, everything up to the far clip in projection (door panels
  included), level lines with names and elevations, and grid lines with their bubbles. Plans show
  section lines and elevation markers. Properties set the far clip and flip the view; the views
  export to PDF (`view.create_section`, `view.create_elevation`, `section.edit`). Not yet:
  crop regions, jogged sections, multi-direction and interior elevation markers, and opening a
  view from its marker.
- **Schedules**: View › Schedules makes a schedule of doors, windows, rooms, walls, floors,
  ceilings or roofs that opens as a grid in its own tab and stays up to date with the model.
  It has one row per element (or, with Itemize off, one per group of identical sorted values,
  with its count), natural and unit-aware sorting, and grand totals that add up areas, volumes,
  lengths and counts. Editing a cell writes the parameter (undoable); computed cells are grey.
  Selecting a row selects its elements. The Schedule tab chooses fields (computed ones such as
  Level, Type, Area and Width, and every parameter of the category) and sorting, and exports
  CSV or tab-separated text with formulas escaped (`schedule.create`, `schedule.get_table`,
  `schedule.set_fields`, `schedule.set_sort`, `schedule.set_cell`, `schedule.export`). Not yet:
  filters, group headers and footers, and calculated fields.
- **Sheets**: View › Sheet makes an ISO A0–A4 sheet, numbered A101, A102, … with Reviera's title
  block. The title block shows the project name and number, the sheet's name, number, date and
  people, and the scale (*As indicated* when scales differ). Place View puts a plan, section or
  elevation on the sheet at its scale, with a numbered view title; a view can be on one sheet
  only. Place Schedule adds a schedule's live table. Viewports and tables are dragged to move
  them and removed with Delete; Properties edit the title-block values. The Project Browser lists
  the sheets with their views, and File › Export PDF prints a sheet on a page of its size
  (`sheet.create`, `sheet.set_parameter`, `sheet.place_view`, `sheet.place_schedule`,
  `viewport.move`, `viewport.remove`, `sheet.delete`). Not yet: title block families, crop
  regions, revisions, sheet collections, guide grids and drag and drop from the browser.
- **COBie handover**: File › Export COBie writes the model's facility-management data as a COBie
  2.4 workbook (XLSX, one sheet per table) or as one CSV file per table. Contact comes from the
  project's COBie settings. Facility, Floor and Space come from the building, levels and rooms.
  Type and Component come from the doors, windows, furniture and equipment, each component placed
  in its room. Attribute carries every parameter. The other sheets are written with their
  headers. Every record carries its IFC class and GlobalId; values are in metres; missing required
  values are "n/a", never invented. Validation reports missing contacts, repeated names (or makes
  them unique on request) and components with no space or type; formulas are kept as text
  (`export.cobie`, `cobie.validate`, `cobie.set_settings`). Not yet: the current NBIMS-US table
  layout, systems and zones from IFC groups, and a settings dialog.
- **Tags and text**: Annotate › Tag by Category (TG) tags the door, window, room, wall or other
  element clicked, with values read live from it: a door's or window's Mark, a room's name,
  number and area, a wall's type. Room Tag (RT) tags rooms, and Tag All Not Tagged tags whole
  categories on the plan's level without duplicates. Tags follow their elements; a tag whose
  element is deleted stays, in orange. Editing a tag's value in Properties changes the element's
  parameter, and Properties also set the tag type, leader and orientation. Text (TX) places a
  note (2.5, 3.5 or 5 mm, optionally wrapped, rotated, and with leaders). Tags and notes are
  selected, dragged and deleted like other elements, and print to PDF (`tag.create`,
  `tag.create_room`, `tag.tag_all`, `tag.set_type`, `tag.set_leader`, `tag.set_default_type`,
  `tag.list`, `text.create`, `text.set_content`, `text.set`, `text.add_leader`,
  `text.remove_leaders`, `text.list`). Not yet: tags and text in sections and on sheets, rich
  text, spell check, tag families and material tags.
- **Dimensions**: Annotate › Aligned Dimension (DI) in plans dimensions parallel wall faces,
  centrelines and cut edges; dimensions to Reviera walls follow them when they move
  (`dim.create_aligned`, `anno.list`). The Wall tool snaps to cut edges in plans.
- **Line weights**: plans draw cut and projection lines with each category's pens
  (`objectstyle.set`) at paper widths for the view scale (`lineweight.set`, `add_scale`),
  so a plan on screen at 100 % matches its print.
- **PDF**: File › Export PDF writes plans, ceiling plans, sections and elevations as vector PDF
  at their scale on the smallest ISO sheet (A4–A0), with pen widths in paper millimetres,
  dimension values, room labels and level names as real text, and a title strip. Sheets print
  on a page of their own size (`export.pdf`).

### Inspecting and editing

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
- **Editing**: the Modify tools Align (AL), Offset (OF), Mirror by picked or drawn axis (MM,
  DM), Move (MV), Copy (CO), Rotate (RO), Trim/Extend to Corner (TR), Trim/Extend to a
  boundary, Split (SL), Pin (PN) and Unpin (UP), in plans, with previews, snapping, and typed
  distances and angles. Selected walls and grids show end grips to drag, and can be dragged
  to move; doors and windows slide along their wall. Walls carry their doors and windows when
  they move, are copied or are mirrored, and reshaping a wall leaves its doors in place
  (`element.move`, `element.copy`, `element.rotate`, `element.mirror`, `element.offset`,
  `element.split`, `element.trim_corner`, `element.trim_extend`, `element.align`,
  `element.pin`, `element.unpin`).

### Interface

- **Tabbed views**: several models open side by side, each view in its own tab with a `wgpu`
  3D viewport (Window › New 3D View, Tile Views, Tab Views, Ctrl+Tab). Tab Views remembers a tiled
  arrangement, and Tile Views brings it back with its dividers where they were. Dockable
  Properties, Project Browser and Model Tree panes.
- **Ribbon and menus**: a tool ribbon with Architecture, Annotate, View and Modify tabs (large
  and small icon buttons in titled panels, the Select panel first), a Quick Access Toolbar
  above it, and a contextual *Modify | <category>* tab while elements are selected (for
  example Flip Hand and Flip Facing for doors). The ribbon, toolbar and menus are built from
  one catalog, `ribbon-default.toml`, checked against the command registry.
- **Navigation**: an Orientation Cube in each 3D view (26 click regions, compass, Home, and a
  PERSP/ISO projection toggle), standard and isometric views with animated transitions, wheel
  zoom about the cursor, orbit about the selection, Previous/Next pan/zoom, and walk mode
  (View › Walk: WASD or arrow keys, drag to look). Click to select; drag to orbit; middle or right
  button to pan.

## Planned next

The first milestone continues with roof openings (skylights and shafts), and layered wall types. Then come curved forms: smooth arched roofs (including vaults that taper over trapezoidal
footprints, ARC-ROOF-008), dome roofs (ARC-ROOF-009), spline-curved walls (ARC-WALL-002), and
boolean intersect, subtract and union between elements (ARC-MOD-028). After that come general
shape and CSG modelling tools,
CADCraft drawing exchange, and the current NBIMS-US COBie layout.

### Working with CADCraft

Reviera is meant to work alongside [CADCraft](https://github.com/storytold/cadcraft), an
open-source 2D drafting application in Rust. The goal is a two-way exchange through DWG and
DXF files: drawings made in CADCraft (site plans, surveys, details, consultant drawings) come
into Reviera as imported or linked CAD, and Reviera's plans and views go back to CADCraft as
DXF or DWG. Both applications read these files with the same library (`acadrust`), so a
drawing is read the same way in each. A corpus of CADCraft-made drawings will check
compatibility continuously, in both directions. The requirements are DM-IMP-013 and DM-IMP-014
in the specification (Chapter 2, §2.11.10–2.11.11). CAD import is not built yet.

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
| `crates/reviera-app` | Application session: documents, file, view-state, inspection, output and recovery commands |
| `crates/reviera-mcp` | MCP server |
| `crates/reviera-render` | Render Scene, plan and section graphics, annotation, stroke font |
| `crates/reviera-render-wgpu` | The built-in `wgpu` viewport |
| `crates/reviera-ui` | The main window (wxWidgets through wxDragon): ribbon, views, tools, panes |
| `crates/…` | Geometry, Godot, Lua, network, BIMserver and OpenCDE crates (placeholders) |
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
