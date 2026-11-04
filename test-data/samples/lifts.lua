-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Hospital, freight and small lifts
-- description: A four-storey clinic and service building under the IBC with a hospital elevator, a class A freight elevator, a class B vehicle elevator, a LULA, a home elevator, a vertical platform lift and a counter-loading dumbwaiter, each sized from its kind's catalogue car with its shaft, walls and entrances; Check Elevators notes only that the makers' hospital car is a little over A17.1's passenger area table.
--
-- Reviera sample model (test-data/samples): elevators and lifts by kind (ARC-VT-005, ARC-VT-006).
-- Under the IBC the packs are `us_ibc_ada` (ADA 407–410 sizes, the stretcher), `us_a17` (car
-- area, freight loading classes, LULA, home elevator and dumbwaiter limits), `us_a18` (platform
-- lift travel) and `practice_us` (catalogue cars of each kind).

reviera.code.set{ code = "ibc" }
local l1 = reviera.level.list()[1].id
local levels = { l1 }
for k, z in ipairs({ 3.5, 7.0, 10.5 }) do
  levels[k + 1] = reviera.level.create{ elevation = z }
end

-- A 12 × 10 m floor plate on every level.
local c = { {0, 0}, {12, 0}, {12, 10}, {0, 10} }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
for _, l in ipairs(levels) do
  reviera.floor.create{ loops = { loop }, level_id = l }
end

-- The lifts are packed close together, so they are made without their controllers and machine
-- spaces (the sample `elevators` shows those).
local function lift(x, y, kind, top, more)
  local args = { point = { x, y }, base_level = levels[1], top_level = levels[top], kind = kind, equipment = false }
  for k, v in pairs(more or {}) do args[k] = v end
  return reviera.elevator.create(args)
end

-- 1. A vehicle elevator (class B, 8000 lb, 2438 × 6426 mm) to Level 2, hydraulic: its entrance
--    an 8 ft opening until vertical bi-parting doors are modelled.
lift(2.2, 1.0, "vehicle", 2, { drive = "hydraulic" })
-- 2. A class A freight elevator (2500 lb, 1626 × 2134 mm), hydraulic, to Level 2.
lift(6.0, 1.0, "freight", 2, { drive = "hydraulic" })
-- 3. A hospital elevator (5000 lb, 1727 × 2743 mm, 1372 mm two-speed door) to Level 4: its
--    side-opening doors stack inside the hoistway, so each entrance is an opening for now.
lift(10.0, 1.0, "hospital", 4)
-- 4. A LULA (1400 lb, 48 × 54 in) to Level 2, with a swinging entrance opening onto the landing.
lift(6.0, 5.0, "lula", 2)
-- 5. A home elevator (1000 lb, 36 × 54 in) to Level 2.
lift(9.0, 6.0, "residence", 2)
-- 6. A vertical platform lift (750 lb, 36 × 60 in) to Level 2: 3.5 m of travel, inside A18.1's
--    4250 mm for an enclosed runway.
lift(6.0, 8.0, "platform_lift", 2)
-- 7. A counter-loading dumbwaiter (500 lb, 36 × 36 × 48 in) to Level 3, its openings 762 mm up.
lift(11.0, 8.5, "dumbwaiter", 3)

-- One note, no errors: the makers' 5000 lb hospital car is 4.74 m², a little over A17.1's
-- passenger table for its load (advisory).
for _, f in ipairs(reviera.elevator.check{}) do
  assert(f.severity == "note", "an error: " .. f.message)
end
