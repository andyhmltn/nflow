# Code review of `src/`

This is an annotated walkthrough of the AI-generated Rust in `src/`, focused on
what's worth absorbing as a Rust learner and what was changed during the review.
Read this alongside the diffs of the commits listed at the bottom.

The goal is not to defend or critique every line. The goal is to point at the
patterns worth internalizing and the ones worth unlearning.

## How to read this document

Each module section has the same shape:

- **What it does** — one paragraph on the module's responsibility.
- **Patterns worth absorbing** — Rust idioms used here that you should
  recognise and reach for yourself.
- **Changes applied** — what I changed and why, so you can read the diff with
  intent rather than as a wall of moves.
- **Things considered, not changed** — places I thought about touching and
  decided to leave, with reasoning.

There is also a top-level section, *General Rust patterns worth absorbing*,
that covers idioms that recur across modules.

---

## General Rust patterns worth absorbing

These show up across multiple files. If you take three things from this
document, take these.

### `&Path` over `&PathBuf`

`PathBuf` is the *owned* path type — it's like `String`. The borrowed view of
`PathBuf` is `&Path` — like `&str` is to `String`. Functions that only need
to *read* a path should take `&Path`:

```rust
fn ensure_config(dir: &Path) -> PathBuf { ... }   // good
fn ensure_config(dir: &PathBuf) -> PathBuf { ... } // works but tells callers
                                                   // they need a PathBuf
```

The `&PathBuf` form is a common AI tell. It works because Rust will deref a
`&PathBuf` to `&Path` automatically — but it forces every caller to materialise
a `PathBuf` even when they have a `&Path` already. Same logic applies to
`&str` vs `&String` and `&[T]` vs `&Vec<T>`: prefer the borrowed slice/path
form in function signatures.

### `let-else` over `match { Some(x) => x, None => return }`

When you want to bind a value or bail out, `let-else` reads better than the
`match` form:

```rust
let Some(focused) = self.focused_window else { return };
let Some(&space_id) = self.window_to_space.get(&focused) else { return };
```

vs

```rust
let focused = match self.focused_window {
    Some(w) => w,
    None => return,
};
```

Both compile to the same code. `let-else` is much easier to read once you have
two or three of them in a row. The codebase already used this pattern in
`watcher.rs` but had drifted back to `match` in `space.rs` — that has been made
consistent.

### `derive(Default)` instead of hand-written `impl Default`

If a struct's default value is just every field's default, `#[derive(Default)]`
produces an identical impl. The hand-written form is a smell — it will silently
go stale if you add a field.

```rust
#[derive(Default)]                              // good
pub struct GapsConfig { pub outer: f64, pub inner: f64 }

impl Default for GapsConfig {                   // unnecessary
    fn default() -> Self { Self { outer: 0.0, inner: 0.0 } }
}
```

### Dead enum variants are *not* future-proofing

`NflowError` had `WindowNotFound(WindowId)` and `SpaceNotFound(SpaceId)`
variants that were never constructed anywhere. AI tends to add "complete-looking"
enum coverage. In practice, dead variants:

- waste readers' time as they look for where the variant is produced
- can never be removed without breaking match exhaustiveness in callers
- should be added the moment a use site appears, not before

Same logic applies to functions: `space_assignments` was defined and exported
but never called anywhere. Removed.

### Imports go at the top

A `use` statement halfway down a file (as in old `hotkey.rs:233`) is a
red flag. Bring all `use`s to the top so the dependency surface is visible at
a glance. The exception is inside an `fn` body where a `use` statement
actually narrows scope (e.g. a single trait import for a single call) —
but even that is rarely worth doing.

### Avoid duplicating logic between two near-identical functions

`watcher.rs` had `discover_windows` and `get_frontmost_window` with about 60
lines of identical CFDictionary destructuring. When you see two functions
diverging by their tail behaviour but sharing their head, factor the head out.
Done in this review.

### Keep FFI surface small and explicit

The `extern "C"` blocks in `hotkey.rs`, `ax.rs`, and `screen.rs` are
*correct* but worth understanding rather than copying:

