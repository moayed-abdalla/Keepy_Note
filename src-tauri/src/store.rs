use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PinMode {
    AlwaysOnTop,
    DesktopEmbed,
}

impl Default for PinMode {
    fn default() -> Self {
        Self::AlwaysOnTop
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StickyTaskItem {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub position: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StickyNoteState {
    pub id: String,
    pub task_list_id: String,
    pub title: String,
    #[serde(default)]
    pub items: Vec<StickyTaskItem>,
    pub color: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub pin_mode: PinMode,
    /// Fingerprint of remote list state for change detection (e.g. joined updated timestamps).
    pub remote_updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub poll_interval_secs: u64,
    pub autostart: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            poll_interval_secs: 60,
            autostart: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppStore {
    pub stickies: Vec<StickyNoteState>,
    pub settings: AppSettings,
}

impl AppStore {
    pub fn load(path: &PathBuf) -> Self {
        let Ok(raw) = fs::read_to_string(path) else {
            return Self::default();
        };

        // Legacy stickies pinned individual tasks (task_id, notes). Clear them so users re-pin lists.
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
            let is_legacy = value
                .get("stickies")
                .and_then(|s| s.as_array())
                .map(|arr| {
                    arr.iter()
                        .any(|s| s.get("task_id").is_some() && s.get("items").is_none())
                })
                .unwrap_or(false);

            if is_legacy {
                let settings = value
                    .get("settings")
                    .and_then(|s| serde_json::from_value(s.clone()).ok())
                    .unwrap_or_default();
                let store = Self {
                    stickies: Vec::new(),
                    settings,
                };
                let _ = store.save(path);
                return store;
            }
        }

        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let raw = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &raw).map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| {
            // Fallback if rename fails on Windows (e.g. destination exists).
            let _ = fs::write(path, &raw);
            e.to_string()
        })?;
        Ok(())
    }
}

pub fn store_path(app_data_dir: PathBuf) -> PathBuf {
    app_data_dir.join("store.json")
}
