//! The jukebox's crossfading channel (music and ambient sessions): the rules of `Jukebox::LoadMusicPlaylist` /
//! `LoadAmbientPlaylist` / `Update` (Engine.dll @0x18012a050 / @0x180129e50 / @0x180129b80).
//!
//! A channel holds the playing session (`cur`) and the one replacing it (`next`), each with a fade volume. Loading a
//! different session drops a pending one and arms a delay: 1 ms when nothing plays, else [`MUSIC_SWITCH_MS`] (music: the
//! old session plays on at full volume) or [`AMBIENT_SWITCH_MS`]; an empty name is "end the session" (it fades out,
//! nothing replaces it). Once the delay is over the old session fades out and the new one in, both linearly at
//! `1 / 3000` per ms (a 3 s crossfade); a session with nothing pending fades in the same way (a first session starts
//! silent). `stop` ends the current session at once (`StopMusic`).
pub const MUSIC_SWITCH_MS: f32 = 5000.0;
pub const AMBIENT_SWITCH_MS: f32 = 1000.0;
/// Re-check of the day / night flip after an ambient load (`kDayNightChangeDelay`).
pub const DAY_NIGHT_WAIT_MS: f32 = 10_000.0;
/// Fade per ms (`0.00033333333f`).
pub const FADE_PER_MS: f32 = 1.0 / 3000.0;

/// A session on the channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    /// Unique per channel: what the player of the session is keyed by.
    pub id: u32,
    /// The session's record.
    pub name: String,
    /// The fade volume, 0..1.
    pub volume: f32,
    /// The caller's flag at load time (an ambient session's "built for night").
    pub flag: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Channel {
    pub cur: Option<Slot>,
    pub next: Option<Slot>,
    pub ending: bool,
    delay_ms: f32,
    ids: u32,
}

impl Channel {
    /// `Load*Playlist(name)`: `false` when it did nothing (the same session is playing and `force` is off).
    pub fn load(&mut self, name: &str, switch_ms: f32, force: bool, flag: bool) -> bool {
        if !force && self.cur.as_ref().is_some_and(|c| c.name == name) {
            return false;
        }
        self.next = None;
        self.delay_ms = if self.cur.is_some() { switch_ms } else { 1.0 };
        if name.is_empty() {
            self.ending = true;
        } else {
            self.ending = false;
            self.ids += 1;
            self.next = Some(Slot {
                id: self.ids,
                name: name.to_owned(),
                volume: 0.0,
                flag,
            });
        }
        true
    }

    /// `StopMusic`: both sessions end at once.
    pub fn stop(&mut self) {
        self.cur = None;
        self.next = None;
        self.ending = false;
    }

    /// Advances the fades by `dt` ms.
    pub fn update(&mut self, dt: f32) {
        let step = dt * FADE_PER_MS;
        if self.next.is_some() || self.ending {
            self.delay_ms -= dt;
            if self.delay_ms <= 0.0 {
                if let Some(c) = &mut self.cur {
                    c.volume = (c.volume - step).max(0.0);
                }
                if let Some(n) = &mut self.next {
                    n.volume = (n.volume + step).min(1.0);
                }
                if self.cur.as_ref().is_none_or(|c| c.volume <= 0.0) {
                    self.cur = self.next.take();
                    self.ending = false;
                }
            }
        } else if let Some(c) = &mut self.cur {
            c.volume = (c.volume + step).min(1.0);
        }
    }

    /// The sessions with something to hear: the current one, and the next once its fade-in has begun (a music session
    /// holds its tracks until then).
    pub fn audible(&self) -> impl Iterator<Item = &Slot> {
        self.cur
            .iter()
            .chain(self.next.iter().filter(|n| n.volume > 0.0))
    }
}

/// The next track of a music session (`MusicSession` tick, `sub_18014F9F0`): sequential from the start, or random with
/// no immediate repeat (`rand() % n`, the current + 1 on a repeat) when `randomize`. `cur` is the last index.
pub fn next_track(n: usize, cur: Option<usize>, randomize: bool, roll: usize) -> usize {
    if n == 0 {
        return 0;
    }
    match (randomize, cur) {
        (false, None) => 0,
        (false, Some(c)) => (c + 1) % n,
        (true, c) => {
            let pick = roll % n;
            if Some(pick) == c {
                (pick + 1) % n
            } else {
                pick
            }
        }
    }
}

/// `DayNightCycle::GetIsDay(t)` @0x18004f510: day lies from `day` (`GetDayTime`) to `night` (`GetNightTime`),
/// hours mod 24.
pub fn is_day(t: f32, day: f32, night: f32) -> bool {
    let mut b = night;
    let mut a = t;
    if b < day {
        b += 24.0;
    }
    if a < day {
        a += 24.0;
    }
    let f = (a - day) / (b - day);
    (0.0..=1.0).contains(&f)
}
