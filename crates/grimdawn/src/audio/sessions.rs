//! The area's music and ambient sessions, event and boss music.
//!
//! Music and ambient sessions run through the jukebox's crossfading channels (`gd_sim::jukebox`,
//! `Jukebox::Update` and `Load*Playlist`): a session change waits 5 s (music; the old music
//! plays on) or 1 s (ambient), then a 3 s linear crossfade; an empty record ends the session by fading out; music
//! tracks chain at random (never the same twice in a row) with no gap; an ambient session with `enableNightSounds`
//! reloads with its `night*` set when the day flips (at most every 10 s). Ambient one-shots: the first sound of each
//! class plays at once, then a wait of `min + rand % (max - min)` whole seconds after each sound. Event and boss music:
//! [`boss_and_event_music`]. Day or night is `AmbianceManager::GetIsDay`, the area's own cycle
//! (`render::env::Ambiance::is_day`).
use super::Sounds;
use super::listener::{BaseVolume, SoundClass, sound};
use super::mixer::{EarGains, Fade, OnEnd, Voice3d, attach_voice};
use super::pak::{ENGINE_HZ, Positioned, numbered, start_voice};
use crate::data::GameData;
use crate::world::StreamedMap;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::map::sector::SectorEntry;
use gd_render::camera::CameraFocus;
use gd_sim::jukebox::Channel;

/// The area's music and ambient sessions (the jukebox, `gd_sim::jukebox`): each channel crossfades its sessions; a
/// session's player entities ([`SessionSound`]) are keyed by slot.
#[derive(Resource, Default)]
pub(crate) struct Ambience {
    music_record: Option<String>,
    ambient_record: Option<String>,
    music: Channel,
    ambient: Channel,
    /// Per music session (slot id): the last track picked, and whether it waits for its file's background read.
    last_track: Vec<(u32, usize, bool)>,
    /// Ms until the day / night flip is looked at again (`kDayNightChangeDelay`, armed by every ambient load).
    day_night_wait: f32,
    /// The current ambient session's slot id, its `enableNightSounds`, and its one-shot classes (frequent, moderate,
    /// unique) in its day or night set, read once per session.
    session_id: u32,
    night_sounds: bool,
    shot_classes: Option<[ShotClass; 3]>,
    /// The ambient loop of each audible ambient session, by slot id (read once per slot).
    loops: Vec<(u32, AmbientLoop)>,
    /// Per one-shot class: the sound playing and the countdown (ms).
    shots: [(Option<Entity>, f32); 3],
    /// Where the sector layers were last probed ([`track_area`]): (focus, region, menu scene).
    probed: Option<(Vec3, Option<usize>, bool)>,
    /// What the ambient layer (2) named there and whether the cell is mapped; whether the listener is in a boss music
    /// zone (sector type 3).
    sector_mapped: bool,
    sector_music: String,
    sector_ambient: String,
    sector_event: String,
    in_boss_zone: bool,
    /// The event music last started (its name), those already played this session.
    last_event: String,
    played_events: Vec<String>,
    /// The playing event music: its sound and its `EventMusicSession` record.
    event: Option<(Entity, String)>,
    /// The event session a boss session interrupted (its record): `PlayBossMusic` only fades its sound, the session
    /// lives on and `StopBossMusic` replays it.
    held_event: Option<String>,
    /// The boss session (`BossMusicSession`), the bosses tracked for it and a boss waiting for the zone.
    boss: Option<Boss>,
    tracked: Vec<Entity>,
    pending_boss: Option<Entity>,
}

/// A boss music session (`BossMusicSession`, `sub_18003F540`): intro, loop, then the
/// boss death or player death stinger; each state's sound is one entity. The record's (file, volume) pairs are read
/// when the session starts.
struct Boss {
    name: String,
    looping: Option<(String, f32)>,
    boss_death: Option<(String, f32)>,
    player_death: Option<(String, f32)>,
    /// `EndXFadeMS`: the fade of the playing sound when a stinger starts.
    end_fade: f32,
    state: u8,
    sound: Option<Entity>,
}

/// A boss record's file `key` and its volume `vkey` (default 1).
fn boss_track(rec: &Record, key: &str, vkey: &str) -> Option<(String, f32)> {
    rec.str(key)
        .map(|p| (p.to_owned(), rec.f32(vkey).unwrap_or(1.0)))
}