- Every `extern fn` declaration is a contract you're asserting against the
  C ABI of a library. Rust will not check it. If you get a signature wrong
  it's UB.
- `unsafe extern "C" fn callback(...)` is for *Rust* functions you're handing
  to C. The `unsafe` here is the function-marker form, meaning "this fn can
  only be called by C, and Rust must not call it directly." Rust 2024 will
  require you to mark the entire `extern` block `unsafe extern "C" { ... }`;
  this codebase is on edition 2021 so the older form still applies.
- Magic numbers in C constants (`K_CG_EVENT_KEY_DOWN: u64 = 1 << 10`) should
  always be named — never sprinkled through call sites. The old code had
  `event_type == 10` as a literal in `event_tap_callback`; renamed.

---

## `types.rs`

**What it does.** Shared data types — window/space ID aliases, `Rect`,
`Column`, `LayoutTree` (the tiling tree), `Command` enum, `PendingSplit`
enum, `NflowError`, and the `Result<T>` alias.

### Patterns worth absorbing

- **Type aliases for domain meaning.** `pub type WindowId = u32;` lets the
  function signature `fn focus(&mut self, w: WindowId)` document intent. If
  you ever need to swap to `u64`, one line changes.
- **Crate-local `Result` alias.** `pub type Result<T> = std::result::Result<T,
  NflowError>;` removes the `NflowError` repetition from function
  signatures and is the standard Rust pattern. Most large crates do this.
- **`impl Display` + `impl std::error::Error` for the error enum.** Required
  to use `NflowError` with `?` against any other `Box<dyn Error>` boundary.

### Changes applied

- Removed `NflowError::WindowNotFound` and `NflowError::SpaceNotFound`
  — never constructed. If a future caller needs them, add them then.

### Things considered, not changed

- `Column` and `LayoutTree` are tiny and pure. Could move to `tiling.rs`,
  but `space.rs` and `tiling.rs` both consume them, so they belong in
  `types.rs`.
- `Command` is a flat enum. Could split into nested enums (focus/move/space
  groups), but flat is readable and the matches are already exhaustive.

---

## `screen.rs`

**What it does.** Reports the main screen's usable rect (display bounds minus
the menu bar) and registers a callback that flips an atomic flag when the
display configuration changes.

### Changes applied

The biggest cleanup target in the codebase. The original `get_menu_bar_height`
looked like this:

```rust
pub fn get_menu_bar_height() -> f64 {
    let display = CGDisplay::main();
    let full = display.bounds();
    let full_height = full.size.height;
    let pixels_height = display.pixels_high() as f64;
    let scale = if pixels_height > 0.0 { pixels_height / full_height } else { 1.0 };
    let menu_bar_logical = if scale >= 2.0 { 25.0 } else { 25.0 };  // both branches identical!
    let actual = unsafe {
        let display_id = core_graphics::display::CGMainDisplayID();
        let bounds = CGDisplayBounds(display_id);
        bounds.size.height
    };
    let usable_bottom = full.origin.y + full.size.height;
    let _ = actual;
    let _ = usable_bottom;
    menu_bar_logical
}
```

This is a function that does a lot of work to return the constant `25.0`. The
`if scale >= 2.0 { 25.0 } else { 25.0 }` is the giveaway — both branches
return the same value. The `let _ = actual;` and `let _ = usable_bottom;`
suppress unused-variable warnings on values that are computed and thrown away.
This is what AI does when it doesn't know what to compute and just writes
plausible-looking math.

Replaced with a named constant. If macOS ever changes the menu bar height we
update one line. If Apple ever ships a way to query it correctly, we replace
the constant with a query.

Also removed the redundant `extern "C" { fn CGDisplayBounds(...) }` declaration —
`core_graphics::CGDisplay::bounds()` already wraps this safely and is used
elsewhere in the same module.

### Things considered, not changed

- The atomic flag pattern (`SCREEN_CHANGED: AtomicBool` set from a C
  callback, swapped to `false` from the run loop tick) is the right shape for
  C-callback-to-main-thread state. Left alone.

---

## `config.rs`

**What it does.** Defines the on-disk TOML schema (`Config`, `Profile`,
`HotkeyConfig`, etc.) and the helpers that pick a profile by screen width
and build the `app_name -> space_id` lookup.

