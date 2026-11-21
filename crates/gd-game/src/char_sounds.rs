//! Character sounds and footprints: which sound pak / footprint decal a
//! character's animation callbacks and swipes pick, with a shapeshift
//! form's overrides (the one source of the form is the [`Character`]'s form
//! slot, `Character::form_records`). The renderer plays them
//! (`grimdawn::audio::character_callbacks`).
//!
//! Game.dll: a `Character` keeps two `CharacterSounds` tables, its
//! own (loaded from its record by `Character::Load`) and an override one
//! (+8648, `SetOverrideSounds` @0x18006d650 loads `replacementSounds` into
//! it, an empty path clears it). Both are read by the table loader
//! `sub_18008F6C0`: `footGrass`, `footDirt`, `footSand`, `footSnow`,
//! `footStone`, `footWood`, `footWater`, `footMud`, `loopingRunningSound`,
//! `attackSound`, `swipeSound`, `stunSound`, `criticalHitSound`,
//! `impactSound`, the locked / cooldown / low-health / not-enough-mana UI
//! sounds, `deathSound1..3`, `voxSound`, `specialAttackSound1..4`. Every
//! use takes the override entry when it has one, else the own one, entry
//! by entry ([`sound`]; e.g. `GetSwipeSound` @0x18005c8f0,
//! `PlayAttackSound` @0x18005c960). The chances (`voxSoundChance`,
//! `specialAttackSound1..4Chance`) are the character's own record's.
//! Footprints the same way (`SetOverrideFootsteps` @0x18006d6c0, loader
//! `sub_180274F20`): `footPrintGrass` … `footPrintMud`, each falling back to
//! `footPrint` of the same record (`sub_180274E10`), then the override set's
//! entry before the own one ([`footprint`]).
//!
//! `Character::AnimationCallback` @0x1800599c0 ([`callback`]): the current
//! action sees the callback first (it may consume it); then `voxSound`,
//! `deathSound1..3`, `specialAttackSound1..4` play that sound at the
//! character (`PlayOverridableSound3d` @0x180059930: with a chance > 0 only
//! when `rand() × 0.0030488 + 0.1` (0.1 .. 100) ≤ it; death sounds always);
//! `Character::Load` @0x180054e70 also keeps `genericSound1..4`, `bodyFallSound`
//! and `voiceSound1..3` of its own record in a map by callback name
//! (`LoadAnimationSound` @0x18006e820); a callback found there plays that
//! sound at the character when the same roll ≤ its `<name>Chance`, a chance
//! of 0 never (sub_180025480, no form override: a Familiar's wing flaps);
//! `L Footstep` / `R Footstep` play the foot sound of the surface below
//! (`World::GetSurfaceTypeBelow`, [`crate::collision::WorldCollision::surface_below`],
//! the rules of the terrain and water in [`gd_sim::surface`]; a surface
//! without foot sounds, or none, plays and places nothing), tracked on the
//! character (`SoundPak::Track`), and place that surface's footprint (`GetFootCoords` @0x180059730: the
//! `BN_LToe` / `BN_RToe` bone (none in the stock meshes: the actor's own
//! coords), moved to the floor and 0.05 m up; `World::PlaceDecal`).
//!
//! Swipes, hits and attack sounds: `crate::combat_sounds` (reading
//! [`sound`] for the character's entries).
//!
//! The error lines (`notEnoughManaSound`, `skillCooldownSound`, `itemCooldownSound`, `inventoryFullSound`):
//! [`crate::error_vox`].
//!
//! The low-health warning ([`low_health_vox`], `Character::PlayLowHealthSoundIfNeeded`
//! @0x18005cf30): a latch per character — at life ≤ `lowHealthTriggerLevel`% of max it plays
//! `lowHealthSound` once and re-arms only at life ≥ `lowHealthResetLevel`% (malepc01 / femalepc01
//! 28 / 30; the levels are the own record's, the sound the override table's). The locked-object
//! voices ([`locked_vox`]: `lockedChestSound` / `lockedDoorSound` / `lockedShrineSound` /
//! `lockedQuestObjectSound`, `Player::PlayLockedChestVox` @0x1803e5c40) are picked here;
//! the refusal paths play them.
//!
//! PORT: the action's own callback handling is not consulted; the running sounds are not played.
use crate::character::Character;
use crate::combat::{Combatant, Dead};
use crate::data::GameData;
use crate::visuals::PlaySound;
use bevy::prelude::*;

