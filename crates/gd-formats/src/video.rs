//! Videos: the engine's `TheoraVideoPlayer` (Engine.dll) plays Ogg files of one Theora video stream and one
//! Vorbis audio stream; the stock ones (`video/intro.ogg`, `gdx1/video/gdx1_intro.ogg`) also carry an Ogg Skeleton
//! stream and are 1920 x 1080 (frame 1920 x 1088), 24 fps, 4:2:0, with 48 kHz stereo Vorbis. Only that container
//! and those two codecs are read ([`Video`]): Theora by `theora_nrp` (a port of libtheora's decoder), Vorbis by
//! lewton (already in the tree through Bevy's audio). Fail-closed: a frame above [`MAX_PIXELS`], audio of more than
//! two channels, or a broken header ends the video.
//!
//! Subtitles (`TheoraVideoPlayer::Play`): `<video without extension>.txt`, one `<seconds>=<text>` line each (the
//! seconds read by `atof`, the text UTF-8); a line without `=` is skipped ([`subtitles`]).
use anyhow::{Context, Result, bail, ensure};
use lewton::audio::{PreviousWindowRight, read_audio_packet_generic};
use lewton::header::{
    IdentHeader, SetupHeader, read_header_comment, read_header_ident, read_header_setup,
};
use lewton::samples::InterleavedSamples;
use std::io::Read;
use theora_nrp::{OgvDecoder, OgvEvent};

/// The largest frame accepted (4K UHD; the stock videos are 1920 x 1088).
pub const MAX_PIXELS: u64 = 3840 * 2160;

/// Bytes read from the file at a time.
const CHUNK: usize = 64 << 10;

/// What [`Video::next`] produced.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// A new picture ([`Video::picture_rgba`]) that shows until `time` seconds; `false` for a dropped frame (the
    /// picture is unchanged).
    Frame { time: f64, new: bool },
    /// Interleaved samples ([`Video::audio`]: channels, rate).
    Audio(Vec<f32>),
}

/// An Ogg Theora + Vorbis file read from `R` as it plays (module doc).
pub struct Video<R> {
    file: R,
    ogv: OgvDecoder,
    eof: bool,
    /// The Vorbis stream: its serial, then its three headers.
    vorbis: Option<Vorbis>,
}

struct Vorbis {
    serial: i32,
    ident: Option<IdentHeader>,
    comment: bool,
    setup: Option<SetupHeader>,
    window: PreviousWindowRight,
}

impl<R: Read> Video<R> {
    pub fn new(file: R) -> Self {
        Video {
            file,
            ogv: OgvDecoder::with_max_pixels(MAX_PIXELS),
            eof: false,
            vorbis: None,
        }
    }

    /// The picture size (once the Theora headers are read).
    pub fn picture_size(&self) -> Option<(u32, u32)> {
        self.ogv.picture_dimensions()
    }

    /// The audio's channels and rate (once the Vorbis headers are read).
    pub fn audio(&self) -> Option<(u8, u32)> {
        let v = self.vorbis.as_ref()?;
        v.setup.as_ref()?;
        let id = v.ident.as_ref()?;
        Some((id.audio_channels, id.audio_sample_rate))
    }

    /// The last frame as RGBA rows of `width * 4` bytes ([`Self::picture_size`]).
    pub fn picture_rgba(&self, out: &mut [u8]) -> Result<()> {
        let (w, _) = self.picture_size().context("no picture yet")?;
        self.ogv
            .picture_rgba(out, w as usize * 4)
            .map_err(|e| anyhow::anyhow!("theora: {e:?}"))
    }

