# Chapter 4 — Structure, Steel, Concrete/Precast, MEP Systems and Analysis

**Status:** Draft 1 (2026-10-12) · **Area codes:** `STR`, `MEP`, `ANA` · **Primary source:** `research/03_structure_mep.md` (cited as `research/03`) · **Architecture source:** `TECH_STACK.md`
**Reference baseline:** Revit 2026. Where Reviera follows the reference's 2027 direction, this is noted.

This chapter covers the engineering disciplines built on Reviera's shared element, type and parameter kernel (chapter 2, `DM-`):

- structural physical modeling and reinforcement
- the structural analytical model
- steel fabrication detailing
- concrete and precast detailing
- the MEP connector, system and routing framework
- HVAC, piping and electrical design
- MEP fabrication
- spaces and system-zones
- the analysis services on the Analyze tab

Requirements are written in Reviera's own terms. The reference product is named only in "Reference" lines and in §4.17.

### Where data is stored (applies to the whole chapter)

Compatibility comes first, so Reviera puts data in standard IFC wherever IFC can express it. That includes:
- physical elements and rebar
- structural analysis model entities, loads, load groups and boundary conditions
- MEP segments, fittings, terminals, equipment and ports
- distribution systems and circuits
- spaces and zones
- standard `Pset_*`/`Qto_*` values

Data that IFC cannot express is **Reviera Extension Data (RXD)**. Its format and carriers are defined in `spec/02a_extension_data_format.md`, and this chapter does not define them. By default RXD is embedded in the IFC file. Every element reference inside RXD uses the IFC GlobalId. This chapter uses two kinds of RXD:

- **RXD element data**: per-element data, such as a rebar set's layout recipe or a column's attachment settings. Each block names the content area it belongs to, for example "RXD element data (`constraints`)".
- **RXD document `<name>`**: project-level documents, such as MEP settings, electrical definitions or analysis settings.

| RXD content area | Kind | Contents used by this chapter |
|------------------|------|-------------------------------|
| `constraints` | element data | Constraints, attachments, joins, justification, layout rules and sketches. Examples: column attachments, beam-system layout, rebar handle constraints. |
| `rebar` | element data + document | Element data: rebar layout-rule recipes, partition, number, splice and crank definitions, area/path/fabric system recipes, per-face cover assignments. Document: reinforcement settings, rounding, cover types and numbering sequences. |
| `mep_settings` | document | Mechanical settings, routing preferences, size tables, pipe segments, fluids, slopes, conduit and cable-tray settings, system-type calculation options. |
| `electrical` | document | Electrical settings, distribution systems and voltages, wiring types, conductor and cable tables, load classifications, demand factors, circuit naming schemes, panel schedule templates, the electrical analytical model. |
| `analysis` | document (+ element data) | Analytical-model settings, load natures and code selection, automation logs, analysis provider settings, energy and route-analysis settings, and caches and indexes of result packages. Element data: analytical-element flags that IFC cannot hold. |
| `fabrication` | document | The MEP fabrication content configuration (§4.10) and the precast configuration. |
| `families` | family recipes | Rebar shape families, truss layouts, connection-library type recipes, deck profiles, fabrication part templates. |
| `structural` *(proposed addition, §4.17 Q1)* | element data + document | Structural settings, structural-connection status data, steel part and assembly marks, precast piece provenance. |

---

## 4.0 Chapter-wide principles

### 4.0.1 Three layers per discipline
Ref: research/03 §0, §15.1–15.4

Every discipline in this chapter is built from three layers:

| Layer | What it is | Examples |
|-------|------------|----------|
| **Physical** | Typed occurrences with geometry, materials and discipline behaviour | Column, beam, rebar set, duct, pipe, panelboard |
| **Logical / analytical** | Elements with no geometry, or idealised geometry, that capture how things connect or how they are idealised for calculation | Analytical member, load, MEP system, electrical circuit, energy model, analytical duct segment |
| **Calculation services** | Asynchronous engines that read the first two layers and write results back as parameters or result packages | Duct sizing, voltage drop, load roll-up, structural solve, energy simulation |

| ID | Priority | Requirement |
|----|----------|-------------|
| STR-GEN-001 | P1 | Reviera shall store logical and analytical elements as first-class, independently persisted elements, not as derived caches. Deleting a physical element shall never silently delete an analytical element unless an explicit association rule says it should. Each such element has its own GlobalId, its own undo history and its own IFC entity (for example `IfcStructuralCurveMember`, `IfcDistributionSystem`), so it survives save, reload and BIMserver round trips with no physical counterpart. When a physical element with associated analytical elements is deleted, the confirmation lists them and offers *Keep* (the default) or *Delete too*; kept elements lose the association and are reported by STR-AM-007. |
| STR-GEN-002 | P1 | Every hosted relationship in this chapter shall cascade deletes and report orphans as warnings, never silently. Examples are a load on an analytical element, rebar in a concrete host, a coupler on bar ends, and a boundary condition on a node. A delete that cascades shows one confirmation that counts the dependants per kind (for example "4 loads, 1 boundary condition") and removes them in the same transaction, so one undo restores everything. A host that stops qualifying without being deleted (for example a material change) leaves its dependants in place, flagged as orphaned, with a warning that selects them and offers re-host or delete. Scripted and MCP calls get the same counts in the command result's `warnings[]` and never prompt. |
| ANA-GEN-001 | P2 | Calculation services shall run on the core worker pool in the background and never block the UI thread. Each run snapshots the model at a transaction boundary. Results are committed in a new transaction tagged with the service name, so they can be undone like any other change. If the model changed after the snapshot was taken, the results are flagged as **stale** and a re-run is offered. |
| ANA-GEN-002 | P2 | Results computed per network or per member shall be stored on parameters or sub-elements that can be scheduled and tagged, not only in reports. Examples are analytical duct or pipe segments, circuit voltage drop and member forces. Each result parameter is read-only, carries the job ID and a stale flag (ANA-GEN-001), and is filterable in view filters. Standard values go to IFC psets where one exists; the rest are RXD element data (`analysis`). Deleting a result package clears its parameters in one undoable transaction. |

### 4.0.2 Optional external engines (plugin model)
Ref: research/03 §4.6, §13.2–13.4, §15.4

Several capabilities in the reference product depend on proprietary or cloud engines: a structural solver link, cloud lighting and solar analysis, online load-combination generation, a fabrication database, and P&ID cloud collaboration. Reviera replaces each of these with a **provider interface** defined in the core, and **optional engine plugins** that implement it.

| ID | Priority | Requirement |
|----|----------|-------------|
| ANA-PLUG-001 | P2 | Reviera shall define *analysis provider* interfaces in the command registry, one per analysis family: `structural`, `seismic`, `energy`, `loads` (thermal heating and cooling loads), `daylight`, `solar` (insolation, sun paths and shadow studies), `hydraulic` (domestic water, fire and plumbing flows), `stormwater` (roof, site and drainage runoff), `airflow` (interior air movement and ventilation, CFD), `wind` (external wind flow and pressures around buildings, CFD), `clash`, `route`. Each provider declares its name, version and SPDX licence; its inputs (the model subset and the exchange format); its outputs (a result-package schema); and its capabilities, for example "linear static" or "modal". Families also include `carbon` (ANA-CARB-001). The manifest is a JSON file validated on `analysis_provider.register`; a manifest with an unknown kind, a missing licence or a licence outside the GPL-compatible allow-list (for in-process providers) is refused with the failing field named. Several providers per kind may be registered; one is the project default, stored in RXD document `analysis`. |
| ANA-PLUG-002 | P2 | A provider is either **in-process** (a Lua add-in, or a native library loaded on demand) or **out-of-process** (a compute job run locally, on a LAN host or on cloud compute through the remote computation subsystem, chapter 2 §2.17, or a Web service). Live calculations are in-process and batch runs out-of-process (ANA-PLUG-008); the analysis layer is specified in §4.0.2a (ANA-PLUG-004 … 009). Out-of-process engines are never linked into the binary, which keeps engines under BSD-style or custom permissive licences clear of linking questions. Engine licences shall still be checked against the GPL-3.0 allow-list before any official bundle ships one (TECH_STACK §4.2). |
| ANA-PLUG-003 | P2 | Reviera shall ship working **built-in reference providers** for calculations that are simple and well defined: duct and pipe friction sizing, voltage drop, demand factors, the lumen method, 2D path of travel and embodied carbon (quantity × factor). Heavy engines are offered as plugins. Built-in providers follow the live and batch split (ANA-PLUG-008): their live calculations run in Reviera's process as the user models, and their batch runs are built into one bundled program, `reviera-analysis-builtin`, run as a compute job on demand (DM-RPC-004) and stopped when idle. They register through the same manifest as plugins, appear in `analysis_provider.list` with licence `GPL-3.0-or-later`, and each has a verification test against a published hand-calculated case in the test suite. They work with no network access. |

Engine suggestions in this chapter are marked **[Suggestion]**. They are candidates to evaluate, not commitments. A summary is in §4.15.

**Commands.**
- `analysis_provider.list(kind?) -> providers[]`
- `analysis_provider.register(manifest_path) -> provider_id`
- `analysis_job.status(job_id) -> {state, progress, messages}`
- `analysis_job.cancel(job_id)`

### 4.0.2a Analysis plugin layer
Ref: owner requirement (2026-10-15); Reviera-originated design.

**Purpose.** Analysis is much more than structural FEA: thermal loads and energy, natural light and shade, earthquake response, plumbing and stormwater flows, interior airflow and external wind. Reviera shall reach every analysis family (ANA-PLUG-001) through one **plugin layer** built on the remote computation subsystem (chapter 2 §2.17, DM-RPC-001 … 008), which abstracts where and how a workload runs: in a local process started on demand, on a more powerful machine on the LAN, or on cloud compute. The direction is to run the most rigorous analyses (fine distillations, nonlinear and time-history runs, high-resolution CFD, annual simulations, large parameter studies) on cloud compute, and the quick screening runs locally. **No analysis engine is memory-resident when it is not in use.**

**Design principles.**
- **Live in-process, batch out-of-process.** A calculation that updates in real time while the user models runs in Reviera's process. A batch run the user starts, or one over the whole model or a large scope, is a compute job (§2.17) and runs out of process, started or connected to when it is needed (ANA-PLUG-008).
- **A distillation per family.** A batch analysis sends only what its family needs (ANA-PLUG-009, under the rules of DM-RPC-003).
- **One contract.** Batch analysis families are job kinds `analysis.<family>` (DM-RPC-002); their messages are the package `reviera.analysis.v1`. A provider can move from local to LAN to cloud to a Web service without changing the analysis commands.

| ID | Priority | Requirement |
|----|----------|-------------|
| ANA-PLUG-004 | P2 | **Transports.** A provider manifest shall name its transport. **`in_process`**: required for `live` providers (ANA-PLUG-008) and opt-in for small batch ones: a Lua add-in (ANA-PLUG-002), the built-in live calculations, or a native shared library with Reviera's C ABI (`reviera_analysis_v1` entry point), loaded when first needed and unloaded when idle; third-party in-process providers must pass the GPL-compatible licence check (TECH_STACK §4.2). **`compute`** (the default for `batch` providers): the provider is the runner of a job kind `analysis.<family>` (DM-RPC-002), run locally, on a paired host or on a cloud host (DM-RPC-004 … 006); the built-in batch calculations ship as one runner, `reviera-analysis-builtin`. **`http`**: a Web service reached through the contract's HTTPS binding (DM-RPC-001), batch only. `analysis_provider.register` refuses a manifest whose mode or transport block is missing or inconsistent (a `live` provider that is not `in_process`), naming the field. |
| ANA-PLUG-005 | P2 | **Provider lifecycle.** A batch provider follows the runner lifecycle and limits of DM-RPC-004 (on demand, idle timeout, OS-enforced memory and CPU limits, contained failures). A live in-process provider holds its working memory only while it computes; a native library is unloaded after the idle timeout where the platform allows. `analysis_provider.start`, `.stop` and `.test` act on the provider's runner; `analysis_job.status` and `.cancel` (ANA-GEN-001) are the analysis view of `compute.status` and `compute.cancel`. |
| ANA-PLUG-006 | P2 | **Analysis results.** A batch analysis's result package (ANA-FEA-001 for structural; each family's own schema otherwise) is taken back into the model by the family's result applier (DM-RPC-002): result parameters (ANA-GEN-002), result packages referenced by the model (STR-RES-001), and display through Analysis Display Styles (ANA-ADS-001). The package's provenance manifest (DM-RPC-001) is shown in Results Explorer, and a result computed from an older snapshot is marked stale (ANA-GEN-001). |
| ANA-PLUG-007 | P2 | **Analysis Providers panel.** *Analyze › Analysis Providers* lists every provider: family, mode (live or batch), transport, where it runs (in Reviera, local runner, host, cloud host, Web service), licence, state, and its settings: live budget, idle timeout, limits, and location per detail level (§2.17.6 routing by rigour). It offers Start, Stop, Test and a link to *Manage › Compute Jobs* (DM-RPC-008). Every action is a command, equally available to Lua and MCP. |
| ANA-PLUG-008 | P2 | **Live and batch calculations.** A provider manifest shall declare its **mode**. **`live`**: a calculation that updates while the user models, for example duct and pipe sizing as a run is drawn, voltage drop as a circuit is built, the lumen method as fixtures are placed, a room's heating load as its walls move, embodied carbon as quantities change, and the modelling checks of the overlap engine (ARC-MOD-030) and clearances (ARC-MOD-031). Live calculations run **in-process** on the core worker pool, on the edited elements only, within a budget of 50 ms per edit on the reference model (settable); one that exceeds its budget three times in a row is suspended for the session with a notice, and its results show as stale until refreshed. **`batch`**: a run the user starts, or one whose scope is the whole model or a large selection, for example a structural or seismic solve, an energy simulation, a daylight or shadow study, a stormwater or hydraulic network run, CFD, or a project-wide clash test (ANA-CLASH-001). Batch runs are compute jobs and never block modelling. A calculation can offer both modes: the built-in duct sizing is live for the run being drawn and batch for *Size whole system*. **`interactive`**: a heavy simulation steered in real time, run as a session (DM-RPC-010) on a LAN or cloud host and shown live in the viewport, a game engine or VR while the user changes scenario inputs; for example external wind, interior airflow, daylight and shadow through the day, a stormwater event, or a structure under a moving load. A provider may offer batch and interactive modes for the same engine. |
| ANA-PLUG-009 | P2 | **Analysis distillers.** Each batch analysis family shall define its distiller (DM-RPC-003): mostly an IFC filter profile that keeps only the elements and data the analysis uses, plus transforms where the family needs another representation or format. Contents per family: **structural and seismic**: the IFC4 Structural Analysis View as a profile: analytical members, panels, nodes, links, supports, sections, materials, masses, loads, cases and combinations, no physical geometry, no transform needed (ANA-FEA-001); **energy and thermal loads**: thermal zones, space boundaries as planar surfaces with their constructions, openings with glazing properties, shading surfaces, internal gains, schedules and the site and weather reference (gbXML or IFC space boundaries); **daylight, sun and shadow**: room and shading surfaces as simplified meshes with reflectance and transmittance, glazing, sensor grids, site context and location (a Radiance scene); **hydraulic and plumbing**: the pipe network graph with lengths, sizes, fittings, elevations, fixtures and demands; **stormwater**: roof and paved catchments with area, slope and surface, gutters, downspouts and drainage runs with their sizes and falls; **airflow**: the interior fluid domain as a watertight mesh with inlets, outlets, heat sources and boundary conditions; **wind**: the exterior envelope as a simplified watertight shell with nearby site context and terrain; **clash**: element bodies of the chosen categories only. Each family's results come back keyed to its distillation and are attached by its result applier (ANA-PLUG-006). |

**Packaging.** Reviera's process contains the live calculations. It bundles `reviera-analysis-builtin` (the built-in batch calculations, ANA-PLUG-003) as a runner beside `reviera`. Engine wrappers (for example a CalculiX, Code_Aster, xara or OpenFOAM runner, §4.15) are separate programs, each in its own folder with its job-kind and provider manifests. The Flatpak and the Windows installer bundle no third-party engine by default; *Get Engines…* in the Providers panel installs them.

**Commands.**
- `analysis_provider.start(provider_id, host_id?) -> {state}`
- `analysis_provider.stop(provider_id) -> ()`
- `analysis_provider.status(provider_id?) -> [{provider_id, mode, state, location, pid?, memory_bytes?, jobs[]}]`
- `analysis_provider.set_options(provider_id, {live_budget_ms?, idle_timeout_s?, memory_limit_bytes?, cpu_limit?, location?}) -> ()`
- `analysis_provider.test(provider_id, host_id?) -> {ok, manifest, messages[]}`

**Acceptance criteria.**
- [ ] Drawing a duct run updates its live sizing within the 50 ms budget per edit without starting any process; *Size whole system* on the same model runs as a batch compute job on `reviera-analysis-builtin`, which exits after its idle timeout.
- [ ] A live provider that overruns its budget three times in a row is suspended with a notice, and modelling stays responsive.
- [ ] The spec's reference case (a simply supported 6 m beam, 10 kN/m, ANA-FEA-001) gives the same result package, apart from timings, run locally and on a paired compute host.
- [ ] The structural distillation of the manor sample is under 5% of the size of its IFC file and contains no physical geometry, and its results attach to the right members.
- [ ] Re-running a 2,000-member model after changing one load sends less than 10% of the first run's bytes.
- [ ] A daylight job and a structural job run at the same time on two runners, each within its own limits, and both exit after their idle timeouts.

### 4.0.3 UI conventions for this chapter
Ref: research/03 §0 (2027 row, Options Bar removed), §15.7

- There is **no Options Bar**. Placement options appear on the contextual "Modify | Place …" ribbon tab and in the Properties palette. Examples are size, elevation, justification, slope and level. This matches the reference product's 2027 direction and Reviera's UI chapter.
- The chapter uses dockable palettes (`wxAuiManager`): **System Browser**, **Fabrication Parts**, **Sizing Calculator**, **P&ID Line List** and **Results Explorer**.
- The chapter uses in-canvas controls: rebar segment lengths, connector glyphs, fitting "+/−" upgrade and downgrade, flip and rotate arrows, and shape handles.
- Discipline settings dialogs are tree-structured and stored per project as RXD documents. They can be moved between projects with *Transfer Project Standards* (chapter 2).
- Keyboard shortcuts in this chapter are **proposed Reviera defaults**. Users can remap them. They are not taken from any reference shortcut file.

### 4.0.4 Ribbon layout (Reviera)
Ref: research/03 §1.1–1.6

| Tab | Panels and tools |
|-----|------------------|
| **Structure** | *Structure*: Beam, Wall: Structural, Column, Floor: Structural, Truss, Brace, Beam System, Component. *Connection*: Connection, Connection Library, Show Warnings. *Foundation*: Isolated, Wall, Slab, Pile. *Datum*, *Work Plane*, *Opening*: shared with Architecture. Panel launcher: Structural Settings. |
| **Steel** | *Connection*: Connection, Connection Library, Automate. *Fabrication Elements*: Plate; Bolts split button (Bolts, Anchors, Holes, Shear Studs); Welds. *Modifiers*: Corner Cut, Cope Skewed, Shorten, Contour Cut. *Parametric Cuts*: Cope, Miter, Saw Cut (Flange/Web), Cut Through, Cut By, Uncope. *Output*: Numbering, NC Export [Suggestion]. |
| **Concrete** | *Reinforcement*: Rebar, Area, Path, Fabric Area, Fabric Sheet, Coupler, Path Distribution, Morphed Distribution, Cover. *Settings*: Reinforcement Settings, Cover Settings, Reinforcement Numbers, Shape Browser. *Precast Detailing*: Split, Mounting Parts, Auto Reinforce, Shop Drawings, CAM Export. *Precast Settings*: Configuration, Custom Fabric Sheet Rules, Automatic Updates toggle. (This follows the reference product's 2027 direction of one concrete tab, with no separate precast tab.) |
| **Systems** | *HVAC*: Duct, Duct Placeholder, Duct Fitting, Duct Accessory, Convert to Flex, Flex Duct, Air Terminal. *Fabrication*: Fabrication Part, Multi-Point Route, Fabrication Settings. *Mechanical*: Mechanical Equipment, Control Device. *Plumbing & Piping*: Pipe, Pipe Placeholder, Parallel Pipes, Pipe Fitting, Pipe Accessory, Flex Pipe, Plumbing Equipment, Plumbing Fixture, Sprinkler. *P&ID*: Line List, P&ID Settings. *Electrical*: Wire, Cable Tray, Conduit, Parallel Conduits, Cable Tray Fitting, Conduit Fitting, Electrical Equipment, Device, Lighting Fixture. |
| **Analyze** | *Structural Analytical Model*: Analytical Member, Panel, Opening, Link, Automation, Association, Adjust, Reset, Check Supports, Check Consistency. *Analytical Loads*: Loads, Boundary Conditions, Load Cases & Combinations. *Structural Analysis*: Run Analysis, Results Explorer, Results Manager. *Spaces & Zones*: Space, Space Separator, Space Tag, Space Naming, System-Zone. *Reports & Schedules*: Pressure Loss Report, Panel Schedules, Heating & Cooling Loads. *Check Systems*: Check Duct/Pipe Systems, Check Circuits, Show Disconnects. *Color Fill*: Duct Legend, Pipe Legend. *Energy*: Location, Energy Settings, Create/Delete Energy Model, Systems Analysis. *Daylight & Solar*: Lighting, Solar. *Route Analysis*: Path of Travel, Multiple Paths, One-Way Indicator, Reveal Obstacles, Settings, Spatial Grid. *Electrical Analysis*: Area-Based Load, Equipment Load, Load Boundaries. *MEP Connections*: Show Connector Controls. |
| **Manage** (discipline part) | Structural Settings; MEP Settings drop-down: Mechanical Settings, Electrical Settings, Conductor & Cable Settings, Load Classifications, Demand Factors, Building/Space Type Settings, Panel Schedule Templates. |

---

## 4.1 Structural physical elements

**Common data model for structural framing.** Columns, beams and braces are family-based occurrences (chapter 2) with these properties:

- a **structural material** whose *material for model behaviour* is steel, concrete, precast concrete, wood or other
- a **section shape** parameter set: hot-rolled, cold-formed or concrete dimensions, used by fabrication and analysis
- a **symbolic representation** at Coarse detail: a stick line with cutback, drawn in the style set in Structural Settings
- an optional **association** to analytical elements (§4.3)

### 4.1.1 Structural columns
**ID:** STR-COL-001 · **Priority:** P1 · **Ref:** research/03 §2.1, §1.1

**Purpose.** Reviera shall provide load-bearing vertical columns. They join to beams, braces and isolated foundations, carry a structural material, can host rebar when they are concrete, and can be associated with an analytical member.

**Workflow (vertical).**
1. Choose *Structure › Structure › Column* (proposed shortcut `CL`), or *Architecture › Column › Structural Column*.
2. Pick a type in the Type Selector. On the contextual tab choose:
   - **Depth** or **Height**
   - the top or bottom constraint: a level, or *Unconnected* with a length
   - *Rotate after placement*
   - in 3D views, the placement level
3. Click to place. The cursor snaps to grid intersections and highlights both grid lines. **Spacebar** turns the column to the directions of the intersecting grids, or by 90° when there is no grid.

**Workflow (multiple placement).**
- *Multiple › At Grids*: select grid lines with a window or Ctrl+click. Spacebar rotates all previews. **Finish** commits them as one transaction.
- *Multiple › At Columns* puts a structural column inside each selected architectural column.

**UI.** Contextual tab "Modify | Place Structural Column": Vertical/Slanted, Multiple (At Grids, At Columns), Tag on Placement, Rotate after placement, Level/Depth/Height. Properties palette: base and top level and offset, *Moves With Grids*, cut styles, extensions, attachment settings, structural material, and the analytical association (read-only).

**Behaviour & rules.**
- Column top and base can **attach** to floors, roofs, beams and reference planes (*Attach Top/Base*, *Detach*). Attachment options:
  - **Attachment Justification**: minimum intersection, column midline, maximum intersection, or tangent
  - **Offset From Attachment**
  - **Base/Top Geometry Alignment**: location line, or the top, centre or bottom of the beam
- **Cut style** at each end is perpendicular, horizontal or vertical. Each end can carry an extension.
- *Moves With Grids* locks the column to its grid intersection, so moving a grid regenerates the column.
- A read-only **Column Location Mark** is derived from the nearest grid intersection, for example "C-4".
- Concrete columns are valid rebar hosts (§4.2.1).

**Data (IFC mapping).**
- `IfcColumn` with PredefinedType `COLUMN`. Types are `IfcColumnType`.
- Body: `IfcExtrudedAreaSolid` over an `IfcProfileDef` subtype (`IfcIShapeProfileDef`, `IfcRectangleProfileDef`, …). Material: `IfcMaterialProfileSetUsage`.
- Property sets: `Pset_ColumnCommon` (LoadBearing = TRUE, IsExternal, FireRating) and `Qto_ColumnBaseQuantities`.
- Containment: `IfcRelContainedInSpatialStructure` to the base storey. Other storeys the column spans are recorded with `IfcRelReferencedInSpatialStructure`.
- Attachments to floors, roofs and beams are `IfcRelConnectsElements`.
- **RXD element data** (`constraints`) holds the attachment targets and justification, offsets, cut styles, extensions and the *Moves With Grids* lock. The location mark is computed, not stored.

**Commands.**
- `column.place(type_id, point, base_level, top_constraint, base_offset, top_offset, rotation?) -> element_id`
- `column.place_at_grids(type_id, grid_ids[], base_level, top_constraint, rotation?) -> element_id[]`
- `column.place_in_columns(type_id, arch_column_ids[]) -> element_id[]`
- `column.attach(column_ids[], end: top|base, target_id, style?, justification?, offset?) -> ()`
- `column.detach(column_ids[], end: top|base, target_ids?) -> ()`

**Acceptance criteria.**
- [ ] With *Moves With Grids* on, moving a grid moves the columns on it. With it off, the columns stay put.
- [ ] *At Grids* with 4 × 3 selected grids creates exactly 12 columns in one undo step.
- [ ] A column attached to a sloped floor follows the floor when the floor slope changes, for each of the four attachment justifications.
- [ ] Exported IFC validates against IFC4 ADD2 TC1, and the column carries `Pset_ColumnCommon.LoadBearing = TRUE`.

*Built (2026-10-16):* `column.place` (vertical structural columns; refuses architectural types with `column.not_structural_type`), `column.place_at_grids` (one column per crossing of the selected grids, in one undo step; a straight-line crossing test, with the grid's own extent), `column.location_mark` (read-only, for example "B-2", or "B-2(+600, -300)" in millimetres when off the crossing) and `column.load_section` / `column.check` (STR-SEC-001). Structural column types share the architectural column's `recipe.column_type` with `structural: true`, a material kind (concrete, steel, precast concrete, wood, other), a material name and a catalogue designation. Shapes: rectangular, round, I (W, IPE, HE, UB, UC), channel, angle, tee, rectangular hollow and circular hollow, with web, flange and wall thicknesses; the type's area and second moments are computed from the outline. The default type, *Concrete Column 400 x 400*, takes its concrete from the model's building code. A new **Structure** tab carries Column (`SC`), At Grids, Load Section, Attach Top and Detach Top, and the datums; box selection in plans now includes grid lines, so At Grids takes a window of grids. IFC: `IfcColumn` (`COLUMN`, `Pset_ColumnCommon.LoadBearing` true) with a faceted B-rep body (hollow sections keep their holes). Not yet: placement previews and Spacebar rotation to grid directions, *At Columns*, Moves With Grids, attaching bases, beams as attachment targets, cut styles and extensions, `IfcExtrudedAreaSolid` with `IfcMaterialProfileSetUsage`, analytical association, coarse stick symbols.

### 4.1.2 Slanted columns
**ID:** STR-COL-002 · **Priority:** P1 · **Ref:** research/03 §2.1

**Purpose.** Inclined columns, for example raking columns and V-columns.

**Workflow.** Choose *Slanted Column* on the contextual tab. In plan, give two clicks, each with its own level and offset (*First Click*, *Second Click*). In 3D or elevation, the clicks can snap to existing structural elements using **3D snapping**.

**Behaviour & rules.**
- The top end must always be higher than the base. If an edit would invert them, Reviera swaps the ends and reports a warning.
- **Column Style** is *Vertical*, *Slanted – End Point Driven* or *Slanted – Angle Driven*. Changing the style keeps the base point.
- Slanted columns are excluded from graphical column schedules (chapter 5) and from Copy/Monitor (chapter 6).
- In point-to-point steel mode (STR-BEAM-003), a slanted column runs to the full end of its system length.

**Data (IFC mapping).** `IfcColumn` with an `Axis` representation along the inclined line. **RXD element data** (`constraints`) holds the column style and the angle that drives it.

**Commands.** `column.place_slanted(type_id, p1, p1_level, p1_offset, p2, p2_level, p2_offset, style) -> element_id`

**Acceptance criteria.**
- [ ] Changing an angle-driven column from 60° to 75° moves the top point and keeps the base fixed.
- [ ] Dragging the base above the top produces a warning, and the column remains valid.

### 4.1.3 Beams (structural framing)
**ID:** STR-BEAM-001 · **Priority:** P1 · **Ref:** research/03 §2.2

**Purpose.** Horizontal, sloped or curved framing members. A **structural usage** value controls coarse line style, scheduling and analytical role.

**Workflow.**
1. Choose *Structure › Beam* (proposed `BM`).
2. On the contextual tab set the placement plane, structural usage (or *Automatic*), **3D Snapping**, **Chain** and Tag on Placement.
3. Click a start point and an end point; a typed length is accepted. Use an arc tool for curved beams.
4. *Multiple › On Grids* places one beam on each selected grid line between consecutive supports (columns, walls or beams).

**UI.** Contextual tab "Modify | Place Beam": draw tools (line, arc, pick lines), Multiple › On Grids, structural usage, 3D snapping, chain, tag. If a plan view's lower clip plane hides the new beam, Reviera shows a non-modal notice that offers to adjust the view range.

**Behaviour & rules.**
- **Automatic structural usage:**

| Supported by | Usage assigned |
|--------------|----------------|
| Walls or columns | Girder |
| Girders | Joist |
| Joists | Purlin |
| A brace, or a member framing into one | Other |
| Drawn as horizontal bracing | Horizontal Bracing |

- Users can edit and lock the usage. A locked usage is not recalculated.
- On sloped work planes, **Orientation** is *Normal* (the section stays perpendicular to the plane) or *Horizontal* (the section stays upright).
- **Cross-Section Rotation** turns the profile about the member axis.

**Data (IFC mapping).**
- `IfcBeam` with PredefinedType set from usage: `BEAM` with ObjectType "GIRDER" or "JOIST"; `LINTEL`; `HOLLOWCORE`; `T_BEAM`.
- Purlins and horizontal bracing are `IfcMember` (`PURLIN` or `BRACE`).
- Types are `IfcBeamType` and `IfcMemberType`.
- Property sets: `Pset_BeamCommon` (Span, Slope, Roll, LoadBearing) and `Qto_BeamBaseQuantities` (Length = cut length, CrossSectionArea, NetVolume, GrossWeight).
- **RXD element data** (`constraints`) holds the usage lock, start and end level offsets, orientation and stick-symbol location.

**Commands.**
- `beam.create(type_id, curve, reference_level, usage?, orientation?, z_justification?, y_justification?) -> element_id`
- `beam.create_on_grids(type_id, grid_ids[], level) -> element_id[]`
- `beam.set_usage(beam_ids[], usage, lock?)`

**Acceptance criteria.**
- [ ] A beam drawn between two columns gets Girder. A beam framing into that girder gets Joist. A beam framing into the joist gets Purlin.
- [ ] *On Grids* across 3 grid lines that each cross 4 columns creates 9 beams, one per bay.
- [ ] A purlin exports as `IfcMember` with PredefinedType `PURLIN`.

*Built (2026-10-16):* `beam.create` (a straight location line at a reference level with an offset at each end, so beams slope; the type defaults to the region's usual rolled beam: W12X26, IPE 300 or UB 305x165x40), `beam.create_on_grids` (a beam per bay between consecutive columns on each selected grid line that reach the level; one undo step), `beam.set_usage` (lock, or `automatic`), `beam.set`, `beam.load_section` and `beam.info` (usage, cut length, what each end rests on). Automatic usage follows the supports found from the model: an end inside a column or wall is supported by it; an end on another beam's side frames into it; on columns or walls a girder, on girders a joist, on joists a purlin, otherwise other. The class is chosen when the beam is made (purlins and horizontal bracing as `IfcMember`, others as `IfcBeam`), because a saved element cannot change class; later usage changes set PredefinedType and ObjectType within that class (`BEAM` + ObjectType `GIRDER`, `JOIST`, `PURLIN`, `BRACE`, or `USERDEFINED` with the usage as ObjectType). `Pset_BeamCommon` (or `Pset_MemberCommon`) carries LoadBearing and Span (the cut length). Orientation *normal* or *horizontal* (upright, cut plumb) and cross-section rotation are built. The Structure tab's Framing panel has Beam (`BM`; click a start and an end, chained, snapping to grid crossings), On Grids and Load Beam Section; Properties show the level, end offsets, orientation, rotation, justification, extensions, usage ("Automatic (Joist)"), supports and cut length. IFC: a faceted B-rep body contained in the reference level. Plan snapping gained *Intersection* (where two grid or wall lines cross). Not yet: arc beams, pick lines, 3D snapping, Tag on Placement, the view-range notice, horizontal bracing by drawing, walls and beams as On Grids supports, `IfcExtrudedAreaSolid` bodies, `Qto_BeamBaseQuantities`, and member types (`IfcMemberType`) for members.

### 4.1.4 Framing justification, extensions, cutback and joins
**ID:** STR-BEAM-002 · **Priority:** P1 · **Ref:** research/03 §2.2, §2.4

**Purpose.** Control precisely where framing geometry sits relative to its location line, and how members meet.

**Behaviour & rules.**
- **Justification:**
  - *yz Justification* is Uniform or Independent per end.
  - *y Justification* is Origin, Left, Centre or Right, with a y offset.
  - *z Justification* is Origin, Top, Centre or Bottom, with a z offset.
  - Lateral justification is also available.
  - The ribbon's **Justification Points** tool picks one of nine section points graphically, alongside y and z offset tools.
- **Extensions:** start and end extensions change the physical length without moving the analytical end. **Shape handles** do the same by dragging.
- **Cutback:** steel members get an automatic gap at joins, set per end. In joins between different materials, concrete takes precedence and cuts steel or wood.
- **Join editor** (*Beam/Column Joins*): arrows at each join choose which member runs through. Ends can be mitred or squared off, and **Disjoin** releases an end. **Change Reference** chooses which face of the joined member the end aligns to.
- **Openings** in framing come from face-based openings (chapter 3) or from family voids.

**Data (IFC mapping).**
- Joins are `IfcRelConnectsPathElements` (ConnectionType `ATSTART`, `ATEND` or `ATPATH`).
- Openings are `IfcOpeningElement` + `IfcRelVoidsElement`.
- Body geometry always reflects the resolved justification, cutback and extensions.
- **RXD element data** (`constraints`) holds justification, extensions, cutback, join priority, mitre and disjoin flags.

**Commands.**
- `framing.set_justification(ids[], y_just, y_offset, z_just, z_offset, mode)`
- `framing.set_extension(id, end, length)`
- `framing.set_cutback(id, end, distance)`
- `framing.join_edit(join_id, through_member_id?, miter?)`
- `framing.disjoin(id, end)`
- `framing.change_reference(id, end, reference)`

**Acceptance criteria.**
- [ ] z Justification = Top with a −50 mm offset puts the top of steel 50 mm below the location line in section.
- [ ] A concrete beam joined to a steel column cuts the column. Swapping the materials reverses the cut.
- [ ] Disjoining an end removes the cutback at that end only.

*Built (2026-10-16):* y justification (origin, left, centre, right; offset positive to the left) and z justification (origin, top, centre, bottom; offset positive up) for both ends together (`framing.set_justification`; *Independent* per end is not built); start and end extensions (`framing.set_extension`); automatic joins at supports: a joined end stops at the supporting column's or wall's face, or at the side of the beam it frames into, less its cutback (`framing.set_cutback`; 12.5 mm by default for steel members, none for others); `framing.disjoin` releases an end (no join, no cutback) and `framing.join` joins it again. The body always reflects justification, joins, cutback and extensions. Not yet: the join editor (`framing.join_edit`, mitres, which member runs through), material precedence (concrete cutting steel), `framing.change_reference`, shape handles, `IfcRelConnectsPathElements`, openings in framing.

### 4.1.5 Point-to-point steel modeling
**ID:** STR-BEAM-003 · **Priority:** P2 · **Ref:** research/03 §2.2 ("Point-to-point steel modeling")

**Purpose.** A project setting that makes steel members run exactly between the points the user clicked, so that system length equals cut length. This suits fabrication-first workflows.

**Behaviour & rules.** When *Structural Settings › Structural Steel Elements › Disable automatic shortening during auto-join* is on:
- Steel beams and braces are no longer shortened at joins.
- Start extension is locked, and start join cutback is removed.
- Weight is recalculated.
- *Change Reference* is unavailable.
- Columns are still shortened. Slanted columns run to the full end of their system length.

Turning the setting on or off regenerates every affected member in one transaction.

**Data (IFC mapping).** Geometry and `Qto_BeamBaseQuantities` reflect the result. **RXD document** `model/structural` (`settings.point_to_point_steel`).

**Commands.** `structural_settings.set(key="point_to_point_steel", value)`

**Acceptance criteria.**
- [ ] With the setting on, a steel beam clicked from (0,0) to (6000,0) and joined to columns at both ends reports Cut Length = 6000 mm.

### 4.1.6 Beam systems
**ID:** STR-BSYS-001 · **Priority:** P1 · **Ref:** research/03 §2.3

**Purpose.** A parametric host element that fills a bay with parallel beams according to a layout rule. The beams it creates are ordinary framing occurrences.

**Workflow (automatic).**
1. In a plan or ceiling view, choose *Structure › Beam System*.
2. Hover over a bay enclosed by supports (walls or beams). The bay highlights, and the preview aligns parallel to the nearest support. Curved supports can bound the bay but cannot set the direction.
3. Click to create. *Walls Define Slope* lets sloped walls drive the elevation.

**Workflow (sketch).** Choose *Sketch Beam System*. Draw with Line or Pick Lines, or use **Pick Supports**, which locks edges to their supports. Set the **Beam Direction** edge, then *Finish*.

**UI.** Contextual tab: Automatic/Sketch, Walls Define Slope, Tag on Placement (system tag or framing tag), Layout Rule. Edit-mode tab: Boundary, Beam Direction, Pick Supports, Finish/Cancel.

**Behaviour & rules.**
- **Layout rules:**

| Rule | Behaviour |
|------|-----------|
| Fixed Distance | Given centre spacing |
| Fixed Number | Given count |
| Maximum Spacing | Smallest count that keeps spacing at or below the value |
| Clear Spacing | Spacing measured between beam faces. Resizing a beam moves its neighbours. |

- **Justification** of the layout: Beginning, End, Centre or Direction Line.
- Other parameters: beam type, elevation, and **3D (non-planar)**, where sketch-line elevations drive the beam elevations.
- When supports move, a boundary locked to them regenerates, and the child beams are re-laid out.
- A system should not span a bay crossed by other beams. If it does, Reviera warns and suggests splitting the system.
- *Remove Beam System* deletes the host and keeps the beams as independent framing.
- Copying or mirroring a system copies its sketch. Support locks that cannot be resolved are released, with a warning.

**Data (IFC mapping).** `IfcElementAssembly` with PredefinedType `BEAM_GRID`, aggregating its child `IfcBeam`/`IfcMember` elements through `IfcRelAggregates`. **RXD element data** (`constraints`) holds the layout rule, spacing or count, justification, elevation, non-planar flag, boundary sketch, direction edge and support locks.

**Commands.**
- `beam_system.create_auto(view_id, pick_point, type_id, layout_rule, value, justification?) -> system_id`
- `beam_system.create_sketch(boundary_curves[], direction_edge, type_id, layout_rule, value, lock_to_supports?) -> system_id`
- `beam_system.set_layout(system_id, rule, value, justification)`
- `beam_system.remove(system_id) -> beam_ids[]`

**Acceptance criteria.**
- [ ] A 6 m × 9 m bay with Maximum Spacing 1.2 m produces the smallest beam count whose spacing does not exceed 1.2 m.
- [ ] Under Clear Spacing, changing one beam to a wider section moves its neighbours so that clear spacing stays the same.
- [ ] Moving a support beam 300 mm moves the locked boundary edge and re-lays out the children in one regeneration.
- [ ] *Remove Beam System* leaves all child beams in place and editable.

### 4.1.7 Braces
**ID:** STR-BRACE-001 · **Priority:** P1 · **Ref:** research/03 §2.4

**Purpose.** Diagonal framing that joins to columns and beams.

**Workflow.**
- **In a framing elevation:** choose *Structure › Brace* (proposed `BR`), snap the start to a column or beam, then snap the end.
- **In plan:** set start level and offset and end level and offset on the contextual tab, then click two points.
- **Chain** (contextual tab toggle, off by default) starts the next brace at the end of the previous one, for K and chevron patterns. **Esc** ends the chain; a second **Esc** leaves the tool.

**UI.** Contextual tab "Modify | Place Brace": Type Selector, Start Level/Offset, End Level/Offset (plan only), Chain, 3D Snapping. Properties: reference level, start and end attachment (the member and distance or ratio along it), cross-section rotation, structural material, structural usage (read-only) and the analytical association (read-only).

**Behaviour & rules.**
- Braces share the framing rules for justification, cutback, extensions and releases (STR-BEAM-002).
- **Attachment.** An end snapped to a column or beam stores the target and the position along it, as a distance from the target's start or as a ratio (Properties choose which). The end follows the target when it moves, stretches or changes level; if the target is deleted the end stays where it was, becomes free, and a warning names the brace.
- **Usage.** Usage is *Other*, or *Horizontal Bracing* when both ends lie in one horizontal plane within 1 mm. It is recomputed after each edit and cannot be overridden (unlike beams).
- At Coarse detail a brace in a plan view is drawn with the brace plan symbol set in Structural Settings (STR-SET-001): parallel line offset, or none. Kicker braces (one end on a beam mid-span) use the kicker symbol.
- A zero-length brace (start and end within 1 mm) is refused with an error.
- Braces are valid rebar hosts when their material is concrete (STR-RBR-001).
- Interactive placement journals each pick (start, end, chain toggle) as a micro-operation so that an unfinished chain is recovered after a crash (§2.1 of the conventions); replay re-runs the picks in order.

**Data (IFC mapping).** `IfcMember` with PredefinedType `BRACE`, typed by `IfcMemberType` (`BRACE`). `Pset_MemberCommon` (Reference, LoadBearing = TRUE, IsExternal, Slope). Axis representation as an `IfcPolyline` "Axis" item plus a swept body. Connections to the attached members: `IfcRelConnectsPathElements` (`ATSTART`/`ATEND`). End attachments (target GlobalId, distance or ratio): RXD element data (`constraints`).

**Commands.**
- `brace.create(type_id, start_point, start_level, start_offset, end_point, end_level, end_offset) -> element_id`

Justification, extension, cutback and join edits use the `framing.*` commands of STR-BEAM-002.

**Acceptance criteria.**
- [ ] A brace snapped to two column–beam intersections in an elevation stays joined when one column moves.
- [ ] A brace exports as `IfcMember` (`BRACE`).
- [ ] A brace attached at ratio 0.5 along a 6 m beam stays at mid-span when the beam is stretched to 8 m.
- [ ] A brace drawn in plan with both ends at Level 2 + 0 shows usage *Horizontal Bracing*; moving one end to Level 3 changes it to *Other*.
- [ ] Deleting the column at one end leaves the brace in place with a free end and one warning that names it.

### 4.1.8 Trusses
**ID:** STR-TRUSS-001 · **Priority:** P1 · **Ref:** research/03 §2.5

**Purpose.** A truss family defines a **layout** of top chord, bottom chord, vertical web and diagonal web lines. Every type in the family shares that layout but assigns its own framing type to each member group.

**Workflow.** Choose *Structure › Truss*, then draw a line or pick a line from start to end. The layout stretches to fit the span.

**UI.**
- Contextual "Modify | Truss" tab: *Edit Profile* (top and bottom chord shapes), *Attach Top/Bottom* (to a roof or structural floor), *Detach*, *Remove Truss*, *Edit Family*.
- When selected, the truss shows as dashed lines. Its members can be selected individually with Tab.
- A one-click action tags all members.

**Behaviour & rules.**
- **Type parameters per group** (top chords, bottom chords, vertical webs, diagonal webs): framing type, start and end release (Pinned, Fixed, Moment), and angle. Chords also carry an analytical vertical projection.
- **Construction parameters:** webs have symbolic cutback, web orientation, bearing chord (top or bottom), truss height and non-bearing offset.
- A member's type can be changed individually, but only within the sizes the truss family allows.
- *Remove Truss* deletes the truss host and keeps the members as plain framing.

**Data (IFC mapping).** `IfcElementAssembly` with PredefinedType `TRUSS`, aggregating `IfcMember` elements: `CHORD` for chords, and `STRUT` or `MEMBER` for webs. The **RXD family recipe** holds the truss layout recipe. RXD element data (`constraints`) holds attachments and profile edits.

**Commands.**
- `truss.create(type_id, start, end, level) -> truss_id`
- `truss.edit_profile(truss_id, top_curves[], bottom_curves[])`
- `truss.attach(truss_id, chord: "top"|"bottom", target_id)`
- `truss.detach(truss_id, chord)`
- `truss.remove(truss_id) -> member_ids[]`

**Acceptance criteria.**
- [ ] A Pratt truss drawn over 12 m and then stretched to 15 m keeps its panel count and re-proportions its webs.
- [ ] Attaching the top chord to a sloped roof regenerates the chords and webs to the roof underside.
- [ ] The export contains one `IfcElementAssembly` (`TRUSS`) that aggregates all chords and webs.

### 4.1.9 Structural walls
**ID:** STR-WALL-001 · **Priority:** P1 · **Ref:** research/03 §2.6

**Purpose.** Load-bearing and shear walls, built on the architectural wall engine (chapter 3, `ARC-WALL-*`).

**Workflow.** *Structure › Wall › Wall: Structural* offers the same options as the architectural wall tool (level, depth or height, location line, chain, offset, radius, join status), with *Structural* set to true.

**Behaviour & rules.**
- **Structural Usage** is Bearing, Shear, Structural Combined or Non-bearing.
- A wall is a valid rebar host when its usage is not Non-bearing and it has a layer whose material behaviour is concrete or precast concrete.
- Analytical automation does not create panels for **slanted structural walls**. Reviera warns when it skips one, and a panel can still be modelled by hand.
- A wall foundation (STR-FND-002) can be attached beneath.

**Data (IFC mapping).** `IfcWall` with PredefinedType `SHEAR` (shear usage) or `SOLIDWALL`, and `Pset_WallCommon.LoadBearing` (TRUE except for Non-bearing). **RXD element data** (`structural`) keeps the exact usage value where IFC cannot tell the difference, for example "Structural Combined".

**Commands.** `wall.create(...)` (chapter 3) with `structural=true, structural_usage`; `wall.set_structural_usage(ids[], usage)`

**Acceptance criteria.**
- [ ] A concrete Bearing wall offers the Rebar tool on its contextual tab. After the change to Non-bearing it no longer does.
- [ ] Bearing walls export with `LoadBearing = TRUE`.

### 4.1.10 Structural floors and composite decks
**ID:** STR-FLOOR-001 · **Priority:** P1 · **Ref:** research/03 §2.7

**Purpose.** Structural slabs with a span direction, optionally on a profiled metal deck.

**Workflow.**
1. Choose *Structure › Floor › Floor: Structural*.
2. Sketch a closed boundary with Boundary Line, Pick Walls or **Pick Supports**.
3. Set the **Span Direction** edge. A span symbol is placed.
4. Optionally use *Extend into wall (to core)*. Finish.

**Behaviour & rules.**
- **Composite deck:** a floor type can include one **Structural Deck** layer. That layer references a *deck profile* family and a *deck usage*:
  - *Bound Layer Above/Below*: concrete fills the deck ribs
  - *Standalone Deck*: the deck is used on its own
- The deck profile is drawn at Medium and Fine detail in sections cut perpendicular to the span.
- **Sub-tools**, shared with chapter 3 where they apply: slab edges, thickened slabs, drop panels, cantilevers, openings, stiffeners at openings, sloped floors (slope arrow) and the span symbol.
- A concrete structural floor is a valid rebar host.

**Data (IFC mapping).**
- `IfcSlab` (`FLOOR`). Layers: `IfcMaterialLayerSetUsage`. The deck layer is an `IfcMaterialLayer` with Category "StructuralDeck".
- The span direction is also expressed through the layer set's reference direction.
- The **RXD family recipe** holds the deck profile recipe and deck usage on the floor type. RXD element data (`constraints`) holds the span-direction edge and sketch.

**Commands.**
- `floor.create(type_id, loops, level_id, offset?, slope?: {line_key|curve, slope} | {arrow: SlopeArrow}, structural?, span_direction_edge?, room_bounding?, attach_walls_below?: bool, join_overlapping_walls?: bool) -> element_id`
- `floor.set_span_direction(floor_id, edge_index | vector)`
- `floor_type.set_deck_layer(type_id, layer_index, profile_family_id, usage)`

**Acceptance criteria.**
- [ ] A composite floor cut perpendicular to its span shows deck ribs at Fine detail and a plain layer at Coarse detail.
- [ ] Rotating the span direction by 90° changes which sections show the rib profile.

### 4.1.11 Foundations
**ID:** STR-FND-001 / 002 / 003 / 004 · **Priority:** P1 (001–003), P2 (004) · **Ref:** research/03 §2.8

| ID | Feature | Workflow | Behaviour & rules | IFC / RXD | Commands |
|----|---------|----------|-------------------|---------------|----------|
| STR-FND-001 | **Isolated footing** | *Foundation › Isolated*: click in plan or 3D. Also *Multiple › At Grids* and *At Columns*. | Joins to the column base above. A read-only **Elevation at Bottom** is available to tags. A foundation tag that shows elevation is supplied. | `IfcFooting` `PAD_FOOTING` (or `PILE_CAP` when the family declares piles), `IfcFootingType`, `Pset_FootingCommon` | `footing.place(type_id, point, level, offset)`, `footing.place_at_columns(type_id, column_ids[])`, `footing.place_at_grids(type_id, grid_ids[])` |
| STR-FND-002 | **Wall foundation** | *Foundation › Wall*: pick structural walls. | Type kind: **Retaining** (toe length, heel length) or **Bearing** (width). Other parameters: thickness, default end extension and **Do Not Break At Inserts**. Follows the wall's length and its openings. | `IfcFooting` `STRIP_FOOTING`; RXD family recipe (type kind and dimensions) and RXD element data `constraints` (wall host) | `wall_foundation.create(type_id, wall_ids[])` |
| STR-FND-003 | **Foundation slab** | *Foundation › Slab*: sketch one closed outer loop (plus inner loops for openings), or pick walls to generate the boundary. Offset from the level, and *extend into wall* (to the wall core) when picking walls. | Its top face sits at the level plus offset and the slab extends **below** it, so it is visible only in views whose range reaches below the level. When no plan of that level has a view depth reaching the slab, the tool offers to create a foundation plan. Has no span direction. Thickness comes from the type's layers. Joins to wall foundations and isolated footings it overlaps (the slab is cut). Valid rebar host (STR-RBR-001). An open or self-intersecting loop is refused with the offending segment highlighted. | `IfcSlab` `BASESLAB`, `IfcSlabType`, `Pset_SlabCommon`; the editable boundary sketch is RXD element data (`constraints`) | `foundation_slab.create(type_id, boundary, level, offset)` |
| STR-FND-004 | **Piles and pile caps** | Place a pile-cap family with nested piles, or a single pile family. | Pile families declare count, length, diameter and kind (bored or driven). Each pile is a separate nested occurrence. This goes beyond the reference's generic foundation families, to give clean IFC. | `IfcPile` (PredefinedType `BORED`, `DRIVEN`, …), `IfcPileType`, `Pset_PileCommon`, aggregated under the cap's `IfcFooting` `PILE_CAP` | `pile.place(type_id, point, level, length)`, `pile_cap.place(type_id, point, level)` |

**Acceptance criteria.**
- [ ] *At Columns* with 20 selected columns places 20 isolated footings centred under them.
- [ ] A retaining wall foundation with Toe 600 mm and Heel 1200 mm shows the correct asymmetric section.
- [ ] A foundation slab at Level 1 is not visible in a Level 1 plan whose view depth ends at the level, and is visible in the generated foundation plan.
- [ ] A four-pile cap exports one `IfcFooting` (`PILE_CAP`) that aggregates four `IfcPile` elements.

### 4.1.12 Structural connections (information carriers)
**ID:** STR-CONN-001 · **Priority:** P1 (generic) / P2 (detailed types, code checking) · **Ref:** research/03 §2.9 · screenshots/structure_mep/structural_connection_symbols.png

**Purpose.** An element placed at a join of two or more framing members. It carries the information engineers, detailers and fabricators exchange about that join: connection type, connected members, geometry, design forces, materials, fabricator, code-check status, approval status and phase.

**Workflow.**
1. Choose *Structure › Connection* or *Steel › Connection*.
2. Pick the members with Ctrl+click or a window, then press **Enter** or **Spacebar**.
3. If members were already selected, the generic connection is placed at once, or the last-used type.

**UI.**
- A generic connection is drawn as a symbolic circle with spokes at Coarse and Medium detail.
- With a generic connection selected, the Type Selector lists **recommended types** first. They are filtered by member count, member shapes, member positions and which member is the main member.
- Properties: *Override by Instance*, Approval Status, Code Check Status, Fabricator, Design Forces (per load case or combination).

**Behaviour & rules.**
- Detailed connection types come from the **connection library** (STR-STL-002). Library types offer *Modify Parameters* (a type editor with live preview) and *Ranges of Applicability* (rules used by automatic placement).
- **Sub-connections** come from parametric cuts (STR-STL-008). They cannot be duplicated or retyped.
- *Override by Instance* unlocks per-instance detail parameters and the code-check tab.
- Other tools:
  - **Propagate**: copy a connection to joins that match it geometrically
  - **Break**: turn the connection into independent plates, bolts and welds
  - **Customize**: see STR-STL-003
  - **Connection Library**: load and unload types, filtered by group
- Unloading a type **deletes its instances** after a confirmation that states how many are affected.

**Connection warnings in views.** A connection whose generator cannot produce valid geometry (unsupported section shape, wrong input order, parameters that do not fit the members, a member moved out of the type's ranges of applicability) keeps its data, shows its symbol only, and gets a **status** of *Failed* with a reason code and a one-line explanation.
- *Connection › Show Warnings* is a per-view toggle (off by default, stored with the view). When on, each failed connection in the view is drawn with a warning marker (a triangle at the join, in the view's warning colour) and a tooltip that gives the reason and the input order the type expects.
- Clicking the marker selects the connection; the Properties palette shows the reason and offers *Modify Parameters*. The same failures are always listed in the project warnings list (chapter 1), whether or not the toggle is on.
- The toggle changes display only. It is not undoable and does not mark the model as changed.

**Data (IFC mapping).**
- `IfcElementAssembly` (PredefinedType `USERDEFINED`, ObjectType "STRUCTURAL_CONNECTION") aggregates the connection's plates, fasteners and cuts.
- Connected members are related with `IfcRelConnectsWithRealizingElements`: the RelatingElement is the main member, the RelatedElement is a secondary member, and the realizing elements are the plates and fasteners.
- The analytical counterpart is an `IfcStructuralPointConnection` (§4.3).
- **RXD element data** (`structural`) holds the connection type reference, approval status, code-check status and report path, fabricator, instance-override flag, main member and design-force references. All references are GlobalIds.

**Commands.**
- `structural_connection.create(member_ids[], type_id?) -> connection_id`
- `structural_connection.set_type(connection_id, type_id)`
- `structural_connection.propagate(connection_id, target_join_ids[] | "matching") -> connection_ids[]`
- `structural_connection.break(connection_id) -> element_ids[]`
- `connection_library.load(type_ids[])`
- `connection_library.unload(type_ids[])`
- `structural_connection.show_warnings(view_id, on: bool)`

**Acceptance criteria.**
- [ ] Selecting a beam and a column and running the tool creates one generic connection symbol at the join.
- [ ] For a beam-to-column-flange join, the Type Selector lists shear-plate and end-plate types ahead of base plates.
- [ ] Unloading a type used by 7 connections asks for confirmation, states the count, and then removes all 7.
- [ ] With *Show Warnings* on, an end-plate connection whose beam is deeper than the plate range shows a warning marker whose tooltip names the failing parameter; turning the toggle off removes the marker but the warning stays in the warnings list, and the undo stack is unchanged.

### 4.1.13 Structural settings and symbolic representation
**ID:** STR-SET-001 · **Priority:** P1 · **Ref:** research/03 §1.6, §15.5–15.6 · web: Analytical Model Settings

**Purpose.** Project-wide structural settings, opened from *Manage › Structural Settings* or the Structure panel launcher.

**UI: the Structural Settings dialog.** A modal, tree-structured dialog (§4.0.3). The left pane lists the pages below; the right pane edits the selected page. *OK* applies all pages in **one transaction** ("Structural Settings"); *Cancel* discards; *Apply* commits and keeps the dialog open. `structural_settings.show(page?)` opens it on a given page: `structural` (symbolic representation and steel), `connection`, `analytical`, `boundary_conditions` or `load_display`.

| Page | What it lists and edits | Validation |
|------|-------------------------|------------|
| **Symbolic Representation** (`structural`) | Brace symbol in plan (parallel line with offset, or none) and its offset; kicker brace symbol; coarse cutback for beams, braces and trusses (default 3 mm, 1/8"); plan symbol for vertical braces above and below the cut plane | Offsets and cutbacks 0–100 mm; negative values refused |
| **Connection** (`connection`) | Connection symbol family per connection type (generic, and each loaded library type); default for new types | The symbol must be a loaded annotation family of the connection-symbol kind |
| **Structural Steel Elements** (`structural`) | Point-to-point mode (STR-BEAM-003): turn off automatic shortening of steel members when they auto-join | — |
| **Load Cases / Load Combinations** | Read-only summaries with a button that opens the manager (STR-LC-001) on the matching tab | — |
| **Analytical Model** (`analytical`) | **Automatic checks** on or off: *Member supports* and *Analytical/physical consistency* (both off by default). **Tolerances**: support distance (default 300 mm), analytical-to-physical distance (default 150 mm), node merge distance (default 5 mm), level and grid alignment distance used by automation (default 300 mm). **Consistency criteria** to include, each a check box (STR-AM-007). *Circular support references* check on or off | Every tolerance > 0 and ≤ 2 000 mm; node merge distance must be smaller than support distance |
| **Boundary Conditions** (`boundary_conditions`) | Symbol family per state (Fixed, Pinned, Roller, User) for point, line and area conditions; spacing of line and area symbols (default 1 000 mm) | Spacing > 0; family must be of the matching boundary-condition symbol kind |
| **Loads Scaling Display** (`load_display`) | Arrow length per force unit (for example 10 mm per 10 kN), per linear force, per area force and per moment unit; whether values are labelled | Scale values > 0 |

Each row in a page shows whether the value differs from the project template default, and *Reset page* restores the template values.

**Behaviour & rules.**
- Changes to symbolic settings regenerate Coarse views on commit; changes to tolerances do not move any element. They take effect on the next check or automation run.
- Turning an automatic check on runs it once over the whole model and then after every transaction that touches structural or analytical elements (STR-AM-007).
- The dialog is available in BIMserver client mode; settings are part of the model and follow check-in.

**Data (IFC mapping).** No IFC equivalent. **RXD document** `model/structural` (`settings.*`), with symbol families referenced by type GlobalId. Analytical tolerances, check switches and load display scale go in RXD document `model/analysis`.

**Commands.** `structural_settings.get() -> table`, `structural_settings.set(key, value)`, `structural_settings.show(page?)`. `set` applies the same validation as the dialog and returns an error naming the key and the allowed range.

**Acceptance criteria.**
- [ ] Changing the coarse cutback from 3 mm to 10 mm updates all coarse plan views on the next regeneration.
- [ ] Settings survive a save and reopen of the `.ifc` with its RXD data, and a BIMserver check-in and get-latest round trip.
- [ ] `structural_settings.show("analytical")` opens the dialog on the Analytical Model page; entering a node merge distance of 400 mm with a support distance of 300 mm disables *OK* and shows the rule.
- [ ] Changing three pages and pressing *OK* creates one undo item; undo restores all three.

### 4.1.14 Section catalogues, materials and regional standards
**ID:** STR-SEC-001 · **Priority:** P1 · **Ref:** research/08 (S0720–S0822)

**Purpose.** Structural types start from the standard sections, materials and design standards that apply where the building is, so users pick a W10X49 in Chicago, an HEB 300 in Munich or a 254 x 254 x 73 UC in Leeds without typing dimensions.

**Behaviour & rules.**
- Reviera ships open **section catalogues** as data files, one per family and standard: AISC W, C, L, WT, rectangular and round HSS and standard pipe (ASTM A6, A500, A53); Euronorm IPE, HEA and HEB (EN 10365); UK UB and UC (BS 4-1). Each lists nominal dimensions only (depth, flange width, web and flange thickness; or height, width and wall; or outside diameter and wall), with its standard, units, shape, default material and sources. Reviera computes area, second moments and radii of gyration from the outline (root fillets are left out, so values are slightly below published ones). Catalogues are small factual subsets; publishers' property tables are not copied.
- The **region** follows the model's building code (§1.x code settings): United States (IBC/IRC), Canada (NBC), Europe (DIN and custom codes), United Kingdom (Approved Documents) or Australia (NCC). It sets the preferred catalogues (listed first), the default steel grade and concrete class (for example ASTM A992 and f'c 4000 psi; S355 and C30/37; CSA 350W and 30 MPa; 300PLUS and 32 MPa), and the design standards named for loads, steel, concrete and timber (ASCE/SEI 7, AISC 360, ACI 318, NDS; NBC, CSA S16, A23.3, O86; EN 1990–1995; AS/NZS 1170, AS 4100, AS 3600, AS 1720.1).
- *Load Section* makes a structural column type from a catalogue designation, named for it, with the catalogue's default material unless another is given. Loading the same designation and material again reuses the type.
- **Advisory checks** (`column.check`) report slenderness against simple code limits: steel KL/r above 200 (AISC 360 user note), concrete k·lu/r above 22 for unbraced columns in the US and Canada (ACI 318), and the slenderness λ for Eurocode regions (EN 1992-1-1 §5.8.3.1, whose limit needs the design load). They are prompts for the engineer, not design checks; design checks belong to analysis providers (§4.0.2a).

**Data.** Catalogues are compiled into Reviera (`crates/reviera-core/data/sections/*.json`, with a provenance README). Types record their catalogue designation and material on the `recipe.column_type`; IFC carries the material name.

**Commands.**
- `structural.catalogs(catalog?) -> [{id, family, standard, units, shape, default_material, preferred, designations[]}]`
- `structural.standards() -> {region, catalogs[], steel, concrete, loads, steel_standard, concrete_standard, timber_standard}`
- `column.load_section(catalog, designation, material?) -> type_id`
- `column.check(column_ids?) -> [{column_id, length, slenderness, limit?, note}]`

**Acceptance criteria.**
- [x] Every catalogue section makes a valid type; HEB 300 and W8X24 have their published dimensions, and HEB 300's computed Ix is within 5 % of the published value.
- [x] A US model lists the AISC catalogues first and loads W shapes in ASTM A992; a DIN model lists HEB, HEA and IPE first with S355 and C30/37.
- [x] In a US model a Pipe 2 STD column 12 m tall exceeds KL/r 200 and a W10X49 4 m tall does not; a 400 × 400 concrete column 12 m tall exceeds k·lu/r 22. In a DIN model the same column reports λ with no limit, citing EN 1992-1-1.
- [ ] Timber sections (NIST PS 20 dressed sizes, EN 338 and glulam classes) and a materials library with strengths.

---

## 4.2 Reinforcement (rebar)

Every reinforcement feature in this section is **P2**, as the priority tiers require, unless stated otherwise. Rebar tools live on the **Concrete** tab. They also appear on the contextual tab of any selected valid host.

### 4.2.1 Rebar hosts
**ID:** STR-RBR-001 · **Priority:** P2 · **Ref:** research/03 §3.1

**Purpose.** Decide which elements can contain reinforcement.

**Behaviour & rules.**
- **Valid hosts:** structural framing, structural columns, foundations, structural connections, floors, walls, foundation slabs, wall foundations, slab edges and parts cut from concrete walls or floors.
- Generic-model families become valid hosts when their family flag *Can Host Rebar* is set.
- **Material condition:** the host material's *model behaviour* must be Concrete or Precast Concrete. Walls and floors must also be structural and contain a concrete layer.
- If a host stops qualifying, its rebar is kept but flagged as **orphaned**, with a warning. A host can stop qualifying through a material change or through becoming non-structural. The user can re-host the rebar or delete it.
- Rebar shall never be silently deleted, except when its host is deleted. In that case the delete cascades and the count is shown in the confirmation.

**Data (IFC mapping).** Rebar elements are related to their host with `IfcRelAggregates`, where the host is the whole and the reinforcement is a part. The IFC4 reference-view convention also accepts `IfcRelContainedInSpatialStructure`. Reviera writes `IfcRelAggregates` and also records the host GlobalId in RXD element data (`rebar`), so that the link survives round trips through tools that drop aggregates. Host eligibility is computed and is not stored.

**Commands.** `rebar.can_host(element_id) -> bool`, `rebar.rehost(rebar_ids[], host_id)`

**Acceptance criteria.**
- [ ] A floor whose concrete layer is changed to timber flags its rebar as orphaned and keeps it in place.
- [ ] Deleting a host column with 3 rebar sets shows "3 reinforcement elements will be deleted" and removes them in the same transaction.

### 4.2.2 Reinforcement settings, rounding and cover
**ID:** STR-RBR-002 · **Priority:** P2 · **Ref:** research/03 §3.1

**Purpose.** Project-level rules for reinforcement.

**Behaviour & rules.**
- **Area and path reinforcement produce real rebar** (one mode per project).
  - When the mode is on, area and path systems generate individual rebar sets. These can be scheduled and edited, and they survive removal of the system.
  - When it is off, the bars are internal to the system.
  - The mode is **locked** once any area or path system exists.
- **Hooks in shape definition** (on or off). This must be decided before the first rebar is placed and is then locked.
- **Presentation defaults** for new sets, and abbreviations for area and path tags.
- **Rounding:** on or off. Method is Nearest, Up or Down. Separate increments apply to bar length, segment length and fabric-sheet dimensions. Bar types can override the project rounding.
- The Properties palette shows rounded values as "exact [rounded]".
- **Cover:** *Cover* tool. Pick elements or faces, then choose a named **cover setting**. The *Cover Settings* dialog creates, duplicates and deletes named cover types with offsets. Each host face stores its cover type, so cover can differ per face.

**UI: Reinforcement Settings dialog** (*Concrete › Settings › Reinforcement Settings*, `reinforcement_settings.show()`). A tree dialog with four pages; *OK* commits all pages as one transaction.
- **General:** *Area and path reinforcement produce rebar* and *Include hooks in shape definition*. Each is greyed out, with the reason in a tooltip, once it is locked (an area or path system exists; any rebar exists).
- **Presentation:** default set presentation for new sets in plan, section and 3D views (Show All, First/Last, Middle), and the default for spliced sets.
- **Abbreviations:** tag abbreviations for area and path reinforcement layers (for example "T1", "B2", "EF") and for the direction labels. Each abbreviation 1–8 characters, unique per layer.
- **Rounding:** on or off; method (Nearest, Up, Down); increments for bar length, segment length and fabric-sheet dimensions (default 1 mm, 5 mm and 5 mm). Increments must be > 0 and ≤ 100 mm. Bar types with their own rounding are listed read-only with a link to the type.

**UI: Rebar Cover Settings dialog** (*Concrete › Settings › Cover Settings*, `rebar_cover_type.manage()`). A grid with one row per cover type: name, offset and a usage count (how many host faces use it).
- *Add* creates a row named "New cover N" with offset 25 mm; *Duplicate* copies the selected row; *Delete* is allowed only for unused types, or after picking a replacement type that all its faces are moved to.
- Names must be unique and non-empty. Offsets must be ≥ 0 and ≤ 500 mm. Changing an offset regenerates every rebar constrained to faces using that type, and the dialog states how many hosts will update before *OK*.
- New projects get the template's cover types; a project always has at least one.

**Data (IFC mapping).**
No IFC equivalent.
- **RXD document** `rebar`: settings (HostRebarInAreaPath, HooksInShape, Rounding.*, Presentation.*) and the cover-type table (name, offset).
- **RXD element data** (`rebar`) on each host: per-face cover assignments (Top, Bottom, Exterior, Interior, Other as cover-type references, with per-face overrides keyed by stable face IDs).

**Commands.**
- `reinforcement_settings.set(key, value)`
- `reinforcement_settings.show()`
- `rebar_cover_type.create(name, offset) -> id`
- `rebar_cover_type.manage()`
- `rebar_cover.assign(host_id, face_ref? , cover_type_id)`

**Acceptance criteria.**
- [ ] With Nearest/10 mm rounding, a computed bar length of 2 437 mm displays as "2437 [2440]", and schedules use 2 440 mm.
- [ ] Trying to change the area/path mode after an area system exists is refused, with an explanation.
- [ ] Deleting a cover type used by 12 faces asks for a replacement type; after *OK* the 12 faces use the replacement and their rebar regenerates in the same transaction.
- [ ] Changing the offset of cover type "Interior 25" to 30 mm moves every bar constrained to faces of that type by 5 mm.

### 4.2.3 Standard (shape-driven) rebar placement
**ID:** STR-RBR-003 · **Priority:** P2 · **Ref:** research/03 §3.2

**Purpose.** Place planar or multi-planar bars from a library of shape families into a host.

**Workflow.**
1. Choose *Concrete › Rebar*, or pick it from a selected host's contextual tab.
2. Pick a shape on the contextual tab or in the **Shape Browser** palette (thumbnails of the loaded shapes).
3. Choose a **placement method**:
   - **Expand to Host:** the shape stretches to fill the host's cover box in the placement plane.
   - **By Two Points:** draw a rectangle, aligned to the view or to a picked reference, that the shape fills. This method also places cranked bars.
   - **Sketch:** draw the bar's centreline (STR-RBR-007).
   - **Free Form:** see STR-RBR-013.
4. Choose a **placement plane**: current work plane, near cover reference or far cover reference.
5. Choose an **orientation**: parallel to work plane, parallel to cover or perpendicular to cover. By Two Points adds Horizontal, Vertical and Perpendicular Segment. The `[` and `]` keys cycle orientations.
6. Choose the **set layout** (STR-RBR-004) and the set type, standard or **varying**. Click to place.

**UI.** Contextual tab "Modify | Place Rebar" with panels Placement Method, Placement Plane, Placement Orientation, Rebar Set, Set Type, Family (Load Shapes) and Show Tooltip. The optional cursor tooltip lists the keys for rotate, flip and align. *Multi-planar shapes* such as stirrups with legs in two planes are placed in section views and offer extra orientations.

**Behaviour & rules.**
- Placement respects host cover. If a bar cannot fit, Reviera refuses it and explains which dimension failed.
- New bars are visible in the placement view and in all section views, and hidden in other views by default (STR-RBR-017).

**Data (IFC mapping).**
- `IfcReinforcingBar`. PredefinedType comes from the shape style and role: `MAIN` for main bars, `LIGATURE` for stirrups and ties, `SHEAR`, `EDGE`, `RING`, `ANCHORING`, `SPACEBAR` or `PUNCHING`.
- The type is `IfcReinforcingBarType`, with NominalDiameter, CrossSectionArea, BarLength, BarSurface (`PLAIN` or `TEXTURED`), **BendingShapeCode** and **BendingParameters** (the A, B, C… values, in order).
- Body geometry: an `IfcSweptDiskSolid` along the bar centreline, one item per bar in the set, using `IfcMappedItem` for identical bars.
- **RXD element data** (`rebar`) holds the layout recipe and identity: HostId, ShapeFamilyId, PlacementMethod, LayoutRule, Quantity, Spacing, ArrayLength, Partition, RebarNumber, Style (Standard or StirrupTie), StirrupTieAttachment, HookStart, HookEnd, HookRotation, EndTreatment, WorkshopInstructions (Bent or Straight) and OverrideHookLengths.
- Mass and total length: a standard quantity set where the schema defines one. Otherwise they are computed on load and not stored.

**Commands.**
- `rebar.place(host_id, shape_id, bar_type_id, method: "expand"|"two_points"|"sketch", plane, orientation, points?, layout?) -> rebar_id`
- `rebar_shape_browser.show()`

**Acceptance criteria.**
- [ ] Expand-to-Host of a closed stirrup shape in a 300 × 600 column section with 40 mm cover produces a stirrup whose outer dimensions equal the host dimensions minus twice the cover.
- [ ] A placed bar exports as `IfcReinforcingBar` with its type's `BendingShapeCode` equal to the shape code of the shape family.

### 4.2.4 Rebar set layouts
**ID:** STR-RBR-004 · **Priority:** P2 · **Ref:** research/03 §3.2 ("Rebar set layout rules")

**Purpose.** Distribute identical bars along a direction.

**Behaviour & rules.**

| Rule | Input | Behaviour |
|------|-------|-----------|
| Single | — | One bar |
| Fixed Number | n | n bars spread evenly over the array length |
| Maximum Spacing | s | Smallest count that keeps centre spacing at or below s |
| Number with Spacing | n, s | n bars at spacing s; the array length follows |
| Minimum Clear Spacing | c | Clear gap at least c; stays valid when bar size changes |
| Custom Spacing | formula | Segment list such as `3x100; ?x200; 5x100`. `?` marks the one variable segment that absorbs the remaining length. A second syntax, `1400@100; 900@300`, gives distance at spacing. |

- Sets are tagged and scheduled as **one object** with quantity and spacing.
- Formula errors are reported inline: a non-numeric value, more than one `?`, or a total longer than the array length.
- Individual bars can be hidden or removed and later reset (STR-RBR-006).

**Data (IFC mapping).** The IFC body carries one swept solid per bar, so other tools see the true count. **RXD element data** (`rebar`) holds the layout recipe: LayoutRule, Quantity, Spacing, ArrayLength and CustomSpacingFormula.

**Commands.** `rebar.set_layout(rebar_id, rule, n?, spacing?, clear?, formula?)`

**Acceptance criteria.**
- [ ] `3x100; ?x200; 5x100` over 2 000 mm yields 3 spaces of 100 mm, then 6 of 200 mm, then 5 of 100 mm. That is 14 spaces filling 300 + 1 200 + 500 = 2 000 mm, so 15 bars, with the first bar at 0.
- [ ] With Minimum Clear Spacing 50 mm, changing the bar diameter from 12 to 20 mm reduces the count where needed so the clear gap stays at least 50 mm.

### 4.2.5 Varying rebar sets
**ID:** STR-RBR-005 · **Priority:** P2 · **Ref:** research/03 §3.2 ("Varying rebar set")

**Purpose.** Sets whose bar lengths change across inclined or skewed straight host faces.

**Workflow.**
1. Start *Rebar* (STR-RBR-003) and choose **Varying** in the *Set Type* panel before placing; or select an existing standard set and choose *Convert to Varying* on its contextual tab.
2. Each bar in the set is re-fitted to the host cover faces at its own position along the array, so bar segments lengthen or shorten with the taper.

**Behaviour & rules.**
- Only straight inclined faces are supported, not arcs. Reviera explains the refusal when a face is curved.
- An existing standard set can be converted to a varying set, and back where the geometry allows. *Convert to Standard* is refused, naming the bars that differ, when the bars no longer share one length within the rounding increment (STR-RBR-002).
- Every layout rule of STR-RBR-004 works on varying sets. Spacing is measured along the array direction, not along the inclined face.
- Bar handles constrained to the inclined faces stay constrained per bar; handles on parallel faces keep one shared constraint.
- **Numbering.** Each distinct length (after rounding) is a separate identity for numbering (STR-RBR-016), so a varying set can carry several rebar numbers; tags show the range ("12–17").
- Schedules list each distinct length, or one row per set with minimum and maximum length when the schedule groups by set.
- A varying set that regenerates to only one distinct length stays varying until converted.

**Data (IFC mapping).** One `IfcReinforcingBar` per distinct length group, aggregated under one `IfcElementAssembly` (`REINFORCEMENT_UNIT`), with the varying flag in RXD element data (`rebar`). Each group's `BarLength` is its own length; the set-level layout recipe (STR-RBR-004) is stored once on the assembly.

**Commands.** `rebar.convert_to_varying(rebar_id)`, `rebar.convert_to_standard(rebar_id)`

**Acceptance criteria.**
- [ ] A set in a tapered wall produces bars whose lengths follow the taper, and the schedule shows a length range.
- [ ] Converting a varying set in a tapered wall back to standard is refused and the message names the number of differing lengths.
- [ ] A varying set with five distinct rounded lengths exports one `IfcElementAssembly` (`REINFORCEMENT_UNIT`) aggregating five `IfcReinforcingBar` groups.
- [ ] Placing a varying set against a curved wall face is refused with an explanation.

### 4.2.6 Rebar editing, propagation and splitting
**ID:** STR-RBR-006 · **Priority:** P2 · **Ref:** research/03 §3.2 ("Editing")

**Behaviour & rules.**
- **Shape handles:** circles at bar ends, triangles on edges, a handle for the bar plane and a handle for the set extent.
- **In-canvas segment length controls:** editable, and rounded according to STR-RBR-002. Hook lengths are read-only unless *Override Hook Lengths* is on.
- **Pick New Host** and **Select Host**.
- **Move Rebar in Set**, **Remove Bar**, **Reset Bars** and **Split Set** at a chosen bar.
- **Propagate:** copy and adapt selected bars to other hosts of the same category. *Align by Host* matches the host's local frame; *Align by Face* matches a picked face. Constraints are re-solved per target, and targets where they cannot be solved are reported.

**Commands.**
- `rebar.set_segment_length(rebar_id, segment, length)`
- `rebar.move_bar_in_set(rebar_id, bar_index, delta)`
- `rebar.remove_bar(rebar_id, index)`
- `rebar.reset_bars(rebar_id)`
- `rebar.split_set(rebar_id, at_index) -> rebar_id`
- `rebar.propagate(rebar_ids[], target_host_ids[], mode: "host"|"face", face_ref?) -> rebar_ids[]`
- `rebar.pick_new_host(rebar_ids[], host_id)`

**Acceptance criteria.**
- [ ] Propagating a stirrup set from one beam to 10 beams of different lengths produces 10 sets, each spanning its own beam between covers.
- [ ] Splitting a 20-bar set at bar 8 yields two sets of 8 and 12 bars with identical type and shape.

### 4.2.7 Sketched rebar and rebar constraints
**ID:** STR-RBR-007 · **Priority:** P2 · **Ref:** research/03 §3.2 ("Rebar constraints"), §3.2 "Shape matching"

**Purpose.** Draw bar centrelines directly, and keep bars constrained to their host.

**Behaviour & rules.**
- **Sketch Rebar:** sketch lines and arcs in a plane inside the host. Reviera matches the result to a loaded shape (STR-RBR-008) or creates a new one.
- **Constraints:** each handle can be constrained to a planar reference:
  - **host cover**
  - **another bar**
  - **host surface** (with an offset)

  The reference plane must be parallel to the handle, within 6° for end handles. Straight bars can attach to stirrup bend points at 45° increments.
- **Edit Constraints mode:**
  - The bar being edited is highlighted in one colour and its constraint target in a second colour (theme tokens, not fixed colours).
  - The offset is shown as an editable dimension.
  - Constraints can be disabled, kept or reset to default.
  - Several sets, including cranked and spliced sets, can be edited together.
- When the host changes, constrained handles follow and unconstrained handles keep their absolute positions.

**Data (IFC mapping).** No IFC equivalent. **RXD element data** (`constraints`) holds one entry per handle: handle ID, target kind (Cover, Bar, Surface), target GlobalId or face reference, and offset.

**Commands.**
- `rebar.sketch(host_id, plane, curves[], bar_type_id, style, hooks?) -> rebar_id`
- `rebar.constrain(rebar_id, handle_id, target_kind, target_ref, offset)`
- `rebar.reset_constraints(rebar_id)`

**Acceptance criteria.**
- [ ] A longitudinal bar constrained to the top cover moves when the cover type changes from 40 to 50 mm.
- [ ] A constraint to a face 10° away from parallel is refused, with an explanation.

### 4.2.8 Rebar shapes, shape codes and shape matching
**ID:** STR-RBR-008 · **Priority:** P2 · **Ref:** research/03 §3.2 ("Rebar shapes and shape codes")

**Purpose.** Reviera shall support a parametric family kind, the **rebar shape family**. It defines a bar's centreline and its dimensions, and a **shape code** that is used in tags, schedules and bending details.

**Behaviour & rules.**
- **Shape family content:**
  - a sketched centreline of rebar lines
  - dimension parameters named A, B, C… that become the shape-code segments
  - major segments
  - bend and hook definitions
  - a multi-planar flag
  - **allowable bar types**
  - a **status checker** that validates the sketch, its dimensions and its constraints, so the family can be used
- **Shape parameters:** style, stirrup/tie attachment, start and end hook settings, shape image, spiral settings, **shape code**, and whether hooks are included in the shape.
- **Shape matching:** a sketched or free-form bar maps to an existing shape when all of these match:
  - segment count
  - joined geometry
  - hook count and direction
  - hook bend dimensions
  - style

  Otherwise Reviera creates a new shape family. It is named with the next free number, which the user can rename.
- **National shape-code standards:** Reviera ships open **shape-code catalogues** as data files that map a code to a segment pattern. Candidates are ISO 4066 and BS 8666 / ACI 315-style numbering. Each catalogue is authored from the published code structure and not copied from vendor templates. Users can add catalogues.
- Shape families can be built in 3D by rotating hooks and cranks out of plane.

**Data (IFC mapping).**
- The shape code goes to `IfcReinforcingBarType.BendingShapeCode`, and the ordered A, B, C… values go to `BendingParameters` (`IfcLengthMeasure` and `IfcPlaneAngleMeasure`).
- Where a standard pset exists, values are also written to it: `Pset_ReinforcingBarBendingsBS8666Common`, `Pset_ReinforcingBarBendingsISOCD3766Common`, `Pset_ReinforcingBarBendingsDIN135610Common` and `Pset_ReinforcingBarBendingsBECCommon`.
- The shape family itself is an `IfcReinforcingBarType`. Its **RXD family recipe** holds the centreline sketch, parameter list and status.

**Commands.**
- `rebar_shape.create_from_sketch(curves[], params, code?) -> shape_id`
- `rebar_shape.match(rebar_id) -> shape_id | null`
- `rebar_shape.check(shape_id) -> issues[]`
- `rebar_shape_catalogue.load(path)`

**Acceptance criteria.**
- [ ] Sketching an L-bar that matches an existing L shape reuses that shape and does not create a new family.
- [ ] Adding a 135° hook to the same sketch creates a new shape, because the hook count differs.
- [ ] An exported bar carries the shape code and parameters in both `IfcReinforcingBarType` and the matching standard pset.

### 4.2.9 Bar types, hooks and cranks
**ID:** STR-RBR-009 · **Priority:** P2 · **Ref:** research/03 §3.2 ("Rebar instance and type data", "Hooks", "Cranks")

**Behaviour & rules.**
- **Bar type:**
  - deformation (deformed or plain)
  - material
  - **bar diameter** and **model diameter**
  - standard bend diameter, standard hook bend diameter and stirrup/tie bend diameter
  - a **hook length table** (one row per hook type)
  - maximum bend radius
  - rounding overrides
  - a graphics subcategory
- **Hook type:** angle, extension multiplier and style.
- **Cranks:** a crank is an offset inside a segment. It is defined by crank length, offset and angle, and is kept inside the cover.
  - *Add Crank* acts on an existing bar. *Place with Crank* uses By Two Points.
  - Adding a crank re-matches the bar to a shape, or creates one.
  - Constraints stay editable on cranked bars.
- **Spiral bars:** height, pitch, and base and top finishing turns.

**Data (IFC mapping).** `IfcReinforcingBarType` attributes The RXD family recipe of the bar type holds what IFC cannot: ModelDiameter, BendDiameters, HookLengthTable and MaxBendRadius. Hook types are kept in **RXD document** `rebar`. Cranks are **RXD element data** (`rebar`): SegmentIndex, Position, Length, Offset and Angle.

**Commands.**
- `rebar_bar_type.create(name, diameter, material_id, ...) -> type_id`
- `rebar_hook_type.create(name, angle, extension_multiplier) -> id`
- `rebar.add_crank(rebar_id, segment, position, length, offset, angle)`

**Acceptance criteria.**
- [ ] Adding a 75 mm crank to segment A of a straight bar produces a bar whose crank stays inside cover, and a matching cranked shape is used or created.

### 4.2.10 Area reinforcement
**ID:** STR-RBR-010 · **Priority:** P2 · **Ref:** research/03 §3.3

**Purpose.** Lay out orthogonal bar layers over a region of a floor, wall or foundation slab.

**Workflow.**
1. Choose *Concrete › Area* and select the host.
2. Sketch a closed boundary. The parallel-line symbol marks the **major direction** edge, which can be changed.
3. Click *Finish*. A reinforcement symbol and tag are placed.

The *Whole Host* option uses the host outline as the boundary.

**Behaviour & rules.**
- **Layout rule:** Maximum Spacing or Fixed Number.
- Additional top/exterior and bottom/interior cover offsets allow layers to be stacked.
- **Layers:** Top/Bottom for floors, Exterior/Interior for walls, each × Major/Minor. Each layer has an on/off toggle, bar type, hook type, hook orientation and spacing.
- Host openings cut the bars.
- In "real rebar" mode (STR-RBR-002) the bars are rebar sets that can be seen in section. *Remove Area System* keeps them.

**Data (IFC mapping).**
- The system is an `IfcElementAssembly` with `REINFORCEMENT_UNIT`, with its recipe (Boundary, MajorDirection, LayoutRule, Layers[]) in RXD element data (`rebar`).
- The bars are `IfcReinforcingBar` elements aggregated into the system.
- The symbol is view-specific annotation (chapter 5).

**Commands.**
- `area_reinforcement.create(host_id, boundary | "whole_host", major_edge, layers[], layout_rule) -> system_id`
- `area_reinforcement.remove_system(system_id) -> rebar_ids[]`

**Acceptance criteria.**
- [ ] A 6 × 4 m slab region with top and bottom × major and minor at 200 mm produces four layers, cut around a 1 × 1 m opening.
- [ ] *Remove Area System* in real-rebar mode leaves four rebar sets that can be scheduled.

### 4.2.11 Path reinforcement
**ID:** STR-RBR-011 · **Priority:** P2 · **Ref:** research/03 §3.4

**Purpose.** Bars laid perpendicular to an **open** path along a slab or wall edge, for example edge U-bars and top bars over supports.

**Workflow.** Choose *Concrete › Path* and sketch an open path. A flip control chooses the side. Finish.

**Behaviour & rules.**
- **Parameters:** layout rule, additional offset, face (top or bottom), bar spacing or number of bars.
- **Primary bar:** type, length, shape (single segment or with perpendicular segments), and start and end hooks.
- Optional **alternating bars** with their own type, length, shape and offset.
- The reinforcement volume is calculated.
- Path boundary visibility can be controlled through visibility/graphics overrides.

**Data (IFC mapping).** `IfcElementAssembly` `REINFORCEMENT_UNIT` with its recipe in RXD element data (`rebar`). The bars are `IfcReinforcingBar`.

**Commands.** `path_reinforcement.create(host_id, path_curves[], side, primary, alternating?, spacing | count, face) -> system_id`

**Acceptance criteria.**
- [ ] A 5 m path at 150 mm spacing with alternating bars offset by 300 mm produces primary and alternating bars in the correct pattern.

### 4.2.12 Fabric (welded wire) reinforcement
**ID:** STR-RBR-012 · **Priority:** P2 · **Ref:** research/03 §3.5 · screenshots/structure_mep/fabric_area_sheet_layout.png

**Purpose.** Mesh sheets placed by area or one at a time.

**Behaviour & rules.**
- **Fabric area:**
  - Select a floor, wall or slab and sketch a closed boundary. The **major direction** sets sheet rotation.
  - Edge controls on the bounding rectangle set the start and end edges, sheet alignment and **lap splices**. Lap position is *Aligned* or *Staggered*.
  - Sheets are cut by the boundary and by host openings.
- **Single fabric sheet:** sheets snap to cover, sheet edges, lap positions and midpoints. *Cut by Host Cover* clips them.
- **Bent fabric sheet:** sketched from a cross-section profile.
- **Custom fabric sheets:** user-edited wire layouts, or rule-generated sheets in precast (STR-PRE-004).
- **Fabric wire type:** diameter and material.
- **Fabric sheet type:** major and minor wire type, spacing, overhangs, layout pattern, unit weight, and sheet length and width.

**Data (IFC mapping).**
- `IfcReinforcingMesh` (PredefinedType `USERDEFINED` or `NOTDEFINED`), typed by `IfcReinforcingMeshType` with MeshLength, MeshWidth, Longitudinal/TransverseBarNominalDiameter, Longitudinal/TransverseBarCrossSectionArea, Longitudinal/TransverseBarSpacing, BendingShapeCode and BendingParameters (for bent sheets).
- The fabric area is an `IfcElementAssembly` `REINFORCEMENT_UNIT` with its recipe (Boundary, MajorDirection, LapSplicePosition, LapLengths) in RXD element data (`rebar`).

**Commands.**
- `fabric_area.create(host_id, boundary, major_edge, sheet_type_id, lap_position, start_edge?) -> system_id`
- `fabric_sheet.place(host_id, sheet_type_id, point, rotation, cut_by_cover?) -> id`
- `fabric_sheet.create_bent(host_id, profile_curves[], sheet_type_id, extent) -> id`

**Acceptance criteria.**
- [ ] A fabric area over a slab with one opening produces sheets clipped by both the boundary and the opening, with the configured lap lengths.
- [ ] Switching from Aligned to Staggered moves every second row of lap splices by half a sheet length.

### 4.2.13 Free-form rebar
**ID:** STR-RBR-013 · **Priority:** P2 · **Ref:** research/03 §3.6

**Purpose.** Bars in hosts with complex or curved geometry, placed by picking host faces in 3D views.

**Behaviour & rules.**
- **Surface distribution:**
  - Non-planar bars are interpolated between a first bar and a last bar. Each of those is defined by where a start or end surface meets the host surface.
  - The bars follow the host surface and respect cover. Where faces have different covers, the largest cover is used.
- **Aligned distribution:**
  - Planar bars. Each bar plane cuts the host surfaces, and the planes are aligned along a **distribution path** built from host edges.
  - Path constraints are edited as *Path Surface 1/2* in Edit Constraints.
  - Each face's own cover is used.
  - This suits stirrups in members with varying sections.
- Shape matching for free-form bars uses *Workshop Instructions = Bent*. If no shape can describe the geometry, the bar reports only its total length.
- Free-form sets accept cranks and couplers.

**Data (IFC mapping).** `IfcReinforcingBar`. Bars use per-bar `IfcSweptDiskSolid` geometry along `IfcIndexedPolyCurve` or B-spline centrelines. RXD element data (`rebar`) records Distribution = "Surface" or "Aligned", with the constraint surfaces referenced by stable face IDs.

**Commands.** `rebar.place_free_form(host_id, kind: "surface"|"aligned", start_face, end_face, path_edges?, bar_type_id, layout) -> rebar_id`

**Acceptance criteria.**
- [ ] A surface distribution on a doubly curved shell keeps every bar at the specified cover within 1 mm at sampled points.
- [ ] An aligned distribution in a tapered beam produces stirrups whose sizes follow the taper.

### 4.2.14 Path shape distribution and morphed distribution
**ID:** STR-RBR-014 · **Priority:** P2 · **Ref:** research/03 §3.6 ("[2027] Path Free Form Rebar", "Morphed") · screenshots/structure_mep/rebar_3d_path_shape_distribution_2027.png · screenshots/structure_mep/rebar_morphed_longitudinal_distribution_2027.png

**Purpose.** Two advanced distributions for infrastructure-type geometry such as bridges and tunnels.

**Behaviour & rules.**
- **Path shape distribution:**
  1. Place a *main bar* in a section view by Two Points, Expand to Host or Sketch.
  2. Click **Path** and select one or more continuous host edges.
  3. Finish.

  Identical planar bars are then distributed along the double-curved edge path. Bar segments can be constrained to edges so that the set follows changes to the host. The distribution range and the main bar can be edited later, and bar lengths may vary.
- **Morphed distribution:** select a transverse stirrup set (circular, spiral or multi-segment). Longitudinal bars are generated that follow the stirrup shape, and they update when the stirrups change.

**Data (IFC mapping).** `IfcReinforcingBar`. RXD element data (`rebar`) records Distribution = "Path" or "Morphed", with the driver set's GlobalId (morphed) or PathEdgeRefs (path).

**Commands.**
- `rebar.distribute_along_path(main_rebar_id, edge_refs[], range?) -> rebar_id`
- `rebar.distribute_morphed(driver_stirrup_set_id, bar_type_id, count | spacing) -> rebar_id`

**Acceptance criteria.**
- [ ] Changing a driver stirrup set's diameter regenerates the morphed longitudinal bars inside the new stirrup outline.
- [ ] A path distribution along a 3D-curved edge keeps each bar plane perpendicular to the path tangent.

### 4.2.15 Splices and couplers
**ID:** STR-RBR-015 · **Priority:** P2 · **Ref:** research/03 §3.7 · screenshots/structure_mep/rebar_couplers.png

**Behaviour & rules.**
- **Splice:**
  - *Splice* on a selected set, by **Pick Line** or **By Length**.
  - Lap and stagger lengths come from the splice type or from the bar type.
  - Spliced sets are constrained to each other and move together.
  - Splicing is supported only on straight segments of shape-driven rebar. It is refused for spiral, arc, multi-planar, area, path and free-form bars. Free-form longitudinal splicing is an open question (§4.17).
- **Coupler:**
  - *Concrete › Coupler*: **On Bar End** or **Between Two Bars**.
  - The coupler type is sized to the bar automatically, and a type is created when none fits.
  - **Tolerances:** the bar ends must be no more than 10 bar diameters apart and offset by no more than 3 diameters. Arc bars must share a centre.
  - Couplers apply **end treatments** that shorten the bars.
  - Couplers can be scheduled, grouped, numbered per partition, tagged, and propagated across sets.
  - The coupler family option *Align to View* hides a coupler when the view direction is more than 75° from the bar axis.
  - Couplers always display as solids in 3D.

**Data (IFC mapping).**
- Couplers are `IfcMechanicalFastener`. In IFC4, PredefinedType is `USERDEFINED` with ObjectType "COUPLER"; in IFC4X3 it is `COUPLER` where available. The type is `IfcMechanicalFastenerType`.
- The coupler is connected to both bars through `IfcRelConnectsWithRealizingElements`, with the bars as relating and related elements and the coupler as the realizing element.
- Splices are stored in RXD element data (`rebar`): SpliceTypeId, LapLength, Stagger and partner GlobalIds.

**Commands.**
- `rebar.splice(rebar_id, mode: "line"|"length", line? , length?, splice_type_id?) -> rebar_ids[]`
- `rebar_coupler.place(bar_end_refs[1..2], type_id?) -> coupler_id`

**Acceptance criteria.**
- [ ] Placing a coupler between two Ø25 bar ends 300 mm apart is refused, because the gap is more than 10 diameters (250 mm).
- [ ] A spliced set exports two bars per splice location, connected by the configured lap length.

### 4.2.16 Reinforcement numbering and partitions
**ID:** STR-RBR-016 · **Priority:** P2 · **Ref:** research/03 §3.1 ("Reinforcement numbering and partitions") · screenshots/structure_mep/rebar_partitions_by_level.png

**Purpose.** Give identical reinforcement elements identical numbers within a partition.

**Behaviour & rules.**
- Each rebar set, fabric sheet and coupler gets a **Rebar Number**.
- Elements that are identical in type, size, material and shape, including segment values after rounding, share a number within the same **Partition**.
- **Partition** is a free-text instance value, used for phase, host type, location, top or bottom, and so on.
- Rebar, fabric and couplers each have their own number sequence.
- *Reinforcement Numbers* dialog: list the sequences per partition, change a start number, **remove gaps** and renumber.
- Numbers update on regeneration when an element's identity changes. Locked numbers (an option) do not.

**UI: Reinforcement Numbers dialog** (*Concrete › Settings › Reinforcement Numbers*, `reinforcement_numbering.show(partition?)`).
- A partition list on the left (every partition name in use, plus "(none)"), with a filter box. `partition` preselects one.
- For the selected partition, three tabs (Rebar, Fabric, Couplers), each a grid with: number, element count, a short identity summary (bar type, shape code, main dimensions), locked flag, and a *Select* button that selects those elements in the model.
- Per sequence: **Start number** (integer ≥ 1); **Remove gaps**; **Renumber** (reassigns numbers in creation order, keeping locked numbers fixed and skipping them).
- Per row: change a number to a free value (refused, with the conflicting row highlighted, if the value is used by a different identity in the same sequence), lock or unlock.
- All edits apply on *OK* as one transaction; *Cancel* discards them.

**Data (IFC mapping).** The rebar number is written to `IfcReinforcingBar.Tag`, and to `Pset_PrecastConcreteElementFabrication.PieceMark` where an exchange requires it. Partition and lock flag: RXD element data (`rebar`). Sequences: RXD document `model/rebar`.

**Commands.**
- `reinforcement_numbering.renumber(partition?, kind: "rebar"|"fabric"|"coupler", start?, remove_gaps?)`
- `reinforcement_numbering.show(partition?)`
- `rebar.set_partition(ids[], name)`

**Acceptance criteria.**
- [ ] Two identical stirrup sets in partition "L1" share number 5. Moving one to partition "L2" gives it a number in L2's sequence.
- [ ] *Remove gaps* turns the sequence 1, 2, 4, 7 into 1, 2, 3, 4 without changing which elements share numbers.
- [ ] In the dialog, typing number 3 for a row when 3 belongs to another identity in the same partition and kind is refused and the other row is highlighted.
- [ ] Renumbering a sequence with number 4 locked keeps 4 on its elements and numbers the others 1, 2, 3, 5, ….

### 4.2.17 Rebar visibility and presentation
**ID:** STR-RBR-017 · **Priority:** P2 · **Ref:** research/03 §3.2 ("Rebar presentation", "View visibility"), §3.8

**Purpose.** Control, per view, which bars of a set are drawn and whether they show through or as solids, so that drawings stay readable.

**Workflow.**
1. Select one or more rebar sets (or area, path or fabric systems).
2. On the contextual tab, choose *View Visibility States* (a grid of the project's views with *Unobscured* and *Solid* check boxes per view; *Solid* is enabled only for 3D views), or choose a presentation mode in the *Presentation* panel, which applies to the active view.
3. For *Select*, click the bars to show; **Enter** finishes. The picks are journaled as micro-operations so an unfinished pick session is recovered after a crash (§2.1 of the conventions).

**Behaviour & rules.**
- **Per-instance, per-view visibility states:**
  - *View unobscured*: the bars show through the host.
  - *View as solid*: applies in 3D views at Fine detail. At Coarse and Medium detail bars are drawn as centrelines.
- Default visibility: the placement view and all section views. Rebar cut by a section is always shown.
- **Set presentation per view:** Show All, Show First/Last, Show Middle, or Select (pick the bars to show).
  - First/Last and Middle draw a **set extent line** with end ticks spanning the whole set, so the distribution is still shown.
  - The default for new sets comes from Reinforcement Settings (STR-RBR-002).
  - Select keeps bar indices; if the bar count later drops below a picked index, that pick is dropped and the view falls back to Show All when no picks remain.
- **Spliced set presentation:** show representative bars per lap zone.
- Presentation changes only what is drawn. Tags, schedules and IFC export always count every bar.
- View templates (chapter 5) can carry a rebar presentation default that overrides the per-set mode in views using the template.
- Visibility and presentation edits are view data and are undoable.

**Data (IFC mapping).** View-dependent settings are stored with the view's own RXD data (chapter 5), as a map from rebar GlobalId to Unobscured, Solid and Presentation (mode plus selected indices). Nothing is written to the rebar's IFC entity.

**Commands.**
- `rebar.set_view_visibility(rebar_ids[], view_id, unobscured?, solid?)`
- `rebar.set_presentation(rebar_ids[], view_id, mode, selected_indices?)`

**Acceptance criteria.**
- [ ] *Show Middle* in a plan view draws one representative bar and the set extent line, and other views are not affected.
- [ ] A set set to *Unobscured* in a 3D view is visible through its concrete host there, and stays hidden by the host in a second 3D view.
- [ ] A set of 15 bars shown with *Show First/Last* still tags as quantity 15 and exports 15 bar bodies.
- [ ] *Solid* cannot be ticked for a plan view in the View Visibility States grid.

### 4.2.18 Rebar documentation
**ID:** STR-RBR-018 · **Priority:** P2 · **Ref:** research/03 §3.8

**Purpose.** Annotate and schedule reinforcement for placing drawings and bar bending schedules.

**Workflow.**
- **Tag:** *Annotate › Tag by Category*, click a set; the tag attaches to the set (or to the representative bar shown by STR-RBR-017).
- **Multi-rebar annotation:** choose *Aligned* or *Linear*, pick one or more sets, place the dimension line, then the tag. Picks are journaled as micro-operations (§2.1 of the conventions).
- **Bending detail:** select a set, choose *Bending Detail*, click to place in the view; the scale and rotation can be set before the click.

**Behaviour & rules.**
- **Rebar tags** read number, shape code, bar size, spacing and quantity.
- **Multi-rebar annotation:** an aligned or linear dimension plus a tag that spans a set or several sets.
  - The dimension measures the set extent and the tag reads quantity, size and spacing (for example "15 Ø12 @ 150").
  - Several sets are accepted only if they are parallel; otherwise the pick is refused with a message.
  - The annotation follows the sets when they move and is deleted with the last set it references.
- **Bending detail:** a view-specific annotation that draws a bar's shape with its segment dimensions. It updates when the bar changes.
  - Segment labels and dimension values use the shape's A–R parameters after rounding (STR-RBR-002). Hooks are drawn with their angle.
  - Varying sets (STR-RBR-005) show the range of each varying segment ("1 200–1 650").
- **Rebar schedule** (chapter 5 schedule engine): bar number, shape code, segments A–R, quantity, total length and mass. It can be filtered by partition.
  - Mass is length × nominal mass per metre of the bar type. Totals use rounded lengths when rounding is on.
  - A bending-schedule template that groups by partition and bar number is shipped.

**Data (IFC mapping).** Annotation entities as defined in chapter 5. Schedule fields read `IfcReinforcingBar`, `IfcReinforcingBarType` and RXD element data (`rebar`). Multi-rebar annotations and bending details store the GlobalIds of their referenced sets in their own view-specific RXD data.

**Commands.**
- `rebar_tag.place(rebar_id, view_id, point, tag_type_id) -> id`
- `multi_rebar_annotation.place(rebar_ids[], view_id, kind: "aligned"|"linear", line) -> id`
- `bending_detail.place(rebar_id, view_id, point, scale?) -> id`

**Acceptance criteria.**
- [ ] Changing segment B of a bar updates its bending detail and the schedule's B column in the same regeneration.
- [ ] A multi-rebar annotation on a set of 15 Ø12 bars at 150 mm reads "15 Ø12 @ 150" and its dimension equals the set extent (2 100 mm).
- [ ] Picking two non-parallel sets for one multi-rebar annotation is refused with a message.
- [ ] The schedule's mass column for 10 bars of 6 m Ø16 (1.578 kg/m) shows 94.7 kg.

---

## 4.3 Structural analytical model, loads and analysis

**Design stance.** Reviera's analytical model is **independent**, following the reference product's current design rather than its older automatic one. Analytical elements are stand-alone, persisted elements. They are optionally associated with physical elements, and they respond to physical changes **only when asked** (automation). The whole analytical model is one `IfcStructuralAnalysisModel` per analysis scenario. A default model named "Analytical Model" is created on first use.

Ref: research/03 §4.1 · screenshots/structure_mep/analytical_automation_physical_to_analytical.png

### 4.3.1 Analytical members
**ID:** STR-AM-001 · **Priority:** P2 · **Ref:** research/03 §4.2

**Purpose.** Line elements that idealise beams, columns and braces.

**Workflow.** Choose *Analyze › Structural Analytical Model › Analytical Member*.
- *Top Point Definition*: one click creates a vertical member between the current level and the level below.
- *Start/End Point Definition*: draw a line or an arc, with 3D snapping and chain.

Nodes are created at both ends (STR-AM-003).

**UI.** Properties:
- highest and lowest associated level
- structural material and physical material asset
- **structural role**: Beam, Column, Brace, Member or Tie
- **section type**, filtered to loaded framing or column types
- section shape
- **cross-section rotation**
- start and end **releases**: Fixed, Pinned or User (six DOF flags)
- *Analyze As*
- the associated physical element (read-only)

**Behaviour & rules.**
- A member exists without any physical element.
- Changing the section type does not change any physical element until *Analytical to Physical* (STR-AM-006) is run.
- The member's local coordinate system is set by its end nodes and the cross-section rotation.

**Data (IFC mapping).**
- `IfcStructuralCurveMember`. PredefinedType: `RIGID_JOINED_MEMBER` by default, `PIN_JOINED_MEMBER` when both ends release all rotations, and `TENSION_MEMBER` or `COMPRESSION_MEMBER` from *Analyze As*.
- Axis: `IfcDirection`. Topology: an `IfcEdge` representation between two `IfcVertexPoint`s.
- Material and section: `IfcMaterialProfileSetUsage` with `IfcRelAssociatesMaterial`.
- End releases: `IfcRelConnectsStructuralMember` to each end node, with AppliedCondition `IfcBoundaryNodeCondition` (translational and rotational stiffness per axis) and ConditionCoordinateSystem.
- The member is included in the model through `IfcRelAssignsToGroup` (RelatingGroup = `IfcStructuralAnalysisModel`).
- **RXD element data** (`analysis`): StructuralRole, HighestLevel, LowestLevel and AnalyzeAs, where the PredefinedType cannot express them.

**Commands.**
- `analytical_member.create(curve, section_type_id, material_id, role, releases?) -> id`
- `analytical_member.create_vertical(point, level, section_type_id) -> id`
- `analytical_member.set_releases(id, end, dof_flags | preset)`

**Acceptance criteria.**
- [ ] A member created with no physical counterpart persists through save and reload as an `IfcStructuralCurveMember` with two `IfcStructuralPointConnection` ends.
- [ ] Setting both ends to Pinned exports with zero rotational stiffness in the end conditions.

### 4.3.2 Analytical panels and openings
**ID:** STR-AM-002 · **Priority:** P2 · **Ref:** research/03 §4.2

**Purpose.** Surface elements that idealise walls, floors and roofs.

**Workflow.**
- **Panel by Boundary:** a planar panel sketched on a reference plane. The plane can be changed later.
- **Panel by Extrusion:** a curved panel. A profile drawn on a work plane is extruded perpendicular to it, by 3 m (10 ft) by default.
- **Analytical Opening:** sketched inside a panel.

**Behaviour & rules.**
- Properties: thickness, material, structural role (floor, wall, roof), *Analyze As* (shell, membrane or bending plate) and openings.
- Panels can be **split** along a line.
- **Cladding panels** carry and transfer lateral wind and snow loads to their supports but add no stiffness. They are flagged and excluded from the stiffness matrix by providers.
- Panels align to supports when automation creates them (STR-AM-006).

**Data (IFC mapping).**
- `IfcStructuralSurfaceMember`. PredefinedType is `SHELL`, `BENDING_ELEMENT` or `MEMBRANE_ELEMENT`; cladding panels use `USERDEFINED` with ObjectType "CLADDING".
- Thickness: the `Thickness` attribute. Geometry: an `IfcFaceSurface` topology representation.
- Openings are inner bounds of the face, and their editable sketches are kept as RXD element data (`constraints`).

**Commands.**
- `analytical_panel.create_by_boundary(plane, boundary_curves[], thickness, material_id, role) -> id`
- `analytical_panel.create_by_extrusion(work_plane, profile_curves[], depth, thickness, material_id) -> id`
- `analytical_panel.split(id, line) -> ids[]`
- `analytical_opening.create(panel_id, boundary_curves[]) -> id`
- `analytical_panel.set_cladding(id, bool)`

**Acceptance criteria.**
- [ ] A curved panel by extrusion exports as an `IfcStructuralSurfaceMember` on a cylindrical face.
- [ ] A cladding panel loaded with an area wind load is reported to providers with stiffness excluded.

### 4.3.3 Analytical nodes and links
**ID:** STR-AM-003 · **Priority:** P2 · **Ref:** research/03 §4.2

**Behaviour & rules.**
- **Nodes are never placed directly.**
  - **End nodes** sit at member ends and panel corners. They merge when elements are joined within tolerance.
  - **Hosted nodes** sit inside a member or panel, are drawn smaller and are driven by the host.
- Each node keeps an **associated level plus offset**, and follows level moves. Optionally it follows grids.
- Nodes define the orientation of the local coordinate system.
- **Analytical links:** a connection between two nodes, rigid by default.
  - Fixity is set per link type: translation and rotation per axis, fixed or released.
  - A link is deleted when either node is deleted.
  - Links cannot be associated with physical elements.

**Data (IFC mapping).**
- Nodes: `IfcStructuralPointConnection` with an `IfcVertexPoint` representation, plus RXD element data (`analysis`): Kind (End or Hosted), host GlobalId, level GlobalId, offset and FollowsGrids.
- Links: `IfcStructuralCurveMember` with PredefinedType `USERDEFINED` and ObjectType "RIGID_LINK", with the link type in RXD element data (`analysis`). Fixity is expressed through `IfcRelConnectsStructuralMember.AppliedCondition` at each end. This open choice is recorded in §4.17.

**Commands.**
- `analytical_link.create(node_a, node_b, link_type_id) -> id`
- `analytical_link_type.create(name, fixity[6]) -> id`
- `analytical_node.merge(tolerance?)`

**Acceptance criteria.**
- [ ] Moving a level by 200 mm moves all nodes associated with it by 200 mm, and the members attached to them follow.
- [ ] Deleting a node deletes the links attached to it and reports the count.

### 4.3.4 Physical–analytical association
**ID:** STR-AM-004 · **Priority:** P2 · **Ref:** research/03 §4.2 ("Association")

**Behaviour & rules.**
- An **association** is a group that relates any number of physical elements to any number of analytical elements. Each element can belong to only one association.
- Commands: *Add*, *Highlight* and *Remove*.
- **Assisted Association** proposes matches by proximity, orientation and section similarity. The user accepts or rejects each one.
- Any physical element can be associated, including in-place families and group members.

**Data (IFC mapping).**
- A one-to-many association uses `IfcRelAssignsToProduct`, with RelatingProduct = physical element and RelatedObjects = analytical items.
- A many-to-many association uses an `IfcGroup` with ObjectType "AnalyticalAssociation" that contains all members through `IfcRelAssignsToGroup`.
- Reviera reads both forms on import.

**Commands.**
- `analytical_association.add(physical_ids[], analytical_ids[]) -> association_id`
- `analytical_association.remove(association_id)`
- `analytical_association.suggest(scope?) -> proposals[]`

**Acceptance criteria.**
- [ ] Trying to add an element that already belongs to an association is refused, and the existing association is named.
- [ ] *Assisted Association* on a 10-column grid frame proposes 10 one-to-one pairs.

### 4.3.5 Loads
**ID:** STR-LOAD-001 · **Priority:** P2 · **Ref:** research/03 §4.3

**Purpose.** Point, line and area actions applied to analytical elements.

**Workflow.** Choose *Analyze › Analytical Loads › Loads*, then pick one of:
- **Point load**: on an end point, or at a picked position
- **Line load**: on a host (a whole member or panel edge), or along a defined path
- **Area load**: on a host (a whole panel), or inside a defined boundary, including sloped area loads

**Behaviour & rules.**
- Every load has **exactly one analytical host**. It follows the host and is deleted with it. A load may lie outside its host, with a warning. *Is Constrained on Host* records whether the load is constrained.
- **Properties:**
  - load case; nature (read-only, from the case)
  - orientation: Project, Host Local CS or Work Plane
  - Fx, Fy, Fz, Mx, My, Mz. Line and area loads take start and end, or three-corner, values for linear variation.
  - *Projected* (for line and area loads)
  - *Is Reaction*
- The display scale per force or moment unit comes from Structural Settings (STR-SET-001).

**Data (IFC mapping).**

| Load | Activity entity | Applied load | Notes |
|------|-----------------|--------------|-------|
| Point | `IfcStructuralPointAction` | `IfcStructuralLoadSingleForce` | — |
| Line | `IfcStructuralCurveAction` | `IfcStructuralLoadLinearForce` | ProjectedOrTrue = `PROJECTED_LENGTH` or `TRUE_LENGTH`; PredefinedType `CONST` or `LINEAR` |
| Area | `IfcStructuralPlanarAction` | `IfcStructuralLoadPlanarForce` | PredefinedType `CONST`, `BILINEAR`, … |

- The host link is `IfcRelConnectsStructuralActivity` (RelatingElement = member, panel or node).
- The load case is set by assigning the action to an `IfcStructuralLoadCase` through `IfcRelAssignsToGroup`.
- Orientation is expressed through the action's coordinate system (`GlobalOrLocal`). IsConstrainedOnHost and IsReaction are RXD element data (`analysis`).

**Commands.**
- `load.create_point(host_id, position?, case_id, forces[6], orientation) -> id`
- `load.create_line(host_id, path?, case_id, start[6], end[6], orientation, projected?) -> id`
- `load.create_area(host_id, boundary?, case_id, values[3][3], orientation, projected?) -> id`

**Acceptance criteria.**
- [ ] Deleting an analytical member deletes its 4 line loads, and the confirmation names the count.
- [ ] A trapezoidal line load exports as `IfcStructuralCurveAction` with `LINEAR` distribution and the correct end values.
- [ ] Changing a load's case updates its nature display immediately.

### 4.3.6 Load cases, natures, combinations and usages
**ID:** STR-LC-001 · **Priority:** P2 · **Ref:** research/03 §4.4 · screenshots/structure_mep/load_cases_and_combinations_dialog.png

**Purpose.** A single dialog to define how loads are grouped and combined.

**UI.** *Analyze › Load Cases & Combinations* (or the shortcut buttons in Structural Settings, STR-SET-001) opens one dialog, `load_case.manage(tab?)`, with tabs (`tab` = `code`, `natures`, `cases`, `combinations` or `usages`):
- *Design Code*: region and code selection, the template file in use and its version, and *Load template…* for a user template.
- *Load Natures*: grid of name, IFC action source, action type (permanent, variable, accidental) and source (Code or User). Code natures are read-only except their display name.
- *Load Cases*: grid of name, nature (drop-down), category, self-weight flag, and a read-only count of loads in the case. *Add*, *Duplicate*, *Delete*.
- *Load Combinations*: grid of name, formula, limit state, usages, plus one factor column per case that can be shown instead of the formula. *Generate…* opens the proposals list described below.
- *Load Usages*: list of usage tags with add, rename and delete; deleting a usage removes it from combinations.

Grids support copy and paste with spreadsheets as tab-separated text.

**Validation in the dialog** (shown inline per cell; *OK* is disabled while any error remains):
- Names are unique within natures, cases, combinations and usages, are non-empty and contain no `*`, `+` or `-`, so formulas parse.
- A formula references only existing cases or combinations, has numeric factors, and cannot reference itself directly or through another combination.
- A case cannot be deleted from inside a formula; it must be deleted on the *Load Cases* tab, which shows the cascade counts (STR-GEN-002).
- Every case must have a nature; a nature in use cannot be deleted.

*OK* commits all tabs as one transaction ("Load Cases and Combinations"); *Cancel* discards everything, including generated proposals not yet added.

**Behaviour & rules.**
- Choosing a **region and design code** fills the load nature list from an open **code template** (a Lua or JSON data file shipped with Reviera). Example natures are dead, live, wind, snow, seismic, roof live, temperature and accidental.
- Each nature records its source: *Code* or *User*. User natures are kept when the code changes.
- **Load cases** have a name, nature and category. Deleting a case warns that its loads will be deleted and that it will be removed from combinations, and then does both.
- **Load combinations** have:
  - a name
  - a **formula** such as `1.35*DL1 + 1.5*LL1`, edited as text or as per-case factor columns
  - a **limit state**: Ultimate, Serviceability, Quasi-permanent, Frequent, Rare, Accidental or Special
  - **usages**, user-defined tags such as Gravity, Lateral or Combined
- **Automatic combination generation:** a local, offline rule engine reads the selected code template and generates combinations. Candidate templates are EN 1990 equations 6.10/6.10a/b and the ASCE 7 basic combinations, written as editable data. Cases whose nature does not fit the template are flagged. The user reviews the proposals and picks *Add to table*.

**Data (IFC mapping).**
- Cases: `IfcStructuralLoadCase` (PredefinedType `LOAD_CASE`). ActionType: `PERMANENT_G`, `VARIABLE_Q` or `EXTRAORDINARY_A`. ActionSource from the nature: `DEAD_LOAD_G`, `LIVE_LOAD_Q`, `SNOW_S`, `WIND_W`, `EARTHQUAKE_E`, `TEMPERATURE_T`, … Self-weight coefficients where they apply.
- Combinations: `IfcStructuralLoadGroup` with PredefinedType `LOAD_COMBINATION`. Each case is assigned with `IfcRelAssignsToGroupByFactor`, and its factor is the case coefficient.
- Limit state is written to the group's `Purpose`. Usages are RXD element data (`analysis`).
- Natures, code selection and combination templates: **RXD document** `analysis`.
- All groups are related to the `IfcStructuralAnalysisModel` through `LoadedBy`.

**Commands.**
- `load_case.create(name, nature, category) -> id`
- `load_case.delete(id)`
- `load_combination.create(name, formula, limit_state, usages[]) -> id`
- `load_combination.generate(code_template_id, options) -> proposals[]`
- `load_nature.create(name, action_source) -> id`
- `structural_code.set(region, code_id)`
- `load_case.manage(tab?)` (opens the dialog; the edits it commits are undoable as one item)

**Acceptance criteria.**
- [ ] Entering `1.35*G + 1.5*Q` creates a combination with two `IfcRelAssignsToGroupByFactor` relations, with factors 1.35 and 1.5.
- [ ] A combination formula `1.0*C1` where C1 is itself a combination that references the first one is flagged as circular and *OK* stays disabled.
- [ ] `load_case.manage("combinations")` opens the dialog on the *Load Combinations* tab.
- [ ] Deleting case Q warns that 12 loads and 3 combinations are affected, and after confirmation removes the loads and strips Q from the 3 formulas.
- [ ] The EN 1990 template generates ULS and SLS combinations offline, with no network access.

### 4.3.7 Boundary conditions
**ID:** STR-BC-001 · **Priority:** P2 · **Ref:** research/03 §4.5 · screenshots/structure_mep/boundary_conditions_on_analytical_panel.png

**Workflow.** Choose *Analyze › Boundary Conditions*, then **Point**, **Line** or **Area**. Pick the host. Point conditions can be positioned anywhere on the host with temporary dimensions.

**Behaviour & rules.**
- The state is **Fixed**, **Pinned**, **Roller** or **User**. User states set each translation and rotation DOF to Fixed, Released or Spring, with a spring modulus.
- Each state has its own symbol family, set in Structural Settings, along with the spacing of line and area symbols. Default symbol families are open, Reviera-authored content.
- Boundary conditions can be tagged, filtered and scheduled.

**Data (IFC mapping).**

| Kind | Connection entity | Applied condition |
|------|-------------------|-------------------|
| Point | `IfcStructuralPointConnection` | `IfcBoundaryNodeCondition` (TranslationalStiffnessX/Y/Z, RotationalStiffnessX/Y/Z as `IfcBoolean` for fixed or released, or a linear or rotational stiffness measure for springs) |
| Line | `IfcStructuralCurveConnection` | `IfcBoundaryEdgeCondition` |
| Area | `IfcStructuralSurfaceConnection` | `IfcBoundaryFaceCondition` |

All are connected to their host with `IfcRelConnectsStructuralMember` and record the named state (Fixed, Pinned, Roller, User) as RXD element data (`analysis`).

**Commands.** `boundary_condition.create(kind: "point"|"line"|"area", host_id, location?, state, dof?: {tx,ty,tz,rx,ry,rz}) -> id`

**Acceptance criteria.**
- [ ] A Pinned point condition exports with all three translations fixed and all rotations released.
- [ ] A User condition with Tz = Spring of 5 000 kN/m exports `TranslationalStiffnessZ` = 5.0E6 N/m.

### 4.3.8 Analytical automation
**ID:** STR-AM-006 · **Priority:** P2 · **Ref:** research/03 §4.2 ("Automation"), §13.7

**Purpose.** Generate or update one model from the other on demand.

**Behaviour & rules.**
- **Physical → Analytical:** runs on selected physical elements, or on all supported elements that have no association.
  - It creates members and panels, aligns them to levels and grids within tolerances, connects nodes, aligns panels to supports, and uses core-layer thickness.
  - It creates the associations.
  - It also re-runs on demand for elements that changed, including new openings.
- **Analytical → Physical:** creates or updates columns, framing, walls and floors from analytical elements. That includes **section changes that come from analysis results**.
- **Rule packages** are Lua scripts: Reviera's equivalent of the reference's visual-programming packages, and a candidate for the future Lua node editor. Users can copy and edit them through *Automation Customization*. The default package ships with Reviera under GPL-3.0-or-later.
- **Disconnect:** removes the associations and leaves both models in place.
- Every run writes a log with created, updated and skipped elements and the reason for each.
- Updates skip analytical elements flagged as manually adjusted (STR-AM-008) unless the run option *Overwrite manual adjustments* is on; skipped elements are logged with that reason. After a run, the consistency check (STR-AM-007) runs on the touched elements when automatic checks are on.

**Data (IFC mapping).** Runs create and update the entities in STR-AM-001 to 004. Run history (package, version, timestamp, counts): **RXD document** `analysis`.

**Commands.**
- `analytical_automation.physical_to_analytical(element_ids[] | "unassociated", package_id?) -> report`
- `analytical_automation.analytical_to_physical(element_ids[], package_id?) -> report`
- `analytical_automation.disconnect(element_ids[])`

**Acceptance criteria.**
- [ ] On a 3-storey, 4 × 3-bay steel frame with slabs, Physical → Analytical creates members for every column and beam and panels for every slab. The nodes are connected, and the solver interface (ANA-FEA-001) reports no unconnected nodes.
- [ ] Changing an analytical member's section and running Analytical → Physical changes the associated beam's type.

### 4.3.9 Analytical model checks: supports and consistency
**ID:** STR-AM-007 · **Priority:** P2 · **Ref:** research/03 §4.1–4.2, §15.1 · web: Analytical Model Settings · web: Modifying Elective Checking Criteria and Tolerances · web: Automatic Checks

**Purpose.** Find problems in the analytical model, and between it and the physical model, before it is sent to an analysis provider: members that nothing supports, nodes that should meet but do not, analytical elements that have drifted from their physical counterparts, and hints of instability.

**Workflow.**
1. Choose *Analyze › Structural Analytical Model › Check Supports* or *Check Consistency*. Choose the scope: whole model (default), current selection, or elements visible in the active view.
2. The check runs in the background (ANA-GEN-001) and opens the **Analytical Checks** palette with the results.
3. Review each issue: *Select*, *Zoom to*, *Isolate* (a temporary view isolation of the elements and their neighbours within the support distance), or *Ignore* with an optional reason.
4. Fix the model (for example with STR-AM-008 adjust, `analytical_node.merge`, or a boundary condition), then *Re-check*. Fixed issues drop off the list; issues still present keep their ignore state.
5. **Automatic mode:** when *Member supports* or *Analytical/physical consistency* is turned on in Structural Settings (STR-SET-001), the corresponding check runs incrementally after every committed transaction that adds, moves or deletes structural or analytical elements. It checks only the changed elements and their neighbours within the support distance, and posts new issues to the project warnings list (chapter 1) as well as the palette. Both modes are off by default, because early, incomplete models would produce many warnings.

**UI.** *Analytical Checks* is a dockable palette (§4.0.3):
- a toolbar with scope, *Check Supports*, *Check Consistency*, *Re-check*, a severity filter (Error, Warning, Hint, Info), *Show ignored* and *Export CSV*
- a tree grouped by check, then by issue; each issue row shows severity, a message, the element names and IDs, and where relevant the measured distance against the tolerance (for example "node gap 42 mm > merge distance 5 mm")
- in the canvas, issue locations are marked with a temporary graphic (a ring at the node or a highlight along the edge) while the palette is open.

**Behaviour & rules.**

*Support check* (`check_supports`). Each analytical member and panel (and each physical structural element that has no association, using its location line or core mid-plane) must be carried by something:

| Code | Issue | Rule | Severity |
|------|-------|------|----------|
| S1 | Unsupported member | A column's lower end, both ends of a beam or brace, and the bearing edge of a panel must each lie within the **support distance** of another structural element, an analytical link or a boundary condition. Columns and walls are supported from below; beams and braces at their ends; floors along at least one edge or at points. | Warning |
| S2 | Circular support | A chain of supports loops back on itself (beam A carried by beam B carried by beam A) without reaching a boundary condition or foundation. Can be turned off in Structural Settings. | Warning |
| S3 | No supports in scope | The scope contains no boundary condition at all. | Error |

*Consistency check* (`check_consistency`). Each criterion can be turned on or off in Structural Settings; C1–C7 are on by default, C8 is off.

| Code | Issue | Rule | Severity |
|------|-------|------|----------|
| C1 | Disconnected nodes | Two end nodes, or an end node and a member or panel edge, lie closer than the **support distance** but farther than the **node merge distance**, so they look connected but are not. Also free member ends that touch nothing and carry no boundary condition. | Warning |
| C2 | Isolated part | A connected group of analytical elements has no boundary condition, link or support path to one; the solver would see a mechanism. | Error |
| C3 | Physical/analytical mismatch | For associated elements, the analytical axis or surface lies farther than the **analytical-to-physical distance** from the physical location line, core mid-plane or default analytical position. Also a different section type, material or thickness from the physical element. | Warning |
| C4 | Analytical outside physical | An associated member's axis, or a panel's surface, leaves the physical body by more than the analytical-to-physical distance. | Warning |
| C5 | No counterpart | An analytical member or panel with no physical association, or a physical structural element (load-bearing column, framing, structural wall or floor) with no analytical association. Analytical-only elements are allowed (STR-GEN-001), so this is informational. Elements that *lost* their association when the other side was deleted are reported as warnings. | Info / Warning |
| C6 | Overlap not reflected | Physical beam and slab (or wall and slab) intersect, but their analytical member and panel neither share nodes nor lie within the support distance of each other. | Warning |
| C7 | Instability hints | Heuristic checks, labelled as hints and never errors: a node where every connected member releases the same rotation and no boundary condition fixes it; a member with torsion (Mx) released at both ends; a member released in all rotations at both ends that is not a truss member; a panel supported only along one straight line with pinned supports. | Hint |
| C8 | Missing properties | An analytical element without a structural material, a physical material asset or a section type; a panel with zero thickness. | Warning |

- Loads that lie outside their host (STR-LOAD-001) are listed under C1's group as a separate *Load off host* issue, and the stale-results state of STR-RES-001 is not part of this check.
- Tolerances come from Structural Settings (STR-SET-001): support distance (default 300 mm), analytical-to-physical distance (150 mm), node merge distance (5 mm). The check stores the tolerance values it ran with in its report.
- **Issue identity.** Each issue has a stable key: check code plus the sorted GlobalIds of the elements involved. *Ignore* is stored against the key and survives re-checks, saves and BIMserver round trips; it is cleared automatically when any involved element changes geometry.
- **Before analysis.** `structural_analysis.run` (ANA-FEA-001) runs the consistency and support checks on its scope first. If errors (S3, C2) remain that are not ignored, the run is held and the user is asked *Run anyway* or *Review*; scripted runs fail with the issue list unless `options.ignore_check_errors` is true.
- Checks never change the model. They are read-only queries and leave the undo stack unchanged; only *Ignore* and *Un-ignore* are undoable model edits.
- Performance target: a full check of 10 000 analytical elements completes within 10 s on the reference machine; an incremental automatic check after a local edit completes within 1 s and never blocks the UI thread.

**Data (IFC mapping).** No IFC equivalent; check results are not exported. The last report per check (timestamp, scope, tolerances used, issue list with keys and GlobalIds) and the ignore list (key, reason, user, date) are stored in **RXD document** `model/analysis` (`checks.*`). Check switches and tolerances are part of the analytical settings in the same document (STR-SET-001).

**Commands.**
- `analytical_model.check_supports(scope?) -> report`
- `analytical_model.check_consistency(scope?) -> report`
- `analytical_model.ignore_issue(issue_key, ignore: bool, reason?)`

`scope` is `"model"` (default), `"selection"`, `"view:<view_id>"` or a list of element IDs. A report is `{run_at, tolerances, issues: [{key, code, severity, message, element_ids[], measured?, tolerance?, ignored}], counts_by_severity}`.

**Acceptance criteria.**
- [ ] A beam whose analytical end node stops 40 mm short of a column's analytical member is reported as C1 "Disconnected nodes" with the measured gap of 40 mm, and *Zoom to* frames the gap.
- [ ] A two-storey frame with no boundary conditions reports S3 and C2 as errors, and Run Analysis is held with a *Run anyway / Review* prompt.
- [ ] A beam supported at one end only is reported as S1; adding a pinned point boundary condition at the free end clears the issue on *Re-check*.
- [ ] Moving a physical beam 200 mm sideways without re-running automation reports C3 on its associated analytical member, with 200 mm against the 150 mm tolerance.
- [ ] An ignored C5 issue stays ignored after save, reopen and re-check, and reappears when the involved member is moved.
- [ ] With automatic consistency checks on, drawing a new analytical beam that misses a column by 20 mm adds one warning to the warnings list within 1 s of the commit.
- [ ] Running either check leaves the undo stack unchanged.

### 4.3.10 Manual analytical adjustment and reset
**ID:** STR-AM-008 · **Priority:** P2 · **Ref:** research/03 §4.2 ("Analytical Node", "Automation") · web: Modify an Analytical Model

**Purpose.** Let the engineer move analytical nodes, edges and whole analytical elements to idealise the model (for example to bring a beam's analytical line to a column centreline) without changing any physical element, and later restore the default position derived from the physical element.

**Workflow (adjust).**
1. Select an analytical member, panel or node (or a physical element with exactly one associated analytical element) and choose *Analyze › Structural Analytical Model › Adjust*, or *Adjust* on the contextual tab.
2. Reviera enters **Adjust mode**: every non-analytical element is drawn half-toned and cannot be selected; the analytical elements stay fully drawn.
3. Click a node to show a move control. For members the control is 3D, in the member's local coordinate system; **Spacebar** switches between local and global axes. For panels the control is 2D in the panel's plane; **Tab** selects an edge instead of a node; **Spacebar** cycles the panel's two local orientations and the global one.
4. Drag along an axis or type a distance in the temporary dimension. Nodes and edges snap to other analytical nodes, analytical members and panel edges, grids, levels and reference planes.
5. Drag an edge, or use Move, to shift a whole member or panel.
6. *Finish* commits all moves as one transaction ("Adjust analytical model"); *Cancel* discards them.

**Workflow (reset).** Select one or more analytical elements (or associated physical elements) and choose *Reset*. Each element returns to the default position that the active automation rule package (STR-AM-006) computes from its associated physical element.

**Behaviour & rules.**
- Physical elements never move in Adjust mode.
- End nodes are shared: moving a node moves the ends of every member and corner of every panel that uses it, and the analytical links attached to it. A node dragged onto another node within the node merge distance merges with it (STR-AM-003); the merge is listed in the status bar and is undone with the session if cancelled.
- Hosted nodes can only slide along or within their host.
- A node keeps its associated level: the level offset is recalculated so the node still follows later level moves.
- Hosted loads and boundary conditions follow their host; a point load or condition placed by position keeps its relative position along the member or panel.
- An element or node moved in Adjust mode is flagged **manually adjusted** (shown in Properties and available to filters and schedules). Automation updates skip flagged elements unless told to overwrite them (STR-AM-006).
- Moving a member's node so far that the analytical-to-physical distance is exceeded is allowed; the consistency check reports it (STR-AM-007, C3).
- **Reset** clears the manually adjusted flag and recomputes geometry from the physical element. It keeps the element's GlobalId, section, releases, loads and boundary conditions. Elements with no physical association have no default position: Reset refuses them and names them. Reset of a node shared with an element that is not being reset moves only the ends of the reset elements and disconnects the shared node; the result is reported so the user can re-run the consistency check.
- **Crash recovery.** Adjust mode is an interactive session (§2.1 of the conventions). Its micro-operation schema is: `enter(element_ids[])`, `pick(node_id | edge_ref)`, `set_cs(local_a | local_b | global)`, `move(target_ref, delta_vector)`, `snap(target_ref, snap_kind)`, `merge(node_a, node_b)`; `finish` and `cancel` close the session. Replay starts from the committed model state at `enter` and re-applies the moves in order, so an unfinished session is offered for resumption after a crash.

**Data (IFC mapping).** Adjusted geometry is written to the same IFC entities as before (STR-AM-001 to 003); no extra entity is created. The manually adjusted flag (per element and per node) and the hash of the default geometry it departed from are **RXD element data** (`analysis`). Unfinished Adjust sessions live only in the live model store's session journal (chapter 2b §6.5), not in the IFC file.

**Commands.**
- `analytical_model.adjust(element_id) -> tool_session` (interactive; the session commits on *Finish*)
- `analytical_node.move(node_ids[], delta, cs?: "global"|"local")` (the non-interactive equivalent for Lua and MCP; sets the manually adjusted flag)
- `analytical_model.reset(element_ids[])`

**Acceptance criteria.**
- [ ] In Adjust mode, moving a beam's end node 150 mm to a column's centreline moves the beam's analytical end and the column's top node together, leaves the physical beam and column unchanged, and creates one undo item on *Finish*.
- [ ] *Cancel* after three node moves leaves the analytical model exactly as before.
- [ ] Running Physical → Analytical on the adjusted beam without *Overwrite manual adjustments* leaves it in place and logs it as skipped; *Reset* then restores its default position and clears the flag.
- [ ] *Reset* on an analytical member with no physical association is refused and names the member.
- [ ] Killing the process during an Adjust session with two moves and restarting offers to resume the session with both moves re-applied.
- [ ] `analytical_node.move([n], {0,0,-100}, "global")` lowers node n by 100 mm and flags it as manually adjusted.

### 4.3.11 Structural analysis provider interface
**ID:** ANA-FEA-001 · **Priority:** P2 · **Ref:** research/03 §4.6

**Purpose.** Reviera shall run structural analyses through a pluggable **structural analysis provider**. This replaces the reference product's proprietary solver link.

**Workflow.**
1. Choose *Analyze › Structural Analysis › Run Analysis*.
2. Choose the provider, scope (whole model or selection), analysis type (linear static; modal and others where the provider supports them), self-weight inclusion, member offsets, code and units.
3. Click *Run*. The job runs in the background (ANA-GEN-001). A notification appears when it completes, and the result package is stored.

**Behaviour & rules.**
- **Pre-analysis checks.** `structural_analysis.run` first runs the analytical model support and consistency checks (STR-AM-007) on the analysed scope. The run is refused while check errors remain, unless `options.ignore_check_errors=true` is passed; the ignored errors are then recorded with the result set.
- **Exchange format:**
  - Reviera exports the analytical model as an **IFC4 Structural Analysis View** subset: members, panels, nodes, links, supports, loads, cases and combinations.
  - It also writes a compact neutral JSON (`reviera-structural-model.json`) for providers that do not read IFC.
- **Result package:** a provider returns a package with these parts:
  - a manifest (provider, version, analysis type, model snapshot hash)
  - **member forces** (Fx, Fy, Fz, Mx, My, Mz) at stations
  - **displacements** at nodes and panel mesh points
  - **reactions**
  - panel stresses and moments
  - optionally **required reinforcement** areas and **proposed sections**
- Packages are stored as `.reviera-results` archives that the model references (STR-RES-001). *Update model* applies proposed sections through STR-AM-006.
- **Partial model transfer:** the scope can be a selection. Nodes on the boundary of the selection get default supports, and the user is warned.
- **[Suggestion] engines**, each wrapped as a compute runner (ANA-PLUG-004, DM-RPC-002), with licences to verify before bundling:

| Engine | Scope | Notes |
|--------|-------|-------|
| CalculiX | General FEA | GPL-2.0-or-later; can be used under GPL-3 |
| Code_Aster | Advanced FEA | GPL-3.0 |
| Kratos Multiphysics | Multiphysics | BSD-style |
| PyNite | Frames and plates | MIT; a lightweight default for linear static frame analysis |
| xara (OpenSees-based) | Seismic and non-linear | BSD-2-Clause. OpenSees itself is not suggested: its licence needs a UC Berkeley licence for commercial redistribution and for revenue-earning project use |

**Data (IFC mapping).** Results summaries: `IfcStructuralResultGroup` (TheoryType `FIRST_ORDER_THEORY` and so on, ResultForLoadGroup → combination or case, IsLinear). Reactions are written as `IfcStructuralPointReaction` on supports. The full result data is referenced through `IfcDocumentReference` and `IfcRelAssociatesDocument`. The package index (path, hash, stale flag) and results caches are kept in **RXD document** `analysis`.

**Commands.**
- `structural_analysis.run(provider_id, scope, analysis_type, options) -> job_id`
- `structural_analysis.export_model(path, format: "ifc"|"json", scope)`
- `structural_analysis.import_results(path) -> package_id`
- `structural_analysis.update_model(package_id, apply_sections?: bool)`
- `analysis_provider.list(kind?) -> providers[]`

**Acceptance criteria.**
- [ ] With the PyNite suggestion installed, a simply supported 6 m beam under a 10 kN/m line load returns a mid-span moment of 45 kN·m ± 0.5%.
- [ ] Editing a load after the run marks the result package stale and shows a notice in Results Explorer.
- [ ] The exported IFC structural model opens in a third-party IFC structural viewer with all members, supports and load cases present.

### 4.3.12 Results Explorer and Results Manager
**ID:** STR-RES-001 · **Priority:** P2 · **Ref:** research/03 §4.6 · screenshots/structure_mep/structural_results_explorer_presets_2027.png · screenshots/structure_mep/structural_results_analysis_insights_2027.png

**Purpose.** Show and manage analysis results inside model views.

**UI.**
- **Results Explorer** is a dockable palette that is active when at least one result package exists. It contains:
  - package and analysis selection
  - case or combination selection
  - result type: member force diagrams for Fx–Mz, displacements (deformed shape), reactions, panel maps for stresses and moments, and required reinforcement
  - display style (ANA-ADS-001), units and scale
- **Result presets:** at least 13 named presets, for example:
  - column axial forces
  - column bending moments
  - support reactions
  - beam moments with deformation
  - beam shear
  - transfer beam forces
  - wall in-plane forces in X and Y
  - wall deflection
  - floor moments with deformation in X and Y
  - floor shear
  - slab twisting moments

  Users can save their own presets.
- **Insights tab:** extreme (minimum and maximum) values per selected result type, with zoom-to-element.
- **Results Manager:** lists, imports, exports and deletes packages.

**Behaviour & rules.** Results are view overlays and never change model geometry. A stale package shows a banner.

**Data (IFC mapping).** Display settings are stored with the view's own RXD data (chapter 5). Presets are kept in RXD document `model/analysis`.

**Commands.**
- `results.show(view_id, package_id, load_group_id, result_type, style_id?)`
- `results.apply_preset(view_id, preset_id)`
- `results.extremes(package_id, result_type, load_group_id) -> {min, max, element_ids}`
- `results.delete_package(package_id)`

**Acceptance criteria.**
- [ ] The "Support Reactions" preset shows reaction arrows with values at every support for the chosen combination.
- [ ] Insights lists the element with the maximum beam moment, and clicking it zooms to that beam.

---

## 4.4 Steel fabrication detailing

All features in this section are **P2**. Steel fabrication elements are shown **only at Fine detail** and need framing families that declare full **section shape parameters**. Reviera ships an *Upgrade Steel Families* utility that adds missing section parameters to older content.

Ref: research/03 §5.1

### 4.4.1 Steel detailing prerequisites and visibility
**ID:** STR-STL-001 · **Priority:** P2 · **Ref:** research/03 §5.1

**Purpose.** Set the conditions under which steel fabrication elements can be created and shown, and give users a way to bring older framing content up to those conditions.

**Behaviour & rules.**
- Plates, bolts, welds, cuts and connection sub-elements are drawn only at Fine detail. At Coarse and Medium detail a connection shows its symbol (STR-CONN-001). The elements always exist in the model: they are scheduled, counted by numbering (STR-STL-010) and exported to IFC whatever the view's detail level.
- **Supported sections.** Steel tools accept framing and columns whose type declares a **section shape** (I/H, channel, angle, T, rectangular and circular hollow, flat bar, cold-formed C and Z) with its section parameters (depth, width, web and flange thickness, root radius, and so on). Members without them are refused by connection, cut and fastener tools with a message that names the member and suggests the upgrade utility.
- **Upgrade utility** (*Steel › Upgrade Steel Families*, `steel_families.upgrade`): scans framing and column families, infers the section shape from the profile geometry and type names, adds the missing section parameters, and fills them from the profile. Families it cannot classify are listed with the reason and left unchanged. The report lists upgraded, skipped and failed families. The run is one undoable transaction.
- When placement fails because of an unsupported shape, the wrong input order or parameters that do not fit, Reviera explains the cause specifically. Tooltips on each connection type describe the required input order.
- **Unlike the reference product**, Reviera's interference check (ANA-CLASH-001) **includes** detailed steel elements and fabrication parts. See §4.17.
- The connection library is stored as **open files**, not in a local database server.
- Steel elements of the Steel tab need a structural material whose model behaviour is steel; other materials are refused with a message.

**Data (IFC mapping).** Section shape and parameters map to the type's `IfcMaterialProfileSet` with a parametrised profile (`IfcIShapeProfileDef`, `IfcUShapeProfileDef`, `IfcLShapeProfileDef`, `IfcTShapeProfileDef`, `IfcRectangleHollowProfileDef`, `IfcCircleHollowProfileDef`, `IfcCShapeProfileDef`, `IfcZShapeProfileDef`); shapes with no parametrised IFC profile use `IfcArbitraryClosedProfileDef` plus the section parameters in RXD family recipe data.

**Commands.** `steel_families.upgrade(family_ids[] | "all") -> report`

**Acceptance criteria.**
- [ ] A bolt group visible at Fine is not drawn at Medium in the same view, and the connection symbol is drawn instead.
- [ ] A bolt group is counted in a schedule and exported to IFC when every view in the model is at Coarse detail.
- [ ] Placing a shear-plate connection on a beam whose family lacks section parameters is refused with a message naming the beam and the upgrade utility.
- [ ] Upgrading a legacy W-shape family adds its section parameters with values equal to the profile dimensions, and the type then exports an `IfcIShapeProfileDef`.

### 4.4.2 Open parametric connection library
**ID:** STR-STL-002 · **Priority:** P2 · **Ref:** research/03 §5.2

**Purpose.** Reviera needs its own library of standard steel connections, because the reference product's connection engine and content are proprietary.

**Behaviour & rules.**
- **Library format:** a connection type is a **package** containing:
  - a manifest (name, group, SPDX licence, version)
  - an **input signature**: member count (1–3 inputs for standard types), member roles (main, secondary, brace), and supported section families (I, H, channel, angle, tube, CHS, flat)
  - a **parameter schema**: plates, bolts, welds, stiffeners and properties, grouped by tab
  - a **Lua generator** that produces plates, fasteners, welds and cuts from the inputs and parameters
  - optional **ranges of applicability**: rules on profile size and geometry
- **Initial library target** (first release): grouped like the reference product's families, but authored from first principles. Groups and examples:

| Group | Types |
|-------|-------|
| Base plates | Pinned and moment base plates with anchors |
| Beam–column | Shear plate (fin plate), end plate (flush and extended), double clip angle, seated beam, moment flange plates |
| Beam–beam | Notched shear plate, clip angle, splice (flange and web plates), end-plate splice |
| Bracing | Gusset plates for 1–4 diagonals, flat bracing with tension bolt, tube-end slotted plate |
| Column | Bolted and welded splices, stiffeners, web doubler, cap plate |
| Frames | Apex haunch, knee (eaves) haunch |

- **Modify Parameters dialog:** tabs generated from the parameter schema, with live 3D preview. The same dialog edits types and instance overrides.
- *Connection Library* (the launcher on *Steel › Connection*) loads and unloads types, filtered by group. Community libraries can be added if their licence is GPL-compatible (TECH_STACK §4.2).

**Data (IFC mapping).** See STR-CONN-001. Library packages are external, but the generated sub-elements are fully written to IFC, so a model opens without the library. The library package and version are recorded in RXD element data (`structural`), and the type recipe in an RXD family recipe.

**Commands.**
- `connection_library.install(path)`
- `connection_type.edit_parameters(type_id, params)`
- `structural_connection.override(connection_id, params)`

**Acceptance criteria.**
- [ ] A shear-plate connection between a W-beam and a column flange creates a plate, a bolt group and welds at Fine detail.
- [ ] Changing the bolt row count from 3 to 4 in the dialog regenerates the plate height.
- [ ] A model whose library package has been removed still opens and shows all plates and bolts, with a warning that the connection cannot be edited.

### 4.4.3 Custom connections
**ID:** STR-STL-003 · **Priority:** P2 · **Ref:** research/03 §5.2 ("Custom connections") · screenshots/structure_mep/steel_custom_connections.png

**Workflow.**
1. Select one or more standard or generic connections and their loose fabrication elements.
2. Choose *Customize*.
3. Name the connection, then **Add** or **Remove** sub-elements: plates, profiles, bolts, welds, parametric cuts and nested standard connections (stiffeners excepted).
4. Finish.

**UI.** While customizing, Reviera is in a **custom connection edit mode**: the members and the selected elements stay fully drawn, everything else is half-toned, and the contextual tab offers *Add*, *Remove*, *Finish* and *Cancel*. The name is asked for first and must be unique among loaded connection types. Each *Add* or *Remove* pick is journaled as a micro-operation, so an unfinished session is recovered after a crash (§2.1 of the conventions); replay re-applies the picks in order.

**Behaviour & rules.**
- A custom connection allows at most **three connected members**. A fourth member in the selection is refused with a message.
- The result is a new connection type. Nested parameters can be exposed as type parameters.
- Welds cannot be attached to sub-elements of custom connections. They are allowed on sub-elements of standard connections.
- **What is stored.** The type records each sub-element relative to the main member's local coordinate system (position, orientation, and the member face or end it is related to), plus the nested standard connections with their parameters. When placed on another join, sub-elements are re-positioned relative to that join's main member; nested standard connections regenerate for the new members.
- **Matching.** A custom type can be placed only on joins with the same member count and roles and the same section families (for example I to I flange). Other joins are refused with the reason.
- **Editing the type.** *Add* and *Remove* on an existing custom type change every instance of that type after a confirmation that states the instance count.
- **Exposed parameters** (`expose_parameter`) appear in Modify Parameters (STR-STL-002) as type parameters with the nested element's current value as default; a path is `<sub-element name>.<parameter>`. An invalid path is refused.
- Loose elements used to build the type are absorbed into the first instance; they are not duplicated.
- Custom types are stored in the project. They can be exported as a connection-library package (STR-STL-002) for reuse in other projects.

**Data (IFC mapping).** Each instance is an `IfcElementAssembly` (`USERDEFINED` "STRUCTURAL_CONNECTION") like any connection (STR-CONN-001), with its plates, fasteners, welds and cuts written in full. The type recipe (sub-element list, relative placements, nested connections, exposed parameters) is an RXD family recipe; the instance records its type in RXD element data (`structural`).

**Commands.**
- `structural_connection.customize(connection_ids[], element_ids[], name) -> type_id`
- `custom_connection.add_elements(type_id, element_ids[])`
- `custom_connection.remove_elements(type_id, element_ids[])`
- `custom_connection.expose_parameter(type_id, path)`

**Acceptance criteria.**
- [ ] A custom connection built from an end plate and an extra stiffener can be placed on another matching join, where it recreates both.
- [ ] Customizing a selection with four connected members is refused with a message.
- [ ] Removing a plate from a custom type used by 5 instances asks for confirmation with the count, then removes the plate from all 5.
- [ ] Exposing `Stiffener 1.Thickness` adds a type parameter whose change from 10 mm to 12 mm updates every instance.

### 4.4.4 Connection automation
**ID:** STR-STL-004 · **Priority:** P2 · **Ref:** research/03 §5.2 ("Connection automation")

**Purpose.** Place connections on many joins at once from rules, instead of one by one.

**Workflow.**
1. Select framing members or a region, or nothing for the whole model, and choose *Steel › Connection › Automate*.
2. In the dialog choose the **rule set** (the project default, or a loaded Lua or JSON rule set), what to do with joins that already have a connection (*Skip*, the default, or *Replace generic only*), and whether to run in **preview** mode.
3. Run. The job runs in the background (ANA-GEN-001). Preview shows the proposed type per join, colour-coded in the active view and listed in the log, without changing the model; *Apply* then commits it.

**Behaviour & rules.**
- *Automate Connections* applies connection types to the selected joins, or to all joins, using ranges of applicability.
- **Joins** are found from framing end joins and member intersections within 1 mm, classified by member count, roles (main, secondary, brace) and section families.
- Each join gets the first type whose rules match.
  - A rule set is an ordered list of rules. Each rule names a connection type and conditions on member roles, section families, profile size ranges (for example depth 200–400 mm), angles between members and material grade.
  - Ranges of applicability stored on library types (STR-STL-002) are used when the rule set does not override them.
- Joins with an existing detailed connection are never changed. Joins where the generator fails keep a generic connection with a *Failed* status (STR-CONN-001).
- A log lists placed, skipped and failed joins with reasons. It can be saved as CSV.
- Rule sets that map profile sizes to connection types are Lua or JSON tables.
- The applied run is one undoable transaction; its log is kept with the run history.

**Data (IFC mapping).** Placed connections as in STR-CONN-001. Rule sets are project-level data in RXD document `model/structural` (`settings.connection_rules`), or external files referenced by path; run logs (date, rule set, counts) are kept there too.

**Commands.** `structural_connection.automate(scope, rule_set_id?, options?: {existing: "skip"|"replace_generic", preview: bool}) -> report`. The report lists `{join, members[], type_id?, status: placed|skipped|failed, reason}` per join. A preview report can be applied with `structural_connection.apply_automation(report_id)`.

**Acceptance criteria.**
- [ ] Automating 200 beam–column joins places a type on every join whose profiles fall within a range, and logs the rest.
- [ ] Re-running automation on the same joins with *Skip* changes nothing and logs all 200 as skipped (already connected).
- [ ] Preview mode leaves the undo stack and model unchanged until *Apply*.
- [ ] The log saved as CSV has one row per join with status and reason.

### 4.4.5 Plates
**ID:** STR-STL-005 · **Priority:** P2 · **Ref:** research/03 §5.3

**Workflow.**
1. Choose *Steel › Plate*. Pick the placement plane: the current work plane, or a planar face of a steel element.
2. Sketch a single closed contour with lines and arcs (rectangle, polygon and circle tools are offered). Each sketch operation is journaled as a micro-operation, so an unfinished sketch is recovered after a crash (§2.1 of the conventions).
3. Finish. Reviera validates the contour and creates the plate.

**Behaviour & rules.**
- Parameters: thickness (default 10 mm, 1–200 mm), material (steel grade), coating and justification (*Top*, *Centre* or *Bottom* relative to the sketch plane), plus a free *Part role* (for example stiffener, gusset, cap plate) used by numbering and schedules.
- The contour must be one closed, non-self-intersecting loop. Several loops or gaps are refused and the offending segment is highlighted. Holes in plates are made with hole patterns or contour cuts (STR-STL-006, STR-STL-008), not inner loops.
- Arcs in the contour are approximated by corner cuts.
- A plate sketched on a face is hosted by that face: it moves with the element and its sketch plane updates when the face moves. If the face disappears the plate stays in place, becomes unhosted and a warning names it.
- *Edit Contour* reopens the sketch. Corner cuts (STR-STL-008) can be applied afterwards.
- Folded plates are produced only by connections. After *Break* they become independent, and their sketch can no longer be edited.
- Plates are drawn only at Fine detail (STR-STL-001), and are valid targets for bolts, welds and cuts.

**Data (IFC mapping).** `IfcPlate` (PredefinedType `SHEET`, or `USERDEFINED` "FOLDED"), typed by `IfcPlateType`. `Pset_PlateCommon`. Geometry: an extruded area solid from the contour. Material: `IfcMaterialLayerSetUsage` with one layer of the plate thickness. The editable contour, justification and host face reference are RXD element data (`constraints`).

**Commands.** `plate.create(work_plane | face_ref, contour_curves[], thickness, material_id, justification) -> id`

**Acceptance criteria.**
- [ ] A plate sketched on a column flange face exports as `IfcPlate` with the correct thickness.
- [ ] A contour with a gap of 2 mm is refused at *Finish* and the open segment end is highlighted.
- [ ] A 15 mm plate with *Bottom* justification on a flange face sits fully outside the flange, with its bottom face on the flange face.
- [ ] Moving the hosting column moves the plate with it.

### 4.4.6 Bolts, anchors, holes and shear studs
**ID:** STR-STL-006 · **Priority:** P2 · **Ref:** research/03 §5.3 · screenshots/structure_mep/steel_fabrication_elements_plate_anchors.png

**Workflow.**
1. Select the steel elements to join with Ctrl+click, then press Enter.
2. Pick the face the pattern is perpendicular to.
3. Sketch a **rectangular** or **circular** pattern. Finish.

**Behaviour & rules.**
- **Bolts:**
  - standard, grade, diameter and assembly (for example bolt + nut + 2 washers)
  - bolt length and **grip length**, both calculated
  - grip length increase
  - **hole definition** per connected element (type, tolerance, slotted)
  - inverted
  - finish calculation at gap
  - location (site or shop) and coating
  - count and spacing per direction, and edge distances
- **Anchors:** the same flow, between concrete and steel. Parameters: standard, length and grade.
- **Holes:** a pattern without fasteners.
- **Shear studs:** a pattern on a steel element. Parameters: standard, diameter and length.
- Bolt standards are open data tables (dimensions per standard). Reviera ships tables only where their content is not copyrighted, and users can add their own.
- The shape of a pattern can be edited after placement.

**Data (IFC mapping).**

| Element | IFC |
|---------|-----|
| Bolt | `IfcMechanicalFastener` `BOLT`, NominalDiameter, NominalLength; `Pset_MechanicalFastenerBolt` (ThreadDiameter, ThreadLength, NutsCount, WashersCount, …) |
| Anchor | `IfcMechanicalFastener` `ANCHORBOLT` |
| Shear stud | `IfcMechanicalFastener` `STUDSHEARCONNECTOR` |
| Hole | `IfcOpeningElement` `OPENING` with `IfcRelVoidsElement` in each pierced element |

A pattern is an `IfcElementAssembly` `USERDEFINED` "FASTENER_GROUP" that aggregates its fasteners, or a single fastener with `IfcMappedItem` copies. The joined elements are related with `IfcRelConnectsWithRealizingElements`. Pattern parameters (kind, counts, spacings, grip, location, hole definitions) are RXD element data (`constraints`).

**Commands.**
- `bolt_pattern.create(element_ids[], face_ref, pattern: "rect"|"circle", params) -> id`
- `anchor_pattern.create(...)`
- `hole_pattern.create(...)`
- `shear_stud_pattern.create(...)`

**Acceptance criteria.**
- [ ] A 2 × 3 M20 bolt pattern through a 15 mm plate and a 10 mm web reports a grip length of 25 mm and creates 6 holes in each element.
- [ ] An exported bolt is an `IfcMechanicalFastener` with `NominalDiameter` = 20 mm.

### 4.4.7 Welds
**ID:** STR-STL-007 · **Priority:** P2 · **Ref:** research/03 §5.3

**Purpose.** Record shop and site welds between steel elements, with the data a fabricator needs.

**Workflow.** Choose *Steel › Welds*. Select the elements to weld (two or more steel elements, Ctrl+click), press Enter, then pick an available **edge**. Only edges where the selected elements touch within 1 mm are offered and highlighted. A cross symbol represents the weld. Picks are journaled as micro-operations (§2.1 of the conventions).

**Behaviour & rules.**
- Parameters: main and double weld type, thickness, location (site or shop), surface shape and preparation. Values come from data tables, not free text.
  - Weld types include fillet, butt (square, single-V, single-bevel, double-V), plug and spot; surface shapes flat, convex and concave. The tables are open data files that users can extend.
  - Thickness is the throat (a) or leg size, > 0 mm; length defaults to the picked edge and can be shortened; an *Intermittent* option takes segment length and pitch.
- Welds **do not move** with their elements. When geometry changes they are flagged for review.
  - Review means: the weld keeps its position, a *needs review* status appears in Properties and in the warnings list, and *Accept* clears it once the user has checked or re-picked the edge.
  - If one of the welded elements is deleted, the weld is deleted with it (STR-GEN-002).
- Welds cannot be placed on sub-elements of custom connections (STR-STL-003).
- Shop welds join elements into one **assembly** for assembly marks (STR-STL-010); site welds do not.
- Welds are drawn only at Fine detail. Weld symbols in drawings are annotations of chapter 5 that read the weld parameters.

**Data (IFC mapping).** `IfcFastener` with PredefinedType `WELD`, and `Pset_FastenerWeld` (Type1, Type2, Surface1, Surface2, Process, a, l, Intermittent, …). Related with `IfcRelConnectsWithRealizingElements`. The picked edge reference, location (site or shop) and review status are RXD element data (`structural`).

**Commands.** `weld.create(element_ids[], edge_ref, params) -> id`

**Acceptance criteria.**
- [ ] Moving a plate with a weld marks the weld "needs review" and leaves its position unchanged.
- [ ] Picking an edge where the selected elements do not touch is not possible: the edge is not highlighted.
- [ ] A 6 mm fillet weld exports as `IfcFastener` (`WELD`) with `Pset_FastenerWeld.a` = 6 mm.
- [ ] A plate shop-welded to a beam gets the beam's assembly mark; the same plate site-welded does not.

### 4.4.8 Modifiers and parametric cuts
**ID:** STR-STL-008 · **Priority:** P2 · **Ref:** research/03 §5.4

| Tool | Behaviour | Command |
|------|-----------|---------|
| **Corner Cut** (plates) | Convex or concave, straight or radius | `steel_modifier.corner_cut(plate_id, corner_ref, kind, size)` |
| **Cope Skewed** (framing) | Skewed cope at a member end | `steel_modifier.cope_skewed(member_id, end, params)` |
| **Shorten** | A cut at the nearest end; positive shortens, negative extends; angles across height and width give diagonal cuts | `steel_modifier.shorten(member_id, end, length, angle_h?, angle_w?)` |
| **Contour Cut** | A sketched hole or notch through framing or plates, with gap width, radius, boring and boundary distances | `steel_modifier.contour_cut(element_id, face_ref, curves[], params)` |
| **Cope** (parametric) | Select intersecting members, then Enter. A swap control chooses which member is cut. | `steel_cut.cope(member_ids[], cut_member_id?)` |
| **Miter**, **Saw Cut – Flange**, **Saw Cut – Web** | End cuts between two members | `steel_cut.miter(a, b)`, `steel_cut.saw(a, b, part: "flange"|"web")` |
| **Cut Through** | Cuts one element by the shape of another, plus a gap | `steel_cut.cut_through(cut_id, cutter_id, gap)` |
| **Cut By** | Cuts by the intersecting geometry | `steel_cut.cut_by(cut_id, cutter_id)` |

**Behaviour & rules.**
- Parametric cuts form a **sub-connection** (shown as a dashed box) that cannot be retyped.
- Modifiers and cuts regenerate when their members move. Void cuts and connections can be combined.
- Shape handles extend a member beyond its shorten modifier.
- **Uncope** (*Parametric Cuts › Uncope*, `steel_cut.uncope(member_ids[])`): select one or more members that carry copes, then press Enter. Every parametric cope on those members is removed and their ends return to the uncut shape. A sub-connection left with no cuts is deleted; one that still holds other cuts (for example a miter) is kept. Copes that belong to a detailed connection (STR-CONN-001) are not removed; they are listed as skipped, because editing them belongs to the connection's parameters. Selecting members with no copes is reported as "nothing to remove" and makes no transaction.
- Deleting a cut opening directly (selecting its dashed box contents) has the same effect for that single cut.

**Data (IFC mapping).** Cuts are `IfcOpeningElement` (PredefinedType `OPENING`, or `USERDEFINED` "COPE", "MITER", …) with `IfcRelVoidsElement`. Their parameters are RXD element data (`constraints`). A sub-connection is an `IfcElementAssembly` `USERDEFINED` "SUB_CONNECTION".

**Commands.** The commands are listed in the table above, plus `steel_cut.uncope(member_ids[])`.

**Acceptance criteria.**
- [ ] A cope between a secondary beam and a girder removes the girder's flange zone from the secondary beam's end and updates when the girder depth changes.
- [ ] *Uncope* on that secondary beam restores its full end profile, deletes the now-empty sub-connection and is undone in one step.
- [ ] *Uncope* on a beam whose cope belongs to a shear-plate connection leaves the cope and reports it as skipped.

### 4.4.9 Connection code checking (provider)
**ID:** STR-STL-009 · **Priority:** P2 · **Ref:** research/03 §2.9 ("Code checking"), §5.2

**Purpose.** Check detailed connections against a design code, and size them, through a pluggable provider rather than a built-in proprietary checker.

**Workflow.**
1. Select one or more detailed connections whose instance has *Override by Instance* on (STR-CONN-001), so that the code-check tab is available.
2. In Modify Parameters, open the code-check tab: choose the provider, the design code the provider offers (for example a Eurocode 3 or AISC 360 module) and the design forces. Forces come from the connection's *Design Forces* property per load case or combination, or from the member end forces of a result package (STR-RES-001).
3. Choose *Check*, *Presize* or *Report*. Checks run in the background (ANA-GEN-001).

**Behaviour & rules.**
- *Check*, *Presize* and *Report* on connections go through a **connection design provider** (ANA-PLUG-001 kind `structural`, capability `connection_check`).
- **Provider input:** member sections and materials, connection type and parameters (plates, bolts, welds), geometry of the join, and the design forces per load group. **Output:** per check item (for example bolt shear, bearing, block tearing, weld resistance) the resistance, the action, the utilisation ratio and pass or fail, plus an overall status and a report file (HTML or PDF).
- Presize grows bolt diameter and plate and weld thickness step by step until the check passes.
  - Steps follow the bolt and plate tables (STR-STL-006). Presize stops with *Failed* after the largest size, or after 20 iterations, and reports which item still fails.
  - Presize changes connection parameters, so it is an undoable model edit; the provider never edits the model directly.
- Status (NotCalculated, Failed or OK) and the report path are written to RXD element data (`structural`).
- Any later change to the connection, its members or its design forces resets the status to *NotCalculated* (stale).
- Connections without design forces are refused with a message naming them.
- Reviera does not bundle a code checker in the first release. The provider interface and a sample Lua provider for simple bolt shear and bearing checks are supplied.

**Data (IFC mapping).** No IFC equivalent for check results. Status, utilisation (the maximum ratio), code, provider, date and report path are RXD element data (`structural`) on the connection. The report file is referenced from the model with `IfcDocumentReference` through `IfcRelAssociatesDocument`.

**Commands.**
- `structural_connection.check(connection_ids[], provider_id, code) -> report`
- `structural_connection.presize(connection_ids[], provider_id, code)`

**Acceptance criteria.**
- [ ] With the sample provider, a bolt group whose design shear exceeds its capacity gets status Failed and a report that shows the utilisation ratio.
- [ ] Presize on that connection increases the bolt diameter until the utilisation is ≤ 1.0, sets status OK, and is undone in one step.
- [ ] Changing the beam size after a successful check resets the status to *NotCalculated*.
- [ ] Checking a connection with no design forces is refused with a message naming it.

### 4.4.10 Steel interoperability and fabrication output
**ID:** STR-STL-010 · **Priority:** P2 · **Ref:** research/03 §5.5

**Behaviour & rules.**
- **IFC is the primary exchange format** with detailing software. Members, plates, fasteners, welds, openings and connection assemblies all round-trip as described above.
- **Part and assembly marks:**
  - *Steel › Numbering* assigns part marks to identical parts and assembly marks to identical assemblies (main member plus everything welded to it in the shop).
  - The part mark is written to `IfcElement.Tag`. The assembly mark is written to the assembly's `Tag`. A copy of both is kept in RXD element data (`structural`) with the numbering options.
  - An assembly is an `IfcElementAssembly` `USERDEFINED` "STEEL_ASSEMBLY".
- **[Suggestion] NC export** as a plugin: per-part CNC files in the openly documented DSTV NC1 format, generated from member profiles, holes, cuts and plates.
- Proprietary exchange formats used by the reference product's detailing integration are **not** supported (§4.17).

**Commands.**
- `steel_numbering.run(scope, options) -> report`
- `steel_nc.export(scope, folder, format="dstv")` (plugin)

**Acceptance criteria.**
- [ ] Two identical beams with identical holes get the same part mark. Adding one hole to one beam gives it a new mark.

---

## 4.5 Concrete detailing and precast

All features in this section are **P2**. Precast tools are on *Concrete › Precast Detailing* and *Precast Settings*. Precast is a **rule-driven pipeline**:
1. split walls and slabs into pieces
2. reinforce the pieces and add mounting parts
3. generate shop drawings
4. export CAM data

Ref: research/03 §6.1

### 4.5.1 Precast configuration
**ID:** STR-PRE-001 · **Priority:** P2 · **Ref:** research/03 §6.1

**Purpose.** One tree-structured dialog holds all precast rules.

**UI.** *Concrete › Precast Settings › Configuration* (`precast_config.show()`). Tree:
- Concrete
  - Wall: Solid Wall, Double Wall
  - Slab: Solid, Hollow-core, Lattice-girder slab

  Each with Segmentation, Reinforcement, Connections (mounting parts) and Shop Drawing nodes.
- Built-in parts: Annotations, Dimensioning
- CAM Export
- Data folder path for templates and families

**What each node edits:**

| Node | Contents | Validation |
|------|----------|------------|
| Segmentation | The rules of STR-PRE-002: minimum and maximum length or width and height, maximum weight, joint gaps, minimum distance to openings, profile overlap; for hollow-core, the slab family, division profile, minimum width and strand diameter | Minimum ≤ maximum; gaps 0–100 mm; maximum weight > 0 t; families must be loaded |
| Reinforcement | Reinforcement types per thickness range (area, edge, lattice girder), as used by STR-PRE-003; custom fabric sheet rules (STR-PRE-004) | Thickness ranges must not overlap; bar and mesh types must exist |
| Connections | Mounting-part rules (STR-PRE-005): part family, count by weight bands, edge distances, symmetry | Weight bands must be contiguous and ascending |
| Shop Drawing | Sheet template, centre-of-gravity symbol, dimension type and distances, descriptions (STR-PRE-006) | Template must contain the required viewports |
| Annotations / Dimensioning | Tag family per category, dimension rules per category | Tag families must be loaded |
| CAM Export | Writer per format, output folder and filename rule (STR-PRE-007) | Filename rule must parse |
| Data folder | Path to templates and families | Path must exist and be readable |

- Invalid values are marked in the tree node and in the cell, and *OK* is disabled until they are fixed.
- *OK* commits as one transaction. The dialog shows how many existing assemblies were made with an older configuration version; Reviera does not re-split or re-reinforce them automatically, but offers *Re-run on affected assemblies* (STR-PRE-002 `precast.resplit`, STR-PRE-003).
- *Export…* and *Import…* buttons call the commands below. Import replaces the whole configuration after a confirmation; references to families not loaded in the project are listed so they can be loaded.
- A new project's configuration comes from its template; Reviera ships an open default configuration.

**Data (IFC mapping).** No IFC equivalent. **RXD document** `fabrication` (precast configuration section), versioned. It can be exported and imported as a standalone file to share between projects. Each version increments on commit; the version used is recorded on each precast assembly (RXD element data `structural`, rule snapshot hash, STR-PRE-002). The standalone file is JSON with a schema version.

**Commands.**
- `precast_config.get(path) -> value`
- `precast_config.set(path, value)`
- `precast_config.export(file)`
- `precast_config.import(file)`
- `precast_config.show()`

`path` addresses a node and key, for example `wall.solid.segmentation.max_length`. `set` applies the dialog's validation and returns an error naming the rule.

**Acceptance criteria.**
- [ ] A configuration exported from one project and imported into another produces identical segmentation results on an identical wall.
- [ ] Entering a minimum wall length of 9 m with a maximum of 8 m marks the Segmentation node invalid and disables *OK*.
- [ ] After changing the maximum weight, the dialog reports how many existing assemblies used the previous version and offers to re-run on them.
- [ ] `precast_config.set("wall.solid.segmentation.max_length", -1)` fails with a message naming the rule.

### 4.5.2 Segmentation (split)
**ID:** STR-PRE-002 · **Priority:** P2 · **Ref:** research/03 §6.2

**Workflow.**
1. Choose *Split*, optionally with *Multiple*.
2. Select walls, floors or foundation slabs, then Finish.
3. For floors, choose the slab kind: **Hollow-core**, **Solid** or **Lattice-girder**.

**Behaviour & rules.**
- **What Split does:** creates parts (chapter 3 parts engine), divides them by the rules, places mounting parts and reinforcement (when automatic updates are on), and creates one **assembly per piece**.
- **Wall rules:** minimum and maximum length, minimum and maximum height, **maximum weight**, horizontal and vertical joint gap, minimum distance to openings, and profile overlap height.
- **Slab rules:** minimum and maximum width, maximum weight and joint gap. Hollow-core slabs add the hollow-core family, a **division profile**, minimum width, segmentation gap and strand diameter.
- **Re-splitting:** after the source boundary changes, Split is re-run on the **assembly**. If it is run on the source element instead, Reviera detects the existing split and offers to redirect.
- **Automatic updates** (a toggle) controls whether mounting parts and reinforcement are re-created when pieces change.
- **Double walls:** a wall type with three layers (concrete, thermal or air, concrete) is split and reinforced as one double-wall piece.
- **Independent lattice girders** can be sketched on lattice-girder slab assemblies.

**Data (IFC mapping).**
- **Initial version (owner decision, 2026-10-12; open question Q13).** The IFC relationship between the source wall or slab and its pieces is **not written**. IFC allows a piece only one decomposition parent, and that parent is its piece assembly (below). The source-to-piece link is kept in RXD element data only. Which IFC relationship, if any, carries it is decided later on expert advice.
- Each piece is an `IfcWall` or `IfcSlab` (or an `IfcBuildingElementPart` with `PRECASTPANEL`) inside an `IfcElementAssembly` (`USERDEFINED` "PRECAST_ASSEMBLY") together with its reinforcement and mounting parts.
- `Pset_PrecastConcreteElementGeneral` and `Pset_PrecastConcreteElementFabrication` (TypeDesignator, PieceMark, ProductionLotId, SerialNumber).
- **RXD element data** (`structural`) holds the source element GlobalId, slab kind and rule snapshot hash.

**Commands.**
- `precast.split(element_ids[], slab_kind?) -> assembly_ids[]`
- `precast.resplit(assembly_ids[])`
- `precast.set_auto_update(bool)`

**Acceptance criteria.**
- [ ] A 30 m × 3 m wall with maximum length 8 m and maximum weight 12 t is split into the fewest pieces that satisfy both limits, with the configured gaps.
- [ ] No joint lies closer to an opening than the minimum distance.
- [ ] Each piece exports as an `IfcElementAssembly` that contains the concrete element and its reinforcement.

### 4.5.3 Automatic precast reinforcement
**ID:** STR-PRE-003 · **Priority:** P2 · **Ref:** research/03 §6.3 · screenshots/structure_mep/precast_wall_auto_reinforcement.png

**Workflow.** Choose *Auto Reinforce*. Select an assembly, or several assemblies of the same kind. Choose the **area reinforcement type** and, for walls, the **edge reinforcement type**. Click OK.

**Behaviour & rules.**
- Reinforcement types are defined per wall or slab thickness. Each covers:
  - area bars or mesh
  - edge bars around openings and edges (for example U-bars)
  - lattice girders for double walls and girder slabs
- Editing a reinforcement type changes every default that refers to it. Reviera offers to re-run on the affected assemblies.
- The dialog offers only the reinforcement types whose thickness range contains the piece thickness. Selecting assemblies of different kinds (for example a wall and a slab) is refused.
- **Re-running** on an assembly that is already reinforced replaces only the reinforcement that the previous run generated (marked as generated, with the run's rule snapshot). Bars the user added by hand are kept; generated bars the user edited are kept and reported, so manual work is never lost silently.
- With automatic updates on (STR-PRE-002), reinforcement is regenerated whenever a piece's geometry changes; with them off, the assembly is flagged as out of date.
- Area reinforcement respects the host cover (STR-RBR-002) and is cut around openings with the configured clearance; edge bars wrap openings and free edges with the configured lap; lattice girders are laid parallel to the span at the configured spacing and edge distance.
- Generated reinforcement gets partition = the assembly's piece mark, so numbering (STR-RBR-016) is per piece.
- The report lists, per assembly, the bars, sheets and girders created, kept or skipped, and why.

**Data (IFC mapping).** Generated rebar and mesh as in §4.2, aggregated into the piece assembly. Lattice girders are `IfcReinforcingElement` subtypes: `IfcReinforcingBar` chords and diagonals grouped in an `IfcElementAssembly` `USERDEFINED` "LATTICE_GIRDER". The generated flag, the reinforcement type used and the rule snapshot hash are RXD element data (`rebar`) on each generated element.

**Commands.** `precast.reinforce(assembly_ids[], area_type_id, edge_type_id?) -> report`

**Acceptance criteria.**
- [ ] Reinforcing a wall piece with a 1.2 × 2.1 m door opening places U-bars around the opening edges and mesh cut around it.
- [ ] Re-running on that piece after the user added one extra bar keeps the extra bar and replaces only generated reinforcement.
- [ ] A 200 mm wall piece is offered only reinforcement types whose thickness range contains 200 mm.
- [ ] Generated bars carry the piece mark as their partition.

### 4.5.4 Custom fabric sheets
**ID:** STR-PRE-004 · **Priority:** P2 · **Ref:** research/03 §3.5, §6.3

**Purpose.** Generate a fabric sheet shaped to each precast piece, instead of cutting standard sheets, so that the plant can produce one welded mesh per piece.

**Workflow.** Select one or more precast assemblies and choose *Concrete › Precast Detailing › Custom Fabric Sheets*, optionally choosing a rule set other than the default. Rules are edited in *Precast Settings › Custom Fabric Sheet Rules* (part of the precast configuration, STR-PRE-001).

**Behaviour & rules.**
- Rule-generated custom sheets are cut around openings. The rules are wire spacing, edge distances, opening clearances and maximum sheet size, set in *Custom Fabric Sheet Rules*. The output is `IfcReinforcingMesh` with per-sheet wire geometry.
- **Rule set contents:** wire diameter and spacing per direction, edge distance per edge kind (outer edge, joint edge), clearance around openings, minimum wire length (shorter wires are dropped), maximum sheet length and width (default 6.0 × 2.5 m), and which layer (near or far face) to fill.
- A piece larger than the maximum sheet size gets several sheets, split along a line perpendicular to the main wires, with the configured lap.
- Wires stop at the clearance line around each opening; wires that would be shorter than the minimum length are omitted, and the omitted count is reported.
- Sheets are regenerated when the piece changes if automatic updates are on (STR-PRE-002); otherwise the piece is flagged as out of date.
- Each sheet is numbered like any fabric sheet (STR-RBR-016); two pieces with identical sheets share a number.

**Data (IFC mapping).** One `IfcReinforcingMesh` per sheet, aggregated into the piece assembly, with MeshLength, MeshWidth, the longitudinal and transverse bar diameters and spacings, and one swept body per wire. The rule set id and the generated flag are RXD element data (`rebar`); rule sets are part of the precast configuration in RXD document `fabrication`.

**Commands.** `precast.generate_custom_fabric(assembly_ids[], rule_id?)`

**Acceptance criteria.**
- [ ] A slab piece with two openings receives one custom sheet whose wires stop at the configured clearance from each opening.
- [ ] A 9 m × 2.4 m wall piece with a 6 m maximum sheet length receives two sheets lapped by the configured lap length.
- [ ] Two identical pieces get sheets with the same rebar number.

### 4.5.5 Mounting parts
**ID:** STR-PRE-005 · **Priority:** P2 · **Ref:** research/03 §6.1–6.2

**Purpose.** Place the embedded parts that a precast piece needs for lifting, fixing and connecting, from rules rather than by hand.

**Workflow.** Select precast assemblies and choose *Concrete › Precast Detailing › Mounting Parts*. Parts are also placed automatically by *Split* when automatic updates are on (STR-PRE-002). Individual parts can afterwards be moved, deleted or added by hand like any family occurrence.

**Behaviour & rules.**
- Rule-driven placement of lifting anchors, loops, bushings, connectors and lattice girders, per configuration.
- Placement rules include count by weight, edge distances and centre-of-gravity symmetry.
  - **Weight bands** decide the count and family (for example up to 10 t: 2 anchors; up to 16 t: 4 anchors). A piece heavier than the last band is not given parts and is reported.
  - Weight is computed from the piece volume (openings removed) and the concrete density of the material (default 2 500 kg/m³).
  - Lifting anchors go on the top edge for walls and the top face for slabs, symmetric about the centre of gravity, at least the configured edge distance from ends and openings. If no symmetric position satisfies the distances, the closest valid positions are used and a warning is issued.
- Mounting-part families can carry dimensioning references for shop drawings.
- Re-running replaces only parts the rules generated; parts added or moved by hand are kept and reported.
- Mounting parts host on the piece and move with it; deleting the piece deletes them (STR-GEN-002).

**Data (IFC mapping).** `IfcMechanicalFastener` (`USERDEFINED` "LIFTING_ANCHOR", "BUSHING", …) or `IfcDiscreteAccessory` (`USERDEFINED` "CONNECTOR", or `ANCHORPLATE`), aggregated into the piece assembly. The generated flag and the rule that placed each part are RXD element data (`structural`). The piece weight is written as NetWeight in `Qto_WallBaseQuantities` or `Qto_SlabBaseQuantities` of the piece.

**Commands.** `precast.place_mounting_parts(assembly_ids[]) -> report`

**Acceptance criteria.**
- [ ] A 9 t wall piece configured for "2 anchors up to 10 t" receives 2 lifting anchors placed symmetrically about the centre of gravity.
- [ ] A 20 t piece with bands only up to 16 t receives no anchors and is listed in the report.
- [ ] Re-running after the user moved one anchor keeps the moved anchor and reports it.

### 4.5.6 Shop drawings
**ID:** STR-PRE-006 · **Priority:** P2 · **Ref:** research/03 §6.4 · screenshots/structure_mep/precast_shop_drawing.png

**Workflow.** Select assemblies and choose *Shop Drawings*. Reviera creates one sheet per assembly, or per group of identical assemblies, from the shop-drawing template configured for that element kind.

**Behaviour & rules.**
- **Templates** are Reviera assembly-sheet templates (chapter 5 sheets and assembly views). They define:
  - viewports: front, sections, 3D and reinforcement
  - a centre-of-gravity symbol
  - dimension type and dimension line distances
  - descriptions
  - schedules: bill of materials for concrete, rebar, mesh and mounting parts
  - per-category tags (*Annotations*) and per-category dimension rules (*Dimensioning*)
- Similar pieces share dimension layouts.
- Only title blocks that contain the required template viewports are accepted. Reviera ships open template title blocks.
- Shop-drawing generation is a command, so Lua scripts and the MCP server can drive and customise it. This replaces the reference product's separate precast API.

**Commands.** `precast.shop_drawings(assembly_ids[], template_id?, group_identical?: bool) -> sheet_ids[]`

**Acceptance criteria.**
- [ ] Generating shop drawings for 10 pieces, of which 4 are identical, with grouping on produces 7 sheets. Each has dimensioned views, a centre-of-gravity symbol and a BOM schedule.

### 4.5.7 CAM export
**ID:** STR-PRE-007 · **Priority:** P2 (framework) / P3 (specific proprietary formats) · **Ref:** research/03 §6.4

**Purpose.** Hand precast pieces to plant production systems as machine-readable files.

**Workflow.** Select precast assemblies (or none, for all assemblies in the project) and choose *Concrete › Precast Detailing › CAM Export*. Choose the writer and the output folder; the filename rule comes from the configuration (STR-PRE-001) and can be overridden for this run. The export runs in the background and ends with a summary.

**Behaviour & rules.**
- A **CAM export framework** with per-format writer plugins.
  - A writer is a provider (ANA-PLUG-001 style manifest with name, version, SPDX licence and file extension), implemented in Lua or as an external process. It receives the neutral piece model below and returns files.
  - `cam_writer.list` lists installed writers with their format and version.
- Each writer node has an output **filename rule** built from variables: name with length, minimum length, padding and alignment; date; time; production number.
  - Syntax: `{Var:Length:MinLength:Pad:Align}`, where any trailing field may be omitted; variables are `Name`, `ProdNo`, `Date` (YYYYMMDD), `Time` (HHMMSS) and `Mark`. Unknown variables and malformed fields are rejected before export with the position of the error.
  - Names that would collide in the folder get a numeric suffix, and the summary lists them.
- **[Suggestion]** writers: Unitechnik (versions 5.2/6.0/7.0) and PXML, as plugins. Implementations must be based on openly available format documentation. If such documentation is not available under acceptable terms, the format stays P3.
- Reviera always provides a neutral **JSON/CSV piece export** (geometry outline, openings, embeds, reinforcement and piece marks) for custom CAM pipelines.
  - Coordinates are in millimetres in the piece's local production system (origin at the lower-left corner of the piece's front face, X along the length).
- Pieces without a piece mark, or whose reinforcement or mounting parts are out of date (STR-PRE-003, STR-PRE-005), are exported only after a confirmation that lists them; scripted calls report them as warnings.
- The export does not change the model.

**Data (IFC mapping).** Reads the precast assemblies, pieces, reinforcement and mounting parts of STR-PRE-002 to 005 and `Pset_PrecastConcreteElementFabrication` (PieceMark, ProductionLotId, SerialNumber). Writer settings and filename rules are stored in the precast configuration (RXD document `fabrication`). Nothing is written back.

**Commands.**
- `precast.cam_export(assembly_ids[], writer_id, folder, filename_rule?) -> files[]`
- `cam_writer.list() -> writers[]`

**Acceptance criteria.**
- [ ] The rule `{Name:12:4:0:left}_{ProdNo}` names files with zero-padded, left-aligned piece names and the production number.
- [ ] The rule `{Nme}_{ProdNo}` is rejected before any file is written, with the position of the unknown variable.
- [ ] The neutral JSON export of a wall piece with one opening lists the outline, the opening, every bar with its shape and the lifting anchors, with coordinates in the piece's local system.
- [ ] Exporting leaves the undo stack unchanged.

---

## 4.6 MEP foundations: connectors, systems and settings

### 4.6.1 Connectors
**ID:** MEP-CONN-001 · **Priority:** P1 · **Ref:** research/03 §7.1

**Purpose.** Connectors are the core MEP abstraction. Every MEP family carries typed connection points: equipment, terminals, fixtures, fittings, accessories, devices and lighting fixtures. Systems, routing and calculations are built from these points.

**Workflow.** In the family editor (chapter 2), *Create › Connector* places a connector on a face or a plane in one of five **domains**: Duct, Pipe, Electrical, Cable Tray or Conduit. Connector parameters can be linked to family parameters.

**Behaviour & rules.**
- **Duct and pipe connector parameters:**
  - **System classification**, a fixed enumeration:
    - air: Supply Air, Return Air, Exhaust Air, Other Air
    - piping: Hydronic Supply, Hydronic Return, Domestic Hot Water, Domestic Cold Water, Sanitary, Vent
    - fire protection: Wet, Dry, Pre-Action, Other
    - Other
    - fittings only: Fitting, Global
  - **Flow direction**: In, Out or Bidirectional
  - **Flow configuration**:
    - *Calculated*: sums the flow downstream
    - *Preset*: a fixed value
    - *System*: a share of the system flow, set by a flow factor from 0 to 1
  - Flow
  - **Loss method**: Not Defined, Coefficient or Specific Loss
  - Shape (round, rectangular or oval) and dimensions
  - Angle
  - Utility flag
  - Fixture units
  - Pipe connection type (threaded, welded, flanged and so on)
- **Electrical connector parameters:**
  - **System type**: Power – Balanced, Power – Unbalanced, Data, Telephone, Security, Fire Alarm, Nurse Call, Controls or Communication
  - Voltage
  - Number of poles
  - Apparent load, per phase where unbalanced
  - Power factor and its state (lagging or leading)
  - **Load classification** and sub-classification
  - Motor flag
- **Cable tray and conduit connectors** carry size and shape only. Conduit also supports **surface connectors**, which accept any number of conduits anywhere on a face.
- **Fittings** declare a **part type**: Elbow, Tee, Cross, Transition, Wye, Tap, Union, Cap, Multi-port or Multi-shape Transition. Routing preferences use the part type to choose fittings automatically.
- The connector **origin and snap points** are previewed in the family editor.

**Data (IFC mapping).**
- Each connector is an `IfcDistributionPort` nested in its element with `IfcRelNests` (the element is RelatingObject, the ports are RelatedObjects).
- Port attributes:
  - FlowDirection: `SINK` for In, `SOURCE` for Out, `SOURCEANDSINK` for Bidirectional
  - PredefinedType: `DUCT`, `PIPE`, `CABLE` or `CABLECARRIER`
  - SystemType: an `IfcDistributionSystemEnum` value derived from the classification
- Shape and size go in `Pset_DistributionPortCommon` and in the port-type psets (`Pset_DistributionPortTypeDuct`, `Pset_DistributionPortTypePipe`, `Pset_DistributionPortTypeCable`) where they are defined.
- Electrical values go to the port, or to the element's `Pset_ElectricalDeviceCommon` (RatedVoltage, NumberOfPoles, PowerFactor, …).
- Connector placement inside the family, the flow configuration and flow factor, and the loss method are kept in the **RXD family recipe**. The resolved flow values are written to standard psets.

**Commands.**
- `connector.add(family_id, domain, face_ref | plane, params) -> connector_id` (family editor)
- `connector.set(element_id, connector_index, params)`
- `connector.list(element_id) -> connectors[]`

**Acceptance criteria.**
- [ ] A VAV box family with one inlet and one outlet duct connector exports two `IfcDistributionPort`s nested with `IfcRelNests`, with `SINK` and `SOURCE` flow directions.
- [ ] A fitting family whose part type is Tee is offered by routing preferences only in Junction rows.

### 4.6.2 Physical connections and the connectivity graph
**ID:** MEP-CONN-002 · **Priority:** P1 · **Ref:** research/03 §7.1–7.2, §15.2

**Purpose.** Reviera shall keep a live **connectivity graph** of connected ports. Systems, sizing, checking and analysis all use it.

**Workflow.** Connections are normally made implicitly: drawing a duct, pipe, tray or conduit from a connector, snapping an end grip onto an open connector, or inserting an inline fitting or accessory. Users can also connect or disconnect explicitly from the context menu of a selected connector glyph (*Connect To…*, *Disconnect*).

**Behaviour & rules.**
- Two ports are connected when they coincide in position, direction (anti-parallel), domain and compatible size, or when a connection command joins them explicitly.
- **Tolerances.** Coincidence is tested within 0.5 mm in position and 0.5° in direction. Size is compatible when shape and nominal size match exactly; a mismatch is allowed only through a transition fitting, which `mep.connect` inserts from the routing preferences when one fits. Otherwise the command is refused with "Connectors differ in size or shape and no transition is available".
- **Domains.** Ports of different domains (for example Duct and Pipe) are never connected. Electrical ports join only through circuits (§4.9), not through this graph's physical edges.
- **One partner per port.** A duct, pipe, tray or conduit port has at most one partner. Conduit surface connectors (MEP-CONN-001) are the exception and accept any number.
- The graph is updated incrementally on each transaction. Network IDs (connected components) are recomputed in parallel. Undo and redo restore the graph exactly, because edges are derived from stored relationships.
- Open ports are tracked so that *Show Disconnects* (MEP-SYS-005) does not need a full scan.
- **Moving connected elements.** Dragging an element drags its connected neighbours' ends with it where the geometry allows (segments stretch; fittings move). If the move would break the anti-parallel rule, the connection is dropped and a warning lists the newly open ports.
- **Linked models.** Ports in linked models can be read for checking, but connections across a link are never created.

**Data (IFC mapping).** `IfcRelConnectsPorts` (RelatingPort, RelatedPort, optional RealizingElement for couplings). Network IDs are computed and not stored. On import, `IfcRelConnectsPorts` between ports whose positions differ by more than the tolerance are kept and flagged with a warning rather than dropped.

**Commands.**
- `mep.connect(port_a, port_b)`: refused across domains or when a port already has a partner.
- `mep.disconnect(port_a, port_b)`
- `mep.network_of(element_id) -> element_ids[]`

**Acceptance criteria.**
- [ ] Drawing a duct from an equipment port creates `IfcRelConnectsPorts` between the equipment port and the duct's start port.
- [ ] Deleting a fitting in a run leaves two open ports, which *Show Disconnects* reports.
- [ ] `mep.connect` between a duct port and a pipe port is refused, and no relationship is written.
- [ ] Undoing a connection and redoing it gives the same network ID membership for every element in the network.

### 4.6.3 System classifications and system types
**ID:** MEP-SYS-001 · **Priority:** P1 · **Ref:** research/03 §7.2

**Purpose.** Group connected elements into logical systems. The classifications are fixed. **System types** are user-defined types within a classification.

**Behaviour & rules.**
- System types appear in the Project Browser under *Families › Duct Systems / Piping Systems*. They can be duplicated.
- A system type has:
  - a graphics override (colour, line pattern)
  - a material
  - **calculations**: All, Flow Only, Performance or None
  - an abbreviation
  - rise and drop symbols
  - for piping: fluid type and temperature, and the flow-conversion method
- The system kinds are duct systems, piping systems, electrical circuits (§4.9) and switch systems.

**Data (IFC mapping).**
- A system type is an `IfcDistributionSystem` occurrence template. Because IFC has no system *type* entity, Reviera writes the type definition to **RXD document** `mep_settings` (system types) and stamps each system occurrence with the type name in its `ObjectType`.
- Each system is an `IfcDistributionSystem` with PredefinedType from the classification:

| Classification | PredefinedType |
|----------------|----------------|
| Supply Air | `AIRCONDITIONING` or `VENTILATION` |
| Return Air | `VENTILATION` |
| Exhaust Air | `EXHAUST` |
| Hydronic Supply / Return | `HEATING` or `CHILLEDWATER` (chosen by fluid temperature, user-overridable) |
| Domestic Cold Water | `DOMESTICCOLDWATER` |
| Domestic Hot Water | `DOMESTICHOTWATER` |
| Sanitary | `DRAINAGE` or `SEWAGE` |
| Vent | `VENT` |
| Fire protection (all) | `FIREPROTECTION` |
| Other | `USERDEFINED` |

- Members are linked with `IfcRelAssignsToGroup`. The serviced building or storeys are linked with `IfcRelServicesBuildings`.
- `Pset_DistributionSystemCommon` and, where it applies, `Pset_DistributionSystemTypeVentilation`.

**Commands.**
- `system_type.create(classification, name, params) -> type_id`
- `system_type.set(type_id, params)`

**Acceptance criteria.**
- [ ] Changing a system type's colour override recolours every element of every system of that type in all views.
- [ ] A domestic cold water system exports as `IfcDistributionSystem` with PredefinedType `DOMESTICCOLDWATER`, assigned to its pipes, fittings and fixtures.

### 4.6.4 Creating and editing systems
**ID:** MEP-SYS-002 · **Priority:** P1 · **Ref:** research/03 §7.2

**Workflow.**
1. Select terminals, fixtures or equipment.
2. Choose *Create Systems › Duct / Piping / Power / Data / Switch …*.
3. Set the system type and a **system name** (a name is proposed automatically). Optionally open the system in the System Editor.

Drawing a duct or pipe from a component whose connector has a system type also creates a system or adds to one.

**UI.** The System Editor mode (*Modify | Duct/Piping Systems › Edit System*) offers:
- Add To System and Remove From System
- **Select Equipment**
- System Properties
- Finish and Cancel

Other tools:
- **Divide System**: when one system contains several separate physical networks, it is split into "Name 1", "Name 2" and so on
- **Disconnect Equipment**
- **Generate Layout** (MEP-DUCT-008)
- **Generate Placeholder**

A **System Selector** drop-down on systems contextual tabs switches between the systems attached to the selected element.

**Behaviour & rules.**
- An element can belong to several systems through different connectors.
- Components with no system are listed as *Unassigned* in the System Browser.
- Only compatible classifications can be added. For example, a supply-air terminal cannot be added to a return-air system.

**Data (IFC mapping).** `IfcRelAssignsToGroup` membership. The equipment that serves a system is recorded through the port relationship, and its GlobalId is mirrored in RXD element data (`mep_settings`) on the system for fast lookup.

**Commands.**
- `system.create(kind, element_ids[], system_type_id, name?) -> system_id`
- `system.add(system_id, element_ids[])`
- `system.remove(system_id, element_ids[])`
- `system.select_equipment(system_id, equipment_id)`
- `system.divide(system_id) -> system_ids[]`
- `system.disconnect_equipment(system_id)`

**Acceptance criteria.**
- [ ] Creating a supply air system from 8 diffusers and then selecting an AHU as equipment gives a system with 9 members and the AHU as source.
- [ ] *Divide System* on a system that spans two unconnected networks creates two systems with suffixes 1 and 2.

### 4.6.5 System Browser
**ID:** MEP-SYS-003 · **Priority:** P1 · **Ref:** research/03 §7.2 · screenshots/structure_mep/system_browser_analytical_systems.png

**UI.** A dockable tree palette, toggled with a proposed `F9`. It has:
- **Views:**
  - *Systems*, with a discipline filter: All, Mechanical, Piping or Electrical
  - *Zones*: system-zones and the spaces assigned to them
  - *Analytical Systems*: water loops, air systems, zone equipment, and the electrical analytical distribution tree
- **Columns** chosen by the user: Flow, Size, Name, Type, Space and Phase.
- An *Unassigned* node.
- Context-menu commands: Select, Show, Pressure Loss Report, Delete and Properties.
- Toolbar commands that add analytical systems and nodes.
- A search box that filters the tree by name, mark or system abbreviation; matching branches expand automatically.

**Purpose.** Give one place to see every system, its members and its unassigned components, and to act on them without hunting through views.

**Behaviour & rules.**
- **Hierarchy.** In the *Systems* view the tree is: discipline › system classification › system (name, type) › equipment › members, with fittings and segments under a collapsible *Segments and fittings* node so that large runs stay readable. An element that belongs to several systems appears under each of them.
- **Unassigned.** Components with at least one connector that has no system are listed under *Unassigned*, grouped by domain. The node count updates live.
- Selection is synchronised both ways with the canvas. Selecting a node selects its members in the active view; members not visible in that view stay selected but are counted in the status bar as "n not visible". *Show* opens or cycles through views where the selection is visible.
- **Columns.** Column values are read-only except *Name*, which renames the system in place (validated against duplicate names within the same classification: "A system with this name already exists"). Column choice and widths are stored per user in `ui/state`.
- **Delete** on a system node deletes the logical system only; its members become *Unassigned*. Physical elements are never deleted from the browser.
- The tree is virtualised so it can show more than 100 000 elements. It refreshes incrementally from the transaction change set, not by rebuilding.
- **Linked models** appear as read-only branches named after the link; their nodes can be selected for *Show* but not edited.

**Data (IFC mapping).** None of its own: the browser reads `IfcDistributionSystem`/`IfcDistributionCircuit` groups and RXD element data. Its layout is user interface state (`ui/state`), not model data.

**Commands.** `system_browser.show(view: "systems"|"zones"|"analytical", discipline?)`. Context-menu items call existing commands: `selection.set`, `pressure_loss_report.export` (MEP-CALC-004), `system.divide` and `element.delete`.

**Acceptance criteria.**
- [ ] Selecting a system node selects all its members in the active view.
- [ ] The *Unassigned* node lists a newly placed diffuser that is not in any system.
- [ ] Deleting a system from the browser leaves its 8 diffusers in the model and lists them under *Unassigned*.
- [ ] Renaming a system to the name of another system of the same classification is refused with a message.

### 4.6.6 Check Systems and Show Disconnects
**ID:** MEP-SYS-005 · **Priority:** P1 · **Ref:** research/03 §8.6 · screenshots/structure_mep/check_pipe_systems_warning.png

**Behaviour & rules.**
- *Check Duct Systems*, *Check Pipe Systems* and *Check Circuits* are toggles that draw warning markers and dashed links for three kinds of problem:
  - systems that are **not well connected**
  - mismatches between **flow and demand configuration**
  - mismatches in **flow direction**
- *Show Disconnects* marks open connectors on ducts, pipes, cable tray, conduit and electrical elements.
- Each marker opens the related warning (chapter 2 warnings).

**Purpose.** Find broken or inconsistent systems before sizing, calculation or fabrication conversion depend on them.

**UI.** *Analyze › Check Systems* panel: three toggle buttons (*Check Duct Systems*, *Check Pipe Systems*, *Check Circuits*) and *Show Disconnects*, which opens a small dialog with a check box per domain (Duct, Pipe, Cable Tray, Conduit, Electrical). Toggle state is per view and survives closing and reopening the view during the session.

**Behaviour & rules.**
- **Checks performed:**
  - *Not well connected*: the system's members do not form one connected network from its equipment, or the system has no equipment.
  - *Flow/demand mismatch*: a terminal connector set to *Preset* sits on a branch whose equipment connector is also *Preset* with a different total, or a *Calculated* connector has nothing downstream.
  - *Flow direction mismatch*: two connected connectors are both In or both Out.
  - *Circuits*: a circuit has no panel, or a member's distribution system does not match the panel (MEP-ELEC-003).
- Markers are view-specific overlays drawn with the theme warning token and a "!" glyph; they are never printed unless *Print warning markers* is set in the print dialog. Dashed links join the marker to the offending connector or element.
- Markers update incrementally after every transaction (no manual refresh). In views with more than 5 000 issues, only the first 5 000 are drawn and the status bar shows the total.
- Elements in linked models are checked but their markers cannot be acted on.
- *Check Systems* does not alter the model, so toggling is not undoable.

**Data (IFC mapping).** No model data. Issues are the standard persisted warnings (`project/warnings`, chapter 2).

**Commands.**
- `mep_check.systems(kind: "duct"|"pipe"|"circuit", view_id, on: bool) -> issues[]`: each issue is `{kind, element_ids[], connector_refs[], message}`.
- `mep_check.show_disconnects(view_id, domains[], on: bool) -> ports[]`

**Acceptance criteria.**
- [ ] A pipe run with one unconnected end shows one disconnect marker. Connecting the end removes the marker without a manual refresh.
- [ ] Two connected connectors both set to Out produce one *flow direction mismatch* issue naming both elements.
- [ ] A supply-air system with no equipment selected is reported as not well connected.
- [ ] *Show Disconnects* with only the Conduit domain ticked shows no markers on open duct ends.

### 4.6.7 Mechanical settings
**ID:** MEP-SET-001 · **Priority:** P1 · **Ref:** research/03 §7.3

**UI.** *Manage › MEP Settings › Mechanical Settings*, or the HVAC and Plumbing panel launchers (`mep_settings.show("mechanical")`, common dialog behaviour in MEP-SET-004). Tree:
- **Hidden Line**: whether MEP hidden lines are drawn, their line style, the inside and outside gap, and the single-line gap.
- **Duct Settings:**
  - single-line fitting annotation scale and fitting annotation size
  - **air density** and **air dynamic viscosity**
  - size separators and suffixes, connector separator, flat-on-top and flat-on-bottom suffixes, rise/drop annotation size
  - **Angles**: any angle, an increment, or a list of specific angles
  - **Conversion**: main and branch duct type, offset and flex settings used by placeholder conversion and layout generation
  - **Rectangular, Oval and Round size tables**, with flags *Used in Size Lists* and *Used in Sizing*
  - **Calculation**: the friction method for straight segments (selectable equations, for example Colebrook or Altshul–Tsal)
- **Pipe Settings:**
  - annotation sizes, suffix and separators
  - **Angles** and **Conversion**
  - **Segments and Sizes**: a segment is a material plus a schedule or class, with roughness and a size catalogue (nominal, inner and outer diameter), each flagged *Used in Size Lists* or *Used in Sizing*
  - **Fluids**: name, then a table of temperature, dynamic viscosity and density
  - **Slopes**: a list of allowed slope values; zero cannot be removed, and values above 45° give a warning
  - **Calculation**: background hydronic network analysis on or off; pressure-drop method (Colebrook or Haaland); fixture-unit-to-flow method

**Data (IFC mapping).** **RXD document** `mep_settings`. Pipe segments are also written as `IfcPipeSegmentType` objects with `Pset_PipeSegmentTypeCommon` (NominalDiameter, InnerDiameter, OuterDiameter, Roughness), so other tools see the size catalogue.

**Commands.**
- `mechanical_settings.get(path) -> value`
- `mechanical_settings.set(path, value)`
- `pipe_segment.create(material_id, schedule, roughness, sizes[]) -> id`
- `fluid.create(name, table[]) -> id`
- `duct_size_table.set(shape, sizes[])`

**Acceptance criteria.**
- [ ] Removing a duct size from *Used in Size Lists* removes it from the size drop-down but keeps existing ducts of that size valid.
- [ ] Trying to delete the zero slope is refused.

### 4.6.8 Electrical settings
**ID:** MEP-SET-002 · **Priority:** P1 (wiring, tray, conduit, voltages) / P2 (load calculations, circuit naming, panel schedules) · **Ref:** research/03 §7.3

**UI.** *Manage › MEP Settings › Electrical Settings* (`mep_settings.show("electrical")`, common dialog behaviour in MEP-SET-004). Tree:
- **Hidden Line.**
- **General:**
  - connector separator
  - electrical data style
  - circuit description format
  - **phase labels** (A/B/C or L1/L2/L3)
  - capitalisation of load names
  - **circuit sequence**: numerical, or grouped by phase
  - default circuit rating
  - circuit path offset
- **Angles.**
- **Wiring:**
  - ambient temperature
  - gap at wire crossings
  - **tick mark** styles for hot, ground and neutral conductors
  - slanted line across tick marks
  - whether tick marks are shown
  - wire sizes, correction factors and ground conductors
  - **wiring types**: material, temperature rating, insulation, maximum size, neutral multiplier, whether a neutral is required, and conduit type
- **Voltage definitions:** name, value, minimum and maximum.
- **Distribution systems:** name, phase (single or three), configuration (wye or delta), number of wires (2–4), line-to-line voltage, line-to-ground voltage, and high-leg phase.
- **Cable tray and conduit settings:** annotation, size separator and suffix, rise/drop symbols, size tables, and conduit **standards**. Reviera ships open tables for common standards and lets users add their own.
- **Load calculations:** on/off for space load calculations, with links to load classifications and demand factors (MEP-ELEC-009/010).
- **Circuit naming** schemes (MEP-ELEC-012).
- **Panel schedules:** spare label, space label, whether spares count in totals, and whether multi-pole circuits are merged into one cell.

**Data (IFC mapping).** **RXD document** `electrical`. Conduit and tray sizes are also written as `IfcCableCarrierSegmentType` objects with their nominal sizes.

**Commands.**
- `electrical_settings.get(path) -> value`
- `electrical_settings.set(path, value)`
- `distribution_system.create(name, phase, config, wires, v_ll, v_lg) -> id`
- `voltage.create(name, value, min, max) -> id`
- `wiring_type.create(params) -> id`

**Acceptance criteria.**
- [ ] A 400/230 V 3-phase 4-wire wye distribution system can be created and assigned to a panel, and only devices with a matching voltage definition can be circuited to it.

### 4.6.9 Conductor and cable settings (international)
**ID:** MEP-SET-003 · **Priority:** P2 · **Ref:** research/03 §7.3 · screenshots/structure_mep/electrical_conductor_and_cable_settings.png

**Behaviour & rules.** *Manage › MEP Settings › Conductor & Cable Settings* is a tabbed dialog with searchable, sortable grids:
- conductor materials
- insulation materials
- **conductor sizes** (name, cross-section area or gauge, diameter)
- **conductor details** (material, size, insulation, ampacity)
- **cable sizes** (number and size of phase conductors, plus ground and neutral)
- **cable types** (assembled cables used by circuits)

The dialog supports both IEC (mm²) and AWG/kcmil conventions. Reviera ships starter tables taken from openly published physical data such as cross-sections and resistivity. Ampacity tables are user-supplied or loaded from content packs, because code tables may be copyrighted (§4.17).

**Data (IFC mapping).** **RXD document** `electrical`. Cable types are written as `IfcCableSegmentType` (`CABLESEGMENT`) with `Pset_CableSegmentTypeCableSegment` and `Pset_CableSegmentTypeConductorSegment`.

**Commands.**
- `conductor_size.create(name, area_mm2 | gauge, diameter) -> id`
- `cable_type.create(name, material_id, insulation_id, phase_conductors: {count, size_id}, neutral?: size_id, ground?: size_id) -> id`
- `conductor_table.import(path)`

**Acceptance criteria.**
- [ ] A 5 × 2.5 mm² Cu/PVC cable type can be defined and chosen on a circuit, and voltage drop uses its conductor resistance.

### 4.6.10 MEP settings dialogs (common shell)
**ID:** MEP-SET-004 · **Priority:** P1 (mechanical, electrical) / P2 (other kinds) · **Ref:** research/03 §1.6, §7.3, §10.4, §11, §12.1, §14

**Purpose.** Give every MEP settings area one entry point, `mep_settings.show(kind)`, and one dialog behaviour, so that the *Manage › MEP Settings* drop-down, the panel launchers on the Systems tab and Lua or MCP callers all reach the same editor.

**Workflow.**
1. Choose *Manage › MEP Settings ›* an item, a Systems-tab panel launcher (HVAC, Plumbing & Piping, Electrical, Fabrication, P&ID), or the proposed shortcuts `MS`, `ES`, `BT`, `FS`.
2. The dialog opens on the requested kind. A tree or tab strip on the left selects a page; the page grid or form is on the right.
3. Edit values. Every field validates as it loses focus; the page shows a red text hint and a theme warning glyph next to an invalid value.
4. *OK* commits all changed pages as **one transaction** named after the dialog ("Mechanical Settings"). *Cancel* discards everything. *Apply* is not offered, so a dialog session is always a single undo step.

**UI.** The `kind` values and the dialog each opens:

| `kind` | Dialog | Pages and grids (detail in) |
|--------|--------|-----------------------------|
| `mechanical` | Mechanical Settings | Hidden Line, Duct Settings, Pipe Settings and their sub-pages (MEP-SET-001) |
| `electrical` | Electrical Settings | General, Wiring, Voltage Definitions, Distribution Systems, Cable Tray and Conduit, Load Calculations, Circuit Naming, Panel Schedules (MEP-SET-002) |
| `conductor_cable` | Conductor & Cable Settings | six searchable grids (MEP-SET-003) |
| `building_space_type` | Building/Space Type Settings | two tabs, *Building Types* and *Space Types* (below) |
| `load_classification` | Load Classifications | list of classifications (below) |
| `demand_factor` | Demand Factors | list of definitions with a method editor (below) |
| `fabrication` | Fabrication Settings | configuration, profile, services (MEP-FAB-002) |
| `pid` | P&ID Settings | *System Types* and *Object Mapping* tabs (MEP-PID-002) |

- **Building/Space Type Settings.** Left: a list of types with *New*, *Duplicate*, *Rename* and *Delete*. Right: lighting load density, power load density, area per person, sensible and latent heat gain per person, plenum lighting contribution, occupancy, lighting and power schedules (picked from the ANA-EM-002 schedule library), heating and cooling set points, outside-air per person and per area, and opening and closing times. A type used by spaces cannot be deleted; the refusal states how many spaces use it.
- **Load Classifications.** Left: the list, with Lighting, Power, HVAC, Motor and Other locked against deletion. Right: name, demand factor (drop-down with an edit button that jumps to the Demand Factors dialog) and the space-load option *Use in space load calculation*. Renaming updates every connector that refers to it.
- **Demand Factors.** Left: definitions. Right: method (Constant, By quantity, By load), *incremental* check box, a range grid (from, to, percentage), the optional largest-motor table, and a live preview that evaluates a typed connected load. Ranges must be contiguous and non-overlapping, start at 0, and have percentages from 0 to 1 000%; the grid flags the first offending row.

**Behaviour & rules.**
- Only one MEP settings dialog is open at a time. Opening a second kind while one is open switches the open dialog to that kind after asking to keep or discard pending edits.
- Settings are project-wide. In BIMserver mode the dialog warns that edits change shared project standards and merge like any other RXD change (02a §8).
- Every page has *Search*, so long size tables and type lists can be filtered.
- A value that is still referenced (a size, a slope, a voltage definition, a wiring type, a load classification) cannot be deleted; the refusal names one referencing element and the count.
- Units follow the project units (chapter 2). Fields accept expressions and unit suffixes.
- The same data can be set without the dialog through `mechanical_settings.set`, `electrical_settings.set`, `space_type.create` and the other commands of the requirements above.

**Data (IFC mapping).** No IFC entity. The pages write to **RXD documents** `mep_settings` (mechanical, P&ID), `electrical` (electrical, conductors, load classifications, demand factors), `analysis` (building and space types) and `fabrication`.

**Commands.** `mep_settings.show(kind: "mechanical"|"electrical"|"conductor_cable"|"building_space_type"|"load_classification"|"demand_factor"|"fabrication"|"pid")`. It only opens the dialog; the edits are recorded as the per-setting commands listed in each requirement, inside one transaction.

**Acceptance criteria.**
- [ ] `mep_settings.show("demand_factor")` opens the Demand Factors dialog, and from Lua or MCP the same call returns without error when no UI is attached (headless) by reporting `ui_unavailable`.
- [ ] Editing three pages of Mechanical Settings and pressing *OK* adds exactly one entry to the undo stack; *Undo* reverts all three pages.
- [ ] Deleting the space type "Office" while 12 spaces use it is refused with a message that names the count 12.
- [ ] A demand-factor range grid with a gap between 10 and 12 kVA cannot be saved, and the row at 12 kVA is flagged.

---

## 4.7 HVAC (ductwork)

### 4.7.1 Duct placement
**ID:** MEP-DUCT-001 · **Priority:** P1 · **Ref:** research/03 §8.1

**Purpose.** Draw rigid ducts that insert fittings automatically according to the duct type's routing preferences.

**Workflow.**
1. Choose *Systems › HVAC › Duct* (proposed `DT`).
2. In the Type Selector pick a duct type (rectangular, round or oval). On the contextual tab and in Properties set:
   - width and height, or diameter
   - **middle elevation**, with a history list, and an elevation lock
   - the reference level
   - the system type
3. Click a start point and an end point. Change the middle elevation and click **Apply** in the middle of a run to create a vertical riser. Risers that span levels, and the rise/drop symbols shown on them, are specified in MEP-DUCT-011.

**UI.** Contextual "Modify | Place Duct" tab:
- **Justification** (horizontal justification, horizontal offset, vertical justification)
- **Automatically Connect**
- **Inherit Elevation** and **Inherit Size**
- sloped connection options *Add Vertical* and *Change Slope*
- **Tag on Placement**
- size and elevation fields (there is no Options Bar)

On a selected duct the canvas shows end grips, editable size text, fitting "+" and "−" controls, flip and rotate. The context menu offers *Draw Duct*, *Draw Flex Duct*, *Cap Open Ends* and *Select* (Tab cycles through connected elements).

**Behaviour & rules.**
- Elbows, tees, transitions and taps are inserted automatically. The routing preferences (MEP-DUCT-002) choose them, and the Angles setting limits the angles.
- *Automatically Connect* snaps to and connects with ducts it crosses. Junctions use the preferred junction type.
- **Size Lock** stops a segment from being resized by sizing.
- Vertical ducts are best drawn in section or elevation, or with Apply.

**Data (IFC mapping).**
- `IfcDuctSegment` (`RIGIDSEGMENT`), typed by `IfcDuctSegmentType`, with two nested `IfcDistributionPort`s.
- Properties: `Pset_DuctSegmentTypeCommon` (Shape, WorkingPressure, …) and `Pset_DuctSegmentOccurrence` (InteriorRoughnessCoefficient, HasLiner, Color).
- Calculated flow values go to `Pset_DuctSegmentOccurrence`, or to an analytical segment (MEP-CALC-002).
- Quantities: `Qto_DuctSegmentBaseQuantities` (Length, GrossCrossSectionArea, OuterSurfaceArea, GrossWeight).
- **RXD element data** (`constraints`): justification, size lock and elevation lock.

**Commands.**
- `duct.create(type_id, start, end, shape_size, middle_elevation, level_id, system_type_id?, justification?) -> element_ids[]` (the segments and fittings created)
- `duct.set_size(ids[], size)`
- `duct.cap_open_ends(ids[])`

**Acceptance criteria.**
- [ ] Drawing an L-shaped 600 × 300 run inserts one elbow from the routing preferences.
- [ ] Changing the middle elevation from 3.0 m to 3.6 m mid-draw and pressing Apply inserts two elbows and a vertical segment.
- [ ] The exported run contains `IfcDuctSegment` and `IfcDuctFitting` elements linked by `IfcRelConnectsPorts`.

### 4.7.2 Routing preferences
**ID:** MEP-DUCT-002 · **Priority:** P1 · **Ref:** research/03 §8.1 ("Routing Preferences"), §15.3

**Purpose.** An ordered list of parts per role and size range, used to choose segments and fittings automatically. This applies to duct, pipe, cable tray and conduit types.

**UI.** *Type Properties › Routing Preferences › Edit*. A grid with one group per role:
- **Segment** (pipe only: material and schedule)
- **Elbow**
- **Junction** (tee, tap or takeoff), with a **preferred junction type** of Tee or Tap
- **Cross**
- **Transition**
- **Union**
- **Multi-shape Transition**, for example rectangular to round
- **Cap**

Each row has a part and a minimum and maximum size. Rows are tried from top to bottom.

**Behaviour & rules.**
- When no row fits, Reviera inserts nothing, leaves the connection open and raises a warning that names the role and size.
- **Reapply Type** re-runs fitting selection on existing runs after preferences change. **Change Type** swaps a run's type and reapplies the preferences.

**Data (IFC mapping).** **RXD document** `mep_settings` (routing preferences per type GlobalId).

**Commands.**
- `routing_preferences.get(type_id) -> table`
- `routing_preferences.set(type_id, table)`
- `mep_run.reapply_type(element_ids[])`
- `mep_run.change_type(element_ids[], type_id)`

**Acceptance criteria.**
- [ ] With Junction rows "Tap ≤ 300 mm, Tee > 300 mm", a 250 mm branch gets a tap and a 400 mm branch gets a tee.
- [ ] After a row is removed, *Reapply Type* replaces the affected fittings and warns where no row fits.

### 4.7.3 Duct fittings and fitting controls
**ID:** MEP-DUCT-003 · **Priority:** P1 · **Ref:** research/03 §8.3

**Behaviour & rules.**
- *Duct Fitting* (proposed `DF`) places a fitting on a duct end or along a duct. A tee inserted in the middle of a run takes the run's size and elevation.
- **In-canvas controls:**
  - size per leg; a transition is added automatically when it is needed
  - **flip**, relative to airflow
  - **rotate**
  - **"+"** upgrades an elbow to a tee, or a tee to a cross
  - **"−"** removes an unused leg
- Damper visibility inside fittings is a family parameter.

**Data (IFC mapping).** `IfcDuctFitting`. PredefinedType: `BEND` for elbows, `JUNCTION` for tees, crosses and taps, `TRANSITION`, `CONNECTOR` for unions, `OBSTRUCTION` for caps, and `ENTRY`/`EXIT`. Types are `IfcDuctFittingType`. `Pset_DuctFittingTypeCommon`. Loss coefficients are written to `Pset_DuctFittingOccurrence` where they are defined.

**Commands.**
- `duct_fitting.place(type_id, host_duct_id | point, params) -> id`
- `mep_fitting.upgrade(id)`
- `mep_fitting.downgrade(id, leg)`
- `mep_fitting.flip(id)`
- `mep_fitting.rotate(id, angle)`

**Acceptance criteria.**
- [ ] "+" on an elbow whose run continues straight turns it into a tee with an open third leg. "−" on that unused leg turns it back.

### 4.7.4 Duct accessories, insulation and lining
**ID:** MEP-DUCT-004 · **Priority:** P1 · **Ref:** research/03 §8.1, §8.3

**Purpose.** Insert inline duct components and apply external insulation or internal lining to duct runs.

**Workflow.**
1. Choose *Systems › HVAC › Duct Accessory* (proposed `DA`) and pick a type in the Type Selector.
2. Hover a duct: the preview snaps to the centreline and orients the accessory along the flow. Click to insert, or click an open duct end to attach it there.
3. For insulation or lining, select ducts, fittings or accessories (Tab selects a whole run) and choose *Modify | Ducts › Insulation › Add* (or *Lining › Add*). A dialog asks for the type and the thickness; *Edit* changes them and *Remove* deletes them.

**Behaviour & rules.**
- *Duct Accessory* (proposed `DA`) inserts dampers, fire dampers, filters, coils and sound attenuators inline. Insertion splits the duct and connects both sides.
- The accessory takes the duct's size and shape when its family has size parameters linked to its connectors. If it cannot match (for example a fixed-size round damper on a rectangular duct), transitions are added from the routing preferences; if none fits, the insert is refused with a message naming the size.
- An accessory must fit between existing fittings: if the free straight length is shorter than the accessory, the insert is refused ("Not enough straight duct for this accessory").
- Deleting an inline accessory leaves the two duct ends open; *Heal* in the context menu rejoins them into one segment when they are collinear and the same size.
- *Add/Edit/Remove Insulation* and *Duct Lining* take a type and a thickness. Insulation is included in interference checking.
- Insulation and lining follow their host: they regenerate when the host is resized or moved and are deleted with it. Lining reduces the free cross-section used by pressure-drop calculations (MEP-CALC-002); insulation does not.
- Default thicknesses: insulation 25 mm, lining 25 mm. Thickness must be greater than 0 and less than half the smallest duct dimension for lining; otherwise the dialog shows an inline error.
- Insulation and lining types are ordinary types (material, roughness for lining) shown in the Project Browser and are drawn using their own object style (chapter 5).

**Data (IFC mapping).**

| Item | IFC |
|------|-----|
| Damper | `IfcDamper` (`FIREDAMPER`, `CONTROLDAMPER`, `BACKDRAFTDAMPER`, …) |
| Filter | `IfcFilter` |
| Coil | `IfcCoil` |
| Sound attenuator | `IfcDuctSilencer` |
| Insulation and lining | `IfcCovering` with PredefinedType `INSULATION` or `WRAPPING`, related with `IfcRelCoversBldgElements`. Thickness on the covering. |

**Commands.**
- `duct_accessory.place(type_id, duct_id, point) -> id`
- `mep_insulation.add(element_ids[], type_id, thickness)`
- `mep_insulation.remove(element_ids[])`
- `mep_insulation.set(element_ids[], type_id?, thickness?)` (*Edit Insulation*)
- `duct_lining.add(element_ids[], type_id, thickness)`
- `duct_lining.remove(element_ids[])`

**Acceptance criteria.**
- [ ] A fire damper inserted in a 400 mm round duct splits it into two segments connected through the damper.
- [ ] Adding 50 mm insulation exports an `IfcCovering` (`INSULATION`) related to the duct.
- [ ] Resizing an insulated 600 × 300 duct to 800 × 300 regenerates its insulation at the new size with the same thickness.
- [ ] Adding 25 mm lining to a 400 × 200 duct lowers the free area used by the pressure-drop calculation to 350 × 150.
- [ ] Inserting a 600 mm long silencer into a 400 mm straight between two elbows is refused with a message.

### 4.7.5 Flexible duct
**ID:** MEP-DUCT-005 · **Priority:** P1 · **Ref:** research/03 §8.3

**Purpose.** Model the short flexible connections between rigid branches and terminals or equipment, so that layouts, quantities and pressure losses include them.

**Workflow.**
1. Choose *Systems › HVAC › Flex Duct* (proposed `FD`), or *Draw Flex Duct* from the context menu of an open connector.
2. Pick the type and size (diameter, or width and height) on the contextual tab and in Properties.
3. Click the start (a connector, a duct centreline or a free point), then intermediate points, then the end. Ending on a connector connects it; double-click or Enter ends at a free point.
4. To convert existing runs, select terminals and choose *Convert to Flex Duct* (proposed `CV`), then enter the maximum length.

**Behaviour & rules.**
- *Flex Duct* (proposed `FD`) has round and rectangular types. It is drawn from a connector or a duct centreline, as a spline with vertex, tangent and end grips. *Flex Duct Controls* add and delete vertices.
- Starting on a duct centreline inserts a takeoff (tap) chosen by the rigid duct's routing preferences; if none is defined for the size, the start is refused with a message naming the role and size.
- The spline passes through every clicked point; end tangents follow the connector directions. Minimum bend radius is the type's *Minimum Bend Radius* (default 1 × diameter); a tighter shape is accepted but flagged with a warning.
- **Pressure loss.** The type carries a roughness and an optional *loss multiplier for compressed flex* (default 1.0), used by MEP-CALC-002.
- **Plan display.** Flex duct can show a type-defined *flex pattern* (a sine or wavy line) in single-line and two-line views.
- *Convert to Flex* (proposed `CV`) converts the last segment before a terminal into flex duct, up to a maximum length.
- *Convert to Flex* works per terminal: if the final rigid segment (plus the elbow at the terminal, if any) is longer than the limit, that terminal is skipped and listed in the report. Converted flex keeps the system and the size of the replaced segment.
- **Recovery.** Drawing journals each click (`flex.pick{point, snap_ref?}`) and each size or type change (`flex.set{param, value}`) as micro-operations; replay re-applies them in order and reopens the session at the last point (conventions §2.1).

**Data (IFC mapping).** `IfcDuctSegment` (`FLEXIBLESEGMENT`), with a swept-disk body along a B-spline. Length goes to `Qto_DuctSegmentBaseQuantities.Length` (spline length). Control points and tangents are RXD element data (`sketch`), so the spline is re-editable.

**Commands.**
- `flex_duct.create(type_id, points[], start_port?, end_port?) -> id`
- `duct.convert_to_flex(terminal_ids[], max_length) -> ids[]`

**Acceptance criteria.**
- [ ] *Convert to Flex* with a 1.5 m limit converts only branches whose final segment is 1.5 m or shorter.
- [ ] A flex duct drawn from a duct centreline to a diffuser inserts a tap from the routing preferences and connects at both ends.
- [ ] The exported flex duct's `Length` equals the spline length within 1 mm.
- [ ] After a simulated crash during drawing with three points picked, the recovered session resumes with the same three points.

### 4.7.6 Justification editor
**ID:** MEP-DUCT-006 · **Priority:** P1 · **Ref:** research/03 §8.1 ("Justify")

**Workflow.**
1. Select a run with Tab.
2. Choose *Justify*.
3. Choose a **control point**: the start or end segment.
4. Pick one of 9 alignment points (top, middle or bottom × left, centre or right), or pick an *alignment line* in a wireframe 3D view.
5. Finish.

**Purpose.** Align a run whose segments change size so that one face or edge stays continuous, for example a flat bottom for ceiling clearance or a flush side along a wall.

**UI.** Contextual tab *Justification Editor*: *Control Point* (cycles between the run's first and last segment, shown by an arrow glyph), the 3 × 3 alignment buttons, *Alignment Line* (active only in a wireframe or hidden-line 3D view), *Finish* and *Cancel*. The current alignment point is drawn on the control segment's end face.

**Behaviour & rules.** The run keeps its control segment fixed and re-aligns every segment to the chosen face or edge. Fittings regenerate.
- Applies to ducts, pipes, cable tray and conduit runs. For round ducts and pipes, only top, middle and bottom are meaningful vertically; left and right align the outside diameter.
- A run is a chain without branches selected with Tab. Selecting elements that do not form one chain is refused: "Justify needs a single run without branches".
- Segments that are size-locked keep their size; only their position moves. Sloped segments keep their slope.
- If re-alignment would disconnect a branch whose own run is not selected, the branch's connecting segment is stretched or moved so it stays connected; if that is impossible, the editor reports the branch and the change is not applied.
- *Alignment Line* aligns the selected edge line of every segment to the picked edge of the control segment.
- The justification chosen is written to each segment's justification settings, so later size changes keep the alignment.
- **Recovery.** The editor journals `justify.control{end}`, `justify.align{point | line_ref}` as micro-operations; replay restores the pending choice before *Finish* (conventions §2.1). *Finish* commits in one transaction; *Cancel* discards it.

**Data (IFC mapping).** Geometry changes only. Justification values are RXD element data (`constraints`) on each segment (MEP-DUCT-001).

**Commands.** `mep_run.justify(element_ids[], control_end, alignment: "TL"|"TC"|…|"BR" | line_ref)`

**Acceptance criteria.**
- [ ] Justifying a run that transitions from 600 to 400 mm wide to Bottom-Left makes the bottoms and left sides of all segments flush.
- [ ] Selecting a run that includes a tee and its branch refuses *Justify* with the message above.
- [ ] After justifying Bottom-Centre, changing one segment's height keeps its bottom flush with the others.

### 4.7.7 Placeholders and conversion
**ID:** MEP-DUCT-007 · **Priority:** P1 · **Ref:** research/03 §8.2

**Purpose.** Let designers lay out duct and pipe routes quickly, early in design, and turn them into fully fitted runs later without redrawing.

**Workflow.**
1. Choose *Duct Placeholder* or *Pipe Placeholder* (on the Duct and Pipe split buttons). Set type, size, middle elevation and system type as for real ducts or pipes (MEP-DUCT-001, MEP-PIPE-001).
2. Click points as for real ducts or pipes. Placeholders connect to each other end-to-end and to equipment connectors.
3. Select placeholders (Tab selects a connected set) and choose *Convert Placeholder*.
4. Review the conversion report: it lists every junction that could not be fitted and leaves those ends open.

**Behaviour & rules.**
- **Duct and pipe placeholders** are single-line elements without fittings, used for fast layout and coordination. They keep their size and system.
- Placeholders join directly at corners and junctions: no elbows or tees exist, so a placeholder network is a graph of segments meeting at points.
- They are drawn as single lines in every detail level and are included in system flow and *Check Systems*, but not in sizing, pressure-drop calculation or interference checking (a body is needed for those).
- Justification settings are not available for placeholders; the line is always the centreline.
- *Convert Placeholder* produces real ducts or pipes and fittings using the type's routing preferences. Conversion warnings are listed for review.
- Conversion keeps each placeholder's GlobalId on the resulting segment and creates new fittings. Sloped pipe placeholders keep their slope.
- A placeholder network that contains a junction of more than four legs, or legs meeting at an angle that the Angles setting does not allow, is converted up to that junction and the remaining ends are left open with a warning.
- Real ducts and pipes cannot be converted back; *Undo* restores the placeholders.

**Data (IFC mapping).** Placeholders are `IfcDuctSegment` or `IfcPipeSegment` with an `Axis` representation only (no Body). A placeholder flag is RXD element data (`constraints`). Other tools see a segment with no body.

**Commands.**
- `duct_placeholder.create(type_id, start, end, size, elevation, level) -> id`
- `pipe_placeholder.create(type_id, start, end, diameter, middle_elevation, level_id, system_type_id?, slope?) -> id`
- `placeholder.convert(ids[]) -> element_ids[]`

**Acceptance criteria.**
- [ ] Converting an L-shaped placeholder pair produces two segments and one elbow at the original elevation.
- [ ] A converted placeholder segment keeps its GlobalId and system membership.
- [ ] A five-leg placeholder junction converts every other junction and leaves that junction's ends open, listed in the report.
- [ ] An exported placeholder has an `Axis` representation and no `Body`.

### 4.7.8 Generate Layout (system-based auto-routing)
**ID:** MEP-DUCT-008 · **Priority:** P2 · **Ref:** research/03 §8.2

**Workflow.**
1. Select a system component and choose *Generate Layout*.
2. Reviera proposes several **solutions**, each a different trunk-and-branch arrangement: network, perimeter, intersections or custom.
3. **Settings**: main and branch type, offsets, and slope for pipe.
4. *Edit Layout* adjusts trunk positions.
5. *Finish Layout* creates placeholders or real elements.

**Purpose.** Propose trunk-and-branch routes for an existing logical system, so a first layout can be produced from equipment and terminals alone.

**UI.** Contextual tab *Generate Layout*: *Solution Type* drop-down (Network, Perimeter, Intersections), a solution counter with previous/next arrows ("Solution 2 of 5"), *Settings*, *Edit Layout*, *Solutions*, *Finish Layout* and *Cancel Layout*. The preview draws the trunk in one theme token and branches in another, both with text labels in the legend, plus warning glyphs where a component cannot be reached.

**Behaviour & rules.** The router avoids nothing in the first release. It routes orthogonally in the layout plane, and clashes are left to the interference check. Obstacle-aware routing is an open question (§4.17).
- **Solution types:**
  - *Network*: a trunk along the minimal rectangle enclosing the components, with branches to each one.
  - *Perimeter*: trunks around the bounding rectangle at an inset set in Settings (default 1 m).
  - *Intersections*: trunks along lines through the components in both directions, choosing intersections that minimise length.
  - *Custom*: any solution after *Edit Layout* has changed it.
  Solutions are ranked by total length; up to 10 per type are offered.
- **Settings dialog:** duct or pipe type for main and branch, offset of the layout plane from the level (default: the equipment connector's elevation), and for pipe a slope from the allowed list. Changing settings regenerates solutions.
- *Edit Layout* lets the user drag trunk lines and move branch connection points. Each drag is journalled as a micro-operation (`layout.move_line{line_index, delta}`, `layout.move_point{point_index, position}`); replay restores the edited solution (conventions §2.1).
- The system must have equipment and at least one other member, and all must be on connectors of the same domain. Otherwise the tool is refused with a message naming the problem.
- *Finish Layout* creates placeholders or real elements in one transaction and connects them to the components. Junctions that cannot be fitted are left open and reported (as for MEP-DUCT-007).
- Unfinished solutions are session data and are discarded by *Cancel Layout*; nothing is written to the model before *Finish*.

**Data (IFC mapping).** No data of its own. The result is ordinary ducts, pipes, placeholders and fittings.

**Commands.**
- `mep_layout.generate(system_id, settings) -> solution_ids[]`
- `mep_layout.edit(solution_id, moves[]) -> solution_id` (each move is `{line_index | point_index, delta | position}`)
- `mep_layout.finish(solution_id, as: "placeholder"|"real") -> element_ids[]`

**Acceptance criteria.**
- [ ] For a system of one AHU and 6 diffusers, at least two solutions are offered, and *Finish* connects all 7 components.
- [ ] Solutions within one type are listed in ascending order of total length.
- [ ] Generate Layout on a system without equipment is refused with a message.
- [ ] Moving a trunk line in *Edit Layout* and finishing creates elements along the moved line.

### 4.7.9 Air terminals and mechanical equipment
**ID:** MEP-DUCT-009 · **Priority:** P1 · **Ref:** research/03 §8.4

**Behaviour & rules.**
- *Air Terminal* (proposed `AT`) supports Tag on Placement. *Air Terminal on Duct* places the terminal on a duct face. The terminal's preset flow adds to the running supply, return or exhaust totals of the space it sits in.
- *Mechanical Equipment* (proposed `ME`): AHUs, VAV boxes, fans, boilers, chillers, pumps and radiators. Spacebar rotates by 90°. Equipment connectors are *Calculated* by default.
- **Equipment sets** group duty and standby pumps (Add and Remove pumps). Only duty units count toward flow.
- *Mechanical Control Device*: thermostats and sensors, specified in MEP-DUCT-012.
- *Connect Into* routes from a selected component's connector into an existing duct or pipe main.

**Data (IFC mapping).**

| Item | IFC |
|------|-----|
| Air terminal | `IfcAirTerminal` (`DIFFUSER`, `GRILLE`, `REGISTER`, `LOUVRE`) |
| VAV box | `IfcAirTerminalBox` |
| AHU | `IfcUnitaryEquipment` (`AIRHANDLER`) |
| Fan | `IfcFan` |
| Boiler | `IfcBoiler` |
| Chiller | `IfcChiller` |
| Pump | `IfcPump` |
| Radiator | `IfcSpaceHeater` |
| Controls | `IfcController`, `IfcSensor` |
| Equipment set | `IfcGroup` with ObjectType "EquipmentSet"; duty and standby roles in RXD element data (`mep_settings`) |

**Commands.**
- `air_terminal.place(type_id, point | duct_face_ref, level, flow?) -> id`
- `mech_equipment.place(type_id, point, level, rotation) -> id`
- `equipment_set.create(equipment_ids[], duty_ids[]) -> id`
- `mep.connect_into(element_id, connector_index, target_id) -> element_ids[]`

**Acceptance criteria.**
- [ ] Placing three 100 L/s supply diffusers in a space sets its actual supply airflow to 300 L/s.
- [ ] In a duty/standby set of two pumps, only the duty pump's flow appears in system totals.

### 4.7.10 Duct and pipe color fill legends
**ID:** MEP-DUCT-010 · **Priority:** P2 · **Ref:** research/03 §8.1 ("Color fill")

**Purpose.** Show at a glance how a calculated or descriptive value varies across duct and pipe networks in a plan or section view.

**Workflow.**
1. In a plan, ceiling plan or section view, choose *Analyze › Color Fill › Duct Legend* or *Pipe Legend*.
2. Click to place the legend. If the view has no scheme for that kind, a *Choose Colour Scheme* dialog lists the existing duct or pipe schemes and offers *Edit*.
3. Select the legend and choose *Edit Scheme* to change the parameter, the ranges and the colours.

**Behaviour & rules.** *Analyze › Color Fill › Duct Legend / Pipe Legend* places a legend and colours runs by a chosen parameter, such as flow, velocity, friction, pressure or size. Colour schemes are shared with chapter 5's colour-fill engine.
- **Parameters offered:** for ducts, flow, velocity, friction, pressure drop, size, system type and system name; for pipes, the same plus fluid temperature and slope. Calculated values need the system's *Calculations* to include them (MEP-SYS-001); elements without a value use the scheme's *No value* colour (default: none).
- **Scheme kinds:** *By value* (each distinct value gets a colour) or *By range* (bands with editable boundaries, default 5 equal bands over the current min–max). Colours come from theme-aware palettes and every legend row carries a text label, so meaning does not depend on colour alone.
- Fill applies to segments and, optionally, to fittings and accessories (scheme option *Include fittings*, default on). Insulation takes the colour of its host.
- One duct legend and one pipe legend per view. Deleting the legend removes the fill from the view. The fill is a view setting and changes no element.
- Values update when the network calculation finishes (MEP-CALC-002); stale results are drawn hatched until recalculation completes.

**Data (IFC mapping).** No model data. Schemes are stored in `views/color_schemes` (kind `colorscheme`) and the legend as view annotation (`anno.colorlegend`), both chapter 5.

**Commands.** `mep_color_legend.place(view_id, kind: "duct"|"pipe", scheme_id, point) -> id`. Schemes are created and edited with `colorscheme.create` (chapter 5).

**Acceptance criteria.**
- [ ] A velocity scheme with bands at 4 and 8 m/s colours each segment in the band for its velocity.
- [ ] Placing a second duct legend in the same view is refused; a pipe legend in the same view is accepted.
- [ ] Deleting the legend removes all duct colouring from that view and from no other view.
- [ ] A segment whose system has *Calculations = None* shows the *No value* colour in a velocity scheme.

### 4.7.11 Duct and pipe sizing
**ID:** MEP-CALC-001 · **Priority:** P2 · **Ref:** research/03 §8.5, §9.1

**Workflow.** Select a run or system, choose *Analysis › Duct/Pipe Sizing*, pick a method and constraints, then OK.

**Behaviour & rules.**
- **Duct methods:**
  - **Friction** and/or **Velocity**, combined as *Only*, *And* or *Or*
  - **Equal Friction**, with a default target of 1 Pa/m (≈ 0.1 in. w.g. per 100 ft)
  - **Static Regain**
- **Pipe methods:** Friction and/or Velocity (Only, And or Or). Pipe sizing is not offered for sanitary systems; drainage sizing is a separate open question.
- **Constraints:**
  - *Branch sizing*: calculated size only, match connector size, or the larger of the two
  - **Restrict Height** and **Restrict Width** (and restrict inner diameter for pipe)
  - Size constraints win over flow targets. Each conflict raises a warning.
- Sizing needs a connected system with valid flow. **Size-locked** segments are skipped.
- **Fitting loss data:** loss coefficients come from each fitting type's data table. That is either a coefficient, a coefficient table by geometry ratio, or a specific loss. Reviera ships open coefficient tables built from public-domain sources and lets content authors supply their own. Reviera does **not** bundle any proprietary fitting-loss database (§4.17).

**Commands.** `mep.size(element_ids[] | system_id, method, targets: {friction?, velocity?}, combine, branch_rule, restrict?: {height?, width?, inner_diameter?}) -> report`

**Acceptance criteria.**
- [ ] Equal Friction at 1 Pa/m on a 3-branch supply system sizes each segment to the smallest size in the *Used in Sizing* table whose friction is at or below 1 Pa/m.
- [ ] Restrict Height = 300 mm keeps every rectangular segment at or below 300 mm high and widens segments to compensate.
- [ ] Size-locked segments keep their size, and the report lists them as skipped.

### 4.7.12 Pressure drop and analytical segments
**ID:** MEP-CALC-002 · **Priority:** P2 · **Ref:** research/03 §8.5 · screenshots/structure_mep/analytical_duct_segments_tag.png · screenshots/structure_mep/analytical_duct_segment_schedule_critical_path.png

**Purpose.** A network calculation produces **analytical segments**: sub-elements that represent the flow path and carry results.

**Behaviour & rules.**
- Pressure drop is calculated from geometry, roughness, fluid properties (air density and viscosity, or the pipe fluid table) and fitting losses. The friction equation is the one selected in settings.
- Each **analytical duct or pipe segment** carries flow, velocity, friction, pressure drop, **network ID** and a **critical path** flag.
- Analytical segments can be selected with Tab, tagged and scheduled. They take per-segment overrides, for example a measured loss. They also work over fabrication parts (§4.10).
- The calculation runs in the background (ANA-GEN-001) when a system's *Calculations* setting is All or Performance.

**Data (IFC mapping).**
- Results are written to the physical segments' standard occurrence psets: `Pset_DuctSegmentOccurrence` and `Pset_PipeSegmentOccurrence`, plus flow-performance values in `Pset_DistributionPortCommon`/port psets where they are defined.
- Analytical segments themselves are computed sub-elements, not IFC entities. Their overrides and the critical-path cache are **RXD element data** (`analysis`) on the physical segment.

**Commands.**
- `mep_network.calculate(system_id | "all") -> job_id`
- `analytical_segment.list(system_id) -> segments[]`
- `analytical_segment.override(segment_ref, params)`

**Acceptance criteria.**
- [ ] A schedule of analytical duct segments filtered by "critical path = yes" lists exactly the segments on the highest-loss path from the fan to a terminal.
- [ ] Changing one segment's size marks the network results stale until recalculation completes.

### 4.7.13 System Inspector
**ID:** MEP-CALC-003 · **Priority:** P2 · **Ref:** research/03 §8.5 ("System Inspector")

**Purpose.** Let the engineer explore a calculated duct or pipe system interactively in a view and see where pressure is lost.

**UI.** Contextual tab *System Inspector*: *Inspect* (toggle), *Properties* (opens the selected section's values in a read-only panel), *Exit*. While active, the canvas shows flow arrows on every section, a hover tooltip, and a small legend explaining the critical-path highlight in words.

**Behaviour & rules.**
- Select a system and choose *Analysis › System Inspector › Inspect*.
- Flow arrows are drawn. Hovering shows flow, static pressure and pressure loss for each section.
- The **critical path is highlighted**, using a theme warning colour rather than a hard-coded red. Terminals show their excess pressure relative to the critical path.
- The inspector needs a well-connected system. It is not offered for fire-protection systems.
- A *section* is a stretch of the network with constant flow between two junctions; hovering any element of it shows the section's values. Values come from the latest network calculation (MEP-CALC-002); if results are stale, the inspector first starts a recalculation and shows a progress bar until it completes.
- Preconditions and refusals: the system must have equipment, *Calculations = All* or *Performance*, and no open connectors; otherwise *Inspect* is refused with the first failing condition named ("System has open connectors: 3. Run Show Disconnects").
- The inspector is a view session: it changes no model data, and editing the model while it is active ends the session.
- Units follow the project's HVAC or piping units (chapter 2 units).

**Data (IFC mapping).** None. It reads analytical-segment results.

**Commands.**
- `system_inspector.start(system_id, view_id)`
- `system_inspector.stop()`
- Section values are also available to scripts through `analytical_segment.list` (MEP-CALC-002).

**Acceptance criteria.**
- [ ] Hovering a branch segment shows its flow and static pressure, and the critical path is visually distinct.
- [ ] *Inspect* on a system with an open connector is refused and the message gives the number of open connectors.
- [ ] The excess pressure shown at a terminal equals the critical-path loss minus that terminal's path loss, within 1 Pa.

### 4.7.14 Pressure loss report
**ID:** MEP-CALC-004 · **Priority:** P2 · **Ref:** research/03 §8.5 ("Pressure Loss Report")

**Purpose.** Produce a shareable, per-section record of a system's pressure losses for review, submittals and equipment selection.

**Workflow.**
1. Open from *Analyze › Reports & Schedules › Pressure Loss Report* (duct or pipe), or right-click a system in the System Browser.
2. In *Select Systems*, tick systems; a filter limits the list by classification. Only systems with *Calculations = All* are selectable; others are shown greyed with the reason.
3. In *Report Settings*, choose the fields per table, the sections to include and the format, then *Generate* and choose a file path.

**Behaviour & rules.**
- Open from *Analyze › Reports & Schedules › Pressure Loss Report*, or right-click in the System Browser.
- Choose systems, then the report settings: fields, system information, critical path and fitting detail.
- Output is HTML or CSV. Systems must have *Calculations = All*.
- **Report sections:** project information; system information (name, type, classification, total flow, equipment); a *Total pressure loss* table by section; *Segment detail* (length, size, flow, velocity, friction, loss); *Fitting and accessory detail* (part type, loss method, coefficient, loss); and *Critical path* (the ordered list of sections and its total).
- Report settings can be saved as named presets in `model/mep` (`mep_settings`), so a team uses the same format.
- Stale results are recalculated before the report is written. If recalculation fails, the report is not written and the error is shown.
- HTML uses the project's units with a units line in the header; CSV writes values in SI base units with a header row naming units, so it can be re-imported by other tools.
- Elements with user overrides (MEP-CALC-002) are flagged in an *Overridden* column.

**Data (IFC mapping).** None in the model; the output is a file. Presets are RXD document `mep_settings`.

**Commands.** `pressure_loss_report.export(system_ids[], settings, path, format: "html"|"csv")`. `settings` is `{preset_id? | fields{}, sections[], include_critical_path, include_fittings}`.

**Acceptance criteria.**
- [ ] The CSV report for a system lists every segment and fitting with flow, size, velocity and loss, and the totals match the System Inspector.
- [ ] A system with *Calculations = Flow Only* cannot be ticked, and the dialog states why.
- [ ] A segment with an overridden loss is marked in the *Overridden* column.

### 4.7.15 Sizing Calculator palette
**ID:** MEP-CALC-005 · **Priority:** P2 · **Ref:** research/03 §8.5 ("MEP Sizing Calculator palette")

**UI.** A dockable palette with:
- volumetric flow, taken from the selection or typed in
- sizing basis: friction, velocity or aspect ratio, each with a tolerance slider
- roughness
- constraints: restrict height, width or inner diameter
- **Show Best Sizes**: a sortable table of candidate sizes, with *Swap Height/Width*
- *Apply*, which resizes the selected duct or pipe

Open it from *View › User Interface › Sizing Calculator* or the right-click menu of a selected duct or pipe. It docks with the other palettes of this chapter (§4.0.3).

**Purpose.** Size a single duct or pipe by hand, comparing candidate sizes against friction, velocity and aspect-ratio targets, without running system sizing.

**Behaviour & rules.**
- **Inputs.** With one duct or pipe selected, the palette reads its flow (calculated or preset), shape, type and roughness. *User value* lets the user type a flow instead. With nothing or several elements selected, the palette works in calculator mode and *Apply* is disabled.
- **Basis and tolerance.** Friction target (default 1 Pa/m), velocity target (default 5 m/s for duct, 1.5 m/s for pipe) and, for rectangular and oval duct, aspect ratio (default 1:3 maximum). Each slider sets a ± tolerance from 0 to 50% (default 10%). Several bases can be ticked; a candidate must satisfy all ticked bases.
- **Candidates** come from the *Used in Sizing* size tables (MEP-SET-001) for the element's shape, or for pipe from the segment's sizes. Friction uses the project's selected equation and fluid properties, as in MEP-CALC-002.
- **Table columns:** size, friction, velocity, aspect ratio, equivalent diameter, and the deviation from each target. Sorting by any column; the row closest to the friction target is pre-selected. *Swap Height/Width* swaps rectangular candidates in the table.
- If no size meets all bases, the table shows the three nearest sizes greyed with a note "No size within tolerance".
- **Apply** resizes the selected element in one undoable transaction, with fittings regenerating. It is refused on size-locked elements ("Element is size-locked").
- Palette inputs are per-user session state; nothing is stored in the model.

**Data (IFC mapping).** None, except the size change made by *Apply*, which is an ordinary geometry and pset update.

**Commands.**
- `sizing_calculator.candidates(flow, basis, tolerance, roughness, constraints) -> sizes[]`
- `pipe.set_size(ids[], diameter)`: the pipe counterpart of `duct.set_size` (MEP-DUCT-001). *Apply* calls one of the two; system sizing (`mep.size`) is not involved.

**Acceptance criteria.**
- [ ] For 1 000 L/s at 1 Pa/m ± 10%, the table lists only sizes whose friction falls in 0.9–1.1 Pa/m, and *Apply* resizes the selection.
- [ ] With friction and aspect ratio (max 1:3) both ticked, no listed rectangular candidate has an aspect ratio above 1:3.
- [ ] *Apply* on a size-locked duct is refused, and the duct keeps its size.

### 4.7.16 Risers and rise/drop display
**ID:** MEP-DUCT-011 · **Priority:** P2 (riser tool) / P1 (rise/drop display) · **Ref:** research/03 §7.2 (system type rise/drop symbols), §8.1, §9.1, §10.7 · web: Specify Rise Drop Symbols for a Piping System; Specifying Rise Drop Symbols for a Duct System

**Purpose.** Create vertical duct, pipe, cable tray and conduit runs between levels from an open end, keep them bound to their levels, and show standard rise and drop symbols wherever a run turns up or down in a plan view.

**Workflow.**
1. Select a duct, pipe, cable tray or conduit with an open end (or an equipment connector), then choose *Systems › Risers › Create Riser*.
2. Click the open connector to rise from. Only open connectors of the four routing domains are pickable.
3. In the contextual tab pick the **target level** and an **offset** from it (default 0). A target below the start gives a drop.
4. Press *Finish*. Reviera inserts an elbow from the routing preferences at the start, draws one vertical segment of the same type, size and system, and leaves the top end open.

Risers can also be drawn by changing the middle elevation mid-run and pressing *Apply* (MEP-DUCT-001), or by drawing in a section or elevation; those risers are ordinary segments and are not level-bound unless the user binds them (below).

**UI.** Contextual tab "Modify | Create Riser": target level drop-down, offset field, *Bind top to level* check box (on by default), *Finish*, *Cancel*. Selected riser segments show a **Top Level** and **Top Offset** in Properties.

**Behaviour & rules.**
- **Level binding.** A level-bound riser stores its top level and offset. When that level's elevation changes, the vertical segment stretches and the open end or connected fitting moves with it. Unbinding keeps the current geometry.
- Errors: no elbow row fits the size ("No Elbow in routing preferences for 300 mm"), target equals the start elevation ("Riser length is zero"), or the connector is not vertical-capable (for example a face-based surface connector): the tool refuses and names the reason. Nothing is created.
- **Rise/drop display.** In plan and ceiling plan views, a vertical segment is drawn with a **rise** symbol when it continues above the cut plane, or a **drop** symbol when it continues below the view's bottom, using the geometry, not the flow direction. A vertical branch from a tee uses **tee up** or **tee down**.
- Symbol sets are chosen per system type (MEP-SYS-001) for ducts and pipes, with six slots: two-line rise, two-line drop, single-line rise, single-line drop, single-line tee up, single-line tee down. Cable tray and conduit take them from Electrical Settings (MEP-SET-002); fabrication parts use the system type of their service's system (MEP-FAB-007).
- Single-line symbols are used at Coarse detail (and Medium for pipe); two-line symbols at the other levels. The symbol size is the rise/drop annotation size in Mechanical or Electrical Settings, scaled with the view.
- Symbols are generated graphics: they are not selectable, cannot be deleted one at a time and follow visibility of their category's *Rise/Drop* subcategory.

**Micro-operation schema and replay (§2.1).** `riser.pick_connector{element_id, connector_index}`, `riser.set_target{level_id, offset, bind}`, `riser.finish`, `riser.cancel`. Replay re-runs the ops in order against the recovered model; a pick whose connector is no longer open ends the replay with the session cancelled.

**Data (IFC mapping).** Riser segments are normal `IfcDuctSegment`, `IfcPipeSegment` or `IfcCableCarrierSegment` occurrences with a vertical axis, connected by `IfcRelConnectsPorts`. Level binding (top level GlobalId, offset) is **RXD element data** (`constraints`). Symbol choices are in **RXD document** `mep_settings` (system types) and `electrical`. Symbols are never exported as IFC.

**Commands.** `mep_run.create_riser(element_id, connector_index, top_level, top_offset?) -> element_ids[]` (the elbow and the vertical segment).

**Acceptance criteria.**
- [ ] From an open 200 mm pipe end on Level 1, `mep_run.create_riser` to Level 3 + 300 mm creates one elbow and one vertical pipe whose top is at Level 3 elevation + 300 mm.
- [ ] Raising Level 3 by 500 mm lengthens the bound riser by 500 mm, and the run stays connected.
- [ ] In a Level 1 plan, the riser shows the system type's single-line rise symbol at Coarse and the two-line rise symbol at Fine; in a Level 3 plan cut below its top it shows nothing extra, and in a plan above it shows a drop symbol.
- [ ] A riser requested where no elbow row fits is refused, and no elements are created.

### 4.7.17 Mechanical control devices
**ID:** MEP-DUCT-012 · **Priority:** P2 · **Ref:** research/03 §1.4, §8.4 · web: Place Mechanical Control Device

**Purpose.** Place thermostats, sensors (temperature, CO₂, humidity, occupancy, pressure) and humidistats, and relate each device to the equipment, terminal or system-zone it controls.

**Workflow.**
1. Choose *Systems › Mechanical › Mechanical Control Device* and pick a type in the Type Selector.
2. Choose a placement mode on the contextual tab: *Place on Vertical Face* (default), *Place on Face* or *Place on Work Plane*. Set the offset or mounting height in Properties.
3. Optionally keep *Tag on Placement* on, with tag orientation (horizontal or vertical) and a leader length.
4. Click to place. Spacebar rotates the device by 90° before the click.
5. Optionally choose *Assign Controlled Element* and pick the equipment, VAV box, damper or system-zone the device controls.

**Behaviour & rules.**
- A face-hosted device moves with its host face and reports its offset from that face; when the host is deleted the device stays in place, unhosted, with a warning.
- A device may carry an electrical connector (Controls or Power) so it can be circuited (MEP-ELEC-002), and a duct connector for duct-mounted sensors. Controls wiring is not modelled.
- A device controls zero or more elements. Assigning a controlled element of another discipline is allowed; assigning the device to itself or to another control device is refused.
- The space a device sits in is found by its room calculation point, as for lighting fixtures (MEP-ELEC-001).
- Control devices are schedulable with their controlled elements and space.

**Data (IFC mapping).** Thermostats and controllers are `IfcController` (`PROGRAMMABLE`, `FLOATING`, `PROPORTIONAL`, `MULTIPOSITION`, `TWOPOSITION`) or `IfcUnitaryControlElement` (`THERMOSTAT`, `HUMIDISTAT`); sensors are `IfcSensor` (`TEMPERATURESENSOR`, `CO2SENSOR`, `HUMIDITYSENSOR`, `MOVEMENTSENSOR`, `PRESSURESENSOR`, …), each with its type psets. The control relationship is `IfcRelFlowControlElements` (the device as RelatedControlElements, the controlled element as RelatingFlowElement). Control of a system-zone is stored as **RXD element data** (`mep_settings`).

**Commands.**
- `mech_control_device.place(type_id, point, level, host_face_ref?) -> id`
- `mech_control_device.assign(device_id, controlled_ids[])`

**Acceptance criteria.**
- [ ] A thermostat placed on a wall at 1 500 mm offset exports as `IfcUnitaryControlElement` (`THERMOSTAT`) contained in the storey, hosted to the wall face.
- [ ] Assigning the thermostat to a VAV box exports an `IfcRelFlowControlElements` relating them.
- [ ] Deleting the host wall leaves the thermostat in place with a warning.

---

## 4.8 Piping, plumbing and fire protection

### 4.8.1 Pipe placement
**ID:** MEP-PIPE-001 · **Priority:** P1 · **Ref:** research/03 §9.1

**Workflow.** Choose *Systems › Plumbing & Piping › Pipe* (proposed `PI`) or *Pipe Placeholder*. Set the diameter, middle elevation and system type, then click points.

**UI.** Contextual tab:
- Justification
- Automatically Connect
- Inherit Elevation and Inherit Size
- **Slope Off / Slope Up / Slope Down** and **Slope Value**
- Show Slope Tooltip
- *Add Vertical* or *Change Slope*, which decide how a sloped pipe connects
- Tag on Placement

**Behaviour & rules.**
- Pipe types use routing preferences with **Segment** rows, which give the material and schedule per size range.
- Fittings are inserted automatically. Vertical pipe is created by changing the elevation and pressing Apply, or by drawing in section or elevation.

**Data (IFC mapping).**
- `IfcPipeSegment` (`RIGIDSEGMENT`), typed by `IfcPipeSegmentType`.
- Property sets: `Pset_PipeSegmentTypeCommon`, `Pset_PipeSegmentOccurrence` (InteriorRoughnessCoefficient, Gradient = slope) and `Qto_PipeSegmentBaseQuantities`.
- Material from the segment: `IfcMaterialProfileSetUsage` with a circular hollow profile.

**Commands.** `pipe.create(type_id, start, end, diameter, middle_elevation, level_id, system_type_id?, slope?) -> element_ids[]`

**Acceptance criteria.**
- [ ] Drawing a 50 mm copper pipe with an elbow uses the segment and elbow given by the routing preferences for 50 mm.
- [ ] A pipe drawn at 1% Slope Down exports `Gradient` = 0.01.

### 4.8.2 Sloped piping and the slope editor
**ID:** MEP-PIPE-002 · **Priority:** P1 · **Ref:** research/03 §9.1 ("Sloped piping")

**Behaviour & rules.** There are four ways to slope pipe:
1. Set a slope while drawing.
2. Use the slope controls on a segment.
3. Use the **Slope Editor**:
   1. Tab-select a section and choose *Slope*.
   2. Pick a value from the allowed slopes list.
   3. Set the **slope control point**. The default is the lowest end.
   4. Finish.

   The slope propagates through fittings, and connected branches adjust.
4. Draw at an angle in a section view.

Allowed values come from Mechanical Settings › Slopes. Fabrication pipework can also be sloped (§4.10).

**Purpose.** Give gravity drainage and other sloped pipework a controlled, consistent fall that survives later edits.

**UI.**
- While drawing: *Slope Off / Slope Up / Slope Down* and *Slope Value* on the contextual tab, with an optional slope tooltip that shows the slope and the end elevation at the cursor.
- On a selected segment: a slope glyph with an editable value at its midpoint and an arrow that flips the direction of fall.
- **Slope Editor** contextual tab: *Slope Value* (drop-down of allowed values), *Slope Control Point* (cycles the arrow glyph between the ends of the selected section), *Finish* and *Cancel*.

**Behaviour & rules.**
- Slope is stored as a ratio (rise ÷ run) and displayed in the project's slope unit (percent by default; ratio or degrees selectable in Project Units).
- **Propagation.** The editor walks the selected section from the control point. Each segment gets the chosen slope; elbows and couplings move with it; tees keep their branches connected by moving the branch's first segment's end and recalculating that segment's slope if it is not in the selection.
- If a branch connection cannot be kept (for example the branch is size-locked and level-locked, or would need a slope outside the allowed list), the editor shows the conflict on the canvas and *Finish* is disabled until the user removes that branch from the section or cancels.
- Slopes not in the allowed list cannot be chosen in the editor. A slope typed directly on a segment that is not in the list is accepted with a warning, so imported models stay valid.
- Vertical segments (≥ 45° when the warning threshold is crossed, MEP-SET-001) are excluded from propagation and keep their geometry.
- **Recovery.** The editor journals `slope.select{element_ids[]}`, `slope.value{ratio}` and `slope.control{end_ref}` as micro-operations; replay restores the pending edit (conventions §2.1). *Finish* commits one transaction.

**Data (IFC mapping).** `Pset_PipeSegmentOccurrence.Gradient` on each segment (MEP-PIPE-001). The section's control point is not stored after *Finish*.

**Commands.** `mep_run.set_slope(element_ids[], slope, control_point_ref)`: refused with "Selection is not a single connected section" when the elements do not form one run.

**Acceptance criteria.**
- [ ] Applying 2% to a 20 m sanitary run with the control point at the downstream end raises the upstream end by 0.4 m, and branch connections remain connected.
- [ ] Each segment of the run then exports `Gradient` = 0.02.
- [ ] A slope value not in the allowed list is not offered in the Slope Editor.
- [ ] A branch that cannot be kept connected blocks *Finish* and is highlighted.

### 4.8.3 Parallel pipes
**ID:** MEP-PIPE-003 · **Priority:** P1 · **Ref:** research/03 §9.1 ("Parallel Pipes")

**Purpose.** Create banks of pipes that follow an existing run, such as supply and return pairs or several services on a shared rack, in one step.

**Workflow.**
1. Choose *Systems › Plumbing & Piping › Parallel Pipes*.
2. On the contextual tab set *Horizontal Number*, *Horizontal Offset*, *Vertical Number* and *Vertical Offset*.
3. Hover a pipe: the preview shows the new runs on the side of the cursor (left/right in plan, above/below in section). Tab extends the hover to the whole run.
4. Click to create.

**Behaviour & rules.**
- Creates copies of an existing run with a horizontal count, vertical count and offsets. Tab selects the whole run.
- Not allowed on runs that contain tees, crosses or valves. The refusal names the element that blocks it.
- Counts include the original: a horizontal number of 3 adds two runs. Limits: 1–20 per direction; offsets must be at least the pipe's outer diameter plus insulation thickness, otherwise the command is refused ("Offset smaller than pipe outside size").
- Offsets are measured centre to centre. At bends, each new run is offset so it stays parallel; its elbows are chosen by routing preferences for the same size, so inner runs get tighter centre-line radii when the fitting allows, and the result is concentric.
- New runs copy type, size, system type, slope and insulation, but get **new system membership**: each copy joins the original's system only if it is connected to equipment; otherwise it is left unassigned and reported.
- Open ends are left open; the tool does not connect copies to equipment.
- Parallel pipes are independent elements after creation; no link to the original is kept.

**Data (IFC mapping).** Ordinary `IfcPipeSegment` and `IfcPipeFitting` elements (MEP-PIPE-001, MEP-PIPE-004).

**Commands.** `pipe.create_parallel(run_element_ids[], h_count, v_count, h_offset, v_offset) -> element_ids[]`

**Acceptance criteria.**
- [ ] Parallel pipes of 3 × 1 at 150 mm on an L-shaped run produce three concentric L-runs with correct elbows.
- [ ] A run containing a gate valve is refused, and the message names the valve.
- [ ] An offset of 40 mm on a 50 mm pipe is refused.
- [ ] Copies of a 1% sloped run have the same 1% slope.

### 4.8.4 Pipe fittings, accessories, flanges, caps and flex
**ID:** MEP-PIPE-004 · **Priority:** P1 · **Ref:** research/03 §9.1

**Purpose.** Place and edit the parts that complete a pipe run: fittings, inline accessories, flanges, caps, insulation and flexible connections.

**Workflow.**
1. *Pipe Fitting* (`PF`) or *Pipe Accessory* (`PA`): pick a type, hover a pipe end or centreline (in plan, section or elevation), click to place.
2. *Flex Pipe* (`FP`): click a start connector or point, intermediate points, and an end connector.
3. *Cap Open Ends*: select pipes and choose the command from the contextual tab or context menu.

**Behaviour & rules.**
- **Pipe fittings** (proposed `PF`) have size, flip, rotate and "+/−" controls, and *Change Tee to Cross*.
- **Pipe accessories** (proposed `PA`): valves, strainers and meters, inserted by clicking on a pipe centreline. Valves can be placed in section or elevation.
- **Flanges** are added automatically per routing preferences (union role), or manually.
- **Caps** come from *Cap Open Ends*.
- **Insulation** works as for ducts.
- **Flex pipe** (proposed `FP`): spline with vertices.
- **Fitting placement.** A fitting placed on a pipe end connects and takes the pipe's size. Placed along a pipe, a tee or cross splits the pipe; elbows and transitions can only go on ends ("Elbows must be placed at an open end"). A fitting placed in free space keeps its family's default size and is unconnected.
- **Accessory sizing.** The accessory adopts the pipe's nominal size when its family links size to its connectors; a fixed-size accessory on a different pipe size gets transitions from routing preferences or is refused if none fits.
- **Flanges and unions.** When the accessory's connection type (for example flanged) differs from the pipe segment's, the union-role row of the routing preferences supplies a matching flange or adapter on each side. Manually placed flanges replace the automatic ones and are not removed by *Reapply Type*.
- **Caps** take the end's size and the cap row of the routing preferences; an end with no matching cap row is reported and left open.
- **Flex pipe** uses the same spline rules, recovery micro-operations (`flex.pick`, `flex.set`) and length quantity as flex duct (MEP-DUCT-005).
- Valve orientation (handle direction) can be rotated in 90° steps with Spacebar before placement and with the rotate control afterwards.

**Data (IFC mapping).**

| Item | IFC |
|------|-----|
| Fitting | `IfcPipeFitting` (`BEND`, `JUNCTION`, `TRANSITION`, `CONNECTOR`, `OBSTRUCTION`, …) |
| Valve | `IfcValve` (`ISOLATING`, `CHECK`, `GATE`, `GLOBE`, …) |
| Meter | `IfcFlowMeter` |
| Strainer | `IfcFilter` (`STRAINER`) |
| Flex pipe | `IfcPipeSegment` (`FLEXIBLESEGMENT`) |
| Insulation | `IfcCovering` (`INSULATION`) |

**Commands.**
- `pipe_fitting.place(type_id, host_pipe_id | point, params) -> id` (params: size?, rotation?, end_ref?)
- `pipe_accessory.place(type_id, pipe_id, point) -> id`
- `mep_fitting.tee_to_cross(id)`
- `flex_pipe.create(type_id, points[], start_port?, end_port?) -> id`
- Caps use `duct.cap_open_ends`' pipe counterpart `pipe.cap_open_ends(ids[])`; fitting controls use `mep_fitting.*` (MEP-DUCT-003); insulation uses `mep_insulation.*` (MEP-DUCT-004).

**Acceptance criteria.**
- [ ] Inserting a gate valve into a 65 mm pipe splits the pipe and adds flanges from the union role on both sides.
- [ ] Placing an elbow on the middle of a pipe is refused with a message.
- [ ] *Cap Open Ends* on a run with three open ends adds three caps of matching size.
- [ ] *Change Tee to Cross* on a tee adds a fourth open leg of the run size.

### 4.8.5 Pipe sizing and fixture units
**ID:** MEP-PIPE-005 · **Priority:** P2 · **Ref:** research/03 §9.1 ("Pipe sizing")

**Behaviour & rules.**
- Pipe sizing uses MEP-CALC-001 with the Friction and Velocity methods. It is not used for sanitary systems.
- Domestic water flow can come from **fixture units**, converted to flow by a selectable method. Reviera's built-in method uses the probabilistic fixture-unit curves from public-domain sources, such as the original U.S. government reports on fixture-unit theory. Users can load their own tables for local codes.
- Velocity limits can depend on annual operating hours, to control noise and erosion.
- Pressure drop uses the Colebrook or Haaland equation.

**Purpose.** Size domestic water and hydronic pipework from its flow, where domestic flow is derived from fixture units rather than summed directly.

**Behaviour & rules (detail).**
- **Flow source.** Fixture connectors carry fixture units (MEP-CONN-001). For domestic cold and hot water systems, each segment's design flow is the conversion of the *sum of downstream fixture units*, not the sum of fixture flows, because simultaneous use is probabilistic. Hydronic systems use summed connector flows.
- **Separate curves** apply to systems dominated by flush tanks and by flush valves; the method picks the curve from a per-fixture *flush valve* flag. If a system mixes both, the flush-valve curve is used and a note is written to the sizing report.
- **Fixture-unit table format.** A user table is a CSV with columns `fixture_units, flow_tank, flow_valve` in ascending order; flow is interpolated linearly between rows. Import validates monotonic rows and refuses others with the line number. The source and edition are stored with the table so the report can name them.
- **Velocity by operating hours.** An optional table of `hours_per_year, max_velocity` (per material) caps velocity; the default table is empty, which means only the user's velocity target applies.
- **Fluids.** Density and viscosity come from the system type's fluid and temperature (MEP-SET-001). A fluid temperature outside the fluid table is refused with "Temperature outside fluid table range".
- **Report.** Sizing writes a report listing each segment's fixture units, design flow, chosen size, velocity and friction, and the conversion method used.

**Data (IFC mapping).** Sizes are ordinary geometry and `Pset_PipeSegmentTypeCommon`/occurrence updates. Fixture units on sanitary terminals go to the terminal's type psets where defined, and otherwise RXD element data (`mep_settings`). The selected method and user tables are RXD document `mep_settings`.

**Commands.**
- `fixture_unit_method.set(method_id | table_path)`
- `mep.size(...)` (shared with MEP-CALC-001)

**Acceptance criteria.**
- [ ] For a given fixture-unit total, the built-in method returns the documented flow within 2%.
- [ ] Colebrook and Haaland agree within 2% for turbulent flow in 50 mm steel pipe.
- [ ] A segment downstream of fixtures totalling 40 fixture units is sized for the converted flow of 40 units, not the sum of the fixtures' individual flows.
- [ ] Importing a fixture-unit CSV with a non-ascending row is refused and the message names the line.

### 4.8.6 Plumbing fixtures, equipment and sanitary workflow
**ID:** MEP-PIPE-006 · **Priority:** P1 · **Ref:** research/03 §9.2

**Behaviour & rules.**
- *Plumbing Fixture* (proposed `PX`) is usually hosted, with *Place on Vertical Face*, *Place on Face* or *Place on Work Plane*. Spacebar rotates.
- *Plumbing Equipment* (water heaters, pumps) is placed like mechanical equipment.
- **Sanitary workflow:**
  1. Select fixtures and use *Create Systems › Piping*, choosing Sanitary or another default (Domestic Cold Water, Domestic Hot Water, Other).
  2. Optionally tile views (proposed `WT`).
  3. Run *Generate Layout* with slope settings.
  4. Add vents, traps and connections between levels by hand.
- A plumbing plan view template is supplied (chapter 5).

**Purpose.** Place sanitary fixtures and plumbing equipment with working connectors, and take them from placement to connected sanitary and domestic water systems.

**UI.** *Systems › Plumbing & Piping › Plumbing Fixture* opens "Modify | Place Plumbing Fixture" with the placement buttons *Place on Vertical Face*, *Place on Face* and *Place on Work Plane*, *Tag on Placement*, and the elevation/offset in Properties. Tiled views (`WT`) are a window command (chapter 1).

**Behaviour & rules.**
- **Hosting.** A face-hosted fixture moves with its host face, including faces of linked models (it keeps a reference to the link face and warns if the link face disappears). A work-plane fixture keeps its offset from the plane. If the host is deleted, the fixture stays in place, becomes unhosted, and a warning is raised.
- **Connectors.** Fixtures typically carry sanitary, vent, cold-water and hot-water connectors, each with its classification and fixture units. Creating a system from a selection only uses connectors of the chosen classification; fixtures without such a connector are skipped and listed.
- **Default systems.** If a project has no system type for a needed classification, Reviera creates one named after the classification the first time it is needed.
- **Sanitary layout.** *Generate Layout* for sanitary systems applies the slope chosen in its settings and runs downhill toward the equipment or stack connector; solutions that would require rising in the flow direction are discarded.
- Vents and traps placed by hand join systems through their connectors like any other fitting or accessory.
- **Plumbing equipment** behaves as mechanical equipment (MEP-DUCT-009): Spacebar rotates by 90°, connectors are *Calculated* by default.

**Data (IFC mapping).** `IfcSanitaryTerminal` (`WASHHANDBASIN`, `WCSEAT`, `SINK`, `SHOWER`, `BATH`, `URINAL`, …), `IfcWasteTerminal` (`FLOORTRAP`, `GULLYTRAP`, …) and `IfcInterceptor`. Water heaters are `IfcBoiler` (`WATER`) or `IfcTank`. Hosting on a face is RXD element data (`hosting`); the fixture is contained in the storey of its level.

**Commands.**
- `plumbing_fixture.place(type_id, host_face_ref | work_plane, point, rotation) -> id`
- Plumbing equipment is placed with `mech_equipment.place` (MEP-DUCT-009); systems are created with `system.create` (MEP-SYS-002) and layouts with `mep_layout.generate` (MEP-DUCT-008).

**Acceptance criteria.**
- [ ] Six basins placed on a wall and combined into a sanitary system appear under one Sanitary node in the System Browser.
- [ ] Moving the host wall by 200 mm moves the six basins with it.
- [ ] Creating a Domestic Hot Water system from a selection of a basin and a WC skips the WC (no hot-water connector) and lists it.
- [ ] A sanitary *Generate Layout* solution never has a segment that rises in the flow direction.

### 4.8.7 Fire protection (sprinklers)
**ID:** MEP-PIPE-007 · **Priority:** P1 (placement) / P2 (hydraulic calculation) · **Ref:** research/03 §9.3

**Behaviour & rules.**
- *Sprinkler* (proposed `SK`): pendent heads hang below and are piped from above; upright heads are the reverse. Hosting options match plumbing fixtures.
- The user chooses heads by temperature rating, response, K-factor and orifice. Reviera does not size or select heads automatically.
- The fire-protection classifications are Wet, Dry, Pre-Action and Other.
- **[Suggestion] (P2)** A sprinkler hydraulic-calculation provider (ANA-PLUG-001 kind `hydraulic`). It would cover Hazen–Williams friction, K-factor head discharge, and demand at the remote area. The reference product does not offer this, and it would fill an open-tool gap. The System Inspector and pressure loss report stay unavailable for fire-protection systems until a provider is installed.

**Data (IFC mapping).** `IfcFireSuppressionTerminal` (`SPRINKLER`), with `Pset_FireSuppressionTerminalTypeSprinkler` (ResponseTimeIndex, ActivationTemperature, CoverageArea, DischargeFlowRate, KFactor, …).

**Commands.** `sprinkler.place(type_id, point | host_face_ref, level) -> id`

**Acceptance criteria.**
- [ ] A sprinkler with K = 80 and 68 °C activation exports those values in `Pset_FireSuppressionTerminalTypeSprinkler`.

### 4.8.8 Hydronic network analysis and hydraulic separation
**ID:** MEP-PIPE-008 · **Priority:** P2 · **Ref:** research/03 §9.4 · screenshots/structure_mep/hydronic_network_primary_loop.png

**Behaviour & rules.**
- When **hydronic network analysis** is on, closed-loop networks report flow and pressure drop on their pumps. The switch is in Mechanical Settings › Pipe › Calculation.
- A **valid network** needs all of the following:
  - one pump per loop
  - a source (boiler or chiller)
  - pipe segments
  - terminals in series or parallel
  - a direct or reverse return
- **Hydraulic separation** (*Modify | Pipes › Hydraulic Separation › Add/Remove*) splits a network into primary, secondary and tertiary loops at a decoupler such as a close tee or a buffer tank. Each loop's pump reports only its own loop's pressure drop. Primary pumps add up the flows of the loops they serve.
- **[Suggestion]** EPANET (public domain/MIT) could serve as an alternative hydraulic provider for large networks.

**Data (IFC mapping).** Results on `IfcPump` go in `Pset_PumpOccurrence` and the pump performance psets where they are defined. Separation points are RXD element data (`analysis`) on the separating fitting.

**Commands.**
- `hydronic.set_enabled(bool)`
- `hydraulic_separation.add(fitting_or_pipe_id)`
- `hydraulic_separation.remove(id)`

**Acceptance criteria.**
- [ ] After a separation is added between a primary chiller loop and a secondary AHU loop, the secondary pump's reported head excludes the chiller's pressure drop.

### 4.8.9 Analytical pipe connections
**ID:** MEP-PIPE-009 · **Priority:** P2 · **Ref:** research/03 §9.4 ("Analytical Pipe Connections")

**Behaviour & rules.**
- *Create › Analytical Connections* makes logical connections from equipment connectors to pipe curves or open pipe ends, without modelling the physical connection. They are used by the hydronic calculation.
- A connection that cannot project perpendicularly onto a pipe attaches to the nearest pipe end.
- *Design to Fabrication* (MEP-FAB-006) refuses selections that contain analytical connections.

**Purpose.** Connect equipment to pipe mains for calculation early in design, before the short final connections (valve sets, flex hoses) are modelled.

**Workflow.**
1. Select one or more pieces of mechanical equipment (or select pipes first and then pick equipment).
2. Choose *Modify | Mechanical Equipment › Create › Analytical Connections*.
3. Pick the target pipe for each open equipment connector, or let Reviera choose: each connector is matched to the nearest pipe of the same system classification within a search radius (default 3 m).
4. Review the preview (dashed lines from connector to attachment point) and *Finish*.

**Behaviour & rules.**
- Only pipe-domain connectors that are open and whose classification matches the pipe's system can be connected. Mismatches are skipped and listed ("Connector 2: Hydronic Return, pipe is Hydronic Supply").
- The attachment point is the perpendicular projection of the connector onto the pipe centreline; if it falls outside the pipe, the nearest pipe end is used.
- An analytical connection carries flow in the hydronic calculation (MEP-PIPE-008) and system membership, but no geometry, no pressure loss of its own (an optional fixed loss can be set as a property), and no quantity.
- It is drawn as a dashed annotation-weight line in plan and 3D, controlled by its own subcategory so that it can be hidden for documentation.
- Moving the equipment or the pipe updates the attachment point. Deleting either deletes the connection. Replacing it with a physical connection (drawing a pipe from the same connector) deletes the analytical connection automatically.
- *Check Systems* treats an analytical connection as connected; *Show Disconnects* does not flag the connector.
- **Recovery.** Each pick is journalled as `analytical_conn.pick{connector_ref, pipe_id}`; replay restores the pending set (conventions §2.1).

**Data (IFC mapping).** `IfcRelConnectsPorts` between the equipment port and a virtual port. The virtual port is an `IfcDistributionPort` nested in the pipe, with an RXD element data (`analysis`) flag "analytical", and no RealizingElement. The optional fixed loss is RXD element data (`analysis`).

**Commands.**
- `analytical_pipe_connection.create(equipment_id, connector_index, pipe_id) -> id`
- Connections are deleted with `element.delete`.

**Acceptance criteria.**
- [ ] A fan-coil connected only through analytical connections contributes its flow to the loop pump.
- [ ] A connector whose projection falls beyond the end of the pipe attaches to the nearest pipe end.
- [ ] Drawing a physical pipe from the same connector removes the analytical connection.
- [ ] A hydronic return connector is not connected to a supply pipe, and the skip is listed.

---

### 4.8.10 Roof drainage: gutters and downspouts
**ID:** MEP-PIPE-010 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** Design a roof's rainwater drainage as part of the plumbing subsystem: gutters along the eaves, outlets, and downspouts to the ground or to a storm drain. *(Owner direction, 2026-10-13: "A gutters tool is a good idea, though it may be more appropriate to make it part of the plumbing subsystem.")* The eave (ARC-ROOF-010) gives the gutter its line and the fascia it hangs on, but it does not own the gutter.

**Workflow.**
1. Systems › Plumbing › **Gutter**: the user picks eaves of a roof (Tab picks a chain of eaves around hips). The gutter follows the eave line, hung on the fascia's face (or on the rafter tails when there is no fascia).
2. **Outlet** and **Downspout** tools place outlets on a gutter, each with a downspout dropping to a level, a point on the ground, or a connection to a storm pipe (MEP-PIPE-001).

**Behaviour & rules.**
- A gutter is a run of the *Storm Drainage* system (MEP-PIPE-006), with a profile type (half-round, K-style or box), a size and a fall (default 1:500) toward its outlets. Its ends and corners take end caps and mitres.
- The gutter regenerates with the roof: changing the eave's overhang, fascia or bearing moves it.
- Sizing: from the roof area draining to each outlet (the plan area of the faces above the gutter's eaves) and a design rainfall intensity, the gutter and downspout sizes are checked (MEP-PIPE-005 style) and flagged when too small.

**Data (IFC mapping).** Gutter: `IfcPipeSegment` (`PredefinedType = GUTTER`), assigned to the storm `IfcDistributionSystem`, with an `IfcRelConnectsElements` to the roof. Downspouts: `IfcPipeSegment` (`RIGIDSEGMENT`) with `IfcPipeFitting` bends; outlets as `IfcPipeFitting` (`CONNECTOR`).

**Commands.**
- `gutter.create(roof_id, edges: [curve_key], type_id?, fall?) -> element_id`
- `gutter.add_outlet(gutter_id, at, downspout_to?: level_id | point | pipe_id) -> element_id`
- `gutter.size(gutter_id, rainfall?) -> report`

**Acceptance criteria.**
- [ ] A gutter along the four eaves of a hip roof is one mitred run hung on the fascias, falling toward its outlet, and exports as `IfcPipeSegment(GUTTER)` in the storm system.
- [ ] Changing the roof's overhang moves the gutter with the eave.
- [ ] A downspout from an outlet to the ground level is a vertical run with bends at the eave offset.

### 4.8.11 Automatic penetrations for pipes, ducts, conduits and cable trays
**ID:** MEP-PEN-001 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); Reviera's own design (Appendix A.2a)

**Purpose.** Openings in walls, floors, roofs and beams where MEP runs pass through them, made and kept automatically from the overlaps the volume overlap engine finds (ARC-MOD-030). *(Owner direction, 2026-10-13: "automatic opening creation for pipes, conduits, and other HVAC entities when developing the Mechanical features".)*

**Behaviour & rules.**
- **Detection.** When a pipe, duct, conduit or cable tray is placed or edited, its overlaps with hosts (walls, floors, ceilings, roofs and, by option, structural framing) are found.
- **Openings.** Each crossing gets an opening in the host:
  - round or rectangular after the run's profile, plus a clearance (per system type, default 25 mm), or a sleeve of a chosen type;
  - fire-rated hosts get a penetration-seal annotation (the host's rating).
- **Following the run.** The opening follows the run: it moves, resizes and is removed with it. A run that no longer passes through removes its opening.
- **Rules.** Per host category and per system classification:
  - *create*: automatic, the default for walls and floors;
  - *flag only*: report the crossing without opening it, the default for structural framing, which needs an engineer's approval;
  - *never*.
- **Coordination.** Openings in another discipline's (linked) model are proposed as provisions-for-voids (BCF topics with the opening's geometry) rather than made.

**Data (IFC mapping).** `IfcOpeningElement` (`PredefinedType = OPENING`) voiding the host (`IfcRelVoidsElement`), and, with a sleeve, an `IfcPipeSegment` (`SLEEVE`) or `IfcDuctSegment` filling it. Provisions for voids are `IfcVirtualElement` / `IfcBuildingElementProxy` (`PROVISIONFORVOID`) per the IFC4 reference views.

**Commands.** `mep.penetrations(run_ids?) -> [crossing]`, `mep.set_penetration_rule(host_category, system?, mode: create|flag|never, clearance?, sleeve_type?) -> ()`.

**Acceptance criteria.**
- [ ] A 100 mm pipe through a 200 mm wall gets a 150 mm round opening (25 mm clearance) through the wall, which follows the pipe when it moves 500 mm along the wall.
- [ ] A duct through a beam is flagged and not opened under the default rules.

## 4.9 Electrical

Placing electrical components, cable tray and conduit is **P1**. Circuiting, panel schedules, demand factors and the electrical analytical model are **P2**.

### 4.9.1 Electrical equipment, devices and lighting fixtures
**ID:** MEP-ELEC-001 · **Priority:** P1 · **Ref:** research/03 §10.1

**Behaviour & rules.**
- *Electrical Equipment* (proposed `EE`): panelboards, switchboards, transformers and motor control centres. They are wall-hosted or free-standing, with Tag on Placement.
- *Device* split button: electrical fixtures (receptacles), communication, data, fire alarm, lighting (switches), nurse call, security and telephone. Most devices are face-hosted.
- *Lighting Fixture* (proposed `LF`): usually ceiling- or wall-hosted.
  - The family's **room calculation point** decides which space the fixture belongs to, for schedules and lighting calculations.
  - **Photometric web files** (IES LM-63, and EULUMDAT as an addition) describe the light distribution. They are used by ANA-LIGHT-001/002 and by rendering.

**Data (IFC mapping).**

| Item | IFC |
|------|-----|
| Panelboard, switchboard, MCC | `IfcElectricDistributionBoard` (`DISTRIBUTIONBOARD`, `SWITCHBOARD`, `MOTORCONTROLCENTRE`, `CONSUMERUNIT`) with `Pset_ElectricDistributionBoardTypeCommon` / `Pset_ElectricDistributionBoardOccurrence` |
| Transformer | `IfcTransformer` |
| Receptacle and outlets | `IfcOutlet` (`POWEROUTLET`, `DATAOUTLET`, `TELEPHONEOUTLET`, `COMMUNICATIONSOUTLET`, `AUDIOVISUALOUTLET`) |
| Switch | `IfcSwitchingDevice` (`TOGGLESWITCH`, `DIMMERSWITCH`, …) |
| Fire alarm and security devices | `IfcAlarm`, `IfcSensor` |
| Lighting fixture | `IfcLightFixture` (`POINTSOURCE`, `DIRECTIONSOURCE`) with `Pset_LightFixtureTypeCommon` |
| Light source | `IfcLightSourceGoniometric`, which references the photometric file through `IfcExternalReference` |

**Commands.**
- `electrical_equipment.place(type_id, point | host_face_ref, level) -> id`
- `device.place(type_id, host_face_ref | point, level) -> id`
- `lighting_fixture.place(type_id, host_face_ref | point, level) -> id`

**Acceptance criteria.**
- [ ] A lighting fixture whose calculation point lies inside Space 101 is listed under Space 101 even when its body crosses into Space 102.
- [ ] A fixture with an IES file exports `IfcLightSourceGoniometric` that references the file.

### 4.9.2 Creating circuits
**ID:** MEP-ELEC-002 · **Priority:** P2 · **Ref:** research/03 §10.2

**Workflow.**
1. Select devices or fixtures.
2. Choose *Create Systems › Power*, or Data, Telephone, Fire Alarm, Nurse Call, Security, Communication or Controls.
3. If the devices use instance-based distribution, a *Circuit Information* dialog asks for voltage and poles.

The circuit is drawn as dashed lines from each device to the panel, or to a home-run arrow. Two in-canvas controls convert the circuit into **permanent wiring**, arc or chamfered.

**Behaviour & rules.**
- Every device on a power circuit must share a compatible distribution system (voltage and number of poles). Devices that are not compatible are skipped and listed.
- Circuits are logical. Adding wires does not create circuits.

**Data (IFC mapping).** `IfcDistributionCircuit` (a subtype of `IfcDistributionSystem`) with PredefinedType `ELECTRICAL`, `LIGHTING`, `DATA`, `COMMUNICATION`, `SECURITY`, `TELEPHONE`, `CONTROL` or `SIGNAL`. Members via `IfcRelAssignsToGroup`. The panel and the protective device are related through ports (MEP-ELEC-006).

**Commands.** `circuit.create(kind, element_ids[], voltage?, poles?) -> circuit_id`

**Acceptance criteria.**
- [ ] Six 230 V receptacles create one power circuit. A 400 V 3-phase motor in the same selection is skipped and reported.
- [ ] The circuit exports as `IfcDistributionCircuit` with 6 assigned members.

### 4.9.3 Circuit editor and panel selection
**ID:** MEP-ELEC-003 · **Priority:** P2 · **Ref:** research/03 §10.2 ("Circuit Editor", "Panel connections")

**Purpose.** Change the members of an existing circuit and choose the panel (or transformer) that feeds it, without deleting and re-creating the circuit.

**Workflow.**
1. Select a circuit (Tab from a device, or pick it in the System Browser) and choose *Modify | Electrical Circuits › Edit Circuit*.
2. The mode greys out everything except the circuit members, the candidate devices and panels.
3. Toggle *Add to Circuit* or *Remove from Circuit* and click devices or fixtures.
4. Choose a panel from the **Select Panel** drop-down (with a search box) or click a panel in the canvas.
5. *Finish* commits all changes in one transaction; *Cancel* discards them.

*Disconnect Panel* (outside the mode) removes the panel from the circuit and leaves the members circuited.

**UI.** Contextual tab "Edit Circuit": Add to Circuit, Remove from Circuit, Select Panel (searchable list showing panel name, distribution system and free slots), Properties, Finish, Cancel. Members are drawn with the selection colour; candidates that cannot be added are drawn halftone, and hovering one shows the reason.

**Behaviour & rules.**
- A device can be added only when its connector system type matches the circuit kind (power, data, fire alarm…) and, for power, its voltage and pole count match the circuit's distribution system. Otherwise the click is refused with "Incompatible: <reason>".
- A device already on another circuit through the same connector is moved, with a notice naming the old circuit. Devices with several electrical connectors ask which connector to use.
- Removing the last member deletes the circuit on *Finish*, after confirmation.
- The panel list offers only panels and transformers whose distribution system is compatible and that have enough free poles for the circuit. Panels in linked models are not offered.
- Selecting a panel assigns the circuit to the first free slot that fits its pole count; circuit numbers follow the panel's naming scheme (MEP-ELEC-012).
- *Disconnect Panel* frees the slot; the circuit keeps its members, loses its number and is listed as unassigned in the System Browser.
- **Micro-operations (§2.1):** each add, remove and panel pick in the mode is journalled as `{op: "add"|"remove"|"panel", element_id}`. Replay after a crash re-applies the list to the circuit's state at mode entry and reopens the mode.

**Data (IFC mapping).** Membership: `IfcRelAssignsToGroup` on the `IfcDistributionCircuit`. Panel: `IfcRelConnectsPorts` between the circuit's home-run port and the board's port (MEP-ELEC-006). Slot and pole positions are RXD element data (`electrical`) on the circuit.

**Commands.**
- `circuit.add(circuit_id, element_ids[])`
- `circuit.remove(circuit_id, element_ids[])`
- `circuit.select_panel(circuit_id, panel_id)`
- `circuit.disconnect_panel(circuit_id)`
- `circuit.compatible_panels(circuit_id, search?) -> panels[]`

The edit mode itself runs as a tool session through `tool.start`, `tool.finish` and `tool.cancel` (Chapter 1).

**Acceptance criteria.**
- [ ] A circuit cannot be assigned to a 120/208 V panel when its devices are 230 V. The search lists only compatible panels.
- [ ] Adding a receptacle that is on circuit LP1-3 to circuit LP1-5 moves it, and LP1-3's load drops by the receptacle's load.
- [ ] *Cancel* after three adds and one panel change leaves the circuit exactly as it was.
- [ ] After *Disconnect Panel* the circuit has no number and appears as unassigned, and its members are unchanged.

### 4.9.4 Circuit path and length
**ID:** MEP-ELEC-004 · **Priority:** P2 · **Ref:** research/03 §10.2 ("Circuit path")

**Purpose.** Give each circuit a measurable route from the panel to its devices, so that circuit length, voltage drop and cable sizing are based on a realistic distance rather than straight lines.

**Workflow.**
1. Select a circuit and choose *Modify | Electrical Circuits › Edit Path*. The current path is drawn as a heavy dashed polyline with node grips, in plan or in 3D.
2. Pick a **path mode** on the contextual tab.
3. In *Custom* mode, drag nodes, add nodes by clicking a segment, delete nodes with Delete, and add **risers** by setting a node's elevation (a vertical segment is created between two nodes at the same plan position).
4. Set the **path offset** and click *Finish* (or *Cancel*).

**Behaviour & rules.**
- **Path modes:**
  - *Farthest Device* (default): the orthogonal route from the panel connector to the most distant device; other devices add no length.
  - *All Devices*: a daisy-chain through every device, ordered by nearest neighbour from the panel.
  - *Custom*: user-edited nodes, including **risers** (vertical segments). Custom paths do not update when devices move; they are flagged "path may be out of date" in the circuit's warnings until edited or reset.
- Default routes are orthogonal (X, then Y, then Z) in the project coordinate system.
- **Path offset** (default from Electrical Settings › General › *Circuit Path Offset*) lifts or lowers horizontal runs relative to the device connectors, for example to run in a ceiling void. Range ±10 m.
- *Reset Path* (in Custom mode) recomputes the default route for the chosen base mode and discards the nodes.
- Switching from *Custom* to another mode discards the nodes after confirmation.
- A circuit without a panel has no path; *Edit Path* is disabled with the tooltip "Select a panel first".
- Circuit length = path length + the runs' allowance for vertical drops at each device. It drives voltage drop and cable sizing (MEP-ELEC-005), which re-run when the length changes.
- **Micro-operations (§2.1):** each node move, insert, delete, riser edit and mode change is journalled as `{op, node_index, point?}`. Replay re-applies them to the path saved at mode entry.

**Data (IFC mapping).** The path (mode, nodes, offset) is RXD element data (`electrical`) on the circuit. The resulting length is written to a circuit length property in the electrical pset where IFC defines one, and otherwise to RXD. The path is not exported as geometry.

**Commands.**
- `circuit.set_path(circuit_id, mode, nodes?[], offset?)`
- `circuit.reset_path(circuit_id)`
- `circuit.path(circuit_id) -> {mode, nodes[], length}`

The *Edit Path* mode runs as a tool session (`tool.start`/`tool.finish`/`tool.cancel`, Chapter 1).

**Acceptance criteria.**
- [ ] Switching from Farthest Device to All Devices increases the reported length by the extra daisy-chain distance, and voltage drop updates.
- [ ] Raising a 4-device circuit's offset by 1 m increases the Farthest Device length by 2 m (up and down), and nothing else changes.
- [ ] Moving a device on a Custom-path circuit flags the path as possibly out of date; *Reset Path* clears the flag.
- [ ] Adding a riser node 3 m above the previous node adds exactly 3 m to the circuit length.

### 4.9.5 Circuit properties, cable sizing and voltage drop
**ID:** MEP-ELEC-005 · **Priority:** P2 · **Ref:** research/03 §10.2 ("Circuit properties", "Wire sizing")

**Behaviour & rules.**
- **Read-only (calculated) properties:**
  - circuit naming index, phase label, way, start slot
  - **circuit number**
  - panel and system type
  - load classification
  - apparent load and current per phase
  - power factor
  - frame
  - length
  - **voltage drop**
  - number of elements
- **Editable properties:** load name, **rating**, connection type (breaker or feed-through lugs), cable type, number of runs, and cable size.
- **Automatic cable sizing:**
  - Reviera picks the smallest conductor that does both of these: carries the rated current after correction factors, and keeps voltage drop at or below a project limit (default 3%).
  - The calculation uses the wiring type or cable type tables.
  - A size the user enters is respected, with a warning if it fails the checks.

**Data (IFC mapping).**
- `Pset_DistributionSystemTypeElectrical` (MaximumAllowedVoltageDrop, NumberOfLiveConductors).
- Protective device: `IfcProtectiveDevice` (`CIRCUITBREAKER`) with `Pset_ProtectiveDeviceTypeCircuitBreaker`. It is nested as a port-bearing component of the board, or written as a virtual device. The approach is chosen in §4.17 Q6.
- Cable: an `IfcCableSegment` occurrence (or type only) with conductor data.
- Calculated values that have no pset home are RXD element data (`electrical`).

**Commands.**
- `circuit.set(circuit_id, {load_name?, rating?, cable_type?, runs?, cable_size?})`
- `circuit.size_cable(circuit_id, max_vd_percent?) -> size`

**Acceptance criteria.**
- [ ] A 16 A, 230 V single-phase circuit 40 m long with a 2 kW load gets the smallest conductor in the cable table whose voltage drop is ≤ 3%. The reported drop matches a hand calculation within 0.1 percentage points.
- [ ] Editing *Rating* to 20 A re-runs cable sizing.

### 4.9.6 Panel connections, feeders and transformers
**ID:** MEP-ELEC-006 · **Priority:** P2 · **Ref:** research/03 §10.2 ("Panel connections")

**Purpose.** Build the physical distribution hierarchy: switchboards feeding panels, panels feeding sub-panels, and transformers changing voltage between them, so that loads and demand roll up to the service entrance.

**Workflow.**
1. Select a child panel (or transformer) and choose *Create Systems › Power*. A feeder circuit is created from the child's supply connector.
2. Choose *Select Panel or Transformer* and pick the parent. The search list shows only compatible parents.
3. Optionally set the feeder's **connection type** to *Feed-Through Lugs* (the parent must have *Feed-Through Lugs* turned on in its properties).

**Behaviour & rules.**
- **Feeders:** the child then becomes a load on its parent panel. Its connected load is the sum of its own circuits; the demand the parent sees is calculated with demand factors by load classification (MEP-ELEC-010), not by adding the child's demand figures.
- **Compatibility:** the feeder's distribution system must match the parent's (phases, configuration, line-to-line voltage). A **transformer** bridges two systems: its primary must match the parent and its **secondary distribution system** (required; the feeder is refused with "Transformer has no secondary distribution system" until it is set) becomes the system for everything downstream.
- **Feed-through lugs:** a feeder on lugs takes no breaker slot on the parent schedule and is listed in a separate *Feed-through* row. Only one lug feeder per parent is allowed unless the panel type says otherwise.
- **Loops are refused:** a panel cannot feed itself directly or through any chain; the message names the chain.
- A distribution element chooses its upstream supply with *Select Supply*. Changing it moves the feeder and all downstream loads; panel schedules of both old and new parents update.
- Deleting a parent panel disconnects the feeder; the child keeps its circuits and reports "No supply".
- Roll-up runs incrementally on each transaction and is parallel per branch of the tree.

**Data (IFC mapping).** Feeders are `IfcDistributionCircuit` between the ports of two boards. Ports connect with `IfcRelConnectsPorts`. Hierarchy: `IfcRelAssignsToGroup` of the child board into the parent's circuit. Transformers are `IfcTransformer` with `Pset_TransformerTypeCommon` (PrimaryVoltage, SecondaryVoltage, MaximumApparentPower). Connection type, lug flag and the secondary distribution system are RXD element data (`electrical`).

**Commands.**
- `panel.create_feeder(child_panel_id, parent_panel_id, connection: "breaker"|"lugs") -> circuit_id`
- `panel.set_supply(panel_id, source_id)`
- `transformer.set_secondary(transformer_id, distribution_system_id)`
- `panel.hierarchy(root_id?) -> tree`

**Acceptance criteria.**
- [ ] A sub-panel with 12 kVA of connected lighting load shows that load, with demand applied, on the parent panel's schedule summary.
- [ ] A 120/208 V panel cannot be fed directly from a 480 V board, but is accepted when fed through a transformer whose secondary distribution system is 120/208 V.
- [ ] Trying to make panel A feed panel B when B already feeds A is refused, and the message names both panels.
- [ ] A lug-connected feeder occupies no slot in the parent's schedule.

### 4.9.7 Wires
**ID:** MEP-ELEC-007 · **Priority:** P2 · **Ref:** research/03 §10.2 ("Wiring")

**Purpose.** Draw wiring on plans as documentation of how devices are connected, with conductor tick marks and home runs, independent of the logical circuit.

**Workflow.**
1. Choose *Systems › Electrical › Wire* and a kind: arc (proposed `EW`), spline or chamfered. Pick a wire type in the Type Selector (it carries the wiring type and tick-mark style).
2. Click a device connector (snaps show connector points), then intermediate points, then the next connector. A spline or chamfered wire takes as many intermediate points as needed; an arc wire takes one mid point per span.
3. End on a connector to join two devices, or click in empty space to end with a **home-run arrow**. Press Esc to finish.
4. Alternatively, on a circuit, click the arc or chamfer control to generate permanent wiring for the whole circuit (MEP-ELEC-002).

**Behaviour & rules.**
- **Adding wires does not create circuits** and does not change circuit membership. A wire between devices on different circuits is allowed and flagged in *Check Circuits* only if the user turns on "wire/circuit mismatch".
- **Conductors:** hot, neutral and ground counts default from the circuit's wiring type (for example 1/1/1 for a single-phase branch). They can be edited per wire; minimum 1 hot, maximum 12 of each.
- **Tick marks** follow Electrical Settings › Wiring: one symbol per conductor kind, an optional slanted line across them, and a show/hide switch. Ticks are drawn at the middle of the longest span.
- **Home-run arrows** show the panel and circuit number as a tag; the arrow count equals the number of circuits carried.
- Wire crossings draw a gap of the size set in Wiring settings on the wire drawn later (the lower in draw order).
- Wires are view-specific annotation. They can be tagged, have vertex and end grips, and stay attached to connectors when devices move; if a connector is deleted, the end becomes free and a warning is issued.
- **Micro-operations (§2.1):** each click is journalled as `{op: "point"|"connector", point | connector_ref}`; replay redraws the unfinished wire in the same view.

**Data (IFC mapping).** View-specific annotation (chapter 5), stored in `views/annotation/<viewId>` as wire objects with points, kind, connector references (GlobalId + connector index) and conductor counts. Conductor counts are RXD element data (`electrical`). Wires are not exported as model elements. Cable runs that must be modelled are created as `IfcCableSegment` (MEP-ELEC-005).

**Commands.**
- `wire.create(view_id, kind: "arc"|"spline"|"chamfer", points[], connectors[], conductors?: {hot, neutral, ground}) -> id`
- `wire.set_conductors(ids[], {hot?, neutral?, ground?})`
- `circuit.create_wiring(circuit_id, view_id, kind: "arc"|"chamfer") -> ids[]`

**Acceptance criteria.**
- [ ] A wire drawn between two receptacles and ending in empty space shows a home-run arrow and tick marks that match its conductor counts.
- [ ] Moving a receptacle drags the attached wire end with it.
- [ ] Deleting a wire never changes any circuit's members or load.
- [ ] Setting a wire to 2 hot, 1 neutral, 1 ground redraws four tick marks of the configured styles.

### 4.9.8 Switch systems
**ID:** MEP-ELEC-008 · **Priority:** P2 · **Ref:** research/03 §10.3

**Purpose.** Record which switch controls which lighting fixtures, for lighting control documentation and switch-ID schedules, separately from the power circuits.

**Workflow.**
1. Select lighting fixtures and choose *Create Systems › Switch*, or right-click a fixture connector and choose *Create Switch System*.
2. Reviera creates the system and opens **Edit Switch System** mode.
3. Use **Add to System** / **Remove from System** to click further fixtures, and **Select Switch** to click a lighting switch device.
4. *Finish* commits; *Cancel* discards.

**Behaviour & rules.**
- Switch systems are **independent** of circuits and wiring. Fixtures on one power circuit can belong to different switch systems, and one switch system can span several circuits and panels.
- A fixture belongs to at most **one** switch system. Adding it to another moves it, with a notice naming the old system.
- Only lighting fixtures (and devices whose family declares a switchable connector) can be members. Only switching devices of the *Lighting* device kind can be selected as the switch; other picks are refused with "Not a lighting switch".
- A switch system may exist without a switch; it is then reported by *Check Circuits* as "switch system without switch" (information only).
- The **Switch ID** parameter on fixtures and on the switch is set from the system name (default "$1", "$2"… per level, editable) and is schedulable and taggable.
- Deleting the switch keeps the system and clears its switch. Deleting the last fixture deletes the system.
- **Micro-operations (§2.1):** each add, remove and switch pick is journalled as `{op, element_id}`; replay re-opens the mode with the same state.

**Data (IFC mapping).** `IfcDistributionSystem` (`LIGHTING`, ObjectType "SwitchSystem"). Members via `IfcRelAssignsToGroup`. The controlling switch is related with `IfcRelFlowControlElements` where it applies, and otherwise recorded in RXD element data (`electrical`). The Switch ID is written to the system's `Name` and to each member's RXD.

**Commands.**
- `switch_system.create(fixture_ids[], switch_id?) -> id`
- `switch_system.select_switch(id, switch_id)`
- `switch_system.add(id, fixture_ids[])`
- `switch_system.remove(id, fixture_ids[])`

The edit mode runs as a tool session (`tool.start`/`tool.finish`/`tool.cancel`, Chapter 1).

**Acceptance criteria.**
- [ ] Two switch systems can contain fixtures that are on the same power circuit.
- [ ] Adding a fixture that belongs to switch system $1 to $2 removes it from $1.
- [ ] Selecting a receptacle as the switch is refused.
- [ ] A schedule of lighting fixtures shows each fixture's Switch ID.

### 4.9.9 Load classifications
**ID:** MEP-ELEC-009 · **Priority:** P2 · **Ref:** research/03 §10.4

**Purpose.** Group electrical loads by kind, so that panel schedules can summarise them and apply the right demand factor to each group.

**UI.** *Manage › MEP Settings › Load Classifications* (`mep_settings.show(kind: "load_classification")`). A two-pane dialog: the list of classifications on the left (New, Duplicate, Rename, Delete), and on the right the selected classification's **demand factor** (drop-down, with a button that opens the Demand Factors dialog, MEP-ELEC-010), an optional **space load** flag, and an *Other loads* note. The dialog validates on OK.

**Behaviour & rules.**
- Defaults are Lighting, Power, HVAC, Motor and Other. Users can add their own, up to 256 per project. Names are unique (case-insensitive) and 1–64 characters.
- Each classification refers to a **demand factor** definition. A new classification defaults to a constant 100% demand factor.
- Classifications are assigned to electrical connectors in the family editor or on placed elements. A **sub-classification** (free text) can be added for finer grouping in schedules. Shared parameters for connector load classification can be created so the value can be scheduled.
- **Space load flag:** when *Load Calculations › run for spaces* is on (MEP-SET-002), classifications with this flag contribute to space electrical load densities (for example lighting and power loads used by ANA-HCL-001).
- **Deleting** a classification that connectors use asks for a replacement; *Other* is proposed. The five defaults can be renamed but not deleted.
- Loads with no classification count as *Other*.
- Changing a classification's demand factor re-runs panel roll-up (MEP-ELEC-006) in the background.

**Data (IFC mapping).** **RXD document** `electrical` (definitions: id, name, demand factor id, space flag). On ports and devices, the classification name is also written to the element's electrical pset where one exists, for example `Pset_ElectricalDeviceCommon` and `Pset_OutletTypeCommon`.

**Commands.**
- `load_classification.create(name, demand_factor_id) -> id`
- `load_classification.set(id, {name?, demand_factor_id?, space_load?})`
- `load_classification.delete(id, replace_with)`
- `connector.set(element_id, index, {load_classification})`

**Acceptance criteria.**
- [ ] Changing a classification's demand factor changes every panel schedule summary row that uses it.
- [ ] Deleting "Kitchen Equipment" while 12 connectors use it asks for a replacement, and afterwards all 12 show the replacement.
- [ ] Deleting the default *Lighting* classification is refused.
- [ ] Creating a second classification named "hvac" is refused as a duplicate.

### 4.9.10 Demand factors
**ID:** MEP-ELEC-010 · **Priority:** P2 · **Ref:** research/03 §10.4

**Behaviour & rules.** Calculation methods:

| Method | Variant | Example |
|--------|---------|---------|
| **Constant** | — | 80% |
| **By quantity** | Whole total at one percentage, picked by count range | 1–2 items: 100%; 3–10 items: 65% |
| **By quantity** | Incremental per range | — |
| **By load** | Whole total at one percentage | — |
| **By load** | Incremental per load range | First 10 kVA at 100%, remainder at 50% |

- An optional **largest-motor rule** weights motors in descending order with configurable percentages, for example 125%, 100%, 75% and 50%.
- Demand factors are applied when loads roll up to panels and feeders.

**Data (IFC mapping).** **RXD document** `electrical`. Panels also write their total demand load to `Pset_ElectricDistributionBoardOccurrence` where the schema defines it.

**Commands.**
- `demand_factor.create(name, method, ranges[], incremental?: bool) -> id`
- `demand_factor.evaluate(id, loads[]) -> demand`

**Acceptance criteria.**
- [ ] Incremental by load, with the first 10 kVA at 100% and the rest at 50%, gives 17.5 kVA for 25 kVA connected.
- [ ] Constant 80% gives 8 kVA for 10 kVA connected.

### 4.9.11 Circuit naming
**ID:** MEP-ELEC-012 · **Priority:** P2 · **Ref:** research/03 §10.5

**Purpose.** Generate circuit names (the "circuit number" shown on tags and panel schedules) from rules, so that projects can follow local conventions such as "1, 3, 5", "L1-3" or "DB2/7".

**UI.** *Electrical Settings › Circuit Naming* lists the schemes, with the project default marked. *New*/*Edit* opens the scheme editor:
- a list of available parts: panel name, circuit naming index, phase label, way, start slot, and any text or number parameter of circuits or panels (including shared parameters);
- the ordered list of chosen parts, each with its own prefix, suffix and separator;
- a live preview for a sample three-phase and a sample single-phase circuit.

**Behaviour & rules.**
- The project's default naming scheme is set in Electrical Settings › Circuit Naming. Built-in schemes are *Numerical*, *Prefixed* and *Panel Name + Number*; built-ins can be duplicated but not edited or deleted.
- A new scheme is made of parameters (panel name, naming index, phase label, way, start slot, shared parameters…) with a prefix, suffix and separators. A scheme needs at least one part that differs between circuits on one panel (index, way or start slot); otherwise saving is refused with "Names would not be unique".
- Multi-pole circuits use the slots they occupy, joined with a comma ("1,3,5") unless the scheme sets *First slot only*.
- Each panel's *Circuit Naming* is *By Project* or a named scheme.
- Circuit numbers update when circuits move in a panel schedule, when the panel's scheme changes, and on *Renumber* or *Update Names*. Locked circuits (MEP-ELEC-015) keep their slot and hence their number.
- If a referenced parameter is empty, the part is omitted together with its prefix and suffix, and the circuit is listed in a warning.
- Deleting a scheme that panels use switches them to *By Project* after confirmation.
- Names are resolved in the background after each transaction; tags and schedules read the resolved value.

**Data (IFC mapping).** Schemes: **RXD document** `electrical`. The resolved circuit name is written to `IfcDistributionCircuit.Name`, and the full description (panel and load name) to `LongName`.

**Commands.**
- `circuit_naming_scheme.create(name, parts[]) -> id`
- `circuit_naming_scheme.set(id, {name?, parts?[]})`
- `circuit_naming_scheme.delete(id)`
- `circuit_naming_scheme.preview(parts[], sample?) -> strings[]`
- `panel.set_naming(panel_id, scheme_id | "project")`

**Acceptance criteria.**
- [ ] The scheme "{Panel}-{Index}" names circuit 7 on panel LP1 "LP1-7". After it moves to slot 9 it becomes "LP1-9".
- [ ] A three-pole circuit in slots 13, 15 and 17 is named "13,15,17" by the *Numerical* scheme.
- [ ] A scheme made only of the panel name cannot be saved.
- [ ] Changing the project default updates every panel set to *By Project* and none of the panels set to a named scheme.

### 4.9.12 Panel schedules
**ID:** MEP-ELEC-013 · **Priority:** P2 · **Ref:** research/03 §10.6 · screenshots/structure_mep/panel_schedule_branch_two_column.png

**Workflow.**
1. Set up load classifications, then demand factors, then assign classifications to connectors.
2. Format templates.
3. Create schedules:
   - Select panels of the same type and choose *Create Panel Schedules*, using the default template or a chosen one.
   - For many panels at once, use *Analyze › Reports & Schedules › Panel Schedules* (proposed `PS`).
4. Manage circuits on the schedule.

Panel schedules appear in the Project Browser under *Panel Schedules* and are placed on sheets like other schedules (chapter 5).

**Behaviour & rules.**
- **Default content:**
  - header: panel, voltage, phases, wires, mains rating, mounting, enclosure, location
  - circuit table: load name, trip, poles, circuit number, load per phase, total
  - manufacturer and notes
  - a **loads summary** by load classification: connected load, demand factor and estimated demand
- Schedules are live views of the circuits, not copies.

**Data (IFC mapping).** The schedule view is a chapter 5 view object. Its definition is RXD data (chapter 5), and it references the panel's GlobalId.

**Commands.** `panel_schedule.create(panel_ids[], template_id?) -> view_ids[]`

**Acceptance criteria.**
- [ ] A schedule for a panel with 18 circuits lists them in slot order, with per-phase totals that match the sum of the circuit loads.
- [ ] Changing a circuit's load name in the model updates the schedule cell, and editing the cell updates the model.

### 4.9.13 Panel schedule templates
**ID:** MEP-ELEC-014 · **Priority:** P2 · **Ref:** research/03 §10.6 ("Templates")

**Purpose.** Define how panel schedules look and what they show, once per project, so that every schedule of a panel kind is formatted the same way and can be re-formatted by switching template.

**Workflow.**
1. Choose *Manage › Panel Schedule Templates › Manage Templates* (`panel_schedule_template.manage`).
2. On the *Manage Templates* tab pick a **kind** (Branch Panel, Data Panel, Switchboard), then select a template and use *Duplicate*, *Rename*, *Delete*, *Set as Default* or *Open* (edit).
3. On the *Apply Templates* tab pick panel schedules in the model and assign a template of the matching kind to them in one step.
4. Editing a template opens it in a schedule-like editor view. *Set Template Options* chooses sections, slot count and columns; cells are mapped to parameters; *Finish Template* saves it.

**UI.**
- **Manage Templates dialog.** Two tabs. *Manage Templates*: kind filter, template list with a **default** marker per kind, buttons as above. *Apply Templates*: a list of panel schedule views (filterable by panel kind and by current template) and a template picker; only templates of the schedule's kind are offered.
- **Template editor.** Ribbon "Modify Panel Schedule Template": *Set Template Options*, *Set Parameter* (map the selected cell to a parameter of the panel, circuit or load summary), *Combine Parameters* (several parameters with separators in one cell), *Edit Font*, *Borders*, *Shading*, *Insert/Delete Row and Column*, *Merge/Unmerge*, *Resize*, *Finish Template* and *Cancel*.
- **Template Options** pages: *General Settings* (section visibility for header, circuit table, loads summary and footer; total width; **slot count**, default 42, range 1–168; whether unused slots are shown), *Circuit Table* (column layout and per-phase load columns), *Loads Summary* (which load classifications are listed: all, or only those with connected load).

**Behaviour & rules.**
- Three kinds:
  - **Branch panel**: two columns with circuits across, two columns with circuits down, or one column
  - **Data panel**: a single circuit column
  - **Switchboard**: one row per feeder or distribution circuit
- The editor works on these sections: header, circuit table, loads summary and footer.
- Cells map to parameters, and formatting and merging are supported. A cell mapped to a parameter that a panel does not have shows blank, not an error.
- Defaults are regional: two-column, circuits across, for imperial templates; one column for metric templates. Each kind has exactly one default template; the first template of a kind becomes the default.
- Deleting a template used by schedules asks for a replacement template of the same kind; the default template of a kind cannot be deleted until another is set as default.
- Template edits update every schedule that uses the template, on *Finish Template*, in one transaction. Per-schedule formatting overrides (MEP-ELEC-015) are kept unless they refer to removed rows or columns, which are reported.
- A slot count smaller than the highest occupied slot of a panel that uses the template is refused, and the message names that panel.

**Data (IFC mapping).** **RXD document** `electrical` (templates, kind defaults). Schedule-to-template assignment is part of the schedule view's RXD data (chapter 5).

**Commands.**
- `panel_schedule_template.manage()` (opens the Manage Templates dialog; no model change by itself)
- `panel_schedule_template.create(kind, name, options) -> id`
- `panel_schedule_template.edit(id, cell_map, formatting)`
- `panel_schedule_template.apply(template_id, schedule_view_ids[])`
- `panel_schedule_template.set_default(template_id)`

**Acceptance criteria.**
- [ ] Switching a schedule from a two-column to a one-column template keeps all circuits and re-lays them out.
- [ ] Applying a Data Panel template to a Branch Panel schedule is not offered in *Apply Templates*, and the command form returns a kind-mismatch error.
- [ ] Mapping a header cell to "Mains Rating" and finishing the template shows each panel's mains rating in all 5 schedules that use the template.
- [ ] Setting the slot count to 24 on a template used by a panel with a circuit in slot 30 is refused, naming that panel.

### 4.9.14 Editing circuits in a panel schedule
**ID:** MEP-ELEC-015 · **Priority:** P2 · **Ref:** research/03 §10.6 ("Modify Panel Schedule ribbon") · screenshots/structure_mep/panel_schedule_modify_ribbon.png

**UI.** Contextual "Modify Panel Schedule" tab:
- **Move Across** and **Move Up/Down**
- **Assign Spare**, **Assign Space** and **Remove Spare/Space**
- **Lock/Unlock** (locked circuits do not move)
- **Group/Ungroup**
- **Rebalance Loads** across phases
- **Update Names**
- **Change Template** and **Change Branch Panel**
- font, borders and shading
- insert and delete rows
- merge cells

**Workflow.** Open a panel schedule view, select one or more circuit cells (or empty slots) and use the contextual tab. Moves can also be made by dragging a circuit row to another slot.

**Behaviour & rules.**
- **Move Across** swaps a circuit between the left and right columns of a two-column schedule; **Move Up/Down** moves it by one slot position on the same side, swapping with the neighbour. A move is refused when the target slots are locked, when a multi-pole circuit would cross the end of the panel, or when its poles would no longer land on distinct phases; the message names the blocking circuit.
- **Spares** are reserved breakers with a rating and a load name (default label from Electrical Settings › Panel Schedules). **Spaces** are empty slots prepared for future breakers. Both occupy slots, can be locked, and count in totals only if *Include Spares in Panel Totals* is on. *Remove Spare/Space* frees the slot.
- **Lock/Unlock**: locked circuits, spares and spaces are not moved by Move, Rebalance or Renumber.
- **Group/Ungroup**: grouped circuits move together and keep their relative order; a group cannot mix sides of a two-column schedule.
- **Rebalance Loads** moves unlocked circuits to reduce the phase imbalance. It uses a greedy assignment by descending load, then local swaps, and never changes a circuit's pole count. It reports the imbalance (maximum deviation from the mean phase load, in VA and %) before and after, and does nothing if it cannot improve it.
- **Update Names** rebuilds load names from the load classification and the space name and number of the first device, for circuits whose load name has not been edited by hand (hand-edited names are kept unless *Overwrite edited names* is ticked).
- **Change Template** re-lays the schedule with another template of the same kind (MEP-ELEC-014). **Change Branch Panel** switches the view to another panel of the same template kind.
- **Formatting** (font, borders, shading, insert/delete rows, merge) applies to this schedule only and is stored as overrides of its template; *Reset to Template* removes them.
- Every slot change re-runs circuit naming (MEP-ELEC-012).

**Data (IFC mapping).** Slot positions, locks, groups, spares and spaces are RXD element data (`electrical`) on the panel. Formatting overrides are part of the schedule view's RXD (chapter 5). Spares are not exported as circuits; they are listed in the panel's RXD only.

**Commands.**
- `panel_schedule.move(view_id, circuit_id, to_slot)`
- `panel_schedule.move_across(view_id, circuit_ids[])`
- `panel_schedule.assign_spare(view_id, slot, rating?)`
- `panel_schedule.assign_space(view_id, slot)`
- `panel_schedule.remove_spare_space(view_id, slots[])`
- `panel_schedule.lock(view_id, circuit_ids[], bool)`
- `panel_schedule.group(view_id, circuit_ids[], bool)`
- `panel_schedule.rebalance(view_id) -> {before, after}`
- `panel_schedule.update_names(view_id, overwrite_edited?)`
- `panel_schedule.change_template(view_id, template_id)`
- `panel_schedule.change_panel(view_id, panel_id)`

**Acceptance criteria.**
- [ ] Rebalancing a panel whose phase loads are 12/6/3 kVA, using movable single-pole circuits, reduces the maximum deviation from the mean and leaves locked circuits where they are.
- [ ] Moving a 2-pole circuit so that its second pole would fall beyond the last slot is refused.
- [ ] With *Include Spares in Panel Totals* off, a 20 A spare does not change the panel totals.
- [ ] *Update Names* leaves a hand-edited load name unchanged unless *Overwrite edited names* is ticked.

### 4.9.15 Cable tray
**ID:** MEP-ELEC-016 · **Priority:** P1 · **Ref:** research/03 §10.7, §7.3 ("Cable Tray Settings"), §8.3 (fitting controls)

**Purpose.** Model cable containment (tray and ladder) for coordination, with automatic and manual fittings, so that routes can be checked for clashes and scheduled by length and size.

**Workflow.**
1. Choose *Systems › Electrical › Cable Tray* (proposed `CT`) and a type.
2. Set width, height, middle elevation and bend radius on the contextual tab or in Properties; set justification and *Automatically Connect* as for ducts (MEP-DUCT-001).
3. Click a start point and further points; bends, tees and crosses are inserted where the run turns or meets another tray. Changing the elevation and pressing *Apply* creates a vertical section (rise or drop, MEP-DUCT-011).
4. To place a fitting by hand, choose *Systems › Electrical › Cable Tray Fitting* (proposed `TF`), pick a fitting type and click on a tray end or along a tray.

**UI.** On a selected tray: end grips, size text, "+"/"−" upgrade and downgrade on fittings, flip, rotate, and a bend-radius grip on bends. The context menu has *Draw Cable Tray*, *Split* and *Select* (Tab selects the run).

**Behaviour & rules.**
- Types: ladder, channel, solid bottom and trough, each **with fittings** (bends, tees and crosses are separate `fitting` elements chosen by routing preferences, MEP-DUCT-002) or **without fittings** (bends are drawn as curved portions of the segment itself, with no separate fitting elements; manual fitting placement is refused on these types).
- The bend radius defaults to width × the type's **bend radius multiplier** (default 1.0). A bend radius below half the width is refused.
- **Manual fittings** (`cable_tray_fitting.place`): placed on an open end, the fitting takes the end's size and elevation and connects; placed along a tray, a tee or cross splits the tray and keeps both parts connected. A transition (reducer) is inserted automatically where a fitting leg and the tray differ in width.
- Fitting controls: elbow to tee and tee to cross ("+"), remove an unused leg ("−"), change angle (snaps to the Angles setting), change bend radius, flip and rotate.
- **Split** cuts a straight at a point into two connected segments, with a union fitting if the type defines one.
- Tray carries no circuit logic; cables are not modelled inside it in the first release.
- Fill ratio is not calculated (open question for a later release).

**Micro-operation schema and replay (§2.1).** `tray.pick_point{xyz, snap_ref?}`, `tray.set_param{name, value}`, `tray.apply_elevation{value}`, `tray.finish`, `tray.cancel`. Replay re-applies the ops in order and regenerates fittings from the routing preferences current at replay time.

**Data (IFC mapping).** `IfcCableCarrierSegment` (`CABLETRAYSEGMENT`, `CABLELADDERSEGMENT` or `CABLETRUNKINGSEGMENT`) with `Pset_CableCarrierSegmentTypeCableTraySegment` (or the ladder or trunking variant) and `Qto_CableCarrierSegmentBaseQuantities`. Fittings are `IfcCableCarrierFitting` (`BEND`, `TEE`, `CROSS`, `REDUCER`), typed by `IfcCableCarrierFittingType`, with nested `IfcDistributionPort`s (`CABLECARRIER`). A curved segment of a type without fittings is one `IfcCableCarrierSegment` with a swept body along a composite curve. Justification and bend-radius multiplier are **RXD element data** (`constraints`).

**Commands.**
- `cable_tray.create(type_id, start, end, width, height, elevation, level) -> element_ids[]`
- `cable_tray.split(id, point) -> ids[]`
- `cable_tray_fitting.place(type_id, host_id?, point?, params) -> id` (`host_id` is a tray or an open end's element; `params` holds angle, leg sizes and bend radius)
- The shared fitting controls `mep_fitting.upgrade`, `mep_fitting.downgrade`, `mep_fitting.flip` and `mep_fitting.rotate` (MEP-DUCT-003) also apply to tray fittings.

**Acceptance criteria.**
- [ ] A ladder tray drawn around a corner gets a bend whose radius is width × the type multiplier.
- [ ] Placing a tee fitting along a 300 mm tray splits it into two segments, and all three elements are connected.
- [ ] `cable_tray_fitting.place` on a type without fittings is refused with a message naming the type.
- [ ] "+" on a bend whose run continues straight turns it into a tee with an open third leg.

### 4.9.16 Conduit and parallel conduits
**ID:** MEP-ELEC-017 · **Priority:** P1 · **Ref:** research/03 §10.7, §7.3 ("Conduit Settings")

**Workflow.** Choose *Systems › Electrical › Conduit* (proposed `CN`), pick a type and a diameter from the active standard, set the elevation, then click points as for cable tray (MEP-ELEC-016). To add a fitting by hand, choose *Conduit Fitting* (proposed `NF`) and click on a conduit end or along a conduit.

**Behaviour & rules.**
- *Conduit* has types with or without fittings. Parameters: diameter, offset and bend radius. Standards and sizes come from Electrical Settings; only sizes of the type's **standard** are offered, and a size outside it is refused.
- Rise and drop symbols appear automatically on vertical changes (MEP-DUCT-011).
- **Conduit fittings** (`conduit_fitting.place`): elbows, tees, crosses, couplings (unions), reducers, boxes (pull and junction boxes with up to six connectors) and caps. Placed on an open end, the fitting takes the conduit's diameter and connects; placed along a conduit, a tee, cross, box or union splits it and keeps both parts connected. Manual fittings are refused on types without fittings, which draw bends as curved portions of the segment.
- The fitting controls of MEP-DUCT-003 ("+", "−", flip, rotate) and change-angle apply to conduit fittings. A box has no "−" control.
- The bend radius defaults to the standard's radius for the diameter, or the diameter × the type multiplier where the standard gives none.
- **Parallel Conduits** offers *Same Bend Radius* or *Concentric Bend Radius*, plus horizontal and vertical counts and offsets. Concentric bends are only for conduit types without fittings.
- **Surface connectors** let conduit leave equipment anywhere on a face (*Connect Conduit to Equipment*). Parallel conduits need a run connected through a surface connector or to cable tray.

**Data (IFC mapping).** `IfcCableCarrierSegment` (`CONDUITSEGMENT`) with `Pset_CableCarrierSegmentTypeConduitSegment`. Fittings are `IfcCableCarrierFitting` (`BEND`, `TEE`, `CROSS`, `REDUCER`; couplings and caps as `USERDEFINED` with ObjectType "COUPLING" or "CAP"), typed by `IfcCableCarrierFittingType`. Pull and junction boxes are `IfcJunctionBox` (`POWER` or `DATA`) with `Pset_JunctionBoxTypeCommon`. All carry nested `CABLECARRIER` ports. The conduit standard name is **RXD element data** (`electrical`) on the type. The cables a conduit carries can be related with `IfcRelAggregates` or `IfcRelContainedIn…` patterns where modelled; this is optional.

**Commands.**
- `conduit.create(type_id, start, end, diameter, elevation, level) -> element_ids[]`
- `conduit.create_parallel(run_ids[], h_count, v_count, h_offset, v_offset, bend: "same"|"concentric") -> element_ids[]`
- `conduit_fitting.place(type_id, host_id?, point?, params) -> id` (`params` holds angle, diameters per leg and, for boxes, the connector layout)

The drawing tool uses the micro-operation schema of MEP-ELEC-016 with the `conduit.` prefix.

**Acceptance criteria.**
- [ ] Concentric parallel conduits around a 90° bend have bend radii that increase by the offset for each conduit.
- [ ] Concentric mode is refused on a conduit type with fittings.
- [ ] A junction box placed along a 25 mm conduit splits it, and the box exports as `IfcJunctionBox` connected to both conduits.
- [ ] A 32 mm conduit cannot be drawn with a type whose standard lists only 16, 20 and 25 mm.

### 4.9.17 Electrical analytical loads (preliminary design)
**ID:** MEP-ELEC-019 · **Priority:** P2 · **Ref:** research/03 §10.8

**Purpose.** Plan loads before physical devices are placed. This works over linked models or imported plans.

**Behaviour & rules.**
- **Area-based load boundaries:** sketched, or derived from rooms or spaces. They can be shown or hidden.
- **Area-based loads** (*Electrical Analysis › Area-Based Load › Add Load Area*):
  1. Pick enclosed regions.
  2. Assign an **area-based load type**: a load classification plus a **power density** in W/m² or W/ft².
  3. True load = density × area. Apparent load = true load ÷ power factor (× quantity).
  4. Loads can be divided.
- **Equipment loads:** load classification, voltage, power factor and apparent load.
- **Load sets** group loads.
- The *Electrical Analytical Load* schedule category summarises area and equipment loads, with grouping and totals.
- Like the reference product, the first release supports three-phase distribution only for analytical distribution elements.

**Data (IFC mapping).** Load areas are `IfcSpatialZone` (`USERDEFINED`, ObjectType "ElectricalLoadArea") with a footprint representation, and load values in RXD element data (`electrical`). Equipment loads are RXD element data on a placeholder `IfcSpatialZone` or on the space they sit in. See §4.17 Q5.

**Commands.**
- `electrical_load_area.create(boundary | space_ids[], load_type_id) -> id`
- `electrical_load_area.divide(id, line) -> ids[]`
- `equipment_load.create(point | space_id, classification, voltage, pf, apparent_load) -> id`
- `electrical_load_type.create(name, classification, density) -> id`
- `load_set.create(load_ids[]) -> id`

**Acceptance criteria.**
- [ ] A 250 m² area with a 12 W/m² lighting load type reports a true load of 3 000 W. With PF 0.9 it reports about 3 333 VA apparent.

### 4.9.18 Conceptual distribution system
**ID:** MEP-ELEC-020 · **Priority:** P2 · **Ref:** research/03 §10.8 ("Conceptual distribution system")

**Behaviour & rules.**
- In the System Browser's *Electrical Analytical Systems* view, build a tree of these nodes:
  - **power source** (utility or generator)
  - **bus** (panelboard or switchboard)
  - **transformer** (needs a secondary distribution system)
  - **transfer switch**
  - loads
- Children inherit the parent's distribution system, and incompatible children are refused.
- **Downstream connected phases** maps single-phase children onto three-phase parents (an edit dialog per node).
- Load totals and demand roll up through the tree.

**Data (IFC mapping).** Nodes are geometry-less occurrences: `IfcElectricDistributionBoard`, `IfcTransformer`, `IfcElectricGenerator` and `IfcSwitchingDevice` (`USERDEFINED` "TRANSFERSWITCH") with no Representation and RXD element data flag "analytical". The tree is expressed with `IfcDistributionCircuit` groups. This is an open choice (§4.17 Q5).

**Commands.**
- `electrical_analytical.add_node(kind, parent_id?, distribution_system_id?) -> id`
- `electrical_analytical.set_phases(node_id, mapping)`
- `electrical_analytical.rollup(root_id) -> report`

**Acceptance criteria.**
- [ ] Adding a 230 V single-phase bus under a 400/230 V three-phase bus asks which phase it connects to, and the parent's per-phase load reflects that choice.

### 4.9.19 Circuit renumbering
**ID:** MEP-ELEC-021 · **Priority:** P2 · **Ref:** research/03 §10.5, §10.6 ("Update Names", "Lock/Unlock") · web: Updating Circuit Names on a Panel

**Purpose.** Close gaps in a panel's circuit numbering after circuits are deleted, moved or disconnected, without re-entering circuits one by one.

**Workflow.**
1. Select a panel (or open its panel schedule) and choose *Renumber Circuits* (proposed `RI`) on the Electrical contextual tab or the Modify Panel Schedule tab.
2. A small dialog shows the start number (default 1) and a preview list: old number, new number, load name and phase, with changed rows highlighted by a text marker.
3. *OK* renumbers; *Cancel* leaves the panel unchanged.

**Behaviour & rules.**
- Circuits are moved to consecutive slots from the start number, keeping their current order. A multi-pole circuit keeps its pole count and occupies as many consecutive slots as before.
- **Locked** circuits (MEP-ELEC-015) and **spaces** stay in their slots; other circuits flow around them. **Spares** are treated like circuits.
- The slot count of the panel cannot be exceeded; if the start number would push a circuit past the last slot, the command is refused and names the first circuit that does not fit.
- On a three-phase panel, moving a circuit changes its phase. The dialog shows the phase loads before and after, and warns when the maximum phase imbalance grows by more than 10 percentage points. It does not rebalance; *Rebalance Loads* is separate.
- Circuit names are re-derived from the panel's circuit naming scheme (MEP-ELEC-012), as *Update Names* does. Load names typed by the user are not changed.
- Wires, tags and schedules that show circuit numbers update in the same transaction. Feeders are renumbered like branch circuits.
- A panel with no gaps and start number 1 is a no-op and adds no undo step.

**Data (IFC mapping).** The new name is written to `IfcDistributionCircuit.Name` (as in MEP-ELEC-012). Slot and start-slot values are **RXD element data** (`electrical`) on each circuit.

**Commands.** `circuit.renumber(panel_id, start?)` (renumbers the panel's circuit indexes and closes gaps).

**Acceptance criteria.**
- [ ] On a single-phase panel with circuits in slots 1, 2, 5 and 9, renumbering gives slots 1, 2, 3 and 4, and the scheme "{Panel}-{Index}" renames the last circuit "LP1-4".
- [ ] A circuit locked in slot 7 is still in slot 7 after renumbering, and no other circuit is placed in it.
- [ ] A 3-pole circuit keeps 3 consecutive slots after renumbering.
- [ ] Renumbering with start 40 on a 42-slot panel holding 5 circuits is refused and names the first circuit that does not fit.

---

## 4.10 MEP fabrication (open fabrication content)

**Design stance.** The reference product's fabrication tools are driven by a proprietary configuration database and proprietary part files. Reviera **does not read or write those formats**. Instead it defines the **Reviera Open Fabrication Content (ROFC)** model. ROFC is a documented, versioned content format that anyone can author, publish and exchange under a licence of their choice. Fabrication models are written to IFC with full geometry, so a model never depends on its content library to be viewed. All features in this section are **P2**.

Ref: research/03 §11 · screenshots/structure_mep/mep_fabrication_parts_model.png

### 4.10.1 ROFC content model
**ID:** MEP-FAB-001 · **Priority:** P2 · **Ref:** research/03 §11 ("Concept")

**Purpose.** Define open fabrication content that covers what LOD 400 duct, pipe and containment detailing needs.

**Behaviour & rules.** An ROFC **configuration** is a folder or ZIP package with:

| Component | Content |
|-----------|---------|
| `manifest` | Name, UUID, version, units, SPDX licence (content may use data licences such as CC-BY-4.0 or CC0), authors |
| **Materials and gauges** | Material, thickness or gauge tables by size, density for weight |
| **Specifications** | Gauge, insulation and connector rules by size range, for example "rectangular low-pressure galvanised" |
| **Connectors** | End-connection kinds (slip, flange, grooved, welded, threaded, press) with length allowances and gap rules |
| **Seams** | Longitudinal seam kinds and allowances |
| **Stiffeners and airturns** | Parametric definitions and placement rules |
| **Part templates** | Parametric patterns: straight, radius bend, segmented bend, offset, tee, wye, cross, transition, reducer, cap, tap, damper, valve body, hanger. Each has a dimension schema and a **sandboxed Lua generator** that produces geometry and connectors. |
| **Services** | Name, abbreviation, system classification mapping, default specification, and one or more **palettes** |
| **Palettes and buttons** | A button maps to one or more part templates with **size conditions** (ranges). Alternatives are shown on the button. |
| **Profiles** | Named subsets of services for a project or trade |
| **Fluids** | Fluid tables for pipework |
| **Reports** | Report and worksheet definitions (MEP-FAB-009) |

- A JSON Schema validates every file. Lua generators run in the restricted sandbox: no `io`, `os` or `ffi`.
- A project uses **one configuration at a time**, optionally filtered by a **profile**. The configuration is identified by UUID and version.
- Content packs can be local folders or HTTP URLs, for example a Git repository or a BIMserver-hosted package. Reviera caches them and verifies their checksums.
- In BIMserver mode, a configuration change is a recorded model change. Collaborators are told to get the latest version before editing fabrication parts.

**Data (IFC mapping).** **RXD document** `fabrication` records the active configuration UUID, version, profile, loaded services and checksums. Part templates are kept as **RXD family recipes**.

**Commands.**
- `fabrication_config.load(path | url, profile?) -> config_id`
- `fabrication_config.reload()`
- `fabrication_config.validate(path) -> issues[]`

**Acceptance criteria.**
- [ ] A configuration with a malformed part template fails validation, and the message names the file and the JSON path.
- [ ] A model with fabrication parts opens and displays correctly when its configuration is unavailable. Editing those parts is disabled, with an explanation.

### 4.10.2 Fabrication settings
**ID:** MEP-FAB-002 · **Priority:** P2 · **Ref:** research/03 §11 ("Fabrication Settings") · screenshots/structure_mep/fabrication_settings_dialog.png

**Purpose.** Choose the project's ROFC configuration and profile, and decide which services and extra parts are loaded, so that the Fabrication Parts palette offers only what the trade needs.

**UI.** *Systems › Fabrication* launcher, the Settings button on the palette, or *Manage › MEP Settings › Fabrication Settings* (`mep_settings.show(kind: "fabrication")`). It contains:
- configuration and profile pickers, with local and remote sources shown by a text label (not vendor icons); each entry shows name, version, licence and source path or URL
- the **services** list: two columns, *Unloaded* and *Loaded*, with *Add*/*Remove* buttons, multi-select and a search box; each service shows its abbreviation and part count
- additional **parts** outside services (single part templates that can be placed without a service)
- *Reload configuration*, which re-reads the content from its source
- tabs for **connection indicators** (how open and mismatched ends are drawn: symbol, size, colour token) and **fluids** (fluid tables from the configuration, read-only, with the option to copy one into Mechanical Settings)

**Behaviour & rules.**
- Changing the **configuration** in a model that already has fabrication parts asks whether to keep the parts; kept parts whose templates are missing in the new configuration become "out of step" and can be viewed but not edited (MEP-FAB-001).
- Changing the **profile** filters the services shown; services that parts use stay loaded even if the new profile omits them, with a notice.
- Unloading a service that parts in the model use is refused; the message gives the part count and offers *Select parts*.
- *Reload configuration* compares checksums. Parts whose template changed are marked out of step and listed; *Update parts* regenerates them in one transaction.
- Remote sources that cannot be reached fall back to the local cache, with a warning stating the cache date.
- All changes apply on OK, in one undoable transaction.

**Data (IFC mapping).** **RXD document** `model/fabrication`: configuration UUID and version, profile, loaded service IDs, extra parts, indicator settings and source checksums.

**Commands.**
- `fabrication_settings.set_services(loaded_ids[])`
- `fabrication_settings.set_profile(profile_id)`
- `fabrication_settings.set_parts(part_template_ids[])`
- `fabrication_settings.set_indicators(params)`
- `fabrication_part.update_out_of_step(scope?) -> ids[]`

**Acceptance criteria.**
- [ ] Unloading a service that parts in the model use is refused, and the dialog lists how many parts use it.
- [ ] Switching to a profile without the "Plumbing" service keeps "Plumbing" loaded while 10 parts use it and shows a notice.
- [ ] After a template file changes in the source, *Reload configuration* lists the affected parts as out of step, and *Update parts* regenerates them while keeping their connections.
- [ ] With the remote source offline, the configuration loads from cache and a warning shows the cache date.

### 4.10.3 Fabrication Parts palette and part placement
**ID:** MEP-FAB-003 · **Priority:** P2 · **Ref:** research/03 §11 ("MEP Fabrication Parts palette", "Placement tools")

**UI.**
- **Palette** (proposed `PB`): service and palette drop-downs; part buttons, where an arrow lists alternatives and a tooltip shows the size-condition ranges; Settings; a **Multi-Point Routing** start button; and a **model consistency check** (parts out of step with content).
- Contextual tab "Modify | Place Fabrication Part":
  - **Edit Part**: dimensions; components (connectors, seams, airturns, stiffeners); options such as the segment count for segmented bends. Double-clicking a part also opens it. Only connectors that are not connected can be edited.
  - **Rotate Part**: Spacebar. Shift+Space rotates tees and taps about the branch. Ctrl+Space rotates in vertical views. Rectangular parts step by 90° and round parts by 45°.
  - **Toggle Connector** (Up arrow) and **Insert Part** (a tee, valve or damper inline)
  - Elevation lock and slope (pipework)

**Data (IFC mapping).**
- Parts are standard MEP entities: `IfcDuctSegment`, `IfcDuctFitting`, `IfcPipeSegment`, `IfcPipeFitting`, `IfcValve`, `IfcDamper`, `IfcCableCarrierSegment`, …
- Each part has full Body geometry, nested ports and a material.
- **RXD element data** (`fabrication`) holds: service, button, template, template parameters, specification, gauge, connectors, seams, item number, spool, alias and product code.
- Standard values are mirrored to psets where they exist: material, gauge as NominalThickness, insulation via `IfcCovering`, and weight in `Qto_*BaseQuantities.GrossWeight`.

**Commands.**
- `fabrication_part.place(service_id, button_id, point, size, elevation, rotation?) -> id`
- `fabrication_part.edit(id, dims?, components?, options?)`
- `fabrication_part.insert(host_part_id, button_id, point) -> id`

**Acceptance criteria.**
- [ ] Placing a straight from a "Rect LP Duct" button at 600 × 400 gives the gauge that the specification sets for that size, and exports the matching material and thickness.
- [ ] Setting a segmented bend to 5 segments regenerates it with 5 segments and keeps its connections.

### 4.10.4 Multi-Point Routing
**ID:** MEP-FAB-004 · **Priority:** P2 · **Ref:** research/03 §11 ("Multi-Point Routing")

**Purpose.** Lay out a fabrication run quickly by clicking its path, letting Reviera choose straights, bends and branch fittings from the active service.

**Workflow.**
1. Start *Multi-Point Routing* from the ribbon (*Systems › Fabrication*), from the palette's start button, or by right-clicking an open connector (*Route from here*).
2. Set service, size and elevation in Properties (the palette's service is the default). A route started from a connector takes that connector's service, size and elevation.
3. Click points in plan, section or 3D. Each click adds a straight; direction changes add a bend.
4. Change size, elevation or slope between clicks; Reviera inserts a transition or a vertical offset at the current point.
5. Click an open connector to finish onto it, or press Esc / *Finish*.

**Behaviour & rules.**
- Parts are chosen per role (straight, bend, tee, transition, offset) from the service's buttons whose **size conditions** contain the current size. The first matching button in palette order wins; alternatives can be set per role in a pop-up before routing.
- Bend angles snap to the angles the service's bend templates support (for example 15°, 30°, 45°, 60°, 90°); off-angle clicks are snapped and a tooltip shows the snapped angle.
- Straights longer than the template's **maximum length** are split into several straights with the configured connector between them.
- Clicking a point on an existing straight of the same service inserts a tee or tap there.
- When no button matches a role and size, the route stops at the last valid point and the message names the role and size.
- Pipework routes can be sloped with the slope value on the contextual tab; slope is kept across bends.
- **Micro-operations (§2.1):** each click and each property change during routing is journalled as `{op: "point"|"set", point?, key?, value?}`; replay rebuilds the unfinished route.

**Data (IFC mapping).** Each created part is an MEP entity with RXD element data (`fabrication`) as in MEP-FAB-003. Parts are connected with `IfcRelConnectsPorts`.

**Commands.** `fabrication_route.multi_point(service_id, points[], size, elevation, start_connector?) -> element_ids[]`

The interactive mode runs as a tool session (`tool.start`/`tool.finish`/`tool.cancel`, Chapter 1); the command above is its scripted equivalent.

**Acceptance criteria.**
- [ ] A 4-point route makes 3 straights and 2 bends from the active service, all connected.
- [ ] A 9 m straight leg with a 3 m maximum template length becomes three straights joined by the service connector.
- [ ] Clicking a 50° change of direction with a service that has only 45° and 90° bends snaps to 45°.
- [ ] A route started from a 400 mm open connector inherits 400 mm and that connector's elevation.

### 4.10.5 Filling gaps
**ID:** MEP-FAB-005 · **Priority:** P2 · **Ref:** research/03 §11 ("Filling gaps")

**Purpose.** Close gaps between fabrication runs automatically, using parts from the services involved.

**Workflow.**
- **Trim/Extend:** pick the end of one run, then the end or side of another.
- **Quick Connect:** drag a straight's end grip onto a connector.
- **Route and Fill:** pick two open connectors. A dialog lists the candidate solutions with a preview in the canvas; filter them by required or excluded fittings, step through them, and *Apply* one.

**Behaviour & rules.**
- **Trim/Extend** joins two runs using the first solution found: it extends or shortens straights and adds one bend or tee. If no solution exists in at most two parts, it is refused with "No simple solution; use Route and Fill".
- **Quick Connect** drags a straight to a connector, whatever its size, and inserts a transition. It needs the two connectors to be roughly aligned (within 5° and with an offset no larger than the transition template allows).
- **Route and Fill** works between two open connectors in a single plane (the plane both connector axes lie in, within 1 mm). Connectors not in one plane are refused with "Connectors are not coplanar". It offers several solutions (up to 20, ordered by part count, then length), which can be filtered by fitting, and shows connection arrows at both ends.
- Parts are taken from the service of the first connector; if the two connectors have different services, the user chooses which.
- **Exclude Parts from Auto Fill** keeps chosen buttons out of these tools; the exclusion list is per project.
- Applying a solution is one undoable transaction. The solution list is discarded after *Apply* or *Cancel*.
- **Micro-operations (§2.1):** connector picks and the chosen solution index are journalled as `{op: "pick"|"choose", connector_ref?, index?}`; replay recomputes the solutions and reselects the chosen one.

**Commands.**
- `fabrication_route.trim_extend(conn_a, conn_b) -> element_ids[]`
- `fabrication_route.quick_connect(part_id, target_connector) -> element_ids[]`
- `fabrication_route.route_and_fill(conn_a, conn_b) -> solutions[]`
- `fabrication_route.apply(solution_id) -> element_ids[]`
- `fabrication_autofill.exclude(button_ids[], excluded: bool)`

**Acceptance criteria.**
- [ ] *Route and Fill* between two offset open ends in one plane returns at least one solution, and applying it leaves no open connectors.
- [ ] *Route and Fill* between two connectors in different planes is refused with the "not coplanar" message.
- [ ] After a 45° bend button is excluded from auto fill, no offered solution contains that button.
- [ ] *Quick Connect* from a 300 mm straight to a 400 mm connector inserts one transition and connects both ends.

### 4.10.6 Design to Fabrication conversion
**ID:** MEP-FAB-006 · **Priority:** P2 · **Ref:** research/03 §11 ("Design to Fabrication")

**Purpose.** Turn a coordinated design-level model (generic ducts and pipes) into fabrication parts without redrawing it.

**Workflow.**
1. Select design elements (Tab-select runs, or use a filter) and choose *Modify › Fabrication › Design to Fabrication*.
2. The dialog shows the **mapping table** for the selection: one row per design type or fitting part type found, with the target service and button. Unmapped rows are highlighted with a text label "Not mapped".
3. Edit rows or choose *Use saved mapping*, then *Convert*.
4. Review the conversion report (converted, skipped, warnings) and use *Select skipped* to inspect elements left unconverted.

**Behaviour & rules.**
- Converts generic design elements into fabrication parts: ducts, duct fittings, duct accessories, flex duct, air terminals, pipes, pipe fittings, pipe accessories and flex pipe.
- A **mapping table** says which service and button replaces each design type or fitting part type. It is editable and stored in RXD document `model/fabrication`. Several named mappings can be saved; one is the project default.
- Mapping rows can be qualified by system classification and size range, so that one duct type maps to a low-pressure service below 600 mm and a medium-pressure service above.
- Conversion is refused when the selection contains analytical pipe connections; the message names them and offers *Remove from selection*.
- Reviera recommends running *Show Disconnects* first and converting in small batches. The converter reports which elements it could not map. Batches larger than 2 000 elements ask for confirmation.
- Geometry is kept: straights keep their centreline, size and elevation; fittings are replaced by the mapped button's part at the same connector positions. Where the fabrication part's length differs (for example a longer bend), adjacent straights are trimmed or extended. If that is impossible, the element is skipped and listed.
- Conversion keeps system membership and GlobalIds where the element kind does not change. Otherwise it records the old GlobalIds for traceability (in RXD element data `fabrication`, key `converted_from`).
- Tags and dimensions on converted elements are re-hosted to the new parts where the element kept its GlobalId; others are listed in the report.
- The whole conversion is one undoable transaction and runs in the background with progress (ANA-GEN-001 job model).

**Commands.**
- `fabrication.convert_design(element_ids[], mapping_id?) -> report`
- `fabrication_mapping.set(rules[])`
- `fabrication_mapping.save(name, rules[], default?: bool) -> mapping_id`
- `fabrication_mapping.preview(element_ids[], mapping_id?) -> rows[]`

**Acceptance criteria.**
- [ ] Converting a 20-element duct run with a complete mapping yields 20 fabrication parts in the same system, and the run stays connected.
- [ ] A selection with one unmapped fitting type converts everything else, and the report lists the unmapped fitting with *Select skipped*.
- [ ] A selection containing an analytical pipe connection is refused before any change is made.
- [ ] A 500 mm round duct converted with size-qualified rows uses the service of the row whose size range contains 500 mm.

### 4.10.7 Fabrication editing tools
**ID:** MEP-FAB-007 · **Priority:** P2 · **Ref:** research/03 §11 ("Editing")

**Purpose.** Edit placed fabrication parts in bulk while keeping runs connected and consistent with their service.

**UI.** Contextual tab "Modify | Fabrication Parts": Change Service, Change Size, Show Service, Edit Part (MEP-FAB-003), Spool (Create, Add, Remove), Slope, and the in-canvas length grip on straights.

**Behaviour & rules.**
- **Change Service** re-maps the selected parts to the equivalent buttons (same role and size condition) in another service. Parts with no equivalent are left unchanged and listed. Connections are kept; mismatched end connectors get a connection indicator.
- **Change Size**, with an option to **resize connected** parts: without it, Reviera inserts transitions at the selection boundary; with it, the size change propagates through connected fittings and straights until a transition, a branch or equipment is met. Branch take-offs keep their branch size.
- **Modify length** of a straight by grip or by typing; the length is limited by the template's minimum and maximum, and connected parts move with the free end only if they are not connected elsewhere, otherwise the edit is refused.
- **Show Service** highlights all parts of the selected part's service in the view until the next selection change.
- **Rise and drop symbols** for fabrication parts follow the system type's rise/drop symbol settings (MEP-SYS-001) and appear in plan for vertical parts, using the display rules of MEP-DUCT-011.
- **Dampers** placed as inline parts can be opened or closed (a template option that changes the displayed blade).
- **Sloped pipework**: the slope editor (MEP-PIPE-002) works on fabrication pipe; bends regenerate to the sloped angle where the template allows it, otherwise the edit is refused naming the part.
- **Spool assignment** groups parts into prefabrication spools. A part belongs to at most one spool; spool names are unique per project and default to "<service abbreviation>-<n>". Spools must be connected sets; adding a disconnected part warns but is allowed.

**Data (IFC mapping).** A spool is an `IfcElementAssembly` (`USERDEFINED` "SPOOL") aggregating its parts. An `IfcPipeSegment` with PredefinedType `SPOOL` is accepted on import. Service, button and spool name are RXD element data (`fabrication`).

**Commands.**
- `fabrication_part.change_service(ids[], service_id)`
- `fabrication_part.change_size(ids[], size, resize_connected?)`
- `fabrication_part.set_length(id, length)`
- `fabrication_part.show_service(view_id, part_id)`
- `fabrication_spool.create(part_ids[], name) -> id`
- `fabrication_spool.add(spool_id, part_ids[])`
- `fabrication_spool.remove(spool_id, part_ids[])`

**Acceptance criteria.**
- [ ] Changing a main from 400 to 500 mm with *resize connected* updates the adjacent fittings, and the branch takeoffs stay connected.
- [ ] Changing a 400 mm straight to 500 mm without *resize connected* inserts transitions at both ends.
- [ ] Change Service on 30 parts where 2 have no equivalent button converts 28 and lists the 2.
- [ ] Adding a part that is already in spool "PL-1" to spool "PL-2" removes it from "PL-1".

### 4.10.8 Hangers and supports
**ID:** MEP-FAB-008 · **Priority:** P2 · **Ref:** research/03 §11 ("Hangers")

**Purpose.** Model the supports of fabrication runs so that rod lengths, quantities and anchor positions can be scheduled and set out on site.

**Workflow.**
1. Choose a hanger button from the palette's hanger group (or *Systems › Fabrication › Hanger*).
2. Click a straight, a fitting extension or a sloped part to place one hanger; or select a run and choose *Place Along Run* with a spacing.
3. Adjust rod length, rod diameter and *attach to structure* in Properties. Press Spacebar before placing to flip the hanger.

**Behaviour & rules.**
- Hangers are placed on straights, on fitting extensions or on sloped parts. A hanger hosted on a part moves with it and is deleted with it (after a warning listing the hangers).
- The hanger size follows the host part's outside size, including insulation when the template says so.
- Parameters: rod length, rod diameter and *attach to structure*, which extends rods to the structure above. *Bearer to bearer* spacing places hangers along a run.
- **Attach to structure** searches upwards (within 5 m by default) for the nearest face of structural floors, beams, roofs or structural framing, in the host model and in room-bounding links. If nothing is found, the rod keeps its typed length and a warning is issued. Rods re-extend when the structure moves.
- **Place along run** spaces hangers at the given interval from the run start, keeps a minimum distance (template value, default 150 mm) from fittings and joints, and adds one hanger within the allowed end distance of each end.
- Spacebar flips a hanger (swaps its rod side for trapeze or offset templates).
- Hanger templates are ROFC part templates; they report the anchor point, rod lengths and ancillaries (nuts, washers) for reports (MEP-FAB-009).

**Data (IFC mapping).** `IfcDiscreteAccessory` (`USERDEFINED` "HANGER") with `IfcRelConnectsElements` to the supported part and to the structural element. On IFC4X3 export, the specialised support entity is used where the schema offers one (to verify). Rod lengths and template parameters are RXD element data (`fabrication`); the anchor point is exported as a named representation point.

**Commands.**
- `hanger.place(part_id, position, template_id, attach_to_structure?) -> id`
- `hanger.place_along(run_ids[], spacing, template_id) -> ids[]`
- `hanger.set(ids[], {rod_length?, rod_diameter?, attach_to_structure?, flipped?})`

**Acceptance criteria.**
- [ ] With *attach to structure* on, a hanger under a slab gets a rod length equal to the distance from the hanger to the slab soffit.
- [ ] Raising the slab by 200 mm lengthens the attached rods by 200 mm.
- [ ] *Place Along Run* at 2 m on a 10 m straight with no fittings places 6 hangers including both ends, none closer than 150 mm to a joint.
- [ ] A hanger with nothing structural within 5 m above keeps its typed rod length and raises a warning.

### 4.10.9 Fabrication outputs
**ID:** MEP-FAB-009 · **Priority:** P2 · **Ref:** research/03 §11 ("Outputs")

**Purpose.** Hand fabrication data to the shop: part lists, ancillaries, cut lists and a machine-readable job, all in open formats.

**Workflow.** Select parts or spools (or choose a scope: view, level, service, spool list), then *Systems › Fabrication › Export Job*, *Fabrication Reports* or *Export Data*. Choose the format, path and report definition, then *Export*.

**Behaviour & rules.**
- **Open job export:** fabrication parts, spools and hangers are exported as:
  - an IFC subset (IFC4, with spools as assemblies), and
  - an ROFC **job file**: JSON with parts, template parameters, cut lengths, flat-pattern data where the template provides it, and item numbers. The file names the configuration UUID and version, and is validated against the published ROFC job schema before it is written.

  Annotation is not included.
- Exports refuse to run while parts in the scope are out of step with their content (MEP-FAB-002); the message offers *Update parts* or *Export anyway* (the latter writes a warning into the job file).
- Parts without an item number get one assigned before export (MEP-FAB-010), with a notice.
- **Fabrication reports** use the schedule engine with report definitions taken from the configuration:
  - item reports (one row per item number, with quantity, size, length, weight)
  - ancillary reports (fasteners, sealant, gaskets, hanger rods and fixings) aggregated from the templates' ancillary lists
  - **linear nesting** of straights into stock lengths
  A report can be placed as a schedule view on a sheet or written to a file.
- **Linear nesting** uses first-fit decreasing by default, with a saw kerf (default 3 mm) and a minimum reusable offcut (default 300 mm); it reports bars used, parts per bar, offcuts and waste percentage. Parts longer than the stock length are listed as "cannot nest".
- **Data export** as CSV (UTF-8, comma-separated, header row) or JSON, for items, ancillaries or nesting results.
- Proprietary job formats are not supported (§4.17).

**Data (IFC mapping).** The IFC subset uses the entities of MEP-FAB-003, MEP-FAB-007 and MEP-FAB-008. Report definitions come from the configuration; the last-used export settings are stored in RXD document `model/fabrication`.

**Commands.**
- `fabrication.export_job(scope, path, format: "ifc"|"rofc-json")`
- `fabrication.report(scope, report_id) -> view_id | file`
- `fabrication.nest_linear(scope, stock_length) -> report`
- `fabrication.export_data(scope, kind: "items"|"ancillaries"|"nesting", path, format: "csv"|"json")`

**Acceptance criteria.**
- [ ] Linear nesting of twelve 2.3 m straights into 6 m stock produces 6 bars with 2 parts each and reports the offcut.
- [ ] A job file exported for 2 spools validates against the ROFC job schema and lists every part with its item number and cut length.
- [ ] A 7 m straight nested into 6 m stock is listed as "cannot nest".
- [ ] The ancillary report for 10 hangers with two nuts each lists 20 nuts.

### 4.10.10 Fabrication parameters
**ID:** MEP-FAB-010 · **Priority:** P2 · **Ref:** research/03 §11 ("Parameters")

**Purpose.** Make fabrication data available to schedules, tags, filters and scripts in the same way as any other parameter.

**Behaviour & rules.** Every fabrication part exposes these read-only or editable parameters:
- service name and abbreviation (read-only; changed with *Change Service*)
- item number (editable; unique per project unless the parts are identical)
- spool (read-only; changed with the spool tools)
- material and gauge (read-only; from the specification)
- insulation type and thickness (editable within the specification's allowed values)
- weight, material weight and volume (read-only; computed from geometry and the material density in the configuration)
- alias (editable free text)
- product code and name (read-only; from the template)
- part pattern number (read-only)
- handle position (editable where the template has a handle)

They are schedulable and filterable, can be used in view filters and tags, and are readable and writable from Lua and MCP through the generic parameter commands of Chapter 2 (`param.get`, `param.set`). Editing a read-only parameter is refused with "Set by fabrication content".

- **Item numbering:** *Number Items* assigns item numbers to the parts in a scope, in run order from a start part, with a prefix and start value. Identical parts (same template, parameters and size) can share a number when *Group identical* is on.
- Weights update in the background when parts change; schedules show a stale marker until done.

**Data (IFC mapping).** Values are RXD element data (`fabrication`). Weight is mirrored to `Qto_*BaseQuantities.GrossWeight`, material and gauge as described in MEP-FAB-003, alias and item number to `IfcElement.Tag` (item number) and `Description` (alias).

**Commands.**
- `fabrication_part.number_items(scope, start_part_id?, prefix?, start?, group_identical?: bool) -> count`

**Acceptance criteria.**
- [ ] A schedule of fabrication parts grouped by service shows total weight per service, and the totals match the sum of the part weights.
- [ ] *Number Items* with prefix "D-" and start 100 on a 5-part run numbers the parts D-100 to D-104 in run order.
- [ ] With *Group identical* on, two identical straights get the same item number.
- [ ] Setting the weight parameter through Lua is refused with "Set by fabrication content".

---

## 4.11 Spaces and system-zones

### 4.11.1 Spaces
**ID:** MEP-SPACE-001 · **Priority:** P1 · **Ref:** research/03 §12.1

**Purpose.** MEP volume elements, independent of architectural rooms. They hold load, airflow and lighting data for analysis and exchange. Reviera recommends placing them everywhere, including plenums, shafts, chases and thin "sliver" spaces.

**Workflow.**
1. Choose *Analyze › Spaces & Zones › Space*.
2. Click inside bounded regions, or use *Place Spaces Automatically*. Automatic placement creates a space in every enclosed region larger than 0.025 m² and numbers them in sequence.
3. *Highlight Boundaries* shows the bounding elements.
4. Set the upper limit and offset on the contextual tab.

**Behaviour & rules.**
- **Space extents:**
  - up to the level above
  - up to the roof
  - up to the ceiling
  - plenum spaces
  - complex vertical extents
- A space is marked *Occupiable* when a room exists at its location.
- **Linked models** bound spaces only when the link is set to *Room Bounding* (chapter 6).
- **Space Naming** copies names and numbers from the rooms in a linked or host model.
- **Editing:**
  - space separator lines
  - combine, divide and move
  - **unplace**: the space stays in schedules as "Not Placed" until it is deleted
  - limits set through properties, drag controls or dimensions
- Spaces are visible in plans and sections, not in elevations or 3D. Space volumes are shown in 3D only through the energy model.

**Data (IFC mapping).**
- `IfcSpace` (PredefinedType `SPACE`, or `INTERNAL`/`EXTERNAL` where known), with ObjectType "MEPSpace" to tell spaces apart from rooms. When a room and a space are in the same place, both are exported as separate `IfcSpace` objects.
- Quantities: `Qto_SpaceBaseQuantities` (NetFloorArea, NetVolume, Height).
- Property sets: `Pset_SpaceCommon` (IsExternal, ReferencedName), `Pset_SpaceThermalRequirements`, `Pset_SpaceThermalLoad`, `Pset_SpaceOccupancyRequirements`, `Pset_SpaceLightingRequirements` and `Pset_SpaceThermalDesign` (supply, return and exhaust airflow).
- Space boundaries: `IfcRelSpaceBoundary` (level 1), and level 2 (`IfcRelSpaceBoundary2ndLevel`) when the energy model is created.
- Containment in the storey.

**Commands.**
- `space.place(view_id, point, upper_limit?, offset?) -> id`
- `space.place_auto(level_id) -> ids[]`
- `space_separator.create(view_id, curves[]) -> ids[]`
- `space.unplace(ids[])`
- `space.name_from_rooms(scope, source_link_id?)`

**Acceptance criteria.**
- [ ] *Place Spaces Automatically* on a level with 25 enclosed regions creates 25 spaces, and marks occupiable the 20 that are under rooms.
- [ ] Space Naming copies "101 Office" from a linked room to the space at the same location.
- [ ] Each space exports as an `IfcSpace` with `Qto_SpaceBaseQuantities.NetVolume` correct to within 0.5%.

### 4.11.2 Space properties and building/space type settings
**ID:** MEP-SPACE-002 · **Priority:** P1 (properties) / P2 (type settings) · **Ref:** research/03 §12.1 ("Space properties")

**Behaviour & rules.**
- **Space properties:**
  - name and number
  - area and volume (the volume calculation setting is in chapter 5)
  - upper limit and offset
  - occupiable and condition type
  - **space type** and building type
  - people: area per person, sensible and latent heat gain per person
  - lighting and power loads: density, or actual values from the fixtures and devices in the space
  - airflow: specified, calculated and actual supply, return and exhaust
  - return-air plenum flag
  - ceiling, wall and floor reflectances
  - lighting calculation workplane height
  - **average estimated illumination** (ANA-LIGHT-001)
  - phase
- ***Building/Space Type Settings*** (Manage › MEP Settings) define default densities, occupancy and operating schedules per type. Reviera ships open starter values. Schedules use the ANA-EM-002 schedule format.
- **Colour schemes** on spaces show airflow, load or type. Space tags are supplied.

**Data (IFC mapping).** The space psets listed in MEP-SPACE-001. Type definitions and schedules: **RXD document** `analysis`.

**Commands.**
- `space.set(ids[], params)`
- `space_type.create(name, params) -> id`
- `building_type.create(name, params) -> id`

**Acceptance criteria.**
- [ ] Assigning the type "Office" sets lighting density, occupancy and schedules to the type defaults, and each value can be overridden per space.

### 4.11.3 System-zones
**ID:** MEP-ZONE-001 · **Priority:** P2 · **Ref:** research/03 §12.2 · screenshots/structure_mep/system_zones_plan.png

**Purpose.** Graphic regions, sketched per level, that mark the areas served by analytical air systems, water loops and zone equipment. They also cover what HVAC zones used to do.

**Workflow.**
- *System-Zone › By Sketch* or *By Level*.
- *Add/Edit System-Zone*.
- *Convert*: turns legacy zones from imported models into system-zones.

**Behaviour & rules.**
- When the energy model is created (ANA-EM-003), every analytical space inside or touching a system-zone is assigned to it.
- Each system-zone is assigned to a piece of zone equipment or an air system.
- The relationships appear in the System Browser's *Zones* view.
- System-zones and analytical spaces must be in the host model, not a link, and in the project phase being analysed.
- Heating and cooling set points, humidity limits and outdoor-air rules are system-zone properties. They are kept so that legacy zone workflows can be converted.

**Data (IFC mapping).**
- The system-zone region is an `IfcSpatialZone` (`THERMAL`) with a footprint and body representation.
- The grouping of spaces is an `IfcZone` (ObjectType "SystemZone"), with spaces linked by `IfcRelAssignsToGroup`.
- Zone equipment is related with `IfcRelServicesBuildings`, or `IfcRelAssignsToGroup` of the equipment into the zone, plus `Pset_ZoneCommon`.
- Set points are in `Pset_SpaceThermalRequirements` on each assigned space.
- The equipment assignment is also kept as RXD element data (`analysis`).

**Commands.**
- `system_zone.create(level_id, boundary | "whole_level") -> id`
- `system_zone.assign(zone_id, equipment_or_system_id)`
- `system_zone.convert_legacy(zone_ids[]) -> ids[]`

**Acceptance criteria.**
- [ ] After energy model creation, a system-zone that covers 6 analytical spaces lists those 6 in the System Browser under its zone equipment.
- [ ] An imported IFC `IfcZone` with set points converts into a system-zone whose region envelopes its spaces.

---

## 4.12 Analysis

The analysis services in this section are **P2** unless stated otherwise. They all run through ANA-GEN-001 (background jobs, stale-result flags) and the provider model (§4.0.2). Their results are drawn in views with the shared analysis display styles (ANA-ADS-001).

### 4.12.1 Location and weather
**ID:** ANA-EM-001 · **Priority:** P2 · **Ref:** research/03 §13.1 ("Location")

**Purpose.** Give every energy, loads, daylight and solar calculation one site definition and one climate source, without any vendor weather service.

**Workflow.**
1. Choose *Analyze › Energy › Location*. The dialog has two tabs, *Location* and *Weather*.
2. *Location*: type an address (resolved only if a geocoding add-in is installed), pick a city from a bundled list built from an openly licensed gazetteer, or enter latitude, longitude, elevation and time zone directly. This is the same project location as chapter 2.
3. *Weather*: *Import EPW…* from a file or a configured URL, or pick a previously imported file. The tab lists the station name, WMO number, distance from the site, period of record and the derived design conditions.
4. Optionally edit the **design conditions** table: heating dry-bulb (99.6%), cooling dry-bulb and mean coincident wet-bulb (0.4%), daily range, and clearness. Edited values are marked as user overrides.

**Behaviour & rules.**
- *Energy › Location* sets the site location (shared with chapter 2's project location) and a **weather file**.
- Supported weather sources are EPW files (an open, widely published format), local or from user-configured URLs.
- Design-day data comes from the weather file or from a user-entered design-conditions table.
- No proprietary weather service is required.
- An EPW file must hold 8 760 hourly records (8 784 in a leap year). A shorter or malformed file is refused, and the message names the first bad line.
- A station more than 50 km away or with an elevation difference above 300 m from the site raises a warning, not a refusal.
- When the weather file changes, energy, loads, daylight and solar results become **stale** (ANA-GEN-001).
- The imported file is copied into the project's resource folder (or BIMserver extended data in client mode), so the project stays portable.

**Data (IFC mapping).** `IfcSite` RefLatitude, RefLongitude and RefElevation. The weather file reference is an `IfcDocumentReference`. Design days are **RXD document** `analysis`.

**Commands.**
- `energy_location.set(lat, lon, elevation?, weather_file?)`
- `weather.import(path) -> id`

**Acceptance criteria.**
- [ ] Loading an EPW file fills the location and design temperatures, and they are shown in Energy Settings.
- [ ] An EPW file truncated at 8 000 records is refused, and the message names the line where it ends.
- [ ] Changing the weather file after a heating and cooling loads run marks that run stale.
- [ ] A user override of the heating design temperature survives re-import of the same EPW file and is marked as an override.

### 4.12.2 Energy settings
**ID:** ANA-EM-002 · **Priority:** P2 · **Ref:** research/03 §13.1 ("Energy Settings", 2027 Energy Model Guide), §12.1

**UI.** *Energy › Energy Settings*, laid out as a step-by-step "Energy Model Guide" panel and an advanced dialog:
- **Mode**: conceptual masses and building elements (default), building elements only, masses only, or rooms/spaces.
- **Ground plane**, **project phase**.
- **Analytical space resolution**: default 457 mm, range 150 mm to 3 m. Gaps smaller than the value are bridged. Gaps larger than twice the value are never bridged.
- **Analytical surface resolution**.
- Perimeter zone depth and division, and core offset.
- **Advanced**:
  - building type and operating schedule
  - HVAC system
  - outdoor air
  - export category (rooms or spaces)
  - conceptual constructions
  - target glazing percentage, sill height, shading, skylights
  - material thermal properties: schematic types, or detailed element layers
- **Schedules** page: the operating, occupancy, lighting, equipment and HVAC availability schedules used by building and space types (MEP-SPACE-002). Each schedule is a named set of 24-hour profiles (fractions 0–1, or set-point temperatures) for weekday, Saturday, Sunday/holiday and design days, editable in a grid with a line-chart preview. Reviera ships open starter schedules.

**Workflow.**
1. Choose *Analyze › Energy › Energy Settings* (`energy_settings.show`). The **Energy Model Guide** panel opens docked on the right with four steps: *Location* (ANA-EM-001), *Model mode and resolution*, *Constructions and glazing*, *Create or update the energy model* (ANA-EM-003). Each step shows a status (not started, complete, needs attention) and a button that opens the matching page.
2. *Advanced…* opens the full modal dialog with the pages above.
3. *OK* commits all changes in one transaction ("Energy Settings"). If an energy model exists, the dialog says which changes will make it stale and offers *Update energy model now*.

**Behaviour & rules.**
- Validation: analytical space resolution 150 mm–3 m; surface resolution 50 mm–1 m and not larger than the space resolution; perimeter zone depth 0–15 m; glazing percentage 0–95%; sill height 0–3 m. Out-of-range values are refused on the field with the allowed range shown.
- The project phase must exist; the ground plane must be a level. Changing either marks the energy model stale.
- *Rooms or spaces* mode needs at least one placed room or space of the chosen export category; the dialog warns if there is none.
- *Conceptual constructions* pick from an open library of schematic constructions with U-values; *detailed element layers* use the material thermal assets (DM-MAT-004).
- The Energy Model Guide is non-modal and updates its step status live; closing it does not discard settings.
- Settings are per project and transferable with *Transfer Project Standards*.

**Data (IFC mapping).** **RXD document** `analysis` (energy settings and schedules). Thermal properties of materials use `Pset_MaterialThermal` (SpecificHeatCapacity, ThermalConductivity) and `Pset_MaterialCommon` (MassDensity).

**Commands.**
- `energy_settings.show()` (opens the Energy Model Guide panel; the *Advanced* dialog is reached from it)
- `energy_settings.set(path, value)`
- `energy_settings.get() -> table`

**Acceptance criteria.**
- [ ] A 300 mm gap between two walls is bridged at the default resolution. A 1 000 mm gap is never bridged.
- [ ] Entering an analytical space resolution of 100 mm is refused with the range 150 mm–3 m shown.
- [ ] Changing the project phase after the energy model was created marks it stale, and the guide shows step 4 as "needs attention".
- [ ] Editing a weekday occupancy profile in the Schedules page changes the occupancy schedule used by every space of the type that refers to it.

### 4.12.3 Energy model
**ID:** ANA-EM-003 · **Priority:** P2 · **Ref:** research/03 §13.1 ("Energy (analytical) model") · screenshots/structure_mep/energy_model_analytical_spaces_3d.png

**Purpose.** Build the analytical energy model: **analytical spaces** (air volumes), **analytical surfaces** (heat-transfer surfaces) and zones.

**Behaviour & rules.**
- **Create Energy Model** builds the model from masses, building elements, or both, using the settings above. It also creates the views *System-Zone* and *Analytical Spaces*.
- **Analytical surfaces** are classified as exterior wall, roof, slab on grade, underground wall or slab, interior wall, interior floor, window, skylight, door, shading or air.
- An existing energy model must be **deleted** before it is re-created (*Delete Energy Model*). Reviera also offers *Update*, which re-creates the model in one transaction and keeps the user's manual reassignments where the analytical spaces still match.
- Building the model runs in parallel per level and per space, using OCCT boolean and offset operations.

**Data (IFC mapping).**
- Analytical spaces are `IfcSpace` (ObjectType "AnalyticalSpace") in an `IfcGroup` "EnergyModel".
- Analytical surfaces are **second-level space boundaries**: `IfcRelSpaceBoundary2ndLevel` with `PhysicalOrVirtualBoundary` and `InternalOrExternalBoundary`, ConnectionGeometry, and `CorrespondingBoundary` pairs.
- Shading surfaces are `IfcShadingDevice` (or `IfcVirtualElement` for air boundaries).
- Reviera thus exports a building-energy-model-ready IFC.

**Commands.**
- `energy_model.create(options?) -> job_id`
- `energy_model.delete()`
- `energy_model.update() -> job_id`
- `energy_model.reassign_space(analytical_space_id, system_zone_id)`

**Acceptance criteria.**
- [ ] For a two-storey box building with 8 rooms, the energy model has 8 analytical spaces, and every interior wall boundary has a corresponding boundary on the neighbouring space.
- [ ] The exported IFC passes the buildingSMART space-boundary validation rules for second-level boundaries.

### 4.12.4 gbXML exchange
**ID:** ANA-EM-004 · **Priority:** P2 · **Ref:** research/03 §13.1 ("gbXML")

**Behaviour & rules.**
- *File › Export › gbXML* writes the energy model, or rooms/spaces in simple mode. gbXML is an open schema.
- *Insert › Import › gbXML* reads load results computed by third-party tools and writes calculated values onto matching spaces (matched by ID, then by name and number).
- **Export dialog.** Source (*Energy model* or *Rooms/spaces*), project phase, a 3D preview with a list of spaces and surfaces, and warnings (unbounded spaces, surfaces with no construction, overlapping spaces). *Next* writes the file. Export with the energy model as source requires an up-to-date energy model; if it is stale, the dialog offers to update it first.
- **Content.** Campus, building, storeys, spaces (with area, volume, people, lighting and equipment loads from space types), surfaces with their adjacent spaces and openings, constructions and layers with material thermal values, the weather location, and schedules. IDs are derived from the IFC GlobalIds of the source spaces (prefixed with `sp-` and with `$` replaced by `-`, so they are valid XML IDs), so a round trip can match them.
- Export writes the gbXML schema version current at release (recorded in the file); units are SI unless the project uses imperial units.
- **Import.** Only result values are read: design heating and cooling loads, airflows and peak times. Geometry in the file is ignored. Spaces that cannot be matched are listed in the report; no spaces are created.
- Imported values are written as calculated results (ANA-GEN-002), tagged with the file name and date, and become stale when the spaces change.

**Data (IFC mapping).** Imported loads go to `Pset_SpaceThermalLoad` and `Pset_SpaceThermalDesign` on the matched `IfcSpace`. The import source is an `IfcDocumentReference` indexed in **RXD document** `analysis`.

**Commands.**
- `gbxml.export(path, source: "energy_model"|"spaces")`
- `gbxml.import_loads(path) -> report`

**Acceptance criteria.**
- [ ] An exported file validates against the current gbXML schema.
- [ ] Re-importing a file in which the loads were edited updates the calculated heating and cooling loads on the matching spaces.
- [ ] A file with one space ID that matches nothing in the model imports the other spaces and lists the unmatched ID in the report.
- [ ] Every exported `Space` element's `id` maps back to the GlobalId of its source space.

### 4.12.5 Analytical HVAC systems
**ID:** ANA-SYS-001 · **Priority:** P2 · **Ref:** research/03 §13.2 ("Add analytical systems")

**Behaviour & rules.** Conceptual HVAC systems are defined in the System Browser's *Analytical Systems* view, with no physical modelling:
- **Water loops:**
  - hot water
  - chilled water (air-cooled or water-cooled chiller, with a condenser loop)
  - condenser water
  - VRF
- **Air systems:** heat-recovery exchanger, preheat coil, cooling coil, heating coil and fan, plus a **DOAS** flag.
- **Zone equipment:**
  - VAV and CAV boxes
  - series and parallel fan-powered boxes
  - four-pipe fan coil
  - PTAC and PTHP
  - WSHP
  - VRF indoor unit
  - active and passive chilled beams
  - radiant panel
  - unit heater and unit ventilator

  **Behaviour** is *One per Space* or *Group Spaces on Equipment*.
- Zone equipment is linked to system-zones (MEP-ZONE-001) and to water loops and air systems.

**Data (IFC mapping).**
- Loops and air systems are `IfcDistributionSystem` (`HEATING`, `CHILLEDWATER`, `CONDENSERWATER`, `REFRIGERATION`, `VENTILATION`, `AIRCONDITIONING`) with ObjectType "Analytical".
- Zone equipment is a geometry-less `IfcUnitaryEquipment`, `IfcAirTerminalBox`, `IfcSpaceHeater` or similar occurrence, with an RXD element data (`analysis`) flag "analytical".
- Component settings (coil types, efficiencies, DOAS) are RXD element data (`analysis`).

**Commands.**
- `analytical_water_loop.create(kind, params) -> id`
- `analytical_air_system.create(params) -> id`
- `zone_equipment.create(kind, behavior, loop_ids[], air_system_id?) -> id`

**Acceptance criteria.**
- [ ] A chilled-water loop with a water-cooled chiller automatically creates its condenser loop, and both appear in the System Browser.

### 4.12.6 Systems analysis (energy simulation)
**ID:** ANA-SIM-001 · **Priority:** P2 · **Ref:** research/03 §13.2 · screenshots/structure_mep/systems_analysis_loads_report.png

**Workflow.**
1. Create the energy model and the analytical systems, then assign the system-zones.
2. Choose *Energy › Systems Analysis* and pick a **workflow**: *HVAC loads and sizing*, or *Annual building energy simulation*.
3. Give a report name and folder, then *Run*.

The job runs in the background with a notification. Spaces that no system serves get an **ideal air-load system**.

**Behaviour & rules.**
- **[Suggestion] Default provider: EnergyPlus**, driven through **OpenStudio** (both BSD-style, run as external processes).
- Reviera translates the energy model, analytical systems, schedules and weather file to an OpenStudio model or EnergyPlus input. It runs the simulation and parses the outputs into a result package.
- **Workflows are scripts**, Lua in Reviera, kept in a user-configurable folder. Users can add measures.
- **Loads report** (HTML/PDF), with unit switching (SI, IP, project units):
  - **zone load summary**: peak cooling and heating per zone, with components (sensible instant and delayed, latent, total)
  - **system load summary**
  - **design psychrometrics**
  - plan and 3D panes and a system schematic
- Annual simulation adds a detailed report: energy end uses, unmet hours, monthly profiles.

**Data (IFC mapping).** Result package referenced through `IfcDocumentReference`. The package index is in **RXD document** `analysis`. Peak loads are written back to spaces (`Pset_SpaceThermalLoad`, `Pset_SpaceThermalDesign`) and to zone equipment.

**Commands.**
- `systems_analysis.run(workflow_id, report_name, folder, provider_id?) -> job_id`
- `systems_analysis.report(job_id) -> path`

**Acceptance criteria.**
- [ ] With the EnergyPlus provider installed, running "HVAC loads and sizing" on a sample office model produces a report with zone peak cooling loads, and those loads are written to the spaces.
- [ ] Editing the model after the run marks the results stale.

### 4.12.7 Heating and cooling loads
**ID:** ANA-HCL-001 · **Priority:** P2 · **Ref:** research/03 §13.3

**Purpose.** A focused design-day loads calculation on the energy model, analytical spaces and system-zones. It is lighter than a full simulation.

**UI.** *Analyze › Reports & Schedules › Heating & Cooling Loads*. The dialog asks whether to update the energy model or reuse it. Options:
- **building service**
- **infiltration class**: Loose, Medium, Tight or None. Defaults are 0.39, 0.19 and 0.10 L/s per m² of exterior surface, user-editable.
- **report type**: Simple, Standard, Detailed or Compact
- **use load credits** (negative loads)
- *Run Analysis*

**Behaviour & rules.**
- **Method:** a **radiant time series (RTS)** calculation provider. It covers:
  - 24-hour design-day profiles
  - conduction through opaque elements with sol-air temperature
  - fenestration solar and conduction gains
  - the radiant/convective split and radiant time series
  - people, lighting and equipment gains
  - infiltration
  - plenum loads
  - psychrometrics, set points and humidification
  - peak hour and month search
- **[Suggestion]** The RTS engine can be a native Rust provider written from the published method description in open literature, or the EnergyPlus provider's sizing run can be used. The project decides which ships by default (§4.17 Q8).
- **Reports** are time-stamped under *Reports* in the Project Browser. Sections: project summary (location, weather, latitude and longitude, design temperatures), building summary, level and zone summaries, and space details.
- **Phase rule:** system-zones and analytical spaces must match the analysed phase and must be in the host model.

**Data (IFC mapping).** Peak values go to `Pset_SpaceThermalLoad` and `Pset_SpaceThermalDesign` (CoolingDesignAirflow, HeatingDesignAirflow, TotalSensibleHeatGain, TotalHeatGain, TotalHeatLoss). Reports are documents (`IfcDocumentReference`) indexed in **RXD document** `analysis`.

**Commands.**
- `hc_loads.run(options) -> job_id`
- `hc_loads.reports() -> report_ids[]`
- `hc_loads.open(report_id)`

**Acceptance criteria.**
- [ ] For a reference single-zone test case with published RTS results, the computed peak sensible cooling load is within 5% of the reference value.
- [ ] Changing the infiltration class from Medium to Tight lowers the heating load, and the report states the class used.

### 4.12.8 Average estimated illumination (lumen method)
**ID:** ANA-LIGHT-001 · **Priority:** P2 · **Ref:** research/03 §10.1, §12.1, §15.4

**Behaviour & rules.**
- Reviera calculates **average estimated illumination** for each space with the lumen (zonal cavity) method:
  - room cavity ratio from the space geometry and the workplane height
  - reflectances from the space properties
  - coefficient of utilisation from the fixture's photometric data, or from a CU table in the family
  - light loss factor
  - fixtures counted by their room calculation point
- This is a built-in provider (ANA-PLUG-003). It updates in the background when fixtures or spaces change.
- **Formula.** E = N × Φ × CU × LLF ÷ A, where N is the fixture count, Φ the rated lumens per fixture (from the photometric file or the family's *Initial Intensity* parameter), and A the space floor area.
- **Room cavity ratio** RCR = 5 × h_rc × (L + W) ÷ (L × W) for rectangles; for any other shape Reviera uses the general form RCR = 2.5 × h_rc × perimeter ÷ area. h_rc is the distance from the workplane (default 0.76 m, from the space's *Lighting Calculation Workplane*) to the fixture plane (the average fixture height in the space).
- **CU** is interpolated from the fixture's CU table (rows by RCR 0–10, columns by ceiling, wall and floor reflectance). If the photometric file has no zonal data and the family has no CU table, the fixture is excluded and listed in the result with reason "no CU data".
- **LLF** defaults to 0.8 and can be set per fixture type (light loss factor parameter) and overridden per space.
- Defaults for reflectances when the space has none: ceiling 0.8, wall 0.5, floor 0.2.
- Spaces with no fixtures report 0 lx; spaces with zero area or no upper limit are skipped and reported.
- The result is compared with the target (`Pset_SpaceLightingRequirements.Illuminance`) and a *Meets target* yes/no parameter is set, usable in colour schemes.

**Data (IFC mapping).** `Pset_SpaceLightingRequirements.Illuminance` (the target) and a calculated value in RXD element data (`analysis`), also exposed as a schedulable parameter.

**Commands.** `lighting.calc_average(space_ids[] | "all") -> results[]`; each result holds space ID, illuminance, RCR, fixture count, excluded fixtures and *meets target*.

**Acceptance criteria.**
- [ ] A 6 × 5 × 3 m space with 6 fixtures of 3 000 lm each, CU = 0.6 and LLF = 0.8 reports 6 × 3 000 × 0.6 × 0.8 ÷ 30 = 288 lx.
- [ ] For the same space with the workplane at 0.76 m and fixtures at 3.0 m, the reported RCR is 5 × 2.24 × 11 ÷ 30 ≈ 4.11.
- [ ] Moving one fixture into the next space updates both spaces' values without a manual command, and marks no other space stale.
- [ ] A fixture with no CU data is excluded and listed with the reason "no CU data".

### 4.12.9 Daylight and electric lighting analysis
**ID:** ANA-LIGHT-002 · **Priority:** P2 · **Ref:** research/03 §13.4 ("Lighting")

**Behaviour & rules.**
- *Daylight & Solar › Lighting* opens an analysis panel. Choose:
  - an analysis type: illuminance at a time, **annual climate-based metrics (sDA/ASE)** or daylight factor
  - levels
  - sensor-grid height and spacing
  - thresholds

  Then *Run*.
- A model-preparation checklist covers rooms or spaces, material reflectance and transmittance, and glazing.
- Results are drawn on grids in views with analysis display styles.
- **[Suggestion] Provider: Radiance**, run as an external process with its permissive licence verified, possibly through an open daylight-simulation wrapper library. Reviera exports geometry, materials (reflectance and transmittance from material appearance and thermal data), sky definitions from the weather file, and luminaires from IES files.
- **No cloud service or subscription is required.**

**Commands.**
- `daylight.run(type, level_ids[], grid: {height, spacing}, thresholds, provider_id?) -> job_id`
- `daylight.show(view_id, job_id, style_id?)`

**Acceptance criteria.**
- [ ] With the Radiance provider installed, a daylight-factor run on a single room with one window produces a grid result shown in a plan view with a legend.

### 4.12.10 Solar analysis
**ID:** ANA-SOLAR-001 · **Priority:** P2 · **Ref:** research/03 §13.4 ("Solar")

**Behaviour & rules.**
- *Daylight & Solar › Solar*:
  - **study type**: cumulative insolation over a date range, or PV potential
  - **settings**: PV panel efficiency, coverage and payback inputs
  - **surfaces**: all roof exterior surfaces, all mass faces, or a selection
  - *Update*

  Results are managed per view.
- **[Suggestion] Providers:** a Radiance-based irradiance provider, or a built-in sky-patch cumulative method. An open PV-modelling library such as pvlib (BSD-3) could be used for PV yield.
- **Sun studies** (still, single-day, multi-day and lighting) and the **sun path** display are view features in chapter 5. The solar analysis uses the same sun model.
- **Inputs.** Site location and weather file (ANA-EM-001); the date range (default a full year) and hours (default sunrise to sunset); a grid spacing on each analysed face (default 1 m, range 0.1–10 m); and shading context, which is every model element and visible linked element in the 3D view the study is run from, unless a category is excluded.
- **Cumulative insolation.** Results are kWh/m² per grid point over the period, plus per-face totals (kWh) and averages. The built-in provider splits the sky into patches, takes direct and diffuse irradiance from the weather file, and tests visibility from each grid point to each patch and to the sun positions by ray casting against the shading context.
- **PV potential.** Per face: usable area (face area × coverage, default 80%), annual energy = insolation × usable area × panel efficiency (default 18%) × system derate (default 0.86), installed cost (cost per kW peak) and simple payback (cost ÷ (annual energy × energy price)). All defaults are editable in the settings and stored with the study.
- **Studies are per view.** Each 3D view keeps a list of studies; *Update* re-runs the active one. Results draw with an analysis display style (ANA-ADS-001), with a legend. Results become stale when the model, the site or the weather file changes.
- A face with no sun hours in the period (for example a north face in winter at high latitude) reports 0, not an error. Faces of elements in a link are analysed only if the link is visible.
- **Export:** per-face results to CSV, and the grid values to the result package.

**Data (IFC mapping).** The result package (grid points, values, per-face totals, inputs) is stored as a file referenced by `IfcDocumentReference` and indexed in **RXD document** `analysis`. Per-face totals are also schedulable result parameters (ANA-GEN-002) on the analysed elements.

**Commands.**
- `solar.run(study_type, date_range, surfaces: "roofs"|"masses"|face_refs[], pv_settings?) -> job_id`
- `solar.export(job_id, path)` (CSV of per-face results)

**Acceptance criteria.**
- [ ] Cumulative insolation on a south-facing roof face is higher than on a north-facing face of the same pitch for a northern-hemisphere site.
- [ ] A roof face fully shaded by a taller neighbouring mass has at least 50% lower insolation than the same face with the mass hidden.
- [ ] For a 100 m² face with 1 200 kWh/m² per year, 80% coverage, 18% efficiency and derate 0.86, PV potential reports 14 861 kWh per year (within 0.5%).
- [ ] Moving the shading mass marks the study stale until *Update* completes.

### 4.12.11 Analysis display styles (visualisation framework)
**ID:** ANA-ADS-001 · **Priority:** P2 · **Ref:** research/03 §13.4 ("Analysis Display Styles"), §4.6 · web: Settings Tab for Analysis Display Styles

**UI: the Analysis Display Styles dialog** (*Manage › Additional Settings › Analysis Display Styles*, `analysis_style.manage`). Left: the list of styles grouped by display kind, with *New*, *Duplicate*, *Rename* and *Delete*. Right: three tabs and a live preview swatch.
- **Settings** (depends on the kind):
  - *Colored surface*: show grid lines, grid line weight (1–16) and colour
  - *Contour*: contour line count or interval, line weight, whether bands between lines are filled
  - *Markers with text*: marker shape (circle, square, triangle), marker size in sheet units (0.5–20 mm), labels (none, all, or only the points the provider marks as predefined), text type, rounding increment
  - *Diagram*: base line, scale (value per sheet mm), outline or filled, labels
  - *Vector*: arrow head, arrow line weight, scale, and whether arrow length follows magnitude
- **Color**: *gradient* (2 or 3 colour stops) or *ranges* (bands with a colour each); *auto* range from the data, or fixed minimum and maximum; values outside the range use the end colours or a separate out-of-range colour. Colours come from theme-aware palettes, including colour-blind-safe sequential and diverging palettes; users may also enter colours.
- **Legend**: show or hide, position (corner of the view or free), title, number of steps (2–20), units and decimal places, text type, and whether the provider description is shown.

**Behaviour & rules.**
- One shared visualisation framework is used by structural results, lighting, solar, airflow and loads overlays, and by Lua add-ins.
- Add-ins publish results through the `analysis_result.publish` command.
- A style has one display kind; result sets of a different kind cannot use it, and the view's style picker offers only compatible styles.
- Each view has a default analysis display style (a view property); a published result uses the style passed to it, or the view default.
- Editing a style redraws every view that uses it. Deleting a style in use asks for a replacement style of the same kind.
- Results are view-specific graphics: they are not model elements, are not exported to IFC, and print and export to PDF and images like other view graphics. *Reveal Obstacles* (ANA-ROUTE-002) and results cannot be shown in the same view.
- Hovering a result point shows its value and units in a tooltip.

**Data (IFC mapping).** Styles are kept as **RXD document** `analysis`. Per-view assignments are part of the view's RXD data (chapter 5). Published values are kept in the result package of their job, not in the view.

**Commands.**
- `analysis_style.manage()` (opens the dialog)
- `analysis_style.create(name, settings) -> id`
- `analysis_result.publish(view_id, element_or_face_refs[], values[], style_id) -> id`
- `analysis_result.clear(view_id, id?)`

**Acceptance criteria.**
- [ ] A Lua script can publish scalar values on a floor's top face and see them in a gradient with a legend.
- [ ] Changing a gradient style to five fixed bands updates every view that uses it without re-running the analysis.
- [ ] Deleting a style used by two views asks for a replacement of the same kind, and both views then draw with it.
- [ ] A *Markers with text* style with rounding 0.5 shows the value 3.26 as 3.5.

### 4.12.12 Path of travel
**ID:** ANA-ROUTE-001 · **Priority:** P2 · **Ref:** research/03 §13.5 · screenshots/structure_mep/path_of_travel_with_schedule.png

**Workflow.** Choose *Route Analysis › Path of Travel*, then click a start and an end point in a **plan view**.

**Behaviour & rules.**
- Reviera computes the **shortest path that avoids obstacles** inside an **analysis zone**: a band between a bottom and a top offset from the level.
- Obstacles are the elements of non-ignored categories whose geometry intersects the zone. Doors are passable.
- **Waypoints** can be added and removed. Paths can be updated after the model changes, individually or all at once.
- Paths are on **one level only**. Stairs and ramps are not followed in the first release (§4.17 Q9).
- Path lines are view-specific elements that can be tagged and scheduled, with length and time (length ÷ walking speed).
- **Implementation:** build a 2D obstacle map from plan-cut geometry, then search a visibility graph or a navigation mesh. An open navigation-mesh library under the zlib licence is a **[Suggestion]**.

**Data (IFC mapping).** View-specific annotation (chapter 5) with RXD element data (`analysis`): start, end, waypoints, length and time. When exported, paths can optionally be `IfcAnnotation` curves.

**Commands.**
- `path_of_travel.create(view_id, start, end, waypoints?[]) -> id`
- `path_of_travel.update(ids[] | "all_in_view")`
- `path_of_travel.add_waypoint(id, point, index?)`

**Acceptance criteria.**
- [ ] In a plan with a wall between start and end and one door, the path passes through the door, and its length is within 1% of the true shortest path that keeps the configured clearance.
- [ ] A Path of Travel schedule lists length and time, and time = length ÷ walk speed.

### 4.12.13 Route analysis settings, obstacles and people-flow tools
**ID:** ANA-ROUTE-002 · **Priority:** P2 · **Ref:** research/03 §13.5

**Behaviour & rules.**
- **Settings:**
  - ignored categories; hidden, demolished and underlay elements are ignored by default, and a nested family needs both its own and its parent's category ignored
  - analysis-zone offsets
  - walking speed
  - clearance
- **Reveal Obstacles:** a view mode that colours the elements counted as obstacles. It is unavailable in views that show analysis results.
- **Multiple Paths:** several simultaneous routes, with a minimum separation.
- **One-Way Indicator:** a family that routing respects.
- **People Content:** occupancy families that block paths (specified in ANA-ROUTE-003).
- **Spatial Grid:** a square or hexagonal filled-region grid over a room. It does not update with the room.
- A sample Lua script "Longest Exit Distance" is supplied. It finds the most remote point in each room relative to the exits.

**Data (IFC mapping).** **RXD document** `analysis` (route settings).

**Commands.**
- `route_settings.set(params)`
- `route.reveal_obstacles(view_id, on: bool)`
- `path_of_travel.create_multiple(view_id, starts[], ends[], min_separation) -> ids[]`
- `one_way_indicator.place(view_id, point, direction) -> id`
- `spatial_grid.create(room_id, kind: "square"|"hex", size) -> id`

**Acceptance criteria.**
- [ ] A one-way indicator that points against the direct route forces the path to take an alternative door.
- [ ] Ignoring the Furniture category lets paths pass through furniture.

### 4.12.14 Interference check (cross-reference)
**ID:** ANA-CLASH-001 · **Priority:** P1 · **Ref:** research/03 §13.6

**Behaviour & rules.**
- The interference check is specified in **chapter 6** (Collaboration, linking and coordination), together with BCF export. This chapter adds discipline requirements:
  - It shall support all categories in this chapter, **including detailed steel elements (plates, bolts, welds) and fabrication parts**. The reference product excludes these (§4.17).
  - Insulation and lining coverings are included.
  - Typical discipline pairs shall be offered as presets: framing–framing, structural–architectural columns, braces–walls and openings, roofs–floors, equipment–floors, ducts–beams, pipes–ducts, and host–links.
- A clash provider interface (ANA-PLUG-001 `clash`) allows external engines. **[Suggestion]** OCCT-based distance and interference checks in parallel, with a bounding-volume hierarchy broad phase.
- **Discipline rules this chapter adds to the chapter 6 check:**
  - Connected MEP elements (ports joined by `IfcRelConnectsPorts`) never clash with each other, and a fitting never clashes with the segments it connects.
  - A host and its own hosted or embedded elements are not reported: rebar inside its concrete host, welds and bolts in the parts they join, a duct accessory inside its duct, a hanger touching the part it supports.
  - Analytical elements (analytical members, nodes, analytical segments, energy model surfaces, geometry-less analytical equipment) are never tested.
  - Placeholders (MEP-DUCT-007) are tested using a swept body of their nominal size, so early coordination works before conversion.
  - Insulation and lining are tested as separate coverings, so a report can tell "insulation clash" from "duct clash".
  - An optional **clearance** per category pair (for example 50 mm between ducts and beams for access) reports near misses as soft clashes.
- **Presets** are stored as named check definitions (RXD document `coordination/clash`, chapter 6) and can be edited and shared with *Transfer Project Standards*.

**Commands.** See chapter 6 (`interference.run`, `interference.report`). The presets above are ordinary check definitions for those commands.

*Built (2026-10-16):* the whole-model run is out of process: `compute.submit` with kind `analysis.clash` distils the model's bodies (booleans applied; related and accepted pairs left out) and the built-in runner measures them with the same overlap engine as the live checks (ARC-MOD-030), so results match `overlap.find` exactly; `compute.results` attaches them to elements. Multi-model scopes are carried in the format (each element names its model; `ClashOptions.against_models`) but not yet offered. Presets, clearances, BCF and the chapter 6 commands are not built yet.

**Acceptance criteria.**
- [ ] A bolt that intersects a duct is reported as a clash between Structural Fasteners and Ducts.
- [ ] An elbow connected to two ducts produces no clash with either duct.
- [ ] A rebar set fully inside its wall host produces no clash; the same rebar crossing a pipe does.
- [ ] With a 50 mm clearance on Ducts–Structural Framing, a duct 30 mm below a beam is reported as a soft clash and one 80 mm below is not.

### 4.12.15 People content for route analysis
**ID:** ANA-ROUTE-003 · **Priority:** P2 · **Ref:** research/03 §13.5 ("People Flow Toolkit") · web: Revit 2022 – Enhanced Feature – People flow Toolkit

**Purpose.** Represent standing, seated or queuing people in a plan so that path-of-travel routing treats them as obstacles, for occupancy, queuing and distancing studies.

**Workflow.**
1. In a plan view choose *Analyze › Route Analysis › People Content*.
2. Pick a type in the Type Selector: *Standing person*, *Seated person*, *Wheelchair user*, *Queue line* (a row of people at a set spacing) or *Group* (people on a circle).
3. Click to place; Spacebar rotates by 90° before the click. For a queue line, click a start and an end point; the count follows from the spacing.
4. Update paths (`path_of_travel.update`) to see their effect, or let auto-update re-route paths in the view.

**Behaviour & rules.**
- People content is a model family category, *People*, with a body footprint (default standing 0.6 × 0.5 m, wheelchair 0.8 × 1.3 m), an optional **personal-space radius** (default 0, editable per type, for example 1.0 m for distancing studies) and a height for 3D display.
- Route analysis treats the footprint plus personal-space radius as an obstacle in the analysis zone. Ignoring the *People* category in Route Analysis Settings (ANA-ROUTE-002) lets paths pass through them.
- A path whose start or end lies inside a person's obstacle area is refused with "start point is blocked".
- People are hosted on the level of the plan view, not on floors, and are visible in plan and 3D; they are excluded from material takeoffs, area and space calculations and interference checks by default.
- People can be counted in schedules (by room or space), which gives a quick occupancy check against the occupant load of the room.
- Starter types are authored from scratch and ship under the project's content licence.

**Data (IFC mapping).** Exported, if the user enables the *People* category in the IFC export setup, as `IfcFurnishingElement` with ObjectType "PeopleContent" and `IfcBuildingElementProxy` as fallback; off by default. Personal-space radius and kind are **RXD element data** (`analysis`).

**Commands.** `people_content.place(view_id, type_id, point, rotation?) -> id` (for a queue line, `type_id` is a queue type and `point` is the start; the end and spacing go in the type's instance parameters).

**Acceptance criteria.**
- [ ] A standing person placed in a 1.2 m wide corridor with a 1.0 m personal-space radius blocks it, and the updated path takes the alternative route.
- [ ] Ignoring the *People* category makes the same path go straight through.
- [ ] A schedule of people by room lists 12 for a room holding a 12-person queue line.

### 4.12.16 Embodied carbon analysis
**ID:** ANA-CARB-001 · **Priority:** P2 · **Ref:** research/03 §0 (2027 version map: carbon data on materials), §15.4 · web: ÖKOBAUDAT; ICE Database FAQs; Introducing Forma Carbon Insights; CarboLifeCalc

**Purpose.** Estimate the embodied carbon (global warming potential, kg CO₂e) of the model from its material quantities and emission factors taken from EPD data, by life-cycle stage, locally and without a vendor cloud service.

**Workflow.**
1. Make sure materials carry carbon assets (DM-MAT-005): either type values in the Material Browser, or use *Match materials* in the analysis dialog.
2. Choose *Analyze › Carbon › Embodied Carbon*. The dialog has four steps:
   1. **Scope**: whole model, current phase only (default: new construction, excluding existing and demolished), a selection, or a design option; categories to include; whether linked models count.
   2. **Data sources**: the active emission-factor libraries in priority order. Reviera ships none of the commercial data sets; the user imports open data sets (for example ÖKOBAUDAT in ILCD+EPD format, or an openLCA export) or a CSV in the documented Reviera factor format. Data sets whose licence forbids embedding in software (for example the ICE database's individual licence) can only be used as a user-imported file on that user's machine.
   3. **Matching**: a grid of project materials with match status (*from material asset*, *matched*, *unmatched*). *Auto-match* proposes factors by material class and name; the user confirms or picks manually. Unmatched materials are reported, not guessed.
   4. **Stages and options**: stages to report (A1–A3 always; optional A4, A5, B1–B5, C1–C4 and module D shown separately), reference study period (default 50 years, for replacement counts in B4), gross floor area for normalisation (default from area plans), and *Write results to IFC*.
3. *Run* starts a background job (ANA-GEN-001). A notification links to the results.

**UI: Results.** A dockable *Carbon Results* panel and an exportable report:
- totals: kg CO₂e for the scope, per stage, and per m² of gross floor area
- **by category** (walls, floors, structural framing, …) and **by material**, as sortable tables and bar charts; biogenic carbon and module D listed separately and never netted into the A–C total
- **by element**: a schedulable *Embodied carbon A1–A3* parameter and per-stage parameters, so schedules, colour schemes and view filters can show hot spots
- a **data quality** column: factor source, generic or product-specific, EPD validity date (expired EPDs flagged)
- *Compare*: two saved runs side by side (for example two design options), with the difference per category

**Behaviour & rules.**
- **Quantity.** For each element and material: volume from the geometry (layer thickness × area for layered types), area for factors declared per m², count for per-piece factors, mass = volume × density (`Pset_MaterialCommon.MassDensity`) for per-kg factors. A unit mismatch with no conversion path (for example a per-m² factor on a material with no area) excludes that material and reports it.
- **Calculation.** GWP = quantity × factor per stage. B4 (replacement) = (A1–A3 + C1–C4) × replacements, with replacements = ceil(study period ÷ reference service life) − 1. Transport (A4) uses a distance and mode per material when given, otherwise is not reported.
- Openings are subtracted; overlapping geometry is counted once per element; elements without materials are listed as *no material*.
- Factors are snapshotted into the run so results stay reproducible after a library update; a library update marks the run stale.
- This is a **built-in provider** (ANA-PLUG-003, family `carbon`); external LCA tools can register as providers of the same family.
- **Export:** CSV and JSON of per-element and per-category results; HTML report; the run's input quantities in a neutral CSV for third-party LCA tools.

**Data (IFC mapping).** When *Write results to IFC* is on, the factor per functional unit goes to `Pset_EnvironmentalImpactIndicators.ClimateChangePerUnit` (with `FunctionalUnitReference`) on the element type, and the element total to `Pset_EnvironmentalImpactValues.ClimateChange` on the occurrence, as DM-MAT-005 describes. The factor libraries, matching table, options and run index are **RXD document** `analysis`; the result package is a file referenced by `IfcDocumentReference`.

**Commands.**
- `embodied_carbon.run(scope?, options?) -> job_id` (`scope`: "model" | "phase" | element_ids[] | design_option_id; `options`: stages, libraries, study_period, gfa, write_ifc)
- `embodied_carbon.import_factors(path, format: "ilcd"|"csv") -> library_id`
- `embodied_carbon.match(material_id, factor_ref)`
- `embodied_carbon.export(job_id, path, format: "csv"|"json"|"html")`
- `embodied_carbon.compare(job_id_a, job_id_b) -> report`

**Acceptance criteria.**
- [ ] A 10 m³ concrete wall with density 2 400 kg/m³ and a factor of 0.10 kg CO₂e/kg reports 2 400 kg CO₂e for A1–A3.
- [ ] With a study period of 60 years and a reference service life of 25 years, B4 counts 2 replacements.
- [ ] A material with no factor is listed as unmatched, and the total states the share of model volume left unmatched.
- [ ] Module D and biogenic carbon appear in separate columns and are not subtracted from the A–C total.
- [ ] Editing a wall's thickness after a run marks the run stale; re-running changes only that wall's result.

---

## 4.13 P&ID integration

**Design stance.** The reference product's P&ID integration depends on a vendor cloud service. Reviera instead reads **open P&ID exchange data**: the **DEXPI** XML standard, which is based on ISO 15926 and the Proteus schema. It offers the same guided-modelling and validation workflow, working locally.

### 4.13.1 P&ID line list and guided modelling
**ID:** MEP-PID-001 · **Priority:** P2 · **Ref:** research/03 §14

**Workflow.**
1. Choose *Systems › P&ID › Line List* and open a P&ID exchange file, DEXPI XML or a CSV line list.
2. The palette's top pane previews the diagram. DEXPI graphics are rendered as 2D; for CSV, an optional PDF of the diagram can be loaded.
3. The bottom pane lists **pipeline segments**, searchable by line number or component. A status glyph shows *modelled*, *partially modelled* or *not modelled*. The glyphs use theme tokens with text labels, so they never rely on colour alone.
4. Double-click a line to list its segments, inline components and from/to equipment. Hovering highlights items in the preview.
5. Model pipes and components in the 3D model, guided by the list. Starting a pipe from the palette sets its system type and size automatically.
6. **Map** P&ID items to existing model elements.

**Behaviour & rules.** Model elements remember their P&ID tag. Re-loading an updated P&ID file keeps the mappings by tag.

**Data (IFC mapping).** The P&ID tag is written to `IfcElement.Tag`, and pipelines to `IfcDistributionSystem.Name`. The mapping table and the source file reference are **RXD document** `mep_settings` (P&ID section). The source file is referenced with `IfcDocumentReference`.

**Commands.**
- `pid.open(path) -> pid_id`
- `pid.list_lines(pid_id, filter?) -> lines[]`
- `pid.start_pipe(pid_id, line_id, segment_id) -> tool_session`
- `pid.map(pid_item_id, element_ids[])`

**Acceptance criteria.**
- [ ] Loading a DEXPI sample file lists every piping network segment with its line number, nominal size and from/to equipment.
- [ ] Starting a pipe from a 50 mm line sets the pipe diameter to 50 mm and the system type from the service mapping.

### 4.13.2 P&ID validation and mapping settings
**ID:** MEP-PID-002 · **Priority:** P2 · **Ref:** research/03 §14 ("Validate", "Mappings")

**Purpose.** Check that the 3D piping model agrees with the P&ID, and define how P&ID classes and properties translate into Reviera types, families and parameters.

**Workflow.**
1. Set up the mappings once in *Systems › P&ID › P&ID Settings* (also `mep_settings.show(kind: "pid")`).
2. Model from the line list (MEP-PID-001).
3. Choose *Validate* on the palette for one line, the selected lines, or the whole P&ID. Results appear as status labels in the line list and as a report.
4. Double-click a result to select and zoom to the model elements involved; use *Ignore* on a part that is intentionally different.

**Behaviour & rules.**
- **Validate** compares the model against the P&ID and marks each line:
  - fully modelled: every P&ID component on the line has a mapped model element, connected in the same order
  - mismatch, such as a size, spec or component difference (nominal size differs, pipe type differs from the spec mapping, or an inline component uses a different family than the mapping)
  - disconnected: the mapped elements of the line do not form one connected path (from the connectivity graph, MEP-CONN-002)
  - with ignored parts: differences the user marked *Ignore*, kept with the user name and date
  - with additional parts: model elements tagged with the line number that the P&ID does not contain (fittings and flanges added by routing preferences are not counted)

  Indicators use text and icons drawn from theme tokens.
- Validation is read-only and runs in the background; it never changes the model. Results are stale once the model or the P&ID file changes.
- **P&ID Settings** has two tabs, *System Types* and *Object Mapping*. They map:
  - piping services to pipe system types
  - pipe line specs to pipe types
  - inline assets to pipe accessory families
  - equipment classes to equipment families
  - P&ID properties to parameters (text, number and Yes/No; units converted where the P&ID states them)
- Each tab is a two-column grid (P&ID item → Reviera target) with a search box, *Auto-map by name* and *Import/Export* as CSV. Rows with no target are labelled "Not mapped" and count as mismatches during validation. A target type or family that is deleted from the project makes its row invalid; OK is allowed but the dialog lists invalid rows.
- Mappings can be saved as a named mapping set and reused across projects through *Transfer Project Standards*.

**Data (IFC mapping).** Mappings, ignore records and the last validation result are stored in **RXD document** `mep_settings` (P&ID section). Line numbers on model elements are in `IfcElement.Tag` (MEP-PID-001).

**Commands.**
- `pid.validate(pid_id, line_ids?[]) -> report`
- `pid.ignore(pid_id, item_id, ignored: bool)`
- `pid_settings.set_mapping(kind, rules[])`
- `pid_settings.auto_map(kind) -> rules[]`
- `pid_settings.export_mapping(path)`, `pid_settings.import_mapping(path)`

**Acceptance criteria.**
- [ ] Changing a modelled valve to a different family than the mapping specifies marks its line "mismatch", and the report names the valve.
- [ ] Deleting a pipe segment in the middle of a fully modelled line changes its status to "disconnected" after re-validation.
- [ ] A difference marked *Ignore* is reported under "ignored parts" and no longer causes a mismatch.
- [ ] *Auto-map by name* maps the service "CW" to a pipe system type named "CW" and leaves unmatched services labelled "Not mapped".

---

## 4.14 IFC mapping summary

| Reviera concept | IFC4 (ADD2 TC1) entity / PredefinedType | Key relationships | Psets / Qtos |
|-----------------|------------------------------------------|-------------------|--------------|
| Structural column | `IfcColumn` `COLUMN` / `IfcColumnType` | `IfcRelDefinesByType`, `IfcRelConnectsElements` (attach) | `Pset_ColumnCommon`, `Qto_ColumnBaseQuantities` |
| Beam (girder, joist) | `IfcBeam` `BEAM`/`LINTEL`/`HOLLOWCORE`/`T_BEAM` | `IfcRelConnectsPathElements` (joins) | `Pset_BeamCommon`, `Qto_BeamBaseQuantities` |
| Purlin, brace, truss chord or web | `IfcMember` `PURLIN`/`BRACE`/`CHORD`/`STRUT`/`MEMBER` | — | `Pset_MemberCommon`, `Qto_MemberBaseQuantities` |
| Beam system / truss | `IfcElementAssembly` `BEAM_GRID` / `TRUSS` | `IfcRelAggregates` | — |
| Structural wall | `IfcWall` `SHEAR`/`SOLIDWALL` | — | `Pset_WallCommon.LoadBearing` |
| Structural floor / foundation slab | `IfcSlab` `FLOOR` / `BASESLAB` | `IfcMaterialLayerSetUsage` | `Pset_SlabCommon` |
| Footings | `IfcFooting` `PAD_FOOTING`/`STRIP_FOOTING`/`PILE_CAP` | — | `Pset_FootingCommon` |
| Pile | `IfcPile` `BORED`/`DRIVEN`/… | `IfcRelAggregates` (under cap) | `Pset_PileCommon` |
| Structural connection | `IfcElementAssembly` `USERDEFINED` | `IfcRelConnectsWithRealizingElements` | — |
| Rebar set | `IfcReinforcingBar` `MAIN`/`LIGATURE`/… / `IfcReinforcingBarType` (BendingShapeCode, BendingParameters) | `IfcRelAggregates` (host) | `Pset_ReinforcingBarBendings*Common` |
| Area/path/fabric system | `IfcElementAssembly` `REINFORCEMENT_UNIT` | `IfcRelAggregates` | — |
| Fabric sheet | `IfcReinforcingMesh` / `IfcReinforcingMeshType` | `IfcRelAggregates` | — |
| Rebar coupler | `IfcMechanicalFastener` `USERDEFINED` "COUPLER" (IFC4X3: `COUPLER`) | `IfcRelConnectsWithRealizingElements` | — |
| Steel plate | `IfcPlate` `SHEET` | — | `Pset_PlateCommon` |
| Bolt / anchor / shear stud | `IfcMechanicalFastener` `BOLT`/`ANCHORBOLT`/`STUDSHEARCONNECTOR` | `IfcRelConnectsWithRealizingElements` | `Pset_MechanicalFastenerBolt` |
| Weld | `IfcFastener` `WELD` | `IfcRelConnectsWithRealizingElements` | `Pset_FastenerWeld` |
| Holes, cuts, copes | `IfcOpeningElement` | `IfcRelVoidsElement` | — |
| Precast piece | `IfcWall`/`IfcSlab` in `IfcElementAssembly` | `IfcRelAggregates` | `Pset_PrecastConcreteElementGeneral`, `Pset_PrecastConcreteElementFabrication` |
| Analytical model | `IfcStructuralAnalysisModel` `LOADING_3D` | `IfcRelAssignsToGroup`, `IfcRelServicesBuildings` | — |
| Analytical member / panel | `IfcStructuralCurveMember` / `IfcStructuralSurfaceMember` | `IfcRelConnectsStructuralMember`, `IfcRelAssignsToProduct` (association) | — |
| Node / support | `IfcStructuralPointConnection` (`IfcStructuralCurveConnection`, `IfcStructuralSurfaceConnection`) | AppliedCondition `IfcBoundaryNodeCondition`/`EdgeCondition`/`FaceCondition` | — |
| Loads | `IfcStructuralPointAction`/`CurveAction`/`PlanarAction` + `IfcStructuralLoadSingleForce`/`LinearForce`/`PlanarForce` | `IfcRelConnectsStructuralActivity` | — |
| Load case / combination | `IfcStructuralLoadCase` / `IfcStructuralLoadGroup` `LOAD_COMBINATION` | `IfcRelAssignsToGroup`, `IfcRelAssignsToGroupByFactor` | — |
| Results | `IfcStructuralResultGroup`, `IfcStructuralPointReaction` | `IfcRelAssociatesDocument` (package) | — |
| Connector | `IfcDistributionPort` `DUCT`/`PIPE`/`CABLE`/`CABLECARRIER` | `IfcRelNests`, `IfcRelConnectsPorts` | `Pset_DistributionPortCommon`, `Pset_DistributionPortType*` |
| Duct / fitting / accessory | `IfcDuctSegment`, `IfcDuctFitting`, `IfcDamper`, `IfcFilter`, `IfcDuctSilencer`, `IfcCoil` | ports | `Pset_DuctSegmentTypeCommon`, `Pset_DuctSegmentOccurrence`, `Qto_DuctSegmentBaseQuantities` |
| Air terminal / equipment | `IfcAirTerminal`, `IfcAirTerminalBox`, `IfcUnitaryEquipment`, `IfcFan`, `IfcPump`, `IfcBoiler`, `IfcChiller`, `IfcSpaceHeater` | ports | type psets per class |
| Pipe / fitting / valve | `IfcPipeSegment`, `IfcPipeFitting`, `IfcValve`, `IfcFlowMeter` | ports | `Pset_PipeSegmentTypeCommon`, `Pset_PipeSegmentOccurrence` |
| Fixtures / sprinklers | `IfcSanitaryTerminal`, `IfcWasteTerminal`, `IfcFireSuppressionTerminal` `SPRINKLER` | ports | `Pset_FireSuppressionTerminalTypeSprinkler` |
| Insulation / lining | `IfcCovering` `INSULATION`/`WRAPPING` | `IfcRelCoversBldgElements` | — |
| Cable tray / conduit | `IfcCableCarrierSegment` `CABLETRAYSEGMENT`/`CONDUITSEGMENT`, `IfcCableCarrierFitting` | ports | `Pset_CableCarrierSegmentType*` |
| Cable | `IfcCableSegment` `CABLESEGMENT` | — | `Pset_CableSegmentType*` |
| Panel / transformer | `IfcElectricDistributionBoard`, `IfcTransformer`, `IfcProtectiveDevice` | ports | `Pset_ElectricDistributionBoard*` |
| Devices / lighting | `IfcOutlet`, `IfcSwitchingDevice`, `IfcAlarm`, `IfcSensor`, `IfcLightFixture` + `IfcLightSourceGoniometric` | ports | `Pset_LightFixtureTypeCommon` |
| System | `IfcDistributionSystem` (PredefinedType from classification) | `IfcRelAssignsToGroup`, `IfcRelServicesBuildings` | `Pset_DistributionSystemCommon` |
| Circuit | `IfcDistributionCircuit` | `IfcRelAssignsToGroup` | `Pset_DistributionSystemTypeElectrical` |
| Fabrication spool / hanger | `IfcElementAssembly` "SPOOL" / `IfcDiscreteAccessory` "HANGER" | `IfcRelAggregates`, `IfcRelConnectsElements` | — |
| Space | `IfcSpace` (ObjectType "MEPSpace") | `IfcRelSpaceBoundary` | `Qto_SpaceBaseQuantities`, `Pset_SpaceThermal*`, `Pset_SpaceLightingRequirements` |
| System-zone | `IfcSpatialZone` `THERMAL` + `IfcZone` | `IfcRelAssignsToGroup` | `Pset_ZoneCommon` |
| Energy model surfaces | `IfcRelSpaceBoundary2ndLevel` | `CorrespondingBoundary` | — |

**General rule.** Anything not listed above, and any value that has no standard home, is stored as RXD (see "Where data is stored" at the start of this chapter) and keyed by GlobalId. The IFC file shall stay valid and meaningful to other tools without it.

---

## 4.15 Optional engine integration summary [Suggestions]

| Capability | Provider kind | Built-in | Suggested open engine (external process) | Licence note (verify before bundling) |
|------------|---------------|----------|-------------------------------------------|----------------------------------------|
| Structural analysis (linear static, modal, non-linear) | `structural` | — | PyNite or Frame3DD (frames), CalculiX, Code_Aster (via IfcOpenShell's ifc2ca), Kratos Multiphysics; xara (seismic, OpenSees-based), all batch providers as compute jobs (ANA-PLUG-004, §2.17) | MIT or GPL-3.0; GPL-2.0-or-later; GPL-3.0; BSD; BSD-2-Clause (OpenSees itself: commercial licence needed, not suggested) |
| Earthquake assessment (response spectrum, time history, pushover) | `seismic` | Equivalent lateral force from the building code's spectrum (DM-CODE-001) | xara (OpenSees-based); Code_Aster | BSD-2-Clause; GPL-3.0 |
| Steel connection checks | `structural` (connection_check) | Sample Lua bolt check | Community providers | Must be GPL-compatible if in-process |
| Load combination generation | built-in rules | EN 1990 / ASCE 7-style templates as data | — | Templates authored from the published equations, not copied text |
| Energy simulation and HVAC sizing | `energy` | — | EnergyPlus via OpenStudio | BSD-style |
| Heating/cooling loads (RTS) | `loads` | Native RTS (optional, §4.17 Q8) | EnergyPlus sizing run | — |
| Daylight and electric lighting | `daylight` | Lumen method | Radiance | Radiance licence (BSD-like) |
| Solar insolation, PV, sun paths and shadow studies | `solar` | Sky-patch cumulative | Radiance; pvlib-python | BSD-like; BSD-3 |
| Hydraulic networks (domestic water, fire) | `hydraulic` | Duct/pipe friction, hydronic loops | EPANET | Public domain/MIT |
| Stormwater runoff and drainage networks | `stormwater` | Rational-method roof and gutter sizing (MEP-PIPE-010) | EPA SWMM | Public domain (verify) |
| Interior airflow and ventilation (CFD) | `airflow` | — | OpenFOAM | GPL-3.0 |
| External wind flow and facade pressures (CFD) | `wind` | — | OpenFOAM | GPL-3.0 |
| Clash detection | `clash` | OCCT + BVH | — | — |
| Route analysis | `route` | Visibility graph | Recast/Detour navigation mesh | zlib |
| Embodied carbon | `carbon` | Quantity × emission factor (ANA-CARB-001) | openLCA, for full life-cycle assessment | MPL-2.0 (verify); data sets have their own licences |
| Steel CNC | output plugin | — | DSTV NC writer | Open format documentation |
| Precast CAM | output plugin | Neutral JSON/CSV | Unitechnik / PXML writers | Depends on documentation terms |
| P&ID | importer | CSV line list | DEXPI reader | Open standard |

---

## 4.16 Command catalog

Every command below is registered once in the command registry and is exposed automatically to the UI, to Lua (`reviera.<command>(...)`) and to MCP (one tool per command). Commands marked "Results only" start background jobs and commit their results in a separate transaction that can be undone. Commands marked "No" are queries, exports or view/session state and leave model data unchanged.

| Command | Parameters | Returns | Undoable | Req ID |
|---------|------------|---------|----------|--------|
| `analysis_provider.list` | kind? | providers[] | No | ANA-PLUG-001 |
| `analysis_provider.register` | manifest_path | provider_id | Yes | ANA-PLUG-001 |
| `analysis_provider.start` | provider_id, host_id? | {state} | No | ANA-PLUG-005 |
| `analysis_provider.stop` | provider_id | — | No | ANA-PLUG-005 |
| `analysis_provider.status` | provider_id? | [{provider_id, mode, state, location, pid?, memory_bytes?, jobs[]}] | No | ANA-PLUG-009 |
| `analysis_provider.set_options` | provider_id, {live_budget_ms?, idle_timeout_s?, memory_limit_bytes?, cpu_limit?, location?} | — | Yes | ANA-PLUG-005 |
| `analysis_provider.test` | provider_id, host_id? | {ok, manifest, messages[]} | No | ANA-PLUG-009 |
| `analysis_job.status` | job_id | {state, progress, messages} | No | ANA-GEN-001 |
| `analysis_job.cancel` | job_id | — | No | ANA-GEN-001 |
| `column.place` | type_id, point, base_level, top_constraint, base_offset, top_offset, rotation? | element_id | Yes | STR-COL-001 |
| `column.place_at_grids` | type_id, grid_ids[], base_level, top_constraint, rotation? | element_id[] | Yes | STR-COL-001 |
| `column.place_in_columns` | type_id, arch_column_ids[] | element_id[] | Yes | STR-COL-001 |
| `column.attach` | column_ids[], end: top\|base, target_id, style?, justification?, offset? | () | Yes | STR-COL-001 |
| `column.detach` | column_ids[], end: top\|base, target_ids? | () | Yes | STR-COL-001 |
| `column.place_slanted` | type_id, p1, p1_level, p1_offset, p2, p2_level, p2_offset, style | element_id | Yes | STR-COL-002 |
| `column.location_mark` | column_ids[] | [{column_id, mark?}] | No | STR-COL-001 |
| `column.load_section` | catalog, designation, material? | type_id | Yes | STR-SEC-001 |
| `column.check` | column_ids? | [advisory] | No | STR-SEC-001 |
| `beam.set` | beam_ids[], reference_level?, start_offset?, end_offset?, rotation?, orientation?, y_justification?, y_offset?, z_justification?, z_offset?, start_extension?, end_extension?, usage? | — | Yes | STR-BEAM-001 |
| `beam.load_section` | catalog, designation, material? | type_id | Yes | STR-SEC-001 |
| `beam.info` | beam_ids[] | [{beam_id, usage, usage_locked, length, supports, designation, material}] | No | STR-BEAM-001 |
| `framing.join` | id, end | — | Yes | STR-BEAM-002 |
| `structural.catalogs` | catalog? | [catalogue] | No | STR-SEC-001 |
| `structural.standards` | — | {region, catalogs, steel, concrete, standards} | No | STR-SEC-001 |
| `beam.create` | type_id, curve, reference_level, usage?, orientation?, z_justification?, y_justification? | element_id | Yes | STR-BEAM-001 |
| `beam.create_on_grids` | type_id, grid_ids[], level | element_id[] | Yes | STR-BEAM-001 |
| `beam.set_usage` | beam_ids[], usage, lock? | — | Yes | STR-BEAM-001 |
| `framing.set_justification` | ids[], y_just, y_offset, z_just, z_offset, mode | — | Yes | STR-BEAM-002 |
| `framing.set_extension` | id, end, length | — | Yes | STR-BEAM-002 |
| `framing.set_cutback` | id, end, distance | — | Yes | STR-BEAM-002 |
| `framing.join_edit` | join_id, through_member_id?, miter? | — | Yes | STR-BEAM-002 |
| `framing.disjoin` | id, end | — | Yes | STR-BEAM-002 |
| `framing.change_reference` | id, end, reference | — | Yes | STR-BEAM-002 |
| `structural_settings.set` | key="point_to_point_steel", value | — | Yes | STR-SET-001, STR-BEAM-003 |
| `beam_system.create_auto` | view_id, pick_point, type_id, layout_rule, value, justification? | system_id | Yes | STR-BSYS-001 |
| `beam_system.create_sketch` | boundary_curves[], direction_edge, type_id, layout_rule, value, lock_to_supports? | system_id | Yes | STR-BSYS-001 |
| `beam_system.set_layout` | system_id, rule, value, justification | — | Yes | STR-BSYS-001 |
| `beam_system.remove` | system_id | beam_ids[] | Yes | STR-BSYS-001 |
| `brace.create` | type_id, start_point, start_level, start_offset, end_point, end_level, end_offset | element_id | Yes | STR-BRACE-001 |
| `truss.create` | type_id, start, end, level | truss_id | Yes | STR-TRUSS-001 |
| `truss.edit_profile` | truss_id, top_curves[], bottom_curves[] | — | Yes | STR-TRUSS-001 |
| `truss.attach` | truss_id, chord: "top"\|"bottom", target_id | — | Yes | STR-TRUSS-001 |
| `truss.detach` | truss_id, chord | — | Yes | STR-TRUSS-001 |
| `truss.remove` | truss_id | member_ids[] | Yes | STR-TRUSS-001 |
| `wall.create` | type_id, curve, base_level, top_constraint: Level(id) \| Unconnected(height), base_offset?, top_offset?, location_line?, flip?: bool, structural?: bool, structural_usage?, room_bounding?: bool, join_status?: Allow\|Disallow, auto_join?: Off\|Join\|JoinAndLock | element_id | Yes | STR-WALL-001 |
| `wall.set_structural_usage` | ids[], usage | — | Yes | STR-WALL-001 |
| `floor.create` | type_id, loops, level_id, offset?, slope?: {line_key\|curve, slope} \| {arrow: SlopeArrow}, structural?, span_direction_edge?, room_bounding?, attach_walls_below?: bool, join_overlapping_walls?: bool | element_id | Yes | STR-FLOOR-001 |
| `floor.set_span_direction` | floor_id, edge_index \| vector | — | Yes | STR-FLOOR-001 |
| `floor_type.set_deck_layer` | type_id, layer_index, profile_family_id, usage | — | Yes | STR-FLOOR-001 |
| `footing.place` | type_id, point, level, offset | — | Yes | STR-FND-001 |
| `footing.place_at_columns` | type_id, column_ids[] | — | Yes | STR-FND-001 |
| `footing.place_at_grids` | type_id, grid_ids[] | — | Yes | STR-FND-001 |
| `wall_foundation.create` | type_id, wall_ids[] | — | Yes | STR-FND-002 |
| `foundation_slab.create` | type_id, boundary, level, offset | — | Yes | STR-FND-003 |
| `pile.place` | type_id, point, level, length | — | Yes | STR-FND-004 |
| `pile_cap.place` | type_id, point, level | — | Yes | STR-FND-004 |
| `structural_connection.create` | member_ids[], type_id? | connection_id | Yes | STR-CONN-001 |
| `structural_connection.set_type` | connection_id, type_id | — | Yes | STR-CONN-001 |
| `structural_connection.propagate` | connection_id, target_join_ids[] \| "matching" | connection_ids[] | Yes | STR-CONN-001 |
| `structural_connection.break` | connection_id | element_ids[] | Yes | STR-CONN-001 |
| `connection_library.load` | type_ids[] | — | Yes | STR-CONN-001 |
| `connection_library.unload` | type_ids[] | — | Yes | STR-CONN-001 |
| `structural_connection.show_warnings` | view_id, on: bool | — | No | STR-CONN-001 |
| `structural_settings.get` | — | table | No | STR-SET-001 |
| `structural_settings.show` | page?: structural, connection, analytical, boundary_conditions or load_display | — | No | STR-SET-001 |
| `rebar.can_host` | element_id | bool | No | STR-RBR-001 |
| `rebar.rehost` | rebar_ids[], host_id | — | Yes | STR-RBR-001 |
| `reinforcement_settings.set` | key, value | — | Yes | STR-RBR-002 |
| `reinforcement_settings.show` | — | — | No | STR-RBR-002 |
| `rebar_cover_type.create` | name, offset | id | Yes | STR-RBR-002 |
| `rebar_cover_type.manage` | — | — | No | STR-RBR-002 |
| `rebar_cover.assign` | host_id, face_ref? , cover_type_id | — | Yes | STR-RBR-002 |
| `rebar.place` | host_id, shape_id, bar_type_id, method: "expand"\|"two_points"\|"sketch", plane, orientation, points?, layout? | rebar_id | Yes | STR-RBR-003 |
| `rebar_shape_browser.show` | — | — | No | STR-RBR-003 |
| `rebar.set_layout` | rebar_id, rule, n?, spacing?, clear?, formula? | — | Yes | STR-RBR-004 |
| `rebar.convert_to_varying` | rebar_id | — | Yes | STR-RBR-005 |
| `rebar.convert_to_standard` | rebar_id | — | Yes | STR-RBR-005 |
| `rebar.set_segment_length` | rebar_id, segment, length | — | Yes | STR-RBR-006 |
| `rebar.move_bar_in_set` | rebar_id, bar_index, delta | — | Yes | STR-RBR-006 |
| `rebar.remove_bar` | rebar_id, index | — | Yes | STR-RBR-006 |
| `rebar.reset_bars` | rebar_id | — | Yes | STR-RBR-006 |
| `rebar.split_set` | rebar_id, at_index | rebar_id | Yes | STR-RBR-006 |
| `rebar.propagate` | rebar_ids[], target_host_ids[], mode: "host"\|"face", face_ref? | rebar_ids[] | Yes | STR-RBR-006 |
| `rebar.pick_new_host` | rebar_ids[], host_id | — | Yes | STR-RBR-006 |
| `rebar.sketch` | host_id, plane, curves[], bar_type_id, style, hooks? | rebar_id | Yes | STR-RBR-007 |
| `rebar.constrain` | rebar_id, handle_id, target_kind, target_ref, offset | — | Yes | STR-RBR-007 |
| `rebar.reset_constraints` | rebar_id | — | Yes | STR-RBR-007 |
| `rebar_shape.create_from_sketch` | curves[], params, code? | shape_id | Yes | STR-RBR-008 |
| `rebar_shape.match` | rebar_id | shape_id \| null | No | STR-RBR-008 |
| `rebar_shape.check` | shape_id | issues[] | No | STR-RBR-008 |
| `rebar_shape_catalogue.load` | path | — | Yes | STR-RBR-008 |
| `rebar_bar_type.create` | name, diameter, material_id, ... | type_id | Yes | STR-RBR-009 |
| `rebar_hook_type.create` | name, angle, extension_multiplier | id | Yes | STR-RBR-009 |
| `rebar.add_crank` | rebar_id, segment, position, length, offset, angle | — | Yes | STR-RBR-009 |
| `area_reinforcement.create` | host_id, boundary \| "whole_host", major_edge, layers[], layout_rule | system_id | Yes | STR-RBR-010 |
| `area_reinforcement.remove_system` | system_id | rebar_ids[] | Yes | STR-RBR-010 |
| `path_reinforcement.create` | host_id, path_curves[], side, primary, alternating?, spacing \| count, face | system_id | Yes | STR-RBR-011 |
| `fabric_area.create` | host_id, boundary, major_edge, sheet_type_id, lap_position, start_edge? | system_id | Yes | STR-RBR-012 |
| `fabric_sheet.place` | host_id, sheet_type_id, point, rotation, cut_by_cover? | id | Yes | STR-RBR-012 |
| `fabric_sheet.create_bent` | host_id, profile_curves[], sheet_type_id, extent | id | Yes | STR-RBR-012 |
| `rebar.place_free_form` | host_id, kind: "surface"\|"aligned", start_face, end_face, path_edges?, bar_type_id, layout | rebar_id | Yes | STR-RBR-013 |
| `rebar.distribute_along_path` | main_rebar_id, edge_refs[], range? | rebar_id | Yes | STR-RBR-014 |
| `rebar.distribute_morphed` | driver_stirrup_set_id, bar_type_id, count \| spacing | rebar_id | Yes | STR-RBR-014 |
| `rebar.splice` | rebar_id, mode: "line"\|"length", line? , length?, splice_type_id? | rebar_ids[] | Yes | STR-RBR-015 |
| `rebar_coupler.place` | bar_end_refs[1..2], type_id? | coupler_id | Yes | STR-RBR-015 |
| `reinforcement_numbering.renumber` | partition?, kind: "rebar"\|"fabric"\|"coupler", start?, remove_gaps? | — | Yes | STR-RBR-016 |
| `reinforcement_numbering.show` | partition? | — | No | STR-RBR-016 |
| `rebar.set_partition` | ids[], name | — | Yes | STR-RBR-016 |
| `rebar.set_view_visibility` | rebar_ids[], view_id, unobscured?, solid? | — | Yes | STR-RBR-017 |
| `rebar.set_presentation` | rebar_ids[], view_id, mode, selected_indices? | — | Yes | STR-RBR-017 |
| `rebar_tag.place` | rebar_id, view_id, point, tag_type_id | id | Yes | STR-RBR-018 |
| `multi_rebar_annotation.place` | rebar_ids[], view_id, kind: "aligned"\|"linear", line | id | Yes | STR-RBR-018 |
| `bending_detail.place` | rebar_id, view_id, point, scale? | id | Yes | STR-RBR-018 |
| `analytical_member.create` | curve, section_type_id, material_id, role, releases? | id | Yes | STR-AM-001 |
| `analytical_member.create_vertical` | point, level, section_type_id | id | Yes | STR-AM-001 |
| `analytical_member.set_releases` | id, end, dof_flags \| preset | — | Yes | STR-AM-001 |
| `analytical_panel.create_by_boundary` | plane, boundary_curves[], thickness, material_id, role | id | Yes | STR-AM-002 |
| `analytical_panel.create_by_extrusion` | work_plane, profile_curves[], depth, thickness, material_id | id | Yes | STR-AM-002 |
| `analytical_panel.split` | id, line | ids[] | Yes | STR-AM-002 |
| `analytical_opening.create` | panel_id, boundary_curves[] | id | Yes | STR-AM-002 |
| `analytical_panel.set_cladding` | id, bool | — | Yes | STR-AM-002 |
| `analytical_link.create` | node_a, node_b, link_type_id | id | Yes | STR-AM-003 |
| `analytical_link_type.create` | name, fixity[6] | id | Yes | STR-AM-003 |
| `analytical_node.merge` | tolerance? | — | Yes | STR-AM-003 |
| `analytical_association.add` | physical_ids[], analytical_ids[] | association_id | Yes | STR-AM-004 |
| `analytical_association.remove` | association_id | — | Yes | STR-AM-004 |
| `analytical_association.suggest` | scope? | proposals[] | No | STR-AM-004 |
| `load.create_point` | host_id, position?, case_id, forces[6], orientation | id | Yes | STR-LOAD-001 |
| `load.create_line` | host_id, path?, case_id, start[6], end[6], orientation, projected? | id | Yes | STR-LOAD-001 |
| `load.create_area` | host_id, boundary?, case_id, values[3][3], orientation, projected? | id | Yes | STR-LOAD-001 |
| `load_case.create` | name, nature, category | id | Yes | STR-LC-001 |
| `load_case.delete` | id | — | Yes | STR-LC-001 |
| `load_combination.create` | name, formula, limit_state, usages[] | id | Yes | STR-LC-001 |
| `load_combination.generate` | code_template_id, options | proposals[] | Yes | STR-LC-001 |
| `load_nature.create` | name, action_source | id | Yes | STR-LC-001 |
| `structural_code.set` | region, code_id | — | Yes | STR-LC-001 |
| `load_case.manage` | tab?: code, natures, cases, combinations or usages | — | No | STR-LC-001 |
| `boundary_condition.create` | kind: "point"\|"line"\|"area", host_id, location?, state, dof?: {tx,ty,tz,rx,ry,rz} | id | Yes | STR-BC-001 |
| `analytical_automation.physical_to_analytical` | element_ids[] \| "unassociated", package_id? | report | Yes | STR-AM-006 |
| `analytical_automation.analytical_to_physical` | element_ids[], package_id? | report | Yes | STR-AM-006 |
| `analytical_automation.disconnect` | element_ids[] | — | Yes | STR-AM-006 |
| `analytical_model.adjust` | element_id | tool_session | Yes | STR-AM-008 |
| `analytical_model.reset` | element_ids[] | — | Yes | STR-AM-008 |
| `analytical_node.move` | node_ids[], delta, cs?: "global"\|"local" | — | Yes | STR-AM-008 |
| `analytical_model.check_supports` | scope? | report | No | STR-AM-007 |
| `analytical_model.check_consistency` | scope? | report | No | STR-AM-007 |
| `analytical_model.ignore_issue` | issue_key, ignore: bool, reason? | — | Yes | STR-AM-007 |
| `structural_analysis.run` | provider_id, scope, analysis_type, options | job_id | Results only | ANA-FEA-001 |
| `structural_analysis.export_model` | path, format: "ifc"\|"json", scope | — | No | ANA-FEA-001 |
| `structural_analysis.import_results` | path | package_id | Yes | ANA-FEA-001 |
| `structural_analysis.update_model` | package_id, apply_sections?: bool | — | Yes | ANA-FEA-001 |
| `results.show` | view_id, package_id, load_group_id, result_type, style_id? | — | No | STR-RES-001 |
| `results.apply_preset` | view_id, preset_id | — | No | STR-RES-001 |
| `results.extremes` | package_id, result_type, load_group_id | {min, max, element_ids} | No | STR-RES-001 |
| `results.delete_package` | package_id | — | Yes | STR-RES-001 |
| `steel_families.upgrade` | family_ids[] \| "all" | report | Yes | STR-STL-001 |
| `connection_library.install` | path | — | Yes | STR-STL-002 |
| `connection_type.edit_parameters` | type_id, params | — | Yes | STR-STL-002 |
| `structural_connection.override` | connection_id, params | — | Yes | STR-STL-002 |
| `structural_connection.customize` | connection_ids[], element_ids[], name | type_id | Yes | STR-STL-003 |
| `custom_connection.add_elements` | type_id, element_ids[] | — | Yes | STR-STL-003 |
| `custom_connection.remove_elements` | type_id, element_ids[] | — | Yes | STR-STL-003 |
| `custom_connection.expose_parameter` | type_id, path | — | Yes | STR-STL-003 |
| `structural_connection.automate` | scope, rule_set_id?, options?: {existing: "skip"\|"replace_generic", preview: bool} | report | Yes | STR-STL-004 |
| `structural_connection.apply_automation` | report_id | report | Yes | STR-STL-004 |
| `plate.create` | work_plane \| face_ref, contour_curves[], thickness, material_id, justification | id | Yes | STR-STL-005 |
| `bolt_pattern.create` | element_ids[], face_ref, pattern: "rect"\|"circle", params | id | Yes | STR-STL-006 |
| `anchor_pattern.create` | ... | — | Yes | STR-STL-006 |
| `hole_pattern.create` | ... | — | Yes | STR-STL-006 |
| `shear_stud_pattern.create` | ... | — | Yes | STR-STL-006 |
| `weld.create` | element_ids[], edge_ref, params | id | Yes | STR-STL-007 |
| `steel_modifier.corner_cut` | plate_id, corner_ref, kind, size | — | Yes | STR-STL-008 |
| `steel_modifier.cope_skewed` | member_id, end, params | — | Yes | STR-STL-008 |
| `steel_modifier.shorten` | member_id, end, length, angle_h?, angle_w? | — | Yes | STR-STL-008 |
| `steel_modifier.contour_cut` | element_id, face_ref, curves[], params | — | Yes | STR-STL-008 |
| `steel_cut.cope` | member_ids[], cut_member_id? | — | Yes | STR-STL-008 |
| `steel_cut.miter` | a, b | — | Yes | STR-STL-008 |
| `steel_cut.saw` | a, b, part: "flange"\|"web" | — | Yes | STR-STL-008 |
| `steel_cut.cut_through` | cut_id, cutter_id, gap | — | Yes | STR-STL-008 |
| `steel_cut.cut_by` | cut_id, cutter_id | — | Yes | STR-STL-008 |
| `steel_cut.uncope` | member_ids[] | — | Yes | STR-STL-008 |
| `structural_connection.check` | connection_ids[], provider_id, code | report | Results only | STR-STL-009 |
| `structural_connection.presize` | connection_ids[], provider_id, code | — | Yes | STR-STL-009 |
| `steel_numbering.run` | scope, options | report | Yes | STR-STL-010 |
| `steel_nc.export` | scope, folder, format="dstv" | — | No | STR-STL-010 |
| `precast_config.get` | path | value | No | STR-PRE-001 |
| `precast_config.set` | path, value | — | Yes | STR-PRE-001 |
| `precast_config.export` | file | — | No | STR-PRE-001 |
| `precast_config.import` | file | — | Yes | STR-PRE-001 |
| `precast_config.show` | — | — | No | STR-PRE-001 |
| `precast.split` | element_ids[], slab_kind? | assembly_ids[] | Yes | STR-PRE-002 |
| `precast.resplit` | assembly_ids[] | — | Yes | STR-PRE-002 |
| `precast.set_auto_update` | bool | — | Yes | STR-PRE-002 |
| `precast.reinforce` | assembly_ids[], area_type_id, edge_type_id? | report | Yes | STR-PRE-003 |
| `precast.generate_custom_fabric` | assembly_ids[], rule_id? | — | Yes | STR-PRE-004 |
| `precast.place_mounting_parts` | assembly_ids[] | report | Yes | STR-PRE-005 |
| `precast.shop_drawings` | assembly_ids[], template_id?, group_identical?: bool | sheet_ids[] | Yes | STR-PRE-006 |
| `precast.cam_export` | assembly_ids[], writer_id, folder, filename_rule? | files[] | No | STR-PRE-007 |
| `cam_writer.list` | — | writers[] | No | STR-PRE-007 |
| `connector.add` | family_id, domain, face_ref \| plane, params | connector_id | Yes | MEP-CONN-001 |
| `connector.set` | element_id, connector_index, params | — | Yes | MEP-CONN-001 |
| `connector.list` | element_id | connectors[] | No | MEP-CONN-001 |
| `mep.connect` | port_a, port_b | — | Yes | MEP-CONN-002 |
| `mep.disconnect` | port_a, port_b | — | Yes | MEP-CONN-002 |
| `mep.network_of` | element_id | element_ids[] | No | MEP-CONN-002 |
| `system_type.create` | classification, name, params | type_id | Yes | MEP-SYS-001 |
| `system_type.set` | type_id, params | — | Yes | MEP-SYS-001 |
| `system.create` | kind, element_ids[], system_type_id, name? | system_id | Yes | MEP-SYS-002 |
| `system.add` | system_id, element_ids[] | — | Yes | MEP-SYS-002 |
| `system.remove` | system_id, element_ids[] | — | Yes | MEP-SYS-002 |
| `system.select_equipment` | system_id, equipment_id | — | Yes | MEP-SYS-002 |
| `system.divide` | system_id | system_ids[] | Yes | MEP-SYS-002 |
| `system.disconnect_equipment` | system_id | — | Yes | MEP-SYS-002 |
| `system_browser.show` | view: "systems"\|"zones"\|"analytical", discipline? | — | No | MEP-SYS-003 |
| `mep_check.systems` | kind: "duct"\|"pipe"\|"circuit", view_id, on: bool | issues[] | No | MEP-SYS-005 |
| `mep_check.show_disconnects` | view_id, domains[], on: bool | ports[] | No | MEP-SYS-005 |
| `mep_settings.show` | kind: mechanical, electrical, conductor_cable, building_space_type, load_classification, demand_factor, fabrication or pid | — | No | MEP-SET-004 |
| `mechanical_settings.get` | path | value | No | MEP-SET-001 |
| `mechanical_settings.set` | path, value | — | Yes | MEP-SET-001 |
| `pipe_segment.create` | material_id, schedule, roughness, sizes[] | id | Yes | MEP-SET-001 |
| `fluid.create` | name, table[] | id | Yes | MEP-SET-001 |
| `duct_size_table.set` | shape, sizes[] | — | Yes | MEP-SET-001 |
| `electrical_settings.get` | path | value | No | MEP-SET-002 |
| `electrical_settings.set` | path, value | — | Yes | MEP-SET-002 |
| `distribution_system.create` | name, phase, config, wires, v_ll, v_lg | id | Yes | MEP-SET-002 |
| `voltage.create` | name, value, min, max | id | Yes | MEP-SET-002 |
| `wiring_type.create` | params | id | Yes | MEP-SET-002 |
| `conductor_size.create` | name, area_mm2 \| gauge, diameter | id | Yes | MEP-SET-003 |
| `cable_type.create` | name, material_id, insulation_id, phase_conductors: {count, size_id}, neutral?: size_id, ground?: size_id | id | Yes | MEP-SET-003 |
| `conductor_table.import` | path | — | Yes | MEP-SET-003 |
| `duct.create` | type_id, start, end, shape_size, middle_elevation, level_id, system_type_id?, justification? | element_ids[] | Yes | MEP-DUCT-001 |
| `duct.set_size` | ids[], size | — | Yes | MEP-DUCT-001 |
| `duct.cap_open_ends` | ids[] | — | Yes | MEP-DUCT-001 |
| `mep_run.create_riser` | element_id, connector_index, top_level, top_offset? | element_ids[] | Yes | MEP-DUCT-011 |
| `routing_preferences.get` | type_id | table | No | MEP-DUCT-002 |
| `routing_preferences.set` | type_id, table | — | Yes | MEP-DUCT-002 |
| `mep_run.reapply_type` | element_ids[] | — | Yes | MEP-DUCT-002 |
| `mep_run.change_type` | element_ids[], type_id | — | Yes | MEP-DUCT-002 |
| `duct_fitting.place` | type_id, host_duct_id \| point, params | id | Yes | MEP-DUCT-003 |
| `mep_fitting.upgrade` | id | — | Yes | MEP-DUCT-003 |
| `mep_fitting.downgrade` | id, leg | — | Yes | MEP-DUCT-003 |
| `mep_fitting.flip` | id | — | Yes | MEP-DUCT-003 |
| `mep_fitting.rotate` | id, angle | — | Yes | MEP-DUCT-003 |
| `duct_accessory.place` | type_id, duct_id, point | id | Yes | MEP-DUCT-004 |
| `mep_insulation.add` | element_ids[], type_id, thickness | — | Yes | MEP-DUCT-004 |
| `mep_insulation.remove` | element_ids[] | — | Yes | MEP-DUCT-004 |
| `mep_insulation.set` | element_ids[], type_id?, thickness? | — | Yes | MEP-DUCT-004 |
| `duct_lining.add` | element_ids[], type_id, thickness | — | Yes | MEP-DUCT-004 |
| `duct_lining.remove` | element_ids[] | — | Yes | MEP-DUCT-004 |
| `flex_duct.create` | type_id, points[], start_port?, end_port? | id | Yes | MEP-DUCT-005 |
| `duct.convert_to_flex` | terminal_ids[], max_length | ids[] | Yes | MEP-DUCT-005 |
| `mep_run.justify` | element_ids[], control_end, alignment: "TL"\|"TC"\|…\|"BR" \| line_ref | — | Yes | MEP-DUCT-006 |
| `duct_placeholder.create` | type_id, start, end, size, elevation, level | id | Yes | MEP-DUCT-007 |
| `pipe_placeholder.create` | type_id, start, end, diameter, middle_elevation, level_id, system_type_id?, slope? | id | Yes | MEP-DUCT-007 |
| `placeholder.convert` | ids[] | element_ids[] | Yes | MEP-DUCT-007 |
| `mep_layout.generate` | system_id, settings | solution_ids[] | Yes | MEP-DUCT-008 |
| `mep_layout.edit` | solution_id, moves[] | solution_id | No | MEP-DUCT-008 |
| `mep_layout.finish` | solution_id, as: "placeholder"\|"real" | element_ids[] | Yes | MEP-DUCT-008 |
| `air_terminal.place` | type_id, point \| duct_face_ref, level, flow? | id | Yes | MEP-DUCT-009 |
| `mech_equipment.place` | type_id, point, level, rotation | id | Yes | MEP-DUCT-009 |
| `mech_control_device.place` | type_id, point, level, host_face_ref? | id | Yes | MEP-DUCT-012 |
| `mech_control_device.assign` | device_id, controlled_ids[] | — | Yes | MEP-DUCT-012 |
| `equipment_set.create` | equipment_ids[], duty_ids[] | id | Yes | MEP-DUCT-009 |
| `mep.connect_into` | element_id, connector_index, target_id | element_ids[] | Yes | MEP-DUCT-009 |
| `mep_color_legend.place` | view_id, kind: "duct"\|"pipe", scheme_id, point | id | Yes | MEP-DUCT-010 |
| `mep.size` | element_ids[] \| system_id, method, targets: {friction?, velocity?}, combine, branch_rule, restrict?: {height?, width?, inner_diameter?} | report | Yes | MEP-CALC-001 |
| `mep_network.calculate` | system_id \| "all" | job_id | Results only | MEP-CALC-002 |
| `analytical_segment.list` | system_id | segments[] | No | MEP-CALC-002 |
| `analytical_segment.override` | segment_ref, params | — | Yes | MEP-CALC-002 |
| `system_inspector.start` | system_id, view_id | — | No | MEP-CALC-003 |
| `system_inspector.stop` | — | — | No | MEP-CALC-003 |
| `pressure_loss_report.export` | system_ids[], settings, path, format: "html"\|"csv" | — | No | MEP-CALC-004 |
| `sizing_calculator.candidates` | flow, basis, tolerance, roughness, constraints | sizes[] | No | MEP-CALC-005 |
| `pipe.set_size` | ids[], diameter | — | Yes | MEP-CALC-005 |
| `pipe.create` | type_id, start, end, diameter, middle_elevation, level_id, system_type_id?, slope? | element_ids[] | Yes | MEP-PIPE-001 |
| `mep_run.set_slope` | element_ids[], slope, control_point_ref | — | Yes | MEP-PIPE-002 |
| `pipe.create_parallel` | run_element_ids[], h_count, v_count, h_offset, v_offset | element_ids[] | Yes | MEP-PIPE-003 |
| `pipe_fitting.place` | type_id, host_pipe_id \| point, params | id | Yes | MEP-PIPE-004 |
| `pipe_accessory.place` | type_id, pipe_id, point | id | Yes | MEP-PIPE-004 |
| `mep_fitting.tee_to_cross` | id | — | Yes | MEP-PIPE-004 |
| `flex_pipe.create` | type_id, points[], start_port?, end_port? | id | Yes | MEP-PIPE-004 |
| `pipe.cap_open_ends` | ids[] | — | Yes | MEP-PIPE-004 |
| `fixture_unit_method.set` | method_id \| table_path | — | Yes | MEP-PIPE-005 |
| `plumbing_fixture.place` | type_id, host_face_ref \| work_plane, point, rotation | id | Yes | MEP-PIPE-006 |
| `sprinkler.place` | type_id, point \| host_face_ref, level | id | Yes | MEP-PIPE-007 |
| `hydronic.set_enabled` | bool | — | Yes | MEP-PIPE-008 |
| `hydraulic_separation.add` | fitting_or_pipe_id | — | Yes | MEP-PIPE-008 |
| `hydraulic_separation.remove` | id | — | Yes | MEP-PIPE-008 |
| `analytical_pipe_connection.create` | equipment_id, connector_index, pipe_id | id | Yes | MEP-PIPE-009 |
| `electrical_equipment.place` | type_id, point \| host_face_ref, level | id | Yes | MEP-ELEC-001 |
| `device.place` | type_id, host_face_ref \| point, level | id | Yes | MEP-ELEC-001 |
| `lighting_fixture.place` | type_id, host_face_ref \| point, level | id | Yes | MEP-ELEC-001 |
| `circuit.create` | kind, element_ids[], voltage?, poles? | circuit_id | Yes | MEP-ELEC-002 |
| `circuit.renumber` | panel_id, start? (renumbers the panel's circuit indexes and closes gaps) | () | Yes | MEP-ELEC-021 |
| `circuit.add` | circuit_id, element_ids[] | — | Yes | MEP-ELEC-003 |
| `circuit.remove` | circuit_id, element_ids[] | — | Yes | MEP-ELEC-003 |
| `circuit.select_panel` | circuit_id, panel_id | — | Yes | MEP-ELEC-003 |
| `circuit.disconnect_panel` | circuit_id | — | Yes | MEP-ELEC-003 |
| `circuit.compatible_panels` | circuit_id, search? | panels[] | No | MEP-ELEC-003 |
| `circuit.set_path` | circuit_id, mode, nodes?[], offset? | — | Yes | MEP-ELEC-004 |
| `circuit.reset_path` | circuit_id | — | Yes | MEP-ELEC-004 |
| `circuit.path` | circuit_id | {mode, nodes[], length} | No | MEP-ELEC-004 |
| `circuit.set` | circuit_id, {load_name?, rating?, cable_type?, runs?, cable_size?} | — | Yes | MEP-ELEC-005 |
| `circuit.size_cable` | circuit_id, max_vd_percent? | size | Yes | MEP-ELEC-005 |
| `panel.create_feeder` | child_panel_id, parent_panel_id, connection: "breaker"\|"lugs" | circuit_id | Yes | MEP-ELEC-006 |
| `panel.set_supply` | panel_id, source_id | — | Yes | MEP-ELEC-006 |
| `transformer.set_secondary` | transformer_id, distribution_system_id | — | Yes | MEP-ELEC-006 |
| `panel.hierarchy` | root_id? | tree | No | MEP-ELEC-006 |
| `wire.create` | view_id, kind: "arc"\|"spline"\|"chamfer", points[], connectors[], conductors?: {hot, neutral, ground} | id | Yes | MEP-ELEC-007 |
| `wire.set_conductors` | ids[], {hot?, neutral?, ground?} | — | Yes | MEP-ELEC-007 |
| `circuit.create_wiring` | circuit_id, view_id, kind: "arc"\|"chamfer" | ids[] | Yes | MEP-ELEC-007 |
| `switch_system.create` | fixture_ids[], switch_id? | id | Yes | MEP-ELEC-008 |
| `switch_system.select_switch` | id, switch_id | — | Yes | MEP-ELEC-008 |
| `switch_system.add` | id, fixture_ids[] | — | Yes | MEP-ELEC-008 |
| `switch_system.remove` | id, fixture_ids[] | — | Yes | MEP-ELEC-008 |
| `load_classification.create` | name, demand_factor_id | id | Yes | MEP-ELEC-009 |
| `load_classification.set` | id, {name?, demand_factor_id?, space_load?} | — | Yes | MEP-ELEC-009 |
| `load_classification.delete` | id, replace_with | — | Yes | MEP-ELEC-009 |
| `demand_factor.create` | name, method, ranges[], incremental?: bool | id | Yes | MEP-ELEC-010 |
| `demand_factor.evaluate` | id, loads[] | demand | No | MEP-ELEC-010 |
| `circuit_naming_scheme.create` | name, parts[] | id | Yes | MEP-ELEC-012 |
| `circuit_naming_scheme.set` | id, {name?, parts?[]} | — | Yes | MEP-ELEC-012 |
| `circuit_naming_scheme.delete` | id | — | Yes | MEP-ELEC-012 |
| `circuit_naming_scheme.preview` | parts[], sample? | strings[] | No | MEP-ELEC-012 |
| `panel.set_naming` | panel_id, scheme_id \| "project" | — | Yes | MEP-ELEC-012 |
| `panel_schedule.create` | panel_ids[], template_id? | view_ids[] | Yes | MEP-ELEC-013 |
| `panel_schedule_template.create` | kind, name, options | id | Yes | MEP-ELEC-014 |
| `panel_schedule_template.edit` | id, cell_map, formatting | — | Yes | MEP-ELEC-014 |
| `panel_schedule_template.manage` | — | — | No | MEP-ELEC-014 |
| `panel_schedule_template.apply` | template_id, schedule_view_ids[] | — | Yes | MEP-ELEC-014 |
| `panel_schedule_template.set_default` | template_id | — | Yes | MEP-ELEC-014 |
| `panel_schedule.move` | view_id, circuit_id, to_slot | — | Yes | MEP-ELEC-015 |
| `panel_schedule.assign_spare` | view_id, slot, rating? | — | Yes | MEP-ELEC-015 |
| `panel_schedule.assign_space` | view_id, slot | — | Yes | MEP-ELEC-015 |
| `panel_schedule.lock` | view_id, circuit_ids[], bool | — | Yes | MEP-ELEC-015 |
| `panel_schedule.rebalance` | view_id | {before, after} | Yes | MEP-ELEC-015 |
| `panel_schedule.update_names` | view_id, overwrite_edited? | — | Yes | MEP-ELEC-015 |
| `panel_schedule.move_across` | view_id, circuit_ids[] | — | Yes | MEP-ELEC-015 |
| `panel_schedule.remove_spare_space` | view_id, slots[] | — | Yes | MEP-ELEC-015 |
| `panel_schedule.group` | view_id, circuit_ids[], bool | — | Yes | MEP-ELEC-015 |
| `panel_schedule.change_template` | view_id, template_id | — | Yes | MEP-ELEC-015 |
| `panel_schedule.change_panel` | view_id, panel_id | — | Yes | MEP-ELEC-015 |
| `cable_tray.create` | type_id, start, end, width, height, elevation, level | element_ids[] | Yes | MEP-ELEC-016 |
| `cable_tray.split` | id, point | ids[] | Yes | MEP-ELEC-016 |
| `cable_tray_fitting.place` | type_id, host_id?, point?, params | id | Yes | MEP-ELEC-016 |
| `conduit.create` | type_id, start, end, diameter, elevation, level | element_ids[] | Yes | MEP-ELEC-017 |
| `conduit.create_parallel` | run_ids[], h_count, v_count, h_offset, v_offset, bend: "same"\|"concentric" | element_ids[] | Yes | MEP-ELEC-017 |
| `conduit_fitting.place` | type_id, host_id?, point?, params | id | Yes | MEP-ELEC-017 |
| `electrical_load_area.create` | boundary \| space_ids[], load_type_id | id | Yes | MEP-ELEC-019 |
| `electrical_load_area.divide` | id, line | ids[] | Yes | MEP-ELEC-019 |
| `equipment_load.create` | point \| space_id, classification, voltage, pf, apparent_load | id | Yes | MEP-ELEC-019 |
| `electrical_load_type.create` | name, classification, density | id | Yes | MEP-ELEC-019 |
| `load_set.create` | load_ids[] | id | Yes | MEP-ELEC-019 |
| `electrical_analytical.add_node` | kind, parent_id?, distribution_system_id? | id | Yes | MEP-ELEC-020 |
| `electrical_analytical.set_phases` | node_id, mapping | — | Yes | MEP-ELEC-020 |
| `electrical_analytical.rollup` | root_id | report | No | MEP-ELEC-020 |
| `fabrication_config.load` | path \| url, profile? | config_id | Yes | MEP-FAB-001 |
| `fabrication_config.reload` | — | — | Yes | MEP-FAB-001 |
| `fabrication_config.validate` | path | issues[] | No | MEP-FAB-001 |
| `fabrication_settings.set_services` | loaded_ids[] | — | Yes | MEP-FAB-002 |
| `fabrication_settings.set_profile` | profile_id | — | Yes | MEP-FAB-002 |
| `fabrication_settings.set_parts` | part_template_ids[] | — | Yes | MEP-FAB-002 |
| `fabrication_settings.set_indicators` | params | — | Yes | MEP-FAB-002 |
| `fabrication_part.update_out_of_step` | scope? | ids[] | Yes | MEP-FAB-002 |
| `fabrication_part.place` | service_id, button_id, point, size, elevation, rotation? | id | Yes | MEP-FAB-003 |
| `fabrication_part.edit` | id, dims?, components?, options? | — | Yes | MEP-FAB-003 |
| `fabrication_part.insert` | host_part_id, button_id, point | id | Yes | MEP-FAB-003 |
| `fabrication_route.multi_point` | service_id, points[], size, elevation, start_connector? | element_ids[] | Yes | MEP-FAB-004 |
| `fabrication_route.trim_extend` | conn_a, conn_b | element_ids[] | Yes | MEP-FAB-005 |
| `fabrication_route.quick_connect` | part_id, target_connector | element_ids[] | Yes | MEP-FAB-005 |
| `fabrication_route.route_and_fill` | conn_a, conn_b | solutions[] | Yes | MEP-FAB-005 |
| `fabrication_route.apply` | solution_id | element_ids[] | Yes | MEP-FAB-005 |
| `fabrication_autofill.exclude` | button_ids[], excluded: bool | — | Yes | MEP-FAB-005 |
| `fabrication.convert_design` | element_ids[], mapping_id? | report | Yes | MEP-FAB-006 |
| `fabrication_mapping.set` | rules[] | — | Yes | MEP-FAB-006 |
| `fabrication_mapping.save` | name, rules[], default?: bool | mapping_id | Yes | MEP-FAB-006 |
| `fabrication_mapping.preview` | element_ids[], mapping_id? | rows[] | No | MEP-FAB-006 |
| `fabrication_part.change_service` | ids[], service_id | — | Yes | MEP-FAB-007 |
| `fabrication_part.change_size` | ids[], size, resize_connected? | — | Yes | MEP-FAB-007 |
| `fabrication_spool.create` | part_ids[], name | id | Yes | MEP-FAB-007 |
| `fabrication_part.set_length` | id, length | — | Yes | MEP-FAB-007 |
| `fabrication_part.show_service` | view_id, part_id | — | No | MEP-FAB-007 |
| `fabrication_spool.add` | spool_id, part_ids[] | — | Yes | MEP-FAB-007 |
| `fabrication_spool.remove` | spool_id, part_ids[] | — | Yes | MEP-FAB-007 |
| `hanger.place` | part_id, position, template_id, attach_to_structure? | id | Yes | MEP-FAB-008 |
| `hanger.place_along` | run_ids[], spacing, template_id | ids[] | Yes | MEP-FAB-008 |
| `hanger.set` | ids[], {rod_length?, rod_diameter?, attach_to_structure?, flipped?} | — | Yes | MEP-FAB-008 |
| `fabrication.export_job` | scope, path, format: "ifc"\|"rofc-json" | — | No | MEP-FAB-009 |
| `fabrication.report` | scope, report_id | view_id \| file | No | MEP-FAB-009 |
| `fabrication.nest_linear` | scope, stock_length | report | No | MEP-FAB-009 |
| `fabrication.export_data` | scope, kind: "items"\|"ancillaries"\|"nesting", path, format: "csv"\|"json" | — | No | MEP-FAB-009 |
| `fabrication_part.number_items` | scope, start_part_id?, prefix?, start?, group_identical?: bool | count | Yes | MEP-FAB-010 |
| `space.place` | view_id, point, upper_limit?, offset? | id | Yes | MEP-SPACE-001 |
| `space.place_auto` | level_id | ids[] | Yes | MEP-SPACE-001 |
| `space_separator.create` | view_id, curves[] | ids[] | Yes | MEP-SPACE-001 |
| `space.unplace` | ids[] | — | Yes | MEP-SPACE-001 |
| `space.name_from_rooms` | scope, source_link_id? | — | Yes | MEP-SPACE-001 |
| `space.set` | ids[], params | — | Yes | MEP-SPACE-002 |
| `space_type.create` | name, params | id | Yes | MEP-SPACE-002 |
| `building_type.create` | name, params | id | Yes | MEP-SPACE-002 |
| `system_zone.create` | level_id, boundary \| "whole_level" | id | Yes | MEP-ZONE-001 |
| `system_zone.assign` | zone_id, equipment_or_system_id | — | Yes | MEP-ZONE-001 |
| `system_zone.convert_legacy` | zone_ids[] | ids[] | Yes | MEP-ZONE-001 |
| `energy_location.set` | lat, lon, elevation?, weather_file? | — | Yes | ANA-EM-001 |
| `weather.import` | path | id | Yes | ANA-EM-001 |
| `energy_settings.set` | path, value | — | Yes | ANA-EM-002 |
| `energy_settings.get` | — | table | No | ANA-EM-002 |
| `energy_settings.show` | — | — | No | ANA-EM-002 |
| `energy_model.create` | options? | job_id | Yes | ANA-EM-003 |
| `energy_model.delete` | — | — | Yes | ANA-EM-003 |
| `energy_model.update` | — | job_id | Yes | ANA-EM-003 |
| `energy_model.reassign_space` | analytical_space_id, system_zone_id | — | Yes | ANA-EM-003 |
| `gbxml.export` | path, source: "energy_model"\|"spaces" | — | No | ANA-EM-004 |
| `gbxml.import_loads` | path | report | Yes | ANA-EM-004 |
| `analytical_water_loop.create` | kind, params | id | Yes | ANA-SYS-001 |
| `analytical_air_system.create` | params | id | Yes | ANA-SYS-001 |
| `zone_equipment.create` | kind, behavior, loop_ids[], air_system_id? | id | Yes | ANA-SYS-001 |
| `systems_analysis.run` | workflow_id, report_name, folder, provider_id? | job_id | Results only | ANA-SIM-001 |
| `systems_analysis.report` | job_id | path | No | ANA-SIM-001 |
| `embodied_carbon.run` | scope?, options? | job_id | Results only | ANA-CARB-001 |
| `embodied_carbon.import_factors` | path, format: "ilcd"\|"csv" | library_id | Yes | ANA-CARB-001 |
| `embodied_carbon.match` | material_id, factor_ref | — | Yes | ANA-CARB-001 |
| `embodied_carbon.export` | job_id, path, format: "csv"\|"json"\|"html" | — | No | ANA-CARB-001 |
| `embodied_carbon.compare` | job_id_a, job_id_b | report | No | ANA-CARB-001 |
| `hc_loads.run` | options | job_id | Results only | ANA-HCL-001 |
| `hc_loads.reports` | — | report_ids[] | No | ANA-HCL-001 |
| `hc_loads.open` | report_id | — | No | ANA-HCL-001 |
| `lighting.calc_average` | space_ids[] \| "all" | results[] | Results only | ANA-LIGHT-001 |
| `daylight.run` | type, level_ids[], grid: {height, spacing}, thresholds, provider_id? | job_id | Results only | ANA-LIGHT-002 |
| `daylight.show` | view_id, job_id, style_id? | — | No | ANA-LIGHT-002 |
| `solar.run` | study_type, date_range, surfaces: "roofs"\|"masses"\|face_refs[], pv_settings? | job_id | Results only | ANA-SOLAR-001 |
| `solar.export` | job_id, path | — | No | ANA-SOLAR-001 |
| `analysis_style.create` | name, settings | id | Yes | ANA-ADS-001 |
| `analysis_result.publish` | view_id, element_or_face_refs[], values[], style_id | id | Yes | ANA-ADS-001 |
| `analysis_style.manage` | — | — | No | ANA-ADS-001 |
| `analysis_result.clear` | view_id, id? | — | Yes | ANA-ADS-001 |
| `path_of_travel.create` | view_id, start, end, waypoints?[] | id | Yes | ANA-ROUTE-001 |
| `path_of_travel.update` | ids[] \| "all_in_view" | — | Yes | ANA-ROUTE-001 |
| `path_of_travel.add_waypoint` | id, point, index? | — | Yes | ANA-ROUTE-001 |
| `route_settings.set` | params | — | Yes | ANA-ROUTE-002 |
| `route.reveal_obstacles` | view_id, on: bool | — | No | ANA-ROUTE-002 |
| `path_of_travel.create_multiple` | view_id, starts[], ends[], min_separation | ids[] | Yes | ANA-ROUTE-002 |
| `one_way_indicator.place` | view_id, point, direction | id | Yes | ANA-ROUTE-002 |
| `people_content.place` | view_id, type_id, point, rotation? | id | Yes | ANA-ROUTE-003 |
| `spatial_grid.create` | room_id, kind: "square"\|"hex", size | id | Yes | ANA-ROUTE-002 |
| `pid.open` | path | pid_id | No | MEP-PID-001 |
| `pid.list_lines` | pid_id, filter? | lines[] | No | MEP-PID-001 |
| `pid.start_pipe` | pid_id, line_id, segment_id | tool_session | Yes | MEP-PID-001 |
| `pid.map` | pid_item_id, element_ids[] | — | Yes | MEP-PID-001 |
| `pid.validate` | pid_id, line_ids?[] | report | No | MEP-PID-002 |
| `pid.ignore` | pid_id, item_id, ignored: bool | — | Yes | MEP-PID-002 |
| `pid_settings.set_mapping` | kind, rules[] | — | Yes | MEP-PID-002 |
| `pid_settings.auto_map` | kind | rules[] | No | MEP-PID-002 |
| `pid_settings.export_mapping` | path | — | No | MEP-PID-002 |
| `pid_settings.import_mapping` | path | — | Yes | MEP-PID-002 |

---

## 4.17 Open questions and deviations from the reference

### 4.17.1 Intentional deviations

| # | Area | Reference behaviour | Reviera behaviour | Reason |
|---|------|--------------------|-------------------|--------|
| D1 | UI | Options Bar holds size, elevation and placement options (2026) | No Options Bar. Everything is on the contextual ribbon and in Properties. | Follows the reference's own 2027 direction and Reviera's UI chapter. |
| D2 | Ribbon | Separate Precast tab (2026) | One **Concrete** tab holds reinforcement and precast | Follows the 2027 direction, with fewer tabs. |
| D3 | Structural solver | Proprietary solver link and proprietary exchange file | Provider interface with IFC Structural Analysis View and neutral JSON exchange. Open engines as plugins. | No proprietary dependency; open standards (§4.3.11). |
| D4 | Load combinations | Online generation that needs sign-in | Offline, rule-based generation from editable code templates | No cloud dependency, and works offline. |
| D5 | Analytical automation | Visual-programming packages | Lua rule packages, shipped under GPL | Lua is Reviera's scripting layer (TECH_STACK §3.5). |
| D6 | Steel connections | Proprietary connection engine and content in a local database server | Open parametric connection library (manifest + Lua generator), stored as files | Proprietary content cannot be used; a database server is unnecessary. |
| D7 | Steel interop | Proprietary exchange formats with detailing software | IFC as the primary exchange, plus an optional DSTV NC plugin | Open formats only. |
| D8 | Interference check | Excludes detailed steel elements and fabrication parts | **Includes** them | Reviera writes full geometry for both, so there is no technical reason to exclude them. This removes the need for an external tool. |
| D9 | MEP fabrication | Proprietary configuration database and part files; proprietary job export | **ROFC** open content model; open job export (IFC + JSON). Proprietary part files are **not** read or written. | Proprietary formats are out of scope (00_overview). |
| D10 | Duct fitting losses | Proprietary fitting-loss database | Loss data per fitting type, with open coefficient tables from public-domain sources and user-supplied tables | The proprietary database cannot be redistributed. |
| D11 | Fixture-unit conversion and ampacity | Ships code tables from specific editions | Built-in method from public-domain sources. Code tables are user-loaded content. | Code tables may be copyrighted. |
| D12 | Lighting and solar analysis | Cloud service with subscription | Local providers (Radiance suggested) | No vendor cloud (P3 policy). |
| D13 | Systems analysis | Built-in simulation integration | Same open engine (EnergyPlus) through OpenStudio, as an optional external provider | Keeps the core GPL-clean, and the engine can be swapped. |
| D14 | P&ID | Vendor cloud P&ID collaboration | Local DEXPI (open standard) or CSV line-list import | No vendor cloud; uses an open standard. |
| D15 | Precast CAM | Built-in vendor-specific writers | Neutral JSON/CSV export, plus format plugins where documentation allows | Depends on the licensing terms of each format's documentation. |
| D16 | Legacy zones | Hidden behind an ini switch | No legacy loads tool. Legacy zones on import convert to system-zones. | Simpler; matches the 2026/2027 direction. |
| D17 | Piles | Generic foundation families | Explicit pile and pile-cap support for clean `IfcPile` export | IFC compatibility. |
| D18 | Sprinkler hydraulics | Not supported | Optional hydraulic-calculation provider | Fills a common gap; optional. |
| D19 | Path of travel | Single level only | Same in the first release; multi-level is under consideration (Q9) | Scope control. |

### 4.17.2 Open questions

| # | Question | Options / notes |
|---|----------|-----------------|
| Q1 | ~~RXD content areas~~ | **Resolved**: `model/structural` (project document) and the `structural` element section are registered in 02a §6.4–§6.5. |
| Q2 | Analytical link representation. IFC4 has no dedicated rigid-link entity. | A `USERDEFINED` curve member (chosen here) or a connection condition on `IfcRelConnectsStructuralMember`. Check against buildingSMART structural MVD guidance. |
| Q3 | Many-to-many physical/analytical association | `IfcGroup` (chosen) versus several `IfcRelAssignsToProduct` relations. Check which form receiving tools understand. |
| Q4 | Coupler PredefinedType in IFC4 | `USERDEFINED` "COUPLER" in IFC4; native `COUPLER` in IFC4X3 (to verify). |
| Q5 | Electrical analytical nodes and loads | Geometry-less occurrences of real entity types (chosen) versus RXD-only nodes. The concern is other tools showing "phantom" equipment. |
| Q6 | Breakers per circuit | Model each breaker as `IfcProtectiveDevice` nested in the board (more faithful, many more entities) or keep breaker data on the circuit. |
| Q7 | Free-form longitudinal splicing | The reference extends this in 2027. Decide whether Reviera's first release includes it. *Deferred to the post-Alpha/POC review (owner, 2026-10-12).* |
| Q8 | Default heating/cooling loads engine | A native Rust RTS implementation (fully built in, more work) or EnergyPlus sizing runs (needs an external install). *Deferred to the post-Alpha/POC review (owner, 2026-10-12).* |
| Q9 | Multi-level path of travel through stairs and ramps | Needs a 3D navigation graph across levels; possibly P3. *Deferred to the post-Alpha/POC review (owner, 2026-10-12).* |
| Q10 | Obstacle-aware auto-routing for Generate Layout and Route and Fill | Leave to clash checking (current), or add a 3D A* router over an occupancy grid. *Deferred to the post-Alpha/POC review (owner, 2026-10-12).* |
| Q11 | Bundled engines | Which suggested engines, if any, ship with official installers. Each needs a licence review and a build for every target platform. *Deferred to the post-Alpha/POC review (owner, 2026-10-12).* |
| Q12 | ~~Cross-reference numbering~~ | **Resolved**: collaboration is Chapter 6 and its file is `06_collaboration_coordination.md`. |
| Q13 | Precast pieces and their source in IFC | **Deferred (owner, 2026-10-12): not implemented in the initial version.** IFC allows one decomposition parent, so a piece cannot be aggregated by both its source element and its piece assembly. The initial version writes only the assembly aggregation and keeps the source link in RXD (STR-PRE-002). A real decision needs advice from an IFC and precast domain expert (buildingSMART precast/structural guidance, and how receiving tools behave). Candidate to evaluate: `IfcRelAssignsToProduct` from source to pieces. |

---

## 4.18 Traceability

| Section of this chapter | Research sections used (`research/03_structure_mep.md`) | Screenshots (internal reference only) |
|-------------------------|-----------------------------------------------------------|----------------------------------------|
| 4.0 Principles, ribbon | §0, §1.1–1.6, §15.1–15.8 | — |
| 4.1 Structural physical elements | §2.1–2.9, §1.1, §1.6, §15.5–15.6; web: Analytical Model Settings (STR-SET-001), Configuring connection settings (course page, search summary only; STR-CONN-001 warnings toggle) | structure_mep/structural_connection_symbols.png |
| 4.2 Reinforcement | §3.1–3.8 | structure_mep/rebar_partitions_by_level.png, fabric_area_sheet_layout.png, rebar_3d_path_shape_distribution_2027.png, rebar_morphed_longitudinal_distribution_2027.png, rebar_couplers.png |
| 4.3 Analytical model, loads, analysis | §4.1–4.6, §13.7, §15.1; web: Analytical Model Settings, Modifying Elective Checking Criteria and Tolerances, Automatic Checks (STR-AM-007), Modify an Analytical Model (STR-AM-008) | structure_mep/analytical_automation_physical_to_analytical.png, load_cases_and_combinations_dialog.png, boundary_conditions_on_analytical_panel.png, structural_results_explorer_presets_2027.png, structural_results_analysis_insights_2027.png |
| 4.4 Steel fabrication | §5.1–5.5, §2.9; IFC4 ADD2 TC1 schema (profile definitions, `Pset_FastenerWeld`) | structure_mep/steel_custom_connections.png, steel_fabrication_elements_plate_anchors.png |
| 4.5 Concrete and precast | §6.1–6.4, §3.5 | structure_mep/precast_wall_auto_reinforcement.png, precast_shop_drawing.png |
| 4.6 MEP foundations | §7.1–7.3, §8.6, §1.6, §10.4, §12.1, §14 (settings dialogs, MEP-SET-004) | structure_mep/system_browser_analytical_systems.png, electrical_conductor_and_cable_settings.png, check_pipe_systems_warning.png |
| 4.7 HVAC | §8.1–8.6, §1.4, §7.2 (rise/drop), §9.1, §10.7; web: Specify Rise Drop Symbols for a Piping System, Specifying Rise Drop Symbols for a Duct System, Place Mechanical Control Device | structure_mep/analytical_duct_segments_tag.png, analytical_duct_segment_schedule_critical_path.png |
| 4.8 Piping, plumbing, fire protection | §9.1–9.4 | structure_mep/hydronic_network_primary_loop.png |
| 4.9 Electrical | §10.1–10.8, §7.3, §8.3 (fitting controls); web: Updating Circuit Names on a Panel | structure_mep/panel_schedule_modify_ribbon.png, panel_schedule_branch_two_column.png |
| 4.10 MEP fabrication | §11 | structure_mep/mep_fabrication_parts_model.png, fabrication_settings_dialog.png |
| 4.11 Spaces and system-zones | §12.1–12.2 | structure_mep/system_zones_plan.png |
| 4.12 Analysis | §13.1–13.7, §10.1, §12.1, §15.4, §0 (2027 carbon data); web: Settings Tab for Analysis Display Styles, Revit 2022 – Enhanced Feature – People flow Toolkit, ÖKOBAUDAT, ICE Database FAQs, Introducing Forma Carbon Insights, CarboLifeCalc | structure_mep/energy_model_analytical_spaces_3d.png, systems_analysis_loads_report.png, path_of_travel_with_schedule.png |
| 4.13 P&ID | §14 | — |
| 4.14–4.15 IFC and engine summaries | §15.8, plus the IFC4 ADD2 TC1 schema (buildingSMART, public) | — |
| 4.17 Deviations | §0 version map, §5.1, §11, §13.4, §13.6, §14 | — |

All reference behaviour was taken from the public help pages listed in `research/03_structure_mep.md` §Sources, and the descriptions here are written independently. No help text, tables, icons or content files from the reference product were reproduced.
