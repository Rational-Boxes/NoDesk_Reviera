-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Evacuation lifts in a tower
-- description: Six floors 3.5 m apart with a stair stacked flight on flight at the west end and a passenger elevator at the east end; marked as an occupant evacuation elevator, egress.route takes it from the top floor when asked (waiting 60 s for the car, then the doors and the ride), and the stairs when not, or when the wait is too long; marked for fire service instead, it is left to firefighters and IFC's FireFightingLift is set.
--
-- Riviera sample model (test-data/samples): lifts in egress routes (ANA-ROUTE-006). A lift's time
-- is the wait for the car (60 s unless given), 10 s of doors, and the ride at its rated speed
-- with the time lost speeding up and slowing down (E-93).

riviera.code.set{ code = "ibc" }
local levels = { riviera.level.list()[1].id }
for k = 1, 5 do
  levels[k + 1] = riviera.level.create{ elevation = 3.5 * k }
end

-- A 32 × 16 m floor plate on every level.
local c = { {0, 0}, {32, 0}, {32, 16}, {0, 16} }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
for _, l in ipairs(levels) do
  riviera.floor.create{ loops = { loop }, level_id = l }
end

-- A stair from each floor to the next at the west end, rising east, the flights alternating
-- between two lines 3 m apart; each cuts its opening in the floor above.
for k = 1, 5 do
  local y = (k % 2 == 1) and 4.0 or 7.0
  local s = riviera.stair.create{ base_level = levels[k], width = 1.2, runs = { { start = { 2.0, y }, ["end"] = { 7.5, y } } } }
  riviera.stair.create_openings{ stair_id = s }
end

-- A passenger elevator at the east end serving every floor, its landing toward the south.
local lift = riviera.elevator.create{ point = { 26.0, 9.0 }, base_level = levels[1], top_level = levels[6] }.elevator

-- From the top floor's east end to the ground floor's. The elevator is not yet an evacuation
-- elevator: the route walks west and takes the five flights.
local from = { point = { 28.0, 3.0 }, level = levels[6] }
local out = { point = { 28.0, 3.0 }, level = levels[1] }
local by_stairs = riviera.egress.route{ start = from, ["end"] = out, lifts = true }
assert(#by_stairs.links == 5 and by_stairs.links[1].kind == "stair", "the stairs")

-- Marked as an occupant evacuation elevator (IBC 3008, EN 81-76), the route takes it: one ride
-- of 17.5 m with its wait, sooner than the stairs.
riviera.elevator.set_egress_role{ elevator_id = lift, role = "occupant_evacuation" }
local by_lift = riviera.egress.route{ start = from, ["end"] = out, lifts = true }
assert(#by_lift.links == 1 and by_lift.links[1].kind == "lift" and by_lift.time < by_stairs.time, "the lift")
print(string.format("by the stairs %.0f s, by the lift %.0f s", by_stairs.time, by_lift.time))
-- Without lifts asked for, or with a three-minute wait for the car, the stairs again.
assert(riviera.egress.route{ start = from, ["end"] = out }.links[1].kind == "stair", "not asked")
assert(riviera.egress.route{ start = from, ["end"] = out, lifts = true, lift_wait = 180 }.links[1].kind == "stair", "a long wait")

-- Drawn in the plans: the walk to the lift on the top floor, and from it on the ground floor.
riviera.view.create_plan{ level_ids = levels }
local drawn = riviera.egress.draw_route{ start = from, ["end"] = out, lifts = true }
assert(#drawn.paths == 2, "a path on each end floor")

assert(#riviera.elevator.check{} == 0, "elevator.check found problems")
