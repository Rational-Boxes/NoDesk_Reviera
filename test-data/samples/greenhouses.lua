-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Greenhouses
-- description: A Venlo glasshouse range on a knee wall, a three-span gutter-connected range turned to run north–south, a small gable greenhouse, a sawtooth range with vents in its vertical faces, a geodesic dome, and a hoop and a gothic film tunnel, each one Glazed Enclosure of glazed roofs, glazed walls, gutters, vents and a steel frame; lettuce and tomatoes grown under grow lights sized for December.
--
-- Reviera sample model (test-data/samples): the Glazed Enclosure tool (ARC-GLZ-001/002). Each
-- greenhouse is one element (`enclosure.create`) that lays out ordinary Reviera elements and
-- builds them again when a parameter changes:
-- - its roofs: a sloped-glazing footprint roof per span (Greenhouse Roof Glass, 1,125 mm panes);
-- - its walls: curtain walls of Greenhouse Wall Glass (800 mm panes), the end walls attached to
--   every span's roof so the glass fills the gables;
-- - a knee wall under the glass where one is set;
-- - gutters on the outer eaves and a valley gutter between each pair of roofs.
-- The sizes are the forms' provisional defaults where none is given (EXPERT_REVIEW E-44).

local l1 = reviera.level.list()[1].id

-- A Venlo range: 2 trusses of 8 m, each with two 4 m roofs at 22°, 24 m long in 4 m bays, the
-- gutters at 5.5 m on a 600 mm knee wall.
local venlo = reviera.enclosure.create{
  form = "venlo", level_id = l1, origin = {0, 0},
  span = "8 m", spans = 2, length = "24 m", bay = "4 m", eave = "5.5 m", pitch = "22°",
  knee = "600 mm",
  -- A headhouse across its west end for potting and packing: 4 m deep, 4 m high, with a 3 m
  -- door into the range for trolleys.
  headhouse = { depth = "4 m", height = "4 m" },
}

-- Glazing comes from the library (ARC-GLZ-003, `glazing.list_types`): diffuse glass on the
-- multispan, twin-wall polycarbonate on the hobby house, the greenhouse glass elsewhere.
-- A gutter-connected range of three 9.6 m gable spans at 5:12, its length turned to run
-- north from (60, 0): the spans step west from there.
local range = reviera.enclosure.create{
  form = "multispan", level_id = l1, origin = {60, 0}, rotation = "90°",
  span = "9.6 m", spans = 3, length = "16 m", bay = "4 m", eave = "4.5 m",
  roof_glazing = "Diffuse AR Glass 4 mm",
}
-- Side vents along its walls' tops as well as the ridge vents (ARC-GLZ-004): 10 % of the
-- floor, opening to 40° on chain actuators.
reviera.enclosure.set_vents{ enclosure_id = range, kind = "side", target_percent = 10, opening_angle = "40°", actuator = "chain" }

-- A hobby greenhouse: one 3.6 m gable span, 6 m long, eaves at 2.2 m, roof at 25°, on timber
-- glazing bars 50 × 100 mm from the frame library (`frame.list_types`).
local hobby = reviera.enclosure.create{
  form = "gable", level_id = l1, origin = {0, 32},
  span = "3.6 m", length = "6 m", bay = "1.2 m", eave = "2.2 m", pitch = "25°",
  roof_glazing = "Twin-Wall Polycarbonate 10 mm", frame_system = "Timber Glazing Bar 50×100",
}

