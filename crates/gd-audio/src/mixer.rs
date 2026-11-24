//! The mixer ([`Bus`]): every sound entity's [`Voice`] is summed in blocks of [`BLOCK`] frames on the audio thread,
//! and a port-owned rodio sink plays that one stereo stream at the selected device's rate ([`start_bus`]):
//! bevy_audio 0.20 opens only the default sink (no public device hook). A `soundDevice` change reopens the sink
//! without dropping the voice queue or the playing voices ([`switch_device`], `devices`).
//!
//! PORT (deliberate): rodio plays every sink through a chain of generic per-sample sources (speed, position, pause,
//! volume, the mixer's resampler); a fight's 60+ voices starved the device callback that way. The bus costs well under
//! 0.1% of a core per voice. WAVs up to [`BUFFER_BYTES`] are decoded once, in the background, and then play from
//! memory (the engine's sound buffers); `.ogg` files and bigger WAVs are decoded while they play (the engine's
//! streams), a looped one's next pass from a decoder built on the game thread ([`Spare`]). The pitch / rate
//! conversion of each file is linear interpolation, as rodio's. 2D sounds (music, ambient loops, dialog, stereo files:
//! the engine's `Play2D` / `StartStream`) pass through without ear gains.
use super::Sounds;
use super::cache::Loaded;
use super::devices::{self, AudioDevices};
use super::listener::{BaseVolume, SoundClass, VOICES, sound};
use bevy::audio::{ChannelCount, Decodable, Sample, SampleRate, Source};
use bevy::prelude::*;
use gd_game_core::options::GameOptions;
use gd_game_core::pause::GamePause;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// Gives `e` its [`Voice`] (as its [`VoiceStart`] says) and hands the voice to the mixer. A voice the mixer cannot
/// take (no audio device: its queue is never drained) ends at once.
pub(super) fn attach_voice(commands: &mut Commands, e: Entity, voice: Voice3d) {
    commands.queue(move |w: &mut World| {
        let (tx, hz) = {
            let s = w.resource::<Sounds>();
            (s.bus.clone(), s.bus_hz)
        };
        // Started while the menu / pause key holds the game: a UI sound (module doc), it plays through the pause.
        let held = w
            .get_resource::<GamePause>()
            .is_some_and(|p| p.menu || p.key);
        let Ok(mut entity) = w.get_entity_mut(e) else {
            return;
        };
        let Some(start) = entity.get::<VoiceStart>().copied() else {
            return;
        };
        let through_pause = held && entity.get::<SoundClass>() != Some(&SoundClass::Music);
        let ctl = Arc::new(VoiceCtl::default());
        let (bus_voice, spare) = voice.start(ctl.clone(), start.speed, hz);
        entity.insert(Voice {
            ctl: ctl.clone(),
            end: start.end,
            spare,
            through_pause,
        });
        if !tx.is_some_and(|tx| tx.try_send(bus_voice).is_ok()) {
            ctl.done.store(true, Ordering::Relaxed);
        }
    });
}

/// A 2D file voice (menu / credits / weather music and loops), no pan; `looped` repeats inside the voice; the entity
/// stays when the file ends ([`OnEnd::Keep`]): the caller manages its lifetime.
pub fn spawn_2d(
    commands: &mut Commands,
    class: SoundClass,
    volume: f32,
    speed: f32,
    file: Loaded,
    looped: bool,
) -> Entity {
    let e = commands
        .spawn(sound(class, volume, OnEnd::Keep, speed))
        .id();
    attach_voice(commands, e, Voice3d::of(file, looped, None));
    e
}

/// A sound fading out to silence, then despawned: `SoundManager::FadeSound` / `SoundManager::Update`
/// (`timeLeft -= min(dt, 100)`, volume = start × timeLeft / total). Used by the boss / event music and
/// the weather loops.
#[derive(Component, Clone, Copy, Debug)]
pub struct Fade {
    from: f32,
    total_ms: f32,
    left_ms: f32,
}

impl Fade {
    /// A fade from `from` over `ms` milliseconds.
    pub fn out(from: f32, ms: f32) -> Self {
        Fade {
            from,
            total_ms: ms.max(1.0),
            left_ms: ms,
        }
    }
}

