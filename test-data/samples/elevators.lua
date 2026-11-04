-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Elevators in an office core
-- description: A five-storey office under the IBC with a pair of passenger elevators sized for the IBC stretcher and a two-stop hydraulic elevator to a mezzanine, each with its car, a shaft cutting the floors from the pit to the overhead, hoistway walls, an entrance at every level served, its controller (and pump unit) in the space it needs, and an elevator schedule with power and heat estimates; Check Elevators and Check Clearances find nothing.
--
-- Reviera sample model (test-data/samples): the elevator tools (ARC-VT-001…004). The rule packs
-- come from the building code: the IBC brings `us_ibc_ada` (ADA car sizes, the IBC 3002.4
-- stretcher from four storeys), `us_a17` (A17.1's car area for the load, advisory) and
-- `practice_us` (maker-style car, hoistway, pit and overhead sizes). No load is given, so each
-- elevator gets the smallest car those packs accept. `practice_us` also estimates each motor's
-- power and heat from the load and speed (planning figures, until the maker's replace them) and
-- gives the space the controller needs (ARC-VT-007…009).

reviera.code.set{ code = "ibc" }
local l1 = reviera.level.list()[1].id
local levels = { l1 }
for k, z in ipairs({ 4.0, 7.5, 11.0, 14.5 }) do
  levels[k + 1] = reviera.level.create{ elevation = z }
end

-- A 24 × 14 m floor plate on every level.
local c = { {0, 0}, {24, 0}, {24, 14}, {0, 14} }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
for _, l in ipairs(levels) do
  reviera.floor.create{ loops = { loop }, level_id = l }
end

-- 1. Two passenger elevators side by side, serving all five levels, their landings toward the
--    south (rotation 0). Five storeys need the IBC stretcher, so each gets the 3500 lb car
--    (2032 × 1651 mm, 21 persons) in a 2591 × 2108 mm hoistway, a 1524 mm pit and a 4115 mm
--    overhead above Level 5 (machine-room-less traction at 1.0 m/s). The second stands one
--    hoistway and two 200 mm walls along, 50 mm clear of the first. Each has its controller in a
--    1219 × 508 mm closet beside the hoistway at the top landing, on the pair's outer sides, and
--    a motor of about 12.2 kW (14.4 kVA, 17.3 A at 480 V) releasing about 3.7 kW of heat.
local a = reviera.elevator.create{ point = {10.0, 7.0}, base_level = levels[1], top_level = levels[5], equipment_side = "left" }
local pitch = a.hoistway[1] + 2 * 0.2 + 0.05
reviera.elevator.create{ point = {10.0 + pitch, 7.0}, base_level = levels[1], top_level = levels[5] }

-- 2. A two-stop hydraulic elevator from Level 1 to Level 2 at 0.5 m/s, its landing toward the
--    west (turned 90°). Two storeys need no stretcher, so the ADA's side-door minimum gives the
--    2000 lb car (1727 × 1295 mm, 914 mm door), a 1219 mm pit and a 3734 mm overhead. Its pump
--    unit and controller stand in a 2184 × 2172 mm pump room beside the hoistway on Level 1.
reviera.elevator.create{ point = {4.0, 7.0}, rotation = math.rad(-90), base_level = levels[1], top_level = levels[2], drive = "hydraulic", speed = 0.5 }

-- An elevator schedule: kind, stops, load, persons, speed, drive, car, hoistway, pit, overhead,
-- motor power and heat.
reviera.schedule.create{ category = "cat.transport", name = "Elevator Schedule" }

-- Nothing to report: `elevator.check` and `clearance.check` both come back empty.
assert(#reviera.elevator.check{} == 0, "elevator.check found problems")
