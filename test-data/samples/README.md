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

| Sample | Shows |
|---|---|
| `house.ifc` (`house.lua`) | Walls, doors and windows. A hipped roof with eaves bearing on the walls (fascia, soffits), walls attached. A gable and a shed dormer joined flush. A U-shaped and an L-shaped stair with railings. A railing with a single gate, a double gate and an opening. A deck with a railing on its boards. Two levels with plans. |
| `observatory.ifc` (`observatory.lua`, Castle Turret plugin) | A drum of curved walls (two arcs) rising to a hemispherical dome, with an observing slot cut from the dome by a boolean (CSG). Doors and windows on the curve, a round floor and room. A barrel-vaulted annex. A spline garden wall. A turret with an onion dome, built by the Castle Turret plugin. ADA 2010 door clearances. |
| `storefront.ifc` (`storefront.lua`) | The door operations side by side in a shop front: a revolving door, an automatic bi-parting sliding door (which takes the wall its leaves slide over), a pair of glass storefront doors and an automatic storefront door; shop windows clear of the sliding leaves; a flat roof; ADA 2010 rules on. |
| `manor.ifc` (`manor.lua`) | The demonstration model. A two-storey house: walls on each level, partitions, an arched front door, round-top and Gothic windows with glazing bars and an oculus, floors, rooms and automatic ceilings, a hipped roof with eaves and a gable dormer with a round-top window (added by the owner). A straight stair that completes itself to Level 2, with railings, opening the ceiling and floor over it for the IRC's headroom (the model's code), and a guard at the top. A round three-storey tower of curved walls with Gothic windows and an oculus, two spiral stairs each opening the floor above, and an onion dome. A deck with a guard railing and a gate. Three levels with plans and a 3D view. |
| `csg.ifc` (`csg.lua`) | The same overlap (a wall pierced by a taller, thicker pier) resolved every way `overlap.resolve` offers: unresolved, cut A by B, cut B by A, join, intersect, union, move apart, accept. Each is labelled in the plan. The one unresolved pair is the model's only overlap (`allow_overlaps`). |

Each sample also has a walkthrough (`house.md`, `observatory.md`, `csg.md`, `manor.md`, `storefront.md`): the manual steps to
build it in the window. It names the UI path for every operation the script uses, and marks
the steps the window cannot do yet. Those gaps are missing UI for core features, or scripts
that need an input form.

Open one with `reviera gui test-data/samples/manor.ifc`.
