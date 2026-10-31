# Glazed enclosures: greenhouses, solariums and atriums

**Status:** Draft 0.3 · 2026-10-28 · the owner answered §10 (commercial first, codes for common
jurisdictions, full load modelling with the analysis tools, commercial growing, both dome kinds).
Nothing here is built yet; the spec sections follow from this plan. Reviera-defined types and
property sets in its own namespace for what IFC lacks (owner, 2026-10-28) are in §8a.
**Owner's requests (2026-10-28):** "a bespoke greenhouse and solarium set of tools"; "integrating
the existing tools for the greenhouse specialization"; "also suitable for large integrated
atriums in offices, hotels, etc."; "with calculations on HVAC, energy, etc. impacts"; "louvres,
baffles, etc. for advanced sunlight management".
**Research:** `research/09_greenhouses_solariums.md` (sources G001–G087) and
`research/10_atriums_solar_shading.md` (sources A001–…) and
`research/11_glazed_enclosure_codes_growing.md` (sources C001–…, codes by jurisdiction and
commercial growing), all public sources only and logged in `RESEARCH_LOG.md`. Source IDs are cited in square brackets.

---

## 1. What the toolset is for

One family of tools for **glazed enclosures**: rooms and buildings whose roof, and often walls,
are mostly glass or clear plastic. They share one structure (a light frame carrying glazing,
gutters and vents) and one set of problems (heat gained and lost through glass, light,
ventilation, condensation, snow and wind on a light frame, safety glazing), so one toolset can
serve:

- **Greenhouses:** hobby houses, lean-tos, tunnels and commercial ranges.
- **Solariums, sunrooms and conservatories** on houses, and **orangeries** with roof lanterns.
- **Atriums:** glazed courtyards and streets inside offices, hotels, malls and hospitals.

Each tool is built from Reviera's existing elements, not a parallel set of them. A greenhouse made
with the new tool is made of ordinary walls, sloped-glazing roofs, curtain walls, gutters, doors
and railings, which every other tool, schedule, analysis and IFC export already understands. The
new part is the **enclosure**: one element that knows the form and its parameters, lays out its
parts, keeps them consistent when a parameter changes, and runs the checks and analyses that
apply to glazed rooms.

## 2. What Reviera already has

| Need | Existing tool | Status |
|---|---|---|
| Glazed roof planes | Sloped glazing: a roof of a Sloped Glazing type becomes panels and mullions (ARC-CW-008) | Built |
| Vertical glazing, doors in it | Curtain walls, grids, curtain doors (ARC-CW-001…006) | Built |
| Curved roofs | Arched and vaulted roofs, tapering vaults (ARC-ROOF-008); domes (ARC-ROOF-009) | Built (solid; not glazed yet) |
| Roof planes, eaves | Footprint and extrusion roofs, eaves bearing (ARC-ROOF-001, -010) | Built |
| Knee (dwarf) walls | Walls, wall tops attaching to roofs | Built |
| Gutters, downspouts, sizing | Gutters and outlets, sized against draining roof area (MEP-PIPE-010) | Built |
| Decks, railings, cresting-like rails | Decks, railings, gates (ARC-…, 3.7.7–3.7.8b) | Built |
| Members | Structural framing, columns, connections | Built |
| Components (benches, fans, heaters) | Component Libraries, mesh objects | Built |
| Heating and cooling loads | Design-day loads, run as jobs (ANA-HCL-001, ANA-ENV-004) | Built; sloped roof areas missing from the energy model |
| Annual energy | Hourly lumped model; EnergyPlus provider (ANA-ENV-005, -007) | Built; no shading devices |
| Daylight, sun | Daylight factor; solar studies on faces (ANA-LIGHT-002, ANA-SOLAR-001) | Built |
| Remote compute | Heavy runs as compute jobs (DM-RPC-011) | Built |

So the plan is mostly integration: one enclosure tool driving these, a glazing and frame type
library, vents and shading devices, and analyses that know about glass.

## 3. The Glazed Enclosure tool

