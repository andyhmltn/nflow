# nflow

A small, opinionated tiling window manager for macOS, written in Rust.

nflow pins each app to a virtual space and tiles its windows into columns, all driven from a single TOML file that reloads automatically. There are no manual window-management hotkeys: switching apps switches spaces, and the layout comes entirely from config. On top of that it adds three keyboard-driven accessibility tools: **hint-mode** (label and click any element on screen), **text-select** (vim-style select-and-copy of visible text), and **scroll-mode** (label and scroll any scroll area with Vim keys).

> Status: early. Built for the author's own daily driving. Expect rough edges.

## Why

macOS has good apps and bad window management. Tools like AeroSpace and yabai already solve most of this; nflow exists because the author wanted:

- **App-pinned spaces.** Each app belongs to a known space. Cmd-Tab to Slack, you land on the Slack space. No mental bookkeeping.
- **Declarative, not interactive.** The layout is whatever the config says. You change it by editing the config, never by poking windows with hotkeys.
- **Profiles by screen width.** A laptop layout and an ultrawide layout, picked automatically when the screen changes.
- **One file, hot reloaded.** Edit `~/.config/nflow/config.toml` and the running daemon picks it up.
- **Keyboard reach.** Hint-mode and text-select drive the pointer and clipboard from the keyboard.
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

## Configuration

Config lives at `~/.config/nflow/config.toml`. Example:

```toml
[hotkeys]
hint-mode             = "alt-cmd-shift-/"
hint-mode-right-click = "alt-cmd-shift-space"
hint-mode-copy-link   = "cmd-shift-l"
text-select           = "cmd-shift-y"
scroll-mode           = "cmd-shift-i"
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

Scenes are named overlays on a profile that swap the app layout of individual spaces on demand, without editing config. Use them to flip a profile between modes, for example a coding layout versus a meetings layout, while keeping the same screen-width profile active.

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

nflow has no manual window-management hotkeys: spaces switch automatically as you change
the frontmost app (Cmd-Tab, Dock click, Spotlight), and the per-space layout comes entirely
from config. All hotkeys are optional; omit one to leave it unbound.

Modifiers: `alt`/`option`, `shift`, `ctrl`/`control`, `cmd`/`command`. Patterns:

- `{n}` expands to the digits 1 through 9 (one binding per space)

| Action               | Default              | Effect                                                |
| -------------------- | -------------------- | ----------------------------------------------------- |
| hint-mode            | `alt-cmd-shift-/`    | Label every clickable element on screen; type the label to click it (Esc cancels) |
| hint-mode-right-click | `alt-cmd-shift-space` | Same as hint-mode, but the label performs a right-click          |
| hint-mode-copy-link  | `cmd-shift-l`        | Label every link on screen; type the label to copy it as a rich hyperlink (title + URL) |
| text-select          | `cmd-shift-y`        | Vim-style select-and-copy of visible text (Esc cancels)          |
| scroll-mode          | `cmd-shift-i`        | Label every scroll area in the focused window; type the label, then scroll it with Vim keys (Esc cancels) |
| apply-scene          | `alt-ctrl-1` ...     | Switch the active profile to scene N (`alt-ctrl-0` restores the default) |

### Copy a link as rich text

`hint-mode-copy-link` labels every link on screen. Type a label and nflow copies that link as a rich hyperlink: the link text plus its URL. Pasted into Slack, Notion, or any rich editor it renders as a named link rather than a bare URL. Handy for grabbing a PR title straight from a GitHub PR list and dropping it into chat. Plain-text targets receive the title.

### Text selection

`text-select` is a Vim-style select-and-copy for any text macOS exposes through the accessibility tree:

1. Trigger it, type a search query, press Return.
2. Every text element containing the query gets a label. Type a label to anchor the selection on that match.
3. Extend the selection with Vim motions: `h`/`l` (char), `w`/`e`/`b` (word), `0`/`$` (line start/end), `j`/`k` (line), `f<c>`/`t<c>` (forward to / till a character), `;` (repeat the last `f`/`t`).
4. Press `y` to copy. `Esc` cancels at any point.

This sets the application's real selection, so it is exact where the app cooperates (text fields, text areas, Safari) and does nothing in apps that expose no settable text range (many Electron apps, terminals). The copy is taken from the matched text directly, so `y` yields the right string even when an app declines to render the highlight. Selection stays within a single text element; spanning multiple elements is not supported yet.

### Scrolling

`scroll-mode` drives scroll areas from the keyboard, for windows that scroll with no keyboard affordance (Outlook's calendar, a chat backlog, a long settings pane):

1. Trigger it. Every scroll area in the focused window gets a label. If there is only one, it is selected automatically.
2. Type a label to select an area. The cursor warps to its center and a prompt appears.
3. Scroll with Vim keys: `h`/`j`/`k`/`l` (line left/down/up/right), `c-d`/`c-u` (half page down/up), `gg` (top), `G` (bottom). Holding a key repeats it.
4. `Esc` exits and restores the cursor to where it was.

Scrolling is driven by synthetic mouse-wheel events at the area's center, so it works in native, web, and Electron apps alike. `gg`/`G` set the area's scroll-bar position directly where the app exposes it, falling back to a wheel burst otherwise.

### Gaps

`gaps.outer` is the margin between the screen edge and the tiled area. `gaps.inner` is the spacing between adjacent windows. Both are in pixels and default to `0`.

### Ignored apps

`[ignore]` `apps = [...]` skips windows whose app name matches. Useful for overlays like Raycast, Spotlight, Alfred, 1Password — apps you never want tiled.

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
  types.rs      core types and errors
docs/
  LESSONS.md   macOS quirks worth knowing before hacking
  plans/       implementation plans for non-trivial features
```

## Development

```sh
cargo test            # unit and integration tests
cargo clippy
cargo fmt
```

## License

Not yet specified.
