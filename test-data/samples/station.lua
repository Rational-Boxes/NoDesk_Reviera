-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Escalators in a transit station
-- description: A concourse over a platform 7 m below, joined by an up-and-down pair of transit escalators (heavy duty, 0.65 m/s) each with a 6 m station run-off zone beyond its ends, and a stair; escalator.traffic checks the units a 9,000-an-hour peak needs with one out, and what the escalators add to the platform's egress capacity under NFPA 130 (at most half) against the IBC (nothing); egress.route finds the way out from the platform's far end, by the stair under the IBC and by an escalator under NFPA 130, drawn in the plans; and egress.platform_clearing checks NFPA 130's 4 and 6 minute tests for 500 people on the platform.
--
-- Riviera sample model (test-data/samples): escalators in stations (ARC-VT-020, ARC-VT-021) and
-- egress routes across levels (ANA-ROUTE-004).
-- `practice_us` gives the transit speed and the 6 m run-off (SCDF, LTA, Network Rail); the unit
-- flow (100 persons a minute) and the evacuation rate (120 a minute per metre of tread, TfL) are
-- planning values (E-90).

riviera.code.set{ code = "ibc" }
local platform = riviera.level.list()[1].id
local concourse = riviera.level.create{ elevation = 7.0 }

-- A 70 × 24 m slab on each level.
local c = { {0, 0}, {70, 0}, {70, 24}, {0, 24} }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
for _, l in ipairs({ platform, concourse }) do
  riviera.floor.create{ loops = { loop }, level_id = l }
end

-- An up-and-down pair from the platform to the concourse: a station's escalators are heavy duty
-- (a truss 640 mm wider than the step, so the pair stands a truss width apart), run at 0.65 m/s
-- and keep 6 m clear beyond each end.
local up = riviera.escalator.create{ point = { 20.0, 10.0 }, base_level = platform, top_level = concourse, context = "transit" }
local down = riviera.escalator.create{ point = { 20.0, 10.0 + up.truss_width }, base_level = platform, top_level = concourse, context = "transit" }
assert(up.speed == 0.65 and up.duty == "transit", "transit defaults")

-- A peak of 9,000 people an hour up (150 a minute) needs two units at 100 a minute, and a third
-- for the one assumed out: this pair is one short.
local t = riviera.escalator.traffic{ up = { up.escalator }, down = { down.escalator }, demand_up = 9000, demand_down = 3000 }
assert(t.up.units_needed == 3, "three units up")

-- Evacuating the platform up: it needs 300 people a minute and its stairs give 200. The up
-- escalator alone runs that way; with it out, nothing is left, so under NFPA 130 the stairs
-- must do it all. Running both up for evacuation, one counts (120 a minute), within half the
-- need (150): enough. Under the IBC escalators never count.
local both = { up.escalator, down.escalator }
local nfpa = riviera.escalator.traffic{ up = both, demand_up = 0, egress = { mode = "nfpa130", required = 300, stairs = 200 } }.egress
assert(nfpa.counted == 120 and nfpa.meets, "NFPA 130 counts one unit")
local ibc = riviera.escalator.traffic{ up = both, demand_up = 0, egress = { mode = "ibc", required = 300, stairs = 200 } }.egress
assert(ibc.counted == 0 and not ibc.meets, "the IBC counts none")

-- A stair from the platform to the concourse at the west end, its opening cut in the concourse.
local stair = riviera.stair.create{ base_level = platform, width = 1.5, runs = { { start = {4.0, 19.0}, ["end"] = {14.64, 19.0} } } }
riviera.stair.create_openings{ stair_id = stair }

-- Out from the platform's far end to the concourse's exit at its east end. Under the IBC only
-- the stair counts: across the platform to the west end, up, and back east. Under NFPA 130 the
-- up escalator counts too, and is the quicker way.
local from = { point = { 64.0, 4.0 }, level = platform }
local exit = { point = { 64.0, 20.0 }, level = concourse }
local by_stair = riviera.egress.route{ start = from, ["end"] = exit }
assert(by_stair.links[1].kind == "stair", "the IBC route takes the stair")
local by_escalator = riviera.egress.route{ start = from, ["end"] = exit, mode = "nfpa130" }
assert(by_escalator.links[1].kind == "escalator" and by_escalator.time < by_stair.time, "NFPA 130 takes an escalator")