**Architecture › Glazed Enclosure** (and `enclosure.create`): choose a form, draw or pick its
footprint (sketch, pick walls of the host building, or typed corners, as every tool takes), and
set its parameters in a dialog with a live preview. The enclosure then owns its parts:

- **Parts it lays out:**
  - knee walls (a wall type and height, or none);
  - the glazed roof: sloped-glazing roof planes, or a glazed arch, gothic arch or dome;
  - glazed walls as curtain walls;
  - corner and ridge posts, rafters or glazing bars, purlins, ridge and eaves beams, as structural
    members or as the glazing system's mullions;
  - gutters and downspouts on the eaves, internal valley gutters in multispans;
  - doors in the glazed walls;
  - vents (§5), and optionally cresting and finials.
- **Edits:** changing a parameter regenerates the parts and keeps the user's edits to them where it
  can (a door moved along a wall, a changed panel type). A part can be **detached** to edit it
  freely; the enclosure then leaves it alone.
- **Grips:** in plans and 3D, the footprint edges, the eaves and ridge heights, and the facet count
  of a Victorian end.

### 3.1 Forms

| Form | Roof | Typical values to start from | IFC roof type |
|---|---|---|---|
| Lean-to | One slope against a host wall | 10° roof, 600 mm dwarf wall [G072]; 2.5°–30° range [G048] | SHED_ROOF |
| Gable (even span) | Two slopes, gable ends | 4:12–6:12 (18°–27°) [G002] | GABLE_ROOF |
| Uneven span | Two slopes, own pitch each | as gable [G067] | GABLE_ROOF |
| Edwardian / Georgian | Hipped, square front | as gable [G052] | HIP_ROOF |
| Victorian | Hipped with a 3- or 5-facet bay end | 3 or 5 facets [G052] | HIP_ROOF |
| P-shape, T-shape | Combinations of the above | [G052] | (aggregate) |
| Orangery | Flat perimeter with a roof lantern | perimeter ≥ 400 mm; lantern 25° on a ≥ 150 mm kerb [G050, G051] | FLAT_ROOF + PAVILION_ROOF |
| Hoop / Quonset tunnel | Half-round or raised-sidewall arch | 6–9 m wide, hoops at 1.2–1.8 m [G004, G069] | BARREL_ROOF |
| Gothic arch | Pointed arch | as hoop [G070] | RAINBOW_ROOF |
| Gutter-connected multispan | Repeated gable or arch bays sharing gutters | gutter height 4.3–4.9 m [G005] | GABLE_ROOF / BARREL_ROOF per bay |
| Venlo | Narrow-roof glass multispan | ≈22° pitch, 4 m bays, 6.4/8/12 m trusses [G006, G007, G009] | GABLE_ROOF |
| Sawtooth | Serrated multispan with vertical vents | [G003, G059] | (per bay) |
| Dome / geodesic | Faceted dome | 2V, 3V, 4V frequency [G060] | DOME_ROOF |
| Atrium roof | Over a courtyard between building wings | §6 | by form |

Defaults are the research's typical values and go in `EXPERT_REVIEW.md` for an expert to confirm
(the minimum glazed pitch especially: 2.5° system minimum, 5° for polycarbonate run-off, about 10°
against condensate drip [G048, G034, G043]).

### 3.2 Glazing and frame types

A library of **glazing build-ups**, each with its light transmission, U-value, solar heat gain
coefficient (g-value), safety class and weight: horticultural float, toughened, laminated, low-e
double and triple units, twin- and multiwall polycarbonate, acrylic, and single or inflated double
polyethylene film [§3 of research 09]. Optical values go in `Pset_MaterialOptical` on the material;
the safety class and polycarbonate wall structure, which IFC has no property for on plates, go in
a Reviera property set [G056 et al.].

**Frame types:** aluminium glazing-bar systems, galvanised steel tube, timber and uPVC, each with
member sections and the deflection limits that apply (for example L/60, or L/175 per lite where a
member carries glass edges, for U.S. sunroom aluminium [§5.2]).

## 4. Interior: growing and use

- **Benches and aisles:** a layout tool that fills the floor with benches of a chosen width and
  aisles between them (Component Library benches), reporting growing area against floor area.
