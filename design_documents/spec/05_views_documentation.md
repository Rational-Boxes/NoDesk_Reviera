# Chapter 5 — Views, Graphics, Annotation, Schedules, Sheets and Output

**Area code:** DOC · **Status:** Draft 1 (2026-10-12) · **Primary source:** `research/04_documentation_collaboration.md` (cited as `research/04 §n`)
**Related chapters:** Chapter 1 (UI shell: ribbon, Project Browser, View Control Bar, keymap), Chapter 2 (DM: data model, families, parameters, formulas, file/IO), Chapter 2a (Reviera Extension Data, RXD), Chapter 3 (ARC: datums, rooms, areas), Chapter 6 (COL: worksharing display, links, positioning, BCF).
**Reference:** Autodesk Revit 2026/2027 documentation and output features, used as the functional reference only.

This chapter specifies how Reviera turns its IFC model into drawings and deliverables. It covers every view type, the rules that decide how each element is drawn in each view, model-reporting and drafting annotation, schedules, sheets, and every output channel. It also specifies how documentation objects persist inside the IFC file, and the hidden-line and GPU pipelines that produce the pictures.

---

## 5.0 Core principles

These principles apply to every requirement in this chapter.

| ID | Principle | Priority |
|----|-----------|----------|
| DOC-CORE-001 | **Views are live projections.** A view stores only its *definition* (type, extents, camera, graphics settings, view-owned annotation). Projected model linework, cut poché and shaded images are always derived, and they are regenerated when the model changes. Derived results may be cached, but the cache is never the source of truth. Cache entries are keyed by element revision and view-definition revision (DOC-HLR-001); a cache miss or a corrupt entry only costs regeneration time, never a wrong drawing. Closing and reopening a project discards no definition data and may discard every cache. The only stored derived result is a saved rendering image (DOC-OUT-011). | P0 |
| DOC-CORE-002 | **Paper-space sizing.** Annotation symbols, text, tag heads, dimension text, line patterns, drafting fill patterns and annotation line weights are specified in paper millimetres. They render at `paper size × view scale` in model space, so that changing the view scale resizes them automatically. Model patterns (DOC-GFX-014), model text and model lines are the exception: they are sized in real units. Annotation positions are stored in view coordinates, so a scale change keeps each tag's anchor on its host while its head grows or shrinks around that anchor; leaders are re-elbowed, never re-routed. Sheets are always 1:1, so sheet annotation is the same in paper and model space. | P0 |
| DOC-CORE-003 | **Model-reporting annotation is associative.** Tags, dimensions, spot dimensions and element/material keynotes bind to model references (element GlobalId plus a stable geometric sub-reference). They update when the referenced element changes and become *orphaned* (kept, flagged, drawn in a warning colour) when the reference disappears. A reference is re-resolved on every regeneration of the view; when a sub-reference cannot be found but the element still exists, Reviera tries the named sub-reference rules of Chapter 2a (for example the same face role on a split wall) before orphaning. An orphan can be re-hosted with *Pick new host* or deleted; it never silently attaches to another element. | P0 |
| DOC-CORE-004 | **View-owned elements.** Text, detail lines, regions, detail components, revision clouds and similar drafting items belong to exactly one view (or sheet) and appear nowhere else, except through dependent views (DOC-VIEW-011). Deleting a view deletes its view-owned elements in the same undo step; the confirmation states how many there are. Copying them to another view is an explicit act (clipboard *Paste aligned to selected views*, *Duplicate with Detailing*, or detail groups) and always creates new objects with new ids. Model elements are never view-owned. | P0 |
| DOC-CORE-005 | **One sheet per model view.** A graphical model view (plan, section, elevation, callout, 3D, drafting, rendering) can be placed on at most one sheet. Schedules, legends, keynote legends, sheet lists, view lists and note blocks can be placed on any number of sheets. The rule is checked by `sheet.place_view`, `viewport.swap_view` and `viewport.move_to_sheet`, which fail with `view.already_on_sheet` (naming the sheet) instead of placing a second copy. To show the same area twice, the user duplicates the view, usually as a dependent (DOC-VIEW-011). Assembly views follow the same rule (§5.16). | P0 |
| DOC-CORE-006 | **One draw list, many outputs.** Every 2D result (on-screen hidden-line view, print, PDF, DXF/DWG, raster image) is produced from the same intermediate *draw list* (DOC-HLR-004). Output is what you see on screen, apart from the documented print options (DOC-OUT-001). The differences allowed between screen and output are exactly: temporary display state (DOC-GFX-008, temporary view properties, reveal hidden, displacement highlight) never prints; *Thin lines* is screen-only; the print options hide or recolour the items they name; raster processing (DOC-HLR-007) changes shaded content only. Any other difference is a defect. | P0 |
| DOC-CORE-007 | **Everything is a command.** Every creation, edit and output operation in this chapter is a registered command, so it is reachable from the ribbon, Lua and MCP. Read-only queries (for example `graphics.explain`) are commands too, flagged as not undoable. Commands that open a dialog (for example `template.manage`, `revision.manage`) are UI entry points only: every change made in the dialog is applied through the undoable commands of the same requirement, so a dialog session that changes five things records those five commands as one undo step named after the dialog. Without a UI (Lua, MCP, headless), dialog commands fail with `ui.unavailable`. | P0 |
| DOC-CORE-008 | **RXD persistence.** Every documentation object defined here persists as Reviera Extension Data (RXD, §5.1, Chapter 2a), embedded in the IFC file by default and keyed to the model by IFC GlobalIds. A project opened locally or downloaded from BIMserver therefore reproduces every view, sheet and annotation exactly, and the file remains valid standard IFC. When another tool has deleted or changed elements that RXD refers to, the reconciliation pass of Chapter 2a §7 runs on open: references to missing elements become orphans (DOC-CORE-003), and nothing is deleted without a report. Session-only state (temporary hide/isolate, highlight modes, render history) is never written to RXD. | P0 |

**Acceptance criteria (core).**
- [ ] Changing a wall's thickness updates every plan, section, schedule and sheet that shows it, without the user refreshing anything.
- [ ] Changing a plan from 1:100 to 1:50 halves the model-space height of every text note in it while its printed height stays the same.
- [ ] Deleting a tagged door leaves the tag in the view marked as orphaned, and the warnings list reports it.
- [ ] Saving a project with views, sheets, schedules and annotation, closing it and reopening it gives identical PDF output (byte-identical page content streams, excluding timestamps).

---

## 5.1 Persistence of documentation objects

**ID:** DOC-IFC-001 · **Priority:** P0 · **Ref:** research/04 §11; TECH_STACK §3.1, §3.7.3; spec/02a_extension_data_format.md

**Purpose.** Views, sheets, schedules, graphic standards and annotation are not building data, and IFC entities cannot express them without loss. Reviera stores them as **Reviera Extension Data (RXD)**: versioned JSON documents with stable object ids, defined in Chapter 2a (`02a_extension_data_format.md`). By default RXD is embedded in the IFC file as valid, standard IFC, so a single `.ifc` file carries the whole project. Chapter 2a defines the carrier (property sets, payload encoding, the alternative external carrier), the object envelope, the reference syntax and the merge rules. This chapter does not redefine any of that. It only names the RXD documents this chapter uses, and specifies how documentation is *exported* as IFC annotation for other tools.

**Behaviour & rules.**
- **Loading.** On open, `views/index`, `sheets/index`, `views/templates`, `views/filters`, `views/types`, `views/color_schemes`, the `standards/*` documents and the `project/*` documents load at once. `views/<viewId>`, `views/annotation/<viewId>`, `schedules/<scheduleId>` and `sheets/<sheetId>` load lazily, the first time the view, sheet or schedule is opened, printed, exported, scheduled (view lists, sheet lists) or queried through Lua or MCP.
- **Saving.** Reviera tracks changes per RXD document. On save, only changed documents are serialised again; unchanged documents are written byte-identical, which keeps BIMserver revisions and text diffs small.
- **Damaged documents.** A document that fails its schema check on load is quarantined: its view, sheet or schedule is listed as *damaged* in the Project Browser and cannot be opened, its raw JSON is kept unchanged and written back unchanged on save, and the problem is listed in the warnings list. The rest of the project opens normally. `rxd.status` (Chapter 2) reports quarantined documents.
- **Versions.** Each document carries the RXD format version (Chapter 2a). Unknown fields from a newer minor version are kept and written back. A document from a newer major version opens read-only, with a banner naming the version.
- **Deleting model elements** never deletes RXD annotation that refers to them; the annotation becomes an orphan (DOC-CORE-003). `rxd.reconcile` (Chapter 2a) can report or remove orphans in bulk.

**Commands.** Persistence has no commands of its own: saving and opening use `file.save`, `file.save_as` and `file.open` (Chapter 2), and inspection uses `rxd.get_document`, `rxd.status` and `rxd.reconcile` (Chapters 2 and 2a). Acceptance criteria are in §5.1.3.

### 5.1.1 What lives where

| Data | Store | Notes |
|---|---|---|
| Elements, types, materials, levels, grids, spatial structure, standard Psets/Qtos, parameter values | IFC entities | Chapter 2. Never duplicated in RXD. |
| Element-scoped documentation data (phase created/demolished, design option membership, per-element display flags) | element-scoped RXD on the element (Chapter 2a) | Also written as the standard `Status` property on export (DOC-GFX-017). |
| Views (all kinds), view range, crop, camera, section box, per-view overrides, hidden elements | RXD documents `views/index` and `views/<viewId>` | One document per view, loaded lazily when the view opens. |
| Navigation state: each view's latest camera and Home (UI-NAV-001) | RXD document `views/state` | Written with the next save; never makes the model modified. |
| View templates, view filters, selection sets | RXD documents `views/templates`, `views/filters`, `model/selection_sets` | |
| View-owned annotation (dimensions, spot dimensions, text, tags, keynotes, symbols, detail lines, regions, detail components, repeating details, insulation, revision clouds, plan regions, colour fill legends, matchlines, view references, elevation markers, legend components) | RXD document `views/annotation/<viewId>` | Kinds `anno.*`. |
| Object styles, line weights, line styles, line patterns, fill patterns, text/dimension/tag styles, halftone settings, detail-level map, discipline profiles | RXD documents `standards/*` | |
| Schedules (all kinds, including keynote legends, sheet lists, view lists, note blocks) | RXD document `schedules/<scheduleId>` | |
| Sheets, title block instances, viewports, schedule instances, sheet annotation, guide grids, saved positions, sheet collections | RXD documents `sheets/index` and `sheets/<sheetId>` | |
| Revisions and revision settings | RXD document `project/revisions` | |
| Phases, phase filters, phase graphics | RXD document `project/phases` | |
| Design option sets and options | RXD document `project/design_options` | |
| Print, PDF, image and export setups; view/sheet sets | RXD document `project/settings` (section `output`) | |
| Images (title block logos, image underlays, saved renderings, schedule images) | RXD assets (Chapter 2a) | |

**Rules.**
- Every documentation object in this chapter is an RXD object with the common envelope (stable `id`, `kind`, per-object `rev`, `name`, `data`, `ext`). References to model objects use IFC GlobalIds, with named geometric sub-references (for example `#face:core.ext`) for dimensions and linework overrides. References between RXD objects use their stable ids. Index-based topology references are never stored.
- Each object kind used below is registered in the Chapter 2a kind catalogue with a JSON Schema before implementation. The **Data** paragraph of each requirement names the RXD document and kind, and lists the payload fields this chapter depends on; the schema is normative.
- Derived results (hidden-line linework, cut poché, schedule cell values, rendered pixels) are never stored in RXD. Saved renderings (DOC-OUT-011) are the one exception: they are stored as image assets.
- Because RXD is merged per object by stable id (three-way, Chapter 2a and Chapter 6 COL-MRG-001), the documents are split so that two users editing different views, sheets or annotation do not conflict.

### 5.1.2 IFC export of documentation as IFC annotation

**ID:** DOC-IFC-002 · **Priority:** P1 · **Ref:** research/04 §6.4 (*Export 2D plan view elements*)

**Purpose.** Other IFC tools do not read RXD. Viewers, facility-management tools and other authoring tools may still want drawings and annotation. The export setup option *Include documentation as IFC annotation* (DOC-OUT-006) writes them as standard IFC4 presentation entities. This is an interoperability export: Reviera never treats it as its own store. When Reviera opens a foreign IFC containing such annotation, it shows that annotation read-only as imported drafting (Chapter 6, COL-LNK-001).

**Behaviour & rules.** When the option is on, for each view selected in the export setup:
- **Contexts.** Reviera creates `IfcGeometricRepresentationSubContext` instances under the `Plan` and `Model` contexts with `ContextIdentifier = 'Annotation'`, a `TargetView` of `PLAN_VIEW`, `REFLECTED_PLAN_VIEW`, `SECTION_VIEW`, `ELEVATION_VIEW` or `MODEL_VIEW`, and `TargetScale` equal to the view's scale (for example `0.01` for 1:100). Sheets, if exported, use a sub-context with `TargetView = USERDEFINED` and `UserDefinedTargetView = 'Sheet'`, in paper millimetres.
- **Grouping.** Each exported view becomes an `IfcGroup` (`ObjectType = 'DRAWING'`, `Name` = view name) whose members (`IfcRelAssignsToGroup`) are that view's `IfcAnnotation` objects. Plan annotation may optionally be contained in the view's storey (`IfcRelContainedInSpatialStructure`).
- **Annotation objects.** Each annotation becomes one `IfcAnnotation`. In IFC4, its class is given by `ObjectType` (`DIMENSION`, `TEXT`, `TAG`, `SYMBOL`, `DETAIL_LINE`, `FILLED_REGION`, `REVISION_CLOUD`, `MATCHLINE`, …). In IFC4X3 exports, `PredefinedType` is set to the matching `IfcAnnotationTypeEnum` value where one exists, otherwise `USERDEFINED` with the same `ObjectType`. Model-reporting annotation is related to the element it reports on with `IfcRelAssignsToProduct` (RelatingProduct = the element), so consumers can tell which door a tag belongs to.
- **Curves.** Lines, polylines, arcs and splines are items of an `IfcGeometricCurveSet`, using `IfcPolyline`, `IfcIndexedPolyCurve` (with arc segments), `IfcTrimmedCurve` of `IfcCircle`/`IfcEllipse`, and `IfcBSplineCurveWithKnots`. Representation identifier `Annotation`; representation type `Annotation2D`, or `GeometricCurveSet` when only curves are present.
- **Fills.** Regions and poché use `IfcAnnotationFillArea` (outer and inner boundaries), styled by `IfcFillAreaStyle` containing `IfcFillAreaStyleHatching` (one per hatch line family), `IfcColourRgb` for solid and background fills, or `IfcFillAreaStyleTiles` where hatching cannot express the pattern.
- **Line appearance.** `IfcStyledItem` → `IfcCurveStyle` with `CurveWidth` (paper mm scaled by the context scale), `CurveColour`, and `CurveFont` as an `IfcCurveStyleFont` built from the line pattern's dash and gap segments.
- **Text.** `IfcTextLiteralWithExtent` (literal, placement, path, `IfcPlanarExtent`, box alignment) styled by `IfcTextStyle` with `IfcTextStyleFontModel`. Rich text is flattened to plain text, one literal per line.
- **Symbols.** Repeated symbols (tag heads, arrowheads, section heads, title block artwork) use `IfcRepresentationMap` / `IfcMappedItem`.
- **Images.** Logos and images use `IfcBlobTexture` (PNG or JPEG) on an `IfcAnnotationFillArea`, through `IfcSurfaceStyleWithTextures`.
- **Drawing linework.** With the setup option *Include 2D linework*, the view's computed draw list (DOC-HLR-004) is also written: one `IfcAnnotation` (`ObjectType = 'DRAWING_LINEWORK'`) per view, whose representation holds one `IfcGeometricCurveSet` per distinct line style (each with its `IfcStyledItem`), plus `IfcAnnotationFillArea` items for cut poché. Tools without hidden-line capability then get a finished 2D drawing.
- **Other documentation data** written on export: phase status as the standard `Status` property (DOC-GFX-017); design options and worksets as `IfcGroup` (DOC-GFX-018; Chapter 6).
- The same export setup can also *strip RXD* (a "clean" exchange file without Reviera data, DOC-OUT-006).

**Acceptance criteria.**
- [ ] An IFC4 file exported with *Include documentation as IFC annotation* validates with IfcOpenShell's schema validator with no errors.
- [ ] A third-party IFC viewer that displays `Annotation` sub-contexts shows the exported floor plan's dimensions and text at the correct position and size.
- [ ] Each exported door tag is related to its door by `IfcRelAssignsToProduct`.
- [ ] With *Include 2D linework* on, the exported plan linework overlays the model plan with no visible offset.
- [ ] Opening a stripped export in Reviera creates no native annotation; the exported annotation appears only as read-only imported drafting.

### 5.1.3 Acceptance criteria (persistence)
- [ ] A saved project is one valid IFC file that, when reopened, restores every view, sheet, schedule and annotation (RXD embedded by default).
- [ ] Switching the project to the external RXD carrier (Chapter 2a) and back gives identical documentation.
- [ ] Opening a project with 500 views opens only the views that were open at the last save; `rxd.status` shows the other view documents as not loaded.
- [ ] A view document edited by hand so that it fails its schema opens as *damaged*, the rest of the project opens, and saving writes the damaged JSON back unchanged.
- [ ] Adding one text note to one view changes only that view's `views/annotation/<viewId>` RXD document (and the RXD index metadata defined in Chapter 2a).
- [ ] Third-party IFC tools open a Reviera project file without errors, ignoring the RXD.

---

## 5.2 Common view model

### Shared view properties and the Project Browser
**ID:** DOC-VIEW-001 · **Priority:** P0 · **Ref:** research/04 §1 (intro table), §11; web: Edit Type Properties for View Tags, About Section Tag Visibility, Hide Section Tags, Find a Referring View

**Purpose.** Give every graphical view one consistent set of properties, so that templates, schedules (View List), filters, Lua and MCP can treat views uniformly.

**Workflow.**
1. The user creates a view with a View-tab tool, or by duplicating an existing view (DOC-VIEW-011).
2. The view appears in the Project Browser in the node chosen by the active browser organisation scheme.
3. With no element selected, the Properties palette shows the active view's properties; edits apply at once and regenerate the view.

**UI.** View tab › Create panel: Plan Views ▾, Elevation ▾, Section, Callout ▾, 3D View ▾, Drafting View, Legends ▾, Schedules ▾, Scope Box, Duplicate View ▾. Project Browser context menu: Open, Duplicate View ▸, Apply/Create Template, Rename, Delete, Add to Sheet, Browser Organization. View Control Bar (Chapter 1): scale, detail level, visual style, sun/shadows, crop toggles, temporary hide/isolate, reveal hidden, temporary view properties, worksharing display, isolate annotations.

**Behaviour & rules.**
- Properties shared by all graphical views:

| Group | Property | Values / notes |
|---|---|---|
| Graphics | Scale | Preset list (1:1 to 1:5000, plus imperial presets) or Custom (any positive ratio). Drives paper-space sizing (DOC-CORE-002). |
| | Display Model | Normal / Halftone / Do not display. "Do not display" leaves only annotation and view-owned elements visible. |
| | Detail Level | Coarse / Medium / Fine (DOC-GFX-010). |
| | Parts Visibility | Show Parts / Show Original / Show Both. |
| | Visibility/Graphics | Opens the view's graphics dialog (DOC-GFX-003). |
| | Graphic Display Options | Opens DOC-GFX-012. |
| | Discipline | Architectural / Structural / Mechanical / Electrical / Plumbing / Coordination. Each discipline has a category visibility profile and halftones non-discipline categories (DOC-GFX-016). |
| | Show Hidden Lines | By Discipline / All / None. |
| | Color Scheme Location, Color Scheme | Background / Foreground; scheme per space-like category (DOC-ANN-022). |
| | Sun Path | On / Off (3D and plans). |
| Underlay (plans) | Base Level, Top Level, Orientation | DOC-VR-004. |
| Extents | Crop View, Crop Region Visible, Annotation Crop, View Range (plans), Far Clipping and Far Clip Offset (sections, elevations), Depth Clipping (plans), Scope Box, Section Box (3D) | |
| Identity | View Template, View Name, Dependency, Title on Sheet, Sheet Number and Sheet Name (read-only), Referencing Sheet / Referencing Detail (read-only), Sheet Collection / Referencing Sheet Collection (read-only) | |
| Phasing | Phase, Phase Filter | DOC-GFX-017. |

- View names are unique within a view kind (plans of the same plan type, sections, 3D views and so on). Renaming to a duplicate name is rejected with a clear message.
- **Browser organisation** schemes group and sort views and sheets by any view or sheet parameter (for example Discipline, then View Type, then Phase). A scheme is a project-level definition; the active scheme is a user preference. Schemes persist in RXD document `standards/browser`.
- Every view has a hidden, stable *view kind* (enumeration) and a *view family type* (user-defined types per kind, for example "Working Plan" or "Presentation Plan"). The type supplies the default template for new views, whether new views are bound to that template, and the marker symbol (callout tag, section head, elevation mark).
- Per-user views: the default 3D view is created per user in BIMserver mode (`{3D - <user>}`), because one user orbiting a shared view must not change another user's view.
- **View tag settings.** Manage › Additional Settings ▾ › Callout Tags / Elevation Tags / Section Tags (`view_tag.settings`) opens a type editor for the *view tag types* that marker symbols use. The dialog lists the tag types of the chosen kind (Duplicate, Rename, Delete; a type in use cannot be deleted until its users are moved to another type) and edits:
  - *Section tags*: section head symbol (an annotation family), section tail symbol (a symbol or *None*), and the broken-line display used by *Gap* mode (DOC-VIEW-004);
  - *Callout tags*: callout head symbol and the corner radius of the callout boundary (paper mm, 0 to 50);
  - *Elevation tags*: the marker body symbol (round, square or a custom annotation family) and the direction arrow symbol.
  Symbols are chosen from loaded annotation families of the matching category; a family whose labels lack the detail and sheet number is accepted with a warning. Edits are type edits: every marker of that type redraws at once. View family types (Section, Detail, Callout and Elevation types) refer to a view tag type, so the tag type is shared by all view types that use it.
- **View tag visibility.** A section, callout or elevation marker is drawn in another view only when the marked view's extents intersect that view's range (plans: the view range slab; sections and elevations: the far clip volume), and when the viewing scale is not coarser than the marker's *Hide at scales coarser than* value. Print option *Hide unreferenced view tags* (DOC-OUT-001) drops markers whose view is not on a sheet.
- **Find referring views.** Project Browser › right-click a view › *Find Referring Views* (`view.find_referring`) lists every view whose marker, callout boundary or view reference symbol points to the chosen view, and every sheet that holds it. The list opens in a *Go to View* dialog: double-click (or *Open*) opens the chosen view or sheet and zooms to the marker. A view nothing refers to gives an empty list with the message "No views refer to this view".

**Data (RXD).** RXD documents `views/index` (id, kind, name, template, browser path) and `views/<viewId>` (kinds `view.plan`, `view.ceiling`, `view.structural`, `view.area`, `view.section`, `view.elevation`, `view.callout`, `view.3d`, `view.camera`, `view.drafting`, `view.legend`, `view.walkthrough`, `view.rendering`). Common payload: `viewFamilyType`, `scale`, `displayModel`, `detailLevel`, `partsVisibility`, `discipline`, `showHiddenLines`, `colorSchemeLocation`, `colorSchemes`, `phase`, `phaseFilter`, `template`, `parent` (dependent views), `titleOnSheet`, `visualStyle`, `crop`, `annotationCrop`, `scopeBox`, `orientation`, and the view's coordinate frame (origin, x and y axes, view direction). View family types: RXD document `views/types`, kind `view.familytype` (with `tagType`, the id of its view tag type). View tag types: RXD document `views/types`, kind `view.tagtype` (`tagKind`: section | callout | elevation, `headSymbol`, `tailSymbol`, `cornerRadius`, `arrowSymbol`). Browser schemes: RXD document `standards/browser`.

**Commands.**
`view.set_property(view_id, name, value) -> ()`
`view.rename(view_id, new_name) -> ()`
`view.delete(view_ids[]) -> ()`
`view.open(view_id) -> ()`
`view.list(filter?) -> view_summary[]`
`viewtype.create(kind, name, default_template_id?, bind_new_views?, marker_type_id?) -> view_family_type_id`
`browser.set_organization(target: views|sheets, scheme_id) -> ()`
`browser.create_organization(name, target, group_by[], sort_by) -> scheme_id`
`view_tag.settings(kind?: callout|elevation|section) -> ()` (opens the view tag type editor described above; the edits it makes are type edits through `type.set_param`, Chapter 2)
`view.find_referring(view_id) -> referrer[]` (query; each entry is `{id, kind: view|sheet, via: marker|callout|viewref|viewport, marker_id?}`)

**Acceptance criteria.**
- [ ] Every property in the table above can be read and written through `view.set_property` from Lua and from MCP.
- [ ] Changing the head symbol of a section tag type redraws every section marker of that type in every view, in one undo step.
- [ ] `view.find_referring` on a detail view referenced by two sections and placed on sheet A501 returns the two host views and A501.
- [ ] Renaming a view to an existing name of the same kind fails with an error naming the clash; no change is made.
- [ ] Switching browser organisation from "by type" to "by Discipline, then type" regroups the browser without changing any view.

---

## 5.3 Plan views

### Creating plan views
**ID:** DOC-VIEW-002 · **Priority:** P0 · **Ref:** research/04 §1.1

**Purpose.** Create horizontal cut views (floor plan, reflected ceiling plan, structural plan, area plan) associated with a level.

**Workflow.**
1. View tab › Create › Plan Views ▾ › Floor Plan / Reflected Ceiling Plan / Structural Plan / Area Plan.
2. In the New Plan dialog the user picks the view family type, one or more levels, and the scale. By default, levels that already have a plan of that type are not listed; clearing *Do not duplicate existing views* lists them too.
3. For an area plan the user also picks an area scheme (Chapter 3); Reviera offers to generate area boundary lines from walls.
4. OK creates one view per level and opens the last one.

**UI.** New Plan dialog: Type selector, Edit Type…, level list (multi-select), *Do not duplicate existing views*, Scale. The level tool (Chapter 3) has a *Make plan view* option that calls the same command.

**Behaviour & rules.**
- A floor plan and a structural plan look down. A reflected ceiling plan looks up from its cut plane and is drawn mirror-correct (as if the ceiling were reflected in a mirror on the floor), so its left and right agree with the floor plan.
- Structural plans default to a cut plane below the level, so that framing supporting the level is visible, and to the Structural discipline.
- Each new view gets its view range defaults from its view family type (DOC-VR-001), and its template from the type's *template for new views*.
- Deleting a level deletes its plan views after a confirmation that lists them.

**Data (RXD).** `views/<viewId>` of kind `view.plan`, `view.ceiling`, `view.structural` or `view.area`, with `level` (`ifc:` GlobalId of the `IfcBuildingStorey`) and `areaScheme` (area plans).

**Commands.** `view.create_plan(kind, level_ids[], view_family_type_id?, scale?, allow_duplicates?, area_scheme_id?) -> view_id[]`

**Acceptance criteria.**
- [ ] Selecting three levels creates three floor plans, named after their levels, in one undo step.
- [ ] A reflected ceiling plan of the same level shows a door at the same paper position as the floor plan.
- [ ] Creating an area plan with *generate boundaries* produces area boundary lines on the exterior wall faces chosen by the area scheme rules (Chapter 3).

### View range definition
**ID:** DOC-VR-001 · **Priority:** P0 · **Ref:** research/04 §1.1 *View Range* · screenshots/documentation/view_range_dialog.png · screenshots/documentation/view_range_section_diagram.png

**Purpose.** Define the vertical slice of the model that a plan shows: the cut plane, and the range above and below it.

**Workflow.**
1. Properties › Extents › View Range › Edit… opens the View Range dialog.
2. For each of the four planes (Top, Cut Plane, Bottom, View Depth), the user picks a level reference and an offset.
3. A *Show diagram* toggle expands an explanatory section diagram that shows the four planes against the current level stack (drawn by Reviera; no reference artwork).
4. Apply or OK validates and regenerates.

**UI.** View Range dialog: four rows (Plane, Level reference drop-down, Offset field); level references are *Associated Level*, *Level Above*, *Level Below*, *Unlimited* (Top and View Depth only) and every named level. Apply / OK / Cancel, diagram toggle.

**Behaviour & rules.**
- Each plane resolves to an absolute elevation `z = elevation(level_ref) + offset`. *Unlimited* resolves to +∞ (Top) or −∞ (View Depth). *Level Above/Below* resolve against the sorted level list at regeneration time, so adding a level in between changes the result.
- **Ordering constraint (floor and structural plans):** `Top ≥ Cut ≥ Bottom ≥ ViewDepth`. **Ceiling plans:** the range is mirrored: `Bottom ≤ Cut ≤ Top`, and View Depth lies *above* Top (`ViewDepth ≥ Top`).
- A combination that violates the ordering is rejected in the dialog (the offending row is highlighted) and through commands (error `view_range.invalid_order`). If a later level move makes a saved range invalid, the view keeps its previous valid result, shows a warning banner, and lists the problem in the warnings list until fixed.
- Defaults for new floor plans: Top = Level Above, offset 0 (or Associated Level + 2300 mm when there is no level above); Cut = Associated Level + 1200 mm; Bottom = Associated Level, offset 0; View Depth = Associated Level, offset 0. Defaults are editable per view family type.
- Selecting a plan's level marker in a section or elevation offers *Show view range*, which draws the four planes of that plan as temporary graphics in the section.

**Data (RXD).** `viewRange` block of the view payload: `top`, `cut`, `bottom`, `depth`, each `{level, offset}`, where `level` is an `ifc:` storey reference or one of the tokens `associated`, `above`, `below`, `unlimited`.

**Commands.** `view.set_view_range(view_id, top, cut, bottom, depth) -> ()` where each plane is `{level_ref, offset}`; `view.get_view_range(view_id) -> {planes, resolved_elevations}`.

**Acceptance criteria.**
- [ ] Setting Cut above Top is rejected with `view_range.invalid_order` and nothing changes.
- [ ] Inserting a level between a plan's level and the next one changes the result of a *Level Above* top plane on the next regeneration.
- [ ] `view.get_view_range` returns resolved elevations that match the dialog.

### View range classification algorithm
**ID:** DOC-VR-002 · **Priority:** P0 · **Ref:** research/04 §1.1 *View Range* rules (a)–(d) · screenshots/documentation/view_range_cut_plane_diagram.png · screenshots/documentation/view_range_view_depth_diagram.png

**Purpose.** Decide, deterministically and per element, which parts of an element a plan shows and with which graphic treatment.

**Behaviour & rules.** For a floor or structural plan with resolved elevations `T ≥ C ≥ B ≥ D` (for a ceiling plan, mirror the vertical axis and swap the roles of above and below, then apply the same rules), the plan is generated as follows.

1. **Candidate selection.** Query the spatial index for elements whose bounding box overlaps the plan's crop prism (crop polygon extruded from D to T, or from D − *floor tolerance* when step 6 applies). Drop elements that the graphics stack hides (§5.11) before any geometry work.
2. **Zones.** The vertical axis is split into zones:
   - *Above* (`z > T`): never shown, except for symbolic plan linework (step 7).
   - *Upper* (`C < z ≤ T`): primary range above the cut plane.
   - *Cut* (`z = C`).
   - *Lower* (`B ≤ z < C`): primary range below the cut plane.
   - *Beyond* (`D ≤ z < B`): the view depth.
   - *Below* (`z < D`).
3. **Cut.** For an element whose solid geometry crosses C, and whose category is *cuttable* (walls, floors, roofs, ceilings, columns, structural framing, stairs, railings, doors, windows, generic models, casework, curtain panels and mullions, MEP segments and others, as set in the category catalogue of Chapter 2): compute the planar section of the solid at C (§5.12, DOC-HLR-002). Draw the section's boundary with the element's *cut line* style and fill its interior with the *cut pattern*. Compound elements are cut layer by layer, and each layer uses its material's cut pattern. A non-cuttable category (for example furniture, plumbing fixtures, specialty equipment) that crosses C is drawn in projection, as if it were entirely in the Lower zone.
4. **Lower zone.** Geometry in the Lower zone is projected straight down onto the view plane and run through hidden-line removal together with everything else visible from above. Visible edges use the *projection line* style; surfaces may carry the *surface pattern* (model patterns show at their real size).
5. **Upper zone.** For each category the catalogue defines an *above-cut behaviour*:
   - `Hide` (the default for most categories);
   - `Projection`, drawn as in step 4 (default for windows, casework, generic models, and any family type whose author has set *Show above cut plane*);
   - `Overhead`, drawn with the `<Overhead>` line style (default for ceilings, soffits, upper cabinets drawn as dashed, and structural framing above the cut in structural plans).
   The table is project data, so offices can change it, and view filters may override it per view.
6. **Beyond zone.** Geometry in the Beyond zone is drawn with the `<Beyond>` line style. **Floor-like exception:** floors, structural slabs, stairs, ramps, roofs and toposolids lying below B are drawn with their normal projection style (not `<Beyond>`) when their top surface lies within the *floor tolerance* below B (default 1220 mm, project setting), **even if that is below D**. This shows the floor of a sunken area or the slab under a level without forcing the user to deepen the view range.
7. **Symbolic plan linework.** Family types may provide symbolic 2D linework for plan views (door swings, window sills, fixture symbols). When an element is visible by steps 3–6, its symbolic linework replaces or supplements its projected geometry according to the family's visibility settings. Symbolic linework is independent of the cut height.
8. **Partial elements.** Elements that span several zones are split with OCCT half-space booleans at C, B and D, and each piece is treated by its own zone. Splitting happens on a temporary copy; the model is never modified.
9. **Hidden-line resolution.** All pieces from steps 3–7 go through one hidden-line pass looking down (DOC-HLR-002). Cut poché occludes geometry beneath it. Pieces from the Upper zone shown as `Overhead` are not occluded by cut geometry (they are drawn over it, as overhead lines conventionally are).
10. **Show Hidden Lines.** When the view's *Show Hidden Lines* is All, or By Discipline with the element's discipline matching the view, edges hidden by step 9 are drawn with the `<Hidden>` line style instead of being dropped.
11. **Linked models.** Elements of linked models go through the same algorithm with the host plan's range (or with the linked view's range when the link's display setting is *By Linked View*, Chapter 6, COL-LNK-007).

**Data (RXD).** The category catalogue (Chapter 2) gains `cuttable`, `aboveCutBehaviour` and `floorLike` attributes, stored per category in RXD document `standards/object_styles`. The floor tolerance is in `project/settings`. The family-type flag *Show above cut plane* is part of the family recipe (Chapter 2).

**Commands.** `category.set_view_range_behaviour(category, cuttable?, above_cut?: hide|projection|overhead, floor_like?) -> ()`; `settings.set_floor_tolerance(length) -> ()`; `view.classify_element(view_id, element_id) -> zone_report` (query; returns which zones and styles apply, for debugging and for AI agents).

*Built (2026-10-20), the Upper zone for services:* an element's above-cut behaviour (`Hide`, `Projection`, `Overhead`) is Projection for ducts, pipes, cable tray, conduit, their fittings, junction boxes and air terminals and Hide for the rest, built in until the category catalogue carries it; floor plans (on screen and printed) draw those elements' edges between the cut plane and the top in projection, overhead ones dashed (3 mm dashes, 1.5 mm gaps on paper). Not yet: `category.set_view_range_behaviour`, the defaults for windows, casework and generic models (Projection) and ceilings and upper cabinets (Overhead), hidden-line removal of the Upper zone, `view.classify_element`.

**Acceptance criteria.**
- [ ] A 3000 mm wall from the level, viewed with C = 1200 mm, is drawn as cut poché with cut lines; a 900 mm high wall is drawn in projection.
- [ ] A window whose head is above C but whose sill is below it shows cut frame lines; a high window entirely above C (but below T) shows in projection; an upper wall cabinet shows dashed `<Overhead>` lines.
- [ ] A column footing between B and D is drawn with `<Beyond>`; one below D is not drawn.
- [ ] A sunken slab 600 mm below B and below D is drawn with projection lines; the same slab 1500 mm below B is not drawn (default tolerance 1220 mm).
- [ ] With Show Hidden Lines = All, a beam hidden below a slab in a structural plan is drawn with the `<Hidden>` style.
- [ ] `view.classify_element` reports the same zones that the drawing shows.

### Plan regions
**ID:** DOC-VR-003 · **Priority:** P1 · **Ref:** research/04 §1.1 *Plan Regions*

**Purpose.** Let part of a plan use a different view range (split levels, mezzanines, clerestory windows).

**Workflow.**
1. View tab › Create › Plan Views ▾ › Plan Region enters sketch mode.
2. The user sketches one or more closed, non-intersecting loops and finishes.
3. In Properties the user edits the region's View Range (same dialog as DOC-VR-001).

**UI.** Sketch mode with the standard draw tools (Chapter 1); Properties: View Range › Edit….

**Behaviour & rules.**
- The plan is partitioned into the region polygons and the remainder. Each part runs DOC-VR-002 with its own range, and the results are clipped to the part's polygon before they are combined into one draw list. Edges along the boundary are not drawn as model edges.
- Plan regions in the same view must not overlap; an overlap is rejected at Finish.
- Regions may extend beyond the crop region; only the part inside is used.
- Region boundaries are view-owned annotation of category *Plan Region* and can be hidden through the graphics dialog. Regions can be copied to other plans with clipboard paste-aligned (Chapter 1).

**Data (RXD).** Annotation object of kind `anno.planregion` in `views/annotation/<viewId>`: `loops` (view coordinates) and its own `viewRange` block.

**Commands.** `view.create_plan_region(view_id, loops[], view_range) -> element_id`; `planregion.set_view_range(region_id, top, cut, bottom, depth) -> ()`.

**Acceptance criteria.**
- [ ] A mezzanine plan region with C = 3800 mm cuts the mezzanine wall inside the region while the rest of the plan keeps C = 1200 mm.
- [ ] Sketching two overlapping regions in one view fails at Finish with a message naming the overlap.

### Underlay
**ID:** DOC-VR-004 · **Priority:** P1 · **Ref:** research/04 §1.1 *Underlay*

**Purpose.** Show another level range in halftone beneath the current plan, for tracing and coordination.

**Workflow.** In Properties › Underlay the user sets *Base Level*, *Top Level* and *Orientation* (Look down / Look up).

**Behaviour & rules.**
- The underlay range is from Base Level to Top Level (Top Level may be None, meaning the next level above Base). Its contents are generated by DOC-VR-002 with a synthetic range (`T` = Top Level, `C` = `T`, `B` = `D` = Base Level for Look down; mirrored for Look up) and drawn with the project's underlay line settings (DOC-GFX-009) beneath all other content.
- Underlay elements are not selectable unless the *Select underlay elements* selection toggle (Chapter 1) is on.
- Underlay is excluded from printing only if the print option *Hide underlay* is on (default off).

**Data (RXD).** `underlay` block of the view payload: `baseLevel`, `topLevel` (`ifc:` storey references or null), `orientation`.

**Commands.** `view.set_underlay(view_id, base_level_id?, top_level_id?, orientation) -> ()`

**Acceptance criteria.**
- [ ] With Base Level = Level 1 in the Level 2 plan, Level 1 walls appear in halftone and cannot be selected with the toggle off.

### Depth clipping and far clipping
**ID:** DOC-VR-005 · **Priority:** P1 · **Ref:** research/04 §1.1 *Depth Clipping / Far Clipping*

**Purpose.** Limit how far a view sees past its view plane, with or without a boundary line.

**Behaviour & rules.**
- **Depth Clipping (plans):** No clip / Clip without line / Clip with line. When clipping is on, geometry beyond View Depth is removed even for floor-like categories (step 6 of DOC-VR-002 is suppressed). *Clip with line* draws the intersection of cut geometry with the depth plane using the `<Beyond>` style.
- **Far Clipping (sections, elevations, callouts):** No clip / Clip with line / Clip without line, plus *Far Clip Offset* (distance from the view plane). With *No clip*, the view sees to infinity. *Clip with line* draws the intersection edges at the far plane.
- Far clip and depth clipping are applied as an extra clipping half-space before hidden-line removal.

**Data (RXD).** View payload fields `depthClipping`, `farClipping`, `farClipOffset`.

**Commands.** `view.set_far_clip(view_id, mode, offset?) -> ()`; `view.set_depth_clipping(view_id, mode) -> ()`

**Acceptance criteria.**
- [ ] A section with Far Clip Offset 5000 mm does not show an elevation of walls 8000 mm behind the cut.
- [ ] *Clip with line* draws the clipped outline of a slab that crosses the far plane.

---

## 5.4 Elevations

**ID:** DOC-VIEW-003 · **Priority:** P0 · **Ref:** research/04 §1.2

**Purpose.** Orthographic views looking horizontally at the building, from outside (building elevations) or inside a room (interior elevations).

**Workflow.**
1. View tab › Create › Elevation ▾ › Elevation; choose an elevation type (for example *Building Elevation* or *Interior Elevation*).
2. In a plan, the marker preview snaps its direction perpendicular to the nearest wall; Tab cycles candidate directions. Click to place.
3. A marker with one active direction creates one view. Selecting the marker body shows one checkbox per direction (up to four); ticking more checkboxes creates more views from the same marker.
4. Selecting a direction arrow shows that view's extents (view plane width, far clip line) with drag grips.
5. *Framing Elevation*: pick a grid line or reference plane; the view is aligned to it and its crop is limited to the selected grid bay.

**UI.** Type Selector (elevation types), Edit Type (marker symbol, default template), placement preview, marker checkboxes, rotate control on the marker.

**Behaviour & rules.**
- Interior elevation markers placed inside a room size their crop to the room's bounding walls, floor and ceiling where they can be found; otherwise to a default size.
- Rotating the marker gives non-orthogonal elevations.
- *Hide at scales coarser than* hides the marker in views at coarser scales.
- The marker head displays the detail and sheet number once the view is placed on a sheet; until then it shows a placeholder dash.
- Deleting a marker deletes its views after confirmation; deleting the last view of a marker deletes the marker.

**Data (RXD).** Marker: annotation object of kind `anno.elevationmarker` in the host plan's annotation document (position, rotation, marker type, `views[4]` slots holding `rv:` view ids or null). Each view: `views/<viewId>` of kind `view.elevation`, with `marker` and `markerSlot` (0–3); framing elevations add `datum` (`ifc:` grid or reference-plane reference) and `bay`.

**Commands.**
`view.create_elevation(host_view_id, point, elevation_type_id, directions[], rotation?) -> {marker_id, view_ids[]}`
`elevationmarker.set_directions(marker_id, directions[]) -> view_ids[]`
`view.create_framing_elevation(host_view_id, datum_id, elevation_type_id, bay?) -> view_id`

**Acceptance criteria.**
- [ ] Placing a marker next to a wall creates a view facing that wall.
- [ ] Ticking all four boxes on an interior marker creates four views, each cropped to the room.
- [ ] After placing the elevation on sheet A201 as detail 3, the marker head shows "3" and "A201".

---

## 5.5 Sections

**ID:** DOC-VIEW-004 · **Priority:** P0 · **Ref:** research/04 §1.3 · screenshots/documentation/section_view_building.png

**Purpose.** Vertical cuts through the model.

**Workflow.**
1. View tab › Create › Section (also on the Quick Access Toolbar); choose *Building Section*, *Wall Section* or *Detail*.
2. Click the start point and the end point in a plan, elevation or section. The view looks to the right-hand side of the drawn direction; a flip control reverses it.
3. Drag the width (crop extents) and far clip grips. Cycle and flip controls at the head and tail change which ends show heads.
4. **Jogged section:** select the section, then use *Split Segment* on the contextual tab, click a point on the line, and drag the new segment. Only available in plans.
5. **Gap display:** the line's midpoint control toggles between a continuous line and a line that shows only its ends.
6. **Reference section:** with *Reference other view* on the contextual tab, the new section line points to an existing view (for example a typical detail in a drafting view) instead of creating a view.

**UI.** Type Selector; contextual tab Modify | Views: Split Segment, Reference other view (with a view picker), Flip; Properties: Crop View, Crop Region Visible, Annotation Crop, Far Clipping, Far Clip Offset, Scope Box, Hide at scales coarser than, Reference Label, Detail Number, Sheet Number, Title on Sheet.

**Behaviour & rules.**
- *Detail*-type sections are listed under Detail Views, default to Fine detail level, and hide their section line in views coarser than a type-defined scale.
- A jogged section is a single view: its cut is a polyline of segments perpendicular to the view direction, each with its own depth position. The cut at each segment is computed separately, and the jog edges are drawn as boundary lines.
- The section head and tail symbols, and the gap display style (Continuous / Gap / Segmented), come from the section type.
- **Reference labels:** reference sections, callouts, elevations and view references carry a *Reference Label* parameter (for example "SIM", "TYP", "OPP"). The type supplies a default; each instance can override it. Labels can include shared parameters and are edited in place on the canvas.
- Moving or rotating the section line regenerates the section view.

**Data (RXD).** `views/<viewId>` of kind `view.section` (or `view.detail` for detail sections): `origin` (section line start), `direction`, `segments` (list of offsets and lengths for jogs), `width`, `farClipOffset`, `headEnd`/`tailEnd`, `gapMode`, `referenceView` (reference sections), `referenceLabel`, `hideAtScalesCoarserThan`. The marker drawn in other views is generated from this definition and is not stored separately.

**Commands.**
`view.create_section(host_view_id, start, end, section_type_id, width?, far_offset?, reference_view_id?) -> view_id`
`section.split_segment(view_id, point, offset) -> ()`
`section.set_gap_mode(view_id, mode) -> ()`
`section.flip(view_id) -> ()`
`view.set_reference_label(view_id_or_marker_id, label?) -> ()`

**Implementation notes (2026-10-13).**
- As built:
  - `view.create_section(start, end, far_offset?, name?)`.
  - `view.create_elevation(point, look, width?, far_offset?, name?)` makes one elevation (no marker element yet).
  - `section.edit(view_id, start?, end?, far_offset?, flip?)` covers moving the line, the far clip and `section.flip`.
- The definition is the view's `section` block: `start`, `end`, `far` and `flipped`.
- How drawings are made:
  - The model is transformed into the view's frame (x to the viewer's right, y up, z toward the viewer).
  - The plan cut/projection pipeline then draws it, with the cut at the view plane and the far clip as the bottom.
  - Levels are drawn as lines with name and elevation.
  - A grid is drawn where its line, extended, meets the view plane, if part of it lies within the view's depth.
- Crop regions, jogs, gap modes, reference sections and markers in other views (beyond plans) come later.

**Acceptance criteria.**
- [ ] Drawing a section across a building creates a section view showing cut walls, floors and roofs, and elevation linework of what lies beyond, up to the far clip.
- [ ] Splitting a segment and dragging it 2 m produces one view with a jog line where the two cuts meet.
- [ ] A reference section pointing to a drafting view shows that view's detail and sheet numbers in its head.

*Built (2026-10-26), hidden lines in sections and elevations:* the projection linework of sections and elevations (DOC-VIEW-003, DOC-VIEW-004), on screen and printed, leaves out edges behind nearer faces: each feature edge is sampled every 50 mm against the faces between it and the view plane (binned on a grid in the view's plane), and the changes between seen and hidden are found to a millimetre. Before, every edge out to the far clip was drawn, so a far wall's windows showed through the near wall. This is the CPU form of the visibility DOC-HLR-003 resolves on the GPU; it serves until that and the exact OCCT drawing (DOC-HLR-002). *(Owner, 2026-10-26: "Fix the elevation hidden-line bug too.")* Not yet: hidden edges in the `<Hidden>` style, plans' projection below the cut (still drawn without occlusion).

---

## 5.6 Callouts

**ID:** DOC-VIEW-005 · **Priority:** P0 · **Ref:** research/04 §1.4 · screenshots/documentation/callout_views_detail.png

**Purpose.** Enlarged views of part of a parent view.

**Workflow.**
1. View tab › Create › Callout ▾ › Rectangle or Sketch.
2. Rectangle: pick two corners in the parent view. Sketch: draw a closed loop and finish.
3. Drag the callout head and its leader to position them.
4. Double-click the callout head (or use *Go to View* from the context menu) to open the callout view.

**Behaviour & rules.**
- **Detail callout** (type of kind Detail): creates a detail view listed under Detail Views. In a plan it has no level association and owns its annotation independently.
- **View callout** (type of the parent's kind, for example floor plan): creates a view of the parent's kind, with its own view range, associated with the parent's level.
- **Sketch callout:** the crop region is the sketched (non-rectangular) loop.
- **Reference callout:** *Reference other view* points the callout to an existing view.
- The callout boundary in the parent and the crop region of the callout view are the same object: editing either updates the other.
- New callouts take phase, detail level and template from their type, and default to a scale twice as large as the parent's (for example 1:50 from 1:100).
- Callouts can be nested.
- In a plan, a detail callout's view can show only annotation (Display Model = Do not display).

**Data (RXD).** `views/<viewId>` of kind `view.callout` (or `view.detail`): `parent`, `boundary` (polyline in parent view coordinates, shared with the crop), `head` (point and leader points), `referenceView`.

**Commands.** `view.create_callout(parent_view_id, boundary, callout_type_id, reference_view_id?) -> view_id`; `callout.set_head(view_id, head_point, leader_points[]) -> ()`

**Acceptance criteria.**
- [ ] Changing the callout view's crop region moves the callout boundary in the parent view in the same undo step.
- [ ] A sketch callout with an L-shaped loop produces an L-shaped crop.

---

## 5.7 3D views, cameras and section boxes

### Default and orthographic 3D
**ID:** DOC-VIEW-006 · **Priority:** P0 · **Ref:** research/04 §1.5; web: Add Tags and Keynotes in a 3D View

**Purpose.** Show the model in 3D for modelling, coordination and presentation.

**Workflow.**
1. View tab › Create › 3D View ▾ › Default 3D View (also on the Quick Access Toolbar). If the user's default 3D view exists it opens; otherwise it is created as an isometric view from the south-east.
2. The user navigates with the view cube, navigation bar and mouse (orbit, pan, zoom; Chapter 1).
3. Renaming the default view keeps it; the next *Default 3D View* command creates a new one.

**Behaviour & rules.**
- Toggling between parallel and perspective projection is available from the view cube context menu.
- **Lock / Unlock 3D view** (View Control Bar lock menu) locks the camera so that tags, keynotes and text can be placed in the 3D view. Annotation in an unlocked view is disallowed. The menu has three items:
  - *Save Orientation and Lock View* stores the current camera as the view's locked orientation and locks it. The user's default 3D view cannot be locked: Reviera asks for a name and saves it as a new named view (`save_as_name`). If the view already has a saved orientation that differs and contains annotation, Reviera warns that the annotation placed at the old orientation will be deleted, and continues only on confirmation.
  - *Restore Orientation and Lock View* returns an unlocked view to its saved orientation and locks it again; annotation placed at that orientation shows again.
  - *Unlock View* allows orbiting. The view's annotation is kept but hidden (and excluded from print) until the orientation is restored.
- In a locked view, orbit, pan-by-orbit and view cube rotation are refused; pan and zoom remain available. Perspective (camera) views can be locked, but tags and keynotes cannot be placed in them. Material tags, room, area and space tags are not allowed in any 3D view.
- New tags in locked 3D views default to a free leader end (DOC-ANN-010).
- **Orient to view / direction**: aligns the 3D camera to a plan, elevation or section, and sets the section box to that view's extents.
- 3D views can be placed on sheets.

**Data (RXD).** `camera` block of a `view.3d` payload: `eye`, `target`, `up` (model coordinates, metres), `perspective`, `fieldOfView` (horizontal, radians), `parallelScale`, `locked`, and `lockedCamera` (the saved orientation: `eye`, `target`, `up`, `parallelScale`; null until first locked).

**Commands.**
`view.create_3d(kind: default|named, name?) -> view_id`
`view.set_camera(view_id, eye, target, up, projection?: orthographic|perspective, fov?) -> ()`
`view.lock_3d(view_id, locked, save_as_name?) -> view_id` (`locked = true` is *Save Orientation and Lock View*; it returns the new view's id when `save_as_name` is used on the default 3D view; `locked = false` is *Unlock View*)
`view.restore_orientation(view_id) -> ()` (*Restore Orientation and Lock View*; fails with `view.no_saved_orientation` when the view was never locked)
`view.orient_to(view_id, source_view_id_or_direction) -> ()`

**Acceptance criteria.**
- [ ] Two users in BIMserver mode each get a separate default 3D view; orbiting one does not change the other.
- [ ] Placing a tag in an unlocked 3D view is refused with a prompt to lock the view.
- [ ] Unlocking a locked view with five tags, orbiting, then *Restore Orientation and Lock View* shows the five tags again at their places.
- [ ] *Save Orientation and Lock View* on the default 3D view asks for a name and creates a new locked view; the default view stays unlocked.

### Cameras and walkthroughs
**ID:** DOC-VIEW-007 · **Priority:** P1 (camera), P2 (walkthrough) · **Ref:** research/04 §1.5 *Camera*, *Walkthrough*

**Purpose.** Perspective views from a chosen eye point, and animated paths through the model.

**Workflow.**
1. Camera: View tab › 3D View ▾ › Camera; click the eye position in a plan, then the target.
2. Properties: Eye Elevation, Target Elevation, Perspective, Far Clip. *Size Crop* sets the field of view as width and height (scaling proportionally if requested). Uncropped perspective views are allowed.
3. Walkthrough: 3D View ▾ › Walkthrough; click key-frame positions along a path in a plan; Finish.
4. *Edit Walkthrough* on the contextual tab: choose the active key frame, move camera and target, play, step frames. The frames dialog sets total frames, frames per second, total time, and uniform speed or per-key-frame timing.

**Behaviour & rules.**
- The camera's field of view is derived from the crop size and the camera distance; changing either keeps the other consistent.
- A walkthrough is a spline path through the key frames; the camera interpolates position and target along it (Catmull–Rom by default).
- Exporting a walkthrough is covered by DOC-OUT-012.

**Data (RXD).** Camera: `camera` block as in DOC-VIEW-006 on a `view.camera` payload, plus `cropSize`. Walkthrough: `view.walkthrough` payload with `keyframes[]` (`eye`, `target`, `time`), `totalFrames`, `fps`, `uniformSpeed`.

**Commands.**
`view.create_camera(host_view_id, eye, target, eye_elevation?, target_elevation?) -> view_id`
`view.size_crop(view_id, width, height, proportional?) -> ()`
`view.create_walkthrough(host_view_id, keyframes[]) -> view_id`
`walkthrough.set_keyframe(view_id, index, eye?, target?, time?) -> ()`
`walkthrough.set_timing(view_id, total_frames, fps, uniform_speed?) -> ()`

**Acceptance criteria.**
- [ ] A camera placed in a plan opens a perspective view whose eye is at the clicked point and default eye elevation (1750 mm above the plan's level).
- [ ] A five-key-frame walkthrough plays back at the set frame rate in the viewport.

### Section box
**ID:** DOC-VIEW-008 · **Priority:** P0 · **Ref:** research/04 §1.5 *Section Box* · screenshots/documentation/3d_view_section_box.png

**Purpose.** Clip a 3D view to a box.

**Workflow.**
1. Properties › Extents › Section Box ✓ shows a box around the model.
2. Selecting the box shows six face grips and a rotation grip; dragging them clips the view in real time.
3. *Selection Box* (default shortcut BX): with elements selected, fits a section box tightly around them in the active 3D view, or in the user's default 3D view when the active view is not 3D.
4. Typed extents: while the box is on, Properties › Extents shows *Box X From*, *Box X To*, *Box Y From*, *Box Y To*, *Box Z From* and *Box Z To* (the faces in the box's own axes, in project length units); typing one moves that face (never past the opposite face). *Built (2026-10-28).*

**Behaviour & rules.**
- The box is an oriented box (a transform plus extents). The GPU viewport clips with six clip planes and fills cut faces with caps (DOC-HLR-006); hidden-line output cuts solids with OCCT and draws section poché on the cut faces when the view's *Show cut faces* property is on (default on).
- The box outline is an annotation category (*Section Boxes*) that can be hidden; when hidden, it can still be selected by Tab or by the Properties checkbox.
- Turning the box off keeps its last extents for when it is turned on again.
- The box's six face grips are shown while the box is selected, and also whenever the *Section Box* toggle on View tab › Inspect is on, so the box can be resized without selecting it first. A face cannot be dragged past the opposite face (the box keeps at least 10 mm in every direction).

**Data (RXD).** `sectionBox` block of a `view.3d` payload: `enabled`, `transform` (4×3 matrix: the box's x, y and z axes, then its origin, in model coordinates; identity when absent), `min`, `max` (in the box's coordinates, metres), `showCutFaces`.

**Commands.**
`view.set_section_box(view_id, enabled, transform?, min?, max?) -> ()`
`view.fit_section_box(element_ids[], view_id?, margin?) -> view_id`

**Acceptance criteria.**
- [ ] Dragging a face grip updates the viewport at interactive frame rates (at least 30 fps on the reference model, Chapter 1 performance targets).
- [ ] *Selection Box* on one room's furniture produces a box that contains all of it with the default margin (300 mm).

### Slice planes
**ID:** DOC-VIEW-014 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); builds on DOC-VIEW-008 and DOC-HLR-006

**Purpose.** Cut a 3D view open along any plane to look inside the model, with or without a section box.

**Workflow.**
1. View tab › Inspect › Slice Plane ▾: *Horizontal* (at the elevation of the point clicked in the view, or at a level's cut plane chosen from the drop-down), *Vertical* (through the clicked point, facing the camera, snapped to the nearest model axis when within 10°), or *On Face* (the plane of the picked planar face, keeping the side the camera is on).
2. Each slice plane shows a translucent outline over the model bounds and an arrow grip at its origin. Dragging the grip moves the plane along its normal in real time; Ctrl+drag rotates it about the vertical. Clicking the arrow flips which side is kept.
3. Inspect › Clear Slices removes them; Properties › Extents › Slice Planes lists them, each with an *Enabled* check box.
4. Typed positions: Properties › Extents shows each slice's position, *Slice n (X)*, *(Y)* or *(Z)* for a plane facing an axis (its coordinate on that axis), *Slice n (along normal)* otherwise (its distance from the origin along its normal); typing one moves the plane along its normal; *Slice n Enabled* keeps a plane but stops it cutting. *Built (2026-10-28).*

**Behaviour & rules.**
- A 3D view has at most eight slice planes, in addition to the six planes of its section box (DOC-HLR-006 raises its clip-plane budget to fifteen: six box, eight slice, one far clip). Eight covers the clipping planes of BCF viewpoints in practice; a viewpoint with more is restored with the first eight and a warning. A slice plane removes everything on its far side; several planes intersect (what remains is kept by all of them).
- Cut faces are capped exactly as for the section box (DOC-HLR-006), and hidden-line output cuts the solids in the same way.
- Slice planes apply to 3D views only. In Blueprint mode (DOC-VIEW-013) the blueprint slice is drawn as one more slice plane while it is being dragged.
- Picking ignores the parts of elements that a slice plane or the section box removes: a click passes through the removed part to what is behind it.
- **BCF.** Section boxes and slice planes are what BCF viewpoints carry as clipping planes (COL-BCF-003), including the viewpoints of issues raised by the owner's FileEngine project, so the mapping is exact in both directions. A BCF clipping plane's direction points to the side that is hidden, so it is the negated slice normal. On capture, an enabled section box becomes six planes and each enabled slice plane one more. On restore, six planes that pair up as opposite, mutually perpendicular faces become the section box (its axes and extents recovered within 1 mm and 0.01°), and every other plane becomes a slice plane. Capturing a restored viewpoint gives back the same planes, up to order.
- Slice planes are part of the view definition (saved, undoable). Dragging a grip is one undo step, committed on release.

**Data (RXD).** `slicePlanes[]` of a `view.3d` payload: `origin` ([x, y, z], metres), `normal` (unit vector pointing to the kept side), `enabled`.

**Commands.**
`view.set_slice_planes(view_id, planes[{origin, normal, enabled?}]) -> ()` (replaces the list; more than eight fails with `view.too_many_slices`; a zero normal fails with `view.bad_plane`)

**Acceptance criteria.**
- [ ] A horizontal slice at 1.2 m above Level 1 shows the walls of Level 1 cut and capped at that height, with the door heads above the cut removed.
- [ ] With a section box and two slice planes, the view keeps only what is inside the box and on the kept side of both planes, and a click on a removed part of a wall selects the element behind it.
- [ ] Dragging the grip and releasing makes one undo step; Undo restores the plane's previous position.

### Blueprint mode (3D slice to 2D drawing)
**ID:** DOC-VIEW-013 · **Priority:** P1 · **Ref:** owner requirement (2026-10-12); builds on DOC-VIEW-002, DOC-VR-001, DOC-VIEW-008 and §5.12

**Purpose.** Go from the 3D model to a conventional 2D drawing in one step. The user slices the model where they want to look, and Reviera turns the slice into the drawing an architect would draw there: a horizontal slice through a storey becomes a floor plan, and a vertical slice becomes a section. The transition is continuous, so the user can see how the drawing comes out of the model.

**Workflow.**
1. In any 3D view the user turns on **Blueprint mode**: View Control Bar › Blueprint, or the default shortcut BP.
2. Reviera proposes a slice:
   - **Horizontal** (default): at the cut-plane height of the level nearest the camera target (the level's default view range, DOC-VR-001).
   - **Vertical**: when the camera looks more horizontal than down (pitch below 30°), at the section box face or the plane through the camera target facing the camera.
   
   The slice plane shows as a translucent plane with a grip. Dragging the grip moves the cut, Tab cycles horizontal and vertical, and clicking a level in the Project Browser snaps the cut to that level's cut plane.
3. The user confirms with Enter or by clicking **Convert to drawing**. The camera animates (about 0.6 s) to orthographic, looking straight at the slice: down onto a horizontal slice, or along the normal of a vertical one. The model above (or in front of) the cut is removed, and the graphics change to drawing conventions:
   - cut elements show cut patterns (poché) and cut line weights;
   - elements below the cut, within the view depth, show in projection;
   - elements beyond the view depth are hidden, or shown as underlay as the view range says;
   - hidden lines are removed (Hidden Line style, §5.12).
4. While in Blueprint mode the view behaves as a plan (or section) view:
   - annotation tools are available: dimensions, tags and text;
   - modelling tools work on the slice plane (the level's work plane for horizontal slices).
5. **Exit** (Esc, or the View Control Bar toggle) animates back to the previous 3D camera.
6. **Save as view** creates a real plan view (DOC-VIEW-002), or a section (DOC-VIEW-004), with the slice's level, view range or section line. Annotation placed during the session moves to the new view. Without saving, annotation placed in Blueprint mode stays with the 3D view's blueprint state (see Data).

**Behaviour & rules.**
- **Drawing equivalence.** A horizontal blueprint of a level, with the level's default view range, produces the same draw list (§5.12) as a new floor plan of that level with the default plan view template. The draw list includes linework, cut patterns, line weights, hidden-line results and which categories show. The acceptance criteria compare the two.
- **Slice choice.**
  - A horizontal slice uses the level's view range: cut plane, top, bottom and view depth. The slice grip moves the cut plane only, and the other planes keep their offsets.
  - A vertical slice uses a section-like range: far clip at the section box, or the model extents.
- **Presentation style.** The drawing uses the view's graphics (object styles, overrides and filters) in black on white by default. An optional **Blueprint colour scheme** draws white and cyan linework on a blueprint-blue background. It is a display setting for the view; printing and export use the normal print colours unless the user chooses otherwise.
- **Live model.**
  - Edits made while in Blueprint mode regenerate the drawing at once: the GPU-assisted hidden-line path on screen, and the exact OCCT pipeline when printing.
  - Undoing past the moment Blueprint mode was entered does not leave the mode.
- **Navigation in the drawing.**
  - Orbit is disabled. Pan and zoom work as in plan views.
  - Orbiting with the mouse, or clicking the ViewCube, offers to exit Blueprint mode.
- **Performance.** Converting the reference model's typical storey to a drawing takes under 1 s on the reference hardware (Chapter 1 targets), animation included. Moving the slice grip updates the cut at interactive rates, at least 20 fps, using GPU clip planes and caps (DOC-HLR-006) before the exact drawing is computed when the grip is released.

**Data (RXD).** `blueprint` block of a `view.3d` payload:
- `enabled`
- `slice`: `{kind: horizontal, level_id, cut_offset}` or `{kind: vertical, origin, normal}`
- `range` (the view range used)
- `colourScheme`: `default` or `blueprint`
- `savedCamera` (the camera to return to)
- `annotations[]` (ids of annotation placed in blueprint mode and not yet moved to a saved view)

**Commands.**
`view.blueprint(view_id, on: bool, slice?: {kind, level_id?, cut_offset?, origin?, normal?}) -> ()`
`view.blueprint_set_slice(view_id, slice) -> ()`
`view.blueprint_save_as_view(view_id, name?) -> view_id`
`view.blueprint_set_colour_scheme(view_id, scheme: default|blueprint) -> ()`

**Acceptance criteria.**
- [ ] In the default 3D view of a two-storey model, Blueprint mode on the ground floor shows a floor plan whose draw list equals that of a new floor plan of that level with the same view range: same linework and cut patterns, within drawing tolerance.
- [ ] Dragging the slice grip from 1200 mm to 900 mm above the level updates the cut at 20 fps or more, and on release the drawing shows windows with sills above 900 mm in projection rather than cut.
- [ ] *Save as view* creates a plan named after the level. Two dimensions placed in Blueprint mode appear in the new plan and no longer in the 3D view.
- [ ] Esc returns to the exact previous 3D camera (eye, target and projection).
- [ ] With a section box enabled and a horizontal camera, Blueprint mode proposes a vertical slice on the box face nearest the camera, and the drawing matches a section through the same plane.
- [ ] `view.blueprint` from Lua and MCP turns the mode on and off with the same result as the UI.

### Displaced elements (exploded views)
**ID:** DOC-VIEW-012 · **Priority:** P2 · **Ref:** research/04 §3.1 (modal display modes: *Highlight Displacement*); web: About Displacement Sets, About Displacement Set Paths, Create a Displacement Set, Edit a Displacement Set, Create Displacement Set Paths, Modify Displacement Set Path Style and Display, Reset a Displacement Set, Highlight Displacement Sets

**Purpose.** Pull elements away from their true position in one view only, to make exploded axonometrics and assembly diagrams, and draw paths back to where they belong. The model is never moved.

**Workflow.**
1. In a 3D view (orthographic or perspective), plan, section or elevation, select one or more model elements and choose Modify › View › *Displace Elements*. The selection becomes a **displacement set** with a drag gizmo of three arrows along the view's model X, Y and Z axes.
2. Drag an arrow, or type *Displacement X/Y/Z* in the Properties palette, to move the set.
3. To refine the set: select it, choose *Edit* on the Modify | Displacement Set tab, then *Add to displacement* or *Remove from displacement* and pick elements; *Finish* or *Cancel*.
4. To explode part of a set further, Tab-select an element inside a set and choose *Displace Elements* again: this creates a nested set whose offset is relative to its parent.
5. *Path*: hover an edge or vertex of a displaced element; a preview line shows the route back to the original position; click to place the path.
6. *Reset* returns the selected set (and its nested sets) to the true position and deletes them with their paths.
7. View Control Bar › *Highlight Displacement Sets* colours each set distinctly while the toggle is on.

**UI.** Modify | Displacement Set contextual tab: Edit, Path, Reset. Edit mode panel: Add to displacement, Remove from displacement, Finish, Cancel. Properties of a set: Displacement X, Y, Z (length), Name. Properties of a path: Style (Straight or Jogged), Line style override. View Control Bar toggle: Highlight Displacement Sets (session state).

**Behaviour & rules.**
- Only model elements can be displaced. Annotation, view-owned drafting, datums (levels, grids, reference planes), rooms and links as a whole cannot; the command reports which picked items were skipped. Elements of a linked model can be displaced individually.
- Each element belongs to at most one set per view (the innermost set when nested). Adding an element that is in another set moves it.
- A displacement is a pure per-view translation applied after the model is evaluated and before stage 1 of the drawing pipeline (DOC-HLR-001). Constraints, joins, hosting and schedules ignore it, and so do other views. A hosted element (door, window) displaced without its host keeps its host's opening cut in the host; displacing the host alone leaves the inserts in place.
- In 2D views the offset component along the view direction is kept but has no visible effect, except that it changes which zone of the view range (DOC-VR-002) the displaced geometry falls in; classification, cutting and hidden-line removal all use the displaced geometry.
- **Annotation on displaced elements.** Tags, keynotes and spot dimensions may be placed on displaced elements and follow the displaced position. Dimensions cannot reference displaced elements (refused with `dim.displaced_reference`), because the measured distance would be false. Temporary dimensions are not shown for displaced elements.
- **Paths** are view-owned annotation of category *Displacement Path*. A path joins a point on a displaced edge or vertex to the same point at its true position. *Straight* is one segment; *Jogged* follows the set's offset axes one after another (X, then Y, then Z, skipping zero components). Paths update when the set moves and are deleted when their element leaves the set. Line weight, colour and pattern come from the *Displacement Path* object style, overridable per view (DOC-GFX-003) and per path.
- **Highlight mode** draws every set in its own colour from a distinct palette, halftones everything else, and frames the view in the highlight colour. It is session state: not saved, not printed (DOC-CORE-006).
- Deleting an element removes it from its set; an empty set is deleted. Duplicating a view with detailing (DOC-VIEW-011) copies its displacement sets; plain duplicate does not. Dependent views share the parent's sets.
- Displacement sets are part of the view definition, so they are not controlled by view templates.
- **Interactive tool.** The set edit mode follows Conventions §2.1: its micro-operations are `disp.add(element)`, `disp.remove(element)`, `disp.offset(vector)` and `disp.path(edge_ref, style)`, each logged with the set id; the replay function re-applies them to the set's state at the start of the session, so an interrupted edit is recovered after a crash (Chapter 2b, DM-LIVE-015).

**Data (RXD).** View payload block `displacementSets[]`, each with `id`, `name`, `parent` (optional), `members[]` (`ifc:` or `link:` references) and `offset` (a 3-vector in model coordinates, metres). Paths are annotation objects of kind `anno.displacementpath` in `views/annotation/<viewId>`: `set`, `ref` (edge or vertex sub-reference plus parameter), `style` (straight | jogged), `lineStyle`. Nothing is written to IFC; IFC annotation export (DOC-IFC-002) writes paths as `IfcAnnotation` curves with `ObjectType = 'DISPLACEMENT_PATH'` and displaced linework in the drawing linework.

**Commands.**
`view.displace_elements(view_id, element_ids[], offset, displacement_set_id?) -> displacement_set_id` (with `displacement_set_id`, the new set is nested in that set and `offset` is relative to it)
`displacement.edit(displacement_set_id, add_ids[], remove_ids[]) -> ()`
`displacement.set_offset(displacement_set_id, offset) -> ()`
`displacement.add_path(displacement_set_id, edge_ref, style?: straight|jogged) -> annotation_id`
`displacement.set_path_style(annotation_ids[], style: straight|jogged, line_style_id?) -> ()`
`displacement.reset(displacement_set_ids[]) -> ()`
`view.highlight_displacement(view_id, on) -> ()` (session state, not undoable)

**Acceptance criteria.**
- [ ] Displacing a roof 3 m upwards in an axonometric moves it in that view only; the plan, the roof's schedule row and its IFC placement are unchanged.
- [ ] A nested set inside a displaced set moves with its parent and keeps its own relative offset.
- [ ] A jogged path from a displaced beam corner follows the X, then Z offsets and redraws when the set moves.
- [ ] Creating a dimension to a displaced element is refused with `dim.displaced_reference`; tagging it is allowed and the tag follows it.
- [ ] *Reset* returns all members to their true positions, deletes the set and its paths, and is undone in one step.
- [ ] Highlight mode never appears in a PDF export.

---

## 5.8 Drafting views

**ID:** DOC-VIEW-009 · **Priority:** P0 · **Ref:** research/04 §1.6; web: Insert a Drafting View from Another Project, Reuse Views and Sheets in Other Projects, Insert Detail Components from a Saved Detail View

**Purpose.** 2D views with no model behind them, for standard details and diagrams.

**Workflow.**
1. View tab › Create › Drafting View; enter a name and scale.
2. Draw with detail lines, regions, detail components, text, dimensions, symbols, and imported or linked CAD (Chapter 6).
3. Reference the view from sections and callouts (DOC-VIEW-004, DOC-VIEW-005).
4. *Insert › Insert from File › Views from File* copies drafting views from another IFC project; *Save to Library › Views* writes selected drafting views into a new IFC file containing only those views and the types they use.

**Behaviour & rules.**
- Dimensions in drafting views reference 2D geometry only.
- When drafting views are inserted, conflicting type names are resolved by the duplicate-type dialog (Chapter 2).
- **Insert Views from File** (Insert › Import › Insert from File ▾ › *Insert Views from File*, `view.insert_from_file`): after a file is chosen, the *Insert Views* dialog lists the source's insertable views with a kind filter (All, Drafting views, Schedules, Legends, Renderings, Sheets) and a preview. Insertable kinds are drafting views, schedules (definitions only; rows come from this project's model), legends, rendering views and sheets whose viewports hold only drafting views, legends, schedules or renderings. Model views (plans, sections, 3D) are not offered, because their content is the source model. Inserted views keep their names; a clash adds a numeric suffix (`Detail 1 (2)`). Types and styles the views use (text, dimension, region, line and fill pattern, detail component families) are copied when missing; when a type of the same name exists, the *current project's* definition is kept and the insert report lists every such case.
- **Insert 2D Elements from File** (Insert from File ▾ › *Insert 2D Elements from File*, `detail.insert_from_file`): copies the view-owned elements (detail lines, filled and masking regions, detail components, repeating details, insulation, text, symbols, dimensions between copied elements) from one source view into the active view, as a group of new objects that the user positions with the cursor (or at `position`), then *Finish*. The result is selected so it can be moved at once. Model geometry is never copied. Elements that cannot be copied are skipped and listed: cut profiles (DOC-GFX-020), elements that create other views (callouts, sections), and annotation that references model elements (tags, keynotes, dimensions to model geometry), since their hosts do not exist in the target.
- Sources can be any IFC file with Reviera RXD, including library files written by *Save to Library* and projects opened read-only by another user. Sources without RXD offer nothing and say why. In BIMserver mode, a source may also be a BIMserver project revision (Chapter 6).
- Insertion is one undo step and assigns new ids to everything inserted; nothing keeps a link to the source.

**Data (RXD).** `views/<viewId>` of kind `view.drafting`; its contents live in `views/annotation/<viewId>`. A library file written by *Save to Library* is an IFC file containing only the RXD for those views and the styles and types they use. Inserted objects record `ext.insertedFrom` (source file name and source object id) for traceability only.

**Commands.** `view.create_drafting(name, scale) -> view_id`; `view.insert_from_file(path, view_names[]?) -> view_id[]` (with no `view_names`, the dialog opens; from Lua and MCP, `view_names` is required); `view.save_to_library(view_ids[], path) -> ()`; `detail.insert_from_file(path, source_view_name, target_view_id, position?) -> annotation_id[]` (fails with `view.kind_mismatch` when the target is a 3D or schedule view).

**Acceptance criteria.**
- [ ] A drafting view saved to a library file and inserted into another project reproduces its contents and line styles.
- [ ] Inserting a drafting view whose text type "2.5 mm Arial" also exists in the target keeps the target's definition and lists the clash in the report.
- [ ] Inserting 2D elements from a source detail view copies its regions, lines and text into the active section, skips its tags, and reports how many items were skipped.
- [ ] A sheet holding only drafting views is offered in *Insert Views*; a sheet holding a floor plan is not.

---

## 5.9 Legends and keynote legends

**ID:** DOC-VIEW-010 · **Priority:** P1 · **Ref:** research/04 §1.7 · screenshots/documentation/keynote_legend_detail.png

**Purpose.** Keys that explain the drawing: symbol legends built from real types, and lists of keynotes used.

**Workflow.**
1. View tab › Legends ▾ › Legend: name and scale.
2. Annotate › Component ▾ › Legend Component: choose a family type, a view direction (plan, front elevation, back elevation, section where the type supports it), a detail level, and for hosted or linear types a host length.
3. Add text, detail lines, symbols and regions as needed.
4. Keynote Legend: View tab › Legends ▾ › Keynote Legend; set name, then fields (Key Value, Keynote Text) through the schedule properties (§5.14).

**Behaviour & rules.**
- A legend component draws the type's geometry, not a model element; editing the type updates every legend component of it. Legend components do not appear in schedules or counts.
- Legends and keynote legends can be placed on many sheets (DOC-CORE-005).
- A keynote legend lists the keynotes used in the project. With *Filter by sheet*, each placed instance lists only keynotes visible on its own sheet.

**Data (RXD).** Legend: `view.legend`; legend components are annotation objects of kind `anno.legendcomponent` (`type` = `ifc:` reference to the family type object, `direction`, `detailLevel`, `hostLength`). Keynote legend: `schedules/<scheduleId>` of kind `schedule.keynotelegend`.

**Commands.**
`view.create_legend(name, scale) -> view_id`
`legend.place_component(view_id, type_id, direction, detail_level?, host_length?, point) -> element_id`
`view.create_keynote_legend(name, fields[]?, filter_by_sheet?) -> schedule_id`

**Acceptance criteria.**
- [ ] Changing a wall type's layer thickness updates its legend component.
- [ ] A keynote legend with *Filter by sheet* placed on two sheets lists different rows matching each sheet's keynotes.

---

## 5.10 Duplicating views and dependent views

**ID:** DOC-VIEW-011 · **Priority:** P0 (Duplicate, With Detailing), P1 (Dependent) · **Ref:** research/04 §1.8

**Purpose.** Reuse a view's set-up for another purpose, or split a large view across several sheets.

**Workflow.** Project Browser › right-click a view › Duplicate View ▸ Duplicate / Duplicate with Detailing / Duplicate as Dependent; or View tab › Create › Duplicate View ▾.

**Behaviour & rules.**

| Mode | Copies view definition (extents, graphics, template) | View-owned annotation | Relationship afterwards |
|---|---|---|---|
| Duplicate | Yes | Not copied | Independent |
| Duplicate with Detailing | Yes | Copied as new, independent elements (new GlobalIds) | Independent |
| Duplicate as Dependent | Yes | **Shared** with the parent and all siblings: an annotation placed in any of them belongs to the parent and is visible in all whose crop shows it | Child of the parent; listed under it in the browser |

- A dependent view has its own crop region (and may follow a scope box) but takes scale, template, view range, detail level, visibility settings and phase from its parent. Those properties are read-only in the dependent.
- *Convert to independent view* breaks the link: the dependent becomes an independent view and receives copies of the shared annotation that its crop shows.
- Deleting a parent requires deleting or converting its dependents first; the dialog offers both.
- Duplicate names follow the pattern `<name> Copy n` or, for dependents, `<name> - Dependent n`.

**Data (RXD).** Dependent views: view payload `parent` with `dependency = "dependent"`. Shared annotation is stored once, in the parent view's annotation document.

**Commands.** `view.duplicate(view_id, mode: plain|with_detailing|dependent, name?) -> view_id`; `view.convert_to_independent(view_id) -> ()`

**Acceptance criteria.**
- [ ] Duplicating with detailing yields annotation with new GlobalIds; moving a copied tag does not move the original.
- [ ] A text note added in dependent view A appears in dependent view B when B's crop contains it, and in the parent.
- [ ] Converting a dependent to independent leaves it with copies of the annotation inside its crop, and later edits to the parent's annotation no longer affect it.

---

## 5.11 Graphics control

*Built (2026-10-27), keynote legends:* View › *Keynote Legend* creates a schedule of kind `schedule.keynotelegend` and opens it: one row per key the model's keynote tags show (DOC-ANN-014), in natural order, with its text from the keynote table (*(not in the keynote table)* for a missing one); columns *Key Value* and *Keynote Text*, or one of them. *Filter by sheet* (asked on creation; `view.set_keynote_legend`) makes each copy placed on a sheet list only the keynotes of the views placed on that sheet; in its own tab it lists them all. Its columns, filters and sorting are its own: `schedule.set_fields` and the like refuse it (`schedule.keynote_legend`). `schedule.get_table(schedule_id, sheet_id?)` gives it as one sheet shows it. Commands: `view.create_keynote_legend(name?, fields?: [key, text], filter_by_sheet?)`, `view.set_keynote_legend(schedule_id, filter_by_sheet?, fields?)`. Legend views follow (below).

*Built (2026-10-27), legend views and legend components:* View › *Legend* makes a legend (`view.legend`, default 1 : 50) and opens it; Annotate › *Legend Component* places a type's drawing in it: choose the type (any wall, floor, door, window or component type of the model) and the view direction (plan, front elevation, back elevation, or section for walls and floors), then click where the drawing's lower-left corner goes. A legend component draws the **type**, not a model element: Reviera builds a sample of the type alone in a scratch copy of the model's types, levels and settings (a wall `host_length` long, default 2 m, 3 m high; a floor `host_length` × 1 m; a door or window in the middle of a wall `host_length` long, default its width plus 1.2 m; a component at the origin), regenerates it, and draws it: in plan cut at 1.2 m with a door's swing, from the front or back as an elevation, or in section across the wall or floor, hidden edges left out, in the elements' pens at the legend's scale. Editing the type (its layers, its size) changes every legend component of it at once. Legend components are annotation (`anno.legendcomponent`): in no schedule, count or IFC export. Legends hold text notes too, are picked and selected like other views (a component inside its drawing's extent, outlined when selected; Properties: type, view direction, host length), print to PDF, and can be placed on many sheets (DOC-CORE-005), which other views cannot. Commands: `view.create_legend(name?, scale?)`, `legend.place_component(view_id, type_id, direction?, host_length?, point)`, `legend.set_component(component_ids, direction?, host_length?, point?)`, `legend.list(view_id) -> {items, failed}`. Not yet: roof, ceiling, column, beam and stair types; a detail level per component; changing a legend's scale in the window; dragging components (`legend.set_component` moves them); legend components in model views.

- [x] *(Built 2026-10-27, `tests/sample_features.rs`.)* The annotated plan sample's *Types Legend* draws its wall type 1.5 m long in plan and 3 m high in section, its door in plan with the swing and from the front, and its window; layering the wall type to 240 mm redraws its plan swatch 240 mm thick; it goes on two sheets; a legend component cannot go in a plan or draw a level.

- [x] *(Built 2026-10-27, `tests/sample_features.rs`.)* The managed project's keynote legend, filtering by sheet, lists its plan's three keys on sheet *Plan and Keynotes*; placed again on a second sheet with a section carrying keynote `09`, that copy lists only `09`, and the first keeps its three; without the filter every copy lists all four.

### The graphics override precedence model
**ID:** DOC-GFX-001 · **Priority:** P0 (layers 0–1, 3, 6), P1 (layers 2, 4–5) · **Ref:** research/04 §3.1, §11 *Key behaviours* 5–6

**Purpose.** Define one deterministic function that decides whether, and how, each element (and each sub-part of it) is drawn in a view. Every graphics feature in this chapter is a layer of this function.

**Behaviour & rules.**

The resolved appearance of an element in a view is a record of independent **graphic attributes**:

| Attribute | Applies to |
|---|---|
| `visible` | whole element, or sub-category geometry |
| `projection_line` = {pattern, colour, weight} | edges seen in projection |
| `surface_pattern_fg` / `surface_pattern_bg` = {visible, pattern, colour} | projected faces |
| `surface_transparency` (0–100 %) | projected faces |
| `cut_line` = {pattern, colour, weight} | cut boundaries |
| `cut_pattern_fg` / `cut_pattern_bg` = {visible, pattern, colour} | cut faces |
| `halftone` (boolean) | all of the above |
| `detail_level` | which family/compound geometry is used |

Each attribute value can be *set* or *unset* ("by layer below"). The resolver walks the layers from lowest to highest. For each attribute, **the highest layer that sets it wins**. Three attributes combine differently:
- `visible`: an element is drawn only if **no** layer hides it (logical AND), except that layer 6c (*Reveal hidden*) can show hidden elements in its highlight colour.
- `halftone`: an element is halftone if **any** layer sets halftone (logical OR).
- `surface_transparency`: highest layer that sets it wins, but a phase or design-option override that applies transparency is combined multiplicatively with it.

**Layers (lowest to highest):**

| # | Layer | Scope | Source |
|---|---|---|---|
| 0a | Object styles | project, per category and sub-category | DOC-GFX-002 |
| 0b | Material graphics | project, per material: surface and cut patterns, shading colour | Chapter 2 materials |
| 0c | Family geometry settings | per family: sub-category assignment, visibility by detail level and by view direction | Chapter 2 families |
| 1 | View category overrides (including discipline profile, DOC-GFX-016) | per view, or the view's template | DOC-GFX-003 |
| 2 | View filters, in list order (the filter listed first wins conflicts) | per view or template | DOC-GFX-006 |
| 3 | Element overrides and per-edge linework | per view, per element | DOC-GFX-007 |
| 4 | Phase status graphics (via the view's phase filter) | per view | DOC-GFX-017 |
| 5 | Design option display, link display settings, workset visibility | per view | DOC-GFX-018, Chapter 6 |
| 6a | Worksharing display colours (when on) | per view, temporary | Chapter 6 COL-WS-004 |
| 6b | Temporary hide/isolate, isolate annotations | per view, temporary | DOC-GFX-008 |
| 6c | Reveal hidden | per view, temporary | DOC-GFX-008 |
| 7 | View-wide modifiers: Display Model = Halftone, underlay, halftone/underlay settings, visual style and graphic display options | per view | DOC-GFX-009, DOC-GFX-011, DOC-GFX-012 |

Further rules:
- **Templates.** When a view has an assigned template, every property included in the template is read from the template instead of the view. That includes the layer 1 and layer 2 tables when *V/G overrides* and *Filters* are included. The view's own values are kept, hidden, and become effective again if the template is removed or the property is excluded.
- **Linked models.** Elements of linked models enter the resolver with the host's layers or the link's own layers, according to the link's display setting (Chapter 6, COL-LNK-007).
- **Sub-category geometry.** Each piece of family geometry carries its sub-category, and the resolver runs per (element, sub-category) pair.
- **Phase "Not displayed"** (layer 4) and **design option not shown** (layer 5) set `visible = false`.
- The resolver is a pure function of (element state, view definition, project standards). Results are cached per view, keyed by element revision and view-definition revision, and recomputed in parallel (rayon) when invalidated.

**UI.** The **Graphics Inspector** (context menu › *Why does this look like this?*) shows, for the picked element, each layer's contribution to each attribute and which layer won. This is a Reviera addition with no counterpart in the reference.

**Data (RXD).** Layers 0a–0c are project and family data: RXD `standards/object_styles`, IFC materials plus their RXD graphics (Chapter 2), and family recipes. Layers 1–3 are the view payload blocks `categoryOverrides` (keyed by category and sub-category), `filterOverrides` (ordered list keyed by filter id) and `elementOverrides` / `linework` (keyed by GlobalId or GlobalId plus edge sub-reference). Because each entry has its own key, two users can change different overrides in one view and the three-way merge keeps both. Layer 6 is session state and is not saved; *Apply hide/isolate to view* writes layer 3 entries.

**Commands.** `graphics.explain(view_id, element_id, subcategory?) -> layer_trace` (query); `graphics.resolve(view_id, element_ids[]) -> resolved_attributes[]` (query).

**Acceptance criteria.**
- [ ] Object style sets wall projection lines to weight 3; a category override sets weight 5; a filter matching exterior walls sets weight 7; an element override on one wall sets weight 2. The resolved weights are 2 for that wall, 7 for other exterior walls, and 5 for interior walls.
- [ ] Two filters both set colour; swapping their order swaps the result.
- [ ] Hiding a category in the view and also setting an element override with `visible = true` leaves the element hidden (AND rule).
- [ ] `graphics.explain` returns the same winning layer for every attribute as the drawing shows, for every element in the 50 test cases of the graphics conformance suite.

### Object styles
**ID:** DOC-GFX-002 · **Priority:** P0 · **Ref:** research/04 §3.1 (1)

**Purpose.** Project-wide default appearance per category and sub-category.

**Workflow.** Manage tab › Settings › Object Styles opens a dialog with tabs Model Objects, Annotation Objects, Analytical Model Objects, Imported Objects. The user edits projection and cut line weight, line colour, line pattern and default material per row, and creates or deletes user sub-categories.

**Behaviour & rules.**
- Cut line weight is offered only for cuttable categories.
- Imported-object rows are created automatically for each layer of an imported or linked CAD file (Chapter 6) and can be edited like other sub-categories.
- Deleting a sub-category moves its geometry to the parent category after confirmation.

**Data (RXD).** RXD document `standards/object_styles`, kind `objectstyle`, one object per (category, sub-category): `projectionPen`, `cutPen`, `colour`, `linePattern`, `material`, plus the view-range attributes of DOC-VR-002. On IFC export with *Include presentation layers*, each styled category is also written as an `IfcPresentationLayerWithStyle` (name = category or sub-category; styles = `IfcCurveStyle` and `IfcFillAreaStyle`) assigned to its elements' representation items.

**Commands.** `objectstyle.set(category, subcategory?, projection_weight?, cut_weight?, colour?, line_pattern_id?, material_id?) -> ()`; `objectstyle.create_subcategory(category, name) -> subcategory_id`; `objectstyle.delete_subcategory(category, name) -> ()`

**Acceptance criteria.**
- [ ] Changing the Doors projection line weight changes every door in every view that has no override.
- [ ] An exported IFC opened in another viewer shows doors on a presentation layer named "Doors".

### Visibility/Graphics dialog (view category overrides)
**ID:** DOC-GFX-003 · **Priority:** P0 · **Ref:** research/04 §3.3 · screenshots/documentation/visibility_graphics_dialog_model_categories.png

**Purpose.** Per-view visibility and appearance by category, filter, link, workset, design option, imported file and point cloud.

**Workflow.**
1. View tab › Graphics › Visibility/Graphics (default shortcut VG or VV), or Properties › Visibility/Graphics › Edit.
2. The dialog title names the view. Tabs: **Model Categories**, **Annotation Categories**, **Analytical Model Categories**, **Imported Categories**, **Filters**, **Links**, **Worksets** (when worksets exist), **Design Options** (when options exist), **Point Clouds** (when clouds are linked), **Coordination Models** (when present).
3. In the category tabs, each row (category, expandable to sub-categories) has: Visible checkbox; Projection/Surface – Lines, Patterns, Transparency; Cut – Lines, Patterns; Halftone; Detail Level (By View / Coarse / Medium / Fine).
4. Clicking a cell opens the override editor for that attribute (line: pattern, colour, weight 1–16; pattern: foreground visible/pattern/colour and background visible/pattern/colour; transparency 0–100 %).
5. Footer controls: master *Show categories in this view* toggle, discipline filter, name search, All / None / Invert / Expand All, *Override Host Layers* (cut line styles per compound-layer function), Object Styles…, Edit…, Reset.

**Behaviour & rules.**
- If an assigned template controls a tab, that tab is read-only and a banner names the template.
- Changes preview live in the view while the dialog is open; Cancel reverts.
- **Override Host Layers:** for walls, floors, roofs and ceilings, the user can set the cut line style for each compound-layer function (Structure, Substrate, Thermal/Air, Finish 1, Finish 2, Membrane), and choose whether the outer boundary uses the category's cut line.
- The Annotation Categories tab covers dimensions, grids, levels, sections, elevations, callouts, tags, text, scope boxes, section boxes, crop boundaries, reference planes, matchlines, revision clouds, plan regions and so on.

**Data (RXD).** View payload `categoryOverrides`: per category and sub-category, `visible`, `projectionLine`, `surfaceFg`, `surfaceBg`, `transparency`, `cutLine`, `cutFg`, `cutBg`, `halftone`, `detailLevel`; and `hostLayerCutStyles` (per compound-layer function).

**Commands.**
`view.set_category_visibility(view_id, categories[], visible) -> ()`
`view.set_category_override(view_id, category, subcategory?, override) -> ()`
`view.clear_category_override(view_id, category, subcategory?) -> ()`
`view.set_host_layer_cut_styles(view_id, function_styles{}) -> ()`
`view.visibility_graphics(view_id? (default: active)) -> ()` opens the dialog described above for the view (or for a template, when given a template id). The dialog is a UI entry point (DOC-CORE-007): OK applies all changed rows through the commands above as one undo step named "Visibility/Graphics"; Apply does the same and keeps the dialog open; Cancel discards. Validation: line weights 1–16, transparency 0–100 %, pattern ids must exist; an invalid cell is outlined and OK is disabled until it is fixed. A row controlled by the view's template, or a view that is a dependent, shows read-only rows with the reason.

**Acceptance criteria.**
- [ ] Hiding Furniture in a plan hides every furniture element in that plan and nowhere else.
- [ ] Setting a red cut pattern on Walls in a view applies to walls without a filter or element override.
- [ ] When a template controls Model Categories, the tab is read-only and `view.set_category_override` on that view returns `view.template_controlled`.

### View templates
**ID:** DOC-GFX-004 · **Priority:** P1 · **Ref:** research/04 §3.2

**Purpose.** Standardise view settings across many views and keep them in step.

**Workflow.**
1. View tab › Graphics › View Templates ▾ › *Create Template from Current View* / *Apply Template Properties to Current View* / *Manage View Templates*.
2. Manage dialog: list of templates (filter by view kind), property list with value and *Include* checkbox per property, Duplicate, Rename, Delete, *Views using this template* list.
3. To bind a view: Properties › View Template › pick a template (assign). To stamp once: Apply Template Properties (apply).

**Behaviour & rules.**
- **Apply** copies the included property values into the view once; there is no lasting link.
- **Assign** creates a lasting link: included properties become read-only in the view and follow template edits immediately.
- Templates have a view kind group: plans (floor, structural, area), ceiling plans, sections/elevations/details, 3D, drafting, schedules, walkthroughs, renderings. A template can be assigned only to views of a compatible kind.
- Template-controllable properties: Scale, Display Model, Detail Level, Parts Visibility, V/G Model, V/G Annotation, V/G Analytical, V/G Imported, Filters, Links, Worksets, Design Options, Model Display (visual style), Shadows, Sketchy Lines, Lighting, Photographic Exposure, Background, Far Clipping, Phase, Phase Filter, Discipline, Show Hidden Lines, Colour Scheme Location, Colour Schemes, Depth Clipping, View Range, Orientation, Underlay Orientation, Section Box (3D), Schedule appearance (schedules). Crop and annotation crop are never template properties.
- The view family type can name a *template for new views* and whether new views are bound to it.
- Deleting a template used by views asks for a replacement or *None*; with *None*, views keep the values they had.
- Templates can be copied between projects (DOC-MNT link: Chapter 6, COL-MNT-005 *Transfer standards*).

**Data (RXD).** RXD document `views/templates`, kind `view.template`: `appliesTo` (view kind group), `included` (list of controlled property names), and the same payload blocks as a view for the included properties. The binding is the view's `template` field; a template keeps no list of its views (the list is computed), so assigning views never edits the template object.

**Commands.**
`template.create_from_view(view_id, name) -> template_id`
`template.apply(view_ids[], template_id) -> ()`
`template.assign(view_ids[], template_id?) -> ()`
`template.set_include(template_id, property, included) -> ()`
`template.set_property(template_id, name, value) -> ()`
`template.delete(template_id, replacement_id?) -> ()`
`template.manage() -> ()` opens the *View Templates* dialog: on the left, the template list with a kind filter (Show type: all, plans, ceiling plans, sections and elevations, 3D, drafting, schedules, walkthroughs, renderings) and a name search; at the bottom, Duplicate, Rename and Delete; in the centre, the property table (Parameter, Value, *Include*) of the selected template, where each value opens the same editor as in the view (for example *Edit…* for V/G and View Range); on the right, *Views using this template* (computed, with Open and *Assign to selection*). Validation: names must be unique among templates of the same kind group; values follow the rules of the corresponding view property (for example the view range ordering of DOC-VR-001); Delete of a template in use asks for a replacement or *None*. Changes are applied as one undo step on OK.

**Acceptance criteria.**
- [ ] Changing the scale in a template changes the scale of every view assigned to it, in one undo step.
- [ ] Unticking *Include* for Detail Level makes Detail Level editable in assigned views while other properties remain locked.
- [ ] Assigning a 3D template to a plan is rejected.

### Temporary view properties
**ID:** DOC-GFX-005 · **Priority:** P1 · **Ref:** research/04 §3.2 *Temporary View Properties*

**Purpose.** Try out a template or change settings temporarily without altering the saved view.

**Workflow.**
1. View Control Bar › Temporary View Properties ▾ › *Enable Temporary View Properties*, or *Temporarily Apply Template Properties* (a dialog lists templates compatible with the view's kind, with a preview of the properties each controls).
2. The user changes view properties, graphics overrides or filters as usual; the changes show at once.
3. *Restore View Properties* ends the mode and puts back the saved values.

**UI.** View Control Bar menu (three items above). While the mode is on, the view has a coloured frame (default purple) and a label "Temporary View Properties" in its top-left corner. The Properties palette shows the temporary values with a marker.

**Behaviour & rules.**
- Reviera takes a snapshot of the view definition (every property, `categoryOverrides`, `filterOverrides`, `elementOverrides`, crop, view range) when the mode starts. Edits in the mode change a session copy; *Restore* discards the copy.
- The edits are not undoable steps of the model, and they do not mark the project as changed. Undo inside the mode works on the session copy only.
- If the view has an assigned template, *Temporarily Apply Template Properties* overrides the assigned template's included properties for the session; the assignment itself does not change.
- Printing and export use the saved properties unless the user explicitly chooses *As displayed* in the output dialog.
- Temporary state is per session, per view and per user: it is not saved, not sent to BIMserver, and is lost when the view is closed or the project is closed. Closing the view asks whether to *Keep as view properties* (applies the session copy through normal commands, one undo step) or discard it.
- The mode is refused on dependent views whose properties come from the parent (DOC-VIEW-011), with a message naming the parent.

**Data.** None; session state only (DOC-CORE-008).

**Commands.** `view.temporary_properties(view_id, mode: enable|apply_template|restore, template_id?) -> ()`. Keeping the temporary state as the view's properties uses `view.set_property`, `view.set_category_override` and the other per-view commands, grouped as one undo step.

**Acceptance criteria.**
- [ ] After temporarily applying a template and restoring, every view property equals its value before.
- [ ] Changing the scale while the mode is on does not mark the project as modified, and a PDF export (not *As displayed*) uses the saved scale.
- [ ] Closing the view with *Keep as view properties* creates one undo step that, when undone, restores the saved values.

### View filters
**ID:** DOC-GFX-006 · **Priority:** P1 · **Ref:** research/04 §3.4; web: Filters Dialog · screenshots/documentation/view_filter_rules_nested_sets.png

**Purpose.** Override graphics or hide elements by rule or by saved selection.

**Workflow.**
1. View tab › Graphics › Filters opens the filter manager: list of filters, categories pane, rules pane.
2. **Rule-based filter:** choose categories; build rules `parameter · operator · value` grouped in nested sets, each set being *All rules true* (AND) or *Any rule true* (OR); Add Rule, Add Set, Delete.
3. **Selection filter:** select elements, then Modify › Selection › Save; edit later with Edit Selection (add/remove mode).
4. In the view's graphics dialog, Filters tab: Add, Remove, Up, Down; per row Enable, Visible, and the same override columns as categories.

**Behaviour & rules.**
- Operators: equals, does not equal, greater than, greater than or equal, less than, less than or equal, contains, does not contain, begins with, does not begin with, ends with, does not end with, has a value, has no value. Numeric comparisons are unit-aware and use a tolerance of half the display precision.
- The parameter list offers parameters common to all chosen categories, including shared and project parameters, plus the pseudo-parameters Workset, Phase Created, Phase Demolished, Design Option, Sheet Collection, IFC Class and IFC PredefinedType.
- Filters apply to linked elements when the link is displayed *By host view* or custom with host filters (Chapter 6).
- Filter order in a view decides conflicts (first listed wins), as specified in DOC-GFX-001.
- A filter definition is project data; applying it to a view stores only the view-specific overrides.
- Rule evaluation is compiled once per filter into a predicate and run in parallel over candidate elements.

**Data (RXD).** RXD document `views/filters`: kind `filter.rule` (`categories`, `ruleTree` of nested `{op: and|or, children}` and `{parameter, operator, value, unit}` nodes) and kind `filter.selection` (`members`: list of `ifc:` references). Per-view application is the view's `filterOverrides` list (DOC-GFX-001).

**Commands.**
`filter.create_rule(name, categories[], rule_tree) -> filter_id`
`filter.update_rule(filter_id, categories?, rule_tree?) -> ()`
`filter.create_selection(name, element_ids[]) -> filter_id`
`filter.edit_selection(filter_id, add_ids[], remove_ids[]) -> ()`
`filter.delete(filter_id) -> ()`
`view.add_filter(view_id, filter_id, index?, visible?, enabled?, override?) -> ()`
`view.set_filter_override(view_id, filter_id, override, visible?, enabled?) -> ()`
`view.reorder_filters(view_id, filter_ids[]) -> ()`
`view.remove_filter(view_id, filter_id) -> ()`
`filter.evaluate(filter_id, view_id?) -> element_id[]` (query)
`view.filters() -> ()` opens the filter manager (View tab › Graphics › Filters, or *Edit/New…* on the Filters tab of the graphics dialog). Left pane: all filters, rule-based and selection-based, with New, Duplicate, Rename and Delete. Middle pane (rule-based filters): category tree with a discipline filter, *Hide un-checked categories*, Check All and Check None. Right pane: the rule tree (Add Rule, Add Set, Delete, AND/OR per set, drawn as nested brackets). Selection filters show their member count and *Edit selection*. Validation: at least one category; each rule needs a parameter available for every chosen category, an operator valid for its data type and, except for *has a value* / *has no value*, a value in the parameter's unit; unchecking a category whose parameter a rule uses marks that rule invalid; names are unique. Delete removes the filter from every view and template that uses it, after a confirmation that counts them. A live count ("matches 124 elements in the active view") updates as rules change.

**Acceptance criteria.**
- [ ] A filter "Walls where (Fire Rating = 60 OR Fire Rating = 90) AND Function = Interior" colours exactly the matching walls.
- [ ] Disabling the filter in the view restores the previous appearance without removing the filter row.
- [ ] `filter.evaluate` returns the same set the view highlights.

### Element and category overrides, hiding and linework
**ID:** DOC-GFX-007 · **Priority:** P0 (hide), P1 (overrides, linework) · **Ref:** research/04 §3.5

**Purpose.** Change the appearance of specific elements, or hide them, in one view.

**Workflow.**
1. Select elements › right-click › Override Graphics in View ▸ By Element… / By Category… / By Filter… (By Filter creates a filter from the selection's categories and a common parameter the user picks).
2. The element override dialog has Visible, Halftone, Projection Lines, Surface Patterns (foreground/background), Surface Transparency, Cut Lines, Cut Patterns.
3. Hide in View ▸ Elements (EH) / Category (VH) / By Filter.
4. Modify › View › Linework (LW): pick a line style, then click edges to restyle them in this view only. `<By Category>` restores the default.

**Behaviour & rules.**
- Element overrides are view-specific and stored per element.
- Linework overrides attach to an edge reference (element GlobalId plus a stable edge descriptor, Chapter 2). When regeneration can no longer find the edge, the override is dropped and a warning is logged.
- Hiding an element in a view hides its hosted annotation (tags, dimensions referencing only that element) in that view too.

*Built (2026-10-28), hiding:* Hide in View (EH) and Hide Category in View (VH) on View › Hide/Isolate, the View menu and the quick menu (*Hide in View (saved)*), and *Unhide All in View*. A view's payload keeps `hidden` `{elements: [GlobalId], categories: [category key]}`; what it lists is left out of the view on screen (plans, sections, elevations and 3D), in printed and sheet drawings and in view images, and is saved and undoable. Commands: `view.hide(view_id, element_ids?, categories?, by_category?)`, `view.unhide(view_id, element_ids?, categories?, all?)`, `view.hidden(view_id) -> {elements, categories, count}`. Its tags, keynotes and spots in that view (and its floating labels in a 3D view) are hidden with it. Not yet: hiding by filter, hiding dimensions that reference only hidden elements, and linework.

*Built (2026-10-28), overrides:* Override Graphics in View… (View › Hide/Isolate, the View menu and the quick menu) sets **surface transparency** (0–100 %) and **halftone** for the selected elements, or for their categories, in one view: transparency draws surfaces translucent in 3D and fades lines in 2D, halftone draws lines at half strength in plans, sections and elevations; both are saved, undoable and printed. View payload `elementOverrides` `{GlobalId: {transparency?, halftone?}}` and `categoryOverrides` `{category key: {…}}`; an element's own value wins over its category's for each attribute, and halftone is on if either sets it. Commands: `view.override_elements(view_id, element_ids[], override)`, `view.clear_element_overrides(view_id, element_ids[]?, all?)`, `view.set_category_override(view_id, category, override)`, `view.clear_category_override(view_id, category)`, `view.overrides(view_id)`. Not yet: projection and cut lines, surface and cut patterns, and the Visibility/Graphics dialog.

**Data (RXD).** View payload `elementOverrides` (keyed by `ifc:` GlobalId), `hidden` (list of GlobalIds), `hiddenCategories`, and `linework` (keyed by `ifc:<GlobalId>#<edge sub-reference>` → line style id).

**Commands.**
`view.override_elements(view_id, element_ids[], override) -> ()`
`view.clear_element_overrides(view_id, element_ids[]) -> ()`
`view.hide_elements(view_id, element_ids[]) -> ()`
`view.unhide_elements(view_id, element_ids[]) -> ()`
`view.set_linework(view_id, edge_refs[], line_style_id?) -> ()`
`filter.create_from_selection(view_id, element_ids[], parameter, name) -> filter_id`

**Acceptance criteria.**
- [ ] Hiding one door hides its tag in that view only.
- [ ] A linework override on a wall edge survives moving the wall; it is dropped with a warning if the edge disappears (for example the wall becomes curved).

### Hidden lines by element and cut profile
**ID:** DOC-GFX-020 · **Priority:** P1 (hidden lines by element), P2 (cut profile) · **Ref:** research/04 §1 (Show Hidden Lines property), §3.5; web: Show Hidden Lines by Element, Change the Cut Profile of Elements

**Purpose.** Two view-specific drafting corrections on model graphics: draw chosen hidden edges of one element behind another as hidden lines, and change the drawn shape of a cut face without changing the model.

**Workflow (hidden lines by element).**
1. Set the view's *Show Hidden Lines* to *By Discipline* (the tool is disabled otherwise, with a tooltip saying why).
2. View tab › Graphics › *Show Hidden Lines*: pick the element in front (the obscuring element), then one or more elements behind it. With *Multiple* on the contextual tab, several obscuring elements can be picked first, then Enter.
3. The edges of the hidden elements that the obscuring elements cover are drawn in the `<Hidden>` style.
4. *Remove Hidden Lines* works the same way and removes the pair.

**Workflow (cut profile).**
1. View tab › Graphics › *Cut Profile*, with the option *Face* (edit the whole cut face boundary) or *Boundary between faces* (edit the line between two adjoining cut faces, for example two layers or two elements).
2. Hover a cut face or boundary in a plan, ceiling plan, section, callout or detail view; click to enter sketch mode.
3. Sketch an open chain of lines and arcs that starts and ends on the face boundary. A flip arrow on the first segment shows the side that remains; click it to change sides. In *Boundary between faces* mode only the two end segments are sketched; Reviera adds the connecting segment along the old boundary.
4. Finish. To restore the original shape, select the profile and choose *Remove Cut Profile*.

**Behaviour & rules.**
- **Hidden lines by element** applies in plans, sections, elevations, callouts and orthographic 3D views. It is not available in perspective views, for linked elements, or when the view's discipline is Mechanical, Electrical or Plumbing.
- Pairs are evaluated in the hidden-line stage (DOC-HLR-002 step 3, DOC-HLR-003): for a pair (A obscures B), edges of B hidden only by A are kept and classified as hidden edges; edges of B hidden by other elements stay hidden. The result uses the hidden-line sub-category of B's category, so object styles and V/G overrides apply.
- Pairs of view-owned detail components work by draw order (DOC-ANN-020): the obscuring component must be in front of the hidden one, otherwise the pair has no effect and the command says so.
- A pair whose element is deleted is removed silently; a pair whose elements no longer overlap in the view has no effect but is kept.
- **Cut profile** changes only the 2D cut region of one element (or one compound layer) in one view. It is applied in stage 6 of the pipeline (DOC-HLR-001) as a 2D Boolean on the cut region: the sketched side is added to, or removed from, the region, and adjoining regions are trimmed so that no two cut regions overlap. Projection linework, 3D geometry, quantities, schedules and other views are unchanged.
- Sketch validation at Finish: the chain must start and end on the selected boundary, must not form a closed loop, must not cross itself, and must not cross the starting boundary (in *Face* mode). An invalid sketch keeps the user in sketch mode with the offending segment highlighted.
- When the element changes so that the cut face no longer exists (for example the wall is moved out of the cut plane), the profile is kept but inactive and is reported in the warnings list; it applies again if the face returns.
- Cut profiles are not copied by *Insert 2D Elements from File* (DOC-VIEW-009) and are copied by *Duplicate with Detailing* (DOC-VIEW-011).
- **Interactive tool.** The cut profile sketch follows Conventions §2.1: micro-operations `cutprofile.segment(curve)`, `cutprofile.delete(index)`, `cutprofile.flip()` with a replay function that rebuilds the sketch after a crash.

**Data (RXD).** Hidden-line pairs: view payload `hiddenLinePairs[] {obscuring, hidden}` (`ifc:` or annotation references). Cut profiles: annotation objects of kind `anno.cutprofile` in `views/annotation/<viewId>`: `target` (element GlobalId plus face or layer sub-reference, or the two faces of a boundary), `mode` (face | boundary), `sketch` (curves in view coordinates), `keepSide`. Nothing is written to the model's IFC geometry; IFC annotation export (DOC-IFC-002) carries the result only in the drawing linework.

**Commands.**
`view.show_hidden_lines(view_id, obscuring_ids[], hidden_ids[]) -> ()` (creates one pair per combination; fails with `view.hidden_lines_not_by_discipline` when the view setting is not By Discipline)
`view.remove_hidden_lines(view_id, element_ids[]) -> ()` (removes every pair in which a listed element is the hidden element)
`view.cut_profile(view_id, target_ref, loops[]) -> ()` (each loop is `{curves[], keep_side: left|right, mode?: face|boundary}`; creates or replaces the profile of that target)
`view.remove_cut_profile(view_id, target_ref) -> ()`

**Acceptance criteria.**
- [ ] In a north elevation with Show Hidden Lines = By Discipline, pairing the north wall with the south entrance door draws the door outline dashed; switching the setting to None hides it.
- [ ] The tool is disabled in a perspective view and in a Mechanical plan.
- [ ] A cut profile that notches a slab edge in one section changes that section only; the slab's volume in schedules is unchanged.
- [ ] A sketch that starts on the boundary but ends inside the face is refused at Finish with the open end highlighted.

### Temporary hide/isolate, reveal hidden, isolate annotations
**ID:** DOC-GFX-008 · **Priority:** P0 · **Ref:** research/04 §3.5

**Purpose.** Quick, non-persistent visibility tools while working.

**Behaviour & rules.**
- View Control Bar glasses menu: Hide Element (HH), Isolate Element (HI), Hide Category (HC), Isolate Category (IC), Reset (HR), Apply Hide/Isolate to View. While active, the view has a coloured border and label.
- Temporary state does not print or export, and is not saved.
- *Apply Hide/Isolate to View* converts the temporary state into permanent element or category hiding (layer 3).
- **Scope.** Each tool acts on the picked elements *by element*, *by element and its children* (subtree), *by category* or *by type*. The children of an element come from the model tree (UI-TREE-001): a storey's children are the elements it contains, a host's children are the elements it hosts (a wall's doors and windows), an assembly's or aggregate's children are its parts, and a grid system's children are its grid lines. Scopes are resolved when the tool runs: elements created later are not included, except that isolating keeps showing new elements created in the view while it is isolated, so new work does not vanish.
- **Temporary translucency** (Translucent Element, HT): the elements stay visible and selectable, drawn at the translucency opacity (default 25 %, set in Options › Graphics), without edges. Picking prefers opaque elements: a click on a translucent element selects it only when nothing opaque is behind it under the cursor.
- **Isolate, ghost others** (HG): isolates like Isolate Element, but draws everything else translucent instead of hiding it, for context.
- Hide wins over translucent, and isolate applies first: an isolated element that is also hidden is hidden. Showing (unhiding) an element clears any temporary state it has.
- **Reveal Hidden Elements** (light-bulb icon, RH): shows elements hidden by any layer in a highlight colour (default magenta), with the view framed in that colour; *Unhide Element* / *Unhide Category* act on the picked items and say which layer hid them.
- **Isolate Annotations:** model elements are halftoned and unselectable so the user can work on annotation; an *Include categories* list (persisting per user across views) keeps chosen model categories selectable.

**Commands.**
`view.temp_hide(view_id?, element_ids[], by: element|subtree|category|type) -> ()` (UI-VCB-001)
`view.temp_isolate(view_id?, element_ids[], by, ghost_others?: bool) -> ()`
`view.temp_translucent(view_id?, element_ids[], by, opacity?) -> ()`
`view.temp_show(view_id?, element_ids[], by) -> ()` (clears temporary hiding and translucency of those elements, and adds them to an active isolation)
`view.temp_reset(view_id? (default: active)) -> ()`
`view.temp_apply(view_id? (default: active)) -> {hidden, overridden}` (*built 2026-10-28:* temporary hiding, and everything outside a plain isolation, becomes Hide in View; translucency, and everything outside a ghosting isolation, becomes a transparency override; DOC-GFX-007)
`view.reveal_hidden(view_id, on) -> ()`
`view.isolate_annotations(view_id, on, include_categories[]?) -> ()`

**Acceptance criteria.**
- [ ] Temporarily isolating three walls, then printing, prints the full view.
- [ ] Hide Element and Children on Level 2 in the model tree hides every wall, floor, door and window on Level 2 in that view only; Reset shows them again.
- [ ] Making a wall translucent shows its doors through it, and clicking where the door is selects the door.
- [ ] In reveal-hidden mode, an element hidden by a filter shows in magenta and *Unhide* reports "hidden by filter <name>".

### Line weights, halftone and underlay settings
**ID:** DOC-GFX-009 · **Priority:** P0 · **Ref:** research/04 §3.6

**Purpose.** Map the 16 logical pens to printed widths.

**Workflow.** Manage › Additional Settings ▾ › Line Weights: tabs *Model Lines* (16 pens × list of view scales), *Perspective Lines* (16 pens), *Annotation Lines* (16 pens); Add/Delete scale columns. Manage › Additional Settings › Halftone/Underlay: underlay line weight and pattern, halftone brightness (0–100).

**Behaviour & rules.**
- Model-line pen widths depend on the view scale: for a view scale not listed, Reviera uses the column of the nearest listed scale, measured by ratio (the larger of the two scale denominators divided by the smaller); ties go to the coarser scale.
- Perspective and annotation pens do not depend on scale.
- Widths are in paper millimetres and are used unchanged by every output (DOC-CORE-006). On screen, *Thin lines* mode (View Control Bar) draws all lines one pixel wide.
- Halftone blends the resolved colour towards the background by the brightness percentage.

**Data (RXD).** RXD document `standards/line_weights` (kind `penset`: model pens per scale, perspective pens, annotation pens) and the `halftone` section of `project/settings`.

**Commands.** `lineweight.set(table: model|perspective|annotation, pen, width_mm, scale?) -> ()`; `lineweight.add_scale(scale) -> ()`; `lineweight.remove_scale(scale) -> ()`; `settings.set_halftone(brightness?, underlay_weight?, underlay_pattern_id?) -> ()`

**Acceptance criteria.**
- [ ] With pen 5 at 0.50 mm for 1:50 and 0.35 mm for 1:200 (and no other scale columns), a 1:50 view prints pen 5 at 0.50 mm, a 1:200 view at 0.35 mm, and a 1:75 view uses the nearest column (1:50, 0.50 mm). If a 1:100 column is added, the 1:75 view uses it instead, because 1:100 is nearer by ratio (1.33 against 1.5).
- [ ] A PDF printed from a view matches the on-screen line widths at 100 % zoom on a calibrated display (within one device pixel).

### Detail level
**ID:** DOC-GFX-010 · **Priority:** P0 · **Ref:** research/04 §3.7 *Detail Level*

**Purpose.** Choose how much geometric detail elements show.

**Workflow.** View Control Bar › Detail Level (Coarse, Medium, Fine), or Properties › Graphics › Detail Level. Per category: Visibility/Graphics › Detail Level column (By View, Coarse, Medium, Fine).

**Behaviour & rules.**
- Coarse / Medium / Fine per view, overridable per category in the graphics dialog (layer 1) and by filter. The resolved detail level is an attribute of DOC-GFX-001, so the highest layer that sets it wins.
- Family geometry declares which detail levels it appears at (Chapter 2). Geometry flagged for none of the levels is never drawn; Reviera warns about it in the family editor.
- Walls, floors, roofs and ceilings show compound layers only at Medium and Fine; at Coarse they show their outline filled with the type's *Coarse scale fill pattern* and colour, with the category's cut line.
- Structural framing and columns switch between a single-line symbol (Coarse) and their true section (Medium, Fine) as defined in Chapter 4.
- MEP segments switch between single-line (Coarse) and double-line (Medium, Fine) as defined in Chapter 4.
- Manage › Additional Settings › Detail Level maps scale ranges to the default detail level of new views. The default map: 1:1 to 1:20 Fine, 1:25 to 1:100 Medium, coarser than 1:100 Coarse. The map applies only when a view is created or its scale is set through the New dialog; changing an existing view's scale never changes its detail level.
- Changing the detail level only re-runs stage 2 of the pipeline (representation choice, DOC-HLR-001) for affected elements; tessellations of each level are cached separately.
- Dependent views and template-bound views take the detail level from their parent or template (read-only).

**Data (RXD).** View payload `detailLevel`; per-category override in `categoryOverrides[*].detailLevel`; scale-to-detail-level map in `project/settings` (`detailLevelMap[] {maxScale, level}`). On IFC export, representations of different detail follow the sub-context convention of Chapter 2.

**Commands.** `view.set_detail_level(view_id, level) -> ()`; `settings.set_detail_level_map(ranges[]) -> ()` (each range `{max_scale_denominator, level}`; ranges must cover all scales without gaps or overlaps, otherwise `detail_level_map.invalid`).

**Acceptance criteria.**
- [ ] Switching a 1:100 plan from Coarse to Fine shows wall layers and door frame detail.
- [ ] A category override of Fine for Doors in a Coarse plan draws doors at Fine and walls at Coarse.
- [ ] Creating a 1:20 plan with the default map gives it Fine; changing its scale to 1:200 later leaves it at Fine.
- [ ] A detail-level map with a gap between 1:50 and 1:75 is rejected with `detail_level_map.invalid`.

*Built (2026-10-16):* `view.set_detail_level` and the view payload's `detailLevel` (Medium when unset); a plan's Properties show Detail Level. Coarse plans draw structural framing as single lines (STR-SET-001). Not yet: compound layers and coarse fills of walls, floors, roofs and ceilings by detail level, per-category and filter overrides, component geometry by detail level, the scale-to-detail-level map.

### Visual styles
**ID:** DOC-GFX-011 · **Priority:** P0 (Wireframe, Hidden Line, Shaded, Consistent Colours), P1 (Textures, Realistic), P2 (Path traced) · **Ref:** research/04 §3.7 *Visual Styles*

**Purpose.** Choose how surfaces and edges are drawn.

| Style | Behaviour in Reviera |
|---|---|
| Wireframe | All edges, no surfaces; hidden edges shown. |
| Hidden Line | Visible edges only, surfaces in the background colour. This is the documentation style; for plans, sections and elevations it uses the exact HLR pipeline when printing and the GPU-assisted pipeline on screen (§5.12). |
| Shaded | Surfaces lit with the material's shading colour; edges optional. |
| Consistent Colours | Material shading colour without lighting. |
| Textures | Consistent colours with the material's texture maps. |
| Realistic | Physically based materials with real-time lighting, shadows and ambient occlusion. |
| Path Traced | Progressive path tracing in the viewport (optional renderer, DOC-OUT-011). Refines while the camera is still. |

**UI.** View Control Bar visual style menu; default shortcuts WF (wireframe), HL (hidden line), SD (shaded).

**Commands.** `view.set_visual_style(view_id, style) -> ()`

**Acceptance criteria.**
- [ ] Each style can be set from Lua and MCP and is saved with the view.
- [ ] The Path Traced style is listed only when an optional renderer is installed; otherwise it is greyed out with a tooltip explaining why.

### Graphic display options
**ID:** DOC-GFX-012 · **Priority:** P1 (P2 for sketchy lines, depth cueing, photographic exposure) · **Ref:** research/04 §3.7 *Graphic Display Options*; web: Define Sun Settings for a View

**Purpose.** Fine-tune the rendering of a view.

**Behaviour & rules.** The Graphic Display Options dialog (default shortcut GD) groups:
- *Model display*: style, show edges, anti-aliasing, whole-view transparency (0–100 %), silhouette edges (line style).
- *Shadows*: cast shadows, ambient shadows (screen-space ambient occlusion).
- *Sketchy lines*: on/off, jitter (0–10), extension (0–10). Implemented by deterministic per-edge noise (the same edge always looks the same), applied in paper space, and therefore also printed.
- *Depth cueing* (sections and elevations with far clipping): fade start and end (percent between near and far planes), fade limit (percent).
- *Lighting*: scheme (exterior or interior; sun only, sun and artificial, artificial only), sun setting (date, time, location, or named preset), artificial light groups, sun intensity, ambient light, shadow darkness (each 0–100).
- *Photographic exposure* (Realistic and Path Traced): automatic or manual exposure value, highlights, shadows, saturation, white point.
- *Background*: none, sky, gradient (sky/horizon/ground colours), image (3D, sections, elevations).
- *Save as View Template…*.

- **Sun Settings dialog** (`sun.settings`; opened from the View Control Bar sun path menu, Manage › Additional Settings › Sun Settings, the Lighting group above and the Render dialog). Left: a preset list (built-in presets such as summer and winter solstice and equinox at noon, plus user presets; *In-session* is the unsaved current setting), with Duplicate, Rename and Delete. Right, by mode:
  - *Still*, *Single Day*, *Multi-Day* (location-based): location (from the project's site location, Chapter 6; *Browse* opens the location dialog), date (start and end for multi-day), time (start and end for single and multi-day, or *Sunrise to sunset*), time interval (15 min, 30 min, 1 h, 1 day), and *Ground plane at level* with a level picker;
  - *Lighting* (direction-based): azimuth (0–360° from true north), altitude (0–90°), *Relative to view* (azimuth measured from the view's up direction instead of true north), and *Ground plane at level*.
  Validation: end date not before start date, end time after start time, interval shorter than the time range, azimuth and altitude within range. *Apply* previews in the active view; OK stores the chosen setting on the view; *Save Settings* stores a named preset. Solar study animation uses the same setting (DOC-OUT-012, `solarstudy.set`).
- The ground plane receives shadows in shaded views only; rendered views need modelled ground (DOC-OUT-011).

**Data (RXD).** `graphicDisplay` block of the view (or template) payload: `edges`, `antiAlias`, `transparency`, `silhouette`, `shadows`, `ambientOcclusion`, `sketchy {jitter, extension}`, `depthCue {start, end, limit}`, `lighting` (including `sun {preset?, mode, date, endDate, time, endTime, interval, azimuth, altitude, relativeToView, groundPlaneLevel}`), `exposure`, `background`. Sun presets: `project/settings` › `sunPresets[]`.

**Commands.** `view.set_graphic_display(view_id, options) -> ()`; `sun.settings() -> ()` (opens the dialog above for the active view; its changes are applied through `view.set_graphic_display` and `solarstudy.set` as one undo step)

**Acceptance criteria.**
- [ ] Turning on sketchy lines in a hidden-line elevation shows the same jitter on screen and in the PDF.
- [ ] Depth cueing fades a façade 30 m away more than one 5 m away.
- [ ] A *Lighting* sun with azimuth 90° and altitude 30° casts shadows towards the west in a shaded 3D view, independent of the project date.
- [ ] A multi-day setting with an end date before its start date cannot be confirmed.

### Line styles and line patterns
**ID:** DOC-GFX-013 · **Priority:** P0 (styles), P1 (pattern editing) · **Ref:** research/04 §3.6

**Purpose.** Named line appearances for drafting and overrides, and the dash patterns they use.

**Behaviour & rules.**
- **Line styles** are sub-categories of the *Lines* category: name, weight (pen), colour, pattern. Built-in styles that cannot be deleted: `<Thin Lines>`, `<Medium Lines>`, `<Wide Lines>`, `<Hidden>`, `<Overhead>`, `<Beyond>`, `<Centerline>`, `<Demolished>`, `<Sketch>`, `<Area Boundary>`, `<Room Separation>`, `<Space Separation>`, `<Insulation Batting Lines>`, `<Lines>`, `<Invisible lines>`, `<Axis of rotation>`. Users add their own.
- **Line patterns** are sequences of dash, space and dot segments with lengths in paper millimetres. Patterns are applied along the whole polyline (continuing across vertices), so corners look consistent. A dot is drawn as a zero-length dash with round caps.
- Deleting a style or pattern in use asks for a replacement.

**Data (RXD).** RXD documents `standards/object_styles` (line styles are sub-categories of *Lines*) and `standards/line_patterns` (kind `linepattern`: ordered `segments` of `{type: dash|space|dot, length_mm}`). On IFC export, patterns become `IfcCurveStyleFont` with `IfcCurveStyleFontPattern` segments.

**Commands.** `linestyle.create(name, weight, colour, pattern_id?) -> style_id`; `linestyle.update(style_id, weight?, colour?, pattern_id?, name?) -> ()`; `linestyle.delete(style_id, replacement_id) -> ()`; `linepattern.create(name, segments[]) -> pattern_id`; `linepattern.update(pattern_id, segments[]) -> ()`; `linepattern.delete(pattern_id, replacement_id?) -> ()`

**Acceptance criteria.**
- [ ] A dash-dot pattern of 6 mm dash, 2 mm space, dot, 2 mm space prints with those lengths on paper at any view scale.

### Fill patterns
**ID:** DOC-GFX-014 · **Priority:** P0 (simple), P1 (custom import, rotation) · **Ref:** research/04 §3.6

**Purpose.** Hatches for surfaces, cuts and regions.

**Behaviour & rules.**
- Two kinds: **Drafting** patterns are sized in paper millimetres and scale with the view; **Model** patterns are sized in real units and stay attached to the geometry they cover (for example brick courses or tiles).
- *Simple* patterns: parallel lines or crosshatch, with angle and spacing.
- *Custom* patterns: imported from the open, widely used text `.pat` hatch definition format, with an import scale. Reviera implements its own parser and ships its own pattern library written for the project.
- Orientation in host layers: *Orient to view*, *Keep readable*, *Align with element*.
- **Solid fill** is a built-in drafting pattern.
- Model patterns on a face can be aligned and moved: the user can tab-select a pattern line and use Align/Move to set the pattern origin and rotation for that face, stored per face.
- Custom patterns can be rotated by any angle and saved as a new pattern.

**Data (RXD).** RXD document `standards/fill_patterns`, kind `fillpattern`: `kind` (drafting|model), `lineFamilies[]` (`angle`, `origin`, `offset`, `shift`, `dashes[]`), `hostOrientation`. Per-face pattern alignment is element-scoped RXD on the element (`face sub-reference → origin, angle`). On IFC export: `IfcFillAreaStyleHatching` per line family (`HatchLineAppearance`, `StartOfNextHatchLine`, `PointOfReferenceHatchLine`, `PatternStart`, `HatchLineAngle`) inside an `IfcFillAreaStyle` (`ModelOrDraughting` = TRUE for model patterns).

**Commands.** `fillpattern.create(name, kind: drafting|model, definition) -> pattern_id`; `fillpattern.import_pat(path, pattern_name, kind, scale?) -> pattern_id`; `fillpattern.rotate_copy(pattern_id, angle, new_name) -> pattern_id`; `fillpattern.delete(pattern_id, replacement_id?) -> ()`; `face.set_pattern_alignment(element_id, face_ref, origin, angle) -> ()`

**Acceptance criteria.**
- [ ] A drafting hatch at 3 mm spacing prints at 3 mm spacing at 1:50 and at 1:200.
- [ ] A 225 × 75 mm brick model pattern shows real-size courses that move with the wall.
- [ ] Importing a `.pat` file with 20 patterns lists all 20 for selection.

### Crop regions and annotation crop
**ID:** DOC-GFX-015 · **Priority:** P0 · **Ref:** research/04 §3.8

**Purpose.** Limit the area a view shows.

**Behaviour & rules.**
- *Crop View* (on/off) and *Crop Region Visible* are view properties and View Control Bar toggles.
- The crop region can be dragged by its edges, edited as a non-rectangular sketch (*Edit Crop*), reset (*Reset Crop*), or sized numerically (*Size Crop*: model crop and annotation crop width/height, proportional option).
- **Crop breaks** (sections and elevations): a break control splits the crop region into two parts that can be moved closer together to shorten a long view; the gap is shown by break lines.
- **Annotation crop:** a second boundary outside the model crop, with an offset per side (paper mm). Annotation that crosses it is clipped; annotation that is entirely outside is hidden. Model-reporting annotation of elements outside the model crop is always hidden.
- Rotating the crop region rotates the view's presentation (for example to set a plan square on the sheet). The plan property *Orientation* (Project North / True North) sets the base orientation (Chapter 6, COL-POS-001).
- Crop changes on a callout or dependent view update the parent's callout boundary or matchline display accordingly.

**Data (RXD).** `crop` block of the view payload: `enabled`, `visible`, `boundary` (polyline in view coordinates), `breaks[]`, `rotation`; `annotationCrop`: `enabled`, `offsets` (four values in paper mm).

**Commands.** `view.set_crop(view_id, enabled?, visible?, shape?) -> ()`; `view.reset_crop(view_id) -> ()`; `view.set_annotation_crop(view_id, enabled, offsets?) -> ()`; `view.add_crop_break(view_id, axis, position, gap) -> ()`; `view.remove_crop_break(view_id, index) -> ()`

**Acceptance criteria.**
- [x] A tag whose host is outside the model crop is hidden even when the tag itself lies inside the annotation crop. *(Built 2026-10-27, above.)*
- [ ] A crop break shortens a 60 m elevation by the gap distance and draws break lines at both cut edges.

*Built (2026-10-27), rectangular crops:* plans (floor, ceiling and area plans), sections and elevations have *Crop View*, *Crop Region Visible* and *Annotation Crop* in Properties › Extents, and *Reset Crop* (fit the model as the view sees it, with a 1 m margin; turning the crop on with no region does the same). The crop region is a rectangle in the view's 2D coordinates (plan x and y; a section's x and height). With the crop on:
- the model is drawn only inside it: cut and projection lines clipped, filled cuts and colour fills clipped triangle by triangle, cut edges (for snapping) clipped, and the shaded model clipped by four clip planes;
- tags, keynotes and spots of elements outside it are hidden wherever they stand;
- with the annotation crop on (offsets per side in paper mm, default 5), annotation lines are clipped to the annotation crop and text that is not wholly inside it is hidden; without it annotation is not clipped;
- a visible region is drawn, and prints;
- printed, exported and sheet drawings are cropped alike (`reviera_render::crop`), so a viewport on a sheet takes the cropped extent.

Data: `crop` `{enabled, visible, min, max}` and `annotationCrop` `{enabled, offsets: [left, right, bottom, top]}` in the view payload. Commands: `view.set_crop(view_id, enabled?, visible?, shape?: {min, max})`, `view.reset_crop(view_id)`, `view.set_annotation_crop(view_id, enabled, offsets?)`, `view.size_crop(view_id, width?, height?, annotation_offsets?)` (*Size Crop*: about the region's centre, in one step; the dialog's proportional option scales the other size with the one changed). While the region is visible, its edges and corners can be dragged in the window (one `view.set_crop` step). Not yet: non-rectangular crops (*Edit Crop*), crop breaks, rotation, and crops of 3D views.

*Built (2026-10-27), scope boxes:* a plan (floor, ceiling or area plan) assigned to a scope box (ARC-SCOPE-001) is cropped to the box's footprint, its bounding rectangle while crops cannot turn, and follows the box when it is moved or resized; its crop is on unless switched off in Properties, and its region cannot be set or reset by hand (`view.crop_follows_scope_box`). A section or elevation assigned to a box is cropped to the box seen from the side: the rectangle round its corners in the view's frame, across the box's width as the view sees it and from its bottom to its top.

- [x] *(Built 2026-10-27, `tests/sample_features.rs`.)* The views sample's *Level 1 - East Half* draws only within its box (4.6 to 11 m), refuses a hand-set region, shows the whole plan with its crop off, and follows the box moved 2 m west.
- [x] *(Built 2026-10-27, `tests/sample_features.rs`.)* The views sample's *Level 1 - Cropped* draws nothing east of its region while *Level 1* runs on; the annotated plan cropped to its living room hides the kitchen's and bedroom's tags (their rooms are outside), and with a 5 mm annotation crop hides the title note and keeps the dimension lines within the annotation crop; turning the crop off brings everything back.

### Discipline display
**ID:** DOC-GFX-016 · **Priority:** P1 · **Ref:** research/04 §1 (Discipline property)

**Purpose.** Tailor a view to a trade.

**Workflow.**
1. Properties › Graphics › Discipline picks the view's discipline; the view regenerates.
2. Manage › Settings › Discipline Profiles opens a table with one column per discipline and one row per model category: *Show*, *Halftone*, *Hidden lines* (drawn when Show Hidden Lines = By Discipline). Reset restores Reviera's defaults.

**Behaviour & rules.**
- Each discipline (Architectural, Structural, Mechanical, Electrical, Plumbing, Coordination) has a profile in project settings: categories hidden, categories halftoned, and whether *Show Hidden Lines = By Discipline* draws hidden lines for its own categories.
- Default profiles: Structural hides non-structural categories and shows structural framing hidden lines; MEP disciplines halftone architectural and structural categories; Coordination shows everything normally.
- The profile acts as part of layer 1 of the graphics model, below the view's explicit category overrides. An explicit *Visible* tick in the view's graphics dialog therefore shows a category the profile hides; the Graphics Inspector reports the profile as the losing layer.
- Annotation categories are never hidden by a profile.
- The discipline also groups the view in the Project Browser under the default organisation scheme, and is a filter field in view lists and view/sheet sets.
- Profiles are project data, so changing one updates every view of that discipline. Templates can include the Discipline property (DOC-GFX-004).
- *Show Hidden Lines by element* (DOC-GFX-020) works only when the view's Show Hidden Lines is By Discipline and the discipline is Architectural, Structural or Coordination, matching the reference product's limits.

**Data (RXD).** Discipline profiles in RXD document `standards/object_styles` (kind `discipline.profile`: `discipline`, `hidden[]`, `halftone[]`, `hiddenLines[]` category keys); view payload `discipline`.

**Commands.** `view.set_discipline(view_id, discipline) -> ()`; `discipline.set_profile(discipline, hidden_categories[], halftone_categories[], hidden_line_categories[]) -> ()`. An unknown category key fails with `category.unknown`.

**Acceptance criteria.**
- [ ] Setting a plan to Mechanical halftones walls and shows ducts normally.
- [ ] Ticking Furniture *Visible* in a Structural plan's graphics dialog shows furniture even though the Structural profile hides it.
- [ ] Removing Floors from the Mechanical profile's halftone list draws floors normally in every Mechanical view at once.

### Phasing display
**ID:** DOC-GFX-017 · **Priority:** P1 · **Ref:** research/04 §3.9

**Purpose.** Show existing, new, demolished and temporary work correctly in each view.

**Workflow.**
1. Manage › Phasing › Phases opens a dialog with tabs **Project Phases** (ordered list; insert before/after; combine with previous/next), **Phase Filters**, **Graphic Overrides**.
2. Each element has *Phase Created* and *Phase Demolished*. Modify › Geometry › Demolish sets Phase Demolished to the active view's phase for clicked elements.
3. Each view has a *Phase* and a *Phase Filter*.

**Behaviour & rules.**
- **Status of an element in a view** with phase `P` (phases ordered by index):

| Condition | Status |
|---|---|
| created after `P` | not shown (always) |
| created before `P`, not demolished or demolished after `P` | Existing |
| created before `P`, demolished in `P` | Demolished |
| created before `P`, demolished before `P` | not shown (always) |
| created in `P`, not demolished in `P` | New |
| created in `P`, demolished in `P` | Temporary |

- **Phase filters** set, for each status, one of *By category* (normal graphics), *Overridden* (use the phase graphic overrides) or *Not displayed*. Built-in filters: Show All, Show Complete, Show Demo + New, Show New, Show Previous + Demo, Show Previous + New, Show Previous Phase, None. Users can add filters.
- **Phase graphic overrides** per status: projection/surface lines and patterns, cut lines and patterns, halftone, and an optional material. Defaults: Existing = halftone; Demolished = `<Demolished>` dashed lines; New and Temporary = by category.
- Phase graphics are layer 4 of DOC-GFX-001.
- Rooms, areas, spaces and schedules are phase-specific (they belong to one phase).
- Linked models map their phases to host phases (Chapter 6, COL-LNK-001).
- The phase definitions themselves are project data owned by Chapter 2; this requirement defines their display.

**Data (RXD).** RXD document `project/phases`: kinds `phase` (ordered), `phase.filter`, `phase.override`. Each element's *Phase Created* and *Phase Demolished* are element-scoped RXD (Chapter 2a) holding phase ids. On IFC export, Reviera writes the standard `Status` property of the element's common property set (`NEW`, `EXISTING`, `DEMOLISH`, `TEMPORARY`) as seen in the phase being exported, and may write phases as `IfcTask` objects ordered by `IfcRelSequence` (export option).

**Commands.**
`phasefilter.create(name, settings{status: by_category|overridden|hidden}) -> filter_id`
`phasefilter.update(filter_id, settings) -> ()`
`phase.set_graphic_override(status, override) -> ()`
`element.demolish(element_ids[], phase_id? (default: the active view's phase), view_id?) -> ()`
`view.set_phase(view_id, phase_id, phase_filter_id?) -> ()`

**Acceptance criteria.**
- [ ] A wall created in Existing and demolished in New Construction shows dashed in the New Construction view with *Show Demo + New*, and is not shown in a later phase view.
- [ ] A temporary element (created and demolished in the same phase) is hidden in views of later phases.
- [ ] Exporting the New Construction phase writes `Status = DEMOLISH` for that wall.

### Design option display
**ID:** DOC-GFX-018 · **Priority:** P1 · **Ref:** research/04 §3.10

**Purpose.** Develop and present alternative designs in one model.

**Workflow.**
1. Manage › Design Options › Design Options dialog: create option sets (for example "Entrance Canopy"), each with options; one option per set is primary. Buttons: New set, New option, Make Primary, Duplicate, Rename, Delete, Accept Primary.
2. *Add to Set*: assign selected elements to one or more options.
3. *Pick to Edit* / the active option drop-down on the status bar: choose the option being edited. New elements go into the active option; elements in other options are greyed and cannot be selected.
4. Per view, in the graphics dialog Design Options tab: for each set, *Automatic* (primary) or a specific option.

**Behaviour & rules.**
- The main model is always shown. For each set, a view shows exactly one option.
- Elements may not join, host or be hosted across options, or between an option and the main model in ways that would make the main model depend on a secondary option; such edits are refused with a message.
- Annotation that references an element in a non-primary option belongs to that option and is shown only when the option is displayed.
- View-owned elements (detail lines, regions) can be edited while an option is active.
- *Accept Primary* moves the primary option's elements into the main model and deletes the set and all other options (with confirmation).
- Schedules show the primary option by default, or a chosen option per set.

**Data (RXD).** RXD document `project/design_options`: kinds `option.set` and `option` (`isPrimary`, `set`). Membership is element-scoped RXD on each element (`designOption` = option id), so moving one element between options touches only that element. On IFC export, either only the main model plus chosen options are written, or (option *Export design options as groups*) each option becomes an `IfcGroup` with its elements assigned by `IfcRelAssignsToGroup`.

**Commands.**
`designoption.create_set(name) -> set_id`
`designoption.create_option(set_id, name) -> option_id`
`designoption.make_primary(option_id) -> ()`
`designoption.add_elements(option_id, element_ids[]) -> ()`
`designoption.remove_elements(element_ids[]) -> ()`
`designoption.set_active(option_id?) -> ()`
`designoption.accept_primary(set_id) -> ()`
`designoption.delete(set_or_option_id) -> ()`
`view.set_design_option(view_id, set_id, option_id?) -> ()`
`designoption.manage() -> ()` opens the *Design Options* dialog: a tree of option sets and their options (the primary option marked "(primary)"), and buttons grouped as *Edit* (Edit Selected, Finish Editing, Cancel Editing), *Option* (New, Make Primary, Duplicate, Delete, Rename), *Option Set* (New, Accept Primary, Delete, Rename). The active option and the number of elements in each option are shown. Validation: option and set names are unique within their parent; the primary option cannot be deleted while other options exist (make another primary first); *Accept Primary* and Delete list what will be deleted and need confirmation; *Make Primary* is refused while an option is being edited. Each button calls the matching command above, so every action is its own undo step.

**Acceptance criteria.**
- [ ] A view set to option B of a set shows option B's elements and hides option A's.
- [ ] *Accept Primary* leaves the primary elements in the main model and deletes the others in one undo step.

### Scope boxes
**ID:** DOC-GFX-019 · **Priority:** P1 · **Ref:** research/04 §3.8 *Scope Boxes*

**Purpose.** One box that controls the crop of many views and the extents of datums.

**Workflow.**
1. View tab › Create › Scope Box: draw a rectangle in a plan; set name and height.
2. Assign: view Properties › Scope Box; datum (level, grid, reference plane) Properties › Scope Box.
3. *View Name Visibility*: choose in which views the box itself is shown.

**Behaviour & rules.**
- A view assigned to a scope box has its crop set to the box's footprint (plans) or the matching face (sections and elevations that intersect it) and cannot edit its crop directly.
- A datum assigned to a scope box has its extents limited to the box in every view.
- Rotating the box rotates the crops of assigned plans.
- Scope boxes print only if not hidden by the print option *Hide scope boxes* (default on).

**Data (RXD).** Annotation-like project object of kind `scopebox` in RXD document `views/index` (name, rectangle, height, rotation, base level, `visibleInViews`). Views and datums refer to it by id; the datum's scope box reference is element-scoped RXD on the grid or level.

**Commands.** `scopebox.create(name, rectangle: {corner_a, corner_b, rotation?}, height, level_id?) -> element_id`; `scopebox.update(scopebox_id, rectangle?, height?, rotation?) -> ()`; `view.set_scope_box(view_ids[], scopebox_id?) -> ()`; `datum.set_scope_box(datum_ids[], scopebox_id?) -> ()`; `scopebox.set_view_visibility(scopebox_id, view_ids[]) -> ()`

**Acceptance criteria.**
- [ ] Resizing a scope box resizes the crops of the four dependent plans assigned to it.
- [ ] A grid assigned to a scope box stops at the box edge in every plan.

---

## 5.12 Drawing generation: hidden-line pipeline and GPU viewport

The reference product's rendering engine is not documented publicly, so this section is Reviera's own design. It is built on OpenCASCADE (OCCT) and a `wgpu` renderer, as set out in TECH_STACK §3.2 (the custom `wgpu` renderer fed by OCCT tessellation was adopted by the owner on 2026-10-12, TECH_STACK §5.2; OCCT's hidden-line algorithms provide the exact 2D output).

### Pipeline overview
**ID:** DOC-HLR-001 · **Priority:** P0 · **Ref:** research/04 §11 *Key behaviours* 1, 3; TECH_STACK §3.2–3.3

**Purpose.** Produce correct, printable 2D drawings (plans, sections, elevations, hidden-line 3D) and responsive interactive views from the same model, using all CPU cores.

**Behaviour & rules.** Generating a view runs these stages. Stages 2–5 run in parallel per element on the rayon pool; each worker owns its OCCT shapes, and no mutable `TopoDS` state is shared between threads.

```
 model change ──► invalidation (element revision, view-definition revision)
        │
 1. Candidate query     spatial index (BVH of element boxes) ∩ view volume; graphics resolver (DOC-GFX-001) drops hidden elements
 2. Representation      choose geometry per detail level / view direction / symbolic linework (family recipe)
 3. Zone split & cut    OCCT half-space booleans at cut, bottom, depth, far-clip and section-box planes;
                        planar sections at the cut plane → cut regions per element and per compound layer
 4. Projection & HLR    interactive: GPU-assisted visibility (DOC-HLR-003)
                        print/export: exact OCCT HLR (DOC-HLR-002)
 5. Classification      each edge/region tagged {element, sub-category, zone, edge kind} → resolved style
 6. 2D clean-up         cut-region union per material and join rules, coincident-line removal, pattern phase
 7. Draw list           ordered display list of styled 2D primitives (DOC-HLR-004)
        │
        ├──► GPU viewport (2D renderer)        ├──► PDF / print        ├──► DXF / DWG        ├──► raster image
```

- Results of stages 2–5 are cached per (element geometry hash, view direction, clipping planes, detail level), so that moving one element only recomputes that element plus the occlusion pass.
- Regeneration is incremental and asynchronous. The viewport shows the previous result until the new one is ready, then swaps it atomically; a progress indicator appears after 250 ms.
- Views that are not open, and are not being printed or exported, are not regenerated (lazy evaluation). Their caches are invalidated only.

**Acceptance criteria.**
- [ ] Moving one door in a 2000-element plan updates the plan in under 200 ms on the reference machine (Chapter 1 performance targets).
- [ ] Regenerating a large plan uses all available cores (CPU utilisation above 80 % on an 8-core machine during the HLR stage).
- [ ] The cached and freshly computed draw lists of the same view are identical.

### Cut and exact hidden-line removal with OCCT
**ID:** DOC-HLR-002 · **Priority:** P0 · **Ref:** research/04 §1.1 (cut plane), §3.6

**Purpose.** Produce exact linework for printing and export.

**Behaviour & rules.**
1. **Cut regions.** For each cuttable element crossing a cut plane (plan cut plane, section plane or plane segments of a jogged section, section box faces), compute the section of the solid with `BRepAlgoAPI_Section`, rebuild closed wires and faces from the section edges, and classify inner loops as holes. For compound elements, cut each layer solid separately so each layer gets its own material pattern. Results are 2D regions in view coordinates.
2. **Clipping.** Clip each solid to the view volume (crop prism between the near plane and far clip, or the zone slabs of DOC-VR-002) with `BRepAlgoAPI_Common` against a box or half-spaces. Unchanged elements that lie entirely inside the volume skip this step.
3. **Hidden-line removal.** Run OCCT's exact `HLRBRep_Algo` with an `HLRAlgo_Projector` set to the view's orthographic (or perspective, for camera views) projection. Feed it all clipped solids in the view, plus the cut regions as occluding planar faces. Extract visible sharp edges, visible smooth edges (only when the visual style shows smooth edges), visible outlines, and (when *Show Hidden Lines* applies) hidden edges with `HLRBRep_HLRToShape`.
4. **Scaling up.** Exact HLR is quadratic in the worst case. Reviera splits large views into screen tiles, runs HLR per tile in parallel with only the solids whose projected boxes overlap the tile (plus all solids in front of them), and stitches edges at tile borders. For very large views it may first run `HLRBRep_PolyAlgo` (mesh-based) to find which solids can occlude which, and then run the exact algorithm only on those.
5. **Arcs and circles.** Edges that project to circles, arcs or lines are recognised and emitted as exact 2D primitives (not polylines), so that DXF and PDF output keep true arcs.
6. Exact HLR is used for print, PDF, DXF/DWG and stored drawing linework. Interactive display may use DOC-HLR-003 while the exact result is computed in the background; once it is ready, the viewport switches to the exact draw list.

**Acceptance criteria.**
- [ ] A section through a cavity wall shows one cut region per layer with each layer's pattern, and the layers' common edges are drawn once.
- [ ] A circular column cut in plan is a true circle in the exported DXF (an arc entity, not a polyline).
- [ ] On screen, the interactive and exact results of the same plan differ by no more than one device pixel along any edge at 100 % zoom.

### GPU-assisted interactive hidden-line display
**ID:** DOC-HLR-003 · **Priority:** P0 · **Ref:** research/04 §1.5 (*Accelerated Graphics* as context)

**Purpose.** Keep 2D views and hidden-line 3D views interactive on large models.

**Behaviour & rules.**
- Elements are tessellated by OCCT (`BRepMesh_IncrementalMesh`) with a chordal deflection based on detail level and on-screen size. Tessellations are cached per element and shared between views.
- Edges come from the B-rep (feature edges), not from the mesh, so that the result is clean linework.
- Visibility is resolved on the GPU: a depth pre-pass renders the triangles (cut by the clip planes and capped, DOC-HLR-006), then edges are drawn with a depth test using a small bias. Edges hidden by the depth test can be drawn again in the `<Hidden>` style when required.
- Cut regions from DOC-HLR-002 step 1 are computed on the CPU (they are cheap) and drawn as filled polygons with the cut pattern.
- When the camera moves, only the GPU passes rerun; no CPU geometry work is done.

**Acceptance criteria.**
- [ ] Panning and zooming a 50 000-element plan stays above 30 fps on the reference GPU.
- [ ] Orbiting a hidden-line 3D view of the same model stays above 20 fps.

### The draw list
**ID:** DOC-HLR-004 · **Priority:** P0 · **Ref:** research/04 §6.1–6.3

**Purpose.** One intermediate 2D representation shared by the screen and every output (DOC-CORE-006).

**Behaviour & rules.**
- A draw list is an ordered set of layers (back to front): background fills and underlay; model surface patterns; model projection linework; cut poché fills; cut linework; overhead and beyond linework; view-owned drafting (ordered by its draw order, DOC-ANN-020); annotation; crop frame.
- Primitives: line, polyline, arc, circle, ellipse, spline (with a flattened copy), filled polygon (with holes) with a fill pattern reference, text run (font, size in paper mm, position, rotation, width factor, decorations), raster image (with clip polygon), and point symbol (a reference to a cached symbol draw list with a transform).
- Each primitive carries a **style key**: resolved colour, pen width in paper mm, line pattern id and phase offset, halftone flag, transparency, and its source (element GlobalId, sub-category, zone, edge kind). The source lets the screen highlight and select, lets DXF choose layers (DOC-OUT-005), and lets PDF write tagged content.
- Line patterns are applied in the consumers in paper space. The draw list keeps the pattern reference and the *pattern phase* (distance along the polyline), so that dashes continue across segment boundaries in every output.
- **Coincident lines:** when two collinear overlapping segments with the same zone exist, the one with the heavier pen (or, on a tie, the higher-priority category per the object style table) is kept for the overlap, and the other is trimmed. The print option *Region edges mask coincident lines* applies the same rule between filled-region boundaries and model lines.
- Draw lists are serialisable (for caching and for Lua and MCP inspection) but are never stored as RXD.

**Commands.** `view.get_draw_list(view_id, region?) -> draw_list` (query, also exposed as the MCP resource `reviera://view/{id}/drawlist`).

**Acceptance criteria.**
- [ ] The PDF, PNG and DXF outputs of one view contain the same set of edges (compared after rasterising each at 300 dpi; at most 0.1 % differing pixels).
- [ ] A dashed polyline with many short segments shows a continuous dash rhythm in all outputs.

### 3D GPU viewport
**ID:** DOC-HLR-005 · **Priority:** P0 (wireframe, hidden line, shaded, consistent colours), P1 (textures, realistic) · **Ref:** research/04 §3.7

**Purpose.** Render 3D views in every visual style (DOC-GFX-011) at interactive rates.

**Behaviour & rules.**
- The renderer uses `wgpu` (Vulkan, Metal, Direct3D 12, or OpenGL ES fallback), drawing into the native child window that the wxWidgets view frame provides (TECH_STACK §3.4).
- It is the built-in **render backend** (DOC-RBE-001): it consumes the same Render Scene and deltas as any plug-in backend, and is always present for editing, selection and 2D views.
- It draws per-element instanced meshes from the OCCT tessellation cache. Family instances that share a type share their mesh. Large models use frustum culling, a BVH, and occlusion culling from the previous frame's depth.
- Per-element resolved graphics (colour, transparency, halftone, visibility) live in a GPU buffer updated from the graphics resolver; a filter or override change updates the buffer without re-tessellating.
- Visual-style features: edges from the B-rep edge cache; silhouettes from a depth/normal edge-detection pass; shadows by cascaded shadow maps; ambient shadows by screen-space ambient occlusion; sketchy lines and depth cueing in shaders using the same deterministic noise as print (DOC-GFX-012); transparency by weighted blended order-independent transparency; physically based shading (metallic/roughness) for Realistic, with materials taken from `IfcSurfaceStyleRendering` and the material's RXD appearance data (Chapter 2).
- Selection uses an ID buffer (one render target holding element indices).
- **Large-coordinate safety:** geometry is stored relative to per-tile origins and rendered relative to the camera (relative-to-eye) so that sites far from the internal origin (Chapter 6, COL-POS-001) do not jitter.

**Acceptance criteria.**
- [ ] A 500 000-triangle model orbits at 60 fps in Shaded style on the reference GPU.
- [ ] Changing a filter colour updates the 3D view within one frame and causes no re-tessellation (checked by counters).
- [ ] A model placed 30 km from the internal origin renders without visible vertex jitter at 1 mm zoom.

### Section box and clip-plane capping
**ID:** DOC-HLR-006 · **Priority:** P0 · **Ref:** research/04 §1.5 *Section Box*

**Purpose.** Show solid, patterned cut faces where section boxes and clip planes cut through elements.

**Behaviour & rules.**
- Interactive: clip planes in the vertex/fragment stage, and caps drawn with a stencil-parity technique per element, filled with the element's resolved cut pattern colour (a solid colour while dragging; the hatch pattern once dragging stops).
- Compound elements are capped per layer, using each layer's material cut pattern; the layer boundaries come from the layer solids, not from the mesh, so adjacent layers share one edge.
- Stencil parity needs closed (watertight) meshes. Elements whose tessellation is not closed (for example some imported Reference View geometry) are drawn clipped but uncapped, and the Graphics Inspector reports "not capped: open mesh". The exact print path caps them when OCCT can build a closed section, and otherwise leaves them open in print too.
- Caps take the cut line and cut pattern of DOC-GFX-001 (so filters and overrides apply), and are drawn only when the 3D view's *Show cut faces* property is on (DOC-VIEW-008).
- Caps are generated for host-model elements and for linked-model elements alike; transparent elements get transparent caps with the same alpha.
- Up to six section-box planes, eight slice planes (DOC-VIEW-014) and one far-clip plane are supported per view; a single element cut by several planes gets one cap per plane, trimmed by the other planes.
- *Interim implementation:* until stencil-parity capping lands, the viewport caps closed meshes by drawing the inside of the solid (its back faces, seen through the cut) flat in the cap colour. This looks the same as a cap for elements that do not overlap; meshes whose winding cannot be made consistent are drawn uncapped.

**Acceptance criteria.**
- [ ] Cutting a wall with a section box shows a closed cap with the wall's cut pattern, with no gaps at layer boundaries.
- [ ] A filter that sets red cut patterns on exterior walls colours their caps red in the 3D view and in its PDF.
- [ ] A box corner that cuts one column with two planes shows two caps that meet along one edge without overlap.

### Raster fallback for printing
**ID:** DOC-HLR-007 · **Priority:** P1 · **Ref:** research/04 §6.1–6.2 (*Vector / Raster processing*)

**Purpose.** Print views whose content cannot be expressed as vectors (shaded or realistic styles, transparency, shadows, images).

**Behaviour & rules.**
- A view is printed as raster when the user chooses *Raster processing*, or automatically when its visual style or content requires it (the output dialog says so).
- Automatic raster is triggered by: visual styles Shaded, Consistent Colours, Textures, Realistic and Path Traced; surface transparency below 100 % opacity; cast or ambient shadows; sketchy lines with raster-only effects; images and point clouds; rendering views. Hidden Line and Wireframe views without these features stay vector.
- The raster is rendered off-screen by the GPU renderer at the chosen raster quality (Low 72 dpi, Medium 150 dpi, High 300 dpi, Presentation 600 dpi), in tiles if the image exceeds GPU limits.
- Tiles overlap by 16 pixels and are blended at the seams, so no tile border is visible. A GPU without enough memory for one tile at the chosen quality falls back to the next lower quality, and the output report says so.
- Only the content inside each viewport's crop is rasterised; the raster is clipped to the crop polygon and placed at exact paper size.
- Annotation and view-owned drafting on top of a raster view stay vector in PDF output.
- Colour mode (DOC-OUT-001) is applied to the raster as well: *Black lines* turns non-white pixels of edges black but keeps shaded surfaces in greyscale; *Greyscale* converts the whole image.
- Raster output is deterministic for a given model snapshot, quality and renderer version, so repeated exports compare equal.

**Acceptance criteria.**
- [ ] A shaded 3D view on a sheet prints as a 300 dpi image, while its title and the title block remain vector (selectable text in the PDF).
- [ ] An A0 sheet with a shaded viewport at Presentation quality prints without visible tile seams.
- [ ] Exporting the same shaded sheet twice gives identical raster data.

---

### Pluggable render backends
**ID:** DOC-RBE-001 · **Priority:** P0 (interface, with the built-in viewport as its first implementation), P2 (third-party backends) · **Ref:** owner directive 2026-10-12 (pluggable renderer for future photo-real and game-engine walkthroughs); TECH_STACK §3.2

**Purpose.** Keep 3D presentation open to other renderers. A realistic offline renderer, a game engine or Blender can show a Reviera model without changes to the core. The built-in `wgpu` viewport (DOC-HLR-005) is the first backend behind the same interface, not a special case.

**Architecture.**
```
 model transactions ──► graphics resolver (DOC-GFX-001) ──► Render Scene (render-agnostic)
                                                               │  snapshot + deltas
            ┌──────────────────────────┬───────────────────────┼──────────────────────────────┐
            ▼                          ▼                       ▼                              ▼
   built-in wgpu viewport     in-process plug-in       out-of-process bridge          offline renderer
   (always present; editing,  (Rust/C ABI; GPL-       (Reviera Scene Protocol,       (stills, animations;
    picking, 2D views)         compatible only)        ADV-RSP-001; any engine)       DOC-OUT-011)
```
- **What is pluggable, and what is not.** Only the 3D *presentation* of a scene is pluggable. The 2D drawing pipeline (cut, exact HLR, draw lists, DOC-HLR-001…004), printing and export stay in the core, so drawings are identical whichever backend is installed. Editing, selection, in-canvas controls and annotation always use the built-in viewport.
- **Render Scene.** The core keeps one render-agnostic scene per open model, built from the graphics resolver and the OCCT tessellation cache:
  - nodes: one per visible element occurrence, with element GlobalId, category, sub-category, phase/design-option state and a world transform (relative to a tile origin, DOC-HLR-005)
  - meshes: shared per type and detail level (instancing), with normals, UVs, and optional feature edges as line sets
  - materials: the physically based description from Reviera appearance assets (base colour, metallic, roughness, transmission, IOR, emission, normal/bump/height maps, decals)
  - lights: lighting-fixture light sources with IES LM-63 photometry and light groups, sun from the project location and time, sky model, environment image
  - cameras: view cameras, section boxes and clip planes as metadata (a backend may ignore what it cannot show, see capabilities)
  - per-element resolved graphics: visibility, colour override, transparency, halftone
- **Deltas.** Each committed transaction produces a scene delta (nodes, meshes or materials added, changed or removed; graphics changes; light and sun changes). Deltas are coalesced per frame for the built-in viewport and per sync interval (default 250 ms) for external backends.
- **Backend interface.** A backend implements: `describe() -> capabilities`, `open(scene_snapshot, target)`, `apply(delta)`, `set_camera(camera)`, `render(request) -> frames/tiles` (offline kinds), `pick(x, y) -> GlobalId?` (optional), and `close()`. A failing backend is unloaded, its views fall back to the built-in viewport, and the failure is reported. It never takes the model down with it.
- **Capabilities** declared by each backend: kinds (`interactive`, `walkthrough`, `offline_still`, `offline_animation`, `vr`); clip planes and section boxes; feature-edge display; Reviera visual styles supported (DOC-GFX-011); IES lights; decals; picking; maximum scene size; colour pipeline (sRGB, HDR, ACES). The UI only offers a backend for tasks its capabilities cover. For example, a backend without clip planes is not offered for a view with an active section box, unless the user accepts "ignore section box".
- **Selecting a backend.** Each 3D view has a *Presentation backend* property (default: built-in). Render (DOC-OUT-011) and walkthrough sessions (ADV-WALK-001) choose from the backends offering that kind. User preferences hold the default backend per kind.
- **Discovery.** Backends are registered by add-in manifests (DM-ADDIN-001) with a `render_backend` entry (id, name, kinds, version, licence SPDX, entry point: library path or executable plus protocol version). `render.backend.list()` reports installed backends with their capabilities and health.

**Commands.** `render.backend.list() -> backend_info[]` (query); `render.backend.set_default(kind, backend_id) -> ()`; `view.set_presentation_backend(view_id, backend_id?) -> ()` (none restores the built-in viewport); `render.backend.test(backend_id) -> report` (loads it against a sample scene and reports capabilities, timing and errors).

**Acceptance criteria.**
- [ ] The built-in viewport passes the same backend conformance suite as third-party backends (snapshot, 10 000 deltas, camera moves, picking).
- [ ] Unloading or crashing a third-party backend returns its views to the built-in viewport within 2 s, with no model change and no lost undo history.
- [ ] Printed and exported drawings are byte-identical whichever presentation backend the 3D views use.
- [ ] A backend that lacks clip planes is not offered for a view with an active section box, unless the user explicitly accepts "ignore section box".

### Backend hosting and licensing
**ID:** DOC-RBE-002 · **Priority:** P2 · **Ref:** owner directive 2026-10-12; TECH_STACK §4.2

**Purpose.** Allow backends under different licences while keeping the GPL boundary clean.

**Behaviour & rules.**
- **In-process plug-ins** (dynamic library with a versioned C ABI wrapped by a Rust trait) run inside Reviera and are add-ins. They must carry a GPL-compatible SPDX licence (TECH_STACK §4.2, DM-ADDIN-003). Suitable for renderers such as Cycles (Apache-2.0), LuxCoreRender (Apache-2.0) or Mitsuba 3 (BSD-3-Clause).
- **Out-of-process bridges** are separate programs that talk to Reviera only through the documented Reviera Scene Protocol (ADV-RSP-001), in the same way as MCP clients. Reviera ships no code of theirs and loads none. This is how game engines and other applications attach, for example a Blender add-on (Blender is GPL), a Godot (MIT) or Bevy (MIT/Apache-2.0) walkthrough application, or a user-built bridge to an engine under a proprietary licence that the user installs themselves. Whether a particular bridge stays at arm's length is for its authors; Reviera's obligation is to keep the protocol public and the boundary process-based. *(Legal review recommended before any official bridge to a proprietary engine.)*
- **Isolation.** An external bridge receives a *read-only* scene. It can send back only: camera and walk-pose updates, pick results (GlobalIds), viewpoint captures (for BCF, COL-BCF-001) and render results (images for the render gallery). It can never send model edits. A bridge that also wants to edit must use the MCP server with its own token and permission class (DM-MCP-002), subject to the usual approvals.
- **Connection security.** Bridges connect to a local endpoint (Unix domain socket or named pipe; loopback TCP only if enabled). A per-session token is handed over at launch or shown in the *Connect backend* dialog. Remote connections are off by default. When enabled they require TLS and a token, and the scene stream can be limited to the active view's visible elements.
- **Resource limits.** Per bridge: maximum scene upload rate, maximum texture resolution sent (default 4096 px), and an option to send simplified meshes (a coarser detail level) for large models.

**Commands.** `render.bridge.launch(backend_id, view_id?, args?) -> session_id` (starts the configured executable with a session token); `render.bridge.connect_info() -> {endpoint, token}` (for bridges started by hand; the token is shown in the UI and is **never** exposed through MCP resources); `render.bridge.disconnect(session_id) -> ()`; `render.bridge.list_sessions() -> session_info[]` (query).

**Acceptance criteria.**
- [ ] An in-process plug-in whose manifest licence is not on the GPL-compatible allow-list is refused at load time, with the reason shown.
- [ ] An external bridge that sends a model-edit message is disconnected and the attempt is logged. The model is unchanged.
- [ ] With remote connections disabled (the default), connecting from another host fails.

---

## 5.13 Annotation

All annotation tools are on the **Annotate** tab, in the panels Dimension, Detail, Text, Tag, Color Fill and Symbol. Reviera has no Options Bar: tool options appear in the tool's contextual ribbon tab and in the Properties palette (Chapter 1). Every annotation object is stored in the RXD annotation document of its owning view (`views/annotation/<viewId>`, kinds `anno.*`); references to model geometry use GlobalIds plus named sub-references (§5.1).

### Common annotation rules
**ID:** DOC-ANN-000 · **Priority:** P0 · **Ref:** research/04 §4 intro, §11 *Key behaviours* 3–4

**Behaviour & rules.**
- **Annotation types** (text types, dimension types, tag families, arrowhead styles) are project standards stored in RXD document `standards/text_styles`. Changing a type changes every instance of it.
- **Fonts.** Text renders with fonts found through the system font configuration. Reviera bundles a default family of open-licence fonts (SIL Open Font License) so that output is the same on every platform. PDF output embeds font subsets. A font missing on a machine is substituted, and the substitution is reported.
- **Orphans.** When a reference can no longer be resolved, model-reporting annotation keeps its last position, is drawn in the *orphaned* colour (default orange; never printed differently unless the print option *Print orphans in warning colour* is on), and is listed in the warnings list (Chapter 6, COL-MNT-004).
- **Linked model references.** References may point into linked models (`link:<linkId>/ifc:<GlobalId>`, Chapter 6). When the link is unloaded, such annotation is hidden, not deleted; it is orphaned only if the element is missing after a reload.
- **Read convention.** Text in annotation stays readable: rotated text flips so that it reads from the bottom or the right of the sheet, unless the type says otherwise.
- **Annotation in 3D.** Allowed only in locked 3D views (DOC-VIEW-006); the annotation plane is the view's projection plane.

**Acceptance criteria.**
- [ ] Opening a project on a machine without the chosen font prints with the substitute font and logs one warning per missing font.
- [ ] Unloading a link hides dimensions to its elements; reloading shows them again unchanged.

### Aligned dimensions
**ID:** DOC-ANN-001 · **Priority:** P0 · **Ref:** research/04 §4.1

**Purpose.** Dimension strings parallel to the measured references.

**Workflow.**
1. Annotate › Dimension › Aligned (default shortcut DI; also on the Quick Access Toolbar).
2. In the contextual tab, choose *Prefer* (wall centrelines, wall faces, centre of core, faces of core) and *Pick* mode (individual references, or entire walls with options for opening centres, opening widths, intersecting walls and intersecting grids).
3. Click references one after another (Tab cycles candidates under the cursor), then click in empty space to place the dimension line.

**Behaviour & rules.**
- References can be faces, edges, centrelines, reference planes, grids, levels (in sections), family reference lines, detail lines and snap points, in the host model or linked models.
- A dimension with *n* references has *n − 1* segments.
- Each reference is stored as a stable sub-reference; after regeneration, the dimension measures the new geometry.
- Temporary dimensions shown during editing (Chapter 1) can be made permanent with their dimension control; the result is an aligned dimension.

**Data (RXD).** Kind `anno.dimension` with `mode = "aligned"`: `style`, `refs[]` (GlobalId sub-references), `line` (origin and direction in view coordinates), `stringType`, `lockedSegments`, `equality`, `textOverrides[]`.

**Commands.** `dim.create_aligned(view_id, refs[], line_point, type_id?, prefer?, pick_mode?) -> dimension_id`; `dim.make_permanent(view_id, temporary_dimension_ref | {ref_a, ref_b}) -> dimension_id`

**Acceptance criteria.**
- [ ] An aligned dimension between two wall faces updates its value when one wall moves.
- [ ] Picking an entire wall with *opening widths* creates a string that includes every door and window opening.

*Built (2026-10-26), in sections and elevations:* aligned dimensions go in sections and elevations too, measured in the view's frame (x to the viewer's right, height up). The Dimension tool there offers, nearest first: levels (`level`, across the view at their elevation); Reviera walls' bases and tops (`base`, `top`), their faces and axes where the view plane cuts them or, seen edge on, where they stand (`face:ext`, `face:int`, `axis`), and their ends (`end:start`, `end:end`); grids crossing the view (`axis`); else the drawing's cut and projected edges as fixed lines. Wall and level references follow the model as in plans. Not yet: floors', roofs' and slabs' tops and bottoms as references (their edges are fixed lines), and references in linked models.

### Linear dimensions
**ID:** DOC-ANN-002 · **Priority:** P0 · **Ref:** research/04 §4.1

**Purpose.** Horizontal or vertical dimensions between points, whatever the direction of the references.

**Workflow.** Annotate › Dimension › Linear. Click two or more points (end points, intersections, arc centres, family reference points; Tab cycles candidates), then move the cursor to choose the axis and click to place the dimension line. Spacebar toggles between horizontal and vertical while placing.

**Behaviour & rules.**
- The dimension line is horizontal or vertical in the view (the user picks by cursor position or by pressing the spacebar). Measured values are projected onto that axis. References are points (end points, intersections, centres).
- "Horizontal" and "vertical" are relative to the view's presentation, so rotating the crop region (DOC-GFX-015) rotates the axis with it.
- References may be in the host model, in linked models (DOC-ANN-000) or on view-owned drafting. A point that becomes coincident with another along the axis gives a zero-length segment, which is drawn with its witness lines and the value 0 unless the type suppresses zero segments.
- Linear dimensions use the same string types, equality, locks and text overrides as aligned dimensions (DOC-ANN-004, DOC-ANN-005).
- Available in plans, ceiling plans, sections, elevations, callouts, drafting views and locked 3D views (where the axis is in the view plane).

**Data (RXD).** `anno.dimension` with `mode = "linear"` and `axis` (horizontal | vertical, in view coordinates), plus the common dimension fields of DOC-ANN-001.

**Commands.** `dim.create_linear(view_id, point_refs[], line_point, axis?: horizontal|vertical, type_id?) -> dimension_id` (fewer than two references fails with `dim.too_few_references`)

**Acceptance criteria.**
- [ ] A linear dimension between the ends of a 45° wall shows its horizontal projection.
- [ ] After rotating a plan's crop by 30°, a new vertical linear dimension is vertical on the sheet.
- [ ] Moving one referenced column updates the linear dimension's value on the next regeneration.

### Angular, radial, diameter and arc-length dimensions
**ID:** DOC-ANN-003 · **Priority:** P1 · **Ref:** research/04 §4.1

**Purpose.** Dimension angles between lines and the size and length of curved geometry.

**Workflow.** Annotate › Dimension › Angular / Radial / Diameter / Arc Length. Pick the references as described below (Tab cycles candidates), then click to place the dimension arc or text.

**Behaviour & rules.**
- **Angular:** between two non-parallel linear references; the user picks the quadrant by cursor position. More than two references that meet at one point give a chain of angles (picked in order). Parallel references are refused with `dim.parallel_references`.
- **Radial:** radius of an arc or circle, with an optional centre mark; the leader goes from the centre or from the arc only, by type. The value text carries the type's radius prefix (default "R").
- **Diameter:** diameter of an arc or circle, with the diameter prefix symbol from the type.
- **Arc length:** pick an arc, then the two radial references (end points or lines) that bound it. The value is the length along the arc between them; the type's arc-length symbol is shown above the value.
- References can be curved walls, arcs in families, circular columns, detail arcs and, for angular dimensions, grids, reference planes and straight edges.
- When the arc becomes a straight line (radius changed to infinite or the wall is made straight), radial, diameter and arc-length dimensions are orphaned (DOC-CORE-003).
- Angular values use the project's angle format (decimal degrees or degrees–minutes–seconds) unless the type overrides it. Radial and diameter dimensions can drive the geometry (DOC-ANN-004) when the referenced element allows its radius to be edited.

**Data (RXD).** `anno.dimension` with `mode` = `angular` | `radial` | `diameter` | `arclength`; angular adds `quadrant`; radial adds `centreMark` and `leaderFrom` (centre | arc); arc length adds `endRefs[2]`.

**Commands.** `dim.create_angular(view_id, ref_a, ref_b, arc_point, type_id?) -> dimension_id`; `dim.create_radial(view_id, arc_ref, point, type_id?, centre_mark?) -> dimension_id`; `dim.create_diameter(view_id, arc_ref, point, type_id?) -> dimension_id`; `dim.create_arc_length(view_id, arc_ref, end_ref_a, end_ref_b, point, type_id?) -> dimension_id`

**Acceptance criteria.**
- [ ] A radial dimension on a curved wall updates when the wall's radius changes.
- [ ] An angular dimension between two grids at 60° shows 60.00° and changes when one grid is rotated.
- [ ] Making a curved wall straight orphans its radial dimension and lists it in the warnings.

### Dimension string types, equality and locks
**ID:** DOC-ANN-004 · **Priority:** P0 (continuous, locks), P1 (baseline, ordinate, equality) · **Ref:** research/04 §4.1

**Behaviour & rules.**
- **String type** (from the dimension type, switchable before placement): *Continuous* (chained segments), *Baseline* (each value measured from the first reference, stacked at the type's baseline spacing), *Ordinate* (a single line with running values from an origin, written at each reference).
- **Equality:** the EQ control on a multi-segment string constrains all segments to be equal. Moving one reference redistributes the others. *Equality display*: Value, Equality Text (for example "EQ"), or Equality Formula (a type-defined formula such as "3 × 1200 = 3600").
- **Locks:** the padlock control on a segment creates a dimensional constraint (Chapter 2/3 constraint engine) that holds that distance. The lock is mirrored into RXD document `model/constraints` so that regeneration does not depend on annotation being loaded.
- **Driving dimensions:** editing a segment's value moves the referenced element that is not locked (the element picked last, unless the user chooses otherwise with the move-element control).
- Removing a dimension with locks asks whether to keep or remove its constraints.

**Data (RXD).** `anno.dimension` fields `stringType`, `equality`, `equalityDisplay`, `lockedSegments`; constraints in `model/constraints` (`constraint.lock`, `constraint.equal`).

**Commands.** `dim.set_equality(dimension_id, on, display?) -> ()`; `dim.lock_segment(dimension_id, segment_index, locked) -> ()`; `dim.set_value(dimension_id, segment_index, value, move_ref?) -> ()`; `dim.set_string_type(dimension_id, type: continuous|baseline|ordinate) -> ()`

**Acceptance criteria.**
- [ ] With EQ on a three-segment string between two fixed walls, moving the middle partition keeps the segments equal.
- [ ] A locked segment cannot be changed by moving one of its walls; the move is refused with a constraint message.
- [ ] Typing a new value into a segment moves the unlocked element by the difference.

### Dimension types, text overrides and witness lines
**ID:** DOC-ANN-005 · **Priority:** P0 · **Ref:** research/04 §4.1 *Dimension type properties*, *Instance*, *Behaviours*

**Behaviour & rules.**
- **Type properties:** string type; leader type (arc or line) and leader tick; tick mark and interior tick mark; line and tick pens; dimension line extension (and flipped extension); witness line control (gap to element, or fixed to the dimension line), length, gap and extension; centreline symbol, pattern and tick; ordinate settings; colour; snap distance; text size, offset, font, background (opaque or transparent), bold/italic/underline, width factor; read convention; units format and alternate units (with format, prefix and suffix); show opening height; suppress spaces; equality text, formula and witness display.
- **Instance text:** the value (read-only unless the dimension drives); *Replace with text* (only allowed for non-numeric text, so that a dimension cannot silently show a false number); prefix, suffix, text above and text below; segment leader visibility; label (bind to a global parameter, Chapter 2).
- **Witness lines:** drag a witness grip to another reference; *Edit Witness Lines* adds or removes references; the gap grip shortens a witness line.
- Moving text off its segment draws a leader when the type asks for one.

**Data (RXD).** `standards/text_styles` kind `dimstyle`; instance fields `textOverrides[]` per segment (`replace`, `prefix`, `suffix`, `above`, `below`), `witnessGaps[]`, `textPositions[]`, `label`.

- **Dimension type editor** (`dimtype.edit`; Annotate › Dimension ▾ › Linear / Angular / Radial / Diameter / Arc Length / Spot Elevation / Spot Coordinate / Spot Slope Dimension Types). It opens the Type Properties dialog filtered to the types of that dimension kind: type list with Duplicate, Rename and Delete (a type in use asks for a replacement), the grouped property grid listed above, and a live preview of a sample string. Validation: text size 0.5–100 mm, width factor 0.1–10, line extensions and witness gaps 0–50 mm (paper), units format must be valid for the dimension kind (length for linear kinds, angle for angular, slope for spot slope). OK applies all edits through `dimtype.update` as one undo step.

**Commands.** `dimtype.create(name, base_type_id?, properties) -> type_id`; `dimtype.update(type_id, properties) -> ()`; `dimtype.edit(kind: linear|angular|radial|diameter|arc_length|spot_elevation|spot_coordinate|spot_slope) -> ()` (opens the editor above); `dim.set_text(dimension_id, segment_index?, replace?, prefix?, suffix?, above?, below?) -> ()`; `dim.edit_witness_lines(dimension_id, add_refs[], remove_refs[]) -> ()`; `dim.set_label(dimension_id, global_parameter_id?) -> ()`

**Acceptance criteria.**
- [ ] *Replace with text* refuses a numeric string such as "3000" and accepts "VARIES".
- [ ] Alternate units show "3000 [9'-10\"]" style output when the type enables them.

### Spot elevations
**ID:** DOC-ANN-006 · **Priority:** P1 · **Ref:** research/04 §4.1 *Spot Elevation*; web: Spot Dimensions, Spot Elevation Type Properties

**Purpose.** Show the height of points on faces and edges in plans, sections, elevations and locked 3D views.

**Behaviour & rules.**
- Options: leader on/off, shoulder; *Relative base*: project base point, survey point, or a chosen level (relative elevations); *Display*: actual (picked point), top, bottom, top and bottom (for sloped or thick elements); prefix and suffix; single or top/bottom indicator; symbol from the type.
- On a sloped surface the value follows the picked point and updates when the surface changes.
- Elevations relative to the survey point use the shared coordinate system (Chapter 6, COL-POS-001).
- **Spot types.** Each spot elevation type sets: *Rotate with component*; leader arrowhead, leader and arrowhead pens; colour; symbol (an annotation family, or none); text font, size, width factor, bold, italic, underline, background (opaque or transparent); text offset from leader and from symbol; text orientation (horizontal or vertical); text location (above, below, or in line with the leader; in-line hides the symbol); units format and alternate units (none, right or below, with format, prefix and suffix); *Elevation origin* (project base point, survey point or relative); elevation indicator, top indicator and bottom indicator texts, each as prefix or suffix.
- **Spot dimension settings** (`spot.settings`; Manage › Additional Settings › Spot Dimensions) opens one dialog with three tabs, *Spot Elevation Types*, *Spot Coordinate Types* and *Spot Slope Types*. Each tab lists the types of that kind (Duplicate, Rename, Delete) and edits their properties as above, with the same validation and preview as the dimension type editor (DOC-ANN-005). The dialog also holds two project-wide settings: the default elevation origin for new spot types, and whether spot values in locked 3D views measure from the project base point or the survey point.
- Spot elevations can be placed on faces and edges of model elements, on toposolids, and on linked elements; not on drafting lines or in drafting views.

**Data (RXD).** `anno.spot` with `spotKind = "elevation"`, `ref` (face or edge sub-reference plus parametric location on it), `base`, `display`, `leader`, `shoulder`, `prefix`, `suffix`. Spot types: `standards/text_styles`, kind `dimstyle` with `dimKind` = `spot_elevation` | `spot_coordinate` | `spot_slope`. Project-wide spot settings: `project/settings` › `spot`.

**Commands.** `spot.create_elevation(view_id, ref, point, leader_point?, type_id?, base?, display?) -> annotation_id`; `spot.settings() -> ()` (opens the Spot Dimensions dialog; type edits are applied through `dimtype.update`, and the dialog's changes, including the two project-wide settings, form one undo step)

**Acceptance criteria.**
- [ ] A spot elevation relative to the survey point on a slab top shows the shared-coordinate elevation, and changes when the survey point is redefined.
- [ ] A spot type with *Display = Top and Bottom* on a 250 mm slab shows two values 250 mm apart, each with its indicator text.
- [ ] Changing the text location of a spot type to *In line with leader* hides the symbol on every spot of that type.

*Built (2026-10-27):* Annotate › Dimension › *Spot Elevation* (EL) in plans, ceiling plans, sections and elevations: click a point on an element, then where the text goes (the leader's shoulder, which runs away from the point). A spot keeps the element, the point (model coordinates) and the direction it was picked along (down in plans, the view direction in sections and elevations); each time it is drawn the application finds the point again on the element's surface along that direction (`reviera_app::spots`), so on a moved or reshaped surface the value follows; *Top* and *Bottom* are the first and last surface of the element on the vertical there. In a plan the pick reads the top of what is there (a slab's top, the top of a wall the plan cuts). Values are signed in project length units (`+3000`, `−450`, `±0`), relative to the project base point (default), the survey point (shared height) or a level; *Top and Bottom* shows `T` and `B` lines; prefix and suffix. A spot whose element is gone, or no longer under its point, keeps its place and is drawn orphaned (orange, DOC-ANN-000). Drawn with the view and in PDF output, picked and highlighted like tags, with its value, base, display, prefix and suffix in Properties, and floating in 3D views as class *Spot elevations and coordinates* (DOC-ANN-025). Commands: `spot.create_elevation(view_id, ref?, point, leader_point?, direction?, base?: project|survey|{level}, display?, prefix?, suffix?)`, `spot.set(annotation_ids, base?, display?, include_elevation?, prefix?, suffix?, leader_point?)`, `spot.list(view_id) -> [{ id, data, picked, point, bottom?, lines, orphaned }]`. Not yet: spot types (`dimtype` kinds, symbols, text styles and indicator texts other than `T`/`B`), the Spot Dimensions dialog, spots in locked 3D views, on linked elements and toposolid points, and dragging a spot's text in the window (`spot.set` moves it).

- [x] *(Built 2026-10-27, `tests/sample_features.rs`.)* The annotated plan sample's spot on the north wall reads `+3000`, then `+3500` after the wall is raised, and `+48500` relative to a survey point at 45 m shared height; its elevation's *Top and Bottom* spot reads `T +3000` / `B ±0`, found on the wall's outer face; moving the wall away orphans the plan spot.

### Spot coordinates
**ID:** DOC-ANN-007 · **Priority:** P1 · **Ref:** research/04 §4.1 *Spot Coordinate*

**Purpose.** Report the plan coordinates of points (setting-out of grids, slab corners, site boundaries).

**Workflow.** Annotate › Dimension › Spot Coordinate; click a point on an element (or a grid intersection), then the leader shoulder and text position.

**Behaviour & rules.**
- Shows north/south and east/west (and optionally elevation) of a point relative to the project base point or the survey point, with indicator texts and order from the type.
- Type options: top and bottom value (north/south first or east/west first), indicator texts (default "N", "E", "EL"), indicator as prefix or suffix, whether elevation is included, units format, alternate units, and the text and leader settings shared with spot elevations (DOC-ANN-006).
- Coordinates follow the shared coordinate system and true north (Chapter 6, COL-POS-001); rotating the survey point or redefining shared coordinates updates every spot coordinate.
- Placeable in plans, ceiling plans, sections, elevations and locked 3D views; references are vertices, edge points or face points of model elements, grid intersections and toposolid points.
- A value whose reference disappears is orphaned (DOC-CORE-003).

**Data (RXD).** `anno.spot` with `spotKind = "coordinate"`, `ref`, `base` (project | survey), `includeElevation`, `leader`, `shoulder`.

**Commands.** `spot.create_coordinate(view_id, ref, point, leader_point?, type_id?, base?) -> annotation_id`

**Acceptance criteria.**
- [ ] A spot coordinate on a grid intersection matches the value reported by `coord.report_point` (Chapter 6).
- [ ] Rotating the project to true north by 10° changes the spot coordinate values without moving the spot in the view.

*Built (2026-10-27):* Annotate › Dimension › *Spot Coordinate* in plans, sections and elevations, placed and kept like spot elevations (DOC-ANN-006): `N` and `E` lines from the project base point along project north (default), or shared coordinates from the survey point turned with true north (`base: survey`); `include_elevation` adds an `EL` line. `spot.create_coordinate(view_id, ref?, point, leader_point?, direction?, base?, include_elevation?, prefix?, suffix?)`. Not yet: spot coordinate types (order and indicator texts), grid-intersection and toposolid references, alternate units.

- [x] *(Built 2026-10-27, `tests/sample_features.rs`.)* The annotated plan sample's spot coordinate on the north-east corner reads `N 6000` / `E 10000`.

### Spot slopes
**ID:** DOC-ANN-008 · **Priority:** P1 · **Ref:** research/04 §4.1 *Spot Slope*

**Purpose.** Report the slope of roofs, ramps, beams, pipes and paved surfaces.

**Workflow.** Annotate › Dimension › Spot Slope; hover a sloped face or edge (the arrow previews the downhill direction), click to place. *Representation* (arrow or triangle) and *Offset from reference* are set on the contextual tab before placing.

**Behaviour & rules.**
- Shows the slope of a face or edge as an arrow or a triangle, with an offset from the reference, in the type's slope units (ratio, percent, degrees, or rise per run).
- The arrow points downhill. In plan views it shows the slope arrow along the steepest descent of the face at the picked point; in sections and elevations it is drawn along the edge as seen, and the triangle form is available only there.
- A horizontal face or edge shows 0 (or the type's text for flat, if set); a vertical face cannot carry a spot slope and is refused.
- Type options: slope units and format, arrowhead, leader line length, text settings shared with DOC-ANN-006, and the triangle's run length (paper mm).
- The value updates when the host slope changes; MEP segments use their own slope (Chapter 4).

**Data (RXD).** `anno.spot` with `spotKind = "slope"`, `representation`, `offset`, `ref` (face or edge sub-reference plus parameter).

**Commands.** `spot.create_slope(view_id, ref, point, type_id?, representation?: arrow|triangle) -> annotation_id`

**Acceptance criteria.**
- [ ] A spot slope on a 2 % ramp shows "2 %" and updates when the ramp slope changes.
- [ ] A spot slope on a roof plane in plan points downhill and flips when the roof's slope direction is reversed.
- [ ] Placing a spot slope on a vertical wall face is refused.

*Built (2026-10-27):* Annotate › Dimension › *Spot Slope* in plans, sections and elevations: one click on a sloped face. The spot reads its element's top surface at the point, found from above each time it is drawn, so it follows the face: in a plan its steepest slope, with a 12 mm arrow through the point pointing downhill and the value above it; in a section or elevation the slope of that face's line in the view plane (a roof's or ramp's profile as cut or seen), as an arrow or a right triangle 10 mm along the horizontal. In a section the click is taken on the cut plane, where the profile is drawn. Units per spot: percent (default, `57.7 %`), ratio (`1 : 1.7`, run per unit of rise) or degrees (`30°`); a flat face reads `0 %`. A point well below its element's top (more than 0.15 m) is on a vertical face, such as a wall's side, and `spot.create_slope` refuses it (`spot.vertical`); a point over no surface of the element is refused too (`spot.no_face`). `spot.create_slope` is the application's (it checks the faces) and stores the spot with `spot.put_slope`; Properties show the value, Slope Units, Representation (sections and elevations), prefix and suffix. The stroke font gains `%` and `°`. Not yet: spot slope types (arrowhead, leader length, flat text), MEP segments' own slopes (Chapter 4), the hover preview of the downhill direction, and an offset from the reference other than moving the text (`spot.set` `leader_point`).

- [x] *(Built 2026-10-27, `tests/sample_features.rs`.)* The house sample's spot slope on its 30° hip roof reads `57.7 %` downhill to the east, `30°` and `1 : 1.7` in the other units, and `1 : 1` once the roof is set to 45°; a section's triangle on the cut roof reads the same slope; a point on a wall's cut face is refused; the triangle is refused in a plan.

### Text notes and leaders
**ID:** DOC-ANN-009 · **Priority:** P0 · **Ref:** research/04 §4.2; web: Edit a Text Note, Text Note Type Properties, Check Spelling in Text Notes, Options Dialog: Check Spelling Tab

**Purpose.** Free text in views and on sheets.

**Workflow.**
1. Annotate › Text › Text (default shortcut TX).
2. Click for unwrapped text, or drag a box for wrapped text; choose leaders in the contextual tab (none, one segment, two segments, arc; left or right side; attached at top, middle or bottom).
3. Type in the in-place editor; finish by clicking outside.

**UI.** Edit Text contextual tab: bold, italic, underline, subscript, superscript, all caps; lists (bullets, numbers, lowercase and uppercase letters) with indent levels; paragraph alignment; add/remove leaders; symbols; find and replace; spell check (in-place underlining with suggestions, using Hunspell dictionaries, LGPL/MPL); *Close* (finish editing). Default shortcuts in the editor: Ctrl+B, Ctrl+I, Ctrl+U, Ctrl+Shift+A (all caps); Esc or a click outside finishes. Options on the Text panel: *Show border when editing*, *Show opaque background when editing*.

**Behaviour & rules.**
- **Text types:** colour, pen, background (opaque or transparent), border on/off and offset, leader arrowhead, font, size (paper mm), tab size, bold/italic/underline, width factor.
- The frame width is the wrapping width; the box can be rotated.
- Leaders end in the type's arrowhead and can be dragged; leaders are not associative (they do not follow elements).
- *Find and replace* works across the current view, selected views or the whole project.
- **Formatting in the editor** (`text.format`): a style toggles on the selected characters (or, with no selection, on the characters typed next). Bold, italic, underline, subscript, superscript and all caps combine freely, except that subscript and superscript exclude each other. All caps changes display only; the stored characters keep their case, so *Find and replace* matches the typed text. Formatting is stored as character-style runs in `content`. The whole edit session, from opening the editor to closing it, is one undo step.
- **Closing the editor** (`text.close_editor`): commits the content with `text.set_content` and leaves edit mode. A note left empty is deleted. The editor is also closed by Esc, a click outside the note, or starting another command.
- **Text type editor** (`texttype.edit`; Annotate › Text panel launcher, or Edit Type on a text note): the Type Properties dialog for text types, listing the type properties above plus line weight, leader/border offset and tab size, with Duplicate, Rename, Delete and a sample preview. Validation: size 0.5–100 mm (paper), width factor 0.1–10, tab size 0.5–100 mm, offset 0–20 mm; the font must be available or a substitute is named (DOC-ANN-000). A type in use cannot be deleted without choosing a replacement.
- **Spell check** (`text.check_spelling`; Annotate › Text › Check Spelling, default shortcut F7): checks text notes in the selection or, with nothing selected, in the active view or sheet. Other text (parameter values, schedule cells, tag labels) is not checked. A dialog shows each unknown word in context with suggestions and Ignore, Ignore All, Add to dictionary, Change, Change All; changes are made through `text.set_content` and form one undo step. Text notes inside groups are skipped unless the group is being edited, and the dialog reports how many were skipped. In BIMserver mode, notes in views owned by someone else are skipped and reported (Chapter 6).
- **In-context spell check:** while the editor is open, unknown words are underlined; the context menu offers suggestions and *Add to dictionary*. Underlining disappears when the editor closes.
- **Spelling options** (Options › Check Spelling, a per-user preference): ignore words in capitals, ignore words with numbers, flag repeated words, main dictionary (any installed Hunspell dictionary; Reviera ships British and American English), and additional user dictionaries (plain word lists; a building-terms list is shipped). In-context checking can be switched off.

**Data (RXD).** `anno.text`: `style`, `position`, `rotation`, `width` (null when unwrapped), `content` (portable rich-text runs: paragraphs, list levels, character styles `bold`, `italic`, `underline`, `sub`, `sup`, `caps`), `leaders[]`, `alignH`, `alignV`. Text types: `standards/text_styles`, kind `textstyle`. User dictionaries are per-user files outside the project and are never written to RXD.

**Commands.** `text.create(view_id, position, content, type_id?, width?, rotation?) -> annotation_id`; `text.set_content(annotation_id, content) -> ()`; `text.format(style: bold|italic|underline|all_caps|subscript|superscript) -> ()` (applies to the open editor's selection; fails with `text.no_editor` when no editor is open); `text.close_editor() -> ()`; `text.add_leader(annotation_id, side, attach, points[], shape?: straight|arc) -> ()`; `text.remove_leaders(annotation_id) -> ()`; `text.find_replace(scope, find, replace, match_case?, whole_word?) -> count`; `texttype.create(name, properties) -> type_id`; `texttype.edit(type_id?) -> ()` (opens the editor above); `text.check_spelling(scope?: view|selection) -> spelling_issue[]` (query; each issue is `{annotation_id, offset, word, suggestions[]}`; from Lua and MCP it only reports, and fixes are made with `text.set_content`)

**Acceptance criteria.**
- [ ] A numbered list with two indent levels keeps its numbering after editing and in the PDF output.
- [ ] *Find and replace* across the project reports the number of replacements and can be undone in one step.
- [ ] Selecting a word and pressing Ctrl+B makes it bold; one Undo after closing the editor removes the whole edit session.
- [ ] `text.check_spelling` on a view with the note "Concret slab" returns one issue for "Concret" with "Concrete" among the suggestions.
- [ ] A text type with size 0 cannot be saved in the type editor.

*Built (2026-10-26), in sections and elevations:* text notes go in sections and elevations, at a position in the view's frame (x to the right, height up), drawn and picked there and printed with the view.

### Tags by category, tag all and loaded tags
**ID:** DOC-ANN-010 · **Priority:** P0 · **Ref:** research/04 §4.3

**Purpose.** Label elements with values read live from their parameters.

**Workflow.**
1. Annotate › Tag › Tag by Category (default shortcut TG): hover an element and click. Contextual options: orientation (horizontal, vertical, or model-aligned in 3D), leader on/off, leader end (attached or free), leader length.
2. Tag All Not Tagged: a dialog lists categories that have a loaded tag; choose all objects in the view or only selected ones, include linked elements, leader and orientation.
3. Loaded Tags and Symbols: per category, the default tag type.

**Behaviour & rules.**
- A tag is an instance of an annotation family whose labels name parameters (Chapter 2). Its text is computed from the host's current parameter values.
- Editing a label in the tag edits the host parameter (for example door number), as a normal undoable parameter change, subject to ownership (Chapter 6).
- A tag with an attached leader follows its host. A tag with a free leader end keeps its position. *Leader start* and *leader end* can each be free or attached; the attachment point can snap to snap references defined in the tag family.
- Tags of linked elements stay bound to the link instance.
- *Tag All Not Tagged* never creates a second tag of the same tag type for an element that already has one in the view.
- Tag placement avoids overlapping existing annotation where possible (simple greedy search of candidate positions around the host).

**Data (RXD).** `anno.tag`: `tagType` (family type reference), `host` (GlobalId or linked reference), `position`, `orientation`, `angle`, `leader {start, end, elbow, startMode, endMode}`, `excludeFromLeaderOutline`.

**Commands.** `tag.create(view_id, host_ref, position?, tag_type_id?, orientation?, leader?) -> annotation_id`; `tag.tag_all(view_id, categories[], scope: view|selection, include_links?, leader?, orientation?) -> annotation_id[]`; `tag.set_default_type(category, tag_type_id) -> ()`; `tag.set_leader(annotation_id, start_mode?, end_mode?, points?) -> ()`

**Acceptance criteria.**
- [ ] Changing a door's Mark from 101 to 102 updates every tag of that door at once.
- [ ] Typing a new value into a door tag changes the door's Mark parameter.
- [ ] Running *Tag All Not Tagged* twice creates no duplicates.

*Built (2026-10-26), in sections and elevations:* tags go in sections and elevations; a tag hangs from its host's point in the model seen through the view's frame (a door's or window's centre, a wall's middle at half its height, a room's tag point 1.2 m above its level, EXPERT_REVIEW E-43). *Tag All Not Tagged* there tags what the view shows: elements of the categories between the view plane and the far clip, within its width.

### Multi-category, material, room, area and space tags
**ID:** DOC-ANN-011 · **Priority:** P0 (room tags), P1 (others) · **Ref:** research/04 §4.3

**Behaviour & rules.**
- **Multi-category tag:** tags elements of any category through parameters they share. It can be the default tag for any category in Loaded Tags and Symbols, and is then used by Tag by Category and Tag All.
- **Material tag:** tags the material of the face or compound layer under the cursor (in plans and sections); the reference is the element plus the layer index sub-reference (`#layer:<function>:<n>`, Chapter 2a).
- **Room, area and space tags:** placed with Architecture › Room & Area › Tag Room (default shortcut RT), Tag Area, and Analyze › Spaces › Tag Space. They show name, number, area and other values. The room tool can tag automatically on placement (Chapter 3).
- A room tag outside its room shows a warning marker until moved back or given a leader.

**Data (RXD).** `anno.tag` with `tagKind` = `multicategory` | `material` | `room` | `area` | `space`.

**Commands.** `tag.create_material(view_id, element_ref, layer_ref, position, tag_type_id?) -> annotation_id`; `tag.create_room(view_id, room_id, position?, tag_type_id?) -> annotation_id`

**Acceptance criteria.**
- [ ] A material tag on the insulation layer of a wall in section shows the insulation material's name.

**Implementation notes (2026-10-13), DOC-ANN-009…011.**
- As built:
  - `tag.create(view_id, host_ref, position?, tag_type_id?, orientation?, leader?)`, `tag.create_room(view_id, room_id, position?, tag_type_id?)` and `tag.tag_all(view_id, categories[], scope: view|selection, element_ids?, leader?, orientation?)`.
  - `tag.set_leader(annotation_id, end_mode: none|attached|free, points?)`, `tag.set_type(annotation_ids[], tag_type_id, orientation?)`, `tag.set_default_type(category, tag_type_id)` and `tag.list(view_id)`.
  - `text.create(view_id, position, content, type_id?, width?, rotation?)`, `text.set_content(annotation_id, content)`, `text.set(annotation_ids[], type_id?, width?, rotation?)`, `text.add_leader(annotation_id, side?, attach?, points[], shape?)`, `text.remove_leaders(annotation_id)` and `text.list(view_id)`.
- Storage: tags and notes are `anno.tag` and `anno.text` objects under the view's annotation prefix, like dimensions. Deleting a view deletes them.
- Tag types are built in, until tag families exist:
  - Door Tag (Mark, box) and Window Tag (Mark, hexagon).
  - Room Tag (name and number) and Room Tag With Area.
  - Wall Tag (type name) and Multi-Category Tag (Mark).
- Tag text: a tag type's fields are schedule fields (`param:elem.mark`, `type`, `room.area`, …), so tags and schedules always show the same values. Editing a tag's value in Properties writes the host's parameter (`schedule.set_cell`).
- Tag placement:
  - A tag keeps its head at an offset from its host's anchor (an insert's centre, a wall's midpoint, a room's tag point), so it follows the host.
  - Moving a tag together with its host moves it once. A tag whose host is deleted is orphaned: drawn in orange, and kept.
- Tag All tags the Reviera walls, doors, windows and rooms on the plan's level. Imported elements are tagged one by one with Tag by Category.
- Text notes:
  - Plain text: `\n` separates lines, and the text wraps at the type's size within an optional width.
  - Types: Text 2.5, 3.5 and 5 mm. Leaders are straight, with a closed arrowhead.
  - The window asks for the text in a multi-line box. Properties edit the text, type, width and rotation.
- Room labels: until now rooms were labelled automatically in plans. A room with a room tag in the view now shows the tag instead.
- Output: the window and PDF use the same layout. The window draws in the stroke font, and the PDF prints real text.
- Later: tags and text in sections, elevations and sheets; rich text and the in-place editor; spell check; find and replace; text types as standards; tag families and material tags; free leader points by dragging; and tag placement that avoids overlaps.

### Tag by example
**ID:** DOC-ANN-012 · **Priority:** P2 · **Ref:** research/04 §4.3 *Tag by Example*

**Purpose.** Repeat an existing tag's set-up on many elements.

**Workflow.** Select a reference tag › Tag by Example; choose target elements (all in view, selection, or by filter, including nested family instances); choose whether to replace existing tags of the same category; a live preview shows proposed tags; confirm.

**UI.** Contextual tab *Tag by Example*: target scope (All in view, Selection, Filter with a filter picker), *Include nested families*, *Replace existing tags*, preview toggle, Finish, Cancel. The preview draws proposed tags in a highlight colour; clicking a proposed tag excludes it.

**Behaviour & rules.**
- New tags copy the reference tag's type, orientation, leader settings and its offset relative to the host's local axes.
- Targets are elements of the reference tag's host category (or of any category the tag family supports, for multi-category tags) that are visible in the view. Elements already carrying a tag of the same type are skipped unless *Replace existing tags* is on, in which case their tags are deleted and recreated.
- The host's local axes are its family placement axes (for walls and other linear elements, the location line direction). Mirrored hosts mirror the offset, so tags stay on the same side relative to the host's facing.
- Where the copied position would overlap existing annotation, the tag is still placed (no automatic shifting), and the count of overlaps is reported so the user can review them.
- Linked elements can be targets when the link is visible (DOC-ANN-000).
- The command is one undo step.

**Data (RXD).** Creates ordinary `anno.tag` objects (DOC-ANN-010); nothing records the example relation.

**Commands.** `tag.by_example(view_id, reference_tag_id, targets: all|selection|filter, filter_id?, replace_existing?) -> annotation_id[]`

**Acceptance criteria.**
- [ ] Tagging 40 windows by example places each tag at the same relative offset as the reference tag.
- [ ] Windows already tagged with the same tag type are skipped unless *Replace existing tags* is on.
- [ ] A mirrored window receives its tag on the mirrored side.

### Stair, beam and other specialised tags
**ID:** DOC-ANN-013 · **Priority:** P1 · **Ref:** research/04 §4.3, §4.5; web: Annotate the Stair Path, About Tagging Beam Systems

**Behaviour & rules.**
- **Stair tread/riser numbers:** numbers along a stair run, with start number, direction, orientation and visibility per run; they are included in Tag All.
- **Beam annotations:** places tags and spot elevations at the ends and middle of selected structural beams by rule (Chapter 4 owns beam-specific settings).
- **Span direction, reinforcement symbols, stair path:** symbols tied to structural floors, reinforcement and stairs (Chapters 3 and 4); they are annotation objects bound to their host like tags.
- **Beam system span tag** (Annotate › Symbol › Beam, `tag.beam_system`): pick a beam system (Chapter 4, STR-BSYS-001) in a plan. The tag is a span arrow drawn perpendicular to the system's beams across the system boundary, with a label that by default reads "<beam type> @ <spacing>" (for fixed-number layouts, "<beam type> × <count>"). Its labels are ordinary tag labels, so the tag family can show other system parameters. The arrow length follows the boundary and the label updates when the layout changes. Framing tags on the individual beams are placed with *Beam Annotations* instead.
- **Stair path** (Annotate › Symbol › Stair Path, `tag.stair_path`): pick a stair in a plan that does not show its path yet. The annotation draws the walk line along the stair's path (centre or offset line from the stair recipe, Chapter 3), a start symbol, an arrowhead at the end, and an "UP" or "DN" text that follows whether the view looks down on the stair from above or below its mid-height. Instance options: show up/down text, up and down text strings, text orientation, text offset, line start and end offsets, *Full step arrow*. Type options: arrow type, start symbol, line style, text font and size, and whether the path is drawn to the cut line only. One stair path per stair per view; placing a second is refused.
- **Reinforcement symbols** (Annotate › Symbol › Area / Path / Fabric Reinforcement, `tag.reinforcement_symbol`): pick an area, path or fabric reinforcement system (Chapter 4). An area symbol draws the system boundary span arrows and a label listing the active layers (for example "T1 Ø12@200, B1 Ø10@150"); a path symbol draws the path with its bar size, spacing and number of bars; a fabric symbol shows the sheet type and lap. Symbols update when the system changes and are orphaned when it is deleted.
- These annotations follow DOC-ANN-010 for leaders, ownership and linked hosts, and they are included in *Tag All Not Tagged* for their categories.

**Data (RXD).** `anno.tag` with `tagKind` = `stairnumber` | `beamsystem` | `stairpath` | `reinforcement` (with `reinforcementKind` = area | path | fabric) | `spandirection`; `host` is the stair, run, beam system or reinforcement system GlobalId. Stair path instance options: `showUpDown`, `upText`, `downText`, `startOffset`, `endOffset`, `fullStepArrow`.

**Commands.** `tag.stair_numbers(view_id, run_ids[], start?, direction?) -> annotation_id[]`; `tag.beam_annotations(view_id, beam_ids[], settings) -> annotation_id[]`; `tag.beam_system(view_id, system_id, type_id?, position?) -> annotation_id`; `tag.stair_path(view_id, stair_id, type_id?) -> annotation_id`; `tag.reinforcement_symbol(view_id, system_id, kind: area|path|fabric, type_id?, position?) -> annotation_id`. A host of the wrong kind fails with `tag.host_kind_mismatch`; these commands are refused in 3D views.

**Acceptance criteria.**
- [ ] Stair tread numbers renumber when a run gains a tread.
- [ ] A beam system span tag reads "W310x39 @ 1200" and changes to "@ 1500" when the system spacing changes.
- [ ] A stair path in the plan of the stair's base level shows "UP"; the same stair in the plan above shows "DN".
- [ ] Adding a top layer to an area reinforcement system adds it to the area reinforcement symbol's label.

### Keynotes
**ID:** DOC-ANN-014 · **Priority:** P1 · **Ref:** research/04 §4.4 · screenshots/documentation/keynote_legend_detail.png

**Purpose.** Short coded notes that refer to a project-wide keynote list.

**Workflow.**
1. Annotate › Tag › Keynote ▾ › Element Keynote / Material Keynote / User Keynote / Keynoting Settings.
2. Keynoting Settings: keynote table source, path type (absolute, relative to the project, or in a library location), Reload, numbering method (*By keynote*, showing the key; *By sheet*, numbering keynotes sequentially per sheet).
3. Element keynote: click an element; it shows the keynote of the element's type. Material keynote: click a face or layer; it shows the material's keynote. User keynote: pick any key from the hierarchy browser.

**Behaviour & rules.**
- **Keynote table format:** Reviera reads a plain tab-separated text file with columns key, text and optional parent key, which builds a tree. It also accepts the same three columns as CSV. Reviera ships its own sample tables (not derived from vendor files).
- The table can also be stored inside the project (RXD), which avoids missing-file problems in BIMserver mode. The settings choose *external file* or *embedded*; an external file can be embedded at any time.
- A missing key shows "?" and is reported.
- Keynote tag types: number only, text only, boxed number.
- Keynote legends: DOC-VIEW-010.

**Data (RXD).** `anno.keynote`: `mode` (element | material | user), `host` reference, `key` (user keynotes), `position`, `leader`. Settings and embedded table in `standards/text_styles` (kind `keynote.settings`, `keynote.table`).

**Commands.** `keynote.settings(source?, path_type?, numbering?: by_keynote|by_sheet, embed?) -> ()`; `keynote.reload() -> ()`; `keynote.create(view_id, mode, host_ref?, key?, position, tag_type_id?) -> annotation_id`

**Acceptance criteria.**
- [ ] Editing a key's text in the table and reloading updates every keynote showing that key.
- [ ] With *By sheet* numbering, the same key shows different numbers on two sheets, matching each sheet's keynote legend.

*Built (2026-10-27):* Annotate › Tag › *Element Keynote* and *User Keynote*, in plans, ceiling plans, sections and elevations: click the element, then where the keynote goes (a user keynote's key is chosen from the table first). An element keynote shows the keynote of the element's type (`type.keynote`, Identity Data), read live: changing the type's keynote, or the table's text, changes every keynote showing it. Styles: *Boxed Number* (default), *Number*, *Text*. A leader runs to the element's tag point when the keynote is placed off it. A key missing from the table (or a type without one) shows `?`, is drawn orphaned (orange) and is listed as missing by `keynote.list`. Keynotes pick, highlight, print, list in Properties (keynote and its text, key for user keynotes, style, leader) and float in 3D views as class *Keynotes* with key and text (DOC-ANN-025). Commands: `keynote.create(view_id, mode?: element|user, host_ref?, key?, position?, style?, leader?)`, `keynote.set(annotation_ids, style?, key?, leader?, position?)`, `keynote.list(view_id?)`. The table is the project's embedded one (`keynotes.load`, `keynotes.set`, DM-CLS-001). Not yet: material keynotes (materials carry no keynote), *By sheet* numbering and keynote legends (DOC-VIEW-010), Keynoting Settings with an external file kept linked, and `keynote.reload` (`keynotes.load` reads the file again).

- [x] *(Built 2026-10-27, `tests/sample_features.rs`.)* The managed project's element keynotes read their types' keys (08 11 13, 08 51 13) and follow a type's change; the Text style reads the table; a user keynote with a key the table lacks shows `?` and is listed as missing.

### Symbols
**ID:** DOC-ANN-015 · **Priority:** P0 · **Ref:** research/04 §4.5

**Purpose.** Place generic, non-reporting annotation symbols.

**Workflow.** Annotate › Symbol › Symbol; choose a type in the Type Selector; set the number of leaders and *Rotate after placement* on the contextual tab; click to place (and click again to set the rotation when that option is on).

**Behaviour & rules.**
- Annotate › Symbol › Symbol places generic annotation family instances (north arrows, graphic scales, centrelines, note symbols). Symbols are sized in paper units and keep their printed size at any scale.
- A north-arrow symbol can be set to point to true north; it then follows the project's true-north angle (Chapter 6).
- A graphic-scale symbol reads the view scale automatically.
- Symbol instances can be scheduled with note-block schedules (DOC-SCH-006).
- Symbols are view-owned (DOC-CORE-004); they can be placed in any 2D view, on sheets, and in locked 3D views (on the view plane). Labels in the symbol family that use instance parameters are edited in place or in Properties.
- Symbols may have any number of leaders (0–4 from the ribbon; more through Lua), each ending in the type's arrowhead and not associated with model elements.
- Symbol families come from Reviera's annotation family library or the user's own families (Chapter 2); a symbol family must have category *Generic Annotations* or another annotation category marked as placeable.
- *Hide at scales coarser than* is available per instance, as for view markers.

**Data (RXD).** `anno.symbol`: `type` (family type reference), `position`, `rotation`, `parameterValues`, `trueNorth` flag, `leaders[]`, `hideCoarserThan`.

**Commands.** `symbol.place(view_id, type_id, position, rotation?, parameters?) -> annotation_id` (a type that is not an annotation family fails with `symbol.not_annotation`)

**Acceptance criteria.**
- [ ] A graphic scale placed in a 1:100 view shows 1:100 divisions and updates when the scale changes to 1:50.
- [ ] A true-north arrow rotates by 15° when the project's true-north angle changes by 15°.
- [ ] A note-block schedule lists two symbols of the same note family placed on one sheet.

### Detail lines
**ID:** DOC-ANN-016 · **Priority:** P0 · **Ref:** research/04 §4.6

**Behaviour & rules.**
- Annotate › Detail › Detail Line (default shortcut DL). Draw tools: line, rectangle, inscribed and circumscribed polygon, circle, arcs (three-point, centre-ends, tangent, fillet), spline, ellipse and partial ellipse, pick lines. Options: chain, offset, radius.
- Each line has a line style (DOC-GFX-013). Detail lines are view-owned and appear only in their view.
- *Pick lines* copies the geometry of model edges into detail lines (not associative).
- Detail lines can be edited with the modify tools of Chapter 3 (move, copy, rotate, mirror, trim/extend, split, offset, align) and with their end-point grips. Joined end points (from *Chain* or snapping) move together when one is dragged, unless the join is broken with *Disallow join*.
- Detail lines are drawn above model content and below annotation, in their draw order (DOC-ANN-020). They can be dimensioned and can host masking.
- Changing the line style of a selection is one command; the style list shows only line styles of the *Lines* category (DOC-GFX-013). Detail lines can also be converted to model lines and back (Chapter 3), which moves them between view ownership and the model.
- Zero-length segments are not created; a curve shorter than the project's short-curve tolerance (Chapter 2) is refused with a warning.
- **Interactive tool.** Drawing follows Conventions §2.1: each finished segment is a micro-operation `detail.segment(curve, style)`, so a crash loses at most the segment being drawn.

**Data (RXD).** `anno.detailline`: `style`, `curve` (line, arc, circle, ellipse or spline in view coordinates), `joins[]` (ids of joined lines at each end), `drawOrder`.

**Commands.** `detail.line(view_id, curves[], line_style_id?) -> annotation_id[]`; `detail.set_line_style(annotation_ids[], line_style_id) -> ()`

**Acceptance criteria.**
- [ ] A chain of detail lines drawn with *Chain* on forms connected segments sharing end points.
- [ ] Dragging a shared end point of a chain moves both joined lines.
- [ ] A detail line in one plan does not appear in a duplicate of that plan made with plain *Duplicate*.

### Filled and masking regions
**ID:** DOC-ANN-017 · **Priority:** P0 · **Ref:** research/04 §4.6

**Behaviour & rules.**
- Annotate › Detail › Region ▾ › Filled Region or Masking Region: sketch one or more closed loops (nested loops become holes).
- **Filled region type:** foreground pattern and colour, background pattern and colour, masking (opaque) on/off, boundary pen. Each boundary edge has its own line style, which may be `<Invisible lines>`.
- **Masking region:** an opaque region in the background colour that hides model and annotation beneath it in its view; boundary lines are optional.
- Area of a filled region is reported as a read-only parameter.
- Loops must be closed and must not self-intersect; nested loops alternate between region and hole; touching or overlapping sibling loops are refused at Finish with the problem edge highlighted.
- Region fill patterns may be drafting patterns (scale with the view) or model patterns (real size, DOC-GFX-014). With *Masking* on in the type, the region hides everything beneath it in draw order, like a masking region, and still shows its own pattern.
- Editing the boundary (*Edit Boundary*) re-enters sketch mode; the sketch follows Conventions §2.1 with micro-operations `region.segment`, `region.delete`, `region.style` and a replay function.
- *Region edges mask coincident lines* (DOC-OUT-001) lets region boundaries hide model lines that lie exactly on them in print.
- Filled regions can be placed in any 2D view, on sheets, in legends and in detail groups. IFC annotation export writes them as `IfcAnnotationFillArea` (DOC-IFC-002).

**Data (RXD).** `anno.region`: `regionKind` (filled | masking), `type`, `loops[]`, `edgeStyles[]`, `drawOrder`. Region types: `standards/text_styles`, kind `regiontype` (`fgPattern`, `fgColour`, `bgPattern`, `bgColour`, `masking`, `pen`).

**Commands.** `region.create(view_id, loops[], kind: filled|masking, type_id?, edge_styles?) -> annotation_id` (invalid loops fail with `region.invalid_loops`); `regiontype.create(name, fg_pattern?, fg_colour?, bg_pattern?, bg_colour?, masking?, pen?) -> type_id`

**Acceptance criteria.**
- [ ] A masking region hides the model linework beneath it on screen and in the PDF.
- [ ] A filled region with an inner loop reports its area as the outer area minus the hole.
- [ ] A sketch with two overlapping loops is refused at Finish.

### Detail components, repeating details, insulation and detail groups
**ID:** DOC-ANN-018 · **Priority:** P1 · **Ref:** research/04 §4.6

**Behaviour & rules.**
- **Detail component** (Annotate › Detail › Component ▾, default shortcut CM): point-based or line-based detail item families (studs, bolts, flashings and so on), view-owned, taggable and keynotable.
- **Repeating detail:** a line-based array of a detail component. Type: component, layout (fill available space, fixed distance, fixed number, maximum spacing), inside (whether end components stay inside the line), spacing, component rotation.
- **Insulation:** batt insulation drawn along a line, with width and bulge-to-width ratio (1/x).
- **Detail group:** a reusable named group of view-owned elements; instances update when the definition changes. Groups follow Chapter 3 group rules, restricted to view-owned elements.

**Data (RXD).** `anno.component` (`type`, `position`, `rotation`, `curve` for line-based), `anno.repeating` (`type`, `curve`), `anno.insulation` (`curve`, `width`, `bulgeRatio`); detail groups in `model/groups` with `scope = "detail"`.

**Commands.** `detail.component(view_id, type_id, position_or_curve, rotation?) -> annotation_id`; `detail.repeating(view_id, type_id, curve) -> annotation_id`; `detail.insulation(view_id, curve, width, bulge_ratio?) -> annotation_id`; `detail.create_group(view_id, annotation_ids[], name) -> group_id`; `detail.place_group(view_id, group_id, position) -> annotation_id`

**Acceptance criteria.**
- [ ] Lengthening a repeating brick detail adds bricks according to the layout rule.
- [ ] Editing a detail group definition updates all instances in all views.

### Draw order
**ID:** DOC-ANN-020 · **Priority:** P1 · **Ref:** research/04 §4.6 *Draw order*

**Purpose.** Control which view-owned drafting is drawn on top where items overlap.

**Workflow.** Select drafting items › Modify › Arrange: Bring to Front, Bring Forward, Send to Back, Send Backward (default shortcuts: none; assignable in the keymap, Chapter 1).

**Behaviour & rules.**
- Modify › Arrange: Bring to Front, Bring Forward, Send to Back, Send Backward. Draw order applies among view-owned drafting (detail components, regions, detail lines) in the same view. Model content is always behind drafting; masking regions mask only what lies below them in this order.
- Annotation (text, tags, dimensions, symbols) is always above drafting and is not ordered by this command; selecting annotation disables the Arrange buttons.
- *Forward* and *Backward* move an item past the next item that overlaps it, not past items elsewhere in the view, so the result is visible after one click.
- New drafting items are placed at the front. Copying preserves relative order within the copied set and places the set at the front of the target view.
- Items in a detail group keep their order inside the group; the group as a whole is ordered among the other items.
- Draw order is honoured in every output, because the draw list sorts view-owned drafting by it (DOC-HLR-004).

**Data (RXD).** Each annotation object has an integer `drawOrder`; ties are broken by id. Reordering rewrites `drawOrder` only on the moved objects (gaps are used) so that concurrent edits of other items in the same view merge cleanly.

**Commands.** `detail.arrange(annotation_ids[], action: front|forward|back|backward) -> ()` (annotation ids of the wrong kind fail with `detail.not_drafting`)

**Acceptance criteria.**
- [ ] Sending a filled region to the back makes a detail component above it visible in the PDF.
- [ ] *Bring Forward* on a line under two overlapping regions puts it above the nearer region only.
- [ ] Reordering one item changes only that item's RXD object.

### Revision clouds
**ID:** DOC-ANN-021 · **Priority:** P1 · **Ref:** research/04 §4.6, §5.7

**Behaviour & rules.**
- Annotate › Detail › Revision Cloud: sketch lines, arcs or a rectangle in any view or on a sheet; the cloud's arcs are generated along the sketch with the project's arc length (paper mm).
- Each cloud has a *Revision* property; it defaults to the latest revision that is not issued.
- Clouds of an issued revision cannot be added, moved or edited (DOC-SHT-008).
- The cloud's revision appears in the revision schedule of every sheet that shows the cloud (directly on the sheet, or in a view placed on it, if the cloud is inside the crop).
- Revision tags are tags of the revision cloud category; they show the revision number (per project or per sheet, DOC-SHT-008).

**Data (RXD).** `anno.revcloud`: `revision` id, `loops[]` (sketch), `arcLength` override (optional). Revisions in `project/revisions`.

**Commands.** `revcloud.create(view_id, loops[], revision_id?) -> annotation_id`; `revcloud.set_revision(annotation_ids[], revision_id) -> ()`

**Acceptance criteria.**
- [ ] Drawing a cloud in a plan that is placed on sheet A101 adds the revision to A101's revision schedule.
- [ ] Moving a cloud of an issued revision is refused.

### Colour fill legends and colour schemes
**ID:** DOC-ANN-022 · **Priority:** P1 · **Ref:** research/04 §4.6 *Color schemes*, *Color Fill Legend*

**Purpose.** Colour rooms, areas, spaces, zones, ducts and pipes by parameter, with a legend.

**Workflow.**
1. Annotate › Color Fill › Color Fill Legend: click in a plan or section; choose the space category and colour scheme.
2. Edit Scheme (from the legend or the view property *Color Scheme*): category, scheme title, *Colour by* parameter, *By value* or *By range* (range edges, units, stepping), include linked elements, per-value colour, fill pattern and visibility; the *In use* column shows values present in the model.

**Behaviour & rules.**
- A view uses one colour scheme per category; the view property *Color Scheme Location* puts fills behind (Background) or in front of (Foreground) model geometry.
- New values found in the model get colours automatically from a perceptually distinct palette; the user can lock colours.
- The legend lists the values present in the view (or all values, by legend type) and updates automatically.

**Data (RXD).** Colour schemes in `views/color_schemes` (kind `colorscheme`: `category`, `parameter`, `mode`, `entries[]` of `{value or range, colour, pattern, visible}`); legends as `anno.colorlegend` (`scheme`, `position`, `columns`).

- **Colour scheme editor** (`colorscheme.edit`; *Edit Scheme* on a selected legend, the view property *Color Scheme*, or Architecture › Room & Area ▾ › Color Schemes). Left: category picker and the list of schemes for that category (Duplicate, Rename, Delete). Right: title, *Colour* parameter, *By value* / *By range*, *Include elements from links*, and the entries table (Value or range, Visible, Colour, Fill pattern, Preview, In use) with Add, Remove, Move up and down. In range mode the table edits the range edges, units and the step used by *Add*. Validation: range edges must increase; a value can appear once; a scheme in use by a view cannot be deleted (the views are listed); changing the parameter of a scheme in use asks for confirmation because it rebuilds the entries. OK applies all changes through `colorscheme.set_entry` and related commands as one undo step.

**Commands.** `colorscheme.create(name, category, parameter, mode: value|range, ranges?) -> scheme_id`; `colorscheme.set_entry(scheme_id, value_or_range, colour?, pattern_id?, visible?) -> ()`; `colorscheme.edit(scheme_id?) -> ()` (opens the editor above); `view.set_color_scheme(view_id, category, scheme_id? (none clears), location?: background|foreground) -> ()`; `colorlegend.place(view_id, scheme_id, position) -> annotation_id`

**Acceptance criteria.**
- [ ] Colouring rooms by Department shows each department in its own colour, and adding a room with a new department adds a legend row.

### Matchlines
**ID:** DOC-ANN-023 · **Priority:** P1 · **Ref:** research/04 §4.7, §1.8

**Purpose.** Mark where a plan split across dependent views (and sheets) continues.

**Workflow.** View tab › Sheet Composition › Matchline enters sketch mode; draw one or more lines (open polylines are allowed); set *Top* and *Bottom* constraints in Properties; Finish. Add view references beside it (DOC-ANN-024).

**Behaviour & rules.**
- View tab › Sheet Composition › Matchline: sketch one or more lines in a parent plan (or in a dependent view, which adds the matchline to the parent).
- Matchlines are visible in the parent and in all its dependents. Their vertical extent (top and bottom constraints) controls which sections and elevations show them.
- Appearance comes from the *Matchline* object style and the view's annotation category settings.
- Top and Bottom are a level plus offset, or *Unlimited*; the default is the parent's level to the level above. Plans of other levels whose cut plane lies within that range also show the matchline when they are parents of dependents with the same scope boxes; otherwise matchlines belong to one parent only.
- A matchline does not crop anything; the dependents' crop regions do. Reviera offers *Fit crops to matchline* on the contextual tab, which snaps the nearest crop edges of the dependents to the matchline (one undo step).
- Editing a matchline (*Edit Sketch*) follows the sketch-mode rules of Conventions §2.1. Deleting the parent's last dependent leaves the matchline in place.
- The matchline is printed with its view; *Hide crop boundaries* does not hide it.

**Data (RXD).** `anno.matchline` in the parent view's annotation document: `curves[]`, `topLevel`, `bottomLevel`, offsets.

**Commands.** `matchline.create(view_id, curves[], top?, bottom?) -> annotation_id` (when `view_id` is a dependent view, the matchline is stored on its parent and the parent id is in the result's owner)

**Acceptance criteria.**
- [ ] A matchline sketched in a parent plan appears in both of its dependent views.
- [ ] A matchline with Top = Level 3 appears in a section through it between the parent level and Level 3 only.
- [ ] *Fit crops to matchline* moves the facing crop edges of two dependents onto the matchline in one undo step.

### View references
**ID:** DOC-ANN-024 · **Priority:** P1 · **Ref:** research/04 §4.7

**Behaviour & rules.**
- View tab › Sheet Composition › View Reference: choose the view kind and the target view; place the symbol (usually beside a matchline).
- The symbol shows the target's detail number and sheet number, and may show the sheet collection and shared parameters. The reference label is per instance, with a type default (DOC-VIEW-004).
- Double-clicking the symbol opens the target view.
- When the target is not on a sheet, the symbol shows placeholder dashes.
- The target list on the contextual tab is filtered by view kind (floor plans, ceiling plans, sections, elevations, drafting views, dependent views) and by name; dependents of the current view's parent are listed first.
- When the target view is deleted, the view reference is orphaned (DOC-CORE-003) and shows "?"; *Change target* re-points it.
- View references can be placed in plans, sections, elevations, callouts, drafting views and on sheets; they are view-owned and are copied by *Duplicate with Detailing*.
- In combined PDF output they become internal links to the target's sheet (DOC-OUT-003). `view.find_referring` (DOC-VIEW-001) lists them as referrers of their target.

**Data (RXD).** `anno.viewref`: `target` (view id), `position`, `label`, `type`, `rotation`.

**Commands.** `viewref.create(view_id, target_view_id, position, type_id?, label?) -> annotation_id`; `view.go_to_target(annotation_id) -> view_id` (fails with `viewref.orphaned` when the target is gone)

**Acceptance criteria.**
- [ ] After the target dependent view is placed on sheet A103 as detail 2, the view reference reads "2 / A103".
- [ ] Deleting the target view shows "?" in the symbol and lists it in the warnings.
- [ ] In a combined PDF, clicking the view reference opens sheet A103.


### Annotation floating in 3D views
**ID:** DOC-ANN-025 · **Priority:** P2 · **Ref:** owner request (2026-10-26)

**Purpose.** See the plans' notes, tags and dimensions in 3D, where they apply, without annotating the 3D view itself.

**Behaviour & rules.**
- A 3D view shows the annotation of the model's plans as floating labels: each a boxed block of text drawn flat to the screen like a tooltip, with a dot at its point in the model and a short leader, so it reads the same from any angle.
- Points: a text note 1 m above its plan's level; a tag (and a keynote) at its element: a door's or window's at the middle of its opening, half its height up; a wall's at its middle, half its height up; a room's 1.2 m above its tag point; a dimension's segment values on its dimension line at the level. A host tagged in several plans is labelled once. *(Corrected 2026-10-27, owner: labels had attached at a door's or window's sill, and a wall's at a fixed height.)*
- Classes: text notes, dimensions, and tags by the category they tag (door tags, window tags, room tags, …). A view shows annotation or not, and each class on its own; a new view shows every class but dimensions.
- Each label's box is centred above its point, with a short leader up to the middle of the box's bottom edge (owner, 2026-10-27). Nearer labels are placed first; one that would cover another moves up, and is left out after three steps. At most 150 at once.
- View tab › Annotation (and Annotate tab › 3D, and the View menu): *Annotation in 3D* (a toggle button showing whether the active view shows them) and *Annotation Visibility…* (a check box for annotation in the view and one for each class with its count).

**Data (RXD).** The view's `rxd:annotations_3d`: `{ shown, hidden: [class…], occlude, hidden_from: [plans | sections | elevations …] }`.

**Commands.** `view.annotations_3d(view_id) -> { shown, occlude, classes[], sources[] }`; `view.set_3d_annotations(view_id, shown?, occlude?, classes?: {class: bool}, sources?: {plans|sections|elevations: bool}) -> ()`

**Acceptance criteria.**
- [x] The annotated plan sample's 3D view shows its two notes and its door, window and room tags, and its dimensions when chosen; hiding door tags leaves the others; undo brings back the previous choice. *(Built 2026-10-26, `tests/sample_features.rs`.)*
- [x] The annotated plan sample's section and elevation hold their own dimensions (3 m heights, the overall and clear widths), tags and notes, drawn and printed with them; shown in 3D when chosen, the elevation's note sits on the south wall's face and double-clicking the section's opens Section A. *(Built 2026-10-26, `tests/sample_features.rs`; checked in the window, with the Dimension tool in the elevation.)*
- [x] Clicking the Kitchen tag's label selects the tag; double-clicking it opens Level 1 with the tag selected and framed; Zoom to Fit with it selected frames it in a plan and in 3D. *(Built 2026-10-26: label hit tests in `reviera_render::tooltips`, each label's plan in `tests/sample_features.rs`; checked in the window.)*

*Built (2026-10-26):* as above. *(Owner, 2026-10-26: "On the 3D annotations should float as tool-tip style display. There needs to be a toggle to turn it on and off." "Visibility controls should be independent visibility, a control with a visibility check for each class of annotation.")* 

*Built (2026-10-26), labels behind the model:* a label is left out while the model hides its place: rays from the eye (or, in a parallel view, from far back along the view direction) to three points up from its anchor (at it, 1.2 m and 2.4 m above) meet what one cannot see through (opaque meshes: not glass, room and space volumes, openings or virtual volumes) more than 0.4 m before the point; what the section box and slices cut away and what is hidden in the view do not count. The test runs every frame on a bounding volume hierarchy of the model's triangles, built once per change (`reviera_render::bvh`). Per view, on unless set (`occlude`, *Hide labels behind the model* in Annotation Visibility).

*Built (2026-10-26), picking labels:* a click on a label (its box or its dot) selects its annotation, before the model under it; nearer labels, drawn in front, are picked first; Ctrl and Shift add and remove as elsewhere. A selected annotation's label is edged in the selection colour. A double click (the second click on the same label within 0.45 s) opens the plan the annotation is drawn in, with it selected. *(Also: a double click's second press now reaches the views as a press on every toolkit; wxWidgets sends it only as a double-click event, so the second of two quick clicks had been dropped.)* *Built (2026-10-26), framing:* the plan opened is zoomed to the annotation: Zoom to Fit (ZF) counts selected annotation, measured as drawn in the active plan (a note's or tag's outline, a dimension's lines) or, in a 3D view, at its labels' points; annotation selected alone is framed with room around it (1.5 times its size, at least 1.5 m), as a clash's box is. *Built (2026-10-27):* spots (class *Spot elevations and coordinates*, at the point each reads, read again on the model) and keynotes (class *Keynotes*, key and text at their element) float too (DOC-ANN-006 … 008, DOC-ANN-014).

*Built (2026-10-26), sections and elevations:* their annotation floats too (DOC-ANN-001, -009, -010 there). A section's notes and dimensions stand on its view plane, mapped from the view's frame; an elevation's, which stands in front of what it shows, are moved back along its view onto the model, where the first surface (glass included) meets the ray within its depth, and stay on its plane when nothing is met; tags stand at their hosts. *From* in Annotation Visibility shows each kind of view's annotation (plans, sections, elevations) on its own; a view shows the plans' only unless set, so existing 3D views look as before (`hidden_from`). A host tagged in several kinds of views shown is labelled once, from the plans first. A double click on a label opens the view it is drawn in, section or elevation too.
---

## 5.14 Schedules

Schedules are tabular views of live model data. Every schedule definition is an RXD document `schedules/<scheduleId>` (kinds `schedule.*`). Cell values are always computed and never stored.

### Creating schedules
**ID:** DOC-SCH-001 · **Priority:** P0 · **Ref:** research/04 §2 intro, §2.1, §2.4

**Purpose.** List elements of a category (or of several categories) with chosen parameters.

**Workflow.**
1. View tab › Create › Schedules ▾ › Schedule/Quantities (or another kind, DOC-SCH-006).
2. New Schedule dialog: discipline filter, category list (including *Multi-Category*, Sheets, Views, Sheet Collections, Revisions), *Schedule building components* or *Schedule keys* (with key name), name, phase, *Include elements in links*.
3. OK opens the Schedule Properties dialog (DOC-SCH-002 onwards); OK there opens the schedule view.

**Behaviour & rules.**
- A component schedule has one row per element (or per group of identical rows when *Itemize every instance* is off, DOC-SCH-005).
- A **multi-category** schedule lists elements of all model categories and offers only parameters common to them (shared parameters, IFC common properties, and built-in identity fields).
- Schedules are phase-specific: they list elements that exist in the schedule's phase, filtered by its phase filter (DOC-GFX-017). Design options: the primary option by default, or a chosen option per set.
- *Include elements in links* adds rows from loaded linked models; those rows are read-only.
- Evaluation is incremental: a parameter change re-evaluates only affected rows, totals and groups.

**Data (RXD).** `schedules/<id>`, kind `schedule.component` (or `schedule.keyschedule`): `category` (or `"multi"`), `phase`, `phaseFilter`, `designOptions`, `includeLinks`, and the blocks `fields`, `filters`, `sortGroup`, `formatting`, `appearance`, `embedded`, `layout` (headers, merges, images, column widths).

**Commands.** `schedule.create(kind, category, name, phase_id?, include_links?, key_name?) -> schedule_id`; `schedule.get_table(schedule_id, include_hidden?) -> table` (query; also MCP resource `reviera://schedule/{id}`)

**Implementation notes (2026-10-13).**
- As built:
  - `schedule.create(category, name?, fields?)`, `schedule.set_fields(schedule_id, fields)` and `schedule.set_sort(schedule_id, levels, itemize?, grand_totals?)`.
  - `schedule.set_cell(element_ids, field, value)`, `schedule.get_table(schedule_id)` and `schedule.fields(category)`.
  - `schedule.export(schedule_id, path, format: csv|tsv, overwrite?)`.
- Schedules are RXD-only elements (`rxd:schedules/<guid>`) with the definition in `rxd:schedule`.
- Fields are computed fields (count, type, level, IFC class and GlobalId, room, insert, wall, slab and roof quantities) or `param:<reference>` for any parameter of the category.
- The view is a grid tab with a Schedule contextual tab (Fields, Sort, Itemize, Export).
  - `schedule.set_filters(schedule_id, filters[{field, op: equals|not_equals|contains|greater_than|less_than, value}])` (built 2026-10-21): rows show only elements passing every filter; numbers compare by value, text ignoring case; Modify Schedule › **Filter…** sets one. Ducts and Pipes schedules have Size, Length and the kept network results (MEP-CALC-002).
- Filter groups in the dialog, header and footer groups, calculated and combined fields, key schedules, phases and links come later.

**Acceptance criteria.**
- [ ] A door schedule lists every door of the schedule's phase and gains a row when a door is placed.
- [ ] `schedule.get_table` returns the same cell text the schedule view shows, including totals.

### Fields
**ID:** DOC-SCH-002 · **Priority:** P0 · **Ref:** research/04 §2.2 *Fields tab*

**Behaviour & rules.**
- Fields tab: available fields on the left, scheduled fields on the right, with move up/down; *Select available fields from*: the category, related categories (for example the room a door opens from and to, the host level, the type), or a linked model; field search and filters by parameter kind.
- Buttons: *New parameter* (creates a project parameter bound to the category, Chapter 2), *Add calculated parameter*, *Combine parameters*, *Edit*, *Delete*.
- **Combined parameters:** concatenate several parameters into one column, each with prefix, suffix and separator; a sample preview is shown.
- Built-in fields include Count, Family, Type, Family and Type, IFC GlobalId, IFC Class, Workset, Phase Created, Phase Demolished, Design Option, and element quantities (length, area, volume) from IFC quantity sets or computed by OCCT.
- Removing a field also removes filters, sorting and formatting that use it, after confirmation.

**Data (RXD).** `fields[]`: `{id, source: param|related|calculated|combined|builtin, parameter, relation?, heading, ...}`.

**Commands.** `schedule.add_field(schedule_id, field_source, index?) -> field_id`; `schedule.remove_field(schedule_id, field_id) -> ()`; `schedule.move_field(schedule_id, field_id, index) -> ()`; `schedule.combine_fields(schedule_id, parts[], name) -> field_id`

**Acceptance criteria.**
- [ ] A door schedule with *To Room: Name* shows the name of the room each door opens into.
- [ ] A combined field "Level-Number" with separator "-" shows "L1-101".

### Calculated values
**ID:** DOC-SCH-003 · **Priority:** P0 (formula), P1 (percentage) · **Ref:** research/04 §2.2 *Calculated values*

**Behaviour & rules.**
- A calculated field has a name, discipline, value type (length, area, volume, number, integer, currency, yes/no, text, and so on), and either a **formula** or a **percentage**.
- Formulas use the formula language of Chapter 2: arithmetic, comparison, `if`, `and`, `or`, `not`, trigonometric functions, `round`, `roundup`, `rounddown`, `abs`, `sqrt`, `min`, `max`, text concatenation, and unit-aware arithmetic (for example dividing an area by a length gives a length). A formula whose units do not match its value type is rejected with an explanation.
- A percentage field gives one field's value as a percentage of its total within the current group or the grand total.
- Calculated fields can be used for filtering, sorting, grouping and conditional formatting (an improvement over the reference, where some of these are limited).

**Data (RXD).** `fields[]` entry with `source = "calculated"`, `valueType`, `formula` or `percentage {of, by: group|grandtotal}`.

**Commands.** `schedule.add_calculated_field(schedule_id, name, value_type, formula?, percentage_of?, percentage_by?) -> field_id`; `schedule.update_calculated_field(schedule_id, field_id, formula?, value_type?) -> ()`

**Acceptance criteria.**
- [ ] A calculated field `Area * 1.2` of type Area shows values 20 % larger than Area.
- [ ] `Area / Width` with value type Area is rejected because the result is a length.

### Filters
**ID:** DOC-SCH-004 · **Priority:** P0 · **Ref:** research/04 §2.2 *Filter tab* · screenshots/documentation/schedule_properties_filter_tab.png

**Behaviour & rules.**
- Filter tab: a list of rules `field · operator · value`, combined with AND by default. Operators as in DOC-GFX-006.
- Reviera removes the reference's limit of eight AND rules: the filter is a rule tree with nested AND/OR sets, using the same editor and evaluation engine as view filters (DOC-GFX-006). Hidden fields and calculated fields can be used.
- *Filter by sheet*: when the schedule is placed on a sheet, each instance shows only elements visible in views placed on that sheet.
- Rules can test any scheduled field, including hidden, calculated and combined fields, and any parameter of the category that is not scheduled (the editor lists them under *Other parameters*). Values are entered in display units; text comparisons follow the operator's case rule (case-insensitive by default, with a *Match case* option per rule).
- A rule whose field is later removed from the schedule stays valid when it tests a category parameter; it is marked invalid when it tested a calculated field that no longer exists, and an invalid rule is ignored with a warning banner on the schedule view.
- Elements of linked models are filtered with the same rules (DOC-SCH-001).
- *Filter by sheet* evaluates visibility in each viewport's view after crop, view range, hidden elements and filters; temporary hiding is ignored. On a schedule view that is not on a sheet, the option has no effect and the schedule shows all rows.
- Evaluation shares the compiled predicates of view filters (DOC-GFX-006) and is incremental: only elements whose tested parameters change are re-evaluated.

**Data (RXD).** `filters` rule tree (same schema as `filter.rule`'s `ruleTree`), `filterBySheet`.

**Commands.** `schedule.set_filter(schedule_id, rule_tree?, filter_by_sheet?) -> ()` (a rule naming an unknown field fails with `schedule.unknown_field`)

**Acceptance criteria.**
- [ ] Filter "(Level = L1 OR Level = L2) AND Fire Rating has a value" lists exactly the matching doors.
- [ ] A door schedule with *Filter by sheet* on sheet A101 lists only doors visible in views on A101.
- [ ] A filter on a hidden calculated field "Area per person < 10 m²" hides the matching rows while the field stays hidden.
- [ ] Hiding a door in a plan on A101 removes it from A101's filtered schedule instance; temporary hiding does not.

### Sorting, grouping and totals
**ID:** DOC-SCH-005 · **Priority:** P0 · **Ref:** research/04 §2.2 *Sorting/Grouping tab*

**Behaviour & rules.**
- *Sort by*, then any number of *Then by* levels, each ascending or descending, with *Header*, *Footer* (title, count and totals / title and totals / count and totals / totals only) and *Blank line* options.
- *Grand totals* with the same choices and a custom title.
- *Itemize every instance*: when off, rows with identical values in all sorted fields collapse into one row; non-identical cells in that row are left blank, and Count shows the number of collapsed elements.
- Sorting is natural (so "Door 2" comes before "Door 10") and unit-aware.
- Totals are computed per field according to the field's calculation mode (DOC-SCH-007).

**Data (RXD).** `sortGroup[]`: `{field, descending, header, footer, blankLine}`; `grandTotals {mode, title}`; `itemize`.

**Commands.** `schedule.set_sort_group(schedule_id, levels[], grand_totals?, itemize?) -> ()`

**Acceptance criteria.**
- [ ] Grouping doors by Level with footer "Count and totals" shows a count per level and a grand total equal to the number of doors.
- [ ] With itemize off, 12 identical door types on one level collapse to one row with Count = 12.

### Schedule kinds
**ID:** DOC-SCH-006 · **Priority:** P0 (component, sheet list, revision schedule), P1 (key, material takeoff, view list, note block, keynote legend), P2 (graphical column schedule; panel schedules are specified in Chapter 4) · **Ref:** research/04 §2.4, §5.2, §5.6

| Kind (RXD `kind`) | Rows | Notes |
|---|---|---|
| Component `schedule.component` | elements of a category, or of all categories (multi-category) | DOC-SCH-001. |
| Key schedule `schedule.keyschedule` | user-defined keys | Each key row sets several parameters at once. Elements of the category gain a key parameter; choosing a key writes the key's values into the element's parameters (they become read-only on the element while the key is set). Rows are added with *Insert data row*. |
| Material takeoff `schedule.material` | one row per (element, material) | Material fields (name, area, volume, cost, description, plus custom material parameters) combined with element fields. Areas and volumes are computed per compound layer or per solid with OCCT. *As paint* option for painted face materials. |
| Sheet list `schedule.sheetlist` | sheets and placeholder sheets | Fields: sheet number and name, current revision, drawn/checked/approved/designed by, issue date, appears in sheet list, sheet collection, scale (or "As indicated"), sheet width and height, title-block parameters, primary title block. *Insert data row* adds a placeholder sheet (DOC-SHT-007). |
| View list `schedule.viewlist` | views | View name, kind, scale, detail level, template, phase, sheet number, dependency, sheet collection. Some fields (such as name, scale, title on sheet) are editable from the list. |
| Note block `schedule.noteblock` | instances of a chosen generic annotation family | Note number and text; filter by sheet. |
| Keynote legend `schedule.keynotelegend` | keynotes in use | DOC-VIEW-010. |
| Revision schedule `schedule.revision` | revisions on the sheet | Defined inside title block families (Chapter 2) and evaluated per sheet: sequence, number, description, date, issued to, issued by. Usually built bottom-up with a fixed height. |
| Graphical column schedule `schedule.graphicalcolumn` | grid × level matrix of structural columns | Groups similar columns, shows level ranges and off-grid columns. Rendered as a drawing, not a table. |
| Sheet collection schedule | sheet collections | Schedules the collection's custom parameters (DOC-SHT-006). |

**Commands.** Kinds are chosen in `schedule.create`. In addition: `schedule.insert_data_row(schedule_id, values?) -> row_key` (key schedules and sheet-list placeholders); `schedule.delete_data_row(schedule_id, row_key) -> ()`; `element.set_key(element_ids[], key_parameter, row_key?) -> ()`

**Acceptance criteria.**
- [ ] Assigning key "Office Standard" to 30 rooms sets their four finish parameters in one undo step.
- [ ] A material takeoff of walls reports a concrete volume matching the sum of the concrete layers' volumes within 0.1 %.
- [ ] A sheet list shows placeholder rows next to real sheets, sorted by number.

### Formatting and conditional formatting
**ID:** DOC-SCH-007 · **Priority:** P0 (formatting), P1 (conditional formatting) · **Ref:** research/04 §2.2 *Formatting tab*

**Behaviour & rules.**
- Per field: heading text, heading orientation (horizontal or vertical), alignment (left, centre, right), field format (units, rounding, unit symbol, digit grouping; or *Use project settings*), hidden field (kept for sorting and filtering, not shown), calculation (standard, totals, minimum, maximum, minimum and maximum).
- **Conditional format:** rules per field, each `test → cell background colour` (and optionally text colour and bold). The test can use any field of the row, not just the formatted one. Rules are checked in order; the first match wins. *Show conditional format on sheets* controls whether colours print.

**Data (RXD).** `formatting` per field: `heading`, `orientation`, `align`, `format`, `hidden`, `calculation`, `conditional[] {ruleTree, background, textColour, bold}`, `showConditionalOnSheets`.

**Commands.** `schedule.set_field_format(schedule_id, field_id, heading?, orientation?, align?, format?, hidden?, calculation?) -> ()`; `schedule.add_conditional_format(schedule_id, field_id, rule_tree, background, text_colour?, bold?) -> rule_id`; `schedule.remove_conditional_format(schedule_id, rule_id) -> ()`

**Acceptance criteria.**
- [ ] A rule "Area < 10 m²" colours those cells red in the schedule view, and in print only when *Show on sheets* is on.

### Appearance
**ID:** DOC-SCH-008 · **Priority:** P0 · **Ref:** research/04 §2.2 *Appearance tab*

**Purpose.** Control how a schedule looks on sheets and in its view.

**Workflow.** Schedule Properties › Appearance tab, or the Appearance panel of the Modify Schedule tab for cell-level styling (DOC-SCH-010).

**Behaviour & rules.**
- Build schedule top-down or bottom-up; grid lines (line style) and whether grid lines show in headers, footers and spacers; outline (line style); row height variable or user-defined; stripe rows (colour; whether they print); show title; show headers; text types for title, headers and body.
- *Blank row before data* inserts one empty row between headers and data.
- Text types are the project's text types (DOC-ANN-009); their size is in paper millimetres, so a schedule's printed size never depends on a view scale.
- Row height: *Variable* fits the tallest wrapped cell; a fixed height (paper mm) clips longer text and marks the clipped cells in the schedule view.
- The schedule view on screen shows the same fonts and grid as the sheet, but with alternating row shading only when *Stripe rows* is on and with interactive column letters and row numbers that never print.
- Cell styles set in the schedule view (DOC-SCH-010) override the appearance settings for those cells.
- Appearance may be included in a schedule view template (DOC-GFX-004).

**Data (RXD).** `appearance` block: `buildOrder` (top_down | bottom_up), `gridLines {style, inHeaders, inFooters, inSpacers}`, `outline`, `rowHeight` (variable or mm), `stripe {colour, print}`, `showTitle`, `showHeaders`, `blankRowBeforeData`, `textTypes {title, header, body}`.

**Commands.** `schedule.set_appearance(schedule_id, appearance) -> ()` (a line style or text type that does not exist fails with `schedule.unknown_style`)

**Acceptance criteria.**
- [ ] A bottom-up schedule lists its first row at the bottom, directly above the headers.
- [ ] Changing the body text type from 2.5 mm to 3.5 mm enlarges the placed schedule on every sheet.
- [ ] With *Stripe rows* on and *print* off, the PDF shows no stripes.

### Embedded schedules
**ID:** DOC-SCH-009 · **Priority:** P2 · **Ref:** research/04 §2.2 *Embedded Schedule tab*

**Purpose.** Show, under each room or space, the elements it contains.

**Workflow.** Schedule Properties › Embedded Schedule tab: tick *Embedded schedule*, choose the category, then *Embedded Schedule Properties…*, which opens a nested Fields / Filter / Sorting/Grouping / Formatting / Appearance dialog.

**Behaviour & rules.**
- Room and space schedules can embed a sub-schedule of a related category (for example the furniture in each room, or the air terminals in each space). The embedded schedule has its own fields, filter, sorting and formatting, and its rows appear under each parent row.
- Containment is decided by the element's room or space calculation point (Chapter 3 room rules; Chapter 4 for spaces), in the schedule's phase. Elements in no room are not listed.
- Parent schedules allowed: rooms, spaces, areas, and zones (HVAC). One level of embedding only.
- Parent rows with no embedded rows show the parent row alone; *Show empty parents* can hide them.
- Embedded rows are editable in the schedule view like normal rows (DOC-SCH-010); totals of the embedded schedule are per parent.
- Export (DOC-SCH-012) writes embedded rows indented under their parent, with the embedded headers once per parent.

**Data (RXD).** `embedded`: a nested schedule definition with `relation` (for example `room-contents`, `space-contents`), `category`, and its own `fields`, `filters`, `sortGroup`, `formatting`, `showEmptyParents`.

**Commands.** `schedule.set_embedded(schedule_id, category?, definition?) -> ()` (with no category, embedding is removed; a parent that is not a room, space, area or zone schedule fails with `schedule.embed_not_supported`)

**Acceptance criteria.**
- [ ] A room schedule with embedded furniture shows each room's furniture rows below the room row.
- [ ] Moving a chair from room 101 to room 102 moves its row to room 102.
- [ ] Embedding in a door schedule is refused with `schedule.embed_not_supported`.

### Editing in the schedule view
**ID:** DOC-SCH-010 · **Priority:** P0 · **Ref:** research/04 §2.3 · screenshots/documentation/schedule_view_room_finish_grouped_headers.png

**Purpose.** Use the schedule as a spreadsheet-like editor for the model.

**UI.** The schedule view is a grid with lettered columns. Contextual tab Modify Schedule:
- Parameters: Format Unit, Calculated, Delete, Combine Parameters.
- Columns: Insert, Delete, Resize, Hide, Unhide All.
- Rows: Insert (title/header rows, or data rows for key schedules and sheet lists), Delete, Resize.
- Titles & Headers: Merge/Unmerge, Insert Image, Clear Cell, Group, Ungroup.
- Appearance: Shading, Borders, Reset, Font, horizontal and vertical alignment.
- Element: Highlight in Model.

**Behaviour & rules.**
- Editing a cell writes the parameter of the underlying element through a normal parameter command (undoable, checked against ownership in BIMserver mode, Chapter 6). Editing a type parameter shows how many instances will change and asks for confirmation. A cell in a collapsed row (itemize off) edits all elements of the row.
- Read-only cells (computed values, linked rows, values controlled by a key) are shown greyed.
- **Group headers:** selecting several adjacent column headers and choosing *Group* adds a common parent header (for example "Finishes" over base, floor, wall and ceiling columns).
- Selecting rows selects the elements; *Highlight in Model* opens or chooses a view that shows them and zooms to them.
- Column widths set here apply to the schedule on every sheet.
- Paste from the clipboard into a range of editable cells is supported (tab-separated text), as one undoable command.

**Data (RXD).** `layout`: `headerGroups[]`, `merges[]`, `images[]` (asset references), `columnWidths[]`, `cellStyles[]`.

**Commands.** `schedule.set_cell(schedule_id, row_key, field_id, value) -> ()`; `schedule.paste(schedule_id, top_left, tsv_text) -> ()`; `schedule.group_headers(schedule_id, field_ids[], caption) -> ()`; `schedule.ungroup_headers(schedule_id, group_id) -> ()`; `schedule.merge_cells(schedule_id, range) -> ()`; `schedule.insert_image(schedule_id, cell, image_asset) -> ()`; `schedule.set_column_width(schedule_id, field_id, width_mm) -> ()`; `schedule.highlight_in_model(schedule_id, row_keys[]) -> view_id`

**Acceptance criteria.**
- [ ] Changing a door's Fire Rating in the schedule changes the door, and Undo restores it.
- [ ] Pasting 20 rows of values from a spreadsheet application fills 20 rows in one undo step.

### Schedules on sheets
**ID:** DOC-SCH-011 · **Priority:** P0 (place), P1 (split) · **Ref:** research/04 §2.5 · screenshots/documentation/schedule_room_finish_on_sheet.png

**Workflow.**
1. Drag a schedule from the Project Browser onto a sheet. A schedule may be placed on any number of sheets.
2. Resize columns with the triangle grips (applies to the schedule everywhere).
3. **Split:** click the split control on the side of the placed schedule to split it at that row, or use *Split & Place* (number of segments, segment height), which places segments side by side on the same sheet. Drag a segment back onto the previous one to join them.
4. Rotate the schedule instance on the sheet (0°, 90°, −90°).

**Behaviour & rules.**
- Each segment has its own height and position; *Repeat headers* is a per-instance option.
- When rows are added, the last segment grows. If it would run off the title block's drawing area, a warning marker appears on the segment.
- Filter by sheet (DOC-SCH-004) uses the sheet of each placed instance.
- *Edit Schedule* (context menu, with a default shortcut) opens the schedule view.

**Data (RXD).** In `sheets/<sheetId>`: `scheduleInstances[] {schedule, position, rotation, segments[] {startRow, height, position}, repeatHeaders}`.

**Commands.** `sheet.place_schedule(sheet_id, schedule_id, position) -> instance_id`; `scheduleinstance.split(instance_id, segment_heights[], positions?) -> ()`; `scheduleinstance.join(instance_id, segment_index) -> ()`; `scheduleinstance.set_rotation(instance_id, angle) -> ()`

**Acceptance criteria.**
- [ ] Splitting a 90-row schedule into three segments of 30 rows each places three segments, each with headers when *Repeat headers* is on.
- [ ] The same schedule placed on two sheets with *Filter by sheet* shows different rows on each.

### Schedule export
**ID:** DOC-SCH-012 · **Priority:** P0 (CSV), P1 (XLSX, ODS) · **Ref:** research/04 §2.5 *Other schedule capabilities*

**Behaviour & rules.**
- File › Export › Reports › Schedule: delimited text (field delimiter, text qualifier, encoding UTF-8 with or without byte-order mark; export title, headers, group headers and footers, blank rows).
- Spreadsheet export to XLSX (using a GPL-compatible writer such as `rust_xlsxwriter`, MIT/Apache) and OpenDocument ODS, keeping group headers, merges, column widths and conditional colours, with numbers written as numeric cells with units in the header.
- *Export schedules as IFC property sets* is an IFC export option (DOC-OUT-006).
- Export writes what the schedule shows: hidden fields are omitted, itemize and grouping are respected, conditional formats are kept only in XLSX and ODS, and images are omitted (a note lists them). Values are those of the model snapshot at the start of the command.
- Delimited text writes values as displayed (with units when the format shows them), so it round-trips into the reference product and other tools; XLSX and ODS write numeric cells with the unit in the header, so formulas work.
- Several schedules can be exported at once to one workbook (one sheet per schedule) through Lua or MCP; worksheet names are truncated to 31 characters and made unique.
- An existing file is overwritten only after confirmation in the UI; from Lua and MCP, `options.overwrite` must be true.
- Characters that a spreadsheet would treat as a formula at the start of a text cell (`=`, `+`, `-`, `@`) are escaped in delimited output, so exported text cannot run as a formula.

**Commands.** `schedule.export(schedule_id, path, format: csv|tsv|xlsx|ods, options?) -> ()` (`options`: `delimiter`, `qualifier`, `bom`, `include {title, headers, groupHeaders, footers, blankRows}`, `overwrite`)

**Acceptance criteria.**
- [ ] An exported XLSX opens in a spreadsheet application with numeric area values that sum to the schedule's grand total.
- [ ] A CSV export of a schedule with a hidden field has no column for that field.
- [ ] A room name starting with "=" is written to CSV so that a spreadsheet shows it as text.

---

## 5.15 Sheets, title blocks and revisions

Sheets are paper-space views stored as RXD documents `sheets/index` and `sheets/<sheetId>` (kind `sheet`). Paper coordinates are millimetres from the sheet origin (lower-left corner of the title block's drawing area by default).

### Creating and duplicating sheets
**ID:** DOC-SHT-001 · **Priority:** P0 (create), P1 (duplicate modes) · **Ref:** research/04 §5.1 · screenshots/documentation/sheet_with_elevation_viewports.png

**Workflow.**
1. View tab › Sheet Composition › Sheet; or Project Browser › right-click *Sheets* or a sheet collection › New Sheet.
2. New Sheet dialog: title block types (with Load…, and *None*), and *Select placeholder sheets* (DOC-SHT-007).
3. OK creates the sheet, numbered by incrementing the last number used in the target collection (A101 → A102; numbers ending in letters increment the letter).
4. Duplicate Sheet ▸ *Empty sheet* / *With sheet detailing* / *With views*: for views, choose Duplicate, Duplicate with Detailing or Duplicate as Dependent (DOC-VIEW-011); the new views are placed at the same positions.

**Behaviour & rules.**
- **Sheet properties:** number, name, issue date, designed by, checked by, drawn by, approved by, *Appears in sheet list*, *Revisions on sheet* (DOC-SHT-008), current revision (read-only: number, date, description, issued by, issued to), sheet collection, guide grid, plus any custom sheet parameters (Chapter 2).
- Sheet numbers are unique within a sheet collection (and among sheets not in a collection). A clash is rejected.
- Deleting a sheet removes its viewports; the views are kept.
- Sheet-owned annotation (text, lines, revision clouds, symbols, images) is stored in the sheet document.
- *Insert › Image* places a raster image or PDF page on a sheet (stored as an RXD asset); *Insert › Spreadsheet* (P2) places a linked spreadsheet range as a table.

**Data (RXD).** `sheets/<sheetId>`: `number`, `name`, `collection`, `parameters{}`, `titleBlocks[]`, `viewports[]`, `scheduleInstances[]`, `annotation[]`, `revisionsOnSheet[]`, `guideGrid`, `appearsInSheetList`, `isPlaceholder`.

**Commands.** `sheet.create(title_block_type_id?, number?, name?, collection_id?, from_placeholder?) -> sheet_id`; `sheet.duplicate(sheet_id, mode: empty|detailing|views, view_mode?: plain|with_detailing|dependent) -> sheet_id`; `sheet.set_parameter(sheet_id, name, value) -> ()`; `sheet.delete(sheet_ids[]) -> ()`; `sheet.place_image(sheet_id, asset_or_path, position, size?) -> annotation_id`

**Acceptance criteria.**
- [ ] Creating a sheet after A109 proposes A110.
- [ ] Duplicating a sheet with views as dependents creates dependent views placed at the same viewport positions.
- [ ] Creating sheet A101 in a collection where A101 already exists is rejected; creating it in another collection succeeds.

### Title blocks and project information
**ID:** DOC-SHT-002 · **Priority:** P0 · **Ref:** research/04 §5.2 · screenshots/documentation/title_block_sheet_information.png

**Purpose.** The sheet border and information panel, filled automatically from project, sheet and collection data.

**Behaviour & rules.**
- Title blocks are annotation families of category *Title Blocks*, created in the family editor (Chapter 2) from Reviera's own sheet-size templates (ISO A0–A4, ANSI A–E, ARCH A–E, and custom sizes). They contain lines, regions, images, logos and **labels**.
- **Label sources**, in priority order for the same parameter name: sheet instance value; sheet collection value (DOC-SHT-006); project information value. Project information covers organisation name and description, building name, author, project issue date, status, client name, address, project name and number, plus custom project-information parameters. Where IFC has a home for a value (for example `IfcProject.Name`, `IfcProject.LongName`, `IfcPostalAddress` of the site or building, `IfcOrganization` of the owner history), Reviera stores it there and reads it back; the rest is RXD.
- The *Scale* label shows the scale of the views on the sheet, or *As indicated* when they differ. The title block type can set a *Scale override for multiple values* text.
- A sheet can hold several title block instances; the *Primary title block* flag decides which one supplies values to sheet list schedules.
- The sheet's *Sheet width* and *Sheet height* come from the primary title block.
- A title block family may embed a revision schedule (DOC-SCH-006).
- **Swap title block:** choosing another type in the Type Selector keeps the sheet's viewports in place (they are positioned relative to the sheet origin).
- *Reset title block positions* returns title blocks on selected sheets to the sheet origin.

**Data (RXD).** `titleBlocks[] {type, position, isPrimary, parameterValues}`; project information as IFC attributes where available, otherwise `project/settings` (`projectInfo`).

**Commands.** `titleblock.place(sheet_id, type_id, position?) -> instance_id`; `titleblock.swap(instance_id, type_id) -> ()`; `titleblock.set_primary(instance_id) -> ()`; `titleblock.reset_positions(sheet_ids[]) -> ()`; `project.set_info(name, value) -> ()`

**Acceptance criteria.**
- [ ] Changing the project name updates the label on every sheet.
- [ ] A sheet with views at 1:50 and 1:100 shows "As indicated", or the type's override text when set.
- [ ] Swapping an A1 title block for an A0 one keeps all viewports where they were.

### Placing views on sheets (viewports)
**ID:** DOC-SHT-003 · **Priority:** P0 · **Ref:** research/04 §5.3

**Workflow.**
1. Drag a view from the Project Browser onto an open sheet; or View tab › Sheet Composition › Place View (dialog listing views not yet placed); or right-click a view › Add to Sheet (choose the sheet).
2. Position the viewport; its view title follows (DOC-SHT-004).
3. Double-click inside a viewport, or use *Activate View*, to edit the view through the sheet (other sheet content is halftoned); double-click outside or *Deactivate View* to return. The double-click behaviour is a user option: Activate, Open in its own tab, or Do nothing. *Open* (context menu) opens the view in its own tab.

**Behaviour & rules.**
- A model view can be on only one sheet (DOC-CORE-005). Dragging an already placed model view offers to go to its sheet instead.
- The viewport shows the view's crop region, or the extents of all visible content if the view is not cropped. Changing the view's crop or scale updates the viewport immediately.
- *Rotation on sheet*: none, 90° clockwise, 90° counter-clockwise.
- **Swap view:** on the Modify | Viewports tab, the view drop-down replaces the placed view with another unplaced view of the same kind group, keeping the position (by view origin or by the anchor, DOC-SHT-005).
- **Move aligned to sheet:** moves selected viewports to another sheet, keeping their positions.
- Viewports can be pinned; new viewports are pinned by default (user option).
- The *Detail number* is unique per sheet; new viewports get the next free number.
- **Align views:** while dragging, a viewport snaps so that its grids, levels and reference planes align with those of other viewports on the sheet or on a chosen guide grid.
- **Open hosting sheet:** Project Browser › right-click a placed view › *Open Sheet* (`sheet.open_for_view`) opens the sheet that holds the view and selects its viewport. The item is disabled for views not on a sheet. A schedule or legend placed on several sheets opens a picker listing them.
- **Remove from sheet:** Project Browser › expand a sheet › right-click a view under it › *Remove from Sheet*, or select the viewport and press Delete. Both call `viewport.remove`; the view itself is kept, its markers in other views lose their sheet and detail numbers (they show placeholder dashes), and any saved position assignment is kept for the next placement.

**Data (RXD).** `viewports[] {id, view, position, viewportType, rotation, detailNumber, anchor, savedPosition, titleOffset, titleLength, pinned}`.

**Commands.** `sheet.place_view(sheet_id, view_id, position, viewport_type_id?) -> viewport_id`; `viewport.move(viewport_id, position) -> ()`; `viewport.set_rotation(viewport_id, rotation) -> ()`; `viewport.swap_view(viewport_id, view_id) -> ()`; `viewport.move_to_sheet(viewport_ids[], sheet_id) -> ()`; `viewport.activate(viewport_id, on) -> ()`; `viewport.set_detail_number(viewport_id, number) -> ()`; `viewport.remove(viewport_ids[]) -> ()`; `sheet.open_for_view(view_id) -> sheet_id` (fails with `view.not_on_sheet`; for views on several sheets, without a UI it returns the first by sheet number)

**Acceptance criteria.**
- [ ] Placing a plan already on sheet A101 onto A102 is refused with an offer to open A101.
- [ ] Changing the plan's scale from 1:100 to 1:50 doubles the viewport's size on the sheet.
- [ ] Swapping a viewport's view keeps its sheet position when both views share the same view origin.
- [ ] *Remove from Sheet* on a section's viewport keeps the section view and changes its marker head to placeholder dashes.

**Implementation notes (2026-10-13), DOC-SHT-001…003 and DOC-SCH-011.**
- As built:
  - `sheet.create(number?, name?, size?)` (ISO `A0`…`A4`, landscape; default `A1`), numbered from the last sheet number (A101 → A102).
  - `sheet.set_parameter(sheet_id, number?, name?, size?, parameters{})` and `sheet.delete(sheet_ids[])`.
  - `sheet.place_view(sheet_id, view_id, position?)` and `sheet.place_schedule(sheet_id, schedule_id, position?)`.
  - `viewport.move(sheet_id, viewport_id, position)` and `viewport.remove(sheet_id, viewport_ids[])`. Viewport ids are per sheet, so these take the sheet too.
- Storage: sheets are RXD-only elements (`rxd:sheets/<guid>`) with their data in `rxd:sheet`: number, size, title-block values, viewports and placed schedules.
- Title block: until title block families exist, every sheet has Reviera's built-in ISO title block.
  - The border has a 20 mm binding margin and 10 mm elsewhere.
  - A 180 × 48 mm information panel holds the project name and number (from `IfcProject`), sheet name and number, the five sheet values, the date, and the scale (*As indicated* when the viewports' scales differ).
- Viewports:
  - Plans, sections and elevations can be placed (3D views and legends come later), each on one sheet only. `view.already_on_sheet` names the sheet that holds it.
  - A viewport draws its view's whole drawing (crop regions come later) at the view scale, centred on its position.
  - Under it sits a view title: detail number in a circle, the view name underlined, and the scale.
  - Deleting a view removes its viewports.
- Schedules on sheets are ruled tables of the live schedule: title, headings, rows and totals, with numbers right-aligned.
- Window:
  - The sheet view draws on paper, through the same pipeline as plans.
  - A click selects a viewport or schedule, dragging moves it, and Delete removes it.
  - The Sheet contextual tab has Place View, Place Schedule and Open View.
  - The Project Browser lists *Sheets (all)* with the views on each sheet. Rename and Delete there work on sheets and schedules too.
- PDF: `export.pdf` prints a sheet as one page exactly its paper size. Text is WinAnsi-encoded, so `m²` prints.
- Later: duplicating, collections, placeholders, revisions, guide grids and saved positions, viewport types and rotation, activating views on sheets, and drag and drop from the browser.

### Viewport types and view titles
**ID:** DOC-SHT-004 · **Priority:** P0 · **Ref:** research/04 §5.4 · screenshots/documentation/viewport_types_view_titles_sheet.png

**Behaviour & rules.**
- Viewport type properties: title family (an annotation family whose labels can show view name or *Title on sheet*, detail number, sheet number, scale, sheet collection, reference label and shared parameters), *Show title* (yes, no, or only when the sheet has more than one viewport), *Show extension line*, extension line pen, colour and pattern, *Preserve title position*.
- Instance: *Title on sheet* (overrides the view name on sheets), detail number, rotation; the title and extension line can be dragged; the extension length has a grip.
- View titles snap to view boundaries, crop regions, title block edges and guide grid lines while dragged, without first selecting the viewport.

**Data (RXD).** Viewport types in `standards/text_styles` (kind `viewport.type`); instance fields in `viewports[]`.

**Commands.** `viewporttype.create(name, title_type_id?, show_title?, show_extension?, pen?, colour?, pattern_id?, preserve_title_position?) -> type_id`; `viewport.set_type(viewport_ids[], type_id) -> ()`; `viewport.set_title(viewport_id, offset?, extension_length?) -> ()`

**Acceptance criteria.**
- [ ] A viewport type with *Show title: when multiple* hides the title on a sheet with one viewport and shows it when a second viewport is added.

### Guide grids, saved positions and view anchors
**ID:** DOC-SHT-005 · **Priority:** P1 · **Ref:** research/04 §5.3 *Align views*, *View-to-Sheet Positioning* · screenshots/documentation/positioning_view_ribbon_saved_positions_2026.png

**Purpose.** Place views consistently across many sheets.

**Behaviour & rules.**
- **Guide grid:** View tab › Sheet Composition › Guide Grid; named, with spacing and extents; assigned to sheets through the sheet property. Viewport datums and view titles snap to it. Guide grids are not printed.
- **Saved positions:** with a viewport selected, *Save Position* stores its sheet position under a name. Views can be assigned a saved position before they are placed; dropping them on any sheet places them at that position. *Edit Position* (with Finish/Cancel) moves the stored position and updates every viewport assigned to it. *Manage Positions* renames and deletes positions.
- **View anchor:** *View origin* (default; ignores the crop, so plans of different levels line up from sheet to sheet), or *Centre*, *Top left*, *Top right*, *Bottom right*, *Bottom left* of the crop region (the viewport moves when the crop changes, so that the anchored point stays fixed; the annotation crop is ignored).
- Legends and graphical column schedules placed on many sheets can share one saved position.
- Saved positions can be transferred to other projects (Chapter 6, COL-MNT-005).

**Data (RXD).** `sheets/index`: `guideGrids[] {id, name, spacing, extents}`, `savedPositions[] {id, name, position, anchor}`; viewport fields `anchor`, `savedPosition`.

**Commands.** `guidegrid.create(name, spacing, extents) -> guide_grid_id`; `sheet.set_guide_grid(sheet_ids[], guide_grid_id?) -> ()`; `position.save(viewport_id, name) -> position_id`; `position.assign(view_ids[], position_id?) -> ()`; `position.edit(position_id, position) -> ()`; `position.delete(position_id) -> ()`; `viewport.set_anchor(viewport_ids[], anchor) -> ()`

**Acceptance criteria.**
- [ ] Assigning a saved position to ten floor plans and placing them on ten sheets puts each at the same sheet coordinates, and the grids line up from sheet to sheet.
- [ ] With anchor *Top left*, enlarging the crop to the right and down leaves the viewport's top-left corner where it was on the sheet.

### Sheet collections
**ID:** DOC-SHT-006 · **Priority:** P1 · **Ref:** research/04 §5.5 · screenshots/documentation/sheet_collections_browser_print_set.png

**Purpose.** Group sheets into issue packages, each with its own numbering and data.

**Behaviour & rules.**
- Project Browser › right-click *Sheets* › New Sheet Collection; rename; delete (sheets move to no collection).
- Sheets move between collections by drag and drop in the browser or through the *Sheet collection* parameter.
- Sheet numbers must be unique within a collection, not across collections. New sheets created in a collection continue its numbering.
- Collections have custom parameters (category *Sheet Collections*), and they can be scheduled. A collection's values show (read-only) on its sheets and in their title block labels. Priority: sheet value, then collection value, then project information value. A parameter bound to both the Sheets and the Sheet Collections categories is two independent parameters.
- *Sheet collection* and *Referencing sheet collection* parameters are available on views, in filters, schedules and labels (view titles, view references, callout heads, section and elevation marks, title blocks).
- Print and export dialogs offer each collection as a selection filter (DOC-OUT-001).

**Data (RXD).** `sheets/index`: `collections[] {id, name, parameters{}}`; sheet field `collection`.

**Commands.** `sheetcollection.create(name) -> collection_id`; `sheetcollection.rename(collection_id, name) -> ()`; `sheetcollection.delete(collection_id) -> ()`; `sheetcollection.move_sheets(sheet_ids[], collection_id?) -> ()`; `sheetcollection.set_parameter(collection_id, name, value) -> ()`

**Acceptance criteria.**
- [ ] Two collections can each hold a sheet numbered G001.
- [ ] Setting *Issue purpose* on a collection shows that text in the title blocks of all its sheets unless a sheet has its own value.

### Sheet list and placeholder sheets
**ID:** DOC-SHT-007 · **Priority:** P1 · **Ref:** research/04 §5.6

**Purpose.** List the whole drawing set, including drawings produced elsewhere, and turn planned drawings into real sheets.

**Workflow.**
1. Create a sheet list (View tab › Schedules ▾ › Sheet List, DOC-SCH-006) and choose its fields.
2. In the sheet list view, Modify Schedule › Rows › *Insert Data Row* adds a placeholder; type its number, name and other values in the row.
3. Later, View tab › Sheet Composition › Sheet › *Select placeholder sheets* (multi-select) creates real sheets from them.

**Behaviour & rules.**
- A placeholder sheet is a row in a sheet list schedule with no real sheet behind it (for example a consultant's drawing). It is added with *Insert data row* and can belong to a sheet collection.
- New Sheet › *Select placeholder sheets* turns placeholders into real sheets, keeping number, name and parameter values.
- Sheet lists can be filtered (for example *Appears in sheet list* = yes and collection = X) and placed on a cover sheet.
- Placeholder numbers obey the same uniqueness rule as sheets (DOC-SHT-001): a placeholder and a real sheet cannot share a number in one collection.
- Placeholders appear in sheet lists, view/sheet sets and the Project Browser (under *Sheets*, with a placeholder icon), but cannot be opened, printed or exported; output commands skip them and report them.
- Placeholders have no title block and no revisions; *Current revision* is blank unless typed into a custom parameter.
- Deleting the placeholder row in a sheet list deletes the placeholder. Real sheets cannot be deleted from a sheet list (the row's Delete is disabled for them).

**Data (RXD).** Placeholder sheets are entries in `sheets/index` with `isPlaceholder = true`, `number`, `name`, `collection`, `parameters{}`, and no sheet document. Converting one creates the `sheets/<sheetId>` document and clears the flag, keeping the same id, so references to it stay valid.

**Commands.** `sheet.create_placeholder(number, name, collection_id?, parameters?) -> sheet_id`; `sheet.convert_placeholders(sheet_ids[], title_block_type_id?) -> sheet_id[]` (a real sheet in the list fails with `sheet.not_placeholder`)

**Acceptance criteria.**
- [ ] Converting a placeholder keeps its number and name and removes the placeholder row.
- [ ] Creating a placeholder numbered like an existing sheet in the same collection is refused.
- [ ] A view/sheet set containing a placeholder exports the other sheets and reports the placeholder as skipped.

### Revisions
**ID:** DOC-SHT-008 · **Priority:** P1 · **Ref:** research/04 §5.7, §4.6 *Revision Cloud*

**Purpose.** Track issued changes on drawings.

**Workflow.**
1. View tab › Sheet Composition › Revisions opens the Sheet Issues/Revisions dialog.
2. Table columns: sequence, revision number (derived), numbering sequence, date, description, issued (checkbox), issued to, issued by, show (cloud and tag / tag / none). Buttons: Add, Move Up/Down, Merge Up/Down, Delete.
3. Settings: numbering *per project* or *per sheet*; revision cloud arc length; **numbering sequences** (numeric with start value, minimum digits, prefix and suffix; alphanumeric with a custom list of values, for example A–Z without I and O).
4. Draw clouds (DOC-ANN-021), tag them, and tick *Issued* when the revision is issued.

**Behaviour & rules.**
- A sheet's revisions are the union of the revisions of clouds visible on it (on the sheet, or inside the crop of a view placed on it) and the revisions added manually through *Revisions on sheet*.
- *Current revision* of a sheet is its highest-sequence revision.
- **Per-sheet numbering:** each sheet numbers its own revisions in sequence order; a revision tag shows the number relevant to the sheet the cloud appears on.
- Ticking *Issued* locks the revision: its clouds cannot be created, moved, edited or deleted, and its row cannot be edited, until it is unticked.
- *Merge* combines a revision with the one above or below; clouds are reassigned.
- Deleting a revision is allowed only when no cloud uses it, or after the user reassigns its clouds.

**Data (RXD).** `project/revisions`: kind `revision` (`sequence`, `numberingSequence`, `date`, `description`, `issued`, `issuedTo`, `issuedBy`, `show`), kind `revision.sequence`, and settings (`numberingMode`, `arcLength`). Sheet field `revisionsOnSheet[]`.

**Commands.** `revision.create(description?, date?, numbering_sequence_id?, index?) -> revision_id`; `revision.update(revision_id, fields) -> ()`; `revision.set_issued(revision_id, issued) -> ()`; `revision.merge(revision_id, direction: up|down) -> ()`; `revision.reorder(revision_id, index) -> ()`; `revision.delete(revision_id, reassign_to?) -> ()`; `revision.set_settings(numbering?: per_project|per_sheet, arc_length?) -> ()`; `revisionsequence.create(name, kind: numeric|alphanumeric, start?, min_digits?, prefix?, suffix?, values?) -> sequence_id`; `sheet.set_revisions_on_sheet(sheet_id, revision_ids[]) -> ()`
`revision.manage() -> ()` opens the *Sheet Issues/Revisions* dialog of the workflow above. Each table edit, button and setting calls the matching command, and OK commits the dialog session as one undo step named "Sheet Issues/Revisions". Validation: a revision row that is issued is read-only except for the *Issued* tick; Merge and Delete are disabled on issued rows and on the only row; numbering sequence values must be unique and non-empty, and an alphanumeric sequence needs at least one value; a numeric sequence needs a start value of 0 or more and minimum digits 1–6; the cloud arc length must be 1–50 mm (paper). Deleting a revision that clouds use asks for the revision to reassign them to.

**Acceptance criteria.**
- [ ] A cloud for revision 3 placed in a view on sheet A201 makes revision 3 appear in A201's revision schedule and sets A201's current revision to 3.
- [ ] With per-sheet numbering, a sheet whose only revisions are sequences 2 and 5 numbers them 1 and 2.
- [ ] Editing a cloud of an issued revision is refused.
- [ ] In the Sheet Issues/Revisions dialog, the Merge button is disabled on an issued revision.

---

## 5.16 Assemblies, assembly views and assembly sheets

An **assembly** is a named, typed set of model elements that is documented and fabricated as one piece: a precast panel with its reinforcement and lifting anchors, a steel column with its welded plates, a prefabricated bathroom pod, a stud-wall panel, a reception desk. Reviera stores it as an `IfcElementAssembly` that aggregates its members. This section owns the assembly element, its type, its views and its sheets. Chapter 4 creates assemblies automatically for precast pieces (STR-PRE-002) and steel shop assemblies (STR-STL-010 numbering) and uses the views and sheets defined here for shop drawings (STR-PRE-006). Chapter 2 (DM-ELEM-002) fixes the storage summary.

### The assembly element and assembly types
**ID:** DOC-ASM-001 · **Priority:** P2 · **Ref:** research/04 §3.6 (*Assembly Code* setting), §5.1 (assembly-specific sheet fields); web: About Assemblies, Create an Assembly, Assembly Instance Properties, Assembly Type Properties, IfcElementAssembly, IfcElementAssemblyType, IfcElementAssemblyTypeEnum, IfcAssemblyPlaceEnum

**Purpose.** Group model elements into a unit that can be tagged, scheduled, filtered, counted and documented, and recognise repeated identical units as instances of one assembly type.

**Workflow.**
1. *Selection first:* select elements, then Modify | <category> › Create › *Create Assembly*. *Tool first:* Modify › Create › *Create Assembly* enters a pick mode with Add (default) and Remove; pick elements, then Finish.
2. The *New Assembly* dialog shows the **naming category** (a drop-down of the categories of the picked elements; the category of the most numerous members is preselected), the **type name**, the **IFC assembly type** (`PredefinedType`, default *User-defined*) and **Assembly place** (Not defined, Site, Factory).
3. Reviera checks whether an identical assembly type already exists (DOC-ASM-002). If one exists, the dialog says so, the type name is read-only and OK creates another instance of that type. Otherwise the type name is editable, prefilled with the naming category's last type name plus the next number (for example "Walls 004").
4. OK creates the assembly. The new type appears in the Project Browser under *Assemblies*, with its instances' count.
5. *Create Instance* (Project Browser › right-click an assembly type) places a copy of the type's members at a picked point, as a new instance of the same type.

**UI.** Ribbon: Modify | <category> › Create › Create Assembly; Modify | Assemblies tab with panels *Assembly* (Edit Assembly, Disassemble, Create Views, Acquire Views, Compare) and the usual Modify tools. Properties of an instance: Naming Category, Mark, Comments, Assembly Place, Phase Created, Phase Demolished. Type properties: Keynote, Model, Manufacturer, Type Comments, URL, Description, Assembly Code and Assembly Description, Type Mark, Cost, IFC assembly type. Project Browser node *Assemblies* › type › views and sheets (Chapter 1 §1.13).

**Behaviour & rules.**
- **Members.** Most model elements can be members: walls, floors, roofs, ceilings, columns, framing, family instances, parts, MEP segments and fittings, generic models, rebar and fabric. These cannot: annotation and view-owned drafting; elements of a linked model; groups (ungroup first); rooms, areas and spaces; masses; model lines; imported or linked CAD and images; elements in a design option other than the one being edited, and mixtures of main-model and option elements; analytical elements and structural loads; datums. The command lists the rejected picks with the reason and creates the assembly from the rest, or fails with `assembly.no_valid_members` when nothing is left.
- **One parent only.** An element can be a member of at most one assembly, because IFC allows an element to decompose only one whole. Picking a member of another assembly is refused (`assembly.already_member`), naming that assembly.
- **System aggregates as one member.** Elements that already decompose a system element of Chapter 3 or 4 (beam system beams, truss chords and webs, curtain wall panels and mullions, stair runs and landings, railing segments, rebar of a reinforcement system, fasteners of a fastener group) cannot be picked individually. The system element itself (beam system, truss, curtain wall, stair, railing, reinforcement system, connection) can be a member as a whole, which nests one `IfcElementAssembly` (or other aggregate) inside another. This is a deliberate extension of the reference product, which refuses such elements; IFC allows nested assemblies.
- **Parts.** `IfcBuildingElementPart` members (Chapter 3 parts) are already aggregated by their host. They can still be members: their membership is written as `IfcRelAssignsToProduct` from the part to the assembly (see Data), and RXD records it.
- **Ch 4 assemblies.** Precast piece assemblies (`PRECAST_ASSEMBLY`, STR-PRE-002) and steel shop assemblies (`STEEL_ASSEMBLY`, STR-STL-010 numbering) are assemblies in the sense of this section: they get types, views and sheets here. Their membership is maintained by the Chapter 4 tools; editing their members by hand (DOC-ASM-003) is allowed but marks them *modified by user*, and the next re-split or re-numbering asks before overwriting. For steel assemblies, the assembly mark from Chapter 4 numbering and the assembly type name are kept equal: Chapter 4 numbering writes both.
- **Naming category** is an instance property used only to name and match types (DOC-ASM-002). Changing it on an instance re-runs matching and may change the instance's type.
- **Assembly origin.** Each instance has an origin (a local coordinate frame). By default it is placed at the minimum corner of the members' bounding box, aligned with the main member's local axes (the first member of the naming category, by member order); for walls this puts X along the wall, Y across it and Z up. The origin orients the assembly views (DOC-ASM-004) and is the frame in which identical assemblies are compared.
- **Mark.** Instance *Mark* defaults to the next unused number within the type; a duplicate Mark within a type gives a warning but is allowed. *Type Mark* must be unique among assembly types; a duplicate gives a warning.
- **Level.** The assembly is contained in the storey of its lowest member. Each member keeps its own level reference in RXD, so its level-based parameters keep working and disassembling restores the original containment.
- **Schedules and filters.** Assemblies are a schedulable category (*Assemblies*), with fields from instance and type properties plus Count and member-derived quantities (sum of member volumes and areas, total weight from materials). Member schedules can show the assembly's type name and Mark through the related field *Assembly* (DOC-SCH-002). Filters and tags work on assemblies like any category.
- **Phasing and options.** An assembly's phase created is the latest phase created of its members unless set explicitly; members can still be demolished individually. An assembly belongs to the design option of its members.

**Data (IFC mapping).**
- Instance: `IfcElementAssembly` with `Name` = type name, `Tag` = Mark (for steel assemblies, the Chapter 4 assembly mark), `AssemblyPlace` from the instance property (`SITE`, `FACTORY` or `NOTDEFINED`), `ObjectPlacement` = the assembly origin (an `IfcLocalPlacement`), and `PredefinedType` left unset when the type sets it. Chapter 4 assemblies keep their own `PredefinedType` and `ObjectType` (`USERDEFINED` with "PRECAST_ASSEMBLY" or "STEEL_ASSEMBLY").
- Members: one `IfcRelAggregates` (RelatingObject = the assembly, RelatedObjects = the members). Members that already decompose another element (parts) are related by `IfcRelAssignsToProduct` instead. Members' own `IfcRelContainedInSpatialStructure` is removed while they are aggregated (IFC requires the whole, not its parts, to be contained) and restored on disassembly.
- Type: `IfcElementAssemblyType` related by `IfcRelDefinesByType`, `Name` = type name, `PredefinedType` = the chosen value of `IfcElementAssemblyTypeEnum` (default `USERDEFINED` with `ElementType` = the naming category name, as the schema requires). Identity data go to `Pset_ManufacturerTypeInformation` and the assembly code to `IfcClassificationReference` through `IfcRelAssociatesClassification`, as for other types (Chapter 3). The assembly has no own geometry; viewers draw its members.
- RXD: document `model/assemblies`, kind `assembly.type` (`ifcType` reference, `namingCategory`, `signature` (the matching hash of DOC-ASM-002), `viewOwner` (the instance that owns the views), `views[]`, `sheets[]`, `template?`) and kind `assembly.instance` (`ifc` reference, `namingCategory`, `memberOrder[]`, `assignedMembers[]` (parts related by `IfcRelAssignsToProduct`), `memberLevels{}`, `userModified` for Chapter 4 assemblies). Category key `cat.assemblies` (Chapter 2).

**Commands.**
`assembly.create(element_ids[], name?, naming_category?) -> assembly_id` (`name` is the type name, used only when the assembly is unique)
`assembly.place_instance(assembly_type_id, position, rotation?) -> assembly_id`
`assembly.set_naming_category(assembly_ids[], naming_category) -> ()`
`assembly.list(assembly_type_id?) -> assembly_summary[]` (query: id, type, Mark, member count, whether it owns the views)

**Acceptance criteria.**
- [ ] Selecting a wall and two windows in it and choosing *Create Assembly* with naming category Walls creates type "Walls 001" and one instance; the IFC file contains one `IfcElementAssembly` aggregating the three elements and typed by an `IfcElementAssemblyType`.
- [ ] Picking a room together with walls creates the assembly from the walls and reports the room as rejected.
- [ ] Picking a beam that belongs to a beam system is refused; picking the beam system itself adds it as one nested member.
- [ ] A part of a layered wall can be a member; the export relates it by `IfcRelAssignsToProduct` and the IFC file validates.
- [ ] Disassembling restores each member's original storey containment.

### Identical-assembly detection and comparison
**ID:** DOC-ASM-002 · **Priority:** P2 · **Ref:** web: About Assemblies, Create an Assembly, Work with an Assembly

**Purpose.** Decide reliably when two assemblies are the same product, so that they share one type and one set of drawings, and explain the difference when they are not.

**Behaviour & rules.**
- Two assemblies **match** when all of these hold:
  1. the same naming category;
  2. the same number of members, with the same category and type for each corresponding member;
  3. the same values of every parameter that affects geometry or material (dimensions, offsets, profile, layer structure, material, rebar shape and size), compared in display precision;
  4. each corresponding member occupies the same position and orientation relative to the assembly origin, within 1 mm and 0.01° (project setting *Assembly match tolerance*);
  5. nested members (system aggregates) match recursively by the same rules.
  Identity data (Mark, comments), phase and workset are not compared.
- **Correspondence** of members is found without relying on pick order: members are sorted by (category, type, position of their reference point in the origin frame), and nested assemblies by their own signature. The resulting order is stored as `memberOrder` and used for views and annotation mapping (DOC-ASM-005).
- Reviera keeps a **signature** per assembly: a hash of the data in rules 1–5, with positions rounded to the tolerance grid. Signatures are compared first; instances with equal signatures are confirmed with the full comparison (to rule out rounding at the grid edges).
- **When matching runs:** at creation; after any edit of an assembly's members or origin (DOC-ASM-003); after a change to a member that affects rules 2–4 (moving a member, changing its type or a geometric parameter); and after a change of naming category. Matching runs in the background and its result is applied as part of the edit that caused it.
- **Type changes after edits.** When an edit makes an instance unique, it becomes an instance of a new type named "<old type name> (2)" (editable). When an edit makes it identical to another existing type, it joins that type. A type that loses its last instance is deleted, with its views and sheets unless they move with their owner instance (DOC-ASM-004).
- **Steel and precast numbering** (Chapter 4) may use stricter or looser criteria for marks (for example ignoring holes). For those assemblies, Chapter 4 numbering decides the type, and the matching here only reports differences; it never re-types them.
- **Compare** (`assembly.compare`; Modify | Assemblies › *Compare*, then pick a second assembly) shows a report of why two assemblies are or are not identical: naming category, member count per category and type, unmatched members, parameter differences per corresponding member (name, value A, value B) and position differences above the tolerance (with the distance). Each line can be clicked to select the members in the view. The same report is returned as data to Lua and MCP.

**Data (RXD).** `assembly.instance.memberOrder[]` and `assembly.type.signature` in `model/assemblies`; the tolerance in `project/settings` › `assemblies.matchTolerance {length, angle}`.

**Commands.**
`assembly.compare(assembly_id_a, assembly_id_b) -> comparison` (query: `{identical, naming_category_equal, unmatched_members[], parameter_differences[], position_differences[]}`)
`assembly.find_identical(assembly_id) -> assembly_id[]` (query: instances of other types that would match if re-typed, which can only differ by an earlier tolerance setting or by Chapter 4 numbering)

**Acceptance criteria.**
- [ ] Two wall-and-window assemblies built separately from the same wall type, window type and window position become two instances of one type.
- [ ] The same two assemblies with different naming categories become two types.
- [ ] Moving the window in one instance by 50 mm makes that instance a new type, and `assembly.compare` reports the window's position difference of 50 mm.
- [ ] Moving it back makes the instance rejoin the original type, and the temporary type is deleted.
- [ ] Picking the same elements in a different order produces the same signature.

### Editing, disassembling and deleting assemblies
**ID:** DOC-ASM-003 · **Priority:** P2 · **Ref:** web: Work with an Assembly, Disassemble an Assembly, Delete an Assembly

**Purpose.** Change an assembly's membership and orientation, change its type, and take it apart.

**Workflow.**
1. Select an assembly (in a project view or an assembly view) › Modify | Assemblies › *Edit Assembly* (or double-click, depending on the user's double-click option, Chapter 1).
2. In edit mode, a floating panel offers *Add*, *Remove*, *Finish* and *Cancel*. Pick elements to add or remove. Elements created with any modelling tool while in edit mode join the assembly automatically.
3. The origin marker is shown; drag its arrows or rotate it with its controls to change the origin.
4. *Finish* applies the edit, re-runs matching (DOC-ASM-002) and reports any type change.
5. To change type: select one or more instances and pick another assembly type in the Type Selector; each instance's members are replaced by a copy of the target type's members placed at the instance's origin.
6. To edit one member without edit mode, Tab-select it inside the assembly and edit it normally.
7. *Disassemble* removes the assembly and leaves its members in the model. Delete removes the assembly *and its members*.

**Behaviour & rules.**
- Edit mode follows Conventions §2.1. Micro-operations: `asm.add(element)`, `asm.remove(element)`, `asm.create(element)` (an element created during the session), `asm.origin(transform)`; the replay function applies them to the assembly's state at the start of the session so an interrupted edit can be recovered after a crash (Chapter 2b, DM-LIVE-015). *Cancel* discards the session, including elements created in it.
- Removing the last member is refused; use Disassemble.
- The member rules of DOC-ASM-001 apply to *Add*.
- Changing the origin changes only the orientation of the assembly views and the comparison frame; members do not move. Because matching uses the origin frame, changing the origin of one instance usually makes it a new type; Reviera offers *Apply origin to all instances of the type*, which moves each instance's origin by the same relative transform and keeps the type.
- **Changing type** replaces members: the old members are deleted and new ones created, so any annotation on the old members in project views is orphaned (DOC-CORE-003), and the confirmation says how many annotations are affected. Members cannot be swapped this way when they are hosts of elements outside the assembly (for example a wall hosting a door that is not a member); the command refuses with the list.
- **Disassemble** deletes the `IfcElementAssembly` and its RXD instance object, restores the members' storey containment and their plain IFC relationships, and keeps their GlobalIds. If the instance owned the type's views (DOC-ASM-004) and other instances exist, ownership passes to the oldest remaining instance; if it was the last instance, the type and its views and sheets are deleted after a confirmation that lists them.
- **Delete** of an instance deletes its members (one undo step). Delete of an assembly type (Project Browser › right-click › Delete) deletes all its instances, their members, and the type's views and sheets, after a confirmation that counts each.
- In BIMserver mode, editing an assembly requires ownership of the assembly and of every member that changes (Chapter 6); *Disassemble* requires ownership of all members.

**Data (RXD).** Edit sessions are not stored; their result changes `IfcRelAggregates` / `IfcRelAssignsToProduct`, the placement and the RXD objects of DOC-ASM-001.

**Commands.**
`assembly.edit_members(assembly_id, add_ids[], remove_ids[]) -> assembly_type_id` (returns the instance's type after matching)
`assembly.set_origin(assembly_id, transform, apply_to_type?) -> ()`
`assembly.set_type(assembly_ids[], assembly_type_id) -> ()`
`assembly.disassemble(assembly_ids[]) -> element_id[]` (returns the freed members)
`assembly.delete_type(assembly_type_id) -> ()`
Deleting an instance uses `element.delete` (Chapter 2).

**Acceptance criteria.**
- [ ] Adding a shelf to one of three identical desk assemblies makes that one a new type; the other two keep the original type and its views.
- [ ] A crash during *Edit Assembly* after two adds is recovered on restart with both adds pending, ready to Finish or Cancel.
- [ ] Rotating the origin of one instance with *Apply origin to all instances of the type* keeps all instances in one type and rotates the type's elevation views.
- [ ] Disassembling the only instance of a type with views asks for confirmation and then deletes the type, its views and its sheet; Undo restores all of them.
- [ ] Deleting an instance deletes its members; disassembling keeps them with unchanged GlobalIds.

### Assembly views
**ID:** DOC-ASM-004 · **Priority:** P2 · **Ref:** web: Create Assembly Views and Sheets, Delete an Assembly, Work with an Assembly

**Purpose.** Create, in one step, the isolated drawings and lists needed to fabricate an assembly type.

**Workflow.**
1. Select an assembly instance › Modify | Assemblies › *Create Views*, or Project Browser › right-click an assembly type › *Create Assembly Views*.
2. The *Create Assembly Views* dialog lists the view kinds with a checkbox, a view template and an *Assign* / *Apply* choice each:
   - **3D Ortho**;
   - **Detail Section A** (horizontal cut through the middle of the assembly) and **Detail Section B** (vertical cut through the middle, along the origin's X axis);
   - **Elevations** Top, Bottom, Left, Right, Front and Back, looking at the six faces of the box around the assembly aligned with its origin;
   - **Part List** (a component schedule of the members);
   - **Material Takeoff** (a material takeoff of the members);
   - **Sheet** (with a title block type; DOC-ASM-005).
   It also sets one scale for all graphical views (default 1:10). Schedule templates offered are those whose category matches the members. The choices are remembered as the project default for next time.
3. OK creates the views in one undo step and lists them in the Project Browser under the assembly type.
4. *Acquire Views* (Modify | Assemblies) moves the type's views and sheets from their current owner instance to the selected instance of the same type.

**Behaviour & rules.**
- **Ownership.** Assembly views belong to the assembly **type** and are drawn from one **owner instance** of that type. Only one instance of a type owns views. *Create Views* is unavailable on an instance of a type whose views belong to another instance (the tooltip names it); Create from the Project Browser uses the oldest instance as owner. Because identical instances are identical in their origin frame, the owner's drawings document every instance of the type.
- **Isolation.** An assembly view shows only the owner instance's members (including nested members) and the view's own annotation and drafting; everything else is excluded at the candidate query (stage 1, DOC-HLR-001), before any graphics layer. Members of other assemblies, links and datums are never shown. Annotation in the view may reference only members.
- **Geometry.** Views are set up in the owner's origin frame: elevations and sections are orthographic and cropped to the member bounding box plus a margin (default 5 mm at paper scale on every side, so 50 mm at 1:10); detail sections cut through the box centre, with far clipping at the box; the 3D view is an isometric from the origin's (+X, −Y, +Z) octant with the section box off. Views can afterwards be edited like any view (crop, scale, rotation of the crop, extra annotation, templates).
- **Live update.** When the owner's members change, its views regenerate; when the assembly origin changes, views are re-aligned and their crop regions re-fitted unless the user has edited the crop (then only re-aligned). Annotation stays attached to members by reference.
- **Type changes.** Views go with their owner instance. If an edit moves the owner to another type that already has views, Reviera asks which set to keep (the reference product deletes one set without a choice); the other set is deleted after confirmation.
- **Extra views.** Sections, callouts and duplicates created inside an assembly view inherit its assembly relation and isolation. Assembly views can be placed on assembly sheets or on ordinary sheets, once each (DOC-CORE-005).
- **Schedules.** The part list and material takeoff are ordinary schedules (DOC-SCH-006) with a fixed filter *Assembly = this type's owner instance* that cannot be removed; their rows are the owner's members. Their *Count* column counts members of one instance; a *Quantity per type* field multiplies by the number of instances of the type.
- **Names.** Views are named "<type name> - 3D Ortho", "<type name> - Detail Section A", "<type name> - Top", "<type name> - Part List" and so on; a clash adds a numeric suffix. Renaming the type renames views whose names still start with the old type name.
- **Tags in assembly views** show the owner's values. Tags of instance parameters that differ between identical instances (Mark, Comments) may use the label option *List all instances of the type*, which shows the values of all instances separated by commas.

**Data (RXD).** Views are ordinary `views/<viewId>` documents (kinds `view.3d`, `view.section`, `view.elevation`, `view.detail`) with an `assembly` block: `type` (the `assembly.type` id), `owner` (`ifc:` reference of the owner instance), `role` (3d | section_a | section_b | top | bottom | left | right | front | back | extra), `cropFitted` (true until the user edits the crop). Schedules: `schedules/<id>` with `assembly` set. `assembly.type.views[]` lists them; the defaults of the dialog are in `project/settings` › `assemblies.viewDefaults`.

**Commands.**
`assembly.create_views(assembly_type_id, views[], scale?, templates?, sheet?: {title_block_type_id, number?, name?}) -> view_id[]` (`views[]` from `3d`, `section_a`, `section_b`, `top`, `bottom`, `left`, `right`, `front`, `back`, `part_list`, `material_takeoff`; `templates` maps a view kind to `{template_id, mode: assign|apply}`; when the type's views are owned by an instance other than the one the user chose, it fails with `assembly.views_owned_elsewhere`)
`assembly.acquire_views(assembly_id) -> ()`

**Acceptance criteria.**
- [ ] *Create Views* with 3D, two sections, six elevations, part list and material takeoff at 1:10 creates eleven views in one undo step, listed under the type in the Project Browser.
- [ ] The Front elevation of a precast wall piece shows only the piece, its rebar and its anchors, even where other walls stand in front of it in the model.
- [ ] Moving a lifting anchor in the owner instance updates the Front elevation and the part list.
- [ ] *Acquire Views* on a second instance of the type makes it the owner; deleting the first instance afterwards keeps the views.
- [ ] *Create Views* on a non-owner instance is unavailable, and the tooltip names the owner.

### Assembly sheets, documentation templates and propagation
**ID:** DOC-ASM-005 · **Priority:** P2 · **Ref:** research/04 §5.1 (assembly-specific sheet fields); web: Create Assembly Views and Sheets; Chapter 4 STR-PRE-006

**Purpose.** Produce one sheet per assembly type quickly and consistently, reuse a proven layout for other types, and carry views and annotation over to similar types.

**Workflow.**
1. Tick **Sheet** in *Create Assembly Views* (DOC-ASM-004) and choose a title block type; or Project Browser › right-click an assembly type › *Create Assembly Sheet*.
2. Open the sheet (listed under the assembly type) and drag the type's views onto it; drafting views, legends and other project views may be placed on it too.
3. To reuse the arrangement: on a finished assembly sheet, *Save as Assembly Template* (sheet context menu) stores the layout. Later, select assembly types or instances › *Apply Assembly Template*, choose the template, and Reviera creates the views and the sheet and places everything as in the template.
4. To carry views to a similar type: Project Browser › right-click the documented type › *Propagate Views…*, choose target types, and choose whether to include annotation.

**Behaviour & rules.**
- **Assembly sheets** are ordinary sheets (DOC-SHT-001) with an `assembly` link to their type. They are listed under the assembly type in the Project Browser and also under *Sheets* when *Show assembly sheets* is on in the browser organisation. The default sheet number is the type's *Type Mark* (or its name when Type Mark is empty), and the default sheet name is the type name; numbers follow the uniqueness rule of DOC-SHT-001, with a numeric suffix on a clash. Title block labels can show assembly type and instance parameters (type name, Type Mark, Assembly Code, Description, number of instances, Marks of all instances).
- One assembly type may have several assembly sheets. Deleting the type deletes them (DOC-ASM-003).
- **Identical instances** need no propagation: they share the type's views and sheets by definition (DOC-ASM-004). The sheet's *Instances* label lists their Marks so the drawing states which pieces it covers.
- **Assembly documentation templates** record: the view kinds and their templates, scale, crop margins, the sheet's title block type, each viewport's position (relative to the title block, by role), viewport types, schedule definitions for the part list and takeoff, and annotation *rules*: per-category tag types to place with *Tag All* in each view, and per-category dimension rules (overall dimensions, member-to-member dimension strings on faces of chosen categories, as in Chapter 4 shop drawings). Templates contain no element references, so they apply to any assembly type. Chapter 4 shop-drawing configuration (STR-PRE-006) chooses one of these templates per precast element kind.
- **Applying a template** to several types creates one set of views and one sheet per type; with *Group identical* (the default), identical instances share one type and therefore one sheet. Viewports whose view kind is not in the template are left out. Reviera reports viewports that did not fit within the title block's drawing area and moves them into it if *Fit to sheet* is on.
- **Propagate views** copies the source type's assembly views (definitions, crop, scale, templates, overrides) to each target type, and, if chosen, its annotation. Annotation is mapped through member correspondence (`memberOrder`, DOC-ASM-002): a tag or dimension whose references map to members of the same category and type in the target is copied; anything that cannot be mapped is skipped and listed in the report. Targets must have the same naming category; a target that already has views of the same role is skipped unless *Replace existing* is on. Propagation is one undo step.
- Assembly sheets print and export like any sheet; the PDF naming rule can use assembly parameters (DOC-OUT-003).

**Data (RXD).** Assembly sheets: `sheets/<sheetId>` with `assembly` (`assembly.type` id), listed in `assembly.type.sheets[]`. Documentation templates: RXD document `model/assemblies`, kind `assembly.template` (`name`, `views[] {role, template, mode, scale, cropMargin}`, `titleBlockType`, `viewports[] {role, position, viewportType}`, `schedules[]`, `tagRules[] {category, tagType, views[]}`, `dimensionRules[]`).

**Commands.**
`assembly.create_sheet(assembly_type_id, title_block_type_id?, number?, name?) -> sheet_id`
`assemblytemplate.save(sheet_id, name) -> template_id` (the sheet must be an assembly sheet)
`assembly.apply_template(assembly_ids_or_type_ids[], template_id, group_identical?=true, fit_to_sheet?=true) -> sheet_id[]`
`assembly.propagate_views(source_type_id, target_type_ids[], include_annotation?=true, replace_existing?=false) -> report`

**Acceptance criteria.**
- [ ] Ticking *Sheet* in *Create Assembly Views* creates a sheet numbered with the type's Type Mark and lists it under the type.
- [ ] Saving a template from a finished sheet and applying it to ten precast piece assemblies, four of them identical, creates seven sheets with the same viewport layout.
- [ ] Propagating views with annotation from type A to a type B that has one extra member copies all tags and dimensions of the shared members and reports none skipped; the extra member gets no annotation.
- [ ] The title block of an assembly sheet covering three identical instances lists the three Marks.

---

## 5.17 Output: print, export and rendering

All output commands work on an immutable **model snapshot** taken when the command starts, so they can run in the background (DOC-OUT-013) while the user keeps editing. All 2D output comes from the draw list (DOC-HLR-004). Output setups are project data stored in RXD document `project/settings` (section `output`), so they travel with the project and can be transferred to other projects (Chapter 6, COL-MNT-005).

### View/sheet sets and print options
**ID:** DOC-OUT-001 · **Priority:** P0 · **Ref:** research/04 §6.1, §6.2 *Options* · screenshots/documentation/sheet_collections_browser_print_set.png

**Purpose.** Choose what to output, and the options common to every 2D output.

**Behaviour & rules.**
- **Range:** current window; visible portion of current window; selected views/sheets. The *View/Sheet Set* dialog lists views and sheets with filters (sheets, views, view kind, sheet collection, name search) and saves the selection as a named set. Sets can also be rule-based (for example "all sheets in collection *DD Issue* with *Appears in sheet list* = yes"), so that new sheets join automatically.
- **Common options:** view links in blue (colour output only); hide reference/work planes; hide unreferenced view tags; hide scope boxes; hide crop boundaries; replace halftone with thin lines; region edges mask coincident lines; hide underlay; print orphans in warning colour.
- **Colour mode:** colour, black lines (all non-white lines and text black, fills kept as they are), greyscale.
- **Hidden-line processing:** vector or raster (DOC-HLR-007).

**Data (RXD).** `project/settings` › `output.viewSheetSets[]` (`{id, name, members[] | rule}`), `output.commonOptions`.

**Commands.** `viewset.save(name, view_and_sheet_ids[]?, rule?) -> set_id`; `viewset.delete(set_id) -> ()`; `viewset.resolve(set_id) -> view_or_sheet_id[]` (query)

**Acceptance criteria.**
- [ ] A rule-based set of collection *DD Issue* includes a sheet added to that collection after the set was saved.
- [ ] *Hide scope boxes* removes scope boxes from every output while they remain visible on screen.

### Print
**ID:** DOC-OUT-002 · **Priority:** P0 · **Ref:** research/04 §6.1

**Workflow.** File › Print (Ctrl+P). Print dialog: printer and its properties; *Print to file*; combine into one job or separate jobs; range (DOC-OUT-001); copies, reverse order, collate; *Settings…* (print setup); Preview; Print.

**Behaviour & rules.**
- **Print setup** (named, saved in the project): paper size and source, orientation, paper placement (centre, or offset from corner with x/y), zoom (fit to page, or a percentage; 100 % prints sheets at true size), raster quality, colour mode, options (DOC-OUT-001). Save, Save As, Revert, Rename, Delete.
- Printing goes through the platform print system: Reviera renders each page to a vector page description (the same renderer as PDF) and hands it to the printer driver through the wxWidgets printing framework (Windows GDI/XPS, macOS Quartz, CUPS on Linux).
- When a sheet is larger than the chosen paper and zoom is 100 %, the dialog warns that the output will be cropped.
- Preview shows exactly what will be sent, page by page.

**Data (RXD).** `output.printSetups[]`.

**Commands.** `print.run(range, set_id?, printer?, setup_id?, copies?, combine?) -> job_id`; `print.setup_save(name, settings) -> setup_id`; `print.setup_delete(setup_id) -> ()`; `print.preview(range, setup_id?) -> page_images[]` (query)

**Acceptance criteria.**
- [ ] An A1 sheet printed at 100 % on A1 paper measures within 0.5 mm of nominal size between title block corners.
- [ ] Printing ten sheets as one job sends ten pages in the order of the view/sheet set.

### PDF export
**ID:** DOC-OUT-003 · **Priority:** P0 · **Ref:** research/04 §6.2 · screenshots/documentation/pdf_export_dialog.png

**Purpose.** Native, printer-independent PDF output.

**Workflow.** File › Export › PDF (also on the Quick Access Toolbar). Dialog sections: Export range; Export setup (in-session or named; New, Duplicate, Rename, Delete, Save); File (combined file name, or separate files with a naming rule; location); Size (use sheet size, or a paper size; fit to page or zoom %); Paper placement (centre, or offset from corner with no margin / printer limit / user-defined x,y); Orientation (automatic, portrait, landscape); Appearance (raster quality, colour mode); Output (vector or raster processing); Options (DOC-OUT-001, plus *Export in background*, on by default). Export / Close.

**Behaviour & rules.**
- **Naming rule:** a file name pattern composed of parameters (sheet number, sheet name, project number, current revision, sheet collection, view name, custom parameters) with prefix, suffix and separators, and a live sample. Characters invalid in file names are replaced with `_`.
- **PDF content:** PDF 1.7 written by Reviera's own writer (built on the MIT/Apache `pdf-writer` crate, with font subsetting). Text is real, searchable text with embedded font subsets. Lines keep their exact widths, caps and dash patterns. Fills use PDF tiling patterns for hatches, or explicit geometry when the pattern is too complex for tiling.
- **Hyperlinks:** view reference symbols, section heads, callout heads and elevation marks on a sheet become internal links to the target sheet when it is in the same combined PDF.
- **Layers:** optional *PDF layers by category* writes optional content groups per category (and sub-category), so that a PDF viewer can toggle them.
- **Metadata:** title (sheet number and name), author, project name and number, and the Reviera version are written to the document information and XMP metadata. Optional PDF/A-2b output.
- Combined files get one bookmark per sheet or view.

**Data (RXD).** `output.pdfSetups[]`.

**Commands.** `export.pdf(range, set_id?, setup_id?, path_or_folder, combine?, naming_rule?, background?) -> job_id`; `export.pdf_setup_save(name, settings) -> setup_id`; `export.pdf_setup_delete(setup_id) -> ()`

**Acceptance criteria.**
- [ ] Text in an exported PDF can be searched and copied.
- [ ] Clicking a section head in a combined PDF jumps to the sheet holding that section.
- [ ] Separate-file export of 20 sheets with naming rule `<Project Number>-<Sheet Number>-<Current Revision>` creates 20 correctly named files.
- [ ] A PDF/A-2b export passes veraPDF validation.

### Raster image export
**ID:** DOC-OUT-004 · **Priority:** P1 · **Ref:** research/04 §6.5 *Images*

**Purpose.** Write views and sheets as raster images for presentations, web pages and documents.

**Workflow.** File › Export › Images and Animations › Image. The dialog has: output file or folder with naming rule; range (DOC-OUT-001); image size; options; format; Export.

**Behaviour & rules.**
- File › Export › Images and Animations › Image: range; image size (fit to N pixels in a direction, or zoom % of real size); options (as DOC-OUT-001); shaded-view resolution (72–600 dpi); format PNG, JPEG (quality levels), TIFF, BMP, TGA, WebP; naming rule as for PDF. Hidden-line views are rasterised from the draw list with anti-aliasing; shaded views are rendered off-screen by the GPU renderer.
- *Fit to N pixels*: N from 32 to 32 768 in the chosen direction (horizontal or vertical); the other dimension follows the view's aspect ratio. *Zoom %* (1–1000 %) of the printed size at the chosen dpi. Larger images are rendered in tiles (DOC-HLR-007).
- The extents are the crop region of a cropped view, the sheet size for a sheet, and the extents of visible content for an uncropped view; *Visible portion of current window* exports exactly what the window shows.
- Line widths keep their paper size at the image's dpi, so a 0.5 mm pen is 6 pixels wide at 300 dpi; *Thin lines* never applies.
- PNG and TIFF can have a transparent background (option); JPEG uses the view background colour. TIFF and PNG embed the dpi.
- Each image gets metadata: title (view or sheet name), software and project name, in the format's standard text fields.
- The command runs as a background job (DOC-OUT-013).

**Commands.** `export.image(range, set_id?, path_or_folder, format, size, dpi?, options?) -> job_id` (`size` is `{fit: horizontal|vertical, pixels}` or `{zoom_percent}`; out-of-range values fail with `export.image_size_invalid`)

**Acceptance criteria.**
- [ ] Exporting a plan to a 4000-pixel-wide PNG produces an image exactly 4000 pixels wide whose linework matches the PDF output.
- [ ] A PNG exported with a transparent background has alpha 0 outside drawn content.
- [ ] Requesting a 50 000-pixel image fails with `export.image_size_invalid`.

### DXF and DWG export
**ID:** DOC-OUT-005 · **Priority:** P1 (DXF), P2 (DWG) · **Ref:** research/04 §6.3

**Purpose.** Exchange 2D drawings (and simple 3D) with CAD users.

**Workflow.** File › Export › CAD Formats › DXF / DWG: choose an export setup (Modify Export Setup…), the range (current view/sheet or a view/sheet set), file naming (automatic long, automatic short, or manual), the file version, and whether views on sheets and links are exported as external references or bound.

**Behaviour & rules.** The export setup dialog has these tabs:
- **Layers:** per category and sub-category, a projection layer, a cut layer and colour numbers; layer modifiers (append category, sub-category, phase, workset, view kind and so on); *Export layer options*: properties by layer with overrides by entity, all by layer without overrides, or all by layer with new layers for overrides. Layer naming standards can be loaded from tables Reviera ships (ISO 13567 and other published conventions, written as Reviera's own tables).
- **Lines:** line scaling mode (model space, paper space, or scaled definitions); mapping of line patterns to linetype names, with Reviera generating the `.lin`-style definitions it needs.
- **Patterns:** mapping of fill patterns to hatch names; unmapped patterns are exported as explicit hatch definitions.
- **Text & fonts:** font mapping; text treatment (keep visual fidelity, or keep editability).
- **Colours:** indexed (255) or true colour.
- **Solids** (3D views): polyface meshes only. Proprietary solid-kernel formats are not written; STEP (DOC-OUT-007) is the solid exchange route.
- **Units & coordinates:** drawing unit; coordinate basis internal or shared (Chapter 6, COL-POS-001).
- **General:** room/space/area boundaries on separate layers; non-plottable layer prefix; hide scope boxes, reference planes and unreferenced tags; preserve coincident lines; default version.
- **Sheets** export to paper space layouts with each viewport as a model-space view at its scale (or flattened to paper space, by option).
- **Writers:** DXF is written by Reviera's own writer (DXF is a published text format). DWG is written through LibreDWG (GPL-3.0-or-later), whose DWG writing is limited; Reviera therefore writes DWG only in the versions LibreDWG supports reliably and offers DXF as the recommended format.
- Exact arcs, circles and ellipses from the draw list are written as true arc, circle and ellipse entities.

**Data (RXD).** `output.cadSetups[]`.

**Commands.** `export.dxf(range, set_id?, setup_id?, folder, naming?, version?, xrefs?) -> job_id`; `export.dwg(range, set_id?, setup_id?, folder, naming?, version?, xrefs?) -> job_id`; `export.cad_setup_save(name, settings) -> setup_id`; `export.cad_setup_load_layer_standard(setup_id, standard) -> ()`

**Acceptance criteria.**
- [ ] A DXF of a floor plan opens in LibreCAD and in another CAD application with walls on the configured cut layer and doors on their projection layer.
- [ ] A circular column is exported as a CIRCLE entity.
- [ ] Exporting a sheet produces one paper-space layout with a viewport at the view's scale.

### IFC export settings
**ID:** DOC-OUT-006 · **Priority:** P0 (save-as variants), P1 (setups, MVD conversion) · **Ref:** research/04 §6.4

**Purpose.** IFC is Reviera's native format, so "IFC export" means writing a *derived* IFC file tailored to a recipient: a different schema or model view definition, a filtered subset, or a clean file without Reviera data.

**Workflow.** File › Export › IFC: file name; setup (in-session, or named setups including built-in ones for IFC4 Reference View, IFC4 Design Transfer View, IFC2X3 Coordination View 2.0, IFC4X3, and a *Basic FM Handover* setup consistent with COBie export, DOC-OUT-015); *Modify setup…*; *Export linked models as separate files* or *federate into one file*.

**Behaviour & rules.** Setup tabs:
- **General:** schema (IFC2X3, IFC4, IFC4X3); model view definition (Reference View writes tessellated geometry such as `IfcTriangulatedFaceSet`; Design Transfer View keeps parametric geometry such as extrusions and Boolean results); file type (`.ifc`, `.ifcXML`, `.ifczip`); phase to export (sets the standard `Status` property, DOC-GFX-017); space boundaries (none, first level, second level); coordinate base (internal origin, project base point, shared coordinates; Chapter 6, COL-POS-003); split walls, columns and ducts by storey; file header (author, organisation, authorisation).
- **Content:** *Include Reviera Extension Data* (on by default; off produces a clean exchange file); *Include documentation as IFC annotation* (DOC-IFC-002) with *Include 2D linework*; export only elements visible in a chosen view; include rooms/areas/spaces as `IfcSpace` volumes; include linked models.
- **Properties:** standard property sets; Reviera parameters as user property sets (with a mapping table from parameter to property set and property name, loadable from a text file); base quantities (computed by OCCT); schedules as property sets (each schedule's fields become a property set on its elements); classification references.
- **Level of detail:** tessellation tolerance for curved geometry.
- **Advanced:** export parts as building elements; use type name only for type objects; store exported GlobalIds back (not needed in Reviera because GlobalIds are native, so this option is absent); export bounding boxes.
- **Geographic reference:** writes `IfcMapConversion` and `IfcProjectedCRS` from the project's positioning (Chapter 6, COL-POS-003).
- **Entity mapping:** a per-category table of IFC class and predefined type used when a category's native class should be exported differently (for example exporting generic models as `IfcBuildingElementProxy`); family types can override it with an *Export as* parameter.
- **Schema conversion:** IFC4 to IFC2X3 and IFC4 to IFC4X3 are done with IfcOpenShell's schema migration functions, followed by Reviera's own fix-ups for entities that have no counterpart. Losses are listed in an export report.
- **Validation:** after writing, the file is validated with IfcOpenShell (schema and, where available, MVD rules); problems are listed in the export report.

**Data (RXD).** `output.ifcSetups[]`.

**Commands.** `export.ifc(path, setup? (name, id or inline settings), settings_override?, schema?, strip_rxd?=false, documentation?, background?) -> job_id`; `export.ifc_setup_save(name, settings) -> setup_id`; `export.ifc_setup_delete(setup_id) -> ()`

**Acceptance criteria.**
- [ ] A Reference View export contains no `IfcExtrudedAreaSolid` or `IfcBooleanResult` and opens in at least two other IFC viewers.
- [ ] A clean export contains no Reviera Extension Data property sets and validates without errors.
- [ ] An IFC2X3 Coordination View export of the sample project validates against the IFC2X3 schema, and the report lists every converted or dropped entity.
- [ ] *Schedules as property sets* writes a property set named after the door schedule onto each door, containing that schedule's field values.

### 3D exchange formats: glTF, OBJ, STEP, FBX
**ID:** DOC-OUT-007 · **Priority:** P1 (glTF, OBJ), P2 (STEP), P3 (FBX) · **Ref:** research/04 §6.5 (*FBX*, *ACIS SAT / STEP*, *Datasmith*)

**Purpose.** Hand 3D geometry and materials to visualisation, game-engine and CAD tools.

**Behaviour & rules.**
- **glTF 2.0** (`.gltf` or binary `.glb`): the recommended visualisation format. Exports the visible contents of the active 3D view (respecting section box, visibility and phase), with physically based materials, texture maps, instancing for repeated family types, lights (`KHR_lights_punctual`), cameras, and per-node `extras` holding the element GlobalId, category and chosen parameters. Large coordinates are offset to a local origin stored in the asset's `extras`.
- **OBJ/MTL:** simple mesh exchange, one group per element.
- **STEP AP214/AP242:** solid exchange through OCCT's STEP writer, with one product per element, colours and names.
- **FBX:** the format's official software kit is proprietary and cannot be used in a GPL program. Reviera does not plan an FBX writer; glTF is the supported alternative. A community writer for the documented ASCII FBX form may be considered later as an add-in.
- Vendor live-link and proprietary visualisation formats are out of scope (P3). Open alternatives: glTF, USD (P2, through the Apache-licensed OpenUSD libraries, as a possible later addition).

**Commands.** `export.gltf(path, view_id?, binary?, options?) -> job_id`; `export.obj(path, view_id?, options?) -> job_id`; `export.step(path, view_id_or_element_ids, schema?: ap214|ap242) -> job_id`

**Acceptance criteria.**
- [ ] A glTF export of a 3D view opens in a standard glTF viewer with correct materials, and each node's `extras` holds its element's GlobalId.
- [ ] Repeated chairs are written as one mesh with many instances.
- [ ] A STEP export reopens in a STEP viewer with one solid per element.

### gbXML export
**ID:** DOC-OUT-008 · **Priority:** P2 · **Ref:** research/04 §6.5 *gbXML*

**Purpose.** Hand an energy model to open or third-party energy and load analysis tools.

**Workflow.** File › Export › gbXML: choose the source (rooms, spaces or the energy analytical model), the phase, and options; Export. A preview lists spaces and surfaces with warnings before writing.

**Behaviour & rules.**
- Exports an energy model in gbXML, built either from rooms/spaces or from the analytical energy model of Chapter 4 (ANA). Spaces become `Space` elements; their bounding surfaces (computed from second-level space boundaries by OCCT) become `Surface` elements with openings, construction and adjacency; location comes from the project's site location. The export report lists surfaces that could not be classified.
- Surface types are classified from the host element and adjacency: exterior wall, interior wall, roof, interior floor, slab on grade, underground wall and slab, ceiling, shade, air (room separation lines), with openings typed as operable window, fixed window, door or opening.
- Constructions and materials are written from the host types' layers with thickness and thermal properties when present (Chapter 2 materials); missing thermal data is reported, not invented.
- Options: export complexity (simple, or with shading surfaces), include unbounded rooms (default off), building type and space types (written as gbXML enumerations), and the coordinate base (internal or shared, Chapter 6).
- Rooms that are not enclosed are skipped and reported. Spaces whose surfaces do not close within the tolerance (5 mm) are written but flagged in the report.
- Units are SI (metres, square metres, cubic metres) unless the project's display units are imperial and the option *Use project units* is on.

**Commands.** `export.gbxml(path, source: rooms|spaces|energy_model, options?) -> job_id`

**Acceptance criteria.**
- [ ] The gbXML of a two-room sample validates against the gbXML schema, and the shared wall appears once with both spaces as adjacent.
- [ ] A room that is not enclosed is listed in the report and not written.
- [ ] A wall type without thermal data is exported with its layer thicknesses and named in the report as missing thermal properties.

### Reports and other exports
**ID:** DOC-OUT-009 · **Priority:** P1 (schedule and room reports), P2 (database export), P3 (vendor formats) · **Ref:** research/04 §6.5

| Output | Reviera behaviour | Priority |
|---|---|---|
| Schedule export | DOC-SCH-012. | P0–P1 |
| Room/area report | HTML report listing each room or area with its boundary triangulation and computed area, for checking area calculations. | P2 |
| Database export | Exports elements, types and parameters to SQLite (one table per category plus a parameters table) or to CSV files. This replaces export to proprietary ODBC targets; SQLite files can be attached to other databases. | P2 |
| Family type catalogue | Exports a family's types and parameter values as a delimited text catalogue (Chapter 2). | P1 |
| DWF/DWFx, DGN, NWC, Datasmith | Proprietary or vendor-specific formats; not provided. Alternatives: PDF for review sets, DXF for CAD exchange, IFC plus BCF for coordination, glTF for visualisation. | P3 |

**Commands.** `export.database(path, format: sqlite|csv, categories[]?) -> job_id`; `export.room_report(path, scope?) -> job_id`

**Acceptance criteria.**
- [ ] A SQLite export contains one row per door in the Doors table, with one column per scheduled door parameter.

### COBie export
**ID:** DOC-OUT-015 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); public standard NBIMS-US V3 §4.2 COBie (nibs.org, S0702); buildingSMART Basic FM Handover view (DOC-OUT-006)

**Purpose.** Hand over the model's facility-management metadata as a COBie (Construction Operations Building information exchange) data set: the spaces, levels, types, components, systems and their attributes, in the tables and columns an owner's maintenance systems import.

**Workflow.**
1. File › Export › COBie (also Manage › Settings › COBie Setup for the stored settings).
2. The dialog has these parts:
   - **Version:** *COBie (NBIMS-US current)*, the default, or *COBie 2.4 (legacy layout)*.
   - **Format:** *Workbook (.xlsx)*, the SpreadsheetML deliverable, or *One delimited file per table*.
   - **Contacts:** the company and person records, with the one that created the data set. They are kept with the project.
   - **Component categories:** the categories whose elements are maintainable assets. Defaults: doors, windows, furniture, casework, specialty equipment, plumbing fixtures, mechanical and electrical equipment, lighting fixtures, air terminals, fire protection and communication devices.
   - **Classification system** for the Category columns (for example OmniClass or Uniclass), read from the elements' classification references (Chapter 2).
   - **Zones** from: room Department, an IFC zone (`IfcZone`), or a chosen room parameter.
   - **Attributes:** all instance and type parameters, the scheduled ones only, or a chosen list.
3. **Validate** checks the data set without writing it (see Validation). **Export** writes it and opens the report.

**Behaviour & rules.**
- **Table mapping (current version).**

| COBie table | Source in the model |
|---|---|
| Instruction | Export date and time, units, classification systems, Reviera and the COBie version |
| Company | The contacts of the COBie settings |
| Facility | Project and building: names, project number, site, units, phase, `ExtIdentifier` = the building's GlobalId |
| Level | Levels (`IfcBuildingStorey`): name, elevation, height to the next level; the site as a level of category "Site" when spaces are placed on it |
| SpaceType | The distinct values of the room parameter chosen as space type (default: Occupancy) |
| Space | Placed rooms (`IfcSpace`): Number and Name, level, space type, net area, net height, and room finishes, read from the room's computed geometry (ARC-ROOM-002) |
| Zone | One record per zone value and room, or per `IfcZone` membership |
| Type | Element types of the component categories: name, category (classification), manufacturer, model, warranty and replacement-cost parameters where present |
| Component | Elements of the component categories: name (Mark, else a generated unique name), type, the space that contains them (for doors, the From/To room), serial and tag numbers, installation date |
| System | MEP systems (`IfcSystem`, Chapter 4) and their components |
| Document | Documents linked to elements (DM document links), as file references |
| Attribute | One record per exported parameter of every Facility, Level, Space, Type and Component record: name, value, unit, category, and the sheet and row it describes |
| Coordinate | Optional: the location point (and bounding box) of each component and space |
| PickList | The pick-list values the data set uses (categories, units, asset types) |
| Resource, Job, Event, Package, Risk | Written with headers and no rows, unless the project has operations data in RXD for them (not specified yet) |

- **COBie 2.4 (legacy layout)** writes the same data under the earlier names and columns: Contact (for Company), Floor (for Level) and the 2.4 worksheet order. It has Assembly, Connection, Spare, Impact and Issue sheets with headers only. SpaceType values go to the Space sheet's Category column.
- **Identity columns.** Every record carries `ExtSystem` = "BIM Reviera <version>", `ExtObject` = the IFC class, and `ExtIdentifier` = the element's IFC GlobalId. A data set exported twice from an unchanged model is identical except for the Instruction table's date, so re-exports can be compared.
- **Names.** COBie requires a unique Name within each table. Duplicates are reported by validation. With *Make names unique* on, the export suffixes them (" (2)", " (3)") and lists every renamed record in the report; the model is not changed.
- **Units.** Values are written in the project's units, with the unit names in the Instruction table and the PickList.
- **Required and optional data.** Columns that COBie requires but the model has no value for are written as "n/a". The export never invents values. The report lists each "n/a" by table and column, so the team can fill them in (typically through a schedule, DOC-SCH-010).
- **Validation** (`cobie.validate`) checks every rule below, before writing or on demand, and reports each problem with the element it comes from (selectable in the canvas).
  - Required columns are present.
  - Names are unique within each table.
  - References resolve: every Component's Space and Type exists, and every Space's Level exists.
  - Components are in a space.
  - Values belong to their pick lists.
  - Types have components.
- **Consistency with IFC.** The *Basic FM Handover* IFC export setup (DOC-OUT-006) writes the same spaces, zones, systems, types, classifications and properties. A COBie data set and an IFC file exported from one model state describe the same objects with the same GlobalIds.
- **Performance and safety.** The export runs as a background job (DOC-OUT-013) on a snapshot of the model. Text values are escaped as for schedule export (DOC-SCH-012), so no cell can run as a formula.

**Implementation notes (2026-10-13).**
- As built:
  - The COBie 2.4 layout, as an XLSX workbook (`rust_xlsxwriter`, MIT/Apache, all cells text) or a CSV per table.
  - `export.cobie(path, version?: v2_4|current, format?: xlsx|csv, overwrite?)`, `cobie.validate()` and `cobie.set_settings(settings)`.
  - The settings are stored on an RXD-only element `rxd:project/cobie`.
  - Values are written in SI units (meters, square meters, cubic meters), as the Facility sheet states, rather than in project units.
- `version: current` is refused until its column order is confirmed against the published standard. The public NIBS page gives the tables but not the column order, and its Annex A was unreachable.
- Components are named by their Mark, which is numbered per category. Doors and windows can therefore share a name, which validation reports.
- Systems, IFC zones, documents, coordinates and the File › Export dialog's options (version, format, categories, attribute selection) come later; the window asks only for the contact.

**Data (RXD).** `project/cobie`: the settings (version, format, contacts, component categories, classification system, zone source, attribute selection, *Make names unique*) and the date of the last export.

**Commands.**
- `export.cobie(path, version?: current|v2_4, format?: xlsx|csv, options?) -> {tables: {name, rows}[], warnings[], path}`
- `cobie.validate(version?) -> issues[]` (each `{table, column?, record, element_id?, rule, message}`; query)
- `cobie.set_settings(settings) -> ()` (undoable; part of the project)

**Acceptance criteria.**
- [ ] On a model with two levels, six rooms, eight doors and four windows, the current-version workbook has Facility 1 row, Level 2, Space 6, Type and Component rows for the doors and windows, and Attribute rows for their parameters. Every Component row names its Space and Type, and every ExtIdentifier is the element's IFC GlobalId.
- [ ] Two doors with the same Mark are reported by validation. With *Make names unique* they export as "D1" and "D1 (2)", and the report lists the rename.
- [ ] The COBie 2.4 layout writes the same records on Contact, Floor, Space, Type and Component sheets, in the 2.4 sheet order.
- [ ] Exporting an unchanged model twice gives files that differ only in the export date.
- [ ] A room name starting with "=" is written as text.
- [ ] Every element in the COBie Component table is present, with the same GlobalId, in a Basic FM Handover IFC export of the same model state.

### Background jobs
**ID:** DOC-OUT-013 · **Priority:** P1 · **Ref:** research/04 §6.2 *Export in background*

**Behaviour & rules.**
- Output commands return a job id at once and run on a background worker using the model snapshot taken at start. The user keeps editing; later edits do not affect the job.
- A jobs panel and a status-bar indicator show progress and completion; a notification offers *Open file* or *Open folder*. Jobs can be cancelled.
- Jobs started from Lua or MCP can be awaited (`job.wait`) or polled (`job.status`).
- Several jobs can run at once, limited by a user setting (default: half the CPU cores).
- **Job states:** queued, running (with percent and the current item, for example "Sheet A205"), succeeded, failed (with the error and the item it occurred on), cancelled. A job that fails on one item continues with the others when the output allows it (separate files) and reports the failures; a combined file fails as a whole.
- **Snapshot cost.** The snapshot is copy-on-write (Chapter 2b), so starting a job does not copy the model; memory grows only with edits made while the job runs.
- **Files.** Output is written to a temporary name in the target folder and renamed on success, so a cancelled or failed job never leaves a half-written file under the final name.
- **Closing the project** while jobs run asks whether to wait or cancel them. Jobs never outlive the session; the jobs panel lists the last 50 finished jobs of the session.
- Jobs started by one user in BIMserver mode run locally on that user's machine; they do not lock anything on the server.

**Commands.** `job.status(job_id) -> status` (query; `{state, percent, current_item?, errors[], outputs[]}`); `job.wait(job_id, timeout?) -> status` (query; returns on completion or after `timeout` seconds with the current status); `job.cancel(job_id) -> ()`; `job.list() -> job_summary[]` (query)

**Acceptance criteria.**
- [ ] While a 200-sheet PDF export runs, the user can move walls with no noticeable delay, and the exported sheets show the model as it was when the export started.
- [ ] Cancelling a separate-file PDF export after 10 of 20 sheets leaves 10 complete files and no partial file.
- [ ] `job.wait` with a 1-second timeout on a long job returns a running status with a percentage.

### Rendering
**ID:** DOC-OUT-011 · **Priority:** P2 · **Ref:** research/04 §6.6; web: Place a Decal in a View, Decal Type Properties, Decal Attributes · screenshots/documentation/rendered_image_raytracer.png

**Purpose.** Photorealistic still images from 3D views, using open-source renderers.

**Workflow.**
1. View tab › Presentation › Render (default shortcut RR) with a 3D view active.
2. Render dialog: renderer (from those installed); *Render* or *Render region* (drag a rectangle in the crop); quality preset (draft, low, medium, high, best) or custom (samples, or render time in minutes, or until stopped); output resolution (screen, or printer dpi with width and height); lighting scheme, sun setting and artificial light groups; background (sky model, colour, image, transparent) and haze; exposure (exposure value, highlights, mid-tones, shadows, white point, saturation).
3. While rendering, the image refines progressively; the user can stop at any time.
4. *Save to project* creates a rendering view (placeable on a sheet); *Export* writes PNG, JPEG, TIFF, or OpenEXR (high dynamic range).

**Behaviour & rules.**
- **Renderer back-ends** are render backends of kind `offline_still`/`offline_animation` (DOC-RBE-001). In-process ones are plug-ins behind the backend trait (scene in, image tiles out); external ones (for example a Blender bridge) attach through the Reviera Scene Protocol (ADV-RSP-001) and return images with `render.result`:
  - **Reviera path tracer** (built in, optional at build time): a progressive unidirectional path tracer in `wgpu` compute shaders, with a BVH built on the GPU, multiple importance sampling, and the same materials as the Realistic style. It also drives the *Path Traced* visual style (DOC-GFX-011).
  - **Cycles** (Apache-2.0), used as the standalone library, as an external render engine with CPU and GPU devices.
  - Others that fit the trait with GPL-compatible licences may be added, for example LuxCoreRender (Apache-2.0) or Mitsuba 3 (BSD-3-Clause).
- **Scene translation:** tessellated geometry from the OCCT cache, with instancing; materials mapped from Reviera appearance data (base colour, metallic, roughness, transmission, IOR, emission, normal and bump maps) to a principled surface model; artificial lights from lighting fixture families with IES LM-63 photometric files and light groups (with dimming); sun position from the project location, date and time; physical sky model.
- **Denoising:** optional Intel Open Image Denoise (Apache-2.0).
- People and plant entourage use ordinary families with mesh geometry; no proprietary entourage formats.
- Vendor cloud rendering services are out of scope (P3).
- **Decals** (Insert › Link › Decal ▾ › *Place Decal* / *Decal Types*, `decal.place`). A decal is an image applied to a planar or cylindrical face, for signs, posters, paintings and screens.
  - *Decal types* hold the image (PNG, JPEG, BMP, or WebP, stored as an RXD asset) and its appearance: brightness (multiplier, default 1.0), reflectivity (0–1), transparency (0–1), finish (matte, satin, glossy, polished), luminance (cd/m², for screens and light boxes), bump pattern and amount (0–1), and cut-outs (none, rectangle, oval, or an image mask). The Decal Types dialog lists types with Duplicate, Rename, Delete (a type in use asks for a replacement) and a preview. Identity data (keynote, model, manufacturer, description, type mark, cost) are ordinary type parameters.
  - *Placement:* in a 2D view or an orthographic 3D view (not a perspective view), choose a type, set *Width* and *Height* (with *Lock proportions*, default on), then click a face. Elliptical faces and non-cylindrical curved faces are refused. The decal follows its host face when the host moves or changes size, keeping its position relative to the face's origin; it is deleted when the host is deleted.
  - *Display:* in Realistic, Path Traced and rendered output the image is mapped onto the face with the type's attributes; in all other styles the decal shows as a placeholder rectangle with a cross, which can be hidden with the *Decals* category.
  - Decals are taggable, schedulable (category *Decals*) and exported to glTF as textured quads offset 0.5 mm from the face (DOC-OUT-007).
- **Render gallery** (`render.gallery`; View tab › Presentation › Render Gallery). A local browser of this user's render history for the open project: every finished or stopped render job is kept with its image (stored as 32-bit floating-point OpenEXR so exposure can be changed later), a thumbnail, the source view, renderer, settings, resolution, samples and render time. The gallery lists them newest first with filters by view and renderer, and offers: open full size, compare two side by side with a slider, *Adjust exposure* (re-tone-maps the stored image without rendering again), *Render again with these settings*, *Save to project* (`render.save_to_project`), *Export* (`render.export`), and delete. History is stored per user in the local application data folder, keyed by the project's GlobalId, with a size limit (default 2 GB; oldest entries are removed first). It is never written into the project, never shared through BIMserver and never uploaded anywhere; only renderings saved to the project become project data.

**Data (RXD).** Render settings in the 3D view payload (`render` block). Saved renderings are views of kind `view.rendering` whose image is an RXD asset; the image is the only derived result stored (§5.1). Decal types: `standards/materials`, kind `decal.type` (`image` asset reference and the attributes above). Placed decals are element-scoped RXD on the host element (`Reviera_Element` section `decals[]`: `id`, `type`, `face` sub-reference, `uv` position, `width`, `height`, `rotation`), so moving or deleting the host carries them; they have no IFC entity of their own. On IFC export the host's face style is not changed; with *Include Reviera Extension Data* off, decals are lost and the export report says so.

**Commands.** `render.start(view_id, settings, renderer?, region?) -> job_id`; `render.stop(job_id) -> ()`; `render.save_to_project(job_id, name) -> view_id`; `render.export(job_id_or_view_id, path, format) -> ()`; `render.list_renderers() -> renderer_info[]` (query); `render.gallery() -> ()` (opens the gallery; session UI, not undoable); `decal.place(view_id, decal_type_id, face_ref, position, width?, height?) -> element_id` (returns the decal's id; a perspective view fails with `decal.perspective_view`, an unsupported face with `decal.face_unsupported`); `decaltype.create(name, image_asset_or_path, attributes?) -> type_id`

**Acceptance criteria.**
- [ ] With no renderer plug-in installed, the Render command explains how to install one and nothing crashes.
- [ ] A rendering saved to the project can be placed on a sheet and printed.
- [ ] Rendering the sample interior with Cycles and with the built-in path tracer gives images whose mean luminance differs by less than 10 % at equal exposure.
- [ ] A decal placed on a wall moves with the wall, shows as a crossed rectangle in Shaded style and as the image in a rendering.
- [ ] Placing a decal in a perspective camera view is refused.
- [ ] After three renders of one view, the render gallery lists three entries; adjusting the exposure of one does not start a render, and none of them is in the saved IFC file.

### Walkthrough and solar study animation export
**ID:** DOC-OUT-012 · **Priority:** P2 · **Ref:** research/04 §1.5 *Walkthrough*, §6.5 *Walkthrough / Solar Study*

**Purpose.** Export moving images of walkthroughs and of sun and shadow studies.

**Workflow.**
1. Solar study: View Control Bar › Sun Path ▾ › Solar Study, or `sun.settings` (DOC-GFX-012), choose Single Day or Multi-Day, dates, times and interval; preview with the play controls on the contextual tab.
2. File › Export › Images and Animations › Walkthrough or Solar Study (the active view must be a walkthrough, or a 3D or plan view with a solar study set): frame range, frames per second, visual style, size, output format; Export.

**Behaviour & rules.**
- File › Export › Images and Animations › Walkthrough / Solar Study: frame range, frames per second, visual style (any style, including a renderer), image size; output as an image sequence (PNG) or a video file (WebM with VP9/AV1, or MP4 with AV1, through a GPL-compatible encoder; codecs with patent licensing are not bundled). Solar studies animate the sun over a day or several days at a chosen interval.
- Frame range defaults to all frames; fps 1–60 (default 15 for solar studies, the walkthrough's own rate for walkthroughs); size by width and height in pixels (16–7680) or by zoom of the view size.
- A solar study frame count is the number of intervals in the time range (single day) or in the date range (multi-day); the sun position per frame comes from the project location (Chapter 6).
- With a renderer style, each frame is a render job with the view's render settings; the export can resume from the last finished frame after a cancel when writing an image sequence.
- Image sequences are named `<base>_0001.png` onward with zero padding to the frame count's digits.
- The export is a background job (DOC-OUT-013) and uses the model snapshot at its start.

**Data (RXD).** Solar study settings are part of the view's `graphicDisplay.lighting.sun` block (DOC-GFX-012); export settings are not stored unless saved as a named animation setup in `project/settings` › `output.animationSetups[]`.

**Commands.** `export.animation(view_id, kind: walkthrough|solar_study, path, frames?, fps?, size, style?, format: images|webm|mp4) -> job_id`; `solarstudy.set(view_id, kind: still|single_day|multi_day, date_range, time_range, interval) -> ()`

**Acceptance criteria.**
- [ ] A 100-frame walkthrough exports 100 numbered PNG files at the chosen size.
- [ ] A single-day solar study from 08:00 to 18:00 at one-hour intervals exports 11 frames whose shadows move from west to east.
- [ ] A WebM export plays in a standard browser at the chosen frame rate.

### Publishing for review
**ID:** DOC-OUT-014 · **Priority:** P3 (vendor cloud publishing), P2 (open alternative) · **Ref:** research/04 §6.7

**Purpose.** Share a reviewable snapshot of the drawings and model without a vendor cloud.

**Workflow.** File › Publish › Review Set: choose a view/sheet set (DOC-OUT-001), the 3D views to include, the 3D format (glTF or IFC), the target (folder, or the current BIMserver project in BIMserver mode), and the PDF setup; Publish.

**Behaviour & rules.**
- Publishing to a vendor cloud document service is out of scope. Open alternative: *Publish review set* writes a chosen view/sheet set as a combined PDF plus a glTF (or IFC) of chosen 3D views, and, in BIMserver mode, attaches them to the current BIMserver revision as revision extended data (Chapter 6; call names to verify against the deployed server), so that an open-source web viewer pointed at the server can show them. Issues raised on the published set travel as BCF (Chapter 6, COL-BCF-001).
- A review set folder contains `<set name>.pdf`, one `<view name>.glb` (or `.ifc`) per 3D view, and a `manifest.json` listing every file with its SHA-256, the project name and number, the source revision (local save time or BIMserver revision id), the publishing user and the time. The manifest lets a reviewer check that files belong together and are unchanged.
- The 3D files keep element GlobalIds (glTF `extras`, IFC natively), so BCF issues raised against them point at the right elements in Reviera.
- In BIMserver mode, publishing requires write access to the project; without it, the command fails before writing anything. The upload is one job; partial uploads are rolled back where the server allows it and reported otherwise.
- Publishing never changes the model; it is not undoable and does not create a project revision on its own.

**Commands.** `publish.review_set(set_id, include_3d_views[]?, target: folder|bimserver, path?) -> job_id`

**Acceptance criteria.**
- [ ] Publishing a review set in Local mode writes a combined PDF and one glTF per selected 3D view to the chosen folder.
- [ ] The manifest's SHA-256 values match the written files.
- [ ] A user without write access in BIMserver mode gets an error before any file is uploaded.

---

## 5.18 Command catalog

Every command below is registered once in the command registry and is therefore available from the UI, from Lua (`reviera.<domain>.<verb>{...}`) and as an MCP tool. "Undoable = No" marks queries, session-only display state, and output commands that do not change the model (output setups themselves are undoable project data). Parameters marked `?` are optional.

| Command | Parameters | Returns | Undoable | Req ID |
|---|---|---|---|---|
| `view.set_property` | view_id, name, value | () | Yes | DOC-VIEW-001 |
| `view.rename` | view_id, new_name | () | Yes | DOC-VIEW-001 |
| `view.delete` | view_ids[] | () | Yes | DOC-VIEW-001 |
| `view.open` | view_id | () | No | DOC-VIEW-001 |
| `view.annotations_3d` | view_id | {shown, occlude, classes[], sources[]} | No | DOC-ANN-025 |
| `view.get` | view_id | view_detail | No | DOC-VIEW-001 |
| `view.set_3d_annotations` | view_id, shown?, occlude?, classes?, sources? | () | Yes | DOC-ANN-025 |
| `view.list` | filter? | view_summary[] | No | DOC-VIEW-001 |
| `viewtype.create` | kind, name, default_template_id?, bind_new_views?, marker_type_id? | view_family_type_id | Yes | DOC-VIEW-001 |
| `view_tag.settings` | kind?: callout\|elevation\|section | () | No | DOC-VIEW-001 |
| `browser.set_organization` | target: views\|sheets, scheme_id | () | Yes | DOC-VIEW-001 |
| `browser.create_organization` | name, target, group_by[], sort_by | scheme_id | Yes | DOC-VIEW-001 |
| `view.find_referring` | view_id | referrer[] | No | DOC-VIEW-001 |
| `view.create_plan` | kind, level_ids[], view_family_type_id?, scale?, allow_duplicates?, area_scheme_id? | view_id[] | Yes | DOC-VIEW-002 |
| `view.set_view_range` | view_id, top, cut, bottom, depth | () | Yes | DOC-VR-001 |
| `view.get_view_range` | view_id | {planes, resolved_elevations} | No | DOC-VR-001 |
| `category.set_view_range_behaviour` | category, cuttable?, above_cut?: hide\|projection\|overhead, floor_like? | () | Yes | DOC-VR-002 |
| `settings.set_floor_tolerance` | length | () | Yes | DOC-VR-002 |
| `view.classify_element` | view_id, element_id | zone_report | No | DOC-VR-002 |
| `view.create_plan_region` | view_id, loops[], view_range | element_id | Yes | DOC-VR-003 |
| `planregion.set_view_range` | region_id, top, cut, bottom, depth | () | Yes | DOC-VR-003 |
| `view.set_underlay` | view_id, base_level_id?, top_level_id?, orientation | () | Yes | DOC-VR-004 |
| `view.set_far_clip` | view_id, mode, offset? | () | Yes | DOC-VR-005 |
| `view.set_depth_clipping` | view_id, mode | () | Yes | DOC-VR-005 |
| `view.create_elevation` | host_view_id, point, elevation_type_id, directions[], rotation? | {marker_id, view_ids[]} | Yes | DOC-VIEW-003 |
| `elevationmarker.set_directions` | marker_id, directions[] | view_ids[] | Yes | DOC-VIEW-003 |
| `view.create_framing_elevation` | host_view_id, datum_id, elevation_type_id, bay? | view_id | Yes | DOC-VIEW-003 |
| `view.create_section` | host_view_id, start, end, section_type_id, width?, far_offset?, reference_view_id? | view_id | Yes | DOC-VIEW-004 |
| `section.split_segment` | view_id, point, offset | () | Yes | DOC-VIEW-004 |
| `section.set_gap_mode` | view_id, mode | () | Yes | DOC-VIEW-004 |
| `section.flip` | view_id | () | Yes | DOC-VIEW-004 |
| `view.set_reference_label` | view_id_or_marker_id, label? | () | Yes | DOC-VIEW-004 |
| `view.create_callout` | parent_view_id, boundary, callout_type_id, reference_view_id? | view_id | Yes | DOC-VIEW-005 |
| `callout.set_head` | view_id, head_point, leader_points[] | () | Yes | DOC-VIEW-005 |
| `view.create_3d` | kind: default\|named, name? | view_id | Yes | DOC-VIEW-006 |
| `view.set_camera` | view_id, eye, target, up, projection?: orthographic\|perspective, fov? | () | Yes (explicit calls; interactive navigation is not, §1.19) | DOC-VIEW-006 |
| `view.lock_3d` | view_id, locked, save_as_name? | view_id | Yes | DOC-VIEW-006 |
| `view.restore_orientation` | view_id | () | Yes | DOC-VIEW-006 |
| `view.orient_to` | view_id, source_view_id_or_direction | () | Yes | DOC-VIEW-006 |
| `view.displace_elements` | view_id, element_ids[], offset, displacement_set_id? | displacement_set_id | Yes | DOC-VIEW-012 |
| `displacement.edit` | displacement_set_id, add_ids[], remove_ids[] | () | Yes | DOC-VIEW-012 |
| `displacement.set_offset` | displacement_set_id, offset | () | Yes | DOC-VIEW-012 |
| `displacement.add_path` | displacement_set_id, edge_ref, style?: straight\|jogged | annotation_id | Yes | DOC-VIEW-012 |
| `displacement.set_path_style` | annotation_ids[], style: straight\|jogged, line_style_id? | () | Yes | DOC-VIEW-012 |
| `displacement.reset` | displacement_set_ids[] | () | Yes | DOC-VIEW-012 |
| `view.highlight_displacement` | view_id, on | () | No | DOC-VIEW-012 |
| `view.create_camera` | host_view_id, eye, target, eye_elevation?, target_elevation? | view_id | Yes | DOC-VIEW-007 |
| `view.size_crop` | view_id, width, height, proportional? | () | Yes | DOC-VIEW-007 |
| `view.create_walkthrough` | host_view_id, keyframes[] | view_id | Yes | DOC-VIEW-007 |
| `walkthrough.set_keyframe` | view_id, index, eye?, target?, time? | () | Yes | DOC-VIEW-007 |
| `walkthrough.set_timing` | view_id, total_frames, fps, uniform_speed? | () | Yes | DOC-VIEW-007 |
| `view.set_section_box` | view_id, enabled, transform?, min?, max? | () | Yes | DOC-VIEW-008 |
| `view.fit_section_box` | element_ids[], view_id?, margin? | view_id | Yes | DOC-VIEW-008 |
| `view.blueprint` | view_id, on: bool, slice? | () | No | DOC-VIEW-013 |
| `view.blueprint_set_slice` | view_id, slice | () | No | DOC-VIEW-013 |
| `view.blueprint_save_as_view` | view_id, name? | view_id | Yes | DOC-VIEW-013 |
| `view.blueprint_set_colour_scheme` | view_id, scheme: default\|blueprint | () | No | DOC-VIEW-013 |
| `view.create_drafting` | name, scale | view_id | Yes | DOC-VIEW-009 |
| `view.insert_from_file` | path, view_names[]? | view_id[] | Yes | DOC-VIEW-009 |
| `view.save_to_library` | view_ids[], path | () | No | DOC-VIEW-009 |
| `detail.insert_from_file` | path, source_view_name, target_view_id, position? | annotation_id[] | Yes | DOC-VIEW-009 |
| `view.create_legend` | name?, scale? | view_id | Yes | DOC-VIEW-010 |
| `legend.place_component` | view_id, type_id, direction?, host_length?, point | element_id | Yes | DOC-VIEW-010 |
| `legend.set_component` | component_ids, direction?, host_length?, point? | () | Yes | DOC-VIEW-010 |
| `legend.list` | view_id | {items, failed} | No | DOC-VIEW-010 |
| `view.create_keynote_legend` | name?, fields[]?, filter_by_sheet? | schedule_id | Yes | DOC-VIEW-010 |
| `view.set_keynote_legend` | schedule_id, filter_by_sheet?, fields[]? | () | Yes | DOC-VIEW-010 |
| `view.duplicate` | view_id, mode: plain\|with_detailing\|dependent, name? | view_id | Yes | DOC-VIEW-011 |
| `view.convert_to_independent` | view_id | () | Yes | DOC-VIEW-011 |
| `graphics.explain` | view_id, element_id, subcategory? | layer_trace | No | DOC-GFX-001 |
| `graphics.resolve` | view_id, element_ids[] | resolved_attributes[] | No | DOC-GFX-001 |
| `objectstyle.set` | category, subcategory?, projection_weight?, cut_weight?, colour?, line_pattern_id?, material_id? | () | Yes | DOC-GFX-002 |
| `objectstyle.create_subcategory` | category, name | subcategory_id | Yes | DOC-GFX-002 |
| `objectstyle.delete_subcategory` | category, name | () | Yes | DOC-GFX-002 |
| `view.set_category_visibility` | view_id, categories[], visible | () | Yes | DOC-GFX-003 |
| `view.set_category_override` | view_id, category, subcategory?, override | () | Yes | DOC-GFX-003 |
| `view.clear_category_override` | view_id, category, subcategory? | () | Yes | DOC-GFX-003 |
| `view.set_host_layer_cut_styles` | view_id, function_styles{} | () | Yes | DOC-GFX-003 |
| `view.visibility_graphics` | view_id? (default: active) | () | No | DOC-GFX-003 |
| `template.create_from_view` | view_id, name | template_id | Yes | DOC-GFX-004 |
| `template.apply` | view_ids[], template_id | () | Yes | DOC-GFX-004 |
| `template.assign` | view_ids[], template_id? | () | Yes | DOC-GFX-004 |
| `template.set_include` | template_id, property, included | () | Yes | DOC-GFX-004 |
| `template.set_property` | template_id, name, value | () | Yes | DOC-GFX-004 |
| `template.delete` | template_id, replacement_id? | () | Yes | DOC-GFX-004 |
| `template.manage` | — | () | No | DOC-GFX-004 |
| `view.temporary_properties` | view_id, mode: enable\|apply_template\|restore, template_id? | () | No | DOC-GFX-005 |
| `filter.create_rule` | name, categories[], rule_tree | filter_id | Yes | DOC-GFX-006 |
| `filter.update_rule` | filter_id, categories?, rule_tree? | () | Yes | DOC-GFX-006 |
| `filter.create_selection` | name, element_ids[] | filter_id | Yes | DOC-GFX-006 |
| `filter.edit_selection` | filter_id, add_ids[], remove_ids[] | () | Yes | DOC-GFX-006 |
| `filter.delete` | filter_id | () | Yes | DOC-GFX-006 |
| `view.add_filter` | view_id, filter_id, index?, visible?, enabled?, override? | () | Yes | DOC-GFX-006 |
| `view.set_filter_override` | view_id, filter_id, override, visible?, enabled? | () | Yes | DOC-GFX-006 |
| `view.reorder_filters` | view_id, filter_ids[] | () | Yes | DOC-GFX-006 |
| `view.remove_filter` | view_id, filter_id | () | Yes | DOC-GFX-006 |
| `filter.evaluate` | filter_id, view_id? | element_id[] | No | DOC-GFX-006 |
| `view.filters` | — | () | No | DOC-GFX-006 |
| `view.override_elements` | view_id, element_ids[], override | () | Yes | DOC-GFX-007 |
| `view.clear_element_overrides` | view_id, element_ids[] | () | Yes | DOC-GFX-007 |
| `view.hide_elements` | view_id, element_ids[] | () | Yes | DOC-GFX-007 |
| `view.unhide_elements` | view_id, element_ids[] | () | Yes | DOC-GFX-007 |
| `view.set_linework` | view_id, edge_refs[], line_style_id? | () | Yes | DOC-GFX-007 |
| `filter.create_from_selection` | view_id, element_ids[], parameter, name | filter_id | Yes | DOC-GFX-007 |
| `view.show_hidden_lines` | view_id, obscuring_ids[], hidden_ids[] | () | Yes | DOC-GFX-020 |
| `view.remove_hidden_lines` | view_id, element_ids[] | () | Yes | DOC-GFX-020 |
| `view.cut_profile` | view_id, target_ref, loops[] | () | Yes | DOC-GFX-020 |
| `view.remove_cut_profile` | view_id, target_ref | () | Yes | DOC-GFX-020 |
| `view.temp_hide_isolate` | view_id, mode: hide_element\|isolate_element\|hide_category\|isolate_category, element_ids_or_categories[] | () | No | DOC-GFX-008 |
| `view.temp_reset` | view_id? (default: active) | () | No | DOC-GFX-008 |
| `view.temp_apply` | view_id? (default: active) | () | Yes | DOC-GFX-008 |
| `view.reveal_hidden` | view_id, on | () | No | DOC-GFX-008 |
| `view.isolate_annotations` | view_id, on, include_categories[]? | () | No | DOC-GFX-008 |
| `lineweight.set` | table: model\|perspective\|annotation, pen, width_mm, scale? | () | Yes | DOC-GFX-009 |
| `lineweight.add_scale` | scale | () | Yes | DOC-GFX-009 |
| `lineweight.remove_scale` | scale | () | Yes | DOC-GFX-009 |
| `settings.set_halftone` | brightness?, underlay_weight?, underlay_pattern_id? | () | Yes | DOC-GFX-009 |
| `view.set_detail_level` | view_id, level | () | Yes | DOC-GFX-010 |
| `settings.set_detail_level_map` | ranges[] | () | Yes | DOC-GFX-010 |
| `view.set_visual_style` | view_id, style | () | Yes | DOC-GFX-011 |
| `view.set_graphic_display` | view_id, options | () | Yes | DOC-GFX-012 |
| `sun.settings` | — | () | No | DOC-GFX-012 |
| `linestyle.create` | name, weight, colour, pattern_id? | style_id | Yes | DOC-GFX-013 |
| `linestyle.update` | style_id, weight?, colour?, pattern_id?, name? | () | Yes | DOC-GFX-013 |
| `linestyle.delete` | style_id, replacement_id | () | Yes | DOC-GFX-013 |
| `linepattern.create` | name, segments[] | pattern_id | Yes | DOC-GFX-013 |
| `linepattern.update` | pattern_id, segments[] | () | Yes | DOC-GFX-013 |
| `linepattern.delete` | pattern_id, replacement_id? | () | Yes | DOC-GFX-013 |
| `fillpattern.create` | name, kind: drafting\|model, definition | pattern_id | Yes | DOC-GFX-014 |
| `fillpattern.import_pat` | path, pattern_name, kind, scale? | pattern_id | Yes | DOC-GFX-014 |
| `fillpattern.rotate_copy` | pattern_id, angle, new_name | pattern_id | Yes | DOC-GFX-014 |
| `fillpattern.delete` | pattern_id, replacement_id? | () | Yes | DOC-GFX-014 |
| `face.set_pattern_alignment` | element_id, face_ref, origin, angle | () | Yes | DOC-GFX-014 |
| `view.set_crop` | view_id, enabled?, visible?, shape? | () | Yes | DOC-GFX-015 |
| `view.reset_crop` | view_id | () | Yes | DOC-GFX-015 |
| `view.set_annotation_crop` | view_id, enabled, offsets? | () | Yes | DOC-GFX-015 |
| `view.add_crop_break` | view_id, axis, position, gap | () | Yes | DOC-GFX-015 |
| `view.remove_crop_break` | view_id, index | () | Yes | DOC-GFX-015 |
| `view.set_discipline` | view_id, discipline | () | Yes | DOC-GFX-016 |
| `discipline.set_profile` | discipline, hidden_categories[], halftone_categories[], hidden_line_categories[] | () | Yes | DOC-GFX-016 |
| `phasefilter.create` | name, settings{status: by_category\|overridden\|hidden} | filter_id | Yes | DOC-GFX-017 |
| `phasefilter.update` | filter_id, settings | () | Yes | DOC-GFX-017 |
| `phase.set_graphic_override` | status, override | () | Yes | DOC-GFX-017 |
| `element.demolish` | element_ids[], phase_id? (default: the active view's phase), view_id? | () | Yes | DOC-GFX-017 |
| `view.set_phase` | view_id, phase_id, phase_filter_id? | () | Yes | DOC-GFX-017 |
| `designoption.create_set` | name | set_id | Yes | DOC-GFX-018 |
| `designoption.create_option` | set_id, name | option_id | Yes | DOC-GFX-018 |
| `designoption.make_primary` | option_id | () | Yes | DOC-GFX-018 |
| `designoption.add_elements` | option_id, element_ids[] | () | Yes | DOC-GFX-018 |
| `designoption.remove_elements` | element_ids[] | () | Yes | DOC-GFX-018 |
| `designoption.set_active` | option_id? | () | Yes | DOC-GFX-018 |
| `designoption.accept_primary` | set_id | () | Yes | DOC-GFX-018 |
| `designoption.delete` | set_or_option_id | () | Yes | DOC-GFX-018 |
| `view.set_design_option` | view_id, set_id, option_id? | () | Yes | DOC-GFX-018 |
| `designoption.manage` | — | () | No | DOC-GFX-018 |
| `scopebox.create` | name, rectangle: {corner_a, corner_b, rotation?}, height, level_id? | element_id | Yes | DOC-GFX-019 |
| `scopebox.update` | scopebox_id, rectangle?, height?, rotation? | () | Yes | DOC-GFX-019 |
| `view.set_scope_box` | view_ids[], scopebox_id? | () | Yes | DOC-GFX-019 |
| `datum.set_scope_box` | datum_ids[], scopebox_id? | () | Yes | DOC-GFX-019 |
| `scopebox.set_view_visibility` | scopebox_id, view_ids[] | () | Yes | DOC-GFX-019 |
| `view.get_draw_list` | view_id, region? | draw_list | No | DOC-HLR-004 |
| `dim.create_aligned` | view_id, refs[], line_point, type_id?, prefer?, pick_mode? | dimension_id | Yes | DOC-ANN-001 |
| `dim.make_permanent` | view_id, temporary_dimension_ref \| {ref_a, ref_b} | dimension_id | Yes | DOC-ANN-001 |
| `dim.create_linear` | view_id, point_refs[], line_point, axis?: horizontal\|vertical, type_id? | dimension_id | Yes | DOC-ANN-002 |
| `dim.create_angular` | view_id, ref_a, ref_b, arc_point, type_id? | dimension_id | Yes | DOC-ANN-003 |
| `dim.create_radial` | view_id, arc_ref, point, type_id?, centre_mark? | dimension_id | Yes | DOC-ANN-003 |
| `dim.create_diameter` | view_id, arc_ref, point, type_id? | dimension_id | Yes | DOC-ANN-003 |
| `dim.create_arc_length` | view_id, arc_ref, end_ref_a, end_ref_b, point, type_id? | dimension_id | Yes | DOC-ANN-003 |
| `dim.set_equality` | dimension_id, on, display? | () | Yes | DOC-ANN-004 |
| `dim.lock_segment` | dimension_id, segment_index, locked | () | Yes | DOC-ANN-004 |
| `dim.set_value` | dimension_id, segment_index, value, move_ref? | () | Yes | DOC-ANN-004 |
| `dim.set_string_type` | dimension_id, type: continuous\|baseline\|ordinate | () | Yes | DOC-ANN-004 |
| `dimtype.create` | name, base_type_id?, properties | type_id | Yes | DOC-ANN-005 |
| `dimtype.update` | type_id, properties | () | Yes | DOC-ANN-005 |
| `dimtype.edit` | kind: linear\|angular\|radial\|diameter\|arc_length\|spot_elevation\|spot_coordinate\|spot_slope | () | No | DOC-ANN-005 |
| `dim.set_text` | dimension_id, segment_index?, replace?, prefix?, suffix?, above?, below? | () | Yes | DOC-ANN-005 |
| `dim.edit_witness_lines` | dimension_id, add_refs[], remove_refs[] | () | Yes | DOC-ANN-005 |
| `dim.set_label` | dimension_id, global_parameter_id? | () | Yes | DOC-ANN-005 |
| `spot.create_elevation` | view_id, ref, point, leader_point?, type_id?, base?, display? | annotation_id | Yes | DOC-ANN-006 |
| `spot.create_coordinate` | view_id, ref, point, leader_point?, type_id?, base? | annotation_id | Yes | DOC-ANN-007 |
| `spot.set` | annotation_ids, base?, display?, include_elevation?, prefix?, suffix?, leader_point? | () | Yes | DOC-ANN-006 |
| `spot.list` | view_id | [{id, data, picked, point, bottom?, lines, orphaned}] | No | DOC-ANN-006 |
| `spot.create_slope` | view_id, ref, point, direction?, representation?: arrow\|triangle, slope_format?, prefix?, suffix? | annotation_id | Yes | DOC-ANN-008 |
| `spot.put_slope` | view_id, ref, point, … (as spot.create_slope) | annotation_id | Yes | DOC-ANN-008 |
| `spot.settings` | — | () | No | DOC-ANN-006 |
| `text.create` | view_id, position, content, type_id?, width?, rotation? | annotation_id | Yes | DOC-ANN-009 |
| `text.format` | style: bold\|italic\|underline\|all_caps\|subscript\|superscript (applies to the open text editor's selection) | () | Yes | DOC-ANN-009 |
| `text.close_editor` | — | () | No | DOC-ANN-009 |
| `text.set_content` | annotation_id, content | () | Yes | DOC-ANN-009 |
| `text.add_leader` | annotation_id, side, attach, points[], shape?: straight\|arc | () | Yes | DOC-ANN-009 |
| `text.remove_leaders` | annotation_id | () | Yes | DOC-ANN-009 |
| `text.set` | annotation_ids[], type_id?, width?, rotation? | () | Yes | DOC-ANN-009 |
| `text.list` | view_id | text_info[] | No | DOC-ANN-009 |
| `text.find_replace` | scope, find, replace, match_case?, whole_word? | count | Yes | DOC-ANN-009 |
| `texttype.create` | name, properties | type_id | Yes | DOC-ANN-009 |
| `texttype.edit` | type_id? | () | No | DOC-ANN-009 |
| `text.check_spelling` | scope?: view\|selection | spelling_issue[] | No | DOC-ANN-009 |
| `tag.create` | view_id, host_ref, position?, tag_type_id?, orientation?, leader? | annotation_id | Yes | DOC-ANN-010 |
| `tag.tag_all` | view_id, categories[], scope: view\|selection, include_links?, leader?, orientation? | annotation_id[] | Yes | DOC-ANN-010 |
| `tag.set_default_type` | category, tag_type_id | () | Yes | DOC-ANN-010 |
| `tag.set_leader` | annotation_id, start_mode?, end_mode?, points? | () | Yes | DOC-ANN-010 |
| `tag.set_type` | annotation_ids[], tag_type_id, orientation? | () | Yes | DOC-ANN-010 |
| `tag.list` | view_id | tag_info[] | No | DOC-ANN-010 |
| `tag.create_material` | view_id, element_ref, layer_ref, position, tag_type_id? | annotation_id | Yes | DOC-ANN-011 |
| `tag.create_room` | view_id, room_id, position?, tag_type_id? | annotation_id | Yes | DOC-ANN-011 |
| `tag.by_example` | view_id, reference_tag_id, targets: all\|selection\|filter, filter_id?, replace_existing? | annotation_id[] | Yes | DOC-ANN-012 |
| `tag.stair_numbers` | view_id, run_ids[], start?, direction? | annotation_id[] | Yes | DOC-ANN-013 |
| `tag.beam_annotations` | view_id, beam_ids[], settings | annotation_id[] | Yes | DOC-ANN-013 |
| `tag.stair_path` | view_id, stair_id, type_id? | annotation_id | Yes | DOC-ANN-013 |
| `tag.beam_system` | view_id, system_id, type_id?, position? | annotation_id | Yes | DOC-ANN-013 |
| `tag.reinforcement_symbol` | view_id, system_id, kind: area\|path\|fabric, type_id?, position? | annotation_id | Yes | DOC-ANN-013 |
| `keynote.settings` | source?, path_type?, numbering?: by_keynote\|by_sheet, embed? | () | Yes | DOC-ANN-014 |
| `keynote.reload` | — | () | Yes | DOC-ANN-014 |
| `keynote.create` | view_id, mode?, host_ref?, key?, position?, style?, leader? | annotation_id | Yes | DOC-ANN-014 |
| `keynote.set` | annotation_ids, style?, key?, leader?, position? | () | Yes | DOC-ANN-014 |
| `keynote.list` | view_id? | [{id, data, host, key, text, lines, frame, anchor, missing}] | No | DOC-ANN-014 |
| `symbol.place` | view_id, type_id, position, rotation?, parameters? | annotation_id | Yes | DOC-ANN-015 |
| `detail.line` | view_id, curves[], line_style_id? | annotation_id[] | Yes | DOC-ANN-016 |
| `detail.set_line_style` | annotation_ids[], line_style_id | () | Yes | DOC-ANN-016 |
| `region.create` | view_id, loops[], kind: filled\|masking, type_id?, edge_styles? | annotation_id | Yes | DOC-ANN-017 |
| `regiontype.create` | name, fg_pattern?, fg_colour?, bg_pattern?, bg_colour?, masking?, pen? | type_id | Yes | DOC-ANN-017 |
| `detail.component` | view_id, type_id, position_or_curve, rotation? | annotation_id | Yes | DOC-ANN-018 |
| `detail.repeating` | view_id, type_id, curve | annotation_id | Yes | DOC-ANN-018 |
| `detail.insulation` | view_id, curve, width, bulge_ratio? | annotation_id | Yes | DOC-ANN-018 |
| `detail.create_group` | view_id, annotation_ids[], name | group_id | Yes | DOC-ANN-018 |
| `detail.place_group` | view_id, group_id, position | annotation_id | Yes | DOC-ANN-018 |
| `detail.arrange` | annotation_ids[], action: front\|forward\|back\|backward | () | Yes | DOC-ANN-020 |
| `revcloud.create` | view_id, loops[], revision_id? | annotation_id | Yes | DOC-ANN-021 |
| `revcloud.set_revision` | annotation_ids[], revision_id | () | Yes | DOC-ANN-021 |
| `colorscheme.create` | name, category, parameter, mode: value\|range, ranges? | scheme_id | Yes | DOC-ANN-022 |
| `colorscheme.set_entry` | scheme_id, value_or_range, colour?, pattern_id?, visible? | () | Yes | DOC-ANN-022 |
| `colorscheme.edit` | scheme_id? | () | No | DOC-ANN-022 |
| `view.set_color_scheme` | view_id, category, scheme_id? (none clears), location?: background\|foreground | () | Yes | DOC-ANN-022 |
| `colorlegend.place` | view_id, scheme_id, position | annotation_id | Yes | DOC-ANN-022 |
| `matchline.create` | view_id, curves[], top?, bottom? | annotation_id | Yes | DOC-ANN-023 |
| `viewref.create` | view_id, target_view_id, position, type_id?, label? | annotation_id | Yes | DOC-ANN-024 |
| `view.go_to_target` | annotation_id | view_id | No | DOC-ANN-024 |
| `schedule.create` | kind, category, name, phase_id?, include_links?, key_name? | schedule_id | Yes | DOC-SCH-001 |
| `schedule.get_table` | schedule_id, include_hidden? | table | No | DOC-SCH-001 |
| `schedule.add_field` | schedule_id, field_source, index? | field_id | Yes | DOC-SCH-002 |
| `schedule.remove_field` | schedule_id, field_id | () | Yes | DOC-SCH-002 |
| `schedule.move_field` | schedule_id, field_id, index | () | Yes | DOC-SCH-002 |
| `schedule.combine_fields` | schedule_id, parts[], name | field_id | Yes | DOC-SCH-002 |
| `schedule.add_calculated_field` | schedule_id, name, value_type, formula?, percentage_of?, percentage_by? | field_id | Yes | DOC-SCH-003 |
| `schedule.update_calculated_field` | schedule_id, field_id, formula?, value_type? | () | Yes | DOC-SCH-003 |
| `schedule.set_filter` | schedule_id, rule_tree?, filter_by_sheet? | () | Yes | DOC-SCH-004 |
| `schedule.set_sort_group` | schedule_id, levels[], grand_totals?, itemize? | () | Yes | DOC-SCH-005 |
| `schedule.insert_data_row` | schedule_id, values? | row_key | Yes | DOC-SCH-006 |
| `schedule.delete_data_row` | schedule_id, row_key | () | Yes | DOC-SCH-006 |
| `element.set_key` | element_ids[], key_parameter, row_key? | () | Yes | DOC-SCH-006 |
| `schedule.set_field_format` | schedule_id, field_id, heading?, orientation?, align?, format?, hidden?, calculation? | () | Yes | DOC-SCH-007 |
| `schedule.add_conditional_format` | schedule_id, field_id, rule_tree, background, text_colour?, bold? | rule_id | Yes | DOC-SCH-007 |
| `schedule.remove_conditional_format` | schedule_id, rule_id | () | Yes | DOC-SCH-007 |
| `schedule.set_appearance` | schedule_id, appearance | () | Yes | DOC-SCH-008 |
| `schedule.set_embedded` | schedule_id, category?, definition? | () | Yes | DOC-SCH-009 |
| `schedule.set_cell` | schedule_id, row_key, field_id, value | () | Yes | DOC-SCH-010 |
| `schedule.paste` | schedule_id, top_left, tsv_text | () | Yes | DOC-SCH-010 |
| `schedule.group_headers` | schedule_id, field_ids[], caption | () | Yes | DOC-SCH-010 |
| `schedule.ungroup_headers` | schedule_id, group_id | () | Yes | DOC-SCH-010 |
| `schedule.merge_cells` | schedule_id, range | () | Yes | DOC-SCH-010 |
| `schedule.insert_image` | schedule_id, cell, image_asset | () | Yes | DOC-SCH-010 |
| `schedule.set_column_width` | schedule_id, field_id, width_mm | () | Yes | DOC-SCH-010 |
| `schedule.highlight_in_model` | schedule_id, row_keys[] | view_id | No | DOC-SCH-010 |
| `sheet.place_schedule` | sheet_id, schedule_id, position | instance_id | Yes | DOC-SCH-011 |
| `scheduleinstance.split` | instance_id, segment_heights[], positions? | () | Yes | DOC-SCH-011 |
| `scheduleinstance.join` | instance_id, segment_index | () | Yes | DOC-SCH-011 |
| `scheduleinstance.set_rotation` | instance_id, angle | () | Yes | DOC-SCH-011 |
| `schedule.export` | schedule_id, path, format: csv\|tsv\|xlsx\|ods, options? | () | No | DOC-SCH-012 |
| `sheet.create` | title_block_type_id?, number?, name?, collection_id?, from_placeholder? | sheet_id | Yes | DOC-SHT-001 |
| `sheet.duplicate` | sheet_id, mode: empty\|detailing\|views, view_mode?: plain\|with_detailing\|dependent | sheet_id | Yes | DOC-SHT-001 |
| `sheet.set_parameter` | sheet_id, name, value | () | Yes | DOC-SHT-001 |
| `sheet.delete` | sheet_ids[] | () | Yes | DOC-SHT-001 |
| `sheet.place_image` | sheet_id, asset_or_path, position, size? | annotation_id | Yes | DOC-SHT-001 |
| `titleblock.place` | sheet_id, type_id, position? | instance_id | Yes | DOC-SHT-002 |
| `titleblock.swap` | instance_id, type_id | () | Yes | DOC-SHT-002 |
| `titleblock.set_primary` | instance_id | () | Yes | DOC-SHT-002 |
| `titleblock.reset_positions` | sheet_ids[] | () | Yes | DOC-SHT-002 |
| `project.set_info` | name, value | () | Yes | DOC-SHT-002 |
| `sheet.place_view` | sheet_id, view_id, position, viewport_type_id? | viewport_id | Yes | DOC-SHT-003 |
| `viewport.move` | viewport_id, position | () | Yes | DOC-SHT-003 |
| `viewport.set_rotation` | viewport_id, rotation | () | Yes | DOC-SHT-003 |
| `viewport.swap_view` | viewport_id, view_id | () | Yes | DOC-SHT-003 |
| `viewport.move_to_sheet` | viewport_ids[], sheet_id | () | Yes | DOC-SHT-003 |
| `viewport.activate` | viewport_id, on | () | No | DOC-SHT-003 |
| `viewport.set_detail_number` | viewport_id, number | () | Yes | DOC-SHT-003 |
| `viewport.remove` | viewport_ids[] | () | Yes | DOC-SHT-003 |
| `sheet.open_for_view` | view_id | sheet_id | No | DOC-SHT-003 |
| `viewporttype.create` | name, title_type_id?, show_title?, show_extension?, pen?, colour?, pattern_id?, preserve_title_position? | type_id | Yes | DOC-SHT-004 |
| `viewport.set_type` | viewport_ids[], type_id | () | Yes | DOC-SHT-004 |
| `viewport.set_title` | viewport_id, offset?, extension_length? | () | Yes | DOC-SHT-004 |
| `guidegrid.create` | name, spacing, extents | guide_grid_id | Yes | DOC-SHT-005 |
| `sheet.set_guide_grid` | sheet_ids[], guide_grid_id? | () | Yes | DOC-SHT-005 |
| `position.save` | viewport_id, name | position_id | Yes | DOC-SHT-005 |
| `position.assign` | view_ids[], position_id? | () | Yes | DOC-SHT-005 |
| `position.edit` | position_id, position | () | Yes | DOC-SHT-005 |
| `position.delete` | position_id | () | Yes | DOC-SHT-005 |
| `viewport.set_anchor` | viewport_ids[], anchor | () | Yes | DOC-SHT-005 |
| `sheetcollection.create` | name | collection_id | Yes | DOC-SHT-006 |
| `sheetcollection.rename` | collection_id, name | () | Yes | DOC-SHT-006 |
| `sheetcollection.delete` | collection_id | () | Yes | DOC-SHT-006 |
| `sheetcollection.move_sheets` | sheet_ids[], collection_id? | () | Yes | DOC-SHT-006 |
| `sheetcollection.set_parameter` | collection_id, name, value | () | Yes | DOC-SHT-006 |
| `sheet.create_placeholder` | number, name, collection_id?, parameters? | sheet_id | Yes | DOC-SHT-007 |
| `sheet.convert_placeholders` | sheet_ids[], title_block_type_id? | sheet_id[] | Yes | DOC-SHT-007 |
| `revision.create` | description?, date?, numbering_sequence_id?, index? | revision_id | Yes | DOC-SHT-008 |
| `revision.update` | revision_id, fields | () | Yes | DOC-SHT-008 |
| `revision.set_issued` | revision_id, issued | () | Yes | DOC-SHT-008 |
| `revision.merge` | revision_id, direction: up\|down | () | Yes | DOC-SHT-008 |
| `revision.reorder` | revision_id, index | () | Yes | DOC-SHT-008 |
| `revision.delete` | revision_id, reassign_to? | () | Yes | DOC-SHT-008 |
| `revision.set_settings` | numbering?: per_project\|per_sheet, arc_length? | () | Yes | DOC-SHT-008 |
| `revisionsequence.create` | name, kind: numeric\|alphanumeric, start?, min_digits?, prefix?, suffix?, values? | sequence_id | Yes | DOC-SHT-008 |
| `sheet.set_revisions_on_sheet` | sheet_id, revision_ids[] | () | Yes | DOC-SHT-008 |
| `revision.manage` | — | () | No | DOC-SHT-008 |
| `assembly.create` | element_ids[], name?, naming_category? | assembly_id | Yes | DOC-ASM-001 |
| `assembly.place_instance` | assembly_type_id, position, rotation? | assembly_id | Yes | DOC-ASM-001 |
| `assembly.set_naming_category` | assembly_ids[], naming_category | () | Yes | DOC-ASM-001 |
| `assembly.list` | assembly_type_id? | assembly_summary[] | No | DOC-ASM-001 |
| `assembly.compare` | assembly_id_a, assembly_id_b | comparison | No | DOC-ASM-002 |
| `assembly.find_identical` | assembly_id | assembly_id[] | No | DOC-ASM-002 |
| `assembly.edit_members` | assembly_id, add_ids[], remove_ids[] | assembly_type_id | Yes | DOC-ASM-003 |
| `assembly.set_origin` | assembly_id, transform, apply_to_type? | () | Yes | DOC-ASM-003 |
| `assembly.set_type` | assembly_ids[], assembly_type_id | () | Yes | DOC-ASM-003 |
| `assembly.disassemble` | assembly_ids[] | element_id[] | Yes | DOC-ASM-003 |
| `assembly.delete_type` | assembly_type_id | () | Yes | DOC-ASM-003 |
| `assembly.create_views` | assembly_type_id, views[], scale?, templates?, sheet?: {title_block_type_id, number?, name?} | view_id[] | Yes | DOC-ASM-004 |
| `assembly.acquire_views` | assembly_id | () | Yes | DOC-ASM-004 |
| `assembly.create_sheet` | assembly_type_id, title_block_type_id?, number?, name? | sheet_id | Yes | DOC-ASM-005 |
| `assemblytemplate.save` | sheet_id, name | template_id | Yes | DOC-ASM-005 |
| `assembly.apply_template` | assembly_ids_or_type_ids[], template_id, group_identical?=true, fit_to_sheet?=true | sheet_id[] | Yes | DOC-ASM-005 |
| `assembly.propagate_views` | source_type_id, target_type_ids[], include_annotation?=true, replace_existing?=false | report | Yes | DOC-ASM-005 |
| `viewset.save` | name, view_and_sheet_ids[]?, rule? | set_id | Yes | DOC-OUT-001 |
| `viewset.delete` | set_id | () | Yes | DOC-OUT-001 |
| `viewset.resolve` | set_id | view_or_sheet_id[] | No | DOC-OUT-001 |
| `print.run` | range, set_id?, printer?, setup_id?, copies?, combine? | job_id | No | DOC-OUT-002 |
| `print.setup_save` | name, settings | setup_id | Yes | DOC-OUT-002 |
| `print.setup_delete` | setup_id | () | Yes | DOC-OUT-002 |
| `print.preview` | range, setup_id? | page_images[] | No | DOC-OUT-002 |
| `export.pdf` | range, set_id?, setup_id?, path_or_folder, combine?, naming_rule?, background? | job_id | No | DOC-OUT-003 |
| `export.pdf_setup_save` | name, settings | setup_id | Yes | DOC-OUT-003 |
| `export.pdf_setup_delete` | setup_id | () | Yes | DOC-OUT-003 |
| `export.image` | range, set_id?, path_or_folder, format, size, dpi?, options? | job_id | No | DOC-OUT-004 |
| `export.dxf` | range, set_id?, setup_id?, folder, naming?, version?, xrefs? | job_id | No | DOC-OUT-005 |
| `export.dwg` | range, set_id?, setup_id?, folder, naming?, version?, xrefs? | job_id | No | DOC-OUT-005 |
| `export.cad_setup_save` | name, settings | setup_id | Yes | DOC-OUT-005 |
| `export.cad_setup_load_layer_standard` | setup_id, standard | () | Yes | DOC-OUT-005 |
| `export.ifc` | path, setup? (name, id or inline settings), settings_override?, schema?, strip_rxd?=false, documentation?, background? | job_id (the job result carries the export report) | No | DOC-OUT-006 |
| `export.ifc_setup_save` | name, settings | setup_id | Yes | DOC-OUT-006 |
| `export.ifc_setup_delete` | setup_id | () | Yes | DOC-OUT-006 |
| `export.gltf` | path, view_id?, binary?, options? | job_id | No | DOC-OUT-007 |
| `export.obj` | path, view_id?, options? | job_id | No | DOC-OUT-007 |
| `export.step` | path, view_id_or_element_ids, schema?: ap214\|ap242 | job_id | No | DOC-OUT-007 |
| `export.gbxml` | path, source: rooms\|spaces\|energy_model, options? | job_id | No | DOC-OUT-008 |
| `export.database` | path, format: sqlite\|csv, categories[]? | job_id | No | DOC-OUT-009 |
| `export.cobie` | path, version?: current\|v2_4, format?: xlsx\|csv, options? | {tables[], warnings[], path} | No | DOC-OUT-015 |
| `cobie.validate` | version? | issues[] | No | DOC-OUT-015 |
| `cobie.set_settings` | settings | () | Yes | DOC-OUT-015 |
| `export.room_report` | path, scope? | job_id | No | DOC-OUT-009 |
| `job.status` | job_id | status | No | DOC-OUT-013 |
| `job.wait` | job_id, timeout? | status | No | DOC-OUT-013 |
| `job.cancel` | job_id | () | No | DOC-OUT-013 |
| `job.list` | — | job_summary[] | No | DOC-OUT-013 |
| `render.start` | view_id, settings, renderer?, region? | job_id | No | DOC-OUT-011 |
| `render.stop` | job_id | () | No | DOC-OUT-011 |
| `render.save_to_project` | job_id, name | view_id | Yes | DOC-OUT-011 |
| `render.export` | job_id_or_view_id, path, format | () | No | DOC-OUT-011 |
| `render.list_renderers` | — | renderer_info[] | No | DOC-OUT-011 |
| `render.gallery` | — | () | No | DOC-OUT-011 |
| `render.backend.list` | — | backend_info[] | No (query) | DOC-RBE-001 |
| `render.backend.set_default` | kind, backend_id | () | No (user preference) | DOC-RBE-001 |
| `render.backend.test` | backend_id | report | No | DOC-RBE-001 |
| `view.set_presentation_backend` | view_id, backend_id? (none: built-in) | () | Yes | DOC-RBE-001 |
| `render.bridge.launch` | backend_id, view_id?, args? | session_id | No | DOC-RBE-002 |
| `render.bridge.connect_info` | — | {endpoint, token} (UI only; never exposed as an MCP resource) | No | DOC-RBE-002 |
| `render.bridge.disconnect` | session_id | () | No | DOC-RBE-002 |
| `render.bridge.list_sessions` | — | session_info[] | No (query) | DOC-RBE-002 |
| `decal.place` | view_id, decal_type_id, face_ref, position, width?, height? | element_id | Yes | DOC-OUT-011 |
| `decaltype.create` | name, image_asset_or_path, attributes? | type_id | Yes | DOC-OUT-011 |
| `export.animation` | view_id, kind: walkthrough\|solar_study, path, frames?, fps?, size, style?, format: images\|webm\|mp4 | job_id | No | DOC-OUT-012 |
| `solarstudy.set` | view_id, kind: still\|single_day\|multi_day, date_range, time_range, interval | () | Yes | DOC-OUT-012 |
| `publish.review_set` | set_id, include_3d_views[]?, target: folder\|bimserver, path? | job_id | No | DOC-OUT-014 |

## 5.19 Open questions and deviations from the reference

**Deviations (intentional).**

| # | Topic | Reviera behaviour | Reason |
|---|---|---|---|
| D5-1 | Storage of documentation | Views, sheets, schedules, standards and annotation are Reviera Extension Data (RXD) embedded in the IFC file by default (Chapter 2a), not a proprietary project database. Annotation is written as `IfcAnnotation` only on export. | Native IFC with full compatibility; per-object three-way merge for BIMserver mode. |
| D5-2 | Options Bar | None. Tool options are on the contextual ribbon tab and in the Properties palette. | Matches the reference's own 2027 direction; one less UI surface to implement. |
| D5-3 | Schedule filters | A nested AND/OR rule tree with no limit on the number of rules, shared with view filters; calculated and hidden fields can be used anywhere. | The reference's eight AND-only rules are a common complaint; one engine for both is simpler. |
| D5-4 | Graphics Inspector | `graphics.explain` and the *Why does this look like this?* command show which override layer won. | Makes the precedence model transparent for users and AI agents. |
| D5-5 | View range category behaviour | The above-cut behaviour and floor-like rule are a visible, editable per-category table rather than fixed internal rules. | Predictability, and offices can match their conventions. |
| D5-6 | Keynote tables | Plain tab- or comma-separated files, optionally embedded in the project. | Avoids missing-file problems, especially in BIMserver mode. |
| D5-7 | *Replace with text* on dimensions | Numeric replacement text is refused. | Prevents drawings that silently show false dimensions. |
| D5-8 | DWG | DXF is the primary CAD format. DWG is written only through LibreDWG and only in the versions it supports reliably; no proprietary DWG libraries. | Licence compatibility (GPL-3.0-or-later). |
| D5-9 | FBX, DWF, DGN, NWC, Datasmith, ODBC, proprietary solid formats | Not provided (P3). Alternatives: glTF, USD (later), STEP, PDF, DXF, IFC + BCF, SQLite/CSV. | Proprietary formats or SDKs incompatible with the GPL. |
| D5-10 | Rendering | Optional open renderers (built-in `wgpu` path tracer, Cycles; others such as LuxCoreRender or Mitsuba possible) behind one interface; no cloud rendering. | No proprietary renderer can be shipped; vendor cloud services are out of scope. |
| D5-11 | Accelerated graphics | There is only one GPU viewport (`wgpu`), always on, which supports every visual style, section boxes, halftone, transparency and links. | Reviera starts with a modern renderer, so there is no legacy engine to toggle. |
| D5-12 | Publishing | Publishing to a vendor cloud is replaced by review sets (PDF + glTF/IFC) written to a folder or attached to a BIMserver revision, with issues in BCF. | Vendor cloud out of scope; open formats instead. |
| D5-13 | Walkthrough video | WebM/AV1 or image sequences; no bundled patent-encumbered codecs. | Licensing. |
| D5-14 | Unused IFC export option | "Store IFC GlobalId in a parameter after export" does not exist. | GlobalIds are native in Reviera. |
| D5-15 | Assembly members | Whole system elements (beam systems, trusses, curtain walls, stairs, railings, reinforcement systems) may be members of an assembly as nested aggregates; parts are members through `IfcRelAssignsToProduct` (DOC-ASM-001). | The reference refuses complex structures; IFC allows nested `IfcElementAssembly`, and precast and steel shop assemblies need it. |
| D5-16 | Assembly views after a type change | When an edited owner instance joins a type that already has views, the user chooses which set to keep (DOC-ASM-004). | The reference deletes one set without a choice. |
| D5-17 | Render gallery | A local render history with exposure re-adjustment (DOC-OUT-011) replaces the reference's cloud render gallery. | Vendor cloud services are out of scope (P3). |
| D5-19 | Pluggable presentation backends | The reference product has one built-in renderer plus a vendor cloud service. Reviera puts all 3D presentation behind an open backend interface (DOC-RBE-001…002). The scene protocol, walkthroughs, the Godot translation layer for VR/AR and the reference bridges are specified in Chapter 7. | Owner directive 2026-10-12: future photo-real, game-engine and Blender backends for simulated walkthroughs. |
| D5-18 | Displaced elements | Displacement is a pure per-view transform, so constrained or hosted elements can be displaced; dimensions to displaced elements are refused instead (DOC-VIEW-012). | The model is never moved, so constraints cannot be violated; a dimension would report a false distance. |

**Open questions.**

| # | Question | Owner |
|---|---|---|
| Q5-1 | Exact list of cuttable categories, above-cut behaviours and floor-like categories (DOC-VR-002) needs confirming against Chapter 2's category catalogue and against real drawing conventions. | DOC + DM |
| Q5-2 | Performance targets for exact HLR on very large views (tile size, PolyAlgo pre-pass thresholds) must be set from prototypes. | DOC |
| Q5-3 | Whether OCCT's `HLRBRep_Algo` is robust enough on IFC-derived tessellated geometry (Reference View imports) or whether mesh-based HLR must be the default for such elements. | DOC |
| Q5-4 | IFC4X3 `IfcAnnotationTypeEnum` values to use for each annotation class on export need checking against the final schema. | DOC |
| Q5-5 | PDF tiling-pattern support varies between PDF viewers; decide when hatches are written as explicit geometry instead. | DOC |
| Q5-6 | ~~RXD document names and kinds must match Chapter 2a~~ **Resolved**: every kind used here is in the Object Kind Registry (02a §6.5); colour schemes are stored in `views/color_schemes`. | DOC + DM |
| Q5-7 | Size limits for embedded RXD in very annotation-heavy projects (thousands of views) still need measuring. ~~Lazy loading per view with the embedded carrier~~ **resolved**: it works with both carriers (02a §3, §9). | DM |
| Q5-8 | Which BIMserver extended-data calls are available for attaching review sets (DOC-OUT-014) on BIMserver 1.5.188, the target release. | COL |
| Q5-9 | ~~Precast pieces with two IFC parents~~ **Deferred**: handled by Chapter 4 Q13. The initial version writes only the assembly aggregation and keeps the source link in RXD; a final decision needs expert advice. | STR + DOC |
| Q5-10 | ~~New RXD kinds to be added to 02a~~ **Resolved**: registered in 02a §6.4–§6.5. | DM |

## 5.20 Traceability

| Research section (`research/04_documentation_collaboration.md`) | Requirements |
|---|---|
| Intro (core abstractions), §11 data model and key behaviours | DOC-CORE-001 … 008, DOC-IFC-001, DOC-GFX-001, DOC-HLR-001 |
| §1 intro (common view properties, Project Browser) | DOC-VIEW-001 |
| §1.1 Plan views, view range, plan regions, underlay, depth clipping | DOC-VIEW-002, DOC-VR-001 … 005 |
| §1.2 Elevations | DOC-VIEW-003 |
| §1.3 Sections | DOC-VIEW-004 |
| §1.4 Callouts | DOC-VIEW-005 |
| §1.5 3D views, camera, walkthrough, section box, accelerated graphics | DOC-VIEW-006 … 008, DOC-HLR-003, DOC-HLR-005, DOC-HLR-006, DOC-OUT-012 |
| §1.6 Drafting views | DOC-VIEW-009 |
| §1.7 Legends and keynote legends | DOC-VIEW-010 |
| §1.8 Duplicating views, dependent views | DOC-VIEW-011, DOC-ANN-023 |
| §2.1–2.5 Schedules | DOC-SCH-001 … 012 |
| §3.1 Graphics resolution stack | DOC-GFX-001 |
| §3.2 View templates, temporary view properties | DOC-GFX-004, DOC-GFX-005 |
| §3.3 Visibility/Graphic Overrides | DOC-GFX-003 |
| §3.4 View filters | DOC-GFX-006 |
| §3.5 Element overrides, hide, linework, temporary hide/isolate, isolate annotations | DOC-GFX-007, DOC-GFX-008 |
| §3.6 Line weights, line styles, line patterns, fill patterns, halftone | DOC-GFX-009, DOC-GFX-013, DOC-GFX-014 |
| §3.7 Detail level, visual styles, graphic display options | DOC-GFX-010 … 012 |
| §3.8 Crop regions, annotation crop, scope boxes | DOC-GFX-015, DOC-GFX-019 |
| §3.9 Phasing | DOC-GFX-017 |
| §3.10 Design options | DOC-GFX-018 |
| §1 (Discipline property) | DOC-GFX-016 |
| §4.1 Dimensions and spot dimensions | DOC-ANN-001 … 008 |
| §4.2 Text and leaders | DOC-ANN-009 |
| §4.3 Tags | DOC-ANN-010 … 013 |
| §4.4 Keynotes | DOC-ANN-014 |
| §4.5 Symbols | DOC-ANN-015 |
| §4.6 Detail lines, regions, components, draw order, revision clouds, colour fill | DOC-ANN-016 … 018, DOC-ANN-020 … 022 |
| §4.7 Matchlines and view references | DOC-ANN-023, DOC-ANN-024 |
| §5.1 Sheets | DOC-SHT-001 |
| §5.2 Title blocks | DOC-SHT-002 |
| §5.3 Placing views, positioning | DOC-SHT-003, DOC-SHT-005 |
| §5.4 Viewport types | DOC-SHT-004 |
| §5.5 Sheet collections | DOC-SHT-006 |
| §5.6 Sheet list, placeholders | DOC-SHT-007 |
| §5.7 Revisions | DOC-SHT-008, DOC-ANN-021 |
| §6.1 Print | DOC-OUT-001, DOC-OUT-002, DOC-HLR-007 |
| §6.2 PDF export | DOC-OUT-003, DOC-OUT-013 |
| §6.3 DWG/DXF export | DOC-OUT-005, DOC-HLR-004 |
| §6.4 IFC export | DOC-OUT-006, DOC-IFC-002 |
| §6.5 Other exports | DOC-OUT-004, DOC-OUT-007 … 009 |
| §6.6 Rendering | DOC-OUT-011 |
| §6.7 Publishing | DOC-OUT-014 |
| §3.6 (*Assembly Code* setting), §5.1 (assembly sheet fields); web: About Assemblies, Create an Assembly, Work with an Assembly, Disassemble an Assembly, Delete an Assembly, Create Assembly Views and Sheets, Assembly Instance/Type Properties; buildingSMART IFC 4.3 docs: IfcElementAssembly, IfcElementAssemblyType, IfcElementAssemblyTypeEnum, IfcAssemblyPlaceEnum | DOC-ASM-001 … 005 |
| §3.1 (*Highlight Displacement*); web: About Displacement Sets and Paths, Create/Edit/Reset/Highlight Displacement Sets, Modify Displacement Set Path Style | DOC-VIEW-012 |
| §1 (Show Hidden Lines), §3.5; web: Show Hidden Lines by Element, Change the Cut Profile of Elements | DOC-GFX-020 |
| web: Insert a Drafting View from Another Project, Reuse Views and Sheets in Other Projects, Insert Detail Components from a Saved Detail View | DOC-VIEW-009 |
| web: Edit Type Properties for View Tags, About Section Tag Visibility, Hide Section Tags, Find a Referring View | DOC-VIEW-001 |
| web: Add Tags and Keynotes in a 3D View | DOC-VIEW-006 |
| web: Spot Dimensions, Spot Elevation Type Properties | DOC-ANN-006 … 008 |
| §4.2; web: Edit a Text Note, Text Note Type Properties, Check Spelling in Text Notes, Options Dialog: Check Spelling Tab | DOC-ANN-009 |
| §4.5; web: Annotate the Stair Path, About Tagging Beam Systems | DOC-ANN-013 |
| §6.6 (decals, cloud render gallery); web: Place a Decal in a View, Decal Type Properties, Decal Attributes | DOC-OUT-011 |
| web: Define Sun Settings for a View | DOC-GFX-012 |
| web: Filters Dialog | DOC-GFX-006 |
| Owner directive 2026-10-12 (pluggable renderer; Godot translation layer for walkthrough and VR/AR); Khronos glTF 2.0 and OpenXR public specifications | DOC-RBE-001 … 002 (and Chapter 7) |
| §10 Version change log | Version-specific behaviours folded into the requirements above (for example DOC-SHT-005, DOC-SHT-006, DOC-ANN-012, DOC-GFX-008) |

Screenshots consulted (internal reference only, never used as Reviera assets): all files in `screenshots/documentation/` that are cited in the requirement headers above.

Web sources marked `web:` are public reference-product help pages and buildingSMART documentation, listed with URLs, access date and HTTP status in the session sources file for this chapter (to be merged into `RESEARCH_LOG.md`).
