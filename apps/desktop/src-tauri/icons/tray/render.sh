#!/bin/sh
# Renders the tray glyphs to the raw 64×64 RGBA the app embeds (`tray.rs`).
# Needs ImageMagick. Run from this folder after changing an SVG.
set -eu
for svg in planet-*.svg; do
  magick -background none -density 384 "$svg" -resize 64x64 -depth 8 "rgba:${svg%.svg}.rgba"
done
