//! User settings persisted as JSON in the app config directory.
//!
//! Secrets (API keys) are NEVER stored here; see `secrets.rs`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyChoice {
    /// The Fn / Globe key (built-in Mac keyboards).
    Fn,
    RightOption,
    RightCommand,
    RightControl,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TriggerMode {
    /// Hold to talk; a short tap latches hands-free mode (press again to finish).
    Hybrid,
    /// Hold to talk only.
    Hold,
    /// Press once to start, press again to finish.
    Toggle,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CloudApiStyle {
    /// OpenRouter JSON body with base64 `input_audio`.
    OpenrouterJson,
    /// OpenAI-compatible multipart upload (`file` field).
    OpenaiMultipart,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Replacement {
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub onboarding_done: bool,
    pub hotkey: HotkeyChoice,
    pub trigger_mode: TriggerMode,
    /// Hold shorter than this (ms) counts as a tap in hybrid mode.
    pub tap_threshold_ms: u64,

    // ---- Recognition ----
    pub cloud_enabled: bool,
    pub cloud_base_url: String,
    pub cloud_api_style: CloudApiStyle,
    pub cloud_model: String,
    /// How long to wait for cloud results after the user stops speaking.
    pub cloud_deadline_ms: u64,
    /// Late cloud results are applied (safely) only within this window.
    pub late_upgrade_window_ms: u64,
    /// "auto" | "zh" | "en" | ...
    pub language: String,
    /// Empty = use system proxy settings.
    pub proxy_url: String,
    /// "large" (bilingual zipformer, default) | "small" (dev only, local cache)
    pub preview_model: String,

    // ---- Text processing ----
    pub cleanup_enabled: bool,
    pub cleanup_model: String,
    pub cleanup_timeout_ms: u64,
    pub cjk_spacing: bool,
    pub remove_fillers: bool,
    pub dictionary: Vec<String>,
    pub replacements: Vec<Replacement>,

    // ---- Privacy ----
    pub save_history: bool,
    pub history_days: u32,
    pub sound_feedback: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            onboarding_done: false,
            hotkey: HotkeyChoice::Fn,
            trigger_mode: TriggerMode::Hybrid,
            tap_threshold_ms: 350,
            cloud_enabled: true,
            cloud_base_url: "https://openrouter.ai/api/v1".into(),
            cloud_api_style: CloudApiStyle::OpenrouterJson,
            cloud_model: "mistralai/voxtral-mini-transcribe-3-0".into(),
            cloud_deadline_ms: 1000,
            late_upgrade_window_ms: 8000,
            language: "auto".into(),
            proxy_url: String::new(),
            preview_model: "large".into(),
            cleanup_enabled: true,
            cleanup_model: "openai/gpt-oss-20b".into(),
            cleanup_timeout_ms: 1200,
            cjk_spacing: true,
            remove_fillers: true,
            dictionary: vec![],
            replacements: vec![],
            save_history: true,
            history_days: 30,
            sound_feedback: false,
        }
    }
}

impl Settings {
    pub fn path(config_dir: &Path) -> PathBuf {
        config_dir.join("settings.json")
    }

    pub fn load(config_dir: &Path) -> Self {
        let p = Self::path(config_dir);
        match std::fs::read_to_string(&p) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                log::warn!("settings.json is invalid ({e}); using defaults");
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir)?;
        let tmp = config_dir.join("settings.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, Self::path(config_dir))
    }

    /// Clamp values coming from the UI to sane ranges.
    pub fn sanitized(mut self) -> Self {
        self.tap_threshold_ms = self.tap_threshold_ms.clamp(150, 800);
        self.cloud_deadline_ms = self.cloud_deadline_ms.clamp(200, 5000);
        self.late_upgrade_window_ms = self.late_upgrade_window_ms.clamp(0, 30000);
        self.cleanup_timeout_ms = self.cleanup_timeout_ms.clamp(200, 3000);
        self.history_days = self.history_days.clamp(1, 3650);
        self.cloud_base_url = self.cloud_base_url.trim().trim_end_matches('/').to_string();
        if self.cloud_base_url.is_empty() {
            self.cloud_base_url = Settings::default().cloud_base_url;
        }
        self.proxy_url = self.proxy_url.trim().to_string();
        self.dictionary = self
            .dictionary
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        self.replacements.retain(|r| !r.from.trim().is_empty());
        self
    }
}
