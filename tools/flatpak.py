#!/usr/bin/env python3
"""Stet's Flatpak: build it from the commit tagged v<version>, check it, and make the signed
Flatpak repository people install it from (ADR-019). The steps, in order:

  python3 tools/flatpak.py build    flatpak-builder on `git archive HEAD` with the manifest
                                    (packaging/flatpak/io.github.pixdevsapps.Stet.yml); the
                                    result goes into an unsigned repository of its own,
                                    .local/flatpak/release/<version>/repo, and is checked
  python3 tools/flatpak.py site     that commit signed into the published repository,
                                    .local/flatpak/site/repo, with static deltas and the AppStream
                                    data, plus stet.flatpakrepo, the app's .flatpakref and an
                                    index.html beside it (the GitHub release carries the same
                                    .flatpakref, which INSTALL.md's command fetches)
  python3 tools/flatpak.py publish  the site pushed to PixDevsApps/stet-flatpak, whose GitHub
                                    Pages serve it: one new commit with no history (the repository
                                    keeps the last commits itself), force-pushed to main
  python3 tools/flatpak.py keygen   the repository's signing key, once, in .local/flatpak/gpg or
                                    STET_FLATPAK_GPG_HOME (never committed; back it up: a lost
                                    key means everyone adds the repository again)

build needs HEAD to be the commit tagged v<version> and the working tree clean (--untagged builds
HEAD anyway, and --worktree the files git would commit now, tracked or new, for a test). site
serves the repository from STET_FLATPAK_URL, ending in "/" (by default GitHub Pages' address of
PixDevsApps/stet-flatpak), and writes it into STET_FLATPAK_SITE (by default .local/flatpak/site).
publish publishes: an agent runs it only on the owner's word.
"""
import base64
import hashlib
import os
import pathlib
import shutil
import subprocess
import sys
import tomllib

root = pathlib.Path(__file__).resolve().parents[1]
APP_ID = "io.github.pixdevsapps.Stet"
BRANCH = "stable"
MANIFEST = root / "packaging" / "flatpak" / f"{APP_ID}.yml"
WORK = root / ".local" / "flatpak"
GPG_HOME = pathlib.Path(os.environ.get("STET_FLATPAK_GPG_HOME") or WORK / "gpg")
SITE = pathlib.Path(os.environ.get("STET_FLATPAK_SITE") or WORK / "site")
SITE_URL = "https://pixdevsapps.github.io/stet-flatpak/"
KEY_NAME = "Stet Flatpak repository"
HOMEPAGE = "https://github.com/PixDevsApps/Stet"
RUNTIME_REPO = "https://dl.flathub.org/repo/flathub.flatpakrepo"
PAGES_REPO = "https://github.com/PixDevsApps/stet-flatpak.git"
# Where the install command fetches the .flatpakref: the latest GitHub release, which counts its
# downloads (GitHub Pages counts nothing). The site keeps its own copy.
RELEASE_REF = f"{HOMEPAGE}/releases/latest/download/{APP_ID}.flatpakref"


def run(*args: str, cwd: pathlib.Path = root) -> None:
    print("+", " ".join(args), flush=True)
    subprocess.run(args, cwd=cwd, check=True)


def output(*args: str, cwd: pathlib.Path = root) -> str:
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def version() -> str:
    return tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def release_dir(ver: str) -> pathlib.Path:
    return WORK / "release" / ver


def check_head(ver: str) -> None:
    if {"--untagged", "--worktree"} & set(sys.argv[2:]):
        return
    tag = f"v{ver}"
    if output("git", "status", "--porcelain", "--untracked-files=no"):
        raise SystemExit("the working tree has uncommitted changes")
    try:
        tagged = output("git", "rev-parse", f"{tag}^{{commit}}")
    except subprocess.CalledProcessError:
        raise SystemExit(f"no tag {tag}: tag the version commit first (or --untagged)")
    if tagged != output("git", "rev-parse", "HEAD"):
        raise SystemExit(f"HEAD isn't {tag}")


