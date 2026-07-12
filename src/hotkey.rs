use std::collections::HashMap;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

use crate::config::HotkeyConfig;
use crate::types::{Command, nflowError, Result};

pub const OPTION_KEY: u32 = 0x0800;
pub const SHIFT_KEY: u32 = 0x0200;
pub const CONTROL_KEY: u32 = 0x1000;
pub const CMD_KEY: u32 = 0x0100;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedHotkey {
    pub keycode: u32,
    pub modifiers: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HotkeyBinding {
    pub hotkey: ParsedHotkey,
    pub command: Command,
}

fn key_to_keycode(key: &str) -> Option<u32> {
    match key {
        "a" => Some(0x00),
        "s" => Some(0x01),
        "d" => Some(0x02),
        "f" => Some(0x03),
        "h" => Some(0x04),
        "g" => Some(0x05),
        "z" => Some(0x06),
        "x" => Some(0x07),
        "c" => Some(0x08),
        "v" => Some(0x09),
        "b" => Some(0x0B),
        "q" => Some(0x0C),
        "w" => Some(0x0D),
        "e" => Some(0x0E),
        "r" => Some(0x0F),
        "y" => Some(0x10),
        "t" => Some(0x11),
        "i" => Some(0x22),
        "1" => Some(0x12),
        "2" => Some(0x13),
        "3" => Some(0x14),
        "4" => Some(0x15),
        "5" => Some(0x17),
        "6" => Some(0x16),
        "7" => Some(0x1A),
        "8" => Some(0x1C),
        "9" => Some(0x19),
        "0" => Some(0x1D),
        "j" => Some(0x26),
        "k" => Some(0x28),
        "l" => Some(0x25),
        "/" => Some(0x2C),
        "." => Some(0x2F),
        "space" => Some(0x31),
        _ => None,
    }
}

pub fn char_for_keycode(keycode: u32) -> Option<char> {
    let c = match keycode {
        0x00 => 'a',
        0x0B => 'b',
        0x08 => 'c',
        0x02 => 'd',
        0x0E => 'e',
        0x03 => 'f',
        0x05 => 'g',
        0x04 => 'h',
        0x22 => 'i',
        0x26 => 'j',
        0x28 => 'k',
        0x25 => 'l',
        0x2E => 'm',
        0x2D => 'n',
        0x1F => 'o',
        0x23 => 'p',
        0x0C => 'q',
        0x0F => 'r',
        0x01 => 's',
        0x11 => 't',
        0x20 => 'u',
        0x09 => 'v',
        0x0D => 'w',
        0x07 => 'x',
        0x10 => 'y',
        0x06 => 'z',
        _ => return None,
    };
    Some(c)
}

fn modifier_to_flag(modifier: &str) -> Option<u32> {
    match modifier {
        "alt" | "option" => Some(OPTION_KEY),
        "shift" => Some(SHIFT_KEY),
        "ctrl" | "control" => Some(CONTROL_KEY),
        "cmd" | "command" => Some(CMD_KEY),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuShortcut {
    pub key: String,
    pub command: bool,
    pub option: bool,
    pub control: bool,
    pub shift: bool,
}

pub fn menu_shortcut(pattern: &str) -> Option<MenuShortcut> {
    let parts: Vec<&str> = pattern.split('-').collect();
    let (key, modifier_parts) = parts.split_last()?;
    key_to_keycode(key)?;
    let mut shortcut = MenuShortcut {
        key: key.to_string(),
        command: false,
        option: false,
        control: false,
        shift: false,
    };
    for part in modifier_parts {
        match modifier_to_flag(part)? {
            OPTION_KEY => shortcut.option = true,
            SHIFT_KEY => shortcut.shift = true,
            CONTROL_KEY => shortcut.control = true,
            CMD_KEY => shortcut.command = true,
            _ => return None,
        }
    }
    Some(shortcut)
}

pub fn parse_hotkey(input: &str) -> Result<ParsedHotkey> {
    if input.is_empty() {
        return Err(nflowError::ConfigParse(
            "hotkey string is empty".to_string(),
        ));
    }

    let parts: Vec<&str> = input.split('-').collect();
    if parts.is_empty() {
        return Err(nflowError::ConfigParse(
            "hotkey string is empty".to_string(),
        ));
    }

    let key = parts[parts.len() - 1];
    let modifier_parts = &parts[..parts.len() - 1];

    let keycode = key_to_keycode(key)
        .ok_or_else(|| nflowError::ConfigParse(format!("unknown key: {key}")))?;

    let mut modifiers: u32 = 0;
    for modifier in modifier_parts {
        let flag = modifier_to_flag(modifier)
            .ok_or_else(|| nflowError::ConfigParse(format!("unknown modifier: {modifier}")))?;
        modifiers |= flag;
    }

    Ok(ParsedHotkey { keycode, modifiers })
}

pub fn expand_hotkey_pattern(pattern: &str) -> Result<Vec<String>> {
    if let Some(start) = pattern.find('{') {
        if let Some(end) = pattern.find('}') {
            let prefix = &pattern[..start];
            let inner = &pattern[start + 1..end];
            let suffix = &pattern[end + 1..];

            if inner == "n" {
                let expanded = (1u8..=9).map(|n| format!("{prefix}{n}{suffix}")).collect();
                return Ok(expanded);
            }

            let chars: Vec<char> = inner.chars().collect();
            let expanded = chars
                .into_iter()
                .map(|c| format!("{prefix}{c}{suffix}"))
                .collect();
            return Ok(expanded);
        }
    }

    Ok(vec![pattern.to_string()])
}

pub fn build_bindings(config: &HotkeyConfig) -> Result<Vec<HotkeyBinding>> {
    let mut bindings = Vec::new();

    let mut push = |pattern: &str, command: Command| -> Result<()> {
        bindings.push(HotkeyBinding {
            hotkey: parse_hotkey(pattern)?,
            command,
        });
        Ok(())
    };

    let optional = [
        (&config.hint_mode, Command::HintMode),
        (&config.hint_mode_right_click, Command::HintModeRightClick),
        (&config.hint_mode_copy_link, Command::HintModeCopyLink),
        (&config.text_select, Command::TextSelect),
        (&config.scroll_mode, Command::ScrollMode),
    ];
    for (maybe_pattern, command) in optional {
        if let Some(pattern) = maybe_pattern {
            push(pattern, command)?;
        }
    }

    if let Some(pattern) = &config.apply_scene {
        if pattern.contains("{n}") {
            for n in 0usize..=9 {
                push(
                    &pattern.replace("{n}", &n.to_string()),
                    Command::ApplyScene(n),
                )?;
            }
        } else {
            push(pattern, Command::ApplyScene(0))?;
        }
    }

    Ok(bindings)
}

type CommandCallback = Box<dyn Fn(Command) + Send>;
static COMMAND_CALLBACK: Mutex<Option<CommandCallback>> = Mutex::new(None);

pub fn set_command_callback<F: Fn(Command) + Send + 'static>(f: F) {
    let mut cb = COMMAND_CALLBACK.lock().unwrap();
    *cb = Some(Box::new(f));
}

const CG_EVENT_FLAG_MASK_ALTERNATE: u64 = 0x00080000;
const CG_EVENT_FLAG_MASK_SHIFT: u64 = 0x00020000;
const CG_EVENT_FLAG_MASK_CONTROL: u64 = 0x00040000;
const CG_EVENT_FLAG_MASK_COMMAND: u64 = 0x00100000;

static BINDING_MAP: Mutex<Option<HashMap<(u32, u32), Command>>> = Mutex::new(None);

fn cg_flags_to_modifier_mask(flags: u64) -> u32 {
    let mut mask = 0u32;
    if flags & CG_EVENT_FLAG_MASK_ALTERNATE != 0 {
        mask |= OPTION_KEY;
    }
    if flags & CG_EVENT_FLAG_MASK_SHIFT != 0 {
        mask |= SHIFT_KEY;
    }
    if flags & CG_EVENT_FLAG_MASK_CONTROL != 0 {
        mask |= CONTROL_KEY;
    }
    if flags & CG_EVENT_FLAG_MASK_COMMAND != 0 {
        mask |= CMD_KEY;
    }
    mask
}

type CGEventTapProxy = *mut std::ffi::c_void;
type CGEventRef = *mut std::ffi::c_void;
type CFMachPortRef = *mut std::ffi::c_void;

extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: unsafe extern "C" fn(
            CGEventTapProxy,
            u32,
            CGEventRef,
            *mut std::ffi::c_void,
        ) -> CGEventRef,
        user_info: *mut std::ffi::c_void,
    ) -> CFMachPortRef;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventKeyboardGetUnicodeString(
        event: CGEventRef,
        max_string_length: usize,
        actual_string_length: *mut usize,
        unicode_string: *mut u16,
    );
    fn CFMachPortCreateRunLoopSource(
        allocator: *const std::ffi::c_void,
        port: CFMachPortRef,
        order: i64,
    ) -> *mut std::ffi::c_void;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

const K_CG_SESSION_EVENT_TAP: u32 = 1;
const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const K_CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;
const K_CG_EVENT_KEY_DOWN_MASK: u64 = 1 << 10;
const K_CG_EVENT_KEY_UP_MASK: u64 = 1 << 11;
const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
const KEY_DOWN_EVENT_TYPE: u32 = 10;
const KEY_UP_EVENT_TYPE: u32 = 11;
const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

static EVENT_TAP: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

unsafe fn typed_char(event: CGEventRef) -> Option<char> {
    let mut buffer = [0u16; 4];
    let mut length: usize = 0;
    CGEventKeyboardGetUnicodeString(event, buffer.len(), &mut length, buffer.as_mut_ptr());
    if length == 0 {
        return None;
    }
    String::from_utf16_lossy(&buffer[..length.min(buffer.len())])
        .chars()
        .next()
        .filter(|c| !c.is_control())
}

fn hint_action_for(cmd: &Command) -> Option<crate::hint::HintAction> {
    use crate::hint::{ClickKind, HintAction};
    match cmd {
        Command::HintMode => Some(HintAction::Click(ClickKind::Left)),
        Command::HintModeRightClick => Some(HintAction::Click(ClickKind::Right)),
        Command::HintModeCopyLink => Some(HintAction::CopyLink),
        _ => None,
    }
}

pub fn run_mode_command(cmd: &Command) -> bool {
    if let Some(action) = hint_action_for(cmd) {
        crate::hint::toggle(crate::screen::get_full_screen_rect(), action);
        return true;
    }
    if *cmd == Command::TextSelect {
        crate::textselect::toggle(crate::screen::get_full_screen_rect());
        return true;
    }
    if *cmd == Command::ScrollMode {
        crate::scroll::toggle(crate::screen::get_full_screen_rect());
        return true;
    }
    false
}

unsafe extern "C" fn event_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    _user_info: *mut std::ffi::c_void,
) -> CGEventRef {
    if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
        || event_type == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
    {
        let tap = EVENT_TAP.load(Ordering::SeqCst);
        if !tap.is_null() {
            log::warn!("event tap disabled (type {event_type:#x}), re-enabling");
            CGEventTapEnable(tap, true);
        }
        return event;
    }

    if event_type == KEY_UP_EVENT_TYPE {
        if crate::scroll::is_active() {
            let keycode = CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) as u32;
            crate::scroll::handle_key_up(keycode);
            return std::ptr::null_mut();
        }
        return event;
    }

    if event_type == KEY_DOWN_EVENT_TYPE {
        let keycode = CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) as u32;
        let flags = CGEventGetFlags(event);
        let modifiers = cg_flags_to_modifier_mask(flags);

        const ESC_KEYCODE: u32 = 0x35;
        const DELETE_KEYCODE: u32 = 0x33;
        const RETURN_KEYCODE: u32 = 0x24;
        const Q_KEYCODE: u32 = 0x0C;

        if crate::hint::is_active() {
            crate::hint::handle_key(keycode, keycode == ESC_KEYCODE, keycode == DELETE_KEYCODE);
            return std::ptr::null_mut();
        }

        if crate::textselect::is_active() {
            crate::textselect::handle_key(
                keycode,
                typed_char(event),
                keycode == ESC_KEYCODE,
                keycode == DELETE_KEYCODE,
                keycode == RETURN_KEYCODE,
            );
            return std::ptr::null_mut();
        }

        if crate::scroll::is_active() {
            crate::scroll::handle_key(
                keycode,
                modifiers,
                keycode == ESC_KEYCODE || keycode == Q_KEYCODE,
                keycode == DELETE_KEYCODE,
            );
            return std::ptr::null_mut();
        }

        let command = {
            let map = BINDING_MAP.lock().unwrap();
            map.as_ref()
                .and_then(|m| m.get(&(keycode, modifiers)).cloned())
        };

        if let Some(cmd) = command {
            if run_mode_command(&cmd) {
                return std::ptr::null_mut();
            }
            log::info!("hotkey fired: {cmd:?}");
            let cb = COMMAND_CALLBACK.lock().unwrap();
            if let Some(f) = cb.as_ref() {
                f(cmd);
            }
            return std::ptr::null_mut();
        }
    }

    event
}