/// A monster's boss-music facts (its record's `MonsterMusic`), read once when it spawns.
#[derive(Component)]
pub(super) struct MonsterMusic {
    name: String,
    radius: f32,
    ignore_layer: bool,
}

pub(super) fn cache_monster_music(
    mut commands: Commands,
    data: Res<GameData>,
    new: Query<(Entity, &gd_game::monsters::Monster), Added<gd_game::monsters::Monster>>,
) {
    for (e, m) in &new {
        let Some(rec) = data.db.get(&m.record) else {
            continue;
        };
        let Some(name) = rec.str("MonsterMusic").filter(|n| !n.is_empty()) else {
            continue;
        };
        commands.entity(e).try_insert(MonsterMusic {
            name: name.to_owned(),
            radius: rec.f32("MonsterMusicRadius").unwrap_or(0.0),
            ignore_layer: rec.bool("ignoreBossMusicLayer"),
        });
    }
}

/// A session's player: a music track or an ambient loop of the (music?, slot) session.
#[derive(Component)]
pub(super) struct SessionSound(bool, u32);

const CLASSES: [&str; 3] = ["frequent", "moderate", "unique"];

/// The record key of an ambient session field in its day (`base`) or night (`night` + capitalised) set.
fn skey(night: bool, base: &str) -> String {
    if night {
        format!("night{}", capitalized(base))
    } else {
        base.to_owned()
    }
}

fn capitalized(class: &str) -> String {
    class[..1].to_ascii_uppercase() + &class[1..]
}

/// `AmbientSession::Load`: the night set is read when the session was made at
/// night and `enableNightSounds` is on.
fn uses_night(rec: &Record, is_day: bool) -> bool {
    !is_day && rec.bool("enableNightSounds")
}

/// `AmbianceManager::GetIsDay`, which `AmbientSession::Load` and the `Jukebox::Update` flip read: the area's own cycle
/// ([`gd_render::render::env::Ambiance::is_day`]); before the first ambiance, the default cycle at the clock.
fn is_day_now(ambiance: Option<&gd_render::render::env::Ambiance>, tod: f32) -> bool {
    ambiance.map_or_else(
        || gd_render::render::env::DayNightCycle::default().is_day(tod),
        |a| a.is_day,
    )
}

/// An ambient session's loop in its day or night set.
struct AmbientLoop {
    path: Option<String>,
    volume: f32,
}

/// One ambient one-shot class of a session in its day or night set.
#[derive(Default)]
struct ShotClass {
    /// `<class>Sound1..N`.
    paths: Vec<String>,
    /// `force2d<Class>N` of each path.
    force2d: Vec<bool>,
    volume: f32,
    min: i32,
    max: i32,
}

impl ShotClass {
    fn read(rec: &Record, night: bool, class: &str) -> Self {
        let paths = numbered(rec, &skey(night, &format!("{class}Sound")));
        let cap = capitalized(class);
        let force2d = (1..=paths.len())
            .map(|n| rec.bool(&skey(night, &format!("force2d{cap}{n}"))))
            .collect();
        ShotClass {
            force2d,
            volume: rec
                .f32(&skey(night, &format!("{class}Volume")))
                .unwrap_or(1.0),
            min: rec
                .i32(&skey(night, &format!("{class}MinTime")))
                .unwrap_or(15)
                .max(0),
            max: rec
                .i32(&skey(night, &format!("{class}MaxTime")))
                .unwrap_or(30),
            paths,
        }
    }

    /// The wait in ms after a sound (`1000 * (min + rand() % (max - min))`, integer seconds;
    /// `sub_1800363A0`).
    fn delay(&self) -> f32 {
        let span = (self.max - self.min).max(1) as u32;
        1000.0 * (self.min as f32 + (fastrand::u32(..) % span) as f32)
    }
}