/// Runs the [`Fade`]s on virtual time (each step at most 100 ms, so a frame hitch cannot jump a fade).
pub(super) fn fade_sounds(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Fade, &mut BaseVolume)>,
) {
    let dt = (time.delta_secs() * 1000.0).min(100.0);
    for (e, mut f, mut v) in &mut q {
        f.left_ms -= dt;
        if f.left_ms <= 0.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        v.0 = f.from * (f.left_ms / f.total_ms).clamp(0.0, 1.0);
    }
}

/// The two ears' gains of one playing 3D voice, shared with the mixing thread.
#[derive(Clone, Default)]
pub struct EarGains(Arc<[AtomicU32; 2]>);

impl EarGains {
    pub(super) fn set(&self, g: [f32; 2]) {
        self.0[0].store(g[0].to_bits(), Ordering::Relaxed);
        self.0[1].store(g[1].to_bits(), Ordering::Relaxed);
    }

    fn get(&self) -> [f32; 2] {
        [
            f32::from_bits(self.0[0].load(Ordering::Relaxed)),
            f32::from_bits(self.0[1].load(Ordering::Relaxed)),
        ]
    }
}

/// A WAV's decoded samples (interleaved), played from memory by the [`Bus`].
#[derive(Clone)]
pub(super) struct Pcm {
    pub(super) samples: Arc<[f32]>,
    pub(super) channels: u16,
    pub(super) rate: u32,
}

/// One file to play: a mono one heard in 3D (`ears` Some), or a 2D one (music, ambient loops, stereo files: `ears`
/// None, the samples pass through). `looped` repeats the file inside the voice. The engine plays music, ambient loops
/// and dialog 2D (`Play2D` / `StartStream`); a stereo file cannot be a 3D buffer
/// (`pak` module doc).
pub struct Voice3d {
    file: AudioSource,
    /// The decoded file; `None`: the file streams.
    pcm: Option<Pcm>,
    looped: bool,
    ears: Option<EarGains>,
}

impl Voice3d {
    pub(super) fn of(file: Loaded, looped: bool, ears: Option<EarGains>) -> Self {
        Voice3d {
            file: file.file,
            pcm: file.pcm,
            looped,
            ears,
        }
    }

    /// The mixer's side of the voice, played at `speed` (the file's rate × `speed`) on a bus at `hz`; a looped stream
    /// also gets the decoder of its next pass ([`Spare`]).
    fn start(self, ctl: Arc<VoiceCtl>, speed: f32, hz: u32) -> (BusVoice, Option<Arc<Spare>>) {
        let mut spare = None;
        let (src, channels, rate) = match self.pcm {
            Some(p) => (
                Src::Pcm {
                    samples: p.samples,
                    at: 0.0,
                },
                p.channels,
                p.rate,
            ),
            None => {
                let dec = self.file.decoder();
                let (channels, rate) = (dec.channels().get(), dec.sample_rate().get());
                if self.looped {
                    spare = Some(Arc::new(Spare {
                        slot: Mutex::new(Some(self.file.decoder())),
                        fresh: AtomicBool::new(true),
                        file: self.file,
                    }));
                }
                (
                    Src::Stream(Box::new(Stream {
                        dec,
                        spare: spare.clone(),
                        cur: [0.0; 2],
                        next: [0.0; 2],
                        at: 0.0,
                        primed: false,
                        last: false,
                    })),
                    channels,
                    rate,
                )
            }
        };
        let voice = BusVoice {
            ctl,
            src,
            channels: channels.max(1) as usize,
            step: rate as f64 * speed as f64 / hz as f64,
            looped: self.looped,
            ears: self.ears,
            gain: None,
        };
        (voice, spare)
    }
}

/// A file's decoder as bevy_audio builds it.
type FileDecoder = <AudioSource as Decodable>::Decoder;

/// A looped stream's decoder for its next pass: built on the game thread ([`refill_spares`]) and swapped in by the
/// mixer at the loop's seam, so the audio thread never probes a file or sets up a Vorbis decoder. After the swap the
/// slot holds the spent decoder until the refill drops it.
struct Spare {
    file: AudioSource,
    slot: Mutex<Option<FileDecoder>>,
    /// The slot holds an unused decoder.
    fresh: AtomicBool,
}

/// What becomes of a sound entity when its file ends: despawned (one-shots, freeing the voice slot), or kept (loops
/// and voices whose owner ends them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnEnd {
    Despawn,
    Keep,
}

/// How [`attach_voice`] starts the entity's [`Voice`].
#[derive(Component, Clone, Copy)]
pub struct VoiceStart {
    pub(super) end: OnEnd,
    pub(super) speed: f32,
}

