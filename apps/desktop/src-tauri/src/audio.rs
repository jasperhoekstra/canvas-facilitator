//! Native audio capture: one microphone stream shared by both connections, resampled to
//! 24 kHz, with client VAD (PRD §7, §11). The facilitator answers in text, so there is no
//! audio output at all — a missing or broken speaker can never block the microphone.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

pub const RATE: u32 = 24_000;
const FRAME: usize = 480; // 20 ms at 24 kHz

#[derive(Serialize, Clone)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub default: bool,
}

pub fn input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let id = |d: &cpal::Device| d.id().map(|i| i.to_string()).unwrap_or_default();
    let name = |d: &cpal::Device| d.description().map(|x| x.to_string()).unwrap_or_else(|_| "Onbekend apparaat".into());
    let def = host.default_input_device().map(|d| id(&d));
    host.input_devices()
        .map(|it| it.map(|d| DeviceInfo { id: id(&d), name: name(&d), default: Some(id(&d)) == def }).collect())
        .unwrap_or_default()
}

/// Requested microphone, or the default one (second value: fell back).
fn pick(host: &cpal::Host, want: &Option<String>) -> Result<(cpal::Device, bool), String> {
    if let Some(d) = want.as_ref().and_then(|w| w.parse::<cpal::DeviceId>().ok()).and_then(|id| host.device_by_id(&id)) {
        return Ok((d, false));
    }
    host.default_input_device().map(|d| (d, want.is_some())).ok_or_else(|| "Geen microfoon gevonden".to_string())
}

/// Streaming linear resampler. ponytail: linear interpolation is fine for speech; swap for a
/// windowed-sinc resampler if WER tests show aliasing artefacts.
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Resampler { step: from as f64 / to as f64, pos: 0.0, prev: 0.0 }
    }
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        // pos is relative to `prev` (index -1) .. input
        while self.pos < input.len() as f64 {
            let i = self.pos.floor() as isize;
            let frac = (self.pos - i as f64) as f32;
            let a = if i < 0 { self.prev } else { input[i as usize] };
            let b = if i + 1 < input.len() as isize { input[(i + 1) as usize] } else { a };
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= input.len() as f64;
        self.prev = *input.last().unwrap();
    }
}

#[derive(Debug)]
pub enum AudioEvent {
    /// 0..1 input level, ~10 Hz.
    Level(f32),
    SpeechStart,
    /// 24 kHz PCM16 LE, only during detected speech (plus pre-roll).
    Frame(Vec<u8>),
    SpeechEnd { duration_ms: u64 },
    /// Still speaking, but long enough to process what was said so far (near-real-time canvas).
    SpeechChunk { duration_ms: u64 },
    /// Too short to be a turn; buffered audio should be cleared.
    SpeechDiscard,
    DeviceFallback(String),
    DeviceError(String),
}

/// Energy VAD with adaptive noise floor. ponytail: energy-based; swap for a model VAD
/// (e.g. Silero) if missed or split turns show up in real sessions.
pub struct Vad {
    floor: f32,
    speaking: bool,
    above: u32,
    below: u32,
    /// Frames since the turn (or the last chunk) started.
    chunk_frames: u32,
    preroll: VecDeque<Vec<u8>>,
}

const START_FRAMES: u32 = 3; // 60 ms
const END_FRAMES: u32 = 25; // 500 ms silence ends a turn
const CHUNK_FRAMES: u32 = 200; // after 4 s of speech, split at the next short pause…
const CHUNK_PAUSE_FRAMES: u32 = 8; // …of 160 ms
const MAX_CHUNK_FRAMES: u32 = 400; // or after 8 s regardless
const MIN_SPEECH_FRAMES: u32 = 15; // 300 ms
const PREROLL_FRAMES: usize = 15;

impl Default for Vad {
    fn default() -> Self {
        Vad { floor: 0.004, speaking: false, above: 0, below: 0, chunk_frames: 0, preroll: VecDeque::new() }
    }
}

