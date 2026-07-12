use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::tiling::compute_layout_with_gaps;
use crate::types::{Column, LayoutTree, Rect, Result, SpaceId, WindowId};

pub trait WindowBridge {
    fn register_window(&mut self, window_id: WindowId, pid: i32);
    fn apply_frame(&mut self, window_id: WindowId, frame: Rect) -> Result<()>;
    fn hide(&mut self, window_id: WindowId) -> Result<()>;
    fn focus(&mut self, window_id: WindowId) -> Result<()>;
    fn apply_frame_hiding(
        &mut self,
        window_id: WindowId,
        frame: Rect,
        hide_titles: &[String],
    ) -> Result<()>;
}

pub struct SpaceManager<B: WindowBridge> {
    bridge: B,
    screen_rect: Rect,
    outer_gap: f64,
    inner_gap: f64,
    spaces: HashMap<SpaceId, LayoutTree>,
    active_space: SpaceId,
    window_to_space: HashMap<WindowId, SpaceId>,
    window_to_app: HashMap<WindowId, String>,
    per_space_focus: HashMap<SpaceId, WindowId>,
    app_layout: BTreeMap<String, (SpaceId, usize, f64)>,
    config_spaces: BTreeSet<SpaceId>,
    hide_titles: Vec<String>,
}

impl<B: WindowBridge> SpaceManager<B> {
    pub fn new(
        bridge: B,
        screen_rect: Rect,
        app_layout: BTreeMap<String, (SpaceId, usize, f64)>,
        outer_gap: f64,
        inner_gap: f64,
        hide_titles: Vec<String>,
    ) -> Self {
        let config_spaces: BTreeSet<SpaceId> = app_layout.values().map(|&(s, _, _)| s).collect();

        let mut spaces = HashMap::new();
        for &space_id in &config_spaces {
            spaces.insert(space_id, LayoutTree::new());
        }

        let active_space = config_spaces.iter().next().copied().unwrap_or(1);
        spaces.entry(active_space).or_default();

        Self {
            bridge,
            screen_rect,
            outer_gap,
            inner_gap,
            spaces,
            active_space,
            window_to_space: HashMap::new(),
            window_to_app: HashMap::new(),
            per_space_focus: HashMap::new(),
            app_layout,
            config_spaces,
            hide_titles,
        }
    }

    pub fn total_window_count(&self) -> usize {
        self.window_to_space.len()
    }

    pub fn space_count(&self) -> usize {
        self.spaces.len()
    }

    pub fn active_space(&self) -> SpaceId {
        self.active_space
    }

    fn next_free_space(&self) -> SpaceId {
        let mut candidate: SpaceId = 1;
        loop {
            if !self.config_spaces.contains(&candidate) && !self.spaces.contains_key(&candidate) {
                return candidate;
            }
            candidate += 1;
        }
    }

    pub fn retile_active_space(&mut self) {
        if let Some(layout) = self.spaces.get(&self.active_space) {
            let frames =
                compute_layout_with_gaps(self.screen_rect, layout, self.outer_gap, self.inner_gap);
            log::debug!(
                "retile space {}: {} columns, {} windows, screen={}x{}",
                self.active_space,
                layout.columns.len(),
                layout.window_count(),
                self.screen_rect.width,
                self.screen_rect.height,
            );
            for (window_id, frame) in frames {
                log::info!(
                    "DIAG retile space {} wid={window_id} app={:?} -> frame={frame:?}",
                    self.active_space,
                    self.window_to_app.get(&window_id)
                );
                if !self.hide_titles.is_empty() {
                    let _ = self
                        .bridge
                        .apply_frame_hiding(window_id, frame, &self.hide_titles);
                } else {
                    let _ = self.bridge.apply_frame(window_id, frame);
                }
            }
        }
    }

