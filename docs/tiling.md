# Tiling Architecture

nflow tiles windows into columns, one column per app, ordered left-to-right by the app's position in the config's `apps` array.

## Column ordering

Each space declares an ordered list of apps in `apps = [...]`. Columns are inserted at the index implied by this array, regardless of spawn order. Slack spawns first but is configured third? It lands in the third column. This means the config file is the single source of truth for layout -- window order is deterministic and never requires manual rearrangement.

Unconfigured apps (not listed in any `apps` array) are placed on an ad-hoc space and cannot be ordered or weighted.

## Column weights

Each column has a `weight` field that controls its proportional width relative to other columns in the same space. Weights come from the optional `weights` map in config:

```toml
[profile.ultrawide.spaces.1]
apps = ["Zen", "Ghostty", "Slack"]
weights = { Ghostty = 2.0 }
```

The usable width is distributed by `weight / sum(weights)`. With weights of 1.0, 2.0, and 1.0, Ghostty gets half the width and Zen and Slack split the remaining half equally. Weights default to 1.0 when omitted.

### Stacked windows

A column can hold multiple stacked windows (same app, multiple windows). Stacked windows are sized equally within the column and do not affect the column's weight or position.

## Width computation

The tiling engine (`src/tiling.rs`) applies:

1. Outer gaps: subtracted from the screen edges.
2. Inner gaps: subtracted between adjacent columns and between stacked windows.
3. Weighted proportional split: each non-empty column receives `usable_width * (col.weight / sum_of_weights)`.

If all weights sum to zero or less, columns fall back to equal split.

## Spaces and profiles

Profiles are selected automatically by screen width. Each profile defines its own set of spaces and app layouts. When the display configuration changes (monitor plugged or unplugged), nflow re-evaluates the active profile and resets to its default scene.

### Scene overlays

Scenes let you swap a profile's app layout on individual spaces without editing the config file. Each scene declares a subset of spaces with overridden `apps` arrays (and optionally `weights`). Spaces not mentioned in a scene keep their profile defaults.

The active scene is set via the `apply-scene` hotkey or the status bar menu. Scene `0` is always the profile default.

## Layout enforcement

Layouts are re-computed every tick (500ms) because some apps (terminal emulators, browsers, Slack) restore their original size after being tiled. Each window has a small retry budget per requested frame; once exhausted, nflow accepts the drift to avoid fighting hard caps set by the app.