/// Probes the sector layers under the listener for the area's music, ambience, event music and boss music zone, and
/// loads changed sessions. The probe reruns only when the focus, the region or the loaded levels changed.
#[allow(clippy::too_many_arguments)]
pub(super) fn track_area(
    world: Option<Res<StreamedMap>>,
    levels: Res<crate::world::Levels>,
    focus: Query<&GlobalTransform, With<CameraFocus>>,
    tod: Res<gd_render::render::env::TimeOfDay>,
    ambiance: Option<Res<gd_render::render::env::Ambiance>>,
    menu: Option<Res<crate::menu_scene::MenuScene>>,
    backdrop: Option<Res<gd_render::render::backdrop::MenuBackdrop>>,
    story: Option<NonSend<crate::story::StoryRuntime>>,
    mut amb: ResMut<Ambience>,
) {
    use gd_sim::jukebox::{AMBIENT_SWITCH_MS, DAY_NIGHT_WAIT_MS, MUSIC_SWITCH_MS};
    if world.is_none() && menu.is_none() {
        return;
    }
    // In the game the camera focus and the world's sectors; in the main menu the scene's map at the character. The
    // sectors are read from the player's own region's level (`Player::UpdateSelf`; the Shattered Realm's
    // regions overlap main-world ground). PORT: the engine's ambience / music sector source is not traced, the
    // player's region stands in for it.
    let region = gd_game::riftgates::player_region(story.as_deref());
    let at = match (&menu, focus.single()) {
        (Some(m), _) => m.view.player.translation,
        (None, Ok(f)) => f.translation(),
        (None, Err(_)) => Vec3::NAN,
    };
    let key = (at, region, menu.is_some());
    let stale = amb.probed != Some(key)
        || levels.is_changed()
        || menu.as_ref().is_some_and(|m| m.is_changed())
        || backdrop.as_ref().is_some_and(|b| b.is_changed());
    let amb = &mut *amb;
    if stale {
        amb.probed = Some(key);
        let entry = |layer: usize| match (&menu, &backdrop, &world, focus.single()) {
            (Some(m), Some(b), ..) => b.sector_entry(m.view.player.translation, layer),
            (None, _, Some(w), Ok(f)) => match region {
                Some(r) => levels.sector_entry_in(&w.map, r, f.translation(), layer),
                None => levels.sector_entry(&w.map, f.translation(), layer),
            },
            _ => None,
        };
        let zone = entry(3).is_some();
        if zone != amb.in_boss_zone {
            debug!("boss music zone: {zone}");
        }
        amb.in_boss_zone = zone;
        // Outside mapped cells whatever plays keeps playing.
        amb.sector_mapped = false;
        if let Some(SectorEntry::Ambient { sounds, .. }) = entry(2) {
            amb.sector_mapped = true;
            for (i, to) in [
                &mut amb.sector_music,
                &mut amb.sector_ambient,
                &mut amb.sector_event,
            ]
            .into_iter()
            .enumerate()
            {
                let name = sounds.get(i).map_or("", String::as_str);
                if to.as_str() != name {
                    to.clear();
                    to.push_str(name);
                }
            }
        }
    }
    if !amb.sector_mapped {
        return;
    }
    // An empty name ends the session (it fades out); the same name keeps it. While a boss session exists the area
    // music is not loaded (`ShouldPlayMusic`).
    if amb.boss.is_none() && amb.music_record.as_deref() != Some(amb.sector_music.as_str()) {
        amb.music_record = Some(amb.sector_music.clone());
        amb.music
            .load(&amb.sector_music, MUSIC_SWITCH_MS, false, false);
    }
    if amb.ambient_record.as_deref() != Some(amb.sector_ambient.as_str()) {
        amb.ambient_record = Some(amb.sector_ambient.clone());
        amb.ambient.load(
            &amb.sector_ambient,
            AMBIENT_SWITCH_MS,
            false,
            is_day_now(ambiance.as_deref(), tod.0),
        );
        amb.day_night_wait = DAY_NIGHT_WAIT_MS;
    }
}

/// Plays one file of a music record once at `volume` (the entity despawns when it ends). A file not loaded yet is
/// read in the background: the entity stands for the music from now on and plays it when it lands
/// (`cache::AwaitedFile`).
fn play_once(
    commands: &mut Commands,
    data: &GameData,
    sounds: &mut Sounds,
    path: &str,
    volume: f32,
) -> Option<Entity> {
    super::cache::start_2d(
        commands,
        data,
        sounds,
        path,
        ENGINE_HZ,
        SoundClass::Music,
        volume,
        OnEnd::Despawn,
    )
}