def build(ver: str) -> None:
    check_head(ver)
    out = release_dir(ver)
    if out.exists():
        shutil.rmtree(out)
    src = out / "src"
    src.mkdir(parents=True)
    if "--worktree" in sys.argv[2:]:
        listed = output("git", "ls-files", "-z", "--cached", "--others", "--exclude-standard")
        for name in filter(None, listed.split("\0")):
            source = root / name
            if source.is_file():
                (src / name).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, src / name)
    else:
        archive = subprocess.run(["git", "archive", "--format=tar", "HEAD"], cwd=root,
                                 check=True, capture_output=True).stdout
        subprocess.run(["tar", "-x", "-C", str(src)], input=archive, check=True)
    run(sys.executable, "-I", "tools/flatpak-cargo-sources.py", "--check", cwd=src)
    run(
        "flatpak-builder", "--force-clean",
        f"--state-dir={WORK / 'state'}", f"--default-branch={BRANCH}",
        f"--repo={out / 'repo'}", str(out / "build"), str(src / MANIFEST.relative_to(root)),
    )
    check(ver)


def check(ver: str) -> None:
    build_dir = release_dir(ver) / "build"
    files = build_dir / "files"
    metainfo = files / "share" / "metainfo" / f"{APP_ID}.metainfo.xml"
    desktop = files / "share" / "applications" / f"{APP_ID}.desktop"
    service = build_dir / "export" / "share" / "dbus-1" / "services" / f"{APP_ID}.service"
    binary = files / "bin" / "stet"
    for path in (metainfo, desktop, binary, service):
        if not path.exists():
            raise SystemExit(f"the build has no {path.relative_to(build_dir)}")
    run("appstreamcli", "validate", "--no-net", "--explain", str(metainfo))
    if f'<release version="{ver}"' not in metainfo.read_text():
        raise SystemExit(f"{metainfo.name} has no <release> for {ver}")
    run("desktop-file-validate", str(desktop))
    # Flatpak rewrites this Exec into a `flatpak run` of the app when it installs it.
    if f"Name={APP_ID}\nExec=/app/bin/stet --gapplication-service\n" not in service.read_text():
        raise SystemExit(f"the exported {service.name} doesn't start /app/bin/stet")
    data = binary.read_bytes()
    for marker in (str(pathlib.Path.home()).encode(), b"/run/build/stet"):
        if marker in data:
            raise SystemExit(f"/app/bin/stet holds {marker.decode()!r}")
    reported = output("flatpak-builder", "--version")
    print(f"checked: {metainfo.name}, {desktop.name}, {service.name}, "
          f"/app/bin/stet ({len(data):,} bytes); {reported}")


def gpg(*args: str) -> str:
    return output("gpg", "--homedir", str(GPG_HOME), "--batch", *args)


def key_id() -> str:
    if not GPG_HOME.exists():
        raise SystemExit("no signing key: python3 tools/flatpak.py keygen")
    for line in gpg("--with-colons", "--list-secret-keys").splitlines():
        fields = line.split(":")
        if fields[0] == "fpr":
            return fields[9]
    raise SystemExit(f"no secret key in {GPG_HOME}")


def keygen() -> None:
    if GPG_HOME.exists() and any(GPG_HOME.iterdir()):
        raise SystemExit(f"{GPG_HOME} already holds a key; remove it by hand to start over")
    GPG_HOME.mkdir(parents=True, exist_ok=True)
    GPG_HOME.chmod(0o700)
    gpg("--passphrase", "", "--quick-gen-key", KEY_NAME, "rsa4096", "sign", "never")
    print(f"signing key {key_id()} in {GPG_HOME}: back the folder up somewhere safe")


def public_key() -> str:
    exported = subprocess.check_output(
        ["gpg", "--homedir", str(GPG_HOME), "--batch", "--export", key_id()]
    )
    return base64.b64encode(exported).decode()


