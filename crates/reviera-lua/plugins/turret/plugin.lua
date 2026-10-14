-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- Castle Turret: a scripting capabilities test. It asks for the turret in a form, then builds
-- it from Reviera's own tools, as a user would:
--   * a round tower: a ring of wall segments (a polygon until curved walls arrive);
--   * a roof: a cone (a hip roof over the polygon), a pointed or onion dome, a hemisphere,
--     or a flat battlement walk (a floor) behind a crenellated parapet;
--   * arrow-slit windows, fitted to their walls.
-- Everything it makes is one undo step.

local turret = {
  title = "Castle Turret",
  description = "A round tower with a conical, domed or battlemented top and arrow slits",
}

local ROOFS = { "Cone", "Pointed dome", "Onion dome", "Hemisphere", "Battlements" }
local WALL = 0.2 -- the default wall type's width

-- A point on the circle of radius r at corner k of n (starting at angle a0).
local function corner(cx, cy, r, k, n, a0)
  local a = a0 + 2 * math.pi * k / n
  return { cx + r * math.cos(a), cy + r * math.sin(a) }
end

-- Walls round the ring, clockwise so their exterior faces are outside. Returns their ids.
local function ring(level, cx, cy, r, n, base, height)
  local ids = {}
  for k = 0, n - 1 do
    local a, b = corner(cx, cy, r, k + 1, n, 0), corner(cx, cy, r, k, n, 0)
    ids[#ids + 1] = reviera.wall.create{
      curve = { line = { start = a, ["end"] = b } },
      base_level = level, base_offset = base,
      top_constraint = { unconnected = height },
    }
  end
  return ids
end

-- The footprint's corners at the walls' exterior faces.
local function outside(cx, cy, r, n)
  local apothem = r * math.cos(math.pi / n) + WALL / 2
  local circum = apothem / math.cos(math.pi / n)
  return circum, apothem
end

-- A crenellated parapet: each side split into merlons and lower crenels.
local function parapet(level, cx, cy, r, n, base, merlon, crenel, height)
  local made = 0
  for k = 0, n - 1 do
    local a, b = corner(cx, cy, r, k + 1, n, 0), corner(cx, cy, r, k, n, 0)
    local dx, dy = b[1] - a[1], b[2] - a[2]
    local len = math.sqrt(dx * dx + dy * dy)
    local units = math.max(1, math.floor(len / (merlon + crenel) + 0.5))
    local step = len / units
    for u = 0, units - 1 do
      -- A merlon (full height) then a crenel (the lower part only).
      local s0, s1, s2 = u * step, u * step + step * merlon / (merlon + crenel), (u + 1) * step
      local p = function(s) return { a[1] + dx * s / len, a[2] + dy * s / len } end
      reviera.wall.create{
        curve = { line = { start = p(s0), ["end"] = p(s1) } },
        base_level = level, base_offset = base, top_constraint = { unconnected = height },
      }
      reviera.wall.create{
        curve = { line = { start = p(s1), ["end"] = p(s2) } },
        base_level = level, base_offset = base, top_constraint = { unconnected = height * 0.45 },
      }
      made = made + 2
    end
  end
  return made
end

function turret.run()
  local levels = reviera.level.list()
  if #levels == 0 then
    error("the model has no levels")
  end
  local names = {}
  for i, l in ipairs(levels) do names[i] = l.name end

  local t = reviera.ui.form{
    title = "Castle Turret",
    message = "A round tower built from walls, a roof and windows.",
    fields = {
      { key = "level", label = "Level", type = "choice", choices = names, default = names[1] },
      { key = "x", label = "Centre X", type = "length", default = 0 },
      { key = "y", label = "Centre Y", type = "length", default = 0 },
      { key = "radius", label = "Radius", type = "length", default = 2.5, min = 1.0, max = 15,
        help = "To the walls' centrelines" },
      { key = "sides", label = "Sides", type = "integer", default = 16, min = 6, max = 48,
        help = "Wall segments round the tower" },
      { key = "height", label = "Wall Height", type = "length", default = 7.0, min = 2.5, max = 60 },
      { key = "roof", label = "Top", type = "choice", choices = ROOFS, default = "Cone" },
      { key = "roof_height", label = "Roof Height", type = "length", default = 4.0, min = 0.5, max = 40,
        help = "The cone's or dome's rise (a hemisphere's is its radius)" },
      { key = "slits", label = "Arrow Slits", type = "integer", default = 4, min = 0, max = 24 },
      { key = "slit_sill", label = "Slit Sill", type = "length", default = 2.5, min = 0.3 },
      { key = "merlon", label = "Merlon Width", type = "length", default = 0.6, min = 0.2 },
      { key = "crenel", label = "Crenel Width", type = "length", default = 0.4, min = 0.2 },
    },
  }
  if t == nil then
    print("Turret cancelled")
    return nil
  end
  local level
  for _, l in ipairs(levels) do
    if l.name == t.level then level = l.id end
  end
  local n, r = t.sides, t.radius
  local walls = ring(level, t.x, t.y, r, n, 0, t.height)
  local circum, apothem = outside(t.x, t.y, r, n)
  local roof, extra = nil, 0

  if t.roof == "Cone" then
    -- A hip roof over the polygon at the walls' exterior faces: a cone of n facets.
    local curves = {}
    for k = 0, n - 1 do
      curves[#curves + 1] = { line = { start = corner(t.x, t.y, circum, k, n, 0),
                                      ["end"] = corner(t.x, t.y, circum, k + 1, n, 0) },
                              overhang = 0.3 }
    end
    roof = reviera.roof.create_footprint{
      curves = curves, level_id = level, base_offset = t.height,
      slope = math.atan(t.roof_height / apothem),
    }
  elseif t.roof == "Battlements" then
    -- A walk on top of the walls, behind a parapet of merlons and crenels.
    local curves = {}
    for k = 0, n - 1 do
      curves[#curves + 1] = { line = { start = corner(t.x, t.y, circum, k, n, 0),
                                      ["end"] = corner(t.x, t.y, circum, k + 1, n, 0) } }
    end
    roof = reviera.floor.create{ loops = { curves }, level_id = level, offset = t.height }
    extra = parapet(level, t.x, t.y, r, n, t.height, t.merlon, t.crenel, 1.2)
  else
    local profile = ({ ["Pointed dome"] = "pointed", ["Onion dome"] = "onion",
                       ["Hemisphere"] = "hemisphere" })[t.roof]
    roof = reviera.roof.create_dome{
      level_id = level, base_offset = t.height, center = { t.x, t.y },
      plan = { polygon = { radius = circum, sides = n, rotation = 0 } },
      profile = profile, rise = t.roof_height,
    }
  end

  -- Arrow slits: narrow windows spread round the ring, fitted to their walls.
  local slits = 0
  for i = 0, t.slits - 1 do
    local w = walls[math.floor(i * n / t.slits) + 1]
    local ok = pcall(reviera.window.place, {
      host_wall_id = w, point_along = 2 * r * math.sin(math.pi / n) / 2,
      sill_height = t.slit_sill, width = 0.25, height = 1.2,
    })
    if ok then slits = slits + 1 end
  end

  print(string.format("Turret: %d walls, %s, %d arrow slits%s", #walls, t.roof:lower(), slits,
    extra > 0 and string.format(", %d parapet walls", extra) or ""))
  return { walls = walls, roof = roof, slits = slits, parapet = extra }
end

return turret
