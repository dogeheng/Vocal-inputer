//! API keys live only in the macOS login Keychain.
//!
//! Rules: never log the key, never send it to the webview, never write it to
//! disk. The UI only learns whether a key exists.

use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

const SERVICE: &str = "voice-input-desktop";
const OPENROUTER_ACCOUNT: &str = "openrouter_api_key";

pub fn openrouter_key() -> Option<String> {
    let bytes = get_generic_password(SERVICE, OPENROUTER_ACCOUNT).ok()?;
    let key = String::from_utf8(bytes).ok()?.trim().to_string();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

pub fn set_openrouter_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.len() < 20 || key.contains(char::is_whitespace) {
        return Err("API Key 格式不正确".into());
    }
    // Re-create instead of update so the item's access list is owned by this
    // app (no Keychain prompt on later reads), even if another tool created
    // the previous item.
    let _ = delete_generic_password(SERVICE, OPENROUTER_ACCOUNT);
    set_generic_password(SERVICE, OPENROUTER_ACCOUNT, key.as_bytes())
        .map_err(|e| format!("写入钥匙串失败：{e}"))
}

pub fn clear_openrouter_key() -> Result<(), String> {
    match delete_generic_password(SERVICE, OPENROUTER_ACCOUNT) {
        Ok(()) => Ok(()),
        // errSecItemNotFound
        Err(e) if e.code() == -25300 => Ok(()),
        Err(e) => Err(format!("删除失败：{e}")),
    }
}

/// A display hint that reveals nothing secret (only the public prefix).
pub fn key_hint(key: &str) -> String {
    let prefix: String = key.chars().take(6).collect();
    format!("{prefix}••••••••")
}
