-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: A furnished apartment
-- description: Four rooms of a 15.6 × 4.4 m apartment furnished from room layout templates — a queen bedroom, a living room, a dining room for four and a home office — every piece and its clear spaces (the ADA interiors pack and planning practice) kept in its room, with an FF&E schedule of the whole.
--
-- Riviera sample model (test-data/samples): room layouts (ARC-INT-001). Each room is closed by
-- walls; interior.layout puts the template's main piece against one of its longer walls and the
-- rest round it, and places nothing when a piece or a use zone would leave the room or stand in
-- another piece. The pieces are the generic furnishings of the bundled Interiors library
-- (ARC-FURN-001).

local l1 = riviera.level.list()[1].id
riviera.clearance.set_rule_set{ standard = "ada_2010" }
riviera.rules.set_packs{ domain = "interiors", packs = { "ada_2010_interiors", "practice_interiors" } }

-- The outer walls round 0.1–15.7 × 0.1–4.5 inside, and partitions at x = 4.2, 9.4 and 13.0:
-- a 4.0 m bedroom, a 5.0 m living room, a 3.4 m dining room and a 2.6 m office, all 4.4 m deep.
local function wall(a, b)
  return riviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 2.7 } }
end
wall({ 0, 0 }, { 15.8, 0 })
wall({ 15.8, 0 }, { 15.8, 4.6 })
wall({ 15.8, 4.6 }, { 0, 4.6 })
local west = wall({ 0, 4.6 }, { 0, 0 })
for _, x in ipairs({ 4.2, 9.4, 13.0 }) do
  wall({ x, 0.1 }, { x, 4.5 })
end

local rooms = {
  { "bedroom_queen", 2.1, "Bedroom" },
  { "living_room", 6.8, "Living room" },
  { "dining_4", 11.2, "Dining room" },
  { "home_office", 14.35, "Office" },
}
local dining_table
for _, r in ipairs(rooms) do
  local made = riviera.interior.layout{ template = r[1], point = { r[2], 2.3 }, level = l1 }
  if r[1] == "dining_4" then dining_table = made.placed[1] end
  assert(#made.placed > 0, r[1] .. " fits its room")
  print(string.format("%s: %d pieces; %.2f of %.2f m along, %.2f of %.2f m across", r[3], #made.placed, made.fit.length_needed, made.fit.length, made.fit.depth_needed, made.fit.depth))
  local room = riviera.room.place{ level_id = l1, point = { r[2], 0.4 } }
  riviera.room.set{ room_ids = { room }, name = r[3] }
end
-- Decorative lighting (ARC-INT-003): sconces along the bedroom's west wall, every 8 ft at 63 in,
-- and a chandelier over the dining table, sized from the room and the table, 32 in above it.
riviera.lighting.sconces{ wall_id = west, level = l1 }
local c = riviera.lighting.chandelier{ over_id = dining_table, level = l1 }
print(string.format("Dining room: a %s, its bottom %.0f mm up", c.item, c.bottom * 1000))
assert(#riviera.rules.check{} == 0, "every piece's clear spaces are free, and the lighting keeps its heights")

-- A room too small for a layout is refused with its shortfall: a queen bedroom in the office.
local tight = riviera.interior.layout{ template = "bedroom_queen", point = { 14.35, 2.3 }, level = l1, fit_only = true }
print(string.format("A queen bedroom in the office: %.0f mm short along, %.0f mm across", tight.fit.length_short * 1000, tight.fit.depth_short * 1000))

riviera.schedule.create_ffe{}
