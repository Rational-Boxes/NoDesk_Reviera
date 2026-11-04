-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Elevator traffic in an office tower
-- description: An eleven-storey office under the IBC: four passenger elevators at 1.0 m/s fail the office up-peak targets (a 30 s interval and 12 % of the population in 5 minutes); six at 2.5 m/s in a second bank meet them for a given population; the population is estimated from the floors at 10 m² a person; and an escalator pair to the first floor is checked against its demand with one unit out.
--
-- Riviera sample model (test-data/samples): elevator and escalator traffic (ARC-VT-018,
-- ARC-VT-019). `practice_us` gives the up-peak coefficients (80 % loading, 10 s a stop, 1.2 s a
-- passenger each way) and the office targets, all flagged for expert review; the figures are a
-- planning check, not a simulation.

riviera.code.set{ code = "ibc" }
local l1 = riviera.level.list()[1].id
local levels = { l1 }
for k = 1, 10 do
  levels[k + 1] = riviera.level.create{ elevation = 4.0 * k }
end

-- A 36 × 24 m floor plate on every level.
local c = { {0, 0}, {36, 0}, {36, 24}, {0, 24} }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
for _, l in ipairs(levels) do
  riviera.floor.create{ loops = { loop }, level_id = l }
end

-- 1. Bank A: four 3500 lb cars at 1.0 m/s serving all eleven levels, side by side.
local bank_a = {}
for k = 0, 3 do
  local e = riviera.elevator.create{ point = { 6.0 + 3.2 * k, 10.0 }, base_level = levels[1], top_level = levels[11], equipment = false }
  bank_a[#bank_a + 1] = e.elevator
end

-- Its up-peak, with the population estimated from the ten floors above the lobby at 10 m² a
-- person (about 830 people on 864 m² floors less the hoistways). At 1.0 m/s the round trip is
-- about 190 s: an interval of about 47 s, over the 30 s target.
local a = riviera.elevator.traffic{ elevators = bank_a }[1]
assert(not a.meets, "bank A should miss the office targets")

-- 2. Bank B: six of the same cars at 2.5 m/s (traction with a machine room, as high-rise banks
--    are), checked as a group of their own for a given 1,200
--    people. By area each car takes 15 passengers (3.35 m² at 0.21 m²), 12 on average; the round
--    trip is about 141 s, so the interval is about 24 s and the handling about 12.7 %.
local bank_b = {}
for k = 0, 5 do
  local e = riviera.elevator.create{ point = { 6.0 + 3.2 * k, 18.0 }, rotation = math.pi, base_level = levels[1], top_level = levels[11], drive = "traction", speed = 2.5, equipment = false }
  bank_b[#bank_b + 1] = e.elevator
end
local b = riviera.elevator.traffic{ elevators = bank_b, population = 1200 }[1]
assert(b.meets, "bank B should meet the office targets")

-- 3. An escalator pair from the lobby to Level 2, one up and one down, each about 6,000 persons
--    an hour; a peak of 2,500 up and 1,500 down. Both run well within capacity; with one unit
--    out each way there is none left, and the notes say the demand falls to the stairs.
local up = riviera.escalator.create{ point = { 22.0, 3.0 }, base_level = levels[1], top_level = levels[2] }
local down = riviera.escalator.create{ point = { 22.0, 3.0 + up.handrail_outer + 0.2 }, base_level = levels[1], top_level = levels[2] }
local t = riviera.escalator.traffic{ up = { up.escalator }, down = { down.escalator }, demand_up = 2500, demand_down = 1500, one_out = false }
assert(t.up.meets and t.down.meets, "the pair should carry the peak")
