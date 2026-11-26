#!/usr/bin/env python3
"""Checks every package Maven resolves for java/ (test scope included) against the license allowlist.

Run from java/: `python3 license-check.py`. Exits 0 only if every package's declared license is allowed.
The license comes from the package's POM <licenses> block, following <parent> POMs. A package whose POMs
declare nothing must be listed in UNDECLARED with the evidence, otherwise the check fails.
"""

import re
import subprocess
import sys
import tempfile
from pathlib import Path

ALLOWED = {"MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0", "Zlib"}

NAMES = {
    "apache license, version 2.0": "Apache-2.0",
    "the apache software license, version 2.0": "Apache-2.0",
    "apache-2.0": "Apache-2.0",
    "mit license": "MIT",
    "the mit license": "MIT",
    "mit": "MIT",
}

# No <licenses> in any POM; the license is in the jar. slf4j-api's META-INF/LICENSE.txt is the MIT text.
UNDECLARED = {("org.slf4j", "slf4j-api"): "MIT"}

REPO = Path.home() / ".m2" / "repository"


def pom_path(group: str, artifact: str, version: str) -> Path:
    return REPO / group.replace(".", "/") / artifact / version / f"{artifact}-{version}.pom"


def declared(group: str, artifact: str, version: str) -> list[str]:
    """License names from the POM, or from the nearest parent that declares any."""
    pom = pom_path(group, artifact, version)
    if not pom.is_file():
        return []
    text = pom.read_text(encoding="utf-8")
    block = re.search(r"<licenses>(.*?)</licenses>", text, re.S)
    if block:
        return [n.strip() for n in re.findall(r"<name>(.*?)</name>", block.group(1), re.S)]
    parent = re.search(r"<parent>(.*?)</parent>", text, re.S)
    if parent:
        g, a, v = (re.search(rf"<{t}>(.*?)</{t}>", parent.group(1)).group(1).strip() for t in ("groupId", "artifactId", "version"))
        return declared(g, a, v)
    return []


def resolved() -> list[tuple[str, str, str]]:
    with tempfile.NamedTemporaryFile("r", suffix=".txt") as out:
        subprocess.run(
            ["mvn", "-q", "-B", "dependency:list", "-DincludeScope=test", f"-DoutputFile={out.name}"],
            check=True,
            capture_output=True,
        )
        lines = out.read().splitlines()
    found = []
    for line in lines:
        parts = line.strip().split(":")
        if len(parts) >= 5 and parts[2] == "jar":
            found.append((parts[0], parts[1], parts[3]))
    return sorted(found)


def main() -> int:
    packages = resolved()
    if not packages:
        print("no packages resolved; is this run from java/?", file=sys.stderr)
        return 1
    failed = False
    for group, artifact, version in packages:
        names = declared(group, artifact, version)
        licenses = {NAMES.get(n.lower(), n) for n in names} or (
            {UNDECLARED[(group, artifact)]} if (group, artifact) in UNDECLARED else set()
        )
        ok = bool(licenses) and licenses <= ALLOWED
        failed |= not ok
        shown = ", ".join(sorted(licenses)) or "NOT DECLARED"
        print(f"{'ok  ' if ok else 'FAIL'} {group}:{artifact}:{version}  {shown}")
    print(f"{len(packages)} packages, {'FAILED' if failed else 'all allowed'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
