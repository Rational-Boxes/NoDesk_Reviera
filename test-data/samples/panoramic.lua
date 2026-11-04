-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Panoramic lifts in a hotel atrium
-- description: A four-storey hotel under the IBC with a panoramic elevator glazed at the rear and sides in a glass hoistway, a rear-glazed elevator in a walled hoistway, and a two-stop hydraulic lift with a glass car and hoistway to the mezzanine; each car's laminated glass is sized for its panels and weighed, a glazed traction car's counterweight moves to the side, and Check Elevators gives one note: glass hoistways over more than two levels need fire-rated glazing unless the authority accepts them unenclosed.
--
-- Riviera sample model (test-data/samples): panoramic cars and glass hoistways (ARC-VT-016,
-- ARC-VT-017). `practice_us` gives the generic glass sizes: 10.76 mm laminated car glass
-- (5 + 5 + 0.76), 10.8 mm hoistway glass on 50 × 110 mm steel posts at most 1.3 m apart with
-- transoms at most 2.5 m apart (the largest pane for that glass), and 300 mm more hoistway width
-- where a glazed traction car's counterweight goes to the side. `us_ibc_ada` carries IBC 2409's
-- note on glass in hoistways.

riviera.code.set{ code = "ibc" }
local l1 = riviera.level.list()[1].id
local levels = { l1 }
for k, z in ipairs({ 4.5, 8.0, 11.5 }) do
  levels[k + 1] = riviera.level.create{ elevation = z }
end

-- A 30 × 20 m floor plate on every level.
local c = { {0, 0}, {30, 0}, {30, 20}, {0, 20} }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
for _, l in ipairs(levels) do
  riviera.floor.create{ loops = { loop }, level_id = l }
end

-- 1. The atrium's panoramic elevator, serving all four levels: glass at the rear and both sides
--    of the car, in a hoistway glazed at the sides and rear on steel posts; its front is a wall
--    with the landing entrances. Its counterweight goes to the side, so the hoistway is 300 mm
--    wider than the plain car's. Four levels of glass hoistway get the IBC note.
riviera.elevator.create{ point = {8.0, 6.0}, base_level = levels[1], top_level = levels[4], car_glass = "rear_sides", hoistway = "glass", equipment_side = "left" }

-- 2. A rear-glazed car in an ordinary walled hoistway, for a glass back wall facing a lightwell.
riviera.elevator.create{ point = {18.0, 6.0}, base_level = levels[1], top_level = levels[4], car_glass = "rear" }

-- 3. A two-stop hydraulic lift from Level 1 to the Level 2 mezzanine, glass car and glass
--    hoistway. A hydraulic car has no counterweight, so its hoistway keeps its size; two levels
--    need no fire-rated glazing note.
riviera.elevator.create{ point = {24.0, 12.0}, base_level = levels[1], top_level = levels[2], drive = "hydraulic", speed = 0.5, car_glass = "rear_sides", hoistway = "glass" }

-- One note (the atrium elevator's glass hoistway), and no errors.
local found = riviera.elevator.check{}
assert(#found == 1 and found[1].kind == "glass" and found[1].severity == "note", "elevator.check: one glass note expected")
