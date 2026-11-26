#!/usr/bin/env python3
"""Checks that every place that carries the release version agrees with the workspace version.

Run from anywhere: `python3 tools/version-check.py [--root DIR] [--tag vX.Y.Z]`. Exits 0 when all sites
agree, 1 on any difference, 2 if a site cannot be parsed.

The source of truth is `version` under `[workspace.package]` in Cargo.toml. Checked against it:
  - core/Cargo.toml and ffi/Cargo.toml must say `version.workspace = true` (no hard-coded copy)
  - Cargo.lock entries for pdfgen-core and ffi
  - node/package.json, and both version fields in node/package-lock.json
  - python/pyproject.toml, `[project] version`
  - java/pom.xml, the project's own <version> (not a dependency or plugin)
  - dotnet/PdfGen/PdfGen.csproj, <Version>
The workspace version must be MAJOR.MINOR.PATCH, optionally with a -prerelease suffix.

`--tag vX.Y.Z` also requires the tag (as `vX.Y.Z`, or `go/vX.Y.Z` as Go needs for a module in a
subdirectory) to match. The Go module has no version in its source; Go clients get it from that tag, so
the Go wrapper cannot drift and is not listed above. The version in the pdfgen.h comment is an
example and is not checked.

Standard library only; works on Python 3.10, so no tomllib.
"""
import json
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

SEMVER = re.compile(r"\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?")


class ParseError(Exception):
    pass


def toml_section(text, header, path):
    """Body of `[header]`, up to the next table header."""
    m = re.search(r"^\[" + re.escape(header) + r"\][ \t]*$(.*?)(?=^\[|\Z)", text, re.M | re.S)
    if not m:
        raise ParseError(f"{path}: no [{header}] table")
    return m.group(1)


def toml_version(text, header, path):
    body = toml_section(text, header, path)
    m = re.search(r'^version[ \t]*=[ \t]*"([^"]+)"', body, re.M)
    if not m:
        raise ParseError(f"{path}: no string `version` in [{header}]")
    return m.group(1)


def cargo_toml(root):
    return toml_version((root / "Cargo.toml").read_text(), "workspace.package", "Cargo.toml")


def member_inherits(root, member):
    path = f"{member}/Cargo.toml"
    body = toml_section((root / path).read_text(), "package", path)
    return re.search(r"^version\.workspace[ \t]*=[ \t]*true[ \t]*$", body, re.M) is not None


def cargo_lock(root):
    text = (root / "Cargo.lock").read_text()
    found = {}
    for block in text.split("[[package]]")[1:]:
        n = re.search(r'^name = "([^"]+)"', block, re.M)
        v = re.search(r'^version = "([^"]+)"', block, re.M)
        if n and v and n.group(1) in ("pdfgen-core", "ffi"):
            found[n.group(1)] = v.group(1)
    for name in ("pdfgen-core", "ffi"):
        if name not in found:
            raise ParseError(f"Cargo.lock: no entry for {name}")
    return found


def node(root):
    pkg = json.loads((root / "node/package.json").read_text())
    lock = json.loads((root / "node/package-lock.json").read_text())
    try:
        return {
            "node/package.json": pkg["version"],
            "node/package-lock.json version": lock["version"],
            'node/package-lock.json packages[""] version': lock["packages"][""]["version"],
        }
    except KeyError as e:
        raise ParseError(f"node: missing key {e}")


def python(root):
    return toml_version((root / "python/pyproject.toml").read_text(), "project", "python/pyproject.toml")


def java(root):
    tree = ET.parse(root / "java/pom.xml").getroot()
    # The namespace is part of every tag; take it from the root element.
    ns = tree.tag[: tree.tag.index("}") + 1] if tree.tag.startswith("{") else ""
    v = tree.find(ns + "version")
    if v is None or not (v.text or "").strip():
        raise ParseError("java/pom.xml: project has no direct <version>")
    return v.text.strip()


def dotnet(root):
    path = "dotnet/PdfGen/PdfGen.csproj"
    m = re.findall(r"<Version>\s*([^<\s]+)\s*</Version>", (root / path).read_text())
    if len(m) != 1:
        raise ParseError(f"{path}: expected exactly one <Version>, found {len(m)}")
    return m[0]


def main(argv):
    root = Path(__file__).resolve().parent.parent
    if "--root" in argv:
        root = Path(argv[argv.index("--root") + 1]).resolve()
    tag = argv[argv.index("--tag") + 1] if "--tag" in argv else None
    try:
        truth = cargo_toml(root)
        sites = {"core/Cargo.toml": member_inherits(root, "core"), "ffi/Cargo.toml": member_inherits(root, "ffi")}
        found = {f"Cargo.lock {k}": v for k, v in cargo_lock(root).items()}
        found.update(node(root))
        found["python/pyproject.toml"] = python(root)
        found["java/pom.xml"] = java(root)
        found["dotnet/PdfGen/PdfGen.csproj"] = dotnet(root)
    except (ParseError, OSError, ValueError, ET.ParseError) as e:
        print(f"version-check: cannot parse: {e}", file=sys.stderr)
        return 2

    problems = []
    if not SEMVER.fullmatch(truth):
        problems.append(f"Cargo.toml: workspace version {truth!r} is not MAJOR.MINOR.PATCH[-prerelease]")
    for site, inherits in sites.items():
        if not inherits:
            problems.append(f"{site}: must use `version.workspace = true`, not its own version")
    for site, v in found.items():
        if v != truth:
            problems.append(f"{site} is {v}, Cargo.toml has {truth}")
    if tag is not None and tag.removeprefix("go/") != f"v{truth}":
        problems.append(f"tag {tag!r} does not match v{truth}")
    if problems:
        print("version-check: FAILED")
        print("\n".join("  " + p for p in problems))
        return 1
    extra = f"; tag {tag} matches" if tag else ""
    print(f"version-check: ok  {truth} in Cargo.toml and {len(found)} other sites{extra}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