-- A sawtooth range for a warm climate: three 6.4 m spans, each one 25° slope rising from its
-- gutter at 4 m to a glazed vertical face whose top band of vents (18 % of the span's floor)
-- lets the hot air out on the lee side; 8 m long in 4 m bays.
local saw = reviera.enclosure.create{
  form = "sawtooth", level_id = l1, origin = {30, 32}, span = "6.4 m", spans = 3, length = "8 m", bay = "4 m",
}

-- Two film tunnels, 9.1 m wide and 12 m long on 1.2 m sidewalls, hoops at most 1.5 m apart: a
-- semicircular hoop house, and a gothic one whose two arcs (radius 0.6 × the span) meet in a
-- pointed crown that sheds condensation and snow.
local tunnel = reviera.enclosure.create{
  form = "hoop", level_id = l1, origin = {12, 40}, span = "9.1 m", length = "12 m", eave = "1.2 m",
}
local gothic = reviera.enclosure.create{
  form = "gothic", level_id = l1, origin = {12, 52}, span = "9.1 m", length = "12 m", eave = "1.2 m",
}
-- Roll-up sides on the hoop tunnel's sidewalls, open 0.8 m from the ground (ARC-GLZ-004).
reviera.enclosure.set_vents{ enclosure_id = tunnel, kind = "roll_up", height = "800 mm" }

-- A glazed geodesic dome (ARC-GLZ-006): 3V, 4 m radius, cut at 5/8 of the sphere, toughened
-- glass on 50 × 100 mm aluminium struts; its report gives the strut lengths for fabrication.
local dome = reviera.roof.create_geodesic{
  level_id = l1, center = {50, 40}, radius = "4 m", frequency = 3, truncation = "5/8",
}
local d = reviera.roof.geodesic{ roof_id = dome }
for _, c in ipairs(d.strut_classes) do
  print(string.format("strut %s: %d × %.3f m", c.label, c.count, c.length))
end
print(string.format("dome: %d hubs, %d panels, height %.2f m", d.hubs, d.panels, d.height))

-- Growing layouts (ARC-GLZ-005), clear of the posts: NFT tables along the Venlo range (runs of
-- at most 12 m falling 2 %), rolling benches in the multispan, benches across the hobby house,
-- ebb-and-flow tables in the hoop tunnel.
local layouts = {
  reviera.growing.layout{ enclosure_id = venlo, system = "nft", direction = "along" },
  reviera.growing.layout{ enclosure_id = range, system = "rolling_benches", direction = "along" },
  reviera.growing.layout{ enclosure_id = hobby, system = "benches", width = 0.9, aisle = 0.6, main_aisle = 0.9 },
  reviera.growing.layout{ enclosure_id = tunnel, system = "ebb_flow", direction = "along" },
  -- High-wire gutters for tomatoes in the sawtooth range: hanging gutters 1.25 m apart falling
  -- 0.5 % to their drain ends, under crop wires at 3.25 m.
  reviera.growing.layout{ host_id = saw, system = "high_wire" },
}
for _, l in ipairs(layouts) do
  local s = reviera.growing.summary{ layout_id = l }
  print(string.format("%s: %d pieces, %.0f m² growing (%.0f %% of the floor)", s.system, s.pieces, s.growing_area, s.share))
end

-- Growing climate and crop light (ARC-GLZ-008/009), at a site in the western Netherlands:
-- lettuce in the Venlo range (17 mol/m²·d), tomatoes in the sawtooth (25 mol/m²·d). Each
-- enclosure's December light through its glass falls far short, so grow lights are hung under
-- the gutters, enough to make up the darkest month over the crop's photoperiod.
reviera.location.set{ lat = 52.0, lon = 4.4, place = "Westland" }
reviera.enclosure.set_climate{ enclosure_id = venlo, crop = "lettuce" }
reviera.enclosure.set_climate{ enclosure_id = saw, crop = "tomato" }
for _, e in ipairs({ venlo, saw }) do
  local lights = reviera.growing.lights{ enclosure_id = e }
  local s = reviera.growing.lights_summary{ lights_id = lights }
  local l = reviera.enclosure.crop_light{ enclosure_id = e }
  local dark = l.months[l.darkest_month]
  print(string.format("grow lights: %d fixtures, %.0f µmol/m²·s, %.0f W/m²; month %d: %.1f natural + %.1f lamps mol/m²·d",
    s.fixtures, s.average_ppfd, s.power_density, l.darkest_month, dark.natural, dark.lamps))
end

for _, e in ipairs({ venlo, range, hobby, saw, tunnel, gothic }) do
  local info = reviera.enclosure.get{ enclosure_id = e }
  print(string.format("%s: %.0f m², ridge %.2f m, %d roofs, %d gutters",
    info.recipe.form, info.floor_area, info.ridge, #info.roofs, #info.gutters))
end
print("Greenhouses built")
return true
