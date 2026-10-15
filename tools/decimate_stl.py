#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Reviera contributors
"""Reduces a binary STL to about a target triangle count by vertex clustering.

Vertices are snapped to a uniform grid, each cell's vertices merged at their mean, and triangles
that collapse (or repeat) dropped. The cell size is found by bisection so the result has at most
the target count. Used to prepare sample library models from large public-domain scans.

    tools/decimate_stl.py input.stl output.stl 40000
"""
import struct
import sys

import numpy as np


def read(path):
    b = open(path, "rb").read()
    n = struct.unpack("<I", b[80:84])[0]
    if len(b) != 84 + 50 * n:
        sys.exit("only binary STL is read")
    rec = np.frombuffer(b, dtype=np.uint8, count=n * 50, offset=84).reshape(n, 50)
    v = rec[:, 12:48].copy().view("<f4").reshape(n, 3, 3).astype(np.float64)
    return v


def cluster(tris, cell):
    pts = tris.reshape(-1, 3)
    keys = np.floor((pts - pts.min(axis=0)) / cell).astype(np.int64)
    uniq, inv = np.unique(keys, axis=0, return_inverse=True)
    inv = inv.reshape(-1)
    sums = np.zeros((len(uniq), 3))
    np.add.at(sums, inv, pts)
    counts = np.bincount(inv, minlength=len(uniq))[:, None]
    verts = sums / counts
    f = inv.reshape(-1, 3)
    keep = (f[:, 0] != f[:, 1]) & (f[:, 1] != f[:, 2]) & (f[:, 0] != f[:, 2])
    f = f[keep]
    # Drop repeated triangles (same vertices, same winding).
    canon = np.sort(f, axis=1)
    _, first = np.unique(canon, axis=0, return_index=True)
    return verts, f[np.sort(first)]


def write(path, verts, faces):
    out = bytearray(b"Reduced by tools/decimate_stl.py".ljust(80, b" "))
    out += struct.pack("<I", len(faces))
    for a, b, c in faces:
        p, q, r = verts[a], verts[b], verts[c]
        n = np.cross(q - p, r - p)
        n = n / (np.linalg.norm(n) or 1.0)
        out += struct.pack("<12fH", *n, *p, *q, *r, 0)
    open(path, "wb").write(out)


def main():
    src, dst, target = sys.argv[1], sys.argv[2], int(sys.argv[3])
    tris = read(src)
    size = np.ptp(tris.reshape(-1, 3), axis=0).max()
    lo, hi = size / 2000.0, size / 10.0
    best = None
    for _ in range(24):
        mid = (lo * hi) ** 0.5
        verts, faces = cluster(tris, mid)
        if len(faces) > target:
            lo = mid
        else:
            hi, best = mid, (verts, faces, mid)
    verts, faces, cell = best
    write(dst, verts, faces)
    print(f"{len(tris)} -> {len(faces)} triangles, cell {cell:.4f}")


if __name__ == "__main__":
    main()
