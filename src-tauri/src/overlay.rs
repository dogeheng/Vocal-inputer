//! Floating status indicator: a non-activating NSPanel that never steals
//! keyboard focus from the app the user is dictating into.

use serde_json::json;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Size, WebviewUrl};
use tauri_nspanel::{tauri_panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask};
use std::sync::atomic::{AtomicU64, Ordering};

pub const LABEL: &str = "overlay";
const WIDTH: f64 = 520.0;
const HEIGHT: f64 = 132.0;
const BOTTOM_MARGIN: f64 = 72.0;

static GENERATION: AtomicU64 = AtomicU64::new(0);

tauri_panel! {
    panel!(OverlayPanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let panel = PanelBuilder::<_, OverlayPanel>::new(app, LABEL)
        .url(WebviewUrl::App("overlay.html".into()))
        .title("VoiceInput Overlay")
        .size(Size::Logical(LogicalSize::new(WIDTH, HEIGHT)))
        .level(PanelLevel::Status)
        .floating(true)
        .has_shadow(false)
        .opaque(false)
        .transparent(true)
        .hides_on_deactivate(false)
        .ignores_mouse_events(true)
        .no_activate(true)
        .works_when_modal(true)
        // Create the native window borderless from the start; replacing the
        // style mask of a live WKWebView window makes AppKit throw (KVO on
        // contentLayoutRect). Only *add* the non-activating flag.
        .with_window(|w| {
            w.decorations(false)
                .resizable(false)
                .shadow(false)
                .skip_taskbar(true)
                .focused(false)
                .visible(false)
        })
        .add_style_mask(StyleMask::empty().nonactivating_panel())
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary()
                .stationary()
                .ignores_cycle(),
        )
        .build()?;
    panel.hide();
    Ok(())
}

/// Bottom-centre of the monitor that currently contains the mouse pointer.
fn reposition(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else { return };
    let cursor = app.cursor_position().ok();
    let monitors = app.available_monitors().unwrap_or_default();
    let monitor = cursor
        .and_then(|c| {
            monitors.iter().find(|m| {
                let p = m.position();
                let s = m.size();
                c.x >= p.x as f64 && c.x < (p.x + s.width as i32) as f64 && c.y >= p.y as f64 && c.y < (p.y + s.height as i32) as f64
            })
        })
        .cloned()
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let scale = m.scale_factor();
        let p = m.position();
        let s = m.size();
        let w = (WIDTH * scale) as i32;
        let h = (HEIGHT * scale) as i32;
        let x = p.x + (s.width as i32 - w) / 2;
        let y = p.y + s.height as i32 - h - (BOTTOM_MARGIN * scale) as i32;
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

fn show(app: &AppHandle) {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        reposition(&a);
        if let Ok(p) = a.get_webview_panel(LABEL) {
            p.order_front_regardless();
            p.show();
        }
    });
}

pub fn hide(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "overlay://state", json!({ "phase": "hidden" }));
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Ok(p) = a.get_webview_panel(LABEL) {
            p.hide();
        }
    });
}

fn hide_after(app: &AppHandle, ms: u64) {
    let gen = GENERATION.load(Ordering::SeqCst);
    let a = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        if GENERATION.load(Ordering::SeqCst) == gen {
            hide(&a);
        }
    });
}

fn emit(app: &AppHandle, payload: serde_json::Value) {
    let _ = app.emit_to(LABEL, "overlay://state", payload);
}

pub fn listening(app: &AppHandle, app_name: &str) {
    emit(app, json!({ "phase": "listening", "app": app_name, "text": "", "latched": false }));
    show(app);
}

pub fn latched(app: &AppHandle) {
    emit(app, json!({ "phase": "listening", "latched": true }));
}

pub fn partial(app: &AppHandle, text: &str) {
    let _ = app.emit_to(LABEL, "overlay://partial", text);
}

pub fn level(app: &AppHandle, level: f32) {
    let _ = app.emit_to(LABEL, "overlay://level", level);
}

pub fn processing(app: &AppHandle) {
    emit(app, json!({ "phase": "processing" }));
}

pub fn done(app: &AppHandle, text: &str, source: &str, cleaned: bool, hint: &str) {
    emit(app, json!({ "phase": "done", "text": text, "source": source, "cleaned": cleaned, "hint": hint }));
    hide_after(app, if hint.is_empty() { 1300 } else { 3200 });
}

pub fn notice(app: &AppHandle, text: &str, hint: &str) {
    emit(app, json!({ "phase": "notice", "text": text, "hint": hint }));
    show(app);
    hide_after(app, 3200);
}

pub fn error(app: &AppHandle, message: &str) {
    emit(app, json!({ "phase": "error", "hint": message }));
    show(app);
    hide_after(app, 2400);
}

pub fn cancelled(app: &AppHandle) {
    emit(app, json!({ "phase": "cancelled", "hint": "已取消" }));
    hide_after(app, 700);
}
