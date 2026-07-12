# Scroll Mode

Scroll mode drives any scroll area from the keyboard. It is for windows that scroll with no keyboard affordance -- Outlook's calendar, a chat backlog, a long settings pane.

## Workflow

### Pick phase

1. Trigger the hotkey. Every scroll area in the frontmost window gets a letter label.
2. If there is exactly one scroll area, it is selected automatically (skip Pick).
3. If there are zero scroll areas in the first walk, `AXManualAccessibility` is set on the app (wakes older Electron accessibility trees) and the walk retries once after 300ms.
4. If still empty, the focused window itself becomes the single target.
5. If there is no focused window at all, a "No scroll areas" toast is shown.

### Scroll phase

1. Type a label to select an area. The cursor warps to its center and a highlight outline appears around the area.
2. Scroll with Vim keys:

| Key     | Action                                   |
|---------|------------------------------------------|
| `j` / `k` | Line down / up (~60px wheel delta)     |
| `h` / `l` | Horizontal left / right                 |
| `c-d`    | Half page down (area height / 2)         |
| `c-u`    | Half page up                             |
| `g` `g`  | Jump to top                              |
| `G`      | Jump to bottom                           |
| `Esc`    | Restore cursor, end session              |

3. Key auto-repeat produces repeated keydown events, so holding a key scrolls continuously.

`gg` uses a `pending_g` flag -- typing `g` starts the two-stroke sequence; any non-`g` key clears it.

## Element targeting

An element is a scroll target when:
- `AXRole == "AXScrollArea"` (native macOS apps), or
- `AXRole == "AXWebArea"` (document scroller in Chromium/Electron/WebKit), or
- It is a large (>= 100x100) `AXGroup`/`AXOutline`/`AXList`/`AXTable` whose `AXDOMClassList` contains a class mentioning `scroll` (catches Slack's `c-scrollbar__hider`, Notion's `notion-scroller`, VS Code's `monaco-scrollable-element`, and similar).

The DOM class list fallback exists because Chromium never maps web-content overflow scrollers to `AXScrollArea` on macOS -- Blink computes `kScrollable` internally but the Cocoa bridge drops it. The size gate (>= 100x100) excludes custom scrollbar thumbs (typically ~6px wide).

## Scroll synthesis

Scrolling is driven by synthetic mouse-wheel events (`CGEventCreateScrollWheelEvent`) with pixel units. A `CGEventSource` with `LocalEventsSuppressionInterval = 0` is created once so wheel events post immediately after a cursor warp (the default ~0.25s suppression would otherwise drop the first scroll).

Sign convention: `j` sends a negative `wheel1` delta (content moves down), `k` sends positive.

### `gg` and `G`

These set the area's `AXVerticalScrollBar` child's `AXValue` to `0.0` (top) or `1.0` (bottom). When that attribute is absent (common in web/Electron views), a burst of large wheel deltas in the right direction is used instead.

## Cursor management

On entering Scroll phase, the current cursor position is saved. The cursor is warped to the area center so wheel events route to the correct window. On Escape, the cursor is restored to its saved position.

## Overlay

During Pick, letter badges are rendered over each scroll area (reusing hint mode's label generation and overlay infrastructure). During Scroll, the area frame is drawn as a highlight outline with a "scroll" prompt badge.
