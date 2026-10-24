//! On-device recognition with sherpa-onnx:
//! * streaming Zipformer (zh/en) for the live preview (lowest TTFT)
//! * SenseVoice (offline, with punctuation/ITN) for the local final text
//! * Silero VAD for speech segmentation

use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OnlineRecognizer, OnlineRecognizerConfig,
    OnlineStream, SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
};
use std::path::{Path, PathBuf};

pub const SAMPLE_RATE: i32 = 16_000;

#[derive(Default, Clone, Debug)]
pub struct ModelPaths {
    pub preview_dir: Option<PathBuf>,
    pub final_dir: Option<PathBuf>,
    pub vad_file: Option<PathBuf>,
}

pub struct LocalAsr {
    online: Option<OnlineRecognizer>,
    offline: Option<OfflineRecognizer>,
    vad_model: Option<String>,
}

fn s(p: &Path) -> Option<String> {
    Some(p.to_string_lossy().into_owned())
}

fn pick(dir: &Path, names: &[&str]) -> Option<String> {
    names.iter().map(|n| dir.join(n)).find(|p| p.is_file()).and_then(|p| s(&p))
}

impl LocalAsr {
    pub fn empty() -> Self {
        LocalAsr { online: None, offline: None, vad_model: None }
    }

    pub fn load(paths: &ModelPaths) -> Self {
        let t = std::time::Instant::now();
        let online = paths.preview_dir.as_deref().and_then(|dir| {
            let mut c = OnlineRecognizerConfig::default();
            c.model_config.transducer.encoder =
                pick(dir, &["encoder-epoch-99-avg-1.int8.onnx", "encoder-epoch-99-avg-1.onnx"]);
            c.model_config.transducer.decoder =
                pick(dir, &["decoder-epoch-99-avg-1.onnx", "decoder-epoch-99-avg-1.int8.onnx"]);
            c.model_config.transducer.joiner =
                pick(dir, &["joiner-epoch-99-avg-1.int8.onnx", "joiner-epoch-99-avg-1.onnx"]);
            c.model_config.tokens = pick(dir, &["tokens.txt"]);
            c.model_config.num_threads = 2;
            c.decoding_method = Some("greedy_search".into());
            c.enable_endpoint = false;
            let r = OnlineRecognizer::create(&c);
            if r.is_none() {
                log::error!("failed to load streaming model from {}", dir.display());
            }
            r
        });
        let offline = paths.final_dir.as_deref().and_then(|dir| {
            let mut c = OfflineRecognizerConfig::default();
            c.model_config.sense_voice.model = pick(dir, &["model.int8.onnx", "model.onnx"]);
            c.model_config.sense_voice.language = Some("auto".into());
            c.model_config.sense_voice.use_itn = true;
            c.model_config.tokens = pick(dir, &["tokens.txt"]);
            c.model_config.num_threads = 4;
            c.model_config.provider = Some("cpu".into());
            c.decoding_method = Some("greedy_search".into());
            let r = OfflineRecognizer::create(&c);
            if r.is_none() {
                log::error!("failed to load SenseVoice from {}", dir.display());
            }
            r
        });
        let vad_model = paths.vad_file.as_deref().filter(|p| p.is_file()).and_then(s);
        log::info!(
            "local models loaded in {} ms (preview={}, final={}, vad={})",
            t.elapsed().as_millis(),
            online.is_some(),
            offline.is_some(),
            vad_model.is_some()
        );
        LocalAsr { online, offline, vad_model }
    }

    pub fn has_preview(&self) -> bool {
        self.online.is_some()
    }
    pub fn has_final(&self) -> bool {
        self.offline.is_some()
    }
    pub fn has_vad(&self) -> bool {
        self.vad_model.is_some()
    }

    pub fn new_stream(&self) -> Option<OnlineStream> {
        self.online.as_ref().map(|r| r.create_stream())
    }

    /// Feed 16 kHz samples and return the current partial text.
    pub fn feed(&self, stream: &OnlineStream, samples: &[f32]) -> Option<String> {
        let r = self.online.as_ref()?;
        stream.accept_waveform(SAMPLE_RATE, samples);
        while r.is_ready(stream) {
            r.decode(stream);
        }
        r.get_result(stream).map(|x| normalize_preview(&x.text))
    }

    pub fn finish(&self, stream: &OnlineStream) -> Option<String> {
        let r = self.online.as_ref()?;
        // Tail padding lets the streaming model flush its look-ahead.
        stream.accept_waveform(SAMPLE_RATE, &vec![0.0; (SAMPLE_RATE as usize) * 3 / 10]);
        stream.input_finished();
        while r.is_ready(stream) {
            r.decode(stream);
        }
        r.get_result(stream).map(|x| normalize_preview(&x.text))
    }

    /// Accurate local transcription of a complete segment (16 kHz mono).
    pub fn transcribe(&self, samples: &[f32]) -> Option<String> {
        let r = self.offline.as_ref()?;
        if samples.len() < (SAMPLE_RATE as usize) / 10 {
            return Some(String::new());
        }
        let stream = r.create_stream();
        stream.accept_waveform(SAMPLE_RATE, samples);
        r.decode(&stream);
        stream.get_result().map(|x| x.text.trim().to_string())
    }

    pub fn new_vad(&self) -> Option<VoiceActivityDetector> {
        let model = self.vad_model.clone()?;
        let cfg = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(model),
                threshold: 0.45,
                min_silence_duration: 0.3,
                min_speech_duration: 0.2,
                window_size: VAD_WINDOW as i32,
                max_speech_duration: 15.0,
            },
            sample_rate: SAMPLE_RATE,
            num_threads: 1,
            provider: Some("cpu".into()),
            debug: false,
            ..Default::default()
        };
        VoiceActivityDetector::create(&cfg, 120.0)
    }
}

pub const VAD_WINDOW: usize = 512;

/// The bilingual Zipformer emits English in upper case BPE pieces; lower-case
/// them for a friendlier preview and tidy spacing.
pub fn normalize_preview(t: &str) -> String {
    let lower: String = t.chars().map(|c| if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { c }).collect();
    lower.split_whitespace().collect::<Vec<_>>().join(" ")
}
