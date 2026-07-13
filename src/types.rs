use std::fmt;

pub type WindowId = u32;
pub type SpaceId = usize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone)]
pub struct Column {
    pub windows: Vec<WindowId>,
    pub weight: f64,
}

impl Column {
    pub fn new(window: WindowId) -> Self {
        Self {
            windows: vec![window],
            weight: 1.0,
        }
    }

    pub fn with_weight(window: WindowId, weight: f64) -> Self {
        Self {
            windows: vec![window],
            weight,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LayoutTree {
    pub columns: Vec<Column>,
}

impl LayoutTree {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
        }
    }

    pub fn add_column(&mut self, window: WindowId) {
        self.columns.push(Column::new(window));
    }

    pub fn add_to_column(&mut self, col_index: usize, window: WindowId) {
        if let Some(col) = self.columns.get_mut(col_index) {
            col.windows.push(window);
        }
    }

    pub fn remove_window(&mut self, window: WindowId) -> bool {
        for col in &mut self.columns {
            if let Some(pos) = col.windows.iter().position(|&w| w == window) {
                col.windows.remove(pos);
                return true;
            }
        }
        false
    }

    pub fn remove_empty_columns(&mut self) {
        self.columns.retain(|col| !col.windows.is_empty());
    }

    pub fn find_window(&self, window: WindowId) -> Option<(usize, usize)> {
        for (col_idx, col) in self.columns.iter().enumerate() {
            for (win_idx, &w) in col.windows.iter().enumerate() {
                if w == window {
                    return Some((col_idx, win_idx));
                }
            }
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub fn window_count(&self) -> usize {
        self.columns.iter().map(|c| c.windows.len()).sum()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    ApplyScene(usize),
    OpenConfig,
    HintMode,
    HintModeRightClick,
    HintModeCopyLink,
    TextSelect,
    ScrollMode,
    MenuSearch,
}

#[derive(Debug)]
pub enum NflowError {
    AccessibilityNotEnabled,
    ConfigParse(String),
    ConfigWatch(String),
    AxError(i32),
    HotkeyRegistration(String),
    ScreenDetection(String),
}

impl fmt::Display for nflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessibilityNotEnabled => {
                write!(f, "accessibility not enabled: grant permission in System Settings > Privacy & Security > Accessibility")
            }
            Self::ConfigParse(msg) => write!(f, "config parse error: {msg}"),
            Self::ConfigWatch(msg) => write!(f, "config watch error: {msg}"),
            Self::AxError(code) => write!(f, "accessibility error: code {code}"),
            Self::HotkeyRegistration(msg) => write!(f, "hotkey registration failed: {msg}"),
            Self::ScreenDetection(msg) => write!(f, "screen detection failed: {msg}"),
        }
    }
}

impl std::error::Error for NflowError {}

pub type Result<T> = std::result::Result<T, NflowError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_column_defaults_to_weight_one() {
        let col = Column::new(42);
        assert_eq!(col.weight, 1.0);
    }
}
