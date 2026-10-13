# Reviera Application Specification

**Document:** Functional and technical specification of Reviera, an open-source BIM authoring application
**Functional reference:** Autodesk® Revit® 2026/2027 (described from public sources only; see `RESEARCH_LOG.md`)
**Licence of Reviera:** GPL-3.0-or-later
**Version:** 0.1 (draft) · 2026-10-12 · **Size:** 751 numbered requirements (317 P0 · 261 P1 · 168 P2 · 5 P3), 1,886 commands

> **Clean-room statement.** This specification describes features, workflows and behaviour in Reviera's own words. Every input was publicly available: vendor online help, public blogs and tutorials, public forums and open-source repositories. All of it is listed with URLs and access checks in `SOURCE_REGISTER.md`. No proprietary, confidential, leaked or NDA-covered information was used. No vendor code, binaries, file formats or assets were examined or copied. Autodesk and Revit are trademarks of Autodesk, Inc. They are used here only to identify the functional reference. Reviera is not affiliated with or endorsed by Autodesk.

---

## 0.1 Purpose and Scope

Reviera is a parametric Building Information Modeling (BIM) authoring application for architecture, structural and MEP design, documentation and coordination. Its goal is functional parity with the main workflows of the reference product. It is built on open standards and open-source components:

- **Open data:** IFC is the native file format, with no proprietary container.
- **Open collaboration:** it can run as a client of the Open Source BIMserver.
- **Open automation:** every operation can be driven from Lua scripts and by AI agents through the Model Context Protocol (MCP).
- **Open licence:** GPL-3.0-or-later.

This document covers:

| Chapter | Content |
|---------|---------|
| 0 | Overview, architecture, glossary, requirement conventions, roadmap (this chapter) |
| 1 | User interface shell |
| 2 | Data model, families, parameters, project setup, file I/O, extensibility (Lua, add-ins, MCP) |
| 2a | Reviera Extension Data (RXD): bespoke format and IFC embedding |
| 2b | Live model store (SQLite) and crash recovery |
| 3 | Architectural modeling and editing tools |
| 4 | Structure, steel, concrete/precast, MEP systems, analysis |
| 5 | Views, graphics, annotation, schedules, sheets, output |
| 6 | Collaboration, linking and coordination (Local and BIMserver modes) |
| A | Appendix A: requirement summary, shared commands, consolidated command index (1,886 commands; 751 requirements) |

Out of scope:
- reproducing vendor-cloud services such as ACC/Forma and Autodesk Assistant (open alternatives are specified instead)
- reading or writing the reference product's proprietary `.rvt` and `.rfa` formats
- vendor content libraries

## 0.2 Product Overview

### 0.2.1 Users

| Persona | Primary needs |
|---------|---------------|
| Architect / designer | Model building elements quickly; produce plans, sections, elevations, 3D views and sheets |
| BIM coordinator | Shared models, linking, clash detection, IFC exchange, standards (templates, parameters) |
| Structural engineer | Physical structural model, analytical model, loads, rebar documentation |
| MEP engineer | Duct, pipe and electrical systems, sizing, circuits, panel schedules, spaces and loads |
| Computational designer / developer | Lua scripting, add-ins, MCP automation, data extraction |
| AI agent (non-human) | Complete, typed, permissioned access to model queries and edits through MCP |

### 0.2.2 Product principles
1. **Single source of truth.** All drawings, schedules and 3D views are live views of one model. Changing an element anywhere updates it everywhere.
2. **Parametric relationships.** Hosting, joins, constraints, formulas and datums propagate changes automatically.
3. **Open by construction, compatibility first.** A Reviera project is a single, schema-valid IFC file. Information IFC can't express (parametric intent, drawings, standards) is stored as Reviera Extension Data (RXD), a published JSON format embedded as standard `Reviera_*` property sets. Other IFC tools keep it or safely ignore it. A clean-IFC export and an external sidecar carrier are also available (Chapter 2a).
4. **Everything is a command.** Each operation is a typed, undoable command, available in the same way to the UI, Lua and MCP.
5. **Multi-core first.** Regeneration, tessellation, hidden-line processing, schedules and export use all available cores.
6. **Familiar workflows, own expression.** Experienced BIM users should find the workflow concepts familiar. Reviera's naming, visual design, icons and defaults are its own.

## 0.3 System Architecture Summary

The full rationale is in `TECH_STACK.md`.

