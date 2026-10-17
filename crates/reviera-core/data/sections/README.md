# Standard structural sections

Catalogues of standard steel sections that Reviera builds structural column (and later beam)
types from (`structural.catalogs`, `column.load_section`; spec STR-COL-001). Each file holds a
section family's designations and nominal dimensions only: the dimensions that the family's
standard fixes (ASTM A6 / AISC for US shapes, EN 10365 for European and UK rolled sections,
ASTM A500 / A53 for hollow sections and pipe). Reviera computes everything else (area, mass,
section properties) from these dimensions; nothing else from the source tables is copied.

They are small, deliberately partial subsets compiled on 2026-10-16 from public sources and
cross-checked against a second public source where one was available. The sources are listed
in `design_documents/SOURCE_REGISTER.md` under the IDs in each file's `source_ids` and in the
table below; the session is in `design_documents/RESEARCH_LOG.md`. Dimensions of standard
sections are facts set by public standards and identical across independent sources; the
source databases themselves (AISC Shapes Database, ArcelorMittal sales programme, SCI Blue
Book) are not redistributed, and these catalogues must stay small subsets of nominal
dimensions.

Units are those of the family's standard (`in` or `mm`). Keys: I shapes, channels and tees `d`
(depth), `bf` (flange width), `tw` (web thickness), `tf` (flange thickness), `r` (root radius,
where given); angles and rectangular hollow sections `h`, `b`, `t`; round hollow sections and
pipe `od`, `t`. Fillets are not modelled yet.

| File | Family | Standard | Sections | Sources |
|---|---|---|---:|---|
| `aisc_c.json` | AISC C (American Standard channel) | ASTM A6/A6M | 15 | S0720, S0721 |
| `aisc_hss_rect.json` | AISC HSS rectangular and square | ASTM A500 (also A1085, A847) per AISC | 18 | S0720, S0721, S0726 |
| `aisc_hss_round.json` | AISC HSS round | ASTM A500 (also A1085) per AISC | 13 | S0720, S0721 |
| `aisc_l.json` | AISC L (angle) | ASTM A6/A6M | 14 | S0720, S0721 |
| `aisc_pipe_std.json` | AISC Pipe, Standard weight (STD) | ASTM A53 Gr. B (AISC-listed pipe) | 15 | S0720, S0721, S0727 |
| `aisc_w.json` | AISC W (wide-flange) | ASTM A6/A6M (dimensions) | 60 | S0720, S0721, S0722 |
| `aisc_wt.json` | AISC WT (tee cut from W shape) | ASTM A6/A6M | 11 | S0720, S0721 |
| `eu_hea.json` | HEA (European wide-flange, HE A) | EN 10365:2017 (dimensions) | 19 | S0722, S0723 |
| `eu_heb.json` | HEB (European wide-flange, HE B) | EN 10365:2017 (dimensions) | 19 | S0722, S0723 |
| `eu_ipe.json` | IPE (European I-beam) | EN 10365:2017 (dimensions) | 18 | S0722, S0723 |
| `uk_ub.json` | UB (UK universal beam) | BS EN 10365:2017 (dimensions | 31 | S0724, S0722 |
| `uk_uc.json` | UC (UK universal column) | BS EN 10365:2017 (dimensions | 24 | S0725, S0722 |