- **Climate equipment** as components: fans, pad walls for evaporative cooling, heaters,
  horizontal energy curtains and shade screens (§5.2), circulation fans, irrigation lines later.
- **Solariums and atriums** use rooms and spaces as now; the enclosure tells the energy model the
  space is a glazed one (§7).

## 5. Vents and sunlight management

### 5.1 Vents

Ridge vents, side and gable vents, roll-up sides on tunnels, and roof louvres, each an operable
part of the glazing (`IfcWindow` in the roof or wall, or `IfcPlate` with an actuator), sized as a
share of floor area. The rule of thumb is roof vents and side vents each 15–20 % of floor area;
fan ventilation at 8 cfm/ft² (about one air change a minute) with inlets at least 1.25 × fan area;
pad area by airflow ÷ 250 for 100 mm pads or ÷ 400 for 150 mm pads [§4 of research 09].

### 5.2 Sunlight management (louvres, baffles, screens)

Planned devices:

- **External fixed:** horizontal louvres and brise-soleil, vertical fins, egg-crate, overhangs,
  perforated and fritted screens.
- **Operable:** motorised louvres and shutters, sun-tracking blades.
- **Roof devices:** baffles and light-diffusing blades under atrium and greenhouse roofs, light
  shelves at the edges of floors facing an atrium.
- **Screens and blinds:** greenhouse shade and energy screens (by shade percentage), interior and
  exterior blinds, electrochromic glazing as a glazing state.

Each device is placed on a face of an enclosure, curtain wall, window or roof (by face, like other
tools), with its blade depth, spacing, angle and length, and a **design aid** that shows the sun
path at the site and the cut-off angle the blades give. Its effect is carried into the analyses
(§7) as a shading factor that changes over the day, not a fixed number.

- **Geometry.** The profile (vertical shadow) angle follows from the sun's altitude and its
  azimuth relative to the façade, tan VSA = tan(altitude) / cos(γ). An overhang's projection for
  full shading of a height h is h / tan VSA, and a fin's depth for a spacing s is s / tan HSA. A
  louvre's cut-off angle follows from its spacing and depth [A038–A040]. Public sources disagree on
  the louvre formula's sign convention, so it goes to expert review.
- **Effect on solar gain.** The total solar energy transmittance of glazing with a device (g_tot),
  or a reduction factor F_c on the glazing's own g-value. Published tables give about 0.05–0.30
  g_tot for external devices, and F_c ≈ 0.25 for ventilated external louvres against 0.75–0.9 for
  interior blinds [A035, A036]. A detailed calculation follows the public summaries of EN 13363
  and EN ISO 52022 [A037].