/// Event and boss music (`AmbianceManager::Update`, `Jukebox::PlayEventMusic` / `PlayBossMusic`,
/// `Player::RegisterMonsterMusic` / `BossMusicStateUpdate`):
/// - the sector's event music record plays once per session per name (`eventMusicName`, `volume`), cutting the area
///   music; when it ends `randomMusicPak` (a music session) resumes, else the area music reloads;
/// - a monster whose record has `MonsterMusic` within `MonsterMusicRadius` of the player tracks for boss music; with the
///   player in a boss music zone (sector type 3) or the record's `ignoreBossMusicLayer`, the boss session starts: the
///   event music fades out over 3 s, the area music is cut, the intro plays, then the loop; when every tracked boss is
///   gone the boss death stinger plays, when the player is dead the player death one (the playing sound fades over
///   `EndXFadeMS`); after the stinger the area music loads again. The ambient sounds go on throughout.
/// - when the boss session ends (`StopBossMusic` @0x18012a540) an event session it interrupted (still alive: the 3 s
///   fade does not stop it) plays again from the start at its volume, else the area music reloads.
///
/// PORT: `LeaveFadeMS` (no reader found) is not modelled.
#[allow(clippy::too_many_arguments)]
pub(super) fn boss_and_event_music(
    mut commands: Commands,
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
    mut amb: ResMut<Ambience>,
    alive_sounds: Query<(), With<SoundClass>>,
    monsters: Query<(Entity, &GlobalTransform, &MonsterMusic), Without<gd_game::combat::Dead>>,
    player: Query<(&GlobalTransform, Has<gd_game::combat::Dead>), With<crate::player::Player>>,
    volumes: Query<&BaseVolume>,
) {
    use gd_sim::jukebox::MUSIC_SWITCH_MS;
    let Ok((pgt, player_dead)) = player.single() else {
        return;
    };
    let amb = &mut *amb;
    let sounds = &mut *sounds;
    // Bosses to track: `MonsterMusic` within `MonsterMusicRadius` (the facts were cached when the monster spawned).
    for (e, gt, mm) in &monsters {
        let r = mm.radius;
        if gt.translation().distance_squared(pgt.translation()) < r * r {
            if !amb.tracked.contains(&e) {
                amb.tracked.push(e);
            }
            amb.pending_boss = Some(e);
        }
    }
    let fade_out = |commands: &mut Commands, e: Entity, ms: f32| {
        let from = volumes.get(e).map_or(1.0, |v| v.0);
        commands.entity(e).try_insert(Fade::out(from, ms));
    };
    // The pending boss starts the boss session in a boss music zone (or ignoring it).
    if let Some(pending) = amb.pending_boss.take()
        && let Ok((_, _, mm)) = monsters.get(pending)
    {
        debug!(
            "boss music pending: {} (zone {}, ignore {})",
            mm.name, amb.in_boss_zone, mm.ignore_layer
        );
        if amb.boss.is_none()
            && (amb.in_boss_zone || mm.ignore_layer)
            && let Some(rec) = data.db.get(&mm.name)
        {
            info!("boss music: {}", mm.name);
            if let Some((e, name)) = amb.event.take() {
                fade_out(&mut commands, e, 3000.0);
                amb.held_event = Some(name);
            }
            amb.music.stop();
            amb.music_record = None;
            let sound = boss_track(rec, "IntroMusic", "IntroVolume")
                .and_then(|(p, v)| play_once(&mut commands, &data, sounds, &p, v));
            amb.boss = Some(Boss {
                name: mm.name.clone(),
                looping: boss_track(rec, "LoopMusic", "LoopingVolume"),
                boss_death: boss_track(rec, "BossDeathMusic", "BossDeathVolume"),
                player_death: boss_track(rec, "PlayerDeathMusic", "PlayerDeathVolume"),
                end_fade: rec.f32("EndXFadeMS").unwrap_or(0.0).max(1.0),
                state: 0,
                sound,
            });
        }
    }
    // The boss state machine.
    if let Some(mut boss) = amb.boss.take() {
        let bosses_dead =
            !amb.tracked.is_empty() && amb.tracked.iter().all(|e| monsters.get(*e).is_err());
        let playing = boss.sound.is_some_and(|e| alive_sounds.contains(e));
        let mut play = |track: &Option<(String, f32)>, commands: &mut Commands| {
            track
                .as_ref()
                .and_then(|(p, v)| play_once(commands, &data, sounds, p, *v))
        };
        let mut done = false;
        match boss.state {
            0 | 1 => {
                let stinger = if bosses_dead {
                    Some((&boss.boss_death, 2))
                } else if player_dead {
                    Some((&boss.player_death, 3))
                } else {
                    None
                };
                if let Some((track, state)) = stinger {
                    if let Some(e) = boss.sound
                        && alive_sounds.contains(e)
                    {
                        fade_out(&mut commands, e, boss.end_fade);
                    }
                    boss.sound = play(track, &mut commands);
                    boss.state = state;
                } else if !playing {
                    boss.sound = play(&boss.looping, &mut commands);
                    boss.state = 1;
                }
            }
            _ => done = !playing,
        }
        if done {
            // `StopBossMusic`: a living event session replays, else the area music loads afresh.
            info!("boss music ended: {}", boss.name);
            amb.tracked.clear();
            let replay = amb.held_event.take().and_then(|name| {
                let rec = data.db.get(&name)?;
                let e = play_once(
                    &mut commands,
                    &data,
                    sounds,
                    rec.str("eventMusicName")?,
                    rec.f32("volume").unwrap_or(1.0),
                )?;
                Some((e, name))
            });
            match replay {
                // The area's name stays remembered (as at an event's start), so it is not reloaded over the event.
                Some(ev) => {
                    amb.event = Some(ev);
                    amb.music_record = Some(amb.sector_music.clone());
                }
                None => amb.music_record = None,
            }
        } else {
            amb.boss = Some(boss);
        }
    }
    // Event music: only while no boss session; once per name per session.
    if amb.boss.is_some() {
        return;
    }
    if amb
        .event
        .as_ref()
        .is_some_and(|(e, _)| !alive_sounds.contains(*e))
        && let Some((_, name)) = amb.event.take()
    {
        // It ended: the named music session resumes, else the area music reloads.
        match data
            .db
            .get(&name)
            .and_then(|r| r.str("randomMusicPak"))
            .filter(|p| !p.is_empty())
        {
            Some(pak) => {
                amb.music_record = Some(pak.to_owned());
                amb.music.load(pak, MUSIC_SWITCH_MS, false, false);
            }
            None => amb.music_record = None,
        }
    }
    if amb.sector_event == amb.last_event {
        return;
    }
    amb.last_event.clone_from(&amb.sector_event);
    let name = &amb.last_event;
    if !name.is_empty()
        && !amb.played_events.contains(name)
        && let Some(rec) = data.db.get(name)
    {
        amb.played_events.push(name.clone());
        info!("event music: {name}");
        // `StopMusic` once the event track plays; the area's name stays remembered, so it is not reloaded until the
        // event ends. A track that fails to play leaves the area music running.
        if let Some(e) = rec.str("eventMusicName").and_then(|p| {
            play_once(
                &mut commands,
                &data,
                sounds,
                p,
                rec.f32("volume").unwrap_or(1.0),
            )
        }) {
            amb.music.stop();
            amb.event = Some((e, name.clone()));
        }
    }
}