    fn hide_space_windows(&mut self, space_id: SpaceId) {
        if let Some(layout) = self.spaces.get(&space_id) {
            let window_ids: Vec<WindowId> = layout
                .columns
                .iter()
                .flat_map(|col| col.windows.iter().copied())
                .collect();
            for wid in window_ids {
                let _ = self.bridge.hide(wid);
            }
        }
    }

    pub fn enforce_layout(&mut self) {
        let space_ids: Vec<SpaceId> = self.spaces.keys().copied().collect();
        for space_id in space_ids {
            if space_id != self.active_space {
                self.hide_space_windows(space_id);
            }
        }
        self.retile_active_space();
    }

    pub fn handle_window_created(&mut self, window_id: WindowId, app_name: &str, pid: i32) {
        self.bridge.register_window(window_id, pid);
        self.window_to_app.insert(window_id, app_name.to_string());

        let target_space = if let Some(&(space_id, _, _)) = self.app_layout.get(app_name) {
            space_id
        } else {
            self.next_free_space()
        };

        log::info!(
            "window_created: app=\"{app_name}\" wid={window_id} -> space {target_space} (active={})",
            self.active_space
        );

        self.spaces.entry(target_space).or_default();

        let weight = self
            .app_layout
            .get(app_name)
            .map(|&(_, _, w)| w)
            .unwrap_or(1.0);
        let idx = {
            let layout = self.spaces.get(&target_space).unwrap();
            self.target_column_index(target_space, app_name, layout)
        };
        let layout = self.spaces.get_mut(&target_space).unwrap();
        layout
            .columns
            .insert(idx, Column::with_weight(window_id, weight));

        self.window_to_space.insert(window_id, target_space);

        if target_space == self.active_space {
            self.retile_active_space();
        } else {
            let _ = self.bridge.hide(window_id);
        }
    }

    pub fn handle_window_destroyed(&mut self, window_id: WindowId) {
        let Some(space_id) = self.window_to_space.remove(&window_id) else {
            return;
        };

        self.window_to_app.remove(&window_id);
        self.remove_window_from_space(space_id, window_id);

        if space_id == self.active_space {
            self.retile_active_space();
        }
    }

    fn remove_window_from_space(&mut self, space_id: SpaceId, window_id: WindowId) {
        if let Some(layout) = self.spaces.get_mut(&space_id) {
            layout.remove_window(window_id);
            layout.remove_empty_columns();
        }

        if self.per_space_focus.get(&space_id) == Some(&window_id) {
            self.per_space_focus.remove(&space_id);
        }

        let is_empty = self.spaces.get(&space_id).is_none_or(|l| l.is_empty());
        if is_empty && !self.config_spaces.contains(&space_id) {
            self.spaces.remove(&space_id);
        }
    }

    pub fn handle_focus_changed(&mut self, window_id: WindowId) {
        if let Some(&space_id) = self.window_to_space.get(&window_id) {
            self.per_space_focus.insert(space_id, window_id);
            if space_id != self.active_space {
                log::info!("focus changed to window {window_id} on space {space_id}, switching from space {}", self.active_space);
                self.switch_space(space_id);
            }
        }
    }

    fn switch_space(&mut self, target: SpaceId) {
        if target == self.active_space {
            return;
        }

        self.hide_space_windows(self.active_space);

        self.active_space = target;
        self.spaces.entry(target).or_default();

        self.retile_active_space();

        if let Some(&wid) = self.per_space_focus.get(&target) {
            let _ = self.bridge.focus(wid);
        }
    }

