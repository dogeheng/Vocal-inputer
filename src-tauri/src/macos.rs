//! Thin, audited wrappers around macOS system APIs:
//! Accessibility (AXUIElement), Secure Input, NSPasteboard, frontmost app and
//! synthetic key events.
#![allow(non_upper_case_globals, non_snake_case, dead_code, unused_unsafe)]

use core_foundation::base::{CFEqual, CFGetTypeID, CFRelease, CFRetain, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::{CFString, CFStringGetTypeID, CFStringRef};
use std::ffi::c_void;

// ---------------------------------------------------------------------------
// Accessibility FFI
// ---------------------------------------------------------------------------

pub type AXUIElementRef = *const c_void;
type AXError = i32;
const kAXErrorSuccess: AXError = 0;
const kAXValueTypeCFRange: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CFRange {
    pub location: isize,
    pub length: isize,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementIsAttributeSettable(
        element: AXUIElementRef,
        attribute: CFStringRef,
        settable: *mut u8,
    ) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> AXError;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> AXError;
    fn AXValueCreate(the_type: u32, value_ptr: *const c_void) -> CFTypeRef;
    fn AXValueGetValue(value: CFTypeRef, the_type: u32, value_ptr: *mut c_void) -> u8;
    fn AXValueGetTypeID() -> usize;
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

pub fn accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// True while some app (password field, Terminal "Secure Keyboard Entry", ...)
/// has enabled Secure Event Input. Our event tap is blind in that state and we
/// must never try to inject text.
pub fn secure_input_enabled() -> bool {
    unsafe { IsSecureEventInputEnabled() != 0 }
}

/// Owned, retained AXUIElement.
pub struct AxElement(AXUIElementRef);

// AXUIElement is a CFType; AX calls are safe from any thread.
unsafe impl Send for AxElement {}
unsafe impl Sync for AxElement {}

impl Clone for AxElement {
    fn clone(&self) -> Self {
        unsafe { CFRetain(self.0 as CFTypeRef) };
        AxElement(self.0)
    }
}

impl Drop for AxElement {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0 as CFTypeRef) };
        }
    }
}

impl PartialEq for AxElement {
    fn eq(&self, other: &Self) -> bool {
        unsafe { CFEqual(self.0 as CFTypeRef, other.0 as CFTypeRef) != 0 }
    }
}

impl std::fmt::Debug for AxElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AxElement({:?})", self.0)
    }
}