```
┌──────────────────────────── wxWidgets UI (main thread) ─────────────────────────────┐
│ Ribbon · QAT · Properties · Project Browser · Views (GPU viewport) · Dialogs · AI pane│
└──────────────┬──────────────────────────────────────────────────────────────────────┘
               │  invoke(command, params)          ▲ events (selection, model changed)
┌──────────────▼──────────────────────────────────────────────────────────────────────┐
│                               Rust core (multi-core)                                 │
│  Command Registry ─ Transaction Manager/Undo ─ Element DB ─ Regeneration Graph       │
│        ▲        ▲                     │ ChangeSets ─► Live store (SQLite WAL, writer thread)│
│   LuaJIT VM   MCP server (rmcp)       ▼                        ▼                     │
│   scripts,    tools = commands   IfcOpenShell            OpenCASCADE                 │
│   add-ins     resources = model  (IFC schema, I/O)       (B-rep, booleans, HLR)      │
│                                       │                                              │
│                         ModelStore trait                                             │
│                ┌──────────────────────┴─────────────────────┐                        │
│        LocalStore (.ifc/.ifczip)              BimServerStore (JSON API, notifications)│
└──────────────────────────────────────────────────────────────────────────────────────┘
```

### 0.3.1 Command model (applies to every chapter)
- A command has a stable dotted name (`wall.create`, `view.duplicate`, `bimserver.checkin`). It also has a parameter schema (JSON Schema), a result schema, an **undoable** flag, a **permission class** (`read`, `edit`, `script`, `admin`), and UI metadata (label, icon id, ribbon location, shortcut).
- Every model-changing command runs inside a transaction. Interactive tools, such as drawing a wall chain, gather input and then issue one or more commands. A whole tool session can be grouped into one undo step.
- Lua sees commands as `reviera.<domain>.<verb>{...}`. MCP sees each command as a tool named `<domain>_<verb>`. Read-only model access is provided as Lua query APIs and MCP resources (`reviera://element/{guid}`, `reviera://view/{id}`, `reviera://schedule/{id}`).
- Each chapter ends with a **command catalog**. Appendix A merges them into one index.

### 0.3.2 Operating modes
- **Local:** the project is an `.ifc` or `.ifczip` file on disk, and one user edits it.
- **BIMserver client:** the project is an Open Source BIMserver project. Reviera keeps an offline working copy, checks in revisions, gets the latest revision, merges changes by GlobalId and shows notifications. Chapter 6 maps the reference product's worksharing concepts (central/local, worksets, element borrowing, synchronise) onto this mode.

### 0.3.3 Persistence summary

| Reference concept | Reviera persistence |
|-------------------|---------------------|
| Project file | IFC4 (ADD2 TC1) `.ifc` / `.ifczip`; IFC4X3 optional |
| Unsaved working state | Live SQLite store `*.rvlive` in the per-user data directory, updated per transaction (Chapter 2b); local only, never exchanged |
| Family file | IFC library file (`IfcProjectLibrary` + type objects) with RXD `families/<id>` embedded |
| Project template | IFC file marked as a template (`Reviera_ProjectTemplate`) |
| Categories | IFC entity class + PredefinedType (+ `Reviera_Category` where finer granularity is needed) |
| Types / instances | `IfcTypeObject` subtypes / occurrence entities |
| Parameter values | Readable Psets/Qtos (standard `Pset_*` where defined, otherwise project-named Psets) |
| Parameter definitions (formulas, groups, shared ids) | RXD document `standards/parameters` |
| Element recipes, sketches, owned constraints | RXD element data in the `Reviera_Element` Pset on the element |
| Cross-element constraints | RXD document `model/constraints` |
| Views, sheets, schedules, view templates, annotation | RXD documents (`Reviera_Doc:*` Psets on IfcProject); optional `IfcAnnotation` export |
| Worksets / phases / design options | RXD documents and element membership (optional `IfcGroup` export) |
| Live ownership / locks (BIMserver) | BIMserver Extended Data, not in the model file |

## 0.4 Glossary

| Term | Meaning in Reviera |
|------|--------------------|
| **Element** | Any persistent object: a model element, datum, view, annotation, type or setting |
| **Category** | A classification of elements (Walls, Doors, …) that controls visibility, graphics and scheduling |
| **Family** | A named definition of a parametric object, with one or more types. **System families** (walls, floors and similar) are built into Reviera. **Loadable families** come from IFC library files. **In-place families** are one-off, project-specific definitions. |
| **Type** | A named set of parameter values shared by many instances (`IfcTypeObject`) |
| **Instance** | One placed occurrence of a type |
| **Host** | An element that another depends on and is cut by, such as a wall hosting a door |
| **Datum** | A reference element: level, grid, reference plane or scope box |
| **View** | A live graphical or tabular representation of the model: a plan, section, 3D view, schedule and so on |
| **View template** | A named, reusable set of view settings |
| **Sheet** | A printable layout holding views and schedules inside a title block |
| **Sketch mode** | An editing mode for drawing 2D boundary or path curves that define an element (floor, roof, stair) |
| **Regeneration** | Recomputing dependent elements and geometry after a change |
| **Transaction** | An atomic, undoable unit of model change |
| **Command** | A registered, typed operation, available through the UI, Lua and MCP |
| **Working copy** | The local copy of a BIMserver project being edited |
| **Check in / Get latest / Sync** | Upload a revision / download the newest revision / do both, with a merge |
| **Lua add-in** | A packaged Lua extension with a manifest that declares a GPL-compatible SPDX licence |
| **MCP** | Model Context Protocol: the interface through which AI agents call Reviera commands |

