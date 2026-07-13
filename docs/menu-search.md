# Menu Search

Menu search is a fuzzy command palette over the frontmost application's menu bar. It collects every pressable, enabled leaf menu item the active app exposes, assigns each a stable hint code, and renders a centered palette you can drive entirely from the keyboard. Search by name, or skip the search and type a code to fire an item instantly.

## Workflow

The palette has two phases.

### Search phase (default)

1. Trigger the hotkey. A centered palette appears listing the menu items, each with a hint-code badge on the left and its breadcrumb title (`File > Save`, `View > Enter Full Screen`, ...) on the right.
2. Type a query. The list fuzzy-filters live and the matched characters highlight in each row.
3. Navigate with `ctrl-j` / `ctrl-k` (also arrow keys and `ctrl-n` / `ctrl-p`). `Enter` fires the highlighted item.
4. `Backspace` edits the query. `Esc` drops into Code phase.

Plain letter keys are query input, so list navigation uses the `ctrl-` chord -- the standard fzf/telescope vocabulary -- to avoid stealing letters from the search.

### Code phase

1. From Search, press `Esc`. The query clears and the full list is shown with its stable codes.
2. Type a hint code. The matcher narrows just like hint-mode: a unique prefix fires that item, an ambiguous prefix waits for more input.
3. `Backspace` edits the code. `Enter` fires the first match. `Esc` exits the palette.

This is the "quick fire" path: once you know a command's code you can open the palette, tap `Esc`, and type the code without searching.

Firing an item performs `AXPress` on its `AXMenuItem` -- the same action macOS posts when you click the entry -- so it works wherever the app exposes a real menu bar, including Electron apps.

## Element collection

The collector (`src/menusearch/collect.rs`) reads the frontmost app's `AXMenuBar` and recurses depth-first:

- `AXMenuBar`'s children are the top-level menus (Apple, File, Edit, ...).
- Each `AXMenuItem` with children wraps a single `AXMenu`; that menu's children are leaf commands or further submenus.
- A node is collected as a leaf when it has no children, a non-empty `AXTitle`, an `AXPress` action, and `AXEnabled` is true (greyed-out items are skipped).
- Each leaf retains its `AXMenuItem` element so `AXPress` can be issued later, and records the breadcrumb path of submenu titles for display and matching.

### Guardrails

- A 400 ms wall-clock budget bounds the walk; items collected after the deadline are discarded.
- Depth is capped at 32 levels and the item count at 2000 to keep pathological trees bounded.
- The walk reads `AXChildren` rather than traversing the live menu, so it never opens menus on screen.

## Fuzzy matching

`src/menusearch/fuzzy.rs` is a subsequence matcher with fzf-style scoring. A query matches when its characters appear in order inside an item's display string (`File > Save`), case-insensitively. Scoring rewards:

- Word-boundary matches (start of string, or following a separator / case transition).
- Contiguous runs of matched characters.
- Earlier matches.

Gaps between matched characters incur a small penalty. The matched positions are returned so the overlay can highlight them. Results are sorted best-score-first; ties break by breadcrumb order for stability.

## Hint codes

Codes are generated once per session from the same home-row-first alphabet as hint-mode (`src/hint/labels.rs`):

- N <= 26 items: single-character codes.
- N > 26: fixed-length, prefix-free multi-character codes.

Codes are stable across both phases, so a code you learn in one session works the next time you trigger the palette for the same app. The code-phase matcher reuses hint-mode's prefix classifier (`Hit` / `Pending` / `NoMatch`).

## Overlay

`src/menusearch/overlay.rs` is a single borderless, transparent `NSWindow` covering the screen (ignores mouse events, floats above all windows, joins all Spaces). Its content view is flipped (`isFlipped = true`) so layout uses top-left origin coordinates.

`drawRect:` paints a centered rounded panel:

- A prompt row showing the phase label (`menu` / `code`), a `›` separator, the current input, and a block cursor.
- A separator line.
- Up to 14 result rows, windowed around the selection. Each row shows the code badge (rounded rect) and the breadcrumb title drawn character by character in a monospaced font so matched characters can be brightened individually. The selected row gets a background highlight; disabled items are greyed.

The session computes a `MenuSnapshot` (visible rows + prompt) on every keystroke and hands it to the overlay; the view just renders what it is given.

## Edge cases

- **Zero items:** if the frontmost app exposes no pressable menu items, the mode exits immediately with a "No menu items" toast.
- **No search matches:** the list empties and `Enter` does nothing; `Backspace` widens the query again.
- **Disabled items:** collected only when enabled, so greyed-out commands never appear.
- **AXPress failure:** if the app rejects the press (rare; some apps require an open menu), the palette still exits and a warning is logged.
- **Trigger while active:** the session guards on `is_active`, so a second trigger is ignored rather than re-collecting.
