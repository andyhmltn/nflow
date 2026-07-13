use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::SystemTime;

use core_foundation::date::CFAbsoluteTimeGetCurrent;
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopTimer};
use core_foundation_sys::runloop::{CFRunLoopTimerContext, CFRunLoopTimerRef};
use objc2_app_kit::NSApplication;
use objc2_foundation::MainThreadMarker;

use nflow::ax::{frontmost_app, is_accessibility_enabled, MacOSBridge};
use nflow::config::{
    app_layout_lookup_with_scene, effective_gaps, hide_titles_with_scene, parse_config_file,
    parse_config_str, scene_list, select_profile, HotkeyConfig,
};
use nflow::daemon;
use nflow::hotkey::{build_bindings, register_hotkeys, set_command_callback};
use nflow::screen::{
    check_screen_changed, get_screen_rect, get_screen_width, register_screen_change_callback,
};
use nflow::space::SpaceManager;
use nflow::statusbar;
use nflow::types::Command;
use nflow::watcher::WindowWatcher;

const DEFAULT_CONFIG: &str = include_str!("../default_config.toml");

struct App {
    space_manager: SpaceManager<MacOSBridge>,
    watcher: WindowWatcher,
    config_path: PathBuf,
    config_mtime: Option<SystemTime>,
    screen_width: u32,
    command_rx: mpsc::Receiver<Command>,
    bridge_registry: BTreeMap<u32, i32>,
    last_frontmost_pid: Option<i32>,
    terminal: Option<String>,
    active_scene: usize,
    scene_labels: Vec<(usize, String)>,
}

fn config_dir() -> PathBuf {
    daemon::config_dir()
}

fn ensure_config(dir: &Path) -> PathBuf {
    let config_file = dir.join("config.toml");
    if !config_file.exists() {
        std::fs::create_dir_all(dir).expect("failed to create config directory");
        std::fs::write(&config_file, DEFAULT_CONFIG).expect("failed to write default config");
        log::info!("created default config at {}", config_file.display());
    }
    config_file
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

extern "C" fn timer_callback(_timer: CFRunLoopTimerRef, info: *mut std::ffi::c_void) {
    let app_ptr = info as *const Rc<RefCell<App>>;
    let app = unsafe { &*app_ptr };
    tick(app);
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        None | Some("start") => daemon::start(),
        Some("run") => run_daemon(),
        Some("stop") => daemon::stop(),
        Some("status") => daemon::status(),
        Some("restart") => {
            daemon::stop();
            daemon::start();
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            eprintln!("usage: nflow [start|run|stop|status|restart]");
            std::process::exit(1);
        }
    }
}

