-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: An accessible hotel guest room
-- description: A 4.0 × 5.4 m guest room with mobility features under the 2010 ADA Standards: a queen bed with a 30 × 48 in clear space along each side (§806.2.3), nightstands at its head, a dresser and a desk, checked against the ADA interiors pack and planning practice. A chair dropped beside the bed is reported, then moved clear.
--
-- Riviera sample model (test-data/samples): shared rule packs and use zones (ARC-RULE-001/002).
-- The ADA rule set brings in `ada_2010_interiors`; `practice_interiors` is added by name for its
-- notes (36 in round a dining table, 30 in in front of a dresser, 24 in at the foot of a bed).
-- The furniture is from the bundled Interiors library, each piece typed with its IFC predefined
-- type (BED, DESK, CHAIR) or, for USERDEFINED, its kind (Dresser, Nightstand), which is how the
-- packs select it.

local l1 = riviera.level.list()[1].id
riviera.clearance.set_rule_set{ standard = "ada_2010" }
riviera.rules.set_packs{ domain = "interiors", packs = { "ada_2010_interiors", "practice_interiors" } }

-- Four 200 mm walls round a 4.0 × 5.4 m inside from (0, 0), 2.7 m high: south, east, north, west.
local t = 0.1
local function wall(a, b)
  return riviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 2.7 } }
end
local south = wall({ -t, -t }, { 4.0 + t, -t })
wall({ 4.0 + t, -t }, { 4.0 + t, 5.4 + t })
wall({ 4.0 + t, 5.4 + t }, { -t, 5.4 + t })
wall({ -t, 5.4 + t }, { -t, -t })
local c = { { 0, 0 }, { 4.0, 0 }, { 4.0, 5.4 }, { 0, 5.4 } }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
riviera.floor.create{ loops = { loop }, level_id = l1 }
-- The entrance from the corridor, near the room's south-west corner.
riviera.door.place{ host_wall_id = south, point_along = 1.0 }

-- The furniture comes from the bundled Interiors library (DM-LIB-006): each piece a model
-- fragment carrying its IFC type (BED, DESK) or kind (Nightstand, Dresser).
local interiors
for _, l in ipairs(riviera.library.bundled()) do
  if l.name == "Riviera Interiors" then interiors = l.path end
end
assert(interiors, "the bundled Interiors library")
local function place(item, x, y, rotation)
  return riviera.component.place_fragment{ library = interiors, item = item, point = { x, y }, level = l1, rotation = rotation or 0 }
end
-- The queen bed (1624 × 2132 mm on its frame) with its head 20 mm off the north wall, centred
-- 2.0 m from the west wall: a 30 in (762 mm) space along each side, 48 in (1219 mm) from its
-- foot, clear to the walls.
local bed_y = 5.4 - 1.066 - 0.02
place("Bedroom/bed_queen_60_x_80_in.ifc", 2.0, bed_y)
-- A nightstand (22 × 16 in) each side of the head, 20 mm from the bed.
place("Bedroom/nightstand.ifc", 2.0 - 0.812 - 0.02 - 0.279, 5.4 - 0.203 - 0.02)
place("Bedroom/nightstand.ifc", 2.0 + 0.812 + 0.02 + 0.279, 5.4 - 0.203 - 0.02)
-- The dresser (60 × 19 in) against the east wall, facing west, with 30 in clear in front of it.
place("Bedroom/dresser.ifc", 4.0 - 0.2415 - 0.02, 1.6, "-90°")
-- The desk (60 × 30 in) against the west wall, facing east, clear of the door's maneuvering space.
place("Office/desk_60_x_30_in.ifc", 0.02 + 0.381, 2.4, "90°")

-- A task chair from the catalogue (furnishing.load), left beside the bed: it takes the bed's
-- left clear space, an error against §806.2.3.
local chair = riviera.furnishing.load{ names = { "Task chair" } }["Task chair"]
local the_chair = riviera.mesh_object.place{ type_id = chair, point = { 0.85, 3.9 }, level = l1 }
local found = riviera.rules.check{}
assert(#found == 1 and found[1].rule == "ADA 2010 806.2.3 left" and found[1].severity == "error", "the chair is in the bed's clear space")
print(string.format("%s: %s (%s, %s)", found[1].severity, found[1].message, found[1].edition, found[1].source))
-- Pushed to the desk, facing it, it is clear of the bed's spaces and the 24 in at its foot.
riviera.mesh_object.set{ element_ids = { the_chair }, point = { 0.782 + 0.02 + 0.305, 2.3 }, rotation = "-90°" }
assert(#riviera.rules.check{} == 0, "nothing in the furniture's use zones")

local zones = 0
for _, z in ipairs(riviera.clearance.list{}) do
  if z.kind == "use_zone" then zones = zones + 1 end
end
print(string.format("%d use zones; the room checks clean", zones))

-- The room, numbered and named, and an FF&E schedule of what is in it (DOC-SCH-016): one row a
-- type, counted.
local room = riviera.room.place{ level_id = l1, point = { 3.0, 0.5 } }
riviera.room.set{ room_ids = { room }, name = "Guest room" }
riviera.schedule.create_ffe{}