impl Vad {
    /// Feed one 20 ms frame; returns events to emit.
    pub fn frame(&mut self, f: &[f32]) -> Vec<AudioEvent> {
        let rms = (f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32).sqrt();
        let pcm: Vec<u8> = f.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
        let loud = rms > (self.floor * 3.2).max(0.012);
        let mut ev = Vec::new();
        if !self.speaking {
            self.floor = if loud { self.floor } else { self.floor * 0.95 + rms * 0.05 }.max(0.0005);
            self.above = if loud { self.above + 1 } else { 0 };
            self.preroll.push_back(pcm);
            if self.preroll.len() > PREROLL_FRAMES {
                self.preroll.pop_front();
            }
            if self.above >= START_FRAMES {
                self.speaking = true;
                self.below = 0;
                self.chunk_frames = self.above;
                ev.push(AudioEvent::SpeechStart);
                ev.extend(self.preroll.drain(..).map(AudioEvent::Frame));
            }
        } else {
            self.chunk_frames += 1;
            self.below = if loud { 0 } else { self.below + 1 };
            ev.push(AudioEvent::Frame(pcm));
            let pause = self.below >= CHUNK_PAUSE_FRAMES && self.below < END_FRAMES;
            if (self.chunk_frames >= CHUNK_FRAMES && pause) || self.chunk_frames >= MAX_CHUNK_FRAMES {
                ev.push(AudioEvent::SpeechChunk { duration_ms: self.chunk_frames as u64 * 20 });
                self.chunk_frames = 0;
            } else if self.below >= END_FRAMES {
                self.speaking = false;
                self.above = 0;
                // Only what was said since the last chunk counts: a silent tail after a chunk is discarded.
                let voiced = self.chunk_frames.saturating_sub(self.below);
                ev.push(if voiced >= MIN_SPEECH_FRAMES {
                    AudioEvent::SpeechEnd { duration_ms: self.chunk_frames as u64 * 20 }
                } else {
                    AudioEvent::SpeechDiscard
                });
            }
        }
        ev
    }

    pub fn reset(&mut self) -> bool {
        let was = self.speaking;
        *self = Vad { floor: self.floor, ..Default::default() };
        was
    }
}

pub type Sink = Arc<dyn Fn(AudioEvent) + Send + Sync>;

pub struct AudioHandle {
    stop: mpsc::Sender<()>,
    pub reset_vad: Arc<AtomicBool>,
}

impl Drop for AudioHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

fn to_mono<T: Sample>(data: &[T], ch: usize) -> Vec<f32>
where
    f32: FromSample<T>,
{
    data.chunks(ch).map(|fr| fr.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / ch as f32).collect()
}

fn build_in<T>(dev: &cpal::Device, cfg: cpal::StreamConfig, tx: mpsc::SyncSender<Vec<f32>>, sink: Sink) -> Result<cpal::Stream, String>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let ch = cfg.channels as usize;
    dev.build_input_stream(
        cfg,
        move |d: &[T], _: &cpal::InputCallbackInfo| {
            let _ = tx.try_send(to_mono(d, ch)); // never block the audio thread
        },
        move |e: cpal::Error| match e.kind() {
            // Driver-level glitches (buffer under/overrun, no realtime priority): capture keeps
            // running, so log only. Showing them as errors alarmed users mid-presentation.
            cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied => crate::diag::log(format!("audio glitch: {e}")),
            _ => sink(AudioEvent::DeviceError(format!("Microfoonfout: {e}"))),
        },
        None,
    )
    .map_err(|e| format!("Microfoon openen mislukt: {e}"))
}

