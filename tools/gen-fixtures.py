#!/usr/bin/env python3
"""Write deterministic large fixtures for the M0 spikes into target/fixtures/.

Existing files are kept unless --force is given. Output is seeded, so every run
produces byte-identical files.
"""

import argparse
import datetime
import json
import random
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "target" / "fixtures"
SEED = 20260930
MIB = 1024 * 1024
CHUNK_LINES = 20_000

WORDS = (
    "buffer cursor line column search replace token parser session backup theme "
    "window tab view scheme encoding decode render layout index offset length "
    "value result error config palette marker bookmark range anchor snapshot "
    "revision document loader saver worker channel queue batch chunk stream "
    "handle state cache count limit width height scale font style color"
).split()
TYPES = "usize u32 u64 i64 f64 bool String Vec<u8> Option<usize> Result<(),Error>".split()
OPS = "+ - * / % == != < > <= >= && ||".split()
NAMES = "alice bob carol dave erin frank grace heidi ivan judy mallory oscar peggy trent".split()
METHODS = "GET GET GET POST PUT DELETE PATCH".split()
LEVELS = ["TRACE"] * 5 + ["DEBUG"] * 20 + ["INFO"] * 60 + ["WARN"] * 10 + ["ERROR"] * 5


def ident(rng):
    return f"{rng.choice(WORDS)}_{rng.choice(WORDS)}"


def phrase(rng, low, high):
    return " ".join(rng.choices(WORDS, k=rng.randint(low, high)))


def code_line(rng, number):
    indent = "    " * rng.randint(0, 5)
    kind = rng.random()
    if kind < 0.05:
        return ""
    if kind < 0.20:
        return f"{indent}// {phrase(rng, 12, 36)}"
    if kind < 0.40:
        a, b = ident(rng), ident(rng)
        return f"{indent}let {a}: {rng.choice(TYPES)} = {b}.{rng.choice(WORDS)}() {rng.choice(OPS)} {rng.randint(0, 99999)}; // #{number} {phrase(rng, 1, 7)}"
    if kind < 0.60:
        args = ", ".join(ident(rng) for _ in range(rng.randint(2, 6)))
        return f"{indent}{ident(rng)}.{rng.choice(WORDS)}_{rng.choice(WORDS)}({args}); // {phrase(rng, 1, 6)}"
    if kind < 0.72:
        return f"{indent}if {ident(rng)} {rng.choice(OPS)} {rng.randint(0, 4096)} && !{ident(rng)}.is_empty() {{"
    if kind < 0.82:
        return f'{indent}log::info!("{phrase(rng, 5, 14)} {{}}", {ident(rng)}); // line {number}'
    if kind < 0.92:
        return f"{indent}}} else if {ident(rng)}[{rng.randint(0, 255)}] == 0x{rng.randint(0, 0xFFFF):04x} {{"
    return f"{indent}}}"


def write_lines(path, count, make_line):
    with path.open("w", encoding="utf-8", newline="\n") as out:
        for start in range(0, count, CHUNK_LINES):
            end = min(start + CHUNK_LINES, count)
            out.write("\n".join(make_line(n) for n in range(start + 1, end + 1)))
            out.write("\n")


def big_lines(path):
    rng = random.Random(SEED)
    write_lines(path, 1_000_000, lambda n: code_line(rng, n))


def rust_item(rng, n):
    name = "".join(w.capitalize() for w in rng.sample(WORDS, 2)) + str(n)
    fields = "\n".join(f"    pub {ident(rng)}: {rng.choice(TYPES)}," for _ in range(rng.randint(2, 6)))
    body = "\n".join(("        " + code_line(rng, n)).rstrip() for _ in range(rng.randint(4, 14)))
    return (
        f"/// {phrase(rng, 4, 12).capitalize()}.\n"
        f"#[derive(Debug, Clone, Default)]\n"
        f"pub struct {name} {{\n{fields}\n}}\n\n"
        f"impl {name} {{\n"
        f"    pub fn {ident(rng)}(&mut self, {ident(rng)}: {rng.choice(TYPES)}) -> Option<usize> {{\n"
        f"{body}\n"
        f"        None\n"
        f"    }}\n"
        f"}}\n\n"
    )