### Patterns worth absorbing

- `#[serde(rename = "kebab-case")]` on each field — clean way to keep
  Rust's `snake_case` field names while accepting kebab-case TOML.
- `#[serde(default)]` on optional sections (`launcher`, `gaps`, `ignore`) —
  user can omit them.
- `BTreeMap` instead of `HashMap` for config-derived data. BTreeMap iteration
  is sorted, so config behaviour is deterministic. (See "things considered.")

### Changes applied

- Removed the hand-written `impl Default for GapsConfig` — `derive(Default)`
  produces the same impl.
- Deleted `space_assignments` — it was exported and tested but never called
  by any production code. The test for it was also deleted.

### Things considered, not changed

- `select_profile` returns the first profile in *BTreeMap iteration order*
  whose `screen-width-min`/`screen-width-max` constraints match. Because
  `BTreeMap` iterates by key, this is alphabetical by profile name. That's
  fragile if a user adds `[profile.aaa]` with no constraints — it silently
  wins over `[profile.laptop]`. A future improvement would be to sort by
  *specificity* (presence of min/max). Not changed because (a) the current
  configs all set explicit ranges, and (b) it's a real behavioural change
  that deserves its own commit and design conversation. Flagging here.

---

## `tiling.rs`

**What it does.** Pure function: given a screen rect, a `LayoutTree`, and gap
sizes, produces the per-window frames. No side effects, no FFI. The most
testable file in the codebase.

### Patterns worth absorbing

- **A pure function with deterministic tests.** `compute_layout_with_gaps`
  is the cleanest design pattern in the codebase: input -> output, no
  globals. Notice how dense the tests are — that's only possible because
  the function is pure.
- The tests assert *invariants* (no overlaps, total area equals screen
  area) not just specific frames. That's a strong test style:

  ```rust
  let total_area: f64 = result.iter().map(|(_, r)| r.width * r.height).sum();
  assert!((total_area - screen.width * screen.height).abs() < 1e-6);
  ```

### Changes applied

- Renamed `total_inner_h` → `total_horizontal_gap` and `total_inner_v` →
  `total_vertical_gap`. The old `_h` / `_v` suffixes read as "height" /
  "vertical" which is the opposite of what they meant (they're horizontal/
  vertical *gaps* between columns/windows respectively).
- Used `usize` arithmetic where possible and converted to `f64` once at the
  point of dividing pixel space. Avoids a sprinkling of `as f64` and the
  awkward `if win_count > 1.0 { ... }` comparing a count to a float.

### Things considered, not changed

- The `compute_layout` and `compute_layout_with_gaps` split (the former
  delegates to the latter with `0.0, 0.0`). Could be one function with default
  args, but Rust doesn't have default args. Two functions is the standard
  Rust pattern for this. Left alone.

---

## `watcher.rs`

**What it does.** Wraps `CGWindowListCopyWindowInfo` to enumerate on-screen
windows and detect new/gone PIDs across polls. Holds the ignore list filter.

### Patterns worth absorbing

- `let-else` for early return on null:
  ```rust
  let Some(array) = copy_window_info(options, kCGNullWindowID) else {
      return Vec::new();
  };
  ```
- Building filter sets from configuration:
  `ignored_apps: HashSet<String>` from a `Vec<String>` — `HashSet` for O(1)
  contains, built once.

### Changes applied

The biggest refactor in this review. `discover_windows` and
`get_frontmost_window` had about 60 lines of identical CFDictionary
destructuring with the same CFString keys, the same downcast chain, the same
bounds-extraction. Factored it into:

```rust
fn extract_window(dict: &CFDictionary<CFString, CFType>, keys: &WindowKeys)
    -> Option<DiscoveredWindow>
```

`WindowKeys` holds the `CFString::from_static_string(...)` values once, instead
of building them inside every loop iteration of every call. Both `discover_windows`
and `get_frontmost_window` now share this helper.

### Things considered, not changed

- `WindowWatcher::new(Vec<String>)` plus `set_ignored_apps(Vec<String>)`
  could merge into a single `with_config` builder, but the call sites are
  fine as-is and the explicitness is helpful.
