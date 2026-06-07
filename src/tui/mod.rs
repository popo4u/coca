mod app;
mod event;
mod render;
mod views;

pub mod formatting {
    use chrono::{DateTime, Local};

    pub fn format_time(timestamp_ms: Option<i64>) -> String {
        let Some(timestamp_ms) = timestamp_ms else {
            return "-".to_string();
        };
        let Some(dt) = DateTime::from_timestamp_millis(timestamp_ms) else {
            return "-".to_string();
        };
        dt.with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    }

    pub fn short_path(path: &str) -> String {
        let Some(home) = dirs::home_dir() else {
            return path.to_string();
        };
        let home = home.to_string_lossy();
        path.strip_prefix(home.as_ref())
            .map(|rest| format!("~{rest}"))
            .unwrap_or_else(|| path.to_string())
    }

    pub fn short_id(id: &str) -> String {
        id.chars().take(8).collect()
    }
}

pub use app::run_tui;
