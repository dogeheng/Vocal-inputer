//! VoiceInput - system-wide voice dictation for macOS.

pub mod audio;
pub mod cloud;
mod commands;
mod controller;
pub mod history;
pub mod hotkey;
pub mod insert;
pub mod local_asr;
pub mod macos;
pub mod models;
mod overlay;
pub mod pipeline;
pub mod secrets;
pub mod settings;
mod state;
pub mod text;

use parking_lot::{Mutex, RwLock};
use settings::Settings;
use state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{ActivationPolicy, AppHandle, Manager, WindowEvent};

pub fn show_main_window(app: &AppHandle) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        let _ = a.set_activation_policy(ActivationPolicy::Regular);
        if let Some(w) = a.get_webview_window("main") {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    });
}

fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开 VoiceInput…", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "暂停语音输入快捷键", true, false, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 VoiceInput", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(app, &[&open, &pause, &sep, &quit])?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))?;
    let pause_item = pause.clone();
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("VoiceInput")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "pause" => {
                let state = app.state::<AppState>();
                let paused = !state.hotkey.paused.load(Ordering::SeqCst);
                state.hotkey.paused.store(paused, Ordering::SeqCst);
                let _ = pause_item.set_checked(paused);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .level_for("tao", log::LevelFilter::Warn)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_macos_permissions::init())
        .plugin(tauri_nspanel::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let settings = Settings::load(&config_dir);
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("vi-net")
                .build()?;
            let history = history::History::open(&data_dir)
                .map_err(|e| log::error!("history unavailable: {e}"))
                .ok();
            if let Some(h) = &history {
                h.prune(settings.history_days);
            }
            let hotkey_shared = hotkey::HotkeyShared::new(settings.hotkey);
            let (ctrl_tx, ctrl_rx) = crossbeam_channel::unbounded();
            let onboarding_done = settings.onboarding_done;

            app.manage(AppState {
                config_dir,
                data_dir,
                http: RwLock::new(cloud::http_client(&settings.proxy_url)),
                settings: RwLock::new(settings),
                // Loaded on a background thread below: a Keychain access prompt
                // must never block the main thread.
                api_key: RwLock::new(None),
                local: RwLock::new(Arc::new(local_asr::LocalAsr::empty())),
                local_loading: AtomicBool::new(false),
                downloading: AtomicBool::new(false),
                rt,
                recorder: audio::Recorder::new(),
                hotkey: hotkey_shared.clone(),
                history,
                phase: Mutex::new("idle"),
                controller_tx: ctrl_tx.clone(),
            });

            {
                let h = handle.clone();
                std::thread::spawn(move || {
                    let key = secrets::openrouter_key();
                    log::info!("api key {}", if key.is_some() { "loaded from keychain" } else { "not configured" });
                    *h.state::<AppState>().api_key.write() = key;
                });
            }

            // Hotkey tap -> controller.
            let (htx, hrx) = crossbeam_channel::bounded(64);
            hotkey::start(htx, hotkey_shared);
            let fwd = ctrl_tx.clone();
            std::thread::spawn(move || {
                for ev in hrx {
                    let _ = fwd.send(controller::Event::Hotkey(ev));
                }
            });
            controller::spawn(handle.clone(), ctrl_rx, ctrl_tx);

            // Never let a cosmetic component take the whole app down.
            if let Err(e) = overlay::create(&handle) {
                log::error!("overlay unavailable: {e}");
            }
            if let Err(e) = create_tray(&handle) {
                log::error!("tray unavailable: {e}");
            }
            commands::reload_local(&handle);

            if onboarding_done {
                let _ = handle.set_activation_policy(ActivationPolicy::Accessory);
            } else {
                show_main_window(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                    let _ = window.app_handle().set_activation_policy(ActivationPolicy::Accessory);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::get_status,
            commands::request_microphone,
            commands::request_accessibility,
            commands::open_privacy_pane,
            commands::set_api_key,
            commands::clear_api_key,
            commands::test_cloud,
            commands::download_models,
            commands::list_history,
            commands::delete_history,
            commands::clear_history,
            commands::copy_text,
            commands::finish_onboarding,
            commands::set_paused,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VoiceInput");
}
