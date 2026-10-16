#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Regenerate spec/A_indexes.md and Revit_Application_Specification.md from spec/*.md.

Usage: python3 design_documents/tools/build_spec.py
"""
import collections
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent
SPEC = ROOT / "spec"
CHAPTERS = [  # (file, chapter label used in indexes)
    ("01_ui_shell.md", "1"),
    ("02_data_model_platform.md", "2"),
    ("02a_extension_data_format.md", "2a"),
    ("02b_live_model_store.md", "2b"),
    ("03_architecture_modeling.md", "3"),
    ("04_structure_mep.md", "4"),
    ("05_views_documentation.md", "5"),
    ("06_collaboration_coordination.md", "6"),
    ("07_advanced_features.md", "7"),
    ("08_remote_computation_analysis.md", "8"),
]
AREAS = ["UI", "DM", "ARC", "STR", "MEP", "ANA", "DOC", "COL", "ADV"]
# Owner of a shared command's signature when it is not the first chapter that lists it.
OWNERS = {"colorscheme.create": "5", "dim.make_permanent": "5", "export.dwg": "5", "export.dxf": "5",
          "export.gltf": "5", "export.image": "5", "export.obj": "5", "export.pdf": "5",
          "rxd.reconcile": "2a", "view.set_color_scheme": "5", "view.temp_apply": "5", "view.temp_reset": "5"}


def build_indexes():
    cmds, uses, reqs = collections.OrderedDict(), collections.defaultdict(list), {}
    sigs = collections.defaultdict(set)
    originals = collections.OrderedDict()
    for fname, ch in CHAPTERS:
        text = (SPEC / fname).read_text()
        in_catalog = False
        for line in text.split("\n"):
            if re.match(r"^#+ ", line):
                low = line.lower()
                in_catalog = "command" in low and ("catalog" in low or line.strip().endswith("Commands"))
            if in_catalog and line.startswith("| `"):
                cells = [c.strip() for c in line.strip().strip("|").split("|")]
                if len(cells) < 5:
                    continue
                # A row may list several commands: "`a.b` / `a.c`" or "`a.b` / `.c`" (same domain prefix).
                names, prev = [], None
                for tok in re.findall(r"`([a-z_0-9.]+)`", cells[0].split("(")[0]):
                    if tok.startswith(".") and prev:
                        tok = prev.rsplit(".", 1)[0] + tok
                    if "." in tok:
                        names.append(tok)
                        prev = tok
                for name in names:
                    uses[name].append(ch)
                    sigs[name].add(tuple(cells[1:4]))
                    cmds.setdefault(name, (ch, *cells[1:5]))
        for m in re.finditer(r"\*\*ID:\*\*\s*([A-Z]+-[A-Z0-9]+-\d+)[^\n]*?\*\*Priority:\*\*\s*([^·\n]+)", text):
            reqs.setdefault(m.group(1), m.group(2))
        # Reviera-originated features (owner directives, no reference-product counterpart).
        # A requirement whose Ref line cites no reference-product research is Reviera's own design.
        for m in re.finditer(r"^\*\*ID:\*\*\s*([A-Z]+-[A-Z0-9]+-\d+)([^\n]*)", text, re.M):
            if "research/" in m.group(2) or "web:" in m.group(2):
                continue
            title = re.findall(r"^#{3,5} (.+)$", text[:m.start()], re.M)
            originals.setdefault(m.group(1), (ch, re.sub(r"^[A-Z]+-[A-Z0-9]+-\d+ — ", "", title[-1]) if title else ""))
        # Ranged headers ("**ID:** DM-MAT-002 … 006"): count every ID in the range under the header's priority.
        for m in re.finditer(r"\*\*ID:\*\*\s*([A-Z]+-[A-Z0-9]+-)(\d+)\s*(?:…|\.\.\.|–|-)\s*(\d+)[^\n]*?\*\*Priority:\*\*\s*([^·\n]+)", text):
            pre, a, b, prio = m.group(1), int(m.group(2)), int(m.group(3)), m.group(4)
            for n in range(a + 1, b + 1):
                reqs.setdefault(f"{pre}{n:03d}", prio)
        for m in re.finditer(r"^\| ([A-Z]+-[A-Z0-9]+-\d+) \|(.*)\| (P\d[^|]*)\|\s*$", text, re.M):
            reqs.setdefault(m.group(1), m.group(3))
        # Chapters 2a and 2b (RXD format, live store) are wholly Reviera's own design.
        if ch in ("2a", "2b"):
            for m in re.finditer(r"^\| ([A-Z]+-[A-Z0-9]+-\d+) \| ([^|]+)\|", text, re.M):
                desc = re.sub(r"^Reviera shall ", "", m.group(2).strip().strip("*"))
                originals.setdefault(m.group(1), (ch, (desc[:90].rsplit(" ", 1)[0] + " …") if len(desc) > 90 else desc))
        # Grouped requirements ("**Priority:** see table"): read the Priority column of tables keyed by ID.
        pri_col = None
        for line in text.split("\n"):
            if not line.startswith("|"):
                pri_col = None
                continue
            cells = [c.strip() for c in re.split(r"(?<!\\)\|", line.strip())[1:-1]]
            if cells and cells[0] == "ID" and "Priority" in cells:
                pri_col = cells.index("Priority")
            elif pri_col is not None and re.fullmatch(r"[A-Z]+-[A-Z0-9]+-\d+", cells[0]) and len(cells) > pri_col:
                if not re.search(r"P\d", reqs.get(cells[0], "")):
                    reqs[cells[0]] = cells[pri_col]

    by_area, totals = collections.defaultdict(collections.Counter), collections.Counter()
    for rid, prio in reqs.items():
        p = (re.findall(r"P\d", prio) or ["?"])[0]
        by_area[rid.split("-")[0]][p] += 1
        totals[p] += 1
    shared = {}
    for k, v in uses.items():
        chs = list(dict.fromkeys(v))
        if len(chs) > 1:
            owner = OWNERS.get(k, chs[0])
            shared[k] = ([owner] + [c for c in chs if c != owner], len(sigs[k]) == 1)
    mismatched = sorted(k for k, (_, ok) in shared.items() if not ok)
    domains = collections.Counter(k.split(".")[0] for k in cmds)

    out = ["# Appendix A — Consolidated Indexes", "",
           "Generated by `tools/build_spec.py` from the chapter command catalogs and requirement blocks. "
           "The owning chapters are normative; this appendix is only an index.", "",
           "## A.1 Requirement summary", "",
           "| Area | P0 | P1 | P2 | P3 | Total |", "|------|---:|---:|---:|---:|------:|"]
    for a in AREAS:
        c = by_area[a]
        out.append(f"| {a} | {c['P0']} | {c['P1']} | {c['P2']} | {c['P3']} | {sum(c.values())} |")
    out += [f"| **Total** | **{totals['P0']}** | **{totals['P1']}** | **{totals['P2']}** | **{totals['P3']}** | **{len(reqs)}** |", "",
            "Where a requirement gives two priorities (for example \"P0 (Local), P1 (BIMserver)\"), it is counted under the first. "
            "The non-functional requirements (NFR-…) are in Chapter 0 §0.7 and are not counted here.", "",
            "## A.2 Commands shared across chapters", "",
            f"{len(shared)} commands appear in more than one chapter's catalog. The first chapter listed owns the command's "
            "signature; the others repeat it with their own requirement IDs. The build compares the parameters, returns and "
            f"undo columns of every copy: **{len(shared) - len(mismatched)} match, {len(mismatched)} differ**"
            + (f" ({', '.join('`' + k + '`' for k in mismatched)})." if mismatched else "."), "",
            "| Command | Chapters (owner first) | Signatures |", "|---------|------------------------|------------|"]
    out += [f"| `{k}` | {', '.join(v)} | {'identical' if ok else '**differ**'} |" for k, (v, ok) in sorted(shared.items())]
    out += ["", "## A.2a Reviera-originated features", "",
            f"{len(originals)} requirements are Reviera's own design: owner directives, the technology stack (IFC/RXD storage, "
            "live store, BIMserver, MCP, scripting) and extensions beyond the reference product's feature set. Their Ref lines "
            "cite no research on the reference product. Listing them separately keeps the clean-room provenance clear: every "
            "other requirement traces to public research on the reference product.", "",
            "| Requirement | Ch | Feature |", "|-------------|----|---------|"]
    out += [f"| {r} | {c} | {t} |" for r, (c, t) in originals.items()]
    out += ["", "## A.3 Consolidated command index", "",
            f"**{len(cmds)} unique commands** in {len(domains)} domains. Every command is exposed automatically to the UI, "
            "to Lua (`reviera.<domain>.<verb>`) and to MCP (`<domain>_<verb>`), subject to its permission class.", "",
            "| Domain | Commands |", "|--------|---------:|"]
    out += [f"| `{d}` | {n} |" for d, n in sorted(domains.items())]
    out += ["", "| Command | Ch | Parameters | Returns | Undoable | Req ID |",
            "|---------|----|------------|---------|----------|--------|"]
    out += [f"| `{k}` | {v[0]} | {v[1]} | {v[2]} | {v[3]} | {v[4]} |" for k, v in sorted(cmds.items())]
    (SPEC / "A_indexes.md").write_text("\n".join(out) + "\n")
    if mismatched:
        print("signature mismatch:", ", ".join(mismatched))
    unparsed = sorted(r for r, p in reqs.items() if not re.search(r"P\d", p))
    if unparsed:
        print("unparsed priority:", ", ".join(unparsed))
    return len(cmds), len(reqs), totals


HEADER = """# Reviera Application Specification

