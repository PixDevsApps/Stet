#!/usr/bin/env bash
# Renders the app icon's PNG sizes from the SVG master with librsvg (rsvg-convert).
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
mkdir -p assets/icons
for size in 16 24 32 48 64 128 256; do
    rsvg-convert -w "$size" -h "$size" assets/brand/io.github.pixdevsapps.Stet.svg \
        -o "assets/icons/io.github.pixdevsapps.Stet-$size.png"
done
