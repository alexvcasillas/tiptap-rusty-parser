#!/usr/bin/env python3
"""Generate large, realistic Tiptap JSONContent documents for perf testing.

    python3 studio/assets/gen_large.py OUT_DIR            # large-1k/10k/50k.json
    python3 studio/assets/gen_large.py OUT_DIR --blocks 10000 --edits 50

Each document has headings, paragraphs with mixed marks (bold, italic, code,
link, strike, an unknown `highlight` mark), bullet/ordered lists nested 4
levels, task lists, tables, code blocks, blockquotes, images, rules and a few
unknown node types (`callout`, inline `mention`). Output is deterministic per
seed. `--edits N` also writes `<name>-edited.json`: a copy with N scattered
edits (text changes, attr changes, inserted and removed blocks) for Diff mode.
Outputs are large: write them outside the repo (e.g. a scratch dir).
"""

import argparse
import copy
import json
import os
import random

WORDS = (
    "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor "
    "incididunt ut labore et dolore magna aliqua enim ad minim veniam quis nostrud "
    "exercitation ullamco laboris nisi aliquip ex ea commodo consequat duis aute irure "
    "in reprehenderit voluptate velit esse cillum fugiat nulla pariatur excepteur sint "
    "occaecat cupidatat non proident sunt culpa qui officia deserunt mollit anim id est"
).split()


def words(r, lo, hi):
    return " ".join(r.choice(WORDS) for _ in range(r.randint(lo, hi)))


def text(t, marks=None):
    n = {"type": "text", "text": t}
    if marks:
        n["marks"] = marks
    return n


def inline(r, lo=1, hi=4):
    out = []
    for _ in range(r.randint(lo, hi)):
        roll = r.random()
        t = words(r, 2, 8) + " "
        if roll < 0.45:
            out.append(text(t))
        elif roll < 0.6:
            out.append(text(t, [{"type": "bold"}]))
        elif roll < 0.7:
            out.append(text(t, [{"type": "italic"}, {"type": "bold"}]))
        elif roll < 0.78:
            out.append(text(t.strip(), [{"type": "code"}]))
            out.append(text(" "))
        elif roll < 0.86:
            href = "https://example.com/" + r.choice(WORDS)
            out.append(
                text(t, [{"type": "link", "attrs": {"href": href, "target": "_blank"}}])
            )
        elif roll < 0.92:
            out.append(text(t, [{"type": "strike"}]))
        elif roll < 0.96:
            out.append(text(t, [{"type": "highlight", "attrs": {"color": "#fde68a"}}]))
        else:
            out.append(
                {"type": "mention", "attrs": {"id": r.randint(1, 999), "label": r.choice(WORDS)}}
            )
    # ProseMirror merges adjacent same-mark text; keep runs as generated.
    return out


def paragraph(r, lo=1, hi=4):
    return {"type": "paragraph", "content": inline(r, lo, hi)}


def heading(r):
    return {
        "type": "heading",
        "attrs": {"level": r.choice([1, 2, 2, 3, 3, 3]), "id": "h-%06d" % r.randint(0, 999999)},
        "content": [text(words(r, 2, 7).capitalize())],
    }


def list_node(r, depth=0, kind=None):
    kind = kind or r.choice(["bulletList", "bulletList", "orderedList"])
    items = []
    for _ in range(r.randint(2, 4)):
        item = {"type": "listItem", "content": [paragraph(r, 1, 1)]}
        if depth < 3 and r.random() < 0.3:
            item["content"].append(list_node(r, depth + 1))
        items.append(item)
    n = {"type": kind, "content": items}
    if kind == "orderedList":
        n["attrs"] = {"start": 1}
    return n


def task_list(r):
    return {
        "type": "taskList",
        "content": [
            {
                "type": "taskItem",
                "attrs": {"checked": r.random() < 0.5},
                "content": [paragraph(r, 1, 2)],
            }
            for _ in range(r.randint(2, 5))
        ],
    }