/// What the game sets of a playing voice and what the mixer reports back.
struct VoiceCtl {
    /// Linear gain (f32 bits).
    volume: AtomicU32,
    paused: AtomicBool,
    /// Set when the game drops the [`Voice`]: the mixer lets it go.
    stopped: AtomicBool,
    /// Set by the mixer once the file ended (or the voice was let go).
    done: AtomicBool,
}

impl Default for VoiceCtl {
    /// Paused at zero volume until [`mix`] starts it.
    fn default() -> Self {
        VoiceCtl {
            volume: AtomicU32::new(0),
            paused: AtomicBool::new(true),
            stopped: AtomicBool::new(false),
            done: AtomicBool::new(false),
        }
    }
}

/// A sound entity's voice in the mixer. Removing it (or despawning the entity) stops the voice.
#[derive(Component)]
pub struct Voice {
    ctl: Arc<VoiceCtl>,
    end: OnEnd,
    /// A looped stream's next-pass decoder, kept filled by [`refill_spares`].
    spare: Option<Arc<Spare>>,
    /// Started while the game was held: a UI sound, not paused by the pause (module doc).
    through_pause: bool,
}

impl Voice {
    pub(super) fn set_volume(&self, v: f32) {
        self.ctl.volume.store(v.to_bits(), Ordering::Relaxed);
    }

    fn set_paused(&self, paused: bool) {
        self.ctl.paused.store(paused, Ordering::Relaxed);
    }

    /// The file ended (never for a looped one).
    fn finished(&self) -> bool {
        self.ctl.done.load(Ordering::Relaxed)
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.ctl.stopped.store(true, Ordering::Relaxed);
    }
}

/// The mixer's rate without an output device to ask ([`start_bus`]).
const FALLBACK_HZ: u32 = 44100;

/// Frames mixed at a time (5.8 ms at 44.1 kHz); a voice's volume and pan changes ramp across one, as rodio's sink
/// applied them every 5 ms.
const BLOCK: usize = 256;

/// A WAV of at most this many bytes (24 s of 16-bit 44.1 kHz mono) is decoded once when loaded and played from
/// memory; a bigger one, and every `.ogg`, is decoded while it plays.
pub(super) const BUFFER_BYTES: usize = 4 << 20;

/// The game's mixer: every voice summed in blocks of [`BLOCK`] frames into one endless stereo span at the device's
/// rate (`hz`). Shared behind a mutex so a device switch ([`open_bus`]) moves it to the new sink with the voice
/// queue and the playing voices intact.
pub(super) struct BusState {
    rx: mpsc::Receiver<BusVoice>,
    hz: u32,
    voices: Vec<BusVoice>,
    /// The current block, interleaved stereo; the [`Bus`] copies it out one block at a time.
    out: Box<[f32]>,
}

impl BusState {
    fn mix_block(&mut self) {
        let BusState {
            rx, voices, out, ..
        } = self;
        voices.extend(rx.try_iter());
        out.fill(0.0);
        voices.retain_mut(|v| {
            let playing = v.mix(out);
            if !playing {
                v.ctl.done.store(true, Ordering::Relaxed);
            }
            playing
        });
    }
}

/// The mixer's rodio source: one [`BLOCK`]-frame span at a time out of the shared [`BusState`] — one lock per
/// block, not per sample.
struct Bus {
    state: Arc<Mutex<BusState>>,
    /// The current block copied out of the state.
    out: Box<[f32]>,
    at: usize,
}

impl Bus {
    fn new(state: Arc<Mutex<BusState>>) -> Bus {
        Bus {
            state,
            out: vec![0.0; BLOCK * 2].into_boxed_slice(),
            at: BLOCK * 2,
        }
    }
}

impl Iterator for Bus {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.at == self.out.len() {
            let mut s = self.state.lock();
            s.mix_block();
            self.out.copy_from_slice(&s.out);
            self.at = 0;
        }
        let s = self.out[self.at];
        self.at += 1;
        Some(s)
    }
}

