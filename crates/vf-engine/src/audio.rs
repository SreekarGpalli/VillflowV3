//! Microphone capture via cpal — CONTRACTS §5.
//!
//! Resolves the current Windows default device at each utterance when
//! `input_device == "system_default"`. Output: 16 kHz mono s16le chunks + RMS.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use tokio::sync::mpsc;

const TARGET_HZ: u32 = 16_000;

/// One chunk of captured audio for the STT pipeline.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    /// PCM 16 kHz mono s16le bytes.
    pub pcm_s16le: Vec<u8>,
    /// RMS level in 0.0..1.0 (for overlay pulse).
    pub rms: f32,
}

/// List input device names; first pseudo-entry is always `"System default"`.
pub fn list_input_devices() -> anyhow::Result<Vec<String>> {
    let host = cpal::default_host();
    let mut names = vec!["System default".to_string()];
    if let Ok(devices) = host.input_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                names.push(name);
            }
        }
    }
    Ok(names)
}

fn pick_device(input_device: &str) -> anyhow::Result<cpal::Device> {
    let host = cpal::default_host();
    if input_device == "system_default" || input_device == "System default" {
        return host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("no default input device"));
    }
    for d in host.input_devices()? {
        if d.name().map(|n| n == input_device).unwrap_or(false) {
            return Ok(d);
        }
    }
    Err(anyhow::anyhow!(
        "input device not found: {input_device}"
    ))
}

/// Live capture handle. Drop (or call [`CaptureHandle::stop`]) to end the stream.
pub struct CaptureHandle {
    stop: Arc<AtomicBool>,
    // Join the worker so the stream is dropped cleanly.
    join: Option<thread::JoinHandle<()>>,
}

impl CaptureHandle {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Start capturing from the configured device. Chunks are sent on `tx` until stopped.
pub fn start_capture(
    input_device: &str,
    tx: mpsc::UnboundedSender<AudioChunk>,
) -> anyhow::Result<CaptureHandle> {
    let device = pick_device(input_device)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();

    let join = thread::Builder::new()
        .name("vf-audio".into())
        .spawn(move || {
            if let Err(e) = run_capture_loop(device, tx, stop_flag) {
                log::error!("audio capture error: {e}");
            }
        })?;

    Ok(CaptureHandle {
        stop,
        join: Some(join),
    })
}

fn run_capture_loop(
    device: cpal::Device,
    tx: mpsc::UnboundedSender<AudioChunk>,
    stop: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let supported = device.default_input_config()?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.clone().into();
    let in_hz = config.sample_rate.0;
    let channels = config.channels as usize;

    // Larger bound reduces silent frame drops under resampler/consumer load.
    let (raw_tx, raw_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(256);

    let stream = match sample_format {
        SampleFormat::F32 => build_stream::<f32>(&device, &config, channels, raw_tx)?,
        SampleFormat::I16 => build_stream::<i16>(&device, &config, channels, raw_tx)?,
        SampleFormat::U16 => build_stream::<u16>(&device, &config, channels, raw_tx)?,
        other => anyhow::bail!("unsupported sample format: {other:?}"),
    };
    stream.play()?;

    // Resampler: process mono f32 frames → 16 kHz.
    let mut resampler = if in_hz != TARGET_HZ {
        let params = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 16,
            window: WindowFunction::BlackmanHarris2,
        };
        Some(SincFixedIn::<f32>::new(
            TARGET_HZ as f64 / in_hz as f64,
            2.0,
            params,
            512,
            1,
        )?)
    } else {
        None
    };

    let mut pending: Vec<f32> = Vec::new();

    while !stop.load(Ordering::SeqCst) {
        let chunk = match raw_rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(c) => c,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };

        let mono = if let Some(ref mut rs) = resampler {
            pending.extend(chunk);
            let chunk_size = rs.input_frames_next();
            let mut out_all = Vec::new();
            while pending.len() >= chunk_size {
                let input: Vec<f32> = pending.drain(..chunk_size).collect();
                let waves = vec![input];
                match rs.process(&waves, None) {
                    Ok(out) => {
                        if let Some(ch0) = out.into_iter().next() {
                            out_all.extend(ch0);
                        }
                    }
                    Err(e) => {
                        log::warn!("resample error: {e}");
                        break;
                    }
                }
            }
            out_all
        } else {
            chunk
        };

        if mono.is_empty() {
            continue;
        }

        let rms = rms_level(&mono);
        let pcm = f32_to_s16le(&mono);
        if tx
            .send(AudioChunk {
                pcm_s16le: pcm,
                rms,
            })
            .is_err()
        {
            break;
        }
    }

    // Flush residual resampler input so the end of the utterance is not clipped.
    if let Some(ref mut rs) = resampler {
        if !pending.is_empty() {
            let chunk_size = rs.input_frames_next();
            if chunk_size > 0 {
                while pending.len() < chunk_size {
                    pending.push(0.0);
                }
                let input: Vec<f32> = pending.drain(..chunk_size).collect();
                let waves = vec![input];
                if let Ok(out) = rs.process(&waves, None) {
                    if let Some(ch0) = out.into_iter().next() {
                        if !ch0.is_empty() {
                            let rms = rms_level(&ch0);
                            let pcm = f32_to_s16le(&ch0);
                            let _ = tx.send(AudioChunk {
                                pcm_s16le: pcm,
                                rms,
                            });
                        }
                    }
                }
            }
        }
    }

    drop(stream);
    Ok(())
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    raw_tx: std::sync::mpsc::SyncSender<Vec<f32>>,
) -> anyhow::Result<cpal::Stream>
where
    T: cpal::Sample + cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let err_fn = |e| log::error!("cpal stream error: {e}");
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            let mut mono = Vec::with_capacity(data.len() / channels.max(1));
            if channels <= 1 {
                for s in data {
                    mono.push(s.to_sample::<f32>());
                }
            } else {
                for frame in data.chunks(channels) {
                    let sum: f32 = frame.iter().map(|s| s.to_sample::<f32>()).sum();
                    mono.push(sum / channels as f32);
                }
            }
            let _ = raw_tx.try_send(mono);
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

fn rms_level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt().clamp(0.0, 1.0)
}

fn f32_to_s16le(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}