fn run_daemon() {
    env_logger::init();
    log::info!("nflow starting");

    if let Some(pid) = daemon::is_running() {
        if pid != std::process::id() as i32 {
            eprintln!("nflow is already running (pid {pid})");
            std::process::exit(1);
        }
    }
    if let Err(e) = daemon::write_pid(std::process::id() as i32) {
        log::warn!("failed to write pid file: {e}");
    }

    if !is_accessibility_enabled() {
        eprintln!("nflow requires accessibility permission.");
        eprintln!("Grant access in System Settings > Privacy & Security > Accessibility");
        std::process::exit(1);
    }

    let dir = config_dir();
    let config_path = ensure_config(&dir);

    let config = parse_config_file(&config_path).unwrap_or_else(|e| {
        log::warn!("failed to parse config, falling back to default: {e}");
        parse_config_str(DEFAULT_CONFIG).expect("default config must be valid")
    });

    let screen_width = get_screen_width();
    let profile = select_profile(&config, screen_width).expect("no matching profile");
    let lookup = app_layout_lookup_with_scene(profile, 0);
    let hide_titles = hide_titles_with_scene(profile, 0);

    let screen_rect = get_screen_rect();
    let (outer_gap, inner_gap) = effective_gaps(&config, profile);
    let bridge = MacOSBridge::new();
    let space_manager = SpaceManager::new(
        bridge,
        screen_rect,
        lookup,
        outer_gap,
        inner_gap,
        hide_titles,
    );

    let bindings = build_bindings(&config.hotkeys).expect("failed to build hotkey bindings");
    log::info!("built {} hotkey bindings", bindings.len());
    register_hotkeys(&bindings).expect("failed to register hotkeys");
    log::info!("hotkeys registered successfully");
    statusbar::update_shortcuts(accessibility_shortcuts(&config.hotkeys));

    register_screen_change_callback().expect("failed to register screen change callback");

    let (command_tx, command_rx) = mpsc::channel::<Command>();

    let menu_tx = command_tx.clone();
    set_command_callback(move |cmd| {
        let _ = command_tx.send(cmd);
    });

    let mut watcher = WindowWatcher::new(config.ignore.apps.clone());
    let (initial_windows, _) = watcher.poll();

    let app = Rc::new(RefCell::new(App {
        space_manager,
        watcher,
        config_path: config_path.clone(),
        config_mtime: file_mtime(&config_path),
        screen_width,
        command_rx,
        bridge_registry: BTreeMap::new(),
        last_frontmost_pid: None,
        terminal: config.terminal.clone(),
        active_scene: 0,
        scene_labels: scene_list(profile),
    }));

    {
        let mut app_ref = app.borrow_mut();
        for win in initial_windows {
            app_ref
                .space_manager
                .handle_window_created(win.window_id, &win.app_name, win.pid);
            app_ref.bridge_registry.insert(win.window_id, win.pid);
        }
        app_ref.space_manager.enforce_layout();
    }

    // The timer context holds a raw pointer to `app`. Validity depends on
    // `app` outliving the run loop. `NSApplication::run()` below never returns
    // (Quit calls `terminate`, which exits the process), so this is sound today;
    // if a clean-exit path is added, the timer must be invalidated before `app`
    // is dropped.
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info: &app as *const Rc<RefCell<App>> as *mut std::ffi::c_void,
        retain: None,
        release: None,
        copyDescription: None,
    };

    let fire_date = unsafe { CFAbsoluteTimeGetCurrent() } + 0.1;
    let timer = CFRunLoopTimer::new(fire_date, 0.5, 0, 0, timer_callback, &mut context);
    let run_loop = CFRunLoop::get_current();
    unsafe {
        run_loop.add_timer(&timer, kCFRunLoopDefaultMode);
    }

    let mtm = MainThreadMarker::new().expect("main must run on the main thread");
    let _status_bar = statusbar::install(mtm, menu_tx);

    log::info!("nflow running");
    unsafe {
        NSApplication::sharedApplication(mtm).run();
    }
}

fn frontmost_follow_target(
    last_pid: Option<i32>,
    frontmost_pid: i32,
    registry: &BTreeMap<u32, i32>,
) -> Option<u32> {
    if last_pid == Some(frontmost_pid) {
        return None;
    }
    registry
        .iter()
        .find(|(_, &p)| p == frontmost_pid)
        .map(|(&w, _)| w)
}