impl Source for Bus {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        let hz = self.state.lock().hz;
        SampleRate::new(hz).unwrap_or(SampleRate::new(FALLBACK_HZ).unwrap())
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// A voice on the mixer's side.
pub struct BusVoice {
    ctl: Arc<VoiceCtl>,
    src: Src,
    /// The file's channels; a mono file feeds both sides, a wider one its first two.
    channels: usize,
    /// File frames per output frame: the file's rate × speed / the mixer's.
    step: f64,
    looped: bool,
    ears: Option<EarGains>,
    /// The gains the last block ended at (the ramp's start).
    gain: Option<[f32; 2]>,
}

enum Src {
    /// A decoded WAV; `at` is the position in frames.
    Pcm {
        samples: Arc<[f32]>,
        at: f64,
    },
    Stream(Box<Stream>),
}

impl BusVoice {
    /// Adds this voice's next block to `out` (interleaved stereo), its gains ramping from the last block's to the
    /// current volume × ears; false once the file ended or the game let the voice go.
    fn mix(&mut self, out: &mut [f32]) -> bool {
        if self.ctl.stopped.load(Ordering::Relaxed) {
            return false;
        }
        if self.ctl.paused.load(Ordering::Relaxed) {
            return true;
        }
        let vol = f32::from_bits(self.ctl.volume.load(Ordering::Relaxed));
        let to = match &self.ears {
            Some(e) => {
                let [l, r] = e.get();
                [l * vol, r * vol]
            }
            None => [vol, vol],
        };
        let from = self.gain.replace(to).unwrap_or(to);
        let n = (out.len() / 2) as f32;
        let ramp = Ramp {
            gain: from,
            step: [(to[0] - from[0]) / n, (to[1] - from[1]) / n],
        };
        match &mut self.src {
            Src::Pcm { samples, at } => mix_pcm(
                samples,
                self.channels,
                at,
                self.step,
                self.looped,
                out,
                ramp,
            ),
            Src::Stream(s) => s.mix(self.channels, self.step, self.looped, out, ramp),
        }
    }
}

/// A block's left / right gains: `gain` before the first frame, + `step` each frame.
#[derive(Clone, Copy)]
struct Ramp {
    gain: [f32; 2],
    step: [f32; 2],
}

/// Adds decoded samples to `out` from frame position `at`, `step` frames per output frame, linearly interpolated;
/// false once a file that does not loop ended.
fn mix_pcm(
    s: &[f32],
    channels: usize,
    at: &mut f64,
    step: f64,
    looped: bool,
    out: &mut [f32],
    ramp: Ramp,
) -> bool {
    let frames = s.len() / channels;
    if frames == 0 {
        return false;
    }
    let len = frames as f64;
    let [mut gl, mut gr] = ramp.gain;
    let [dl, dr] = ramp.step;
    let mut pos = *at;
    let mut f = 0;
    while f < out.len() {
        let i = pos as usize;
        let t = (pos - i as f64) as f32;
        let j = if i + 1 < frames {
            i + 1
        } else if looped {
            0
        } else {
            i
        };
        let (a, b) = (i * channels, j * channels);
        let (la, lb) = (s[a], s[b]);
        let l = la + (lb - la) * t;
        let r = if channels > 1 {
            let (ra, rb) = (s[a + 1], s[b + 1]);
            ra + (rb - ra) * t
        } else {
            l
        };
        gl += dl;
        gr += dr;
        out[f] += l * gl;
        out[f + 1] += r * gr;
        f += 2;
        pos += step;
        if pos >= len {
            if !looped {
                return false;
            }
            pos %= len;
        }
    }
    *at = pos;
    true
}

/// A file decoded while it plays (`.ogg`, big WAVs): two frames kept for the interpolation, `at` the position between
/// them.
struct Stream {
    dec: FileDecoder,
    /// A looped stream's next-pass decoder.
    spare: Option<Arc<Spare>>,
    cur: [f32; 2],
    next: [f32; 2],
    at: f64,
    primed: bool,
    /// `next` is past the file's end (a stream that does not loop).
    last: bool,
}

impl Stream {
    /// The file's next frame (its first two channels; a mono one on both). A looped stream at its end swaps in the
    /// [`Spare`] decoder and goes on from the start; silence while the spare is not ready yet.
    fn frame(&mut self, channels: usize, looped: bool) -> Option<[f32; 2]> {
        let mut l = self.dec.next();
        if l.is_none() && looped {
            let swapped = self.spare.as_ref().is_some_and(|sp| {
                if !sp.fresh.load(Ordering::Acquire) {
                    return false;
                }
                let Some(mut slot) = sp.slot.try_lock() else {
                    return false;
                };
                let Some(d) = slot.as_mut() else {
                    return false;
                };
                std::mem::swap(&mut self.dec, d);
                sp.fresh.store(false, Ordering::Release);
                true
            });
            if !swapped {
                return Some([0.0; 2]);
            }
            l = self.dec.next();
        }
        let l = l?;
        let r = if channels > 1 {
            self.dec.next().unwrap_or(l)
        } else {
            l
        };
        for _ in 2..channels {
            self.dec.next();
        }
        Some([l, r])
    }

