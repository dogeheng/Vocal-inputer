//! Commands invoked from the webview. The API key never crosses this
//! boundary in the outbound direction.

use crate::history::HistoryEntry;
use crate::local_asr::LocalAsr;
use crate::models::{self, ModelStatus};
use crate::settings::Settings;
use crate::state::AppState;
use crate::{cloud, macos, secrets};
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatus {
    pub preview: bool,
    pub final_model: bool,
    pub vad: bool,
    pub loading: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub accessibility: bool,
    pub microphone: bool,
    pub hotkey_active: bool,
    pub secure_input: bool,
    pub models: Vec<ModelStatus>,
    pub local: LocalStatus,
    pub downloading: bool,
    pub api_key_set: bool,
    pub key_hint: Option<String>,
    pub phase: String,
    pub version: String,
}

pub fn reload_local(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        if state.local_loading.swap(true, Ordering::SeqCst) {
            return;
        }
        let paths = state.model_paths();
        let asr = LocalAsr::load(&paths);
        *state.local.write() = Arc::new(asr);
        state.local_loading.store(false, Ordering::SeqCst);
        let _ = app.emit("models://loaded", ());
    });
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.read().clone()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    let new = settings.sanitized();
    let old = state.settings.read().clone();
    new.save(&state.config_dir).map_err(|e| format!("保存失败：{e}"))?;
    *state.settings.write() = new.clone();
    state.hotkey.set_trigger(new.hotkey);
    if old.proxy_url != new.proxy_url {
        *state.http.write() = cloud::http_client(&new.proxy_url);
    }
    if old.preview_model != new.preview_model {
        reload_local(&app);
    }
    if !new.save_history {
        // Turning history off also wipes what was stored.
        if let Some(h) = &state.history {
            h.clear();
        }
    }
    Ok(new)
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Result<Status, String> {
    let microphone = tauri_plugin_macos_permissions::check_microphone_permission().await;
    let local = state.local.read().clone();
    let key = state.api_key.read().clone();
    Ok(Status {
        accessibility: macos::accessibility_trusted(),
        microphone,
        hotkey_active: state.hotkey.tap_ok.load(Ordering::SeqCst),
        secure_input: macos::secure_input_enabled(),
        models: models::status(&state.data_dir),
        local: LocalStatus {
            preview: local.has_preview(),
            final_model: local.has_final(),
            vad: local.has_vad(),
            loading: state.local_loading.load(Ordering::SeqCst),
        },
        downloading: state.downloading.load(Ordering::SeqCst),
        api_key_set: key.is_some(),
        key_hint: key.as_deref().map(secrets::key_hint),
        phase: state.phase.lock().to_string(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

#[tauri::command]
pub async fn request_microphone() -> Result<bool, String> {
    if !tauri_plugin_macos_permissions::check_microphone_permission().await {
        let _ = tauri_plugin_macos_permissions::request_microphone_permission().await;
    }
    let ok = tauri_plugin_macos_permissions::check_microphone_permission().await;
    if !ok {
        macos::open_privacy_pane("microphone");
    }
    Ok(ok)
}

#[tauri::command]
pub async fn request_accessibility() -> Result<bool, String> {
    if !macos::accessibility_trusted() {
        tauri_plugin_macos_permissions::request_accessibility_permission().await;
        macos::open_privacy_pane("accessibility");
    }
    Ok(macos::accessibility_trusted())
}

#[tauri::command]
pub fn open_privacy_pane(kind: String) {
    macos::open_privacy_pane(&kind);
}

#[tauri::command]
pub async fn set_api_key(state: State<'_, AppState>, key: String) -> Result<String, String> {
    let key = key.trim().to_string();
    let (client, base) = (state.http.read().clone(), state.settings.read().cloud_base_url.clone());
    let summary = cloud::check_key(&client, &base, &key).await?;
    secrets::set_openrouter_key(&key)?;
    *state.api_key.write() = Some(key);
    Ok(summary)
}

#[tauri::command]
pub async fn clear_api_key(state: State<'_, AppState>) -> Result<(), String> {
    secrets::clear_openrouter_key()?;
    *state.api_key.write() = None;
    Ok(())
}

#[tauri::command]
pub async fn test_cloud(state: State<'_, AppState>) -> Result<String, String> {
    let key = state.api_key.read().clone().ok_or("尚未填写 API Key")?;
    let (client, base) = (state.http.read().clone(), state.settings.read().cloud_base_url.clone());
    cloud::check_key(&client, &base, &key).await
}

#[tauri::command]
pub fn download_models(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.downloading.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let client = state.http.read().clone();
    let data_dir = state.data_dir.clone();
    let app2 = app.clone();
    state.rt.spawn(async move {
        let a = app2.clone();
        let r = models::download_missing(&client, &data_dir, move |p| {
            let _ = a.emit("models://progress", p);
        })
        .await;
        let st = app2.state::<AppState>();
        st.downloading.store(false, Ordering::SeqCst);
        let _ = app2.emit("models://done", serde_json::json!({ "ok": r.is_ok(), "error": r.err() }));
        reload_local(&app2);
    });
    Ok(())
}

#[tauri::command]
pub fn list_history(state: State<'_, AppState>, limit: Option<u32>) -> Vec<HistoryEntry> {
    state.history.as_ref().map(|h| h.list(limit.unwrap_or(200))).unwrap_or_default()
}

#[tauri::command]
pub fn delete_history(state: State<'_, AppState>, id: i64) {
    if let Some(h) = &state.history {
        h.delete(id);
    }
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) {
    if let Some(h) = &state.history {
        h.clear();
    }
}

#[tauri::command]
pub fn copy_text(text: String) {
    macos::clipboard_set_text(&text);
}

#[tauri::command]
pub fn finish_onboarding(state: State<'_, AppState>) -> Result<(), String> {
    let mut s = state.settings.write();
    s.onboarding_done = true;
    s.save(&state.config_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_paused(state: State<'_, AppState>, paused: bool) {
    state.hotkey.paused.store(paused, Ordering::SeqCst);
}
