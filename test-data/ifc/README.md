# IFC validation models

Third-party sample models used by `crates/reviera-ifc/tests` for lossless round-trip and
tessellation checks. They are not covered by the project licence.

| File | Model | Schema | SHA-256 |
|---|---|---|---|
| `AC20-FZK-Haus.ifc` | FZK-Haus (single-family house) | IFC4 | `ea6f04eaf92fac4d7ad0038bc3d2dfea4c094dd3f516ecc33c50bf1835ca108d` |
| `AC20-Institute-Var-2.ifc` | Institute building, variant 2 | IFC4 | `a06b9cf1922cc0a1e200719189054510dd3e1d188292325f66614f7c1fdb37b4` |

**Provenance.** Freely distributed sample models from the Karlsruhe Institute of Technology (KIT,
Institute for Automation and Applied Informatics) FZK model collection, exported from ARCHICAD 20
(file headers dated 2016-12-21 and 2017-01-11). Added at the project owner's direction, 2026-10-12.
They are test inputs only: no Reviera code or specification text is derived from them.

To validate against other local files without adding them to the repository, set
`REVIERA_IFC_SAMPLES` to a directory of `.ifc` files.