/// Runs the two channels and their players (`Jukebox::Update`): fades, the day / night reload of an ambient session
/// with `enableNightSounds`, a music track chain (a random track, never the same twice in a row; the next starts as
/// the last ends), the ambient loop; the sessions' fade is the players' volume.
#[allow(clippy::too_many_arguments)]
pub(super) fn drive_sessions(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
    tod: Res<gd_render::render::env::TimeOfDay>,
    ambiance: Option<Res<gd_render::render::env::Ambiance>>,
    mut amb: ResMut<Ambience>,
    mut players: Query<(Entity, &SessionSound, &mut BaseVolume)>,
) {
    use gd_sim::jukebox::{AMBIENT_SWITCH_MS, DAY_NIGHT_WAIT_MS, next_track};
    let dt = time.delta_secs() * 1000.0;
    let day = is_day_now(ambiance.as_deref(), tod.0);
    let amb = &mut *amb;
    let sounds = &mut *sounds;
    // The day / night flip.
    amb.day_night_wait = (amb.day_night_wait - dt).max(0.0);
    if amb.day_night_wait <= 0.0
        && amb.ambient.next.is_none()
        && !amb.ambient.ending
        && amb.night_sounds
        && let Some(cur) = amb.ambient.cur.as_ref()
        && cur.flag != day
    {
        let name = cur.name.clone();
        amb.ambient.load(&name, AMBIENT_SWITCH_MS, true, day);
        amb.day_night_wait = DAY_NIGHT_WAIT_MS;
    }
    amb.music.update(dt);
    amb.ambient.update(dt);
    // Music: the track chain of each audible session.
    for slot in amb.music.audible() {
        // `MusicSession::Load` (Engine.dll sub_18014F790): every track's SoundDescriptor is built with volume
        // 0.8 (the ctor default), and the tick (sub_18014F9F0) plays it unchanged; the session fade is `slot.volume`.
        let nominal = 0.8 * slot.volume;
        if let Some((.., mut v)) = players.iter_mut().find(|(_, s, _)| s.0 && s.1 == slot.id) {
            v.set_if_neq(BaseVolume(nominal));
            continue;
        }
        let Some(rec) = data.db.get(&slot.name) else {
            continue;
        };
        let tracks = rec.strs("tracks");
        let randomize = rec.get("randomize").is_none() || rec.bool("randomize");
        let last = amb.last_track.iter_mut().find(|(id, ..)| *id == slot.id);
        // A track still being read stays the pick until it lands.
        let pick = match &last {
            Some(t) if t.2 => t.1,
            _ => next_track(
                tracks.len(),
                last.as_ref().map(|t| t.1),
                randomize,
                fastrand::usize(..1 << 16),
            ),
        };
        let file = tracks
            .get(pick)
            .map(|track| sounds.file_or_fetch(&data, track));
        let waiting = matches!(file, Some(Err(true)));
        match last {
            Some(t) => (t.1, t.2) = (pick, waiting),
            None => amb.last_track.push((slot.id, pick, waiting)),
        }
        if let (Some(track), Some(Ok(f))) = (tracks.get(pick), file) {
            info!("music: {track}");
            let speed = f.speed(ENGINE_HZ);
            let e = commands
                .spawn((
                    sound(SoundClass::Music, nominal, OnEnd::Despawn, speed),
                    SessionSound(true, slot.id),
                ))
                .id();
            attach_voice(&mut commands, e, Voice3d::of(f, false, None));
        }
    }
    // Ambient: the loop of each audible session (its day or night set), read once per slot.
    for slot in amb.ambient.audible() {
        if !amb.loops.iter().any(|(id, _)| *id == slot.id) {
            let l = data.db.get(&slot.name).map_or(
                AmbientLoop {
                    path: None,
                    volume: 1.0,
                },
                |rec| {
                    let night = uses_night(rec, slot.flag);
                    AmbientLoop {
                        path: rec.str(&skey(night, "ambientLoop")).map(str::to_owned),
                        volume: rec.f32(&skey(night, "ambientLoopVolume")).unwrap_or(1.0),
                    }
                },
            );
            amb.loops.push((slot.id, l));
        }
        let Some((_, l)) = amb.loops.iter().find(|(id, _)| *id == slot.id) else {
            continue;
        };
        let v = l.volume * slot.volume;
        if let Some((.., mut b)) = players.iter_mut().find(|(_, s, _)| !s.0 && s.1 == slot.id) {
            b.set_if_neq(BaseVolume(v));
            continue;
        }
        // Read in the background the first time: the loop starts on a later frame.
        if let Some(path) = &l.path
            && let Ok(f) = sounds.file_or_fetch(&data, path)
        {
            info!("ambience: {path}");
            let speed = f.speed(ENGINE_HZ);
            // The loop repeats inside the voice; the entity is kept, the session despawns it.
            let e = commands
                .spawn((
                    sound(SoundClass::Ambient, v, OnEnd::Keep, speed),
                    SessionSound(false, slot.id),
                ))
                .id();
            attach_voice(&mut commands, e, Voice3d::of(f, true, None));
        }
    }
    // Players of sessions no longer audible end.
    for (e, s, _) in &players {
        let channel = if s.0 { &amb.music } else { &amb.ambient };
        if !channel.audible().any(|a| a.id == s.1) {
            commands.entity(e).try_despawn();
        }
    }
    let Ambience {
        music,
        ambient,
        last_track,
        loops,
        ..
    } = &mut *amb;
    last_track.retain(|(id, ..)| music.audible().any(|s| s.id == *id));
    loops.retain(|(id, _)| ambient.audible().any(|s| s.id == *id));
    // The one-shots follow the current ambient session; its record is read once per session (the slot id is unique
    // per load). A new session starts its class timers at 0 (the first sound plays at once).
    let id = amb.ambient.cur.as_ref().map_or(0, |c| c.id);
    if id != amb.session_id {
        amb.session_id = id;
        amb.shots = [(None, 0.0); 3];
        let rec = amb
            .ambient
            .cur
            .as_ref()
            .and_then(|c| data.db.get(&c.name).map(|r| (r, c.flag)));
        amb.night_sounds = rec.is_some_and(|(r, _)| r.bool("enableNightSounds"));
        amb.shot_classes = rec.map(|(r, flag)| {
            let night = uses_night(r, flag);
            CLASSES.map(|c| ShotClass::read(r, night, c))
        });
    }
}

