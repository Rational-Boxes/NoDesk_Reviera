# Specification Conventions (for chapter authors)

These rules apply to every chapter of the Reviera Application Specification.

## 1. Clean-room writing rules (mandatory)
1. **Use your own words.** Describe behaviour, workflows and requirements. Do **not** copy sentences, tables or help text from the research files' quotations of Autodesk material. A short quoted term or UI label (for example a dialog name like "View Range") is fine. Paragraphs are not.
2. **Treat Autodesk features as the functional reference, not as wording or design to copy.** Write requirements as "Reviera shall…".
3. **Do not reproduce any of these:** icons, artwork, Autodesk file contents (for example `KeyboardShortcuts.xml`, shared-parameter file headers, template content), or code from the Autodesk API.
4. **Name things neutrally.** Generic industry terms (wall, level, grid, section, schedule, sheet, family/type) are fine. Do not use Autodesk product or brand names as Reviera names: say "Reviera" instead of "Revit", "Reviera Home" instead of "Revit Home". Name the reference product only in the "Reference" lines.
5. **Every feature cites the research section it came from** (for example `Ref: research/02 §4.3`), so it can be traced through `RESEARCH_LOG.md`. A UI feature also cites the relevant screenshot path (for example `screenshots/ui/properties_palette_annotated.png`). Screenshots are **internal reference only**; never embed them as Reviera assets.

## 2. Target technology (see `TECH_STACK.md`)
- **Data and storage.** Native IFC through IfcOpenShell. The default project schema is IFC4 (ADD2 TC1); IFC4X3 is optional. Anything IFC can't express is **Reviera Extension Data (RXD)**: refer to it as "RXD document `<name>`" or "RXD element data (`<kind>`)". The storage carrier (embedded `Reviera_*` Psets by default, or an external sidecar) is defined only in `spec/02a_extension_data_format.md`.
- **Application and modes.** The core is written in Rust and uses multiple cores. Geometry uses OpenCASCADE, the UI uses wxWidgets, and scripting uses LuaJIT.
- **Shared commands.** Every operation is a command in a single registry. That registry automatically exposes it to the UI, to Lua and to an MCP server for AI agents.
  - **High-level tools too.** Automation tools (stairs, roof shapes, gable infill, dormers, tag-all, footprint edits and the like) must be equally available to Lua and MCP. Their logic is a registered command, and the window only gathers input and calls it. A behaviour that exists only in the window is a defect.
  - **Discoverability.** Every command domain belongs to an MCP toolset (DM-MCP-003).
  - *(Owner clarification, 2026-10-13.)*
- **Operating modes.** **Local** (an `.ifc` file) and **BIMserver client** (Open Source BIMserver over its JSON API).
- **Licence.** GPL-3.0-or-later. Distributed add-ins must be GPL-compatible.

## 2.1 Rule for interactive tools
Every multi-step interactive tool (sketch modes, multi-click placement, group or in-place editing) shall declare a **micro-operation schema and a replay function**, so that an unfinished session can be recovered after a crash (Chapter 2b §6.5, DM-LIVE-015).

## 3. Requirement IDs
Format: `<AREA>-<FEATURE>-<NNN>`. Example: `ARC-WALL-001`.

| Area code | Chapter |
|-----------|---------|
| UI | User interface shell |
| DM | Data model, families, parameters, project setup, file/IO, extensibility |
| ARC | Architectural modeling and modify tools |
| STR | Structure, steel, concrete/precast |
| MEP | Mechanical, electrical, plumbing, fabrication |
| ANA | Analysis |
| DOC | Views, graphics, annotation, schedules, sheets, output |
| COL | Collaboration, linking, coordination, BIMserver |
| ADV | Advanced features beyond the reference product (Chapter 7) |

## 4. Priority tiers

| Tier | Meaning |
|------|---------|
| **P0 — MVP** | Needed for a usable single-user architectural authoring tool: shell, datums, walls/doors/windows/floors/roofs/ceilings/stairs (basic), rooms, plan/section/elevation/3D views, basic annotation, schedules, sheets, PDF output, IFC local mode, Lua, MCP. |
| **P1 — v1.0** | Families editor, curtain walls, full annotation, view templates and filters, BIMserver mode, structural physical elements, basic MEP routing, links, phasing, design options. |
| **P2 — Later** | Rebar, steel fabrication, precast, MEP fabrication, electrical circuiting and panel schedules, energy/loads analysis, rendering, P&ID. |
| **P3 — Out of scope / defer** | Vendor-cloud-specific features (ACC/Forma, Autodesk Assistant) and anything else not applicable. Note an open alternative where one exists. |

## 5. Feature block template
Use this structure for each feature (heading level 3 or 4):

```markdown
### <Feature name>
**ID:** ARC-WALL-001 · **Priority:** P0 · **Ref:** research/02 §4.1–4.4 · screenshots/architecture/wall_edit_assembly_layers_2026.png

**Purpose.** One or two sentences.

**Workflow.**
1. …
2. …

**UI.** Ribbon tab › panel › tool; default shortcut; which properties appear; contextual tab contents; sketch mode, if any.

**Behaviour & rules.** Bullets covering constraints, joins, edge cases and regeneration effects.

**Data (IFC mapping).** Entity, type and PredefinedType; relationships; Psets/Qtos; RXD element data or documents.

**Commands.** `domain.verb` names with their key parameters, for example
`wall.create(type_id, curve, base_level, top_constraint, base_offset, top_offset, location_line, flip?) -> element_id`.
These are exposed to Lua and MCP automatically.

**Acceptance criteria.**
- [ ] Testable statement…
```

Smaller features can be grouped into a table: `ID | Feature | Priority | Description | Commands`.

## 6. Chapter ending
Every chapter ends with these three sections:
1. **Command catalog**: a table of every command defined in the chapter: `Command | Parameters | Returns | Undoable | Req ID`.
2. **Open questions / deviations from reference**: places where Reviera intentionally differs from the reference product, and why. Example: no Options Bar, matching the reference's 2027 direction; contextual ribbon plus Properties palette instead.
3. **Traceability**: a list of the research sections used.