pub use gd_sim::surface::Surface;

/// The `CharacterSounds` entry `slot` of `c`: the form's `replacementSounds`
/// entry when it has one, else the character record's.
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

/// The footprint decal of `c` on `surface`: the form's
/// `replacementFootsteps` set, else the character record's; in each
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

/// What an animation callback makes the character do ([`callback`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Heard {
    /// A sound pak played at the character.
    pub sound: Option<String>,
    /// A footprint decal (`L Footstep`: the left foot).
    pub footprint: Option<(String, bool)>,
    /// The sound moves with the character (footsteps).
    pub tracked: bool,
}

/// Whether the callback `name` needs the surface below the character.
pub fn is_footstep(name: &str) -> bool {
    matches!(name, "L Footstep" | "R Footstep")
}

/// The engine's sound roll (`rand() × 0.0030488 + 0.1`, 0.1 .. 100).
pub fn roll() -> f32 {
    crate::rng::f32() * 99.9 + 0.1
}

/// `Character::AnimationCallback` for the callback `name` (module doc), the
/// ground being `surface` (none: no foot sound); `roll` is drawn only for a
/// sound with a chance.
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

/// Whether a swipe callback of the animation slot `anim` reaches a weapon
/// skill's `SwipeAction` (`SkillActivatedWeapon` and its kin; the spells' is
/// only their swipe sound, `crate::combat_sounds::swipe_action`): the default
/// attack (the Attack or an `AttTurn*` slot) or a cast of a weapon skill (`cast`, e.g. a form's
/// claws in their special animation).
/// PORT: the cast's kind stands in for the skill class's `SwipeAction`.
pub fn weapon_swipe(anim: gd_sim::anim::AnimType, cast: Option<&crate::casting::Casting>) -> bool {
    gd_sim::anim::is_attack(anim) || cast.is_some_and(|c| c.kind == crate::casting::Kind::Weapon)
}

/// A locked object's refusal voice (`Player::PlayLocked*Vox`, module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockedKind {
    Chest,
    Door,
    Shrine,
    QuestObject,
}

impl LockedKind {
    /// The kind's `CharacterSounds` slot.
    pub fn slot(self) -> &'static str {
        match self {
            LockedKind::Chest => "lockedChestSound",
            LockedKind::Door => "lockedDoorSound",
            LockedKind::Shrine => "lockedShrineSound",
            LockedKind::QuestObject => "lockedQuestObjectSound",
        }
    }
}

/// The locked-object voice pak of `c` for `kind` (override else own, [`sound`]);
/// the chest / door / shrine / quest-object refusal paths play it.
pub fn locked_vox(data: &GameData, c: &Character, kind: LockedKind) -> Option<String> {
    sound(data, c, kind.slot())
}

/// A character's low-health latch with its record's levels (fractions of max life), read once.
#[derive(Component)]
pub struct LowHealth {
    trigger: f32,
    reset: f32,
    latched: bool,
}

/// The low-health warning of the living characters (module doc,
/// `Character::PlayLowHealthSoundIfNeeded` @0x18005cf30): a latch per character
/// — at life ≤ `lowHealthTriggerLevel`% of max it plays `lowHealthSound` at the
/// character once, and re-arms only at life ≥ `lowHealthResetLevel`%. The levels
/// are the character record's (0 when unset: never triggers); the sound is the
/// override table's ([`sound`]). Registered by `CombatSoundsPlugin`. Only characters
/// whose life changed are visited; [`LowHealth`] is added at the first wound.
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
    /// Updates the latch at `life` of `max`; true when it just triggered.
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
