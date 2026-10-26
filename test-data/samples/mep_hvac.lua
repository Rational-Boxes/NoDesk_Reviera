-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Office air system
-- description: An open-plan office split into two spaces by a separation line, each its own system-zone, served by an air handling unit through a supply main, four diffusers connected into it and a return grille.
--
-- Reviera sample model (test-data/samples): MEP spaces placed automatically on either side of
-- a space separator (MEP-SPACE-001), an Office space type with its loads and set points
-- (MEP-SET-004), a system-zone per space with the air handler assigned (MEP-ZONE-001), an air
-- handling unit joined to a 500 × 300 rectangular supply main by Connect Into, four 600 diffusers at
-- 150 L/s each joined to the main the same way, a sound attenuator on the main, a return grille,
-- and the supply air system (MEP-DUCT-009). Two rooms on the same line: space.airflow (and a
-- room schedule's Supply Airflow) shows 300 L/s supplied to each; Analyze › Loads and
-- Sustainability › Heating & Cooling Loads run on the spaces.

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
local function wall(a, b)
  return reviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 3.4 } }
end

-- The office: 14 × 9 m on the walls' centrelines, with windows on the long sides.
local c = { {0, 0}, {14, 0}, {14, 9}, {0, 9} }
local walls = {}
for i = 1, 4 do
  walls[i] = wall(c[i % 4 + 1], c[i])
end
for _, x in ipairs({ 3, 7, 11 }) do
  reviera.window.place{ host_wall_id = walls[1], point_along = 14 - x }
  reviera.window.place{ host_wall_id = walls[3], point_along = x }
end
reviera.door.place{ host_wall_id = walls[4], point_along = 4.5 }

-- Two spaces on either side of a separation line, typed as offices.
reviera.space_separator.create{ level_id = l1, lines = { { start = { 7, 0 }, ["end"] = { 7, 9 } } } }
local spaces = reviera.space.place_auto{ level_id = l1 }
reviera.space.set{ ids = spaces, limit_offset = 3.4 }
reviera.space_type.create{ name = "Office", values = {
  lighting_load_density = 8.0, power_load_density = 10.0, area_per_person = 10.0,
  sensible_heat_per_person = 75.0, latent_heat_per_person = 55.0, outside_air_per_person = 10.0,
  heating_set_point = 20.0, cooling_set_point = 24.0,
} }
reviera.space.set_type{ element_ids = spaces, type_id = "office" }
-- Rooms on the same line, for the rooms' airflow (Room › Supply Airflow in schedules).
reviera.roomseparator.create{ level_id = l1, lines = { { start = { 7, 0 }, ["end"] = { 7, 9 } } } }
reviera.room.place{ level_id = l1, point = { 3.5, 4.5 } }
reviera.room.place{ level_id = l1, point = { 10.5, 4.5 } }

-- The air system: the unit by the south wall, turned so its supply outlet faces the main along
-- the middle at 3.1 m.
local t = reviera.mep_type.load_starters{ names = { "Air Handling Unit", "Supply Diffuser 600", "Return Grille 600" } }
local ahu = reviera.mech_equipment.place{ type_id = t["Air Handling Unit"], point = { 5.5, 1.8 }, level = l1, rotation = "90°" }
local main = reviera.duct.create{ points = { { 1.2, 5.5, 3.1 }, { 12.8, 5.5, 3.1 } }, level = l1, shape = "rectangular", width = 0.5, height = 0.3 }
local main_id = main.segments[1]
local pieces = { main_id }
local rep = reviera.mep.connect_into{ element_id = ahu, connector_index = 0, target_id = main_id }
for _, s in ipairs(rep.segments) do pieces[#pieces + 1] = s end

-- Four diffusers, two in each space, joined into the main.
local diffusers = {}
for _, p in ipairs({ { 2.5, 3.0 }, { 4.0, 7.5 }, { 9.5, 3.0 }, { 11.5, 7.5 } }) do
  local d = reviera.air_terminal.place{ type_id = t["Supply Diffuser 600"], point = p, level = l1, offset = 2.5, flow = 0.15 }
  diffusers[#diffusers + 1] = d.id
end
-- Each Connect Into splits the main at its tee: try the pieces of the main in turn.
for _, d in ipairs(diffusers) do
  local joined, why = false, {}
  for _, target in ipairs(pieces) do
    local ok, rep = pcall(reviera.mep.connect_into, { element_id = d, connector_index = 0, target_id = target })
    if not ok then why[#why + 1] = tostring(target) .. ": " .. tostring(rep) end
    if ok then
      for _, s in ipairs(rep.segments) do pieces[#pieces + 1] = s end
      joined = true
      break
    end
  end
  assert(joined, "a diffuser could not be connected into the main: " .. table.concat(why, "; "))
end
reviera.duct_accessory.place{ kind = "sound_attenuator", point = { 7.6, 5.5 }, length = 0.6 }
reviera.air_terminal.place{ type_id = t["Return Grille 600"], point = { 7.0, 8.0 }, level = l1, offset = 2.5, flow = 0.4 }
reviera.system.create{ kind = "duct", element_ids = diffusers, name = "Supply Air" }

-- A system-zone per space, both served by the air handler.
for _, b in ipairs({ { { 0, 0 }, { 7, 0 }, { 7, 9 }, { 0, 9 } }, { { 7, 0 }, { 14, 0 }, { 14, 9 }, { 7, 9 } } }) do
  local z = reviera.system_zone.create{ level_id = l1, boundary = b, name = (b[1][1] == 0) and "West" or "East" }
  reviera.system_zone.assign{ zone_id = z, equipment_or_system_id = ahu }
end

label(0.5, -1.2, "Air handler, Connect Into the 500 x 300 main, attenuator; four diffusers at 150 L/s")
label(7.2, 9.6, "Space separator: two spaces, two system-zones (West, East)")
