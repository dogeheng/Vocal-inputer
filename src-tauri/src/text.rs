//! Deterministic text post-processing. Always runs (also offline) and is the
//! fallback when LLM cleanup is disabled, slow or fails.

use crate::settings::{Replacement, Settings};
use once_cell::sync::Lazy;
use regex::Regex;

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F |
        0x3040..=0x30FF | 0xAC00..=0xD7AF)
}

fn is_cjk_punct(c: char) -> bool {
    "，。！？；：、“”‘’（）《》【】…—".contains(c)
}

fn has_cjk(s: &str) -> bool {
    s.chars().any(is_cjk)
}

/// Join per-chunk transcripts.
pub fn join_segments(parts: &[String]) -> String {
    let mut out = String::new();
    for p in parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        if out.is_empty() {
            out.push_str(p);
            continue;
        }
        let last = out.chars().last().unwrap_or(' ');
        let first = p.chars().next().unwrap_or(' ');
        let cjk_join = is_cjk(last) || is_cjk_punct(last) || is_cjk(first) || is_cjk_punct(first);
        if cjk_join {
            if is_cjk(last) && is_cjk(first) {
                out.push('，');
            }
        } else if !last.is_whitespace() {
            if last.is_alphanumeric() && first.is_alphanumeric() && first.is_uppercase() {
                out.push('.');
            }
            out.push(' ');
        }
        out.push_str(p);
    }
    out
}

static RE_ZH_FILLER_LEAD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?:[嗯呃额啊哦唉]+[，,、\s]*)+").unwrap());
static RE_ZH_FILLER_INNER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([，。！？、,.!?\s])(?:[嗯呃额]+[，,、\s]*)+").unwrap());
static RE_EN_FILLER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(^|[\s,，])(?:um+|uh+|erm+|hmm+)(?:[,，]\s*|\s+|$)").unwrap());
static RE_DUP_PUNCT: Lazy<Regex> = Lazy::new(|| Regex::new(r"([，。！？、；：,.!?;:])[，。、,.]+").unwrap());
static RE_SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]{2,}").unwrap());
static RE_CJK_LATIN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([\p{Han}\p{Hiragana}\p{Katakana}])([A-Za-z])").unwrap());
static RE_LATIN_CJK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([A-Za-z])([\p{Han}\p{Hiragana}\p{Katakana}])").unwrap());

pub fn remove_fillers(t: &str) -> String {
    let t = RE_ZH_FILLER_LEAD.replace(t, "");
    let t = RE_ZH_FILLER_INNER.replace_all(&t, "$1");
    let t = RE_EN_FILLER.replace_all(&t, "$1");
    t.trim().to_string()
}

/// Half-width punctuation next to CJK becomes full-width.
pub fn normalize_punctuation(t: &str) -> String {
    if !has_cjk(t) {
        return t.to_string();
    }
    let chars: Vec<char> = t.chars().collect();
    let cjk_count = chars.iter().filter(|c| is_cjk(**c)).count();
    let latin_words = t.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()).count();
    let cjk_dominant = cjk_count >= latin_words * 2;
    let mut out = String::with_capacity(t.len());
    for (i, &c) in chars.iter().enumerate() {
        let prev = if i > 0 { chars[i - 1] } else { ' ' };
        let next = chars.get(i + 1).copied().unwrap_or(' ');
        let at_end = i + 1 == chars.len();
        let near_cjk = is_cjk(prev)
            || (is_cjk(next) && !prev.is_ascii_alphanumeric())
            || (cjk_dominant && (at_end || next == ' ' && chars.get(i + 2).map_or(false, |n| is_cjk(*n))));
        let mapped = if near_cjk {
            match c {
                ',' => Some('，'),
                '.' if !next.is_ascii_digit() => Some('。'),
                '?' => Some('？'),
                '!' => Some('！'),
                ';' => Some('；'),
                ':' if !next.is_ascii_digit() => Some('：'),
                _ => None,
            }
        } else {
            None
        };
        match mapped {
            Some(m) => {
                out.push(m);
            }
            None => out.push(c),
        }
    }
    // Remove spaces that follow full-width punctuation.
    let mut cleaned = String::with_capacity(out.len());
    let mut prev = ' ';
    for c in out.chars() {
        if c == ' ' && is_cjk_punct(prev) {
            continue;
        }
        cleaned.push(c);
        prev = c;
    }
    RE_DUP_PUNCT.replace_all(&cleaned, "$1").into_owned()
}

/// Insert a space between CJK and Latin letters ("和Alex开会" -> "和 Alex 开会").
pub fn cjk_latin_spacing(t: &str) -> String {
    let t = RE_CJK_LATIN.replace_all(t, "$1 $2");
    RE_LATIN_CJK.replace_all(&t, "$1 $2").into_owned()
}

pub fn apply_replacements(t: &str, reps: &[Replacement]) -> String {
    let mut s = t.to_string();
    for r in reps {
        if !r.from.is_empty() {
            s = s.replace(&r.from, &r.to);
        }
    }
    s
}

