# NoDesk Reviera

**Open-source BIM authoring on native IFC: build anywhere, even from the beach.**

![A laptop showing a building model on a beach table, next to a "Beach Life" mug](design_documents/beach_life.jpg)

Reviera is a parametric Building Information Modeling (BIM) application for architecture,
structure and MEP. IFC is its native file format, so there is no proprietary container. Every
operation can be driven by Lua scripts and by AI agents through the Model Context Protocol (MCP).

> **Status: early development.** The specification is complete as a draft. The application
> already models a small building end to end: straight and curved walls, doors and windows,
> floors, ceilings, decks, hip, gable, arched and domed roofs with eaves and dormers, stairs,
> railings with gates, rooms, levels and grids. It finds and resolves overlaps with kept
> booleans (OpenCASCADE), checks door clearances, and draws plans, ceiling plans, sections,
> elevations, schedules and sheets to PDF and COBie. Every operation is a command that the
> ribbon, Lua scripts and AI agents (MCP) share. Layered walls, columns, openings, shafts and
> arrays are the newest; the first pre-release packages are being prepared.

## Contents

- [Screenshots](#screenshots)
- [What works today](#what-works-today): [platform](#platform-and-file-format),
  [automation](#commands-scripting-and-ai-agents), [modelling](#modelling),
  [coordination](#overlaps-booleans-and-clearances), [documentation](#views-and-documentation),
  [inspecting and editing](#inspecting-and-editing), [the window](#the-window)
- [Sample models](#sample-models)
- [Not built yet](#not-built-yet)
- [Building](#building), [Trying it](#trying-it), [Repository layout](#repository-layout)

## Screenshots

The KIT FZK *AC20-Institute-Var-2* sample model (`test-data/ifc/`) open in Reviera, October 2026.

![A door selected in the 3D view, with the contextual Modify | Doors tab](design_documents/application_screenshots/3D-view.png)

![The Erdgeschoss floor plan: cut walls, stairs and furniture](design_documents/application_screenshots/plan-view.png)

![A section box cutting the Institute model open, in isometric projection](design_documents/application_screenshots/section-box.png)

The Manor sample (`test-data/samples/manor.ifc`), drawn see-through in a Hide/Isolate view, with
its roof selected and the Modify | Roofs tab open: the dormer, the spiral stairs inside the tower,
the porch columns and the shaped windows show through the walls.

![The Manor sample see-through in 3D, its roof selected](design_documents/application_screenshots/manor-transparency.png)

## What works today

Command names are given in `code` after each feature; every one of them can be run from the
ribbon's tools, the command shell, Lua and MCP alike.

### Platform and file format

- **Native IFC** through IfcOpenShell and OpenCASCADE: open, edit, save and tessellate IFC2X3,
  IFC4 and IFC4X3_ADD2 models, with lossless round trips of files from other tools. Elements
  Reviera does not model itself (furniture, imported stairs, equipment) are kept, drawn and
  selectable.
- **Reviera elements are standard IFC.** Walls, slabs, roofs, stairs, railings, doors, windows,
  spaces, storeys and grids are written as ordinary typed IFC entities with their relationships
  (containment, voids and fills, path connections, interferences), so other tools see them.
  Reviera's own data (recipes, sketches, views, settings) is embedded as standard `Reviera_*`
  property sets (RXD), and Reviera reopens its elements as editable.
- **Element identity** (GlobalIds and stable internal IDs) survives save and reopen.
- **Transactions, undo and redo** through one command registry: every change is a transaction,
  undo groups bundle multi-step tools and scripts, and the history lists who did what (user,
  script or agent) (`edit.undo`, `edit.redo`, `edit.history`, `element.changed_since`).
- **Crash recovery.** Every change is mirrored to an encrypted SQLite live store (SQLCipher,
  with the key kept in the OS keyring). After a crash, unsaved work and its undo history come
  back; the window offers to restore it (`recovery.list`, `recovery.restore`,
  `recovery.show_changes`, `recovery.discard`, `live.*`).
- **Units**: SI storage with project display units (metric and imperial, feet-and-inches);
  every length field accepts typed units (`4.5 m`, `12' 6"`) (`units.get`, `units.set_preset`,
  `units.set_format`, `units.set_number_format`, `units.format`, `units.parse`).
- **Help** (Help › Reviera Help, F1): a help tab beside the views with getting started,
  navigation and selection basics, and the sample models, each opened (as a fresh copy) from its
  link. Help › About gives the version and licence.
- **Several models at once**: each opens in its own tabs; File › Close Model (Ctrl+Shift+W)
  closes the active one and its views, asking to save changes.
- **Model Properties** (Manage › Settings, UN; File menu): the model-wide settings in one dialog:
  units preset and number format, the **building code** (IBC, IRC, Approved Document K, NBC
  Canada, NCC Australia, DIN 18065 or custom) with its occupancy and the model's own stair
  headroom, and the accessibility rule set. Tools follow the model's code by default and may use
  another for one operation (`code.get`, `code.set`; DM-CODE-001).
- **Parameters**: built-in, project and shared parameters, bound to types or instances and
  written to IFC property sets; Properties edit them, with type parameters marked
  (`param.list`, `param.get`, `param.set`, `param.set_many`, `param.clear`, `param.definition`,
  `param.project.add/modify/remove`, `param.set_ifc_home`, `pset.set`, `pset.remove`).
- **Types**: wall, door, window, floor, ceiling, roof, stair, railing and grid types, duplicated,
  renamed and edited the same way, from Properties or commands (`type.list`, `type.duplicate`,
  `type.rename`, `type.set_param`, `element.set_type`).

### Commands, scripting and AI agents

- **One command registry**: 207 commands in 46 domains today, each with a JSON Schema, a
  summary and description, a permission level (`read_only`, `model_edit`, `full_scripting`)
  and attribution. `registry.list` and `registry.describe` list and explain them; the command
  shell (`reviera shell`) runs them with JSON arguments.
- **Lua scripting** (LuaJIT): `script.run` runs a script or a `.lua` file against the open model.
  `reviera.<domain>.<verb>{...}` (or `reviera.call`) runs any command, `reviera.ui.form{...}` asks
  the user for input (length, number, integer, checkbox, choice and text fields, with defaults,
  ranges and help, validated and asked again until valid), and `print` reports. A run is one undo
  step and rolls back entirely if it fails.
- **Plugins**: a folder whose `plugin.lua` returns `{title, description, run}`, or a single
  `.lua` file with a `-- title:` header. They are found in the built-in set, the user's library
  (`~/.config/reviera/plugins`) and `REVIERA_PLUGINS` folders (`plugin.list`, `plugin.run`,
  `script.import`, `script.remove`). The built-in **Castle Turret** plugin builds a round tower
  (walls, a cone, pointed, onion or hemispherical dome or battlements, and arrow slits) from a
  form; it is the scripting capabilities test.
- **Scripts in the window**: the **Scripts** menu has Run Script…, Import into Library… (a
  script or a plugin folder), Manage Library… (each known script with its description, to run,
  remove or import) and every known script. A script's form shows as a dialog in the model's
  units. Headless runs (shell, MCP, tests) answer forms from `answers` and the defaults.
- **MCP server** for AI agents (`reviera --mcp-stdio`): every command is a tool, grouped into
  toolsets (core, modeling, documentation, families, materials, project_setup, io,
  collaboration, analysis, scripting, admin) that agents enable as needed, with discovery
  tools, `query_elements` and `get_element`, undo/redo/history, undo groups, preview and
  confirm for destructive changes, and resources for documents and elements. Agents get the
  same transactions, warnings and permission checks as the user.

### Modelling

#### Walls

- **Drawing**: Architecture › Wall (WA) draws chains of walls on a plan's level, snapping to wall
  ends, midpoints, wall lines, cut edges and 15° angles; type a length and press Enter, and
  Spacebar flips the exterior side. **Arc Wall** draws a wall through its start, end and a point
  on the arc; **Spline Wall** draws one smoothly through clicked points (Enter ends it). Walls
  appear at once, regenerated from their recipe (`wall.create`, `wall.set_location`,
  `wall.flip`).
- **Properties**: a selected wall's base level and offset, top (a level plus a top offset, or an
  unconnected height) and type (choose, Duplicate Type…, rename, width). With the Wall tool
  running, the type row picks the type new walls get (`wall.set_constraints`).
- **Joins**: walls that meet join at mitred L corners and T junctions, recomputed as they move;
  curved walls join straight ones with mitred corners. A join can be disallowed per end
  (`wall.set_join_allowed`, `wall.joins_query`).
- **Attach**: a wall's top attaches to a roof or dome above it, so gable walls rise to the ridge
  and a drum rises to its dome; the part above the roof's lowest point follows the roof
  (`wall.attach`, `wall.detach`; Modify | Walls › Attach Top/Base).
- **Curved walls** carry doors and windows along the curve (square to it), bound rooms, attach to
  roofs and save as faceted B-reps.
- **Layers** (Modify | Walls › Edit Assembly): a wall type's layers, exterior first, each with a
  function, priority, material, thickness and wrapping, and the core; validated as the
  specification asks. The type's width is their total, plans draw the layer boundaries in cut
  walls, and IFC gets an `IfcMaterialLayerSet` with an `IfcMaterialLayerSetUsage` per wall
  (`compound.get`, `compound.set_layers`; ARC-CMPD-001/002).
- **IFC**: `IfcWall` with an `Axis` and a `SweptSolid` body (curved and attached walls as faceted
  B-reps in the wall's own frame), typed, contained in its storey and connected to its
  neighbours with `IfcRelConnectsPathElements`.

#### Doors and windows

- Architecture › Door (DR) and Window (WN) place them in walls (straight or curved) in plans or
  3D. The preview follows the cursor along the wall, the cursor's side sets the facing and
  Spacebar flips a door's hand.
- **Fitting**: a new one fits the wall where it lands. It slides or shrinks into the free gap,
  and on a dormer it rises above the main roof and shrinks under the dormer roof. The preview
  shows the fitted size.
- **Editing**: drag a selected one along its wall, or type its distance from either end of the
  wall. Properties edit the sill height (the head height follows), its own width and height (or
  the type's), the hand and facing, and the type (door and window types to duplicate and resize).
- **Shapes** (ARC-WIN-002): a type's Shape in Properties makes it round, elliptical, round-top
  (semicircular, segmental, elliptical, Gothic or Tudor heads), a fan light, gable-headed, a
  trapezoid or a polygon, with its own values (rise, radius ratio, sides, rotation, side
  heights) and radiating or upright glazing bars. Doors take round-top heads. The wall's opening,
  the frame (at a constant width along arcs) and the glass follow the outline; plans cut it at
  its true width at the cut height; fitting keeps a round window round and a semicircular or
  Gothic head's shape.
- **Door operations** (ARC-DOOR-002): a door type opens as a single or double swing, a
  revolving door (its width the drum) or an automatic bi-parting sliding door, with solid or
  glass storefront leaves and an automatic operator. A sliding door takes the wall its leaves
  slide over (fitting, refusals and a *slide space* virtual volume); plans draw each kind's
  symbol; IFC writes the operation type and `HasDrive`.
- Walls are cut, plans show door swings, and each new door or window gets the next Mark.
- **IFC**: `IfcDoor`/`IfcWindow` filling an `IfcOpeningElement` that voids the wall (`door.place`,
  `window.place`, `insert.set_position`, `insert.set_sill_height`, `insert.set_size`,
  `insert.flip_hand`, `insert.flip_facing`). A shaped one's opening is its outline extruded
  through the wall (an `IfcCircleProfileDef` for a round window), and a shaped window says
  `PartitioningType = USERDEFINED` with its shape.

#### Sketch mode

Floors, ceilings, decks and roof footprints are sketched in a plan on a contextual *Modify |
Create … Boundary* tab:

- **Pick Walls** takes the wall face on the cursor's side, locked so the element follows the
  wall. Tab takes the whole chain of walls joined or meeting end to end. A curved wall gives its
  face's curve.
- **Line**, **Rectangle**, **Arc** (start, end, a point on it) and **Circle** (centre and a
  point) draw free curves, snapping like the drawing tools. Sketch curves may pass through
  points (`through`: one point makes an arc, more a spline).
- **Modify** selects curves to drag, delete or edit (length; for roofs slope, overhang and
  Defines Slope). Delete removes the selected or the last curve.
- **Finish** checks the sketch: open ends, branches, crossings and overlaps are marked in red
  and the session stays open. Inner loops make holes. **Cancel** discards it.
- A finished floor, ceiling, deck or roof shows its outline with grips when selected. Dragging an
  edge or a corner changes it in place, as one undoable edit, and Edit Boundary / Edit Footprint
  reopens the sketch (`sketch.move_curve`, `sketch.move_corner`).

#### Floors and ceilings

- **Floors**: Architecture › Floor (FL) sketches a floor. Floors have types with a thickness and a
  height offset, and show area, perimeter and volume in Properties. Saved as `IfcSlab` with an
  extruded profile (`floor.create`, `floor.edit_boundary`, `floor.set_level`).
- **Ceilings**: Architecture › Ceiling places an automatic ceiling in the room under the cursor
  (its edges follow the walls' finish faces, round rooms included, and move with them), or
  Sketch Ceiling. Ceilings have types and sit 2600 mm above their level by default. A ceiling
  can follow the roof over it (Shape: Follow Roof, in the Ceiling tool or Properties): vaulted
  under a pitched roof, curved under an arched roof or a dome. Saved as `IfcCovering` (CEILING)
  (`ceiling.create_auto`, `ceiling.create`, `ceiling.edit_boundary`, `ceiling.set_shape`).
- **Curtain walls** (Architecture › Curtain Wall): two clicks draw one; its type lays out the
  vertical and horizontal grid lines (Fixed Distance, Fixed Number, Maximum or Minimum Spacing,
  with the instance's justification and offset), mullions and glazed, solid or empty panels.
  Drawn along a wall, it cuts its own opening. IFC: `IfcCurtainWall` aggregating `IfcPlate`
  panels and `IfcMember` mullions (`curtain.create`, `curtain.set`; ARC-CW-001 … 006,
  ARC-WALL-013).
- **Columns** (Architecture › Column): architectural columns of rectangular or round types,
  placed by clicking in a plan (Spacebar turns them 90°), from a base level to a top level or a
  height. Attach Top (Modify | Columns) attaches their tops to the roof, floor or ceiling above,
  cut by a sloped underside. They move, rotate, mirror and copy, and are written as `IfcColumn`
  (`column.place_architectural`, `column.set`, `column.attach`, `column.detach`; ARC-COL-001).
- **Openings** (Architecture › Opening):
  - **Wall Opening**: hover a wall and click to cut a rectangular hole of the tool's width and
    height through a straight or curved wall. Its base and top follow levels; it slides along the
    wall and moves, copies and goes with it (`opening.create_wall`, `opening.edit_wall`;
    ARC-WALL-012, ARC-OPEN-003).
  - **Vertical**: sketch loops in a plan; Finish cuts the floor (or ceiling) on the level they
    overlap most, else a roof, plumb; one partly outside notches the edge. Plans draw it crossed
    corner to corner; its grips reshape it; it moves with its host and goes with it
    (`opening.create_vertical`, `opening.set_loops`; ARC-OPEN-002).
  - **Shaft**: sketch loops; the shaft cuts every floor, ceiling and roof between its base and top
    levels, including ones added later (`opening.create_shaft`, `opening.edit_shaft`;
    ARC-OPEN-004).
  - IFC: an `IfcOpeningElement` voids each host, whose own body stays whole; a shaft is an
    `IfcBuildingElementProxy` (PROVISIONFORVOID) with one derived opening per host.
- **Skylights** (Architecture › Opening › Skylight): click a roof in a plan or 3D. A pitched
  roof gets a roof window lying in the slope; a flat roof a flat sheet, a pyramid or a ridge
  lantern on a curb. A ridge lantern's panes are re-mapped from its length (as many equal panes
  as keep each within the largest pane size). Each cuts its own opening and moves with its roof.
  IFC: `IfcWindow` (SKYLIGHT) filling an `IfcOpeningElement` in the roof (`skylight.create`,
  `skylight.set`; ARC-WIN-001).

#### Roofs

- **Footprint roofs**: Architecture › Roof sketches a footprint. Pick Walls takes the walls'
  exterior faces with a 600 mm overhang and puts the roof on the walls' tops, and offers to
  attach the walls. A weighted straight skeleton makes hip, gable, shed, pavilion and cut-off
  (mansard) roofs from any simple footprint, with different slopes per edge and courtyards
  (falling to the street and to the courtyard, or a flat slab with the courtyard as a hole).
  Each face is a mitred slab of the type's thickness. Properties choose the Roof Shape (Hip,
  Gable, Gable rotated, Shed, Shed reversed or Flat), overhang, base offset, slope, cutoff level
  and type (`roof.create_footprint`, `roof.edit_footprint`, `roof.set_params`, `roof.set_edge`,
  `roof.set_slope`, `roof.set_shape`).
- **Attach Walls**: Modify | Roofs › Attach Walls attaches every perimeter wall under a roof at
  once; interior walls stay as they are (`roof.attach_walls`).
- **Eaves**: Modify | Roofs › Eaves makes a roof's sloped edges bear on the walls below. The
  overhang is measured from the wall's exterior face. The bearing is the exterior face, the core
  centre or the interior face, and a heel height raises the roof. The rafter cut is plumb, square
  or two-cut. A fascia and a soffit (boxed, sloped or open) return to the wall's face, mitred at
  hips. A roof made from picked walls gets eaves by default (`roof.set_eaves`, `roof.eaves`).
- **Dormers**: Modify | Roofs › Dormer places gable, hipped, shed (flat lift) or flat dormers
  where you click on a roof, in a plan or 3D. Each comes with its roof, a front wall and cheek
  walls down to the main roof, trimmed to its underside so the cheeks follow the slope. Its roof joins the main roof flush: it reaches back until its top
  meets the main roof's and is cut along it, so the two meet in valleys and rain runs off. The
  main roof is cut open below the dormer, inside its walls. Windows fit the dormer's front wall.
  Properties change the style, width, front height, slope and overhang; deleting the dormer (or
  its roof) removes its parts (`dormer.create`, `dormer.set`, `dormer.move`).
- **Arched roofs**: Architecture › Arched Roof sketches a four-sided footprint and makes a smooth
  barrel or segmental vault springing from the longer sides (semicircle, segment, elliptical and
  catenary profiles). Over a trapezoid it tapers, keeping its rise or its slope at the springing.
  Rise, overhangs and profile are in Properties, and the end walls attach up to the arch
  (`roof.create_arched`, `roof.set_arch`).
- **Domes**: Architecture › Dome places a hemisphere, segmental, pointed or onion dome over a
  circle, an ellipse, or a square, hexagonal or octagonal cloister vault, with an optional
  oculus and faceting. Properties edit the centre, radius (a hemisphere's rise follows), sides,
  rotation, rise, profile ratio and oculus (`roof.create_dome`, `roof.set_dome`).
- **IFC**: `IfcRoof` with faceted B-rep bodies (`BARREL_ROOF` and `DOME_ROOF` for curved roofs).

#### Stairs, railings and decks

- **Stairs**: Architecture › Stair (ST) draws a cast-in-place stair: click the start and end of
  each run while the status bar counts the risers down; the next run starts after an automatic
  landing (straight, L and U arrangements). The riser count is the fewest that keep each riser
  within the type's maximum (180 mm by default), and runs hold what their length allows at the
  tread depth. Properties set the desired risers, tread depth and width and show the actual
  riser height and warnings. **Spiral Stair** makes a full-step spiral from its centre and a
  point on its walking line. New stairs get railings on both sides. Saved as `IfcStair`
  (`stair.create`, `stair.set_params`).
- **Ramps** (Architecture › Ramp): click a centreline in a plan from the bottom up. Corners get
  square landings, and a run longer than the type's maximum incline length (9 m) is split by
  landings. A slope steeper than the type's maximum (1:12) gives a warning. Thick or solid
  shapes; `IfcRamp` (`ramp.create`, `ramp.set`; ARC-RAMP-001).
- **Stairs through floors** (ARC-STAIR-007): a run drawn to within a tread of the top snaps to
  reach the level above and completes the stair; Esc completes it too, the last run carrying on.
  The stair keeps reaching its top as levels move, and its railings extend with it. Where a floor
  or ceiling over it leaves less headroom than the building code asks (from the nosing line), the
  stair offers to open it over its full width, under the model's code or another one (each listed
  with its headroom and clause); Modify | Stairs › Stair Openings offers it again
  (`stair.headroom_check`, `stair.create_openings`).
- **Railings**: Architecture › Railing sketches a guard railing's path (sketched over a deck,
  floor or flat roof, it stands on its top). Modify | Floors / Roofs › Guard Edges puts railings
  along every open edge of a deck, patio, floor or flat roof, leaving out edges with a wall
  along them, and they follow the host's outline. Stair railings follow the nosing line and
  run flat across landings, round the outside of a turn (Modify | Stairs › Railings). Each has a
  top and bottom rail, posts at the ends and corners, and balusters spaced by the type
  (`railing.create`, `railing.create_on_edges`, `railing.create_on_stair`).
- **Openings and gates**: Modify | Railings › Opening, Gate and Double Gate: click on the railing
  where each goes. An opening gets a post each side; a gate fills it with one or two leaves,
  swinging in or out, and is its own `IfcDoor` (GATE) (`railing.add_opening`,
  `railing.add_gate`, `railing.move_feature`, `railing.remove_feature`, `railing.features`).
- **Decks**: Architecture › Deck sketches a deck's outline. Its boards are laid out one by one:
  courses across from the start edge (the last ripped, or balanced), every end joint on a joist
  and staggered, picture-frame borders mitred at the corners, and pieces cut from the shortest
  stock length that fits, with offcuts reused. The framing is laid out too: joists, beams, posts
  and footings. Properties change the board width, gap, direction, stock lengths, frame,
  stagger and spacings and show the cut list (boards, stock to buy, framing, screws). A deck is
  a floor, so outline grips, Edit Boundary and Guard Edges work on it (`deck.create`,
  `deck.set_boards`, `deck.set_framing`, `deck.boards`, `deck.cut_list`).

#### Rooms, levels and grids

- **Rooms**: Architecture › Room (RM) places a room in the region of wall faces under the cursor,
  shown as you hover; Place Rooms Automatically fills every enclosed region of the level,
  numbered in reading order. Rooms measure to the walls' finish faces (curved walls included),
  leave out islands such as free-standing walls, follow their walls, and become Not Enclosed if
  the walls open up. Plans label each room with its name, number and area. Saved as `IfcSpace`
  (Name = Number, LongName = Name) under its storey (`room.place`,
  `room.place_automatically`, `room.set`).
- **Levels**: Architecture › Level (LL) adds a level with its floor plan, named after the level
  below. Name and Elevation are edited in Properties; moving a level moves the walls on it and
  stretches the walls up to it; deleting one lists what goes with it first. Saved as
  `IfcBuildingStorey` (`level.create`, `level.set_elevation`, `level.rename`, `level.list`).
- **Grids**: Architecture › Grid (GR) draws grid lines with snapping; names continue the series
  (1, 2, 3 … or A, B, C). Grids show in every plan and in PDF with their bubbles, are renamed in
  Properties and dimensioned, and dimensions follow a moved grid. Grid types set the bubble, a
  continuous, gapped or custom centre segment, and the pens and patterns. Saved as one `IfcGrid`
  with an `IfcGridAxis` per line; axes changed in another tool are taken over on open
  (`grid.create`, `grid.rename`, `grid.set_curve`, `grid.list`).

### Overlaps, booleans and clearances

- **Volume overlap engine**: View › Check Overlaps finds where elements' bodies overlap (walls,
  floors, roofs, stairs, railings, doors, windows and imported elements), listed largest first
  with volume and depth, and selects and frames a picked pair. Touching is not overlapping, and
  pairs that belong together by design (a door and its wall, a dormer and its roof, a gate and
  its railing) are left out (`overlap.find`).
- **Resolving**: a picked overlap is accepted as intended (it then drops out of the checks),
  moved apart, or resolved by a boolean: cut either by the other (optionally hiding the cutting
  body, as for a slot's cutter), join by precedence (floors and roofs cut walls, walls cut
  columns), intersect or union (`overlap.resolve`, `overlap.accept`, `overlap.accepted`).
- **Kept booleans**: computed by OpenCASCADE and re-made whenever either element changes;
  Modify › Remove Boolean takes them off. Saved as the shaped bodies with an
  `IfcRelInterferesElements` for each (`geometry.boolean`, `geometry.remove_boolean`).
- **Virtual volumes**: every door has its swing volume, and with the ADA 2010 rule set (View ›
  Accessibility Rules) its pull-side and push-side maneuvering clearances. Check Overlaps lists
  what intrudes into them (and doors that would hit each other). View › Virtual Volumes shows
  them hidden, translucent or solid in each view; they are never exported as physical
  elements (`clearance.set_rule_set`, `clearance.list`, `clearance.check`,
  `view.set_virtual_volumes`).

### Views and documentation

- **Views as model objects**: plans, sections, elevations, 3D views, schedules and sheets are
  saved in the IFC file. The Project Browser opens, renames, duplicates and deletes them, and
  each 3D view's camera, Home, section box and slices come back on reopen (`view.create_3d`,
  `view.list`, `view.open`, `view.rename`, `view.duplicate`, `view.delete`, `view.set_camera`,
  `view.get_camera`, `view.set_home`, `view.orient`, `view.set_projection`).
- **Floor and ceiling plans**: View › Floor Plan creates a plan per level, drawn as a top-down cut
  at the view range's cut plane, with cut poché, projection lines, door swings, room labels,
  grids, section and elevation markers. Ceiling Plan makes reflected ceiling plans (cut at
  2300 mm, looking up). Curved walls, arches and domes are cut true to their shape. The cut plane
  and view range are edited in Properties (`view.create_plan`, `view.set_view_range`,
  `view.get_view_range`).
- **Sections and elevations**: View › Section draws a section line with two clicks; Building
  Elevations makes North, South, East and West elevations. They draw cut walls and floors as
  poché, everything up to the far clip in projection, level lines and grid bubbles. Properties
  set the far clip and flip the view (`view.create_section`, `view.create_elevation`,
  `section.edit`).
- **Schedules**: View › Schedules makes a live schedule of doors, windows, rooms, walls, floors,
  ceilings or roofs in its own tab: one row per element or per group of identical values (with
  counts), unit-aware sorting, and totals of areas, volumes, lengths and counts. Editing a cell
  writes the parameter (undoable); selecting a row selects its elements. The Schedule tab
  chooses fields and sorting and exports CSV or tab-separated text with formulas escaped
  (`schedule.create`, `schedule.fields`, `schedule.get_table`, `schedule.set_fields`,
  `schedule.set_sort`, `schedule.set_cell`, `schedule.export`).
- **Sheets**: View › Sheet makes an ISO A0–A4 sheet, numbered A101, A102, …, with Reviera's
  title block (project, sheet, date, people and scale, *As indicated* when scales differ).
  Place View puts a plan, section or elevation on it at its scale with a numbered title; Place
  Schedule adds a schedule's live table; viewports are dragged and deleted (`sheet.create`,
  `sheet.set_parameter`, `sheet.place_view`, `sheet.place_schedule`, `sheet.delete`,
  `viewport.move`, `viewport.remove`).
- **Tags and text**: Annotate › Tag by Category (TG) tags doors, windows, rooms, walls and other
  elements with values read live from them; Room Tag (RT) tags rooms; Tag All Not Tagged tags
  whole categories without duplicates. Editing a tag's value changes the element; tags follow
  their elements and stay, in orange, when it is deleted. Text (TX) places notes (2.5, 3.5 or
  5 mm, wrapped, rotated, with leaders) (`tag.*`, `text.*`).
- **Dimensions**: Annotate › Aligned Dimension (DI) dimensions parallel wall faces, centrelines,
  grids and cut edges; dimensions to Reviera walls and grids follow them (`dim.create_aligned`,
  `anno.list`).
- **Line weights**: plans draw cut and projection lines with each category's pens at paper widths
  for the view scale, so a plan on screen at 100 % matches its print (`objectstyle.set`,
  `lineweight.set`, `lineweight.add_scale`, `lineweight.remove_scale`).
- **PDF**: File › Export PDF writes plans, ceiling plans, sections, elevations and sheets as
  vector PDF at their scale, with pen widths in paper millimetres and real text (`export.pdf`).
- **COBie handover**: File › Export COBie writes a COBie 2.4 workbook (XLSX, or one CSV per
  table): Contact, Facility, Floor, Space, Type, Component (each in its room) and Attribute
  (every parameter), every record with its IFC class and GlobalId; missing required values are
  "n/a", never invented. Validation reports missing contacts, repeated names (or makes them
  unique) and components with no space or type (`export.cobie`, `cobie.validate`,
  `cobie.set_settings`).

### Inspecting and editing

- **Section box and slices**: View › Section Box clips a 3D view to a box whose face grips drag in
  real time; Selection Box (BX) fits it around the selection. Horizontal and Vertical Slice cut
  the view at a clicked point (up to eight planes, each with a grip). Cut solids are capped.
  They are saved with the view and map exactly to BCF clipping planes (`view.set_section_box`,
  `view.fit_section_box`, `view.set_slice_planes`).
- **Hide and isolate**: Hide, Isolate, Isolate with ghosting and Translucent work on elements,
  on elements with their children, by category or by type (HH, HI, HC, IC, HG, HT; HR resets).
  This is temporary per-view state that is never saved or printed (`view.temp_*`).
- **Model Tree** (MT): Project › Site › Building › storeys › categories › elements, with hosted
  elements under their hosts; its context menu selects, hides, isolates or boxes whole branches
  (`model.tree`).
- **Selection**: click to select in a view; Ctrl+click adds and Shift+click removes. A box selects
  several: left to right a window (entirely inside), right to left a crossing (inside or
  touching); in plans a left drag boxes, in 3D views Ctrl or Shift + left drag (Ctrl adds the
  boxed set, Shift removes it). The Model Tree, schedules and Check Overlaps also select several
  at once. The selection drives Properties and the contextual tab (`selection.get`, `selection.set`, `selection.add`, `selection.remove`,
  `selection.clear`, `element.get_ids`, `element.find_by_guid`).
- **Quick modify menu**: a middle click on an element opens Move, Copy, Rotate, Mirror, Flip,
  Hide, Make Translucent/Opaque, Isolate, Reset Hide/Isolate, Section Box, Pin/Unpin and Delete
  at the cursor; a middle click on empty space offers the reset when anything is hidden. With two elements selected it
  checks whether they overlap and, when they do, offers the booleans: cut either way, intersect,
  union, join or accept (UI-SEL-006).
- **Measure** (Modify › Measure): click points in a plan; the status bar shows each segment, its
  angle and the chain's total (`measure.between`, `measure.chain`, `measure.along`; ARC-MOD-023).
- **Modify tools**: Align (AL), Offset (OF), Mirror by picked or drawn axis (MM, DM), Move (MV),
  Copy (CO), Rotate (RO), Array (AR) and Radial Array (members as independent copies; Move To
  2nd or Last; ARC-MOD-026), Trim/Extend to Corner (TR), Trim/Extend to a boundary, Split (SL),
  Pin (PN), Unpin (UP) and Delete (DE), with previews, snapping and typed distances and angles.
  Selected walls and grids show end grips to drag and can be dragged to move; walls carry their
  doors and windows when they move, are copied or mirrored (`element.move`, `element.copy`,
  `element.rotate`, `element.mirror`, `element.offset`, `element.split`, `element.trim_corner`,
  `element.trim_extend`, `element.align`, `element.pin`, `element.unpin`, `element.delete`,
  `element.array_linear`, `element.array_radial`). A translucent element is picked like any
  other; only hidden elements (and ghosted ones while isolating) let clicks through.

### The window

- **Tabbed views**: several models open side by side, each view in its own tab with a `wgpu`
  viewport (Window › New 3D View, Tile Views, Tab Views, Ctrl+Tab; Tile Views remembers its
  arrangement). Each tab has a close button; drag a tab to reorder it, onto another group's tabs
  to move it there, or to an edge of the view area to tile it. The help tab opens at start. Dockable Properties, Project Browser and Model Tree panes.
- **Ribbon**: Architecture (Wall, Arc Wall, Spline Wall, Curtain Wall, Door, Window, Floor,
  Ceiling, Roof,
  Arched Roof, Dome, Column, Room, Stair, Spiral Stair, Railing, Deck, Wall Opening, Vertical,
  Shaft,
  Level, Grid), Annotate
  (dimensions, text, tags), View (views, sheets, navigation, coordination, section box and
  slices, hide/isolate, windows, panes) and Modify tabs, with a Quick Access Toolbar above.
  While elements are selected a contextual *Modify | <category>* tab adds their tools (for
  example Flip Hand for doors, Eaves and Dormer for roofs, Gate for railings). The ribbon,
  toolbar and menus come from one catalog, `ribbon-default.toml`, checked against the command
  registry.
- **Menus**: File, Edit, View, Architecture, Annotate, Modify, Scripts and Window.
- **Keyboard shortcuts**: two-letter sequences typed in a view, shown in tooltips and menus:
  WA wall, DR door, WN window, FL floor, RM room, ST stair, LL level, GR grid, TX text, TG and RT
  tags, DI dimension, MD modify, DE delete, PP properties, MT model tree, ZF/ZP/ZN zoom, PI
  projection, WK walk, SX/BX section and selection box, plus the Modify and hide/isolate keys
  above.
- **Properties palette**: the selected element's identity, its own rows (wall constraints and
  type, insert placement and size, roof shape and eaves, arch and dome, stair, deck, dormer,
  room, tag and note values) and every parameter, grouped; type rows choose, duplicate and edit
  types. With a tool running and nothing selected it shows the tool's type. With nothing
  selected it shows the active view (name, view range, projection, section box).
- **Navigation**: an Orientation Cube in each 3D view (26 click regions, compass, Home, and a
  perspective/isometric toggle), standard views with animated transitions, wheel zoom about the
  cursor, Previous/Next pan/zoom, and walk mode (WASD or arrow keys, drag to look). Dragging
  orbits about the selection's centre, or about the point of the model under the cursor; the
  middle or right button pans, and Shift + middle drag orbits.
- **Platform**: Linux (X11, and Wayland through a native subsurface) and Windows 10/11 (also
  under Wine, where Help falls back to wxWidgets' HTML window and the view draws with OpenGL);
  wxWidgets and `wgpu` keep macOS within reach.

## Sample models

`test-data/samples` holds Reviera's own sample models. Each is rebuilt by a Lua script and
checked by a test (no overlaps, no clearance intrusions, saves and reopens), and each has a
walkthrough (`house.md`, `observatory.md`, `csg.md`, `manor.md`, `storefront.md`) giving the manual steps to build it in the
window, which confirms that every operation it uses has its UI:

- **Storefront doors** (`storefront.ifc`): a shop front with a revolving door, an automatic
  sliding door, a pair of glass swing doors and an automatic storefront door, under ADA 2010.
- **Manor with tower** (`manor.ifc`), the demonstration model: a two-storey house with an arched
  door, round-top, Gothic and oculus windows, rooms, ceilings, a hipped roof with eaves and a
  dormer, and a straight stair that completes itself and opens the ceiling and floor over it
  for the IRC's headroom; a round three-storey tower with two spiral stairs opening the floors
  above them, under an onion dome; and a deck with a gated railing.
- **House with dormers** (`house.ifc`): eaves, flush-joined dormers, L and U stairs with
  railings, a gated front railing and a deck with its railing.
- **Observatory** (`observatory.ifc`): a round drum of arc walls under a dome, a vaulted annex, a
  spline garden wall and a turret built by the Castle Turret plugin; the dome's observing slot is
  cut by a boolean, and the ADA 2010 rules are on.
- **Overlap resolutions** (`csg.ifc`): one overlap resolved every way, side by side (cut either
  way, join, intersect, union, move apart and accept) next to an unresolved pair.

Open one with `reviera gui test-data/samples/manor.ifc`, or rebuild them with
`REVIERA_REGEN_SAMPLES=1 cargo test -p reviera-app --test samples`.

## Not built yet

The specification (`design_documents/spec/`) describes the whole application; these are the
nearest gaps in what is built:

- **Walls**: location lines other than the centreline, layer cleanup at joins and wrapping,
  layered floors and roofs, a material library, T joins into the side of a curved wall,
  attaching bases or to floors and ceilings, and true curved IFC geometry (curved walls, arches
  and domes are faceted B-reps).
- **Windows and doors**: quarter-round fan lights, arcs as true arc segments in IFC (shaped
  outlines are written as polylines), the arch above the cut dashed in plans, and built-in
  shaped types ready in the type list (today a type is duplicated and its Shape set); gate
  swing arcs in plan.
- **Roofs**: skylight openings square to the roof face, flashing, and skylight types; slope arrows, picking single eave edges in the window,
  dormer openings as separate IFC opening elements. Gutters and downspouts are planned as part
  of plumbing (MEP-PIPE-010).
- **Openings**: openings by face (square to a sloped roof), across roof ridges and in arched
  roofs and domes, and guards around a stair opening (a Pick Edges railing can follow it).
- **Stairs, ramps and railings**: winders, supports, editing runs with handles, continuous handrails
  and baluster families; curved ramps, ramp railings and editing a ramp's path with grips.
- **Decks**: ledgers, span tables, breakers and board overrides.
- **Rooms**: room separation lines, tags as elements, other area rules, volumes clipped by floors
  and roofs.
- **Coordination**: clash tests (COL-CLASH), automatic pipe and duct penetrations (MEP-PEN-001),
  bathroom fixture clear floor spaces and turning spaces, ISO 21542.
- **Documentation**: crop regions, jogged sections, interior elevation markers, opening a view
  from its marker; schedule filters, grouping and calculated fields; title block families,
  revisions and sheet collections; tags and text in sections and on sheets; the current NBIMS-US
  COBie layout and a COBie settings dialog.
- **Custom objects from meshes** (DM-IMP-015): importing OBJ, STL, PLY and glTF/GLB (3MF and OFF
  later) as reusable object types with a category, material slots and plan outlines, placed many
  times, reloaded from the file, and saved to an object library. Specified; not built yet.
- **Stamps** (DM-LIB-002): any library object, family type or saved group picked up as a stamp
  from the Library panel's Stamps strip (or Alt+click on a placed copy), inserting a copy at each
  click in a plan or on a face in 3D; all copies share one type (Make Unique detaches one); rows
  and random scatter later. Specified; not built yet.
- **Interface**: general shape tools; a macOS build (the script is written, not yet run on a
  Mac); a Windows GUI that opens without a console window beside it.
- **MCP**: streamable HTTP with tokens, rate and size limits, subscriptions, prompts,
  `execute_lua` and view images.
- **Structure, MEP, families and collaboration** (BIMserver, OpenCDE, Godot) are specified but
  not started.

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

Linux (Fedora) is the development platform today; Windows builds are cross-compiled from it, and
macOS is a target.

```sh
# Toolchain and libraries (Fedora)
sudo dnf install rustup cmake ninja-build gcc-c++ opencascade-devel boost-devel eigen3-devel \
    libxml2-devel json-devel perl-FindBin perl-IPC-Cmd perl-Time-Piece webkit2gtk4.1-devel
rustup-init

# The first build also downloads and compiles wxWidgets 3.3.3 (through wxDragon).
# IfcOpenShell 0.8.5, built into third_party/ (about 15 minutes)
tools/build-ifcopenshell.sh

cargo build
cargo test --workspace
```

**Packages.** `packaging/` builds the release packages: a Flatpak (built and tested on Fedora
43), a Windows installer with a portable ZIP (cross-built on Fedora; tested on Windows 11 and
under Wine), and a macOS `.app` in a `.dmg` (written, not yet run on a Mac). See [packaging/README.md](packaging/README.md).

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
| `crates/reviera-core` | Element store, transactions, undo, command registry, and the modelling: walls, inserts, sketches, floors, roofs, stairs, railings, decks, rooms, overlaps, clearances |
| `crates/reviera-ifc` | IfcOpenShell bridge (C++ shim through `cxx`) |
| `crates/reviera-rxd` | Reviera Extension Data encoding |
| `crates/reviera-store` | Local `.ifc` projects with embedded RXD |
| `crates/reviera-livestore` | Encrypted live store and crash recovery |
| `crates/reviera-app` | Application session: documents, file, view-state, inspection, output and recovery commands |
| `crates/reviera-mcp` | MCP server |
| `crates/reviera-render` | Render Scene, plan and section graphics, annotation, stroke font |
| `crates/reviera-render-wgpu` | The built-in `wgpu` viewport |
| `crates/reviera-ui` | The main window (wxWidgets through wxDragon): ribbon, views, tools, panes |
| `crates/reviera-lua` | The LuaJIT scripting host: the `reviera` API, forms, plugin discovery, the built-in plugins |
| `crates/reviera-geom` | OpenCASCADE solid kernel (`cxx`): booleans and their cache |
| `crates/reviera-godot`, `reviera-net`, `reviera-bimserver`, `reviera-opencde` | Placeholders for the Godot viewer, networking, BIMserver and OpenCDE |
| `design_documents/` | The specification (`spec/`), technology stack, research log and source register |
| `schemas/`, `clients/rsp/` | Open data formats and protocol clients (MIT) |
| `test-data/ifc/` | Third-party IFC models for tests (see their README for provenance) |
| `test-data/samples/` | Reviera's own sample models, their Lua scripts and walkthroughs |

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
