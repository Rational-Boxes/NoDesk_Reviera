# Reviera sample models

Models built with Reviera, to show and test its modelling features. Unlike `test-data/ifc`
(third-party validation models), these are the project's own, under the project licence.

Each sample is rebuilt from `samples.json`, a list of command steps: Lua scripts in this
folder (`script.run`), plugin runs (`plugin.run`) and settings. The test
`crates/reviera-app/tests/samples.rs` rebuilds every sample. Each must have no overlapping
elements beyond those it leaves on purpose, and no intrusions into its clearances, and must save and reopen intact. To rewrite
the `.ifc` files after a change:

```sh
REVIERA_REGEN_SAMPLES=1 cargo test -p reviera-app --test samples
```

or only some of them, by name: `REVIERA_REGEN_SAMPLES=temple,csg`.

A sample with a `weather` entry has that EPW imported after it is saved, so the file is kept in
`<name>.resources/weather` beside it; Help copies the folder with the sample. The weather year
`reviera_synthetic.epw` is synthetic, made by `make_weather.py` (sun positions, a clear-sky model
dimmed by a repeatable cloud cover, seasonal and daily temperatures): the project's own, not
measured weather. `crates/reviera-app/tests/sample_analyses.rs` checks that the MEP and
sustainability samples show their features from their saved files, running their analyses.

Each entry's `group` names the part of the application it shows, as the ribbon's tabs are
(Architecture, Structure, View, Modify …); Help's landing page lists the samples under those
headings, in the ribbon's order.

| Sample | Shows |
|---|---|
| `house.ifc` (`house.lua`) | Walls, doors and windows. A hipped roof with eaves bearing on the walls (fascia, soffits), walls attached. A gable and a shed dormer joined flush. A U-shaped and an L-shaped stair with railings. A railing with a single gate, a double gate and an opening. A deck with a railing on its boards. Two levels with plans. |
| `observatory.ifc` (`observatory.lua`, Castle Turret plugin) | A drum of curved walls (two arcs) rising to a hemispherical dome, with an observing slot cut from the dome by a boolean (CSG). Doors and windows on the curve, a round floor and room. A barrel-vaulted annex. A spline garden wall. A turret with an onion dome, built by the Castle Turret plugin. ADA 2010 door clearances. |
| `storefront.ifc` (`storefront.lua`) | The door operations side by side in a shop front: a revolving door, an automatic bi-parting sliding door (which takes the wall its leaves slide over), a pair of glass storefront doors and an automatic storefront door; shop windows clear of the sliding leaves; a flat roof; ADA 2010 rules on. |
| `manor.ifc` (`manor.lua`) | The demonstration model. A two-storey house: walls on each level, partitions, an arched front door, round-top and Gothic windows with glazing bars and an oculus, floors, rooms and automatic ceilings, a hipped roof with eaves and a gable dormer with a round-top window (added by the owner). A straight stair that completes itself to Level 2, with railings, opening the ceiling and floor over it for the IRC's headroom (the model's code), and a guard at the top. A round three-storey tower of curved walls with Gothic windows and an oculus, two spiral stairs each opening the floor above, and an onion dome. A deck with a guard railing and a gate. Three levels with plans and a 3D view. |
| `csg.ifc` (`csg.lua`) | The same overlap (a wall pierced by a taller, thicker pier) resolved every way `overlap.resolve` offers: unresolved, cut A by B, cut B by A, join, intersect, union, move apart, accept. Each is labelled in the plan. The one unresolved pair is the model's only overlap (`allow_overlaps`). |
| `steel_frame.ifc` (`steel_frame.lua`) | A two-storey steel frame (IBC, AISC shapes): W12X53 columns on a 30 × 25 ft grid, W18X35 girders, W16X26 beams, W12X26 infill beam systems framing into the girders, HSS X-bracing, 6" concrete slabs (columns cut through them), spread footings, steel connections from the library (base plates with anchor rods, shear plates, brace gussets; plates, bolts and welds at Fine detail); its analytical model (members split where beams frame in, slab panels), pinned column bases, dead, live, roof live and wind cases with area and point loads, and the ASCE 7 combinations, ready for Run Analysis. |
| `timber.ifc` (`timber.lua`) | A three-storey mass timber frame (DIN 18065, so Eurocode materials): glulam GL24h columns storey by storey, GL28h beams, 200 mm CLT floors of C24, glulam diagonal braces, pad footings; its analytical model with CLT panels, pinned column bases, G, Q and S cases and the EN 1990 6.10 combinations. |
| `structural_checks.ifc` (`structural_checks.lua`) | A small steel frame with deliberate faults, each labelled in the Level 2 plan, for Check Supports and Check Consistency: a floating ring of beams (S2, C2), a beam resting on nothing (S1), a member 40 mm short of a column (C1), members framing mid-span with no node (C1), a girder moved off its analytical line (C3, C4), a beam with no analytical member (C5), a member pinned at both ends (C7), a slab well below its beams (C6). Its six overlaps are part of the faults (`allow_overlaps`). |
| `wood_frame.ifc` (`wood_frame.lua`) | A two-storey light wood-frame house (IRC, NA lumber): 2x6 stud bearing walls on strip footings, 2x10 joists at 16" with a 4x12 flush beam on 4x6 posts, a 3/4" plywood subfloor, a slab on grade, gable Fink trusses (6:12) of 2x6 and 2x4 at 24"; its analytical model with wall and subfloor panels, supports at the post bases, wall bottoms and bearings, and the ASCE 7 combinations. |

