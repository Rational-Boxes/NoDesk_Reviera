-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Washrooms and a locker room
-- description: An office's washrooms under the IBC and the 2010 ADA Standards: a men's toilet core counted from the occupant load, with an accessible compartment, urinals behind screens and lavatories; an accessible single-user toilet with its grab bars; and a locker room with two-tier lockers, benches and gang showers.
--
-- Reviera sample model (test-data/samples): the commercial bathroom tools (ARC-BATH-003, 007,
-- 008, 009). The IBC brings the `us_plumbing` pack (IPC clearances and Table 403.1's counts);
-- setting the accessibility rule set to the ADA adds `ada_2010`. Each room is four 200 mm walls;
-- the comments give the rooms' inside sizes.

reviera.code.set{ code = "ibc", occupancy = "other" }
reviera.clearance.set_rule_set{ standard = "ada_2010" }
local l1 = reviera.level.list()[1].id

-- Four walls round an inside `w` × `l` from (x, y); returns them south, east, north, west.
local function room(x, y, w, l)
  local t = 0.1
  local x0, y0, x1, y1 = x - t, y - t, x + w + t, y + l + t
  local function wall(a, b)
    return reviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 3.0 } }
  end
  return { wall({x0, y0}, {x1, y0}), wall({x1, y0}, {x1, y1}), wall({x1, y1}, {x0, y1}), wall({x0, y1}, {x0, y0}) }
end

-- 1. A men's room, 9 × 4 m: the core along its north wall. The fixtures are counted from the
--    office's load (IPC Table 403.1, occupancy B: 150 people, half of them men), urinals taking
--    the place of WCs where the table allows (`bath.fixture_count` shows the sums). The
--    accessible compartment comes first, 60 × 59 in with its grab bars; partitions stand 12 in
--    off the floor and the compartment doors swing out.
local men = room(0, 0, 9, 4)
reviera.bath.toilet_core{ wall_id = men[3], start = {0, 3.9}, ["end"] = {9, 3.9}, level = l1, occupancy = "B", occupant_load = 150, sex = "male", urinals_for_wcs = true }

-- 2. An accessible single-user toilet, 1.8 × 2.6 m: the WC 18 in from the side wall, the
--    lavatory beside it, and the ADA's side and rear grab bars (`bath.layout`).
local acc = room(11, 0, 1.8, 2.6)
reviera.bath.layout{ template = "accessible_single_user", point = {11.9, 1.3}, level = l1, wet_wall_id = acc[4] }

-- 3. A locker room, 6 × 5 m: 40 two-tier lockers along the long walls, benches down the
--    middle with the ADA's 30 in in front of the lockers, six gang showers along the west wall
--    with a drying zone, and an accessible bench; 5 % of the lockers kept accessible.
room(0, 6, 6, 5)
reviera.bath.locker_room{ point = {3, 8.5}, level = l1, lockers = 40, tiers = 2, showers = 6 }
