//! Process-wide application state managed by Tauri.

use crate::audio::Recorder;
use crate::controller;
use crate::history::History;
use crate::hotkey::HotkeyShared;
use crate::local_asr::{LocalAsr, ModelPaths};
use crate::models;
use crate::settings::Settings;
use crossbeam_channel::Sender;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct AppState {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub settings: RwLock<Settings>,
    /// In-memory copy of the Keychain secret (never serialized).
    pub api_key: RwLock<Option<String>>,
    pub local: RwLock<Arc<LocalAsr>>,
    pub local_loading: AtomicBool,
    pub downloading: AtomicBool,
    pub http: RwLock<reqwest::Client>,
    pub rt: tokio::runtime::Runtime,
    pub recorder: Recorder,
    pub hotkey: Arc<HotkeyShared>,
    pub history: Option<History>,
    pub phase: Mutex<&'static str>,
    #[allow(dead_code)]
    pub controller_tx: Sender<controller::Event>,
}

impl AppState {
    pub fn set_phase_label(&self, l: &'static str) {
        *self.phase.lock() = l;
    }

    pub fn model_paths(&self) -> ModelPaths {
        let roots = models::search_roots(&self.data_dir);
        let wants_small = self.settings.read().preview_model == "small";
        let preview_dir = if wants_small {
            models::resolve_small_preview(&roots).or_else(|| models::resolve(&models::PREVIEW_LARGE, &roots))
        } else {
            models::resolve(&models::PREVIEW_LARGE, &roots).or_else(|| models::resolve_small_preview(&roots))
        };
        ModelPaths {
            preview_dir,
            final_dir: models::resolve(&models::FINAL_SENSEVOICE, &roots),
            vad_file: models::resolve(&models::VAD, &roots).map(|d| d.join("silero_vad.onnx")),
        }
    }
}