def site(ver: str) -> None:
    url = os.environ.get("STET_FLATPAK_URL") or SITE_URL
    if not url.endswith("/"):
        raise SystemExit("STET_FLATPAK_URL must end in /")
    source = release_dir(ver) / "repo"
    if not source.exists():
        raise SystemExit(f"run build first: {source} is missing")
    fingerprint = key_id()
    signing = [f"--gpg-sign={fingerprint}", f"--gpg-homedir={GPG_HOME}"]
    repo = SITE / "repo"
    if not repo.exists():
        repo.mkdir(parents=True)
        run("ostree", "init", "--mode=archive-z2", f"--repo={repo}")
    ref = f"app/{APP_ID}/x86_64/{BRANCH}"
    run("flatpak", "build-commit-from", *signing, f"--src-repo={source}", str(repo), ref)
    run(
        "flatpak", "build-update-repo", *signing, "--generate-static-deltas", "--prune",
        "--prune-depth=3", "--title=Stet", f"--homepage={HOMEPAGE}",
        f"--default-branch={BRANCH}", str(repo),
    )
    key = public_key()
    (SITE / "stet.flatpakrepo").write_text(
        "[Flatpak Repo]\n"
        "Title=Stet\n"
        f"Url={url}repo/\n"
        f"Homepage={HOMEPAGE}\n"
        "Comment=Stet, the text and code editor, from its own repository\n"
        f"DefaultBranch={BRANCH}\n"
        f"GPGKey={key}\n"
    )
    (SITE / f"{APP_ID}.flatpakref").write_text(
        "[Flatpak Ref]\n"
        "Title=Stet\n"
        f"Name={APP_ID}\n"
        f"Branch={BRANCH}\n"
        f"Url={url}repo/\n"
        "SuggestRemoteName=stet\n"
        "IsRuntime=false\n"
        f"RuntimeRepo={RUNTIME_REPO}\n"
        f"GPGKey={key}\n"
    )
    (SITE / "index.html").write_text(index_html(ver))
    (SITE / ".nojekyll").write_text("")
    digest = hashlib.sha256(key.encode()).hexdigest()[:16]
    print(f"site for {ver} in {SITE}; key {fingerprint} (base64 sha256 {digest}); serve it at {url}")


def index_html(ver: str) -> str:
    return f"""<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Stet Flatpak repository</title>
<style>
  body {{ font: 16px/1.5 system-ui, sans-serif; max-width: 42rem; margin: 2rem auto; padding: 0 1rem; }}
  pre {{ background: #8881; padding: .75rem; overflow-x: auto; }}
</style>
<h1>Stet Flatpak repository</h1>
<p>Stet {ver}, the text and code editor, as a Flatpak for Linux distributions other than Omarchy
and Arch. On Omarchy, install the Arch package instead: see
<a href="{HOMEPAGE}/blob/main/docs/INSTALL.md">INSTALL.md</a>.</p>
<p>Install it, and the GNOME runtime it needs from Flathub, with one line:</p>
<pre>flatpak install --user {RELEASE_REF}</pre>
<p>Updates then come with <code>flatpak update</code>, or from GNOME Software or KDE Discover.
The repository on its own: <a href="stet.flatpakrepo">stet.flatpakrepo</a>; the file the line
above fetches is <a href="{APP_ID}.flatpakref">here</a> too.</p>
"""


def publish(ver: str) -> None:
    index = SITE / "index.html"
    if not index.exists() or f"Stet {ver}," not in index.read_text():
        raise SystemExit(f"run site first: {SITE} isn't {ver}'s")
    pages = WORK / "pages"
    if pages.exists():
        shutil.rmtree(pages)
    shutil.copytree(SITE, pages)
    run("git", "init", "--quiet", "--initial-branch=main", cwd=pages)
    run("git", "add", "--all", cwd=pages)
    run("git", "commit", "--quiet", "--message", f"Stet {ver}", cwd=pages)
    run("git", "push", "--force", PAGES_REPO, "main", cwd=pages)


def main() -> int:
    commands = {"build": build, "check": check, "site": site, "publish": publish}
    if len(sys.argv) >= 2 and sys.argv[1] == "keygen":
        keygen()
        return 0
    if len(sys.argv) < 2 or sys.argv[1] not in commands:
        print(__doc__, file=sys.stderr)
        return 2
    commands[sys.argv[1]](version())
    return 0


if __name__ == "__main__":
    sys.exit(main())