impl AxElement {
    fn from_create(ptr: AXUIElementRef) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(AxElement(ptr))
        }
    }

    pub fn system_wide() -> Option<Self> {
        let el = Self::from_create(unsafe { AXUIElementCreateSystemWide() })?;
        unsafe { AXUIElementSetMessagingTimeout(el.0, 0.25) };
        Some(el)
    }

    pub fn application(pid: i32) -> Option<Self> {
        let el = Self::from_create(unsafe { AXUIElementCreateApplication(pid) })?;
        unsafe { AXUIElementSetMessagingTimeout(el.0, 0.25) };
        Some(el)
    }

    fn copy_attr(&self, name: &str) -> Option<CFTypeRef> {
        let attr = CFString::new(name);
        let mut value: CFTypeRef = std::ptr::null();
        let err = unsafe {
            AXUIElementCopyAttributeValue(self.0, attr.as_concrete_TypeRef(), &mut value)
        };
        if err == kAXErrorSuccess && !value.is_null() {
            Some(value)
        } else {
            None
        }
    }

    pub fn element_attr(&self, name: &str) -> Option<AxElement> {
        // The returned AXUIElement is +1 retained (Copy rule).
        self.copy_attr(name).map(|v| AxElement(v as AXUIElementRef))
    }

    pub fn string_attr(&self, name: &str) -> Option<String> {
        let v = self.copy_attr(name)?;
        unsafe {
            if CFGetTypeID(v) == CFStringGetTypeID() {
                let s = CFString::wrap_under_create_rule(v as CFStringRef);
                Some(s.to_string())
            } else {
                CFRelease(v);
                None
            }
        }
    }

    pub fn range_attr(&self, name: &str) -> Option<CFRange> {
        let v = self.copy_attr(name)?;
        let mut range = CFRange::default();
        let ok = unsafe {
            let ok = CFGetTypeID(v) == AXValueGetTypeID()
                && AXValueGetValue(v, kAXValueTypeCFRange, &mut range as *mut _ as *mut c_void) != 0;
            CFRelease(v);
            ok
        };
        ok.then_some(range)
    }

    pub fn set_range_attr(&self, name: &str, range: CFRange) -> bool {
        let attr = CFString::new(name);
        unsafe {
            let v = AXValueCreate(kAXValueTypeCFRange, &range as *const _ as *const c_void);
            if v.is_null() {
                return false;
            }
            let err = AXUIElementSetAttributeValue(self.0, attr.as_concrete_TypeRef(), v);
            CFRelease(v);
            err == kAXErrorSuccess
        }
    }

    pub fn set_string_attr(&self, name: &str, value: &str) -> bool {
        let attr = CFString::new(name);
        let v = CFString::new(value);
        unsafe {
            AXUIElementSetAttributeValue(
                self.0,
                attr.as_concrete_TypeRef(),
                v.as_concrete_TypeRef() as CFTypeRef,
            ) == kAXErrorSuccess
        }
    }

    pub fn set_bool_attr(&self, name: &str, value: bool) -> bool {
        let attr = CFString::new(name);
        let v = if value { CFBoolean::true_value() } else { CFBoolean::false_value() };
        unsafe {
            AXUIElementSetAttributeValue(
                self.0,
                attr.as_concrete_TypeRef(),
                v.as_concrete_TypeRef() as CFTypeRef,
            ) == kAXErrorSuccess
        }
    }

    pub fn is_settable(&self, name: &str) -> bool {
        let attr = CFString::new(name);
        let mut settable: u8 = 0;
        let err = unsafe {
            AXUIElementIsAttributeSettable(self.0, attr.as_concrete_TypeRef(), &mut settable)
        };
        err == kAXErrorSuccess && settable != 0
    }

    pub fn pid(&self) -> Option<i32> {
        let mut pid = 0;
        (unsafe { AXUIElementGetPid(self.0, &mut pid) } == kAXErrorSuccess).then_some(pid)
    }
}

/// Snapshot of the focused UI element at a point in time.
#[derive(Clone, Debug)]
pub struct FocusInfo {
    pub element: Option<AxElement>,
    pub role: String,
    pub subrole: String,
    pub editable: bool,
    pub secure: bool,
}

impl FocusInfo {
    pub fn empty() -> Self {
        FocusInfo { element: None, role: String::new(), subrole: String::new(), editable: false, secure: false }
    }
}

/// Chromium/Electron apps build their AX tree lazily. Setting
/// `AXManualAccessibility` wakes it without the side effects of
/// `AXEnhancedUserInterface` (which breaks window animations).
pub fn wake_accessibility(pid: i32) {
    if let Some(app) = AxElement::application(pid) {
        let _ = app.set_bool_attr("AXManualAccessibility", true);
    }
}

pub fn focused_element_info() -> FocusInfo {
    if !accessibility_trusted() {
        return FocusInfo::empty();
    }
    let Some(sys) = AxElement::system_wide() else { return FocusInfo::empty() };
    let Some(el) = sys.element_attr("AXFocusedUIElement") else { return FocusInfo::empty() };
    let role = el.string_attr("AXRole").unwrap_or_default();
    let subrole = el.string_attr("AXSubrole").unwrap_or_default();
    let secure = subrole == "AXSecureTextField";
    let editable = matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField")
        || el.is_settable("AXSelectedText")
        || el.is_settable("AXValue") && role != "AXCheckBox" && role != "AXSlider";
    FocusInfo { element: Some(el), role, subrole, editable, secure }
}

// ---------------------------------------------------------------------------
// Frontmost application
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontApp {
    pub pid: i32,
    pub bundle_id: String,
    pub name: String,
}

