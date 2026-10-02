#!/usr/bin/env python3
"""Record the licenses of the crates compiled into the stet binary and copy their texts.

Walks the locked dependency graph from the `stet` package, following normal dependencies only
(build scripts and dev-dependencies are not shipped), and writes:
  packaging/dependency-licenses.json   name, version, declared license, repository, texts
  packaging/licenses/<name>-<version>/  the license files each crate ships at its root
Fails when a crate declares no license, or ships no license text and has no recorded text.
"""
import json
import pathlib
import shutil
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
PREFIXES = ("license", "licence", "copying", "notice", "unlicense")


def main() -> int:
    host = subprocess.check_output(["rustc", "-vV"], text=True).split("host: ")[1].splitlines()[0]
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", host],
            cwd=root,
        )
    )
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    start = next(id for id, package in packages.items() if package["name"] == "stet" and package["source"] is None)

    shipped, pending = set(), [start]
    while pending:
        node = nodes[pending.pop()]
        for dep in node["deps"]:
            normal = any(kind["kind"] is None for kind in dep["dep_kinds"])
            if normal and dep["pkg"] not in shipped:
                shipped.add(dep["pkg"])
                pending.append(dep["pkg"])

    record_file = root / "packaging" / "dependency-licenses.json"
    previous = {}
    if record_file.exists():
        previous = {(r["name"], r["version"]): r for r in json.loads(record_file.read_text())}
    out = root / "packaging" / "licenses"
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    records, problems = [], []
    for package in sorted((packages[id] for id in shipped), key=lambda p: (p["name"], p["version"])):
        if package["source"] is None:
            continue
        source = pathlib.Path(package["manifest_path"]).parent
        texts = []
        for file in sorted(source.iterdir()):
            if file.is_file() and file.name.lower().startswith(PREFIXES):
                destination = out / f"{package['name']}-{package['version']}" / file.name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(file, destination)
                texts.append(str(destination.relative_to(root)))
        if not texts:
            texts = [
                path
                for path in previous.get((package["name"], package["version"]), {}).get("license_files", [])
                if (root / path).exists()
            ]
        if not package["license"]:
            problems.append(f"{package['name']} {package['version']}: no license declared")
        if not texts:
            problems.append(f"{package['name']} {package['version']}: no license text")
        records.append(
            {
                "name": package["name"],
                "version": package["version"],
                "license": package["license"],
                "repository": package["repository"],
                "license_files": texts,
            }
        )
    record_file.write_text(json.dumps(records, indent=2) + "\n")
    print(f"{len(records)} crates recorded in {record_file.relative_to(root)}")
    for problem in problems:
        print(f"problem: {problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
