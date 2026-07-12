# Text Select

Text select is a keyboard-driven workflow for searching, selecting, and copying visible text. It works through the macOS Accessibility API, setting the application's real text selection where the app supports it.

## Workflow

The mode has three phases: Search, Pick, and Visual.

### Search phase

1. Trigger the hotkey. A prompt appears; type a search query.
2. Press Return. Every visible text element containing the query (case-insensitive substring match) gets a letter label overlaid at the match location.
3. If nothing matches, the mode exits.

### Pick phase

1. Type a label to select that occurrence. The selection anchor is set at the match start, and the matched substring itself is highlighted.
2. The overlay tears down. A small mode indicator shows that Visual mode is active.

### Visual phase

The anchor is fixed at the match start. Vim motions extend the selection head:

| Key     | Motion                         |
|---------|--------------------------------|
| `h` / `l` | Character left / right       |
| `w`     | Next word start                 |
| `e`     | Next word end                   |
| `b`     | Previous word start             |
| `0` / `^` | Line start / first non-blank |
| `$`     | Line end                        |
| `j` / `k` | Line down / up (multi-line)  |
| `f<c>`  | Forward onto next `<c>`         |
| `t<c>`  | Forward to just before `<c>`   |
| `;`     | Repeat last `f` / `t`           |
| `y`     | Yank (copy) and exit            |
| `Esc`   | Cancel, exit                    |

After each motion, the selection range `{location, length}` is recomputed from `min/max(anchor, head)` and set on the element via `AXSelectedTextRange`. The app renders its native highlight.

On `y`, the text for the computed range is read via `AXStringForRange` and written to the pasteboard as plain text.

## Element targeting

Text elements are identified from the accessibility tree:
- `AXTextField`, `AXTextArea`: readable text from `AXValue`.
- `AXStaticText`: readable text from the element value.

Each occurrence is matched against the search query and its on-screen bounds are read via `AXBoundsForRange(location, length)`, falling back to the element's frame when bounds are unavailable.

### Limitations

- **Per-element only.** A selection stays within a single AX text element (a paragraph, a field, a table cell). Spanning multiple elements is not supported.
- **App support required.** Text selection sets the application's real selection. Apps that do not expose settable text ranges (many Electron apps, terminals, partially Chrome) silently do nothing. The copy still works -- the text is read directly from the element, not from the app's selection.
- **UTF-16 offsets.** Character offsets are tracked in UTF-16 code units to match AX text range indexing, with conversion at the boundary between Rust string indices and AX offsets.

## How it extends hint mode

Text select reuses the same collection, label generation, overlay, and match infrastructure as hint mode. The differences:
- **Collection filter:** keeps only text-bearing elements instead of pressable elements.
- **Element handle retained:** the matched `AXUIElementRef` is kept alive (via `CFRetain`) so the terminal action can read attributes and set the selection range.
- **Terminal action:** instead of a synthetic click, it enters Visual phase and drives the selection with vim motions.