- **Effect on daylight.** The device is geometry in the daylight ray casting, with its slats'
  reflectance and transmittance (IFC's `Pset_ShadingDeviceCommon` has these) [A001].
- **Control.** For operable devices and vents, a small rule set the energy and daylight runs
  follow hour by hour:
  - track the sun to the cut-off slat angle (solar position to NREL's SPA accuracy);
  - deploy above an irradiance or light threshold, after a delay (2 min is typical);
  - retract in wind by the device's EN 13659 wind class, with a hold (12 min);
  - close vents in rain and open smoke vents on a fire alarm;
  - limit sun penetration into the floor (0.9 m at desks, 1.8 m near circulation in one tested
    building) and window luminance (about 1,800–2,200 cd/m²) [A036, A047–A049].

## 6. Atriums

- **Forms.** Central (surrounded by floors), linear (a glazed street), attached (along one side of
  the building) and edge atriums; roofs as ridge, pyramid, barrel vault, space frame or ETFE
  cushions, over an opening through the building's floors [A045, A055, A059].
- **Atrium roof** over the opening, using the enclosure's parts sized for larger spans, with the
  frame in the structural analysis tools.
- **Atrium space:** an `IfcSpace` spanning the storeys it passes through, grouped by
  `IfcSpatialZone` (fire safety, thermal, ventilation zones), with its balconies, railings and edge
  glazing, so analyses treat it as one tall stratified volume [A001].
- **Fire and smoke checks** from public sources:
  - IBC §404: smoke control when an atrium connects more than two storeys; no ceiling sprinklers
    needed above 55 ft; 1 h separation, or glass walls wetted by sprinklers 100–300 mm from the
    glass at ≤ 1.8 m spacing; at most three storeys open to it; 200 ft travel within it [A003];
  - smoke filling, plume and exhaust rates by the published NFPA 92B (1991) equations with t² design
    fires of 2, 5 or 25 MW [A004, A006]; the current edition's plugholing and spill-plume constants are
    paywalled, so the user enters them with the edition named;
  - UK: Approved Document B points to BS 9999 for atriums; the NHS guide's roof vents of 10 % of
    plan area up to 18 m high, mechanical extract above; smoke vents rated by their declared
    discharge coefficient (EN 12101-2), not an assumed 0.6 [A008–A010, A012].
- **Environmental behaviour:** stack ventilation through the atrium (neutral plane, opening areas);
  stratification; daylight to the floors around it (a glazed roof cuts the daylight factor by about
  45 %, and its well index sets how far down it reaches); cold downdraught from tall glazing, with
  the published correlations and the effect of mullions and transoms [A013–A018].
- **Energy-code checks:** ASHRAE 90.1 window-to-wall (40 %) and skylight-to-roof (3 %) limits per
  space-conditioning category; IECC's minimum skylight area for large high spaces, which names
  atriums, by its effective-aperture formula; UK Part L's top-lit reference for zones above 6 m
  [A026, A027, A029, A030].

## 7. Analyses: HVAC, energy and the rest

The enclosure feeds Reviera's existing engines rather than adding new solvers wherever it can.
Heavy runs go out of process as compute jobs, as the other analyses do.

| Analysis | How | Builds on |
|---|---|---|
| Quick heat loss of a greenhouse | The published table method: Q = U·A·ΔT·(construction and wind factors) + 0.018·N·V·ΔT for air leakage [G011, G013] | New, instant |
| Design heating and cooling loads | The existing loads with glazed roofs and walls, sloped roof areas and shading devices | `hc_loads.*`; fills the energy model's *sloped roof areas* gap |
| Annual energy and running cost | The existing hourly model and EnergyPlus provider, with shading devices and their control, vents, energy curtains | `energy.*` |
| Impact on the host building | A buffer-space comparison: the house or office with and without the enclosure, heated or unheated, open or separated | `energy.*` (two runs) |
| Overheating | Hours above a threshold in the enclosure, with and without shading and ventilation | `energy.*` |
| Ventilation sizing | Vent area, fan airflow and pad area by the rules in §5.1; natural ventilation by stack and wind | New |
| Daylight and light transmission | Daylight factor through the glazing types and shading devices; for greenhouses, light reaching the crop | `daylight.*` |
| Sun and shading | Solar study on the enclosure; shading masks of the devices | `solar.*` |
| Condensation | Surface temperature of the glazing against the inside dew point; roof pitch against drip | New |
| Snow and wind on the frame | Simplified checks with slope factors from the public tables, flagged as preliminary, with the structural analysis tools for the frame | New + structural analysis |
| Gutter and downspout sizing | Existing check, with internal valley gutters | `MEP-PIPE-010` |
| Code checks | UK conservatory exemption (≤ 30 m², ground level, thermal separation, no heating from the house), UK permitted heights, IRC sunroom (> 40 % glazed; category I–V), safety glazing in sloped and overhead positions and critical zones, IECC isolated-sunroom U-values | New (rule sets, per jurisdiction) |

Results are written to the spaces and the enclosure (as the loads already are), shown in the
Analysis pane and in reports, and go stale when the model or weather change.

## 8. IFC

| Part | IFC | Notes |
|---|---|---|
| Enclosure | `IfcElementAssembly` (or `IfcBuildingElementProxy` until an assembly type fits) aggregating its parts | Reviera recipe for the form and parameters |
| Glazed roof | `IfcRoof` (type by form) aggregating `IfcPlate` (CURTAIN_PANEL or SHEET) and `IfcMember` (RAFTER, PURLIN, MULLION) | As sloped glazing is now |
| Glazed walls | `IfcCurtainWall` with `IfcPlate`, `IfcMember` | As now |
| Frame members | `IfcMember`, `IfcColumn`, `IfcBeam`; IFC4.3 adds ARCH_SEGMENT and TIEBAR | [G056] |
| Vents | `IfcWindow` (roof or wall) with operation type; `IfcActuator` for motorised ones | |
| Gutters | `IfcPipeSegment` GUTTER with `Pset_PipeSegmentTypeGutter`; `IfcStackTerminal` RAINWATERHOPPER | As MEP-PIPE-010 |
| Shading devices | `IfcShadingDevice`: JALOUSIE (louvres), SHUTTER, AWNING, or USERDEFINED with ObjectType `Fin`, `BriseSoleil`, `Baffle`, `LightShelf`, `Screen` (IFC has no LOUVER type) | [A001]; §8a |
| Motorised devices, sensors | `IfcActuator` (SUNBLINDACTUATOR; FailPosition, ManualOverride), `IfcSensor` (RADIATION, WIND, RAIN, LIGHT), assigned to the device | [A001] |
| Atrium | `IfcSpace` spanning storeys, in `IfcSpatialZone`s (FIRESAFETY, THERMAL, VENTILATION) | [A001] |
| Glazing optics | `Pset_MaterialOptical` on the material; §8a for what IFC lacks | [G056] |

## 8a. Reviera's own types and properties

IFC has no entity for a glazed enclosure, a growing gutter or a light shelf, and no property for a
shading device's g_tot or a glazing unit's safety class. A valid IFC file cannot add entity
classes, so, as the owner suggests, the toolset defines **its own types in Reviera's namespace**,
each carried by the nearest IFC entity, following the rules Reviera already uses (spec Ch 2:
categories from the IFC class, `PredefinedType` and `ObjectType`; Reviera data in `Reviera_*`
property sets; the building still readable by a tool that ignores them):

- **Types.** New categories in the category registry, each with its IFC carrier:

  | Reviera type | IFC carrier |
  |---|---|
  | Glazed enclosure | `IfcElementAssembly` USERDEFINED, ObjectType `GlazedEnclosure` |
  | Louvre, fin, brise-soleil, baffle, light shelf, screen | `IfcShadingDevice` (as §8) |
  | Growing gutter (NFT, high-wire), bench, rolling bench | `IfcFurniture` or `IfcBuildingElementProxy` USERDEFINED, ObjectType `GrowingGutter`, `Bench`, `RollingBench` |
  | Pad wall, energy curtain, shade screen | `IfcBuildingElementProxy` / `IfcShadingDevice` USERDEFINED |
  | Vent | `IfcWindow` with `Reviera_Vent` |
  | Atrium | `IfcSpace` with ObjectType `Atrium` |

- **Property sets** in Reviera's namespace for the engineering data IFC has no home for, shown in
  Properties and schedules like any other, for example:
  - `Reviera_GlazingUnit`: safety class, polycarbonate wall structure, light transmission,
    horticultural diffusion (haze);
  - `Reviera_ShadingPerformance`: g_tot, F_c, cut-off angle, blade depth, spacing, tilt and tilt
    range, wind class, control mode;
  - `Reviera_EnclosureDesign`: form, design life class, importance factor, vent ratio;
  - `Reviera_GrowingClimate`: crop, temperature and humidity setpoints, target daily light
    integral, CO₂ setpoint;
  - `Reviera_Vent`: free area, discharge coefficient, actuator, smoke-vent rating.
- **Published definitions.** Each property set is defined once, machine-readably, with names,
  types, units and descriptions, published under the MIT licence with Reviera's other schemas (as
  the interfaces policy asks), so other tools can read them. Publishing them in the buildingSMART
  Data Dictionary (bSDD) under a Reviera domain is a later option.
- **Standard first.** Where a standard property exists it is used instead (`Pset_ShadingDeviceCommon`
  transmittance and reflectance, `Pset_MaterialOptical`, `Pset_PipeSegmentTypeGutter`,
  `Pset_SpaceThermalDesign`), and a Reviera property never repeats one.

## 9. Building it, in phases

Commercial cases come first, then residential (owner, 2026-10-28). Each phase ends with a sample
model and a walkthrough on the Help page (as every ribbon tab has), the spec and README updated,
and the defaults recorded for expert review.

1. **The enclosure and commercial greenhouses.** The enclosure element and tool; the glazing and
   frame library; gable, gutter-connected multispan, Venlo, hoop and gothic tunnels and sawtooth
   forms; gutters (with internal valley gutters), vents, doors; benches, rolling benches and
   aisles, growing gutters (NFT and high-wire); the quick heat loss and ventilation sizing.
   *Sample: a Venlo glasshouse range with a headhouse.*
2. **Hothouse climate and growing.** Heating (pipe rail, under-bench), energy and shade screens,
   pad-and-fan cooling, supplemental lighting laid out for a target daily light integral, CO₂
   enrichment, climate setpoints by crop; crop light (DLI) through the glazing, screens and
   lamps; growing-area schedules. *Built 2026-10-29/30: crop presets and the growing climate
   (ARC-GLZ-008), crop light month by month and grow lights for the darkest month (ARC-GLZ-009),
   energy, shade and blackout screens (ARC-GLZ-010), the quick heat loss (ARC-GLZ-011, a phase-1
   item), ventilation sizing with pads (ARC-GLZ-012, a phase-1 item), CO₂ enrichment sizing
   (ARC-GLZ-013), heating sizing (ARC-GLZ-014) and growing-area schedules (ARC-GLZ-015, built
   2026-10-31). Heating pipes laid round the perimeter and under the benches
   (ARC-GLZ-016, built 2026-10-31). Still to do: pad-and-fan equipment in the model, pipe-rail
   heating, and flow and return headers.*
3. **Sunlight management.** Shading devices on any face (louvres, fins, brise-soleil, baffles,
   light shelves, screens), with the sun-path design aid; their effect in daylight, solar studies
   and loads; operable devices and their control.
4. **Atriums.** Atrium roofs over floor openings (ridge, pyramid, barrel vault, space frame),
   atrium spaces through storeys, smoke vents and the code summaries; stratification and stack
   ventilation. *Sample: a hotel atrium.*
5. **Loads, energy and structure.** Full load modelling, integrated with the environmental,
   sustainability and structural analysis tools (owner, 2026-10-28): the glazed enclosure in the
   energy model (sloped and curved glazed surfaces, shading devices and their control, vents,
   screens) for design loads, annual energy and the EnergyPlus provider; the buffer-space
   comparison against the host building; overheating; and the frame in the structural analysis
   tools with snow (drift and sliding off glazing), wind and crop loads by the greenhouse
   structural standards' structure (design life classes), with load combinations from the
   project's code.
6. **Residential glazed rooms.** Lean-to, Edwardian, Victorian (3 and 5 facets), P- and T-shapes,
   orangery with a roof lantern, solariums on knee walls. *Sample: a Victorian conservatory and
   an orangery on the energy house.*
7. **Building codes.** Rule sets for the common jurisdictions researched (U.S. IBC/IRC/IECC, UK,
   Canada, EU and Eurocodes, Netherlands, Australia, New Zealand), checking occupancy, areas,
   heights, separation, plastics and skylight limits, safety glazing zones, and energy limits.
   Each rule set is data, so more jurisdictions can be added. The checks for each phase's forms
   ship with that phase; this phase completes the set.

**Domes:** both faceted geodesic domes (frequency 2V–4V, struts and hubs) and glazing on the smooth
domes and vaults that exist now (owner, 2026-10-28), in phase 1 for vaults and phase 6 for domes.

## 10. The owner's answers (2026-10-28)

1. **First phase:** "Start with commercial, then continue to residential cases."
2. **Jurisdictions:** "Yes, research and encode common building codes." (research/11; phase 7.)
3. **Structural and loads:** "Yes, advanced load modeling integrates with
   environmental/sustainability/structure analysis tools." (phase 5.)
4. **Commercial growing:** "Yes, include commercial grows/hothouse cases." (phase 2.)
5. **Domes:** "Support both": faceted geodesic glazing and glazing on smooth domes and vaults.