    /// The next frame or block of samples, in file order; `None` at the end.
    pub fn step(&mut self) -> Result<Option<Event>> {
        loop {
            let event = self
                .ogv
                .next_event()
                .map_err(|e| anyhow::anyhow!("theora: {e:?}"))?;
            match event {
                Some(OgvEvent::Headers { .. }) => {}
                Some(OgvEvent::VideoFrame {
                    time, duplicate, ..
                }) => {
                    return Ok(Some(Event::Frame {
                        time,
                        new: !duplicate,
                    }));
                }
                Some(OgvEvent::Packet { serialno, packet }) => {
                    let bytes = packet.packet;
                    if let Some(samples) = vorbis(&mut self.vorbis, serialno, bytes, packet.bos())?
                    {
                        return Ok(Some(Event::Audio(samples)));
                    }
                }
                None if self.eof => return Ok(None),
                None => {
                    if self.ogv.read_from(&mut self.file, CHUNK)? == 0 {
                        self.eof = true;
                        self.ogv.mark_eof();
                    }
                }
            }
        }
    }
}

/// Feeds a non-Theora packet to the Vorbis stream (the first `\x01vorbis` one found); its decoded samples, if any.
fn vorbis(
    state: &mut Option<Vorbis>,
    serial: i32,
    bytes: &[u8],
    bos: bool,
) -> Result<Option<Vec<f32>>> {
    if state.is_none() && bos && bytes.starts_with(b"\x01vorbis") {
        *state = Some(Vorbis {
            serial,
            ident: None,
            comment: false,
            setup: None,
            window: PreviousWindowRight::new(),
        });
    }
    let Some(v) = state.as_mut().filter(|v| v.serial == serial) else {
        return Ok(None);
    };
    let Some(ident) = &v.ident else {
        let id = read_header_ident(bytes).map_err(|e| anyhow::anyhow!("vorbis: {e:?}"))?;
        ensure!(
            (1..=2).contains(&id.audio_channels),
            "vorbis: {} channels",
            id.audio_channels
        );
        v.ident = Some(id);
        return Ok(None);
    };
    if !v.comment {
        read_header_comment(bytes).map_err(|e| anyhow::anyhow!("vorbis: {e:?}"))?;
        v.comment = true;
        return Ok(None);
    }
    let Some(setup) = &v.setup else {
        let s = read_header_setup(
            bytes,
            ident.audio_channels,
            (ident.blocksize_0, ident.blocksize_1),
        )
        .map_err(|e| anyhow::anyhow!("vorbis: {e:?}"))?;
        v.setup = Some(s);
        return Ok(None);
    };
    if bytes.is_empty() {
        return Ok(None);
    }
    let s: InterleavedSamples<f32> =
        match read_audio_packet_generic(ident, setup, bytes, &mut v.window) {
            Ok(s) => s,
            // A damaged packet is dropped (libvorbis skips it as well).
            Err(_) => return Ok(None),
        };
    if s.samples.is_empty() {
        return Ok(None);
    }
    Ok(Some(s.samples))
}

/// A subtitle file's lines (module doc) as (seconds, text), in file order.
pub fn subtitles(text: &str) -> Vec<(f32, String)> {
    text.lines()
        .filter_map(|l| {
            let (t, s) = l.split_once('=')?;
            Some((atof(t), s.trim_end_matches('\r').to_owned()))
        })
        .collect()
}

/// `atof`: the longest leading number, 0 without one.
fn atof(s: &str) -> f32 {
    let s = s.trim_start();
    let end = s
        .char_indices()
        .take_while(|&(i, c)| c.is_ascii_digit() || c == '.' || (i == 0 && (c == '-' || c == '+')))
        .last()
        .map_or(0, |(i, c)| i + c.len_utf8());
    s[..end].parse().unwrap_or(0.0)
}

/// Opens a video by its engine path (`video/intro.ogg`) under the install `root`, each path component matched
/// ignoring case (the install ships `video/Intro.ogg`); the engine opens it relative to the install, not through
/// the archives, so a mod cannot replace it.
pub fn find(root: &std::path::Path, path: &str) -> Result<std::path::PathBuf> {
    let mut at = root.to_path_buf();
    for part in path.split(['/', '\\']).filter(|p| !p.is_empty()) {
        let entry = std::fs::read_dir(&at)
            .with_context(|| format!("reading {}", at.display()))?
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part));
        match entry {
            Some(e) => at = e.path(),
            None => bail!("video not found: {path}"),
        }
    }
    Ok(at)
}
