# Reviera Sample Library

A Component Library (DM-LIB-004): a folder of ordinary 3D files with one manifest,
`reviera-library.json`, in its root. Open it in Reviera to place its components; the sample
models use it (`test-data/samples/temple.lua`).

Everything here is free to use, change and redistribute. Each item's provenance is also in its
details in the manifest.

| Item | Licence | Source |
|---|---|---|
| `Classical/Columns/parthenon_column.obj` (+ `.mtl`) | CC0 1.0 (shaft); capital by Reviera contributors, GPL-3.0-or-later or, at your option, CC0 1.0 | Shaft: [*Antike griechische Säule 20220605.stl*](https://commons.wikimedia.org/wiki/File:Antike_griechische_S%C3%A4ule_20220605.stl) by PantheraLeo1359531 on Wikimedia Commons, public-domain dedication (CC0). Reduced from 494,392 to 20,000 triangles with `tools/decimate_stl.py` and scaled by 0.7 to the Parthenon's proportions; the echinus and abacus were added by `tools/make_parthenon_column.py`. |

To rebuild the column from the original scan:

```sh
curl -L -o column.stl 'https://upload.wikimedia.org/wikipedia/commons/7/71/Antike_griechische_S%C3%A4ule_20220605.stl'
tools/decimate_stl.py column.stl column_20k.stl 20000
tools/make_parthenon_column.py column_20k.stl test-data/library/Classical/Columns
```
