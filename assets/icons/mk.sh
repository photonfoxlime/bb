make_icns () {
  local in="$1"
  local out="$2"
  local set="${out%.icns}.iconset"
  rm -rf "$set"
  mkdir -p "$set"

  sips -z 16 16     "$in" --out "$set/icon_16x16.png"
  sips -z 32 32     "$in" --out "$set/icon_16x16@2x.png"
  sips -z 32 32     "$in" --out "$set/icon_32x32.png"
  sips -z 64 64     "$in" --out "$set/icon_32x32@2x.png"
  sips -z 128 128   "$in" --out "$set/icon_128x128.png"
  sips -z 256 256   "$in" --out "$set/icon_128x128@2x.png"
  sips -z 256 256   "$in" --out "$set/icon_256x256.png"
  sips -z 512 512   "$in" --out "$set/icon_256x256@2x.png"
  sips -z 512 512   "$in" --out "$set/icon_512x512.png"
  sips -z 1024 1024 "$in" --out "$set/icon_512x512@2x.png"

  iconutil -c icns "$set" -o "$out"
  rm -rf "$set"
}

ORIGINAL=blooming-blockery-260303-small.PNG

make_icns $ORIGINAL icon.icns
magick $ORIGINAL -alpha on -background none -define icon:auto-resize=256,128,64,48,32,16 icon.ico
