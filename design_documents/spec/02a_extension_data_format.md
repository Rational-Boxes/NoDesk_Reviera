# Chapter 2a — Reviera Extension Data (RXD): Format and IFC Embedding

**ID prefix:** `DM-RXD` · **Priority:** P0 · **Status:** Draft 0.2 · 2026-10-12

**Owner directive (2026-10-12):** *"Full interoperability using IFC, with any additional information held in a bespoke format. If it can be embedded in a custom IFC data section that would be optimal. Priority is compatibility."*

**Decision.**
- Reviera project data is **IFC first**.
- Information IFC cannot express is held in one bespoke format, **Reviera Extension Data (RXD)**: versioned JSON documents defined by JSON Schemas.
- By default, RXD is **embedded inside the IFC file** using only standard, schema-valid IFC constructs: custom property sets with text values.
- An **external sidecar** (`.reviera`) is a second carrier for exactly the same documents. It is used when embedding isn't wanted or a downstream tool removes unknown data.

---

## 1. Principles (in priority order)

| # | Principle |
|---|-----------|
| 1 | **Compatibility first.** Every file Reviera writes is a valid IFC file under its declared schema, and passes the buildingSMART schema and normative-rule checks. Reviera never writes custom entities, non-standard sections or comments that carry data. |
| 2 | **IFC holds everything it can express.** This includes spatial structure, elements, types, materials and layer sets, openings, ports, systems, zones, grids, classifications, standard Psets and Qtos, and the *values* of user-defined parameters, which go in readable Psets. Other tools see the complete building without knowing about Reviera. |
| 3 | **RXD holds only what IFC can't.** This includes parametric intent (constraints, sketches, generation recipes, family definitions, formulas), documentation (views, sheets, schedules, view-specific annotation, graphic standards), and authoring organisation (phases, design options, worksets, browser organisation, project UI state). |
| 4 | **References go one way.** RXD points into IFC by **GlobalId**. IFC content never depends on RXD. |
| 5 | **Degrades safely.** If a tool removes RXD, the IFC model is still complete and correct; only drawings and parametric intent are lost. If a tool edits elements, Reviera detects it and reconciles (§7). |
| 6 | **Open format.** The JSON Schemas are published under the MIT licence, together with this document, so any tool can read and write RXD. |

## 2. Why Property Sets (the evaluation behind "custom IFC data section")

IFC has no official extension section for arbitrary vendor data. The candidate mechanisms were evaluated for compatibility:

| Mechanism | Valid IFC? | Survives other tools? | Verdict |
|-----------|-----------|------------------------|---------|
| Custom entity types | ❌ invalid against the schema | ❌ | Rejected |
| STEP comments `/* … */` | Valid | ❌ removed by parsers and writers | Rejected |
| Custom HEADER entities / Part 21 Ed.3 sections | Doubtful | ❌ not supported by mainstream IFC tools | Rejected |
| `IfcDocumentReference` to an external file | ✅ | Only the link survives; the content is outside the file | Used only for the external-sidecar carrier (§5) |
| Repurposing `IfcBlobTexture` and similar binary fields | Formally valid | Semantically wrong; tools may strip or alter them | Rejected |
| **Custom property sets (`Reviera_*`) with `IfcText` values** | ✅ IFC2X3/IFC4/IFC4X3; custom Pset names without the `Pset_` prefix are allowed | ✅ property sets are the most widely kept part of IFC across viewers, checkers, model servers (including BIMserver's object storage) and authoring tools | **Chosen** |

Two further benefits of property sets:
- Data **attached to the element it describes** moves with that element through partial exports, model splitting, federation and BIMserver's object-level storage.
- The design **works unchanged for every IFC encoding**: STEP (`.ifc`), `.ifczip`, ifcXML, and future JSON-based IFC.

## 3. Embedded Carrier (default)

### 3.1 Placement

| RXD scope | Attached to | Property set name |
|-----------|-------------|-------------------|
| Manifest | `IfcProject` | `Reviera_Manifest` |
| Project-level document (views, sheets, schedules, standards, phases, …; §6) | `IfcProject` | `Reviera_Doc:<document-path>` (one Pset per document, e.g. `Reviera_Doc:views/index`) |
| Element data (recipe, sketch, owned constraints, phase/option/workset membership) | The element itself, e.g. `IfcWall`, `IfcStair` | `Reviera_Element` |
| Type data (family linkage, type parameter formulas) | The `IfcTypeObject` | `Reviera_Type` |
| Family recipe (shared by all its types) | `IfcProject` (project) or `IfcProjectLibrary` (family file) | `Reviera_Doc:families/<familyId>` |

All are attached through standard relations: `IfcRelDefinesByProperties`, or `HasPropertySets` for type objects.

### 3.2 Property layout

**`Reviera_Manifest`** (one per file):

| Property | IFC type | Content |
|----------|----------|---------|
| `Format` | IfcLabel | `reviera-rxd` |
| `FormatVersion` | IfcLabel | `1.0` |
| `DataId` | IfcIdentifier | UUID that identifies this model's RXD set |
| `DataRevision` | IfcInteger | Increments on every save |
| `Writer` | IfcLabel | `Reviera 0.4.0` |
| `Carrier` | IfcLabel | `embedded`, or `external` (see §5) |
| `Index` | IfcText | A JSON document index with path, kind, schema version, sha256 and object count for each document |

**`Reviera_Doc:<path>`** and **`Reviera_Element`** / **`Reviera_Type`**:

| Property | IFC type | Content |
|----------|----------|---------|
| `Kind` | IfcLabel | Document kind (for example `views.index`). For element data, a comma-separated list of the sections present (for example `identity,sketch,recipe`); see §6.4. |
| `SchemaVersion` | IfcLabel | For example `1.0` |
| `Encoding` | IfcLabel | `json` or `zstd+base64` |
| `Sha256` | IfcIdentifier | Hash of the decoded JSON bytes |
| `Rev` | IfcInteger | Object/document revision, used for merging |
| `GeomHash` | IfcIdentifier | **Element data only.** Hash of the element's canonical IFC representation as Reviera last generated it. Used to detect edits made by other tools (§7). |
| `Data` | IfcText | Payload, when it fits in one chunk |
| `Data_0001` … `Data_nnnn` | IfcText | Payload chunks, when it doesn't |

### 3.3 Encoding rules (compatibility-driven)
1. **Choosing an encoding:**
   - Payloads of **8 KiB or less** are stored as `json`: readable in any IFC property viewer.
   - Larger payloads are stored as `zstd+base64`: compact, and base64 is plain ASCII.
2. **ASCII only.** JSON is written with every non-ASCII character escaped as `\uXXXX`. Every payload string is therefore 7-bit ASCII, which avoids differences between tools in handling STEP `\X2\` encodings. STEP string escaping (doubled `'` and `\\`) is applied by the IFC writer (IfcOpenShell).
3. **Chunk size.** Each chunk holds at most **32,000 characters**, configurable, chosen to stay clear of string-buffer limits in older parsers. Chunks are joined in numeric order before decoding.
4. **Canonical JSON.** Keys are sorted, the representation is deterministic and there is no insignificant whitespace. This makes `Sha256` stable and diffs small.
5. **Size guard.** If one document would exceed a configurable limit (default 64 MiB encoded), Reviera warns and suggests the external carrier.

### 3.4 Example (STEP excerpt)

```
#5021=IFCPROPERTYSINGLEVALUE('Kind',$,IFCLABEL('identity,recipe'),$);
#5022=IFCPROPERTYSINGLEVALUE('Encoding',$,IFCLABEL('json'),$);
#5023=IFCPROPERTYSINGLEVALUE('GeomHash',$,IFCIDENTIFIER('b3f1c09e…'),$);
#5024=IFCPROPERTYSINGLEVALUE('Data',$,IFCTEXT('{"identity":{"iid":48213},"recipe":{"kind":"recipe.stair","riserHeight":0.175,"runs":[{"end":[3.6,0],"risers":12,"start":[0,0]}],"treadDepth":0.28}}'),$);
#5025=IFCPROPERTYSET('1hQ$…',#5,'Reviera_Element',$,(#5021,#5022,#5023,#5024));
#5026=IFCRELDEFINESBYPROPERTIES('2aX0…',#5,$,$,(#4800),#5025);   /* #4800 = IFCSTAIR */
```

## 4. Exports for Exchange

| Command | Result |
|---------|--------|
| **Save** (default) | IFC with embedded RXD: full fidelity, and still fully standard IFC |
| **Export clean IFC** (`export.ifc{strip_rxd=true}`) | IFC with all `Reviera_*` Psets removed, for recipients who want no vendor-specific properties. The building model is identical. |
| **Export IFC with documentation** (`export.ifc{documentation=true}`) | Also writes documentation in standard IFC form for consumers that support it: annotation as `IfcAnnotation` (`IfcGeometricCurveSet`, `IfcTextLiteralWithExtent`), worksets and design options as `IfcGroup`, phase status as `Pset_*Common.Status`. This is an export; RXD remains the authoritative store. |
| **Save with external sidecar** | See §5 |

## 5. External Carrier (`.reviera` sidecar), the alternative

The external carrier holds the **same RXD documents** in a file outside the IFC. Use it when:
- the project is version-controlled in git (directory form gives one JSON file per document, so diffs are clean)
- a downstream tool in the round-trip strips unknown Psets
- documents exceed the embedding size guard

**Forms.**
- `X.reviera`: a ZIP holding `manifest.json` (stored first, uncompressed) plus `<document-path>.json`.
- `X.reviera/`: an unpacked directory with the same layout.
- `X.rvb`: a bundle ZIP holding `model.ifc` and `reviera/…`.

**Binding.** In external mode the IFC file still carries `Reviera_Manifest`, with `Carrier = external`, its `DataId` and `DataRevision`, and an `IfcDocumentReference` to the sidecar file name. **Element-level `Reviera_Element` and `Reviera_Type` Psets are still embedded**, because they are small and travel with their elements. Only `Reviera_Doc:*` documents move out of the file.

**Lookup on open.** Use embedded documents if present. Otherwise look for `X.reviera` (file or folder), then a `.rvb` containing the IFC. If nothing is found, open with default standards and show a notice.

## 6. Object Model and Document Catalogue

### 6.1 Object envelope
All RXD objects share one envelope:

```json
{ "id": "0192a3f5-1b2c-7d3e-8f40-112233445566", "kind": "view.plan", "rev": 7,
  "name": "Level 1", "data": { }, "ext": { } }
```

- `id` is a UUIDv7 for objects that exist only in RXD.
- Element data uses the element's GlobalId as its identity.

### 6.2 References

| Target | Form | Example |
|--------|------|---------|
| IFC object | `ifc:<GlobalId>` | `ifc:3vB2YO$MX4xv5uCqZZG05x` |
| RXD object | `rv:<uuid>` | `rv:0192a3f5-…` |
| Named geometric sub-reference | `ifc:<GlobalId>#<ref>` | `#face:core.ext`, `#edge:top.start`, `#refplane:Left`, `#end:1` |
| Linked model element | `link:<linkId>/ifc:<GlobalId>` | — |

Sub-references are **named, never index-based**. The geometry engine resolves them after each regeneration. Units are SI (metres, radians). 3D coordinates are in the IFC project world coordinate system. View annotation coordinates are in the view's own plane coordinate system.

### 6.3 Documents (format version 1.0)

| Document path | Scope | Kinds / contents | Chapter |
|---------------|-------|------------------|---------|
| `project/settings` | project | display units and formats, snaps, starting view, project information not covered by IFC | 2 |
| `project/phases` | project | phases, phase filters, phase graphic overrides | 5 |
| `project/design_options` | project | option sets, options, primary option | 5 |
| `project/worksets` | project | worksets: name, kind, default visibility | 6 |
| `project/revisions` | project | drawing revisions and numbering | 5 |
| `project/links` | project | linked IFC, CAD, point cloud, image and PDF sources, with positioning | 6 |
| `standards/object_styles`, `line_weights`, `line_patterns`, `fill_patterns`, `text_styles` | project | graphic standards, in open definitions | 5 |
| `standards/parameters` | project | parameter **definitions**: data type, unit kind, group, scope, category bindings, formula, shared identifier, target Pset and property | 2 |
| `standards/browser` | project | project browser organisation | 1 |
| `model/constraints` | project | constraints between elements: locks, EQ, alignments, attach relations, join overrides. Constraints owned by one element are stored in its `Reviera_Element`. | 3 |
| `model/groups`, `model/rooms`, `model/selection_sets` | project | group definitions, room and area settings, area schemes, selection sets | 3 |
| `model/mep`, `model/electrical`, `model/analysis`, `model/fabrication` | project | routing preferences, sizing tables, demand factors, panel schedule templates, analysis settings and results, MEP fabrication (ROFC) and precast configuration | 4 |
| `model/structural`, `model/rebar` | project and element | structural settings; connection status; steel part and assembly marks; precast piece provenance; rebar numbering sequences and partitions (element-level parts are stored in `Reviera_Element`) | 4 |
| `views/index`, `views/templates`, `views/filters`, `views/<id>` | project | view definitions and graphics overrides | 5 |
| `views/<id>/annotation` | project | view-specific annotation for that view | 5 |
| `schedules/<id>` | project | schedule definitions | 5 |
| `sheets/index`, `sheets/<id>` | project | sheets, viewports, schedule instances, sheet annotation | 5 |
| `families/<id>` | project/library | parametric family recipe | 2 |
| `ui/state` | project | open views and tab layout (project-scoped only; per-user preferences never go into RXD) | 1 |
| **`Reviera_Element`** | element | `recipe.*` (stair, railing, roof, curtain grid, wall profile, toposolid grading, rebar set, …), `sketch`, owned constraints, `membership` (phase created/demolished, design option, workset) | 3, 4, 5, 6 |
| **`Reviera_Type`** | type | family id, type-parameter formulas and values not representable in standard Psets | 2 |

Live **ownership/lock** state is *not* stored in the model file; see §8.

### 6.4 Name Registry (normative; consolidated from Chapters 1–6)

The names below are canonical. Earlier drafts of chapters may use the aliases shown, which mean the same document.

**Project-level documents** (`Reviera_Doc:<path>`):

| Canonical path | Aliases used in chapters | Defined in |
|----------------|-------------------------|------------|
| `project/settings` | `settings`, `units` | Ch 2 §2.10, Ch 6. Named sections include `site` (Ch 3 site settings), `assemblies`, `sunPresets`, `spot`, `detailLevelMap` and `output.animationSetups` (Ch 5) |
| `project/identity` | `identity` (document) | Ch 2 §2.5: IDs for non-rooted resources, next internal ID |
| `project/location` | `location` | Ch 2 §2.10 |
| `project/warnings` | `warnings` | Ch 2 §2.7 (persisted failures) |
| `project/phases` | `phasing` | Ch 5 |
| `project/design_options` | `design_options` | Ch 5 |
| `project/worksets` | `worksets` | Ch 6 |
| `project/revisions` | — | Ch 5 |
| `project/links` | `links` | Ch 6 |
| `project/reconcile_log` | — | 02a §7 |
| `standards/parameters` | `parameters` | Ch 2 §2.9 |
| `standards/materials` | `materials` | Ch 2 §2.10 (appearance, thermal and carbon data beyond IFC material properties) |
| `standards/styles` | `styles` | Ch 2 §2.10.3: the umbrella for the next five documents; writers may split it |
| `standards/object_styles`, `standards/line_weights`, `standards/line_patterns`, `standards/fill_patterns`, `standards/text_styles` | — | Ch 5 |
| `standards/browser` | — | Ch 1, Ch 5 |
| `model/datums` | `datums` | Ch 2 §2.3.5 (non-storey levels and similar), Ch 3 |
| `model/constraints` | — | Ch 3 |
| `model/groups` | `groups` | Ch 3 |
| `model/assemblies` | `assemblies` | Ch 5 §5.16 (assembly types, instances and documentation templates) |
| `model/parts` | — | Ch 3 §3.6.11 (part divisions and merges) |
| `model/rooms` | — | Ch 3 |
| `model/selection_sets` | — | Ch 3 |
| `model/structural`, `model/rebar` | `structural`, `rebar` | Ch 4 |
| `model/mep`, `model/electrical` | `mep_settings`, `electrical` | Ch 4 |
| `model/analysis` | `analysis` | Ch 4 |
| `model/fabrication` | `fabrication` | Ch 4 |
| `views/index`, `views/templates`, `views/filters`, `views/types` | — | Ch 5 |
| `views/bookmarks` | — | Ch 1 §1.29.5: shared view bookmarks. Personal ones are never stored in RXD. |
| `views/color_schemes` | "colour schemes in the views documents" | Ch 3 §rooms, Ch 5 |
| `views/<viewId>` | `views` | Ch 5 |
| `views/annotation/<viewId>` | `annotation`, `views/<id>/annotation` | Ch 5 |
| `schedules/<scheduleId>` | — | Ch 5 |
| `sheets/index`, `sheets/<sheetId>` | — | Ch 5 |
| `coordination/monitors`, `coordination/clash`, `coordination/issues` | — | Ch 6 (monitor settings, clash tests and results, BCF topics and extensions) |
| `families/index`, `families/<id>`, `families/<id>/package` | `families/<family-guid>`, `family_package` | Ch 2 §2.8 |
| `scripts/<name>` | `scripts` | Ch 2 §2.12, 02a §9 |
| `ui/state` | — | Ch 1: also holds `recommended_layouts` and `recommended_scenarios` |
| `addins/<namespace>/…` | — | 02a §9 |

**Element data** (`Reviera_Element` on an occurrence). The payload is one JSON object, and its top-level keys are **sections**. Each section is optional, and the property `Kind` lists the sections present.

| Section | Contents | Defined in |
|---------|----------|------------|
| `identity` | persisted 64-bit internal ID, category key when it can't be derived from IFC | Ch 2 §2.5 |
| `elements` | element facts IFC can't hold, such as a reference to a non-storey level; `numbering` (rule-based numbering state: rule id, manual override flag) | Ch 2 §2.3, §2.10.16 |
| `hosting` | host details beyond `IfcRelVoids`/`IfcRelFills` (offsets, cut behaviour); `orphaned` flag set when a copied or monitored host is lost (Ch 6 COL-CM-003) | Ch 2, Ch 6 |
| `decals` | decals placed on the element's faces: decal type, face reference, position, size | Ch 5 DOC-OUT-011 |
| `membership` | phase created/demolished, design option, workset | Ch 2, Ch 5, Ch 6 |
| `phasing` | alias of the phase part of `membership` | Ch 2 |
| `placement` | placement intent: work plane, offsets, flip state, host face reference | Ch 3 |
| `sketch` | sketch loops and paths, slope arrows, per-edge attributes | Ch 3 |
| `recipe` | `{ "kind": "recipe.wall" \| "recipe.compound" \| "recipe.datum" \| "recipe.roof" \| "recipe.stair" \| "recipe.railing" \| "recipe.curtain_grid" \| "recipe.opening" \| "recipe.toposolid" \| "recipe.property_line" \| "recipe.rebarset" \| …, … }` | Ch 3, Ch 4 |
| `constraints` | constraints owned by this element alone | Ch 3, Ch 4 |
| `paint` | per-face material overrides (painted faces) | Ch 3 |
| `adaptive` | adaptive point placements | Ch 2 §2.8 |
| `structural`, `rebar`, `electrical`, `mep_settings`, `analysis` | discipline-specific element data | Ch 4 |

**Type data** (`Reviera_Type` on an `IfcTypeObject`). Sections are `family` (family id and type-catalogue row), `formulas`, `layers` (layer data IFC can't hold, such as core boundary, wrapping, variable layer, stacked-wall and sweep/reveal definitions), and `params` (type values IFC can't hold). Chapter 2 refers to this as element data (`types`). Type recipes use the `recipe` section with the type kinds listed in §6.5.

### 6.5 Object Kind Registry (normative)

Every RXD object carries a `kind` (§6.1). The kinds below are registered for format version 1.0. A reader that meets an unregistered kind keeps the object unchanged and reports it (§7); add-ins use `addin.<namespace>.<kind>`. New kinds are added here first, then used in the chapters.

| Namespace | Registered kinds | Stored in | Defined in |
|-----------|------------------|-----------|------------|
| `recipe.*` (element) | `recipe.wall`, `recipe.datum`, `recipe.roof`, `recipe.stair`, `recipe.railing`, `recipe.curtain_grid`, `recipe.opening`, `recipe.toposolid`, `recipe.property_line`, `recipe.rebarset`, `recipe.part` | `Reviera_Element` → `recipe` | Ch 3, Ch 4 |
| `recipe.*` (type) | `recipe.compound`, `recipe.stacked_wall`, `recipe.curtain_type`, `recipe.railing_type`, `recipe.stair_type` | `Reviera_Type` → `recipe` | Ch 3 |
| element sections | `placement`, `paint`, `sketch` (sections, not kinds; listed for completeness) | `Reviera_Element` | Ch 3 |
| `view.*` | `view.plan`, `view.ceiling`, `view.structural`, `view.area`, `view.section`, `view.elevation`, `view.callout`, `view.detail`, `view.drafting`, `view.legend`, `view.camera`, `view.walkthrough`, `view.rendering`, `view.familytype`, `view.template`, `view.tagtype` (in `views/types`) | `views/<viewId>`, `views/templates`, `views/types` | Ch 5 |
| `viewport.*` | `viewport.type` | `standards/text_styles` | Ch 5 |
| `anno.*` | `anno.dimension`, `anno.keynote`, `anno.colorlegend`, `anno.planregion`, `anno.elevationmarker`, `anno.legendcomponent`, `anno.cutprofile`, `anno.displacementpath`, and the other annotation kinds of Ch 5 §5.13 | `views/annotation/<viewId>` | Ch 5 |
| `schedule.*` | `schedule.component`, `schedule.keyschedule`, `schedule.keynotelegend`, `schedule.revision`, `schedule.graphicalcolumn`, and the other schedule kinds of Ch 5 §5.14 | `schedules/<scheduleId>` | Ch 5 |
| `keynote.*` | `keynote.settings`, `keynote.table` | `standards/text_styles` | Ch 5 |
| `filter.*` | `filter.rule`, `filter.selection` | `views/filters` | Ch 5 |
| `colorscheme` | `colorscheme` | `views/color_schemes` | Ch 3, Ch 5 |
| `scopebox` | `scopebox` (name, rectangle, height, rotation, base level, visibility) | `views/index` | Ch 3, Ch 5 |
| `discipline.*` | `discipline.profile` | `standards/object_styles` | Ch 5 |
| `phase.*`, `option.*` | `phase`, `phase.filter`, `phase.override`; `option.set`, `option` | `project/phases`, `project/design_options` | Ch 2, Ch 5 |
| `revision.*` | `revision`, `revision.sequence` | `project/revisions` | Ch 5 |
| `link.*` | `link.ifc`, `link.cad`, `link.image`, `link.pdf`, `link.pointcloud` | `project/links` | Ch 6 |
| `monitor.*`, `clash.*`, `issue.*` | `monitor.pair`, `monitor.settings`; `clash.test`, `clash.result`; `issue.topic`, `issue.extensions` | `coordination/monitors`, `coordination/clash`, `coordination/issues` | Ch 6 |
| `assembly.*` | `assembly.type`, `assembly.instance`, `assembly.template` | `model/assemblies` | Ch 5 §5.16 |
| `part.*` | `part.division`, `part.merge` (the part element itself carries `recipe.part`) | `model/parts` | Ch 3 §3.6.11 |
| `numbering.*` | `numbering.rule` | `project/settings` | Ch 2 §2.10.16 |
| `decal.*` | `decal.type` | `standards/materials` | Ch 2 §2.10.15, Ch 5 |
| structural data | No separate kinds: project settings are the `settings` object of `model/structural`; connection status, part and assembly marks and precast provenance are fields of the `structural` element section | `model/structural`, `Reviera_Element` → `structural` | Ch 4 |

## 7. Integrity and Reconciliation (edits by other tools)

On open, Reviera checks:
1. **Manifest.** `DataId` is present. The hash of each document matches its `Sha256`. If a document is corrupt, it is opened read-only, reported, and can be restored from backup.
2. **Element data.** For each element with `Reviera_Element`, Reviera recomputes the hash of its current IFC representation and compares it with `GeomHash`:
   - **Equal:** the recipe is authoritative and the element stays fully parametric.
   - **Different:** the element was edited outside Reviera, and its recipe is marked **stale**. On the first edit, the user chooses between *keep external geometry* (the recipe is dropped and the element becomes a non-parametric IFC element) and *regenerate from recipe*.
3. **Dangling references.** For RXD references to GlobalIds that no longer exist:
   - annotation attached to them (tags, dimension references) is removed and listed
   - constraints that use them are removed
   - view element overrides that use them are removed
4. **Elements without data.** New elements with no `Reviera_Element` (created by other tools) are imported as **non-parametric** elements: selectable, editable as IFC geometry, schedulable.
   - **Datums that IFC fully describes** (grid axes) are the exception to steps 2 and 4: the IFC data wins without a choice, and axes added by other tools become Reviera grids (DM-IFC-006).
5. **Report.** A reconciliation report appears in the Warnings dialog and is stored as `project/reconcile_log`.

## 8. BIMserver Mode
- **Model data.** Embedded RXD travels **as ordinary IFC objects** in each check-in and download. No special transport is needed, and BIMserver's object-level storage, queries and merging keep it, because it is ordinary property sets. Each element's data stays attached to its element.
- **Merging (sync).**
  - IFC objects are merged by GlobalId.
  - RXD objects are merged by `id` in a three-way merge against the common base revision. Fields changed on only one side merge automatically; the same field changed on both sides is a conflict for the user to resolve; a delete on one side against an edit on the other is also a conflict.
  - Element-level RXD follows its element's merge result.
- **Live ownership and locks** (the worksharing "borrowing" equivalent). These change constantly and must not need new revisions, so they use BIMserver **Extended Data** on the latest revision. The calls below are confirmed in the public BIMserver `ServiceInterface` source:
  1. Register schema `org.reviera.ownership/1.0` (`addExtendedDataSchema`; look it up with `getExtendedDataSchemaByName`).
  2. Publish: `uploadFile`, then `addExtendedDataToRevision(roid, …)`.
  3. Read: `getLastExtendedDataOfRevisionAndSchema(roid, schemaId)`, then `getFile`.
  4. Refresh on notification, or poll every 30 seconds; concurrency is optimistic.
  5. Locks are advisory, matching what BIMserver supports. Reviera ships no enforcement plugin (TECH_STACK §5, decision 8).
- **External carrier with BIMserver.** If a project uses the external carrier, the `.reviera` file is attached to each revision as Extended Data (schema `org.reviera.rxd/1.0`).

## 9. Saving, Security, Performance, Versioning
- **Atomic save.**
  - Embedded mode: write to a temporary file, fsync it, then rename it over the original.
  - External mode: write both files to temporaries, then rename the IFC first and the sidecar second.
  - Rotating backups keep IFC and sidecar paired.
- **Security.**
  - RXD must not contain executable content. The one exception is project Lua macros, stored in document `scripts/<name>`. Macros are **disabled until the user trusts them**, and then run at the chosen Lua sandbox level.
  - Decoders enforce limits on decompressed size and chunk count.
  - The external ZIP carrier rejects path traversal.
- **Performance.**
  - RXD documents are decoded in parallel (rayon).
  - View annotation is loaded lazily, when its view is first opened. With the embedded carrier this still works: the IFC parser reads the `Reviera_Doc:views/annotation/<viewId>` property sets as raw strings, and decompression, JSON parsing and object construction are deferred until the view is opened. (This answers Chapter 5 question Q5-7.)
  - Large numeric arrays may use `zstd+base64` binary buffers, described in the style of glTF: `{ "buffer": "<doc>#bin0", "type": "f64", "count": n }`.
- **Versioning.** `FormatVersion` uses `MAJOR.MINOR`.
  - Minor versions only add things. Unknown kinds, fields and extension namespaces are **kept as-is on save**.
  - A major version change triggers a migration on open, and Reviera warns before saving a file that older versions can't read.
- **Add-in data.** Add-ins store their data under reverse-DNS namespaces in `ext` (for example `"org.example.acoustics"`), or in documents named `addins/<namespace>/…`, and must register a JSON Schema for it.

## 10. Compatibility Test Suite (required)

| Test | Pass condition |
|------|----------------|
| Schema validity | Every saved file passes the IfcOpenShell validator and the buildingSMART validation service: schema and normative rules |
| Round-trip: IfcOpenShell (Bonsai) | Open, edit an unrelated element, save; RXD Psets are intact and `GeomHash` is unchanged for the untouched elements |
| Round-trip: Open Source BIMserver | Check in, then download; every `Reviera_*` Pset is byte-identical after decoding |
| Round-trip: other IFC tools (a list kept in the test plan) | Record whether `Reviera_*` Psets are kept. Tools that strip them are documented with "use external carrier" guidance |
| Viewer behaviour | Common IFC viewers open embedded-RXD files without errors or warnings |
| Clean export | `strip_rxd` output contains no `Reviera_*` names and gives an identical building model |
| Reconciliation | A wall's geometry edited in a third-party tool is detected as stale; a deleted wall removes its tags and dimensions, with a report |

## 11. Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| DM-RXD-001 | Reviera shall store non-IFC data only as RXD documents and element data, using the schemas defined in this chapter. Read access for scripts and agents is `rxd.get_document(path) -> JSON`, which returns one decoded document by its canonical path (§6.4), and `rxd.get_element_data(element) -> JSON`, which returns the `Reviera_Element` or `Reviera_Type` payload of the element with that GlobalId. Both fail with `rxd.not_found` when the document or data is absent; neither can write. | P0 |
| DM-RXD-002 | By default, Reviera shall embed RXD in the IFC file using only `Reviera_*` property sets with `IfcText` and `IfcLabel` values, as in §3; the file shall remain schema-valid. `file.save(document_id?) -> status` writes the active (or named) document in its current carrier, atomically (§9). | P0 |
| DM-RXD-003 | Payload encoding shall follow §3.3: ASCII-only JSON or zstd+base64, chunks of at most 32,000 characters, canonical JSON, and a hash on each document. `rxd.validate() -> report` checks every document of the active project against its JSON Schema, its chunk sequence and its `Sha256`, and lists failures by document path without changing anything. | P0 |
| DM-RXD-004 | Reviera shall support the external carrier (`.reviera` file or folder, `.rvb` bundle) holding identical documents, chosen per project. The carrier is chosen with `file.save_as(document_id?, path, kind?, format?, carrier?) -> status`, where `carrier` is `embedded`, `external_file`, `external_dir` or `bundle`. When the lookup on open (§5) finds no sidecar, `file.locate_rxd(path) -> status` binds a `.reviera` file or folder chosen by the user; a sidecar whose `DataId` does not match the manifest is refused. | P1 |
| DM-RXD-005 | Reviera shall provide *Export clean IFC* and *Export IFC with documentation*. | P0 / P1 |
| DM-RXD-006 | Reviera shall detect external edits (`GeomHash`), dangling references and missing data, and reconcile them with a report (§7). `rxd.reconcile(mode: report\|apply, drop_orphans?=true) -> report` runs the checks on demand: `report` only lists findings, `apply` performs the removals as one undoable transaction and appends to `project/reconcile_log`. | P0 |
| DM-RXD-007 | Unknown RXD kinds, fields and extension namespaces shall survive a round trip unchanged. | P0 |
| DM-RXD-008 | In BIMserver mode, RXD shall travel inside the IFC; ownership state shall use Extended Data (§8); sync shall perform the three-way RXD merge. | P1 |
| DM-RXD-009 | Project macros stored in RXD shall be disabled until trusted. `rxd.trust_scripts(scope: once\|always, sandbox level) -> status` records the user's decision for the current document GUID and script hashes; `always` is remembered until a script changes, and the level can never exceed the user's maximum in Script Security (Ch 2 §2.13.7). | P0 |
| DM-RXD-010 | The compatibility test suite (§10) shall run in CI on every release. | P0 |
| DM-RXD-011 | The RXD JSON Schemas shall be published under the **MIT licence** (owner decision, 2026-10-12), separately from the GPL-3.0-or-later application, so that any tool, including proprietary ones, can read and write RXD. The format definition (this chapter) is published with them as the RXD package. See §12. | P1 |

**Acceptance criteria**
- [ ] A Reviera project saved by default is a single `.ifc` file that passes schema validation and reopens in Reviera with all views, sheets and parametric intent restored.
- [ ] The same file opens in a third-party IFC viewer without errors, showing the complete building.
- [ ] Re-saving an unchanged project produces identical RXD payload bytes (deterministic output).
- [ ] A document of 10 MB is chunked, and reassembles to the same `Sha256`.
- [ ] A project switched to the external carrier and back is identical in content.

## 12. Licensing of the RXD Schemas

- **Licence.** The RXD JSON Schemas are licensed under the **MIT licence** (SPDX `MIT`). The rest of Reviera remains GPL-3.0-or-later.
- **Repository layout.**
  - The schemas live in a self-contained directory, `schemas/rxd/`, with its own `LICENSE` file (MIT text, `Copyright (c) <year> Reviera contributors`) and a `README.md` stating the scope of the licence.
  - Each schema file carries `"$comment": "SPDX-License-Identifier: MIT"`.
  - The schemas are also released as a separate versioned package (for example `reviera-rxd-schemas-1.0.zip`, plus an npm/crates package if useful).
- **Scope of the MIT grant.** It covers the JSON Schema files, the example documents and the format description that ships with them (this chapter, exported as the package `README`/`SPEC.md`).
  - Not covered: Reviera's Rust code that reads and writes RXD. That code stays GPL-3.0-or-later. Rust types *generated* from the schemas inside the Reviera repository are part of the GPL application.
  - Third parties may generate their own code from the MIT schemas under any licence.
- **Contributions.** Changes to `schemas/rxd/` are accepted under MIT. The CI licence check enforces SPDX `MIT` for that path and `GPL-3.0-or-later` everywhere else.
- **Compatibility.** MIT-licensed files inside a GPL-3.0-or-later project are fully compatible: the combined application is distributed under the GPL, and the schema files keep their MIT terms when distributed on their own.
- **Schema identifiers.** `$id` URIs use `https://nodeskreviera.org/schemas/rxd/<major>.<minor>/<name>.schema.json` (owner, 2026-10-12, DM-RXD-OD2; one-word domains were taken). The domain is provisional until the project registers it, which must happen before 1.0 ships, and the URLs must then resolve to the published MIT schemas. Once a schema version is released its `$id` never changes, so the domain is effectively permanent from 1.0 on.

## 13. Commands

| Command | Parameters | Returns | Undoable | Req ID |
|---------|------------|---------|----------|--------|
| `file.save` | document_id? (default: active) | status | No | DM-RXD-002 |
| `file.save_as` | document_id? (default: active), path, kind?, format?: ifc\|ifczip, carrier?: embedded\|external_file\|external_dir\|bundle | status | No | DM-RXD-004 |
| `export.ifc` (options defined here; base command in Ch 2 DM-FILE-004) | path, setup? (name, id or inline settings), settings_override?, schema?, strip_rxd?=false, documentation?, background? | job_id (the job result carries the export report) | No | DM-RXD-005 |
| `file.locate_rxd` | path to an external `.reviera` file or folder | status | No | DM-RXD-004 |
| `rxd.validate` | — | report | No | DM-RXD-003 |
| `rxd.reconcile` | mode: report\|apply, drop_orphans?=true | report | Yes | DM-RXD-006 |
| `rxd.get_document` | path | JSON | No (read) | DM-RXD-001 |
| `rxd.get_element_data` | element (GlobalId) | JSON | No (read) | DM-RXD-001 |
| `rxd.trust_scripts` | scope: `once`/`always`, sandbox level | status | No | DM-RXD-009 |

These are also exposed read-only as the MCP resources `reviera://rxd/{path}` and `reviera://element/{guid}/rxd`. Writes always go through domain commands, never through raw document editing.
