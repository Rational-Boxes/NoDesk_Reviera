-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Bathrooms and a home spa
-- description: Six rooms of a house under the IRC: a 5 × 8 ft full bath and a three-quarter bath laid out from templates, a level-access shower room with its floor falling to a linear drain and its waterproofing scheduled, a hot tub room, a two-tier sauna with its heater sized from the room, and a steam room with its ceiling sloped to shed condensate.
--
-- Reviera sample model (test-data/samples): the bathroom tools (ARC-BATH-001…014). The rule
-- packs come from the building code: the IRC brings `us_plumbing` (IPC/IRC clearances, shower
-- falls and waterproofing) and `us_spa` (NEC 680 for the hot tub, the sauna heater table). Each
-- room is four 200 mm walls; the comments give the rooms' inside sizes.

reviera.code.set{ code = "irc" }
local l1 = reviera.level.list()[1].id

-- Four walls round an inside `w` × `l` from (x, y), `h` high; returns them south, east, north,
-- west.
local function room(x, y, w, l, h)
  local t = 0.1
  local x0, y0, x1, y1 = x - t, y - t, x + w + t, y + l + t
  local function wall(a, b)
    return reviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = h or 2.5 } }
  end
  return { wall({x0, y0}, {x1, y0}), wall({x1, y0}, {x1, y1}), wall({x1, y1}, {x0, y1}), wall({x0, y1}, {x0, y0}) }
end

-- A floor over a room's inside.
local function floor(x, y, w, l)
  local c = { {x, y}, {x + w, y}, {x + w, y + l}, {x, y + l} }
  local loop = {}
  for i = 1, 4 do
    loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
  end
  return reviera.floor.create{ loops = { loop }, level_id = l1 }
end

-- 1. A full bath, 5 × 8 ft (1.525 × 2.44 m): the tub across one end, the WC and a pedestal
--    lavatory along the wet wall at the IPC's spacings (`bath.layout`).
--    IRC R307.1's 21 in in front of a tub has no width in the source, so Reviera takes the tub's
--    whole length; the WC beside the tub stands in it, and the pair is accepted as intended.
room(0, 0, 1.525, 2.44)
local full = reviera.bath.layout{ template = "full_bath", point = {0.76, 1.22}, level = l1 }
reviera.overlap.accept{ a = full.placed[1], b = full.placed[2], note = "A 5 × 8 ft bath: the WC 15 in from the tub, as IRC Figure R307.1 draws it" }

-- 2. A three-quarter bath, 1.6 × 2.8 m: a 900 mm shower tray, the WC and the lavatory.
room(3, 0, 1.6, 2.8)
reviera.bath.layout{ template = "three_quarter_bath", point = {3.8, 1.4}, level = l1 }

-- 3. A level-access shower room, 2.4 × 2.5 m on a floor: a 60 × 30 in roll-in former against the
--    south wall, its floor falling to a linear drain along the back at the IPC's ¼ in per foot
--    (`bath.shower_falls`), and the walls round it and its floor to be waterproofed to 6 ft
--    (`bath.waterproofing` lists them).
room(6, 0, 2.4, 2.5)
floor(6, 0, 2.4, 2.5)
local t = reviera.bath.load_components{ names = { "Roll-in shower, 60 × 30 in", "Hot tub, 2.0 × 2.0 m" } }
local shower = reviera.bath.place_component{ type_id = t["Roll-in shower, 60 × 30 in"], point = {7.2, 0.1}, level = l1 }
reviera.bath.shower_falls{ element_ids = { shower }, drain = "linear" }

-- 4. A hot tub room, 5 × 4 m: a 2 m tub free-standing in the middle with 0.9 m clear all round
--    for its service panels; its filled load (5.4 kPa) is on the lowest level, so Check
--    Bathrooms asks for a 100 mm pad.
room(10, 0, 5, 4)
floor(10, 0, 5, 4)
reviera.bath.place_component{ type_id = t["Hot tub, 2.0 × 2.0 m"], point = {12.5, 2.0}, level = l1, work_plane = true }

-- 5. A sauna, 2.4 × 2.0 m and 2.2 m high (10.6 m³): two bench tiers along the south wall and an
--    8 kW wall heater sized from the room (`bath.sauna`); its door, in the north wall, opens
--    outward.
local sauna_walls = room(17, 0, 2.4, 2.0, 2.2)
reviera.bath.sauna{ point = {18.2, 1.0}, level = l1, bench_wall_id = sauna_walls[1] }
local door = reviera.door.place{ host_wall_id = sauna_walls[3], point_along = 1.3 }
for _, f in ipairs(reviera.bath.check{}) do
  if f.kind == "sauna" and f.other == door then
    reviera.insert.flip_facing{ element_ids = { door } }
  end
end

-- 6. A steam room, 2.4 × 1.8 m with walls 2.4 m high, on a floor (`bath.steam_room`): a ceiling
--    2.13 m high over the bench wall falling 2 in per ft (1:6) to 1.83 m so condensate runs to
--    the far wall, a tiled bench along the south wall, the steam head low on the east wall across
--    from it and its control 1.5 m up on the north wall, and the floor falling ¼ in per ft to a
--    centre drain. A tiled room of 8.6 m³ needs a generator for that volume (makers' factors).
local steam_walls = room(21, 0, 2.4, 1.8, 2.4)
floor(21, 0, 2.4, 1.8)
reviera.bath.steam_room{ point = {22.2, 0.9}, level = l1, bench_wall_id = steam_walls[1], material = "tile" }

-- 7. Drawings: rooms for the two baths, interior elevations of the full bath's four walls at
--    1:25 on an A3 sheet (`bath.elevations`), and the plumbing fixture and accessories schedules
--    on a sheet of their own (`bath.schedules`), the fixtures with their drainage and water
--    supply fixture units by room.
reviera.room.place{ level_id = l1, point = {0.76, 1.22} }
reviera.room.place{ level_id = l1, point = {3.8, 1.4} }
reviera.bath.elevations{ point = {0.76, 1.22}, level = l1, name = "Full bath" }
local schedules_sheet = reviera.sheet.create{ name = "Bathroom schedules", size = "A3" }
reviera.bath.schedules{ sheet_id = schedules_sheet }