/// Fix the casing of dictionary terms written in Latin script
/// ("openrouter" -> "OpenRouter").
pub fn apply_dictionary_case(t: &str, dict: &[String]) -> String {
    let mut s = t.to_string();
    for term in dict.iter().filter(|w| w.is_ascii() && w.len() >= 2) {
        let pattern = format!(r"(?i)\b{}\b", regex::escape(term));
        if let Ok(re) = Regex::new(&pattern) {
            s = re.replace_all(&s, term.as_str()).into_owned();
        }
    }
    s
}

/// Full deterministic pipeline.
pub fn rule_process(t: &str, settings: &Settings) -> String {
    let mut s = t.trim().to_string();
    if s.is_empty() {
        return s;
    }
    if settings.remove_fillers {
        s = remove_fillers(&s);
    }
    s = normalize_punctuation(&s);
    s = apply_replacements(&s, &settings.replacements);
    s = apply_dictionary_case(&s, &settings.dictionary);
    if settings.cjk_spacing {
        s = cjk_latin_spacing(&s);
    }
    s = RE_SPACES.replace_all(&s, " ").into_owned();
    s.trim().to_string()
}

/// Final touches after optional LLM cleanup (the LLM may undo spacing).
pub fn finalize(t: &str, settings: &Settings) -> String {
    let mut s = apply_replacements(t.trim(), &settings.replacements);
    s = apply_dictionary_case(&s, &settings.dictionary);
    if settings.cjk_spacing {
        s = cjk_latin_spacing(&s);
    }
    s
}

static RE_LLM_TRIGGER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(?i)(嗯|呃|额|那个那个|就是说|然后然后|不对|不是不是|改成|改为|我是说|我的意思是|应该是|更正|说错了|重新说|算了|",
        r"第一|第二|首先|其次|最后一点|",
        r"\bum+\b|\buh+\b|\byou know\b|\bi mean\b|\bscratch that\b|\bno wait\b|\bactually\b|\bsorry\b|\bfirst(ly)?\b|\bsecond(ly)?\b)"
    ))
    .unwrap()
});

/// Whether LLM cleanup is worth its latency for this transcript: fillers,
/// self-corrections, stutters, enumerations or long dictation.
pub fn needs_llm(raw: &str) -> bool {
    let cjk_count = raw.chars().filter(|c| is_cjk(*c)).count();
    let latin_words = raw.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| w.len() > 1).count();
    if cjk_count >= 120 || latin_words >= 60 || RE_LLM_TRIGGER.is_match(raw) {
        return true;
    }
    // Stutters: "the the", "我我", "然后然后".
    let words: Vec<&str> = raw.split_whitespace().collect();
    if words.windows(2).any(|w| w[0].eq_ignore_ascii_case(w[1]) && w[0].chars().any(|c| c.is_alphabetic())) {
        return true;
    }
    let cjk: Vec<char> = raw.chars().filter(|c| is_cjk(*c)).collect();
    for n in 1..=2 {
        if cjk.len() >= 2 * n && cjk.windows(2 * n).any(|w| w[..n] == w[n..] && !matches!(w[0], '谢' | '看' | '试' | '想' | '天' | '常' | '人' | '刚' | '慢' | '好')) {
            return true;
        }
    }
    false
}

pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fillers() {
        assert_eq!(remove_fillers("嗯，我们明天开会"), "我们明天开会");
        assert_eq!(remove_fillers("我们，呃，明天开会"), "我们，明天开会");
        assert_eq!(remove_fillers("um, I think so"), "I think so");
    }

    #[test]
    fn punct_and_spacing() {
        let s = normalize_punctuation("明天下午三点和Alex开会,讨论Q4 review.");
        assert_eq!(s, "明天下午三点和Alex开会，讨论Q4 review。");
        assert_eq!(cjk_latin_spacing("和Alex开会"), "和 Alex 开会");
        assert_eq!(cjk_latin_spacing("3点"), "3点");
        assert_eq!(normalize_punctuation("version 3.5 is out."), "version 3.5 is out.");
    }

    #[test]
    fn join() {
        let v = vec!["我们明天开会".to_string(), "讨论预算。".to_string()];
        assert_eq!(join_segments(&v), "我们明天开会，讨论预算。");
        let v = vec!["Hello there.".to_string(), "How are you?".to_string()];
        assert_eq!(join_segments(&v), "Hello there. How are you?");
    }

    #[test]
    fn llm_trigger() {
        assert!(needs_llm("我们周四再讨论，不对，改成周五"));
        assert!(needs_llm("um so we should ship it"));
        assert!(needs_llm("我我觉得可以"));
        assert!(needs_llm("I think the the plan works"));
        assert!(!needs_llm("明天下午三点开会。"));
        assert!(!needs_llm("Please attach the latest Figma link."));
        assert!(!needs_llm("谢谢你，我们明天见"));
    }

    #[test]
    fn dictionary() {
        let d = vec!["OpenRouter".to_string()];
        assert_eq!(apply_dictionary_case("use openrouter now", &d), "use OpenRouter now");
    }
}
