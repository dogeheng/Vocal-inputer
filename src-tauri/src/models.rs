//! Local model catalogue, discovery and download.
//!
//! Files are fetched one by one (int8 weights only) from Hugging Face, falling
//! back to the hf-mirror.com mirror (reachable from mainland China) and, for
//! the VAD, GitHub releases.

use futures_util::StreamExt;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct ModelFile {
    pub name: &'static str,
    /// Expected size in bytes (used for verification and progress).
    pub size: u64,
}

pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub dir: &'static str,
    pub hf_repo: &'static str,
    pub extra_urls: &'static [&'static str],
    pub files: &'static [ModelFile],
}

pub const PREVIEW_LARGE: ModelSpec = ModelSpec {
    id: "preview",
    label: "实时预览模型（流式 Zipformer 中英双语）",
    dir: "sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20",
    hf_repo: "csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20",
    extra_urls: &[],
    files: &[
        ModelFile { name: "encoder-epoch-99-avg-1.int8.onnx", size: 181_895_032 },
        ModelFile { name: "decoder-epoch-99-avg-1.onnx", size: 13_876_452 },
        ModelFile { name: "joiner-epoch-99-avg-1.int8.onnx", size: 3_228_404 },
        ModelFile { name: "tokens.txt", size: 56_317 },
    ],
};

/// Dev-only alternative (not downloadable per-file); used if present locally.
pub const PREVIEW_SMALL_DIR: &str = "sherpa-onnx-streaming-zipformer-small-bilingual-zh-en-2023-02-16";

pub const FINAL_SENSEVOICE: ModelSpec = ModelSpec {
    id: "final",
    label: "离线精识别模型（SenseVoice 中英日韩粤）",
    dir: "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17",
    hf_repo: "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17",
    extra_urls: &[],
    files: &[
        ModelFile { name: "model.int8.onnx", size: 239_233_841 },
        ModelFile { name: "tokens.txt", size: 315_894 },
    ],
};

pub const VAD: ModelSpec = ModelSpec {
    id: "vad",
    label: "语音活动检测（Silero VAD）",
    dir: "silero-vad",
    hf_repo: "csukuangfj/vad",
    extra_urls: &["https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx"],
    files: &[ModelFile { name: "silero_vad.onnx", size: 0 }],
};

pub const ALL: [&ModelSpec; 3] = [&PREVIEW_LARGE, &FINAL_SENSEVOICE, &VAD];

pub fn primary_root(app_data: &Path) -> PathBuf {
    app_data.join("models")
}

/// Search order: app data dir, then the developer cache.
pub fn search_roots(app_data: &Path) -> Vec<PathBuf> {
    let mut v = vec![primary_root(app_data)];
    if let Ok(home) = std::env::var("HOME") {
        v.push(PathBuf::from(home).join(".cache/voice-input-desktop/models"));
    }
    v
}

fn file_ok(path: &Path, expected: u64) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => m.is_file() && m.len() > 0 && (expected == 0 || m.len() == expected),
        Err(_) => false,
    }
}

/// Directory that contains every file of `spec`, if any.
pub fn resolve(spec: &ModelSpec, roots: &[PathBuf]) -> Option<PathBuf> {
    for root in roots {
        let mut candidates = vec![root.join(spec.dir)];
        if spec.id == "vad" {
            candidates.push(root.clone()); // dev cache keeps the VAD at the root
        }
        for dir in candidates {
            if spec.files.iter().all(|f| file_ok(&dir.join(f.name), f.size)) {
                return Some(dir);
            }
        }
    }
    None
}

pub fn resolve_small_preview(roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .map(|r| r.join(PREVIEW_SMALL_DIR))
        .find(|d| d.join("encoder-epoch-99-avg-1.int8.onnx").is_file() && d.join("tokens.txt").is_file())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub label: String,
    pub ready: bool,
    pub size: u64,
    pub path: Option<String>,
}

