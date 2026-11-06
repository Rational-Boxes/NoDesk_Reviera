# Riviera's bundled Component Libraries

Three Component Libraries ship with Riviera (DM-LIB-006). Each is a folder of model fragments,
one IFC4 file an item, with a `riviera-library.json` manifest:

| Library | Folder | Holds |
|---|---|---|
| Riviera Interiors | `interiors/` | beds (US and UK sizes), nightstands, dressers, wardrobes, sofas, armchairs, tables, chairs, desks, a reception desk, a rug; sets: a dining set, a queen bedroom, an office workstation |
| Riviera Gym Equipment | `gym/` | treadmill, elliptical, stair climber, bikes, rower, leg press, cable crossover, racks, Smith machine, dumbbell rack, bench, plate tree, lifting platform |
| Riviera Retail and Checkout Fixtures | `retail/` | gondolas and end caps, a pallet rack bay, a produce table, garment racks, a mannequin, a showcase, a cash wrap, a shopping cart, an EAS pedestal, checkstands, self-checkouts, an attendant station |

Each item is a generic placeholder at the size research note 15 gives
(`design_documents/research/15_gyms_retail_interiors.md`). Each manifest entry has a
`provenance.sources` field with the source IDs, which are listed in
`design_documents/SOURCE_REGISTER.md`. Sizes no source gave are judgement and are marked so in
the item's description (`design_documents/EXPERT_REVIEW.md`, E-103).

Each fragment carries its IFC class and predefined type, or for USERDEFINED its kind as the
type's ElementType. Riviera's rule packs pick furniture by these (ARC-RULE-002), so a placed bed
gets its clear spaces at once. Treadmills, checkstands and other powered items carry a power
connector. Each item's Uniclass 2015 code is in its tags.

Everything here is made by Riviera from `crates/riviera-core/src/furnishings.rs` (ARC-FURN-001).
It is free to use, change and redistribute under CC0 1.0. To rebuild the libraries from the
catalogue (each library's id and each item's GUID are kept), run:

```sh
RIVIERA_REGEN_LIBRARIES=1 cargo test -p riviera-app --test bundled_libraries
```
