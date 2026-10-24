//! Text insertion into the focused field of the frontmost app.
//!
//! Strategy (most compatible first):
//! 1. Refuse password fields (AXSecureTextField).
//! 2. Snapshot the pasteboard, put our text on it (marked transient), post
//!    ⌘V, then restore the user's clipboard if nobody touched it meanwhile.
//! 3. If no editable focus can be detected we still paste, but leave the text
//!    on the clipboard so the user can paste manually (graceful degradation).
//!
//! Late cloud upgrades use AX to verify that the caret is still right after
//! the text we inserted before replacing it.

use crate::macos::{self, AxElement, CFRange, FocusInfo, FrontApp};
use crate::text::utf16_len;
use serde::Serialize;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Target {
    pub app: Option<FrontApp>,
    pub focus: FocusInfo,
}

/// Capture the frontmost app and focused element (call at hotkey press).
pub fn capture_target() -> Target {
    let app = macos::frontmost_app();
    if let Some(a) = &app {
        macos::wake_accessibility(a.pid);
    }
    Target { app, focus: macos::focused_element_info() }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum InsertOutcome {
    /// Pasted; `verified` = AX confirmed the caret moved by the text length.
    Pasted { verified: bool },
    /// Pasted blindly (no editable focus detected); text left on clipboard.
    PastedUnverified,
    /// Not inserted; text copied to clipboard instead.
    CopiedOnly { reason: String },
    Refused { reason: String },
}

#[derive(Clone, Debug)]
pub struct InsertRecord {
    pub pid: Option<i32>,
    pub element: Option<AxElement>,
    pub text: String,
    /// UTF-16 offset where the text starts.
    pub start: Option<isize>,
}

fn paste_with_restore(text: &str, keep_on_clipboard: bool) -> Result<(), String> {
    let saved = if keep_on_clipboard { None } else { Some(macos::clipboard_save()) };
    let cc = if keep_on_clipboard {
        macos::clipboard_set_text(text)
    } else {
        macos::clipboard_set_transient_text(text)
    };
    std::thread::sleep(Duration::from_millis(12));
    macos::post_cmd_v()?;
    if let Some(saved) = saved {
        // Give the target app time to read the pasteboard, then restore.
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(350));
            if macos::clipboard_change_count() == cc {
                macos::clipboard_restore(&saved);
            }
        });
    }
    Ok(())
}

pub fn insert(text: &str) -> (InsertOutcome, InsertRecord) {
    let focus = macos::focused_element_info();
    let pid = focus.element.as_ref().and_then(|e| e.pid()).or_else(|| macos::frontmost_app().map(|a| a.pid));
    let mut record = InsertRecord { pid, element: focus.element.clone(), text: text.to_string(), start: None };

    if text.is_empty() {
        return (InsertOutcome::Refused { reason: "没有可插入的文字".into() }, record);
    }
    if focus.secure {
        return (InsertOutcome::Refused { reason: "密码输入框不支持语音输入".into() }, record);
    }
    if !macos::accessibility_trusted() {
        macos::clipboard_set_text(text);
        return (
            InsertOutcome::CopiedOnly { reason: "未授权辅助功能，已复制到剪贴板".into() },
            record,
        );
    }

    let before = focus.element.as_ref().and_then(|e| e.range_attr("AXSelectedTextRange"));
    let editable = focus.element.is_some() && focus.editable;
    if let Err(e) = paste_with_restore(text, !editable) {
        macos::clipboard_set_text(text);
        return (InsertOutcome::CopiedOnly { reason: e }, record);
    }
    if !editable {
        return (InsertOutcome::PastedUnverified, record);
    }

    // Verify through AX (best effort; many apps update asynchronously).
    let mut verified = false;
    if let (Some(el), Some(b)) = (focus.element.as_ref(), before) {
        for _ in 0..6 {
            std::thread::sleep(Duration::from_millis(30));
            if let Some(after) = el.range_attr("AXSelectedTextRange") {
                if after.location == b.location + utf16_len(text) as isize && after.length == 0 {
                    verified = true;
                    break;
                }
            }
        }
        if verified {
            record.start = Some(b.location);
        }
    }
    (InsertOutcome::Pasted { verified }, record)
}

fn utf16_slice(s: &str, start: usize, len: usize) -> Option<String> {
    let v: Vec<u16> = s.encode_utf16().collect();
    if start + len > v.len() {
        return None;
    }
    String::from_utf16(&v[start..start + len]).ok()
}

/// Replace the previously inserted text with `new_text` only if it is
/// provably still there and the caret has not moved.
pub fn try_replace(record: &InsertRecord, new_text: &str) -> Result<(), String> {
    let start = record.start.ok_or("插入位置未知")?;
    let el = record.element.as_ref().ok_or("没有可验证的输入框")?;
    let focus = macos::focused_element_info();
    let now = focus.element.as_ref().ok_or("输入焦点已变化")?;
    if now != el {
        return Err("输入焦点已变化".into());
    }
    let old_len = utf16_len(&record.text);
    let value = el.string_attr("AXValue").ok_or("无法读取输入框内容")?;
    if utf16_slice(&value, start as usize, old_len).as_deref() != Some(record.text.as_str()) {
        return Err("文本已被修改".into());
    }
    let sel = el.range_attr("AXSelectedTextRange").ok_or("无法读取光标位置")?;
    if sel.location != start + old_len as isize || sel.length != 0 {
        return Err("光标已移动".into());
    }
    if !el.set_range_attr("AXSelectedTextRange", CFRange { location: start, length: old_len as isize }) {
        return Err("无法选中原文本".into());
    }
    std::thread::sleep(Duration::from_millis(20));
    let check = el.range_attr("AXSelectedTextRange");
    if check != Some(CFRange { location: start, length: old_len as isize }) {
        return Err("选区未生效".into());
    }
    paste_with_restore(new_text, false)?;
    Ok(())
}
