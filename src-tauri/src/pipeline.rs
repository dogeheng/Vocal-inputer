//! Dictation session engine (independent of Tauri so it can be tested with
//! WAV files).
//!
//! Timeline of one utterance:
//! 1. Audio is resampled to 16 kHz and fed to the streaming model -> live
//!    preview (lowest time-to-first-token).
//! 2. Silero VAD finds pauses. Each time >= `min_chunk_secs` of speech has
//!    accumulated and a pause is seen, that chunk is *immediately* uploaded to
//!    the cloud and transcribed locally by SenseVoice in parallel.
//! 3. When the user stops, only the tail chunk is still in flight. We wait for
//!    cloud results until `deadline`; any chunk without a cloud result uses the
//!    local SenseVoice text. Late cloud results are handed back for an
//!    optional, safe in-place upgrade.

use crate::audio::AudioChunk;
use crate::cloud::{self, CloudConfig};
use crate::local_asr::{LocalAsr, SAMPLE_RATE, VAD_WINDOW};
use crate::text;
use crossbeam_channel::{unbounded, Receiver, Sender};
use serde::Serialize;
use sherpa_onnx::LinearResampler;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SR: usize = SAMPLE_RATE as usize;

pub trait SessionObserver: Send + Sync + 'static {
    fn on_partial(&self, _text: &str) {}
    fn on_chunk_dispatched(&self, _index: usize, _secs: f32) {}
}

pub struct NoopObserver;
impl SessionObserver for NoopObserver {}

#[derive(Clone)]
pub struct SessionParams {
    pub local: Arc<LocalAsr>,
    pub rt: tokio::runtime::Handle,
    pub http: reqwest::Client,
    pub cloud: Option<CloudConfig>,
    pub deadline: Duration,
    pub min_chunk_secs: f32,
    pub cloud_timeout: Duration,
}