/// A `SoundDescriptor`'s own `fallOff`: constructor @0x1801b29f0 (+68 = 5.5), which the ambient sessions'
/// one-shots keep.
const DEFAULT_FALLOFF: f32 = 5.5;

/// Where an ambient session's one-shot plays: `sub_180036260`, 1 m from the ears at `rand() % 360` degrees,
/// level with them (`force2d<Class>N` plays it 2D). It stays there as the player walks on.
pub fn ambient_spot(ears: Vec3, roll: u32) -> Vec3 {
    let a = (roll % 360) as f32 * 0.017444445;
    ears + Vec3::new(a.cos(), 0.0, a.sin())
}

/// Frequencies of an ambient session's one-shots (frequent / moderate / unique): the `AmbientSession` updates
/// (Engine.dll sub_180036440, sub_180036690, sub_1800368E0): `rand() % 4` picks 2 and 1 semitones down, 44100 Hz or 1
/// semitone up.
pub const AMBIENT_ONESHOT_HZ: [f32; 4] = [39293.098, 41630.4, 44100.0, 46701.902];

/// The ambient session's one-shots (`sub_1800363A0`): per class, while its sound
/// plays the wait is armed once; when nothing plays the wait counts down, and at 0 the next sound starts (so the first
/// plays at once). The class volume is not scaled by the session fade. A class without sounds never plays; a file that
/// cannot be read waits as if it had played instead of retrying every frame.
pub(super) fn ambient_oneshots(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
    mut amb: ResMut<Ambience>,
    alive: Query<(), With<SoundClass>>,
) {
    let amb = &mut *amb;
    let Some(classes) = amb.shot_classes.as_ref() else {
        return;
    };
    let dt = time.delta_secs() * 1000.0;
    for (class, shot) in classes.iter().zip(&mut amb.shots) {
        if class.paths.is_empty() {
            continue;
        }
        if shot.0.is_some_and(|e| alive.contains(e)) {
            if shot.1 == 0.0 {
                shot.1 = class.delay();
            }
            continue;
        }
        if shot.1 > 0.0 {
            shot.1 -= dt;
            continue;
        }
        let pick = fastrand::usize(..class.paths.len());
        let f = match sounds.file_or_fetch(&data, &class.paths[pick]) {
            Ok(f) => f,
            // Read in the background: the shot plays on a later frame instead of stalling this one.
            Err(true) => continue,
            Err(false) => {
                shot.1 = class.delay();
                continue;
            }
        };
        let speed = f.speed(AMBIENT_ONESHOT_HZ[fastrand::usize(..4)]);
        let place = sounds
            .listener
            .ears
            .filter(|_| f.mono && !class.force2d[pick])
            .map(|ears| Positioned {
                target: None,
                at: ambient_spot(ears, fastrand::u32(..)),
                volume: class.volume,
                falloff: DEFAULT_FALLOFF,
                ears: EarGains::default(),
            });
        let e = start_voice(
            &mut commands,
            &sounds.listener,
            f,
            SoundClass::Ambient,
            class.volume,
            OnEnd::Despawn,
            speed,
            place,
        );
        // Accepted: the wait is armed once it plays.
        *shot = (Some(e), 0.0);
    }
}
