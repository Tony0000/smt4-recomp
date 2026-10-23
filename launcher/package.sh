#!/bin/sh
# Builds the launcher and puts it in the file a release ships for this
# system, in target/release, named the way launchers that install from
# releases, such as Quiver Launcher, tell systems apart:
#
#   smt4-launcher-windows-x64.zip   smt4-launcher.exe
#   smt4-launcher-macos-arm64.zip   SMT4 Launcher.app, signed ad hoc
#   smt4-launcher-linux-x64.tar.gz  smt4-launcher
#
# It holds no game code, so it can be shared. Run from launcher/, in a
# POSIX shell (Git Bash on Windows).
set -eu
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
        app="$out/SMT4 Launcher.app"
        rm -rf "$app"
        mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
        cp "$out/smt4-launcher" "$app/Contents/MacOS/smt4-launcher"
        version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
        cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>SMT4 Launcher</string>
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
        for size in 16 32 128 256 512; do
            sips -z $size $size assets/icon.png --out "$set_dir/icon_${size}x${size}.png" > /dev/null
            sips -z $((size * 2)) $((size * 2)) assets/icon.png --out "$set_dir/icon_${size}x${size}@2x.png" > /dev/null
        done
        iconutil -c icns "$set_dir" -o "$app/Contents/Resources/launcher.icns"
        rm -rf "$(dirname "$set_dir")"
        codesign --force --sign - "$app"
        package="$out/smt4-launcher-macos-$arch.zip"
        rm -f "$package"
        # ditto keeps the bundle whole: its executable bit and signature
        ditto -c -k --keepParent "$app" "$package"
        ;;
    Linux)
        package="$out/smt4-launcher-linux-$arch.tar.gz"
        tar -czf "$package" -C "$out" smt4-launcher
        ;;
    MINGW* | MSYS* | CYGWIN* | Windows_NT)
        package="$out/smt4-launcher-windows-$arch.zip"
        rm -f "$package"
        powershell -NoProfile -Command "Compress-Archive -Force -Path '$out/smt4-launcher.exe' -DestinationPath '$package'"
        ;;
    *)
        echo "no package for $(uname -s)" >&2
        exit 1
        ;;
esac
echo "$package"
