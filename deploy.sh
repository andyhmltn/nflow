#!/bin/sh
set -e

cargo build --release
cargo install --path .

APP_NAME="nflow"
BUILD_DIR="$(mktemp -d)"
APP="$BUILD_DIR/$APP_NAME.app"
MACOS="$APP/Contents/MacOS"

mkdir -p "$MACOS"
cp target/release/nflow "$MACOS/nflow-bin"

cat > "$MACOS/$APP_NAME" <<'STUB'
#!/bin/sh
if [ "$#" -eq 0 ]; then
    set -- run
fi
exec "$(dirname "$0")/nflow-bin" "$@"
STUB
chmod +x "$MACOS/$APP_NAME" "$MACOS/nflow-bin"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>nflow</string>
    <key>CFBundleIdentifier</key>
    <string>com.nflow.app</string>
    <key>CFBundleName</key>
    <string>nflow</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>LSUIElement</key>
    <true/>
</dict>
</plist>
PLIST

DEST="$HOME/Applications"
mkdir -p "$DEST"
echo "Installing $APP_NAME.app to $DEST"
if [ -d "$DEST/$APP_NAME.app" ]; then
    mkdir -p "$DEST/$APP_NAME.app/Contents/MacOS"
    cp "$APP/Contents/Info.plist" "$DEST/$APP_NAME.app/Contents/Info.plist"
    cp "$MACOS/nflow-bin" "$DEST/$APP_NAME.app/Contents/MacOS/nflow-bin.new"
    mv -f "$DEST/$APP_NAME.app/Contents/MacOS/nflow-bin.new" "$DEST/$APP_NAME.app/Contents/MacOS/nflow-bin"
    cp "$MACOS/$APP_NAME" "$DEST/$APP_NAME.app/Contents/MacOS/$APP_NAME.new"
    mv -f "$DEST/$APP_NAME.app/Contents/MacOS/$APP_NAME.new" "$DEST/$APP_NAME.app/Contents/MacOS/$APP_NAME"
else
    cp -R "$APP" "$DEST/$APP_NAME.app"
fi
rm -rf "$BUILD_DIR"

"$DEST/$APP_NAME.app/Contents/MacOS/nflow" enable-autostart

"$DEST/$APP_NAME.app/Contents/MacOS/nflow" restart

echo "Done. Launch via the 'nflow' command or $DEST/$APP_NAME.app"