- `pid_is_running` uses `libc::kill(pid, 0)`. This is the canonical Unix way
  to ask "is a PID alive" without sending a real signal. It's correct.

---

## `hotkey.rs`

**What it does.** Parses hotkey strings (`"alt-shift-h"`), expands patterns
(`"alt-{n}"` → `["alt-1", ..., "alt-9"]`), maps the string config to a list
of `HotkeyBinding` values, and installs a `CGEventTap` that fires registered
commands on key-down events.

### Patterns worth absorbing

- **Match table for keycodes.** `key_to_keycode` is a flat `match key { "a" =>
  Some(0x00), ... }`. This is more readable, more compile-time-checked, and
  faster than a runtime `HashMap`. Don't reach for collections when a `match`
  works.
- **`BIT_MASK` constants from C** (e.g. `OPTION_KEY: u32 = 0x0800`) declared
  as Rust constants near top of file. Trivial but matters: never let raw
  `0x0800` appear in code.
- **Trait object behind a `Mutex<Option<...>>` for callbacks crossing FFI.**
  `static COMMAND_CALLBACK: Mutex<Option<CommandCallback>>` is how you pass
  Rust closures to C-callback contexts that don't accept user data — the
  callback fires, locks, unwraps, calls. Verbose but correct.

### Changes applied

- **Moved the floating `use std::sync::Mutex; use std::collections::HashMap;`
  from line 233 to the top of the file.** Imports belong at the top.
- **Factored `build_bindings`.** The original was 100+ lines of:
  ```rust
  let hotkey = parse_hotkey(&config.focus_left)?;
  bindings.push(HotkeyBinding { hotkey, command: Command::FocusLeft });
  let hotkey = parse_hotkey(&config.focus_down)?;
  bindings.push(HotkeyBinding { hotkey, command: Command::FocusDown });
  // ...10 more identical blocks
  ```
  Replaced with a small helper `push_simple` and a table of
  `(&str, Command)` pairs. The function is now ~30 lines and the binding
  list reads as data.
- Named the `event_type == 10` magic number as
  `const KEY_DOWN_EVENT_TYPE: u32 = 10;`.

### Things considered, not changed

- `BINDING_MAP: Mutex<Option<HashMap<...>>>` could be `OnceLock<Mutex<...>>`
  to skip the outer `Option`, but that's a perf-irrelevant readability
  tradeoff and the current shape works.
- `register_hotkeys` re-installs the tap on every config reload. This leaks
  the previous `CFMachPortRef`. Real fix needs `CFMachPortInvalidate` and
  removing the run-loop source. Logged as a known issue but out of scope —
  it's a real bug, not a style cleanup.

---

## `ax.rs`

**What it does.** The C-FFI bridge to the macOS Accessibility API.
Implements `WindowBridge` for `MacOSBridge`: register an app element by PID,
apply a frame (set position/size), hide a window (move offscreen via
`AXHidden`), focus a window (`AXRaise` + activate process).

### Patterns worth absorbing

- **`extern "C"` block declares the C symbols you'll call.** Each declaration
  is a contract you're asserting matches the dylib at link time. Rust does
  not verify this — if you misdeclare `AXValueGetValue`'s signature, you get
  UB.
- **`unsafe { ... }` blocks scoped narrowly around the C call.** Never wrap
  a whole function in `unsafe`. The pattern in this file is good: each FFI
  call sits inside its own `unsafe` block immediately after Rust-side setup.
- **CFRelease pattern.** Every `Copy*` and `*Create*` C function returns a
  reference that Rust must release with `CFRelease`. The code consistently
  pairs them, often via early-return `return Some(...)` after `CFRelease`.
  This pairing is the entire point of `Drop` in safe Rust — but for FFI
  types you have to do it by hand.
- **`pub(crate)` visibility.** `frames_match` is `pub(crate)`, meaning
  it's visible to the rest of the crate but not exported as part of the
  public library API. Reach for this rather than `pub` whenever you only
  need crate-level access.

### Changes applied

No changes in `ax.rs`. The FFI surface is correct, narrow, and consistent.

### Things considered, not changed

