# Hint Mode

Hint mode labels every clickable element on screen with a keyboard shortcut. Typing the label synthetically clicks that element. It is the keyboard equivalent of pointing and clicking with a mouse.

## How it works

A global hotkey triggers hint mode. The system collects every pressable element visible on the active Space, assigns each a letter label, and renders an overlay of badges on screen. Typing a label's letters narrows the match until a unique target is hit, then a synthetic mouse click is posted at the element's center. Escape cancels at any time.

## Element collection

The collector (`src/hint/collect.rs`) works in three passes:

1. **Snapshot on-screen windows.** `CGWindowListCopyWindowInfo` returns every window visible on the active Space with its owner PID and CG window ID.

2. **Walk the accessibility tree.** For each PID, create an `AXUIElement` for the application, read its `AXWindows`, and keep only windows whose CG ID is in the on-screen set. Recurse each window's `AXChildren` depth-first.

3. **Filter for pressable elements.** An element is a target if its `AXActions` contains `AXPress`, or its role is a text input (`AXTextField`, `AXTextArea`, `AXComboBox`).

### Guardrails

- A 250ms wall-clock budget bounds the full collection walk. Elements collected after the deadline are discarded.
- `AXUIElementSetMessagingTimeout` (250ms per app element) prevents one wedged app from freezing the walk.
- Maximum recursion depth of 40 levels and a cap of 500 targets prevent pathological trees from exploding.
- Targets sharing a center point (nested pressables in web views) are deduplicated.

### Coordinate system

Accessibility rects use top-left origin. The overlay view uses bottom-left origin (standard `NSView` drawing). The flip is `view_y = screen_height - (ax_y + height)`.

## Label generation

Labels are generated from a home-row-first alphabet:

```
a s d f g h j k l  (left hand, home row first)
q w e r u i o p    (left hand, upper row)
t y n m b v c x z  (remaining keys)
```

- N <= 26 (alphabet size): single-character labels.
- N > 26: fixed-length multi-character labels with no prefix collisions. The closest, most central targets get the shortest labels.

The match state machine (`src/hint/matcher.rs`) classifies typed prefixes as:
- `Hit`: exactly one label starts with the typed prefix.
- `Pending`: more than one label matches.
- `NoMatch`: zero labels match; the keystroke is ignored.

## Synthetic click

On a unique match, the overlay is torn down and a `CGEvent` mouse down+up pair is posted at the target's center via `CGEventPost(kCGHIDEventTap)`. The cursor is left at the click point.

## Overlay

A single borderless `NSWindow` covers the entire screen:
- Transparent background, ignores mouse events, floats above all windows.
- Joins all Spaces (visible regardless of Space switching).
- `drawRect:` paints a rounded-rect badge with the label text at each target's flipped position.
- On each keystroke, non-matching labels are dimmed or hidden.

## Variants

### Right-click

Same collection and label flow, but the synthetic action is a right-click (mouse button 2).

### Copy link as rich text

Collects only `AXLink` elements. On match, reads two attributes from the element:
- `AXURL` for the link destination.
- `AXTitle`, `AXDescription`, or a descendant `AXStaticText` for the link text.

Writes both flavors to the pasteboard:
- `public.html`: `<a href="ESCAPED_URL">ESCAPED_TITLE</a>`
- `public.utf8-plain-text`: the title

Rich editors (Slack, Notion) render the HTML flavor as a named link. Plain-text targets receive the title. If `AXURL` is absent, only the plain title is written.

## Edge cases

- **Trigger while active:** restarts the session (re-collects targets, rebuilds overlay).
- **Zero targets:** exits immediately with no overlay (optionally logs).
- **Trigger chord modifiers still held:** matches on the base keycode, ignores the trigger's modifier flags.
- **Wedged app:** the per-app timeout keeps the system responsive; the wedged app's subtree is skipped.
