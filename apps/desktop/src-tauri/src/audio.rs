//! Native audio: one capture shared by both connections, client VAD, playback with
//! played-sample accounting for barge-in truncation (PRD §7, §11).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

pub const RATE: u32 = 24_000;
const FRAME: usize = 480; // 20 ms at 24 kHz

#[derive(Serialize, Clone)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub default: bool,
}

#[derive(Serialize, Clone)]
pub struct Devices {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

pub fn devices() -> Devices {
    let host = cpal::default_host();
    let id = |d: &cpal::Device| d.id().map(|i| i.to_string()).unwrap_or_default();
    let name = |d: &cpal::Device| d.description().map(|x| x.to_string()).unwrap_or_else(|_| "Onbekend apparaat".into());
    let di = host.default_input_device().map(|d| id(&d));
    let dout = host.default_output_device().map(|d| id(&d));
    let list = |it: Option<Vec<cpal::Device>>, def: &Option<String>| -> Vec<DeviceInfo> {
        it.unwrap_or_default()
            .iter()
            .map(|d| DeviceInfo { id: id(d), name: name(d), default: Some(id(d)) == *def })
            .collect()
    };
    Devices {
        inputs: list(host.input_devices().ok().map(|i| i.collect()), &di),
        outputs: list(host.output_devices().ok().map(|i| i.collect()), &dout),
    }
}

fn pick(host: &cpal::Host, want: &Option<String>, input: bool) -> Result<(cpal::Device, bool), String> {
    if let Some(w) = want {
        if let Ok(id) = w.parse::<cpal::DeviceId>() {
            if let Some(d) = host.device_by_id(&id) {
                return Ok((d, false));
            }
        }
    }
    let d = if input { host.default_input_device() } else { host.default_output_device() };
    d.map(|d| (d, want.is_some())).ok_or_else(|| if input { "Geen microfoon gevonden" } else { "Geen uitvoerapparaat gevonden" }.to_string())
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

/// Shared switches the session controller flips. All default to open for the mic test.
pub struct Gate {
    pub capture: AtomicBool,
    pub playback: AtomicBool,
}

impl Gate {
    pub fn open() -> Arc<Gate> {
        Arc::new(Gate { capture: AtomicBool::new(true), playback: AtomicBool::new(true) })
    }
    pub fn close_all(&self) {
        self.capture.store(false, Ordering::SeqCst);
        self.playback.store(false, Ordering::SeqCst);
    }
}

struct Segment {
    item_id: String,
    samples: VecDeque<f32>,
    played: u64,
}

#[derive(Default)]
struct PlayState {
    segments: VecDeque<Segment>,
    resampler: Option<(String, Resampler)>,
    device_rate: u32,
}

/// Assistant audio queue at device rate.
pub struct Playback {
    st: Mutex<PlayState>,
    gate: Arc<Gate>,
    /// Lock-free view for the VAD and UI, so they never contend with the output callback.
    playing: AtomicBool,
}

impl Playback {
    fn new(device_rate: u32, gate: Arc<Gate>) -> Arc<Self> {
        Arc::new(Playback { st: Mutex::new(PlayState { device_rate, ..Default::default() }), gate, playing: AtomicBool::new(false) })
    }

    /// Queue 24 kHz PCM16 mono for an assistant audio item.
    pub fn push(&self, item_id: &str, pcm16: &[u8]) {
        let input: Vec<f32> = pcm16.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        let mut st = self.st.lock().unwrap();
        let rate = st.device_rate;
        if st.resampler.as_ref().map(|r| r.0.as_str()) != Some(item_id) {
            st.resampler = Some((item_id.to_string(), Resampler::new(RATE, rate)));
        }
        let mut out = Vec::with_capacity(input.len() * rate as usize / RATE as usize + 2);
        st.resampler.as_mut().unwrap().1.process(&input, &mut out);
        match st.segments.back_mut() {
            Some(s) if s.item_id == item_id => s.samples.extend(out),
            _ => st.segments.push_back(Segment { item_id: item_id.into(), samples: out.into(), played: 0 }),
        }
        self.playing.store(true, Ordering::SeqCst);
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst)
    }

    /// Stop immediately. Returns (item_id, played_ms) for each item that still had audio queued.
    pub fn clear(&self) -> Vec<(String, u64)> {
        let mut st = self.st.lock().unwrap();
        let rate = st.device_rate as u64;
        let out = st
            .segments
            .iter()
            .filter(|s| !s.samples.is_empty())
            .map(|s| (s.item_id.clone(), s.played * 1000 / rate))
            .collect();
        st.segments.clear();
        st.resampler = None;
        self.playing.store(false, Ordering::SeqCst);
        out
    }

    fn fill(&self, out: &mut [f32]) {
        let mut st = self.st.lock().unwrap();
        if !self.gate.playback.load(Ordering::SeqCst) {
            st.segments.clear();
            self.playing.store(false, Ordering::SeqCst);
            out.fill(0.0);
            return;
        }
        for o in out.iter_mut() {
            loop {
                match st.segments.front_mut() {
                    Some(s) if s.samples.is_empty() => {
                        st.segments.pop_front();
                    }
                    Some(s) => {
                        *o = s.samples.pop_front().unwrap();
                        s.played += 1;
                        break;
                    }
                    None => {
                        *o = 0.0;
                        break;
                    }
                }
            }
        }
        if st.segments.is_empty() {
            self.playing.store(false, Ordering::SeqCst);
        }
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
    /// Too short to be a turn; buffered audio should be cleared.
    SpeechDiscard,
    DeviceFallback(String),
    DeviceError(String),
}

/// Energy VAD with adaptive noise floor. ponytail: energy-based; swap for a model VAD
/// (e.g. Silero) if self-triggering or missed turns show up in the AC-VOICE runs.
pub struct Vad {
    floor: f32,
    speaking: bool,
    above: u32,
    below: u32,
    speech_frames: u32,
    preroll: VecDeque<Vec<u8>>,
}

const START_FRAMES: u32 = 3; // 60 ms
const START_FRAMES_DURING_PLAYBACK: u32 = 8; // 160 ms: needs sustained speech to barge in
const END_FRAMES: u32 = 35; // 700 ms silence ends a turn
const MIN_SPEECH_FRAMES: u32 = 15; // 300 ms
const PREROLL_FRAMES: usize = 15;

impl Default for Vad {
    fn default() -> Self {
        Vad { floor: 0.004, speaking: false, above: 0, below: 0, speech_frames: 0, preroll: VecDeque::new() }
    }
}

impl Vad {
    /// Feed one 20 ms frame; returns events to emit.
    pub fn frame(&mut self, f: &[f32], playing: bool) -> Vec<AudioEvent> {
        let rms = (f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32).sqrt();
        let pcm: Vec<u8> = f.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
        // Echo suppression: while assistant audio plays, demand clearly louder speech.
        let factor = if playing { 9.0 } else { 3.2 };
        let thresh = (self.floor * factor).max(if playing { 0.03 } else { 0.012 });
        let loud = rms > thresh;
        let mut ev = Vec::new();
        if !self.speaking {
            self.floor = if loud { self.floor } else { self.floor * 0.95 + rms * 0.05 }.max(0.0005);
            self.above = if loud { self.above + 1 } else { 0 };
            self.preroll.push_back(pcm);
            if self.preroll.len() > PREROLL_FRAMES {
                self.preroll.pop_front();
            }
            if self.above >= if playing { START_FRAMES_DURING_PLAYBACK } else { START_FRAMES } {
                self.speaking = true;
                self.below = 0;
                self.speech_frames = self.above;
                ev.push(AudioEvent::SpeechStart);
                ev.extend(self.preroll.drain(..).map(AudioEvent::Frame));
            }
        } else {
            self.speech_frames += 1;
            self.below = if loud { 0 } else { self.below + 1 };
            ev.push(AudioEvent::Frame(pcm));
            if self.below >= END_FRAMES {
                self.speaking = false;
                self.above = 0;
                let voiced = self.speech_frames - self.below;
                ev.push(if voiced >= MIN_SPEECH_FRAMES {
                    AudioEvent::SpeechEnd { duration_ms: self.speech_frames as u64 * 20 }
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
    pub playback: Arc<Playback>,
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
        move |e| sink(AudioEvent::DeviceError(format!("Microfoonfout: {e}"))),
        None,
    )
    .map_err(|e| format!("Microfoon openen mislukt: {e}"))
}

fn build_out<T>(dev: &cpal::Device, cfg: cpal::StreamConfig, pb: Arc<Playback>, sink: Sink) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let ch = cfg.channels as usize;
    let mut mono = Vec::new();
    dev.build_output_stream(
        cfg,
        move |d: &mut [T], _: &cpal::OutputCallbackInfo| {
            mono.resize(d.len() / ch, 0.0);
            pb.fill(&mut mono);
            for (fr, v) in d.chunks_mut(ch).zip(mono.iter()) {
                fr.fill(T::from_sample(*v));
            }
        },
        move |e| sink(AudioEvent::DeviceError(format!("Uitvoerfout: {e}"))),
        None,
    )
    .map_err(|e| format!("Uitvoerapparaat openen mislukt: {e}"))
}

macro_rules! by_format {
    ($fmt:expr, $f:ident, $($a:expr),*) => {
        match $fmt {
            cpal::SampleFormat::F32 => $f::<f32>($($a),*),
            cpal::SampleFormat::I16 => $f::<i16>($($a),*),
            cpal::SampleFormat::U16 => $f::<u16>($($a),*),
            cpal::SampleFormat::I32 => $f::<i32>($($a),*),
            other => Err(format!("Niet-ondersteund sampleformaat {other:?}")),
        }
    };
}

/// Start capture + playback on a dedicated thread (cpal streams stay on the thread that made them).
pub fn start(input: Option<String>, output: Option<String>, gate: Arc<Gate>, sink: Sink) -> Result<AudioHandle, String> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<Arc<Playback>, String>>();
    let reset_vad = Arc::new(AtomicBool::new(false));
    let reset2 = reset_vad.clone();
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            let host = cpal::default_host();
            let setup = (|| -> Result<(cpal::Stream, cpal::Stream, Arc<Playback>, mpsc::Receiver<Vec<f32>>, u32), String> {
                let (idev, ifb) = pick(&host, &input, true)?;
                let (odev, ofb) = pick(&host, &output, false)?;
                if ifb || ofb {
                    sink(AudioEvent::DeviceFallback("Gekozen audioapparaat niet gevonden; standaardapparaat gebruikt".into()));
                }
                let icfg = idev.default_input_config().map_err(|e| format!("Microfoon niet beschikbaar of geen toestemming: {e}"))?;
                let ocfg = odev.default_output_config().map_err(|e| format!("Uitvoerapparaat niet beschikbaar: {e}"))?;
                let in_rate = icfg.sample_rate();
                let pb = Playback::new(ocfg.sample_rate(), gate.clone());
                let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(64);
                let is = by_format!(icfg.sample_format(), build_in, &idev, icfg.config(), tx, sink.clone())?;
                let os = by_format!(ocfg.sample_format(), build_out, &odev, ocfg.config(), pb.clone(), sink.clone())?;
                is.play().map_err(|e| format!("Microfoon starten mislukt: {e}"))?;
                os.play().map_err(|e| format!("Uitvoer starten mislukt: {e}"))?;
                Ok((is, os, pb, rx, in_rate))
            })();
            let (_is, _os, pb, rx, in_rate) = match setup {
                Ok(v) => v,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(pb.clone()));
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
                if !gate.capture.load(Ordering::SeqCst) {
                    // Muted/paused/expired: drop audio, never catch up later.
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
                    for e in vad.frame(&f, pb.is_playing()) {
                        sink(e);
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let playback = ready_rx.recv().map_err(|_| "Audiothread gestopt".to_string())??;
    Ok(AudioHandle { stop: stop_tx, playback, reset_vad })
}

/// 0.6 s 440 Hz tone for the speaker test.
pub fn tone() -> Vec<u8> {
    (0..RATE as usize * 6 / 10)
        .map(|i| {
            let env = (i as f32 / 1200.0).min(1.0) * ((RATE as usize * 6 / 10 - i) as f32 / 1200.0).min(1.0);
            ((i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * 0.25 * env * 32767.0) as i16
        })
        .flat_map(|s| s.to_le_bytes())
        .collect()
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

    fn feed(v: &mut Vad, amp: f32, frames: usize, playing: bool) -> Vec<AudioEvent> {
        let f: Vec<f32> = (0..FRAME).map(|i| if i % 2 == 0 { amp } else { -amp }).collect();
        (0..frames).flat_map(|_| v.frame(&f, playing)).collect()
    }

    #[test]
    fn vad_turn_lifecycle() {
        let mut v = Vad::default();
        assert!(feed(&mut v, 0.001, 50, false).is_empty());
        let ev = feed(&mut v, 0.2, 50, false);
        assert!(matches!(ev[0], AudioEvent::SpeechStart));
        assert!(ev.iter().filter(|e| matches!(e, AudioEvent::Frame(_))).count() >= 50);
        let ev = feed(&mut v, 0.001, 40, false);
        assert!(ev.iter().any(|e| matches!(e, AudioEvent::SpeechEnd { .. })));
        // a click is discarded
        feed(&mut v, 0.2, 4, false);
        let ev = feed(&mut v, 0.001, 40, false);
        assert!(ev.iter().any(|e| matches!(e, AudioEvent::SpeechDiscard)));
    }

    #[test]
    fn vad_ignores_quiet_echo_during_playback() {
        let mut v = Vad::default();
        feed(&mut v, 0.001, 50, false);
        assert!(feed(&mut v, 0.02, 50, true).iter().all(|e| !matches!(e, AudioEvent::SpeechStart)));
    }

    #[test]
    fn playback_counts_played_and_gate_mutes() {
        let g = Gate::open();
        let pb = Playback::new(24_000, g.clone());
        pb.push("a", &vec![0u8; 24_000 * 2]); // 1 s
        let mut out = vec![0f32; 12_000];
        pb.fill(&mut out);
        let c = pb.clear();
        assert_eq!(c, vec![("a".to_string(), 500)]);
        pb.push("b", &vec![1u8; 4800]);
        g.playback.store(false, Ordering::SeqCst);
        let mut out = vec![1f32; 100];
        pb.fill(&mut out);
        assert!(out.iter().all(|x| *x == 0.0) && !pb.is_playing());
    }
}
