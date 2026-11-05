-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Waiting on the way up from a deep station
-- description: A platform, a concourse 6 m above it and the street 6 m above that, with two wide stairs up from the platform and one narrow stair on up to the street; egress.platform_clearing finds the platform clears in time but the narrow stair holds people up on the concourse (NFPA 130 Annex C), and a second stair to the street brings the time to safety back within 6 minutes.
--
-- Riviera sample model (test-data/samples): waits at the exits of the levels a route crosses after
-- the platform (ANA-ROUTE-005). Each bottleneck on the way holds people up for as much longer as
-- its flow takes than the longest before it.

riviera.code.set{ code = "ibc" }
local platform = riviera.level.list()[1].id
local concourse = riviera.level.create{ elevation = 6.0 }
local street = riviera.level.create{ elevation = 12.0 }

-- A 60 × 12 m slab on each level.
local c = { {0, 0}, {60, 0}, {60, 12}, {0, 12} }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
for _, l in ipairs({ platform, concourse, street }) do
  riviera.floor.create{ loops = { loop }, level_id = l }
end

-- Two 2 m stairs from the platform up to the concourse at the west end, rising east, each with its
-- opening (2 × 2000 mm × 0.0555 = 222 people a minute).
for _, y in ipairs({ 3.0, 8.0 }) do
  local s = riviera.stair.create{ base_level = platform, width = 2.0, runs = { { start = { 4.0, y }, ["end"] = { 13.5, y } } } }
  riviera.stair.create_openings{ stair_id = s }
end
-- One 1.2 m stair from the concourse on up to the street at the east end (67 a minute).
local up = riviera.stair.create{ base_level = concourse, width = 1.2, runs = { { start = { 40.0, 3.0 }, ["end"] = { 49.5, 3.0 } } } }
riviera.stair.create_openings{ stair_id = up }

-- 400 people on the platform, from its east end to the street's east end. Off the platform in 1.80
-- minutes; up the narrow stair in 6.01: they wait 4.21 minutes on the concourse.
local args = { level = platform, load = 400, remote = { 58.0, 6.0 }, safety = { point = { 56.0, 9.0 }, level = street } }
local narrow = riviera.egress.platform_clearing(args)
assert(narrow.meets_platform and #narrow.level_exits == 1 and narrow.exit_wait > 4 and not narrow.meets_safety, "the narrow stair holds them up")
print(string.format("one stair up: off the platform %.2f min, up to the street %.2f min, waiting %.2f; to safety %.2f min", narrow.platform_time, narrow.level_exits[1].flow_time, narrow.exit_wait, narrow.safety_time))

-- A second 1.2 m stair to the street halves the wait's flow: 3.00 minutes up, 1.20 waiting.
local second = riviera.stair.create{ base_level = concourse, width = 1.2, runs = { { start = { 40.0, 9.0 }, ["end"] = { 49.5, 9.0 } } } }
riviera.stair.create_openings{ stair_id = second }
local two = riviera.egress.platform_clearing(args)
assert(two.meets_safety and two.exit_wait < narrow.exit_wait, "two stairs up")
print(string.format("two stairs up: up to the street %.2f min, waiting %.2f; to safety %.2f min", two.level_exits[1].flow_time, two.exit_wait, two.safety_time))
