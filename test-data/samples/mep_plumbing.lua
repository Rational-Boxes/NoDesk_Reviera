-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Washroom plumbing and sprinklers
-- description: A washroom with lavatories, WCs, a urinal and a floor drain, a sanitary drain sloped at 1% below the floor and the fixtures' sanitary system, a cold-water main with a gate valve, a water heater, and a wet sprinkler branch with pendent heads.
--
-- Riviera sample model (test-data/samples): plumbing fixtures placed against the wall's face
-- (MEP-PIPE-006); a 100 mm sanitary main sloped at 1% below the floor (MEP-PIPE-002) and the
-- sanitary system of the fixtures and the floor drain; a 28 mm domestic cold-water main high on the wall with a gate valve
-- (pipe accessories); a water heater; the sanitary and domestic cold water systems; a wet
-- sprinkler branch at 3.0 m with three pendent heads joined into it by Connect Into, its tees and
-- elbows made as it goes (MEP-PIPE-004, MEP-PIPE-007).

local l1 = riviera.level.list()[1].id
riviera.view.create_plan{ level_ids = { l1 } }
riviera.view.create_3d{}
local plan
for _, v in ipairs(riviera.view.list()) do
  if v.name == "Level 1" and v.kind ~= "3d" then plan = v.id end
end
local function label(x, y, text)
  riviera.text.create{ view_id = plan, position = { x, y }, content = text }
end
local function wall(a, b)
  return riviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 3.3 } }
end

-- The washroom: 7 × 4.5 m on the walls' centrelines.
local c = { {0, 0}, {7, 0}, {7, 4.5}, {0, 4.5} }
local walls = {}
for i = 1, 4 do
  walls[i] = wall(c[i % 4 + 1], c[i])
end
riviera.door.place{ host_wall_id = walls[1], point_along = 6.0 }

-- Fixtures along the north wall, a floor drain in the middle, the heater in the south-west corner.
local t = riviera.mep_type.load_starters{ names = { "Lavatory", "Water Closet", "Urinal", "Floor Drain", "Water Heater", "Pendent Sprinkler" } }
local fixtures = {}
for _, f in ipairs({ { "Lavatory", 0.8 }, { "Lavatory", 1.7 }, { "Urinal", 2.7 }, { "Water Closet", 4.0 }, { "Water Closet", 5.2 } }) do
  local id = riviera.plumbing_fixture.place{ type_id = t[f[1]], point = { f[2], 4.2 }, level = l1 }
  fixtures[#fixtures + 1] = id
end
local drain = riviera.plumbing_fixture.place{ type_id = t["Floor Drain"], point = { 2.2, 2.2 }, level = l1 }
local heater = riviera.mech_equipment.place{ type_id = t["Water Heater"], point = { 0.6, 0.6 }, level = l1 }

-- The sanitary main below the floor, falling 1% to the east.
local main = riviera.pipe.create{ points = { { 0.6, 3.0, -0.6 }, { 6.4, 3.0, -0.6 } }, level = l1, diameter = 0.1, slope = 0.01, system_type_id = "sanitary" }
local pieces = {}
for _, s in ipairs(main.segments) do pieces[#pieces + 1] = s end
local function join(element, connector)
  local why = {}
  for _, target in ipairs(pieces) do
    local ok, rep = pcall(riviera.mep.connect_into, { element_id = element, connector_index = connector, target_id = target })
    if ok then
      for _, s in ipairs(rep.segments) do pieces[#pieces + 1] = s end
      return
    end
    why[#why + 1] = tostring(rep)
  end
  error("could not connect " .. tostring(element) .. ": " .. table.concat(why, "; "))
end
-- Connect Into takes a level main: the fixtures join the sloped drain's system logically.
local waste = { drain }
for _, f in ipairs(fixtures) do waste[#waste + 1] = f end
riviera.system.create{ kind = "piping", element_ids = waste, system_type_id = "sanitary", skip_incompatible = true, name = "Sanitary" }

-- Domestic cold water high on the north wall, with a gate valve.
local cold = riviera.pipe.create{ points = { { 0.3, 4.0, 2.6 }, { 6.6, 4.0, 2.6 } }, level = l1, diameter = 0.028, system_type_id = "domestic_cold_water" }
riviera.pipe_accessory.place{ kind = "gate_valve", pipe_id = cold.segments[1] }
riviera.system.create{ kind = "piping", element_ids = fixtures, system_type_id = "domestic_cold_water", skip_incompatible = true, name = "Cold Water" }

-- A wet sprinkler branch at 3.0 m with three pendent heads.
local branch = riviera.pipe.create{ points = { { 0.6, 2.0, 3.0 }, { 6.4, 2.0, 3.0 } }, level = l1, diameter = 0.032, system_type_id = "fire_protection_wet" }
pieces = {}
for _, s in ipairs(branch.segments) do pieces[#pieces + 1] = s end
for _, x in ipairs({ 1.5, 3.5, 5.5 }) do
  local head = riviera.sprinkler.place{ type_id = t["Pendent Sprinkler"], point = { x, 1.6 }, level = l1, offset = 2.6 }
  join(head, 0)
end

label(0.3, 5.2, "Lavatories, urinal, WCs, floor drain; the sanitary main falls 1% below the floor")
label(0.3, -1.0, "Water heater; cold-water main with gate valve; sprinkler branch at 3.0 m")