pub fn frontmost_app() -> Option<FrontApp> {
    use objc2_app_kit::NSWorkspace;
    unsafe {
        let ws = NSWorkspace::sharedWorkspace();
        let app = ws.frontmostApplication()?;
        Some(FrontApp {
            pid: app.processIdentifier(),
            bundle_id: app.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default(),
            name: app.localizedName().map(|s| s.to_string()).unwrap_or_default(),
        })
    }
}

// ---------------------------------------------------------------------------
// Pasteboard
// ---------------------------------------------------------------------------

/// Full copy of the general pasteboard (all items, all types).
pub struct SavedClipboard {
    items: Vec<Vec<(String, Vec<u8>)>>,
}

pub fn clipboard_change_count() -> isize {
    use objc2_app_kit::NSPasteboard;
    unsafe { NSPasteboard::generalPasteboard().changeCount() }
}

pub fn clipboard_save() -> SavedClipboard {
    use objc2_app_kit::NSPasteboard;
    let mut items = Vec::new();
    unsafe {
        let pb = NSPasteboard::generalPasteboard();
        if let Some(arr) = pb.pasteboardItems() {
            for item in arr.iter() {
                let mut entries = Vec::new();
                for t in item.types().iter() {
                    if let Some(data) = item.dataForType(&t) {
                        entries.push((t.to_string(), data.to_vec()));
                    }
                }
                items.push(entries);
            }
        }
    }
    SavedClipboard { items }
}

fn write_items(items: &[Vec<(String, Vec<u8>)>]) -> isize {
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSData, NSString};
    unsafe {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        if !items.is_empty() {
            let objs: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = items
                .iter()
                .map(|entries| {
                    let it = NSPasteboardItem::new();
                    for (t, d) in entries {
                        let data = NSData::with_bytes(d);
                        it.setData_forType(&data, &NSString::from_str(t));
                    }
                    ProtocolObject::from_retained(it)
                })
                .collect();
            let arr = NSArray::from_retained_slice(&objs);
            pb.writeObjects(&arr);
        }
        pb.changeCount()
    }
}

pub fn clipboard_restore(saved: &SavedClipboard) {
    write_items(&saved.items);
}

/// Put `text` on the pasteboard, flagged transient so clipboard managers that
/// honour nspasteboard.org conventions skip it. Returns the new change count.
pub fn clipboard_set_transient_text(text: &str) -> isize {
    let items = vec![vec![
        ("public.utf8-plain-text".to_string(), text.as_bytes().to_vec()),
        ("org.nspasteboard.TransientType".to_string(), Vec::new()),
        ("org.nspasteboard.AutoGeneratedType".to_string(), Vec::new()),
    ]];
    write_items(&items)
}

/// Put `text` on the pasteboard as a normal (persistent) copy.
pub fn clipboard_set_text(text: &str) -> isize {
    write_items(&[vec![("public.utf8-plain-text".to_string(), text.as_bytes().to_vec())]])
}

// ---------------------------------------------------------------------------
// Synthetic key events
// ---------------------------------------------------------------------------

/// Marker stored in `kCGEventSourceUserData` so our own event tap can ignore
/// the events we synthesize.
pub const SYNTHETIC_MARK: i64 = 0x564F_4943; // "VOIC"

const KEYCODE_V: u16 = 9;

pub fn post_cmd_v() -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, EventField};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "无法创建键盘事件源".to_string())?;
    let down = CGEvent::new_keyboard_event(src.clone(), KEYCODE_V, true)
        .map_err(|_| "无法创建按键事件".to_string())?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    down.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTHETIC_MARK);
    let up = CGEvent::new_keyboard_event(src, KEYCODE_V, false)
        .map_err(|_| "无法创建按键事件".to_string())?;
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    up.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTHETIC_MARK);
    down.post(CGEventTapLocation::HID);
    std::thread::sleep(std::time::Duration::from_millis(6));
    up.post(CGEventTapLocation::HID);
    Ok(())
}

/// Open a pane of System Settings › Privacy & Security.
pub fn open_privacy_pane(kind: &str) {
    let anchor = match kind {
        "microphone" => "Privacy_Microphone",
        "accessibility" => "Privacy_Accessibility",
        "inputMonitoring" => "Privacy_ListenEvent",
        _ => "Privacy",
    };
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
    let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
}
