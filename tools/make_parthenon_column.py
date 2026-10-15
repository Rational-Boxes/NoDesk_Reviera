#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Reviera contributors
"""Builds the sample library's Parthenon-style Doric column (an OBJ with its MTL).

The shaft is a public-domain (CC0) Doric shaft from Wikimedia Commons, "Antike griechische Säule
20220605.stl" by PantheraLeo1359531 (https://commons.wikimedia.org/wiki/File:Antike_griechische_S%C3%A4ule_20220605.stl),
reduced with tools/decimate_stl.py and scaled to Parthenon proportions. The capital (a curved
echinus and a square abacus) is modelled here. The column stands on the origin, Z up, in metres.

    tools/make_parthenon_column.py shaft_reduced.stl out_dir
"""
import math
import struct
import sys

import numpy as np

SCALE = 0.7  # the scan's 13.4 m shaft to the Parthenon's (about 9.4 m)


def read_stl(path):
    b = open(path, "rb").read()
    n = struct.unpack("<I", b[80:84])[0]
    rec = np.frombuffer(b, dtype=np.uint8, count=n * 50, offset=84).reshape(n, 50)
    return rec[:, 12:48].copy().view("<f4").reshape(n, 3, 3).astype(np.float64)


def main():
    src, out = sys.argv[1], sys.argv[2]
    tris = read_stl(src)
    pts = tris.reshape(-1, 3)
    mid = pts[np.abs(pts[:, 2] - np.median(pts[:, 2])) < 0.5]
    axis = [(mid[:, 0].min() + mid[:, 0].max()) / 2, (mid[:, 1].min() + mid[:, 1].max()) / 2]
    zmin = pts[:, 2].min()
    tris = (tris - [axis[0], axis[1], zmin]) * SCALE
    top = tris[:, :, 2].max()
    ring = tris.reshape(-1, 3)
    top_r = np.sqrt((ring[ring[:, 2] > top - 0.2][:, :2] ** 2).sum(axis=1)).max()

    verts, index = [], {}

    def vid(p):
        # Shared vertices (to 0.1 mm), so the file stays small.
        key = tuple(round(v, 4) for v in p)
        if key not in index:
            verts.append(key)
            index[key] = len(verts)
        return index[key]

    # Shaft: the scan's triangles (welded per triangle; the importer welds them again).
    shaft = []
    for t in tris:
        f = [vid(tuple(p)) for p in t]
        if len(set(f)) == 3:
            shaft.append(f)

    # Echinus: a curved cushion from the shaft's top radius out to R, 0.45 m high, closed.
    seg, rows, h, r_out = 48, 8, 0.45, 1.15
    profile = []
    for k in range(rows + 1):
        u = k / rows
        # A convex quarter curve: fast growth at first, flattening under the abacus.
        r = top_r + (r_out - top_r) * math.sin(u * math.pi / 2) ** 0.6
        profile.append((r, top + u * h))
    grid = [[vid((r * math.cos(2 * math.pi * j / seg), r * math.sin(2 * math.pi * j / seg), z))
             for j in range(seg)] for (r, z) in profile]
    echinus = []
    for k in range(rows):
        for j in range(seg):
            a, b = grid[k][j], grid[k][(j + 1) % seg]
            c, d = grid[k + 1][(j + 1) % seg], grid[k + 1][j]
            echinus += [[a, b, c], [a, c, d]]
    bottom_c = vid((0.0, 0.0, top))
    top_c = vid((0.0, 0.0, top + h))
    for j in range(seg):
        echinus.append([bottom_c, grid[0][(j + 1) % seg], grid[0][j]])
        echinus.append([top_c, grid[rows][j], grid[rows][(j + 1) % seg]])

    # Abacus: a square slab on the echinus.
    s, z0, z1 = 1.15, top + h, top + h + 0.35
    c = [vid((x, y, z)) for z in (z0, z1) for (x, y) in ((-s, -s), (s, -s), (s, s), (-s, s))]
    quads = [(0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)]
    abacus = [[c[q[0]], c[q[1]], c[q[2]]] for q in quads] + [[c[q[0]], c[q[2]], c[q[3]]] for q in quads]

    with open(f"{out}/parthenon_column.obj", "w") as f:
        f.write("# Parthenon-style Doric column for Reviera's sample library.\n")
        f.write("# Shaft: CC0, Wikimedia Commons, 'Antike griechische Säule 20220605.stl' by PantheraLeo1359531,\n")
        f.write("# reduced and scaled. Capital modelled by tools/make_parthenon_column.py. Metres, Z up.\n")
        f.write("mtllib parthenon_column.mtl\n")
        for p in verts:
            f.write("v {:.4f} {:.4f} {:.4f}\n".format(*p))
        for name, group in (("marble", shaft), ("marble_capital", echinus + abacus)):
            f.write(f"usemtl {name}\n")
            for t in group:
                f.write("f {} {} {}\n".format(*t))
    with open(f"{out}/parthenon_column.mtl", "w") as f:
        f.write("newmtl marble\nKd 0.91 0.89 0.83\nnewmtl marble_capital\nKd 0.93 0.91 0.86\n")
    print(f"shaft {len(shaft)}, capital {len(echinus) + len(abacus)} triangles; height {z1:.2f} m, "
          f"shaft top radius {top_r:.3f} m")


if __name__ == "__main__":
    main()
