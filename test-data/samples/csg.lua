-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Overlap resolutions
-- description: The same overlap resolved every way: cut either way, join, intersect, union, move apart, accept.
--
-- Reviera sample model (test-data/samples): eight identical pairs in a row, each a 4 m wall (A,
-- 3 m high) pierced by a 600 mm pier (B, 4.5 m high), resolved by overlap.resolve as View ›
-- Check Overlaps offers it (ARC-MOD-030/032). The first pair is left unresolved for comparison,
-- so the model has exactly one overlap. Labels in the plan name each resolution.

local level = reviera.level.list()[1].id
local plan = reviera.view.create_plan{ level_ids = { level } }[1]

-- The pier type: the generic wall type at 600 mm.
local generic
local function wall(a, b, height, type_id)
  return reviera.wall.create{
    type_id = type_id,
    curve = { line = { start = a, ["end"] = b } },
    base_level = level, top_constraint = { unconnected = height },
  }
end
-- The first wall makes the generic type; the pier type copies it.
local first_a = wall({-2, 0}, {2, 0}, 3.0)
for _, t in ipairs(reviera.type.list()) do
  if t.name == "Generic - 200mm" then generic = t.id end
end
local pier = reviera.type.duplicate{ type_id = generic, new_name = "Pier 600" }
reviera.type.set_param{ type_id = pier, param = "width", value = 0.6 }

local cases = {
  { mode = nil, label = "Unresolved" },
  { mode = "cut_a", label = "Cut A by B" },
  { mode = "cut_b", label = "Cut B by A" },
  { mode = "join", label = "Join (A cuts B)" },
  { mode = "intersect", label = "Intersect (A keeps only what it shares)" },
  { mode = "union", label = "Union (B fused into A)" },
  { mode = "move_apart", label = "Move B apart" },
  { mode = "accept", label = "Accept as intended" },
}
for i, c in ipairs(cases) do
  local x = (i - 1) * 7.0
  local a = (i == 1) and first_a or wall({x - 2, 0}, {x + 2, 0}, 3.0)
  local b = wall({x, -1.0}, {x, 1.0}, 4.5, pier)
  if c.mode then
    reviera.overlap.resolve{ a = a, b = b, mode = c.mode }
  end
  reviera.text.create{ view_id = plan, position = {x - 2, -2.0}, content = i .. ". " .. c.label }
end

print("Overlap resolutions built")
return true
