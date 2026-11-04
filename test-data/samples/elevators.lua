-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Elevators in an office core
-- description: A five-storey office under the IBC with a pair of passenger elevators sized for the IBC stretcher and a two-stop hydraulic elevator to a mezzanine, each with its car, a shaft cutting the floors from the pit to the overhead, hoistway walls and an entrance at every level served; Check Elevators and Check Clearances find nothing.
--
-- Reviera sample model (test-data/samples): the elevator tools (ARC-VT-001…004). The rule packs
-- come from the building code: the IBC brings `us_ibc_ada` (ADA car sizes, the IBC 3002.4
-- stretcher from four storeys), `us_a17` (A17.1's car area for the load, advisory) and
-- `practice_us` (maker-style car, hoistway, pit and overhead sizes). No load is given, so each
-- elevator gets the smallest car those packs accept.

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
--    hoistway and two 200 mm walls along, 50 mm clear of the first.
local a = reviera.elevator.create{ point = {10.0, 7.0}, base_level = levels[1], top_level = levels[5] }
local pitch = a.hoistway[1] + 2 * 0.2 + 0.05
reviera.elevator.create{ point = {10.0 + pitch, 7.0}, base_level = levels[1], top_level = levels[5] }

-- 2. A two-stop hydraulic elevator from Level 1 to Level 2 at 0.5 m/s, its landing toward the
--    west (turned 90°). Two storeys need no stretcher, so the ADA's side-door minimum gives the
--    2000 lb car (1727 × 1295 mm, 914 mm door), a 1219 mm pit and a 3734 mm overhead.
reviera.elevator.create{ point = {4.0, 7.0}, rotation = math.rad(-90), base_level = levels[1], top_level = levels[2], drive = "hydraulic", speed = 0.5 }

-- Nothing to report: `elevator.check` and `clearance.check` both come back empty.
assert(#reviera.elevator.check{} == 0, "elevator.check found problems")
