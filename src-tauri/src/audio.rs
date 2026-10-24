//! Microphone capture on a dedicated thread (cpal / CoreAudio).
//!
//! The input stream is opened only while the user is dictating, so the
//! orange "microphone in use" indicator in the menu bar exactly matches the
//! recording state.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Mono f32 samples at the device's native rate.
pub struct AudioChunk {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
}

enum Cmd {
    Start {
        sink: Sender<AudioChunk>,
        reply: Sender<Result<AudioInfo, String>>,
    },
    Stop,
}

pub struct Recorder {
    cmd: Sender<Cmd>,
    /// Latest RMS level (f32 bits), 0..1.
    pub level: Arc<AtomicU32>,
}

impl Recorder {
    pub fn new() -> Self {
        let (tx, rx) = bounded::<Cmd>(8);
        let level = Arc::new(AtomicU32::new(0));
        let lv = level.clone();
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || audio_thread(rx, lv))
            .expect("spawn audio thread");
        Recorder { cmd: tx, level }
    }

    pub fn start(&self, sink: Sender<AudioChunk>) -> Result<AudioInfo, String> {
        let (rtx, rrx) = bounded(1);
        self.cmd
            .send(Cmd::Start { sink, reply: rtx })
            .map_err(|_| "音频线程不可用".to_string())?;
        rrx.recv_timeout(Duration::from_secs(4))
            .map_err(|_| "打开麦克风超时".to_string())?
    }

    pub fn stop(&self) {
        let _ = self.cmd.send(Cmd::Stop);
        self.level.store(0f32.to_bits(), Ordering::Relaxed);
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}

fn audio_thread(rx: Receiver<Cmd>, level: Arc<AtomicU32>) {
    let mut stream: Option<cpal::Stream> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start { sink, reply } => {
                stream = None; // close any previous stream first
                match open_stream(sink, level.clone()) {
                    Ok((s, info)) => {
                        stream = Some(s);
                        let _ = reply.send(Ok(info));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Cmd::Stop => {
                if let Some(s) = stream.take() {
                    let _ = s.pause();
                    drop(s);
                }
            }
        }
    }
}

fn open_stream(
    sink: Sender<AudioChunk>,
    level: Arc<AtomicU32>,
) -> Result<(cpal::Stream, AudioInfo), String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "没有可用的麦克风".to_string())?;
    let name = device.name().unwrap_or_else(|_| "Microphone".into());
    let supported = device
        .default_input_config()
        .map_err(|e| format!("读取麦克风配置失败：{e}"))?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate.0;

    let err_fn = |e| log::error!("audio stream error: {e}");
    let stream = match sample_format {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, channels, sample_rate, sink, level, err_fn),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, channels, sample_rate, sink, level, err_fn),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, channels, sample_rate, sink, level, err_fn),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, channels, sample_rate, sink, level, err_fn),
        other => return Err(format!("不支持的采样格式：{other:?}")),
    }
    .map_err(|e| format!("打开麦克风失败：{e}"))?;
    stream.play().map_err(|e| format!("启动录音失败：{e}"))?;
    Ok((
        stream,
        AudioInfo { device: name, sample_rate, channels: channels as u16 },
    ))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    sample_rate: u32,
    sink: Sender<AudioChunk>,
    level: Arc<AtomicU32>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    use cpal::Sample;
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let frames = data.len() / channels.max(1);
            let mut mono = Vec::with_capacity(frames);
            let mut sum_sq = 0f32;
            for frame in data.chunks(channels.max(1)) {
                let mut acc = 0f32;
                for s in frame {
                    acc += f32::from_sample(*s);
                }
                let v = acc / frame.len() as f32;
                sum_sq += v * v;
                mono.push(v);
            }
            if frames > 0 {
                let rms = (sum_sq / frames as f32).sqrt();
                level.store(rms.to_bits(), Ordering::Relaxed);
            }
            let _ = sink.try_send(AudioChunk { samples: mono, sample_rate });
        },
        err_fn,
        None,
    )
}