pub enum Ctrl {
    Finish,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TextSource {
    Cloud,
    Local,
    Mixed,
    Preview,
    Empty,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkOutcome {
    pub start_s: f32,
    pub end_s: f32,
    pub local: Option<String>,
    pub cloud: Option<String>,
    pub cloud_error: Option<String>,
    pub cloud_ms: Option<u64>,
    pub local_ms: Option<u64>,
}

pub type CloudResult = (Result<String, String>, u64);

pub struct LateJobs {
    pub pending: Vec<(usize, Receiver<CloudResult>)>,
}

pub struct SessionResult {
    pub audio_secs: f32,
    pub speech_detected: bool,
    pub preview: String,
    pub chunks: Vec<ChunkOutcome>,
    pub text: String,
    pub source: TextSource,
    pub first_partial_ms: Option<u64>,
    /// Time from Finish to merged text.
    pub finalize_ms: u64,
    pub late: Option<LateJobs>,
}

struct ChunkJob {
    start: usize,
    end: usize,
    local_rx: Option<Receiver<(Option<String>, u64)>>,
    cloud_rx: Option<Receiver<CloudResult>>,
    outcome: ChunkOutcome,
    local_done: bool,
    cloud_done: bool,
}

pub struct Session {
    pub audio_tx: Sender<AudioChunk>,
    ctrl_tx: Sender<Ctrl>,
    join: Option<JoinHandle<Option<SessionResult>>>,
}

impl Session {
    pub fn start(params: SessionParams, observer: Arc<dyn SessionObserver>) -> Session {
        let (audio_tx, audio_rx) = unbounded::<AudioChunk>();
        let (ctrl_tx, ctrl_rx) = unbounded::<Ctrl>();
        let join = std::thread::Builder::new()
            .name("session".into())
            .spawn(move || run(params, observer, audio_rx, ctrl_rx))
            .expect("spawn session");
        Session { audio_tx, ctrl_tx, join: Some(join) }
    }

    /// Stop capturing and wait for the merged result.
    pub fn finish(mut self) -> Option<SessionResult> {
        let _ = self.ctrl_tx.send(Ctrl::Finish);
        self.join.take().and_then(|j| j.join().ok()).flatten()
    }

    pub fn cancel(mut self) {
        let _ = self.ctrl_tx.send(Ctrl::Cancel);
        if let Some(j) = self.join.take() {
            std::thread::spawn(move || {
                let _ = j.join();
            });
        }
    }
}

struct Engine {
    params: SessionParams,
    observer: Arc<dyn SessionObserver>,
    audio: Vec<f32>,
    resampler: Option<(u32, LinearResampler)>,
    stream: Option<sherpa_onnx::OnlineStream>,
    vad: Option<sherpa_onnx::VoiceActivityDetector>,
    vad_buf: Vec<f32>,
    vad_fed: usize,
    speech_detected: bool,
    chunk_begin: usize,
    pending_speech: Option<(usize, usize)>,
    jobs: Vec<ChunkJob>,
    preview: String,
    started: Instant,
    first_partial_ms: Option<u64>,
}

impl Engine {
    fn push_audio(&mut self, chunk: AudioChunk) {
        let samples = if chunk.sample_rate as usize == SR {
            chunk.samples
        } else {
            let need_new = !matches!(&self.resampler, Some((sr, _)) if *sr == chunk.sample_rate);
            if need_new {
                self.resampler = LinearResampler::create(chunk.sample_rate as i32, SAMPLE_RATE)
                    .map(|r| (chunk.sample_rate, r));
            }
            match &self.resampler {
                Some((_, r)) => r.resample(&chunk.samples, false),
                None => return,
            }
        };
        if samples.is_empty() {
            return;
        }
        self.audio.extend_from_slice(&samples);

        if let Some(stream) = &self.stream {
            if let Some(t) = self.params.local.feed(stream, &samples) {
                if t != self.preview {
                    if self.first_partial_ms.is_none() && !t.is_empty() {
                        self.first_partial_ms = Some(self.started.elapsed().as_millis() as u64);
                    }
                    self.preview = t;
                    self.observer.on_partial(&self.preview);
                }
            }
        }
        self.feed_vad(false);
    }

    fn feed_vad(&mut self, flush: bool) {
        let Some(vad) = &self.vad else { return };
        // Feed exact windows from the 16k buffer.
        while self.vad_fed + VAD_WINDOW <= self.audio.len() {
            self.vad_buf.clear();
            self.vad_buf.extend_from_slice(&self.audio[self.vad_fed..self.vad_fed + VAD_WINDOW]);
            vad.accept_waveform(&self.vad_buf);
            self.vad_fed += VAD_WINDOW;
        }
        if flush {
            vad.flush();
        }
        let mut segments = Vec::new();
        while !vad.is_empty() {
            if let Some(seg) = vad.front() {
                let start = seg.start().max(0) as usize;
                segments.push((start, start + seg.n().max(0) as usize));
            }
            vad.pop();
        }
        for (s, e) in segments {
            self.speech_detected = true;
            self.pending_speech = Some(match self.pending_speech {
                Some((ps, _)) => (ps, e),
                None => (s, e),
            });
            let (ps, pe) = self.pending_speech.unwrap();
            if !flush && (pe - ps) as f32 / SR as f32 >= self.params.min_chunk_secs {
                let end = (pe + SR / 4).min(self.audio.len());
                self.dispatch(end);
            }
        }
    }

    /// Dispatch audio [chunk_begin, end) as one chunk.
    fn dispatch(&mut self, end: usize) {
        let mut start = self.chunk_begin;
        if self.jobs.is_empty() {
            if let Some((ps, _)) = self.pending_speech {
                start = ps.saturating_sub(SR * 3 / 10);
            }
        }
        self.pending_speech = None;
        if end <= start {
            return;
        }
        self.chunk_begin = end;
        let samples: Vec<f32> = self.audio[start..end].to_vec();
        let index = self.jobs.len();
        let secs = samples.len() as f32 / SR as f32;
        self.observer.on_chunk_dispatched(index, secs);

        // Local accurate pass (separate thread; SenseVoice is fast).
        let local_rx = if self.params.local.has_final() {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let local = self.params.local.clone();
            let s = samples.clone();
            std::thread::spawn(move || {
                let t = Instant::now();
                let r = local.transcribe(&s);
                let _ = tx.send((r, t.elapsed().as_millis() as u64));
            });
            Some(rx)
        } else {
            None
        };

        // Cloud pass.
        let cloud_rx = self.params.cloud.clone().map(|cfg| {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let client = self.params.http.clone();
            let timeout = self.params.cloud_timeout;
            let wav = cloud::wav_bytes(&samples);
            self.params.rt.spawn(async move {
                let t = Instant::now();
                let r = cloud::transcribe(&client, &cfg, wav, timeout).await;
                let _ = tx.send((r, t.elapsed().as_millis() as u64));
            });
            rx
        });

        self.jobs.push(ChunkJob {
            start,
            end,
            local_done: local_rx.is_none(),
            cloud_done: cloud_rx.is_none(),
            local_rx,
            cloud_rx,
            outcome: ChunkOutcome {
                start_s: start as f32 / SR as f32,
                end_s: end as f32 / SR as f32,
                local: None,
                cloud: None,
                cloud_error: None,
                cloud_ms: None,
                local_ms: None,
            },
        });
    }

    fn poll_jobs(&mut self) {
        for j in &mut self.jobs {
            if !j.local_done {
                if let Some(rx) = &j.local_rx {
                    if let Ok((r, ms)) = rx.try_recv() {
                        j.outcome.local = r;
                        j.outcome.local_ms = Some(ms);
                        j.local_done = true;
                    }
                }
            }
            if !j.cloud_done {
                if let Some(rx) = &j.cloud_rx {
                    if let Ok((r, ms)) = rx.try_recv() {
                        apply_cloud(&mut j.outcome, r, ms);
                        j.cloud_done = true;
                    }
                }
            }
        }
    }

    fn finish(mut self) -> SessionResult {
        let finish_at = Instant::now();
        // Flush resampler tail.
        if let Some((_, r)) = &self.resampler {
            let tail = r.resample(&[], true);
            self.audio.extend_from_slice(&tail);
        }
        let final_preview = self
            .stream
            .as_ref()
            .and_then(|s| self.params.local.finish(s))
            .unwrap_or_default();
        if !final_preview.is_empty() {
            self.preview = final_preview;
        }
        self.feed_vad(true);

        let total = self.audio.len();
        let audio_secs = total as f32 / SR as f32;
        let has_vad = self.vad.is_some();
        if self.pending_speech.is_some() {
            self.dispatch(total);
        } else if self.jobs.is_empty() && total > SR / 4 && (!has_vad || !self.preview.is_empty()) {
            // No VAD available, or VAD heard nothing but the preview did.
            self.chunk_begin = 0;
            self.dispatch(total);
        }

        let deadline = finish_at + self.params.deadline;
        let hard_stop = deadline + Duration::from_millis(2500);
        loop {
            self.poll_jobs();
            let locals = self.jobs.iter().all(|j| j.local_done);
            let clouds = self.jobs.iter().all(|j| j.cloud_done);
            let now = Instant::now();
            if (locals && clouds) || (locals && now >= deadline) || now >= hard_stop {
                break;
            }
            std::thread::sleep(Duration::from_millis(4));
        }

        let chunks: Vec<ChunkOutcome> = self.jobs.iter().map(|j| j.outcome.clone()).collect();
        let (text, source) = merge(&chunks, &self.preview);
        let pending: Vec<(usize, Receiver<CloudResult>)> = self
            .jobs
            .iter_mut()
            .enumerate()
            .filter(|(_, j)| !j.cloud_done)
            .filter_map(|(i, j)| j.cloud_rx.take().map(|rx| (i, rx)))
            .collect();
        let _ = (self.jobs.first().map(|j| j.start), self.jobs.last().map(|j| j.end));

        SessionResult {
            audio_secs,
            speech_detected: self.speech_detected || (!has_vad && !self.preview.is_empty()),
            preview: self.preview.clone(),
            chunks,
            text,
            source,
            first_partial_ms: self.first_partial_ms,
            finalize_ms: finish_at.elapsed().as_millis() as u64,
            late: if pending.is_empty() { None } else { Some(LateJobs { pending }) },
        }
    }
}

fn apply_cloud(o: &mut ChunkOutcome, r: Result<String, String>, ms: u64) {
    o.cloud_ms = Some(ms);
    match r {
        Ok(t) if !t.trim().is_empty() => o.cloud = Some(t.trim().to_string()),
        Ok(_) => o.cloud_error = Some("empty".into()),
        Err(e) => o.cloud_error = Some(e),
    }
}

/// Best available text per chunk: cloud > local SenseVoice.
pub fn merge(chunks: &[ChunkOutcome], preview: &str) -> (String, TextSource) {
    let mut parts = Vec::new();
    let (mut n_cloud, mut n_local) = (0, 0);
    for c in chunks {
        if let Some(t) = c.cloud.as_ref().filter(|t| !t.is_empty()) {
            parts.push(t.clone());
            n_cloud += 1;
        } else if let Some(t) = c.local.as_ref().filter(|t| !t.is_empty()) {
            parts.push(t.clone());
            n_local += 1;
        }
    }
    let text = text::join_segments(&parts);
    if text.is_empty() {
        if preview.trim().is_empty() {
            return (String::new(), TextSource::Empty);
        }
        return (preview.trim().to_string(), TextSource::Preview);
    }
    let src = match (n_cloud, n_local) {
        (c, 0) if c > 0 => TextSource::Cloud,
        (0, _) => TextSource::Local,
        _ => TextSource::Mixed,
    };
    (text, src)
}

/// Block until late cloud results arrive (or `window` elapses) and return the
/// upgraded merge if anything changed.
pub fn await_late(
    mut chunks: Vec<ChunkOutcome>,
    late: LateJobs,
    preview: &str,
    window: Duration,
) -> Option<(String, TextSource, Vec<ChunkOutcome>)> {
    let until = Instant::now() + window;
    let mut changed = false;
    for (i, rx) in late.pending {
        let left = until.saturating_duration_since(Instant::now());
        if let Ok((r, ms)) = rx.recv_timeout(left) {
            let before = chunks[i].cloud.clone();
            apply_cloud(&mut chunks[i], r, ms);
            changed |= chunks[i].cloud.is_some() && chunks[i].cloud != before;
        }
    }
    if !changed {
        return None;
    }
    let (t, s) = merge(&chunks, preview);
    Some((t, s, chunks))
}

fn run(
    params: SessionParams,
    observer: Arc<dyn SessionObserver>,
    audio_rx: Receiver<AudioChunk>,
    ctrl_rx: Receiver<Ctrl>,
) -> Option<SessionResult> {
    let stream = params.local.new_stream();
    let vad = params.local.new_vad();
    let mut eng = Engine {
        params,
        observer,
        audio: Vec::with_capacity(SR * 30),
        resampler: None,
        stream,
        vad,
        vad_buf: Vec::with_capacity(VAD_WINDOW),
        vad_fed: 0,
        speech_detected: false,
        chunk_begin: 0,
        pending_speech: None,
        jobs: Vec::new(),
        preview: String::new(),
        started: Instant::now(),
        first_partial_ms: None,
    };
    loop {
        crossbeam_channel::select! {
            recv(audio_rx) -> msg => match msg {
                Ok(chunk) => eng.push_audio(chunk),
                Err(_) => {
                    // Audio source closed: wait for an explicit control message.
                    match ctrl_rx.recv() {
                        Ok(Ctrl::Finish) => return Some(eng.finish()),
                        _ => return None,
                    }
                }
            },
            recv(ctrl_rx) -> msg => match msg {
                Ok(Ctrl::Finish) => {
                    while let Ok(chunk) = audio_rx.try_recv() {
                        eng.push_audio(chunk);
                    }
                    return Some(eng.finish());
                }
                _ => return None,
            },
        }
        eng.poll_jobs();
    }
}
