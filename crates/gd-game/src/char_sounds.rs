//! Character sounds and footprints: which sound pak / footprint decal a character's animation
//! callbacks and swipes pick, with a shapeshift form's overrides (`Character::form_records`).
//!
//! A `Character` keeps two `CharacterSounds` tables: its own and an override one
//! (`SetOverrideSounds` loads `replacementSounds`). Every use takes the override entry when it has
//! one, else the own one, entry by entry ([`sound`]). The chances (`voxSoundChance`,
//! `specialAttackSound1..4Chance`) are the own record's. Footprints likewise
//! (`SetOverrideFootsteps`): `footPrint<Surface>`, falling back to `footPrint` of the same record,
//! the override set before the own one ([`footprint`]).
//!
//! `Character::AnimationCallback` ([`callback`]): `voxSound`, `deathSound1..3`,
//! `specialAttackSound1..4` play at the character (with a chance > 0 only when
//! `rand() × 0.0030488 + 0.1` ≤ it; death sounds always); the own record's `genericSound1..4`,
//! `bodyFallSound` and `voiceSound1..3` play when the same roll ≤ its `<name>Chance` (0 never; no
//! form override); `L Footstep` / `R Footstep` play the foot sound of the surface below (none or
//! no foot sounds: nothing), tracked on the character, and place that surface's footprint at the
//! actor's coords on the floor, 0.05 m up (the stock meshes lack `BN_LToe` / `BN_RToe`).
//!
//! Low-health warning ([`low_health_vox`], `Character::PlayLowHealthSoundIfNeeded`): a latch per
//! character — at life ≤ `lowHealthTriggerLevel`% of max it plays `lowHealthSound` once and
//! re-arms at life ≥ `lowHealthResetLevel`% (levels from the own record, sound from the override
//! table). Locked-object voices ([`locked_vox`], `Player::PlayLockedChestVox`) are picked here.
//!
//! PORT: the action's own callback handling is not consulted; the running sounds are not played.
use bevy::prelude::*;
use gd_game_core::character::Character;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::visuals::PlaySound;

pub use gd_sim::surface::Surface;

/// The form's `replacementSounds` entry when it has one, else the character record's.
pub fn sound(data: &GameData, c: &Character, slot: &str) -> Option<String> {
    let entry = |rec: &str| {
        data.db
            .get(rec)
            .and_then(|r| r.str(slot).filter(|s| !s.is_empty()).map(str::to_owned))
    };
    c.form_records()
        .filter(|f| !f.sounds.is_empty())
        .and_then(|f| entry(&f.sounds))
        .or_else(|| entry(&c.record))
}

