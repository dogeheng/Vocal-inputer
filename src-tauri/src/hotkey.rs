//! Global push-to-talk key detection with a Quartz event tap.
//!
//! * Modifier-only triggers (Fn/Globe, Right ⌥, Right ⌘, Right ⌃) are detected
//!   from `flagsChanged` events, which carry press *and* release - required for
//!   hold-to-talk and impossible with Carbon `RegisterEventHotKey`.
//! * The tap is an *active* filter (`kCGEventTapOptionDefault`) so that Esc can
//!   be swallowed while recording; that needs only the Accessibility grant.
//! * If macOS disables the tap (callback too slow / user input), it is
//!   re-enabled immediately.
#![allow(non_upper_case_globals)]

use crate::macos::SYNTHETIC_MARK;
use crate::settings::HotkeyChoice;
use core_foundation_sys::mach_port::{CFMachPortCreateRunLoopSource, CFMachPortRef};
use core_foundation_sys::runloop::{
    kCFRunLoopCommonModes, CFRunLoopAddSource, CFRunLoopGetCurrent, CFRunLoopRun,
};
use crossbeam_channel::Sender;
use once_cell::sync::OnceCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;
type CGEventTapCallBack =
    unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> u64;
}

const kCGSessionEventTap: u32 = 1;
const kCGHeadInsertEventTap: u32 = 0;
const kCGEventTapOptionDefault: u32 = 0;

const EV_KEY_DOWN: u32 = 10;
const EV_KEY_UP: u32 = 11;
const EV_FLAGS_CHANGED: u32 = 12;
const EV_TAP_DISABLED_TIMEOUT: u32 = 0xFFFF_FFFE;
const EV_TAP_DISABLED_USER: u32 = 0xFFFF_FFFF;

const FIELD_AUTOREPEAT: u32 = 8;
const FIELD_KEYCODE: u32 = 9;
const FIELD_USER_DATA: u32 = 42;

const FLAG_SECONDARY_FN: u64 = 0x0080_0000;
// Device-dependent bits (IOLLEvent.h) that distinguish left/right modifiers.
const NX_DEVICERCTLKEYMASK: u64 = 0x0000_2000;
const NX_DEVICERALTKEYMASK: u64 = 0x0000_0040;
const NX_DEVICERCMDKEYMASK: u64 = 0x0000_0010;

const KC_ESCAPE: i64 = 53;
const KC_RIGHT_COMMAND: i64 = 54;
const KC_RIGHT_OPTION: i64 = 61;
const KC_RIGHT_CONTROL: i64 = 62;
const KC_FUNCTION: i64 = 63;
const KC_GLOBE: i64 = 179;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    TriggerDown,
    TriggerUp,
    /// Another key was pressed while the trigger is held (likely a shortcut).
    OtherKey,
    /// Esc while capture is active (event swallowed).
    Escape,
}

/// State shared between the tap thread and the rest of the app.
pub struct HotkeyShared {
    trigger: AtomicU8,
    /// While true, Esc is swallowed and reported as `HotkeyEvent::Escape`.
    pub capture_escape: AtomicBool,
    /// Event tap is installed and running.
    pub tap_ok: AtomicBool,
    /// When true, trigger events are not reported (e.g. app paused).
    pub paused: AtomicBool,
}

impl HotkeyShared {
    pub fn new(choice: HotkeyChoice) -> Arc<Self> {
        let s = Arc::new(HotkeyShared {
            trigger: AtomicU8::new(0),
            capture_escape: AtomicBool::new(false),
            tap_ok: AtomicBool::new(false),
            paused: AtomicBool::new(false),
        });
        s.set_trigger(choice);
        s
    }

    pub fn set_trigger(&self, choice: HotkeyChoice) {
        let v = match choice {
            HotkeyChoice::Fn => 0,
            HotkeyChoice::RightOption => 1,
            HotkeyChoice::RightCommand => 2,
            HotkeyChoice::RightControl => 3,
        };
        self.trigger.store(v, Ordering::SeqCst);
    }

    fn trigger(&self) -> HotkeyChoice {
        match self.trigger.load(Ordering::Relaxed) {
            1 => HotkeyChoice::RightOption,
            2 => HotkeyChoice::RightCommand,
            3 => HotkeyChoice::RightControl,
            _ => HotkeyChoice::Fn,
        }
    }
}

struct TapCtx {
    tx: Sender<HotkeyEvent>,
    shared: Arc<HotkeyShared>,
    port: AtomicPtr<c_void>,
    trigger_down: AtomicBool,
}

static CTX: OnceCell<TapCtx> = OnceCell::new();

