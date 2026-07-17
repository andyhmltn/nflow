# Pluck

Pluck is a fuzzy finder over all the visible text on screen. Trigger the hotkey and nflow collects every text element the accessibility tree exposes across all on-screen windows, tokenises it into words (or lines), and renders a centered palette. Type to fuzzy-filter, pick a token, and copy it to the clipboard without touching the mouse.

It is the screen-wide analogue of `pluck` from the author's terminal toolchain: where that tool fuzzes the tmux pane backlog, this one fuzzes the whole display.

## Workflow

1. Trigger the hotkey (default `alt-cmd-shift-o`). nflow walks every on-screen window's accessibility tree, reads the `AXValue` of every text element (`AXStaticText`, `AXTextField`, `AXTextArea`, `AXComboBox`, `AXSearchField`) and every link (`AXLink`), reconstructs visual lines from the pieces, tokenises the result, and shows a centered palette listing the candidates.
2. Type a query. The list fuzzy-filters live and the matched characters highlight in each row.
3. Navigate with `ctrl-j` / `ctrl-k` (also arrow keys and `ctrl-n` / `ctrl-p`).
4. `Enter` copies the highlighted token to the pasteboard and exits. `Tab` toggles a mark on the highlighted row for multi-copy; `Enter` then copies every marked token (or just the highlighted one if nothing is marked).
5. `ctrl-f` cycles the tokenisation mode between **words** and **lines**, keeping the current query. Words are trimmed of surrounding brackets, quotes, and trailing punctuation; lines are reconstructed visual lines (see below).
6. `ctrl-m` copies the selection rendered as **markdown**: each link in a reconstructed line becomes `[text](url)`. Falls back to the plain text for candidates that contain no links (e.g. words, or lines with no links). Rows that have a markdown form show an `md` marker at the right edge.
7. `Backspace` edits the query. `Esc` exits without copying.

Plain letter keys are query input, so list navigation uses the `ctrl-` chord -- the same fzf/telescope vocabulary menu-search uses -- to avoid stealing letters from the search.

## Visual line reconstruction

On a web page a single sentence is usually split across several accessibility elements -- a run of static text, then a link, then more static text. For example the Wikipedia hamburger hatnote:

> This article is about the dish. For the meat served as part of such a dish, see Patty. For other uses, see Hamburger (disambiguation).

Here "Patty" and "Hamburger (disambiguation)" are links, each its own `AXLink` element, and the surrounding prose is `AXStaticText`. Reading each element's `AXValue` in isolation yields fragments and loses the link URLs.

Pluck records each text and link piece with its screen frame and window id, then groups pieces that share a window and overlap vertically (by at least half the smaller piece's height) into a single reconstructed line, breaking on a new visual row or a large horizontal gap. Pieces are joined left-to-right with a space inserted only when they don't already touch and the next piece doesn't start with punctuation, so `Patty` + `.` reads as `Patty.`. The reconstructed line carries both its plain text and, separately, a markdown rendering where link pieces become `[text](url)` -- which is what `ctrl-m` copies.

Line reconstruction is per-window, so text from adjacent tiled nflow windows never merges into one line.

## Tokenisation

`src/pluck/collect.rs` turns the reconstructed lines into candidates:

- **Words:** `split_whitespace` on the line's plain text, then trim surrounding `()[]{}<>'"\`,;` and trailing `.` or `:`. Tokens shorter than five characters are dropped (the same threshold as the terminal `pluck`, which keeps the palette to meaningful words rather than every `a`, `the`, `of`).
- **Lines:** whole reconstructed visual lines, trimmed. Lines shorter than five characters are dropped.

Candidates are deduplicated by exact string, preserving first-seen order so the palette is stable across keystrokes. Each line candidate also carries its markdown rendering (when it contains links); word candidates never do. The mode is shown in the prompt (`[words]` / `[lines]`); `ctrl-f` re-tokenises against the already-collected pieces without re-walking the accessibility tree, so the switch is instant.

## Element collection

Pluck uses hint-mode's `collect_text_and_link_targets` (`src/hint/collect.rs`), a single walk that retains both text-role elements and `AXLink` elements. It snapshots on-screen windows via `CGWindowListCopyWindowInfo`, walks each application's `AXWindows`, and recurses `AXChildren` depth-first. Each retained `AxElement`'s `AXValue` (and, for links, `AXURL` and the link label via `AXTitle`/`AXDescription`/descendant text) is read once at collection time and cached, so cycling modes with `ctrl-f` does not re-query the accessibility tree.

The same guardrails as the other collectors apply: a wall-clock budget bounds the walk, depth and target counts are capped, and elements outside the screen rect are filtered out.

## Fuzzy matching

Pluck reuses menu-search's subsequence matcher (`src/menusearch/fuzzy.rs`) verbatim. A query matches when its characters appear in order inside a candidate, case-insensitively, with fzf-style scoring that rewards word-boundary matches, contiguous runs, and early matches. The matched positions are returned so the overlay can highlight them. Results are sorted best-score-first; ties break by original order for stability.

## Overlay

`src/pluck/overlay.rs` is a single borderless, transparent `NSWindow` covering the screen (ignores mouse events, floats above all windows, joins all Spaces). Its content view is flipped so layout uses top-left origin coordinates.

`drawRect:` paints a centered rounded panel:

- A prompt row showing `pluck`, a `›` separator, the current query, and a block cursor. The mode indicator (`[words]` / `[lines]`) sits at the right edge of the prompt row.
- A separator line.
- Up to 14 result rows, windowed around the selection. Each row shows the token drawn character by character in a monospaced font so matched characters can be brightened individually. The selected row gets a background highlight; marked rows show a leading `●`. A footer line records the binding cheatsheet (`enter=copy  tab=mark  ctrl-f=mode  esc=cancel`).

The session computes a `PluckSnapshot` (visible rows + prompt + mode) on every keystroke and hands it to the overlay; the view just renders what it is given.

## Copying

Selecting a token copies it to the system pasteboard via `NSPasteboard`'s `generalPasteboard`, the same mechanism text-select uses for `y`. `Enter` copies the plain text; `ctrl-m` copies the markdown rendering (links as `[text](url)`), falling back to plain text for candidates with no links. When multiple rows are marked, words are joined with a single space and lines with a newline (markdown copy always joins with a newline), mirroring the terminal `pluck`'s join rules. A toast confirms success ("Copied" / "Copied markdown").

## Edge cases

- **No text on screen:** if no text or link elements are found (or all are empty), the mode exits immediately with a "No text on screen" toast.
- **No search matches:** the list empties and `Enter` does nothing; `Backspace` widens the query again.
- **Trigger while active:** the session guards on `is_active`, so a second trigger is ignored rather than re-collecting.
- **App-private text:** apps that expose no `AXValue` on their text elements (some Electron custom controls, DRM-protected video, image-only text) contribute nothing; pluck can only see what the accessibility tree sees.
- **Links without a URL:** a link whose `AXURL` is missing still contributes its visible text to reconstructed lines; `ctrl-m` just copies it as plain text.
