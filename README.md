# nflow

A tiling window manager and keyboard accessibility toolkit for macOS.

nflow pins each app to a virtual space and tiles its windows into columns, all driven from a single TOML file that reloads automatically. No manual window-management hotkeys: switching apps switches spaces, and the layout comes entirely from config.

On top of that, nflow adds five keyboard-driven accessibility tools that let you use macOS without touching the mouse:

- **Hint-mode.** Label every clickable element on screen with a keyboard shortcut, then click, right-click, or copy links without touching the trackpad.
- **Text-select.** Search for text on screen, anchor a selection, then extend it with Vim motions (h/j/k/l, w/e/b, f/t, and more). Press y to copy.
- **Scroll-mode.** Label every scroll area in the focused window, then scroll with Vim keys (j/k, c-d/c-u, gg/G). Works everywhere synthetic mouse-wheel events reach.
- **Menu-search.** A fuzzy command palette over the frontmost app's menu bar. Search every menu item by name, or type its hint code to fire it instantly.
- **Pluck.** A fuzzy finder over all the visible text on screen. Tokenise every text element into words or lines, filter fuzzy, and copy any token to the clipboard.

> Status: early. Built for the author's own daily driving. Expect rough edges.

## Why

macOS has good apps and bad window management. Tools like AeroSpace and yabai already solve most of the tiling problem. nflow exists because the author wanted:

- **App-pinned spaces.** Each app belongs to a known space. Cmd-Tab to Slack, you land on the Slack space. No mental bookkeeping.
- **Declarative, not interactive.** The layout is whatever the config says. You change it by editing the config, never by poking windows with hotkeys.
- **Profiles by screen width.** A laptop layout and an ultrawide layout, picked automatically when the screen changes.
- **One file, hot reloaded.** Edit `~/.config/nflow/config.toml` and the running daemon picks it up.
- **Keyboard reach.** Hint-mode, text-select, scroll-mode, menu-search, and pluck drive the pointer, clipboard, scrolling, menu bar, and on-screen text from the keyboard. The goal is to make macOS usable with only a keyboard -- clicking buttons, copying text, scrolling windows, firing menu commands, and lifting any visible string off the screen without a mouse or trackpad.
- **Tiny.** A few thousand lines of Rust over the Accessibility API. Easy to read, easy to change.

## Requirements

- macOS (tested on Darwin 25, Apple Silicon)
- Rust toolchain (stable)
- Accessibility permission for the binary (System Settings -> Privacy & Security -> Accessibility)
- No conflicting window manager. Carbon hotkey registration silently loses to whoever grabbed the binding first; if AeroSpace, Rectangle, etc. are running, quit them before launching nflow.

## Build and run

```sh
cargo build --release
./target/release/nflow
```

On first launch nflow writes a default config to `~/.config/nflow/config.toml` and exits with an error if Accessibility permission is missing. Grant it, then re-run.

For verbose logs:

```sh
RUST_LOG=info ./target/release/nflow
```

## Accessibility tools

Most of nflow's hotkey actions are keyboard accessibility tools. They work through the macOS Accessibility API to drive the pointer, clipboard, scroll wheel, and on-screen text from the keyboard.

### Hint-mode: click anything by label

Press the hint-mode hotkey (default `alt-cmd-shift-/`) and every clickable element on screen gets a short label overlaid on it. Type the label to click that element. Esc cancels.

Variants:

- Right-click: `alt-cmd-shift-space` labels elements and performs a right-click.
- Copy link: `cmd-shift-l` labels every link on screen. Type a label and nflow copies that link as a rich hyperlink (link text + URL). Pasted into Slack, Notion, or any rich editor it renders as a named link rather than a bare URL. Plain-text targets receive just the title.

These bindings are configurable in `config.toml` and visible in the nflow status bar menu.

### Text-select: Vim-style text selection from the keyboard

`text-select` (`cmd-shift-y`) is a search-and-select workflow for any text macOS exposes through the accessibility tree:

1. Trigger it, type a search query, press Return.
2. Every text element containing the query gets a label. Type a label to anchor the selection on that match.
3. Extend the selection with Vim motions: `h`/`l` (character), `w`/`e`/`b` (word), `0`/`$` (line start/end), `j`/`k` (line), `f<c>`/`t<c>` (forward to/till a character), `;` (repeat the last `f`/`t`).
4. Press `y` to copy. Esc cancels at any point.

This sets the application's real selection, so it is exact where the app cooperates (text fields, text areas, Safari) and does nothing in apps that expose no settable text range (many Electron apps, terminals). The copy is taken from the matched text directly, so `y` yields the right string even when an app declines to render the highlight. Selection stays within a single text element; spanning multiple elements is not supported yet.

### Scroll-mode: scroll any scroll area with Vim keys

`scroll-mode` (`cmd-shift-i`) drives scroll areas from the keyboard, for windows that scroll with no keyboard affordance (Outlook's calendar, a chat backlog, a long settings pane):

1. Trigger it. Every scroll area in the focused window gets a label. If there is only one, it is selected automatically.
2. Type a label to select an area. The cursor warps to its center and a prompt appears.
3. Scroll with Vim keys: `h`/`j`/`k`/`l` (line left/down/up/right), `c-d`/`c-u` (half page down/up), `gg` (top), `G` (bottom). Holding a key repeats it.
4. Esc exits and restores the cursor to where it was.

Scrolling is driven by synthetic mouse-wheel events at the area's center, so it works in native, web, and Electron apps alike. `gg`/`G` set the area's scroll-bar position directly where the app exposes it, falling back to a wheel burst otherwise.

### Menu-search: fuzzy palette over the menu bar

`menu-search` (`alt-cmd-shift-p`) is a command palette for the active app's menu bar. Trigger it and nflow collects every pressable, enabled leaf in the frontmost app's menus (File > Save, View > Enter Full Screen, ...), assigns each a stable hint code, and renders a centered palette:

1. **Search phase** (default): type a query and the list fuzzy-filters live, with matched characters highlighted. Navigate with `ctrl-j`/`ctrl-k` (also arrow keys and `ctrl-n`/`ctrl-p`); `Enter` fires the highlighted item. `Backspace` edits the query.
2. Press `Esc` to drop into **Code phase**: the query clears and the full list is shown with its codes. Type a hint code to fire that item instantly, hint-mode style (`Backspace` edits, `Enter` fires the first match). `Esc` again exits.

Selecting an item performs `AXPress` on its `AXMenuItem` -- the same action macOS posts when you click the entry -- so it works in native, web, and Electron apps that expose a real menu bar. Vim-style navigation (`ctrl-j`/`ctrl-k`) keeps your hands on the home row; the hint codes let you skip the search entirely once you've learned a command's code.

### Pluck: fuzzy-find any text on screen

`pluck` (`alt-cmd-shift-o`) is a fuzzy finder over all the visible text on screen. Trigger it and nflow collects every text element the accessibility tree exposes across all on-screen windows, tokenises it into words (or lines), and renders a centered palette:

1. Type a query and the list fuzzy-filters live, with matched characters highlighted. Navigate with `ctrl-j`/`ctrl-k` (also arrow keys and `ctrl-n`/`ctrl-p`).
2. `Enter` copies the highlighted token to the clipboard and exits. `Tab` toggles a mark on the highlighted row for multi-copy; `Enter` then copies every marked token (words joined with a space, lines with a newline).
3. `ctrl-f` cycles the tokenisation mode between **words** and **lines**, keeping the current query. `Backspace` edits the query. `Esc` exits without copying.

It is the screen-wide analogue of a terminal fuzzy-finder: where that fuzzes a pane backlog, pluck fuzzes the whole display. Words are trimmed of surrounding brackets and quotes and must be at least five characters, which keeps the palette to meaningful tokens. Pluck reuses text-select's text collector and menu-search's fuzzy matcher, so it sees exactly what the accessibility tree sees and ranks the same way.

## Configuration

Config lives at `~/.config/nflow/config.toml`. Example:

```toml
[hotkeys]
hint-mode             = "alt-cmd-shift-/"
hint-mode-right-click = "alt-cmd-shift-space"
hint-mode-copy-link   = "cmd-shift-l"
text-select           = "cmd-shift-y"
scroll-mode           = "cmd-shift-i"
menu-search           = "alt-cmd-shift-p"
pluck                 = "alt-cmd-shift-o"
apply-scene           = "alt-ctrl-{n}"

terminal = "Ghostty"

[gaps]
outer = 8
inner = 6

[ignore]
apps = ["Raycast", "Spotlight", "Alfred", "1Password"]

[profile.laptop]
screen-width-max = 2560

[profile.laptop.spaces.1]
apps = ["Zen", "Ghostty", "Slack"]
weights = { Ghostty = 2.0 }

[profile.ultrawide]
screen-width-min = 2561

[profile.ultrawide.spaces.1]
apps = ["Zen", "Ghostty", "Slack"]
weights = { Ghostty = 2.0 }

[profile.ultrawide.spaces.3]
apps = ["Spotify"]

[profile.ultrawide.scene.coding]
number = 1

[profile.ultrawide.scene.coding.spaces.1]
apps = ["Ghostty", "Zen"]

[profile.ultrawide.scene.meetings]
number = 2

[profile.ultrawide.scene.meetings.spaces.1]
apps = ["Zoom", "Slack"]
```

### Profiles

Each `[profile.<name>]` is selected by current screen width. The first profile whose `screen-width-min` and `screen-width-max` bracket the active screen wins. nflow re-evaluates this when the display configuration changes.

### Spaces and apps

`[profile.<name>.spaces.<n>]` declares space `n` and the apps that live on it. App names come from `kCGWindowOwnerName` (the process name), not the marketing name. Run with `RUST_LOG=info` and watch the logs to see what your apps actually report. For example, "Zen Browser" shows up as `Zen`.

Columns are laid out left-to-right in the order their app appears in `apps`. An optional `weights = { App = N }` map sets per-app column width relative to the others (default 1.0). Example:

```toml
[profile.ultrawide.spaces.1]
apps = ["Zen", "Ghostty", "Slack"]
weights = { Ghostty = 2.0 }
```

That gives Ghostty half the usable width; Zen and Slack split the remainder.

Windows for unmapped apps land on a fresh ad-hoc space.

### Scenes

Scenes are named overlays on a profile that swap the app layout of individual spaces on demand, without editing config. Use them to flip a profile between modes -- for example a coding layout versus a meetings layout -- while keeping the same screen-width profile active.

```toml
[profile.ultrawide.scene.coding]
number = 1

[profile.ultrawide.scene.coding.spaces.1]
apps = ["Ghostty", "Zen"]

[profile.ultrawide.scene.meetings]
number = 2

[profile.ultrawide.scene.meetings.spaces.1]
apps = ["Zoom", "Slack"]
```

Each `[profile.<name>.scene.<key>]` declares a scene with a `number` (1 through 9, used by the hotkey) and an optional `name` shown in the status bar menu (the key is used when `name` is omitted). Its `[...spaces.<n>]` blocks override the matching spaces from the profile; spaces the scene does not mention keep their profile defaults.

Bind `apply-scene = "alt-ctrl-{n}"` to switch scenes by number. Scene `0` (`alt-ctrl-0`) is always the profile default, restoring the plain `[profile.<name>.spaces.<n>]` layout. The active scene is also selectable from the nflow status bar menu. Scenes are scoped to their profile, so switching screens re-evaluates the profile and resets to its default scene.

### Hotkeys

nflow has no manual window-management hotkeys: spaces switch automatically as you change the frontmost app (Cmd-Tab, Dock click, Spotlight), and the per-space layout comes entirely from config. All hotkeys are optional; omit one to leave it unbound.

Modifiers: `alt`/`option`, `shift`, `ctrl`/`control`, `cmd`/`command`. Patterns:

- `{n}` expands to the digits 1 through 9 (one binding per space)

| Action                | Default               | Effect                                               |
| --------------------- | --------------------- | ---------------------------------------------------- |
| hint-mode             | `alt-cmd-shift-/`     | Label every clickable element on screen; type the label to click it (Esc cancels) |
| hint-mode-right-click | `alt-cmd-shift-space` | Same as hint-mode, but performs a right-click        |
| hint-mode-copy-link   | `cmd-shift-l`         | Label every link on screen; type the label to copy it as a rich hyperlink (title + URL) |
| text-select           | `cmd-shift-y`         | Vim-style select-and-copy of visible text (Esc cancels) |
| scroll-mode           | `cmd-shift-i`         | Label every scroll area in the focused window; type the label, then scroll it with Vim keys (Esc cancels) |
| menu-search           | `alt-cmd-shift-p`     | Fuzzy command palette over the frontmost app's menu bar; search or type a hint code to fire a menu item (Esc cancels) |
| pluck                 | `alt-cmd-shift-o`     | Fuzzy-find any visible text on screen; copy the highlighted token (or marked tokens) to the clipboard (Esc cancels) |
| apply-scene           | `alt-ctrl-1` ...      | Switch the active profile to scene N (`alt-ctrl-0` restores the default) |

### Gaps

`gaps.outer` is the margin between the screen edge and the tiled area. `gaps.inner` is the spacing between adjacent windows. Both are in pixels and default to `0`.

### Ignored apps

`[ignore]` `apps = [...]` skips windows whose app name matches. Useful for overlays like Raycast, Spotlight, Alfred, 1Password -- apps you never want tiled.

### Terminal

Top-level `terminal` is the app name nflow opens `$EDITOR` in when you pick "Open config" from the status-bar menu. Defaults to `Ghostty`.

## How it works

- A 500 ms `CFRunLoop` timer drives the daemon. Each tick: drain hotkey commands, poll `CGWindowList` for new and gone windows, detect frontmost-app change, then re-apply the layout.
- Layouts are columns of stacked windows. A configured app inserts at its `apps`-array index regardless of spawn order; an unconfigured app appends.
- Column widths come from `weights` in config (default 1.0 each). Width is distributed proportionally over the usable area.
- Windows on inactive spaces are hidden via `AXHidden` on the app element.
- Frontmost-app changes (via Cmd-Tab, Dock click, Spotlight) auto-switch to that app's space. This is the only navigation model; there are no manual focus/move/switch hotkeys.
- Layouts are re-enforced every tick because some apps (terminal emulators, browsers, Slack) restore their old size after being resized. Each window has a small retry budget per requested frame; once exhausted nflow accepts the drift to avoid fighting hard caps.

`docs/LESSONS.md` collects the macOS quirks that shaped these choices: AXUIElement lifetime, position-then-size-then-position ordering, `_AXUIElementGetWindow`, Carbon's silent hotkey conflicts, and so on. Worth reading if you plan to hack on the code.

## Project layout

```
src/
  main.rs       daemon, run loop, tick
  config.rs     TOML parsing, profile selection
  hotkey.rs     Carbon RegisterEventHotKey + pattern expansion
  ax.rs         Accessibility bridge (move, resize, focus, hide)
  watcher.rs    CGWindowList polling
  screen.rs     screen geometry and change detection
  space.rs      space + window state machine
  tiling.rs     column layout math
  hint/         hint-mode: label and click/right-click/copy-link screen elements
  textselect/   vim-style keyboard text selection
  scroll/       scroll-mode: label and scroll scroll areas with Vim keys
  menusearch/   menu-search: fuzzy command palette over the menu bar
  pluck/        pluck: fuzzy finder over all visible on-screen text
  types.rs      core types and errors
docs/
  LESSONS.md      macOS quirks worth knowing before hacking
  tiling.md       Column layout, weights, spaces, and profiles
  hint-mode.md    Click/right-click/copy-link by keyboard
  text-select.md  Vim-style text selection via the accessibility tree
  scroll-mode.md  Keyboard scroll areas
  menu-search.md  Fuzzy command palette over the menu bar
  pluck.md        Fuzzy finder over all visible on-screen text
```

## Development

```sh
cargo test            # unit and integration tests
cargo clippy
cargo fmt
```

## License

Not yet specified.