## 0.5 Requirement Conventions

The full rules are in `spec/00_conventions.md`.
- **IDs** have the form `<AREA>-<FEATURE>-<NNN>`. Area codes: `UI`, `DM`, `ARC`, `STR`, `MEP`, `ANA`, `DOC`, `COL`.
- **Priorities:**
  - **P0**: MVP, a single-user architectural authoring tool
  - **P1**: v1.0, complete multi-discipline authoring with collaboration
  - **P2**: later, advanced discipline and analysis features
  - **P3**: deferred or not applicable
- Each requirement uses **"shall"** language and has **acceptance criteria** written as testable check-boxes.
- Each feature has a **Ref:** line pointing to the research section (and screenshot) it came from, so it can be traced back to the public source register.

## 0.6 Delivery Roadmap (indicative)

| Milestone | Scope | Key chapters |
|-----------|-------|--------------|
| **M0 — Foundations** | Rust core, command registry, transactions/undo, live SQLite store and crash recovery, IFC load/save (IfcOpenShell), OCCT geometry pipeline, GPU viewport in a wx frame, Lua host, MCP server skeleton | 0, 1 (shell basics), 2 |
| **M1 — MVP (P0)** | Levels, grids, walls (compound layers, joins), doors and windows, floors, roofs, ceilings, basic stairs and railings, rooms; plan, section, elevation and 3D views with view range; dimensions, tags and text; schedules; sheets with title blocks; PDF export; Local mode | 1, 2, 3, 5 |
| **M2 — v1.0 (P1)** | Family editor, curtain walls, view templates and filters, phasing, design options, groups, BIMserver client mode, links and coordinates, interference check and BCF, structural physical elements, basic duct and pipe routing | 2, 3, 4, 5, 6 |
| **M3 — Discipline depth (P2)** | Rebar, analytical model and loads, steel connections, electrical circuits and panel schedules, MEP fabrication content, energy and loads analysis through open engines, rendering | 4, 5 |

## 0.7 Non-functional Requirements

| ID | Requirement | Target |
|----|-------------|--------|
| NFR-PERF-001 | Opening a 200 MB IFC4 model on an 8-core workstation | ≤ 60 s to first interactive view |
| NFR-PERF-002 | Regenerating a single wall edit with 50 dependents (joins, hosted doors, room boundaries) | ≤ 150 ms |
| NFR-PERF-003 | 3D viewport navigation on a 1 M-triangle model | ≥ 30 fps on a mid-range GPU |
| NFR-PERF-004 | Parallel scaling of full regeneration and export | ≥ 5× speed-up on 8 cores versus 1 |
| NFR-REL-001 | Crash safety | Live SQLite store updated after every committed transaction (Chapter 2b). An application crash loses no more than 250 ms of committed work, and power loss no more than about 1 s. File writes are atomic. |
| NFR-REL-002 | Round-trip fidelity | Saving an untouched IFC file loses no entities or properties (ids preserved) |
| NFR-PORT-001 | Platforms | Windows 10 and later (x64); macOS 10.12 and later on Intel, macOS 11 and later on Apple silicon (universal binary); current Linux distributions (x64, X11/Wayland via GTK), shipped as a Flatpak that bundles its dependencies (TECH_STACK §5.4) |
| NFR-ACC-001 | Accessibility | Full keyboard operation; screen-reader labels on all controls; HiDPI scaling at 100–300 % |
| NFR-I18N-001 | Localisation | All UI strings externalised (gettext); metric and imperial units; RTL-safe layouts |
| NFR-SEC-001 | Scripting and MCP safety | Permission classes enforced; Lua sandbox levels; MCP over HTTP requires authentication; credentials never exposed as MCP resources |
| NFR-LIC-001 | Licence compliance | SPDX header in every source file; CI licence allow-list for dependencies; add-in loader enforces a GPL-compatible SPDX licence by default |
| NFR-CR-001 | Clean-room integrity | No vendor assets, text, or file contents in the repository; contributions certify public-source provenance (DCO plus a provenance statement) |