pub fn status(app_data: &Path) -> Vec<ModelStatus> {
    let roots = search_roots(app_data);
    ALL.iter()
        .map(|s| {
            let p = resolve(s, &roots);
            ModelStatus {
                id: s.id.into(),
                label: s.label.into(),
                ready: p.is_some(),
                size: s.files.iter().map(|f| f.size).sum(),
                path: p.map(|p| p.display().to_string()),
            }
        })
        .collect()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub model: String,
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
    pub source: String,
}

fn hosts() -> [&'static str; 2] {
    ["https://huggingface.co", "https://hf-mirror.com"]
}

/// Pick the fastest reachable host (HF or the China mirror).
async fn pick_host(client: &reqwest::Client, probe_repo: &str, probe_file: &str) -> Vec<String> {
    let mut ordered: Vec<(u128, String)> = Vec::new();
    let probes = hosts().map(|h| {
        let url = format!("{h}/{probe_repo}/resolve/main/{probe_file}");
        let c = client.clone();
        async move {
            let t = std::time::Instant::now();
            let ok = c
                .get(&url)
                .header("Range", "bytes=0-0")
                .timeout(Duration::from_secs(6))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false);
            (ok, t.elapsed().as_millis(), h.to_string())
        }
    });
    let results = futures_util::future::join_all(probes).await;
    for (ok, ms, h) in results {
        if ok {
            ordered.push((ms, h));
        }
    }
    ordered.sort_by_key(|(ms, _)| *ms);
    let mut v: Vec<String> = ordered.into_iter().map(|(_, h)| h).collect();
    for h in hosts() {
        if !v.iter().any(|x| x == h) {
            v.push(h.to_string());
        }
    }
    v
}

async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected: u64,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(60 * 30))
        .send()
        .await
        .map_err(|e| format!("连接失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(expected);
    let part = dest.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut stream = resp.bytes_stream();
    let mut done: u64 = 0;
    let mut last_emit = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断：{e}"))?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        done += chunk.len() as u64;
        if done - last_emit > 512 * 1024 || done == total {
            last_emit = done;
            on_progress(done, total);
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if expected != 0 && done != expected {
        let _ = std::fs::remove_file(&part);
        return Err(format!("文件大小不符（{done} / {expected}）"));
    }
    std::fs::rename(&part, dest).map_err(|e| e.to_string())
}

/// Download every missing model into the app data directory.
pub async fn download_missing(
    client: &reqwest::Client,
    app_data: &Path,
    mut on_progress: impl FnMut(DownloadProgress),
) -> Result<(), String> {
    let roots = search_roots(app_data);
    let hosts = pick_host(client, FINAL_SENSEVOICE.hf_repo, "tokens.txt").await;
    for spec in ALL {
        if resolve(spec, &roots).is_some() {
            continue;
        }
        let dir = primary_root(app_data).join(spec.dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for f in spec.files {
            let dest = dir.join(f.name);
            if file_ok(&dest, f.size) {
                continue;
            }
            let mut urls: Vec<String> = hosts
                .iter()
                .map(|h| format!("{h}/{}/resolve/main/{}", spec.hf_repo, f.name))
                .collect();
            urls.extend(spec.extra_urls.iter().map(|s| s.to_string()));
            let mut last_err = String::new();
            let mut ok = false;
            for url in &urls {
                let source = url.split('/').nth(2).unwrap_or("").to_string();
                let r = download_file(client, url, &dest, f.size, |d, t| {
                    on_progress(DownloadProgress {
                        model: spec.id.into(),
                        file: f.name.into(),
                        downloaded: d,
                        total: t,
                        source: source.clone(),
                    })
                })
                .await;
                match r {
                    Ok(()) => {
                        ok = true;
                        break;
                    }
                    Err(e) => {
                        log::warn!("download {url} failed: {e}");
                        last_err = e;
                    }
                }
            }
            if !ok {
                return Err(format!("{} 下载失败：{last_err}", f.name));
            }
        }
    }
    Ok(())
}
