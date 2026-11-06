-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: An accessible hotel guest room
-- description: A 4.0 × 5.4 m guest room with mobility features under the 2010 ADA Standards: a queen bed with a 30 × 48 in clear space along each side (§806.2.3), nightstands at its head, a dresser and a desk, checked against the ADA interiors pack and planning practice. A chair dropped beside the bed is reported, then moved clear.
--
-- Riviera sample model (test-data/samples): shared rule packs and use zones (ARC-RULE-001/002).
-- The ADA rule set brings in `ada_2010_interiors`; `practice_interiors` is added by name for its
-- notes (36 in round a dining table, 30 in in front of a dresser, 24 in at the foot of a bed).
-- The furniture is box types until the interiors library comes (INTERIORS_PLAN.md §4.1): each
-- type is given its IFC predefined type (BED, DESK, CHAIR) or, for USERDEFINED, its kind
-- (Dresser, Nightstand), which is how the packs select it.

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

-- Box furniture types (width × depth × height; a type's front is its −y side).
local function furniture(name, size, kind, color)
  local ty = riviera.mep_type.create{ name = name, category = "furniture", size = size, color = color }
  if kind:match("^%u+$") then
    riviera.mesh_object.set_type{ type_id = ty, predefined_type = kind }
  else
    riviera.mesh_object.set_type{ type_id = ty, element_type = kind }
  end
  return ty
end
local wood = { 0.55, 0.40, 0.25, 1.0 }
local bed = furniture("Queen bed 60 × 80 in", { 1.524, 2.032, 0.6 }, "BED", { 0.85, 0.85, 0.80, 1.0 })
local stand = furniture("Nightstand", { 0.5, 0.4, 0.6 }, "Nightstand", wood)
local dresser = furniture("Dresser", { 1.5, 0.5, 0.8 }, "Dresser", wood)
local desk = furniture("Desk", { 1.2, 0.6, 0.75 }, "DESK", wood)
local chair = furniture("Desk chair", { 0.5, 0.5, 0.85 }, "CHAIR", { 0.25, 0.30, 0.40, 1.0 })

local function place(ty, x, y, rotation)
  return riviera.mesh_object.place{ type_id = ty, point = { x, y }, level = l1, rotation = rotation or 0 }
end
-- The bed's head 20 mm off the north wall, centred 2.0 m from the west wall: a 30 in (762 mm)
-- space along each side, 48 in (1219 mm) from its foot, clear to the walls.
local the_bed = place(bed, 2.0, 5.4 - 1.016 - 0.02)
place(stand, 2.0 - 0.762 - 0.25 - 0.02, 5.4 - 0.2 - 0.02)
place(stand, 2.0 + 0.762 + 0.25 + 0.02, 5.4 - 0.2 - 0.02)
-- The dresser against the east wall, facing west, with 30 in clear in front of it.
place(dresser, 4.0 - 0.25 - 0.02, 1.6, "-90°")
-- The desk against the west wall, facing east, clear of the door's maneuvering space.
place(desk, 0.3 + 0.02, 2.45, "90°")

-- A chair left beside the bed takes its left clear space: an error against §806.2.3.
local the_chair = place(chair, 0.85, 3.9)
local found = riviera.rules.check{}
assert(#found == 1 and found[1].rule == "ADA 2010 806.2.3 left" and found[1].severity == "error", "the chair is in the bed's clear space")
print(string.format("%s: %s (%s, %s)", found[1].severity, found[1].message, found[1].edition, found[1].source))
-- Pushed in at the desk, it is clear.
riviera.mesh_object.set{ element_ids = { the_chair }, point = { 0.9, 2.45 }, rotation = "-90°" }
assert(#riviera.rules.check{} == 0, "nothing in the furniture's use zones")

local zones = 0
for _, z in ipairs(riviera.clearance.list{ element_ids = { the_bed } }) do
  if z.kind == "use_zone" then zones = zones + 1 end
end
print(string.format("bed: %d use zones; the room checks clean", zones))