    pub fn reload_config(
        &mut self,
        new_app_layout: BTreeMap<String, (SpaceId, usize, f64)>,
        screen_rect: Rect,
        outer_gap: f64,
        inner_gap: f64,
        hide_titles: Vec<String>,
    ) {
        self.screen_rect = screen_rect;
        self.outer_gap = outer_gap;
        self.inner_gap = inner_gap;
        self.app_layout = new_app_layout;
        self.hide_titles = hide_titles;
        self.config_spaces = self.app_layout.values().map(|&(s, _, _)| s).collect();

        for &space_id in &self.config_spaces {
            self.spaces.entry(space_id).or_default();
        }

        let all_windows: Vec<(WindowId, String)> = self
            .window_to_app
            .iter()
            .map(|(&wid, app)| (wid, app.clone()))
            .collect();

        for space_layout in self.spaces.values_mut() {
            *space_layout = LayoutTree::new();
        }
        self.window_to_space.clear();

        for (wid, app_name) in &all_windows {
            let target = self
                .app_layout
                .get(app_name)
                .map(|&(s, _, _)| s)
                .unwrap_or_else(|| self.next_free_space());

            let weight = self
                .app_layout
                .get(app_name)
                .map(|&(_, _, w)| w)
                .unwrap_or(1.0);

            self.spaces.entry(target).or_default();
            let layout = self.spaces.get_mut(&target).unwrap();
            let idx = Self::target_column_index_static(
                &self.app_layout,
                &self.window_to_app,
                target,
                app_name,
                layout,
            );
            let insert_at = idx.min(layout.columns.len());
            layout
                .columns
                .insert(insert_at, Column::with_weight(*wid, weight));
            self.window_to_space.insert(*wid, target);
        }

        let empty_non_config: Vec<SpaceId> = self
            .spaces
            .iter()
            .filter(|(&sid, layout)| layout.is_empty() && !self.config_spaces.contains(&sid))
            .map(|(&sid, _)| sid)
            .collect();
        for sid in empty_non_config {
            self.spaces.remove(&sid);
        }

        self.retile_active_space();
    }

    fn target_column_index(&self, space_id: SpaceId, app_name: &str, layout: &LayoutTree) -> usize {
        Self::target_column_index_static(
            &self.app_layout,
            &self.window_to_app,
            space_id,
            app_name,
            layout,
        )
    }

    fn target_column_index_static(
        app_layout: &BTreeMap<String, (SpaceId, usize, f64)>,
        window_to_app: &HashMap<WindowId, String>,
        space_id: SpaceId,
        app_name: &str,
        layout: &LayoutTree,
    ) -> usize {
        let Some(&(_, target_idx, _)) = app_layout.get(app_name) else {
            return layout.columns.len();
        };
        layout
            .columns
            .iter()
            .take_while(|col| {
                col.windows
                    .first()
                    .and_then(|w| window_to_app.get(w))
                    .and_then(|other| app_layout.get(other))
                    .filter(|(s, _, _)| *s == space_id)
                    .map(|(_, idx, _)| *idx < target_idx)
                    .unwrap_or(true)
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Debug, Clone, PartialEq)]
    enum MockCall {
        ApplyFrame(WindowId, Rect),
        Hide(WindowId),
        Focus(WindowId),
        ApplyFrameHiding(WindowId, Rect, Vec<String>),
    }

    struct MockBridge {
        calls: RefCell<Vec<MockCall>>,
    }

    impl MockBridge {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }

        fn take_calls(&self) -> Vec<MockCall> {
            self.calls.borrow_mut().drain(..).collect()
        }
    }

    impl WindowBridge for MockBridge {
        fn register_window(&mut self, _window_id: WindowId, _pid: i32) {}

        fn apply_frame(&mut self, window_id: WindowId, frame: Rect) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(MockCall::ApplyFrame(window_id, frame));
            Ok(())
        }

        fn hide(&mut self, window_id: WindowId) -> Result<()> {
            self.calls.borrow_mut().push(MockCall::Hide(window_id));
            Ok(())
        }

        fn focus(&mut self, window_id: WindowId) -> Result<()> {
            self.calls.borrow_mut().push(MockCall::Focus(window_id));
            Ok(())
        }

