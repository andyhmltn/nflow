use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use crate::types::{nflowError, Result, SpaceId};

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct HotkeyConfig {
    #[serde(rename = "apply-scene", default)]
    pub apply_scene: Option<String>,
    #[serde(rename = "hint-mode", default)]
    pub hint_mode: Option<String>,
    #[serde(rename = "hint-mode-right-click", default)]
    pub hint_mode_right_click: Option<String>,
    #[serde(rename = "hint-mode-copy-link", default)]
    pub hint_mode_copy_link: Option<String>,
    #[serde(rename = "text-select", default)]
    pub text_select: Option<String>,
    #[serde(rename = "scroll-mode", default)]
    pub scroll_mode: Option<String>,
    #[serde(rename = "menu-search", default)]
    pub menu_search: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SpaceConfig {
    pub apps: Vec<String>,
    #[serde(default)]
    pub weights: BTreeMap<String, f64>,
    #[serde(rename = "hide-windows", default)]
    pub hide_windows: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Scene {
    pub number: usize,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub spaces: BTreeMap<String, SpaceConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Profile {
    #[serde(rename = "screen-width-min")]
    pub screen_width_min: Option<u32>,
    #[serde(rename = "screen-width-max")]
    pub screen_width_max: Option<u32>,
    #[serde(default)]
    pub gaps: Option<GapsConfig>,
    pub spaces: BTreeMap<String, SpaceConfig>,
    #[serde(default)]
    pub scene: BTreeMap<String, Scene>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct GapsConfig {
    #[serde(default)]
    pub outer: f64,
    #[serde(default)]
    pub inner: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct IgnoreConfig {
    #[serde(default)]
    pub apps: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub hotkeys: HotkeyConfig,
    #[serde(default)]
    pub gaps: GapsConfig,
    #[serde(default)]
    pub terminal: Option<String>,
    #[serde(default)]
    pub ignore: IgnoreConfig,
    pub profile: BTreeMap<String, Profile>,
}

pub fn parse_config_str(s: &str) -> Result<Config> {
    toml::from_str(s).map_err(|e| nflowError::ConfigParse(e.to_string()))
}

pub fn parse_config_file(path: &Path) -> Result<Config> {
    let contents =
        std::fs::read_to_string(path).map_err(|e| nflowError::ConfigParse(e.to_string()))?;
    parse_config_str(&contents)
}

pub fn select_profile(config: &Config, screen_width: u32) -> Option<&Profile> {
    for profile in config.profile.values() {
        let min_ok = profile
            .screen_width_min
            .is_none_or(|min| screen_width >= min);
        let max_ok = profile
            .screen_width_max
            .is_none_or(|max| screen_width <= max);
        if min_ok && max_ok {
            return Some(profile);
        }
    }
    config.profile.values().next()
}

pub fn effective_gaps(config: &Config, profile: &Profile) -> (f64, f64) {
    let gaps = profile.gaps.as_ref().unwrap_or(&config.gaps);
    (gaps.outer, gaps.inner)
}

pub fn app_layout_lookup(profile: &Profile) -> BTreeMap<String, (SpaceId, usize, f64)> {
    app_layout_lookup_with_scene(profile, 0)
}

pub fn scene_list(profile: &Profile) -> Vec<(usize, String)> {
    let mut result: Vec<(usize, String)> = vec![(0, "Default".to_string())];
    for (key, scene) in &profile.scene {
        let label = scene.name.clone().unwrap_or_else(|| key.clone());
        result.push((scene.number, label));
    }
    result.sort_by_key(|(number, _)| *number);
    result.dedup_by_key(|(number, _)| *number);
    result
}

fn merged_spaces_with_scene(
    profile: &Profile,
    scene_number: usize,
) -> BTreeMap<String, SpaceConfig> {
    let mut spaces = profile.spaces.clone();

    if scene_number != 0 {
        if let Some(scene) = profile.scene.values().find(|p| p.number == scene_number) {
            for (key, space_config) in &scene.spaces {
                spaces.insert(key.clone(), space_config.clone());
            }
        }
    }

    spaces
}

pub fn hide_titles_with_scene(profile: &Profile, scene_number: usize) -> Vec<String> {
    let spaces = merged_spaces_with_scene(profile, scene_number);
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for space_config in spaces.values() {
        for title in &space_config.hide_windows {
            if seen.insert(title.clone()) {
                result.push(title.clone());
            }
        }
    }
    result
}

pub fn app_layout_lookup_with_scene(
    profile: &Profile,
    scene_number: usize,
) -> BTreeMap<String, (SpaceId, usize, f64)> {
    let spaces = merged_spaces_with_scene(profile, scene_number);

    let mut result = BTreeMap::new();
    for (key, space_config) in &spaces {
        let Ok(id) = key.parse::<SpaceId>() else {
            continue;
        };
        for (idx, app) in space_config.apps.iter().enumerate() {
            let weight = space_config.weights.get(app).copied().unwrap_or(1.0);
            result.insert(app.clone(), (id, idx, weight));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_toml() -> &'static str {
        r#"
[profile.laptop]
screen-width-max = 2560

[profile.laptop.spaces.1]
apps = ["Zen Browser", "Ghostty"]

[profile.laptop.spaces.2]
apps = ["Slack"]

[profile.ultrawide]
screen-width-min = 2561

[profile.ultrawide.spaces.1]
apps = ["Zen Browser", "Ghostty"]

[profile.ultrawide.spaces.2]
apps = ["Slack"]

[profile.ultrawide.spaces.3]
apps = ["Spotify"]
"#
    }

    #[test]
    fn parse_valid_config() {
        let config = parse_config_str(sample_toml()).unwrap();
        assert_eq!(config.profile.len(), 2);
        assert!(config.profile.contains_key("laptop"));
        assert!(config.profile.contains_key("ultrawide"));
    }

    #[test]
    fn parse_profile_spaces() {
        let config = parse_config_str(sample_toml()).unwrap();
        let laptop = &config.profile["laptop"];
        assert_eq!(laptop.screen_width_max, Some(2560));
        assert_eq!(laptop.screen_width_min, None);
        assert_eq!(laptop.spaces["1"].apps, vec!["Zen Browser", "Ghostty"]);
        assert_eq!(laptop.spaces["2"].apps, vec!["Slack"]);
    }

    #[test]
    fn parse_ultrawide_profile() {
        let config = parse_config_str(sample_toml()).unwrap();
        let uw = &config.profile["ultrawide"];
        assert_eq!(uw.screen_width_min, Some(2561));
        assert_eq!(uw.spaces.len(), 3);
        assert_eq!(uw.spaces["3"].apps, vec!["Spotify"]);
    }

    #[test]
    fn parse_invalid_toml_returns_error() {
        let result = parse_config_str("this is not valid toml [[[");
        assert!(result.is_err());
    }

    #[test]
    fn omitting_hotkeys_is_ok() {
        let config = parse_config_str(
            "[profile.laptop]\nscreen-width-max = 2560\n\n[profile.laptop.spaces.1]\napps = [\"Ghostty\"]\n",
        )
        .unwrap();
        assert_eq!(config.hotkeys, HotkeyConfig::default());
    }

    #[test]
    fn parse_missing_profile_returns_error() {
        let result = parse_config_str("[gaps]\nouter = 4\n");
        assert!(result.is_err());
    }

    #[test]
    fn select_laptop_profile() {
        let config = parse_config_str(sample_toml()).unwrap();
        let profile = select_profile(&config, 2560).unwrap();
        assert_eq!(profile.screen_width_max, Some(2560));
    }

    #[test]
    fn select_ultrawide_profile() {
        let config = parse_config_str(sample_toml()).unwrap();
        let profile = select_profile(&config, 5120).unwrap();
        assert_eq!(profile.screen_width_min, Some(2561));
    }

    #[test]
    fn select_profile_fallback_to_first() {
        let config = parse_config_str(sample_toml()).unwrap();
        let profile = select_profile(&config, 1);
        assert!(profile.is_some());
    }

    #[test]
    fn parse_space_with_weights() {
        let toml = r#"
[profile.uw]
screen-width-min = 3000

[profile.uw.spaces.1]
apps = ["Zen", "Ghostty", "Slack"]
weights = { Ghostty = 2.0 }
"#;
        let config = parse_config_str(toml).unwrap();
        let space = &config.profile["uw"].spaces["1"];
        assert_eq!(space.weights.get("Ghostty"), Some(&2.0));
        assert!(!space.weights.contains_key("Zen"));
    }

    #[test]
    fn profile_gaps_override_top_level() {
        let toml = r#"
[gaps]
outer = 50
inner = 50

[profile.laptop]
screen-width-max = 3439

[profile.laptop.gaps]
outer = 0
inner = 0

[profile.laptop.spaces.1]
apps = ["Ghostty"]

[profile.ultrawide]
screen-width-min = 3440

[profile.ultrawide.spaces.1]
apps = ["Ghostty"]
"#;
        let config = parse_config_str(toml).unwrap();
        let laptop = &config.profile["laptop"];
        let uw = &config.profile["ultrawide"];

        assert_eq!(effective_gaps(&config, laptop), (0.0, 0.0));
        assert_eq!(effective_gaps(&config, uw), (50.0, 50.0));
    }

    #[test]
    fn app_layout_lookup_carries_index_and_weight() {
        let toml = r#"
[profile.uw]
screen-width-min = 3000

[profile.uw.spaces.1]
apps = ["Zen", "Ghostty", "Slack"]
weights = { Ghostty = 2.0 }
"#;
        let config = parse_config_str(toml).unwrap();
        let profile = &config.profile["uw"];
        let lookup = app_layout_lookup(profile);

        assert_eq!(lookup["Zen"], (1, 0, 1.0));
        assert_eq!(lookup["Ghostty"], (1, 1, 2.0));
        assert_eq!(lookup["Slack"], (1, 2, 1.0));
    }

    fn scene_toml() -> &'static str {
        r#"
[hotkeys]
apply-scene = "alt-ctrl-{n}"

[profile.uw]
screen-width-min = 3000

[profile.uw.spaces.1]
apps = ["Zen", "Slack"]

[profile.uw.spaces.3]
apps = ["Spotify"]

[profile.uw.scene.coding]
number = 1

[profile.uw.scene.coding.spaces.1]
apps = ["Ghostty", "Zen"]
weights = { Ghostty = 2.0 }

[profile.uw.scene.meetings]
number = 2

[profile.uw.scene.meetings.spaces.1]
apps = ["Zen", "Slack"]
weights = { "Microsoft Teams" = 2.0 }
"#
    }

    #[test]
    fn parse_scenes() {
        let config = parse_config_str(scene_toml()).unwrap();
        assert_eq!(config.hotkeys.apply_scene.as_deref(), Some("alt-ctrl-{n}"));
        let uw = &config.profile["uw"];
        assert_eq!(uw.scene.len(), 2);
        assert_eq!(uw.scene["coding"].number, 1);
        assert_eq!(uw.scene["coding"].spaces["1"].apps, vec!["Ghostty", "Zen"]);
    }

    #[test]
    fn scene_zero_yields_profile_default() {
        let config = parse_config_str(scene_toml()).unwrap();
        let profile = &config.profile["uw"];
        let lookup = app_layout_lookup_with_scene(profile, 0);
        assert_eq!(lookup["Zen"], (1, 0, 1.0));
        assert_eq!(lookup["Slack"], (1, 1, 1.0));
        assert!(!lookup.contains_key("Ghostty"));
    }

    #[test]
    fn scene_overrides_named_space_only() {
        let config = parse_config_str(scene_toml()).unwrap();
        let profile = &config.profile["uw"];
        let lookup = app_layout_lookup_with_scene(profile, 1);
        assert_eq!(lookup["Ghostty"], (1, 0, 2.0));
        assert_eq!(lookup["Zen"], (1, 1, 1.0));
        assert!(!lookup.contains_key("Slack"));
        assert_eq!(lookup["Spotify"], (3, 0, 1.0));
    }

    #[test]
    fn parse_hide_windows_defaults_empty_and_collects_from_scene() {
        let toml = r#"
[hotkeys]
apply-scene = "alt-ctrl-{n}"

[profile.uw]
screen-width-min = 3000

[profile.uw.spaces.1]
apps = ["Zen", "Slack"]

[profile.uw.scene.chat]
number = 2

[profile.uw.scene.chat.spaces.1]
apps = ["Zen", "Microsoft Teams", "Slack"]
hide-windows = ["Calendar"]
"#;
        let config = parse_config_str(toml).unwrap();
        let profile = &config.profile["uw"];
        assert_eq!(profile.spaces["1"].hide_windows, Vec::<String>::new());
        assert!(hide_titles_with_scene(profile, 0).is_empty());
        let hide = hide_titles_with_scene(profile, 2);
        assert_eq!(hide, vec!["Calendar".to_string()]);
    }

    #[test]
    fn unknown_scene_number_falls_back_to_default() {
        let config = parse_config_str(scene_toml()).unwrap();
        let profile = &config.profile["uw"];
        let lookup = app_layout_lookup_with_scene(profile, 7);
        assert_eq!(lookup["Zen"], (1, 0, 1.0));
        assert_eq!(lookup["Slack"], (1, 1, 1.0));
    }
}