-- Drawn in the plans: each leg as a path of travel in its level's floor plan, with its length
-- and time, kept up to date by path_of_travel.update as the model changes.
riviera.view.create_plan{ level_ids = { platform, concourse } }
local drawn = riviera.egress.draw_route{ start = from, ["end"] = exit, mode = "nfpa130" }
assert(#drawn.paths == 2 and #drawn.unplanned == 0, "a path of travel on each level")

-- NFPA 130's two tests for the platform, its 500 people (a train of 350 entering and 150 waiting
-- to board, NFPA 130 5.5.5.1): out
-- through the stair (1500 mm at 0.0555 a minute a millimetre, 83 a minute) and the escalators
-- (one out, the other counted up to the stairs' share) within 4 minutes; and from its far end to
-- the concourse exit within 6, walked at 37.7 m/min, climbed at 15 m/min, with the wait at the
-- escalator.
local clearing = riviera.egress.platform_clearing{ level = platform, occupancy = { trains = { 350 }, entraining = 150 }, remote = { 64.0, 4.0 }, safety = exit }
assert(clearing.load == 500 and clearing.load_basis.from == "trains", "the trains govern")
print(string.format("platform %.2f min, to safety %.2f min", clearing.platform_time, clearing.safety_time))
assert(clearing.meets_platform and clearing.meets_safety, "the platform clears in time")
local stairs_only = riviera.egress.platform_clearing{ level = platform, load = 500, remote = { 64.0, 4.0 }, safety = exit, mode = "ibc" }
assert(not stairs_only.meets_platform, "the stair alone is too narrow")

-- The area figure for a station serving a dense neighbourhood, 7 ft² (0.65 m²) a person over the
-- whole 70 × 24 m platform slab (1,680 m², 2,585 people), is far more than the trains bring: it
-- governs, and the platform takes 15.5 minutes to clear. A real platform's area stops at the train's length.
local dense = riviera.egress.platform_clearing{ level = platform, occupancy = { trains = { 350 }, entraining = 150, area_per_person = 0.65 }, remote = { 64.0, 4.0 }, safety = exit }
assert(dense.load_basis.from == "area" and not dense.meets_platform, "the area governs")
print(string.format("dense: %.0f m², %.0f people, %.1f min", dense.load_basis.area, dense.load, dense.platform_time))

-- Three elevators from the platform to the concourse east of the escalators, their landings
-- toward the south: two for occupant evacuation, one for fire service. NFPA 130 5.5.6.3.3 lets
-- evacuation elevators count (some adopting codes delete it): one is taken as out of service
-- and the fire service one is kept for firefighters, so one counts, at its rated persons each
-- round trip.
local lifts = {}
for k = 0, 2 do
  lifts[k + 1] = riviera.elevator.create{ point = { 44.0 + 4.0 * k, 16.0 }, base_level = platform, top_level = concourse }.elevator
end
riviera.elevator.set_egress_role{ elevator_id = lifts[1], role = "occupant_evacuation" }
riviera.elevator.set_egress_role{ elevator_id = lifts[2], role = "occupant_evacuation" }
riviera.elevator.set_egress_role{ elevator_id = lifts[3], role = "fire_service" }
local with_lifts = riviera.egress.platform_clearing{ level = platform, occupancy = { trains = { 350 }, entraining = 150 }, remote = { 64.0, 4.0 }, safety = exit, lifts = true }
assert(with_lifts.lifts_counted > 0 and with_lifts.platform_time < clearing.platform_time, "a lift counts")
print(string.format("with lifts: %.1f a minute from the lift, %.2f min", with_lifts.lifts_counted, with_lifts.platform_time))