        fn apply_frame_hiding(
            &mut self,
            window_id: WindowId,
            frame: Rect,
            hide_titles: &[String],
        ) -> Result<()> {
            self.calls.borrow_mut().push(MockCall::ApplyFrameHiding(
                window_id,
                frame,
                hide_titles.to_vec(),
            ));
            Ok(())
        }
    }

    fn screen() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        }
    }

    fn lookup_zen_ghostty() -> BTreeMap<String, (SpaceId, usize, f64)> {
        let mut m = BTreeMap::new();
        m.insert("Zen Browser".into(), (1usize, 0usize, 1.0));
        m.insert("Ghostty".into(), (1, 1, 1.0));
        m.insert("Slack".into(), (2, 0, 1.0));
        m
    }

    fn make_manager() -> SpaceManager<MockBridge> {
        SpaceManager::new(
            MockBridge::new(),
            screen(),
            lookup_zen_ghostty(),
            0.0,
            0.0,
            Vec::new(),
        )
    }

    fn make_manager_with_hide_titles(titles: &[&str]) -> SpaceManager<MockBridge> {
        SpaceManager::new(
            MockBridge::new(),
            screen(),
            lookup_zen_ghostty(),
            0.0,
            0.0,
            titles.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn configured_app_assigned_to_correct_space() {
        let mut mgr = make_manager();
        mgr.handle_window_created(100, "Zen Browser", 0);
        assert_eq!(mgr.window_to_space[&100], 1);
    }

    #[test]
    fn second_configured_app_same_space() {
        let mut mgr = make_manager();
        mgr.handle_window_created(100, "Zen Browser", 0);
        mgr.handle_window_created(101, "Ghostty", 0);
        assert_eq!(mgr.window_to_space[&100], 1);
        assert_eq!(mgr.window_to_space[&101], 1);
    }

    #[test]
    fn unlisted_app_gets_new_space() {
        let mut mgr = make_manager();
        mgr.handle_window_created(200, "Firefox", 0);
        let space = mgr.window_to_space[&200];
        assert!(!mgr.config_spaces.contains(&space));
    }

    #[test]
    fn two_unlisted_apps_get_different_spaces() {
        let mut mgr = make_manager();
        mgr.handle_window_created(200, "Firefox", 0);
        mgr.handle_window_created(201, "Safari", 0);
        let s1 = mgr.window_to_space[&200];
        let s2 = mgr.window_to_space[&201];
        assert_ne!(s1, s2);
    }

    #[test]
    fn window_on_active_space_gets_tiled() {
        let mut mgr = make_manager();
        mgr.handle_window_created(100, "Zen Browser", 0);
        let calls = mgr.bridge.take_calls();
        assert!(calls
            .iter()
            .any(|c| matches!(c, MockCall::ApplyFrame(100, _))));
    }

    #[test]
    fn window_on_inactive_space_gets_hidden() {
        let mut mgr = make_manager();
        mgr.handle_window_created(200, "Slack", 0);
        let calls = mgr.bridge.take_calls();
        assert!(calls.iter().any(|c| matches!(c, MockCall::Hide(200))));
        assert!(!calls
            .iter()
            .any(|c| matches!(c, MockCall::ApplyFrame(200, _))));
    }

    #[test]
    fn window_destroyed_removes_from_space() {
        let mut mgr = make_manager();
        mgr.handle_window_created(100, "Zen Browser", 0);
        mgr.handle_window_destroyed(100);
        assert!(!mgr.window_to_space.contains_key(&100));
        let layout = &mgr.spaces[&1];
        assert!(layout.find_window(100).is_none());
    }

    #[test]
    fn empty_auto_space_reclaimed() {
        let mut mgr = make_manager();
        mgr.handle_window_created(200, "Firefox", 0);
        let space = mgr.window_to_space[&200];
        mgr.handle_window_destroyed(200);
        assert!(!mgr.spaces.contains_key(&space));
    }

    #[test]
    fn configured_space_not_reclaimed_when_empty() {
        let mut mgr = make_manager();
        mgr.handle_window_created(100, "Zen Browser", 0);
        mgr.handle_window_destroyed(100);
        assert!(mgr.spaces.contains_key(&1));
    }

    #[test]
    fn focus_tracking_per_space() {
        let mut mgr = make_manager();
        mgr.handle_window_created(100, "Zen Browser", 0);
        mgr.handle_window_created(101, "Ghostty", 0);
        mgr.handle_focus_changed(101);

        mgr.handle_window_created(200, "Slack", 0);
        mgr.handle_focus_changed(200);

        mgr.bridge.take_calls();
        mgr.handle_focus_changed(101);

        let calls = mgr.bridge.take_calls();
        assert!(calls.iter().any(|c| matches!(c, MockCall::Focus(101))));
    }

    #[test]
    fn every_window_in_exactly_one_space() {
        let mut mgr = make_manager();
        mgr.handle_window_created(100, "Zen Browser", 0);
        mgr.handle_window_created(101, "Ghostty", 0);
        mgr.handle_window_created(200, "Slack", 0);
        mgr.handle_window_created(300, "Firefox", 0);
        mgr.handle_window_created(102, "Zen Browser", 0);
        mgr.handle_window_destroyed(300);

        let mut seen: HashMap<WindowId, usize> = HashMap::new();
        for (space_id, layout) in &mgr.spaces {
            for col in &layout.columns {
                for &wid in &col.windows {
                    *seen.entry(wid).or_insert(0) += 1;
                    assert_eq!(
                        mgr.window_to_space[&wid], *space_id,
                        "window_to_space mismatch for window {wid}"
                    );
                }
            }
        }

        for (&wid, &count) in &seen {
            assert_eq!(count, 1, "window {wid} appears in {count} spaces");
        }

        for (&wid, &space_id) in &mgr.window_to_space {
            assert!(
                mgr.spaces
                    .get(&space_id)
                    .is_some_and(|l| l.find_window(wid).is_some()),
                "window {wid} tracked in space {space_id} but not in layout"
            );
        }
    }

    #[test]
    fn hide_titles_route_active_window_through_hiding_path() {
        let mut mgr = make_manager_with_hide_titles(&["Calendar"]);
        mgr.handle_window_created(100, "Zen Browser", 0);
        let calls = mgr.bridge.take_calls();
        assert!(calls.iter().any(|c| matches!(
            c,
            MockCall::ApplyFrameHiding(100, _, titles) if titles == &["Calendar".to_string()]
        )));
        assert!(!calls
            .iter()
            .any(|c| matches!(c, MockCall::ApplyFrame(100, _))));
    }

    #[test]
    fn without_hide_titles_uses_plain_apply_frame() {
        let mut mgr = make_manager();
        mgr.handle_window_created(100, "Zen Browser", 0);
        let calls = mgr.bridge.take_calls();
        assert!(calls
            .iter()
            .any(|c| matches!(c, MockCall::ApplyFrame(100, _))));
        assert!(!calls
            .iter()
            .any(|c| matches!(c, MockCall::ApplyFrameHiding(..))));
    }

    #[test]
    fn hide_titles_do_not_apply_to_inactive_space() {
        let mut mgr = make_manager_with_hide_titles(&["Calendar"]);
        mgr.handle_window_created(200, "Slack", 0);
        let calls = mgr.bridge.take_calls();
        assert!(!calls
            .iter()
            .any(|c| matches!(c, MockCall::ApplyFrameHiding(..))));
        assert!(calls.iter().any(|c| matches!(c, MockCall::Hide(200))));
    }

    #[test]
    fn reload_config_reassigns_windows() {
        let mut mgr = make_manager();
        mgr.handle_window_created(100, "Zen Browser", 0);
        mgr.handle_window_created(200, "Slack", 0);
        assert_eq!(mgr.window_to_space[&100], 1);
        assert_eq!(mgr.window_to_space[&200], 2);

        let mut new_lookup = BTreeMap::new();
        new_lookup.insert("Zen Browser".into(), (3usize, 0usize, 1.0));
        new_lookup.insert("Slack".into(), (3, 1, 1.0));

        mgr.reload_config(new_lookup, screen(), 0.0, 0.0, Vec::new());

        assert_eq!(mgr.window_to_space[&100], 3);
        assert_eq!(mgr.window_to_space[&200], 3);
    }

    #[test]
    fn reload_config_reroutes_app_dropped_from_config_to_free_space() {
        let mut mgr = make_manager();
        mgr.handle_window_created(100, "Zen Browser", 0);
        mgr.handle_window_created(200, "Slack", 0);

        let mut new_lookup = BTreeMap::new();
        new_lookup.insert("Zen Browser".into(), (1usize, 0usize, 1.0));

        mgr.reload_config(new_lookup, screen(), 0.0, 0.0, Vec::new());

        assert_eq!(mgr.window_to_space[&100], 1);
        let slack_space = mgr.window_to_space[&200];
        assert!(!mgr.config_spaces.contains(&slack_space));
        assert_ne!(slack_space, 1);
    }

    #[test]
    fn columns_inserted_in_config_order_regardless_of_spawn_order() {
        let mut app_layout = BTreeMap::new();
        app_layout.insert("Zen".to_string(), (1usize, 0usize, 1.0));
        app_layout.insert("Ghostty".to_string(), (1, 1, 2.0));
        app_layout.insert("Slack".to_string(), (1, 2, 1.0));

        let bridge = MockBridge::new();
        let mut mgr = SpaceManager::new(
            bridge,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 4000.0,
                height: 1000.0,
            },
            app_layout,
            0.0,
            0.0,
            Vec::new(),
        );

        mgr.handle_window_created(101, "Slack", 1);
        mgr.handle_window_created(102, "Zen", 2);
        mgr.handle_window_created(103, "Ghostty", 3);

        let layout = &mgr.spaces[&1];
        assert_eq!(layout.columns[0].windows, vec![102]);
        assert_eq!(layout.columns[1].windows, vec![103]);
        assert_eq!(layout.columns[2].windows, vec![101]);
        assert!((layout.columns[1].weight - 2.0).abs() < 1e-9);
        assert!((layout.columns[0].weight - 1.0).abs() < 1e-9);
        assert!((layout.columns[2].weight - 1.0).abs() < 1e-9);
    }

    #[test]
    fn reload_reorders_columns_and_restamps_weights() {
        let mut initial = BTreeMap::new();
        initial.insert("Zen".to_string(), (1usize, 0usize, 1.0));
        initial.insert("Ghostty".to_string(), (1, 1, 1.0));
        initial.insert("Slack".to_string(), (1, 2, 1.0));

        let mut mgr = SpaceManager::new(
            MockBridge::new(),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 4000.0,
                height: 1000.0,
            },
            initial,
            0.0,
            0.0,
            Vec::new(),
        );

        mgr.handle_window_created(201, "Slack", 1);
        mgr.handle_window_created(202, "Zen", 2);
        mgr.handle_window_created(203, "Ghostty", 3);

        let mut reloaded = BTreeMap::new();
        reloaded.insert("Zen".to_string(), (1usize, 0usize, 1.0));
        reloaded.insert("Ghostty".to_string(), (1, 1, 2.0));
        reloaded.insert("Slack".to_string(), (1, 2, 1.0));

        mgr.reload_config(
            reloaded,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 4000.0,
                height: 1000.0,
            },
            0.0,
            0.0,
            Vec::new(),
        );

        let layout = &mgr.spaces[&1];
        assert_eq!(layout.columns[0].windows, vec![202]);
        assert_eq!(layout.columns[1].windows, vec![203]);
        assert_eq!(layout.columns[2].windows, vec![201]);
        assert!((layout.columns[0].weight - 1.0).abs() < 1e-9);
        assert!((layout.columns[1].weight - 2.0).abs() < 1e-9);
        assert!((layout.columns[2].weight - 1.0).abs() < 1e-9);
    }
}