    fn advance(&mut self, channels: usize, looped: bool) {
        self.cur = self.next;
        match self.frame(channels, looped) {
            Some(f) => self.next = f,
            None => self.last = true,
        }
    }

    /// [`mix_pcm`] for a stream.
    fn mix(
        &mut self,
        channels: usize,
        step: f64,
        looped: bool,
        out: &mut [f32],
        ramp: Ramp,
    ) -> bool {
        if !self.primed {
            self.primed = true;
            let Some(f) = self.frame(channels, looped) else {
                return false;
            };
            self.next = f;
            self.advance(channels, looped);
        }
        let [mut gl, mut gr] = ramp.gain;
        let [dl, dr] = ramp.step;
        let mut f = 0;
        while f < out.len() {
            let t = self.at as f32;
            gl += dl;
            gr += dr;
            out[f] += (self.cur[0] + (self.next[0] - self.cur[0]) * t) * gl;
            out[f + 1] += (self.cur[1] + (self.next[1] - self.cur[1]) * t) * gr;
            f += 2;
            self.at += step;
            while self.at >= 1.0 {
                self.at -= 1.0;
                if self.last {
                    return false;
                }
                self.advance(channels, looped);
            }
        }
        true
    }
}

/// The port-owned rodio output playing the [`Bus`], kept in [`Sounds`] so the stream lives as long as the bus:
/// `_sink` / `_player` are keep-alive handles (dropping them stops playback), `device` is the index actually
/// opened (0 = the default sink, also the fallback of a device that failed to open).
pub(super) struct BusSink {
    _sink: rodio::MixerDeviceSink,
    _player: rodio::Player,
    device: i32,
}

/// Starts the mixer: the voice queue and the shared mixing state, then the port-owned rodio sink on the options'
/// `soundDevice` ([`open_bus`]). bevy_audio 0.20 opens only the default sink (no public device hook), so the port
/// owns this one; it plays for the whole process, reopened on a device change ([`switch_device`]).
pub(super) fn start_bus(
    mut sounds: ResMut<Sounds>,
    options: Res<GameOptions>,
    devices: Res<AudioDevices>,
) {
    let (tx, rx) = mpsc::sync_channel(VOICES * 4);
    sounds.bus = Some(tx);
    sounds.bus_state = Some(Arc::new(Mutex::new(BusState {
        rx,
        hz: FALLBACK_HZ,
        voices: Vec::with_capacity(VOICES * 2),
        out: vec![0.0; BLOCK * 2].into_boxed_slice(),
    })));
    open_bus(&mut sounds, &devices, options.int("soundDevice"));
}

/// Opens the bus's sink on the `soundDevice` index (0 = system default) and appends a [`Bus`] reading the shared
/// mixing state. A selected device that fails to open falls back to the default sink and is recorded as index 0,
/// so a re-Apply of the same `soundDevice` retries it ([`switch_device`]); no sink at all keeps the
/// current one playing (at startup: silence, [`FALLBACK_HZ`]). The state — the voice queue and the playing voices —
/// survives the switch; the voices rescale to the new rate.
fn open_bus(sounds: &mut Sounds, devices: &AudioDevices, index: i32) {
    let Some(state) = sounds.bus_state.clone() else {
        return;
    };
    let (sink, opened) = match devices::selected_output(devices, index) {
        Some(d) => match rodio::DeviceSinkBuilder::from_device(d).and_then(|b| b.open_stream()) {
            Ok(s) => (Ok(s), index),
            Err(e) => {
                warn!("sound device failed to open ({e}); using the default");
                (rodio::DeviceSinkBuilder::open_default_sink(), 0)
            }
        },
        None => (rodio::DeviceSinkBuilder::open_default_sink(), 0),
    };
    let sink = match sink {
        Ok(s) => s,
        Err(e) => {
            warn!("no audio output device ({e})");
            return;
        }
    };
    // The old sink (its audio thread gone) stops before the new bus reads the state.
    sounds.bus_out = None;
    let hz = sink.config().sample_rate().get();
    {
        let mut s = state.lock();
        if s.hz != hz {
            let scale = s.hz as f64 / hz as f64;
            for v in &mut s.voices {
                v.step *= scale;
            }
            s.hz = hz;
        }
    }
    let player = rodio::Player::connect_new(sink.mixer());
    player.append(Bus::new(state));
    sounds.bus_hz = hz;
    sounds.bus_out = Some(BusSink {
        _sink: sink,
        _player: player,
        device: opened,
    });
    info!("sound mixer at {hz} Hz");
}

/// The options window's Apply changing `soundDevice`: reopens the bus's sink on the new device, keeping the voice
/// queue and the playing voices ([`open_bus`]). PORT: the engine stores a device index too; hotplug shifts the
/// enumeration under a saved index — the same weakness (a stale index falls back to the default).
pub(super) fn switch_device(
    options: Option<Res<GameOptions>>,
    devices: Res<AudioDevices>,
    mut sounds: ResMut<Sounds>,
) {
    let Some(options) = options.filter(|o| o.is_changed()) else {
        return;
    };
    let index = options.int("soundDevice");
    if sounds.bus_out.as_ref().is_some_and(|o| o.device == index) {
        return;
    }
    open_bus(&mut sounds, &devices, index);
}

/// Builds a fresh next-pass decoder for every looped stream that used its [`Spare`] (off the audio thread; the spent
/// one is dropped here).
pub(super) fn refill_spares(voices: Query<&Voice>) {
    for sp in voices.iter().filter_map(|v| v.spare.as_ref()) {
        if sp.fresh.load(Ordering::Acquire) {
            continue;
        }
        let dec = sp.file.decoder();
        let mut slot = sp.slot.lock();
        *slot = Some(dec);
        sp.fresh.store(true, Ordering::Release);
    }
}

/// Despawns the one-shots whose file ended (bevy_audio's `PlaybackMode::Despawn`), freeing their voice slot.
pub(super) fn reap_voices(mut commands: Commands, voices: Query<(Entity, &Voice)>) {
    for (e, v) in &voices {
        if v.end == OnEnd::Despawn && v.finished() {
            commands.entity(e).try_despawn();
        }
    }
}

/// Whether the window's focus loss silences the game: Grim Dawn.exe main loop `Game::MainLoop`: while the window
/// is not active and `(int)(inactiveUpdateRate * 30) <= 0` (default 0) it calls the window's deactivate handler
/// `Game::OnDeactivate` once (`PauseGameTime` in single player, `SoundManager::FastPause` = `Pause`: every playing
/// sound paused); regaining focus calls `Game::OnActivate` (`UnpauseGameTime` unless a menu pause holds,
/// `SoundManager::UnPause` unless game time is still paused). With a rate above 0 nothing is paused (the loop only
/// sleeps).
pub fn unfocused_mute(options: &GameOptions, focused: bool) -> bool {
    !focused && (options.float("inactiveUpdateRate") * 30.0) as i32 <= 0
}

/// The focus change ([`unfocused_mute`]) into `GamePause::unfocused`: the game time's pause and [`mix`]'s mute.
pub(super) fn focus_pause(
    mut focus: MessageReader<bevy::window::WindowFocused>,
    options: Res<GameOptions>,
    mut pause: ResMut<GamePause>,
) {
    if let Some(f) = focus.read().last() {
        pause.unfocused = unfocused_mute(&options, f.focused);
    }
}

/// Volumes from the options, the game pause and the window focus (module docs). In `Last`, after every system that
/// starts a sound, so a sound started this frame plays from this frame's mix on. A voice's volume is written when it
/// starts, its [`BaseVolume`] changes or the options change; its pause when it starts or the pause / focus changes.
pub(super) fn mix(
    options: Res<GameOptions>,
    pause: Res<GamePause>,
    mut sounds: ResMut<Sounds>,
    q: Query<(Ref<Voice>, &SoundClass, Option<Ref<BaseVolume>>)>,
    mut held: Local<Option<(bool, bool)>>,
) {
    let reread = options.is_changed();
    if reread {
        sounds.listener.read_volumes(&options);
    }
    let l = &sounds.listener;
    let state = (pause.unfocused, pause.paused());
    let repause = *held != Some(state);
    *held = Some(state);
    for (voice, class, base) in &q {
        if let Some(b) = base
            && (reread || voice.is_changed() || b.is_changed())
        {
            voice.set_volume(b.0 * l.gain_final(*class));
        }
        if repause || voice.is_changed() {
            let play = !pause.unfocused && (voice.through_pause || !pause.paused());
            voice.set_paused(!play);
        }
    }
}