pub fn register_hotkeys(bindings: &[HotkeyBinding]) -> Result<()> {
    let mut map = HashMap::new();
    for binding in bindings {
        map.insert(
            (binding.hotkey.keycode, binding.hotkey.modifiers),
            binding.command.clone(),
        );
    }

    log::info!("registering {} hotkey bindings via CGEventTap", map.len());
    *BINDING_MAP.lock().unwrap() = Some(map);

    let tap = unsafe {
        CGEventTapCreate(
            K_CG_SESSION_EVENT_TAP,
            K_CG_HEAD_INSERT_EVENT_TAP,
            K_CG_EVENT_TAP_OPTION_DEFAULT,
            K_CG_EVENT_KEY_DOWN_MASK | K_CG_EVENT_KEY_UP_MASK,
            event_tap_callback,
            std::ptr::null_mut(),
        )
    };

    if tap.is_null() {
        return Err(nflowError::HotkeyRegistration(
            "CGEventTapCreate failed -- ensure Accessibility permission is granted".into(),
        ));
    }

    EVENT_TAP.store(tap, Ordering::SeqCst);

    unsafe {
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        if !source.is_null() {
            use core_foundation::base::TCFType;
            use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
            let rl = CFRunLoop::get_current();
            core_foundation_sys::runloop::CFRunLoopAddSource(
                rl.as_concrete_TypeRef() as *mut _,
                source as *mut _,
                kCFRunLoopCommonModes,
            );
        }
        CGEventTapEnable(tap, true);
    }

    log::info!("CGEventTap installed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> HotkeyConfig {
        HotkeyConfig::default()
    }

    #[test]
    fn parse_alt_h() {
        let hotkey = parse_hotkey("alt-h").unwrap();
        assert_eq!(hotkey.modifiers, OPTION_KEY);
        assert_eq!(hotkey.keycode, 0x04);
    }

    #[test]
    fn apply_scene_expands_zero_through_nine() {
        let config = HotkeyConfig {
            apply_scene: Some("alt-ctrl-{n}".to_string()),
            ..sample_config()
        };
        let bindings = build_bindings(&config).unwrap();
        let scenes: Vec<usize> = bindings
            .iter()
            .filter_map(|b| match b.command {
                Command::ApplyScene(n) => Some(n),
                _ => None,
            })
            .collect();
        assert_eq!(scenes, (0..=9).collect::<Vec<_>>());
    }

    #[test]
    fn apply_scene_absent_by_default() {
        let bindings = build_bindings(&sample_config()).unwrap();
        assert!(!bindings
            .iter()
            .any(|b| matches!(b.command, Command::ApplyScene(_))));
    }

    #[test]
    fn hint_mode_binding_present_when_set() {
        let config = HotkeyConfig {
            hint_mode: Some("alt-cmd-shift-/".to_string()),
            ..sample_config()
        };
        let bindings = build_bindings(&config).unwrap();
        let b = bindings
            .iter()
            .find(|b| b.command == Command::HintMode)
            .expect("HintMode binding");
        assert_eq!(b.hotkey.modifiers, OPTION_KEY | CMD_KEY | SHIFT_KEY);
        assert_eq!(b.hotkey.keycode, 0x2C);
    }

    #[test]
    fn hint_mode_absent_by_default() {
        let bindings = build_bindings(&sample_config()).unwrap();
        assert!(!bindings.iter().any(|b| b.command == Command::HintMode));
    }

    #[test]
    fn copy_link_and_text_select_bindings_present_when_set() {
        let config = HotkeyConfig {
            hint_mode_copy_link: Some("alt-cmd-shift-l".to_string()),
            text_select: Some("alt-cmd-shift-y".to_string()),
            ..sample_config()
        };
        let bindings = build_bindings(&config).unwrap();
        assert!(bindings
            .iter()
            .any(|b| b.command == Command::HintModeCopyLink));
        assert!(bindings.iter().any(|b| b.command == Command::TextSelect));
    }

    #[test]
    fn copy_link_and_text_select_absent_by_default() {
        let bindings = build_bindings(&sample_config()).unwrap();
        assert!(!bindings
            .iter()
            .any(|b| b.command == Command::HintModeCopyLink));
        assert!(!bindings.iter().any(|b| b.command == Command::TextSelect));
    }

    #[test]
    fn parse_alt_cmd_shift_period() {
        let hotkey = parse_hotkey("alt-cmd-shift-.").unwrap();
        assert_eq!(hotkey.modifiers, OPTION_KEY | CMD_KEY | SHIFT_KEY);
        assert_eq!(hotkey.keycode, 0x2F);
    }

    #[test]
    fn parse_alt_cmd_shift_space() {
        let hotkey = parse_hotkey("alt-cmd-shift-space").unwrap();
        assert_eq!(hotkey.modifiers, OPTION_KEY | CMD_KEY | SHIFT_KEY);
        assert_eq!(hotkey.keycode, 0x31);
    }

    #[test]
    fn hint_mode_right_click_binding_present_when_set() {
        let config = HotkeyConfig {
            hint_mode_right_click: Some("alt-cmd-shift-.".to_string()),
            ..sample_config()
        };
        let bindings = build_bindings(&config).unwrap();
        let b = bindings
            .iter()
            .find(|b| b.command == Command::HintModeRightClick)
            .expect("HintModeRightClick binding");
        assert_eq!(b.hotkey.modifiers, OPTION_KEY | CMD_KEY | SHIFT_KEY);
        assert_eq!(b.hotkey.keycode, 0x2F);
    }

    #[test]
    fn hint_mode_right_click_absent_by_default() {
        let bindings = build_bindings(&sample_config()).unwrap();
        assert!(!bindings
            .iter()
            .any(|b| b.command == Command::HintModeRightClick));
    }

    #[test]
    fn parse_alt_cmd_shift_slash() {
        let hotkey = parse_hotkey("alt-cmd-shift-/").unwrap();
        assert_eq!(hotkey.modifiers, OPTION_KEY | CMD_KEY | SHIFT_KEY);
        assert_eq!(hotkey.keycode, 0x2C);
    }

    #[test]
    fn parse_alt_shift_1() {
        let hotkey = parse_hotkey("alt-shift-1").unwrap();
        assert_eq!(hotkey.modifiers, OPTION_KEY | SHIFT_KEY);
        assert_eq!(hotkey.keycode, 0x12);
    }

    #[test]
    fn parse_alt_t() {
        let hotkey = parse_hotkey("alt-t").unwrap();
        assert_eq!(hotkey.keycode, 0x11);
    }

    #[test]
    fn parse_alt_v() {
        let hotkey = parse_hotkey("alt-v").unwrap();
        assert_eq!(hotkey.keycode, 0x09);
    }

    #[test]
    fn parse_invalid_key_returns_error() {
        let result = parse_hotkey("alt-z99");
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_modifier_returns_error() {
        let result = parse_hotkey("super-h");
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_string_returns_error() {
        let result = parse_hotkey("");
        assert!(result.is_err());
    }

    #[test]
    fn expand_n_pattern() {
        let expanded = expand_hotkey_pattern("alt-{n}").unwrap();
        assert_eq!(expanded.len(), 9);
        assert_eq!(expanded[0], "alt-1");
        assert_eq!(expanded[8], "alt-9");
    }

    #[test]
    fn expand_hjkl_pattern() {
        let expanded = expand_hotkey_pattern("alt-{hjkl}").unwrap();
        assert_eq!(expanded.len(), 4);
        assert_eq!(expanded[0], "alt-h");
        assert_eq!(expanded[1], "alt-j");
        assert_eq!(expanded[2], "alt-k");
        assert_eq!(expanded[3], "alt-l");
    }

    #[test]
    fn expand_shift_n_pattern() {
        let expanded = expand_hotkey_pattern("alt-shift-{n}").unwrap();
        assert_eq!(expanded.len(), 9);
        assert_eq!(expanded[0], "alt-shift-1");
        assert_eq!(expanded[8], "alt-shift-9");
    }

    #[test]
    fn no_pattern_returns_single() {
        let expanded = expand_hotkey_pattern("alt-h").unwrap();
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0], "alt-h");
    }

    #[test]
    fn unclosed_pattern_passes_through_as_literal() {
        let expanded = expand_hotkey_pattern("alt-{n").unwrap();
        assert_eq!(expanded, vec!["alt-{n".to_string()]);
    }

    #[test]
    fn menu_shortcut_parses_modifiers_and_key() {
        let shortcut = menu_shortcut("alt-cmd-shift-/").unwrap();
        assert_eq!(shortcut.key, "/");
        assert!(shortcut.option);
        assert!(shortcut.command);
        assert!(shortcut.shift);
        assert!(!shortcut.control);
    }

    #[test]
    fn menu_shortcut_keeps_named_keys() {
        let shortcut = menu_shortcut("cmd-space").unwrap();
        assert_eq!(shortcut.key, "space");
        assert!(shortcut.command);
    }

    #[test]
    fn menu_shortcut_rejects_unknown_tokens() {
        assert_eq!(menu_shortcut("hyper-i"), None);
        assert_eq!(menu_shortcut("cmd-unknownkey"), None);
        assert_eq!(menu_shortcut(""), None);
    }

    #[test]
    fn empty_config_produces_no_bindings() {
        let bindings = build_bindings(&sample_config()).unwrap();
        assert!(bindings.is_empty());
    }

    #[test]
    fn build_bindings_from_full_config() {
        let config = HotkeyConfig {
            apply_scene: Some("alt-ctrl-{n}".to_string()),
            hint_mode: Some("alt-cmd-shift-/".to_string()),
            hint_mode_right_click: Some("alt-cmd-shift-space".to_string()),
            hint_mode_copy_link: Some("cmd-shift-l".to_string()),
            text_select: Some("cmd-shift-y".to_string()),
            scroll_mode: Some("cmd-shift-i".to_string()),
        };
        let bindings = build_bindings(&config).unwrap();
        assert_eq!(bindings.len(), 15);

        let scenes = bindings
            .iter()
            .filter(|b| matches!(b.command, Command::ApplyScene(_)))
            .count();
        assert_eq!(scenes, 10);
        assert!(bindings.iter().any(|b| b.command == Command::HintMode));
        assert!(bindings.iter().any(|b| b.command == Command::TextSelect));
    }
}
