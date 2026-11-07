-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Service counters
-- description: A 7.6 × 5.0 m reception lobby with a 42 in transaction counter lowered to 36 in for a parallel approach, a 36 in service counter with a forward-approach portion open below, and a 29 in work surface, every portion checked against the 2010 ADA Standards (§904.4, §306).
--
-- Riviera sample model (test-data/samples): counters with height zones (ARC-INT-004). counter.run
-- builds a counter along a wall's face from casework bodies and tops; its accessible portion's
-- top is typed for the interiors rule packs, which check its length and height, its 30 × 48 in
-- clear floor space and, for a forward approach, the knee clearance under it.

local l1 = riviera.level.list()[1].id
riviera.code.set{ code = "ibc" }
riviera.clearance.set_rule_set{ standard = "ada_2010" }
riviera.rules.set_packs{ domain = "interiors", packs = { "ada_2010_interiors", "practice_interiors" } }

-- Walls on centre lines round 0–7.8 × 0–5.2, 200 mm thick: inside 0.1–7.7 × 0.1–5.1.
local function wall(a, b)
  return riviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 3.0 } }
end
wall({ 0, 0 }, { 7.8, 0 })
wall({ 7.8, 0 }, { 7.8, 5.2 })
wall({ 7.8, 5.2 }, { 0, 5.2 })
wall({ 0, 5.2 }, { 0, 0 })

local function report(name, c)
  local p = c.portion
  if p then
    print(string.format("%s: %.2f m at %.0f in, an accessible portion %.2f–%.2f m at %.0f in (%s)", name, c.length, c.height / 0.0254, p.from, p.to, p.height / 0.0254, p.approach))
  else
    print(string.format("%s: %.2f m at %.0f in", name, c.length, c.height / 0.0254))
  end
end

-- The tellers' transaction counter on the south wall, 4.0 m, its last 36 in lowered for a
-- parallel approach, 1.7 m short of the east wall so the 48 in space along it fits.
report("Transaction counter", riviera.counter.run{ start = { 2.0, 0.1 }, ["end"] = { 6.0, 0.1 }, level = l1, kind = "transaction", accessible = { approach = "parallel" } })
-- The reception desk on the west wall, 2.4 m at 36 in, its first 30 in open below for a forward
-- approach.
report("Reception", riviera.counter.run{ start = { 0.1, 2.0 }, ["end"] = { 0.1, 4.4 }, level = l1, kind = "service", accessible = { approach = "forward", at = "start" } })
-- A writing surface on the north wall, 29 in and open below.
report("Writing surface", riviera.counter.run{ start = { 4.0, 5.1 }, ["end"] = { 6.0, 5.1 }, level = l1, kind = "work" })

assert(#riviera.rules.check{} == 0, "every accessible portion is long enough, low enough and clear in front and below")
assert(#riviera.overlap.find{}.overlaps == 0, "no body or top overlaps another or a wall")

local room = riviera.room.place{ level_id = l1, point = { 3.9, 2.6 } }
riviera.room.set{ room_ids = { room }, name = "Lobby" }
riviera.schedule.create_ffe{}