| `mep_hvac.ifc` (`mep_hvac.lua`) | An open office split by a space separator into two Office spaces (loads and set points) and two system-zones; an air handling unit and four 150 L/s diffusers joined into a 400 mm supply main by Connect Into (tees and elbows made), a sound attenuator, a return grille, the supply air system; rooms on the same line show 300 L/s supplied to each. |
| `mep_plumbing.ifc` (`mep_plumbing.lua`) | A washroom: lavatories, a urinal and WCs on the wall's face, a floor drain, a water heater; a 100 mm sanitary main sloped at 1% below the floor and the fixtures' sanitary system; a cold-water main with a gate valve; a wet sprinkler branch with three pendent heads joined into it. |
| `mep_electrical.ifc` (`mep_electrical.lua`) | A main switchboard feeding a distribution board; nine 5 000 lm ceiling luminaires on circuit LP1-1 with a switch, receptacles on LP1-2 and LP1-3, a smoke detector; the space's lumen-method illuminance (about 324 lx). |
| `energy_house.ifc` (`energy_house.lua`, weather in `energy_house.resources`) | A four-room bungalow ready for the energy tools: a site at 51.5° N with the synthetic weather year, layered insulated exterior walls (U 0.18), large south triple glazing and small north windows with U-values and solar gain, a slab, four Dwelling spaces; for Heating & Cooling Loads, Annual Energy (about 51 kWh/m² a year with a heat pump), Embodied Carbon and the gbXML and HBJSON exports. |
| `daylight_studio.ifc` (`daylight_studio.lua`, weather in `daylight_studio.resources`) | Two identical studios with large south windows under a roof slab; a neighbouring block 8 m high, 5 m south of the east one. Daylight Factor gives about 3.9% in the west studio and 2.5% in the east; Solar Study and PVGIS Check on the roof. |

Each sample also has a walkthrough (`house.md`, `observatory.md`, `csg.md`, `manor.md`, `storefront.md`, `steel_frame.md`, `timber.md`, `wood_frame.md`, `structural_checks.md`, `mep_hvac.md`, `mep_plumbing.md`, `mep_electrical.md`, `energy_house.md`, `daylight_studio.md`): the manual steps to
build it in the window. It names the UI path for every operation the script uses, and marks
the steps the window cannot do yet. Those gaps are missing UI for core features, or scripts
that need an input form.

Open one with `reviera gui test-data/samples/manor.ifc`.