fn tick(app: &Rc<RefCell<App>>) {
    let mut app = app.borrow_mut();

    while let Ok(cmd) = app.command_rx.try_recv() {
        match cmd {
            Command::ApplyScene(number) if app.active_scene != number => {
                log::info!("applying scene {number}");
                app.active_scene = number;
                reload_config(&mut app);
            }
            Command::OpenConfig => {
                let config_path = app.config_path.clone();
                open_config_in_editor(app.terminal.as_deref(), &config_path);
            }
            Command::HintMode
            | Command::HintModeRightClick
            | Command::HintModeCopyLink
            | Command::TextSelect
            | Command::ScrollMode
            | Command::MenuSearch => {
                nflow::hotkey::run_mode_command(&cmd);
            }
            _ => {}
        }
    }

    let (new_windows, gone_pids) = app.watcher.poll();

    for pid in &gone_pids {
        let gone_wids: Vec<u32> = app
            .bridge_registry
            .iter()
            .filter(|(_, p)| *p == pid)
            .map(|(w, _)| *w)
            .collect();
        for wid in gone_wids {
            log::info!("window_destroyed: wid={wid} pid={pid}");
            app.bridge_registry.remove(&wid);
            app.space_manager.handle_window_destroyed(wid);
        }
    }

    let frontmost = frontmost_app();
    let frontmost_pid = frontmost.as_ref().map(|(pid, _)| *pid);

    for win in new_windows {
        app.bridge_registry.insert(win.window_id, win.pid);
        app.space_manager
            .handle_window_created(win.window_id, &win.app_name, win.pid);

        if frontmost_pid == Some(win.pid) {
            log::info!(
                "new window for frontmost app \"{}\" (wid={}), following to its space",
                win.app_name,
                win.window_id
            );
            app.space_manager.handle_focus_changed(win.window_id);
            app.last_frontmost_pid = Some(win.pid);
        }
    }

    if let Some((pid, name)) = frontmost {
        if let Some(focused_wid) =
            frontmost_follow_target(app.last_frontmost_pid, pid, &app.bridge_registry)
        {
            log::info!(
                "frontmost changed: {:?} -> {} (pid {})",
                app.last_frontmost_pid,
                name,
                pid
            );
            app.last_frontmost_pid = Some(pid);
            app.space_manager.handle_focus_changed(focused_wid);
        }
    }

    app.space_manager.enforce_layout();

    if check_screen_changed() {
        log::info!("screen changed, reloading");
        reload_config(&mut app);
    }

    let current_width = get_screen_width();
    if current_width != app.screen_width {
        log::info!(
            "screen width changed via poll: {} -> {}, reloading",
            app.screen_width,
            current_width
        );
        reload_config(&mut app);
    }

    let current_mtime = file_mtime(&app.config_path);
    if current_mtime != app.config_mtime {
        log::info!("config file changed, reloading");
        app.config_mtime = current_mtime;
        reload_config(&mut app);
    }

    statusbar::update_menu_state(
        app.space_manager.total_window_count(),
        app.space_manager.space_count(),
        app.space_manager.active_space(),
        app.active_scene,
        app.scene_labels.clone(),
    );
}

fn write_editor_wrapper(editor: &str, target: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let script_path = std::env::temp_dir().join("nflow-edit-config");
    let content = format!("#!/bin/sh\nexec {editor} \"{}\"\n", target.display());
    std::fs::write(&script_path, content)?;
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;
    Ok(script_path)
}

fn open_config_in_editor(terminal: Option<&str>, config_path: &Path) {
    let terminal = terminal.unwrap_or("Ghostty");
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());

    let wrapper = match write_editor_wrapper(&editor, config_path) {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(e) => {
            log::error!("failed to write editor wrapper: {e}");
            return;
        }
    };

    log::info!("opening config: terminal={terminal} editor={editor}");
    let result = std::process::Command::new("open")
        .args(["-na", terminal, "--args", "-e", &wrapper])
        .spawn();
    if let Err(e) = result {
        log::error!("failed to open config in terminal: {e}");
    }
}

fn reload_config(app: &mut App) {
    let config = match parse_config_file(&app.config_path) {
        Ok(c) => c,
        Err(e) => {
            log::error!("failed to parse config on reload: {e}");
            return;
        }
    };

    let screen_rect = get_screen_rect();
    let screen_width = screen_rect.width as u32;
    app.screen_width = screen_width;

    let profile = match select_profile(&config, screen_width) {
        Some(p) => p,
        None => {
            log::error!("no matching profile for screen width {screen_width}");
            return;
        }
    };

    let lookup = app_layout_lookup_with_scene(profile, app.active_scene);
    let hide_titles = hide_titles_with_scene(profile, app.active_scene);
    app.scene_labels = scene_list(profile);
    let (outer_gap, inner_gap) = effective_gaps(&config, profile);
    app.space_manager
        .reload_config(lookup, screen_rect, outer_gap, inner_gap, hide_titles);
    app.terminal = config.terminal.clone();
    app.watcher.set_ignored_apps(config.ignore.apps.clone());

    match build_bindings(&config.hotkeys) {
        Ok(bindings) => {
            if let Err(e) = register_hotkeys(&bindings) {
                log::error!("failed to re-register hotkeys: {e}");
            } else {
                statusbar::update_shortcuts(accessibility_shortcuts(&config.hotkeys));
            }
        }
        Err(e) => {
            log::error!("failed to build hotkey bindings on reload: {e}");
        }
    }
}

