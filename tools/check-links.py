#!/usr/bin/env python3
"""Checks the relative links in the repository's Markdown files: every linked file exists, and
every #anchor names a heading of that file (GitHub's anchor rules). External links are not
fetched. Usage: python3 -I tools/check-links.py; exits 1 and lists the broken links if any.
"""
import pathlib
import re
import subprocess
import sys
import unicodedata

root = pathlib.Path(__file__).resolve().parents[1]
LINK = re.compile(r"(?<!\!)\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")
FENCE = re.compile(r"^\s*(```|~~~)")


def slug(heading: str) -> str:
    text = re.sub(r"`|\*\*|__|\[([^\]]*)\]\([^)]*\)", lambda m: m.group(1) or "", heading)
    out = []
    for char in text.lower():
        if char in " -":
            out.append("-" if char == " " else char)
        elif char == "_" or unicodedata.category(char)[0] in "LN":
            out.append(char)
    return "".join(out)


def anchors(path: pathlib.Path, cache: dict) -> set:
    if path not in cache:
        found, counts, fenced = set(), {}, False
        for line in path.read_text(encoding="utf-8").splitlines():
            if FENCE.match(line):
                fenced = not fenced
                continue
            match = None if fenced else HEADING.match(line)
            if match:
                base = slug(match.group(2))
                count = counts.get(base, 0)
                counts[base] = count + 1
                found.add(base if count == 0 else f"{base}-{count}")
        cache[path] = found
    return cache[path]


def main() -> int:
    tracked = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", "*.md"],
        cwd=root, check=True, capture_output=True, text=True,
    ).stdout.split("\0")
    cache, broken = {}, []
    for name in filter(None, tracked):
        source = root / name
        if not source.is_file():
            continue
        fenced = False
        for number, line in enumerate(source.read_text(encoding="utf-8").splitlines(), 1):
            if FENCE.match(line):
                fenced = not fenced
            if fenced:
                continue
            for target in LINK.findall(re.sub(r"`[^`]*`", "", line)):
                if re.match(r"[a-z][a-z0-9+.-]*:", target):
                    continue
                file, _, anchor = target.partition("#")
                path = (source.parent / file).resolve() if file else source
                if not path.exists():
                    broken.append(f"{name}:{number}: no file {file}")
                elif anchor and path.suffix == ".md" and anchor not in anchors(path, cache):
                    broken.append(f"{name}:{number}: no heading #{anchor} in {file or name}")
    for line in broken:
        print(line, file=sys.stderr)
    return 1 if broken else 0


if __name__ == "__main__":
    sys.exit(main())