/// Start capture on a dedicated thread (cpal streams stay on the thread that made them).
/// `capture` is the session's gate: when false (muted/paused/closed) audio is dropped.
pub fn start(input: Option<String>, capture: Arc<AtomicBool>, sink: Sink) -> Result<AudioHandle, String> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let reset_vad = Arc::new(AtomicBool::new(false));
    let reset2 = reset_vad.clone();
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            let host = cpal::default_host();
            let setup = (|| -> Result<(cpal::Stream, mpsc::Receiver<Vec<f32>>, u32), String> {
                let (dev, fell_back) = pick(&host, &input)?;
                if fell_back {
                    sink(AudioEvent::DeviceFallback("Gekozen microfoon niet gevonden; standaardmicrofoon gebruikt".into()));
                }
                let cfg = dev.default_input_config().map_err(|e| format!("Microfoon niet beschikbaar of geen toestemming: {e}"))?;
                let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(64);
                let stream = match cfg.sample_format() {
                    cpal::SampleFormat::F32 => build_in::<f32>(&dev, cfg.config(), tx, sink.clone()),
                    cpal::SampleFormat::I16 => build_in::<i16>(&dev, cfg.config(), tx, sink.clone()),
                    cpal::SampleFormat::U16 => build_in::<u16>(&dev, cfg.config(), tx, sink.clone()),
                    cpal::SampleFormat::I32 => build_in::<i32>(&dev, cfg.config(), tx, sink.clone()),
                    other => Err(format!("Niet-ondersteund sampleformaat {other:?}")),
                }?;
                stream.play().map_err(|e| format!("Microfoon starten mislukt: {e}"))?;
                Ok((stream, rx, cfg.sample_rate()))
            })();
            let (_stream, rx, in_rate) = match setup {
                Ok(v) => v,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            let mut rs = Resampler::new(in_rate, RATE);
            let mut buf: Vec<f32> = Vec::new();
            let mut vad = Vad::default();
            let mut level_acc = 0f32;
            let mut level_n = 0;
            let mut gated = false;
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                let chunk = match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(c) => c,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                if reset2.swap(false, Ordering::SeqCst) {
                    vad.reset();
                    buf.clear();
                }
                if !capture.load(Ordering::SeqCst) {
                    // Muted/paused/closed: drop audio, never catch up later.
                    if vad.reset() {
                        sink(AudioEvent::SpeechDiscard);
                    }
                    buf.clear();
                    if !gated {
                        gated = true;
                        sink(AudioEvent::Level(0.0));
                    }
                    continue;
                }
                gated = false;
                rs.process(&chunk, &mut buf);
                while buf.len() >= FRAME {
                    let f: Vec<f32> = buf.drain(..FRAME).collect();
                    let rms = (f.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
                    level_acc = level_acc.max(rms);
                    level_n += 1;
                    if level_n == 5 {
                        sink(AudioEvent::Level((level_acc * 6.0).min(1.0)));
                        level_acc = 0.0;
                        level_n = 0;
                    }
                    for e in vad.frame(&f) {
                        sink(e);
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    ready_rx.recv().map_err(|_| "Audiothread gestopt".to_string())??;
    Ok(AudioHandle { stop: stop_tx, reset_vad })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_rate_and_continuity() {
        let mut r = Resampler::new(48_000, 24_000);
        let mut out = Vec::new();
        for _ in 0..10 {
            r.process(&vec![0.5; 4800], &mut out);
        }
        assert!((out.len() as i64 - 24_000).abs() <= 2, "{}", out.len());
        assert!(out[10..].iter().all(|x| (x - 0.5).abs() < 1e-6));
    }

    fn feed(v: &mut Vad, amp: f32, frames: usize) -> Vec<AudioEvent> {
        let f: Vec<f32> = (0..FRAME).map(|i| if i % 2 == 0 { amp } else { -amp }).collect();
        (0..frames).flat_map(|_| v.frame(&f)).collect()
    }

    #[test]
    fn vad_turn_lifecycle() {
        let mut v = Vad::default();
        assert!(feed(&mut v, 0.001, 50).is_empty());
        let ev = feed(&mut v, 0.2, 50);
        assert!(matches!(ev[0], AudioEvent::SpeechStart));
        assert!(ev.iter().filter(|e| matches!(e, AudioEvent::Frame(_))).count() >= 50);
        let ev = feed(&mut v, 0.001, 40);
        assert!(ev.iter().any(|e| matches!(e, AudioEvent::SpeechEnd { .. })));
        // a click is discarded
        feed(&mut v, 0.2, 4);
        let ev = feed(&mut v, 0.001, 40);
        assert!(ev.iter().any(|e| matches!(e, AudioEvent::SpeechDiscard)));
    }

    #[test]
    fn long_speech_is_chunked_at_pauses() {
        let mut v = Vad::default();
        feed(&mut v, 0.001, 50);
        let ev = feed(&mut v, 0.2, 210);
        assert!(!ev.iter().any(|e| matches!(e, AudioEvent::SpeechChunk { .. })), "no split mid-word");
        let ev = feed(&mut v, 0.001, 10);
        assert!(ev.iter().any(|e| matches!(e, AudioEvent::SpeechChunk { .. })), "split at a short pause");
        assert!(!ev.iter().any(|e| matches!(e, AudioEvent::SpeechEnd { .. })));
        // without any pause, a hard split after 8 s
        let ev = feed(&mut v, 0.2, 410);
        assert!(ev.iter().any(|e| matches!(e, AudioEvent::SpeechChunk { .. })));
        let ev = feed(&mut v, 0.2, 20);
        assert!(ev.iter().all(|e| matches!(e, AudioEvent::Frame(_))));
        let ev = feed(&mut v, 0.001, 30);
        assert!(ev.iter().any(|e| matches!(e, AudioEvent::SpeechEnd { .. })));
    }
}
