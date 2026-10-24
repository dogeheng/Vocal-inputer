//! Pipeline benchmark: plays a WAV file into a dictation session in real
//! time (or faster) and reports preview latency, final latency and text.
//!
//!   cargo run --bin vi_bench -- <file.wav> [--no-cloud] [--fast] [--deadline 1000]
//!
//! The OpenRouter key is read from the Keychain and never printed.

use std::sync::Arc;
use std::time::{Duration, Instant};
use voice_input_lib::{audio::AudioChunk, cloud, local_asr, models, pipeline, secrets, settings::Settings, text};

struct Printer(Instant);
impl pipeline::SessionObserver for Printer {
    fn on_partial(&self, t: &str) {
        println!("  [{:>5} ms] partial: {}", self.0.elapsed().as_millis(), t);
    }
    fn on_chunk_dispatched(&self, i: usize, secs: f32) {
        println!("  [{:>5} ms] chunk #{i} dispatched ({secs:.2}s audio)", self.0.elapsed().as_millis());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: vi_bench <file.wav> [--no-cloud] [--fast] [--deadline ms]");
    let no_cloud = args.iter().any(|a| a == "--no-cloud");
    let fast = args.iter().any(|a| a == "--fast");
    let deadline = args
        .iter()
        .position(|a| a == "--deadline")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000u64);

    let home = std::env::var("HOME").unwrap();
    let data_dir = std::path::PathBuf::from(home).join("Library/Application Support/com.peihengli.voiceinput");
    let roots = models::search_roots(&data_dir);
    let paths = local_asr::ModelPaths {
        preview_dir: models::resolve(&models::PREVIEW_LARGE, &roots).or_else(|| models::resolve_small_preview(&roots)),
        final_dir: args
            .iter()
            .position(|a| a == "--final-dir")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from)
            .or_else(|| models::resolve(&models::FINAL_SENSEVOICE, &roots)),
        vad_file: models::resolve(&models::VAD, &roots).map(|d| d.join("silero_vad.onnx")),
    };
    let t = Instant::now();
    let local = Arc::new(local_asr::LocalAsr::load(&paths));
    println!("models: preview={} final={} vad={} (load {} ms)", local.has_preview(), local.has_final(), local.has_vad(), t.elapsed().as_millis());

    let mut reader = hound::WavReader::open(path).expect("open wav");
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader.samples::<i32>().map(|s| s.unwrap() as f32 / (1i64 << (spec.bits_per_sample - 1)) as f32).collect(),
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
    };
    let mono: Vec<f32> = samples.chunks(spec.channels as usize).map(|f| f.iter().sum::<f32>() / f.len() as f32).collect();
    println!("audio: {:.2}s @ {} Hz", mono.len() as f32 / spec.sample_rate as f32, spec.sample_rate);

    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let s = Settings::default();
    let http = cloud::http_client("");
    let key = if no_cloud { None } else { secrets::openrouter_key() };
    println!("cloud: {}", if key.is_some() { s.cloud_model.as_str() } else { "disabled" });
    let cloud_cfg = key.clone().map(|api_key| cloud::CloudConfig {
        base_url: s.cloud_base_url.clone(),
        style: s.cloud_api_style,
        model: s.cloud_model.clone(),
        language: None,
        api_key,
    });
    if let Some(c) = &cloud_cfg {
        rt.block_on(cloud::warm_up(&http, &c.base_url, &c.api_key));
    }
    let params = pipeline::SessionParams {
        local,
        rt: rt.handle().clone(),
        http: http.clone(),
        cloud: cloud_cfg,
        deadline: Duration::from_millis(deadline),
        min_chunk_secs: 2.5,
        cloud_timeout: Duration::from_secs(15),
    };
    let start = Instant::now();
    let session = pipeline::Session::start(params, Arc::new(Printer(start)));
    let step = spec.sample_rate as usize / 50; // 20 ms
    for (i, c) in mono.chunks(step).enumerate() {
        let _ = session.audio_tx.send(AudioChunk { samples: c.to_vec(), sample_rate: spec.sample_rate });
        if !fast {
            let due = start + Duration::from_millis(20 * (i as u64 + 1));
            if let Some(d) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(d);
            }
        }
    }
    let released = Instant::now();
    println!("  [{:>5} ms] -- released --", start.elapsed().as_millis());
    let r = session.finish().expect("result");
    let wait = released.elapsed().as_millis();
    for (i, c) in r.chunks.iter().enumerate() {
        println!(
            "  chunk #{i} {:.2}-{:.2}s local({:?}ms)={:?} cloud({:?}ms)={:?} err={:?}",
            c.start_s, c.end_s, c.local_ms, c.local, c.cloud_ms, c.cloud, c.cloud_error
        );
    }
    let polished = text::rule_process(&r.text, &s);
    let mut cleaned = None;
    println!("needs_llm: {}", text::needs_llm(&r.text));
    if let Some(k) = &key {
        let t = Instant::now();
        let res = rt.block_on(cloud::cleanup(&http, &s.cloud_base_url, k, &s.cleanup_model, &polished, &s.dictionary, Duration::from_millis(s.cleanup_timeout_ms)));
        cleaned = Some((res, t.elapsed().as_millis()));
    }
    println!("first partial: {:?} ms after start", r.first_partial_ms);
    println!("release -> merged text: {wait} ms (finalize {} ms), source={:?}", r.finalize_ms, r.source);
    println!("preview : {}", r.preview);
    println!("merged  : {}", r.text);
    println!("rules   : {}", polished);
    if let Some((res, ms)) = cleaned {
        match res {
            Ok(t) => println!("cleanup : {} ({ms} ms)", text::finalize(&t, &s)),
            Err(e) => println!("cleanup : <fallback> {e} ({ms} ms)"),
        }
    }
    if let Some(late) = r.late {
        println!("late jobs pending: {}", late.pending.len());
        if let Some((t, src, _)) = pipeline::await_late(r.chunks.clone(), late, &r.preview, Duration::from_secs(10)) {
            println!("late upgrade ({src:?}): {t}");
        }
    }
}
