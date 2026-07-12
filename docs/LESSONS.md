# Lessons Learned

## macOS Accessibility API

### AXUIElement Lifetime Management
Child AXUIElement references (windows from an app's AXWindows array) are invalidated when the parent CFArray is released. Calling CFRelease on the array frees all child elements. Fix: call CFRetain on any child element you want to keep, or re-fetch it each time you need it. nflow chose the re-fetch approach -- store the app-level AXUIElement (from AXUIElementCreateApplication) and look up the specific window element on demand via _AXUIElementGetWindow to match CGWindowList IDs.

### Window ID Matching
CGWindowList assigns numeric window IDs (kCGWindowNumber). AXUIElement has no public API to get this ID. The private function _AXUIElementGetWindow(element, &mut u32) bridges the two. Without this, there's no way to match a CGWindowList entry to the correct AXUIElement when an app has multiple windows.

### Position/Size Set Order
macOS may adjust window position when size changes (to keep the window on screen). Setting position then size can result in the window ending up at the wrong position. Fix: set position, then size, then position again.

### Apps Resist Resizing
Terminal emulators (Ghostty), browsers (Zen), and other apps may remember and restore their window size after being resized by an external process. Fix: retile the active space on every tick (every 500ms) to continuously enforce the layout.

The retry budget matters. Some apps (Slack especially) silently reject the first AXSize set after launch but accept later attempts once their window is settled. nflow tracks per-window `(last_attempted_frame, attempts_remaining)` and re-applies on each retile cycle until the actual frame matches or attempts run out (default 4). Without this, the first failed resize wins and the window stays at its remembered size forever. Within a single apply_frame call, retrying rapidly back-to-back doesn't help and actively hurts grid-snapping apps like Ghostty, which re-evaluate cell counts on each AXSize and end up smaller than requested.

### AXEnhancedUserInterface Suppresses Resizes
Setting `AXEnhancedUserInterface = true` on the app element suppresses macOS's window-resize animation, which makes tiling look instant. But several apps (Slack, Ghostty, anything Electron-shaped) silently ignore `AXSize` writes while this flag is on. Fix: keep the flag on by default (animation suppression is worth it), but toggle it OFF immediately before every position/size write and back ON immediately after. yabai uses the same trick. Without this, requested widths are quietly clamped to the window's previous size or some internal cap.

### AXMinimized vs Offscreen Hiding
Using AXMinimized to hide windows causes them to disappear from CGWindowListOptionOnScreenOnly, which means the watcher reports them as "gone" and nflow loses track of them. Using negative coordinates (e.g., -32000, -32000) leaves visible slivers because macOS clamps window positions. Fix: move windows to large positive coordinates (99999, 99999) which are off the right/bottom edge of any screen and don't get clamped.

## CGWindowList Filtering

### Multiple Windows Per App
CGWindowListCopyWindowInfo returns ALL windows for each process, including helper windows, floating panels, status items, and background windows. Without filtering, an app like Finder might contribute 7 entries, each becoming a separate column in the tiling layout. Fix: filter by minimum size (100x100) and deduplicate by PID (one window per app).

### Layer Filtering
CGWindowList includes windows at various layers. Layer 0 is normal application windows. System UI elements (menu bar, dock, notification center) are at other layers. Always filter to layer 0 only.

## App Name Matching

### Names Don't Match Marketing Names
macOS reports app names via kCGWindowOwnerName which comes from the process name, not the marketing name. "Zen Browser" reports as just "Zen". Always check the actual reported name with debug logging before writing config rules.

## Carbon Hotkey Registration

### Foreground Process Requirement
Carbon RegisterEventHotKey only delivers events to processes that have a connection to the window server. A plain CLI binary running from a terminal doesn't receive them. Fix: call TransformProcessType to convert the process to a foreground application before registering hotkeys.

### No Error on Conflict
RegisterEventHotKey may succeed even when another app holds the same hotkey. The registration succeeds but events never arrive. If AeroSpace (or another window manager) is running, it grabs the hotkeys first and nflow silently receives nothing.

## Focus-Follows-App (Space Switching)

### Primary Navigation Model
The original design assumed keyboard shortcuts would be the primary way to switch spaces. In practice, users switch apps via Cmd-Tab, clicking the dock, or Spotlight -- not by pressing alt-1 through alt-9. nflow must detect the frontmost app change and auto-switch to that app's space.

### Frontmost App Detection
CGWindowListCopyWindowInfo returns windows sorted by z-order (frontmost first). The first layer-0 window with size >= 100x100 belongs to the frontmost app. Polling this on each tick and comparing PIDs detects app switches.

## CFRunLoop Timer

### Tick Frequency
A 1-second timer is too slow for responsive window management. Windows appear unstyled for a full second before being tiled. 500ms is a reasonable compromise between responsiveness and CPU usage. The retile-every-tick approach means windows get enforced even when apps fight back.