/// The form's `replacementFootsteps` set, else the character record's; in each
/// `footPrint<Surface>`, else `footPrint`.
pub fn footprint(data: &GameData, c: &Character, surface: Surface) -> Option<String> {
    let key = format!("footPrint{}", surface.name());
    let entry = |rec: &str| {
        let r = data.db.get(rec)?;
        let own = |k: &str| r.str(k).filter(|s| !s.is_empty()).map(str::to_owned);
        own(&key).or_else(|| own("footPrint"))
    };
    c.form_records()
        .filter(|f| !f.footsteps.is_empty())
        .and_then(|f| entry(&f.footsteps))
        .or_else(|| entry(&c.record))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Heard {
    pub sound: Option<String>,
    /// `true`: the left foot.
    pub footprint: Option<(String, bool)>,
    /// Moves with the character (footsteps).
    pub tracked: bool,
}

pub fn is_footstep(name: &str) -> bool {
    matches!(name, "L Footstep" | "R Footstep")
}

/// `rand() × 0.0030488 + 0.1`, 0.1 .. 100.
pub fn roll() -> f32 {
    gd_game_core::rng::f32() * 99.9 + 0.1
}

/// `Character::AnimationCallback`; `surface` none: no foot sound; `roll` is drawn only for a sound
/// with a chance.
pub fn callback(
    data: &GameData,
    c: &Character,
    name: &str,
    surface: impl Into<Option<Surface>>,
    roll: impl FnOnce() -> f32,
) -> Heard {
    let left = match name {
        "L Footstep" => true,
        "R Footstep" => false,
        "deathSound1" | "deathSound2" | "deathSound3" => {
            return Heard {
                sound: sound(data, c, name),
                ..default()
            };
        }
        "genericSound1" | "genericSound2" | "genericSound3" | "genericSound4" | "bodyFallSound"
        | "voiceSound1" | "voiceSound2" | "voiceSound3" => {
            let Some(r) = data.db.get(&c.record) else {
                return Heard::default();
            };
            let Some(pak) = r.str(name).filter(|s| !s.is_empty()) else {
                return Heard::default();
            };
            let chance = r.f32(&format!("{name}Chance")).unwrap_or(0.0);
            return Heard {
                sound: (roll() <= chance).then(|| pak.to_owned()),
                ..default()
            };
        }
        "voxSound"
        | "specialAttackSound1"
        | "specialAttackSound2"
        | "specialAttackSound3"
        | "specialAttackSound4" => {
            let chance = data
                .db
                .get(&c.record)
                .and_then(|r| r.f32(&format!("{name}Chance")))
                .unwrap_or(0.0);
            let s = sound(data, c, name).filter(|_| chance <= 0.0 || roll() <= chance);
            return Heard {
                sound: s,
                ..default()
            };
        }
        _ => return Heard::default(),
    };
    let Some(surface) = surface.into() else {
        return Heard::default();
    };
    let foot = format!("foot{}", surface.name());
    Heard {
        sound: sound(data, c, &foot),
        footprint: footprint(data, c, surface).map(|d| (d, left)),
        tracked: true,
    }
}

/// `Player::PlayLocked*Vox`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockedKind {
    Chest,
    Door,
    Shrine,
    QuestObject,
}

impl LockedKind {
    pub fn slot(self) -> &'static str {
        match self {
            LockedKind::Chest => "lockedChestSound",
            LockedKind::Door => "lockedDoorSound",
            LockedKind::Shrine => "lockedShrineSound",
            LockedKind::QuestObject => "lockedQuestObjectSound",
        }
    }
}

/// Override else own ([`sound`]).
pub fn locked_vox(data: &GameData, c: &Character, kind: LockedKind) -> Option<String> {
    sound(data, c, kind.slot())
}

/// The record's levels as fractions of max life.
#[derive(Component)]
pub struct LowHealth {
    trigger: f32,
    reset: f32,
    latched: bool,
}

/// `Character::PlayLowHealthSoundIfNeeded` for living characters whose life changed (levels 0 when
/// unset: never triggers). [`LowHealth`] is added at the first wound.
#[allow(clippy::type_complexity)]
pub fn low_health_vox(
    mut commands: Commands,
    data: Res<GameData>,
    mut chars: Query<
        (
            Entity,
            &Character,
            &Combatant,
            &GlobalTransform,
            Option<&mut LowHealth>,
        ),
        (Without<Dead>, Changed<Combatant>),
    >,
    mut out: MessageWriter<PlaySound>,
) {
    for (e, c, cb, gt, state) in &mut chars {
        let max = cb.profile.sheet.life;
        if max <= 0.0 || cb.life <= 0.0 {
            continue;
        }
        let warn = match state {
            Some(mut s) => s.step(cb.life, max),
            None if cb.life >= max => continue,
            None => {
                let rec = data.db.get(&c.record);
                let level = |k: &str| rec.and_then(|r| r.f32(k)).unwrap_or(0.0) * 0.01;
                let mut s = LowHealth {
                    trigger: level("lowHealthTriggerLevel"),
                    reset: level("lowHealthResetLevel"),
                    latched: false,
                };
                let warn = s.step(cb.life, max);
                commands.entity(e).try_insert(s);
                warn
            }
        };
        if warn && let Some(pak) = sound(&data, c, "lowHealthSound") {
            out.write(PlaySound {
                pak,
                at: Some(gt.translation()),
            });
        }
    }
}

impl LowHealth {
    fn step(&mut self, life: f32, max: f32) -> bool {
        if self.latched {
            if life >= self.reset * max {
                self.latched = false;
            }
            false
        } else if life <= self.trigger * max {
            self.latched = true;
            true
        } else {
            false
        }
    }
}