def medium_rs(path):
    rng = random.Random(SEED + 1)
    size = 0
    n = 0
    with path.open("w", encoding="utf-8", newline="\n") as out:
        out.write("#![allow(dead_code, unused)]\n\nuse std::collections::HashMap;\n\n")
        while size < 10 * MIB:
            n += 1
            item = rust_item(rng, n)
            out.write(item)
            size += len(item.encode("utf-8"))


def json_record(rng, n):
    return {
        "id": n,
        "name": f"{rng.choice(NAMES)} {rng.choice(WORDS)}",
        "active": rng.random() < 0.7,
        "score": round(rng.uniform(0, 1000), 3),
        "tags": rng.sample(WORDS, rng.randint(1, 5)),
        "note": phrase(rng, 3, 12) + (" café 漢字" if n % 97 == 0 else ""),
        "position": {"line": rng.randint(1, 1_000_000), "column": rng.randint(1, 400)},
        "parent": None if n % 5 == 0 else rng.randint(1, max(1, n - 1)),
    }


def single_line_json(path):
    rng = random.Random(SEED + 2)
    parts = []
    size = 0
    n = 0
    while size < 10 * MIB:
        n += 1
        part = json.dumps(json_record(rng, n), ensure_ascii=False, separators=(",", ":"))
        parts.append(part)
        size += len(part.encode("utf-8")) + 1
    text = '{"version":1,"items":[' + ",".join(parts) + "]}"
    json.loads(text)
    path.write_text(text, encoding="utf-8")


def search_log(path):
    rng = random.Random(SEED + 3)
    base = datetime.datetime(2026, 9, 30, tzinfo=datetime.timezone.utc)
    clock = {"ms": 0}

    def line(n):
        clock["ms"] += rng.randint(0, 40)
        stamp = base + datetime.timedelta(milliseconds=clock["ms"])
        level = rng.choice(LEVELS)
        worker = rng.randint(0, 15)
        if level in ("WARN", "ERROR"):
            detail = f'error="{phrase(rng, 1, 4)}" retry={rng.randint(0, 5)}'
        else:
            detail = f'msg="{phrase(rng, 1, 4)}"'
        return (
            f"{stamp:%Y-%m-%dT%H:%M:%S}.{stamp.microsecond // 1000:03d}Z {level:<5} [worker-{worker:02d}] "
            f"{rng.choice(METHODS)} /api/v1/{rng.choice(WORDS)}/{rng.randint(1, 99999)} "
            f"status={rng.choice((200, 200, 200, 201, 204, 304, 400, 404, 500))} "
            f"duration_ms={rng.uniform(0.1, 900):.1f} ip=10.{rng.randint(0, 255)}.{rng.randint(0, 255)}.{rng.randint(1, 254)} "
            f"user={rng.choice(NAMES)} {detail}"
        )

    write_lines(path, 1_000_000, line)


FIXTURES = {
    "big-1m-lines.txt": big_lines,
    "medium-10mb.rs": medium_rs,
    "single-line-10mb.json": single_line_json,
    "search-1m.log": search_log,
}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--force", action="store_true", help="regenerate files that already exist")
    parser.add_argument("names", nargs="*", help=f"only these fixtures: {', '.join(FIXTURES)}")
    args = parser.parse_args()
    unknown = [name for name in args.names if name not in FIXTURES]
    if unknown:
        parser.error(f"unknown fixtures: {', '.join(unknown)}")
    OUT.mkdir(parents=True, exist_ok=True)
    for name, generate in FIXTURES.items():
        if args.names and name not in args.names:
            continue
        path = OUT / name
        if path.exists() and not args.force:
            print(f"kept      {path.relative_to(ROOT)} ({path.stat().st_size / MIB:.1f} MiB)")
            continue
        partial = path.with_name(path.name + ".partial")
        generate(partial)
        partial.replace(path)
        print(f"generated {path.relative_to(ROOT)} ({path.stat().st_size / MIB:.1f} MiB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