- **Caching CFStrings.** Every `apply_frame`/`focus`/`hide` call rebuilds the
  same CFStrings (`"AXPosition"`, `"AXSize"`, `"AXHidden"`, etc.) — every
  tick, for every window. This is the main perf cost of the bridge.
  Caching them is the right move *eventually*, but it requires either
  manual `CFRelease` in `Drop` for `MacOSBridge` or a deliberate decision
  to leak them for the lifetime of the program. Both are reasonable; both
  add code that has to be right or the program crashes. Left as a flagged
  improvement, not done in this review.
- `ax_get_position` and `ax_get_size` could be merged into a generic
  `ax_get_value<T>(window, attr, ax_type, default)` helper. They're already
  short and the FFI type-juggling makes the generic version uglier than the
  duplication. Left alone.
- `MacOSBridge` doesn't `impl Default`. It could, but `MacOSBridge::new()` is
  the only constructor and clear at call sites.

---

## `space.rs`

**What it does.** The state machine. Holds the per-space `LayoutTree`s,
the active space, focus tracking per space, the zoom flag, and the pending
split. Routes `Command` enum variants into mutations on this state and
calls into `WindowBridge` to apply them. By far the largest module.

### Patterns worth absorbing

- **Trait + mock for testability.** `WindowBridge` defined as a trait,
  `MockBridge` implements it for tests, `MacOSBridge` implements it for
  production. This is the single biggest reason `space.rs` has the test
  coverage it does. Internalise this pattern: any time you have OS
  side-effects you want to unit-test, hide them behind a trait.
- **Generic over the bridge.** `SpaceManager<B: WindowBridge>` — the
  implementation doesn't care which bridge; only `main.rs` and the tests
  pick concrete types. Zero-cost abstraction at runtime.