/// Returns Some(is_down) if this flagsChanged event concerns the trigger key.
fn trigger_transition(choice: HotkeyChoice, keycode: i64, flags: u64) -> Option<bool> {
    match choice {
        HotkeyChoice::Fn if keycode == KC_FUNCTION || keycode == KC_GLOBE => {
            Some(flags & FLAG_SECONDARY_FN != 0)
        }
        HotkeyChoice::RightOption if keycode == KC_RIGHT_OPTION => {
            Some(flags & NX_DEVICERALTKEYMASK != 0)
        }
        HotkeyChoice::RightCommand if keycode == KC_RIGHT_COMMAND => {
            Some(flags & NX_DEVICERCMDKEYMASK != 0)
        }
        HotkeyChoice::RightControl if keycode == KC_RIGHT_CONTROL => {
            Some(flags & NX_DEVICERCTLKEYMASK != 0)
        }
        _ => None,
    }
}

unsafe extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    etype: u32,
    event: CGEventRef,
    _user_info: *mut c_void,
) -> CGEventRef {
    let Some(ctx) = CTX.get() else { return event };

    if etype == EV_TAP_DISABLED_TIMEOUT || etype == EV_TAP_DISABLED_USER {
        let port = ctx.port.load(Ordering::SeqCst);
        if !port.is_null() {
            CGEventTapEnable(port as CFMachPortRef, true);
        }
        return event;
    }
    if event.is_null() || CGEventGetIntegerValueField(event, FIELD_USER_DATA) == SYNTHETIC_MARK {
        return event;
    }

    match etype {
        EV_FLAGS_CHANGED => {
            let keycode = CGEventGetIntegerValueField(event, FIELD_KEYCODE);
            let flags = CGEventGetFlags(event);
            if let Some(down) = trigger_transition(ctx.shared.trigger(), keycode, flags) {
                let was = ctx.trigger_down.swap(down, Ordering::SeqCst);
                if was != down && !ctx.shared.paused.load(Ordering::Relaxed) {
                    let _ = ctx.tx.try_send(if down {
                        HotkeyEvent::TriggerDown
                    } else {
                        HotkeyEvent::TriggerUp
                    });
                }
            } else if ctx.trigger_down.load(Ordering::Relaxed) {
                // Another modifier changed while holding the trigger, e.g. Fn+⇧.
                let _ = ctx.tx.try_send(HotkeyEvent::OtherKey);
            }
            event
        }
        EV_KEY_DOWN | EV_KEY_UP => {
            let keycode = CGEventGetIntegerValueField(event, FIELD_KEYCODE);
            let repeat = CGEventGetIntegerValueField(event, FIELD_AUTOREPEAT) != 0;
            if keycode == KC_ESCAPE && ctx.shared.capture_escape.load(Ordering::Relaxed) {
                if etype == EV_KEY_DOWN && !repeat {
                    let _ = ctx.tx.try_send(HotkeyEvent::Escape);
                }
                return std::ptr::null_mut(); // swallow
            }
            if etype == EV_KEY_DOWN && !repeat && ctx.trigger_down.load(Ordering::Relaxed) {
                let _ = ctx.tx.try_send(HotkeyEvent::OtherKey);
            }
            event
        }
        _ => event,
    }
}

/// Install the event tap on a dedicated run-loop thread. Retries until the
/// Accessibility permission is granted.
pub fn start(tx: Sender<HotkeyEvent>, shared: Arc<HotkeyShared>) {
    if CTX
        .set(TapCtx {
            tx,
            shared: shared.clone(),
            port: AtomicPtr::new(std::ptr::null_mut()),
            trigger_down: AtomicBool::new(false),
        })
        .is_err()
    {
        log::warn!("hotkey tap already started");
        return;
    }
    std::thread::Builder::new()
        .name("hotkey-tap".into())
        .spawn(move || unsafe {
            let mask: u64 = (1 << EV_KEY_DOWN) | (1 << EV_KEY_UP) | (1 << EV_FLAGS_CHANGED);
            let mut warned = false;
            loop {
                let port = CGEventTapCreate(
                    kCGSessionEventTap,
                    kCGHeadInsertEventTap,
                    kCGEventTapOptionDefault,
                    mask,
                    tap_callback,
                    std::ptr::null_mut(),
                );
                if port.is_null() {
                    if !warned {
                        log::warn!("event tap unavailable (Accessibility not granted yet); retrying");
                        warned = true;
                    }
                    shared.tap_ok.store(false, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(1500));
                    continue;
                }
                let ctx = CTX.get().expect("ctx");
                ctx.port.store(port as *mut c_void, Ordering::SeqCst);
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
                CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
                CGEventTapEnable(port, true);
                shared.tap_ok.store(true, Ordering::SeqCst);
                log::info!("global hotkey event tap installed");
                CFRunLoopRun();
                // Run loop exited unexpectedly; try to reinstall.
                shared.tap_ok.store(false, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(500));
            }
        })
        .expect("spawn hotkey thread");
}
