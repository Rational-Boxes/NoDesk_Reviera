# Features

[← Home](index.md)

Reviera is in early development. This page lists what works today. The
[README](https://github.com/Rational-Boxes/NoDesk_Reviera#not-built-yet) lists what is still to
come.

## Native IFC

- Open, edit and save **IFC2X3, IFC4 and IFC4X3** models through IfcOpenShell and OpenCASCADE.
- Reviera's own elements are **standard IFC entities** with their relationships, so other tools
  see real walls, slabs, beams and plates, not proxies.
- Elements Reviera doesn't model itself are kept, drawn and selectable, so files from other
  tools round-trip losslessly.
- **Crash recovery:** every change is mirrored to an encrypted local store, so unsaved work and its
  undo history come back after a crash.
- **Units:** metric and imperial display units, including feet and inches. Every length field
  accepts typed units such as `4.5 m` or `12' 6"`.

## Modelling

- Straight and curved **walls**, with layered assemblies, joins and wall openings.
- **Doors and windows**, including shaped windows, and curtain walls with grids, panels and
  mullions.
- **Floors, ceilings and decks**, with deck boards and railings.
- **Roofs:** hip, gable, arched and domed, with eaves, dormers and skylights.
- **Stairs**, including spiral stairs. Stairs can extend to the next level and offer slab
  openings with code headroom. **Railings** have gates and follow their host.
- **Rooms, areas, levels, grids** and reference planes.
- A **building code** setting (IBC, IRC, Approved Document K, NBC Canada, NCC Australia,
  DIN 18065 or custom) that tools follow for headroom, clearances and defaults.

## Components and libraries

- **Component Libraries** of ordinary 3D files (OBJ, STL, PLY, glTF) and IFC components.
- Place them like stamps, make new ones from any selection, and repeat them along paths with
  parametric **path arrays**.

## Coordination

- **Overlap detection** with resolve modes: cut one element by another, or accept the overlap
  as intended. Results are kept as booleans.
- **Clash detection** that runs out of process, with a pane to review, frame and resolve
  each clash.
- **Door clearance** checks against accessibility rule sets.
- **BCF** viewpoints, including section boxes and slice planes.

## Structure

- **Columns, beams, braces and trusses** from AISC, European (IPE, HEA, HEB) and UK (UB, UC) steel
  catalogues, and timber and North American lumber sections. Usage is detected from supports (girder, joist, purlin).
- **Truss profiles:** flat, gable, mono, scissor, bowstring and custom. Layouts include Fink,
  King Post and Queen Post. Trusses can attach to a roof.
- **Foundations:** isolated footings under columns, wall foundations, slabs, piles and pile caps.
- **Steel connections** from a built-in library: shear plates, extended end plates, base plates
  with anchor rods, and brace gussets.
  - Their plates, bolts and welds are real IFC elements, drawn at Fine detail.
  - Parameters can be set per project or per connection.
- **Connection automation:** place connections on every join at once from a rule set. A 3D
  preview is colour-coded by type, and a log can be saved as CSV.
- **Analytical model:** members, nodes and panels made from the physical model, with
  supports, releases, Adjust and Reset. Consistency and support checks flag problems before
  you analyse.
- **Loads:** load cases, point, line and area loads, and ASCE 7 combination generation.
- **Built-in linear static solver.** The Results Explorer draws force diagrams and deformed
  shapes, lists the largest values, and saves presets and result packages.

## Documentation

- **Plans, ceiling plans, sections and elevations**, drawn from the model with cut and
  projection graphics.
- **Schedules and sheets**, exported to **PDF**.
- **COBie** export for facility handover.
- **DWG and DXF** exchange through open-source libraries.

## Scripting and AI agents

- **One command registry:** every tool is a command with a JSON Schema, a description and a
  permission level (read only, model edit, full scripting).
- **Lua scripting** (LuaJIT): call any command, ask the user for input with forms, and
  package scripts as **plugins**. A script run is one undo step and rolls back if it fails.
- **MCP server** for AI agents. Every command is a tool, grouped into toolsets that agents enable as
  they need them. Agents get preview and confirm for destructive changes, and the same
  transactions and permission checks as the user.
- A **command shell** (`reviera shell`) for running commands headlessly.

## Platforms

- **Linux:** Flatpak, built and tested on Fedora 43.
- **Windows:** installer and portable ZIP, tested on Windows 11.
- **macOS:** an `.app` in a `.dmg` is written but not yet tested on a Mac.

---

[See it in action →](screenshots.md) · [Get started →](get-started.md)