- **Property-style tests.** `every_window_in_exactly_one_space` is the
  best test in the codebase. It runs a sequence of mutations and then
  asserts that the resulting state satisfies an invariant ("every window
  appears in exactly one space"). This is much more valuable than asserting
  specific frame coordinates after specific commands.

### Changes applied

- **`let-else` consistently.** Six functions had the
  `let x = match opt { Some(v) => v, None => return };` shape. Converted to
  `let Some(x) = opt else { return };`. Same machine code, easier to read.
- **`launcher_split_down` was a copy of `split_vertical`.** Both produced
  `PendingSplit::Stack { col: focused_column() }`. Removed the duplicate
  — the `Command::LauncherSplitDown` arm in `handle_command` now calls
  `split_vertical()` directly. Less surface, no behaviour change.
- **Extracted `remove_window_from_layout`** — `move_to_space` and
  `handle_window_destroyed` had the same five-line "remove window from its
  space's layout, drop empty columns, drop empty non-config space" sequence.
  Now both call the same helper.
- **Simplified `reload_config`'s open-coded `next_free_space` logic** — it
  had the same loop as the existing `next_free_space` method, but with extra
  conditions. Refactored to use the existing method.

### Things considered, not changed

- Splitting `space.rs` into `space/state.rs` + `space/commands.rs` +
  `space/layout.rs`. The file is large (~1000 lines) but the cohesion is
  high and there's no obvious clean cut — the command handlers all touch
  the same state. Worth revisiting if it grows much further.
- `OFF_SCREEN` is a const `Rect { x: 99999.0, ... }` — the magic number is
  documented in `docs/LESSONS.md`. Could read better as a function `fn
  off_screen_for(screen_rect: Rect) -> Rect` that picks an offset
  guaranteed to be off-screen, but the current value works for any
  reasonable display.

---

## `main.rs`

**What it does.** Entry point. Loads config, builds the `SpaceManager`, registers
hotkeys and screen-change callback, sets up a 500ms `CFRunLoopTimer` whose
callback re-borrows the global `App` and calls `tick()`. The launcher
round-trip lives here.

### Patterns worth absorbing

- **`Rc<RefCell<App>>` for shared interior-mutable state on a single
  thread.** This is the standard pattern when you need callbacks that
  mutate shared state on a single-threaded run loop. Note: the `Rc` is fine
  because we never share across threads; the `RefCell` is fine because we
  always borrow inside `tick()` only. If we ever multi-thread this, we'd
  reach for `Arc<Mutex<...>>` instead.
- **`include_str!` to embed the default config in the binary.** Means a
  fresh install can always write a sane default; no need to ship a separate
  data file.

### Changes applied

- **`&Path` instead of `&PathBuf`** for `ensure_config` and `file_mtime`.
- **Removed the duplicate `bridge_registry` initialization** — `App` was
  built with an empty `BTreeMap`, and then immediately overwritten with the
  one populated from `initial_windows`. Simpler to populate the field
  directly inside the borrow.
- **Documented the `Rc<RefCell<App>>`-as-`*mut c_void` pattern with a
  comment** noting that the `info` pointer's validity depends on the `app`
  binding outliving the run loop. In practice `main` runs `run_current()`
  forever and never returns, but it's a real footgun if anyone ever adds
  a clean-exit path. The `retain`/`release` callbacks on the timer context
  are `None`, meaning the run loop won't keep the `Rc` alive itself.

### Things considered, not changed

- `parse_launcher_result` could move into `lib.rs` to be unit-testable,
  but it's three lines and tested implicitly by integration. Left alone.
- The launcher result polling reads the file every tick. This is fine —
  the launcher writes via tmp+rename which is atomic, so no torn-read risk.
  Documented in a comment near the read.

---

## Tests

**Before:** `tests/integration.rs` had four tests:

- `test_config_round_trip` — duplicated unit tests in `config.rs` and
  `hotkey.rs`.
- `test_tiling_deterministic` — calls `compute_layout` twice and asserts
  the results match. This is a tautology: `compute_layout` is a pure
  function with no hidden state, so it must be deterministic by definition.
  This test asserts that f64 arithmetic is deterministic.
- `test_window_discovery` — `#[ignore]`, depends on real OS state.
- `test_screen_detection` — `#[ignore]`, depends on real screen state.

The `#[ignore]` tests are not exercised by `cargo test` and rot fast.
`test_config_round_trip` and `test_tiling_deterministic` add no signal over
the existing unit tests. Deleted the file.

**After:** all tests now live as `#[cfg(test)] mod tests { ... }` blocks
inside the modules they test. New tests added:

- `hotkey::tests::expand_unknown_pattern_passes_through` — pins behaviour
  for `"alt-{xyz"` (no closing brace) and confirms it's treated as a literal.
- `space::tests::reload_config_keeps_config_space_when_app_removed` — pins
  the behaviour where removing an app from config keeps its existing
  windows on whatever space they ended up on, rather than orphaning them.
- `tiling::tests::gaps_consume_full_outer_padding` — adds an explicit gap
  test (the existing tests are all zero-gap).

`cargo test` runs everything in milliseconds with no skipped tests.

---

## Commits in this review

The diffs are split for readability. Read the doc section first, then the
diff, in order:

1. **`331832d` review/cleanup: dead code, &Path, derive(Default), screen const**
   — surface fixes; no behaviour change.
2. **`68116fe` review/refactor: dedupe watcher CFDictionary, hotkey table,
   space let-else** — invariant-preserving refactors. Tests still pass
   unchanged.
3. **`3a30b12` review/tests + polish: replace integration.rs, clear clippy**
   — drop the tautological integration tests, add three new focused unit
   tests, and clear up the small clippy warnings (`is_none_or`,
   `or_default()`, `impl Default for MacOSBridge`, etc.).

After all three: `cargo test` passes 72 tests in <1s with 0 ignored,
`cargo build --release` is clean, and `cargo clippy --all-targets` is
warning-free.

## Final state

- `src/` totals ~3,200 lines (was ~3,400).
- `tests/` directory removed; all tests live as `#[cfg(test)] mod tests`
  inside the modules they exercise.
- `docs/code-review.md` (this file) is the explanation of every non-trivial
  change.
- `docs/LESSONS.md` retains the macOS-API-specific knowledge from the
  original build.

If anything in this review feels wrong or unconvincing, push back. Code
review is not a one-way conversation, and the goal is for you to end up
understanding *why* every change was made, not just to take it on faith.
