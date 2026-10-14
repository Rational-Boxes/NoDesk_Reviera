# Reviera sample models

Models built with Reviera, to show and test its modelling features. Unlike `test-data/ifc`
(third-party validation models), these are the project's own, under the project licence.

Each sample is rebuilt from `samples.json`, a list of command steps: Lua scripts in this
folder (`script.run`), plugin runs (`plugin.run`) and settings. The test
`crates/reviera-app/tests/samples.rs` rebuilds every sample. Each must have no overlapping
elements and no intrusions into its clearances, and must save and reopen intact. To rewrite
the `.ifc` files after a change:

```sh
REVIERA_REGEN_SAMPLES=1 cargo test -p reviera-app --test samples
```

| Sample | Shows |
|---|---|
| `house.ifc` (`house.lua`) | Walls, doors and windows. A hipped roof with eaves bearing on the walls (fascia, soffits), walls attached. A gable and a shed dormer joined flush. A U-shaped and an L-shaped stair with railings. A railing with a single gate, a double gate and an opening. A deck with a railing on its boards. Two levels with plans. |
| `observatory.ifc` (`observatory.lua`, Castle Turret plugin) | A drum of curved walls (two arcs) under a hemispherical dome with an oculus. Doors and windows on the curve, a round floor and room. A barrel-vaulted annex. A spline garden wall. A turret with an onion dome, built by the Castle Turret plugin. ADA 2010 door clearances. |

Open one with `reviera gui test-data/samples/observatory.ifc`.