def table(r):
    cols = 3

    def cell(kind):
        return {
            "type": kind,
            "attrs": {"colspan": 1, "rowspan": 1, "colwidth": None},
            "content": [{"type": "paragraph", "content": [text(words(r, 1, 4))]}],
        }

    rows = [{"type": "tableRow", "content": [cell("tableHeader") for _ in range(cols)]}]
    for _ in range(r.randint(2, 5)):
        rows.append({"type": "tableRow", "content": [cell("tableCell") for _ in range(cols)]})
    return {"type": "table", "content": rows}


def code_block(r):
    lines = [
        "fn %s(x: u32) -> u32 {" % r.choice(WORDS),
        "    let y = x * %d;" % r.randint(2, 99),
        "    y + %d" % r.randint(0, 9),
        "}",
    ]
    return {
        "type": "codeBlock",
        "attrs": {"language": r.choice(["rust", "ts", "python", None])},
        "content": [text("\n".join(lines))],
    }


def block(r):
    roll = r.random()
    if roll < 0.12:
        return heading(r)
    if roll < 0.62:
        return paragraph(r)
    if roll < 0.74:
        return list_node(r)
    if roll < 0.77:
        return task_list(r)
    if roll < 0.82:
        return table(r)
    if roll < 0.87:
        return code_block(r)
    if roll < 0.91:
        return {"type": "blockquote", "content": [paragraph(r, 1, 3) for _ in range(r.randint(1, 2))]}
    if roll < 0.93:
        return {"type": "image", "attrs": {"src": "https://picsum.photos/%d" % r.randint(100, 999), "alt": None}}
    if roll < 0.95:
        return {"type": "horizontalRule"}
    if roll < 0.98:
        return {
            "type": "callout",
            "attrs": {"tone": r.choice(["info", "warning", "danger"]), "emoji": "!"},
            "content": [paragraph(r, 1, 3)],
        }
    return {"type": "customWidget", "attrs": {"widgetId": r.randint(1, 99999), "config": {"a": 1, "b": [1, 2]}}}


def document(n, seed):
    r = random.Random(seed)
    return {"type": "doc", "content": [heading(r)] + [block(r) for _ in range(n - 1)]}


def first_text(node):
    if node.get("type") == "text":
        return node
    for c in node.get("content", []):
        t = first_text(c)
        if t:
            return t
    return None


def edited(doc, n, seed):
    """Copy of `doc` with `n` scattered edits across the whole document."""
    r = random.Random(seed)
    out = copy.deepcopy(doc)
    blocks = out["content"]
    span = max(1, len(blocks) // max(1, n))
    for k in range(n):
        i = min(len(blocks) - 1, k * span + r.randrange(span))
        kind = k % 5
        b = blocks[i]
        if kind == 0:
            t = first_text(b)
            if t:
                t["text"] = "EDITED " + t["text"]
                continue
        if kind == 1 and b.get("type") == "heading":
            b["attrs"]["level"] = 1 if b["attrs"]["level"] != 1 else 2
            continue
        if kind == 2:
            blocks.insert(i, {"type": "paragraph", "content": [text("inserted block %d" % k)]})
            continue
        if kind == 3 and len(blocks) > 2:
            del blocks[i]
            continue
        b.setdefault("attrs", {})["data-edit"] = k
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out_dir")
    ap.add_argument("--blocks", type=int, nargs="*", default=[1000, 10000, 50000])
    ap.add_argument("--edits", type=int, default=50)
    ap.add_argument("--seed", type=int, default=7)
    a = ap.parse_args()
    os.makedirs(a.out_dir, exist_ok=True)
    for n in a.blocks:
        name = "large-%dk" % (n // 1000) if n >= 1000 else "large-%d" % n
        doc = document(n, a.seed)
        path = os.path.join(a.out_dir, name + ".json")
        with open(path, "w") as f:
            json.dump(doc, f, separators=(",", ":"))
        print("%s  %.1f MB" % (path, os.path.getsize(path) / 1e6))
        if a.edits:
            path = os.path.join(a.out_dir, name + "-edited.json")
            with open(path, "w") as f:
                json.dump(edited(doc, a.edits, a.seed + 1), f, separators=(",", ":"))
            print("%s  %.1f MB" % (path, os.path.getsize(path) / 1e6))


if __name__ == "__main__":
    main()
