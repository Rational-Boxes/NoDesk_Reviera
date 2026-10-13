#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Render Revit_Application_Specification.md to a single self-contained web page.

Usage: python3 design_documents/tools/build_spec_html.py
Output: design_documents/web/reviera_spec.html
Screenshots are intentionally not embedded (internal reference only; see RESEARCH_LOG.md).
"""
import html
import json
import pathlib
import re

import markdown

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "Revit_Application_Specification.md"
OUT = ROOT / "web" / "reviera_spec.html"


HTML_TAGS = set("a abbr b blockquote br code del details div em h1 h2 h3 h4 h5 h6 hr i img kbd li ol p pre s span strong sub summary sup table tbody td th thead tr u ul".split())


def escape_placeholders(body):
    """Escape placeholder text such as "<user>" that Markdown passed through as raw HTML.

    Code spans are already escaped by Markdown, so any remaining tag that is not real HTML is a placeholder.
    An unclosed unknown element swallows everything after it, and sanitising viewers then drop that content.
    """
    return re.sub(r"<(/?)([A-Za-z][\w-]*)([^<>]*)>",
                  lambda m: m.group(0) if m.group(2).lower() in HTML_TAGS
                  else "&lt;" + m.group(1) + m.group(2) + m.group(3) + "&gt;", body)


def render():
    text = SRC.read_text()
    # Drop the generated header + contents table; the page has its own title block and nav.
    text = text[text.index("# Chapter 0"):]
    md = markdown.Markdown(extensions=["tables", "fenced_code", "toc", "sane_lists"],
                           extension_configs={"toc": {"permalink": False}})
    body = md.convert(text)
    body = escape_placeholders(body)

    # Priority badges and requirement anchors.
    body = re.sub(r"<strong>Priority:</strong>\s*(P\d)",
                  lambda m: f'<strong>Priority:</strong> <span class="pri pri-{m.group(1).lower()}">{m.group(1)}</span>', body)
    body = re.sub(r"<strong>ID:</strong>\s*([A-Z]+-[A-Z0-9]+-\d+)",
                  lambda m: f'<strong>ID:</strong> <code class="rid" id="{m.group(1)}">{m.group(1)}</code>', body)
    body = re.sub(r"<td>(P[0-3])</td>", lambda m: f'<td><span class="pri pri-{m.group(1).lower()}">{m.group(1)}</span></td>', body)
    body = body.replace("<li>[ ] ", '<li class="ac"><span class="box" aria-hidden="true"></span>')
    body = body.replace("<table>", '<div class="tbl"><table>').replace("</table>", "</table></div>")
    body = body.replace("<hr />", "")

    # Split into chapter sections at each <h1>.
    chunks = re.split(r"(?=<h1 )", body)
    sections, nav, index = [], [], []
    for chunk in chunks:
        m = re.match(r'<h1 id="([^"]+)">(.*?)</h1>', chunk)
        if not m:
            continue
        cid, ctitle = m.group(1), re.sub("<[^>]+>", "", m.group(2))
        label, _, name = ctitle.partition(" — ")
        # Drawing-callout bubble: chapter number over the part type, as on a sheet reference.
        kind, _, num = label.partition(" ")
        bubble = f'<span class="bubble" aria-hidden="true"><b>{html.escape(num)}</b><i>{"CH" if kind == "Chapter" else "APP"}</i></span>'
        chunk = chunk.replace(m.group(0), f'<h1 id="{cid}">{bubble}<span><span class="sr">{html.escape(label)}: </span>{html.escape(html.unescape(name or label))}</span></h1>', 1)
        subs = [(i, re.sub("<[^>]+>", "", t)) for i, t in re.findall(r'<h2 id="([^"]+)">(.*?)</h2>', chunk)]
        sections.append(f'<section class="chapter" id="sec-{cid}">{chunk}</section>')
        sub_html = "".join(f'<li><a href="#{i}">{html.escape(html.unescape(t))}</a></li>' for i, t in subs)
        nav.append(f'<li class="nav-ch"><details><summary><a href="#{cid}"><span class="ch-no">{html.escape(label)}</span>'
                   f'<span class="ch-name">{html.escape(html.unescape(name or label))}</span></a></summary><ol>{sub_html}</ol></details></li>')
        for lvl, i, t in re.findall(r'<h([1-4]) id="([^"]+)">(.*?)</h\1>', chunk):
            index.append([html.unescape(re.sub("<[^>]+>", "", t)), i, int(lvl), label])
    return "".join(sections), "".join(nav), index


def stats():
    a = (ROOT / "spec" / "A_indexes.md").read_text()
    tot = re.search(r"\| \*\*Total\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\*", a)
    cmds = re.search(r"\*\*(\d+) unique commands\*\*", a).group(1)
    return dict(p0=tot.group(1), p1=tot.group(2), p2=tot.group(3), p3=tot.group(4), reqs=tot.group(5), cmds=cmds)


TEMPLATE = (pathlib.Path(__file__).parent / "spec_page_template.html").read_text()


def main():
    sections, nav, index = render()
    s = stats()
    page = (TEMPLATE.replace("{{NAV}}", nav).replace("{{CONTENT}}", sections)
            .replace("{{INDEX}}", json.dumps(index, ensure_ascii=False).replace("</", "<\\/")))
    for k, v in s.items():
        page = page.replace("{{" + k.upper() + "}}", f"{int(v):,}")
    OUT.parent.mkdir(exist_ok=True)
    OUT.write_text(page)
    print(f"wrote {OUT} ({OUT.stat().st_size/1e6:.1f} MB, {len(index)} headings)")


if __name__ == "__main__":
    main()
