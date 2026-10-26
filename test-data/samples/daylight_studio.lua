-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Daylight studios
-- description: Two identical studios with large south windows under a flat roof slab; the east one faces a neighbouring block 8 m high, 5 m away. Run Daylight Factor to compare them, a Solar Study for the roof's PV potential, and PVGIS Check to cross-check it.
--
-- Reviera sample model (test-data/samples): two 6 × 6 m studios side by side, each with two
-- 2.4 × 1.5 m south windows (the study's default visible transmittance of 0.7, as the type gives
-- no Pset_DoorWindowGlazingType.VisibleLightTransmittance) and a 300 mm roof slab on a roof level
-- at 3.0 m (the slab, walls and the neighbour obstruct the sky; the window glass transmits); a
-- free-standing wall 8 m high and 7 m long 5 m south of the east studio stands for the
-- neighbouring block. A space in each studio. Sustainability › Daylight & Solar › Daylight
-- Factor (ANA-LIGHT-002, ANA-ENV-006) shows the west studio well daylit and the east studio's
-- front row cut by the block; Show on Model colours each sensor. The site and the synthetic
-- weather year are set for Solar Study (ANA-ENV-002) on the roof and for Annual Energy.

local l1 = reviera.level.list()[1].id
local roof_level = reviera.level.create{ elevation = 3.0, name = "Roof" }
reviera.view.create_plan{ level_ids = { l1 } }
reviera.view.create_3d{}
local plan
for _, v in ipairs(reviera.view.list()) do
  if v.name == "Level 1" and v.kind ~= "3d" then plan = v.id end
end
local function label(x, y, text)
  reviera.text.create{ view_id = plan, position = { x, y }, content = text }
end
local function wall(a, b, height)
  return reviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = height or 3.0 } }
end
local function type_named(name)
  for _, t in ipairs(reviera.type.list()) do
    if t.name == name then return t.id end
  end
end

reviera.location.set{ lat = 51.5, lon = 0.0, elevation = 20.0, time_zone = 0.0, place = "Synthetic Temperate" }

-- The studios: 12 × 6 m on the walls' centrelines, clockwise, a party wall at x = 6 under the
-- roof slab.
local c = { {0, 0}, {12, 0}, {12, 6}, {0, 6} }
local outer = {}
for i = 1, 4 do
  outer[i] = wall(c[i % 4 + 1], c[i])
end
wall({ 6, 0 }, { 6, 6 }, 2.7)

-- Two large south windows in each studio (the study's default visible transmittance, 0.7).
local first = reviera.window.place{ host_wall_id = outer[1], point_along = 12 - 1.7 }
local glazing = reviera.type.duplicate{ type_id = type_named("Window 1200 x 1200"), new_name = "Studio Glazing 2400 x 1500" }
reviera.type.set_param{ type_id = glazing, param = "width", value = 2.4 }
reviera.type.set_param{ type_id = glazing, param = "height", value = 1.5 }
-- The first window made the default type; it takes the studio glazing too.
for _, x in ipairs({ 4.3, 7.7, 10.3 }) do
  reviera.window.place{ host_wall_id = outer[1], point_along = 12 - x, type_id = glazing }
end
reviera.element.set_type{ element_ids = { first }, type_id = glazing }
reviera.door.place{ host_wall_id = outer[3], point_along = 3.0 }
reviera.door.place{ host_wall_id = outer[3], point_along = 9.0 }

-- The roof slab, inside the walls' faces, on the roof level.
local function rect(x0, y0, x1, y1)
  local p = { {x0, y0}, {x1, y0}, {x1, y1}, {x0, y1} }
  local out = {}
  for i = 1, 4 do out[i] = { line = { start = p[i], ["end"] = p[i % 4 + 1] } } end
  return out
end
reviera.floor.create{ loops = { rect(0.1, 0.1, 11.9, 5.9) }, level_id = roof_level }

-- The neighbouring block: a wall 8 m high, 5 m south of the east studio.
wall({ 13.5, -5 }, { 6.5, -5 }, 8.0)

reviera.space.set{ ids = reviera.space.place_auto{ level_id = l1 }, limit_offset = 2.7 }

label(0.5, -1.4, "West studio: open sky")
label(6.5, -1.4, "East studio: the block 8 m high, 5 m away")
label(6.5, -5.8, "Neighbouring block (a wall 8 m high)")
