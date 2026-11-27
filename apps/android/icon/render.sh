#!/bin/sh
# Renders the launcher icon's layers to PNGs at every density, from foreground.svg (the desktop
# logo's planet and ring, in the adaptive icon's safe zone). The monochrome layer, for themed
# icons, is the same shape with the eyes cut out, as the desktop's tray glyph has them.
# Needs ImageMagick. Run from this folder after changing the SVG.
set -eu
res=../app/src/main/res
for density in mdpi:108 hdpi:162 xhdpi:216 xxhdpi:324 xxxhdpi:432; do
  name=${density%%:*}
  size=${density##*:}
  mkdir -p "$res/mipmap-$name"
  magick -background none -density 1200 foreground.svg -resize "${size}x${size}" -strip \
    "png32:$res/mipmap-$name/ic_launcher_foreground.png"
  magick "$res/mipmap-$name/ic_launcher_foreground.png" -fuzz 35% -transparent '#3a2bbd' \
    -channel RGB -evaluate set 0 +channel -strip "png32:$res/mipmap-$name/ic_launcher_monochrome.png"
done
