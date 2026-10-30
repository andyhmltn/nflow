#!/bin/sh
set -e

APP_NAME="nflow"
BUNDLE_ID="com.nflow.app"
SIGN_IDENTITY="nflow-signing"
DEST="$HOME/Applications"
PLIST="$HOME/Library/LaunchAgents/com.nflow.plist"
UID_NUM=$(id -u)

cargo build --release
cargo install --path .

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT
APP="$STAGING/$APP_NAME.app"
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

cat > "$APP/Contents/Info.plist" <<PLIST_EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
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
PLIST_EOF

codesign --force --options runtime --timestamp=none \
    --identifier "$BUNDLE_ID" -s "$SIGN_IDENTITY" \
    "$MACOS/nflow-bin"
codesign --force --options runtime --timestamp=none \
    --identifier "$BUNDLE_ID" -s "$SIGN_IDENTITY" \
    "$APP"
codesign --verify --strict "$APP"

NEW_LEAF=$(codesign -d -r- "$MACOS/nflow-bin" 2>&1 | awk -F'H"' '/certificate leaf/ {print $2}' | tr -d '"')
if [ -d "$DEST/$APP_NAME.app" ]; then
    OLD_LEAF=$(codesign -d -r- "$DEST/$APP_NAME.app/Contents/MacOS/nflow-bin" 2>&1 | awk -F'H"' '/certificate leaf/ {print $2}' | tr -d '"' || true)
    if [ -n "$OLD_LEAF" ] && [ "$OLD_LEAF" != "$NEW_LEAF" ]; then
        echo "ERROR: signing cert leaf changed ($OLD_LEAF -> $NEW_LEAF)" >&2
        echo "       installing would void existing TCC grants. Aborting." >&2
        exit 1
    fi
fi

echo "Stopping existing nflow agent"
launchctl bootout gui/"$UID_NUM"/com.nflow 2>/dev/null || true
waited=0
while launchctl print gui/"$UID_NUM"/com.nflow >/dev/null 2>&1; do
    sleep 0.2
    waited=$((waited + 1))
    if [ "$waited" -gt 50 ]; then
        echo "ERROR: agent did not stop after 10s" >&2
        exit 1
    fi
done
pkill -x nflow-bin 2>/dev/null || true
sleep 0.3

echo "Installing $APP_NAME.app to $DEST"
mkdir -p "$DEST"
STAGING_PARENT="$DEST/.$APP_NAME.app.staging.$$"
cp -R "$APP" "$STAGING_PARENT"
if [ -d "$DEST/$APP_NAME.app" ]; then
    BACKUP="$DEST/.$APP_NAME.app.old.$$"
    mv "$DEST/$APP_NAME.app" "$BACKUP"
    mv "$STAGING_PARENT" "$DEST/$APP_NAME.app"
    rm -rf "$BACKUP"
else
    mv "$STAGING_PARENT" "$DEST/$APP_NAME.app"
fi

codesign --verify --strict "$DEST/$APP_NAME.app"

"$DEST/$APP_NAME.app/Contents/MacOS/nflow-bin" enable-autostart

xattr -c "$PLIST" 2>/dev/null || true

attempt=1
while [ "$attempt" -le 5 ]; do
    if launchctl bootstrap gui/"$UID_NUM" "$PLIST" 2>/dev/null; then
        break
    fi
    if [ "$attempt" = 5 ]; then
        launchctl bootstrap gui/"$UID_NUM" "$PLIST"
        exit 1
    fi
    sleep 0.5
    attempt=$((attempt + 1))
done
launchctl kickstart -k gui/"$UID_NUM"/com.nflow

echo "Done. Launch via the 'nflow' command or $DEST/$APP_NAME.app"
