#!/usr/bin/env bash
# Build Rayperclip and package it as a signed macOS .app bundle.
#
# Usage:
#   ./scripts/bundle.sh             release build + dist/Rayperclip.app
#   ./scripts/bundle.sh --install   ... and copy it to ~/Applications
#   ./scripts/bundle.sh --open      ... and launch it
#   ./scripts/bundle.sh --icon      regenerate assets/AppIcon.icns first
#
# Environment:
#   SIGN_IDENTITY   codesign identity (default "-" = ad-hoc signing)
#   INSTALL_DIR     install location for --install (default ~/Applications)

set -euo pipefail

APP_NAME="Rayperclip"
BUNDLE_ID="nl.ray.rayperclip"
BIN_NAME="rayperclip"
MIN_MACOS="12.0"

SIGN_IDENTITY="${SIGN_IDENTITY:--}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/Applications}"

INSTALL=0
OPEN=0
FORCE_ICON=0

usage() {
    sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
}

for arg in "$@"; do
    case "$arg" in
        --install) INSTALL=1 ;;
        --open) OPEN=1 ;;
        --icon) FORCE_ICON=1 ;;
        -h | --help) usage; exit 0 ;;
        *) echo "❌ Unknown option: $arg" >&2; usage >&2; exit 2 ;;
    esac
done

step() { printf '\n\033[1;34m▶ %s\033[0m\n' "$*"; }
fail() { printf '❌ %s\n' "$*" >&2; exit 1; }

# Always work from the project root, wherever the script is called from.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
[[ -f Cargo.toml ]] || fail "Cargo.toml not found in $ROOT"
[[ "$(uname -s)" == "Darwin" ]] || fail "Bundling only works on macOS"
[[ "$(uname -m)" == "arm64" ]] || fail "Only Apple Silicon (arm64) builds are supported"
for tool in cargo codesign plutil sips iconutil; do
    command -v "$tool" >/dev/null || fail "Required tool '$tool' not found"
done

VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n1)"
[[ -n "$VERSION" ]] || fail "Could not read version from Cargo.toml"
BUILD_NUMBER="$(date +%Y%m%d%H%M)"

ASSETS="$ROOT/assets"
ICON_PNG="$ASSETS/AppIcon.png"
ICON_ICNS="$ASSETS/AppIcon.icns"
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"

# --- icon --------------------------------------------------------------------

make_icon() {
    command -v swift >/dev/null || fail "'swift' is needed to render the icon (xcode-select --install)"
    mkdir -p "$ASSETS"
    swift "$ROOT/scripts/render-icon.swift" "$ICON_PNG"

    local tmp iconset
    tmp="$(mktemp -d)"
    iconset="$tmp/AppIcon.iconset"
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
        sips -z "$size" "$size" "$ICON_PNG" --out "$iconset/icon_${size}x${size}.png" >/dev/null
        local double=$((size * 2))
        sips -z "$double" "$double" "$ICON_PNG" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil --convert icns --output "$ICON_ICNS" "$iconset"
    rm -rf "$tmp"
}

if [[ $FORCE_ICON -eq 1 || ! -f "$ICON_ICNS" ]]; then
    step "Rendering app icon 🖇️"
    make_icon
fi

# --- build -------------------------------------------------------------------

step "Building $APP_NAME $VERSION (release, arm64)"
cargo build --release
BIN="$ROOT/target/release/$BIN_NAME"
[[ -x "$BIN" ]] || fail "Build output $BIN not found"

# --- bundle ------------------------------------------------------------------

step "Creating $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/$APP_NAME"
cp "$ICON_ICNS" "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$BUILD_NUMBER</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>$MIN_MACOS</string>
    <key>LSUIElement</key>
    <true/>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.productivity</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSAccessibilityUsageDescription</key>
    <string>$APP_NAME needs Accessibility permission to paste clips automatically.</string>
</dict>
</plist>
EOF
plutil -lint "$APP/Contents/Info.plist" >/dev/null || fail "Info.plist is invalid"

# --- sign --------------------------------------------------------------------

if [[ "$SIGN_IDENTITY" == "-" ]]; then
    step "Signing (ad-hoc)"
else
    step "Signing with '$SIGN_IDENTITY'"
fi
codesign --force --sign "$SIGN_IDENTITY" --identifier "$BUNDLE_ID" --timestamp=none "$APP" 2>&1 \
    | grep -v "replacing existing signature" || true
codesign --verify --strict "$APP" || fail "Signature verification failed"

RESULT="$APP"

# --- install -----------------------------------------------------------------

if [[ $INSTALL -eq 1 ]]; then
    step "Installing to $INSTALL_DIR"
    mkdir -p "$INSTALL_DIR"
    if pgrep -x "$APP_NAME" >/dev/null; then
        echo "Stopping running ${APP_NAME}…"
        pkill -x "$APP_NAME" || true
        for _ in 1 2 3 4 5 6 7 8 9 10; do
            pgrep -x "$APP_NAME" >/dev/null || break
            sleep 0.3
        done
    fi
    rm -rf "${INSTALL_DIR:?}/$APP_NAME.app"
    cp -R "$APP" "$INSTALL_DIR/"
    touch "$INSTALL_DIR/$APP_NAME.app"  # make Finder pick up the new icon
    RESULT="$INSTALL_DIR/$APP_NAME.app"
fi

# --- open --------------------------------------------------------------------

if [[ $OPEN -eq 1 ]]; then
    step "Launching $APP_NAME"
    open "$RESULT"
fi

# --- summary -----------------------------------------------------------------

ARCHS="$(lipo -archs "$RESULT/Contents/MacOS/$APP_NAME")"
SIZE="$(du -sh "$RESULT" | awk '{print $1}')"
cat <<EOF

✅ $APP_NAME $VERSION (build $BUILD_NUMBER) is ready
   📦 $RESULT
   🧬 $ARCHS · $SIZE
   ✍️  signed: $([[ "$SIGN_IDENTITY" == "-" ]] && echo "ad-hoc" || echo "$SIGN_IDENTITY")

🔓 For automatic pasting, allow $APP_NAME under
   System Settings → Privacy & Security → Accessibility.
   (After a new ad-hoc build, remove the old entry and add it again.)
EOF
