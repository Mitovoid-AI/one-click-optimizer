//! Small per-user settings file: %LOCALAPPDATA%\OneClickOptimizer\settings.json.

use crate::util::data_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Release the user chose to skip in the update prompt.
    #[serde(default)]
    pub skip_version: Option<String>,
    /// One-click Optimise runs used without Pro.
    #[serde(default)]
    pub free_runs_used: u32,
    /// Scheduled clean-up (Pro).
    #[serde(default)]
    pub autopilot: crate::autopilot::Schedule,
    /// Programs you chose to keep running ("Never close this" in History):
    /// never ticked for one-click Optimise and never closed by Autopilot.
    /// Keys come from `optimize::keep_key`.
    #[serde(default)]
    pub keep_running: Vec<String>,
}

fn path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn load() -> Settings {
    std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(s: &Settings) {
    if let Ok(bytes) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(path(), bytes);
    }
}

/// Load, change, save.
pub fn update(f: impl FnOnce(&mut Settings)) {
    let mut s = load();
    f(&mut s);
    save(&s);
}
