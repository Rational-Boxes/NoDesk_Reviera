-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Energy house
-- description: A four-room bungalow set up for the energy tools: a site and a synthetic weather year, layered and insulated exterior walls, large south and small north windows with their U-values and solar gain, residential spaces with loads and set points — ready for Heating & Cooling Loads, Annual Energy, Embodied Carbon and the gbXML and HBJSON exports.
--
-- Reviera sample model (test-data/samples): the project location at 51.5° N, 0° E with the
-- sample's synthetic weather year (reviera_synthetic.epw, made by make_weather.py, kept in
-- energy_house.resources/weather; ANA-EM-001); exterior walls of render, wood-fibre insulation,
-- block and plaster (materials for Embodied Carbon, ANA-ENV-001) with U 0.18 W/m²K, partitions
-- at the default U; two window types, south 2.4 × 1.5 m triple glazing (U 0.8, g 0.5) and north
-- 1.2 × 1.2 m (U 1.1, g 0.6), and the front door at U 1.2 (energy_model.set_thermal); a ground
-- floor slab; four MEP spaces of a Dwelling type (lighting 5 W/m², equipment 4 W/m², 25 m² a
-- person, 21 °C heating and 25 °C cooling set points, 8 L/s a person of outdoor air). The
-- energy model (Sustainability › Energy › Energy Model) has 4 spaces, 8 exterior walls, 4
-- shared partitions, 6 windows and a door; the roofs are at the default U.

local l1 = reviera.level.list()[1].id
reviera.view.create_plan{ level_ids = { l1 } }
reviera.view.create_3d{}
local plan
for _, v in ipairs(reviera.view.list()) do
  if v.name == "Level 1" and v.kind ~= "3d" then plan = v.id end
end
local function label(x, y, text)
  reviera.text.create{ view_id = plan, position = { x, y }, content = text }
end
local function type_named(name)
  for _, t in ipairs(reviera.type.list()) do
    if t.name == name then return t.id end
  end
end
local function wall(a, b)
  return reviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 2.7 } }
end

-- The site: southern England's latitude (the weather year is imported with the sample).
reviera.location.set{ lat = 51.5, lon = 0.0, elevation = 20.0, time_zone = 0.0, place = "Synthetic Temperate" }

-- The bungalow: 12 × 9 m on the walls' centrelines, clockwise so each exterior faces out.
local c = { {0, 0}, {12, 0}, {12, 9}, {0, 9} }
local outer = {}
for i = 1, 4 do
  outer[i] = wall(c[i % 4 + 1], c[i])
end
-- outer[1] is the south wall (12,0)→(0,0), [2] east, [3] north (0,9)→(12,9), [4] west.
local exterior = reviera.type.duplicate{ type_id = type_named("Generic - 200mm"), new_name = "Exterior - Insulated Block 300" }
reviera.compound.set_layers{ type_id = exterior, layers = {
  { ["function"] = "finish1", material = "Lime Render", thickness = "15 mm" },
  { ["function"] = "thermal_air", material = "Wood Fibre Insulation", thickness = "160 mm" },
  { ["function"] = "structure", material = "Concrete Block", thickness = "110 mm" },
  { ["function"] = "finish2", material = "Gypsum Plaster", thickness = "15 mm" },
} }
reviera.element.set_type{ element_ids = outer, type_id = exterior }
reviera.energy_model.set_thermal{ type_id = exterior, u_value = 0.18 }
wall({ 6, 0 }, { 6, 9 })
wall({ 0, 4.5 }, { 6, 4.5 })
wall({ 6, 4.5 }, { 12, 4.5 })

-- Windows: one small north window a room (the first makes the default type), two large south
-- windows a room of a wider type; a front door.
for _, x in ipairs({ 3.0, 9.0 }) do
  reviera.window.place{ host_wall_id = outer[3], point_along = x }
end
local north = type_named("Window 1200 x 1200")
local south = reviera.type.duplicate{ type_id = north, new_name = "South Glazing 2400 x 1500" }
reviera.type.set_param{ type_id = south, param = "width", value = 2.4 }
reviera.type.set_param{ type_id = south, param = "height", value = 1.5 }
for _, x in ipairs({ 1.7, 4.3, 7.7, 10.3 }) do
  reviera.window.place{ host_wall_id = outer[1], point_along = 12 - x, type_id = south }
end
reviera.door.place{ host_wall_id = outer[4], point_along = 6.8 }
reviera.energy_model.set_thermal{ type_id = south, u_value = 0.8, shgc = 0.5 }
reviera.energy_model.set_thermal{ type_id = north, u_value = 1.1, shgc = 0.6 }
reviera.energy_model.set_thermal{ type_id = type_named("Single Door 900 x 2100"), u_value = 1.2 }

-- The ground floor slab.
reviera.floor.create{ loops = { {
  { line = { start = { 0, 0 }, ["end"] = { 12, 0 } } }, { line = { start = { 12, 0 }, ["end"] = { 12, 9 } } },
  { line = { start = { 12, 9 }, ["end"] = { 0, 9 } } }, { line = { start = { 0, 9 }, ["end"] = { 0, 0 } } },
} }, level_id = l1 }

-- Four spaces of a dwelling type.
local spaces = reviera.space.place_auto{ level_id = l1 }
reviera.space.set{ ids = spaces, limit_offset = 2.7 }
reviera.space_type.create{ name = "Dwelling", values = {
  lighting_load_density = 5.0, power_load_density = 4.0, area_per_person = 25.0,
  sensible_heat_per_person = 70.0, latent_heat_per_person = 45.0, outside_air_per_person = 8.0,
  heating_set_point = 21.0, cooling_set_point = 25.0,
} }
reviera.space.set_type{ element_ids = spaces, type_id = "dwelling" }

label(0.2, -1.2, "South: 2.4 × 1.5 m triple glazing (U 0.8, g 0.5); walls U 0.18; synthetic weather at 51.5° N")
label(0.2, 9.8, "North: 1.2 × 1.2 m windows (U 1.1); four Dwelling spaces, 21 °C / 25 °C")
