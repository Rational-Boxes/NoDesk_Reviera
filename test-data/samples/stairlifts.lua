-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Stairlifts and platform stairlifts in a parish hall and its house
-- description: Two storeys under the IBC with three stairs: the house's stair with a seated stairlift, the hall's straight stair with a wheelchair platform lift on straight rails, and its turning stair with a platform lift on a curved rail, each sized under the vertical-transport rule packs; the stairs keep their required widths beside the folded units, the platforms' head boxes clear the floor above, and Check Stairlifts and Check Clearances find nothing.
--
-- Reviera sample model (test-data/samples): stairlifts and inclined platform lifts
-- (ARC-VT-013…015). Under the IBC a lift on a dwelling's stair leaves 20 in (508 mm) beside the
-- folded unit, and any other stair keeps its required width (36 in, 914 mm, for fewer than 50
-- people); A18.1, as makers quote it, keeps a 1524 mm head box over the platform's deck and caps
-- speed at 0.15 m/s; ADA gives a 30 × 48 in wheelchair space at each end; `practice_us` gives the
-- makers' generic sizes.

reviera.code.set{ code = "ibc" }
local l1 = reviera.level.list()[1].id
local l2 = reviera.level.create{ elevation = 3.0 }

local function rect(x0, y0, x1, y1)
  local c = { {x0, y0}, {x1, y0}, {x1, y1}, {x0, y1} }
  local loop = {}
  for i = 1, 4 do
    loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
  end
  return loop
end

-- A 32 × 16 m floor on Level 1. Level 2's floor leaves open stairwells over the hall's two
-- stairs, as long as their flights, so the platforms' head boxes clear it.
reviera.floor.create{ loops = { rect(0, 0, 32, 16) }, level_id = l1 }
reviera.floor.create{ loops = { rect(0, 0, 32, 16), rect(13, 1.8, 17.48, 3.0), rect(24, 1.6, 27.4, 4.86) }, level_id = l2 }

-- 1. The house's stair: 914 mm wide, 17 risers of 176 mm on 280 mm treads, with a railing on
--    its right only and its opening in Level 2 cut for its own headroom. A seated stairlift on
--    the left, going up, folds to 320 mm from the wall: 594 mm is left, more than the 508 mm the
--    IBC asks on a dwelling's stair.
local house = reviera.stair.create{ base_level = l1, width = 0.914, railings = false, runs = { { start = {3.0, 3.0}, ["end"] = {7.48, 3.0} } } }
reviera.railing.create_on_stair{ stair_id = house, sides = { "right" } }
reviera.stair.create_openings{ stair_id = house }
local seated = reviera.stairlift.create{ stair = house, side = "left", stair_use = "dwelling" }

-- 2. The hall's straight stair, 1200 mm wide, serving 40 people. A platform lift on straight
--    rails, 800 × 1250 mm, on its right: folded, it stands 260 mm from the wall, leaving 940 mm
--    of the 914 mm the stair needs. Its 1524 mm head box rides 0.92 m above the nosings at the
--    stair's 32.7°; its platform reaches 1.9 m past the bottom nosing, and a wheelchair space
--    waits beyond each end.
local hall = reviera.stair.create{ base_level = l1, width = 1.2, railings = false, runs = { { start = {13.0, 2.4}, ["end"] = {17.48, 2.4} } } }
reviera.railing.create_on_stair{ stair_id = hall, sides = { "left" } }
reviera.stairlift.create{ stair = hall, side = "right", kind = "platform", occupants = 40 }

-- 3. The hall's back stair turns left at a landing, 1300 mm wide. A stair that turns takes the
--    curved family by default: an 800 × 1220 mm platform on tubes that follow the outside of the
--    turn (its right side), needing 1225 mm on the inside of the bend.
local back = reviera.stair.create{ base_level = l1, width = 1.3, railings = false, runs = { { start = {24.0, 2.25}, ["end"] = {26.24, 2.25} }, { start = {26.75, 2.9}, ["end"] = {26.75, 4.86} } } }
reviera.stairlift.create{ stair = back, side = "right", kind = "platform", occupants = 40 }

assert(#reviera.stairlift.check{} == 0, "stairlift.check found problems")
