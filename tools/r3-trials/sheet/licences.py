"""Print one `licence` line of contracts A13.1 for each crate that a candidate ships.

Usage: python3 -I licences.py --read YYYY-MM-DD

A candidate ships the crates that `cargo tree` reaches from its library crates
by normal edges for every target, proc-macro crates and their own dependencies
left out, because those run in the build and are not in a shipped bundle.
"""

import json
import os
import re
import subprocess
import sys

ALLOWED = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0"]
CANDIDATES = {
    "umya": ["umya-spreadsheet", "calamine"],
    "rewrite": ["calamine", "rust_xlsxwriter"],
}
HERE = os.path.dirname(os.path.abspath(__file__))


def cargo(*args):
    return subprocess.run(
        ["cargo", *args], cwd=HERE, check=True, capture_output=True, text=True
    ).stdout


def shipped(root):
    out = cargo(
        "tree", "--locked", "-p", root, "-e", "normal,no-proc-macro",
        "--target", "all", "--prefix", "none", "-f", "{p}",
    )
    found = set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 2:
            found.add((parts[0], parts[1].lstrip("v")))
    return found


def conjuncts(expr):
    expr = expr.replace("/", " OR ")
    depth, parts, current = 0, [], []
    for token in re.findall(r"\(|\)|[^\s()]+", expr):
        if token == "(":
            depth += 1
        elif token == ")":
            depth -= 1
        elif token == "AND" and depth == 0:
            parts.append(current)
            current = []
            continue
        current.append(token)
    parts.append(current)
    return parts


def pick(tokens):
    text = " ".join(t for t in tokens if t not in ("(", ")"))
    options = [o.strip() for o in text.split(" OR ")]
    for allowed in ALLOWED:
        if allowed in options:
            return allowed, True
    return options[0].replace(" ", "_"), False


def main():
    if len(sys.argv) != 3 or sys.argv[1] != "--read" or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", sys.argv[2]):
        sys.stderr.write("REFUSED usage: licences.py --read YYYY-MM-DD\n")
        return 2
    read = sys.argv[2]
    meta = json.loads(cargo("metadata", "--locked", "--format-version", "1"))
    licence_of = {(p["name"], p["version"]): p.get("license") or "" for p in meta["packages"]}
    items = set()
    for roots in CANDIDATES.values():
        for root in roots:
            items |= shipped(root)
    lines = set()
    for item in sorted(items):
        expr = licence_of.get(item, "")
        if not expr:
            lines.add((item[0], "none", "no"))
            continue
        for tokens in conjuncts(expr):
            spdx, ok = pick(tokens)
            lines.add((item[0], spdx, "yes" if ok else "no"))
    for name, spdx, ok in sorted(lines):
        print(f"licence item={name} spdx={spdx} read={read} ok={ok}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
