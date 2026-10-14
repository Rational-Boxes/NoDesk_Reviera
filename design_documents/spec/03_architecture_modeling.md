# Chapter 3 — Architectural Modeling and Editing Tools

**Area code:** ARC · **Primary source:** `research/02_architecture_modeling.md` · **Target architecture:** `TECH_STACK.md`
**Functional reference:** Revit 2026 behaviour, with the 2027 changes applied where they differ (most importantly, there is no Options Bar).

This chapter specifies how Reviera creates and edits the building model. It covers datums, walls, inserts, curtain systems, slabs, roofs, circulation, openings, rooms and areas, free-form and site elements, the general modify tools, groups, constraints and selection. It also defines the geometry algorithms that turn parametric elements into OpenCASCADE (OCCT) solids: wall joins and layer cleanup, layer wrapping, roof generation, stair and railing generation, shape editing, boolean cuts and room boundary computation. Those algorithms are written at implementation depth.

---

## 3.0 Conventions used in this chapter

### 3.0.1 Units, tolerances and coordinates
- All lengths in the core are stored in **metres** as `f64`; angles are stored in **radians**. Display units come from the project settings (DM chapter). Values quoted here in mm or feet are display equivalents.
- **Model tolerances**, used by every algorithm in this chapter unless one says otherwise:

| Name | Value | Use |
|---|---|---|
| `TOL_POINT` | 0.05 mm | Two points are coincident |
| `TOL_ANGLE` | 1e-6 rad | Two directions are parallel |
| `TOL_JOIN` | 1 mm + half the other wall's width | Searching for wall-end join candidates |
| `TOL_ROOM_SNAP` | 0.5 mm | Vertex merging in the room-boundary arrangement |
| `TOL_SHORT` | 0.8 mm | Shortest curve allowed in a sketch or location line ("element too short" error) |
| `OCCT_FUZZY` | 1e-5 m | Fuzzy value passed to OCCT Boolean operations (`BOPAlgo_Options::SetFuzzyValue`) |

- The model uses a right-handed coordinate system: +Z is up, project north is +Y in the project coordinate system, and true north is applied through the site's rotation (DM chapter).
- **"Left of the drawing direction".** For a curve traced from start to end and viewed from above, *left* is the side reached by rotating the tangent by +90° about +Z.

### 3.0.2 Requirement blocks
Feature blocks follow the template in `00_conventions.md` §5. Each **Commands** line names registry commands. All of them are undoable unless they are marked *(query)*. They are exposed automatically to the UI, to Lua (`reviera.<domain>.<verb>(...)`) and to MCP. The following conventions apply to every command signature in this chapter:
- `curve` is a 2D or 3D curve value: `Line{p0,p1}`, `Arc{center,radius,a0,a1,normal}`, `Arc3{p0,pm,p1}`, `Ellipse{center,rx,ry,rot,a0,a1}`, `Spline{ctrl,knots,degree}` or `CurveLoop[...]`.
- `loops` is a list of closed `CurveLoop`s. The first loop is the outer loop and the rest are holes. Commands may accept several outer loops when the feature allows it.
- `level_ref` is the element ID of a level. `host_ref` is an element ID, optionally followed by a face reference (`elem#face:<stable-face-key>`).
- `?` marks an optional parameter. Every create command returns the new element's `element_id`, and those that create more than one element return `[element_id]`.

### 3.0.3 Regeneration model (summary)
The Rust core keeps a **regeneration graph**. Its nodes are elements and its directed edges mean "the output of A is an input of B". Every feature block lists the edges it adds. The detailed ordering and parallelism rules are in §3.15. The edge kinds are:

| Edge kind | Meaning |
|---|---|
| `datum` | The element reads the elevation, plane or curve of a level, grid, reference plane or work plane |
| `join` | Two elements share a join node, and both read the resolved join topology |
| `attach` | The element's extent is clipped by another element's geometry |
| `host` | A hosted element reads its host's placement, and the host reads the insert's cutting geometry |
| `cut` | A Boolean subtraction of one element's tool solid from another element's body |
| `bound` | A room, area or space reads bounding geometry |
| `type` | An instance reads its type's parameters (compound structure, layout rules) |
| `group` | A group instance reads its group type definition |
| `constraint` | A constraint node reads and writes references of several elements |
| `sketch-lock` | A sketch line follows a picked edge of another element |

### 3.0.4 IFC conventions
- The default schema is **IFC4 ADD2 TC1**. Where IFC4X3 offers a better entity or enumeration, the IFC4X3 mapping is given as well, marked **(4X3)**.
- Every physical element is contained in its storey through `IfcRelContainedInSpatialStructure`. Elements on a reference (non-story) level are contained in the nearest story level at or below that level, and the true reference level is recorded in RXD element data (`placement`).
- Every type is linked to its occurrences through `IfcRelDefinesByType`, and materials are linked through `IfcRelAssociatesMaterial`.
- **IFC keeps** element geometry and representations, types, material layer sets, openings and voids (`IfcRelVoidsElement` / `IfcRelFillsElement`), `IfcRelSpaceBoundary`, standard `Pset_*` / `Qto_*` sets, and the values of user-visible parameters (property sets defined by the DM chapter).
- **Reviera Extension Data (RXD)** keeps everything else. The carrier, encoding, geometry hash and reconciliation rules are defined in `spec/02a_extension_data_format.md`; this chapter only says *which* RXD object holds each piece of data, using two forms:
  - **"RXD element data (`<kind>`)"** is data owned by one element (or one type): its recipe (`recipe.wall`, `recipe.roof`, `recipe.stair`, `recipe.railing`, `recipe.curtain_grid`, `recipe.toposolid`, …), its `sketch`, and the constraints and join overrides it owns. For types this is the type-level RXD data.
  - **"RXD document `<name>`"** is project-level data. This chapter uses `model/constraints` (constraints between elements, attach relations, Join Geometry and Cut Geometry pairs), `model/groups`, `model/rooms` (room and area computation settings, area schemes and rules, unplaced rooms), `model/selection_sets`, `model/parts` (part divisions and merges), `project/settings` (site settings), `families/<id>` (family recipes, including in-place families and masses) and the views documents (colour schemes; DOC chapter).

| Data written by this chapter | Where it goes |
|---|---|
| Sketch curves, per-curve properties, slope arrows, span direction, pick-and-lock targets, shape-edit modifiers of floors and roofs | RXD element data (`sketch`) on the host |
| Wall location-line choice and flip, per-end allow/disallow join, per-end wrap override, wall profile, hosted-wall relation and offset | RXD element data (`recipe.wall`) |
| Join type, join order and display at a join node; attach top/base relations; Join Geometry / Cut Geometry / Join Roof pairs; alignment locks, locked dimensions, EQ; *Moves With Nearby/Grids* links | RXD document `model/constraints` |
| Compound-structure flags (wraps, variable, structural, exact priority), core boundaries, vertical regions, type sweeps and reveals, stacked-wall rows, curtain layout rules, railing rail and baluster tables, stair run and landing types | RXD type data (`recipe.compound`, `recipe.stacked_wall`, `recipe.curtain_type`, `recipe.railing_type`, `recipe.stair_type`) |
| Datum extents (3D and per-view 2D), bubbles, elbows, end locks, scope box assignment, grid-axis identity | RXD element data (`recipe.datum`) on the datum or `IfcGrid` |
| Stair runs, landings, supports, multistory connections | RXD element data (`recipe.stair`) |
| Curtain grid and mullion instance layout | RXD element data (`recipe.curtain_grid`) |
| Toposolid points, split lines, sub-divisions, excavation and grading links, contour settings | RXD element data (`recipe.toposolid`) |
| Work-plane host and reference-level hosting of any element | RXD element data (`placement`) |
| Group types, exclusions, array associations | RXD document `model/groups` |
| Room/area settings, area schemes and rules, unplaced rooms | RXD document `model/rooms` |
| Colour schemes | RXD views documents (DOC chapter) |
| Saved selection sets | RXD document `model/selection_sets` |
| Part identity, parent, exclusion, overrides and face offsets | RXD element data (`recipe.part`) on each part |
| Part divisions (curves, references, gap, profile) and merges | RXD document `model/parts` |
| Site settings (contour defaults, section graphics, property-line angle format) and toposolid smooth shading | RXD document `project/settings` (section `site`) |
| Flip state (facing, hand, mirrored) of family instances | RXD element data (`placement`) |

All references are IFC GlobalIds, in the reference forms defined in `02a_extension_data_format.md` §6.2. Sub-element references use named sub-references, such as `#end:1`, `#face:core.ext` or `#axis:<uuid>`, never indices.
- **Edits by other tools.** When the geometry hash in an element's RXD data no longer matches its IFC representation, or the element has no RXD data, Reviera treats the element as described in `02a_extension_data_format.md` §7: it is either stale (the user chooses between keeping the external geometry and regenerating) or non-parametric.
- In the IFC files Reviera writes, every element has an `Axis` or `FootPrint` representation where one makes sense, plus a `Body` representation. The `Body` uses the simplest valid form in this order of preference: `IfcExtrudedAreaSolid`, then `IfcBooleanClippingResult`, then `IfcFacetedBrep` / `IfcAdvancedBrep`, then `IfcPolygonalFaceSet`. On load, the parametric definition in RXD takes precedence (unless it is stale) over the explicit body. The body is a cache that lets other tools read the file.

---

## 3.1 Shared interaction model

The behaviours in this section are implemented once, in the tool framework, and are reused by every tool in this chapter.

### 3.1.1 Tool framework and contextual tabs
**ID:** ARC-TOOL-001 · **Priority:** P0 · **Ref:** research/02 §1, §2.1, §2.4, §17.1 · screenshots/architecture/modify_walls_contextual_ribbon.png

**Purpose.** Every creation or edit tool runs on one shared state machine and presents its options in one shared, consistent way.

**Workflow.**
1. The user starts a tool from the ribbon, a shortcut, the Lua/MCP command, *Create Similar*, or *Repeat last command*.
2. The tool enters **Pick-type**. The Type Selector defaults to the last type used with this tool in the session.
3. The tool enters **Options**. The contextual tab `Modify | Place <Element>` opens, and the Properties palette shows the *New <Element>* instance defaults.
4. The tool enters **Input**. Clicks, typed values and picks are collected, and a live preview is drawn.
5. The tool enters **Commit**. One transaction is created, the regeneration graph runs, and the tool returns to **Input** so that the user can place again.
6. **Esc** once cancels the current input chain. **Esc** a second time, or pressing *Modify*, ends the tool.

**UI.**
- The contextual tab always contains the *Select*, *Properties*, *Clipboard*, *Geometry*, *Modify*, *View*, *Measure* and *Create* panels. It then adds the **Draw** gallery and any tool-specific panels (*Placement*, *Tag*, *Constraint*, *Mode*).
- The Draw gallery offers: Line, Rectangle, Inscribed Polygon, Circumscribed Polygon, Circle, Start-End-Radius Arc, Center-Ends Arc, Tangent End Arc, Fillet Arc, Spline, Ellipse, Partial Ellipse, Pick Lines, and Pick Faces / Pick Walls. A tool enables only the entries it supports.
- Selecting existing elements shows `Modify | <Category>` with *Mode* (Edit Sketch, Edit Profile, Edit Type, as applicable) and element-specific panels.
- **There is no Options Bar.** Every tool option is either a control on the contextual tab (toggle, numeric field or drop-down) or a field in the Properties palette's *New <Element>* block.

**Behaviour & rules.**
- Tool options are **remembered per tool for the session**. The user can choose to persist them per user in the user settings file (`reviera-user.toml`; UI chapter).
- Every tool can be invoked in two orders: *select, then tool* (the tool acts on the current selection at once) and *tool, then select*. In the second order the user confirms with **Enter** or **Finish**.
- Each commit is a single undo step. A multi-step tool, such as sketch mode or the group editor, uses a **transaction group** that collapses into one undo step on Finish and is rolled back completely on Cancel.
- A tool descriptor declares its panels, options, input phases and command bindings. The UI tab is built from that descriptor, so tools added from Lua get the same UI treatment.

**Data (IFC mapping).** None. Session state only.

**Commands.** `tool.start(command_id, type_id?, preset_args?) -> session_id`, `tool.cancel()`, `tool.finish() -> ()`, `tool.set_option(tool_id? (default: active tool), key, value) -> ()`, `tool.repeat_last() -> session_id`. Placement itself always goes through the element-specific create commands, so scripts and agents never need the interactive tool.

**Acceptance criteria.**
- [ ] Every placement tool in this chapter opens a contextual tab, and no control is shown outside the tab or the Properties palette.
- [ ] Placing three walls in a row and pressing Ctrl+Z three times removes them one at a time.
- [ ] Cancelling a sketch session leaves the model byte-identical to its state before the session (comparing the GlobalId set and the attribute hashes).
- [ ] A tool option changed in one invocation keeps its value in the next invocation in the same session.

### 3.1.2 Type and instance properties
**ID:** ARC-TOOL-002 · **Priority:** P0 · **Ref:** research/02 §2.2

**Purpose.** Every element exposes **type** parameters, which are shared by all instances of the type, and **instance** parameters, which belong to one element.

**Workflow.** The Type Selector at the top of the Properties palette changes the type of the selected elements, or the type that a running tool will place. **Edit Type** opens the Type Properties dialog, which offers Duplicate, Rename, Load (for loadable families) and a preview pane.

**Behaviour & rules.**
- **System types** (walls, floors, roofs, ceilings, stairs, railings, ramps, curtain walls and systems, toposolids, levels, grids) are defined in the project.
- **Loadable types** (doors, windows, components, columns, profiles, balusters, panels, mullion profiles) come from Reviera family files (DM chapter).
- Changing the type of a placed element keeps the element's reference geometry: the location line for walls, the boundary for slabs and the insertion point for families.
- A type change that cannot be applied, such as a door type that is wider than its host segment, is rejected with a warning that names the elements involved.

**Data (IFC mapping).** `IfcTypeObject` subtypes linked through `IfcRelDefinesByType`. Type parameters are held in property sets on the type, and instance parameters in property sets on the occurrence. Details are in the DM chapter.

**Commands.** `element.set_type(element_ids, type_id)`, `type.duplicate(type_id, new_name) -> type_id`, `type.rename(type_id, name) -> ()`, `type.set_param(type_id, param, value) -> ()`, `element.set_param(element_ids, name, value)`.

**Acceptance criteria.**
- [ ] Changing a wall's type from 200 mm to 300 mm with Location Line = Finish Face: Exterior leaves the exterior face where it was.
- [ ] Editing a type parameter regenerates every instance of that type in a single transaction.

### 3.1.3 Sketch mode
**ID:** ARC-SKCH-001 · **Priority:** P0 · **Ref:** research/02 §2.3, §17.2 · screenshots/architecture/roof_footprint_sketch_mode.png

**Purpose.** A modal editing session for elements whose shape comes from user-drawn curves rather than from a single click.

**Elements that use sketch mode:** floors, sketched ceilings, footprint and extrusion roofs, soffits, openings (shaft, vertical, by face, dormer), railing paths, stair and ramp sketches (boundary, riser, path), split face, toposolid boundaries and sub-divisions, wall elevation profiles, property lines, multi-segment grids, and area or room-separation boundaries when they are edited as a set.

**Workflow.**
1. The user starts a sketch-based tool, or chooses *Edit Sketch* / *Edit Boundary* / *Edit Profile* on an existing element.
2. The view enters sketch mode. All non-sketch elements are drawn in the half-tone "edit mode" colour, which the user can configure, and they still snap. Sketch curves are drawn in the sketch colour (magenta by default).
3. The contextual tab shows the **Draw** gallery, the sketch-element pickers relevant to the host (Boundary Line, Slope Arrow, Span Direction, Riser, Stair Path, Symbolic Line and so on) and a **Mode** panel with **Finish** (✓) and **Cancel** (✗).
4. The user draws, picks or edits curves. Selecting a sketch curve shows its per-curve properties in the Properties palette.
5. **Finish** validates the sketch (ARC-SKCH-002). If the sketch is valid, the element is regenerated and the session closes. If it is invalid, a dialog lists the problems and highlights the offending curves, and the session stays open.

**UI.** The draw options *Chain*, *Offset*, *Radius* (fillet corners while chaining) and *Lock* (for Pick tools) appear as contextual-tab controls. Right-click › *Snap Overrides › Close* closes an open chain, and Tab cycles between the candidate closing points.

**Behaviour & rules.**
- A sketch is a **child model** owned by the host element: sketch curves, slope arrows, span direction symbols, riser and boundary curves and the stair path. It is stored with the host and keeps stable IDs across edits, so that dimensions and locks can refer to sketch curves.
- **Pick-and-lock.** A curve created with Pick Walls, Pick Lines or Pick Roof Edges while *Lock* is on gets a `sketch-lock` dependency on the picked edge or face. When the source moves, the sketch curve is recomputed and the host regenerates. If the source is deleted, the lock is dropped silently and the curve keeps its last position.
- Pick Walls uses the wall's finish face on the side the cursor is on, unless *Extend into wall (to core)* is on, in which case it uses the core face. The *Offset* value is measured from that face.
- **Tab** while hovering a pick candidate selects the connected chain of the same kind, for example all walls joined end to end.
- **Curved curves.** The Draw gallery has Arc (start, end, a point on it) and Circle (centre, a point on it; two half arcs). A sketch curve may pass through points between its ends, as a curved wall does (one point: a circular arc; more: a spline); it is validated and gives profiles as its sampled polyline. Pick Walls on a curved wall takes the face's curve, locked: it follows the face's current curve. In curve arguments: `{line, through?, lock?}`. *Built: floors, ceilings and decks; roof footprints stay straight (arched roofs and domes make curved roofs).*
- The sketch session uses a transaction group (ARC-TOOL-001), so Cancel discards every change.

**Data (IFC mapping).** The sketch is stored as RXD element data (`sketch`) on the host. Each curve has a stable key, its geometry, its role (boundary, slope arrow, riser and so on), its per-curve properties and an optional lock target (GlobalId plus face key). The resulting geometry is exported normally: for example, a floor boundary becomes the `IfcArbitraryProfileDefWithVoids` of its extrusion.

**Commands.** `sketch.begin(host_id, role?) -> session_id`, `sketch.add_curve(session_id, curve, role?, props?) -> curve_key`, `sketch.pick(session_id, ref, offset?, lock?, chain?) -> [curve_key]`, `sketch.edit_curve(session_id, curve_key, curve?, props?)`, `sketch.delete_curve(session_id, curve_key)`, `sketch.finish(session_id) -> element_id`, `sketch.cancel(session_id)`. Every create command that takes `loops` creates the sketch implicitly, so a non-interactive caller never needs a session.

**Acceptance criteria.**
- [ ] A floor sketched with Pick Walls and Lock on follows the wall when the wall is moved 500 mm, and its area updates.
- [ ] Cancelling an Edit Boundary session restores the previous boundary exactly.
- [ ] Finish on an open loop leaves the session open and highlights the open ends.

**Implementation notes (2026-10-13).**
- As built, sketch sessions run in the window (floors, sketched ceilings, footprint roofs), not as `sketch.*` session commands. Callers without a window send whole curve sets to the create and edit commands.
- **Modify** in the Draw panel, the default when editing a finished element:
  - Selects a curve. Delete removes it; with nothing selected, Delete removes the last curve.
  - Dragging a curve moves it across its own direction. Each neighbour keeps its direction and is stretched or shortened to meet it.
  - Dragging a corner moves it, and every curve ending there follows.
  - A moved curve leaves the wall it was locked to; neighbours keep their locks, since they stay on their lines.
- **Curve Properties:** Length (editable: the end moves along the curve) and whether the curve follows a wall. Roofs add Defines Roof Slope, Slope and Overhang.
- Editing sessions start from the element's current footprint: the corners where its curves' current (possibly wall-locked) lines meet.
- **Footprint grips on the model:**
  - A selected roof, floor or ceiling in a plan shows its footprint outline, with grips at the corners and edge middles.
  - Dragging a grip moves the edge or corner by the same rules, without opening the sketch, as one undoable `roof.edit_footprint` or `floor.edit_boundary`.

### 3.1.4 Sketch validation
**ID:** ARC-SKCH-002 · **Priority:** P0 · **Ref:** research/02 §2.3

**Purpose.** A deterministic way to decide whether a sketch can produce an element, with messages that tell the user what to fix.

**Behaviour & rules.** The validator runs these checks in order on the curves with the "boundary" role:
1. **Short curves.** Any curve shorter than `TOL_SHORT` is an error.
2. **Near-axis curves.** A line within 0.2° of the X or Y axis of the view or work plane, but not exactly on it, raises a *warning* ("Line is slightly off axis"). This is not an error.
3. **Overlaps.** Two curves that share a positive-length piece are an error ("Curves overlap").
4. **Loop building.** Endpoints are merged within `TOL_POINT`, and a graph is built in which curves are edges. Every vertex must have degree exactly 2. A vertex of degree 1 means an open loop, and a vertex of degree 3 or more means a branch. Both are errors ("Curves must form closed loops").
5. **Self-intersection.** No two curves may intersect except at a shared endpoint. Reviera uses exact line/arc intersection; ellipses and splines are subdivided adaptively to `TOL_POINT`. An intersection is an error ("Curves intersect").
6. **Nesting.** The loops are classified by containment. At even depth (0, 2, …) a loop is an outer boundary, and at odd depth it is a hole. An element type that allows only one outer loop (for example a roof by footprint) reports "Only one outer boundary is allowed" when it finds more.
7. **Role-specific rules.**
   - An extrusion roof needs exactly one **open** chain.
   - Stair and ramp sketches need exactly two non-touching boundary chains and at least one riser.
   - A railing path needs exactly one connected chain, open or closed.
   - A slope arrow's tail must lie on a boundary curve.
   - There may be at most one span direction symbol.

Loop orientation is normalised on Finish: outer loops counter-clockwise and holes clockwise, seen from the sketch-plane normal.

**Acceptance criteria.**
- [ ] A square with one side drawn 0.01 mm short reports an open loop at exactly that vertex.
- [ ] A loop that has a hole loop inside a second hole loop produces an "island" outer region, so a floor with a nested island has two solid regions.
- [ ] A figure-eight sketch is rejected with "Curves intersect".

### 3.1.5 Per-curve sketch properties and slope arrows
**ID:** ARC-SKCH-003 · **Priority:** P0 · **Ref:** research/02 §2.3, §7.1, §7.3, §7.4

**Purpose.** Sketch curves carry parameters that shape the 3D result, such as slopes, overhangs and offsets.

**Behaviour & rules.**
- **Floor and ceiling boundary curves:** *Defines Slope* (bool), *Slope* (rise/run or angle) and *Offset from Base* (m). At most one curve may define slope. Two parallel curves whose *Offset from Base* values differ also define the slab plane.
- **Roof footprint curves:** *Defines Roof Slope* (bool), *Slope*, *Overhang* (m, positive outward), *Plate Offset from Base* (m), *Offset from Roof Base* (m) and *Extend into wall (to core)* (bool).
- **Slope arrow** (a two-point sketch element whose tail lies on a boundary curve):
  - *Specify* is either `HeightAtTail` or `Slope`.
  - With `HeightAtTail`, the arrow has *Level at Tail*, *Height Offset at Tail*, *Level at Head* and *Height Offset at Head*.
  - With `Slope`, the arrow has *Level at Tail*, *Height Offset at Tail* and *Slope*.
  - The arrow defines a plane. The plane contains the tail point at the tail height, rises along the arrow's direction, and its contour lines (lines of equal height) run at right angles to the arrow.
  - A boundary curve that carries a slope arrow cannot also be slope-defining. In roofs, slope arrows are used for dormer cheeks, four-sided gables and similar cases.
- **Span direction** (structural decks; STR chapter) is a symbol curve that records an orientation and takes no other part in geometry.
- **Validation.** Contradictory definitions, for example a slope arrow and a slope-defining curve that cover the same slab, are an error on Finish.

**Data (IFC mapping).** These properties are stored per curve in RXD element data (`sketch`). The resulting tilt and faces are written as explicit geometry.

**Acceptance criteria.**
- [ ] A floor with a single slope-defining curve at 2% produces a top face that rises 2 cm per metre measured at right angles to that curve.
- [ ] A slope arrow from tail height 0 to head height 1.0 m over a 4 m arrow length gives a plane with a slope of 25%.

### 3.1.6 Temporary dimensions
**ID:** ARC-DIM-001 · **Priority:** P0 · **Ref:** research/02 §2.4, §15.1

**Purpose.** On-the-fly dimensions that show where an element is and let the user drive its position by typing a value.

**Workflow.**
1. While an element is being placed, or when exactly one element is selected, Reviera shows temporary dimensions to the **nearest parallel references** on each principal side.
2. The user clicks the dimension text, types a value and presses Enter. The **selected** element moves, and the reference at the other end stays where it is.
3. The user can drag a witness-line grip to another reference, or click the grip repeatedly to cycle through the candidate references on the same element (for walls: face, centreline, core face, core centre).
4. The *Make permanent* icon below a temporary dimension turns it into a permanent dimension (DOC chapter).

**UI.** Text size and background (opaque or transparent) are set in Options › Graphics. The default references are set in Manage › Additional Settings › **Temporary Dimensions**: walls measure to Centerlines, Center of Core, Faces or Faces of Core, and doors and windows measure to Centerlines or Openings.

**Behaviour & rules.**
- **Reference search.** The candidates are references (faces, centrelines, datum planes and reference planes) that are parallel to the element's reference within `TOL_ANGLE` and visible in the view. Reviera chooses the closest candidate on each side within 30 m, and falls back to the nearest grid or level.
- For a hosted insert, Reviera measures along the host wall to the nearest perpendicular walls or openings, as set in the Temporary Dimensions settings.
- Editing a temporary dimension runs a move command, so it respects pins, constraints and hosting (§3.12).

**Commands.** `dim.temporary_query(element_id, view_id) -> [TempDim]` *(query)*. Driving a temporary dimension issues `element.move` or `wall.set_location`. `dim.make_permanent(view_id, temporary_dimension_ref | {ref_a, ref_b}) -> dimension_id` creates the permanent dimension.

**Acceptance criteria.**
- [ ] Selecting a door shows dimensions from the door centreline to the nearest perpendicular walls on both sides. Typing a new value moves the door along its host.
- [ ] Clicking a wall-reference grip repeatedly cycles through at least four references and then wraps around.

### 3.1.7 Listening dimensions
**ID:** ARC-DIM-002 · **Priority:** P0 · **Ref:** research/02 §2.4, §11.3 · web: Use Listening Dimensions

**Purpose.** Typed precise input while drawing.

**Workflow.**
1. The user starts a line-based tool and clicks the first point. A *listening dimension* (the live length, radius or angle shown next to the rubber-band preview) follows the cursor.
2. The user types a digit, a sign or a decimal separator. An inline input box opens next to the cursor, pre-filled with the typed character, and the preview freezes its direction.
3. **Enter** commits the value; **Esc** closes the box without committing and leaves the tool waiting for the second point; **Backspace** edits the text.
4. In a chain tool (walls, lines, sketch lines with *Chain*), the committed end point becomes the next start point and the box closes until the next keystroke.

**UI.** The box shows the value in display units with the unit symbol of the project's length or angle format. The listening dimension itself is drawn in the temporary-dimension style (ARC-DIM-001).

**Behaviour & rules.**
- After the first click of a line-based input (lines, arcs, polygons by radius, circle radius, ellipse radii, offsets, move and copy distances), typing a numeric key opens an inline input box next to the cursor.
- **Enter** commits the value along the current cursor direction.
- **Which quantity.** The box drives the primary linear quantity of the step: line length; chord length for the second point of a three-point arc; radius for circles, polygons and centre-end arcs; distance for Move, Copy and Offset. When a step has no linear quantity (for example the sweep of a centre-end arc after its radius is fixed), the box takes an angle in the project's angle format.
- **Direction.** The direction is the cursor direction at the moment the box opened, after snapping and any orthogonal constraint (Shift, or *Constrain* on Move and Copy). Moving the mouse while the box is open does not change it.
- The input accepts the project's display units and these shorthands: imperial `8 6` means 8'-6"; `8'6"`, `2500` (in project units) and expressions such as `1200+600` are also accepted.
- **Grammar.** A value is an arithmetic expression with `+ − × /` (also typed `*`), parentheses and unit-suffixed terms (`mm`, `cm`, `m`, `'`, `"`, fractions such as `3 1/2"`). Unsuffixed numbers use the project's length unit. The same parser is used by every numeric field in the Properties palette (UI chapter).
- **Errors.** An unparsable entry turns the box red and is not committed ("Invalid value"). Zero or negative lengths are rejected ("Value must be greater than 0"). A length shorter than `TOL_SHORT` gives "Element is too short". The box stays open so the user can correct it.
- Listening dimensions are not available for splines or rectangles, because those inputs have more than one dimension.
- **Replay.** The typed value is recorded in the tool's micro-operation log as an explicit point (`{op: "point", xyz, typed: "<text>"}`), so crash recovery (§2.1 of the conventions) replays the exact coordinates rather than re-parsing.

**Commands.** No command of its own. The resolved point is passed to the running tool exactly as a click would be; scripts call the creation command (for example `wall.create`) with explicit coordinates.

**Acceptance criteria.**
- [ ] Wall tool, first click, type `5000`, Enter: the result is a wall exactly 5.000 m long along the cursor direction.
- [ ] In an imperial project, typing `8 6` produces 2.5908 m.
- [ ] Typing `1200+600` in a metric project draws a line 1.800 m long; typing `(2*900)/2` draws 0.900 m.
- [ ] Typing `-300` or `abc` keeps the box open with an error and creates nothing.
- [ ] Moving the mouse after the box has opened does not change the direction of the committed segment.

### 3.1.8 Snapping, alignment lines and constraint keys
**ID:** ARC-SNAP-001 · **Priority:** P0 · **Ref:** research/02 §2.4, §16

**Purpose.** Precise cursor input.

**Behaviour & rules.**
- **Snap kinds:** endpoints, midpoints, nearest, work-plane grid, quadrants, intersections, centres, perpendicular, tangents, points, remote objects and point clouds. Each kind can be switched on or off in Manage › Snaps.
- Length and angle increments depend on zoom level. Each zoom band has a configurable increment.
- **Two-letter one-shot overrides** apply to the next pick only: `SE` endpoint, `SM` midpoint, `SN` nearest, `SI` intersection, `SC` centre, `SP` perpendicular, `ST` tangent, `SQ` quadrant, `SW` work-plane grid, `SX` points, `SR` remote objects, `PC` point cloud, `SO` snaps off and `SS` override off.
- **Alignment lines** are dashed temporary lines drawn through the endpoints of nearby geometry, extended in parallel and perpendicular directions. They snap the cursor when it is within 6 px.
- **Shift** held while drawing constrains input to orthogonal directions in the view or work plane.
- A snap marker shows the kind of snap with a glyph and a tooltip.

**Commands.** `snap.set_options(kinds, increments?)`. Snapping is interactive only. Scripted commands always take exact coordinates.

**Acceptance criteria.**
- [ ] With all snaps switched off except Midpoint, the cursor snaps only to midpoints.
- [ ] `SO` disables snapping for exactly one pick.

### 3.1.9 Tab and Spacebar behaviours
**ID:** ARC-KEY-001 · **Priority:** P0 · **Ref:** research/02 §2.4, §4.2, §5.2, §5.4, §8.2, §15.3

**Purpose.** Two keys with consistent, context-dependent meanings.

**Behaviour & rules.**
- **Tab** cycles the **pre-selection candidate** under the cursor. The candidates come from a ranked list:
  1. Overlapping elements under the cursor, frontmost first.
  2. The connected chain of the hovered element (walls joined end to end, lines in a chain, or slab edges along a perimeter).
  3. Sub-elements of the hovered element: curtain panel, grid or mullion; stacked sub-wall; stair run, landing or support; group member; shape-edit vertex or edge; layer boundary in Edit Assembly.
  4. Alternative references for Align, dimensions and temporary dimensions.

  The status bar names the current candidate. Shift+Tab cycles backwards.
- **Spacebar**, while placing:
  - **walls** (including By Room and By Segment): flips the exterior side about the location line;
  - **doors**: flips the hand;
  - **windows**: no action;
  - **components**: rotates the preview 90° about Z, or aligns it to the nearest wall when one is within snap distance;
  - **stair winders**: rotates the turn direction.
- **Spacebar** with a selection flips walls, cycles doors and windows through their four facing/hand combinations, and rotates free-standing components by 90° (ARC-FLIP-001).
- **Spacebar** in Rotate (§3.12) picks a new centre of rotation.
- Elliptical walls ignore the Spacebar flip.

**Acceptance criteria.**
- [ ] Hovering a wall joined in a closed rectangle and pressing Tab once highlights all four walls, and a click selects them all.
- [ ] Spacebar while drawing a wall from left to right puts the exterior face below the wall line in plan.

### 3.1.10 In-canvas element controls
**ID:** ARC-CTRL-001 · **Priority:** P0 · **Ref:** research/02 §2.5

**Purpose.** Direct manipulation of selected elements in the view.

**Behaviour & rules.** Each selected element shows the controls its kind supports:
- blue endpoint drag dots (curve-driven elements);
- shape handles (triangular arrows that drag a face or an extent);
- flip arrows;
- a pushpin (pinned state);
- padlocks (locked constraints);
- the make-permanent dimension icon;
- an **Allow/Disallow join** glyph at each wall end, in plan and in 3D;
- an **end-wrap toggle** at each wall end, in plan;
- **Configure Grid Layout** (curtain faces);
- a **3D/2D extent** toggle, a bubble-visibility checkbox and an Add Elbow control (datums);
- direct-manipulation arrows (stairs, §3.7).

Dragging a control issues the matching command once, on mouse-up. The preview during the drag uses the same regeneration code path with a scratch transaction.

**Acceptance criteria.**
- [ ] Dragging a wall end dot regenerates its joins live and commits a single undo step.
- [ ] A pinned element shows its pushpin, and its drag controls are disabled.

---

## 3.2 Datums

### 3.2.1 Levels
**ID:** ARC-LVL-001 · **Priority:** P0 · **Ref:** research/02 §3.1 · screenshots/architecture/levels_in_elevation_view.png

**Purpose.** Named horizontal planes that host level-based elements and define stories.

**Workflow.**
1. The user opens a section or elevation view. The tool is unavailable in other views.
2. The user starts Architecture › Datum › **Level** (`LL`). Draw choices are **Line** and **Pick Lines** (with *Offset*).
3. While the cursor moves, level ends align with the ends of existing levels, and a temporary vertical dimension to the nearest level is shown. The user clicks the start and then the end.
4. Reviera names the level by incrementing the trailing number or letter of the previous name: *Level 2* → *Level 3*, *L02* → *L03*, *A* → *B*.
5. Clicking the name label renames the level. Reviera then asks whether to rename the associated views. Clicking the elevation label edits the elevation.

**UI.** Placement-panel controls: **Make Plan View** (on by default) and **Plan View Types…** (Floor Plan, Ceiling Plan, Structural Plan, and so on). Instance and type parameters are listed below.

**Behaviour & rules.**
- A **story level** has `Building Story = true` and its own plan views. A **reference level** has neither. Make Plan View creates the selected plan view types.
- Copying or arraying a level always creates **reference levels** without views. View › Plan Views lists the levels that have no views yet.
- **Changing the elevation** moves every element whose base or top constraint refers to the level. This happens through `datum` edges; offsets are kept.
- **Deleting a level** deletes its views and every element hosted on it, after a confirmation dialog that lists the counts by category.
- **Instance parameters:** Elevation, Name, Story Above (used for IFC export height), **Computation Height** (the height at which room areas are measured, default 0), Scope Box, Structural, Building Story, Design Option (read-only).
- **Type parameters:** Elevation Base (Project Base Point or Survey Point), Line Weight, Colour, Line Pattern, Symbol (head with bubble, head without bubble, or none), Symbol at End 1 Default and Symbol at End 2 Default.
- Datum extents and head controls follow ARC-DATUM-001.

**Regeneration.** A level is a root node. Its outgoing `datum` edges go to all level-based elements (walls, floors, roofs, ceilings, components, rooms, stairs and so on) and to the views associated with it.

**Data (IFC mapping).**
- **Story level:** `IfcBuildingStorey`, aggregated under `IfcBuilding` (`IfcRelAggregates`). `Elevation` is set from the level elevation in the building's coordinate system, and `ObjectPlacement` is relative to the building. `Pset_BuildingStoreyCommon` holds `AboveGround` and `EntranceLevel` where the user has set them.
- **Reference level:** `IfcAnnotation` with `ObjectType = "ReferenceLevel"`, contained in the nearest story level below it, with a 3D `Axis` representation (a horizontal line). (4X3: `IfcAnnotation.PredefinedType = USERDEFINED`.)
- **Parameter values** (Elevation, Computation Height, Story Above, Structural, Building Story) are stored as IFC properties on the level. The level type is an `IfcTypeObject` with `ApplicableOccurrence = "IfcBuildingStorey"`, and its graphic parameters are stored as properties on that type.
- **RXD element data** (`recipe.datum`): the 3D extent, the per-view 2D extents keyed by view GlobalId, bubble flags, elbows, end locks and the scope box assignment.

**Commands.**
- `level.create(elevation, name?, type_id?, make_plan_views?: bool, plan_view_types?: [view_type_id], extent?: Line) -> element_id`
- `level.set_elevation(level_id, elevation)`
- `level.rename(level_id, name, rename_views?: bool)`
- `level.set_story(level_id, is_building_story)`
- `level.create_plan_views(level_ids, view_type_ids) -> [view_id]`

**Acceptance criteria.**
- [ ] Changing a level's elevation by +300 mm raises every wall based on it by 300 mm and changes the height of every wall whose top constraint is that level.
- [ ] Copying a level produces a reference level with no plan views, and it is exported as `IfcAnnotation`, not as a storey.
- [ ] Deleting a level shows the dependent-element count before it commits.

### 3.2.2 Grids
**ID:** ARC-GRID-001 · **Priority:** P0 · **Ref:** research/02 §3.2 · screenshots/architecture/grid_type_properties_dialog.png

**Purpose.** Vertical datum planes that organise the design. Columns can follow them.

**Workflow.**
1. The user starts Architecture › Datum › **Grid** (`GR`) in a plan view, or in a section or elevation to adjust vertical extents.
2. Draw choices: Line, Start-End-Radius Arc, Center-Ends Arc, Pick Lines, and **Multi-Segment**. Multi-Segment opens a sketch in which the user draws a connected chain of lines and arcs and then presses Finish.
3. The user clicks the start and the end. The label increments automatically: numbers follow numbers and letters follow letters, and renaming the first grid to a letter switches the series to letters.
4. Grid ends align with parallel grids and show a lock (ARC-DATUM-001).

**UI.** Selected-grid controls: bubble checkboxes at each end, the 2D/3D extent toggle, Add Elbow, end locks, and centre-segment handles when Center Segment is None or Custom.

**Behaviour & rules.**
- **Type parameters:** Symbol, **Center Segment** (Continuous, None or Custom) with its weight, colour and pattern, End Segment weight, colour and pattern, **End Segments Length** (in paper space), plan-view symbol defaults for each end, and the non-plan-view symbol default (Top, Bottom, Both or None).
- **Instance parameters:** Name and Scope Box.
- Grid names must be unique in the project. A duplicate name is rejected with a warning.
- A multi-segment grid is edited only through *Edit Sketch*. It cannot have different scopes for different segments, and Copy/Monitor (COL chapter) cannot monitor it.
- Arc grids appear only in plan views. A grid appears in a section or elevation only where the view plane crosses it.

**Regeneration.** Root node, with outgoing `datum` edges to columns that have *Moves With Grids* on, to grid-locked constraints, and to dimensions.

**Data (IFC mapping).**
- **`IfcGrid`** is contained in the ground story. By default Reviera writes one per scope box plus one for the grids that have no scope box. Each grid line is an **`IfcGridAxis`** with `AxisTag` = Name, `AxisCurve` set to `IfcPolyline` / `IfcTrimmedCurve` / `IfcCompositeCurve` (for multi-segment grids), and `SameSense`.
- **U/V assignment** follows DM-IFC-006: lines within ±45° of project X are `UAxes`, the others `VAxes`, and arcs and multi-segment grids `WAxes`. IFC4 requires `VAxes` to be non-empty; see Open questions.
- **Per-axis data** is RXD, as DM-IFC-006 specifies: one RXD document per grid line, `model/datums/<axis GlobalId>`, holding the grid system's GlobalId, the curve, bubble flags, the type reference, the 3D and per-view 2D extents, elbow offsets and the scope box. The grid's name is the `AxisTag`. The `IfcGrid`'s own `Reviera_Element` data only marks it as a Reviera grid system.
- **Edits by other tools.** On open, the `IfcGridAxis` entities win over the RXD records (DM-IFC-006): moved, retagged, added and removed axes move, rename, add and remove grids.
- **Grid type:** `IfcTypeObject` (`ApplicableOccurrence = "IfcGrid"`) whose graphic parameters are stored as properties on the type.

**Commands.**
- `grid.create(curve | segments: [curve], name?, type_id?, scope_box_id?) -> element_id`
- `grid.rename(grid_id, name)`
- `grid.set_curve(grid_id, curve)`

**Acceptance criteria.**
- [ ] Ten grids drawn in sequence get the names 1–10. When the first grid of a new run is renamed "A", the next ones are named B, C, and so on.
- [ ] A column with *Moves With Grids* follows the grid when the grid is moved, and stays put when the setting is off.
- [ ] Every grid line round-trips through IFC with the same name, curve and extents.

### 3.2.3 Reference planes
**ID:** ARC-REFP-001 · **Priority:** P0 · **Ref:** research/02 §3.3

**Purpose.** Infinite construction planes for alignment, dimensioning, hosting work planes, sketch guides and family skeletons.

**Workflow.** The user starts Work Plane › **Ref Plane** (`RP`) and either draws a line with two clicks or uses Pick Lines (with *Offset* and *Lock*). The plane passes through the line and is at right angles to the view plane.

**Behaviour & rules.**
- A reference plane is shown as a green dashed line in every view to which it is perpendicular, with extent handles.
- **Properties:** Name (named planes appear in the Work Plane dialog list), Subcategory (user-defined, for visibility control) and Scope Box. In families: *Is Reference* (Not a Reference, Strong, Weak, Left, Center (Left/Right), Right, Front, Back, Bottom, Top, …) and *Defines Origin*.
- 2D and 3D extents work as described in ARC-DATUM-001. The plane itself is unbounded for hosting and alignment purposes.

**Regeneration.** Root node, with `datum` edges to elements whose work plane is this plane and to aligned or locked references.

**Data (IFC mapping).** `IfcAnnotation` with `ObjectType = "ReferencePlane"`, `Name` = the plane's name and an `Axis` representation (the defining line). Subcategory, Is Reference and Defines Origin are parameter values stored as properties. The plane's normal, extents and scope box are stored in RXD element data (`recipe.datum`).

**Commands.** `refplane.create(p0, p1, view_normal | normal, name?, subcategory?) -> element_id`, `refplane.rename(id, name)`.

**Acceptance criteria.**
- [ ] A named reference plane appears in the Work Plane › Name list, and a model line can be sketched on it.
- [ ] A reference plane drawn in plan is invisible in a 3D view and visible in any section cut at right angles to it.

### 3.2.4 Datum extents
**ID:** ARC-DATUM-001 · **Priority:** P0 · **Ref:** research/02 §3.1, §3.2, §3.5

**Purpose.** Control how far levels, grids and reference planes extend, either in all views or in one view.

**Behaviour & rules.**
- **3D (model) extents** are the datum's real extent and are shared by every parallel view. **2D (view-specific) extents** override them in one view. A grip label toggles between "3D" and "2D".
- Right-click › **Reset to 3D Extents** removes the 2D override. **Maximize 3D Extents** extends the datum to the model's bounding box.
- **Propagate Extents** (Modify | Grids/Levels › Datum panel) copies the current view's 2D extents to the selected parallel views.
- **Alignment locks.** Aligned datum ends show a padlock. When it is locked, dragging one end drags all the aligned ends.
- **Add Elbow** inserts a jog near the bubble, with two draggable vertices. The jog is per view.
- Each end has a bubble-visibility flag, per view.
- A datum appears in a view only if its 3D extent (or its scope box, if it has one) crosses the view's cut plane or crop region. Datums that are not perpendicular to the view appear only where they cross it.

**Commands.**
- `datum.set_extent(datum_id, view_id?, end, point)` (no `view_id` means the 3D extent)
- `datum.reset_to_3d(datum_ids, view_id)`
- `datum.maximize_3d(datum_ids)`
- `datum.propagate_extents(datum_ids, source_view_id, target_view_ids)`
- `datum.set_bubble(datum_id, view_id, end, visible)`
- `datum.add_elbow(datum_id, view_id, end)`

**Acceptance criteria.**
- [ ] Dragging a grid end in 2D mode in plan view A does not change it in plan view B. Propagating the extents to B does.
- [ ] Locked aligned level ends move together. After unlocking, they move independently.

### 3.2.5 Scope boxes
**ID:** ARC-SCOPE-001 · **Priority:** P1 · **Ref:** research/02 §3.4 · screenshots/architecture/scope_boxes_in_plan.png

**Purpose.** Named 3D boxes that control where datums are visible and what their extents are, and that drive consistent view crops.

**Workflow.**
1. The user starts View › Create › **Scope Box**. This works only in plan views. The user sets *Name* and *Height* on the contextual tab and clicks two opposite corners.
2. The box can then be resized with its handles in plan or 3D, and rotated with its rotate control or the Rotate tool.
3. The user assigns datums to the box (Properties › *Scope Box*) and assigns views to the box (view property *Scope Box*).

**Behaviour & rules.**
- A datum assigned to a box appears only in views whose cut plane or view plane crosses the box. Its 3D extents are set to the box faces plus a margin of 2 mm in paper space. Moving or resizing the box moves the datum ends.
- A view assigned to a box gets a crop region equal to the box footprint (rotated if the box is rotated) and an unlocked crop. Changing the box updates the crop.
- **Views Visible** (an instance property with an Edit dialog) lists the views in which the box itself is drawn.
- Deleting a box clears all assignments. Datums keep their current extents.

**Regeneration.** Root node, with `datum` edges to assigned datums and to the crop regions of assigned views.

**Data (IFC mapping).** `IfcAnnotation` with `ObjectType = "ScopeBox"`, `Name` = the box name and a `Box` representation (`IfcBoundingBox`, oriented by `ObjectPlacement`). RXD element data (`recipe.datum`) on the box holds the Views Visible list. Datums and views record their box assignment in their own RXD data.

**Commands.**
- `scopebox.create(name, rectangle: {corner_a, corner_b, rotation?}, height, level_id?) -> element_id`
- `scopebox.set_extents(id, min, max, rotation?)`
- `datum.assign_scope_box(datum_ids, scope_box_id | null)`
- `view.assign_scope_box(view_ids, scope_box_id | null)` (DOC owns views, and this command is listed here for completeness)

**Acceptance criteria.**
- [ ] Rotating a scope box by 30° rotates the crop region of every assigned view by 30°.
- [ ] A grid assigned to a box that does not reach Level 3 is not shown on the Level 3 plan.

### 3.2.6 Work planes
**ID:** ARC-WP-001 · **Priority:** P0 · **Ref:** research/02 §3.6 · web: Edit with the Workplane Viewer; Zoom to Fit Workplane Viewer

**Purpose.** Every view has an active work plane, which is used for sketching, placing work-plane-based families, model lines and text, and for in-plane Rotate and Mirror in 3D.

**Workflow.**
1. The user opens Work Plane › **Set**. The Work Plane dialog shows the current plane (with *Show* and *Dissociate*) and offers four ways to specify a new one:
   - **Name**: choose a level, grid or named reference plane;
   - **Pick a plane**: any planar dimensionable face, including faces in linked models;
   - **Pick a line**: use the plane the line was sketched on;
   - **Pick a new host**: available only when a work-plane-based element is selected; the user then chooses Vertical Face, Face or Work Plane.
2. If the chosen plane is perpendicular to the current view, a **Go To View** dialog lists suitable views and opens the one the user chooses.

**UI.**
- **Show** toggles a translucent plane with a grid, whose spacing is the work plane's *Work Plane Grid Spacing* property.
- **Viewer** opens the **Work Plane Viewer** (below), a temporary view looking straight at the active plane.
- Set › **Pick a Plane** is available as a direct shortcut.

**Work Plane Viewer.** Architecture › Work Plane › **Viewer**, also on the Work Plane panel of the `Modify | <Element>` tab for work-plane-based elements and sketch-based forms (sweeps, swept blends, in-place and mass forms).
- **Which plane.** With a work-plane-based element or a profile selected, the viewer looks at that element's plane. Otherwise it looks at the active work plane of the view it was started from (the last active view). If that view has no work plane (an elevation or section where none has been set), Reviera asks for one first, as for any work-plane tool.
- **What it is.** An orthographic view whose view direction is the plane's normal and whose up direction is the plane's Y axis (or world +Z projected onto the plane when the plane is horizontal). It opens in a new tab or floating window, **zoomed to the extents of the model**; ordinary zoom and pan then apply. It has no crop and no far clip, and uses the *Medium* detail level and the source view's visual style.
- **Temporary.** It is not listed in the Project Browser, cannot be placed on a sheet, renamed or saved, and is discarded when closed or when the project closes. Only one viewer exists per project window; starting it again re-aims the existing one.
- **Editing.** All modify and sketch tools work in it. Edits are ordinary transactions, so they appear in every other view at once and undo as usual. Its work plane is the plane it looks at.
- **Data.** Nothing is written to IFC or RXD; the viewer is session state only.

**Behaviour & rules.**
- The default work plane of a plan view is its associated level. Elevations and sections have no default and must be set before a work-plane tool can run; if none is set, Reviera asks the user for one.
- A work-plane-based element stores its host plane. Moving the host moves the element. The element exposes the *Work Plane* instance parameter, *Edit Work Plane* and *Pick New*.
- *Dissociate* fixes the element in place: it keeps its current world position and loses the host.

**Regeneration.** `datum` edge from the hosting plane (level, grid, reference plane or face) to every work-plane-based element.

**Data (IFC mapping).** View work planes are view data (DOC chapter). Each element's work-plane host (GlobalId plus face key, origin, X direction and normal) is stored in RXD element data (`placement`). The element's placement is written as an `IfcLocalPlacement` relative to the host's placement when the host is a product, or to the storey otherwise.

**Commands.**
- `workplane.set(view_id, by: Name(datum_id) | Face(host_ref) | Line(element_id) | Plane(origin, normal, xdir)) -> plane`
- `workplane.show(view_id, on)`
- `workplane.viewer(view_id? (default: the active view)) -> view_id` opens or re-aims the Work Plane Viewer and returns its transient view ID, which is valid only for the session
- `element.set_work_plane(element_ids, plane | host_ref)`
- `element.dissociate_work_plane(element_ids)`

**Acceptance criteria.**
- [ ] Model text placed on a named reference plane moves with the plane when the plane is moved.
- [ ] Choosing a vertical plane in a plan view offers Go To View with at least one section or elevation.
- [ ] With a sweep's profile plane selected, Viewer opens a view normal to that plane, zoomed to the model extents. Editing the profile there updates the open 3D view without a refresh, and closing the viewer leaves no new entry in the Project Browser.

---

## 3.3 Walls

### 3.3.1 Wall kinds and function
**ID:** ARC-WALL-001 · **Priority:** P0 · **Ref:** research/02 §4.1

**Purpose.** Define which kinds of wall Reviera supports and how each one is classified.

| Kind | Description | Priority | Spec |
|---|---|---|---|
| **Basic wall** | A single compound assembly of layers, optionally with vertical regions | P0 | §3.3.2–§3.3.15 |
| **Stacked wall** | Basic sub-walls stacked vertically in one element | P1 | ARC-STKW-001 |
| **Curtain wall** | A grid, panel and mullion system | P1 | §3.5 |
| **In-place wall** | A free-form in-place family in the Walls category | P1 | ARC-INPL-001 |
| **Wall by face** | A basic wall generated from a mass or generic-model face | P2 | ARC-MASS-003 |

**Behaviour & rules.**
- The type parameter **Function** takes one of Interior, Exterior, Foundation, Retaining, Soffit or Core-Shaft. Filters, schedules and analysis export use it, and it sets the IFC classification shown below.
- Architecture › Build › Wall offers: *Wall: Architectural* (`WA`), *Wall: Structural* (the same tool with Structural = true; STR chapter), *Wall by Face*, *Wall: Sweep* and *Wall: Reveal*.

**Data (IFC mapping).** Each wall is an **`IfcWall`** with an **`IfcWallType`**. The IFC4 exporter can write `IfcWallStandardCase` for a wall that is vertical, has a line or arc axis, has constant thickness, uses `IfcMaterialLayerSetUsage` and has an unedited profile. The default is plain `IfcWall`, which is valid in both IFC4 and 4X3.

| Function | `PredefinedType` (IFC4) | (4X3) | `Pset_WallCommon.IsExternal` |
|---|---|---|---|
| Interior | `STANDARD` (`PARTITIONING` when Structural = false and the type is flagged as partition) | same | FALSE |
| Exterior | `STANDARD` | same | TRUE |
| Foundation | `USERDEFINED`, ObjectType `FOUNDATIONWALL` | same | TRUE |
| Retaining | `SOLIDWALL` | `RETAININGWALL` | TRUE |
| Soffit | `USERDEFINED`, ObjectType `SOFFIT` | same | FALSE |
| Core-Shaft | `SHEAR` if structural, otherwise `STANDARD` | same | FALSE |
| Stacked (parent) | `ELEMENTEDWALL` | same | from sub-walls |

`Pset_WallCommon.LoadBearing` is set from *Structural*, and `Pset_WallCommon.ExtendToStructure` from whether the wall's top is attached.

**Commands.** See ARC-WALL-002 and later blocks.

**Acceptance criteria.**
- [ ] A Retaining-function wall exports with `SOLIDWALL` in IFC4 and `RETAININGWALL` in IFC4X3.

### 3.3.2 Placing walls
**ID:** ARC-WALL-002 · **Priority:** P0 · **Ref:** research/02 §4.2 · screenshots/architecture/walls_by_room_by_segment.png

**Purpose.** Create walls from drawn or picked curves.

**Workflow.**
1. The user starts Architecture › Build › Wall › *Wall: Architectural* (`WA`) in a plan or 3D view. Slanted and tapered types can also be drawn in elevation and section views.
2. The user chooses a type and, optionally, edits the *New Walls* instance properties (Base Constraint, Location Line, Room Bounding and so on).
3. The user sets the contextual-tab options listed in the table below.
4. The user draws with Line, Rectangle, Inscribed or Circumscribed Polygon, Circle, the arc tools, Ellipse, Partial Ellipse, **Spline** (model lines, reference planes and element edges; Tab picks a chain) or **Pick Faces** (vertical mass or generic-model faces; Tab picks all of them).
5. Spacebar flips the exterior side while drawing. With no flip, the **exterior is on the left** of the drawing direction, so a wall drawn from left to right has its exterior face on the upper side in plan.
6. The user finishes with Esc or *Modify*.

**UI.** Options on the contextual tab:

| Control | Values |
|---|---|
| Level | Base level. Shown only in 3D views; non-story levels are allowed |
| Height / Depth | *Height* draws upward from the base. *Depth* draws downward, and the top is then the base level |
| Top constraint | A level, or *Unconnected* plus a value |
| Location Line | Wall Centerline (default), Core Centerline, Finish Face: Exterior, Finish Face: Interior, Core Face: Exterior, Core Face: Interior |
| Chain | On or off |
| Offset | Distance of the location line from the input curve. Not available for ellipses |
| Radius | Automatic fillet between chained segments |
| Join Status | Allow (default) or Disallow. Kept for the session |
| Auto Join / Auto Join & Lock | Placement-panel toggles (ARC-WALL-015) |
| Place By Segment / Place By Room | Placement-panel modes (ARC-WALL-014) |
| Hosted Wall | Constraint-panel toggle, used with By Segment and By Room (ARC-WALL-016) |
| Room Bounding | Pre-set in *New Walls* properties |

**Behaviour & rules.**
- Each drawn segment becomes one wall. In a chain, consecutive walls are joined end to end (ARC-JOIN-001). A *Radius* value inserts an arc wall between each pair of segments.
- **Spline walls** (*owner clarification, 2026-10-13*):
  - Drawing: the user clicks control points (open, or closed with *Close*), or picks a spline model line. The wall's location line is a cubic B-spline through the points (interpolating), or by control points (*By Control Points* option).
  - Editing: grips on the points edit the curve, and Tab inserts a point.
  - Geometry: the faces are offset curves of the location line (`Geom2d_OffsetCurve`), approximated by B-splines within 0.01 mm. Where the offset would self-intersect, the curvature radius is below half the width, and the wall is refused with *"Curve too tight for the wall's width"* and the point marked.
  - Wall behaviour: spline walls join, host inserts (an insert's along-position is measured on the arc length, and its opening follows the curve), attach to roofs and floors, and appear in schedules with their arc length.
  - IFC: the body is an `IfcSweptAreaSolid` along an `IfcBSplineCurveWithKnots` axis (or a faceted B-rep when a consumer needs it), and the `Axis` representation is the B-spline curve.
  - Command: `wall.create` takes `curve: Spline{ctrl, knots, degree}` or `{through: [points], closed?}`.
- Ellipse and partial-ellipse walls require Location Line = Wall Centerline and ignore Spacebar.
- A wall shorter than `TOL_SHORT`, or shorter than its own width when Join Status is Allow, is rejected with "Wall is too short".
- Pick Faces creates walls whose location line lies on the face, according to the Location Line option, and records *Related to Mass* (ARC-MASS-003).
- **Location line persistence.**
  - The location line stays in place through type changes.
  - Changing the *Location Line* parameter of a placed wall leaves its geometry where it is. Reviera recomputes the stored location-curve offset so that the body does not move.
  - Flipping mirrors the wall about its **current** location line.
  - The blue end dots are drawn on the location line.

**Regeneration.** Edges into the wall: `datum` from the base and top levels; `join` from its join nodes; `type` from the wall type; `attach` from attach targets; `host` from the host wall (hosted walls). Edges out of the wall: `host` to its inserts; `bound` to rooms on the levels the wall spans; `cut` to embedded curtain-wall hosts.

**Data (IFC mapping).**
- **Placement and axis.** `ObjectPlacement` puts the local X axis along the start tangent, with the origin at the start point of the **wall centreline** at the base elevation. The `Axis` representation is the location *centreline* as an `IfcPolyline` (straight walls) or `IfcTrimmedCurve` on an `IfcCircle` / `IfcEllipse`.
- **Body.** The `Body` representation follows §3.0.4.
- **Layer usage.** `IfcMaterialLayerSetUsage` with `LayerSetDirection = AXIS2`. The exterior is at local +Y. Layers are listed from exterior to interior, so an unflipped wall has `DirectionSense = NEGATIVE` and `OffsetFromReferenceLine = +d_ext`, where `d_ext` is the distance from the centreline to the exterior finish face. A flipped wall has `DirectionSense = POSITIVE` and `OffsetFromReferenceLine = -d_ext`. `ReferenceExtent` is the unconnected height.
- **Parameter values.** Base and top constraints are stored as storey GlobalIds and offsets; Room Bounding, Cross-Section, Angle and the others are IFC properties (DM property sets).
- **RXD.** Element data (`recipe.wall`) holds the location-line choice, flip state and per-end join status. RXD document `model/constraints` holds the join-node overrides.

**Commands.**
- `wall.create(type_id, curve, base_level, top_constraint: Level(id) | Unconnected(height), base_offset?, top_offset?, location_line?, flip?: bool, structural?: bool, structural_usage?, room_bounding?: bool, join_status?: Allow|Disallow, auto_join?: Off|Join|JoinAndLock) -> element_id`
- `wall.create_chain(type_id, curves: [curve], closed?: bool, fillet_radius?, ...same as wall.create) -> [element_id]`
- `wall.create_from_faces(type_id, face_refs, base_level, location_line, ...) -> [element_id]`
- `wall.set_location(wall_id, curve)`
- `wall.set_constraints(wall_id, base_level?, base_offset?, top_constraint?: Level(id) | Unconnected(height), top_offset?)` — omitted values are kept; a top at or below the base is refused. *Built; the wall's Properties (Constraints: Base Constraint, Base Offset, Top Constraint, Unconnected Height, Top Offset; Type: choose, Duplicate Type…, Type Name, Width) use it and the type commands.*
- `wall.set_location_line(wall_ids, location_line)`
- `wall.flip(wall_ids)`

**Acceptance criteria.**
- [ ] A rectangle drawn clockwise in plan produces four walls whose exterior faces all point outward, and a counter-clockwise rectangle puts the exterior faces inward (exterior on the left).
- [ ] Changing a placed wall's Location Line from Centerline to Finish Face: Exterior does not move any face. A flip after that change mirrors the wall about the exterior face.
- [ ] A 250 mm wall, unflipped, with its centreline as the location line, exports `OffsetFromReferenceLine = 0.125` and `DirectionSense = NEGATIVE`, and an independent IFC viewer shows the layers on the correct sides.

**Implementation notes (2026-10-13), curved walls.** As built:
- **Curve:** `wall.create` and `wall.set_location` take `curve: {line, through: [points]}`. One point makes a circular arc through the start, the point and the end; two or more make a centripetal Catmull–Rom spline through them all. Points in line with the ends give a straight wall. The recipe keeps `through`; moves, rotations and mirrors carry it.
- **Sampling:** the curve is sampled as a polyline (arcs at most 3° per segment, splines 16 segments per span). Tangents are averaged at the samples (exact on an arc), and ends are square to the end tangent.
- **Body:** one closed prism per segment between the curve offset half the width to each side, mitred at shared edges, and split at opening edges. Doors and windows cut it along the curve's length.
- **Inserts:** an insert's `along` is measured on the curve. A plan point is projected onto it. The insert stays straight, square to the curve where it stands, and fits within the curve's length.
- **Rooms:** curved walls bound rooms by their offset polylines.
- **Joins:** each end of a curved wall joins through a 1 m stand-in along its end tangent. The stand-in takes part in L and T joins like any wall, and its solved corners become the curved wall's end corners. A straight wall meeting an arc gets a clean mitre, and the join names the curved wall. Two arcs meeting tangentially (a drum's halves) are parallel there and stay butted.
- **Attached tops** (ARC-WALL-008): a curved wall attaches to a roof or dome piece by piece. Each piece is attached like a short straight wall, and the roof must cover the whole curve. `roof.attach_walls` finds curved perimeter walls, judged at the curve's middle. A dome needs a small overhang past the drum's outer face so its facets cover the wall.
- **IFC:** a faceted B-rep in a frame at the wall's start along its chord. Openings are placed at their point and tangent on the curve in that frame.
- **Window:** Architecture › Arc Wall (start, end, then a point on the arc) and Spline Wall (points; Enter ends at the last), each with a live preview.
- **Later:**
  - T joins into a curved wall's side away from its ends;
  - a true curved IFC body and axis (`IfcTrimmedCurve` / `IfcBSplineCurveWithKnots`);
  - the "curve too tight" check;
  - closed splines;
  - drawing a chain of arcs;
  - smooth plan poché (the segments show).

### 3.3.3 Wall instance parameters
**ID:** ARC-WALL-003 · **Priority:** P0 · **Ref:** research/02 §4.3

| Group | Parameters | Notes |
|---|---|---|
| Constraints | Location Line; Location Line Offset (only when the wall is used as a curtain panel); **Base Constraint**; **Base Offset**; Base Is Attached (RO); Base Extension Distance; **Top Constraint**; **Unconnected Height**; **Top Offset**; Top Is Attached (RO); Top Extension Distance; **Room Bounding**; Related to Mass (RO); Hosted (RO); Offset from Host | The extension distances are enabled only when the type has extendable layers (ARC-CMPD-004). The host fields apply to hosted walls only |
| Cross-Section | **Cross-Section** = Vertical, Slanted or Tapered; **Angle From Vertical** (Slanted, −90° to +90°); Enable Angle Overrides; Exterior Angle; Interior Angle (Tapered) | ARC-WALL-005 |
| Structural | Structural; Enable Analytical Model; Structural Usage | STR chapter |
| Dimensions | Length, Area, Volume (RO) | Exported as `Qto_WallBaseQuantities` (Length, Width, Height, GrossSideArea, NetSideArea, GrossVolume, NetVolume) |
| Identity | Image, Comments, Mark | `Name` = Mark if one is set, `Description` = Comments |
| Phasing | Phase Created, Phase Demolished | DM chapter |

**Behaviour & rules.**
- **Height computation.** `z_base = elev(BaseConstraint) + BaseOffset`. `z_top = elev(TopConstraint) + TopOffset`, or `z_base + UnconnectedHeight` when the wall is unconnected. The result must satisfy `z_top − z_base > TOL_SHORT`; otherwise Reviera reports the error "Wall top below base" and leaves the wall unregenerated (its last valid body is kept and the wall is flagged).
- **Length** is measured along the location centreline after joins are resolved, as the reference product does at the wall's computation length.
- **Area** is measured on the exterior face (finish exterior), net of openings.

**Acceptance criteria.**
- [ ] With Top Constraint = Level 2 (elevation 3.0) and Top Offset = −0.2, the wall height equals 2.8 m minus the base offset.

### 3.3.4 Wall type parameters
**ID:** ARC-WALL-004 · **Priority:** P0 · **Ref:** research/02 §4.4

**Behaviour & rules.** Type parameters:
- Structure (opens Edit Assembly);
- **Wrapping at Inserts** (Do not wrap, Exterior, Interior or Both);
- **Wrapping at Ends** (None, Exterior or Interior);
- Width (RO, the sum of the layer thicknesses);
- Function;
- Default Exterior Angle and Default Interior Angle (tapered walls);
- Width Measured At (Top or Base, tapered walls);
- Coarse Scale Fill Pattern and Coarse Scale Fill Colour;
- identity data (Keynote, Model, Manufacturer, Type Comments, URL, Description, Assembly Code, Type Mark, Fire Rating, Cost);
- analytical properties (Heat Transfer Coefficient U, Thermal Resistance R, Thermal Mass, Absorptance, Roughness).

R and U are computed from the layer materials' thermal assets: `R = Rsi + Σ tᵢ/λᵢ + Rse`, using the surface resistances set in the project's energy settings (ANA chapter).

**Data (IFC mapping).**
- `IfcWallType` (`PredefinedType` as in ARC-WALL-001).
- `Pset_WallCommon` on the type: FireRating, ThermalTransmittance (U), IsExternal, AcousticRating where it is set.
- `Pset_ConcreteElementGeneral` and similar sets where the material calls for them.
- Identity data uses `Pset_ManufacturerTypeInformation` and the classification references for the assembly code (`IfcClassificationReference` through `IfcRelAssociatesClassification`).
- The wrapping settings are stored in RXD type data (`recipe.compound`).

**Acceptance criteria.**
- [ ] A type with a 100 mm brick layer (λ 0.77), a 50 mm insulation layer (λ 0.035) and a 100 mm block layer (λ 0.5), with Rsi 0.13 and Rse 0.04, reports R ≈ 1.93 m²K/W.

### 3.3.5 Compound structure model (shared)
**ID:** ARC-CMPD-001 · **Priority:** P0 · **Ref:** research/02 §1 (2026 changes), §4.5, §17.3 · screenshots/architecture/wall_edit_assembly_layers_2026.png

**Purpose.** One layered-structure data model shared by walls, floors, roofs, ceilings, building pads (on import) and toposolids.

**Behaviour & rules.**
- A **compound structure** is an ordered list of layers. Walls are listed from exterior to interior. Floors, roofs, ceilings and toposolids are listed from top to bottom.
- Each layer has these fields:
  - `function` ∈ {Structure, Substrate, Thermal/Air, Finish 1, Finish 2, Membrane};
  - `priority` (an integer 1–9, where 1 is the highest; **independent of function**);
  - `material`;
  - `thickness`;
  - `wraps` (bool);
  - `structural_material` (bool; at most one layer, and that layer must be in the core);
  - `variable` (bool; at most one layer).
- **Default priorities** apply when a layer is created: Structure 1, Substrate 2, Thermal/Air 3, Finish 1 4, Finish 2 5, Membrane 0. Membrane means zero thickness and no part in priority cleanup. The user may change any non-membrane priority.
- **Core.** Two boundary markers, `core_start` and `core_end`, are layer indices with `0 ≤ core_start ≤ core_end ≤ n`. The core is the layer range `[core_start, core_end)` and may be **empty** (`core_start = core_end`). When the core becomes empty, every dimension and constraint that referenced a core face is deleted, after a warning that lists them.
- **Validation:**
  - Every non-membrane layer must be at least 1/8" thick (3.2 mm).
  - Membrane layers must have thickness 0.
  - Core layers must have thickness greater than 0.
  - Priorities must not *ascend* from the core outward: on each side of the core, walking from the core boundary toward the face, each layer's priority number must be greater than or equal to the previous one (lower or equal priority). Membranes are exempt.
  - At most one layer may be `variable`. A variable layer is required for tapered walls and allowed for shape-edited slabs.
  - A type with a variable layer cannot use vertical-structure editing (ARC-CMPD-003).
- The *Default Wrapping* settings (At Inserts and At Ends) are type parameters that the Edit Assembly dialog edits.

**Data (IFC mapping).**
- **`IfcMaterialLayerSet`** is associated with the type. Each layer is an **`IfcMaterialLayer`** with:
  - `Material`;
  - `LayerThickness`;
  - `IsVentilated` (TRUE for an Air layer, FALSE otherwise, UNKNOWN for a membrane);
  - `Name` = the function name;
  - `Category` = `LoadBearing` (Structure), `Insulation` (Thermal), `Finish` (Finish 1/2), `Membrane` or `Substrate`;
  - `Priority` = `100 − 10·(p−1)`, clamped to 0–100, with membranes written as 0. IFC's priority runs the other way (higher means stronger), so Reviera reverses the scale on export.
- The occurrence carries `IfcMaterialLayerSetUsage`.
- **RXD type data** (`recipe.compound`): the exact Reviera priority, wraps, variable and structural flags per layer (keyed by a named layer sub-reference, not by index), `core_start` and `core_end`, the vertical regions, the type-level sweeps and reveals, and the default wrapping.
- **Import without RXD.** When a type has no RXD data (for example, it was created by another tool), Reviera infers the core as the contiguous `LoadBearing` layers and the priorities from `Category`.

**Commands.**
- `compound.set_layers(type_id, layers: [{function, priority?, material_id, thickness, wraps?, structural_material?, variable?}], core_start, core_end)`
- `compound.insert_layer(type_id, index, layer)`
- `compound.delete_layer(type_id, index)`
- `compound.move_layer(type_id, index, delta)`
- `compound.duplicate_layer(type_id, index)`
- `compound.set_default_wrapping(type_id, at_inserts, at_ends)`

**Acceptance criteria.**
- [ ] A type whose layers are all outside the core (empty core) saves, regenerates and round-trips through IFC and RXD.
- [ ] Setting a Finish layer to priority 1 while it lies between a priority-3 layer and the core fails validation with a message naming both layers.
- [ ] Reading an IFC with Priority 100/90/80 and no RXD data yields Reviera priorities 1/2/3.

### 3.3.6 Edit Assembly dialog
**ID:** ARC-CMPD-002 · **Priority:** P0 · **Ref:** research/02 §4.5 · screenshots/architecture/wall_edit_assembly_layers_2026.png

**Purpose.** The UI for editing a compound structure.

**UI.**
- **Header:** Family, Type, Total thickness, Resistance R, Thermal Mass and Sample Height (the height of the wall preview).
- **Layers grid**, with exterior (or top) at the top: Function, **Priority**, Material (opens the Material Browser), Thickness, Wraps, Structural Material and Variable. Two locked rows mark the core boundaries ("Core Boundary — above" and "Core Boundary — below"), and the user can move them with Up and Down.
- **Buttons:** Insert, Delete, **Duplicate**, Up and Down.
- **Default Wrapping** drop-downs (walls only).
- **Modify Vertical Structure** (walls only, enabled when the preview is set to Section): Modify, Merge Regions, Assign Layers, Split Region, Sweeps… and Reveals….
- **Preview** pane with a View choice of Floor Plan or Section, which renders the real join-free geometry of a sample wall.

**Behaviour & rules.** The dialog edits a copy of the type. **OK** commits one transaction that updates the type and regenerates every instance. **Cancel** discards the copy. Validation (ARC-CMPD-001) runs on OK, and its errors are shown inline next to the offending row.

**Commands.** The `compound.*` commands, applied as a batch.

**Acceptance criteria.**
- [ ] Changing a layer's material and pressing OK updates all instances. Cancel leaves them untouched.

### 3.3.7 Vertically compound walls (regions)
**ID:** ARC-CMPD-003 · **Priority:** P1 · **Ref:** research/02 §4.5

**Purpose.** Wall types whose material changes with height, for example a brick plinth below render.

**Behaviour & rules.**
- **Region model.** The wall cross-section in the type's sample plane (the thickness direction against height) is divided into rectangular **regions**. Each region is assigned to one layer row, which defines its function, priority and material. Region boundaries are horizontal or vertical segments. A horizontal boundary carries a **distance from the base or from the top** of the wall, and a padlock marks which of the two it is.
- **Split Region.** Hovering a vertical layer border previews a *horizontal* split at the cursor height, and hovering a horizontal border previews a *vertical* split. A click commits the split, and both new regions keep the material they had.
- **Merge Regions.** The user clicks a border, and an arrow shows which region absorbs the other.
- **Assign Layers.** The user selects a row and then clicks regions to assign that row to them.
- **Modify.** Selecting an outer vertical boundary shows a temporary dimension that sets the thickness. Selecting the top or bottom edge of a layer shows a padlock: an **unlocked** layer is *extendable* (ARC-CMPD-004).
- **Rules:**
  - Rows must be contiguous from left to right.
  - A row cannot appear on both sides of the core.
  - Core boundaries and membranes cannot step.
  - Only layers that run the full height can have a thickness.
  - Moving one exterior edge moves every exterior edge.
- **Instance geometry.** At height `z` within the wall, each region resolves to an absolute interval `[z0, z1]`. The base or top distance is measured from the instance's base or top. If the wall is shorter than the type's fixed regions, regions are clipped from the end opposite their anchor. The variable region (the one with no fixed distance) absorbs the remaining height.

**Data (IFC mapping).** IFC4 has no region concept. The type's `IfcMaterialLayerSet` describes the full-height layers. Regions are stored in RXD type data (`recipe.compound`). The exported `Body` is the true geometry, and with the export option *Parts per layer/region* (P2) each region is also exported as an `IfcBuildingElementPart` aggregated under the wall, carrying its own `IfcMaterial`.

**Commands.**
- `compound.split_region(type_id, region_key, orientation: Horizontal|Vertical, position)`
- `compound.merge_regions(type_id, region_a, region_b, keep: A|B)`
- `compound.assign_layer(type_id, region_keys, layer_index)`
- `compound.set_region_anchor(type_id, boundary_key, from: Base|Top, distance)`

**Acceptance criteria.**
- [ ] A type with a 600 mm brick region anchored to the base shows the brick from 0 to 600 mm on a 3 m wall and on a 1 m wall alike. On a 0.5 m wall the brick region is clipped at 0.5 m.

### 3.3.8 Extendable layers
**ID:** ARC-CMPD-004 · **Priority:** P1 · **Ref:** research/02 §4.3, §4.5

**Purpose.** Let some layers of a wall run past the wall's base or top, for example a brick skin that drops below the slab edge or a parapet finish that rises above the roof line.

**Workflow.**
1. In Edit Assembly (ARC-CMPD-002) with the preview set to Section, the user clicks **Modify**, selects the top or bottom edge of a layer and clicks its padlock to unlock it. The type now marks that layer edge as extendable.
2. In the project, the user sets **Top Extension Distance** or **Base Extension Distance** on wall instances, or selects a wall in a section, elevation or 3D view, presses Tab until the extendable edge's shape handle is highlighted, and drags it.

**Behaviour & rules.**
- Layers whose top (or bottom) edge is unlocked in the type are *extendable*, and unlocked layers must be adjacent to each other.
- **Type rules.** Top and bottom are unlocked independently. The unlocked set at each end is one contiguous run of layers; unlocking a layer that would split the run is refused with "Extendable layers must be adjacent". Membranes follow their neighbours and cannot be unlocked on their own. A layer inside a vertical region (ARC-CMPD-003) can be unlocked only when the region touches that end of the wall.
- The instance parameters **Top Extension Distance** and **Base Extension Distance** move the extendable layers' top or bottom edges by that amount, which can be positive or negative. In section, elevation and 3D views, Tab exposes shape handles for the same edges.
- **Limits.** A negative extension may not shorten the extendable layers to less than `TOL_SHORT` in height ("Layer extension exceeds wall height"). There is no upper limit for positive values. When the type has no extendable layer at an end, that end's parameter is read-only and 0.
- **Geometry.** Each extendable layer's solid is built from the wall's layer solid with its top or base face moved along the wall's vertical direction (along the slant direction for slanted walls, ARC-WALL-010). The move happens after joins and before insert voids, so doors and windows still cut the extended layers. At a corner, two joined walls with equal extensions on the same layer functions produce a clean extended corner; unequal extensions leave a step.
- **Rooms and joins.** Extended parts take part in layer cleanup with joined walls of the same function and priority (ARC-JOIN-001). They do not change the wall's room-bounding trace, because rooms are computed at the computation height (ARC-ROOM-002).
- Extension is disabled while the wall's top or base is attached (ARC-WALL-008). Its parameters become read-only and are ignored.

**Data (IFC mapping).** The extended geometry is part of the wall's `Body`. `IfcMaterialLayerSetUsage` cannot express per-layer heights, so the extension values are written as property values and stored in RXD element data (`recipe.wall`); the unlocked edges are flags per layer in RXD type data (`recipe.compound`).

**Commands.**
- `compound.set_extendable(type_id, index, top?: bool, base?: bool)` sets or clears the padlock state of a layer edge in the type.
- `wall.set_extension(wall_ids, top?: f64, base?: f64)`.

**Acceptance criteria.**
- [ ] With an extendable brick layer and Base Extension Distance = −300 mm, the brick stops 300 mm below the wall base, while the other layers stay at the base.
- [ ] Unlocking the bottom edges of layers 1 and 3 but not layer 2 is refused with a message naming the layers.
- [ ] A window placed in a wall whose brick layer extends 500 mm above the top still cuts the brick, and attaching the wall top to a roof makes Top Extension Distance read-only.

### 3.3.9 Layer wrapping
**ID:** ARC-WRAP-001 · **Priority:** P1 · **Ref:** research/02 §4.4, §4.5 (Wrapping), §5.5

**Purpose.** Finish layers turn into the jambs of openings and around the free ends of walls.

**Behaviour & rules.**
- **Conditions.** A layer wraps when all of these hold: its *Wraps* flag is set; the type's wrapping setting for that side is on; the wall is not tapered; and the host is a wall (floors and roofs never wrap).
  - For **inserts**, wrapping is controlled by *Wrapping at Inserts* (Exterior, Interior, Both or Do not wrap). The insert family's *Wall Closure* type parameter, if set, overrides it.
  - For **ends**, wrapping is controlled by *Wrapping at Ends* (Exterior, Interior or None). Each end of each wall can override it ("By type", "Exterior", "Interior" or "None"), and that override is stored in the wall's RXD element data (`recipe.wall`).
- **Insert-wrapping algorithm.** It runs per insert, in the wall's local frame: u along the wall, v through the thickness from the exterior face (v = 0) inward, and z up.
  1. Let `R` be the rough-opening profile of the insert projected onto the wall's elevation plane (u, z), taken from the family's opening-cut geometry.
  2. Let `c` be the **closure depth**: the v coordinate of the family's *Wall Closure* reference plane. If the family has none, `c` is the core's exterior face.
  3. For the exterior side, let `W = [w₁ … w_k]` be the wrapping layers between v = 0 and the core, ordered from the face inward, with thicknesses `t₁ … t_k`, and let `T_j = Σ_{i≤j} tᵢ`.
  4. Lay out the jamb lining. Wrapping layer `w_j` occupies the ring `Offset(R, T_j) \ Offset(R, T_{j−1})` (a 2D outward offset of the profile) over the depth `v ∈ [v_start(w_j), c]`, where `v_start` is the layer's own starting depth. The outermost face layer therefore lines the jamb surface directly.
  5. Every **non-wrapping** layer, and every layer deeper than the wrapping layers on that side, loses the volume `Offset(R, T_k) × [0, c]` (the "wrap pocket").
  6. The insert's void `R × [0, wall width]` is then cut from all layers.
  7. The interior side is handled the same way from v = width with depths mirrored. *Both* applies the two sides independently, each up to its own closure depth: the exterior up to `c`, and the interior up to `c_int` (a second closure plane in the family, or the interior core face).
  8. **Curved walls.** The 2D operations in steps 4 and 5 are done in the *developed* (u, z) space of the location curve at each layer's offset radius and then mapped back onto the cylinder. OCCT builds the ring solids with `BRepOffsetAPI_MakeOffset` (planar wire offset, `GeomAbs_Intersection` joins) followed by `BRepPrimAPI_MakePrism` for straight walls. For curved walls the developed faces are mapped through `BRepBuilderAPI_GTransform` approximations and the solid is built with `BRepOffsetAPI_ThruSections`.
- **End-wrapping algorithm.** At a wall end that is **not joined** to anything:
  - *Exterior*: the wrapping exterior layers continue around the end face. Each wrapping layer `w_j` forms an L: its band along the wall, plus a cap across the end at distance `T_{j−1} … T_j` beyond the core's end face. The cap runs from the exterior face to the interior face of the core. Interior-side layers stop flush with the outer face of the cap.
  - *Interior*: the same, mirrored.
  - The core end face is the reference. Non-wrapping layers on the wrapping side are shortened by `T_k` so that the caps fit within the original end position.
- Wrapping is recomputed when the insert moves, when the insert family's closure changes, or when the layers change.

**Regeneration.** Edges `host` (insert → wall) and `type` (wall type → wall). The wrap pocket is part of the wall body, so the wall regenerates whenever any insert in it changes.

**Data (IFC mapping).** The result is part of the wall `Body`. The insert's `IfcOpeningElement` stays the rough opening (`R` extruded through the full width). Wrapped layers are only visible in the body and in layer-wise parts (when exported).

**Commands.** `wall.set_end_wrap(wall_id, end: Start|End, mode: ByType|Exterior|Interior|None)`, plus the type commands in ARC-CMPD-001 and ARC-WALL-004.

**Acceptance criteria.**
- [ ] A 12.5 mm plasterboard layer with Wraps on, and Wrapping at Inserts = Interior, lines all four sides of a window opening up to the window's closure plane. In a plan cut the board turns into the jamb.
- [ ] A free wall end with Wrapping at Ends = Exterior shows the exterior render wrapping around the end and stopping at the interior face of the core.
- [ ] A tapered wall ignores all wrapping settings without error.

### 3.3.10 Slanted and tapered walls
**ID:** ARC-WALL-005 · **Priority:** P1 · **Ref:** research/02 §4.3, §4.5 · screenshots/architecture/tapered_walls_section.png

**Purpose.** Walls whose faces are not vertical.

**Behaviour & rules.**
- **Slanted walls** (Cross-Section = Slanted).
  - *Angle From Vertical* `α` ∈ (−90°, +90°). A positive angle tilts the wall top toward the right of the drawing direction, which is the interior side when the wall is unflipped.
  - The cross-section is a parallelogram: at height `h` above the base, every layer is offset by `h·tan α` along the wall normal.
  - In-canvas dot controls at the top let the user set the angle or the top offset, snapping to 1° increments.
  - Inserts and sweeps stay vertical unless their *Orientation* instance parameter is set to Vertical, Perpendicular to Face or Parallel to Ground.
  - Only **butt** joins are allowed (no miter or square-off), and **reveals** are not allowed.
  - *Split with Gap* along a vertical plane is not allowed.
  - Attach targets must cover the wall's complete footprint in plan.
- **Tapered walls** (Cross-Section = Tapered). The type needs a variable layer.
  - **Exterior Angle** `αₑ` and **Interior Angle** `αᵢ` default from the type, and an instance can override them with *Enable Angle Overrides*. A positive angle tilts the top of that face **inward**.
  - The type's thickness is measured at the base or at the top, as *Width Measured At* says.
  - The variable layer absorbs the difference, so its thickness at height h is `t_v(h) = t_v0 − h·(tan αₑ + tan αᵢ)` when measured at the base.
  - If `t_v(h) ≤ 0` anywhere within the wall height, Reviera reports "Variable layer thickness would reach zero" and does not commit the change.
  - Layers on the exterior side of the variable layer stay parallel to the exterior face, and layers on the interior side stay parallel to the interior face.
- **Geometry.** Each layer is the region between two *ruled* surfaces. Straight walls are built as a prism of the layer's 2D cross-section polygon, in the plane normal to the wall axis, along the wall length (`BRepPrimAPI_MakePrism`). Curved walls are built as a sweep of the cross-section along the location curve (`BRepOffsetAPI_MakePipeShell` with a fixed binormal of +Z, which keeps the faces conical).

**Data (IFC mapping).** The body is an `IfcExtrudedAreaSolid` of the parallelogram or trapezoid profile along the wall axis for straight walls, and an `IfcSurfaceCurveSweptAreaSolid` or Brep for curved walls. `IfcMaterialLayerSetUsage` still applies, and for tapered walls IFC4 has `IfcMaterialLayerWithOffsets` (OffsetDirection AXIS3, with the offset values at the top). Reviera writes it for tapered walls.

**Commands.** `wall.set_cross_section(wall_ids, mode: Vertical|Slanted|Tapered, angle?: f64, exterior_angle?: f64, interior_angle?: f64, override_angles?: bool)`.

**Acceptance criteria.**
- [ ] A 3 m slanted wall at +10° has its top face displaced by 0.529 m toward the interior.
- [ ] A tapered type with a 200 mm variable layer, measured at the base, with 1° on each face, built 3 m high, has a variable layer 0.0953 m thick at the top. With 2° on each face the variable layer would reach −0.0095 m within the height, and the change is rejected.

### 3.3.11 Stacked walls
**ID:** ARC-STKW-001 · **Priority:** P1 · **Ref:** research/02 §4.6

**Purpose.** One element that stacks several basic wall types vertically.

**Behaviour & rules.**
- **Type structure:**
  - **Offset reference**, one of Wall Centerline, Core Centerline, Finish Face (Ext/Int) or Core Face (Ext/Int). It becomes each sub-wall's location line.
  - **Sample Height**.
  - Rows from top to bottom. Each row has a basic wall type, a **Height** (exactly one row is *Variable*), an **Offset** from the reference (positive toward the exterior), Top and Base extensions (when the sub-type's layers are extendable) and a **Flip** flag.
  - Buttons: Insert, Delete, Up, Down and Variable.
- **Instance layout.** For wall height `H`, each fixed row takes its own height, and the variable row takes `H − Σ fixed`. If that would be ≤ 0, Reviera reports "Stacked wall too short for its fixed rows".
- Sub-walls are real child elements. They are selectable with Tab and expose a subset of instance parameters (Room Bounding, Structural, phasing, Mark).
- Joins: each sub-wall joins with whatever is at its own height, using the join algorithm (§3.3.12) once per height interval.
- Ellipse stacked walls need Offset = Wall Centerline and every row's offset set to 0.
- A stacked wall can be split only by a vertical plane.
- **Break Up** turns the stacked wall into independent basic walls and keeps their positions and joins.

**Data (IFC mapping).** The parent is an `IfcWall` (`ELEMENTEDWALL`) with an `IfcWallType` (`ELEMENTEDWALL`), and it aggregates (`IfcRelAggregates`) one `IfcWall` per sub-wall, each with its own type and layer set usage. The parent has no `Body` of its own. The row table is stored in RXD type data (`recipe.stacked_wall`).

**Commands.**
- `stackedwall.set_rows(type_id, offset_reference, rows: [{wall_type_id, height | Variable, offset, flip, top_ext?, base_ext?}])`
- `wall.break_up(stacked_wall_ids) -> [element_id]`

**Acceptance criteria.**
- [ ] A stacked wall with a 900 mm fixed base row and a variable upper row, drawn 2.7 m high, has sub-walls 0.9 m and 1.8 m high.
- [ ] Break Up produces two basic walls whose IFC GlobalIds are those of the former sub-walls.

### 3.3.12 Wall joins and layer cleanup
**ID:** ARC-JOIN-001 · **Priority:** P0 · **Ref:** research/02 §1 (2025/2026), §4.5 (Layer join rules), §4.7, §17.4 · screenshots/architecture/compound_layer_priority_joins.png

**Purpose.** Walls that meet are connected cleanly, and their layers are resolved by priority.

**Workflow.**
- Walls join automatically when they are drawn or moved so that an end comes within `TOL_JOIN` of another wall. They do not join if the end's Join Status is Disallow.
- **Wall Joins** (Modify › Geometry): the user clicks inside the grey square that marks a join, or draws a window around several joins, and then chooses:
  - **Butt** (default), **Miter** or **Square off**;
  - **Previous / Next**, which cycles the join order (single join only);
  - *Display*: Use View Setting, Clean Join or Don't Clean Join;
  - **Allow Join / Disallow Join**.
- **In-canvas glyph.** At each selected wall end, in plan and 3D, a glyph toggles Allow/Disallow. A disallowed end stops square at its own location point.

**Behaviour & rules.**

**A. Join detection.** Detection runs per *join node*.
1. For each wall end `E` (point `P_E` on the location curve, tangent `d_E` pointing *out of* the wall) whose status is Allow, Reviera queries an R-tree of wall footprints at `P_E` with radius `TOL_JOIN`.
2. **End-to-end candidates.** Another wall end `F` with `|P_E − P_F| ≤ TOL_JOIN`, or whose extended location lines cross within `max(w_E, w_F)` of both endpoints, forms an **L/corner node**. A third or fourth end joins the same node when it lies within `TOL_JOIN` of the node point.
3. **End-to-span candidates.** If `P_E`, extended along `d_E` by at most `w_other/2 + TOL_JOIN`, lands inside another wall's footprint away from that wall's ends, the result is a **T node** (E abuts the continuous wall).
4. Two walls whose spans cross with neither end involved get no automatic join. The user can create one with Join Geometry.
5. **Conditions for a node:**
   - The walls' vertical extents overlap.
   - The walls are on the same level band, with their base levels within the overlap.
   - The angle between the location lines is above 1°. Parallel overlapping walls only join through Join Geometry.
   - The number of ends in an L node is at most 4. With more, Reviera reports "Too many walls at one join" and leaves the extras unjoined.
6. The node's **join order** is a list of the wall ends in it. By default the first wall drawn comes first. *Previous* and *Next* rotate the list.
7. **Mid-end faces.** Vertical faces created by an edited profile or by a wall opening (ARC-WALL-009, ARC-OPEN-003) are join targets just like wall ends. A wall end that abuts a notch face forms a T node against that face.

**B. Layer bands.** For each wall `W` at height interval `I` (see C), Reviera builds the plan **bands**:
- Each layer `j` occupies the strip between two offset curves of the location centreline, at signed distances `a_j` and `b_j`. Positive distances are toward the exterior, and the values follow from the layer thicknesses and the flip.
- Every band carries `(wall, layer index, priority p, in_core flag, material)`.
- Membrane layers have zero width. They take no part in the cleanup and are re-inserted afterward as zero-thickness boundaries.
- Arc walls produce concentric arcs. Elliptical walls produce offset curves (`Geom2d_OffsetCurve`), which are approximated by B-splines to within 0.01 mm where an exact type is needed.

**C. Height intervals.** The join is solved separately in each vertical interval in which the set of participating bands is constant.
1. Collect breakpoints: the base and top of each wall in the node (after attachment, sampled at the node location), every region boundary (ARC-CMPD-003) and every extension boundary (ARC-CMPD-004).
2. Sort the breakpoints. Each pair of consecutive breakpoints defines an interval `I = [z₀, z₁]`.
3. Inside each interval, run step D with the bands present there. A wall whose extent does not cover `I` is absent from that interval, so the other walls' ends behave as free ends there (and get end wrapping, if it applies).

**D. Priority painting (the cleanup core).** This step works on one node and one interval.
1. **Extend.** Each joining wall end is extended along its own direction until its bands reach the *far* face of the farthest other wall in the node. Bands are kept as 2D regions.
2. **Cells.** In the overlap zone, compute the arrangement of all band boundaries. Each **cell** is the intersection of one band from the arriving wall `A` with one band from the other wall `B`. For three or four walls, cells are intersections of bands from every wall that covers that point.
3. **Reach rule.** Band `a` of the arriving wall `A` *reaches* cell `(a, b)` when two conditions hold:
   - every band of `B` that lies between B's face on A's side and band `b` (exclusive) has a priority *number* strictly greater than `p_a` (that is, `a` is stronger than each one); and
   - `p_b ≥ p_a`.

   **Core override:** if `a` is in the core and `b` is not, `a` reaches `b` whatever their priority numbers. If `b` is in the core and `a` is not, `a` never reaches beyond B's core face.
4. **Ownership.**
   - If exactly one of `a` and `b` reaches the cell, that band owns it.
   - If both reach the cell, which happens only when `p_a = p_b`: in an **L node** the cell is divided by its **miter diagonal**, the segment from the cell corner where the bands' outer boundaries meet to the corner where their inner boundaries meet. Each half goes to the band on its side. In a **T node** the continuous wall's band owns the cell, so the arriving band stops at the cell boundary.
   - If neither reaches the cell, the cell goes to the band of the wall that is first in the join order. This is what the user sees as the join order changing with *Next* and *Previous*.
5. **Join type adjustments:**
   - **Butt.** Steps 1–4 as written.
   - **Miter.** Allowed only for L nodes of two walls. Step 4 is replaced: every cell is split by the **node miter line**, which joins the intersection of the two exterior faces to the intersection of the two interior faces. Each wall owns the side that faces its own body. Priorities are ignored.
   - **Square off.** Allowed for L nodes. The first wall in the join order is extended to the far face of the second wall and owns every cell. The second wall is trimmed at the first wall's near face. No layer interpenetration takes place.
   - Slanted walls allow only Butt. A wall abutting the interior face of another (a T node) allows only Butt.
6. **Trim.** Every part of an extended band that lies beyond the cells it owns is discarded. The final plan region of band `a` is its original strip, plus the cells it owns, minus the cells owned by other bands.
7. **Merge for display.** Adjacent regions from different walls are merged into one display region when they have the same material and the view's *Wall Join Display* allows it. Clean all wall joins merges always. *Clean same type wall joins* merges only when both walls have the same type. The *Display* override on the node takes precedence. Merging affects plan cut graphics only and never the solids.
8. **Multi-wall nodes (3–4 walls).** Walls are processed in join order. The first two form an L or T resolution. Each later wall is then resolved as a T into the combined footprint of the walls already processed, and its bands use the priority and core rules against whichever band of the combined footprint they meet.

**E. 3D realisation.** For vertical walls, each band region of each interval is extruded from `z₀` to `z₁` (`BRepPrimAPI_MakePrism`). The pieces of one layer are fused (`BRepAlgoAPI_Fuse` with `SetRunParallel(true)` and the fuzzy value, followed by `ShapeUpgrade_UnifySameDomain` to remove the interval seams). Slanted and tapered walls use **termination surfaces** in place of 2D painting:
- The termination of band `a` is the layer interface surface of wall `B` at which step D's reach rule stops it. That surface is a ruled plane when B is slanted.
- The band's swept solid is split with `BRepAlgoAPI_Splitter`, using that surface (limited to B's vertical extent) as the tool, and the piece on A's side is kept.
- Above or below B's extent, the band ends at its own end plane.

**F. Interaction with other operations.**
- Attach clipping (ARC-WALL-008) is applied *after* the join, to each wall's own solid.
- Profile edits (ARC-WALL-009) are applied before the join, as a prism mask in the wall's plane. They therefore change the breakpoints in C.
- Inserts and wrapping (ARC-WRAP-001) are applied last.
- The R-tree, detection and painting results are cached per node. A node is marked dirty when any of its walls changes curve, type, height, flip, location line or join override.

**Regeneration.** Each node is a graph node with `join` edges to and from its walls. When a node is re-solved, every wall in it regenerates, and nodes that share no wall regenerate in parallel (rayon). A wall end that moves can change node membership. In that case detection runs again for the old node and the new node in the same transaction.

**Data (IFC mapping).**
- Each pairwise connection is an **`IfcRelConnectsPathElements`** with `RelatingConnectionType` and `RelatedConnectionType` ∈ {`ATSTART`, `ATEND`, `ATPATH`} (L: ATSTART/ATEND pairs; T: ATEND to ATPATH).
- `RelatingPriorities` and `RelatedPriorities` list the layer indices of each wall that pass through, in IFC layer order, as the IFC definition expects. They are computed from step D for the interval that contains the walls' common base.
- The join type, join order and display override of each node are stored in RXD document `model/constraints`. Allow/disallow per end is owned by the wall and stored in its RXD element data (`recipe.wall`).
- The resolved body is in each wall's `Body` representation.

**Commands.**
- `wall.join_set_type(node_ref | wall_end_refs, type: Butt|Miter|SquareOff)`
- `wall.join_cycle_order(node_ref, step: +1|-1)`
- `wall.join_set_display(node_ref, display: ViewSetting|Clean|DontClean)`
- `wall.set_join_allowed(wall_id, end: Start|End, allowed: bool)`
- `wall.joins_query(wall_id) -> [JoinNode]` *(query)*

**Acceptance criteria.**
- [ ] **Figure-8 test from the reference behaviour.** Wall A is a CMU priority-1 core with insulation (3), an air gap (3) and brick (4). It meets wall B, a stud priority-1 core with finishes. A's CMU passes through B's finish layers and stops at B's core. A's insulation stops at B's air gap. A plan cut matches the ownership diagram in `screenshots/architecture/compound_layer_priority_joins.png`.
- [ ] Two identical walls in an L corner, with Butt, show continuous exterior finish around the corner with one miter-diagonal seam, and no line between same-material cores when *Clean all wall joins* is set.
- [ ] Miter on an L of a 300 mm wall and a 200 mm wall places the seam along the line joining the outer-face intersection and the inner-face intersection.
- [ ] Disallowing the join at one end leaves that wall ending square at its location point, with a visible gap.
- [ ] A T join of a 100 mm partition into a 300 mm external wall stops the partition's finishes at the external wall's interior finish, and the partition's core at the external wall's core face.
- [ ] Walls of different heights: above the shorter wall's top, the taller wall's layers end as a free end. They do not extend into empty space.
- [ ] Regenerating 1 000 independent L corners runs on at least four cores at once. Measured by a regeneration benchmark, the speed-up over single-threaded is at least 2.5×.

### 3.3.13 Attach Top/Base and Detach
**ID:** ARC-WALL-008 · **Priority:** P0 · **Ref:** research/02 §4.8, §7.1, §7.4

**Purpose.** A wall's top or base follows a roof, floor, ceiling, reference plane or another wall.

**Workflow.**
1. The user selects one or more walls and chooses Modify Wall › **Attach Top/Base**.
2. The user chooses Top or Base on the contextual tab.
3. The user picks the target.
4. **Detach Top/Base** works the same way: the user picks targets to detach, or uses **Detach All**.

**Behaviour & rules.**
- **Valid targets:** floors, roofs (including extrusion roofs), ceilings, non-vertical reference planes, the walls directly above or below, in-place floors and roofs, toposolids (base only), and elements in linked models (attach base only, read-only).
- **Top attach.** Let `S` be the target's solid. The wall body is clipped by the *upward shadow* of the target's bottom faces:
  `Tool = Prism(BottomFaces(S) ∩ WallFootprintXY, +Z, ∞) ∪ S`, and `Wall := Wall − Tool`.
  Reviera uses the underside faces (those whose normal has Z < 0) and extrudes them up to the scene's bounding box (`BRepPrimAPI_MakePrism` with a finite vector).
  - Only the part of the wall under the target is affected. Partial overlap therefore attaches only the overlapped part, and the rest keeps its own top constraint.
  - A reference plane counts as an infinite half-space.
- **Base attach** works the same way with the downward shadow of the target's top faces.
- **Height before clipping.** The wall's nominal top is raised to the maximum height of the target over the wall's footprint, so that the wall can grow up to a high ridge. The *Top Is Attached* flag then becomes true. The nominal top and the stored constraint are kept for detaching.
- **Slanted walls.** The target must cover the wall's whole plan projection. Otherwise Reviera reports "Attach target does not cover the slanted wall".
- **Reference planes.** Attaching to a second reference plane on the same end replaces the first.
- Extendable layers are disabled while the wall is attached.
- After a floor or roof is finished, Reviera offers to attach the walls directly below to it (ARC-FLR-001, ARC-ROOF-001).

**Regeneration.** An `attach` edge runs from the target to the wall. A cycle (for example, a roof whose footprint is locked to a wall that is attached to the same roof) is permitted because the footprint lock reads only the wall's **plan** curve, which does not depend on the attachment. The graph splits the wall node into a *plan* stage and a *body* stage so that no real cycle exists (§3.15).

**Data (IFC mapping).** An `IfcRelConnectsElements` with `Name = "AttachTop"` or `"AttachBase"` (RelatingElement = target, RelatedElement = wall). The clipped body is written as an `IfcBooleanClippingResult` when the target is planar (with an `IfcHalfSpaceSolid` or `IfcPolygonalBoundedHalfSpace`), and as a Brep otherwise. The attach relation is also stored in RXD document `model/constraints`, which is authoritative.

**Implementation notes (2026-10-13).** As built, tops attach to Reviera roofs. The wall's footprint is split by the roof's faces (each piece's top on its face's plane), and the roof must cover the whole footprint (`wall.attach_not_covered`). The body is a faceted B-rep in the wall's own placement frame, so openings placed relative to the wall keep their place. The attach target is stored in the wall's recipe (`attach_top`) until `model/constraints` exists, and the `IfcRelConnectsElements` comes later. The viewport draws the wall cut by its openings up to the roof's lowest point over it, with the part above as a separate solid. Deleting the roof detaches its walls.

**Commands.**
- `wall.attach(wall_ids, end: Top|Base, target_id)`
- `wall.detach(wall_ids, end: Top|Base, target_ids? )` (no targets means Detach All)

**Acceptance criteria.**
- [ ] Walls attached to a 30° gable roof follow the roof underside, giving a triangular gable-end wall profile.
- [ ] A wall half under a floor is clipped only under the floor, and the other half keeps its unconnected height.
- [ ] Moving the roof up 500 mm raises the attached wall tops by 500 mm.
- [ ] Detaching restores the original top constraint.

### 3.3.14 Edit Profile (elevation profile)
**ID:** ARC-WALL-009 · **Priority:** P1 · **Ref:** research/02 §4.9 · screenshots/architecture/wall_profile_mid_end_faces.png

**Purpose.** Change a straight wall's elevation outline: gables, notches and holes.

**Workflow.**
1. The user selects a straight wall and chooses Mode › **Edit Profile**. In a plan view Reviera offers Go To View.
2. The wall outline appears as sketch curves, together with dashed reference planes that show the original rectangle: the base, the top and the two ends.
3. The user edits: deletes, splits or adds lines, draws arcs, and adds inner closed loops for holes.
4. **Finish** applies the profile. **Reset Profile** restores the rectangle.

**Behaviour & rules.**
- Sketch curves that snap to a reference plane are **locked** to it and move when the wall's height or length changes. Unlocking them frees them.
- Validation (ARC-SKCH-002) requires exactly one outer loop and any number of hole loops.
- **Geometry.** The profile `Π` (in the wall's local u–z plane) is turned into a mask solid `M = Prism(Π, normal, ±(width/2 + margin))`. The wall's layers are intersected with `M` *before* the join step, which uses the profile's z range for its breakpoints.
- **Mid-end faces.** Every vertical edge of `Π` that is not at the wall's two ends produces a vertical face, which is a join target (ARC-JOIN-001 A.7).
- Not available for arc or elliptical walls. Those use Wall Opening instead.

**Data (IFC mapping).** The body is written as an `IfcExtrudedAreaSolid` of the plan footprint, intersected (`IfcBooleanResult` INTERSECTION) with an `IfcExtrudedAreaSolid` of `Π`, or as a Brep. The profile sketch is stored in RXD element data (`sketch`, role `profile`).

**Commands.** `wall.edit_profile(wall_id, loops) `, `wall.reset_profile(wall_ids)`.

**Acceptance criteria.**
- [ ] A notch 1 m deep and 1 m high in the top corner creates a mid-end face, and a perpendicular wall drawn to that face joins it.
- [ ] Raising the wall's top constraint moves the locked top line, and the notch keeps its depth.

### 3.3.15 Wall sweeps and reveals
**ID:** ARC-WALL-010 · **Priority:** P1 · **Ref:** research/02 §4.5 (Sweeps/Reveals), §4.10

**Purpose.** Additive profiles (cornices, skirting) and subtractive profiles (grooves) along walls.

**Workflow (instance level).**
1. The user starts Architecture › Build › Wall › **Wall: Sweep** or **Wall: Reveal** in a 3D or elevation view.
2. The user chooses the type and, on the Placement panel, **Horizontal** or **Vertical**.
3. Hovering a wall previews the sweep, and a click places it. Reviera pre-selects the same height on adjacent joined walls so that a click continues the run.
4. **Restart** starts a separate run. Clicking empty space ends the tool.
5. Selected sweeps offer *Add/Remove Walls*, *Modify Returns* (Straight or Angled) and end drag handles.

**Behaviour & rules.**
- **Type parameters:** Profile (a profile family), Material, Cuts Wall, Cut by Inserts, Default Setback, and Subcategory.
- **Instance parameters:** Wall Offset, Level, Offset from Level, Length.
- **Type-level sweeps and reveals** (Edit Assembly): rows with Profile, Material, Distance, From (Top or Base), Side (Exterior or Interior), Offset, Flip, Setback, Cuts Wall and Cuttable. They run the full wall length and are part of the wall element.
- **Geometry.** The profile (a closed 2D wire from the profile family) is placed in the plane normal to the wall path at the start, with its origin at the wall face plus the offset, at height `z = level + offset`. It is swept along the face path (the location curve offset to the face) with `BRepOffsetAPI_MakePipeShell` (fixed binormal +Z for horizontal runs).
  - Across joined walls, the path is the face chain with miter corners: the shell is built per wall and trimmed by the bisector planes.
  - *Cuts Wall*: the sweep's solid is subtracted from the wall layers it overlaps (for example, to recess a cornice).
  - A **reveal** is always subtracted, and *Cut by Inserts* subtracts insert voids from the sweep.
  - Returns at the ends: **Straight** caps the end with a planar face. **Angled** sweeps the profile around a 90° turn into the wall face.
  - **Vertical** sweeps run from the wall base to the wall top at a distance along the wall.
- **Slanted walls.** Reveals are not allowed. Sweeps take the *Orientation* setting (Perpendicular to Face or Parallel to Ground).

**Data (IFC mapping).**
- **Instance sweep:** `IfcCovering` with `PredefinedType = MOLDING`, linked to the wall through `IfcRelCoversBldgElements`, and a body of `IfcFixedReferenceSweptAreaSolid` (profile = `IfcArbitraryClosedProfileDef`, directrix = the face path) or a Brep.
- **Reveal:** `IfcOpeningElement` with `PredefinedType = RECESS`, voiding the wall through `IfcRelVoidsElement`.
- **Type-level sweeps** are part of the wall body, and the row table is stored in RXD type data (`recipe.compound`).

**Commands.**
- `wallsweep.create(type_id, wall_ids, orientation: Horizontal|Vertical, offset_from_level | distance_along, side?) -> element_id`
- `wallreveal.create(...)` (same parameters as `wallsweep.create`)
- `wallsweep.add_walls(sweep_id, wall_ids)`
- `wallsweep.remove_walls(sweep_id, wall_ids)`
- `wallsweep.set_returns(sweep_id, end, mode: Straight|Angled)`
- `walltype.set_sweeps(type_id, rows)`
- `walltype.set_reveals(type_id, rows)`

**Acceptance criteria.**
- [ ] A cornice placed on one wall of a closed rectangle extends to all four walls on click, with mitered corners.
- [ ] A reveal 20 mm deep cuts every layer it reaches and appears as an `IfcOpeningElement` (RECESS).

### 3.3.16 Split, Split with Gap and horizontal split (walls)
**ID:** ARC-WALL-011 · **Priority:** P0 · **Ref:** research/02 §4.12, §13

**Purpose.** Divide one wall into two or more walls.

**Behaviour & rules.**
- **Split Element** (`SL`): a click at a point on the wall divides it there.
  - The two new walls keep the type, constraints and parameters. The original GlobalId stays with the part that contains the wall's start point, and the other part gets a new GlobalId.
  - The new ends are joined to each other with a butt join, so they look continuous.
  - Inserts go to whichever part contains their centre.
  - With *Delete Inner Segment* on, the user clicks two points and the middle segment is deleted.
- **Horizontal split.** In elevation or 3D, hovering near a vertical edge previews a horizontal split line, which snaps to levels. The lower wall gets *Top Constraint* = that level, or an unconnected height. The upper wall gets *Base Constraint* = that level, or a base offset. Stacked walls cannot be split horizontally.
- **Split with Gap.** The *Joint Gap* is between 1.6 mm and 305 mm. A click cuts out a gap of that width centred on the click. The two new ends get Disallow Join, and a locked dimension between them records the gap. To close the gap, the user unlocks the dimension, allows the joins and drags the ends together. A slanted wall cannot be split by a vertical plane.

**Commands.**
- `element.split(element_id, point, delete_inner?: point2?) -> [element_id]` (shared with ARC-MOD-010)
- `wall.split_horizontal(wall_id, z | level_id) -> [element_id]`
- `element.split_with_gap(element_id, point, gap) -> [element_id]`

**Acceptance criteria.**
- [ ] Splitting a 10 m wall at 4 m gives walls of 4 m and 6 m. A door at 2 m stays hosted on the first wall, and the IFC export contains the original GlobalId exactly once.
- [ ] Split with Gap of 20 mm leaves exactly 20 mm between the faces of the new ends, held by a locked dimension.

### 3.3.17 Wall openings
**ID:** ARC-WALL-012 · **Priority:** P0 · **Ref:** research/02 §4.11, §9

**Purpose.** A plain rectangular hole through a wall, without a door or window family. The full specification of the opening element is ARC-OPEN-003; this block states what the wall side of the relation does.

**Workflow.**
1. In an elevation, section or 3D view, the user chooses Modify | Walls › Modify Wall › **Wall Opening** (or Architecture › Opening › Wall) and picks a straight or arc wall.
2. The user drags a rectangle on the wall face. Two clicks set opposite corners; temporary dimensions show the distances to the wall ends and to the base level.
3. The opening is created at once. Selecting it shows drag controls on its four edges and temporary dimensions; it can be moved along the wall with Move or by dragging.

**Behaviour & rules.**
- **Valid hosts.** Basic walls, straight or arc, vertical or slanted. Stacked walls are cut through every sub-wall the rectangle covers. Curtain walls, elliptical walls and in-place walls are not valid hosts ("Wall Opening cannot be placed in this wall").
- **Extent.** The rectangle is limited to the wall's elevation outline. Dragging an edge past the wall end or top clips it to the outline, and the opening becomes a notch.
- **Wall faces.** The cut creates *mid-end faces*: the wall's layer structure is closed on the opening sides like a wall end, so finish layers wrap into it when the type's *Wrapping at Inserts* applies (ARC-WRAP-001). Dimensions and Align can reference those faces.
- **Following the wall.** The opening keeps its position along the wall (as a distance from the wall start) and its base and top constraints. Moving, lengthening or changing the type of the wall keeps the opening; if the wall becomes shorter than the opening's start position, the opening is deleted with a warning.
- **Copying and deleting.** Copying the wall copies its openings. Deleting the wall deletes them. Splitting the wall (ARC-WALL-011) moves the opening to the part that contains its centre, and an opening that straddles the split point is clipped on each part.
- **Rooms.** The opening does not connect rooms on either side, because room boundaries use the uncut wall body (ARC-ROOM-002).

**Data (IFC mapping).** An `IfcOpeningElement` (`PredefinedType = OPENING`) with an extruded rectangular body through the wall thickness, related through `IfcRelVoidsElement`, with no fill. The *u* range and constraints are stored in RXD element data (`recipe.opening`).

**Commands.** `opening.create_wall(wall_id, u0, u1, base: {level_id, offset}, top: Level{id,offset} | Unconnected(h)) -> element_id` (ARC-OPEN-003); `element.move` and `element.set_param` edit it afterwards.

**Acceptance criteria.**
- [ ] A 1.2 × 2.1 m wall opening in a plastered block wall shows the plaster returned into the reveal when the type wraps at inserts.
- [ ] Shortening the wall so that its end lies inside the opening turns the opening into a notch; shortening it past the opening's start deletes the opening and reports it.
- [ ] Rooms on both sides of a wall opening stay separate.

### 3.3.18 Embedded curtain walls
**ID:** ARC-WALL-013 · **Priority:** P1 · **Ref:** research/02 §4.13, §6.2

**Behaviour & rules.**
- A curtain wall whose type has **Automatically Embed** on, and which is drawn parallel to a basic wall (within `TOL_ANGLE`) with its location line within 152.4 mm (6") of that wall's location line and overlapping it in plan and in height, **embeds** in that wall. The host wall is then cut by the curtain wall's outline extruded through the host's thickness, as if it were an insert.
- **Nesting.** A curtain wall embedded in a curtain-wall panel of type wall is allowed.
- Editing the embedded wall's profile updates the host opening.
- The embedding is a `cut` edge from the curtain wall to the host.

**Data (IFC mapping).** An `IfcOpeningElement` voids the host (`IfcRelVoidsElement`), and the curtain wall fills it (`IfcRelFillsElement`).

**Commands.** No command of its own. Embedding is automatic on create or move. `geometry.cut` (ARC-MOD-016) forces a cut manually.

**Acceptance criteria.**
- [ ] Drawing a storefront along a brick wall cuts a matching opening, and moving the storefront moves the opening.

### 3.3.19 Walls by Room and by Segment
**ID:** ARC-WALL-014 · **Priority:** P1 · **Ref:** research/02 §1 (2026), §4.2 · screenshots/architecture/walls_by_room_by_segment.png

**Purpose.** Generate finish walls (linings) against existing walls, columns or room boundaries.

**Workflow.**
1. The user starts the Wall tool and chooses a lining type, then **Place By Segment** or **Place By Room**.
2. **By Segment.** Hovering a wall or column segment previews a new wall placed against the face on the cursor side. Tab toggles whether columns are included in the chain. A click creates the wall.
3. **By Room.** Hovering a room or any closed area previews walls along its whole boundary. A click creates them all.
4. Spacebar, before the click, flips which side of the new wall faces the target.

**Behaviour & rules.**
- Only targets cut by the plan view's level are eligible. Curtain walls, ellipse walls, face-based walls, in-place walls and room separation lines are not.
- Targets in linked models are allowed. The target does not need to be room-bounding.
- The new wall's location line is set to the finish face that touches the target. Its base and top come from the *New Walls* properties.
- Each boundary segment produces one wall. Consecutive walls are joined to each other. The new wall is joined to the target (Auto Join, ARC-WALL-015) only when it is a hosted wall or when Auto Join is on.
- With **Hosted Wall** on, every new wall is hosted on its target (ARC-WALL-016). Structural walls and curtain walls cannot be hosts *during* this creation, but can be picked as hosts afterwards.
- **Boundary source.** By Room uses the room boundary computed at the room's computation height (ARC-ROOM-002), at the finish-face rule, regardless of the project's area rule.

**Commands.**
- `wall.create_by_segment(type_id, target_ref, side: Left|Right, include_columns?: bool, hosted?: bool, base_level?, top_constraint?, ...) -> [element_id]`
- `wall.create_by_room(type_id, room_id | point, flip?: bool, hosted?: bool, ...) -> [element_id]`

**Acceptance criteria.**
- [ ] By Room in a rectangular room with a column in one corner creates a lining that wraps the column's two exposed faces when columns are included.
- [ ] By Segment against a wall in a linked model creates the lining in the host model, with no hosting relationship.

### 3.3.20 Auto Join and Auto Join & Lock
**ID:** ARC-WALL-015 · **Priority:** P1 · **Ref:** research/02 §1 (2025), §4.2

**Behaviour & rules.**
- When a new wall overlaps an existing parallel wall face to face (for example, a lining placed against a core wall), **Auto Join** creates a Join Geometry relation between them. Inserts placed in either wall then cut both.
- **Auto Join & Lock** also creates an alignment-lock constraint between the touching faces, so the two walls move together.
- Neither option applies to structural, face-based, stacked or curtain walls, or to Pick Faces placement.
- Slanted walls get Auto Join only, never the lock.
- Inserts with complex cuts (non-prismatic voids) cut only the first wall.

**Data (IFC mapping).** `IfcRelConnectsElements` (Name "JoinGeometry"), and the lock in RXD document `model/constraints`.

**Commands.** The option is passed to `wall.create(type_id, curve, base_level, top_constraint: Level(id) | Unconnected(height), base_offset?, top_offset?, location_line?, flip?: bool, structural?: bool, structural_usage?, room_bounding?: bool, join_status?: Allow|Disallow, auto_join?: Off|Join|JoinAndLock) -> element_id`. Afterward, `geometry.join` and `constraint.lock_alignment` apply.

**Acceptance criteria.**
- [ ] A door placed in a core wall that was auto-joined to a lining cuts both walls.

### 3.3.21 Hosted walls
**ID:** ARC-WALL-016 · **Priority:** P1 · **Ref:** research/02 §1 (2027), §4.13

**Purpose.** A lining or finish wall that follows its host wall.

**Behaviour & rules.**
- A hosted wall stores `(host_wall, side, offset_from_host ≥ 0)`.
- Its location curve is recomputed from the host's face curve on that side, offset by `offset_from_host`. It therefore follows the host's movement, rotation and changes to the host's cross-section (slanted or tapered).
- It follows the host even when it is pinned.
- If the offset is ≤ 152.4 mm and Auto Join is on, inserts in the host also cut the hosted wall.
- Linked walls cannot be hosts.
- A host cannot be aligned to its own hosted wall, because that would be a cycle and the command is rejected.
- **Commands on the contextual tab:** *Select Main Host*, *Remove Hosted Relationship* (the wall becomes free and stays in place) and *Pick New Host*.
- The read-only *Hosted* parameter is filterable and schedulable ("Walls Hosted = Yes").

**Regeneration.** A `host` edge from the host wall's *plan* stage to the hosted wall.

**Data (IFC mapping).** `IfcRelConnectsElements` (Name "HostedWall", Relating = host). The relation and offset are owned by the hosted wall and stored in its RXD element data (`recipe.wall`), and *Hosted* is written as a property value.

**Commands.**
- `wall.set_host(wall_id, host_wall_id, side, offset)`
- `wall.remove_host(wall_ids)`
- `wall.select_main_host(wall_id) -> element_id` *(query)*

**Acceptance criteria.**
- [ ] Rotating the host 15° rotates a hosted lining with it and keeps the offset.
- [ ] Removing the relationship leaves the lining exactly in place.

### 3.3.22 Other wall behaviours
**ID:** ARC-WALL-017 · **Priority:** P0 · **Ref:** research/02 §4.13

**Purpose.** Smaller wall behaviours that do not need a block of their own.

| Behaviour | Rule |
|---|---|
| Flip arrows | Same as `wall.flip`: mirrors the wall about its location line. The layer order is reversed, so the exterior face moves to the other side; the location line, length, height and joins stay. When the location line is not the wall centreline, the body moves across it (for example, *Finish Face: Exterior* stays put and the wall swings to its other side). Hosted doors and windows keep their world position and their facing and hand; their voids are recomputed for the new layer order. Room boundaries at the finish rule move with the body. Flipping is refused on a pinned wall and on a wall in a group instance outside the group editor (ARC-MOD-000). Elliptical walls have no flip control. See also ARC-FLIP-001 |
| Spacebar | With walls selected, Spacebar flips them as the flip arrows do (ARC-KEY-001). During placement it flips the side of the wall that is drawn as exterior |
| Change type | Keeps the location line (ARC-WALL-002). The joins are re-solved for the new thickness, inserts stay hosted (an insert whose void no longer fits reports "Cannot cut instance out of wall" and stays), and dimensions to faces that still exist (matched by face role: exterior finish, core face, centreline) stay attached. Dimensions to faces that no longer exist are deleted with a warning. Changing to a curtain or stacked type is allowed only when the wall has no profile edits that the new kind cannot represent |
| Moves With Nearby Elements | A component or line whose *Moves With Nearby Elements* is on, and which lies parallel to a wall within 300 mm, keeps its perpendicular distance when the wall moves. The relation is stored in RXD document `model/constraints` and re-detected on each move. Only straight walls and straight model lines or component reference lines take part; moving the wall along its own direction does not move the follower. The follower moves even if it is pinned (ARC-MOD-000) |
| Disjoin on move | ARC-MOD-004: moving a wall with *Disjoin* breaks its end joins first, so the joined walls do not stretch |

**Data (IFC mapping).** A flip changes `IfcMaterialLayerSetUsage.DirectionSense` (and `OffsetFromReferenceLine` when the location line is not the centreline) and the explicit body. The *Moves With Nearby Elements* flag is a property value on the follower.

**Commands.** `wall.flip(wall_ids)`, `element.set_type(element_ids, type_id)` and `element.set_param(element_ids, "MovesWithNearbyElements", true)`; `element.flip` (ARC-FLIP-001) dispatches to `wall.flip` for walls.

**Acceptance criteria.**
- [ ] Flipping a wall drawn on *Finish Face: Exterior* keeps that face's line in place and reverses the layers; a door in the wall keeps its swing side.
- [ ] Changing a 200 mm wall to a 300 mm type with the location line on the core centreline keeps the centreline and re-cleans both end joins.
- [ ] A model line 200 mm from a wall, with Moves With Nearby Elements on, stays 200 mm away when the wall is moved 1 m perpendicular to itself, and does not move when the wall is moved along its length.

---

## 3.4 Doors, windows, components and architectural columns

### 3.4.1 Hosting model for loadable families
**ID:** ARC-HOST-001 · **Priority:** P0 · **Ref:** research/02 §5.1, §17.5

**Purpose.** One hosting model for every loadable family.

| Hosting kind | Valid hosts | Placement behaviour |
|---|---|---|
| Wall-hosted | Basic and stacked walls; curtain-wall panels only after the panel has been swapped to a wall type | Hovering a wall shows a preview; the family's opening void cuts the host |
| Floor-, ceiling- or roof-hosted | That category | Hovering a host of that category shows a preview |
| Face-based | Any planar or curved face, including linked and imported faces | Placement panel: *Place on Vertical Face*, *Place on Face* or *Place on Work Plane* |
| Work-plane-based | The active work plane | *Place on Work Plane* |
| Level-based (non-hosted) | A level plus an offset. The element may rest on the infinite plane of a floor's top face | A click places it; Spacebar rotates |
| Line-based | A work plane | Two clicks define the length |

**Behaviour & rules.**
- **Cutting.** A wall-hosted family carries an *opening cut* (a void solid in the family's coordinate system).
  - On placement, Reviera transforms the void into the host's coordinates and creates an `IfcOpeningElement` whose body is the void, clipped to the host's thickness plus 1 mm on each side.
  - The host body is then cut (`BRepAlgoAPI_Cut`), and wrapping applies (ARC-WRAP-001).
  - If the void does not reach through the full host width (for example, a recess), the opening's PredefinedType is `RECESS`.
- **Constraint along the host.** An insert can only move along its host's surface and keeps its offset from the host face. Moving it off the host requires **Disjoin** (ARC-MOD-004) or *Pick New Host*.
- **Rehosting.** *Pick New Host* (Modify | <category> › Host panel):
  - a level-based element can move to another level, floor or surface (in section or elevation);
  - a face-based or work-plane-based element can move to another face or plane.
- **Host deletion.** Deleting a host deletes its hosted inserts. Face-based elements on a deleted face become *orphaned*: they stay where they are, report "Lost host" and can be rehosted.
- **Joined walls.** An insert in a wall that is joined (Join Geometry or Auto Join) to a parallel wall also cuts that wall when its void reaches it.
- **Slanted walls.** Inserts keep their *Orientation* setting (Vertical or Slanted). Slanted orientation tilts the family to match the wall angle.

**Regeneration.**
- Edges into the insert: `host` (the host's plan and body stages) and `datum` (the level).
- Edge out of the insert: `cut` to the host body (and to joined walls).
- The insert's void is computed from the insert alone, so the host never waits on the insert's full geometry. This avoids a cycle.

**Data (IFC mapping).** The host is related to its opening through `IfcRelVoidsElement` (RelatingBuildingElement = host, RelatedOpeningElement = opening). The opening is related to the insert through `IfcRelFillsElement`. The insert is contained in the host's storey. For face-based elements, the host is recorded in RXD element data (`placement`), and in IFC through `IfcRelConnectsElements` (Name "HostedBy").

**Commands.**
- `element.pick_new_host(element_ids, host_ref, placement?: VerticalFace|Face|WorkPlane)`
- `element.host_query(element_id) -> host_ref` *(query)*

**Acceptance criteria.**
- [ ] A door moved along its wall stays in the wall. When the wall moves, the door moves with it.
- [ ] Deleting the host wall deletes its doors after a warning that gives the count.
- [ ] Each wall-hosted insert exports an `IfcOpeningElement` with matching void and fill relationships, and an IFC checker (IfcOpenShell validate) reports no errors.

### 3.4.2 Doors
**ID:** ARC-DOOR-001 · **Priority:** P0 · **Ref:** research/02 §5.2

**Purpose.** Place door families in walls.

**Workflow.**
1. The user starts Architecture › Build › **Door** (`DR`). It works in plan, section, elevation and 3D views.
2. The user picks a type, or uses *Load Family*.
3. Optionally, the user turns on **Tag on Placement** (Tag panel) and sets orientation (Horizontal or Vertical), *Leader*, leader length and *Tags…*.
4. Hovering a wall shows a preview with temporary dimensions (ARC-DIM-001). Moving the cursor toward one face of the wall or the other sets the facing (the swing side). **Spacebar** flips the hand.
5. A click places the door, and the tool repeats until Esc.

**UI.** Selected doors show *Flip hand* (horizontal arrows) and *Flip facing* (vertical arrows) controls, which are also in the right-click menu. Which controls appear depends on the family's flip settings.

**Behaviour & rules.**
- **Instance parameters:** Level, Sill Height (changing it moves the door; it does not resize it), Head Height (= sill + height), Orientation (slanted walls), Frame Type, Frame Material, Finish, Comments, **Mark**, phasing, **From Room** and **To Room** (RO).
- **Mark.** Each new door gets the next Mark in its category ("1", "2", … or the last Mark's increment).
- **Type parameters:** Width, Height, Thickness, Rough Width, Rough Height, Function (Interior or Exterior), Wall Closure, Construction Type, materials, Fire Rating, Type Mark, Operation and Cost.
- **From/To Room.** Two points are sampled 0.3 m apart along the facing normal from the family's *room calculation point*, at the room's computation height. Each point is located in the room set of the door's phase (ARC-ROOM-002). *To Room* is on the facing side. The *Flip* commands swap the two values. Both values are recomputed whenever rooms or doors regenerate.
- **Edge cases:**
  - A door cannot be placed in a curtain panel (use a curtain-wall door, ARC-CW-006).
  - A door wider than its host segment gives "Cannot cut instance out of wall": the door stays and its void is not applied.
  - A door in one of a pair of parallel walls cuts the other only when the walls are joined.

**Data (IFC mapping).**
- **`IfcDoor`** with `OverallHeight`, `OverallWidth`, `PredefinedType = DOOR` (`GATE` or `TRAPDOOR` where the family says so) and `OperationType` from the family (for example `SINGLE_SWING_LEFT`), written on both the occurrence and the type (`IfcDoorType.OperationType`), with `UserDefinedOperationType` when the value is `USERDEFINED`.
- **`IfcDoorType`** with `IfcDoorLiningProperties` and `IfcDoorPanelProperties` derived from family parameters where it can map them.
- `Pset_DoorCommon` holds Reference (Type Mark), FireRating, IsExternal (Function), HandicapAccessible and similar where they are set.
- Hand and facing are expressed by the placement's mirrored axis. A mirrored instance has a negative determinant, and Reviera writes `IfcCartesianTransformationOperator3DnonUniform` in the mapped representation, so viewers show it correctly.
- From/To Room are written as `IfcRelSpaceBoundary` (for the door) and as property values.

**Commands.**
- `door.place(type_id, host_wall_id, point_along: f64 | point, sill_height?, flip_hand?: bool, flip_facing?: bool, tag?: {orientation, leader, leader_length}) -> element_id`
- `insert.flip_hand(element_ids)`
- `insert.flip_facing(element_ids)`

**Acceptance criteria.**
- [ ] Placing a door 900 mm wide with its centre 600 mm from a perpendicular wall face, using temporary-dimension settings set to Faces, gives a temporary dimension of exactly 600 mm.
- [ ] Flipping facing swaps From Room and To Room.
- [ ] Ten doors placed in sequence are numbered 1 to 10.

### 3.4.2a Door operations: double, revolving, sliding, storefront and automatic doors
**ID:** ARC-DOOR-002 · **Priority:** P1 · **Ref:** owner requirement (2026-10-14); Reviera's own design (Appendix A.2a); IFC4 `IfcDoorTypeOperationEnum`, `Pset_DoorCommon.HasDrive`; ADA 2010 404.1, 404.3

**Purpose.** The commonest door kinds beyond a single swinging leaf, as values of the built-in door type. *(Owner requests, 2026-10-14: "Door options need to include double-doors, turnstyle doors, and glass store style swing and automatic door options"; "Yes, revolving door. And automatic sliding door that includes the space needed for the sliding elements.")*

**Behaviour & rules.**
- **Operation** (type parameter): *Single Swing* (today's doors); *Double Swing*, two leaves hinged at both jambs meeting in the middle (a pair; round-top pairs split the arched leaf); *Revolving*, four wings in a cross on a centre post inside a drum whose diameter is the door's width, with curved glass enclosures across the wall's line on both sides of the passage and a canopy; *Sliding (Bi-parting)*, two leaves riding just off the wall face on the side away from the facing side, lapping 50 mm past each jamb. Revolving and sliding doors are rectangles; arched heads take swing operations only.
- **Leaves** (type parameter): *Solid*, or *Glass (Storefront)*: 100 mm stiles and top rail, a 250 mm bottom rail, full glass between.
- **Automatic** (type parameter): a powered operator housing over the head on the leaves' side, 150 mm high and deep, across the leaves' travel; IFC `Pset_DoorCommon.HasDrive = TRUE` on the type.
- **The space a sliding door needs.** Open, each leaf slides past its jamb by its own width. The door takes that wall as well as its opening: fitting slides it along (or narrows it) until the leaves fit the wall's clear span and miss the other doors and windows at their height, and a placement that does not fit is refused ("the door and the wall its leaves slide over do not fit along its wall"). The wall beside the opening is a virtual volume (*slide space*, ARC-MOD-031): the leaves' travel each side, from the wall face out 100 mm, to the leaves' top, so anything placed against the wall there is reported.
- **Swing volumes.** Each swinging leaf has its own (a pair has two). A revolving door's drum is its volume. A sliding door has its slide space.
- **Maneuvering clearances** (ADA 2010) apply to manual swinging doors. Automatic doors, sliding and revolving doors have none of their own: ADA 404.3 covers automatic doors, and a revolving door needs an accessible door beside it (404.1).
- **Plans** draw each swinging leaf open with its arc, a sliding door's arrows along the wall to where its leaves go, and a revolving door's drum (its wings are cut). **3D** draws glass as glass and storefront stiles, operators, the post and canopy as metal.

**Data (IFC mapping).** `IfcDoor.OperationType` and `IfcDoorType.OperationType`: `SINGLE_SWING_LEFT`/`RIGHT`, `DOUBLE_DOOR_SINGLE_SWING`, `REVOLVING`, `DOUBLE_DOOR_SLIDING`. A revolving door's body is extruded plan profiles; the others are boxes. `Pset_DoorCommon.HasDrive` for automatic doors.

**Commands.** `type.set_param(type_id, "operation", single_swing|double_swing|revolving|sliding)`, `type.set_param(type_id, "panel", solid|glass)`, `type.set_param(type_id, "automatic", true|false)`; `door.place` and `window.place` find the host from `point` when `host_wall_id` is omitted (the one wall at that point; stacked walls must be named).

**Acceptance criteria.**
- [x] A 1.8 m automatic glass pair has two leaves of stiles, rails and glass, an operator, two swing volumes and no maneuvering clearances; its type says `HasDrive`. *(Built 2026-10-14.)*
- [x] A 2.2 m revolving door has a drum and wings and no frame boxes; it refuses an arched head; its volume is the drum.
- [x] A 1.8 m automatic sliding door at 4 m in a wall takes 2.2 m to 5.8 m of it; a window placed at 6.0 m is refused; placed 0.5 m from a wall's end it slides along until its leaves fit; it has two slide spaces.
- [x] The IFC carries `DOUBLE_DOOR_SINGLE_SWING`, `REVOLVING`, `DOUBLE_DOOR_SLIDING` and `HasDrive`.
- [x] The storefront sample (`test-data/samples/storefront.lua`) has all four in one front with no overlaps or intrusions.

### 3.4.3 Windows
**ID:** ARC-WIN-001 · **Priority:** P0 · **Ref:** research/02 §5.3

**Purpose.** Place window families in walls, and skylights in roofs.

**Workflow.** The user starts Architecture › Build › **Window** (`WN`). The same tag and temporary-dimension behaviour as doors applies. Spacebar has no effect; after placement, flip controls flip facing and hand where the family allows it.

**Behaviour & rules.**
- **Instance parameters:** Level, **Sill Height** (defaults to the type's Default Sill Height), **Head Height**, Orientation, Mark, Comments and phasing.
- **Type parameters:** Width, Height, Default Sill Height, Wall Closure, Window Inset, Construction Type and materials.
- Roof-hosted window families cut roofs at right angles to the roof face (a skylight).
- **Fit to the host** (owner requirement, 2026-10-13: "windows should be able to dynamically resize so they can fit on the selected wall"). Placing a window or door fits it to the wall where it lands, and the placement preview shows the fitted size as the cursor moves:
  - it slides inside the free gap (the wall's clear span, less the other inserts at its height) when the gap is wide enough, and otherwise shrinks to the gap;
  - it rises onto the wall's open face, 100 mm above the top of any roof the wall stands on (a dormer's front and cheek walls);
  - it shrinks to 50 mm under the roof the wall is attached to.
  - The fitted size is the insert's own Width and Height (instance values that override the type's), which Properties show and edit; clearing *Own Size* returns to the type's. Where not even the smallest size (the frame each side and 100 mm) fits, placement is refused.
- **Window Inset** moves the family's glazing plane in from the exterior face. It is applied as a translation along the wall normal before the void is computed.

**Data (IFC mapping).** **`IfcWindow`** (`PredefinedType = WINDOW`, `SKYLIGHT` for roof-hosted, `LIGHTDOME` where flagged) with `OverallHeight` and `OverallWidth`. **`IfcWindowType`** with `PartitioningType` from the family. `Pset_WindowCommon` holds IsExternal, FireRating, ThermalTransmittance and similar values. Openings and fills are as in ARC-HOST-001.

**Commands.** `window.place(type_id, host_id, point_along | point, sill_height?, flip_facing?, width?, height?, fit?) -> element_id`; `insert.set_size(element_ids, width?, height?, fit?)`; `insert.set_position(element_id, along | from_end)` places the centre a distance along the host from its start (or end), along the curve on a curved wall, refusing a place where it no longer fits (*built; Properties › Placement › From Wall Start / From Wall End*).

**Acceptance criteria.**
- [ ] Changing a window type's Default Sill Height does not move windows that are already placed. It changes only the default for new ones.
- [ ] A skylight in a 30° roof cuts an opening at right angles to the roof face.
- [x] The default 1.2 m window placed in a 1 m wall is 1 m wide; placed in a flat dormer's front wall it sits above the main roof and shrinks under the dormer roof.

### 3.4.3a Window shapes: round, round-top and other profiles
**ID:** ARC-WIN-002 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** Windows (and doors) that are not rectangles: round (oculus) windows, round-top (arched-head) windows, and the common head and outline shapes, as parametric built-in types. *(Owner request, 2026-10-13: "spec out more complex windows including round and with a round top".)*

**Workflow.** The user picks a window type whose **Shape** is not rectangular in the type selector (or duplicates a type and changes its Shape in Type Properties), then places it like any window (ARC-WIN-001), including the fit to the host.

**Behaviour & rules.**
- **Shapes** (type parameter *Shape*, with the shape's own parameters):
  - *Rectangle* (today's windows).
  - *Round* (circular, an oculus): Diameter is both Width and Height; an *Elliptical* variant has its own Width and Height.
  - *Round top* (arched head): a rectangle below a head arch. Head profiles:
    - *semicircular* (rise = Width / 2);
    - *segmental* (a Rise below Width / 2; radius from width and rise);
    - *elliptical* (a Rise; half-ellipse);
    - *Gothic / pointed* (two arcs meeting at the apex; a radius ratio of the span, default 1.0 for an equilateral arch);
    - *Tudor / four-centred* (two radii; a Rise).
    Height is the overall height to the top of the arch; Spring Height (the rectangle's height) is derived and shown.
  - *Quarter round / half round* (a fan light): the arch alone, with no rectangle below.
  - *Triangle / gable* (a pitched head, its Rise or angle) and *Trapezoid* (the left and right heights differ, for raking walls).
  - *Polygon* (an octagon or hexagon, with the number of sides and a rotation).
- **Frame and glazing follow the outline:** the frame is the outline offset inward by the frame width (circular arcs stay arcs). The sash and glass are inside it. Mullions and muntins (a radial "sunburst" option for round tops and fans) come from a pattern parameter.
- **Opening in the host:** the wall's void is the outline, not its bounding rectangle. It is extruded through the wall's thickness (`BRepPrimAPI_MakePrism` of the outline face, then `BRepAlgoAPI_Cut`). Plans cut at the cut plane show the opening's width at that height (a round window cut below its centre shows a narrower opening).
- **Fit to the host** (ARC-WIN-001) keeps the shape's proportions: a round window shrinks its diameter; a round-top keeps its head profile and gives up spring height first, then width.
- **Curved walls:** in a curved wall (ARC-WALL-002) the outline is projected onto the wall's surface, and the frame stays straight across (the wall bends around it) for openings up to the wall's chord tolerance.
- **Doors** take the round-top and Gothic heads too (arched doorways).
- **Display:** elevations and 3D show the true outline. Plans show the frame at the cut height, and the arch in the head dashed above the cut where the view shows it.

**Data (IFC mapping).** `IfcWindow` with `PartitioningType` and `UserDefinedPartitioningType` for shaped windows, and `IfcWindowType` carrying the shape's parameters in the Reviera property set. The opening's `IfcOpeningElement` body is an `IfcExtrudedAreaSolid` of an `IfcArbitraryClosedProfileDef` (an `IfcIndexedPolyCurve` with arc segments, or an `IfcCircleProfileDef` for round windows), so consumers see the true shape. The recipe is RXD type data (`recipe.window_type`, `shape`).

**Commands.** `type.set_param(type_id, "shape", rectangle|round|elliptical|round_top|fan|gable|trapezoid|polygon)` and the shape parameters (`rise`, `head_profile`, `radius_ratio`, `sides`, `rotation`, `left_height`, `right_height`, `muntins`), as for other type parameters. Placement is `window.place` / `door.place` unchanged.

**Acceptance criteria.**
- [x] A round window of 900 mm diameter cuts a circular opening in its wall (the void's volume is π · 0.45² · the wall's thickness within 0.5%) and exports with an `IfcCircleProfileDef` opening.
- [x] A 1.2 m-wide semicircular round-top window 2.0 m high has its spring at 1.4 m, and its frame follows the arch at a constant frame width.
- [x] A Gothic window with a radius ratio of 1.0 has two arcs of radius equal to its width meeting at the apex.
- [x] A plan cut at 1.2 m through a round window centred at 1.5 m with a 900 mm diameter shows an opening of 2 · √(0.45² − 0.3²) ≈ 0.67 m.
- [x] Fitted into a wall too narrow for it, a round window keeps its circle and shrinks its diameter.

*Built (2026-10-14):* every shape and head profile above, glazing bars (radiating or upright), round-top doors, the cut in straight and curved walls, fitting by proportion, Properties rows for the shape's values, and the IFC opening and body as extruded profiles (`IfcCircleProfileDef` for round windows; other outlines as `IfcPolyline`s, not yet `IfcIndexedPolyCurve` arcs). Not yet: the quarter-round fan, the head arch dashed above the cut in plans, and shaped types ready-made in the type list.

### 3.4.4 Components
**ID:** ARC-COMP-001 · **Priority:** P0 · **Ref:** research/02 §5.4

**Purpose.** Place any loadable family: furniture, fixtures, equipment, planting, specialty items and so on.

**Workflow.**
1. The user starts Architecture › Build › Component › **Place a Component** (`CM`). The tool is available only in views that suit the family (for example, plan or 3D for furniture).
2. The user picks a type, or uses Load Family. For face-based and work-plane-based families, the user picks a placement option; the last choice is remembered.
3. Spacebar cycles the rotation in 90° steps, or aligns the preview to the nearest wall. *Rotate after placement* (on the ribbon) asks for an angle after each click.
4. A click places the component.

**Behaviour & rules.**
- **Instance parameters** (typical): Level or Host, Offset or Elevation from Level, **Moves With Nearby Elements**, Work Plane, Schedule Level, Mark, Comments and phasing.
- The context menu offers *Pick New Host* (ARC-HOST-001) and *Edit Family* (DM chapter).
- With *Moves With Nearby Elements* on, Reviera records the nearest parallel wall within 300 mm at placement time, and the component keeps its offset from that wall (ARC-WALL-017).

**Data (IFC mapping).** The IFC entity is chosen from the family's category through the category mapping table (DM chapter). Examples: Furniture → `IfcFurniture`; Casework → `IfcFurniture` (`PredefinedType = SHELF` or USERDEFINED); Plumbing Fixtures → `IfcSanitaryTerminal`; Planting → `IfcGeographicElement` (4X3: `VEGETATION`); Generic Models → `IfcBuildingElementProxy`. The geometry is an `IfcMappedItem` of the type's `RepresentationMap`, with the family geometry built once per type.

**Commands.** `component.place(type_id, point | host_ref, rotation?: f64, level_id?, offset?, placement?: VerticalFace|Face|WorkPlane) -> element_id`.

**Acceptance criteria.**
- [ ] Three chairs placed with Spacebar pressed once each are rotated 0°, 90° and 180° relative to the first.
- [ ] Twenty instances of one type share a single `IfcRepresentationMap` in the IFC.

### 3.4.5 Architectural columns
**ID:** ARC-COL-001 · **Priority:** P0 · **Ref:** research/02 §5.5

**Purpose.** Non-structural columns: casings, pilasters and decorative columns.

**Workflow.**
1. The user starts Architecture › Build › Column › **Column: Architectural** in a plan or 3D view.
2. The user sets the options: *Rotate after placement*, *Level* (3D views only), *Height/Depth*, *Level/Unconnected* plus a value, and *Room Bounding*.
3. A click places the column. Its two centre reference planes can be aligned to grids or walls.

**Behaviour & rules.**
- An architectural column that is **joined to walls** takes the wall's material (the core material of the joined wall).
- The wall's wrapping layers wrap around the column faces that face away from the wall. Reviera treats the column's section as a mid-span obstacle and applies end-wrapping (ARC-WRAP-001) to the wall pieces that meet it.
- Structural columns behave differently and are covered in the STR chapter.
- With **Moves With Grids** on, a column at a grid intersection keeps its offsets from both grids.
- **Attach Top/Base** (Modify | Columns):
  - *Attach Column*: Top or Base.
  - **Attachment Style**: Cut Column, Cut Target or Do Not Cut.
  - **Attachment Justification**: Minimum Intersection, Intersect Column Midline or Maximum Intersection.
  - *Offset from Attachment*.
  - Valid targets: roofs, floors, ceilings, reference planes, structural framing, isolated foundations, foundation slabs and levels.
  - The column's top is moved to the height given by the justification: the lowest, middle or highest point where the target surface crosses the column's footprint. Then, if the style is Cut Column, the column is cut by the target's shadow (as in ARC-WALL-008). If the style is Cut Target, the target is cut by the column.
  - *Detach Top/Base* reverses the operation.
- **Instance parameters:** Base Level, Base Offset, Top Level, Top Offset, Moves With Grids, Room Bounding, Top/Base Is Attached (RO), Attachment Justification at Top/Base, Offset From Attachment at Top/Base, Comments, Mark and phasing.

**Data (IFC mapping).** `IfcColumn` with `PredefinedType = COLUMN` (`PILASTER` where the family is flagged) and `Pset_ColumnCommon.LoadBearing = FALSE`. A property value marks it as architectural. Attach relations are `IfcRelConnectsElements` ("AttachTop" or "AttachBase") and are recorded in RXD document `model/constraints`.

**Commands.**
- `column.place_architectural(type_id, point, base_level, top_constraint, base_offset?, top_offset?, rotation?, room_bounding?) -> element_id`
- `column.attach(column_ids[], end: top|base, target_id, style?, justification?, offset?) -> ()`
- `column.detach(column_ids[], end: top|base, target_ids?) -> ()`

**Acceptance criteria.**
- [x] A column attached at its top to a sloping roof with Maximum Intersection reaches the highest point where the roof crosses the column's footprint. With Minimum Intersection it reaches the lowest.
- [ ] A column joined to a plastered wall takes the plaster finish on its faces.

*Built (2026-10-15):* `column.place_architectural`, `column.set`, `column.attach` (top only; targets: Reviera roofs, floors and ceilings; Cut Column style: with Maximum and Midline the top is cut by the underside), `column.detach`; column types (`IfcColumnType` with a `recipe.column_type`: rectangular width × depth or round diameter) through `type.duplicate`, `type.set_param` and `element.set_type`. Columns move, rotate, mirror and copy, follow their levels and are deleted with them; an attach target that goes leaves the column at its own top. Architecture › **Column** places them in plans (Spacebar turns 90°), Properties edits constraints, rotation and type, and Modify | Columns › Attach Top / Detach Top attach to what is directly above. IFC: `IfcColumn` (`COLUMN`, `Pset_ColumnCommon.LoadBearing` false) with a faceted B-rep body, contained in its base level. Not yet: attaching bases, Cut Target and Do Not Cut styles, reference planes and levels as targets, wall materials and wrapping, Moves With Grids, Room Bounding.

### 3.4.6 Flip controls and mirrored state
**ID:** ARC-FLIP-001 · **Priority:** P0 · **Ref:** research/02 §2.4, §2.5, §4.13, §5.2, §5.3, §13 (Edge cases) · web: About Flip Controls; Flip Elements

**Purpose.** One set of rules for flipping elements in place (the paired-arrow controls, the context menu and the Spacebar) and for recording the resulting orientation, including mirroring, so that it survives IFC round trips.

**Workflow.**
1. The user selects one or more elements. Each element that supports flipping shows its flip controls (ARC-CTRL-001).
2. The user clicks a control, chooses **Flip Hand** or **Flip Facing** from the right-click menu, or presses **Spacebar**.
3. The element flips at once; each click or key press is one undo step.

**UI.**
- **Doors and windows** show *Flip hand* (horizontal paired arrows, at the insert's centreline on the facing side) and *Flip facing* (vertical paired arrows, across the wall). The family decides which of the two exist.
- **Walls** show one pair of arrows on the exterior side (ARC-WALL-017). Fascias, gutters and slab edges show horizontal and vertical arrows (ARC-SLED-001, ARC-ROOF-004). Stairs show their up/down arrow while being edited (ARC-STAIR-002).
- **Other family instances** show the flip controls defined in the family editor (DM chapter). Each control is **single** or **double** and **horizontal** or **vertical**: a single control turns the instance 180° about the vertical axis through the control's point; a double control mirrors the instance across the plane through the control's axis.
- The right-click menu lists the same actions with their names (*Flip Hand*, *Flip Facing*, *Flip Work Plane*, or the family control's label).

**Behaviour & rules.**
- **Definitions.** For a hosted insert, *facing* is the side of the host towards which the family's facing direction points (a door's swing side), and *hand* is the side of the family's centreline on which its hinge or handle lies. **Flip facing** reflects the instance across the vertical plane that contains the insertion point and is parallel to the host face; **flip hand** reflects it across the vertical plane through the insertion point at right angles to the host face. Doing both is equivalent to a 180° rotation.
- **Spacebar** on a selection: walls flip; doors and windows cycle through the four combinations in the order (facing, hand) = (0,0) → (1,0) → (1,1) → (0,1) → (0,0); free-standing components rotate 90° about their insertion point; work-plane-based instances with a *Flip Work Plane* control flip to the other side of their plane. If the selection mixes kinds that respond differently, each element does its own action. If an element cannot respond (no control, pinned, or locked by a constraint), nothing changes for it and the status bar says why.
- **Hosted constraints.** A flipped insert stays in its host at the same position along the host and the same sill height. Its opening void is recomputed; the host is cut again, and wrapping is reapplied. Door *From Room* and *To Room* swap when facing flips (ARC-DOOR-001).
- **References.** Dimensions and alignments attached to named family references (for example *Left*, *Right*, *Exterior*) stay attached to the same named reference, which may now lie in a different position; constraints are re-solved (ARC-CNST-002). If a locked dimension would change, the flip is refused with "Constraints are not satisfied".
- **Pins and groups.** A flip is a geometric modification: pinned elements and members of group instances outside the group editor are refused (ARC-MOD-000).
- **Mirrored state.** Flip hand and flip facing do **not** set *Mirrored*. *Mirrored* (read-only, schedulable, filterable) is true when the instance's net transform has been reflected an odd number of times by the Mirror tool (ARC-MOD-003) or by a family mirror (double) control. Mirroring a door or window with the Mirror tool is recorded as the matching flip when the mirror plane is parallel or perpendicular to the host face, **and** sets *Mirrored*, so a mirrored door can be found in a schedule even though its hand looks the same as a flipped one.
- **Elements without flip support.** In-place families, model text, groups and rooms have no flip control; `element.flip` reports "Element cannot be flipped" for them and changes nothing.

**Data (IFC mapping).**
- The flags `facing_flipped`, `hand_flipped` and `mirrored` (DM-INST-001) are stored in RXD element data (`placement`) and written as property values (*FacingFlipped*, *HandFlipped*, *Mirrored*) in the instance property set defined by the DM chapter.
- **Geometry.** IFC placements (`IfcAxis2Placement3D`) cannot reflect, so the net transform `T = placement · F_facing · F_hand · M` is split: when `det(T) > 0` (no flip, both flips, or two mirrors), the reflection pairs are folded into a rotation of the `ObjectPlacement`; when `det(T) < 0`, the `IfcMappedItem.MappingTarget` is an `IfcCartesianTransformationOperator3DnonUniform` with `Scale = −1` on the reflected axis (X for hand, Y for facing), and the placement carries the rest.
- **Doors and windows.** `IfcDoor.OperationType` swaps `…_LEFT` and `…_RIGHT` values when the effective hand changes (for example `SINGLE_SWING_LEFT` ↔ `SINGLE_SWING_RIGHT`); the type keeps its own value, and the occurrence value wins. The swing side is visible in the geometry and in the `IfcRelSpaceBoundary` of the door.
- **On import without RXD**, Reviera derives the flags from the mapping transform's determinant and axis signs and marks them as inferred.

**Commands.**
- `insert.flip_hand(element_ids)` and `insert.flip_facing(element_ids)` flip doors, windows and other hosted inserts that have the matching control.
- `element.flip(element_ids, control?: Facing|Hand|WorkPlane|Family(control_key)) -> ()` is the generic entry point used by the flip controls, the context menu and `ui.flip_selection` (UI chapter). Without `control`, walls call `wall.flip`, fascias, gutters and slab edges flip horizontally (`hostsweep.flip`), inserts flip facing, and family instances use their first family control. Elements that do not support the control are skipped and listed in the result warning.
- `element.flip_state(element_id) -> {facing_flipped, hand_flipped, mirrored, controls: [control_key]}` *(query)*.

**Acceptance criteria.**
- [ ] Pressing Space four times with a door selected visits four distinct facing/hand combinations and returns to the start; each press is one undo step.
- [ ] Flipping the facing of a door between two rooms swaps its From Room and To Room values and moves its swing to the other side of the wall without moving its centreline.
- [ ] A door flipped on both axes exports with a positive-determinant placement (no reflection operator); a door flipped on one axis exports an `IfcCartesianTransformationOperator3DnonUniform` with one negative scale, and IfcOpenShell renders it with the swing on the correct side.
- [ ] A right-hand door mirrored with the Mirror tool and a right-hand door changed with Flip Hand look identical, but only the mirrored one reports *Mirrored* = true in a schedule.
- [ ] `element.flip` on a pinned window changes nothing and returns "Element is pinned".

---

## 3.5 Curtain walls and curtain systems

### 3.5.1 Curtain element model
**ID:** ARC-CW-001 · **Priority:** P1 · **Ref:** research/02 §6.1 · screenshots/architecture/curtain_wall_rendered.png

**Purpose.** A curtain element is a surface divided by **grids** into cells. Each cell is filled by a **panel**, and **mullions** are swept along the grid segments and the borders.

**Behaviour & rules.**
- **Hosts.** Curtain walls (a wall kind), curtain systems (on faces) and sloped glazing (a roof type) all share one model:
  - a **curtain surface** (planar, or faceted-cylindrical for arc walls) with local axes U (along) and V (up), or Grid 1 and Grid 2 for systems and sloped glazing;
  - a set of **grid lines**, each with segments that can be turned off;
  - **cells**;
  - **panels**;
  - **mullions**.
- **Default types:** a plain type with no grids and no mullions; a glazing type with preset grids; and a storefront type with preset grids and mullions.
- **Grid lines.** Grid lines are either **type-driven** (generated by the layout rules and pinned) or **independent**, which happens when the user adds one or unpins one. Independent grid lines keep their absolute position when the wall is resized.
- **Panels** have one of these types:
  - a system panel (Glazed, Solid or Empty);
  - a loadable curtain-panel family;
  - any basic or stacked wall type;
  - a nested curtain wall type;
  - a curtain-wall door or window family.

**Data (IFC mapping).**
- **`IfcCurtainWall`** with an **`IfcCurtainWallType`** aggregates (`IfcRelAggregates`) its parts:
  - panels as **`IfcPlate`** (`PredefinedType = CURTAIN_PANEL`), or as `IfcWall`, `IfcDoor` or `IfcWindow` when the panel is one of those;
  - mullions as **`IfcMember`** (`PredefinedType = MULLION`).
- The `IfcCurtainWall` itself has an `Axis` representation (its base line) and no body.
- Grid lines (U/V positions, segment on/off, pin state, independent flag) are stored in RXD element data (`recipe.curtain_grid`). The type's layout rules are stored in RXD type data (`recipe.curtain_type`).

**Acceptance criteria.**
- [ ] A 6 m × 3 m curtain wall with three vertical grid lines and one horizontal grid line exports one `IfcCurtainWall` aggregating 8 `IfcPlate` and the matching `IfcMember` parts, and it opens in an independent viewer with every part present.

### 3.5.2 Creating curtain walls
**ID:** ARC-CW-002 · **Priority:** P1 · **Ref:** research/02 §6.2

**Workflow.** The user starts the Wall tool with a curtain-wall type. Line, Pick Lines and Pick Faces work, as do Spacebar and listening dimensions.

**Behaviour & rules.**
- **Arc curtain walls** are faceted at their vertical grid lines: each panel is a planar chord, and mullions sit on the arc. With no vertical grid lines, the panel is curved (a cylindrical panel) only if its type allows that. Otherwise Reviera adds a facet every 15° and warns.
- For non-rectangular outlines, the user can edit the profile (ARC-WALL-009) or attach the top (ARC-WALL-008). Cells that cross the outline are clipped to it, panels take the clipped shape, and border mullions follow the outline.
- Embedding is described in ARC-WALL-013.

**Commands.** `wall.create` with a curtain type. `curtain.set_profile(wall_id, loops)` is an alias of `wall.edit_profile`.

**Acceptance criteria.**
- [ ] An arc curtain wall with vertical grid lines every 1 m has planar panels whose corners lie on the arc.

### 3.5.3 Grid layout solver
**ID:** ARC-CW-003 · **Priority:** P1 · **Ref:** research/02 §6.3, §17.6 · screenshots/architecture/curtain_grid_layout_controls.png

**Purpose.** Compute type-driven grid positions from layout rules.

**Behaviour & rules.**
- **Inputs** for each direction (vertical grids along U; horizontal grids along V):
  - Layout `L` ∈ {None, Fixed Distance, Fixed Number, Maximum Spacing, Minimum Spacing};
  - Spacing `S`;
  - Number `n` (instance, 1–200);
  - **Justification** `J` ∈ {Beginning, Center, End} (instance);
  - **Angle** `θ` (instance);
  - **Offset** `o` (instance);
  - *Adjust for Mullion Size* (type).
- **Measurement frame.** Grid lines run at angle `θ` from the V axis (for vertical grids). Positions are measured along the direction perpendicular to the lines, **d**. The surface's extent along **d** is `[d_min, d_max]` (taken from the outline's corners), with length `D = d_max − d_min`. The justification origin `d₀` is `d_min` (Beginning), `(d_min + d_max)/2` (Center) or `d_max` (End), plus `o`.
- **Algorithm:**
  - *Fixed Distance*: let `r = D − ⌊D/S⌋·S` be the leftover. The lines are at `d_min + o + k·S` (Beginning), `d_max + o − k·S` (End) or `d_min + r/2 + o + k·S` (Center), for every integer k for which the line lies strictly inside `(d_min, d_max)` by more than `TOL_POINT`. Center justification therefore leaves equal leftovers at the two ends.
  - *Fixed Number*: `n` is the **number of interior grid lines**. Positions are `d_min + k·D/(n+1)` for k = 1…n. Justification and offset do not apply.
  - *Maximum Spacing*: panel count `m = ceil(D / S)`, positions `d_min + k·D/m` for k = 1…m−1.
  - *Minimum Spacing*: panel count `m = max(1, floor(D / S))`, positions `d_min + k·D/m`.
  - *None*: no type-driven lines.
- **Adjust for Mullion Size.** When it is on, the spacing applies to the **clear panel width**, measured between mullion faces, instead of between centrelines. Reviera solves for line positions `x_k` such that `x_{k+1} − x_k − (w_k + w_{k+1})/2 = S_clear`, using the actual widths of the interior mullions `w_k` and border mullions (`w_b`) for this layout. For Max/Min Spacing and Fixed Number, the effective length becomes `D − Σ mullion widths` before dividing.
- **Independent lines** keep their absolute positions. Type-driven lines that fall within 1 mm of an independent line are suppressed.
- The solver runs in `O(n)` and is a pure function of its inputs, so it can be unit-tested.

**UI.** **Configure Grid Layout** is an in-canvas control on a selected curtain face. It shows a justification-origin arrow for each direction (which the user can click to cycle Beginning, Center and End) and angle and offset fields for each direction.

**Commands.** `curtain.set_layout(element_id, direction: U|V, justification?, angle?, offset?, number?)` and `curtaintype.set_layout(type_id, direction, layout, spacing?, adjust_for_mullion?: bool)`.

**Acceptance criteria.**
- [ ] Fixed Distance 1.5 m on a 10 m wall: Beginning justification gives lines at 1.5, 3.0 … 9.0, with a 1.0 m remainder at the end. Center justification gives 6 full panels with 0.5 m left at each end.
- [ ] Maximum Spacing 1.2 m on a 10 m wall gives 9 equal panels of 1.111 m.
- [ ] With Adjust for Mullion Size on, 50 mm interior mullions, 100 mm border mullions and Fixed Distance 1.0 m, every clear panel width is 1.000 m.

### 3.5.4 Manual curtain grids
**ID:** ARC-CW-004 · **Priority:** P1 · **Ref:** research/02 §6.4

**Workflow.**
1. The user starts Architecture › Build › **Curtain Grid** in an elevation or 3D view.
2. The user picks a Placement mode: **All Segments**, **One Segment**, or **All Except Picked** (click segments to exclude, then Finish).
3. Hovering near a vertical edge previews a horizontal grid line, and hovering near a horizontal edge previews a vertical one. The preview snaps to the midpoints and thirds of panels, to visible levels, grids and reference planes, and to grid lines on curtain walls that share a corner. A click places the grid line.

**Behaviour & rules.**
- A selected grid line offers **Add/Remove Segments**: clicking a segment removes it, which merges the two adjacent cells into one panel, and clicking a dashed (removed) segment restores it.
- Temporary dimensions are shown to the neighbouring grid lines, and the line has a pin control.
- Manual lines are independent.

**Commands.**
- `curtain.add_grid(element_id, direction: U|V, position, mode: All|One(segment)|AllExcept([segment])) -> grid_key`
- `curtain.set_grid_segments(element_id, grid_key, segments_on: [bool])`
- `curtain.move_grid(element_id, grid_key, position)`
- `curtain.delete_grid(element_id, grid_key)`

**Acceptance criteria.**
- [ ] Removing the middle segment of a vertical grid line merges two panels into one panel of twice the height. Restoring the segment splits it again, and the original panel type is kept.

### 3.5.5 Mullions
**ID:** ARC-CW-005 · **Priority:** P1 · **Ref:** research/02 §6.4

**Workflow.** The user starts Architecture › Build › **Mullion** and picks a Placement mode: **Grid Line** (the whole line), **Grid Line Segment** or **All Grid Lines**. The same modes apply to border edges.

**Behaviour & rules.**
- **Mullion types:**
  - Rectangular: profile, thickness, width on side 1 and side 2, and offset;
  - Circular: radius;
  - Corner L: leg lengths and thickness;
  - Corner V;
  - Corner Trapezoid: centre width and depth;
  - Corner Quad: depth 1 and 2, which gives a kite, rectangle or square.
- **Custom profiles** come from profile families. Closed multi-loop profiles (for example, hollow sections) are accepted, and open loops are rejected.
- **Instance parameters:** Angle, Offset (locked when the mullion is type-driven), Length and Mark.
- **Generation:**
  1. A mullion is created for each grid segment that is on, and for each border edge that has a border mullion type.
  2. **Continuity at each grid intersection.** Precedence, highest first: a per-mullion override (**Make Continuous** or **Break at Join**); then the type's **Join Condition** (Vertical Grid Continuous, Horizontal Grid Continuous, Border and Vertical Continuous, Border and Horizontal Continuous; for systems and sloped glazing, Grid 1 or Grid 2 Continuous); and then, with Not Defined, the **vertical** direction.
  3. The continuous mullion runs through the intersection. The others are trimmed at the continuous mullion's faces: each end is cut by planes offset ±(width/2) from the continuous line.
  4. The geometry is a sweep of the profile along the segment's straight line (`BRepPrimAPI_MakePrism`, or `BRepOffsetAPI_MakePipe` on faceted arcs). The profile is centred on the line, offset by the mullion offset along the surface normal, and rotated by the Angle.
  5. **Corner mullions** sit at the shared vertical edge of two curtain walls that meet at angle φ. Their cross-section is built from φ and the type's dimensions (L: two legs along the two faces; V: two legs mirrored about the bisector; trapezoid and quad: the polygon from their parameters).
- Type-driven mullions are pinned. Deleting one requires unpinning it first.

**Commands.**
- `mullion.add(element_id, target: GridLine(key) | Segment(key, idx) | AllGridLines | Border(edge), type_id) -> [element_id]`
- `mullion.set_continuity(mullion_id, mode: Continuous|Break)`
- `curtaintype.set_mullions(type_id, interior_v, border1_v, border2_v, interior_h, border1_h, border2_h)`

**Acceptance criteria.**
- [ ] With Join Condition = Horizontal Grid Continuous, the horizontal mullions are unbroken and the vertical mullions stop at their faces.
- [ ] A Make Continuous override on one vertical mullion wins over the type setting at that mullion's intersections only.
- [ ] A hollow two-loop mullion profile produces a solid with an inner void.

### 3.5.6 Panels and curtain-wall doors
**ID:** ARC-CW-006 · **Priority:** P1 · **Ref:** research/02 §6.5

**Workflow.**
1. The user hovers a panel's edge and presses Tab until the panel is highlighted, then selects it.
2. If the panel is pinned, the user unpins it.
3. The user changes its type in the Type Selector.

**Behaviour & rules.**
- A panel can take any of the panel kinds listed in ARC-CW-001.
- A wall-type panel has *Categorize as* = Panel or Wall, which controls scheduling.
- Panels cannot be resized directly. Moving grid lines resizes them.
- Each cell polygon is used as follows:
  - **system panel**: a planar plate of the panel's thickness, offset along the surface normal;
  - **family panel**: an instance driven by width and height (the cell's bounding rectangle) and clipped to the cell if the cell is not rectangular;
  - **wall-type panel**: a wall whose location line is the cell's bottom edge and whose profile is the cell.
- **Curtain-wall door.** Only door families made for curtain walls can replace a panel. The door fills the cell. The sill mullion is not removed automatically: the user unpins and deletes it. Switching back to a panel type reverts the change. Curtain-wall doors are tagged and scheduled as doors.
- Inserts in wall-type panels are positioned relative to the **curtain wall** as a whole, so they do not follow the panel when grid lines move.

**Data (IFC mapping).** A panel is an `IfcPlate`, `IfcWall`, `IfcDoor` or `IfcWindow` aggregated in the curtain wall (ARC-CW-001).

**Commands.** `curtain.set_panel_type(panel_ids, type_id)`. Pin and unpin are covered by ARC-MOD-013.

**Acceptance criteria.**
- [ ] Swapping a glazed panel for a curtain-wall door turns that cell into an `IfcDoor` aggregated in the `IfcCurtainWall`, and it appears in the door schedule.

### 3.5.7 Curtain systems by face
**ID:** ARC-CW-007 · **Priority:** P2 · **Ref:** research/02 §6.6, §12.1

**Workflow.**
1. The user starts Massing & Site › Model by Face › **Curtain System by Face** and chooses a type.
2. With *Select Multiple* (on by default), the user clicks faces to add or remove them, or drags a pick box over a whole form. *Clear Selection* empties the set.
3. **Create System** builds the system.

**Behaviour & rules.**
- Each face gets its own UV grid (Grid 1 and Grid 2, each with angle and offset).
- Curved faces are tessellated at the grid lines into planar panels.
- A curtain system has no editable sketch. It updates only through *Update to Face* (ARC-MASS-003).
- Manual grid lines snap to boundary intersections and reference planes.

**Data (IFC mapping).** `IfcCurtainWall` with `ObjectType = "CurtainSystem"`, aggregating the same parts as a curtain wall.

**Commands.** `curtainsystem.create_by_face(type_id, face_refs) -> element_id`.

**Acceptance criteria.**
- [ ] A curtain system on the four faces of a mass box produces four grids whose corner mullions meet.

### 3.5.8 Sloped glazing
**ID:** ARC-CW-008 · **Priority:** P1 · **Ref:** research/02 §6.1, §7.4

**Purpose.** Glazed roofs and skylights built from the curtain-element model (grids, mullions and panels) on the planes of a footprint or extrusion roof.

**Workflow.**
1. The user creates a footprint roof (ARC-ROOF-001) or an extrusion roof (ARC-ROOF-002), or selects an existing one.
2. In the Type Selector the user picks a *Sloped Glazing* type. The roof becomes a set of glazed planes.
3. The user adds grids with Architecture › Build › **Curtain Grid** (Tab switches between Grid 1 and Grid 2 while placing), then mullions and panel changes as on a curtain wall.

**Behaviour & rules.**
- Changing a footprint or extrusion roof to a *Sloped Glazing* type replaces its body with a curtain surface on each roof plane.
- **Sketch kept.** The roof keeps its footprint or profile sketch, slopes, cutoff and joins. *Edit Footprint* / *Edit Profile* still work, and the glazing regenerates from the new planes.
- **Grid directions.** On each plane, **Grid 1** lines run parallel to the plane's eave (its lowest horizontal edge; for an extrusion roof, the extrusion direction) and **Grid 2** lines run up the slope, at right angles to Grid 1. Type layout rules (*Fixed Distance*, *Fixed Number*, *Maximum Spacing*, justification and angle, ARC-CW-003) are measured on the sloped plane, from the eave for Grid 1 and from the plane's justification point for Grid 2.
- Grids follow the Grid 1 and Grid 2 rules. When placing a grid line, Tab switches between the two directions.
- **Edges.** Hips, valleys and ridges are plane boundaries. The type's border mullions are placed on them once, shared by both planes and mitred to the bisector of the dihedral angle. Eaves and gables get the type's border mullions.
- Mullions and panels behave as described in ARC-CW-005 and ARC-CW-006.
- **Type parameters** are those of a curtain type (panel, join condition, grid layouts, interior and border mullions), plus the roof's *Rafter Cut* and *Fascia Depth*, which apply to the border mullions at the eave.
- **Changing back** to a basic roof type discards the grids, mullions and panel overrides after a warning that gives their count. The roof sketch is unchanged.
- **Hosting.** Sloped glazing is not a valid host for roof-hosted windows (use panels instead); walls can still attach to it (ARC-WALL-008), using its glazing plane.

**Data (IFC mapping).** An `IfcRoof` aggregating one `IfcCurtainWall` (`ObjectType = "SlopedGlazing"`) per roof plane, each with its own `IfcPlate` and `IfcMember` parts. The grid layout is stored in RXD element data (`recipe.curtain_grid`) per plane, keyed by the plane's sketch-edge sub-reference so it survives sketch edits that keep that edge.

**Commands.** `element.set_type(roof_id, sloped_glazing_type_id)` followed by the `curtain.*` commands.

**Acceptance criteria.**
- [ ] A hip roof changed to sloped glazing produces four glazed planes whose hip mullions sit on the hip lines.
- [ ] With Grid 1 at a fixed distance of 1.5 m, a 30° plane whose slope length is 6 m gets three interior Grid 1 lines measured along the slope.
- [ ] Changing the slope of one footprint edge from 30° to 45° regenerates that plane's grid and keeps manually added grid lines on the other planes.
- [ ] Changing the roof back to a basic type warns with the number of mullions and panels to be removed and keeps the footprint.

---

## 3.6 Floors, slab edges, ceilings, roofs, shape editing and parts

### 3.6.1 Floors
**ID:** ARC-FLR-001 · **Priority:** P0 · **Ref:** research/02 §7.1

**Purpose.** Horizontal or sloped layered slabs defined by a sketched boundary.

**Workflow.**
1. The user starts Architecture › Build › Floor › **Floor: Architectural** (the drop-down also offers Floor: Structural (`SB`), Floor by Face and Floor: Slab Edge).
2. In the sketch (Modify | Create Floor Boundary), the user draws:
   - **Boundary Line**: Pick Walls (default), lines and shapes, or Pick Lines;
   - **Slope Arrow**;
   - **Span Direction** (structural decks).

   Options: *Offset*, and **Extend into wall (to core)**.
3. Outer loops define the floor, and inner loops define holes. **Finish**.
4. If walls below end at this floor's level, Reviera asks whether to attach them to the floor's underside. If the floor overlaps walls, it asks whether to join geometry so that the floor cuts them.

**Behaviour & rules.**
- **Instance parameters:** Level, **Height Offset from Level**, Room Bounding, Related to Mass (RO), Structural, Curved Edge Condition (shape-edited), **Slope Angle** (adjusts the slope-defining line without editing the sketch), Perimeter, Area, Volume, Elevation at Top, Elevation at Bottom, Elevation at Core Top, Elevation at Core Bottom, Thickness (RO, or editable when shape-edited with a variable layer), Comments, Mark and phasing.
- **Type parameters:** Structure (ARC-CMPD-001), Default Thickness (RO), Function (Interior or Exterior), Coarse Scale Fill, Structural Material, analytical and thermal properties, and identity data.
- **Reference.** The *top* of the floor is at `level + offset`, and the layers stack downward.
- **Slopes.** A slope can come from a slope arrow, from two parallel boundary lines with different *Offset from Base* values, or from one line with *Defines Slope* and a *Slope* value. All three produce a tilted reference plane `P`. The top surface lies on `P`, with the pivot at the slope-defining line or the arrow's tail.
- **Geometry.** The boundary loops are mapped to a face on `P`. Each layer is a prism of thickness `t_j` along `−n_P`, where `n_P` is the normal of `P`, so the layers stay perpendicular to the sloped face (`BRepPrimAPI_MakePrism`, then fused per layer). If the floor is shape-edited, ARC-SHAPE-001 applies.
- **Cuts.** Floors cut walls with which they have Join Geometry. Shafts and openings cut floors (§3.8).

**Regeneration.**
- Edges into the floor: `datum` (level), `sketch-lock` (picked walls) and `type`.
- Edges out of the floor: `attach` (walls attached to it), `bound` (rooms above and below, for volume), `host` (floor-hosted families) and `cut` (Join Geometry pairs).

**Data (IFC mapping).**
- **`IfcSlab`** (`PredefinedType = FLOOR`; `BASESLAB` when the floor is structural and its function is foundation, STR chapter) with an **`IfcSlabType`**.
- `IfcMaterialLayerSetUsage` with `LayerSetDirection = AXIS3`, `DirectionSense = NEGATIVE` and `OffsetFromReferenceLine = 0`, so the layers hang down from the top.
- **Body.** An `IfcExtrudedAreaSolid` (profile `IfcArbitraryProfileDefWithVoids`, depth = thickness, `ExtrudedDirection = (0,0,−1)`) in a placement tilted to `P` for sloped floors, or an `IfcFacetedBrep` when the floor is shape-edited. When the floor has more than one outer loop, the body has one solid per loop.
- **Quantities.** `Qto_SlabBaseQuantities`: Width (thickness), Perimeter, GrossArea, NetArea, GrossVolume, NetVolume.
- **Properties.** `Pset_SlabCommon`: IsExternal, LoadBearing, PitchAngle.
- The sketch is stored in RXD element data (`sketch`).

**Commands.**
- `floor.create(type_id, loops, level_id, offset?, slope?: {line_key|curve, slope} | {arrow: SlopeArrow}, structural?, span_direction_edge?, room_bounding?, attach_walls_below?: bool, join_overlapping_walls?: bool) -> element_id` (one element per call, unless the caller passes `split_loops = true`)
- `floor.edit_boundary(floor_id, loops, slope?)`
- `floor.set_slope_angle(floor_id, angle)`

**Acceptance criteria.**
- [ ] A floor sketched with Pick Walls and *Extend into wall (to core)* on has its edges on the walls' core faces.
- [ ] A floor with a 1:50 slope arrow 10 m long rises 0.2 m from tail to head, and its layers are perpendicular to the slope.
- [ ] Answering Yes to the attach prompt attaches the walls below, and their tops follow the floor's underside.

### 3.6.2 Slab edges
**ID:** ARC-SLED-001 · **Priority:** P1 · **Ref:** research/02 §7.2

**Purpose.** Profiles swept along slab edges, such as thickened edges and curbs.

**Workflow.**
1. The user starts Floor › **Floor: Slab Edge** and hovers the horizontal edges of floors (or of toposolids and model lines). A click adds an edge. Edges picked one after another form one mitered run.
2. **Restart Slab Edge** starts a new run that is not mitered to the previous one. *Modify* ends the tool.

**Behaviour & rules.**
- **Selected-element tools:** *Add/Remove Segments*, horizontal and vertical flip arrows, and end drag handles.
- **Instance parameters:** Vertical Profile Offset, Horizontal Profile Offset, Angle, Length, Volume, Mark and phasing.
- **Type parameters:** Profile and Material.
- **Geometry.** The profile is placed on the edge's normal plane, with its origin on the edge and its X axis pointing away from the slab. It is offset and rotated by the instance values and swept along the edge chain (`BRepOffsetAPI_MakePipeShell`). Corners are mitered by bisector planes.
- The run cuts its host where the two overlap. This is the default join rule for host sweeps (ARC-MOD-017).

**Data (IFC mapping).** **`IfcSlab`** with `PredefinedType = USERDEFINED` and `ObjectType = "SLABEDGE"`, connected to its host through `IfcRelConnectsElements`. The body is an `IfcFixedReferenceSweptAreaSolid` or a Brep. See Open questions for the alternatives considered.

**Commands.**
- `slabedge.create(type_id, edge_refs: [ref], ...offsets) -> element_id`
- `hostsweep.add_segments(id, edge_refs)`
- `hostsweep.remove_segments(id, edge_refs)`
- `hostsweep.flip(id, axis: Horizontal|Vertical)`

**Acceptance criteria.**
- [ ] A slab edge picked along the four edges of a rectangular floor forms one closed, mitered run, and its volume equals the profile area times the centroid path length within 0.5%.

### 3.6.3 Ceilings
**ID:** ARC-CEIL-001 · **Priority:** P0 · **Ref:** research/02 §7.3

**Purpose.** Ceilings at a height above a level, placed automatically in a room or sketched.

**Workflow.**
1. The user starts Architecture › Build › **Ceiling** in a reflected ceiling plan.
2. **Automatic Ceiling** (default): hovering inside a closed loop of walls highlights the loop, and a click creates the ceiling. Room separation lines are ignored.
3. **Sketch Ceiling**: the user draws boundary lines (outer loop plus hole loops) and optionally a slope arrow, then presses Finish.

**Behaviour & rules.**
- **Automatic loop detection** uses the room-boundary arrangement (ARC-ROOM-002) at the ceiling height (`level + offset`). Only walls are used, with the area rule fixed to *at wall finish*. The arrangement face that contains the cursor becomes the boundary.
- **Slopes** work as for floors (slope arrow, two parallel lines with different offsets, or one line that defines slope). A ceiling slopes in one direction only.
- **Instance parameters:** Level, **Height Offset From Level**, Room Bounding, Slope, Perimeter, Area, Volume, Comments, Mark and phasing.
- **Types:** a *Basic Ceiling* (one material and a thickness, with no layers) or a *Compound Ceiling* (Edit Assembly).
- **Reference.** The ceiling's **bottom** face is at `level + offset`, and the layers stack upward.
- **Model pattern alignment.** The ceiling's surface pattern (a model pattern from its material) can be selected with Tab and then aligned, rotated or moved. Each face stores the pattern's origin and angle.

**Data (IFC mapping).** **`IfcCovering`** with `PredefinedType = CEILING` and an `IfcCoveringType`. The covering is related to the rooms it covers through `IfcRelCoversSpaces`, computed from the room footprint overlap. A compound ceiling has `IfcMaterialLayerSetUsage` (AXIS3, POSITIVE). `Qto_CoveringBaseQuantities` holds Width, GrossArea and NetArea.

**Commands.**
- `ceiling.create_auto(type_id, level_id, point, offset?) -> element_id`
- `ceiling.create(type_id, loops, level_id, offset?, slope?) -> element_id`
- `ceiling.edit_boundary(ceiling_id, loops, slope?)`
- `surfacepattern.align(element_id, face_ref, origin?, angle?)`

**Acceptance criteria.**
- [ ] Clicking inside a room enclosed by four walls creates a ceiling whose edges lie on the walls' interior finish faces.
- [ ] Each ceiling exports as `IfcCovering(CEILING)` with an `IfcRelCoversSpaces` link to the room it covers.

### 3.6.4 Roof by footprint
**ID:** ARC-ROOF-001 · **Priority:** P0 · **Ref:** research/02 §7.4 · screenshots/architecture/roof_footprint_sketch_mode.png · screenshots/architecture/roof_different_eave_heights_render.png

**Purpose.** Pitched, hipped, gabled and flat roofs generated from a plan footprint and per-edge slopes.

**Workflow.**
1. The user starts Architecture › Build › Roof › **Roof by Footprint** in a plan or reflected ceiling plan view. If the active level is the lowest level, Reviera offers to move the roof to the level above.
2. The user draws the footprint with Pick Walls (default), lines or shapes, and optionally slope arrows. Options: **Defines slope** (applies to each new line), **Overhang** (Pick Walls) and **Extend to wall core**.
3. The footprint must be one closed outer loop, with optional inner loops for openings. A triangle symbol marks each slope-defining edge. Edges that do not define slope become gables.
4. On **Finish**, Reviera offers to attach the **perimeter** walls under the roof, so that gable ends fill up to the roof and eave walls meet its underside (*gable infill*; see Behaviour & rules).

**Behaviour & rules.**
- **Footprint curve properties:** Defines Roof Slope, Slope, Overhang, Plate Offset from Base, Offset from Roof Base, Extend into wall (to core) and Length.
- **Roof shape presets** (instance property *Roof Shape*; `roof.set_shape`): Hip, Gable, Gable (rotated), Shed, Shed (reversed) and Flat set which footprint edges define slope, measured against the longest edge (edges within 10° of its direction count as along it). Courtyard edges follow the same rule. A roof whose edges match no preset shows *Custom*. One *Overhang* property sets every edge's overhang. *(Owner clarification, 2026-10-13: more controls for roof geometry, flat and gabled forms.)*
- **Gable infill** (*Attach Walls*, `roof.attach_walls`; offered on Finish whatever the footprint was drawn with, and on the Modify | Roofs tab):
  - It attaches the tops of the **perimeter** walls the roof covers whole and that start below it. Gable walls rise to the roof, and eave walls meet its underside.
  - **Interior walls are never attached automatically**; the user attaches them with Attach Top/Base (ARC-WALL-008).
  - A wall is on the perimeter when, beside one of its faces at its middle, there is no enclosed region of its level, or the region there reaches out from under the roof (a courtyard).
  - *(Owner clarification, 2026-10-13: automatic gable infill, perimeter walls only.)*
- **Editing the footprint on the model** (ARC-SKCH-001): the selected roof shows grips at the footprint's corners and edge middles. Dragging an edge moves it across itself, with its neighbours stretched or shortened to meet it; dragging a corner moves it. Each drag is one undoable edit (`sketch.move_curve`, `sketch.move_corner`).
- **Instance parameters:** Base Level, **Base Offset From Level**, Room Bounding, Related to Mass, **Cutoff Level**, **Cutoff Offset**, **Rafter Cut** (Plumb Cut, Two Cut – Plumb, Two Cut – Square), **Fascia Depth** (between 0 and the thickness), **Rafter or Truss**, Maximum Ridge Height (RO), Slope (applied to every slope-defining edge), Thickness (RO), Volume, Area, Comments, Mark and phasing.
- **Type parameters:** Structure, Default Thickness, coarse fill and analytical properties.
- **Align Eaves** (in the sketch): the user selects eave lines and chooses *Adjust Height* (each edge's plate offset is changed) or *Adjust Overhang*. Reviera then computes the change for each edge so that all the selected eave lines end at the same height.

**Roof generation algorithm** (geometry kernel; runs per roof):
1. **Edge data.** For each footprint edge `e` (in counter-clockwise order), let:
   - `L_e` be the **plate line**: the sketch line, offset to the wall's interior face when *Rafter or Truss* = Truss, or to the exterior face when it is Rafter. The offset is relative to the measured wall face when the edge is locked to a wall; otherwise the sketch line itself is the plate line.
   - `h_e = z_base + PlateOffset_e` be the plate height, where `z_base = elev(BaseLevel) + BaseOffset`.
   - `s_e = tan(slope_e)` be the gradient. A gable edge has `s_e = ∞`.
   - `o_e ≥ 0` be the overhang.
2. **Edge planes.** Each slope-defining edge carries the plane `P_e(p) = h_e + s_e · δ_e(p)`, where `δ_e(p)` is the signed horizontal distance from `L_e` toward the inside of the footprint. Each gable edge carries the vertical plane through `L_e`. An edge with a slope arrow carries the arrow's plane (ARC-SKCH-003) and is treated as slope-defining with that plane.
3. **Eave polygon.** Each slope-defining edge line is moved outward by `o_e`, and its eave height becomes `h_e − s_e·o_e`. Gable edges are moved outward by their overhang at the same plate height. The eave polygon `E` is the closed loop of these lines, with consecutive lines intersected.
4. **Wavefront propagation (generalised weighted straight skeleton).** Reviera treats the roof as a wavefront that rises in z.
   - At height z, the active edge `e` is the **contour line** of `P_e` at height z: the set of points where `P_e(p) = z`. For edges that are parallel to their plane's contour, this is the edge line moved inward by `(z − h_e)/s_e`. For a slope-arrow plane, it is a line perpendicular to the arrow. Gable edges do not move.
   - Starting from `z₀ = min over e of the eave heights`, Reviera processes events in increasing z:
     - **edge events**: an active edge shrinks to zero length, its two neighbours become adjacent, and a skeleton node is created;
     - **split events**: a reflex vertex of the wavefront hits an opposite edge, the polygon splits, and a node is created;
     - **vertex events**: several reflex vertices meet at once, which is handled as simultaneous split events, with ties broken by edge index for determinism.
   - Event times are the solutions of three-plane intersections (`P_a ∩ P_b ∩ P_c`), solved in `f64` with a relative tolerance of 1e-9. Coincident events within `TOL_POINT` are merged.
   - Propagation stops when every wavefront component has collapsed, or at the **cutoff height** `z_c = elev(CutoffLevel) + CutoffOffset`. At the cutoff, the remaining wavefront polygon becomes a flat top face (or an opening, if the *Cutoff* option says to leave it open for a second roof).
   - Each footprint edge `e` produces one **roof face**: the region swept by its active edge, which lies on `P_e`. Gable edges produce vertical gable faces, which become the gable walls' attach target and are not part of the roof's top surface.
   - **Reference implementations.** The classic straight-skeleton event algorithm (Aichholzer and Aurenhammer, and its weighted generalisation) and the procedural-extrusion model by Kelly and Wonka (2011) are published references for this approach. Reviera implements it itself in Rust. CGAL's straight-skeleton package is GPL-3.0-or-later and compatible, but it is not a required dependency.
   - **Convex footprints.** For a convex footprint with all edges slope-defining and no arrows, the result must equal the lower envelope `z(p) = min_e P_e(p)`. The test suite uses this as a cross-check.
5. **Inner loops (courtyards).** An inner loop of the footprint is a **courtyard**. Its edges are footprint edges like the outline's: slope-defining by default (the roof falls toward the courtyard), or gables. They take part in the skeleton as further wavefronts: the outline runs counter-clockwise and each courtyard clockwise, and a reflex vertex of one wavefront may meet an edge of another, joining them. A flat roof gets a slab with holes. Openings through a finished roof (shafts, skylights, dormers) are made with the opening tools (ARC-OPEN-*), not with footprint loops. *(Owner clarification, 2026-10-13: full-loop buildings with a central courtyard.)*
6. **Surface to solid.**
   - The skeleton faces form the roof's **reference surface**: the underside of the roof at the plate line.
   - Each layer is the shell between this surface offset by `T_{j−1}` and by `T_j` along the face normals (upward). OCCT builds these with `BRepOffsetAPI_MakeThickSolid` / `BRepOffsetAPI_MakeOffsetShape` using `GeomAbs_Intersection` joins, so hip and valley seams stay planar intersections.
   - **Eave cut** (the vertical-normal plane through each eave line):
     - *Plumb Cut*: the solid is cut by the vertical plane through the eave line.
     - *Two Cut – Plumb*: a vertical cut of height *Fascia Depth*, measured down from the top surface, followed by a horizontal cut.
     - *Two Cut – Square*: a cut perpendicular to the roof face of height *Fascia Depth*, followed by a horizontal cut.
     - Gable edges get a vertical cut at the eave polygon.
   - Every cut is a `BRepAlgoAPI_Cut` with a half-space or with a prism of the cut polygon.
7. **Joins.**
   - *Join/Unjoin Roof* (ARC-ROOF-006) extends this roof's picked edge face until it meets the target roof's face, and Reviera clips it there.
   - Walls attached to the roof use its underside (ARC-WALL-008).
   - Roof–roof overlaps that are not joined stay overlapping. Reviera does not merge them silently.
8. **Validation.**
   - If the skeleton fails (for example, because of degenerate input), Reviera reports "Roof cannot be generated from this footprint" and names the edges involved. The previous valid geometry is kept.
   - A slope steeper than 89.9° is an error.
   - A footprint that is not closed, or that intersects itself, fails sketch validation.

**Regeneration.**
- Edges into the roof: `datum` (base and cutoff levels), `sketch-lock` (picked walls; plan stage only, §3.15) and `type`.
- Edges out of the roof: `attach` to walls and columns, `host` to skylights, `cut` (Join/Unjoin Roof, Dormer), and `bound` to rooms.

**Data (IFC mapping).**
- **`IfcRoof`** with an **`IfcRoofType`** aggregates (`IfcRelAggregates`) one **`IfcSlab`** (`PredefinedType = ROOF`) per skeleton face. Each slab has `IfcMaterialLayerSetUsage` (AXIS3, POSITIVE from the reference surface) and a body that is an `IfcExtrudedAreaSolid` along the face normal, clipped by `IfcBooleanClippingResult` half-spaces for the eave cuts and neighbouring planes. When clipping would need more than eight half-spaces, the body is an `IfcFacetedBrep` instead.
- **`IfcRoof.PredefinedType`** is classified from the resolved faces:
  - all faces horizontal → `FLAT_ROOF`;
  - one sloped face → `SHED_ROOF`;
  - two opposed faces with gables → `GABLE_ROOF`;
  - all footprint edges slope-defining and four or more faces meeting at ridges → `HIP_ROOF`;
  - hips combined with gables → `HIPPED_GABLE_ROOF`;
  - a cutoff with a flat top over hips → `MANSARD_ROOF`;
  - all faces meeting at one apex → `PAVILION_ROOF`;
  - otherwise `FREEFORM`.
- `Pset_RoofCommon` holds IsExternal and ProjectedArea, and `Qto_RoofBaseQuantities` holds GrossArea, NetArea and ProjectedArea.
- The footprint sketch is stored in RXD element data (`sketch`), and the cut options in RXD element data (`recipe.roof`).

**Commands.**
- `roof.create_footprint(type_id, loop: [{curve, defines_slope, slope?, overhang?, plate_offset?, offset_from_base?, extend_to_core?, lock_ref?}], holes?, base_level, base_offset?, cutoff?: {level_id, offset}, rafter_cut?, fascia_depth?, rafter_or_truss?, slope_arrows?: [SlopeArrow], attach_walls?: bool) -> element_id`
- `roof.edit_footprint(roof_id, ...)` (same parameters)
- `roof.align_eaves(roof_id, edge_keys, mode: AdjustHeight|AdjustOverhang)`
- `roof.set_slope(roof_id, slope, edge_keys?)`
- `roof.set_params(roof_ids[], level_id?, base_offset?, slope?, cutoff_level_id?|null, cutoff_offset?)` (the instance parameters in one undo step)
- `roof.set_edge(roof_id, keys[], defines_slope?, slope?|null, overhang?)` (footprint curve properties after the sketch is finished)
- `roof.set_shape(roof_id, shape: hip|gable|gable_rotated|shed|shed_reversed|flat, overhang?)` (sets which footprint curves slope from a named shape)

**Implementation notes (2026-10-13).**
- As built, `roof.create_footprint` takes `curves: [{line, lock?, defines_slope?, slope?, overhang?}]` (keyed 1, 2, … in order) and `level_id`, like the other sketch-based commands. Plate offsets, rafter cuts other than plumb, slope arrows, holes, Align Eaves and `attach_walls` come later.
- The body is the `IfcRoof`'s own `Brep` representation, one `IfcFacetedBrep` per face slab (IFC allows a roof either to have its own body or to be decomposed). Aggregated `IfcSlab` parts come later with layered roof types.
- Per-edge eave heights are handled by extending each face from the plate down its own plane along the corner trajectories to its eave line, so steeper edges reach lower eaves.
- Each face slab is mitred against its neighbours by the plane through the shared edge halfway between the two face normals, and cut plumb at eaves and rakes.
- A new roof whose footprint was picked from walls sets its base offset to the walls' tops (Reviera's default; the user can change it).
- A roof drawn with lines on the highest level (no level above it) also starts at the tops of the walls on that level, rather than at floor height inside them.
- **Roof shape** (Properties, `roof.set_shape`):
  - Hip: every curve slopes.
  - Gable: the curves along the longest one slope; the ends are gables.
  - Gable (rotated): the curves across the longest one slope.
  - Shed: only the longest curve slopes. Shed (reversed): only the curve opposite it.
  - Flat: no curve slopes.
  - "Along" means within 10° of the longest curve's direction. A roof whose curves match no shape shows *Custom*. Slopes and overhangs are kept.
- Properties also set the **Overhang** of every curve (*Varies* when they differ), and **Cutoff Level** is chosen from the levels.
- **Attach Walls** (`roof.attach_walls`, Modify | Roofs): attaches the Reviera **perimeter** walls that the roof covers and that start below it. Gable walls fill up to the ridge, and eave walls meet the roof's underside.
  - A wall is on the perimeter when, just beside one of its faces at its middle, there is no enclosed region of its level.
  - Interior walls are left alone; Attach Top attaches them by hand.
  - This is offered after any new roof, however its footprint was made, not only one from picked walls.
  - Attached walls follow later shape and footprint changes.
- Still to come: editing single footprint curves on the model (grips, and moving or deleting one curve in Edit Footprint), and per-curve Properties during the sketch.

**Acceptance criteria.**
- [ ] A 10 m × 6 m rectangle with all four edges at 30° gives a hip roof whose ridge is 4 m long, at a height of 3·tan 30° = 1.732 m above the plate.
- [ ] The same rectangle with the two short edges as gables gives a gable roof whose 10 m ridge is at 1.732 m.
- [ ] An L-shaped footprint with all edges at 35° gives a valley along the bisector of the reflex corner. Every face is planar, and the faces meet without gaps within `TOL_POINT`.
- [ ] Edges with different slopes (30° and 45°) on the same rectangle give a ridge that is offset toward the steeper side, matching the analytic result.
- [ ] A cutoff at 1.0 m above the plate on a 30° hip produces a flat top face, and `PredefinedType = MANSARD_ROOF`.
- [ ] Two footprint roofs at different plate heights joined with Join/Unjoin Roof reproduce the figure in `screenshots/architecture/roof_different_eave_heights_render.png` (different eave heights on one building).
- [ ] For convex footprints, the faces match the lower-envelope cross-check within 0.01 mm on 1 000 randomly generated cases.
- [ ] **L-shaped buildings** (owner clarification, 2026-10-13), with the inner corner in either orientation, as a hip roof and as a gable roof:
  - The roof's projected area equals the L grown by the overhang exactly. The concave (inner) corner is neither missed nor doubled, and every convex corner is covered.
  - All six perimeter walls attach, the two at the inner corner included. Each wall's top follows the roof's underside along its length and never dips below the plate.
  - A gable end wall rises to its ridge.
- [ ] A **square building** gives a **pyramid** (hip roof, four faces meeting at one apex over the middle). As a gable, two walls become gable ends that reach the ridge.
- [ ] A **courtyard building** (a full loop around a central courtyard):
  - The ring is roofed once, with its ridge halfway across the ring, and the courtyard stays open. Both the street and the courtyard walls attach.
  - As a flat roof, the slab has the courtyard as a hole, and the saved IFC body has the same volume.
- [ ] Changing a roof from Hip to Gable keeps its attached walls attached, and the gable walls follow to the new ridge.
- [ ] Attach Walls on a building with an interior partition attaches the perimeter walls and leaves the partition at its own height.

### 3.6.5 Roof by extrusion
**ID:** ARC-ROOF-002 · **Priority:** P0 · **Ref:** research/02 §7.4

**Workflow.**
1. The user starts Roof › **Roof by Extrusion** in an elevation, section or 3D view.
2. The user picks a vertical work plane (Work Plane dialog).
3. In the **Roof Reference Level and Offset** dialog, the user picks the level (the highest level by default) and an offset. A non-zero offset creates a reference plane at that height.
4. The user sketches an **open** profile of lines and arcs and presses Finish.

**Behaviour & rules.**
- **Geometry.** The open profile wire `W` is the **top** surface's section. It is offset downward by the roof thickness: `BRepOffsetAPI_MakeOffset` with `IsOpenResult = true` gives the parallel curve, and the two are joined at the ends with perpendicular segments to make a closed face, which is then extruded along the work-plane normal from *Extrusion Start* to *Extrusion End* (`BRepPrimAPI_MakePrism`).
- **Layers** are offset bands of the profile.
- The default extrusion depth spans the walls under the profile, or 6 m if there are none.
- **Instance parameters:** Work Plane, Extrusion Start, Extrusion End, Reference Level, Level Offset, Rafter Cut, Fascia Depth, Slope (RO, for straight profiles) and the same identity and phasing parameters as footprint roofs.
- The roof can be rehosted with *Edit Work Plane*.

**Data (IFC mapping).** An `IfcRoof` aggregating one `IfcSlab(ROOF)` per profile segment (straight segments) or one slab for an arc (curved). Each body is an `IfcExtrudedAreaSolid` with the closed profile as an `IfcArbitraryClosedProfileDef`.

**Commands.** `roof.create_extrusion(type_id, work_plane, profile: [curve], ref_level, level_offset?, start, end) -> element_id`.

**Acceptance criteria.**
- [ ] An arc profile extruded 8 m gives a barrel roof (`PredefinedType = BARREL_ROOF`) of uniform thickness.

### 3.6.6 Roof by face
**ID:** ARC-ROOF-003 · **Priority:** P2 · **Ref:** research/02 §7.4, §12.1 · web: Create a Roof from a Mass Face; Update Face Selections for Roofs and Curtain Systems

**Purpose.** Roofs on free-form surfaces, generated from the faces of a mass or of an in-place or loadable generic model.

**Workflow.**
1. The user opens a view that shows the mass (Show Mass must not hide it, ARC-MASS-001) and starts Massing & Site › Model by Face › **Roof by Face**.
2. The user picks a roof type and, on the contextual tab, the **Level** (default: the active view's level, or in 3D the highest level at or below the lowest picked face).
3. With **Select Multiple** on (the default), each click toggles a face in the pick set; the cursor shows + or −. **Clear Selection** empties the set; **Create Roof** builds the roof. With Select Multiple off, one click creates a roof on that face at once.
4. Later, **Edit Face Selection** (Modify | Roofs › Model by Face) re-enters the pick mode with the current faces selected, and **Recreate Roof** rebuilds the roof from the new set.

**Behaviour & rules.**
- The user picks non-vertical faces of a mass (Select Multiple, Clear Selection, Create Roof).
- **Valid faces.** Planar or curved faces of masses and generic models whose normal is nowhere horizontal (at every sample point the normal is at least 1° away from the horizontal plane). All faces of one roof must belong to the same mass or generic-model instance ("Faces must belong to one element").
- *Picked Faces Location* = Top or Bottom of the roof at the face. The roof is offset from the face by its thickness in the matching direction (`BRepOffsetAPI_MakeOffsetShape`).
- One roof cannot mix upward-facing and downward-facing faces. The user can split the mass face first (ARC-MOD-019) and build one roof from each part.
- **Several faces.** Adjacent picked faces form one continuous roof: the layer solids are offset per face and the offset surfaces are intersected at the shared edges, so there is no gap or overlap. Faces that do not touch give one roof element with several disjoint bodies.
- **Layers.** The compound structure is offset layer by layer, normal to the face (ARC-CMPD-001). A type with a variable layer is refused ("Variable layers are not supported for roofs by face"). *Slope*, *Cutoff* and *Rafter Cut* are not available; the slope is whatever the face gives.
- **Openings and joins.** Vertical, by-face and shaft openings (ARC-OPEN-001, ARC-OPEN-002, ARC-OPEN-004) cut it, and walls can attach to it (ARC-WALL-008). Join/Unjoin Roof (ARC-ROOF-006) does not apply.
- The roof records *Related to Mass* and updates only through **Update to Face** (ARC-MASS-003).
- **Mass removed.** If the mass is deleted, the roof keeps its last geometry, *Related to Mass* becomes false, and Update to Face and Edit Face Selection are no longer offered.

**Data (IFC mapping).** `IfcRoof` (`PredefinedType = FREEFORM` unless the type says otherwise) aggregating one `IfcSlab` (`PredefinedType = ROOF`) per connected body, each with `IfcMaterialLayerSetUsage`. Curved bodies are written as `IfcAdvancedBrep` and planar ones as `IfcFacetedBrep`. The face references and pick location are stored in RXD element data (`recipe.roof`).

**Commands.**
- `roof.create_by_face(type_id, face_refs, location: Top|Bottom, level_id) -> element_id`
- `element.edit_face_selection(element_id, add?: [face_ref], remove?: [face_ref])` changes the face set of a roof by face or a curtain system by face (ARC-CW-007) and rebuilds it (the *Recreate* step).
- `element.update_to_face(element_ids)` (ARC-MASS-003).

**Acceptance criteria.**
- [ ] Mixing an upward-facing face and a downward-facing face in one selection is rejected with a clear message.
- [ ] A roof by face on the curved top of a half-cylinder mass, with *Picked Faces Location* = Bottom, has its underside on the face within `TOL_POINT` and exports an `IfcAdvancedBrep` body.
- [ ] Adding an adjacent face with Edit Face Selection and Recreate Roof gives one continuous roof with no gap at the shared edge.
- [ ] Picking faces of two different masses for one roof is refused.

### 3.6.7 Fascia, gutter and soffit
**ID:** ARC-ROOF-004 · **Priority:** P1 · **Ref:** research/02 §7.4

**Behaviour & rules.**
- **Fascia and Gutter** (host sweeps, built as in ARC-SLED-001):
  - The user picks roof edges, soffit edges, the edges of other fascias, or model lines. Gutters accept horizontal edges only.
  - Consecutive picks form one continuous mitered sweep. *Restart* starts a new sweep, and clicking empty space finishes.
  - Selected sweeps offer *Add/Remove Segments*, flip, **Modify Mitering** (Vertical, Horizontal or Perpendicular to the edge), and end handles.
  - **Instance parameters:** Vertical Profile Offset, Horizontal Profile Offset, Angle, Length.
  - **Type parameters:** Profile, Material.
  - Gutter end caps are separate families.
- **Soffit** (a sketch):
  - **Pick Roof Edges** creates sketch lines locked to the roof's eave edges.
  - **Pick Walls** uses the walls' outer faces.
  - The user trims the lines into a closed loop and presses Finish.
  - A soffit can be sloped with a slope arrow or line properties, like a floor.
  - It stays associated with the roof and walls unless it was drawn with plain lines.
  - Its type has a compound structure.

**Data (IFC mapping).**
- **Fascia**: `IfcCovering` (`PredefinedType = USERDEFINED`, `ObjectType = "FASCIA"`).
- **Gutter**: `IfcPipeSegment` (`PredefinedType = GUTTER`).
- **Soffit**: `IfcCovering` (`USERDEFINED`, `ObjectType = "SOFFIT"`) with `IfcMaterialLayerSetUsage`.
- All three are linked to the roof through `IfcRelCoversBldgElements` (fascia and soffit) or `IfcRelConnectsElements` (gutter).

**Commands.**
- `fascia.create(type_id, edge_refs, mitering?) -> element_id`
- `gutter.create(type_id, edge_refs) -> element_id`
- `soffit.create(type_id, loops, level_id, offset?, slope?) -> element_id`
- `hostsweep.set_mitering(id, mode)`

**Acceptance criteria.**
- [ ] A gutter picked along the four eaves of a hip roof forms one continuous mitered sweep, and it exports as `IfcPipeSegment(GUTTER)`.
- [ ] A soffit made with Pick Roof Edges follows the eave when the roof overhang changes.

### 3.6.7a Eaves and wall bearing
**ID:** ARC-ROOF-010 · **Priority:** P0 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** Define a roof's eaves as a construction that meets the perimeter walls correctly, not only as an overhang of the roof plane. *(Owner clarification, 2026-10-13: "the overhang tool… needs to implement the correct methods to join with perimeter walls".)*

**Workflow.**
1. The user selects a roof and chooses Modify | Roofs › **Eaves**.
2. The user picks eave edges (Tab picks all of them), or chooses *All eaves*. The settings below apply to the picked edges and are kept per edge.

**Behaviour & rules.**
- **Bearing:** where the roof's plate line sits on the wall below each eave:
  - the wall's *exterior face* (the default for picked walls), its *core centre* or its *interior face*;
  - a **heel height**: the vertical depth of the roof construction measured at the bearing line.
  - The roof's plate height is the wall's top at the bearing. The roof edge is the bearing line, so the overhang is measured from the wall's **exterior face** whatever the bearing.
- **Overhang:** a horizontal distance beyond the wall's exterior face. The roof plane continues down its slope to the eave line.
- **Rafter cut** (eave end): *plumb*, *square* (perpendicular to the roof) or *two-cut* (a plumb cut of the fascia depth, then a level cut), with the **fascia depth**.
- **Fascia:** optional, a host sweep along the eave (ARC-ROOF-004).
- **Soffit** (the underside closure):
  - *boxed* (horizontal, from the bottom of the fascia back to the wall's exterior face),
  - *sloped* (following the roof's underside back to the wall),
  - or *open* (none).
  - The soffit **returns to the wall face** and is trimmed against it, so there is no gap between the soffit and the wall.
- **Gutter:** not part of the eave. Gutters and downspouts belong to the plumbing subsystem (MEP-PIPE-010), which hangs them on the eave's fascia. *(Owner direction, 2026-10-13.)*
- **Wall join at the eave:**
  - The walls under an eave are **attached** to the roof (ARC-WALL-008). Their tops follow the roof's underside across the wall's thickness (a birdsmouth-like top: high at the interior face, low at the exterior face), and stop at the soffit where there is one.
  - A gable wall under a gable edge rises to the roof's underside (gable infill). The roof's rake overhang is measured from that wall's exterior face too.
  - Wall finishes (cladding layers) run up to the soffit.
- **Consistency:** each eave's settings regenerate with the roof. Moving a wall the footprint is locked to moves the bearing line, so the eave, fascia, soffit and attachment all follow.
- **Defaults:** a roof made with Pick Walls gets eaves on its slope-defining edges: exterior-face bearing, the overhang given in the tool, a plumb cut, a boxed soffit, and walls attached.

**Data (IFC mapping).** The eave settings are RXD element data on the roof (`recipe.roof`, per edge). Fascia and soffit are written as in ARC-ROOF-004, and the attachment as in ARC-WALL-008.

**Commands.** `roof.set_eaves(roof_id, edges: [curve_key] | all, bearing?: exterior|core|interior, heel?, overhang?, cut?: plumb|square|two_cut, fascia_depth?, fascia?: bool|type_id, soffit?: boxed|sloped|open, attach_walls?: bool, remove?: bool) -> ()`; `roof.eaves(roof_id) -> [eave]`

**Acceptance criteria.**
- [x] A 45° hip roof on 300 mm walls with exterior-face bearing, a 500 mm overhang and a boxed soffit:
  - every wall top meets the roof's underside across its full thickness;
  - the soffit runs from the fascia's bottom to the wall's exterior face with no gap;
  - the eave line is 500 mm out from the exterior face.
- [x] Changing the bearing to the core centre keeps the overhang measured from the exterior face. The underside then meets the plate at the wall's centre line: across half the wall, the roof's plane moves by tan(slope) × half the wall's thickness.
- [x] A two-cut eave with a 200 mm fascia depth has a 200 mm plumb face and a level underside at the eave.
- [x] Moving a locked eave wall 300 mm outward moves the eave, the fascia and the soffit with it, and the wall stays attached.

**Implementation notes (2026-10-13).** As built:
- **Storage:** eaves are per-edge `eave` settings in the roof recipe. Each eave finds the wall beneath it: the wall its curve is locked to, or else the nearest parallel wall the curve lies within.
- **Bearing and heel:** each eave's plate line moves to the bearing (and further out by `raise / tan(slope)` for a heel deeper than the roof's plumb depth), and its overhang is measured from the wall's exterior face.
- **Rafter cuts:** a square cut is a side plane square to the roof. A two-cut clips the face's slab with a level plane at the fascia depth below the eave's top and caps it; such a slab is written to IFC as triangles.
- **Fascia and soffit:** 25 mm fascia and 12 mm boxed or sloped soffit bands along each eave, mitred where neighbouring eaves meet. They are generated with the roof and written as extra B-reps of the `IfcRoof`. Separate covering elements and type choices are deferred.
- **Defaults:** a roof made from picked walls gets a default eave on each slope-defining picked curve. Footprint edits keep each curve's eave by key.
- **Wall join:** `roof.set_eaves` attaches the perimeter walls (ARC-WALL-008).
- **In the window:** Modify | Roofs › Eaves turns the selected roof's sloped edges into eaves. Bearing, Rafter Cut, Fascia, Soffit, Heel Height and Fascia Depth are in the Properties palette (they apply to every eave). Per-edge settings are by command.
- **Not built yet:**
  - picking edges in the window;
  - cladding layers stopping at the soffit (needs layered walls).

### 3.6.8 Slope arrows in roofs
**ID:** ARC-ROOF-005 · **Priority:** P1 · **Ref:** research/02 §7.4 · web: Slope Arrow Properties

**Purpose.** Slope a roof plane from an edge that is not slope-defining, so that gables, dormers and planes whose fall is not perpendicular to an edge can be modelled in one footprint.

**Workflow.**
1. In the footprint sketch (Edit Footprint), the user clears *Defines Roof Slope* on the edge that should be driven, splitting it first with Split Element if only part of it should be.
2. The user chooses Draw › **Slope Arrow** and clicks the tail (snapped onto that edge) and then the head (inside the footprint).
3. In the Properties palette the user sets *Specify* and its values (ARC-SKCH-003). The *Length* is read-only.
4. **Finish Edit Mode** regenerates the roof.

**Behaviour & rules.**
- **Values.** *Specify = Height at Tail* uses *Level at Tail*, *Height Offset at Tail*, *Level at Head* and *Height Offset at Head*; the slope is (head height − tail height) / arrow length in plan. *Specify = Slope* uses *Level at Tail*, *Height Offset at Tail* and *Slope*. The defaults are the roof's base level, an offset equal to the edge's *Plate Offset from Base*, and the roof type's default slope. Negative offsets are allowed.
- **Placement rules.** The tail must lie on a boundary edge (within `TOL_POINT`), and that edge must not be slope-defining; the head must lie inside the footprint or on its boundary. A zero-length arrow, an arrow whose head equals its tail, or a *Height at Tail* arrow with equal heights over a gable edge is reported on Finish ("Slope arrow does not define a slope").
- **Several arrows on one edge.** Each arrow drives the portion of the edge between its tail and the nearest split points; two arrows on the same unsplit edge are an error ("Only one slope arrow per edge segment").
- **Interaction with cutoff.** The cutoff level (ARC-ROOF-001) clips arrow-driven planes as it clips any plane.
- A slope arrow is placed on a footprint edge that is **not** slope-defining (ARC-SKCH-003). Typical uses:
  - **four-sided gable**: an arrow from a gable edge to the ridge;
  - **dormer without side walls**: split a footprint line twice, clear the slope-defining flag of the middle segment, and draw two arrows from its ends to its midpoint;
  - **tapered** ramps and slabs.
- In the skeleton, the arrow's plane replaces the edge's plane (ARC-ROOF-001, step 2).
- **Different eave heights** on one building are modelled as two roofs at different plate heights, using cutoff levels, joined with Join/Unjoin Roof.
- **Floors and ceilings** use the same arrow element (ARC-SKCH-003); there it tilts the whole slab, and at most one arrow is allowed per slab.

**Data (IFC mapping).** Arrows are sketch curves with role `slope_arrow` in RXD element data (`sketch`); their values are per-curve properties. The resulting planes are part of the explicit roof body.

**Commands.** In a sketch session, `sketch.add_curve(session_id, Line{tail, head}, role: "slope_arrow", props: {specify, level_tail, offset_tail, level_head?, offset_head?, slope?}) -> curve_key` and `sketch.edit_curve` for later changes; non-interactively, the `slope_arrows: [SlopeArrow]` parameter of `roof.create_footprint` and `roof.edit_footprint` (ARC-ROOF-001).

**Acceptance criteria.**
- [ ] A dormer made with two arrows from the ends of a 3 m segment to its midpoint, rising 1 m, gives two triangular faces that meet the main roof faces without gaps.
- [ ] On a 10 × 6 m footprint with only the two long edges slope-defining, an arrow from the middle of one short edge to the ridge with *Specify = Slope* 30° turns that gable into a hip plane at 30°.
- [ ] An arrow whose tail is not on a boundary edge, or that sits on a slope-defining edge, is reported on Finish and the sketch stays open.

### 3.6.8a Dormers (automated)
**ID:** ARC-ROOF-007 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** Add a dormer to a sloped roof in one step, with its roof, its walls, the walls that fill the gaps down to the main roof, and the opening in the main roof. This automates the manual sequence of ARC-OPEN-005 (dormer walls and roof, Join/Unjoin Roof, Dormer opening).

**Workflow.**
1. The user selects a footprint roof and starts Modify | Roofs › **Dormer**, or Architecture › Roof › Dormer.
2. In the contextual tab or Properties, the user chooses the **style**:
   - *Gable (pitched)*: a ridge running into the main roof, with a gable front;
   - *Hipped (pitched)*: the front is hipped too;
   - *Shed (flat lift)*: one plane falling to the front, at a shallower slope than the main roof;
   - *Flat*: a flat lifted roof.
3. The user sets the **width**, the **front wall height** (from the main roof's plate or a level), the dormer roof's slope (pitched styles), its overhang, and the wall type (default: the type of the wall below the eave).
4. The user clicks on a sloped face of the main roof. The click gives the dormer's centre line, which runs up the face's fall line. The front stands at the click point, or set back by the *Front setback* option from the eave line below.
5. Reviera previews the dormer. A second click (or Enter) places it.

**Behaviour & rules.**
- **Generated parts** (one dormer *assembly* that owns them):
  - the **front wall**;
  - the two **cheek walls**, which fill the gap between the dormer roof and the main roof (*gap walls*);
  - the **dormer roof** (a footprint roof joined to the main roof, ARC-ROOF-006);
  - a **dormer opening** cutting the main roof inside the dormer walls (ARC-OPEN-005).
- **Gap walls.** Each cheek wall's base follows the main roof's top surface along the wall, so it sits on the slope. Its top follows the dormer roof's underside. The front wall runs from the main roof up to the dormer eave (gable styles: to the gable). Openings (a window) can be hosted in the front wall.
- **Depth.** The dormer runs back up the main roof until its roof meets the main roof's surface:
  - for pitched styles, at the dormer ridge's height;
  - for shed and flat styles, at the dormer roof's back edge.
  - The cheek walls end where they meet the main roof.
- **Editing.** Width, height, style, slope, overhang and setback stay editable. Moving the dormer slides it on the main roof face, and every part regenerates. Deleting the dormer deletes its parts and closes the opening.
- **Validation.**
  - A dormer must fit on one face of the main roof (between its eave, ridge and hips), or the user is told which edge it crosses.
  - A dormer roof slope steeper than the main roof's (shed style) is refused.

**Data (IFC mapping).** The dormer is an `IfcElementAssembly` (ObjectType "DORMER") aggregating its `IfcWall`s, its `IfcRoof` and the `IfcOpeningElement` that voids the main roof (`IfcRelVoidsElement`). The recipe is RXD element data (`recipe.dormer`).

**Commands.**
- `dormer.create(roof_id, point, style: gable|hip|shed|flat, width, front_height, slope?, overhang?, setback?, wall_type_id?) -> element_id`
- `dormer.set(dormer_id, style?, width?, front_height?, slope?, overhang?, setback?) -> ()`
- `dormer.move(dormer_id, point) -> ()`

**Acceptance criteria.**
- [ ] A gable dormer 2 m wide with a 1.2 m front wall on a 45° roof face has two cheek walls whose bases lie on the main roof's top surface and whose tops meet the dormer roof, with no gap greater than `TOL_POINT`. The dormer ridge meets the main roof face.
- [ ] A shed (flat-lift) dormer at 15° on a 45° roof ends where its roof meets the main roof, and its cheek walls are trapezoids filling the gap.
- [ ] The main roof has an opening exactly inside the dormer walls' interior faces. Deleting the dormer closes it.
- [ ] `dormer.create` from Lua or MCP gives the same model as the window tool.

**Implementation notes (2026-10-13).**
- As built:
  - `dormer.create(roof_id, point, style?, width?, front_height?, slope?, overhang?)`, `dormer.set(dormer_id, style?, width?, front_height?, slope?, overhang?)` and `dormer.move(dormer_id, point)`.
  - Defaults: 1.8 m wide, eave 0.9 m above the main roof, the main roof's slope (a shed takes a third of it), 300 mm overhang. Front setback and wall type choice come later.
- **The dormer roof** is a Reviera footprint roof on the dormer's outline, using the main roof's type. Its edges are set by style:
  - gable: sides slope, front gable;
  - hip: front slopes too;
  - shed: front only;
  - flat: none.
- **Depth and the flush join** (owner clarifications, 2026-10-13: "the dormer roofs need to be joined better so that the joins are flush… a roof that rain flows off of correctly"; "the up-slope of the dormer needs to join so that it is not above the main roof at the uphill join"):
  - The dormer runs back until the *top* of its roof meets the main roof's top surface: its ridge's top (gable, hip), its rising plane's top (shed), or its flat top.
  - The dormer roof's body is then cut along the main roof's top plane. Nothing of it stands above or behind the main roof at the uphill end; its top surfaces meet the main roof's in valley lines, so water runs off the dormer onto the main roof.
  - The whole outline must lie on the face it was placed on (`dormer.does_not_fit`). That also refuses a dormer whose ridge would rise above the main roof's ridge.
- **Gap walls:** a front wall and two cheek walls (the project's default wall type), their outer faces on the outline.
  - They run from just below the main roof's underside at the front, and their tops are attached to the dormer roof (ARC-WALL-008). The front gable and the cheeks therefore fill exactly to the dormer roof.
  - The cheek walls end where the dormer roof's underside meets the main roof: the eave (gable, hip, flat) or the shed's underside plane.
  - Their bases are level, not yet following the main roof's slope, so the part below the main roof stays inside the building.
- **Parts and ownership:**
  - The dormer is an `IfcElementAssembly` (ObjectType "DORMER"), with RXD `recipe.dormer`.
  - Its parts carry `rxd:dormer` and go with it: deleting the dormer deletes them, and deleting the main roof deletes its dormers.
  - Edits rebuild the parts. The Model Tree shows the parts under the dormer, and the dormer under its roof.
- **Window:** Modify | Roofs › Dormer asks for the style, then each click on the selected roof places one, in a plan or on the roof in 3D. Selecting any part shows the dormer's own properties (style, width, front height, overhang, slope).
- The opening in the main roof: built (ARC-OPEN-005).
- Later:
  - flashing and crickets at the valleys;
  - wall bases on the main roof's slope;
  - aggregation of the parts in IFC;
  - rebuilding when the main roof changes;
  - hipped-gable fronts.

### 3.6.8b Arched and vaulted roofs
**ID:** ARC-ROOF-008 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** Smooth arched (barrel and segmental) roofs over a footprint, including **tapering vaults** whose arch widens or narrows from one end to the other to fit a trapezoidal footprint.

**Workflow.**
1. Architecture › Roof › **Arched Roof**.
2. The user picks or sketches a four-sided footprint and the **spring edges**: the two edges the arch springs from. The other two are the arch's ends.
3. The user sets the arch: **rise** (or a constant radius), the profile (*circular segment*, *semicircle*, *elliptical* or *catenary*), the spring height, the thickness from the type, and the overhangs (along the spring edges and at the ends).

**Behaviour & rules.**
- **Constant section** (parallel spring edges): the profile is swept straight along the spring edges' direction (a barrel or segmental vault).
- **Tapering vault** (spring edges not parallel, as in a trapezoid): at each end the span is the distance between the spring edges there.
  - *Constant rise*: every section rises the same height, so the radius changes along the vault.
  - *Constant slope at the springing*: every section meets the spring edges at the same angle, so the rise scales with the span.
  - The surface is ruled between corresponding points of the end sections (equal parameters along each profile). It is built with `BRepOffsetAPI_ThruSections` through end sections and intermediate sections every 500 mm, so it stays smooth.
- **Thickness** is offset along the surface normal, giving a constant thickness; ends are cut vertically.
- **Walls attach** to the underside of an arched roof like any roof (ARC-WALL-008). The end walls fill up to the arch, the gable infill of ARC-ROOF-001.
- **Display:** smooth shading in 3D. Plans show the outline and the crown line dashed.

**Data (IFC mapping).** `IfcRoof` with `PredefinedType = BARREL_ROOF`. Its body is an `IfcExtrudedAreaSolid` of the profile for constant sections, or an `IfcAdvancedBrep` (B-spline faces; a faceted B-rep where a consumer needs one) for tapering vaults. The recipe is RXD element data (`recipe.roof`, kind `arched`).

**Commands.** `roof.create_arched(footprint: [4 points] | curves, spring_edges: [k1, k2], level_id, base_offset?, rise?, radius?, profile?: segment|semicircle|elliptical|catenary, taper?: constant_rise|constant_slope, overhang?, end_overhang?, type_id?) -> element_id` and `roof.set_arch(roof_id, rise?, radius?, profile?, taper?) -> ()`.

**Acceptance criteria.**
- [x] A 6 m-wide, 10 m-long footprint with a 1.5 m rise gives a circular-segment barrel roof of radius 3.75 m. Its crown is 1.5 m above the springing all along.
- [x] A trapezoid whose span narrows from 6 m to 4 m along 8 m, with constant rise, gives a smooth vault. Every section is a circular segment of rise 1.5 m, with a radius of 3.75 m at the wide end and 2.08 m at the narrow end, and the surface has no creases.
- [x] With constant slope at the springing instead, the narrow end's rise is 1.0 m.
- [x] End walls attached to an arched roof reach the arch exactly.

**Implementation notes (2026-10-13).** As built:
- An arched roof is a footprint roof (four curves) with an `arch` in its recipe:
  - spring edges (default the longer pair), rise or radius, profile (segment, semicircle, elliptical, catenary), taper (constant rise or constant slope), side and end overhangs.
  - `roof.create_arched` and `roof.set_arch`.
- **Generation:** sections every 500 mm along the vault at equal parameters (cosine-spaced across, 24 steps), so a trapezoid gives a smooth taper. Each grid cell is two flat triangles, which are the roof's faces, so wall attachment, underside queries and plans work as for any roof. Overhangs continue the arch's tangent (capped at 80°) and the vault past its ends.
- **Body:** one closed shell, the grid offset by the type's thickness along the smoothed vertex normals. It is written as a faceted B-rep (`BARREL_ROOF`). The B-spline `IfcAdvancedBrep` and `IfcExtrudedAreaSolid` forms come later, with OCCT.
- **Window:** Architecture › Arched Roof sketches the footprint like Roof, and Finish makes it arched (and offers to attach the walls; the end walls fill up to the arch). A selected arched roof shows Profile, Taper, Rise and the overhangs in Properties.

### 3.6.8c Dome roofs
**ID:** ARC-ROOF-009 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** Domes over circular, elliptical and polygonal plans.

**Workflow.** Architecture › Roof › **Dome**: the user picks a circle, an ellipse or a regular polygon (sketched, or picked from walls), then sets the rise and the profile.

**Behaviour & rules.**
- **Profiles:** *hemisphere* (rise equals the radius), *segmental* (a rise below the radius), *pointed* (two arcs meeting at the crown, given the ratio of their radius to the span) and *onion* (a spline profile with a bulge ratio).
- **Plans:** over a circle the profile is revolved (`BRepPrimAPI_MakeRevol`). Over an ellipse the revolved dome is scaled in plan. Over a regular polygon it is a **cloister vault** (each side a cylindrical segment meeting at the hips), or a **polygonal dome** with flat facets, by option.
- **Oculus:** an optional opening at the crown, of a given diameter, with or without a lantern curb.
- **Drum:** domes sit on walls (a drum) attached to their springing. Walls attached under a dome follow its underside.
- **Thickness** is constant, offset along the normal.

**Data (IFC mapping).** `IfcRoof` with `PredefinedType = DOME_ROOF`. The body is an `IfcRevolvedAreaSolid` for circular domes, otherwise an `IfcAdvancedBrep` or faceted B-rep. The recipe is RXD element data (`recipe.roof`, kind `dome`).

**Commands.** `roof.create_dome(plan: Circle{center, radius} | Ellipse{...} | Polygon{center, radius, sides, rotation}, level_id, base_offset?, rise?, profile?: hemisphere|segmental|pointed|onion, oculus?, polygon_mode?: cloister|faceted, type_id?) -> element_id` and `roof.set_dome(roof_id, ...) -> ()`.

**Acceptance criteria.**
- [x] A hemisphere over a 4 m-radius circle has its crown 4 m above the springing, and the volume of its shell matches 2/3·π·(R₂³ − R₁³) within 0.5%.
- [x] An octagonal cloister vault has eight cylindrical faces meeting at straight hips in plan.
- [x] An oculus of 1 m leaves a 1 m circular opening at the crown.

**Implementation notes (2026-10-13).** As built:
- A dome is an `IfcRoof` whose recipe holds its plan (circle, ellipse, regular polygon), rise, profile (hemisphere, segmental, pointed with an arc ratio, onion with a bulge ratio), oculus and a faceted option. It has no footprint sketch.
- `roof.create_dome` and `roof.set_dome`.
- **Generation:** rings up the profile (16, or 5 faceted) by directions round the plan (48 for curves; six per polygon side including the corners, so the cloister vault's hips are straight). The faces and the shell are built as for arched roofs, and saved as `DOME_ROOF`.
- **Window:** Architecture › Dome asks for the plan and profile, then two clicks in a plan (the centre and a point on the springing; a polygon's first corner is there). The dome springs from the top of the walls inside its circle (a drum), else from the level's walls. Properties show Profile, Rise, Oculus and Faceted.
- **Later:** a lantern curb at the oculus, a revolved-solid IFC body, ellipse picking in the window.

### 3.6.9 Join/Unjoin Roof
**ID:** ARC-ROOF-006 · **Priority:** P1 · **Ref:** research/02 §7.4, §9

**Workflow.** The user starts Modify › Geometry › **Join/Unjoin Roof**, picks the edge of the roof to extend, and then picks the face of the roof or wall to join to.

**Behaviour & rules.**
- The picked edge's face plane `P_e` is extended until it meets the target surface. The extended face is then clipped by the target's surface (`BRepAlgoAPI_Splitter`), and the part beyond is removed.
- **Targets:** any roof face, or a wall face (for wall targets, only footprint roofs can be joined). A roof with several top faces cannot be a target.
- Join Geometry must not be used for dormers.
- Unjoin restores the edge's original termination.
- The relation is a `cut` / `attach` edge from the target to the joined roof.

**Data (IFC mapping).** `IfcRelConnectsElements` (Name "JoinRoof"), with the relation stored in RXD document `model/constraints`.

**Commands.** `roof.join(roof_id, edge_ref, target_ref)` and `roof.unjoin(roof_id, edge_ref?)`.

**Acceptance criteria.**
- [ ] A dormer roof joined to the main roof ends exactly on the main roof's top surface, with no gap or overlap greater than `TOL_POINT`.

### 3.6.10 Shape editing (floors, roofs, structural floors, toposolids)
**ID:** ARC-SHAPE-001 · **Priority:** P1 · **Ref:** research/02 §1, §7.5, §17.7 · screenshots/architecture/shape_editing_points_toposolid.png

**Purpose.** Warp the top surface of a slab with points and split lines, for falls to drains, terrain and similar cases.

**UI.** With an element selected, the **Shape Editing** panel offers:

| Tool | Behaviour |
|---|---|
| **Modify Sub Elements** | Select points or edges (Tab cycles between them). A blue arrow drags vertically, and a red square (toposolids only) drags horizontally. A text control or the *Elevation* field sets the exact height relative to the **Elevation Base** (Current Level, Project Base Point, Survey Point or Internal Origin). Dragging an edge moves its middle and keeps the relative heights of its ends |
| **Add Point** | *Along Surface* mode (with *Offset from Surface*) or *Absolute* mode (with *Elevation*) |
| **Add Split Line** | From a vertex, edge, face or point to another. Uses the same two modes, and snaps with temporary dimensions |
| **Pick Supports** | Creates split lines along picked beams and lowers the slab bottom onto them (structural floors) |
| **Convert folding lines to split lines** | Turns automatic triangulation edges into explicit split lines |
| **Reset Shape** | Removes every modifier |
| **Snap to Editor Points and Lines** | Toggle, off by default |
| **Snap XYZ** | Reports the Z of the reference object |
| **Preview Points** | Toggles the display of points, for performance |
| Copy/Paste | Ctrl+C and Ctrl+V of points and lines inside edit mode, including between toposolids |

**Behaviour & rules.**
1. **Modifier set.** The modifier set is:
   - the boundary vertices, each with a height offset (default 0);
   - added points (x, y, z_offset);
   - split lines (polylines with heights at their vertices; between vertices, heights interpolate linearly along the line).
2. **Triangulation.** A constrained Delaunay triangulation (CDT) is built in plan, with:
   - vertices: boundary vertices, added points and split-line vertices;
   - constraints: boundary edges, hole edges and split-line segments.

   Arcs in the boundary are first discretised to a chord error of 1 mm. The triangulation uses the `spade` crate (MIT/Apache-2.0), or an equivalent library with a compatible licence.
3. **Folding lines** are the triangulation edges that are not constraints. Folding lines are hidden by default.
4. **Top surface.** Each triangle is lifted to the heights of its vertices, giving planar facets. A point added *Along Surface* takes the current height of the surface at its (x, y) position plus its offset, evaluated **once, at insertion**. After that its height is absolute.
5. **Layers.** The behaviour depends on the type:
   - **No variable layer:** the whole slab follows the warped top with constant thickness. Each layer surface is the top surface offset along the facet normals (floors and roofs, `BRepOffsetAPI_MakeOffsetShape` with intersection joins) or straight down (toposolids).
   - **One variable layer:** the layers **above** the variable layer follow the warped top. The layers **below** it keep the original flat (or sloped-plane) shape. The variable layer fills the space between, and it must stay ≥ 0 thick everywhere; otherwise Reviera reports "Variable layer would be negative" and does not commit the edit.
6. **Curved Edge Condition** (instance, for arc boundaries): *Conform to curve* keeps the arc edge on the surface; *Project to side* uses vertical faces on the arc.
7. **Validation.** Points outside the boundary are rejected. A split line that crosses another split line creates a vertex where they cross, whose height is interpolated from the first line.

**Regeneration.** Shape modifiers belong to the host. Changing the boundary keeps points that are still inside and discards the others, with a warning.

**Data (IFC mapping).**
- The modifiers are stored in RXD element data: `sketch` (role `shape`) for floors and roofs, and `recipe.toposolid` for toposolids.
- The body becomes an `IfcFacetedBrep` (IFC4) or an `IfcPolygonalFaceSet` with `Closed = TRUE`.
- `IfcMaterialLayerSetUsage` is kept. For a variable layer, IFC4 offers `IfcMaterialLayerWithOffsets`, which cannot describe a free-form variable thickness. Reviera writes the nominal thickness and puts the true geometry in the body.

**Commands.**
- `shape.add_point(element_id, xy, mode: AlongSurface(offset)|Absolute(z), elevation_base?) -> key`
- `shape.add_split_line(element_id, points: [xy], mode, heights?) -> key`
- `shape.move(element_id, keys, dz?, dxy?)`
- `shape.set_elevation(element_id, keys, z, elevation_base?)`
- `shape.pick_supports(element_id, beam_ids)`
- `shape.convert_folding_lines(element_id)`
- `shape.reset(element_id)`
- `shape.copy(element_id, keys) -> clip` *(query)*
- `shape.paste(element_id, clip, offset?)`

**Acceptance criteria.**
- [ ] A flat roof with one point dropped 50 mm in the middle of a 10 m × 10 m area, with an insulation layer marked variable, gives four planar facets falling to the drain. The deck stays flat, and the insulation thickness at the drain is reduced by exactly 50 mm.
- [ ] Without a variable layer, the same edit lowers the whole slab, and the thickness measured perpendicular to each facet stays constant.
- [ ] Reset Shape restores the original body exactly, and the body's hash matches the one recorded before the edit.

### 3.6.11 Parts: model and creation
**ID:** ARC-PART-001 · **Priority:** P2 · **Ref:** research/02 §2.1 (Figure 1, Create panel), §4.5 · web: About Parts; Create Parts

**Purpose.** Break design elements into the pieces that will actually be built (each layer of a wall, each slab layer, pieces of a layer divided into panels), so that construction modellers can schedule, tag, filter, sequence and export them, while the design element stays the single source of geometry.

**Workflow.**
1. The user selects eligible elements and chooses Modify | <Element> › Create › **Create Parts**, or starts Modify › Create › **Create Parts** first, picks elements (ineligible ones are drawn in half-tone and cannot be picked) and presses Enter or Spacebar.
2. Reviera creates the parts in one transaction. The current view switches its *Parts Visibility* to *Show Parts* if it was *Show Original*, and the status bar reports how many parts were created.
3. The user selects parts to divide (ARC-PART-002), merge or exclude (ARC-PART-003), or to edit their properties (ARC-PART-004).

**Behaviour & rules.**
- **Eligible sources.** Elements with a compound structure: basic walls (not stacked or curtain walls, and not wall sweeps), foundation walls, floors and structural floors (except shape-edited floors with more than one layer), roofs by footprint, extrusion or face, ceilings, foundation slabs, slab edges, fascias and gutters. Family instances of the categories structural framing, columns and structural columns are also eligible, and give a single part each. Elements in a linked model are eligible; their parts live in the host model. An element that already has parts is not eligible again.
- **One part per layer.** A layered source gives one part per non-membrane layer (membranes, which have zero thickness, give none). A vertically compound wall (ARC-CMPD-003) gives one part per region. Each part's solid is the layer's solid **after** joins, layer cleanup, wrapping, attachment, extensions and insert voids, so the parts tile the source exactly with no gaps or overlaps.
- **Identity.** A part is identified by `(source GlobalId, layer or region sub-reference, division path)`, where the division path lists the divisions and pieces that led to it. The identity is stable across regeneration, so a part keeps its GlobalId, Mark and overrides while its source changes, as long as the same layer and piece still exist.
- **Source changes.** Parts regenerate automatically whenever the source changes, after the source's body stage:
  - geometry, joins, openings, inserts, orientation (flip) or thickness: every part is rebuilt and its divisions re-applied;
  - type change or layers added or removed: parts are matched to the new layers by layer sub-reference, then by function and order. A new layer gets a new, undivided part; a part whose layer has gone is deleted, together with its descendants, and the deletion is listed in a warning;
  - material change of a layer: parts with *Material By Original* on follow it;
  - the source becomes ineligible (for example a floor is shape-edited and has several layers): its parts keep their last geometry and are flagged "Source cannot be divided into parts".
- **Delete and copy.** Deleting the source deletes all its parts and their descendants (after a confirmation that gives the count). Copying, mirroring or arraying the source copies its parts, divisions, merges, exclusions and overrides. A source whose parts exist is still editable in every way.
- **Linked sources.** When the link is unloaded or the source element disappears from it, the parts keep their last geometry and are flagged "Source unavailable"; they regenerate when the link is reloaded.
- **Selection.** In a view showing parts, clicking selects a part; the source element is not selectable there (ARC-PART-004). Tab cycles between overlapping parts.

**Regeneration.** A `host` edge from the source element's body stage to each part (inserts reach the parts through the source's cut body, so they need no edge of their own), and `datum` edges from the references used by divisions (ARC-PART-002). Parts of one source regenerate together; different sources regenerate in parallel.

**Data (IFC mapping).**
- Each part that currently exists as a *leaf* (not divided further, not absorbed in a merge) is an **`IfcBuildingElementPart`** that decomposes its source element through **`IfcRelAggregates`** (`RelatingObject` = source, `RelatedObjects` = its parts). `PredefinedType` = `INSULATION` for parts of Thermal/Air layers, `PRECASTPANEL` for parts the STR chapter marks as precast, otherwise `USERDEFINED` with `ObjectType` = the layer function (for example "Finish 1"), or `NOTDEFINED` for single-part family sources.
- Each part has its own `Body` (an `IfcExtrudedAreaSolid` where the piece is prismatic, otherwise an `IfcFacetedBrep`) and its own `IfcRelAssociatesMaterial` with an `IfcMaterial`. It is contained in the storey of its *Base Level* (or the source's storey when Non Associated).
- **Source body.** By default (export option *Parts: replace source body*, on), a source that has parts is written with its `Axis`/`FootPrint` representation and its material layer set but **without** a `Body`, so that tools which read both do not count the volume twice. With the option off, both bodies are written.
- Excluded parts (ARC-PART-003) are not exported. Intermediate parts (divided or merged away) are not exported; their history is kept in RXD.
- RXD element data (`recipe.part`) on each part holds its identity tuple, its parent part, its division and merge membership, its excluded flag and its overrides. Division and merge definitions are project-level and stored in RXD document `model/parts`.
- **Import.** An `IfcBuildingElementPart` without RXD data is imported as a non-parametric part of its aggregating element; it cannot be divided further until the user regenerates parts from the source (`parts.create` on the source replaces it after confirmation).

**Commands.** `parts.create(host_ids) -> [element_id]` returns the new parts. Ineligible elements in `host_ids` are skipped and listed in the result warning; if none is eligible the command fails with "No element can be divided into parts".

**Acceptance criteria.**
- [ ] Create Parts on a wall with brick, cavity insulation, block and plaster layers and a window creates four parts whose volumes add up to the wall's volume within 0.01%, each with the opening cut.
- [ ] Changing the wall's block thickness from 100 to 140 mm regenerates the parts in place; their GlobalIds and Marks are unchanged.
- [ ] Changing the wall to a type without the plaster layer deletes the plaster part and reports it.
- [ ] The IFC export contains one `IfcBuildingElementPart` per part, aggregated by the `IfcWall`, and the `IfcWall` has no `Body` representation; IfcOpenShell's validator reports no errors.
- [ ] A stacked wall and a curtain wall cannot be picked while Create Parts is active.

### 3.6.12 Dividing parts
**ID:** ARC-PART-002 · **Priority:** P2 · **Ref:** research/02 §2.3 · web: Divide Parts; Edit a Part Division; Division Instance Properties; Create a Division Profile Family

**Purpose.** Cut parts into smaller pieces (panels, pours, sheets) along sketched lines or along levels, grids and reference planes, optionally with a gap and a profiled edge.

**Workflow.**
1. The user selects one or more parts and chooses Modify | Parts › Part › **Divide Parts**. A division session opens: the selected parts are tinted, an outline offset 300 mm (at view scale) outside their combined extent is drawn dashed, and the *Division* properties appear in the Properties palette.
2. **By sketch:** the user chooses **Edit Sketch**, sets or shows the work plane (Work Plane panel; default: the plane of the largest face of the first part), sketches division curves and chooses Finish Sketch.
3. **By references:** the user chooses **Intersecting References**. The dialog lists the levels, grids and named reference planes that intersect the parts, with a filter drop-down (All, Levels, Grids, Reference Planes) and an Offset column per reference (positive or negative, default 0). The user ticks references and presses OK.
4. The user sets *Divider Gap* and the profile options (below) and can **Add** or **Remove** parts from the division.
5. **Finish Edit Mode** applies the division; **Cancel** discards the session.
6. Later, selecting any part made by a division and choosing **Edit Division** reopens the same session: the original parts are shown in a light tint and the division curves in a highlight colour; curves can be dragged, deleted or added, references changed, and parts added or removed.

**Behaviour & rules.**
- **Division curves.** Lines, arcs, splines and closed loops on one work plane. An open chain must cross the parts' dashed outline at both ends (it is extended to infinity in its end directions for the cut); a closed loop cuts out an island. Curves that do not cross any part are kept but have no effect, and the session warns about them on Finish.
- **Cut.** Each curve is swept along the work-plane normal into a cutting surface that passes right through the parts (`BRepPrimAPI_MakePrism` of the curve over the parts' extent plus 1 m each way). References are used as infinite planes moved by their offset. All cutting surfaces split each part in one `BRepAlgoAPI_Splitter` call; every resulting solid becomes a new part, and the parent part stays in the model as a hidden intermediate element.
- **Divider Gap** (≥ 0, default 0) removes a slab of that width centred on each cutting surface (`BRepAlgoAPI_Cut` with the surface thickened by ±gap/2 using `BRepOffsetAPI_MakeThickSolid`). A piece that disappears entirely within a gap is not created, and the session reports it.
- **Edge profile.** *Split Profile Type* (default *None*) applies a division profile (a 2D profile family whose usage is *Generic*; DM chapter) along each divider edge. *Edge Match* decides how the two faces of a joint are shaped: **Complementary** (one side gets the profile as a tongue, the other the matching groove), **Mirrored** (both sides get the profile mirrored across the divider) or **Rotated** (the second side gets the profile turned 180° about the divider line). *Profile Along Flip* swaps which side gets which shape (all three modes); *Profile Across Flip* turns the profile over across the part thickness (Complementary and Rotated only). *Profile Offset* moves the profile from the divider centre. The profile is swept along the divider edge on each piece and added or subtracted (`BRepOffsetAPI_MakePipeShell`, then fuse or cut).
- **Nesting.** A part made by a division can itself be divided; the division path grows by one level. Editing an outer division re-runs the inner ones on the new pieces; an inner division whose curves no longer cross its part leaves the part whole.
- **Following references.** Divisions by reference follow their references: moving a grid moves the cut. A sketched division has a `datum` edge to its work plane's host.
- **Adding and removing parts** in a session (or with the commands) only changes which parts the same curves and references cut. Removing a part restores it as one piece and deletes its descendants after a warning.
- **Session recovery.** The division session is an interactive tool (conventions §2.1). Its micro-operation schema is: `begin {part_ids | division_id}`, the `sketch.*` operations, `set_refs {[(ref, offset)]}`, `set_props {gap, profile}`, `add_parts {ids}`, `remove_parts {ids}`, `finish`, `cancel`. Replaying them on the model state at `begin` rebuilds the session after a crash.

**Data (IFC mapping).** The division definition (parts, curves with work plane, references with offsets, gap and profile settings) is stored in RXD document `model/parts` (kind `part.division`) under a division ID. Only the resulting leaf parts are exported (ARC-PART-001); the gap is visible as the space between them.

**Commands.**
- `parts.divide(part_ids, curves?: [curve], work_plane?: plane | host_ref, refs?: [{ref, offset}], gap?: f64, profile?: {type_id, edge_match: Complementary|Mirrored|Rotated, flip_along?: bool, flip_across?: bool, offset?: f64}) -> {division_id, parts: [element_id]}` divides non-interactively.
- `parts.division_begin(part_ids | division_id) -> session_id` opens the interactive session; within it, the `sketch.*` commands edit curves.
- `parts.division_set(session_id, refs?, gap?, profile?)`, `parts.division_add(session_id, part_ids)` and `parts.division_remove(session_id, part_ids)` change it.
- `parts.division_finish(session_id) -> {division_id, parts: [element_id]}` and `parts.division_cancel(session_id)` end it.

**Acceptance criteria.**
- [ ] Dividing a 10 m brick part with three vertical sketch lines 2.5 m apart and a 10 mm gap gives four pieces whose lengths are 2.495, 2.49, 2.49 and 2.495 m.
- [ ] Dividing the parts of a 4-storey wall by the intersecting levels with offset +50 mm gives one piece per storey; raising Level 3 by 300 mm moves that cut by 300 mm.
- [ ] A Complementary tongue-and-groove profile gives matching faces: the two pieces fused together have the same volume as the undivided part minus the gap volume, within 0.01%.
- [ ] Killing the process during a division session and reopening the file offers to restore the session, and restoring it reproduces the sketched curves exactly.

### 3.6.13 Merging, excluding, restoring and deleting parts
**ID:** ARC-PART-003 · **Priority:** P2 · **Ref:** research/02 §2.1 · web: Work with Merged Parts; Exclude Parts; Delete a Part

**Purpose.** Join pieces that will be built as one, take pieces out of the quantities without deleting them, and undo part creation.

**Workflow.**
- **Merge:** select two or more parts › Modify | Parts › Part › **Merge Parts**. To change a merge, select the merged part › **Edit Merged**, then **Add** or **Remove** parts and **Finish**. Edit Merged is also offered inside an Edit Division session when the division contains a merged part; the edit modes nest as deep as needed.
- **Exclude:** select parts › Modify | Parts › Exclude › **Exclude Parts**. **Restore:** hover an excluded part (it highlights), click it, and choose **Restore Parts** or click the *Restore excluded part* control shown next to it.
- **Delete:** select a part and press Delete. A warning says that the edit deletes parts and lists how many; *Delete Element(s)* confirms.

**Behaviour & rules.**
- **Merge conditions.** All parts must have the same material (after overrides) and must touch or overlap: every part must share a face area of at least 1 mm² or a common volume with another part in the set (connectivity over the whole set). Parts of different source elements may be merged. Failures: fewer than two parts ("Select at least two parts"), different materials ("Parts with different materials cannot be merged"), or not connected ("Merged part would have disconnected pieces").
- **Merge geometry.** `BRepAlgoAPI_Fuse` of the parts, then `ShapeUpgrade_UnifySameDomain` to remove internal faces. The merged part takes the first picked part's Mark, comments and overrides; the merged-away parts become hidden intermediates.
- **Later changes.** If a source change makes a merged part disconnected, the merge is kept as a multi-body part and a warning names it; the user can remove pieces from the merge. Removing parts until fewer than two remain dissolves the merge.
- **Edit Merged recovery.** The Edit Merged mode is an interactive tool (conventions §2.1) with the micro-operations `begin {merged_part_id}`, `add {ids}`, `remove {ids}`, `finish` and `cancel`; replaying them rebuilds an interrupted session.
- **Exclude.** An excluded part stays in the model but is not drawn (except when highlighted under the cursor or selected), is left out of schedules, material take-offs, area and volume totals, clash detection and IFC export, and does not cut or join. Dimensions to it are kept. Excluding a merged part excludes it as a whole. Restore reverses all of this.
- **Delete.** Deleting a part also deletes every part derived from it (its division descendants and any merge that contains it). Deleting one piece of a division deletes the whole division result (all its sibling pieces, listed in the warning) and reveals the parent part again. Deleting a first-level part deletes all parts of that source and reveals the source element; this is how Create Parts is undone later.

**Data (IFC mapping).** Merge definitions are stored in RXD document `model/parts` (kind `part.merge`); the excluded flag is in each part's RXD element data (`recipe.part`). A merged part is exported as one `IfcBuildingElementPart`. Excluded parts are not exported.

**Commands.**
- `parts.merge(part_ids) -> element_id`
- `parts.edit_merged(merged_part_id, add?: [element_id], remove?: [element_id])`
- `parts.exclude(part_ids)` and `parts.restore(part_ids)`
- `element.delete(element_ids[], confirm_dependents?=true) -> element_ids[]` (ARC-MOD-014) deletes parts and returns the descendants deleted with them.

**Acceptance criteria.**
- [ ] Merging three adjacent plasterboard pieces gives one part whose volume is their sum and which has no internal faces.
- [ ] Merging a brick piece with a block piece is refused with the material message.
- [ ] An excluded part does not appear in a part schedule or in the IFC export, and Restore brings it back with the same GlobalId.
- [ ] Deleting one of four pieces of a divided brick part deletes all four pieces and shows the undivided brick part again; deleting that part then removes every part of the wall and shows the wall.

### 3.6.14 Part properties, visibility and scheduling
**ID:** ARC-PART-004 · **Priority:** P2 · **Ref:** research/02 §2.2 · web: Part Instance Properties; Control Part Visibility and Appearance

**Purpose.** Give each part its own construction data (material, level, phasing, identity) and control in each view whether parts, their source elements or both are shown.

**UI.**
- **View property** *Parts Visibility* (Graphics group): **Show Parts** (default for new views; parts are drawn and selectable, sources are hidden and not selectable), **Show Original** (sources are drawn and selectable, parts hidden; the source cannot be picked by Create Parts in this mode), **Show Both** (both drawn and separately selectable). View templates can control it.
- **Object style.** *Parts* is a top-level model category with its own cut and projection line weights, colour, pattern and material, and can be overridden per view or per element (DOC chapter). Its cut pattern is the part's material cut pattern.
- **Shape handles.** With *Show Shape Handles* on, a selected part shows a handle on each planar face; dragging one moves that face along its normal (Edit Part Geometry). The offset is stored per face sub-reference.

**Behaviour & rules.**
- **Instance parameters:**
  - *Base Level* (default: the source's level; another level or **Non Associated**);
  - *Volume*, *Area* (the largest face), *Height*, and, for parts of walls, *Length* and *Width* (all read-only, from the part's solid);
  - *Comments*, *Mark*;
  - *Show Shape Handles*;
  - *Original Category*, *Original Family*, *Original Type* (read-only, from the source);
  - *Material By Original* (bool, default on) and *Material* (editable only when Material By Original is off);
  - *Construction* = **Core** or **Finish** (read-only: Core for layers between the core boundaries, Finish otherwise);
  - *Phase Created By Original* and *Phase Demolished By Original* (bool, default on) and *Phase Created* and *Phase Demolished* (editable only when the matching flag is off).
- **Overrides persist.** Material, phase and level overrides, Mark and Comments are kept through source regeneration as long as the part's identity exists (ARC-PART-001). Divided pieces inherit their parent's overrides; a merged part takes those of its first part.
- **Face offsets** (shape handles) are re-applied after each regeneration; an offset that would make the part invalid (zero or negative thickness) is clamped and reported.
- **Scheduling.** *Parts* can be scheduled and tagged like any category, with the parameters above. Excluded parts, hidden intermediates and merged-away parts never appear; a merged part appears once. Material take-offs use the part's material and volume.
- **Rebar.** Concrete parts can host reinforcement and carry rebar cover (STR chapter); precast splitting (STR chapter) uses this parts engine.

**Data (IFC mapping).** Parameters are written as property values on the `IfcBuildingElementPart`; Base Level gives its storey containment; the phase values follow the phasing rules (DOC chapter). Volume, Area, Length, Width and Height are written as an `IfcElementQuantity` named "BaseQuantities". Overrides and face offsets are stored in RXD element data (`recipe.part`). *Parts Visibility* is view data (RXD document `views/<viewId>`).

**Commands.**
- `parts.set_overrides(part_ids, material?: ByOriginal | material_id, base_level?: ByOriginal | level_id | NonAssociated, phase_created?: ByOriginal | phase_id, phase_demolished?: ByOriginal | phase_id | None)`
- `parts.offset_face(part_id, face_ref, distance)`
- `view.set_parts_visibility(view_id, mode: ShowParts|ShowOriginal|ShowBoth) -> ()`
- `element.set_param` for Mark, Comments and other writable parameters.

**Acceptance criteria.**
- [ ] Turning Material By Original off on one plaster part and choosing lime render changes only that part; changing the wall type's plaster material afterwards changes the other plaster parts but not that one.
- [ ] A view set to *Show Original* shows and selects the wall, not its parts; *Show Both* lets each be selected separately.
- [ ] A part schedule grouped by *Original Type* and *Construction* lists core and finish parts separately and omits excluded parts.
- [ ] Dragging a part's top face up 50 mm with its shape handle survives a later change of the wall's height (the face stays 50 mm above the new top).

---

## 3.7 Stairs, railings and ramps

### 3.7.1 Stair assembly model
**ID:** ARC-STAIR-001 · **Priority:** P0 · **Ref:** research/02 §8.1, §17.8 · screenshots/architecture/stair_t_shaped_component_assembly.png

**Purpose.** A stair is an assembly of related components that is edited in a dedicated assembly mode.

**Behaviour & rules.**
- **Components:**
  - **Runs**: straight, full-step spiral, centre-ends spiral, L-shaped winder, U-shaped winder or sketched;
  - **Landings**: automatic, picked from two runs, or sketched;
  - **Supports**: stringers and carriages, created automatically from the type or by Pick Edges;
  - **Railings**: generated on Finish.
- **Relationship graph** (inside the stair recipe):
  - Each run knows its `relative_base_height` and `relative_top_height` within the stair.
  - Runs that are connected through a landing share that landing's height.
  - When the user removes risers from one run, the next connected run gains the same number, so the total riser count stays equal to `N_desired`. Reviera applies this rebalancing to the *downstream* run along the stair path, or to the upstream run if no downstream run exists.
  - Automatic and picked landings reshape when their runs move. Sketched landings do not.
- **Arrangements.** Overlapping and T-shaped arrangements (several runs meeting one landing) are allowed.
- A stair cannot be a member of an assembly element.
- **Single editing path.** There is no separate legacy "stair by sketch" tool. Sketching happens through *Create Sketch* inside the assembly.

**Data (IFC mapping).**
- **`IfcStair`** with an **`IfcStairType`** aggregates (`IfcRelAggregates`):
  - **`IfcStairFlight`** per run (`PredefinedType` = `STRAIGHT`, `WINDER`, `SPIRAL`, `CURVED` or `FREEFORM`), with `NumberOfRisers`, `NumberOfTreads`, `RiserHeight` and `TreadLength` set (IFC4 occurrence attributes) and `Pset_StairFlightCommon`;
  - **`IfcSlab`** with `PredefinedType = LANDING` per landing;
  - **`IfcMember`** with `PredefinedType = STRINGER` per support (carriages are `USERDEFINED`, ObjectType "CARRIAGE");
  - **`IfcRailing`** per generated railing.
- **`IfcStair.PredefinedType`** is classified from the arrangement: one straight run → `STRAIGHT_RUN_STAIR`; two straight runs in line → `TWO_STRAIGHT_RUN_STAIR`; an L shape with a landing → `QUARTER_TURN_STAIR`; an L winder → `QUARTER_WINDING_STAIR`; a U shape with a landing → `HALF_TURN_STAIR`; a U winder → `HALF_WINDING_STAIR`; a spiral → `SPIRAL_STAIR`; a curved run → `CURVED_RUN_STAIR`; two runs meeting one landing from opposite sides → `DOUBLE_RETURN_STAIR`; otherwise `USERDEFINED`.
- `Pset_StairCommon` holds NumberOfRiser, NumberOfTreads, RiserHeight, TreadLength, RequiredHeadroom where it is set, and IsExternal.
- The full recipe is stored in RXD element data (`recipe.stair`).

**Acceptance criteria.**
- [ ] A U-shaped stair (two runs and a landing) exports `IfcStair(HALF_TURN_STAIR)` aggregating two `IfcStairFlight`, one `IfcSlab(LANDING)`, its supports and its railings.

### 3.7.2 Creating stairs by component
**ID:** ARC-STAIR-002 · **Priority:** P0 · **Ref:** research/02 §8.2 · screenshots/architecture/stair_direct_manipulation_controls.png

*Owner clarification (2026-10-13): both **simple** stairs (straight runs with landings: straight, L and U arrangements) and **spiral** stairs (full-step and centre-ends) are required.*

**Implementation notes (2026-10-13), ARC-STAIR-001…005.**
- As built:
  - `stair.create(type_id?, base_level, top?, base_offset?, top_offset?, desired_risers?, tread_depth?, width?, runs[{start, end?, center?, ccw?, width?, risers?}])` and `stair.set_params(stair_ids[], base_level?, base_offset?, top?, top_offset?, desired_risers?, tread_depth?, width?, rebalance?)`.
  - A run with `center` is a spiral about it, its walking line through `start`. It turns toward `end`, or with no `end` it places every remaining riser (full-step).
  - The top defaults to the next level up.
- Calculation follows ARC-STAIR-003, with *Begin and End with Riser* on: `N = ceil(H / R_max)` or the desired count, and `k − 1` treads per run of `k` risers. Runs keep their riser counts, and `stair.set_params` gives the last run the difference.
- Geometry:
  - **Straight runs** are monolithic: the profile of the steps, with a smooth underside at the structural depth square to the pitch line, pushed across the run's width.
  - **Spiral treads** are annular sectors (wedges when the inner radius reaches the centre), one tread depth along the walking line each. Each is a solid down by a riser and the structural depth; the first stands on the floor.
  - **Automatic landings** are the convex hull of the adjoining run ends (extended by the width at turns), at the shared height and the type's landing thickness.
- The type is "Cast-In-Place Stair" (`IfcStairType`): maximum riser 180 mm, minimum tread 280 mm, minimum width 1 m, structural depth 150 mm, landing 200 mm.
- **IFC:** `IfcStair` with its predefined type from the arrangement (`SPIRAL_STAIR` for a spiral); one faceted B-rep per run, landing or spiral tread, contained in the base level. Aggregated flights and landings come later.
- **Window:**
  - **Stair (ST)**: click each run's start and end, with the riser countdown; landings join the runs.
  - **Spiral Stair**: click the centre, then a point on the walking line.
  - Properties edit the desired risers, tread depth and width, and show the actual values and warnings.
- **Later:** winders, centre-ends spiral by clicks, assembled runs and nosings, supports, direct-manipulation handles, multistory stairs, and plan symbols above the cut. Generated railings: built (ARC-RAIL-001, on stairs).

**Workflow.**
1. The user starts Architecture › Circulation › **Stair** in a plan or 3D view. Tiling a plan view and a 3D view side by side is recommended.
2. In the Components panel, **Run** is selected. The Draw gallery offers Straight, Full-Step Spiral, Center-Ends Spiral, L-Shape Winder, U-Shape Winder and **Create Sketch**.
3. The user sets the options:
   - **Location Line**: Exterior Support: Left, Run: Left, Run: Center, Run: Right or Exterior Support: Right, relative to the up direction;
   - **Offset**: positive to the right of the path;
   - **Actual Run Width**: the width without supports;
   - **Automatic Landing**: on by default.
4. The user picks the stair type (Assembled, Cast-in-Place or Precast) and sets the *New Stairs* and *New Stairs: Runs* properties.
5. Tools › **Railing**: the user chooses a railing type or *None*, and a position of **Treads** or **Stringer**.
6. The user draws runs:
   - **Straight**: click a start point. The tooltip shows how many risers have been created and how many remain. Click an end point. To make a landing, click when the run has the risers wanted, then click further along the extension line to start the next run. The landing depth defaults to the run width.
   - **Full-Step Spiral**: click the centre, then the radius. Every remaining riser is placed counter-clockwise; the stair may exceed 360°; **Flip** reverses the direction.
   - **Center-Ends Spiral**: click the centre, the start and the end (less than 360°).
   - **L or U winder**: one click places the whole winder. *Mirror Preview* and Spacebar change the turn. The winder snaps to walls. Direct-manipulation controls balance the legs, and *Parallel Treads at Start/End* replace winder treads with parallel ones.
7. **Landings**: *Pick Two Runs* (two runs in the same session whose end and start heights match) or *Create Sketch*.
8. **Supports**: *Pick Edges* (Tab picks a continuous boundary, with mitered joins). The stair type must have a side support type.
9. **Finish Edit Mode** generates the railings.

**UI.**
- **Other assembly-mode tools:** **Flip** (reverses the up direction), **Convert** (turns a component into a sketched component; one-way, after a confirmation), **Edit Sketch**, **Connect Levels** (multistory; *Align with* Mid-Landing or Run-Start) and Railing.
- **Direct-manipulation controls** in plan:
  - run-end arrows: drag to add or remove risers; the next run is rebalanced;
  - run-width arrows on both sides;
  - riser-count controls at both ends;
  - winder leg-balance dots;
  - spiral radius and angle handles;
  - landing edge arrows (automatic landings).

**Commands.**
- `stair.create(type_id, base_level, top_level | Unconnected(height), base_offset?, top_offset?, desired_risers?, tread_depth?, runs: [RunSpec], landings?: [LandingSpec], supports?: [SupportSpec], railing?: {type_id | None, position: Treads|Stringer}) -> element_id`
  - `RunSpec = {kind: Straight{start,end} | FullSpiral{center,radius,start_angle,ccw} | CenterEndsSpiral{center,start,end} | LWinder{corner, leg1, leg2, mirror} | UWinder{...} | Sketch{left: [curve], right: [curve], risers: [curve], path?: [curve]}, location_line, offset, width, begin_with_riser, end_with_riser, risers?}`
  - `LandingSpec = Auto | PickRuns(a,b) | Sketch{loops, path?}`
- `stair.edit(stair_id) -> session_id` (opens an assembly session)
- `stair.add_run(session_id, RunSpec) -> key`
- `stair.add_landing(session_id, LandingSpec) -> key`
- `stair.add_support(session_id, edge_refs, side) -> key`
- `stair.set_run_risers(session_id, run_key, n)`
- `stair.flip(session_id)`
- `stair.convert_to_sketch(session_id, component_key)`
- `stair.finish(session_id)`
- `stair.cancel(session_id)`

**Acceptance criteria.**
- [ ] A straight stair from Level 1 (0.0) to Level 2 (3.0) with Maximum Riser 0.18 creates 17 risers of 0.17647 m, and the tooltip counts down to 0.
- [ ] Clicking after 9 risers, then continuing in line, gives a landing whose depth equals the run width, and a second run of 8 risers.
- [ ] Dragging the end arrow of run 1 to remove 2 risers adds 2 risers to run 2, and the total height is unchanged.

### 3.7.3 Stair calculation rules
**ID:** ARC-STAIR-003 · **Priority:** P0 · **Ref:** research/02 §8.4

**Behaviour & rules.**
- **Inputs:**
  - `H = z_top − z_base`, where `z_top` is the top level plus its offset (or the *Desired Stair Height* when the top is unconnected);
  - the type's **Maximum Riser Height** `R_max`, **Minimum Tread Depth** `T_min` and **Minimum Run Width** `W_min`;
  - the instance's **Desired Number of Risers** `N_d` and **Actual Tread Depth** `T`.
- **Derivation:**
  1. `N_min = ceil(H / R_max − 1e-9)`. The default `N_d` is `N_min`. If the user enters `N_d < N_min`, Reviera warns "Riser height exceeds maximum", and the value is allowed but flagged.
  2. Actual riser height `R = H / N_d`.
  3. **Treads per run.** For a run with `k` risers, *Begin with Riser* `B` and *End with Riser* `E`: `treads = k − 1 + (1 − B) + (1 − E)`. Each flag that is off adds a tread at that end, while the riser count stays the same.
  4. Straight run length = `treads · T`.
  5. **Winder treads.** The user's tread depth `T` is applied at the **walking line**, which lies at the path offset (by default the run's centre line). Inner and outer tread widths follow the winder geometry (ARC-STAIR-004).
  6. **Spiral runs.** The angle per tread is `φ = T / r_w`, where `r_w` is the walking-line radius. Total angle = `treads · φ`.
- **Stair Calculator** (optional, on the type): the rule `a ≤ m·R + n·T ≤ b`, with user-set `m`, `n`, `a` and `b`. The default is `m = 2`, `n = 1`, `a = 0.60` m and `b = 0.65` m. If the rule is not met, Reviera shows a warning, not an error.
- **Validation warnings:** `R > R_max`, `T < T_min`, run width `< W_min`, and "Not enough risers placed" when a run session is finished with risers remaining. In that last case the stair is still created, and its top is not connected.
- **Multistory stairs.** *Connect Levels* (or *Multistory Top Level*) repeats the stair between each pair of consecutive selected levels. The repetitions are generated copies. If the story heights differ, each copy recomputes `N` and `R` for its own story while keeping the same run layout. A copy whose layout cannot fit gets its own warning.

**Commands.** `stair.set_rules(type_id, max_riser, min_tread, min_width, calculator?: {m,n,min,max})` and `stair.connect_levels(stair_id, level_ids, align_with: MidLanding|RunStart)`.

**Acceptance criteria.**
- [ ] With H = 2.8 m, R_max = 0.19 and B = E = true, the stair has 15 risers of 0.18667 m and 14 treads.
- [ ] Turning *End with Riser* off adds one tread and leaves the riser count at 15.
- [ ] A multistory stair connecting levels with story heights of 3.0 m and 3.3 m has 17 risers in the first copy and 19 in the second, with R_max = 0.18.

### 3.7.4 Stair geometry generation
**ID:** ARC-STAIR-004 · **Priority:** P0 · **Ref:** research/02 §8.2–§8.4, §17.8

**Behaviour & rules.** Each component is generated independently (in parallel), using its own frame: s along the path, w across the width and z up.
1. **Straight monolithic run** (cast-in-place):
   - Build the side profile in the (s, z) plane:
     - a step polyline: riser `R` (vertical, or slanted by the *Slanted* riser setting) and tread `T` + nosing length `ν` (each tread overhangs the riser below by ν);
     - the underside: for *Smooth*, a line parallel to the pitch line (through the tread nosings) at a perpendicular distance of **Structural Depth**; for *Stepped*, a copy of the steps offset downward by the structural depth.
   - Close the profile at the bottom with a horizontal line to the floor and at the top with a vertical line.
   - Extrude it across the width (`BRepPrimAPI_MakePrism`).
   - The nosing profile (if any) is swept along the front edge of each tread and fused.
2. **Straight non-monolithic run** (assembled):
   - each tread is a prism of the tread profile (a rectangle `T + ν` by tread thickness, or a tread-profile family);
   - each riser is a plate between consecutive treads;
   - *Apply Nosing Profile* (Front only, Front and Left, Front and Right, or Front, Left and Right) sweeps the nosing profile along the matching tread edges.
3. **Winder runs:**
   - The walking line `γ` is the path at the walking offset.
   - Tread boundaries are placed every `T` along `γ`.
   - Inside the turn, each boundary is a line through its point on `γ`, at an angle that varies **linearly** from parallel (at the ends of the parallel-tread zones) to radial through the inner corner point (at the turn's middle). This "balanced" distribution gives a positive inner tread width wherever the minimum inner width (1/3 of `T`, configurable) allows.
   - If the minimum inner width cannot be met, Reviera warns "Winder inner width below minimum".
   - Each tread is the polygon between consecutive boundaries and the run's side lines, extruded by the tread thickness at height `i·R`.
4. **Spiral runs.** Each tread is an annular sector `[r_in, r_out] × [θ_i, θ_i + φ]`, built with `BRepPrimAPI_MakeRevol` of a rectangle about the centre axis or as a prism of the sector face, at height `i·R`. A monolithic underside is a helicoidal ruled surface between the inner and outer helices offset downward by the structural depth. It is built with `BRepOffsetAPI_ThruSections` through radial section lines every 5°.
5. **Landings.**
   - An automatic landing between two runs is the convex hull of the end edge of the incoming run and the start edge of the outgoing run, extended so that its depth along each run equals at least the run width.
   - A landing between collinear runs is a rectangle (width × run width).
   - Its top is at the shared height, and its thickness comes from the landing type (*Monolithic*, or *Use Same Tread/Riser Settings as Run*).
   - The solid is a prism of the outline (`BRepPrimAPI_MakePrism`, downward).
   - A sketched landing uses its sketch loop.
6. **Supports:**
   - **Stringer (closed):** a plate in the (s, z) plane, between lines parallel to the pitch line at the support's top and bottom offsets. It is trimmed horizontally at the floor and the top level (`BRepAlgoAPI_Cut` with half-spaces) and positioned at the side with the lateral offset. On winders and spirals, the support is swept along the side curve (`BRepOffsetAPI_MakePipeShell`) and lifted along a helical law.
   - **Carriage (open):** a profile placed under the treads, either sawtooth or straight.
   - **Middle supports:** `n` carriages spaced evenly across the width.
   - **Picked supports:** the profile is swept along the picked edge chain, with miter joins.
7. **Cut by levels.** Every component is cut by the half-space below `z_base − 1 mm`, so that nothing hangs below the base floor.
8. **Plan representation** (DOC chapter): the cut line and the parts above the cut are drawn dashed, by subcategory. The stair path, up/down text and tread and riser numbers are annotation.

**Acceptance criteria.**
- [ ] A cast-in-place straight run with structural depth 150 mm has an underside parallel to the pitch line, at a perpendicular distance of 150 mm from the nosing line.
- [ ] An L winder with three winder treads has walking-line tread depths equal to `T` within 0.1 mm, and every inner tread width is greater than 0.
- [ ] The volume of a full-step spiral with 16 treads matches the analytic sector volume within 0.5%.

### 3.7.5 Stair parameters
**ID:** ARC-STAIR-005 · **Priority:** P0 · **Ref:** research/02 §8.3, §8.4

| Level | Parameters |
|---|---|
| **Stair type** | Calculation Rules (Maximum Riser Height, Minimum Tread Depth, Minimum Run Width, Calculator); Construction (Run Type, Landing Type, Function); End Connection (precast: Straight Cut or Notch, with notch dimensions); Supports (Right/Left Support = None, Stringer (Closed) or Carriage (Open); Right/Left Support Type; Right/Left Lateral Offset; Middle Support, its type and count); identity data |
| **Stair instance** | Base Level, Base Offset, Top Level (default: the next level, or *Unconnected*), Top Offset, Desired Stair Height, Multistory Top Level and connected levels, **Desired Number of Risers**, Actual Number of Risers (RO), **Actual Riser Height** (RO), **Actual Tread Depth**, Tread/Riser Start Number, phasing |
| **Run type** (Monolithic or Non-Monolithic) | Underside Surface (Smooth or Stepped), Structural Depth, Monolithic/Tread/Riser Material, **Tread** (on/off, thickness, profile, **Nosing Length**, nosing profile, Apply Nosing Profile), **Riser** (on/off, **Slanted**, thickness, profile, Riser to Tread Connection) |
| **Run instance** | Location Path Justification, Relative Base Height, Relative Top Height, Run Height, **Actual Run Width**, **Begin with Riser**, **End with Riser**, Actual Number of Risers and Treads, Actual Riser Height, Actual Tread Depth, Parallel Treads at Start/End (winders) |
| **Landing type** | Monolithic or Non-monolithic, Thickness, Material, Use Same Tread/Riser Settings as Run |

**Sketched components.** *Run › Create Sketch* takes **Boundary** curves (left and right, which must not touch; each may have several segments), **Riser** curves, and an optional **Stair Path**. A sketched run has no automatic relationships. A monolithic stair with zero tread thickness is allowed.

**Data (IFC mapping).** The run type and landing type are `IfcStairFlightType` and `IfcSlabType` (`LANDING`), and their recipes are stored in RXD type data (`recipe.stair_type`).

**Acceptance criteria.**
- [ ] A sketched run with a curved left boundary and a straight right boundary generates treads between its riser curves, and keeps them when the stair width parameter changes.

### 3.7.6 Stair annotation
**ID:** ARC-STAIR-006 · **Priority:** P1 · **Ref:** research/02 §1 (2027), §8.4

**Behaviour & rules.**
- **Stair path** (Stair Path type: up and down arrows, start and end symbols).
- **Cut mark** (single or double zig-zag, set by type).
- Up/down text.
- **Tread/riser numbering** (the *Number Tread/Riser* tool). The numbers have **type properties**: font, size, bold, italic, underline, width factor, background, and independent display above and below the cut line. They have their own visibility categories and work with *Tag All Not Tagged*.
- **Rule-Based Numbering** (Manage › Numbering) is specified in Chapter 2 §2.10.16 (DM-NUM-001).
- The *editing colour* used in assembly mode can be configured.

These are view annotation and are specified in the DOC chapter. This chapter requires that the stair recipe expose, for each tread and riser, a stable key, its nosing line, its centre point and its number.

**Commands.** `stair.number_treads(view_id, stair_id, start_number?, type_id?) -> element_id`.

**Acceptance criteria.**
- [ ] Tread numbers keep their values after a run is moved. They change only when the number of treads changes.

### 3.7.6a Stairs through floors: completing to the level above, openings and railings
**ID:** ARC-STAIR-007 · **Priority:** P1 · **Ref:** owner requirement (2026-10-14); Reviera's own design (Appendix A.2a); headroom by DM-CODE-001

**Purpose.** A stair reaches the level above without counting risers, and the floor or ceiling it passes under is opened to the code's headroom, with the opening editable. *(Owner requests, 2026-10-14: "modify to have it automatically extend, and when encountering a slab/ceiling/etc. intelligently offer to create the opening, and the opening needs to have adjustment handles. If a banister attached to the stairs it should also auto-extend"; "When offering to create the opening reference well known standards for clearances required for typical construction codes"; "Stairs extend should be able to 'snap' to the level above and intelligently complete the stairs.")*

**Behaviour & rules.**
- **Snapping to the level above.** While a run is drawn, a length within one tread of the one that places every riser left snaps to it. The status bar says the stair reaches the level above, and that click makes the stair.
- **Completing.** Ending the tool early (Esc) completes the stair: the last run carries straight on with the risers left, and the preview shows that before Esc.
- **Extending as the levels move.** A stair made this way (`auto_extend`, the default for new stairs) keeps reaching its top: when a level moves or the riser count changes, the last run takes the difference. Railings on the stair follow its runs as they are, so they extend with it.
- **Headroom check.** Every Reviera floor and ceiling above the stair's base and lower than its top plus the headroom is checked. Where it leaves less than the headroom over a tread (measured from the nosing line, taken at the next nosing's height) or a landing, the region over those treads and landings, the stair's full width, is the opening it needs. Floors already opened there are not short.
- **The offer.** After the stair is made (and from **Stair Openings** on the stair's contextual tab), the floors that are short are named, with their least headroom or "in the stair's way", and the choice is the model's code (first, as the default) or another code, each with its headroom and clause, or not to open. The opening is made in each short floor (`stair.create_openings`); running it again reshapes the stair's own openings.
- **The opening** is a vertical opening (ARC-OPEN-002) named *Stair Opening* that records the stair, the headroom and the clause. In plans of its floor's level it is drawn crossed corner to corner; selecting it shows grips at its corners and edge middles, and dragging them reshapes it (`opening.set_loops`).
- Guards around the opening at the upper floor are not added yet; a railing by Pick Edges (ARC-RAIL-001) can follow its edges.

**Commands.** `stair.create(…, auto_extend?)` · `stair.set_params(…, auto_extend?)` · `stair.headroom_check(stair_id, code?, headroom?) -> {headroom, clause, obstructions[]}` · `stair.create_openings(stair_id, code?, headroom?) -> opening_ids` · `opening.set_loops(opening_id, loops)`.

**Acceptance criteria.**
- [x] A single run drawn 2 m long from Level 1 (0) to Level 2 (3 m) has 17 risers placed and no warning; raising Level 2 to 3.6 m gives 20 risers and its railings reach 0.84 m further. *(Built 2026-10-14.)*
- [x] Under a 300 mm floor at Level 2, the IBC opening covers the 14 treads whose next nosing is above 0.668 m (14 × 280 mm × 1.2 m); Canada's 1950 mm opens less; after opening, the check finds nothing left to open.
- [x] Drawing a run to within a tread of the top shows "reaches Level 2" and makes the stair on the click; the offer then makes the opening, which shows crossed in the Level 2 plan and widens when its edge grip is dragged.

### 3.7.7 Railings
**ID:** ARC-RAIL-001 · **Priority:** P0 · **Ref:** research/02 §8.5, §17.9

**Purpose.** Railing systems, either free-standing or hosted on stairs, ramps, floors, slab edges, wall tops, roofs or toposolids.

**Workflow.**
- **Sketch Path.** The user starts Architecture › Circulation › Railing › **Sketch Path**.
  1. Optionally, the user uses *Pick New Host*; otherwise the railing starts on the current level.
  2. The user sketches **one connected chain**, open or closed. On stairs, the chain is sketched along the inside of the stringer.
  3. Canvas controls: *Toggle Sketch Orientation* (swaps start and end) and, for closed loops, *Move Beginning to Next Corner*.
  4. **Finish**.
- **Place on Stair/Ramp.** The user picks a stair or ramp, and the railing is hosted on the **Treads** or the **Stringer**.
- **Pick Edges** (patios, decks, balconies and flat roofs; *owner clarification, 2026-10-13*):
  - The user picks open edges of a floor, slab, deck (ARC-DECK-001), flat roof or toposolid sub-region. **Tab** picks the whole perimeter chain, and walls along an edge break it.
  - The railing follows the picked edges, set in by the *Edge Setback* (default 50 mm, or the deck's fascia for decks).
  - Its base sits on the host's top surface (on a flat roof, on the roof's top or its parapet). Its path is kept in step with the host: when the host's outline changes, the railing follows.
  - *Guard all open edges* places one railing along every edge of the host that has no wall, and leaves openings at the stairs that land on the host.
- Railings are also created automatically with stairs and ramps (ARC-STAIR-002, ARC-RAMP-001).

**Behaviour & rules.**
- **Per segment** (Edit Path): *Height Correction* = By Type or a custom value; *Slope* = **By Host**, **Flat** or **Sloped**.
- **Type parameters:**
  - **Railing Height**: the height of the top-most rail;
  - **Rail Structure (Non-Continuous)**: rows of Name, Height, Offset, Profile and Material;
  - **Baluster Placement** (ARC-RAIL-002);
  - **Baluster Offset**;
  - **Use Landing Height Adjustment** and the amount;
  - **Angled Joins**: Add Vertical/Horizontal Segments or Add No Connector;
  - **Tangent Joins**: Add Vertical/Horizontal Segments, No Connector or Extend Rails to Meet;
  - **Rail Connections**: Trim or Weld;
  - **Top Rail**: height and type;
  - **Handrail 1 and Handrail 2**: type and **Position** (Left, Right, Left and Right, or None).
- **Instance parameters:** Base Level, Base Offset, **Offset from Path**, Tread/Stringer Offset (on stairs), Length and phasing.
- **Path elevation** for each segment:
  - *By Host*: the path follows the host surface. On stairs, the path height at a point on a run is the height of the **nosing line** under it. On landings it is the landing's top. When *Use Landing Height Adjustment* is on, the landing value is shifted by the adjustment.
  - *Flat*: constant at the segment's start height.
  - *Sloped*: linear between the segment's two end heights.
- **Transitions** between a sloped and a flat segment follow the join rules: *Add Vertical/Horizontal Segments* inserts a vertical jog or a horizontal extension so that rails meet; *No Connector* leaves a gap; *Extend Rails to Meet* (tangent joins) extends the rails to their intersection.
- **Non-continuous rails.** Each rail row is the path offset horizontally by the row's offset plus *Offset from Path*, lifted by the row's height above the path elevation. The rail profile is swept along the offset path with `BRepOffsetAPI_MakePipeShell` (auxiliary +Z binormal on flat segments; a Frenet frame is avoided so that the profile does not twist). At each segment join, rails are trimmed (*Trim*) or fused (*Weld*).
- **Continuous rails** (Top Rail, Handrail 1 and Handrail 2) are sub-elements, selectable with Tab.
  - **Type parameters:** Construction (Default Join: Fillet or Miter; Fillet Radius; Hand Clearance; Profile; Projection; Transitions: Gooseneck, None or Simple); Extensions at the beginning (bottom) and end (top) (Style: Wall, Floor or Post; Length; Plus Tread Depth); Terminations; Supports (family; Layout: Fixed Distance, Align with Posts, Fixed Number, Maximum Spacing or Minimum Spacing; Spacing; Justification).
  - **Editing:** *Edit Rail* (edits the path for extensions) and *Edit Rail Joins* (per join: By Type, Fillet, Miter or Flush).
  - **Generation:** the continuous path is built and then filleted at joins with the fillet radius (`Geom2dAPI` fillet in plan, then lifted). Extensions are appended, the profile is swept, and support families are placed along the path by the support layout. The layout uses the same algorithm as the curtain grid solver (ARC-CW-003) applied to path length.

**Data (IFC mapping).**
- **`IfcRailing`** (`PredefinedType = GUARDRAIL`, or `HANDRAIL` when the type has no balusters, or `BALUSTRADE` when balusters are densely spaced; the user can override this) with an **`IfcRailingType`**. It aggregates:
  - continuous rails as nested **`IfcRailing(HANDRAIL)`** (4X3: `IfcRail` is not used, because it belongs to railway rails);
  - non-continuous rails as **`IfcMember`** (`USERDEFINED`, ObjectType "RAIL");
  - balusters as **`IfcMember`** (`USERDEFINED`, ObjectType "BALUSTER");
  - posts as **`IfcMember(POST)`**;
  - supports as **`IfcMember`** (`USERDEFINED`, ObjectType "RAILSUPPORT").
- The railing is related to its host through `IfcRelConnectsElements` (Name "HostedBy") and, for stairs, through aggregation into the `IfcStair`.
- `Pset_RailingCommon` holds Height, IsExternal and Diameter (for round handrails).
- The path is stored in RXD element data (`sketch`), and the rail and baluster tables in RXD type data (`recipe.railing_type`).

**Commands.**
- `railing.create(type_id, path: [curve], host_ref?, base_level?, base_offset?, offset_from_path?, segment_slopes?: [ByHost|Flat|Sloped], height_corrections?: [f64|ByType]) -> element_id`
- `railing.place_on_host(type_id, host_id, position: Treads|Stringer) -> [element_id]`
- `railing.create_on_edges(type_id, host_id, edges?: [curve_key] | all_open, setback?) -> [element_id]` (Pick Edges; with no edges, every open edge)
- `railing.edit_path(railing_id, path, ...)`
- `railing.flip_path(railing_id)`
- `railing.set_rail_join(railing_id, join_ref, mode: ByType|Fillet|Miter|Flush)`
- `railingtype.set_rails(type_id, rows)`
- `railingtype.set_continuous_rail(type_id, which: Top|Handrail1|Handrail2, props)`

**Acceptance criteria.**
- [ ] A railing on a straight stair run with By Host follows the nosing line at a constant vertical height. On the landing it is flat, and the join is made with a vertical segment when Angled Joins = Add Vertical/Horizontal Segments.
- [ ] A top rail with Fillet joins of radius 50 mm has continuous tangents at every corner.
- [ ] A handrail with Position = Left and Right on a two-run stair produces two handrails, with wall extensions of the configured length at the bottom and the top.
- [ ] *Guard all open edges* on an L-shaped deck against a house wall puts railings on the four edges away from the wall and none along it. It leaves a gap where the deck stair lands, and the railings follow when the deck's outline is edited.
- [ ] A railing picked along a flat roof's perimeter sits on the roof's top, set in by its setback, with posts at the corners.

**Implementation notes (2026-10-13), ARC-RAIL-001/002.**
- As built:
  - `railing.create(type_id?, path, closed?, base_level, base_offset?)` (Sketch Path).
  - `railing.create_on_edges(type_id?, host_id, edges?, setback?)` (Pick Edges and Guard all open edges) for floors, decks and flat roofs.
- **Hosted paths:** a hosted railing keeps its host and the host's curve keys. Its path is those curves moved in by the setback and met at the corners, at the host's top, so it follows the host's outline.
- **Open edges:** an edge is open when no Reviera wall runs along it (parallel, its centreline within half its width plus 150 mm). Each run of open edges is one railing; a whole loop is a closed one.
- **Default type** "Guard Rail - 1000mm" (`IfcRailingType`): 1 m to the top of a 50 × 50 mm top rail, a 40 × 40 mm bottom rail 80 mm up, 20 mm balusters at 120 mm, and 80 mm posts reserving 50 mm on each side.
- **Balusters** follow ARC-RAIL-002 with one pattern row:
  - the pattern breaks at every segment end, is justified at its beginning, and has no excess fill;
  - posts go at the start, the end and every corner, and remove the balusters within their reserved space.
  - The specified 3.05 m case gives 24 balusters.
- **IFC:** an `IfcRailing` (GUARDRAIL) with one faceted B-rep per rail, post and baluster, contained in its level or its host's. Deleting the host or the level deletes the railing.
- **Window:** Architecture › Railing (click the points; the first point closes the path; Esc finishes) and Modify | Floors / Roofs › Guard Edges.
- **On stairs** (Place on Stair, Treads):
  - `railing.create_on_stair(stair_id, sides?, inset?, type_id?)`; `stair.create` places railings on both sides unless `railings: false`, and Modify | Stairs › Railings adds them to a selected stair.
  - The path is the stair's side, `inset` (50 mm) in from its edge, along the nosing line: from the first nosing to the top.
  - On a landing it is flat at the landing's height. At a turn it goes round the landing's corner on both sides; a half turn goes round the outside and straight across the well on the inside.
  - Rails follow the sloped path at the type's heights, measured plumb above the nosing line. Posts and balusters stand plumb.
  - The railing follows the stair (its runs, levels and moves) and is deleted with it. It is contained in the stair's base level; aggregation into the `IfcStair` comes later.
- **Later:** continuous handrails and their extensions, By Host heights per segment, angled joins (vertical/horizontal segments), baluster families and multi-row patterns, stair gaps in guarded edges, and editing paths.

### 3.7.8 Baluster placement
**ID:** ARC-RAIL-002 · **Priority:** P0 · **Ref:** research/02 §8.5 (Edit Baluster Placement), §17.9

**Purpose.** A deterministic algorithm for placing balusters and posts.

**Behaviour & rules.**
- **Main pattern rows.** Each row has: Name, Baluster Family (or None), **Base** (Host, Top of Rail Structure or a rail name), Base Offset, **Top** (Top Rail Elevation or a rail name), Top Offset, **Distance from previous**, and Offset (perpendicular to the path).
- **Pattern settings:**
  - **Pattern Length** `P` = the sum of all *Distance from previous* values, plus the distance from the last row to the end of the pattern;
  - **Break Pattern at**: Each Segment End, Angles Greater Than `α`, or Never;
  - **Justify**: Beginning, End, Center or Spread Pattern to Fit;
  - **Excess length fill**: None, Truncate, Repeat Pattern, or Spacing (with a baluster family and spacing value).
- **Posts:** Start, Corner and End post rows (family, base, top and their offsets, **Space** and Offset). **Corner Posts At**: Each Segment End, Angles Greater Than or Never.
- **Stairs:** **Use Baluster Per Tread On Stairs**, **Balusters Per Tread** `b` and its family. On stair runs this overrides the main pattern.
- **Algorithm.**
  1. Split the path into **pattern runs** at break points: segment ends, or joins whose deflection angle is greater than `α`, according to *Break Pattern at*. Every length `s` is measured along the path in plan.
  2. Place posts. A start post goes at s = 0 of the path. An end post goes at s = L (open paths only). A corner post goes at each break point that meets the *Corner Posts At* rule. Each post at position `s_p` reserves `[s_p − Space, s_p + Space]`.
  3. For each pattern run of length `Λ` (between `s_a` and `s_b`):
     - **Beginning:** the pattern origin is `s_a`. Instances are placed at `s_a + kP + d_i` while `< s_b`.
     - **End:** the same, mirrored from `s_b`.
     - **Center:** the origin is `s_a + (Λ − ⌊Λ/P⌋·P)/2`.
     - **Spread Pattern to Fit:** `m = max(1, round(Λ/P))`, `P' = Λ/m`, and every distance in the pattern is scaled by `P'/P`. Excess fill does not apply.
     - **Excess length fill** (for the leftover `r = Λ − ⌊Λ/P⌋·P`): *None* leaves `r` empty. *Truncate* places pattern rows from the start of the pattern while they fit within `r`. *Repeat Pattern* places one more full pattern starting in `r` and keeps the rows that fall before `s_b`. *Spacing* fills `r` with the excess-fill family at the given spacing, using Center justification within `r`.
  4. Remove every baluster that falls inside a post's reserved interval.
  5. **Per-tread override** on stair runs (when enabled): for each tread `i`, with its nosing at `s_i` and depth `T`, place `b` balusters at `s_i + T·(j − ½)/b` for j = 1…b, measured from the nosing toward the next riser. Then apply the baluster offset.
  6. **Heights.** Each baluster runs from its Base reference (the host surface height at `s`, from the path elevation; or the height of the named rail) plus the base offset, up to its Top reference plus the top offset. The family is stretched to that length. Where the top reference is sloped, the family's top is cut by the plane of the rail's underside (`BRepAlgoAPI_Cut`).
  7. Balusters are instances of their family, written as mapped items in IFC.

**Commands.** `railingtype.set_baluster_pattern(type_id, rows, break_at, angle?, justify, excess_fill, excess_family?, excess_spacing?)`, `railingtype.set_posts(type_id, start, corner, end, corner_at, angle?)` and `railingtype.set_per_tread(type_id, enabled, count, family_id)`.

**Acceptance criteria.**
- [ ] A 3.05 m straight path with a one-row pattern (Distance from previous = 0.12 m, so P = 0.12 m), Beginning justification, Excess fill = None, and start and end posts with Space 0.05 m: the candidate positions are 0.12, 0.24 … 3.00 (25 positions). The one at 3.00 lies inside the end post's reserved interval [3.00, 3.10] and is removed, leaving 24 balusters.
- [ ] Spread Pattern to Fit on a 3.05 m path with P = 0.12 gives 25 patterns of 0.122 m.
- [ ] With 2 balusters per tread on 0.28 m treads, balusters sit 0.07 m and 0.21 m behind each nosing.

### 3.7.8b Railing openings and gates
**ID:** ARC-RAIL-003 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** Openings and gates in a railing, added the way dormers are added to a roof. The user lays out the railing first, then selects it and adds features through its Modify tab. *(Owner clarifications, 2026-10-13: "Rails should also include gates, conceptually an opening or door"; "simple openings in the rail system and with a gate, single and double door"; "same Modify enhancement model as adding dormers to a roof".)*

**Workflow.**
1. Select a railing. Modify | Railings offers **Opening** and **Gate** (*Single* or *Double*).
2. Click on the railing where the opening or gate is centred. The width comes from the tool's options: an opening's clear width, or the gate type's leaf widths.
3. A selected opening or gate shows its width, its position along the path, and for a gate the hinge side, swing direction and leaf count. Handles drag it along the railing and change its width.

**Behaviour & rules.**
- **Opening:** a gap in the railing's rails and balusters, of a clear width between the inner faces of two posts. A post is added on each side of the gap (or an existing post is used when one is within the post's reserved space). Balusters inside the gap and the posts' reserved spaces are removed (ARC-RAIL-002). Rails stop at the posts.
- **Gate:** an opening filled by a gate:
  - **single** (one leaf, hinged on one side) or **double** (two leaves meeting in the middle);
  - leaves as frames with the railing's balusters or infill;
  - a **swing direction** (in or out, from a reference side), a hinge side, and an optional latch post.
  - The gate is a hosted insert of the railing, the way a door is hosted by a wall. It moves with the railing's path and keeps its position along it.
- Openings and gates are kept by their **position along the path** (distance from the path's start). When the path changes, they keep that distance, clamped to the path's length.
- Openings cannot overlap each other or a corner. One that would is refused, with the conflicting feature or corner named.
- **Plan symbol:** gates show their swing arc, like doors.

**Data (IFC mapping).**
- **Opening:** the railing's rails are broken at the opening, with no IFC opening element (a railing has no voidable body). The opening is RXD data on the railing.
- **Gate:** an `IfcDoor` (`PredefinedType = GATE`), with an `IfcDoorType` (`OperationType = SINGLE_SWING_LEFT`/`RIGHT`, or `DOUBLE_DOOR_SINGLE_SWING`), contained in the railing's storey. It is related to the railing through `IfcRelConnectsElements` (Name "Gate").

**Commands.**
- `railing.add_opening(railing_id, at? | point?, width?) -> key`
- `railing.add_gate(railing_id, at? | point?, leaves?: 1|2, width?, swing_out?, hinge_end?) -> element_id`
- `railing.move_feature(railing_id, key, at? | point?, width?)`
- `railing.remove_feature(railing_id, key)`
- `railing.features(railing_id) -> [feature]`

**Acceptance criteria.**
- [x] A 900 mm opening in a 6 m deck railing has a post on each side, no balusters or rails across the gap, and stays 900 mm clear.
- [x] A double gate of 1.2 m places two leaves of 600 mm, swinging outward, and exports as `IfcDoor(GATE)` with `DOUBLE_DOOR_SINGLE_SWING`.
- [x] Moving the railing's path keeps the gate the same distance from the path's start.
- [x] `railing.add_gate` from Lua or MCP gives the same model as the window tool.

**Implementation notes (2026-10-13).** As built:
- **Storage:** openings and gates are `features` in the railing's recipe, each with a key, its centre's distance along the path (`at`) and its clear width. Commands take `at` or a plan `point`, which is projected onto the path.
- **Layout:** a post is added each side of the gap. Rails stop at those posts, and balusters are dropped from the gap and the posts' reserved spaces. A feature must lie within one straight run, clear of its corner posts (`railing.feature_off_segment`), and must not overlap another (`railing.feature_overlap`).
- **Path changes:** when the path changes, a feature that no longer fits is left out of the layout (and its gate is not drawn) until the path makes room again.
- **Gate:** an `IfcDoor` (`GATE`) holding the railing's GUID and the feature key. Its leaves are drawn from the railing: stiles, a top and bottom rail, and balusters, with 10 mm clearances. The IFC writer sets `OverallWidth`, `OverallHeight` and `OperationType` (IFC4). Default widths are 1 m for a single gate and 1.2 m for a double.
- **Lifecycle:** deleting the railing deletes its gates. `railing.remove_feature` removes a gate and its element. Deleting a gate on its own leaves its opening.
- **Read-back:** `railing.features` lists a railing's openings and gates.
- **In the window:** Modify | Railings › Opening, Gate and Double Gate. With the railing selected, click it in a plan or in 3D; Esc ends the tool.
- **Deferred:**
  - the gate's swing arc in plan, and the swing and hinge settings in the Properties palette;
  - drag handles;
  - reusing an existing post;
  - the `IfcDoorType` and the `IfcRelConnectsElements` to the railing.

### 3.7.8a Decks
**ID:** ARC-DECK-001 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** Design decks and patios: the outline, the framing beneath, and the boards laid out one by one, so that quantities and a cut list come from the model. *(Owner requirement, 2026-10-13: "a bespoke deck design subsystem for mapping out the deck boards".)*

**Workflow.**
1. Architecture › Deck: the user sketches the deck's outline in a plan (ARC-SKCH-001), with cut-outs for trees or hatches as inner loops. Pick Walls takes a wall's exterior face for a deck *ledgered* to the house.
2. The user sets the deck height (relative to a level, or as a top elevation) and chooses a **deck type** (board and framing settings, below).
3. **Finish** lays out the framing and the boards. The Deck contextual tab offers Edit Outline, Board Layout, Framing, Stairs and Guard Open Edges (ARC-RAIL-001).

**Behaviour & rules.**
- **Boards (deck type, overridable per deck):**
  - board profile: width, thickness, material and finish;
  - **gap** between boards, and end gap;
  - **direction** (an angle, default perpendicular to the ledger or to the longest edge);
  - **stock lengths** available (for example 3.6, 4.2 and 4.8 m);
  - **picture frame**: n border boards around the outline, mitred at corners;
  - **breaker boards** (boards across the field, at set positions or splitting the field into bays);
  - **fascia** boards on the outer faces;
  - **fastening**: face screws per joist crossing, or hidden clips.
- **Board layout algorithm (deterministic):**
  1. Build the field: the outline less the picture frame and breaker bays, less any cut-outs.
  2. Lay courses perpendicular to the direction, at board width plus gap, starting from the *start edge* (the ledger, or the edge the user picks). The first course is full width. The last course is ripped to fit and flagged when narrower than a set minimum (default a third of the board width); *Balance courses* instead centres the field so both edge courses are equal.
  3. Clip each course to the field. A course is broken into boards where it meets cut-outs and breakers.
  4. Split each board run into pieces no longer than the longest stock length. **Every end joint lands on a joist centre** (or a doubled joist / blocking when *Joints on doubled joists* is set).
  5. **Stagger:** joints in adjacent courses are at least the *minimum stagger* apart (default two joist spaces), and no joint repeats on the same joist within *n* courses (default 3). Patterns: *Random* (seeded, so the layout is repeatable), *Stair-step* or *Thirds*.
  6. **Stock optimisation:** each piece is assigned to the shortest stock length it fits, by a first-fit-decreasing cut plan. Offcuts long enough are reused.
  7. Boards are numbered course by course from the start edge.
- **Framing:**
  - a **ledger** on the house wall (or free-standing *beams* at both ends);
  - **joists** at a set spacing (default 400 mm centres) perpendicular to the boards, sized from a span table in the type, with doubled joists under breakers and picture frames, and **blocking** where joints need it;
  - **beams/bearers** across the joists at a set spacing;
  - **posts** on **footings**, at a maximum spacing along the beams.
  - The framing regenerates with the outline, and the board layout follows it.
- **Height:** the boards' top is the deck height. The framing hangs below it, and posts run to the footings (on the toposolid or a level).
- **Quantities and cut list:**
  - `deck.cut_list` gives each board's course, length, stock length and the offcut;
  - it also gives totals per stock length, fastener counts, joist, beam, post and footing counts and lengths, and the board area;
  - a deck schedule (DOC-SCH-001) lists the boards and the framing.
- **Editing:** single boards can be selected (Tab) to override their length or joint, or to be removed. The overrides survive a regeneration where the board still exists.
- **Stairs and railings:** stairs (ARC-STAIR-002) can start from a deck edge. *Guard Open Edges* places railings along the open edges (ARC-RAIL-001).

**Data (IFC mapping).**
- The deck is an **`IfcSlab`** (`PredefinedType = FLOOR`, ObjectType "DECK") whose body is the board field. It aggregates:
  - the boards as **`IfcCovering(FLOORING)`** pieces, or, by option, one covering with the boards as representation items;
  - joists and ledgers as **`IfcMember(JOIST)`**, beams as **`IfcBeam`**, posts as **`IfcColumn`** and footings as **`IfcFooting(PAD_FOOTING)`**.
- The layout settings and per-board overrides are RXD element data (`recipe.deck`); the deck type is RXD type data (`recipe.deck_type`).

**Commands.**
- `deck.create(outline: loops, level_id, height?, type_id?, start_edge?, direction?) -> element_id`
- `deck.set_boards(deck_id, width?, thickness?, gap?, direction?, stock_lengths?, picture_frame?, breakers?, stagger?: {pattern, min, repeat_after}, balance_courses?, seed?)`
- `deck.set_framing(deck_id, joist_spacing?, joist_size?, beam_spacing?, post_spacing?, ledger_wall_id?)`
- `deck.boards(deck_id) -> [board]` (query: course, start, end, length, stock length)
- `deck.cut_list(deck_id) -> cut_list` (query)
- `deck.override_board(deck_id, board_key, length?, joint?, removed?)`

**Acceptance criteria.**
- [ ] A 4.0 × 6.0 m deck with 140 mm boards and a 5 mm gap, boards along the 6 m side, gives 28 courses, the last ripped to fit.
  - With stock lengths of 3.6 and 4.8 m and joists at 400 mm centres, every end joint is on a joist centre.
  - No two adjacent courses have joints within 800 mm.
- [ ] *Balance courses* on the same deck makes the first and last courses equal.
- [ ] A one-board picture frame gives mitred border boards at all four corners, a doubled joist under the frame's inner edge, and a field one board narrower on each side.
- [ ] The cut list's total board length equals the sum of the boards' lengths, and each stock length's count covers the pieces assigned to it.
- [ ] Editing the outline (moving an edge 600 mm) regenerates the courses, the joists and the cut list. A board override on a board that still exists is kept.
- [ ] `deck.create` and `deck.cut_list` from Lua or MCP give the same deck and list as the window.

**Implementation notes (2026-10-13).**
- As built, a deck is a **floor** carrying deck settings (`rxd:deck`, ObjectType "DECK"). Its outline, level and height are the floor's, so Edit Boundary, the footprint grips, `sketch.move_*` and Guard Edges railings all work on it.
- **Commands:**
  - `deck.create(outline, level_id, height?, settings?)`;
  - `deck.set_boards` and `deck.set_framing` (`deck_id`, plus the fields to change);
  - `deck.boards` and `deck.cut_list`.
  - Per-board overrides (`deck.override_board`) and a deck type come later.
- **Layout, as specified:**
  - picture frame rows mitred at the corners;
  - courses from the field's lowest edge across the boards (or balanced), each where its centre line is inside the field;
  - pieces no longer than the longest stock, with every joint on a joist;
  - stagger (*longest*, *stair-step* or seeded *random*) keeping joints in neighbouring courses at least the minimum apart, and not repeating a joist within *n* courses;
  - first-fit-decreasing stock assignment with offcut reuse.
  - The default direction runs along the outline's longest edge.
- **Framing:** joists at the spacing with rims at both ends (doubled inside a picture frame), beams across them, and posts on the beams on pad footings at the level. A deck too low for its framing is flagged.
- **Body:** boards and framing as solids (in IFC, one faceted B-rep each) instead of the floor's slab.
- **Properties:** the board and framing settings, and the cut list (boards, length, stock to buy, area, joists, beams, posts and footings, screws, warnings).
- **Later:** a ledger on the house wall, the span table, breaker boards, blocking under joints, hidden fasteners, board overrides, stairs from deck edges, and aggregated IFC members.


### 3.7.9 Ramps
**ID:** ARC-RAMP-001 · **Priority:** P1 · **Ref:** research/02 §8.6

**Workflow.**
1. The user starts Architecture › Circulation › **Ramp** in a plan or 3D view.
2. In the sketch (Modify | Create Ramp Sketch), the user either uses **Run** (Line or Center-ends Arc), which is the simplest option and adds landings automatically when a run exceeds *Maximum Incline Length*, or draws **Boundary** and **Riser** curves for custom shapes.
3. Tools › **Railing** chooses the railing type.
4. **Finish**.

**Behaviour & rules.**
- **Instance parameters:** Base Level, Base Offset, Top Level, Top Offset, Multistory Top Level, Text (Up/Down), **Width**.
- **Type parameters:** **Thickness** (used when Shape = Thick), **Function**, Text Size and Font, **Ramp Material**, **Maximum Incline Length**, **Ramp Max Slope (1/x)**, **Shape** (Thick or Solid) and identity data.
- **Length.** The required horizontal length is `H · x`, where `H` is the height and `1/x` the maximum slope. When the run tool is used, the tooltip shows the remaining length. A drawn run that is too short raises "Ramp slope exceeds maximum", and the ramp is still created.
- **Automatic landings.** When a straight run's length exceeds *Maximum Incline Length*, Reviera inserts a landing whose depth equals the ramp width. Runs between landings share the height in proportion to their lengths.
- **Geometry.**
  - *Thick*: the top surface is a ruled surface between the boundary curves, rising linearly along the run path. The ramp is a slab of the given thickness under it, built as `BRepOffsetAPI_ThruSections` of cross-sections every 0.5 m along curved paths, or a prism for straight runs.
  - *Solid*: the volume between the top surface and the base level.
- Usability tip carried into the UI: when the default top level would make the ramp unrealistically long, Reviera suggests setting the top level to the base level with a small top offset.
- Sloped or shape-edited floors are an accepted alternative way to model ramps.

**Data (IFC mapping).** **`IfcRamp`** (`STRAIGHT_RUN_RAMP`, `TWO_STRAIGHT_RUN_RAMP`, `QUARTER_TURN_RAMP`, `TWO_QUARTER_TURN_RAMP`, `HALF_TURN_RAMP` or `SPIRAL_RAMP`, classified as for stairs) with an **`IfcRampType`**, aggregating **`IfcRampFlight`** (`STRAIGHT` or `SPIRAL`), **`IfcSlab(LANDING)`** and **`IfcRailing`**. `Pset_RampCommon` holds RequiredSlope and IsExternal, and `Pset_RampFlightCommon` holds Headroom, ClearWidth and Slope. The sketch is stored in RXD element data (`sketch`).

**Commands.** `ramp.create(type_id, base_level, top_level | Unconnected(h), base_offset?, top_offset?, runs?: [curve], boundary?: [[curve],[curve]], risers?: [curve], width?, railing?) -> element_id` and `ramp.edit_sketch(ramp_id, ...)`.

**Acceptance criteria.**
- [ ] A ramp 0.6 m high at 1:12 with Maximum Incline Length 6 m, drawn as one straight run, is 7.2 m long and gets one landing between two 3.6 m runs.

---

## 3.8 Openings

### 3.8.1 Opening by face
**ID:** ARC-OPEN-001 · **Priority:** P0 · **Ref:** research/02 §9

**Workflow.** The user starts Architecture › Opening › **By Face**, picks a face of a floor, roof, ceiling, beam, brace or column, sketches a closed shape on it, and presses Finish.

**Behaviour & rules.** The sketch plane is the picked face's plane. The opening solid is the sketch region extruded **along the face normal**, through the host's full thickness plus 1 mm on each side. It is subtracted from the host (`BRepAlgoAPI_Cut`). On a curved face, the sketch is projected onto the face's tangent plane at the pick point. The opening can be edited with *Edit Sketch*.

**Data (IFC mapping).** **`IfcOpeningElement`** (`OPENING`) related to the host through `IfcRelVoidsElement`, with a body of `IfcExtrudedAreaSolid` along the face normal. The sketch is stored in RXD element data (`sketch`).

**Commands.** `opening.create_by_face(host_ref, loops) -> element_id`.

**Acceptance criteria.**
- [ ] An opening by face on a 30° roof cuts at right angles to the roof surface, so its sides lean 30° from vertical.

### 3.8.2 Vertical opening
**ID:** ARC-OPEN-002 · **Priority:** P0 · **Ref:** research/02 §9

**Purpose.** A plan-shaped hole through one slab-like element, with vertical sides whatever the slope of the element.

**Workflow.**
1. The user starts Architecture › Opening › **Vertical** in a plan, ceiling plan, section, elevation or 3D view and picks a floor (architectural or structural), roof or ceiling.
2. Sketch mode opens on the host's level (in a section or elevation, the user picks a plan view through Go To View). The user sketches one or more closed loops with the draw tools or Pick Lines.
3. **Finish Edit Mode** creates the opening. Selecting it later offers **Edit Sketch**.

**Behaviour & rules.** The user picks a floor, roof or ceiling, sketches loops in plan and presses Finish. The opening is extruded **vertically** through the whole host, whatever the host's slope, and it cuts that host only.
- **Sketch rules.** Loops must be closed and must not intersect each other (ARC-SKCH-002). Several loops make one opening element with several voids. A loop that lies partly outside the host is allowed: the part outside cuts nothing, and the host's edge is notched. A loop entirely outside the host is an error on Finish ("Opening does not intersect host").
- **Extent.** The void runs from below the host's lowest point to above its highest point (host bounding box plus 1 mm each way), so it cuts sloped, shape-edited and warped hosts completely.
- **Following the host.** The opening is hosted: it moves with the host, is copied with it and is deleted with it. If the host's boundary is edited so that the opening no longer intersects it, the opening stays and reports "Opening does not intersect host".
- **Other elements.** Only the picked host is cut. To cut several elements on several levels, use a shaft (ARC-OPEN-004). Rooms are not affected, but an opening in a room-bounding floor or ceiling is subtracted when room volumes are computed (ARC-ROOM-002), because the cut host is used.
- **Instance parameters:** Phase Created and Phase Demolished only; a vertical opening has no height parameters.
- **Hosting.** Elements hosted on the cut face (for example, face-based fixtures) inside the opening lose their host and are reported as orphaned (ARC-HOST-001).

**Data (IFC mapping).** An `IfcOpeningElement` whose body is an `IfcExtrudedAreaSolid` with Z direction, related to the host through `IfcRelVoidsElement`. `PredefinedType = OPENING`. The loops are stored in RXD element data (`sketch`) on the opening.

**Commands.** `opening.create_vertical(host_id, loops) -> element_id`; `sketch.begin(opening_id)` edits the loops afterwards.

**Acceptance criteria.**
- [ ] A vertical opening in a 30° roof has vertical sides.
- [ ] A 1 × 1 m loop that half overlaps the edge of a floor notches the floor by 0.5 m² and nothing else.
- [ ] A vertical opening in the upper of two stacked floors leaves the lower floor intact.
- [ ] Moving the host floor 2 m moves the opening with it.

*Built (2026-10-14):* `opening.create_vertical` and `opening.set_loops` on Reviera floors, ceilings and footprint roofs; loops partly outside notch the edge (polygon booleans by iOverlay); the opening moves and mirrors with its host and is deleted with it; Reviera's display cuts the host while the IFC keeps a slab whole and voids it with the opening (an `IfcExtrudedAreaSolid` of the loops through the host plus 1 mm; a roof's faceted body is written with the hole). Plans draw it crossed with grips. Stair openings (ARC-STAIR-007) make them. *Built (2026-10-15):* Architecture › Opening › **Vertical** sketches the loops on the plan's level; Finish cuts the floor (in a ceiling plan the ceiling) on that level the loops overlap most, else a roof; `opening.create_vertical` also takes sketch `curves`. In a roof an opening must lie within one face. Not yet: picking the host explicitly, Edit Sketch (grips reshape it), openings across roof faces and in arched roofs and domes.

### 3.8.3 Wall opening
**ID:** ARC-OPEN-003 · **Priority:** P0 · **Ref:** research/02 §4.11, §9

**Workflow.** The user starts Opening › **Wall** in an elevation, section or 3D view, picks a straight or arc wall, and drags a rectangle.

**Behaviour & rules.**
- The opening is a rectangle in the wall's developed (u, z) plane. Afterward, drag controls and temporary dimensions edit it, and it can be moved along the wall.
- **Instance parameters:** Base Constraint, Base Offset, Top Constraint, Top Offset and Unconnected Height.
- The cut goes through the wall's full thickness, normal to the location curve. On curved slanted walls the cut is therefore not rectangular in plan.
- The opening produces mid-end faces (ARC-JOIN-001).
- Cased openings use a loadable passage-opening family, which is an insert.

**Data (IFC mapping).** An `IfcOpeningElement` related to the wall through `IfcRelVoidsElement`, with no filling element.

**Commands.** `opening.create_wall(wall_id, u0, u1, base: {level_id, offset}, top: Level{id,offset} | Unconnected(h)) -> element_id`.

**Acceptance criteria.**
- [x] A wall opening that follows its base and top levels grows when the top level is raised.

*Built (2026-10-15):* `opening.create_wall` (`wall_id`, `u0`, `u1`, `base_level` defaulting to the wall's, `base_offset`, `top_constraint` defaulting to 2.1 m, `top_offset`) and `opening.edit_wall`, in straight and curved walls; refused where it leaves the wall's clear span or overlaps a door, window or another opening (and doors and windows are refused over it). It slides along the wall with `element.move`, moves and is copied with its wall and is deleted with it or its levels. Architecture › Opening › **Wall Opening** places it by hovering a wall (width and height in Properties); Properties edits its constraints, width and position. IFC: an `IfcOpeningElement` (box through the wall plus 1 mm, in the wall's frame) voiding the wall. Not yet: drag controls in elevations, mid-end faces for joins and wrapping, cased openings.

### 3.8.4 Shaft opening
**ID:** ARC-OPEN-004 · **Priority:** P0 · **Ref:** research/02 §9

**Workflow.** The user starts Opening › **Shaft** in a plan view, sketches the boundary (lines, or Pick Walls) and optional **Symbolic Lines** (shown at every cut level), and presses Finish.

**Behaviour & rules.**
- **Instance parameters:** **Base Constraint** (defaults to the active plan level), Base Offset, **Top Constraint**, Top Offset and Unconnected Height.
- The shaft solid is the boundary region extruded from `z_base` to `z_top`. It cuts **every** floor, roof, ceiling and toposolid that it intersects. Walls, structural framing and families are not cut.
- When the shaft is moved in any view, it moves everywhere.
- **Implementation.** The shaft queries the R-tree for cuttable elements that overlap its bounding box. It adds a `cut` edge to each one and keeps that list up to date as elements are added, moved or deleted. Each host gets its own derived opening.

**Data (IFC mapping).** The shaft itself is an **`IfcBuildingElementProxy`** with `PredefinedType = PROVISIONFORVOID` and the full prism as its body. For each host it cuts, Reviera writes a separate **`IfcOpeningElement`** (the part of the prism within the host, plus 1 mm), related to the host through `IfcRelVoidsElement`. Each derived opening records the shaft it comes from in RXD element data (`recipe.opening`). The symbolic lines are annotation (DOC chapter).

**Commands.** `opening.create_shaft(loops, base_level, base_offset?, top: Level{id,offset} | Unconnected(h), symbolic_lines?: [curve]) -> element_id`.

**Acceptance criteria.**
- [x] A shaft from Level 1 to Roof cuts every floor between them. A floor added later in that range is cut automatically.
- [x] The IFC export contains one `IfcOpeningElement` per cut host, and each one is voided correctly.

*Built (2026-10-15):* `opening.create_shaft` (loops or sketch curves, base level and offset, top level and offset or a height) and `opening.edit_shaft`; it cuts every Reviera floor, ceiling and footprint roof whose body its extent reaches into (computed on regeneration, so later hosts are cut too) and moves, copies and mirrors with the modify tools. Architecture › Opening › **Shaft** sketches it on the plan's level, up to the next level; plans of every level it passes draw it crossed with grips; Properties edits its extent and lists what it cuts. IFC: the proxy's body is the whole prism; each host gets a derived `IfcOpeningElement` (marked `rxd:shaft <shaft> <host>` in its Description, not loaded as an element) that is updated or removed on save. Not yet: symbolic lines, Pick Walls boundaries, toposolids (none exist yet).

### 3.8.5 Dormer opening
**ID:** ARC-OPEN-005 · **Priority:** P1 · **Ref:** research/02 §9

**Workflow.**
1. The user models the dormer walls and the dormer roof, and joins the dormer roof to the main roof with **Join/Unjoin Roof** (ARC-ROOF-006).
2. The user starts Opening › **Dormer** and picks the main roof.
3. With *Pick Roof/Wall Edges*, the user picks the boundary. Valid references are: the joined roof or its bottom face, the side faces of walls, the bottom faces of floors, the edges of the roof being cut, and model lines on its face.
4. **Finish**.

**Behaviour & rules.**
- The picked references form a closed loop, projected onto the main roof's top surface. The opening solid is that loop extruded vertically down through the main roof, but limited to the region **below** the dormer roof's underside and **inside** the dormer walls' interior faces. As a result, the cut is vertical at the dormer walls and horizontal (following the dormer roof) at the top.
- The loop stays locked to the picked references.

**Data (IFC mapping).** An `IfcOpeningElement` in each affected `IfcSlab` of the main roof.

**Commands.** `opening.create_dormer(roof_id, refs: [ref]) -> element_id`.

**Acceptance criteria.**
- [x] A dormer opening moves with the dormer walls when they are moved 300 mm along the roof.

**Implementation notes (2026-10-13).** As built for automated dormers (ARC-ROOF-007):
- **The opening:** the main roof is cut plumb inside the dormer's front and cheek walls' interior faces, from the front wall back to the cheeks' ends (where the main roof's top meets the dormer eave). It is taken from the walls as they are, so it follows them.
- **The slab:** the face's slab is rebuilt with the hole (plumb sides) and written as triangles. The roof's areas, volume and underside leave the hole out.
- **Later:**
  - an `IfcOpeningElement` (today the hole is in the roof's body);
  - Opening › Dormer by picked references for hand-made dormers;
  - the region under a gable dormer's ridge beyond the cheeks.

---

## 3.9 Rooms, areas and colour schemes

### 3.9.1 Rooms
**ID:** ARC-ROOM-001 · **Priority:** P0 · **Ref:** research/02 §10.1

**Purpose.** Spaces bounded by room-bounding elements, which carry identity, finishes and computed quantities.

**Workflow.**
1. The user starts Architecture › Room & Area › **Room** (`RM`) in a plan view.
2. The user sets the options on the contextual tab:
   - Tag on Placement (on by default);
   - **Upper Limit** (a level) and **Offset** (default 4000 mm, or 10' in imperial templates);
   - tag orientation (Horizontal, Vertical or Model) and Leader;
   - **Room**: *New* or an existing **unplaced** room.
3. A click inside an enclosed region places the room. **Place Rooms Automatically** places a room in every enclosed region larger than 0.25 sq ft (0.023 m²) on the level, numbered in sequence.
4. **Highlight Boundaries** shows all room-bounding elements in a highlight colour and opens a warning list.

**UI.** The Room & Area panel holds: Room, **Room Separator**, Tag Room (`RT`), *Tag All Not Tagged*, Area and Volume Computations, Area, Area Boundary, Tag Area, and Color Schemes.

**Behaviour & rules.**
- **Room-bounding elements by default:** walls (basic, curtain, in-place and face-based), roofs, floors, ceilings (including in-place and face-based ones), architectural columns, structural columns whose material is concrete, curtain systems, and room separation lines. Every one of these has a *Room Bounding* parameter. Linked models bound rooms when their link type has *Room Bounding* on (COL chapter).
- **Narrow regions.** If the region has no point at least 153 mm (half of 306 mm) from its boundary — that is, if the region is too narrow (computed by an inward polygon offset) — placement is rejected with "Room is too narrow".
- **Not enclosed.** Placing a room in an unenclosed region creates the room with *Area = Not Enclosed* and the warning "Room is not in a properly enclosed region".
- **Redundant rooms.** A second room in a region that already has a room is *Redundant* ("Multiple rooms are in the same enclosed region"). Both rooms take the region's area, and both are flagged.
- **Room Separator.** These lines (Line, Arc and Pick Lines) split space. Rooms update automatically. They are visible in plan and 3D, and are controlled by the "Room Separation" line subcategory.
- **Tags** snap to the **room reference point**, which is the intersection of the room's two reference lines. Initially these lines run through the centroid of the boundary's bounding box. *Re-center Reference Lines* (Modify | Rooms) recomputes them, and *Move to Room Reference* (Modify | Room Tags) moves tags back to that point.
- **Unplaced rooms.** Deleting a room in a view *unplaces* it: it stays in the project, schedules list it as Not Placed, and it can be placed again from the Room tool's Room list. Deleting it from a room schedule removes it permanently.
- **Phases.** Rooms belong to one phase. They can be copied between phases, and boundaries are computed with the elements that exist in that phase.
- **Instance parameters:** Level (RO), **Upper Limit**, **Limit Offset**, **Base Offset**, Area, Perimeter, Unbounded Height, Volume (when enabled), Computation Height (RO, from the level), Number, Name, Comments, Occupancy, Department, Base Finish, Ceiling Finish, Wall Finish, Floor Finish, Occupant and Phase.

**Data (IFC mapping).**
- **`IfcSpace`** (`PredefinedType = INTERNAL`; `EXTERNAL` for rooms flagged as exterior) aggregated (`IfcRelAggregates`) under its `IfcBuildingStorey`. `Name` = Number and `LongName` = Name, which is the usual IFC convention.
- **Representations.** `FootPrint` (`IfcGeometricCurveSet` / `IfcArbitraryProfileDefWithVoids` curves) and `Body` (an `IfcExtrudedAreaSolid` from base to upper limit, or the clipped room solid as an `IfcFacetedBrep` when volume computation is on and the room is clipped).
- **Quantities and properties.** `Qto_SpaceBaseQuantities`: NetFloorArea, GrossFloorArea, NetPerimeter, GrossPerimeter, Height, NetVolume and FinishCeilingHeight. `Pset_SpaceCommon`: Reference, IsExternal and the finish properties where `Pset_SpaceCoveringRequirements` applies (FloorCovering, WallCovering, CeilingCovering).
- **Space boundaries.** Each boundary segment is written as an **`IfcRelSpaceBoundary`** (1st level: `PhysicalOrVirtualBoundary` = PHYSICAL for elements, VIRTUAL for separation lines; `InternalOrExternalBoundary` from the wall's function), with `ConnectionGeometry` (an `IfcConnectionCurveGeometry` at the computation height).
- **Room separation lines** are **`IfcVirtualElement`** (4X3: `PredefinedType = BOUNDARY`) with an `Axis` representation.
- **Unplaced rooms** are `IfcSpace` objects without a representation, still aggregated under the storey, and listed in RXD document `model/rooms`.

**Commands.**
- `room.place(level_id, point, upper_limit?: level_id, limit_offset?, base_offset?, name?, number?, phase_id?, room_id?: existing_unplaced, tag?: {type_id, orientation, leader}) -> element_id`
- `room.place_automatically(level_id, phase_id?) -> [element_id]`
- `room.unplace(room_ids)`
- `room.delete(room_ids)`
- `roomseparator.create(level_id, curves: [curve]) -> [element_id]`
- `room.recenter_reference(room_ids)`
- `roomtag.move_to_reference(tag_ids)`
- `room.highlight_boundaries(view_id) -> [element_id]` *(query)*

**Implementation notes (2026-10-13).**
- As built:
  - Rooms are bounded by walls at their finish faces, cut 1 mm above the level.
  - `room.place` takes `level_id`, `point`, `upper_limit?`, `limit_offset?`, `base_offset?`, `name?` and `number?`.
  - `room.set` sets Number, Name, upper limit and offsets.
  - Place Rooms Automatically numbers rooms in reading order (top row first, left to right).
- Until room tag elements exist (DOC-ANN-011), plans draw each room's Name, Number and Area at its tag point. These labels select the room.
- The volume is area × unbounded height; clipped volumes come later.
- Separation lines, unplaced rooms, phases, `IfcRelSpaceBoundary` and `Qto_SpaceBaseQuantities` come later.

**Acceptance criteria.**
- [ ] Place Rooms Automatically on a level with six enclosed regions creates six rooms numbered 1–6, and none for a 0.01 m² gap.
- [ ] Deleting a placed room in plan makes it *Not Placed* in the room schedule. Placing it again keeps its Number and Name.
- [ ] Every room exports as `IfcSpace` with `Name` = Number and `LongName` = Name, and with at least one `IfcRelSpaceBoundary` per bounding wall.

### 3.9.2 Room boundary computation
**ID:** ARC-ROOM-002 · **Priority:** P0 · **Ref:** research/02 §10.1 (Room computation, Area and Volume Computations), §17.10 · web: Room Area and Volume

**Purpose.** A deterministic, robust algorithm that computes room boundaries, areas, perimeters and volumes.

**Behaviour & rules.**

**Settings** (*Area and Volume Computations*, stored in RXD document `model/rooms`):
- **Volume Computations**: *Areas only* (default) or *Areas and Volumes*.
- **Room Area Computation**: *At wall finish* (default), *At wall center*, *At wall core layer* or *At wall core center*.
- **Area Schemes** tab (ARC-AREA-001).

These settings apply to the whole project. Walls that came from a non-parametric import (no RXD recipe) always use their actual faces, which is the finish rule.

**Area and Volume Computations dialog** (Architecture › Room & Area panel drop-down):
- **Computations tab.** *Volume Computations* is a pair of radio buttons (*Areas only*, *Areas and Volumes*) with a note that volume computation adds time to every room regeneration. *Room Area Computation* has four radio buttons for the area rules above and a small plan diagram of a wall showing where the boundary line falls for the selected rule. A read-only line shows the current computation height of the active view's level and a link that selects that level so its *Computation Height* can be edited (a level parameter, default 0; ARC-LVL-001).
- **Area Schemes tab.** The list of area schemes with Name and Description, and New, Rename and Delete (ARC-AREA-001). Deleting a scheme that has area plans lists the plans that will be deleted and asks for confirmation. Names must be unique and non-empty.
- **Validation and effect.** OK applies both tabs as one transaction. Changing the area rule or switching volumes on marks every room dirty, and the rooms are recomputed (in parallel by level) before the dialog closes; a progress bar appears when this takes longer than 0.5 s. Switching volumes off clears the stored volumes and makes *Volume* show "Not Computed". Cancel changes nothing.

**Commands.**
- `room.computation_settings() -> ()` opens the dialog (UI only).
- `room.set_computation_settings(volumes?: AreasOnly|AreasAndVolumes, area_rule?: Finish|Center|Core|CoreCenter) -> ()` applies the settings without the dialog and triggers the recomputation.
- The area-scheme commands are in ARC-AREA-001.

**Algorithm.** Run per room. Rooms that share a level band and phase share the arrangement built in steps 2–3.
1. **Computation plane.** `z_c = elev(Level) + Level.ComputationHeight`. The computation height is a level parameter that defaults to 0. When the room's base offset puts the room's floor above `z_c`, Reviera uses `z_c' = max(z_c, z_base + 1 mm)`.
2. **Candidate collection.** Query the bounding-box R-tree for elements whose 3D bounding box crosses `z_c` and the level's plan extent, then keep those that meet all of these:
   - `RoomBounding = true` (from the host model, or from linked models whose link is room-bounding);
   - they **exist in the room's phase** (created in or before that phase and not demolished in or before it);
   - they are not inside an excluded design option (the room sees the primary option, plus the option it belongs to);
   - they are one of:
     - vertical bounding elements (walls, columns, curtain walls and systems, in-place and face-based elements in bounding categories);
     - room separation lines on the room's level (whatever their height).
3. **Sectioning.** For each candidate, compute its plan trace at `z_c`.
   - **Walls**, according to the area rule:
     - *finish*: section the wall's **uncut body** — the body after joins and attachment, but **before** insert voids, wall openings and reveals are cut — with the plane `z = z_c` (`BRepAlgoAPI_Section` with `gp_Pln`), and take the closed loops (`ShapeAnalysis_FreeBounds::ConnectEdgesToWires`). Using the uncut body stops doors and windows from connecting rooms. Wall sweeps take part only if their own *Room Bounding* is on.
     - *center*: the wall's **centreline** curve (the geometric centre of the full thickness, not the location line). It is added as a zero-width boundary curve, trimmed to the join-resolved length.
     - *core*: section the core layers' union and take its loops.
     - *core center*: the core's centreline curve.
   - **Profile-edited walls and attached walls** are sectioned by their real geometry, so a notch that lies below `z_c` opens the boundary.
   - **Columns, in-place elements and curtain systems**: section loops. Curtain walls use the curve along the panel centre plane.
   - **Room separation lines**: their curves, as zero-width boundary curves.
4. **Planar arrangement.** Insert every loop edge and every zero-width curve into a 2D arrangement (a doubly-connected edge list).
   - Vertices within `TOL_ROOM_SNAP` are merged, and nearly collinear overlapping edges are merged.
   - Intersections are computed with a sweep-line over a uniform grid (exact line/line and line/arc intersections; other curves are flattened to a chord error of 0.1 mm), and edges are split at every intersection.
   - Each face of the arrangement is classified as **solid** when it lies inside a sectioned solid loop (a point-in-loop test on the original loop) or **void** otherwise.
   - Each half-edge records its source element (GlobalId), the sub-reference of the face it came from, and the rule that produced it.
5. **Region.** Locate the placement point `p` (or the room reference point for existing rooms) in the arrangement.
   - If `p` lies in a solid face, report "Room placement point is inside an element" and do not move the room. Placing interactively at such a point is rejected.
   - If `p` lies in the unbounded face, the room is **Not Enclosed**.
   - Otherwise, the **room region** `Ω` is the void face that contains `p`. Its boundary consists of an outer loop and zero or more inner loops (islands such as free-standing columns).
6. **Clean-up.**
   - **Antennas** (dangling edges: degree-1 chains inside `Ω`, such as a separation line that stops in the middle of the room) remain *boundary segments* for reference but contribute **nothing** to area or perimeter.
   - **Free-standing walls** inside `Ω` create inner loops; their footprint is excluded from the area, and their faces count toward the perimeter.
   - **Too narrow:** `Ω` is offset inward by 153 mm (`BRepOffsetAPI_MakeOffset` on the face, or a robust 2D offset). If the result is empty, the room is "Too narrow". For an existing room this keeps the area but flags it.
7. **Area and perimeter.**
   - `Area = |∫ over Ω|`, computed exactly for line and arc edges with the shoelace formula plus circular-segment terms, and to 1e-9 relative accuracy for other curves.
   - `Perimeter` = the sum of the lengths of the outer and inner loops, excluding antennas.
8. **Redundant rooms.** If two placed rooms of the same phase locate to the same `Ω`, both report the area of `Ω`. The one placed later is flagged *Redundant*.
9. **Volume** (when *Areas and Volumes* is on):
   - The band is `z_b = elev(Level) + BaseOffset` to `z_t = elev(UpperLimit) + LimitOffset`. *Unbounded Height* is `z_t − z_b`.
   - **Seed prism** `Q = Prism(Ω ⊕ m, z_b, z_t)`, where `⊕ m` is an outward offset by `m`. The margin `m` is the largest horizontal distance by which any bounding vertical element in the band deviates from its trace at `z_c` (for example, slanted walls). It is computed from the element bounding boxes, and is 0 when all elements are vertical.
   - **Subtract** every room-bounding solid that crosses `Q`: vertical elements (uncut bodies, as in step 3) and horizontal elements (floors, ceilings, roofs and their in-place equivalents). Use `BRepAlgoAPI_Cut` with all tools in one operation (`SetTools`), `SetRunParallel(true)` and the fuzzy value.
   - The room solid is the connected component (`TopExp` over solids) that contains the point `(p, z_c)`. If ceilings or roofs do not close the top, the solid simply reaches `z_t`.
   - `Volume = GProp volume` of that solid.
10. **Outputs.**
    - The boundary segments are an ordered list of `(curve, source element GlobalId or separation line, sub-reference)` for each loop. Wall finish computation, tag placement, "By Room" wall creation (ARC-WALL-014), door From/To (ARC-DOOR-001) and `IfcRelSpaceBoundary` export all use this list.
    - The room's `FootPrint` is `Ω`.
11. **Incremental regeneration.**
    - A room depends (`bound` edges) on every element returned by the candidate query in step 2, and on the level, the upper limit level and the phase.
    - When any of those elements changes geometry, Room Bounding, phase or design option, the room is marked dirty. So is any room whose band contains the changed element's new or old bounding box.
    - Dirty rooms that share a level and phase are recomputed together, with one arrangement per (level, phase, computation height). Different levels run in parallel.

**Acceptance criteria.**
- [ ] A 5 m × 4 m room (finish to finish) bounded by 200 mm walls reports 20.000 m² *At wall finish*, 21.840 m² *At wall center*, and an 18.000 m perimeter at the finish faces.
- [ ] `room.set_computation_settings(area_rule: Center)` updates every room's area in one undo step; undoing it restores the finish-face areas. Switching volumes off makes every room's *Volume* read "Not Computed".
- [ ] A door in a bounding wall does not merge the rooms on its two sides.
- [ ] A 300 × 300 mm column inside the room reduces the area by 0.09 m² and adds 1.2 m to the perimeter.
- [ ] A separation line that stops 1 m short of the opposite wall leaves a single room, and the area does not change.
- [ ] With the computation height set to 1.2 m, one bounding wall slanted 10° outward increases the area by approximately `1.2·tan 10°` × that wall's length (ignoring corner effects), compared with a computation height of 0.
- [ ] A room under a sloped ceiling that bounds it has a volume equal to the analytic prism-minus-wedge value within 0.1%.
- [ ] Moving one wall recomputes only the rooms on that wall's levels. An unrelated level is not recomputed; this is measured by an instrumentation counter.

### 3.9.3 Areas and area plans
**ID:** ARC-AREA-001 · **Priority:** P1 · **Ref:** research/02 §10.1 (Area Schemes), §10.2

**Purpose.** Areas defined per *scheme* (for example gross, rentable or a regulatory scheme), each with its own boundaries and rules.

**Workflow.**
1. Manage the schemes in *Area and Volume Computations › Area Schemes* (create, rename, delete). The defaults are *Gross Building* and *Rentable*.
2. Room & Area › Area › **Area Plan** opens a dialog: Type (scheme), levels (multi-select), *Do not duplicate existing views*, and Scale. Reviera then asks whether to create area boundary lines automatically along the closed loops of exterior walls. (Curtain systems must be sketched by hand.)
3. **Area Boundary** (Room & Area › Area): Draw, or **Pick Lines** with **Apply Area Rules** (the line's position follows the area-type rules; an area must be placed before its type is known) or with an *Offset*. Picked boundaries follow their walls; drawn ones are fixed.
4. **Area** tool: Tag on Placement, orientation, Leader, and *Area*: New or Unplaced. A click inside boundaries places an area. An area can be placed unbounded and bounded later.
5. Areas are made visible through V/G › Area › Interior Fill and Reference (DOC chapter).

**Behaviour & rules.**
- **Area boundary computation.** This is ARC-ROOM-002 steps 4–8, with the candidates replaced by the scheme's **area boundary lines** on the level. Walls and columns are not used directly.
- **Area types and rules.** Each scheme has area types. A **rule table** maps the pair of area types on the two sides of a picked boundary to a boundary position: *exterior face*, *interior face*, *wall centreline*, *face toward side A or B*, or *glass line*. Defaults for the two built-in schemes:
  - **Gross Building:** Gross Building Area ↔ outside → the exterior face. Exterior Area ↔ Gross Building Area → the exterior face. Exterior Area ↔ Exterior Area → the wall centreline.
  - **Rentable:** Office ↔ Office → the centreline. Office ↔ Exterior → the face on the office side. Office ↔ Building Common → the centreline. Store ↔ any → the centreline. Major Vertical Penetration ↔ any → the face on the penetration side. When a window is taller than 50% of the wall height, the boundary moves to the glazing plane of the window (the "glass line").
- The rule table is editable per scheme and stored in RXD document `model/rooms`.
- **Dynamic boundaries.** A picked boundary with *Apply Area Rules* is recomputed whenever its wall moves or the area type on either side changes.
- Areas can be tagged (*Tag Area*), unplaced and deleted like rooms. Area schedules are per scheme.

**Data (IFC mapping).**
- Each area is an **`IfcSpace`**: `PredefinedType = GFA` for the Gross Building scheme and `USERDEFINED` (`ObjectType` = the scheme name) for other schemes. It is aggregated under the storey and marked as an area (not a room) by a property value.
- Each scheme is an **`IfcZone`** (`ObjectType = "AreaScheme"`) whose members (`IfcRelAssignsToGroup`) are that scheme's areas.
- Area boundary lines are `IfcVirtualElement` (4X3: `BOUNDARY`) with the scheme recorded as a property.
- `Qto_SpaceBaseQuantities.GrossFloorArea` holds the area.

**Commands.**
- `areascheme.create(name, description?) -> id`
- `areascheme.set_rules(scheme_id, rules: [{type_a, type_b, position}])`
- `areaplan.create(scheme_id, level_ids, scale, auto_boundaries?: bool) -> [view_id]`
- `areaboundary.create(view_id, curves | picks: [ref], apply_rules?: bool, offset?) -> [element_id]`
- `area.place(view_id, point, area_type?, name?, number?) -> element_id`
- `area.unplace(ids)`

**Acceptance criteria.**
- [ ] A Gross Building area plan created with automatic boundaries has its boundary on the exterior faces of the exterior walls, and the area equals the building footprint.
- [ ] In the Rentable scheme, two Office areas separated by a 200 mm wall share a boundary on the wall centreline. Changing one of them to Exterior Area moves the boundary to the office-side face.

### 3.9.4 Colour schemes
**ID:** ARC-CLR-001 · **Priority:** P1 · **Ref:** research/02 §10.3 · screenshots/architecture/room_color_scheme_by_value.png · screenshots/architecture/room_color_scheme_by_range.png

**Purpose.** Colour rooms, areas, spaces, zones and MEP elements by the value of one of their parameters.

**Workflow.**
1. Room & Area › **Color Schemes** (or selecting a colour fill legend and choosing *Edit Scheme*) opens the **Edit Color Scheme** dialog:
   - **Category**: Rooms, Areas (per scheme), Spaces, HVAC Zones, Ducts or Pipes;
   - **Schemes**: Duplicate, Rename or Delete;
   - **Title**;
   - **Color**: the parameter to colour by;
   - **By value** or **By range**;
   - the rows table: Value, **Visible**, **Color**, **Fill Pattern**, Preview and **In Use**;
   - *Include elements from links*.
2. **By range** rows have *At least* (editable), *Less than* (RO, derived from the next row), Caption and *Edit Format*.
3. **Apply.** The view property *Color Scheme*, with *Color Scheme Location* = Background or Foreground, applies the scheme. Placing a **Color Fill Legend** (Annotate › Color Fill › Legend; DOC chapter) also applies it. Schemes work in plans and sections.

**Behaviour & rules.**
- **By value.** The row list is the set of distinct values of the parameter among elements of the category in the project (plus links, when included). Rows for values that no longer exist stay, marked as not *In Use*. New values get a colour from a pastel palette by default: Reviera's palette, the HSV wheel at saturation 0.35 and value 0.95, assigned by golden-ratio hue steps.
- **By range.** Rows are sorted by *At least*, and an element falls in the row with the largest *At least* ≤ its value. Units follow the parameter.
- An element whose value is not in a visible row is not coloured.
- **Graphics.** The fill draws the element's footprint (rooms and areas: `Ω` from ARC-ROOM-002) with the row's colour and pattern. In sections it draws the cut of the room solid.
- **Legend.** Type properties: swatch size, font, title text, opaque or transparent background, and *Values Displayed* = By View (only the values present in the view) or All.

**Data (IFC mapping).** Scheme definitions are stored in the RXD views documents (DOC chapter). When the IFC export option *Export colour fills as styles* is set, the colour of each room's `FootPrint` is also written as an `IfcStyledItem` in the plan annotation context.

**Commands.**
- `colorscheme.create(name, category, parameter, mode: value|range, ranges?) -> scheme_id`
- `colorscheme.set_rows(scheme_id, rows: [{value | at_least, color, pattern?, visible, caption?}])`
- `colorscheme.duplicate(scheme_id, name) -> id`
- `view.set_color_scheme(view_id, category, scheme_id? (none clears), location?: background|foreground) -> ()`

**Acceptance criteria.**
- [ ] Colouring by Department with five departments produces five rows, each with a distinct colour. Renaming one room's department to a new value adds a sixth row automatically.
- [ ] Colouring by Area with ranges at 0, 10, 20 and 50 m² puts a 20.0 m² room in the 20–50 row.

---

## 3.10 In-place models, model text and model lines

### 3.10.1 Model In-Place
**ID:** ARC-INPL-001 · **Priority:** P1 · **Ref:** research/02 §11.1

**Purpose.** Unique, free-form elements modelled in the context of the project.

**Workflow.**
1. The user starts Architecture › Build › Component › **Model In-Place** (it is also on the Structure and Systems tabs).
2. The user picks a category in the *Family Category and Parameters* dialog. The category decides where the element appears in the Project Browser and how it is scheduled, shown and exported. The user then gives it a **Name**.
3. The in-place family editor opens inside the project. Other elements are shown in half-tone but can still be snapped to. The available tools are:
   - solid forms: **Extrusion**, **Blend**, **Revolve**, **Sweep** and **Swept Blend**;
   - void forms of the same five kinds;
   - Model Line, Component, Model Text, Opening, Reference Line and Reference Plane;
   - Set Work Plane;
   - Family Types (parameters) and Category & Parameters;
   - Cut Geometry and Join Geometry with project elements.
4. **Finish Model** (or **Cancel Model**).

**Behaviour & rules.**
- An in-place family has exactly one type. Copying an in-place instance creates a **new** in-place family, because each in-place instance is unique.
- Room bounding, hosting and attaching all work for in-place elements:
  - elements in the Walls, Floors, Roofs and Ceilings categories can be room-bounding;
  - in-place walls can host doors and windows;
  - walls can attach to in-place roofs and floors.
- **Form geometry in OCCT:**
  - *Extrusion*: `BRepPrimAPI_MakePrism` of the sketch face between the start and end offsets.
  - *Blend*: `BRepOffsetAPI_ThruSections` between the base and top sketches, with vertex mapping set by the user.
  - *Revolve*: `BRepPrimAPI_MakeRevol` about the axis line, between the start and end angles.
  - *Sweep*: `BRepOffsetAPI_MakePipeShell` of the profile along a sketched or picked path.
  - *Swept Blend*: `BRepOffsetAPI_MakePipeShell` with two profiles and a law.
  - **Voids** are subtracted from all solids of the same family, and from project elements given in Cut Geometry pairs.
- *Edit In-Place* re-opens the editor.

**Data (IFC mapping).**
- The occurrence uses the IFC class of its category (for example, Walls → `IfcWall`, Roofs → `IfcRoof`, Casework → `IfcFurniture`, Generic Models → `IfcBuildingElementProxy`). It has a unique `IfcTypeProduct` of the matching type class.
- **Body.** It is written as CSG wherever IFC4 can express it:
  - `IfcExtrudedAreaSolid` (extrusion);
  - `IfcRevolvedAreaSolid` (revolve);
  - `IfcExtrudedAreaSolidTapered` (blends with matching topology);
  - `IfcFixedReferenceSweptAreaSolid` (sweeps);
  - `IfcBooleanResult` DIFFERENCE (voids).

  Any other form is written as an `IfcAdvancedBrep` or `IfcFacetedBrep`.
- The form recipe is stored in RXD document `families/<id>`, flagged as in-place.

**Commands.**
- `inplace.begin(category, name) -> session_id`
- `inplace.add_form(session_id, kind: Extrusion|Blend|Revolve|Sweep|SweptBlend, solid: bool, params) -> key`
- `inplace.finish(session_id) -> element_id`
- `inplace.cancel(session_id)`
- `inplace.edit(element_id) -> session_id`

All family-editor commands from the DM chapter can be used inside the session.

**Acceptance criteria.**
- [ ] An in-place wall made from an extrusion can host a door, and it bounds rooms.
- [ ] Copying an in-place element produces a second family with a new name, and editing one copy does not change the other.

### 3.10.2 Model text
**ID:** ARC-MTXT-001 · **Priority:** P1 · **Ref:** research/02 §11.2

**Workflow.**
1. The user sets the work plane.
2. The user starts Architecture › Model › **Model Text** and enters multi-line text in the *Edit Text* dialog.
3. The preview follows the cursor, and a click places the text.

**Behaviour & rules.**
- **Instance parameters:** Work Plane, Text, **Horizontal Align** (Left, Center or Right), **Material**, **Depth** (0 means flat, which is recommended for performance), Comments, Mark and phasing.
- **Type parameters:** Text Font, Text Size, Bold and Italic.
- Model text is edited with *Edit Text*, moved and rotated like any element, and rehosted with *Edit Work Plane*.
- **Glyph geometry.** The glyph outlines are taken from the font through FreeType (FTL/GPL-2.0+ dual licence; Reviera uses it under the GPL-2.0+ option, which is compatible). The quadratic and cubic contours become B-spline wires. They are built into faces, with holes found by contour orientation and nesting, and extruded by Depth (`BRepPrimAPI_MakePrism`). Text with Depth 0 is a set of planar faces.

**Data (IFC mapping).**
- **Depth > 0:** `IfcBuildingElementProxy` (`ObjectType = "ModelText"`) whose body is an `IfcPolygonalFaceSet` of the tessellated glyphs, or Brep when the export precision is set to high.
- **Depth = 0:** `IfcAnnotation` with an `IfcTextLiteralWithExtent` in a 3D annotation context, plus a tessellated `IfcAnnotationFillArea` copy.
- In both cases the text, font and alignment are stored as property values.

**Commands.** `modeltext.create(text, work_plane, point, type_id, h_align?, material_id?, depth?) -> element_id` and `modeltext.set_text(element_id, text)`.

**Acceptance criteria.**
- [ ] The text "ABC" at depth 50 mm produces a solid with three lumps. The "A" has an enclosed hole.

### 3.10.3 Model lines
**ID:** ARC-MLIN-001 · **Priority:** P0 · **Ref:** research/02 §11.3

**Workflow.**
1. The user starts Architecture › Model › **Model Line** (`LI`) and chooses a **Line Style** on the Line Style panel. Line style is not available for sketch lines.
2. The user draws with any shape from the Draw gallery. The options are **Placement Plane** (a level or plane from the list, or *Pick…*), **Chain**, **Offset** and **Radius**.
3. Listening dimensions work for length, radius, ellipse radii and polygon size.

**Behaviour & rules.**
- Model lines are 3D curves, lying on work planes, and visible in every view. By contrast, detail lines (DOC chapter) belong to a single view.
- *Convert Lines* (Modify | Lines) turns model lines into detail lines and back.
- Model lines can be picked with *Pick Lines* when placing walls, gutters, fascias and slab edges.

**Data (IFC mapping).** `IfcAnnotation` (`ObjectType = "ModelLine"`; in 4X3, `PredefinedType = USERDEFINED`) whose `Annotation` representation is a 3D curve (`IfcPolyline`, `IfcTrimmedCurve` or `IfcBSplineCurveWithKnots`), styled with the line style (`IfcStyledItem` with `IfcCurveStyle`). The work plane is stored in RXD element data (`placement`).

**Commands.** `modelline.create(curves: [curve], work_plane, line_style_id?) -> [element_id]` and `line.convert(element_ids, to: Model|Detail, view_id?) -> [element_id]`.

**Acceptance criteria.**
- [ ] A model line drawn on Level 1 appears in the floor plan, in elevations and in 3D. Converting it to a detail line in the plan removes it from the other views.

---

## 3.11 Massing and site

### 3.11.1 Masses
**ID:** ARC-MASS-001 · **Priority:** P2 · **Ref:** research/02 §12.1 · web: Control Visibility of Mass Instances

**Purpose.** Conceptual volumes for early design and for model-by-face.

**Workflow.**
1. On the Massing & Site tab, the **Show Mass** drop-down sets how masses are displayed (see *Show Mass* below).
2. **In-Place Mass**: the user names the mass, and the **conceptual design environment** opens.
3. The user draws profiles (lines, reference lines, points, arcs and splines, closed or open), selects them and chooses **Create Form** (Solid or Void). Reviera infers the form from the selection:

| Selection | Form | OCCT construction |
|---|---|---|
| One closed loop | Extrusion | `BRepPrimAPI_MakePrism` |
| Profile plus a coplanar axis line | Revolve | `BRepPrimAPI_MakeRevol` |
| Path plus one profile | Sweep | `BRepOffsetAPI_MakePipeShell` |
| Path plus several profiles | Swept blend | `BRepOffsetAPI_MakePipeShell` with sections |
| Stacked closed profiles | Loft | `BRepOffsetAPI_ThruSections(solid=true)` |
| One open profile | Surface | `BRepPrimAPI_MakePrism` of the wire (a face) |

4. **Editing tools:** a 3D gizmo that drags faces, edges and vertices; **X-Ray** (shows the defining profiles and path); **Add Edge**; **Add Profile**; **Dissolve** (back to profiles); and **Rebuild**. *Divide Surface* (UV grids, patterns and adaptive components) is P3 (see Deviations).
5. **Finish Mass**.
6. **Place Mass** inserts loadable mass families.

**Behaviour & rules.**
- **Join masses** (Join Geometry) to remove overlapping volume from the gross figures.
- **Show Mass.** Mass is a model category that is hidden by default in new views. The Show Mass drop-down has four modes:
  - *by View Settings* (default): masses and their subcategories (Form, Mass Floor, pattern lines) display as each view's Visibility/Graphics settings say. This is the only mode that affects printing and export.
  - *Form and Floors*: mass forms and mass floors display in every view, even where the Mass category is off, so that the user can work with them without changing views. Printing, export and view templates still follow the view settings.
  - *Surface Types* and *Zones and Shades*: display mass surfaces, or energy-analysis zones and shading surfaces, so that they can be selected and given energy settings. Both are available only after an energy model has been created (ANA chapter); otherwise they are disabled with a tooltip saying why.
  The mode is session state of the project window, not model data: it is not saved, is not part of undo, and other users and BIMserver clients do not see it. When `view_id` is given, the mode applies to that view only until the view is closed, which lets scripts render a view with masses without changing the user's global mode.
- **Masses in views.** A mass that is hidden by the current mode cannot be selected; model-by-face tools (ARC-MASS-003) switch the mode to *Form and Floors* for their duration and restore it afterwards.
- **Instance parameters:** Gross Floor Area, Gross Surface Area, Gross Volume, Mass Floors (levels), Phase, Design Option and Workset.

**Data (IFC mapping).** `IfcBuildingElementProxy` (`ObjectType = "Mass"`, 4X3 `PredefinedType = USERDEFINED`) with an `IfcAdvancedBrep` body. The form recipe is stored in RXD document `families/<id>`. Masses are excluded from the default IFC export view (*Export masses* option, off by default).

**Commands.**
- `mass.begin_inplace(name) -> session_id`
- `mass.create_form(session_id, curve_refs: [ref], solid: bool) -> key`
- `mass.edit_form(session_id, key, op: MoveSub{ref,vector}|AddEdge|AddProfile|Dissolve|Rebuild)`
- `mass.finish(session_id) -> element_id`
- `mass.place(type_id, point, rotation?, level_id?) -> element_id`
- `mass.show(view_id?, mode: by_view_settings|form_and_floors|surface_types|zones_and_shades) -> ()` sets the Show Mass mode (display setting, not undoable)

**Acceptance criteria.**
- [ ] Selecting three stacked closed profiles and choosing Create Form produces a lofted solid, and its gross volume matches an independent OCCT calculation.
- [ ] In a plan view whose Mass category is off, *Form and Floors* shows the mass and lets it be selected; printing that view to PDF does not include the mass; switching back to *by View Settings* hides it again and leaves the undo stack unchanged.

### 3.11.2 Mass floors
**ID:** ARC-MASS-002 · **Priority:** P2 · **Ref:** research/02 §12.1 · web: Create Mass Floors; About Mass Floors at the Top of a Mass; About Mass Floors at the Bottom of a Mass

**Purpose.** Slice a mass at project levels to get floor areas, perimeters, façade areas and volumes per storey for early design analysis, and to drive Floor by Face.

**Workflow.**
1. The user selects one or more masses in any view and chooses Modify | Mass › Model › **Mass Floors**.
2. The **Mass Floors** dialog lists every story and reference level with a checkbox, sorted by elevation, plus *Select All* and *Select None*. With several masses selected, a box that differs between them shows as indeterminate; leaving it so keeps each mass's own choice.
3. OK creates, keeps or deletes mass floors to match the ticks, in one transaction.

**Behaviour & rules.**
- The user selects a mass and chooses **Mass Floors**, then ticks the levels.
- For each ticked level `L`, Reviera computes the planar section of the mass at `elev(L)` (`BRepAlgoAPI_Section`), builds faces from it, and creates a mass floor.
- **Ticked levels that miss the mass** are remembered but produce no floor. If the mass is later changed so that it crosses such a level, the floor appears automatically; if it stops crossing, the floor disappears (and comes back when it crosses again). Mass floors, unlike elements by face, always follow their mass.
- Levels added later that cross the mass get mass floors automatically when the mass has *all levels* selected.
- **Top and bottom.** No mass floor is created at a level that coincides (within `TOL_POINT`) with a top face of the mass, because nothing lies above it. A floor *is* created at a level that coincides with the bottom face. Any part of the mass below the lowest mass floor is not counted in any mass floor's figures; the mass's own *Gross Volume* and *Gross Surface Area* still include it.
- **Properties:** Floor Perimeter, Floor Area, Exterior Surface Area (the mass's side surface between this level and the next), Floor Volume (the mass volume between this level and the next), Level, **Usage** and Mark.
- **Band definition.** "The next" level is the next ticked level above that crosses the mass, or the top of the mass. The topmost floor's Exterior Surface Area therefore also includes the roof faces of the mass above it, and can be noticeably larger than the floors below. The bottom face of the mass is never part of any floor's exterior surface area.
- **Several section loops.** A level that cuts the mass into separate islands gives one mass floor whose area is the sum of the islands; holes are subtracted.
- **Joined masses** (Join Geometry) are sliced as their union, so overlapping volume is counted once.
- **Usage** is a free-text instance parameter used for grouping in schedules. Mark is user-set.
- Mass floors can be scheduled and tagged.
- **Deleting** a level deletes its mass floors; unticking a level in the dialog deletes the mass floor and any tag on it after a warning. Floors made from a mass floor (Floor by Face, ARC-MASS-003) are not deleted; they lose their link.

**Data (IFC mapping).** `IfcSpace` (`PredefinedType = GFA`, `ObjectType = "MassFloor"`) aggregated under the storey, with `Qto_SpaceBaseQuantities`. (GrossFloorArea, GrossPerimeter, GrossVolume; the exterior surface area is written as a property value.) The ticked levels (including those that currently miss the mass) are stored in the mass instance's RXD element data (`elements`). Mass floors are exported only with the *Export masses* option (ARC-MASS-001).

**Commands.** `mass.set_floors(mass_id, level_ids | All)`.

**Acceptance criteria.**
- [ ] A 20 × 30 × 12 m box mass with levels every 3 m creates four mass floors of 600 m² each, and each has a floor volume of 1800 m³.
- [ ] With a level at the top face of that mass also ticked, no fifth floor is created, and the fourth floor's exterior surface area is 300 m² (sides) + 600 m² (roof) = 900 m².
- [ ] Ticking a level 3 m above the mass creates nothing; stretching the mass to 15 m creates the floor at that level without reopening the dialog.

### 3.11.3 Model by face and Update to Face
**ID:** ARC-MASS-003 · **Priority:** P2 · **Ref:** research/02 §4.1, §12.1

**Behaviour & rules.**
- **Wall by Face** (non-horizontal faces; options Level, Height and Location Line), **Floor by Face** (picks **mass floors**), **Roof by Face** (ARC-ROOF-003) and **Curtain System by Face** (ARC-CW-007) all work the same way: with *Select Multiple* (on by default), the user clicks faces to add or remove them, can use *Clear Selection*, and then chooses **Create**.
- The new elements record *Related to Mass* and **do not update automatically**. Selecting one and choosing **Update to Face** regenerates it from the current face. Update to Face has no effect on walls with explicit top constraints, and it discards sketch edits.
- *Select elements based on a mass* selects the dependent elements again.
- **Implementation.** Each element stores `(mass GlobalId, face sub-reference)`. Face sub-references are named from the form recipe (for example `#face:extrusion1.side3`) and stay stable while the recipe's topology is unchanged. When a face cannot be resolved, Update to Face reports "Face no longer exists".

**Commands.**
- `wall.create_by_face(type_id, face_refs, level_id, location_line, height?) -> [element_id]`
- `floor.create_by_mass_floor(type_id, mass_floor_ids) -> [element_id]`
- `element.update_to_face(element_ids)`
- `mass.select_dependents(mass_id) -> [element_id]` *(query)*

**Acceptance criteria.**
- [ ] After the mass is stretched, Update to Face makes a wall by face match the new face. Before Update to Face, the wall stays unchanged.

### 3.11.4 Toposolid
**ID:** ARC-TOPO-001 · **Priority:** P1 · **Ref:** research/02 §1 (2024, 2026), §12.2 · screenshots/architecture/toposolid_site_plan_and_3d.png · web: Toposolid Smooth Shading; Toposolid Enhancements (2025)

**Purpose.** A solid, layered terrain element that can be shape-edited, sub-divided, excavated, split and graded, and that can host site elements.

**Workflow.**
- **Create from Sketch** (Massing & Site › Model Site › Toposolid):
  1. Sketch a closed boundary and Finish.
  2. Use Shape Editing › **Add Point** (*Along Surface* or *Absolute*; *Elevation Base* = Current Level, Project Base Point, Survey Point or Internal Origin; *Elevation*).
  3. Use **Modify Sub Elements** to drag points (blue arrow for vertical, red square for horizontal) or type elevations.
- **Create from Import:** from 3D points or contour polylines in an imported or linked DWG, or from a points file (CSV or TXT: x, y, z with a header-row option). The number of points is capped by the setting `toposolid.max_points` (default 20 000, allowed range 10 000–50 000). For linked civil topography, `toposolid.link_max_points` applies. Above the cap, Reviera simplifies the points (ARC-TOPO-005) before triangulating and reports what it did.
- **Link Topography:** linked civil surfaces (LandXML and IFC4X3 `IfcGeographicElement`/terrain). Linking is covered in the COL chapter.

**Behaviour & rules.**
- **Geometry.** The top surface is a TIN: a CDT of the boundary vertices, points and split lines (ARC-SHAPE-001), with heights taken from the points.
  - Boundary vertices that have no point take their height from the nearest triangle of the surrounding points, extrapolated linearly if needed. If there are no points at all, they take `level + offset`.
  - The solid is built from: the top TIN; the vertical side faces along the boundary; and a flat bottom at `z_bot = min(top) − Σ layer thicknesses`, or at the type's *Default Thickness* below the level if that is lower.
  - Layers: the topmost layers follow the top surface (offset straight down). The type's **variable** layer, which is the core by default, absorbs the varying depth. The layers below the variable layer are flat, parallel to the bottom.
  - OCCT builds the top as a shell of triangular faces sewn with the side and bottom faces (`BRepBuilderAPI_Sewing`, then `BRepBuilderAPI_MakeSolid`).
- **Hosting.** Site components, parking components, railings, floor-based families and slab edges (curbs) can be hosted on the surface.
- **Surface pattern** comes from the top layer's material.
- **Instance parameters:** Level, Height Offset from Level, Room Bounding, Curved Edge Condition, Perimeter, Area, Volume (after cuts and excavations), Elevation at Top, Elevation at Bottom and phasing.
- **Type parameters:** Structure (layers), Default Thickness, coarse fill, **Contour Display** (ARC-TOPO-004), Structural Material and thermal properties.
- **Display.** Folding lines (the TIN edges) are hidden by default (V/G › Toposolid › Folding Lines) and are always suppressed while the toposolid is being shape-edited, so the contours stay readable; contours remain visible during editing when the view and the type show them.
- **Smooth shading** (Massing & Site › Model Site › **Toposolid Smooth Shading**, a toggle):
  - When on, every toposolid in the project is drawn with per-vertex normals averaged over the adjacent top triangles (weighted by triangle angle), so the faceted TIN reads as a continuous surface. Side and bottom faces and sharp creases (dihedral angle above 30°, split lines and sub-division boundaries) keep hard normals.
  - It applies in the shaded visual styles (Shaded, Consistent Colors, Textures, Realistic). Hidden Line and Wireframe are unchanged, and contours, cut patterns and dimensions are unaffected because the geometry is unchanged.
  - Limitations while it is on: surface (model and drafting) patterns are not drawn on toposolid tops, decals cannot be placed on toposolids, and paint and graphic overrides of toposolid materials are not shown. The toggle's tooltip lists these.
  - It is a project-wide display setting stored in RXD document `project/settings` (`site.smooth_shading`, default off). It is not part of undo. Image, glTF and OBJ exports write the smoothed normals when it is on; IFC geometry is never smoothed.

**Regeneration.** Edges out of the toposolid: `host` to the elements on it; `cut` from cutting and excavating elements into it (ARC-TOPO-002). Sub-divisions read the parent's surface.

**Data (IFC mapping).**
- **`IfcGeographicElement`** (`PredefinedType = TERRAIN`, an IFC4 enumeration value), contained in **`IfcSite`**.
- **Body:** an `IfcPolygonalFaceSet` (closed), or an `IfcFacetedBrep` for IFC4 consumers that do not support face sets.
- `IfcMaterialLayerSetUsage` (AXIS3, NEGATIVE).
- When the export option *Site geometry on IfcSite* is set, the top TIN is also written as the `IfcSite`'s `Body` (an open `IfcTriangulatedFaceSet`), because many viewers expect terrain there. The option is on by default when there is exactly one toposolid.
- The points, split lines and settings are stored in RXD element data (`recipe.toposolid`).

**Commands.**
- `toposolid.create(type_id, boundary: loop, level_id, offset?, points?: [xyz], split_lines?: [[xyz]]) -> element_id`
- `toposolid.create_from_import(type_id, source: ImportRef(element_id) | File(path), level_id, max_points?) -> element_id`
- `toposolid.toggle_smooth(on? (default: toggle)) -> ()` switches smooth shading for the project (display setting, not undoable)
- the `shape.*` commands (ARC-SHAPE-001)

**Acceptance criteria.**
- [ ] A toposolid created from a CSV of 5 000 points produces a TIN with every point as a vertex. Its volume matches an independent prism-sum calculation within 0.1%.
- [ ] Importing 60 000 points with the default cap produces at most 20 000 vertices, and the user is told about the simplification.
- [ ] The IFC export opens in an independent viewer with the terrain visible, both under the `IfcSite` and as an `IfcGeographicElement`.
- [ ] Turning smooth shading on changes the shaded display of every toposolid without changing its volume, contours or IFC body, and does not add an undo step; with it on, a toposolid's surface pattern is not drawn.

### 3.11.5 Sub-divisions, split and excavation
**ID:** ARC-TOPO-002 · **Priority:** P1 · **Ref:** research/02 §1 (2026), §12.2, §12.3 · screenshots/architecture/toposolid_subdivision_road.png · web: Merge Toposurfaces (2016); What's New in Revit Site Planning and Design 2024

**Behaviour & rules.**
- **Sub-Divide** (Modify | Toposolid › Toposolid Shaping):
  1. The user sketches a region and chooses a sub-division **toposolid type** and a **Sub-divide Offset**: positive raises it, negative excavates the host, 0 is flush.
  2. **Geometry.** The parent's TIN is clipped to the region. The region boundary is inserted as constraints, with heights sampled from the parent's surface, and the result is lifted by the offset to form the sub-division's top. Its layers come from its own type.
     - For offset > 0, the sub-division sits on the parent: its bottom is the parent's surface.
     - For offset < 0, the parent loses the volume between its surface and the sub-division's top within the region (`BRepAlgoAPI_Cut` of the parent with a prism from the lowered top up to the parent's top). The **Excavation Volume** is that volume.
  3. The sub-division follows the parent's later edits. *Edit Sketch* changes the region.
- **Split Element** on a toposolid: the user sketches a closed loop inside it, or an open chain whose two ends touch the boundary. The result is two independent toposolids, each keeping its own points. Sub-divisions that cross the split line stay with the part that holds the larger share of their area. Sub-divisions that lie wholly within a removed region are deleted, and the user is notified.
- **Excavate / Remove Excavation:**
  - A floor, roof, building pad or another toposolid that intersects the host excavates it.
  - The cut tool is the cutter's solid **plus** a vertical prism of its plan footprint from the cutter's bottom up past the host's top. This is what lets a basement slab remove the soil above it.
  - The host's volume updates. Excavation is a `cut` edge.
  - **Robustness.** Reviera does not jitter void positions. It relies on OCCT's fuzzy Booleans (`OCCT_FUZZY`) and retries once with a fuzzy value 10× larger before reporting "Excavation failed". See Deviations.
- **Cut Geometry / Join Geometry** with floors, walls, foundations and similar elements uses the general rules (ARC-MOD-016, ARC-MOD-017).
- **Merge Toposolids** (Modify | Toposolid › Toposolid Shaping › **Merge**; see D16). It reverses a split and joins terrain pieces into one element:
  1. The user selects two or more toposolids (or starts the tool and picks them one after another, then Finish). An option *Remove points on common edges* (on by default) is on the contextual tab.
  2. **Conditions.** The toposolids must be on the same level, in the same phase and design option, and of the same type; in plan, their boundaries must share at least one edge segment longer than `TOL_SHORT` or overlap. Toposolids that only touch at a point, or are apart, are refused ("Toposolids must overlap or share an edge"). A different type is refused with the two type names; the user changes one first.
  3. **Result.** The first picked toposolid survives and keeps its GlobalId, type, parameters and phase; the others are deleted. Its boundary becomes the union of the plan outlines (holes where the outlines enclose a gap). Its points are the union of all points; where two inputs overlap, the points of the first picked one win and the others' points inside the overlap are dropped. With *Remove points on common edges* on, points that lie on the former shared boundary (within `TOL_POINT`) and are not needed to keep the surface's height there within 1 mm are removed; this cleans up the extra points a split inserts.
  4. **Dependants.** Sub-divisions, excavations, cuts, joins, graded-region links and hosted elements (site components, parking, railings, slab edges) of the deleted toposolids are moved to the survivor. Dimensions and tags that referenced deleted toposolids are removed and listed.
  5. The merge is one undo step. In IFC the survivor keeps its `IfcGeographicElement`; the deleted ones disappear.

**Data (IFC mapping).**
- A sub-division is an `IfcGeographicElement(TERRAIN)` aggregated under its parent (`IfcRelAggregates`).
- An excavation is written as an `IfcOpeningElement` in the host (`IfcRelVoidsElement`). In IFC4X3 the excavated soil is also written as an **`IfcEarthworksCut`**, when the IFC4X3 schema is selected.
- The excavation volume is stored as a property value.

**Commands.**
- `toposolid.subdivide(toposolid_id, loops, type_id, offset) -> element_id`
- `toposolid.split(toposolid_id, curves: [curve]) -> [element_id]`
- `toposolid.excavate(toposolid_id, cutter_ids)`
- `toposolid.remove_excavation(toposolid_id, cutter_ids?)`
- `toposolid.merge(toposolid_ids, remove_common_points?: bool = true) -> element_id` returns the surviving toposolid (the first in the list)

**Acceptance criteria.**
- [ ] A road sub-division with offset −0.3 m over a 100 m² flat region reports an excavation volume of 30 m³.
- [ ] A 10 × 10 m basement floor with its top 3 m below a flat terrain removes 300 m³ plus the floor's own volume from the toposolid.
- [ ] Splitting a toposolid with an open chain and merging the two parts again restores one element with the original GlobalId, the original volume within 0.1%, and no extra points on the former split line.
- [ ] Merging two toposolids that touch only at a corner, or that use different types, is refused and changes nothing.

### 3.11.6 Graded region
**ID:** ARC-TOPO-003 · **Priority:** P2 · **Ref:** research/02 §12.2

**Workflow.**
1. The user starts Massing & Site › Modify Site › **Graded Region** and chooses either *Create a new toposolid exactly like the existing one* or *based on perimeter points only*.
2. The user picks a toposolid from an existing phase.
3. Reviera **demolishes** the original in the current phase (Phase Demolished = current phase) and creates a copy (Phase Created = current phase). The copy has either all the original points or only the perimeter points.
4. The user shape-edits the copy.

**Behaviour & rules.**
- **Cut and fill.** Selecting the graded toposolid shows **Cut**, **Fill** and **Net cut/fill**. Reviera computes them against the demolished original:
  1. Overlay the two TINs in plan: take the union of their vertices and edges and build a CDT.
  2. In each resulting triangle `τ`, `Δz = z_new − z_old` is linear. If `Δz` changes sign inside `τ`, split `τ` along the zero line.
  3. `Fill = Σ ∫_τ max(Δz, 0)` and `Cut = Σ ∫_τ max(−Δz, 0)`. Both are exact for piecewise-linear surfaces (triangle area × mean Δz on each part). `Net = Fill − Cut`.
- The link between the original and the graded copy is stored in RXD element data (`recipe.toposolid`).

**Commands.** `toposolid.create_graded_region(toposolid_id, mode: Copy|PerimeterOnly) -> element_id` and `toposolid.cut_fill(toposolid_id) -> {cut, fill, net}` *(query)*.

**Acceptance criteria.**
- [ ] Lowering one interior point by 1 m on a graded copy of a flat 20 × 20 m toposolid, triangulated so that the point's star covers 100 m², reports Cut = 33.33 m³ and Fill = 0.

### 3.11.7 Contours, labels and site settings
**ID:** ARC-TOPO-004 · **Priority:** P1 · **Ref:** research/02 §1 (2025), §12.2, §12.3 · web: Control Contour Display on Toposolids; Video: Configure Site Settings; Toposurface Settings; Property Line Data Settings

**Purpose.** Contour lines and labels on toposolids, and the project-wide site settings that give contour defaults, section graphics for terrain and the angle format of property lines.

**Workflow.**
1. **Per type:** select a toposolid › Edit Type › *Contour Display* › **Edit…** opens the Contour Display dialog for that type.
2. **Project-wide:** Massing & Site › Model Site panel › dialog launcher (**Site Settings**) opens the Site Settings dialog.
3. **Labels:** Massing & Site › Modify Site › **Label Contours**, then draw a line across the contours in a plan view.

**UI.**
- **Contour Display dialog** (type). A checkbox *Use site settings for primary contours* (on for new types) and a table of additional rows. Each row has: visible (checkbox), Start, Stop, Interval, Range Type (Single Value or Multiple Values), Subcategory (a Topography line subcategory, for example Primary or Secondary Contours). Buttons Insert, Delete, Up and Down. With Range Type = Single Value only Start is used. Validation: Interval > 0 for Multiple Values, Stop ≥ Start, and at most 10 000 contour heights per row ("Too many contours; increase the interval"); errors show inline and block OK.
- **Site Settings dialog** (project), three groups:
  - *Contour Line Display*: **At intervals of** (checkbox and length, default 1 m or 5' in imperial templates), **Passing through elevation** (default 0, measured from the project's elevation base), and an *Additional Contours* table with the same columns as above. These are the primary contours of every type that uses site settings.
  - *Section Graphics*: **Section cut material** (default: the template's earth material) and **Elevation of poché base** (default −10 m). The material is the default for the core layer of new toposolid types and of types that Generate Toposolid creates (ARC-TOPO-006); surface-only imported terrain that has not been converted is drawn in sections as a fill of this material from the surface down to the poché base.
  - *Property Data*: **Angle display**: *Degrees from N/S* (bearings such as N 45°00'00" E, the default) or *Degrees* (a 360° azimuth clockwise from north); **Units**: *Degrees-minutes-seconds* (default) or *Decimal degrees*. They apply to the property-line table (ARC-SITE-001) and to property-line tags.
  - Validation: interval > 0, poché base below the lowest toposolid bottom (a warning, not an error, if it is not). OK applies everything as one undoable transaction; Cancel discards.

**Behaviour & rules.**
- **Contour Display** (a type property; dialog). Rows have Start, Stop, Interval, Range Type (Single Value or Multiple Values), Subcategory and visibility. There is one row set for primary contours and one for secondary contours.
- **Contour heights.** With site settings in use, the primary heights are `z = e0 + k·i` for every integer `k`, where `i` is the interval and `e0` the passing-through elevation (for `i` = 10 and `e0` = 5: …, −15, −5, 5, 15, …). The type's rows add more heights. A height produced by several rows is drawn once, with the subcategory of the last row in the list.
- **Contour generation.** Each triangle of the top TIN is sliced at every contour height `z_k` with a linear interpolation along its edges. The segments are chained into polylines, and closed contours are detected. Contours are recomputed lazily, per view, when they are displayed.
- Contours are drawn on the top surface of toposolids and sub-divisions whose type shows them; a sub-division's type decides its own contours. They are not drawn on excavated or cut faces.
- **Label Contours** (Modify Site): the user draws a label line, and labels are placed where that line crosses each contour. The labels are view annotation (DOC chapter).
- V/G controls contours per view.
- Changing site settings or a type's contour rows regenerates the contours of all affected toposolids in open views at once.

**Data (IFC mapping).** Contours are not written to IFC (they are derived). The type's rows are stored in RXD type data of the toposolid type (`params`); the site settings are stored in RXD document `project/settings` (section `site`). Contour labels are view annotation (DOC chapter).

**Commands.**
- `toposolidtype.set_contours(type_id, rows)` and `contour.label(view_id, line: Line, label_type_id?) -> element_id`.
- `site.settings() -> ()` opens the Site Settings dialog (UI only).
- `site.set_settings(contour_interval?: f64 | None, passing_elevation?, additional_contours?: rows, section_material_id?, poche_base_elevation?, angle_display?: Bearing|Azimuth, angle_units?: DMS|Decimal) -> ()` applies the same values without the dialog.

**Acceptance criteria.**
- [ ] A plane tilted 10% over 50 m with a 1 m contour interval shows five contours, evenly spaced 10 m apart.
- [ ] With an interval of 10 m and a passing-through elevation of 5 m, a terrain from −20 m to +20 m shows contours at −15, −5, 5 and 15 m only.
- [ ] Switching *Angle display* to *Degrees* shows a property-line segment previously listed as N 90°00'00" E as 90°, in the table and in its tag, without changing the geometry.
- [ ] A Contour Display row with Interval 0 and Range Type Multiple Values blocks OK with an inline error.

### 3.11.8 Simplify surface
**ID:** ARC-TOPO-005 · **Priority:** P1 · **Ref:** research/02 §1 (2024), §12.2 · web: Simplify Topography

**Purpose.** Reduce the number of points of a dense terrain (typically an import) so that editing, regeneration and navigation stay fast, while keeping the surface within a stated vertical error.

**Workflow.**
1. The user selects one toposolid and chooses Modify | Toposolid › Shape Editing › **Simplify**.
2. The **Simplify Toposolid** dialog shows the current point count and offers either *Target number of points* (default: half the current count) or *Maximum vertical error* (default 50 mm). A **Preview** button computes the result and shows the resulting point count and the actual maximum vertical deviation; the view shows the simplified surface in a temporary overlay.
3. OK applies the result; Cancel discards the preview.

**Behaviour & rules.**
- **Simplify Topography** reduces the number of points to a target count, or to a vertical tolerance.
- **Algorithm.** Greedy decimation: repeatedly remove the interior vertex whose removal changes the surface least (the vertical error of re-triangulating its star), using a priority queue, until the target is met. Boundary and split-line vertices are never removed. Points carrying a "locked" flag (for example, survey benchmarks) are never removed either.
- **Error measure.** The vertical error of a removal is the largest |Δz| between the old and new surfaces at the removed vertex and at the vertices removed earlier inside the affected star, so the error is tracked against the *original* points, not only against the previous step. The reported maximum deviation is that tracked value.
- **Limits.** The target cannot go below the number of protected vertices (boundary, split-line and locked) plus one; a lower request stops at that number and says so. A toposolid with fewer than 50 interior points cannot be simplified ("Nothing to simplify").
- **Dependants.** Sub-divisions, graded-region links, excavations and hosted elements regenerate on the new surface; hosted elements keep their (x, y) and take the new surface height. Cut and fill of a graded region (ARC-TOPO-003) changes accordingly and is recomputed.
- The simplification is one undoable step.
- **Imports.** Create from Import uses the same algorithm when an import exceeds the point cap (ARC-TOPO-001).

**Data (IFC mapping).** The remaining points replace the old ones in RXD element data (`recipe.toposolid`); the body is regenerated. The tolerance used and the reported deviation are recorded in the element's RXD data for traceability.

**Commands.** `toposolid.simplify(toposolid_id, target_points? , max_vertical_error?)`; exactly one of the two must be given ("Specify a target or a tolerance, not both"). The command returns nothing; the resulting count and deviation are reported in the result message.

**Acceptance criteria.**
- [ ] Simplifying a 20 000-point terrain to 5 000 points keeps the maximum vertical deviation at or below the reported value, and the boundary is unchanged.
- [ ] Simplifying with a maximum vertical error of 0 removes only points that lie exactly on the plane of their neighbours, so a perfectly planar terrain is reduced to its boundary vertices plus one point.
- [ ] Locked survey points are still present after simplifying to the minimum.

### 3.11.9 Legacy toposurface and building pad
**ID:** ARC-TOPO-006 · **Priority:** P3 · **Ref:** research/02 §1 (2024), §12.3 · web: Generate a Toposolid from a Toposurface

**Purpose.** Bring surface-only terrain from other tools into the toposolid model without supporting the legacy element kinds themselves.

**Workflow.**
1. On IFC import (or link-and-bind), Reviera detects surface-only terrain and lists it in the import report with a **Generate Toposolid** action. The user can also select such an element later (it is imported as a non-parametric element) and choose Modify › **Generate Toposolid**.
2. A dialog asks for the **Base level** (default: the level nearest below the surface's lowest point) and the **Toposolid type** (default: the project's default toposolid type).
3. OK creates the toposolid. The source surface stays in the model, unchanged, until the user deletes it.

**Behaviour & rules.**
- Reviera **does not create** surface-only terrains or building pads. Open alternative: a toposolid, plus a floor that excavates it.
- **On IFC import**, a terrain that is a surface without thickness (an open TIN on `IfcSite` or an `IfcGeographicElement`) is offered for conversion with **Generate Toposolid**: a base level and type dialog, non-destructive, with the original kept as a reference until it is deleted.
  - Sub-regions (separate surface patches with other materials) become sub-divisions.
  - Site components are rehosted onto the new toposolid if they can be. Otherwise they are listed.
  - Railings are rehosted automatically.
- **Detection.** A surface-only terrain is an `IfcSite` `Body` or an `IfcGeographicElement(TERRAIN)` whose representation is an open shell (`IfcTriangulatedFaceSet` with `Closed = FALSE`, an open `IfcShellBasedSurfaceModel`, or an `IfcTriangulatedIrregularNetwork` in IFC4X3).
- **Conversion.** The surface's vertices become toposolid points, and its outer boundary (the free edges, chained) becomes the boundary loop. Inner free-edge loops become holes. Triangles are kept by inserting their edges as split lines only where the CDT of the points would differ by more than `TOL_POINT`, so the top matches the source.
- **Sub-regions.** Each differently-styled patch becomes a sub-division with offset 0 and a new toposolid type named after the patch's material, with the material on its top layer and the default thickness. A patch with more than 100 boundary loops, or whose loops cannot be closed, is skipped and listed (for example a striped crossing modelled as one patch).
- **Pads.** An imported `IfcSlab(BASESLAB)` that is flagged as a pad (its `ObjectType` or a pad property says so) becomes an ordinary floor that excavates the generated toposolid (ARC-TOPO-002).
- **Point cap.** If the surface has more vertices than `toposolid.max_points`, it is simplified first (ARC-TOPO-005) and the user is told.

**Data (IFC mapping).** The new toposolid is an ordinary `IfcGeographicElement(TERRAIN)` (ARC-TOPO-001). The link to the source surface is stored in its RXD element data (`recipe.toposolid`) until the source is deleted.

**Commands.** `toposolid.generate_from_surface(surface_element_id, type_id, level_id) -> element_id`.

**Acceptance criteria.**
- [ ] An IFC file with a TIN on `IfcSite` converts to a toposolid whose top matches the TIN within `TOL_POINT`.
- [ ] A terrain with two differently styled patches produces one toposolid with two flush sub-divisions, each with its own new type.
- [ ] The source surface is still present after conversion, and deleting it does not change the toposolid.

### 3.11.10 Site components, parking and property lines
**ID:** ARC-SITE-001 · **Priority:** P1 · **Ref:** research/02 §12.4

**Behaviour & rules.**
- **Site Component** (Model Site): trees (planting), entourage and site furniture. The user picks a type and clicks to place. Each component is hosted on the toposolid (its height is the surface height at the insertion point) or on a level.
- **Parking Component:** placed on a toposolid or on a sub-division road. Parking components are usually arrayed. *Pick New Host* rehosts them, and they keep sitting on the surface when moved.
- **Property Line** (Modify Site):
  - *Create by sketching* expects a closed loop. An open loop gives a warning that no area can be computed.
  - *Create by entering distances and bearings* uses a table. Each row has Distance, Bearing (N or S, degrees-minutes-seconds, E or W), Type (Line or Arc), Radius and L/R, with Insert, Delete, Up and Down. The user then clicks to place the result. Bearings are relative to **True North** (the project's site rotation).
  - A sketched property line can be converted to the table form.
  - Property lines report their area and can be tagged.
- **Surface hosting** means that whenever the surface changes, the component's Z is set to the surface height at its (x, y) plus its offset.

**Data (IFC mapping).**
- **Site components:** the IFC class follows their category (planting → `IfcGeographicElement`, 4X3 `VEGETATION`; site furniture → `IfcFurniture`; entourage → `IfcBuildingElementProxy`), contained in `IfcSite`.
- **Parking components:** `IfcBuildingElementProxy` (`ObjectType = "Parking"`) or, when the family is flagged as a parking space, `IfcSpace(PARKING)`.
- **Property lines:** `IfcAnnotation` (`ObjectType = "PropertyLine"`) contained in `IfcSite`, with an `Annotation` curve. Their area is written as a property, and the bearings table is stored in RXD element data (`recipe.property_line`).

**Commands.**
- `sitecomponent.place(type_id, point, host_id?, rotation?) -> element_id`
- `propertyline.create(loops | table: [{distance, bearing, kind, radius?, side?}], origin?) -> element_id`
- `propertyline.to_table(element_id)`

**Acceptance criteria.**
- [ ] A property line entered as a table with four rows (N 0°E 30 m, N 90°E 20 m, S 0°E 30 m, S 90°W 20 m) closes and reports 600 m².
- [ ] A tree on a toposolid moves up 1 m when the surface under it is raised 1 m.

---

## 3.12 Modify tools

These tools are on the **Modify** tab and are repeated on every `Modify | <Element>` contextual tab. All of them support both invocation orders (ARC-TOOL-001). Their options sit on the contextual tab and are remembered for the session. The common rules in ARC-MOD-000 apply to all of them.

### 3.12.1 Common rules for modify operations
**ID:** ARC-MOD-000 · **Priority:** P0 · **Ref:** research/02 §13 (Edge cases), §15.2, §17.12

**Behaviour & rules.**
- **Pins.** A pinned element cannot be moved, rotated, mirrored, scaled or deleted by a user operation. Trying gives "Element is pinned", and the command fails. Implicit moves still apply to pinned elements, because those changes come from the element's dependencies: following a host, a level change, *Moves With Nearby Elements*, or an array or group regeneration.
- **Constraints.** Every geometric modification is applied to the target elements and then run through the constraint solver (ARC-CNST-002).
  - If satisfying the constraints needs other elements to move, they move too. For example, a locked alignment drags its partner.
  - If the constraints cannot be satisfied, the transaction is rolled back, and the "Constraints are not satisfied" dialog offers **Remove Constraints** (deletes the conflicting constraints and retries) or **Unjoin Elements** (where joins conflict).
- **Hosting.** Hosted inserts move only along their host unless the command has `disjoin = true`.
- **3D views.** Rotate and Mirror in a 3D view work in the active work plane. The work plane must be set first.
- **Mirroring** a family instance sets its read-only *Mirrored* flag (it can be scheduled and filtered). On hosted doors and windows, mirroring can flip hand or facing; Reviera stores the resulting hand and facing explicitly.
- **Groups.** Modifying a member of a group instance outside the group editor is refused. The user is pointed to *Edit Group* (ARC-GRP-002).
- **Copy and Copy to Clipboard.** Copy keeps elements on the same level and in the same view. Pasting to other levels is done with Paste Aligned (ARC-MOD-021).
- **Option memory.** Tool settings (Multiple, Copy, Disjoin, Constrain, Graphical or Numerical, joint gap, Delete Inner Segment, array options, Prefer) are kept for the session and can be stored in the user settings.

### 3.12.2 Modify tool requirements

| ID | Tool | Priority | Shortcut | Behaviour | Commands |
|---|---|---|---|---|---|
| ARC-MOD-001 | **Align** | P0 | AL | Options: *Multiple Alignment* (or hold Ctrl), *Lock*, *Prefer* (Wall faces, Wall centerlines, Faces of core or Center of core). The user picks the **reference** first, then the element edge, face or line that should move. The element is translated (or, for curves such as wall ends, extended) so that the picked geometry becomes coplanar or collinear with the reference. *Lock* creates an alignment constraint (ARC-CNST-002). Tab cycles references. Works in plan, elevation and 3D. Also aligns surface-pattern lines and extends wall or beam ends | `element.align(reference_ref, target_refs: [ref], lock?: bool)` |
| ARC-MOD-002 | **Offset** | P0 | OF | *Graphical* or *Numerical* (value), and *Copy* (Ctrl toggles it). Hovering an element or chain (Tab selects a chain of the same family) previews a parallel copy on the cursor side; a click creates it. Graphical mode picks a point, drags and accepts a listening dimension. Works for lines, walls, beams and braces. Walls are offset by their location line | `element.offset(element_ids, distance, side_point, copy: bool) -> [element_id]` |
| ARC-MOD-003 | **Mirror – Pick Axis / Draw Axis** | P0 | MM / DM | *Copy* (on by default). Pick Axis takes an existing line, reference plane, grid or wall face. Draw Axis takes a temporary two-point axis. Elements are reflected through the vertical plane that contains the axis (in plan), or through the plane through the axis that is perpendicular to the work plane (in 3D). Inserts can be mirrored without their hosts if a valid host lies at the mirrored position | `element.mirror(element_ids, axis: Line | ref, copy: bool) -> [element_id]` |
| ARC-MOD-004 | **Move** | P0 | MV | *Constrain* (orthogonal or along the element only), *Disjoin*, and *Multiple*. The user clicks a start point and an end point, or types a distance. **Disjoin** breaks joins and hosting: a hosted insert can move to another host, which gives it a new GlobalId, resets its Mark and loses its dimension references. Arrow keys nudge the selection by the zoom-dependent increment, and Shift+arrow by ten times that. Dragging moves too, with snapping | `element.move(element_ids, vector, disjoin?: bool) -> [element_id]` |
| ARC-MOD-005 | **Copy** | P0 | CO / CC | Copies the selection by a vector, staying in the same level and view context. Select first, or start the tool, select and press Enter. Options: *Constrain* (orthogonal directions only, or along the element when one linear element is selected), *Disjoin* (a copied hosted insert may land on another host, which it then joins; without it the copy stays on the source host and the vector is projected onto that host) and **Multiple** (after each destination the tool asks for another, each measured from the same base point, until Esc). The user picks a base point, then destinations, or types distances (ARC-DIM-002). Copies get new GlobalIds and the next free Mark; joins and constraints among copied elements are copied, those to uncopied elements are dropped. Copies stay on the source's level even when the destination lies at another level's height (use Paste Aligned, ARC-MOD-021). Pinned elements can be copied, and their copies are not pinned. A copied group member is refused outside the group editor | `element.copy(element_ids, vectors: [vector]) -> [[element_id]]` (one inner list per destination, one undo step for all) |
| ARC-MOD-006 | **Rotate** | P0 | RO (R3 sets a new centre) | *Disjoin*, *Copy*, *Angle* (typed; enabled after the first ray), *Center of rotation*: Place or Default. The centre control starts at the selection's bounding-box centre; the user can drag it, click it and pick, press Spacebar and pick, or use *Place*. The user clicks a first ray and a second ray, or types an angle. The axis is normal to the view (in 3D, normal to the work plane). Walls cannot be rotated in elevation. Hosted inserts cannot be rotated away from their wall | `element.rotate(element_ids, center: point, angle, axis?: vector, copy?: bool) -> [element_id]` |
| ARC-MOD-007 | **Trim/Extend to Corner** | P0 | TR | The user clicks two elements, each on the part to **keep**; Tab cycles overlapping candidates. Both curves are trimmed or extended to their intersection (for arcs, the intersection nearer to the click points). Works for walls (location lines; the end joins are re-solved into an L join), model and detail lines, beams and braces. Both elements must lie in the same plane (the plan, or the same work plane). Parallel elements give "Elements do not intersect" and nothing changes. Inserts that end up outside a trimmed wall are deleted after a warning. Pinned elements and group members are refused (ARC-MOD-000). The tool stays active for more pairs until Esc; each pair is one undo step | `element.trim_corner(a: (element_id, keep_point), b: (element_id, keep_point))` |
| ARC-MOD-008 | **Trim/Extend Single** | P0 | — | The user picks the **boundary** first: a line, wall face or location line, reference plane, grid, level (in elevation) or model edge; Tab cycles candidates. Then the user picks the element to change. If it crosses the boundary or its extension, the part on the clicked side is kept and the rest is removed; if it stops short, its nearer end is extended to the boundary. The boundary never changes and stays active, so more elements can be picked until Esc or until another trim mode is chosen (which clears the picks). Valid targets: walls, lines, beams, braces and MEP curves. An element parallel to the boundary is refused ("Element cannot be trimmed or extended to the boundary"). Joins at the moved end are re-solved | `element.trim_extend(boundary_ref, target: (element_id, keep_point))` |
| ARC-MOD-009 | **Trim/Extend Multiple** | P0 | — | The user picks a boundary, then clicks or box-selects targets. A crossing window (dragged right to left) selects the elements it touches. Works for walls, lines, beams, braces, ducts, pipes, conduit and cable tray. A window dragged left to right takes only elements fully inside it. For boxed elements, the kept part is the one on the side of the boundary where the box was started; for clicked elements, the clicked side. Elements that cannot reach the boundary (parallel, or in another plane) are skipped and listed in the status bar. Each click or box is one undo step. The same pin, group and join rules as Trim/Extend Single apply | `element.trim_extend_multiple(boundary_ref, targets: [(element_id, keep_point)])` |
| ARC-MOD-010 | **Split Element** | P0 | SL | *Delete Inner Segment*. Works for walls (ARC-WALL-011), lines, beams, MEP curves (MEP chapter: inserts unions or couplings according to routing preferences) and toposolids (by sketch, ARC-TOPO-002). A click on the element divides it there into two elements of the same type and parameters; with *Delete Inner Segment* the user clicks two points and the piece between them is removed. The original GlobalId stays with the piece that contains the start point. Hosted elements go to the piece that contains their insertion point; dimensions to the new ends are not created and dimensions spanning the cut keep their references. A click within `TOL_SHORT` of an end does nothing ("Element too short"). Walls can also be split horizontally in elevation and 3D (ARC-WALL-011). Pinned elements and group members are refused (ARC-MOD-000) | `element.split(element_id, point, delete_inner?: point2) -> [element_id]` |
| ARC-MOD-011 | **Split with Gap** | P1 | — | *Joint Gap*, between 1.6 and 305 mm. Works for walls, structural framing and columns. A click removes a gap of that width centred on the click point and leaves two elements; for structural columns the cut is horizontal (in elevation, section or 3D). The new ends get *Disallow Join* and a locked dimension that records the gap; the joint is closed by unlocking the dimension, allowing the joins and dragging the ends together. A value outside the range is refused with the range in the message; the last value is remembered for the session (default 20 mm). Not available for curtain walls, stacked walls, or slanted walls cut by a vertical plane. Hosted inserts that fall inside the gap are deleted after a warning | `element.split_with_gap(element_id, point, gap) -> [element_id]` |
| ARC-MOD-012 | **Scale** | P1 | RE | *Graphical* (origin, then two vectors whose length ratio is the factor) or *Numerical* (factor). Scaling is proportional about the origin. Only these elements can be scaled: walls (curve only; thickness unchanged), lines, images, imported CAD, reference planes and dimension positions. If any selected element cannot be scaled, the tool is disabled | `element.scale(element_ids, origin, factor)` |
| ARC-MOD-013 | **Pin / Unpin** | P0 | PN / UP | The pushpin control toggles the pin. *Activate Controls* (Modify \| Multi-Select › Selection) shows pushpins on a large selection. Type-driven curtain grids and mullions are pinned automatically, and unpinning one makes it independent. A pinned element cannot be moved, rotated, mirrored, scaled, flipped or deleted by the user ("Element is pinned") but still follows implicit changes (host, level, Moves With Nearby Elements, arrays, groups; ARC-MOD-000). Clicking the pushpin unpins (the pushpin shows a red cross) and clicking again re-pins in the new position. Unpin also works from the tool (UP) on a selection. The *Select Pinned Elements* toggle can make pinned elements unselectable (ARC-SEL-001). Elements already in the requested state are ignored. The pin state is saved with the element and is undoable | `element.pin(element_ids)`, `element.unpin(element_ids)` |
| ARC-MOD-014 | **Delete** | P0 | DE / Del | Deletes without using the clipboard. Cascades (hosted inserts, a level's views and hosted elements, group members) are listed in a confirmation dialog before commit. Deletes fail on pinned elements (ARC-MOD-000). The dependants collected also include openings, parts (ARC-PART-003), and dimensions and tags whose references disappear; the dialog lists them by category with counts and offers Delete or Cancel. With `confirm_dependents = false` (scripts and MCP) the cascade happens without asking and the result lists it. Deleting a placed room in a view unplaces it (ARC-ROOM-001). Deleting the last member of a group instance deletes the instance. Elements in links and members of group instances outside the group editor cannot be deleted | `element.delete(element_ids[], confirm_dependents?=true) -> element_ids[]` (everything deleted, dependants included) |
| ARC-MOD-015 | **Match Type Properties** | P1 | MA | A paintbrush cursor: the user clicks the source, then targets of the same category. Only the **type** is copied. *Select Multiple* lets the user box-select and then Finish. Esc once empties the brush; Esc twice exits. Works across tiled views, and a type in the Project Browser can be the source | `element.match_type(source_id, target_ids)` |
| ARC-MOD-016 | **Cut Geometry / Uncut** | P1 | — | *Multiple Cut*. The user picks the element to be cut, then the cutting element. Allowed pairs: toposolid, floors, ceilings, roofs, slab edges, fascias, gutters, soffits, walls, wall sweeps, structural foundations and mass voids; family voids flagged *Cut with Voids When Loaded* cut hosts. Creates a `cut` edge, and `BRepAlgoAPI_Cut` runs at regeneration. Stored in RXD document `model/constraints`, and in IFC as `IfcRelInterferesElements` (`InterferenceType = "Cut"`, `ImpliedOrder = TRUE`) | `geometry.cut(target_id, cutter_ids)`, `geometry.uncut(target_id, cutter_ids?)` |
| ARC-MOD-017 | **Join Geometry / Unjoin / Switch Join Order** | P0 | — | *Multiple Join*. Joining removes the visible edge between the elements and gives them a shared cut-pattern display. One element cuts the other, according to this precedence table: walls cut columns (architectural); structural elements cut non-structural hosts; floors, ceilings and roofs cut walls; gutters, fascias and slab edges cut their hosts; otherwise the first element picked cuts the second. *Switch Join Order* swaps them. The material of the first pick is applied to both in the merged cut display. In IFC: `IfcRelConnectsElements` (Name "JoinGeometry") plus `IfcRelInterferesElements` for the cut direction | `geometry.join(a_id, b_ids)`, `geometry.unjoin(a_id, b_ids?)`, `geometry.switch_join_order(a_id, b_id)` |
| ARC-MOD-028 | **Boolean geometry between elements** (owner clarification, 2026-10-13) | P1 | — | Beyond Cut and Join: **Intersect** keeps only the volume two elements share (for example a vault cut to a room's footprint), **Subtract** removes one element's volume from another's and **Union** fuses elements into one body. The first pick is the target and keeps its category, type and parameters; the others are *tools*. A tool may be hidden afterwards (*Hide tool*), deleted (*Consume tool*) or kept. The operation is a `csg` edge in the regeneration graph (`BRepAlgoAPI_Common`, `_Cut`, `_Fuse`), re-run whenever either element changes, and stored in RXD document `model/constraints`. In IFC the target's body is the result (as `IfcBooleanResult` of the two bodies where both are swept or CSG solids, otherwise a faceted B-rep), with `IfcRelInterferesElements` (`InterferenceType` "Intersect", "Subtract" or "Union"). Undoable, and *Remove Boolean* restores the target | `geometry.boolean(target_id, tool_ids, op: intersect\|subtract\|union, tools?: keep\|hide\|consume)`, `geometry.remove_boolean(target_id, tool_ids?)` |
| ARC-MOD-018 | **Wall Joins** | P0 | — | Modify › Geometry › **Wall Joins**: hover a join (its node is marked by a square) and click it, or window-select several nodes. The contextual tab then offers *Butt*, *Miter* and *Square Off* (L joins only), *Previous* / *Next* to cycle which wall runs through (one node only), *Display* (*Use View Setting*, *Clean Join*, *Don't Clean Join*) and *Allow* / *Disallow Join*. Changes apply immediately and the tool stays on the node until Esc. Options that do not suit the selected nodes are disabled. The geometry rules, join limits and IFC mapping are in ARC-JOIN-001 | `wall.join_set_type(node_ref \| wall_end_refs, type: Butt\|Miter\|SquareOff)`, `wall.join_cycle_order(node_ref, step: +1\|-1)`, `wall.join_set_display(node_ref, display: ViewSetting\|Clean\|DontClean)`, `wall.set_join_allowed(wall_id, end: Start\|End, allowed: bool)` |
| ARC-MOD-019 | **Split Face** | P1 | SF | The user picks a face (Tab cycles), sketches a closed loop inside it or an open chain ending on its edges, and presses Finish. The face is divided into regions that can be painted separately. The element's structure is unchanged. Not available on arc or elliptical walls. Deleting a split region removes it. Family instances (except columns) must be split in the family editor. OCCT: `BRepFeat_SplitShape` with the projected sketch edges | `face.split(face_ref, curves: [curve]) -> [face_key]` |
| ARC-MOD-020 | **Paint / Remove Paint** | P1 | PT | The Material Browser is available while Paint is active. The user hovers a face (Tab cycles) and clicks to apply a material. The status bar shows existing paint. Works on walls, floors, roofs, masses, columns and in-place families. Does not work on loadable component instances. Painting a face that has a model pattern makes the pattern lines dimensionable and alignable. Paint is stored per face sub-reference in RXD element data (`paint`), and exported as an `IfcStyledItem` on the faces, which IFC supports only per representation item: Reviera splits the painted faces into their own `IfcFacetedBrep` item when exporting | `face.paint(face_refs, material_id)`, `face.remove_paint(face_refs)` |
| ARC-MOD-021 | **Copy, Cut and Paste (clipboard)** | P0 | Ctrl+C / Ctrl+X / Ctrl+V | The Paste menu offers: **Paste from Clipboard** (enters paste mode, where the elements can be moved before Finish or Cancel); **Aligned to Selected Levels** (a searchable multi-select list); **Aligned to Selected Views** (view-specific elements; similar view types only); **Aligned to Current View**; **Aligned to Same Place** (also between design options, worksets and models that share coordinates); and **Aligned to Picked Level** (elevation views: pick a level line). The clipboard holds an IFC fragment plus its RXD element data, so pasting between Reviera instances works | `clipboard.copy(element_ids)`, `clipboard.cut(element_ids)`, `clipboard.paste(mode: Free(vector)|Levels([id])|Views([id])|CurrentView|SamePlace|PickedLevel(id)) -> [element_id]` |
| ARC-MOD-022 | **Create Similar** | P0 | CS | Starts the placement tool for the selected element's type, with its instance parameters as the *New* defaults. For walls the height properties are copied, and the top constraint is adapted to the active level. Esc twice exits. Exactly one element must be selected (also from the context menu). Only parameters that the family or system element defines are copied (dimensions, offsets, heights, hosting and placement options); user parameters such as Comments and Mark are not. For walls these are Base Offset, Unconnected Height, Top/Base Extension Distance, Top Constraint and Top Offset; on another level the top constraint moves the same number of levels up. Slanted walls copy only their type. The defaults hold until the user changes the type in the Type Selector. Not available for elements without a placement tool (groups, links, imports, in-place families): "Create Similar is not available for this element" | `tool.create_similar(element_id)` |
| ARC-MOD-023 | **Measure** | P0 | — | *Between Two References* and *Along Element*. A temporary read-out; no element is created. **Between Two References**: click a start point or reference and an end point or reference (snaps apply; `SP` measures perpendicular to the target); the read-out shows the distance and, in plan and elevation, the angle from horizontal. In 3D views Ctrl constrains the second point to the plane through the first point perpendicular to the view direction. Option *Chain*: each click adds a segment, the status bar keeps a running total, and a double-click ends the chain. **Along Element**: click a wall, model line, beam or MEP curve (Tab takes a chain) to show its length; it is not available in 3D or for detail lines. Read-outs stay until the next measurement; Esc once clears, twice exits | `measure.between(ref_a, ref_b) -> {distance, dx, dy, dz, angle?}` *(query)*, `measure.along(element_id) -> length` *(query)*, `measure.chain(points: [point]) -> {segments: [f64], total}` *(query)* |
| ARC-MOD-024 | **Demolish** | P1 | — | A hammer cursor: each clicked element gets Phase Demolished = the view's phase, and its graphics follow the phase filter. Undone by setting Phase Demolished to None. Elements that can be demolished highlight on hover. The change is model data, so the element shows as demolished in every view of that phase. Elements created after the view's phase, or already demolished in an earlier phase, are refused with a status-bar message. Hosted inserts are demolished with their host. Nothing is deleted or unhosted, and rooms in later phases recompute without the element. Linked elements and annotation cannot be demolished. Esc or Modify exits; each click is one undo step | `element.demolish(element_ids[], phase_id? (default: the active view's phase), view_id?) -> ()` |
| ARC-MOD-025 | **Linework** | P1 | LW | A view-specific override of the line style of model edges. Specified in the DOC chapter. The user picks a line style on the contextual tab (including *Invisible lines*, and *By Category* to remove an override) and clicks edges, Tab cycling between coincident edges; dragging an edge's end controls limits the override to part of the edge. It works on projected and cut edges in plan, section, elevation and orthographic 3D views. The element itself is unchanged, and an override is dropped with a warning when its edge disappears (DOC-GFX-007) | `view.set_linework(view_id, edge_refs[], line_style_id?)` (DOC) |
| → ARC-MOD-026 | **Array** | P0 | AR | ARC-MOD-026 below | `element.array_linear`, `element.array_radial`, `array.set_count` |
| ARC-MOD-027 | **Repeat last command** | P0 | RC / Enter | Re-runs the last tool with its options. Enter repeats only when no tool is active and nothing is being edited; RC and the context-menu item *Repeat [tool name]* always repeat. The repeated tool starts with the options it had when it ended (Multiple, Copy, Disjoin, joint gap, and so on) and the same type, but with no selection, so select-first tools ask for a selection. Commands run from Lua or MCP are not recorded as "last"; view-navigation and selection commands are not either. After a session tool (sketch, group editor) the repeat starts a new session. If the last tool is unavailable in the current view, the status bar says so | `tool.repeat_last() -> session_id` |

**Acceptance criteria (modify tools).**
- [ ] Align with Lock between a wall face and a grid keeps the wall on the grid when the grid moves 1 m.
- [ ] Moving a pinned wall with `element.move` fails with "Element is pinned", and the model is unchanged. Moving its base level still moves it.
- [ ] Move with Disjoin from wall A to wall B rehosts a window, and its GlobalId changes.
- [ ] Rotating four walls about a placed centre by 30° keeps every join between them.
- [ ] Trim/Extend to Corner on two non-touching walls produces an L join with no gap.
- [ ] Paste Aligned to Selected Levels (Levels 2–5) creates four copies, each based on its own level.
- [ ] Join Geometry between a floor and a wall makes the floor cut the wall, and *Switch Join Order* reverses that.
- [ ] Mirror with Copy of a right-handed door produces a left-handed door with *Mirrored* = true.

### 3.12.2a Volume overlaps, virtual volumes and their resolution
**ID:** ARC-MOD-030 · **Priority:** P0 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** One low-level operation, *volume overlap*, that finds where element bodies (and virtual volumes, ARC-MOD-031) occupy the same space and measures it. It serves:
- boolean resolution (ARC-MOD-032);
- clash detection (COL-CLASH-001/002);
- door-swing and accessibility clearance checks;
- automatic openings for pipes, ducts and conduits (MEP-PEN-001).

*(Owner direction, 2026-10-13: "We will need a generalized overlap detector… Volume overlay is a low-level operation used for multiple cases.")*

**Behaviour & rules.**
- **Bodies.** Every element's body as closed meshes in project coordinates:
  - Reviera's own elements regenerated from their recipes (walls joined and cut by their openings, floors, ceilings, decks, roofs with their trims, stairs, railings, gates, doors and windows);
  - elements known only from the IFC file, as tessellated for the 3D view.

  The 3D view draws the same bodies, so what is checked is what is seen.
- **Broad phase.** Axis-aligned boxes, sorted along x and swept; pairs whose boxes overlap by more than the tolerance in every axis go on.
- **Narrow phase.** A grid of vertical lines across the two boxes' common box (up to the resolution each way, default 48). On each line every closed mesh is crossed an even number of times, which gives the intervals inside it; a body's intervals are their union. Summed over the cells, the length of the two bodies' common intervals gives:
  - the overlap's volume;
  - its depth (the longest vertical run);
  - its box and its centroid.
- **Tolerance.** Runs shorter than the tolerance (default 1 mm) and overlaps smaller than the minimum volume (default 1 cm³) are contact, not overlap. Touching faces never overlap.
- **Related pairs.** Pairs that overlap by design are left out unless asked: an element and its host (a door and its wall, a railing and its deck or stair), a gate and its railing, a dormer's parts and their main roof.
- **Accuracy.** The volume converges with the resolution. Slivers thinner than a cell in both plan directions can be missed, so a check needing certainty raises the resolution. Exact solid booleans (OCCT `BRepAlgoAPI_Common`) refine flagged pairs where present.

**UI.** View › Coordination › **Check Overlaps** lists the overlaps largest first (the pair, the volume and the depth). Picking one selects both elements and frames them.

**Commands.** `overlap.find(element_ids?, against?, tolerance?, min_volume?, resolution?, include_related?, limit?) -> {overlaps: [{a, b, a_label, b_label, volume, depth, min, max, centroid}], bodies}`. It is available to Lua scripts and MCP like every command.

**Acceptance criteria.**
- [x] Two 2 m cubes offset by 1 m each way overlap by 1 m³, with their centroid at the common box's centre; cubes that touch, or stand one on the other, do not overlap.
- [x] A room of four joined walls with a door, a floor under it and an attached roof has no overlaps. A wall run through one of its walls and a railing through another are both found, the wall with 0.08 m³.
- [x] A pipe through a hole in a slab does not overlap the slab; one through the slab's edge does, by its volume in the slab.

**Implementation notes (2026-10-13).**
- `reviera_core::bodies` gives every Reviera element's closed parts (the 3D view now draws them), and `reviera_core::overlap` measures them.
- `overlap.find` is an application command, so imported elements are included.
- The window lists the overlaps and frames a picked pair.
- Later: overlap boxes drawn in the view, a background check with progress for large models, and the OCCT refinement.

**ID:** ARC-MOD-031 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** *Virtual volumes*: non-physical spaces that elements need kept clear. They take part in overlap checks (ARC-MOD-030) but are never built, scheduled as material or exported as physical bodies. *(Owner requests, 2026-10-13:)*
- *"virtual volume objects for door swings, and accessibility constraints that detect collision with other geometry";*
- *"accessibility related space requirements around bathroom objects";*
- *"the virtual volumes should have an option to appear as translucent objects".*

**Behaviour & rules.**
- **Sources.**
  - *Door swings:* the volume a door leaf sweeps (its arc in plan, to the leaf's height), for each leaf, on the swing side.
  - *Accessibility clearances*, by the project's accessibility standard (a selectable rule set, ADA 2010 and ISO 21542 to start):
    - the clear floor space at fixtures (toilet, lavatory, urinal, shower, bath, kitchen sink and appliances);
    - grab-bar and transfer zones;
    - a wheelchair turning space in rooms flagged accessible (a 1500 mm circle or T-shape);
    - maneuvering clearances at doors (the push and pull sides);
    - headroom over stairs and routes.
  - *Element-defined:* a type may carry its own clearance boxes (equipment service access, panel working space), with a name and the categories that may intrude.
- **Regeneration.** Each volume is regenerated with its element. It moves, flips and resizes with it.
- **Checks.** A virtual volume overlapping a physical body is flagged, except by the bodies the rule allows: a door's swing may cross its own wall's opening, and a fixture's clear space may contain its own fixture and the floor. Two virtual volumes may overlap where the rule allows (two clear floor spaces may share).
- **Display.** Per view, Visibility/Graphics › *Virtual Volumes*: *Hidden*, *Translucent* (the default in coordination views) or *Solid*, with a colour per kind. Flagged volumes show in the warning colour.
- **Rule sets** are RXD data (`standards/accessibility`), editable and versioned with the project.

**Data (IFC mapping).** Virtual volumes are not exported as physical elements. On request (*Export clearances*), they are written as `IfcSpatialZone` (`PredefinedType = USERDEFINED`, ObjectType "Clearance") with their element related by `IfcRelAssignsToProduct`.

**Commands.**
- `clearance.list(element_ids?) -> [{element, kind, rule, mesh}]`
- `clearance.check(element_ids?, rules?) -> [overlap]`
- `clearance.set_rule_set(standard)`
- `view.set_virtual_volumes(view_id, mode: hidden|translucent|solid)`

**Acceptance criteria.**
- [ ] A 900 mm door's swing volume flags a cabinet placed within its arc, and not the wall it opens from.
- [ ] Under ADA 2010, a toilet's 1525 × 1420 mm clear floor space flags a lavatory placed inside it.
- [ ] A room flagged accessible without a 1500 mm turning circle clear of fixtures is reported.

**Implementation notes (2026-10-13).** As built:
- **Door swings:** each leaf's quarter-circle sweep, from the sill to the leaf's top, on the swing side. Always present.
- **ADA 2010 404.2.4.1 front-approach clearances:** with the rule set `ada_2010` (`clearance.set_rule_set`, held on the project), each door gets a pull-side clearance (1525 mm deep, the clear width plus 455 mm past the latch) and a push-side clearance (1220 mm deep, the clear width), both taken to 2030 mm high.
- **Checks:** `clearance.check` tests every volume against every body (imported elements too), allowing the door itself and its wall, and door swings against each other, so doors that would hit each other are found. View › Check Overlaps lists intrusions with the overlaps, and picking one shows the volumes translucent.
- **Display:** View › Virtual Volumes cycles the view between hidden (the default), translucent and solid; the choice is stored on the view (`view.set_virtual_volumes`). Plans and sections never cut the volumes, and Zoom to Fit ignores them.
- **Later:**
  - fixture clear floor spaces and grab-bar zones (with components);
  - turning spaces in accessible rooms;
  - latch-side and hinge-side approaches, and the closer and latch variants of 404.2.4;
  - ISO 21542;
  - element-defined service clearances;
  - flagged volumes drawn in the warning colour;
  - IFC export of clearances.

**ID:** ARC-MOD-032 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** Overlaps between elements are flagged, and the user resolves each by choosing a boolean (CSG) mode. *(Owner request, 2026-10-13: "collisions should be detected, flagged and allow the user to resolve by selecting a CSG mode to resolve the overlaps.")*

**Behaviour & rules.**
- **Flagging.** Overlaps found by ARC-MOD-030 are listed, and optionally shown in the view as their boxes. A setting checks after each edit, and only the edited elements.
- **Resolution modes**, chosen per overlap:
  - *Cut A by B* / *Cut B by A* (ARC-MOD-016);
  - *Join* (ARC-MOD-017);
  - *Subtract*, *Intersect* or *Union* (ARC-MOD-028);
  - *Move apart* (along the shortest separating axis);
  - *Accept* (marked intended, no longer flagged).

  The chosen relation is kept and re-applied when either element changes.
- **Accepted overlaps** are stored by GlobalId pair with who accepted them and when (RXD `coordination/overlaps`).

**Commands.** `overlap.resolve(a, b, mode: cut_a|cut_b|join|subtract|intersect|union|move_apart|accept) -> ()`, `overlap.accepted() -> [pair]`.

**Acceptance criteria.**
- [ ] A beam through a wall flagged by Check Overlaps and resolved with *Cut B by A* leaves the wall cut by the beam, and the overlap is no longer flagged.
- [x] An overlap resolved with *Accept* stays unflagged after both elements move.

**Implementation notes (2026-10-13).** As built:
- **Accept** (`overlap.accept`, or `overlap.resolve` mode `accept`): pairs are held on the project by GlobalId, with who accepted them, when (UTC) and a note. `overlap.find` and `clearance.check` leave them out (`include_accepted` brings them back). `overlap.accepted` lists them, and `accepted: false` takes one back.
- **Move apart** moves one element (`moves`: `a` or `b`) by the shortest of the four plan moves (±x, ±y) that clears the other's box, re-checking, as one undo step. It is refused if they still overlap.
- **Boolean modes** (cut A by B, cut B by A, join by precedence, intersect, union): each becomes a kept boolean (ARC-MOD-028, below).
- **Window:** picking an overlap in Check Overlaps offers:
  - Select and frame;
  - Accept as intended;
  - the cuts, join, intersect and union;
  - Move one of them apart.

  View › Coordination › Remove Boolean removes the selected elements' booleans.
- **The solid boolean kernel** (`reviera-geom`, OCCT 7.9):
  - each closed part becomes a solid (its triangles sewn into a shell, turned to face out);
  - `BRepAlgoAPI_Cut`, `_Common` or `_Fuse` runs on the compounds;
  - coplanar faces are merged (`ShapeUpgrade_UnifySameDomain`), and the result is triangulated back.
- **IFC:** a boolean target's body is written as its shaped result (faceted B-reps; a wall's in its own frame, so its openings keep their placement). Each relation is an `IfcRelInterferesElements` (IFC4) with its InterferenceType (`Subtract`, `Intersect`, `Union`) and ImpliedOrder true. IfcOpenShell reads the result back: a cut wall's tessellated volume is the cut volume.
- **Kept booleans** (`geometry.boolean`, `geometry.remove_boolean`):
  - The relations are held on the project (`rxd:csg`) by GlobalId.
  - The application applies them wherever bodies are built, so the 3D view and the overlap engine see the result. Results are cached by their inputs, so a target is re-made only when it or its tool changes.
  - A union hides the tool's own body. Pairs in a boolean count as related, so they are no longer flagged.
  - A cycle (a tool already shaped by its target) is refused.
- **Later:**
  - IFC bodies as `IfcBooleanResult` where both bodies are swept or CSG solids;
  - checking after each edit;
  - drawing the overlap boxes in the view;
  - moving in z;
  - storing accepted pairs in `coordination/overlaps`.

### 3.12.3 Array
**ID:** ARC-MOD-026 · **Priority:** P0 · **Ref:** research/02 §1 (2026), §13

**Workflow.**
- **Options:** **Linear** or **Radial**, **Group and Associate**, **Number** (including the original), **Move To**: *2nd* or *Last*, *Constrain*, *Angle* (radial), and the centre of rotation (*Place* is the default for radial arrays).
- **Linear:** click the start, then the second member (Move To: 2nd) or the last member (Move To: Last), or type a distance.
- **Radial:** place the centre, pick the start ray, then pick the end ray or type an angle.

**Behaviour & rules.**
- **Without Group and Associate**, the copies are independent elements.
- **With Group and Associate**:
  - Each member is a model-group instance of an automatically created group type (ARC-GRP-001), and the array is an **array set** that records the base vector or angle, the count, the *Move To* mode and the members.
  - Selecting the array shows an editable **count label**.
  - Changing the count with **Append to End** keeps the spacing and adds or removes members at the end. With Append to End off, the endpoints stay fixed and the spacing is redistributed (only when the array was created with Move To: Last).
  - Dimensions between array members can be given a parameter label (DOC chapter).
- Detail elements and model elements cannot be arrayed together.
- **Regeneration.** The array set is a node with `group` edges to its members. Changing the count adds or removes group instances in one transaction.

**Data (IFC mapping).** Each member is an `IfcGroup` (ARC-GRP-001). The array set is an `IfcGroup` with `ObjectType = "ArraySet"` whose members are the member groups (`IfcRelAssignsToGroup`). The parameters are stored in RXD document `model/groups`.

**Commands.**
- `element.array_linear(element_ids, vector, count, move_to: Second|Last, group_and_associate: bool, constrain?: bool) -> array_id | [element_id]`
- `element.array_radial(element_ids, center, angle, count, move_to: Second|Last, axis?, group_and_associate: bool) -> array_id | [element_id]`
- `array.set_count(array_id, count, append_to_end: bool)`

**Acceptance criteria.**
- [ ] A linear array of 5 chairs at 0.8 m, with Group and Associate on, can be changed to 7 by editing the count label, and the spacing stays 0.8 m.
- [ ] A radial array of 8 columns over 360° (Move To: Last, 360°) places the columns 45° apart without duplicating the first one.

---

## 3.13 Groups

### 3.13.1 Group types and instances
**ID:** ARC-GRP-001 · **Priority:** P1 · **Ref:** research/02 §14, §17.13

**Purpose.** Reusable sets of elements whose instances stay identical.

**Workflow.**
- **Create.** The user selects elements and groups and chooses Create › **Create Group** (`GP`), enters a name, and optionally opens the group in the editor. Architecture › Model › Model Group › *Create Group* creates an empty group in the editor.
- **Place.** Architecture › Model › Model Group › *Place Model Group*, or Annotate › Detail Group › *Place Detail Group*, or dragging from the Project Browser (Groups › Model or Detail), or right-click › *Create Instance*.

**Behaviour & rules.**
- **Kinds:**
  - **model groups** (model elements, which may include rooms);
  - **detail groups** (view-specific elements);
  - **attached detail groups** (a detail group bound to a model group for one view kind: plan, or section/elevation; never 3D).

  A group cannot mix model and detail elements. Selecting both and choosing Create Group makes a model group plus an attached detail group.
- Dimensions, tags and keynotes can only be grouped together with the elements they refer to. Otherwise they go into an attached detail group.
- **Origin.** A group has an origin with three drag controls: the centre moves the origin, and the arm ends rotate it about Z. When placing or pasting, the cursor holds the origin. Swapping the type keeps the origin's position.
- **Instance parameters:** Reference Level, **Origin Level Offset** (the group's height above its level; unhosted components may not follow), Mirrored (RO), phase and similar.
- **Type operations:** Duplicate, Rename and **Swap** (with the Type Selector). On swap, attached detail groups with the same name are mapped automatically and dependent references are re-targeted. Where that cannot be done, Reviera warns.
- **Nested groups** are allowed and are selected with Tab.
- **Implementation model.**
  - A group **type** holds a *template*: member elements in the group's local frame, each with a stable member key.
  - Each **instance** is a transform plus a set of excluded member keys.
  - Each instance's members are real elements with their own GlobalIds, created and regenerated from the template. They are linked by `group` edges.
  - Editing the template re-creates or updates the members of every instance, which preserves member GlobalIds by member key.
  - Member elements are read-only outside the group editor (ARC-MOD-000).
- **Hosted members.** A member whose host is outside the group is rehosted, in each instance, onto the element at the corresponding transformed location if one exists. If none exists, the member is flagged "Host not found".
- **Members on several levels.** A level-based member keeps its level offset relative to the group's reference level. If that level does not exist for an instance, the member uses the reference level plus an offset.
- **Constraints** between group members and outside elements are removed when the group is created, with a warning.

**Data (IFC mapping).**
- Each group **instance** is an **`IfcGroup`** (`ObjectType` = the group type name) whose members are assigned with `IfcRelAssignsToGroup`. Nested groups are assigned the same way.
- IFC has no group-type concept, so the template, the origin, the attached-detail-group bindings, the member keys and the exclusions are stored in RXD document `model/groups`.
- Detail groups are `IfcGroup` objects whose members are view annotation (DOC chapter).

**Commands.**
- `group.create(element_ids, name, kind?: Model|Detail) -> {group_type_id, instance_id, attached_detail_type_id?}`
- `group.place(group_type_id, point, rotation?, level_id?, origin_offset?) -> element_id`
- `group.swap_type(instance_ids, group_type_id)`
- `group.duplicate_type(group_type_id, name) -> group_type_id`
- `group.rename_type(group_type_id, name)`
- `group.ungroup(instance_ids) -> [element_id]`
- `group.set_attached_detail_visible(instance_id, view_id, attached_type_ids)`

**Acceptance criteria.**
- [ ] Creating a group of a bathroom (walls, fixtures and a room) and placing three instances, then adding a fixture in the editor, adds that fixture to all four instances (the original plus three).
- [ ] Ungroup leaves the elements in place as independent elements, and the group type remains in the project.
- [ ] IFC export writes one `IfcGroup` per instance with the right members, and the file still opens in an IFC viewer after the RXD data has been stripped.

### 3.13.2 Group editor
**ID:** ARC-GRP-002 · **Priority:** P1 · **Ref:** research/02 §14

**Workflow.** The user chooses *Edit Group* (`EG`), or double-clicks the group if that is enabled in Options. The group editor offers **Add** (`AP`), **Remove** (`RG`), **Attach** (`AD`; creates an attached detail group), *Group Properties*, **Finish** (`FG`) and **Cancel**.

**Behaviour & rules.**
- While the editor is open, the elements outside the group are shown in half-tone.
- On Finish, the edits are propagated to all instances in one transaction group.
- View-specific elements added to a model group stay in the view and are not added to the group.
- Reviera warns when an instance has been changed outside the editor in a way that breaks identity, which can only happen through scripts that bypass the commands. The warning offers *Restore* or *Make a new group type*.

**Commands.**
- `group.edit(instance_id) -> session_id`
- `group.add_members(session_id, element_ids)`
- `group.remove_members(session_id, element_ids)`
- `group.attach_detail(session_id, view_kind, name) -> session_id`
- `group.finish(session_id)`
- `group.cancel(session_id)`

**Acceptance criteria.**
- [ ] Cancel in the group editor leaves every instance unchanged.

### 3.13.3 Excluding members, saving, loading and converting to a link
**ID:** ARC-GRP-003 · **Priority:** P1 · **Ref:** research/02 §14

**Behaviour & rules.**
- **Exclude.** The user tabs to a member of an instance and clicks the exclude icon (or right-click › *Exclude*). The member is hidden, is not scheduled and is not exported for that instance. Hosted elements are rehosted if possible.
  - **Move to Project** removes the member from that instance and turns it into an independent, editable element.
  - **Restore Excluded Member** (`RB`) and **Restore All Excluded** (`RA`) undo exclusions.
- **Save Group** (right-click in the Project Browser) writes the group as a standalone **`.ifc`** file (with its RXD data). **Load as Group** (Insert tab) loads an `.ifc` file as a group type, with these options: attached details, include levels, include grids. A group saved this way can be edited as a separate project and loaded again into every open project (*Load as Group into Open Projects*).
- **Convert to link.** Modify | Model Groups › **Link** offers *Replace with a new project file* (Save Group, then a link) or *Replace with an existing project file*. A name conflict prompts Yes, No or Cancel. The reverse is *Bind* a link into a group (COL chapter).

**Commands.**
- `group.exclude_member(instance_id, member_key)`
- `group.restore_member(instance_id, member_key | All)`
- `group.move_member_to_project(instance_id, member_key) -> element_id`
- `group.save(group_type_id, path)`
- `group.load(path, options) -> group_type_id`
- `group.convert_to_link(instance_ids, target: NewFile(path)|ExistingFile(path)) -> link_id`

**Acceptance criteria.**
- [ ] An excluded member is absent from the schedule and the IFC export for that instance only.
- [ ] A group saved to `.ifc` and loaded into another project produces the same members, with new GlobalIds.

---

## 3.14 Temporary dimensions, constraints and selection

Temporary dimensions are specified in ARC-DIM-001 and listening dimensions in ARC-DIM-002.

### 3.14.1 Constraint model
**ID:** ARC-CNST-001 · **Priority:** P0 · **Ref:** research/02 §15.2

**Purpose.** Persistent geometric relationships between references.

**Behaviour & rules.**
- **Explicit constraints:**
  - **locked dimensions** (the padlock on a permanent linear, angular or radial dimension);
  - **locked alignments** (Align › Lock, pick-and-lock in sketches);
  - **EQ equality** (the EQ symbol on a multi-segment dimension).
- **Display.** When the referenced elements are selected, explicit constraints are drawn as blue dashed lines. **Reveal Constraints** (View Control Bar) shows every constraint in the view in red.
- **Persistence.** Constraints are **model elements**, not view-specific ones. They survive deletion of the dimension that created them: Reviera asks whether to keep or remove the constraints.
- **Looser constraints** (unlocked dimensions and alignments) are dropped silently when they conflict.
- **Implied constraints** (a wall attached to a roof, joined walls, hosted inserts, sketch lines locked to picked edges, *Moves With Nearby Elements*) are kept unless they conflict.
- **EQ display** is a dimension setting: Value, Equality Text ("EQ") or Equality Formula (DOC chapter).
- In worksharing mode, every element involved must be editable before a constraint can be added (COL chapter).

**Data (IFC mapping).** Constraints are stored in RXD document `model/constraints`. Each record holds: kind (`Distance`, `Alignment`, `Equality`, `Angle`, `Radius`); references (GlobalId plus a named sub-reference, such as `#face:ext` or `#refplane:Center`); the locked value; and the ID of the dimension that created it, if any. IFC has no equivalent; IFC4 `IfcConstraint` is for requirements, not geometry.

### 3.14.2 Constraint solving
**ID:** ARC-CNST-002 · **Priority:** P0 · **Ref:** research/02 §13, §15.2

**Behaviour & rules.**
- **Variables.** Each constrained element contributes the degrees of freedom that its kind exposes: translation for families and inserts (along the host for hosted ones), endpoint and curve offsets for curve-driven elements, and the rotation angle where it applies.
- **Equations.**
  - A distance constraint between two parallel references gives `n·(p_b − p_a) = d`.
  - An alignment constraint gives `n·(p_b − p_a) = 0` with parallel normals.
  - An equality constraint gives `d_1 = d_2 = … = d_k`.
- **Solver.**
  1. A modification first *proposes* new values for the variables it touched.
  2. The solver collects the connected component of constraints reachable from the modified elements.
  3. It solves the component by least-change: minimise the weighted squared change of the untouched variables, subject to the equations. The proposed variables are fixed. It uses Gauss–Newton with a QR decomposition (the `nalgebra` crate, Apache-2.0).
  4. Most constraint graphs are linear (translations), so the solve converges in one iteration. Rotations need up to 20 iterations with a tolerance of `TOL_POINT`.
  5. **Failure.** If the system has no solution (inconsistent) or does not converge, Reviera reports "Constraints are not satisfied", lists the constraints involved, and offers **Remove Constraints**.
  6. **Looser constraints.** Unlocked alignments are tried as soft equations with weight 1e-3 and dropped if the solve fails. Then the solve is retried.
- **Ordering.** The solve runs inside the transaction, before geometry regeneration. Solved variable changes become ordinary parameter changes, which then dirty the regeneration graph.

**Commands.**
- `constraint.lock_dimension(dimension_id, segment?: usize)`
- `constraint.unlock_dimension(dimension_id, segment?)`
- `constraint.lock_alignment(ref_a, ref_b) -> constraint_id`
- `constraint.set_equality(dimension_id, on: bool)`
- `constraint.delete(constraint_ids)`
- `constraint.list(element_id?) -> [Constraint]` *(query)*
- `view.reveal_constraints(view_id, on)`

**Acceptance criteria.**
- [ ] Three walls with an EQ dimension across them stay evenly spaced when the outer wall is moved.
- [ ] Locking a 3000 mm dimension and then moving one wall drags the other wall by the same amount.
- [ ] Locking two dimensions that contradict each other (3000 and 3500 between the same references) fails, and offers Remove Constraints.
- [ ] Deleting a locked dimension prompts to keep or remove its constraint. Keeping it leaves a constraint that Reveal Constraints shows.

### 3.14.3 Selection
**ID:** ARC-SEL-001 · **Priority:** P0 · **Ref:** research/02 §15.3

**Behaviour & rules.**
- **Basic actions.** Click selects. **Ctrl+click** adds and **Shift+click** removes.
- **Window and crossing.** A **window** dragged left to right selects only elements fully inside it. A **crossing** window dragged right to left selects elements it touches.
- **Tab** cycles the candidates (ARC-KEY-001).
- **Select All Instances** (`SA`): *Visible in View* or *In Entire Project*.
- **Filter** dialog: category checkboxes with counts.
- **Select Previous**: Ctrl+←.
- **Selection toggles** (status bar and Modify › Select): Select Links, Select Underlay Elements, Select Pinned Elements, Select Elements by Face, Drag Elements on Selection.
- **Selection is local.** It belongs to the session and is never written to the model. Lua and MCP can query and set the active selection.

**Commands.**
- `selection.set(ids[], mode?: replace|add|remove (default replace)) -> ()`
- `selection.add(element_ids)`
- `selection.remove(element_ids)`
- `selection.get() -> {ids[], by_category}` *(query)*
- `selection.select_all_instances(type_ids? (default: types of the current selection), scope: view|project) -> element_ids[]`
- `selection.filter(category_ids[]) -> element_ids[]`
- `selection.previous() -> ()`
- `selection.set_toggles(links?, underlay?, pinned?, by_face?, drag_on_select?)`

These commands are session-only and are not undoable.

**Acceptance criteria.**
- [ ] A crossing window over the corner of a building selects every wall it touches, and a window over the same area selects only the walls that lie fully inside it.
- [ ] With *Select Pinned Elements* off, pinned elements cannot be selected by clicking.

### 3.14.4 Saved selections (selection sets)
**ID:** ARC-SEL-002 · **Priority:** P1 · **Ref:** research/02 §15.3

**Purpose.** Name a set of elements once and recall it later for editing, checking or graphic overrides.

**Workflow.**
- With a selection active, the user chooses Modify | Multi-Select › Selection › **Save** (or Manage › Selection), and enters a name.
- **Load** recalls a saved selection.
- **Edit** opens Edit Filters, with *Add to Selection*, *Remove from Selection* and edit mode for adding or removing members.

**UI.**
- **Save Selection** dialog: a Name field, pre-filled with "Selection N". Names must be unique among selection sets and view filters (they share one name space, DOC chapter) and non-empty; a duplicate name offers *Replace* or *Cancel*.
- **Retrieve Filters** (Load) dialog: the list of saved selection sets with their member counts, sorted by name, with a search box. Choosing one and OK replaces the current selection with its members that are visible in the current view; a check box *Include elements not visible in this view* selects the others too.
- **Edit Filters** dialog: the list of saved sets with New, Duplicate, Rename and Delete. Choosing **Edit** for a selection set closes the dialog and enters **Edit Selection** mode in the active view: current members are highlighted, and the contextual tab offers *Add to Selection*, *Remove from Selection* and *Finish Selection* / *Cancel*. Clicks and window selections add or remove members according to the active button. The mode records `add {ids}`, `remove {ids}`, `finish` and `cancel` micro-operations so that an interrupted edit can be replayed (conventions §2.1).

**Behaviour & rules.**
- A saved selection is stored as a *selection filter*: an explicit list of GlobalIds.
- **What can be a member.** Model and annotation elements in the current model, group instances (as a whole) and link instances (as a whole). Elements inside a linked model or inside a group instance cannot be members individually; such picks are ignored with a status-bar note.
- Members that are deleted are dropped silently.
- **Copy and undo.** Copying an element does not add the copy to its sets. Saving, editing, renaming and deleting a set are undoable model changes; loading a set only changes the session selection.
- A saved selection can also be used as a view filter for graphic overrides (DOC chapter). Deleting a set that is used in a view or view template asks for confirmation and removes it from those views.
- **Collaboration.** Sets are shared project data: in BIMserver mode they are merged by set ID like other RXD objects (02a §8), and two users adding members to the same set merge without conflict.

**Data (IFC mapping).** Stored in RXD document `model/selection_sets`. The optional export setting *Selection sets as IfcGroup* also writes each set as an `IfcGroup` (`ObjectType = "SelectionSet"`).

**Commands.**
- `selectionset.save(name, element_ids) -> id`
- `selectionset.load(id) -> [element_id]`
- `selectionset.edit(id, add?: [element_id], remove?: [element_id])`
- `selectionset.delete(id)`
- `selectionset.rename(id, name)` and `selectionset.list() -> [{id, name, count}]` *(query)*

**Acceptance criteria.**
- [ ] A saved set of 12 elements, one of which is later deleted, loads 11 elements without error.
- [ ] Saving a set with the name of an existing view filter offers Replace or Cancel; Cancel changes nothing.
- [ ] In Edit Selection mode, window-selecting three walls with *Remove from Selection* active and pressing Finish leaves the set three members smaller, as one undo step.
- [ ] Exporting with *Selection sets as IfcGroup* gives one `IfcGroup` per set, whose `IfcRelAssignsToGroup` lists exactly the member elements.

---

## 3.15 Regeneration dependencies and ordering

### 3.15.1 Regeneration graph for architectural elements
**ID:** ARC-REGEN-001 · **Priority:** P0 · **Ref:** research/02 §17 (all items) · TECH_STACK.md §3.3

**Purpose.** One deterministic, parallel recompute order for every element in this chapter.

**Behaviour & rules.**
- **Two stages per element.** Every curve-driven or sketch-driven element has a **plan stage** (location curves, sketch loops, datum references and resolved parameters) and a **body stage** (the 3D solid). Other elements may depend on either stage. Splitting the stages removes the cycles that would otherwise occur. Examples: a roof footprint locked to a wall reads only the wall's plan stage, while the wall attached to that roof reads the roof's body stage. A hosted wall reads its host's plan stage.
- **Phases within a transaction:**

| Phase | Work | Parallelism |
|---|---|---|
| R0 Parameters | Apply the command's parameter changes and run formulas (DM chapter) | Serial |
| R1 Constraints | Solve the affected constraint components (ARC-CNST-002) | Parallel over independent components |
| R2 Datums | Levels, grids, reference planes, scope boxes, work planes | Parallel |
| R3 Plan stage | Wall location curves (including hosted walls), sketch loops (resolving pick-locks), curtain grid layouts, stair recipes (riser counts, run paths), railing paths, group instance transforms and member templates | Parallel, in topological order |
| R4 Join resolution | Detect wall joins and paint them (ARC-JOIN-001), per node | Parallel over nodes that share no wall |
| R5 Bodies (unclipped) | Wall layer solids with join and profile applied; floors, roofs (skeleton), ceilings, stairs, ramps, railings, toposolids (TIN), families (mapped geometry), masses | Parallel over elements; each worker builds its own OCCT shapes (no shared `TopoDS` across threads) |
| R6 Interactions | Attach clipping (target bodies → walls and columns), Join Geometry and Cut Geometry, embedded curtain-wall cuts, insert voids and wrapping, shafts and openings, excavation, Join Roof | Parallel over connected components of the `attach` and `cut` graph; serial inside a component, in topological order |
| R7 Derived | Room and area boundaries, volumes, door From/To rooms, quantities (`Qto_*`), shape-edit cut/fill, contours (lazy) | Parallel per (level, phase) |
| R8 Commit | Swap the new geometry and quantities in atomically, update spatial indices, mark the views dirty (DOC chapter) | Serial |

- **Edge summary** (this chapter):

| From (producer) | To (consumer) | Kind | Stage read |
|---|---|---|---|
| Level | Every element constrained to the level; plan views | `datum` | — |
| Grid | Columns with *Moves With Grids*; locked references | `datum` | — |
| Reference plane, face | Work-plane-based elements; locked sketch lines | `datum`, `sketch-lock` | plan or body |
| Wall | Join node | `join` | plan |
| Join node | Each wall in the node | `join` | — |
| Wall (plan) | Roof, floor or ceiling sketch locked to it; hosted wall; By-Room linings (creation only) | `sketch-lock`, `host` | plan |
| Floor, roof, ceiling, reference plane, wall | Attached walls and columns | `attach` | body |
| Insert (void only) | Host wall, floor or roof body | `cut` | — |
| Host | Insert placement | `host` | plan (walls), body (faces) |
| Cutter (Cut Geometry, shaft, excavation, Join Geometry) | Cut element | `cut` | body |
| Wall type, floor type and other types | Instances | `type` | — |
| Room-bounding element | Rooms and areas in its level band | `bound` | body (uncut) |
| Toposolid | Hosted site components, sub-divisions | `host` | body |
| Group type | Group instances; array sets | `group` | — |
| Constraint | The elements it references | `constraint` | — |
| Mass | Elements by face (only on *Update to Face*) | (explicit) | body |

- **Cycle handling.** Adding an edge that would close a cycle at the same stage is rejected, with the message "Circular reference: A → B → … → A", and the command that would create it fails. Examples: aligning a host to its own hosted wall; attaching wall A's top to wall B while B's base is attached to A at the same location.
- **Determinism.** Inside each phase, the results do not depend on thread scheduling. Elements are processed in topological order, ties are broken by internal ID, and the floating-point Boolean operations run with fixed fuzzy values.
- **Failure isolation.** If one element's geometry fails (an OCCT exception or an invalid solid detected by `BRepCheck_Analyzer`), that element keeps its last valid body, is flagged with a warning, and its dependants regenerate against the old body. The transaction still commits unless the failure came from the command's own target element *and* the command was interactive. In that case the user chooses whether to commit or roll back.

**Acceptance criteria.**
- [ ] Moving one wall in a 500-wall model regenerates only that wall, its join nodes' walls, its inserts, the attached elements and the rooms on its level. This is verified with the instrumentation counter.
- [ ] Regenerating a model twice from the same IFC and RXD data produces byte-identical geometry hashes.
- [ ] Creating a two-element attach cycle is refused with the circular-reference message, and the model is unchanged.

---

## 3.16 Default keyboard shortcuts (this chapter)

**ID:** ARC-KEY-002 · **Priority:** P0 · **Ref:** research/02 §16

Two-letter sequences are typed without a modifier. The user can rebind every shortcut. Bindings are stored in Reviera's own keymap format (UI chapter); no reference-product keymap file is read or written.

| Command | Key | Command | Key |
|---|---|---|---|
| Wall | WA | Align | AL |
| Door | DR | Offset | OF |
| Window | WN | Mirror – Pick Axis | MM |
| Component | CM | Mirror – Draw Axis | DM |
| Column (structural, STR) | CL | Move | MV |
| Level | LL | Copy | CO / CC |
| Grid | GR | Rotate / new centre | RO / R3 |
| Reference Plane | RP | Trim/Extend to Corner | TR |
| Room | RM | Split Element | SL |
| Tag Room | RT | Array | AR |
| Floor: Structural | SB | Scale | RE |
| Model Line | LI | Pin / Unpin | PN / UP |
| Create Group | GP | Delete | DE |
| Edit / Finish Group | EG / FG | Match Type Properties | MA |
| Add to / Remove from Group | AP / RG | Paint | PT |
| Attach Detail Group | AD | Split Face | SF |
| Ungroup | UG | Linework | LW |
| Create Similar | CS | Select All Instances | SA |
| Repeat last command | RC / Enter | Restore excluded / all | RB / RA |
| Snap overrides | SE SM SN SI SC SP ST SQ SX SW SR PC SO SS | Hide element / category (DOC) | EH / VH |

**Acceptance criteria.**
- [ ] Typing `W` then `A` in a plan view starts the wall tool. Rebinding the Wall tool to `WW` takes effect at once and persists across sessions.

---

## Command catalog

All commands below are registered once in the command registry and exposed automatically to the UI, to Lua (`reviera.<domain>.<verb>`) and to MCP (one tool per command, JSON Schema generated from the definition). Parameter types follow §3.0.2. *Session* commands (sketch, stair assembly, group editor, in-place and mass editors) record into a transaction group that becomes one undo step on finish.

| Command | Parameters | Returns | Undoable | Req ID |
|---|---|---|---|---|
| `tool.start` | command_id, type_id?, preset_args? | session_id | No (the tool's submissions are) | ARC-TOOL-001 |
| `tool.cancel` | — | () | No (the tool's submissions are; a session collapses to one undo step) | ARC-TOOL-001 |
| `tool.finish` | — | () | No (the tool's submissions are; a session collapses to one undo step) | ARC-TOOL-001 |
| `tool.set_option` | tool_id? (default: active tool), key, value | () | No (session state) | ARC-TOOL-001 |
| `tool.repeat_last` | — | session_id | No (session state) | ARC-TOOL-001, ARC-MOD-027 |
| `element.set_type` | element_ids, type_id | — | Yes | ARC-TOOL-002, ARC-CW-008 |
| `type.duplicate` | type_id, new_name | type_id | Yes | ARC-TOOL-002 |
| `type.rename` | type_id, name | () | Yes | ARC-TOOL-002 |
| `type.set_param` | type_id, param, value | () | Yes | ARC-TOOL-002 |
| `element.set_param` | element_ids, name, value | — | Yes | ARC-TOOL-002 |
| `sketch.begin` | host_id, role? | session_id | Within session; one step on finish | ARC-SKCH-001 |
| `sketch.add_curve` | session_id, curve, role?, props? | curve_key | Within session; one step on finish | ARC-SKCH-001 |
| `sketch.pick` | session_id, ref, offset?, lock?, chain? | [curve_key] | Within session; one step on finish | ARC-SKCH-001 |
| `sketch.edit_curve` | session_id, curve_key, curve?, props? | — | Within session; one step on finish | ARC-SKCH-001 |
| `sketch.delete_curve` | session_id, curve_key | — | Within session; one step on finish | ARC-SKCH-001 |
| `sketch.finish` | session_id | element_id | Yes (one step) | ARC-SKCH-001 |
| `sketch.cancel` | session_id | — | Discards session | ARC-SKCH-001 |
| `dim.temporary_query` | element_id, view_id | [TempDim] | No (query) | ARC-DIM-001 |
| `element.move` | element_ids, vector, disjoin?: bool | [element_id] | Yes | ARC-DIM-001, ARC-MOD-004, ARC-MOD-000 |
| `wall.set_location` | wall_id, curve | — | Yes | ARC-DIM-001, ARC-WALL-002 |
| `wall.set_constraints` | wall_id, base_level?, base_offset?, top_constraint?, top_offset? | — | Yes | ARC-WALL-001 |
| `dim.make_permanent` | view_id, temporary_dimension_ref \| {ref_a, ref_b} | dimension_id | Yes | ARC-DIM-001 |
| `snap.set_options` | kinds, increments? | — | No (session state) | ARC-SNAP-001 |
| `level.create` | elevation, name?, type_id?, make_plan_views?: bool, plan_view_types?: [view_type_id], extent?: Line | element_id | Yes | ARC-LVL-001 |
| `level.set_elevation` | level_id, elevation | — | Yes | ARC-LVL-001 |
| `level.rename` | level_id, name, rename_views?: bool | — | Yes | ARC-LVL-001 |
| `level.set_story` | level_id, is_building_story | — | Yes | ARC-LVL-001 |
| `level.create_plan_views` | level_ids, view_type_ids | [view_id] | Yes | ARC-LVL-001 |
| `grid.create` | curve \| segments: [curve], name?, type_id?, scope_box_id? | element_id | Yes | ARC-GRID-001 |
| `grid.rename` | grid_id, name | — | Yes | ARC-GRID-001 |
| `grid.set_curve` | grid_id, curve | — | Yes | ARC-GRID-001 |
| `refplane.create` | p0, p1, view_normal \| normal, name?, subcategory? | element_id | Yes | ARC-REFP-001 |
| `refplane.rename` | id, name | — | Yes | ARC-REFP-001 |
| `datum.set_extent` | datum_id, view_id?, end, point | — | Yes | ARC-DATUM-001 |
| `datum.reset_to_3d` | datum_ids, view_id | — | Yes | ARC-DATUM-001 |
| `datum.maximize_3d` | datum_ids | — | Yes | ARC-DATUM-001 |
| `datum.propagate_extents` | datum_ids, source_view_id, target_view_ids | — | Yes | ARC-DATUM-001 |
| `datum.set_bubble` | datum_id, view_id, end, visible | — | Yes | ARC-DATUM-001 |
| `datum.add_elbow` | datum_id, view_id, end | — | Yes | ARC-DATUM-001 |
| `scopebox.create` | name, rectangle: {corner_a, corner_b, rotation?}, height, level_id? | element_id | Yes | ARC-SCOPE-001 |
| `scopebox.set_extents` | id, min, max, rotation? | — | Yes | ARC-SCOPE-001 |
| `datum.assign_scope_box` | datum_ids, scope_box_id \| null | — | Yes | ARC-SCOPE-001 |
| `view.assign_scope_box` | view_ids, scope_box_id \| null | — | Yes | ARC-SCOPE-001 |
| `workplane.set` | view_id, by: Name(datum_id) \| Face(host_ref) \| Line(element_id) \| Plane(origin, normal, xdir) | plane | Yes | ARC-WP-001 |
| `workplane.show` | view_id, on | — | No (session state) | ARC-WP-001 |
| `workplane.viewer` | view_id? (default: the active view) | view_id | No (session state) | ARC-WP-001 |
| `element.set_work_plane` | element_ids, plane \| host_ref | — | Yes | ARC-WP-001 |
| `element.dissociate_work_plane` | element_ids | — | Yes | ARC-WP-001 |
| `wall.create` | type_id, curve, base_level, top_constraint: Level(id) \| Unconnected(height), base_offset?, top_offset?, location_line?, flip?: bool, structural?: bool, structural_usage?, room_bounding?: bool, join_status?: Allow\|Disallow, auto_join?: Off\|Join\|JoinAndLock | element_id | Yes | ARC-WALL-002, ARC-WALL-015, ARC-CW-002 |
| `wall.create_chain` | type_id, curves: [curve], closed?: bool, fillet_radius?, ...same as wall.create | [element_id] | Yes | ARC-WALL-002 |
| `wall.create_from_faces` | type_id, face_refs, base_level, location_line, ... | [element_id] | Yes | ARC-WALL-002 |
| `wall.set_location_line` | wall_ids, location_line | — | Yes | ARC-WALL-002 |
| `wall.flip` | wall_ids | — | Yes | ARC-WALL-002, ARC-WALL-017 |
| `compound.set_layers` | type_id, layers: [{function, priority?, material_id, thickness, wraps?, structural_material?, variable?}], core_start, core_end | — | Yes | ARC-CMPD-001 |
| `compound.insert_layer` | type_id, index, layer | — | Yes | ARC-CMPD-001 |
| `compound.delete_layer` | type_id, index | — | Yes | ARC-CMPD-001 |
| `compound.move_layer` | type_id, index, delta | — | Yes | ARC-CMPD-001 |
| `compound.duplicate_layer` | type_id, index | — | Yes | ARC-CMPD-001 |
| `compound.set_default_wrapping` | type_id, at_inserts, at_ends | — | Yes | ARC-CMPD-001 |
| `parts.create` | host_ids | [element_id] | Yes | ARC-PART-001 |
| `parts.divide` | part_ids, curves?: [curve], work_plane?: plane \| host_ref, refs?: [{ref, offset}], gap?: f64, profile?: {type_id, edge_match: Complementary\|Mirrored\|Rotated, flip_along?: bool, flip_across?: bool, offset?: f64} | {division_id, parts: [element_id]} | Yes | ARC-PART-002 |
| `parts.division_begin` | part_ids \| division_id | session_id | Within session; one step on finish | ARC-PART-002 |
| `parts.division_set` | session_id, refs?, gap?, profile? | — | Within session; one step on finish | ARC-PART-002 |
| `parts.division_add` | session_id, part_ids | — | Within session; one step on finish | ARC-PART-002 |
| `parts.division_remove` | session_id, part_ids | — | Within session; one step on finish | ARC-PART-002 |
| `parts.division_finish` | session_id | {division_id, parts: [element_id]} | Yes (one step) | ARC-PART-002 |
| `parts.division_cancel` | session_id | — | Discards session | ARC-PART-002 |
| `parts.merge` | part_ids | element_id | Yes | ARC-PART-003 |
| `parts.edit_merged` | merged_part_id, add?: [element_id], remove?: [element_id] | — | Yes | ARC-PART-003 |
| `parts.exclude` | part_ids | — | Yes | ARC-PART-003 |
| `parts.restore` | part_ids | — | Yes | ARC-PART-003 |
| `parts.set_overrides` | part_ids, material?: ByOriginal \| material_id, base_level?: ByOriginal \| level_id \| NonAssociated, phase_created?: ByOriginal \| phase_id, phase_demolished?: ByOriginal \| phase_id \| None | — | Yes | ARC-PART-004 |
| `parts.offset_face` | part_id, face_ref, distance | — | Yes | ARC-PART-004 |
| `view.set_parts_visibility` | view_id, mode: ShowParts\|ShowOriginal\|ShowBoth | () | Yes | ARC-PART-004 |
| `compound.split_region` | type_id, region_key, orientation: Horizontal\|Vertical, position | — | Yes | ARC-CMPD-003 |
| `compound.merge_regions` | type_id, region_a, region_b, keep: A\|B | — | Yes | ARC-CMPD-003 |
| `compound.assign_layer` | type_id, region_keys, layer_index | — | Yes | ARC-CMPD-003 |
| `compound.set_region_anchor` | type_id, boundary_key, from: Base\|Top, distance | — | Yes | ARC-CMPD-003 |
| `wall.set_extension` | wall_ids, top?: f64, base?: f64 | — | Yes | ARC-CMPD-004 |
| `compound.set_extendable` | type_id, index, top?: bool, base?: bool | — | Yes | ARC-CMPD-004 |
| `wall.set_end_wrap` | wall_id, end: Start\|End, mode: ByType\|Exterior\|Interior\|None | — | Yes | ARC-WRAP-001 |
| `wall.set_cross_section` | wall_ids, mode: Vertical\|Slanted\|Tapered, angle?: f64, exterior_angle?: f64, interior_angle?: f64, override_angles?: bool | — | Yes | ARC-WALL-005 |
| `stackedwall.set_rows` | type_id, offset_reference, rows: [{wall_type_id, height \| Variable, offset, flip, top_ext?, base_ext?}] | — | Yes | ARC-STKW-001 |
| `wall.break_up` | stacked_wall_ids | [element_id] | Yes | ARC-STKW-001 |
| `wall.join_set_type` | node_ref \| wall_end_refs, type: Butt\|Miter\|SquareOff | — | Yes | ARC-JOIN-001, ARC-MOD-018 |
| `wall.join_cycle_order` | node_ref, step: +1\|-1 | — | Yes | ARC-JOIN-001, ARC-MOD-018 |
| `wall.join_set_display` | node_ref, display: ViewSetting\|Clean\|DontClean | — | Yes | ARC-JOIN-001, ARC-MOD-018 |
| `wall.set_join_allowed` | wall_id, end: Start\|End, allowed: bool | — | Yes | ARC-JOIN-001, ARC-MOD-018 |
| `wall.joins_query` | wall_id | [JoinNode] | No (query) | ARC-JOIN-001 |
| `wall.attach` | wall_ids, end: Top\|Base, target_id | — | Yes | ARC-WALL-008 |
| `wall.detach` | wall_ids, end: Top\|Base, target_ids? | — | Yes | ARC-WALL-008 |
| `wall.edit_profile` | wall_id, loops | — | Yes | ARC-WALL-009, ARC-CW-002 |
| `wall.reset_profile` | wall_ids | — | Yes | ARC-WALL-009 |
| `wallsweep.create` | type_id, wall_ids, orientation: Horizontal\|Vertical, offset_from_level \| distance_along, side? | element_id | Yes | ARC-WALL-010 |
| `wallreveal.create` | type_id, wall_ids, orientation: Horizontal\|Vertical, offset_from_level \| distance_along, side? | element_id | Yes | ARC-WALL-010 |
| `wallsweep.add_walls` | sweep_id, wall_ids | — | Yes | ARC-WALL-010 |
| `wallsweep.remove_walls` | sweep_id, wall_ids | — | Yes | ARC-WALL-010 |
| `wallsweep.set_returns` | sweep_id, end, mode: Straight\|Angled | — | Yes | ARC-WALL-010 |
| `walltype.set_sweeps` | type_id, rows | — | Yes | ARC-WALL-010 |
| `walltype.set_reveals` | type_id, rows | — | Yes | ARC-WALL-010 |
| `element.split` | element_id, point, delete_inner?: point2? | [element_id] | Yes | ARC-WALL-011, ARC-MOD-010 |
| `wall.split_horizontal` | wall_id, z \| level_id | [element_id] | Yes | ARC-WALL-011 |
| `element.split_with_gap` | element_id, point, gap | [element_id] | Yes | ARC-WALL-011, ARC-MOD-011 |
| `geometry.cut` | target_id, cutter_ids | — | Yes | ARC-WALL-013, ARC-MOD-016 |
| `wall.create_by_segment` | type_id, target_ref, side: Left\|Right, include_columns?: bool, hosted?: bool, base_level?, top_constraint?, ... | [element_id] | Yes | ARC-WALL-014 |
| `wall.create_by_room` | type_id, room_id \| point, flip?: bool, hosted?: bool, ... | [element_id] | Yes | ARC-WALL-014 |
| `geometry.join` | a_id, b_ids | — | Yes | ARC-WALL-015, ARC-MOD-017 |
| `constraint.lock_alignment` | ref_a, ref_b | constraint_id | Yes | ARC-WALL-015, ARC-CNST-002 |
| `wall.set_host` | wall_id, host_wall_id, side, offset | — | Yes | ARC-WALL-016 |
| `wall.remove_host` | wall_ids | — | Yes | ARC-WALL-016 |
| `wall.select_main_host` | wall_id | element_id | No (query) | ARC-WALL-016 |
| `element.pick_new_host` | element_ids, host_ref, placement?: VerticalFace\|Face\|WorkPlane | — | Yes | ARC-HOST-001 |
| `element.host_query` | element_id | host_ref | No (query) | ARC-HOST-001 |
| `door.place` | type_id, host_wall_id, point_along: f64 \| point, sill_height?, flip_hand?: bool, flip_facing?: bool, tag?: {orientation, leader, leader_length} | element_id | Yes | ARC-DOOR-001 |
| `insert.flip_hand` | element_ids | — | Yes | ARC-DOOR-001, ARC-FLIP-001 |
| `insert.flip_facing` | element_ids | — | Yes | ARC-DOOR-001, ARC-FLIP-001 |
| `element.flip` | element_ids, control?: Facing\|Hand\|WorkPlane\|Family(control_key) | () | Yes | ARC-FLIP-001 |
| `element.flip_state` | element_id | {facing_flipped, hand_flipped, mirrored, controls: [control_key]} | No (query) | ARC-FLIP-001 |
| `window.place` | type_id, host_id, point_along \| point, sill_height?, flip_facing?, width?, height?, fit? | element_id | Yes | ARC-WIN-001 |
| `insert.set_size` | element_ids, width?, height?, fit? | — | Yes | ARC-WIN-001, ARC-DOOR-001 |
| `insert.set_position` | element_id, along \| from_end | — | Yes | ARC-WIN-001, ARC-DOOR-001 |
| `component.place` | type_id, point \| host_ref, rotation?: f64, level_id?, offset?, placement?: VerticalFace\|Face\|WorkPlane | element_id | Yes | ARC-COMP-001 |
| `column.place_architectural` | type_id, point, base_level, top_constraint, base_offset?, top_offset?, rotation?, room_bounding? | element_id | Yes | ARC-COL-001 |
| `column.attach` | column_ids[], end: top\|base, target_id, style?, justification?, offset? | () | Yes | ARC-COL-001 |
| `column.detach` | column_ids[], end: top\|base, target_ids? | () | Yes | ARC-COL-001 |
| `curtain.set_profile` | wall_id, loops | — | Yes | ARC-CW-002 |
| `curtain.set_layout` | element_id, direction: U\|V, justification?, angle?, offset?, number? | — | Yes | ARC-CW-003 |
| `curtaintype.set_layout` | type_id, direction, layout, spacing?, adjust_for_mullion?: bool | — | Yes | ARC-CW-003 |
| `curtain.add_grid` | element_id, direction: U\|V, position, mode: All\|One(segment)\|AllExcept([segment]) | grid_key | Yes | ARC-CW-004 |
| `curtain.set_grid_segments` | element_id, grid_key, segments_on: [bool] | — | Yes | ARC-CW-004 |
| `curtain.move_grid` | element_id, grid_key, position | — | Yes | ARC-CW-004 |
| `curtain.delete_grid` | element_id, grid_key | — | Yes | ARC-CW-004 |
| `mullion.add` | element_id, target: GridLine(key) \| Segment(key, idx) \| AllGridLines \| Border(edge), type_id | [element_id] | Yes | ARC-CW-005 |
| `mullion.set_continuity` | mullion_id, mode: Continuous\|Break | — | Yes | ARC-CW-005 |
| `curtaintype.set_mullions` | type_id, interior_v, border1_v, border2_v, interior_h, border1_h, border2_h | — | Yes | ARC-CW-005 |
| `curtain.set_panel_type` | panel_ids, type_id | — | Yes | ARC-CW-006 |
| `curtainsystem.create_by_face` | type_id, face_refs | element_id | Yes | ARC-CW-007 |
| `floor.create` | type_id, loops, level_id, offset?, slope?: {line_key\|curve, slope} \| {arrow: SlopeArrow}, structural?, span_direction_edge?, room_bounding?, attach_walls_below?: bool, join_overlapping_walls?: bool | element_id | Yes | ARC-FLR-001 |
| `floor.edit_boundary` | floor_id, loops, slope? | — | Yes | ARC-FLR-001 |
| `floor.set_slope_angle` | floor_id, angle | — | Yes | ARC-FLR-001 |
| `slabedge.create` | type_id, edge_refs: [ref], ...offsets | element_id | Yes | ARC-SLED-001 |
| `hostsweep.add_segments` | id, edge_refs | — | Yes | ARC-SLED-001 |
| `hostsweep.remove_segments` | id, edge_refs | — | Yes | ARC-SLED-001 |
| `hostsweep.flip` | id, axis: Horizontal\|Vertical | — | Yes | ARC-SLED-001 |
| `ceiling.create_auto` | type_id, level_id, point, offset? | element_id | Yes | ARC-CEIL-001 |
| `ceiling.create` | type_id, loops, level_id, offset?, slope? | element_id | Yes | ARC-CEIL-001 |
| `ceiling.edit_boundary` | ceiling_id, loops, slope? | — | Yes | ARC-CEIL-001 |
| `surfacepattern.align` | element_id, face_ref, origin?, angle? | — | Yes | ARC-CEIL-001 |
| `roof.create_footprint` | type_id, loop: [{curve, defines_slope, slope?, overhang?, plate_offset?, offset_from_base?, extend_to_core?, lock_ref?}], holes?, base_level, base_offset?, cutoff?: {level_id, offset}, rafter_cut?, fascia_depth?, rafter_or_truss?, slope_arrows?: [SlopeArrow], attach_walls?: bool | element_id | Yes | ARC-ROOF-001 |
| `roof.edit_footprint` | roof_id, ... | — | Yes | ARC-ROOF-001 |
| `roof.align_eaves` | roof_id, edge_keys, mode: AdjustHeight\|AdjustOverhang | — | Yes | ARC-ROOF-001 |
| `roof.set_shape` | roof_id, shape: hip\|gable\|gable_rotated\|shed\|shed_reversed\|flat, overhang? | — | Yes | ARC-ROOF-001 |
| `roof.attach_walls` | roof_id | wall_ids[] | Yes | ARC-ROOF-001 |
| `sketch.move_curve` | element_id, key, vector | — | Yes | ARC-SKCH-001 |
| `sketch.move_corner` | element_id, from, to | — | Yes | ARC-SKCH-001 |
| `dormer.create` | roof_id, point, style: gable\|hip\|shed\|flat, width, front_height, slope?, overhang?, setback?, wall_type_id? | element_id | Yes | ARC-ROOF-007 |
| `dormer.set` | dormer_id, style?, width?, front_height?, slope?, overhang?, setback? | — | Yes | ARC-ROOF-007 |
| `dormer.move` | dormer_id, point | — | Yes | ARC-ROOF-007 |
| `roof.create_arched` | footprint \| curves, spring_edges, level_id, base_offset?, rise?, radius?, profile?, taper?, overhang?, end_overhang?, type_id? | element_id | Yes | ARC-ROOF-008 |
| `roof.set_arch` | roof_id, rise?, radius?, profile?, taper? | — | Yes | ARC-ROOF-008 |
| `roof.create_dome` | plan, level_id, base_offset?, rise?, profile?, oculus?, polygon_mode?, type_id? | element_id | Yes | ARC-ROOF-009 |
| `roof.set_dome` | roof_id, rise?, profile?, oculus?, polygon_mode? | — | Yes | ARC-ROOF-009 |
| `geometry.boolean` | target_id, tool_ids, op: intersect\|subtract\|union, tools?: keep\|hide\|consume | — | Yes | ARC-MOD-028 |
| `geometry.remove_boolean` | target_id, tool_ids? | — | Yes | ARC-MOD-028 |
| `overlap.find` | element_ids?, against?, tolerance?, min_volume?, resolution?, include_related?, limit? | {overlaps, bodies} | No | ARC-MOD-030 |
| `overlap.resolve` | a, b, mode: cut_a\|cut_b\|join\|subtract\|intersect\|union\|move_apart\|accept | — | Yes | ARC-MOD-032 |
| `overlap.accept` | a, b, note?, accepted? | — | Yes | ARC-MOD-032 |
| `overlap.accepted` | — | [pair] | No | ARC-MOD-032 |
| `clearance.list` | element_ids? | [clearance] | No | ARC-MOD-031 |
| `clearance.check` | element_ids?, rules? | [overlap] | No | ARC-MOD-031 |
| `clearance.set_rule_set` | standard: none\|ada_2010 | — | Yes | ARC-MOD-031 |
| `view.set_virtual_volumes` | view_id, mode: hidden\|translucent\|solid | — | Yes | ARC-MOD-031 |
| `railing.create_on_edges` | type_id, host_id, edges?: [curve_key]\|all_open, setback? | [element_id] | Yes | ARC-RAIL-001 |
| `railing.create_on_stair` | stair_id, sides?: [left\|right], inset?, type_id? | [element_id] | Yes | ARC-RAIL-001 |
| `deck.create` | outline: loops, level_id, height?, type_id?, start_edge?, direction? | element_id | Yes | ARC-DECK-001 |
| `deck.set_boards` | deck_id, width?, thickness?, gap?, direction?, stock_lengths?, picture_frame?, breakers?, stagger?, balance_courses?, seed? | — | Yes | ARC-DECK-001 |
| `deck.set_framing` | deck_id, joist_spacing?, joist_size?, beam_spacing?, post_spacing?, ledger_wall_id? | — | Yes | ARC-DECK-001 |
| `deck.boards` | deck_id | [board] | No | ARC-DECK-001 |
| `deck.cut_list` | deck_id | cut_list | No | ARC-DECK-001 |
| `roof.set_eaves` | roof_id, edges?, bearing?, heel?, overhang?, cut?, fascia_depth?, fascia?, soffit?, attach_walls?, remove? | — | Yes | ARC-ROOF-010 |
| `roof.eaves` | roof_id | [eave] | No | ARC-ROOF-010 |
| `railing.add_opening` | railing_id, at?\|point?, width? | key | Yes | ARC-RAIL-003 |
| `railing.add_gate` | railing_id, at?\|point?, leaves?: 1\|2, width?, swing_out?, hinge_end? | element_id | Yes | ARC-RAIL-003 |
| `railing.move_feature` | railing_id, key, at?\|point?, width? | — | Yes | ARC-RAIL-003 |
| `railing.remove_feature` | railing_id, key | — | Yes | ARC-RAIL-003 |
| `railing.features` | railing_id | [feature] | No | ARC-RAIL-003 |
| `deck.override_board` | deck_id, board_key, length?, joint?, removed? | — | Yes | ARC-DECK-001 |
| `roof.set_slope` | roof_id, slope, edge_keys? | — | Yes | ARC-ROOF-001 |
| `roof.create_extrusion` | type_id, work_plane, profile: [curve], ref_level, level_offset?, start, end | element_id | Yes | ARC-ROOF-002 |
| `roof.create_by_face` | type_id, face_refs, location: Top\|Bottom, level_id | element_id | Yes | ARC-ROOF-003 |
| `element.edit_face_selection` | element_id, add?: [face_ref], remove?: [face_ref] | — | Yes | ARC-ROOF-003, ARC-CW-007 |
| `fascia.create` | type_id, edge_refs, mitering? | element_id | Yes | ARC-ROOF-004 |
| `gutter.create` | type_id, edge_refs | element_id | Yes | ARC-ROOF-004 |
| `soffit.create` | type_id, loops, level_id, offset?, slope? | element_id | Yes | ARC-ROOF-004 |
| `hostsweep.set_mitering` | id, mode | — | Yes | ARC-ROOF-004 |
| `roof.join` | roof_id, edge_ref, target_ref | — | Yes | ARC-ROOF-006 |
| `roof.unjoin` | roof_id, edge_ref? | — | Yes | ARC-ROOF-006 |
| `shape.add_point` | element_id, xy, mode: AlongSurface(offset)\|Absolute(z), elevation_base? | key | Yes | ARC-SHAPE-001 |
| `shape.add_split_line` | element_id, points: [xy], mode, heights? | key | Yes | ARC-SHAPE-001 |
| `shape.move` | element_id, keys, dz?, dxy? | — | Yes | ARC-SHAPE-001 |
| `shape.set_elevation` | element_id, keys, z, elevation_base? | — | Yes | ARC-SHAPE-001 |
| `shape.pick_supports` | element_id, beam_ids | — | Yes | ARC-SHAPE-001 |
| `shape.convert_folding_lines` | element_id | — | Yes | ARC-SHAPE-001 |
| `shape.reset` | element_id | — | Yes | ARC-SHAPE-001 |
| `shape.copy` | element_id, keys | clip | No (query) | ARC-SHAPE-001 |
| `shape.paste` | element_id, clip, offset? | — | Yes | ARC-SHAPE-001 |
| `stair.create` | type_id, base_level, top_level \| Unconnected(height), base_offset?, top_offset?, desired_risers?, tread_depth?, runs: [RunSpec], landings?: [LandingSpec], supports?: [SupportSpec], railing?: {type_id \| None, position: Treads\|Stringer} | element_id | Yes | ARC-STAIR-002 |
| `stair.edit` | stair_id | session_id | Within session; one step on finish | ARC-STAIR-002 |
| `stair.add_run` | session_id, RunSpec | key | Within session; one step on finish | ARC-STAIR-002 |
| `stair.add_landing` | session_id, LandingSpec | key | Within session; one step on finish | ARC-STAIR-002 |
| `stair.add_support` | session_id, edge_refs, side | key | Within session; one step on finish | ARC-STAIR-002 |
| `stair.set_run_risers` | session_id, run_key, n | — | Within session; one step on finish | ARC-STAIR-002 |
| `stair.flip` | session_id | — | Within session; one step on finish | ARC-STAIR-002 |
| `stair.convert_to_sketch` | session_id, component_key | — | Within session; one step on finish | ARC-STAIR-002 |
| `stair.finish` | session_id | — | Yes (one step) | ARC-STAIR-002 |
| `stair.cancel` | session_id | — | Discards session | ARC-STAIR-002 |
| `stair.set_rules` | type_id, max_riser, min_tread, min_width, calculator?: {m,n,min,max} | — | Yes | ARC-STAIR-003 |
| `stair.connect_levels` | stair_id, level_ids, align_with: MidLanding\|RunStart | — | Yes | ARC-STAIR-003 |
| `stair.number_treads` | view_id, stair_id, start_number?, type_id? | element_id | Yes | ARC-STAIR-006 |
| `stair.headroom_check` | stair_id, code?, headroom? | {headroom, clause, obstructions} | No | ARC-STAIR-007 |
| `stair.create_openings` | stair_id, code?, headroom? | element_ids | Yes | ARC-STAIR-007 |
| `railing.create` | type_id, path: [curve], host_ref?, base_level?, base_offset?, offset_from_path?, segment_slopes?: [ByHost\|Flat\|Sloped], height_corrections?: [f64\|ByType] | element_id | Yes | ARC-RAIL-001 |
| `railing.place_on_host` | type_id, host_id, position: Treads\|Stringer | [element_id] | Yes | ARC-RAIL-001 |
| `railing.edit_path` | railing_id, path, ... | — | Yes | ARC-RAIL-001 |
| `railing.flip_path` | railing_id | — | Yes | ARC-RAIL-001 |
| `railing.set_rail_join` | railing_id, join_ref, mode: ByType\|Fillet\|Miter\|Flush | — | Yes | ARC-RAIL-001 |
| `railingtype.set_rails` | type_id, rows | — | Yes | ARC-RAIL-001 |
| `railingtype.set_continuous_rail` | type_id, which: Top\|Handrail1\|Handrail2, props | — | Yes | ARC-RAIL-001 |
| `railingtype.set_baluster_pattern` | type_id, rows, break_at, angle?, justify, excess_fill, excess_family?, excess_spacing? | — | Yes | ARC-RAIL-002 |
| `railingtype.set_posts` | type_id, start, corner, end, corner_at, angle? | — | Yes | ARC-RAIL-002 |
| `railingtype.set_per_tread` | type_id, enabled, count, family_id | — | Yes | ARC-RAIL-002 |
| `ramp.create` | type_id, base_level, top_level \| Unconnected(h), base_offset?, top_offset?, runs?: [curve], boundary?: [[curve],[curve]], risers?: [curve], width?, railing? | element_id | Yes | ARC-RAMP-001 |
| `ramp.edit_sketch` | ramp_id, ... | — | Yes | ARC-RAMP-001 |
| `opening.create_by_face` | host_ref, loops | element_id | Yes | ARC-OPEN-001 |
| `opening.create_vertical` | host_id, loops | element_id | Yes | ARC-OPEN-002 |
| `opening.set_loops` | opening_id, loops | — | Yes | ARC-OPEN-002, ARC-STAIR-007 |
| `opening.create_wall` | wall_id, u0, u1, base: {level_id, offset}, top: Level{id,offset} \| Unconnected(h) | element_id | Yes | ARC-OPEN-003 |
| `opening.create_shaft` | loops, base_level, base_offset?, top: Level{id,offset} \| Unconnected(h), symbolic_lines?: [curve] | element_id | Yes | ARC-OPEN-004 |
| `opening.create_dormer` | roof_id, refs: [ref] | element_id | Yes | ARC-OPEN-005 |
| `room.place` | level_id, point, upper_limit?: level_id, limit_offset?, base_offset?, name?, number?, phase_id?, room_id?: existing_unplaced, tag?: {type_id, orientation, leader} | element_id | Yes | ARC-ROOM-001 |
| `room.place_automatically` | level_id, phase_id? | [element_id] | Yes | ARC-ROOM-001 |
| `room.unplace` | room_ids | — | Yes | ARC-ROOM-001 |
| `room.delete` | room_ids | — | Yes | ARC-ROOM-001 |
| `roomseparator.create` | level_id, curves: [curve] | [element_id] | Yes | ARC-ROOM-001 |
| `room.recenter_reference` | room_ids | — | Yes | ARC-ROOM-001 |
| `roomtag.move_to_reference` | tag_ids | — | Yes | ARC-ROOM-001 |
| `room.highlight_boundaries` | view_id | [element_id] | No (query) | ARC-ROOM-001 |
| `room.computation_settings` | — | () | No (opens dialog) | ARC-ROOM-002 |
| `room.set_computation_settings` | volumes?: AreasOnly\|AreasAndVolumes, area_rule?: Finish\|Center\|Core\|CoreCenter | () | Yes | ARC-ROOM-002 |
| `areascheme.create` | name, description? | id | Yes | ARC-AREA-001 |
| `areascheme.set_rules` | scheme_id, rules: [{type_a, type_b, position}] | — | Yes | ARC-AREA-001 |
| `areaplan.create` | scheme_id, level_ids, scale, auto_boundaries?: bool | [view_id] | Yes | ARC-AREA-001 |
| `areaboundary.create` | view_id, curves \| picks: [ref], apply_rules?: bool, offset? | [element_id] | Yes | ARC-AREA-001 |
| `area.place` | view_id, point, area_type?, name?, number? | element_id | Yes | ARC-AREA-001 |
| `area.unplace` | ids | — | Yes | ARC-AREA-001 |
| `colorscheme.create` | name, category, parameter, mode: value\|range, ranges? | scheme_id | Yes | ARC-CLR-001 |
| `colorscheme.set_rows` | scheme_id, rows: [{value \| at_least, color, pattern?, visible, caption?}] | — | Yes | ARC-CLR-001 |
| `colorscheme.duplicate` | scheme_id, name | id | Yes | ARC-CLR-001 |
| `view.set_color_scheme` | view_id, category, scheme_id? (none clears), location?: background\|foreground | () | Yes | ARC-CLR-001 |
| `inplace.begin` | category, name | session_id | Within session; one step on finish | ARC-INPL-001 |
| `inplace.add_form` | session_id, kind: Extrusion\|Blend\|Revolve\|Sweep\|SweptBlend, solid: bool, params | key | Within session; one step on finish | ARC-INPL-001 |
| `inplace.finish` | session_id | element_id | Yes (one step) | ARC-INPL-001 |
| `inplace.cancel` | session_id | — | Discards session | ARC-INPL-001 |
| `inplace.edit` | element_id | session_id | Within session; one step on finish | ARC-INPL-001 |
| `modeltext.create` | text, work_plane, point, type_id, h_align?, material_id?, depth? | element_id | Yes | ARC-MTXT-001 |
| `modeltext.set_text` | element_id, text | — | Yes | ARC-MTXT-001 |
| `modelline.create` | curves: [curve], work_plane, line_style_id? | [element_id] | Yes | ARC-MLIN-001 |
| `line.convert` | element_ids, to: Model\|Detail, view_id? | [element_id] | Yes | ARC-MLIN-001 |
| `mass.begin_inplace` | name | session_id | Within session; one step on finish | ARC-MASS-001 |
| `mass.create_form` | session_id, curve_refs: [ref], solid: bool | key | Within session; one step on finish | ARC-MASS-001 |
| `mass.edit_form` | session_id, key, op: MoveSub{ref,vector}\|AddEdge\|AddProfile\|Dissolve\|Rebuild | — | Within session; one step on finish | ARC-MASS-001 |
| `mass.finish` | session_id | element_id | Yes (one step) | ARC-MASS-001 |
| `mass.place` | type_id, point, rotation?, level_id? | element_id | Yes | ARC-MASS-001 |
| `mass.show` | view_id?, mode: by_view_settings\|form_and_floors\|surface_types\|zones_and_shades | () | No (display setting) | ARC-MASS-001 |
| `mass.set_floors` | mass_id, level_ids \| All | — | Yes | ARC-MASS-002 |
| `wall.create_by_face` | type_id, face_refs, level_id, location_line, height? | [element_id] | Yes | ARC-MASS-003 |
| `floor.create_by_mass_floor` | type_id, mass_floor_ids | [element_id] | Yes | ARC-MASS-003 |
| `element.update_to_face` | element_ids | — | Yes | ARC-MASS-003 |
| `mass.select_dependents` | mass_id | [element_id] | No (query) | ARC-MASS-003 |
| `toposolid.create` | type_id, boundary: loop, level_id, offset?, points?: [xyz], split_lines?: [[xyz]] | element_id | Yes | ARC-TOPO-001 |
| `toposolid.create_from_import` | type_id, source: ImportRef(element_id) \| File(path), level_id, max_points? | element_id | Yes | ARC-TOPO-001 |
| `toposolid.toggle_smooth` | on? | () | No (display setting) | ARC-TOPO-001 |
| `toposolid.subdivide` | toposolid_id, loops, type_id, offset | element_id | Yes | ARC-TOPO-002 |
| `toposolid.split` | toposolid_id, curves: [curve] | [element_id] | Yes | ARC-TOPO-002 |
| `toposolid.excavate` | toposolid_id, cutter_ids | — | Yes | ARC-TOPO-002 |
| `toposolid.remove_excavation` | toposolid_id, cutter_ids? | — | Yes | ARC-TOPO-002 |
| `toposolid.merge` | toposolid_ids, remove_common_points?: bool = true | element_id | Yes | ARC-TOPO-002 |
| `toposolid.create_graded_region` | toposolid_id, mode: Copy\|PerimeterOnly | element_id | Yes | ARC-TOPO-003 |
| `toposolid.cut_fill` | toposolid_id | {cut, fill, net} | No (query) | ARC-TOPO-003 |
| `toposolidtype.set_contours` | type_id, rows | — | Yes | ARC-TOPO-004 |
| `contour.label` | view_id, line: Line, label_type_id? | element_id | Yes | ARC-TOPO-004 |
| `toposolid.simplify` | toposolid_id, target_points? , max_vertical_error? | — | Yes | ARC-TOPO-005 |
| `toposolid.generate_from_surface` | surface_element_id, type_id, level_id | element_id | Yes | ARC-TOPO-006 |
| `sitecomponent.place` | type_id, point, host_id?, rotation? | element_id | Yes | ARC-SITE-001 |
| `propertyline.create` | loops \| table: [{distance, bearing, kind, radius?, side?}], origin? | element_id | Yes | ARC-SITE-001 |
| `propertyline.to_table` | element_id | — | Yes | ARC-SITE-001 |
| `site.settings` | — | () | No (opens dialog) | ARC-TOPO-004 |
| `site.set_settings` | contour_interval?: f64 \| None, passing_elevation?, additional_contours?: rows, section_material_id?, poche_base_elevation?, angle_display?: Bearing\|Azimuth, angle_units?: DMS\|Decimal | () | Yes | ARC-TOPO-004 |
| `element.align` | reference_ref, target_refs: [ref], lock?: bool | — | Yes | ARC-MOD-001 |
| `element.offset` | element_ids, distance, side_point, copy: bool | [element_id] | Yes | ARC-MOD-002 |
| `element.mirror` | element_ids, axis: Line \| ref, copy: bool | [element_id] | Yes | ARC-MOD-003 |
| `element.copy` | element_ids, vectors: [vector] | [[element_id]] | Yes | ARC-MOD-005 |
| `element.rotate` | element_ids, center: point, angle, axis?: vector, copy?: bool | [element_id] | Yes | ARC-MOD-006 |
| `element.trim_corner` | a: (element_id, keep_point), b: (element_id, keep_point) | — | Yes | ARC-MOD-007 |
| `element.trim_extend` | boundary_ref, target: (element_id, keep_point) | — | Yes | ARC-MOD-008 |
| `element.trim_extend_multiple` | boundary_ref, targets: [(element_id, keep_point)] | — | Yes | ARC-MOD-009 |
| `element.scale` | element_ids, origin, factor | — | Yes | ARC-MOD-012 |
| `element.pin` | element_ids | — | Yes | ARC-MOD-013 |
| `element.unpin` | element_ids | — | Yes | ARC-MOD-013 |
| `element.delete` | element_ids[], confirm_dependents?=true | element_ids[] (deleted, dependents included) | Yes | ARC-MOD-014 |
| `element.match_type` | source_id, target_ids | — | Yes | ARC-MOD-015 |
| `geometry.uncut` | target_id, cutter_ids? | — | Yes | ARC-MOD-016 |
| `geometry.unjoin` | a_id, b_ids? | — | Yes | ARC-MOD-017 |
| `geometry.switch_join_order` | a_id, b_id | — | Yes | ARC-MOD-017 |
| `face.split` | face_ref, curves: [curve] | [face_key] | Yes | ARC-MOD-019 |
| `face.paint` | face_refs, material_id | — | Yes | ARC-MOD-020 |
| `face.remove_paint` | face_refs | — | Yes | ARC-MOD-020 |
| `clipboard.copy` | element_ids | — | No (session state) | ARC-MOD-021 |
| `clipboard.cut` | element_ids | — | Yes | ARC-MOD-021 |
| `clipboard.paste` | mode: Free(vector)\|Levels([id])\|Views([id])\|CurrentView\|SamePlace\|PickedLevel(id) | [element_id] | Yes | ARC-MOD-021 |
| `tool.create_similar` | element_id | — | No (session state) | ARC-MOD-022 |
| `measure.between` | ref_a, ref_b | {distance, dx, dy, dz, angle?} | No (query) | ARC-MOD-023 |
| `measure.along` | element_id | length | No (query) | ARC-MOD-023 |
| `measure.chain` | points: [point] | {segments: [f64], total} | No (query) | ARC-MOD-023 |
| `element.demolish` | element_ids[], phase_id? (default: the active view's phase), view_id? | () | Yes | ARC-MOD-024 |
| `element.array_linear` | element_ids, vector, count, move_to: Second\|Last, group_and_associate: bool, constrain?: bool | array_id \| [element_id] | Yes | ARC-MOD-026 |
| `element.array_radial` | element_ids, center, angle, count, move_to: Second\|Last, axis?, group_and_associate: bool | array_id \| [element_id] | Yes | ARC-MOD-026 |
| `array.set_count` | array_id, count, append_to_end: bool | — | Yes | ARC-MOD-026 |
| `group.create` | element_ids, name, kind?: Model\|Detail | {group_type_id, instance_id, attached_detail_type_id?} | Yes | ARC-GRP-001 |
| `group.place` | group_type_id, point, rotation?, level_id?, origin_offset? | element_id | Yes | ARC-GRP-001 |
| `group.swap_type` | instance_ids, group_type_id | — | Yes | ARC-GRP-001 |
| `group.duplicate_type` | group_type_id, name | group_type_id | Yes | ARC-GRP-001 |
| `group.rename_type` | group_type_id, name | — | Yes | ARC-GRP-001 |
| `group.ungroup` | instance_ids | [element_id] | Yes | ARC-GRP-001 |
| `group.set_attached_detail_visible` | instance_id, view_id, attached_type_ids | — | Yes | ARC-GRP-001 |
| `group.edit` | instance_id | session_id | Within session; one step on finish | ARC-GRP-002 |
| `group.add_members` | session_id, element_ids | — | Within session; one step on finish | ARC-GRP-002 |
| `group.remove_members` | session_id, element_ids | — | Within session; one step on finish | ARC-GRP-002 |
| `group.attach_detail` | session_id, view_kind, name | session_id | Within session; one step on finish | ARC-GRP-002 |
| `group.finish` | session_id | — | Yes (one step) | ARC-GRP-002 |
| `group.cancel` | session_id | — | Discards session | ARC-GRP-002 |
| `group.exclude_member` | instance_id, member_key | — | Yes | ARC-GRP-003 |
| `group.restore_member` | instance_id, member_key \| All | — | Yes | ARC-GRP-003 |
| `group.move_member_to_project` | instance_id, member_key | element_id | Yes | ARC-GRP-003 |
| `group.save` | group_type_id, path | — | Yes | ARC-GRP-003 |
| `group.load` | path, options | group_type_id | Yes | ARC-GRP-003 |
| `group.convert_to_link` | instance_ids, target: NewFile(path)\|ExistingFile(path) | link_id | Yes | ARC-GRP-003 |
| `constraint.lock_dimension` | dimension_id, segment?: usize | — | Yes | ARC-CNST-002 |
| `constraint.unlock_dimension` | dimension_id, segment? | — | Yes | ARC-CNST-002 |
| `constraint.set_equality` | dimension_id, on: bool | — | Yes | ARC-CNST-002 |
| `constraint.delete` | constraint_ids | — | Yes | ARC-CNST-002 |
| `constraint.list` | element_id? | [Constraint] | No (query) | ARC-CNST-002 |
| `view.reveal_constraints` | view_id, on | — | No (session state) | ARC-CNST-002 |
| `selection.set` | ids[], mode?: replace\|add\|remove (default replace) | () | No (session state) | ARC-SEL-001 |
| `selection.add` | element_ids | — | No (session state) | ARC-SEL-001 |
| `selection.remove` | element_ids | — | No (session state) | ARC-SEL-001 |
| `selection.get` | — | {ids[], by_category} | No (query) | ARC-SEL-001 |
| `selection.select_all_instances` | type_ids? (default: types of the current selection), scope: view\|project | element_ids[] | No (session state) | ARC-SEL-001 |
| `selection.filter` | category_ids[] | element_ids[] | No (session state) | ARC-SEL-001 |
| `selection.previous` | — | () | No (session state) | ARC-SEL-001 |
| `selection.set_toggles` | links?, underlay?, pinned?, by_face?, drag_on_select? | — | No (session state) | ARC-SEL-001 |
| `selectionset.save` | name, element_ids | id | Yes | ARC-SEL-002 |
| `selectionset.load` | id | [element_id] | No (session state) | ARC-SEL-002 |
| `selectionset.edit` | id, add?: [element_id], remove?: [element_id] | — | Yes | ARC-SEL-002 |
| `selectionset.delete` | id | — | Yes | ARC-SEL-002 |
| `selectionset.rename` | id, name | — | Yes | ARC-SEL-002 |
| `selectionset.list` | — | [{id, name, count}] | No (query) | ARC-SEL-002 |

---

## Open questions / deviations from reference

### Intentional deviations

| # | Topic | Reviera behaviour | Reason |
|---|---|---|---|
| D1 | Options Bar | None. Tool options live on the contextual tab or in the Properties *New <Element>* block (ARC-TOOL-001) | Matches the reference's own 2027 direction, and gives one UI model for every tool and for tools added from Lua |
| D2 | Project format | IFC4 is the native format. Parametric intent is stored as RXD (`02a_extension_data_format.md`) | Required by `TECH_STACK.md`. Other IFC tools can read every element, and a tool that drops RXD still leaves valid geometry |
| D3 | Compound priority scale | Reviera uses 1 (strongest) to 9. IFC4 `IfcMaterialLayer.Priority` uses 0–100 (higher is stronger), so export maps it with `100 − 10·(p−1)`. The exact value is kept in RXD | IFC defines the opposite direction |
| D4 | Join limit | Automatic detection is limited to 4 ends per node, as in the reference. The algorithm itself is general (ARC-JOIN-001 D.8) | Keeps the expected behaviour while leaving room to raise the limit later |
| D5 | Curtain "Fixed Number" | Defined as the number of **interior grid lines** (ARC-CW-003) | The reference's wording is ambiguous, and Reviera states one meaning so the result is testable |
| D6 | Excavation robustness | OCCT fuzzy Booleans with one retry, instead of randomly jittering void positions | Deterministic regeneration (ARC-REGEN-001) rules out random perturbation |
| D7 | Import thresholds | Point caps are ordinary settings (`toposolid.max_points`, `toposolid.link_max_points`), not an INI file | Single settings system (DM chapter) |
| D8 | Legacy terrain | No surface-only toposurface or building pad creation. Imported surface terrains are converted with Generate Toposolid (ARC-TOPO-006) | Reviera has no legacy files to support. The toposolid workflow covers the same use cases |
| D9 | Group files | *Save Group* and *Load as Group* use `.ifc` with embedded RXD, not a proprietary format | Open format. The proprietary formats are out of scope (`00_overview.md`) |
| D10 | Stair by sketch | No separate legacy tool. Sketching happens only inside the stair assembly (ARC-STAIR-002) | Matches the reference's current behaviour, with a single code path |
| D11 | Divide Surface, patterns, adaptive components in the conceptual environment | P3 (deferred). Masses support forms and editing only (ARC-MASS-001) | Large scope that the core architectural workflows do not need. Lua-driven panelisation is an open alternative |
| D12 | Model text glyphs | Rendered through FreeType from system or bundled fonts | Licence-compatible font pipeline. Font availability varies by platform, so bundling an open font (for example DejaVu) is recommended |
| D13 | Room reference lines | Initially through the centre of the boundary's bounding box | Simple and deterministic |
| D14 | Paint on faces in IFC | Painted faces are split into separate representation items so they can carry `IfcStyledItem` | IFC styles apply per representation item, not per face |
| D15 | In-place copies | Copying still creates a new in-place family, as in the reference. A later option may allow *Convert to loadable family* (DM chapter) | Keeps instances independent, as users expect |
| D16 | Toposolid merge | Reviera provides `toposolid.merge`, which joins toposolids that share an edge or overlap into one element (ARC-TOPO-002) | The reference dropped Merge Surfaces with the move to toposolids and offers only Join Geometry, which leaves the seam visible; a true merge is the natural inverse of Split |
| D17 | Parts in IFC | By default a source element with parts is exported without its own `Body`; its leaf parts carry the geometry as `IfcBuildingElementPart` through `IfcRelAggregates` (ARC-PART-001). An option writes both | Avoids double counting in take-off and clash tools that read both the whole and its parts |
| D18 | Mirrored state | *Mirrored* is set only by mirroring (Mirror tool or a family mirror control), never by Flip Hand / Flip Facing, and the flips are stored explicitly (ARC-FLIP-001) | Keeps mirrored doors findable in schedules and makes the IFC reflection operator deterministic |

### Open questions

1. **IFC4 `IfcGrid` with a single direction.** IFC4 requires both `UAxes` and `VAxes` to be non-empty. When every grid line is parallel, should Reviera (a) put the last axis in `VAxes` (valid, but semantically wrong), (b) write one `IfcGrid` per axis plus a zero-length companion axis, or (c) use `WAxes`? The current draft follows (a), with a reconciliation note in the RXD data. This needs a decision together with the DM chapter's IFC validation rules.
2. **Slab edges in IFC.** The draft uses `IfcSlab(USERDEFINED "SLABEDGE")`. The alternatives are `IfcBuildingElementProxy` and `IfcMember`, and the choice affects quantity take-off in other tools. A decision is needed together with the STR chapter, which owns thickened edges and curbs.
3. **Areas as `IfcSpace`.** Writing areas from area schemes as `IfcSpace` (GFA or USERDEFINED) may make naive tools count spaces twice. One option is to export areas only when the user asks for them (the *Export area schemes* setting).
4. **Reference levels.** Non-story levels are written as `IfcAnnotation`. IFC4X3 would allow `IfcBuildingStorey` with a "not a story" property, but other tools would still treat it as a storey. Confirm with COL, because BIMserver's storey-based queries are affected.
5. ~~**RXD kind names.**~~ **Resolved**: every element and type kind used in this chapter is registered in `02a_extension_data_format.md` §6.5 (Object Kind Registry).
6. **Room boundary performance targets.** Proposed: placing a single room takes under 50 ms, and Place Rooms Automatically on a level with 500 rooms takes under 2 s on a 4-core reference machine. To be confirmed with the benchmarking plan.
7. **Winder distribution.** The linear-angle "balanced" winder distribution (ARC-STAIR-004) is a reasonable default. Regional codes may need alternatives (for example, a fixed minimum inner width or kite winders), which could be added as a type option.
8. **Roof skeleton with slope arrows.** The generalised wavefront handles arrows whose contour direction differs from the edge direction. Pathological inputs, such as arrows that make a face rise toward the footprint edge, need defined error messages, and a corpus of test footprints should be built.

---

## Traceability

| Research section (`research/02_architecture_modeling.md`) | Covered by |
|---|---|
| §1 Release differences (2024–2027) | ARC-TOOL-001 (no Options Bar), ARC-CMPD-001 (empty core, Priority column, Duplicate layer), ARC-JOIN-001 (in-canvas join control), ARC-WRAP-001 (per-end wrap), ARC-WALL-014/015/016 (By Room/Segment, Auto Join, hosted walls), ARC-SHAPE-001 (snap options, copy/paste), ARC-TOPO-001–006 (toposolid, sub-division offset, simplify), ARC-STAIR-006 (2027 tread numbering), ARC-MOD-* (2026 tool-option migration), D1 |
| §2.1 Ribbon and contextual tabs | ARC-TOOL-001, ARC-PART-001 (Create › Parts) |
| §2.2 Type vs instance | ARC-TOOL-002 |
| §2.3 Sketch mode | ARC-SKCH-001, ARC-SKCH-002, ARC-SKCH-003 |
| §2.4 Placement aids | ARC-DIM-001, ARC-DIM-002, ARC-SNAP-001, ARC-KEY-001 |
| §2.5 Element controls | ARC-CTRL-001, ARC-FLIP-001 |
| §3.1 Levels | ARC-LVL-001 |
| §3.2 Grids | ARC-GRID-001 |
| §3.3 Reference planes | ARC-REFP-001 |
| §3.4 Scope boxes | ARC-SCOPE-001 |
| §3.5 Datum extents | ARC-DATUM-001 |
| §3.6 Work planes | ARC-WP-001 |
| §4.1 Wall families and kinds | ARC-WALL-001 |
| §4.2 Placing walls | ARC-WALL-002, ARC-WALL-014, ARC-WALL-015 |
| §4.3 Wall instance properties | ARC-WALL-003, ARC-WALL-005, ARC-CMPD-004 |
| §4.4 Wall type properties | ARC-WALL-004 |
| §4.5 Edit Assembly, layer joins, regions, wrapping, slanted and tapered | ARC-CMPD-001–004, ARC-WRAP-001, ARC-WALL-005, ARC-WALL-010, ARC-JOIN-001, ARC-PART-001 (layer-to-part mapping) |
| §4.6 Stacked walls | ARC-STKW-001 |
| §4.7 Wall joins | ARC-JOIN-001, ARC-MOD-018 |
| §4.8 Attach/Detach | ARC-WALL-008 |
| §4.9 Edit Profile | ARC-WALL-009 |
| §4.10 Sweeps and reveals | ARC-WALL-010 |
| §4.11 Wall openings | ARC-WALL-012, ARC-OPEN-003 |
| §4.12 Split | ARC-WALL-011, ARC-MOD-010, ARC-MOD-011 |
| §4.13 Other wall behaviours, embedded and hosted walls | ARC-WALL-013, ARC-WALL-016, ARC-WALL-017 |
| §5.1 Hosting model | ARC-HOST-001 |
| §5.2 Doors | ARC-DOOR-001, ARC-FLIP-001 |
| §5.3 Windows | ARC-WIN-001, ARC-FLIP-001 |
| §5.4 Components | ARC-COMP-001 |
| §5.5 Architectural columns | ARC-COL-001 |
| §6.1–6.6 Curtain walls and systems | ARC-CW-001 – ARC-CW-008 |
| §7.1 Floors | ARC-FLR-001 |
| §7.2 Slab edges | ARC-SLED-001 |
| §7.3 Ceilings | ARC-CEIL-001 |
| §7.4 Roofs (footprint, extrusion, face, fascia, gutter, soffit, slope arrows, joins, sloped glazing) | ARC-ROOF-001 – ARC-ROOF-006, ARC-CW-008 |
| §7.5 Shape editing | ARC-SHAPE-001 |
| §8.1–8.4 Stairs | ARC-STAIR-001 – ARC-STAIR-006 |
| §8.5 Railings | ARC-RAIL-001, ARC-RAIL-002 |
| §8.6 Ramps | ARC-RAMP-001 |
| §9 Openings | ARC-OPEN-001 – ARC-OPEN-005 |
| §10.1 Rooms and room computation | ARC-ROOM-001, ARC-ROOM-002 |
| §10.2 Areas | ARC-AREA-001 |
| §10.3 Colour schemes | ARC-CLR-001 |
| §11.1 Model In-Place | ARC-INPL-001 |
| §11.2 Model text | ARC-MTXT-001 |
| §11.3 Model lines | ARC-MLIN-001 |
| §12.1 Massing and model by face | ARC-MASS-001 – ARC-MASS-003, ARC-CW-007, ARC-ROOF-003 |
| §12.2 Toposolid | ARC-TOPO-001 – ARC-TOPO-005 (smooth shading, merge, site settings) |
| §12.3 Legacy toposurface and building pad | ARC-TOPO-006, ARC-TOPO-002 (merge), ARC-TOPO-004 (site settings) |
| §12.4 Site components, parking, property lines | ARC-SITE-001 |
| §13 Modify tools | ARC-MOD-000 – ARC-MOD-027, ARC-FLIP-001 (mirrored instances) |
| §14 Groups | ARC-GRP-001 – ARC-GRP-003, ARC-MOD-026 |
| §15.1 Temporary dimensions | ARC-DIM-001, ARC-DIM-002 |
| §15.2 Constraints | ARC-CNST-001, ARC-CNST-002 |
| §15.3 Selection and selection sets | ARC-SEL-001, ARC-SEL-002 |
| §16 Keyboard shortcuts | ARC-KEY-002 |
| §17 Implementation notes 1–13 | ARC-TOOL-001 (1), ARC-SKCH-* (2), ARC-CMPD-* (3), ARC-JOIN-001 (4), ARC-HOST-001 (5), ARC-CW-003/005 (6), ARC-SHAPE-001 (7), ARC-STAIR-* (8), ARC-RAIL-* (9), ARC-ROOM-002/ARC-AREA-001/ARC-CLR-001 (10), ARC-TOPO-* (11), ARC-MOD-000/026 (12), ARC-GRP-* (13), ARC-REGEN-001 |
| §18 Screenshot index | Cited in the feature blocks (internal reference only, never shipped) |
| §19 Sources | Through `RESEARCH_LOG.md` |
| Public web sources added in the 2026-10-12 gap fill (vendor help pages and one public blog), cited as `web:` in the Ref lines | ARC-PART-001 – ARC-PART-004 (About/Create/Divide/Merge/Exclude/Delete Parts, Part and Division Instance Properties, Part Visibility), ARC-FLIP-001 (About Flip Controls, Flip Elements), ARC-WP-001 (Workplane Viewer), ARC-ROOM-002 (Room Area and Volume), ARC-MASS-001/002 (Mass visibility, mass floors at top and bottom), ARC-TOPO-001 (Smooth Shading), ARC-TOPO-002 (Merge Toposurfaces; Ascent 2024 site article), ARC-TOPO-004 (Contour Display, Site/Toposurface Settings, Property Line Data), ARC-TOPO-005/006, ARC-ROOF-003/005, ARC-DIM-002, ARC-MOD-007–009, 013, 022–024 |
