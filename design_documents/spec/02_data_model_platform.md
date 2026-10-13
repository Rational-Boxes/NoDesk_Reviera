# Chapter 2 — Data Model, Families, Parameters, Project Setup, File I/O and Extensibility

**Area code:** DM · **Status:** Draft 1 (2026-10-12) · **Primary source:** `research/05_data_model_platform.md` · **Architecture source:** `TECH_STACK.md` · **Extension data carrier:** `spec/02a_extension_data_format.md`

This chapter is the backbone of the Reviera specification. It defines what a Reviera model *is*, how it is stored, how it changes, how users and programs extend it, and how it moves in and out of the application. Other chapters (ARC, STR, MEP, ANA, DOC, COL) define particular element kinds and tools. They build on the element model, parameter system, transaction model, storage split and command registry defined here. Where this chapter and a domain chapter disagree about storage or identity, this chapter wins. Where this chapter and `02a_extension_data_format.md` disagree about *how* extension data is physically carried, `02a` wins.

> **Storage design (owner decision, 2026-10-12; compatibility first).** A Reviera project is a **single valid IFC file** (`.ifc` or `.ifczip`).
> 1. **Standard IFC** holds everything IFC can express: spatial structure, elements, types, geometry, materials and layer sets, standard `Pset_*`/`Qto_*`, the *values* of user parameters (as property sets that other tools can read), classifications, grids, openings and voids.
> 2. **Reviera Extension Data (RXD)** holds everything IFC cannot express: constraints, sketches, family parametric definitions, parameter definition metadata, views, sheets, schedules, view-specific annotation, phases, design options, worksets, revisions, links, styles, patterns and settings. RXD has two forms:
>    - **RXD element data**: data scoped to one IFC element (its recipe, sketch, constraints, hosting detail and memberships). By default it is carried in a `Reviera_Element` property set on that element.
>    - **RXD documents**: project-scoped documents (for example `views`, `sheets`, `annotation`, `styles`, `families/<guid>`). By default they are carried in `Reviera_Document_*` property sets on `IfcProject`.
> 3. An external **`.reviera` sidecar** is an *alternative carrier* for the same RXD. It is useful for git, or when a downstream tool strips unknown property sets.
>
> How RXD is encoded, chunked, compressed, versioned and validated, and how the sidecar carrier works, is defined in **`spec/02a_extension_data_format.md`**. This chapter defines **what** RXD holds, **which** element or document it belongs to, and how it refers to IFC. Document and key names used here are proposals for `02a`; if `02a` renames one, `02a` wins.

---

## 2.0 Reading guide

### 2.0.1 Scope

| In scope | Defined elsewhere |
|---|---|
| Element model (category, family, type, instance) and its IFC4 mapping | Behaviour of particular elements, such as walls or ducts (ARC, STR, MEP) |
| The split between standard IFC and RXD, and the RXD contents for this chapter's data | How RXD is carried, encoded and validated, and the sidecar carrier (`02a`) |
| Identity, hosting, dependency graph, regeneration, transactions, failures | View graphics, annotation behaviour, schedules and sheets (DOC). This chapter only states where they are *stored*. |
| Family editor, family file format, the conceptual massing environment | Worksharing, sync and BIMserver protocol details (COL) |
| Parameters, formulas, units, the shared parameter library | Ribbon layout and palette behaviour in general (UI) |
| Project setup (Manage features), materials | Analysis engines (ANA) |
| File I/O, schema policy, import/export, migration | |
| Command registry, Lua API, add-ins, visual programming, MCP server | |

### 2.0.2 Notation

- **Requirement keywords.** "Shall" is mandatory. "Should" is recommended. "May" is optional.
- **Lua examples** use the API surface defined in §2.13. **JSON examples** are informative; the normative JSON Schemas are published with `02a`.
- **IFC entity names** follow IFC4 ADD2 TC1. Where IFC4X3 differs (for example `IfcBuildingElementType` became `IfcBuiltElementType`), the difference is noted.
- **"RXD document `x`"** means the data is stored in that RXD document. By default it is embedded in the IFC file as a `Reviera_Doc:<path>` property set; the external `.reviera` container is the alternative carrier (see `02a`). Document names given here are proposals for `02a`; if `02a` renames a document, `02a` wins.
- **Grammar** is written in EBNF (ISO/IEC 14977 style, with `|`, `[ ]`, `{ }`).
- **Internal units** are SI (§2.9). Any number in this chapter without a unit is in internal units.

### 2.0.3 Glossary

| Term | Meaning in Reviera |
|---|---|
| **Element** | Anything with identity in the model: physical objects, datums, annotation, views, sheets, types, settings. An element is an IFC entity, an RXD-only record, or an IFC entity with RXD element data. |
| **Category** | A fixed classification defined by the application. It sets default graphics, behaviour flags and the IFC entity class. |
| **Family** | A named definition that shares parameters, geometry logic and representation across a set of types. |
| **Type** | A named set of type-parameter values inside a family. It is stored as an `IfcTypeObject` subtype. |
| **Instance** | A placed occurrence of a type. It is stored as an IFC occurrence entity (`IfcWall`, `IfcDoor`, …). |
| **IFC-backed element** | An element whose primary record is an `IfcRoot` entity in the `.ifc` file (it may also have RXD element data). |
| **RXD-only element** | An element that has no IFC entity (views, sheets, annotation, phases, …). It exists only as a record in an RXD document. It still has a GUID in the same 22-character IFC format. |
| **RXD** | Reviera Extension Data: the data IFC cannot express, as *RXD element data* (on one IFC element) and *RXD documents* (project-scoped). Carriers are defined in `spec/02a_extension_data_format.md` (§2.4). |
| **Command** | A typed, registered operation. It is the only way to change the model (§2.12). |
| **Regeneration** | Recomputing derived values and geometry after a change, in dependency order (§2.6). |
| **Family package** | A family stored on disk as an `.ifc` type library carrying the family's RXD (§2.8.14). |

---

## 2.1 Architectural principles

### Model-wide principles
**ID:** DM-CORE-001 · **Priority:** P0 · **Ref:** research/05 §1, §16.1, §16.9; TECH_STACK §1, §3.1, §3.7

**Purpose.** These rules shape every other requirement in this chapter.

**Behaviour & rules.**
1. **IFC is the model of record for everything it can express.** The in-memory building model is an IFC instance population, managed through IfcOpenShell. Reviera has no proprietary project format. RXD holds only what IFC cannot express.
2. **One database per project.** In memory, IFC entities and RXD records form one element store with one transaction system. Every view, schedule and sheet is a live query of it. A change made anywhere appears everywhere on the next regeneration.
3. **Everything with identity is an element.** Every element has an internal ID and a durable GUID (§2.5). IFC-backed elements use their `GlobalId`. RXD-only elements get a GUID in the same format.
4. **IFC first.** Data is stored in a standard IFC entity, attribute, relationship or buildingSMART property set (`Pset_*`, `Qto_*`) whenever one has the right meaning. Data goes to RXD only when IFC has no suitable construct (the split is in §2.4.2).
5. **The building stands without RXD.** The file shall be valid for its declared schema. A tool that ignores or strips every `Reviera_*` property set shall still see correct geometry, spatial structure, types, materials, standard properties and user parameter values. Losing RXD loses *intent* (constraints, views, annotation), not the building.
6. **References by GUID only.** RXD refers to IFC entities by `GlobalId`, never by STEP instance number (`#123`).
7. **Integrity checks.** `02a` defines how Reviera detects RXD that is missing, stale or belongs to another project (for example, after a downstream tool rewrote the file). This chapter defines the user-facing reconciliation (§2.4.3).
8. **Lossless pass-through.** IFC entities, properties and relationships that Reviera does not understand are kept and written back unchanged, with the same GlobalId and values, unless the user edits or deletes their owner. Unknown RXD documents and unknown keys are also kept (`02a`).
9. **Commands only.** Every change goes through a registered command inside a transaction (§2.7, §2.12). The UI, Lua, add-ins and MCP have no other write path.
10. **SI internal units.** All stored quantities use SI base or derived units (metre, radian, kilogram, second, kelvin, …). Display units are a formatting layer (§2.9.5).
11. **Deterministic results.** For the same model and the same command sequence, regeneration gives the same geometry and values, whatever the number of threads (§2.6.4).
12. **Mode independence.** Everything in this chapter behaves the same in Local mode and BIMserver mode. Only open, save, sync and history differ. Because RXD is embedded in IFC by default, it travels with each BIMserver revision as ordinary properties (COL chapter; `02a`).

**Acceptance criteria.**
- [ ] A model saved by Reviera passes IfcOpenShell schema validation and the buildingSMART validation service's schema and normative rule checks for IFC4 ADD2 TC1.
- [ ] Opening a third-party IFC4 file, then saving it without edits, keeps every entity's GlobalId, every property value and every relationship. A semantic diff with IfcOpenShell's diff tooling reports no differences except the header, `IfcOwnerHistory`/application entries and added `Reviera_*` property sets.
- [ ] Stripping every `Reviera_*` property set from a saved project and opening it gives a model with all elements, types, materials and parameter values, plus a notice that views, annotation and constraints were not found.
- [ ] No code path outside the command layer can mutate the element store. This is enforced by Rust visibility (the store's mutating API is crate-private to the transaction module).

---

## 2.2 Element model

### 2.2.1 Elements
**ID:** DM-ELEM-001 · **Priority:** P0 · **Ref:** research/05 §1.1, §1.3, §16.1

**Purpose.** To define the common shape of every stored object.

**Behaviour & rules.**
- Every element shall expose these common attributes, where they apply:

| Attribute | Meaning | Storage |
|---|---|---|
| `id` | Internal 64-bit ID (§2.5) | RXD element data (`identity`)  |
| `guid` | Durable GUID | `IfcRoot.GlobalId`; for RXD-only elements, the record's `guid` |
| `category` | Category key (for example `cat.walls`) | IFC class + `PredefinedType` (+ `ObjectType`); RXD element data (`elements`) only when not derivable |
| `subcategory` | For geometry parts | IFC presentation layer assignment (§2.10.3) |
| `name` | Element or type name | `IfcRoot.Name` |
| `description` | Free text | `IfcRoot.Description` |
| `type_id` | Its type, for instances | `IfcRelDefinesByType` |
| `level_id` | Reference level | Spatial containment; RXD element data (`elements`) when the level is a non-storey level (§2.3.5) |
| `host_id` | Host element, for hosted instances | `IfcRelFillsElement` / `IfcRelVoidsElement` / `IfcRelConnectsElements`; RXD element data (`hosting`) for the host reference detail |
| `owner_view_id` | Owning view, for view-specific elements | RXD document `views/annotation/<viewId>` |
| `phase_created` / `phase_demolished` | Phasing | RXD element data (`phasing`); derived `Pset_*Common.Status` in IFC (§2.3.11) |
| `design_option_id` | Design option membership | RXD element data (`membership`) |
| `workset_id` | Workset membership | RXD element data (`membership`) (COL) |
| `group_id` | Model/detail group membership | `IfcRelAssignsToGroup` (model groups); RXD document `model/groups` |
| `location` | Point, curve or placement | `IfcObjectPlacement` + `Axis` representation |
| `parameters` | All parameter values | IFC attributes, Psets, Qtos (§2.9) |
| `geometry` | Evaluated geometry | `IfcProductDefinitionShape` |

- Elements shall be grouped into these **roles**. The role controls where an element can appear and how it behaves on regeneration:
  - **Model elements** (IFC-backed). These are hosts (walls, floors, roofs, ceilings, toposolids, foundations, stairs) and components (doors, windows, furniture, columns, beams, equipment, and so on). Walls hosted on walls are allowed as hosts (ARC chapter).
  - **Datum elements.** Storey levels and grids are IFC-backed. Non-storey levels, reference planes and scope boxes are RXD-only (RXD document `model/datums`). In families, reference lines and points are in the family definition.
  - **View-specific elements** (RXD-only, RXD document `views/annotation/<viewId>`). Annotation (dimensions, tags, text, symbols, spot elevations, keynotes, revision clouds) and details (detail lines, filled and masking regions, detail components, repeating details, detail groups). Each has exactly one owner view.
  - **Views and sheets** (RXD-only). Every view kind, plus sheets, viewports and schedules (DOC chapter).
  - **Type and family elements.** Types are IFC-backed. A family's parametric definition is an RXD document (§2.8.14).
  - **Settings elements.** Materials and classifications are IFC-backed. Project information is IFC-backed (`IfcProject`, `IfcSite`, `IfcBuilding`, `IfcPostalAddress`). Units, phases, design options, worksets, line and fill patterns, line styles, object styles, filters, view templates, parameter definitions and bindings, and global parameters are RXD-only.
- Elements shall be addressable by internal ID and by GUID through the same query API (§2.13.3), whatever file their primary record lives in.

**Acceptance criteria.**
- [ ] Every element in a loaded model has a non-zero internal ID and a valid GUID.
- [ ] A query `category == cat.doors` returns every door instance and no door types. A separate type query returns the door types.
- [ ] Deleting a view deletes every element whose owner view is that view, in the same transaction.

### 2.2.2 Categories and subcategories
**ID:** DM-CATG-001 · **Priority:** P0 · **Ref:** research/05 §1.1, §9.2, §16.1

**Purpose.** Categories give every element a fixed classification. That classification drives graphics, scheduling, tagging, filtering, behaviour flags and IFC export.

**Behaviour & rules.**
- Categories are **defined by the application** in a versioned **category registry**. Users cannot create categories. Add-ins may *not* add categories in v1. Instead they use `cat.generic_models` with an IFC `ObjectType` (see open question Q-DM-01).
- Each category definition shall contain:
  - `key` (stable string, for example `cat.walls`), display name (localisable), discipline;
  - `ifc_class`, `ifc_type_class` and a default `PredefinedType` (RXD-only categories have `ifc_class = none`);
  - behaviour flags: `is_model`, `is_annotation`, `is_cuttable`, `is_hostable_target` (can be a host), `requires_host`, `is_room_bounding_default`, `is_schedulable`, `is_taggable`, `is_tag_category` (with target categories), `allows_material`, `allows_subcategories`;
  - default Object Style entries for projection and cut (line weight, colour, pattern, material) (§2.10.3);
  - built-in parameters and their IFC mapping (§2.9.2).
- **Subcategories** are user- or family-defined children of a category, for example Doors › Frame/Mullion, Doors › Panel, Doors › Swing. They carry their own Object Style. Subcategories created in a family are added to the project when the family loads. Name clashes merge into the existing subcategory.
- The category's internal numeric ID shall be **negative** and stable across releases. This keeps category IDs separate from element IDs (§2.5).
- Subcategory definitions and their GUIDs are stored in RXD document `standards/styles` (§2.10.3). In IFC, geometry assigned to a subcategory is placed on an `IfcPresentationLayerAssignment` named `<Category>|<Subcategory>`, so other tools can toggle it.

**Category registry (normative baseline; domain chapters add rows).**

| Category key | IFC occurrence | IFC type | Default PredefinedType | Notes |
|---|---|---|---|---|
| `cat.walls` | `IfcWall` | `IfcWallType` | `STANDARD` (layered), `SOLIDWALL`, `PARTITIONING`, `SHEAR`, `ELEMENTEDWALL` (curtain), `PARAPET` | `IfcWallStandardCase` is never written for IFC4 (it is deprecated). |
| `cat.curtain_walls` | `IfcCurtainWall` | `IfcCurtainWallType` | `USERDEFINED` | Aggregates panels and mullions (§2.3.9) |
| `cat.curtain_panels` | `IfcPlate` | `IfcPlateType` | `CURTAIN_PANEL` | A door in a curtain panel is an `IfcDoor` aggregated by the curtain wall. |
| `cat.curtain_mullions` | `IfcMember` | `IfcMemberType` | `MULLION` | |
| `cat.floors` | `IfcSlab` | `IfcSlabType` | `FLOOR`, `BASESLAB`, `LANDING` | |
| `cat.roofs` | `IfcRoof` (aggregate) + `IfcSlab` `ROOF` parts | `IfcRoofType` | `FLAT_ROOF`, `SHED_ROOF`, `GABLE_ROOF`, `HIP_ROOF`, `FREEFORM`, … | A single-face roof may be written as an `IfcSlab` `ROOF` (open question Q-DM-02). |
| `cat.ceilings` | `IfcCovering` | `IfcCoveringType` | `CEILING` | |
| `cat.doors` | `IfcDoor` | `IfcDoorType` | `DOOR`, `GATE`, `TRAPDOOR` | `OperationType` on the type |
| `cat.windows` | `IfcWindow` | `IfcWindowType` | `WINDOW`, `SKYLIGHT`, `LIGHTDOME` | `PartitioningType` on the type |
| `cat.stairs` | `IfcStair` (aggregate) | `IfcStairType` | from shape, for example `STRAIGHT_RUN_STAIR` | Runs `IfcStairFlight`, landings `IfcSlab` `LANDING` |
| `cat.railings` | `IfcRailing` | `IfcRailingType` | `HANDRAIL`, `GUARDRAIL`, `BALUSTRADE` | |
| `cat.ramps` | `IfcRamp` | `IfcRampType` | `STRAIGHT_RUN_RAMP`, … | |
| `cat.columns` (architectural) | `IfcColumn` | `IfcColumnType` | `COLUMN` + `Pset_ColumnCommon.LoadBearing=false` | |
| `cat.structural_columns` | `IfcColumn` | `IfcColumnType` | `COLUMN` + `LoadBearing=true` | STR chapter |
| `cat.structural_framing` | `IfcBeam` / `IfcMember` | `IfcBeamType` / `IfcMemberType` | `BEAM`, `JOIST`, `BRACE` | STR chapter |
| `cat.structural_foundations` | `IfcFooting` / `IfcPile` | `IfcFootingType` / `IfcPileType` | `PAD_FOOTING`, `STRIP_FOOTING`, … | |
| `cat.furniture` | `IfcFurniture` | `IfcFurnitureType` | `USERDEFINED` | IFC4: `IfcFurnishingElement` subtype |
| `cat.casework` | `IfcFurniture` | `IfcFurnitureType` | `USERDEFINED` (ObjectType `Casework`) | |
| `cat.plumbing_fixtures` | `IfcSanitaryTerminal` | `IfcSanitaryTerminalType` | `WASHHANDBASIN`, `TOILETPAN`, … | |
| `cat.generic_models` | `IfcBuildingElementProxy` | `IfcBuildingElementProxyType` | `USERDEFINED` | IFC4X3: `IfcBuiltElement` may be used |
| `cat.specialty_equipment` | `IfcBuildingElementProxy` | `IfcBuildingElementProxyType` | `USERDEFINED` (ObjectType `SpecialtyEquipment`) | |
| `cat.mass` | `IfcBuildingElementProxy` | `IfcBuildingElementProxyType` | `USERDEFINED` (ObjectType `Mass`) | Not exported to exchange MVDs by default (§2.11.4) |
| `cat.rooms` | `IfcSpace` | `IfcSpaceType` | `INTERNAL`, `EXTERNAL` | `Qto_SpaceBaseQuantities` |
| `cat.areas` | `IfcSpace` | — | `USERDEFINED` (ObjectType `Area:<scheme>`) | `IfcZone` for area groupings |
| `cat.levels` | `IfcBuildingStorey` (storey levels) | — | — | Non-storey levels: RXD document `model/datums` (§2.3.5) |
| `cat.grids` | `IfcGrid` (axes are `IfcGridAxis`) | — | — | §2.3.6 |
| `cat.reference_planes`, `cat.scope_boxes` | — | — | — | RXD document `model/datums` |
| `cat.openings` | `IfcOpeningElement` | — | `OPENING`, `RECESS` | |
| `cat.toposolid` | `IfcGeographicElement` (IFC4) | `IfcGeographicElementType` | `TERRAIN` | IFC4X3 may use `IfcEarthworksFill` / `IfcGeotechnicalStratum` |
| `cat.ducts`, `cat.pipes`, … | `IfcDuctSegment`, `IfcPipeSegment`, … | matching types | per MEP chapter | MEP chapter |
| `cat.parts` | `IfcBuildingElementPart` | `IfcBuildingElementPartType` | `INSULATION`, `PRECASTPANEL`, `USERDEFINED` | |
| `cat.assemblies` | `IfcElementAssembly` | `IfcElementAssemblyType` | `USERDEFINED` | |
| Annotation categories (`cat.dimensions`, `cat.text_notes`, `cat.*_tags`, `cat.generic_annotations`, `cat.detail_items`, `cat.filled_regions`, `cat.revision_clouds`, …) | — | — | — | RXD document `views/annotation/<viewId>` (optional `IfcAnnotation` on export, §2.11.4) |
| `cat.views`, `cat.sheets`, `cat.schedules`, `cat.viewports` | — | — | — | RXD documents `views/<viewId>`, `sheets`, `schedules` |

**Data (storage).** The category registry itself is application data, not model data. For IFC-backed elements the category is derived from the IFC class, `PredefinedType` and `ObjectType`, using the table above. When that is ambiguous (for example furniture vs casework), the category key is in RXD element data (`elements`). If there is no RXD entry, the category is inferred from the IFC class.

**Commands.**
`subcategory.create(parent_category, name) -> element_id` ·
`subcategory.rename(subcategory_id, name)` ·
`subcategory.delete(subcategory_id, reassign_to?)`

**Acceptance criteria.**
- [ ] Each IFC-backed row of the registry round-trips: create an element of the category, save, reopen. It comes back with the same category, and an IFC viewer shows the IFC class listed in the table.
- [ ] Importing a third-party `IfcDoor` with no RXD assigns `cat.doors`.
- [ ] Deleting a subcategory that geometry uses either reassigns that geometry to the parent category or to the named subcategory. Undo restores both the subcategory and the assignments.

### 2.2.3 Families: system, loadable and in-place
**ID:** DM-FAM-001 · **Priority:** P0 (system, loadable placement) / P1 (in-place, family editing) · **Ref:** research/05 §1.2, §4.9, §16.2

**Purpose.** To define the three family kinds and where their definitions live.

**Behaviour & rules.**

| Kind | Definition lives in | Created by | Editable with | Portability |
|---|---|---|---|---|
| **System family** | The application (geometry logic in Rust). Its types live in the project. | Built in. Types are created with *Duplicate* in Type Properties. | Specialised editors (for example the wall layer editor in ARC) | Types move between projects only through Transfer Project Standards (§2.10.9) or copy/paste |
| **Loadable family** | A family package (§2.8.14). A full copy is embedded in each project that uses it. | The Reviera Family Editor | The Family Editor (opened from a package, or from the project with *Edit Family*) | Saved and loaded as family packages |
| **In-place family** | The project only. It is single-use. | *Model In-Place* inside a project | The Family Editor tools, in an in-place session | Not reusable. Copying makes an independent copy. |

- A loadable family embedded in a project shall carry its **full** definition (parameters, formulas, forms, constraints, nested families, lookup tables), not only the evaluated geometry. This lets the family be edited from the project and saved back out as a package.
- Loading a family that already exists (same family GUID, §2.5) shall offer **Keep project values** or **Overwrite parameter values from the family**. In both cases, instance placements are kept. Types that were removed from the family file stay in the project until purged.
- Loading a family with the same *name* but a different family GUID shall prompt the user to **rename** the incoming family or **replace** the existing one. Replacing remaps types by name and reports any instances whose types could not be remapped.
- Nested shared families that are already in the project prompt **Use project version** / **Use version from family** (§2.8.10).

**Data (storage).**
- IFC: each loadable or in-place family is an `IfcProjectLibrary` (Name = family name) declared by the `IfcProject` through `IfcRelDeclares`. The family declares its types through a second `IfcRelDeclares` (§2.3.2). System families have no library object. Their types are declared directly by the `IfcProject`.
- RXD document `families/<id>` holds the parametric definition (§2.8.14). RXD document `families/index` lists every family with kind (`system`/`loadable`/`in_place`), system-family key (for example `basic_wall`, `stacked_wall`, `curtain_wall`), template, category, version and the GUIDs of its types.

**Commands.**
`family.load(path, type_names?, overwrite_values=false) -> family_id` ·
`family.reload(family_id, path?, overwrite_values=false)` ·
`family.save_as(family_id, path)` ·
`family.edit(family_id) -> family_document_id` ·
`family.rename(family_id, name)` ·
`family.delete(family_id)` ·
`family.create_in_place(category, name) -> family_document_id`

**Acceptance criteria.**
- [ ] Load a door family, place three instances, edit the family from the project to change frame depth, and reload it. All three instances update and keep their positions and hosts.
- [ ] *Save As* on an embedded family produces a family package that loads into a second, empty project with the same types and parameter values.
- [ ] Loading with *Keep project values* leaves type-parameter values that were changed in the project unchanged. *Overwrite* resets them to the file's values.

### 2.2.4 Types
**ID:** DM-TYPE-001 · **Priority:** P0 · **Ref:** research/05 §1.1, §7.6; screenshots/platform/family_types_dialog_parameter_groups.png

**Purpose.** A type is a named, reusable set of type-parameter values.

**Workflow.**
1. Select an instance. In the Properties palette, choose **Edit Type**.
2. In **Type Properties**, choose **Duplicate**, enter a name and edit the values. Choose **Rename** to rename the type.
3. Choose **OK**. All instances of the type regenerate.

**UI.** Properties palette › type selector and **Edit Type** button. The Type Properties dialog has: Family drop-down, Type drop-down, Duplicate, Rename, a grouped parameter grid (Parameter | Value, with a formula indicator), Preview, and a link to the family's editor (for loadable families) or assembly editor (for system families).

**Behaviour & rules.**
- Type names are unique within a family. Duplicating a type copies every type-parameter value, plus its material assignments and layer structure.
- Editing a type parameter shall regenerate every instance of that type in a single transaction.
- Changing the type of an instance (**type swap**) keeps every instance parameter that has the same definition (same GUID or same built-in key) in both types. It keeps named references that dimensions and constraints point to (§2.8.3). It reports dimensions or constraints that could not be kept as warnings.
- A type can be deleted only when it has no instances, or after confirming that its instances will be deleted too.
- **Type Properties dialog** (`type.edit_properties(type_id?)`). With no argument it opens on the type of the current selection; if the selection holds several types it opens on the first and the Type drop-down lists the others; with nothing selected it opens on the type chosen in the Properties palette type selector. It refuses with `type.none_selected` when there is no type in context.
  - The dialog edits a **working copy**. Values are checked as they are typed (data type, unit parsing §2.9.5, `min`/`max`, `enum`), and an invalid cell is outlined with the error in its tooltip. **OK** and **Apply** are disabled while any cell is invalid.
  - **Apply** commits all changed values as one transaction named "Edit Type: <type name>" (a sequence of `type.set_param` calls) and keeps the dialog open; **OK** applies and closes; **Cancel** discards uncommitted edits.
  - **Duplicate** asks for a name, proposes "<name> 2" (next free number), and refuses a name already used in the family. **Rename** follows the same rule. Both act immediately as their own transactions and switch the dialog to the result.
  - Formula-driven, reporting and read-only parameters are greyed. Parameters associated with a global parameter show the association and are read-only (§2.9.6).
  - **Preview** (toggle) shows a live thumbnail of the type in a plan, elevation or 3D view of the user's choice; it regenerates from the working copy so the effect of an unapplied value is visible.
  - The dialog reports how many instances use the type ("Used by 128 instances"), so the user knows the reach of an edit.

**Data (storage).** `IfcXxxType` (§2.3.2). Type-parameter values live in property sets in `IfcTypeObject.HasPropertySets`. `IfcTypeProduct.Tag` holds the type mark. Duplicating creates a new type entity with a new GlobalId. Type data IFC cannot hold (for example wall core boundaries and wrapping) is in RXD element data (`types`), keyed by the type's GlobalId.

**Commands.**
`type.duplicate(type_id, new_name) -> type_id` ·
`type.rename(type_id, name) -> ()` ·
`type.delete(type_id, delete_instances=false)` ·
`type.set_param(type_id, param, value) -> ()` ·
`type.edit_properties(type_id?)` (opens the Type Properties dialog) ·
`instance.change_type(element_ids[], type_id)`

**Acceptance criteria.**
- [ ] In Type Properties, typing "abc" into a Length cell outlines it with a parse error and disables OK; correcting it re-enables OK, and OK changes the value of every instance in one undo item.
- [ ] Duplicating a wall type and changing a layer's thickness in the copy does not affect walls of the original type.
- [ ] Swapping a door between two types of the same family keeps its Mark, host and position, and keeps dimensions that point to the "Left" and "Right" named references.
- [ ] The IFC export of a duplicated type shows two distinct `IfcDoorType` entities, each linked to its own instances through `IfcRelDefinesByType`.

### 2.2.5 Instances
**ID:** DM-INST-001 · **Priority:** P0 · **Ref:** research/05 §1.1, §2.2

**Purpose.** An instance is one placed occurrence of a type, with its own location and instance-parameter values.

**Behaviour & rules.**
- **Placement modes** come from the family template (§2.8.1). They are: level-based (elevation is taken from a level plus an offset), work-plane-based, face-based, host-based (wall, floor, ceiling, roof), line-based, two-level-based, adaptive (N points), and view-specific (annotation and detail).
- A parameter is *either* a type parameter *or* an instance parameter in a given family (§2.9.1). Instance parameters do not override type parameters.
- Flip, mirror and rotation state are instance properties: `facing_flipped`, `hand_flipped`, `mirrored`, plus a rotation about the placement Z axis.
- Every instance shall report computed, read-only parameters: area, volume, length and perimeter where they apply, host, level and room. These are refreshed by regeneration.

**Data (storage).** IFC: the occurrence entity, with `ObjectPlacement` (an `IfcLocalPlacement` relative to the storey placement, or to the host for hosted elements) and `Representation`. The body is usually an `IfcMappedItem` of the type's `RepresentationMap`, whose transform includes any mirroring (§2.3.10). RXD element data (`placement`) keeps the flip flags and work-plane reference (02a §6.4); RXD element data (`elements`) keeps the level offset to a non-storey level and the room-calculation point. These cannot be recovered reliably from the transform.

**Commands.**
`instance.place(type_id, placement, host_id?, level_id?, params?) -> element_id` ·
`instance.place_adaptive(type_id, points[], hosts?[]) -> element_id` ·
`instance.flip(element_id, axis: "facing"|"hand"|"work_plane")` ·
`instance.pick_new_host(element_id, host_id, face_ref?)`

**Acceptance criteria.**
- [ ] A level-based instance moved to another level keeps its absolute elevation if "keep elevation" is chosen, or keeps its offset if not.
- [ ] Flipping a door's facing changes the exported geometry and the RXD flag. The opening and fill relationships stay intact.

### 2.2.6 Groups, assemblies, parts and links (storage)
**ID:** DM-ELEM-002 · **Priority:** P1 · **Ref:** research/05 §1.4, §7.6

**Purpose.** To fix how compound elements are stored. Their tools are defined in ARC (groups, parts), DOC (assemblies, Ch 5 §5.16) and COL (links).

**Behaviour & rules.**
- **Model and detail groups.** A group type is an element that holds a template of member elements. Each group instance holds real member elements that are kept in step with the group type. Parameters bound with "values vary by group instance" may differ between instances. Parameters bound with "values aligned per group type" are forced equal (§2.9.3).
- **Assemblies.** A named set of elements with a naming category. Assembly views are defined in DOC.
- **Parts.** Elements derived from layers of layered hosts. They can be divided, merged and excluded.
- **Links.** A link type references an external model (a file path or a BIMserver project/revision). A link instance places it with a transform. Linked elements are read-only.

**Data (storage).**

| Concept | IFC | RXD |
|---|---|---|
| Model group instance | `IfcGroup` (ObjectType = group type name) + `IfcRelAssignsToGroup` (members), so other tools see the grouping | RXD document `model/groups`: group types (member template), instance → type, per-instance variations |
| Detail group | — | RXD document `model/groups` + members in RXD document `views/annotation/<viewId>` |
| Assembly | `IfcElementAssembly` + `IfcRelAggregates` (members) | RXD document `model/assemblies` (naming category, assembly views) |
| Parts | `IfcBuildingElementPart` aggregated by the host through `IfcRelAggregates` | RXD element data `parts` on the host (division sketches, exclusions) |
| Link type / instance | Optional `IfcDocumentReference` through `IfcRelAssociatesDocument` on `IfcProject` | RXD document `project/links` (source URI, revision, transform, visibility) |

**Commands.** Defined in ARC, DOC and COL. This chapter reserves the command namespaces `group.*`, `assembly.*`, `part.*` and `link.*`.

**Acceptance criteria.**
- [ ] A model with two instances of a group type round-trips. After reopening, editing one member in one instance and choosing "apply to group type" updates the other instance.

---

## 2.3 IFC4 mapping

### 2.3.1 Schema baseline and header
**ID:** DM-IFC-001 · **Priority:** P0 · **Ref:** research/05 §11.1, §12.1; TECH_STACK §3.1, §3.7.3

**Behaviour & rules.**
- The **default project schema** is **IFC4 ADD2 TC1** (`FILE_SCHEMA(('IFC4'))`). Projects may be created in, or converted to, **IFC4X3_ADD2** in Local mode. In BIMserver mode this is allowed only when the server advertises a matching deserializer (COL chapter).
- Reading is supported for **IFC2X3 TC1**, **IFC4** and **IFC4X3_ADD2**. An IFC2X3 file is upgraded in memory to the project schema (§2.11.8). The user is told that saving writes the new schema, and that "Export IFC2X3" (§2.11.4) remains available.
- The header written by Reviera shall contain:
  - `FILE_DESCRIPTION(('ViewDefinition [DesignTransferView]'),'2;1')`;
  - `FILE_NAME` with the application name `Reviera`, the version, the author and organisation from Project Information, and a UTC timestamp;
  - `FILE_SCHEMA(('IFC4'))` or `FILE_SCHEMA(('IFC4X3_ADD2'))`.
- `IfcOwnerHistory` is **optional** in IFC4. Reviera shall write it on `IfcRoot` entities only when the project setting "Write owner history" is on (default: off in Local mode, on in BIMserver mode). Change attribution is otherwise kept in RXD element data (`identity`) (§2.5).
- `IfcProject.RepresentationContexts` shall contain a `Model` context (3D) with sub-contexts `Body`, `Axis`, `Box`, `FootPrint`, `Clearance` and `Reference`, and a `Plan` context (2D) with sub-contexts `Annotation` (`PLAN_VIEW`, `SECTION_VIEW`, `ELEVATION_VIEW`) and `Body`/`Axis`. `TrueNorth` and `WorldCoordinateSystem` follow §2.10.7.
- `IfcProject.UnitsInContext` shall declare SI units for every measure Reviera writes. It shall use `IfcSIUnit` (`METRE`, `SQUARE_METRE`, `CUBIC_METRE`, `RADIAN`, …), even when display units are imperial. Display units are kept in RXD document `project/settings` (§2.9.5).

**Acceptance criteria.**
- [ ] A new project file has exactly one `IfcProject`, one `IfcUnitAssignment` in SI, and the representation contexts listed above.
- [ ] Converting a project from IFC4 to IFC4X3_ADD2 and back produces a model with the same element count, GUIDs and RXD content. Entity renames (for example `IfcBuildingElementType` → `IfcBuiltElementType`) are listed in the migration report.

### 2.3.2 Types and occurrences
**ID:** DM-IFC-002 · **Priority:** P0 · **Ref:** research/05 §1.1, §1.2; TECH_STACK §3.1

**Behaviour & rules.**
- Every Reviera **type** shall be an `IfcTypeObject` subtype matching its category (§2.2.2). Its `Name` is `<FamilyName>:<TypeName>`, the convention other tools use to split family and type. Its `ElementType` is `<TypeName>`. Its `Tag` holds the type mark.
- `PredefinedType` shall be set from the category default, or from the family's IFC classification family parameter (§2.8.2). When it is `USERDEFINED`, the type's `ElementType` and the occurrence's `ObjectType` shall carry the user-defined value.
- Every **instance** shall be linked to its type through exactly one `IfcRelDefinesByType` (`RelatingType` = type, `RelatedObjects` = all its instances). Reviera keeps **one relationship per type** and updates its `RelatedObjects` set, rather than creating one relationship per instance.
- **Shared geometry.** A type's evaluated geometry for the default parameter state is written once as `IfcTypeProduct.RepresentationMaps` (an `IfcRepresentationMap` with `Body`, and also `Axis`, `FootPrint` or plan `Annotation` maps where they exist). An instance whose geometry equals the type's geometry under a rigid transform (plus mirroring) shall use an `IfcMappedItem` referencing that map. An instance with unique geometry (because of instance parameters, joins or cuts) shall have its own representation.
- An occurrence's `PredefinedType` shall be `NOTDEFINED`, or equal to its type's value. Reviera writes `NOTDEFINED` on occurrences when the type is set, so the value is not duplicated.
- **Families** have no IFC class. A loadable or in-place family is an `IfcProjectLibrary` declared by the `IfcProject` through `IfcRelDeclares`, and it declares its types (§2.2.3). This is valid IFC4 and lets other tools see which types belong together.

**Example (informative, STEP fragment).**

```
#100= IFCPROJECTLIBRARY('3vB2…',$,'Single-Flush Door',$,$,$,$,$,$);
#101= IFCRELDECLARES('0Lw9…',$,$,$,#1,(#100));             /* project declares the family */
#110= IFCDOORTYPE('1xH4…',$,'Single-Flush Door:0915 x 2134',$,$,(#120,#121),(#130),'D1','0915 x 2134',.DOOR.,.SINGLE_SWING_LEFT.,.F.,$);
#111= IFCRELDECLARES('2Qa8…',$,$,$,#100,(#110));           /* family declares its types   */
#140= IFCDOOR('0aZ1…',$,'Single-Flush Door:0915 x 2134:301245',$,$,#141,#142,'D-104',2.134,0.915,.NOTDEFINED.,$,$);
#150= IFCRELDEFINESBYTYPE('3pR7…',$,$,$,(#140,#160,#170),#110);
```

**Acceptance criteria.**
- [ ] In a model with 500 instances of 3 door types, the file has 3 `IfcRelDefinesByType` relationships, and a viewer groups the doors by type.
- [ ] Identical furniture instances share a single `IfcRepresentationMap`. The file size grows by less than 2 KB per extra identical instance.
- [ ] An instance with a unique instance-parameter value (for example a different width) gets its own representation, and keeps its type relationship.

### 2.3.3 Property sets and quantity sets
**ID:** DM-IFC-003 · **Priority:** P0 · **Ref:** research/05 §7, §12.2; screenshots/platform/ifc_property_mapping_2027.png

**Purpose.** Parameter *values* are stored as IFC properties, so other tools can read them. Parameter *definitions* (data type, group, formula, GUID, tooltip, binding) are RXD (document `parameters`, §2.9).

**Behaviour & rules.**
- **Type parameters** live in property sets attached to the type through `IfcTypeObject.HasPropertySets`. **Instance parameters** live in property sets attached to the occurrence through `IfcRelDefinesByProperties`.
- When an occurrence has a property set with the same `Name` as one on its type, IFC treats the occurrence's property as the override. Reviera never writes the *same* property on both type and occurrence for one parameter, because a parameter is either type or instance (§2.9.1). When reading third-party files, Reviera follows IFC: the occurrence value wins, and the property is shown as an instance parameter.
- Each parameter has exactly one **IFC home** (`pset_name`, `property_name`), taken from its definition (§2.9.2). The order of preference is:
  1. a direct IFC attribute (for example `Name`, `Description`, `Tag`, `OverallHeight`);
  2. a buildingSMART property set for the entity (`Pset_WallCommon`, …);
  3. a buildingSMART quantity set (`Qto_*BaseQuantities`), for computed quantities;
  4. the property set named in the parameter definition (project, shared and family parameters; §2.9.2);
  5. for built-in parameters with no standard home, a property set named after the parameter's group (for example `Identity Data`, `Constraints`, `Other`), with the property named after the parameter (see Q-DM-03).
- **Quantity sets** are written as `IfcElementQuantity` with `MethodOfMeasurement = 'BaseQuantities'` from regenerated geometry on each save. They are read-only in Reviera. If a user edits a quantity in a third-party file, the edit is kept as an "imported quantity" until the element is regenerated.
- Property values shall use the IFC measure type of the parameter's data type (§2.9.4). Enumerations shall use `IfcPropertyEnumeratedValue` with a shared `IfcPropertyEnumeration`. Element references (material, family type, element) shall use `IfcPropertyReferenceValue`.
- A property set with no values is not written. A bound parameter with no value is written as a property with a null `NominalValue`, so schedules in other tools can see the field.

**Standard property sets written (baseline; domain chapters extend).**

| Category | Property set | Properties Reviera maps |
|---|---|---|
| Walls | `Pset_WallCommon` | Reference (type name), Status (phase status §2.3.11), AcousticRating, FireRating, Combustible, SurfaceSpreadOfFlame, ThermalTransmittance (from layers §2.10.5), IsExternal (Function = Exterior), LoadBearing (Structural Usage), ExtendToStructure, Compartmentation |
| Walls | `Qto_WallBaseQuantities` | Length, Width, Height, GrossFootprintArea, NetFootprintArea, GrossSideArea, NetSideArea, GrossVolume, NetVolume, GrossWeight, NetWeight |
| Doors | `Pset_DoorCommon` | Reference, Status, FireRating, AcousticRating, SecurityRating, IsExternal, Infiltration, ThermalTransmittance, GlazingAreaFraction, HandicapAccessible, FireExit, SelfClosing, SmokeStop |
| Doors | `Qto_DoorBaseQuantities` | Width, Height, Perimeter, Area |
| Windows | `Pset_WindowCommon` | Reference, Status, AcousticRating, FireRating, SecurityRating, IsExternal, Infiltration, ThermalTransmittance, GlazingAreaFraction, HasSillExternal, HasSillInternal, HasDrive, SmokeStop, FireExit |
| Windows | `Qto_WindowBaseQuantities` | Width, Height, Perimeter, Area |
| Floors | `Pset_SlabCommon` | Reference, Status, AcousticRating, FireRating, Combustible, SurfaceSpreadOfFlame, ThermalTransmittance, IsExternal, LoadBearing, PitchAngle, Compartmentation |
| Floors | `Qto_SlabBaseQuantities` | Width (thickness), Length, Depth, Perimeter, GrossArea, NetArea, GrossVolume, NetVolume, GrossWeight, NetWeight |
| Roofs | `Pset_RoofCommon` / `Qto_RoofBaseQuantities` | Reference, Status, AcousticRating, FireRating, IsExternal, ThermalTransmittance / GrossArea, NetArea, ProjectedArea |
| Ceilings | `Pset_CoveringCommon` / `Qto_CoveringBaseQuantities` | Reference, Status, FireRating, AcousticRating, FlammabilityRating, IsExternal, ThermalTransmittance, TotalThickness, Finish / Width, GrossArea, NetArea |
| Columns | `Pset_ColumnCommon` / `Qto_ColumnBaseQuantities` | Reference, Status, Slope, Roll, IsExternal, LoadBearing, FireRating / Length, CrossSectionArea, OuterSurfaceArea, GrossSurfaceArea, NetSurfaceArea, GrossVolume, NetVolume, GrossWeight, NetWeight |
| Beams | `Pset_BeamCommon` / `Qto_BeamBaseQuantities` | Reference, Status, Span, Slope, Roll, IsExternal, LoadBearing, FireRating / Length, CrossSectionArea, OuterSurfaceArea, GrossSurfaceArea, NetSurfaceArea, GrossVolume, NetVolume, GrossWeight, NetWeight |
| Stairs | `Pset_StairCommon` / `Pset_StairFlightCommon` | NumberOfRiser, NumberOfTreads, RiserHeight, TreadLength, NosingLength, WalkingLineOffset, TreadLengthAtOffset, TreadLengthAtInnerSide, WaistThickness, RequiredHeadroom, HandicapAccessible, HasNonSkidSurface, IsExternal, FireRating |
| Railings | `Pset_RailingCommon` | Reference, Status, Height, Diameter, IsExternal |
| Rooms | `Pset_SpaceCommon` / `Qto_SpaceBaseQuantities` | Reference (number), IsExternal, GrossPlannedArea, NetPlannedArea, PubliclyAccessible, HandicapAccessible / Height, FinishCeilingHeight, FinishFloorHeight, GrossPerimeter, NetPerimeter, GrossFloorArea, NetFloorArea, GrossWallArea, NetWallArea, GrossCeilingArea, NetCeilingArea, GrossVolume, NetVolume |
| Levels | `Pset_BuildingStoreyCommon` / `Qto_BuildingStoreyBaseQuantities` | EntranceLevel, AboveGround, SprinklerProtection, SprinklerProtectionAutomatic, GrossPlannedArea, NetPlannedArea / GrossHeight, NetHeight, GrossPerimeter, GrossFloorArea, NetFloorArea, GrossVolume, NetVolume |
| Project | `Pset_BuildingCommon` (on `IfcBuilding`), `Pset_SiteCommon` (on `IfcSite`) | BuildingID, IsPermanentID, ConstructionMethod, FireProtectionClass, SprinklerProtection, OccupancyType, GrossPlannedArea, NumberOfStoreys, YearOfConstruction, IsLandmarked / BuildableArea, SiteCoverageRatio, FloorAreaRatio, BuildingHeightLimit, TotalArea |
| Generic models | `Pset_BuildingElementProxyCommon` | Reference, Status, IsExternal, LoadBearing, FireRating |
| Furniture | `Pset_FurnitureTypeCommon` | Reference, Style, NominalHeight, NominalLength, NominalDepth, MainColour, IsBuiltIn |
| Any type with product data | `Pset_ManufacturerTypeInformation` (type) | Manufacturer, ModelReference, ModelLabel, ProductionYear, GlobalTradeItemNumber, ArticleNumber |
| Any element with carbon data | `Pset_EnvironmentalImpactIndicators` (type or occurrence) | Reference, FunctionalUnitReference, Unit, ExpectedServiceLife, ClimateChangePerUnit (from material carbon, §2.10.5) |
| Any element | `Pset_Condition`, `Pset_Warranty` | Only when the user binds them |

- **Mark** shall map to `IfcElement.Tag`. **Type Mark** shall map to `IfcTypeProduct.Tag`. **Classification Code** shall map to `IfcClassificationReference` through `IfcRelAssociatesClassification`. The classification system (for example Uniclass 2015, OmniClass, MasterFormat, Uniformat) is an `IfcClassification` (§2.10.10).
- Built-in parameters with no standard home (Comments, Keynote, Assembly Code, Cost, URL, Type Comments, Description, Model, Manufacturer when there is no product data) go to group-named property sets under rule 5 above.

**Commands.**
`pset.set(element_id, pset_name, property_name, value)` ·
`pset.remove(element_id, pset_name, property_name?)` ·
`qto.recompute(element_ids?[])`

**Acceptance criteria.**
- [ ] Setting a wall's Fire Rating to "EI 60" writes `Pset_WallCommon.FireRating = IfcLabel('EI 60')` on the wall *type*, because the built-in definition is type-level for walls.
- [ ] Opening the saved file in a third-party IFC viewer shows `Qto_WallBaseQuantities.NetSideArea` equal (within 1 mm²) to the Reviera schedule's wall area.
- [ ] A third-party file with `Pset_DoorCommon.FireRating` on both a type and an occurrence shows the occurrence value in Reviera, marked as an instance override.
- [ ] A shared parameter "Acoustic Class" bound to doors appears in a third-party viewer under its configured property set name, with the value entered in Reviera.

### 2.3.4 Spatial structure
**ID:** DM-IFC-004 · **Priority:** P0 · **Ref:** research/05 §1.3, §9.1, §9.8

**Behaviour & rules.**
- Each project shall have this decomposition, using `IfcRelAggregates`: `IfcProject` → one or more `IfcSite` → one or more `IfcBuilding` → zero or more `IfcBuildingStorey`. A new project contains one site ("Default Site"), one building ("Building 1") and two storeys ("Level 1", "Level 2").
- Every model element that has a reference level shall be in exactly one `IfcRelContainedInSpatialStructure`, whose `RelatingStructure` is the storey for that level (§2.3.5). Elements without a level are contained in the building or, for site elements, in the site.
- Elements that are parts of another element (curtain panels, stair flights, roof slabs, assembly members, shared nested components) shall **not** be contained in a spatial structure directly. They are reached through `IfcRelAggregates` from the parent, as IFC requires.
- **Rooms** (`IfcSpace`) shall be aggregated by their storey through `IfcRelAggregates`, not contained.
- Elements that span several storeys (for example a multi-storey column or a curtain wall) are contained in their base storey. The other storeys are listed through `IfcRelReferencedInSpatialStructure`.
- Reviera may support several buildings on one site. `level.create` takes an optional building.

**Commands.**
`site.create(name) -> element_id` · `building.create(site_id, name) -> element_id` · `spatial.reassign(element_ids[], container_id)`

**Acceptance criteria.**
- [ ] Validation finds no element contained in more than one spatial structure, and no aggregated part that is also contained.
- [ ] A two-storey column is contained in Level 1 and referenced in Level 2.

### 2.3.5 Levels
**ID:** DM-IFC-005 · **Priority:** P0 · **Ref:** research/05 §1.3, §2.2; TECH_STACK §3.1

**Behaviour & rules.**
- A level marked **Building Storey** (default on) shall be an `IfcBuildingStorey`, aggregated by its building. Its `Elevation` attribute and its placement Z shall both equal the level elevation relative to the building's zero.
- A level marked as **not** a storey (for example "Top of Parapet") has no IFC construct with the right meaning. It is a RXD-only datum in RXD document `model/datums`. Elements whose reference level is a non-storey level are contained in the nearest storey *below* it. Their reference level and offset are kept in RXD element data (`elements`), so offsets keep their meaning.
- Toggling the storey flag converts between the two forms in a single transaction. The GUID is preserved (the RXD record takes over the storey's GlobalId, and vice versa). Spatial containment is reassigned.
- Level data that IFC cannot hold (elevation base Project/Survey, structural flag, computation height, level head type, 2D/3D extents per view) is in RXD document `model/datums`.

**Acceptance criteria.**
- [ ] Changing a level's elevation moves every element whose base is constrained to that level, in the same transaction, and updates `IfcBuildingStorey.Elevation`.
- [ ] A non-storey level is not listed as a storey by a third-party viewer. Elements referencing it appear in the storey below.

### 2.3.6 Grids
**ID:** DM-IFC-006 · **Priority:** P0 · **Ref:** research/05 §1.3; TECH_STACK §3.1

**Behaviour & rules.**
- Reviera treats each grid line as its own element (selectable, with its own name, type and extents). IFC groups axes into an `IfcGrid`. The mapping is:
  - each **grid system** (default: one per building, or one per user-defined grid group) is one `IfcGrid`, contained in the lowest storey, or in the building;
  - each Reviera grid line is one `IfcGridAxis` in `UAxes`, `VAxes` or `WAxes`. Reviera assigns U to lines within ±45° of project X, V to the rest, and W to arcs and multi-segment grids;
  - `IfcGridAxis.AxisTag` is the grid name, and `AxisCurve` is the 2D curve. Multi-segment grids use an `IfcCompositeCurve`.
- `IfcGridAxis` is not an `IfcRoot`, so it has no GlobalId. Per-axis identity is kept in RXD document `model/datums`. Each record holds `{grid_guid, axis_tag, axis_guid, internal_id, grid_type, extents, bubble_visibility}`, keyed by the `IfcGrid` GlobalId plus the axis tag.
- Renaming a grid updates `AxisTag` and the RXD key in the same transaction. The axis GUID is unchanged.

**Acceptance criteria.**
- [ ] Six orthogonal grid lines produce one `IfcGrid` with three U axes and three V axes, and a third-party viewer shows their tags.
- [ ] After renaming a grid from "A" to "A1" and reopening, the grid keeps its GUID, and dimensions to it are still attached.

### 2.3.7 Openings, voids and fillings
**ID:** DM-IFC-007 · **Priority:** P0 · **Ref:** research/05 §2.2, §4.4; TECH_STACK §3.1

**Behaviour & rules.**
- When a hosted component cuts its host (doors, windows, wall openings, shafts, and components with an "opening cut" void), Reviera shall create an `IfcOpeningElement` (`PredefinedType = OPENING` or `RECESS`). It is linked to the host by `IfcRelVoidsElement` (`RelatingBuildingElement` = host, `RelatedOpeningElement` = opening).
- When a component fills that opening, `IfcRelFillsElement` (`RelatingOpeningElement` = opening, `RelatedBuildingElement` = component) shall link it.
- The opening's placement shall be relative to the host. The component's placement shall be relative to the opening (or to the host), so that moving the host in a third-party tool keeps the door in place.
- The host's `Body` representation written to IFC shall be the **uncut** body. Other tools apply the opening, as IFC intends. Reviera's own display uses the cut body (computed by OCCT). For coordination exports, the option "Write cut host geometry" (default off) writes the cut body instead.
- **Voids that cut several hosts** (for example a shaft through floors) produce one `IfcOpeningElement` per host. RXD element data (`hosting`) on each opening records which source element made each opening.
- **Non-cutting hosting** (a face-hosted light fixture on a ceiling, or a wall-hosted cabinet without a cut) shall be written as `IfcRelConnectsElements` (no connection geometry) between host and hosted element, so other tools can see the link. The host reference detail (stable reference string §2.8.3, face UV, offset) is in RXD element data (`hosting`).

**Acceptance criteria.**
- [ ] A door in a wall produces exactly one opening, one voids relationship and one fills relationship.
- [ ] A third-party viewer shows the wall with a hole at the door, using only the IFC relationships.
- [ ] Deleting the wall deletes its doors (after a confirmation that lists them) together with the opening entities and both relationships.

### 2.3.8 Materials, layer sets and profile sets
**ID:** DM-IFC-008 · **Priority:** P0 · **Ref:** research/05 §1.2, §4.8, §10; TECH_STACK §3.1

**Behaviour & rules.**
- **Layered system types** (basic walls, floors, roofs, ceilings) shall have an `IfcMaterialLayerSet` attached to the **type** through `IfcRelAssociatesMaterial`. Each layer is an `IfcMaterialLayer` with:
  - `Material` (`IfcMaterial`), `LayerThickness`, `IsVentilated` (for air layers);
  - `Name` = layer function (`Structure`, `Substrate`, `ThermalInsulation`, `Membrane`, `Finish1`, `Finish2`);
  - `Category` = `LoadBearing` / `Insulation` / `Finish`, following buildingSMART recommendations;
  - `Priority` = join priority (0–100), mapped from Reviera's layer priority (ARC chapter).
- Each **instance** of a layered type shall have an `IfcMaterialLayerSetUsage` attached through `IfcRelAssociatesMaterial`. It records:
  - `LayerSetDirection` = `AXIS2` for walls and `AXIS3` for slabs, roofs and ceilings;
  - `DirectionSense` = `POSITIVE` or `NEGATIVE`, depending on flip state;
  - `OffsetFromReferenceLine` = the signed distance from the wall location line (or slab reference plane) to the first layer;
  - `ReferenceExtent` = the wall height, where it applies.
- Layer data IFC cannot hold (core boundaries, wrapping at inserts and ends, "variable" layer flags, structural-material flag, stacked-wall and sweep/reveal definitions) is in RXD element data (`types`), keyed by type GlobalId (ARC chapter).
- **Profiled types** (beams, columns, mullions, braces) shall use `IfcMaterialProfileSet` on the type and `IfcMaterialProfileSetUsage` on the instance. `CardinalPoint` maps from the Reviera justification.
- **Components with several materials** (doors, furniture) shall use `IfcMaterialConstituentSet` on the type. There is one constituent per material parameter (for example "Frame", "Panel", "Glass"), and its `Name` is the parameter name.
- **Instance material overrides** (a material bound to an instance parameter) shall be written as a constituent set on the occurrence.
- `<By Category>` materials take the subcategory or category material from Object Styles (§2.10.3), and that resolved material is the one written.

**Acceptance criteria.**
- [ ] A 3-layer exterior wall type round-trips with identical layer order, thicknesses, materials and core boundaries.
- [ ] Flipping a wall instance flips `DirectionSense`. A third-party viewer that renders layers shows the finish on the other side.
- [ ] A door family with "Frame Material" and "Panel Material" parameters exports an `IfcMaterialConstituentSet` with two constituents named after the parameters.

### 2.3.9 Other relationships
**ID:** DM-IFC-009 · **Priority:** P0 (joins, aggregation) / P1 (others) · **Ref:** research/05 §1.4, §2.2, §2.3

| Reviera relationship | IFC | RXD |
|---|---|---|
| Wall-to-wall joins | `IfcRelConnectsPathElements` (`ATSTART` / `ATEND` / `ATPATH`) | element data `joins`: join style (butt, miter, square-off), disallow-join flags |
| Join Geometry / Cut Geometry | `IfcRelConnectsElements` | element data `joins`: join order, cutting element |
| Wall attached to roof or floor (top/base) | `IfcRelConnectsElements` | element data `constraints` (§2.4.4) |
| Curtain wall → panels, mullions | `IfcRelAggregates` | element data `types` on the curtain wall type: curtain grid definition |
| Stair → runs, landings, supports | `IfcRelAggregates` | element data `sketch`: stair run and boundary recipes |
| Roof → roof slabs | `IfcRelAggregates` | element data `sketch` |
| Shared nested family instance | `IfcRelAggregates` (parent occurrence → nested occurrences) | §2.8.10 |
| Room bounding | `IfcRelSpaceBoundary` (first level; second level on export option) | element data `elements`: room-bounding flags |
| Covering applied to element or space | `IfcRelCoversBldgElements` / `IfcRelCoversSpaces` | — |
| MEP connections | `IfcRelConnectsPorts`, `IfcRelNests` (element → `IfcDistributionPort`) | MEP chapter |
| Structural analytical association | `IfcRelAssignsToProduct` | STR chapter |
| Element ↔ classification | `IfcRelAssociatesClassification` | — |
| Element ↔ external document | `IfcRelAssociatesDocument` | — |
| Group, workset, design option, view membership | Model groups only: `IfcRelAssignsToGroup` | element data `membership`; RXD documents `model/groups`, `annotation` |

**Acceptance criteria.**
- [ ] Two walls joined in an L produce one `IfcRelConnectsPathElements` with ATEND/ATSTART, and the join (including its style) round-trips.

### 2.3.10 Geometry representation
**ID:** DM-IFC-010 · **Priority:** P0 · **Ref:** research/05 §1.1, §4.4, §4.7; TECH_STACK §3.2

**Behaviour & rules.**
- **Body** representations shall use the simplest IFC form that is exact:
  1. `SweptSolid` (`IfcExtrudedAreaSolid`, `IfcRevolvedAreaSolid`) for extrusions and revolves;
  2. `Clipping` (`IfcBooleanClippingResult` with `IfcHalfSpaceSolid`) for walls cut by roofs or sloped tops;
  3. `CSG` / boolean results for simple void cuts inside families;
  4. `AdvancedSweptSolid` (`IfcSurfaceCurveSweptAreaSolid`, `IfcSweptDiskSolid`, `IfcFixedReferenceSweptAreaSolid`) for sweeps;
  5. `Brep` (`IfcFacetedBrep`) or `AdvancedBrep` (`IfcAdvancedBrep`) for blends, lofts and free forms;
  6. `Tessellation` (`IfcPolygonalFaceSet`, `IfcTriangulatedFaceSet`) for mesh imports and as a fallback.
- **Axis** (`Curve2D`/`Curve3D`) shall be written for walls, beams, columns, members, ducts and pipes. **FootPrint** shall be written for slabs and spaces. **Box** (`IfcBoundingBox`) shall be written for every product.
- **Plan-symbolic geometry** of types (door swings, plan-only symbols) shall be written in the `Plan` context `Annotation` sub-context as `IfcAnnotationFillArea`/`IfcPolyline`/`IfcIndexedPolyCurve` on the type's `RepresentationMaps`. This is building content that IFC can express, so it stays in IFC. The visibility rules that choose *which* symbolic geometry to show are in the family definition (RXD).
- **Detail-level variants** (Coarse/Medium/Fine) are a Reviera concept. The **Fine** body is written to the `Body` sub-context. Coarse and Medium bodies, where they differ, are written as additional `IfcShapeRepresentation`s with `RepresentationIdentifier = 'Body'` in **additional sub-contexts** named `Body-Coarse` / `Body-Medium` (target view `MODEL_VIEW`). This keeps all geometry in IFC. Reviera reads them back by sub-context name.
- **Styling.** Surface styles come from materials (§2.10.5) through the material's `IfcMaterialDefinitionRepresentation`, or through `IfcStyledItem` on body items.
- **Precision.** `IfcGeometricRepresentationContext.Precision` = 1e-5 m. OCCT tolerances are set from it (§2.6.5).

**Acceptance criteria.**
- [ ] A straight wall without cuts is written as `IfcExtrudedAreaSolid`, not as a B-rep.
- [ ] A family with Coarse/Fine variants round-trips both variants.
- [ ] A third-party viewer that does not understand the extra sub-contexts displays the Fine body only.

### 2.3.11 Phases, design options and worksets (storage)
**ID:** DM-IFC-011 · **Priority:** P1 · **Ref:** research/05 §2.4; TECH_STACK §3.1, §3.7.2

**Behaviour & rules.**
- **Phases** and **phase filters** are RXD-only: RXD document `project/phases` holds the ordered phases, the phase filters and the project's current phase. Each element's created/demolished phase is element data `phasing`.
- The IFC **Status** property of each element's `Pset_*Common` shall be *derived* on save from the element's phases *relative to the current phase*: `NEW` (created in it), `EXISTING` (created earlier, not demolished), `DEMOLISH` (demolished in it), `TEMPORARY` (created and demolished in it). On reading a file with no RXD, phases are inferred from Status: `EXISTING` → phase "Existing", everything else → phase "New Construction".
- **Design option sets and options** are RXD-only (RXD document `project/design_options`; each element's membership is element data `membership`). On save, the IFC file contains elements of the main model and of **every** option. Non-primary option elements are listed in RXD so Reviera can hide them. On *export* to an exchange MVD (§2.11.4), only the main model and primary options are written, unless the user chooses otherwise.
- **Worksets and ownership** are RXD-only (RXD document `project/worksets`, element data `membership`; COL chapter).

**Acceptance criteria.**
- [ ] With the current phase set to "New Construction", a wall created in "Existing" and demolished in "New Construction" exports `Status = DEMOLISH`.
- [ ] Opening the `.ifc` alone in a third-party viewer shows elements of secondary design options. Opening it in Reviera hides them in views set to show the primary option.

---

## 2.4 Storage split: standard IFC and Reviera Extension Data (RXD)

### 2.4.1 Principles
**ID:** DM-RXD-001 · **Priority:** P0 · **Ref:** research/05 §2.3, §13.1 (extensible storage), §16.9; TECH_STACK §3.1, §3.7; spec/02a_extension_data_format.md

**Purpose.** IFC cannot hold parametric intent: constraints, sketches, family formulas, view and sheet definitions, or the meaning of annotation. RXD stores that intent with the model.

**Behaviour & rules.**
- Physical carriage (property-set names, JSON or compressed chunks, sizes, schema versions, validation, the alternative `.reviera` sidecar carrier) is defined in `spec/02a_extension_data_format.md`. This chapter does not redefine it.
- **Scope rule.** Data about one IFC element, which is meaningless without that element, is **RXD element data** on that element. Everything else is a record in a project-scoped **RXD document**. Examples:
  - a floor's sketch, a wall's attach constraints, a door's flip flags, an element's phase and design-option membership → RXD element data;
  - a view, a sheet, a dimension, a phase definition, an object style, a family recipe → RXD document records.
- **References.** Every RXD reference to an IFC element shall use its `GlobalId`. References to non-rooted IFC resources (grid axes, materials, profiles) use a composite key (for example `<IfcGrid GlobalId>/<AxisTag>`) or the RXD GUID assigned to that resource (§2.5). RXD-only elements reference each other by their GUID.
- **References to sub-element geometry** (a face of a wall, the "Left" reference of a door) use **stable reference strings** (§2.8.3).
- **Ownership.** Each RXD document record has exactly one owner: an IFC element GUID, or an RXD-only element GUID. When the owner is deleted, its records are deleted in the same transaction. Element data is deleted with its element automatically. This keeps BIMserver merges by GlobalId correct (COL chapter).
- **Orphans.** On open, RXD records whose owner GUID is missing from the IFC (for example, because another tool deleted the element) shall be listed in an **Extension Data Reconciliation** report. The user can drop them (default) or keep them, inactive, for a later merge.
- **Edits outside Reviera.** For each IFC-backed element with intent, RXD element data keeps a BLAKE3 hash of its IFC placement and body (`identity.geometry_hash`). If another tool changed the IFC so that the hash no longer matches, Reviera shall flag the element as "edited outside Reviera" and offer to **keep the IFC** (dropping conflicting intent, for example a sketch) or to **reapply intent** (regenerating from RXD and overwriting the outside edit).
- **Forward compatibility.** Unknown documents and unknown keys are kept and written back unchanged (`02a`).

**Acceptance criteria.**
- [ ] Deleting a floor in a third-party tool and reopening in Reviera removes its element data with it. Constraints in RXD documents that referenced it are listed as orphans, and dropping them leaves no dangling references.
- [ ] A floor whose extrusion depth was changed in another IFC editor is flagged on open, and both resolutions work.

### 2.4.2 What goes where
**ID:** DM-RXD-002 · **Priority:** P0 · **Ref:** research/05 §1–§10; spec/02a_extension_data_format.md

| Data | Standard IFC | RXD (element data key, or document name; `02a` is authoritative) |
|---|---|---|
| Spatial structure, storeys | ✔ | document `datums` (non-storey levels, level display data) |
| Grids | ✔ `IfcGrid`/`IfcGridAxis` | document `datums` (axis identity, extents, bubbles) |
| Reference planes, scope boxes | — | document `datums` |
| Elements, types, geometry, placements | ✔ | element data `placement` (flip flags, work plane) and `elements` (room-calc point, ambiguous category); on types, element data `types` (layer/curtain extras) |
| Materials, layer/profile/constituent sets | ✔ (+ `IfcMaterialProperties`, `IfcSurfaceStyle`) | document `materials` (appearance beyond IFC, graphics patterns, carbon record metadata §2.10.5) |
| Standard Psets/Qtos | ✔ | — |
| User parameter **values** | ✔ (as property sets) | — |
| Parameter **definitions** (data type, group, formula, GUID, tooltip, bindings) | — | document `parameters` |
| Global parameters (definitions, values, formulas, associations) | — | document `parameters` |
| Classifications | ✔ `IfcClassification`/`IfcClassificationReference` | — |
| Openings, voids, fills | ✔ | element data `hosting` |
| Joins | ✔ (connection relationships) | element data `joins` |
| Constraints, locks, EQ, labelled dimensions | — | element data `constraints` (IFC owner) or the owning record in document `annotation` (dimension owner) |
| Sketches and edit-sketch recipes | — | element data `sketch` |
| Families: parametric definition | `IfcProjectLibrary` + types + `RepresentationMaps` | documents `families/<family-guid>`, `families/index` |
| Lookup tables | — | inside the family document (or an RXD asset, `02a`) |
| Views, view templates, filters, graphic overrides | — | document `views` |
| Sheets, viewports, title-block instance data, sheet collections | — | document `sheets` |
| Schedules | — | document `schedules` |
| View-specific annotation and details | — | document `annotation` |
| Revisions | — | document `revisions` |
| Object styles, line weights, line styles, line patterns, fill patterns | — | document `styles` |
| Phases, phase filters | Derived `Status` only | document `phasing` (definitions); element data `phasing` (created/demolished) |
| Design options | Elements only | document `design_options` (sets, options); element data `membership` |
| Worksets, ownership | — | document `worksets` (definitions); element data `membership` (COL) |
| Model groups, detail groups | `IfcGroup` for model group instances | document `groups` |
| Links registry | Optional `IfcDocumentReference` | document `links` (COL) |
| Units (display) | SI `IfcUnitAssignment` | document `units` |
| Project information | ✔ `IfcProject`, `IfcSite`, `IfcBuilding`, `IfcPostalAddress`, `IfcOrganization`, Psets | — |
| Location: geo-referencing | ✔ `IfcMapConversion`, `IfcProjectedCRS`, `IfcSite` lat/long, `TrueNorth` | document `location` (named shared-coordinate sites, weather) |
| Project, snap, browser-organisation, starting-view and UI settings | — | document `settings` |
| Warnings | — | document `warnings` |
| Identity: internal IDs, change stamps, geometry hashes | — | element data `identity`; document `identity` for RXD GUIDs of non-rooted resources |
| Document scripts | — | document `scripts` (§2.13.8) |
| Images and other binaries | `IfcImageTexture`/`IfcBlobTexture` for material textures | RXD assets (`02a`) |

### 2.4.3 Reconciliation
**ID:** DM-RXD-003 · **Priority:** P0 · **Ref:** spec/02a_extension_data_format.md; TECH_STACK §3.7

**Behaviour & rules.**
- On every open, Reviera shall run the integrity checks that `02a` defines. Their outcomes are shown to the user like this:
  - **RXD present and consistent:** open normally.
  - **No RXD found** (a third-party file, or a file whose `Reviera_*` property sets were stripped): open as an IFC-only model (§2.11.1) and tell the user. If an external `.reviera` sidecar for this file is found beside it, offer to attach it.
  - **RXD belongs to another project** (identity mismatch as defined in `02a`): refuse to bind it. Offer to open IFC-only, or to choose another sidecar.
  - **RXD stale or partial** (for example, IFC edited by another tool after the last Reviera save): open with the RXD, then run reconciliation (§2.4.1) and show which elements and documents are affected.
- The reconciliation report lists orphans, hash mismatches and documents that failed schema validation. Each item has a resolution, and every resolution can be undone.

**Commands.** `rxd.status() -> {present, carrier, schema_version, orphans, mismatches, invalid_documents}` · `rxd.reconcile(mode: report|apply, drop_orphans?=true) -> report` · `rxd.attach_sidecar(path)` · `rxd.export_sidecar(path)`

**Acceptance criteria.**
- [ ] A project saved by Reviera, then re-saved by a tool that strips unknown property sets, opens as IFC-only. Attaching the sidecar exported before the strip restores views and annotation.
- [ ] Pairing a project's IFC with another project's sidecar is refused.

### 2.4.4 Constraints
**ID:** DM-RXD-004 · **Priority:** P0 (locks, alignments, level constraints) / P1 (equality, labelled dimensions, global-parameter links) · **Ref:** research/05 §2.3, §4.5

**Behaviour & rules.**
- Constraints are stored with their owner: as RXD element data `constraints` when the owner is an IFC element, or inside the owning record (for example a dimension in RXD document `views/annotation/<viewId>`). Each record has this shape (informative; the schema is in `02a`):

```json
{
  "id": "1kP0…",                    // GUID of the constraint (22-char IFC format)
  "owner": "0aZ1…",                 // owning element GUID (IFC or RXD-only)
  "kind": "align|distance|equal|angle|attach_top|attach_base|level_offset|host|parallel|tangent|fixed",
  "refs": ["0aZ1…#face:exterior", "2Hq…#ref:Left"],  // stable references, §2.8.3
  "value": 0.0,                     // distance (m) or angle (rad)
  "param": "CorridorWidth",         // optional: label (family, project or global parameter)
  "locked": true,
  "reporting": false
}
```

- The owner is chosen like this: a dimension owns its distance, equality and label constraints. The *moving* element owns an alignment lock (the one that was aligned onto the target). A wall owns its attach and level constraints.
- When a referenced element is deleted, its constraint records are removed in the same transaction. The removal is reported as a warning (§2.7.4) when the constraint was locked.

### 2.4.5 Sketches
**ID:** DM-RXD-005 · **Priority:** P0 · **Ref:** research/05 §2.3, §4.4

**Behaviour & rules.**
- RXD element data `sketch` holds, for every sketch-based element (floor, roof, ceiling, opening, filled region, stair boundary, in-place form), keyed by the owning element GUID:
  - `plane`: the work plane, as a reference or an explicit plane;
  - `curves`: each with an `id`, a kind (`line`, `arc`, `circle`, `ellipse`, `spline`, `point`), geometry in sketch-plane coordinates, and flags (`is_reference`, `defines_slope`, `slope`, `offset`, `overhang`) that the owning element interprets;
  - `loops`: an ordered list of curve IDs with orientation;
  - `constraints`: the same record format as §2.4.4, with references of the form `#curve:<id>:<start|end|mid>`;
  - `picked_from`: for curves created with "Pick Walls" or "Pick Lines", a reference to the source, so the curve follows it.
- The evaluated result is in IFC (for example, the slab's `IfcArbitraryProfileDefWithVoids`). If the RXD sketch is missing (for example, a downstream tool stripped it), *Edit Boundary* rebuilds a sketch from the IFC profile, without constraints or picked-from links.
- Sketch validation (closed loops, no self-intersection, no overlapping loops where they are not allowed) runs on **Finish Sketch**. Errors keep the user in sketch mode (§2.7.4).

**Acceptance criteria.**
- [ ] A floor sketched with "Pick Walls" follows the walls when they move, after the project has been saved and reopened.
- [ ] With the RXD stripped from the file, *Edit Boundary* on that floor gives an editable sketch that matches the slab outline.

---

## 2.5 Element identity

### Internal ID and GlobalId
**ID:** DM-ID-001 · **Priority:** P0 · **Ref:** research/05 §2.1, §16.1; TECH_STACK §3.3

**Purpose.** Every element needs a fast in-process key and a durable, globally unique key.

**Behaviour & rules.**
- **GUID.** Every IFC-backed element uses its `GlobalId`. This is a 128-bit UUID encoded as IFC's 22-character base-64 string (alphabet `0-9A-Za-z_$`). RXD-only elements get a GUID in the **same encoding**, so tools handle every GUID the same way.
  - New GUIDs shall be random UUID v4, or UUID v7 when the setting "time-ordered GUIDs" is on (this improves locality in BIMserver).
  - A GUID is never reused, and never changes over the life of the element. That includes save, reopen, schema upgrade, type swap, move between levels, conversion between storey and non-storey level, and BIMserver sync.
  - Copy, paste, duplicate, mirror-copy and array create new GUIDs. The source GUID is kept in element data `identity` (`source_guid`).
- **Non-rooted IFC resources** (materials, profiles, styles, grid axes) have no GlobalId in IFC. Reviera gives them an RXD GUID, mapped in RXD document `project/identity` to a resource key (for example `material:<Name>` or `<IfcGrid GlobalId>/<AxisTag>`). Renaming a material updates the key, not the GUID.
- **Internal ID** (`ElementId`) is an unsigned 64-bit integer, unique within one model and never reused within it:
  - It is persisted in element data `identity` (for IFC-backed elements) or in the element's record (for RXD-only elements). The next free value is in RXD document `project/identity`.
  - On opening a file with no RXD (a third-party file), IDs are assigned in ascending order of STEP instance number. This makes them deterministic for that file.
  - On merging another revision, or pasting from another model, any clashing internal IDs in the *incoming* elements are reassigned. A remap table is reported to Lua/MCP (`model.id_remap` event), so scripts holding IDs can update them. The GUID is the only key that is guaranteed across models.
  - IDs 1–999 are reserved for model singletons (project information, project settings). Negative values are reserved for categories and built-in parameters.
- **STEP instance numbers** (`#123`) are never used as identity. They may change on every save.
- The UI shall offer **IDs of Selection** (shows internal ID and GUID, both copyable) and **Select by ID** (accepts either).

**Commands.**
`element.get_ids(element_ids[]) -> [{id, guid}]` ·
`element.select_by_id(ids_or_guids[]) -> element_id[]` ·
`element.find_by_guid(guid) -> element_id?`

**Acceptance criteria.**
- [ ] 10,000 elements created, saved, reopened, saved again and reopened all keep identical GUIDs and internal IDs.
- [ ] Copying 50 elements creates 50 new GUIDs and IDs, each with `source_guid` pointing at its source.
- [ ] Merging two BIMserver revisions that each created an element with internal ID 5001 leaves both elements in the model, with distinct IDs, and emits a remap event.
- [ ] Select by ID accepts `3vB2Qe…` (22 characters) and `5001`, and selects the same element.

### Version stamps
**ID:** DM-ID-002 · **Priority:** P1 · **Ref:** research/05 §2.1 (document version)

**Purpose.** Caches, add-ins, agents and exporters need a cheap way to ask "has anything changed since I last looked, and what?" without diffing the model.

**Behaviour & rules.**
- The model shall keep a **model version**: a 128-bit episode GUID (new on each open or session start) plus a monotonically increasing transaction counter. Add-ins and MCP clients can read it to detect changes cheaply (`model.version`).
- The counter increases by exactly 1 for every committed transaction, including undo, redo, journal replay and merged BIMserver revisions. Rolled-back transactions and read-only commands do not change it. Undo does **not** restore an earlier counter value: undoing is itself a new change.
- Each element shall keep the transaction counter of its last change (in memory), and `last_modified_txn` / `last_modified_by` / `last_modified_at` in element data `identity` (persisted as `<episode>:<counter>`).
- "Change" means any facet of the element changed: parameters, placement, geometry after regeneration, type, host, memberships or its RXD element data. An element touched by regeneration only because a dependency was re-evaluated, but whose results are identical, is **not** marked changed.
- Deleted elements are tracked as tombstones (internal ID, GUID, counter of deletion) for the current episode, so a change query can report deletions.
- A version from a **different episode** (for example after reopening the file) cannot be compared element by element. `element.changed_since` then returns `{stale_episode: true}` and the caller must re-read everything. Within an episode, versions are totally ordered.
- `last_modified_by` holds the origin of the transaction (`user`, `lua:<script>`, `addin:<id>`, `mcp:<client>`, `sync:<author>`), never credentials.

**Data (storage).** The episode GUID and counter are in memory and in the live store (Ch 2b). Per-element stamps are persisted in RXD element data `identity` for IFC-backed elements, and in each record for RXD-only elements. They are not written to exchange exports (§2.11.4).

**Commands.** `model.version() -> {episode, counter}` · `element.changed_since(version) -> {changed[], deleted[], stale_episode}`

**Acceptance criteria.**
- [ ] `element.changed_since(v)` called right after `model.version()` returns empty lists. After a move, `changed` holds exactly the moved and regenerated elements.
- [ ] Undoing that move increases the counter again, and `element.changed_since` with the post-move version returns the same elements.
- [ ] Deleting a door reports its ID as deleted in the next `element.changed_since` call; calling with a version from before the last file open returns `stale_episode: true`.
- [ ] Changing a type parameter that does not affect the geometry of a dependent tag leaves the tag out of the changed list.

---

## 2.6 Hosting, the parametric dependency graph and regeneration

### 2.6.1 Hosting and dependent elements
**ID:** DM-HOST-001 · **Priority:** P0 · **Ref:** research/05 §2.2, §4.1

**Purpose.** Hosted elements follow their hosts, and deleting a host deals with its dependents in a predictable way.

**Behaviour & rules.**
- **Hosting modes** (fixed by the family template, §2.8.1): wall-, floor-, ceiling-, roof-, face-, line-, work-plane-, level- and two-level-based, and adaptive.
- **Moving or changing a host** moves or re-evaluates its hosted elements in the same transaction. A face-hosted element follows its host face through its stable reference (§2.8.3). If that face no longer exists after the change, the element becomes **orphaned**. It keeps its last world position and gets a warning, and the user can use *Pick New Host*.
- **Deleting a host** shall show a dialog listing its **dependents**, before the transaction commits:
  - hosted components and openings: deleted;
  - tags and view-specific annotation that refer only to deleted elements: deleted;
  - dimensions that refer to a deleted element: the reference is removed, and the dimension is deleted if it has fewer than two references left;
  - constraints: removed (with a warning for locked ones);
  - sketch lines picked from the host: unlinked, and their geometry kept.
- Elements hosted on **linked** model faces keep their position when the link is unloaded. They are flagged "host not available" until the link is reloaded (COL).
- Walls hosted on walls, attachments to roofs and floors, and other host rules come from domain chapters, and are expressed through the same dependency graph.

**Commands.**
`element.dependents(element_ids[]) -> {element_id, action}[]` ·
`element.delete(element_ids[], confirm_dependents?=true) -> element_ids[]` ·
`host.rehost(element_id, host_id, reference?)`

**Acceptance criteria.**
- [ ] Deleting a wall with 2 doors, 1 tag per door and a dimension to the wall face shows all 5 dependents with their actions, and *Cancel* leaves the model unchanged.
- [ ] Changing a host wall's thickness keeps hosted doors centred in the wall, or at the same offset from the reference face, as the family's placement rule says.

### 2.6.2 The dependency graph
**ID:** DM-REGEN-001 · **Priority:** P0 · **Ref:** research/05 §2.3, §3, §16.5; TECH_STACK §3.3

**Purpose.** "A change anywhere is a change everywhere" is put into practice by an explicit graph that records which derived values depend on which inputs.

**Behaviour & rules.**
- The core shall maintain a directed **dependency graph** in memory. It is rebuilt from the model on open, and kept up to date by every transaction. The graph is not saved to disk. Its sources (constraints, hosting, sketches, formulas) are saved in IFC and RXD.
- **Nodes** are fine-grained *facets* of elements, not whole elements. This avoids recomputing more than needed:

| Facet | Example |
|---|---|
| `params(e)` | Evaluated parameter values of element *e*, including formula results |
| `placement(e)` | Location point, curve or transform |
| `shape(e)` | Uncut solid of *e* (OCCT B-rep) |
| `cut_shape(e)` | Shape after joins, openings and void cuts |
| `profile(e)` | 2D boundary (sketch-based elements, rooms) |
| `quantities(e)` | Area, volume, length, Qto values |
| `annotation(a)` | Text values and geometry of annotation *a* |
| `view_cache(v)` | Visibility and display lists of view *v* (DOC) |

- **Edges** say "B must be recomputed when A changes". They come from:
  1. type → instance (`params(type)` → `params(instance)`, `shape(instance)`);
  2. formulas (each referenced parameter → the formula's parameter);
  3. global parameters → the parameters and dimensions associated with them;
  4. constraints (§2.4.4): references → constrained element placement;
  5. hosting: `shape(host)` → `placement(hosted)`; `placement(hosted)` → `cut_shape(host)` (openings);
  6. joins: `shape(a)`, `shape(b)` → `cut_shape(a)`, `cut_shape(b)`;
  7. level and datum constraints: `placement(level)` → base and top of constrained elements;
  8. room bounding: `cut_shape(bounding elements)` → `profile(room)` → `quantities(room)`;
  9. annotation references: referenced facets → `annotation(a)`;
  10. view dependencies: any visible facet → `view_cache(v)` (lazily, only for open views).
- **Cycles.** The graph must stay acyclic. A command that would create a cycle (for example, a formula `A = B + 1` when `B = A * 2`, or two walls each attached to the other) shall be **rejected** before commit. The error names the elements and parameters in the cycle (§2.7.4).
- **Strongly coupled groups.** Some inputs must be solved together (for example the reference planes and labelled dimensions in one family, or the curves of one sketch with their constraints). Each such group is a single **solver node**. Its members are solved together by the constraint solver (§2.6.3).
- The graph shall be inspectable for debugging: `regen.explain(element_id)` returns the upstream inputs and downstream dependents of an element's facets.

**Commands.** `regen.explain(element_id, facet?) -> {upstream[], downstream[]}`

**Acceptance criteria.**
- [ ] Changing one door type's width recomputes only that type's instances, their host walls' `cut_shape`, the affected rooms and the open views showing them. Instrumentation shows that no other elements were recomputed.
- [ ] Creating a circular formula reference is rejected with a message naming both parameters, and the model is unchanged.

### 2.6.3 Constraint solving
**ID:** DM-REGEN-002 · **Priority:** P0 (1-D dimensional constraints) / P2 (general 2D solver) · **Ref:** research/05 §2.3, §4.5, §16.5

**Purpose.** To solve locks, alignments, equalities and labelled dimensions.

**Behaviour & rules.**
- **v1 solver (P0/P1):** a 1-D solver, like the reference product. Constraints act on *distances along a direction* between parallel references (planes, lines, faces). Each solver node is split into independent axis groups (for example X, Y, Z, and each family-local direction). Each group is solved as a sparse linear system, with locked or labelled values as equations and current positions as soft targets (least change). Angular constraints on reference lines are solved in a second pass, with the same method on angles.
- **Over-constrained** or **inconsistent** systems shall be reported as an error that names the conflicting constraints and highlights them in the view. That is a clear improvement on a generic "constraints are not satisfied" message. Resolutions offered: *Remove constraint(s)* (choose which), *Unjoin*, *Cancel*.
- **Under-constrained** systems are allowed. Free references keep their current position.
- **P2:** a general 2D sketch solver (points, lines, arcs, with coincident, tangent, perpendicular, parallel, distance, radius and angle constraints). It is based on an open solver, for example the PlaneGCS solver (LGPL-2.1+) or SolveSpace's solver (GPL-3.0). It applies inside sketches and the family editor.
- The solver shall be deterministic. For the same inputs it returns bit-identical results.

**Acceptance criteria.**
- [ ] A family with Width and Depth labelled dimensions, plus an EQ constraint about the centre plane, flexes correctly through 20 random values in the valid range.
- [ ] Locking two contradictory distances between the same pair of planes reports an error naming both dimensions, and *Remove constraint* on either one resolves it.

### 2.6.4 Parallel regeneration
**ID:** DM-REGEN-003 · **Priority:** P0 · **Ref:** research/05 §3, §15.1 (single-threaded regeneration in the reference); TECH_STACK §3.2, §3.3

**Purpose.** To use all CPU cores for geometry, where the reference product mostly does not.

**Behaviour & rules.**
- **When.** Regeneration runs inside every transaction, before commit (§2.7.1). Lua scripts and commands may also call `regen.now()` part-way through a transaction to read updated values.
- **Algorithm.**
  1. Mark every facet that the transaction's changes touched as **dirty**, then propagate dirtiness downstream.
  2. Order the dirty subgraph into **topological levels** (Kahn's algorithm). Ties are broken by element internal ID, so the order is deterministic.
  3. Evaluate each level in parallel with **rayon** (`par_iter` over the level's facets). Each task reads only *committed or already-evaluated* upstream results, and writes its result into a **per-task output slot**. Tasks never write to shared state.
  4. After each level, merge the slots into the transaction's **staging area** in internal-ID order.
  5. After the last level, check the warnings and errors (§2.7.4). If there are no blocking errors, swap the staging area into the committed store **atomically**, in a single `ArcSwap`-style pointer swap per store partition, so readers (the UI, MCP resource reads) see either the old state or the new state, never a mix.
- **Determinism.** Parallel results shall not depend on thread count or scheduling. Floating-point reductions that cross tasks (for example the total area of a room from several boundary segments) shall be done in a fixed order after the parallel step.
- **Granularity.** Very small facets (parameter evaluation) are batched, so each rayon task does at least about 50 µs of work. Large booleans may themselves use OCCT's internal parallelism (§2.6.5).
- **Cancellation.** Long regenerations shall check a cancellation token between levels and inside long OCCT operations (`Message_ProgressRange` user break). Cancelling rolls the transaction back.
- **Progress.** Regenerations longer than 300 ms report progress to the UI status bar and to MCP progress notifications.
- **Background view work.** View caches (`view_cache(v)`) for views that are not visible are recomputed lazily, when a view is activated. They are not part of the transaction.

**Acceptance criteria.**
- [ ] Regenerating a 20,000-element benchmark model after changing a shared wall type is at least 3× faster on 8 cores than on 1 core (same machine).
- [ ] The same benchmark, run with 1, 2, 4, 8 and 16 threads, produces identical geometry hashes (element data `identity.geometry_hash`) for every element.
- [ ] Cancelling a long regeneration leaves the model exactly as it was before the command.

### 2.6.5 OCCT threading rules
**ID:** DM-REGEN-004 · **Priority:** P0 · **Ref:** TECH_STACK §3.2

**Purpose.** OCCT is not uniformly thread-safe. These rules make parallel use of it safe.

**Behaviour & rules.**
1. **No shared mutable shapes.** A `TopoDS_Shape` that is being built or modified is owned by exactly one task. Shapes in the committed store are **immutable**. A task that needs to change one first copies it (`BRepBuilderAPI_Copy`) or builds a new shape from it.
2. **Read-only sharing is allowed** only for committed, immutable shapes, and only for operations documented as read-only (for example bounding boxes or point classification on an already-meshed shape). Every operation that writes caches into a shape (for example triangulation stored on faces, or `BRepMesh_IncrementalMesh`) shall run on a task-owned copy.
3. **Per-thread tools.** Algorithm objects (`BRepAlgoAPI_*`, `BRepOffsetAPI_*`, `BRepMesh_*`, `ShapeFix_*`) are created per task, never shared or reused across threads.
4. **No global state changes during regeneration.** Global OCCT settings (for example `BRepAlgo` fuzzy values via static setters, `Standard::` allocator settings, message gravity) are set once at start-up and never changed while workers run.
5. **Memory manager.** OCCT shall be configured to use the system allocator in its thread-safe mode (`MMGT_OPT=0` or the build's equivalent; verify for the chosen OCCT version), set before the first OCCT call.
6. **Handles across threads.** `Handle(Standard_Transient)` reference counts are atomic in current OCCT releases, so passing handles of immutable objects between threads is allowed. This shall be confirmed by a stress test for the OCCT version that is pinned.
7. **Internal parallelism.** Boolean and meshing algorithms support `SetRunParallel(true)`. Reviera shall enable it only when the current regeneration level has fewer tasks than the number of worker threads, to avoid oversubscription. OCCT's thread pool (`OSD_ThreadPool`) shall be sized to the rayon pool size.
8. **Tolerances.** Fuzzy values and tolerances come from the project precision (1e-5 m, §2.3.10). Boolean failures are retried once with a fuzzy value 10× larger. If that also fails, the element gets a warning, and the uncut shape is shown.
9. **Isolation of crashes.** OCCT exceptions (`Standard_Failure`) are caught at the FFI boundary and become element warnings. A failure in one task never aborts other tasks.

**Acceptance criteria.**
- [ ] A ThreadSanitizer/stress test runs 1,000 random boolean regenerations on 16 threads with no data races reported in Reviera code or in the FFI layer.
- [ ] A deliberately failing boolean (coincident faces) produces one element warning, and the other 999 elements regenerate normally.

---

## 2.7 Transactions, undo/redo, warnings and errors

### 2.7.1 Transactions
**ID:** DM-TXN-001 · **Priority:** P0 · **Ref:** research/05 §3, §16.6; TECH_STACK §3.3

**Purpose.** Every change is atomic, named and undoable.

**Behaviour & rules.**
- **Transaction.** Every command runs in exactly one top-level transaction, named after the command (for example "Wall", "Move", "Modify type attributes"). A transaction has the states *Started → (Regenerating) → Committed | RolledBack*.
- **Sub-transaction (savepoint).** A transaction may create nested savepoints. Each can be rolled back on its own, without ending the outer transaction. Savepoints do not appear in the undo history.
- **Transaction group.** A group bundles several transactions:
  - **Assimilate** merges them into a single undo item with the group's name (for multi-step tools such as "Create stair", or an MCP agent's multi-step task).
  - **Commit** keeps them as separate undo items.
  - **Rollback** undoes all of them.
- **Serialisation.** Only one transaction can be open on a document at a time. Requests from the UI, Lua and MCP are queued on the **command queue** and run in order (TECH_STACK §3.3). Read-only queries do not need a transaction, and run concurrently against the last committed snapshot.
- **Contents.** A transaction records a **change set**: elements added, modified (per facet and per parameter, with before and after values) and deleted, plus RXD records added, changed and removed. This change set drives undo, events, updaters (§2.13.5) and BIMserver sync (COL).
- **Failure → rollback.** An unhandled error, a panic in a command, a Lua error or a blocking failure (§2.7.4) rolls the transaction back completely.
- **Read-only mode.** Commands declared `effect = read` (§2.12) run without a transaction. Trying to write from them is an error.

**Commands.** These are internal primitives, exposed to Lua (§2.13.4) and MCP (§2.16.4), not as stand-alone tools: `txn.begin(name)`, `txn.commit()`, `txn.rollback()`, `txn.savepoint() -> sp`, `txn.rollback_to(sp)`, `txn.group_begin(name)`, `txn.group_end(mode: "assimilate"|"commit"|"rollback")`.

**Acceptance criteria.**
- [ ] A Lua script that creates 10 walls and then raises an error leaves no walls in the model, and no undo item.
- [ ] A group of 3 transactions assimilated as "Create stair" shows as one undo item, and one undo removes all three transactions' changes.
- [ ] During a long transaction started by MCP, a read-only MCP resource read returns the last committed state without waiting.

### 2.7.2 Undo and redo
**ID:** DM-TXN-002 · **Priority:** P0 · **Ref:** research/05 §3

**Purpose.** Multi-level undo and redo of named operations.

**UI.** Quick Access Toolbar › Undo and Redo split buttons. Each drop-down lists the transaction names, newest first, and each entry shows its source (UI, Lua script name, add-in ID or MCP client name). Choosing an entry undoes or redoes everything up to and including it. Shortcuts: Ctrl+Z / Ctrl+Y (UI chapter).

**Behaviour & rules.**
- Undo is stored as **inverse change sets** (not full snapshots). Memory use is bounded by a setting (default 512 MB, or 100 items, whichever is reached first). The oldest items are dropped when the limit is reached.
- Undo **survives save** in Local mode (a difference from the reference product).
- Undo is **cleared** by: closing the document; *Get latest* / *Sync* in BIMserver mode (the merge changes the base); purge with "compact undo"; and schema conversion. The user is warned before each of these.
- Any new transaction clears the redo stack.
- Undo and redo are themselves transactions for events and updaters: `document.changed` fires with `undo = true`. Updaters do **not** run on undo or redo, because their effects are already part of the recorded change set.
- Every undo item records its **origin**: `{source: "ui"|"lua"|"addin"|"mcp", actor, session_id}`. MCP undo tools can only undo items from their own session, unless they have the *full* permission level (§2.16.5).

**Commands.** `edit.undo(steps=1)` · `edit.redo(steps=1)` · `edit.history() -> [{name, origin, time}]`

**Acceptance criteria.**
- [ ] After 50 mixed edits, undo of all 50 returns the model to a state whose IFC export is semantically identical to the original.
- [ ] Undo after save, in Local mode, works, and the model shows as modified again.
- [ ] An MCP session with *model-edit* permission cannot undo a UI user's transaction.

### 2.7.3 Change journal and recovery
**ID:** DM-TXN-003 · **Priority:** P1 · **Ref:** research/05 §11.1 (journals, backups)

**Behaviour & rules.**
- Reviera shall write a **session journal**: an append-only log of every executed command with its parameters, its origin and its result. It is in JSON Lines (`journal-<date>-<pid>.jsonl` in the user's state directory). It is used for diagnostics and crash recovery. MCP tool calls are logged with the client name.
- **Crash recovery.** Reviera writes an **autosave** of the model every N minutes (default 10) to a recovery file, in the background, from a committed snapshot. On the next start after a crash, it offers to restore the autosave and replay the journal entries since the autosave.
- Journal replay can also be used as a **macro recording** source (§2.13.8).
- Journals never contain credentials or MCP authentication tokens.

**Commands.** `journal.replay(path, stop_on_error=true)`

**Acceptance criteria.**
- [ ] Killing the process after 20 edits and restarting offers recovery. Accepting gives a model that matches the state after the 20th edit.

### 2.7.4 Warnings, errors and failure handling
**ID:** DM-FAIL-001 · **Priority:** P0 · **Ref:** research/05 §2.3, §3

**Purpose.** Problems found during a transaction are reported consistently and can be resolved by users or by code.

**Behaviour & rules.**
- Every problem is a **failure message** created from a **failure definition** in a registry. Each definition has a stable ID (for example `dm.constraint.unsatisfied`, `arc.wall.overlap`, `dm.regen.boolean_failed`), a severity, a message template, the elements involved and the resolutions it offers.
- **Severities:**

| Severity | Effect | Stored | Example |
|---|---|---|---|
| **Info** | Shown briefly in the status area | No | "Family loaded with 3 types" |
| **Warning** | Transaction commits. A non-modal warning pop-up lists the warnings and their elements. | Yes, in RXD document `project/warnings` | Overlapping walls; room not enclosed; boolean failed |
| **Error** | Transaction cannot commit until it is resolved. A modal dialog offers the resolutions and *Cancel*. | No | Constraints cannot be satisfied; sketch not closed; circular reference |
| **Fatal** | Transaction rolls back. No resolution. | No | Internal invariant broken |

- **Resolutions** are typed actions, such as *Remove constraint*, *Unjoin elements*, *Delete element(s)*, *Disallow join*, *Keep editing (sketch)*. Applying a resolution re-runs regeneration inside the same transaction.
- **Stored warnings** are re-checked on every regeneration of the elements involved. A warning disappears when its condition is gone. The warnings of a model are listed in **Review Warnings** (Manage › Inquiry): grouped by definition, with element IDs, *Show* (zooms to the elements), *Delete checked* and *Export* (HTML/CSV).
- **Programmatic handling.**
  - Lua and add-ins can register a **failure preprocessor** for a transaction. It receives every failure before the UI does, and can *dismiss* a warning, *resolve* an error with one of its resolutions, or let it continue to the UI.
  - In MCP and other non-interactive contexts there is no UI: warnings are returned in the tool result, and errors fail the call with their resolutions listed. The agent can then call again with `failure_policy`: `{"<failure-id>": "<resolution>"}`.
- Each command result shall include `warnings[]` (ID, message, element IDs) so that scripts can react.

**UI.** Warning pop-up (bottom right, non-modal, with "Show", "More info", "Expand"). Error dialog (modal, with resolution buttons). Manage › Inquiry › Review Warnings. A warning count badge in the status bar.

**Data (storage).** Stored warnings: RXD document `project/warnings` (failure definition ID, element GUIDs, first-seen time, dismissed state).

**Commands.**
`warnings.list(filter?: {kind?, severity?, element_ids?}) -> warning[]` ·
`warnings.show(warning_id)` ·
`warnings.dismiss(warning_ids[])` (sets the same stored acknowledged state in `project/warnings` as Chapter 6's `warnings.acknowledge`; the two are aliases) ·
`warnings.export(path, format: html|csv) -> ()` ·
`failure.set_policy(policy)` (per transaction or per session)

**Acceptance criteria.**
- [ ] Two overlapping walls produce a stored warning that is listed in Review Warnings, survives save and reopen, and disappears when one wall is moved away.
- [ ] An MCP tool call that causes a `dm.constraint.unsatisfied` error fails, lists the resolutions, and leaves the model unchanged. Repeating the call with `failure_policy: {"dm.constraint.unsatisfied": "remove_constraints"}` succeeds and reports which constraints were removed.
- [ ] A Lua failure preprocessor that dismisses `arc.wall.overlap` lets a batch script create overlapping walls with no pop-ups, and the warnings are still stored.

---

## 2.8 The Reviera Family Editor

The **Reviera Family Editor** is a document mode for creating and editing loadable families. It opens a *family document*. That is an in-memory model of the same kind as a project, with its own transactions and undo, but holding one family definition instead of a building. In-place families use the same tools inside a project (§2.2.3).

### 2.8.0 Family Editor environment
**ID:** DM-FED-000 · **Priority:** P1 · **Ref:** research/05 §4 (intro), §4.9; screenshots/platform/family_types_dialog_parameter_groups.png, screenshots/platform/family_types_cabinet_variations_2026.png

**Purpose.** A dedicated environment that looks like the project environment but offers family-authoring tools.

**Workflow.**
1. **File › New › Family**, then choose a family template (§2.8.1). Or open a family package. Or select an instance in a project and choose **Edit Family**.
2. Draw reference planes, add dimensions and label them with parameters, and create forms locked to the planes.
3. Open **Family Types**, create types, and flex the parameters.
4. Choose **Load into Project** (or **Load into Project and Close**), or **Save** to a family package.

**UI.**
- The document opens in its own view windows, with a Project Browser listing the family views: *Ref. Level* plan, ceiling plan, Front/Back/Left/Right elevations, and a default 3D view. Its title-bar badge reads "Family: <name>".
- **Create** tab panels:
  - *Properties*: Family Types, Family Category and Parameters, Type Properties;
  - *Forms*: Extrusion, Blend, Revolve, Sweep, Swept Blend, and Void Forms (the same five as voids);
  - *Model*: Model Line, Component (nest), Model Text, Opening (host templates only), Model Group;
  - *Control*: Flip Control;
  - *Connectors*: Electrical, Duct, Pipe, Cable Tray, Conduit (MEP templates);
  - *Datum*: Reference Plane, Reference Line, Reference Point (conceptual and adaptive);
  - *Work Plane*: Set, Show, Viewer;
  - *Family Editor*: Load into Project, Load into Project and Close.
- **Annotate** tab: Aligned/Angular/Radial/Diameter/Arc-length dimensions, Label (annotation families), Symbolic Line, Detail Component, Filled Region, Masking Region, Text.
- **Modify** tab: the standard modify tools (ARC chapter) plus Align/Lock, Array (with a count parameter), Cut/Uncut Geometry, Join/Unjoin Geometry, and Visibility Settings for the selected geometry.
- The Properties palette shows the selected form's properties (extents, Visible, Visibility Settings, Material, Subcategory, Solid/Void). The **"="** button beside a property associates it with a family parameter.
- There is no Options Bar. Placement and drawing options are in the contextual tab (UI chapter; matches the reference's 2027 direction).

**Behaviour & rules.**
- A family document has the same transaction, undo, warning and regeneration behaviour as a project (§2.6, §2.7).
- **Load into Project** pushes the current definition into one or more open projects or family documents, chosen in a dialog. It uses the reload rules in §2.2.3. A family document opened from a project with *Edit Family* offers the source project first.
- Several family documents and projects may be open at once.

**Commands.**
`family_doc.new(template_path) -> document_id` ·
`family_doc.open(path) -> document_id` ·
`family_doc.save(document_id, path?)` ·
`family_doc.load_into(document_id, target_document_ids[], overwrite_values=false)`

**Acceptance criteria.**
- [ ] A user can create a parametric table family (four legs and a top driven by Width, Depth and Height), flex it through 3 types, load it into a project and schedule its types, without leaving the application.
- [ ] Undo in a family document never affects an open project, and the reverse is also true.

### 2.8.1 Family templates
**ID:** DM-FED-001 · **Priority:** P1 · **Ref:** research/05 §4.1, §11.1

**Purpose.** Templates fix a family's category, hosting behaviour, default references and dimensionality before any geometry exists.

**Behaviour & rules.**
- A family template is a family package (§2.8.14) with the template flag set in its family-package manifest (`02a`). By convention the file is named `<Name>.familytemplate.ifc`. Opening one always creates a new, untitled family document.
- A template fixes:
  - **category** (it can be changed later only within the template's compatibility set; for example Generic Model ↔ Furniture is allowed, Door → Window is not);
  - **placement mode**: `level`, `work_plane`, `face`, `wall_hosted`, `floor_hosted`, `ceiling_hosted`, `roof_hosted`, `line`, `two_level`, `adaptive`, `pattern_panel`, `view_specific`, `profile`, `title_block`, `annotation`, `mass`. This **cannot** be changed after creation, except that `level` families may be switched to `work_plane` with the *Work Plane-Based* flag (§2.8.2);
  - **dimensionality**: 3D, 2D detail, 2D annotation, 2D profile, title block, conceptual (CME, §2.8.13);
  - **default references and parameters**: for example Left/Right/Front/Back/Centre planes and Width/Depth labels; or, for door templates, a host wall with a default opening and Width/Height parameters.
- Reviera shall ship its own templates. They shall be authored from scratch (not copied from the reference product's content) and cover these groups:

| Group | Templates (metric and imperial variants) |
|---|---|
| Generic | Generic Model; Generic Model (face-based, line-based, wall-, floor-, ceiling- and roof-hosted, two-level-based); Generic Model Adaptive; Generic Model Pattern-Based |
| Architecture | Door; Door (curtain wall); Window; Window (curtain wall); Furniture; Furniture System; Casework; Plumbing Fixture; Specialty Equipment; Entourage; Planting; Baluster; Baluster Post; Baluster Panel; Railing Support; Railing Termination; Curtain Wall Panel; Column; Parking; Site |
| Structure | Structural Framing (beams and braces); Structural Framing (complex and trusses); Structural Column; Structural Foundation; Structural Stiffener; Structural Connection; Structural Truss (STR chapter) |
| MEP | Mechanical Equipment; duct and pipe fittings (elbow, tee, cross, transition, union, cap); Air Terminal; Electrical Equipment; Electrical Fixture; Lighting Fixture (ceiling, wall, linear); device templates (communication, data, fire alarm, security, nurse call); cable tray and conduit fittings (MEP chapter) |
| 2D | Detail Item; Detail Item (line-based); Profile; Profile (hosted); Profile (rail); Profile (mullion); Profile (stair nosing); Profile (reveal) |
| Annotation | Generic Annotation; Generic Tag; Multi-Category Tag; category tags (Door, Window, Room, Area, Space, Wall, …); Material Tag; Keynote Tag; Section Head; Elevation Mark; Level Head; Grid Head; Callout Head; View Title; Spot Elevation Symbol; Revision Tag; North Arrow; Stair Path; Span Direction Symbol |
| Title blocks | ISO A0–A4; ANSI A–E; ARCH A–E; Custom Size |
| Conceptual | Mass; Generic Model Adaptive; Curtain Panel Pattern-Based (rectangle, rhomboid, triangle bent/flat, hexagon, octagon, arrows, zigzag, half/third step) |

- Template search paths: the shipped template folder, the user template folder, and any folder listed in settings (UI chapter). Template sets are installed per locale, with the metric set as the default.
- Users can save any family document as a template (**Save As › Family Template**). The template keeps the family's category, placement mode, references and parameters. Geometry is kept only if the user chooses to keep it.

**Commands.** `family_template.list(locale?) -> template[]` · `family_doc.save_as_template(document_id, path, keep_geometry=false)`

**Acceptance criteria.**
- [ ] Each shipped template opens, flexes its default parameters without errors, and loads into an empty project, where its placement mode works (for example, a wall-hosted template places only on walls).
- [ ] An attempt to change a Door family's category to Windows is refused, and the message explains why.

### 2.8.2 Family category and family parameters
**ID:** DM-FED-002 · **Priority:** P1 · **Ref:** research/05 §4.2

**Purpose.** Family-level behaviour flags that apply to the whole family and cannot vary by type.

**Workflow.**
1. In a family document, Create › Properties › **Family Category and Parameters** (`family_doc.category_params()`).
2. Pick a category in the tree. The flag grid below refreshes to the flags that apply to it, keeping the values of flags both categories share.
3. Set flags. Choose **OK** to apply the category change and all flag changes as one transaction, or **Cancel**.

**UI.** Create › Properties › **Family Category and Parameters**: a category tree (filtered to the compatible set) and a property grid of family parameters. Only the flags that apply to the category are shown.
- The tree has a discipline filter (All, Architecture, Structure, Mechanical, Electrical, Piping) and a search field. Categories outside the template's compatibility set (§2.8.1) are not listed; a note under the tree says why ("Door templates can only be Doors").
- Each flag row shows its name, value control (check box, drop-down or text) and a tooltip with its meaning. Flags that the template fixes (for example `work_plane_based` for face-based templates) are shown greyed with the reason.

**Validation.**
- Changing the category is checked before it is applied. If geometry is assigned to subcategories of the old category, the dialog lists them and offers to **move** them to same-named subcategories of the new category (created if missing) or to the new category itself. Nested shared components and tag targets are unaffected.
- `ifc_class` / `ifc_predefined_type` drop-downs list only values allowed for the category (§2.2.2). `USERDEFINED` requires a non-empty text value, otherwise OK is disabled.
- `classification_code` must exist in a loaded classification system when one is active (§2.10.10); an unknown code is accepted only after confirmation and is flagged in the family's warnings.
- Turning `shared` off on a family that is nested as shared in an open project warns that its instances will stop being schedulable there after reload.

**Behaviour & rules.** These family parameters shall be supported:

| Flag | Applies to | Meaning |
|---|---|---|
| `work_plane_based` | Level-based 3D families | Instances host on a work plane or face when placed |
| `always_vertical` | Work-plane- and face-based | The instance stays vertical on a sloped host (default on for most categories) |
| `cut_with_voids_when_loaded` | 3D | Voids in other families and in-place elements may cut this family |
| `shared` | All | When nested, instances are separate, selectable, taggable and schedulable elements in projects (§2.8.10) |
| `room_calculation_point` | Furniture, equipment, doors, windows, … | Shows a movable point that sets which room or space the instance belongs to. Doors and windows also report To Room/From Room. |
| `part_type` | MEP and structural | Normal, Elbow, Tee, Cross, Transition, Cap, Union, Multi-port, Damper, Panelboard, Switchboard, Transformer, … (MEP/STR chapters) |
| `classification_code` / `classification_title` | All | A classification reference (§2.10.10) |
| `ifc_class` / `ifc_predefined_type` | Model families | Overrides the category's default IFC class and `PredefinedType` within the allowed set (§2.2.2) |
| `visible_in_plan` / `show_precut_in_plan` | Doors, windows, … | Plan representation behaviour |
| `maintain_annotation_orientation` | 3D | Nested annotation stays readable when the host is rotated in elevation |
| `can_host_rebar` | Structural | STR chapter |
| `structural_material_type`, `symbolic_cutback`, `stick_symbol_location` | Structural framing | STR chapter |
| `rotate_with_component`, `fixed_rotation` | Tags and annotation | Annotation behaviour |
| `load_classification`, `default_elevation`, `round_connector_dimension` | MEP | MEP chapter |
| `lock_rotation`, `moves_with_nearby_elements` | Some categories | Placement behaviour |

**Data (storage).** RXD document `families/<id>` (`flags`). `ifc_class`/`ifc_predefined_type` and the classification also go to IFC: the type's entity class, `PredefinedType` and `IfcRelAssociatesClassification`.

**Commands.** `family_doc.set_category(category)` · `family_doc.set_flag(flag, value)` · `family_doc.category_params()` (opens the dialog; its OK runs the two commands above inside one transaction)

**Acceptance criteria.**
- [ ] Changing a Generic Model family with a "Frame" subcategory to Furniture offers to move the geometry to "Furniture › Frame"; accepting creates that subcategory, and one undo restores the category and the assignments.
- [ ] A furniture family with `shared = on`, nested in a "Desk Set" family, gives individually schedulable furniture instances in a project. With `shared = off`, it gives none.
- [ ] Setting `ifc_predefined_type = USERDEFINED` with value "Kiosk" exports the type with `PredefinedType=USERDEFINED` and `ElementType='Kiosk'`.

### 2.8.3 Reference planes, reference lines and stable references
**ID:** DM-FED-003 · **Priority:** P1 (planes, lines, named references) / P0 (stable reference strings, needed by projects) · **Ref:** research/05 §4.3, §4.5; screenshots/platform/reporting_parameters_angles_2026.png

**Purpose.** References are the skeleton of a parametric family, and the anchor points that project dimensions and constraints attach to.

**Behaviour & rules.**
- **Reference planes** have: `name` (used to set the work plane and as a stable reference name), `is_reference`, `defines_origin`, `wall_closure` (door and window templates: where host layers wrap), `subcategory`.
- `is_reference` values:

| Value | Project behaviour |
|---|---|
| `not_a_reference` | Invisible to project dimensioning and snapping |
| `weak` | Can be dimensioned. Lower priority for temporary dimensions. |
| `strong` | Highest priority for temporary dimensions and snapping in projects |
| `left`, `center_lr`, `right`, `front`, `center_fb`, `back`, `bottom`, `center_elev`, `top` | *Named* references. They are strong, and are preserved across type swaps and family swaps (dimensions to "Left" survive a swap to any family that also has "Left"). |

- `defines_origin`: the three planes (one per axis) with this flag set define the insertion point. Changing them moves the geometry relative to the placement point. Placed instances keep their placement point when the family is reloaded.
- **Reference lines** have two ends, and four implied work planes (two along the line, one normal at each end). They can be driven by length and angle parameters, and are used for hinged and rotating rigs. In the CME they are references for forms (§2.8.13).
- **Reference points** (CME and adaptive families): free, hosted on a curve or face (parameterised by normalised length or UV), or *driving* (adaptive points, §2.8.13).
- **Stable reference strings.** Anything outside a family or element (project dimensions, constraints, hosted elements, tags) shall refer to sub-element geometry through a **stable reference string**. That string is not a geometry index. Its grammar:

```ebnf
stable_ref   = element_guid , "#" , ref_kind , ":" , ref_name , [ "/" , occurrence ] ;
ref_kind     = "ref"      (* named reference plane/line of a family: Left, Right, Center_LR, or a user name *)
             | "face"     (* semantic face of a system element: exterior, interior, core_exterior, core_interior, top, bottom, start, end *)
             | "edge" | "vertex"
             | "axis"     (* location line or axis *)
             | "form"     (* a face of a family form: <form-id>.<face-role> *)
             | "curve"    (* sketch curve: <curve-id>[:start|:end|:mid] *)
             | "point"    (* adaptive or reference point: <index or name> *) ;
ref_name     = name_char , { name_char } ;
occurrence   = digit , { digit } ;   (* nth occurrence, for repeated features *)
```

  System elements publish their semantic faces (domain chapters list them: for example, walls publish `exterior`, `interior`, `core_exterior`, `core_interior`, `top`, `bottom`, `start`, `end`, `axis`). Family forms publish face roles derived from the form recipe (for example an extrusion's `start`, `end` and `side.<sketch-curve-id>`). These do not change when topology changes, unless the recipe changes.
- Reference resolution failure (for example, a named reference that is missing after a family swap) shall produce a warning. The dependent constraint or dimension is removed or detached, as §2.6.1 describes.

**Data (storage).** RXD document `families/<id>` (`references[]`). Stable reference strings appear in RXD wherever references are stored.

**Commands.**
`family_doc.reference_plane.create(p1, p2, view_id, name?, is_reference="weak") -> element_id` ·
`family_doc.reference_line.create(p1, p2, view_id?) -> element_id` ·
`family_doc.reference.set(element_id, name?, is_reference?, defines_origin?, wall_closure?)` ·
`reference.resolve(stable_ref) -> {element_id, geometry}`

**Acceptance criteria.**
- [ ] A project dimension from a door's "Left" reference to a grid survives swapping the door to another door family that has a "Left" reference, and moves to the new reference position.
- [ ] Moving the planes that define the origin moves the family geometry relative to placed instances' insertion points, and each instance keeps its placement point after reload.

### 2.8.4 Forms: extrusion, blend, revolve, sweep, swept blend, and voids
**ID:** DM-FED-004 · **Priority:** P1 · **Ref:** research/05 §4.4, §4.7; screenshots/platform/formula_driven_sunshade_family_2026.png

**Purpose.** The five solid form types, plus their void counterparts, built from sketches and profiles.

**Workflow (example: extrusion).**
1. Create › Forms › **Extrusion**. Set the work plane if prompted.
2. Sketch one or more closed loops (the sketch-mode contextual tab offers line, rectangle, polygon, circle, arc, spline, pick lines, pick references). Lock sketch lines to reference planes with the padlock.
3. Set Extrusion Start and End in Properties, or with shape handles.
4. **Finish** (✓). The form is created and can be selected. Its handles can be locked to reference planes.

**Behaviour & rules.**

| Form | Inputs | Parameters | OCCT build (indicative) | IFC result |
|---|---|---|---|---|
| **Extrusion** | One work plane, one or more closed loops (nested loops become holes) | `start`, `end` (along the plane normal) | `BRepPrimAPI_MakePrism` | `IfcExtrudedAreaSolid` |
| **Blend** | Base sketch and top sketch on parallel planes (one closed loop each), editable vertex mapping | `first_end`, `second_end`, vertex connections, twist | `BRepOffsetAPI_ThruSections` (ruled) | `IfcAdvancedBrep` or `IfcFacetedBrep` |
| **Revolve** | Closed profile loop(s) plus an axis line in the same plane | `start_angle`, `end_angle` | `BRepPrimAPI_MakeRevol` | `IfcRevolvedAreaSolid` |
| **Sweep** | Path (sketched or picked: lines, arcs, splines; open or closed; 2D in v1, 3D in CME) plus one profile (sketched, or a loaded profile family and type) | profile `angle`, `flip`, horizontal/vertical `offset`, `trajectory_segmentation`, `max_segment_angle` | `BRepOffsetAPI_MakePipe` / `MakePipeShell` | `IfcSurfaceCurveSweptAreaSolid` / `IfcFixedReferenceSweptAreaSolid`, or B-rep |
| **Swept blend** | One path segment plus two profiles (one at each end) | profile angles, offsets, vertex mapping | `BRepOffsetAPI_MakePipeShell` with two sections | `IfcAdvancedBrep` |
| **Void** (each of the above) | Same | Same | Same, then boolean cut | Boolean result, or `IfcOpeningElement` when it cuts the host (§2.3.7) |

- Every form has these properties: `visible` (Yes/No, can be associated with a Yes/No parameter); visibility settings (§2.8.11); `material` (fixed, `<By Category>`, or associated with a Material parameter); `subcategory`; `solid_or_void`; and the extents above, each of which can be associated with parameters.
- **Voids** cut the solids in the same family that they intersect, automatically. *Cut Geometry* and *Uncut Geometry* control cutting explicitly, including cutting of nested shared components. In hosted templates, a void with "Cut host" creates an opening cut in the host (§2.3.7).
- **Join Geometry** inside a family merges solids for display (no seam lines) and for IFC export (one solid where the materials match).
- Each form's sketches use the sketch model and constraints of §2.4.5.
- **Shape points display** (`family_doc.shape_points.toggle()`, View Control Bar in family documents). Shows or hides, in the active family view, the glyphs of points that shape forms: sketch vertices of selected forms, blend vertex-mapping points, profile insertion points on sweep paths, reference points and adaptive points (with their index numbers, §2.8.13). It is view state only: not undoable, not saved in the family, and not shown in projects. It is on by default in the conceptual environment and off elsewhere. Hidden points can still be picked with Tab.
- Other family modelling tools: **Model Line**, **Model Text** (font, size, depth, text parameter), **Opening** (host templates), **Array** (linear and radial; count can be an Integer parameter, including 0 and 1), **Flip Control** (an arrow that flips the instance in projects), **Component** (nesting, §2.8.10), **Detail Component**, **Filled Region**, **Masking Region**, **Symbolic Line** (§2.8.11).

**Data (storage).** RXD document `families/<id>` (`forms[]`, each with a recipe: kind, sketch IDs, path, profile reference, parameters, visibility, material, subcategory). The evaluated geometry for each type is in IFC (`RepresentationMaps`; §2.8.14).

**Commands.**
`family_doc.form.extrusion(sketch, start, end, solid=true) -> element_id` ·
`family_doc.form.blend(base_sketch, top_sketch, first_end, second_end, vertex_map?, solid=true) -> element_id` ·
`family_doc.form.revolve(sketch, axis, start_angle, end_angle, solid=true) -> element_id` ·
`family_doc.form.sweep(path, profile, profile_options?, solid=true) -> element_id` ·
`family_doc.form.swept_blend(path, profile_start, profile_end, options?, solid=true) -> element_id` ·
`family_doc.form.set(element_id, props)` ·
`family_doc.geometry.cut(cutting_id, target_id)` · `family_doc.geometry.uncut(cutting_id, target_id)` ·
`family_doc.geometry.join(a_id, b_id)` · `family_doc.geometry.unjoin(a_id, b_id)` ·
`family_doc.array.create(element_ids[], kind: "linear"|"radial", count, spacing_or_angle, count_param?) -> element_id` ·
`family_doc.model_text.create(text, position, font, size, depth) -> element_id` ·
`family_doc.control.create(position, direction: "single_h"|"double_h"|"single_v"|"double_v") -> element_id` ·
`family_doc.shape_points.toggle() -> ()`

**Acceptance criteria.**
- [ ] Each of the five form types and their voids can be created, locked to reference planes, flexed through parameters, and exported to IFC with the representation type listed in the table (or a valid B-rep).
- [ ] A void extrusion in a wall-hosted family, marked "Cut host", cuts the host wall in the project and creates an `IfcOpeningElement`.
- [ ] An array whose count is driven by an Integer parameter flexes through 0, 1 and 10 without errors.

### 2.8.5 Dimensions, labels, locking, flexing and reporting parameters
**ID:** DM-FED-005 · **Priority:** P1 · **Ref:** research/05 §4.5; screenshots/platform/reporting_parameters_angles_2026.png

**Purpose.** Dimensions with labels turn geometry into parameters.

**Behaviour & rules.**
- Aligned, linear, angular, radial, diameter and arc-length dimensions can be placed between references (planes, lines, form faces, edges).
- **Label** a dimension with an existing parameter of matching data type, or create a new parameter from the dimension. The label is shown on the dimension text ("Width = 900").
- **EQ** on a multi-segment dimension makes its segments equal (an equality constraint).
- **Lock** (padlock) makes a dimension's value fixed, or locks an alignment created by *Align* or by sketching onto a reference.
- **Reporting parameters.** A labelled dimension can be marked *reporting*. It then **reads** the geometry instead of driving it, and its value can be used in formulas. A reporting parameter that reads host geometry (for example host wall thickness in a wall-hosted family) is always an instance parameter. Reporting parameters cannot be given formulas.
- **Flexing.** In the Family Types dialog (§2.8.6), **Apply** regenerates the family with the current values. Solver errors are reported as in §2.6.3, naming the conflicting constraints. A **Flex Test** command runs every type, plus optional random values in declared ranges, and reports failures.
- **Parameter ranges.** Numeric parameters may declare `min`/`max` (a Reviera addition). Values outside the range are refused in Properties and in commands, with an error. Flex Test uses the ranges.

**Commands.**
`family_doc.dimension.create(refs[], kind, view_id, position) -> element_id` ·
`family_doc.dimension.label(dimension_id, param_name | new_param_def)` ·
`family_doc.dimension.set_eq(dimension_id, on)` ·
`family_doc.constraint.lock(dimension_or_alignment_id, on)` ·
`family_doc.align(target_ref, moving_ref, lock=false)` ·
`family_doc.flex_test(random_samples=0) -> report`

**Acceptance criteria.**
- [ ] A labelled dimension marked reporting (host wall thickness) updates as the host wall type changes in the project, and drives a formula for frame depth.
- [ ] Flex Test on a family with a deliberate over-constraint reports the failing type and names both conflicting constraints.

### 2.8.6 Family parameters and the Family Types dialog
**ID:** DM-FED-006 · **Priority:** P1 · **Ref:** research/05 §4.6, §7.1; screenshots/platform/family_types_dialog_parameter_groups.png, screenshots/platform/family_types_cabinet_variations_2026.png

**Purpose.** To manage a family's parameters, types and formulas in one place.

**UI.** Create › Properties › **Family Types** dialog:
- *Type name* drop-down, with New, Rename, Delete and Duplicate type buttons;
- a search field;
- a grid grouped by parameter group (§2.9.2), with columns **Parameter | Value | Formula | Lock**. Instance parameters show "(default)" after the name. Formula-driven values are read-only and greyed. Reporting parameters are marked;
- a toolbar: New, Edit, Delete and Duplicate parameter; Move Up/Down; Sort A–Z/Z–A; **Manage Lookup Tables** (§2.8.8); **Add from Parameter Library** (§2.9.7); Flex Test;
- **Apply** (regenerates and flexes) and **OK** / **Cancel**.

**Workflow (dialog).**
1. Create › Properties › **Family Types** (`family_doc.types()`), or double-click a label in the canvas. The dialog opens on the current type.
2. Pick or create a type, edit values and formulas, add or edit parameters.
3. **Apply** commits the pending edits as one transaction ("Family Types") and flexes the geometry while the dialog stays open, so the user sees the result. **OK** applies and closes. **Cancel** rolls back everything applied since the dialog opened, after a confirmation when anything was applied.

**Validation.**
- Values are parsed per data type and unit (§2.9.5) as they are typed. A formula is checked with `formula.validate` on each keystroke pause (300 ms): syntax, unknown names, unit dimension mismatch and cycles are shown under the cell, and Apply/OK are disabled while any formula is invalid.
- Type names are unique within the family; empty names and names containing `:` are refused (`:` separates family and type names in IFC, §2.3.2).
- Parameter names are unique within the family and must not clash with a built-in parameter of the family's category. A deleted parameter that is used in formulas, labels, associations or lookup-table calls lists those uses and is refused until they are removed.
- If flexing fails (§2.6.3), Apply rolls back that step, keeps the dialog open, selects the offending type and names the conflicting constraints.

**Behaviour & rules.**
- **Parameter Properties** dialog (new or edit): kind (*Family parameter* or *Shared parameter*, chosen from the library §2.9.7); name; discipline and data type (§2.9.4); group; Type or Instance; Reporting; tooltip (up to 500 characters); `min`/`max`; IFC home (property set and property name, §2.9.2, defaulting to a set named after the family's category, for example `Doors Parameters`, configurable; see Q-DM-03); for Family Type parameters, the category.
- **Family parameters** are not schedulable or taggable in projects. **Shared parameters** are (they carry a GUID; §2.9.7). A family parameter can be **converted** to shared (choosing a library definition with a matching data type), and back to family. Values and formulas are kept.
- Parameters can be **duplicated**, which creates a copy with the suffix " (copy)".
- Type parameter formulas cannot reference instance parameters. Instance parameter formulas may reference type parameters (§2.8.7).
- **Lock** fixes a dimension-driven parameter's value during flexing of others. It applies only to dimension labels.
- **Type changes.** New types copy the current type's values. Renaming a type in a family that is loaded in projects renames it there on reload. Deleted types are kept in projects until purged (§2.2.3).

**Commands.**
`family_doc.param.add(def: {name, data_type, group, is_instance, shared_guid?, reporting?, tooltip?, min?, max?, ifc_home?}) -> param_id` ·
`family_doc.param.edit(param_id, changes)` ·
`family_doc.param.delete(param_id)` ·
`family_doc.param.duplicate(param_id) -> param_id` ·
`family_doc.param.to_shared(param_id, library_guid)` · `family_doc.param.to_family(param_id)` ·
`family_doc.param.set_formula(param_id, formula | null)` ·
`family_doc.type.create(name) -> type_id` · `family_doc.type.rename(type_id, name)` · `family_doc.type.delete(type_id)` ·
`family_doc.type.set_value(type_id, param_id, value)` · `family_doc.type.set_current(type_id)` ·
`family_doc.types()` (opens the Family Types dialog)

**Acceptance criteria.**
- [ ] Entering the formula `Width + Height` for an Area parameter shows a unit-dimension error under the cell and disables OK.
- [ ] A family with 12 parameters in 4 groups shows them grouped as defined. Search "wid" filters to the Width parameters.
- [ ] Converting a family parameter "Leaf Width" to a shared parameter keeps its formula and values, and the parameter becomes available in project schedules after reload.
- [ ] Entering a value below `min` is refused with a message showing the allowed range.

### 2.8.7 The formula language
**ID:** DM-FRM-001 · **Priority:** P1 · **Ref:** research/05 §4.6, §7.4, §16.4

**Purpose.** A small, unit-aware expression language for family parameters, global parameters and calculated values (schedules and labels; DOC chapter). It works like the reference product's formulas, with clear improvements: `<=`/`>=`, text concatenation, explicit unit rules, references to global parameters, and table lookups with interpolation.

**Behaviour & rules.**

**(a) Grammar (normative).**

```ebnf
formula      = expression ;
expression   = concat ;
concat       = comparison , { "&" , comparison } ;              (* text concatenation *)
comparison   = additive , [ comp_op , additive ] ;              (* non-associative *)
comp_op      = "=" | "==" | "<>" | "!=" | "<" | ">" | "<=" | ">=" ;
additive     = term , { ( "+" | "-" ) , term } ;
term         = unary , { ( "*" | "/" ) , unary } ;
unary        = [ "-" | "+" ] , power ;
power        = primary , [ "^" , unary ] ;                       (* right-associative *)
primary      = quantity | text | boolean | reference | call | "(" , expression , ")" ;
quantity     = number , [ unit ] | feet_inches ;
number       = digits , [ "." , digits ] , [ exponent ] | "." , digits , [ exponent ] ;
digits       = digit , { digit } ;
exponent     = ( "e" | "E" ) , [ "+" | "-" ] , digits ;
feet_inches  = digits , "'" , [ [ "-" ] , inch_part , '"' ] ;     (* e.g. 5'  5'-6"  5' 6 1/2" *)
inch_part    = digits , [ ( " " , digits , "/" , digits ) | ( "." , digits ) ] | digits , "/" , digits ;
unit         = unit_symbol ;                                     (* from the unit catalogue §2.9.5: mm cm m km in ft ° deg rad grad m² m2 m³ m3 kg t N kN Pa kPa MPa s min h K °C W kW V A Hz lx lm cd … *)
text         = '"' , { text_char | '""' } , '"' ;
boolean      = "true" | "false" ;
reference    = [ "@" ] , ( name | "[" , bracket_name , "]" ) ;   (* "@" marks a global parameter *)
call         = function_name , "(" , [ expression , { "," , expression } ] , ")" ;
name         = name_start , { name_char } ;                      (* longest match against defined parameter names; may contain inner spaces *)
```

- Formulas are **locale-independent**: the decimal mark is always `.` and the argument separator is always `,`. The UI shows a hint when the user's locale uses a decimal comma.
- **Names.** A reference may be written bare when it matches a defined parameter name exactly. The tokenizer tries the longest defined name at each position, so names with spaces (`Frame Width`) work without brackets. Names that collide with function names, units or keywords, or that contain operator characters, must be written in brackets: `[Width-2]`, `[mm]`. Name matching is case-sensitive. A case-insensitive near-miss produces a "did you mean" hint.
- **Reserved words:** `true`, `false`, and all function names.

**(b) Types and units.**
- Value types: **Quantity** (a number with a physical dimension, which may be dimensionless), **Integer**, **Boolean**, **Text**, **ElementRef** (material, family type, image, …).
- Every quantity carries a **dimension vector** over (length, mass, time, current, temperature, amount, luminous intensity, angle, currency). Values are held in SI internally (§2.9.5).
- `+`, `-` and comparisons require equal dimensions. `*` and `/` combine dimensions. `^` requires a dimensionless, constant exponent when the base has a dimension. The result must have integer dimension exponents (so `sqrt(Area)` is a Length, and `sqrt(Length)` is an error).
- **Unit literals** are converted to SI at parse time: `1200 mm` is 1.2 m; `30°` is π/6 rad.
- **Unit-less literals in dimensioned contexts (compatibility rule).** A bare number added to, subtracted from or compared with a dimensioned quantity is read in the **project display unit** for that dimension (for example `Width + 50` with display unit mm means `Width + 50 mm`). It produces a lint warning `formula.unitless_literal` with a one-click fix that adds the unit. A formula whose overall result is dimensionless, assigned to a dimensioned parameter, is read in that parameter's display unit, with the same lint. This keeps formulas that users bring from other tools working, while steering them towards explicit units.
- **Angles.** The angle is a dimension. Trigonometric functions accept angles, or dimensionless numbers read as radians. Inverse functions return angles.
- **Assignment.** The result must match the target parameter's data type:
  - Integer targets round half away from zero (warning if the fraction is greater than 1e-9);
  - Yes/No targets need a Boolean;
  - Text targets accept Text, or any value formatted with the target's display format;
  - ElementRef targets accept only `if(...)` results whose branches are ElementRefs of the right kind, or references to parameters of the same kind.
- **Integer arithmetic.** Integers promote to dimensionless quantities in arithmetic. `/` always gives a quantity. Use `rounddown(a / b)` for integer division.

**(c) Functions (normative set).**

| Function | Signature | Notes |
|---|---|---|
| `if` | `if(cond, a, b)` | Lazy: only the chosen branch is evaluated. `a` and `b` must have the same type and dimension. |
| `and`, `or` | `and(b1, b2, …)`, `or(b1, b2, …)` | Two or more Boolean arguments, short-circuit |
| `not` | `not(b)` | |
| `abs`, `sign` | `abs(x)`, `sign(x)` | `sign` returns −1, 0 or 1 (dimensionless) |
| `min`, `max` | `min(x1, x2, …)` | Same dimension |
| `clamp` | `clamp(x, lo, hi)` | |
| `mod` | `mod(a, b)` | Result has the sign of `b`. Same dimension. |
| `sqrt` | `sqrt(x)` | Halves dimension exponents (must be even) |
| `exp`, `ln` | `exp(x)`, `ln(x)` | Dimensionless only |
| `log` | `log(x)` or `log(x, base)` | Base 10 by default. Dimensionless only. |
| `round`, `roundup`, `rounddown` | `round(x)`, `round(x, increment)` | Half away from zero / towards +∞ / towards −∞. Without an increment, a dimensioned `x` rounds to whole **display units** (compatibility). With an increment, it rounds to multiples of the increment (same dimension). |
| `sin`, `cos`, `tan` | `sin(a)` | Angle or radians |
| `asin`, `acos`, `atan` | `asin(x)` | Return an angle |
| `atan2` | `atan2(y, x)` | Same dimension for `y` and `x`. Returns an angle. |
| `pi` | `pi()` | |
| `lookup` | `lookup(table, column, default, key1, …)` | Exact match (relative tolerance 1e-9). Returns `default` if no row matches (§2.8.8). |
| `lookup_nearest` | `lookup_nearest(table, column, default, key1, …)` | Nearest row by normalised key distance |
| `lookup_interp` | `lookup_interp(table, column, default, key)` | Linear interpolation on one numeric key, clamped at the ends |
| `size_lookup` | as `lookup` | Alias accepted for compatibility |
| `len` | `len(t)` | Character count (Unicode scalar values) |
| `upper`, `lower`, `trim` | `upper(t)` | |
| `left`, `right`, `mid` | `left(t, n)`, `mid(t, start, n)` | `start` is 1-based |
| `contains` | `contains(t, s)` | Boolean |
| `text` | `text(x)` or `text(x, "format")` | Formats a value. Format strings: `"0.00"`, `"#,##0"`, or a unit spec such as `"unit:mm;dp:0;symbol"` (§2.9.5). |
| `number` | `number(t)` or `number(t, unit)` | Parses text. Error if not numeric. |
| `value_in` | `value_in(x, unit)` | The number of `unit`s in `x`, dimensionless (for example `value_in(Width, mm)`) |
| `count_true` | `count_true(b1, b2, …)` | Integer count of true values |

**(d) Scope and evaluation.**
- **Family formulas** may reference parameters of the same family, and **global parameters** with `@Name`. Global references are resolved in the project the family is loaded into. In the family document, and in projects without that global, the *default value declared in the family* is used. That default is part of the parameter's "global input" declaration. This is a Reviera extension (P2).
- **Type parameter formulas** may reference only type parameters (and globals). **Instance parameter formulas** may reference type and instance parameters.
- **Global parameter formulas** may reference other global parameters (P1) and the project information parameters (P2).
- Formulas are compiled once into a typed expression tree with constant folding. They are evaluated in Rust, not in Lua, for speed and determinism. Evaluation order follows the dependency graph (§2.6.2). Cycles are rejected.
- Errors (unknown name, type or dimension mismatch, division by zero, a table not found, a function argument out of domain) mark the parameter as **in error**. The error message includes the character position. A family that has formula errors cannot be loaded into a project.

**(e) Examples (informative).**

```
Leaf Width          = Width - 2 * Frame Width
Handle Height       = clamp(Height * 0.45, 900 mm, 1100 mm)
Number of Panels    = rounddown(Width / 600 mm)
Is Wide             = Width >= 1200 mm
Description Text    = "Door " & text(Width, "unit:mm;dp:0") & " x " & text(Height, "unit:mm;dp:0")
Fin Angle           = if(@Orientation = "South", 35°, 15°)
Elbow Radius        = lookup_interp(ElbowRadii, "Radius", 1.5 * Diameter, Diameter)
```

**Commands.** `formula.validate(context_id, param_id | null, text) -> {ok, type, dimension, errors[], lints[]}` · `formula.evaluate(context_id, text) -> value` (evaluates without storing)

**Acceptance criteria.**
- [ ] The parser accepts every example in (e), and a conformance test suite of at least 300 cases covering each grammar rule and function (with expected values or errors).
- [ ] `Width + 50`, with display units in mm, evaluates as `Width + 0.05 m` and produces the `formula.unitless_literal` lint. The quick-fix rewrites it to `Width + 50 mm`.
- [ ] `sqrt(Length)` is rejected with a dimension error that points at character 1.
- [ ] A type formula referencing an instance parameter is rejected with a message that names both parameters.
- [ ] Formula results are identical on all supported platforms (bit-exact for +, −, ×, ÷ and sqrt; within 1 ulp for transcendental functions, using a portable libm).

### 2.8.8 Lookup tables
**ID:** DM-LUT-001 · **Priority:** P1 · **Ref:** research/05 §4.6

**Purpose.** To drive parameters from tabular data (for example fitting dimensions by nominal size), embedded in the family.

**UI.** Family Types › **Manage Lookup Tables**: a list of tables, with Import, Export, Rename, Delete and an embedded grid editor (edit cells, add and remove rows and columns, check types).

**Behaviour & rules.**
- Tables are **embedded** in the family. Files are only used for import and export.
- **Reviera table format** (CSV, RFC 4180, UTF-8, `,` separator, `.` decimal mark):
  - The first header cell is the row label (it is not used for lookup).
  - Every other header cell is `ColumnName[spec]` or `ColumnName[spec:unit]`, where `spec` is a data-type key from §2.9.4 (`length`, `area`, `angle`, `number`, `integer`, `text`, `yes_no`, …) and `unit` is a unit key from §2.9.5 (default: SI).
  - Each data row has a label, then one value per column, in the declared unit. Empty cells are null.

```csv
Label,ND[length:mm],Radius[length:mm],Socket Depth[length:mm],Material[text]
DN15,15,22.5,16,Copper
DN20,20,30,19,Copper
DN25,25,37.5,22,Copper
```

- **Import** also accepts the widely used `Name##spec##unit` header style, so tables can be migrated from other tools. Unknown spec or unit tokens produce a mapping dialog.
- Lookup semantics are defined by the `lookup*` functions (§2.8.7). Key columns are matched by the order of the key arguments. If a table has duplicate keys, the first matching row wins, and a warning is shown on import.
- Size limit: 100,000 rows × 64 columns per table (bigger tables are refused, with a message).

**Data (storage).** RXD document `families/<id>` (`lookup_tables[]`, in a typed, columnar form). Very large tables may be RXD assets (`02a`).

**Commands.**
`family_doc.lookup_table.import(path, name?) -> table_id` ·
`family_doc.lookup_table.export(table_id, path)` ·
`family_doc.lookup_table.rename(table_id, name)` ·
`family_doc.lookup_table.delete(table_id)` ·
`family_doc.lookup_table.set_cell(table_id, row, column, value)`

**Acceptance criteria.**
- [ ] Importing the example table and using `lookup(Pipes, "Radius", 0 mm, ND)` gives 30 mm when ND = 20 mm.
- [ ] Exporting and re-importing a table gives byte-identical CSV (except line endings, which are normalised to LF).
- [ ] A `Name##length##millimeters` header imports with the right spec and unit.

### 2.8.9 Type catalogs
**ID:** DM-TCAT-001 · **Priority:** P1 · **Ref:** research/05 §4.6, §11.1

**Purpose.** Families with hundreds of sizes (for example steel sections) are loaded with only the types that a project needs.

**Behaviour & rules.**
- A family package may contain **many types** (as IFC types). When it holds more than a threshold (default 20 types), or has a companion type catalog, loading it shows the **Specify Types** dialog: a table of types with sortable, filterable parameter columns and multi-select. Only the selected types are copied into the project.
- **Companion type catalog** (optional, for families whose types are defined as data rather than in the package): a CSV file with the same base name as the family package plus `.types.csv` (for example `HEA.family.ifc` + `HEA.types.csv`). It uses the header grammar of §2.8.8, with the first column being the type name. Each row becomes a type. Its values override the family defaults for the listed parameters.
- **Reload** with a catalog keeps the types already in the project and offers to add more.
- **Export Family Types**: writes the family's current types to a catalog file.
- **Import Family Types** (Insert › Import › Import Family Types; `family.import_types(family_id?, path)`) creates types from a catalogue file for a family that is **already in the document**, without reloading the family package.
  - In a **family document** `family_id` is omitted: the rows become types of the open family (`family_doc.type.create` plus `family_doc.type.set_value` for each value).
  - In a **project**, `family_id` names a loaded family. The command first opens the **Specify Types** dialog on the catalogue rows so the user can pick which rows to import; through Lua and MCP all rows are imported unless the call is preceded by `family.list_types` and a filtered catalogue.
  - The family is identified by the user's pick or, when `family_id` is omitted in a project, by the catalogue's base name matching a loaded family name; no match fails with `family.not_found`.
  - A row whose type name already exists asks **Overwrite values**, **Skip** or **Import as "<name> (2)"** (one choice for all, or per row). Through commands the default is Skip, and the result lists skipped rows.
  - Columns are matched to family parameters by name, then by shared GUID when the header carries one (§2.8.8). Unknown columns, values that fail unit parsing, and values outside a parameter's `min`/`max` are reported per row and cell; the row is still imported with those cells left at the family default.
  - Formula-driven and reporting parameters cannot be set from a catalogue; such columns are reported and ignored.
  - The whole import is one transaction ("Import Family Types"); its report lists created, overwritten, skipped and failed rows.
  - Both the Reviera `.types.csv` grammar and the reference product's catalogue header grammar (`Name##LENGTH##MILLIMETERS` style) are accepted on import, so existing catalogues can be reused.

**Commands.**
`family.list_types(path) -> [{type_name, values}]` (reads a package or catalog without loading it) ·
`family.load(path, type_names[])` (see §2.2.3) ·
`family.export_types(family_id, path)` ·
`family.import_types(family_id?, path) -> type_id[]`

**Acceptance criteria.**
- [ ] A package with 400 types loads in under 2 s when 3 types are selected, and the project then contains only those 3 types.
- [ ] A type catalog with a column that is not a parameter of the family reports that column and ignores it.
- [ ] Importing a 10-row catalogue into a loaded "HEA" family whose project already has 2 of those types creates 8 types, skips 2 (by default), and one undo removes all 8.
- [ ] A catalogue written with the reference product's `##`-typed header imports into a Reviera family with the same values (within unit-conversion tolerance).

### 2.8.10 Nested and shared nested families
**ID:** DM-NEST-001 · **Priority:** P1 · **Ref:** research/05 §4.10

**Purpose.** Families built from other families: door hardware, casework assemblies, curtain panels with doors, tags with symbols.

**Behaviour & rules.**
- **Nesting.** *Component* places an instance of a loaded family inside the family document. The nested family's definition is embedded in the parent.
- **Associated parameters.** Any parameter of a nested instance (or nested type) can be associated with a parent parameter of the same data type, with the **"="** button in Properties. The child's value then follows the parent's. Associations are type-to-type, or instance-to-instance/type.
- **Family Type parameters.** A parent parameter of data type *Family Type* (§2.9.4), restricted to a category, can drive which nested type is used. In projects, users can swap the nested type through that parameter. The candidates are the nested family's types and, optionally, all loaded families of that category.
- **Shared nested families.** If the child has `shared = on` (§2.8.2):
  - in projects, nested instances become **real elements**: selectable with Tab, taggable, schedulable, and with their own GUID;
  - in IFC they are separate occurrences, aggregated by the parent occurrence through `IfcRelAggregates` (§2.3.9), and typed by their own `IfcTypeObject`;
  - their GUIDs are **stable across regeneration**: each shared nested instance's GUID is derived deterministically, as UUID v5 of (parent GUID, nested-instance path). The same parent therefore always gets the same nested GUIDs, even after reload;
  - when a shared nested family is already loaded in the project with the same family GUID, loading asks *Use project version* or *Use version from family*.
- **Non-shared nested families** are merged into the parent's geometry. In IFC they appear as part of the parent type's `RepresentationMap` (as `IfcMappedItem`s of the child type's maps).
- Nesting depth limit: 8 levels. Recursive nesting (a family nesting itself, directly or indirectly) is refused.

**Data (storage).** RXD document `families/<id>` (`nested[]`: child family GUID, type or Family Type parameter, placement, associations, visibility). The child's own definition is a separate `families/<child-guid>` document.

**Commands.**
`family_doc.component.place(family_id, type_id, placement) -> element_id` ·
`family_doc.param.associate(nested_element_id, child_param, parent_param_id | null)`

**Acceptance criteria.**
- [ ] A shared nested chair in a "Table and Chairs" family schedules as 4 chairs per table in a project. Their GUIDs are unchanged after reloading the family.
- [ ] A Family Type parameter "Handle Type" lets a project user swap the nested handle on a door type, and the change exports as a different nested type.

### 2.8.11 Visibility, detail levels, symbolic geometry and materials in families
**ID:** DM-FED-007 · **Priority:** P1 · **Ref:** research/05 §4.7, §4.8

**Behaviour & rules.**
- **Visibility settings** of each form, model line, nested component, filled region and symbolic line:
  - **Detail levels**: Coarse, Medium, Fine (any combination);
  - **View directions**: Plan/RCP; Front/Back; Left/Right (for 3D families);
  - **Only when cut** in plan or section.
- **Visible** may be bound to a Yes/No parameter (for example "Show Handle").
- **Symbolic lines** and 2D elements in 3D families appear only in views parallel to the plane they were drawn in. They are used for door swings and plan-only symbols.
- **Draw order** of 2D elements in 3D families is controllable (bring to front, send to back).
- **Plan pre-cut** behaviour comes from the family flags (§2.8.2).
- **Preview Visibility** (View Control Bar in family documents; `family_doc.preview_visibility.toggle(view_id?)`). When on, the family view draws only what a project view of the same kind, direction and detail level would show: elements hidden by their visibility settings, symbolic lines drawn in other planes, and `visible` parameters that are false for the current type are hidden; elements that would be cut only when cut are drawn as cut. A banner shows "Preview visibility: on". It is view state only, not undoable and not saved; elements hidden by it cannot be selected until it is turned off. The default is off.
- **Materials.** A form's material can be fixed, `<By Category>`, or bound to a Material parameter (type or instance). When a family is loaded, the materials it uses are copied into the project. If a project material with the same name exists, the project's version wins, unless the user chose *Overwrite parameter values* on load.
- IFC: the visibility and detail-level variants are written as in §2.3.10. Plan symbolic geometry goes in the type's plan `Annotation` representation map.

**Commands.** `family_doc.visibility.set(element_ids[], {coarse, medium, fine, plan, front_back, left_right, only_when_cut, visible_param?})` · `family_doc.draw_order(element_ids[], "front"|"back"|"forward"|"backward")` · `family_doc.preview_visibility.toggle(view_id?) -> ()`

**Acceptance criteria.**
- [ ] A door family whose panel is visible only at Medium and Fine, and whose swing symbolic line is in Plan only, shows the correct geometry at all three detail levels in a project plan and elevation.
- [ ] With Preview Visibility on in the family's Ref. Level plan at Coarse, the same door shows exactly what a Coarse project plan shows, and turning it off shows all elements again.

### 2.8.12 Special family kinds
**ID:** DM-FSPEC-001 … 006 · **Priority:** see table · **Ref:** research/05 §5.1–§5.8; screenshots/platform/edit_label_dialog_2026.png

| ID | Kind | Priority | Requirements |
|---|---|---|---|
| DM-FSPEC-001 | **Annotation families: tags and symbols** | P0 (shipped tag families, used as is) / P1 (editing) | 2D families with lines, filled regions, text and **labels**. A label binds text to one or more parameters, each with prefix, suffix, sample value, spaces before it, line break after it, and optional unit-format override. Labels may contain **calculated values** (formulas, §2.8.7, over the tagged element's parameters). Tag families have a target category. **Multi-category tags** read shared and built-in common parameters from any category, and can be the default tag for *Tag by Category* and *Tag All Not Tagged*. Families set rotate-with-component and fixed-rotation flags. Leader behaviour is defined in DOC. **Edit Label** dialog: category parameters on the left; label parameters (Spaces, Prefix, Sample Value, Suffix, Break) on the right. |
| DM-FSPEC-002 | **Title block families** | P0 (shipped title blocks) / P1 (editing) | Sheet-size templates. They contain border lines, images (logos, RXD assets) and labels bound to **Sheet**, **Sheet Collection** and **Project Information** parameters (including shared parameters). They can have an embedded revision schedule (DOC), several types, and Yes/No-driven visibility. A type property overrides the "As indicated" scale text. Title-block parameters are available to sheet list schedules (DOC). |
| DM-FSPEC-003 | **Profile families** | P1 | 2D closed loops used by sweeps, wall sweeps and reveals, slab edges, fascias, gutters, railings, mullions (several loops allowed), stair nosings and custom structural sections. Profiles may have types and parameters. Loops must be closed, non-self-intersecting and drawn in the template's single plane; open or crossing loops are refused on save with the offending curves highlighted. The template's origin is the insertion point used by hosts. A *profile usage* type parameter (generic, mullion, rail, nosing, reveal, slab edge) filters which tools list the profile. In IFC, a profile type used by a profiled element becomes an `IfcArbitraryClosedProfileDef` (or `IfcArbitraryProfileDefWithVoids`) referenced from `IfcMaterialProfile`. |
| DM-FSPEC-004 | **Detail item families** | P1 | 2D, category Detail Items, placed in views. Contents: detail lines, filled and masking regions, text, labels and nested detail items, with types and parameters. Line-based detail items stretch between two picked points and support **repeating details** (DOC). Draw order inside the family and against other view elements is kept (§2.8.11). Detail items can be tagged, keynoted and scheduled. They never appear in 3D or in IFC exchange exports. Storage: RXD documents (`families/…` for the definition; `views/annotation/<viewId>` for placed instances). |
| DM-FSPEC-005 | **MEP connectors** | P1 | Connector elements in families: duct, pipe, cable tray, conduit and electrical. Each has system classification, shape (round, rectangular, oval), size linked to parameters, flow configuration (calculated, preset, system), flow direction (in, out, bidirectional), loss method, and electrical properties (voltage, poles, apparent load, power factor). One connector may be *primary*. Connectors can be *linked* for pass-through flow. In IFC: `IfcDistributionPort` nested in the type (`IfcRelNests`) with `FlowDirection`, `PredefinedType` and `SystemType`, plus `Pset_DistributionPortCommon` and the port-type psets. The detailed behaviour is in the MEP chapter. |
| DM-FSPEC-006 | **Mass families** | P2 | Built in the CME (§2.8.13), as loadable families or in-place masses. In projects: *Show Mass* toggle, **mass floors** by level (one per selected level that cuts the mass), gross floor area, volume and surface area reported as read-only parameters, and **by-face** creation of walls, floors, roofs and curtain systems, with *Update to Face* (ARC chapter). Masses are hidden in views unless shown, and are excluded from exchange exports by default (§2.11.4). Mass floor areas feed area schedules and energy analysis (ANA). |

**Commands.** `family_doc.label.create(position, params[{param, prefix?, suffix?, sample?, spaces?, break?}], text_type_id) -> element_id` · `family_doc.label.edit(label_id, params[])` · `family_doc.connector.create(kind, face_ref | work_plane, props) -> element_id` · `family_doc.connector.set(connector_id, props)`

**Acceptance criteria.**
- [ ] A door tag family with a label "Mark" plus a calculated value `text(Width, "unit:mm;dp:0") & "x" & text(Height, "unit:mm;dp:0")` shows "D-104" and "915x2134" for a tagged door.
- [ ] A title block label bound to a shared project-information parameter "Client Reference" shows the project's value on every sheet.
- [ ] A pipe elbow family with two pipe connectors linked to a "Nominal Diameter" parameter exports two `IfcDistributionPort`s nested in its type.

### 2.8.13 Adaptive components and the conceptual massing environment (CME)
**ID:** DM-CME-001 … 005 · **Priority:** P2 · **Ref:** research/05 §5.5, §5.6, §6; screenshots/platform/adaptive_component_repetition_2026.png, screenshots/platform/conceptual_divided_surface_patterned_2026.png, screenshots/platform/conceptual_uv_grid_2026.png

**Purpose.** A 3D-first environment for masses, adaptive components and pattern-based panels, used for façades, trusses and complex repeated geometry.

#### DM-CME-001 — Conceptual environment
- Uses 3D levels and work planes, reference points (free, hosted on curves or faces, driving), a 3D snapping system, a 3D move gizmo (drag along or across axes) and **X-Ray** mode (shows the form's profiles, path and edges).
- **Create Form** (solid or void) from selected curves and references. It infers the form type:
  - one closed profile → **extrusion** (one open profile → surface);
  - two or more profiles on non-intersecting planes → **loft**;
  - profile + path → **sweep**;
  - profile + line in the same plane → **revolve**;
  - several profiles along a path → **swept blend**.
  If several results are possible, a chooser appears (thumbnails of the options).
- **Direct editing**: push and pull faces, edges and vertices with the gizmo. *Add Edge*, *Add Profile*, *Dissolve* (back to profiles), *Rebuild*.
- Forms are either **reference-based** (driven by references and parameters) or **unconstrained** (directly editable).
- OCCT: lofts use `BRepOffsetAPI_ThruSections` (smooth); sweeps use `BRepOffsetAPI_MakePipeShell`; surfaces use `BRepFill`/`GeomFill`. IFC: `IfcAdvancedBrep` (B-spline surfaces); `IfcTriangulatedFaceSet` as a fallback.
- **Workflow.** (1) Create a family from the Mass, Generic Model Adaptive or pattern-based template; the document opens in a 3D view with two reference planes and a level. (2) Draw model or reference lines on a work plane or on 3D levels, or place reference points. (3) Select curves and choose *Create Form › Solid* or *Void*. (4) Edit the result with the gizmo, or add profiles and edges. (5) Flex parameters in Family Types (§2.8.6) and load into a project.
- **Work planes in 3D.** Clicking on a face, level or reference plane makes it the active work plane for the next sketch (*Set work plane automatically*, default on). Reference points host their own three work planes. The 3D snapping set adds face, edge, point-on-curve and mid-point snaps.
- **Model lines vs reference lines.** Forms built on model lines consume them (the lines become form edges). Forms built on reference lines stay driven by them, so moving the reference line or changing a parameter on it reshapes the form.
- **Edge cases.** Self-intersecting or zero-area profiles are refused with the curve highlighted. A loft whose profiles have different vertex counts uses automatic vertex matching, which the user can edit in X-Ray. A form that fails to rebuild after a parameter change keeps its last valid shape and raises a warning naming the failing step (§2.7.4).
- **Data.** RXD document `families/<id>` (`forms[]` with kind `cme_form`, the referenced curves and points, and whether each form is reference-based or unconstrained); evaluated geometry per type in IFC as in §2.8.14.
- **Acceptance.** Moving a reference line that drives a loft reshapes the loft in the same transaction; deleting a model line used by a form is refused with a message naming the form.

#### DM-CME-002 — Divided surfaces and paths
- **Divide Surface** creates a UV grid on a face. Each direction is set by number, or by distance (with justification, grid rotation and belt offset). Alternatively, the surface is divided by **intersecting references** (levels, reference planes, lines), and the *Face Manager* edits which references are used.
- **Divide Path** places nodes along curves (by number, distance, or intersections).
- **Surface representation** controls show and hide the original surface, nodes, U/V grid lines, pattern lines and pattern fill.

#### DM-CME-003 — Patterns and pattern-based panels
- Built-in **patterns** for divided surfaces: Rectangle, Rectangle Checkerboard, 1/2 Step, 1/3 Step, Arrows, Hexagon, Octagon, Octagon Rotate, Rhomboid, Rhomboid Checkerboard, Triangle (bent and flat), Triangle Checkerboard (bent and flat), Zigzag.
- **Pattern-based panel families** (template *Curtain Panel Pattern-Based*) define a tile with adaptive corner points. Applied to a divided surface, they populate each tile, adapting to the node positions. Border tiles: *Partial*, *Overhanging*, *Empty* (per surface).

#### DM-CME-004 — Adaptive components
- Family template *Generic Model Adaptive*. **Reference points** can be made **adaptive**: they are numbered placement points, plus optional *shape handle* points. Geometry is built between them from reference lines and forms. Orientation options for each point: global, host, instance, and their combinations.
- **Placement**: the user clicks N points in a project, on a divided-surface node, on a divided-path node or on any surface. The instance adapts to them. The command form is `instance.place_adaptive` (§2.2.5).
- **Repeat** distributes an adaptive component across a divided path or a surface node grid, following the pattern of the first placement. The repeater is one element with a *repeater* type.
- In IFC, each placed adaptive instance is an occurrence with its own representation (geometry is unique per placement). Its adaptive point positions are RXD element data (`adaptive`).

#### DM-CME-005 — Masses in projects
- See DM-FSPEC-006. Energy-analysis use of masses is in the ANA chapter.

**Commands.**
`cme.form.create(curve_refs[], solid=true, choice?) -> element_id` ·
`cme.form.push_pull(subelement_ref, vector)` ·
`cme.form.add_edge(face_ref, p1, p2)` · `cme.form.add_profile(form_id, parameter)` · `cme.form.dissolve(form_id)` ·
`cme.divide_surface(face_ref, {u, v} | {references[]}) -> element_id` ·
`cme.divide_path(curve_refs[], spec) -> element_id` ·
`cme.surface.set_pattern(divided_surface_id, pattern, border_tiles?)` ·
`cme.surface.apply_component(divided_surface_id, panel_type_id)` ·
`cme.point.create(position | host_ref, adaptive=false) -> element_id` · `cme.point.make_adaptive(point_id, index?)` ·
`cme.repeat(adaptive_instance_id) -> element_id`

**Acceptance criteria.**
- [ ] A loft created from three closed profiles on parallel planes can be dissolved back to the profiles and re-formed with the same result.
- [ ] A curved façade divided 20 × 10 and populated with a 4-point adaptive panel produces 200 panels that follow the surface when its control profile moves.
- [ ] A 2-point adaptive beam repeated along a 50-node divided path creates 49 instances, and these export to IFC as occurrences with unique geometry.

### 2.8.14 Family package file format
**ID:** DM-FFMT-001 · **Priority:** P0 (reading packages for placement) / P1 (writing from the Family Editor) · **Ref:** research/05 §4.9, §11.1, §16.9; TECH_STACK §3.1; spec/02a_extension_data_format.md

**Purpose.** Families are stored as **IFC type libraries**, so that any IFC tool can use a family's types and geometry, while Reviera keeps the full parametric definition as RXD.

**Behaviour & rules.**
- A **family package** is a valid IFC4 file (IFC4X3 optional, matching the target project schema). Recommended names: `<Family>.family.ifc` for one family, `<Library>.library.ifc` for a collection, `<Name>.familytemplate.ifc` for templates. `.ifczip` is allowed.
- **IFC content.** The package shall contain:
  1. one `IfcProject` (Name = the family or library name), with SI units and the representation contexts of §2.3.1. It contains no spatial structure, and no occurrences except as noted below;
  2. one `IfcProjectLibrary` per family (the **primary** family, plus every nested family it needs), declared by the project with `IfcRelDeclares`;
  3. each family's types, as `IfcTypeObject` subtypes declared by the family's library with `IfcRelDeclares`. Each type carries:
     - its type-parameter values as property sets (§2.3.3);
     - its evaluated geometry for that type's values: `RepresentationMaps` for `Body` (Fine, plus `Body-Coarse`/`Body-Medium` when different), `Axis`, `FootPrint`, plan `Annotation`;
     - its material assignments: `IfcMaterialLayerSet`, `IfcMaterialProfileSet` or `IfcMaterialConstituentSet`;
     - its ports (`IfcDistributionPort`, nested) for MEP families;
     - its classification (`IfcRelAssociatesClassification`);
  4. every `IfcMaterial` used, with its properties and styles (§2.10.5);
  5. for annotation, title block, detail and profile families, which have no IFC type class: no IFC types. Their whole definition is RXD.
- **RXD content** (carried as `02a` defines; logical content only here):
  - an RXD document `families/<id>/package` (the package manifest):
    - package format version and minimum Reviera version;
    - the primary family GUID and the list of contained families;
    - the template flag and the source template GUID;
    - a preview image (RXD asset, PNG, 512 × 512);
    - author, licence (an SPDX expression, optional for content but recommended), description and keywords;
  - one RXD document `families/<id>` per family, holding: `name`, `category`, `template`, `placement_mode`, `flags` (§2.8.2), `parameters[]` (§2.8.6, with formulas), `types[]` (name, values, link to the IFC type GUID), `references[]` (§2.8.3), `sketches[]`, `forms[]` (§2.8.4), `dimensions[]` and `constraints[]` (§2.8.5), `symbolic[]` (2D elements with view orientation), `nested[]` (§2.8.10), `arrays[]`, `controls[]`, `connectors[]`, `labels[]` (annotation families), `lookup_tables[]` (§2.8.8), `adaptive_points[]`, `divided_surfaces[]` (CME), `subcategories[]`, `views[]` (family views and their settings).
- **Informative skeleton of `families/<family-guid>`:**

```json
{
  "schema": "reviera.family", "schema_version": "1.0.0",
  "guid": "3vB2…", "name": "Single-Flush Door", "category": "cat.doors",
  "template": "door", "placement_mode": "wall_hosted",
  "flags": { "shared": false, "room_calculation_point": true, "ifc_predefined_type": "DOOR" },
  "parameters": [
    { "id": "p1", "name": "Width",  "data_type": "length", "group": "dimensions", "instance": false,
      "ifc_home": ["Pset_DoorCommon", null], "attribute": "OverallWidth" },
    { "id": "p2", "name": "Frame Width", "data_type": "length", "group": "construction", "instance": false },
    { "id": "p3", "name": "Leaf Width", "data_type": "length", "group": "dimensions", "instance": false,
      "formula": "Width - 2 * Frame Width" }
  ],
  "types": [ { "name": "0915 x 2134", "ifc_type": "1xH4…", "values": { "p1": 0.915, "p2": 0.05 } } ],
  "references": [ { "id": "r1", "name": "Left", "is_reference": "left", "plane": { "origin": [-0.4575,0,0], "normal": [1,0,0] } } ],
  "forms": [ { "id": "f1", "kind": "extrusion", "sketch": "s1", "start": 0, "end": { "param": "p5" },
               "material": { "param": "p9" }, "subcategory": "Panel",
               "visibility": { "coarse": false, "medium": true, "fine": true } } ],
  "nested": [], "lookup_tables": [], "connectors": []
}
```

- **Consistency rule.** On save, the IFC types and their geometry shall be regenerated from the RXD definition, so the two always agree. On load, if the RXD is missing (for example, the package was re-saved by a tool that strips unknown property sets), the family loads as a **static family**. Its types and geometry are usable, but parametric editing is unavailable, and the user is told why.
- **Third-party type libraries.** Any IFC file whose `IfcProject` declares an `IfcProjectLibrary` with types (or declares types directly) can be loaded as static families, one family per library, or per type class when there is no library.
- **Embedding.** Inside a project, an embedded family is exactly the same content: an `IfcProjectLibrary` + types in the project's IFC, plus the `families/<guid>` RXD document. *Save Family As* writes a package from it unchanged.
- **Versioning.** Each family has a `family_version` (semver) that increases on every save from the Family Editor. On load, Reviera compares family GUID and version (§2.2.3).

**Commands.** `family.package.inspect(path) -> {families[], types[], has_rxd, version, licence}` · plus `family.load`, `family.save_as` (§2.2.3) and `family_doc.save` (§2.8.0).

**Acceptance criteria.**
- [ ] A family package saved by Reviera passes IFC4 schema validation. It opens in at least two other open-source IFC tools (for example IfcOpenShell-based Bonsai, and one web viewer), which show its types with correct geometry and property values.
- [ ] Re-saving a package through a tool that strips unknown property sets, then loading it, gives a static family with all types, and the user is told that parametric editing is unavailable.
- [ ] *Save Family As* from a project, followed by loading into a new project, gives types and instances that match the original exactly (the same geometry hashes).

### 2.8.15 Library browser
**ID:** DM-LIB-001 · **Priority:** P1 (local folders) / P2 (organisation libraries over HTTPS or git) · **Ref:** research/05 §4.9 (load family, content library), §11.1, §16.2; spec/01 (Insert › Load from Library)

**Purpose.** Find, inspect and load families and types from local folders and from organisation libraries without knowing file paths. It is the open counterpart of a vendor cloud content library.

**Workflow.**
1. Insert › Load from Library › **Browse Library** (`library.browse(query?, source?)`), or *Load Family…* › *Browse library* in the file dialog. The **Library** panel opens docked to the right (it can float).
2. Pick a source in the tree, or search across all sources. Narrow the results with filters.
3. Select a result. The preview pane shows the family, its types and its key parameters.
4. Choose **Load** (opens Specify Types when the family has many types, §2.8.9), **Load and Place** (loads, then starts placement with the chosen type), or **Open** (opens the package in the Family Editor; not available for read-only sources unless saved as a copy).

**UI.**
- **Sources tree**: *Reviera Library* (shipped content), *My Libraries* (user folders), *Organisation* (configured remote libraries and network folders), *Favourites*, *Recently loaded*. Each source shows its state (indexed, indexing n %, offline, error).
- **Search box** with free text over name, type names, category, keywords and description; **filters** for category, placement mode (§2.8.1), IFC class, discipline, licence, author or publisher, and "has types matching" (for example `Width >= 900 mm`, using the collector's operators, §2.13.3).
- **Results** as a thumbnail grid or a list (Name, Category, Types, Source, Version, Licence, Modified), sortable, with the count of matches.
- **Preview pane**: an interactive 3D preview (orbit, zoom; plan and front views for 2D families) rendered locally from the package geometry; the type list with each type's values for up to 8 key parameters chosen by the family's author (default: the dimension group); the licence (SPDX), author, version and description from the package manifest (§2.8.14); and warnings (for example "Static family: no parametric data", "Requires Reviera 1.3").
- Context menu: Add to / Remove from Favourites, Show in folder, Copy path or URL, Properties.

**Behaviour & rules.**
- **Indexing.** Each source is indexed in the background into a per-user cache (`<cache_dir>/reviera/library-index.sqlite`). The index stores, per family: package path or URL, content hash, family GUID and version, name, category, placement mode, type names and key values, keywords, licence and the 512 × 512 preview image from the manifest. Re-indexing is incremental by modification time and hash. A 10,000-family folder indexes once and is searchable within 200 ms per query afterwards.
- **Library files.** A `<Library>.library.ifc` holding several families shows one result per family. Third-party IFC type libraries are indexed as static families (§2.8.14) and marked so.
- **Organisation libraries (P2).** A remote library is a static JSON index (`reviera.library-index`, version 1) at an HTTPS URL or in a git repository: one entry per package with URL, SHA-256, family GUID, version, licence and preview URL. Packages are downloaded on demand into the cache and verified against the checksum; a mismatch is refused. Sources can be pre-configured by an administrator policy file, and those cannot be removed by the user. No vendor service is needed.
- **Duplicates.** When the same family GUID appears in several sources, results group them and show the highest version first; the user can pick another version.
- **Loading** uses `family.load` (§2.2.3) with its reload rules: if the family is already in the project, the comparison of GUID and version is shown before loading.
- **Licence display.** Packages without a licence are shown with "Licence not stated". A package whose licence forbids redistribution is still loadable for private use, but a notice is shown once per package.
- **Offline.** Cached remote packages remain loadable offline; uncached ones are listed but greyed.

**Data (storage).** No model data. Sources, favourites and the index are per-user application data (UI chapter, preferences). Loaded families are embedded as usual (§2.2.3). The source URL and package hash are recorded in `families/index` so *Reload from library* can find a newer version.

**Commands.**
`library.browse(query?, source?)` (opens the panel) ·
`library.search(text?, filters?, source?, limit=100) -> library_item[]` ·
`library.source.add(path_or_url, name?) -> source_id` · `library.source.remove(source_id)` ·
`library.refresh(source_id?)` ·
`library.load(item_ref, type_names?, place=false) -> family_id`

**Acceptance criteria.**
- [ ] Adding a local folder of 2,000 family packages indexes it in the background without blocking editing; a search for "door 900" then returns matching door families within 200 ms.
- [ ] Selecting a result shows a rotatable 3D preview and its types without loading the family into the project (the model version is unchanged).
- [ ] A remote library whose package checksum does not match its index entry is refused with an error naming the package, and nothing is loaded.
- [ ] *Load and Place* on a door with 30 types opens Specify Types, then starts door placement with the chosen type.

---

## 2.9 Parameters and units

### 2.9.1 Kinds of parameters
**ID:** DM-PARAM-001 · **Priority:** P0 (built-in, project) / P1 (family, shared, global) · **Ref:** research/05 §7.1, §7.6

**Purpose.** Parameters carry all element data. This section defines their kinds and how each kind is identified.

**Behaviour & rules.**

| Kind | Defined in | Identity | Schedulable / taggable | Typical use |
|---|---|---|---|---|
| **Built-in** | The application's parameter registry | Stable key (for example `wall.base_offset`) with a negative internal ID | Mostly yes | Mark, Comments, Level, Base Offset, Area, Volume, Fire Rating |
| **Family** | A family definition | Name, unique within the family | **No** | Driving geometry and formulas inside a family |
| **Shared** | A **parameter library** (§2.9.7) | **GUID** | Yes, including tags and title blocks | Firm-wide data fields used in families and projects |
| **Project** | The project, bound to categories | Name (if local) or GUID (if shared) | Schedules yes. Tags only if shared. | Project-specific data on many categories |
| **Global** | The project | GUID of the global parameter | Not directly (they can be shown through calculated values) | Driving dimensions and values project-wide |
| **Key schedule** | A key schedule (DOC) | Name | Yes | Pick-list values that set several parameters at once |
| **IFC-native** | A third-party IFC file | (`Pset name`, `property name`) | Yes | Properties Reviera found in imported files that do not match any definition |

- Every parameter value is either a **type** value (stored on the type) or an **instance** value (stored on the instance). The definition's binding decides which (§2.9.3). It is never both.
- **Read-only computed parameters** (Area, Volume, Length, Perimeter, Count, Host, Room) are produced by regeneration. They are shown greyed out, and commands that try to set them fail with a clear error.
- **IFC-native parameters.** When Reviera opens an IFC file, every property that does not match a built-in, shared or project definition becomes an *IFC-native parameter*. It is shown in Properties under its property-set name, can be edited and scheduled, and is written back to the same property set. The user can **promote** one to a project or shared parameter. Promotion keeps the values and adds a definition.

**Acceptance criteria.**
- [ ] A third-party IFC file with a custom property set `ACME_Asset` (property `AssetTag`) shows `AssetTag` in Properties and in the schedule field list. Edits are written back to `ACME_Asset.AssetTag`.

### 2.9.2 Parameter definitions
**ID:** DM-PARAM-002 · **Priority:** P0 · **Ref:** research/05 §7.1, §7.7, §16.3

**Purpose.** Parameter definitions are first-class objects with a fixed set of fields.

**Behaviour & rules.** A parameter definition shall have these fields:

| Field | Meaning |
|---|---|
| `id` | Internal ID |
| `guid` | Required for shared, global and project parameters. Built-in parameters use their key. |
| `name` | Display name (unique among the definitions bound to a category) |
| `data_type` | A data-type key (§2.9.4), for example `length`, `yes_no`, `family_type:cat.doors` |
| `group` | A parameter group key (below) |
| `discipline` | `common`, `structural`, `hvac`, `electrical`, `piping`, `energy`, `infrastructure` |
| `tooltip` | Up to 500 characters |
| `visible` | Hidden parameters are kept but not shown (for add-in use) |
| `user_modifiable` | If false, only code can change values |
| `hide_when_no_value` | Hidden in Properties when empty |
| `min` / `max` | Optional range (numeric types) |
| `enum` | Optional allowed-values list (text and integer types). Written to IFC as `IfcPropertyEnumeration`. |
| `ifc_home` | Property-set name and property name (and, for built-in parameters, an IFC attribute) used to store the value (§2.3.3) |
| `formula` | Family and global parameters only |
| `reporting` | Family and global parameters only |

- **Parameter groups** (the "group under" list; keys are stable and display names are localisable): `analysis_results`, `analytical_model`, `constraints`, `construction`, `data`, `dimensions`, `division_geometry`, `electrical`, `electrical_circuiting`, `electrical_lighting`, `electrical_loads`, `electrical_analysis`, `energy_analysis`, `fire_protection`, `forces`, `general`, `graphics`, `green_building`, `identity_data`, `ifc_parameters`, `layers`, `materials_and_finishes`, `mechanical`, `mechanical_flow`, `mechanical_loads`, `model_properties`, `moments`, `other`, `phasing`, `photometrics`, `plumbing`, `primary_end`, `rebar_set`, `releases_member_forces`, `secondary_end`, `segments_and_fittings`, `set`, `slab_shape_edit`, `structural`, `structural_analysis`, `text`, `title_text`, `visibility`, `carbon` (Reviera addition).
- **Default IFC home** for user parameters: a property set named by the binding's configured *property-set name*. The default is `<Project or Library name> Parameters`, for example "ACME Parameters". Parameters may be given a specific property set name and property name, so that they map onto a buildingSMART property (for example a shared parameter "Fire Rating" mapped to `Pset_WallCommon.FireRating` for walls and `Pset_DoorCommon.FireRating` for doors). The IFC home can differ per bound category.
- **Built-in parameter registry.** The application ships a registry of built-in parameters: key, name, data type, group, the categories it applies to, type/instance, read-only flag and IFC home. Domain chapters add rows. Examples:

| Key | Name | Data type | Type/Instance | IFC home |
|---|---|---|---|---|
| `elem.mark` | Mark | `text` | Instance | `IfcElement.Tag` |
| `elem.comments` | Comments | `text` | Instance | group-named set `Identity Data`.`Comments` (§2.3.3) |
| `type.type_mark` | Type Mark | `text` | Type | `IfcTypeProduct.Tag` |
| `type.description` | Description | `text` | Type | `IfcRoot.Description` (type) |
| `type.fire_rating` | Fire Rating | `text` | Type | `Pset_<Class>Common.FireRating` |
| `type.manufacturer` | Manufacturer | `text` | Type | `Pset_ManufacturerTypeInformation.Manufacturer` |
| `type.model` | Model | `text` | Type | `Pset_ManufacturerTypeInformation.ModelLabel` |
| `type.cost` | Cost | `currency` | Type | `Identity Data`.`Cost` (`IfcMonetaryMeasure`) |
| `type.classification_code` | Classification Code | `text` | Type | `IfcClassificationReference.Identification` |
| `type.keynote` | Keynote | `text` | Type | `Identity Data`.`Keynote` |
| `elem.level` | Level | `element:level` | Instance | Spatial containment |
| `elem.phase_created` | Phase Created | `element:phase` | Instance | RXD element data `phasing` |
| `wall.base_offset` | Base Offset | `length` | Instance | RXD element data `constraints` (and the evaluated geometry) |
| `wall.unconnected_height` | Unconnected Height | `length` | Instance | `Qto_WallBaseQuantities.Height` (as a derived value) + RXD |
| `door.width` | Width | `length` | Type | `IfcDoor.OverallWidth` (instance attribute, written from the type) + `IfcDoorType` property |

**Commands.** `param.definition(param_ref) -> definition` · `param.list(category?, element_id?) -> definition[]` · `param.set_ifc_home(param_ref, category?, pset_name, property_name)`

**Acceptance criteria.**
- [ ] Mapping a shared parameter "Fire Rating" to `Pset_WallCommon.FireRating` for walls and to `Pset_DoorCommon.FireRating` for doors writes each value to the right standard property set.
- [ ] `param.list("cat.walls")` returns every built-in, project and shared parameter bound to walls, with its data type and IFC home.

### 2.9.3 Project parameters and category bindings
**ID:** DM-PARAM-003 · **Priority:** P0 · **Ref:** research/05 §7.3, §7.6; screenshots/platform/parameter_properties_shared_project_parameter.png, screenshots/platform/manage_tab_settings_panel.png

**Purpose.** To add data fields to project elements by category.

**Workflow.**
1. Manage › Settings › **Project Parameters**. The list is sorted A–Z and has a search field.
2. **Add**: choose *Project parameter* (enter name, data type, group) or *Shared parameter* (pick from the parameter library). Choose **Type** or **Instance**. For instance parameters, choose **Values aligned per group type** or **Values can vary by group instance**. Choose the categories on the right (a filterable tree with *Check all* / *Check none*, and *Hide unchecked*). Optionally set the IFC property-set name.
3. **OK**. Every existing element in the chosen categories gets the parameter, with no value.
4. **Modify** changes the binding (categories, group, IFC home). **Remove** deletes the definition and all its values, after a confirmation that shows how many values will be lost.

**Behaviour & rules.**
- A binding is (definition, categories, type or instance, group behaviour). A definition can have only one binding in a project.
- Changing a binding from instance to type (or back) is allowed. Reviera asks how to resolve values that differ between instances of the same type (take the most common value, take the first value, or clear). The resolution is reported.
- Parameters can also be created from the schedule-field dialog (DOC) and from the Family Types dialog (as family parameters). Those are the same definitions.
- A project parameter with the same name as a built-in parameter of a bound category is refused.
- **Project Parameters dialog** (`param.project.manage()`). The list shows every project and shared parameter binding with Name, Data type, Type/Instance, Group, Categories (count, with the list in a tooltip), Source (Project, or the library name for shared ones) and IFC property set. Buttons: **Add**, **Modify**, **Remove** and **Duplicate** (copies a project parameter's definition and binding under a new name and GUID, without values; it is `param.project.add` with the copied definition). A search field filters by name, and a drop-down filters by category.
  - The **Parameter Properties** sub-dialog validates before OK is enabled: a name is required, at most 255 characters, unique among definitions bound to any of the chosen categories, and free of `[ ] { } | ; < >` (characters reserved by formulas and naming rules); a data type is required; at least one category must be ticked; a property-set name, if given, must not start with `Pset_` or `Qto_` unless it maps onto that standard set's existing property (§2.9.2), and must not start with `Reviera_`.
  - Changes made in the dialog (several adds, modifies and removes) are applied together on **OK** as one transaction named "Project Parameters"; **Cancel** discards them.
  - Removing a binding used by schedules, filters, tags, view templates or formulas lists those uses first.

**Data (storage).** Definitions and bindings: RXD document `standards/parameters`. Values: IFC property sets (§2.3.3).

**Commands.**
`param.project.add(def, categories[], is_instance, group_behavior: "aligned"|"vary", pset_name?) -> param_id` ·
`param.project.modify(param_id, changes)` ·
`param.project.remove(param_id)` ·
`param.get(element_id, param_ref) -> value` ·
`param.set(element_ids[], param_ref, value)` ·
`param.set_many(changes: [{element_id, param_ref, value}])` ·
`param.clear(element_ids[], param_ref)` ·
`param.project.manage()` (opens the Project Parameters dialog)

**Acceptance criteria.**
- [ ] In the Project Parameters dialog, adding "Fire Rating" bound to Doors (where a built-in Fire Rating exists) is refused with the clash named, and OK stays disabled until the name is changed.
- [ ] Adding an instance text parameter "Room Finish Code" to Rooms and Areas makes it appear in both categories' Properties and schedules, and in IFC under the configured property set.
- [ ] Changing a parameter from instance to type, when instances of the same type disagree, shows the resolution dialog and applies the choice.
- [ ] `param.set` on a read-only computed parameter fails with error `param.read_only`.

### 2.9.4 Data types and IFC measure types
**ID:** DM-PARAM-004 · **Priority:** P0 (common) / P1 (discipline types) · **Ref:** research/05 §7.7, §8

**Purpose.** A fixed, versioned catalogue of data types. Each type maps to an IFC measure type and an SI internal unit.

**Behaviour & rules.**
- Each data type has a **key** (for example `length`) and a **versioned ID** (`reviera:spec/length@1`). Keys are used in formulas, tables and APIs. Versioned IDs are stored in RXD, so that a future change in meaning can be migrated.
- The catalogue (normative baseline; domain chapters may add types, but not change existing ones):

| Key | Discipline | Storage | IFC value type | SI internal unit |
|---|---|---|---|---|
| `text` | common | string | `IfcLabel` (≤ 255 chars) or `IfcText` | — |
| `multiline_text` | common | string | `IfcText` | — |
| `integer` | common | i64 | `IfcInteger` | — |
| `number` | common | f64 | `IfcReal` | — |
| `yes_no` | common | bool | `IfcBoolean` | — |
| `url` | common | string | `IfcURIReference` | — |
| `date` / `datetime` | common | ISO 8601 | `IfcDate` / `IfcDateTime` | — |
| `length` | common | f64 | `IfcLengthMeasure` (`IfcPositiveLengthMeasure` when `min > 0`) | m |
| `distance` | common | f64 | `IfcLengthMeasure` | m |
| `area` | common | f64 | `IfcAreaMeasure` | m² |
| `volume` | common | f64 | `IfcVolumeMeasure` | m³ |
| `angle` | common | f64 | `IfcPlaneAngleMeasure` | rad |
| `rotation_angle` | common | f64 | `IfcPlaneAngleMeasure` | rad |
| `slope` | common | f64 | `IfcRatioMeasure` (rise/run) | — |
| `ratio` / `percentage` | common | f64 | `IfcRatioMeasure` / `IfcPositiveRatioMeasure` | — |
| `currency` | common | f64 | `IfcMonetaryMeasure` (with `IfcMonetaryUnit` from project currency) | currency unit |
| `mass` | common | f64 | `IfcMassMeasure` | kg |
| `mass_density` | common | f64 | `IfcMassDensityMeasure` | kg/m³ |
| `mass_per_length` | common | f64 | `IfcMassPerLengthMeasure` | kg/m |
| `time` | common | f64 | `IfcTimeMeasure` | s |
| `duration` | common | ISO 8601 | `IfcDuration` | — |
| `speed` | common | f64 | `IfcLinearVelocityMeasure` | m/s |
| `cost_per_area` | common | f64 | `IfcReal` + unit in the template (no IFC measure) | currency/m² |
| `material` | common | element ref | `IfcPropertyReferenceValue` → `IfcMaterial` | — |
| `image` | common | asset ref | `IfcPropertyReferenceValue` → `IfcImageTexture` / `IfcURIReference` | — |
| `fill_pattern` | common | element ref | `IfcLabel` (pattern name) | — |
| `family_type:<category>` | common | element ref | `IfcPropertyReferenceValue` → `IfcTypeObject` (or `IfcLabel` name) | — |
| `element:<kind>` | common | element ref | `IfcPropertyReferenceValue` → `IfcObjectDefinition` / `IfcLabel` GUID | — |
| `load_classification` | electrical | element ref | `IfcLabel` | — |
| `force` | structural | f64 | `IfcForceMeasure` | N |
| `linear_force` | structural | f64 | `IfcLinearForceMeasure` | N/m |
| `area_force` | structural | f64 | `IfcPlanarForceMeasure` | N/m² |
| `moment` | structural | f64 | `IfcTorqueMeasure` | N·m |
| `linear_moment` | structural | f64 | `IfcLinearMomentMeasure` | N·m/m |
| `stress` | structural | f64 | `IfcPressureMeasure` (`IfcModulusOfElasticityMeasure` for moduli) | Pa |
| `unit_weight` | structural | f64 | `IfcReal` (no IFC measure) | N/m³ |
| `thermal_expansion` | structural | f64 | `IfcThermalExpansionCoefficientMeasure` | 1/K |
| `section_area` / `section_modulus` / `moment_of_inertia` / `warping_constant` | structural | f64 | `IfcAreaMeasure` / `IfcSectionModulusMeasure` / `IfcMomentOfInertiaMeasure` / `IfcWarpingConstantMeasure` | m² / m³ / m⁴ / m⁶ |
| `point_spring` / `line_spring` / `area_spring` / `rotational_spring` | structural | f64 | `IfcLinearStiffnessMeasure` / `IfcModulusOfLinearSubgradeReactionMeasure` / `IfcModulusOfSubgradeReactionMeasure` / `IfcRotationalStiffnessMeasure` | N/m, N/m², N/m³, N·m/rad |
| `acceleration` | structural | f64 | `IfcAccelerationMeasure` | m/s² |
| `frequency` | structural/electrical | f64 | `IfcFrequencyMeasure` | Hz |
| `temperature` | hvac/piping | f64 | `IfcThermodynamicTemperatureMeasure` | K |
| `temperature_difference` | hvac | f64 | `IfcThermodynamicTemperatureMeasure` (difference flag in the template) | K |
| `pressure` | hvac/piping | f64 | `IfcPressureMeasure` | Pa |
| `airflow` / `flow` | hvac/piping | f64 | `IfcVolumetricFlowRateMeasure` | m³/s |
| `velocity` | hvac/piping | f64 | `IfcLinearVelocityMeasure` | m/s |
| `power` / `heat_gain` / `cooling_load` / `heating_load` | hvac/electrical | f64 | `IfcPowerMeasure` | W |
| `power_density` | hvac/electrical | f64 | `IfcHeatFluxDensityMeasure` | W/m² |
| `energy` | energy | f64 | `IfcEnergyMeasure` | J |
| `thermal_conductivity` | energy | f64 | `IfcThermalConductivityMeasure` | W/(m·K) |
| `heat_transfer_coefficient` | energy | f64 | `IfcThermalTransmittanceMeasure` | W/(m²·K) |
| `thermal_resistance` | energy | f64 | `IfcThermalResistanceMeasure` | m²·K/W |
| `specific_heat` | energy | f64 | `IfcSpecificHeatCapacityMeasure` | J/(kg·K) |
| `thermal_mass` | energy | f64 | `IfcReal` (heat capacity per area; no IFC measure) | J/(m²·K) |
| `permeability` | energy | f64 | `IfcVaporPermeabilityMeasure` | kg/(s·m·Pa) |
| `dynamic_viscosity` | hvac/piping | f64 | `IfcDynamicViscosityMeasure` | Pa·s |
| `roughness` | hvac/piping | f64 | `IfcLengthMeasure` | m |
| `duct_size` / `pipe_size` / `conduit_size` / `cable_tray_size` / `bar_diameter` | mep/structural | f64 | `IfcPositiveLengthMeasure` | m |
| `current` | electrical | f64 | `IfcElectricCurrentMeasure` | A |
| `electrical_potential` | electrical | f64 | `IfcElectricVoltageMeasure` | V |
| `apparent_power` | electrical | f64 | `IfcPowerMeasure` (VA; apparent flag in the template) | VA |
| `illuminance` | electrical | f64 | `IfcIlluminanceMeasure` | lx |
| `luminous_flux` | electrical | f64 | `IfcLuminousFluxMeasure` | lm |
| `luminous_intensity` | electrical | f64 | `IfcLuminousIntensityMeasure` | cd |
| `luminance` | electrical | f64 | `IfcReal` (no IFC measure) | cd/m² |
| `efficacy` | electrical | f64 | `IfcReal` | lm/W |
| `color_temperature` | electrical | f64 | `IfcThermodynamicTemperatureMeasure` | K |
| `resistivity` | electrical | f64 | `IfcElectricResistanceMeasure` × m (as `IfcReal`) | Ω·m |
| `number_of_poles` | electrical | i64 | `IfcCountMeasure` | — |
| `demand_factor` | electrical | f64 | `IfcRatioMeasure` | — |
| `global_warming_potential` | carbon (Reviera) | f64 | `IfcMassMeasure` (kg CO₂e) | kg CO₂e |
| `gwp_per_mass` / `gwp_per_volume` / `gwp_per_area` | carbon (Reviera) | f64 | `IfcReal` (unit in the template) | kgCO₂e/kg, /m³, /m² |

- Where IFC has no measure type, the value is written as `IfcReal`. The unit is given in the matching `IfcSimplePropertyTemplate.PrimaryUnit` when templates are exported (§2.11.4). Reviera reads the unit from the definition, not from the value.
- Changing the data type of an existing definition is not allowed for shared parameters (it would break GUID identity). For project and family parameters it is allowed only between compatible types (for example `number` ↔ `integer`, `text` ↔ `multiline_text`), with a preview of the converted values.

**Acceptance criteria.**
- [ ] For every row, setting a value, saving and reopening gives the same value, and the IFC property uses the listed value type.
- [ ] A third-party file with a property stored as `IfcThermalTransmittanceMeasure` is recognised as `heat_transfer_coefficient`, and displayed in the project's chosen unit.

### 2.9.5 Units
**ID:** DM-UNIT-001 · **Priority:** P0 · **Ref:** research/05 §8; screenshots/platform/project_units_dialog.png

**Purpose.** SI internally, any reasonable unit for display and input, set per discipline and per data type.

**Behaviour & rules.**
- **Internal units are SI**: metre, radian, kilogram, second, kelvin, ampere, mole, candela and their derived units. This differs from the reference product, which uses decimal feet internally. Conversion happens only at input (parsing) and output (formatting).
- **Unit catalogue.** Every unit has a key (`millimetre`), a versioned ID (`reviera:unit/millimetre@1`), symbols (`mm`), the data types it applies to, and an exact conversion to SI (scale + offset, held as rationals where possible, for example 1 in = 0.0254 m exactly). It shall include at least: mm, cm, dm, m, km, in, ft, ft-in (with fractional and decimal inch forms), yd, mi; mm², cm², m², ha, km², in², ft², ac; mm³, cm³, L, m³, in³, ft³, yd³, gal (US/UK); °, rad, grad, % slope, rise/run (1:n), rise per 12 in; kg, t, g, lb, kip; N, kN, lbf, kip-force; Pa, kPa, MPa, bar, psi, in H₂O, in Hg; °C, °F, K, °R (with offsets); L/s, m³/h, CFM, GPM; W, kW, BTU/h, ton of refrigeration; lx, fc; V, A, VA, kVA; and the project currency.
- **Project Units** (Manage › Settings › Project Units; shortcut UN):
  - a **discipline** selector (Common, Structural, HVAC, Electrical, Piping, Energy, Infrastructure);
  - per data type, a **format**: unit, **rounding** (number of decimal places, or a custom increment such as 5 mm or 1/16″), unit **symbol** (none, or one of the unit's symbols), and the flags *suppress trailing zeros*, *suppress 0 feet*, *suppress 0 inches*, *show + for positive values*, *use digit grouping*, *suppress spaces*;
  - **decimal symbol and digit grouping** (`123,456.78`, `123.456,78`, `123 456,78`, `12,34,567.89`).
- **Format overrides.** Dimension types, schedule fields, tag labels and parameters may override the project format (*Use project settings* off).
- **Input parsing.** Value fields accept any unit of the right dimension (`3' 6"`, `1067`, `1.067 m`, `42 in`). A bare number uses the field's display unit. Simple arithmetic is accepted (`=1200/2 + 50 mm`) through the formula evaluator (§2.8.7).
- **Presets.** Metric (mm), Metric (m), Imperial (ft-in), Imperial (decimal ft). Project templates store their unit set.
- **IFC.** `IfcUnitAssignment` stays SI (§2.3.1). Display formats are in RXD document `project/settings`. On export to third-party exchanges, an option lets the user write the project's display length unit as `IfcSIUnit` with a prefix (for example millimetre) or as `IfcConversionBasedUnit` (inch, foot), with values converted. This is for receivers that expect it.

**UI.** The Project Units dialog: discipline drop-down; a list of data types with their current format sample; a Format button for each, which opens the **Format** dialog (unit, rounding, symbol, flags, with a live sample); and decimal and grouping settings. Ref: screenshots/platform/project_units_dialog.png.

**Workflow (Project Units dialog, `units.edit()`).**
1. Manage › Settings › **Project Units** (UN). The dialog opens on the Common discipline.
2. Optionally choose a **preset** from the drop-down at the top (Metric (mm), Metric (m), Imperial (ft-in), Imperial (decimal ft)); this fills every format and can still be edited row by row.
3. Click a row's Format button. In the Format dialog choose the unit (only units whose dimension matches the data type are listed), the rounding, the symbol and the flags. The sample shows a fixed test value per data type (for Length, 1.2345678 m) formatted live.
4. **OK** in the Format dialog returns to the list; **OK** in Project Units applies every change as one transaction ("Project Units"). **Cancel** discards all.

**Validation.**
- A custom rounding increment must be positive and expressed in a unit of the same dimension; it is stored in SI and shown in the chosen unit.
- Decimal places range 0–12. Fractional-inch rounding offers 1/2 to 1/256 only for ft-in and in units.
- The decimal symbol and the digit-grouping symbol must differ; choosing the same character is refused.
- Flags that do not apply to the unit (for example *suppress 0 feet* for millimetres) are hidden, not just disabled.
- A row whose format is overridden somewhere (dimension types, schedule fields) shows the number of overrides; changing the project format does not change those overrides.

**Data (storage).** RXD document `project/settings` (formats per data type, decimal symbol, grouping, overrides catalogue). Overrides on dimension types or schedules are kept with those objects (DOC).

**Commands.**
`units.get() -> unit_settings` ·
`units.set_format(data_type, format)` ·
`units.set_preset(preset)` ·
`units.format(value, data_type, format?) -> text` ·
`units.parse(text, data_type) -> value` ·
`units.edit()` (opens the Project Units dialog)

**Acceptance criteria.**
- [ ] In Project Units, setting both the decimal symbol and digit grouping to "," is refused; setting Length to millimetres with 0 decimals shows "1235" as the sample, and OK creates one undo item.
- [ ] With Length set to feet and fractional inches, rounded to 1/16″, `units.format(1.0, "length")` returns `3' - 3 3/8"`, and parsing that string gives a value within 1/32″ of 1.0 m.
- [ ] Changing the display unit from mm to m changes no stored value. An IFC diff before and after shows only RXD changes.
- [ ] `units.parse("5' 6\"", "length")` returns 1.6764 m (exact to 1e-12).

### 2.9.6 Global parameters
**ID:** DM-GPAR-001 · **Priority:** P1 · **Ref:** research/05 §7.4, §2.3

**Purpose.** Named project-wide values that drive dimensions and element parameters.

**Workflow.**
1. Manage › Settings › **Global Parameters**: New, Edit, Delete, Duplicate. Sort and filter. Each row shows Name, Value, Formula, Reporting.
2. Create "CorridorWidth" (Length, group Dimensions) = 1800 mm.
3. Select a dimension between two corridor walls. In the dimension's contextual tab, choose **Label › CorridorWidth**. The walls move to satisfy the value.
4. Select doors. In Properties, use the small **"="** button beside *Sill Height* to associate it with the global "DoorSillHeight".

**Behaviour & rules.**
- A global parameter has a name, data type (any numeric type, plus `yes_no`, `text`, `integer`, `material`), group, an optional formula (over other globals), a **reporting** flag and a tooltip.
- A **driving** global can label instance dimensions (as an equality constraint, §2.4.4) and can be associated with instance parameters of matching data type (but not with type parameters or read-only parameters).
- A **reporting** global is set from a labelled dimension. It reads the measured value, and can be used in other globals' formulas. It cannot label more than one dimension.
- Deleting a global used by dimensions or parameters shows how many uses there are. The uses become plain values (dimensions are unlabelled, and parameters keep their current value).
- Associations are shown in Properties (the "=" button turns blue, and the value is greyed). **Select elements** on a global highlights everything it drives.
- **Global Parameters dialog** (`global.manage()`, shortcut GL):
  - a grid grouped by parameter group, with columns **Name | Value | Formula | Reporting | Uses**; formula-driven values are greyed; reporting rows show the dimension they read;
  - a toolbar: **New** and **Edit** (both open the Global Parameter Properties sub-dialog: name, discipline, data type, group, reporting, tooltip), **Duplicate**, **Delete**, Move Up/Down, Sort A–Z/Z–A, **Select elements**, and a search field;
  - values and formulas are edited in the grid; **Apply** commits pending edits as one transaction ("Global Parameters") and keeps the dialog open, **OK** applies and closes, **Cancel** discards uncommitted edits.
- **Validation.** Names are unique (case-insensitive) among globals, at most 255 characters, and may not contain `[ ] { } | ; < > @` (the `@` prefix is how formulas refer to globals, §2.8.7). Formulas are checked with `formula.validate` against the other globals: unknown names, unit dimension mismatch and cycles are shown under the cell and block Apply. The data type of a global cannot change once it has associations. A reporting global cannot be given a formula or a typed value. Deleting shows the use count first.

**Data (storage).** RXD document `standards/parameters` (`globals[]`: definition, value, formula, reporting, list of associations as element GUID + parameter key or dimension GUID).

**Commands.**
`global.create(name, data_type, value | formula, group?, reporting=false) -> global_id` ·
`global.set(global_id, value | {formula})` ·
`global.delete(global_id)` ·
`global.associate(global_id, element_ids[], param_ref)` ·
`global.dissociate(element_ids[], param_ref)` ·
`global.label_dimension(global_id, dimension_id)` ·
`global.list() -> global[]` ·
`global.select_driven(global_id) -> element_id[]` ·
`global.manage()` (opens the Global Parameters dialog)

**Acceptance criteria.**
- [ ] Changing "CorridorWidth" from 1800 mm to 2000 mm moves the labelled walls in one transaction, and undo restores both the value and the walls.
- [ ] A global formula `CorridorWidth * 2 + 300 mm` updates when CorridorWidth changes. A cycle between two globals is rejected.
- [ ] 200 doors associated with "DoorSillHeight" update together when it changes.

### 2.9.7 Parameter libraries (shared-parameter equivalent)
**ID:** DM-SPAR-001 · **Priority:** P1 · **Ref:** research/05 §7.2, §7.5, §16.7; screenshots/platform/edit_shared_parameters_dialog.png

**Purpose.** GUID-identified parameter definitions shared across projects and families, in an open, diff-friendly format of Reviera's own design.

**Behaviour & rules.**
- A **Reviera parameter library** is a UTF-8 JSON file with the extension `.rplib.json`. It is validated by a published JSON Schema (`reviera.paramlib`, version 1). A library contains **groups** (for organising only) and **definitions**. Several libraries may be active at once. Each has a stable library ID, so definitions from different libraries never clash.
- **Format (normative).**

```json
{
  "schema": "reviera.paramlib",
  "schema_version": "1.0.0",
  "library": {
    "id": "6f1c9d2e-8a4b-4c1e-9f0a-2b7d3e5c1a90",
    "name": "ACME Practice Parameters",
    "version": "2026.3.0",
    "publisher": "ACME Architects",
    "license": "CC-BY-4.0",
    "default_pset": "ACME_Common"
  },
  "groups": [
    { "id": "g-identity", "name": "Identity" },
    { "id": "g-acoustic", "name": "Acoustics" }
  ],
  "definitions": [
    {
      "guid": "3b0d2a4e-5f61-4c7a-9e2b-1d0c8f6a7b51",
      "name": "Acoustic Class",
      "data_type": "text",
      "enum": ["A", "B", "C", "D"],
      "group": "g-acoustic",
      "param_group": "identity_data",
      "discipline": "common",
      "tooltip": "Sound reduction class according to the practice standard.",
      "visible": true,
      "user_modifiable": true,
      "hide_when_no_value": false,
      "ifc": { "pset": "ACME_Common", "property": "AcousticClass" },
      "labels": ["acoustics", "doors", "walls"],
      "deprecated": false,
      "superseded_by": null
    }
  ]
}
```

- **Rules.**
  - `guid` is a canonical UUID string. It is the identity. `name`, `tooltip`, `group`, `labels` and `ifc` may change between library versions. `data_type` may **not** change.
  - Definitions are never deleted from a published library. They are marked `deprecated`, optionally with `superseded_by`.
  - The `ifc` mapping is the *default* IFC home. Projects can override it per category (§2.9.2).
  - Files are written with sorted keys and 2-space indentation, so they diff well in version control.
- **Embedding.** When a shared definition is used in a project or family, a full copy of it (with the library ID and version) is stored in the project's RXD document `standards/parameters`. Projects therefore never depend on the library file being present.
- **Conflicts.** If a project already has a definition with the same GUID but different metadata (for example a newer name), loading a family or importing a library offers *Keep project* or *Update from source*. If the data types differ, the newer definition is refused with an error that names both sources.
- **UI: Manage › Settings › Parameter Libraries**:
  - the active libraries list (Add file, Add URL, Remove, Create new);
  - for the selected library: a group tree and a definitions grid (search, filter by label or data type, show deprecated);
  - New, Edit and Deprecate definition; New and Rename group; **Export to IFC templates** (§2.11.4).
  - Read-only libraries (opened from a URL or a read-only file) show a lock.
  - The dialog (`paramlib.manage()`) edits library files, not the model, so its changes are not undoable in the project. Edits are held in memory per library and written on **Save library** or on **OK** (an atomic write, with sorted keys and 2-space indentation as the rules above require); **Cancel** discards unsaved edits after confirmation. A library changed on disk by another program while open is reloaded on request, with a notice if local edits would be lost.
  - **Definition editor validation:** a name is required (at most 255 characters, unique within its group, free of `[ ] { } | ; < >`); a data type is required and is locked once the definition has been saved; `enum` values must be unique and valid for the data type; `min` must not exceed `max`; the IFC property set must not start with `Reviera_`, and a `Pset_` home must name a property that exists in that standard set; `superseded_by` must name a non-deprecated definition with the same data type.
  - A column **In project** marks definitions already embedded in the active project, and **Add to project…** opens Project Parameters with the definition pre-selected (§2.9.3).
- **Team distribution (open alternative to a vendor cloud service).** A library can be opened from an HTTPS URL or a git repository path. Reviera caches it, and checks for updates on start-up and on demand. Firms publish their standard library as a versioned file in their own repository or web server. No vendor service is needed.
- **Import** of the reference product's shared-parameter text files is supported for migration. GUIDs, names, groups, data types (mapped to §2.9.4 keys), visibility and descriptions are preserved. Unknown data-type tokens go to a mapping dialog. **Export** to that text format is P2, for interoperability with firms that still use it.
- **IFC templates.** A library can also be written as an IFC file containing an `IfcProjectLibrary` with `IfcPropertySetTemplate` and `IfcSimplePropertyTemplate` entities. Each template's `GlobalId` is the definition GUID (encoded to 22 characters), with `PrimaryMeasureType` and units from §2.9.4. This is a read/write interchange form, for tools that use IFC templates.

**Commands.**
`paramlib.open(path_or_url) -> library_id` ·
`paramlib.create(path, name) -> library_id` ·
`paramlib.close(library_id)` ·
`paramlib.definition.add(library_id, def) -> guid` ·
`paramlib.definition.edit(library_id, guid, changes)` ·
`paramlib.definition.deprecate(library_id, guid, superseded_by?)` ·
`paramlib.group.add(library_id, name) -> group_id` · `paramlib.group.rename(library_id, group_id, name)` ·
`paramlib.import_legacy(path, target_library_id)` ·
`paramlib.export_ifc_templates(library_id, path)` ·
`paramlib.search(text?, labels?, data_type?) -> definition[]` ·
`paramlib.manage()` (opens the Parameter Libraries dialog)

**Acceptance criteria.**
- [ ] A library with 500 definitions opens in under 200 ms, and its JSON Schema validation passes.
- [ ] Importing a reference-product shared-parameter file with 50 parameters gives 50 definitions with identical GUIDs. A family using one of them, after round-tripping through Reviera, schedules in the same column as a project parameter with that GUID.
- [ ] Changing `data_type` of an existing GUID in a library file is reported as a validation error on open.
- [ ] A project that uses shared parameters opens and schedules correctly with every library file removed from the machine.

### 2.9.8 Extended properties (external data)
**ID:** DM-PARAM-005 · **Priority:** P3 (vendor cloud equivalent) / P2 (open alternative) · **Ref:** research/05 §7.1 (extended properties, 2027), §12.1

**Purpose.** Showing data from external systems (for example asset registers or coordination models) next to element parameters, without copying it into the model.

**Behaviour & rules.**
- The vendor-cloud feature is out of scope (P3).
- **Open alternative (P2):** an add-in API (§2.13) for **external property providers**. A provider registers a namespace, and answers `get_properties(guid[]) -> {name, value, data_type}[]` (read-only). Results are shown in Properties under the provider's name, and are available in schedules as *external fields*. They are never written to IFC unless the user chooses **Materialise**, which copies them into a property set.
- **Registration.** `reviera.external_props.register{namespace, name, fields = {{name, data_type, unit?}}, get_properties = fn, refresh_interval_s?}`. The declared `fields` list is what schedules and filters offer before any value is fetched. A provider needs trust level `standard` or higher when it reads files or the network (§2.13.7).
- **Fetching and caching.** Values are requested in batches of at most 1,000 GUIDs, on a worker coroutine (§2.13.6), never on the UI thread. They are cached per session with the time fetched. Properties shows cached values immediately, marked with the provider's icon and a "fetched hh:mm" tooltip; **Refresh** (per provider, or on the palette's context menu) re-fetches. A provider that times out (default 10 s per batch) or raises an error shows "unavailable" in its rows; the rest of the UI is unaffected.
- **Matching.** Providers key on IFC `GlobalId` by default. A provider may declare `key = {param = "<param_ref>"}` instead (for example an asset tag), in which case Reviera sends that parameter's values.
- **Use in documentation.** External fields can be shown in schedules, used in schedule filters and sorting, and in view filters (DOC). They cannot be edited, used in family formulas, or drive geometry. A schedule that uses an external field shows a "live data" note with the fetch time, and PDF export prints the fetch time in the schedule footer.
- **Materialise.** Copies the current values into a user property set as one undoable transaction, creating project parameter bindings if needed (§2.9.3). Data types are preserved; unknown types become `text`.
- **Data (storage).** Nothing is stored in the model unless materialised. The list of providers a schedule uses is kept in that schedule's RXD document, so opening the project without the add-in shows the columns as "provider missing".

**Commands.** `external_props.list_providers() -> provider[]` · `external_props.materialise(provider, element_ids[], pset_name)`

**Acceptance criteria.**
- [ ] A sample Lua provider that reads a CSV keyed by GUID shows its values in Properties for matching elements, and *Materialise* writes them to the chosen property set.
- [ ] A provider that sleeps for 30 s does not freeze the UI; its rows show "unavailable" after the 10 s timeout, and other parameters remain editable.
- [ ] A schedule with an external field, opened without the provider's add-in, keeps the column and shows "provider missing" instead of failing.

---

## 2.10 Project setup and the Manage features

### 2.10.1 Manage tab
**ID:** DM-SET-001 · **Priority:** P0 · **Ref:** research/05 §9 (intro); screenshots/platform/manage_tab_settings_panel.png, screenshots/platform/manage_tab_visual_programming_panel.jpg

**Purpose.** One ribbon tab for project-wide settings and model-management tools.

**UI.** Ribbon **Manage** tab, with these panels (each tool is a registered command; shortcuts are in the UI chapter):

| Panel | Tools |
|---|---|
| Settings | Materials · Object Styles · Snaps · Project Information · Project Parameters · Parameter Libraries · Global Parameters · Transfer Project Standards · Purge Unused · Project Units · Structural Settings ▾ · MEP Settings ▾ · Panel Schedule Templates ▾ · Additional Settings ▾ (Line Styles, Line Weights, Line Patterns, Fill Patterns, Material Assets, Analysis Display Styles, Sheet Issues/Revisions, Arrowheads, Temporary Dimensions, Detail Level, Halftone/Underlay, Assembly Code, Keynoting, Classification Systems, Area and Volume Computations, Color Fill Schemes) |
| Project Location | Location · Coordinates ▾ · Position ▾ |
| Design Options | Design Options · Pick to Edit · Add to Set (ARC chapter) |
| Manage Project | Manage Links · Manage Images (§2.10.14) · Starting View · Decal Types (§2.10.15) |
| Phasing | Phases (§2.10.12) |
| Numbering | Rule-Based Numbering (§2.10.16) |
| Selection | Save · Load · Edit (UI chapter) |
| Inquiry | IDs of Selection · Select by ID · Review Warnings · Extension Data Status (§2.4.3) |
| Scripts | Script Manager · Run Script… · Script Security (§2.13.7, §2.13.8) |
| Add-ins | Add-in Manager (§2.14) |
| Visual Programming | Reviera Graph · Graph Player (§2.15, P2) |
| AI Agents | MCP Server status · Permissions · Session log (§2.16) |

**Acceptance criteria.**
- [ ] Every tool in the table runs a registered command, and so is callable from Lua and MCP with the same result as from the ribbon.

### 2.10.2 Project information
**ID:** DM-SET-002 · **Priority:** P0 · **Ref:** research/05 §9.1

**Purpose.** A single record of project identity, used by title blocks, schedules and exports.

**Behaviour & rules.**
- The **Project Information** dialog edits these fields, and any project parameters bound to the *Project Information* category:

| Field | IFC home |
|---|---|
| Project Name | `IfcProject.LongName` (with `IfcProject.Name` holding the project number, the usual convention) |
| Project Number | `IfcProject.Name` |
| Project Status / Phase | `IfcProject.Phase` |
| Project Issue Date | `Identity Data`.`ProjectIssueDate` on `IfcProject` (`IfcDate`) |
| Client Name | `IfcActor` (role `CLIENT`) with `IfcRelAssignsToActor` → `IfcProject` |
| Project Address | `IfcPostalAddress` on `IfcBuilding.BuildingAddress` (and `IfcSite.SiteAddress`) |
| Building Name | `IfcBuilding.Name` / `LongName` |
| Organisation Name / Description | `IfcOrganization` (owning organisation; also `IfcOwnerHistory` when written) |
| Author | `FILE_NAME` author, and `IfcPerson` |
| Energy settings, route-analysis settings | RXD document `project/settings` (ANA chapter) |

- Project information is one element (a singleton, internal ID 1). It cannot be deleted.
- **Project Information dialog** (`project.info.edit()`; Manage › Settings › Project Information). A property grid grouped as *Identity Data* (organisation, author, building name), *Project* (name, number, status, issue date, client), *Address* and *Other*, followed by every project parameter bound to the Project Information category under its own group.
  - **Project Address** opens a structured address editor (address lines, town, region, postal code, country) that writes `IfcPostalAddress`; a free-text address from a third-party file is shown in the first line and kept unchanged until edited.
  - *Energy Settings* and *Route Analysis Settings* rows open their own dialogs (ANA chapter).
  - **Validation.** Project Number may be empty but, when set, must be at most 255 characters (`IfcLabel`). Issue Date accepts ISO 8601 or the user's locale format and is stored as `IfcDate`. Text fields over 255 characters are refused, except the description fields, which are `IfcText`. Shared parameters bound to Project Information use their own data-type validation (§2.9.4).
  - **OK** applies all changed fields as one transaction ("Project Information"); **Cancel** discards them. The dialog is also reachable from Properties when nothing is selected and the active view is a sheet.
- In BIMserver mode, project information is an ordinary element for ownership: editing it takes its advisory lock (COL).

**Commands.** `project.info.get() -> info` · `project.info.set(fields)` · `project.info.edit()` (opens the Project Information dialog)

**Acceptance criteria.**
- [ ] Setting Client Name to "City of Example" produces an `IfcActor` with role CLIENT assigned to the project, and a title-block label bound to Client Name shows it.
- [ ] Entering an address in the structured editor writes an `IfcPostalAddress` with separate `Town`, `PostalCode` and `Country`, which a third-party viewer shows; an issue date typed as `12/10/2026` in a UK locale is stored as `2026-10-12`.

### 2.10.3 Object styles and subcategories
**ID:** DM-OBJS-001 · **Priority:** P0 · **Ref:** research/05 §9.2

**Purpose.** Default graphics for every category and subcategory.

**UI.** Manage › Settings › **Object Styles**. Tabs: *Model Objects*, *Annotation Objects*, *Analytical Model Objects*, *Imported Objects*. A tree grid (category with subcategories) with columns: **Line Weight Projection**, **Line Weight Cut** (only for cuttable categories), **Line Colour**, **Line Pattern**, **Material**. Buttons: *Modify Subcategories* (New, Delete, Rename), *Select All / None / Invert*, filter by discipline.

**Behaviour & rules.**
- Object styles are the **lowest** layer of graphic precedence. View-level overrides (visibility/graphics, filters, view templates; DOC chapter) take precedence over them.
- Built-in subcategories (for example Doors › Frame/Mullion, Glass, Opening, Panel, Plan Swing, Elevation Swing; Walls › Common Edges, Hidden Lines; Windows › Sill/Head, Trim; Imported Objects › one subcategory per imported layer) are created automatically. They cannot be deleted.
- Imported CAD layers become subcategories of *Imported Objects* › `<file name>` (§2.11.5).
- **IFC.** For other tools, each category and subcategory used by model geometry is written as an `IfcPresentationLayerWithStyle`. Its name is `<Category>` or `<Category>|<Subcategory>`, and it has `LayerOn = TRUE` and `LayerStyles` = an `IfcCurveStyle` (projection colour and width). The `AssignedItems` are the representation items of that subcategory. Cut weights, patterns and materials are only in RXD.

**Data (storage).** RXD document `standards/styles` (`object_styles[]`: category or subcategory key and GUID, projection/cut weight, colour, pattern, material GUID).

**Commands.**
`object_style.set(category_or_subcategory, {projection_weight?, cut_weight?, color?, pattern?, material?})` ·
`object_style.get(category_or_subcategory) -> style`
(plus `subcategory.create/rename/delete`, §2.2.2)

**Acceptance criteria.**
- [ ] Setting Walls cut line weight to pen 5 changes the cut walls in every plan view that has no override, and leaves views with an override unchanged.
- [ ] A third-party viewer lists layers "Doors|Panel" and "Doors|Frame/Mullion", and toggling them hides the matching parts of the doors.

### 2.10.4 Line weights, line patterns, line styles and fill patterns
**ID:** DM-LINE-001 (line weights, patterns, styles) · DM-PAT-001 (fill patterns) · **Priority:** P0 · **Ref:** research/05 §9.3

**Behaviour & rules.**
- **Line weights** (Manage › Additional Settings › Line Weights). There are **16 pens**, on three tabs:
  - **Model** line weights are scale-dependent. They form a table of pen thickness (mm on paper) by view scale. Columns 1:10 … 1:5000 can be added and removed. Weights for scales between columns are taken from the nearest column at a larger scale.
  - **Perspective** line weights are scale-independent.
  - **Annotation** line weights are scale-independent.
- **Line patterns**: named sequences of dash, dot and space lengths (paper units). Defaults: Solid, Dash, Dot, Dash Dot, Hidden, Centre, Phantom, Overhead, Demolished, … Editing a pattern updates every use.
- **Line styles** (Manage › Additional Settings › Line Styles): subcategories of the *Lines* category, each with weight, colour and pattern. Built-in styles: Thin Lines, Medium Lines, Wide Lines, Hidden Lines, Overhead Lines, `<Sketch>`, `<Room Separation>`, `<Area Boundary>`, `<Space Separation>`, `<Centerline>`, `<Demolished>`, `<Beyond>`, `<Insulation Batting Lines>`. Users may add styles. Built-in styles cannot be deleted.
- **Fill patterns** (Manage › Additional Settings › Fill Patterns):
  - **Drafting** patterns are defined in paper units, and keep the same size at any scale.
  - **Model** patterns are defined in real-world units, align to geometry, and can be moved and rotated on faces.
  - Each pattern is *simple* (parallel lines or crosshatch, with angle and spacing), *custom* (a grid of line families with angle, origin, offset, shift and dash sequence), or *solid fill*.
  - Patterns may be **rotated** (custom patterns too).
  - **Double fill**: filled regions, materials and overrides may use a **foreground** and a **background** pattern, each with its own colour.
- **Pattern files.** Reviera reads and writes **`.pat`** files (the widely used, publicly documented hatch format used by many CAD tools) for import and export. Its own storage is the RXD form. Reviera ships its own default pattern set, authored from scratch.

**Data (storage).** RXD document `standards/styles` (`line_weights`, `line_patterns[]`, `line_styles[]`, `fill_patterns[]`). In IFC, a material's cut and surface patterns are also written as `IfcFillAreaStyle` with `IfcFillAreaStyleHatching` (simple and crosshatch patterns), so other tools can draw them (§2.10.5).

**Commands.**
`line_weight.set(kind: "model"|"perspective"|"annotation", pen, scale?, thickness_mm)` ·
`line_weight.add_scale(scale)` · `line_weight.remove_scale(scale)` ·
`line_pattern.create(name, segments[]) -> id` · `line_pattern.edit(id, segments[])` · `line_pattern.delete(id, replace_with?)` ·
`line_style.create(name, weight, color, pattern) -> id` · `line_style.edit(id, props)` · `line_style.delete(id, replace_with?)` ·
`fill_pattern.create(name, kind: "drafting"|"model", definition) -> id` · `fill_pattern.edit(id, definition)` · `fill_pattern.delete(id, replace_with?)` ·
`fill_pattern.import_pat(path, names?[], kind) -> id[]` · `fill_pattern.export_pat(ids[], path)`

**Acceptance criteria.**
- [ ] Pen 3 in the model table at 1:100 = 0.35 mm, and at 1:200 = 0.25 mm. A wall drawn with pen 3 prints at those widths in PDFs at each scale (DOC).
- [ ] A `.pat` file with 5 patterns imports, renders the same as a reference rendering within a tolerance of 1 px at 300 dpi, and exports back to an equivalent `.pat`.
- [ ] A double fill (foreground diagonal in red, background solid in light grey) shows both layers in a filled region and in a material's cut pattern.

### 2.10.5 Materials
**ID:** DM-MAT-001 … 007 · **Priority:** P0 (identity, graphics, basic appearance) / P1 (physical, thermal, libraries) / P1 (carbon) · **Ref:** research/05 §10, §4.8; screenshots/platform/material_browser_appearance_tab_2026.png, screenshots/platform/material_browser_physical_asset_annotated_2026.png, screenshots/platform/material_carbon_information_2027.png

**Purpose.** A material combines identity, drawing graphics, rendering appearance, and physical, thermal and carbon data.

#### DM-MAT-001 — Material element and IFC mapping (P0)
- Each material is an **`IfcMaterial`** (`Name`, `Description`, `Category` = the material class, for example `concrete`, `steel`, `wood`, `masonry`, `glass`, `gypsum`, `insulation`, `plastic`, following buildingSMART recommendations). It has a RXD GUID (§2.5).
- Material data is spread across IFC and RXD like this:

| Tab | Data | IFC | RXD (document `materials`) |
|---|---|---|---|
| **Identity** | Name, description, class, comments, keywords, manufacturer, model, cost, URL, keynote, mark | `IfcMaterial` attributes; `IfcMaterialProperties` `Pset_MaterialCommon` (where it applies) plus a material property set `Identity Data` (manufacturer, model, cost, URL, keynote, mark); `IfcExternalReferenceRelationship` for library origin | comments, keywords |
| **Graphics** | Shading colour and transparency ("use appearance colour"), surface pattern (foreground/background, colour, alignment), cut pattern (foreground/background, colour) | `IfcMaterialDefinitionRepresentation` → `IfcStyledRepresentation` → `IfcSurfaceStyle` (`IfcSurfaceStyleShading`/`Rendering`), plus `IfcFillAreaStyle` for patterns | pattern references and alignment |
| **Appearance** | Rendering asset (§DM-MAT-002) | `IfcSurfaceStyleRendering` (diffuse colour, transparency, specular, reflectance method), `IfcSurfaceStyleWithTextures` / `IfcImageTexture` | the full PBR asset |
| **Physical** | Structural asset (§DM-MAT-003) | `Pset_MaterialMechanical`, `Pset_MaterialCommon`, `Pset_MaterialSteel`, `Pset_MaterialConcrete`, `Pset_MaterialWood` (as `IfcMaterialProperties`) | asset identity and sharing |
| **Thermal** | Thermal asset (§DM-MAT-004) | `Pset_MaterialThermal`, `Pset_MaterialHygroscopic`, `Pset_MaterialOptical`, `Pset_MaterialEnergy` | asset identity and sharing |
| **Carbon** | Embodied carbon (§DM-MAT-005) | material property set `Carbon` (GWP values) | database link and record metadata |

- **Assets** (appearance, physical, thermal, carbon) are separate, shareable records in RXD document `standards/materials`. Several materials may share one asset. The UI shows a sharing indicator with the user count, and offers *Duplicate asset* to make a private copy. When written to IFC, shared asset values are copied into each material's property sets.
- **Names.** Material names are unique in a project (case-insensitive) and at most 255 characters. Because IFC identifies non-rooted materials by name, renaming updates the RXD resource key (§2.5) and every `IfcMaterial.Name`, layer set and constituent set that uses it, in one transaction.
- **Default material.** Every project has a built-in *Default* material, used where an element or family form says `<By Category>` and the category's object style has no material (§2.10.3). It cannot be deleted or renamed, but its values can be edited.
- **Third-party materials.** An `IfcMaterial` found in an opened file with no RXD becomes a Reviera material: its `IfcMaterialProperties` populate the matching asset fields, its surface style populates Graphics and Appearance, and properties with no Reviera home are kept as IFC-native parameters of the material (§2.9.1). Two third-party materials with the same name are kept apart by suffixing " (2)" on the second, and the change is reported.
- **Where materials are used.** Materials are assigned to layers of layered types, to profile sets, to family forms (fixed or by parameter), to painted faces (ARC) and through object styles. `material.list` reports, per material, the number of types and instances that use it, which the browser shows as a count.
- **Acceptance.** Renaming "Concrete C30" to "Concrete C30/37" changes the name in every layer set of the exported IFC and keeps the material's RXD GUID; opening a third-party file with an `IfcMaterial` "Brick" carrying `Pset_MaterialCommon.MassDensity = 1800` shows 1800 kg/m³ on the Physical tab.

#### DM-MAT-002 — Appearance (P0 basic, P2 full rendering)
- The appearance model is a **physically based (metal/roughness) material**, compatible with glTF 2.0 PBR, so export to glTF (§2.11.5) is lossless. Fields:
  - base colour or texture;
  - metallic, roughness;
  - normal and bump map;
  - occlusion;
  - emissive colour and strength;
  - transmission and IOR (glass);
  - opacity and cutout map;
  - texture transform (scale in real-world units, offset, rotation);
  - tint.
- **Schema presets** (Generic, Ceramic, Concrete, Glazing, Hardwood, Masonry, Metal, Metallic Paint, Mirror, Plastic, Solid Glass, Stone, Wall Paint, Water) are templates that set sensible PBR values. They are UI presets, not separate schemas.
- Textures are RXD assets. When "Embed textures in IFC" is on (default for textures ≤ 1 MB), they are also written as `IfcBlobTexture` (PNG/JPEG). Otherwise they are written as `IfcImageTexture` with a relative URL.
- Thumbnails are rendered locally (sphere, cube or plane swatch).

#### DM-MAT-003 — Physical (structural) asset (P1)
- Behaviour: isotropic, orthotropic or transversely isotropic.
- Basic thermal: thermal expansion coefficient (by direction).
- Mechanical: Young's modulus, Poisson's ratio, shear modulus (by direction), density.
- Strength: yield strength, tensile strength, and per-class fields (concrete compressive strength, lightweight flag, shear strength modification; wood species, grade, bending, compression parallel/perpendicular, shear, tension; steel resistance/reduction factors).
- IFC: `Pset_MaterialMechanical` (YoungModulus, ShearModulus, PoissonRatio, ThermalExpansionCoefficient), `Pset_MaterialCommon` (MassDensity, Porosity), `Pset_MaterialSteel` (YieldStress, UltimateStress, …), `Pset_MaterialConcrete` (CompressiveStrength, MaxAggregateSize, …), `Pset_MaterialWood` (Species, StrengthGrade, …). Values with no standard home go to the material property set `Physical`.

#### DM-MAT-004 — Thermal asset (P1)
- Fields: thermal conductivity, specific heat, density, emissivity, permeability, porosity, reflectivity, electrical resistivity, transmits light (Yes/No), and for gases and liquids, compressibility and viscosity.
- IFC: `Pset_MaterialThermal` (ThermalConductivity, SpecificHeatCapacity), `Pset_MaterialCommon` (MassDensity, Porosity), `Pset_MaterialOptical` (emissivity as ThermalIrEmissivityFront/Back; reflectance), `Pset_MaterialHygroscopic` (VaporPermeability). The rest go to the material property set `Thermal`.
- **Assembly thermal values.** Layered types compute *Heat Transfer Coefficient (U)*, *Thermal Resistance (R)* and *Thermal Mass* from their layers and thicknesses (with standard surface resistances, configurable per project in Energy Settings). These are written to `Pset_*Common.ThermalTransmittance` on the type (§2.3.3).

#### DM-MAT-005 — Carbon asset (P1)
- Fields:
  - **database** (free text, with presets such as "ÖKOBAUDAT", "ICE", "EC3", "Manual") and **record ID / URL**;
  - **declared unit** (kg, m³, m², piece);
  - **GWP A1–A3** (kg CO₂e per declared unit), and optional **A4, A5, B, C1–C4, D** stages;
  - **biogenic carbon**, **reference service life**, **EPD validity date**, **notes**.
- The carbon asset is used by carbon reporting (ANA chapter). Element-level results are calculated from material quantities (volume × density → mass, then × GWP/kg, or area × GWP/m²). They are written to `Pset_EnvironmentalImpactIndicators.ClimateChangePerUnit` and the related properties of the element type, when *Write carbon results to IFC* is on.
- **Data connectors** to online EPD databases are add-ins (§2.14), not built in. The open data sets that Reviera may bundle shall be checked for licence compatibility before inclusion.

#### DM-MAT-006 — Material Browser and libraries (P0 basic, P1 libraries)
- **Material Browser** (Manage › Settings › Materials; shortcut MAT):
  - **left top**: project materials (search, filter by class, list or thumbnail view);
  - **left bottom**: library panel (shipped Reviera library, Favourites, user libraries; tree by class);
  - **right**: the editor, with tabs Identity / Graphics / Appearance / Physical / Thermal / Carbon;
  - **toolbar**: New, Duplicate, Rename, Delete, Add to Favourites, Add from library to project (also by drag-and-drop), Create/Open/Remove library;
  - each asset tab has *Replace asset* (opens the **Asset Browser**), *Duplicate asset*, *Remove asset*;
  - batch operations: select several materials and change a shared field (for example class, cut pattern) at once.
- **Material libraries** are IFC files (`<Name>.materials.ifc`). The file has one `IfcProject` that declares an `IfcProjectLibrary` named after the library. Each material is associated with the library through `IfcRelAssociatesMaterial`, so the file is valid IFC. Assets and other RXD go in the library's RXD documents (`materials`), carried as `02a` defines. Textures are RXD assets.
- Reviera ships a starter library authored from scratch, with appearance textures under compatible licences (for example CC0).
- **Deleting** a material in use asks for a replacement material. Purge removes unused materials and assets (§2.10.11).
- **Browser workflow** (`material.browser(material_id?)`). The browser opens on the given material, on the material of the selected element's first layer or form, or on the last material viewed. Selecting a project material loads it into the editor. Edits on the editor tabs are held as pending changes; **Apply** commits them as one transaction ("Edit Material: <name>"), **OK** applies and closes, **Cancel** discards pending changes. Switching to another material with pending changes asks *Apply*, *Discard* or *Cancel*.
- **Project list.** Search matches name, description, class, keywords and comments. Filters: class, *Used in project* / *Unused*, *Has appearance texture*, *Has carbon data*. Each row shows a swatch, the name and a use count. Multi-selection enables the batch edit described above; fields that differ across the selection show "<varies>".
- **Library panel.** The tree lists each open library by class. Library materials can be previewed in the editor read-only; **Add to project** copies the material with its assets (`material.library.add_to_project`). If a project material with the same name exists, the user chooses *Replace values* (keeps the project GUID), *Add as "<name> (2)"* or *Skip*. Read-only libraries (shipped, or opened from a read-only location) show a lock; dragging a project material onto an editable library adds it there (`material.library.add_from_project`).
- **Validation.** Name rules as in DM-MAT-001. Numeric asset fields are unit-parsed (§2.9.5) and range-checked (for example density > 0, Poisson's ratio between −1 and 0.5, transparency 0–100 %, roughness 0–1); invalid fields block Apply. Pattern pickers list only drafting patterns for Cut and both kinds for Surface (§2.10.4).

**Commands.**
`material.create(name, class?, from_library?) -> material_id` ·
`material.duplicate(material_id, name, share_assets=true) -> material_id` ·
`material.rename(material_id, name)` ·
`material.delete(material_id, replace_with?)` ·
`material.set(material_id, {identity?, graphics?, appearance?, physical?, thermal?, carbon?})` ·
`material.asset.duplicate(material_id, asset_kind) -> asset_id` ·
`material.asset.replace(material_id, asset_kind, asset_id)` ·
`material.library.open(path) -> library_id` · `material.library.create(path, name) -> library_id` ·
`material.library.add_to_project(library_id, material_names[]) -> material_id[]` ·
`material.library.add_from_project(library_id, material_ids[])` ·
`material.list(filter?) -> material[]` ·
`material.browser(material_id?)` (opens the Material Browser)

**Acceptance criteria.**
- [ ] A material with a red concrete appearance, a diagonal cut pattern, and density 2400 kg/m³ exports to IFC with `IfcSurfaceStyleRendering` (red), an `IfcFillAreaStyleHatching`, and `Pset_MaterialCommon.MassDensity = 2400`. A third-party viewer renders the wall in red.
- [ ] Two materials sharing a physical asset both update when the asset's density changes. After *Duplicate asset* on one, they are independent.
- [ ] A wall type with layers 15 mm gypsum (λ = 0.25), 100 mm mineral wool (λ = 0.035) and 15 mm gypsum reports U ≈ 0.32 W/(m²·K) (with Rsi + Rse = 0.17 m²K/W), and writes it to `Pset_WallCommon.ThermalTransmittance`.
- [ ] A material with carbon GWP A1–A3 = 0.15 kg CO₂e/kg and density 2400 kg/m³, used for 10 m³ of wall, reports 3,600 kg CO₂e for those walls.
- [ ] A material library saved by Reviera opens as valid IFC4, and its materials can be added to a second project, keeping all assets.

#### DM-MAT-007 — Asset Browser and asset editing (P1)
**ID:** DM-MAT-007 · **Priority:** P1 · **Ref:** research/05 §10; screenshots/platform/material_browser_physical_asset_annotated_2026.png

**Purpose.** Assets (appearance, physical, thermal, carbon) are shared records that several materials can use. The Asset Browser finds and swaps them, and the asset tabs edit them safely.

**Workflow.**
1. On an asset tab of the Material Browser choose **Replace asset**, or open Manage › Additional Settings › **Material Assets** (`material.assets(material_id?)`). The **Asset Browser** opens filtered to the asset kind of the tab (or, from the Manage menu, to all kinds).
2. Browse the *Project* assets and each library's assets by kind and class (for example Physical › Concrete), or search by name, keyword or property value (for example "density > 2000 kg/m³").
3. Select an asset to preview its values (and, for appearance, a rendered swatch).
4. **Replace in material** swaps the material's asset for the chosen one (`material.asset.replace`), or **Add to project** copies a library asset without assigning it.

**UI.** Asset Browser: kind tabs (Appearance, Physical, Thermal, Carbon); a tree of sources (Project, Reviera library, user libraries) and classes; a results list (name, class, users, source); a preview pane. Toolbar: Replace in material, Add to project, Duplicate, Rename, Delete (project assets only), Show users.

**Behaviour & rules.**
- An asset tab shows the asset's name and a **sharing indicator** ("Shared by 4 materials"). Editing a shared asset asks first whether to edit it for all users or to **Duplicate asset** for this material only (`material.asset.duplicate`). The choice can be remembered for the session.
- Assets have names unique per kind within the project; library assets keep their library identity so a later *Add to project* of the same asset is recognised and not duplicated.
- An asset with no users is kept until purged (§2.10.11). Deleting an asset in use is refused; the user must replace it in each material first (Show users lists them).
- Physical and thermal assets whose values have a standard IFC home are written into each user material's property sets on save (DM-MAT-001); an asset edit therefore marks every user material as changed.
- Appearance assets with textures show texture size and source; a missing texture file shows a broken-link badge and offers *Reload from…* (§2.10.14).

**Data (storage).** Assets are records in RXD document `standards/materials` with kinds per asset type, referenced by GUID from each material; IFC carries copies of their standard values (DM-MAT-001).

**Commands.** `material.assets(material_id?)` (opens the Asset Browser) · `material.asset.duplicate(material_id, asset_kind) -> asset_id` · `material.asset.replace(material_id, asset_kind, asset_id)`

**Acceptance criteria.**
- [ ] Editing the density on a physical asset shared by 3 materials, and choosing *Duplicate asset*, changes only the edited material; the other two keep the old value and the indicator on the edited one reads "Not shared".
- [ ] Deleting an asset used by 2 materials is refused with both material names listed.
- [ ] Searching the Asset Browser for "density > 2000 kg/m³" lists only physical assets whose density exceeds that value.

### 2.10.6 Additional settings (annotation and computation)
**ID:** DM-SET-003 · **Priority:** P0 (temporary dimensions, detail level, area/volume) / P1 (others) · **Ref:** research/05 §9.4

**Behaviour & rules.** These settings are project-wide. Their behaviour is defined in the chapters named. This chapter only defines their storage (RXD document `project/settings`, or `styles` for graphic styles) and their commands:

| Setting | Behaviour defined in | Command |
|---|---|---|
| Arrowheads (Arrow, Dot, Diagonal, Heavy end tick, Datum triangle, Elevation target, Loop, Box, Kink, …) | DOC | `settings.arrowhead.create/edit/delete` |
| Temporary dimensions (walls: centreline, faces, core centre, core faces; doors and windows: centreline, openings) | UI | `settings.temporary_dimensions.set(walls, doors_windows)` |
| Detail level by view scale (Coarse/Medium/Fine thresholds) | DOC | `settings.detail_level_by_scale.set(table)` |
| Halftone and underlay | DOC | `settings.halftone.set(props)` |
| Area and volume computations (room area at wall finish, centre, core layer, core centre; volume computation on/off; area schemes) | ARC | `settings.area_volume.set(props)` |
| Colour fill schemes | DOC | (DOC chapter) |
| Sun settings | DOC/ANA | (DOC chapter) |
| Callout, elevation and section tags; spot dimension settings | DOC | (DOC chapter) |
| Keynoting settings (file path, numbering) | DOC | `settings.keynoting.set(path, numbering)` |
| Assembly code / classification tables | §2.10.10 | — |
| Structural settings (symbolic cut-backs, brace symbols, load cases and combinations, analytical settings, boundary-condition symbols, rebar cover) | STR | (STR chapter) |
| MEP settings (mechanical, electrical, piping, load classifications, demand factors, building and space types) | MEP | (MEP chapter) |
| Panel schedule templates | MEP | (MEP chapter) |

**Acceptance criteria.**
- [ ] Changing room area computation from "at wall finish" to "at wall centre" recomputes every room area in one transaction.

### 2.10.7 Project location, coordinates and position
**ID:** DM-LOC-001 · **Priority:** P0 (internal origin, base points, true north, lat/long) / P1 (map search, weather, named sites, acquire/publish) · **Ref:** research/05 §9.8; screenshots/platform/location_and_site_dialog_2026.png

**Purpose.** To place the project on Earth, and relate its internal coordinates to survey and shared coordinates.

**UI.** Manage › Project Location › **Location** opens the *Location, Weather and Site* dialog:
- **Location** tab: *Define location by* — a map search (an address search and map, using an open map service such as OpenStreetMap/Nominatim; configurable, with no vendor service), or a **city list** (bundled, open data) with latitude, longitude, time zone and daylight saving.
- **Weather** tab: heating and cooling design temperatures, clearness number. Use the nearest weather station (from an open data set such as the ASHRAE climate design conditions or EnergyPlus EPW metadata, subject to licence checks), or enter values.
- **Site** tab: named **sites** (shared-coordinate positions of this model in a larger context), each with an angle from project north to true north. Make current, Duplicate, Rename, Delete.

Coordinates ▾: *Acquire Coordinates* (from a link), *Publish Coordinates* (to a link), *Specify Coordinates at Point*, *Report Shared Coordinates*. Position ▾: *Relocate Project*, *Rotate True North*, *Rotate Project North*, *Mirror Project*.

**Behaviour & rules.**
- Three reference points:
  - **Internal origin**: fixed, the origin of internal coordinates. It is never moved.
  - **Project base point**: a user-placed point that defines project coordinates (and, optionally, the zero for level elevations).
  - **Survey point**: a user-placed point linking the project to a real-world coordinate system.
- **IFC mapping:**
  - `IfcSite.RefLatitude`, `RefLongitude` (as `IfcCompoundPlaneAngleMeasure`) and `RefElevation`;
  - **geo-referencing** with **`IfcMapConversion`** (`Eastings`, `Northings`, `OrthogonalHeight`, `XAxisAbscissa`, `XAxisOrdinate`, `Scale`) from the model's context to an **`IfcProjectedCRS`** (`Name` = an EPSG code such as `EPSG:25832`, plus `GeodeticDatum`, `VerticalDatum`, `MapProjection`, `MapZone` and `MapUnit` where known), calculated from the survey point;
  - project north and true north through the context's `TrueNorth` direction;
  - the project base point is written as the `IfcSite` placement (default) or as the world coordinate system, chosen in the IFC export setup (§2.11.4).
- Named sites, the base point and survey point positions, clipped/unclipped state, and weather data are in RXD document `project/location`.
- **Relocate**, **Rotate True North** and **Mirror** change the coordinate transforms (and, for mirror, the geometry). They are single transactions, and are undoable.
- A CRS database (PROJ, MIT licence) is used to validate EPSG codes and, optionally, to convert latitude and longitude to map coordinates.

**Commands.**
`location.set(lat, lon, elevation?, time_zone?, dst?)` ·
`location.search(address) -> candidates[]` ·
`location.weather.set(props | {station_id})` ·
`site.position.create(name) -> id` · `site.position.set_current(id)` · `site.position.rename(id, name)` · `site.position.delete(id)` ·
`coordinates.set_survey_point(point, shared_coords)` ·
`coordinates.set_project_base_point(point, angle_to_true_north?)` ·
`coordinates.set_crs(epsg_code)` ·
`coordinates.acquire(link_instance_id)` · `coordinates.publish(link_instance_id, site_position_id)` ·
`coordinates.report(point) -> {internal, project, shared, lat_lon?}` ·
`position.relocate(translation, rotation?)` · `position.rotate_true_north(angle)` · `position.rotate_project_north(angle, align_views=false)` · `position.mirror(axis)`

**Acceptance criteria.**
- [ ] Setting the survey point to E 500000, N 5700000 with CRS EPSG:25832 writes an `IfcMapConversion` and `IfcProjectedCRS`, and a third-party geo-aware viewer places the model at that location.
- [ ] Rotating true north by 30° changes `TrueNorth` and the shadow direction of sun studies, but no element coordinates.
- [ ] Report Shared Coordinates at a picked point gives values consistent with the survey point within 0.1 mm.

### 2.10.8 Snaps
**ID:** DM-SET-004 · **Priority:** P0 · **Ref:** research/05 §9.5

**Purpose.** Consistent, predictable snapping for every pick, in every tool, with user control over what snaps and at what increments.

**UI.** Manage › Settings › **Snaps** dialog: *Snaps Off* master switch; *Dimension snaps* with two lists (length increments and angular increments, each a semicolon-separated list such as `1000; 100; 20; 5` mm and `90; 45; 15; 5; 1` °); an *Object snaps* check list with each snap's override shortcut beside it; *Check all* / *Check none*; *Snap to remote objects* and *Snap to point clouds*; *Restore defaults*; and, when the user has rights, *Save as project default*.

**Behaviour & rules.**
- Settings: dimension snap increments (length and angle), which depend on zoom (a list of increments). Object snaps: endpoints, midpoints, nearest, work plane grid, quadrants, intersections, centres, perpendicular, tangents, points, *snap to remote objects*, *snap to point clouds*. Each snap has a two-letter override shortcut (UI chapter).
- Snap settings are **per user** (application preferences). An optional project default can be stored in RXD document `project/settings`.
- **Increment choice.** The increment used at a given moment is the smallest one in the list whose on-screen size is at least 10 pixels (configurable 4–40) at the current zoom, so snapping becomes finer as the user zooms in.
- **Priority.** When several snaps are within the pick tolerance (default 8 pixels), the order is: forced override, endpoint, intersection, centre, midpoint, perpendicular, tangent, quadrant, point, nearest, work-plane grid. The candidate glyph and a tooltip name the snap; Tab cycles among candidates (UI chapter).
- **Overrides.** A two-letter shortcut (the list is owned by the UI chapter; for example SE endpoint, SM midpoint, SI intersection, SC centre, SN nearest, SP perpendicular, ST tangent, SQ quadrant, SX point, SW work-plane grid, SR remote, SO snaps off for the next pick, SS turn override off) applies to the **next pick only**, even if that snap is switched off.
- **Validation.** Increment lists accept units (§2.9.5); entries must be positive, are de-duplicated and sorted descending on OK; an empty list disables that kind of dimension snap. Angular increments must divide 360° into a whole number of steps or are refused.
- **Project default.** When a project carries a snaps default and the user has never changed their own settings, the project default is used; otherwise the user's settings win. Opening a project never silently overwrites a user's settings.
- Snapping is computed by the view's pick service from the visible geometry cache, so remote snaps and point-cloud snaps are limited to what is displayed (and to loaded point-cloud tiles).

**Commands.** `snaps.get() -> settings` · `snaps.set(settings)` (settings = `{enabled, length_increments[], angle_increments[], object_snaps: {name: bool}, remote, point_clouds, pixel_threshold?, scope?: "user"|"project_default"}`; neither scope creates an undo item: the user scope is a preference, and the project default is written to `project/settings` and marks the project modified)

**Acceptance criteria.**
- [ ] Turning off "Midpoints" stops midpoint snapping in all views. The per-snap shortcut still forces it for the next pick.
- [ ] With length increments `1000; 100; 10` mm, drawing a wall at a zoom where 100 mm is 15 pixels snaps lengths to 100 mm; zooming in until 10 mm is 12 pixels snaps to 10 mm.
- [ ] Entering an angular increment of 7° is refused with a message; entering `"2 ft; 6 in"` as length increments is stored as 0.6096 m and 0.1524 m.

### 2.10.9 Transfer project standards
**ID:** DM-XFER-001 · **Priority:** P1 · **Ref:** research/05 §9.6

**Purpose.** To copy settings and system-family types from one open project (or template) to another.

**Workflow.**
1. Open the source and the target projects. Make the target active.
2. Manage › Settings › **Transfer Project Standards**. Choose the source project.
3. Tick the item kinds to copy. Each kind expands to show **individual items**, so selection can be granular.
4. **OK**. If items with the same name exist, choose **Overwrite**, **New only** or **Cancel**. A summary lists what was copied, renamed or skipped.

**Behaviour & rules.**
- Transferable kinds include at least: wall, floor, roof, ceiling, stair, railing, ramp, curtain-wall, mullion and panel types; dimension and text types; arrowheads; fill patterns; line patterns; line styles; line weights; materials (with assets); object styles; project information; project units; parameter definitions and bindings; global parameters; view templates; view filters; phase settings and phase filters; print and export setups; keynote and classification settings; colour fill schemes; MEP settings (pipe and duct types, segments, sizes, fluids, system types, electrical settings, load classifications); structural settings; title-block and annotation type settings.
- Dependencies are copied too (for example a wall type brings its materials and their fill patterns). Dependencies that already exist are matched by GUID first, then by name.
- Overwrite keeps the target's GUIDs, and only replaces values, so references in the target stay valid.

**Commands.** `standards.list(source_document_id) -> {kind, items[]}[]` · `standards.transfer(source_document_id, selection[{kind, item_ids?}], on_conflict: overwrite|new_only|rename) -> transfer_report`

**Acceptance criteria.**
- [ ] Transferring one wall type copies it with its 3 materials and 2 fill patterns, which the target did not have, and the report lists all 6 items.
- [ ] Transferring with *Overwrite* a material that exists in the target keeps the target material's GUID and changes its values.

### 2.10.10 Classification systems and keynotes
**ID:** DM-CLS-001 · **Priority:** P1 · **Ref:** research/05 §4.2 (classification code), §11.1 (keynote and classification files), §9.4

**Behaviour & rules.**
- **Classification systems** (for example Uniclass 2015, OmniClass, MasterFormat, Uniformat II, CCI, or national systems) are loaded from files into the project. They are stored in IFC as `IfcClassification` (Source, Edition, Name, Location), with the referenced codes as `IfcClassificationReference` (Identification, Name, ReferencedSource for hierarchy). They are associated with types or occurrences through `IfcRelAssociatesClassification`.
- Reviera reads classification tables from CSV (`code,parent_code,title`) and from IFC files that contain `IfcClassification` trees, and from the buildingSMART Data Dictionary (bSDD) API when online (P2).
- The *Classification Code* / *Assembly Code* fields in Properties open a searchable tree picker for the active system. More than one system may be used at once (one code per system per element).
- **Keynotes** are tab-separated text files (`key`, `text`, `parent_key`). The path is relative to the project, or absolute. Keynote tags (DOC) read them. Keynote values on elements and materials are parameters (§2.9.2).

**Commands.** `classification.load(path_or_url, name?) -> system_id` · `classification.assign(element_ids[], system_id, code)` · `classification.unassign(element_ids[], system_id)` · `keynotes.load(path)` · `keynotes.reload()`

**Acceptance criteria.**
- [ ] Assigning Uniclass `Ss_25_10_30` to a wall type writes an `IfcClassificationReference` with Identification `Ss_25_10_30`, linked to an `IfcClassification` named "Uniclass 2015".

### 2.10.11 Purge unused
**ID:** DM-PURGE-001 · **Priority:** P0 · **Ref:** research/05 §9.7

**Purpose.** To remove unused content and keep projects lean.

**UI.** Manage › Settings › **Purge Unused**. A tree grouped by kind (families › types, materials, assets, groups, view templates, filters, line patterns, fill patterns, line styles, text and dimension types, unplaced views (optional, off by default), parameter definitions without values, classification systems, RXD orphans), with *Check all*, *Check none*, *Expand/Collapse* and a count per group. **OK** purges the checked items.

**Behaviour & rules.**
- "Unused" means: no element, type, view, template, setting or formula refers to the item (computed from the dependency graph and RXD references).
- Purge runs **multiple passes** automatically: removing a family can make its nested families and materials unused. The dialog shows the final result of all passes. It offers a *Show items freed by this selection* preview.
- Items owned by another user (BIMserver advisory locks; COL) cannot be purged and are shown greyed out.
- Built-in and system items (built-in line styles, the default material, built-in subcategories) are never offered.
- Purge is one transaction, so it can be undone. The option "compact undo after purge" clears the undo stack to free memory.

**Commands.** `purge.list(include_unplaced_views=false) -> {kind, items[]}[]` · `purge.run(item_ids[], compact_undo=false) -> report`

**Acceptance criteria.**
- [ ] In a project with one loaded but unused family that nests two others and uses three materials, a single purge removes all six items, and undo brings them all back.

### 2.10.12 Phases and design options (settings)
**ID:** DM-PHASE-001 · **Priority:** P1 · **Ref:** research/05 §2.4

**Purpose.** Define the project's construction sequence, how each phase status is drawn, and which statuses each view shows.

**Workflow.**
1. Manage › Phasing › **Phases** (`phase.manage()`). The dialog opens on *Project Phases*.
2. Add phases with **Insert Before** / **Insert After** on the selected row; rename in place; give each a description.
3. On *Phase Filters*, create filters and set each status to *By category*, *Overridden* or *Not displayed*.
4. On *Graphic Overrides*, set the look of each status.
5. **OK** applies all changes as one transaction ("Phases"); **Cancel** discards them.

**Behaviour & rules.**
- **Manage › Phasing › Phases** dialog, with three tabs:
  - **Project Phases**: an ordered list (earliest first) with columns Number, Name, Description and a *Current* marker (**Set as current** chooses the project's current phase, which drives the IFC Status of §2.3.11; the default is the last phase); insert before or after, combine with previous or next, rename;
  - **Phase Filters**: New, Existing, Demolished and Temporary states, each set to *By category*, *Overridden* or *Not displayed*;
  - **Graphic Overrides**: per phase status, projection and cut line style, fill pattern, halftone, material.
- Defaults: phases "Existing" and "New Construction"; phase filters "Show All", "Show Complete", "Show Demo + New", "Show New", "Show Previous + Demo", "Show Previous + New", "Show Previous Phase".
- **Element status in a view** is derived from the view's phase: *New* (created in the view's phase), *Existing* (created earlier, not demolished), *Demolished* (demolished in the view's phase), *Temporary* (created and demolished in the same phase). Elements created in a later phase are not shown.
- Combining phases moves elements of the removed phase into the remaining phase. Views, schedules and rooms whose phase was removed move to the remaining phase too. An element whose created and demolished phases become the same becomes *Temporary*, and the dialog reports how many.
- **Validation.** Phase and filter names are unique and non-empty. At least one phase must exist; the last phase cannot be combined away. A phase cannot be deleted directly, only combined, so no element is ever left without a phase. The four built-in statuses cannot be renamed. Built-in phase filters can be edited but not deleted; a user filter used by views lists those views before it can be deleted, and they switch to "Show All".
- **New elements** take the active view's phase as *Phase Created*; demolishing (ARC's Demolish tool) sets *Phase Demolished* to the view's phase. `element.set_phase` refuses a demolished phase earlier than the created phase.
- **IFC.** Phases are not IFC entities; on save each element's status relative to the project's **current phase** is written to `Pset_*Common.Status` (`NEW`, `EXISTING`, `DEMOLISH`, `TEMPORARY`) and exchange exports use the phase chosen in the setup (§2.3.11, §2.11.4).
- Design options are managed in the ARC chapter. Their storage is §2.3.11.

**Data (storage).** RXD document `project/phases`: kinds `phase` (name, description, order, GUID), `phase.filter` (per-status presentation) and `phase.override` (per-status graphics). Element membership is RXD element data (`membership`/`phasing`).

**Commands.** `phase.create(name, after_phase_id?) -> phase_id` · `phase.rename(phase_id, name)` · `phase.combine(phase_id, with: "previous"|"next")` · `phase_filter.create(name, states) -> id` · `phase_filter.edit(id, states)` · `phase.graphic_overrides.set(status, overrides)` · `element.set_phase(element_ids[], created?, demolished?)` · `phase.manage()` (opens the Phases dialog)

**Acceptance criteria.**
- [ ] Combining "Phase 2" with "Phase 1" moves every element created in Phase 2 into Phase 1, and views whose phase was Phase 2 change to Phase 1.
- [ ] Trying to combine away the only remaining phase is refused; renaming a phase to an existing phase name is refused.
- [ ] A wall created in "Existing" and demolished in "New Construction" exports with `Pset_WallCommon.Status = DEMOLISH`, and a view with phase "New Construction" and filter "Show Demo + New" draws it with the Demolished overrides.

### 2.10.13 Starting view and project templates
**ID:** DM-SET-005 (starting view) · DM-TMPL-001 (project templates) · **Priority:** P1 / P0 · **Ref:** research/05 §9.12, §11.1

**Behaviour & rules.**
- **Starting view**: the view opened when the project opens (default: the last view used). It is stored in RXD document `project/settings`.
- **Images** are managed as described in §2.10.14, and **decal types** in §2.10.15.
- **Project templates (DM-TMPL-001, P0).** A project template is a project marked as a template in its RXD manifest (`02a`). By convention it is named `<Name>.template.ifc`. *New Project* creates a copy with **new GUIDs** for everything (project, site, building, storeys, views, types, …), so projects made from the same template never share element GUIDs. Exception: definitions with intended global identity (shared parameter GUIDs, family GUIDs, material RXD GUIDs) keep their GUIDs.
- Reviera ships these templates, authored from scratch: *Architectural (metric)*, *Architectural (imperial)*, *Multi-discipline (metric)*, *Multi-discipline (imperial)*, *Construction*, and *Empty*.

**Commands.** `project.new(template_path?) -> document_id` · `project.save_as_template(path)` · `settings.starting_view.set(view_id | "last")`

**Acceptance criteria.**
- [ ] Two projects created from one template have no `GlobalId` in common for spatial elements, views or system types, but share family GUIDs and shared parameter GUIDs.

### 2.10.14 Image management
**ID:** DM-IMG-001 · **Priority:** P1 · **Ref:** research/05 §9.12; web: Manage Images Dialog; spec/06 COL-LNK-005

**Purpose.** One place to see, add, replace and remove every raster image and PDF page image the project holds, whatever uses it: title blocks, drafting views and sheets, schedules (image parameters), materials (textures), decals and family image parameters.

**Workflow.**
1. Insert › Import › **Manage Images**, or Manage › Manage Project › **Manage Images** (`image.manage()`).
2. Select an image to see where it is used. Add, reload, replace, place or delete as needed.
3. Each action runs immediately as its own transaction; **Close** closes the dialog.

**UI.** A table with columns **Preview** (thumbnail), **Name** (file name with extension), **Kind** (imported, linked, rendering, texture), **Pixel size**, **File size**, **Count** (placements plus parameter and material uses) and **Path** (source path; "in project" for images with no external source, such as saved renderings). A **Path type** switch (*Relative to project* / *Absolute*) controls how source paths of linked images are stored. Buttons: **Add…**, **Reload**, **Reload From…**, **Place Instance**, **Show users** (lists each view, sheet, element, type or material using the image, with *Go to*), **Delete**. Sorting by any column; search by name.

**Behaviour & rules.**
- **Storage and de-duplication.** Imported images are RXD assets, de-duplicated by SHA-256 hash: adding the same file twice stores it once (COL-LNK-005). Linked images keep only their path and are re-read on open and on Reload.
- **Add** imports PNG, JPEG, TIFF (first page), BMP and WebP files, and single PDF pages rasterised at a chosen resolution (150–600 dpi, default 300). Images above 16,384 × 16,384 pixels are refused with `image.too_large` and a downscale offer.
- **Reload** re-reads the image from its recorded path; **Reload From** points it at another file and replaces the content. Every user keeps its size, position and crop. A missing source keeps the last embedded content (for imported images) or shows a "not found" placeholder (for linked images).
- **Place Instance** places the selected image in the active drafting view, legend or sheet, at its native size (from its dpi, 96 dpi if none), and starts the placement tool (`image.place`). Placement is one click (native size) or two clicks (opposite corners, with *Lock proportions* on by default); the tool declares its micro-operations (pick corner, pick opposite corner, toggle proportions) and a replay function, as every interactive tool must (Ch 2b §6.5, DM-LIVE-015).
- **Delete** of an unused image removes it. Deleting an image in use offers to delete its placements too, or to **replace** it with another image (`replace_with`), which keeps every placement's size and position and every parameter or material reference. An image parameter value can be cleared without deleting the image (the *None* choice in the image parameter's picker, §2.9.4).
- Changing **Path type** rewrites the stored paths of all linked images in one transaction; images whose path cannot be made relative (another drive) stay absolute and are listed.
- Purge offers unused images (§2.10.11).

**Data (storage).** Imported images: RXD assets with hash, original name, pixel size and dpi; placements are view- or sheet-owned records (`views/annotation/<viewId>`); linked images are kind `link.image` in `project/links` (Ch 6). Image parameters hold an asset reference (§2.9.4); material textures are written to IFC as described in DM-MAT-002.

**Commands.**
`image.manage()` (opens the dialog) ·
`image.list() -> image[]` ·
`image.add(path, page?, dpi?) -> image_id` ·
`image.reload(image_id, path?)` (no path: re-read the recorded source) ·
`image.place(image_id, view_id, position, width?) -> element_id` ·
`image.set_path_type(mode: "relative"|"absolute") -> report` ·
`image.delete(image_ids[], replace_with?) -> ()`

**Acceptance criteria.**
- [ ] Adding the same PNG twice creates one image row whose Count rises as it is placed; the IFC/RXD stores one asset.
- [ ] *Reload From* on a logo used in a title block and two drafting views changes the picture in all three without changing their sizes or positions.
- [ ] Deleting an image used as a material texture and in one sheet, with `replace_with` set, leaves the material and the sheet pointing at the replacement; undo restores the original.

### 2.10.15 Decal types
**ID:** DM-DECAL-001 · **Priority:** P2 · **Ref:** research/05 §9.12; web: Decal Type Properties; web: Decal Attributes

**Purpose.** Decals put images (signs, posters, screens, labels) on model faces for rendered and realistic views without modelling them. A decal type holds the image and its rendering attributes; placing it is a DOC tool.

**Workflow.**
1. Manage › Manage Project › **Decal Types**, or Insert › Decal › Decal Types (`decal.types()`).
2. **Create new decal** (enter a name), then choose an image source (an existing project image or a new file, §2.10.14) and set its attributes.
3. Choose **OK**. Place decals with Insert › Decal › **Place Decal** (`decal.place`, Chapter 5, DOC-OUT-011).

**UI.** Decal Types dialog: a list of decal types on the left (search, New, Duplicate, Rename, Delete); on the right the selected type's **attributes** with a live preview swatch rendered with the current attributes. The type's identity fields (Type Mark, Manufacturer, Model, Keynote, Description, Type Comments, URL, Cost, Assembly Code) are edited in its Type Properties (§2.2.4).

**Behaviour & rules.**
- **Attributes** of a decal type:

| Attribute | Range / values | Default |
|---|---|---|
| Image | A project image (PNG, JPEG, BMP; alpha is used as transparency) | — (required) |
| Brightness | 0–10, a multiplier on the image's luminance | 1.0 |
| Reflectivity | 0–1 | 0.1 |
| Transparency | 0–1 | 0 |
| Finish | Matte, Satin, Glossy, Polished (sets PBR roughness 0.9 / 0.5 / 0.2 / 0.05) | Matte |
| Luminance | 0–100,000 cd/m², with presets (none, dim screen, LCD screen, lit sign) | 0 |
| Bump pattern | None, or a project image used as a height map | None |
| Bump amount | 0–1 | 0.3 |
| Cut-outs | None, Rectangular, Oval, or an image whose alpha or luminance masks the decal | None |

- The attributes map onto the PBR appearance model of DM-MAT-002 (base colour texture, roughness, emissive, opacity, normal or bump), so decals render through the same pipeline as materials and export losslessly to glTF.
- Editing a type changes every placed decal of that type. Types are unique by name; a type with placements cannot be deleted until its decals are deleted, which the dialog offers.
- **Placing** (Chapter 5): `decal.place(view_id, decal_type_id, face_ref, position, width?, height?) -> element_id` puts a rectangle on a planar or cylindrical face; *Lock proportions* is on by default. Decals follow their host face when it moves and are deleted with it. They show as a placeholder outline in non-rendered visual styles and as the image in Realistic and rendered views.
- Decal types and placed decals can be copied between projects with Transfer Project Standards and copy/paste (§2.10.9).
- **IFC.** Decals are not building elements. They are not written to exchange exports; with *Export IFC with documentation* (`02a` §4) a decal may be written as an `IfcAnnotation` with a textured surface (P3).

**Data (storage).** Decal types: records in RXD document `standards/materials` (kind `decal.type`, proposed for `02a`), referencing an image asset. Placed decals are stored as Chapter 5 defines for `decal.place` (DOC-OUT-011): a record holding the type, the host GUID and a stable face reference (§2.8.3).

**Commands.** `decal.types()` (opens the dialog) · `decal.type.create(name, image_id, attributes?) -> type_id` · `decal.type.edit(type_id, attributes)` · plus `type.duplicate`, `type.rename` and `type.delete` (§2.2.4), which work on decal types.

**Acceptance criteria.**
- [ ] A decal type with a PNG with alpha, placed on a wall, renders with the transparent pixels showing the wall material; changing its Brightness to 2 brightens every placed decal of that type.
- [ ] Deleting a decal type with 3 placements is refused until the placements are deleted, and the dialog offers to delete them in the same transaction.
- [ ] A glTF export of a 3D view with a decal contains a textured quad on the wall face using the decal image.

### 2.10.16 Rule-based numbering
**ID:** DM-NUM-001 · **Priority:** P2 · **Ref:** research/05 §14 (2027: rule-based numbering); web: What's New in Revit 2027 (Symetri); web: Revit 2027 (BIM Pure)

**Purpose.** Number elements automatically and consistently from rules, such as "doors numbered per room as `<Room Number>-A`, `-B`", or "columns per level as `L2-C001`", instead of typing marks by hand or running scripts.

**Workflow.**
1. Manage › Numbering › **Rule-Based Numbering** (`numbering.rule_based()`). The dialog lists the project's numbering rules.
2. **New** (+): name the rule, for example "Doors by room".
3. **Elements**: choose one or more categories, then optionally a filter: an existing view filter, or rules built in place with the same rule editor as view filters (parameter · operator · value, nested AND/OR sets). Elements that match can also be excluded by a condition (for example *Comments equals "Exclude"*). The matching count is shown live.
4. **Target**: choose the parameter that receives the number. Default: *Mark* for instance rules, *Type Mark* for type rules. Any writable text or integer instance parameter (built-in, project or shared) that is bound to every chosen category may be used.
5. **Partition** (optional): choose one or more parameters, for example *Level*, or *To Room: Number*. Each distinct combination of values forms its own **sequence**, numbered independently. With no partition, all matching elements form one sequence.
6. **Matching** (optional): choose when elements count as "the same" and share one number: *None* (default), *Same type*, *Same values of* chosen parameters, or *Same type and geometry*.
7. **Format**: build the number from segments, with a live sample: literal text, the value of a parameter (from the element, its type, its level or room), and the **counter**. The counter is *digits* (minimum width with leading zeros), *upper-case letters* or *lower-case letters* (A…Z, AA…; with an optional skip list, default I and O). Set the **start** value (default 1 or A) and the **increment** (default 1); a start may be overridden per partition value.
8. **Order**: choose how elements in a sequence are ordered: *Creation order* (default), *By location* (left to right then top to bottom, or by distance along a picked path), or *Sorted by* up to three parameters.
9. Review the **preview** table (element, sequence, current value, proposed value, conflicts). Choose **Apply now** to number, and tick **Apply automatically** to keep numbering new and changed elements.
10. **OK** saves the rules; the changes and any numbering applied are one transaction ("Rule-Based Numbering").

**UI.** Left: the ordered list of rules (name, categories, target, enabled, auto) with New, Duplicate, Rename, Delete, Move Up/Down. Right: the rule editor in five sections (Elements, Target, Partition and Matching, Format, Order). Bottom: the preview table with filters (*Changes only*, *Conflicts only*) and a *Select in model* button. A rule with an error shows a red marker and the reason.

**Behaviour & rules.**
- **Sequences.** A sequence is identified by the rule and the partition values. An element with no value for a partition parameter goes into a "(none)" sequence of its own. Within a sequence, matched elements always get the same number and unmatched elements never share one.
- **Automatic mode.** Enabled rules with *Apply automatically* run as a built-in updater (§2.13.5) inside the transaction that adds elements, or that changes a partition, filter or matching parameter. New elements get the next free number of their sequence; an element that moves to another sequence (for example to another room) gets the next free number there. Changes are part of the user's undo item. Automatic mode never renumbers elements that keep their sequence, so numbers stay stable as the model grows.
- **Apply now** offers two modes: *Fill missing* (only elements with an empty target) and *Renumber all* (recomputes every element in the rule's scope by its order, closing gaps). *Renumber all* shows the number of changed values before applying.
- **Manual edits.** When a user edits the target value of a numbered element by hand, the element is marked *manually numbered* for that rule and is skipped by automatic mode; its value still counts as taken in its sequence. *Reset to rule* clears the mark.
- **Gaps.** Deleting an element leaves a gap; automatic mode does not reuse it (to keep issued numbers stable) unless the rule's *Reuse gaps* option is on.
- **Several rules.** Rules are evaluated in list order. If two enabled rules would write the same target parameter of the same element, the later one is skipped for that element and the dialog shows an overlap warning on both rules. Rules writing different targets may overlap freely.
- **Uniqueness check.** After numbering, duplicate values of the target within a sequence (possible after manual edits) raise a warning (§2.7.4) that names the elements.
- **Groups and linked models.** Elements inside group instances are numbered only when the target is bound with *values can vary by group instance* (§2.9.3); otherwise they are skipped and reported. Elements in linked models are never numbered.
- **BIMserver mode.** Numbers assigned locally may collide with numbers assigned by another user in the same sequence. After a sync, colliding elements in automatic rules are renumbered in order of creation time then GUID, and the sync report lists every changed value.
- **Validation.** A rule needs a name (unique), at least one category and a target parameter bound to every chosen category; the format must contain exactly one counter segment; the start must be valid for the counter kind; partition and order parameters must exist on the chosen categories. Invalid rules can be saved disabled but cannot be applied.

**Data (storage).** Rules: records of kind `numbering.rule` in RXD document `project/settings` (proposed for `02a`): name, order, enabled, auto, categories, filter (a `filter.rule` reference or an inline rule), target parameter, partition parameters, matching mode, format segments, start overrides per partition value, order mode, gap policy. Per-element state (the rule ID that last numbered it and the manual flag) is RXD element data `elements` (`numbering`). Numbers themselves are ordinary parameter values, so they reach IFC through the target parameter's IFC home (for *Mark*, `IfcElement.Tag`).

**Commands.**
`numbering.rule_based()` (opens the dialog) ·
`numbering.rule.create(def) -> rule_id` ·
`numbering.rule.edit(rule_id, changes)` ·
`numbering.rule.delete(rule_id)` ·
`numbering.rule.reorder(rule_ids[])` ·
`numbering.preview(rule_id | def, element_ids?) -> [{element_id, sequence, current, proposed, conflict?}]` ·
`numbering.apply(rule_id?, mode: "fill_missing"|"renumber_all", element_ids?) -> report` ·
`numbering.reset_manual(element_ids[], rule_id?)`

**Acceptance criteria.**
- [ ] A rule "Doors by room" (doors; target Mark; partition *To Room: Number*; format `<To Room: Number>-<letters A>`) numbers the three doors of room 101 as `101-A`, `101-B`, `101-C` and the door of room 102 as `102-A`.
- [ ] With *Apply automatically* on, placing a fourth door into room 101 sets its Mark to `101-D` in the same undo item as the placement; moving it into room 102 changes it to `102-B`.
- [ ] Two identical chairs of the same type, with matching *Same type and geometry*, get the same number; a third chair of another type gets the next number.
- [ ] A door whose Mark was typed by hand keeps it through *Fill missing* and automatic numbering, and *Reset to rule* brings it back under the rule.
- [ ] A rule whose format has no counter segment cannot be applied, and the dialog names the error.

---

## 2.11 File I/O

### 2.11.0 Creating new documents
**ID:** DM-FILE-006 · **Priority:** P0 (model, model template, family) / P1 (title block, annotation symbol, conceptual mass) · **Ref:** research/05 §4.1, §9.12, §11.1; spec/01 (File › New, Reviera Home)

**Purpose.** One entry point to start any kind of Reviera document from a template.

**Workflow.**
1. **File › New** (Ctrl+N), or **New…** on Reviera Home (`file.new(kind, template?, schema?)`). The **New** dialog opens for the chosen kind (Model by default).
2. For a model: choose a project template from the list (from the template folders in Options › File Locations, plus *Browse…* and *None*), choose **Create new: Model** or **Model template**, and, in Local mode, the IFC schema (IFC4, default, or IFC4X3).
3. For a family, title block, annotation symbol or conceptual mass: the family template chooser opens, filtered to that kind (§2.8.1).
4. **OK** creates an untitled document and opens its starting view. The document is saved only when the user saves.

**Behaviour & rules.**
- **Kinds.** `model` copies the chosen project template with new GUIDs (DM-TMPL-001; the same operation as `project.new`). `model_template` does the same, but the result is flagged as a template, so its first save defaults to `<Name>.template.ifc`. `family`, `title_block`, `annotation_symbol` and `mass_family` call `family_doc.new` with a template of that kind; `template` is then a family template path, and a template of the wrong kind is refused with `file.template_kind_mismatch`.
- **No template.** `template` omitted for a model uses the default template set in Options; *None* creates an empty project with one site, one building, one storey ("Level 1", elevation 0), SI units in IFC, the Metric (mm) display preset and the built-in standards (line weights, patterns, object styles). Families always need a template.
- **Schema.** `schema` applies to models only. IFC4X3 is offered only in Local mode and when the template's schema allows it; a template saved in IFC4 is converted on creation (§2.11.3) with the usual report. In BIMserver mode the schema is the server project's schema.
- **Untitled documents** get a live store at once (Ch 2b §2), so work is recoverable before the first save. The title bar shows "Untitled 1", "Untitled 2", … per session.
- **Missing or invalid templates** (not found, not a Reviera template, newer RXD major version) are reported in the dialog with the reason, and the user picks another; a template from an older RXD version is migrated in memory (§2.11.8).
- New documents take the project information defaults from the template, with Project Issue Date set to today and Author set to the user name from Options.

**Data (storage).** Nothing is written until the first save. The new project's `Reviera_Manifest` records the source template's name and DataId for traceability (`02a`).

**Commands.** `file.new(kind: model|model_template|family|title_block|annotation_symbol|mass_family, template?, schema?: IFC4|IFC4X3) -> document_id` (plus `project.new`, §2.10.13, and `family_doc.new`, §2.8.0, which it calls)

**Acceptance criteria.**
- [ ] `file.new(kind="model")` with no template and no default creates a project with one storey "Level 1" that saves as a valid IFC4 file.
- [ ] `file.new(kind="family", template="Door.familytemplate.ifc")` opens a family document with the door template's references and parameters; passing a title-block template with `kind="family"` fails with `file.template_kind_mismatch`.
- [ ] A new untitled model, edited and then killed before any save, is offered in the recovery dialog on restart.

### 2.11.1 Opening models
**ID:** DM-FILE-001 · **Priority:** P0 · **Ref:** research/05 §11.1, §11.2, §12.1; TECH_STACK §3.1, §3.7

**Purpose.** To open any IFC file, whether written by Reviera or by another tool, and make it editable.

**Workflow.**
1. **File › Open** (Ctrl+O), choose an `.ifc` or `.ifczip` file. Or choose a recent file on Reviera Home (UI chapter), or a BIMserver project (COL chapter).
2. A progress bar shows the stages: parse → build element store → RXD binding and checks (§2.4.3) → regeneration of view caches for the starting view.
3. If the file needs a schema upgrade, or has RXD warnings, a summary dialog lists them before editing begins.

**Behaviour & rules.**
- **Parsing** uses IfcOpenShell. The file is parsed on a worker thread. The UI stays responsive, and *Cancel* is available.
- **Geometry** is **not** recomputed from RXD on open. The IFC geometry is used as is (tessellated with OCCT, in parallel). Regeneration only runs for elements that are edited later, or flagged by reconciliation. This keeps open times proportional to file size.
- **Foreign elements.** Elements without RXD (from other tools) become **IFC-native elements**:
  - they can be selected, moved, copied, rotated, mirrored, deleted, scheduled and tagged;
  - their properties can be edited;
  - their geometry is fixed (treated like a direct shape).
  - **Recognise** (Modify › Recognise, P1) converts them to parametric Reviera elements where possible: walls with an `IfcMaterialLayerSetUsage` and an extruded or clipped body become basic walls; extruded slabs become floors with a sketch; storey-contained doors and windows in openings become placed instances of static families made from their types. Recognition never changes the GUIDs. It reports elements it could not convert.
- **Large models (P1).** *Partial open*: choose storeys, disciplines (by IFC class) or worksets to load; unloaded elements stay in the file untouched and are written back on save. **Incremental open (P2)**: show the model while geometry is still being tessellated, coarse first.
- **Read-only open** is available (for example, for files on read-only media). Save is disabled, and *Save As* is offered.
- **Files from a newer Reviera** follow §2.11.8.
- **IFC open options** (File › Open › *IFC open options…*; `file.ifc_open_options(options?) -> options`). A per-user dialog that sets the defaults used when opening IFC files; called without arguments it opens the dialog, with `options` it sets them and returns the result. Settings:
  - *Partial open*: always open everything (default), or always ask with the storey, IFC-class and workset chooser;
  - *Incremental open* (P2): on or off;
  - *Third-party files*: run **Recognise** automatically on open (off by default), and which classes to try (walls, slabs, doors and windows);
  - *Unit conversion report*: show, or only log;
  - *Tessellation quality* for third-party B-rep and swept geometry: coarse, normal (default), fine (chordal deviation 5 mm / 1 mm / 0.2 mm, angular 20° / 10° / 5°);
  - *Schema upgrade*: for IFC2X3 files, open as an upgraded copy (default) or read-only;
  - *Missing RXD sidecar*: look for it silently, or always ask (§2.4.3).
  Values are validated on OK (for example a chordal deviation must be between 0.05 mm and 50 mm). The options apply to the next open; the File › Open dialog shows the active options in a summary line with a link to change them for that one open.

**Commands.** `file.open(path, {partial?: {storeys?, classes?, worksets?}, read_only?}) -> document_id` · `file.close(document_id, save?: "ask"|"yes"|"no")` · `element.recognise(element_ids[]) -> report` · `file.ifc_open_options(options?) -> options`

**Acceptance criteria.**
- [ ] A 500 MB IFC4 file from another authoring tool opens in under 60 s on the reference machine (8 cores, 32 GB), and every element is selectable, schedulable and exportable.
- [ ] Recognise on 200 third-party walls with layer-set usages converts at least 95% to basic walls with the same layers and GUIDs, and reports the rest with a reason.
- [ ] Opening a file with partial load (one storey), editing it and saving keeps the unloaded storeys' entities byte-equivalent in a semantic diff.
- [ ] Setting *Partial open* to *always ask* in IFC open options makes the next File › Open show the storey chooser; the setting persists across application restarts.

### 2.11.2 Saving
**ID:** DM-FILE-002 · **Priority:** P0 · **Ref:** research/05 §11.1, §11.2; TECH_STACK §3.1

**Behaviour & rules.**
- **Save** (Ctrl+S) writes the project in its current schema to `.ifc` (STEP, ISO 10303-21) or `.ifczip` (a ZIP holding one `.ifc` file, as buildingSMART defines). RXD is written with it, in the carrier set for the project (embedded by default; `02a`).
- **Atomic write.** Saving writes to a temporary file in the same folder, flushes it to disk, then renames it over the original. A failure or crash during save never damages the original file.
- **Background save.** Saving serialises a **committed snapshot** on a worker thread. The user can keep working. Edits made during the save are included in the next save.
- **Deterministic output.** For an unchanged model, two saves produce identical files except the header timestamp. Entity order is stable: by kind (project and contexts, units, spatial structure, types, elements, relationships, resources), then by GlobalId (or by owner for non-rooted entities). STEP instance numbers are reassigned in that order. This makes `.ifc` files usable in version control and keeps diffs small.
- **Backups.** Local saves keep N previous versions (default 3), as `<name>.<nnnn>.ifc` in a `<name>_backup` folder next to the project. Each backup is a full copy (with its RXD).
- **Save As** can change the file name, the format (`.ifc` ↔ `.ifczip`) and the RXD carrier (embedded, sidecar, or both; `02a`). It cannot change the schema. Use **Convert Schema** for that (§2.11.3).
- **File locking.** In Local mode, Reviera creates a lock file (`.~lock.<name>#`) while a project is open, and warns if another user or process has the file open. It then offers read-only open.
- **Compression.** `.ifczip` uses DEFLATE (level 6 by default). STEP text for large models typically compresses by 80–90%.

**Commands.** `file.save(document_id? (default: active)) -> status` · `file.save_as(document_id? (default: active), path, kind?, format?: ifc|ifczip, carrier?: embedded|external_file|external_dir|bundle) -> status` · `file.backups(document_id) -> backup[]` · `file.restore_backup(document_id, backup_id)`

**Acceptance criteria.**
- [ ] Killing the process at a random point during save (tested 100 times) never leaves a corrupt or truncated project file. The original or the new file is always intact.
- [ ] Saving the same unchanged model twice gives byte-identical files except the `FILE_NAME` timestamp.
- [ ] Editing during a background save of a 300 MB model is possible, and the next save includes the edit.

### 2.11.3 IFC schema policy and conversion
**ID:** DM-FILE-003 · **Priority:** P0 (IFC4 read/write, IFC2X3 read) / P1 (IFC4X3 projects, IFC2X3 export) · **Ref:** research/05 §11.2, §12.1, §12.2; TECH_STACK §3.1, §3.7.3

**Behaviour & rules.**

| Schema | Open | Native project schema | Export |
|---|---|---|---|
| IFC2X3 TC1 | ✔ (upgraded in memory to IFC4) | ✘ | ✔ (Coordination View 2.0) |
| IFC4 ADD2 TC1 | ✔ | ✔ **default** | ✔ (Reference View, Design Transfer View) |
| IFC4X3 ADD2 | ✔ | ✔ (Local mode; BIMserver mode only if the server supports it) | ✔ (Reference View) |
| IFC-XML / IFC-JSON (`.ifcxml`, `.ifcjson`) | P3 | ✘ | P3 |
| IFC5 (alpha) | Watch list | ✘ | ✘ |

- **Convert Schema** (File › Convert Schema) upgrades IFC4 → IFC4X3 or downgrades IFC4X3 → IFC4. Both directions keep GUIDs. Entities that have no counterpart in the target schema (for example IFC4X3 infrastructure classes in IFC4) are converted to `IfcBuildingElementProxy` with `ObjectType` set to the source class name. The original class is kept in RXD element data, so a later upgrade can restore it. A report lists every conversion.
- **IFC2X3 open** upgrades with the same mapping as IfcOpenShell's schema-migration tooling (for example `IfcWallStandardCase` → `IfcWall`; type `ApplicableOccurrence` and owner history handled as IFC4 requires; property names updated where buildingSMART renamed them). Elements and properties that cannot be mapped are kept as proxies and listed in the report.
- **Units on open.** Files in non-SI units (for example millimetres or feet) are converted to SI on open. Saving writes SI (§2.3.1). The conversion is reported once.

**Commands.** `file.convert_schema(document_id, target: "IFC4"|"IFC4X3_ADD2") -> report`

**Acceptance criteria.**
- [ ] An IFC2X3 sample set (at least 20 files from public buildingSMART and open-source test suites) opens with no crash, and with all `IfcProduct` entities present after upgrade.
- [ ] IFC4 → IFC4X3 → IFC4 keeps every GUID, and every property value.

### 2.11.4 IFC export setups (exchange exports)
**ID:** DM-FILE-004 · **Priority:** P0 (basic Reference View export) / P1 (setups, property mapping) / P2 (IDS validation) · **Ref:** research/05 §12.1, §12.2, §12.3; screenshots/platform/ifc_property_mapping_2027.png

**Purpose.** The native file is already IFC. **Export** produces a *derived* IFC file for a specific exchange (a Model View Definition, a client requirement, a coordination platform), leaving the native file unchanged.

**UI.** File › Export › **IFC**. The dialog has a named **setup** drop-down (New, Duplicate, Rename, Delete, Import/Export setup as JSON) and these tabs:
- **General**:
  - schema and MVD: IFC4 Reference View; IFC4 Design Transfer View; IFC2X3 Coordination View 2.0; IFC4X3 Reference View;
  - phase to export;
  - design options (primary only, or all);
  - space boundaries (none, first level, second level);
  - coordinate base (internal origin, project base point, survey point, shared site);
  - split walls, columns and ducts by storey;
  - file format (`.ifc`, `.ifczip`).
- **Content**:
  - export only elements visible in a chosen view;
  - include RXD (off for exchange exports; it makes a "native" copy when on);
  - export annotation as `IfcAnnotation` (P2);
  - export rooms, areas and zones;
  - base quantities on or off;
  - material layer, profile and constituent sets on or off;
  - classification systems to include;
  - export parameter templates (`IfcPropertySetTemplate`) for user parameters;
  - level of detail (Coarse, Medium, Fine) for the `Body`;
  - Reviera-only categories (masses, reference planes): exclude by default.
- **Property Mapping** (P1):
  - a tree of IFC classes on the left, with a filter;
  - for the selected class, a grid of property sets and properties: include on/off; target property set and property name; data type; source (a Reviera parameter, a formula over parameters, or a constant); and *reset to default* per row;
  - pre-defined groups: *IFC common property sets*, *Base quantities*, *User parameters*;
  - mapping profiles can be saved per setup, and exported or imported as JSON.
  - This takes the place of a mapping text file. Ref: screenshots/platform/ifc_property_mapping_2027.png.
- **Validation (P2):**
  - validate the result against IFC schema and normative rules (IfcOpenShell validator);
  - validate against an **IDS** file (buildingSMART Information Delivery Specification 1.0), with a report linking failing elements back to the model.

**Behaviour & rules.**
- Exports run on a committed snapshot in the background, with progress and cancel.
- An export **never** changes GUIDs. The same element has the same GlobalId in the native file and in every export. Elements that are split by storey get derived GUIDs, as UUID v5 of (element GUID, storey GUID), and are aggregated under an `IfcElementAssembly` that carries the original GUID.
- Setups are stored in RXD document `project/settings` (`ifc_export_setups[]`). Shipped default setups: *IFC4 Reference View*, *IFC4 Design Transfer View*, *IFC2X3 Coordination View 2.0*, *IFC4X3 Reference View*.

**Commands.** `export.ifc(path, setup? (name, id or inline settings), settings_override?, schema?, strip_rxd?=false, documentation?, background?) -> job_id` · `export.ifc_setup.save(setup)` · `export.ifc_setup.delete(name)` · `export.ifc_setup.list() -> setup[]` · `validate.ifc(path?) -> report` · `validate.ids(ids_path, scope?) -> report`

**Acceptance criteria.**
- [ ] An IFC4 Reference View export of the sample project passes the buildingSMART validation service with no errors.
- [ ] A property mapping that sends parameter "Acoustic Class" to `Pset_DoorCommon.AcousticRating` for doors produces that property in the export, and leaves the native file unchanged.
- [ ] Every GUID in the export also exists in the native file (except split-element GUIDs, which are derived deterministically).
- [ ] An IDS requiring `Pset_WallCommon.FireRating` on all external walls reports the 3 walls with no value, and each report row selects the wall in the model.

### 2.11.5 Import and link formats
**ID:** DM-IMP-001 … 012 · **Priority:** see table · **Ref:** research/05 §12.1, §12.3

**Behaviour & rules.**
- **Insert › Import** brings content into the model, as an import element in a view (2D), or as model geometry (3D). **Insert › Link** keeps a reference to the external file, reloaded on open or on demand (COL chapter for links of IFC and Reviera models).
- Common options for CAD imports:
  - positioning: auto centre-to-centre, auto origin-to-internal-origin, by shared coordinates, or manual;
  - units: auto-detect, or a chosen unit;
  - current view only (2D);
  - layer selection: all, visible, or chosen;
  - colours: preserve, invert, or black and white;
  - correct lines slightly off axis.
- Imported CAD becomes an **import instance** with an *Imported Objects* subcategory per layer (§2.10.3). Import instances can be exploded (partially or fully) into detail lines, text and filled regions, or into model lines. They are stored as RXD assets (the source file, or a converted cache) plus an RXD document `views/annotation/<viewId>` record (2D) or an `IfcBuildingElementProxy` with tessellated geometry (3D, `ObjectType` = `ImportedGeometry`).
- **Open libraries only.** Every format is implemented with GPL-3.0-compatible libraries. Formats that need proprietary SDKs are P3, with an open alternative noted.

| ID | Format | Import | Link | Priority | Library (licence) | Notes |
|---|---|---|---|---|---|---|
| DM-IMP-001 | IFC (2X3, 4, 4X3) | ✔ (open, or merge into the current model) | ✔ | P0 (open) / P1 (merge, link) | IfcOpenShell (LGPL-3.0) | Merge keeps GUIDs. Clashing GUIDs get new ones, with a report. Merge options: target storeys (by name match, by elevation within 1 mm, or a chosen storey), IFC classes to include, and whether to bring property sets for unknown definitions. Merged elements without RXD are IFC-native (§2.11.1) until recognised. Linking an IFC keeps it read-only and reloads it on change (COL). The merge is one transaction. |
| DM-IMP-002 | DXF (R12–2018) | ✔ | ✔ | P1 | LibreDWG (GPL-3.0+) DXF reader, or a Rust DXF crate (MIT) | Layers, blocks (as nested import groups), text, hatches, dimensions (as exploded geometry). Units come from `$INSUNITS`, or the chosen unit when it is missing. Splines are kept as NURBS, arcs and ellipses as exact curves; proxy and unsupported entities are listed in the import report. Layer visibility and colour can be overridden per view (DOC). |
| DM-IMP-003 | DWG (R13–2018 and later where the library supports it) | ✔ | ✔ | P1 | LibreDWG (GPL-3.0+) | Coverage depends on LibreDWG. Unsupported entities are listed in a report. Same options and results as DXF. Xrefs are resolved from the DWG's folder and imported as nested groups, or skipped with a warning. Files saved by versions newer than the library supports are refused with the version found and a suggestion to save as DXF from the source tool. |
| DM-IMP-004 | PDF (as underlay) | ✔ | ✔ | P1 | PDFium (BSD-3/Apache-2.0) | Choose a page, resolution, and vector snapping to the PDF's path geometry. Pages are rendered at 72–600 dpi (default 150) for display; vector paths are extracted once into a snapping cache. Scale is set by the page's declared size or calibrated by two picked points of known distance. Password-protected PDFs are refused. Placed in plan, drafting and sheet views. |
| DM-IMP-005 | Images (PNG, JPEG, TIFF, BMP, WebP) | ✔ | ✔ | P1 | `image` crate (MIT/Apache-2.0) | Placed in views, or on sheets. Images are RXD assets de-duplicated by hash, or linked by path (§2.10.14, COL-LNK-005). Native size comes from the image's dpi (96 dpi if none); maximum 16,384 × 16,384 pixels. Draw layer background or foreground; scale calibration by two points; alpha transparency kept. |
| DM-IMP-006 | OBJ, STL, PLY | ✔ (project or family) | — | P1 | Rust crates (MIT/Apache-2.0) | Mesh. Optional decimation, and conversion to `IfcTriangulatedFaceSet`. Options: units (auto from file, or chosen), up-axis (Y or Z), merge coplanar triangles, decimate to a target triangle count, and category. Into a family, the mesh becomes a non-parametric form; into a project, a direct shape (§2.13.10). Non-manifold meshes are accepted for display and flagged. |
| DM-IMP-007 | glTF 2.0 / GLB | ✔ (project or family) | — | P2 | `gltf` crate (MIT/Apache-2.0) | Mesh with PBR materials (mapped to §2.10.5). Each glTF material becomes a Reviera material (matched by name to existing ones, or created) with its textures as RXD assets. Node hierarchy becomes nested groups or one element per node (option). Animations and cameras are ignored with a note. |
| DM-IMP-008 | STEP AP203/AP214/AP242, IGES, BREP | ✔ (project or family) | — | P1 | OCCT (LGPL-2.1 + exception) | Exact B-rep, used for manufacturer content in families. Solids keep exact faces (`IfcAdvancedBrep` on export); assemblies can be imported as one element or one element per part. Units come from the file. Colour and layer attributes (AP214/AP242) set materials and subcategories. Healing (sewing, small-edge removal) runs with a report. |
| DM-IMP-009 | 3DM (Rhino) | ✔ | — | P2 | openNURBS (licence to be checked) | Only if the licence check passes; otherwise P3. Imports breps, extrusions and meshes with layer names as subcategories; NURBS surfaces kept exact. Block instances become nested groups. Units from the file. |
| DM-IMP-010 | Point clouds: E57, LAS/LAZ, PLY | — | ✔ | P2 | libE57Format (BSL-1.0), `las`/`laz` crates (MIT/Apache-2.0) | Indexed into an octree cache stored on disk next to the project (not part of IFC or RXD; rebuilt if missing). The link itself is in RXD document `project/links`. Snapping to points. Indexing runs in the background with progress; display uses level-of-detail streaming within a point budget (default 5 million points). Colour by RGB, intensity, elevation or classification. Clipped by section boxes and view ranges. |
| DM-IMP-011 | BCF 2.1 / 3.0 (`.bcfzip`) | ✔ | — | P1 | Built in (XML, ZIP) | Topics, viewpoints and comments, linked to elements by IFC GUID (COL chapter). Viewpoints restore camera, section planes, visibility and selection. Components with GUIDs not found in the model are listed. Re-importing the same file updates topics by GUID instead of duplicating them. |
| DM-IMP-012 | gbXML | ✔ (as masses or spaces) | — | P3 | — | The ANA chapter covers export. Import would create masses or spaces from `Campus/Building/Space` surfaces; it stays P3 because round-tripping analytical geometry rarely helps authoring. |
| — | SKP, DGN (V8), SAT, FBX, NWC/NWD, RVT/RFA, ADSK, AXM | — | — | **P3** | No open library with acceptable licence and coverage | Open alternative: export to IFC, DXF, glTF or STEP from the source tool. DGN V7 might be reachable through GDAL/OGR (MIT), which is a P3 candidate. |

- **Reference-product models.** There is no open reader for the reference product's native files. The supported migration path is: export IFC4 from the reference product (its exporter is open source); import the shared-parameter file (§2.9.7); import lookup tables and type catalogs (§2.8.8, §2.8.9); then use *Recognise* (§2.11.1). The migration guide shall document recommended exporter settings.

**Commands.**
`import.cad(path, view_id?, options) -> import_id` ·
`link.cad(path, view_id?, options) -> element_id` ·
`import.explode(import_id, mode: partial|full) -> element_ids[]` ·
`import.pdf(path, page, view_id, options) -> element_id` ·
`import.image(path, view_id, placement) -> element_id` ·
`import.mesh(path, target: "project"|"family", category?, options) -> element_id` ·
`import.step(path, target: "project"|"family", category?) -> element_id` ·
`link.point_cloud(path, placement) -> element_id` ·
`import.bcf(path) -> topic_id[]` ·
`import.ifc_merge(path, options) -> report`

**Acceptance criteria.**
- [ ] A DXF R2018 file with 50 layers, blocks and hatches imports into a plan view. Each layer appears as an Imported Objects subcategory, and Explode produces detail lines on matching line styles.
- [ ] A STEP AP214 file of a manufacturer's product imports into a Generic Model family as an exact B-rep, and exports from a project as `IfcAdvancedBrep`.
- [ ] A vector PDF underlay offers snapping to its line endpoints.

### 2.11.6 Export formats
**ID:** DM-EXP-001 … 012 · **Priority:** see table · **Ref:** research/05 §12.2

| ID | Format | Source | Priority | Library (licence) | Notes |
|---|---|---|---|---|---|
| DM-EXP-001 | PDF (vector) | Sheets and views | P0 | Defined in DOC (own vector pipeline; for example `pdf-writer`, MIT/Apache-2.0) | Combined or separate files, naming rules, print order, raster fallback for shaded views. Fonts are embedded as subsets; text stays selectable and searchable. Sheet size, orientation and margins come from the title block; line weights from the line weight table at the view scale (§2.10.4). PDF/A-2b output is an option. Details in DOC. |
| DM-EXP-002 | IFC (exchange) | Model | P0/P1 | IfcOpenShell | §2.11.4. Exchange exports never change the native file. Clean IFC (`strip_rxd`) and IFC with documentation are options of the same command (`02a` §4). Setups, property mapping and IDS validation as in §2.11.4. |
| DM-EXP-003 | DXF | Views and sheets (2D), 3D views (3D) | P1 | Own writer, or LibreDWG | Layer mapping by category and subcategory, using layer standard tables (for example ISO 13567, AIA, BS 1192). Text, hatches, dimensions as entities or exploded. Versions R2000–R2018 selectable. Sheets export with views as model-space xrefs or flattened into paper space (option). Colours by layer or by entity; line patterns as linetypes. Fill patterns become hatches using the `.pat` definitions (§2.10.4). |
| DM-EXP-004 | DWG | Views and sheets | P2 | LibreDWG (GPL-3.0+) | DWG output depends on LibreDWG write support (check version coverage). DXF is the recommended fallback. Same setups, layer mapping and naming as DXF. If the chosen version cannot be written, the job fails for that file with a message suggesting DXF, and the rest of the batch continues. |
| DM-EXP-005 | Images (PNG, JPEG, TIFF) | Views and sheets | P1 | `image` crate | Resolution, size, visual style, crop. Size by pixel width or height, or by dpi at the sheet's paper size; maximum 16,384 pixels per side. Background white, transparent (PNG) or as shown. Each view or sheet becomes one file named by the naming rule. |
| DM-EXP-006 | glTF 2.0 / GLB | 3D views | P1 | `gltf` crate | PBR materials (§2.10.5), element GUIDs and selected parameters in `extras`, one node per element, instancing for repeated types. Options: binary GLB or glTF with separate files, Draco mesh compression (on or off), texture size limit, which parameters to include, and coordinate base (internal origin, survey point). Only elements visible in the view are exported. |
| DM-EXP-007 | OBJ (+MTL) | 3D views | P1 | Own writer | One object per element (named by GUID), MTL materials with diffuse colour and base texture; units in metres; Y-up or Z-up option. Hidden elements in the view are skipped. |
| DM-EXP-008 | STL | Selection, or 3D view | P2 | Own writer | For 3D printing. Watertight check. Binary or ASCII STL; scale factor and units chosen; a report lists open edges and self-intersections per element before writing. |
| DM-EXP-009 | gbXML | Spaces and analytical model | P2 | Own writer | Defined in the ANA chapter. Written from rooms or spaces and the energy analytical model, with constructions and openings. |
| DM-EXP-010 | BCF 2.1 / 3.0 | Issues | P1 | Built in | COL chapter. Writes the selected topics with viewpoints (camera, section planes, visible and selected components by IFC GUID) and comments. |
| DM-EXP-011 | CSV / TSV / XLSX | Schedules | P0 (CSV) / P1 (XLSX) | `rust_xlsxwriter` (MIT/Apache-2.0) | DOC chapter. Exports the schedule as shown: headers, grouping rows and totals optional; values formatted (display units) or raw (SI). XLSX keeps column widths and number formats. |
| DM-EXP-012 | SQLite parameter database | Model parameters | P2 | `rusqlite` (MIT) | Open alternative to ODBC export: one table per category with every parameter, keyed by GUID. Re-import of edited values (P2) by GUID. Extra tables hold types, levels, rooms and the parameter definitions (data type, unit); values are stored in SI with a unit column. Re-import updates only writable parameters, as one transaction with a change report, and ignores rows whose GUID is not in the model. |
| — | STEP AP242 | Selection | P2 | OCCT | Exact solids for fabrication |
| — | SVG | Views and sheets | P2 | Own writer | |
| — | USD | 3D views | P3 | OpenUSD (licence compatibility to be checked) | Open alternative: glTF |
| — | DWF/DWFx, NWC, FBX, ADSK | — | P3 | Proprietary | Open alternatives: PDF, IFC, glTF |

- **Bulk export (DM-EXP-013, P1).** File › Export › **Batch Export**: choose sheets and views, and one or more formats (PDF, DXF/DWG, images, IFC, glTF). Each output has a naming rule built from parameters (for example `{Sheet Number}-{Sheet Name}-{Current Revision}`). Batch export runs in the background, with a report. Batch-export sets are saved in RXD document `project/settings`.

**Commands.**
`export.pdf(range, set_id?, setup_id?, path_or_folder, combine?, naming_rule?, background?) -> job_id` (DOC chapter defines options) ·
`export.dxf(range, set_id?, setup_id?, folder, naming?, version?, xrefs?) -> job_id` · `export.dwg(range, set_id?, setup_id?, folder, naming?, version?, xrefs?) -> job_id` ·
`export.image(range, set_id?, path_or_folder, format, size, dpi?, options?) -> job_id` ·
`export.gltf(path, view_id?, binary?, options?) -> job_id` · `export.obj(path, view_id?, options?) -> job_id` · `export.stl(element_ids[], path)` ·
`export.sqlite(path, categories?[])` ·
`export.batch(set_name | set) -> report`

**Acceptance criteria.**
- [ ] A glTF export of a 3D view opens in at least two open-source glTF viewers with correct materials, and each node's `extras.guid` equals the element's IFC GlobalId.
- [ ] A DXF export of a plan opens in an open-source CAD application with one layer per category/subcategory, following the chosen layer standard.
- [ ] A batch export of 40 sheets to PDF and DXF with a naming rule produces 80 files, named as the rule says.

### 2.11.7 Headless and command-line operation
**ID:** DM-FILE-005 · **Priority:** P1 · **Ref:** research/05 §13.3 (cloud batch processing), §11.2 (batch upgrade)

**Purpose.** An open, local equivalent of cloud batch processing: run Reviera without a UI for upgrades, exports, checks and configurators.

**Behaviour & rules.**
- `reviera --headless [--script <file.lua>] [--command <name> --args <json>] [--trust <level>] <model.ifc> …` runs commands or Lua scripts against one or more models, then exits. The exit code is 0 for success and non-zero for failure, and a JSON report goes to stdout or to a file.
- `reviera --mcp-stdio [model.ifc]` starts the MCP server on stdio without a UI (§2.16.2).
- Headless mode uses the same command registry and transactions. Commands that need user interaction (picks, dialogs) fail with `ui.required`, unless their parameters are fully given.
- Typical scripts: batch schema conversion, batch IFC/PDF export, model checking, parameter updates from CSV, and family package upgrades.

**Commands.** (process-level, not registry commands) — documented in the user manual.

**Acceptance criteria.**
- [ ] `reviera --headless --command export.ifc --args '{"path":"out.ifc","setup_name":"IFC4 Reference View"}' model.ifc` produces the same file as the UI export.
- [ ] Running a headless script on 100 files in a loop uses all cores when invoked as several processes, and leaks no memory between files within one process.

### 2.11.8 Versioning, upgrade and migration
**ID:** DM-MIG-001 · **Priority:** P0 (rules) / P1 (save to previous RXD version) · **Ref:** research/05 §11.2, §16.9

**Purpose.** To avoid the reference product's one-way, yearly file-format break.

**Behaviour & rules.**
- **Two version axes:**
  1. the **IFC schema** (governed by buildingSMART; §2.11.3);
  2. the **RXD schema version** (semver, set by Reviera; `02a`).
  The application version is recorded too, but it does not by itself make files incompatible.
- **Compatibility promise:**
  - **Same RXD major version**: any Reviera release opens and saves the file without loss. Newer minor versions add optional keys. Older releases keep unknown keys (§2.4.1) and warn that "some features were created in a newer version and are shown read-only".
  - **Newer RXD major version**: older releases open the file as **IFC-only plus read-only RXD**. Building elements and parameters are editable. RXD documents that cannot be read are kept unchanged on save, and are flagged as stale for reconciliation when the newer release reopens the file.
  - **Older RXD major version**: newer releases migrate on open, using a chain of migration steps (vN → vN+1 → …). Each step is a pure function over RXD documents, with a unit test and sample files. The original file is never overwritten until the user saves, and a backup of the pre-migration file is kept.
- **Save to previous version (P1).** *File › Save As › Previous Version* writes RXD at the previous major version, when the model uses no features that exist only in the newer one. If it does, the dialog lists those features, and they are dropped (or turned into static IFC geometry) on export.
- **Family packages** follow the same rules. A batch upgrade of a folder of families is available through the headless mode (§2.11.7) and the `family.package.upgrade` command.
- **Release policy.** RXD major versions should change at most once every two years, with at least one year of overlap where the previous major version can still be written.

**Commands.** `file.save_previous_version(document_id, path) -> report` · `family.package.upgrade(paths[], in_place=false) -> report` · `rxd.migration_report(document_id) -> report`

**Acceptance criteria.**
- [ ] A project saved by release N (RXD 1.4) opens in release N−1 (RXD 1.2) with no data loss on save. The 1.4-only keys are kept and the user is warned.
- [ ] A project with RXD 2.0 opened in a 1.x release keeps all 2.0 RXD documents byte-identical through an edit and save of building elements.
- [ ] Each migration step has a test that migrates a sample file, and checks the result against the target JSON Schema.

---

## 2.12 The command registry

### 2.12.1 Command definitions
**ID:** DM-CMD-001 · **Priority:** P0 · **Ref:** research/05 §13.1 (external commands), §16.11; TECH_STACK §2, §3.5, §3.6

**Purpose.** Every operation is defined **once**, as a typed command. The UI, Lua, add-ins, MCP, the journal and the documentation are all generated from that definition.

**Behaviour & rules.**
- A command definition (Rust: a struct implementing a `Command` trait, registered at start-up with a macro; Lua add-ins register commands through the same registry, §2.14) shall contain:

| Field | Meaning |
|---|---|
| `name` | `domain.verb` or `domain.sub.verb`, lower snake case (for example `wall.create`, `family_doc.form.extrusion`). Unique and stable. |
| `version` | Integer, increased on any breaking change to parameters or result |
| `summary` / `description` | One line, and a paragraph (localisable). Used for tooltips, Lua docs and MCP tool descriptions. |
| `params` | A JSON Schema (draft 2020-12), generated from Rust types (`schemars`). It includes units (`x-reviera-data-type: "length"`), element-reference kinds (`x-reviera-element: {category: "cat.walls"}`), defaults and enums. |
| `result` | A JSON Schema for the result |
| `effect` | One or more of `read`, `edit`, `file_read`, `file_write`, `network`, `script`, `ui`, `destructive`, `settings` |
| `undoable` | Whether it creates an undo item (`false` for `read`, file and UI-only commands) |
| `transaction_name` | Default undo name |
| `availability` | A predicate over the context: document kind (project, family, family kind), active view kind, selection (count, categories), mode (Local or BIMserver), sketch mode, edit mode |
| `interactive` | Whether it can prompt for picks and dialogs when parameters are missing (UI only), and which parameters can be picked |
| `ui` | Optional ribbon placement (tab, panel, order), icon ID, default shortcut, context-menu placement |
| `permissions` | The minimum MCP permission level (§2.16.5) and Lua trust level (§2.13.7), derived from `effect` unless overridden |
| `deprecated` | Optional: replacement command and removal version |
| `examples` | Example calls (used in documentation, MCP descriptions and tests) |

- **One execution path.** Every caller (a ribbon button, a shortcut, Lua, MCP, journal replay) calls `registry.execute(name, args, context)`. That call validates `args` against the schema, checks `availability` and permissions, opens the transaction, runs the command, regenerates, commits or rolls back, and returns `{result, warnings[], changed: {added[], modified[], deleted[]}}`.
- **Units in arguments.** Numeric arguments with a data type accept either a number in SI, or a string with a unit (`"1200 mm"`), which is parsed with §2.9.5.
- **Element references in arguments** accept an internal ID (integer) or a GUID (string), and are normalised before the command runs.
- **Errors** are typed: `{code, message, details, resolutions?}`. Codes are stable strings (for example `param.read_only`, `ui.required`, `availability.wrong_view`, `failure.unresolved`).
- **Generated outputs:** the Lua binding (§2.13.2), MCP tools (§2.16.3), ribbon and shortcut bindings, the command-reference documentation (Markdown and HTML), JSON Schemas for each command (published with each release), and conformance test stubs.

**Commands.** `registry.list(filter?) -> command_summary[]` · `registry.describe(name) -> definition` · `registry.execute(name, args)` (the generic entry point; exposed to Lua as `reviera.run` and to MCP as `run_command`)

**Acceptance criteria.**
- [ ] A CI check fails if any `pub` model-mutating function in the core can be reached other than through a registered command.
- [ ] For every registered command, the generated Lua function, MCP tool and documentation entry exist and agree with the schema (checked by a CI test that compares them).
- [ ] Calling `wall.create` with `"height": "3000 mm"` and with `"height": 3.0` gives identical walls.

### 2.12.2 Command naming and stability
**ID:** DM-CMD-002 · **Priority:** P0 · **Ref:** TECH_STACK §2

**Behaviour & rules.**
- Domains are reserved per chapter: `ui.*`, `view.*`, `sheet.*`, `schedule.*`, `annotate.*` (DOC); `wall.*`, `floor.*`, `roof.*`, `door.*`, `modify.*` (ARC); `str.*` (STR); `mep.*` (MEP); `analysis.*` (ANA); `bimserver.*`, `link.*`, `workset.*` (COL); and the DM domains defined in this chapter (`element`, `instance`, `type`, `family`, `family_doc`, `cme`, `param`, `paramlib`, `global`, `units`, `material`, `object_style`, `line_*`, `fill_pattern`, `project`, `location`, `coordinates`, `position`, `site`, `building`, `spatial`, `phase`, `standards`, `purge`, `classification`, `keynotes`, `file`, `import`, `link`, `export`, `validate`, `rxd`, `edit`, `warnings`, `failure`, `regen`, `registry`, `script`, `addin`, `graph`, `mcp`, `model`, `settings`, `snaps`, `image`, `pset`, `qto`, `host`, `subcategory`, `formula`, `journal`, `external_props`, `library`, `numbering`, `decal`).
- **Add-in commands** are named `<addin-id>.<verb>` (for example `org.example.roomtools.renumber`). They cannot use reserved domains.
- **Stability.** A command's name and `version` form its contract. Breaking changes need a new version, and the previous version stays callable for at least one RXD major cycle (§2.11.8), through an adapter. Deprecations are announced in release notes and in `registry.describe`.

**Acceptance criteria.**
- [ ] Registering an add-in command named `wall.create` is refused at load time with an error naming the reserved domain.

---

## 2.13 Lua scripting API

### 2.13.1 Runtime
**ID:** DM-LUA-001 · **Priority:** P0 · **Ref:** research/05 §13.1, §13.4, §13.7; TECH_STACK §3.5

**Behaviour & rules.**
- LuaJIT is embedded through `mlua` (feature `luajit`). The language level is **Lua 5.1 plus LuaJIT extensions**. Scripts shall not rely on Lua 5.3+ features (integers, `goto` in 5.1 mode, `utf8` library). Reviera provides a `reviera.utf8` helper, and 64-bit integer IDs are exposed as LuaJIT `int64_t` cdata *or* as strings, as chosen by `reviera.config.id_mode` (default: strings for GUIDs, Lua numbers for internal IDs below 2^53, which covers all IDs in practice).
- **States:**
  - a **main state** runs on the command thread. It runs macros, add-ins, updaters and event handlers. It has full access to the API (subject to trust level);
  - **worker states** (optional) run pure-computation functions in parallel (for example geometry calculations for many elements). They have no access to the model or to I/O. They receive copied data and return values. They are pooled.
  - Each add-in gets its **own environment** (a separate global table) in the main state, so add-ins cannot overwrite each other's globals. Add-ins at trust level `native` may use their own state on request.
- **Platform check.** On platforms where the LuaJIT JIT is not reliable (to be checked per release; for example some ARM64 macOS builds), Reviera runs LuaJIT in interpreter mode (`jit.off()`). Results stay the same, but some scripts run more slowly.

**Acceptance criteria.**
- [ ] Two add-ins that both define a global `config` do not see each other's value.
- [ ] A worker-state function that tries to call `reviera.query` fails with `lua.no_model_access`.

### 2.13.2 Module map
**ID:** DM-LUA-002 · **Priority:** P0 · **Ref:** research/05 §13.1; TECH_STACK §3.5

**Behaviour & rules.** The global table `reviera` contains:

| Module | Contents | Generated? |
|---|---|---|
| `reviera.<domain>.<verb>(args)` | One function per registered command (for example `reviera.wall.create{...}`). Takes a table of named arguments, and returns the result table. Raises a Lua error with a typed error object on failure. | Yes, from the registry |
| `reviera.run(name, args)` | Generic command call by name | — |
| `reviera.doc` | The active document: `id`, `kind`, `path`, `title`, `is_family`, `version()`, `documents()` (all open documents), `activate(id)` | — |
| `reviera.query` | The element collector (§2.13.3) | — |
| `reviera.element(id_or_guid)` | Returns an element proxy (read-only properties, plus convenience writes that call commands) | — |
| `reviera.txn` | Transactions (§2.13.4) | — |
| `reviera.events` | Events (§2.13.5) | — |
| `reviera.updaters` | Updaters (§2.13.5) | — |
| `reviera.post`, `reviera.timer`, `reviera.async` | Deferred and asynchronous execution (§2.13.6) | — |
| `reviera.geom` | Read-only geometry access, and value types `XYZ`, `UV`, `Transform`, `BBox`, `Curve`, `Plane`, `Solid` (§2.13.10) | — |
| `reviera.units` | `parse`, `format`, `convert`, data-type constants | Wrappers of `units.*` |
| `reviera.ui` | Selection, picks, task dialogs, forms, ribbon and dockable panels (§2.13.9) | — |
| `reviera.fs`, `reviera.http` | File and network access, subject to trust level (§2.13.7) | — |
| `reviera.json`, `reviera.csv`, `reviera.utf8`, `reviera.log` | Utilities | — |
| `reviera.addin` | The calling add-in's ID, version, data folder and settings store | — |

- **Element proxies.** `reviera.element(x)` returns a proxy with fields `id`, `guid`, `category`, `name`, `type` (a proxy), `level`, `host`, `owner_view`, `location`, `bbox`, and methods:
  - `:param(name_or_guid_or_key)` returns `{value, display, data_type, is_read_only, is_type}`;
  - `:params()` returns all parameters;
  - `:set(name, value)` calls `param.set`, and therefore needs an open transaction;
  - `:geometry(options)`, `:dependents()`, `:is_valid()`.
  Proxies always read the **current** state of the document as the calling script sees it (inside a transaction, uncommitted changes are visible).

**Example (informative).**

```lua
-- Number all doors on Level 1 by room, inside one undoable transaction.
local doors = reviera.query()
  :of_category("cat.doors")
  :instances()
  :on_level("Level 1")
  :to_list()

reviera.txn.run("Renumber doors", function()
  for i, d in ipairs(doors) do
    local room = d:param("To Room").value or d:param("From Room").value
    local num  = room and reviera.element(room):param("Number").value or "X"
    d:set("Mark", string.format("%s-%02d", num, i))
  end
end)
```

**Acceptance criteria.**
- [ ] Every registered command is callable as `reviera.<name>` with a table argument, and the result matches the MCP tool result for the same arguments.
- [ ] The example above runs on the sample project, creates one undo item named "Renumber doors", and changes only Mark values.

### 2.13.3 Element queries (the collector)
**ID:** DM-LUA-003 · **Priority:** P0 · **Ref:** research/05 §13.1 (filtered element collector); TECH_STACK §3.5

**Purpose.** A fast, chainable way to find elements. It is the Reviera counterpart of a filtered element collector.

**Behaviour & rules.**
- `reviera.query([doc])` starts a query over a **consistent snapshot**: the committed state, or the transaction's current state when called inside a transaction. Filters are chained, and the query is **lazy**: nothing runs until it is iterated or a terminal method is called.
- **Quick filters** use indexes in the element store (category, class, type, level, owner view, workset, design option, bounding-box R-tree, GUID). **Slow filters** need element data (parameter values, geometry intersection, Lua predicates). The query planner always applies quick filters first, whatever order they were written in.

| Method | Kind | Meaning |
|---|---|---|
| `:of_category(key, …)` | quick | One or more categories (subcategories included) |
| `:of_ifc_class(name, …)` | quick | IFC class, including subclasses (for example `"IfcBuildingElement"`) |
| `:instances()` / `:types()` | quick | Instances only, or types only |
| `:of_type(type_id)` / `:of_family(family_id)` | quick | |
| `:on_level(level)` | quick | By reference level (name, ID or GUID) |
| `:in_view(view_id)` | quick | Visible in a view (uses the view's visibility cache) |
| `:owned_by_view(view_id)` | quick | View-specific elements of a view |
| `:in_workset(id)` / `:in_design_option(id)` / `:in_phase(id, status?)` | quick | |
| `:in_bbox(min, max, mode)` | quick | `mode` = `intersects`, `inside` or `contains` (R-tree) |
| `:ids(list)` / `:guids(list)` | quick | Restrict to a set |
| `:where(param, op, value)` | slow | `op`: `==`, `~=`, `<`, `<=`, `>`, `>=`, `contains`, `begins`, `ends`, `has_value`, `no_value`. Unit-aware, for example `:where("Width", ">=", "900 mm")`. |
| `:where_rule(rule)` | slow | A filter rule in the same format as view filters (DOC), including AND/OR nesting |
| `:intersects_element(id)` / `:intersects_solid(solid)` | slow | Exact geometric intersection (OCCT) |
| `:host_of(id)` / `:hosted_by(id)` / `:in_room(id)` | slow | Relationship filters |
| `:filter(fn)` | slow | A Lua predicate |
| `:excluding(ids)` | quick | |
| `:sort_by(param_or_fn, desc?)` | terminal helper | |
| `:to_list()`, `:ids()`, `:first()`, `:count()`, `:each(fn)`, `:iter()` (for `for el in q:iter() do`) | terminal | |
| `:group_by(param)` | terminal | Returns `{value → list}` |
| `:explain()` | terminal | Returns the query plan (index used, estimated rows) |

- Query results are proxies. They are invalidated if the elements are deleted later. `:is_valid()` checks this.
- The same query model is used by the MCP `query_elements` tool (§2.16.4). Its JSON form is `{category?, ifc_class?, instances?, type?, level?, view?, bbox?, where?: [{param, op, value}], limit?, offset?, fields?: []}`.

**Commands.** `element.query(spec) -> {ids[], total, fields?}` (the registry form, used by MCP)

**Acceptance criteria.**
- [ ] On a 100,000-element model, `query():of_category("cat.walls"):count()` takes under 5 ms, and `:where("Fire Rating","==","EI 60")` over 10,000 walls takes under 50 ms.
- [ ] Chaining `:filter(fn)` before `:of_category` gives the same results, with the same plan, as the reverse order.
- [ ] `:explain()` shows the index used for `of_category` and `in_bbox`.

### 2.13.4 Transactions from Lua
**ID:** DM-LUA-004 · **Priority:** P0 · **Ref:** research/05 §3; TECH_STACK §3.3, §3.5

**Behaviour & rules.**
- `reviera.txn.run(name, fn)` opens a transaction, runs `fn`, and commits. If `fn` raises an error, or returns `false`, the transaction is rolled back. It returns `true, result`, or `false, err`.
- `reviera.txn.savepoint(fn)` runs `fn` inside a savepoint. An error inside it rolls back the savepoint only, and the error is returned instead of raised.
- `reviera.txn.group(name, mode, fn)` runs `fn` inside a transaction group. `mode` is `"assimilate"`, `"commit"` or `"rollback"` (rollback is useful for "what-if" calculations).
- Explicit forms `txn.begin/commit/rollback` exist for advanced use. A transaction that is still open when the script ends is rolled back, with a warning.
- **Write outside a transaction.** Calling an `edit` command outside a transaction opens and commits an **implicit** transaction for that single command (one undo item per call). This is convenient for interactive use. Scripts should use `txn.run` to group changes.
- `reviera.txn.on_failure(fn)` registers a failure preprocessor for the current transaction (§2.7.4). `fn(failures)` returns a table of actions: `{id = "dismiss" | "<resolution>" | "continue"}`.
- `reviera.regen.now()` runs regeneration part-way through the transaction (§2.6.4).

**Acceptance criteria.**
- [ ] `txn.run` with a function that errors after creating 3 elements leaves 0 elements and no undo item, and returns `false` with the error.
- [ ] A transaction group with mode `rollback` that moves 100 elements and reads their new room assignments leaves the model unchanged, and returns the values read.

### 2.13.5 Events and updaters (dynamic model update)
**ID:** DM-LUA-005 · **Priority:** P1 · **Ref:** research/05 §3 (events, updaters), §13.1

**Purpose.** Scripts can react to changes **after** they are committed (events), or **inside** the transaction that made them (updaters), so that dependent data stays correct automatically.

**Behaviour & rules.**
- **Events** (handlers run after commit, read-only by default; a handler may start its own transaction with `reviera.post`, §2.13.6):

| Event | Payload |
|---|---|
| `app.started`, `app.closing` | — |
| `document.opened`, `document.closing`, `document.saved`, `document.saving` (can cancel), `document.activated` | document ID, path |
| `document.changed` | `{txn_name, origin, added[], modified[{id, facets[], params[]}], deleted[], undo?, redo?}` |
| `view.activated`, `selection.changed` | view ID / element IDs |
| `family.loaded`, `family.reloaded` | family ID |
| `model.id_remap` | `{old → new}` (§2.5) |
| `bimserver.revision_available`, `bimserver.synced` | COL chapter |
| `command.before`, `command.after` | command name, args, result. `command.before` can **cancel** (trust level `standard` and above; §2.13.7). |
| `failure.processing` | the failure list (as for preprocessors) |
| `app.idle` | — (fired after 100 ms of no input, at most 10 times per second) |

- **Updaters** (the equivalent of dynamic model update):

```lua
reviera.updaters.register{
  id       = "org.example.roomtools.door-room-sync",   -- unique, stable
  name     = "Copy room number to door",
  triggers = {
    { query = { category = "cat.doors", instances = true }, change = "geometry" },
    { query = { category = "cat.rooms" },                  change = { param = "Number" } },
    { query = { category = "cat.doors" },                  change = "added" },
  },
  priority = 100,          -- lower runs first; ties broken by id
  optional = true,         -- if the add-in is missing, the model still opens without prompts
  run = function(ctx)      -- runs INSIDE the triggering transaction, before commit
    for _, id in ipairs(ctx.affected("cat.doors")) do
      local d = reviera.element(id)
      local room = d:param("To Room").value
      if room then d:set("Room Number", reviera.element(room):param("Number").value) end
    end
  end,
}
```

  - Change kinds: `added`, `deleted`, `geometry` (placement or shape facet changed), `type_changed`, `{param = <ref>}`, `any`.
  - Updaters run after the command, **before** regeneration finishes. Their changes become part of the same transaction and undo item. Regeneration then runs again for what they changed.
  - **Loop control**: an updater's own changes can trigger other updaters, but not itself again in the same pass. The whole updater phase runs at most 10 passes. If it does not settle, the transaction fails with `updater.no_convergence`, naming the updaters involved.
  - **Time budget**: each updater has a soft limit (default 200 ms per transaction). Going over it is logged and shown in the Add-in Manager. An updater that raises an error is **disabled** for the session, a warning is shown, and the transaction continues without its changes (default), or fails (when the updater is registered with `on_error = "fail"`).
  - Updaters do not run on undo, redo, or merges of incoming BIMserver revisions (where the changes already include the updater results).
  - **Model declaration.** When a document is saved with an active updater that made changes, the updater's ID and the add-in ID are recorded in RXD document `project/settings` (`updaters_used`). On open without that add-in: if `optional = true`, nothing happens; otherwise, the user is warned that "data maintained by add-in X may become out of date if you edit".

**Commands.** `events` and `updaters` are API-level, not registry commands. Introspection: `addin.updaters() -> [{id, addin, triggers, enabled, stats}]` · `addin.updater.enable(id, on)`

**Acceptance criteria.**
- [ ] The example updater sets "Room Number" on a door when the door is moved into another room. The door move and the parameter change are one undo item.
- [ ] Two updaters that keep changing each other's trigger parameters fail the transaction with `updater.no_convergence` after 10 passes, and the model is unchanged.
- [ ] A `document.changed` handler receives the exact added, modified and deleted IDs of each transaction.

### 2.13.6 Deferred, asynchronous and modeless execution
**ID:** DM-LUA-006 · **Priority:** P1 · **Ref:** research/05 §3 (modeless contexts, external events)

**Purpose.** Modeless UIs, timers, network calls and long computations must change the model safely. This is the equivalent of external events.

**Behaviour & rules.**
- The model can only be changed on the command thread, in a transaction. Code running in other contexts (dockable-panel callbacks, timers, HTTP callbacks, worker states) cannot change the model directly. Instead it uses:
  - **`reviera.post(fn, opts?)`** queues `fn` to run on the command thread, in the next free slot, with full API access. `opts.name` labels it in the journal. `opts.coalesce = key` replaces a pending call with the same key. Returns a handle with `:cancel()` and `:status()`.
  - **`reviera.timer(ms, fn, {repeat?})`** runs `fn` through `post` after a delay.
  - **`reviera.async(fn)`** runs `fn` as a coroutine on the command thread. Inside it, `reviera.await(x)` suspends without blocking the UI until `x` (an HTTP request, a worker-state job, a timer, or a user pick in the UI) completes. It then resumes on the command thread. Transactions cannot be held open across an `await`. Trying to do so fails with `txn.await_in_transaction`.
  - **`reviera.workers.map(fn_source, items, opts)`** runs a pure function over items in worker states, in parallel, and returns the results to the coroutine.
- The UI never waits for Lua code to finish. Long-running script steps show a progress bar with *Cancel*. Cancel raises `lua.cancelled` at the next API call or yield point.

**Acceptance criteria.**
- [ ] A dockable panel button that calls `reviera.post` to create a wall works while the user is orbiting a 3D view, and the wall appears as one undo item.
- [ ] An `async` script that awaits an HTTP request of 5 s does not freeze the UI. The user can keep editing, and the script's transaction runs after the response.

### 2.13.7 Sandbox and trust levels
**ID:** DM-LUA-007 · **Priority:** P0 · **Ref:** research/05 §13.2 (load prompts), §13.4 (macro security); TECH_STACK §3.5

**Purpose.** To run scripts from many sources safely. LuaJIT's FFI gives native memory access, so access must be limited by trust.

**Behaviour & rules.**

| Level | Name | Model | Files | Network | OS / processes | `ffi`, `debug`, `load` of bytecode | Typical use |
|---|---|---|---|---|---|---|---|
| 0 | `read_only` | Queries only. No transactions. | ✘ | ✘ | ✘ | ✘ | MCP `execute_lua` at read-only permission, document scripts not yet trusted |
| 1 | `model` | Read and write through commands | ✘ (except the add-in's own data folder) | ✘ | ✘ | ✘ | Most add-ins and macros |
| 2 | `standard` | As level 1, plus events that can cancel | Read and write in user-chosen paths (through file dialogs, or paths declared in the manifest and approved), and the add-in data folder | HTTPS to hosts declared in the manifest | ✘ | ✘ | Add-ins that import and export data |
| 3 | `trusted` | As level 2 | Any path the user can access | Any host | `os.execute`/`io.popen` with a confirmation prompt the first time | ✘ | Firm-internal tools |
| 4 | `native` | As level 3 | Any | Any | Any | ✔ `ffi`, `debug`, bytecode loading | Add-ins that bind native libraries. Requires explicit user approval with a strong warning. |

- **Removed or wrapped globals** at levels 0–2: `io`, `os` (except `os.time`, `os.clock`, `os.date`), `require` of C modules, `package.loadlib`, `load`/`loadstring` with bytecode (text chunks are allowed), `dofile`/`loadfile` (replaced by `reviera.addin.require` for files inside the add-in package), `debug`, `ffi`, `jit` (except `jit.status`), `collectgarbage("stop")`.
- **Resource limits** (all levels except `native`, configurable):
  - wall-clock limit per synchronous call (default 30 s, then a prompt to continue or cancel);
  - instruction-count hook, for interrupting runaway loops (the JIT is disabled for level-0 scripts so the hook is reliable; at higher levels the check also runs on every API call);
  - memory limit per add-in, through a custom allocator where the binding supports it, and otherwise by monitoring.
- **Granting trust.**
  - The add-in's manifest *requests* a level (§2.14.1). The user *grants* it on first load, through a dialog that lists the capabilities.
  - Grants are remembered per add-in ID **and** content hash. A changed add-in asks again.
  - Signed add-ins from publishers the user trusts (OpenPGP or minisign keys in the trust store) skip the prompt, up to level 3.
  - Level 4 always asks.
- **Document scripts** (stored in a model's RXD, §2.13.8) run at level 0 until the user trusts that document's scripts. Trust is per document GUID and script hash.
- **Script Security** settings (Manage › Scripts): enable or disable application scripts and document scripts, ask before running, and the trust store (keys, granted add-ins, revocations).
- **Script Security dialog** (`script.security()`), with three tabs:
  - **Scripts**: *Application scripts* — Enabled / Ask before each run / Disabled; *Document scripts* — Disabled until trusted (default) / Ask when a document with scripts opens / Always disabled; the **maximum trust level** a user may grant (default `trusted`; `native` only when the user ticks a separate, warned box); the wall-clock and memory limits (wall clock 5–600 s, default 30 s; memory 16 MB–4 GB per add-in, default 256 MB).
  - **Trusted publishers**: the list of OpenPGP and minisign public keys, with fingerprint, name, added date and the highest level their signed add-ins get without a prompt (at most `trusted`). Buttons: Import key…, Remove, Show fingerprint. A key must parse and have a valid fingerprint to be added.
  - **Grants**: every remembered grant (add-in ID, version hash, level, date) and every trusted document (document GUID, title, script hashes, level, date), with **Revoke**. Revoking takes effect for the next load or run; a running add-in is not interrupted.
  - Settings are per user. An administrator **policy file** can lock any of them (shown greyed with "set by your organisation"), for example to force *Document scripts: Always disabled* or to cap the maximum level. Policy always wins over user settings.
  - **OK** saves the settings (not undoable; they are not model data); **Cancel** discards them.

**Commands.** `script.security()` (opens the Script Security dialog); trust decisions for add-ins use `addin.trust.grant` / `addin.trust.revoke` (§2.14.3) and for document scripts `rxd.trust_scripts` (`02a`).

**Acceptance criteria.**
- [ ] A level-1 script calling `io.open("/etc/passwd")` fails with `lua.sandbox.denied`, and the attempt is logged.
- [ ] A level-0 script running `while true do end` is interrupted within the configured limit, with no UI freeze longer than 1 s after the user chooses Cancel.
- [ ] Changing one byte of a trusted add-in's code makes it ask for trust again on next load.
- [ ] A document script in a downloaded project cannot write to the model until the user trusts it.
- [ ] With a policy file that sets *Document scripts: Always disabled*, the Script Security dialog shows that option greyed, and `rxd.trust_scripts` fails with `lua.policy.denied`.

### 2.13.8 Scripts (macros), the Script Manager and document scripts
**ID:** DM-LUA-008 · **Priority:** P1 · **Ref:** research/05 §13.4, §13.7; screenshots/platform/macro_manager_and_vscode_2026.png, screenshots/platform/macro_vscode_editor_2026.png

**Purpose.** Lightweight automation without packaging an add-in.

**UI.** Manage › Scripts › **Script Manager**:
- tabs: **Application** (user scripts in the user script folder) and **Document** (scripts stored in the model);
- each tab is a tree of modules (Lua files) and entry points (functions marked with an annotation comment, for example `--@macro "Renumber doors"`);
- buttons: New module, New macro, Edit, Rename, Delete, Run, Step into (debugger), Import, Export, *Open folder*;
- **Edit** opens the built-in **script editor**: a dockable panel with syntax highlighting, completion generated from the registry, inline docs, a console (REPL) with result inspection, breakpoints and a variable view. Or it opens an **external editor** chosen in settings (any editor; Reviera provides a Language Server for completion, `reviera-lua-ls`, and writes type stub files for common Lua language servers).

**Behaviour & rules.**
- **Application scripts** live in `$XDG_CONFIG_HOME/reviera/scripts` (or the platform equivalent) and are available in every document.
- **Document scripts** are stored in RXD document `scripts/<name>` and travel with the model. They are subject to §2.13.7.
- **Recording (P2).** *Record* captures the commands the user runs (from the journal, §2.7.3) into a new Lua macro, with literal arguments. The user can then edit it.
- **Quick-access shell.** A **Lua console** panel (View › User Interface › Lua Console) runs one-line commands against the active document, at trust level `model`, or higher if granted.
- **Script Manager dialog** (`script.manager()`). It is modeless, so the user can keep working while it is open.
  - Each module row shows its name, scope, last modified time, trust status (document modules: *Not trusted* / *Trusted at level n*) and load errors. Each entry point shows its display name (from the `--@macro` annotation), description (the comment block above it) and an optional `--@args` schema, from which **Run** builds a parameter form.
  - **New module** asks for a name (a valid Lua module name: letters, digits and `_`, starting with a letter, unique in the scope) and creates it from a template with one sample entry point. **New macro** adds an annotated function to the selected module. **Rename** renames a module or entry point and updates shortcut and Quick Access Toolbar bindings. **Delete** asks for confirmation and lists bindings that will be removed.
  - Modules are syntax-checked on save and on refresh; a module with a syntax error is listed with the line and message and its entry points are disabled.
  - Changes to document modules are model changes (one undoable transaction each, stored in `scripts/<name>`); changes to application modules are file operations and are not undoable.
  - **Run** executes the entry point at the module's granted trust level; document modules that are not trusted run only after the trust prompt (§2.13.7). Output and errors go to the console panel.
- **Run Script…** (Manage › Scripts, and Add-ins › External; `script.run_file(path, entry?, args?)`) runs a Lua file from disk once, without adding it to the Script Manager. The first run of a file with an unknown hash shows a prompt naming the file and its SHA-256 hash, offering to run it at level `model` (default) or at a higher level up to the user's maximum (§2.13.7); the choice is remembered per hash, like add-in grants, and a changed file asks again. Without `entry`, the file's main chunk runs; with `entry`, that global function is called with `args`. The most recent 10 files are offered in a list. Its transactions follow §2.13.4, so a script that changes the model creates undo items named by the script.

**Commands.** `script.run(module, entry, args?)` · `script.list(scope: "application"|"document") -> script[]` · `script.create(scope, module, source)` · `script.delete(scope, module)` · `script.import(scope, path)` · `script.export(scope, module, path)` · `script.record_start()` · `script.record_stop(module_name) -> source` · `script.manager()` (opens the Script Manager) · `script.run_file(path, entry?, args?)`

**Acceptance criteria.**
- [ ] A macro created in the Script Manager appears in the Run list immediately, and can be bound to a keyboard shortcut and a Quick Access Toolbar button.
- [ ] A document script survives save and reopen, and is listed under Document after reopening.
- [ ] The built-in editor offers completion for `reviera.wall.cr…` with the parameter list of `wall.create`.
- [ ] `script.run_file` on a file that calls `wall.create` asks for trust once, creates one undo item, and on a second run with the same file hash runs without asking.
- [ ] A document module with a syntax error is shown in the Script Manager with its line number, and its entry points cannot be run.

### 2.13.9 UI extension API
**ID:** DM-LUA-009 · **Priority:** P1 · **Ref:** research/05 §13.1 (ribbon, dockable panes, dialogs, selection)

**Behaviour & rules.**
- **Ribbon.** Add-ins declare ribbon items in their manifest (§2.14.1) or at runtime with `reviera.ui.ribbon.add{tab, panel, items}`. Items are push buttons, split buttons, pull-downs, toggles, combo boxes and text boxes. Each one calls a registered command, with an availability predicate. Add-ins get their own tab by default, or their own panel in the *Add-ins* tab.
- **Selection and picks.** `reviera.ui.selection()` and `reviera.ui.select(ids)`. `reviera.ui.pick_object{filter, prompt}`, `pick_objects`, `pick_point{snap, work_plane}`, `pick_box`, `pick_face`, `pick_edge`. Picks work inside `reviera.async` coroutines (§2.13.6) and in synchronous commands.
- **Dialogs.**
  - `reviera.ui.task_dialog{title, main, content, buttons, commands, checkbox}` shows a task dialog;
  - `reviera.ui.form{title, fields = {...}}` builds a declarative form, with fields such as text, number with unit, element picker, drop-down, check box, table and file picker, plus validation;
  - forms are rendered in native wxWidgets controls.
- **Dockable panels.** `reviera.ui.panel.register{id, title, default_dock, build = function(p) … end}` uses the same declarative controls, plus a list or table view, a tree view and an HTML view. The HTML view is a sandboxed web view with no access to the API except through a message channel that calls registered commands.
- **Transient graphics.** `reviera.ui.overlay` draws temporary lines, points, text and meshes in views (not saved, cleared on command end or on request), for previews and analysis display.

**Acceptance criteria.**
- [ ] An add-in that declares a ribbon tab with 3 buttons shows them on start-up. Each button is greyed out when its command's availability predicate is false (for example, when no wall is selected).
- [ ] A form with a length field accepts `3' 6"` and returns 1.0668 m.

### 2.13.10 Geometry API
**ID:** DM-LUA-010 · **Priority:** P1 · **Ref:** research/05 §13.1 (geometry, direct shapes, ray casting)

**Behaviour & rules.**
- **Value types** (immutable, in `reviera.geom`): `XYZ`, `UV`, `Transform`, `Plane`, `BBox`, `Curve` (line, arc, ellipse, NURBS, polyline), `CurveLoop`, `Solid`, `Mesh`.
- **Access:** `element:geometry{detail = "fine", view = id?, include_symbolic?}` returns solids, curves and meshes in world coordinates, with stable references for faces and edges (§2.8.3).
- **Operations** (pure, run on OCCT copies):
  - boolean union, difference and intersection;
  - extrude, revolve, sweep, loft;
  - section by plane;
  - volume, area and centroid;
  - point containment, distance, closest point;
  - ray cast against the model (`reviera.geom.raycast{origin, direction, filter}` returns hits with element ID, face reference and distance).
- **Direct shapes.** `reviera.direct_shape.create{category, geometry, name?, type?}` creates a non-parametric element of a category from solids or meshes. It is stored as an IFC element with that geometry (§2.11.1 "IFC-native element" behaviour).

**Commands.** `direct_shape.create(category, geometry, name?, type_id?) -> element_id` · `direct_shape.set_geometry(element_id, geometry)` · `geom.raycast(origin, direction, filter?) -> hit[]`

**Acceptance criteria.**
- [ ] `geom.raycast` from inside a room towards +X returns the first wall hit, with the face reference `#face:interior`.

### 2.13.11 API versioning and documentation
**ID:** DM-LUA-011 · **Priority:** P0 · **Ref:** research/05 §13.1 (API changes by release), §16.11

**Purpose.** Add-ins and scripts must keep working across Reviera releases, and authors must know exactly what changed. This avoids the forced rebuilds that the reference product's runtime changes caused (Appendix A).

**Behaviour & rules.**
- The Lua API has a **semver API version**, separate from the application version. Add-ins declare a compatible range (§2.14.1). Minor versions only add features. Major versions may remove features, after one minor release of deprecation warnings.
- **What the API version covers:** every `reviera.*` module function and its arguments, the element proxy fields and methods, event names and payloads, updater trigger kinds, error codes, and the **command versions** visible to Lua (§2.12.2). A command's own `version` changing is an API change: a breaking command change bumps the API major version, unless the old command version stays callable through an adapter (in which case it is a minor change).
- **Range check at load.** The add-in's `reviera_api` range (Cargo-style semver syntax: `^`, `~`, `>=`, `<`, comma for AND) is checked against the running API version before its `entry` runs. Outside the range, the add-in is not loaded and the Add-in Manager shows both versions. Scripts without a manifest (Script Manager, `script.run_file`) may declare `--@reviera_api "^1.2"` on their first line; without it they run against the current version with no check.
- **Deprecations.** A deprecated function or command still works and logs one warning per add-in per session, naming the replacement and the removal version. `reviera.api.deprecations()` lists the deprecated items an add-in has called in this session; the Add-in Manager shows a count per add-in.
- **Feature probing.** `reviera.api.version` (a string), `reviera.api.has(name)` (true if a module function or command exists in this build) let scripts adapt to optional features (for example BIMserver mode, P2 commands).
- **Documentation.** Generated reference documentation (HTML and Markdown) lists every module, function, command and event, with examples. It is published per API version, and shipped offline with the application. Each entry shows the version that introduced it, and deprecated entries show their replacement. A **changelog** per API version lists additions, deprecations and removals, generated from the registry and the Lua binding definitions and checked in CI.
- Lua type stubs (for common Lua language servers) are generated per API version.
- **Support window.** Each API major version is supported (bug fixes, no removals) for at least two years after the next major version ships, matching the RXD policy (§2.11.8).

**Commands.** None of its own; `registry.describe` (§2.12.1) reports each command's version and deprecation, and the documentation is served as MCP resources `reviera://docs/{topic}` (§2.16.6).

**Acceptance criteria.**
- [ ] An add-in that declares `reviera_api = "^1.2"` loads on API 1.5, and is refused on API 2.0 with a message naming the incompatibility.
- [ ] Calling a deprecated function logs exactly one warning per session for that add-in, naming the replacement, and the call still succeeds.
- [ ] The generated changelog for a release lists every command whose `version` changed, and CI fails if a command's schema changed without a version change.
- [ ] `reviera.api.has("bimserver.connect")` returns false in a build without BIMserver support and true otherwise.

---

## 2.14 Add-ins

### 2.14.1 Add-in manifest
**ID:** DM-ADDIN-001 · **Priority:** P0 (manifest, licence check) / P1 (UI, signing, registry) · **Ref:** research/05 §13.2; TECH_STACK §4.2

**Purpose.** A declarative description of an add-in. It is used for discovery, licence policy, trust, command registration and UI placement.

**Behaviour & rules.**
- An add-in is a folder (or a `.revieraaddin` ZIP of that folder) containing `reviera-addin.toml`, Lua sources, resources, a `LICENSE` file and, optionally, `README`, icons, translations and tests.
- **Manifest format (normative; TOML 1.0):**

```toml
manifest_version = 1

[addin]
id          = "org.example.roomtools"      # reverse-DNS, unique, stable
name        = "Room Tools"
version     = "1.4.2"                      # semver
license     = "GPL-3.0-or-later"           # REQUIRED: SPDX license expression
authors     = ["Jane Doe <jane@example.org>"]
description = "Renumbering and room-door synchronisation."
homepage    = "https://example.org/roomtools"
source      = "https://git.example.org/roomtools"   # REQUIRED for registry submission
reviera_api = "^1.2"                       # semver range of the Lua API (§2.13.11)
entry       = "main.lua"                   # run on load; returns an init function
min_reviera = "1.0.0"                      # optional

[trust]
level = "standard"                         # requested level (§2.13.7)
network_hosts = ["api.example.org"]        # for level "standard"
paths = ["~/Documents/RoomTools"]          # extra file paths requested (approved by the user)

[[commands]]                                # registered in the command registry (§2.12)
name        = "org.example.roomtools.renumber"
function    = "commands.renumber"          # Lua path inside the add-in
summary     = "Renumber rooms along a path"
effect      = ["edit"]
params_schema = "schemas/renumber.json"    # JSON Schema; exposed to MCP automatically
mcp         = true                         # expose as an MCP tool (default true; false hides it)

[[ribbon]]
tab = "Room Tools"
panel = "Numbering"
items = [ { command = "org.example.roomtools.renumber", label = "Renumber", icon = "icons/renumber.svg" } ]

[[updaters]]
id = "org.example.roomtools.door-room-sync"
module = "updaters.door_room"
optional = true

[[panels]]
id = "org.example.roomtools.browser"
title = "Room Browser"
module = "ui.browser"

[dependencies]
"org.example.common" = "^2.0"

[signature]                                 # optional
key_id = "minisign:<publisher public key>"     # signature file: reviera-addin.toml.minisig
```

- **`license` is mandatory.** Its value must be a valid SPDX license expression (SPDX 2.3 syntax, including `AND`, `OR`, `WITH` and `+`).
- **Required files:** `LICENSE` (the licence text). For registry submission, the source is required as well.
- **Validation** happens on discovery. An invalid manifest stops the add-in from loading, and the Add-in Manager shows the field errors.

**Commands.** `addin.validate(path) -> {ok, errors[], license_status}`

**Acceptance criteria.**
- [ ] A manifest missing `license`, `id`, `version` or `reviera_api` fails validation, and each missing field is named.
- [ ] A manifest whose `[[commands]]` entry refers to a missing JSON Schema file fails validation. With a valid schema, the command appears in `registry.describe` and as an MCP tool with that schema.

### 2.14.2 Discovery, licence policy and loading
**ID:** DM-ADDIN-002 · **Priority:** P0 · **Ref:** research/05 §13.2; TECH_STACK §4.2

**Behaviour & rules.**
- **Locations** (searched in this order; a later location never overrides an add-in ID already found, and duplicates are reported):
  1. per user: `$XDG_DATA_HOME/reviera/addins/<id>/` (Linux), `%APPDATA%\Reviera\addins\<id>\` (Windows), `~/Library/Application Support/Reviera/addins/<id>/` (macOS);
  2. all users: a system-wide, admin-writable location (for example `/usr/share/reviera/addins/`, `%ProgramFiles%\Reviera\addins\`). It is not user-writable, for security;
  3. folders listed in settings (for example a firm network share), which can be marked trusted by an administrator policy file.
- **Licence check (as required by TECH_STACK §4.2).** The loader parses the SPDX expression and evaluates it against the **GPL-compatible allow-list**:
  - allowed identifiers: `GPL-3.0-only`, `GPL-3.0-or-later`, `LGPL-2.1-or-later`, `LGPL-3.0-only`, `LGPL-3.0-or-later`, `AGPL-3.0-only`, `AGPL-3.0-or-later`, `MIT`, `MIT-0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Apache-2.0`, `MPL-2.0`, `Zlib`, `0BSD`, `Unlicense`, `CC0-1.0`, `BSL-1.0`, `Artistic-2.0`, `X11`, `Python-2.0`, and others that a maintainer adds after review;
  - the allow-list is a versioned data file shipped with Reviera and updated with releases;
  - evaluation: an `OR` is compatible if **any** branch is compatible; an `AND` is compatible if **all** parts are; `WITH <exception>` is compatible if the base licence is and the exception is on the allowed-exceptions list (for example `Classpath-exception-2.0`, `LLVM-exception`);
  - `GPL-2.0-only` (without an `OR` to a compatible licence), proprietary, `LicenseRef-*`, "non-commercial" and missing licences are **not** compatible.
- **Not compatible → not loaded by default.** The Add-in Manager lists the add-in as *Blocked (licence)*, showing its expression and why it failed.
- **User override.** The user can enable a clearly labelled override per add-in ("Load this add-in anyway. I understand that it is not GPL-compatible, and that I may not distribute it with Reviera"). Overrides are remembered per add-in ID and version. The GPL does not restrict private use, so this matches TECH_STACK §4.2.
- **Load order.** Dependencies first (topological). Cycles are refused. Each add-in's `entry` runs in its own environment at its granted trust level (§2.13.7). Its returned `init(ctx)` is called. Its commands, ribbon items, panels and updaters are registered.
- **Unload and reload** (P1): an add-in can be disabled or reloaded without restarting. Its commands, ribbon items, panels and updaters are removed, and its handlers are unregistered. This is useful for development (a hot-reload button).
- **Failure isolation.** An error in an add-in's `entry` disables that add-in only, and the error is shown in the Add-in Manager.

**Acceptance criteria.**
- [ ] An add-in with `license = "MIT OR Proprietary-ACME"` loads (the `MIT` branch is compatible). One with `license = "GPL-2.0-only"` is blocked. One with no `license` field is blocked, with "missing license".
- [ ] With the override enabled for a blocked add-in, it loads, and the Add-in Manager shows an "override" badge.
- [ ] An add-in whose `entry` raises an error does not prevent other add-ins or the application from starting.

### 2.14.3 Add-in Manager, packaging and registry
**ID:** DM-ADDIN-003 · **Priority:** P1 · **Ref:** research/05 §13.2, §13.7

**UI.** Manage › Add-ins › **Add-in Manager**: a list with name, ID, version, licence (with compatibility status), granted trust level, source location, enabled/disabled, load time and errors. Buttons: *Enable/Disable*, *Reload*, *Install from file…*, *Install from registry…*, *Uninstall*, *Update*, *Show commands*, *Show updaters* (with run statistics), *Open folder*, *Review trust*.

**Add-in Manager dialog** (`addin.manage()`). It is modeless.
- **List.** Status badges: *Loaded*, *Disabled*, *Blocked (licence)*, *Blocked (API version)*, *Failed* (with the error), *Override* (licence override in effect), *Update available*. Filters: All, Enabled, Problems, Updates. Search by name, ID or command name. Sorting by any column.
- **Details pane** for the selected add-in: manifest fields (description, authors, homepage, source, licence expression and why it passed or failed, requested and granted trust level, network hosts and paths), dependencies with their resolved versions, registered commands (name, effect, MCP exposure), ribbon items, panels, updaters with statistics (runs, average and maximum time, errors), deprecation warnings (§2.13.11), and the last 50 log lines.
- **Actions and their rules.**
  - *Enable/Disable* (`addin.enable`) takes effect at once (unload or load, §2.14.2). Disabling an add-in that others depend on lists the dependants and disables them too after confirmation.
  - *Uninstall* (`addin.uninstall`) is possible only for per-user add-ins; all-user and policy-folder add-ins show why not. It removes the folder after unloading and keeps the add-in's data folder unless *Remove settings and data* is ticked.
  - *Install from file…* (`addin.install`) validates the package (manifest, signature, licence, API range, dependency availability) and shows a summary before extracting. An ID that is already installed is treated as an update; a lower version asks for confirmation (downgrade).
  - *Update* (`addin.update`) downloads from the registry the add-in came from, verifies checksum and signature, and asks for trust again when the requested level or content hash changed.
  - *Licence override* (`addin.set_override`) is shown only for blocked add-ins, behind the warning text of §2.14.2.
  - *Review trust* opens the grant for this add-in (`addin.trust.grant` / `addin.trust.revoke`).
- Changes made here are application settings, not model changes, and are not undoable.
- An add-in that registered updaters which made changes in the active project shows a "used by this project" marker (from `updaters_used`, §2.13.5), and disabling it warns that data it maintains may become out of date.

**Behaviour & rules.**
- **Packaging.** `.revieraaddin` is a ZIP of the add-in folder. *Install from file* verifies the manifest and the optional signature, shows the requested trust level and licence, then extracts the archive to the per-user location.
- **Registry (P2).** An optional public registry is an index of add-ins: a static Git-hosted JSON index of package URLs and checksums, which anyone can mirror. It only accepts add-ins whose licence is compatible and whose source is public, as TECH_STACK §4.2 requires. Each entry records the SPDX expression, the source URL, checksums and signatures. The application can browse, install and update from configured registries.
- **Developer tools.** `reviera addin new <id>` creates a template add-in with a manifest, `LICENSE` (GPL-3.0-or-later by default), tests and type stubs. `reviera addin pack` validates and packs. `reviera addin test` runs tests headless (§2.11.7).

**Commands.** `addin.list() -> addin[]` · `addin.enable(id, on)` · `addin.reload(id)` · `addin.install(path_or_registry_id) -> id` · `addin.uninstall(id)` · `addin.update(id)` · `addin.set_override(id, on)` · `addin.trust.grant(id, level)` · `addin.trust.revoke(id)` · `addin.manage()` (opens the Add-in Manager)

**Acceptance criteria.**
- [ ] Installing a signed add-in from a trusted key needs no trust prompt up to level 3. Installing an unsigned one shows the prompt with the requested capabilities.
- [ ] Reloading an add-in after editing its Lua code updates its commands without restarting, and its ribbon buttons keep working.
- [ ] Disabling an add-in on which two others depend lists both and, after confirmation, disables all three; their commands disappear from `registry.list` and from the MCP tool list.
- [ ] Uninstalling an add-in from the all-users location is refused with a message explaining that it needs an administrator.

---

## 2.15 Visual programming (future)

### 2.15.1 Reviera Graph
**ID:** DM-VPL-001 · **Priority:** P2 · **Ref:** research/05 §13.5, §13.6; screenshots/platform/dynamo_interface_library_canvas.jpg, screenshots/platform/dynamo_revit_node_library.jpg, screenshots/platform/dynamo_documentation_browser_2026.png

**Purpose.** A node-based visual programming environment for users who do not write code. It is built on the command registry and Lua, and does not need a separate runtime.

**Behaviour & rules.**
- **Graph files** (`.rgraph.json`, JSON, validated by a JSON Schema) contain nodes, typed ports, wires, groups, notes, graph inputs and outputs, and metadata (name, description, licence, required add-ins, API version).
- **Node library** (searchable tree, with documentation for each node):
  - **Registry nodes**: one node per registered command, generated automatically. Inputs come from the parameter schema; outputs from the result schema.
  - **Query nodes**: built from the collector (§2.13.3), for example *All Elements of Category* and *Filter by Parameter*.
  - **Core nodes**: math, logic, lists (map, filter, zip, flatten, group, sort), strings, dictionaries, units, geometry value types (§2.13.10).
  - **Script nodes**: a Lua code block, and a *formula* node (§2.8.7).
  - **Custom nodes**: sub-graphs saved as `.rgraphnode.json`.
  - **Package nodes**: from add-ins.
- **Execution.**
  - The graph is compiled to Lua, and runs at the graph's granted trust level (§2.13.7).
  - Each run is **one transaction** (or a group, when nodes ask for it).
  - Run modes: *Manual*, *Automatic* (re-run on input change, with a debounce), *Periodic*.
  - **Element binding**: a re-run updates the elements that the previous run created (tracked in RXD element data `graph_binding` by graph ID and node path), instead of creating duplicates.
- **Canvas.** Pan and zoom, wire routing, node preview bubbles, a background 3D preview of geometry values, run status per node (OK, warning, error), and a linter (unused inputs, missing licences, unsafe trust).

**Commands.** `graph.open(path) -> graph_id` · `graph.run(graph_id, inputs?) -> outputs` · `graph.save(graph_id, path?)` · `graph.compile(graph_id) -> lua_source`

**Acceptance criteria.**
- [ ] A graph "All walls of category → Filter by Fire Rating = EI 60 → Set Comments 'checked'" runs as one undo item, and running it again changes nothing.
- [ ] A graph that creates 10 columns on grid intersections updates the same 10 columns (the same GUIDs) when the grid moves and the graph runs again.

### 2.15.2 Graph Player
**ID:** DM-VPL-002 · **Priority:** P2 · **Ref:** research/05 §13.5; screenshots/platform/dynamo_player_inputs_2026.png

**Purpose.** Let users who never open the graph editor run published graphs with their own inputs.

**Workflow.**
1. Manage › Visual Programming › **Graph Player**. The panel lists graphs from the configured folders.
2. Select a graph; its marked inputs appear as a form, with the author's defaults.
3. Set inputs (pick elements in the model where asked) and choose **Run**.
4. Read the status, warnings and outputs; select output elements with *Select in model*.

**Behaviour & rules.** A dockable panel that lists graphs from configured folders. Each graph shows its marked inputs (number with unit, slider, drop-down, element picker, file picker, Boolean) and outputs. *Run* executes it with status and warnings. Users who do not edit graphs can run them. Graph Player shows the graph's required trust level, and refuses graphs that need a higher level than the user has granted.
- **Folders.** The user folder plus any folders in settings (for example a firm share); an administrator policy can add read-only folders. Subfolders appear as groups. Search filters by name and description.
- **Inputs.** Only nodes the author marked *Is input* are shown, in the author's order, with their names and tooltips. Values are validated against the node's type and range before Run is enabled; element pickers state the expected categories and count. Input values are remembered per user per graph (*Reset to defaults* restores them).
- **Running.** A run is one transaction named after the graph (or a group, if the graph asks), like `graph.run` (§2.15.1). Progress and *Cancel* are shown; cancelling rolls the run back. Automatic mode is not available in the Player; each run is explicit.
- **Outputs.** Nodes marked *Is output* show their values (numbers with units, text, lists truncated to 100 items, element lists with *Select in model*).
- **Errors.** A graph that fails to load (schema error, missing add-in, newer API) is listed with the reason and cannot be run. A run that fails shows the failing node's name and message, and the model is unchanged.

**Commands.** `graph.player.list(folder?) -> graph_summary[]` · (runs through `graph.run`)

**Acceptance criteria.**
- [ ] A graph with a length input and an element-picker input can be run from Graph Player without opening the graph editor.
- [ ] Entering a value outside the input's declared range disables Run and names the range.
- [ ] A graph that needs an add-in that is not installed is listed with "requires org.example.tools" and cannot be run.

### 2.15.3 Design-space exploration
**ID:** DM-VPL-003 · **Priority:** P2 · **Ref:** research/05 §13.6

**Purpose.** Generate and compare many design alternatives from a graph, then apply the chosen one. It is a local, open alternative to vendor cloud services.

**Workflow.**
1. In the graph editor, mark inputs as **variables** (with min, max and step, or a list of values) and outputs as **goals** (minimise or maximise) or **constraints** (bounds that an outcome must meet).
2. *Create Study*, choose a solver and a budget (number of outcomes), and *Generate*.
3. Explore the results; pick an outcome; *Apply to model*.

**Behaviour & rules.** A *study* is a graph with variable inputs (ranges) and goals (outputs to minimise or maximise) plus constraints. Solvers: *Randomise*, *Cross product*, *Optimise* (NSGA-II, implemented in Rust), *Like this* (local search around a chosen outcome). Outcomes run in parallel in **headless worker documents** (copies of the model in memory, §2.11.7). They are shown in a scatter plot and a parallel-coordinates chart, with thumbnails, and any outcome can be **applied** to the model as one transaction. This is a local, open alternative to vendor cloud services.
- **Budget and parallelism.** The number of parallel workers defaults to the number of physical cores minus one, limited by memory (each worker holds a model copy; the study estimates memory before starting and lowers parallelism if needed). *Cross product* refuses budgets larger than the product of the variable steps and says so.
- **Determinism.** Each study stores its random seed; re-running with the same seed, graph and model version gives the same outcomes.
- **Constraints.** Outcomes that break a constraint are kept but marked infeasible and excluded from the Pareto set.
- **Results.** Each outcome stores its variable values, goal values, constraint status, a thumbnail of a chosen view, and the run time. Results can be filtered by brushing axes, sorted, and exported as CSV. *Apply* re-runs the graph with that outcome's inputs on the live model (one transaction), so the applied result is real model content, not a copy of the worker's geometry.
- **Failure.** An outcome whose run fails is recorded with its error and does not stop the study. *Cancel* keeps the outcomes finished so far.
- **Storage.** Studies (definition, seed, outcomes) are saved next to the graph as `<graph>.study.json`; they are not model data. Thumbnails are stored in the same folder.

**Commands.** `study.create(graph_id, variables, goals) -> study_id` · `study.run(study_id, solver, budget) -> outcomes` · `study.apply(study_id, outcome_id)`

**Acceptance criteria.**
- [ ] An Optimise study over 2 variables and 2 goals with a budget of 200 runs returns a Pareto set, uses all cores, and applying an outcome reproduces its reported goal values.
- [ ] Running the same study twice with the same seed gives identical outcome tables.
- [ ] An outcome that breaks a constraint is shown as infeasible and is not in the Pareto set.

---

## 2.16 The MCP server

### 2.16.1 Overview
**ID:** DM-MCP-001 · **Priority:** P0 · **Ref:** research/05 §12.3 (AI assistant using MCP tool calls), §16.11; TECH_STACK §3.6

**Purpose.** Every Reviera operation is available to AI agents through the Model Context Protocol. This is achieved by construction, from the command registry.

**Behaviour & rules.**
- The MCP server is built into the application with the official Rust MCP SDK (`rmcp`). It follows the current published MCP specification (protocol version negotiated at initialisation). The supported revision is listed in the release notes.
- It exposes **tools** (generated from the registry, plus a few compound tools), **resources** (model data) and **prompts** (task templates). It supports **notifications** (resource updates, tool-list changes), **progress** and **cancellation**.
- The server is **off by default**. The user turns it on in Manage › AI Agents (or with a command-line flag) and chooses the transports and the permission level.
- Agents have no write path of their own. Their changes go through the same command queue, transactions, undo and warnings as user actions (§2.7).

### 2.16.2 Transports and authentication
**ID:** DM-MCP-002 · **Priority:** P0 (stdio) / P1 (streamable HTTP) · **Ref:** TECH_STACK §3.6

**Behaviour & rules.**
- **stdio.** `reviera --mcp-stdio [model.ifc]` runs Reviera headless (§2.11.7) as an MCP server on stdin/stdout, for local agents that start it as a sub-process. Trust follows the user who started it. The permission level comes from a command-line flag (default `read_only`).
- **Streamable HTTP** (when the UI is running). The server listens on `127.0.0.1` (default port 47800, configurable). Binding to other interfaces requires TLS and an explicit setting.
  - **Authentication**: bearer tokens created in Manage › AI Agents › *New client token*. Each token has a name, a permission level, an optional expiry and an optional document scope. Tokens are stored hashed. Remote deployments may use OAuth 2.1 authorisation as the MCP specification describes (P2).
  - The `Origin` header is checked to prevent DNS rebinding attacks. CORS is closed by default.
- **Status UI.** An *AI Agents* panel shows connected clients, their permission levels, active sessions, recent tool calls (from the journal), and a **Disconnect** / **Revoke** button for each one. A status-bar icon shows when an agent is connected, and pulses while an agent is running a tool.

**Commands.** `mcp.start(transport, options)` · `mcp.stop()` · `mcp.token.create(name, level, expiry?, document_scope?) -> token` · `mcp.token.revoke(token_id)` · `mcp.sessions() -> session[]` · `mcp.session.disconnect(session_id)`

**Acceptance criteria.**
- [ ] With the HTTP transport on, a request without a valid token gets HTTP 401, and a request from a disallowed `Origin` gets HTTP 403.
- [ ] Revoking a token disconnects its sessions within 1 s.
- [ ] Credentials (BIMserver passwords, tokens) never appear in any tool result, resource or log.

### 2.16.3 Tool catalogue: generation from the registry
**ID:** DM-MCP-003 · **Priority:** P0 · **Ref:** TECH_STACK §2, §3.6

**Behaviour & rules.**
- **One tool per command.** For each registered command (core and add-in, unless `mcp = false`):
  - tool **name** = the command name (for example `wall.create`). For clients that do not accept `.` in tool names, the setting *MCP tool name style* = `underscore` maps it to `wall_create`;
  - **description** = summary + description + key examples, plus the data types and units of numeric arguments;
  - **inputSchema** = the command's parameter JSON Schema. Arguments with a data type also accept unit strings (§2.12.1);
  - **outputSchema** = the result schema. Results are returned as structured content, plus a short text summary;
  - **annotations**, derived from `effect`: `readOnlyHint` (effect is only `read`), `destructiveHint` (`destructive` or `delete`), `idempotentHint` (declared per command), `openWorldHint` (`network`).
- **Toolsets.** Hundreds of tools overwhelm agents, so tools are grouped into **toolsets**, derived from command domains: `core` (query, inspect, select, param get/set, undo), `modeling`, `documentation`, `families`, `materials`, `project_setup`, `io`, `collaboration`, `analysis`, `scripting`, `admin`, and one toolset per add-in.
  - By default a session exposes `core` plus the toolsets chosen in the client token's configuration.
  - Agents can list and enable toolsets at runtime with `toolsets.list` / `toolsets.enable` / `toolsets.disable`. The server then sends `notifications/tools/list_changed`.
- **Discovery tools** (always available):
  - `find_commands(query, limit?)` does a semantic and keyword search over the entire registry, including disabled toolsets, and returns names, summaries and schemas;
  - `describe_command(name)`;
  - `run_command(name, arguments)` calls any command that the session's permission level allows, even when its toolset is not enabled.
- The tool list is generated at start-up, and regenerated when add-ins load or unload.

**Acceptance criteria.**
- [ ] For every registered command, `find_commands` can find it by its summary text, and `run_command` can execute it with the same result as the specific tool.
- [ ] Loading an add-in that registers 2 commands sends `tools/list_changed`, and the new tools appear with their schemas.
- [ ] Every tool marked `readOnlyHint=true` is in fact read-only (verified by a test that runs each read-only tool and checks that the model version is unchanged).

### 2.16.4 Compound and session tools
**ID:** DM-MCP-004 · **Priority:** P0 (query, inspect, undo) / P1 (others) · **Ref:** TECH_STACK §3.6

| Tool | Purpose | Permission |
|---|---|---|
| `query_elements(spec)` | The collector query in JSON form (§2.13.3), with `fields` to return chosen parameters, plus paging | read_only |
| `get_element(id_or_guid, fields?)` | The element summary: category, type, level, host, all parameters with display values, bounding box | read_only |
| `get_selection()` / `set_selection(ids)` | The user's current selection | read_only / model_edit |
| `get_view_image(view_id, width, height, options)` | A PNG render of a view (returns image content), so an agent can see what it did | read_only |
| `execute_lua(source, args?)` | Runs Lua for compound operations, at the trust level allowed for the session (level 0 for read_only, level 1 for model_edit, as configured for full_scripting). The result is the returned value (as JSON) plus captured log output. | full_scripting (level 0 version: read_only) |
| `batch(calls[], atomic=true)` | Runs several commands in order. With `atomic`, they run in one transaction, and are all rolled back if any fails. | model_edit |
| `session.begin_group(name)` / `session.end_group(mode)` | A transaction group around several tool calls, so that one undo reverts an agent's multi-step task (§2.7.1) | model_edit |
| `undo(steps?)` / `redo(steps?)` / `history()` | Limited to the session's own items, unless the level is full_scripting | model_edit |
| `preview(command, arguments)` | Runs a command in a transaction that is then rolled back. Returns the change set, the warnings and, optionally, a view image. | model_edit |
| `confirm(preview_token)` | In *confirm mode*, destructive commands return a preview token instead of running. The user (in the AI Agents panel), or the agent if the policy allows, confirms it. | model_edit |
| `toolsets.list` / `toolsets.enable` / `toolsets.disable` | See §2.16.3 | read_only |
| `find_commands`, `describe_command`, `run_command` | See §2.16.3 | (as the target command) |

**Acceptance criteria.**
- [ ] An agent creates 4 walls and a floor inside `session.begin_group("Create room")` … `end_group("assimilate")`. The UI shows one undo item named "Create room (agent: <client>)", and one undo removes everything.
- [ ] `preview("element.delete", …)` returns the dependents that would be deleted, and leaves the model unchanged (same model version).
- [ ] `get_view_image` returns a PNG that matches the view's current state after the agent's last transaction.

### 2.16.5 Permissions and safety
**ID:** DM-MCP-005 · **Priority:** P0 · **Ref:** TECH_STACK §3.6

**Behaviour & rules.**
- **Permission levels** (per token or stdio session):

| Level | Allowed command effects | Lua (`execute_lua`) |
|---|---|---|
| `read_only` | `read` | Trust level 0 |
| `model_edit` | `read`, `edit`, `settings`, `ui` (selection, view activation). `file_write` limited to export into a configured output folder. | Trust level 0 |
| `full_scripting` | All, including `file_read`/`file_write` anywhere, `network`, `script`, `destructive` without confirmation (unless confirm mode is forced) | Up to the level set in the token (default 1; never `native` unless the user enables it explicitly) |

- **Confirm mode** (default on for `model_edit`): commands with effect `destructive` (delete, purge, schema conversion, overwrite on load, BIMserver check-in) need confirmation (§2.16.4).
- **Attribution.** Every transaction made by an agent records `origin = {source: "mcp", actor: <client name>, session_id}`. It appears in the undo list, the journal and, in BIMserver mode, in the check-in comment and element change stamps (RXD element data `identity.last_modified_by`).
- **Limits.** Per-session rate limit (default 20 write calls per second). A maximum result size, with paging for queries. A maximum wall time per call (default 120 s; long operations report progress and support cancellation).
- **Isolation.** A session can only act on the documents in its token's scope. File paths are checked against allowed folders. Agents cannot read or change the MCP configuration, tokens or trust store.
- **Human visibility.** The AI Agents panel can show a running feed of tool calls. *Pause agents* holds the command queue for agent calls (they wait or time out) until resumed.

**Acceptance criteria.**
- [ ] A `read_only` session calling `wall.create` gets an error `permission.denied` with the required level, and the model is unchanged.
- [ ] With confirm mode on, `element.delete` from a `model_edit` session returns a preview token, and nothing is deleted until it is confirmed.
- [ ] After *Pause agents*, a pending agent write is not executed until *Resume*.

### 2.16.6 Resource catalogue
**ID:** DM-MCP-006 · **Priority:** P0 (element, type, view, project, categories, commands) / P1 (others) · **Ref:** TECH_STACK §3.6

**Behaviour & rules.** Resources are read-only JSON (or other stated MIME types). Resource templates use RFC 6570 URI templates. `{doc}` defaults to the active document when omitted (`reviera://element/{id}` = `reviera://doc/active/element/{id}`).

| URI template | MIME | Content |
|---|---|---|
| `reviera://documents` | application/json | Open documents (ID, title, path or BIMserver project, kind, schema, mode) |
| `reviera://doc/{doc}/project` | application/json | Project information, units summary, location, phases, current phase |
| `reviera://doc/{doc}/element/{id_or_guid}` | application/json | Element summary (as `get_element`) |
| `reviera://doc/{doc}/element/{id_or_guid}/geometry{?detail,format}` | model/gltf-binary or application/json | The element's geometry (glTF, or a JSON B-rep summary) |
| `reviera://doc/{doc}/type/{id_or_guid}` | application/json | Type with parameters and instance count |
| `reviera://doc/{doc}/family/{id_or_guid}` | application/json | Family summary, types and parameters (definitions, with formulas when allowed) |
| `reviera://doc/{doc}/categories` | application/json | Categories used in the model, with counts and subcategories |
| `reviera://doc/{doc}/levels`, `…/grids` | application/json | Datums with elevations and positions |
| `reviera://doc/{doc}/views`, `…/view/{id}` | application/json | View list, or one view's definition (type, scale, level, template, crop, filters) |
| `reviera://doc/{doc}/view/{id}/image{?w,h}` | image/png | Rendered view image |
| `reviera://doc/{doc}/sheets`, `…/sheet/{id}` | application/json | Sheets and placed viewports |
| `reviera://doc/{doc}/schedule/{id}{?format}` | application/json or text/csv | Schedule data |
| `reviera://doc/{doc}/parameters` | application/json | Parameter catalogue: built-in, project, shared and global definitions with bindings |
| `reviera://doc/{doc}/materials` | application/json | Materials and assets (summaries) |
| `reviera://doc/{doc}/warnings` | application/json | Stored warnings |
| `reviera://doc/{doc}/selection` | application/json | Current selection (subscribable) |
| `reviera://doc/{doc}/history` | application/json | Undo history with origins |
| `reviera://commands`, `reviera://command/{name}` | application/json | The registry (summaries), or one definition with its schemas |
| `reviera://units` | application/json | Data types and unit catalogue (§2.9.4, §2.9.5) |
| `reviera://docs/{topic}` | text/markdown | Built-in documentation pages (API reference, how-tos) |

- **Subscriptions.** Clients can subscribe to `element/{id}`, `selection`, `warnings` and `history`. The server sends `notifications/resources/updated` after commits that affect them (coalesced, at most 5 per second per resource). `notifications/resources/list_changed` is sent when documents open or close.
- Resources never include credentials. They include RXD content only as summarised fields, never raw RXD carrier data.

**Acceptance criteria.**
- [ ] Reading `reviera://element/{guid}` for a door returns its category, type, host wall GUID, level and all parameters with display values in project units.
- [ ] A client subscribed to `selection` receives an update within 200 ms of the user changing the selection.

### 2.16.7 Prompts
**ID:** DM-MCP-007 · **Priority:** P2 · **Ref:** research/05 §12.3

**Purpose.** Give agents ready-made, well-tested task templates, so common jobs use the right tools in the right order.

**Behaviour & rules.** The server offers MCP prompts as task templates, for example: *Audit model* (warnings, unplaced views, missing parameters); *Prepare IFC export* (check against an IDS); *Create sheets from views*; *Explain this element*; *Colour views by parameter*. Prompts are generated from documented Lua and graph templates. Add-ins can contribute prompts in their manifest.
- **Definition.** A prompt has a name, title, description, typed arguments (with descriptions, required flags and allowed values) and a message template. Templates may embed resources (§2.16.6), for example the current selection or the warnings list, which are resolved when `prompts/get` is called.
- **Tool references.** A prompt names the tools it expects the agent to use. At start-up, and when add-ins load, every prompt is checked against the current tool catalogue; a prompt that refers to a missing tool is withdrawn from `prompts/list` and logged. `notifications/prompts/list_changed` is sent when the list changes.
- **Permissions.** A prompt declares the permission level its tools need; `prompts/list` for a session shows only prompts the session's level can complete.
- **Add-in prompts** are declared in the manifest (`[[prompts]]` with `name`, `file` and `arguments`) and are namespaced `<addin-id>/<name>`.
- **Localisation.** Prompt titles and descriptions follow the application language; message templates are written in English unless the add-in supplies translations.

**Commands.** (protocol-level, `prompts/list` and `prompts/get`)

**Acceptance criteria.**
- [ ] `prompts/list` returns the built-in prompts with argument descriptions, and `prompts/get("Audit model")` returns a message sequence that refers only to existing tools.
- [ ] A `read_only` session does not see *Create sheets from views* in `prompts/list`.
- [ ] Disabling an add-in that contributed a prompt removes it from `prompts/list` and sends `list_changed`.

---

## Appendix A — Reference evolution (release history and version notes)

**Ref:** research/05 §14, §15, §13.1, §11.2 · screenshots/platform/wall_tool_ribbon_no_options_bar_2027.png, screenshots/platform/autodesk_assistant_2027.png

The reference product releases once a year, each spring, with point updates during the year. Its file format changes with each yearly release, and older versions cannot read newer files. The table summarises what each release added in the areas this chapter covers (and some others), and what that means for Reviera's priorities. This summary is informative. It guides priorities, and is not a feature list to copy.

| Reference release | Notable additions (summarised) | Implication for Reviera |
|---|---|---|
| 2020 (Apr 2019) | PDF and image underlays with snapping; elliptical walls; egress path analysis; double fill patterns; multiline text and image parameters; visual programming installed by default; internal origin made visible | Double fill patterns, image and multiline parameters, and PDF underlay snapping are now baseline expectations (DM-PAT-001, DM-PARAM-004, DM-IMP-004). Internal origin, base point and survey point are P0 (DM-LOC-001). |
| 2021 (Apr 2020) | Slanted walls; generative design studies; real-time realistic views; 3D rebar; systems analysis; tags rotating with elements | Design-space exploration is P2 and runs locally (DM-VPL-003). |
| 2022 (Apr 2021) | Native vector PDF export; tapered walls; the new unit API (versioned spec and unit IDs) replacing old enumerations; nested AND/OR filter rules; coordination-model link (a clash-review format); Rhino import | PDF export is P0 (DOC). Versioned data-type and unit IDs are adopted from the start (DM-PARAM-004, DM-UNIT-001), avoiding a later API break. |
| 2023 (Apr 2022) | New independent structural analytical model; cloud parameter service (preview, then GA in 2023.1); duplicate family parameters; filled regions in 3D families; IFC element mapping; Infrastructure units discipline | The parameter library with URL/git distribution (DM-SPAR-001) is the open counterpart of the cloud parameter service. Duplicate parameters and the Infrastructure discipline are included. |
| 2024 (Apr 2023) | Toposolid; dark theme; Project Browser search; one simplified default template; 2D draw order in 3D families; relocatable collaboration cache | Draw order in families (DM-FED-007), and a small set of shipped templates (DM-TMPL-001). |
| 2025 (Apr 2024) | Move to a new .NET runtime (add-ins rebuilt); sheet collections; family arrays of 0 or 1 elements; mullion profiles with several loops; macros edited in an external code editor; bulk export of several formats | Arrays with count 0/1 (DM-FED-004), multi-loop profiles (DM-FSPEC-003), an external editor and language server for scripts (DM-LUA-008), and bulk export (DM-EXP-013). A stable, semver-versioned Lua API (DM-LUA-011) avoids forced rebuilds. |
| 2026 (Apr 2025) | GPU-accelerated graphics (preview); shared parameters on view markers; title-block "As indicated" override; sheet-collection parameters; compound structures without a core; add-in dependency isolation and add-in manager settings; built-in add-in manager UI | Per-add-in environments and an Add-in Manager from v1 (DM-LUA-001, DM-ADDIN-003). Cores are optional in layered types (§2.3.8). |
| 2027 (7 Apr 2026) | Rule-based numbering; built-in AI assistant (preview) that calls MCP tools; Options Bar removed (options moved into the contextual ribbon and Properties); walls hosted on walls; multi-category tags as defaults; tag leader control; carbon asset on materials; IFC property-mapping dialog; extended properties from external data; new .NET runtime; Windows 11 only | Confirms the direction of several Reviera design choices: MCP as a first-class interface (§2.16, P0), no Options Bar (UI chapter), a carbon asset (DM-MAT-005), a property-mapping UI (DM-FILE-004), and walls as hosts (§2.6.1). Extended properties get an open provider API (DM-PARAM-005). |
| 2027.1 (Jun 2026) | Rotatable custom fill patterns; AI assistant reliability (MCP tool calls logged in journals); ambient shadows | Pattern rotation (DM-PAT-001); every MCP call is logged in the session journal (DM-TXN-003, DM-MCP-005). |
| 2027.2 (Jul 2026) | Tag by example; view-title snapping; incremental model open option | Partial and incremental open (DM-FILE-001, P1/P2). |

**Version and platform notes that shape Reviera:**
1. **One-way upgrades.** Each yearly release changes the native format. There is no "save as previous version", and families must be upgraded too. → Reviera uses IFC plus a semver-versioned RXD, with a documented compatibility promise and save-to-previous (DM-MIG-001).
2. **Mostly single-threaded regeneration** (the reference's own hardware guidance favours high single-core clock speed). → Parallel regeneration is a core differentiator (DM-REGEN-003).
3. **Runtime churn for extensions.** The reference changed .NET runtimes in 2025 and again in 2027, so add-ins had to be rebuilt each time. → The Lua API is versioned separately from the application (DM-LUA-011). Lua add-ins need no compilation.
4. **Late add-in isolation and security moves.** Dependency isolation arrived in 2026, and all-user add-ins moved to a protected program folder in 2027. → Reviera has isolated environments, admin-only system add-in folders, trust levels and signing from v1 (DM-LUA-007, DM-ADDIN-002).
5. **Windows-only, Windows 11 only from 2027.** → Reviera targets Windows 10 and later, macOS 10.12 and later, and current Linux distributions through Flatpak (TECH_STACK §5.4).
6. **Product tiers.** The reference's lower-cost tier lacks the API, macros, visual programming, the conceptual massing environment and IFC import. → Reviera has no feature tiers. Everything in this chapter is available to every user.
7. **Vendor cloud services** (cloud parameters, cloud worksharing, content libraries, issue tracking, AI assistant service) → P3. Open alternatives are noted where they exist: parameter libraries over git/HTTPS (DM-SPAR-001), BIMserver (COL), BCF (DM-IMP-011), and MCP with any agent (§2.16).

---

## Command catalog

All commands defined in this chapter. Every command in this table is also a Lua function (`reviera.<name>`) and an MCP tool (§2.16.3), unless noted. "Undoable = No" covers read-only, file, preference and session operations. `txn.*` are transaction primitives used by Lua and MCP, not stand-alone tools.

| Command | Parameters | Returns | Undoable | Req ID |
|---|---|---|---|---|
| `subcategory.create` | parent_category, name | element_id | Yes | DM-CATG-001 |
| `subcategory.rename` | subcategory_id, name | — | Yes | DM-CATG-001 |
| `subcategory.delete` | subcategory_id, reassign_to? | — | Yes | DM-CATG-001 |
| `family.load` | path, type_names?, overwrite_values=false | family_id | Yes | DM-FAM-001 |
| `family.reload` | family_id, path?, overwrite_values=false | — | Yes | DM-FAM-001 |
| `family.save_as` | family_id, path | — | No | DM-FAM-001 |
| `family.edit` | family_id | family_document_id | No | DM-FAM-001 |
| `family.rename` | family_id, name | — | Yes | DM-FAM-001 |
| `family.delete` | family_id | — | Yes | DM-FAM-001 |
| `family.create_in_place` | category, name | family_document_id | Yes | DM-FAM-001 |
| `type.duplicate` | type_id, new_name | type_id | Yes | DM-TYPE-001 |
| `type.rename` | type_id, name | () | Yes | DM-TYPE-001 |
| `type.delete` | type_id, delete_instances=false | — | Yes | DM-TYPE-001 |
| `type.set_param` | type_id, param, value | () | Yes | DM-TYPE-001 |
| `type.edit_properties` | type_id? | — | No | DM-TYPE-001 |
| `instance.change_type` | element_ids[], type_id | — | Yes | DM-TYPE-001 |
| `instance.place` | type_id, placement, host_id?, level_id?, params? | element_id | Yes | DM-INST-001 |
| `instance.place_adaptive` | type_id, points[], hosts?[] | element_id | Yes | DM-INST-001 |
| `instance.flip` | element_id, axis: "facing"\|"hand"\|"work_plane" | — | Yes | DM-INST-001 |
| `instance.pick_new_host` | element_id, host_id, face_ref? | — | Yes | DM-INST-001 |
| `pset.set` | element_id, pset_name, property_name, value | — | Yes | DM-IFC-003 |
| `pset.remove` | element_id, pset_name, property_name? | — | Yes | DM-IFC-003 |
| `qto.recompute` | element_ids?[] | — | Yes | DM-IFC-003 |
| `site.create` | name | element_id | Yes | DM-IFC-004 |
| `building.create` | site_id, name | element_id | Yes | DM-IFC-004 |
| `spatial.reassign` | element_ids[], container_id | — | Yes | DM-IFC-004 |
| `rxd.status` | — | {present, carrier, schema_version, orphans, mismatches, invalid_documents} | No | DM-RXD-003 |
| `rxd.reconcile` | mode: report\|apply, drop_orphans?=true | report | Yes | DM-RXD-003 |
| `rxd.attach_sidecar` | path | — | Yes | DM-RXD-003 |
| `rxd.export_sidecar` | path | — | No | DM-RXD-003 |
| `element.get_ids` | element_ids[] | [{id, guid}] | No | DM-ID-001 |
| `element.select_by_id` | ids_or_guids[] | element_id[] | No | DM-ID-001 |
| `element.find_by_guid` | guid | element_id? | No | DM-ID-001 |
| `model.version` | — | {episode, counter} | No | DM-ID-002 |
| `element.changed_since` | version | {changed[], deleted[], stale_episode} | No | DM-ID-002 |
| `element.dependents` | element_ids[] | {element_id, action}[] | No | DM-HOST-001 |
| `element.delete` | element_ids[], confirm_dependents?=true | element_ids[] (deleted, dependents included) | Yes | DM-HOST-001 |
| `host.rehost` | element_id, host_id, reference? | — | Yes | DM-HOST-001 |
| `regen.explain` | element_id, facet? | {upstream[], downstream[]} | No | DM-REGEN-001 |
| `regen.now` | — | () | No (runs inside the open transaction) | DM-REGEN-003 |
| `txn.begin` | name | — | — (primitive) | DM-TXN-001 |
| `txn.commit` | — | — | — (primitive) | DM-TXN-001 |
| `txn.rollback` | — | — | — (primitive) | DM-TXN-001 |
| `txn.savepoint` | — | sp | — (primitive) | DM-TXN-001 |
| `txn.rollback_to` | sp | — | — (primitive) | DM-TXN-001 |
| `txn.group_begin` | name | — | — (primitive) | DM-TXN-001 |
| `txn.group_end` | mode: "assimilate"\|"commit"\|"rollback" | — | — (primitive) | DM-TXN-001 |
| `edit.undo` | steps=1 | — | — (undo operation) | DM-TXN-002 |
| `edit.redo` | steps=1 | — | — (undo operation) | DM-TXN-002 |
| `edit.history` | — | [{name, origin, time}] | No | DM-TXN-002 |
| `journal.replay` | path, stop_on_error=true | — | Yes (one item per entry) | DM-TXN-003 |
| `warnings.list` | filter?: {kind?, severity?, element_ids?} | warning[] | No (query) | DM-FAIL-001 |
| `warnings.show` | warning_id | — | No | DM-FAIL-001 |
| `warnings.dismiss` | warning_ids[] | — | Yes | DM-FAIL-001 |
| `warnings.export` | path, format: html\|csv | () | No | DM-FAIL-001 |
| `failure.set_policy` | policy | — | No | DM-FAIL-001 |
| `family_doc.new` | template_path | document_id | No | DM-FED-000 |
| `family_doc.open` | path | document_id | No | DM-FED-000 |
| `family_doc.save` | document_id, path? | — | No | DM-FED-000 |
| `family_doc.load_into` | document_id, target_document_ids[], overwrite_values=false | — | Yes | DM-FED-000 |
| `family_template.list` | locale? | template[] | No | DM-FED-001 |
| `family_doc.save_as_template` | document_id, path, keep_geometry=false | — | No | DM-FED-001 |
| `family_doc.set_category` | category | — | Yes | DM-FED-002 |
| `family_doc.set_flag` | flag, value | — | Yes | DM-FED-002 |
| `family_doc.category_params` | — | — | No | DM-FED-002 |
| `family_doc.reference_plane.create` | p1, p2, view_id, name?, is_reference="weak" | element_id | Yes | DM-FED-003 |
| `family_doc.reference_line.create` | p1, p2, view_id? | element_id | Yes | DM-FED-003 |
| `family_doc.reference.set` | element_id, name?, is_reference?, defines_origin?, wall_closure? | — | Yes | DM-FED-003 |
| `reference.resolve` | stable_ref | {element_id, geometry} | No | DM-FED-003 |
| `family_doc.form.extrusion` | sketch, start, end, solid=true | element_id | Yes | DM-FED-004 |
| `family_doc.form.blend` | base_sketch, top_sketch, first_end, second_end, vertex_map?, solid=true | element_id | Yes | DM-FED-004 |
| `family_doc.form.revolve` | sketch, axis, start_angle, end_angle, solid=true | element_id | Yes | DM-FED-004 |
| `family_doc.form.sweep` | path, profile, profile_options?, solid=true | element_id | Yes | DM-FED-004 |
| `family_doc.form.swept_blend` | path, profile_start, profile_end, options?, solid=true | element_id | Yes | DM-FED-004 |
| `family_doc.form.set` | element_id, props | — | Yes | DM-FED-004 |
| `family_doc.shape_points.toggle` | — | () | No (view state) | DM-FED-004 |
| `family_doc.geometry.cut` | cutting_id, target_id | — | Yes | DM-FED-004 |
| `family_doc.geometry.uncut` | cutting_id, target_id | — | Yes | DM-FED-004 |
| `family_doc.geometry.join` | a_id, b_id | — | Yes | DM-FED-004 |
| `family_doc.geometry.unjoin` | a_id, b_id | — | Yes | DM-FED-004 |
| `family_doc.array.create` | element_ids[], kind: "linear"\|"radial", count, spacing_or_angle, count_param? | element_id | Yes | DM-FED-004 |
| `family_doc.model_text.create` | text, position, font, size, depth | element_id | Yes | DM-FED-004 |
| `family_doc.control.create` | position, direction: "single_h"\|"double_h"\|"single_v"\|"double_v" | element_id | Yes | DM-FED-004 |
| `family_doc.dimension.create` | refs[], kind, view_id, position | element_id | Yes | DM-FED-005 |
| `family_doc.dimension.label` | dimension_id, param_name \| new_param_def | — | Yes | DM-FED-005 |
| `family_doc.dimension.set_eq` | dimension_id, on | — | Yes | DM-FED-005 |
| `family_doc.constraint.lock` | dimension_or_alignment_id, on | — | Yes | DM-FED-005 |
| `family_doc.align` | target_ref, moving_ref, lock=false | — | Yes | DM-FED-005 |
| `family_doc.flex_test` | random_samples=0 | report | No | DM-FED-005 |
| `family_doc.param.add` | def: {name, data_type, group, is_instance, shared_guid?, reporting?, tooltip?, min?, max?, ifc_home?} | param_id | Yes | DM-FED-006 |
| `family_doc.param.edit` | param_id, changes | — | Yes | DM-FED-006 |
| `family_doc.param.delete` | param_id | — | Yes | DM-FED-006 |
| `family_doc.param.duplicate` | param_id | param_id | Yes | DM-FED-006 |
| `family_doc.param.to_shared` | param_id, library_guid | — | Yes | DM-FED-006 |
| `family_doc.param.to_family` | param_id | — | Yes | DM-FED-006 |
| `family_doc.param.set_formula` | param_id, formula \| null | — | Yes | DM-FED-006 |
| `family_doc.type.create` | name | type_id | Yes | DM-FED-006 |
| `family_doc.type.rename` | type_id, name | — | Yes | DM-FED-006 |
| `family_doc.type.delete` | type_id | — | Yes | DM-FED-006 |
| `family_doc.type.set_value` | type_id, param_id, value | — | Yes | DM-FED-006 |
| `family_doc.type.set_current` | type_id | — | Yes | DM-FED-006 |
| `family_doc.types` | — | — | No | DM-FED-006 |
| `formula.validate` | context_id, param_id \| null, text | {ok, type, dimension, errors[], lints[]} | No | DM-FRM-001 |
| `formula.evaluate` | context_id, text | value | No | DM-FRM-001 |
| `family_doc.lookup_table.import` | path, name? | table_id | Yes | DM-LUT-001 |
| `family_doc.lookup_table.export` | table_id, path | — | Yes | DM-LUT-001 |
| `family_doc.lookup_table.rename` | table_id, name | — | Yes | DM-LUT-001 |
| `family_doc.lookup_table.delete` | table_id | — | Yes | DM-LUT-001 |
| `family_doc.lookup_table.set_cell` | table_id, row, column, value | — | Yes | DM-LUT-001 |
| `family.list_types` | path | [{type_name, values}] | No | DM-TCAT-001 |
| `family.export_types` | family_id, path | — | No | DM-TCAT-001 |
| `family.import_types` | family_id?, path | type_id[] | Yes | DM-TCAT-001 |
| `family_doc.component.place` | family_id, type_id, placement | element_id | Yes | DM-NEST-001 |
| `family_doc.param.associate` | nested_element_id, child_param, parent_param_id \| null | — | Yes | DM-NEST-001 |
| `family_doc.visibility.set` | element_ids[], {coarse, medium, fine, plan, front_back, left_right, only_when_cut, visible_param?} | — | Yes | DM-FED-007 |
| `family_doc.draw_order` | element_ids[], "front"\|"back"\|"forward"\|"backward" | — | Yes | DM-FED-007 |
| `family_doc.preview_visibility.toggle` | view_id? | () | No (view state) | DM-FED-007 |
| `family_doc.label.create` | position, params[{param, prefix?, suffix?, sample?, spaces?, break?}], text_type_id | element_id | Yes | DM-FSPEC-001 |
| `family_doc.label.edit` | label_id, params[] | — | Yes | DM-FSPEC-001 |
| `family_doc.connector.create` | kind, face_ref \| work_plane, props | element_id | Yes | DM-FSPEC-005 |
| `family_doc.connector.set` | connector_id, props | — | Yes | DM-FSPEC-005 |
| `cme.form.create` | curve_refs[], solid=true, choice? | element_id | Yes | DM-CME-001 |
| `cme.form.push_pull` | subelement_ref, vector | — | Yes | DM-CME-001 |
| `cme.form.add_edge` | face_ref, p1, p2 | — | Yes | DM-CME-001 |
| `cme.form.add_profile` | form_id, parameter | — | Yes | DM-CME-001 |
| `cme.form.dissolve` | form_id | — | Yes | DM-CME-001 |
| `cme.divide_surface` | face_ref, {u, v} \| {references[]} | element_id | Yes | DM-CME-002 |
| `cme.divide_path` | curve_refs[], spec | element_id | Yes | DM-CME-002 |
| `cme.surface.set_pattern` | divided_surface_id, pattern, border_tiles? | — | Yes | DM-CME-003 |
| `cme.surface.apply_component` | divided_surface_id, panel_type_id | — | Yes | DM-CME-003 |
| `cme.point.create` | position \| host_ref, adaptive=false | element_id | Yes | DM-CME-004 |
| `cme.point.make_adaptive` | point_id, index? | — | Yes | DM-CME-004 |
| `cme.repeat` | adaptive_instance_id | element_id | Yes | DM-CME-004 |
| `family.package.inspect` | path | {families[], types[], has_rxd, version, licence} | No | DM-FFMT-001 |
| `library.browse` | query?, source? | — | No | DM-LIB-001 |
| `library.search` | text?, filters?, source?, limit=100 | library_item[] | No | DM-LIB-001 |
| `library.source.add` | path_or_url, name? | source_id | No | DM-LIB-001 |
| `library.source.remove` | source_id | — | No | DM-LIB-001 |
| `library.refresh` | source_id? | — | No | DM-LIB-001 |
| `library.load` | item_ref, type_names?, place=false | family_id | Yes | DM-LIB-001 |
| `param.definition` | param_ref | definition | No | DM-PARAM-002 |
| `param.list` | category?, element_id? | definition[] | No | DM-PARAM-002 |
| `param.set_ifc_home` | param_ref, category?, pset_name, property_name | — | Yes | DM-PARAM-002 |
| `param.project.add` | def, categories[], is_instance, group_behavior: "aligned"\|"vary", pset_name? | param_id | Yes | DM-PARAM-003 |
| `param.project.modify` | param_id, changes | — | Yes | DM-PARAM-003 |
| `param.project.remove` | param_id | — | Yes | DM-PARAM-003 |
| `param.project.manage` | — | — | No | DM-PARAM-003 |
| `param.get` | element_id, param_ref | value | No | DM-PARAM-003 |
| `param.set` | element_ids[], param_ref, value | — | Yes | DM-PARAM-003 |
| `param.set_many` | changes: [{element_id, param_ref, value}] | — | Yes | DM-PARAM-003 |
| `param.clear` | element_ids[], param_ref | — | Yes | DM-PARAM-003 |
| `units.get` | — | unit_settings | No | DM-UNIT-001 |
| `units.set_format` | data_type, format | — | Yes | DM-UNIT-001 |
| `units.set_preset` | preset | — | Yes | DM-UNIT-001 |
| `units.format` | value, data_type, format? | text | No | DM-UNIT-001 |
| `units.parse` | text, data_type | value | No | DM-UNIT-001 |
| `units.edit` | — | — | No | DM-UNIT-001 |
| `global.create` | name, data_type, value \| formula, group?, reporting=false | global_id | Yes | DM-GPAR-001 |
| `global.set` | global_id, value \| {formula} | — | Yes | DM-GPAR-001 |
| `global.delete` | global_id | — | Yes | DM-GPAR-001 |
| `global.associate` | global_id, element_ids[], param_ref | — | Yes | DM-GPAR-001 |
| `global.dissociate` | element_ids[], param_ref | — | Yes | DM-GPAR-001 |
| `global.label_dimension` | global_id, dimension_id | — | Yes | DM-GPAR-001 |
| `global.list` | — | global[] | No | DM-GPAR-001 |
| `global.select_driven` | global_id | element_id[] | Yes | DM-GPAR-001 |
| `global.manage` | — | — | No | DM-GPAR-001 |
| `paramlib.open` | path_or_url | library_id | No (library file) | DM-SPAR-001 |
| `paramlib.create` | path, name | library_id | No (library file) | DM-SPAR-001 |
| `paramlib.close` | library_id | — | No (library file) | DM-SPAR-001 |
| `paramlib.definition.add` | library_id, def | guid | No (library file) | DM-SPAR-001 |
| `paramlib.definition.edit` | library_id, guid, changes | — | No (library file) | DM-SPAR-001 |
| `paramlib.definition.deprecate` | library_id, guid, superseded_by? | — | No (library file) | DM-SPAR-001 |
| `paramlib.group.add` | library_id, name | group_id | No (library file) | DM-SPAR-001 |
| `paramlib.group.rename` | library_id, group_id, name | — | No (library file) | DM-SPAR-001 |
| `paramlib.import_legacy` | path, target_library_id | — | No (library file) | DM-SPAR-001 |
| `paramlib.export_ifc_templates` | library_id, path | — | No (library file) | DM-SPAR-001 |
| `paramlib.search` | text?, labels?, data_type? | definition[] | No (library file) | DM-SPAR-001 |
| `paramlib.manage` | — | — | No | DM-SPAR-001 |
| `external_props.list_providers` | — | provider[] | No | DM-PARAM-005 |
| `external_props.materialise` | provider, element_ids[], pset_name | — | Yes | DM-PARAM-005 |
| `project.info.get` | — | info | No | DM-SET-002 |
| `project.info.set` | fields | — | Yes | DM-SET-002 |
| `project.info.edit` | — | — | No | DM-SET-002 |
| `object_style.set` | category_or_subcategory, {projection_weight?, cut_weight?, color?, pattern?, material?} | — | Yes | DM-OBJS-001 |
| `object_style.get` | category_or_subcategory | style | No | DM-OBJS-001 |
| `line_weight.set` | kind: "model"\|"perspective"\|"annotation", pen, scale?, thickness_mm | — | Yes | DM-LINE-001 |
| `line_weight.add_scale` | scale | — | Yes | DM-LINE-001 |
| `line_weight.remove_scale` | scale | — | Yes | DM-LINE-001 |
| `line_pattern.create` | name, segments[] | id | Yes | DM-LINE-001 |
| `line_pattern.edit` | id, segments[] | — | Yes | DM-LINE-001 |
| `line_pattern.delete` | id, replace_with? | — | Yes | DM-LINE-001 |
| `line_style.create` | name, weight, color, pattern | id | Yes | DM-LINE-001 |
| `line_style.edit` | id, props | — | Yes | DM-LINE-001 |
| `line_style.delete` | id, replace_with? | — | Yes | DM-LINE-001 |
| `fill_pattern.create` | name, kind: "drafting"\|"model", definition | id | Yes | DM-LINE-001 |
| `fill_pattern.edit` | id, definition | — | Yes | DM-LINE-001 |
| `fill_pattern.delete` | id, replace_with? | — | Yes | DM-LINE-001 |
| `fill_pattern.import_pat` | path, names?[], kind | id[] | Yes | DM-LINE-001 |
| `fill_pattern.export_pat` | ids[], path | — | Yes | DM-LINE-001 |
| `material.create` | name, class?, from_library? | material_id | Yes | DM-MAT-006 |
| `material.duplicate` | material_id, name, share_assets=true | material_id | Yes | DM-MAT-006 |
| `material.rename` | material_id, name | — | Yes | DM-MAT-006 |
| `material.delete` | material_id, replace_with? | — | Yes | DM-MAT-006 |
| `material.set` | material_id, {identity?, graphics?, appearance?, physical?, thermal?, carbon?} | — | Yes | DM-MAT-006 |
| `material.asset.duplicate` | material_id, asset_kind | asset_id | Yes | DM-MAT-007 |
| `material.asset.replace` | material_id, asset_kind, asset_id | — | Yes | DM-MAT-007 |
| `material.library.open` | path | library_id | No | DM-MAT-006 |
| `material.library.create` | path, name | library_id | No | DM-MAT-006 |
| `material.library.add_to_project` | library_id, material_names[] | material_id[] | Yes | DM-MAT-006 |
| `material.library.add_from_project` | library_id, material_ids[] | — | No | DM-MAT-006 |
| `material.list` | filter? | material[] | No | DM-MAT-006 |
| `material.browser` | material_id? | — | No | DM-MAT-006 |
| `material.assets` | material_id? | — | No | DM-MAT-007 |
| `settings.temporary_dimensions.set` | walls, doors_windows | — | Yes | DM-SET-003 |
| `settings.detail_level_by_scale.set` | table | — | Yes | DM-SET-003 |
| `settings.halftone.set` | props | — | Yes | DM-SET-003 |
| `settings.area_volume.set` | props | — | Yes | DM-SET-003 |
| `settings.keynoting.set` | path, numbering | — | Yes | DM-SET-003 |
| `location.set` | lat, lon, elevation?, time_zone?, dst? | — | Yes | DM-LOC-001 |
| `location.search` | address | candidates[] | No | DM-LOC-001 |
| `location.weather.set` | props \| {station_id} | — | Yes | DM-LOC-001 |
| `site.position.create` | name | id | Yes | DM-LOC-001 |
| `site.position.set_current` | id | — | Yes | DM-LOC-001 |
| `site.position.rename` | id, name | — | Yes | DM-LOC-001 |
| `site.position.delete` | id | — | Yes | DM-LOC-001 |
| `coordinates.set_survey_point` | point, shared_coords | — | Yes | DM-LOC-001 |
| `coordinates.set_project_base_point` | point, angle_to_true_north? | — | Yes | DM-LOC-001 |
| `coordinates.set_crs` | epsg_code | — | Yes | DM-LOC-001 |
| `coordinates.acquire` | link_instance_id | — | Yes | DM-LOC-001 |
| `coordinates.publish` | link_instance_id, site_position_id | — | Yes | DM-LOC-001 |
| `coordinates.report` | point | {internal, project, shared, lat_lon?} | No | DM-LOC-001 |
| `position.relocate` | translation, rotation? | — | Yes | DM-LOC-001 |
| `position.rotate_true_north` | angle | — | Yes | DM-LOC-001 |
| `position.rotate_project_north` | angle, align_views=false | — | Yes | DM-LOC-001 |
| `position.mirror` | axis | — | Yes | DM-LOC-001 |
| `snaps.get` | — | settings | No | DM-SET-004 |
| `snaps.set` | settings | — | No | DM-SET-004 |
| `standards.list` | source_document_id | {kind, items[]}[] | No | DM-XFER-001 |
| `standards.transfer` | source_document_id, selection[{kind, item_ids?}], on_conflict: overwrite\|new_only\|rename | transfer_report | Yes | DM-XFER-001 |
| `classification.load` | path_or_url, name? | system_id | Yes | DM-CLS-001 |
| `classification.assign` | element_ids[], system_id, code | — | Yes | DM-CLS-001 |
| `classification.unassign` | element_ids[], system_id | — | Yes | DM-CLS-001 |
| `keynotes.load` | path | — | Yes | DM-CLS-001 |
| `keynotes.reload` | — | — | Yes | DM-CLS-001 |
| `purge.list` | include_unplaced_views=false | {kind, items[]}[] | No | DM-PURGE-001 |
| `purge.run` | item_ids[], compact_undo=false | report | Yes | DM-PURGE-001 |
| `numbering.rule_based` | — | — | No | DM-NUM-001 |
| `numbering.rule.create` | def | rule_id | Yes | DM-NUM-001 |
| `numbering.rule.edit` | rule_id, changes | — | Yes | DM-NUM-001 |
| `numbering.rule.delete` | rule_id | — | Yes | DM-NUM-001 |
| `numbering.rule.reorder` | rule_ids[] | — | Yes | DM-NUM-001 |
| `numbering.preview` | rule_id \| def, element_ids? | [{element_id, sequence, current, proposed, conflict?}] | No | DM-NUM-001 |
| `numbering.apply` | rule_id?, mode: "fill_missing"\|"renumber_all", element_ids? | report | Yes | DM-NUM-001 |
| `numbering.reset_manual` | element_ids[], rule_id? | — | Yes | DM-NUM-001 |
| `phase.create` | name, after_phase_id? | phase_id | Yes | DM-PHASE-001 |
| `phase.rename` | phase_id, name | — | Yes | DM-PHASE-001 |
| `phase.combine` | phase_id, with: "previous"\|"next" | — | Yes | DM-PHASE-001 |
| `phase_filter.create` | name, states | id | Yes | DM-PHASE-001 |
| `phase_filter.edit` | id, states | — | Yes | DM-PHASE-001 |
| `phase.graphic_overrides.set` | status, overrides | — | Yes | DM-PHASE-001 |
| `phase.manage` | — | — | No | DM-PHASE-001 |
| `element.set_phase` | element_ids[], created?, demolished? | — | Yes | DM-PHASE-001 |
| `project.new` | template_path? | document_id | No | DM-TMPL-001 |
| `project.save_as_template` | path | — | No | DM-TMPL-001 |
| `settings.starting_view.set` | view_id \| "last" | — | Yes | DM-SET-005 |
| `image.list` | — | image[] | No | DM-IMG-001 |
| `image.add` | path, page?, dpi? | image_id | Yes | DM-IMG-001 |
| `image.place` | image_id, view_id, position, width? | element_id | Yes | DM-IMG-001 |
| `image.set_path_type` | mode: "relative"\|"absolute" | report | Yes | DM-IMG-001 |
| `image.delete` | image_ids[], replace_with? | () | Yes | DM-IMG-001 |
| `image.reload` | image_id, path? | — | Yes | DM-IMG-001 |
| `image.manage` | — | — | No | DM-IMG-001 |
| `decal.types` | — | — | No | DM-DECAL-001 |
| `decal.type.create` | name, image_id, attributes? | type_id | Yes | DM-DECAL-001 |
| `decal.type.edit` | type_id, attributes | — | Yes | DM-DECAL-001 |
| `file.open` | path, {partial?: {storeys?, classes?, worksets?}, read_only?} | document_id | No | DM-FILE-001 |
| `file.new` | kind: model\|model_template\|family\|title_block\|annotation_symbol\|mass_family, template?, schema?: IFC4\|IFC4X3 | document_id | No | DM-FILE-006 |
| `file.close` | document_id, save?: "ask"\|"yes"\|"no" | — | No | DM-FILE-001 |
| `element.recognise` | element_ids[] | report | Yes | DM-FILE-001 |
| `file.ifc_open_options` | options? | options | No | DM-FILE-001 |
| `file.save` | document_id? (default: active) | status | No | DM-FILE-002 |
| `file.save_as` | document_id? (default: active), path, kind?, format?: ifc\|ifczip, carrier?: embedded\|external_file\|external_dir\|bundle | status | No | DM-FILE-002 |
| `file.backups` | document_id | backup[] | No | DM-FILE-002 |
| `file.restore_backup` | document_id, backup_id | — | No | DM-FILE-002 |
| `file.convert_schema` | document_id, target: "IFC4"\|"IFC4X3_ADD2" | report | No (clears undo) | DM-FILE-003 |
| `export.ifc` | path, setup? (name, id or inline settings), settings_override?, schema?, strip_rxd?=false, documentation?, background? | job_id (the job result carries the export report) | No | DM-FILE-004 |
| `export.ifc_setup.save` | setup | — | No | DM-FILE-004 |
| `export.ifc_setup.delete` | name | — | No | DM-FILE-004 |
| `export.ifc_setup.list` | — | setup[] | No | DM-FILE-004 |
| `validate.ifc` | path? | report | No | DM-FILE-004 |
| `validate.ids` | ids_path, scope? | report | No | DM-FILE-004 |
| `import.cad` | path, view_id?, options | import_id | Yes | DM-IMP-002/003 |
| `link.cad` | path, view_id?, options | element_id | Yes | DM-IMP-002/003 |
| `import.explode` | import_id, mode: partial\|full | element_ids[] | Yes | DM-IMP-002/003 |
| `import.pdf` | path, page, view_id, options | element_id | Yes | DM-IMP-004 |
| `import.image` | path, view_id, placement | element_id | Yes | DM-IMP-005 |
| `import.mesh` | path, target: "project"\|"family", category?, options | element_id | Yes | DM-IMP-006/007 |
| `import.step` | path, target: "project"\|"family", category? | element_id | Yes | DM-IMP-008 |
| `link.point_cloud` | path, placement | element_id | Yes | DM-IMP-010 |
| `import.bcf` | path | topic_id[] | Yes | DM-IMP-011 |
| `import.ifc_merge` | path, options | report | Yes | DM-IMP-001 |
| `export.pdf` | range, set_id?, setup_id?, path_or_folder, combine?, naming_rule?, background? | job_id | No | DM-EXP-001 |
| `export.dxf` | range, set_id?, setup_id?, folder, naming?, version?, xrefs? | job_id | No | DM-EXP-003 |
| `export.dwg` | range, set_id?, setup_id?, folder, naming?, version?, xrefs? | job_id | No | DM-EXP-004 |
| `export.image` | range, set_id?, path_or_folder, format, size, dpi?, options? | job_id | No | DM-EXP-005 |
| `export.gltf` | path, view_id?, binary?, options? | job_id | No | DM-EXP-006 |
| `export.obj` | path, view_id?, options? | job_id | No | DM-EXP-007 |
| `export.stl` | element_ids[], path | — | No | DM-EXP-008 |
| `export.sqlite` | path, categories?[] | — | No | DM-EXP-012 |
| `export.batch` | set_name \| set | report | No | DM-EXP-013 |
| `file.save_previous_version` | document_id, path | report | No | DM-MIG-001 |
| `family.package.upgrade` | paths[], in_place=false | report | No | DM-MIG-001 |
| `rxd.migration_report` | document_id | report | No | DM-MIG-001 |
| `registry.list` | filter? | command_summary[] | No | DM-CMD-001 |
| `registry.describe` | name | definition | No | DM-CMD-001 |
| `registry.execute` | name, args | — | As target command | DM-CMD-001 |
| `element.query` | spec | {ids[], total, fields?} | No | DM-LUA-003 |
| `addin.updaters` | — | [{id, addin, triggers, enabled, stats}] | No | DM-LUA-005 |
| `addin.updater.enable` | id, on | — | No | DM-LUA-005 |
| `script.run` | module, entry, args? | — | Per transactions in script | DM-LUA-008 |
| `script.list` | scope: "application"\|"document" | script[] | No | DM-LUA-008 |
| `script.create` | scope, module, source | — | Yes (document scope only) | DM-LUA-008 |
| `script.delete` | scope, module | — | Yes (document scope only) | DM-LUA-008 |
| `script.import` | scope, path | — | No | DM-LUA-008 |
| `script.export` | scope, module, path | — | No | DM-LUA-008 |
| `script.record_start` | — | — | No | DM-LUA-008 |
| `script.record_stop` | module_name | source | No | DM-LUA-008 |
| `script.manager` | — | — | No | DM-LUA-008 |
| `script.run_file` | path, entry?, args? | — | Per transactions in script | DM-LUA-008 |
| `script.security` | — | — | No | DM-LUA-007 |
| `direct_shape.create` | category, geometry, name?, type_id? | element_id | Yes | DM-LUA-010 |
| `direct_shape.set_geometry` | element_id, geometry | — | Yes | DM-LUA-010 |
| `geom.raycast` | origin, direction, filter? | hit[] | No | DM-LUA-010 |
| `addin.validate` | path | {ok, errors[], license_status} | No | DM-ADDIN-001 |
| `addin.list` | — | addin[] | No | DM-ADDIN-003 |
| `addin.enable` | id, on | — | No | DM-ADDIN-003 |
| `addin.reload` | id | — | No | DM-ADDIN-003 |
| `addin.install` | path_or_registry_id | id | No | DM-ADDIN-003 |
| `addin.uninstall` | id | — | No | DM-ADDIN-003 |
| `addin.update` | id | — | No | DM-ADDIN-003 |
| `addin.set_override` | id, on | — | No | DM-ADDIN-003 |
| `addin.trust.grant` | id, level | — | No | DM-ADDIN-003 |
| `addin.trust.revoke` | id | — | No | DM-ADDIN-003 |
| `addin.manage` | — | — | No | DM-ADDIN-003 |
| `graph.open` | path | graph_id | No | DM-VPL-001 |
| `graph.run` | graph_id, inputs? | outputs | Yes (one item per run) | DM-VPL-001 |
| `graph.save` | graph_id, path? | — | No | DM-VPL-001 |
| `graph.compile` | graph_id | lua_source | No | DM-VPL-001 |
| `graph.player.list` | folder? | graph_summary[] | No | DM-VPL-002 |
| `study.create` | graph_id, variables, goals | study_id | No | DM-VPL-003 |
| `study.run` | study_id, solver, budget | outcomes | No | DM-VPL-003 |
| `study.apply` | study_id, outcome_id | — | Yes | DM-VPL-003 |
| `mcp.start` | transport, options | — | No | DM-MCP-002 |
| `mcp.stop` | — | — | No | DM-MCP-002 |
| `mcp.token.create` | name, level, expiry?, document_scope? | token | No | DM-MCP-002 |
| `mcp.token.revoke` | token_id | — | No | DM-MCP-002 |
| `mcp.sessions` | — | session[] | No | DM-MCP-002 |
| `mcp.session.disconnect` | session_id | — | No | DM-MCP-002 |
| `session.begin_group` | name | — | — (groups items) | DM-MCP-004 |
| `session.end_group` | mode | — | — (groups items) | DM-MCP-004 |
| `toolsets.list` | — | — | No | DM-MCP-004 |
| `toolsets.enable` | — | — | No | DM-MCP-004 |
| `toolsets.disable` | — | — | No | DM-MCP-004 |
| `settings.arrowhead.create` / `.edit` / `.delete` | name, style, props / id, props / id, replace_with? | element_id / — / — | Yes | DM-SET-003 |

**MCP-only tools** (protocol-level compound tools, not registry commands; §2.16.3, §2.16.4):

| Tool | Parameters | Returns | Undoable | Req ID |
|---|---|---|---|---|
| `find_commands` | query, limit? | command summaries with schemas | No | DM-MCP-003 |
| `describe_command` | name | command definition | No | DM-MCP-003 |
| `run_command` | name, arguments | the command's result | As target command | DM-MCP-003 |
| `query_elements` | spec (collector JSON) | {ids[], total, fields?} | No | DM-MCP-004 |
| `get_element` | id_or_guid, fields? | element summary | No | DM-MCP-004 |
| `get_selection` / `set_selection` | — / ids[] | ids[] / — | No | DM-MCP-004 |
| `get_view_image` | view_id, width, height, options? | PNG image | No | DM-MCP-004 |
| `execute_lua` | source, args? | returned value (JSON), log | Per transactions in script | DM-MCP-004 |
| `batch` | calls[], atomic=true | results[] | Yes (one item if atomic) | DM-MCP-004 |
| `undo` / `redo` / `history` | steps? | — / — / history[] | — (undo operations) | DM-MCP-004 |
| `preview` | command, arguments | change set, warnings, image? | No (always rolled back) | DM-MCP-004 |
| `confirm` | preview_token | the command's result | Yes | DM-MCP-004 |

---

## Open questions and deviations from the reference

### Open questions

| ID | Question | Proposed answer / next step |
|---|---|---|
| Q-DM-01 | Should add-ins be able to define new categories? | Not in v1. Use `cat.generic_models` with `ObjectType`, or a registry extension in v2 once IFC mapping rules for custom categories are agreed. A v2 design would let an add-in register `addin.<namespace>.<key>` categories that must name an IFC class and `PredefinedType` (usually `USERDEFINED` plus `ObjectType`), default object styles and behaviour flags; projects using them would still open without the add-in, falling back to Generic Models. *Deferred to the post-Alpha/POC review (owner, 2026-10-12).* |
| Q-DM-02 | Roofs: always `IfcRoof` with `IfcSlab` parts, or a single `IfcSlab ROOF` for one-face roofs? | Default to `IfcRoof` aggregate (more consistent); make the simple form an export option. Check with downstream tools. Test plan: export both forms from five roof shapes (flat, mono-pitch, gable, hip, freeform), open them in at least three viewers and two quantity take-off tools, and record which form gives correct areas and classification. Decide before P1 roof work starts. |
| Q-DM-03 | Property-set naming for built-in parameters that have no buildingSMART home (group-named sets such as `Identity Data`), and the default set for user parameters (`<Project> Parameters` / `<Category> Parameters`). | Needs owner confirmation. The alternative is one vendor-neutral set name (for example `Common`). Must align with `02a`, which owns the `Reviera_*` names. Criteria: names must not collide with buildingSMART `Pset_`/`Qto_` prefixes, must be stable across languages (not localised group names), and must be readable in third-party schedules. A sample file with both conventions will be reviewed with two downstream users. |
| Q-DM-04 | Is `IfcProjectLibrary` declared by `IfcProject` (families inside a project) accepted by the buildingSMART validation service and by common viewers? | Test with sample files before P1. Fallback: declare types directly on `IfcProject` and keep the family grouping in RXD only. Test: a project with three families as `IfcProjectLibrary` objects, validated by the buildingSMART validation service and opened in at least three viewers and BIMserver. If any rejects or loses the types, use the fallback and record the finding in `02a`. |
| Q-DM-05 | Detail-level geometry in extra sub-contexts (`Body-Coarse`/`Body-Medium`): do other tools ignore them safely? | Test with at least three viewers. Fallback: keep Coarse/Medium in RXD as recipes only. Also check file-size impact on a 50 MB model (extra maps for families with detail-level variants) and whether BIMserver stores the extra sub-contexts. Decide before the P1 Family Editor milestone. |
| Q-DM-06 | Sizes of embedded RXD documents in BIMserver (large `IfcText` values) and their effect on server performance. | Measure with a 100-sheet project. `02a` defines chunking. COL owns BIMserver specifics. Measurements to record: check-in time, server storage, download time and query speed with embedded RXD versus the external carrier, at 10, 100 and 500 sheets. If embedded documents slow the server noticeably, the BIMserver default carrier becomes external. |
| Q-DM-07 | OCCT thread-safety details (memory manager mode, handle reference counting) for the pinned OCCT version. | Stress tests in CI (DM-REGEN-004). Run the parallel regeneration suite under ThreadSanitizer-equivalent checks and a long-running soak test (10,000 random edits) per OCCT upgrade; document the memory-manager mode and any operations that must be serialised. Any crash or data race blocks the OCCT upgrade. |
| Q-DM-08 | General 2D constraint solver choice (PlaneGCS vs SolveSpace solver) and its licence fit. | Decide before P2. Both are GPL-3.0-compatible as stated, but this must be verified. Compare on a benchmark of 200 sketches from shipped families: solve time, robustness with redundant constraints, quality of conflict reports, and Rust binding effort. The licence check goes to the dependency licence register. |
| Q-DM-09 | Licence and coverage checks for LibreDWG (DWG read/write versions), openNURBS (3DM), OpenUSD and bundled open data (weather, cities, EPD sets, textures). | Track in the dependency licence register (`cargo-deny` plus a data licence list). Each format or data set gets an owner, a licence verdict and a coverage note (versions read and written). Formats that fail the check move to P3 with the open alternative stated in §2.11.5 and §2.11.6. |
| Q-DM-10 | MCP tool names containing `.`: accepted by all target MCP clients? | Default to dotted names, with the `underscore` style setting as a fallback (DM-MCP-003). Revisit when the MCP naming guidance changes. Test the five most used MCP clients at each release and record which need the fallback; the setting can be chosen per client token. |
| Q-DM-11 | ~~RXD document and key names used here~~ | **Resolved**: aligned with the Name Registry and Object Kind Registry in `02a` §6.4–§6.5, which are authoritative. New names proposed by this chapter (kinds `numbering.rule` and `decal.type`, element-data key `elements.numbering`) are listed for `02a` to register. |
| Q-DM-12 | LuaJIT on ARM64 macOS, and LuaJIT memory limits through `mlua`. | Validate per platform. Fall back to interpreter mode and to memory monitoring (DM-LUA-001, DM-LUA-007). The CI matrix runs the Lua test suite with JIT on and off on Windows x64, Linux x64 and arm64, and macOS arm64; any platform with failures ships with the JIT off until fixed. |
### Intentional deviations from the reference

| Area | Reference behaviour | Reviera behaviour | Reason |
|---|---|---|---|
| Native format | Proprietary compound file, one-way yearly upgrades | A single valid IFC file, with RXD embedded as standard property sets (or an optional sidecar); semver RXD; save to previous version | Openness, longevity, version-control friendliness (§2.1, DM-MIG-001) |
| Internal units | Decimal feet | SI (metres, radians, …) | Matches IFC, and avoids conversion error (DM-UNIT-001) |
| Regeneration | Largely single-threaded | Parallel by dependency level with rayon; deterministic | Use of modern CPUs (DM-REGEN-003) |
| Over-constraint errors | A generic "constraints not satisfied" message | Names and highlights the conflicting constraints | Usability (DM-REGEN-002) |
| Undo after save | Cleared in some situations, such as synchronisation | Kept across local saves | Usability (DM-TXN-002) |
| Formula language | No `<=`/`>=`, no text concatenation, implicit units, no global references in families | `<=`/`>=`, `&` concatenation, explicit unit rules with a compatibility rule, `@Global` references, interpolating lookups, text functions | Expressiveness and safety (DM-FRM-001) |
| Shared parameters | One active UTF-16 text file per session; or a vendor cloud service | Several JSON parameter libraries, versioned, distributable by git/HTTPS; import of legacy text files | Openness, diffs, team distribution without a vendor cloud (DM-SPAR-001) |
| Lookup tables and type catalogs | `##`-typed header CSV, `.txt` catalogs | `Name[spec:unit]` header CSV, `.types.csv`; legacy header import | Own, readable format (DM-LUT-001, DM-TCAT-001) |
| Family files | Proprietary container | IFC type library + RXD; usable as static types by any IFC tool | Interoperability (DM-FFMT-001) |
| Add-ins | Compiled .NET assemblies; XML manifest; runtime rebuilds on .NET changes | Lua packages; TOML manifest with a mandatory SPDX `license`; GPL-compatibility check; trust levels | GPL policy (TECH_STACK §4.2); safety; no rebuilds (§2.14) |
| Macros | C# with an external IDE and SDK install | Lua, built-in editor, optional external editor with a language server; recording | Lower barrier (DM-LUA-008) |
| Visual programming | A separate bundled application | Built on the command registry and Lua, compiled to Lua | One API surface (DM-VPL-001) |
| AI integration | A vendor-hosted assistant (preview) | Built-in MCP server for any agent; per-token permissions; confirm mode; attribution | Openness and control (§2.16) |
| Options Bar | Removed in 2027 | Never present; contextual ribbon and Properties | Matches the reference's direction (DM-FED-000; UI chapter) |
| ODBC export | ODBC database export | SQLite export and re-import by GUID | Portable, open (DM-EXP-012) |
| Cloud batch processing | Vendor cloud service | Local headless mode and CLI | No vendor dependency (DM-FILE-005) |
| Product tiers | A reduced "LT" tier | No tiers | Open-source project |

---

## Traceability

**Research sections used (`research/05_data_model_platform.md`):**

| Research section | Used in |
|---|---|
| §1 Core data model (§1.1–§1.4) | §2.1, §2.2.1–§2.2.6, §2.3.2 |
| §2 Identity, hosting, change engine, phases/options/worksets (§2.1–§2.4) | §2.3.7, §2.3.11, §2.4.4, §2.5, §2.6.1–§2.6.3 |
| §3 Transactions, undo, regeneration, events, external events | §2.6.4, §2.7.1–§2.7.4, §2.13.4–§2.13.6 |
| §4 Family Editor (§4.1–§4.10) | §2.2.3, §2.8.0–§2.8.11, §2.8.14, §2.8.15 (library browser), §2.11.0 (new family documents) |
| §5 Special family types (§5.1–§5.8) | §2.8.12, §2.8.13 |
| §6 Conceptual massing environment | §2.8.13 |
| §7 Parameters (§7.1–§7.7) | §2.3.3, §2.9.1–§2.9.4, §2.9.6–§2.9.8 |
| §8 Units | §2.9.4, §2.9.5 |
| §9 Project setup / Manage (§9.1–§9.13) | §2.10.1–§2.10.15 (§9.12 for image management and decal types, §2.10.14–§2.10.15) |
| §10 Materials | §2.3.8, §2.10.5 |
| §11 File formats, versioning, worksharing storage | §2.3.1, §2.7.3, §2.11.1–§2.11.3, §2.11.8 |
| §12 Import / export / interoperability | §2.11.4–§2.11.6, §2.16 (AI assistant direction) |
| §13 Extensibility (API, manifests, cloud automation, macros, visual programming, generative design, third-party platforms) | §2.12–§2.16, §2.11.7 |
| §14 Release history | Appendix A; §2.10.16 (rule-based numbering, 2027) |
| §15 System requirements, licensing, product variants | Appendix A (version notes), §2.6.4 |
| §16 Implications (recommendations 1–12) | §2.1, §2.5, §2.6, §2.7, §2.8, §2.9, §2.10, §2.11.8, §2.12–§2.16 |

**Screenshots referenced (internal reference only, never Reviera assets):** `screenshots/platform/` — `family_types_dialog_parameter_groups.png`, `family_types_cabinet_variations_2026.png`, `reporting_parameters_angles_2026.png`, `formula_driven_sunshade_family_2026.png`, `edit_label_dialog_2026.png`, `adaptive_component_repetition_2026.png`, `conceptual_divided_surface_patterned_2026.png`, `conceptual_uv_grid_2026.png`, `edit_shared_parameters_dialog.png`, `parameter_properties_shared_project_parameter.png`, `project_units_dialog.png`, `manage_tab_settings_panel.png`, `manage_tab_visual_programming_panel.jpg`, `location_and_site_dialog_2026.png`, `material_browser_appearance_tab_2026.png`, `material_browser_physical_asset_annotated_2026.png`, `material_carbon_information_2027.png`, `ifc_property_mapping_2027.png`, `macro_manager_and_vscode_2026.png`, `macro_vscode_editor_2026.png`, `dynamo_interface_library_canvas.jpg`, `dynamo_revit_node_library.jpg`, `dynamo_documentation_browser_2026.png`, `dynamo_player_inputs_2026.png`, `wall_tool_ribbon_no_options_bar_2027.png`, `autodesk_assistant_2027.png`.

**Web sources (public pages, accessed 2026-10-12; logged in the chapter's sources file):**

| Source | Used in |
|---|---|
| Symetri, "What's New in Revit 2027: A Guide to the Latest Features" (web: What's New in Revit 2027 (Symetri)) | §2.10.16 DM-NUM-001 |
| BIM Pure, "Revit 2027" (web: Revit 2027 (BIM Pure)) | §2.10.16 DM-NUM-001 |
| Autodesk public help, "Manage Images Dialog" (web: Manage Images Dialog) | §2.10.14 DM-IMG-001 |
| Autodesk public help, "Decal Type Properties" and "Decal Attributes" (web: Decal Type Properties; web: Decal Attributes) | §2.10.15 DM-DECAL-001 |

**Other sources:** `spec/06_collaboration_coordination.md` COL-LNK-005 (image assets, kept consistent with §2.10.14); `TECH_STACK.md` §1–§5 (architecture, IFC mapping, OCCT threading, LuaJIT, MCP, BIMserver modes, licensing and add-in policy); `spec/00_conventions.md`; `spec/02a_extension_data_format.md` (RXD carrier; referenced, not redefined).
