-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Office electrical and lighting
-- description: A main switchboard feeding a lighting and power board, nine ceiling luminaires on a lighting circuit with a switch by the door, receptacles on two power circuits, and the space's average illuminance by the lumen method.
--
-- Reviera sample model (test-data/samples): electrical equipment (MEP-ELEC-001): a main
-- switchboard MDB and a distribution board LP1 fed from it (panel feeders), circuits made from
-- the devices and luminaires and put on LP1 (MEP-ELEC-021 numbering: LP1-1, -2, -3); nine
-- ceiling luminaires of 5 000 lm with a coefficient-of-utilisation table, so the space's
-- average illuminance comes from the lumen method (ANA-LIGHT-001); receptacles and a light
-- switch on the walls; a smoke detector. Systems › Electrical › Panel Schedules and Loads
-- Summary report LP1's circuits and load.

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
  return reviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 3.3 } }
end

-- The office: 10 × 7 m on the walls' centrelines, a door in the west wall, windows south.
local c = { {0, 0}, {10, 0}, {10, 7}, {0, 7} }
local walls = {}
for i = 1, 4 do
  walls[i] = wall(c[i % 4 + 1], c[i])
end
reviera.door.place{ host_wall_id = walls[4], point_along = 5.5 }
for _, x in ipairs({ 2.5, 5, 7.5 }) do
  reviera.window.place{ host_wall_id = walls[1], point_along = 10 - x }
end
local space = reviera.space.place_auto{ level_id = l1 }[1]
reviera.space.set{ ids = { space }, limit_offset = 3.3 }

local t = reviera.mep_type.load_starters{ names = { "Switchboard", "Distribution Board", "Ceiling Luminaire", "Receptacle", "Light Switch", "Smoke Detector" } }

-- The boards on the north wall, LP1 fed from MDB.
local mdb = reviera.electrical_equipment.place{ type_id = t["Switchboard"], point = { 8.4, 6.5 }, level = l1 }
local lp1 = reviera.electrical_equipment.place{ type_id = t["Distribution Board"], point = { 6.6, 6.75 }, level = l1 }
reviera.panel.set{ panel_id = mdb, name = "MDB" }
reviera.panel.set{ panel_id = lp1, name = "LP1" }
reviera.panel.create_feeder{ child_panel_id = lp1, parent_panel_id = mdb }

-- Nine luminaires of 5 000 lm (CU 0.6), about 330 lx, on one lighting circuit with the switch
-- by the door.
reviera.lighting_fixture_type.set_lighting{ type_id = t["Ceiling Luminaire"], lumens = 5000.0,
  cu_table = { rcr = { 0, 10 }, columns = { { 0.8, 0.5, 0.2 } }, values = { { 0.6 }, { 0.6 } } } }
local lights = {}
for _, x in ipairs({ 1.8, 5.0, 8.2 }) do
  for _, y in ipairs({ 1.4, 3.5, 5.6 }) do
    lights[#lights + 1] = reviera.lighting_fixture.place{ type_id = t["Ceiling Luminaire"], point = { x, y }, level = l1, offset = 3.0 }
  end
end
local switch = reviera.device.place{ type_id = t["Light Switch"], point = { 0.15, 4.6 }, level = l1, offset = 1.1 }
local circuit = reviera.circuit.create{ kind = "power", element_ids = lights }.circuit
reviera.circuit.select_panel{ circuit_id = circuit, panel_id = lp1 }

-- Receptacles round the walls on two circuits, a smoke detector on the ceiling.
local west, east = {}, {}
for _, p in ipairs({ { 0.15, 1.5 }, { 2.5, 0.15 }, { 4.5, 0.15 } }) do
  west[#west + 1] = reviera.device.place{ type_id = t["Receptacle"], point = p, level = l1, offset = 0.3 }
end
for _, p in ipairs({ { 6.5, 0.15 }, { 9.85, 2.0 }, { 9.85, 4.5 } }) do
  east[#east + 1] = reviera.device.place{ type_id = t["Receptacle"], point = p, level = l1, offset = 0.3 }
end
for _, group in ipairs({ west, east }) do
  local r = reviera.circuit.create{ kind = "power", element_ids = group }.circuit
  reviera.circuit.select_panel{ circuit_id = r, panel_id = lp1 }
end
reviera.device.place{ type_id = t["Smoke Detector"], point = { 3.4, 2.4 }, level = l1, offset = 3.2 }

label(0.3, -1.0, "MDB feeds LP1: lighting LP1-1 (9 × 5 000 lm), receptacles LP1-2 and LP1-3")
label(0.3, 7.7, "Select the space: its average illuminance by the lumen method")
