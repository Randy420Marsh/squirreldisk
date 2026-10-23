#!/usr/bin/env python3
"""Writes THIRD-PARTY-LICENSES.txt: the licenses of the fonts and Rust crates
built into SquirrelDisk.

    scripts/third-party-licenses.py [--check]

Run it after changing dependencies (Cargo.lock) or fonts. It works offline,
from `cargo metadata` and the crate sources cargo already downloaded; run
`cargo fetch` first if a crate is missing. Only crates that end up in a
release are listed: normal dependencies (not dev or build-only ones) for the
platforms we ship, with the default features. --check exits 1 if the file is
out of date, for CI.
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "THIRD-PARTY-LICENSES.txt")
TARGETS = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "aarch64-linux-android",
]
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|copyright|notice|unlicense|ofl|ufl)([-_.].*)?$", re.I)
RULE = "=" * 78


def metadata(target):
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--offline", "--locked", "--filter-platform", target],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout
    return json.loads(out)


def shipped():
    """The packages built into a release on any target, by id."""
    packages = {}
    for target in TARGETS:
        meta = metadata(target)
        packages.update({p["id"]: p for p in meta["packages"]})
        nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
        root = meta["resolve"]["root"]
        seen, todo = set(), [root]
        while todo:
            node = nodes[todo.pop()]
            for dep in node["deps"]:
                # a null kind is a normal dependency; build and dev ones don't ship
                if any(k["kind"] is None for k in dep["dep_kinds"]) and dep["pkg"] not in seen:
                    seen.add(dep["pkg"])
                    todo.append(dep["pkg"])
        yield from ((i, packages[i]) for i in seen)


def read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read().replace("\r\n", "\n").strip("\n")


def license_texts(pkg):
    """(file name, text) for each license file the crate ships."""
    d = os.path.dirname(pkg["manifest_path"])
    names = sorted(n for n in os.listdir(d) if LICENSE_FILE.match(n) and os.path.isfile(os.path.join(d, n)))
    if pkg.get("license_file"):
        lf = os.path.normpath(os.path.join(d, pkg["license_file"]))
        if os.path.isfile(lf) and os.path.basename(lf) not in names:
            return [(os.path.basename(lf), read(lf))] + [(n, read(os.path.join(d, n))) for n in names]
    return [(n, read(os.path.join(d, n))) for n in names]


APACHE_START = "TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION"
APACHE_END = "END OF TERMS AND CONDITIONS"
APACHE_REF = "[The Apache License, Version 2.0: the full text is at the end of this file.]"
MIT = """MIT License

Copyright (c) the copyright holders named above

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."""


def shorten(text, apache):
    """The Apache License appears once, at the end: a crate's copy becomes a
    pointer to it, keeping the crate's own copyright lines. `apache` collects
    the first full copy."""
    start, end = text.find(APACHE_START), text.find(APACHE_END)
    if start < 0 or end < start:
        return text
    head_end = text.rfind("Apache License", 0, start)
    if head_end < 0:
        return text
    head_end = text.rfind("\n", 0, head_end) + 1
    if not apache:
        apache.append("\n".join(l.rstrip() for l in text[head_end : end + len(APACHE_END)].split("\n")).strip("\n"))
    head = text[:head_end].strip("\n")
    # the appendix is the "how to apply" boilerplate; keep filled-in copyrights
    own = [l.strip() for l in text[end:].split("\n") if "Copyright" in l and "[" not in l]
    return "\n".join(x for x in [head, APACHE_REF, *own] if x)


def fonts():
    """The fonts compiled into the app: ours, then egui's defaults."""
    out = [RULE, "FONTS", RULE, ""]
    out.append("Inter (Regular, SemiBold), Fredoka (SemiBold): assets/fonts")
    out.append("")
    out.append(read(os.path.join(ROOT, "assets/fonts/OFL.txt")))
    epaint = [p for _, p in shipped_list if p["name"] == "epaint_default_fonts"]
    if epaint:
        d = os.path.join(os.path.dirname(epaint[0]["manifest_path"]), "fonts")
        for title, f in [
            ("Ubuntu Light (egui's default font), Copyright 2011 Canonical Ltd.", "UFL.txt"),
            ("Noto Emoji (egui's default emoji font), Copyright 2013 Google Inc.", "OFL.txt"),
            ("Hack (egui's default monospace font)", "Hack-Regular.txt"),
            ("emoji-icon-font (egui's default icon font)", "emoji-icon-font-mit-license.txt"),
        ]:
            out += ["", "-" * 78, title, "-" * 78, "", read(os.path.join(d, f))]
    return out


def crates():
    out = [RULE, "RUST CRATES", RULE, ""]
    pkgs = sorted((p for _, p in shipped_list if p["source"]), key=lambda p: (p["name"], p["version"]))
    out.append(f"{len(pkgs)} crates, with the license each is distributed under:")
    out.append("")
    for p in pkgs:
        out.append(f"  {p['name']} {p['version']}: {p.get('license') or 'see its license file'}")
    # crates with the same license files share one copy of the text
    groups = {}
    missing = []
    apache = []
    for p in pkgs:
        texts = license_texts(p)
        if not texts:
            missing.append(p)
            continue
        key = tuple(shorten(t, apache) for _, t in texts)
        groups.setdefault(key, []).append(p)
    for key, members in sorted(groups.items(), key=lambda g: (g[1][0]["name"], g[1][0]["version"])):
        names = ", ".join(f"{p['name']} {p['version']}" for p in members)
        out += ["", RULE, names, RULE]
        for text in key:
            out += ["", text]
    if missing:
        out += ["", RULE, "Crates without a license file of their own", RULE, ""]
        for p in missing:
            authors = ", ".join(p.get("authors") or []) or "its authors"
            out.append(f"  {p['name']} {p['version']}: {p.get('license')}, copyright {authors}")
        out += ["", "Their license texts are the standard MIT and Apache texts below, and the"]
        out += ["Boost (BSL-1.0) text given above for the crates that ship it."]
    out += ["", RULE, "STANDARD LICENSE TEXTS", RULE, "", "-" * 78, "MIT", "-" * 78, "", MIT]
    if apache:
        out += ["", "-" * 78, "Apache License, Version 2.0", "-" * 78, "", apache[0]]
    return out


def main():
    global shipped_list
    shipped_list = list({i: p for i, p in shipped()}.items())
    text = "\n".join(
        [
            "SquirrelDisk is licensed under the GNU Affero General Public License",
            "version 3 (see LICENSE). It includes the fonts and Rust crates below,",
            "each under its own license. This file is made by",
            "scripts/third-party-licenses.py; don't edit it by hand.",
            "",
        ]
        + fonts()
        + [""]
        + crates()
    ) + "\n"
    if "--check" in sys.argv:
        current = open(OUT, encoding="utf-8").read() if os.path.exists(OUT) else ""
        if current != text:
            print("THIRD-PARTY-LICENSES.txt is out of date: run scripts/third-party-licenses.py", file=sys.stderr)
            sys.exit(1)
        return
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"wrote {OUT} ({len(text) // 1024} KiB)")


if __name__ == "__main__":
    main()
