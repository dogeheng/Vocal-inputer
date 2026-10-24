//! Dictation controller: hotkey state machine -> recording -> recognition ->
//! cleanup -> insertion, with overlay feedback and history.

use crate::cloud::{self, CloudConfig};
use crate::hotkey::HotkeyEvent;
use crate::insert::{self, InsertOutcome};
use crate::pipeline::{self, Session, SessionObserver, SessionParams, SessionResult, TextSource};
use crate::settings::TriggerMode;
use crate::state::AppState;
use crate::{overlay, text};
use crossbeam_channel::{Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const MAX_RECORDING: Duration = Duration::from_secs(6 * 60);
/// Another key within this time after the press cancels (it was a shortcut).
const SHORTCUT_GRACE: Duration = Duration::from_millis(900);

pub enum Event {
    Hotkey(HotkeyEvent),
    /// Internal: stop due to max length.
    ForceStop(u64),
    /// Internal: processing worker finished.
    Done(u64),
}

enum Phase {
    Idle,
    Recording {
        id: u64,
        started: Instant,
        latched: bool,
        session: Session,
        target: insert::Target,
        rec_flag: Arc<AtomicBool>,
    },
    Processing {
        id: u64,
        cancel: Arc<AtomicBool>,
    },
}

struct OverlayObserver {
    app: AppHandle,
}

impl SessionObserver for OverlayObserver {
    fn on_partial(&self, text: &str) {
        overlay::partial(&self.app, text);
    }
}

pub fn spawn(app: AppHandle, rx: Receiver<Event>, tx: Sender<Event>) {
    std::thread::Builder::new()
        .name("controller".into())
        .spawn(move || run(app, rx, tx))
        .expect("spawn controller");
}

fn run(app: AppHandle, rx: Receiver<Event>, tx: Sender<Event>) {
    let mut phase = Phase::Idle;
    let mut next_id: u64 = 1;
    while let Ok(ev) = rx.recv() {
        let state = app.state::<AppState>();
        if let Event::Hotkey(h) = &ev {
            let _ = app.emit_to("main", "hotkey://event", format!("{h:?}"));
        }
        phase = match (phase, ev) {
            (Phase::Idle, Event::Hotkey(HotkeyEvent::TriggerDown)) => {
                let id = next_id;
                next_id += 1;
                start_recording(&app, &state, id, &tx).unwrap_or(Phase::Idle)
            }
            (Phase::Recording { id, started, latched, session, target, rec_flag }, Event::Hotkey(h)) => {
                let s = state.settings.read().clone();
                let held = started.elapsed();
                match h {
                    HotkeyEvent::TriggerUp => {
                        let stop = match s.trigger_mode {
                            TriggerMode::Hold => true,
                            TriggerMode::Toggle => false,
                            TriggerMode::Hybrid => latched || held >= Duration::from_millis(s.tap_threshold_ms),
                        };
                        if stop {
                            stop_and_process(&app, &state, id, session, target, rec_flag, &tx)
                        } else {
                            if !latched {
                                overlay::latched(&app);
                            }
                            Phase::Recording { id, started, latched: true, session, target, rec_flag }
                        }
                    }
                    HotkeyEvent::TriggerDown => {
                        if latched || s.trigger_mode == TriggerMode::Toggle {
                            stop_and_process(&app, &state, id, session, target, rec_flag, &tx)
                        } else {
                            Phase::Recording { id, started, latched, session, target, rec_flag }
                        }
                    }
                    HotkeyEvent::Escape => {
                        cancel_recording(&state, session, rec_flag);
                        overlay::cancelled(&app);
                        Phase::Idle
                    }
                    HotkeyEvent::OtherKey => {
                        if !latched && held < SHORTCUT_GRACE {
                            // The user was pressing a keyboard shortcut.
                            cancel_recording(&state, session, rec_flag);
                            overlay::hide(&app);
                            Phase::Idle
                        } else {
                            Phase::Recording { id, started, latched, session, target, rec_flag }
                        }
                    }
                }
            }
            (Phase::Recording { id, session, target, rec_flag, .. }, Event::ForceStop(fid)) if fid == id => {
                stop_and_process(&app, &state, id, session, target, rec_flag, &tx)
            }
            (Phase::Processing { id, cancel }, Event::Hotkey(HotkeyEvent::Escape)) => {
                cancel.store(true, Ordering::SeqCst);
                state.hotkey.capture_escape.store(false, Ordering::SeqCst);
                overlay::cancelled(&app);
                let _ = id;
                Phase::Idle
            }
            (Phase::Processing { id, .. }, Event::Done(did)) if did == id => {
                state.hotkey.capture_escape.store(false, Ordering::SeqCst);
                Phase::Idle
            }
            (p, _) => p,
        };
        state.set_phase_label(match &phase {
            Phase::Idle => "idle",
            Phase::Recording { .. } => "recording",
            Phase::Processing { .. } => "processing",
        });
    }
}

fn build_params(state: &AppState) -> SessionParams {
    let s = state.settings.read().clone();
    let key = state.api_key.read().clone();
    let cloud = match (s.cloud_enabled, key) {
        (true, Some(api_key)) => Some(CloudConfig {
            base_url: s.cloud_base_url.clone(),
            style: s.cloud_api_style,
            model: s.cloud_model.clone(),
            language: if s.language == "auto" { None } else { Some(s.language.clone()) },
            api_key,
        }),
        _ => None,
    };
    SessionParams {
        local: state.local.read().clone(),
        rt: state.rt.handle().clone(),
        http: state.http.read().clone(),
        cloud,
        deadline: Duration::from_millis(s.cloud_deadline_ms),
        min_chunk_secs: 2.5,
        cloud_timeout: Duration::from_secs(15),
    }
}

fn start_recording(app: &AppHandle, state: &AppState, id: u64, tx: &Sender<Event>) -> Option<Phase> {
    let params = build_params(state);
    if !params.local.has_final() && !params.local.has_preview() && params.cloud.is_none() {
        overlay::error(app, "请先下载本地模型或填写云端 API Key");
        crate::show_main_window(app);
        return None;
    }
    let target = insert::capture_target();
    if target.focus.secure {
        overlay::error(app, "密码输入框不支持语音输入");
        return None;
    }
    if let Some(c) = &params.cloud {
        let (client, base, key) = (params.http.clone(), c.base_url.clone(), c.api_key.clone());
        state.rt.spawn(async move { cloud::warm_up(&client, &base, &key).await });
    }
    let session = Session::start(params, Arc::new(OverlayObserver { app: app.clone() }));
    if let Err(e) = state.recorder.start(session.audio_tx.clone()) {
        session.cancel();
        overlay::error(app, &e);
        return None;
    }
    state.hotkey.capture_escape.store(true, Ordering::SeqCst);
    let app_name = target.app.as_ref().map(|a| a.name.clone()).unwrap_or_default();
    overlay::listening(app, &app_name);

    // Level meter + max-duration guard.
    let rec_flag = Arc::new(AtomicBool::new(true));
    {
        let flag = rec_flag.clone();
        let app = app.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let t0 = Instant::now();
            while flag.load(Ordering::Relaxed) {
                let lvl = app.state::<AppState>().recorder.level();
                overlay::level(&app, lvl);
                if t0.elapsed() > MAX_RECORDING {
                    let _ = tx.send(Event::ForceStop(id));
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
    }
    Some(Phase::Recording { id, started: Instant::now(), latched: false, session, target, rec_flag })
}

fn cancel_recording(state: &AppState, session: Session, rec_flag: Arc<AtomicBool>) {
    rec_flag.store(false, Ordering::SeqCst);
    state.recorder.stop();
    session.cancel();
    state.hotkey.capture_escape.store(false, Ordering::SeqCst);
}

fn stop_and_process(
    app: &AppHandle,
    state: &AppState,
    id: u64,
    session: Session,
    target: insert::Target,
    rec_flag: Arc<AtomicBool>,
    tx: &Sender<Event>,
) -> Phase {
    rec_flag.store(false, Ordering::SeqCst);
    let released = Instant::now();
    state.recorder.stop();
    overlay::processing(app);
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    let app2 = app.clone();
    let tx2 = tx.clone();
    std::thread::spawn(move || {
        let result = session.finish();
        if !c2.load(Ordering::SeqCst) {
            if let Some(r) = result {
                process_result(&app2, r, target, released, &c2);
            } else {
                overlay::error(&app2, "识别失败");
            }
        }
        let _ = tx2.send(Event::Done(id));
    });
    Phase::Processing { id, cancel }
}

fn source_label(s: TextSource) -> &'static str {
    match s {
        TextSource::Cloud => "cloud",
        TextSource::Local => "local",
        TextSource::Mixed => "mixed",
        TextSource::Preview => "preview",
        TextSource::Empty => "empty",
    }
}

struct Polished {
    text: String,
    cleaned: bool,
    /// LLM result still in flight (it was slower than the wait budget).
    pending: Option<Receiver<Result<String, String>>>,
}

/// Rule-based processing plus optional LLM cleanup. Waits at most `wait`;
/// when the LLM is slower the rule text is returned immediately and the LLM
/// result is delivered later through `pending` for a safe in-place upgrade.
fn polish(state: &AppState, raw: &str, wait: Duration) -> Polished {
    let s = state.settings.read().clone();
    let rule = text::rule_process(raw, &s);
    let plain = |t: String| Polished { text: t, cleaned: false, pending: None };
    if !(s.cleanup_enabled && s.cloud_enabled) || rule.chars().count() < 6 || !text::needs_llm(raw) {
        return plain(rule);
    }
    let Some(key) = state.api_key.read().clone() else { return plain(rule) };
    let client = state.http.read().clone();
    let (tx, rx) = crossbeam_channel::bounded(1);
    let input = rule.clone();
    state.rt.spawn(async move {
        let r = cloud::cleanup(
            &client,
            &s.cloud_base_url,
            &key,
            &s.cleanup_model,
            &input,
            &s.dictionary,
            Duration::from_secs(8),
        )
        .await
        .map(|t| text::finalize(&t, &s));
        let _ = tx.send(r);
    });
    match rx.recv_timeout(wait) {
        Ok(Ok(t)) => Polished { text: t, cleaned: true, pending: None },
        Ok(Err(e)) => {
            log::info!("cleanup skipped: {e}");
            plain(rule)
        }
        Err(_) => Polished { text: rule, cleaned: false, pending: Some(rx) },
    }
}

fn process_result(app: &AppHandle, r: SessionResult, target: insert::Target, released: Instant, cancel: &AtomicBool) {
    let state = app.state::<AppState>();
    log::info!(
        "session: audio={:.1}s chunks={} source={:?} first_partial={:?}ms finalize={}ms",
        r.audio_secs,
        r.chunks.len(),
        r.source,
        r.first_partial_ms,
        r.finalize_ms
    );
    if r.text.trim().is_empty() {
        overlay::error(app, if r.speech_detected { "没有识别到文字" } else { "没有听到声音" });
        return;
    }
    let cleanup_wait = Duration::from_millis(state.settings.read().cleanup_timeout_ms);
    let Polished { text: final_text, cleaned, pending } = polish(&state, &r.text, cleanup_wait);
    if cancel.load(Ordering::SeqCst) {
        return;
    }
    let (outcome, record) = insert::insert(&final_text);
    let latency = released.elapsed().as_millis() as u64;
    let src = source_label(r.source);
    match &outcome {
        InsertOutcome::Pasted { .. } => overlay::done(app, &final_text, src, cleaned, ""),
        InsertOutcome::PastedUnverified => {
            overlay::done(app, &final_text, src, cleaned, "若没有出现文字，可直接按 ⌘V 粘贴")
        }
        InsertOutcome::CopiedOnly { reason } => overlay::notice(app, &final_text, &format!("{reason}")),
        InsertOutcome::Refused { reason } => overlay::error(app, reason),
    }
    let s = state.settings.read().clone();
    let outcome_label = match &outcome {
        InsertOutcome::Pasted { verified: true } => "pasted",
        InsertOutcome::Pasted { verified: false } => "pasted-unverified",
        InsertOutcome::PastedUnverified => "pasted-blind",
        InsertOutcome::CopiedOnly { .. } => "copied",
        InsertOutcome::Refused { .. } => "refused",
    };
    let app_name = target.app.as_ref().map(|a| a.name.clone()).unwrap_or_default();
    let bundle = target.app.as_ref().map(|a| a.bundle_id.clone()).unwrap_or_default();
    let hist_id = if s.save_history {
        state.history.as_ref().and_then(|h| {
            h.add(&app_name, &bundle, &final_text, &r.text, src, r.audio_secs, latency, outcome_label)
        })
    } else {
        None
    };
    let _ = app.emit_to("main", "history://changed", ());
    let _ = app.emit_to(
        "main",
        "dictation://result",
        serde_json::json!({
            "text": final_text, "raw": r.text, "preview": r.preview, "source": src,
            "latencyMs": latency, "firstPartialMs": r.first_partial_ms, "chunks": r.chunks,
            "outcome": outcome_label
        }),
    );

    // Late upgrades: slower cloud chunks first (more accurate), otherwise a
    // slower LLM cleanup. Replaced in place only when provably safe.
    let window = Duration::from_millis(s.late_upgrade_window_ms);
    if window.is_zero() || (r.late.is_none() && pending.is_none()) {
        return;
    }
    let app = app.clone();
    let chunks = r.chunks.clone();
    let preview = r.preview.clone();
    let late = r.late;
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let mut best: Option<(String, &'static str, &'static str)> = None;
        if let Some(late) = late {
            if let Some((upgraded, source, _)) = pipeline::await_late(chunks, late, &preview, window) {
                let p = polish(&state, &upgraded, Duration::from_secs(4));
                best = Some((p.text, source_label(source), "已用更准确的云端结果替换"));
            }
        }
        if best.is_none() {
            if let Some(rx) = pending {
                if let Ok(Ok(t)) = rx.recv_timeout(window) {
                    best = Some((t, src, "已替换为整理后的文字"));
                }
            }
        }
        let Some((new_text, new_src, message)) = best else { return };
        if new_text == final_text {
            return;
        }
        let replaced = matches!(outcome, InsertOutcome::Pasted { verified: true })
            && insert::try_replace(&record, &new_text).is_ok();
        if let (Some(h), Some(id)) = (state.history.as_ref(), hist_id) {
            h.update_text(id, &new_text, new_src);
            let _ = app.emit_to("main", "history://changed", ());
        }
        overlay::notice(&app, &new_text, if replaced { message } else { "更准确的结果已保存到历史记录" });
    });
}