fn accessibility_shortcuts(hotkeys: &HotkeyConfig) -> Vec<statusbar::MenuShortcutEntry> {
    let entries = [
        ("Hint click", &hotkeys.hint_mode, Command::HintMode),
        (
            "Hint right-click",
            &hotkeys.hint_mode_right_click,
            Command::HintModeRightClick,
        ),
        (
            "Copy link",
            &hotkeys.hint_mode_copy_link,
            Command::HintModeCopyLink,
        ),
        ("Text select", &hotkeys.text_select, Command::TextSelect),
        ("Scroll mode", &hotkeys.scroll_mode, Command::ScrollMode),
        ("Menu search", &hotkeys.menu_search, Command::MenuSearch),
    ];
    entries
        .into_iter()
        .filter_map(|(title, pattern, command)| {
            pattern.as_ref().map(|p| statusbar::MenuShortcutEntry {
                title: title.to_string(),
                pattern: p.clone(),
                command,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{accessibility_shortcuts, frontmost_follow_target, Command, HotkeyConfig};
    use std::collections::BTreeMap;

    #[test]
    fn accessibility_shortcuts_lists_only_configured_modes() {
        let hotkeys = HotkeyConfig {
            apply_scene: Some("alt-ctrl-{n}".to_string()),
            hint_mode: Some("alt-cmd-shift-/".to_string()),
            scroll_mode: Some("cmd-shift-i".to_string()),
            menu_search: Some("alt-cmd-shift-p".to_string()),
            ..HotkeyConfig::default()
        };
        let shortcuts = accessibility_shortcuts(&hotkeys);
        assert_eq!(
            shortcuts,
            vec![
                nflow::statusbar::MenuShortcutEntry {
                    title: "Hint click".to_string(),
                    pattern: "alt-cmd-shift-/".to_string(),
                    command: Command::HintMode,
                },
                nflow::statusbar::MenuShortcutEntry {
                    title: "Scroll mode".to_string(),
                    pattern: "cmd-shift-i".to_string(),
                    command: Command::ScrollMode,
                },
                nflow::statusbar::MenuShortcutEntry {
                    title: "Menu search".to_string(),
                    pattern: "alt-cmd-shift-p".to_string(),
                    command: Command::MenuSearch,
                },
            ]
        );
    }

    #[test]
    fn accessibility_shortcuts_empty_config_is_empty() {
        assert!(accessibility_shortcuts(&HotkeyConfig::default()).is_empty());
    }

    #[test]
    fn no_follow_when_frontmost_unchanged() {
        let registry = BTreeMap::from([(10u32, 5i32)]);
        assert_eq!(frontmost_follow_target(Some(5), 5, &registry), None);
    }

    #[test]
    fn no_follow_when_window_not_registered_yet() {
        let registry = BTreeMap::new();
        assert_eq!(frontmost_follow_target(None, 5, &registry), None);
    }

    #[test]
    fn follows_registered_window_on_change() {
        let registry = BTreeMap::from([(10u32, 5i32)]);
        assert_eq!(frontmost_follow_target(None, 5, &registry), Some(10));
    }

    #[test]
    fn pending_transition_survives_until_window_registers() {
        let mut last_pid = None;

        let registry_before = BTreeMap::new();
        let target = frontmost_follow_target(last_pid, 5, &registry_before);
        assert_eq!(target, None);
        if target.is_some() {
            last_pid = Some(5);
        }
        assert_eq!(last_pid, None);

        let registry_after = BTreeMap::from([(10u32, 5i32)]);
        let target = frontmost_follow_target(last_pid, 5, &registry_after);
        assert_eq!(target, Some(10));
    }
}
