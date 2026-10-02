#!/usr/bin/env bash
# Fresh-install test (M9, packaging checks P2, P3 and P5): builds the Arch package from the
# committed tree, then in a clean archlinux:base container installs it with its dependencies,
# checks the installed files (pacman -Qkk), the command, the desktop file and the D-Bus service,
# starts Stet headless under GTK Broadway until its first frame, and uninstalls it, checking that
# a user's session and backups stay.
#
# Usage: bash tools/container-test.sh [--skip-build]
# Needs: docker (the user in the docker group), makepkg, and the archlinux:base image. The
# pacman cache is kept in .local/container/pacman-cache, so later runs download less.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
work=$root/.local/container
mkdir -p "$work/pacman-cache"

if [[ ${1:-} != --skip-build ]]; then
    python3 tools/package-source.py
    rm -rf "$work/build"
    mkdir -p "$work/build"
    cp .local/package/PKGBUILD .local/package/stet-*.tar.gz "$work/build/"
    # --nodeps: Rust comes from rustup on the development machine, not from pacman.
    (cd "$work/build" && makepkg --cleanbuild --force --nodeps --noconfirm)
fi
package=$(ls -t "$work"/build/stet-*.pkg.tar.zst | grep -v debug | head -n 1)
echo "package: $package"

docker run --rm \
    -v "$package:/pkg/$(basename "$package"):ro" \
    -v "$work/pacman-cache:/var/cache/pacman/pkg" \
    archlinux:base bash -euo pipefail -c '
        pacman -Syu --noconfirm --needed dbus desktop-file-utils ttf-dejavu >/dev/null
        # The Docker image leaves out docs, man pages and locales (NoExtract); a desktop does not.
        sed -i "/^NoExtract/d" /etc/pacman.conf
        echo "== install"
        pacman -U --noconfirm /pkg/*.pkg.tar.zst >/dev/null
        pacman -Q stet
        echo "== pacman -Qkk"
        pacman -Qkk stet
        echo "== files"
        test -x /usr/bin/stet
        desktop-file-validate /usr/share/applications/io.github.pixdevsapps.Stet.desktop && echo "desktop file valid"
        grep -q "^Exec=/usr/bin/stet --gapplication-service" /usr/share/dbus-1/services/io.github.pixdevsapps.Stet.service && echo "D-Bus service file present"
        test -f /usr/share/licenses/stet/LICENSE && test -f /usr/share/licenses/stet/dependency-licenses.json && echo "licenses present"
        test -f /usr/share/doc/stet/USER_GUIDE.md && echo "user guide present"
        stet --version
        echo "== headless first frame"
        useradd -m tester
        runuser -u tester -- bash -c "
            export XDG_RUNTIME_DIR=\$(mktemp -d) GDK_BACKEND=broadway BROADWAY_DISPLAY=:5
            gtk4-broadwayd :5 >/dev/null 2>&1 &
            sleep 1
            dbus-run-session -- env STET_EXIT_AFTER_MS=3000 stet ~/note.txt 2>&1 | grep -E \"first-frame|ERROR|CRITICAL\" || true
            mkdir -p ~/.local/state/stet/backup && echo draft > ~/.local/state/stet/backup/kept.txt
            kill %1 || true
        "
        echo "== uninstall keeps user data"
        pacman -R --noconfirm stet >/dev/null
        test ! -e /usr/bin/stet && echo "stet removed"
        test -f /home/tester/.local/state/stet/backup/kept.txt && echo "the user state stayed"
    '
