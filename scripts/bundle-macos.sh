#!/usr/bin/env bash
# 把 rustduck 组装成 macOS .app。
set -euo pipefail

# 默认值跟随本机架构:写死 aarch64 会让 Intel Mac 上的报错建议指向错误的 target
case "$(uname -m)" in
  arm64) HOST_TARGET="aarch64-apple-darwin" ;;
  *)     HOST_TARGET="x86_64-apple-darwin" ;;
esac
TARGET="${1:-$HOST_TARGET}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ICON="$ROOT/assets/icons/macos/rustduck.icns"

APP="$ROOT/target/bundle/Rustduck.app"
BIN="$ROOT/target/$TARGET/release/rustduck"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
[ -n "$VERSION" ] || { echo "无法从 Cargo.toml 读出 version" >&2; exit 1; }
[ -f "$BIN" ] || { echo "找不到 $BIN,先跑 cargo build --release --target $TARGET" >&2; exit 1; }
[ -f "$ICON" ] || { echo "找不到图标 $ICON" >&2; exit 1; }

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/rustduck"
cp "$ICON" "$APP/Contents/Resources/rustduck.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Rustduck</string>
    <key>CFBundleDisplayName</key>
    <string>Rustduck</string>
    <key>CFBundleIdentifier</key>
    <string>com.rustduck.desktop</string>
    <key>CFBundleExecutable</key>
    <string>rustduck</string>
    <key>CFBundleIconFile</key>
    <string>rustduck.icns</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST
echo "$APP"
