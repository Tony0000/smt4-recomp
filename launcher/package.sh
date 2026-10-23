#!/bin/sh
# Builds the launcher and puts it, with the Zakuro that runs the game, in
# the file a release ships for this system, in target/release, named the
# way launchers that install from releases, such as Quiver Launcher, tell
# systems apart:
#
#   smt4-windows-x64.zip   smt4-launcher.exe, zakuro.exe
#   smt4-macos-arm64.zip   SMT4.app, with zakuro and MoltenVK, signed ad hoc
#   smt4-linux-x64.tar.gz  smt4-launcher, zakuro
#
# ZAKURO_BIN names the zakuro binary to ship, built from the Tony0000/zakuro
# commit Cargo.lock pins (on macOS with --no-default-features, which leaves
# OpenGL out). macOS takes MoltenVK from Homebrew (brew install molten-vk).
#
# SMT4_ICON, on macOS, names a picture for the app's icon in place of the
# launcher's, such as the game's cover, for an app you keep yourself: a
# release must not carry the game's art, so it stays out of the repository.
#
# It holds no game code, so it can be shared. Run in a POSIX shell (Git
# Bash on Windows).
set -eu

if [ -z "${ZAKURO_BIN:-}" ]; then
    echo "ZAKURO_BIN is not set: point it at the zakuro binary to ship, built with" >&2
    echo "cargo build --release -p zakuro from the Tony0000/zakuro commit in Cargo.lock" >&2
    exit 1
fi
if [ ! -f "$ZAKURO_BIN" ]; then
    echo "ZAKURO_BIN is $ZAKURO_BIN, which is not a file" >&2
    exit 1
fi
# the paths may be relative to where the script was called from
zakuro=$(cd "$(dirname "$ZAKURO_BIN")" && pwd)/$(basename "$ZAKURO_BIN")
icon=
if [ -n "${SMT4_ICON:-}" ]; then
    icon=$(cd "$(dirname "$SMT4_ICON")" && pwd)/$(basename "$SMT4_ICON")
fi
cd "$(dirname "$0")"

cargo build --release
out=target/release
arch=$(uname -m)
case "$arch" in
    x86_64 | amd64) arch=x64 ;;
    arm64 | aarch64) arch=arm64 ;;
esac

case "$(uname -s)" in
    Darwin)
        molten=$(brew --prefix molten-vk)
        app="$out/SMT4.app"
        rm -rf "$app"
        mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources"
        cp "$out/smt4-launcher" "$app/Contents/MacOS/smt4-launcher"
        cp "$zakuro" "$app/Contents/MacOS/zakuro"
        # Zakuro looks for it in <exe>/../Frameworks; -L copies the file
        # itself, not Homebrew's link to it
        cp -L "$molten/lib/libMoltenVK.dylib" "$app/Contents/Frameworks/libMoltenVK.dylib"
        chmod u+w "$app/Contents/Frameworks/libMoltenVK.dylib"
        cp "$molten/LICENSE" "$app/Contents/Resources/MoltenVK-LICENSE"
        version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
        cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>SMT4</string>
    <key>CFBundleDisplayName</key>
    <string>Shin Megami Tensei IV for PC</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.smt4-recomp.launcher</string>
    <key>CFBundleExecutable</key>
    <string>smt4-launcher</string>
    <key>CFBundleIconFile</key>
    <string>launcher</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF
        set_dir=$(mktemp -d)/launcher.iconset
        mkdir -p "$set_dir"
        # with the margin macOS leaves round an app's icon
        picture=assets/icon-mac.png
        if [ -n "$icon" ]; then
            # square, the picture whole on black, for the sizes below
            picture="$(dirname "$set_dir")/icon.png"
            sips -s format png "$icon" --out "$picture" > /dev/null
            side=$(sips -g pixelWidth -g pixelHeight "$picture" | awk '/pixel/ {print $2}' | sort -n | tail -n 1)
            sips -p "$side" "$side" --padColor 000000 "$picture" --out "$picture" > /dev/null
        fi
        for size in 16 32 128 256 512; do
            sips -z $size $size "$picture" --out "$set_dir/icon_${size}x${size}.png" > /dev/null
            sips -z $((size * 2)) $((size * 2)) "$picture" --out "$set_dir/icon_${size}x${size}@2x.png" > /dev/null
        done
        iconutil -c icns "$set_dir" -o "$app/Contents/Resources/launcher.icns"
        rm -rf "$(dirname "$set_dir")"
        # from the inside out: a bundle's signature covers what it holds
        codesign --force --sign - "$app/Contents/Frameworks/libMoltenVK.dylib"
        codesign --force --sign - "$app/Contents/MacOS/zakuro"
        codesign --force --sign - "$app/Contents/MacOS/smt4-launcher"
        codesign --force --sign - "$app"
        package="$out/smt4-macos-$arch.zip"
        rm -f "$package"
        # ditto keeps the bundle whole: its executable bits and signatures
        ditto -c -k --keepParent "$app" "$package"
        ;;
    Linux)
        stage="$out/package"
        rm -rf "$stage"
        mkdir -p "$stage"
        cp "$out/smt4-launcher" "$stage/smt4-launcher"
        cp "$zakuro" "$stage/zakuro"
        package="$out/smt4-linux-$arch.tar.gz"
        tar -czf "$package" -C "$stage" smt4-launcher zakuro
        ;;
    MINGW* | MSYS* | CYGWIN* | Windows_NT)
        # copied by the shell first, which reads its own paths in ZAKURO_BIN
        stage="$out/package"
        rm -rf "$stage"
        mkdir -p "$stage"
        cp "$out/smt4-launcher.exe" "$stage/smt4-launcher.exe"
        cp "$zakuro" "$stage/zakuro.exe"
        package="$out/smt4-windows-$arch.zip"
        rm -f "$package"
        powershell -NoProfile -Command "Compress-Archive -Force -Path '$stage/*' -DestinationPath '$package'"
        ;;
    *)
        echo "no package for $(uname -s)" >&2
        exit 1
        ;;
esac
echo "$package"