**Functional reference:** Autodesk® Revit® 2026/2027, described only from publicly available sources
**Product:** Reviera, an open-source BIM authoring application (GPL-3.0-or-later; RXD schemas MIT)
**Compiled from:** `design_documents/spec/*.md` by `tools/build_spec.py`. Edit the chapter files, not this file.
**Companion documents:** `TECH_STACK.md` (architecture and licensing) · `RESEARCH_LOG.md` (clean-room provenance) · `SOURCE_REGISTER.md` (public sources, reference screenshots) · `research/01–07` (raw research, internal)

> Autodesk and Revit are trademarks of Autodesk, Inc., used here only to identify the functional reference. Reviera is not affiliated with or endorsed by Autodesk. Screenshot paths cited in "Ref:" lines point to internal reference material; it is not part of Reviera.

"""


def assemble():
    order = ["00_overview.md"] + [f for f, _ in CHAPTERS] + ["A_indexes.md", "00_conventions.md"]
    renames = {"00_overview.md": ("# Reviera Application Specification", "# Chapter 0 — Overview"),
               "00_conventions.md": ("# Specification Conventions (for chapter authors)", "# Appendix B — Specification Conventions")}
    toc = ["## Contents", "", "| Part | Title | Source file |", "|------|-------|-------------|"]
    parts = []
    for fname in order:
        text = (SPEC / fname).read_text()
        if fname in renames:
            text = text.replace(*renames[fname], 1)
        title = re.search(r"^# (.+)$", text, re.M).group(1)
        anchor = re.sub(r"[^a-z0-9 -]", "", title.lower()).replace(" ", "-")
        label, _, rest = title.partition(" — ")
        toc.append(f"| [{label}](#{anchor}) | {rest} | `spec/{fname}` |")
        parts.append(text.rstrip() + "\n")
    doc = HEADER + "\n".join(toc) + "\n\n---\n\n" + "\n\n---\n\n".join(parts)
    (ROOT / "Revit_Application_Specification.md").write_text(doc)


if __name__ == "__main__":
    n_cmds, n_reqs, totals = build_indexes()
    assemble()
    print(f"{n_reqs} requirements {dict(totals)}; {n_cmds} commands; spec assembled")
