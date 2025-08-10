// snaplock/src-tauri/src/constants.rs

use std::time::Duration;

pub const PREPARATION_DELAY: Duration = Duration::from_secs(2);
pub const SHORTCUT_DEBOUNCE_TIME: Duration = Duration::from_millis(500);
pub const SHORTCUT_FLAG_CLEAR_DELAY: Duration = Duration::from_millis(1000);
pub const EVENT_IGNORE_WINDOW_MS: u64 = 500; // 减少事件忽略窗口从1000ms到500ms